using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
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
        public bool Fallback;
        public bool MusicBrainzResolved;
        public bool ReviewRequired;
        public string Action = "Incomplete";
        public string Destination = "";

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
        Blue
    }

    internal sealed class MainForm : FluentForm
    {
        private const string EventPrefix = "@@SPLINED_GUI@@";
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
        private GroupBox mediaFilterPanel;
        private TableLayoutPanel libraryLayout;
        private Button mediaFilterToggle;
        private readonly Dictionary<MediaStatusFilter, CheckBox> mediaStatusFilters = new Dictionary<MediaStatusFilter, CheckBox>();
        private readonly Dictionary<MediaStatusFilter, Label> mediaStatusBullets = new Dictionary<MediaStatusFilter, Label>();
        private readonly Dictionary<MediaStatusFilter, Label> mediaStatusDescriptions = new Dictionary<MediaStatusFilter, Label>();
        private CheckBox filteredScanRead;
        private CheckBox filteredScanWrite;
        private Button selectAndLaunch;
        private CheckBox selectModeAll;
        private CheckBox selectModeNone;
        private CheckBox selectModeFiltered;
        private bool updatingFilteredControls;
        private bool autoLaunchFinished;
        private bool autoLaunchRun;
        private bool initialSelectionRestored;
        private string activeRunMode;
        private SplitContainer mainSplit;
        private SplitContainer rightSplit;
        private TreeView tree;
        private ToolTip treeToolTip;
        private Button launch;
        private ToolStripMenuItem settingsMenuItem;
        private RichTextBox activity;
        private FlowLayoutPanel candidateCards;
        private Button useSelected;
        private Button keepLocal;
        private Button compare;
        private Button skip;
        private Button refineFallback;
        private Button retryMusicBrainz;
        private FlowLayoutPanel fallbackActions;
        private Button enableHover;
        private Label candidateContext;
        private StatusStrip statusStrip;
        private ToolStripStatusLabel statusLabel;
        private ToolStripProgressBar progress;
        private int selectedCandidateIndex = -1;
        private int recommendedCandidateIndex = -1;
        private readonly Dictionary<int, CandidateView> candidates = new Dictionary<int, CandidateView>();
        private readonly HashSet<int> compareCandidateIndexes = new HashSet<int>();
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
        private string fallbackRetryArtist = "";
        private string fallbackRetryAlbum = "";
        private HoverPreviewForm hoverPreview;
        private const int ExpandedMediaFilterHeight = 421;
        private const int CollapsedMediaFilterHeight = 36;

        public MainForm(ConfigState state)
            : this(state, ConfigStore.LoadUi())
        {
        }

        internal MainForm(ConfigState state, UiState initialUiState)
        {
            this.state = state;
            uiState = initialUiState ?? ConfigStore.LoadUi();
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
            ApplyTheme();
            ThemeManager.PrepareForFirstShow(this, uiState.Theme);
            SetStatus("Loading library...");
            Load += delegate { RestorePanelSizes(); };
            Shown += async delegate
            {
                await ReloadLibraryAsync(false);
                if (uiState.ShowStatusOnLaunch)
                    using (StatusForm form = new StatusForm(this.state)) form.ShowDialog(this);
            };
            FormClosing += MainFormClosing;
            FormClosed += delegate { if (treeToolTip != null) treeToolTip.Dispose(); };
        }

        private void BuildLayout()
        {
            MenuStrip menu = BuildMenu();
            MainMenuStrip = menu;
            Panel header = new Panel { Dock = DockStyle.Top, Height = 54, Padding = new Padding(ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space4), Tag = "titlebar-surface" };
            Controls.Add(header);
            // Let MenuStrip perform its native item measurement before the
            // first paint. Hosting it directly avoids the empty item bounds
            // produced by the former auto-sized TableLayout combination.
            menu.AutoSize = true;
            menu.Dock = DockStyle.Top;
            menu.GripStyle = ToolStripGripStyle.Hidden;
            menu.LayoutStyle = ToolStripLayoutStyle.HorizontalStackWithOverflow;
            header.Controls.Add(menu);

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
            mainSplit.Orientation = Orientation.Vertical;
            mainSplit.SplitterWidth = 7;
            mainSplit.Panel1MinSize = 280;
            mainSplit.Panel2MinSize = 420;
            mainSplit.Panel1.Padding = new Padding(ThemeManager.Space8, ThemeManager.Space8, ThemeManager.Space4, ThemeManager.Space8);
            mainSplit.Panel2.Padding = new Padding(ThemeManager.Space4, ThemeManager.Space8, ThemeManager.Space8, ThemeManager.Space8);
            mainSplit.SplitterDistance = Math.Max(mainSplit.Panel1MinSize, Math.Min(uiState.MainSplitterDistance, Math.Max(mainSplit.Panel1MinSize, ClientSize.Width - mainSplit.Panel2MinSize - mainSplit.SplitterWidth)));
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
            ToolStripMenuItem reload = new ToolStripMenuItem("Reload Library");
            reload.Click += async delegate { await ReloadLibraryAsync(false); };
            ToolStripMenuItem exit = new ToolStripMenuItem("Exit");
            exit.Click += delegate { Close(); };
            file.DropDownItems.Add(settingsMenuItem);
            file.DropDownItems.Add(credentials);
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

            ToolStripMenuItem status = new ToolStripMenuItem("Status");
            status.Click += delegate { using (StatusForm form = new StatusForm(state)) form.ShowDialog(this); };
            ToolStripMenuItem help = new ToolStripMenuItem("Help");
            ToolStripMenuItem helpPage = new ToolStripMenuItem("Help");
            helpPage.Click += delegate { HelpWindows.OpenHelp(this); };
            ToolStripMenuItem checkUpdate = new ToolStripMenuItem("Check for Update...");
            checkUpdate.Click += CheckForUpdateClicked;
            ToolStripMenuItem about = new ToolStripMenuItem("About...");
            about.Click += delegate { using (AboutForm form = new AboutForm()) form.ShowDialog(this); };
            help.DropDownItems.Add(helpPage);
            help.DropDownItems.Add(checkUpdate);
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
            TableLayoutPanel layout = new FluentCardTableLayoutPanel { Name = "mediaLibrarySelectionCard", VisualRole = CardVisualRole.Panel };
            libraryLayout = layout;
            layout.Dock = DockStyle.Fill;
            layout.Padding = new Padding(ThemeManager.Space12);
            layout.ColumnCount = 1;
            layout.RowCount = 3;
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 34));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, ExpandedMediaFilterHeight));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            parent.Controls.Add(layout);

            layout.Controls.Add(BuildPanelTitle("Media Library Selection",
                "Select Media expands the Artist, Album, folder-status, selection-mode, and scan-mode controls. Select [ALL] checks every normally eligible album, Select [NONE] clears all selections, and Select [FILTERED] checks only the visible filtered results. LAUNCH processes checked albums only."), 0, 0);

            layout.Controls.Add(BuildMediaFilterPanel(), 0, 1);

            tree = new TreeView { Dock = DockStyle.Fill, CheckBoxes = true, ShowNodeToolTips = false, HideSelection = false, BorderStyle = BorderStyle.FixedSingle };
            treeToolTip = ThemeManager.CreateToolTip();
            tree.AfterCheck += TreeAfterCheck;
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

        private Control BuildMediaFilterPanel()
        {
            TableLayoutPanel outer = new TableLayoutPanel { Name = "mediaFilterContainer", Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Margin = new Padding(0), Padding = new Padding(0) };
            outer.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            outer.RowStyles.Add(new RowStyle(SizeType.Absolute, CollapsedMediaFilterHeight));
            outer.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            mediaFilterToggle = new FluentButton
            {
                Name = "mediaFilterToggle",
                Text = "Select Media  ▾",
                Dock = DockStyle.Fill,
                Height = CollapsedMediaFilterHeight,
                TextAlign = ContentAlignment.MiddleLeft,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                Tag = "success",
                Margin = new Padding(0, 0, 0, 2)
            };
            mediaFilterToggle.Click += delegate { SetMediaFilterExpanded(mediaFilterPanel == null || !mediaFilterPanel.Visible); };
            mediaFilterPanel = new FluentGroupBox { Name = "mediaFilterPanel", Text = "", Dock = DockStyle.Fill, Padding = new Padding(8) };
            TableLayoutPanel layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3 };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));

            artistFilter = AddMediaTextFilter(layout, 0, "Artist:", "mediaArtistFilter");
            albumFilter = AddMediaTextFilter(layout, 1, "Album:", "mediaAlbumFilter");
            artistFilter.TextChanged += MediaFilterCriteriaChanged;
            albumFilter.TextChanged += MediaFilterCriteriaChanged;

            TableLayoutPanel columns = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Padding = new Padding(0, 4, 0, 0) };
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 38));
            columns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 62));

            GroupBox statuses = new FluentGroupBox { Name = "mediaStatusFilters", Text = "Folder status", Dock = DockStyle.Fill, Padding = new Padding(7) };
            TableLayoutPanel statusRows = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 6 };
            for (int row = 0; row < 6; row++) statusRows.RowStyles.Add(new RowStyle(SizeType.Percent, 16.66f));
            AddMediaStatusFilter(statusRows, 0, MediaStatusFilter.White, "Unprocessed");
            AddMediaStatusFilter(statusRows, 1, MediaStatusFilter.Orange, "Processed");
            AddMediaStatusFilter(statusRows, 2, MediaStatusFilter.Red, "Bypassed");
            AddMediaStatusFilter(statusRows, 3, MediaStatusFilter.Purple, "Partial / Timeout");
            AddMediaStatusFilter(statusRows, 4, MediaStatusFilter.Green, "Artist Complete");
            AddMediaStatusFilter(statusRows, 5, MediaStatusFilter.Blue, "Artist Contains Bypass");
            statuses.Controls.Add(statusRows);
            columns.Controls.Add(statuses, 0, 0);
            columns.Controls.Add(BuildFilteredAutomationPanel(), 1, 0);
            layout.Controls.Add(columns, 0, 2);
            mediaFilterPanel.Controls.Add(layout);
            outer.Controls.Add(mediaFilterToggle, 0, 0);
            outer.Controls.Add(mediaFilterPanel, 0, 1);
            return outer;
        }

        private static TextBox AddMediaTextFilter(TableLayoutPanel parent, int row, string labelText, string name)
        {
            TableLayoutPanel line = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1 };
            line.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 58));
            line.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            line.Controls.Add(new Label { Text = labelText, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 0);
            TextBox box = new FluentTextBox { Name = name, Dock = DockStyle.Fill };
            line.Controls.Add(box, 1, 0);
            parent.Controls.Add(line, 0, row);
            return box;
        }

        private void AddMediaStatusFilter(TableLayoutPanel parent, int row, MediaStatusFilter stateFilter, string label)
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
            parent.Controls.Add(line, 0, row);
        }

        private Control BuildFilteredAutomationPanel()
        {
            TableLayoutPanel stack = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3, Padding = new Padding(5, 0, 0, 0) };
            stack.RowStyles.Add(new RowStyle(SizeType.Absolute, 90));
            stack.RowStyles.Add(new RowStyle(SizeType.Absolute, 96));
            stack.RowStyles.Add(new RowStyle(SizeType.Percent, 100));

            GroupBox select = new FluentGroupBox { Text = "Select Mode", Dock = DockStyle.Fill, Padding = new Padding(9, 8, 7, 7) };
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
            select.Controls.Add(selectChoices);

            GroupBox scan = new FluentGroupBox { Text = "Scan Mode", Dock = DockStyle.Fill, Padding = new Padding(9, 8, 7, 7) };
            TableLayoutPanel scanChoices = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 2, Padding = new Padding(0), Margin = new Padding(0) };
            scanChoices.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            scanChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 50));
            scanChoices.RowStyles.Add(new RowStyle(SizeType.Percent, 50));
            filteredScanRead = new FluentCheckBox { Name = "filteredScanRead", Text = "Filtered Scan [READ]", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            filteredScanWrite = new FluentCheckBox { Name = "filteredScanWrite", Text = "Filtered Scan [WRITE]", AutoSize = false, Dock = DockStyle.Fill, Margin = new Padding(0) };
            filteredScanRead.CheckedChanged += FilteredScanChoiceChanged;
            filteredScanWrite.CheckedChanged += FilteredScanChoiceChanged;
            scanChoices.Controls.Add(filteredScanRead, 0, 0);
            scanChoices.Controls.Add(filteredScanWrite, 0, 1);
            scan.Controls.Add(scanChoices);

            GroupBox automatic = new FluentGroupBox { Text = "Auto Mode", Dock = DockStyle.Fill, Padding = new Padding(9, 8, 7, 7) };
            FlowLayoutPanel autoChoices = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false, Padding = new Padding(2, 0, 0, 0) };
            selectAndLaunch = new FluentButton
            {
                Name = "selectAndLaunch",
                Text = "AUTO LAUNCH",
                Width = 160,
                Height = 28,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                Tag = "success",
                Margin = new Padding(0)
            };
            selectAndLaunch.Click += SelectAndLaunchClicked;
            autoChoices.Controls.Add(selectAndLaunch);
            automatic.Controls.Add(autoChoices);

            ToolTip help = ThemeManager.CreateToolTip();
            help.SetToolTip(select, "Choose ALL, NONE, or FILTERED. FILTERED checks only the currently visible Artist and Album results and keeps its selected indicator until the selection is manually changed.");
            help.SetToolTip(scan, "Choose one temporary Read or Write mode. Config v5 is not changed and bypass/timeout authority is preserved.");
            help.SetToolTip(automatic, "AUTO LAUNCH invokes the normal LAUNCH workflow for the albums that are already checked. It never expands the run to other visible or hidden albums.");
            select.Tag = help;
            scan.Tag = help;
            automatic.Tag = help;
            stack.Controls.Add(select, 0, 0);
            stack.Controls.Add(scan, 0, 1);
            stack.Controls.Add(automatic, 0, 2);
            return stack;
        }

        private void SetMediaFilterExpanded(bool expanded)
        {
            if (mediaFilterPanel == null || libraryLayout == null) return;
            mediaFilterPanel.Visible = expanded;
            mediaFilterToggle.Text = expanded ? "Select Media  ▾" : "Select Media  ▸";
            libraryLayout.RowStyles[1].Height = expanded ? ExpandedMediaFilterHeight : CollapsedMediaFilterHeight;
            libraryLayout.PerformLayout();
            uiState.MediaFilterExpanded = expanded;
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
            if (mainSplit != null && mainSplit.Width > mainSplit.Panel1MinSize + mainSplit.Panel2MinSize + mainSplit.SplitterWidth)
            {
                int maximum = mainSplit.Width - mainSplit.Panel2MinSize - mainSplit.SplitterWidth;
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
                filteredScanRead.Checked = String.Equals(uiState.FilteredScanMode, "read", StringComparison.OrdinalIgnoreCase);
                filteredScanWrite.Checked = String.Equals(uiState.FilteredScanMode, "write", StringComparison.OrdinalIgnoreCase);
                filteredScanRead.Enabled = !filteredScanWrite.Checked;
                filteredScanWrite.Enabled = !filteredScanRead.Checked;
            }
            finally { updatingFilteredControls = false; }
            SetMediaFilterExpanded(uiState.MediaFilterExpanded);
            UpdateAutoLaunchButton();
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
                default: return uiState.MediaShowWhite;
            }
        }

        private void MediaFilterCriteriaChanged(object sender, EventArgs e)
        {
            autoLaunchFinished = false;
            BuildTree();
            UpdateAutoLaunchButton();
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
                autoLaunchFinished = false;
            }
            finally { updatingFilteredControls = false; }
            ThemeManager.Apply(mediaFilterPanel, uiState.Theme);
            UpdateMediaFilterColors();
            UpdateAutoLaunchButton();
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

            autoLaunchFinished = false;
            if (Object.ReferenceEquals(selected, selectModeAll))
            {
                selectionMode = SelectionMode.All;
                foreach (AlbumInfo album in albums) album.Selected = album.EligibleByDefault;
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

        private void SelectAndLaunchClicked(object sender, EventArgs e)
        {
            if (updatingFilteredControls) return;
            if (running)
            {
                StopRun();
                return;
            }
            if (!filteredScanRead.Checked && !filteredScanWrite.Checked)
            {
                MessageBox.Show(this, "Choose Filtered Scan [READ] or Filtered Scan [WRITE] first.", "Filtered scan mode", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            if (SelectedAlbumsForAutoLaunch(albums).Count == 0)
            {
                MessageBox.Show(this, "Check one or more Album folders first. AUTO LAUNCH processes only albums that are already checked.", "No Album selections", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            autoLaunchFinished = false;
            autoLaunchRun = true;
            LaunchClicked(launch, EventArgs.Empty);
        }

        private static List<AlbumInfo> SelectedAlbumsForAutoLaunch(IEnumerable<AlbumInfo> source)
        {
            return (source ?? Enumerable.Empty<AlbumInfo>())
                .Where(album => album != null && album.Selected && album.State != AlbumState.TimeoutActive)
                .ToList();
        }

        private int SelectVisibleFilteredAlbums()
        {
            if (running) return 0;
            List<AlbumInfo> visible = tree.Nodes.Cast<TreeNode>()
                .SelectMany(node => node.Nodes.Cast<TreeNode>())
                .Select(node => node.Tag as AlbumInfo)
                .Where(album => album != null)
                .Distinct()
                .ToList();
            List<AlbumInfo> bypassed = visible.Where(album => album.State == AlbumState.Bypassed).ToList();
            bool includeBypassed = false;
            if (bypassed.Count > 0)
            {
                DialogResult answer = MessageBox.Show(this,
                    "The filtered results contain " + bypassed.Count + " bypassed Album(s). Include them with a temporary override for this run?\r\n\r\nStored bypass history will not be deleted. Choosing No still selects other eligible filtered results.",
                    "Temporary bypass overrides", MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2);
                includeBypassed = answer == DialogResult.Yes;
            }

            foreach (AlbumInfo album in albums)
            {
                album.Selected = false;
                album.BypassOverride = false;
            }
            foreach (AlbumInfo album in visible)
            {
                if (album.State == AlbumState.New || album.State == AlbumState.Processed)
                    album.Selected = true;
                else if (album.State == AlbumState.Bypassed && includeBypassed)
                {
                    album.Selected = true;
                    album.BypassOverride = true;
                }
            }
            selectionMode = SelectionMode.Filtered;
            BuildTree();
            UpdateSelectionControls();
            return albums.Count(album => album.Selected);
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
                    default: color = palette.StatusWhite; break;
                }
                pair.Value.ForeColor = color;
                Label description;
                if (mediaStatusDescriptions.TryGetValue(pair.Key, out description))
                    description.ForeColor = color;
            }
            UpdateAutoLaunchButton();
        }

        private void UpdateAutoLaunchButton()
        {
            if (selectAndLaunch == null) return;
            bool modeChosen = filteredScanRead != null && filteredScanWrite != null
                && (filteredScanRead.Checked || filteredScanWrite.Checked);
            bool selectedActionable = SelectedAlbumsForAutoLaunch(albums).Count > 0;

            if (running && awaitingDecision)
            {
                selectAndLaunch.Text = "WAITING";
                selectAndLaunch.Tag = "waiting";
                selectAndLaunch.Enabled = true;
            }
            else if (running)
            {
                selectAndLaunch.Text = "STOP";
                selectAndLaunch.Tag = null;
                selectAndLaunch.Enabled = true;
            }
            else if (autoLaunchFinished)
            {
                selectAndLaunch.Text = "FINISHED";
                selectAndLaunch.Tag = null;
                selectAndLaunch.Enabled = false;
            }
            else
            {
                selectAndLaunch.Text = "AUTO LAUNCH";
                selectAndLaunch.Tag = "success";
                selectAndLaunch.Enabled = !loadingLibrary && modeChosen && selectedActionable;
            }
            ThemeManager.StyleButton(selectAndLaunch, uiState.Theme);
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
            parent.Controls.Add(rightSplit);

            TableLayoutPanel upper = new FluentCardTableLayoutPanel { Name = "scanActivityCard", Dock = DockStyle.Fill, Padding = new Padding(ThemeManager.Space12), RowCount = 2, ColumnCount = 1, VisualRole = CardVisualRole.Panel };
            upper.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
            upper.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            rightSplit.Panel1.Controls.Add(upper);
            upper.Controls.Add(BuildPanelTitle("Scan Activity and Decisions",
                "Live discovery, validation, provider results, and write/read outcomes appear here."), 0, 0);
            Panel activityCard = new FluentCardPanel { Name = "activityLogCard", Dock = DockStyle.Fill, Padding = new Padding(2), VisualRole = CardVisualRole.Log };
            activity = new RichTextBox { Dock = DockStyle.Fill, ReadOnly = true, BorderStyle = BorderStyle.None, DetectUrls = false, Font = new Font("Cascadia Mono", 9f) };
            activityCard.Controls.Add(activity);
            upper.Controls.Add(activityCard, 0, 1);

            TableLayoutPanel lower = new FluentCardTableLayoutPanel { Name = "artworkCandidatesCard", Dock = DockStyle.Fill, Padding = new Padding(ThemeManager.Space12), RowCount = 5, ColumnCount = 1, VisualRole = CardVisualRole.Panel };
            lower.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
            lower.RowStyles.Add(new RowStyle(SizeType.Absolute, 24));
            lower.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            lower.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            lower.RowStyles.Add(new RowStyle(SizeType.Absolute, 56));
            rightSplit.Panel2.Controls.Add(lower);
            lower.Controls.Add(BuildPanelTitle("Artwork Candidates and Preview",
                "Real provider candidates for the current album will appear below."), 0, 0);
            candidateContext = new Label { Text = "", Dock = DockStyle.Fill, AutoEllipsis = true };
            lower.Controls.Add(candidateContext, 0, 1);
            candidateCards = new WatermarkFlowLayoutPanel { Dock = DockStyle.Fill, AutoScroll = true, WrapContents = true, FlowDirection = FlowDirection.LeftToRight, Padding = new Padding(ThemeManager.Space8) };
            candidateCards.Resize += delegate { candidateCards.PerformLayout(); };
            lower.Controls.Add(candidateCards, 0, 2);
            fallbackActions = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, AutoSizeMode = AutoSizeMode.GrowAndShrink, WrapContents = false, Visible = false, Padding = new Padding(0, 3, 0, 3) };
            refineFallback = new FluentButton { Text = "Refine Fallback Search...", Width = 190, Height = 32, Enabled = false, Tag = "primary" };
            refineFallback.Click += RefineFallbackClicked;
            refineFallback.EnabledChanged += delegate { ThemeManager.StyleButton(refineFallback, uiState.Theme); };
            fallbackActions.Controls.Add(refineFallback);
            retryMusicBrainz = new FluentButton { Text = "Retry MusicBrainz", Width = 165, Height = 32, Enabled = false, Visible = false, Tag = "primary" };
            retryMusicBrainz.Click += RetryMusicBrainzClicked;
            retryMusicBrainz.EnabledChanged += delegate { ThemeManager.StyleButton(retryMusicBrainz, uiState.Theme); };
            fallbackActions.Controls.Add(retryMusicBrainz);
            fallbackActions.Controls.Add(new InfoButton("Edit the tagged Artist and Album search values, then rerun the current album through SPLINED fallback. This does not change the music files or bypass history."));
            lower.Controls.Add(fallbackActions, 0, 3);
            FlowLayoutPanel actions = new FlowLayoutPanel { Name = "candidateActionRow", Dock = DockStyle.Fill, WrapContents = false, Padding = new Padding(0, 5, 0, 7), Margin = new Padding(0) };
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
            actions.Controls.Add(launch); actions.Controls.Add(useSelected); actions.Controls.Add(keepLocal); actions.Controls.Add(compare); actions.Controls.Add(skip);
            enableHover = new FluentButton { Name = "enableHoverButton", Text = "Enable Hover", Width = 125, Height = 34, Enabled = false };
            enableHover.Click += delegate
            {
                uiState.HoverEnabled = !uiState.HoverEnabled;
                if (!uiState.HoverEnabled) CloseHoverPreview();
                SaveUiState();
                UpdateHoverButton();
            };
            actions.Controls.Add(enableHover);
            actions.Controls.Add(new InfoButton("The recommended candidate is selected first. Check one candidate to use it, check several to compare them, or skip the album. Processing continues after you confirm a choice."));
            lower.Controls.Add(actions, 0, 4);
        }

        private async Task ReloadLibraryAsync(bool preserveSelection)
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
            SetStatus("Loading library without blocking the window...");
            UpdateSelectionControls();
            try
            {
                List<AlbumInfo> loaded = await Task.Run(delegate { return LibraryInventory.Load(snapshot); });
                if (version != reloadVersion || IsDisposed) return;
                albums = loaded;
                foreach (AlbumInfo album in albums)
                {
                    bool selected;
                    bool safeState = album.State == AlbumState.New || album.State == AlbumState.Processed;
                    album.Selected = safeState && ((preserveSelection
                        && previouslySelected.TryGetValue(album.Path, out selected)
                        && selected) || persistedSelection.Contains(album.Path));
                    album.BypassOverride = false;
                }
                initialSelectionRestored = true;
                selectionMode = SelectionMode.Select;
                BuildTree();
                int selectedCount = albums.Count(album => album.Selected);
                SetStatus(albums.Count + " album folders loaded. " + (selectedCount == 0
                    ? "Select is ready; no albums are checked."
                    : selectedCount + " saved album selection(s) restored."));
            }
            catch (Exception error)
            {
                if (version != reloadVersion || IsDisposed) return;
                albums.Clear();
                BuildTree();
                SetStatus(error.Message);
            }
            finally
            {
                if (version == reloadVersion)
                {
                    loadingLibrary = false;
                    UpdateSelectionControls();
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
                bool anyAlbumCategoryVisible = new[] { MediaStatusFilter.White, MediaStatusFilter.Orange, MediaStatusFilter.Red, MediaStatusFilter.Purple }
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
                    TreeNode node = new TreeNode(album.Title) { Tag = album, Checked = album.Selected, ForeColor = HistoryResolver.StateColor(album.State, dark), ToolTipText = album.ToolTip };
                    artistNode.Nodes.Add(node);
                }
                tree.Nodes.Add(artistNode);
            }
            tree.EndUpdate();
            suppressTreeEvents = false;
            UpdateAutoLaunchButton();
        }

        private bool IsMediaStatusChecked(MediaStatusFilter filter)
        {
            CheckBox control;
            return !mediaStatusFilters.TryGetValue(filter, out control) || control.Checked;
        }

        private static MediaStatusFilter AlbumFilterCategory(AlbumState state)
        {
            if (state == AlbumState.Processed) return MediaStatusFilter.Orange;
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
            autoLaunchFinished = false;
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
            UpdateParentCheck(e.Node.Parent);
            UpdateSelectionControls();
        }

        private void TreeNodeMouseClick(object sender, TreeNodeMouseClickEventArgs e)
        {
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
            int count = albums.Count(album => album.Selected);
            bool waiting = running && awaitingDecision;
            launch.Enabled = running || (!loadingLibrary && count > 0);
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
            UpdateAutoLaunchButton();
            if (!running && !loadingLibrary) SetStatus(count == 0 ? "No albums selected. Launch is disabled." : count + " album(s) selected. Launch will process only these albums.");
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
            List<AlbumInfo> selected = albums.Where(album => album.Selected).ToList();
            if (selected.Count == 0) return;
            string core = FindCoreExecutable();
            if (core == null)
            {
                autoLaunchRun = false;
                MessageBox.Show(this, "The live SPLINED processing core was not found beside the GUI.", "SPLINED core missing", MessageBoxButtons.OK, MessageBoxIcon.Error);
                UpdateAutoLaunchButton();
                return;
            }
            string runConfigPath = state.ConfigPath;
            string temporaryConfigPath = null;
            activeRunMode = filteredScanRead != null && filteredScanRead.Checked
                ? "read"
                : filteredScanWrite != null && filteredScanWrite.Checked
                    ? "write"
                    : state.Mode;
            if (!String.Equals(activeRunMode, state.Mode, StringComparison.OrdinalIgnoreCase))
            {
                try
                {
                    ConfigState runState = state.Clone();
                    runState.Mode = activeRunMode;
                    temporaryConfigPath = Path.Combine(ConfigStore.AppRoot, "_cache", "gui-run-" + Guid.NewGuid().ToString("N") + ".toml");
                    ConfigStore.SaveTemporaryRunConfig(runState, temporaryConfigPath);
                    runConfigPath = temporaryConfigPath;
                }
                catch (Exception error)
                {
                    activeRunMode = null;
                    autoLaunchRun = false;
                    MessageBox.Show(this, "Unable to prepare the temporary filtered run mode.\r\n\r\n" + error.Message,
                        "Filtered scan mode", MessageBoxButtons.OK, MessageBoxIcon.Error);
                    UpdateAutoLaunchButton();
                    return;
                }
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

            for (int index = 0; index < selected.Count && !stopRequested; index++)
            {
                AlbumInfo album = selected[index];
                activeLaunchAlbum = album;
                fallbackMode = false;
                fallbackReason = "";
                fallbackArtist = "";
                fallbackAlbum = "";
                SetFallbackControls(false);
                progress.Value = (int)Math.Round(index * 100.0 / selected.Count);
                BeginAlbumRunStatistics(album);
                AppendAlbumActivityHeader(index + 1, selected.Count, album.Artist, album.Title);
                AppendActivity("  1. Reading local album and tag evidence...\r\n");
                await RunCoreAlbum(core, album, runConfigPath);
                // A launch queue is a snapshot. Once this album's process has
                // ended, consume its check even after STOP/error so it cannot
                // lead the next independently selected artist's run. Albums not
                // yet attempted remain checked and resumable.
                ConsumeLaunchAlbumSelection(album);
                FinishActiveAlbumStatistics(stopRequested ? "Stopped" : "Incomplete");
                activeLaunchAlbum = null;
                AppendActivity("\r\n");
            }

            if (!String.IsNullOrWhiteSpace(temporaryConfigPath))
            {
                try { if (File.Exists(temporaryConfigPath)) File.Delete(temporaryConfigPath); }
                catch { }
            }
            currentProcess = null;
            activeLaunchAlbum = null;
            running = false;
            string completedRunMode = activeRunMode;
            activeRunMode = null;
            if (autoLaunchRun) autoLaunchFinished = true;
            autoLaunchRun = false;
            ClearCandidates();
            candidateContext.Text = "";
            progress.Value = 100;
            progress.Visible = false;
            SetStatus(stopRequested ? "Stopped by user." : "Run complete. LAUNCH is ready for another selected run.");
            launch.Text = "LAUNCH";
            ApplyModeActivityAppearance();
            await ReloadLibraryAsync(true);
            if (!stopRequested && runAlbumStatistics.Count > 0)
                ShowAlbumRunReport(completedRunMode, selected.Count);
        }

        private async Task RunCoreAlbum(string core, AlbumInfo album, string runConfigPath)
        {
            string retryArtist = null;
            string retryAlbum = null;
            do
            {
                fallbackRetryRequested = false;
                musicBrainzRetryRequested = false;
                await RunCoreAlbumOnce(core, album, retryArtist, retryAlbum, runConfigPath);
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
        }

        private Task RunCoreAlbumOnce(string core, AlbumInfo album, string retryArtist, string retryAlbum, string runConfigPath)
        {
            TaskCompletionSource<bool> completion = new TaskCompletionSource<bool>();
            ProcessStartInfo start = new ProcessStartInfo();
            start.FileName = core;
            start.Arguments = "--config-path " + QuoteArgument(runConfigPath) + " --scan-dir-path " + QuoteArgument(album.Path);
            start.WorkingDirectory = ConfigStore.AppRoot;
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.WindowStyle = ProcessWindowStyle.Hidden;
            start.RedirectStandardOutput = true;
            start.RedirectStandardError = true;
            start.RedirectStandardInput = true;
            start.EnvironmentVariables["SPLINED_GUI_EVENTS"] = "1";
            start.EnvironmentVariables["SPLINED_GUI_REVIEW"] = "1";
            start.EnvironmentVariables["NO_COLOR"] = "1";
            if (!String.IsNullOrWhiteSpace(retryArtist)) start.EnvironmentVariables["SPLINED_FALLBACK_ARTIST"] = retryArtist;
            if (!String.IsNullOrWhiteSpace(retryAlbum)) start.EnvironmentVariables["SPLINED_FALLBACK_ALBUM"] = retryAlbum;
            if (album.BypassOverride) start.EnvironmentVariables["SPLINED_BYPASS_OVERRIDE"] = "1";
            Process process = new Process();
            process.StartInfo = start;
            process.EnableRaisingEvents = true;
            process.OutputDataReceived += delegate(object sender, DataReceivedEventArgs args) { if (args.Data != null) HandleCoreLine(args.Data, false); };
            process.ErrorDataReceived += delegate(object sender, DataReceivedEventArgs args) { if (args.Data != null) HandleCoreLine(args.Data, true); };
            process.Exited += delegate
            {
                completion.TrySetResult(true);
                process.Dispose();
            };
            try
            {
                currentProcess = process;
                process.Start();
                process.BeginOutputReadLine();
                process.BeginErrorReadLine();
            }
            catch (Exception error)
            {
                AppendActivitySafe("  ERROR: " + error.Message + "\r\n", ActivityTone.Error);
                completion.TrySetResult(false);
            }
            return completion.Task;
        }

        private void HandleCoreLine(string line, bool error)
        {
            if (line.StartsWith(EventPrefix, StringComparison.Ordinal))
            {
                string body = line.Substring(EventPrefix.Length);
                try
                {
                    Dictionary<string, object> payload = json.Deserialize<Dictionary<string, object>>(body);
                    BeginInvoke((MethodInvoker)delegate { ApplyCoreEvent(payload); });
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
            else if (eventName == "candidates")
            {
                fallbackMode = ReadBool(payload, "fallback") || fallbackMode;
                musicBrainzRetryAvailable = ReadBool(payload, "musicbrainz_retry_available");
                string eventFallbackReason = ReadString(payload, "fallback_reason");
                if (!String.IsNullOrWhiteSpace(eventFallbackReason)) fallbackReason = eventFallbackReason;
                object raw;
                if (payload.TryGetValue("items", out raw)) ShowCandidates(raw as object[] ?? ToObjectArray(raw));
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
                    if (activeAlbumStatistics != null)
                        activeAlbumStatistics.ProviderDiagnostics += diagnostics.Length;
                    foreach (object item in diagnostics)
                    {
                        Dictionary<string, object> diagnostic = item as Dictionary<string, object>;
                        if (diagnostic != null) AppendActivity("     " + ReadString(diagnostic, "source") + ": " + ReadString(diagnostic, "message") + "\r\n", ActivityTone.Warning);
                    }
                }
            }
            else if (eventName == "album_completed")
            {
                string action = ReadString(payload, "action");
                string destination = ReadString(payload, "destination");
                MarkActiveAlbumOutcome(String.IsNullOrWhiteSpace(action) ? "Completed" : action, destination);
                AppendActivity("  5. " + action + ": " + destination + "\r\n", ActivityTone.Success);
                CompleteAlbumEvent(payload, "Artwork choice completed. Candidate results cleared.");
            }
            else if (eventName == "album_postponed")
            {
                MarkActiveAlbumOutcome("Postponed", ReadString(payload, "reason"));
                AppendActivity("  Timeout active; album postponed by authoritative history.\r\n", ActivityTone.Warning);
                CompleteAlbumEvent(payload, "Album postponed. Candidate results cleared.");
            }
            else if (eventName == "album_skipped")
            {
                string skipReason = ReadString(payload, "reason");
                MarkActiveAlbumOutcome("Skipped", skipReason);
                AppendActivity("  Album skipped: " + skipReason + ".\r\n", ActivityTone.Warning);
                CompleteAlbumEvent(payload, "Album skipped. Candidate results cleared.");
            }
            else if (eventName == "decision_required")
            {
                if (ReadString(payload, "reason").Equals("fallback", StringComparison.OrdinalIgnoreCase)) fallbackMode = true;
                if (activeAlbumStatistics != null) activeAlbumStatistics.ReviewRequired = true;
                awaitingDecision = true;
                UpdateCandidateActions();
                skip.Enabled = true;
                UpdateSelectionControls();
                AppendActivity(fallbackMode
                    ? "  Fallback review required. The recommended exception follows Python fallback ranking; select one, compare several, or bypass.\r\n"
                    : "  Review required. Select one candidate to use, select several to compare, or skip this album.\r\n", ActivityTone.Accent);
            }
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
                view.Range = ReadString(candidate, "range_class");
                view.Acceptable = ReadBool(candidate, "acceptable");
                view.PolicyStatus = ReadString(candidate, "policy_status");
                view.PolicyReason = ReadString(candidate, "policy_reason");
                view.SourceOverrideActive = ReadBool(candidate, "source_override_active");
                view.Recommended = recommended;
                view.CachePath = ReadString(candidate, "cache_path");
                view.Url = ReadString(candidate, "url");
                view.LocalOrigin = ReadString(candidate, "local_origin");
                view.LocalReference = ReadString(candidate, "local_reference");
                candidates[candidateIndex] = view;
                if (recommended)
                {
                    recommendedCandidateIndex = candidateIndex;
                    compareCandidateIndexes.Add(candidateIndex);
                    selectedCandidateIndex = candidateIndex;
                }
                candidateCards.Controls.Add(BuildCandidateCard(view));
            }
            UpdateHoverButton();
            UpdateCandidateActions();
        }

        private Control BuildCandidateCard(CandidateView view)
        {
            Panel card = new FluentCardPanel
            {
                Width = 196,
                Height = 260,
                Margin = new Padding(ThemeManager.Space4),
                Padding = new Padding(1),
                Tag = view.Index,
                VisualRole = view.Recommended ? CardVisualRole.Recommended : CardVisualRole.Nested
            };
            bool sourceRejected = view.SourceOverrideActive
                && view.PolicyStatus.Equals("reject", StringComparison.OrdinalIgnoreCase);
            CheckBox choose = new FluentCheckBox
            {
                Left = 8,
                Top = 6,
                Width = 176,
                Height = 24,
                Text = view.Recommended ? "Recommended" : "Candidate " + view.Index,
                Checked = !sourceRejected && compareCandidateIndexes.Contains(view.Index),
                Enabled = !sourceRejected,
                Tag = view.Index
            };
            choose.CheckedChanged += delegate
            {
                if (choose.Checked) compareCandidateIndexes.Add(view.Index);
                else compareCandidateIndexes.Remove(view.Index);
                selectedCandidateIndex = compareCandidateIndexes.Count == 1 ? compareCandidateIndexes.First() : -1;
                UpdateCandidateActions();
            };
            card.Controls.Add(choose);
            PictureBox image = new PictureBox { Left = 17, Top = 34, Width = 160, Height = 150, SizeMode = PictureBoxSizeMode.Zoom, BorderStyle = BorderStyle.None };
            image.Image = LoadImageCopy(view.CachePath);
            image.Cursor = Cursors.Hand;
            image.MouseEnter += delegate { if (uiState.HoverEnabled) ShowHoverPreview(view); };
            image.MouseLeave += delegate { if (uiState.HoverEnabled) CloseHoverPreview(); };
            card.Controls.Add(image);
            Label source = new Label { Left = 8, Top = 188, Width = 180, Height = 20, Text = view.DisplaySource, AutoEllipsis = true };
            card.Controls.Add(source);
            LinkLabel sourceLink = CandidateUrlUi.Create(view.Resolution, view.Url, uiState.Theme);
            sourceLink.Left = 8; sourceLink.Top = 208; sourceLink.Width = 180; sourceLink.Height = 20;
            card.Controls.Add(sourceLink);
            bool fallbackOnly = view.PolicyStatus.Equals("fallback", StringComparison.OrdinalIgnoreCase);
            string qualityText = fallbackOnly
                ? view.Range + " - Fallback only"
                : sourceRejected
                    ? view.Range + " - Source policy reject"
                    : view.Range + (view.Acceptable ? " - Acceptable" : " - Outside range");
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            Color qualityColor = fallbackOnly ? palette.Warning : view.Acceptable ? palette.Success : palette.Error;
            Label quality = new Label { Left = 8, Top = 228, Width = 180, Height = 20, Text = qualityText, ForeColor = qualityColor, AutoEllipsis = true };
            card.Controls.Add(quality);
            if (!String.IsNullOrWhiteSpace(view.PolicyReason))
            {
                ToolTip policyTip = ThemeManager.CreateToolTip();
                policyTip.SetToolTip(quality, view.PolicyReason);
                quality.Tag = policyTip;
            }
            card.Click += delegate { if (choose.Enabled) choose.Checked = !choose.Checked; };
            ThemeManager.Apply(card, uiState.Theme);
            return card;
        }

        private void UpdateCandidateActions()
        {
            selectedCandidateIndex = compareCandidateIndexes.Count == 1 ? compareCandidateIndexes.First() : -1;
            useSelected.Enabled = awaitingDecision && selectedCandidateIndex >= 0;
            bool hasLocal = candidates.Values.Any(candidate => candidate.Source.Equals("local", StringComparison.OrdinalIgnoreCase) || candidate.Source.Equals("webpstill", StringComparison.OrdinalIgnoreCase));
            keepLocal.Visible = hasLocal;
            keepLocal.Enabled = awaitingDecision && hasLocal;
            compare.Enabled = candidates.Count > 1 && compareCandidateIndexes.Count > 1;
            skip.Enabled = awaitingDecision;
            refineFallback.Enabled = awaitingDecision && fallbackMode;
            retryMusicBrainz.Visible = fallbackMode && musicBrainzRetryAvailable;
            retryMusicBrainz.Enabled = awaitingDecision && fallbackMode && musicBrainzRetryAvailable;
            ThemeManager.StyleButton(useSelected, uiState.Theme);
            ThemeManager.StyleButton(keepLocal, uiState.Theme);
            ThemeManager.StyleButton(refineFallback, uiState.Theme);
            ThemeManager.StyleButton(retryMusicBrainz, uiState.Theme);
        }

        private void SetFallbackControls(bool visible)
        {
            if (fallbackActions == null) return;
            fallbackActions.Visible = visible;
            refineFallback.Enabled = visible && awaitingDecision;
            retryMusicBrainz.Visible = visible && musicBrainzRetryAvailable;
            retryMusicBrainz.Enabled = visible && awaitingDecision && musicBrainzRetryAvailable;
            ThemeManager.StyleButton(refineFallback, uiState.Theme);
            ThemeManager.StyleButton(retryMusicBrainz, uiState.Theme);
        }

        private void ClearCandidates()
        {
            CloseHoverPreview();
            foreach (Control control in candidateCards.Controls)
            {
                foreach (PictureBox picture in control.Controls.OfType<PictureBox>()) if (picture.Image != null) picture.Image.Dispose();
                control.Dispose();
            }
            candidateCards.Controls.Clear();
            candidateCards.AutoScrollPosition = Point.Empty;
            candidates.Clear();
            compareCandidateIndexes.Clear();
            selectedCandidateIndex = -1;
            recommendedCandidateIndex = -1;
            awaitingDecision = false;
            useSelected.Enabled = false;
            keepLocal.Enabled = false;
            keepLocal.Visible = false;
            compare.Enabled = false;
            skip.Enabled = false;
            UpdateHoverButton();
            UpdateSelectionControls();
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
            if (completed == null && activeLaunchAlbum != null
                && (String.IsNullOrWhiteSpace(albumPath) || SameAlbumPath(activeLaunchAlbum.Path, albumPath)))
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
            try { return Path.GetFullPath(normalized); }
            catch { return normalized; }
        }

        private void UseSelectedClicked(object sender, EventArgs e)
        {
            if (selectedCandidateIndex < 0 || currentProcess == null) return;
            ConfirmAndUseCandidate(selectedCandidateIndex);
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
            candidateContext.Text = "Retrying the current album's MusicBrainz release lookup...";
            ClearCandidates();
            SetFallbackControls(false);
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
                string effect = keepingLocal
                    ? "Keep the existing local artwork and continue without replacing it?"
                    : state.Mode.Equals("write", StringComparison.OrdinalIgnoreCase)
                        ? "Write this artwork using the active Config v5 output rules?"
                        : "Use this artwork for the Read-mode review sample? Album artwork will not be changed.";
                DialogResult answer = MessageBox.Show(this,
                    candidate.DisplaySource + "  " + candidate.Resolution + "\r\n\r\n" + effect,
                    "Confirm artwork selection", MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2);
                if (answer != DialogResult.Yes) return;
            }
            SendDecision("{\"action\":\"use\",\"index\":" + index + "}");
            candidateContext.Text = "Artwork selection confirmed. Moving to the next selected album...";
            ClearCandidates();
        }

        private void ShowHoverPreview(CandidateView candidate)
        {
            CloseHoverPreview();
            hoverPreview = new HoverPreviewForm(candidate, uiState.Theme, uiState);
            hoverPreview.FormClosed += delegate { hoverPreview = null; };
            hoverPreview.Show(this);
        }

        private void CloseHoverPreview()
        {
            if (hoverPreview != null && !hoverPreview.IsDisposed) hoverPreview.Close();
            hoverPreview = null;
        }

        private void StopRun()
        {
            stopRequested = true;
            SetStatus("Stopping current processing...");
            ClearCandidates();
            candidateContext.Text = "Processing stopped. Candidate results cleared.";
            try { if (currentProcess != null && !currentProcess.HasExited) currentProcess.Kill(); }
            catch { }
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

        private void CheckForUpdateClicked(object sender, EventArgs e)
        {
            try
            {
                DialogResult open = MessageBox.Show(this,
                    "Installed: " + ReleaseInfo.VersionLabel
                    + "\r\n\r\nOpen the official SPLINED releases page to check for updates?",
                    "SPLINED update check", MessageBoxButtons.YesNo, MessageBoxIcon.Information);
                if (open == DialogResult.Yes)
                    Process.Start(new ProcessStartInfo { FileName = ReleaseInfo.ReleasesUrl, UseShellExecute = true });
            }
            catch (Exception error)
            {
                MessageBox.Show(this,
                    "SPLINED could not open the releases page.\r\n\r\n"
                    + ReleaseInfo.ReleasesUrl + "\r\n\r\n" + error.Message,
                    "SPLINED update check", MessageBoxButtons.OK, MessageBoxIcon.Warning);
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
                statusStrip.BackColor = palette.TopNavigationSurface;
                statusStrip.ForeColor = palette.TextSecondary;
                UpdateMediaFilterColors();
                UpdateHoverButton();
                UpdateSelectionControls();
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
            if (rightSplit != null && rightSplit.SplitterDistance >= rightSplit.Panel1MinSize)
                uiState.RightSplitterDistance = rightSplit.SplitterDistance;
            if (mediaFilterPanel != null) uiState.MediaFilterExpanded = mediaFilterPanel.Visible;
            if (artistFilter != null) uiState.MediaArtistFilter = artistFilter.Text;
            if (albumFilter != null) uiState.MediaAlbumFilter = albumFilter.Text;
            uiState.MediaShowWhite = IsMediaStatusChecked(MediaStatusFilter.White);
            uiState.MediaShowOrange = IsMediaStatusChecked(MediaStatusFilter.Orange);
            uiState.MediaShowRed = IsMediaStatusChecked(MediaStatusFilter.Red);
            uiState.MediaShowPurple = IsMediaStatusChecked(MediaStatusFilter.Purple);
            uiState.MediaShowGreen = IsMediaStatusChecked(MediaStatusFilter.Green);
            uiState.MediaShowBlue = IsMediaStatusChecked(MediaStatusFilter.Blue);
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

        private void MarkActiveAlbumOutcome(string action, string destination)
        {
            if (activeAlbumStatistics == null) return;
            activeAlbumStatistics.Action = String.IsNullOrWhiteSpace(action) ? "Completed" : action;
            activeAlbumStatistics.Destination = destination ?? "";
            activeAlbumStatistics.FinishedUtc = DateTime.UtcNow;
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
            AppendActivity("  •  Albums processed: ", ActivityTone.Muted);
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

        private static Image LoadImageCopy(string path)
        {
            try
            {
                byte[] bytes = File.ReadAllBytes(path);
                using (MemoryStream stream = new MemoryStream(bytes))
                using (Image source = Image.FromStream(stream)) return new Bitmap(source);
            }
            catch { return null; }
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
        public string Source;
        public string Resolution;
        public int PixelWidth;
        public int PixelHeight;
        public string Range;
        public bool Acceptable;
        public string PolicyStatus = "";
        public string PolicyReason = "";
        public bool SourceOverrideActive;
        public bool Recommended;
        public string CachePath;
        public string Url;
        public string LocalOrigin = "";
        public string LocalReference = "";
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
        public static LinkLabel Create(string leadingText, string url, string theme)
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
                        Process.Start(new ProcessStartInfo { FileName = Convert.ToString(args.Link.LinkData), UseShellExecute = true });
                    }
                    catch (Exception error)
                    {
                        MessageBox.Show(label.FindForm(), error.Message, "Unable to open artwork source", MessageBoxButtons.OK, MessageBoxIcon.Error);
                    }
                };
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
