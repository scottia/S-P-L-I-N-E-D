using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;

namespace Splined.WindowsGui
{
    internal sealed class SourcePolicyState
    {
        public bool Enabled = true;
        public bool SourceOverride;
        public string MinimumRangeType = "LowerRange";
        public bool AllowBelowMinimumFallback;
        public int? MinimumShortSide;
        public int? MaximumShortSide;
        public int? MinimumWidth;
        public int? MinimumHeight;
        public bool PrimaryImageOnly = true;

        public SourcePolicyState Clone()
        {
            return (SourcePolicyState)MemberwiseClone();
        }
    }

    internal enum SourcePolicyResult
    {
        Accept,
        Fallback,
        Reject
    }

    internal sealed class SourcePolicyDecision
    {
        public SourcePolicyResult Result;
        public string RangeType;
        public int ShortSide;
        public string Reason;
    }

    internal static class SourcePolicyRules
    {
        public static readonly string[] RangeTypes = new[]
        {
            "BelowMinimum", "LowerRange", "Ideal", "UpperRange", "Ladder", "AboveLadder"
        };

        public static SourcePolicyDecision Evaluate(ConfigState state, SourcePolicyState policy, int width, int height)
        {
            int shortSide = Math.Min(Math.Max(0, width), Math.Max(0, height));
            string rangeType = Classify(state, shortSide);

            if (!policy.SourceOverride)
            {
                bool accepted = rangeType != "BelowMinimum" && rangeType != "AboveLadder";
                return new SourcePolicyDecision
                {
                    Result = accepted ? SourcePolicyResult.Accept : SourcePolicyResult.Reject,
                    RangeType = rangeType,
                    ShortSide = shortSide,
                    Reason = accepted
                        ? "Accepted by the global artwork range"
                        : rangeType == "BelowMinimum"
                            ? "Short side is below the global minimum of " + state.RangeMin + " px"
                            : "Short side is above the global ladder of " + state.RangeLadder + " px"
                };
            }

            if (policy.MinimumShortSide.HasValue && shortSide < policy.MinimumShortSide.Value)
                return Reject(rangeType, shortSide, "Short side is below the source minimum of " + policy.MinimumShortSide.Value + " px");
            if (policy.MaximumShortSide.HasValue && shortSide > policy.MaximumShortSide.Value)
                return Reject(rangeType, shortSide, "Short side is above the source maximum of " + policy.MaximumShortSide.Value + " px");
            if (policy.MinimumWidth.HasValue && width < policy.MinimumWidth.Value)
                return Reject(rangeType, shortSide, "Width is below the source minimum of " + policy.MinimumWidth.Value + " px");
            if (policy.MinimumHeight.HasValue && height < policy.MinimumHeight.Value)
                return Reject(rangeType, shortSide, "Height is below the source minimum of " + policy.MinimumHeight.Value + " px");

            if (RangeRank(rangeType) < RangeRank(policy.MinimumRangeType))
            {
                string fallbackRange = AdjacentFallbackRange(policy.MinimumRangeType);
                if (policy.AllowBelowMinimumFallback && String.Equals(rangeType, fallbackRange, StringComparison.OrdinalIgnoreCase))
                {
                    return new SourcePolicyDecision
                    {
                        Result = SourcePolicyResult.Fallback,
                        RangeType = rangeType,
                        ShortSide = shortSide,
                        Reason = rangeType + " is the single fallback level below source minimum " + policy.MinimumRangeType
                    };
                }
                return Reject(rangeType, shortSide, "Candidate range is below source minimum " + policy.MinimumRangeType);
            }

            return new SourcePolicyDecision
            {
                Result = SourcePolicyResult.Accept,
                RangeType = rangeType,
                ShortSide = shortSide,
                Reason = "Candidate range meets source minimum " + policy.MinimumRangeType
            };
        }

        public static string Classify(ConfigState state, int shortSide)
        {
            if (shortSide < state.RangeMin) return "BelowMinimum";
            if (shortSide < state.RangeIdeal) return "LowerRange";
            if (shortSide == state.RangeIdeal) return "Ideal";
            if (shortSide <= state.RangeMax) return "UpperRange";
            if (shortSide <= state.RangeLadder) return "Ladder";
            return "AboveLadder";
        }

        public static int DerivedMinimum(ConfigState state, string rangeType)
        {
            switch (rangeType)
            {
                case "BelowMinimum": return 0;
                case "Ideal": return state.RangeIdeal;
                case "UpperRange": return state.RangeIdeal + 1;
                case "Ladder": return state.RangeMax + 1;
                case "AboveLadder": return state.RangeLadder + 1;
                default: return state.RangeMin;
            }
        }

        public static int RangeRank(string rangeType)
        {
            for (int index = 0; index < RangeTypes.Length; index++)
                if (RangeTypes[index].Equals(rangeType, StringComparison.OrdinalIgnoreCase)) return index;
            return 1;
        }

        public static string AdjacentFallbackRange(string minimumRangeType)
        {
            int rank = RangeRank(minimumRangeType);
            return rank > 0 ? RangeTypes[rank - 1] : null;
        }

        private static SourcePolicyDecision Reject(string rangeType, int shortSide, string reason)
        {
            return new SourcePolicyDecision
            {
                Result = SourcePolicyResult.Reject,
                RangeType = rangeType,
                ShortSide = shortSide,
                Reason = reason
            };
        }
    }

    internal sealed class ConfigState
    {
        // Keep the native GUI default in the same priority order as the
        // authoritative Python help/config. Existing Config v5 files retain
        // their explicitly saved order.
        public static readonly string[] ArtworkSources = new[] { "deezer", "itunes", "fanarttv", "lastfm", "coverartarchive", "discogs" };
        public static readonly string[] KnownSources = new[] { "deezer", "itunes", "fanarttv", "lastfm", "coverartarchive", "discogs", "musicbrainz" };
        public string ConfigPath;
        public string Mode = "read";
        public string Verbosity = "info";
        public string MusicLibrary = "";
        public List<string> IgnoredSubs = new List<string>();
        public string ScanLibraryDir = "";
        public bool ScanMode = true;
        public bool LibraryScan;
        public double ScanModeTimeout = 24;
        public string CacheDir;
        public string LogDir;
        public string HistoryDir;
        public string CredentialDir;
        public List<string> Formats = new List<string>(new[] { "jpeg", "png", "webp" });
        public List<string> Sources = new List<string>(ArtworkSources);
        public List<string> ExcludedSources = new List<string>();
        public Dictionary<string, SourcePolicyState> SourcePolicies = new Dictionary<string, SourcePolicyState>(StringComparer.OrdinalIgnoreCase);
        public int LogRetentionDays = 14;
        public int HistoryRetentionDays;
        public bool HistoryEnabled = true;
        public bool SampleWrite = true;
        public string FileName = "cover";
        public bool PreserveFile = true;
        public bool Square = true;
        public string SquareMode = "crop";
        public int SquareRoundTo = 16;
        public bool UpscaleBelowIdeal;
        public bool EvaluateFinalImage = true;
        public int RangeMin = 1200;
        public int RangeIdeal = 1800;
        public int RangeMax = 2400;
        public int RangeLadder = 3600;

        // Config v5 reserves SPLINEAI internally. It is deliberately not shown
        // by the Windows GUI, but these values are retained during every save.
        public bool SplineAiEnabled;
        public string SplineAiEndpoint = "";

        public ConfigState Clone()
        {
            ConfigState copy = (ConfigState)MemberwiseClone();
            copy.IgnoredSubs = new List<string>(IgnoredSubs);
            copy.Formats = new List<string>(Formats);
            copy.Sources = new List<string>(Sources);
            copy.ExcludedSources = new List<string>(ExcludedSources);
            copy.SourcePolicies = SourcePolicies.ToDictionary(pair => pair.Key, pair => pair.Value.Clone(), StringComparer.OrdinalIgnoreCase);
            return copy;
        }
    }

    internal sealed class UiState
    {
        public string Theme = "System";
        public bool ShowStatusOnLaunch = true;
        public bool ShowConfirmations = true;
        public bool HoverEnabled;
        public bool MediaFilterExpanded = true;
        public string MediaArtistFilter = "";
        public string MediaAlbumFilter = "";
        public bool MediaShowWhite = true;
        public bool MediaShowOrange = true;
        public bool MediaShowRed = true;
        public bool MediaShowPurple = true;
        public bool MediaShowGreen = true;
        public bool MediaShowBlue = true;
        public string FilteredScanMode = "";
        public List<string> SelectedAlbumPaths = new List<string>();
        public int MainWidth = 1280;
        public int MainHeight = 840;
        public int MainX = -1;
        public int MainY = -1;
        public bool MainMaximized;
        public int MainSplitterDistance = 430;
        public int RightSplitterDistance = 285;
        public int SetupWidth = 980;
        public int SetupHeight = 790;
        public int SetupX = -1;
        public int SetupY = -1;
        public bool SetupMaximized;
        public int SetupPrimaryTab;
        public int SetupAdvancedTab;
        public int CompareWidth = 980;
        public int CompareHeight = 620;
        public int PreviewWidth = 520;
        public int PreviewHeight = 560;
    }

    internal static class ConfigStore
    {
        public static readonly string AppRoot = ResolveAppRoot();
        public static readonly string DefaultConfigPath = Path.Combine(AppRoot, "config", "config.toml");
        public static readonly string LocatorPath = Path.Combine(AppRoot, "config.location");
        public static readonly string UiPath = Path.Combine(AppRoot, "config", "ui.toml");

        private static string ResolveAppRoot()
        {
            string configured = Environment.GetEnvironmentVariable("SPLINED_HOME");
            return Path.GetFullPath(String.IsNullOrWhiteSpace(configured)
                ? AppDomain.CurrentDomain.BaseDirectory
                : configured);
        }

        public static string GetConfigPath()
        {
            try
            {
                if (File.Exists(LocatorPath))
                {
                    string value = File.ReadAllText(LocatorPath).Trim();
                    if (value.Length > 0)
                        return ResolvePortablePath(value);
                }
            }
            catch { }
            return DefaultConfigPath;
        }

        public static ConfigState Load()
        {
            ConfigState state = Defaults();
            state.ConfigPath = GetConfigPath();
            if (!File.Exists(state.ConfigPath))
                return state;

            string text = File.ReadAllText(state.ConfigPath);
            int version = ReadInt(text, "", "config_version", 0);
            if (version != 5)
                throw new InvalidOperationException("Unsupported SPLINED configuration version " + version + "; expected Config v5.");

            state.Mode = ReadString(text, "", "mode", "read");
            state.Verbosity = ReadString(text, "", "verbosity", "info");
            state.MusicLibrary = ResolveOptionalPath(ReadString(text, "library", "music_library", ""));
            state.IgnoredSubs = ReadArray(text, "library", "ignored_subs");
            state.ScanLibraryDir = ResolveOptionalPath(ReadString(text, "scan", "scan_library_dir", ""));
            state.ScanMode = ReadBool(text, "scan", "scan_mode", true);
            state.LibraryScan = ReadBool(text, "scan", "library_scan", false);
            state.ScanModeTimeout = ReadDoubleOrOff(text, "scan", "scan_mode_timeout", 24);
            state.CacheDir = ResolvePortablePath(ReadString(text, "scan", "cache_dir", "_cache"));
            state.LogDir = ResolvePortablePath(ReadString(text, "scan", "log_dir", "_logs"));
            state.HistoryDir = ResolvePortablePath(ReadString(text, "scan", "history_dir", "_logs/_history"));
            state.CredentialDir = ResolvePortablePath(ReadString(text, "credentials", "credential_dir", "credentials"));
            state.Formats = ReadArray(text, "output", "file_formats");
            if (state.Formats.Count == 0) state.Formats.AddRange(new[] { "jpeg", "png", "webp" });
            state.Sources = ReadArray(text, "sources", "cover_sources");
            if (state.Sources.Count == 0) state.Sources.AddRange(ConfigState.ArtworkSources);
            state.ExcludedSources = ReadArray(text, "sources", "exclude_cover_sources");
            foreach (string source in ConfigState.KnownSources)
            {
                string section = "source_policies." + source;
                SourcePolicyState policy = new SourcePolicyState();
                bool defaultEnabled = source.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase)
                    || (state.Sources.Contains(source, StringComparer.OrdinalIgnoreCase)
                        && !state.ExcludedSources.Contains(source, StringComparer.OrdinalIgnoreCase));
                policy.Enabled = ReadBool(text, section, "enabled", defaultEnabled);
                policy.SourceOverride = ReadBool(text, section, "source_override", false);
                policy.MinimumRangeType = ReadString(text, section, "minimum_range_type", "LowerRange");
                if (!SourcePolicyRules.RangeTypes.Contains(policy.MinimumRangeType, StringComparer.OrdinalIgnoreCase))
                    policy.MinimumRangeType = "LowerRange";
                policy.AllowBelowMinimumFallback = ReadBool(text, section, "allow_below_minimum_fallback", false);
                policy.MinimumShortSide = ReadNullableInt(text, section, "minimum_short_side");
                policy.MaximumShortSide = ReadNullableInt(text, section, "maximum_short_side");
                policy.MinimumWidth = ReadNullableInt(text, section, "minimum_width");
                policy.MinimumHeight = ReadNullableInt(text, section, "minimum_height");
                policy.PrimaryImageOnly = ReadBool(text, section, "primary_image_only", true);
                state.SourcePolicies[source] = policy;
            }
            state.LogRetentionDays = ReadInt(text, "logging", "retention_days", 14);
            state.HistoryRetentionDays = ReadInt(text, "history", "retention_days", 0);
            state.HistoryEnabled = ReadBool(text, "history", "enabled", true);
            state.SampleWrite = ReadBool(text, "samples", "sample_write", true);
            state.FileName = ReadString(text, "output", "file_name", "cover");
            state.PreserveFile = ReadBool(text, "output", "preserve_file", true);
            state.Square = ReadBool(text, "output", "square", true);
            state.SquareMode = ReadString(text, "output", "square_mode", "crop");
            state.SquareRoundTo = ReadInt(text, "output", "square_round_to", 16);
            state.UpscaleBelowIdeal = ReadBool(text, "output", "upscale_below_ideal", false);
            state.EvaluateFinalImage = ReadBool(text, "output", "evaluate_final_image", true);
            state.RangeMin = ReadInt(text, "range", "min", 1200);
            state.RangeIdeal = ReadInt(text, "range", "ideal", 1800);
            state.RangeMax = ReadInt(text, "range", "max", 2400);
            state.RangeLadder = ReadInt(text, "range", "ladder", 3600);
            state.SplineAiEnabled = ReadBool(text, "splineai", "enabled", false);
            state.SplineAiEndpoint = ReadString(text, "splineai", "endpoint", "");
            return state;
        }

        public static ConfigState Defaults()
        {
            ConfigState state = new ConfigState();
            state.ConfigPath = DefaultConfigPath;
            state.CacheDir = Path.Combine(AppRoot, "_cache");
            state.LogDir = Path.Combine(AppRoot, "_logs");
            state.HistoryDir = Path.Combine(AppRoot, "_logs", "_history");
            state.CredentialDir = Path.Combine(AppRoot, "credentials");
            return state;
        }

        public static void Save(ConfigState state)
        {
            Validate(state);
            WriteTextAtomic(state.ConfigPath, BuildConfigText(state));
            WriteTextAtomic(LocatorPath, ToPortablePath(state.ConfigPath) + Environment.NewLine);
        }

        public static void SaveTemporaryRunConfig(ConfigState state, string path)
        {
            if (String.IsNullOrWhiteSpace(path))
                throw new InvalidOperationException("A temporary run configuration path is required.");
            ConfigState snapshot = state.Clone();
            snapshot.ConfigPath = path;
            Validate(snapshot);
            WriteTextAtomic(path, BuildConfigText(snapshot));
        }

        public static void Validate(ConfigState state)
        {
            if (state.RangeMin >= state.RangeIdeal)
                throw new InvalidOperationException("Minimum artwork resolution must be below Ideal.");
            if (state.RangeIdeal > state.RangeMax)
                throw new InvalidOperationException("Ideal artwork resolution cannot exceed Maximum.");
            if (state.RangeMax >= state.RangeLadder)
                throw new InvalidOperationException("Maximum artwork resolution must be below Ladder.");
            if (state.Formats.Count == 0)
                throw new InvalidOperationException("At least one output format must remain enabled.");
            if (state.FileName.Trim().Length == 0 || state.FileName.IndexOfAny(new[] { '\\', '/' }) >= 0 || Path.HasExtension(state.FileName))
                throw new InvalidOperationException("Output file name must be a filename stem without an extension or directory.");
            if (state.ScanModeTimeout < 0)
                throw new InvalidOperationException("Scan timeout cannot be negative.");
            foreach (KeyValuePair<string, SourcePolicyState> pair in state.SourcePolicies)
            {
                SourcePolicyState policy = pair.Value;
                foreach (int? value in new[] { policy.MinimumShortSide, policy.MaximumShortSide, policy.MinimumWidth, policy.MinimumHeight })
                    if (value.HasValue && value.Value <= 0)
                        throw new InvalidOperationException("Source policy dimensions must be greater than zero when entered (" + pair.Key + ").");
                if (policy.MinimumShortSide.HasValue && policy.MaximumShortSide.HasValue
                    && policy.MinimumShortSide.Value > policy.MaximumShortSide.Value)
                    throw new InvalidOperationException("Source minimum short side cannot exceed maximum short side (" + pair.Key + ").");
            }
        }

        public static UiState LoadUi()
        {
            UiState state = new UiState();
            try
            {
                if (!File.Exists(UiPath)) return state;
                string text = File.ReadAllText(UiPath);
                state.Theme = ReadString(text, "ui", "theme", "System");
                state.ShowStatusOnLaunch = ReadBool(text, "ui", "show_status_on_launch", true);
                state.ShowConfirmations = ReadBool(text, "ui", "show_confirmations", true);
                state.HoverEnabled = ReadBool(text, "ui", "hover_enabled", false);
                state.MediaFilterExpanded = ReadBool(text, "ui", "media_filter_expanded", true);
                state.MediaArtistFilter = ReadString(text, "ui", "media_artist_filter", "");
                state.MediaAlbumFilter = ReadString(text, "ui", "media_album_filter", "");
                state.MediaShowWhite = ReadBool(text, "ui", "media_show_white", true);
                state.MediaShowOrange = ReadBool(text, "ui", "media_show_orange", true);
                state.MediaShowRed = ReadBool(text, "ui", "media_show_red", true);
                state.MediaShowPurple = ReadBool(text, "ui", "media_show_purple", true);
                state.MediaShowGreen = ReadBool(text, "ui", "media_show_green", true);
                state.MediaShowBlue = ReadBool(text, "ui", "media_show_blue", true);
                state.FilteredScanMode = ReadString(text, "ui", "filtered_scan_mode", "");
                state.SelectedAlbumPaths = ReadArray(text, "ui", "selected_album_paths");
                state.MainWidth = ReadInt(text, "ui", "main_width", 1280);
                state.MainHeight = ReadInt(text, "ui", "main_height", 840);
                state.MainX = ReadInt(text, "ui", "main_x", -1);
                state.MainY = ReadInt(text, "ui", "main_y", -1);
                state.MainMaximized = ReadBool(text, "ui", "main_maximized", false);
                state.MainSplitterDistance = ReadInt(text, "ui", "main_splitter_distance", 430);
                state.RightSplitterDistance = ReadInt(text, "ui", "right_splitter_distance", 285);
                state.SetupWidth = ReadInt(text, "ui", "setup_width", 980);
                state.SetupHeight = ReadInt(text, "ui", "setup_height", 790);
                state.SetupX = ReadInt(text, "ui", "setup_x", -1);
                state.SetupY = ReadInt(text, "ui", "setup_y", -1);
                state.SetupMaximized = ReadBool(text, "ui", "setup_maximized", false);
                state.SetupPrimaryTab = ReadInt(text, "ui", "setup_primary_tab", 0);
                state.SetupAdvancedTab = ReadInt(text, "ui", "setup_advanced_tab", 0);
                state.CompareWidth = ReadInt(text, "ui", "compare_width", 980);
                state.CompareHeight = ReadInt(text, "ui", "compare_height", 620);
                state.PreviewWidth = ReadInt(text, "ui", "preview_width", 520);
                state.PreviewHeight = ReadInt(text, "ui", "preview_height", 560);
            }
            catch { }
            return state;
        }

        public static void SaveUi(UiState state)
        {
            string text = "[ui]" + Environment.NewLine
                + "show_status_on_launch = " + Bool(state.ShowStatusOnLaunch) + Environment.NewLine
                + "show_confirmations = " + Bool(state.ShowConfirmations) + Environment.NewLine
                + "hover_enabled = " + Bool(state.HoverEnabled) + Environment.NewLine
                + "theme = " + Quote(state.Theme) + Environment.NewLine
                + "media_filter_expanded = " + Bool(state.MediaFilterExpanded) + Environment.NewLine
                + "media_artist_filter = " + Quote(state.MediaArtistFilter) + Environment.NewLine
                + "media_album_filter = " + Quote(state.MediaAlbumFilter) + Environment.NewLine
                + "media_show_white = " + Bool(state.MediaShowWhite) + Environment.NewLine
                + "media_show_orange = " + Bool(state.MediaShowOrange) + Environment.NewLine
                + "media_show_red = " + Bool(state.MediaShowRed) + Environment.NewLine
                + "media_show_purple = " + Bool(state.MediaShowPurple) + Environment.NewLine
                + "media_show_green = " + Bool(state.MediaShowGreen) + Environment.NewLine
                + "media_show_blue = " + Bool(state.MediaShowBlue) + Environment.NewLine
                + "filtered_scan_mode = " + Quote(state.FilteredScanMode) + Environment.NewLine
                + "selected_album_paths = " + FormatArray(state.SelectedAlbumPaths) + Environment.NewLine
                + "main_width = " + Math.Max(940, state.MainWidth) + Environment.NewLine
                + "main_height = " + Math.Max(640, state.MainHeight) + Environment.NewLine
                + "main_x = " + state.MainX + Environment.NewLine
                + "main_y = " + state.MainY + Environment.NewLine
                + "main_maximized = " + Bool(state.MainMaximized) + Environment.NewLine
                + "main_splitter_distance = " + Math.Max(280, state.MainSplitterDistance) + Environment.NewLine
                + "right_splitter_distance = " + Math.Max(150, state.RightSplitterDistance) + Environment.NewLine
                + "setup_width = " + Math.Max(820, state.SetupWidth) + Environment.NewLine
                + "setup_height = " + Math.Max(650, state.SetupHeight) + Environment.NewLine
                + "setup_x = " + state.SetupX + Environment.NewLine
                + "setup_y = " + state.SetupY + Environment.NewLine
                + "setup_maximized = " + Bool(state.SetupMaximized) + Environment.NewLine
                + "setup_primary_tab = " + Math.Max(0, state.SetupPrimaryTab) + Environment.NewLine
                + "setup_advanced_tab = " + Math.Max(0, state.SetupAdvancedTab) + Environment.NewLine
                + "compare_width = " + Math.Max(760, state.CompareWidth) + Environment.NewLine
                + "compare_height = " + Math.Max(500, state.CompareHeight) + Environment.NewLine
                + "preview_width = " + Math.Max(360, state.PreviewWidth) + Environment.NewLine
                + "preview_height = " + Math.Max(420, state.PreviewHeight) + Environment.NewLine;
            WriteTextAtomic(UiPath, text);
        }

        public static string ResolvePortablePath(string value)
        {
            if (String.IsNullOrWhiteSpace(value)) return "";
            if (Path.IsPathRooted(value) || value.StartsWith("\\\\", StringComparison.Ordinal))
                return Path.GetFullPath(value);
            return Path.GetFullPath(Path.Combine(AppRoot, value.Replace('/', Path.DirectorySeparatorChar)));
        }

        private static string ResolveOptionalPath(string value)
        {
            return String.IsNullOrWhiteSpace(value) ? "" : ResolvePortablePath(value);
        }

        public static string ToPortablePath(string value)
        {
            if (String.IsNullOrWhiteSpace(value)) return "";
            try
            {
                string root = AppRoot.TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
                string full = Path.GetFullPath(value);
                if (full.StartsWith(root, StringComparison.OrdinalIgnoreCase))
                    return full.Substring(root.Length).Replace('\\', '/');
            }
            catch { }
            return value;
        }

        public static void WriteTextAtomic(string path, string contents)
        {
            WriteTextAtomic(path, contents, null);
        }

        internal static bool WriteTextAtomic(string path, string contents, Func<string, bool> protectStagedFile)
        {
            string parent = Path.GetDirectoryName(path);
            if (String.IsNullOrEmpty(parent))
                throw new InvalidOperationException("Unable to determine configuration directory for " + path);
            Directory.CreateDirectory(parent);
            string staged = Path.Combine(parent, ".splined-" + Guid.NewGuid().ToString("N") + ".tmp");
            string backup = path + ".splined-backup";
            bool protectionApplied = protectStagedFile == null;
            try
            {
                File.WriteAllText(staged, contents, new UTF8Encoding(false));
                if (protectStagedFile != null) protectionApplied = protectStagedFile(staged);
                if (File.Exists(backup)) File.Delete(backup);
                if (File.Exists(path)) File.Move(path, backup);
                File.Move(staged, path);
                if (File.Exists(backup)) File.Delete(backup);
            }
            catch
            {
                if (!File.Exists(path) && File.Exists(backup)) File.Move(backup, path);
                throw;
            }
            finally
            {
                if (File.Exists(staged)) File.Delete(staged);
            }
            return protectionApplied;
        }

        private static string BuildConfigText(ConfigState state)
        {
            StringBuilder text = new StringBuilder();
            text.AppendLine("# ============================================================================");
            text.AppendLine("# SPLINED Configuration v5");
            text.AppendLine("# Relative runtime paths are resolved from the SPLINED application root.");
            text.AppendLine("# ============================================================================");
            text.AppendLine();
            text.AppendLine("config_version = 5");
            text.AppendLine("mode = " + Quote(state.Mode));
            text.AppendLine("verbosity = " + Quote(state.Verbosity));
            text.AppendLine();
            text.AppendLine("[library]");
            text.AppendLine("music_library = " + Quote(ToPortablePath(state.MusicLibrary)));
            text.AppendLine("ignored_subs = " + FormatArray(state.IgnoredSubs));
            text.AppendLine();
            text.AppendLine("[scan]");
            text.AppendLine("scan_library_dir = " + Quote(ToPortablePath(state.ScanLibraryDir)));
            text.AppendLine("scan_mode = " + Bool(state.ScanMode));
            text.AppendLine("library_scan = " + Bool(state.LibraryScan));
            text.AppendLine("scan_mode_timeout = " + state.ScanModeTimeout.ToString("0.###", CultureInfo.InvariantCulture));
            text.AppendLine("cache_dir = " + Quote(ToPortablePath(state.CacheDir)));
            text.AppendLine("log_dir = " + Quote(ToPortablePath(state.LogDir)));
            text.AppendLine("history_dir = " + Quote(ToPortablePath(state.HistoryDir)));
            text.AppendLine();
            text.AppendLine("[output]");
            text.AppendLine("file_name = " + Quote(state.FileName));
            text.AppendLine("file_formats = " + FormatArray(state.Formats));
            text.AppendLine("preserve_file = " + Bool(state.PreserveFile));
            text.AppendLine("square = " + Bool(state.Square));
            text.AppendLine("square_mode = " + Quote(state.SquareMode));
            text.AppendLine("square_round_to = " + state.SquareRoundTo);
            text.AppendLine("upscale_below_ideal = " + Bool(state.UpscaleBelowIdeal));
            text.AppendLine("evaluate_final_image = " + Bool(state.EvaluateFinalImage));
            text.AppendLine();
            text.AppendLine("[range]");
            text.AppendLine("min = " + state.RangeMin);
            text.AppendLine("ideal = " + state.RangeIdeal);
            text.AppendLine("max = " + state.RangeMax);
            text.AppendLine("ladder = " + state.RangeLadder);
            text.AppendLine();
            text.AppendLine("[sources]");
            text.AppendLine("cover_sources = " + FormatArray(state.Sources.Where(source => ConfigState.ArtworkSources.Contains(source, StringComparer.OrdinalIgnoreCase))));
            text.AppendLine("exclude_cover_sources = " + FormatArray(state.ExcludedSources.Where(source => ConfigState.ArtworkSources.Contains(source, StringComparer.OrdinalIgnoreCase))));
            text.AppendLine();
            foreach (string source in ConfigState.KnownSources)
            {
                SourcePolicyState policy;
                if (!state.SourcePolicies.TryGetValue(source, out policy)) policy = new SourcePolicyState();
                text.AppendLine("[source_policies." + source + "]");
                text.AppendLine("enabled = " + Bool(policy.Enabled));
                text.AppendLine("source_override = " + Bool(policy.SourceOverride));
                if (source.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase))
                {
                    text.AppendLine();
                    continue;
                }
                text.AppendLine("minimum_range_type = " + Quote(policy.MinimumRangeType));
                text.AppendLine("allow_below_minimum_fallback = " + Bool(policy.AllowBelowMinimumFallback));
                if (policy.MinimumShortSide.HasValue) text.AppendLine("minimum_short_side = " + policy.MinimumShortSide.Value);
                if (policy.MaximumShortSide.HasValue) text.AppendLine("maximum_short_side = " + policy.MaximumShortSide.Value);
                if (policy.MinimumWidth.HasValue) text.AppendLine("minimum_width = " + policy.MinimumWidth.Value);
                if (policy.MinimumHeight.HasValue) text.AppendLine("minimum_height = " + policy.MinimumHeight.Value);
                text.AppendLine("primary_image_only = " + Bool(policy.PrimaryImageOnly));
                text.AppendLine();
            }
            text.AppendLine("[samples]");
            text.AppendLine("sample_write = " + Bool(state.SampleWrite));
            text.AppendLine();
            text.AppendLine("[credentials]");
            text.AppendLine("credential_dir = " + Quote(ToPortablePath(state.CredentialDir)));
            text.AppendLine();
            text.AppendLine("[logging]");
            text.AppendLine("retention_days = " + state.LogRetentionDays);
            text.AppendLine();
            text.AppendLine("[history]");
            text.AppendLine("enabled = " + Bool(state.HistoryEnabled));
            text.AppendLine("retention_days = " + state.HistoryRetentionDays);
            text.AppendLine();
            text.AppendLine("[splineai]");
            text.AppendLine("enabled = " + Bool(state.SplineAiEnabled));
            text.AppendLine("endpoint = " + Quote(state.SplineAiEndpoint));
            return text.ToString();
        }

        private static string ReadSection(string text, string section)
        {
            if (String.IsNullOrEmpty(section))
                return Regex.Replace(text, @"(?ms)^\s*\[[^\]]+\].*$", "");
            Match match = Regex.Match(text, @"(?ms)^\s*\[" + Regex.Escape(section) + @"\]\s*(.*?)(?=^\s*\[|\z)");
            return match.Success ? match.Groups[1].Value : "";
        }

        private static string ReadString(string text, string section, string key, string fallback)
        {
            string scope = ReadSection(text, section);
            Match match = Regex.Match(scope, "(?m)^\\s*" + Regex.Escape(key) + "\\s*=\\s*(?:\"((?:\\\\.|[^\"])*)\"|'([^']*)')\\s*(?:#.*)?$");
            if (!match.Success) return fallback;
            return match.Groups[1].Success
                ? match.Groups[1].Value.Replace("\\\"", "\"").Replace("\\\\", "\\")
                : match.Groups[2].Value;
        }

        private static int ReadInt(string text, string section, string key, int fallback)
        {
            string scope = ReadSection(text, section);
            Match match = Regex.Match(scope, @"(?m)^\s*" + Regex.Escape(key) + @"\s*=\s*(-?\d+)\s*$");
            int value;
            return match.Success && Int32.TryParse(match.Groups[1].Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out value) ? value : fallback;
        }

        private static int? ReadNullableInt(string text, string section, string key)
        {
            string scope = ReadSection(text, section);
            Match match = Regex.Match(scope, @"(?m)^\s*" + Regex.Escape(key) + @"\s*=\s*(\d+)\s*$");
            int value;
            return match.Success && Int32.TryParse(match.Groups[1].Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out value)
                ? (int?)value
                : null;
        }

        private static double ReadDoubleOrOff(string text, string section, string key, double fallback)
        {
            string scope = ReadSection(text, section);
            Match match = Regex.Match(scope, "(?mi)^\\s*" + Regex.Escape(key) + "\\s*=\\s*(false|\"off\"|-?\\d+(?:\\.\\d+)?)\\s*$");
            if (!match.Success) return fallback;
            string raw = match.Groups[1].Value.Trim('"');
            if (raw.Equals("false", StringComparison.OrdinalIgnoreCase) || raw.Equals("off", StringComparison.OrdinalIgnoreCase)) return 0;
            double value;
            return Double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out value) ? value : fallback;
        }

        private static bool ReadBool(string text, string section, string key, bool fallback)
        {
            string scope = ReadSection(text, section);
            Match match = Regex.Match(scope, @"(?mi)^\s*" + Regex.Escape(key) + @"\s*=\s*(true|false)\s*$");
            return match.Success ? match.Groups[1].Value.Equals("true", StringComparison.OrdinalIgnoreCase) : fallback;
        }

        private static List<string> ReadArray(string text, string section, string key)
        {
            string scope = ReadSection(text, section);
            List<string> values = new List<string>();
            Match start = Regex.Match(scope, @"(?m)^\s*" + Regex.Escape(key) + @"\s*=\s*\[");
            if (!start.Success) return values;

            StringBuilder content = new StringBuilder();
            bool doubleQuoted = false;
            bool singleQuoted = false;
            bool escaped = false;
            for (int index = start.Index + start.Length; index < scope.Length; index++)
            {
                char character = scope[index];
                if (doubleQuoted)
                {
                    content.Append(character);
                    if (escaped) escaped = false;
                    else if (character == '\\') escaped = true;
                    else if (character == '"') doubleQuoted = false;
                    continue;
                }
                if (singleQuoted)
                {
                    content.Append(character);
                    if (character == '\'') singleQuoted = false;
                    continue;
                }
                if (character == ']') break;
                content.Append(character);
                if (character == '"') doubleQuoted = true;
                else if (character == '\'') singleQuoted = true;
            }

            foreach (Match item in Regex.Matches(content.ToString(), "\"((?:\\\\.|[^\"])*)\"|'([^']*)'"))
            {
                string value = item.Groups[1].Success
                    ? item.Groups[1].Value.Replace("\\\"", "\"").Replace("\\\\", "\\").Trim()
                    : item.Groups[2].Value.Trim();
                if (value.Length > 0 && !values.Any(existing => existing.Equals(value, StringComparison.OrdinalIgnoreCase)))
                    values.Add(value);
            }
            return values;
        }

        private static string Quote(string value)
        {
            return "\"" + (value ?? "").Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
        }

        private static string FormatArray(IEnumerable<string> values)
        {
            List<string> clean = values.Where(value => !String.IsNullOrWhiteSpace(value)).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
            if (clean.Count == 0) return "[]";
            return "[" + Environment.NewLine + String.Join(Environment.NewLine, clean.Select(value => "    " + Quote(value.Trim()) + ",")) + Environment.NewLine + "]";
        }

        private static string Bool(bool value)
        {
            return value ? "true" : "false";
        }
    }
}
