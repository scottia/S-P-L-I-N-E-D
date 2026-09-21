using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;

namespace Splined.WindowsGui
{
    internal enum AlbumState
    {
        New,
        Processed,
        TimeoutActive,
        Bypassed
    }

    internal enum ArtistAggregateState
    {
        Unprocessed,
        Partial,
        Complete,
        ContainsBypass
    }

    internal sealed class AlbumInfo
    {
        public string Artist;
        public string Title;
        public string Path;
        public List<string> AudioFiles = new List<string>();
        public AlbumState State;
        public bool Selected;
        public bool BypassOverride;
        public DateTime? CompletedUtc;
        public DateTime? EligibleUtc;
        public string Outcome;
        public bool HasLocalArtwork;
        public List<string> LocalArtworkFiles = new List<string>();

        public bool EligibleByDefault { get { return State == AlbumState.New; } }
        public bool HasHistory { get { return CompletedUtc.HasValue; } }

        public string ToolTip
        {
            get
            {
                if (State == AlbumState.Bypassed)
                    return "Bypassed. Selecting this album requires a temporary one-run bypass override; bypass history is retained.";
                if (State == AlbumState.TimeoutActive && EligibleUtc.HasValue)
                {
                    TimeSpan remaining = EligibleUtc.Value - DateTime.UtcNow;
                    if (remaining < TimeSpan.Zero) remaining = TimeSpan.Zero;
                    return "Previously processed. Timeout remaining: " + FormatRemaining(remaining)
                        + ". Eligible again: " + EligibleUtc.Value.ToLocalTime().ToString("yyyy-MM-dd h:mm:ss tt zzz", CultureInfo.CurrentCulture) + ".";
                }
                if (State == AlbumState.Processed)
                    return HasHistory
                        ? "Previously processed and currently eligible for manual reprocessing."
                        : "Existing local cover artwork detected (" + String.Join(", ", LocalArtworkFiles.Select(System.IO.Path.GetFileName)) + "). Eligible for manual reprocessing.";
                return "Never processed. Eligible for Select All and artist selection.";
            }
        }

        private static string FormatRemaining(TimeSpan value)
        {
            if (value.TotalDays >= 1) return ((int)value.TotalDays) + "d " + value.Hours + "h " + value.Minutes + "m";
            if (value.TotalHours >= 1) return ((int)value.TotalHours) + "h " + value.Minutes + "m";
            return Math.Max(0, value.Minutes) + "m " + value.Seconds + "s";
        }
    }

    internal static class ArtistStateResolver
    {
        public static ArtistAggregateState Aggregate(IEnumerable<AlbumInfo> albums)
        {
            List<AlbumInfo> values = albums == null ? new List<AlbumInfo>() : albums.ToList();
            if (values.Any(album => album.State == AlbumState.Bypassed))
                return ArtistAggregateState.ContainsBypass;
            if (values.Count == 0 || values.All(album => album.State == AlbumState.New))
                return ArtistAggregateState.Unprocessed;
            if (values.All(album => album.State == AlbumState.Processed || album.State == AlbumState.TimeoutActive))
                return ArtistAggregateState.Complete;
            return ArtistAggregateState.Partial;
        }

        public static Color StateColor(ArtistAggregateState state, bool dark)
        {
            if (state == ArtistAggregateState.ContainsBypass)
                return ThemeManager.StatusColor(ThemeStatusColor.Blue, dark ? "Dark" : "Light");
            if (state == ArtistAggregateState.Complete)
                return ThemeManager.StatusColor(ThemeStatusColor.Green, dark ? "Dark" : "Light");
            if (state == ArtistAggregateState.Partial)
                return ThemeManager.StatusColor(ThemeStatusColor.Purple, dark ? "Dark" : "Light");
            return ThemeManager.StatusColor(ThemeStatusColor.White, dark ? "Dark" : "Light");
        }

        public static string ToolTip(ArtistAggregateState state)
        {
            if (state == ArtistAggregateState.ContainsBypass)
                return "Contains one or more bypassed albums. White albums select normally; bypassed albums require a temporary one-run override.";
            if (state == ArtistAggregateState.Complete)
                return "All albums are processed. Processed albums remain available for manual reprocessing.";
            if (state == ArtistAggregateState.Partial)
                return "Partially processed. Selecting this artist selects only eligible white albums.";
            return "Unprocessed. Selecting this artist selects all eligible white albums.";
        }
    }

    internal static class ArtistSelectionRules
    {
        public static void Apply(IEnumerable<AlbumInfo> albums, bool selected, bool includeBypassed)
        {
            if (albums == null) return;
            foreach (AlbumInfo album in albums)
            {
                if (!selected)
                {
                    album.Selected = false;
                    album.BypassOverride = false;
                    continue;
                }
                if (album.State == AlbumState.New)
                {
                    album.Selected = true;
                    continue;
                }
                if (album.State == AlbumState.Bypassed && includeBypassed)
                {
                    album.Selected = true;
                    album.BypassOverride = true;
                }
                // Processed and timeout-active albums are never auto-selected.
                // Existing manual selections are intentionally left unchanged.
            }
        }
    }

    internal static class LibraryInventory
    {
        private static readonly HashSet<string> AudioExtensions = new HashSet<string>(new[] { ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif" }, StringComparer.OrdinalIgnoreCase);

        public static List<AlbumInfo> Load(ConfigState config)
        {
            List<AlbumInfo> albums = new List<AlbumInfo>();
            if (String.IsNullOrWhiteSpace(config.MusicLibrary) || !Directory.Exists(config.MusicLibrary)) return albums;
            Visit(config.MusicLibrary, config.MusicLibrary, config.IgnoredSubs, config.FileName, true, albums);
            albums.Sort(delegate(AlbumInfo left, AlbumInfo right) { return StringComparer.OrdinalIgnoreCase.Compare(left.Path, right.Path); });
            HistoryResolver.Apply(config, albums);
            return albums;
        }

        private static void Visit(string root, string directory, IList<string> ignored, string configuredFileName, bool isRoot, List<AlbumInfo> albums)
        {
            DirectoryInfo info;
            try { info = new DirectoryInfo(directory); }
            catch { return; }
            if (!isRoot && ignored.Any(pattern => Wildcard(pattern, info.Name))) return;

            FileInfo[] files;
            DirectoryInfo[] children;
            try
            {
                files = info.GetFiles();
                children = info.GetDirectories();
            }
            catch { return; }
            List<string> audio = files.Where(file => AudioExtensions.Contains(file.Extension)).Select(file => file.FullName).OrderBy(path => path, StringComparer.OrdinalIgnoreCase).ToList();
            if (audio.Count > 0)
            {
                List<string> localArtwork = files
                    .Where(file => IsLocalArtwork(file, configuredFileName))
                    .Select(file => file.FullName)
                    .OrderBy(path => path, StringComparer.OrdinalIgnoreCase)
                    .ToList();
                string relative = directory.Substring(root.TrimEnd(Path.DirectorySeparatorChar).Length).TrimStart(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
                string[] parts = relative.Split(new[] { Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar }, StringSplitOptions.RemoveEmptyEntries);
                albums.Add(new AlbumInfo
                {
                    Artist = parts.Length > 1 ? parts[0] : (info.Parent != null ? info.Parent.Name : "Library"),
                    Title = info.Name,
                    Path = info.FullName,
                    AudioFiles = audio,
                    HasLocalArtwork = localArtwork.Count > 0,
                    LocalArtworkFiles = localArtwork,
                    State = AlbumState.New
                });
            }
            foreach (DirectoryInfo child in children.OrderBy(value => value.Name, StringComparer.OrdinalIgnoreCase))
                if ((child.Attributes & FileAttributes.ReparsePoint) == 0) Visit(root, child.FullName, ignored, configuredFileName, false, albums);
        }

        private static bool IsLocalArtwork(FileInfo file, string configuredFileName)
        {
            if (file == null || (file.Attributes & FileAttributes.ReparsePoint) != 0) return false;
            string extension = file.Extension.TrimStart('.');
            if (!new[] { "jpg", "jpeg", "png", "webp" }.Contains(extension, StringComparer.OrdinalIgnoreCase)) return false;
            string stem = Path.GetFileNameWithoutExtension(file.Name);
            string configured = String.IsNullOrWhiteSpace(configuredFileName) ? "cover" : configuredFileName.Trim();
            return stem.StartsWith("cover", StringComparison.OrdinalIgnoreCase)
                || stem.StartsWith(configured, StringComparison.OrdinalIgnoreCase);
        }

        private static bool Wildcard(string pattern, string value)
        {
            if (String.IsNullOrWhiteSpace(pattern)) return false;
            string regex = "^" + Regex.Escape(pattern.Trim()).Replace("\\*", ".*").Replace("\\?", ".") + "$";
            return Regex.IsMatch(value, regex, RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        }
    }

    internal static class HistoryResolver
    {
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
        private const long UnixEpochTicks = 621355968000000000L;

        public static void Apply(ConfigState config, IList<AlbumInfo> albums)
        {
            foreach (AlbumInfo album in albums)
            {
                album.State = album.HasLocalArtwork ? AlbumState.Processed : AlbumState.New;
                album.CompletedUtc = null;
                album.EligibleUtc = null;
                album.Outcome = null;
            }
            if (!config.HistoryEnabled) return;
            string path = Path.Combine(config.HistoryDir, "scan-completed-history.json");
            if (!File.Exists(path)) return;

            Dictionary<string, object> root;
            try { root = Json.Deserialize<Dictionary<string, object>>(File.ReadAllText(path)); }
            catch { return; }
            object rawAlbums;
            if (root == null || !root.TryGetValue("albums", out rawAlbums)) return;
            Dictionary<string, object> entries = rawAlbums as Dictionary<string, object>;
            if (entries == null) return;
            Dictionary<string, object> insensitive = new Dictionary<string, object>(entries, StringComparer.OrdinalIgnoreCase);
            DateTime now = DateTime.UtcNow;
            string policy = PolicyFingerprint(config);

            foreach (AlbumInfo album in albums)
            {
                object rawEntry;
                if (!insensitive.TryGetValue(album.Path, out rawEntry)) continue;
                Dictionary<string, object> entry = rawEntry as Dictionary<string, object>;
                if (entry == null) continue;
                double completedUnix;
                if (!TryDouble(entry, "completed_at_unix", out completedUnix)) continue;
                DateTime completed = UnixToUtc(completedUnix);
                if (config.HistoryRetentionDays > 0 && completed.AddDays(config.HistoryRetentionDays) <= now) continue;
                album.CompletedUtc = completed;
                album.Outcome = ReadText(entry, "outcome");
                if (!String.IsNullOrEmpty(album.Outcome) && album.Outcome.IndexOf("bypass", StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    album.State = AlbumState.Bypassed;
                    continue;
                }
                album.State = AlbumState.Processed;
                if (config.ScanModeTimeout <= 0) continue;
                DateTime eligible = completed.AddHours(config.ScanModeTimeout);
                if (eligible <= now) continue;
                if (!String.Equals(ReadText(entry, "album_fingerprint"), AlbumFingerprint(album), StringComparison.OrdinalIgnoreCase)) continue;
                if (!String.Equals(ReadText(entry, "policy_fingerprint"), policy, StringComparison.OrdinalIgnoreCase)) continue;
                album.State = AlbumState.TimeoutActive;
                album.EligibleUtc = eligible;
            }
        }

        public static Color StateColor(AlbumState state, bool dark)
        {
            string theme = dark ? "Dark" : "Light";
            if (state == AlbumState.Bypassed) return ThemeManager.StatusColor(ThemeStatusColor.Red, theme);
            if (state == AlbumState.TimeoutActive) return ThemeManager.StatusColor(ThemeStatusColor.Purple, theme);
            if (state == AlbumState.Processed) return ThemeManager.StatusColor(ThemeStatusColor.Orange, theme);
            return ThemeManager.StatusColor(ThemeStatusColor.White, theme);
        }

        public static string AlbumFingerprint(AlbumInfo album)
        {
            using (SHA256 sha = SHA256.Create())
            {
                foreach (string path in album.AudioFiles.OrderBy(value => value.ToLowerInvariant(), StringComparer.Ordinal))
                {
                    string payload;
                    try
                    {
                        FileInfo info = new FileInfo(path);
                        long nanos = (info.LastWriteTimeUtc.Ticks - UnixEpochTicks) * 100L;
                        payload = info.Name + "\0" + info.Length.ToString(CultureInfo.InvariantCulture) + "\0" + nanos.ToString(CultureInfo.InvariantCulture) + "\0";
                    }
                    catch { payload = Path.GetFileName(path) + "\0unstatable\0"; }
                    byte[] bytes = Encoding.UTF8.GetBytes(payload);
                    sha.TransformBlock(bytes, 0, bytes.Length, null, 0);
                }
                sha.TransformFinalBlock(new byte[0], 0, 0);
                return Hex(sha.Hash);
            }
        }

        public static string PolicyFingerprint(ConfigState config)
        {
            List<string> sources = config.Sources.Where(source => !config.ExcludedSources.Contains(source, StringComparer.OrdinalIgnoreCase)).ToList();
            string output = "{"
                + "\"evaluate_final_image\":" + Bool(config.EvaluateFinalImage) + ","
                + "\"file_formats\":" + Json.Serialize(config.Formats) + ","
                + "\"file_name\":" + Json.Serialize(config.FileName) + ","
                + "\"preserve_file\":" + Bool(config.PreserveFile) + ","
                + "\"square\":" + Bool(config.Square) + ","
                + "\"square_mode\":" + Json.Serialize(config.SquareMode) + ","
                + "\"square_round_to\":" + config.SquareRoundTo.ToString(CultureInfo.InvariantCulture) + ","
                + "\"upscale_below_ideal\":" + Bool(config.UpscaleBelowIdeal)
                + "}";
            string range = "{\"ideal\":" + config.RangeIdeal + ",\"ladder\":" + config.RangeLadder + ",\"max\":" + config.RangeMax + ",\"min\":" + config.RangeMin + "}";
            string payload = "{"
                + "\"mode\":" + Json.Serialize(config.Mode) + ","
                + "\"output\":" + output + ","
                + "\"range\":" + range + ","
                + "\"samples\":{\"sample_write\":" + Bool(config.SampleWrite) + "},"
                + "\"sources\":" + Json.Serialize(sources)
                + "}";
            using (SHA256 sha = SHA256.Create()) return Hex(sha.ComputeHash(Encoding.UTF8.GetBytes(payload)));
        }

        private static string Bool(bool value) { return value ? "true" : "false"; }

        private static string ReadText(Dictionary<string, object> values, string key)
        {
            object value;
            return values.TryGetValue(key, out value) && value != null ? Convert.ToString(value, CultureInfo.InvariantCulture) : "";
        }

        private static bool TryDouble(Dictionary<string, object> values, string key, out double value)
        {
            object raw;
            value = 0;
            return values.TryGetValue(key, out raw) && raw != null && Double.TryParse(Convert.ToString(raw, CultureInfo.InvariantCulture), NumberStyles.Float, CultureInfo.InvariantCulture, out value);
        }

        private static DateTime UnixToUtc(double seconds)
        {
            return new DateTime(UnixEpochTicks, DateTimeKind.Utc).AddSeconds(seconds);
        }

        private static string Hex(byte[] bytes)
        {
            StringBuilder text = new StringBuilder(bytes.Length * 2);
            foreach (byte value in bytes) text.Append(value.ToString("x2", CultureInfo.InvariantCulture));
            return text.ToString();
        }
    }
}
