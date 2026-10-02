using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal sealed class MusicBrainzMatchesForm : FluentForm
    {
        private readonly ListView matches;
        private readonly Button use;
        public int SelectedMatchIndex { get; private set; }
        public bool SearchRequested { get; private set; }
        public bool LeaveUnchanged { get; private set; }
        public bool AuthorityEditRequested { get; private set; }
        public string EditedRecordingMbid { get; private set; }
        public string EditedArtistMbids { get; private set; }
        public string EditedReleaseMbid { get; private set; }

        public MusicBrainzMatchesForm(string artist, string title, object[] rawItems, bool compilationTrack, string theme)
        {
            Text = "MusicBrainz Matches — " + artist + " — " + title;
            StartPosition = FormStartPosition.CenterParent;
            Size = new Size(1120, 700);
            MinimumSize = new Size(820, 500);
            ThemeManager.PrepareForm(this);
            TableLayoutPanel layout = new FluentCardTableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(12), RowCount = 4, ColumnCount = 1, VisualRole = CardVisualRole.Panel };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 112));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 52));
            Controls.Add(layout);
            layout.Controls.Add(new Label { Dock = DockStyle.Fill, AutoEllipsis = true,
                Text = "Choose the authoritative release for this track. Results are grouped by decade and release type. Green is the most recently inspected release; blue releases were inspected earlier. Preview opens the exact MusicBrainz URL." }, 0, 0);

            Dictionary<string, object> first = rawItems != null && rawItems.Length > 0
                ? rawItems[0] as Dictionary<string, object>
                : null;
            TableLayoutPanel authority = new FluentCardTableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(8, 4, 8, 4),
                RowCount = 3,
                ColumnCount = 3,
                VisualRole = CardVisualRole.Nested
            };
            authority.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 165));
            authority.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            authority.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 145));
            FluentTextBox artistId = AuthorityRow(authority, 0, "MusicBrainz Artist ID(s)", StringListValue(first, "artist_mbids"));
            FluentTextBox releaseId = AuthorityRow(authority, 1, "MusicBrainz Release ID", TextValue(first, "release_mbid", ""));
            FluentTextBox recordingId = AuthorityRow(authority, 2, "MusicBrainz Recording ID", TextValue(first, "recording_mbid", ""));
            Button applyAuthority = new FluentButton { Text = "Apply IDs", Dock = DockStyle.Fill, Margin = new Padding(6, 3, 0, 3), Tag = "primary" };
            applyAuthority.Click += delegate
            {
                if (!ValidSingleMbid(recordingId.Text) || !ValidSingleMbid(releaseId.Text) || !ValidMbidList(artistId.Text))
                {
                    MessageBox.Show(this, "Recording and Release accept one MusicBrainz UUID. Artist accepts one or more UUIDs separated by commas.", "MusicBrainz authority", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    return;
                }
                AuthorityEditRequested = true;
                EditedRecordingMbid = recordingId.Text.Trim();
                EditedArtistMbids = artistId.Text.Trim();
                EditedReleaseMbid = releaseId.Text.Trim();
                DialogResult = DialogResult.Yes;
                Close();
            };
            authority.Controls.Add(applyAuthority, 2, 0);
            authority.SetRowSpan(applyAuthority, 3);
            layout.Controls.Add(authority, 0, 1);

            matches = new ListView { Dock = DockStyle.Fill, View = View.Details, FullRowSelect = true, HideSelection = false, MultiSelect = false, ShowGroups = true };
            matches.Columns.Add("[#]", 52); matches.Columns.Add("Artist", 190); matches.Columns.Add("Country", 74);
            matches.Columns.Add("Date", 100); matches.Columns.Add("Release Type", 110); matches.Columns.Add("Release", 350);
            matches.Columns.Add("Resolution", 100); matches.Columns.Add("URL", 90);
            Populate(rawItems ?? new object[0]);
            matches.SelectedIndexChanged += delegate { use.Enabled = matches.SelectedItems.Count == 1; };
            matches.DoubleClick += delegate { UseSelected(); };
            layout.Controls.Add(matches, 0, 2);

            FlowLayoutPanel actions = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false, Padding = new Padding(0, 8, 0, 0) };
            use = new FluentButton { Text = "Use Release", Width = 130, Height = 34, Enabled = false, Tag = "success" };
            use.Click += delegate { UseSelected(); };
            Button preview = new FluentButton { Text = "Preview URL", Width = 125, Height = 34 };
            preview.Click += delegate { PreviewSelected(); };
            Button search = new FluentButton { Text = "Search Artist / Track", Width = 175, Height = 34, Tag = "primary" };
            search.Click += delegate { SearchRequested = true; DialogResult = DialogResult.Retry; Close(); };
            Button unchanged = new FluentButton
            {
                Text = compilationTrack ? "Leave Track Unchanged" : "Return to Source Results",
                Width = compilationTrack ? 175 : 185,
                Height = 34
            };
            unchanged.Click += delegate { LeaveUnchanged = true; DialogResult = DialogResult.Ignore; Close(); };
            actions.Controls.Add(use); actions.Controls.Add(preview); actions.Controls.Add(search); actions.Controls.Add(unchanged);
            actions.Controls.Add(new InfoButton("Search Artist / Track performs the bounded MusicBrainz Recording search without using the curated compilation Album name."));
            layout.Controls.Add(actions, 0, 3);
            ThemeManager.Apply(this, theme);
            ThemeManager.PrepareForFirstShow(this, theme);
        }

        private void Populate(object[] rawItems)
        {
            Dictionary<string, ListViewGroup> groups = new Dictionary<string, ListViewGroup>(StringComparer.OrdinalIgnoreCase);
            foreach (object raw in rawItems)
            {
                Dictionary<string, object> item = raw as Dictionary<string, object>;
                if (item == null) continue;
                string decade = TextValue(item, "decade", "Unknown");
                string releaseType = TextValue(item, "release_class", "album").ToUpperInvariant();
                string groupKey = decade + "|" + releaseType;
                ListViewGroup group;
                if (!groups.TryGetValue(groupKey, out group))
                {
                    group = new ListViewGroup("[" + decade + "]  RELEASE TYPE [" + releaseType + "]", HorizontalAlignment.Left);
                    groups[groupKey] = group; matches.Groups.Add(group);
                }
                int index = IntValue(item, "index", matches.Items.Count + 1);
                ListViewItem row = new ListViewItem(index.ToString(), group);
                row.SubItems.Add(TextValue(item, "release_artist", TextValue(item, "recording_artist", "")));
                row.SubItems.Add(TextValue(item, "country", "")); row.SubItems.Add(TextValue(item, "release_date", ""));
                row.SubItems.Add(releaseType); row.SubItems.Add(TextValue(item, "release_title", ""));
                row.SubItems.Add(TextValue(item, "resolution", "—")); row.SubItems.Add("[URL]"); row.Tag = item;
                if (BoolValue(item, "current")) row.ForeColor = ThemeManager.CurrentPalette.Success;
                else if (BoolValue(item, "visited")) row.ForeColor = ThemeManager.CurrentPalette.StatusBlue;
                matches.Items.Add(row);
            }
        }

        private void UseSelected()
        {
            if (matches.SelectedItems.Count != 1) return;
            Dictionary<string, object> item = matches.SelectedItems[0].Tag as Dictionary<string, object>;
            if (item == null) return;
            SelectedMatchIndex = IntValue(item, "index", 0);
            if (SelectedMatchIndex <= 0) return;
            DialogResult = DialogResult.OK; Close();
        }

        private void PreviewSelected()
        {
            if (matches.SelectedItems.Count != 1) return;
            Dictionary<string, object> item = matches.SelectedItems[0].Tag as Dictionary<string, object>;
            string url = item == null ? "" : TextValue(item, "url", "");
            if (String.IsNullOrWhiteSpace(url)) return;
            try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
            catch (Exception error) { MessageBox.Show(this, "Unable to open MusicBrainz URL.\r\n\r\n" + error.Message, "MusicBrainz preview", MessageBoxButtons.OK, MessageBoxIcon.Warning); }
        }

        private static FluentTextBox AuthorityRow(TableLayoutPanel panel, int row, string label, string value)
        {
            panel.RowStyles.Add(new RowStyle(SizeType.Percent, 33.333f));
            panel.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, row);
            FluentTextBox text = new FluentTextBox { Text = value ?? "", Dock = DockStyle.Fill, Margin = new Padding(3, 2, 3, 2) };
            panel.Controls.Add(text, 1, row);
            return text;
        }

        private static bool ValidSingleMbid(string value)
        {
            if (String.IsNullOrWhiteSpace(value)) return true;
            Guid parsed;
            return Guid.TryParseExact(value.Trim(), "D", out parsed);
        }

        private static bool ValidMbidList(string value)
        {
            if (String.IsNullOrWhiteSpace(value)) return true;
            return value.Split(new[] { ',', ';', ' ' }, StringSplitOptions.RemoveEmptyEntries).All(ValidSingleMbid);
        }

        private static string StringListValue(Dictionary<string, object> item, string key)
        {
            if (item == null || !item.ContainsKey(key) || item[key] == null) return "";
            object[] values = item[key] as object[];
            if (values == null) return Convert.ToString(item[key]);
            return String.Join(", ", values.Select(Convert.ToString));
        }

        private static string TextValue(Dictionary<string, object> item, string key, string fallback) { object value; return item.TryGetValue(key, out value) && value != null ? Convert.ToString(value) : fallback; }
        private static int IntValue(Dictionary<string, object> item, string key, int fallback) { object value; int parsed; return item.TryGetValue(key, out value) && Int32.TryParse(Convert.ToString(value), out parsed) ? parsed : fallback; }
        private static bool BoolValue(Dictionary<string, object> item, string key) { object value; bool parsed; return item.TryGetValue(key, out value) && Boolean.TryParse(Convert.ToString(value), out parsed) && parsed; }
    }
}
