using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class GuiSelfTests
    {
        [STAThread]
        private static int Main()
        {
            try
            {
                string internalSettings = Path.Combine(Path.GetTempPath(), "splined-windows-v4-tests-" + Guid.NewGuid().ToString("N"));
                Environment.SetEnvironmentVariable("SPLINED_INTERNAL_SETTINGS_TEST_DIR", internalSettings);
                string library = Path.Combine(ConfigStore.AppRoot, "fixture-library");
                string firstAlbum = Path.Combine(library, "Artist One", "Album One");
                string ignoredAlbum = Path.Combine(library, "Skip This", "Ignored Album");
                string unicodeArtist = Path.Combine(library, "Françoise Hardy");
                string unicodeAlbum = Path.Combine(unicodeArtist, "Cafe\u0301 — Big Bambú");
                string mojibakeAlbum = Path.Combine(library, "Cheech & Chong", "Big BambÃº");
                Directory.CreateDirectory(firstAlbum);
                Directory.CreateDirectory(ignoredAlbum);
                Directory.CreateDirectory(unicodeAlbum);
                Directory.CreateDirectory(mojibakeAlbum);
                File.WriteAllBytes(Path.Combine(firstAlbum, "track.mp3"), new byte[] { 0 });
                File.WriteAllBytes(Path.Combine(ignoredAlbum, "track.mp3"), new byte[] { 0 });

                ConfigState state = ConfigStore.Defaults();
                Assert(state.CacheDir == Path.Combine(ConfigStore.DefaultUserDataRoot, "cache")
                    && state.TemporaryCacheDir == Path.Combine(ConfigStore.DefaultUserDataRoot, "run-cache")
                    && state.LogDir == Path.Combine(ConfigStore.DefaultUserDataRoot, "logs")
                    && state.CredentialDir == Path.Combine(ConfigStore.DefaultUserDataRoot, "credentials"),
                    "First-run data paths did not default to the current user's Local AppData directory.");
                state.CacheDir = Path.Combine(internalSettings, "cache");
                state.TemporaryCacheDir = Path.Combine(internalSettings, "run-cache");
                state.LogDir = Path.Combine(internalSettings, "logs");
                state.CredentialDir = Path.Combine(internalSettings, "credentials");
                state.ConfigPath = ConfigStore.DefaultConfigPath;
                state.MusicLibrary = library;
                string[] excludedFolders =
                {
                    "Skip*", "[Octo-Fiesta]", "[Artist Singles]", "[no artist]", "[videos]",
                    "[original]", "#SyncVersion", "@eaDir", ".stfolder-*"
                };
                state.IgnoredSubs.AddRange(excludedFolders);
                state.Mode = "write";
                state.ScanModeTimeout = 0;
                state.AiSplinedEnabled = true;
                state.AiSplinedEndpoint = "internal-placeholder";
                state.AiSplinedMinimumShortSide = 640;
                state.AiSplinedAllowBelowMinimumOverride = true;
                state.SourcePolicies["discogs"] = new SourcePolicyState
                {
                    SourceOverride = true,
                    MinimumRangeType = "LowerRange",
                    AllowBelowMinimumFallback = true,
                    MinimumShortSide = 1500,
                    MaximumShortSide = 4200,
                    MinimumWidth = 1400,
                    MinimumHeight = 1300,
                    PrimaryImageOnly = false
                };
                ConfigStore.Save(state);

                ConfigState loaded = ConfigStore.Load();
                Assert(loaded.Mode == "write", "Config v5 mode did not round-trip.");
                Assert(loaded.IgnoredSubs.SequenceEqual(excludedFolders), "Excluded folders with bracketed names did not round-trip.");
                Assert(loaded.ScanModeTimeout == 0, "Disabled timeout did not round-trip.");
                string configText = ConfigStore.ExportConfigText(loaded);
                Assert(configText.Contains("[credentials]") && configText.Contains("credential_dir =")
                    && configText.Contains("temporary_cache_dir =")
                    && !configText.Contains("[fanarttv]") && !configText.Contains("[lastfm]")
                    && !configText.Contains("[musicbrainz]") && !configText.Contains("credential_file")
                    && !configText.Contains("token_file") && !configText.Contains("client_id"),
                    "Config v5 must contain only the credential directory, never provider filenames or credential fields.");
                Assert(CredentialStore.PathFor(loaded, "fanarttv") == Path.Combine(loaded.CredentialDir, "fanarttv.json")
                    && CredentialStore.PathFor(loaded, "lastfm") == Path.Combine(loaded.CredentialDir, "lastfm.json")
                    && CredentialStore.PathFor(loaded, "musicbrainz") == Path.Combine(loaded.CredentialDir, "musicbrainz.json"),
                    "Provider credential files were not derived from the credential directory.");
                Assert(loaded.AiSplinedEnabled && loaded.AiSplinedEndpoint == "internal-placeholder"
                    && loaded.AiSplinedMinimumShortSide == 640
                    && loaded.AiSplinedAllowBelowMinimumOverride,
                    "Hidden AISPLINE values were not preserved.");
                SourcePolicyState discogsPolicy = loaded.SourcePolicies["discogs"];
                Assert(discogsPolicy.SourceOverride && discogsPolicy.MinimumRangeType == "LowerRange"
                    && discogsPolicy.AllowBelowMinimumFallback && discogsPolicy.MinimumShortSide == 1500
                    && discogsPolicy.MaximumShortSide == 4200 && discogsPolicy.MinimumWidth == 1400
                    && discogsPolicy.MinimumHeight == 1300 && !discogsPolicy.PrimaryImageOnly,
                    "Per-source policy did not round-trip through Config v5.");
                Assert(SourcePolicyRules.Evaluate(loaded, discogsPolicy, 1499, 1499).Result == SourcePolicyResult.Reject,
                    "Explicit per-source minimum did not reject the lower segment.");
                Assert(SourcePolicyRules.Evaluate(loaded, discogsPolicy, 1500, 1500).Result == SourcePolicyResult.Accept,
                    "Explicit per-source minimum did not accept the upper segment.");
                SourcePolicyState fallbackPolicy = new SourcePolicyState
                {
                    SourceOverride = true,
                    MinimumRangeType = "Ideal",
                    AllowBelowMinimumFallback = true
                };
                Assert(SourcePolicyRules.Evaluate(loaded, fallbackPolicy, 1500, 1500).Result == SourcePolicyResult.Fallback,
                    "The adjacent LowerRange fallback below Ideal was not applied.");
                Assert(SourcePolicyRules.Evaluate(loaded, fallbackPolicy, 900, 900).Result == SourcePolicyResult.Reject,
                    "Ideal fallback incorrectly opened every range down through BelowMinimum.");
                Assert(SourcePolicyRules.Evaluate(loaded, new SourcePolicyState(), 900, 900).Result == SourcePolicyResult.Reject,
                    "Override-off source policy must preserve the existing global range behavior.");
                SourcePolicyState strictPolicy = new SourcePolicyState
                {
                    StrictOverride = true,
                    SourceOverride = true,
                    MinimumRangeType = "Ideal",
                    MinimumShortSide = 2000
                };
                Assert(SourcePolicyRules.Evaluate(loaded, strictPolicy, 1500, 1500).Result == SourcePolicyResult.Accept,
                    "Strict Override did not supersede Source Override and retain the global range.");
                Assert(loaded.SourcePolicies["amazon"].StrictOverride
                    && configText.Contains("strict_override = true"),
                    "Amazon strict policy default did not round-trip through Windows internal settings.");

                ConfigState optionCoverage = loaded.Clone();
                optionCoverage.ConfigPath = ConfigStore.InternalSettingsLabel;
                optionCoverage.Mode = "read";
                optionCoverage.Verbosity = "trace";
                optionCoverage.ScanLibraryDir = firstAlbum;
                optionCoverage.ScanMode = false;
                optionCoverage.LibraryScan = true;
                optionCoverage.ScanModeTimeout = 12.5;
                optionCoverage.CacheDir = Path.Combine(ConfigStore.AppRoot, "coverage-cache");
                optionCoverage.TemporaryCacheDir = Path.Combine(ConfigStore.AppRoot, "coverage-run-cache");
                optionCoverage.SqliteShared = true;
                optionCoverage.LogDir = Path.Combine(ConfigStore.AppRoot, "coverage-logs");
                optionCoverage.CredentialDir = Path.Combine(ConfigStore.AppRoot, "coverage-credentials");
                optionCoverage.Formats = new List<string>(new[] { "webp", "jpeg", "png" });
                optionCoverage.Sources = new List<string>(new[] { "discogs", "coverartarchive", "lastfm", "fanarttv", "itunes", "deezer" });
                optionCoverage.ExcludedSources = new List<string>(new[] { "deezer" });
                optionCoverage.LogRetentionDays = 33;
                optionCoverage.HistoryEnabled = true;
                optionCoverage.HistoryRetentionDays = 90;
                optionCoverage.SampleWrite = false;
                optionCoverage.FileName = "folder";
                optionCoverage.PreserveFile = false;
                optionCoverage.Square = false;
                optionCoverage.SquareMode = "off";
                optionCoverage.SquareRoundTo = 8;
                optionCoverage.UpscaleBelowIdeal = true;
                optionCoverage.UpscaleMaxPercent = 175;
                optionCoverage.UpscaleAdaptiveDefaults = false;
                optionCoverage.UpscalePicturePercent = 6;
                optionCoverage.UpscaleSharpenPercent = 4;
                optionCoverage.UpscaleSoftnessPercent = 2;
                optionCoverage.UpscaleContrastPercent = 7;
                optionCoverage.UpscaleExposurePercent = -3;
                optionCoverage.UpscaleBrightnessPercent = 5;
                optionCoverage.UpscaleGammaPercent = -2;
                optionCoverage.UpscaleColorTemperature = -25;
                optionCoverage.EvaluateFinalImage = false;
                optionCoverage.RangeMin = 1000;
                optionCoverage.RangeIdeal = 1700;
                optionCoverage.RangeMax = 2300;
                optionCoverage.RangeLadder = 3500;
                ConfigStore.Save(optionCoverage);
                ConfigState optionReopened = ConfigStore.Load();
                Assert(optionReopened.Mode == "read" && optionReopened.Verbosity == "trace"
                    && optionReopened.ScanLibraryDir == firstAlbum && !optionReopened.ScanMode && optionReopened.LibraryScan
                    && Math.Abs(optionReopened.ScanModeTimeout - 12.5) < 0.001,
                    "Python runtime and scan options did not round-trip through Config v5.");
                Assert(optionReopened.CacheDir == optionCoverage.CacheDir
                    && optionReopened.TemporaryCacheDir == optionCoverage.TemporaryCacheDir
                    && optionReopened.SqliteShared && optionReopened.LogDir == optionCoverage.LogDir
                    && optionReopened.CredentialDir == optionCoverage.CredentialDir,
                    "Python directory options did not round-trip through Config v5.");
                Assert(optionReopened.ConfigPath == ConfigStore.InternalSettingsLabel,
                    "Windows v4 settings did not remain in the internal Windows store.");
                string redactedLog = RuntimeLog.Redact("Authorization: Bearer top-secret access_token=also-secret");
                Assert(!redactedLog.Contains("top-secret") && !redactedLog.Contains("also-secret"),
                    "Runtime diagnostics did not redact authorization and token values.");
                string runLogDirectory = Path.Combine(optionReopened.LogDir, "run");
                Directory.CreateDirectory(runLogDirectory);
                string staleRunLog = Path.Combine(runLogDirectory, "splined-debug-stale.log");
                File.WriteAllText(staleRunLog, "stale");
                RuntimeLog.Initialize(optionReopened);
                Assert(!File.Exists(staleRunLog) && File.Exists(RuntimeLog.Path),
                    "Windows startup did not replace prior run diagnostics with the current session log.");
                Assert(optionReopened.Formats.SequenceEqual(optionCoverage.Formats)
                    && optionReopened.Sources.SequenceEqual(optionCoverage.Sources)
                    && optionReopened.ExcludedSources.SequenceEqual(optionCoverage.ExcludedSources),
                    "Output format or artwork-source order/exclusion did not round-trip.");
                Assert(optionReopened.LogRetentionDays == 33 && optionReopened.HistoryEnabled && optionReopened.HistoryRetentionDays == 90
                    && !optionReopened.SampleWrite && optionReopened.FileName == "folder" && !optionReopened.PreserveFile
                    && !optionReopened.Square && optionReopened.SquareMode == "off" && optionReopened.SquareRoundTo == 8
                    && optionReopened.UpscaleBelowIdeal && optionReopened.UpscaleMaxPercent == 175
                    && !optionReopened.UpscaleAdaptiveDefaults && optionReopened.UpscalePicturePercent == 6
                    && optionReopened.UpscaleSharpenPercent == 4 && optionReopened.UpscaleSoftnessPercent == 2
                    && optionReopened.UpscaleContrastPercent == 7 && optionReopened.UpscaleExposurePercent == -3
                    && optionReopened.UpscaleBrightnessPercent == 5 && optionReopened.UpscaleGammaPercent == -2
                    && optionReopened.UpscaleColorTemperature == -25
                    && !optionReopened.EvaluateFinalImage,
                    "Python output/sample or Config v5 retention options did not round-trip.");
                Assert(optionReopened.RangeMin == 1000 && optionReopened.RangeIdeal == 1700
                    && optionReopened.RangeMax == 2300 && optionReopened.RangeLadder == 3500,
                    "Artwork resolution range did not round-trip.");
                ConfigStore.Save(loaded);

                string snapshotJson = new JavaScriptSerializer().Serialize(new Dictionary<string, object>
                {
                    { "schema_version", 2 },
                    { "database_path", Path.Combine(loaded.CacheDir, "splined.db") },
                    { "canonical_root", loaded.MusicLibrary },
                    { "local_root", loaded.MusicLibrary },
                    { "albums", new object[] { new Dictionary<string, object>
                        {
                            { "album_key", "tag:album-one" },
                            { "artist", "Artist One" }, { "tagged_artist", "Tagged Artist" },
                            { "title", "Album One" }, { "path", firstAlbum },
                            { "representative_file", Path.Combine(firstAlbum, "track.mp3") }, { "status", "unprocessed" },
                            { "compilation", true }, { "compilation_track_art_eligible", true }, { "track_count", 1 },
                            { "compilation_tracks", new object[] { new Dictionary<string, object>
                                {
                                    { "path", Path.Combine(firstAlbum, "track.mp3") },
                                    { "title", "Fixture Track" },
                                    { "embedded_artwork_recorded", true }
                                }
                            } },
                            { "release_year", "1998" },
                            { "has_local_artwork", false }, { "local_artwork_files", new string[0] },
                            { "cover_path", Path.Combine(firstAlbum, "cover.jpg") }, { "cover_name", "cover.jpg" },
                            { "cover_format", "JPEG" }, { "cover_width", 1500 }, { "cover_height", 1500 },
                            { "root_files", 12 }, { "cover_files", 1 }
                        }
                    } }
                });
                AlbumInfo[] albums = LibraryInventory.FromSnapshotJson(snapshotJson).ToArray();
                Assert(albums.Length == 1 && albums[0].Title == "Album One" && albums[0].Key == "tag:album-one"
                    && albums[0].ReleaseYear == "1998" && albums[0].TrackCount == 1
                    && albums[0].Compilation && albums[0].CompilationTrackArtworkPending
                    && albums[0].CompilationTracks.Count == 1
                    && albums[0].CompilationTracks[0].Album == albums[0]
                    && albums[0].CompilationTracks[0].EmbeddedArtworkRecorded
                    && albums[0].CoverName == "cover.jpg" && albums[0].CoverWidth == 1500
                    && albums[0].RootFiles == 12,
                    "SQLite snapshot did not populate the Album identity model.");
                Assert(albums[0].State == AlbumState.New && albums[0].EligibleByDefault, "A new SQLite Album was not eligible by default.");
                string coverOnlySnapshot = snapshotJson.Replace(
                    "\"has_local_artwork\":false",
                    "\"has_local_artwork\":true");
                AlbumInfo coverOnlyAlbum = LibraryInventory.FromSnapshotJson(coverOnlySnapshot).Single();
                Assert(coverOnlyAlbum.HasLocalArtwork && coverOnlyAlbum.State == AlbumState.New && coverOnlyAlbum.EligibleByDefault,
                    "Existing artwork incorrectly created Processed authority in the Windows picker.");
                string oversizedSnapshot = snapshotJson.Replace("Album One", new string('A', 2200000));
                Assert(LibraryInventory.FromSnapshotJson(oversizedSnapshot).Count == 1,
                    "SQLite snapshots larger than JavaScriptSerializer's legacy 2 MB default were rejected.");

                VerifyArtistAggregateAndSelectionRules();

                string processedSnapshot = snapshotJson.Replace("\"status\":\"unprocessed\"", "\"status\":\"processed\"")
                    .Replace("\"has_local_artwork\":false", "\"has_local_artwork\":true");
                AlbumInfo indexedAlbum = LibraryInventory.FromSnapshotJson(processedSnapshot).Single(album => album.Path == firstAlbum);
                Assert(indexedAlbum.HasLocalArtwork && indexedAlbum.State == AlbumState.Processed,
                    "SQLite snapshot did not preserve an explicit Processed status.");

                UiState ui = new UiState
                {
                    Theme = "Dark", ShowStatusOnLaunch = false, ShowConfirmations = false, HoverEnabled = true, ShowArtwork = true,
                    ShowMediaSelector = false, MediaFilterExpanded = false, CandidateFilterExpanded = true,
                    MediaArtistFilter = "Alpha", MediaAlbumFilter = "Fresh",
                    MediaShowRed = false, FilteredScanMode = "read",
                    ShowTracks = true, SelectedCompilationTrackPath = Path.Combine(firstAlbum, "track.mp3"),
                    SelectedAlbumPaths = new List<string> { firstAlbum },
                    MainWidth = 1320, MainHeight = 860, MainX = 110, MainY = 90,
                    MainSplitterDistance = 455, RightSplitterDistance = 305,
                    SetupWidth = 1040, SetupHeight = 820, SetupX = 130, SetupY = 100,
                    SetupPrimaryTab = 1, SetupAdvancedTab = 2,
                    CompareWidth = 1110, CompareHeight = 710, PreviewWidth = 610, PreviewHeight = 650
                };
                ConfigStore.SaveUi(ui);
                UiState loadedUi = ConfigStore.LoadUi();
                Assert(loadedUi.Theme == "Dark" && !loadedUi.ShowStatusOnLaunch && !loadedUi.ShowConfirmations && loadedUi.HoverEnabled && loadedUi.ShowArtwork
                    && !loadedUi.ShowMediaSelector && !loadedUi.MediaFilterExpanded && loadedUi.CandidateFilterExpanded
                    && loadedUi.MediaArtistFilter == "Alpha" && !loadedUi.MediaShowRed
                    && loadedUi.ShowTracks && loadedUi.SelectedCompilationTrackPath == Path.Combine(firstAlbum, "track.mp3")
                    && loadedUi.FilteredScanMode == "read" && loadedUi.SelectedAlbumPaths.SequenceEqual(new[] { firstAlbum })
                    && loadedUi.MainWidth == 1320 && loadedUi.MainSplitterDistance == 455
                    && loadedUi.SetupWidth == 1040 && loadedUi.SetupAdvancedTab == 2
                    && loadedUi.CompareWidth == 1110 && loadedUi.PreviewHeight == 650, "Internal Windows interface settings did not round-trip.");
                loadedUi.ShowTracks = false;
                loadedUi.SelectedCompilationTrackPath = "";
                loadedUi.CandidateExcludedSources = new List<string> { "amazon" };
                loadedUi.CandidateExcludedTypes = new List<string> { "Rejected" };
                loadedUi.CandidateExcludedPolicies = new List<string> { "Strict" };
                loadedUi.CandidateExcludedRanges = new List<string> { "BelowMinimum" };
                ConfigStore.SaveUi(loadedUi);
                UiState filteredUi = ConfigStore.LoadUi();
                Assert(filteredUi.CandidateExcludedSources.SequenceEqual(new[] { "amazon" })
                    && filteredUi.CandidateExcludedTypes.SequenceEqual(new[] { "Rejected" })
                    && filteredUi.CandidateExcludedPolicies.SequenceEqual(new[] { "Strict" })
                    && filteredUi.CandidateExcludedRanges.SequenceEqual(new[] { "BelowMinimum" }),
                    "Artwork Filter choices did not persist in Interface Settings.");
                string backupPath = Path.Combine(internalSettings, "portable-settings.spl");
                BackupSelection backupSelection = new BackupSelection
                {
                    Settings = true,
                    Interface = true,
                    Credentials = false,
                    Database = false,
                    Diagnostics = false
                };
                BackupService.Export(backupPath, loaded, filteredUi, backupSelection, "fixture-password");
                SplinedBackupPayload protectedBackup = BackupService.Read(backupPath, "fixture-password");
                Assert(protectedBackup.settings.Contains("config_version = 5")
                    && protectedBackup.interface_settings.Contains("[ui]")
                    && protectedBackup.interface_settings.Contains("candidate_excluded_sources = [")
                    && protectedBackup.interface_settings.Contains("    \"amazon\",")
                    && protectedBackup.credentials.Count == 0 && protectedBackup.database == null,
                    "Selective password-protected .spl export did not preserve its chosen sections.");
                loadedUi.CandidateExcludedSources.Clear();
                loadedUi.CandidateExcludedTypes.Clear();
                loadedUi.CandidateExcludedPolicies.Clear();
                loadedUi.CandidateExcludedRanges.Clear();
                ConfigStore.SaveUi(loadedUi);
                bool wrongPasswordRejected = false;
                try { BackupService.Read(backupPath, "wrong-password"); }
                catch (InvalidOperationException) { wrongPasswordRejected = true; }
                Assert(wrongPasswordRejected, "Password-protected .spl backup accepted an incorrect password.");
                string savedConfigBeforeTemporaryRun = ConfigStore.ExportConfigText(ConfigStore.Load());
                ConfigState temporaryRunState = loaded.Clone();
                temporaryRunState.Mode = "read";
                string runtimeConfigText = ConfigStore.ExportConfigText(temporaryRunState);
                Assert(runtimeConfigText.Contains("mode = \"read\"")
                    && ConfigStore.ExportConfigText(ConfigStore.Load()) == savedConfigBeforeTemporaryRun,
                    "The in-memory runtime settings handoff changed the saved internal settings.");
                Assert(!Directory.Exists(Path.Combine(ConfigStore.AppRoot, ".splined-runtime")),
                    "The in-memory runtime settings handoff created a portable runtime directory.");

                bool emptySnapshotRejected = false;
                try { LibraryInventory.FromSnapshotJson(""); }
                catch (InvalidOperationException) { emptySnapshotRejected = true; }
                Assert(emptySnapshotRejected, "An empty SQLite media snapshot was accepted.");

                using (SetupForm setup = new SetupForm(loaded, false))
                {
                    setup.ShowInTaskbar = false;
                    setup.StartPosition = FormStartPosition.Manual;
                    setup.Location = new Point(-32000, -32000);
                    setup.Show();
                    Application.DoEvents();
                    Assert(Descendants(setup).OfType<Button>().Where(button => !(button is InfoButton)).All(button => button is FluentButton),
                        "A Settings action control bypassed the shared owner-painted FluentButton geometry.");
                    Assert(Descendants(setup).OfType<ComboBox>().All(combo => combo is FluentComboBox)
                        && Descendants(setup).OfType<NumericUpDown>().All(number => number is FluentNumericUpDown)
                        && Descendants(setup).OfType<CheckBox>().All(check => check is FluentCheckBox)
                        && Descendants(setup).OfType<TextBox>().Where(text => !(text.Parent is NumericUpDown)).All(text => text is FluentTextBox),
                        "A Settings input bypassed the shared Fluent Compact text, combo, spinner, or checkbox renderer.");
                    using (Bitmap embeddedWatermark = EmbeddedAssets.LoadWatermark())
                        Assert(embeddedWatermark != null,
                            "The responsive SPLINED watermark resource is missing.");
                    FieldInfo setupRootField = typeof(SetupForm).GetField("rootLayout", BindingFlags.Instance | BindingFlags.NonPublic);
                    Assert(setupRootField.GetValue(setup) is TableLayoutPanel
                        && !(setupRootField.GetValue(setup) is WatermarkTableLayoutPanel),
                        "Settings must remain an opaque information surface; the watermark belongs only in artwork preview.");
                    TextBox ignored = setup.Controls.Find("ignoredDirectories", true).OfType<TextBox>().Single();
                    Assert(ignored.Text == String.Join(", ", excludedFolders), "Excluded folders disappeared when Settings was reopened.");
                    TabControl advanced = (TabControl)typeof(SetupForm).GetField("advancedTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(setup);
                    Assert(advanced.TabPages.Cast<TabPage>().Select(page => page.Text).SequenceEqual(new[] { "Library, Paths & Processing", "Artwork & Output", "Sources & Matching" }),
                        "Advanced settings are not grouped into the three task-oriented tabs.");
                    TabPage pathsTab = advanced.TabPages.Cast<TabPage>().Single(page => page.Text == "Library, Paths & Processing");
                    string[] pathLabels = Descendants(pathsTab).OfType<Label>().Select(label => label.Text).ToArray();
                    Assert(pathLabels.Contains("SQL Database Directory *")
                        && pathLabels.Contains("Temporary Run Cache *")
                        && pathLabels.Contains("Credentials Directory *")
                        && pathLabels.Contains("Logs Directory *"),
                        "Settings did not separate and order database, temporary cache, credentials, and logs paths.");
                    Assert(Descendants(setup).OfType<Label>().Any(label => label.Text == "Music library scan folder")
                        && Descendants(setup).OfType<Label>().Any(label => label.Text == "Excluded folders")
                        && !Descendants(pathsTab).OfType<Label>().Any(label => label.Text.Contains("Scan directory")),
                        "Music library scan and excluded-folder controls were not moved into the aligned library header.");
                    Assert(IsDescendant(pathsTab, setup.Controls.Find("runtimeSettingsGroup", true).Single())
                        && IsDescendant(pathsTab, setup.Controls.Find("retentionSettingsGroup", true).Single())
                        && IsDescendant(pathsTab, setup.Controls.Find("retentionGroup", true).Single()),
                        "Runtime and Retention settings were not moved under Paths.");
                    Control tools = Descendants(setup).OfType<GroupBox>().Single(group => group.Text == "Tools");
                    Control pathsGroup = setup.Controls.Find("pathsGroup", true).Single();
                    Assert(IsDescendant(pathsTab, tools) && tools.Top >= pathsGroup.Bottom,
                        "Tools was not placed beneath Paths.");
                    Control pathActions = setup.Controls.Find("pathsActionRow", true).Single();
                    Button pathCredentials = setup.Controls.Find("pathsCredentialsButton", true).OfType<Button>().Single();
                    Button restorePortable = Descendants(pathActions).OfType<Button>().Single(button => button.Text == "Restore Suggested Paths");
                    Assert(IsDescendant(pathActions, pathCredentials) && IsDescendant(pathActions, restorePortable)
                        && Math.Abs(pathCredentials.Top - restorePortable.Top) <= 2,
                        "Credential / Status and Restore Suggested Paths are not aligned in the Paths action row.");
                    Control artworkOptions = setup.Controls.Find("artworkOptionsGroup", true).Single();
                    Dictionary<string, Button> formats = (Dictionary<string, Button>)typeof(SetupForm).GetField("formatButtons", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(setup);
                    Assert(formats.Count == 3 && formats.Values.All(button => IsDescendant(artworkOptions, button)),
                        "Output format buttons were not placed beside Artwork Options.");
                    Dictionary<string, Button> artworkToggles = (Dictionary<string, Button>)typeof(SetupForm).GetField("optionButtons", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(setup);
                    NumericUpDown maximumUpscale = setup.Controls.Find("upscaleMaxPercent", true).OfType<NumericUpDown>().Single();
                    Control maximumUpscaleRow = setup.Controls.Find("upscaleMaxRow", true).Single();
                    Assert(maximumUpscale.Value == 200 && !maximumUpscale.Enabled
                        && maximumUpscaleRow.Parent.Controls.GetChildIndex(maximumUpscaleRow)
                            == maximumUpscaleRow.Parent.Controls.GetChildIndex(artworkToggles["upscale"]) + 1,
                        "Maximum Upscale was not hidden directly below the disabled Upscale control.");
                    typeof(Button).GetMethod("OnClick", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(artworkToggles["upscale"], new object[] { EventArgs.Empty });
                    advanced.SelectedTab = advanced.TabPages[1];
                    Application.DoEvents();
                    Assert(maximumUpscale.Enabled && maximumUpscaleRow.Visible,
                        "Maximum Upscale did not appear when Upscale below ideal was activated.");
                    string[] advancedUpscaleInputs = { "upscalePicturePercent", "upscaleSharpenPercent",
                        "upscaleSoftnessPercent", "upscaleContrastPercent", "upscaleExposurePercent",
                        "upscaleBrightnessPercent", "upscaleGammaPercent", "upscaleColorTemperature" };
                    Control defaultUpscaleGroup = setup.Controls.Find("defaultUpscaleGroup", true).Single();
                    Assert(advancedUpscaleInputs.All(name => setup.Controls.Find(name, true).OfType<NumericUpDown>().Count() == 1)
                        && advancedUpscaleInputs.All(name => IsDescendant(defaultUpscaleGroup, setup.Controls.Find(name, true).Single())),
                        "Default Upscale / Advanced does not expose the same eight persistent controls as Artwork Filter.");
                    ComboBox existingArtwork = setup.Controls.Find("existingArtworkAction", true).OfType<ComboBox>().Single();
                    Assert(existingArtwork is FluentComboBox && existingArtwork.Items.Count == 2 && existingArtwork.SelectedIndex == 1
                        && Convert.ToString(existingArtwork.Items[0]).Contains("no numbered copies"),
                        "Existing-artwork overwrite/preserve behavior is not visible or did not repopulate.");
                    string[] commandButtons = { "Validate Saved Settings", "Credentials / Status...", "Open Logs Folder" };
                    Assert(commandButtons.All(expected => Descendants(setup).OfType<Button>().Any(button => button.Text == expected)),
                        "Advanced Settings omitted internal-settings validation, credentials, or diagnostic-folder tools.");
                    Assert(!Descendants(setup).OfType<Button>().Any(button => button.Text == "Open Config Folder"),
                        "Windows v4 still exposes an external configuration folder.");
                    Assert(!Descendants(setup).OfType<Button>().Any(button => button.Text == "Resolved Config...")
                        && !Descendants(setup).OfType<GroupBox>().Any(group => group.Text.IndexOf("Python", StringComparison.OrdinalIgnoreCase) >= 0),
                        "Settings still exposes the removed Python Help/Config Coverage surface.");
                    Control sourceConstraints = setup.Controls.Find("advancedSourceConstraintsGroup", true).Single();
                    Control resolutionRange = setup.Controls.Find("artworkResolutionGroup", true).Single();
                    TabPage sourcesTab = advanced.TabPages.Cast<TabPage>().Single(page => page.Text == "Sources & Matching");
                    Assert(IsDescendant(sourcesTab, resolutionRange) && !IsDescendant(sourceConstraints, resolutionRange)
                        && !Descendants(sourcesTab).Any(control => control.Text == "Test Candidate"),
                        "Artwork Resolution Range did not replace Test Candidate in the Sources policy preview.");
                    ComboBox fallback = setup.Controls.Find("sourceFallbackRange", true).OfType<ComboBox>().Single();
                    Assert(fallback.Items.Cast<object>().Any(item => Convert.ToString(item).Contains("one level below")),
                        "The fallback dropdown does not name its adjacent range.");
                    ListBox sourcePriority = setup.Controls.Find("artworkSourcePriority", true).OfType<ListBox>().Single();
                    Assert(sourcePriority.Items.Count == ConfigState.ArtworkSources.Length
                        && Convert.ToString(sourcePriority.Items[0]) == "Deezer",
                        "Artwork source priority is not visible/editable in Python's default order.");
                    Assert(setup.Icon != null, "The SPLINED application icon was not applied to Settings.");
                    VerifyRetentionLayout(setup, advanced, pathsTab);
                }

                object[] matchFixture =
                {
                    new Dictionary<string, object>
                    {
                        { "index", 1 }, { "decade", "2010s" }, { "release_class", "album" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "Fixture Album" },
                        { "release_date", "2020-01-01" }, { "country", "GB" },
                        { "current", true },
                        { "recording_mbid", "59a0c68f-ec68-418d-a29a-fa54a7d9aea9" },
                        { "artist_mbids", new ArrayList { "291dcfb8-b31c-496a-905b-9955509d75b6" } },
                        { "release_mbid", "5d05694f-2b0f-427e-9df8-78dbc0983681" },
                        { "release_group_mbid", "420c6768-0685-415a-bb59-d6a275121125" },
                        { "url", "https://musicbrainz.org/release/5d05694f-2b0f-427e-9df8-78dbc0983681" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 2 }, { "decade", "2000s" }, { "release_class", "ep" },
                        { "release_artist", "Various Artists" }, { "release_title", "Fixture EP" },
                        { "release_date", "2024" }, { "country", "US" },
                        { "visited", true },
                        { "release_mbid", "9e8005ec-0ee4-4c64-8431-cb315c2c5742" },
                        { "url", "https://musicbrainz.org/release/9e8005ec-0ee4-4c64-8431-cb315c2c5742" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 3 }, { "decade", "2000s" }, { "release_class", "album" },
                        { "release_artist", "Various Artists" }, { "release_title", "Fixture Compilation" },
                        { "release_date", "2020" }, { "country", "GB" },
                        { "release_mbid", "0c9bcf05-ddb3-4377-aab5-1c0a2264d55a" },
                        { "url", "https://musicbrainz.org/release/0c9bcf05-ddb3-4377-aab5-1c0a2264d55a" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 4 }, { "decade", "2020s" }, { "release_class", "single" },
                        { "release_artist", "Zed Artist" }, { "release_title", "Zed Single" },
                        { "release_date", "2025" }, { "country", "US" },
                        { "release_mbid", "11111111-1111-4111-8111-111111111111" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 5 }, { "decade", "Unknown" }, { "release_class", "ep" },
                        { "release_artist", "Alpha Artist" }, { "release_title", "Unknown Date EP" },
                        { "release_date", "" }, { "country", "" },
                        { "release_mbid", "22222222-2222-4222-8222-222222222222" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 6 }, { "decade", "1990s" }, { "release_class", "soundtrack" },
                        { "release_artist", "Various Artists" }, { "release_title", "Fixture Soundtrack" },
                        { "release_date", "1998" }, { "country", "GB" },
                        { "release_mbid", "33333333-3333-4333-8333-333333333333" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 7 }, { "decade", "2020s" }, { "release_class", "compilation" },
                        { "release_artist", "Named Compilation Artist" }, { "release_title", "Fixture Compilation Family" },
                        { "release_date", "2022" }, { "country", "CA" },
                        { "release_mbid", "44444444-4444-4444-8444-444444444444" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 8 }, { "decade", "2020s" }, { "release_class", "single" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "Newest Fixture" },
                        { "release_date", "2021" }, { "country", "" },
                        { "release_mbid", "55555555-5555-4555-8555-555555555555" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 9 }, { "decade", "2020s" }, { "release_class", "ep" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "US Fixture" },
                        { "release_date", "2020-01-01" }, { "country", "US" },
                        { "release_mbid", "66666666-6666-4666-8666-666666666666" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 10 }, { "decade", "2020s" }, { "release_class", "album" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "Canada Fixture" },
                        { "release_date", "2020-01-01" }, { "country", "CA" },
                        { "release_mbid", "77777777-7777-4777-8777-777777777777" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 11 }, { "decade", "2010s" }, { "release_class", "album" },
                        { "release_artist", "Alpha Artist" }, { "release_title", "Known Date Album" },
                        { "release_date", "2010" }, { "country", "DE" },
                        { "release_mbid", "88888888-8888-4888-8888-888888888888" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 12 }, { "decade", "2020s" }, { "release_class", "album" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "Unknown Country Fixture" },
                        { "release_date", "2020-01-01" }, { "country", "" },
                        { "release_mbid", "99999999-9999-4999-8999-999999999999" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 13 }, { "decade", "2020s" }, { "release_class", "album" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "Alpha Tie" },
                        { "release_date", "2020-01-01" }, { "country", "CA" },
                        { "release_mbid", "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb" }
                    },
                    new Dictionary<string, object>
                    {
                        { "index", 14 }, { "decade", "2020s" }, { "release_class", "album" },
                        { "release_artist", "Fixture Artist" }, { "release_title", "Alpha Tie" },
                        { "release_date", "2020-01-01" }, { "country", "CA" },
                        { "release_mbid", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" }
                    }
                };
                using (FluentForm matchesHost = new FluentForm())
                {
                    MusicBrainzMatchesPanel matches = new MusicBrainzMatchesPanel(
                        "Fixture Artist", "Fixture Track", "Fixture Album Artist", "Fixture Album", matchFixture,
                        "7b0e3436-afe7-4da7-8d41-b793b8d84b51",
                        "5f9ee42f-84b1-42bb-a318-09a05b3fcde1",
                        "ac8e3469-8f18-4ffc-8581-ade501ae0dc5",
                        false, loadedUi.Theme);
                    matchesHost.Controls.Add(matches);
                    matchesHost.ShowInTaskbar = false;
                    matchesHost.StartPosition = FormStartPosition.Manual;
                    matchesHost.Location = new Point(-32000, -32000);
                    matchesHost.Show();
                    Application.DoEvents();
                    Assert(Descendants(matches).OfType<TextBox>().Count() == 3
                        && Descendants(matches).OfType<Button>().Any(button => button.Text == "Apply IDs")
                        && Descendants(matches).OfType<Button>().Any(button => button.Text == "Return to Source Results"),
                        "MusicBrainz Matches omitted session-only Artist/Release/Recording authority editing or normal-Album return navigation.");
                    Assert(!Descendants(matches).OfType<Button>().Any(button => button.Text == "Search Artist / Track")
                        && Descendants(matches).OfType<Button>().Any(button => button.Text == "Filter by Artist ▼")
                        && Descendants(matches).OfType<Button>().Any(button => button.Text == "Filter by Release Type ▼"),
                        "MusicBrainz Matches retained free-text search or omitted the dependent row filters.");
                    Assert(Descendants(matches).OfType<TextBox>().Any(box => box.Text == "5f9ee42f-84b1-42bb-a318-09a05b3fcde1")
                        && !Descendants(matches).OfType<TextBox>().Any(box => box.Text == "291dcfb8-b31c-496a-905b-9955509d75b6"),
                        "MusicBrainz authority fields did not use the queried track authority supplied above the results.");
                    ListView matchList = Descendants(matches).OfType<ListView>().Single();
                    Assert(matchList.Items[0].Name == "musicBrainzCurrentAlbumCategory"
                        && matchList.Items[0].SubItems[1].Text == "[CURRENT ALBUM]"
                        && matchList.Items[1].Text == "[*]"
                        && matchList.Items[1].SubItems[4].Text == ""
                        && matchList.Items.Cast<ListViewItem>()
                            .SelectMany(row => row.SubItems.Cast<ListViewItem.ListViewSubItem>())
                            .Count(cell => cell.Text == "[CURRENT ALBUM]") == 1
                        && matchList.Items[0].ForeColor.ToArgb() == ThemeManager.CurrentPalette.Warning.ToArgb()
                        && matchList.Items[1].ForeColor.ToArgb() == ThemeManager.CurrentPalette.Warning.ToArgb(),
                        "MusicBrainz Matches did not show CURRENT ALBUM exactly once while keeping its yellow [*] authority row first.");
                    string[] sortedResultIndexes = matchList.Items.Cast<ListViewItem>()
                        .Where(row => row.Tag is Dictionary<string, object> && row.Text != "[*]")
                        .Select(row => row.Text)
                        .ToArray();
                    Assert(sortedResultIndexes.SequenceEqual(new[] { "11", "5", "8", "9", "14", "13", "10", "1", "12", "4", "6", "7", "2", "3" })
                        && matchList.Items.Cast<ListViewItem>()
                            .Where(row => row.Name == "musicBrainzResultCategory")
                            .All(row => row.SubItems[1].Text.StartsWith("RELEASE TYPE [", StringComparison.Ordinal)
                                && row.SubItems[1].Text.IndexOf("2010s", StringComparison.Ordinal) < 0
                                && row.SubItems[1].Text.IndexOf("2020s", StringComparison.Ordinal) < 0),
                        "MusicBrainz result rows were not sorted by Artist/family, Date, Country, title, and MBID with release-type-only headings.");
                    Assert(matchList.Items.Cast<ListViewItem>().Any(row => row.Tag == matchFixture[0]
                        && row.ForeColor.ToArgb() == ThemeManager.CurrentPalette.Success.ToArgb())
                        && matchList.Items.Cast<ListViewItem>().Any(row => row.Tag == matchFixture[1]
                        && row.ForeColor.ToArgb() == ThemeManager.CurrentPalette.StatusOrange.ToArgb()),
                        "MusicBrainz current/visited rows did not use green and orange state colors.");
                    string originalResultUrl = Convert.ToString(((Dictionary<string, object>)matchFixture[0])["url"]);
                    TextBox releaseAuthority = Descendants(matches).OfType<TextBox>()
                        .Single(box => box.Text == "ac8e3469-8f18-4ffc-8581-ade501ae0dc5");
                    releaseAuthority.Text = "4f725973-aaf1-4d0d-a775-0a90ed2a0757";
                    Descendants(matches).OfType<Button>().Single(button => button.Text == "Apply IDs").PerformClick();
                    Dictionary<string, object> authorityRow = (Dictionary<string, object>)matchList.Items[1].Tag;
                    Assert(Convert.ToString(authorityRow["url"]).EndsWith("/4f725973-aaf1-4d0d-a775-0a90ed2a0757")
                        && Convert.ToString(((Dictionary<string, object>)matchFixture[0])["url"]) == originalResultUrl,
                        "Apply IDs changed a result URL or failed to update only the authority row link.");
                    ListViewItem selectedEp = matchList.Items.Cast<ListViewItem>().Single(row => row.Tag == matchFixture[1]);
                    selectedEp.Selected = true;
                    matches.ApplyFilterSelection(new[] { "Various Artists" }, new[] { "ep" });
                    Assert(matchList.Items[0].Name == "musicBrainzCurrentAlbumCategory" && matchList.Items[1].Text == "[*]"
                        && matchList.Items.Cast<ListViewItem>().Count(row => row.Tag is Dictionary<string, object>
                            && !Object.ReferenceEquals(row.Tag, authorityRow)) == 1
                        && matchList.Items.Cast<ListViewItem>().Any(row => row.Tag == matchFixture[1] && row.Selected)
                        && matchList.Items.Cast<ListViewItem>().Where(row => row.Name == "musicBrainzResultCategory")
                            .All(row => row.SubItems[1].Text == "RELEASE TYPE [EP]")
                        && matches.ReleaseTypeFilterChoices.SequenceEqual(new[] { "ep", "album", "soundtrack" }),
                        "MusicBrainz Artist/Release Type filters did not combine actual row values, preserve selection, or rebuild headings.");
                    string[] previewUrls = MusicBrainzMatchesPanel.ArtworkPreviewUrls((Dictionary<string, object>)matchFixture[0]);
                    Assert(previewUrls.Length == 2
                        && previewUrls[0].Contains("/release-group/420c6768-0685-415a-bb59-d6a275121125/front")
                        && previewUrls[1].Contains("/release/5d05694f-2b0f-427e-9df8-78dbc0983681/front"),
                        "MusicBrainz artwork preview did not prefer release-group front art with exact-release fallback.");
                }

                Dictionary<string, object> fanartCredential = new Dictionary<string, object>
                {
                    { "api_key", "fixture-project-key" }, { "client_key", "" }, { "api_version", "v3.2" }
                };
                CredentialStore.ClearValidationMetadata(fanartCredential);
                CredentialStore.Save(loaded, "fanarttv", fanartCredential);
                Assert(CredentialStore.Status(loaded, "fanarttv") == "v3.2 SAVED",
                    "An untested Fanart.tv credential must not be reported as live/tested.");
                CredentialStore.ApplyValidationMetadata(fanartCredential, new CredentialValidationResult
                {
                    Success = true, ApiVersion = "v3.2", Endpoint = "Fanart.tv v3.2 album endpoint"
                });
                CredentialStore.Save(loaded, "fanarttv", fanartCredential);
                Assert(CredentialStore.Status(loaded, "fanarttv") == "v3.2 TESTED",
                    "A validated Fanart.tv v3.2 credential was not identified.");
                bool credentialProtected = CredentialStore.Save(loaded, "lastfm", new Dictionary<string, object> { { "api_key", "fixture-lastfm-key" } });
                string protectedCredentialPath = CredentialStore.PathFor(loaded, "lastfm");
                FileSecurity protectedCredentialAcl = File.GetAccessControl(protectedCredentialPath);
                SecurityIdentifier currentUserSid = WindowsIdentity.GetCurrent().User;
                bool currentUserHasFullControl = protectedCredentialAcl
                    .GetAccessRules(true, false, typeof(SecurityIdentifier))
                    .Cast<FileSystemAccessRule>()
                    .Any(rule => currentUserSid != null
                        && currentUserSid.Equals(rule.IdentityReference)
                        && rule.AccessControlType == AccessControlType.Allow
                        && (rule.FileSystemRights & FileSystemRights.FullControl) == FileSystemRights.FullControl);
                Assert(!credentialProtected || (protectedCredentialAcl.AreAccessRulesProtected && currentUserHasFullControl),
                    "Credential save reported protection without a protected user-specific ACL.");
                Assert(File.Exists(protectedCredentialPath),
                    "Credential save failed when ACL protection was unavailable.");
                Assert(CredentialStore.Status(loaded, "lastfm") == "SAVED",
                    "Last.fm artwork reads should require only the API key, not a session.");
                CredentialStore.Save(loaded, "musicbrainz", new Dictionary<string, object>
                {
                    { "oauth_enabled", true }, { "client_id", "fixture-client" }, { "client_secret", "fixture-secret" },
                    { "callback_uri", "urn:ietf:wg:oauth:2.0:oob" }, { "oauth_scope", "profile" },
                    { "access_token", "fixture-access" }, { "refresh_token", "fixture-refresh" },
                    { "future_auth_field", "preserve-root" },
                    { "options", new Dictionary<string, object>
                        {
                            { "retry_max", 6 }, { "min_delay", 1.25 }, { "recording_timeout", 11 },
                            { "future_option", "preserve-option" }
                        }
                    }
                });
                Assert(CredentialStore.Status(loaded, "musicbrainz") == "TOKEN SAVED",
                    "MusicBrainz OAuth token presence was not shown accurately.");
                MusicBrainzRuntimeOptions musicBrainzOptions = CredentialStore.LoadMusicBrainzOptions(loaded);
                Assert(musicBrainzOptions.RetryMax == 6 && Math.Abs(musicBrainzOptions.MinDelay - 1.25) < 0.001
                    && musicBrainzOptions.RecordingTimeout == 11,
                    "Existing custom MusicBrainz options did not load.");
                musicBrainzOptions.RetryMax = 8;
                CredentialStore.MergeMusicBrainzOptions(loaded, musicBrainzOptions);
                Dictionary<string, object> savedMusicBrainz = CredentialStore.Load(loaded, "musicbrainz");
                Dictionary<string, object> savedMusicBrainzOptions = (Dictionary<string, object>)savedMusicBrainz["options"];
                Assert(Convert.ToString(savedMusicBrainz["access_token"]) == "fixture-access"
                    && Convert.ToInt32(savedMusicBrainzOptions["retry_max"]) == 8
                    && Math.Abs(Convert.ToDouble(savedMusicBrainzOptions["min_delay"]) - 1.25) < 0.001
                    && Convert.ToInt32(savedMusicBrainzOptions["recording_timeout"]) == 11,
                    "Changing retry_max modified another MusicBrainz credential or option value.");
                Assert(Convert.ToString(savedMusicBrainz["future_auth_field"]) == "preserve-root"
                    && Convert.ToString(savedMusicBrainzOptions["future_option"]) == "preserve-option",
                    "Unknown MusicBrainz credential fields did not survive the option merge.");
                CredentialStore.Save(loaded, "musicbrainz", new Dictionary<string, object> { { "client_id", "fixture-client-updated" } });
                savedMusicBrainz = CredentialStore.Load(loaded, "musicbrainz");
                savedMusicBrainzOptions = (Dictionary<string, object>)savedMusicBrainz["options"];
                Assert(Convert.ToString(savedMusicBrainz["access_token"]) == "fixture-access"
                    && Convert.ToInt32(savedMusicBrainzOptions["retry_max"]) == 8
                    && Math.Abs(Convert.ToDouble(savedMusicBrainzOptions["min_delay"]) - 1.25) < 0.001
                    && Convert.ToInt32(savedMusicBrainzOptions["recording_timeout"]) == 11,
                    "Changing MusicBrainz authentication removed runtime options or the existing token.");

                loaded.SourcePolicies["musicbrainz"].Enabled = false;
                loaded.SourcePolicies["musicbrainz"].StrictOverride = true;
                loaded.SourcePolicies["musicbrainz"].SourceOverride = true;
                ConfigStore.Save(loaded);
                ConfigState reopened = ConfigStore.Load();
                Assert(!reopened.SourcePolicies["musicbrainz"].Enabled
                    && reopened.SourcePolicies["musicbrainz"].StrictOverride
                    && reopened.SourcePolicies["musicbrainz"].SourceOverride,
                    "MusicBrainz Source Enabled / Strict Override / Source Override did not round-trip.");
                Dictionary<string, object> afterPolicySave = CredentialStore.Load(reopened, "musicbrainz");
                Dictionary<string, object> afterPolicyOptions = (Dictionary<string, object>)afterPolicySave["options"];
                Assert(Convert.ToString(afterPolicySave["access_token"]) == "fixture-access"
                    && Convert.ToInt32(afterPolicyOptions["retry_max"]) == 8
                    && Math.Abs(Convert.ToDouble(afterPolicyOptions["min_delay"]) - 1.25) < 0.001
                    && Convert.ToInt32(afterPolicyOptions["recording_timeout"]) == 11,
                    "Saving MusicBrainz source policy modified credentials or runtime options.");
                using (SetupForm musicBrainzSetup = new SetupForm(reopened, false))
                {
                    musicBrainzSetup.ShowInTaskbar = false;
                    musicBrainzSetup.StartPosition = FormStartPosition.Manual;
                    musicBrainzSetup.Location = new Point(-32000, -32000);
                    musicBrainzSetup.Show();
                    TabControl primary = (TabControl)typeof(SetupForm).GetField("primaryTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(musicBrainzSetup);
                    TabControl advanced = (TabControl)typeof(SetupForm).GetField("advancedTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(musicBrainzSetup);
                    primary.SelectedIndex = 1;
                    for (int index = 0; index < advanced.TabPages.Count; index++)
                        if (advanced.TabPages[index].Text == "Sources & Matching") advanced.SelectedIndex = index;
                    ComboBox selector = (ComboBox)typeof(SetupForm).GetField("sourceSelector", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(musicBrainzSetup);
                    for (int index = 0; index < selector.Items.Count; index++)
                        if (Convert.ToString(selector.Items[index]) == "MusicBrainz / CAA") selector.SelectedIndex = index;
                    Application.DoEvents();
                    Assert((int)musicBrainzSetup.Controls.Find("musicBrainzRetryMax", true).OfType<NumericUpDown>().Single().Value == 8
                        && Math.Abs((double)musicBrainzSetup.Controls.Find("musicBrainzMinDelay", true).OfType<NumericUpDown>().Single().Value - 1.25) < 0.001
                        && (int)musicBrainzSetup.Controls.Find("musicBrainzRecordingTimeout", true).OfType<NumericUpDown>().Single().Value == 11,
                        "Reopened Settings did not repopulate all MusicBrainz runtime options.");
                    Assert(musicBrainzSetup.Controls.Find("musicBrainzOptionsGroup", true).Single().Visible
                        && musicBrainzSetup.Controls.Find("artworkResolutionGroup", true).Single().Visible,
                        "MusicBrainz did not expose both request options and artwork policy controls.");
                }
                int fanartCovers;
                Assert(ProviderCredentialValidator.IsFanartV32AlbumResponse(
                    "{\"albums\":[{\"release_group_id\":\"1b022e01-4da6-387b-8658-8678046e4cef\",\"albumcover\":[{\"url\":\"https://example.test/cover.jpg\"}]}]}",
                    out fanartCovers) && fanartCovers == 1,
                    "Fanart.tv v3.2 albums-array validation failed.");
                Assert(!ProviderCredentialValidator.IsFanartV32AlbumResponse(
                    "{\"albums\":{\"legacy\":{\"albumcover\":[]}}}", out fanartCovers),
                    "Fanart.tv v3 object response was incorrectly accepted as v3.2.");
                using (CredentialsForm credentials = new CredentialsForm(loaded))
                {
                    Assert(credentials.Controls.Find("fanarttvTestLink", true).Length == 1
                        && credentials.Controls.Find("lastfmTestLink", true).Length == 1
                        && credentials.Controls.Find("discogsTestLink", true).Length == 1
                        && credentials.Controls.Find("musicbrainzTestLink", true).Length == 1,
                        "Credential status does not expose all four live test links.");
                }
                using (CredentialEditForm fanartEdit = new CredentialEditForm(loaded, "fanarttv", "Fanart.tv"))
                {
                    Assert(fanartEdit.Controls.Find("fanarttvApiIdentity", true).OfType<Label>().Single().Text.Contains("v3.2")
                        && fanartEdit.Controls.Find("fanarttvCredentialTestButton", true).Length == 1,
                        "Fanart.tv credential editor does not identify and test API v3.2.");
                }

                VerifyThemeSystem();
                VerifyWatermarkAndIcon();
                using (AboutForm about = new AboutForm())
                {
                    Assert(about.Text == "About S:P:L:I:N:E:D"
                        && Descendants(about).OfType<Label>().Any(label => label.Text == "S:P:L:I:N:E:D")
                        && Descendants(about).OfType<Label>().Any(label => label.Text == "v3.0.0 Stable")
                        && !Descendants(about).OfType<Label>().Any(label => label.Text == "Stable v3"
                            || label.Text.StartsWith("Windows version", StringComparison.OrdinalIgnoreCase)),
                        "The About surface does not expose the concise v3.0.0 Stable identity.");
                    Assert(Descendants(about).OfType<Button>().Select(button => button.Text)
                            .OrderBy(text => text).SequenceEqual(new[] { "Close", "Help" }),
                        "About must expose only Help and Close actions.");
                    Assert(Descendants(about).OfType<Button>().All(button => button is FluentButton),
                        "The About surface bypassed the shared Fluent action-button treatment.");
                }

                MethodInfo quoteArgument = typeof(MainForm).GetMethod("QuoteArgument", BindingFlags.Static | BindingFlags.NonPublic);
                string unc = @"\\server\share\music\Artist\Album";
                string quotedUnc = (string)quoteArgument.Invoke(null, new object[] { unc });
                Assert(quotedUnc == "\"" + unc + "\"", "UNC command argument changed its leading backslashes.");
                MethodInfo resolveUnicodePath = typeof(MainForm).GetMethod("ResolveExistingAlbumPath", BindingFlags.Static | BindingFlags.NonPublic);
                string requestedUnicodeAlbum = Path.Combine(unicodeArtist, "Café — Big Bambú");
                string resolvedUnicodeAlbum = (string)resolveUnicodePath.Invoke(null, new object[] { requestedUnicodeAlbum });
                Assert(Directory.Exists(resolvedUnicodeAlbum)
                    && resolvedUnicodeAlbum.Normalize(System.Text.NormalizationForm.FormC)
                        == unicodeAlbum.Normalize(System.Text.NormalizationForm.FormC),
                    "Equivalent composed/decomposed French and accented Album paths were not resolved to the physical directory.");
                string indexedMojibakeAlbum = Path.Combine(library, "Cheech & Chong", "Big Bambú");
                string resolvedMojibakeAlbum = (string)resolveUnicodePath.Invoke(null, new object[] { indexedMojibakeAlbum });
                Assert(resolvedMojibakeAlbum == mojibakeAlbum && Directory.Exists(resolvedMojibakeAlbum),
                    "UTF-8/Windows-1252 mojibake Album names were not translated to the physical directory.");
                string stableManifest = "https://github.com/scottia/S-P-L-I-N-E-D/releases/download/1.0.18/windows-update.json";
                string stableArchive = "https://github.com/scottia/S-P-L-I-N-E-D/releases/download/1.0.18/splined-windows-x86_64.zip";
                string stableRelease = "https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/1.0.18";
                Assert(WindowsUpdateService.IsApprovedStableManifestUrl(stableManifest)
                    && WindowsUpdateService.IsApprovedReleasePageUrl(stableRelease)
                    && !WindowsUpdateService.IsApprovedReleasePageUrl(
                        "https://github.com.evil.invalid/scottia/S-P-L-I-N-E-D/releases/tag/1.0.18"),
                    "Official versioned Windows notification assets or release pages are not isolated to GitHub.");
                string releasesJson = "["
                    + "{\"draft\":false,\"prerelease\":true,\"tag_name\":\"preview\",\"html_url\":\"https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/preview\",\"assets\":[]},"
                    + "{\"draft\":false,\"prerelease\":false,\"tag_name\":\"1.0.18\",\"html_url\":\"" + stableRelease + "\",\"assets\":["
                    + "{\"name\":\"windows-update.json\",\"browser_download_url\":\"" + stableManifest + "\"},"
                    + "{\"name\":\"splined-windows-x86_64.zip\",\"browser_download_url\":\"" + stableArchive + "\"}]}]";
                WindowsUpdateLocation stableLocation = WindowsUpdateService.SelectStableUpdateLocation(releasesJson);
                Assert(stableLocation.ManifestUrl == stableManifest && stableLocation.ReleaseUrl == stableRelease,
                    "Stable update discovery did not select the official archive notification and release page.");
                WindowsUpdateService.ValidateManifest(new WindowsUpdateManifest
                {
                    schema = 2,
                    channel = "stable",
                    commit = "0123456789abcdef0123456789abcdef01234567",
                    short_commit = "0123456",
                    release_url = stableRelease
                }, stableLocation);

                loadedUi.HoverEnabled = false;
                loadedUi.ShowMediaSelector = true;
                loadedUi.CandidateFilterExpanded = false;
                ConfigStore.SaveUi(loadedUi);
                using (MainForm form = new MainForm(loaded))
                {
                    Assert(form.Text == ReleaseInfo.WindowTitle && form.Text == "S:P:L:I:N:E:D"
                        && form.Text.IndexOf("Beta", StringComparison.OrdinalIgnoreCase) < 0
                        && form.Text.IndexOf("RC", StringComparison.OrdinalIgnoreCase) < 0,
                        "The running Windows title is not sourced from the Stable v3 release identity.");
                    Assert(form.BackColor == ThemeManager.CurrentPalette.WindowBackground,
                        "The main window was not themed before its first visible frame.");
                    Assert(!Descendants(form).OfType<Label>().Any(label => label.Text.StartsWith("Media Library:", StringComparison.OrdinalIgnoreCase)),
                        "The removed Media Library path is still displayed in the top navigation row.");
                    Assert(form.Icon != null, "The SPLINED application icon was not applied to the main window.");
                    Assert(Descendants(form).OfType<Button>().Where(button => !(button is InfoButton)).All(button => button is FluentButton),
                        "A main-window action control bypassed the shared owner-painted FluentButton geometry.");
                    FieldInfo launchField = typeof(MainForm).GetField("launch", BindingFlags.Instance | BindingFlags.NonPublic);
                    Button launch = (Button)launchField.GetValue(form);
                    Assert(!launch.Enabled && Convert.ToString(launch.Tag) == "launch",
                        "LAUNCH must be disabled with an empty selection and retain the primary-action visual role.");
                    Control candidateActionRow = form.Controls.Find("candidateActionRow", true).Single();
                    Assert(Object.ReferenceEquals(launch.Parent, candidateActionRow)
                        && candidateActionRow.Controls.GetChildIndex(launch) == 0,
                        "The main LAUNCH control is not the first button beside the Artwork Filter.");
                    Assert(form.MainMenuStrip.Items.OfType<ToolStripMenuItem>()
                            .Any(item => item.Text == "Help" && item.Available),
                        "Help/About was pushed into the compact header overflow instead of remaining visibly reachable.");
                    ToolStripMenuItem helpMenu = form.MainMenuStrip.Items.OfType<ToolStripMenuItem>()
                        .Single(item => item.Text == "Help");
                    Assert(helpMenu.DropDownItems.OfType<ToolStripMenuItem>().Select(item => item.Text)
                            .SequenceEqual(new[] { "Help", "Check for Update...", "About..." })
                        && !helpMenu.DropDownItems.OfType<ToolStripMenuItem>().Any(item =>
                            item.Text.IndexOf("Documentation", StringComparison.OrdinalIgnoreCase) >= 0
                            || item.Text.IndexOf("Python", StringComparison.OrdinalIgnoreCase) >= 0
                            || item.Text.IndexOf("Coverage", StringComparison.OrdinalIgnoreCase) >= 0),
                        "The Help menu still exposes duplicate Documentation/Python coverage surfaces.");
                    Assert(form.MainMenuStrip is FluentMenuStrip
                        && form.MainMenuStrip.AutoSize && form.MainMenuStrip.Dock == DockStyle.Fill
                        && Descendants(form).OfType<SplinedWordmark>().Any(),
                        "The top menu does not use the centralized first-frame Fluent menu surface.");
                    form.MainMenuStrip.CreateControl();
                    form.MainMenuStrip.PerformLayout();
                    using (Bitmap menuBitmap = new Bitmap(Math.Max(1, form.MainMenuStrip.Width), Math.Max(1, form.MainMenuStrip.Height)))
                    {
                        form.MainMenuStrip.DrawToBitmap(menuBitmap, new Rectangle(Point.Empty, menuBitmap.Size));
                        Color menuBackground = ThemeManager.CurrentPalette.TopNavigationSurface;
                        int visibleMenuPixels = 0;
                        for (int y = 0; y < menuBitmap.Height; y += 2)
                            for (int x = 0; x < menuBitmap.Width; x += 2)
                            {
                                Color pixel = menuBitmap.GetPixel(x, y);
                                if (Math.Abs(pixel.R - menuBackground.R) + Math.Abs(pixel.G - menuBackground.G) + Math.Abs(pixel.B - menuBackground.B) > 45)
                                    visibleMenuPixels++;
                            }
                        Assert(visibleMenuPixels > 20,
                            "File / View / Status / Help do not paint visibly against the title/navigation surface.");
                    }
                    ToolStripMenuItem viewMenu = form.MainMenuStrip.Items.OfType<ToolStripMenuItem>()
                        .Single(item => item.Text == "View");
                    ToolStripMenuItem appearanceMenu = viewMenu.DropDownItems.OfType<ToolStripMenuItem>()
                        .Single(item => item.Text == "Appearance");
                    ToolStripMenuItem showArtwork = viewMenu.DropDownItems.OfType<ToolStripMenuItem>()
                        .Single(item => item.Text == "Show Artwork");
                    ToolStripDropDownMenu appearanceDropDown = appearanceMenu.DropDown as ToolStripDropDownMenu;
                    Assert(form.MainMenuStrip.Renderer is FluentMenuRenderer
                        && viewMenu.DropDown.Renderer is FluentMenuRenderer
                        && appearanceMenu.DropDown.Renderer is FluentMenuRenderer
                        && appearanceDropDown != null && appearanceDropDown.ShowCheckMargin && !appearanceDropDown.ShowImageMargin,
                        "View / Appearance is not using the shared rounded menu renderer and custom theme-selection margin.");
                    Assert(appearanceMenu.DropDownItems.OfType<ToolStripMenuItem>().Count() == 3
                        && appearanceMenu.DropDownItems.OfType<ToolStripMenuItem>().Select(item => item.Text).SequenceEqual(new[] { "System", "Dark", "Light" })
                        && appearanceMenu.DropDownItems.OfType<ToolStripMenuItem>().All(item => item.Padding.Top >= ThemeManager.Space4
                            && item.Padding.Bottom >= ThemeManager.Space4 && item.Margin.Left >= ThemeManager.Space4),
                        "System / Light / Dark do not use the shared Fluent Compact theme-menu spacing.");
                    TableLayoutPanel artworkWorkspace = form.Controls.Find("activityWorkspace", true).OfType<TableLayoutPanel>().Single();
                    Assert(showArtwork.Checked && artworkWorkspace.ColumnStyles[1].Width > 0,
                        "View / Show Artwork did not restore the persisted top-right artwork panel.");
                    artworkWorkspace.Size = new Size(900, 240);
                    typeof(MainForm).GetMethod("UpdateArtworkSquareLayout", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(form, new object[] { false });
                    Assert((int)artworkWorkspace.ColumnStyles[1].Width == 240 + ThemeManager.Space4,
                        "The embedded Artwork column did not remain square with the Activity workspace height.");
                    Assert(!Descendants(form).OfType<Label>().Any(label => label.Text == "Choose Select, All, or None. Launch uses checked albums only."
                            || label.Text == "Live discovery, validation, provider results, and write/read outcomes appear here."
                            || label.Text == "Real provider candidates for the current album will appear below."),
                        "A removed panel helper sentence is still consuming visible layout space.");
                    foreach (string titleText in new[] { "Media Library Selection", "Scan Activity and Decisions" })
                    {
                        Label titleLabel = Descendants(form).OfType<Label>().Single(label => label.Text == titleText);
                        Assert(titleLabel.Parent.Controls.OfType<InfoButton>().Any(), titleText + " does not have its replacement information tooltip beside the heading.");
                    }
                    Button artworkFilter = form.Controls.Find("candidateFilterButton", true).OfType<Button>().Single();
                    FluentCardPanel artworkFilterPanel = (FluentCardPanel)typeof(MainForm)
                        .GetField("candidateFilterPanel", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Assert(artworkFilter is SpectrumToggleButton && artworkFilter.Text.Contains("Artwork Filter")
                        && artworkFilterPanel.VisualRole == CardVisualRole.SpectrumNested,
                        "Artwork Candidates did not use the Media Selection-style dropdown and spectrum filter frame.");
                    MethodInfo invokeArtworkFilter = typeof(Button).GetMethod("OnClick", BindingFlags.Instance | BindingFlags.NonPublic);
                    invokeArtworkFilter.Invoke(artworkFilter, new object[] { EventArgs.Empty });
                    Assert(artworkFilterPanel.Parent == null && artworkFilter.Text.Contains("▸") && !artworkFilter.Enabled,
                        "Artwork Filter opened a blank expanded surface before candidate results existed.");
                    FluentCardTableLayoutPanel libraryCard = form.Controls.Find("mediaLibrarySelectionCard", true).Single() as FluentCardTableLayoutPanel;
                    FluentCardTableLayoutPanel activityPanel = form.Controls.Find("scanActivityCard", true).Single() as FluentCardTableLayoutPanel;
                    FluentCardTableLayoutPanel candidatesPanel = form.Controls.Find("artworkCandidatesCard", true).Single() as FluentCardTableLayoutPanel;
                    Assert(libraryCard != null && activityPanel != null && candidatesPanel != null,
                        "The three major work areas do not use the shared rounded panel surface.");
                    Button selectorVisibility = form.Controls.Find("mediaSelectorVisibilityToggle", true).OfType<Button>().Single();
                    Button selectorExpand = form.Controls.Find("mediaSelectorExpandToggle", true).OfType<Button>().Single();
                    Assert(selectorVisibility is SpectrumToggleButton && selectorVisibility.Text == "«"
                        && selectorExpand is SpectrumToggleButton && selectorExpand.Text == "≫"
                        && selectorVisibility.Font.SizeInPoints == selectorExpand.Font.SizeInPoints
                        && selectorVisibility.Font.SizeInPoints >= 13f
                        && selectorVisibility.Width >= 40
                        && selectorVisibility.Padding.All == 0,
                        "The Media Library Selection show/hide glyph is not visible at the panel-title font size.");
                    CheckBox showTracksControl = form.Controls.Find("showTracks", true).OfType<CheckBox>().Single();
                    Assert(showTracksControl.Text == "Show Tracks"
                        && showTracksControl.Parent == form.Controls.Find("mediaStatusFilters", true).Single().Controls[0],
                        "Show Tracks is not present in the Folder status selector.");
                    SplitContainer selectorSplit = (SplitContainer)typeof(MainForm).GetField("mainSplit", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    ToolStripMenuItem showMediaSelector = viewMenu.DropDownItems.OfType<ToolStripMenuItem>()
                        .Single(item => item.Text == "Show Media Album Selector");
                    invokeArtworkFilter.Invoke(selectorVisibility, new object[] { EventArgs.Empty });
                    Control selectorRail = form.Controls.Find("mediaLibrarySelectionCollapsedRail", true).Single();
                    Assert(!selectorSplit.Panel1Collapsed && selectorSplit.SplitterDistance <= 58
                        && selectorRail.Parent.Controls.GetChildIndex(selectorRail) == 0
                        && !libraryCard.Visible && !showMediaSelector.Checked,
                        "The Media Album Selector top-right control did not reduce the panel to its reopen rail."
                        + " collapsed=" + selectorSplit.Panel1Collapsed
                        + " distance=" + selectorSplit.SplitterDistance
                        + " rail=" + selectorRail.Visible
                        + " content=" + libraryCard.Visible
                        + " menu=" + showMediaSelector.Checked);
                    showMediaSelector.PerformClick();
                    Assert(!selectorSplit.Panel1Collapsed && selectorSplit.SplitterDistance >= 180
                        && libraryCard.Parent.Controls.GetChildIndex(libraryCard) == 0
                        && !selectorRail.Visible && showMediaSelector.Checked,
                        "View > Show Media Album Selector did not restore the hidden selector.");
                    TableLayoutPanel libraryScrollCanvas = form.Controls.Find("mediaLibrarySelectionScrollCanvas", true).Single() as TableLayoutPanel;
                    Assert(!libraryCard.AutoScroll && libraryScrollCanvas != null && libraryScrollCanvas.AutoScroll && libraryScrollCanvas.AutoScrollMinSize.Height > 0
                        && activityPanel.AutoScroll && activityPanel.AutoScrollMinSize.Height > 0
                        && candidatesPanel.AutoScroll && candidatesPanel.AutoScrollMinSize.Height > 0,
                        "The framed Media Selection scroll canvas or another independently scrolling work area is missing.");
                    MethodInfo showSelectedAlbum = typeof(MainForm).GetMethod("ShowSelectedAlbum", BindingFlags.Instance | BindingFlags.NonPublic);
                    AlbumInfo previewAlbum = new AlbumInfo { Artist = "Preview Artist", Title = "Preview Album", Path = firstAlbum,
                        ReleaseYear = "1998", TrackCount = 12, RootFiles = 14, CoverName = "cover.jpg", CoverWidth = 1500, CoverHeight = 1500 };
                    showSelectedAlbum.Invoke(form, new object[] { previewAlbum });
                    Label albumInfo = form.Controls.Find("selectedAlbumInfo", true).OfType<Label>().Single();
                    Label artworkCaption = form.Controls.Find("artworkPreviewCaption", true).OfType<Label>().Single();
                    Assert(albumInfo.Text.Contains("Preview Artist") && albumInfo.Text.Contains("Tracks 12")
                        && artworkCaption.Text.Contains("cover.jpg") && artworkCaption.Text.Contains("1500 x 1500"),
                        "Selected Album metadata and cover resolution were not projected into the Activity/Artwork split.");
                    Assert(((FluentCardPanel)form.Controls.Find("selectedAlbumInfoCard", true).Single()).VisualRole == CardVisualRole.SpectrumNested
                        && ((FluentCardTableLayoutPanel)form.Controls.Find("selectedAlbumArtworkCard", true).Single()).VisualRole == CardVisualRole.SpectrumNested,
                        "Album information and Artwork do not use matching spectrum frames.");
                    showSelectedAlbum.Invoke(form, new object[] { null });
                    Assert(albumInfo.Text.Length == 0, "Album information did not clear when no Album has focus.");
                    showSelectedAlbum.Invoke(form, new object[] { previewAlbum });
                    showArtwork.PerformClick();
                    Assert(!showArtwork.Checked && artworkWorkspace.ColumnStyles[1].Width == 0,
                        "View / Show Artwork did not collapse the embedded preview surface.");
                    showArtwork.PerformClick();
                    FieldInfo candidateCardsField = typeof(MainForm).GetField("candidateCards", BindingFlags.Instance | BindingFlags.NonPublic);
                    FlowLayoutPanel candidateCards = (FlowLayoutPanel)candidateCardsField.GetValue(form);
                    Assert(candidateCards.WrapContents && candidateCards.FlowDirection == FlowDirection.LeftToRight,
                        "Candidate cards must wrap into additional rows as the window width changes.");
                    Assert(candidateCards is WatermarkFlowLayoutPanel,
                        "Artwork Candidates does not use the responsive watermark surface.");
                    MethodInfo buildCandidateCard = typeof(MainForm).GetMethod("BuildCandidateCard", BindingFlags.Instance | BindingFlags.NonPublic);
                    using (Control recommendedCard = (Control)buildCandidateCard.Invoke(form, new object[]
                    {
                        new CandidateView { Index = 1, Source = "itunes", Resolution = "1800 x 1800", Range = "ideal", Acceptable = true, Recommended = true }
                    }))
                    {
                        Assert(recommendedCard is FluentCardPanel
                            && ((FluentCardPanel)recommendedCard).VisualRole == CardVisualRole.Recommended,
                            "Recommended artwork does not use the subtle centralized candidate-card accent role.");
                    }
                    MethodInfo showCandidates = typeof(MainForm).GetMethod("ShowCandidates", BindingFlags.Instance | BindingFlags.NonPublic);
                    showCandidates.Invoke(form, new object[] { new object[]
                    {
                        CandidatePayload(9, "discogs", 3200, 3200, false, ""),
                        CandidatePayload(4, "itunes", 1800, 1800, true, ""),
                        CandidatePayload(7, "local", 600, 600, false, "cover-file"),
                        CandidatePayload(8, "amazon", 2400, 2400, false, "")
                    } });
                    int[] displayedOrder = candidateCards.Controls.Cast<Control>().Select(card => (int)card.Tag).ToArray();
                    Assert(displayedOrder.SequenceEqual(new[] { 7, 4, 9, 8 }),
                        "Candidate order is not LOCAL, recommended, then descending resolution.");
                    Assert(((FluentCardPanel)candidateCards.Controls[0]).VisualRole == CardVisualRole.LocalCandidateGlass
                        && ((FluentCardPanel)candidateCards.Controls[1]).VisualRole == CardVisualRole.Recommended
                        && ((FluentCardPanel)candidateCards.Controls[2]).VisualRole == CardVisualRole.CandidateGlass,
                        "Candidate-only purple/green/clear glass roles were not assigned.");
                    PictureBox candidateThumb = candidateCards.Controls[0].Controls["candidateImage"] as PictureBox;
                    Assert(candidateThumb != null && candidateThumb.Width == candidateThumb.Height
                        && candidateThumb.BackColor == ThemeManager.PaletteFor("Dark").SurfacePrimary,
                        "Candidate thumbnails do not preserve square responsive geometry on an opaque, untinted surface.");
                    Assert(!candidateCards.Controls[0].Controls.ContainsKey("candidateQuality")
                        && !candidateCards.Controls[0].Controls.ContainsKey("candidateSource")
                        && candidateCards.Controls[0].Controls["candidateUrl"].Tag is ToolTip,
                        "Candidate status prose was not replaced by the compact hover information tip.");

                    Dictionary<string, object> belowMinimum = CandidatePayload(10, "amazon", 600, 600, false, "");
                    belowMinimum["source_range_class"] = "below-minimum";
                    belowMinimum["range_class"] = "below-minimum";
                    belowMinimum["projected_width"] = 600;
                    belowMinimum["projected_height"] = 600;
                    belowMinimum["upscaled"] = false;
                    Dictionary<string, object> upscalable = CandidatePayload(11, "amazon", 1200, 1200, false, "");
                    upscalable["source_range_class"] = "lower-range";
                    upscalable["range_class"] = "ideal";
                    upscalable["projected_width"] = 1800;
                    upscalable["projected_height"] = 1800;
                    upscalable["upscaled"] = true;
                    string upscalePreviewPath = Path.Combine(internalSettings, "upscale-preview.jpg");
                    using (Bitmap fixture = new Bitmap(12, 12)) fixture.Save(upscalePreviewPath, System.Drawing.Imaging.ImageFormat.Jpeg);
                    previewAlbum.CoverPath = upscalePreviewPath;
                    previewAlbum.LocalArtworkFiles.Add(upscalePreviewPath);
                    previewAlbum.HasLocalArtwork = true;
                    showSelectedAlbum.Invoke(form, new object[] { previewAlbum });
                    Button directSaveExisting = (Button)typeof(MainForm)
                        .GetField("useSelected", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Button directUpscalePreview = (Button)typeof(MainForm)
                        .GetField("upscalePreview", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Dictionary<int, CandidateView> directCandidates = (Dictionary<int, CandidateView>)typeof(MainForm)
                        .GetField("candidates", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Assert(artworkFilter.Enabled && directCandidates.Count == 1
                        && directCandidates.Values.Single().IsLocal && directUpscalePreview.Enabled
                        && directSaveExisting.Text == "Save Existing" && directSaveExisting.Enabled,
                        "Selecting an Album with cover.* did not expose direct Artwork Filter editing before a provider run.");
                    upscalable["cache_path"] = upscalePreviewPath;
                    object[] upscaleCandidates = { belowMinimum, upscalable };
                    showCandidates.Invoke(form, new object[] { upscaleCandidates });
                    Assert(artworkFilter.Enabled, "Artwork Filter did not become available with candidate results.");
                    SplitContainer filterSplit = (SplitContainer)typeof(MainForm)
                        .GetField("rightSplit", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    int filterClosedDistance = filterSplit.SplitterDistance;
                    int filterClosedPanel2Minimum = filterSplit.Panel2MinSize;
                    invokeArtworkFilter.Invoke(artworkFilter, new object[] { EventArgs.Empty });
                    Panel activityHost = form.Controls.Find("activityContentHost", true).OfType<Panel>().Single();
                    Label activityTitle = form.Controls.Find("scanActivityTitle", true).OfType<Label>().Single();
                    Assert(activityHost.Controls.Contains(artworkFilterPanel) && artworkFilter.Text.Contains("▾")
                        && activityTitle.Text == "Candidate Findings | Upscale & Artwork Editing"
                        && filterSplit.SplitterDistance > filterClosedDistance
                        && filterSplit.Panel2MinSize < filterClosedPanel2Minimum,
                        "Artwork Filter did not replace Scan Activity beside Selected Album Artwork.");
                    TableLayoutPanel filterColumns = artworkFilterPanel.Controls.OfType<TableLayoutPanel>().Single();
                    GroupBox upscaleGroup = form.Controls.Find("candidateUpscaleGroup", true).OfType<GroupBox>().Single();
                    Assert(filterColumns.ColumnCount == 5
                        && filterColumns.ColumnStyles.Count == 5 && filterColumns.RowCount == 3
                        && filterColumns.Dock == DockStyle.Fill && !artworkFilterPanel.AutoScroll
                        && filterColumns.GetPositionFromControl(upscaleGroup).Row == 2
                        && filterColumns.GetColumnSpan(upscaleGroup) == 5
                        && new[] { "candidateFindingsGroup", "candidateSourcesGroup", "candidateRangesGroup", "candidateUpscaleGroup" }
                            .All(name => form.Controls.Find(name, true).OfType<FluentGroupBox>().Single().SpectrumBorder),
                        "Artwork Filter is not rendered as four compact spectrum-framed groups without nested scrolling.");
                    TableLayoutPanel previewLine = form.Controls.Find("upscalePreviewLine", true).OfType<TableLayoutPanel>().Single();
                    Panel advancedControls = form.Controls.Find("upscaleAdvancedControls", true).OfType<Panel>().Single();
                    TableLayoutPanel advancedGrid = form.Controls.Find("upscaleAdvancedGrid", true).OfType<TableLayoutPanel>().Single();
                    Button compactPreview = form.Controls.Find("upscalePreviewButton", true).OfType<Button>().Single();
                    Button showFullPreview = form.Controls.Find("upscaleShowFullButton", true).OfType<Button>().Single();
                    Assert(form.Controls.Find("upscaleAdaptiveDefaults", true).OfType<CheckBox>().Single().Checked
                        && new[] { "picture", "sharpen", "softness", "contrast", "exposure", "brightness", "gamma", "temperature" }
                            .All(key => form.Controls.Find("upscaleProfileSlider_" + key, true).OfType<TrackBar>().Single().Orientation == Orientation.Vertical)
                        && form.Controls.Find("upscaleProfileSlider_sharpen", true).OfType<TrackBar>().Single().Value == 0
                        && form.Controls.Find("upscaleProfileSlider_temperature", true).OfType<TrackBar>().Single().Value == 0
                        && form.Controls.Find("upscaleProfileReset_gamma", true).OfType<Button>().Single().Text == "↺"
                        && form.Controls.Find("upscaleProfileFrame_brightness", true).OfType<GroupBox>().Single().Text.Contains("Brightness")
                        && form.Controls.Find("upscaleProfileFrame_temperature", true).OfType<GroupBox>().Single().Text.Contains("Color")
                        && !advancedControls.AutoScroll && advancedGrid.Dock == DockStyle.Fill
                        && advancedGrid.ColumnCount == 8 && advancedGrid.RowCount == 1
                        && Enumerable.Range(0, 8).All(index => ((FluentGroupBox)advancedGrid.GetControlFromPosition(index, 0)).SpectrumBorder)
                        && new[] { "picture", "sharpen", "softness", "contrast", "exposure", "brightness", "gamma", "temperature" }
                            .All(key => form.Controls.Find("upscaleProfileSlider_" + key, true).OfType<TrackBar>().Single().Anchor == AnchorStyles.None)
                        && form.Controls.Find("upscaleProfileControl_brightness", true).Single().Controls.OfType<TableLayoutPanel>().Single().Controls.OfType<Button>().Count() == 1
                        && previewLine.ColumnCount == 3 && previewLine.GetPositionFromControl(showFullPreview).Column == 1
                        && previewLine.ColumnStyles[0].SizeType == SizeType.Absolute
                        && previewLine.ColumnStyles[0].Width == 168 && compactPreview.Width <= 170,
                        "Artwork Filter did not expose the saved Default Upscale / Advanced profile.");
                    Assert(((FluentCardPanel)candidateCards.Controls.Cast<Control>().Single(card => (int)card.Tag == 10)).VisualRole == CardVisualRole.RejectedCandidateGlass
                        && ((FluentCardPanel)candidateCards.Controls.Cast<Control>().Single(card => (int)card.Tag == 11)).VisualRole == CardVisualRole.UpscaleCandidateGlass,
                        "BelowMinimum and policy-qualified Minimum-to-Ideal candidates did not receive red and magenta backgrounds respectively.");
                    CheckBox amazonFilter = form.Controls.Find("candidateFilter_source_amazon", true).OfType<CheckBox>().Single();
                    CheckBox upscaleFilter = form.Controls.Find("candidateFilter_type_Upscalable", true).OfType<CheckBox>().Single();
                    CheckBox rejectedFilter = form.Controls.Find("candidateFilter_type_Rejected", true).OfType<CheckBox>().Single();
                    CheckBox idealFilter = form.Controls.Find("candidateFilter_range_Ideal", true).OfType<CheckBox>().Single();
                    CheckBox lowerFilter = form.Controls.Find("candidateFilter_range_LowerRange", true).OfType<CheckBox>().Single();
                    CheckBox belowFilter = form.Controls.Find("candidateFilter_range_BelowMinimum", true).OfType<CheckBox>().Single();
                    Assert(amazonFilter.Checked && upscaleFilter.Checked && rejectedFilter.Checked
                        && lowerFilter.Checked && belowFilter.Checked
                        && !idealFilter.Enabled && !idealFilter.Checked
                        && Descendants(artworkFilterPanel).OfType<Label>().Any(label => label.Text == "Wanted"),
                        "Inline Candidate FILTER did not expose the result-backed source, type, and unwanted-range choices.");
                    TableLayoutPanel sourceFilterColumn = amazonFilter.Parent as TableLayoutPanel;
                    TableLayoutPanelCellPosition amazonPosition = sourceFilterColumn == null
                        ? new TableLayoutPanelCellPosition(-1, -1)
                        : sourceFilterColumn.GetPositionFromControl(amazonFilter);
                    Control amazonCount = sourceFilterColumn == null ? null
                        : sourceFilterColumn.GetControlFromPosition(1, amazonPosition.Row);
                    filterColumns.PerformLayout();
                    foreach (TableLayoutPanel filterColumn in filterColumns.Controls.OfType<TableLayoutPanel>())
                        filterColumn.PerformLayout();
                    CheckBox[] visibleFilterOptions = Descendants(artworkFilterPanel)
                        .OfType<CheckBox>()
                        .Where(check => check.Name.StartsWith("candidateFilter_", StringComparison.Ordinal))
                        .ToArray();
                    Assert(sourceFilterColumn != null && sourceFilterColumn.ColumnCount == 3
                        && amazonPosition.Column == 0 && amazonCount is Label
                        && sourceFilterColumn.ColumnStyles[0].SizeType == SizeType.Absolute
                        && sourceFilterColumn.ColumnStyles[1].SizeType == SizeType.Absolute
                        && amazonFilter.Text == "Amazon" && !((Label)amazonCount).AutoSize
                        && visibleFilterOptions.All(check => check.Width >= TextRenderer.MeasureText(check.Text, check.Font,
                            new Size(Int32.MaxValue, Int32.MaxValue), TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix).Width + 30),
                        "Artwork Filter names and adjacent counts are not aligned without truncation.");
                    amazonFilter.Checked = false;
                    Assert(candidateCards.Controls.Count == 0 && !upscaleFilter.Enabled && !upscaleFilter.Checked
                        && !rejectedFilter.Enabled && !rejectedFilter.Checked,
                        "Source filtering did not auto-unselect and gray dependent Image Type choices.");
                    Assert(ConfigStore.LoadUi().CandidateExcludedSources.Contains("amazon"),
                        "Artwork Filter source choice was not saved immediately for later albums and runs.");
                    showCandidates.Invoke(form, new object[] { upscaleCandidates });
                    amazonFilter = form.Controls.Find("candidateFilter_source_amazon", true).OfType<CheckBox>().Single();
                    Assert(!amazonFilter.Checked && candidateCards.Controls.Count == 0,
                        "Artwork Filter source exclusion did not survive the next Album candidate set.");
                    amazonFilter.Checked = true;
                    upscaleFilter = form.Controls.Find("candidateFilter_type_Upscalable", true).OfType<CheckBox>().Single();
                    rejectedFilter = form.Controls.Find("candidateFilter_type_Rejected", true).OfType<CheckBox>().Single();
                    Assert(candidateCards.Controls.Count == 2 && upscaleFilter.Enabled && upscaleFilter.Checked
                        && rejectedFilter.Enabled && rejectedFilter.Checked,
                        "Dependent Candidate FILTER choices did not restore automatically with their source.");
                    Assert(!ConfigStore.LoadUi().CandidateExcludedSources.Contains("amazon"),
                        "Restored Artwork Filter source choice remained excluded in saved Interface Settings.");
                    Control upscalableCard = candidateCards.Controls.Cast<Control>().Single(card => (int)card.Tag == 11);
                    ((CheckBox)upscalableCard.Controls["candidateChoice"]).Checked = true;
                    Button upscalePreviewButton = form.Controls.Find("upscalePreviewButton", true).OfType<Button>().Single();
                    showFullPreview = form.Controls.Find("upscaleShowFullButton", true).OfType<Button>().Single();
                    PictureBox projectedPreview = form.Controls.Find("artworkPreviewImage", true).OfType<PictureBox>().Single();
                    Label selectedCandidatePreviewTitle = (Label)typeof(MainForm)
                        .GetField("artworkPreviewTitle", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Assert(upscalePreviewButton.Enabled && selectedCandidatePreviewTitle.Text == "Selected Candidate Artwork"
                        && projectedPreview.Image != null,
                        "Selecting one result did not focus its artwork preview and activate editing.");
                    Assert(showFullPreview.Enabled,
                        "Upscale Show Full did not activate beside Upscale Preview for the selected result.");
                    UiState hoverUi = (UiState)typeof(MainForm)
                        .GetField("uiState", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    hoverUi.HoverEnabled = true;
                    hoverUi.ShowArtwork = true;
                    Dictionary<int, CandidateView> hoverCandidates = (Dictionary<int, CandidateView>)typeof(MainForm)
                        .GetField("candidates", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    MethodInfo showHoverPreview = typeof(MainForm).GetMethod("ShowHoverPreview", BindingFlags.Instance | BindingFlags.NonPublic);
                    advancedGrid = form.Controls.Find("upscaleAdvancedGrid", true).OfType<TableLayoutPanel>().Single();
                    showHoverPreview.Invoke(form, new object[] { hoverCandidates[11] });
                    Image firstHoverImage = projectedPreview.Image;
                    showHoverPreview.Invoke(form, new object[] { hoverCandidates[11] });
                    Assert(Object.ReferenceEquals(firstHoverImage, projectedPreview.Image),
                        "Repeated hover over one result reloaded the same artwork.");
                    Assert(!advancedGrid.IsDisposed,
                        "Result hover disturbed the Artwork Filter workspace.");
                    typeof(MainForm).GetMethod("CloseHoverPreview", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                    hoverUi.HoverEnabled = false;
                    TrackBar brightnessProfile = form.Controls.Find("upscaleProfileSlider_brightness", true).OfType<TrackBar>().Single();
                    brightnessProfile.Value = 3;
                    Assert(ConfigStore.Load().UpscaleBrightnessPercent == 3
                        && form.Controls.Find("upscaleProfileValue_brightness", true).OfType<Label>().Single().Text == "<+3%>",
                        "Artwork Filter advanced profile did not update its value or save to Config v5.");
                    typeof(MainForm).GetMethod("UpscalePreviewClicked", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(form, new object[] { upscalePreviewButton, EventArgs.Empty });
                    Assert(projectedPreview.Image != null && projectedPreview.Image.Width == 1800
                        && projectedPreview.Image.Height == 1800,
                        "Upscale Preview did not render the projected Ideal-size image in memory.");
                    Assert(artworkCaption.Text.Contains("brightness +3%") && artworkCaption.Text.Contains("contrast")
                        && artworkCaption.Text.Contains("preview only"),
                        "Upscale Preview did not disclose the live saved profile correction.");
                    typeof(MainForm).GetMethod("UpscaleShowFullClicked", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(form, new object[] { showFullPreview, EventArgs.Empty });
                    FullSizeArtworkPreviewForm fullPreview = (FullSizeArtworkPreviewForm)typeof(MainForm)
                        .GetField("fullUpscalePreview", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    PictureBox fullPreviewImage = Descendants(fullPreview).OfType<PictureBox>().Single();
                    Assert(fullPreview.Visible && fullPreviewImage.SizeMode == PictureBoxSizeMode.Normal
                        && fullPreviewImage.Image.Width == 1800 && fullPreviewImage.Image.Height == 1800
                        && fullPreviewImage.Size == fullPreviewImage.Image.Size,
                        "Upscale Show Full did not open the upscaled edit at 100% actual pixel size.");
                    fullPreview.Close();
                    Dictionary<string, object> ladderEdit = CandidatePayload(13, "itunes", 3000, 3000, false, "");
                    ladderEdit["source_range_class"] = "ladder";
                    ladderEdit["range_class"] = "ladder";
                    ladderEdit["projected_width"] = 3000;
                    ladderEdit["projected_height"] = 3000;
                    ladderEdit["cache_path"] = upscalePreviewPath;
                    showCandidates.Invoke(form, new object[] { new object[] { ladderEdit } });
                    Control ladderCard = candidateCards.Controls.Cast<Control>().Single(card => (int)card.Tag == 13);
                    ((CheckBox)ladderCard.Controls["candidateChoice"]).Checked = true;
                    upscalePreviewButton = form.Controls.Find("upscalePreviewButton", true).OfType<Button>().Single();
                    showFullPreview = form.Controls.Find("upscaleShowFullButton", true).OfType<Button>().Single();
                    Assert(upscalePreviewButton.Enabled && showFullPreview.Enabled,
                        "A Ladder candidate was incorrectly blocked from manual editing and full-size preview.");
                    typeof(MainForm).GetMethod("UpscalePreviewClicked", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(form, new object[] { upscalePreviewButton, EventArgs.Empty });
                    HashSet<int> editedCandidates = (HashSet<int>)typeof(MainForm)
                        .GetField("editedCandidateIndexes", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Assert(editedCandidates.Contains(13) && projectedPreview.Image.Width == 3000
                        && projectedPreview.Image.Height == 3000,
                        "Manual editing did not retain a non-upscaled Ladder result at native resolution.");
                    FieldInfo candidateProcessField = typeof(MainForm).GetField("currentProcess", BindingFlags.Instance | BindingFlags.NonPublic);
                    FieldInfo candidateAwaitingField = typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic);
                    Button candidateUseSelected = (Button)typeof(MainForm)
                        .GetField("useSelected", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    using (Process liveProcess = Process.GetCurrentProcess())
                    {
                        candidateProcessField.SetValue(form, liveProcess);
                        candidateAwaitingField.SetValue(form, false);
                        typeof(MainForm).GetMethod("UpdateCandidateActions", BindingFlags.Instance | BindingFlags.NonPublic)
                            .Invoke(form, null);
                        Assert(candidateUseSelected.Enabled,
                            "Use Selected remained disabled while alternate MusicBrainz candidates were live before decision_required arrived.");
                        candidateProcessField.SetValue(form, null);
                    }
                    candidateAwaitingField.SetValue(form, true);
                    Dictionary<string, object> existingIdeal = CandidatePayload(12, "local", 1800, 1800, false, "cover-file");
                    existingIdeal["cache_path"] = upscalePreviewPath;
                    existingIdeal["local_reference"] = "cover.jpg";
                    showCandidates.Invoke(form, new object[] { new object[] { existingIdeal } });
                    upscalePreviewButton = form.Controls.Find("upscalePreviewButton", true).OfType<Button>().Single();
                    Control existingIdealCard = candidateCards.Controls.Cast<Control>().Single(card => (int)card.Tag == 12);
                    Assert(((CheckBox)existingIdealCard.Controls["candidateChoice"]).Checked,
                        "An existing local cover was not preselected for immediate preview and editing.");
                    Assert(upscalePreviewButton.Enabled,
                        "An existing ideal cover did not keep its direct editing action enabled.");
                    Assert(selectedCandidatePreviewTitle.Text == "Existing Cover Editing",
                        "Selecting an existing local cover did not focus it in Selected Album Artwork; title was '"
                        + selectedCandidatePreviewTitle.Text + "'.");
                    TrackBar pictureProfile = form.Controls.Find("upscaleProfileSlider_picture", true).OfType<TrackBar>().Single();
                    pictureProfile.Value = 2;
                    typeof(MainForm).GetMethod("UpscalePreviewClicked", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(form, new object[] { upscalePreviewButton, EventArgs.Empty });
                    Label artworkPreviewTitle = (Label)typeof(MainForm)
                        .GetField("artworkPreviewTitle", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Assert(artworkPreviewTitle.Text == "Existing Cover Edit Preview"
                        && artworkCaption.Text.Contains("picture +2%"),
                        "Existing cover edits were not rendered through the live preview path.");
                    invokeArtworkFilter.Invoke(artworkFilter, new object[] { EventArgs.Empty });
                    Assert(!artworkFilterPanel.Visible && filterSplit.SplitterDistance == filterClosedDistance
                        && filterSplit.Panel2MinSize == filterClosedPanel2Minimum,
                        "Closing Artwork Filter did not restore the previous Activity/Candidate divider.");
                    FieldInfo selectionField = typeof(MainForm).GetField("selectionMode", BindingFlags.Instance | BindingFlags.NonPublic);
                    Assert((SelectionMode)selectionField.GetValue(form) == SelectionMode.Select, "Manual Select must be the default selection mode.");
                    VerifyMediaFilter(form);
                    FieldInfo formUiField = typeof(MainForm).GetField("uiState", BindingFlags.Instance | BindingFlags.NonPublic);
                    UiState formUi = (UiState)formUiField.GetValue(form);
                    MethodInfo applyTheme = typeof(MainForm).GetMethod("ApplyTheme", BindingFlags.Instance | BindingFlags.NonPublic);
                    List<AlbumInfo> themeAlbums = (List<AlbumInfo>)typeof(MainForm).GetField("albums", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Dictionary<string, bool> selectedBeforeTheme = themeAlbums.ToDictionary(album => album.Path, album => album.Selected);
                    foreach (string theme in new[] { "Dark", "Light", "Dark", "System", "Light", "System", "Dark" })
                    {
                        formUi.Theme = theme;
                        applyTheme.Invoke(form, null);
                        Assert(themeAlbums.All(album => album.Selected == selectedBeforeTheme[album.Path]),
                            "Theme switching changed library selection state in " + theme + " mode.");
                        Assert(form.Icon != null && candidateCards is WatermarkFlowLayoutPanel,
                            "Theme switching lost the application icon or candidate watermark surface.");
                    }
                    CandidateView embeddedView = new CandidateView { Source = "embedded", LocalOrigin = "embedded-track", LocalReference = "track.mp3" };
                    CandidateView fileView = new CandidateView { Source = "local", LocalOrigin = "cover-file", LocalReference = "cover-(2).png" };
                    Assert(embeddedView.DisplaySource == "Embedded from track · track.mp3"
                        && fileView.DisplaySource == "Cover file · cover-(2).png",
                        "Local candidates do not identify embedded-track versus cover-file origin.");
                    MethodInfo applyEvent = typeof(MainForm).GetMethod("ApplyCoreEvent", BindingFlags.Instance | BindingFlags.NonPublic);
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "release_resolved" }, { "artist", "Edited Artist" }, { "release", "Edited Album" },
                        { "fallback", true }, { "reason", "NO MBID FOUND" }
                    } });
                    FieldInfo contextField = typeof(MainForm).GetField("candidateContext", BindingFlags.Instance | BindingFlags.NonPublic);
                    Assert(((Label)contextField.GetValue(form)).Text.Contains("FALLBACK MODE") && ((Label)contextField.GetValue(form)).Text.Contains("NO MBID FOUND"),
                        "Windows fallback context was not displayed.");
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "decision_required" }, { "reason", "fallback" }
                    } });
                    FieldInfo refineField = typeof(MainForm).GetField("refineFallback", BindingFlags.Instance | BindingFlags.NonPublic);
                    Assert(((Button)refineField.GetValue(form)).Enabled, "Fallback Artist/Album retry was not enabled for review.");
                    formUi.ShowArtwork = false;
                    typeof(MainForm).GetMethod("ApplyArtworkPanelVisibility", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "musicbrainz_matches" }, { "artist", "Fixture Artist" }, { "title", "Fixture Track" },
                        { "compilation_track", false }, { "items", matchFixture }
                    } });
                    Control embeddedMatches = form.Controls.Find("musicBrainzMatchesPanel", true).Single();
                    Assert(activityHost.Controls.Contains(embeddedMatches) && activityTitle.Text == "MusicBrainz Matches"
                        && artworkWorkspace.ColumnStyles[1].Width > 0,
                        "MusicBrainz Matches did not replace Scan Activity while forcing the shared Artwork panel visible.");
                    typeof(MainForm).GetMethod("CloseMusicBrainzMatchesWorkspace", BindingFlags.Instance | BindingFlags.NonPublic)
                        .Invoke(form, new object[] { true });
                    Assert(activityTitle.Text == "Scan Activity and Decisions"
                        && activityHost.Controls.Find("musicBrainzMatchesPanel", true).Length == 0,
                        "Returning from MusicBrainz Matches did not restore Scan Activity when no candidate results remained.");
                    formUi.ShowArtwork = true;
                    typeof(MainForm).GetMethod("ApplyArtworkPanelVisibility", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                    FieldInfo runningField = typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic);
                    FieldInfo awaitingField = typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic);
                    MethodInfo updateSelection = typeof(MainForm).GetMethod("UpdateSelectionControls", BindingFlags.Instance | BindingFlags.NonPublic);
                    runningField.SetValue(form, true);
                    updateSelection.Invoke(form, null);
                    Assert(launch.Enabled && launch.Text == "WAITING" && Convert.ToString(launch.Tag) == "waiting",
                        "LAUNCH must become the orange WAITING state while an artwork decision is pending.");
                    Assert(launch.FlatAppearance.BorderSize == 0 && launch.Region == null,
                        "LAUNCH / WAITING still clips antialiasing through a rounded HWND region.");
                    Assert(form.Controls.Find("selectAndLaunch", true).Length == 0,
                        "Select Media still contains the duplicate mini-LAUNCH control.");
                    FieldInfo settingsField = typeof(MainForm).GetField("settingsMenuItem", BindingFlags.Instance | BindingFlags.NonPublic);
                    Assert(!((ToolStripMenuItem)settingsField.GetValue(form)).Enabled,
                        "Settings must be unavailable while an album decision is active.");
                    FieldInfo candidatesField = typeof(MainForm).GetField("candidates", BindingFlags.Instance | BindingFlags.NonPublic);
                    Dictionary<int, CandidateView> candidateMap = (Dictionary<int, CandidateView>)candidatesField.GetValue(form);
                    Button hoverButton = form.Controls.Find("enableHoverButton", true).OfType<Button>().Single();
                    candidateMap[99] = new CandidateView { Index = 99 };
                    typeof(MainForm).GetMethod("UpdateHoverButton", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                    Assert(hoverButton.Enabled && hoverButton.Text == "Enable Hover", "Enable Hover was not converted to a candidate-lifecycle action button.");
                    Color neutralHoverColor = hoverButton.ForeColor;
                    MethodInfo buttonClick = typeof(Button).GetMethod("OnClick", BindingFlags.Instance | BindingFlags.NonPublic);
                    buttonClick.Invoke(hoverButton, new object[] { EventArgs.Empty });
                    Assert(ConfigStore.LoadUi().HoverEnabled && hoverButton.ForeColor != neutralHoverColor,
                        "Enable Hover did not toggle on, persist, and display the green selected style.");
                    buttonClick.Invoke(hoverButton, new object[] { EventArgs.Empty });
                    Assert(!ConfigStore.LoadUi().HoverEnabled,
                        "Enable Hover did not toggle off and persist through internal Windows interface settings.");
                    candidateMap.Clear();
                    candidateMap[1] = new CandidateView { Index = 1 };
                    candidateCards.Controls.Add(new Panel());
                    RichTextBox activity = (RichTextBox)typeof(MainForm).GetField("activity", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "album_completed" }, { "album_path", "fixture" },
                        { "action", "Installed" }, { "destination", "cover.jpg" }
                    } });
                    Assert(candidateMap.Count == 0 && candidateCards.Controls.Count == 0 && !(bool)awaitingField.GetValue(form),
                        "Completed artwork decisions must clear the visible candidate search.");
                    TableLayoutPanel reportColumn = (TableLayoutPanel)typeof(MainForm)
                        .GetField("activityColumn", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    bool filterWorkspaceActive = (bool)typeof(MainForm)
                        .GetField("candidateFilterWorkspaceActive", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    Assert(activityTitle.Text == "Scan Activity and Decisions" && activityHost.Controls.Contains(reportColumn)
                        && Descendants(reportColumn).Contains(activity) && !filterWorkspaceActive
                        && activityHost.Controls.Find("musicBrainzMatchesPanel", true).Length == 0,
                        "Completed artwork decisions did not close review workspaces and restore the final Album report.");
                    Assert(launch.Enabled && launch.Text == "STOP", "STOP must remain enabled while processing.");
                    MethodInfo appendColored = typeof(MainForm).GetMethod("AppendActivity", BindingFlags.Instance | BindingFlags.NonPublic, null,
                        new[] { typeof(string), typeof(ActivityTone) }, null);
                    int colorStart = activity.TextLength;
                    appendColored.Invoke(form, new object[] { "palette-check", ActivityTone.Error });
                    activity.Select(colorStart, "palette-check".Length);
                    Assert(activity.SelectionColor != activity.ForeColor && activity.SelectionColor != Color.Empty,
                        "Scan Activity did not preserve semantic text colors.");

                    ThemePalette activityPalette = ThemeManager.PaletteFor(formUi.Theme);
                    typeof(MainForm).GetMethod("ApplyModeActivityAppearance", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                    Assert(activity.BackColor.ToArgb() == activityPalette.PanelSurface.ToArgb()
                        && activityPalette.LogWriteBackground.ToArgb() == activityPalette.PanelSurface.ToArgb(),
                        "Scan Activity still uses the old red-brown WRITE surface instead of the Artwork Candidates panel blue.");

                    activity.Clear();
                    MethodInfo appendAlbumHeader = typeof(MainForm).GetMethod("AppendAlbumActivityHeader", BindingFlags.Instance | BindingFlags.NonPublic);
                    appendAlbumHeader.Invoke(form, new object[] { 7, 9, "Report Artist", "Report Album" });
                    activity.Select(0, "[7/9]".Length);
                    Color headerIndexColor = activity.SelectionColor;
                    activity.Select(activity.Text.IndexOf("Report Artist", StringComparison.Ordinal), "Report Artist".Length);
                    Color headerArtistColor = activity.SelectionColor;
                    activity.Select(activity.Text.IndexOf("Report Album", StringComparison.Ordinal), "Report Album".Length);
                    Color headerAlbumColor = activity.SelectionColor;
                    Assert(headerIndexColor.ToArgb() == activityPalette.StatusOrange.ToArgb()
                        && headerArtistColor.ToArgb() == activityPalette.StatusPurple.ToArgb()
                        && headerAlbumColor.ToArgb() == activityPalette.LogWarning.ToArgb(),
                        "Album activity headings do not use orange [position], purple Artist, and yellow Album segments.");

                    MethodInfo beginRunStats = typeof(MainForm).GetMethod("BeginAlbumRunStatistics", BindingFlags.Instance | BindingFlags.NonPublic);
                    MethodInfo finishRunStats = typeof(MainForm).GetMethod("FinishActiveAlbumStatistics", BindingFlags.Instance | BindingFlags.NonPublic);
                    MethodInfo showRunReport = typeof(MainForm).GetMethod("ShowAlbumRunReport", BindingFlags.Instance | BindingFlags.NonPublic);
                    beginRunStats.Invoke(form, new object[] { Album("Report Artist", "Report Album", AlbumState.New) });
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "release_resolved" }, { "artist", "Report Artist" }, { "release", "Report Album" },
                        { "fallback", false }, { "reason", "" }
                    } });
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "candidates" }, { "items", new object[0] }, { "hidden_by_source_policy", 2 }, { "fallback", false }
                    } });
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "decision_required" }, { "reason", "review" }
                    } });
                    // Reproduce Windows raising process exit before the final
                    // redirected album_completed line is dispatched.
                    finishRunStats.Invoke(form, new object[] { "Incomplete" });
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "album_completed" }, { "album_path", "Report Artist\\Report Album" },
                        { "action", "ReadOnly" }, { "destination", "cover.jpg" },
                        { "source", "amazon" }, { "source_width", 1200 }, { "source_height", 1200 },
                        { "final_width", 1800 }, { "final_height", 1800 }, { "resized", true },
                        { "converted", false }, { "upscale_backend", "gpu-lanczos3" }
                    } });
                    showRunReport.Invoke(form, new object[] { "read", 1 });
                    Assert(activity.Text.StartsWith("S:P:L:I:N:E:D ALBUM RUN REPORT", StringComparison.Ordinal)
                        && activity.Text.Contains("Albums reviewed: 1/1")
                        && activity.Text.Contains("[1/1] Report Artist - Report Album")
                        && activity.Text.Contains("2 evaluated") && activity.Text.Contains("2 policy-hidden")
                        && activity.Text.Contains("Outcome: ReadOnly") && activity.Text.Contains("Result: cover.jpg")
                        && activity.Text.Contains("Selected: Amazon") && activity.Text.Contains("Source 1200 x 1200")
                        && activity.Text.Contains("Final 1800 x 1800") && activity.Text.Contains("Upscale: gpu-lanczos3")
                        && !activity.Text.Contains("SPLINED LIVE PROCESSING"),
                        "A late album_completed event did not replace Incomplete with the authoritative per-album result.");

                    FlowLayoutPanel candidateActions = form.Controls.Find("candidateActionRow", true).OfType<FlowLayoutPanel>().Single();
                    candidateActions.PerformLayout();
                    Assert(candidateActions.Parent is TableLayoutPanel
                        && ((TableLayoutPanel)candidateActions.Parent).GetPositionFromControl(candidateActions).Row == 0
                        && candidateActions.Controls.OfType<Button>().Where(button => button.Visible)
                            .All(button => button.Bottom + button.Margin.Bottom <= candidateActions.ClientSize.Height),
                        "Candidate actions were not moved into the Artwork Filter header or a header button is clipped.");

                    SplitContainer mainPanels = (SplitContainer)typeof(MainForm).GetField("mainSplit", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    SplitContainer rightPanels = (SplitContainer)typeof(MainForm).GetField("rightSplit", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    int expectedMainDistance = Math.Min(mainPanels.Width - mainPanels.Panel2MinSize - mainPanels.SplitterWidth, mainPanels.Panel1MinSize + 37);
                    int expectedRightDistance = Math.Min(rightPanels.Height - rightPanels.Panel2MinSize - rightPanels.SplitterWidth, rightPanels.Panel1MinSize + 41);
                    mainPanels.SplitterDistance = expectedMainDistance;
                    rightPanels.SplitterDistance = expectedRightDistance;
                    typeof(MainForm).GetMethod("SaveUiState", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                    UiState savedPanelState = ConfigStore.LoadUi();
                    Assert(savedPanelState.MainSplitterDistance == expectedMainDistance
                        && savedPanelState.RightSplitterDistance == expectedRightDistance,
                        "Main library and Activity/Candidate panel sizes were not retained on exit/save.");

                    MethodInfo applyLayoutPreset = typeof(MainForm).GetMethod("ApplyLayoutPreset", BindingFlags.Instance | BindingFlags.NonPublic);
                    applyLayoutPreset.Invoke(form, new object[] { "Stacked" });
                    Assert(mainPanels.Orientation == Orientation.Horizontal
                        && libraryScrollCanvas.AutoScroll && activityPanel.AutoScroll && candidatesPanel.AutoScroll,
                        "Stacked layout did not preserve independent scrolling for all three work areas.");
                }

                Console.WriteLine("PASS: Stable v3 identity and About surface, Fluent Compact buttons/dropdowns/spinners/checkboxes/tabs, persisted panel sizes and independently scrollable work areas, Artwork Filter header actions, Select/Scan Mode controls, unclipped Select and candidate action rows, clean title tooltips and tree-state images, semantic Folder status legend, blue Activity surface, segmented album headings, per-album completion statistics, consumed launch selections across completion/STOP and normalized UNC paths, nested View/Appearance menu, pre-display theme initialization, centralized Dark/Light/System theme transitions, DPI-aware rounded action/focus geometry, 100/125/150% responsive layout paths, Config v5 and credential isolation, reorganized Settings, equal retention panels, source range preview, authoritative cover/history reconciliation, artist aggregate/selection rules, live in-memory filtering, persisted hover action, theme-stable watermark, multicolor icon resources, source policy, fallback, UNC handling, and LAUNCH/WAITING/STOP lifecycle.");
                return 0;
            }
            catch (Exception error)
            {
                Console.Error.WriteLine("FAIL: " + error);
                return 1;
            }
        }

        private static void VerifyArtistAggregateAndSelectionRules()
        {
            AlbumInfo whiteOne = Album("Artist", "White One", AlbumState.New);
            AlbumInfo whiteTwo = Album("Artist", "White Two", AlbumState.New);
            AlbumInfo orange = Album("Artist", "Orange", AlbumState.Processed);
            AlbumInfo red = Album("Artist", "Red", AlbumState.Bypassed);
            AlbumInfo purple = Album("Artist", "Purple", AlbumState.TimeoutActive);
            AlbumInfo pendingCompilation = Album("Artist", "Pending Compilation", AlbumState.Processed);
            pendingCompilation.Compilation = true;
            pendingCompilation.CompilationTrackArtworkEligible = true;
            pendingCompilation.Outcome = "local";
            AlbumInfo bypassedCompilation = Album("Artist", "Bypassed Compilation", AlbumState.Bypassed);
            bypassedCompilation.Compilation = true;
            bypassedCompilation.CompilationTrackArtworkEligible = true;
            bypassedCompilation.Outcome = "bypass";
            AlbumInfo completedCompilation = Album("Artist", "Completed Compilation", AlbumState.Processed);
            completedCompilation.Compilation = true;
            completedCompilation.CompilationTrackArtworkEligible = true;
            completedCompilation.Outcome = "embedded-compilation";

            Assert(ArtistStateResolver.Aggregate(new[] { whiteOne, whiteTwo }) == ArtistAggregateState.Unprocessed,
                "An all-white Artist did not aggregate to WHITE.");
            Assert(ArtistStateResolver.Aggregate(new[] { whiteOne, red }) == ArtistAggregateState.ContainsBypass,
                "An Artist with one RED Album did not aggregate to BLUE.");
            Assert(ArtistStateResolver.Aggregate(new[] { whiteOne, orange }) == ArtistAggregateState.Partial,
                "A partial Artist without bypass did not aggregate to PURPLE.");
            Assert(ArtistStateResolver.Aggregate(new[] { whiteOne, orange, red }) == ArtistAggregateState.ContainsBypass,
                "A partial Artist with bypass did not aggregate to BLUE.");
            Assert(ArtistStateResolver.Aggregate(new[] { orange, Album("Artist", "Orange Two", AlbumState.Processed) }) == ArtistAggregateState.Complete,
                "A fully processed Artist without bypass did not aggregate to GREEN.");
            Assert(ArtistStateResolver.Aggregate(new[] { orange, red }) == ArtistAggregateState.ContainsBypass,
                "A fully processed Artist with bypass did not aggregate to BLUE.");

            List<AlbumInfo> selection = new List<AlbumInfo> { whiteOne, orange, red, purple,
                pendingCompilation, bypassedCompilation, completedCompilation };
            ArtistSelectionRules.Apply(selection, true, false);
            Assert(whiteOne.Selected && pendingCompilation.Selected && bypassedCompilation.Selected
                && !completedCompilation.Selected && !orange.Selected && !red.Selected && !purple.Selected,
                "Artist selection did not include pending compilation track-art work or selected completed/ordinary protected Albums.");
            Assert(bypassedCompilation.BypassOverride,
                "Pending compilation track-art work did not receive its temporary runtime history override.");
            Assert(!red.BypassOverride && red.State == AlbumState.Bypassed,
                "A RED Album was selected or its saved bypass state changed without confirmation.");
            Assert(purple.State == AlbumState.TimeoutActive,
                "Artist selection changed timeout-active Album authority.");

            ArtistSelectionRules.Apply(selection, false, false);
            ArtistSelectionRules.Apply(selection, true, true);
            Assert(whiteOne.Selected && pendingCompilation.Selected && bypassedCompilation.Selected
                && red.Selected && red.BypassOverride && red.State == AlbumState.Bypassed,
                "A confirmed temporary RED override did not select the Album while retaining bypass authority.");
            Assert(!orange.Selected && !purple.Selected && !completedCompilation.Selected,
                "An ordinary processed, completed compilation, or timeout-active Album was auto-selected by Artist selection.");
        }

        private static AlbumInfo Album(string artist, string title, AlbumState state)
        {
            return new AlbumInfo { Artist = artist, Title = title, Path = artist + "\\" + title, State = state };
        }

        private static Dictionary<string, object> CandidatePayload(int index, string source, int width, int height, bool recommended, string localOrigin)
        {
            return new Dictionary<string, object>
            {
                { "index", index }, { "source", source }, { "width", width }, { "height", height },
                { "recommended", recommended }, { "acceptable", true }, { "range_class", "Ideal" },
                { "policy_status", "accept" }, { "cache_path", "" }, { "url", "" },
                { "local_origin", localOrigin }, { "local_reference", "" }
            };
        }

        private static void VerifyRetentionLayout(SetupForm setup, TabControl advanced, TabPage pathsTab)
        {
            TabControl primary = (TabControl)typeof(SetupForm).GetField("primaryTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(setup);
            primary.SelectedIndex = 1;
            advanced.SelectedTab = pathsTab;
            foreach (Size size in new[] { new Size(980, 790), new Size(1225, 988), new Size(1470, 1185) })
            {
                setup.ClientSize = size;
                setup.PerformLayout();
                Application.DoEvents();
                AssertSettingsLayout(setup);
            }
            // Windows Forms applies the same proportional Control.Scale path
            // when moving to 125% and 150% DPI. Apply 1.25, then another 1.20
            // (1.50 total) to verify both common scaling boundaries.
            foreach (float factor in new[] { 1.25f, 1.20f })
            {
                setup.Scale(new SizeF(factor, factor));
                setup.PerformLayout();
                Application.DoEvents();
                AssertSettingsLayout(setup);
            }
        }

        private static void AssertSettingsLayout(SetupForm setup)
        {
            Control retention = setup.Controls.Find("retentionGroup", true).Single();
            Control history = setup.Controls.Find("retentionSettingsGroup", true).Single();
            Assert(Math.Abs(retention.Width - history.Width) <= 12 && Math.Abs(retention.Top - history.Top) <= 2,
                "Retention sibling panels did not remain equal-width and vertically aligned on resize/DPI scaling.");
            Control actions = setup.Controls.Find("pathsActionRow", true).Single();
            foreach (Control button in actions.Controls)
                Assert(button.Right <= actions.ClientSize.Width + 2,
                    "A Paths action control clipped at a tested Windows resize/DPI layout.");
            Control runtimeLeft = setup.Controls.Find("runtimeLeftColumn", true).Single();
            Control runtimeRight = setup.Controls.Find("runtimeRightColumn", true).Single();
            Assert(Math.Abs(runtimeLeft.Width - runtimeRight.Width) <= 12 && Math.Abs(runtimeLeft.Top - runtimeRight.Top) <= 2,
                "Runtime controls are not aligned in balanced left/right columns.");
        }

        private static void VerifyMediaFilter(MainForm form)
        {
            List<AlbumInfo> filterAlbums = new List<AlbumInfo>
            {
                Album("Alpha Artist", "Fresh Album", AlbumState.New),
                Album("Alpha Artist", "Processed Album", AlbumState.Processed),
                Album("Bypass Artist", "Red Album", AlbumState.Bypassed),
                Album("Timeout Artist", "Waiting Album", AlbumState.TimeoutActive),
                Album("Incomplete Artist", "Resume Album", AlbumState.Incomplete),
                Album("Complete Artist", "Done One", AlbumState.Processed),
                Album("Complete Artist", "Done Two", AlbumState.Processed)
            };
            filterAlbums[0].Selected = true;
            FieldInfo albumsField = typeof(MainForm).GetField("albums", BindingFlags.Instance | BindingFlags.NonPublic);
            albumsField.SetValue(form, filterAlbums);
            MethodInfo buildTree = typeof(MainForm).GetMethod("BuildTree", BindingFlags.Instance | BindingFlags.NonPublic);
            TreeView tree = (TreeView)typeof(MainForm).GetField("tree", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            TextBox artist = form.Controls.Find("mediaArtistFilter", true).OfType<TextBox>().Single();
            TextBox album = form.Controls.Find("mediaAlbumFilter", true).OfType<TextBox>().Single();
            artist.Text = "";
            album.Text = "";
            foreach (string filterName in new[] { "mediaFilterWhite", "mediaFilterOrange", "mediaFilterRed", "mediaFilterPurple", "mediaFilterGreen", "mediaFilterBlue", "mediaFilterIncomplete" })
                form.Controls.Find(filterName, true).OfType<CheckBox>().Single().Checked = true;
            Dictionary<string, AlbumState> originalStates = filterAlbums.ToDictionary(item => item.Path, item => item.State);
            Dictionary<string, bool> originalSelections = filterAlbums.ToDictionary(item => item.Path, item => item.Selected);

            MethodInfo setExpanded = typeof(MainForm).GetMethod("SetMediaFilterExpanded", BindingFlags.Instance | BindingFlags.NonPublic);
            TableLayoutPanel libraryLayout = (TableLayoutPanel)typeof(MainForm).GetField("libraryLayout", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            Button collapse = form.Controls.Find("mediaFilterToggle", true).OfType<Button>().Single();
            setExpanded.Invoke(form, new object[] { false });
            Assert(Math.Abs(libraryLayout.RowStyles[1].Height - 38) < 0.1 && collapse.Text.Contains("▸"),
                "Select did not collapse and return its space to the folder tree.");
            Control filterContainer = form.Controls.Find("mediaFilterContainer", true).Single();
            filterContainer.PerformLayout();
            Assert(collapse.Bottom <= filterContainer.ClientSize.Height - 1,
                "The collapsed Select button is still clipped along its bottom edge.");
            setExpanded.Invoke(form, new object[] { true });
            Assert(libraryLayout.RowStyles[1].Height > 250 && collapse.Text.Contains("▾"),
                "Select did not expand after being collapsed.");
            Assert(form.Controls.Find("mediaFilterButton", true).Length == 0
                && collapse.Text.StartsWith("Select Media", StringComparison.Ordinal)
                && !Descendants(form).Any(control => String.Equals(control.Text, "Media Filter", StringComparison.Ordinal)),
                "A redundant Media Filter button/title remains instead of the single collapsible Select Media control.");
            Assert(new[] { "White", "Orange", "Red", "Purple", "Green", "Blue" }
                    .All(colorName => !Descendants(form.Controls.Find("mediaStatusFilters", true).Single()).OfType<Label>()
                        .Any(label => label.Text.IndexOf(colorName, StringComparison.OrdinalIgnoreCase) >= 0)),
                "Media Filter still spells out color names instead of using color bullets.");
            IDictionary statusBullets = (IDictionary)typeof(MainForm).GetField("mediaStatusBullets", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            IDictionary statusDescriptions = (IDictionary)typeof(MainForm).GetField("mediaStatusDescriptions", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            Assert(statusBullets.Count == 7 && statusDescriptions.Count == 7,
                "Folder-status bullets or matching semantic labels are missing.");
            foreach (DictionaryEntry entry in statusBullets)
            {
                Label bullet = (Label)entry.Value;
                Label description = (Label)statusDescriptions[entry.Key];
                Assert(bullet.Width >= 23 && bullet.Height >= 23 && bullet.ForeColor.ToArgb() == description.ForeColor.ToArgb(),
                    "A Folder status bullet is too small or its label does not match the semantic status color.");
            }
            CheckBox scanRead = form.Controls.Find("filteredScanRead", true).OfType<CheckBox>().Single();
            CheckBox scanWrite = form.Controls.Find("filteredScanWrite", true).OfType<CheckBox>().Single();
            Button primaryLaunch = (Button)typeof(MainForm).GetField("launch", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            CheckBox selectAll = form.Controls.Find("selectModeAll", true).OfType<CheckBox>().Single();
            CheckBox selectNone = form.Controls.Find("selectModeNone", true).OfType<CheckBox>().Single();
            CheckBox selectFiltered = form.Controls.Find("selectModeFiltered", true).OfType<CheckBox>().Single();
            CheckBox autoAll = form.Controls.Find("autoScanAll", true).OfType<CheckBox>().Single();
            CheckBox autoSelected = form.Controls.Find("autoScanSelected", true).OfType<CheckBox>().Single();
            ConfigState formState = (ConfigState)typeof(MainForm).GetField("state", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            Assert(scanRead.Text == "Launch [READ] Source Results" && scanWrite.Text == "Launch [LIVE WRITE] Choice Results"
                && (formState.Mode.Equals("write", StringComparison.OrdinalIgnoreCase) ? scanWrite.Checked : scanRead.Checked)
                && ((GroupBox)scanRead.Parent.Parent).Text == "Launch Mode"
                && ((GroupBox)selectFiltered.Parent.Parent).Text.StartsWith("Select Mode [", StringComparison.Ordinal)
                && ((GroupBox)autoAll.Parent.Parent).Text == "Album Scanning"
                && autoAll.Text == "Auto Scan [All]" && autoSelected.Text == "Auto Scan [Selected]"
                && selectAll.Text == "Select [ALL]" && selectNone.Text == "Select [NONE]" && selectFiltered.Text == "Select [FILTERED]",
                "Select/Launch labels are incorrect or Config v5 did not initialize the active launch mode.");
            foreach (GroupBox modeGroup in new[] { (GroupBox)selectFiltered.Parent.Parent, (GroupBox)autoAll.Parent.Parent, (GroupBox)scanRead.Parent.Parent })
            {
                modeGroup.PerformLayout();
                Control clipped = modeGroup.Controls.Cast<Control>().SelectMany(control => control.Controls.Cast<Control>())
                    .FirstOrDefault(control => control.Right > control.Parent.ClientSize.Width || control.Bottom > control.Parent.ClientSize.Height);
                Assert(clipped == null,
                    modeGroup.Text + " contains clipped option text or checkbox rows (group " + modeGroup.ClientSize.Width + "x" + modeGroup.ClientSize.Height
                    + (clipped == null ? "" : ", control " + clipped.Text + " at " + clipped.Bounds + " in " + clipped.Parent.ClientSize) + ").");
            }
            foreach (CheckBox option in new[] { selectAll, selectNone, selectFiltered, autoAll, autoSelected, scanRead, scanWrite })
                Assert(option.Width >= TextRenderer.MeasureText(option.Text, option.Font).Width + 28,
                    option.Text + " does not have enough themed checkbox width to render without ellipsis.");
            Assert((formState.Mode.Equals("write", StringComparison.OrdinalIgnoreCase) ? scanWrite.Checked && !scanRead.Checked : scanRead.Checked && !scanWrite.Checked)
                && !selectFiltered.Checked,
                "Config v5 launch mode was not restored without selecting Auto Mode.");
            MethodInfo getLaunchAlbums = typeof(MainForm).GetMethod("GetLaunchAlbums", BindingFlags.Instance | BindingFlags.NonPublic);
            Assert(!autoAll.Checked && !autoSelected.Checked,
                "Auto Scan must remain opt-in so ordinary Select modes always use operator review.");
            autoSelected.Checked = true;
            Assert(((List<AlbumInfo>)getLaunchAlbums.Invoke(form, null)).Count == filterAlbums.Count(item => item.Selected),
                "Auto Scan [Selected] does not use only explicit Album selections.");
            autoAll.Checked = true;
            List<AlbumInfo> allQueue = (List<AlbumInfo>)getLaunchAlbums.Invoke(form, null);
            Assert(allQueue.All(item => item.Selected || item.State == AlbumState.New)
                && allQueue.Any(item => item.State == AlbumState.New),
                "Auto Scan [All] does not use every Unprocessed Album plus explicit selections.");
            autoAll.Checked = false;
            Assert(((List<AlbumInfo>)getLaunchAlbums.Invoke(form, null)).Count == filterAlbums.Count(item => item.Selected),
                "Clearing Auto Scan did not restore the ordinary reviewed selection queue.");
            autoSelected.Checked = true;
            scanRead.Checked = false;
            scanWrite.Checked = false;
            Assert(!primaryLaunch.Enabled, "The single primary LAUNCH must be disabled until a Launch Mode is chosen.");
            scanRead.Checked = true;
            Assert(scanRead.Checked && !scanWrite.Checked && !scanWrite.Enabled,
                "Filtered Scan [READ] did not gray the mutually exclusive Write choice.");
            Assert(primaryLaunch.Enabled && primaryLaunch.Text.StartsWith("LAUNCH", StringComparison.Ordinal),
                "The single primary LAUNCH did not become active after selecting a Launch Mode.");
            Assert(scanRead.ForeColor != scanWrite.ForeColor,
                "Filtered Scan [READ] and [WRITE] do not have distinct green/red label colors.");
            scanRead.Checked = false;
            Assert(scanWrite.Enabled, "Clearing Filtered Scan [READ] did not restore the Write choice.");
            scanWrite.Checked = true;
            Assert(scanWrite.Checked && !scanRead.Checked && !scanRead.Enabled,
                "Filtered Scan [WRITE] did not gray the mutually exclusive Read choice.");
            scanWrite.Checked = false;

            buildTree.Invoke(form, null);
            Assert(tree.StateImageList != null && tree.StateImageList.Images.Count == 2,
                "The media tree did not receive the shared red/green radio-state images.");
            Assert(!tree.ShowNodeToolTips,
                "The folder tree still uses native tooltip rendering instead of the clean owner-drawn tooltip surface.");
            using (Bitmap uncheckedState = new Bitmap(tree.StateImageList.Images[0]))
            {
                Color stateCorner = uncheckedState.GetPixel(0, 0);
                Assert(IsCloserTo(stateCorner, ThemeManager.CurrentPalette.TreeSurface, Color.White),
                    "The folder-list state image still contains a white native/transparent outline artifact.");
            }
            Assert(tree.Nodes.Count == 5, "Media Filter did not initially show the in-memory Artist model.");
            artist.Text = "alpha";
            Assert(tree.Nodes.Count == 1 && tree.Nodes[0].Text == "Alpha Artist",
                "Artist live text filtering is not immediate or case-insensitive.");
            album.Text = "fresh";
            Assert(tree.Nodes.Count == 1 && tree.Nodes[0].Nodes.Count == 1 && tree.Nodes[0].Nodes[0].Text == "Fresh Album",
                "Combined Artist + Album live filtering did not narrow the existing tree model.");
            artist.Text = "";
            album.Text = "waiting";
            Assert(tree.Nodes.Count == 1 && tree.Nodes[0].Text == "Timeout Artist",
                "Album live text filtering did not match the Album folder name.");
            album.Text = "Unprocessed";
            Assert(tree.Nodes.Count == 0,
                "Folder-status check marks or labels leaked into Artist/Album text search results.");

            album.Text = "";
            CheckBox[] filters = form.Controls.Find("mediaFilterWhite", true).OfType<CheckBox>()
                .Concat(form.Controls.Find("mediaFilterOrange", true).OfType<CheckBox>())
                .Concat(form.Controls.Find("mediaFilterRed", true).OfType<CheckBox>())
                .Concat(form.Controls.Find("mediaFilterPurple", true).OfType<CheckBox>())
                .Concat(form.Controls.Find("mediaFilterGreen", true).OfType<CheckBox>())
                .Concat(form.Controls.Find("mediaFilterBlue", true).OfType<CheckBox>())
                .Concat(form.Controls.Find("mediaFilterIncomplete", true).OfType<CheckBox>())
                .ToArray();
            foreach (CheckBox filter in filters) filter.Checked = false;
            form.Controls.Find("mediaFilterOrange", true).OfType<CheckBox>().Single().Checked = true;
            Assert(VisibleAlbumTitles(tree).All(title => title == "Processed Album" || title.StartsWith("Done", StringComparison.Ordinal))
                && VisibleAlbumTitles(tree).Count == 3,
                "Orange status filtering did not show only processed Albums.");
            form.Controls.Find("mediaFilterOrange", true).OfType<CheckBox>().Single().Checked = false;
            form.Controls.Find("mediaFilterRed", true).OfType<CheckBox>().Single().Checked = true;
            Assert(VisibleAlbumTitles(tree).SequenceEqual(new[] { "Red Album" }),
                "Red status filtering did not show bypassed Albums.");
            form.Controls.Find("mediaFilterRed", true).OfType<CheckBox>().Single().Checked = false;
            form.Controls.Find("mediaFilterPurple", true).OfType<CheckBox>().Single().Checked = true;
            Assert(tree.Nodes.Cast<TreeNode>().Any(node => node.Text == "Alpha Artist")
                && VisibleAlbumTitles(tree).Contains("Waiting Album"),
                "Purple status filtering did not include partial Artists and timeout-active Albums.");
            form.Controls.Find("mediaFilterPurple", true).OfType<CheckBox>().Single().Checked = false;
            form.Controls.Find("mediaFilterGreen", true).OfType<CheckBox>().Single().Checked = true;
            Assert(tree.Nodes.Cast<TreeNode>().Any(node => node.Text == "Complete Artist")
                && tree.Nodes.Cast<TreeNode>().Any(node => node.Text == "Timeout Artist")
                && !tree.Nodes.Cast<TreeNode>().Any(node => node.Text == "Alpha Artist"),
                "Green status filtering did not show complete Artists.");
            form.Controls.Find("mediaFilterGreen", true).OfType<CheckBox>().Single().Checked = false;
            form.Controls.Find("mediaFilterBlue", true).OfType<CheckBox>().Single().Checked = true;
            Assert(tree.Nodes.Count == 1 && tree.Nodes[0].Text == "Bypass Artist",
                "Blue status filtering did not show Artists containing bypass.");

            form.Controls.Find("mediaFilterBlue", true).OfType<CheckBox>().Single().Checked = false;
            form.Controls.Find("mediaFilterIncomplete", true).OfType<CheckBox>().Single().Checked = true;
            Assert(VisibleAlbumTitles(tree).SequenceEqual(new[] { "Resume Album" }),
                "Incomplete status filtering did not remain independent from Artist Contains Bypass.");

            foreach (CheckBox filter in filters) filter.Checked = true;
            Assert(tree.Nodes.Count == 5, "Clearing Media Filter criteria did not restore all rows.");
            Assert(filterAlbums.All(item => item.State == originalStates[item.Path] && item.Selected == originalSelections[item.Path]),
                "Media Filter mutated underlying Album status or selection state.");

            artist.Text = "Alpha";
            foreach (CheckBox filter in filters) filter.Checked = true;
            selectFiltered.Checked = true;
            Assert(filterAlbums.Single(item => item.Title == "Fresh Album").Selected
                && filterAlbums.Single(item => item.Title == "Processed Album").Selected
                && filterAlbums.Where(item => item.Artist != "Alpha Artist").All(item => !item.Selected)
                && selectFiltered.Checked
                && !selectAll.Checked && !selectNone.Checked,
                "Select [FILTERED] did not select exactly the visible results and retain its green checked state.");
            selectAll.Checked = true;
            Assert(selectAll.Checked && !selectNone.Checked && !selectFiltered.Checked
                && filterAlbums.Single(item => item.Title == "Fresh Album").Selected
                && filterAlbums.Where(item => item.Title != "Fresh Album").All(item => !item.Selected),
                "Select [ALL] did not match Python by replacing selection with the active Artist's unprocessed Albums.");
            selectNone.Checked = true;
            Assert(selectNone.Checked && !selectAll.Checked && !selectFiltered.Checked && filterAlbums.All(item => !item.Selected),
                "Select [NONE] did not become the sole selected mode or clear all Album selections.");
            filterAlbums.ForEach(item => item.Selected = false);
            filterAlbums.Single(item => item.Title == "Fresh Album").Selected = true;
            filterAlbums.Single(item => item.Title == "Red Album").Selected = true;
            List<AlbumInfo> autoQueue = filterAlbums.Where(item => item.Selected && item.State != AlbumState.TimeoutActive).ToList();
            Assert(autoQueue.Select(item => item.Title).OrderBy(value => value).SequenceEqual(new[] { "Fresh Album", "Red Album" })
                && autoQueue.Count == 2,
                "The single LAUNCH expanded the checked Album set to visible or full-library rows.");

            AlbumInfo firstLaunch = Album("First Launch Artist", "First Launch Album", AlbumState.New);
            firstLaunch.Path = @"\\server\music\First Launch Artist\First Launch Album";
            firstLaunch.Selected = true;
            AlbumInfo nextArtist = Album("Next Artist", "Next Album", AlbumState.New);
            nextArtist.Path = @"\\server\music\Next Artist\Next Album";
            nextArtist.Selected = true;
            List<AlbumInfo> consecutiveLaunchAlbums = new List<AlbumInfo> { firstLaunch, nextArtist };
            albumsField.SetValue(form, consecutiveLaunchAlbums);
            UiState formUi = (UiState)typeof(MainForm).GetField("uiState", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            formUi.SelectedAlbumPaths = consecutiveLaunchAlbums.Select(item => item.Path).ToList();
            typeof(MainForm).GetField("activeLaunchAlbum", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, firstLaunch);
            MethodInfo applyEvent = typeof(MainForm).GetMethod("ApplyCoreEvent", BindingFlags.Instance | BindingFlags.NonPublic);
            applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
            {
                { "event", "album_completed" }, { "album_path", firstLaunch.Path + "\\" },
                { "action", "Installed" }, { "destination", "cover.jpg" }, { "mode", "write" }
            } });
            List<AlbumInfo> nextQueue = consecutiveLaunchAlbums.Where(item => item.Selected && item.State != AlbumState.TimeoutActive).ToList();
            Assert(!firstLaunch.Selected && firstLaunch.State == AlbumState.Processed
                && nextArtist.Selected && nextQueue.Count == 1 && Object.ReferenceEquals(nextQueue[0], nextArtist),
                "A completed launch album survived an equivalent trailing-separator path and led the next artist's queue.");

            AlbumInfo readOnlyAlbum = Album("Read Artist", "Read Album", AlbumState.New);
            readOnlyAlbum.Path = @"\\server\music\Read Artist\Read Album";
            consecutiveLaunchAlbums.Add(readOnlyAlbum);
            applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
            {
                { "event", "album_completed" }, { "album_path", readOnlyAlbum.Path },
                { "action", "ReadOnly" }, { "destination", "cover.jpg" }, { "mode", "read" }
            } });
            Assert(readOnlyAlbum.State == AlbumState.New,
                "READ review incorrectly marked an Album as processed.");

            MethodInfo setSelectorVisible = typeof(MainForm).GetMethod("SetMediaSelectorVisible", BindingFlags.Instance | BindingFlags.NonPublic);
            MethodInfo restoreSelector = typeof(MainForm).GetMethod("RestoreMediaSelectorAfterCompletedRun", BindingFlags.Instance | BindingFlags.NonPublic);
            setSelectorVisible.Invoke(form, new object[] { false, false });
            restoreSelector.Invoke(form, new object[] { false, false, 1 });
            SplitContainer mainSplit = (SplitContainer)typeof(MainForm).GetField("mainSplit", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            Assert(!mainSplit.Panel1Collapsed,
                "A successfully completed selected-Album run did not restore the hidden Media Album Selector.");

            firstLaunch.Selected = true;
            formUi.SelectedAlbumPaths = new List<string> { firstLaunch.Path };
            typeof(MainForm).GetField("activeLaunchAlbum", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, firstLaunch);
            applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
            {
                { "event", "album_completed" }, { "album_path", @"\\server\music\First Launch Artist\Big BambÃº" },
                { "action", "Installed" }, { "destination", "cover.jpg" }
            } });
            Assert(!firstLaunch.Selected,
                "A translated physical Album completion did not consume its proper-Unicode indexed selection.");

            typeof(MainForm).GetField("activeLaunchAlbum", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, nextArtist);
            typeof(MainForm).GetMethod("ConsumeLaunchAlbumSelection", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { nextArtist });
            nextQueue = consecutiveLaunchAlbums.Where(item => item.Selected && item.State != AlbumState.TimeoutActive).ToList();
            Assert(!nextArtist.Selected && nextQueue.Count == 0
                && !(formUi.SelectedAlbumPaths ?? new List<string>()).Any(path => path.IndexOf("Next Album", StringComparison.OrdinalIgnoreCase) >= 0),
                "A stopped/ended active launch album remained checked or persisted into the next launch queue.");
            typeof(MainForm).GetField("activeLaunchAlbum", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, null);
        }

        private static List<string> VisibleAlbumTitles(TreeView tree)
        {
            return tree.Nodes.Cast<TreeNode>().SelectMany(node => node.Nodes.Cast<TreeNode>()).Select(node => node.Text).ToList();
        }

        private static void VerifyWatermarkAndIcon()
        {
            UiState original = ConfigStore.LoadUi();
            MethodInfo loadWatermark = typeof(ThemeManager).GetMethod("LoadWatermark", BindingFlags.Static | BindingFlags.NonPublic);
            object initialResource = loadWatermark.Invoke(null, null);
            Assert(initialResource != null, "The SPLINED watermark resource could not be loaded.");
            foreach (string theme in new[] { "Dark", "Light", "System", "Dark", "Light" })
            {
                UiState themed = ConfigStore.LoadUi();
                themed.Theme = theme;
                ConfigStore.SaveUi(themed);
                using (WatermarkFlowLayoutPanel panel = new WatermarkFlowLayoutPanel())
                using (Bitmap rendered = new Bitmap(640, 420))
                {
                    panel.Size = rendered.Size;
                    ThemeManager.Apply(panel, theme);
                    panel.CreateControl();
                    panel.DrawToBitmap(rendered, new Rectangle(Point.Empty, rendered.Size));
                    Color baseColor = rendered.GetPixel(3, 3);
                    int visiblePixels = 0;
                    for (int y = 0; y < rendered.Height; y += 4)
                        for (int x = 0; x < rendered.Width; x += 4)
                        {
                            Color pixel = rendered.GetPixel(x, y);
                            if (Math.Abs(pixel.R - baseColor.R) + Math.Abs(pixel.G - baseColor.G) + Math.Abs(pixel.B - baseColor.B) > 5)
                                visiblePixels++;
                        }
                    Assert(visiblePixels > 100, "The watermark disappeared after switching to " + theme + " theme.");
                }
                Assert(Object.ReferenceEquals(initialResource, loadWatermark.Invoke(null, null)),
                    "Theme switching recreated or discarded the watermark resource.");
            }
            ConfigStore.SaveUi(original);
            ThemeManager.Initialize(original.Theme);

            using (Bitmap icon = EmbeddedAssets.LoadApplicationIconImage())
            {
                Assert(icon.GetPixel(0, 0).A == 0 && icon.GetPixel(icon.Width - 1, icon.Height - 1).A == 0,
                    "The SPLINED S icon did not preserve a transparent background.");
                int red = 0, green = 0, blue = 0;
                for (int y = 0; y < icon.Height; y += 4)
                    for (int x = 0; x < icon.Width; x += 4)
                    {
                        Color pixel = icon.GetPixel(x, y);
                        if (pixel.A < 100) continue;
                        if (pixel.R > pixel.G + 30 && pixel.R > pixel.B + 30) red++;
                        if (pixel.G > pixel.R + 20 && pixel.G > pixel.B + 10) green++;
                        if (pixel.B > pixel.R + 20 && pixel.B > pixel.G + 10) blue++;
                    }
                Assert(red > 100 && green > 100 && blue > 100,
                    "The application icon is not the multicolor angular SPLINED S.");
            }
            using (Icon executableIcon = Icon.ExtractAssociatedIcon(Application.ExecutablePath))
                Assert(executableIcon != null, "The packaged executable does not expose a Windows icon resource.");
        }

        private static void VerifyThemeSystem()
        {
            ThemePalette dark = ThemeManager.PaletteFor("Dark");
            ThemePalette light = ThemeManager.PaletteFor("Light");
            ThemePalette system = ThemeManager.PaletteFor("System");
            Assert(dark.Dark && !light.Dark && (Object.ReferenceEquals(system, dark) || Object.ReferenceEquals(system, light)),
                "System theme did not resolve through the central Dark/Light palette.");
            Assert(dark.WindowBackground != dark.SurfacePrimary && dark.SurfacePrimary != dark.SurfaceRaised
                && light.WindowBackground != light.SurfacePrimary && light.SurfacePrimary != light.SurfaceRaised,
                "Theme surfaces do not retain the intended visual hierarchy.");
            Assert(dark.WindowBackground.B > dark.WindowBackground.R
                && dark.PanelSurface != dark.NestedCardSurface && dark.TreeSurface != dark.PanelSurface
                && dark.PrimaryAction != dark.ButtonBackground && dark.TopNavigationSurface != dark.TitlebarBackground,
                "The dark simulation palette is missing its centralized navy surface/action hierarchy.");
            Assert(dark.AccentPrimary != dark.StatusGreen && dark.AccentPrimary != dark.StatusRed
                && light.AccentPrimary != light.StatusGreen && light.AccentPrimary != light.StatusRed,
                "General action accents reused a semantic folder-status color.");
            Assert(dark.ButtonActive != dark.StatusGreen && dark.ButtonActive != dark.StatusOrange
                && light.ButtonActive != light.StatusGreen && light.ButtonActive != light.StatusOrange,
                "Active-button tokens reused semantic library-status colors.");
            Assert(dark.StatusPurple != dark.CategoryMagenta && dark.StatusPurple != dark.StatusBlue
                && dark.StatusPurple.R < 180 && dark.StatusPurple.B > dark.StatusPurple.R,
                "Dark purple status text is still too light or reuses a MusicBrainz category color.");
            Assert(ThemeManager.StatusColor(ThemeStatusColor.Orange, "Dark") == dark.StatusOrange
                && ThemeManager.StatusColor(ThemeStatusColor.Blue, "Light") == light.StatusBlue,
                "Folder status colors are no longer sourced from the semantic status palette.");
            Assert(typeof(FluentMenuRenderer).GetMethod("OnRenderMenuItemBackground", BindingFlags.Instance | BindingFlags.NonPublic).DeclaringType == typeof(FluentMenuRenderer)
                && typeof(FluentMenuRenderer).GetMethod("OnRenderItemCheck", BindingFlags.Instance | BindingFlags.NonPublic).DeclaringType == typeof(FluentMenuRenderer)
                && typeof(FluentMenuRenderer).GetMethod("OnRenderToolStripBorder", BindingFlags.Instance | BindingFlags.NonPublic).DeclaringType == typeof(FluentMenuRenderer),
                "The menu renderer fell back to native square hover, check, or popup-border painting.");
            Rectangle menuHighlight = FluentMenuRenderer.HighlightBounds(new Size(120, 30), false);
            using (GraphicsPath menuPath = ThemeManager.RoundedPath(menuHighlight, FluentMenuRenderer.HighlightRadius))
            {
                Assert(!menuPath.IsVisible(menuHighlight.Left, menuHighlight.Top)
                    && menuPath.IsVisible(menuHighlight.Left + menuHighlight.Width / 2, menuHighlight.Top + menuHighlight.Height / 2),
                    "The menu highlight geometry still paints square outer corners.");
            }

            using (Form form = new Form())
            using (FluentGroupBox group = new FluentGroupBox { Text = "Theme QA", Size = new Size(340, 120) })
            using (Button button = new FluentButton { Text = "Disabled", Size = new Size(120, 34), Enabled = false })
            using (ComboBox combo = new FluentComboBox { Width = 150 })
            using (NumericUpDown number = new FluentNumericUpDown { Width = 100, Value = 12 })
            using (CheckBox check = new FluentCheckBox { Text = "Preserved", Checked = true })
            using (TextBox text = new FluentTextBox { Width = 150, Text = "Theme input" })
            {
                combo.Items.AddRange(new object[] { "One", "Two" });
                combo.SelectedIndex = 0;
                group.Controls.Add(button);
                group.Controls.Add(combo);
                group.Controls.Add(number);
                group.Controls.Add(check);
                group.Controls.Add(text);
                form.Controls.Add(group);
                ThemeManager.Apply(form, "Dark");
                Assert(!form.IsHandleCreated && !combo.IsHandleCreated,
                    "Theme application forced control handles during hidden construction.");
                Assert(form.BackColor == dark.WindowBackground && group.BackColor == dark.NestedCardSurface
                    && button.ForeColor == dark.TextDisabled && combo.BackColor == dark.InputBackground
                    && combo.DrawMode == DrawMode.OwnerDrawFixed
                    && number.BorderStyle == BorderStyle.None && text.BorderStyle == BorderStyle.None && check.Checked,
                    "Dark theme application did not style controls centrally or changed control state.");
                ThemeManager.Apply(form, "Light");
                Assert(form.BackColor == light.WindowBackground && group.BackColor == light.NestedCardSurface
                    && button.ForeColor == light.TextDisabled && combo.BackColor == light.InputBackground
                    && number.Value == 12 && check.Checked,
                    "Light theme switching lost styling or control state.");
            }

            using (ToolTip tip = ThemeManager.CreateToolTip())
                Assert(tip.OwnerDraw && tip.ShowAlways,
                    "Tooltips are not using the opaque shared theme renderer.");

            using (FluentCheckBox stateCheck = new FluentCheckBox { Size = new Size(26, 24), Checked = true })
            using (Bitmap checkedImage = new Bitmap(26, 24))
            using (Bitmap uncheckedImage = new Bitmap(26, 24))
            {
                ThemeManager.Apply(stateCheck, "Dark");
                stateCheck.DrawToBitmap(checkedImage, new Rectangle(Point.Empty, checkedImage.Size));
                stateCheck.Checked = false;
                stateCheck.DrawToBitmap(uncheckedImage, new Rectangle(Point.Empty, uncheckedImage.Size));
                Assert(IsCloserTo(checkedImage.GetPixel(9, 12), dark.CheckOnBackground, dark.CheckOffBackground)
                    && IsCloserTo(uncheckedImage.GetPixel(2, 12), dark.CheckOffBackground, dark.CheckOnBackground)
                    && !IsCloserTo(uncheckedImage.GetPixel(9, 12), dark.CheckOffBackground, dark.CheckOnBackground),
                    "Radio states are not solid green when selected and red outlines when unselected.");
            }

            ThemeManager.Initialize("Dark");
            using (Form focusHost = new Form
            {
                FormBorderStyle = FormBorderStyle.None,
                ShowInTaskbar = false,
                StartPosition = FormStartPosition.Manual,
                Location = new Point(-32000, -32000),
                Size = new Size(190, 80)
            })
            using (Button active = new FluentButton { Text = "WAITING", Size = new Size(150, 38), Location = new Point(20, 20), Tag = "waiting" })
            {
                focusHost.Controls.Add(active);
                ThemeManager.StyleButton(active, "Dark");
                ThemeManager.Apply(focusHost, "Dark");
                focusHost.Show();
                active.Focus();
                Assert(active is FluentButton && active.Focused && active.FlatAppearance.BorderSize == 0 && active.Region == null,
                    "Shared owner-painted action buttons still depend on a clipped native window region.");
                using (Bitmap rendered = new Bitmap(active.Width, active.Height))
                {
                    active.DrawToBitmap(rendered, new Rectangle(Point.Empty, rendered.Size));
                    Color corner = rendered.GetPixel(0, 0);
                    Color edge = rendered.GetPixel(0, active.Height / 2);
                    Assert(corner.ToArgb() != edge.ToArgb(),
                        "Rendered focused-button corner is still filled like a square control.");
                }
                focusHost.Hide();
            }
            using (FluentButton bounded = new FluentButton { Text = "LAUNCH", Size = new Size(150, 38) })
            using (Bitmap canvas = new Bitmap(230, 118))
            using (Graphics graphics = Graphics.FromImage(canvas))
            {
                Color guard = Color.Magenta;
                graphics.Clear(guard);
                GraphicsState shifted = graphics.Save();
                graphics.TranslateTransform(40, 40);
                ThemeManager.StyleButton(bounded, "Dark");
                ThemeManager.DrawButton(bounded, graphics, ThemeManager.PaletteFor("Dark"), false, false);
                graphics.Restore(shifted);
                Assert(canvas.GetPixel(5, 5).ToArgb() == guard.ToArgb()
                    && canvas.GetPixel(225, 113).ToArgb() == guard.ToArgb(),
                    "Owner-painted controls wrote outside their translated client rectangle.");
            }
            string sourceRoot = ConfigStore.AppRoot;
            while (!File.Exists(Path.Combine(sourceRoot, "ThemeManager.cs")) && Directory.GetParent(sourceRoot) != null)
                sourceRoot = Directory.GetParent(sourceRoot).FullName;
            string themeSource = File.ReadAllText(Path.Combine(sourceRoot, "ThemeManager.cs"));
            Assert(themeSource.IndexOf("private const int WmPrint", StringComparison.OrdinalIgnoreCase) < 0
                && themeSource.IndexOf("Graphics.FromHdc", StringComparison.OrdinalIgnoreCase) < 0,
                "Shared controls still paint directly into a WM_PRINT parent HDC.");
            Assert(ReleaseInfo.NumericVersion == "3.0.0" && ReleaseInfo.SemanticVersion == "3.0.0"
                && ReleaseInfo.Channel == "Stable" && ReleaseInfo.WindowTitle == "S:P:L:I:N:E:D"
                && ReleaseInfo.VersionLabel == "v3.0.0 Stable"
                && ReleaseInfo.DisplayName == "S:P:L:I:N:E:D v3.0.0 Stable"
                && ReleaseInfo.PackageName == "SPLINED-Windows-GUI-3.0.0-Stable-v3",
                "The authoritative Windows Stable v3 release identity is inconsistent.");
            Assert(ReleaseInfo.RepositoryUrl == "https://github.com/scottia/S-P-L-I-N-E-D"
                && ReleaseInfo.HelpUrl == "https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/README.md"
                && ReleaseInfo.ReleasesUrl == "https://github.com/scottia/S-P-L-I-N-E-D/releases/latest",
                "Repository, Help, and release URLs are not centralized on their stable public targets.");
        }

        private static void Assert(bool condition, string message)
        {
            if (!condition) throw new InvalidOperationException(message);
        }

        private static bool IsCloserTo(Color value, Color expected, Color alternative)
        {
            int expectedDistance = Math.Abs(value.R - expected.R) + Math.Abs(value.G - expected.G) + Math.Abs(value.B - expected.B);
            int alternativeDistance = Math.Abs(value.R - alternative.R) + Math.Abs(value.G - alternative.G) + Math.Abs(value.B - alternative.B);
            return expectedDistance < alternativeDistance;
        }

        private static bool IsDescendant(Control ancestor, Control control)
        {
            for (Control current = control; current != null; current = current.Parent)
                if (Object.ReferenceEquals(current, ancestor)) return true;
            return false;
        }

        private static IEnumerable<Control> Descendants(Control root)
        {
            foreach (Control child in root.Controls)
            {
                yield return child;
                foreach (Control descendant in Descendants(child))
                    yield return descendant;
            }
        }
    }
}
