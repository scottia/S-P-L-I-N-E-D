using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Collections;
using System.Collections.Generic;
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
                string library = Path.Combine(ConfigStore.AppRoot, "fixture-library");
                string firstAlbum = Path.Combine(library, "Artist One", "Album One");
                string ignoredAlbum = Path.Combine(library, "Skip This", "Ignored Album");
                Directory.CreateDirectory(firstAlbum);
                Directory.CreateDirectory(ignoredAlbum);
                File.WriteAllBytes(Path.Combine(firstAlbum, "track.mp3"), new byte[] { 0 });
                File.WriteAllBytes(Path.Combine(ignoredAlbum, "track.mp3"), new byte[] { 0 });
                string fixtureCover = Path.Combine(firstAlbum, "cover.jpg");
                if (File.Exists(fixtureCover)) File.Delete(fixtureCover);

                ConfigState state = ConfigStore.Defaults();
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
                state.SplineAiEnabled = true;
                state.SplineAiEndpoint = "internal-placeholder";
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
                string configText = File.ReadAllText(loaded.ConfigPath);
                Assert(configText.Contains("[credentials]") && configText.Contains("credential_dir =")
                    && !configText.Contains("[fanarttv]") && !configText.Contains("[lastfm]")
                    && !configText.Contains("[musicbrainz]") && !configText.Contains("credential_file")
                    && !configText.Contains("token_file") && !configText.Contains("client_id"),
                    "Config v5 must contain only the credential directory, never provider filenames or credential fields.");
                Assert(CredentialStore.PathFor(loaded, "fanarttv") == Path.Combine(loaded.CredentialDir, "fanarttv.json")
                    && CredentialStore.PathFor(loaded, "lastfm") == Path.Combine(loaded.CredentialDir, "lastfm.json")
                    && CredentialStore.PathFor(loaded, "musicbrainz") == Path.Combine(loaded.CredentialDir, "musicbrainz.json"),
                    "Provider credential files were not derived from the credential directory.");
                Assert(loaded.SplineAiEnabled && loaded.SplineAiEndpoint == "internal-placeholder", "Hidden SPLINEAI values were not preserved.");
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

                ConfigState optionCoverage = loaded.Clone();
                optionCoverage.ConfigPath = Path.Combine(ConfigStore.AppRoot, "all-options-config.toml");
                optionCoverage.Mode = "read";
                optionCoverage.Verbosity = "trace";
                optionCoverage.ScanLibraryDir = firstAlbum;
                optionCoverage.ScanMode = false;
                optionCoverage.LibraryScan = true;
                optionCoverage.ScanModeTimeout = 12.5;
                optionCoverage.CacheDir = Path.Combine(ConfigStore.AppRoot, "coverage-cache");
                optionCoverage.LogDir = Path.Combine(ConfigStore.AppRoot, "coverage-logs");
                optionCoverage.HistoryDir = Path.Combine(ConfigStore.AppRoot, "coverage-history");
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
                Assert(optionReopened.CacheDir == optionCoverage.CacheDir && optionReopened.LogDir == optionCoverage.LogDir
                    && optionReopened.HistoryDir == optionCoverage.HistoryDir && optionReopened.CredentialDir == optionCoverage.CredentialDir,
                    "Python directory options did not round-trip through Config v5.");
                Assert(optionReopened.Formats.SequenceEqual(optionCoverage.Formats)
                    && optionReopened.Sources.SequenceEqual(optionCoverage.Sources)
                    && optionReopened.ExcludedSources.SequenceEqual(optionCoverage.ExcludedSources),
                    "Output format or artwork-source order/exclusion did not round-trip.");
                Assert(optionReopened.LogRetentionDays == 33 && optionReopened.HistoryEnabled && optionReopened.HistoryRetentionDays == 90
                    && !optionReopened.SampleWrite && optionReopened.FileName == "folder" && !optionReopened.PreserveFile
                    && !optionReopened.Square && optionReopened.SquareMode == "off" && optionReopened.SquareRoundTo == 8
                    && optionReopened.UpscaleBelowIdeal && !optionReopened.EvaluateFinalImage,
                    "Python output/sample or Config v5 retention options did not round-trip.");
                Assert(optionReopened.RangeMin == 1000 && optionReopened.RangeIdeal == 1700
                    && optionReopened.RangeMax == 2300 && optionReopened.RangeLadder == 3500,
                    "Artwork resolution range did not round-trip.");
                ConfigStore.Save(loaded);

                AlbumInfo[] albums = LibraryInventory.Load(loaded).ToArray();
                Assert(albums.Length == 1 && albums[0].Title == "Album One", "Library inventory did not honor ignored directories.");
                Assert(albums[0].State == AlbumState.New && albums[0].EligibleByDefault, "A new album was not eligible by default.");

                loaded.HistoryDir = Path.Combine(ConfigStore.AppRoot, "history-fixture");
                Directory.CreateDirectory(loaded.HistoryDir);
                Dictionary<string, object> historyEntry = new Dictionary<string, object>();
                historyEntry["completed_at_unix"] = (DateTime.UtcNow - new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc)).TotalSeconds;
                historyEntry["outcome"] = "installed";
                historyEntry["album_fingerprint"] = HistoryResolver.AlbumFingerprint(albums[0]);
                historyEntry["policy_fingerprint"] = HistoryResolver.PolicyFingerprint(loaded);
                Dictionary<string, object> historyAlbums = new Dictionary<string, object>();
                historyAlbums[albums[0].Path] = historyEntry;
                File.WriteAllText(Path.Combine(loaded.HistoryDir, "scan-completed-history.json"),
                    new JavaScriptSerializer().Serialize(new Dictionary<string, object> { { "albums", historyAlbums } }));
                HistoryResolver.Apply(loaded, albums);
                Assert(albums[0].State == AlbumState.Processed && !albums[0].EligibleUtc.HasValue,
                    "Timeout 0 must leave processed albums manually eligible instead of purple/blocked.");

                VerifyArtistAggregateAndSelectionRules();

                ConfigState filesystemState = loaded.Clone();
                filesystemState.HistoryEnabled = false;
                File.WriteAllBytes(fixtureCover, new byte[] { 0xFF, 0xD8, 0xFF, 0xD9 });
                AlbumInfo filesystemAlbum = LibraryInventory.Load(filesystemState).Single(album => album.Path == firstAlbum);
                Assert(filesystemAlbum.HasLocalArtwork && filesystemAlbum.State == AlbumState.Processed,
                    "Library reload did not reconcile an existing cover* file into processed album state.");

                UiState ui = new UiState
                {
                    Theme = "Dark", ShowStatusOnLaunch = false, ShowConfirmations = false, HoverEnabled = true,
                    MediaFilterExpanded = false, MediaArtistFilter = "Alpha", MediaAlbumFilter = "Fresh",
                    MediaShowRed = false, FilteredScanMode = "read",
                    SelectedAlbumPaths = new List<string> { firstAlbum },
                    MainWidth = 1320, MainHeight = 860, MainX = 110, MainY = 90,
                    MainSplitterDistance = 455, RightSplitterDistance = 305,
                    SetupWidth = 1040, SetupHeight = 820, SetupX = 130, SetupY = 100,
                    SetupPrimaryTab = 1, SetupAdvancedTab = 2,
                    CompareWidth = 1110, CompareHeight = 710, PreviewWidth = 610, PreviewHeight = 650
                };
                ConfigStore.SaveUi(ui);
                UiState loadedUi = ConfigStore.LoadUi();
                Assert(loadedUi.Theme == "Dark" && !loadedUi.ShowStatusOnLaunch && !loadedUi.ShowConfirmations && loadedUi.HoverEnabled
                    && !loadedUi.MediaFilterExpanded && loadedUi.MediaArtistFilter == "Alpha" && !loadedUi.MediaShowRed
                    && loadedUi.FilteredScanMode == "read" && loadedUi.SelectedAlbumPaths.SequenceEqual(new[] { firstAlbum })
                    && loadedUi.MainWidth == 1320 && loadedUi.MainSplitterDistance == 455
                    && loadedUi.SetupWidth == 1040 && loadedUi.SetupAdvancedTab == 2
                    && loadedUi.CompareWidth == 1110 && loadedUi.PreviewHeight == 650, "GUI-local ui.toml did not round-trip.");
                string savedConfigBeforeTemporaryRun = File.ReadAllText(loaded.ConfigPath);
                string temporaryRunConfig = Path.Combine(ConfigStore.AppRoot, "temporary-filtered-run.toml");
                ConfigState temporaryRunState = loaded.Clone();
                temporaryRunState.Mode = "read";
                ConfigStore.SaveTemporaryRunConfig(temporaryRunState, temporaryRunConfig);
                Assert(File.ReadAllText(temporaryRunConfig).Contains("mode = \"read\"")
                    && File.ReadAllText(loaded.ConfigPath) == savedConfigBeforeTemporaryRun,
                    "A temporary filtered Read/Write override changed the saved Config v5 document.");
                File.Delete(temporaryRunConfig);

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
                    Assert(File.Exists(Path.Combine(ConfigStore.AppRoot, "splined-watermark.png")),
                        "The responsive SPLINED watermark asset is missing.");
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
                    Button restorePortable = Descendants(pathActions).OfType<Button>().Single(button => button.Text == "Restore Portable Defaults");
                    Assert(IsDescendant(pathActions, pathCredentials) && IsDescendant(pathActions, restorePortable)
                        && Math.Abs(pathCredentials.Top - restorePortable.Top) <= 2,
                        "Credential / Status and Restore Portable Defaults are not aligned in the Paths action row.");
                    Control artworkOptions = setup.Controls.Find("artworkOptionsGroup", true).Single();
                    Dictionary<string, Button> formats = (Dictionary<string, Button>)typeof(SetupForm).GetField("formatButtons", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(setup);
                    Assert(formats.Count == 3 && formats.Values.All(button => IsDescendant(artworkOptions, button)),
                        "Output format buttons were not placed beside Artwork Options.");
                    ComboBox existingArtwork = setup.Controls.Find("existingArtworkAction", true).OfType<ComboBox>().Single();
                    Assert(existingArtwork is FluentComboBox && existingArtwork.Items.Count == 2 && existingArtwork.SelectedIndex == 1
                        && Convert.ToString(existingArtwork.Items[0]).Contains("no numbered copies"),
                        "Existing-artwork overwrite/preserve behavior is not visible or did not repopulate.");
                    string[] commandButtons = { "Validate Saved Config", "Credentials / Status...", "Open Config Folder", "Open Logs Folder" };
                    Assert(commandButtons.All(expected => Descendants(setup).OfType<Button>().Any(button => button.Text == expected)),
                        "Advanced Settings omitted Config validation, credentials, or diagnostic-folder tools.");
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
                CredentialStore.Save(loaded, "lastfm", new Dictionary<string, object> { { "api_key", "fixture-lastfm-key" } });
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
                loaded.SourcePolicies["musicbrainz"].SourceOverride = true;
                ConfigStore.Save(loaded);
                ConfigState reopened = ConfigStore.Load();
                Assert(!reopened.SourcePolicies["musicbrainz"].Enabled && reopened.SourcePolicies["musicbrainz"].SourceOverride,
                    "MusicBrainz Source Enabled / Source Override did not round-trip.");
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
                        if (Convert.ToString(selector.Items[index]) == "MusicBrainz") selector.SelectedIndex = index;
                    Application.DoEvents();
                    Assert((int)musicBrainzSetup.Controls.Find("musicBrainzRetryMax", true).OfType<NumericUpDown>().Single().Value == 8
                        && Math.Abs((double)musicBrainzSetup.Controls.Find("musicBrainzMinDelay", true).OfType<NumericUpDown>().Single().Value - 1.25) < 0.001
                        && (int)musicBrainzSetup.Controls.Find("musicBrainzRecordingTimeout", true).OfType<NumericUpDown>().Single().Value == 11,
                        "Reopened Settings did not repopulate all MusicBrainz runtime options.");
                    Assert(musicBrainzSetup.Controls.Find("musicBrainzOptionsGroup", true).Single().Visible
                        && !musicBrainzSetup.Controls.Find("artworkResolutionGroup", true).Single().Visible,
                        "MusicBrainz did not suppress inapplicable artwork-only controls.");
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

                loadedUi.HoverEnabled = false;
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
                        "The main LAUNCH control is not the first button in the Artwork Candidates bottom action row.");
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
                        && form.MainMenuStrip.AutoSize && form.MainMenuStrip.Dock == DockStyle.Top,
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
                    Assert(!Descendants(form).OfType<Label>().Any(label => label.Text == "Choose Select, All, or None. Launch uses checked albums only."
                            || label.Text == "Live discovery, validation, provider results, and write/read outcomes appear here."
                            || label.Text == "Real provider candidates for the current album will appear below."),
                        "A removed panel helper sentence is still consuming visible layout space.");
                    foreach (string titleText in new[] { "Media Library Selection", "Scan Activity and Decisions", "Artwork Candidates and Preview" })
                    {
                        Label titleLabel = Descendants(form).OfType<Label>().Single(label => label.Text == titleText);
                        Assert(titleLabel.Parent.Controls.OfType<InfoButton>().Any(), titleText + " does not have its replacement information tooltip beside the heading.");
                    }
                    Assert(form.Controls.Find("mediaLibrarySelectionCard", true).Single() is FluentCardTableLayoutPanel
                        && form.Controls.Find("scanActivityCard", true).Single() is FluentCardTableLayoutPanel
                        && form.Controls.Find("artworkCandidatesCard", true).Single() is FluentCardTableLayoutPanel,
                        "The three major work areas do not use the shared rounded panel surface.");
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
                    FieldInfo runningField = typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic);
                    FieldInfo awaitingField = typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic);
                    MethodInfo updateSelection = typeof(MainForm).GetMethod("UpdateSelectionControls", BindingFlags.Instance | BindingFlags.NonPublic);
                    runningField.SetValue(form, true);
                    updateSelection.Invoke(form, null);
                    Assert(launch.Enabled && launch.Text == "WAITING" && Convert.ToString(launch.Tag) == "waiting",
                        "LAUNCH must become the orange WAITING state while an artwork decision is pending.");
                    Assert(launch.FlatAppearance.BorderSize == 0 && launch.Region == null,
                        "LAUNCH / WAITING still clips antialiasing through a rounded HWND region.");
                    Button filteredLaunch = form.Controls.Find("selectAndLaunch", true).OfType<Button>().Single();
                    Assert(filteredLaunch.Enabled && filteredLaunch.Text == "WAITING" && Convert.ToString(filteredLaunch.Tag) == "waiting",
                        "Auto Mode mini-LAUNCH did not mirror the orange WAITING state.");
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
                        "Enable Hover did not toggle off and persist through the GUI-local ui.toml mechanism.");
                    candidateMap.Clear();
                    candidateMap[1] = new CandidateView { Index = 1 };
                    candidateCards.Controls.Add(new Panel());
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "album_completed" }, { "album_path", "fixture" },
                        { "action", "Installed" }, { "destination", "cover.jpg" }
                    } });
                    Assert(candidateMap.Count == 0 && candidateCards.Controls.Count == 0 && !(bool)awaitingField.GetValue(form),
                        "Completed artwork decisions must clear the visible candidate search.");
                    Assert(launch.Enabled && launch.Text == "STOP", "STOP must remain enabled while processing.");
                    RichTextBox activity = (RichTextBox)typeof(MainForm).GetField("activity", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
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
                    applyEvent.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "album_completed" }, { "album_path", "report-fixture" },
                        { "action", "Installed" }, { "destination", "cover.jpg" }
                    } });
                    finishRunStats.Invoke(form, new object[] { "Incomplete" });
                    showRunReport.Invoke(form, new object[] { "write", 1 });
                    Assert(activity.Text.StartsWith("S:P:L:I:N:E:D ALBUM RUN REPORT", StringComparison.Ordinal)
                        && activity.Text.Contains("[1/1] Report Artist - Report Album")
                        && activity.Text.Contains("2 evaluated") && activity.Text.Contains("2 policy-hidden")
                        && activity.Text.Contains("Outcome: Installed") && activity.Text.Contains("Result: cover.jpg")
                        && !activity.Text.Contains("SPLINED LIVE PROCESSING"),
                        "Completed processing did not replace live activity with the per-album statistics report.");

                    FlowLayoutPanel candidateActions = form.Controls.Find("candidateActionRow", true).OfType<FlowLayoutPanel>().Single();
                    candidateActions.PerformLayout();
                    Assert(candidateActions.Padding.Bottom >= 7
                        && candidateActions.Controls.OfType<Button>().Where(button => button.Visible)
                            .All(button => button.Bottom + button.Margin.Bottom <= candidateActions.ClientSize.Height),
                        "A bottom candidate action button is still clipped by its row.");

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
                }

                Console.WriteLine("PASS: Stable v3 identity and About surface, Fluent Compact buttons/dropdowns/spinners/checkboxes/tabs, persisted panel sizes, relocated bottom-row LAUNCH, Select/Scan Mode controls, unclipped Select and candidate action rows, clean title tooltips and tree-state images, semantic Folder status legend, blue Activity surface, segmented album headings, per-album completion statistics, consumed launch selections across completion/STOP and normalized UNC paths, nested View/Appearance menu, pre-display theme initialization, centralized Dark/Light/System theme transitions, DPI-aware rounded action/focus geometry, 100/125/150% responsive layout paths, Config v5 and credential isolation, reorganized Settings, equal retention panels, source range preview, authoritative cover/history reconciliation, artist aggregate/selection rules, live in-memory filtering, persisted hover action, theme-stable watermark, multicolor icon resources, source policy, fallback, UNC handling, and LAUNCH/WAITING/STOP lifecycle.");
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

            List<AlbumInfo> selection = new List<AlbumInfo> { whiteOne, orange, red, purple };
            ArtistSelectionRules.Apply(selection, true, false);
            Assert(whiteOne.Selected && !orange.Selected && !red.Selected && !purple.Selected,
                "Artist selection did not select only eligible WHITE Albums by default.");
            Assert(!red.BypassOverride && red.State == AlbumState.Bypassed,
                "A RED Album was selected or its saved bypass state changed without confirmation.");
            Assert(purple.State == AlbumState.TimeoutActive,
                "Artist selection changed timeout-active Album authority.");

            ArtistSelectionRules.Apply(selection, false, false);
            ArtistSelectionRules.Apply(selection, true, true);
            Assert(whiteOne.Selected && red.Selected && red.BypassOverride && red.State == AlbumState.Bypassed,
                "A confirmed temporary RED override did not select the Album while retaining bypass authority.");
            Assert(!orange.Selected && !purple.Selected,
                "ORANGE or timeout-active PURPLE Albums were auto-selected by Artist selection.");
        }

        private static AlbumInfo Album(string artist, string title, AlbumState state)
        {
            return new AlbumInfo { Artist = artist, Title = title, Path = artist + "\\" + title, State = state };
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
            foreach (string filterName in new[] { "mediaFilterWhite", "mediaFilterOrange", "mediaFilterRed", "mediaFilterPurple", "mediaFilterGreen", "mediaFilterBlue" })
                form.Controls.Find(filterName, true).OfType<CheckBox>().Single().Checked = true;
            Dictionary<string, AlbumState> originalStates = filterAlbums.ToDictionary(item => item.Path, item => item.State);
            Dictionary<string, bool> originalSelections = filterAlbums.ToDictionary(item => item.Path, item => item.Selected);

            MethodInfo setExpanded = typeof(MainForm).GetMethod("SetMediaFilterExpanded", BindingFlags.Instance | BindingFlags.NonPublic);
            TableLayoutPanel libraryLayout = (TableLayoutPanel)typeof(MainForm).GetField("libraryLayout", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            Button collapse = form.Controls.Find("mediaFilterToggle", true).OfType<Button>().Single();
            setExpanded.Invoke(form, new object[] { false });
            Assert(Math.Abs(libraryLayout.RowStyles[1].Height - 36) < 0.1 && collapse.Text.Contains("▸"),
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
            Assert(statusBullets.Count == 6 && statusDescriptions.Count == 6,
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
            Button autoLaunch = form.Controls.Find("selectAndLaunch", true).OfType<Button>().Single();
            CheckBox selectAll = form.Controls.Find("selectModeAll", true).OfType<CheckBox>().Single();
            CheckBox selectNone = form.Controls.Find("selectModeNone", true).OfType<CheckBox>().Single();
            CheckBox selectFiltered = form.Controls.Find("selectModeFiltered", true).OfType<CheckBox>().Single();
            Assert(scanRead.Text == "Filtered Scan [READ]" && scanWrite.Text == "Filtered Scan [WRITE]"
                && ((GroupBox)scanRead.Parent.Parent).Text == "Scan Mode"
                && ((GroupBox)selectFiltered.Parent.Parent).Text == "Select Mode"
                && selectAll.Text == "Select [ALL]" && selectNone.Text == "Select [NONE]" && selectFiltered.Text == "Select [FILTERED]",
                "Select Mode or Scan Mode labels do not match the requested compact design.");
            foreach (GroupBox modeGroup in new[] { (GroupBox)selectFiltered.Parent.Parent, (GroupBox)scanRead.Parent.Parent })
            {
                modeGroup.PerformLayout();
                Control clipped = modeGroup.Controls.Cast<Control>().SelectMany(control => control.Controls.Cast<Control>())
                    .FirstOrDefault(control => control.Right > control.Parent.ClientSize.Width || control.Bottom > control.Parent.ClientSize.Height);
                Assert(clipped == null,
                    modeGroup.Text + " contains clipped option text or checkbox rows (group " + modeGroup.ClientSize.Width + "x" + modeGroup.ClientSize.Height
                    + (clipped == null ? "" : ", control " + clipped.Text + " at " + clipped.Bounds + " in " + clipped.Parent.ClientSize) + ").");
            }
            foreach (CheckBox option in new[] { selectAll, selectNone, selectFiltered, scanRead, scanWrite })
                Assert(option.Width >= TextRenderer.MeasureText(option.Text, option.Font).Width + 28,
                    option.Text + " does not have enough themed checkbox width to render without ellipsis.");
            Assert(scanRead.Checked && !scanWrite.Checked && !selectFiltered.Checked,
                "Persisted Filtered Scan [READ] choice was not restored without selecting Auto Mode.");
            scanRead.Checked = false;
            Assert(!autoLaunch.Enabled, "Auto Mode mini-LAUNCH must be gray until a Scan Mode is chosen.");
            scanRead.Checked = true;
            Assert(scanRead.Checked && !scanWrite.Checked && !scanWrite.Enabled,
                "Filtered Scan [READ] did not gray the mutually exclusive Write choice.");
            Assert(autoLaunch.Enabled && autoLaunch.Text == "AUTO LAUNCH" && Convert.ToString(autoLaunch.Tag) == "success"
                && autoLaunch.ForeColor == ThemeManager.CurrentPalette.Success,
                "Auto Mode mini-LAUNCH did not become green/active after selecting a Scan Mode.");
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
                "The media tree did not receive the shared red/green checkbox images.");
            Assert(!tree.ShowNodeToolTips,
                "The folder tree still uses native tooltip rendering instead of the clean owner-drawn tooltip surface.");
            using (Bitmap uncheckedState = new Bitmap(tree.StateImageList.Images[0]))
            {
                Color stateCorner = uncheckedState.GetPixel(0, 0);
                Assert(IsCloserTo(stateCorner, ThemeManager.CurrentPalette.TreeSurface, Color.White),
                    "The folder-list state image still contains a white native/transparent outline artifact.");
            }
            Assert(tree.Nodes.Count == 4, "Media Filter did not initially show the in-memory Artist model.");
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

            foreach (CheckBox filter in filters) filter.Checked = true;
            Assert(tree.Nodes.Count == 4, "Clearing Media Filter criteria did not restore all rows.");
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
                && filterAlbums.Where(item => item.EligibleByDefault).All(item => item.Selected),
                "Select [ALL] did not become the sole selected mode or select all normally eligible Albums.");
            selectNone.Checked = true;
            Assert(selectNone.Checked && !selectAll.Checked && !selectFiltered.Checked && filterAlbums.All(item => !item.Selected),
                "Select [NONE] did not become the sole selected mode or clear all Album selections.");
            filterAlbums.ForEach(item => item.Selected = false);
            filterAlbums.Single(item => item.Title == "Fresh Album").Selected = true;
            filterAlbums.Single(item => item.Title == "Red Album").Selected = true;
            MethodInfo autoSelection = typeof(MainForm).GetMethod("SelectedAlbumsForAutoLaunch", BindingFlags.Static | BindingFlags.NonPublic);
            List<AlbumInfo> autoQueue = (List<AlbumInfo>)autoSelection.Invoke(null, new object[] { filterAlbums });
            Assert(autoQueue.Select(item => item.Title).OrderBy(value => value).SequenceEqual(new[] { "Fresh Album", "Red Album" })
                && autoQueue.Count == 2,
                "AUTO LAUNCH expanded the checked Album set to visible or full-library rows.");

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
                { "action", "Installed" }, { "destination", "cover.jpg" }
            } });
            List<AlbumInfo> nextQueue = (List<AlbumInfo>)autoSelection.Invoke(null, new object[] { consecutiveLaunchAlbums });
            Assert(!firstLaunch.Selected && nextArtist.Selected && nextQueue.Count == 1 && Object.ReferenceEquals(nextQueue[0], nextArtist),
                "A completed launch album survived an equivalent trailing-separator path and led the next artist's queue.");

            typeof(MainForm).GetField("activeLaunchAlbum", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, nextArtist);
            typeof(MainForm).GetMethod("ConsumeLaunchAlbumSelection", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { nextArtist });
            nextQueue = (List<AlbumInfo>)autoSelection.Invoke(null, new object[] { consecutiveLaunchAlbums });
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

            string pngPath = Path.Combine(ConfigStore.AppRoot, "splined-app-icon.png");
            string icoPath = Path.Combine(ConfigStore.AppRoot, "app.ico");
            Assert(File.Exists(pngPath) && File.Exists(icoPath), "The multicolor SPLINED S icon assets are missing.");
            using (Bitmap icon = new Bitmap(pngPath))
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
            using (BinaryReader reader = new BinaryReader(File.OpenRead(icoPath)))
            {
                Assert(reader.ReadUInt16() == 0 && reader.ReadUInt16() == 1, "The Windows application icon resource has an invalid header.");
                int count = reader.ReadUInt16();
                HashSet<int> sizes = new HashSet<int>();
                for (int index = 0; index < count; index++)
                {
                    int width = reader.ReadByte();
                    reader.ReadByte();
                    reader.ReadBytes(14);
                    sizes.Add(width == 0 ? 256 : width);
                }
                Assert(new[] { 16, 24, 32, 48, 64, 128, 256 }.All(sizes.Contains),
                    "The Windows icon resource does not contain every required application-icon resolution.");
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
                Assert(IsCloserTo(checkedImage.GetPixel(8, 12), dark.CheckOnBackground, dark.CheckOffBackground)
                    && IsCloserTo(uncheckedImage.GetPixel(8, 12), dark.CheckOffBackground, dark.CheckOnBackground)
                    && dark.CheckGlyph.R > 220 && dark.CheckGlyph.G > 170 && dark.CheckGlyph.B < 120,
                    "Checkbox state surfaces are not dark green/red with a yellow checked glyph.");
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
