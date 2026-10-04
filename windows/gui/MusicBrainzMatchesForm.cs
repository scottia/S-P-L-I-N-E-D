using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal sealed class MusicBrainzMatchEventArgs : EventArgs
    {
        public Dictionary<string, object> Item { get; private set; }
        public int Index { get; private set; }

        public MusicBrainzMatchEventArgs(Dictionary<string, object> item, int index)
        {
            Item = item;
            Index = index;
        }
    }

    internal sealed class MusicBrainzAuthorityEventArgs : EventArgs
    {
        public string RecordingMbid { get; private set; }
        public string ArtistMbids { get; private set; }
        public string ReleaseMbid { get; private set; }

        public MusicBrainzAuthorityEventArgs(string recordingMbid, string artistMbids, string releaseMbid)
        {
            RecordingMbid = recordingMbid ?? "";
            ArtistMbids = artistMbids ?? "";
            ReleaseMbid = releaseMbid ?? "";
        }
    }

    /// <summary>
    /// MusicBrainz decision surface embedded in the Scan Activity workspace so
    /// the shared Artwork panel remains visible throughout authority review.
    /// </summary>
    internal sealed class MusicBrainzMatchesPanel : UserControl
    {
        private readonly ListView matches;
        private readonly Button use;
        private readonly FluentTextBox artistId;
        private readonly FluentTextBox releaseId;
        private readonly FluentTextBox recordingId;
        private int hoveredMatchIndex = -1;

        public event EventHandler<MusicBrainzMatchEventArgs> UseRequested;
        public event EventHandler<MusicBrainzMatchEventArgs> ArtworkPreviewRequested;
        public event EventHandler ArtworkPreviewEnded;
        public event EventHandler SearchRequested;
        public event EventHandler LeaveRequested;
        public event EventHandler<MusicBrainzAuthorityEventArgs> AuthorityEditRequested;

        public MusicBrainzMatchesPanel(string artist, string title, string albumArtist, string albumTitle, object[] rawItems, bool compilationTrack, string theme)
        {
            Name = "musicBrainzMatchesPanel";
            Dock = DockStyle.Fill;
            Margin = Padding.Empty;
            TableLayoutPanel layout = new FluentCardTableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(ThemeManager.Space8),
                RowCount = 5,
                ColumnCount = 1,
                VisualRole = CardVisualRole.Nested
            };
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 92));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            Controls.Add(layout);
            layout.Controls.Add(new Label
            {
                Dock = DockStyle.Fill,
                AutoEllipsis = true,
                Text = "Choose the authoritative release for " + artist + " — " + title
                    + ". Hover an artwork [URL] to preview it at right; green is current and blue was inspected earlier."
            }, 0, 0);

            Dictionary<string, object> first = rawItems != null && rawItems.Length > 0
                ? rawItems[0] as Dictionary<string, object>
                : null;
            TableLayoutPanel authority = new FluentCardTableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(6, 2, 6, 2),
                RowCount = 3,
                ColumnCount = 3,
                VisualRole = CardVisualRole.Nested
            };
            authority.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 165));
            authority.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            authority.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 112));
            artistId = AuthorityRow(authority, 0, "MusicBrainz Artist ID(s)", StringListValue(first, "artist_mbids"));
            releaseId = AuthorityRow(authority, 1, "MusicBrainz Release ID", TextValue(first, "release_mbid", ""));
            recordingId = AuthorityRow(authority, 2, "MusicBrainz Recording ID", TextValue(first, "recording_mbid", ""));
            Button applyAuthority = new FluentButton { Text = "Apply IDs", Dock = DockStyle.Fill, Margin = new Padding(6, 3, 0, 3), Tag = "primary" };
            applyAuthority.Click += delegate { RequestAuthorityEdit(); };
            authority.Controls.Add(applyAuthority, 2, 0);
            authority.SetRowSpan(applyAuthority, 3);
            layout.Controls.Add(authority, 0, 1);

            Label currentAlbum = new Label
            {
                Name = "musicBrainzCurrentAlbum",
                Dock = DockStyle.Fill,
                AutoEllipsis = true,
                TextAlign = ContentAlignment.MiddleLeft,
                Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                Text = "CURRENT ALBUM  ·  "
                    + (String.IsNullOrWhiteSpace(albumArtist) ? artist : albumArtist)
                    + "  •  "
                    + (String.IsNullOrWhiteSpace(albumTitle) ? title : albumTitle)
            };
            layout.Controls.Add(currentAlbum, 0, 2);

            matches = new ListView
            {
                Name = "musicBrainzMatchesList",
                Dock = DockStyle.Fill,
                View = View.Details,
                FullRowSelect = true,
                HideSelection = false,
                MultiSelect = false,
                ShowGroups = false
            };
            matches.Columns.Add("[#]", 46);
            matches.Columns.Add("Artist", 170);
            matches.Columns.Add("Country", 66);
            matches.Columns.Add("Date", 92);
            matches.Columns.Add("Release Type", 102);
            matches.Columns.Add("Release", 285);
            matches.Columns.Add("Resolution", 96);
            matches.Columns.Add("URL", 72);
            Populate(rawItems ?? new object[0]);
            matches.SelectedIndexChanged += delegate
            {
                Dictionary<string, object> item = SelectedItem();
                use.Enabled = item != null;
                if (item != null) RaiseArtworkPreview(item);
            };
            matches.DoubleClick += delegate { RequestUse(); };
            matches.MouseMove += MatchesMouseMove;
            matches.MouseLeave += delegate
            {
                hoveredMatchIndex = -1;
                EventHandler handler = ArtworkPreviewEnded;
                if (handler != null) handler(this, EventArgs.Empty);
            };
            layout.Controls.Add(matches, 0, 3);

            FlowLayoutPanel actions = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                WrapContents = false,
                Padding = new Padding(0, 6, 0, 0)
            };
            use = new FluentButton { Text = "Use Release", Width = 116, Height = 32, Enabled = false, Tag = "success" };
            use.Click += delegate { RequestUse(); };
            Button openPage = new FluentButton { Text = "Open MB Page", Width = 122, Height = 32 };
            openPage.Click += delegate { OpenSelectedMusicBrainzPage(); };
            Button search = new FluentButton { Text = "Search Artist / Track", Width = 165, Height = 32, Tag = "primary" };
            search.Click += delegate
            {
                EventHandler handler = SearchRequested;
                if (handler != null) handler(this, EventArgs.Empty);
            };
            Button unchanged = new FluentButton
            {
                Text = compilationTrack ? "Leave Track Unchanged" : "Return to Source Results",
                Width = compilationTrack ? 170 : 180,
                Height = 32
            };
            unchanged.Click += delegate
            {
                EventHandler handler = LeaveRequested;
                if (handler != null) handler(this, EventArgs.Empty);
            };
            actions.Controls.Add(use);
            actions.Controls.Add(openPage);
            actions.Controls.Add(search);
            actions.Controls.Add(unchanged);
            actions.Controls.Add(new InfoButton("Search Artist / Track performs the bounded MusicBrainz Recording search without using the curated compilation Album name."));
            layout.Controls.Add(actions, 0, 4);
            ThemeManager.Apply(this, theme);
            currentAlbum.ForeColor = ThemeManager.PaletteFor(theme).CategoryMagenta;
        }

        internal static string[] ArtworkPreviewUrls(Dictionary<string, object> item)
        {
            if (item == null) return new string[0];
            string releaseGroup = TextValue(item, "release_group_mbid", TextValue(item, "release_group_id", "")).Trim();
            string release = TextValue(item, "release_mbid", TextValue(item, "id", "")).Trim();
            List<string> urls = new List<string>();
            Guid parsed;
            if (Guid.TryParseExact(releaseGroup, "D", out parsed))
                urls.Add("https://coverartarchive.org/release-group/" + releaseGroup + "/front");
            if (Guid.TryParseExact(release, "D", out parsed))
                urls.Add("https://coverartarchive.org/release/" + release + "/front");
            return urls.Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        }

        private void Populate(object[] rawItems)
        {
            HashSet<string> groups = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach (object raw in rawItems)
            {
                Dictionary<string, object> item = raw as Dictionary<string, object>;
                if (item == null) continue;
                string decade = TextValue(item, "decade", "Unknown");
                string releaseType = TextValue(item, "release_class", "album").ToUpperInvariant();
                string groupKey = decade + "|" + releaseType;
                if (groups.Add(groupKey))
                {
                    ListViewItem category = new ListViewItem("");
                    category.SubItems.Add("[" + decade + "]  RELEASE TYPE [" + releaseType + "]");
                    while (category.SubItems.Count < matches.Columns.Count) category.SubItems.Add("");
                    category.ForeColor = ThemeManager.CurrentPalette.CategoryMagenta;
                    category.Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold);
                    category.Tag = null;
                    matches.Items.Add(category);
                }
                int index = IntValue(item, "index", matches.Items.Count + 1);
                ListViewItem row = new ListViewItem(index.ToString());
                row.SubItems.Add(TextValue(item, "release_artist", TextValue(item, "recording_artist", "")));
                row.SubItems.Add(TextValue(item, "country", ""));
                row.SubItems.Add(TextValue(item, "release_date", ""));
                row.SubItems.Add(releaseType);
                row.SubItems.Add(TextValue(item, "release_title", ""));
                row.SubItems.Add(TextValue(item, "resolution", "—"));
                row.SubItems.Add(ArtworkPreviewUrls(item).Length == 0 ? "" : "[URL]");
                row.Tag = item;
                if (BoolValue(item, "current")) row.ForeColor = ThemeManager.CurrentPalette.Success;
                else if (BoolValue(item, "visited")) row.ForeColor = ThemeManager.CurrentPalette.StatusBlue;
                matches.Items.Add(row);
            }
        }

        private void MatchesMouseMove(object sender, MouseEventArgs e)
        {
            ListViewItem row = matches.GetItemAt(e.X, e.Y);
            Dictionary<string, object> item = row == null ? null : row.Tag as Dictionary<string, object>;
            bool overUrl = item != null && row.SubItems.Count > 7 && row.SubItems[7].Bounds.Contains(e.Location)
                && ArtworkPreviewUrls(item).Length > 0;
            matches.Cursor = overUrl ? Cursors.Hand : Cursors.Default;
            int index = overUrl ? IntValue(item, "index", -1) : -1;
            if (index == hoveredMatchIndex) return;
            hoveredMatchIndex = index;
            if (overUrl) RaiseArtworkPreview(item);
            else
            {
                EventHandler handler = ArtworkPreviewEnded;
                if (handler != null) handler(this, EventArgs.Empty);
            }
        }

        private void RaiseArtworkPreview(Dictionary<string, object> item)
        {
            if (item == null || ArtworkPreviewUrls(item).Length == 0) return;
            EventHandler<MusicBrainzMatchEventArgs> handler = ArtworkPreviewRequested;
            if (handler != null) handler(this, new MusicBrainzMatchEventArgs(item, IntValue(item, "index", 0)));
        }

        private Dictionary<string, object> SelectedItem()
        {
            return matches.SelectedItems.Count == 1
                ? matches.SelectedItems[0].Tag as Dictionary<string, object>
                : null;
        }

        private void RequestUse()
        {
            Dictionary<string, object> item = SelectedItem();
            int index = IntValue(item, "index", 0);
            if (item == null || index <= 0) return;
            EventHandler<MusicBrainzMatchEventArgs> handler = UseRequested;
            if (handler != null) handler(this, new MusicBrainzMatchEventArgs(item, index));
        }

        private void RequestAuthorityEdit()
        {
            if (!ValidSingleMbid(recordingId.Text) || !ValidSingleMbid(releaseId.Text) || !ValidMbidList(artistId.Text))
            {
                MessageBox.Show(this, "Recording and Release accept one MusicBrainz UUID. Artist accepts one or more UUIDs separated by commas.", "MusicBrainz authority", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            EventHandler<MusicBrainzAuthorityEventArgs> handler = AuthorityEditRequested;
            if (handler != null) handler(this, new MusicBrainzAuthorityEventArgs(recordingId.Text.Trim(), artistId.Text.Trim(), releaseId.Text.Trim()));
        }

        private void OpenSelectedMusicBrainzPage()
        {
            Dictionary<string, object> item = SelectedItem();
            string url = item == null ? "" : TextValue(item, "url", "");
            if (String.IsNullOrWhiteSpace(url)) return;
            try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
            catch (Exception error) { MessageBox.Show(this, "Unable to open MusicBrainz URL.\r\n\r\n" + error.Message, "MusicBrainz page", MessageBoxButtons.OK, MessageBoxIcon.Warning); }
        }

        private static FluentTextBox AuthorityRow(TableLayoutPanel panel, int row, string label, string value)
        {
            panel.RowStyles.Add(new RowStyle(SizeType.Percent, 33.333f));
            panel.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, row);
            FluentTextBox text = new FluentTextBox { Text = value ?? "", Dock = DockStyle.Fill, Margin = new Padding(3, 1, 3, 1) };
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
            string scalar = item[key] as string;
            if (scalar != null) return scalar;
            IEnumerable values = item[key] as IEnumerable;
            if (values == null) return Convert.ToString(item[key]);
            return String.Join(", ", values.Cast<object>().Select(Convert.ToString));
        }

        private static string TextValue(Dictionary<string, object> item, string key, string fallback)
        {
            object value;
            return item != null && item.TryGetValue(key, out value) && value != null ? Convert.ToString(value) : fallback;
        }

        private static int IntValue(Dictionary<string, object> item, string key, int fallback)
        {
            object value;
            int parsed;
            return item != null && item.TryGetValue(key, out value) && Int32.TryParse(Convert.ToString(value), out parsed) ? parsed : fallback;
        }

        private static bool BoolValue(Dictionary<string, object> item, string key)
        {
            object value;
            bool parsed;
            return item != null && item.TryGetValue(key, out value) && Boolean.TryParse(Convert.ToString(value), out parsed) && parsed;
        }
    }

}
