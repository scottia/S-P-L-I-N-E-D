using System;
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

    /// <summary>
    /// MusicBrainz decision surface embedded in the Scan Activity workspace so
    /// the shared Artwork panel remains visible throughout authority review.
    /// </summary>
    internal sealed class MusicBrainzMatchesPanel : UserControl
    {
        private const string AuthorityRowName = "musicBrainzAuthorityRow";
        private readonly ListView matches;
        private readonly Button use;
        private readonly Button openPage;
        private readonly Button artistFilter;
        private readonly Button releaseTypeFilter;
        private readonly FluentTextBox artistId;
        private readonly FluentTextBox releaseId;
        private readonly FluentTextBox recordingId;
        private readonly List<Dictionary<string, object>> resultItems;
        private readonly Dictionary<string, object> authorityItem;
        private readonly HashSet<string> selectedArtists;
        private readonly HashSet<string> selectedReleaseTypes;
        private readonly string authorityArtist;
        private readonly string authorityRelease;
        private readonly string theme;
        private ToolStripDropDown activeFilter;
        private int hoveredMatchIndex = -1;

        public event EventHandler<MusicBrainzMatchEventArgs> UseRequested;
        public event EventHandler<MusicBrainzMatchEventArgs> ArtworkPreviewRequested;
        public event EventHandler ArtworkPreviewEnded;
        public event EventHandler LeaveRequested;

        public MusicBrainzMatchesPanel(
            string artist,
            string title,
            string albumArtist,
            string albumTitle,
            object[] rawItems,
            string authorityRecordingMbid,
            string authorityArtistMbids,
            string authorityReleaseMbid,
            bool compilationTrack,
            string theme)
        {
            this.theme = theme;
            authorityArtist = String.IsNullOrWhiteSpace(albumArtist) ? artist : albumArtist;
            authorityRelease = String.IsNullOrWhiteSpace(albumTitle) ? title : albumTitle;
            resultItems = (rawItems ?? new object[0])
                .OfType<Dictionary<string, object>>()
                .ToList();
            selectedArtists = new HashSet<string>(
                DistinctValues(resultItems, ArtistValue), StringComparer.OrdinalIgnoreCase);
            selectedReleaseTypes = new HashSet<string>(
                DistinctValues(resultItems, ReleaseTypeValue), StringComparer.OrdinalIgnoreCase);
            authorityItem = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase)
            {
                { "authority", true },
                { "index", 0 },
                { "recording_mbid", authorityRecordingMbid ?? "" },
                { "artist_mbids", authorityArtistMbids ?? "" },
                { "release_mbid", authorityReleaseMbid ?? "" },
                { "release_artist", authorityArtist },
                { "release_title", authorityRelease },
                { "release_class", "CURRENT ALBUM" },
                { "url", MusicBrainzReleaseUrl(authorityReleaseMbid) }
            };

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
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            Controls.Add(layout);
            layout.Controls.Add(new Label
            {
                Dock = DockStyle.Fill,
                AutoEllipsis = true,
                Text = "Choose the authoritative release for " + artist + " — " + title
                    + ". Yellow is the current Album, green is the most recent visit, and orange marks earlier visits."
            }, 0, 0);

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
            artistId = AuthorityRow(authority, 0, "MusicBrainz Artist ID(s)", authorityArtistMbids);
            releaseId = AuthorityRow(authority, 1, "MusicBrainz Release ID", authorityReleaseMbid);
            recordingId = AuthorityRow(authority, 2, "MusicBrainz Recording ID", authorityRecordingMbid);
            Button applyAuthority = new FluentButton { Text = "Apply IDs", Dock = DockStyle.Fill, Margin = new Padding(6, 3, 0, 3), Tag = "primary" };
            applyAuthority.Click += delegate { ApplyAuthorityIds(); };
            authority.Controls.Add(applyAuthority, 2, 0);
            authority.SetRowSpan(applyAuthority, 3);
            layout.Controls.Add(authority, 0, 1);

            FlowLayoutPanel filters = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                WrapContents = false,
                Margin = Padding.Empty,
                Padding = new Padding(0, 2, 0, 2)
            };
            artistFilter = new FluentButton { Name = "musicBrainzArtistFilter", Text = "Filter by Artist ▼", Width = 170, Height = 30 };
            releaseTypeFilter = new FluentButton { Name = "musicBrainzReleaseTypeFilter", Text = "Filter by Release Type ▼", Width = 205, Height = 30 };
            artistFilter.Click += delegate { ShowArtistFilter(); };
            releaseTypeFilter.Click += delegate { ShowReleaseTypeFilter(); };
            filters.Controls.Add(artistFilter);
            filters.Controls.Add(releaseTypeFilter);
            filters.Controls.Add(new Label
            {
                AutoSize = true,
                Padding = new Padding(8, 7, 0, 0),
                Text = "Ctrl selects multiple values · Enter applies"
            });
            layout.Controls.Add(filters, 0, 2);

            matches = new ListView
            {
                Name = "musicBrainzMatchesList",
                Dock = DockStyle.Fill,
                View = View.Details,
                FullRowSelect = true,
                HideSelection = false,
                MultiSelect = false,
                ShowGroups = false,
                ShowItemToolTips = true
            };
            matches.Columns.Add("[#]", 46);
            matches.Columns.Add("Artist", 170);
            matches.Columns.Add("Country", 66);
            matches.Columns.Add("Date", 92);
            matches.Columns.Add("Release Type", 112);
            matches.Columns.Add("Release", 285);
            matches.Columns.Add("Resolution", 96);
            matches.Columns.Add("URL", 72);
            use = new FluentButton { Text = "Use Release", Width = 116, Height = 32, Enabled = false, Tag = "success" };
            openPage = new FluentButton { Text = "Open MB Page", Width = 122, Height = 32, Enabled = false };
            RebuildVisibleRows();
            matches.SelectedIndexChanged += delegate { SelectionChanged(); };
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
            use.Click += delegate { RequestUse(); };
            openPage.Click += delegate { OpenSelectedMusicBrainzPage(); };
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
            actions.Controls.Add(unchanged);
            layout.Controls.Add(actions, 0, 4);
            ThemeManager.Apply(this, theme);
            ApplyRowColors();
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing && activeFilter != null)
            {
                activeFilter.Close();
                activeFilter.Dispose();
                activeFilter = null;
            }
            base.Dispose(disposing);
        }

        internal static string[] ArtworkPreviewUrls(Dictionary<string, object> item)
        {
            if (item == null || IsAuthority(item)) return new string[0];
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

        internal string[] ArtistFilterChoices
        {
            get { return DistinctValues(resultItems, ArtistValue); }
        }

        internal string[] ReleaseTypeFilterChoices
        {
            get { return AvailableReleaseTypes(); }
        }

        internal void ApplyFilterSelection(IEnumerable<string> artists, IEnumerable<string> releaseTypes)
        {
            selectedArtists.Clear();
            foreach (string value in artists ?? Enumerable.Empty<string>()) selectedArtists.Add(value ?? "");
            selectedReleaseTypes.Clear();
            foreach (string value in releaseTypes ?? Enumerable.Empty<string>()) selectedReleaseTypes.Add(value ?? "");
            RebuildVisibleRows();
        }

        private void ShowArtistFilter()
        {
            string[] choices = ArtistFilterChoices;
            ShowMultiSelectFilter(artistFilter, choices, selectedArtists, delegate(HashSet<string> selected)
            {
                string[] previousAvailable = AvailableReleaseTypes();
                bool allTypesSelected = previousAvailable.All(value => selectedReleaseTypes.Contains(value));
                selectedArtists.Clear();
                selectedArtists.UnionWith(selected);
                string[] narrowed = AvailableReleaseTypes();
                if (allTypesSelected)
                {
                    selectedReleaseTypes.Clear();
                    selectedReleaseTypes.UnionWith(narrowed);
                }
                else
                {
                    selectedReleaseTypes.IntersectWith(narrowed);
                }
                RebuildVisibleRows();
            });
        }

        private void ShowReleaseTypeFilter()
        {
            ShowMultiSelectFilter(releaseTypeFilter, AvailableReleaseTypes(), selectedReleaseTypes, delegate(HashSet<string> selected)
            {
                selectedReleaseTypes.Clear();
                selectedReleaseTypes.UnionWith(selected);
                RebuildVisibleRows();
            });
        }

        private void ShowMultiSelectFilter(Button owner, string[] choices, HashSet<string> selected, Action<HashSet<string>> apply)
        {
            if (activeFilter != null)
            {
                activeFilter.Close();
                activeFilter.Dispose();
            }
            ListBox list = new ListBox
            {
                BorderStyle = BorderStyle.None,
                IntegralHeight = false,
                SelectionMode = System.Windows.Forms.SelectionMode.MultiExtended,
                Width = Math.Max(owner.Width, 220),
                Height = Math.Max(70, Math.Min(260, Math.Max(1, choices.Length) * 24 + 8))
            };
            foreach (string choice in choices) list.Items.Add(choice);
            for (int index = 0; index < choices.Length; index++)
                if (selected.Contains(choices[index])) list.SetSelected(index, true);
            ToolStripControlHost host = new ToolStripControlHost(list)
            {
                AutoSize = false,
                Margin = Padding.Empty,
                Padding = Padding.Empty,
                Size = list.Size
            };
            ToolStripDropDown dropDown = new ToolStripDropDown
            {
                AutoSize = false,
                Padding = new Padding(1),
                Size = new Size(list.Width + 2, list.Height + 2)
            };
            dropDown.Items.Add(host);
            list.KeyDown += delegate(object sender, KeyEventArgs args)
            {
                if (args.Control && args.KeyCode == Keys.A)
                {
                    for (int index = 0; index < list.Items.Count; index++) list.SetSelected(index, true);
                    args.Handled = true;
                    args.SuppressKeyPress = true;
                }
                else if (args.KeyCode == Keys.Enter)
                {
                    HashSet<string> values = new HashSet<string>(
                        list.SelectedItems.Cast<object>().Select(Convert.ToString),
                        StringComparer.OrdinalIgnoreCase);
                    apply(values);
                    dropDown.Close(ToolStripDropDownCloseReason.Keyboard);
                    args.Handled = true;
                    args.SuppressKeyPress = true;
                }
            };
            dropDown.Closed += delegate
            {
                if (ReferenceEquals(activeFilter, dropDown)) activeFilter = null;
                dropDown.Dispose();
            };
            activeFilter = dropDown;
            ThemeManager.Apply(list, theme);
            dropDown.Show(owner, new Point(0, owner.Height));
            list.Focus();
        }

        private void RebuildVisibleRows()
        {
            string selectedKey = SelectedRowKey();
            matches.BeginUpdate();
            try
            {
                matches.Items.Clear();
                AddAuthorityRows();
                string visibleReleaseType = null;
                foreach (Dictionary<string, object> item in SortedVisibleResults())
                {
                    string releaseType = ReleaseTypeValue(item);
                    if (!String.Equals(visibleReleaseType, releaseType, StringComparison.OrdinalIgnoreCase))
                    {
                        AddResultCategory(releaseType);
                        visibleReleaseType = releaseType;
                    }
                    matches.Items.Add(ResultRow(item));
                }
                ApplyRowColors();
                RestoreSelectedRow(selectedKey);
            }
            finally { matches.EndUpdate(); }
            SelectionChanged();
        }

        private void AddAuthorityRows()
        {
            ListViewItem category = new ListViewItem("") { Name = "musicBrainzCurrentAlbumCategory" };
            category.SubItems.Add("[CURRENT ALBUM]");
            while (category.SubItems.Count < matches.Columns.Count) category.SubItems.Add("");
            category.Tag = null;
            category.Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold);
            matches.Items.Add(category);

            ListViewItem row = new ListViewItem("[*]")
            {
                Name = AuthorityRowName,
                Tag = authorityItem,
                ToolTipText = AuthorityToolTip()
            };
            row.SubItems.Add(authorityArtist);
            row.SubItems.Add("");
            row.SubItems.Add("");
            row.SubItems.Add("");
            row.SubItems.Add(authorityRelease);
            row.SubItems.Add("—");
            row.SubItems.Add(String.IsNullOrWhiteSpace(TextValue(authorityItem, "url", "")) ? "" : "[URL]");
            matches.Items.Add(row);
        }

        private void AddResultCategory(string releaseType)
        {
            ListViewItem category = new ListViewItem("") { Name = "musicBrainzResultCategory" };
            category.SubItems.Add("RELEASE TYPE [" + releaseType.ToUpperInvariant() + "]");
            while (category.SubItems.Count < matches.Columns.Count) category.SubItems.Add("");
            category.Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold);
            category.Tag = null;
            matches.Items.Add(category);
        }

        private ListViewItem ResultRow(Dictionary<string, object> item)
        {
            string releaseType = ReleaseTypeValue(item);
            int index = IntValue(item, "index", 0);
            ListViewItem row = new ListViewItem(index.ToString()) { Tag = item };
            row.SubItems.Add(ArtistValue(item));
            row.SubItems.Add(TextValue(item, "country", ""));
            row.SubItems.Add(TextValue(item, "release_date", ""));
            row.SubItems.Add(releaseType.ToUpperInvariant());
            row.SubItems.Add(TextValue(item, "release_title", ""));
            row.SubItems.Add(TextValue(item, "resolution", "—"));
            row.SubItems.Add(ArtworkPreviewUrls(item).Length == 0 ? "" : "[URL]");
            return row;
        }

        private void ApplyRowColors()
        {
            ThemePalette palette = ThemeManager.PaletteFor(theme);
            foreach (ListViewItem row in matches.Items)
            {
                Dictionary<string, object> item = row.Tag as Dictionary<string, object>;
                if (row.Name == "musicBrainzCurrentAlbumCategory" || IsAuthority(item))
                    row.ForeColor = palette.Warning;
                else if (row.Name == "musicBrainzResultCategory")
                    row.ForeColor = palette.CategoryMagenta;
                else if (BoolValue(item, "current"))
                    row.ForeColor = palette.Success;
                else if (BoolValue(item, "visited"))
                    row.ForeColor = palette.StatusOrange;
            }
        }

        private void SelectionChanged()
        {
            Dictionary<string, object> item = SelectedItem();
            use.Enabled = item != null && !IsAuthority(item) && IntValue(item, "index", 0) > 0;
            openPage.Enabled = item != null && !String.IsNullOrWhiteSpace(TextValue(item, "url", ""));
            if (item != null && !IsAuthority(item)) RaiseArtworkPreview(item);
        }

        private void MatchesMouseMove(object sender, MouseEventArgs e)
        {
            ListViewItem row = matches.GetItemAt(e.X, e.Y);
            Dictionary<string, object> item = row == null ? null : row.Tag as Dictionary<string, object>;
            bool overUrl = item != null && !IsAuthority(item) && row.SubItems.Count > 7
                && row.SubItems[7].Bounds.Contains(e.Location) && ArtworkPreviewUrls(item).Length > 0;
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
            if (item == null || IsAuthority(item) || index <= 0) return;
            EventHandler<MusicBrainzMatchEventArgs> handler = UseRequested;
            if (handler != null) handler(this, new MusicBrainzMatchEventArgs(item, index));
        }

        private void ApplyAuthorityIds()
        {
            if (!ValidSingleMbid(recordingId.Text) || !ValidSingleMbid(releaseId.Text) || !ValidMbidList(artistId.Text))
            {
                MessageBox.Show(this, "Recording and Release accept one MusicBrainz UUID. Artist accepts one or more UUIDs separated by commas.", "MusicBrainz authority", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            authorityItem["recording_mbid"] = recordingId.Text.Trim();
            authorityItem["artist_mbids"] = NormalizeMbidList(artistId.Text);
            authorityItem["release_mbid"] = releaseId.Text.Trim();
            authorityItem["url"] = MusicBrainzReleaseUrl(releaseId.Text);
            artistId.Text = Convert.ToString(authorityItem["artist_mbids"]);
            ListViewItem row = matches.Items.Cast<ListViewItem>().FirstOrDefault(item => item.Name == AuthorityRowName);
            if (row != null)
            {
                row.SubItems[7].Text = String.IsNullOrWhiteSpace(TextValue(authorityItem, "url", "")) ? "" : "[URL]";
                row.ToolTipText = AuthorityToolTip();
                row.Selected = true;
            }
            SelectionChanged();
        }

        private void OpenSelectedMusicBrainzPage()
        {
            Dictionary<string, object> item = SelectedItem();
            string url = item == null ? "" : TextValue(item, "url", "");
            if (String.IsNullOrWhiteSpace(url)) return;
            try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
            catch (Exception error) { MessageBox.Show(this, "Unable to open MusicBrainz URL.\r\n\r\n" + error.Message, "MusicBrainz page", MessageBoxButtons.OK, MessageBoxIcon.Warning); }
        }

        private string SelectedRowKey()
        {
            if (matches.SelectedItems.Count != 1) return null;
            Dictionary<string, object> item = matches.SelectedItems[0].Tag as Dictionary<string, object>;
            if (IsAuthority(item)) return "authority";
            return item == null ? null : ResultKey(item);
        }

        private void RestoreSelectedRow(string key)
        {
            if (String.IsNullOrWhiteSpace(key)) return;
            foreach (ListViewItem row in matches.Items)
            {
                Dictionary<string, object> item = row.Tag as Dictionary<string, object>;
                string candidate = IsAuthority(item) ? "authority" : item == null ? null : ResultKey(item);
                if (!String.Equals(key, candidate, StringComparison.OrdinalIgnoreCase)) continue;
                row.Selected = true;
                row.Focused = true;
                return;
            }
        }

        private string[] AvailableReleaseTypes()
        {
            return DistinctValues(
                resultItems.Where(item => selectedArtists.Contains(ArtistValue(item))),
                ReleaseTypeValue);
        }

        private IEnumerable<Dictionary<string, object>> SortedVisibleResults()
        {
            return resultItems
                .Where(item => selectedArtists.Contains(ArtistValue(item))
                    && selectedReleaseTypes.Contains(ReleaseTypeValue(item)))
                .OrderBy(ResultFamilyRank)
                .ThenBy(item => ResultFamilyRank(item) == 0 ? ArtistValue(item) : "", StringComparer.OrdinalIgnoreCase)
                .ThenByDescending(DateSortValue)
                .ThenBy(CountrySortRank)
                .ThenBy(CountrySortValue, StringComparer.OrdinalIgnoreCase)
                .ThenBy(item => TextValue(item, "release_title", ""), StringComparer.OrdinalIgnoreCase)
                .ThenBy(item => TextValue(item, "release_mbid", ""), StringComparer.OrdinalIgnoreCase);
        }

        private static int ResultFamilyRank(Dictionary<string, object> item)
        {
            string releaseType = ReleaseTypeValue(item);
            if (releaseType.Equals("soundtrack", StringComparison.OrdinalIgnoreCase)) return 1;
            if (releaseType.Equals("compilation", StringComparison.OrdinalIgnoreCase)) return 2;
            if (ArtistValue(item).Equals("Various Artists", StringComparison.OrdinalIgnoreCase)) return 3;
            if (String.IsNullOrWhiteSpace(ArtistValue(item))) return 4;
            return 0;
        }

        private static int DateSortValue(Dictionary<string, object> item)
        {
            string[] parts = TextValue(item, "release_date", "").Trim().Split('-');
            int year;
            if (parts.Length == 0 || !Int32.TryParse(parts[0], out year) || year <= 0) return -1;
            int month = DatePart(parts, 1, 12);
            int day = DatePart(parts, 2, 31);
            return year * 10000 + month * 100 + day;
        }

        private static int DatePart(string[] parts, int index, int maximum)
        {
            int value;
            return index < parts.Length && Int32.TryParse(parts[index], out value)
                && value >= 0 && value <= maximum ? value : 0;
        }

        private static int CountrySortRank(Dictionary<string, object> item)
        {
            string country = TextValue(item, "country", "").Trim();
            if (country.Equals("US", StringComparison.OrdinalIgnoreCase)) return 0;
            return String.IsNullOrWhiteSpace(country) ? 2 : 1;
        }

        private static string CountrySortValue(Dictionary<string, object> item)
        {
            return TextValue(item, "country", "").Trim();
        }

        private string AuthorityToolTip()
        {
            return "Artist MBID(s): " + TextValue(authorityItem, "artist_mbids", "")
                + "\r\nRelease MBID: " + TextValue(authorityItem, "release_mbid", "")
                + "\r\nRecording MBID: " + TextValue(authorityItem, "recording_mbid", "");
        }

        private static FluentTextBox AuthorityRow(TableLayoutPanel panel, int row, string label, string value)
        {
            panel.RowStyles.Add(new RowStyle(SizeType.Percent, 33.333f));
            panel.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, row);
            FluentTextBox text = new FluentTextBox { Text = value ?? "", Dock = DockStyle.Fill, Margin = new Padding(3, 1, 3, 1) };
            panel.Controls.Add(text, 1, row);
            return text;
        }

        private static string[] DistinctValues(IEnumerable<Dictionary<string, object>> items, Func<Dictionary<string, object>, string> value)
        {
            HashSet<string> seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            return items.Select(value).Where(item => seen.Add(item)).ToArray();
        }

        private static string ArtistValue(Dictionary<string, object> item)
        {
            return TextValue(item, "release_artist", TextValue(item, "recording_artist", ""));
        }

        private static string ReleaseTypeValue(Dictionary<string, object> item)
        {
            return TextValue(item, "release_class", "album").Trim();
        }

        private static string ResultKey(Dictionary<string, object> item)
        {
            return TextValue(item, "release_mbid", "") + "#" + IntValue(item, "index", 0);
        }

        private static bool IsAuthority(Dictionary<string, object> item)
        {
            return BoolValue(item, "authority");
        }

        private static string MusicBrainzReleaseUrl(string releaseMbid)
        {
            Guid parsed;
            string value = (releaseMbid ?? "").Trim();
            return Guid.TryParseExact(value, "D", out parsed)
                ? "https://musicbrainz.org/release/" + value.ToLowerInvariant()
                : "";
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

        private static string NormalizeMbidList(string value)
        {
            return String.Join(", ", (value ?? "")
                .Split(new[] { ',', ';', ' ' }, StringSplitOptions.RemoveEmptyEntries)
                .Select(item => item.Trim().ToLowerInvariant()));
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
            return item != null && item.TryGetValue(key, out value) && value != null
                && Boolean.TryParse(Convert.ToString(value), out parsed) && parsed;
        }
    }
}
