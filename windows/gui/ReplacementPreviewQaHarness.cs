using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    /// <summary>
    /// Deterministic visual fixture for the replacement shell. Fixture names,
    /// counts and artwork live only in this QA executable and never enter the
    /// production application or database.
    /// </summary>
    internal static class ReplacementPreviewQaHarness
    {
        private static string output;
        private static string assetDirectory;

        [STAThread]
        private static void Main()
        {
            output = Environment.GetEnvironmentVariable("SPLINED_QA_OUTPUT");
            if (String.IsNullOrWhiteSpace(output))
                output = Path.Combine(ConfigStore.AppRoot, "replacement-preview-qa");
            output = Path.GetFullPath(output);
            assetDirectory = Path.Combine(output, "fixture-assets");
            Directory.CreateDirectory(output);
            Directory.CreateDirectory(assetDirectory);

            ThemeManager.Initialize("Dark");
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            ConfigState state = BuildState();
            UiState ui = BuildUi();
            using (MainForm form = new MainForm(state, ui))
            {
                form.StartPosition = FormStartPosition.Manual;
                form.Location = new Point(-30000, -30000);
                form.ShowInTaskbar = false;
                form.Size = new Size(1680, 945);
                form.Shown += delegate
                {
                    form.BeginInvoke((MethodInvoker)delegate
                    {
                        try
                        {
                            InstallFixture(form);
                            CaptureModes(form, state);
                        }
                        finally
                        {
                            foreach (Form owned in form.OwnedForms.ToArray()) owned.Close();
                            typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic)
                                .SetValue(form, false);
                            typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic)
                                .SetValue(form, false);
                            form.Close();
                        }
                    });
                };
                Application.Run(form);
            }
        }

        private static ConfigState BuildState()
        {
            ConfigState state = ConfigStore.Defaults();
            state.Mode = "write";
            state.MusicLibrary = assetDirectory;
            state.Sources = new List<string>
            {
                "navidrome", "deezer", "itunes", "fanarttv", "lastfm",
                "musicbrainz", "coverartarchive", "discogs", "amazon"
            };
            state.UpscalePicturePercent = 4;
            state.UpscaleSharpenPercent = 2;
            state.UpscaleContrastPercent = 3;
            return state;
        }

        private static UiState BuildUi()
        {
            return new UiState
            {
                Theme = "Dark",
                ShowStatusOnLaunch = false,
                MainMaximized = false,
                MainWidth = 1680,
                MainHeight = 945,
                MainSplitterDistance = 272,
                RightSplitterDistance = 690,
                ArtworkPreviewWidth = 410,
                ArtworkFiltersExpanded = true,
                ArtworkEditingExpanded = true,
                ArtworkWorkspaceMode = "FindMatch",
                LibraryNavigationMode = "Artists",
                PrimaryAdjustments = new List<string>
                    { "picture", "sharpen", "softness", "contrast", "exposure" }
            };
        }

        private static void InstallFixture(MainForm form)
        {
            typeof(MainForm).GetField("reloadVersion", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(form, Int32.MaxValue);
            List<string> artwork = Enumerable.Range(1, 9)
                .Select(index => CreateFixtureArtwork(index,
                    index <= 4 ? 1400 : index <= 7 ? 1200 : 900)).ToList();
            string[] artists =
            {
                "All Artists", "QA Ensemble", "Azure Echo", "Cinder Avenue",
                "Electric Atlas", "Glass Harbor", "Indigo North", "Neon Fields",
                "Pixel Choir", "Signal Bloom", "Velvet Transit"
            };
            List<AlbumInfo> albums = (List<AlbumInfo>)typeof(MainForm)
                .GetField("albums", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            albums.Clear();
            for (int index = 1; index < artists.Length; index++)
            {
                albums.Add(new AlbumInfo
                {
                    Artist = artists[index],
                    Title = index == 1 ? "Spectrum Stories" : "Album " + index,
                    Path = Path.Combine(@"C:\QA\Music", artists[index], "Album " + index),
                    State = index % 4 == 0 ? AlbumState.Processed : AlbumState.New,
                    Selected = index == 1,
                    CoverPath = artwork[(index - 1) % artwork.Count],
                    CoverName = "cover.png",
                    CoverWidth = 1400,
                    CoverHeight = 1400,
                    HasLocalArtwork = true,
                    TrackCount = 10 + index,
                    ReleaseYear = (2010 + index).ToString()
                });
            }
            typeof(MainForm).GetField("replacementLibraryMode", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(form, LibraryNavigationMode.Artists);
            typeof(MainForm).GetMethod("RefreshReplacementLibraryEntities",
                BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
            typeof(MainForm).GetField("displayedAlbum", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(form, albums[0]);
            typeof(MainForm).GetMethod("RenderDisplayedAlbum",
                BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);

            string[] sources =
            {
                "navidrome", "local", "musicbrainz", "discogs", "itunes",
                "deezer", "lastfm", "fanarttv", "coverartarchive"
            };
            List<object> candidates = new List<object>();
            for (int index = 0; index < artwork.Count; index++)
            {
                int side = index < 4 ? 1400 : index < 7 ? 1200 : 900;
                candidates.Add(new Dictionary<string, object>
                {
                    { "index", index + 1 }, { "source", sources[index] },
                    { "width", side }, { "height", side }, { "format", "png" },
                    { "range_class", side < 1200 ? "BelowMinimum" : "LowerRange" },
                    { "source_range_class", side < 1200 ? "BelowMinimum" : "LowerRange" },
                    { "acceptable", index < 7 }, { "recommended", index == 0 },
                    { "approved", index < 3 }, { "square", true },
                    { "upscaled", side < 1400 },
                    { "policy_status", index < 7 ? "accept" : "reject" },
                    { "policy_reason", index < 7 ? "Accepted by QA policy" : "Below configured minimum" },
                    { "cache_path", artwork[index] },
                    { "url", "https://qa.invalid/artwork/" + (index + 1) },
                    { "image_type", index == 6 ? "BackCover" : "FrontCover" }
                });
            }
            typeof(MainForm).GetMethod("ShowCandidates", BindingFlags.Instance | BindingFlags.NonPublic)
                .Invoke(form, new object[] { candidates.ToArray() });
            ((Label)typeof(MainForm).GetField("candidateContext",
                BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form)).Text =
                "QA Ensemble  ›  Spectrum Stories  ·  9 candidates";
            typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(form, true);
            typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(form, true);
            typeof(MainForm).GetField("activeLaunchAlbum", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(form, albums[0]);
            typeof(MainForm).GetMethod("UpdateSelectionControls",
                BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
            form.PerformLayout();
            Application.DoEvents();
        }

        private static void CaptureModes(MainForm form, ConfigState state)
        {
            MethodInfo showMode = typeof(MainForm).GetMethod("ShowReplacementMode",
                BindingFlags.Instance | BindingFlags.NonPublic);
            ShowAndCapture(form, showMode, ArtworkWorkspaceMode.FindMatch,
                "actual_01_FIND_MATCH_ARTWORK.png");
            ShowAndCapture(form, showMode, ArtworkWorkspaceMode.Edit,
                "actual_02_EDIT_ARTWORK.png");
            ShowAndCapture(form, showMode, ArtworkWorkspaceMode.Upscale,
                "actual_03_UPSCALE_ENHANCE.png");

            showMode.Invoke(form, new object[] { ArtworkWorkspaceMode.FindMatch, false });
            Dictionary<string, object>[] matches = Enumerable.Range(1, 6)
                .Select(index => new Dictionary<string, object>
                {
                    { "index", index }, { "release_artist", "QA Ensemble" },
                    { "release_title", index == 1 ? "Spectrum Stories" : "Spectrum Stories · Edition " + index },
                    { "country", index % 2 == 0 ? "CA" : "US" },
                    { "date", (2010 + index).ToString() }, { "release_type", "Album" },
                    { "release_mbid", "00000000-0000-4000-8000-" + index.ToString("D12") },
                    { "release_group_mbid", "10000000-0000-4000-8000-" + index.ToString("D12") },
                    { "visited", index < 3 }, { "current_visit", index == 1 },
                    { "url", "https://musicbrainz.org/release/00000000-0000-4000-8000-" + index.ToString("D12") }
                }).ToArray();
            typeof(MainForm).GetMethod("ApplyCoreEvent", BindingFlags.Instance | BindingFlags.NonPublic)
                .Invoke(form, new object[] { new Dictionary<string, object>
                {
                    { "event", "musicbrainz_matches" }, { "artist", "QA Ensemble" },
                    { "title", "Spectrum Stories" }, { "album_artist", "QA Ensemble" },
                    { "album", "Spectrum Stories" }, { "items", matches },
                    { "authority_recording_mbid", "20000000-0000-4000-8000-000000000001" },
                    { "authority_artist_mbids", "30000000-0000-4000-8000-000000000001" },
                    { "authority_release_mbid", "00000000-0000-4000-8000-000000000001" },
                    { "compilation_track", false }
                } });
            form.PerformLayout();
            Application.DoEvents();
            CaptureWindow(form, Path.Combine(output, "actual_04_MUSICBRAINZ_RESULTS.png"));
            typeof(MainForm).GetMethod("CloseMusicBrainzMatchesWorkspace",
                BindingFlags.Instance | BindingFlags.NonPublic, null, new[] { typeof(bool) }, null)
                .Invoke(form, new object[] { false });
            ((Label)typeof(MainForm).GetField("candidateContext",
                BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form)).Text =
                "QA Ensemble  ›  Spectrum Stories  ·  9 candidates";

            typeof(MainForm).GetMethod("OpenUpscaleEditor", BindingFlags.Instance | BindingFlags.NonPublic)
                .Invoke(form, null);
            Application.DoEvents();
            UpscaleEditorForm editor = (UpscaleEditorForm)typeof(MainForm)
                .GetField("upscaleEditor", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            if (editor == null) throw new InvalidOperationException("Custom Upscale did not open for QA.");
            editor.Location = new Point(-28000, -28000);
            editor.Size = new Size(1500, 900);
            editor.PerformLayout();
            Application.DoEvents();
            CaptureWindow(editor, Path.Combine(output, "actual_05_CUSTOM_UPSCALE.png"));
            editor.Close();

            using (CredentialEditForm credentials = new CredentialEditForm(state,
                "navidrome", "Navidrome"))
            {
                credentials.Location = new Point(-28000, -28000);
                credentials.Show(form);
                credentials.PerformLayout();
                Application.DoEvents();
                CaptureWindow(credentials,
                    Path.Combine(output, "actual_06_NAVIDROME_CREDENTIALS.png"));
                credentials.Close();
            }

            form.Size = new Size(1280, 800);
            showMode.Invoke(form, new object[] { ArtworkWorkspaceMode.FindMatch, false });
            form.PerformLayout();
            Application.DoEvents();
            CaptureWindow(form, Path.Combine(output, "actual_07_RESPONSIVE_1280x800.png"));
        }

        private static void ShowAndCapture(MainForm form, MethodInfo showMode,
            ArtworkWorkspaceMode mode, string fileName)
        {
            showMode.Invoke(form, new object[] { mode, false });
            form.PerformLayout();
            Application.DoEvents();
            CaptureWindow(form, Path.Combine(output, fileName));
        }

        private static string CreateFixtureArtwork(int index, int side)
        {
            string path = Path.Combine(assetDirectory,
                "artwork-" + index + "-" + side + ".png");
            if (File.Exists(path)) return path;
            Color[] accents =
            {
                Color.FromArgb(27, 210, 222), Color.FromArgb(71, 92, 245),
                Color.FromArgb(190, 46, 231), Color.FromArgb(245, 53, 153),
                Color.FromArgb(250, 139, 52), Color.FromArgb(239, 206, 57)
            };
            float scale = side / 900f;
            using (Bitmap bitmap = new Bitmap(side, side))
            using (Graphics graphics = Graphics.FromImage(bitmap))
            using (LinearGradientBrush gradient = new LinearGradientBrush(
                new Rectangle(0, 0, side, side), Color.FromArgb(5, 22, 42),
                Color.FromArgb(30, accents[index % accents.Length]),
                index % 2 == 0 ? 35f : 145f))
            using (Pen line = new Pen(accents[index % accents.Length], 12 * scale))
            using (Font title = new Font("Segoe UI", 54 * scale, FontStyle.Bold))
            using (Font subtitle = new Font("Segoe UI", 24 * scale, FontStyle.Regular))
            {
                graphics.SmoothingMode = SmoothingMode.AntiAlias;
                graphics.FillRectangle(gradient, 0, 0, side, side);
                for (int ring = 0; ring < 8; ring++)
                    graphics.DrawEllipse(line, (90 + ring * 35) * scale,
                        (90 + ring * 35) * scale,
                        (720 - ring * 70) * scale, (720 - ring * 70) * scale);
                TextRenderer.DrawText(graphics, "SPECTRUM", title,
                    new Rectangle((int)(60 * scale), (int)(330 * scale),
                        (int)(780 * scale), (int)(90 * scale)), Color.White,
                    TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
                TextRenderer.DrawText(graphics, "QA ARTWORK  " + index, subtitle,
                    new Rectangle((int)(60 * scale), (int)(430 * scale),
                        (int)(780 * scale), (int)(60 * scale)),
                    accents[index % accents.Length],
                    TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
                bitmap.Save(path, ImageFormat.Png);
            }
            return path;
        }

        private static void CaptureWindow(Form form, string path)
        {
            form.CreateControl();
            form.PerformLayout();
            Application.DoEvents();
            using (Bitmap screenshot = new Bitmap(Math.Max(1, form.Width),
                Math.Max(1, form.Height)))
            {
                bool printed = false;
                try
                {
                    using (Graphics target = Graphics.FromImage(screenshot))
                    {
                        IntPtr hdc = target.GetHdc();
                        try { printed = PrintWindow(form.Handle, hdc, 2); }
                        finally { target.ReleaseHdc(hdc); }
                    }
                }
                catch { printed = false; }
                if (!printed)
                    form.DrawToBitmap(screenshot, new Rectangle(Point.Empty,
                        new Size(Math.Min(screenshot.Width, form.ClientSize.Width),
                            Math.Min(screenshot.Height, form.ClientSize.Height))));
                screenshot.Save(path, ImageFormat.Png);
            }
        }

        [DllImport("user32.dll")]
        private static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    }
}
