using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Net;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal enum SelectionMode
    {
        None,
        Select,
        Filtered,
        All
    }

    internal enum ActivityTone
    {
        Normal,
        Heading,
        Accent,
        Success,
        Warning,
        Error,
        Muted,
        Orange,
        Purple,
        Yellow
    }

    internal sealed class AlbumRunStatistics
    {
        public string Artist;
        public string Album;
        public string AlbumPath;
        public DateTime StartedUtc;
        public DateTime FinishedUtc;
        public int EvaluatedCandidates;
        public int VisibleCandidates;
        public int HiddenCandidates;
        public int ProviderDiagnostics;
        public readonly List<string> ProviderDiagnosticMessages = new List<string>();
        public bool Fallback;
        public bool MusicBrainzResolved;
        public bool ReviewRequired;
        public string Action = "Incomplete";
        public string Destination = "";
        public string SelectedSource = "";
        public string SourceResolution = "";
        public string FinalResolution = "";
        public string UpscaleBackend = "none";
        public int PicturePercent;
        public int BrightnessPercent;
        public int ContrastPercent;
        public int ExposurePercent;
        public int SharpenPercent;
        public int SoftnessPercent;
        public int GammaPercent;
        public int ColorTemperature;
        public bool AdaptiveDefaults;
        public bool QualityEligible = true;
        public bool Resized;
        public bool Converted;

        public TimeSpan Duration
        {
            get
            {
                DateTime end = FinishedUtc == DateTime.MinValue ? DateTime.UtcNow : FinishedUtc;
                return end > StartedUtc ? end - StartedUtc : TimeSpan.Zero;
            }
        }
    }

    internal enum MediaStatusFilter
    {
        White,
        Orange,
        Red,
        Purple,
        Green,
        Blue,
        Incomplete
    }

    internal sealed class MainForm : FluentForm
    {
        private struct ToneCorrection
        {
            public readonly float Picture;
            public readonly float Brightness;
            public readonly float Contrast;
            public readonly float Exposure;
            public readonly float Sharpen;
            public readonly float Softness;
            public readonly float Gamma;
            public readonly float Temperature;
            public int PicturePercent { get { return (int)Math.Round(Picture * 100); } }
            public int BrightnessPercent { get { return (int)Math.Round(Brightness * 100); } }
            public int ContrastPercent { get { return (int)Math.Round(Contrast * 100); } }
            public int ExposurePercent { get { return (int)Math.Round(Exposure * 100); } }
            public int SharpenPercent { get { return (int)Math.Round(Sharpen * 100); } }
            public int SoftnessPercent { get { return (int)Math.Round(Softness * 100); } }
            public int GammaPercent { get { return (int)Math.Round(Gamma * 100); } }
            public int TemperatureValue { get { return (int)Math.Round(Temperature * 100); } }

            public ToneCorrection(float brightness, float contrast, float exposure = 0,
                float sharpen = 0, float temperature = 0, float picture = 0,
                float softness = 0, float gamma = 0)
            {
                Picture = picture;
                Brightness = brightness;
                Contrast = contrast;
                Exposure = exposure;
                Sharpen = sharpen;
                Softness = softness;
                Gamma = gamma;
                Temperature = temperature;
            }
        }

        private const string EventPrefix = "@@SPLINED_GUI@@";
        private static readonly Color CandidateMagenta = Color.FromArgb(242, 72, 171);
        private readonly JavaScriptSerializer json = new JavaScriptSerializer();
        private ConfigState state;
        private UiState uiState;
        private List<AlbumInfo> albums = new List<AlbumInfo>();
        private SelectionMode selectionMode = SelectionMode.Select;
        private bool suppressTreeEvents;
        private bool running;
        private bool stopRequested;
        private bool loadingLibrary;
        private int reloadVersion;
        private Process currentProcess;
        private TextBox artistFilter;
        private TextBox albumFilter;
        private Control mediaFilterPanel;
        private TableLayoutPanel libraryLayout;
        private Button mediaFilterToggle;
        private readonly Dictionary<MediaStatusFilter, CheckBox> mediaStatusFilters = new Dictionary<MediaStatusFilter, CheckBox>();
        private readonly Dictionary<MediaStatusFilter, Label> mediaStatusBullets = new Dictionary<MediaStatusFilter, Label>();
        private readonly Dictionary<MediaStatusFilter, Label> mediaStatusDescriptions = new Dictionary<MediaStatusFilter, Label>();
        private CheckBox filteredScanRead;
        private CheckBox filteredScanWrite;
        private CheckBox selectModeAll;
        private CheckBox selectModeNone;
        private CheckBox selectModeFiltered;
        private GroupBox selectModeGroup;
        private CheckBox autoScanAll;
        private CheckBox autoScanSelected;
        private bool autoScanEnabled;
        private string autoScanScope = "selected";
        private bool updatingFilteredControls;
        private bool initialSelectionRestored;
        private string activeRunMode;
        private SplitContainer mainSplit;
        private SplitContainer rightSplit;
        private TreeView tree;
        private ToolTip treeToolTip;
        private Button launch;
        private ToolStripMenuItem settingsMenuItem;
        private ToolStripMenuItem checkUpdateMenuItem;
        private ToolStripMenuItem showArtworkMenuItem;
        private ToolStripMenuItem showMediaSelectorMenuItem;
        private Button mediaSelectorVisibilityToggle;
        private RichTextBox activity;
        private TableLayoutPanel activityWorkspace;
        private Panel activityContentHost;
        private TableLayoutPanel activityColumn;
        private Label scanActivityTitle;
        private MusicBrainzMatchesPanel musicBrainzMatchesPanel;
        private Control artworkPreviewCard;
        private PictureBox artworkPreviewImage;
        private Label artworkPreviewTitle;
        private Label artworkPreviewCaption;
        private Label selectedAlbumInfo;
        private AlbumInfo displayedAlbum;
        private bool candidatePreviewActive;
        private bool adjustingArtworkLayout;
        private int musicBrainzPreviewVersion;
        private readonly SemaphoreSlim musicBrainzPreviewGate = new SemaphoreSlim(1, 1);
        private readonly Dictionary<string, byte[]> musicBrainzPreviewCache = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
        private readonly Queue<string> musicBrainzPreviewCacheOrder = new Queue<string>();
        private const long CandidateImageMemoryCacheLimit = 256L * 1024L * 1024L;
        private readonly Dictionary<string, byte[]> candidateImageMemoryCache = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
        private readonly Queue<string> candidateImageMemoryCacheOrder = new Queue<string>();
        private long candidateImageMemoryCacheBytes;
        private FlowLayoutPanel candidateCards;
        private Button useSelected;
        private Button keepLocal;
        private Button compare;
        private Button skip;
        private Button refineFallback;
        private Button retryMusicBrainz;
        private Button backToMusicBrainz;
        private FlowLayoutPanel candidateActionRow;
        private Button enableHover;
        private Button upscalePreview;
        private CheckBox upscaleAdaptiveDefaults;
        private Label upscaleCandidateDimensions;
        private readonly Dictionary<string, TrackBar> upscaleProfileSliders = new Dictionary<string, TrackBar>(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, Label> upscaleProfileValues = new Dictionary<string, Label>(StringComparer.OrdinalIgnoreCase);
        private readonly System.Windows.Forms.Timer upscalePreviewTimer = new System.Windows.Forms.Timer();
        private readonly HashSet<int> editedLocalCandidateIndexes = new HashSet<int>();
        private Label candidateContext;
        private StatusStrip statusStrip;
        private ToolStripStatusLabel statusLabel;
        private ToolStripProgressBar progress;
        private int selectedCandidateIndex = -1;
        private int recommendedCandidateIndex = -1;
        private readonly Dictionary<int, CandidateView> candidates = new Dictionary<int, CandidateView>();
        private readonly HashSet<int> compareCandidateIndexes = new HashSet<int>();
        private bool standaloneExistingCoverEdit;
        private Button candidateFilterButton;
        private TableLayoutPanel candidateLayout;
        private Control candidateFilterPanel;
        private bool candidateFilterWorkspaceActive;
        private int candidateFilterPreviousSplitterDistance = -1;
        private int candidateFilterPreviousPanel2MinSize = -1;
        private CheckBox candidateShowAll;
        private readonly List<CandidateFilterBinding> candidateFilterBindings = new List<CandidateFilterBinding>();
        private bool updatingCandidateFilters;
        private readonly HashSet<string> excludedCandidateSources = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> excludedCandidateTypes = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> excludedCandidatePolicies = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> excludedCandidateRanges = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        private readonly List<AlbumRunStatistics> runAlbumStatistics = new List<AlbumRunStatistics>();
        private AlbumRunStatistics activeAlbumStatistics;
        private AlbumInfo activeLaunchAlbum;
        private bool awaitingDecision;
        private bool fallbackMode;
        private string fallbackReason = "";
        private string fallbackArtist = "";
        private string fallbackAlbum = "";
        private bool fallbackRetryRequested;
        private bool musicBrainzRetryRequested;
        private bool musicBrainzRetryAvailable;
        private bool musicBrainzBackAvailable;
        private string fallbackRetryArtist = "";
        private string fallbackRetryAlbum = "";
        private HoverPreviewForm hoverPreview;
        private bool updateCheckRunning;
        private bool applyingLayoutPreset;
        private const int ExpandedMediaFilterHeight = 475;
        private const int CollapsedMediaFilterHeight = 38;
        private const int LibraryPanelMinimumWidth = 360;
        private const int LibraryTreeMinimumHeight = 170;
        private const int ActivityPanelMinimumWidth = 520;
        private const int ActivityPanelMinimumHeight = 230;
        private const int AlbumActivityMinimumWidth = 340;
        private const int CandidatePanelMinimumWidth = 720;
        private const int CandidatePanelMinimumHeight = 420;

        public MainForm(ConfigState state)
            : this(state, ConfigStore.LoadUi())
        {
        }

        internal MainForm(ConfigState state, UiState initialUiState)
        {
            this.state = state;
            RuntimeLog.Initialize(state);
            uiState = initialUiState ?? ConfigStore.LoadUi();
            excludedCandidateSources.UnionWith(uiState.CandidateExcludedSources ?? new List<string>());
            excludedCandidateTypes.UnionWith(uiState.CandidateExcludedTypes ?? new List<string>());
            excludedCandidatePolicies.UnionWith(uiState.CandidateExcludedPolicies ?? new List<string>());
            excludedCandidateRanges.UnionWith(uiState.CandidateExcludedRanges ?? new List<string>());
            upscalePreviewTimer.Interval = 180;
            upscalePreviewTimer.Tick += delegate
            {
                upscalePreviewTimer.Stop();
                RenderUpscalePreview();
            };
            FormClosed += delegate { upscalePreviewTimer.Stop(); upscalePreviewTimer.Dispose(); };
            ThemeManager.EnsureInitialized(uiState.Theme);
            ThemeManager.PrepareForm(this);
            Text = ReleaseInfo.WindowTitle;
            StartPosition = FormStartPosition.CenterScreen;
            Size = new Size(Math.Max(940, uiState.MainWidth), Math.Max(640, uiState.MainHeight));
            MinimumSize = new Size(940, 640);
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;
            BuildLayout();
            RestoreMainWindowState();
            RestoreMediaFilterState();
            SetCandidateFilterExpanded(uiState.CandidateFilterExpanded, false);
            SetMediaSelectorVisible(uiState.ShowMediaSelector, false);
            ApplyTheme();
            ThemeManager.PrepareForFirstShow(this, uiState.Theme);
            SetStatus("Loading library...");
            Load += delegate { RestorePanelSizes(); };
            Shown += async delegate
            {
                await ReloadLibraryAsync(false);
                if (uiState.ShowStatusOnLaunch)
                    using (StatusForm form = new StatusForm(this.state)) form.ShowDialog(this);
                await CheckForUpdateAsync(false);
            };
            FormClosing += MainFormClosing;
            FormClosed += delegate
            {
                RuntimeLog.Write("info", "windows.gui.closed");
                if (treeToolTip != null) treeToolTip.Dispose();
                DisposeArtworkPreviewImage();
                candidateImageMemoryCache.Clear();
                candidateImageMemoryCacheOrder.Clear();
                candidateImageMemoryCacheBytes = 0;
            };
        }

        private void BuildLayout()
        {
            MenuStrip menu = BuildMenu();
            MainMenuStrip = menu;
            Panel header = new Panel { Dock = DockStyle.Top, Height = 54, Padding = new Padding(ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space4), Tag = "titlebar-surface" };
            Controls.Add(header);
            TableLayoutPanel navigation = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Margin = Padding.Empty, Padding = Padding.Empty };
            navigation.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 178));
            navigation.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            SplinedWordmark wordmark = new SplinedWordmark { Dock = DockStyle.Fill, Margin = new Padding(8, 0, 0, 0) };
            navigation.Controls.Add(wordmark, 0, 0);
            menu.AutoSize = true;
            menu.Dock = DockStyle.Fill;
            menu.GripStyle = ToolStripGripStyle.Hidden;
            menu.LayoutStyle = ToolStripLayoutStyle.HorizontalStackWithOverflow;
            navigation.Controls.Add(menu, 1, 0);
            header.Controls.Add(navigation);

            statusStrip = new StatusStrip();
            statusStrip.Dock = DockStyle.Bottom;
            statusLabel = new ToolStripStatusLabel("Ready") { Spring = true, TextAlign = ContentAlignment.MiddleLeft };
            progress = new ToolStripProgressBar { Width = 180, Minimum = 0, Maximum = 100, Visible = false };
            statusStrip.Items.Add(statusLabel);
            statusStrip.Items.Add(progress);
            Controls.Add(statusStrip);

            mainSplit = new SplitContainer();
            mainSplit.Dock = DockStyle.Fill;
            mainSplit.Size = new Size(Math.Max(850, ClientSize.Width), Math.Max(500, ClientSize.Height));
            mainSplit.Orientation = uiState.LayoutStacked ? Orientation.Horizontal : Orientation.Vertical;
            mainSplit.SplitterWidth = 7;
            mainSplit.Panel1MinSize = uiState.LayoutStacked ? 180 : 280;
            mainSplit.Panel2MinSize = uiState.LayoutStacked ? 250 : 420;
            ApplyMainSplitPadding();
            int mainExtent = uiState.LayoutStacked ? ClientSize.Height : ClientSize.Width;
            mainSplit.SplitterDistance = Math.Max(mainSplit.Panel1MinSize, Math.Min(uiState.MainSplitterDistance, Math.Max(mainSplit.Panel1MinSize, mainExtent - mainSplit.Panel2MinSize - mainSplit.SplitterWidth)));
            mainSplit.SplitterMoved += delegate
            {
                if (applyingLayoutPreset) return;
                uiState.LayoutPreset = "Custom";
                uiState.LayoutStacked = mainSplit.Orientation == Orientation.Horizontal;
            };
            Controls.Add(mainSplit);
            mainSplit.BringToFront();

            BuildLibraryPanel(mainSplit.Panel1);
            BuildProcessingPanel(mainSplit.Panel2);
        }

        private MenuStrip BuildMenu()
        {
            MenuStrip menu = new FluentMenuStrip();
            ToolStripMenuItem file = new ToolStripMenuItem("File");
            settingsMenuItem = new ToolStripMenuItem("Settings...");
            settingsMenuItem.Click += OpenSettings;
            ToolStripMenuItem credentials = new ToolStripMenuItem("Credentials...");
            credentials.Click += delegate { using (CredentialsForm form = new CredentialsForm(state)) form.ShowDialog(this); };
            ToolStripMenuItem backup = new ToolStripMenuItem("Backup");
            ToolStripMenuItem exportBackup = new ToolStripMenuItem("Export Backup...");
            exportBackup.Click += delegate { using (BackupForm form = new BackupForm(state, uiState, false, null)) form.ShowDialog(this); };
            ToolStripMenuItem importBackup = new ToolStripMenuItem("Import Backup...");
            importBackup.Click += async delegate
            {
                using (BackupForm form = new BackupForm(state, uiState, true, null))
                {
                    if (form.ShowDialog(this) != DialogResult.OK) return;
                }
                state = ConfigStore.Load();
                uiState = ConfigStore.LoadUi();
                ApplyTheme();
                await ReloadLibraryAsync(false);
            };
            backup.DropDownItems.Add(exportBackup);
            backup.DropDownItems.Add(importBackup);
            ToolStripMenuItem reload = new ToolStripMenuItem("Refresh Library Index");
            reload.Click += async delegate { await ReloadLibraryAsync(false, true); };
            ToolStripMenuItem exit = new ToolStripMenuItem("Exit");
            exit.Click += delegate { Close(); };
            file.DropDownItems.Add(settingsMenuItem);
            file.DropDownItems.Add(credentials);
            file.DropDownItems.Add(backup);
            file.DropDownItems.Add(reload);
            file.DropDownItems.Add(new ToolStripSeparator());
            file.DropDownItems.Add(exit);

            ToolStripMenuItem view = new ToolStripMenuItem("View");
            ToolStripMenuItem appearance = new ToolStripMenuItem("Appearance");
            foreach (string choice in new[] { "System", "Dark", "Light" })
            {
                ToolStripMenuItem item = new ToolStripMenuItem(choice) { Tag = choice, Checked = choice == uiState.Theme };
                item.Click += ThemeClicked;
                appearance.DropDownItems.Add(item);
            }
            view.DropDownItems.Add(appearance);
            ToolStripMenuItem layout = new ToolStripMenuItem("Panel Layout");
            foreach (string choice in new[] { "Balanced", "Wider Select Media", "Wider Decisions", "Stacked" })
            {
                ToolStripMenuItem item = new ToolStripMenuItem(choice) { Tag = choice, Checked = String.Equals(choice, uiState.LayoutPreset, StringComparison.OrdinalIgnoreCase) };
                item.Click += LayoutPresetClicked;
                layout.DropDownItems.Add(item);
            }
            layout.DropDownItems.Add(new ToolStripSeparator());
            layout.DropDownItems.Add(new ToolStripMenuItem("Drag the panel dividers for a custom layout") { Enabled = false });
            view.DropDownItems.Add(layout);
            view.DropDownItems.Add(new ToolStripSeparator());
            showArtworkMenuItem = new ToolStripMenuItem("Show Artwork") { Checked = uiState.ShowArtwork };
            showArtworkMenuItem.Click += delegate
            {
                uiState.ShowArtwork = !uiState.ShowArtwork;
                showArtworkMenuItem.Checked = uiState.ShowArtwork;
                ApplyArtworkPanelVisibility();
                SaveUiState();
            };
            view.DropDownItems.Add(showArtworkMenuItem);
            showMediaSelectorMenuItem = new ToolStripMenuItem("Show Media Album Selector") { Checked = uiState.ShowMediaSelector };
            showMediaSelectorMenuItem.Click += delegate
            {
                SetMediaSelectorVisible(!uiState.ShowMediaSelector, true);
            };
            view.DropDownItems.Add(showMediaSelectorMenuItem);

            ToolStripMenuItem status = new ToolStripMenuItem("Status");
            status.Click += delegate { using (StatusForm form = new StatusForm(state)) form.ShowDialog(this); };
            ToolStripMenuItem help = new ToolStripMenuItem("Help");
            ToolStripMenuItem helpPage = new ToolStripMenuItem("Help");
            helpPage.Click += delegate { HelpWindows.OpenHelp(this); };
            checkUpdateMenuItem = new ToolStripMenuItem("Check for Update...");
            checkUpdateMenuItem.Click += CheckForUpdateClicked;
            ToolStripMenuItem about = new ToolStripMenuItem("About...");
            about.Click += delegate { using (AboutForm form = new AboutForm()) form.ShowDialog(this); };
            help.DropDownItems.Add(helpPage);
            help.DropDownItems.Add(checkUpdateMenuItem);
            help.DropDownItems.Add(new ToolStripSeparator());
            help.DropDownItems.Add(about);
            menu.Items.Add(file);
            menu.Items.Add(view);
            menu.Items.Add(status);
            menu.Items.Add(help);
            return menu;
        }

        private void BuildLibraryPanel(Control parent)
        {
            TableLayoutPanel frame = new FluentCardTableLayoutPanel { Name = "mediaLibrarySelectionCard", VisualRole = CardVisualRole.Panel, Dock = DockStyle.Fill, Padding = new Padding(1), ColumnCount = 1, RowCount = 1 };
            frame.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            frame.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            TableLayoutPanel layout = new TableLayoutPanel { Name = "mediaLibrarySelectionScrollCanvas" };
            libraryLayout = layout;
            layout.Dock = DockStyle.Fill;
            layout.AutoScroll = true;
            layout.Padding = new Padding(ThemeManager.Space12);
            layout.ColumnCount = 1;
            layout.RowCount = 3;
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 34));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, ExpandedMediaFilterHeight));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            UpdateLibraryScrollCanvas(true);
            frame.Controls.Add(layout, 0, 0);
            parent.Controls.Add(frame);

            layout.Controls.Add(BuildMediaLibraryTitle(), 0, 0);

            layout.Controls.Add(BuildMediaFilterPanel(), 0, 1);

            tree = new TreeView { Dock = DockStyle.Fill, CheckBoxes = true, ShowNodeToolTips = false, HideSelection = false, BorderStyle = BorderStyle.FixedSingle };
            treeToolTip = ThemeManager.CreateToolTip();
            tree.AfterCheck += TreeAfterCheck;
            tree.AfterSelect += TreeAfterSelect;
            tree.NodeMouseClick += TreeNodeMouseClick;
            tree.NodeMouseHover += TreeNodeMouseHover;
            tree.MouseLeave += delegate { treeToolTip.Hide(tree); };
            Panel treeCard = new FluentCardPanel { Name = "mediaLibraryTreeCard", Dock = DockStyle.Fill, Padding = new Padding(1), VisualRole = CardVisualRole.Nested };
            treeCard.Controls.Add(tree);
            layout.Controls.Add(treeCard, 0, 2);
        }

        private static Control BuildPanelTitle(string text, string tooltip)
        {
            FlowLayoutPanel row = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                Margin = new Padding(0),
                Padding = new Padding(0)
            };
            row.Controls.Add(new Label
            {
                Text = text,
                AutoSize = true,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle),
                Margin = new Padding(0, 4, 6, 0)
            });
            row.Controls.Add(new InfoButton(tooltip) { Margin = new Padding(0, 1, 0, 0) });
            return row;
        }

        private Control BuildMediaLibraryTitle()
        {
            TableLayoutPanel row = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = new Padding(0),
                Padding = new Padding(0)
            };
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 42));
            row.Controls.Add(BuildPanelTitle("Media Library Selection",
                "Select Media expands the Artist, Album, folder-status, selection-mode, and scan-mode controls. Select [ALL] replaces selection with the active Artist's unprocessed Albums, Select [NONE] clears selection, and Select [FILTERED] requires Artist or Album filter text. LAUNCH processes selected Albums only."), 0, 0);
            mediaSelectorVisibilityToggle = new FluentButton
            {
                Name = "mediaSelectorVisibilityToggle",
                Text = "🔛",
                Dock = DockStyle.Fill,
                Margin = new Padding(2, 0, 0, 2),
                AccessibleName = "Hide Media Album Selector"
            };
            ToolTip tip = ThemeManager.CreateToolTip();
            tip.SetToolTip(mediaSelectorVisibilityToggle, "Hide the Media Album Selector. Restore it from View > Show Media Album Selector.");
            mediaSelectorVisibilityToggle.Tag = tip;
            mediaSelectorVisibilityToggle.Click += delegate { SetMediaSelectorVisible(false, true); };
            row.Controls.Add(mediaSelectorVisibilityToggle, 1, 0);
            return row;
        }

        private void SetMediaSelectorVisible(bool visible, bool save)
        {
            uiState.ShowMediaSelector = visible;
            if (mainSplit != null) mainSplit.Panel1Collapsed = !visible;
            if (showMediaSelectorMenuItem != null) showMediaSelectorMenuItem.Checked = visible;
            if (save) SaveUiState();
        }

        private void RestoreMediaSelectorAfterCompletedRun(bool stopped, bool failed, int completedAlbums)
        {
            if (!stopped && !failed && completedAlbums > 0 && !uiState.ShowMediaSelector)
                SetMediaSelectorVisible(true, true);
        }

        private Control BuildCandidateFilterHeader()
        {
            TableLayoutPanel row = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = new Padding(0),
                Padding = new Padding(0)
            };
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 304));
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            FlowLayoutPanel filterHeading = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                Margin = new Padding(0),
                Padding = new Padding(0)
            };
            candidateFilterButton = new SpectrumToggleButton
            {
                Name = "candidateFilterButton",
                Text = "Artwork Filter · All  ▸",
                Width = 260,
                Height = 34,
                TextAlign = ContentAlignment.MiddleLeft,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle),
                Margin = new Padding(0, 0, 0, 2)
            };
            candidateFilterButton.Click += delegate
            {
                if (candidates.Count == 0)
                {
                    SetCandidateFilterExpanded(false, false);
                    return;
                }
                SetCandidateFilterExpanded(!CandidateFilterWorkspaceVisible, true);
            };
            filterHeading.Controls.Add(candidateFilterButton);
            filterHeading.Controls.Add(new InfoButton("Filter only the candidates shown for the current album. Source order follows Artwork Source Priority. Filtering never changes provider ranking or the underlying result set.")
            {
                Margin = new Padding(ThemeManager.Space4, 1, 0, 0)
            });
            row.Controls.Add(filterHeading, 0, 0);
            candidateActionRow = BuildCandidateActionRow();
            row.Controls.Add(candidateActionRow, 1, 0);
            return row;
        }

        private FlowLayoutPanel BuildCandidateActionRow()
        {
            FlowLayoutPanel actions = new FlowLayoutPanel
            {
                Name = "candidateActionRow",
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                AutoScroll = true,
                Padding = new Padding(ThemeManager.Space4, 1, 0, 1),
                Margin = new Padding(0)
            };
            launch = new FluentButton { Text = "LAUNCH", Width = 145, Height = 34, Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold), Enabled = false, Tag = "launch" };
            launch.Click += LaunchClicked;
            launch.EnabledChanged += delegate { ThemeManager.StyleButton(launch, uiState.Theme); };
            useSelected = new FluentButton { Text = "Use Selected", Width = 135, Height = 34, Enabled = false, Tag = "success" };
            useSelected.EnabledChanged += delegate { ThemeManager.StyleButton(useSelected, uiState.Theme); };
            keepLocal = new FluentButton { Text = "Keep Local", Width = 120, Height = 34, Enabled = false, Visible = false, Tag = "success" };
            keepLocal.Click += KeepLocalClicked;
            keepLocal.EnabledChanged += delegate { ThemeManager.StyleButton(keepLocal, uiState.Theme); };
            compare = new FluentButton { Text = "Compare", Width = 115, Height = 34, Enabled = false };
            skip = new FluentButton { Text = "Skip Album", Width = 120, Height = 34, Enabled = false };
            useSelected.Click += UseSelectedClicked;
            compare.Click += CompareClicked;
            skip.Click += SkipClicked;
            refineFallback = new FluentButton { Text = "Refine Fallback Search...", Width = 190, Height = 34, Enabled = false, Visible = false, Tag = "primary" };
            refineFallback.Click += RefineFallbackClicked;
            refineFallback.EnabledChanged += delegate { ThemeManager.StyleButton(refineFallback, uiState.Theme); };
            retryMusicBrainz = new FluentButton { Text = "MusicBrainz Matches...", Width = 185, Height = 34, Enabled = false, Visible = false, Tag = "primary" };
            retryMusicBrainz.Click += RetryMusicBrainzClicked;
            retryMusicBrainz.EnabledChanged += delegate { ThemeManager.StyleButton(retryMusicBrainz, uiState.Theme); };
            backToMusicBrainz = new FluentButton { Text = "Back to MB Matches", Width = 155, Height = 34, Enabled = false, Visible = false, Tag = "primary" };
            backToMusicBrainz.Click += BackToMusicBrainzClicked;
            enableHover = new FluentButton { Name = "enableHoverButton", Text = "Enable Hover", Width = 125, Height = 34, Enabled = false };
            enableHover.Click += delegate
            {
                uiState.HoverEnabled = !uiState.HoverEnabled;
                if (!uiState.HoverEnabled) CloseHoverPreview();
                SaveUiState();
                UpdateHoverButton();
            };
            actions.Controls.Add(launch);
            actions.Controls.Add(useSelected);
            actions.Controls.Add(keepLocal);
            actions.Controls.Add(compare);
            actions.Controls.Add(skip);
            actions.Controls.Add(refineFallback);
            actions.Controls.Add(retryMusicBrainz);
            actions.Controls.Add(backToMusicBrainz);
            actions.Controls.Add(enableHover);
            actions.Controls.Add(new InfoButton("The recommended candidate is selected first. Activate one candidate to use it, activate several to compare them, or skip the Album. Processing continues after you confirm a choice."));
            return actions;
        }

        private Control BuildCandidateFilterPanel()
        {
            Panel frame = new FluentCardPanel
            {
                Name = "candidateFilterPanel",
                Dock = DockStyle.Fill,
                AutoScroll = true,
                Padding = new Padding(ThemeManager.Space4),
                Margin = new Padding(0, 0, 0, ThemeManager.Space4),
                VisualRole = CardVisualRole.SpectrumNested
            };
            frame.Resize += delegate { ApplyRoundedCandidateFilterRegion(frame); };
            return frame;
        }

        private static void ApplyRoundedCandidateFilterRegion(Control control)
        {
            if (control == null || control.Width < 2 || control.Height < 2) return;
            using (System.Drawing.Drawing2D.GraphicsPath path = ThemeManager.RoundedPath(
                new RectangleF(0, 0, control.Width - 1, control.Height - 1), ThemeManager.CardRadius))
            {
                Region previous = control.Region;
                control.Region = new Region(path);
                if (previous != null) previous.Dispose();
            }
        }

        private void SetCandidateFilterExpanded(bool expanded, bool save)
        {
            if (expanded && candidates.Count == 0) expanded = false;
            if (save) uiState.CandidateFilterExpanded = expanded;
            ApplyCandidateFilterWorkspace(expanded);
            UpdateCandidateFilterButtonText();
            if (save) SaveUiState();
        }

        private bool CandidateFilterWorkspaceVisible
        {
            get
            {
                return candidateFilterPanel != null && activityContentHost != null
                    && candidateFilterPanel.Parent == activityContentHost && candidateFilterWorkspaceActive;
            }
        }

        private void ApplyCandidateFilterWorkspace(bool visible)
        {
            if (candidateFilterPanel == null || activityContentHost == null) return;
            if (visible)
            {
                CloseMusicBrainzMatchesWorkspace(false);
                ExpandCandidateFilterWorkspace();
                if (candidateFilterPanel.Parent != activityContentHost)
                {
                    if (candidateFilterPanel.Parent != null) candidateFilterPanel.Parent.Controls.Remove(candidateFilterPanel);
                    activityContentHost.Controls.Add(candidateFilterPanel);
                }
                candidateFilterWorkspaceActive = true;
                candidateFilterPanel.Visible = true;
                candidateFilterPanel.BringToFront();
                if (activityColumn != null) activityColumn.Visible = false;
                if (scanActivityTitle != null) scanActivityTitle.Text = "Candidate Findings | Upscale & Artwork Editing";
            }
            else
            {
                candidateFilterWorkspaceActive = false;
                candidateFilterPanel.Visible = false;
                RestoreCandidateFilterWorkspaceHeight();
                if (musicBrainzMatchesPanel == null && activityColumn != null) activityColumn.Visible = true;
                if (musicBrainzMatchesPanel == null && scanActivityTitle != null) scanActivityTitle.Text = "Scan Activity and Decisions";
            }
            ApplyArtworkPanelVisibility();
            UpdateArtworkSquareLayout(false);
            if (visible && IsHandleCreated)
                BeginInvoke((MethodInvoker)delegate
                {
                    ExpandCandidateFilterWorkspace();
                    UpdateArtworkSquareLayout(false);
                });
        }

        private void ExpandCandidateFilterWorkspace()
        {
            if (rightSplit == null || rightSplit.Height <= 0) return;
            if (candidateFilterPreviousSplitterDistance < 0)
                candidateFilterPreviousSplitterDistance = rightSplit.SplitterDistance;
            if (candidateFilterPreviousPanel2MinSize < 0)
                candidateFilterPreviousPanel2MinSize = rightSplit.Panel2MinSize;
            rightSplit.Panel2MinSize = Math.Min(rightSplit.Panel2MinSize, 120);
            int maximum = rightSplit.Height - rightSplit.Panel2MinSize - rightSplit.SplitterWidth;
            if (maximum < rightSplit.Panel1MinSize) return;
            int desired = Math.Min(maximum, Math.Max(rightSplit.Panel1MinSize, 680));
            if (rightSplit.SplitterDistance < desired) rightSplit.SplitterDistance = desired;
        }

        private void RestoreCandidateFilterWorkspaceHeight()
        {
            if (rightSplit == null || candidateFilterPreviousSplitterDistance < 0) return;
            int restoredMinimum = candidateFilterPreviousPanel2MinSize >= 0
                ? candidateFilterPreviousPanel2MinSize
                : rightSplit.Panel2MinSize;
            int maximum = rightSplit.Height - restoredMinimum - rightSplit.SplitterWidth;
            if (maximum >= rightSplit.Panel1MinSize)
                rightSplit.SplitterDistance = Math.Max(rightSplit.Panel1MinSize,
                    Math.Min(candidateFilterPreviousSplitterDistance, maximum));
            rightSplit.Panel2MinSize = restoredMinimum;
            candidateFilterPreviousSplitterDistance = -1;
            candidateFilterPreviousPanel2MinSize = -1;
        }

        private Control BuildMediaFilterPanel()
        {
            TableLayoutPanel outer = new TableLayoutPanel { Name = "mediaFilterContainer", Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Margin = new Padding(0), Padding = new Padding(0) };
            outer.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            outer.RowStyles.Add(new RowStyle(SizeType.Absolute, CollapsedMediaFilterHeight));
            outer.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            mediaFilterToggle = new SpectrumToggleButton
            {
                Name = "mediaFilterToggle",
                Text = "Select Media  ▾",
                Dock = DockStyle.Fill,
                Height = CollapsedMediaFilterHeight,
                TextAlign = ContentAlignment.MiddleLeft,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle),
                Margin = new Padding(0, 0, 0, 2)
            };
            mediaFilterToggle.Click += delegate { SetMediaFilterExpanded(mediaFilterPanel == null || !mediaFilterPanel.Visible); };
            mediaFilterPanel = new FluentCardPanel { Name = "mediaFilterPanel", Dock = DockStyle.Fill, Padding = new Padding(9), VisualRole = CardVisualRole.SpectrumNested };
            TableLayoutPanel layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3 };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));

            artistFilter = AddMediaTextFilter(layout, 0, "Artist Filter", "mediaArtistFilter");
            albumFilter = AddMediaTextFilter(layout, 1, "Album Filter", "mediaAlbumFilter");
            artistFilter.TextChanged += MediaFilterCriteriaChanged;
            albumFilter.TextChanged += MediaFilterCriteriaChanged;

            TableLayoutPanel columns = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Padding = new Padding(0, 4, 0, 0) };
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            columns.RowStyles.Add(new RowStyle(SizeType.Absolute, 230));
            columns.RowStyles.Add(new RowStyle(SizeType.Percent, 100));

            GroupBox statuses = new FluentGroupBox { Name = "mediaStatusFilters", Text = "Folder status", Dock = DockStyle.Fill, Padding = new Padding(7) };
            TableLayoutPanel statusRows = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 4 };
            statusRows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            statusRows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            for (int row = 0; row < 4; row++) statusRows.RowStyles.Add(new RowStyle(SizeType.Percent, 25));
            AddMediaStatusFilter(statusRows, 0, 0, MediaStatusFilter.White, "Unprocessed");
            AddMediaStatusFilter(statusRows, 1, 0, MediaStatusFilter.Purple, "Partial / Error");
            AddMediaStatusFilter(statusRows, 2, 0, MediaStatusFilter.Blue, "Artist Bypass");
            AddMediaStatusFilter(statusRows, 3, 0, MediaStatusFilter.Green, "Artist Complete");
            AddMediaStatusFilter(statusRows, 0, 1, MediaStatusFilter.Orange, "Processed");
            AddMediaStatusFilter(statusRows, 1, 1, MediaStatusFilter.Incomplete, "Incomplete");
            AddMediaStatusFilter(statusRows, 2, 1, MediaStatusFilter.Red, "Bypassed");
            statuses.Controls.Add(statusRows);
            columns.Controls.Add(BuildFilteredAutomationPanel(), 0, 0);
            columns.Controls.Add(statuses, 0, 1);
            layout.Controls.Add(columns, 0, 2);
            mediaFilterPanel.Controls.Add(layout);
            outer.Controls.Add(mediaFilterToggle, 0, 0);
            outer.Controls.Add(mediaFilterPanel, 0, 1);
            return outer;
        }

        private static TextBox AddMediaTextFilter(TableLayoutPanel parent, int row, string labelText, string name)
        {
            TableLayoutPanel line = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1 };
            line.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 104));
            line.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            line.Controls.Add(new Label { Text = labelText, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 0);
            TextBox box = new FluentTextBox { Name = name, Dock = DockStyle.Fill };
            line.Controls.Add(box, 1, 0);
            parent.Controls.Add(line, 0, row);
            return box;
        }

        private void AddMediaStatusFilter(TableLayoutPanel parent, int row, int column, MediaStatusFilter stateFilter, string label)
        {
            CheckBox filter = new FluentCheckBox
            {
                Name = "mediaFilter" + stateFilter,
                Text = "",
                Checked = SavedMediaStatusFilter(stateFilter),
                AutoSize = false,
                Width = 20,
                Dock = DockStyle.Left,
                Margin = new Padding(0),
                Tag = stateFilter
            };
            filter.CheckedChanged += MediaFilterCriteriaChanged;
            mediaStatusFilters[stateFilter] = filter;
            FlowLayoutPanel line = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false, Margin = new Padding(0), Padding = new Padding(0) };
            Label bullet = new Label
            {
                Text = "●",
                AutoSize = false,
                Width = 23,
                Height = 23,
                Dock = DockStyle.None,
                TextAlign = ContentAlignment.MiddleCenter,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle),
                Margin = new Padding(0)
            };
            Label description = new Label { Text = label, AutoSize = true, Margin = new Padding(1, 3, 0, 0) };
            bullet.Click += delegate { filter.Checked = !filter.Checked; };
            description.Click += delegate { filter.Checked = !filter.Checked; };
            mediaStatusBullets[stateFilter] = bullet;
            mediaStatusDescriptions[stateFilter] = description;
            line.Controls.Add(filter);
            line.Controls.Add(bullet);
            line.Controls.Add(description);
            parent.Controls.Add(line, column, row);
        }

        private Control BuildFilteredAutomationPanel()
        {
            TableLayoutPanel stack = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Padding = new Padding(0) };
            stack.RowStyles.Add(new RowStyle(SizeType.Percent, 55));
            stack.RowStyles.Add(new RowStyle(SizeType.Percent, 45));

            TableLayoutPanel top = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Margin = Padding.Empty };
            top.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            top.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));

            selectModeGroup = new FluentGroupBox { Name = "selectModeGroup", Text = "Select Mode [0]", Dock = DockStyle.Fill, Padding = new Padding(9, 8, 7, 7) };
            TableLayoutPanel selectChoices = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3, Padding = new Padding(0), Margin = new Padding(0) };
            selectChoices.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            selectChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 33.33f));
            selectChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 33.33f));
            selectChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 33.34f));
            selectModeAll = new FluentCheckBox { Name = "selectModeAll", Text = "Select [ALL]", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            selectModeNone = new FluentCheckBox { Name = "selectModeNone", Text = "Select [NONE]", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            selectModeFiltered = new FluentCheckBox { Name = "selectModeFiltered", Text = "Select [FILTERED]", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            selectModeAll.CheckedChanged += SelectModeChoiceChanged;
            selectModeNone.CheckedChanged += SelectModeChoiceChanged;
            selectModeFiltered.CheckedChanged += SelectModeChoiceChanged;
            selectChoices.Controls.Add(selectModeAll, 0, 0);
            selectChoices.Controls.Add(selectModeNone, 0, 1);
            selectChoices.Controls.Add(selectModeFiltered, 0, 2);
            selectModeGroup.Controls.Add(selectChoices);

            GroupBox automatic = new FluentGroupBox { Name = "autoScanGroup", Text = "Album Scanning", Dock = DockStyle.Fill, Padding = new Padding(9, 8, 7, 7) };
            TableLayoutPanel automaticChoices = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Margin = Padding.Empty };
            automaticChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 50));
            automaticChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 50));
            autoScanAll = new FluentCheckBox { Name = "autoScanAll", Text = "Auto Scan [All]", AutoSize = false, Dock = DockStyle.Fill, Margin = Padding.Empty };
            autoScanSelected = new FluentCheckBox { Name = "autoScanSelected", Text = "Auto Scan [Selected]", AutoSize = false, Dock = DockStyle.Fill, Margin = Padding.Empty };
            autoScanAll.CheckedChanged += AutoScanChoiceChanged;
            autoScanSelected.CheckedChanged += AutoScanChoiceChanged;
            automaticChoices.Controls.Add(autoScanAll, 0, 0);
            automaticChoices.Controls.Add(autoScanSelected, 0, 1);
            automatic.Controls.Add(automaticChoices);
            top.Controls.Add(selectModeGroup, 0, 0);
            top.Controls.Add(automatic, 1, 0);

            GroupBox scan = new FluentGroupBox { Text = "Launch Mode", Dock = DockStyle.Fill, Padding = new Padding(9, 8, 7, 7) };
            TableLayoutPanel scanChoices = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Padding = new Padding(0), Margin = new Padding(0) };
            scanChoices.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            scanChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 50));
            scanChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 50));
            filteredScanRead = new FluentCheckBox { Name = "filteredScanRead", Text = "Launch [READ] Source Results", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            filteredScanWrite = new FluentCheckBox { Name = "filteredScanWrite", Text = "Launch [LIVE WRITE] Choice Results", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            filteredScanRead.CheckedChanged += FilteredScanChoiceChanged;
            filteredScanWrite.CheckedChanged += FilteredScanChoiceChanged;
            scanChoices.Controls.Add(filteredScanRead, 0, 0);
            scanChoices.Controls.Add(filteredScanWrite, 0, 1);
            scan.Controls.Add(scanChoices);

            ToolTip help = ThemeManager.CreateToolTip();
            help.SetToolTip(selectModeGroup, "Matches Python: ALL replaces selection with the active Artist's unprocessed Albums; NONE clears selection; FILTERED requires Artist or Album text and replaces selection with matching unprocessed or processed Albums.");
            help.SetToolTip(automatic, "Auto Scan is optional unattended processing. [All] processes every unprocessed Album plus explicit selections; [Selected] processes only explicit selections. Only a policy-qualified Ideal candidate is accepted automatically. Ideal does not verify visual accuracy.");
            help.SetToolTip(scan, "Choose one Read or Live Write mode, then use the single LAUNCH button in Artwork Candidates and Preview. Internal settings are not changed and bypass/timeout authority is preserved.");
            selectModeGroup.Tag = help;
            automatic.Tag = help;
            scan.Tag = help;
            stack.Controls.Add(top, 0, 0);
            stack.Controls.Add(scan, 0, 1);
            return stack;
        }

        private void SetMediaFilterExpanded(bool expanded)
        {
            if (mediaFilterPanel == null || libraryLayout == null) return;
            mediaFilterPanel.Visible = expanded;
            mediaFilterToggle.Text = expanded ? "Select Media  ▾" : "Select Media  ▸";
            libraryLayout.RowStyles[1].Height = expanded ? ExpandedMediaFilterHeight : CollapsedMediaFilterHeight;
            UpdateLibraryScrollCanvas(expanded);
            libraryLayout.PerformLayout();
            uiState.MediaFilterExpanded = expanded;
        }

        private void UpdateLibraryScrollCanvas(bool expanded)
        {
            if (libraryLayout == null) return;
            int filterHeight = expanded ? ExpandedMediaFilterHeight : CollapsedMediaFilterHeight;
            int minimumHeight = 34 + filterHeight + LibraryTreeMinimumHeight + (ThemeManager.Space12 * 2);
            libraryLayout.AutoScrollMinSize = new Size(LibraryPanelMinimumWidth, minimumHeight);
        }

        private void RestoreMainWindowState()
        {
            if (uiState.MainX >= 0 && uiState.MainY >= 0)
            {
                Rectangle saved = new Rectangle(uiState.MainX, uiState.MainY, Width, Height);
                if (Screen.AllScreens.Any(screen => screen.WorkingArea.IntersectsWith(saved)))
                {
                    StartPosition = FormStartPosition.Manual;
                    Location = saved.Location;
                }
            }
            if (uiState.MainMaximized) WindowState = FormWindowState.Maximized;
        }

        private void RestorePanelSizes()
        {
            int mainExtent = mainSplit == null ? 0 : mainSplit.Orientation == Orientation.Vertical ? mainSplit.Width : mainSplit.Height;
            if (mainSplit != null && mainExtent > mainSplit.Panel1MinSize + mainSplit.Panel2MinSize + mainSplit.SplitterWidth)
            {
                int maximum = mainExtent - mainSplit.Panel2MinSize - mainSplit.SplitterWidth;
                mainSplit.SplitterDistance = Math.Max(mainSplit.Panel1MinSize, Math.Min(uiState.MainSplitterDistance, maximum));
            }
            if (rightSplit != null && rightSplit.Height > rightSplit.Panel1MinSize + rightSplit.Panel2MinSize + rightSplit.SplitterWidth)
            {
                int maximum = rightSplit.Height - rightSplit.Panel2MinSize - rightSplit.SplitterWidth;
                rightSplit.SplitterDistance = Math.Max(rightSplit.Panel1MinSize, Math.Min(uiState.RightSplitterDistance, maximum));
            }
        }

        private void RestoreMediaFilterState()
        {
            updatingFilteredControls = true;
            try
            {
                artistFilter.Text = uiState.MediaArtistFilter ?? "";
                albumFilter.Text = uiState.MediaAlbumFilter ?? "";
                // Config v5 owns the launch mode at application start. A stale
                // interface-state choice must not silently turn a saved WRITE
                // configuration into a READ run on the next launch.
                string launchMode = state.Mode;
                uiState.FilteredScanMode = launchMode;
                filteredScanRead.Checked = String.Equals(launchMode, "read", StringComparison.OrdinalIgnoreCase);
                filteredScanWrite.Checked = String.Equals(launchMode, "write", StringComparison.OrdinalIgnoreCase);
                filteredScanRead.Enabled = !filteredScanWrite.Checked;
                filteredScanWrite.Enabled = !filteredScanRead.Checked;
                autoScanScope = String.Equals(uiState.AutoScanScope, "all", StringComparison.OrdinalIgnoreCase) ? "all" : "selected";
                autoScanEnabled = uiState.AutoScanEnabled;
                autoScanAll.Checked = autoScanEnabled && autoScanScope == "all";
                autoScanSelected.Checked = autoScanEnabled && autoScanScope == "selected";
            }
            finally { updatingFilteredControls = false; }
            SetMediaFilterExpanded(uiState.MediaFilterExpanded);
            UpdateSelectionControlsCore(false);
        }

        private bool SavedMediaStatusFilter(MediaStatusFilter filter)
        {
            switch (filter)
            {
                case MediaStatusFilter.Orange: return uiState.MediaShowOrange;
                case MediaStatusFilter.Red: return uiState.MediaShowRed;
                case MediaStatusFilter.Purple: return uiState.MediaShowPurple;
                case MediaStatusFilter.Green: return uiState.MediaShowGreen;
                case MediaStatusFilter.Blue: return uiState.MediaShowBlue;
                case MediaStatusFilter.Incomplete: return uiState.MediaShowIncomplete;
                default: return uiState.MediaShowWhite;
            }
        }

        private void AutoScanChoiceChanged(object sender, EventArgs e)
        {
            CheckBox selected = sender as CheckBox;
            if (updatingFilteredControls || selected == null) return;
            updatingFilteredControls = true;
            try
            {
                if (selected.Checked)
                {
                    autoScanAll.Checked = Object.ReferenceEquals(selected, autoScanAll);
                    autoScanSelected.Checked = Object.ReferenceEquals(selected, autoScanSelected);
                    autoScanScope = autoScanAll.Checked ? "all" : "selected";
                    autoScanEnabled = true;
                }
                else if (!autoScanAll.Checked && !autoScanSelected.Checked)
                {
                    autoScanEnabled = false;
                }
                uiState.AutoScanEnabled = autoScanEnabled;
                uiState.AutoScanScope = autoScanScope;
            }
            finally { updatingFilteredControls = false; }
            UpdateSelectionControlsCore(true);
        }

        private void MediaFilterCriteriaChanged(object sender, EventArgs e)
        {
            BuildTree();
            UpdateSelectionControlsCore(false);
        }

        private void FilteredScanChoiceChanged(object sender, EventArgs e)
        {
            if (updatingFilteredControls) return;
            updatingFilteredControls = true;
            try
            {
                if (Object.ReferenceEquals(sender, filteredScanRead) && filteredScanRead.Checked)
                    filteredScanWrite.Checked = false;
                if (Object.ReferenceEquals(sender, filteredScanWrite) && filteredScanWrite.Checked)
                    filteredScanRead.Checked = false;
                filteredScanRead.Enabled = !running && (!filteredScanWrite.Checked || filteredScanRead.Checked);
                filteredScanWrite.Enabled = !running && (!filteredScanRead.Checked || filteredScanWrite.Checked);
                uiState.FilteredScanMode = filteredScanRead.Checked ? "read" : filteredScanWrite.Checked ? "write" : "";
            }
            finally { updatingFilteredControls = false; }
            ThemeManager.Apply(mediaFilterPanel, uiState.Theme);
            UpdateMediaFilterColors();
            UpdateSelectionControlsCore(false);
        }

        private void SelectModeChoiceChanged(object sender, EventArgs e)
        {
            CheckBox selected = sender as CheckBox;
            if (updatingFilteredControls || selected == null || running) return;
            if (!selected.Checked)
            {
                bool clearedActiveMode = (Object.ReferenceEquals(selected, selectModeAll) && selectionMode == SelectionMode.All)
                    || (Object.ReferenceEquals(selected, selectModeNone) && selectionMode == SelectionMode.None)
                    || (Object.ReferenceEquals(selected, selectModeFiltered) && selectionMode == SelectionMode.Filtered);
                if (clearedActiveMode)
                {
                    selectionMode = SelectionMode.Select;
                    UpdateSelectionControls();
                }
                return;
            }

            updatingFilteredControls = true;
            try
            {
                selectModeAll.Checked = Object.ReferenceEquals(selected, selectModeAll);
                selectModeNone.Checked = Object.ReferenceEquals(selected, selectModeNone);
                selectModeFiltered.Checked = Object.ReferenceEquals(selected, selectModeFiltered);
            }
            finally { updatingFilteredControls = false; }

            if (Object.ReferenceEquals(selected, selectModeAll))
            {
                ArtistNodeInfo activeArtist = CurrentArtistNode();
                if (activeArtist == null)
                {
                    selectionMode = SelectionMode.Select;
                    UpdateSelectModeChecks();
                    SetStatus("Select [ALL] requires an Artist.");
                    return;
                }
                selectionMode = SelectionMode.All;
                foreach (AlbumInfo album in albums)
                {
                    album.Selected = activeArtist.AllAlbums.Contains(album) && album.State == AlbumState.New;
                    album.BypassOverride = false;
                }
                BuildTree();
                UpdateSelectionControls();
            }
            else if (Object.ReferenceEquals(selected, selectModeNone))
            {
                selectionMode = SelectionMode.None;
                foreach (AlbumInfo album in albums) { album.Selected = false; album.BypassOverride = false; }
                ClearRightWorkspace();
                BuildTree();
                UpdateSelectionControls();
            }
            else
            {
                selectionMode = SelectionMode.Filtered;
                SelectVisibleFilteredAlbums();
            }
        }

        private int SelectVisibleFilteredAlbums()
        {
            if (running) return 0;
            string artistText = (artistFilter == null ? "" : artistFilter.Text).Trim();
            string albumText = (albumFilter == null ? "" : albumFilter.Text).Trim();
            if (artistText.Length == 0 && albumText.Length == 0)
            {
                SetStatus("Select [FILTERED] requires Artist or Album filter text.");
                return albums.Count(album => album.Selected);
            }
            foreach (AlbumInfo album in albums)
            {
                bool textMatch = (artistText.Length == 0 || album.Artist.IndexOf(artistText, StringComparison.OrdinalIgnoreCase) >= 0)
                    && (albumText.Length == 0 || album.Title.IndexOf(albumText, StringComparison.OrdinalIgnoreCase) >= 0);
                album.Selected = textMatch && (album.State == AlbumState.New || album.State == AlbumState.Processed);
                album.BypassOverride = false;
            }
            selectionMode = SelectionMode.Filtered;
            BuildTree();
            UpdateSelectionControls();
            return albums.Count(album => album.Selected);
        }

        private ArtistNodeInfo CurrentArtistNode()
        {
            TreeNode node = tree == null ? null : tree.SelectedNode;
            if (node != null && node.Tag is AlbumInfo) node = node.Parent;
            ArtistNodeInfo active = node == null ? null : node.Tag as ArtistNodeInfo;
            if (active != null) return active;
            return tree == null || tree.Nodes.Count == 0 ? null : tree.Nodes[0].Tag as ArtistNodeInfo;
        }

        private void UpdateMediaFilterColors()
        {
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            if (filteredScanRead != null)
                filteredScanRead.ForeColor = palette.Success;
            if (filteredScanWrite != null)
                filteredScanWrite.ForeColor = palette.Error;
            foreach (KeyValuePair<MediaStatusFilter, Label> pair in mediaStatusBullets)
            {
                Color color;
                switch (pair.Key)
                {
                    case MediaStatusFilter.Orange: color = palette.StatusOrange; break;
                    case MediaStatusFilter.Red: color = palette.StatusRed; break;
                    case MediaStatusFilter.Purple: color = palette.StatusPurple; break;
                    case MediaStatusFilter.Green: color = palette.StatusGreen; break;
                    case MediaStatusFilter.Blue: color = palette.StatusBlue; break;
                    case MediaStatusFilter.Incomplete: color = palette.StatusBlue; break;
                    default: color = palette.StatusWhite; break;
                }
                pair.Value.ForeColor = color;
                Label description;
                if (mediaStatusDescriptions.TryGetValue(pair.Key, out description))
                    description.ForeColor = color;
            }
            UpdateSelectionControlsCore(false);
        }

        private void UpdateMediaFilterCounts()
        {
            Dictionary<MediaStatusFilter, int> counts = Enum.GetValues(typeof(MediaStatusFilter))
                .Cast<MediaStatusFilter>().ToDictionary(value => value, value => 0);
            foreach (AlbumInfo album in albums)
                counts[AlbumFilterCategory(album.State)]++;
            foreach (IGrouping<string, AlbumInfo> group in albums.GroupBy(album => album.Artist, StringComparer.OrdinalIgnoreCase))
            {
                MediaStatusFilter category = ArtistFilterCategory(ArtistStateResolver.Aggregate(group.ToList()));
                if (category == MediaStatusFilter.Green || category == MediaStatusFilter.Blue || category == MediaStatusFilter.Purple)
                    counts[category]++;
            }
            foreach (KeyValuePair<MediaStatusFilter, Label> pair in mediaStatusDescriptions)
                pair.Value.Text = MediaStatusLabel(pair.Key) + "  [" + counts[pair.Key] + "]";
            if (selectModeGroup != null)
                selectModeGroup.Text = "Select Mode [" + albums.Count(album => album.Selected) + "]";
        }

        private static string MediaStatusLabel(MediaStatusFilter filter)
        {
            switch (filter)
            {
                case MediaStatusFilter.Orange: return "Processed";
                case MediaStatusFilter.Red: return "Bypassed";
                case MediaStatusFilter.Purple: return "Partial / Error";
                case MediaStatusFilter.Green: return "Artist Complete";
                case MediaStatusFilter.Blue: return "Artist Bypass";
                case MediaStatusFilter.Incomplete: return "Incomplete";
                default: return "Unprocessed";
            }
        }

        private void BuildProcessingPanel(Control parent)
        {
            rightSplit = new SplitContainer();
            rightSplit.Dock = DockStyle.Fill;
            rightSplit.Size = new Size(Math.Max(500, parent.ClientSize.Width), Math.Max(500, parent.ClientSize.Height));
            rightSplit.Orientation = Orientation.Horizontal;
            rightSplit.SplitterWidth = 7;
            rightSplit.Panel1MinSize = 150;
            rightSplit.Panel2MinSize = 220;
            rightSplit.Panel1.Padding = new Padding(0, 0, 0, ThemeManager.Space4);
            rightSplit.Panel2.Padding = new Padding(0, ThemeManager.Space4, 0, 0);
            rightSplit.SplitterDistance = Math.Max(rightSplit.Panel1MinSize, Math.Min(uiState.RightSplitterDistance, Math.Max(rightSplit.Panel1MinSize, rightSplit.Height - rightSplit.Panel2MinSize - rightSplit.SplitterWidth)));
            rightSplit.SplitterMoved += delegate { UpdateArtworkSquareLayout(true); };
            rightSplit.Resize += delegate { UpdateArtworkSquareLayout(false); };
            parent.Controls.Add(rightSplit);

            TableLayoutPanel upper = new FluentCardTableLayoutPanel { Name = "scanActivityCard", Dock = DockStyle.Fill, AutoScroll = true, AutoScrollMinSize = new Size(ActivityPanelMinimumWidth, ActivityPanelMinimumHeight), Padding = new Padding(ThemeManager.Space12), RowCount = 2, ColumnCount = 1, VisualRole = CardVisualRole.Panel };
            upper.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
            upper.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            rightSplit.Panel1.Controls.Add(upper);
            Control activityTitleRow = BuildPanelTitle("Scan Activity and Decisions",
                "Live discovery, validation, provider results, and write/read outcomes appear here. MusicBrainz Matches replace the Activity surface while a release decision is active; artwork URLs preview in the shared panel at right.");
            scanActivityTitle = activityTitleRow.Controls.OfType<Label>().First();
            scanActivityTitle.Name = "scanActivityTitle";
            upper.Controls.Add(activityTitleRow, 0, 0);
            activityWorkspace = new TableLayoutPanel { Name = "activityWorkspace", Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Margin = Padding.Empty, Padding = Padding.Empty };
            activityWorkspace.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 64));
            activityWorkspace.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 36));
            activityWorkspace.Resize += delegate { UpdateArtworkSquareLayout(false); };
            activityContentHost = new Panel { Name = "activityContentHost", Dock = DockStyle.Fill, Margin = new Padding(0, 0, ThemeManager.Space4, 0), Padding = Padding.Empty };
            activityColumn = new TableLayoutPanel { Name = "activityColumn", Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Margin = Padding.Empty, Padding = Padding.Empty };
            activityColumn.RowStyles.Add(new RowStyle(SizeType.Absolute, 96));
            activityColumn.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            Panel albumInfoCard = new FluentCardPanel { Name = "selectedAlbumInfoCard", Dock = DockStyle.Fill, Padding = new Padding(ThemeManager.Space8), Margin = new Padding(0, 0, 0, ThemeManager.Space4), VisualRole = CardVisualRole.SpectrumNested };
            selectedAlbumInfo = new Label { Name = "selectedAlbumInfo", Dock = DockStyle.Fill, AutoEllipsis = true, Text = "", TextAlign = ContentAlignment.MiddleLeft };
            albumInfoCard.Controls.Add(selectedAlbumInfo);
            activityColumn.Controls.Add(albumInfoCard, 0, 0);
            Panel activityCard = new FluentCardPanel { Name = "activityLogCard", Dock = DockStyle.Fill, Padding = new Padding(2), VisualRole = CardVisualRole.Log };
            activity = new RichTextBox { Dock = DockStyle.Fill, ReadOnly = true, BorderStyle = BorderStyle.None, DetectUrls = false, Font = new Font("Cascadia Mono", 9f) };
            activityCard.Controls.Add(activity);
            activityColumn.Controls.Add(activityCard, 0, 1);
            activityContentHost.Controls.Add(activityColumn);
            activityWorkspace.Controls.Add(activityContentHost, 0, 0);

            TableLayoutPanel artwork = new FluentCardTableLayoutPanel { Name = "selectedAlbumArtworkCard", Dock = DockStyle.Fill, Padding = new Padding(ThemeManager.Space8), RowCount = 3, ColumnCount = 1, Margin = new Padding(ThemeManager.Space4, 0, 0, 0), VisualRole = CardVisualRole.SpectrumNested };
            artwork.RowStyles.Add(new RowStyle(SizeType.Absolute, 28));
            artwork.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            artwork.RowStyles.Add(new RowStyle(SizeType.Absolute, 28));
            artworkPreviewTitle = new Label { Text = "Selected Album Artwork", Dock = DockStyle.Fill, Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle), TextAlign = ContentAlignment.MiddleLeft };
            artwork.Controls.Add(artworkPreviewTitle, 0, 0);
            artworkPreviewImage = new PictureBox { Name = "artworkPreviewImage", Dock = DockStyle.Fill, SizeMode = PictureBoxSizeMode.Zoom, BorderStyle = BorderStyle.None };
            artwork.Controls.Add(artworkPreviewImage, 0, 1);
            artworkPreviewCaption = new Label { Name = "artworkPreviewCaption", Text = "No artwork selected", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleCenter, AutoEllipsis = true };
            artwork.Controls.Add(artworkPreviewCaption, 0, 2);
            artworkPreviewCard = artwork;
            activityWorkspace.Controls.Add(artwork, 1, 0);
            upper.Controls.Add(activityWorkspace, 0, 1);
            ApplyArtworkPanelVisibility();

            TableLayoutPanel lower = new FluentCardTableLayoutPanel { Name = "artworkCandidatesCard", Dock = DockStyle.Fill, AutoScroll = true, AutoScrollMinSize = new Size(CandidatePanelMinimumWidth, CandidatePanelMinimumHeight), Padding = new Padding(ThemeManager.Space12), RowCount = 3, ColumnCount = 1, VisualRole = CardVisualRole.Panel };
            candidateLayout = lower;
            lower.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            lower.RowStyles.Add(new RowStyle(SizeType.Absolute, 24));
            lower.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            rightSplit.Panel2.Controls.Add(lower);
            lower.Controls.Add(BuildCandidateFilterHeader(), 0, 0);
            candidateFilterPanel = BuildCandidateFilterPanel();
            candidateFilterPanel.Visible = false;
            candidateContext = new Label { Text = "", Dock = DockStyle.Fill, AutoEllipsis = true };
            lower.Controls.Add(candidateContext, 0, 1);
            candidateCards = new WatermarkFlowLayoutPanel { Dock = DockStyle.Fill, AutoScroll = true, WrapContents = true, FlowDirection = FlowDirection.LeftToRight, Padding = new Padding(ThemeManager.Space8) };
            candidateCards.Resize += delegate { ResizeCandidateCards(); };
            lower.Controls.Add(candidateCards, 0, 2);
        }

        private Task ReloadLibraryAsync(bool preserveSelection)
        {
            return ReloadLibraryAsync(preserveSelection, false);
        }

        private async Task ReloadLibraryAsync(bool preserveSelection, bool refreshIndex)
        {
            int version = ++reloadVersion;
            loadingLibrary = true;
            Dictionary<string, bool> previouslySelected = preserveSelection
                ? albums.ToDictionary(album => album.Path, album => album.Selected, StringComparer.OrdinalIgnoreCase)
                : new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
            HashSet<string> persistedSelection = !preserveSelection && !initialSelectionRestored
                ? new HashSet<string>(uiState.SelectedAlbumPaths ?? new List<string>(), StringComparer.OrdinalIgnoreCase)
                : new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            ConfigState snapshot = state.Clone();
            string core = FindCoreExecutable();
            SetStatus(refreshIndex ? "Refreshing SQLite Album Status index..." : "Loading Album Status from SQLite...");
            UpdateSelectionControls();
            try
            {
                string displayedPath = displayedAlbum == null ? "" : displayedAlbum.Path;
                List<AlbumInfo> loaded = await Task.Run(delegate { return LibraryInventory.Load(snapshot, core, refreshIndex); });
                if (version != reloadVersion || IsDisposed) return;
                albums = loaded;
                foreach (AlbumInfo album in albums)
                {
                    bool selected;
                    bool safeState = album.State == AlbumState.New || album.State == AlbumState.Incomplete || album.State == AlbumState.Processed;
                    album.Selected = safeState && ((preserveSelection
                        && previouslySelected.TryGetValue(album.Path, out selected)
                        && selected) || persistedSelection.Contains(album.Path));
                    album.BypassOverride = false;
                }
                initialSelectionRestored = true;
                selectionMode = SelectionMode.Select;
                BuildTree();
                displayedAlbum = String.IsNullOrWhiteSpace(displayedPath)
                    ? null
                    : albums.FirstOrDefault(album => SameAlbumPath(album.Path, displayedPath));
                RenderDisplayedAlbum();
                int selectedCount = albums.Count(album => album.Selected);
                SetStatus(albums.Count + " album folders loaded. " + (selectedCount == 0
                    ? "Select is ready; no Albums are selected."
                    : selectedCount + " saved album selection(s) restored."));
            }
            catch (Exception error)
            {
                if (version != reloadVersion || IsDisposed) return;
                albums.Clear();
                BuildTree();
                displayedAlbum = null;
                RenderDisplayedAlbum();
                SetStatus(error.Message);
            }
            finally
            {
                if (version == reloadVersion)
                {
                    loadingLibrary = false;
                    // Preserve the completed load count or exact SQLite/core
                    // error. A selection summary will replace it only after a
                    // subsequent selection action.
                    UpdateSelectionControlsCore(false);
                }
            }
        }

        private void BuildTree()
        {
            if (tree == null) return;
            suppressTreeEvents = true;
            tree.BeginUpdate();
            tree.Nodes.Clear();
            string artistText = (artistFilter == null ? "" : artistFilter.Text).Trim();
            string albumText = (albumFilter == null ? "" : albumFilter.Text).Trim();
            bool dark = ThemeManager.IsDark(uiState.Theme);
            IEnumerable<IGrouping<string, AlbumInfo>> groups = albums
                .GroupBy(album => album.Artist, StringComparer.OrdinalIgnoreCase)
                .OrderBy(group => group.Key, StringComparer.OrdinalIgnoreCase);
            foreach (IGrouping<string, AlbumInfo> group in groups)
            {
                if (artistText.Length > 0 && group.Key.IndexOf(artistText, StringComparison.OrdinalIgnoreCase) < 0) continue;
                List<AlbumInfo> allArtistAlbums = group.OrderBy(album => album.Title, StringComparer.OrdinalIgnoreCase).ToList();
                ArtistAggregateState aggregate = ArtistStateResolver.Aggregate(allArtistAlbums);
                bool artistCategoryVisible = IsMediaStatusChecked(ArtistFilterCategory(aggregate));
                bool anyAlbumCategoryVisible = new[] { MediaStatusFilter.White, MediaStatusFilter.Orange, MediaStatusFilter.Red, MediaStatusFilter.Purple, MediaStatusFilter.Incomplete }
                    .Any(IsMediaStatusChecked);
                List<AlbumInfo> textMatched = allArtistAlbums
                    .Where(album => albumText.Length == 0 || album.Title.IndexOf(albumText, StringComparison.OrdinalIgnoreCase) >= 0)
                    .ToList();
                List<AlbumInfo> visibleAlbums = textMatched
                    .Where(album => IsMediaStatusChecked(AlbumFilterCategory(album.State)))
                    .ToList();
                if (!anyAlbumCategoryVisible && artistCategoryVisible)
                    visibleAlbums = textMatched;
                if (visibleAlbums.Count == 0)
                {
                    if (!artistCategoryVisible || albumText.Length > 0) continue;
                    visibleAlbums = allArtistAlbums;
                }

                ArtistNodeInfo artistInfo = new ArtistNodeInfo(group.Key, allArtistAlbums, visibleAlbums, aggregate);
                TreeNode artistNode = new TreeNode(group.Key)
                {
                    Tag = artistInfo,
                    Checked = visibleAlbums.Any(album => album.Selected),
                    ForeColor = ArtistStateResolver.StateColor(aggregate, dark),
                    ToolTipText = ArtistStateResolver.ToolTip(aggregate)
                };
                foreach (AlbumInfo album in visibleAlbums)
                {
                    TreeNode node = new TreeNode(album.Title) { Tag = album, Checked = album.Selected, ForeColor = AlbumStatePresentation.StateColor(album.State, dark), ToolTipText = album.ToolTip };
                    artistNode.Nodes.Add(node);
                }
                tree.Nodes.Add(artistNode);
            }
            tree.EndUpdate();
            suppressTreeEvents = false;
            UpdateMediaFilterCounts();
            UpdateSelectionControlsCore(false);
        }

        private bool IsMediaStatusChecked(MediaStatusFilter filter)
        {
            CheckBox control;
            return !mediaStatusFilters.TryGetValue(filter, out control) || control.Checked;
        }

        private static MediaStatusFilter AlbumFilterCategory(AlbumState state)
        {
            if (state == AlbumState.Processed) return MediaStatusFilter.Orange;
            if (state == AlbumState.Incomplete) return MediaStatusFilter.Incomplete;
            if (state == AlbumState.Bypassed) return MediaStatusFilter.Red;
            if (state == AlbumState.TimeoutActive) return MediaStatusFilter.Purple;
            return MediaStatusFilter.White;
        }

        private static MediaStatusFilter ArtistFilterCategory(ArtistAggregateState state)
        {
            if (state == ArtistAggregateState.ContainsBypass) return MediaStatusFilter.Blue;
            if (state == ArtistAggregateState.Complete) return MediaStatusFilter.Green;
            if (state == ArtistAggregateState.Partial) return MediaStatusFilter.Purple;
            return MediaStatusFilter.White;
        }

        private void TreeAfterCheck(object sender, TreeViewEventArgs e)
        {
            if (suppressTreeEvents || running) return;
            if (selectionMode != SelectionMode.Select)
            {
                selectionMode = SelectionMode.Select;
                UpdateSelectionControls();
            }

            ArtistNodeInfo artist = e.Node.Tag as ArtistNodeInfo;
            if (artist != null)
            {
                bool includeBypassed = false;
                if (e.Node.Checked)
                {
                    int bypassCount = artist.VisibleAlbums.Count(album => album.State == AlbumState.Bypassed && !album.BypassOverride);
                    if (bypassCount > 0)
                    {
                        DialogResult allow = MessageBox.Show(this,
                            "This artist contains " + bypassCount + " bypassed album(s). Include those albums using a temporary override for this run?\r\n\r\nThe stored bypass history will not be deleted. Choosing No still selects eligible white albums.",
                            "Temporary bypass overrides", MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2);
                        includeBypassed = allow == DialogResult.Yes;
                    }
                }
                suppressTreeEvents = true;
                ArtistSelectionRules.Apply(artist.VisibleAlbums, e.Node.Checked, includeBypassed);
                foreach (TreeNode child in e.Node.Nodes)
                {
                    AlbumInfo album = child.Tag as AlbumInfo;
                    child.Checked = album != null && album.Selected;
                }
                e.Node.Checked = artist.VisibleAlbums.Any(album => album.Selected);
                suppressTreeEvents = false;
                UpdateSelectionControls();
                return;
            }

            AlbumInfo item = e.Node.Tag as AlbumInfo;
            if (item == null) return;
            if (e.Node.Checked && item.State == AlbumState.TimeoutActive)
            {
                suppressTreeEvents = true;
                e.Node.Checked = false;
                suppressTreeEvents = false;
                MessageBox.Show(this, item.ToolTip, "Album timeout active", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            if (e.Node.Checked && item.State == AlbumState.Bypassed && !item.BypassOverride)
            {
                DialogResult allow = MessageBox.Show(this,
                    "This album is marked bypassed. Override bypass for this run only?\r\n\r\nThe stored bypass history will not be deleted.",
                    "Temporary bypass override", MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2);
                if (allow != DialogResult.Yes)
                {
                    suppressTreeEvents = true; e.Node.Checked = false; suppressTreeEvents = false;
                    return;
                }
                item.BypassOverride = true;
            }
            item.Selected = e.Node.Checked;
            if (!item.Selected) item.BypassOverride = false;
            ShowSelectedAlbum(item);
            UpdateParentCheck(e.Node.Parent);
            UpdateSelectionControls();
        }

        private void TreeAfterSelect(object sender, TreeViewEventArgs e)
        {
            AlbumInfo album = e.Node == null ? null : e.Node.Tag as AlbumInfo;
            ShowSelectedAlbum(album);
        }

        private void TreeNodeMouseClick(object sender, TreeNodeMouseClickEventArgs e)
        {
            if (e.Button == MouseButtons.Left)
                ShowSelectedAlbum(e.Node == null ? null : e.Node.Tag as AlbumInfo);
            if (e.Button == MouseButtons.Right && e.Node.ToolTipText.Length > 0)
                SetStatus(e.Node.ToolTipText);
        }

        private void TreeNodeMouseHover(object sender, TreeNodeMouseHoverEventArgs e)
        {
            if (e.Node == null || String.IsNullOrWhiteSpace(e.Node.ToolTipText))
            {
                treeToolTip.Hide(tree);
                return;
            }
            Point position = tree.PointToClient(Cursor.Position);
            treeToolTip.Show(e.Node.ToolTipText, tree, position.X + 16, position.Y + 18, 12000);
        }

        private void UpdateParentCheck(TreeNode parent)
        {
            if (parent == null) return;
            suppressTreeEvents = true;
            parent.Checked = parent.Nodes.Count > 0 && parent.Nodes.Cast<TreeNode>().Any(node => ((AlbumInfo)node.Tag).Selected);
            suppressTreeEvents = false;
        }

        private bool IsNodeSelected(TreeNode node)
        {
            AlbumInfo album = node.Tag as AlbumInfo;
            if (album != null) return album.Selected;
            ArtistNodeInfo artist = node.Tag as ArtistNodeInfo;
            return artist != null && artist.VisibleAlbums.Count > 0 && artist.VisibleAlbums.Any(value => value.Selected);
        }

        private void UpdateSelectionControls()
        {
            UpdateSelectionControlsCore(true);
        }

        private void UpdateSelectionControlsCore(bool updateStatus)
        {
            int selectedCount = albums.Count(album => album.Selected);
            int count = GetLaunchAlbums().Count;
            bool waiting = running && awaitingDecision;
            bool launchModeChosen = filteredScanRead != null && filteredScanWrite != null
                && (filteredScanRead.Checked || filteredScanWrite.Checked);
            launch.Enabled = running || (!loadingLibrary && count > 0 && launchModeChosen);
            launch.Tag = waiting ? "waiting" : "launch";
            launch.Text = waiting ? "WAITING" : running ? "STOP" : (count > 0 ? "LAUNCH (" + count + ")" : "LAUNCH");
            ThemeManager.StyleButton(launch, uiState.Theme);
            if (settingsMenuItem != null)
                settingsMenuItem.Enabled = !running && !awaitingDecision && candidates.Count == 0;
            UpdateSelectModeChecks();
            bool filterActionsEnabled = !running && !loadingLibrary;
            if (artistFilter != null) artistFilter.Enabled = filterActionsEnabled;
            if (albumFilter != null) albumFilter.Enabled = filterActionsEnabled;
            foreach (CheckBox filter in mediaStatusFilters.Values) filter.Enabled = filterActionsEnabled;
            if (filteredScanRead != null)
                filteredScanRead.Enabled = filterActionsEnabled && (!filteredScanWrite.Checked || filteredScanRead.Checked);
            if (filteredScanWrite != null)
                filteredScanWrite.Enabled = filterActionsEnabled && (!filteredScanRead.Checked || filteredScanWrite.Checked);
            if (selectModeAll != null) selectModeAll.Enabled = filterActionsEnabled;
            if (selectModeNone != null) selectModeNone.Enabled = filterActionsEnabled;
            if (selectModeFiltered != null) selectModeFiltered.Enabled = filterActionsEnabled;
            if (autoScanAll != null) autoScanAll.Enabled = filterActionsEnabled;
            if (autoScanSelected != null) autoScanSelected.Enabled = filterActionsEnabled;
            if (selectModeGroup != null) selectModeGroup.Text = "Select Mode [" + selectedCount + "]";
            if (updateStatus && !running && !loadingLibrary)
                SetStatus(count == 0 ? "No albums selected. Launch is disabled."
                    : autoScanEnabled
                        ? count + " album(s) queued by Auto Scan [" + (autoScanScope == "all" ? "All" : "Selected") + "]; only Ideal candidates are accepted unattended."
                        : count + " selected album(s) queued for operator review.");
        }

        private List<AlbumInfo> GetLaunchAlbums()
        {
            if (autoScanEnabled && String.Equals(autoScanScope, "all", StringComparison.OrdinalIgnoreCase))
                return albums.Where(album => album.State == AlbumState.New || album.Selected).ToList();
            return albums.Where(album => album.Selected).ToList();
        }

        private void UpdateSelectModeChecks()
        {
            if (selectModeAll == null || selectModeNone == null || selectModeFiltered == null) return;
            bool wasUpdating = updatingFilteredControls;
            updatingFilteredControls = true;
            try
            {
                selectModeAll.Checked = selectionMode == SelectionMode.All;
                selectModeNone.Checked = selectionMode == SelectionMode.None;
                selectModeFiltered.Checked = selectionMode == SelectionMode.Filtered;
            }
            finally { updatingFilteredControls = wasUpdating; }
        }

        private void ClearRightWorkspace()
        {
            activity.Clear();
            fallbackMode = false;
            fallbackReason = "";
            SetFallbackControls(false);
            candidateContext.Text = "";
            ClearCandidates();
            progress.Visible = false;
            progress.Value = 0;
            ApplyModeActivityAppearance();
        }

        private async void LaunchClicked(object sender, EventArgs e)
        {
            if (running)
            {
                StopRun();
                return;
            }
            List<AlbumInfo> selected = GetLaunchAlbums();
            if (selected.Count == 0) return;
            if (!filteredScanRead.Checked && !filteredScanWrite.Checked)
            {
                MessageBox.Show(this, "Choose Launch [READ] or Launch [LIVE WRITE] before starting the selected albums.",
                    "Launch mode required", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            string core = FindCoreExecutable();
            if (core == null)
            {
                MessageBox.Show(this, "The live SPLINED processing core was not found beside the GUI.", "SPLINED core missing", MessageBoxButtons.OK, MessageBoxIcon.Error);
                UpdateSelectionControlsCore(false);
                return;
            }
            string runConfigText;
            activeRunMode = filteredScanRead != null && filteredScanRead.Checked
                ? "read"
                : filteredScanWrite != null && filteredScanWrite.Checked
                    ? "write"
                    : state.Mode;
            RuntimeLog.Write("info", "windows.batch.start mode=" + activeRunMode
                + " configured_mode=" + state.Mode + " albums=" + selected.Count);
            try
            {
                ConfigState runState = state.Clone();
                runState.Mode = activeRunMode;
                runConfigText = ConfigStore.ExportConfigText(runState);
            }
            catch (Exception error)
            {
                activeRunMode = null;
                MessageBox.Show(this, "Unable to prepare the in-memory runtime settings.\r\n\r\n" + error.Message,
                    "SPLINED runtime settings", MessageBoxButtons.OK, MessageBoxIcon.Error);
                UpdateSelectionControlsCore(false);
                return;
            }
            running = true;
            stopRequested = false;
            activity.Clear();
            runAlbumStatistics.Clear();
            activeAlbumStatistics = null;
            fallbackMode = false;
            fallbackReason = "";
            ClearCandidates();
            ApplyModeActivityAppearance();
            progress.Visible = true;
            progress.Value = 0;
            UpdateSelectionControls();
            AppendActivity("SPLINED LIVE PROCESSING\r\n", ActivityTone.Heading);
            if (activeRunMode.Equals("read", StringComparison.OrdinalIgnoreCase))
                AppendActivity("READ MODE — REVIEW ONLY — ALBUM ARTWORK WILL NOT BE WRITTEN\r\n", ActivityTone.Accent);
            else
                AppendActivity("WRITE MODE — SELECTED ARTWORK CAN CHANGE ALBUM FILES\r\n", ActivityTone.Warning);
            AppendActivity("Selected albums: " + selected.Count + "\r\n\r\n", ActivityTone.Muted);

            bool runFailed = false;
            for (int index = 0; index < selected.Count && !stopRequested; index++)
            {
                AlbumInfo album = selected[index];
                activeLaunchAlbum = album;
                ShowSelectedAlbum(album);
                fallbackMode = false;
                fallbackReason = "";
                fallbackArtist = "";
                fallbackAlbum = "";
                SetFallbackControls(false);
                progress.Value = (int)Math.Round(index * 100.0 / selected.Count);
                BeginAlbumRunStatistics(album);
                AppendAlbumActivityHeader(index + 1, selected.Count, album.Artist, album.Title);
                AppendActivity("  1. Reading local album and tag evidence...\r\n");
                ConfigState albumRunState = state.Clone();
                albumRunState.Mode = activeRunMode;
                runConfigText = ConfigStore.ExportConfigText(albumRunState);
                bool albumSucceeded = await RunCoreAlbum(core, album, runConfigText);
                if (!albumSucceeded && !stopRequested)
                {
                    FinishActiveAlbumStatistics("Failed");
                    activeLaunchAlbum = null;
                    ShowSelectedAlbum(null);
                    runFailed = true;
                    AppendActivity("  Processing core failed. This album remains selected; later albums were not started.\r\n", ActivityTone.Error);
                    RuntimeLog.Write("error", "batch.stopped_on_album_error album=" + album.Path);
                    break;
                }
                // A launch queue is a snapshot. Consume this album after a
                // successful or operator-stopped run so it cannot lead the next
                // independently selected artist's run. Failed and not-yet-run
                // albums remain checked and resumable.
                ConsumeLaunchAlbumSelection(album);
                FinishActiveAlbumStatistics(stopRequested ? "Stopped" : "Incomplete");
                activeLaunchAlbum = null;
                ShowSelectedAlbum(null);
                AppendActivity("\r\n");
            }

            currentProcess = null;
            activeLaunchAlbum = null;
            ShowSelectedAlbum(null);
            running = false;
            string completedRunMode = activeRunMode;
            activeRunMode = null;
            ClearCandidates();
            candidateContext.Text = "";
            progress.Value = 100;
            progress.Visible = false;
            SetStatus(stopRequested ? "Stopped by user."
                : runFailed ? "Run stopped after an error. The failed and remaining albums are still selected."
                : "Run complete. LAUNCH is ready for another selected run.");
            launch.Text = "LAUNCH";
            ApplyModeActivityAppearance();
            await ReloadLibraryAsync(true);
            RestoreMediaSelectorAfterCompletedRun(stopRequested, runFailed, runAlbumStatistics.Count);
            if (!stopRequested && !runFailed && runAlbumStatistics.Count > 0)
                ShowAlbumRunReport(completedRunMode, selected.Count);
        }

        private async Task<bool> RunCoreAlbum(string core, AlbumInfo album, string runConfigText)
        {
            string retryArtist = null;
            string retryAlbum = null;
            do
            {
                fallbackRetryRequested = false;
                musicBrainzRetryRequested = false;
                bool succeeded = await RunCoreAlbumOnce(core, album, retryArtist, retryAlbum, runConfigText);
                if (!succeeded) return false;
                if (musicBrainzRetryRequested && !stopRequested)
                {
                    retryArtist = null;
                    retryAlbum = null;
                    AppendActivity("  Retrying the current album's MusicBrainz release lookup...\r\n", ActivityTone.Accent);
                }
                else if (fallbackRetryRequested && !stopRequested)
                {
                    retryArtist = fallbackRetryArtist;
                    retryAlbum = fallbackRetryAlbum;
                    AppendActivity("  Retrying current album fallback: " + retryArtist + " - " + retryAlbum + "\r\n");
                }
            }
            while ((fallbackRetryRequested || musicBrainzRetryRequested) && !stopRequested);
            return !stopRequested;
        }

        private Task<bool> RunCoreAlbumOnce(string core, AlbumInfo album, string retryArtist, string retryAlbum, string runConfigText)
        {
            TaskCompletionSource<bool> completion = new TaskCompletionSource<bool>();
            string scanPath = ResolveExistingAlbumPath(album.Path);
            if (!String.Equals(scanPath, album.Path, StringComparison.Ordinal))
                RuntimeLog.Write("debug", "album.path.translated indexed=" + album.Path + " physical=" + scanPath);
            ProcessStartInfo start = new ProcessStartInfo();
            start.FileName = core;
            // Keep the child in core scan mode instead of the no-argument
            // Windows entry point, which launches the GUI shell.
            start.Arguments = "--scan-dir";
            start.WorkingDirectory = ConfigStore.AppRoot;
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.WindowStyle = ProcessWindowStyle.Hidden;
            start.RedirectStandardOutput = true;
            start.RedirectStandardError = true;
            start.RedirectStandardInput = true;
            // The Rust core always emits UTF-8. Relying on the Windows ANSI
            // code page corrupts non-ASCII album paths in GUI events and can
            // turn a valid destination into a missing preview/SQLite path.
            start.StandardOutputEncoding = new UTF8Encoding(false);
            start.StandardErrorEncoding = new UTF8Encoding(false);
            start.EnvironmentVariables["SPLINED_GUI_EVENTS"] = "1";
            start.EnvironmentVariables["SPLINED_GUI_REVIEW"] = "1";
            if (autoScanEnabled) start.EnvironmentVariables["SPLINED_GUI_AUTO_IDEAL"] = "1";
            start.EnvironmentVariables["SPLINED_CONFIG_TOML"] = runConfigText;
            // Pass the resolved physical Windows directory losslessly. The
            // shared SQLite identity may use proper Unicode while the same
            // SMB folder is exposed on Windows with a legacy-decoded name.
            start.EnvironmentVariables["SPLINED_SCAN_DIR_PATH"] = scanPath;
            // Preserve the SQLite/index identity separately from the translated
            // physical directory so completion updates the authoritative row.
            start.EnvironmentVariables["SPLINED_INDEXED_ALBUM_PATH"] = album.Path;
            if (!String.IsNullOrWhiteSpace(album.Key))
                start.EnvironmentVariables["SPLINED_INDEXED_ALBUM_KEY"] = album.Key;
            start.EnvironmentVariables["NO_COLOR"] = "1";
            if (!String.IsNullOrWhiteSpace(retryArtist)) start.EnvironmentVariables["SPLINED_FALLBACK_ARTIST"] = retryArtist;
            if (!String.IsNullOrWhiteSpace(retryAlbum)) start.EnvironmentVariables["SPLINED_FALLBACK_ALBUM"] = retryAlbum;
            if (album.BypassOverride) start.EnvironmentVariables["SPLINED_BYPASS_OVERRIDE"] = "1";
            Process process = new Process();
            process.StartInfo = start;
            process.OutputDataReceived += delegate(object sender, DataReceivedEventArgs args) { if (args.Data != null) HandleCoreLine(args.Data, false); };
            process.ErrorDataReceived += delegate(object sender, DataReceivedEventArgs args) { if (args.Data != null) HandleCoreLine(args.Data, true); };
            try
            {
                RuntimeLog.Write("debug", "album.core.start album=" + scanPath);
                currentProcess = process;
                process.Start();
                process.BeginOutputReadLine();
                process.BeginErrorReadLine();
                // Process.Exited may be raised before the asynchronous stdout
                // and stderr handlers receive their final buffered lines. The
                // completion task must not release the Album loop until both
                // redirected streams have drained, otherwise album_completed
                // can arrive after the report has already become Incomplete.
                Task.Run(delegate
                {
                    int exitCode = -1;
                    try
                    {
                        process.WaitForExit();
                        exitCode = process.ExitCode;
                    }
                    catch (Exception error)
                    {
                        RuntimeLog.Write("error", "album.core.wait_failed album=" + album.Path
                            + " error=" + error.Message);
                    }
                    RuntimeLog.Write(exitCode == 0 ? "debug" : "error",
                        "album.core.exit code=" + exitCode + " output_drained=true album=" + album.Path);
                    completion.TrySetResult(exitCode == 0);
                    process.Dispose();
                });
            }
            catch (Exception error)
            {
                RuntimeLog.Write("error", "album.core.start_failed album=" + album.Path + " error=" + error.Message);
                AppendActivitySafe("  ERROR: " + error.Message + "\r\n", ActivityTone.Error);
                completion.TrySetResult(false);
            }
            return completion.Task;
        }

        private void HandleCoreLine(string line, bool error)
        {
            RuntimeLog.Write(error ? "error" : "debug", "core " + line);
            if (line.StartsWith(EventPrefix, StringComparison.Ordinal))
            {
                string body = line.Substring(EventPrefix.Length);
                try
                {
                    Dictionary<string, object> payload = json.Deserialize<Dictionary<string, object>>(body);
                    if (InvokeRequired) Invoke((MethodInvoker)delegate { ApplyCoreEvent(payload); });
                    else ApplyCoreEvent(payload);
                }
                catch (Exception parseError) { AppendActivitySafe("  Event error: " + parseError.Message + "\r\n", ActivityTone.Error); }
                return;
            }
            if (error && !String.IsNullOrWhiteSpace(line)) AppendActivitySafe("  ERROR: " + StripAnsi(line) + "\r\n", ActivityTone.Error);
        }

        private void ApplyCoreEvent(Dictionary<string, object> payload)
        {
            string eventName = ReadString(payload, "event");
            if (eventName == "local_preflight")
            {
                string message = ReadString(payload, "message");
                if (!String.IsNullOrWhiteSpace(message))
                    AppendActivity("  Local artwork: " + message + "\r\n", ActivityTone.Success);
            }
            else if (eventName == "release_resolved")
            {
                string artist = ReadString(payload, "artist");
                string release = ReadString(payload, "release");
                fallbackMode = ReadBool(payload, "fallback");
                fallbackReason = ReadString(payload, "reason");
                if (activeAlbumStatistics != null)
                {
                    activeAlbumStatistics.Fallback = fallbackMode;
                    activeAlbumStatistics.MusicBrainzResolved = !fallbackMode;
                }
                if (fallbackMode)
                {
                    fallbackArtist = artist;
                    fallbackAlbum = release;
                    SetFallbackControls(true);
                    if (String.IsNullOrWhiteSpace(fallbackReason)) fallbackReason = "TAG FALLBACK";
                    candidateContext.Text = "FALLBACK MODE — " + fallbackReason + " — " + artist + " - " + release;
                    AppendActivity("  2. FALLBACK MODE: " + fallbackReason + "\r\n", ActivityTone.Warning);
                    AppendActivity("     Using tagged Artist + Album because exact MusicBrainz authority is unavailable.\r\n", ActivityTone.Warning);
                    AppendActivity("  3. Querying fallback-capable artwork providers...\r\n");
                }
                else
                {
                    SetFallbackControls(false);
                    candidateContext.Text = artist + " - " + release;
                    AppendActivity("  2. MusicBrainz release resolved: " + artist + " - " + release + "\r\n", ActivityTone.Success);
                    string releaseMbid = ReadString(payload, "release_mbid");
                    string evidenceTrack = ReadString(payload, "evidence_track");
                    if (!String.IsNullOrWhiteSpace(releaseMbid))
                    {
                        string evidenceName = String.IsNullOrWhiteSpace(evidenceTrack)
                            ? "tagged album track"
                            : Path.GetFileName(evidenceTrack);
                        AppendActivity("     Tagged Album ID: " + releaseMbid + " (from " + evidenceName + ")\r\n", ActivityTone.Accent);
                    }
                    AppendActivity("  3. Querying enabled artwork providers...\r\n");
                }
            }
            else if (eventName == "musicbrainz_matches")
            {
                object raw;
                object[] items = payload.TryGetValue("items", out raw) ? ToObjectArray(raw) : new object[0];
                awaitingDecision = true;
                candidateContext.Text = "MusicBrainz Matches — " + ReadString(payload, "artist") + " — " + ReadString(payload, "title");
                UpdateSelectionControls();
                ShowMusicBrainzMatchesWorkspace(
                    ReadString(payload, "artist"), ReadString(payload, "title"),
                    ReadString(payload, "album_artist"), ReadString(payload, "album"), items,
                    ReadBool(payload, "compilation_track"));
            }
            else if (eventName == "compilation_started")
            {
                AppendActivity("  Curated compilation: per-track embedded artwork mode. Folder cover files will not be changed.\r\n", ActivityTone.Warning);
            }
            else if (eventName == "compilation_track_started")
            {
                AppendActivity("  Track " + ReadInt(payload, "index", 0) + "/" + ReadInt(payload, "total", 0)
                    + ": " + ReadString(payload, "artist") + " — " + ReadString(payload, "title") + "\r\n", ActivityTone.Accent);
            }
            else if (eventName == "compilation_track_completed")
            {
                AppendActivity("     " + ReadString(payload, "action") + " from " + ReadString(payload, "source")
                    + " (" + ReadInt(payload, "width", 0) + "x" + ReadInt(payload, "height", 0) + ")\r\n", ActivityTone.Success);
            }
            else if (eventName == "source_results_cache_hit")
            {
                AppendActivity("     Restored " + ReadInt(payload, "candidates", 0)
                    + " cached source result(s); provider APIs were not queried again.\r\n", ActivityTone.Accent);
            }
            else if (eventName == "source_results_restored")
            {
                candidateContext.Text = "Source Results — choose artwork or open MusicBrainz Matches again.";
                awaitingDecision = true;
                UpdateCandidateActions();
            }
            else if (eventName == "musicbrainz_authority_error")
            {
                string message = ReadString(payload, "message");
                AppendActivity("     MusicBrainz authority correction rejected: " + message + "\r\n", ActivityTone.Warning);
                SetStatus("MusicBrainz authority correction was rejected; review the IDs and try again.");
            }
            else if (eventName == "compilation_track_unresolved" || eventName == "compilation_lookup_warning")
            {
                string issue = ReadString(payload, "reason");
                if (String.IsNullOrWhiteSpace(issue)) issue = ReadString(payload, "message");
                AppendActivity("     Unresolved: " + issue + "\r\n", ActivityTone.Warning);
            }
            else if (eventName == "candidates")
            {
                fallbackMode = ReadBool(payload, "fallback") || fallbackMode;
                musicBrainzRetryAvailable = ReadBool(payload, "musicbrainz_retry_available")
                    || ReadBool(payload, "musicbrainz_matches_available");
                string eventFallbackReason = ReadString(payload, "fallback_reason");
                if (!String.IsNullOrWhiteSpace(eventFallbackReason)) fallbackReason = eventFallbackReason;
                object raw;
                if (payload.TryGetValue("items", out raw)) ShowCandidates(raw as object[] ?? ToObjectArray(raw));
                musicBrainzBackAvailable = ReadBool(payload, "musicbrainz_back_available");
                SetFallbackControls(fallbackMode);
                int hidden = ReadInt(payload, "hidden_by_source_policy", 0);
                int evaluated = candidates.Count + hidden;
                if (activeAlbumStatistics != null)
                {
                    activeAlbumStatistics.VisibleCandidates = candidates.Count;
                    activeAlbumStatistics.HiddenCandidates = hidden;
                    activeAlbumStatistics.EvaluatedCandidates = evaluated;
                }
                AppendActivity("  4. Downloaded and evaluated " + evaluated + " real artwork candidate(s); showing " + candidates.Count + " allowed by the active source policies" + (fallbackMode ? " (Python-compatible fallback top 10)." : ".") + "\r\n");
                if (hidden > 0)
                    AppendActivity("     Hidden by active source policy: " + hidden + ".\r\n", ActivityTone.Warning);
                if (candidates.Count == 0 && hidden > 0)
                    candidateContext.Text = "No artwork candidates meet the active source policies. Skip this album or stop processing.";
            }
            else if (eventName == "album_musicbrainz_retry_requested")
            {
                musicBrainzRetryRequested = true;
                candidateContext.Text = "Retrying the current album's MusicBrainz release lookup...";
                ClearCandidates();
                SetFallbackControls(false);
            }
            else if (eventName == "provider_diagnostics")
            {
                object raw;
                if (payload.TryGetValue("items", out raw))
                {
                    object[] diagnostics = ToObjectArray(raw);
                    foreach (object item in diagnostics)
                    {
                        Dictionary<string, object> diagnostic = item as Dictionary<string, object>;
                        if (diagnostic == null) continue;
                        string note = ReadString(diagnostic, "source") + ": " + ReadString(diagnostic, "message");
                        if (activeAlbumStatistics != null
                            && !activeAlbumStatistics.ProviderDiagnosticMessages.Contains(note))
                            activeAlbumStatistics.ProviderDiagnosticMessages.Add(note);
                        AppendActivity("     " + note + "\r\n", ActivityTone.Warning);
                    }
                    if (activeAlbumStatistics != null)
                        activeAlbumStatistics.ProviderDiagnostics = activeAlbumStatistics.ProviderDiagnosticMessages.Count;
                }
            }
            else if (eventName == "album_completed")
            {
                string action = ReadString(payload, "action");
                string destination = ReadString(payload, "destination");
                CaptureFinalArtworkStatistics(payload);
                MarkAlbumOutcome(payload, String.IsNullOrWhiteSpace(action) ? "Completed" : action, destination);
                UpdateCompletedAlbumStatus(payload, action);
                AppendActivity("  5. " + action + ": " + destination + "\r\n", ActivityTone.Success);
                CompleteAlbumEvent(payload, "Artwork choice completed. Candidate results cleared.");
            }
            else if (eventName == "album_postponed")
            {
                MarkAlbumOutcome(payload, "Postponed", ReadString(payload, "reason"));
                AppendActivity("  Timeout active; album postponed by authoritative history.\r\n", ActivityTone.Warning);
                CompleteAlbumEvent(payload, "Album postponed. Candidate results cleared.");
            }
            else if (eventName == "album_skipped")
            {
                string skipReason = ReadString(payload, "reason");
                MarkAlbumOutcome(payload, "Skipped", skipReason);
                AppendActivity("  Album skipped: " + skipReason + ".\r\n", ActivityTone.Warning);
                CompleteAlbumEvent(payload, "Album skipped. Candidate results cleared.");
            }
            else if (eventName == "decision_required")
            {
                string decisionReason = ReadString(payload, "reason");
                if (decisionReason.Equals("fallback", StringComparison.OrdinalIgnoreCase)) fallbackMode = true;
                if (activeAlbumStatistics != null) activeAlbumStatistics.ReviewRequired = true;
                awaitingDecision = true;
                UpdateCandidateActions();
                skip.Enabled = true;
                UpdateSelectionControls();
                AppendActivity(decisionReason.Equals("auto-no-ideal", StringComparison.OrdinalIgnoreCase)
                    ? "  Auto Scan found no policy-qualified Ideal candidate. Operator review is required; resolution alone cannot verify cover accuracy.\r\n"
                    : fallbackMode
                    ? "  Fallback review required. The recommended exception follows Python fallback ranking; select one, compare several, or bypass.\r\n"
                    : "  Review required. Select one candidate to use, select several to compare, or skip this album.\r\n", ActivityTone.Accent);
            }
        }

        private void RebuildCandidateFilterPanel()
        {
            if (candidateFilterPanel == null) return;
            foreach (Control child in candidateFilterPanel.Controls.Cast<Control>().ToArray()) child.Dispose();
            candidateFilterPanel.Controls.Clear();
            candidateFilterBindings.Clear();
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            TableLayoutPanel columns = new TableLayoutPanel
            {
                Dock = DockStyle.Top,
                AutoSize = false,
                Height = 620,
                ColumnCount = 5,
                RowCount = 3,
                Padding = new Padding(ThemeManager.Space4),
                Margin = new Padding(0)
            };
            columns.RowStyles.Add(new RowStyle(SizeType.Absolute, 270));
            columns.RowStyles.Add(new RowStyle(SizeType.Absolute, ThemeManager.Space8));
            columns.RowStyles.Add(new RowStyle(SizeType.Absolute, 326));
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 32));
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, ThemeManager.Space8));
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 36));
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, ThemeManager.Space8));
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 32));

            TableLayoutPanel types = CandidateFilterColumn();
            candidateShowAll = new FluentCheckBox
            {
                Name = "candidateShowAll",
                Text = "Show all results",
                AutoSize = true,
                ForeColor = palette.TextPrimary,
                Margin = new Padding(0, 0, 0, ThemeManager.Space4)
            };
            candidateShowAll.CheckedChanged += delegate
            {
                if (updatingCandidateFilters || !candidateShowAll.Checked) return;
                excludedCandidateSources.Clear();
                excludedCandidateTypes.Clear();
                excludedCandidatePolicies.Clear();
                excludedCandidateRanges.Clear();
                SaveCandidateFilterState();
                ApplyCandidateFilters();
            };
            int showAllRow = AddCandidateFilterRow(types, 27);
            types.Controls.Add(candidateShowAll, 0, showAllRow);
            types.SetColumnSpan(candidateShowAll, 3);
            AddCandidateFilterSection(types, "Image Type", "type", new[]
            {
                new CandidateFilterOption("Recommended", candidates.Values.Any(candidate => candidate.Recommended), palette.Success),
                new CandidateFilterOption("Upscalable", candidates.Values.Any(candidate => candidate.UpscaleEligible), CandidateMagenta),
                new CandidateFilterOption("Rejected", candidates.Values.Any(candidate => candidate.IsNegative), palette.Error),
                new CandidateFilterOption("Local", candidates.Values.Any(candidate => candidate.IsLocal), palette.StatusPurple)
            }, true, true);
            AddCandidateFilterSection(types, "Policy", "policy", new[]
            {
                new CandidateFilterOption("Acceptable", candidates.Values.Any(candidate => candidate.Acceptable), palette.Success),
                new CandidateFilterOption("Strict", candidates.Values.Any(candidate => candidate.StrictOverrideActive), palette.StatusPurple)
            }, true, true);

            TableLayoutPanel sources = CandidateFilterColumn();
            List<CandidateFilterOption> sourceOptions = new List<CandidateFilterOption>();
            IEnumerable<string> sourceOrder = state.Sources
                .Where(source => candidates.Values.Any(candidate => candidate.SourceKey.Equals(source, StringComparison.OrdinalIgnoreCase)))
                .Concat(candidates.Values.Select(candidate => candidate.SourceKey)
                    .Where(source => !state.Sources.Contains(source, StringComparer.OrdinalIgnoreCase))
                    .Distinct(StringComparer.OrdinalIgnoreCase));
            foreach (string source in sourceOrder.Distinct(StringComparer.OrdinalIgnoreCase))
                sourceOptions.Add(new CandidateFilterOption(CandidateSourceName(source), true, CandidateSourceColor(source, palette), source));
            AddCandidateFilterSection(sources, "Source Selection", "source", sourceOptions, false, false);

            TableLayoutPanel ranges = CandidateFilterColumn();
            AddCandidateFilterSection(ranges, "Wanted", "range", new[]
            {
                CandidateRangeOption("Ideal", palette.Success),
                CandidateRangeOption("UpperRange", palette.AccentPrimary, "Upper range"),
                CandidateRangeOption("Ladder", palette.Warning),
                CandidateRangeOption("AboveLadder", palette.StatusOrange, "Above ladder")
            }, true, true);
            AddCandidateFilterSection(ranges, "Unwanted", "range", new[]
            {
                CandidateRangeOption("LowerRange", palette.Warning, "Lower range"),
                CandidateRangeOption("BelowMinimum", palette.Error, "Below minimum")
            }, true, true);

            TableLayoutPanel upscale = BuildUpscaleFilterColumn();

            columns.Controls.Add(WrapCandidateFilterGroup("candidateFindingsGroup", "Candidate Findings", types), 0, 0);
            columns.Controls.Add(WrapCandidateFilterGroup("candidateSourcesGroup", "Source Selection", sources), 2, 0);
            columns.Controls.Add(WrapCandidateFilterGroup("candidateRangesGroup", "Resolution", ranges), 4, 0);
            GroupBox upscaleGroup = WrapCandidateFilterGroup("candidateUpscaleGroup", "Upscale / Advanced", upscale);
            columns.Controls.Add(upscaleGroup, 0, 2);
            columns.SetColumnSpan(upscaleGroup, 5);
            candidateFilterPanel.Controls.Add(columns);
            ApplyRoundedCandidateFilterRegion(candidateFilterPanel);
            ThemeManager.Apply(candidateFilterPanel, uiState.Theme);
            RefreshCandidateFilterDependencies();
        }

        private static GroupBox WrapCandidateFilterGroup(string name, string title, Control content)
        {
            GroupBox group = new FluentGroupBox
            {
                Name = name,
                Text = title,
                Dock = DockStyle.Fill,
                Padding = new Padding(ThemeManager.Space8, ThemeManager.Space12, ThemeManager.Space8, ThemeManager.Space8),
                Margin = new Padding(0)
            };
            content.Dock = DockStyle.Top;
            group.Controls.Add(content);
            return group;
        }

        private static TableLayoutPanel CandidateFilterColumn()
        {
            TableLayoutPanel column = new TableLayoutPanel
            {
                Dock = DockStyle.Top,
                AutoScroll = false,
                AutoSize = true,
                AutoSizeMode = AutoSizeMode.GrowAndShrink,
                ColumnCount = 3,
                RowCount = 0,
                Padding = new Padding(ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space4),
                Margin = new Padding(0)
            };
            // Do not use AutoSize for the option and count cells here. In a
            // percent-sized parent TableLayoutPanel, WinForms can resolve the
            // trailing 100% column first and collapse both AutoSize columns to
            // their minimum widths. The checkbox then paints every label with
            // an ellipsis even though the outer third has ample room.
            column.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 1));
            column.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 1));
            column.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            return column;
        }

        private static int AddCandidateFilterRow(TableLayoutPanel column, float height)
        {
            int row = column.RowCount;
            column.RowCount = row + 1;
            while (column.RowStyles.Count <= row) column.RowStyles.Add(new RowStyle());
            column.RowStyles[row].SizeType = SizeType.Absolute;
            column.RowStyles[row].Height = height;
            return row;
        }

        private TableLayoutPanel BuildUpscaleFilterColumn()
        {
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            upscaleProfileSliders.Clear();
            upscaleProfileValues.Clear();
            TableLayoutPanel column = new TableLayoutPanel
            {
                Dock = DockStyle.Top,
                AutoScroll = false,
                AutoSize = true,
                AutoSizeMode = AutoSizeMode.GrowAndShrink,
                ColumnCount = 1,
                RowCount = 0,
                Padding = new Padding(ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space4),
                Margin = new Padding(0)
            };
            upscaleAdaptiveDefaults = new FluentCheckBox
            {
                Name = "upscaleAdaptiveDefaults",
                Text = "Apply default upscale",
                Checked = state.UpscaleAdaptiveDefaults,
                Dock = DockStyle.Fill,
                AutoSize = false,
                ForeColor = palette.Success,
                Margin = new Padding(0, 1, 0, 0)
            };
            upscaleAdaptiveDefaults.CheckedChanged += delegate
            {
                if (updatingCandidateFilters) return;
                state.UpscaleAdaptiveDefaults = upscaleAdaptiveDefaults.Checked;
                PersistUpscaleProfile();
            };
            AddUpscaleFilterRow(column, upscaleAdaptiveDefaults, 25);
            upscalePreview = new FluentButton
            {
                Name = "upscalePreviewButton",
                Text = "Upscale Preview",
                Dock = DockStyle.Fill,
                Enabled = false,
                Tag = "primary",
                Margin = new Padding(0)
            };
            upscalePreview.Click += UpscalePreviewClicked;
            upscalePreview.EnabledChanged += delegate { ThemeManager.StyleButton(upscalePreview, uiState.Theme); };
            upscaleCandidateDimensions = new Label
            {
                Name = "upscaleCandidateDimensions",
                Text = "Aspect ratio / Resolution",
                Dock = DockStyle.Fill,
                AutoEllipsis = true,
                ForeColor = palette.TextSecondary,
                TextAlign = ContentAlignment.MiddleLeft,
                Margin = new Padding(ThemeManager.Space8, 0, 0, 0)
            };
            TableLayoutPanel previewLine = new TableLayoutPanel
            {
                Name = "upscalePreviewLine",
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = new Padding(0, ThemeManager.Space4, 0, ThemeManager.Space4)
            };
            previewLine.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 168));
            previewLine.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            previewLine.Controls.Add(upscalePreview, 0, 0);
            previewLine.Controls.Add(upscaleCandidateDimensions, 1, 0);
            AddUpscaleFilterRow(column, previewLine, 38);
            AddUpscaleFilterRow(column, new Label
            {
                Name = "upscaleAdvancedLegend",
                Text = "▣ Picture   ◆ Sharpen   ◌ Softness   ◐ Contrast   ☀ Exposure   ✦ Brightness   γ Gamma   🌡 Color",
                Dock = DockStyle.Fill,
                AutoEllipsis = false,
                TextAlign = ContentAlignment.MiddleLeft,
                Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                Margin = new Padding(0, ThemeManager.Space4, 0, ThemeManager.Space4)
            }, 27);
            FlowLayoutPanel advanced = new FlowLayoutPanel
            {
                Name = "upscaleAdvancedControls",
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                AutoScroll = true,
                Margin = new Padding(0),
                Padding = new Padding(0, 0, 0, ThemeManager.Space4)
            };
            advanced.Controls.Add(BuildUpscaleProfileControl("picture", "Picture", -20, 20, state.UpscalePicturePercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("sharpen", "Sharpen", 0, 20, state.UpscaleSharpenPercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("softness", "Softness", 0, 20, state.UpscaleSoftnessPercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("contrast", "Contrast", -20, 20, state.UpscaleContrastPercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("exposure", "Exposure", -20, 20, state.UpscaleExposurePercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("brightness", "Brightness", -20, 20, state.UpscaleBrightnessPercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("gamma", "Gamma", -20, 20, state.UpscaleGammaPercent));
            advanced.Controls.Add(BuildUpscaleProfileControl("temperature", "Color", -100, 100, state.UpscaleColorTemperature));
            AddUpscaleFilterRow(column, advanced, 220);
            return column;
        }

        private static void AddUpscaleFilterRow(TableLayoutPanel column, Control control, float height)
        {
            int row = AddCandidateFilterRow(column, height);
            column.Controls.Add(control, 0, row);
        }

        private Control BuildUpscaleProfileControl(string key, string label, int minimum, int maximum, int value)
        {
            TableLayoutPanel control = new TableLayoutPanel
            {
                Name = "upscaleProfileControl_" + key,
                Width = 104,
                Height = 212,
                ColumnCount = 1,
                RowCount = 4,
                Margin = new Padding(0, 0, ThemeManager.Space4, 0),
                Padding = new Padding(2)
            };
            control.RowStyles.Add(new RowStyle(SizeType.Absolute, 22));
            control.RowStyles.Add(new RowStyle(SizeType.Absolute, 22));
            control.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            control.RowStyles.Add(new RowStyle(SizeType.Absolute, 35));
            Label heading = new Label
            {
                Text = UpscaleProfileIcon(key),
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.MiddleCenter,
                AutoEllipsis = false,
                Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                Margin = new Padding(0)
            };
            Label current = new Label
            {
                Name = "upscaleProfileValue_" + key,
                Text = UpscaleProfileDisplay(key, value),
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.MiddleCenter,
                AutoEllipsis = true,
                Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                Margin = new Padding(0)
            };
            TrackBar slider = new TrackBar
            {
                Name = "upscaleProfileSlider_" + key,
                Minimum = minimum,
                Maximum = maximum,
                TickFrequency = key.Equals("temperature", StringComparison.OrdinalIgnoreCase) ? 20 : 5,
                SmallChange = 1,
                LargeChange = key.Equals("temperature", StringComparison.OrdinalIgnoreCase) ? 10 : 5,
                Value = Math.Max(minimum, Math.Min(maximum, value)),
                Dock = DockStyle.Fill,
                Orientation = Orientation.Vertical,
                TickStyle = TickStyle.Both,
                Margin = new Padding(22, 2, 22, 2)
            };
            upscaleProfileSliders[key] = slider;
            upscaleProfileValues[key] = current;
            TableLayoutPanel resetRow = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 3,
                RowCount = 1,
                Margin = new Padding(0)
            };
            resetRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 35));
            resetRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 30));
            resetRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 35));
            FluentButton reset = new FluentButton
            {
                Name = "upscaleProfileReset_" + key,
                Text = "↺",
                Dock = DockStyle.Fill,
                Margin = new Padding(1),
                Tag = "compact"
            };
            reset.Click += delegate { slider.Value = 0; };
            ToolTip resetHelp = ThemeManager.CreateToolTip();
            resetHelp.SetToolTip(reset, "Reset " + label + " to its default value.");
            resetHelp.SetToolTip(heading, label);
            resetHelp.SetToolTip(current, label + " current value");
            resetHelp.SetToolTip(slider, "Adjust " + label.ToLowerInvariant() + ".");
            control.Tag = resetHelp;
            resetRow.Controls.Add(reset, 1, 0);
            slider.ValueChanged += delegate
            {
                current.Text = UpscaleProfileDisplay(key, slider.Value);
                SetUpscaleProfileValue(key, slider.Value);
                PersistUpscaleProfile();
            };
            control.Controls.Add(heading, 0, 0);
            control.Controls.Add(current, 0, 1);
            control.Controls.Add(slider, 0, 2);
            control.Controls.Add(resetRow, 0, 3);
            return control;
        }

        private static string UpscaleProfileDisplay(string key, int value)
        {
            if (key.Equals("temperature", StringComparison.OrdinalIgnoreCase))
                return value == 0 ? "<Middle>" : value < 0 ? "<Cool " + Math.Abs(value) + ">" : "<Warm " + value + ">";
            return "<" + value.ToString("+0;-0;0") + "%>";
        }

        private static string UpscaleProfileIcon(string key)
        {
            if (key.Equals("picture", StringComparison.OrdinalIgnoreCase)) return "▣";
            if (key.Equals("sharpen", StringComparison.OrdinalIgnoreCase)) return "◆";
            if (key.Equals("softness", StringComparison.OrdinalIgnoreCase)) return "◌";
            if (key.Equals("contrast", StringComparison.OrdinalIgnoreCase)) return "◐";
            if (key.Equals("exposure", StringComparison.OrdinalIgnoreCase)) return "☀";
            if (key.Equals("brightness", StringComparison.OrdinalIgnoreCase)) return "✦";
            if (key.Equals("gamma", StringComparison.OrdinalIgnoreCase)) return "γ";
            return "🌡";
        }

        private void SetUpscaleProfileValue(string key, int value)
        {
            if (key.Equals("picture", StringComparison.OrdinalIgnoreCase)) state.UpscalePicturePercent = value;
            else if (key.Equals("sharpen", StringComparison.OrdinalIgnoreCase)) state.UpscaleSharpenPercent = value;
            else if (key.Equals("softness", StringComparison.OrdinalIgnoreCase)) state.UpscaleSoftnessPercent = value;
            else if (key.Equals("contrast", StringComparison.OrdinalIgnoreCase)) state.UpscaleContrastPercent = value;
            else if (key.Equals("exposure", StringComparison.OrdinalIgnoreCase)) state.UpscaleExposurePercent = value;
            else if (key.Equals("brightness", StringComparison.OrdinalIgnoreCase)) state.UpscaleBrightnessPercent = value;
            else if (key.Equals("gamma", StringComparison.OrdinalIgnoreCase)) state.UpscaleGammaPercent = value;
            else if (key.Equals("temperature", StringComparison.OrdinalIgnoreCase)) state.UpscaleColorTemperature = value;
        }

        private void PersistUpscaleProfile()
        {
            try
            {
                ConfigStore.Save(state, false);
                RuntimeLog.Write("info", "windows.upscale.profile adaptive=" + state.UpscaleAdaptiveDefaults
                    + " picture=" + state.UpscalePicturePercent + " sharpen=" + state.UpscaleSharpenPercent
                    + " softness=" + state.UpscaleSoftnessPercent + " contrast=" + state.UpscaleContrastPercent
                    + " exposure=" + state.UpscaleExposurePercent + " brightness=" + state.UpscaleBrightnessPercent
                    + " gamma=" + state.UpscaleGammaPercent
                    + " temperature=" + state.UpscaleColorTemperature);
            }
            catch (Exception error)
            {
                SetStatus("Unable to save upscale profile: " + error.Message);
                return;
            }
            ScheduleUpscalePreview();
        }

        private void ScheduleUpscalePreview()
        {
            CandidateView candidate;
            if (selectedCandidateIndex < 0 || !candidates.TryGetValue(selectedCandidateIndex, out candidate)
                || !CandidateCanEdit(candidate)) return;
            if (candidate.IsLocal) editedLocalCandidateIndexes.Add(candidate.Index);
            upscalePreviewTimer.Stop();
            upscalePreviewTimer.Start();
        }

        private static bool CandidateCanEdit(CandidateView candidate)
        {
            return candidate != null && (candidate.UpscaleEligible || candidate.IsLocal);
        }

        private CandidateFilterOption CandidateRangeOption(string key, Color color, string label = null)
        {
            return new CandidateFilterOption(label ?? DisplayRange(key),
                candidates.Values.Any(candidate => candidate.SourceRangeKey.Equals(key, StringComparison.OrdinalIgnoreCase)), color, key);
        }

        private void AddCandidateFilterSection(TableLayoutPanel column, string title, string dimension,
            IEnumerable<CandidateFilterOption> options, bool showHeading = true, bool showMissing = false)
        {
            List<CandidateFilterOption> present = (showMissing ? options : options.Where(option => option.Present)).ToList();
            if (present.Count == 0) return;
            Font optionFont = ThemeManager.UiFont(ThemeFontRole.Control);
            TextFormatFlags measureFlags = TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix;
            int optionWidth = present.Max(option => TextRenderer.MeasureText(option.Label, optionFont,
                new Size(Int32.MaxValue, Int32.MaxValue), measureFlags).Width) + 34;
            int countWidth = TextRenderer.MeasureText("[9999]", optionFont,
                new Size(Int32.MaxValue, Int32.MaxValue), measureFlags).Width + ThemeManager.Space8;
            column.ColumnStyles[0].Width = Math.Max(column.ColumnStyles[0].Width, optionWidth);
            column.ColumnStyles[1].Width = Math.Max(column.ColumnStyles[1].Width, countWidth);
            if (showHeading)
            {
                Label heading = new Label
                {
                    Text = title,
                    AutoSize = true,
                    Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                    Margin = new Padding(0, ThemeManager.Space4, 0, ThemeManager.Space4)
                };
                int headingRow = AddCandidateFilterRow(column, 27);
                column.Controls.Add(heading, 0, headingRow);
                column.SetColumnSpan(heading, 3);
            }
            foreach (CandidateFilterOption option in present)
            {
                CheckBox check = new FluentCheckBox
                {
                    Name = "candidateFilter_" + dimension + "_" + option.Key,
                    Text = option.Label,
                    AutoSize = false,
                    Dock = DockStyle.Fill,
                    Font = optionFont,
                    ForeColor = option.Color,
                    Tag = option.Key,
                    Margin = new Padding(0, 1, ThemeManager.Space4, 0)
                };
                Label count = new Label
                {
                    Text = "[0]",
                    AutoSize = false,
                    Dock = DockStyle.Fill,
                    Font = optionFont,
                    TextAlign = ContentAlignment.MiddleLeft,
                    ForeColor = option.Color,
                    Margin = new Padding(0, 4, 0, 0)
                };
                CandidateFilterBinding binding = new CandidateFilterBinding(dimension, option.Key, option.Color, check, count);
                candidateFilterBindings.Add(binding);
                check.CheckedChanged += delegate
                {
                    if (updatingCandidateFilters) return;
                    HashSet<string> excluded = CandidateExcludedSet(dimension);
                    if (check.Checked) excluded.Remove(option.Key);
                    else excluded.Add(option.Key);
                    SaveCandidateFilterState();
                    ApplyCandidateFilters();
                };
                int optionRow = AddCandidateFilterRow(column, 25);
                column.Controls.Add(check, 0, optionRow);
                column.Controls.Add(count, 1, optionRow);
            }
        }

        private void UpdateCompletedAlbumStatus(Dictionary<string, object> payload, string action)
        {
            string mode = ReadString(payload, "mode");
            if (!mode.Equals("write", StringComparison.OrdinalIgnoreCase)) return;
            if (action.Equals("Bypassed", StringComparison.OrdinalIgnoreCase)
                || action.Equals("Postponed", StringComparison.OrdinalIgnoreCase)
                || action.Equals("Skipped", StringComparison.OrdinalIgnoreCase)
                || action.Equals("NoSelection", StringComparison.OrdinalIgnoreCase)) return;
            string albumPath = ReadString(payload, "album_path");
            AlbumInfo completed = albums.FirstOrDefault(album => SameAlbumPath(album.Path, albumPath))
                ?? activeLaunchAlbum;
            if (completed == null) return;
            completed.State = AlbumState.Processed;
            completed.CompletedUtc = DateTime.UtcNow;
            completed.Outcome = ReadString(payload, "source");
            BuildTree();
            if (displayedAlbum != null && SameAlbumPath(displayedAlbum.Path, completed.Path))
            {
                displayedAlbum = completed;
                RenderDisplayedAlbum();
            }
        }

        private HashSet<string> CandidateExcludedSet(string dimension)
        {
            if (dimension == "source") return excludedCandidateSources;
            if (dimension == "type") return excludedCandidateTypes;
            if (dimension == "policy") return excludedCandidatePolicies;
            return excludedCandidateRanges;
        }

        private void SaveCandidateFilterState()
        {
            uiState.CandidateExcludedSources = excludedCandidateSources.OrderBy(value => value, StringComparer.OrdinalIgnoreCase).ToList();
            uiState.CandidateExcludedTypes = excludedCandidateTypes.OrderBy(value => value, StringComparer.OrdinalIgnoreCase).ToList();
            uiState.CandidateExcludedPolicies = excludedCandidatePolicies.OrderBy(value => value, StringComparer.OrdinalIgnoreCase).ToList();
            uiState.CandidateExcludedRanges = excludedCandidateRanges.OrderBy(value => value, StringComparer.OrdinalIgnoreCase).ToList();
            ConfigStore.SaveUi(uiState);
        }

        private void RefreshCandidateFilterDependencies()
        {
            if (candidateFilterPanel == null) return;
            updatingCandidateFilters = true;
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            foreach (CandidateFilterBinding binding in candidateFilterBindings)
            {
                int count = candidates.Values.Count(candidate => CandidatePassesFilters(candidate, binding.Dimension)
                    && CandidateMatchesFilterOption(candidate, binding.Dimension, binding.Key));
                bool available = count > 0;
                binding.Check.Enabled = available;
                binding.Check.Checked = available && !CandidateExcludedSet(binding.Dimension).Contains(binding.Key);
                binding.Check.ForeColor = available ? binding.Color : palette.TextDisabled;
                binding.Count.Text = "[" + count + "]";
                binding.Count.ForeColor = available ? binding.Color : palette.TextDisabled;
            }
            if (candidateShowAll != null)
            {
                candidateShowAll.Checked = CandidateActiveFilterCount() == 0;
                candidateShowAll.ForeColor = palette.TextPrimary;
            }
            updatingCandidateFilters = false;
            UpdateCandidateFilterButtonText();
        }

        private bool CandidateMatchesFilterOption(CandidateView view, string dimension, string key)
        {
            if (dimension == "source") return view.SourceKey.Equals(key, StringComparison.OrdinalIgnoreCase);
            if (dimension == "range") return view.SourceRangeKey.Equals(key, StringComparison.OrdinalIgnoreCase);
            if (dimension == "policy")
                return key.Equals("Acceptable", StringComparison.OrdinalIgnoreCase) ? view.Acceptable : view.StrictOverrideActive;
            if (key.Equals("Recommended", StringComparison.OrdinalIgnoreCase)) return view.Recommended;
            if (key.Equals("Upscalable", StringComparison.OrdinalIgnoreCase)) return view.UpscaleEligible;
            if (key.Equals("Rejected", StringComparison.OrdinalIgnoreCase)) return view.IsNegative;
            return view.IsLocal;
        }

        private void ApplyCandidateFilters()
        {
            DisposeCandidateCards();
            foreach (CandidateView view in SortedCandidateViews().Where(CandidatePassesFilters))
                candidateCards.Controls.Add(BuildCandidateCard(view));
            RefreshCandidateFilterDependencies();
            ResizeCandidateCards();
            UpdateHoverButton();
            UpdateCandidateActions();
            RefreshSelectedCandidatePreview();
        }

        private int CandidateActiveFilterCount()
        {
            return excludedCandidateSources.Count + excludedCandidateTypes.Count
                + excludedCandidatePolicies.Count + excludedCandidateRanges.Count;
        }

        private void UpdateCandidateFilterButtonText()
        {
            if (candidateFilterButton == null) return;
            int active = CandidateActiveFilterCount();
            string stateText = active == 0 ? "All" : active + " active";
            candidateFilterButton.Enabled = candidates.Count > 0;
            candidateFilterButton.Text = "Artwork Filter · " + stateText
                + (CandidateFilterWorkspaceVisible ? "  ▾" : "  ▸");
        }

        private IEnumerable<CandidateView> SortedCandidateViews()
        {
            return candidates.Values
                .OrderBy(candidate => candidate.IsLocal ? 0 : candidate.Recommended ? 1 : 2)
                .ThenByDescending(candidate => (long)candidate.PixelWidth * candidate.PixelHeight)
                .ThenByDescending(candidate => Math.Min(candidate.PixelWidth, candidate.PixelHeight))
                .ThenBy(candidate => candidate.Index);
        }

        private bool CandidatePassesFilters(CandidateView view)
        {
            return CandidatePassesFilters(view, "");
        }

        private bool CandidatePassesFilters(CandidateView view, string ignoredDimension)
        {
            if (ignoredDimension != "source" && excludedCandidateSources.Contains(view.SourceKey)) return false;
            if (ignoredDimension != "type")
            {
                if (view.Recommended && excludedCandidateTypes.Contains("Recommended")) return false;
                if (view.UpscaleEligible && excludedCandidateTypes.Contains("Upscalable")) return false;
                if (view.IsNegative && excludedCandidateTypes.Contains("Rejected")) return false;
                if (view.IsLocal && excludedCandidateTypes.Contains("Local")) return false;
            }
            if (ignoredDimension != "policy")
            {
                if (view.Acceptable && excludedCandidatePolicies.Contains("Acceptable")) return false;
                if (view.StrictOverrideActive && excludedCandidatePolicies.Contains("Strict")) return false;
            }
            return ignoredDimension == "range" || !excludedCandidateRanges.Contains(view.SourceRangeKey);
        }

        private void ShowCandidates(object[] items)
        {
            ClearCandidates();
            int index = 0;
            foreach (object item in items)
            {
                Dictionary<string, object> candidate = item as Dictionary<string, object>;
                if (candidate == null) continue;
                int candidateIndex = ReadInt(candidate, "index", ++index);
                bool recommended = ReadBool(candidate, "recommended");
                CandidateView view = new CandidateView();
                view.Index = candidateIndex;
                view.Source = ReadString(candidate, "source");
                view.PixelWidth = ReadInt(candidate, "width", 0);
                view.PixelHeight = ReadInt(candidate, "height", 0);
                view.Resolution = view.PixelWidth + " x " + view.PixelHeight;
                view.Format = ReadString(candidate, "format");
                view.SourceRange = ReadString(candidate, "source_range_class");
                view.Range = ReadString(candidate, "range_class");
                view.ProjectedWidth = ReadInt(candidate, "projected_width", view.PixelWidth);
                view.ProjectedHeight = ReadInt(candidate, "projected_height", view.PixelHeight);
                view.Upscaled = ReadBool(candidate, "upscaled");
                view.Approved = ReadBool(candidate, "approved");
                view.Square = ReadBool(candidate, "square");
                view.Acceptable = ReadBool(candidate, "acceptable");
                view.PolicyStatus = ReadString(candidate, "policy_status");
                view.PolicyReason = ReadString(candidate, "policy_reason");
                view.SourceOverrideActive = ReadBool(candidate, "source_override_active");
                view.StrictOverrideActive = ReadBool(candidate, "strict_override_active");
                view.StrictStatus = ReadString(candidate, "strict_status");
                view.StrictReason = ReadString(candidate, "strict_reason");
                view.StrictPreferredEligible = ReadBool(candidate, "strict_preferred_eligible");
                view.StrictAutoEligible = ReadBool(candidate, "strict_auto_eligible");
                view.Recommended = recommended;
                view.CachePath = ReadString(candidate, "cache_path");
                view.Url = ReadString(candidate, "url");
                view.LocalOrigin = ReadString(candidate, "local_origin");
                view.LocalReference = ReadString(candidate, "local_reference");
                if (String.IsNullOrWhiteSpace(view.SourceRange)) view.SourceRange = CandidateRangeKey(Math.Min(view.PixelWidth, view.PixelHeight));
                if (String.IsNullOrWhiteSpace(view.Range)) view.Range = CandidateRangeKey(Math.Min(view.ProjectedWidth, view.ProjectedHeight));
                view.UpscaleEligible = view.Upscaled && view.Acceptable && !view.IsRejected
                    && Math.Min(view.PixelWidth, view.PixelHeight) < state.RangeIdeal;
                candidates[candidateIndex] = view;
                if (recommended)
                {
                    recommendedCandidateIndex = candidateIndex;
                    compareCandidateIndexes.Add(candidateIndex);
                    selectedCandidateIndex = candidateIndex;
                }
            }
            RebuildCandidateFilterPanel();
            if (selectedCandidateIndex < 0)
            {
                CandidateView editableLocal = candidates.Values.FirstOrDefault(candidate => candidate.IsLocal && CandidateCanEdit(candidate));
                if (editableLocal != null)
                {
                    compareCandidateIndexes.Add(editableLocal.Index);
                    selectedCandidateIndex = editableLocal.Index;
                }
            }
            ApplyCandidateFilters();
            SetCandidateFilterExpanded(uiState.CandidateFilterExpanded, false);
        }

        private string CandidateRangeKey(int shortSide)
        {
            if (shortSide < state.RangeMin) return "BelowMinimum";
            if (shortSide < state.RangeIdeal) return "LowerRange";
            if (shortSide == state.RangeIdeal) return "Ideal";
            if (shortSide <= state.RangeMax) return "UpperRange";
            if (shortSide <= state.RangeLadder) return "Ladder";
            return "AboveLadder";
        }

        private Control BuildCandidateCard(CandidateView view)
        {
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            bool sourceRejected = view.SourceOverrideActive
                && view.PolicyStatus.Equals("reject", StringComparison.OrdinalIgnoreCase);
            CardVisualRole role = view.IsLocal ? CardVisualRole.LocalCandidateGlass
                : view.UpscaleEligible ? CardVisualRole.UpscaleCandidateGlass
                : view.IsNegative
                    ? CardVisualRole.RejectedCandidateGlass
                    : view.Recommended ? CardVisualRole.Recommended
                    : CardVisualRole.CandidateGlass;
            Panel card = new FluentCardPanel
            {
                Width = 196,
                Height = 234,
                Margin = new Padding(ThemeManager.Space4),
                Padding = new Padding(1),
                Tag = view.Index,
                VisualRole = role
            };
            CheckBox choose = new FluentCheckBox
            {
                Left = 8,
                Top = 6,
                Width = 176,
                Height = 24,
                Text = CandidateHeading(view),
                Checked = !sourceRejected && compareCandidateIndexes.Contains(view.Index),
                Enabled = !sourceRejected,
                Tag = view.Index,
                ForeColor = CandidateSourceColor(view.SourceKey, palette)
            };
            choose.Name = "candidateChoice";
            choose.CheckedChanged += delegate
            {
                if (choose.Checked) compareCandidateIndexes.Add(view.Index);
                else compareCandidateIndexes.Remove(view.Index);
                selectedCandidateIndex = compareCandidateIndexes.Count == 1 ? compareCandidateIndexes.First() : -1;
                UpdateCandidateActions();
                RefreshSelectedCandidatePreview();
            };
            card.Controls.Add(choose);
            PictureBox image = new PictureBox
            {
                Name = "candidateImage",
                Left = 17,
                Top = 34,
                Width = 160,
                Height = 160,
                SizeMode = PictureBoxSizeMode.Zoom,
                BorderStyle = BorderStyle.None,
                BackColor = palette.SurfacePrimary
            };
            image.Image = LoadImageCopy(view.CachePath);
            image.Cursor = Cursors.Hand;
            image.MouseEnter += delegate { if (uiState.HoverEnabled) ShowHoverPreview(view); };
            image.MouseLeave += delegate { if (uiState.HoverEnabled) CloseHoverPreview(); };
            card.Controls.Add(image);
            LinkLabel sourceLink = CandidateUrlUi.Create("ⓘ", view.Url, uiState.Theme,
                delegate { if (uiState.ShowArtwork) ShowArtworkCandidate(view); },
                delegate { if (uiState.HoverEnabled) ShowHoverPreview(view); },
                delegate { if (uiState.HoverEnabled) CloseHoverPreview(); });
            sourceLink.Left = 8; sourceLink.Top = 202; sourceLink.Width = 180; sourceLink.Height = 24;
            sourceLink.Name = "candidateUrl";
            sourceLink.TextAlign = ContentAlignment.MiddleLeft;
            card.Controls.Add(sourceLink);
            ToolTip details = ThemeManager.CreateToolTip();
            string detailText = CandidateDetails(view);
            details.SetToolTip(card, detailText);
            details.SetToolTip(choose, detailText);
            details.SetToolTip(image, detailText);
            details.SetToolTip(sourceLink, detailText);
            sourceLink.Tag = details;
            card.Click += delegate { if (choose.Enabled) choose.Checked = !choose.Checked; };
            card.Resize += delegate { LayoutCandidateCard(card); };
            ThemeManager.Apply(card, uiState.Theme);
            choose.ForeColor = CandidateSourceColor(view.SourceKey, palette);
            image.BackColor = palette.SurfacePrimary;
            LayoutCandidateCard(card);
            return card;
        }

        private static string CandidateHeading(CandidateView view)
        {
            string marker = view.IsLocal ? "⌂" : "";
            if (!view.IsLocal && view.Recommended) marker += "★";
            if (!view.IsLocal && view.UpscaleEligible) marker += "⇧";
            else if (!view.IsLocal && view.IsNegative) marker += "×";
            if (marker.Length > 0) marker += " ";
            return marker + CandidateSourceName(view.SourceKey);
        }

        private static string CandidateSourceName(string source)
        {
            if (String.IsNullOrWhiteSpace(source)) return "Unknown";
            switch (source.Trim().ToLowerInvariant())
            {
                case "local": return "Local";
                case "itunes": return "iTunes";
                case "coverartarchive": return "CAA Cover";
                case "musicbrainz": return "MusicBrainz";
                case "fanarttv": return "FanArtTV";
                case "amazon": return "Amazon";
                case "discogs": return "Discogs";
                case "lastfm": return "Last.fm";
                case "deezer": return "Deezer";
                case "webpstill": return "Local";
                default: return source;
            }
        }

        private static Color CandidateSourceColor(string source, ThemePalette palette)
        {
            switch ((source ?? "").Trim().ToLowerInvariant())
            {
                case "local":
                case "webpstill": return palette.StatusPurple;
                case "itunes": return palette.AccentPrimary;
                case "coverartarchive": return palette.Warning;
                case "musicbrainz": return palette.Link;
                case "fanarttv": return palette.Success;
                case "amazon": return palette.StatusOrange;
                case "discogs": return palette.StatusPurple;
                case "lastfm": return palette.Error;
                case "deezer": return palette.StatusBlue;
                default: return palette.TextPrimary;
            }
        }

        private static string CandidateDetails(CandidateView view)
        {
            StringBuilder detail = new StringBuilder();
            detail.AppendLine("Source: " + view.DisplaySource);
            detail.AppendLine("Source image: " + view.Resolution + " · " + DisplayRange(view.SourceRangeKey));
            if (view.ProjectedWidth > 0 && view.ProjectedHeight > 0
                && (view.ProjectedWidth != view.PixelWidth || view.ProjectedHeight != view.PixelHeight))
                detail.AppendLine("Projected result: " + view.ProjectedWidth + " x " + view.ProjectedHeight + " · " + DisplayRange(view.ProjectedRangeKey));
            detail.AppendLine("Format: " + (String.IsNullOrWhiteSpace(view.Format) ? "unknown" : view.Format.ToUpperInvariant())
                + " · Square: " + (view.Square ? "yes" : "no"));
            detail.AppendLine("Approved: " + (view.Approved ? "yes" : "no")
                + " · Acceptable: " + (view.Acceptable ? "yes" : "no"));
            if (view.Upscaled)
                detail.AppendLine("Upscale: " + (view.UpscaleEligible ? "eligible after policy and Minimum checks" : "not eligible for Preferred/Auto"));
            if (view.StrictOverrideActive)
                detail.AppendLine("Strict: " + view.StrictStatus + " · Preferred " + (view.StrictPreferredEligible ? "yes" : "no")
                    + " · Auto " + (view.StrictAutoEligible ? "yes" : "no"));
            string reason = view.StrictOverrideActive && !String.IsNullOrWhiteSpace(view.StrictReason) ? view.StrictReason : view.PolicyReason;
            if (!String.IsNullOrWhiteSpace(reason)) detail.AppendLine("Policy: " + reason);
            if (view.Recommended) detail.Append("SPLINED recommended result");
            return detail.ToString().TrimEnd();
        }

        private static string DisplayRange(string key)
        {
            switch (key)
            {
                case "BelowMinimum": return "Below minimum";
                case "LowerRange": return "Lower range";
                case "UpperRange": return "Upper range";
                case "AboveLadder": return "Above ladder";
                default: return key;
            }
        }

        private void ResizeCandidateCards()
        {
            if (candidateCards == null || candidateCards.Controls.Count == 0) return;
            int available = Math.Max(156, candidateCards.ClientSize.Width - candidateCards.Padding.Horizontal - 22);
            int gap = ThemeManager.Space4 * 2;
            int columns = Math.Max(1, Math.Min(candidateCards.Controls.Count, (available + gap) / (180 + gap)));
            int width = Math.Max(156, Math.Min(240, (available - gap * columns) / columns));
            foreach (Control card in candidateCards.Controls)
            {
                card.Size = new Size(width, width + 44);
                LayoutCandidateCard(card);
            }
            candidateCards.PerformLayout();
        }

        private static void LayoutCandidateCard(Control card)
        {
            if (card == null) return;
            int contentWidth = Math.Max(120, card.ClientSize.Width - 16);
            int imageSize = Math.Max(100, Math.Min(contentWidth, card.ClientSize.Height - 66));
            Control choice = card.Controls["candidateChoice"];
            Control image = card.Controls["candidateImage"];
            Control url = card.Controls["candidateUrl"];
            if (choice != null) choice.SetBounds(8, 6, contentWidth, 24);
            if (image != null) image.SetBounds(8 + Math.Max(0, (contentWidth - imageSize) / 2), 34, imageSize, imageSize);
            int metadataTop = 38 + imageSize;
            if (url != null) url.SetBounds(8, metadataTop, contentWidth, 22);
        }

        private void UpdateCandidateActions()
        {
            selectedCandidateIndex = compareCandidateIndexes.Count == 1 ? compareCandidateIndexes.First() : -1;
            bool standaloneSelection = standaloneExistingCoverEdit && selectedCandidateIndex >= 0
                && candidates.ContainsKey(selectedCandidateIndex) && candidates[selectedCandidateIndex].IsLocal;
            useSelected.Text = standaloneExistingCoverEdit ? "Save Existing" : "Use Selected";
            useSelected.Enabled = (awaitingDecision || standaloneSelection) && selectedCandidateIndex >= 0;
            bool hasLocal = candidates.Values.Any(candidate => candidate.Source.Equals("local", StringComparison.OrdinalIgnoreCase) || candidate.Source.Equals("webpstill", StringComparison.OrdinalIgnoreCase));
            keepLocal.Visible = hasLocal;
            keepLocal.Enabled = awaitingDecision && hasLocal;
            compare.Enabled = candidates.Count > 1 && compareCandidateIndexes.Count > 1;
            skip.Enabled = awaitingDecision;
            refineFallback.Enabled = awaitingDecision && fallbackMode;
            retryMusicBrainz.Visible = musicBrainzRetryAvailable;
            retryMusicBrainz.Enabled = awaitingDecision && musicBrainzRetryAvailable;
            backToMusicBrainz.Visible = musicBrainzBackAvailable;
            backToMusicBrainz.Enabled = awaitingDecision && musicBrainzBackAvailable;
            CandidateView previewCandidate = null;
            bool canPreviewUpscale = selectedCandidateIndex >= 0
                && candidates.TryGetValue(selectedCandidateIndex, out previewCandidate)
                && CandidateCanEdit(previewCandidate);
            if (upscalePreview != null)
            {
                upscalePreview.Visible = true;
                upscalePreview.Enabled = canPreviewUpscale;
            }
            UpdateUpscaleCandidateContext(canPreviewUpscale ? previewCandidate : null);
            ThemeManager.StyleButton(useSelected, uiState.Theme);
            ThemeManager.StyleButton(keepLocal, uiState.Theme);
            ThemeManager.StyleButton(refineFallback, uiState.Theme);
            ThemeManager.StyleButton(retryMusicBrainz, uiState.Theme);
            ThemeManager.StyleButton(backToMusicBrainz, uiState.Theme);
            if (upscalePreview != null) ThemeManager.StyleButton(upscalePreview, uiState.Theme);
        }

        private void UpdateUpscaleCandidateContext(CandidateView candidate)
        {
            if (upscaleCandidateDimensions == null) return;
            if (candidate == null)
            {
                upscaleCandidateDimensions.Text = "Select one upscalable result";
                return;
            }
            int divisor = GreatestCommonDivisor(Math.Max(1, candidate.PixelWidth), Math.Max(1, candidate.PixelHeight));
            string ratio = (candidate.PixelWidth / divisor) + ":" + (candidate.PixelHeight / divisor);
            int targetWidth = candidate.ProjectedWidth > 0 ? candidate.ProjectedWidth : state.RangeIdeal;
            int targetHeight = candidate.ProjectedHeight > 0 ? candidate.ProjectedHeight : state.RangeIdeal;
            upscaleCandidateDimensions.Text = ratio + "  /  " + candidate.PixelWidth + " × " + candidate.PixelHeight
                + " → " + targetWidth + " × " + targetHeight;
        }

        private static int GreatestCommonDivisor(int left, int right)
        {
            while (right != 0)
            {
                int remainder = left % right;
                left = right;
                right = remainder;
            }
            return Math.Max(1, left);
        }

        private void SetFallbackControls(bool visible)
        {
            if (refineFallback == null) return;
            refineFallback.Visible = visible;
            refineFallback.Enabled = visible && awaitingDecision;
            retryMusicBrainz.Visible = musicBrainzRetryAvailable;
            retryMusicBrainz.Enabled = awaitingDecision && musicBrainzRetryAvailable;
            ThemeManager.StyleButton(refineFallback, uiState.Theme);
            ThemeManager.StyleButton(retryMusicBrainz, uiState.Theme);
        }

        private void ClearCandidates()
        {
            CloseHoverPreview();
            CloseMusicBrainzMatchesWorkspace(false);
            DisposeCandidateCards();
            candidateCards.AutoScrollPosition = Point.Empty;
            candidates.Clear();
            standaloneExistingCoverEdit = false;
            candidateFilterBindings.Clear();
            if (candidateFilterPanel != null)
            {
                foreach (Control child in candidateFilterPanel.Controls.Cast<Control>().ToArray()) child.Dispose();
                candidateFilterPanel.Controls.Clear();
            }
            SetCandidateFilterExpanded(false, false);
            UpdateCandidateFilterButtonText();
            compareCandidateIndexes.Clear();
            selectedCandidateIndex = -1;
            recommendedCandidateIndex = -1;
            awaitingDecision = false;
            useSelected.Enabled = false;
            useSelected.Text = "Use Selected";
            keepLocal.Enabled = false;
            keepLocal.Visible = false;
            compare.Enabled = false;
            skip.Enabled = false;
            upscalePreviewTimer.Stop();
            upscalePreview = null;
            upscaleAdaptiveDefaults = null;
            upscaleCandidateDimensions = null;
            upscaleProfileSliders.Clear();
            upscaleProfileValues.Clear();
            editedLocalCandidateIndexes.Clear();
            musicBrainzBackAvailable = false;
            if (backToMusicBrainz != null) { backToMusicBrainz.Visible = false; backToMusicBrainz.Enabled = false; }
            UpdateHoverButton();
            UpdateSelectionControls();
        }

        private void DisposeCandidateCards()
        {
            foreach (Control control in candidateCards.Controls)
            {
                foreach (PictureBox picture in control.Controls.OfType<PictureBox>()) if (picture.Image != null) picture.Image.Dispose();
                control.Dispose();
            }
            candidateCards.Controls.Clear();
        }

        private void UpdateHoverButton()
        {
            if (enableHover == null) return;
            enableHover.Enabled = candidates.Count > 0;
            ThemeManager.StyleChoiceButton(enableHover, uiState.HoverEnabled, uiState.Theme);
        }

        private void CompleteAlbumEvent(Dictionary<string, object> payload, string context)
        {
            string albumPath = ReadString(payload, "album_path");
            AlbumInfo completed = albums.FirstOrDefault(album => SameAlbumPath(album.Path, albumPath));
            // The core reports the physical Windows path. A shared SQLite row
            // can intentionally retain the proper-Unicode indexed path while
            // the SMB directory exposes a legacy-decoded name. Events belong
            // to the one active Album process, so retain that indexed identity
            // when the two path spellings cannot compare equal.
            if (completed == null && activeLaunchAlbum != null)
                completed = activeLaunchAlbum;
            ConsumeLaunchAlbumSelection(completed);
            ClearCandidates();
            fallbackMode = false;
            fallbackReason = "";
            musicBrainzRetryAvailable = false;
            SetFallbackControls(false);
            candidateContext.Text = context;
            UpdateSelectionControls();
        }

        private void ConsumeLaunchAlbumSelection(AlbumInfo album)
        {
            if (album == null) return;
            bool persisted = (uiState.SelectedAlbumPaths ?? new List<string>()).Any(path => SameAlbumPath(path, album.Path));
            bool changed = album.Selected || album.BypassOverride || persisted;
            album.Selected = false;
            album.BypassOverride = false;
            if (!changed) return;
            BuildTree();
            UpdateSelectionControls();
            SaveUiState();
        }

        private static bool SameAlbumPath(string first, string second)
        {
            if (String.IsNullOrWhiteSpace(first) || String.IsNullOrWhiteSpace(second)) return false;
            return NormalizeAlbumPath(first).Equals(NormalizeAlbumPath(second), StringComparison.OrdinalIgnoreCase);
        }

        private static string NormalizeAlbumPath(string path)
        {
            string normalized = (path ?? "").Trim().Replace('/', '\\');
            while (normalized.Length > 3 && normalized.EndsWith("\\", StringComparison.Ordinal))
                normalized = normalized.Substring(0, normalized.Length - 1);
            try { normalized = Path.GetFullPath(normalized); }
            catch { }
            return normalized.Normalize(NormalizationForm.FormC);
        }

        private static string ResolveExistingAlbumPath(string path)
        {
            if (String.IsNullOrWhiteSpace(path) || Directory.Exists(path)) return path;
            try
            {
                string full = Path.GetFullPath(path);
                string root = Path.GetPathRoot(full);
                if (String.IsNullOrWhiteSpace(root) || !Directory.Exists(root)) return path;
                string current = root;
                string relative = full.Substring(root.Length);
                foreach (string component in relative.Split(new[] { '\\', '/' }, StringSplitOptions.RemoveEmptyEntries))
                {
                    string candidate = Path.Combine(current, component);
                    if (Directory.Exists(candidate))
                    {
                        current = candidate;
                        continue;
                    }
                    List<string> equivalents = Directory.EnumerateDirectories(current)
                        .Where(entry => EquivalentWindowsFolderName(Path.GetFileName(entry), component))
                        .OrderBy(entry => entry, StringComparer.OrdinalIgnoreCase)
                        .Take(2)
                        .ToList();
                    if (equivalents.Count != 1) return path;
                    current = equivalents[0];
                }
                return Directory.Exists(current) ? current : path;
            }
            catch
            {
                return path;
            }
        }

        private static bool EquivalentWindowsFolderName(string physicalName, string indexedName)
        {
            string physical = (physicalName ?? "").Normalize(NormalizationForm.FormC);
            string indexed = (indexedName ?? "").Normalize(NormalizationForm.FormC);
            if (physical.Equals(indexed, StringComparison.OrdinalIgnoreCase)) return true;
            return TranslateLegacyWindowsName(physical).Normalize(NormalizationForm.FormC)
                    .Equals(indexed, StringComparison.OrdinalIgnoreCase)
                || TranslateLegacyWindowsName(indexed).Normalize(NormalizationForm.FormC)
                    .Equals(physical, StringComparison.OrdinalIgnoreCase);
        }

        private static string TranslateLegacyWindowsName(string value)
        {
            string current = value ?? "";
            Encoding legacy = Encoding.GetEncoding(
                1252,
                EncoderFallback.ExceptionFallback,
                DecoderFallback.ExceptionFallback);
            Encoding utf8 = new UTF8Encoding(false, true);
            for (int pass = 0; pass < 3; pass++)
            {
                try
                {
                    string repaired = utf8.GetString(legacy.GetBytes(current));
                    if (repaired == current) break;
                    current = repaired;
                }
                catch (EncoderFallbackException) { break; }
                catch (DecoderFallbackException) { break; }
            }
            return current;
        }

        private async void UseSelectedClicked(object sender, EventArgs e)
        {
            if (selectedCandidateIndex < 0) return;
            if (standaloneExistingCoverEdit && currentProcess == null)
            {
                await SaveStandaloneExistingCoverAsync();
                return;
            }
            if (currentProcess == null) return;
            ConfirmAndUseCandidate(selectedCandidateIndex);
        }

        private async Task SaveStandaloneExistingCoverAsync()
        {
            CandidateView candidate;
            if (!standaloneExistingCoverEdit || selectedCandidateIndex < 0
                || !candidates.TryGetValue(selectedCandidateIndex, out candidate)
                || !candidate.IsLocal || String.IsNullOrWhiteSpace(candidate.CachePath)) return;
            if (!state.Mode.Equals("write", StringComparison.OrdinalIgnoreCase))
            {
                MessageBox.Show(this,
                    "Existing-cover preview is available in Read mode, but saving requires Write mode.",
                    "SPLINED existing cover", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            if (uiState.ShowConfirmations)
            {
                DialogResult answer = MessageBox.Show(this,
                    "Apply the current Upscale / Advanced profile directly to this existing cover?\r\n\r\n"
                    + candidate.CachePath,
                    "Save existing cover edit", MessageBoxButtons.YesNo, MessageBoxIcon.Question,
                    MessageBoxDefaultButton.Button2);
                if (answer != DialogResult.Yes) return;
            }
            string core = FindCoreExecutable();
            if (core == null)
            {
                MessageBox.Show(this, "The SPLINED processing core was not found beside the GUI.",
                    "SPLINED core missing", MessageBoxButtons.OK, MessageBoxIcon.Error);
                return;
            }

            useSelected.Enabled = false;
            progress.Visible = true;
            progress.Style = ProgressBarStyle.Marquee;
            SetStatus("Saving existing cover edit...");
            string output = "";
            string error = "";
            int exitCode = -1;
            try
            {
                ConfigState editState = state.Clone();
                editState.Mode = "write";
                ProcessStartInfo start = new ProcessStartInfo
                {
                    FileName = core,
                    Arguments = "--edit-existing-cover " + QuoteArgument(candidate.CachePath),
                    WorkingDirectory = ConfigStore.AppRoot,
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    WindowStyle = ProcessWindowStyle.Hidden,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    StandardOutputEncoding = new UTF8Encoding(false),
                    StandardErrorEncoding = new UTF8Encoding(false)
                };
                start.EnvironmentVariables["SPLINED_CONFIG_TOML"] = ConfigStore.ExportConfigText(editState);
                await Task.Run(delegate
                {
                    using (Process process = Process.Start(start))
                    {
                        output = process.StandardOutput.ReadToEnd();
                        error = process.StandardError.ReadToEnd();
                        process.WaitForExit();
                        exitCode = process.ExitCode;
                    }
                });
                RuntimeLog.Write(exitCode == 0 ? "info" : "error",
                    "existing_cover.edit exit=" + exitCode + " path=" + candidate.CachePath
                    + " stderr=" + (String.IsNullOrWhiteSpace(error) ? "none" : error.Trim()));
                if (exitCode != 0)
                    throw new InvalidOperationException(String.IsNullOrWhiteSpace(error)
                        ? "The SPLINED core did not save the existing cover edit."
                        : error.Trim());

                InvalidateCandidateImage(candidate.CachePath);
                using (Image updated = LoadImageCopy(candidate.CachePath))
                {
                    if (updated != null && displayedAlbum != null)
                    {
                        displayedAlbum.CoverWidth = updated.Width;
                        displayedAlbum.CoverHeight = updated.Height;
                    }
                }
                editedLocalCandidateIndexes.Clear();
                candidatePreviewActive = false;
                RenderDisplayedAlbum();
                ShowStandaloneExistingCoverEditor(displayedAlbum);
                SetStatus("Existing cover edit saved without running provider discovery.");
                if (!String.IsNullOrWhiteSpace(output))
                    RuntimeLog.Write("debug", "existing_cover.edit.result " + output.Trim());
            }
            catch (Exception saveError)
            {
                RuntimeLog.Write("error", "existing_cover.edit.failed path=" + candidate.CachePath
                    + " error=" + saveError.Message);
                MessageBox.Show(this, "Unable to save the existing cover edit.\r\n\r\n" + saveError.Message,
                    "SPLINED existing cover", MessageBoxButtons.OK, MessageBoxIcon.Error);
                SetStatus("Existing cover edit was not saved.");
            }
            finally
            {
                progress.Visible = false;
                progress.Style = ProgressBarStyle.Blocks;
                UpdateCandidateActions();
            }
        }

        private void KeepLocalClicked(object sender, EventArgs e)
        {
            if (!awaitingDecision || currentProcess == null) return;
            CandidateView local = candidates.Values.FirstOrDefault(candidate =>
                candidate.Source.Equals("local", StringComparison.OrdinalIgnoreCase) ||
                candidate.Source.Equals("webpstill", StringComparison.OrdinalIgnoreCase));
            if (local == null) return;
            ConfirmAndUseCandidate(local.Index);
        }

        private void RefineFallbackClicked(object sender, EventArgs e)
        {
            if (!fallbackMode || !awaitingDecision || currentProcess == null) return;
            using (FallbackSearchForm form = new FallbackSearchForm(fallbackArtist, fallbackAlbum, uiState.Theme))
            {
                if (form.ShowDialog(this) != DialogResult.OK) return;
                fallbackRetryArtist = form.Artist;
                fallbackRetryAlbum = form.Album;
                fallbackRetryRequested = true;
                Dictionary<string, object> command = new Dictionary<string, object>();
                command["action"] = "retry";
                command["artist"] = fallbackRetryArtist;
                command["album"] = fallbackRetryAlbum;
                SendDecision(json.Serialize(command));
                candidateContext.Text = "Retrying fallback with edited Artist / Album values...";
                ClearCandidates();
                SetFallbackControls(false);
            }
        }

        private void RetryMusicBrainzClicked(object sender, EventArgs e)
        {
            if (!musicBrainzRetryAvailable || !awaitingDecision || currentProcess == null) return;
            SendDecision("{\"action\":\"retry_musicbrainz\"}");
            candidateContext.Text = "Loading MusicBrainz Matches...";
            retryMusicBrainz.Enabled = false;
        }

        private void BackToMusicBrainzClicked(object sender, EventArgs e)
        {
            if (!musicBrainzBackAvailable || !awaitingDecision || currentProcess == null) return;
            SendDecision("{\"action\":\"back_musicbrainz\"}");
            candidateContext.Text = "Returning to cached MusicBrainz Matches...";
            ClearCandidates();
        }

        private void SkipClicked(object sender, EventArgs e)
        {
            if (currentProcess == null) return;
            SendDecision("{\"action\":\"bypass\"}");
            candidateContext.Text = "Album skipped. Moving to the next selected album...";
            ClearCandidates();
        }

        private void SendDecision(string command)
        {
            try { currentProcess.StandardInput.WriteLine(command); currentProcess.StandardInput.Flush(); }
            catch (Exception error) { AppendActivity("  Unable to send decision: " + error.Message + "\r\n"); }
        }

        private void CompareClicked(object sender, EventArgs e)
        {
            if (candidates.Count == 0) return;
            CloseHoverPreview();
            List<CandidateView> selected = new List<CandidateView>();
            if (candidates.ContainsKey(recommendedCandidateIndex)) selected.Add(candidates[recommendedCandidateIndex]);
            foreach (int index in compareCandidateIndexes.OrderBy(value => value))
                if (candidates.ContainsKey(index) && index != recommendedCandidateIndex) selected.Add(candidates[index]);
            if (selected.Count < 2)
                foreach (CandidateView candidate in candidates.Values.OrderBy(value => value.Index))
                    if (!selected.Contains(candidate)) selected.Add(candidate);
            using (CandidateCompareForm form = new CandidateCompareForm(selected, uiState.Theme, uiState, state.Mode, awaitingDecision))
            {
                if (form.ShowDialog(this) == DialogResult.OK && form.SelectedCandidateIndex > 0)
                    ConfirmAndUseCandidate(form.SelectedCandidateIndex, false);
            }
        }

        private void ConfirmAndUseCandidate(int index, bool confirm = true)
        {
            if (!awaitingDecision || currentProcess == null || !candidates.ContainsKey(index)) return;
            CandidateView candidate = candidates[index];
            if (confirm && uiState.ShowConfirmations)
            {
                bool keepingLocal = candidate.Source.Equals("local", StringComparison.OrdinalIgnoreCase)
                    || candidate.Source.Equals("webpstill", StringComparison.OrdinalIgnoreCase);
                bool editingLocal = keepingLocal && editedLocalCandidateIndexes.Contains(candidate.Index);
                string effect = editingLocal
                    ? state.Mode.Equals("write", StringComparison.OrdinalIgnoreCase)
                        ? "Apply the previewed editing profile directly to this existing cover?"
                        : "Evaluate the previewed existing-cover edit in Read mode without changing the Album?"
                    : keepingLocal
                    ? "Keep the existing local artwork and continue without replacing it?"
                    : state.Mode.Equals("write", StringComparison.OrdinalIgnoreCase)
                        ? "Write this artwork using the active Config v5 output rules?"
                        : "Use this artwork for the Read-mode review sample? Album artwork will not be changed.";
                DialogResult answer = MessageBox.Show(this,
                    candidate.DisplaySource + "  " + candidate.Resolution + "\r\n\r\n" + effect,
                    "Confirm artwork selection", MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2);
                if (answer != DialogResult.Yes) return;
            }
            Dictionary<string, object> command = new Dictionary<string, object>();
            command["action"] = "use";
            command["index"] = index;
            command["upscale_adaptive_defaults"] = state.UpscaleAdaptiveDefaults;
            command["upscale_picture_percent"] = state.UpscalePicturePercent;
            command["upscale_sharpen_percent"] = state.UpscaleSharpenPercent;
            command["upscale_softness_percent"] = state.UpscaleSoftnessPercent;
            command["upscale_contrast_percent"] = state.UpscaleContrastPercent;
            command["upscale_exposure_percent"] = state.UpscaleExposurePercent;
            command["upscale_brightness_percent"] = state.UpscaleBrightnessPercent;
            command["upscale_gamma_percent"] = state.UpscaleGammaPercent;
            command["upscale_color_temperature"] = state.UpscaleColorTemperature;
            command["edit_existing_cover"] = candidate.IsLocal && editedLocalCandidateIndexes.Contains(candidate.Index);
            SendDecision(json.Serialize(command));
            candidateContext.Text = "Artwork selection confirmed. Moving to the next selected album...";
            ClearCandidates();
        }

        private void ShowMusicBrainzMatchesWorkspace(string artist, string title, string albumArtist, string albumTitle, object[] items, bool compilationTrack)
        {
            ApplyCandidateFilterWorkspace(false);
            CloseMusicBrainzMatchesWorkspace(false);
            if (activityContentHost == null) return;
            musicBrainzMatchesPanel = new MusicBrainzMatchesPanel(artist, title, albumArtist, albumTitle, items, compilationTrack, uiState.Theme);
            musicBrainzMatchesPanel.UseRequested += delegate(object sender, MusicBrainzMatchEventArgs args)
            {
                Dictionary<string, object> command = new Dictionary<string, object>();
                command["action"] = "use_musicbrainz_match";
                command["index"] = args.Index;
                CloseMusicBrainzMatchesWorkspace();
                SendDecision(json.Serialize(command));
            };
            musicBrainzMatchesPanel.SearchRequested += delegate
            {
                CloseMusicBrainzMatchesWorkspace();
                SendDecision("{\"action\":\"search_musicbrainz\"}");
            };
            musicBrainzMatchesPanel.LeaveRequested += delegate
            {
                CloseMusicBrainzMatchesWorkspace();
                SendDecision("{\"action\":\"leave_unchanged\"}");
            };
            musicBrainzMatchesPanel.AuthorityEditRequested += delegate(object sender, MusicBrainzAuthorityEventArgs args)
            {
                Dictionary<string, object> command = new Dictionary<string, object>();
                command["action"] = "edit_musicbrainz_authority";
                command["recording_mbid"] = args.RecordingMbid;
                command["artist_mbids"] = args.ArtistMbids;
                command["release_mbid"] = args.ReleaseMbid;
                CloseMusicBrainzMatchesWorkspace();
                SendDecision(json.Serialize(command));
            };
            musicBrainzMatchesPanel.ArtworkPreviewRequested += delegate(object sender, MusicBrainzMatchEventArgs args)
            {
                PreviewMusicBrainzArtworkAsync(args.Item, args.Index);
            };
            musicBrainzMatchesPanel.ArtworkPreviewEnded += delegate { EndMusicBrainzArtworkPreview(); };
            activityColumn.Visible = false;
            activityContentHost.Controls.Add(musicBrainzMatchesPanel);
            musicBrainzMatchesPanel.BringToFront();
            if (scanActivityTitle != null) scanActivityTitle.Text = "MusicBrainz Matches";
            ApplyArtworkPanelVisibility();
            UpdateArtworkSquareLayout(false);
        }

        private void CloseMusicBrainzMatchesWorkspace(bool restoreCandidateFilter = true)
        {
            musicBrainzPreviewVersion++;
            MusicBrainzMatchesPanel panel = musicBrainzMatchesPanel;
            musicBrainzMatchesPanel = null;
            if (panel != null)
            {
                activityContentHost.Controls.Remove(panel);
                panel.Dispose();
                candidatePreviewActive = false;
            }
            if (restoreCandidateFilter && uiState.CandidateFilterExpanded && candidates.Count > 0)
                ApplyCandidateFilterWorkspace(true);
            else
            {
                if (activityColumn != null) activityColumn.Visible = true;
                if (scanActivityTitle != null) scanActivityTitle.Text = "Scan Activity and Decisions";
            }
            ApplyArtworkPanelVisibility();
            if (panel != null && !FocusSelectedCandidatePreview()) RenderDisplayedAlbum();
        }

        private async void PreviewMusicBrainzArtworkAsync(Dictionary<string, object> item, int index)
        {
            MusicBrainzMatchesPanel owner = musicBrainzMatchesPanel;
            if (owner == null || item == null) return;
            string[] urls = MusicBrainzMatchesPanel.ArtworkPreviewUrls(item);
            if (urls.Length == 0) return;
            int version = ++musicBrainzPreviewVersion;
            candidatePreviewActive = true;
            artworkPreviewTitle.Text = "MusicBrainz Artwork Preview";
            string release = ReadString(item, "release_title");
            artworkPreviewCaption.Text = "Loading " + (String.IsNullOrWhiteSpace(release) ? "release artwork" : release) + "...";
            ApplyArtworkPanelVisibility();
            await Task.Delay(140);
            if (version != musicBrainzPreviewVersion || owner != musicBrainzMatchesPanel) return;
            await musicBrainzPreviewGate.WaitAsync();
            try
            {
                if (version != musicBrainzPreviewVersion || owner != musicBrainzMatchesPanel) return;
                Exception lastError = null;
                foreach (string url in urls)
                {
                    try
                    {
                        Stopwatch timer = Stopwatch.StartNew();
                        byte[] bytes;
                        if (!musicBrainzPreviewCache.TryGetValue(url, out bytes))
                        {
                            bytes = await DownloadArtworkPreviewAsync(url);
                            RememberMusicBrainzPreview(url, bytes);
                        }
                        if (version != musicBrainzPreviewVersion || owner != musicBrainzMatchesPanel) return;
                        Image replacement = ImageFromBytes(bytes);
                        if (replacement == null) throw new InvalidOperationException("The artwork response is not a supported image.");
                        string resolution = replacement.Width + "x" + replacement.Height;
                        Image previous = artworkPreviewImage.Image;
                        artworkPreviewImage.Image = replacement;
                        if (previous != null) previous.Dispose();
                        artworkPreviewTitle.Text = "MusicBrainz Artwork Preview";
                        artworkPreviewCaption.Text = (String.IsNullOrWhiteSpace(release) ? "MusicBrainz release" : release)
                            + " · " + resolution;
                        timer.Stop();
                        RuntimeLog.Write("debug", "musicbrainz.artwork_preview success index=" + index
                            + " resolution=" + resolution + " elapsed_ms=" + timer.ElapsedMilliseconds);
                        return;
                    }
                    catch (Exception error)
                    {
                        lastError = error;
                    }
                }
                if (version != musicBrainzPreviewVersion || owner != musicBrainzMatchesPanel) return;
                artworkPreviewTitle.Text = "MusicBrainz Artwork Preview";
                artworkPreviewCaption.Text = "Artwork unavailable for "
                    + (String.IsNullOrWhiteSpace(release) ? "this release" : release);
                RuntimeLog.Write("warning", "musicbrainz.artwork_preview unavailable index=" + index
                    + " error=" + (lastError == null ? "no artwork endpoint succeeded" : lastError.Message));
            }
            finally
            {
                musicBrainzPreviewGate.Release();
            }
        }

        private void EndMusicBrainzArtworkPreview()
        {
            musicBrainzPreviewVersion++;
            candidatePreviewActive = false;
            RenderDisplayedAlbum();
        }

        private static Task<byte[]> DownloadArtworkPreviewAsync(string url)
        {
            return Task.Run(delegate
            {
                HttpWebRequest request = (HttpWebRequest)WebRequest.Create(url);
                request.AllowAutoRedirect = true;
                request.Timeout = 7000;
                request.ReadWriteTimeout = 7000;
                request.UserAgent = "SPLINED-Windows/1.0";
                using (HttpWebResponse response = (HttpWebResponse)request.GetResponse())
                using (Stream source = response.GetResponseStream())
                using (MemoryStream destination = new MemoryStream())
                {
                    if (response.ContentLength > 20 * 1024 * 1024)
                        throw new InvalidOperationException("Artwork preview exceeds the 20 MB safety limit.");
                    byte[] buffer = new byte[81920];
                    int read;
                    while ((read = source.Read(buffer, 0, buffer.Length)) > 0)
                    {
                        destination.Write(buffer, 0, read);
                        if (destination.Length > 20 * 1024 * 1024)
                            throw new InvalidOperationException("Artwork preview exceeds the 20 MB safety limit.");
                    }
                    return destination.ToArray();
                }
            });
        }

        private void RememberMusicBrainzPreview(string url, byte[] bytes)
        {
            if (String.IsNullOrWhiteSpace(url) || bytes == null || bytes.Length == 0) return;
            if (!musicBrainzPreviewCache.ContainsKey(url)) musicBrainzPreviewCacheOrder.Enqueue(url);
            musicBrainzPreviewCache[url] = bytes;
            while (musicBrainzPreviewCacheOrder.Count > 12)
            {
                string oldest = musicBrainzPreviewCacheOrder.Dequeue();
                musicBrainzPreviewCache.Remove(oldest);
            }
        }

        private static Image ImageFromBytes(byte[] bytes)
        {
            try
            {
                using (MemoryStream stream = new MemoryStream(bytes))
                using (Image source = Image.FromStream(stream)) return new Bitmap(source);
            }
            catch { return null; }
        }

        private void ShowHoverPreview(CandidateView candidate)
        {
            CloseHoverPreview();
            if (uiState.ShowArtwork)
            {
                ShowArtworkCandidate(candidate);
                return;
            }
            hoverPreview = new HoverPreviewForm(candidate, uiState.Theme, uiState);
            hoverPreview.FormClosed += delegate { hoverPreview = null; };
            hoverPreview.Show(this);
        }

        private void CloseHoverPreview()
        {
            if (hoverPreview != null && !hoverPreview.IsDisposed) hoverPreview.Close();
            hoverPreview = null;
            if (candidatePreviewActive)
            {
                candidatePreviewActive = false;
                if (!FocusSelectedCandidatePreview()) RenderDisplayedAlbum();
            }
        }

        private bool FocusSelectedCandidatePreview()
        {
            if (musicBrainzMatchesPanel != null || selectedCandidateIndex < 0) return false;
            CandidateView candidate;
            if (!candidates.TryGetValue(selectedCandidateIndex, out candidate)
                || String.IsNullOrWhiteSpace(candidate.CachePath)
                || !File.Exists(candidate.CachePath)) return false;
            candidatePreviewActive = true;
            artworkPreviewTitle.Text = candidate.IsLocal ? "Existing Cover Editing" : "Selected Candidate Artwork";
            SetArtworkPreview(candidate.CachePath,
                candidate.DisplaySource + " · " + candidate.Resolution + " · " + DisplayRange(candidate.SourceRangeKey));
            ApplyArtworkPanelVisibility();
            return true;
        }

        private void RefreshSelectedCandidatePreview()
        {
            if (FocusSelectedCandidatePreview()) return;
            candidatePreviewActive = false;
            ApplyArtworkPanelVisibility();
        }

        private void ShowArtworkCandidate(CandidateView candidate)
        {
            if (candidate == null || !uiState.ShowArtwork) return;
            candidatePreviewActive = true;
            artworkPreviewTitle.Text = "Candidate Artwork Preview";
            SetArtworkPreview(candidate.CachePath,
                candidate.DisplaySource + " · " + candidate.Resolution + " · " + candidate.Range);
        }

        private void UpscalePreviewClicked(object sender, EventArgs e)
        {
            CandidateView candidate;
            if (selectedCandidateIndex >= 0 && candidates.TryGetValue(selectedCandidateIndex, out candidate) && candidate.IsLocal)
                editedLocalCandidateIndexes.Add(candidate.Index);
            RenderUpscalePreview();
        }

        private void RenderUpscalePreview()
        {
            CandidateView candidate;
            if (selectedCandidateIndex < 0
                || !candidates.TryGetValue(selectedCandidateIndex, out candidate)
                || !CandidateCanEdit(candidate)
                || String.IsNullOrWhiteSpace(candidate.CachePath)
                || !File.Exists(candidate.CachePath)) return;

            using (Image source = LoadImageCopy(candidate.CachePath))
            {
                if (source == null) return;
                int targetWidth = candidate.ProjectedWidth > 0 ? candidate.ProjectedWidth : state.RangeIdeal;
                int targetHeight = candidate.ProjectedHeight > 0 ? candidate.ProjectedHeight : state.RangeIdeal;
                ToneCorrection correction = BuildUpscaleCorrection(source);
                Bitmap resized = new Bitmap(targetWidth, targetHeight, PixelFormat.Format32bppArgb);
                resized.SetResolution(source.HorizontalResolution > 0 ? source.HorizontalResolution : 96,
                    source.VerticalResolution > 0 ? source.VerticalResolution : 96);
                using (Graphics graphics = Graphics.FromImage(resized))
                {
                    graphics.CompositingMode = CompositingMode.SourceCopy;
                    graphics.CompositingQuality = CompositingQuality.HighQuality;
                    graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                    graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                    graphics.SmoothingMode = SmoothingMode.HighQuality;
                    graphics.DrawImage(source, new Rectangle(0, 0, targetWidth, targetHeight));
                }
                Bitmap projected = ApplyToneCorrection(resized, correction);
                resized.Dispose();
                if (!uiState.ShowArtwork)
                {
                    uiState.ShowArtwork = true;
                    ApplyArtworkPanelVisibility();
                    SaveUiState();
                }
                candidatePreviewActive = true;
                artworkPreviewTitle.Text = candidate.IsLocal ? "Existing Cover Edit Preview" : "Upscale Preview";
                SetArtworkPreviewImage(projected, candidate.DisplaySource + " · "
                    + candidate.Resolution + " → " + targetWidth + " x " + targetHeight
                    + " · picture " + correction.PicturePercent.ToString("+0;-0;0") + "%"
                    + " · brightness " + correction.BrightnessPercent.ToString("+0;-0;0") + "%"
                    + " · contrast " + correction.ContrastPercent.ToString("+0;-0;0") + "%"
                    + " · exposure " + correction.ExposurePercent.ToString("+0;-0;0") + "%"
                    + " · sharpen " + correction.SharpenPercent + "%"
                    + " · softness " + correction.SoftnessPercent + "%"
                    + " · gamma " + correction.GammaPercent.ToString("+0;-0;0") + "%"
                    + " · color " + UpscaleProfileDisplay("temperature", correction.TemperatureValue)
                    + " · preview only");
            }
        }

        private ToneCorrection BuildUpscaleCorrection(Image source)
        {
            ToneCorrection automatic = state.UpscaleAdaptiveDefaults
                ? AnalyzeToneCorrection(source)
                : new ToneCorrection(0, 0);
            return new ToneCorrection(
                automatic.Brightness + state.UpscaleBrightnessPercent / 100f,
                automatic.Contrast + state.UpscaleContrastPercent / 100f,
                state.UpscaleExposurePercent / 100f,
                state.UpscaleSharpenPercent / 100f,
                state.UpscaleColorTemperature / 100f,
                state.UpscalePicturePercent / 100f,
                state.UpscaleSoftnessPercent / 100f,
                state.UpscaleGammaPercent / 100f);
        }

        private static ToneCorrection AnalyzeToneCorrection(Image source)
        {
            const int sampleSize = 256;
            using (Bitmap sample = new Bitmap(sampleSize, sampleSize, PixelFormat.Format24bppRgb))
            {
                using (Graphics graphics = Graphics.FromImage(sample))
                {
                    graphics.CompositingQuality = CompositingQuality.HighQuality;
                    graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                    graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                    graphics.DrawImage(source, new Rectangle(0, 0, sampleSize, sampleSize));
                }
                double sum = 0;
                double sumSquares = 0;
                int shadows = 0;
                int highlights = 0;
                int count = sampleSize * sampleSize;
                for (int y = 0; y < sampleSize; y++)
                {
                    for (int x = 0; x < sampleSize; x++)
                    {
                        Color pixel = sample.GetPixel(x, y);
                        double luminance = (0.2126 * pixel.R + 0.7152 * pixel.G + 0.0722 * pixel.B) / 255.0;
                        sum += luminance;
                        sumSquares += luminance * luminance;
                        if (luminance <= 0.02) shadows++;
                        if (luminance >= 0.98) highlights++;
                    }
                }
                double mean = sum / count;
                double deviation = Math.Sqrt(Math.Max(0, sumSquares / count - mean * mean));
                double shadowRatio = shadows / (double)count;
                double highlightRatio = highlights / (double)count;
                double brightness = mean < 0.24 && shadowRatio < 0.70
                    ? Math.Min(0.05, (0.24 - mean) * 0.5)
                    : mean > 0.76 && highlightRatio < 0.70
                        ? -Math.Min(0.05, (mean - 0.76) * 0.5)
                        : 0;
                double contrast = deviation < 0.16 && shadowRatio < 0.20 && highlightRatio < 0.20
                    ? Math.Min(0.08, (0.16 - deviation) * 0.8)
                    : 0;
                return new ToneCorrection((float)brightness, (float)contrast);
            }
        }

        private static Bitmap ApplyToneCorrection(Bitmap resized, ToneCorrection correction)
        {
            if (Math.Abs(correction.Brightness) < 0.0001f && Math.Abs(correction.Contrast) < 0.0001f
                && Math.Abs(correction.Exposure) < 0.0001f && Math.Abs(correction.Temperature) < 0.0001f
                && Math.Abs(correction.Picture) < 0.0001f && Math.Abs(correction.Sharpen) < 0.0001f
                && Math.Abs(correction.Softness) < 0.0001f && Math.Abs(correction.Gamma) < 0.0001f)
                return new Bitmap(resized);
            Bitmap adjusted = new Bitmap(resized.Width, resized.Height, PixelFormat.Format32bppArgb);
            adjusted.SetResolution(resized.HorizontalResolution, resized.VerticalResolution);
            float contrastScale = 1f + correction.Contrast;
            float exposureScale = 1f + correction.Exposure;
            float translation = (0.5f * (1f - contrastScale) + correction.Brightness) * exposureScale;
            float redScale = contrastScale * exposureScale * (1f + correction.Temperature * 0.10f);
            float greenScale = contrastScale * exposureScale;
            float blueScale = contrastScale * exposureScale * (1f - correction.Temperature * 0.10f);
            ColorMatrix matrix = new ColorMatrix(new[]
            {
                new[] { redScale, 0f, 0f, 0f, 0f },
                new[] { 0f, greenScale, 0f, 0f, 0f },
                new[] { 0f, 0f, blueScale, 0f, 0f },
                new[] { 0f, 0f, 0f, 1f, 0f },
                new[] { translation, translation, translation, 0f, 1f }
            });
            using (ImageAttributes attributes = new ImageAttributes())
            using (Graphics graphics = Graphics.FromImage(adjusted))
            {
                attributes.SetColorMatrix(matrix, ColorMatrixFlag.Default, ColorAdjustType.Bitmap);
                graphics.CompositingMode = CompositingMode.SourceCopy;
                graphics.DrawImage(resized, new Rectangle(0, 0, resized.Width, resized.Height),
                    0, 0, resized.Width, resized.Height, GraphicsUnit.Pixel, attributes);
            }
            Bitmap processed = adjusted;
            if (Math.Abs(correction.Picture) >= 0.0001f || Math.Abs(correction.Gamma) >= 0.0001f)
            {
                Bitmap replacement = ApplyPreviewPictureGamma(processed, correction.Picture, correction.Gamma);
                processed.Dispose();
                processed = replacement;
            }
            if (correction.Softness > 0.0001f)
            {
                Bitmap replacement = ApplyPreviewSoftness(processed, correction.Softness);
                processed.Dispose();
                processed = replacement;
            }
            if (correction.Sharpen > 0.0001f)
            {
                Bitmap replacement = ApplyPreviewSharpen(processed, correction.Sharpen);
                processed.Dispose();
                processed = replacement;
            }
            return processed;
        }

        private static Bitmap ApplyPreviewPictureGamma(Bitmap source, float picture, float gamma)
        {
            Bitmap output = new Bitmap(source);
            Rectangle bounds = new Rectangle(0, 0, output.Width, output.Height);
            BitmapData data = output.LockBits(bounds, ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
            int bytes = Math.Abs(data.Stride) * data.Height;
            byte[] pixels = new byte[bytes];
            Marshal.Copy(data.Scan0, pixels, 0, bytes);
            float saturation = 1f + picture;
            double exponent = gamma >= 0 ? 1.0 / (1.0 + gamma) : 1.0 - gamma;
            for (int y = 0; y < output.Height; y++)
            {
                for (int x = 0; x < output.Width; x++)
                {
                    int offset = y * data.Stride + x * 4;
                    double blue = pixels[offset] / 255.0;
                    double green = pixels[offset + 1] / 255.0;
                    double red = pixels[offset + 2] / 255.0;
                    double luminance = 0.0722 * blue + 0.7152 * green + 0.2126 * red;
                    pixels[offset] = CorrectPreviewChannel(blue, luminance, saturation, exponent);
                    pixels[offset + 1] = CorrectPreviewChannel(green, luminance, saturation, exponent);
                    pixels[offset + 2] = CorrectPreviewChannel(red, luminance, saturation, exponent);
                }
            }
            Marshal.Copy(pixels, 0, data.Scan0, bytes);
            output.UnlockBits(data);
            return output;
        }

        private static byte CorrectPreviewChannel(double value, double luminance, float saturation, double exponent)
        {
            double saturated = Math.Max(0, Math.Min(1, luminance + (value - luminance) * saturation));
            return (byte)Math.Max(0, Math.Min(255, (int)Math.Round(Math.Pow(saturated, exponent) * 255)));
        }

        private static Bitmap ApplyPreviewSoftness(Bitmap source, float amount)
        {
            Bitmap input = new Bitmap(source);
            Bitmap output = new Bitmap(input.Width, input.Height, PixelFormat.Format32bppArgb);
            output.SetResolution(input.HorizontalResolution, input.VerticalResolution);
            Rectangle bounds = new Rectangle(0, 0, input.Width, input.Height);
            BitmapData sourceData = input.LockBits(bounds, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            BitmapData outputData = output.LockBits(bounds, ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
            int bytes = Math.Abs(sourceData.Stride) * sourceData.Height;
            byte[] original = new byte[bytes];
            byte[] softened = new byte[bytes];
            Marshal.Copy(sourceData.Scan0, original, 0, bytes);
            Buffer.BlockCopy(original, 0, softened, 0, bytes);
            float blend = Math.Max(0f, Math.Min(0.20f, amount));
            for (int y = 1; y < input.Height - 1; y++)
            {
                for (int x = 1; x < input.Width - 1; x++)
                {
                    int offset = y * sourceData.Stride + x * 4;
                    for (int channel = 0; channel < 3; channel++)
                    {
                        float average = (original[offset - 4 + channel] + original[offset + 4 + channel]
                            + original[offset - sourceData.Stride + channel] + original[offset + sourceData.Stride + channel]) / 4f;
                        softened[offset + channel] = (byte)Math.Max(0, Math.Min(255,
                            (int)Math.Round(original[offset + channel] * (1f - blend) + average * blend)));
                    }
                }
            }
            Marshal.Copy(softened, 0, outputData.Scan0, bytes);
            input.UnlockBits(sourceData);
            output.UnlockBits(outputData);
            input.Dispose();
            return output;
        }

        private static Bitmap ApplyPreviewSharpen(Bitmap source, float amount)
        {
            Bitmap input = source.PixelFormat == PixelFormat.Format32bppArgb
                ? new Bitmap(source)
                : source.Clone(new Rectangle(0, 0, source.Width, source.Height), PixelFormat.Format32bppArgb);
            Bitmap output = new Bitmap(input.Width, input.Height, PixelFormat.Format32bppArgb);
            output.SetResolution(input.HorizontalResolution, input.VerticalResolution);
            Rectangle bounds = new Rectangle(0, 0, input.Width, input.Height);
            BitmapData sourceData = input.LockBits(bounds, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            BitmapData outputData = output.LockBits(bounds, ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
            int bytes = Math.Abs(sourceData.Stride) * sourceData.Height;
            byte[] original = new byte[bytes];
            byte[] sharpened = new byte[bytes];
            Marshal.Copy(sourceData.Scan0, original, 0, bytes);
            Buffer.BlockCopy(original, 0, sharpened, 0, bytes);
            float edge = Math.Max(0f, Math.Min(0.05f, amount * 0.25f));
            for (int y = 1; y < input.Height - 1; y++)
            {
                for (int x = 1; x < input.Width - 1; x++)
                {
                    int offset = y * sourceData.Stride + x * 4;
                    for (int channel = 0; channel < 3; channel++)
                    {
                        float value = original[offset + channel] * (1f + 4f * edge)
                            - edge * (original[offset - 4 + channel] + original[offset + 4 + channel]
                                + original[offset - sourceData.Stride + channel] + original[offset + sourceData.Stride + channel]);
                        sharpened[offset + channel] = (byte)Math.Max(0, Math.Min(255, (int)Math.Round(value)));
                    }
                }
            }
            Marshal.Copy(sharpened, 0, outputData.Scan0, bytes);
            input.UnlockBits(sourceData);
            output.UnlockBits(outputData);
            input.Dispose();
            return output;
        }

        private void ShowSelectedAlbum(AlbumInfo album)
        {
            displayedAlbum = running && activeLaunchAlbum != null ? activeLaunchAlbum : album;
            candidatePreviewActive = false;
            RenderDisplayedAlbum();
            if (!running)
                ShowStandaloneExistingCoverEditor(displayedAlbum);
        }

        private void ShowStandaloneExistingCoverEditor(AlbumInfo album)
        {
            ClearCandidates();
            if (album == null) return;
            string coverPath = album.CoverPath;
            if (String.IsNullOrWhiteSpace(coverPath) || !File.Exists(coverPath))
                coverPath = album.LocalArtworkFiles.FirstOrDefault(File.Exists) ?? "";
            if (String.IsNullOrWhiteSpace(coverPath)) return;

            int width = album.CoverWidth;
            int height = album.CoverHeight;
            if (width <= 0 || height <= 0)
            {
                using (Image image = LoadImageCopy(coverPath))
                {
                    if (image == null) return;
                    width = image.Width;
                    height = image.Height;
                }
            }
            int projectedWidth = width;
            int projectedHeight = height;
            int shortSide = Math.Min(width, height);
            bool withinUpscaleLimit = shortSide > 0
                && (long)state.RangeIdeal * 100L <= (long)shortSide * state.UpscaleMaxPercent;
            bool upscale = state.UpscaleBelowIdeal && shortSide < state.RangeIdeal && withinUpscaleLimit;
            if (upscale)
            {
                double scale = state.RangeIdeal / (double)shortSide;
                projectedWidth = Math.Max(1, (int)Math.Round(width * scale));
                projectedHeight = Math.Max(1, (int)Math.Round(height * scale));
            }

            CandidateView local = new CandidateView
            {
                Index = 1,
                Source = "local",
                PixelWidth = width,
                PixelHeight = height,
                Resolution = width + " x " + height,
                Format = String.IsNullOrWhiteSpace(album.CoverFormat) ? Path.GetExtension(coverPath).TrimStart('.') : album.CoverFormat,
                SourceRange = CandidateRangeKey(shortSide),
                Range = CandidateRangeKey(Math.Min(projectedWidth, projectedHeight)),
                ProjectedWidth = projectedWidth,
                ProjectedHeight = projectedHeight,
                Upscaled = upscale,
                UpscaleEligible = upscale,
                Approved = true,
                Square = width == height,
                Acceptable = true,
                PolicyStatus = "accept",
                PolicyReason = "Existing local artwork remains directly editable",
                StrictStatus = "validated-local",
                StrictPreferredEligible = true,
                StrictAutoEligible = true,
                CachePath = coverPath,
                LocalOrigin = "cover-file",
                LocalReference = Path.GetFileName(coverPath)
            };
            candidates[local.Index] = local;
            standaloneExistingCoverEdit = true;
            compareCandidateIndexes.Add(local.Index);
            selectedCandidateIndex = local.Index;
            RebuildCandidateFilterPanel();
            ApplyCandidateFilters();
            candidateContext.Text = "Existing cover ready for direct preview and editing — no provider scan required.";
            SetCandidateFilterExpanded(uiState.CandidateFilterExpanded, false);
        }

        private void RenderDisplayedAlbum()
        {
            if (selectedAlbumInfo == null) return;
            artworkPreviewTitle.Text = "Selected Album Artwork";
            if (displayedAlbum == null)
            {
                selectedAlbumInfo.Text = "";
                selectedAlbumInfo.ForeColor = ThemeManager.PaletteFor(uiState.Theme).TextPrimary;
                SetArtworkPreview("", "");
                return;
            }
            selectedAlbumInfo.ForeColor = AlbumStatePresentation.StateColor(displayedAlbum.State, ThemeManager.IsDark(uiState.Theme));
            string year = String.IsNullOrWhiteSpace(displayedAlbum.ReleaseYear) ? "—" : displayedAlbum.ReleaseYear;
            string coverName = String.IsNullOrWhiteSpace(displayedAlbum.CoverName)
                ? (displayedAlbum.LocalArtworkFiles.Count == 0 ? "No cover.* found" : Path.GetFileName(displayedAlbum.LocalArtworkFiles[0]))
                : displayedAlbum.CoverName;
            string resolution = displayedAlbum.CoverWidth > 0 && displayedAlbum.CoverHeight > 0
                ? displayedAlbum.CoverWidth + " x " + displayedAlbum.CoverHeight
                : "resolution unavailable";
            selectedAlbumInfo.Text = displayedAlbum.Artist + "  ·  " + displayedAlbum.Title + Environment.NewLine
                + "Year " + year + "  ·  Tracks " + displayedAlbum.TrackCount + "  ·  " + displayedAlbum.State + Environment.NewLine
                + "Artwork " + coverName + "  ·  " + resolution + "  ·  Root files " + displayedAlbum.RootFiles + "  ·  Cover files " + displayedAlbum.CoverFiles + Environment.NewLine
                + "Path " + displayedAlbum.Path;
            string coverPath = displayedAlbum.CoverPath;
            if (String.IsNullOrWhiteSpace(coverPath) || !File.Exists(coverPath))
                coverPath = displayedAlbum.LocalArtworkFiles.FirstOrDefault(File.Exists) ?? "";
            SetArtworkPreview(coverPath, coverName + " · " + resolution);
        }

        private void SetArtworkPreview(string path, string caption)
        {
            if (artworkPreviewImage == null) return;
            SetArtworkPreviewImage(LoadImageCopy(path), caption);
        }

        private void SetArtworkPreviewImage(Image replacement, string caption)
        {
            if (artworkPreviewImage == null)
            {
                if (replacement != null) replacement.Dispose();
                return;
            }
            Image previous = artworkPreviewImage.Image;
            artworkPreviewImage.Image = replacement;
            if (previous != null) previous.Dispose();
            artworkPreviewCaption.Text = String.IsNullOrWhiteSpace(caption) ? "No artwork selected" : caption;
        }

        private void DisposeArtworkPreviewImage()
        {
            if (artworkPreviewImage == null || artworkPreviewImage.Image == null) return;
            artworkPreviewImage.Image.Dispose();
            artworkPreviewImage.Image = null;
        }

        private void ApplyArtworkPanelVisibility()
        {
            if (activityWorkspace == null || activityWorkspace.ColumnStyles.Count < 2) return;
            // MusicBrainz review temporarily forces the shared Artwork panel
            // visible without changing the user's persisted View preference.
            bool workspaceReview = musicBrainzMatchesPanel != null || CandidateFilterWorkspaceVisible || candidatePreviewActive;
            bool visible = uiState.ShowArtwork || workspaceReview;
            if (artworkPreviewCard != null) artworkPreviewCard.Visible = visible;
            activityWorkspace.ColumnStyles[0].SizeType = SizeType.Percent;
            activityWorkspace.ColumnStyles[0].Width = 100;
            activityWorkspace.ColumnStyles[1].SizeType = SizeType.Absolute;
            activityWorkspace.ColumnStyles[1].Width = 0;
            if (showArtworkMenuItem != null) showArtworkMenuItem.Checked = visible;
            if (visible && musicBrainzMatchesPanel == null && !candidatePreviewActive) RenderDisplayedAlbum();
            else if (!visible) CloseHoverPreview();
            UpdateArtworkSquareLayout(false);
            activityWorkspace.PerformLayout();
        }

        private void UpdateArtworkSquareLayout(bool constrainSplitter)
        {
            if (adjustingArtworkLayout || activityWorkspace == null || activityWorkspace.ColumnStyles.Count < 2) return;
            if ((!uiState.ShowArtwork && musicBrainzMatchesPanel == null && !CandidateFilterWorkspaceVisible && !candidatePreviewActive)
                || activityWorkspace.ClientSize.Width <= 0 || activityWorkspace.ClientSize.Height <= 0)
            {
                activityWorkspace.ColumnStyles[1].SizeType = SizeType.Absolute;
                activityWorkspace.ColumnStyles[1].Width = 0;
                return;
            }
            adjustingArtworkLayout = true;
            try
            {
                int marginAllowance = ThemeManager.Space4;
                int required = activityWorkspace.ClientSize.Height + marginAllowance;
                int available = Math.Max(0, activityWorkspace.ClientSize.Width - AlbumActivityMinimumWidth);
                if (constrainSplitter && rightSplit != null && required > available)
                {
                    int correction = required - available;
                    int target = Math.Max(rightSplit.Panel1MinSize, rightSplit.SplitterDistance - correction);
                    if (target < rightSplit.SplitterDistance)
                    {
                        rightSplit.SplitterDistance = target;
                        required = Math.Max(0, activityWorkspace.ClientSize.Height + marginAllowance);
                        available = Math.Max(0, activityWorkspace.ClientSize.Width - AlbumActivityMinimumWidth);
                    }
                }
                activityWorkspace.ColumnStyles[0].SizeType = SizeType.Percent;
                activityWorkspace.ColumnStyles[0].Width = 100;
                activityWorkspace.ColumnStyles[1].SizeType = SizeType.Absolute;
                activityWorkspace.ColumnStyles[1].Width = Math.Max(0, Math.Min(required, available));
            }
            finally { adjustingArtworkLayout = false; }
        }

        private void StopRun()
        {
            stopRequested = true;
            SetStatus("Stopping current processing...");
            ClearCandidates();
            candidateContext.Text = "Processing stopped. Candidate results cleared.";
            Process stopping = currentProcess;
            if (stopping == null) return;
            RuntimeLog.Write("warning", "batch.stop_requested");
            try
            {
                if (!stopping.HasExited) stopping.StandardInput.Close();
            }
            catch { }
            Task.Run(async delegate
            {
                await Task.Delay(5000);
                try
                {
                    if (!stopping.HasExited)
                    {
                        RuntimeLog.Write("warning", "batch.stop_force_kill_after_grace");
                        stopping.Kill();
                    }
                }
                catch { }
            });
        }

        private async void OpenSettings(object sender, EventArgs e)
        {
            if (running || awaitingDecision || candidates.Count > 0)
            {
                MessageBox.Show(this,
                    "Settings are unavailable while an album search or artwork decision is active. Choose, skip, or stop the current album first.",
                    "Finish the current album", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            SaveUiState();
            try { state = ConfigStore.Load(); }
            catch (Exception error)
            {
                MessageBox.Show(this, error.Message, "Unable to load Config v5", MessageBoxButtons.OK, MessageBoxIcon.Error);
                return;
            }
            bool oldHistory = state.HistoryEnabled;
            int oldRetention = state.HistoryRetentionDays;
            DialogResult settingsResult;
            using (SetupForm setup = new SetupForm(state, false))
            {
                settingsResult = setup.ShowDialog(this);
                if (settingsResult == DialogResult.OK)
                    state = setup.SavedState != null ? setup.SavedState.Clone() : ConfigStore.Load();
            }
            uiState = ConfigStore.LoadUi();
            if (settingsResult != DialogResult.OK) return;
            if ((oldHistory && !state.HistoryEnabled) || (state.HistoryRetentionDays > 0 && (oldRetention == 0 || state.HistoryRetentionDays < oldRetention)))
                MessageBox.Show(this, "History was disabled or shortened. Status colors disappear wherever authoritative history is no longer available; those folders return to standard colors.", "History coloring changed", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            ApplyModeActivityAppearance();
            await ReloadLibraryAsync(false);
        }

        private void ThemeClicked(object sender, EventArgs e)
        {
            ToolStripMenuItem selected = (ToolStripMenuItem)sender;
            uiState.Theme = Convert.ToString(selected.Tag);
            SaveUiState();
            foreach (ToolStripMenuItem sibling in selected.Owner.Items.OfType<ToolStripMenuItem>()) sibling.Checked = sibling == selected;
            ApplyThemeAndMaybeRebuildTree(true);
        }

        private void LayoutPresetClicked(object sender, EventArgs e)
        {
            ToolStripMenuItem selected = sender as ToolStripMenuItem;
            if (selected == null) return;
            ApplyLayoutPreset(Convert.ToString(selected.Tag));
            foreach (ToolStripMenuItem sibling in selected.Owner.Items.OfType<ToolStripMenuItem>())
                sibling.Checked = sibling == selected;
            SaveUiState();
        }

        private void ApplyLayoutPreset(string preset)
        {
            if (mainSplit == null) return;
            applyingLayoutPreset = true;
            try
            {
                bool stacked = String.Equals(preset, "Stacked", StringComparison.OrdinalIgnoreCase);
                mainSplit.Orientation = stacked ? Orientation.Horizontal : Orientation.Vertical;
                mainSplit.Panel1MinSize = stacked ? 180 : 280;
                mainSplit.Panel2MinSize = stacked ? 250 : 420;
                ApplyMainSplitPadding();
                int extent = stacked ? mainSplit.Height : mainSplit.Width;
                double proportion = String.Equals(preset, "Wider Select Media", StringComparison.OrdinalIgnoreCase) ? 0.46
                    : String.Equals(preset, "Wider Decisions", StringComparison.OrdinalIgnoreCase) ? 0.27
                    : stacked ? 0.40 : 0.35;
                int maximum = Math.Max(mainSplit.Panel1MinSize, extent - mainSplit.Panel2MinSize - mainSplit.SplitterWidth);
                mainSplit.SplitterDistance = Math.Max(mainSplit.Panel1MinSize, Math.Min((int)Math.Round(extent * proportion), maximum));
                uiState.LayoutPreset = preset;
                uiState.LayoutStacked = stacked;
                uiState.MainSplitterDistance = mainSplit.SplitterDistance;
            }
            finally { applyingLayoutPreset = false; }
        }

        private void ApplyMainSplitPadding()
        {
            if (mainSplit == null) return;
            if (mainSplit.Orientation == Orientation.Horizontal)
            {
                mainSplit.Panel1.Padding = new Padding(ThemeManager.Space8, ThemeManager.Space8, ThemeManager.Space8, ThemeManager.Space4);
                mainSplit.Panel2.Padding = new Padding(ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space8);
            }
            else
            {
                mainSplit.Panel1.Padding = new Padding(ThemeManager.Space8, ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8);
                mainSplit.Panel2.Padding = new Padding(ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space8, ThemeManager.Space8);
            }
        }

        private async void CheckForUpdateClicked(object sender, EventArgs e)
        {
            await CheckForUpdateAsync(true);
        }

        private async Task CheckForUpdateAsync(bool interactive)
        {
            if (updateCheckRunning) return;
            updateCheckRunning = true;
            if (checkUpdateMenuItem != null) checkUpdateMenuItem.Enabled = false;
            try
            {
                string channel = WindowsUpdateService.UsesDevChannel ? "dev" : "stable";
                if (interactive) SetStatus("Checking for Windows " + channel + " updates...");
                WindowsUpdateCheck update = await WindowsUpdateService.CheckAsync();
                RuntimeLog.Write("info", "windows.update.checked current=" + BuildInfo.ShortCommit
                    + " latest=" + update.Manifest.short_commit
                    + " available=" + update.Available.ToString().ToLowerInvariant());
                if (!update.Available)
                {
                    if (interactive) SetStatus("SPLINED is current at " + channel + " commit " + BuildInfo.ShortCommit + ".");
                    if (interactive)
                        MessageBox.Show(this,
                            "SPLINED is current.\r\n\r\nInstalled commit: " + BuildInfo.ShortCommit,
                            "SPLINED update check", MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }

                WindowsUpdateManifest manifest = update.Manifest;
                SetStatus("Windows " + channel + " update " + manifest.short_commit + " is available.");
                string published = String.IsNullOrWhiteSpace(manifest.published_at)
                    ? "unknown"
                    : manifest.published_at;
                DialogResult install = MessageBox.Show(this,
                    "A Windows " + channel + " update is available.\r\n\r\n"
                    + "Installed commit: " + BuildInfo.ShortCommit + "\r\n"
                    + "Available commit: " + manifest.short_commit + "\r\n"
                    + "Published: " + published + "\r\n\r\n"
                    + "Download, verify, install, and restart SPLINED now?",
                    "SPLINED update available", MessageBoxButtons.YesNo, MessageBoxIcon.Information);
                if (install != DialogResult.Yes) return;
                if (running)
                {
                    MessageBox.Show(this,
                        "Finish or stop the active album run before installing the update.",
                        "SPLINED update", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    return;
                }

                SetStatus("Downloading and verifying Windows update " + manifest.short_commit + "...");
                progress.Style = ProgressBarStyle.Marquee;
                progress.Visible = true;
                string updater = await WindowsUpdateService.DownloadAndStageAsync(manifest);
                RuntimeLog.Write("info", "windows.update.verified current=" + BuildInfo.ShortCommit
                    + " available=" + manifest.short_commit);
                MessageBox.Show(this,
                    "The update was downloaded and SHA-256 verified. SPLINED will now restart.",
                    "SPLINED update ready", MessageBoxButtons.OK, MessageBoxIcon.Information);
                WindowsUpdateService.LaunchUpdater(updater);
                RuntimeLog.Write("info", "windows.update.launched commit=" + manifest.short_commit);
                Application.Exit();
            }
            catch (Exception error)
            {
                if (interactive) SetStatus("Windows update check failed.");
                RuntimeLog.Write("error", "windows.update.failed type=" + error.GetType().Name
                    + " message=" + error.Message);
                if (interactive)
                    MessageBox.Show(this,
                        "SPLINED could not complete the update check.\r\n\r\n" + error.Message,
                        "SPLINED update check", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
            finally
            {
                updateCheckRunning = false;
                if (checkUpdateMenuItem != null) checkUpdateMenuItem.Enabled = true;
                progress.Visible = false;
                progress.Style = ProgressBarStyle.Blocks;
                if (!IsDisposed && !running) UpdateSelectionControls();
            }
        }

        private void ApplyTheme()
        {
            ApplyThemeAndMaybeRebuildTree(false);
        }

        private void ApplyThemeAndMaybeRebuildTree(bool rebuildTree)
        {
            ThemeManager.Apply(this, uiState.Theme, delegate
            {
                ApplyModeActivityAppearance();
                ThemePalette palette = ThemeManager.CurrentPalette;
                candidateCards.BackColor = palette.PanelSurface;
                foreach (Control card in candidateCards.Controls)
                {
                    int candidateIndex = card.Tag is int ? (int)card.Tag : -1;
                    CandidateView candidate;
                    PictureBox thumb = card.Controls["candidateImage"] as PictureBox;
                    if (thumb != null) thumb.BackColor = palette.SurfacePrimary;
                    CheckBox heading = card.Controls["candidateChoice"] as CheckBox;
                    if (heading != null && candidates.TryGetValue(candidateIndex, out candidate))
                        heading.ForeColor = CandidateSourceColor(candidate.SourceKey, palette);
                }
                statusStrip.BackColor = palette.TopNavigationSurface;
                statusStrip.ForeColor = palette.TextSecondary;
                UpdateMediaFilterColors();
                if (candidates.Count > 0) RebuildCandidateFilterPanel();
                else RefreshCandidateFilterDependencies();
                UpdateHoverButton();
                UpdateSelectionControls();
                RenderDisplayedAlbum();
                if (showMediaSelectorMenuItem != null) showMediaSelectorMenuItem.Checked = uiState.ShowMediaSelector;
                if (rebuildTree) BuildTree();
            });
        }

        private void ApplyModeActivityAppearance()
        {
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            activity.BackColor = palette.PanelSurface;
            activity.ForeColor = palette.LogForeground;
        }

        private void MainFormClosing(object sender, FormClosingEventArgs e)
        {
            if (running)
            {
                DialogResult answer = MessageBox.Show(this, "Stop the active album operation and close SPLINED?", "Processing is active", MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2);
                if (answer != DialogResult.Yes) { e.Cancel = true; return; }
                StopRun();
            }
            SaveUiState();
        }

        private void SaveUiState()
        {
            Rectangle bounds = WindowState == FormWindowState.Normal ? Bounds : RestoreBounds;
            if (bounds.Width > 0 && bounds.Height > 0)
            {
                uiState.MainWidth = bounds.Width;
                uiState.MainHeight = bounds.Height;
                uiState.MainX = bounds.X;
                uiState.MainY = bounds.Y;
            }
            uiState.MainMaximized = WindowState == FormWindowState.Maximized;
            if (mainSplit != null && mainSplit.SplitterDistance >= mainSplit.Panel1MinSize)
                uiState.MainSplitterDistance = mainSplit.SplitterDistance;
            if (rightSplit != null)
            {
                int persistedRightDistance = candidateFilterPreviousSplitterDistance >= rightSplit.Panel1MinSize
                    ? candidateFilterPreviousSplitterDistance
                    : rightSplit.SplitterDistance;
                if (persistedRightDistance >= rightSplit.Panel1MinSize)
                    uiState.RightSplitterDistance = persistedRightDistance;
            }
            if (mediaFilterPanel != null) uiState.MediaFilterExpanded = mediaFilterPanel.Visible;
            if (artistFilter != null) uiState.MediaArtistFilter = artistFilter.Text;
            if (albumFilter != null) uiState.MediaAlbumFilter = albumFilter.Text;
            uiState.MediaShowWhite = IsMediaStatusChecked(MediaStatusFilter.White);
            uiState.MediaShowOrange = IsMediaStatusChecked(MediaStatusFilter.Orange);
            uiState.MediaShowRed = IsMediaStatusChecked(MediaStatusFilter.Red);
            uiState.MediaShowPurple = IsMediaStatusChecked(MediaStatusFilter.Purple);
            uiState.MediaShowGreen = IsMediaStatusChecked(MediaStatusFilter.Green);
            uiState.MediaShowBlue = IsMediaStatusChecked(MediaStatusFilter.Blue);
            uiState.MediaShowIncomplete = IsMediaStatusChecked(MediaStatusFilter.Incomplete);
            uiState.AutoScanEnabled = autoScanEnabled;
            uiState.AutoScanScope = autoScanScope;
            uiState.FilteredScanMode = filteredScanRead != null && filteredScanRead.Checked
                ? "read"
                : filteredScanWrite != null && filteredScanWrite.Checked ? "write" : "";
            uiState.SelectedAlbumPaths = albums
                .Where(album => album.Selected && album.State != AlbumState.Bypassed && album.State != AlbumState.TimeoutActive)
                .Select(album => album.Path)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToList();
            ConfigStore.SaveUi(uiState);
        }

        private string FindCoreExecutable()
        {
            string embeddedHost = Environment.GetEnvironmentVariable("SPLINED_CORE_PATH");
            if (!String.IsNullOrWhiteSpace(embeddedHost) && File.Exists(embeddedHost))
                return Path.GetFullPath(embeddedHost);
            string path = Path.Combine(ConfigStore.AppRoot, "splined.exe");
            if (File.Exists(path)) return path;
            return null;
        }

        private void BeginAlbumRunStatistics(AlbumInfo album)
        {
            activeAlbumStatistics = new AlbumRunStatistics
            {
                Artist = album == null ? "Unknown Artist" : album.Artist,
                Album = album == null ? "Unknown Album" : album.Title,
                AlbumPath = album == null ? "" : album.Path,
                StartedUtc = DateTime.UtcNow
            };
            runAlbumStatistics.Add(activeAlbumStatistics);
        }

        private AlbumRunStatistics FindAlbumRunStatistics(Dictionary<string, object> payload)
        {
            string albumPath = ReadString(payload, "album_path");
            if (activeAlbumStatistics != null
                && (String.IsNullOrWhiteSpace(albumPath) || SameAlbumPath(activeAlbumStatistics.AlbumPath, albumPath)))
                return activeAlbumStatistics;
            if (String.IsNullOrWhiteSpace(albumPath)) return null;
            return runAlbumStatistics.LastOrDefault(report => SameAlbumPath(report.AlbumPath, albumPath));
        }

        private void MarkAlbumOutcome(Dictionary<string, object> payload, string action, string destination)
        {
            AlbumRunStatistics report = FindAlbumRunStatistics(payload);
            if (report == null) return;
            report.Action = String.IsNullOrWhiteSpace(action) ? "Completed" : action;
            report.Destination = destination ?? "";
            report.FinishedUtc = DateTime.UtcNow;
        }

        private void CaptureFinalArtworkStatistics(Dictionary<string, object> payload)
        {
            AlbumRunStatistics report = FindAlbumRunStatistics(payload);
            if (report == null) return;
            report.SelectedSource = ReadString(payload, "source");
            int sourceWidth = ReadInt(payload, "source_width", 0);
            int sourceHeight = ReadInt(payload, "source_height", 0);
            int finalWidth = ReadInt(payload, "final_width", 0);
            int finalHeight = ReadInt(payload, "final_height", 0);
            report.SourceResolution = sourceWidth > 0 && sourceHeight > 0
                ? sourceWidth + " x " + sourceHeight : "";
            report.FinalResolution = finalWidth > 0 && finalHeight > 0
                ? finalWidth + " x " + finalHeight : "";
            report.UpscaleBackend = ReadString(payload, "upscale_backend");
            report.PicturePercent = ReadInt(payload, "picture_percent", 0);
            report.BrightnessPercent = ReadInt(payload, "brightness_percent", 0);
            report.ContrastPercent = ReadInt(payload, "contrast_percent", 0);
            report.ExposurePercent = ReadInt(payload, "exposure_percent", 0);
            report.SharpenPercent = ReadInt(payload, "sharpen_percent", 0);
            report.SoftnessPercent = ReadInt(payload, "softness_percent", 0);
            report.GammaPercent = ReadInt(payload, "gamma_percent", 0);
            report.ColorTemperature = ReadInt(payload, "color_temperature", 0);
            report.AdaptiveDefaults = ReadBool(payload, "upscale_adaptive_defaults");
            report.QualityEligible = ReadBool(payload, "quality_eligible");
            report.Resized = ReadBool(payload, "resized");
            report.Converted = ReadBool(payload, "converted");
        }

        private void FinishActiveAlbumStatistics(string defaultAction)
        {
            if (activeAlbumStatistics == null) return;
            if (activeAlbumStatistics.FinishedUtc == DateTime.MinValue)
            {
                activeAlbumStatistics.FinishedUtc = DateTime.UtcNow;
                activeAlbumStatistics.Action = String.IsNullOrWhiteSpace(defaultAction) ? "Incomplete" : defaultAction;
            }
            activeAlbumStatistics = null;
        }

        private void AppendAlbumActivityHeader(int index, int total, string artist, string albumTitle)
        {
            AppendActivity("[" + index + "/" + total + "]", ActivityTone.Orange);
            AppendActivity(" " + (String.IsNullOrWhiteSpace(artist) ? "Unknown Artist" : artist), ActivityTone.Purple);
            AppendActivity(" - ", ActivityTone.Normal);
            AppendActivity((String.IsNullOrWhiteSpace(albumTitle) ? "Unknown Album" : albumTitle) + "\r\n", ActivityTone.Yellow);
        }

        private void ShowAlbumRunReport(string runMode, int requestedAlbums)
        {
            activity.Clear();
            AppendActivity("S:P:L:I:N:E:D ALBUM RUN REPORT\r\n", ActivityTone.Heading);
            AppendActivity("Mode: ", ActivityTone.Muted);
            AppendActivity((runMode ?? state.Mode).ToUpperInvariant(),
                String.Equals(runMode, "read", StringComparison.OrdinalIgnoreCase) ? ActivityTone.Accent : ActivityTone.Warning);
            bool readReport = String.Equals(runMode, "read", StringComparison.OrdinalIgnoreCase);
            AppendActivity(readReport ? "  •  Albums reviewed: " : "  •  Albums processed: ", ActivityTone.Muted);
            AppendActivity(runAlbumStatistics.Count + "/" + requestedAlbums + "\r\n\r\n", ActivityTone.Success);

            for (int index = 0; index < runAlbumStatistics.Count; index++)
            {
                AlbumRunStatistics report = runAlbumStatistics[index];
                AppendAlbumActivityHeader(index + 1, runAlbumStatistics.Count, report.Artist, report.Album);

                AppendActivity("  Outcome: ", ActivityTone.Muted);
                ActivityTone outcomeTone = report.Action.Equals("Skipped", StringComparison.OrdinalIgnoreCase)
                    || report.Action.Equals("Postponed", StringComparison.OrdinalIgnoreCase)
                    ? ActivityTone.Warning
                    : report.Action.Equals("Incomplete", StringComparison.OrdinalIgnoreCase)
                        || report.Action.Equals("Stopped", StringComparison.OrdinalIgnoreCase)
                        ? ActivityTone.Error
                        : ActivityTone.Success;
                AppendActivity(report.Action + "\r\n", outcomeTone);

                AppendActivity("  Discovery: ", ActivityTone.Muted);
                AppendActivity(report.Fallback ? "Fallback" : report.MusicBrainzResolved ? "MusicBrainz" : "Provider search",
                    report.Fallback ? ActivityTone.Purple : ActivityTone.Accent);
                AppendActivity("  •  Review: " + (report.ReviewRequired ? "required" : "automatic") + "\r\n", ActivityTone.Muted);

                AppendActivity("  Candidates: ", ActivityTone.Muted);
                AppendActivity(report.EvaluatedCandidates + " evaluated", ActivityTone.Accent);
                AppendActivity("  •  " + report.VisibleCandidates + " shown", ActivityTone.Success);
                AppendActivity("  •  " + report.HiddenCandidates + " policy-hidden", report.HiddenCandidates > 0 ? ActivityTone.Warning : ActivityTone.Muted);
                AppendActivity("  •  " + report.ProviderDiagnostics + " provider note(s)\r\n", report.ProviderDiagnostics > 0 ? ActivityTone.Warning : ActivityTone.Muted);
                foreach (string note in report.ProviderDiagnosticMessages)
                    AppendActivity("    - " + note + "\r\n", ActivityTone.Warning);

                if (!String.IsNullOrWhiteSpace(report.SelectedSource))
                {
                    AppendActivity("  Selected: ", ActivityTone.Muted);
                    AppendActivity(CandidateSourceName(report.SelectedSource), ActivityTone.Accent);
                    if (!String.IsNullOrWhiteSpace(report.SourceResolution))
                        AppendActivity("  •  Source " + report.SourceResolution, ActivityTone.Muted);
                    if (!String.IsNullOrWhiteSpace(report.FinalResolution))
                        AppendActivity("  →  Final " + report.FinalResolution, report.Resized ? ActivityTone.Purple : ActivityTone.Success);
                    AppendActivity("\r\n");
                }
                bool hasArtworkAdjustments = report.PicturePercent != 0 || report.BrightnessPercent != 0
                    || report.ContrastPercent != 0 || report.ExposurePercent != 0
                    || report.SharpenPercent != 0 || report.SoftnessPercent != 0
                    || report.GammaPercent != 0 || report.ColorTemperature != 0 || report.AdaptiveDefaults;
                if ((!String.IsNullOrWhiteSpace(report.UpscaleBackend)
                        && !report.UpscaleBackend.Equals("none", StringComparison.OrdinalIgnoreCase))
                    || hasArtworkAdjustments)
                {
                    AppendActivity(report.UpscaleBackend.Equals("none", StringComparison.OrdinalIgnoreCase)
                        ? "  Artwork edit: " : "  Upscale: ", ActivityTone.Muted);
                    AppendActivity(report.UpscaleBackend.Equals("none", StringComparison.OrdinalIgnoreCase)
                        ? "native resolution" : report.UpscaleBackend, ActivityTone.Purple);
                    if (report.PicturePercent != 0)
                        AppendActivity("  •  picture " + report.PicturePercent.ToString("+0;-0;0") + "%", ActivityTone.Muted);
                    if (report.BrightnessPercent != 0)
                        AppendActivity("  •  brightness " + report.BrightnessPercent.ToString("+0;-0;0") + "%", ActivityTone.Muted);
                    if (report.ContrastPercent != 0)
                        AppendActivity("  •  contrast " + report.ContrastPercent.ToString("+0;-0;0") + "%", ActivityTone.Muted);
                    if (report.ExposurePercent != 0)
                        AppendActivity("  •  exposure " + report.ExposurePercent.ToString("+0;-0;0") + "%", ActivityTone.Muted);
                    if (report.SharpenPercent != 0)
                        AppendActivity("  •  sharpen " + report.SharpenPercent + "%", ActivityTone.Muted);
                    if (report.SoftnessPercent != 0)
                        AppendActivity("  •  softness " + report.SoftnessPercent + "%", ActivityTone.Muted);
                    if (report.GammaPercent != 0)
                        AppendActivity("  •  gamma " + report.GammaPercent.ToString("+0;-0;0") + "%", ActivityTone.Muted);
                    if (report.ColorTemperature != 0)
                        AppendActivity("  •  color " + UpscaleProfileDisplay("temperature", report.ColorTemperature), ActivityTone.Muted);
                    if (report.AdaptiveDefaults)
                        AppendActivity("  •  adaptive defaults", ActivityTone.Muted);
                    if (!report.QualityEligible)
                        AppendActivity("  •  manual-only quality", ActivityTone.Warning);
                    AppendActivity(report.Converted ? "  •  format converted\r\n" : "\r\n", ActivityTone.Muted);
                }

                AppendActivity("  Duration: " + report.Duration.TotalSeconds.ToString("0.0") + "s", ActivityTone.Muted);
                if (!String.IsNullOrWhiteSpace(report.Destination))
                {
                    AppendActivity("  •  Result: ", ActivityTone.Muted);
                    AppendActivity(report.Destination, outcomeTone);
                }
                AppendActivity("\r\n\r\n");
            }

            AppendActivity("Library colors and selections have been reconciled from current folder/history/bypass authority.\r\n", ActivityTone.Muted);
            activity.SelectionStart = 0;
            activity.SelectionLength = 0;
            activity.ScrollToCaret();
        }

        private void AppendActivity(string text)
        {
            AppendActivity(text, ActivityTone.Normal);
        }

        private void AppendActivity(string text, ActivityTone tone)
        {
            activity.SelectionStart = activity.TextLength;
            activity.SelectionLength = 0;
            activity.SelectionColor = ActivityColor(tone);
            activity.SelectedText = text;
            activity.SelectionColor = activity.ForeColor;
            activity.SelectionStart = activity.TextLength;
            activity.ScrollToCaret();
        }

        private void AppendActivitySafe(string text)
        {
            AppendActivitySafe(text, ActivityTone.Normal);
        }

        private void AppendActivitySafe(string text, ActivityTone tone)
        {
            if (IsDisposed) return;
            if (InvokeRequired) BeginInvoke((MethodInvoker)delegate { AppendActivity(text, tone); });
            else AppendActivity(text, tone);
        }

        private Color ActivityColor(ActivityTone tone)
        {
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            switch (tone)
            {
                case ActivityTone.Heading: return palette.LogHeading;
                case ActivityTone.Accent: return palette.LogAccent;
                case ActivityTone.Success: return palette.LogSuccess;
                case ActivityTone.Warning: return palette.LogWarning;
                case ActivityTone.Error: return palette.LogError;
                case ActivityTone.Muted: return palette.LogMuted;
                case ActivityTone.Orange: return palette.StatusOrange;
                case ActivityTone.Purple: return palette.StatusPurple;
                case ActivityTone.Yellow: return palette.LogWarning;
                default: return activity.ForeColor;
            }
        }

        private void SetStatus(string text)
        {
            statusLabel.Text = text;
        }

        private static string QuoteArgument(string value)
        {
            string argument = value ?? "";
            if (argument.IndexOf('"') >= 0)
                throw new ArgumentException("Windows paths containing a quote character are not supported.", "value");
            return "\"" + argument + "\"";
        }

        private static string StripAnsi(string value)
        {
            return Regex.Replace(value, "\\x1B\\[[0-?]*[ -/]*[@-~]", "");
        }

        private static string ReadString(Dictionary<string, object> values, string key)
        {
            object value;
            return values != null && values.TryGetValue(key, out value) && value != null ? Convert.ToString(value) : "";
        }

        private static int ReadInt(Dictionary<string, object> values, string key, int fallback)
        {
            object value;
            int parsed;
            return values != null && values.TryGetValue(key, out value) && value != null && Int32.TryParse(Convert.ToString(value), out parsed) ? parsed : fallback;
        }

        private static bool ReadBool(Dictionary<string, object> values, string key)
        {
            object value;
            bool parsed;
            return values != null && values.TryGetValue(key, out value) && value != null && Boolean.TryParse(Convert.ToString(value), out parsed) && parsed;
        }

        private static object[] ToObjectArray(object value)
        {
            object[] array = value as object[];
            if (array != null) return array;
            System.Collections.ArrayList list = value as System.Collections.ArrayList;
            return list == null ? new object[0] : list.ToArray();
        }

        private Image LoadImageCopy(string path)
        {
            try
            {
                if (String.IsNullOrWhiteSpace(path)) return null;
                string key = Path.GetFullPath(path);
                byte[] bytes;
                if (!candidateImageMemoryCache.TryGetValue(key, out bytes))
                {
                    bytes = File.ReadAllBytes(path);
                    if (bytes.LongLength <= CandidateImageMemoryCacheLimit)
                    {
                        candidateImageMemoryCache[key] = bytes;
                        candidateImageMemoryCacheOrder.Enqueue(key);
                        candidateImageMemoryCacheBytes += bytes.LongLength;
                        while (candidateImageMemoryCacheBytes > CandidateImageMemoryCacheLimit
                            && candidateImageMemoryCacheOrder.Count > 0)
                        {
                            string oldest = candidateImageMemoryCacheOrder.Dequeue();
                            byte[] removed;
                            if (candidateImageMemoryCache.TryGetValue(oldest, out removed))
                            {
                                candidateImageMemoryCache.Remove(oldest);
                                candidateImageMemoryCacheBytes -= removed.LongLength;
                            }
                        }
                    }
                }
                using (MemoryStream stream = new MemoryStream(bytes))
                using (Image source = Image.FromStream(stream)) return new Bitmap(source);
            }
            catch { return null; }
        }

        private void InvalidateCandidateImage(string path)
        {
            if (String.IsNullOrWhiteSpace(path)) return;
            string key;
            try { key = Path.GetFullPath(path); }
            catch { return; }
            byte[] removed;
            if (candidateImageMemoryCache.TryGetValue(key, out removed))
            {
                candidateImageMemoryCache.Remove(key);
                candidateImageMemoryCacheBytes -= removed.LongLength;
            }
        }

        private sealed class ArtistNodeInfo
        {
            public string Name;
            public List<AlbumInfo> AllAlbums;
            public List<AlbumInfo> VisibleAlbums;
            public ArtistAggregateState AggregateState;
            public ArtistNodeInfo(string name, List<AlbumInfo> allAlbums, List<AlbumInfo> visibleAlbums, ArtistAggregateState aggregateState)
            {
                Name = name;
                AllAlbums = allAlbums;
                VisibleAlbums = visibleAlbums;
                AggregateState = aggregateState;
            }
        }
    }

    internal sealed class CandidateView
    {
        public int Index;
        public string Source = "";
        public string Resolution;
        public int PixelWidth;
        public int PixelHeight;
        public int ProjectedWidth;
        public int ProjectedHeight;
        public string Format = "";
        public string SourceRange = "";
        public string Range = "";
        public bool Upscaled;
        public bool UpscaleEligible;
        public bool Approved;
        public bool Square;
        public bool Acceptable;
        public string PolicyStatus = "";
        public string PolicyReason = "";
        public bool SourceOverrideActive;
        public bool StrictOverrideActive;
        public string StrictStatus = "";
        public string StrictReason = "";
        public bool StrictPreferredEligible = true;
        public bool StrictAutoEligible = true;
        public bool Recommended;
        public string CachePath = "";
        public string Url = "";
        public string LocalOrigin = "";
        public string LocalReference = "";
        public string SourceKey
        {
            get { return IsLocal ? "local" : (Source ?? "").Trim().ToLowerInvariant(); }
        }
        public string SourceRangeKey
        {
            get { return NormalizeRange(SourceRange); }
        }
        public string ProjectedRangeKey
        {
            get { return NormalizeRange(Range); }
        }
        public bool IsRejected
        {
            get { return !Acceptable || PolicyStatus.Equals("reject", StringComparison.OrdinalIgnoreCase); }
        }
        public bool IsNegative
        {
            get { return IsRejected || SourceRangeKey.Equals("BelowMinimum", StringComparison.OrdinalIgnoreCase); }
        }
        public bool IsLocal
        {
            get
            {
                return (Source ?? "").Equals("local", StringComparison.OrdinalIgnoreCase)
                    || (Source ?? "").Equals("webpstill", StringComparison.OrdinalIgnoreCase)
                    || !String.IsNullOrWhiteSpace(LocalOrigin);
            }
        }
        public string DisplaySource
        {
            get
            {
                if (LocalOrigin.Equals("embedded-track", StringComparison.OrdinalIgnoreCase))
                    return "Embedded from track" + (String.IsNullOrWhiteSpace(LocalReference) ? "" : " · " + LocalReference);
                if (LocalOrigin.Equals("cover-file", StringComparison.OrdinalIgnoreCase))
                    return "Cover file" + (String.IsNullOrWhiteSpace(LocalReference) ? "" : " · " + LocalReference);
                return Source;
            }
        }

        private static string NormalizeRange(string value)
        {
            string compact = (value ?? "").Replace("-", "").Replace("_", "").Replace(" ", "").ToLowerInvariant();
            if (compact == "belowminimum") return "BelowMinimum";
            if (compact == "lowerrange") return "LowerRange";
            if (compact == "ideal") return "Ideal";
            if (compact == "upperrange") return "UpperRange";
            if (compact == "ladder") return "Ladder";
            if (compact == "aboveladder") return "AboveLadder";
            return value ?? "";
        }
    }

    internal sealed class CandidateFilterOption
    {
        public readonly string Label;
        public readonly string Key;
        public readonly bool Present;
        public readonly Color Color;

        public CandidateFilterOption(string label, bool present, Color color, string key = null)
        {
            Label = label;
            Key = key ?? label;
            Present = present;
            Color = color;
        }
    }

    internal sealed class CandidateFilterBinding
    {
        public readonly string Dimension;
        public readonly string Key;
        public readonly Color Color;
        public readonly CheckBox Check;
        public readonly Label Count;

        public CandidateFilterBinding(string dimension, string key, Color color, CheckBox check, Label count)
        {
            Dimension = dimension;
            Key = key;
            Color = color;
            Check = check;
            Count = count;
        }
    }

    internal sealed class FallbackSearchForm : FluentForm
    {
        private readonly TextBox artist;
        private readonly TextBox album;
        public string Artist { get { return artist.Text.Trim(); } }
        public string Album { get { return album.Text.Trim(); } }

        public FallbackSearchForm(string currentArtist, string currentAlbum, string theme)
        {
            Text = "Refine SPLINED Fallback Search";
            StartPosition = FormStartPosition.CenterParent;
            Size = new Size(570, 260);
            MinimumSize = new Size(500, 240);
            MaximizeBox = false;
            MinimizeBox = false;
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;

            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(16), ColumnCount = 2, RowCount = 4 };
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 80));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 52));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            Controls.Add(root);

            Label explanation = new Label { Text = "Edit only the provider-search values for this retry. Music tags and SPLINED history are not changed.", Dock = DockStyle.Fill, AutoSize = false };
            root.SetColumnSpan(explanation, 2);
            root.Controls.Add(explanation, 0, 0);
            root.Controls.Add(new Label { Text = "Artist", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 1);
            artist = new FluentTextBox { Dock = DockStyle.Fill, Text = currentArtist ?? "" };
            root.Controls.Add(artist, 1, 1);
            root.Controls.Add(new Label { Text = "Album", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 2);
            album = new FluentTextBox { Dock = DockStyle.Fill, Text = currentAlbum ?? "" };
            root.Controls.Add(album, 1, 2);

            FlowLayoutPanel actions = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft, WrapContents = false, Padding = new Padding(0, 8, 0, 0) };
            Button retry = new FluentButton { Text = "Retry Search", Width = 125, Height = 34, Tag = "primary", DialogResult = DialogResult.OK };
            Button cancel = new FluentButton { Text = "Cancel", Width = 100, Height = 34, DialogResult = DialogResult.Cancel };
            retry.Click += delegate
            {
                if (Artist.Length == 0 || Album.Length == 0)
                {
                    MessageBox.Show(this, "Both Artist and Album are required for fallback search.", "SPLINED fallback", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    DialogResult = DialogResult.None;
                }
            };
            actions.Controls.Add(retry);
            actions.Controls.Add(cancel);
            root.SetColumnSpan(actions, 2);
            root.Controls.Add(actions, 0, 3);
            AcceptButton = retry;
            CancelButton = cancel;
            ThemeManager.Apply(this, theme);
            ThemeManager.StyleButton(retry, theme);
            artist.SelectAll();
            ThemeManager.PrepareForFirstShow(this, theme);
        }
    }

    internal static class CandidateUrlUi
    {
        public static LinkLabel Create(string leadingText, string url, string theme,
            Action preview = null, Action hover = null, Action leave = null)
        {
            LinkLabel label = new LinkLabel { Text = leadingText ?? "", AutoEllipsis = true, TextAlign = ContentAlignment.MiddleCenter };
            ThemePalette palette = ThemeManager.PaletteFor(theme);
            label.ForeColor = palette.TextPrimary;
            label.LinkColor = palette.Link;
            label.ActiveLinkColor = palette.LinkActive;
            label.VisitedLinkColor = label.LinkColor;
            Uri parsed;
            if (Uri.TryCreate(url, UriKind.Absolute, out parsed) && (parsed.Scheme == Uri.UriSchemeHttp || parsed.Scheme == Uri.UriSchemeHttps))
            {
                label.Text += "  [URL]";
                int start = label.Text.LastIndexOf("URL", StringComparison.Ordinal);
                label.Links.Add(start, 3, parsed.AbsoluteUri);
                label.LinkClicked += delegate(object sender, LinkLabelLinkClickedEventArgs args)
                {
                    try
                    {
                        if (preview != null) preview();
                        Process.Start(new ProcessStartInfo { FileName = Convert.ToString(args.Link.LinkData), UseShellExecute = true });
                    }
                    catch (Exception error)
                    {
                        MessageBox.Show(label.FindForm(), error.Message, "Unable to open artwork source", MessageBoxButtons.OK, MessageBoxIcon.Error);
                    }
                };
                if (hover != null) label.MouseEnter += delegate { hover(); };
                if (leave != null) label.MouseLeave += delegate { leave(); };
            }
            return label;
        }
    }

    internal sealed class CandidateCompareForm : FluentForm
    {
        private readonly UiState uiState;
        private readonly string mode;
        private readonly bool allowSelection;
        private readonly FlowLayoutPanel previews;
        public int SelectedCandidateIndex { get; private set; }

        public CandidateCompareForm(IList<CandidateView> candidates, string theme, UiState ui, string activeMode, bool canSelect)
        {
            uiState = ui;
            mode = activeMode;
            allowSelection = canSelect;
            Text = "SPLINED - Compare Artwork";
            StartPosition = FormStartPosition.CenterParent;
            Size = new Size(Math.Max(760, ui.CompareWidth), Math.Max(500, ui.CompareHeight));
            MinimumSize = new Size(760, 500);
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;

            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(12), ColumnCount = 1, RowCount = 2 };
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            Controls.Add(root);
            previews = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoScroll = true, WrapContents = false, FlowDirection = FlowDirection.LeftToRight, Padding = new Padding(4) };
            root.Controls.Add(previews, 0, 0);

            foreach (CandidateView candidate in candidates)
                previews.Controls.Add(BuildPreview(candidate.Recommended ? "SPLINED Recommended" : "Candidate " + candidate.Index, candidate, theme));

            FlowLayoutPanel actions = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft, WrapContents = false, Padding = new Padding(0, 7, 0, 0) };
            Button close = new FluentButton { Text = "Close", Width = 110, Height = 34 };
            close.Click += delegate { Close(); };
            actions.Controls.Add(close);
            root.Controls.Add(actions, 0, 1);

            Resize += delegate { ResizePreviewCards(); };
            FormClosing += delegate
            {
                uiState.CompareWidth = Width;
                uiState.CompareHeight = Height;
                ConfigStore.SaveUi(uiState);
            };
            FormClosed += delegate
            {
                foreach (PictureBox picture in FindPictures(previews))
                    if (picture.Image != null) picture.Image.Dispose();
            };
            ThemeManager.Apply(this, theme);
            ResizePreviewCards();
            ThemeManager.PrepareForFirstShow(this, theme);
        }

        private Control BuildPreview(string heading, CandidateView candidate, string theme)
        {
            TableLayoutPanel panel = new FluentCardTableLayoutPanel { Width = 330, Height = 450, Margin = new Padding(ThemeManager.Space4), Padding = new Padding(ThemeManager.Space8), RowCount = 4, ColumnCount = 1, Tag = candidate.Index };
            panel.RowStyles.Add(new RowStyle(SizeType.Absolute, 34));
            panel.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            panel.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            panel.RowStyles.Add(new RowStyle(SizeType.Absolute, 44));
            panel.Controls.Add(new Label { Text = heading, Dock = DockStyle.Fill, Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle), TextAlign = ContentAlignment.MiddleCenter }, 0, 0);
            Panel imageHost = new Panel { Dock = DockStyle.Fill };
            PictureBox picture = new PictureBox { SizeMode = PictureBoxSizeMode.Zoom, BorderStyle = BorderStyle.None, Image = LoadCandidateImage(candidate.CachePath) };
            imageHost.Controls.Add(picture);
            Action fitImage = delegate
            {
                int sourceWidth = candidate.PixelWidth > 0 ? candidate.PixelWidth : (picture.Image == null ? 1 : picture.Image.Width);
                int sourceHeight = candidate.PixelHeight > 0 ? candidate.PixelHeight : (picture.Image == null ? 1 : picture.Image.Height);
                int availableWidth = Math.Max(1, imageHost.ClientSize.Width - 12);
                int availableHeight = Math.Max(1, imageHost.ClientSize.Height - 12);
                double scale = Math.Min(1.0, Math.Min(600.0 / sourceWidth, 600.0 / sourceHeight));
                scale = Math.Min(scale, Math.Min(availableWidth / (double)sourceWidth, availableHeight / (double)sourceHeight));
                picture.Size = new Size(Math.Max(1, (int)Math.Round(sourceWidth * scale)), Math.Max(1, (int)Math.Round(sourceHeight * scale)));
                picture.Location = new Point(Math.Max(0, (imageHost.ClientSize.Width - picture.Width) / 2), Math.Max(0, (imageHost.ClientSize.Height - picture.Height) / 2));
            };
            imageHost.Resize += delegate { fitImage(); };
            panel.Controls.Add(imageHost, 0, 1);
            LinkLabel metadata = CandidateUrlUi.Create(candidate.DisplaySource + "  " + candidate.Resolution + "  " + candidate.Range, candidate.Url, theme);
            metadata.Dock = DockStyle.Fill;
            panel.Controls.Add(metadata, 0, 2);
            Button choose = new FluentButton { Text = mode.Equals("write", StringComparison.OrdinalIgnoreCase) ? "Select and Write" : "Select for Read Review", Dock = DockStyle.Fill, Height = 34, Tag = "primary", Enabled = allowSelection };
            choose.Click += delegate
            {
                string effect = mode.Equals("write", StringComparison.OrdinalIgnoreCase)
                    ? "Write this artwork using the active Config v5 output rules?"
                    : "Use this artwork for the Read-mode review sample? Album artwork will not be changed.";
                if (uiState.ShowConfirmations
                    && MessageBox.Show(this, candidate.Source + "  " + candidate.Resolution + "\r\n\r\n" + effect,
                        "Confirm artwork selection", MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2) != DialogResult.Yes) return;
                SelectedCandidateIndex = candidate.Index;
                DialogResult = DialogResult.OK;
                Close();
            };
            panel.Controls.Add(choose, 0, 3);
            ThemeManager.StyleButton(choose, theme);
            fitImage();
            return panel;
        }

        private void ResizePreviewCards()
        {
            if (previews == null || previews.Controls.Count == 0) return;
            int visibleColumns = Math.Min(3, previews.Controls.Count);
            int width = Math.Max(285, (previews.ClientSize.Width - 18) / visibleColumns - 10);
            int height = Math.Max(390, previews.ClientSize.Height - 18);
            foreach (Control preview in previews.Controls)
            {
                preview.Width = width;
                preview.Height = height;
            }
        }

        internal static Image LoadCandidateImage(string path)
        {
            try { byte[] bytes = File.ReadAllBytes(path); using (MemoryStream stream = new MemoryStream(bytes)) using (Image source = Image.FromStream(stream)) return new Bitmap(source); }
            catch { return null; }
        }

        private static IEnumerable<PictureBox> FindPictures(Control root)
        {
            foreach (Control child in root.Controls)
            {
                PictureBox picture = child as PictureBox;
                if (picture != null) yield return picture;
                foreach (PictureBox nested in FindPictures(child)) yield return nested;
            }
        }
    }

    internal sealed class HoverPreviewForm : FluentForm
    {
        private readonly UiState uiState;
        private readonly PictureBox picture;

        public HoverPreviewForm(CandidateView candidate, string theme, UiState ui)
        {
            uiState = ui;
            Text = "Artwork Preview - " + candidate.Source;
            StartPosition = FormStartPosition.Manual;
            Size = new Size(Math.Max(360, ui.PreviewWidth), Math.Max(420, ui.PreviewHeight));
            MinimumSize = new Size(360, 420);
            FormBorderStyle = FormBorderStyle.SizableToolWindow;
            ShowInTaskbar = false;
            TopMost = true;
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;
            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(10), RowCount = 2, ColumnCount = 1 };
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            Controls.Add(root);
            picture = new PictureBox { Dock = DockStyle.Fill, SizeMode = PictureBoxSizeMode.Zoom, BorderStyle = BorderStyle.None, Image = CandidateCompareForm.LoadCandidateImage(candidate.CachePath) };
            root.Controls.Add(picture, 0, 0);
            root.Controls.Add(new Label { Text = candidate.Source + "  " + candidate.Resolution + "  " + candidate.Range + "   (resize or close this preview)", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleCenter }, 0, 1);
            Rectangle work = Screen.FromPoint(Cursor.Position).WorkingArea;
            Location = new Point(Math.Min(work.Right - Width, Cursor.Position.X + 18), Math.Min(work.Bottom - Height, Math.Max(work.Top, Cursor.Position.Y - 50)));
            FormClosing += delegate
            {
                uiState.PreviewWidth = Width;
                uiState.PreviewHeight = Height;
                ConfigStore.SaveUi(uiState);
            };
            FormClosed += delegate { if (picture.Image != null) picture.Image.Dispose(); };
            ThemeManager.Apply(this, theme);
            ThemeManager.PrepareForFirstShow(this, theme);
        }

        protected override bool ShowWithoutActivation { get { return true; } }
    }
}
