using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal sealed class SetupForm : FluentForm
    {
        private ConfigState state;
        private readonly UiState uiState;
        private readonly bool firstRun;
        private TextBox music;
        private TextBox ignored;
        private TextBox configPath;
        private TextBox credentialDir;
        private TextBox cacheDir;
        private TextBox logDir;
        private TextBox historyDir;
        private TextBox scanDir;
        private TextBox fileName;
        private ComboBox existingArtworkAction;
        private NumericUpDown rangeMin;
        private NumericUpDown rangeIdeal;
        private NumericUpDown rangeMax;
        private NumericUpDown rangeLadder;
        private NumericUpDown squareRound;
        private Dictionary<string, Button> optionButtons;
        private Dictionary<string, Button> formatButtons;
        private Dictionary<string, bool> sourceEnabledStates = new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        private ListBox sourcePriority;
        private Button sourceEarlier;
        private Button sourceLater;
        private ComboBox sourceSelector;
        private ComboBox sourceEnabled;
        private ComboBox sourceOverride;
        private ComboBox sourceMinimumRange;
        private ComboBox sourceBelowFallback;
        private ComboBox sourcePrimaryOnly;
        private TableLayoutPanel sourceSettingsTable;
        private TableLayoutPanel advancedSourceTable;
        private GroupBox advancedSourceGroup;
        private Control artworkResolutionRangeGroup;
        private GroupBox musicBrainzOptionsGroup;
        private NumericUpDown musicBrainzRetryMax;
        private NumericUpDown musicBrainzMinDelay;
        private NumericUpDown musicBrainzRecordingTimeout;
        private MusicBrainzRuntimeOptions musicBrainzOptions;
        private TextBox sourceMinimumShort;
        private TextBox sourceMaximumShort;
        private TextBox sourceMinimumWidth;
        private TextBox sourceMinimumHeight;
        private Label sourceDerivedMinimum;
        private Label sourcePolicyValidation;
        private Label sourcePolicySummary;
        private Label sourceConstraintSummary;
        private TableLayoutPanel sourceRangePreview;
        private readonly List<Control> sourceOverrideControls = new List<Control>();
        private bool loadingSourceEditor;
        private string currentSourceKey;
        private ComboBox mode;
        private ComboBox verbosity;
        private CheckBox scanMode;
        private CheckBox libraryScan;
        private CheckBox showConfirmations;
        private NumericUpDown timeout;
        private CheckBox sampleWrite;
        private NumericUpDown logDays;
        private ComboBox historyRetention;
        private TabControl primaryTabs;
        private TabControl advancedTabs;
        private TableLayoutPanel rootLayout;
        public ConfigState SavedState { get; private set; }

        public SetupForm(ConfigState source, bool isFirstRun)
            : this(source, isFirstRun, ConfigStore.LoadUi())
        {
        }

        internal SetupForm(ConfigState source, bool isFirstRun, UiState initialUiState)
        {
            state = source.Clone();
            musicBrainzOptions = CredentialStore.LoadMusicBrainzOptions(state);
            uiState = initialUiState ?? ConfigStore.LoadUi();
            ThemeManager.EnsureInitialized(uiState.Theme);
            ThemeManager.PrepareForm(this);
            firstRun = isFirstRun;
            Text = "SPLINED - Setup / Settings";
            StartPosition = FormStartPosition.CenterParent;
            MinimumSize = new Size(820, 650);
            Size = new Size(Math.Max(820, uiState.SetupWidth), Math.Max(650, uiState.SetupHeight));
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;
            ShowIcon = true;
            BuildLayout();
            Populate();
            RestoreWindowState();
            ThemeManager.Apply(this, uiState.Theme);
            RefreshToggleVisuals();
            UpdateSourcePolicyPreview();
            ThemeManager.PrepareForFirstShow(this, uiState.Theme);
            Resize += delegate { ApplyResponsivePadding(); };
            FormClosing += SetupFormClosing;
            ApplyResponsivePadding();
        }

        private void RestoreWindowState()
        {
            if (!firstRun)
            {
                primaryTabs.SelectedIndex = Math.Max(0, Math.Min(uiState.SetupPrimaryTab, primaryTabs.TabCount - 1));
                advancedTabs.SelectedIndex = Math.Max(0, Math.Min(uiState.SetupAdvancedTab, advancedTabs.TabCount - 1));
            }
            if (uiState.SetupX >= 0 && uiState.SetupY >= 0)
            {
                Rectangle saved = new Rectangle(uiState.SetupX, uiState.SetupY, Width, Height);
                if (Screen.AllScreens.Any(screen => screen.WorkingArea.IntersectsWith(saved)))
                {
                    StartPosition = FormStartPosition.Manual;
                    Location = saved.Location;
                }
            }
            if (uiState.SetupMaximized) WindowState = FormWindowState.Maximized;
        }

        private void SetupFormClosing(object sender, FormClosingEventArgs e)
        {
            Rectangle bounds = WindowState == FormWindowState.Normal ? Bounds : RestoreBounds;
            if (bounds.Width > 0 && bounds.Height > 0)
            {
                uiState.SetupWidth = bounds.Width;
                uiState.SetupHeight = bounds.Height;
                uiState.SetupX = bounds.X;
                uiState.SetupY = bounds.Y;
            }
            uiState.SetupMaximized = WindowState == FormWindowState.Maximized;
            if (primaryTabs != null) uiState.SetupPrimaryTab = primaryTabs.SelectedIndex;
            if (advancedTabs != null) uiState.SetupAdvancedTab = advancedTabs.SelectedIndex;
            ConfigStore.SaveUi(uiState);
        }

        private void BuildLayout()
        {
            // Keep Settings opaque. The full-form watermark caused transparent
            // child corners to reveal stale pixels during scroll/composited paint.
            // Branding remains in Artwork Candidates and Preview where it cannot
            // interfere with information-dense controls.
            TableLayoutPanel root = new TableLayoutPanel();
            rootLayout = root;
            root.Dock = DockStyle.Fill;
            root.Padding = new Padding(18, 14, 18, 14);
            root.ColumnCount = 1;
            root.RowCount = 7;
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 8));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 52));
            Controls.Add(root);

            Label title = new Label();
            title.Dock = DockStyle.Fill;
            title.Text = "S:P:L:I:N:E:D Setup";
            title.Font = ThemeManager.UiFont(ThemeFontRole.AppTitle);
            title.TextAlign = ContentAlignment.MiddleLeft;
            root.Controls.Add(title, 0, 0);

            Label intro = new Label();
            intro.Dock = DockStyle.Fill;
            intro.Text = "SPLINED finds, evaluates, and installs consistent album artwork for your music library. Choose the library to scan, then keep the portable setup or customize it under Advanced.";
            intro.AutoEllipsis = true;
            root.Controls.Add(intro, 0, 1);

            music = CreatePathRow(root, 2, "Music library", true, false);

            Panel ignoredPanel = new Panel();
            ignoredPanel.Dock = DockStyle.Fill;
            Label ignoredLabel = new Label();
            ignoredLabel.Text = "Excluded folders - comma separated (saved in Config v5)";
            ignoredLabel.Dock = DockStyle.Top;
            ignoredLabel.Height = 22;
            ignored = new FluentTextBox();
            ignored.Name = "ignoredDirectories";
            ignored.Dock = DockStyle.Top;
            ignoredPanel.Controls.Add(ignored);
            ignoredPanel.Controls.Add(ignoredLabel);
            root.Controls.Add(ignoredPanel, 0, 3);

            primaryTabs = new ThemedTabControl();
            primaryTabs.Dock = DockStyle.Fill;
            primaryTabs.TabPages.Add(BuildRecommendedTab());
            primaryTabs.TabPages.Add(BuildAdvancedTab());
            root.Controls.Add(primaryTabs, 0, 5);

            FlowLayoutPanel actions = new FlowLayoutPanel();
            actions.Dock = DockStyle.Fill;
            actions.FlowDirection = FlowDirection.RightToLeft;
            actions.WrapContents = false;
            actions.Padding = new Padding(0, 8, 0, 0);
            Button save = ActionButton("Save and Continue", 165);
            save.Tag = "primary";
            save.Click += SaveClicked;
            Button cancel = ActionButton("Cancel", 120);
            cancel.DialogResult = DialogResult.Cancel;
            Button help = ActionButton("Help", 120);
            help.Click += delegate { HelpWindows.ShowSetupHelp(this); };
            actions.Controls.Add(save);
            actions.Controls.Add(cancel);
            Panel spacer = new Panel();
            spacer.Width = 10;
            actions.Controls.Add(spacer);
            actions.Controls.Add(help);
            root.Controls.Add(actions, 0, 6);
            AcceptButton = save;
            CancelButton = cancel;
        }

        private void ApplyResponsivePadding()
        {
            if (rootLayout == null) return;
            int horizontal = Math.Max(18, (ClientSize.Width - 1500) / 2);
            rootLayout.Padding = new Padding(horizontal, 14, horizontal, 14);
        }

        private TextBox CreatePathRow(TableLayoutPanel root, int row, string labelText, bool folder, bool file)
        {
            TableLayoutPanel panel = new TableLayoutPanel();
            panel.Dock = DockStyle.Fill;
            panel.ColumnCount = 2;
            panel.RowCount = 2;
            panel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            panel.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 112));
            panel.RowStyles.Add(new RowStyle(SizeType.Absolute, 23));
            panel.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            Label label = new Label();
            label.Text = labelText;
            label.Dock = DockStyle.Fill;
            label.TextAlign = ContentAlignment.BottomLeft;
            panel.Controls.Add(label, 0, 0);
            panel.SetColumnSpan(label, 2);
            TextBox box = new FluentTextBox();
            box.Dock = DockStyle.Fill;
            panel.Controls.Add(box, 0, 1);
            Button browse = ActionButton("Browse...", 102);
            browse.Dock = DockStyle.Fill;
            browse.Click += delegate
            {
                if (file)
                {
                    using (SaveFileDialog dialog = new SaveFileDialog())
                    {
                        dialog.Filter = "TOML configuration (*.toml)|*.toml|All files (*.*)|*.*";
                        dialog.FileName = Path.GetFileName(box.Text);
                        dialog.InitialDirectory = ExistingDirectory(box.Text);
                        if (dialog.ShowDialog(this) == DialogResult.OK) box.Text = dialog.FileName;
                    }
                }
                else if (folder)
                {
                    using (FolderBrowserDialog dialog = new FolderBrowserDialog())
                    {
                        dialog.Description = "Select " + labelText.ToLowerInvariant();
                        if (Directory.Exists(box.Text)) dialog.SelectedPath = box.Text;
                        if (dialog.ShowDialog(this) == DialogResult.OK) box.Text = dialog.SelectedPath;
                    }
                }
            };
            panel.Controls.Add(browse, 1, 1);
            root.Controls.Add(panel, 0, row);
            return box;
        }

        private TabPage BuildRecommendedTab()
        {
            TabPage page = new TabPage("Recommended");
            TableLayoutPanel layout = new TableLayoutPanel();
            layout.Dock = DockStyle.Fill;
            layout.Padding = new Padding(20, 14, 20, 14);
            layout.RowCount = 8;
            layout.ColumnCount = 1;
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 92));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 102));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 25));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 170));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 26));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            page.Controls.Add(layout);

            Label purpose = new Label();
            purpose.Dock = DockStyle.Fill;
            purpose.Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle);
            purpose.Text = "SPLINED scans your music library, finds and evaluates album artwork, and lets you review or apply the best available artwork.";
            layout.Controls.Add(purpose, 0, 0);

            Label workflow = new Label();
            workflow.Dock = DockStyle.Fill;
            workflow.Text = "1. Choose the music library.\r\n2. Review the proposed configuration.\r\n3. Save and open the library.\r\n4. Select artists or albums and launch.";
            layout.Controls.Add(workflow, 0, 1);

            GroupBox defaults = new FluentGroupBox { Text = "Portable defaults", Dock = DockStyle.Fill };
            Label summary = new Label();
            summary.Dock = DockStyle.Fill;
            summary.Padding = new Padding(12, 8, 12, 8);
            summary.Text = "Configuration: stored beside SPLINED        Credentials: stored beside SPLINED\r\nCache: stored beside SPLINED                       Logs: retained for 14 days\r\nHistory: retained forever";
            defaults.Controls.Add(summary);
            layout.Controls.Add(defaults, 0, 2);

            FlowLayoutPanel choices = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false };
            Button setUp = ActionButton("Set Up SPLINED", 155);
            setUp.Tag = "primary";
            setUp.Click += delegate { music.Focus(); music.SelectAll(); };
            Button useExisting = ActionButton("Use Existing Configuration...", 235);
            useExisting.Click += UseExistingConfiguration;
            Button advanced = ActionButton("Advanced Settings", 165);
            advanced.Click += delegate { primaryTabs.SelectedIndex = 1; };
            choices.Controls.Add(setUp);
            choices.Controls.Add(useExisting);
            choices.Controls.Add(advanced);
            choices.Controls.Add(new InfoButton("Set Up uses portable defaults, Use Existing opens a prior Config v5 without resetting it, and Advanced exposes every Config v5 control."));
            layout.Controls.Add(choices, 0, 3);

            Label details = new Label { Text = "Portable folder detail", Dock = DockStyle.Fill, TextAlign = ContentAlignment.BottomLeft };
            layout.Controls.Add(details, 0, 4);

            TextBox tree = new FluentTextBox();
            tree.Dock = DockStyle.Fill;
            tree.Multiline = true;
            tree.ReadOnly = true;
            tree.ScrollBars = ScrollBars.Vertical;
            tree.Font = new Font("Consolas", 9.5f);
            tree.Text = "<where SPLINED is run>\r\n+-- config\r\n|   +-- config.toml\r\n|   +-- ui.toml\r\n+-- credentials\r\n+-- _cache\r\n|   +-- samples\r\n+-- _logs\r\n    +-- _history";
            layout.Controls.Add(tree, 0, 5);

            Label active = new Label();
            active.Name = "activeConfigLabel";
            active.Dock = DockStyle.Fill;
            active.AutoEllipsis = true;
            layout.Controls.Add(active, 0, 6);
            return page;
        }

        private TabPage BuildAdvancedTab()
        {
            TabPage page = new TabPage("Advanced");
            TableLayoutPanel layout = new TableLayoutPanel();
            layout.Dock = DockStyle.Fill;
            layout.Padding = new Padding(10);
            layout.RowCount = 2;
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            Label hint = new Label();
            hint.Dock = DockStyle.Fill;
            hint.Text = "Advanced is optional. Portable values are already filled in; change only the setting you intend to customize.";
            layout.Controls.Add(hint, 0, 0);
            advancedTabs = new ThemedTabControl();
            advancedTabs.Dock = DockStyle.Fill;
            advancedTabs.TabPages.Add(BuildPathsTab());
            advancedTabs.TabPages.Add(BuildArtworkTab());
            advancedTabs.TabPages.Add(BuildSourcesTab());
            layout.Controls.Add(advancedTabs, 0, 1);
            page.Controls.Add(layout);
            return page;
        }

        private TabPage BuildPathsTab()
        {
            TabPage page = new TabPage("Library, Paths & Processing");
            Panel scroll = new Panel { Dock = DockStyle.Fill, AutoScroll = true, Padding = new Padding(10) };
            TableLayoutPanel stack = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 1, RowCount = 4 };
            stack.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int row = 0; row < 4; row++) stack.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            scroll.Controls.Add(stack);
            page.Controls.Add(scroll);

            GroupBox paths = new FluentGroupBox { Name = "pathsGroup", Text = "Application and Library Paths", Dock = DockStyle.Top, AutoSize = true, Padding = new Padding(10) };
            TableLayoutPanel table = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 3, RowCount = 8 };
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 185));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 102));
            table.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            for (int index = 1; index <= 6; index++) table.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            table.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            paths.Controls.Add(table);
            stack.Controls.Add(paths, 0, 0);
            Label info = new Label();
            info.Dock = DockStyle.Fill;
            info.Text = "Paths inside the SPLINED folder are saved as portable relative paths. External, NAS, and cloud paths remain absolute.";
            table.Controls.Add(info, 0, 0);
            table.SetColumnSpan(info, 2);
            InfoButton help = new InfoButton("Portable paths move with the SPLINED folder. A selected external path is retained exactly.");
            help.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            table.Controls.Add(help, 2, 0);
            configPath = AddPathSetting(table, 1, "Configuration file", true);
            credentialDir = AddPathSetting(table, 2, "Credential directory", false);
            cacheDir = AddPathSetting(table, 3, "Cache directory", false);
            logDir = AddPathSetting(table, 4, "Log directory", false);
            historyDir = AddPathSetting(table, 5, "History directory", false);
            scanDir = AddPathSetting(table, 6, "Scan directory (optional)", false);
            FlowLayoutPanel pathActions = new FlowLayoutPanel { Name = "pathsActionRow", Dock = DockStyle.Fill, WrapContents = false, AutoSize = true };
            Button credentialsButton = ActionButton("Credentials / Status...", 190);
            credentialsButton.Name = "pathsCredentialsButton";
            credentialsButton.Click += delegate { using (CredentialsForm form = new CredentialsForm(state)) form.ShowDialog(this); };
            Button restore = ActionButton("Restore Portable Defaults", 210);
            restore.Tag = "reset";
            restore.Anchor = AnchorStyles.Left | AnchorStyles.Top;
            restore.Click += delegate { RestorePortablePaths(); };
            pathActions.Controls.Add(credentialsButton);
            pathActions.Controls.Add(restore);
            pathActions.Controls.Add(new InfoButton("Credentials remain JSON files beneath the configured credential directory. Restoring portable paths does not rewrite or expose credential contents."));
            table.Controls.Add(pathActions, 0, 7);
            table.SetColumnSpan(pathActions, 3);

            stack.Controls.Add(BuildAdvancedCommandsGroup(), 0, 1);
            stack.Controls.Add(BuildRuntimeSettingsGroup(), 0, 2);
            stack.Controls.Add(BuildRetentionPair(), 0, 3);

            return page;
        }

        private TextBox AddPathSetting(TableLayoutPanel table, int row, string label, bool file)
        {
            Label title = new Label();
            title.Text = label;
            title.Dock = DockStyle.Fill;
            title.TextAlign = ContentAlignment.MiddleLeft;
            table.Controls.Add(title, 0, row);
            TextBox box = new FluentTextBox();
            box.Dock = DockStyle.Fill;
            table.Controls.Add(box, 1, row);
            Button browse = ActionButton("Browse...", 92);
            browse.Dock = DockStyle.Top;
            browse.Click += delegate
            {
                if (file)
                {
                    using (SaveFileDialog dialog = new SaveFileDialog())
                    {
                        dialog.Filter = "TOML configuration (*.toml)|*.toml|All files (*.*)|*.*";
                        dialog.FileName = Path.GetFileName(box.Text);
                        dialog.InitialDirectory = ExistingDirectory(box.Text);
                        if (dialog.ShowDialog(this) == DialogResult.OK) box.Text = dialog.FileName;
                    }
                }
                else
                {
                    using (FolderBrowserDialog dialog = new FolderBrowserDialog())
                    {
                        if (Directory.Exists(box.Text)) dialog.SelectedPath = box.Text;
                        if (dialog.ShowDialog(this) == DialogResult.OK) box.Text = dialog.SelectedPath;
                    }
                }
            };
            table.Controls.Add(browse, 2, row);
            return box;
        }

        private TabPage BuildArtworkTab()
        {
            TabPage page = new TabPage("Artwork & Output");
            TableLayoutPanel root = new TableLayoutPanel();
            root.Dock = DockStyle.Fill;
            root.Padding = new Padding(15);
            root.ColumnCount = 1;
            root.RowCount = 3;
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 88));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            page.Controls.Add(root);

            FlowLayoutPanel nameRow = new FlowLayoutPanel();
            nameRow.Dock = DockStyle.Fill;
            nameRow.WrapContents = false;
            nameRow.Controls.Add(new Label { Text = "Output file name", Width = 135, TextAlign = ContentAlignment.MiddleLeft, Height = 28 });
            fileName = new FluentTextBox();
            fileName.Width = 180;
            nameRow.Controls.Add(fileName);
            nameRow.Controls.Add(new InfoButton("Filename stem only. Enabled output formats determine whether SPLINED writes cover.jpg, cover.png, or cover.webp."));
            root.Controls.Add(nameRow, 0, 0);

            GroupBox replacement = new FluentGroupBox { Text = "Existing JPG / PNG Files", Dock = DockStyle.Fill, Padding = new Padding(10) };
            TableLayoutPanel replacementRow = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 3, RowCount = 1 };
            replacementRow.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 165));
            replacementRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            replacementRow.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 30));
            replacementRow.Controls.Add(new Label { Text = "When better art is chosen", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 0);
            existingArtworkAction = new FluentComboBox { Name = "existingArtworkAction", Dock = DockStyle.Fill };
            existingArtworkAction.Items.AddRange(new object[]
            {
                "Replace matching JPG/PNG files (no numbered copies)",
                "Preserve existing files and create cover-(2), cover-(3)…"
            });
            replacementRow.Controls.Add(existingArtworkAction, 1, 0);
            replacementRow.Controls.Add(new InfoButton("Replace removes superseded matching cover*.jpg, cover*.jpeg, and cover*.png files after the better artwork is safely installed. WebP files are never removed. Preserve creates numbered copies instead."), 2, 0);
            replacement.Controls.Add(replacementRow);
            root.Controls.Add(replacement, 0, 1);

            GroupBox options = new FluentGroupBox();
            options.Name = "artworkOptionsGroup";
            options.Text = "Artwork Options";
            options.Dock = DockStyle.Fill;
            TableLayoutPanel optionColumns = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Padding = new Padding(8) };
            optionColumns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 58));
            optionColumns.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 42));
            optionButtons = new Dictionary<string, Button>(StringComparer.OrdinalIgnoreCase);
            FlowLayoutPanel optionFlow = new FlowLayoutPanel();
            optionFlow.Dock = DockStyle.Fill;
            optionFlow.FlowDirection = FlowDirection.TopDown;
            optionFlow.WrapContents = false;
            optionFlow.Padding = new Padding(12, 10, 12, 10);
            optionFlow.Controls.Add(CreateToggle("Square artwork", "square", optionButtons));
            optionFlow.Controls.Add(CreateToggle("Crop to square when required", "crop", optionButtons));
            optionFlow.Controls.Add(CreateToggle("Upscale below ideal", "upscale", optionButtons));
            optionFlow.Controls.Add(CreateToggle("Evaluate final image", "evaluate", optionButtons));

            formatButtons = new Dictionary<string, Button>(StringComparer.OrdinalIgnoreCase);
            FlowLayoutPanel formatFlow = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false, Padding = new Padding(12, 10, 12, 10) };
            foreach (KeyValuePair<string, string> format in new[]
            {
                new KeyValuePair<string, string>("jpeg", "JPEG"),
                new KeyValuePair<string, string>("png", "PNG"),
                new KeyValuePair<string, string>("webp", "WebP")
            })
            {
                Button button = CreateToggle(format.Value, format.Key, formatButtons);
                button.Width = 245;
                formatFlow.Controls.Add(button);
            }

            GroupBox processing = new FluentGroupBox { Text = "Image Processing", Dock = DockStyle.Fill };
            processing.Controls.Add(optionFlow);
            GroupBox formats = new FluentGroupBox { Text = "Output Formats", Dock = DockStyle.Fill };
            formats.Controls.Add(formatFlow);
            optionColumns.Controls.Add(processing, 0, 0);
            optionColumns.Controls.Add(formats, 1, 0);
            options.Controls.Add(optionColumns);
            root.Controls.Add(options, 0, 2);
            return page;
        }

        private Control BuildArtworkResolutionRangeGroup()
        {
            GroupBox range = new FluentGroupBox { Name = "artworkResolutionGroup", Text = "Artwork Resolution Range (Config v5)", Dock = DockStyle.Fill, Padding = new Padding(8) };
            TableLayoutPanel ranges = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 4, RowCount = 4, Padding = new Padding(8) };
            for (int index = 0; index < 4; index++) ranges.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 25));
            string[] names = { "Min", "Ideal", "Max", "Ladder" };
            NumericUpDown[] values = new NumericUpDown[4];
            for (int index = 0; index < 4; index++)
            {
                ranges.Controls.Add(new Label { Text = names[index], Dock = DockStyle.Fill, TextAlign = ContentAlignment.BottomLeft }, index, 0);
                values[index] = new FluentNumericUpDown { Minimum = 1, Maximum = 20000, Dock = DockStyle.Fill, ThousandsSeparator = true };
                ranges.Controls.Add(values[index], index, 1);
            }
            rangeMin = values[0];
            rangeIdeal = values[1];
            rangeMax = values[2];
            rangeLadder = values[3];
            Label recommended = new Label { Text = "Recommended: 1200 / 1800 / 2400 / 3600", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft };
            ranges.Controls.Add(recommended, 0, 2);
            ranges.SetColumnSpan(recommended, 3);
            InfoButton rangeInfo = new InfoButton("The global scale supplies the named ranges used by every source policy. Minimum < Ideal <= Maximum < Ladder.");
            rangeInfo.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            ranges.Controls.Add(rangeInfo, 3, 2);
            FlowLayoutPanel round = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false, AutoSize = true };
            round.Controls.Add(new Label { Text = "Square round-to", Width = 135, Height = 28, TextAlign = ContentAlignment.MiddleLeft });
            squareRound = new FluentNumericUpDown { Minimum = 0, Maximum = 512, Width = 90 };
            round.Controls.Add(squareRound);
            ranges.Controls.Add(round, 0, 3);
            ranges.SetColumnSpan(round, 4);
            range.Controls.Add(ranges);
            return range;
        }

        private TabPage BuildSourcesTab()
        {
            TabPage page = new TabPage("Sources & Matching");
            TableLayoutPanel root = new TableLayoutPanel();
            root.Dock = DockStyle.Fill;
            root.Padding = new Padding(10);
            root.ColumnCount = 2;
            root.RowCount = 1;
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            page.Controls.Add(root);

            Panel leftScroll = new Panel { Dock = DockStyle.Fill, AutoScroll = true, Padding = new Padding(0, 0, 8, 0) };
            TableLayoutPanel left = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 1, RowCount = 3 };
            left.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            left.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            left.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            left.RowStyles.Add(new RowStyle(SizeType.Absolute, 190));
            leftScroll.Controls.Add(left);
            root.Controls.Add(leftScroll, 0, 0);

            GroupBox sourceBox = new FluentGroupBox { Text = "Source Settings", Dock = DockStyle.Top, AutoSize = true, Padding = new Padding(8) };
            sourceSettingsTable = CreateSourceSettingsTable(8);
            TableLayoutPanel sourceTable = sourceSettingsTable;
            sourceSelector = SourceCombo();
            foreach (string key in ConfigState.KnownSources)
                sourceSelector.Items.Add(new SourceChoice(key, SourceDisplayName(key)));
            AddSourceRow(sourceTable, 0, "Source", sourceSelector, "Select the artwork provider whose saved source policy is being edited.");

            sourceEnabled = YesNoCombo();
            AddSourceRow(sourceTable, 1, "Source Enabled", sourceEnabled, "Enables or disables this provider without deleting its saved policy.");

            sourceOverride = YesNoCombo();
            AddSourceRow(sourceTable, 2, "Source Override", sourceOverride, "No uses SPLINED's existing global artwork-range behavior. Yes activates this provider's saved custom range and dimension policy.");

            sourceMinimumRange = new FluentComboBox { Dock = DockStyle.Fill };
            sourceMinimumRange.Items.AddRange(SourcePolicyRules.RangeTypes.Cast<object>().ToArray());
            AddSourceRow(sourceTable, 3, "Minimum Range Type", sourceMinimumRange, "The lowest normal SPLINED range accepted for this provider when Source Override is Yes.");

            sourceBelowFallback = SourceCombo();
            sourceBelowFallback.Name = "sourceFallbackRange";
            sourceBelowFallback.Items.Add("Disabled");
            AddSourceRow(sourceTable, 4, "Fallback range", sourceBelowFallback, "Allows only the single SPLINED range immediately below Minimum Range Type when no accepted candidate is available. It never opens every lower range.");

            sourcePolicyValidation = new Label { Dock = DockStyle.Fill, AutoSize = true, ForeColor = ThemeManager.PaletteFor(uiState.Theme).Error, Padding = new Padding(3, 3, 3, 5) };
            sourceTable.Controls.Add(sourcePolicyValidation, 0, 5);
            sourceTable.SetColumnSpan(sourcePolicyValidation, 3);
            sourceBox.Controls.Add(sourceTable);
            left.Controls.Add(sourceBox, 0, 0);

            advancedSourceGroup = new FluentGroupBox { Name = "advancedSourceConstraintsGroup", Text = "Advanced Source Constraints", Dock = DockStyle.Top, AutoSize = true, Padding = new Padding(8) };
            GroupBox advanced = advancedSourceGroup;
            advancedSourceTable = CreateSourceSettingsTable(8);
            TableLayoutPanel advancedTable = advancedSourceTable;
            sourceMinimumShort = DimensionBox();
            AddSourceDimensionRow(advancedTable, 0, "Minimum short side", sourceMinimumShort, "Blank adds no custom minimum. The selected Range Type remains the normal derived threshold.");
            sourceDerivedMinimum = new Label { Dock = DockStyle.Fill, AutoSize = true, Padding = new Padding(3, 0, 3, 4) };
            advancedTable.Controls.Add(sourceDerivedMinimum, 0, 1);
            advancedTable.SetColumnSpan(sourceDerivedMinimum, 3);
            sourceMaximumShort = DimensionBox();
            AddSourceDimensionRow(advancedTable, 2, "Maximum short side", sourceMaximumShort, "Blank means no additional source maximum; this is independent of the global ladder when override is active.");
            sourceMinimumWidth = DimensionBox();
            AddSourceDimensionRow(advancedTable, 3, "Minimum width", sourceMinimumWidth, "Optional hard width requirement for this source.");
            sourceMinimumHeight = DimensionBox();
            AddSourceDimensionRow(advancedTable, 4, "Minimum height", sourceMinimumHeight, "Optional hard height requirement for this source.");
            sourcePrimaryOnly = YesNoCombo();
            AddSourceRow(advancedTable, 5, "Primary image only", sourcePrimaryOnly, "Uses provider-supplied front/primary metadata only. No visual content recognition is performed. Available where the provider supplies this metadata.");
            Button useDerived = ActionButton("Clear custom minimum", 185);
            useDerived.Click += delegate { sourceMinimumShort.Text = ""; };
            FlowLayoutPanel derivedAction = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false, AutoSize = true };
            derivedAction.Controls.Add(useDerived);
            derivedAction.Controls.Add(new InfoButton("Clearing the custom minimum returns authority to the selected Minimum Range Type while retaining the global range thresholds."));
            advancedTable.Controls.Add(derivedAction, 0, 6);
            advancedTable.SetColumnSpan(derivedAction, 3);
            musicBrainzOptionsGroup = BuildMusicBrainzOptionsGroup();
            musicBrainzOptionsGroup.Margin = new Padding(0, 8, 0, 0);
            advancedTable.Controls.Add(musicBrainzOptionsGroup, 0, 7);
            advancedTable.SetColumnSpan(musicBrainzOptionsGroup, 3);
            advanced.Controls.Add(advancedTable);
            left.Controls.Add(advanced, 0, 1);

            sourceOverrideControls.Add(sourceMinimumRange);
            sourceOverrideControls.Add(sourceBelowFallback);
            sourceOverrideControls.Add(sourceMinimumShort);
            sourceOverrideControls.Add(sourceMaximumShort);
            sourceOverrideControls.Add(sourceMinimumWidth);
            sourceOverrideControls.Add(sourceMinimumHeight);
            sourceOverrideControls.Add(sourcePrimaryOnly);
            sourceOverrideControls.Add(useDerived);

            left.Controls.Add(BuildSourcePriorityGroup(), 0, 2);

            root.Controls.Add(BuildSourcePolicyPreview(), 1, 0);

            sourceSelector.SelectedIndexChanged += SourceSelectionChanged;
            sourceEnabled.SelectedIndexChanged += SourceEditorChanged;
            sourceOverride.SelectedIndexChanged += SourceEditorChanged;
            sourceMinimumRange.SelectedIndexChanged += SourceMinimumRangeChanged;
            sourceBelowFallback.SelectedIndexChanged += SourceEditorChanged;
            sourcePrimaryOnly.SelectedIndexChanged += SourceEditorChanged;
            foreach (TextBox box in new[] { sourceMinimumShort, sourceMaximumShort, sourceMinimumWidth, sourceMinimumHeight })
            {
                box.TextChanged += SourceEditorChanged;
                box.KeyPress += DimensionKeyPress;
            }
            foreach (NumericUpDown globalRange in new[] { rangeMin, rangeIdeal, rangeMax, rangeLadder })
                globalRange.ValueChanged += delegate { UpdateSourcePolicyPreview(); };
            return page;
        }

        private Control BuildSourcePriorityGroup()
        {
            GroupBox group = new FluentGroupBox
            {
                Name = "artworkSourcePriorityGroup",
                Text = "Artwork Source Priority",
                Dock = DockStyle.Fill,
                Padding = new Padding(8)
            };
            TableLayoutPanel layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 2 };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 112));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            Label explanation = new Label
            {
                Text = "Earlier sources win otherwise equal scoring decisions. Disabled sources remain in their saved position.",
                Dock = DockStyle.Fill,
                AutoEllipsis = true
            };
            layout.Controls.Add(explanation, 0, 0);
            layout.SetColumnSpan(explanation, 2);

            sourcePriority = new ListBox { Name = "artworkSourcePriority", Dock = DockStyle.Fill, IntegralHeight = false };
            sourcePriority.SelectedIndexChanged += delegate { UpdateSourcePriorityButtons(); };
            layout.Controls.Add(sourcePriority, 0, 1);

            FlowLayoutPanel actions = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false };
            sourceEarlier = ActionButton("Move Earlier", 100);
            sourceLater = ActionButton("Move Later", 100);
            sourceEarlier.Click += delegate { MoveSourcePriority(-1); };
            sourceLater.Click += delegate { MoveSourcePriority(1); };
            actions.Controls.Add(sourceEarlier);
            actions.Controls.Add(sourceLater);
            actions.Controls.Add(new InfoButton("This edits [sources].cover_sources order. Source Enabled controls exclusions without discarding priority."));
            layout.Controls.Add(actions, 1, 1);
            group.Controls.Add(layout);
            return group;
        }

        private void MoveSourcePriority(int offset)
        {
            if (sourcePriority == null || sourcePriority.SelectedIndex < 0) return;
            int current = sourcePriority.SelectedIndex;
            int target = current + offset;
            if (target < 0 || target >= sourcePriority.Items.Count) return;
            object selected = sourcePriority.Items[current];
            sourcePriority.Items.RemoveAt(current);
            sourcePriority.Items.Insert(target, selected);
            sourcePriority.SelectedIndex = target;
            UpdateSourcePriorityButtons();
        }

        private void UpdateSourcePriorityButtons()
        {
            if (sourceEarlier == null || sourceLater == null || sourcePriority == null) return;
            sourceEarlier.Enabled = sourcePriority.SelectedIndex > 0;
            sourceLater.Enabled = sourcePriority.SelectedIndex >= 0 && sourcePriority.SelectedIndex < sourcePriority.Items.Count - 1;
        }

        private GroupBox BuildMusicBrainzOptionsGroup()
        {
            GroupBox group = new FluentGroupBox { Name = "musicBrainzOptionsGroup", Text = "MusicBrainz Runtime Options", Dock = DockStyle.Top, AutoSize = true, Padding = new Padding(8) };
            TableLayoutPanel table = CreateSourceSettingsTable(3);
            musicBrainzRetryMax = new FluentNumericUpDown { Name = "musicBrainzRetryMax", Minimum = 0, Maximum = 20, Dock = DockStyle.Fill };
            AddSourceRow(table, 0, "Retry maximum", musicBrainzRetryMax, "Maximum retry count for retryable MusicBrainz requests. Default: 4.");
            musicBrainzMinDelay = new FluentNumericUpDown { Name = "musicBrainzMinDelay", Minimum = 0.01M, Maximum = 60, DecimalPlaces = 2, Increment = 0.05M, Dock = DockStyle.Fill };
            AddSourceRow(table, 1, "Minimum delay (seconds)", musicBrainzMinDelay, "Minimum interval between MusicBrainz requests. Default: 1.05 seconds.");
            musicBrainzRecordingTimeout = new FluentNumericUpDown { Name = "musicBrainzRecordingTimeout", Minimum = 1, Maximum = 300, DecimalPlaces = 0, Dock = DockStyle.Fill };
            AddSourceRow(table, 2, "Recording timeout (seconds)", musicBrainzRecordingTimeout, "Timeout for MusicBrainz recording queries. Default: 7 seconds.");
            foreach (NumericUpDown control in new[] { musicBrainzRetryMax, musicBrainzMinDelay, musicBrainzRecordingTimeout })
                control.ValueChanged += SourceEditorChanged;
            group.Controls.Add(table);
            return group;
        }

        private TableLayoutPanel CreateSourceSettingsTable(int rows)
        {
            TableLayoutPanel table = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 3, RowCount = rows, Padding = new Padding(2) };
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 43));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 57));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 30));
            for (int index = 0; index < rows; index++) table.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            return table;
        }

        private void AddSourceRow(TableLayoutPanel table, int row, string label, Control editor, string help)
        {
            table.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, AutoSize = true, TextAlign = ContentAlignment.MiddleLeft, Padding = new Padding(0, 5, 3, 5) }, 0, row);
            editor.Margin = new Padding(3, 4, 3, 4);
            table.Controls.Add(editor, 1, row);
            table.Controls.Add(new InfoButton(help), 2, row);
        }

        private void AddSourceDimensionRow(TableLayoutPanel table, int row, string label, TextBox editor, string help)
        {
            table.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, AutoSize = true, TextAlign = ContentAlignment.MiddleLeft, Padding = new Padding(0, 5, 3, 5) }, 0, row);
            FlowLayoutPanel value = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false, AutoSize = true, Margin = new Padding(3, 2, 3, 2) };
            editor.Dock = DockStyle.None;
            editor.Width = 120;
            editor.TextAlign = HorizontalAlignment.Right;
            value.Controls.Add(editor);
            value.Controls.Add(new Label { Text = "px", AutoSize = true, Padding = new Padding(0, 5, 0, 0) });
            table.Controls.Add(value, 1, row);
            table.Controls.Add(new InfoButton(help), 2, row);
        }

        private static ComboBox SourceCombo()
        {
            return new FluentComboBox { Dock = DockStyle.Fill };
        }

        private static ComboBox YesNoCombo()
        {
            ComboBox combo = SourceCombo();
            combo.Items.AddRange(new object[] { "Yes", "No" });
            return combo;
        }

        private static TextBox DimensionBox()
        {
            return new FluentTextBox { Dock = DockStyle.Fill, MaxLength = 6 };
        }

        private Control BuildSourcePolicyPreview()
        {
            GroupBox preview = new FluentGroupBox { Text = "Range Effect / Policy Preview", Dock = DockStyle.Fill, Padding = new Padding(10) };
            Panel previewScroll = new Panel { Dock = DockStyle.Fill, AutoScroll = true };
            TableLayoutPanel layout = new TableLayoutPanel { Dock = DockStyle.Top, Height = 590, ColumnCount = 1, RowCount = 4 };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            // Header plus the six named ranges, with room for the common case where
            // one range is split by an explicit per-source threshold.
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 255));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 205));

            TableLayoutPanel summary = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1 };
            summary.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            summary.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 30));
            sourcePolicySummary = new Label { Dock = DockStyle.Fill, AutoSize = false, Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold), TextAlign = ContentAlignment.MiddleLeft };
            summary.Controls.Add(sourcePolicySummary, 0, 0);
            summary.Controls.Add(new InfoButton("This is the effective policy used by live processing. Green accepts, amber is fallback-only, red rejects, and gray is inactive. Text labels are always shown."), 1, 0);
            layout.Controls.Add(summary, 0, 0);

            Panel rangeScroll = new Panel { Dock = DockStyle.Fill, AutoScroll = true };
            sourceRangePreview = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 3, RowCount = 1, CellBorderStyle = TableLayoutPanelCellBorderStyle.Single };
            sourceRangePreview.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 39));
            sourceRangePreview.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 31));
            sourceRangePreview.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 30));
            rangeScroll.Controls.Add(sourceRangePreview);
            layout.Controls.Add(rangeScroll, 0, 1);

            sourceConstraintSummary = new Label { Dock = DockStyle.Fill, AutoSize = false, TextAlign = ContentAlignment.MiddleLeft, AutoEllipsis = true, Padding = new Padding(3) };
            layout.Controls.Add(sourceConstraintSummary, 0, 2);

            artworkResolutionRangeGroup = BuildArtworkResolutionRangeGroup();
            layout.Controls.Add(artworkResolutionRangeGroup, 0, 3);
            previewScroll.Controls.Add(layout);
            preview.Controls.Add(previewScroll);
            return preview;
        }

        private void SourceSelectionChanged(object sender, EventArgs e)
        {
            if (loadingSourceEditor) return;
            SaveCurrentSourceEditor(false);
            SourceChoice choice = sourceSelector.SelectedItem as SourceChoice;
            currentSourceKey = choice == null ? null : choice.Key;
            LoadCurrentSourceEditor();
        }

        private void SourceEditorChanged(object sender, EventArgs e)
        {
            if (loadingSourceEditor) return;
            SaveCurrentSourceEditor(false);
            ApplySourceOverrideState();
            UpdateDerivedMinimumLabel();
            UpdateSourcePolicyPreview();
        }

        private void SourceMinimumRangeChanged(object sender, EventArgs e)
        {
            if (loadingSourceEditor) return;
            bool keepFallback = sourceBelowFallback != null && sourceBelowFallback.SelectedIndex == 1;
            loadingSourceEditor = true;
            try { PopulateFallbackChoices(keepFallback); }
            finally { loadingSourceEditor = false; }
            SourceEditorChanged(sender, e);
        }

        private void PopulateFallbackChoices(bool enabled)
        {
            string minimum = Convert.ToString(sourceMinimumRange.SelectedItem);
            if (String.IsNullOrWhiteSpace(minimum)) minimum = "LowerRange";
            string adjacent = SourcePolicyRules.AdjacentFallbackRange(minimum);
            sourceBelowFallback.Items.Clear();
            sourceBelowFallback.Items.Add("Disabled");
            if (!String.IsNullOrWhiteSpace(adjacent))
                sourceBelowFallback.Items.Add(adjacent + " (one level below)");
            sourceBelowFallback.SelectedIndex = enabled && sourceBelowFallback.Items.Count > 1 ? 1 : 0;
        }

        private void LoadCurrentSourceEditor()
        {
            if (String.IsNullOrWhiteSpace(currentSourceKey)) return;
            loadingSourceEditor = true;
            try
            {
                SourcePolicyState policy = GetSourcePolicy(currentSourceKey);
                bool enabled;
                if (!sourceEnabledStates.TryGetValue(currentSourceKey, out enabled)) enabled = false;
                sourceEnabled.SelectedIndex = enabled ? 0 : 1;
                sourceOverride.SelectedIndex = policy.SourceOverride ? 0 : 1;
                SelectComboText(sourceMinimumRange, policy.MinimumRangeType, "LowerRange");
                PopulateFallbackChoices(policy.AllowBelowMinimumFallback);
                sourceMinimumShort.Text = OptionalDimensionText(policy.MinimumShortSide);
                sourceMaximumShort.Text = OptionalDimensionText(policy.MaximumShortSide);
                sourceMinimumWidth.Text = OptionalDimensionText(policy.MinimumWidth);
                sourceMinimumHeight.Text = OptionalDimensionText(policy.MinimumHeight);
                sourcePrimaryOnly.SelectedIndex = policy.PrimaryImageOnly ? 0 : 1;
                if (currentSourceKey.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase))
                {
                    musicBrainzRetryMax.Value = Clamp(musicBrainzOptions.RetryMax, musicBrainzRetryMax.Minimum, musicBrainzRetryMax.Maximum);
                    musicBrainzMinDelay.Value = Clamp((decimal)musicBrainzOptions.MinDelay, musicBrainzMinDelay.Minimum, musicBrainzMinDelay.Maximum);
                    musicBrainzRecordingTimeout.Value = Clamp(musicBrainzOptions.RecordingTimeout, musicBrainzRecordingTimeout.Minimum, musicBrainzRecordingTimeout.Maximum);
                }
                sourcePolicyValidation.Text = "";
            }
            finally
            {
                loadingSourceEditor = false;
            }
            ApplySourceOverrideState();
            UpdateDerivedMinimumLabel();
            UpdateSourcePolicyPreview();
        }

        private bool SaveCurrentSourceEditor(bool strict)
        {
            if (String.IsNullOrWhiteSpace(currentSourceKey) || loadingSourceEditor) return true;
            SourcePolicyState policy = GetSourcePolicy(currentSourceKey);
            sourceEnabledStates[currentSourceKey] = sourceEnabled.SelectedIndex == 0;
            policy.Enabled = sourceEnabledStates[currentSourceKey];
            policy.SourceOverride = sourceOverride.SelectedIndex == 0;
            if (currentSourceKey.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase))
            {
                musicBrainzOptions.RetryMax = (int)musicBrainzRetryMax.Value;
                musicBrainzOptions.MinDelay = (double)musicBrainzMinDelay.Value;
                musicBrainzOptions.RecordingTimeout = (int)musicBrainzRecordingTimeout.Value;
                sourcePolicyValidation.Text = "";
                return true;
            }
            int? minimumShort;
            int? maximumShort;
            int? minimumWidth;
            int? minimumHeight;
            string error;
            if (!TryReadDimension(sourceMinimumShort, "Minimum short side", out minimumShort, out error)
                || !TryReadDimension(sourceMaximumShort, "Maximum short side", out maximumShort, out error)
                || !TryReadDimension(sourceMinimumWidth, "Minimum width", out minimumWidth, out error)
                || !TryReadDimension(sourceMinimumHeight, "Minimum height", out minimumHeight, out error))
            {
                sourcePolicyValidation.Text = error;
                sourcePolicyValidation.ForeColor = ThemeManager.PaletteFor(uiState.Theme).Error;
                if (strict) throw new InvalidOperationException(error);
                return false;
            }
            if (minimumShort.HasValue && maximumShort.HasValue && minimumShort.Value > maximumShort.Value)
            {
                error = "Minimum short side cannot exceed maximum short side.";
                sourcePolicyValidation.Text = error;
                sourcePolicyValidation.ForeColor = ThemeManager.PaletteFor(uiState.Theme).Error;
                if (strict) throw new InvalidOperationException(error);
                return false;
            }

            policy.MinimumRangeType = Convert.ToString(sourceMinimumRange.SelectedItem);
            if (String.IsNullOrWhiteSpace(policy.MinimumRangeType)) policy.MinimumRangeType = "LowerRange";
            policy.AllowBelowMinimumFallback = sourceBelowFallback.SelectedIndex == 1;
            policy.MinimumShortSide = minimumShort;
            policy.MaximumShortSide = maximumShort;
            policy.MinimumWidth = minimumWidth;
            policy.MinimumHeight = minimumHeight;
            policy.PrimaryImageOnly = sourcePrimaryOnly.SelectedIndex == 0;
            sourcePolicyValidation.Text = "";
            return true;
        }

        private void ApplySourceOverrideState()
        {
            bool active = sourceOverride != null && sourceOverride.SelectedIndex == 0;
            bool musicBrainz = "musicbrainz".Equals(currentSourceKey, StringComparison.OrdinalIgnoreCase);
            if (advancedSourceGroup != null) advancedSourceGroup.Text = musicBrainz ? "Advanced Source Settings" : "Advanced Source Constraints";
            if (sourcePolicyValidation != null) sourcePolicyValidation.Visible = !musicBrainz;
            SetRowsVisible(sourceSettingsTable, new[] { 3, 4 }, !musicBrainz);
            SetRowsVisible(advancedSourceTable, Enumerable.Range(0, 7), !musicBrainz);
            if (musicBrainzOptionsGroup != null) musicBrainzOptionsGroup.Visible = musicBrainz;
            if (artworkResolutionRangeGroup != null) artworkResolutionRangeGroup.Visible = !musicBrainz;
            foreach (Control control in sourceOverrideControls)
                control.Enabled = active && !musicBrainz;
            foreach (NumericUpDown control in new[] { musicBrainzRetryMax, musicBrainzMinDelay, musicBrainzRecordingTimeout })
                if (control != null) control.Enabled = active && musicBrainz;
            if (sourceBelowFallback != null)
                sourceBelowFallback.Enabled = active && !musicBrainz && sourceBelowFallback.Items.Count > 1;
            if (sourceDerivedMinimum != null) sourceDerivedMinimum.Enabled = active && !musicBrainz;
            if (sourcePrimaryOnly != null)
                sourcePrimaryOnly.Enabled = active && !musicBrainz && SupportsPrimaryImageMetadata(currentSourceKey);
        }

        private static void SetRowsVisible(TableLayoutPanel table, IEnumerable<int> rows, bool visible)
        {
            if (table == null) return;
            HashSet<int> selected = new HashSet<int>(rows);
            foreach (Control control in table.Controls)
                if (selected.Contains(table.GetPositionFromControl(control).Row)) control.Visible = visible;
        }

        private void UpdateDerivedMinimumLabel()
        {
            if (sourceDerivedMinimum == null || sourceMinimumRange == null) return;
            if ("musicbrainz".Equals(currentSourceKey, StringComparison.OrdinalIgnoreCase))
            {
                sourceDerivedMinimum.Text = "Artwork resolution ranges do not apply to MusicBrainz metadata queries.";
                return;
            }
            SyncPreviewRange();
            string rangeType = Convert.ToString(sourceMinimumRange.SelectedItem);
            if (String.IsNullOrWhiteSpace(rangeType)) rangeType = "LowerRange";
            int derived = SourcePolicyRules.DerivedMinimum(state, rangeType);
            sourceDerivedMinimum.Text = rangeType == "BelowMinimum"
                ? "Derived threshold: no range minimum. Blank above means no additional constraint."
                : "Derived from " + rangeType + ": " + derived + " px. Optional fallback: "
                    + SourcePolicyRules.AdjacentFallbackRange(rangeType) + " only.";
        }

        private void UpdateSourcePolicyPreview()
        {
            if (sourceRangePreview == null || String.IsNullOrWhiteSpace(currentSourceKey)) return;
            SyncPreviewRange();
            SourcePolicyState policy = GetSourcePolicy(currentSourceKey);
            bool enabled;
            if (!sourceEnabledStates.TryGetValue(currentSourceKey, out enabled)) enabled = false;
            string sourceName = SourceDisplayName(currentSourceKey);
            bool musicBrainz = currentSourceKey.Equals("musicbrainz", StringComparison.OrdinalIgnoreCase);
            sourcePolicySummary.Text = sourceName + "  •  " + (enabled ? "Enabled" : "Disabled")
                + (musicBrainz
                    ? (policy.SourceOverride ? "  •  Custom runtime options" : "  •  Default runtime options")
                    : (policy.SourceOverride ? "  •  Custom minimum: " + policy.MinimumRangeType : "  •  Global policy"));

            sourceRangePreview.SuspendLayout();
            sourceRangePreview.Controls.Clear();
            sourceRangePreview.RowStyles.Clear();
            sourceRangePreview.RowCount = 1;
            if (musicBrainz)
            {
                AddPreviewHeader(0, "Runtime option", "Saved value", "Effective value");
                string[] names = { "retry_max", "min_delay", "recording_timeout" };
                string[] saved = {
                    musicBrainzOptions.RetryMax.ToString(),
                    musicBrainzOptions.MinDelay.ToString("0.##") + " s",
                    musicBrainzOptions.RecordingTimeout + " s"
                };
                string[] defaults = { "4", "1.05 s", "7 s" };
                for (int index = 0; index < names.Length; index++)
                {
                    int optionRow = index + 1;
                    sourceRangePreview.RowCount++;
                    sourceRangePreview.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
                    sourceRangePreview.Controls.Add(PreviewCell(names[index], FontStyle.Regular), 0, optionRow);
                    sourceRangePreview.Controls.Add(PreviewCell(saved[index], FontStyle.Regular), 1, optionRow);
                    Label effective = PreviewCell(policy.SourceOverride ? saved[index] : defaults[index], FontStyle.Bold);
                    effective.ForeColor = enabled ? PolicyColor("ACCEPT") : PolicyColor("INACTIVE");
                    sourceRangePreview.Controls.Add(effective, 2, optionRow);
                }
                sourceRangePreview.ResumeLayout();
                sourceConstraintSummary.Text = !enabled
                    ? "MusicBrainz metadata queries are disabled; authentication and saved options remain unchanged."
                    : policy.SourceOverride
                        ? "Custom options from credentials\\musicbrainz.json are active. Artwork range and image filters do not apply."
                        : "Built-in defaults are active; saved custom options remain preserved. Artwork range and image filters do not apply.";
                return;
            }
            AddPreviewHeader(0, "Range Type", "Short side", "Effective status");
            List<PolicyRangeSegment> segments = BuildPolicyRangeSegments(policy, enabled);
            int row = 1;
            foreach (PolicyRangeSegment segment in segments)
            {
                sourceRangePreview.RowCount++;
                sourceRangePreview.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
                Label name = PreviewCell(segment.Name, FontStyle.Regular);
                Label pixels = PreviewCell(segment.Display, FontStyle.Regular);
                Label status = PreviewCell(segment.Status, FontStyle.Bold);
                Color semantic = PolicyColor(segment.Status);
                status.ForeColor = semantic;
                sourceRangePreview.Controls.Add(name, 0, row);
                sourceRangePreview.Controls.Add(pixels, 1, row);
                sourceRangePreview.Controls.Add(status, 2, row);
                row++;
            }
            sourceRangePreview.ResumeLayout();

            List<string> constraints = new List<string>();
            if (!enabled) constraints.Add("Provider is disabled; saved policy remains available.");
            else if (!policy.SourceOverride) constraints.Add("Override is off. Saved source values are retained but inactive; global range behavior is unchanged.");
            else
            {
                string fallbackRange = SourcePolicyRules.AdjacentFallbackRange(policy.MinimumRangeType);
                if (policy.AllowBelowMinimumFallback && !String.IsNullOrWhiteSpace(fallbackRange))
                    constraints.Add("fallback: " + fallbackRange + " only");
                if (policy.MinimumShortSide.HasValue) constraints.Add("short side ≥ " + policy.MinimumShortSide.Value + " px");
                if (policy.MaximumShortSide.HasValue) constraints.Add("short side ≤ " + policy.MaximumShortSide.Value + " px");
                if (policy.MinimumWidth.HasValue) constraints.Add("width ≥ " + policy.MinimumWidth.Value + " px");
                if (policy.MinimumHeight.HasValue) constraints.Add("height ≥ " + policy.MinimumHeight.Value + " px");
                if (SupportsPrimaryImageMetadata(currentSourceKey)) constraints.Add(policy.PrimaryImageOnly ? "provider primary/front images only" : "all provider-described image types");
                else constraints.Add("primary-image metadata is not exposed by this provider");
                if (constraints.Count == 0) constraints.Add("No additional advanced constraints.");
            }
            sourceConstraintSummary.Text = String.Join("  •  ", constraints);
        }

        private List<PolicyRangeSegment> BuildPolicyRangeSegments(SourcePolicyState policy, bool enabled)
        {
            List<PolicyRangeBase> ranges = new List<PolicyRangeBase>
            {
                new PolicyRangeBase("BelowMinimum", 0, Math.Max(0, state.RangeMin - 1), "<" + state.RangeMin),
                new PolicyRangeBase("LowerRange", state.RangeMin, Math.Max(state.RangeMin, state.RangeIdeal - 1), state.RangeMin + "–" + (state.RangeIdeal - 1)),
                new PolicyRangeBase("Ideal", state.RangeIdeal, state.RangeIdeal, state.RangeIdeal.ToString()),
                new PolicyRangeBase("UpperRange", state.RangeIdeal + 1, state.RangeMax, (state.RangeIdeal + 1) + "–" + state.RangeMax),
                new PolicyRangeBase("Ladder", state.RangeMax + 1, state.RangeLadder, (state.RangeMax + 1) + "–" + state.RangeLadder),
                new PolicyRangeBase("AboveLadder", state.RangeLadder + 1, null, ">" + state.RangeLadder)
            };
            List<int> breaks = new List<int>();
            if (enabled && policy.SourceOverride)
            {
                if (policy.MinimumShortSide.HasValue) breaks.Add(policy.MinimumShortSide.Value);
                if (policy.MaximumShortSide.HasValue && policy.MaximumShortSide.Value < Int32.MaxValue) breaks.Add(policy.MaximumShortSide.Value + 1);
                if (policy.MinimumWidth.HasValue) breaks.Add(policy.MinimumWidth.Value);
                if (policy.MinimumHeight.HasValue) breaks.Add(policy.MinimumHeight.Value);
            }

            List<PolicyRangeSegment> result = new List<PolicyRangeSegment>();
            foreach (PolicyRangeBase range in ranges)
            {
                List<int> starts = new List<int> { range.Start };
                starts.AddRange(breaks.Where(value => value > range.Start && (!range.End.HasValue || value <= range.End.Value)));
                starts = starts.Distinct().OrderBy(value => value).ToList();
                for (int index = 0; index < starts.Count; index++)
                {
                    int start = starts[index];
                    int? end = index + 1 < starts.Count ? (int?)(starts[index + 1] - 1) : range.End;
                    SourcePolicyDecision decision = SourcePolicyRules.Evaluate(state, policy, start, start);
                    string status = enabled ? ResultLabel(decision.Result) : "INACTIVE";
                    string display = starts.Count == 1 ? range.Display : SegmentDisplay(start, end);
                    result.Add(new PolicyRangeSegment(range.Name, display, status));
                }
            }
            return result;
        }

        private void AddPreviewHeader(int row, string left, string middle, string right)
        {
            sourceRangePreview.RowStyles.Add(new RowStyle(SizeType.Absolute, 31));
            sourceRangePreview.Controls.Add(PreviewCell(left, FontStyle.Bold), 0, row);
            sourceRangePreview.Controls.Add(PreviewCell(middle, FontStyle.Bold), 1, row);
            sourceRangePreview.Controls.Add(PreviewCell(right, FontStyle.Bold), 2, row);
        }

        private static Label PreviewCell(string text, FontStyle style)
        {
            return new Label { Text = text, Dock = DockStyle.Fill, AutoSize = false, TextAlign = ContentAlignment.MiddleLeft, Padding = new Padding(5, 0, 2, 0), Font = ThemeManager.UiFont(ThemeFontRole.Minor, style) };
        }

        private Color PolicyColor(string status)
        {
            ThemePalette palette = ThemeManager.PaletteFor(uiState.Theme);
            if (status.StartsWith("ACCEPT", StringComparison.OrdinalIgnoreCase)) return palette.Success;
            if (status.StartsWith("FALLBACK", StringComparison.OrdinalIgnoreCase)) return palette.Warning;
            if (status.StartsWith("REJECT", StringComparison.OrdinalIgnoreCase)) return palette.Error;
            return palette.TextSecondary;
        }

        private static string ResultLabel(SourcePolicyResult result)
        {
            if (result == SourcePolicyResult.Accept) return "ACCEPT";
            if (result == SourcePolicyResult.Fallback) return "FALLBACK";
            return "REJECT";
        }

        private static string SegmentDisplay(int start, int? end)
        {
            if (!end.HasValue) return ">=" + start;
            if (start == end.Value) return start.ToString();
            if (start == 0) return "<" + (end.Value + 1);
            return start + "–" + end.Value;
        }

        private void SyncPreviewRange()
        {
            if (rangeMin == null) return;
            state.RangeMin = (int)rangeMin.Value;
            state.RangeIdeal = (int)rangeIdeal.Value;
            state.RangeMax = (int)rangeMax.Value;
            state.RangeLadder = (int)rangeLadder.Value;
        }

        private SourcePolicyState GetSourcePolicy(string source)
        {
            SourcePolicyState policy;
            if (!state.SourcePolicies.TryGetValue(source, out policy))
            {
                policy = new SourcePolicyState();
                state.SourcePolicies[source] = policy;
            }
            return policy;
        }

        private static bool TryReadDimension(TextBox box, string label, out int? value, out string error)
        {
            value = null;
            error = "";
            string text = box.Text.Trim();
            if (text.Length == 0) return true;
            int parsed;
            if (!Int32.TryParse(text, out parsed) || parsed <= 0 || parsed > 999999)
            {
                error = label + " must be blank or a whole number from 1 through 999999.";
                return false;
            }
            value = parsed;
            return true;
        }

        private static string OptionalDimensionText(int? value)
        {
            return value.HasValue ? value.Value.ToString() : "";
        }

        private static void DimensionKeyPress(object sender, KeyPressEventArgs e)
        {
            if (!Char.IsControl(e.KeyChar) && !Char.IsDigit(e.KeyChar)) e.Handled = true;
        }

        private static bool SupportsPrimaryImageMetadata(string source)
        {
            return "coverartarchive".Equals(source, StringComparison.OrdinalIgnoreCase);
        }

        private static string SourceDisplayName(string source)
        {
            switch ((source ?? "").ToLowerInvariant())
            {
                case "itunes": return "iTunes";
                case "discogs": return "Discogs";
                case "lastfm": return "Last.fm";
                case "fanarttv": return "Fanart.tv";
                case "coverartarchive": return "Cover Art Archive";
                case "deezer": return "Deezer";
                case "musicbrainz": return "MusicBrainz";
                default: return source;
            }
        }

        private static void SelectComboText(ComboBox combo, string value, string fallback)
        {
            for (int index = 0; index < combo.Items.Count; index++)
            {
                if (String.Equals(Convert.ToString(combo.Items[index]), value, StringComparison.OrdinalIgnoreCase))
                {
                    combo.SelectedIndex = index;
                    return;
                }
            }
            combo.SelectedItem = fallback;
        }

        private Control BuildRuntimeSettingsGroup()
        {
            GroupBox group = new FluentGroupBox { Name = "runtimeSettingsGroup", Text = "Runtime", Dock = DockStyle.Top, AutoSize = true, Padding = new Padding(10) };
            TableLayoutPanel outer = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 2, RowCount = 2 };
            outer.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            outer.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            outer.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            outer.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            group.Controls.Add(outer);
            Label info = new Label { Text = "Config v5 operational controls used by scanning and launch behavior.", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft };
            outer.Controls.Add(info, 0, 0);
            InfoButton help = new InfoButton("A zero timeout disables the purple waiting state. Read mode previews changes; Write mode installs validated artwork.");
            help.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            outer.Controls.Add(help, 1, 0);

            TableLayoutPanel left = RuntimeColumn();
            TableLayoutPanel right = RuntimeColumn();
            left.Name = "runtimeLeftColumn";
            right.Name = "runtimeRightColumn";
            outer.Controls.Add(left, 0, 1);
            outer.Controls.Add(right, 1, 1);

            mode = RuntimeCombo(new[] { "read", "write" });
            AddRuntimeSetting(left, 0, "Mode", mode);
            scanMode = new FluentCheckBox { Text = "Enabled", Dock = DockStyle.Fill, AutoSize = true };
            AddRuntimeSetting(left, 1, "Scan mode", scanMode);
            timeout = new FluentNumericUpDown { Minimum = 0, Maximum = 8760, DecimalPlaces = 1, Increment = 0.5M, Dock = DockStyle.Fill };
            AddRuntimeSetting(left, 2, "Timeout (hours)", timeout);
            sampleWrite = new FluentCheckBox { Text = "Enabled", Dock = DockStyle.Fill, AutoSize = true };
            AddRuntimeSetting(left, 3, "Write review samples", sampleWrite);

            verbosity = RuntimeCombo(new[] { "error", "warn", "info", "debug", "trace" });
            AddRuntimeSetting(right, 0, "Verbosity", verbosity);
            libraryScan = new FluentCheckBox { Text = "Enabled", Dock = DockStyle.Fill, AutoSize = true };
            AddRuntimeSetting(right, 1, "Full library scan", libraryScan);
            showConfirmations = new FluentCheckBox { Text = "Enabled", Dock = DockStyle.Fill, AutoSize = true };
            showConfirmations.AccessibleDescription = "When cleared, repetitive artwork-choice confirmations are skipped. Bypass and history safety warnings remain enabled.";
            AddRuntimeSetting(right, 2, "Show confirmations", showConfirmations);

            return group;
        }

        private static TableLayoutPanel RuntimeColumn()
        {
            TableLayoutPanel table = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 2, RowCount = 4, Margin = new Padding(0, 0, 12, 0) };
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 155));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int row = 0; row < 4; row++) table.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            return table;
        }

        private static ComboBox RuntimeCombo(string[] values)
        {
            ComboBox combo = new FluentComboBox { Dock = DockStyle.Fill, Margin = new Padding(3, 6, 3, 3) };
            combo.Items.AddRange(values);
            return combo;
        }

        private static void AddRuntimeSetting(TableLayoutPanel table, int row, string label, Control control)
        {
            table.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft, AutoEllipsis = true }, 0, row);
            control.Margin = control is ComboBox ? new Padding(3, 6, 3, 3) : new Padding(3, 8, 3, 3);
            table.Controls.Add(control, 1, row);
        }

        private Control BuildRetentionPair()
        {
            TableLayoutPanel pair = new TableLayoutPanel
            {
                Name = "retentionPair",
                Dock = DockStyle.Top,
                AutoSize = true,
                ColumnCount = 2,
                RowCount = 1,
                Margin = new Padding(0, 7, 0, 7)
            };
            pair.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            pair.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            Control retention = BuildLogRetentionSettingsGroup();
            Control history = BuildRetentionSettingsGroup();
            retention.Margin = new Padding(0, 0, 5, 0);
            history.Margin = new Padding(5, 0, 0, 0);
            pair.Controls.Add(retention, 0, 0);
            pair.Controls.Add(history, 1, 0);
            return pair;
        }

        private Control BuildLogRetentionSettingsGroup()
        {
            GroupBox group = new FluentGroupBox { Name = "retentionGroup", Text = "Retention", Dock = DockStyle.Fill, AutoSize = true, Padding = new Padding(10) };
            TableLayoutPanel table = new TableLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, ColumnCount = 3, RowCount = 2 };
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 150));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 34));
            table.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            table.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            table.Controls.Add(new Label
            {
                Text = "Disposable diagnostic logs may be pruned without changing album status.",
                Dock = DockStyle.Fill,
                AutoEllipsis = true
            }, 0, 0);
            table.SetColumnSpan(table.GetControlFromPosition(0, 0), 3);
            table.Controls.Add(new Label { Text = "Retain logs for (days)", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 1);
            logDays = new FluentNumericUpDown { Minimum = 1, Maximum = 3650, Dock = DockStyle.Fill };
            table.Controls.Add(logDays, 1, 1);
            table.Controls.Add(new InfoButton("Recommended: 14 days. Logs are troubleshooting output and may be pruned safely."), 2, 1);
            group.Controls.Add(table);
            return group;
        }

        private Control BuildRetentionSettingsGroup()
        {
            GroupBox group = new FluentGroupBox { Name = "retentionSettingsGroup", Text = "Retention and Status History", Dock = DockStyle.Fill, AutoSize = true, Padding = new Padding(10) };
            TableLayoutPanel table = new TableLayoutPanel();
            table.Dock = DockStyle.Fill;
            table.AutoSize = true;
            table.ColumnCount = 3;
            table.RowCount = 2;
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 150));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            table.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 34));
            table.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            table.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            group.Controls.Add(table);
            Label info = new Label { Text = "History supplies authoritative processed, timeout, and bypass states.", Dock = DockStyle.Fill, AutoEllipsis = true };
            table.Controls.Add(info, 0, 0); table.SetColumnSpan(info, 3);
            table.Controls.Add(new Label { Text = "Retain history for", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 1);
            historyRetention = new FluentComboBox { Dock = DockStyle.Fill };
            historyRetention.Items.AddRange(new object[] { "Off", "Forever", "30 days", "90 days", "365 days" });
            table.Controls.Add(historyRetention, 1, 1);
            table.Controls.Add(new InfoButton("Turning history off or shortening retention can remove stored bypass/timeout/processed authority. Existing cover artwork is still reconciled from the album folder."), 2, 1);
            return group;
        }

        private Control BuildAdvancedCommandsGroup()
        {
            GroupBox group = new FluentGroupBox { Text = "Tools", Dock = DockStyle.Top, AutoSize = true, Padding = new Padding(10) };
            TableLayoutPanel layout = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 1, RowCount = 2 };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 40));
            layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            layout.Controls.Add(new Label
            {
                Text = "Validate Config v5 or open the configured diagnostic locations.",
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.MiddleLeft
            }, 0, 0);
            FlowLayoutPanel buttons = new FlowLayoutPanel { Dock = DockStyle.Top, AutoSize = true, WrapContents = true, Padding = new Padding(0, 4, 0, 4) };

            Button validate = ActionButton("Validate Saved Config", 175);
            validate.Click += delegate
            {
                try
                {
                    ConfigState saved = ConfigStore.Load();
                    ConfigStore.Validate(saved);
                    MessageBox.Show(this, "The saved Config v5 is valid.", "SPLINED config check", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
                catch (Exception error)
                {
                    MessageBox.Show(this, error.Message, "Config v5 validation failed", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }
            };

            Button configFolder = ActionButton("Open Config Folder", 165);
            configFolder.Click += delegate { OpenContainingFolder(configPath.Text); };
            Button logsFolder = ActionButton("Open Logs Folder", 155);
            logsFolder.Click += delegate { OpenContainingFolder(logDir.Text); };

            buttons.Controls.Add(validate);
            buttons.Controls.Add(configFolder);
            buttons.Controls.Add(logsFolder);
            buttons.Controls.Add(new InfoButton("Config validation uses the same Config v5 rules as Save and Continue. Public command documentation opens from Help."));
            layout.Controls.Add(buttons, 0, 1);
            group.Controls.Add(layout);
            return group;
        }

        private void OpenContainingFolder(string configuredPath)
        {
            try
            {
                string path = (configuredPath ?? "").Trim();
                string folder = Directory.Exists(path) ? path : Path.GetDirectoryName(path);
                if (String.IsNullOrWhiteSpace(folder) || !Directory.Exists(folder))
                    throw new InvalidOperationException("The configured folder does not exist yet: " + path);
                Process.Start(new ProcessStartInfo { FileName = "explorer.exe", Arguments = "\"" + folder + "\"", UseShellExecute = true });
            }
            catch (Exception error)
            {
                MessageBox.Show(this, error.Message, "Unable to open folder", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private Button CreateToggle(string label, string key, Dictionary<string, Button> collection)
        {
            Button button = ActionButton(label, 310);
            button.Height = 30;
            button.Margin = new Padding(2);
            button.Tag = new ToggleTag(label, key, false);
            button.Click += delegate
            {
                ToggleTag tag = (ToggleTag)button.Tag;
                if (collection == formatButtons && tag.Enabled && EnabledCount(collection) <= 1)
                {
                    MessageBox.Show(this, "At least one output format must remain enabled.", "Output Formats", MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }
                tag.Enabled = !tag.Enabled;
                UpdateToggle(button);
            };
            collection[key] = button;
            return button;
        }

        private static int EnabledCount(Dictionary<string, Button> buttons)
        {
            return buttons.Values.Count(button => ((ToggleTag)button.Tag).Enabled);
        }

        private static void SetToggle(Dictionary<string, Button> buttons, string key, bool enabled)
        {
            ToggleTag tag = (ToggleTag)buttons[key].Tag;
            tag.Enabled = enabled;
            UpdateToggle(buttons[key]);
        }

        private static void UpdateToggle(Button button)
        {
            ToggleTag tag = (ToggleTag)button.Tag;
            button.Text = tag.Label + "   [" + (tag.Enabled ? "ENABLED" : "DISABLED") + "]";
            ThemePalette palette = ThemeManager.CurrentPalette;
            ThemeManager.StyleButton(button, ThemeManager.CurrentTheme);
            button.FlatAppearance.BorderSize = 0;
            if (tag.Enabled)
            {
                button.BackColor = palette.ButtonBackground;
                button.ForeColor = palette.Success;
                button.FlatAppearance.BorderColor = palette.Success;
                button.FlatAppearance.MouseOverBackColor = palette.ButtonHover;
                button.FlatAppearance.MouseDownBackColor = palette.ButtonPressed;
            }
            else
            {
                button.BackColor = palette.ButtonBackground;
                button.ForeColor = palette.Error;
                button.FlatAppearance.BorderColor = palette.BorderSubtle;
                button.FlatAppearance.MouseOverBackColor = palette.ButtonHover;
                button.FlatAppearance.MouseDownBackColor = palette.ButtonPressed;
            }
            ThemeManager.ApplyRoundedRegion(button, ThemeManager.ControlRadius);
            button.Invalidate();
        }

        private void RefreshToggleVisuals()
        {
            foreach (Dictionary<string, Button> collection in new[] { optionButtons, formatButtons })
                if (collection != null)
                    foreach (Button button in collection.Values) UpdateToggle(button);
            UpdateSourcePolicyPreview();
        }

        private void Populate()
        {
            music.Text = state.MusicLibrary;
            ignored.Text = String.Join(", ", state.IgnoredSubs);
            configPath.Text = state.ConfigPath;
            credentialDir.Text = state.CredentialDir;
            cacheDir.Text = state.CacheDir;
            logDir.Text = state.LogDir;
            historyDir.Text = state.HistoryDir;
            scanDir.Text = state.ScanLibraryDir;
            fileName.Text = state.FileName;
            rangeMin.Value = Clamp(state.RangeMin, rangeMin.Minimum, rangeMin.Maximum);
            rangeIdeal.Value = Clamp(state.RangeIdeal, rangeIdeal.Minimum, rangeIdeal.Maximum);
            rangeMax.Value = Clamp(state.RangeMax, rangeMax.Minimum, rangeMax.Maximum);
            rangeLadder.Value = Clamp(state.RangeLadder, rangeLadder.Minimum, rangeLadder.Maximum);
            squareRound.Value = Clamp(state.SquareRoundTo, squareRound.Minimum, squareRound.Maximum);
            existingArtworkAction.SelectedIndex = state.PreserveFile ? 1 : 0;
            SetToggle(optionButtons, "square", state.Square);
            SetToggle(optionButtons, "crop", state.SquareMode.Equals("crop", StringComparison.OrdinalIgnoreCase));
            SetToggle(optionButtons, "upscale", state.UpscaleBelowIdeal);
            SetToggle(optionButtons, "evaluate", state.EvaluateFinalImage);
            sourceEnabledStates.Clear();
            foreach (string source in ConfigState.KnownSources)
            {
                SourcePolicyState policy = GetSourcePolicy(source);
                sourceEnabledStates[source] = policy.Enabled;
            }
            foreach (string format in formatButtons.Keys.ToList())
                SetToggle(formatButtons, format, state.Formats.Contains(format, StringComparer.OrdinalIgnoreCase));
            sourcePriority.Items.Clear();
            IEnumerable<string> orderedSources = state.Sources
                .Where(source => ConfigState.ArtworkSources.Contains(source, StringComparer.OrdinalIgnoreCase))
                .Concat(ConfigState.ArtworkSources.Where(source => !state.Sources.Contains(source, StringComparer.OrdinalIgnoreCase)))
                .Distinct(StringComparer.OrdinalIgnoreCase);
            foreach (string source in orderedSources)
                sourcePriority.Items.Add(new SourceChoice(source, SourceDisplayName(source)));
            if (sourcePriority.Items.Count > 0) sourcePriority.SelectedIndex = 0;
            UpdateSourcePriorityButtons();
            mode.SelectedItem = state.Mode;
            if (mode.SelectedIndex < 0) mode.SelectedIndex = 0;
            verbosity.SelectedItem = state.Verbosity;
            if (verbosity.SelectedIndex < 0) verbosity.SelectedItem = "info";
            scanMode.Checked = state.ScanMode;
            libraryScan.Checked = state.LibraryScan;
            showConfirmations.Checked = uiState.ShowConfirmations;
            timeout.Value = Clamp((decimal)state.ScanModeTimeout, timeout.Minimum, timeout.Maximum);
            sampleWrite.Checked = state.SampleWrite;
            logDays.Value = Clamp(state.LogRetentionDays, logDays.Minimum, logDays.Maximum);
            if (!state.HistoryEnabled) historyRetention.SelectedIndex = 0;
            else if (state.HistoryRetentionDays == 30) historyRetention.SelectedIndex = 2;
            else if (state.HistoryRetentionDays == 90) historyRetention.SelectedIndex = 3;
            else if (state.HistoryRetentionDays == 365) historyRetention.SelectedIndex = 4;
            else historyRetention.SelectedIndex = 1;
            Label active = FindControl<Label>(primaryTabs, "activeConfigLabel");
            if (active != null) active.Text = "Active configuration: " + state.ConfigPath;
            loadingSourceEditor = true;
            int discogsIndex = -1;
            for (int index = 0; index < sourceSelector.Items.Count; index++)
            {
                SourceChoice choice = sourceSelector.Items[index] as SourceChoice;
                if (choice != null && choice.Key == "discogs") discogsIndex = index;
            }
            sourceSelector.SelectedIndex = discogsIndex >= 0 ? discogsIndex : 0;
            SourceChoice selectedSource = sourceSelector.SelectedItem as SourceChoice;
            currentSourceKey = selectedSource == null ? null : selectedSource.Key;
            loadingSourceEditor = false;
            LoadCurrentSourceEditor();
            if (firstRun) primaryTabs.SelectedIndex = 0;
        }

        private void SaveClicked(object sender, EventArgs e)
        {
            bool newHistoryEnabled = historyRetention.SelectedIndex != 0;
            int newHistoryDays = historyRetention.SelectedIndex == 2 ? 30 : historyRetention.SelectedIndex == 3 ? 90 : historyRetention.SelectedIndex == 4 ? 365 : 0;
            bool historyReduced = (state.HistoryEnabled && !newHistoryEnabled)
                || (newHistoryEnabled && newHistoryDays > 0 && (state.HistoryRetentionDays == 0 || newHistoryDays < state.HistoryRetentionDays));
            if (historyReduced)
            {
                DialogResult choice = MessageBox.Show(
                    this,
                    "Turning history off or shortening retention can make authoritative status history unavailable. Affected red, purple, orange, and green folders will return to standard colors.\r\n\r\nSave this change?",
                    "History coloring will change",
                    MessageBoxButtons.YesNo,
                    MessageBoxIcon.Warning,
                    MessageBoxDefaultButton.Button2);
                if (choice != DialogResult.Yes) return;
            }

            try
            {
                SaveCurrentSourceEditor(true);
                state.ConfigPath = configPath.Text.Trim();
                state.MusicLibrary = music.Text.Trim();
                state.IgnoredSubs = ignored.Text.Split(',').Select(value => value.Trim()).Where(value => value.Length > 0).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
                state.CredentialDir = credentialDir.Text.Trim();
                state.CacheDir = cacheDir.Text.Trim();
                state.LogDir = logDir.Text.Trim();
                state.HistoryDir = historyDir.Text.Trim();
                state.ScanLibraryDir = scanDir.Text.Trim();
                state.FileName = fileName.Text.Trim();
                state.RangeMin = (int)rangeMin.Value;
                state.RangeIdeal = (int)rangeIdeal.Value;
                state.RangeMax = (int)rangeMax.Value;
                state.RangeLadder = (int)rangeLadder.Value;
                state.SquareRoundTo = (int)squareRound.Value;
                state.PreserveFile = existingArtworkAction.SelectedIndex == 1;
                state.Square = ToggleEnabled(optionButtons, "square");
                state.SquareMode = ToggleEnabled(optionButtons, "crop") ? "crop" : "off";
                state.UpscaleBelowIdeal = ToggleEnabled(optionButtons, "upscale");
                state.EvaluateFinalImage = ToggleEnabled(optionButtons, "evaluate");
                state.Formats = formatButtons.Where(pair => ((ToggleTag)pair.Value.Tag).Enabled).Select(pair => pair.Key).ToList();
                state.Sources = sourcePriority.Items.Cast<SourceChoice>().Select(choice => choice.Key).ToList();
                foreach (string source in ConfigState.ArtworkSources)
                    if (!state.Sources.Contains(source, StringComparer.OrdinalIgnoreCase)) state.Sources.Add(source);
                state.ExcludedSources = ConfigState.ArtworkSources.Where(source => !sourceEnabledStates.ContainsKey(source) || !sourceEnabledStates[source]).ToList();
                state.Mode = Convert.ToString(mode.SelectedItem);
                state.Verbosity = Convert.ToString(verbosity.SelectedItem);
                state.ScanMode = scanMode.Checked;
                state.LibraryScan = libraryScan.Checked;
                state.ScanModeTimeout = (double)timeout.Value;
                state.SampleWrite = sampleWrite.Checked;
                state.LogRetentionDays = (int)logDays.Value;
                state.HistoryEnabled = newHistoryEnabled;
                state.HistoryRetentionDays = newHistoryDays;
                ConfigStore.Validate(state);
                string musicBrainzCredential = CredentialStore.PathFor(state, "musicbrainz");
                SourcePolicyState musicBrainzPolicy = GetSourcePolicy("musicbrainz");
                bool customMusicBrainzOptions = musicBrainzOptions.RetryMax != 4
                    || Math.Abs(musicBrainzOptions.MinDelay - 1.05) > 0.0001
                    || musicBrainzOptions.RecordingTimeout != 7;
                if (File.Exists(musicBrainzCredential))
                    CredentialStore.MergeMusicBrainzOptions(state, musicBrainzOptions);
                else if (musicBrainzPolicy.SourceOverride && customMusicBrainzOptions)
                    throw new InvalidOperationException("Configure MusicBrainz credentials before saving custom MusicBrainz runtime options.");
                ConfigStore.Save(state);
                uiState.ShowConfirmations = showConfirmations.Checked;
                ConfigStore.SaveUi(uiState);
                SavedState = state.Clone();
                DialogResult = DialogResult.OK;
                Close();
            }
            catch (Exception error)
            {
                MessageBox.Show(this, error.Message, "Unable to save Config v5", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private void UseExistingConfiguration(object sender, EventArgs e)
        {
            using (OpenFileDialog dialog = new OpenFileDialog())
            {
                dialog.Filter = "TOML configuration (*.toml)|*.toml|All files (*.*)|*.*";
                dialog.CheckFileExists = true;
                if (dialog.ShowDialog(this) != DialogResult.OK) return;
                try
                {
                    ConfigStore.WriteTextAtomic(ConfigStore.LocatorPath, ConfigStore.ToPortablePath(dialog.FileName) + Environment.NewLine);
                    state = ConfigStore.Load();
                    Populate();
                }
                catch (Exception error)
                {
                    MessageBox.Show(this, error.Message, "Unable to use configuration", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }
            }
        }

        private void RestorePortablePaths()
        {
            ConfigState defaults = ConfigStore.Defaults();
            configPath.Text = defaults.ConfigPath;
            credentialDir.Text = defaults.CredentialDir;
            cacheDir.Text = defaults.CacheDir;
            logDir.Text = defaults.LogDir;
            historyDir.Text = defaults.HistoryDir;
            scanDir.Text = "";
        }

        private static bool ToggleEnabled(Dictionary<string, Button> buttons, string key)
        {
            return ((ToggleTag)buttons[key].Tag).Enabled;
        }

        private static decimal Clamp(decimal value, decimal min, decimal max)
        {
            return Math.Min(max, Math.Max(min, value));
        }

        private static string ExistingDirectory(string path)
        {
            if (Directory.Exists(path)) return path;
            string parent = Path.GetDirectoryName(path);
            return Directory.Exists(parent) ? parent : ConfigStore.AppRoot;
        }

        private static Button ActionButton(string text, int width)
        {
            return new FluentButton { Text = text, Width = width, Height = 32, Margin = new Padding(4) };
        }

        private static T FindControl<T>(Control parent, string name) where T : Control
        {
            foreach (Control control in parent.Controls)
            {
                T match = control as T;
                if (match != null && match.Name == name) return match;
                T nested = FindControl<T>(control, name);
                if (nested != null) return nested;
            }
            return null;
        }

        private sealed class ToggleTag
        {
            public string Label;
            public string Key;
            public bool Enabled;
            public ToggleTag(string label, string key, bool enabled) { Label = label; Key = key; Enabled = enabled; }
        }

        private sealed class SourceChoice
        {
            public readonly string Key;
            private readonly string label;
            public SourceChoice(string key, string label) { Key = key; this.label = label; }
            public override string ToString() { return label; }
        }

        private sealed class PolicyRangeBase
        {
            public readonly string Name;
            public readonly int Start;
            public readonly int? End;
            public readonly string Display;
            public PolicyRangeBase(string name, int start, int? end, string display)
            {
                Name = name; Start = start; End = end; Display = display;
            }
        }

        private sealed class PolicyRangeSegment
        {
            public readonly string Name;
            public readonly string Display;
            public readonly string Status;
            public PolicyRangeSegment(string name, string display, string status)
            {
                Name = name; Display = display; Status = status;
            }
        }
    }
}
