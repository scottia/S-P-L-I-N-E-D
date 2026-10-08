using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Diagnostics;
using System.Text;
using System.Web.Script.Serialization;

namespace Splined.WindowsGui
{
    internal enum AlbumState
    {
        New,
        Incomplete,
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
        public string Key;
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
        public bool Compilation;
        public bool CompilationTrackArtworkEligible;
        public List<CompilationTrackInfo> CompilationTracks = new List<CompilationTrackInfo>();
        public bool CompilationTracksLoaded;
        public bool HasLocalArtwork;
        public List<string> LocalArtworkFiles = new List<string>();
        public int TrackCount;
        public string ReleaseYear;
        public string CoverPath;
        public string CoverName;
        public string CoverFormat;
        public int CoverWidth;
        public int CoverHeight;
        public int RootFiles;
        public int CoverFiles;

        public bool CompilationTrackArtworkPending
        {
            get
            {
                return Compilation && CompilationTrackArtworkEligible
                    && !String.Equals(Outcome, "embedded-compilation", StringComparison.OrdinalIgnoreCase);
            }
        }
        public bool EligibleByDefault
        {
            get
            {
                return State == AlbumState.New || State == AlbumState.Incomplete
                    || (CompilationTrackArtworkPending && State != AlbumState.TimeoutActive);
            }
        }
        public bool HasHistory { get { return CompletedUtc.HasValue; } }

        public string ToolTip
        {
            get
            {
                if (CompilationTrackArtworkPending && State == AlbumState.Bypassed)
                    return "Compilation track artwork is still pending. Prior Album-level bypass history does not block the separate embedded-art workflow.";
                if (CompilationTrackArtworkPending && State == AlbumState.Processed)
                    return "Album-level artwork was processed, but embedded artwork is still pending for this compilation's tracks.";
                if (State == AlbumState.Bypassed)
                    return "Bypassed. Selecting this album requires a temporary one-run bypass override; bypass history is retained.";
                if (State == AlbumState.Incomplete)
                    return "Started but incomplete. Select this album to resume processing from its persisted SQLite progress.";
                if (State == AlbumState.TimeoutActive && EligibleUtc.HasValue)
                {
                    TimeSpan remaining = EligibleUtc.Value - DateTime.UtcNow;
                    if (remaining < TimeSpan.Zero) remaining = TimeSpan.Zero;
                    return "Previously processed. Timeout remaining: " + FormatRemaining(remaining)
                        + ". Eligible again: " + EligibleUtc.Value.ToLocalTime().ToString("yyyy-MM-dd h:mm:ss tt zzz", CultureInfo.CurrentCulture) + ".";
                }
                if (State == AlbumState.Processed)
                    return "Previously processed by SPLINED and currently eligible for manual reprocessing.";
                if (HasLocalArtwork)
                    return "Never processed by SPLINED. Existing local cover artwork detected ("
                        + String.Join(", ", LocalArtworkFiles.Select(System.IO.Path.GetFileName))
                        + "); eligible for Select All and artist selection.";
                return "Never processed by SPLINED. Eligible for Select All and artist selection.";
            }
        }

        private static string FormatRemaining(TimeSpan value)
        {
            if (value.TotalDays >= 1) return ((int)value.TotalDays) + "d " + value.Hours + "h " + value.Minutes + "m";
            if (value.TotalHours >= 1) return ((int)value.TotalHours) + "h " + value.Minutes + "m";
            return Math.Max(0, value.Minutes) + "m " + value.Seconds + "s";
        }
    }

    internal sealed class CompilationTrackInfo
    {
        public AlbumInfo Album;
        public string Path;
        public string Title;
        public bool EmbeddedArtworkRecorded;

        public string FileName
        {
            get { return System.IO.Path.GetFileName(Path ?? "") ?? ""; }
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
                if (album.EligibleByDefault)
                {
                    album.Selected = true;
                    album.BypassOverride = album.State == AlbumState.Bypassed
                        && album.CompilationTrackArtworkPending;
                    continue;
                }
                if (album.State == AlbumState.Bypassed && includeBypassed)
                {
                    album.Selected = true;
                    album.BypassOverride = true;
                }
                // Ordinary processed and timeout-active albums are never auto-selected.
                // A pending compilation is separate per-track work and remains eligible
                // until the embedded-compilation completion authority is recorded.
                // Existing manual selections are intentionally left unchanged.
            }
        }
    }

    internal static class LibraryInventory
    {
        private static readonly JavaScriptSerializer Json = CreateSnapshotSerializer();

        private static JavaScriptSerializer CreateSnapshotSerializer()
        {
            JavaScriptSerializer serializer = new JavaScriptSerializer();
            // A several-thousand-Album SQLite projection legitimately exceeds
            // JavaScriptSerializer's 2 MB default. The Rust core is the trusted
            // local producer, and the GUI still deserializes only its DTO shape.
            serializer.MaxJsonLength = Int32.MaxValue;
            return serializer;
        }

#pragma warning disable 0649 // Populated reflectively by JavaScriptSerializer.
        private sealed class SnapshotPayload
        {
            public int schema_version;
            public string database_path;
            public string canonical_root;
            public string local_root;
            public SnapshotAlbum[] albums;
        }

        private sealed class SnapshotAlbum
        {
            public string album_key;
            public string artist;
            public string tagged_artist;
            public string title;
            public string path;
            public string representative_file;
            public string status;
            public bool compilation;
            public bool compilation_track_art_eligible;
            public SnapshotTrack[] compilation_tracks;
            public int track_count;
            public string release_year;
            public bool has_local_artwork;
            public string[] local_artwork_files;
            public string cover_path;
            public string cover_name;
            public string cover_format;
            public int cover_width;
            public int cover_height;
            public int root_files;
            public int cover_files;
            public string processed_at;
            public string timeout_until;
            public string selected_source;
        }

        private sealed class SnapshotTrack
        {
            public string path;
            public string title;
            public bool embedded_artwork_recorded;
        }
#pragma warning restore 0649

        public static List<AlbumInfo> Load(ConfigState config, bool refresh)
        {
            if (!NativeCore.IsAvailable)
                throw new InvalidOperationException("The fixed SPLINED Rust core DLL was not found for the SQLite media snapshot.");
            Stopwatch timing = Stopwatch.StartNew();
            RuntimeLog.Write("debug", "media_snapshot.start refresh=" + refresh);
            string output = NativeCore.MediaSnapshot(config, refresh);
            if (String.IsNullOrWhiteSpace(output))
                throw new InvalidOperationException("SPLINED returned no SQLite media snapshot.");
            List<AlbumInfo> albums = FromSnapshotJson(output);
            timing.Stop();
            RuntimeLog.Write("debug", "media_snapshot.timing refresh=" + refresh
                + " elapsed_ms=" + timing.ElapsedMilliseconds
                + " albums=" + albums.Count);
            return albums;
        }

        internal static List<AlbumInfo> FromSnapshotJson(string text)
        {
            SnapshotPayload snapshot = Json.Deserialize<SnapshotPayload>(text);
            if (snapshot == null || snapshot.schema_version != 2)
                throw new InvalidOperationException("SPLINED returned an incompatible SQLite media snapshot.");
            return (snapshot.albums ?? new SnapshotAlbum[0]).Select(item =>
            {
                AlbumInfo album = new AlbumInfo
                {
                    Key = item.album_key,
                    Artist = item.artist,
                    Title = item.title,
                    Path = item.path,
                    AudioFiles = String.IsNullOrWhiteSpace(item.representative_file)
                        ? new List<string>()
                        : new List<string> { item.representative_file },
                    HasLocalArtwork = item.has_local_artwork,
                    LocalArtworkFiles = (item.local_artwork_files ?? new string[0]).ToList(),
                    TrackCount = item.track_count,
                    Compilation = item.compilation,
                    CompilationTrackArtworkEligible = item.compilation_track_art_eligible,
                    ReleaseYear = item.release_year,
                    CoverPath = item.cover_path,
                    CoverName = item.cover_name,
                    CoverFormat = item.cover_format,
                    CoverWidth = item.cover_width,
                    CoverHeight = item.cover_height,
                    RootFiles = item.root_files,
                    CoverFiles = item.cover_files,
                    State = ParseStatus(item.status),
                    CompletedUtc = ParseTimestamp(item.processed_at),
                    EligibleUtc = ParseTimestamp(item.timeout_until),
                    Outcome = item.selected_source
                };
                album.CompilationTracks = (item.compilation_tracks ?? new SnapshotTrack[0])
                    .Select(track => new CompilationTrackInfo
                    {
                        Album = album,
                        Path = track.path,
                        Title = track.title,
                        EmbeddedArtworkRecorded = track.embedded_artwork_recorded
                    })
                    .OrderBy(track => track.FileName, StringComparer.OrdinalIgnoreCase)
                    .ToList();
                album.CompilationTracksLoaded = album.TrackCount > 0
                    && album.CompilationTracks.Count >= album.TrackCount;
                return album;
            }).OrderBy(album => album.Path, StringComparer.OrdinalIgnoreCase).ToList();
        }

        private static AlbumState ParseStatus(string status)
        {
            if (String.Equals(status, "incomplete", StringComparison.OrdinalIgnoreCase)) return AlbumState.Incomplete;
            if (String.Equals(status, "processed", StringComparison.OrdinalIgnoreCase)) return AlbumState.Processed;
            if (String.Equals(status, "timeout", StringComparison.OrdinalIgnoreCase)) return AlbumState.TimeoutActive;
            if (String.Equals(status, "bypassed", StringComparison.OrdinalIgnoreCase)) return AlbumState.Bypassed;
            return AlbumState.New;
        }

        private static DateTime? ParseTimestamp(string value)
        {
            if (String.IsNullOrWhiteSpace(value)) return null;
            double seconds;
            if (Double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out seconds))
                return new DateTime(621355968000000000L, DateTimeKind.Utc).AddSeconds(seconds);
            DateTime parsed;
            return DateTime.TryParse(value, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out parsed)
                ? parsed
                : (DateTime?)null;
        }

        private static string Quote(string value) { return "\"" + (value ?? "").Replace("\"", "\\\"") + "\""; }
    }

    internal static class AlbumStatePresentation
    {
        public static Color StateColor(AlbumState state, bool dark)
        {
            string theme = dark ? "Dark" : "Light";
            if (state == AlbumState.Incomplete) return ThemeManager.StatusColor(ThemeStatusColor.Blue, theme);
            if (state == AlbumState.Bypassed) return ThemeManager.StatusColor(ThemeStatusColor.Red, theme);
            if (state == AlbumState.TimeoutActive) return ThemeManager.StatusColor(ThemeStatusColor.Purple, theme);
            if (state == AlbumState.Processed) return ThemeManager.StatusColor(ThemeStatusColor.Orange, theme);
            return ThemeManager.StatusColor(ThemeStatusColor.White, theme);
        }

    }
}
