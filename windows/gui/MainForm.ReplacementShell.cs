using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    /// <summary>
    /// Active Windows artwork-workspace composition. Existing scan, candidate,
    /// MusicBrainz and write behavior remains in MainForm; this partial class
    /// replaces only the production shell and presentation adapters.
    /// </summary>
    internal sealed partial class MainForm
    {
        private bool replacementShellActive;
        private Panel replacementHeader;
        private Panel replacementLibrarySidebar;
        private FlowLayoutPanel replacementLibraryEntities;
        private TextBox replacementLibrarySearch;
        private Label replacementLibraryStatus;
        private readonly Dictionary<LibraryNavigationMode, Button> replacementLibraryTabs =
            new Dictionary<LibraryNavigationMode, Button>();
        private LibraryNavigationMode replacementLibraryMode;
        private string replacementArtistScope = "";
        private TableLayoutPanel replacementWorkspace;
        private FlowLayoutPanel replacementModeTabs;
        private Panel replacementModeContent;
        private readonly Dictionary<ArtworkWorkspaceMode, Button> replacementModeButtons =
            new Dictionary<ArtworkWorkspaceMode, Button>();
        private ArtworkWorkspaceMode replacementMode;
        private HorizontalWheelFlowPanel replacementSourceStrip;
        private NeonCardPanel replacementFiltersCard;
        private NeonCardPanel replacementEditingCard;
        private Panel replacementFiltersBody;
        private Panel replacementEditingBody;
        private Button replacementFiltersToggle;
        private Button replacementEditingToggle;
        private readonly Dictionary<string, TriStateFilterControl> replacementTriStateFilters =
            new Dictionary<string, TriStateFilterControl>(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, ModernSpectrumSlider> replacementAdjustmentSliders =
            new Dictionary<string, ModernSpectrumSlider>(StringComparer.OrdinalIgnoreCase);
        private Label replacementCandidateHeading;
        private Label replacementCandidateSummary;
        private Label replacementArtworkBadge;
        private Label replacementQualityBadge;
        private Button replacementCustomUpscale;
        private string replacementDraggedSource = "";
        private RichTextBox replacementActivityLog;
        private Control replacementMusicBrainzHost;

        private void BuildLayout()
        {
            replacementShellActive = true;
            replacementLibraryMode = ParseLibraryMode(uiState.LibraryNavigationMode);
            replacementMode = ParseWorkspaceMode(uiState.ArtworkWorkspaceMode);
            SuspendLayout();
            ThemePalette palette = ThemeManager.CurrentPalette;
            BackColor = palette.WindowBackground;
            Padding = Padding.Empty;

            MenuStrip menu = BuildMenu();
            menu.Visible = false;
            MainMenuStrip = menu;
            Controls.Add(menu);
            BuildReplacementStatusBar();
            replacementHeader = BuildReplacementHeader();
            Controls.Add(replacementHeader);

            mainSplit = new SplitContainer
            {
                Name = "replacementMainSplit",
                Dock = DockStyle.Fill,
                Orientation = Orientation.Vertical,
                SplitterWidth = 1,
                FixedPanel = FixedPanel.Panel1,
                BackColor = palette.BorderSubtle
            };
            mainSplit.Size = new Size(Math.Max(980, ClientSize.Width), Math.Max(600, ClientSize.Height));
            mainSplit.Panel1MinSize = 230;
            mainSplit.Panel2MinSize = 600;
            int libraryWidth = Math.Max(230, Math.Min(310,
                uiState.MainSplitterDistance > 0 ? uiState.MainSplitterDistance : 272));
            TrySetSplitterDistance(mainSplit, libraryWidth);
            mainSplit.SplitterMoved += delegate
            {
                if (!applyingLayoutPreset) uiState.LayoutPreset = "Custom";
                QueueUpscaleResponsiveLayout();
            };
            Controls.Add(mainSplit);
            mainSplit.BringToFront();

            BuildReplacementLibrary(mainSplit.Panel1);
            BuildReplacementWorkspace(mainSplit.Panel2);
            BuildReplacementCompatibilityControls();
            ShowReplacementMode(replacementMode, false);
            ResumeLayout(true);
        }

        private void BuildReplacementStatusBar()
        {
            statusStrip = new StatusStrip
            {
                Name = "replacementStatusBar",
                Dock = DockStyle.Bottom,
                SizingGrip = false,
                Height = 24
            };
            statusLabel = new ToolStripStatusLabel("Ready")
            {
                Spring = true,
                TextAlign = ContentAlignment.MiddleLeft
            };
            progress = new ToolStripProgressBar
            {
                Width = 180,
                Minimum = 0,
                Maximum = 100,
                Visible = false
            };
            statusStrip.Items.Add(statusLabel);
            statusStrip.Items.Add(progress);
            statusStrip.Items.Add(new ToolStripStatusLabel("SPLINED  " + ReleaseInfo.NumericVersion));
            Controls.Add(statusStrip);
        }

        private Panel BuildReplacementHeader()
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            Panel header = new Panel
            {
                Name = "replacementAppBar",
                Dock = DockStyle.Top,
                Height = 62,
                Padding = new Padding(14, 6, 14, 6),
                BackColor = palette.TitlebarBackground,
                Tag = "titlebar-surface"
            };
            TableLayoutPanel row = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 4,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 300));
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 360));
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 220));

            TableLayoutPanel brand = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            brand.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 72));
            brand.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            Bitmap repositoryLogo = EmbeddedAssets.LoadWatermark();
            PictureBox logo = new PictureBox
            {
                Name = "replacementBrandLogo",
                Dock = DockStyle.Fill,
                Image = repositoryLogo,
                SizeMode = PictureBoxSizeMode.Zoom,
                Margin = new Padding(0, 0, 8, 0),
                AccessibleName = "SPLINED logo"
            };
            header.Disposed += delegate
            {
                if (logo.Image != null) logo.Image.Dispose();
                logo.Image = null;
            };
            brand.Controls.Add(logo, 0, 0);
            TableLayoutPanel brandText = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                RowCount = 2,
                ColumnCount = 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            brandText.RowStyles.Add(new RowStyle(SizeType.Percent, 66));
            brandText.RowStyles.Add(new RowStyle(SizeType.Percent, 34));
            brandText.Controls.Add(new SplinedWordmark { Dock = DockStyle.Fill, Margin = Padding.Empty }, 0, 0);
            brandText.Controls.Add(new Label
            {
                Dock = DockStyle.Fill,
                Text = "MUSIC LOOKS BETTER HERE",
                Font = ThemeManager.UiFont(ThemeFontRole.Minor, FontStyle.Bold),
                ForeColor = palette.TextSecondary,
                TextAlign = ContentAlignment.TopLeft,
                Margin = Padding.Empty
            }, 0, 1);
            brand.Controls.Add(brandText, 1, 0);
            row.Controls.Add(brand, 0, 0);

            FluentTextBox globalSearch = new FluentTextBox
            {
                Name = "globalLibrarySearch",
                Dock = DockStyle.Fill,
                Margin = new Padding(34, 7, 34, 7),
                Font = ThemeManager.UiFont(ThemeFontRole.Control),
                CueText = "Search artists, albums, or folders..."
            };
            globalSearch.AccessibleDescription = "Search artists, albums, or folders";
            globalSearch.TextChanged += delegate
            {
                if (replacementLibrarySearch != null && replacementLibrarySearch.Text != globalSearch.Text)
                    replacementLibrarySearch.Text = globalSearch.Text;
            };
            row.Controls.Add(globalSearch, 1, 0);

            FlowLayoutPanel navigation = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                Margin = Padding.Empty,
                Padding = new Padding(0, 4, 0, 4)
            };
            Button home = ReplacementNavButton("⌂  Home", 82);
            home.Click += delegate { ShowReplacementMode(ArtworkWorkspaceMode.FindMatch, true); };
            Button scan = ReplacementNavButton("⌗  Scan", 82);
            scan.Click += ReplacementScanClicked;
            Button settings = ReplacementNavButton("⚙  Settings", 104);
            settings.Click += OpenSettings;
            Button help = ReplacementNavButton("?  Help", 78);
            help.Click += delegate { HelpWindows.OpenHelp(this); };
            navigation.Controls.Add(home);
            navigation.Controls.Add(scan);
            navigation.Controls.Add(settings);
            navigation.Controls.Add(help);
            row.Controls.Add(navigation, 2, 0);
            row.Controls.Add(new Label
            {
                Dock = DockStyle.Fill,
                Text = "MANAGE · ENHANCE · PRESERVE\r\nMUSIC FOREVER",
                Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                ForeColor = palette.TextSecondary,
                TextAlign = ContentAlignment.MiddleRight
            }, 3, 0);
            header.Controls.Add(row);
            return header;
        }

        private Button ReplacementNavButton(string text, int width)
        {
            Button button = new FluentButton
            {
                Text = text,
                Width = width,
                Height = 42,
                Margin = new Padding(2, 0, 2, 0),
                FlatStyle = FlatStyle.Flat
            };
            ThemeManager.StyleButton(button, uiState.Theme);
            return button;
        }

        private void ReplacementScanClicked(object sender, EventArgs e)
        {
            if (running)
            {
                LaunchClicked(sender, e);
                return;
            }
            ContextMenuStrip choices = new ContextMenuStrip();
            ToolStripMenuItem read = new ToolStripMenuItem("READ · inspect source results");
            ToolStripMenuItem write = new ToolStripMenuItem("LIVE WRITE · apply selected artwork");
            read.Click += delegate
            {
                filteredScanRead.Checked = true;
                filteredScanWrite.Checked = false;
                LaunchClicked(sender, e);
            };
            write.Click += delegate
            {
                filteredScanWrite.Checked = true;
                filteredScanRead.Checked = false;
                LaunchClicked(sender, e);
            };
            choices.Items.Add(read);
            choices.Items.Add(write);
            Control control = sender as Control;
            choices.Show(control ?? this, new Point(0, control == null ? 0 : control.Height));
        }

        private void BuildReplacementLibrary(Control parent)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            replacementLibrarySidebar = new Panel
            {
                Name = "replacementMediaLibrary",
                Dock = DockStyle.Fill,
                Padding = new Padding(10, 8, 10, 8),
                BackColor = palette.SurfacePrimary
            };
            parent.Controls.Add(replacementLibrarySidebar);
            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 6,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 40));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 44));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 126));
            replacementLibrarySidebar.Controls.Add(layout);

            TableLayoutPanel title = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            title.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            title.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 34));
            title.Controls.Add(new Label
            {
                Text = "▤  Media Library",
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                TextAlign = ContentAlignment.MiddleLeft
            }, 0, 0);
            Button collapse = new FluentButton { Text = "⌄", Dock = DockStyle.Fill, Margin = new Padding(2) };
            collapse.Click += delegate { SetMediaSelectorVisible(!uiState.ShowMediaSelector, true); };
            title.Controls.Add(collapse, 1, 0);
            layout.Controls.Add(title, 0, 0);

            Button library = new FluentButton
            {
                Text = "♫  Main Music Library                                      ⌄",
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.MiddleLeft,
                Margin = new Padding(0, 2, 0, 4)
            };
            library.Click += OpenSettings;
            layout.Controls.Add(library, 0, 1);

            TableLayoutPanel tabs = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 3,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            for (int index = 0; index < 3; index++) tabs.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 33.333f));
            AddReplacementLibraryTab(tabs, LibraryNavigationMode.Artists, "Artists", 0);
            AddReplacementLibraryTab(tabs, LibraryNavigationMode.Albums, "Albums", 1);
            AddReplacementLibraryTab(tabs, LibraryNavigationMode.Folders, "Folders", 2);
            layout.Controls.Add(tabs, 0, 2);

            replacementLibrarySearch = new FluentTextBox
            {
                Name = "replacementLibrarySearch",
                Dock = DockStyle.Fill,
                Margin = new Padding(0, 5, 0, 5),
                Font = ThemeManager.UiFont(ThemeFontRole.Control),
                CueText = "Search artists..."
            };
            replacementLibrarySearch.AccessibleDescription = "Search library";
            replacementLibrarySearch.TextChanged += delegate { RefreshReplacementLibraryEntities(); };
            layout.Controls.Add(replacementLibrarySearch, 0, 3);

            replacementLibraryEntities = new FlowLayoutPanel
            {
                Name = "replacementLibraryEntities",
                Dock = DockStyle.Fill,
                AutoScroll = true,
                FlowDirection = FlowDirection.TopDown,
                WrapContents = false,
                Margin = Padding.Empty,
                Padding = new Padding(0, 2, 0, 2)
            };
            replacementLibraryEntities.SizeChanged += delegate { SizeReplacementLibraryRows(); };
            layout.Controls.Add(replacementLibraryEntities, 0, 4);

            NeonCardPanel status = new NeonCardPanel
            {
                Name = "replacementLibraryStatusCard",
                Dock = DockStyle.Fill,
                SpectrumBorder = false,
                Margin = new Padding(0, 5, 0, 0),
                Padding = new Padding(10)
            };
            replacementLibraryStatus = new Label
            {
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                TextAlign = ContentAlignment.MiddleLeft,
                AutoEllipsis = true
            };
            status.Controls.Add(replacementLibraryStatus);
            layout.Controls.Add(status, 0, 5);
            mediaFilterPanel = replacementLibrarySidebar;
        }

        private void AddReplacementLibraryTab(TableLayoutPanel owner,
            LibraryNavigationMode mode, string text, int column)
        {
            Button button = new FluentButton
            {
                Text = text,
                Dock = DockStyle.Fill,
                Margin = new Padding(column == 0 ? 0 : 2, 2, column == 2 ? 0 : 2, 2),
                Tag = mode
            };
            button.Click += delegate
            {
                replacementLibraryMode = mode;
                replacementArtistScope = "";
                uiState.LibraryNavigationMode = mode.ToString();
                SaveUiState();
                RefreshReplacementLibraryTabs();
                RefreshReplacementLibraryEntities();
            };
            replacementLibraryTabs[mode] = button;
            owner.Controls.Add(button, column, 0);
            RefreshReplacementLibraryTabs();
        }

        private void BuildReplacementWorkspace(Control parent)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            rightSplit = new SplitContainer
            {
                Name = "replacementWorkspaceSplit",
                Dock = DockStyle.Fill,
                Orientation = Orientation.Horizontal,
                SplitterWidth = 6,
                BackColor = palette.BorderSubtle
            };
            rightSplit.Size = new Size(Math.Max(780, parent.ClientSize.Width), Math.Max(650, parent.ClientSize.Height));
            rightSplit.Panel1MinSize = 280;
            rightSplit.Panel2MinSize = 180;
            int desiredCandidateHeight = rightSplit.Height <= 760 ? 186
                : Math.Max(218, Math.Min(236, rightSplit.Height / 4));
            int preferredTop = rightSplit.Height - desiredCandidateHeight - rightSplit.SplitterWidth;
            int maximumTop = rightSplit.Height - rightSplit.Panel2MinSize - rightSplit.SplitterWidth;
            if (uiState.RightSplitterDistance >= (int)Math.Round(rightSplit.Height * 0.55)
                && uiState.RightSplitterDistance <= maximumTop)
                preferredTop = uiState.RightSplitterDistance;
            TrySetSplitterDistance(rightSplit, Math.Max(rightSplit.Panel1MinSize,
                Math.Min(maximumTop, preferredTop)));
            rightSplit.SplitterMoved += delegate
            {
                uiState.RightSplitterDistance = rightSplit.SplitterDistance;
                QueueUpscaleResponsiveLayout();
            };
            parent.Controls.Add(rightSplit);

            activityArtworkSplit = new SplitContainer
            {
                Name = "replacementContentPreviewSplit",
                Dock = DockStyle.Fill,
                Orientation = Orientation.Vertical,
                SplitterWidth = 6,
                BackColor = palette.BorderSubtle
            };
            activityArtworkSplit.Size = new Size(Math.Max(900, rightSplit.Panel1.ClientSize.Width),
                Math.Max(400, rightSplit.Panel1.ClientSize.Height));
            activityArtworkSplit.Panel1MinSize = 420;
            activityArtworkSplit.Panel2MinSize = 240;
            int previewWidth = Math.Max(300, Math.Min(450,
                uiState.ArtworkPreviewWidth > 0 ? uiState.ArtworkPreviewWidth : 410));
            TrySetSplitterDistance(activityArtworkSplit,
                Math.Max(activityArtworkSplit.Panel1MinSize,
                    activityArtworkSplit.Width - previewWidth - activityArtworkSplit.SplitterWidth));
            activityArtworkSplit.SplitterMoved += delegate
            {
                uiState.ArtworkPreviewWidth = Math.Max(300, activityArtworkSplit.Panel2.Width);
                artworkPreviewRestorePending = false;
                QueueUpscaleResponsiveLayout();
            };
            rightSplit.Panel1.Controls.Add(activityArtworkSplit);

            replacementWorkspace = new TableLayoutPanel
            {
                Name = "replacementArtworkWorkspace",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 2,
                Padding = new Padding(8, 7, 4, 4),
                Margin = Padding.Empty
            };
            replacementWorkspace.RowStyles.Add(new RowStyle(SizeType.Absolute, 54));
            replacementWorkspace.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            activityArtworkSplit.Panel1.Controls.Add(replacementWorkspace);
            replacementModeTabs = BuildReplacementModeTabs();
            replacementWorkspace.Controls.Add(replacementModeTabs, 0, 0);
            replacementModeContent = new Panel
            {
                Name = "replacementModeContent",
                Dock = DockStyle.Fill,
                Margin = Padding.Empty,
                Padding = new Padding(0, 6, 0, 0)
            };
            activityContentHost = replacementModeContent;
            replacementWorkspace.Controls.Add(replacementModeContent, 0, 1);
            BuildReplacementArtworkPreview(activityArtworkSplit.Panel2);
            BuildReplacementCandidateStrip(rightSplit.Panel2);
        }

        private FlowLayoutPanel BuildReplacementModeTabs()
        {
            FlowLayoutPanel tabs = new FlowLayoutPanel
            {
                Name = "replacementModeTabs",
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                Margin = Padding.Empty,
                Padding = new Padding(0, 2, 0, 2)
            };
            AddReplacementModeTab(tabs, ArtworkWorkspaceMode.FindMatch, "⌕  Find & Match Artwork", 240);
            AddReplacementModeTab(tabs, ArtworkWorkspaceMode.Edit, "✎  Edit Artwork", 190);
            AddReplacementModeTab(tabs, ArtworkWorkspaceMode.Upscale, "✦  Upscale / Enhance", 220);
            AddReplacementModeTab(tabs, ArtworkWorkspaceMode.Advanced, "☷  Advanced", 170);
            tabs.SizeChanged += delegate { ResizeReplacementModeTabs(); };
            return tabs;
        }

        private void ResizeReplacementModeTabs()
        {
            if (replacementModeTabs == null || replacementModeTabs.Controls.Count == 0) return;
            int usable = Math.Max(400, replacementModeTabs.ClientSize.Width
                - replacementModeTabs.Padding.Horizontal - 8);
            int width = Math.Max(96, usable / replacementModeTabs.Controls.Count);
            foreach (Button button in replacementModeTabs.Controls.OfType<Button>())
                button.Width = width;
        }

        private void AddReplacementModeTab(FlowLayoutPanel owner, ArtworkWorkspaceMode mode,
            string text, int width)
        {
            Button button = new FluentButton
            {
                Text = text,
                Width = width,
                Height = 44,
                Margin = new Padding(0, 0, 2, 0),
                Tag = mode,
                Font = ThemeManager.UiFont(ThemeFontRole.Control)
            };
            button.Click += delegate { ShowReplacementMode(mode, true); };
            replacementModeButtons[mode] = button;
            owner.Controls.Add(button);
        }

        private void BuildReplacementArtworkPreview(Control parent)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            NeonCardPanel card = new NeonCardPanel
            {
                Name = "selectedArtworkCard",
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Padding = new Padding(12),
                Margin = new Padding(4, 7, 8, 4)
            };
            artworkPreviewCard = card;
            parent.Controls.Add(card);
            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 4,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 62));
            card.Controls.Add(layout);

            TableLayoutPanel heading = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 3,
                RowCount = 1,
                Margin = Padding.Empty
            };
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            artworkPreviewTitle = new Label
            {
                Text = "●  Selected Artwork",
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                ForeColor = palette.TextPrimary,
                TextAlign = ContentAlignment.MiddleLeft
            };
            replacementArtworkBadge = ReplacementBadge("—", palette.TextSecondary);
            replacementQualityBadge = ReplacementBadge("Awaiting selection", palette.Success);
            heading.Controls.Add(artworkPreviewTitle, 0, 0);
            heading.Controls.Add(replacementArtworkBadge, 1, 0);
            heading.Controls.Add(replacementQualityBadge, 2, 0);
            layout.Controls.Add(heading, 0, 0);

            Panel imageHost = new Panel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(8),
                Margin = new Padding(0, 4, 0, 4),
                BackColor = palette.TreeSurface
            };
            artworkPreviewImage = new PictureBox
            {
                Name = "artworkPreviewImage",
                Dock = DockStyle.Fill,
                SizeMode = PictureBoxSizeMode.Zoom,
                BorderStyle = BorderStyle.None,
                BackColor = palette.TreeSurface,
                Cursor = Cursors.Default
            };
            imageHost.Controls.Add(artworkPreviewImage);

            replacementCustomUpscale = new FluentButton
            {
                Name = "openCustomUpscale",
                Text = "↗  Open Custom Upscale",
                Anchor = AnchorStyles.Top | AnchorStyles.Right,
                Size = new Size(205, 36),
                Location = new Point(Math.Max(8, imageHost.ClientSize.Width - 217), 12),
                Tag = "primary"
            };
            replacementCustomUpscale.Click += ArtworkPreviewClicked;
            imageHost.Controls.Add(replacementCustomUpscale);
            replacementCustomUpscale.BringToFront();
            imageHost.SizeChanged += delegate
            {
                replacementCustomUpscale.Location = new Point(
                    Math.Max(8, imageHost.ClientSize.Width - replacementCustomUpscale.Width - 12), 12);
            };
            layout.Controls.Add(imageHost, 0, 1);

            FlowLayoutPanel zoom = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                Padding = new Padding(0, 2, 0, 2),
                Margin = Padding.Empty
            };
            foreach (string label in new[] { "−", "100%", "+", "Fit", "⛶" })
            {
                Button button = new FluentButton { Text = label, Width = label == "100%" ? 70 : 48, Height = 30 };
                button.Enabled = label == "Fit" || label == "⛶";
                zoom.Controls.Add(button);
            }
            layout.Controls.Add(zoom, 0, 2);
            artworkPreviewCaption = new Label
            {
                Name = "artworkPreviewCaption",
                Text = "No artwork selected",
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.TopLeft,
                ForeColor = palette.TextSecondary,
                AutoEllipsis = true,
                Padding = new Padding(2, 8, 2, 0)
            };
            layout.Controls.Add(artworkPreviewCaption, 0, 3);
            artworkPreviewImage.Click += ArtworkPreviewClicked;
            artworkPreviewTitle.Click += ArtworkPreviewClicked;
            artworkPreviewCaption.Click += ArtworkPreviewClicked;
            card.Click += ArtworkPreviewClicked;
        }

        private Label ReplacementBadge(string text, Color color)
        {
            return new Label
            {
                Text = text,
                AutoSize = true,
                ForeColor = color,
                TextAlign = ContentAlignment.MiddleCenter,
                Margin = new Padding(5, 8, 5, 6),
                Padding = new Padding(8, 2, 8, 2),
                Font = ThemeManager.UiFont(ThemeFontRole.Minor, FontStyle.Bold)
            };
        }

        private void BuildReplacementCandidateStrip(Control parent)
        {
            NeonCardPanel card = new NeonCardPanel
            {
                Name = "replacementCandidateArtwork",
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Padding = new Padding(8),
                Margin = new Padding(4, 2, 8, 8)
            };
            parent.Controls.Add(card);
            candidateLayout = new TableLayoutPanel
            {
                Name = "replacementCandidateLayout",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 3,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            candidateLayout.RowStyles.Add(new RowStyle(SizeType.Absolute, 34));
            candidateLayout.RowStyles.Add(new RowStyle(SizeType.Absolute, 18));
            candidateLayout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            card.Controls.Add(candidateLayout);
            TableLayoutPanel heading = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = Padding.Empty
            };
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            replacementCandidateHeading = new Label
            {
                Text = "⌕  Candidate Artwork",
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                TextAlign = ContentAlignment.MiddleLeft
            };
            replacementCandidateSummary = new Label
            {
                Text = "No candidates",
                AutoSize = true,
                TextAlign = ContentAlignment.MiddleRight,
                Margin = new Padding(8, 12, 8, 0)
            };
            heading.Controls.Add(replacementCandidateHeading, 0, 0);
            heading.Controls.Add(replacementCandidateSummary, 1, 0);
            candidateLayout.Controls.Add(heading, 0, 0);
            candidateContext = new Label
            {
                Text = "Select an album and start a scan to discover artwork.",
                Dock = DockStyle.Fill,
                AutoEllipsis = true,
                ForeColor = ThemeManager.CurrentPalette.TextSecondary
            };
            candidateLayout.Controls.Add(candidateContext, 0, 1);
            candidateCards = new HorizontalWheelFlowPanel
            {
                Name = "replacementCandidateStrip",
                Dock = DockStyle.Fill,
                Padding = new Padding(4, 2, 4, 2),
                Margin = Padding.Empty
            };
            candidateCards.Resize += delegate { ResizeCandidateCards(); };
            candidateLayout.Controls.Add(candidateCards, 0, 2);
        }

        private void BuildReplacementCompatibilityControls()
        {
            artistFilter = replacementLibrarySearch;
            albumFilter = new TextBox { Visible = false };
            tree = new TreeView
            {
                Name = "compatibilityLibraryTree",
                CheckBoxes = true,
                Visible = false,
                Size = new Size(1, 1)
            };
            tree.AfterCheck += TreeAfterCheck;
            tree.AfterSelect += TreeAfterSelect;
            tree.NodeMouseClick += TreeNodeMouseClick;
            tree.NodeMouseHover += TreeNodeMouseHover;
            tree.BeforeExpand += TreeBeforeExpand;
            treeToolTip = CreateOwnedToolTip();
            replacementLibrarySidebar.Controls.Add(tree);
            filteredScanRead = new FluentCheckBox { Name = "filteredScanRead", Visible = false };
            filteredScanWrite = new FluentCheckBox { Name = "filteredScanWrite", Visible = false };
            filteredScanRead.CheckedChanged += FilteredScanChoiceChanged;
            filteredScanWrite.CheckedChanged += FilteredScanChoiceChanged;
            replacementLibrarySidebar.Controls.Add(filteredScanRead);
            replacementLibrarySidebar.Controls.Add(filteredScanWrite);
            launch = new FluentButton { Name = "replacementLaunch", Text = "Search Again", Enabled = false, Visible = false };
            launch.Click += LaunchClicked;
            replacementLibrarySidebar.Controls.Add(launch);
            activity = new RichTextBox
            {
                Name = "replacementActivityLog",
                Dock = DockStyle.Fill,
                ReadOnly = true,
                BorderStyle = BorderStyle.None,
                DetectUrls = false,
                Font = new Font("Cascadia Mono", 9f)
            };
            replacementActivityLog = activity;
            candidateFilterPanel = new Panel { Name = "replacementFilterCompatibility", Visible = false };
            mediaSelectorExpandedContent = replacementLibrarySidebar;
            mediaSelectorCollapsedRail = new Panel { Visible = false };
        }

        private void ShowReplacementMode(ArtworkWorkspaceMode mode, bool save)
        {
            if (!replacementShellActive || replacementModeContent == null) return;
            if (musicBrainzMatchesPanel != null) CloseMusicBrainzMatchesWorkspace(false);
            replacementMode = mode;
            if (save)
            {
                uiState.ArtworkWorkspaceMode = mode.ToString();
                SaveUiState();
            }
            foreach (KeyValuePair<ArtworkWorkspaceMode, Button> item in replacementModeButtons)
            {
                item.Value.Tag = item.Key == mode ? "primary" : null;
                ThemeManager.StyleButton(item.Value, uiState.Theme);
            }
            replacementModeContent.SuspendLayout();
            Control previousMode = activityColumn;
            if (replacementActivityLog != null && replacementActivityLog.Parent != null
                && previousMode != null && previousMode.Contains(replacementActivityLog))
                replacementActivityLog.Parent.Controls.Remove(replacementActivityLog);
            if (previousMode != null && replacementModeContent.Controls.Contains(previousMode))
            {
                replacementModeContent.Controls.Remove(previousMode);
                previousMode.Dispose();
            }
            replacementModeContent.Controls.Clear();
            activityColumn = new TableLayoutPanel
            {
                Name = "replacementModeColumn",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            activityColumn.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            activityColumn.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            replacementModeContent.Controls.Add(activityColumn);
            if (mode == ArtworkWorkspaceMode.Edit)
                activityColumn.Controls.Add(BuildReplacementEditMode(), 0, 0);
            else if (mode == ArtworkWorkspaceMode.Upscale)
                activityColumn.Controls.Add(BuildReplacementUpscaleMode(), 0, 0);
            else if (mode == ArtworkWorkspaceMode.Advanced)
                activityColumn.Controls.Add(BuildReplacementAdvancedMode(), 0, 0);
            else
                activityColumn.Controls.Add(BuildReplacementFindMode(), 0, 0);
            replacementModeContent.ResumeLayout(true);
            ApplyReplacementCollapsedState();
            UpdateCandidateActions();
        }

        private Control BuildReplacementFindMode()
        {
            TableLayoutPanel content = new TableLayoutPanel
            {
                Name = "replacementFindMatchMode",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 4,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 56));
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 102));
            content.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 44));
            content.Controls.Add(BuildReplacementCandidateHeading("Candidate Findings",
                "Search multiple sources to find the best artwork for your music."), 0, 0);
            replacementSourceStrip = BuildReplacementSourceStrip();
            content.Controls.Add(replacementSourceStrip, 0, 1);

            TableLayoutPanel cards = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = new Padding(0, 5, 0, 4)
            };
            cards.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 37));
            cards.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 63));
            replacementFiltersCard = BuildReplacementFiltersCard();
            replacementEditingCard = BuildReplacementEditingCard(false);
            cards.Controls.Add(replacementFiltersCard, 0, 0);
            cards.Controls.Add(replacementEditingCard, 1, 0);
            content.Controls.Add(cards, 0, 2);
            candidateActionRow = BuildReplacementActionRow();
            content.Controls.Add(candidateActionRow, 0, 3);
            scanActivityCard = content;
            return content;
        }

        private Control BuildReplacementEditMode()
        {
            TableLayoutPanel content = new TableLayoutPanel
            {
                Name = "replacementEditMode",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 3,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 102));
            content.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 44));
            content.Controls.Add(BuildReplacementSourceArtworkStrip(), 0, 0);
            replacementEditingCard = BuildReplacementEditingCard(true);
            replacementEditingCard.Margin = new Padding(0, 5, 0, 4);
            content.Controls.Add(replacementEditingCard, 0, 1);
            candidateActionRow = BuildReplacementActionRow();
            content.Controls.Add(candidateActionRow, 0, 2);
            scanActivityCard = content;
            return content;
        }

        private Control BuildReplacementUpscaleMode()
        {
            TableLayoutPanel content = new TableLayoutPanel
            {
                Name = "replacementUpscaleMode",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 3,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 56));
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 102));
            content.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            content.Controls.Add(BuildReplacementCandidateHeading("Upscale / Enhance Artwork",
                "Increase resolution and preserve native detail with the configured SPLINED upscale backend."), 0, 0);
            content.Controls.Add(BuildReplacementSourceArtworkStrip(), 0, 1);

            TableLayoutPanel body = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = new Padding(0, 5, 0, 4)
            };
            body.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 46));
            body.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 54));
            body.Controls.Add(BuildReplacementEnhancementPreview(), 0, 0);
            replacementEditingCard = BuildReplacementEditingCard(false);
            replacementEditingCard.Margin = new Padding(3, 0, 0, 0);
            body.Controls.Add(replacementEditingCard, 1, 0);
            content.Controls.Add(body, 0, 2);
            scanActivityCard = content;
            return content;
        }

        private Control BuildReplacementEnhancementPreview()
        {
            NeonCardPanel preview = new NeonCardPanel
            {
                Name = "replacementEnhancementPreview",
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Margin = new Padding(0, 0, 3, 0),
                Padding = new Padding(8)
            };
            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 2,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 34));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            preview.Controls.Add(layout);

            CandidateView candidate = null;
            if (selectedCandidateIndex >= 0) candidates.TryGetValue(selectedCandidateIndex, out candidate);
            if (candidate == null) candidate = SortedCandidateViews().FirstOrDefault();
            string currentText = candidate == null ? "BEFORE" : "BEFORE  ·  " + candidate.Resolution;
            string projected = candidate == null ? "AFTER" : "AFTER  ·  "
                + (candidate.ProjectedWidth > 0 && candidate.ProjectedHeight > 0
                    ? candidate.ProjectedWidth + " x " + candidate.ProjectedHeight
                    : state.RangeIdeal + " x " + state.RangeIdeal);
            layout.Controls.Add(new Label
            {
                Text = currentText,
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                ForeColor = ThemeManager.CurrentPalette.TextSecondary,
                TextAlign = ContentAlignment.MiddleCenter
            }, 0, 0);
            layout.Controls.Add(new Label
            {
                Text = projected,
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                ForeColor = ThemeManager.CurrentPalette.Success,
                TextAlign = ContentAlignment.MiddleCenter
            }, 1, 0);

            Image current = candidate == null ? null : LoadImageCopy(candidate.CachePath);
            Image enhanced = candidate == null ? null : RenderUpscaleImage(candidate, state.Clone());
            PictureBox before = new PictureBox
            {
                Dock = DockStyle.Fill,
                SizeMode = PictureBoxSizeMode.Zoom,
                BackColor = ThemeManager.CurrentPalette.TreeSurface,
                Image = current,
                Margin = new Padding(0, 0, 2, 0)
            };
            PictureBox after = new PictureBox
            {
                Dock = DockStyle.Fill,
                SizeMode = PictureBoxSizeMode.Zoom,
                BackColor = ThemeManager.CurrentPalette.TreeSurface,
                Image = enhanced,
                Margin = new Padding(2, 0, 0, 0)
            };
            layout.Controls.Add(before, 0, 1);
            layout.Controls.Add(after, 1, 1);
            preview.Disposed += delegate
            {
                if (before.Image != null) before.Image.Dispose();
                if (after.Image != null) after.Image.Dispose();
                before.Image = null;
                after.Image = null;
            };
            return preview;
        }

        private Control BuildReplacementAdvancedMode()
        {
            TableLayoutPanel content = new TableLayoutPanel
            {
                Name = "replacementAdvancedMode",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 2,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 56));
            content.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            content.Controls.Add(BuildReplacementCandidateHeading("Scan Activity and Decisions",
                "Live processing events beside the complete Config v5 artwork-adjustment set."), 0, 0);
            TableLayoutPanel body = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 2,
                RowCount = 1,
                Margin = Padding.Empty,
                Padding = new Padding(0, 5, 0, 4)
            };
            body.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 54));
            body.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 46));
            NeonCardPanel log = new NeonCardPanel
            {
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Margin = new Padding(0, 0, 3, 0),
                Padding = new Padding(2)
            };
            if (replacementActivityLog.Parent != null)
                replacementActivityLog.Parent.Controls.Remove(replacementActivityLog);
            log.Controls.Add(replacementActivityLog);
            body.Controls.Add(log, 0, 0);
            body.Controls.Add(BuildReplacementAllAdjustmentsCard(), 1, 0);
            content.Controls.Add(body, 0, 1);
            scanActivityCard = content;
            return content;
        }

        private Control BuildReplacementAllAdjustmentsCard()
        {
            NeonCardPanel card = new NeonCardPanel
            {
                Name = "replacementAdvancedAdjustments",
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Margin = new Padding(3, 0, 0, 0),
                Padding = new Padding(10)
            };
            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 3,
                RowCount = AdjustmentCatalog.All.Length + 1,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 92));
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 66));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            Label heading = new Label
            {
                Text = "☷  Complete Advanced Adjustments",
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                TextAlign = ContentAlignment.MiddleLeft
            };
            layout.Controls.Add(heading, 0, 0);
            layout.SetColumnSpan(heading, 3);
            for (int index = 0; index < AdjustmentCatalog.All.Length; index++)
            {
                AdjustmentDefinition definition = AdjustmentCatalog.All[index];
                int row = index + 1;
                layout.RowStyles.Add(new RowStyle(SizeType.Percent,
                    100f / AdjustmentCatalog.All.Length));
                layout.Controls.Add(new Label
                {
                    Text = definition.Glyph + "  " + definition.Label,
                    Dock = DockStyle.Fill,
                    TextAlign = ContentAlignment.MiddleLeft,
                    AutoEllipsis = true
                }, 0, row);
                ModernSpectrumSlider slider = new ModernSpectrumSlider
                {
                    Minimum = definition.Minimum,
                    Maximum = definition.Maximum,
                    Value = AdjustmentCatalog.GetValue(state, definition.Key),
                    Dock = DockStyle.Fill,
                    Margin = new Padding(4, 4, 8, 4),
                    Tag = definition.Key
                };
                FluentNumericUpDown value = new FluentNumericUpDown
                {
                    Minimum = definition.Minimum,
                    Maximum = definition.Maximum,
                    Value = slider.Value,
                    Dock = DockStyle.Fill,
                    Margin = new Padding(0, 4, 0, 4)
                };
                slider.ValueChanged += delegate
                {
                    value.Value = slider.Value;
                    AdjustmentCatalog.SetValue(state, definition.Key, slider.Value);
                    PersistUpscaleProfile();
                };
                value.ValueChanged += delegate { slider.Value = Decimal.ToInt32(value.Value); };
                layout.Controls.Add(slider, 1, row);
                layout.Controls.Add(value, 2, row);
            }
            card.Controls.Add(layout);
            return card;
        }

        private Control BuildReplacementMusicBrainzWorkspace(Control results)
        {
            TableLayoutPanel content = new TableLayoutPanel
            {
                Name = "replacementMusicBrainzWorkspace",
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 3,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 50));
            content.RowStyles.Add(new RowStyle(SizeType.Absolute, 102));
            content.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            content.Controls.Add(BuildReplacementCandidateHeading(
                "Search Multiple Sources for the Best Match",
                "MusicBrainz release authority is embedded below; CURRENT ALBUM remains fixed above ordinary results."), 0, 0);
            replacementSourceStrip = BuildReplacementSourceStrip();
            content.Controls.Add(replacementSourceStrip, 0, 1);
            results.Dock = DockStyle.Fill;
            results.Margin = new Padding(0, 4, 0, 0);
            content.Controls.Add(results, 0, 2);
            return content;
        }

        private Control BuildReplacementCandidateHeading(string title, string subtitle)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            TableLayoutPanel heading = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 3,
                RowCount = 2,
                Margin = Padding.Empty,
                Padding = new Padding(10, 2, 4, 2)
            };
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            heading.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
            heading.RowStyles.Add(new RowStyle(SizeType.Percent, 58));
            heading.RowStyles.Add(new RowStyle(SizeType.Percent, 42));
            scanActivityTitle = new Label
            {
                Text = "⌕  " + title,
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                TextAlign = ContentAlignment.BottomLeft
            };
            Label description = new Label
            {
                Text = subtitle,
                Dock = DockStyle.Fill,
                ForeColor = palette.TextSecondary,
                TextAlign = ContentAlignment.TopLeft,
                AutoEllipsis = true
            };
            selectedAlbumInfo = new Label
            {
                Text = "Select an album",
                AutoSize = true,
                ForeColor = palette.TextSecondary,
                TextAlign = ContentAlignment.MiddleRight,
                Margin = new Padding(8, 10, 8, 0)
            };
            Button searchAgain = new FluentButton
            {
                Text = "Search Again",
                Dock = DockStyle.Fill,
                Margin = new Padding(3, 6, 0, 3),
                Tag = "primary"
            };
            searchAgain.Click += delegate
            {
                if (!running)
                {
                    if (!filteredScanRead.Checked && !filteredScanWrite.Checked)
                        filteredScanRead.Checked = true;
                    LaunchClicked(searchAgain, EventArgs.Empty);
                }
            };
            heading.Controls.Add(scanActivityTitle, 0, 0);
            heading.Controls.Add(description, 0, 1);
            heading.Controls.Add(selectedAlbumInfo, 1, 0);
            heading.SetRowSpan(selectedAlbumInfo, 2);
            heading.Controls.Add(searchAgain, 2, 0);
            heading.SetRowSpan(searchAgain, 2);
            return heading;
        }

        private HorizontalWheelFlowPanel BuildReplacementSourceStrip()
        {
            HorizontalWheelFlowPanel strip = new HorizontalWheelFlowPanel
            {
                Name = "replacementSourceStrip",
                Dock = DockStyle.Fill,
                AutoScroll = true,
                AllowDrop = true,
                Padding = new Padding(3, 4, 3, 5),
                Margin = Padding.Empty
            };
            strip.DragEnter += ReplacementSourceDragEnter;
            strip.DragOver += ReplacementSourceDragOver;
            strip.DragDrop += ReplacementSourceDragDrop;
            RefreshReplacementSourceStrip(strip);
            return strip;
        }

        private Control BuildReplacementSourceArtworkStrip()
        {
            HorizontalWheelFlowPanel strip = new HorizontalWheelFlowPanel
            {
                Name = "replacementSourceArtworkStrip",
                Dock = DockStyle.Fill,
                AutoScroll = true,
                Padding = new Padding(3, 4, 3, 5),
                Margin = Padding.Empty
            };
            foreach (CandidateView candidate in SortedCandidateViews())
                strip.Controls.Add(BuildReplacementCandidateCard(candidate, true));
            if (strip.Controls.Count == 0)
                strip.Controls.Add(new Label
                {
                    Text = "Source Artwork · run a scan to populate candidates",
                    AutoSize = true,
                    Margin = new Padding(10, 38, 10, 0),
                    ForeColor = ThemeManager.CurrentPalette.TextSecondary
                });
            return strip;
        }

        private NeonCardPanel BuildReplacementFiltersCard()
        {
            NeonCardPanel card = new NeonCardPanel
            {
                Name = "replacementFiltersCard",
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Margin = new Padding(0, 0, 3, 0),
                Padding = new Padding(10)
            };
            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 2,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            replacementFiltersToggle = ReplacementCardToggle("⌕  Filters",
                uiState.ArtworkFiltersExpanded, delegate
                {
                    uiState.ArtworkFiltersExpanded = !uiState.ArtworkFiltersExpanded;
                    SaveUiState();
                    ApplyReplacementCollapsedState();
                });
            replacementFiltersBody = new HiddenScrollPanel { Dock = DockStyle.Fill };
            replacementFiltersBody.SizeChanged += delegate { ResizeReplacementFilterControls(); };
            layout.Controls.Add(replacementFiltersToggle, 0, 0);
            layout.Controls.Add(replacementFiltersBody, 0, 1);
            card.Controls.Add(layout);
            RebuildReplacementFilters();
            return card;
        }

        private NeonCardPanel BuildReplacementEditingCard(bool expandedEditMode)
        {
            NeonCardPanel card = new NeonCardPanel
            {
                Name = "replacementEditingCard",
                Dock = DockStyle.Fill,
                SpectrumBorder = true,
                Margin = new Padding(3, 0, 0, 0),
                Padding = new Padding(10)
            };
            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 2,
                Margin = Padding.Empty,
                Padding = Padding.Empty
            };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            replacementEditingToggle = ReplacementCardToggle(
                expandedEditMode ? "✎  Edit Artwork" : "✦  Artwork Preview & Editing",
                uiState.ArtworkEditingExpanded, delegate
                {
                    uiState.ArtworkEditingExpanded = !uiState.ArtworkEditingExpanded;
                    SaveUiState();
                    ApplyReplacementCollapsedState();
                });
            replacementEditingBody = new HiddenScrollPanel { Dock = DockStyle.Fill };
            layout.Controls.Add(replacementEditingToggle, 0, 0);
            layout.Controls.Add(replacementEditingBody, 0, 1);
            card.Controls.Add(layout);
            RebuildReplacementEditing(expandedEditMode);
            return card;
        }

        private Button ReplacementCardToggle(string text, bool expanded, EventHandler click)
        {
            Button button = new FluentButton
            {
                Text = text + (expanded ? "  ⌃" : "  ⌄"),
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.MiddleLeft,
                Margin = Padding.Empty,
                FlatStyle = FlatStyle.Flat
            };
            button.Click += click;
            return button;
        }

        private FlowLayoutPanel BuildReplacementActionRow()
        {
            FlowLayoutPanel actions = BuildCandidateActionRow();
            actions.Name = "replacementCandidateActions";
            actions.AutoScroll = false;
            actions.Padding = new Padding(0, 6, 0, 4);
            launch.Visible = false;
            useSelected.Text = "Apply to Selected";
            useSelected.Width = 140;
            useSelected.Tag = "primary";
            keepLocal.Width = 100;
            compare.Width = 90;
            skip.Width = 110;
            enableHover.Visible = false;
            if (actions.Controls.Count > 0)
                actions.Controls[actions.Controls.Count - 1].Visible = false;
            return actions;
        }

        private void ApplyReplacementCollapsedState()
        {
            if (replacementFiltersBody != null)
                replacementFiltersBody.Visible = uiState.ArtworkFiltersExpanded;
            if (replacementEditingBody != null)
                replacementEditingBody.Visible = uiState.ArtworkEditingExpanded;
            if (replacementFiltersToggle != null)
                replacementFiltersToggle.Text = "⌕  Filters  "
                    + (uiState.ArtworkFiltersExpanded ? "⌃" : "⌄");
            if (replacementEditingToggle != null)
                replacementEditingToggle.Text = (replacementMode == ArtworkWorkspaceMode.Edit
                    ? "✎  Edit Artwork  " : "✦  Artwork Preview & Editing  ")
                    + (uiState.ArtworkEditingExpanded ? "⌃" : "⌄");
        }

        private void RefreshReplacementLibraryTabs()
        {
            foreach (KeyValuePair<LibraryNavigationMode, Button> item in replacementLibraryTabs)
            {
                item.Value.Tag = item.Key == replacementLibraryMode ? "primary" : null;
                ThemeManager.StyleButton(item.Value, uiState.Theme);
            }
            if (replacementLibrarySearch != null)
            {
                string noun = replacementLibraryMode == LibraryNavigationMode.Artists ? "artists"
                    : replacementLibraryMode == LibraryNavigationMode.Albums ? "albums" : "folders";
                replacementLibrarySearch.AccessibleDescription = "Search " + noun;
            }
        }

        private void RefreshReplacementLibraryEntities()
        {
            if (!replacementShellActive || replacementLibraryEntities == null) return;
            string query = replacementLibrarySearch == null ? "" : replacementLibrarySearch.Text.Trim();
            replacementLibraryEntities.SuspendLayout();
            foreach (Control child in replacementLibraryEntities.Controls.Cast<Control>().ToArray())
                child.Dispose();
            replacementLibraryEntities.Controls.Clear();

            if (replacementLibraryMode == LibraryNavigationMode.Artists)
            {
                IEnumerable<IGrouping<string, AlbumInfo>> artists = albums
                    .GroupBy(album => album.Artist ?? "Unknown Artist", StringComparer.OrdinalIgnoreCase)
                    .Where(group => query.Length == 0 || group.Key.IndexOf(query,
                        StringComparison.OrdinalIgnoreCase) >= 0)
                    .OrderBy(group => group.Key, StringComparer.OrdinalIgnoreCase);
                foreach (IGrouping<string, AlbumInfo> artist in artists)
                {
                    AlbumInfo representative = artist.OrderBy(item => item.Title,
                        StringComparer.OrdinalIgnoreCase).FirstOrDefault();
                    Control row = BuildReplacementLibraryRow(artist.Key,
                        artist.Count() + (artist.Count() == 1 ? " album" : " albums"),
                        representative == null ? "" : representative.CoverPath,
                        Object.ReferenceEquals(displayedAlbum, representative), representative);
                    row.Tag = artist.Key;
                    row.Click += delegate
                    {
                        replacementArtistScope = artist.Key;
                        if (representative != null) ShowSelectedAlbum(representative);
                        HighlightReplacementLibraryRow(row);
                    };
                    WireReplacementRowChildren(row);
                    replacementLibraryEntities.Controls.Add(row);
                }
            }
            else
            {
                IEnumerable<AlbumInfo> visible = albums.Where(album =>
                    String.IsNullOrWhiteSpace(replacementArtistScope)
                    || album.Artist.Equals(replacementArtistScope, StringComparison.OrdinalIgnoreCase));
                if (query.Length > 0)
                    visible = visible.Where(album => album.Title.IndexOf(query,
                        StringComparison.OrdinalIgnoreCase) >= 0
                        || album.Artist.IndexOf(query, StringComparison.OrdinalIgnoreCase) >= 0
                        || album.Path.IndexOf(query, StringComparison.OrdinalIgnoreCase) >= 0);
                visible = replacementLibraryMode == LibraryNavigationMode.Folders
                    ? visible.OrderBy(album => album.Path, StringComparer.OrdinalIgnoreCase)
                    : visible.OrderBy(album => album.Artist, StringComparer.OrdinalIgnoreCase)
                        .ThenBy(album => album.Title, StringComparer.OrdinalIgnoreCase);
                foreach (AlbumInfo album in visible)
                {
                    string subtitle = replacementLibraryMode == LibraryNavigationMode.Folders
                        ? album.Path : album.Artist;
                    Control row = BuildReplacementLibraryRow(album.Title, subtitle,
                        album.CoverPath, ReferenceEquals(displayedAlbum, album), album);
                    row.Click += delegate
                    {
                        ShowSelectedAlbum(album);
                        HighlightReplacementLibraryRow(row);
                    };
                    row.DoubleClick += delegate
                    {
                        if (!running && album.State != AlbumState.TimeoutActive)
                        {
                            album.Selected = !album.Selected;
                            UpdateSelectionControls();
                            SaveUiState();
                            RefreshReplacementLibraryEntities();
                        }
                    };
                    WireReplacementRowChildren(row);
                    replacementLibraryEntities.Controls.Add(row);
                }
            }
            replacementLibraryEntities.ResumeLayout(true);
            SizeReplacementLibraryRows();
            UpdateReplacementLibraryStatus();
            EnrichReplacementNavidromeArtworkAsync();
        }

        private Control BuildReplacementLibraryRow(string title, string subtitle,
            string imagePath, bool selected, AlbumInfo album)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            NeonCardPanel row = new NeonCardPanel
            {
                Name = "replacementLibraryRow",
                Width = Math.Max(190, replacementLibraryEntities.ClientSize.Width - 22),
                Height = 58,
                Margin = new Padding(0, 2, 0, 2),
                Padding = new Padding(5),
                Selected = selected,
                Cursor = Cursors.Hand,
                Tag = album
            };
            PictureBox image = new PictureBox
            {
                Name = "libraryArtwork",
                Left = 6,
                Top = 6,
                Width = 44,
                Height = 44,
                SizeMode = PictureBoxSizeMode.Zoom,
                BackColor = palette.TreeSurface,
                Image = LoadImageCopy(imagePath),
                Cursor = Cursors.Hand
            };
            row.Disposed += delegate
            {
                if (image.Image != null) image.Image.Dispose();
                image.Image = null;
            };
            Label name = new Label
            {
                Name = "libraryEntityTitle",
                Left = 60,
                Top = 7,
                Width = Math.Max(90, row.Width - 70),
                Height = 22,
                Text = title,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, selected ? FontStyle.Bold : FontStyle.Regular),
                ForeColor = palette.TextPrimary,
                AutoEllipsis = true,
                Cursor = Cursors.Hand
            };
            Label detail = new Label
            {
                Name = "libraryEntityDetail",
                Left = 60,
                Top = 29,
                Width = Math.Max(90, row.Width - 70),
                Height = 20,
                Text = subtitle,
                ForeColor = palette.TextSecondary,
                AutoEllipsis = true,
                Cursor = Cursors.Hand
            };
            row.Controls.Add(image);
            row.Controls.Add(name);
            row.Controls.Add(detail);
            row.Resize += delegate
            {
                name.Width = Math.Max(90, row.ClientSize.Width - 70);
                detail.Width = name.Width;
            };
            return row;
        }

        private static void WireReplacementRowChildren(Control row)
        {
            foreach (Control child in row.Controls)
            {
                child.Click += delegate
                {
                    NeonCardPanel card = row as NeonCardPanel;
                    if (card != null) card.ActivateCard();
                };
            }
        }

        private void HighlightReplacementLibraryRow(Control selected)
        {
            foreach (NeonCardPanel row in replacementLibraryEntities.Controls.OfType<NeonCardPanel>())
            {
                row.Selected = Object.ReferenceEquals(row, selected);
                row.Invalidate();
            }
        }

        private void SizeReplacementLibraryRows()
        {
            if (replacementLibraryEntities == null) return;
            int width = Math.Max(190, replacementLibraryEntities.ClientSize.Width - 22);
            foreach (Control row in replacementLibraryEntities.Controls) row.Width = width;
        }

        private void UpdateReplacementLibraryStatus()
        {
            if (replacementLibraryStatus == null) return;
            int artists = albums.Select(album => album.Artist).Distinct(StringComparer.OrdinalIgnoreCase).Count();
            int withArtwork = albums.Count(album => album.HasLocalArtwork || !String.IsNullOrWhiteSpace(album.CoverPath));
            int missing = Math.Max(0, albums.Count - withArtwork);
            int selected = albums.Count(album => album.Selected);
            replacementLibraryStatus.Text = "Library Status\r\n"
                + "●  Artists                 " + artists + "\r\n"
                + "●  Albums                  " + albums.Count + "\r\n"
                + "●  With artwork            " + withArtwork + "\r\n"
                + "●  Missing artwork         " + missing + "\r\n"
                + "●  Selected                " + selected;
        }

        private async void EnrichReplacementNavidromeArtworkAsync()
        {
            if (!replacementShellActive || replacementLibraryEntities == null
                || !NavidromeClient.IsConfigured(state)) return;
            try
            {
                NavidromeClient client = NavidromeClient.FromState(state);
                List<NavidromeArtist> remoteArtists = await client.GetArtistsAsync();
                if (IsDisposed || replacementLibraryEntities == null) return;
                string cache = Path.Combine(state.CacheDir, "navidrome-artwork");
                foreach (Control row in replacementLibraryEntities.Controls.Cast<Control>().ToArray())
                {
                    if (row.IsDisposed) continue;
                    PictureBox picture = row.Controls["libraryArtwork"] as PictureBox;
                    if (picture == null || picture.Image != null) continue;
                    AlbumInfo album = row.Tag as AlbumInfo;
                    string artistName = album == null ? row.Tag as string : album.Artist;
                    NavidromeArtist remoteArtist = remoteArtists.FirstOrDefault(item =>
                        NavidromeClient.NormalizeIdentity(item.Name)
                        == NavidromeClient.NormalizeIdentity(artistName));
                    if (remoteArtist == null) continue;
                    string coverArtId = remoteArtist.CoverArtId;
                    if (album != null)
                    {
                        List<NavidromeAlbum> remoteAlbums = await client.GetArtistAlbumsAsync(remoteArtist.Id);
                        NavidromeAlbum remoteAlbum = NavidromeClient.MatchAlbum(album, remoteAlbums);
                        if (remoteAlbum != null) coverArtId = remoteAlbum.CoverArtId;
                    }
                    string cached = await client.CacheCoverArtAsync(coverArtId, cache, 128);
                    if (row.IsDisposed || picture.IsDisposed || String.IsNullOrWhiteSpace(cached)) continue;
                    picture.Image = LoadImageCopy(cached);
                }
            }
            catch (Exception error)
            {
                RuntimeLog.Write("debug", "windows.navidrome.library_enrichment_unavailable error="
                    + error.Message);
            }
        }

        private void RebuildReplacementFilters()
        {
            if (replacementFiltersBody == null) return;
            replacementFiltersBody.SuspendLayout();
            foreach (Control child in replacementFiltersBody.Controls.Cast<Control>().ToArray())
                child.Dispose();
            replacementFiltersBody.Controls.Clear();
            replacementTriStateFilters.Clear();
            FlowLayoutPanel list = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                AutoScroll = false,
                FlowDirection = FlowDirection.TopDown,
                WrapContents = false,
                Padding = new Padding(2, 3, 2, 3)
            };
            list.Controls.Add(ReplacementSectionLabel("Image Type / Category"));
            AddReplacementTriState(list, "FrontCover", "Front Cover",
                candidates.Values.Count(candidate => candidate.ImageCategory == "FrontCover"));
            AddReplacementTriState(list, "BackCover", "Back Cover",
                candidates.Values.Count(candidate => candidate.ImageCategory == "BackCover"));
            AddReplacementTriState(list, "ArtistImage", "Artist Image",
                candidates.Values.Count(candidate => candidate.ImageCategory == "ArtistImage"));
            AddReplacementTriState(list, "MediaBooklet", "Media / Booklet",
                candidates.Values.Count(candidate => candidate.ImageCategory == "MediaBooklet"));
            AddReplacementTriState(list, "Other", "Other",
                candidates.Values.Count(candidate => candidate.ImageCategory == "Other"));
            list.Controls.Add(ReplacementSectionLabel("Candidate Policy"));
            AddReplacementTriState(list, "Recommended", "Recommended",
                candidates.Values.Count(candidate => candidate.Recommended));
            AddReplacementTriState(list, "Upscalable", "Upscalable",
                candidates.Values.Count(candidate => candidate.UpscaleEligible));
            AddReplacementTriState(list, "Rejected", "Rejected",
                candidates.Values.Count(candidate => candidate.IsNegative));
            AddReplacementTriState(list, "Local", "Local",
                candidates.Values.Count(candidate => candidate.IsLocal));
            Button reset = new FluentButton
            {
                Text = "↻  Reset filters",
                Width = 140,
                Height = 28,
                Margin = new Padding(2, 4, 2, 2)
            };
            reset.Click += delegate
            {
                excludedCandidateTypes.Clear();
                uiState.CandidateIncludedTypes.Clear();
                SaveCandidateFilterState();
                SaveUiState();
                RebuildReplacementFilters();
                ApplyCandidateFilters();
            };
            list.Controls.Add(reset);
            replacementFiltersBody.Controls.Add(list);
            replacementFiltersBody.ResumeLayout(true);
            ResizeReplacementFilterControls();
            ApplyReplacementCollapsedState();
        }

        private void ResizeReplacementFilterControls()
        {
            if (replacementFiltersBody == null) return;
            int width = Math.Max(150, replacementFiltersBody.ClientSize.Width - 10);
            foreach (Control control in replacementFiltersBody.Controls
                .Cast<Control>().SelectMany(item => item.Controls.Cast<Control>()))
            {
                if (control is TriStateFilterControl || control is Label)
                    control.Width = width;
            }
        }

        private Label ReplacementSectionLabel(string text)
        {
            return new Label
            {
                Text = text,
                AutoSize = false,
                Width = 280,
                Height = 22,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                ForeColor = ThemeManager.CurrentPalette.TextPrimary,
                TextAlign = ContentAlignment.BottomLeft,
                Margin = new Padding(2, 5, 2, 1)
            };
        }

        private void AddReplacementTriState(FlowLayoutPanel owner, string key,
            string label, int count)
        {
            TriStateFilterValue value = excludedCandidateTypes.Contains(key)
                ? TriStateFilterValue.Disregarded
                : uiState.CandidateIncludedTypes.Contains(key, StringComparer.OrdinalIgnoreCase)
                    ? TriStateFilterValue.Included : TriStateFilterValue.Neutral;
            TriStateFilterControl control = new TriStateFilterControl
            {
                Name = "triState" + key,
                Caption = label + "  (" + count + ")",
                State = value,
                Width = 300,
                Height = 24,
                Margin = new Padding(2, 0, 2, 0),
                Font = ThemeManager.UiFont(ThemeFontRole.Body)
            };
            control.StateChanged += delegate
            {
                excludedCandidateTypes.Remove(key);
                uiState.CandidateIncludedTypes.RemoveAll(item =>
                    item.Equals(key, StringComparison.OrdinalIgnoreCase));
                if (control.State == TriStateFilterValue.Disregarded)
                    excludedCandidateTypes.Add(key);
                else if (control.State == TriStateFilterValue.Included)
                    uiState.CandidateIncludedTypes.Add(key);
                SaveCandidateFilterState();
                SaveUiState();
                ApplyCandidateFilters();
            };
            replacementTriStateFilters[key] = control;
            owner.Controls.Add(control);
        }

        private void RebuildReplacementEditing(bool fullEditMode)
        {
            if (replacementEditingBody == null) return;
            replacementEditingBody.SuspendLayout();
            foreach (Control child in replacementEditingBody.Controls.Cast<Control>().ToArray())
                child.Dispose();
            replacementEditingBody.Controls.Clear();
            replacementAdjustmentSliders.Clear();

            TableLayoutPanel layout = new TableLayoutPanel
            {
                Dock = DockStyle.Top,
                AutoSize = true,
                AutoSizeMode = AutoSizeMode.GrowAndShrink,
                ColumnCount = 1,
                RowCount = 3,
                Padding = new Padding(2),
                Margin = Padding.Empty
            };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 34));

            FlowLayoutPanel primary = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                AutoScroll = false,
                AllowDrop = true,
                Margin = Padding.Empty,
                Padding = new Padding(0, 2, 0, 2)
            };
            primary.DragEnter += ReplacementAdjustmentDragEnter;
            primary.DragDrop += ReplacementAdjustmentDragDrop;
            List<string> selected = AdjustmentCatalog.NormalizePrimary(uiState.PrimaryAdjustments);
            uiState.PrimaryAdjustments = selected;
            foreach (string key in selected)
            {
                AdjustmentDefinition definition = AdjustmentCatalog.Find(key);
                Button button = new FluentButton
                {
                    Text = definition.Glyph + "  " + definition.Label,
                    Width = 84,
                    Height = 34,
                    Tag = definition.Key,
                    Margin = new Padding(0, 0, 4, 0)
                };
                button.MouseDown += delegate(object sender, MouseEventArgs args)
                {
                    if (args.Button == MouseButtons.Left)
                        button.DoDragDrop(button.Tag.ToString(), DragDropEffects.Move);
                };
                button.Click += delegate { ToggleReplacementPrimaryAdjustment(definition.Key); };
                primary.Controls.Add(button);
            }
            Button arrange = new FluentButton
            {
                Text = "☷  Arrange / Select",
                Width = 126,
                Height = 34,
                Tag = "primary"
            };
            arrange.Click += delegate { ShowReplacementAdjustmentMenu(arrange); };
            primary.Controls.Add(arrange);
            layout.Controls.Add(primary, 0, 0);

            TableLayoutPanel rows = new TableLayoutPanel
            {
                Dock = DockStyle.Top,
                AutoSize = true,
                AutoSizeMode = AutoSizeMode.GrowAndShrink,
                ColumnCount = 3,
                RowCount = selected.Count,
                Margin = Padding.Empty,
                Padding = new Padding(0, 2, 0, 2)
            };
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 112));
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 76));
            for (int index = 0; index < selected.Count; index++)
            {
                string key = selected[index];
                AdjustmentDefinition definition = AdjustmentCatalog.Find(key);
                rows.RowStyles.Add(new RowStyle(SizeType.Absolute, fullEditMode ? 42 : 30));
                rows.Controls.Add(new Label
                {
                    Text = definition.Label,
                    Dock = DockStyle.Fill,
                    TextAlign = ContentAlignment.MiddleLeft
                }, 0, index);
                ModernSpectrumSlider slider = new ModernSpectrumSlider
                {
                    Name = "replacementSlider" + definition.Key,
                    Dock = DockStyle.Fill,
                    Minimum = definition.Minimum,
                    Maximum = definition.Maximum,
                    Value = AdjustmentCatalog.GetValue(state, definition.Key),
                    Margin = new Padding(4, fullEditMode ? 5 : 2, 10, fullEditMode ? 4 : 2),
                    Tag = definition.Key
                };
                FluentNumericUpDown value = new FluentNumericUpDown
                {
                    Dock = DockStyle.Fill,
                    Minimum = definition.Minimum,
                    Maximum = definition.Maximum,
                    Value = slider.Value,
                    Margin = new Padding(0, fullEditMode ? 5 : 2, 0, fullEditMode ? 4 : 2)
                };
                slider.ValueChanged += delegate
                {
                    value.Value = slider.Value;
                    AdjustmentCatalog.SetValue(state, definition.Key, slider.Value);
                    PersistUpscaleProfile();
                    upscalePreviewTimer.Stop();
                    upscalePreviewTimer.Start();
                };
                value.ValueChanged += delegate
                {
                    slider.Value = Decimal.ToInt32(value.Value);
                };
                replacementAdjustmentSliders[definition.Key] = slider;
                rows.Controls.Add(slider, 1, index);
                rows.Controls.Add(value, 2, index);
            }
            layout.Controls.Add(rows, 0, 1);

            FlowLayoutPanel additional = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                AutoScroll = true,
                Margin = Padding.Empty,
                Padding = new Padding(0, 3, 0, 1)
            };
            foreach (AdjustmentDefinition definition in AdjustmentCatalog.All
                .Where(item => !selected.Contains(item.Key, StringComparer.OrdinalIgnoreCase)))
            {
                Button toggle = new FluentButton
                {
                    Text = "+  " + definition.Label,
                    Width = 108,
                    Height = 32,
                    Margin = new Padding(0, 0, 4, 0),
                    Enabled = selected.Count < 5
                };
                toggle.Click += delegate { ToggleReplacementPrimaryAdjustment(definition.Key); };
                additional.Controls.Add(toggle);
            }
            layout.Controls.Add(additional, 0, 2);
            replacementEditingBody.Controls.Add(layout);
            replacementEditingBody.ResumeLayout(true);
            ApplyReplacementCollapsedState();
        }

        private void ToggleReplacementPrimaryAdjustment(string key)
        {
            List<string> selected = AdjustmentCatalog.NormalizePrimary(uiState.PrimaryAdjustments);
            int index = selected.FindIndex(item => item.Equals(key, StringComparison.OrdinalIgnoreCase));
            if (index >= 0)
            {
                if (selected.Count <= 1) return;
                selected.RemoveAt(index);
            }
            else
            {
                if (selected.Count >= 5)
                {
                    SetStatus("Five primary adjustments are already selected. Remove one first.");
                    return;
                }
                selected.Add(key);
            }
            uiState.PrimaryAdjustments = selected;
            SaveUiState();
            RebuildReplacementEditing(replacementMode == ArtworkWorkspaceMode.Edit);
        }

        private void ShowReplacementAdjustmentMenu(Control owner)
        {
            ContextMenuStrip menu = new ContextMenuStrip();
            List<string> selected = AdjustmentCatalog.NormalizePrimary(uiState.PrimaryAdjustments);
            foreach (AdjustmentDefinition definition in AdjustmentCatalog.All)
            {
                ToolStripMenuItem item = new ToolStripMenuItem(definition.Label)
                {
                    Checked = selected.Contains(definition.Key, StringComparer.OrdinalIgnoreCase),
                    Tag = definition.Key
                };
                item.Click += delegate { ToggleReplacementPrimaryAdjustment(definition.Key); };
                menu.Items.Add(item);
            }
            menu.Show(owner, new Point(0, owner.Height));
        }

        private void ReplacementAdjustmentDragEnter(object sender, DragEventArgs e)
        {
            e.Effect = e.Data.GetDataPresent(DataFormats.Text)
                ? DragDropEffects.Move : DragDropEffects.None;
        }

        private void ReplacementAdjustmentDragDrop(object sender, DragEventArgs e)
        {
            string dragged = e.Data.GetData(DataFormats.Text) as string;
            FlowLayoutPanel owner = sender as FlowLayoutPanel;
            if (String.IsNullOrWhiteSpace(dragged) || owner == null) return;
            Point location = owner.PointToClient(new Point(e.X, e.Y));
            Button target = owner.Controls.OfType<Button>()
                .Where(button => button.Tag is string
                    && AdjustmentCatalog.Find(button.Tag.ToString()) != null)
                .OrderBy(button => Math.Abs(button.Left + button.Width / 2 - location.X))
                .FirstOrDefault();
            if (target == null) return;
            List<string> selected = AdjustmentCatalog.NormalizePrimary(uiState.PrimaryAdjustments);
            selected.RemoveAll(item => item.Equals(dragged, StringComparison.OrdinalIgnoreCase));
            int targetIndex = selected.FindIndex(item => item.Equals(target.Tag.ToString(),
                StringComparison.OrdinalIgnoreCase));
            selected.Insert(Math.Max(0, targetIndex), dragged);
            uiState.PrimaryAdjustments = selected;
            SaveUiState();
            RebuildReplacementEditing(replacementMode == ArtworkWorkspaceMode.Edit);
        }

        private void RefreshReplacementSourceStrip(HorizontalWheelFlowPanel strip)
        {
            if (strip == null) return;
            strip.SuspendLayout();
            foreach (Control child in strip.Controls.Cast<Control>().ToArray()) child.Dispose();
            strip.Controls.Clear();
            Dictionary<string, int> counts = candidates.Values
                .GroupBy(candidate => candidate.SourceKey, StringComparer.OrdinalIgnoreCase)
                .ToDictionary(group => group.Key, group => group.Count(), StringComparer.OrdinalIgnoreCase);
            foreach (ArtworkSourceDescriptor source in ArtworkSourceCatalog.Project(state, counts))
            {
                NeonCardPanel card = new NeonCardPanel
                {
                    Name = "sourceCard_" + source.Key,
                    Width = 112,
                    Height = 94,
                    Margin = new Padding(3),
                    Padding = new Padding(7),
                    SpectrumBorder = source.Key.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase),
                    Selected = !excludedCandidateSources.Contains(source.Key),
                    Cursor = Cursors.Hand,
                    Tag = source
                };
                Label glyph = new Label
                {
                    Dock = DockStyle.Top,
                    Height = 29,
                    Text = source.Glyph,
                    Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                    ForeColor = CandidateSourceColor(source.Key, ThemeManager.CurrentPalette),
                    TextAlign = ContentAlignment.MiddleCenter,
                    Cursor = Cursors.Hand
                };
                Label name = new Label
                {
                    Dock = DockStyle.Top,
                    Height = 25,
                    Text = source.Label,
                    Font = ThemeManager.UiFont(ThemeFontRole.Minor, FontStyle.Bold),
                    TextAlign = ContentAlignment.MiddleCenter,
                    AutoEllipsis = true,
                    Cursor = Cursors.Hand
                };
                string stateText = source.Key.Equals("navidrome", StringComparison.OrdinalIgnoreCase)
                    ? (source.CredentialConfigured ? "● Connected" : "Credentials needed")
                    : source.Enabled ? source.CandidateCount + " found" : "Disabled";
                Label status = new Label
                {
                    Dock = DockStyle.Fill,
                    Text = stateText,
                    Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                    ForeColor = source.Enabled
                        ? source.Key.Equals("navidrome", StringComparison.OrdinalIgnoreCase)
                            && source.CredentialConfigured ? ThemeManager.CurrentPalette.Success
                            : ThemeManager.CurrentPalette.TextSecondary
                        : ThemeManager.CurrentPalette.TextDisabled,
                    TextAlign = ContentAlignment.MiddleCenter,
                    Cursor = Cursors.Hand
                };
                card.Controls.Add(status);
                card.Controls.Add(name);
                card.Controls.Add(glyph);
                EventHandler activate = delegate
                {
                    if (source.Key.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase)
                        && retryMusicBrainz != null && retryMusicBrainz.Enabled)
                    {
                        RetryMusicBrainzClicked(card, EventArgs.Empty);
                        return;
                    }
                    if (excludedCandidateSources.Contains(source.Key))
                        excludedCandidateSources.Remove(source.Key);
                    else
                    {
                        excludedCandidateSources.Clear();
                        foreach (ArtworkSourceDescriptor other in ArtworkSourceCatalog.Project(state, counts))
                            if (!other.Key.Equals(source.Key, StringComparison.OrdinalIgnoreCase))
                                excludedCandidateSources.Add(other.Key);
                    }
                    SaveCandidateFilterState();
                    ApplyCandidateFilters();
                    RefreshReplacementSourceStrip(strip);
                };
                card.Click += activate;
                glyph.Click += activate;
                name.Click += activate;
                status.Click += activate;
                MouseEventHandler startDrag = delegate(object sender, MouseEventArgs args)
                {
                    if (args.Button != MouseButtons.Left || source.BuiltIn) return;
                    replacementDraggedSource = source.Key;
                    card.DoDragDrop(source.Key, DragDropEffects.Move);
                };
                card.MouseDown += startDrag;
                glyph.MouseDown += startDrag;
                name.MouseDown += startDrag;
                status.MouseDown += startDrag;
                strip.Controls.Add(card);
            }
            strip.ResumeLayout(true);
        }

        private void ReplacementSourceDragEnter(object sender, DragEventArgs e)
        {
            e.Effect = e.Data.GetDataPresent(DataFormats.Text)
                ? DragDropEffects.Move : DragDropEffects.None;
        }

        private void ReplacementSourceDragOver(object sender, DragEventArgs e)
        {
            ReplacementSourceDragEnter(sender, e);
        }

        private void ReplacementSourceDragDrop(object sender, DragEventArgs e)
        {
            HorizontalWheelFlowPanel strip = sender as HorizontalWheelFlowPanel;
            string dragged = e.Data.GetData(DataFormats.Text) as string;
            if (strip == null || String.IsNullOrWhiteSpace(dragged)) dragged = replacementDraggedSource;
            if (strip == null || String.IsNullOrWhiteSpace(dragged)) return;
            Point point = strip.PointToClient(new Point(e.X, e.Y));
            NeonCardPanel target = strip.Controls.OfType<NeonCardPanel>()
                .Where(card => card.Tag is ArtworkSourceDescriptor
                    && !((ArtworkSourceDescriptor)card.Tag).BuiltIn)
                .OrderBy(card => Math.Abs(card.Left + card.Width / 2 - point.X))
                .FirstOrDefault();
            ArtworkSourceDescriptor descriptor = target == null ? null : target.Tag as ArtworkSourceDescriptor;
            if (descriptor != null && ArtworkSourceCatalog.Reorder(state, dragged, descriptor.Key))
            {
                SetStatus("Artwork source priority updated in Config v5.");
                RefreshReplacementSourceStrip(strip);
            }
            replacementDraggedSource = "";
        }

        private Control BuildReplacementCandidateCard(CandidateView view, bool compact)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            int width = compact ? 126 : 174;
            int imageHeight = compact ? 72 : 100;
            NeonCardPanel card = new NeonCardPanel
            {
                Name = "replacementCandidate_" + view.Index,
                Width = width,
                Height = compact ? 100 : 148,
                Margin = new Padding(4, 2, 4, 2),
                Padding = new Padding(5),
                SpectrumBorder = view.Recommended || view.IsLocal,
                Selected = compareCandidateIndexes.Contains(view.Index),
                Cursor = Cursors.Hand,
                Tag = view.Index
            };
            PictureBox image = new PictureBox
            {
                Name = "candidateImage",
                Dock = DockStyle.Top,
                Height = imageHeight,
                SizeMode = PictureBoxSizeMode.Zoom,
                BorderStyle = BorderStyle.None,
                BackColor = palette.TreeSurface,
                Image = LoadImageCopy(view.CachePath),
                Cursor = Cursors.Hand
            };
            card.Disposed += delegate
            {
                if (image.Image != null) image.Image.Dispose();
                image.Image = null;
            };
            Label source = new Label
            {
                Name = "candidateSource",
                Dock = DockStyle.Top,
                Height = 22,
                Text = CandidateSourceName(view.SourceKey),
                ForeColor = CandidateSourceColor(view.SourceKey, palette),
                Font = ThemeManager.UiFont(ThemeFontRole.Minor, FontStyle.Bold),
                TextAlign = ContentAlignment.MiddleLeft,
                AutoEllipsis = true,
                Cursor = Cursors.Hand
            };
            Label dimensions = new Label
            {
                Name = "candidateDimensions",
                Dock = DockStyle.Fill,
                Text = view.Resolution,
                ForeColor = palette.TextSecondary,
                Font = ThemeManager.UiFont(ThemeFontRole.Minor),
                TextAlign = ContentAlignment.MiddleLeft,
                Cursor = Cursors.Hand
            };
            card.Controls.Add(dimensions);
            card.Controls.Add(source);
            card.Controls.Add(image);
            EventHandler choose = delegate
            {
                bool additive = (ModifierKeys & Keys.Control) == Keys.Control;
                if (!additive) compareCandidateIndexes.Clear();
                if (additive && compareCandidateIndexes.Contains(view.Index))
                    compareCandidateIndexes.Remove(view.Index);
                else
                    compareCandidateIndexes.Add(view.Index);
                selectedCandidateIndex = compareCandidateIndexes.Count == 1
                    ? compareCandidateIndexes.First() : -1;
                UpdateCandidateActions();
                RefreshSelectedCandidatePreview();
                ApplyCandidateFilters();
            };
            card.Click += choose;
            image.Click += choose;
            source.Click += choose;
            dimensions.Click += choose;
            image.MouseEnter += delegate { if (uiState.HoverEnabled) ShowHoverPreview(view); };
            image.MouseLeave += delegate { if (uiState.HoverEnabled) ScheduleCloseHoverPreview(); };
            ToolTip details = CreateOwnedToolTip();
            details.SetToolTip(card, CandidateDetails(view));
            details.SetToolTip(image, CandidateDetails(view));
            return card;
        }

        private void RefreshReplacementModeAfterCandidateChange()
        {
            if (!replacementShellActive) return;
            if (replacementSourceStrip != null && !replacementSourceStrip.IsDisposed)
                RefreshReplacementSourceStrip(replacementSourceStrip);
            if (replacementCandidateSummary != null)
                replacementCandidateSummary.Text = candidates.Count + (candidates.Count == 1
                    ? " candidate" : " candidates");
            if (replacementFiltersBody != null && !replacementFiltersBody.IsDisposed)
                RebuildReplacementFilters();
            if (replacementMode == ArtworkWorkspaceMode.Edit
                || replacementMode == ArtworkWorkspaceMode.Upscale)
                ShowReplacementMode(replacementMode, false);
        }

        private static LibraryNavigationMode ParseLibraryMode(string value)
        {
            LibraryNavigationMode mode;
            return Enum.TryParse(value, true, out mode) ? mode : LibraryNavigationMode.Artists;
        }

        private static ArtworkWorkspaceMode ParseWorkspaceMode(string value)
        {
            ArtworkWorkspaceMode mode;
            return Enum.TryParse(value, true, out mode)
                && mode != ArtworkWorkspaceMode.MusicBrainz
                && mode != ArtworkWorkspaceMode.CustomUpscale
                ? mode : ArtworkWorkspaceMode.FindMatch;
        }
    }
}
