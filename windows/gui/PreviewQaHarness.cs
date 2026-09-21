using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class PreviewQaHarness
    {
        [STAThread]
        private static void Main(string[] args)
        {
            UiState uiState = ConfigStore.LoadUi();
            if (Array.IndexOf(args, "--dark") >= 0) uiState.Theme = "Dark";
            else if (Array.IndexOf(args, "--light") >= 0) uiState.Theme = "Light";
            else if (Array.IndexOf(args, "--system") >= 0) uiState.Theme = "System";
            uiState.ShowStatusOnLaunch = false;
            uiState.MainMaximized = false;
            uiState.SetupMaximized = false;
            uiState.MainWidth = 1600;
            uiState.MainHeight = 1000;
            uiState.MainSplitterDistance = 430;
            uiState.RightSplitterDistance = 285;
            ThemeManager.Initialize(uiState.Theme);
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            if (Array.IndexOf(args, "--settings-capture") >= 0)
            {
                CaptureSettings(uiState);
                return;
            }
            if (Array.IndexOf(args, "--about-capture") >= 0)
            {
                CaptureAbout();
                return;
            }
            string imageDirectory = Path.Combine(ConfigStore.AppRoot, "preview-qa");
            Directory.CreateDirectory(imageDirectory);
            Color[] colors = { Color.FromArgb(53, 102, 145), Color.FromArgb(132, 76, 103), Color.FromArgb(75, 126, 83), Color.FromArgb(151, 104, 45) };
            string[] sources = { "iTunes", "Discogs", "Cover Art Archive", "Last.fm" };
            List<Dictionary<string, object>> candidates = new List<Dictionary<string, object>>();
            for (int index = 0; index < 12; index++)
            {
                string path = Path.Combine(imageDirectory, "candidate-" + (index + 1) + ".png");
                using (Bitmap bitmap = new Bitmap(800, 800))
                using (Graphics graphics = Graphics.FromImage(bitmap))
                using (SolidBrush brush = new SolidBrush(colors[index % colors.Length]))
                using (Font font = new Font("Segoe UI", 42, FontStyle.Bold))
                {
                    graphics.FillRectangle(brush, 0, 0, bitmap.Width, bitmap.Height);
                    TextRenderer.DrawText(graphics, sources[index % sources.Length] + "\r\n" + (index + 1), font, new Rectangle(35, 35, 730, 730), Color.White,
                        TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.WordBreak);
                    bitmap.Save(path, ImageFormat.Png);
                }
                candidates.Add(new Dictionary<string, object>
                {
                    { "index", index + 1 },
                    { "source", sources[index % sources.Length].ToLowerInvariant().Replace(" ", "") },
                    { "width", 1800 }, { "height", 1800 },
                    { "range_class", "ideal" }, { "acceptable", true },
                    { "policy_status", "accept" }, { "policy_reason", "Accepted by the active policy" },
                    { "source_override_active", false }, { "recommended", index == 0 },
                    { "cache_path", path }, { "url", "https://example.invalid/candidate/" + (index + 1) }
                });
            }
            ConfigState state = ConfigStore.Defaults();
            state.Mode = "write";
            state.MusicLibrary = imageDirectory;
            using (MainForm form = new MainForm(state, uiState))
            {
                form.Size = new Size(1600, 1000);
                bool capture = Array.IndexOf(args, "--capture") >= 0;
                if (capture)
                {
                    form.ShowInTaskbar = false;
                    form.StartPosition = FormStartPosition.Manual;
                    form.Location = new Point(-32000, -32000);
                }
                form.Shown += delegate
                {
                    typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, true);
                    MethodInfo apply = typeof(MainForm).GetMethod("ApplyCoreEvent", BindingFlags.Instance | BindingFlags.NonPublic);
                    typeof(MainForm).GetMethod("BeginAlbumRunStatistics", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form,
                        new object[] { new AlbumInfo { Artist = "QA Artist", Title = "Responsive Candidate Rows", Path = @"C:\QA\Report", State = AlbumState.New } });
                    typeof(MainForm).GetMethod("AppendAlbumActivityHeader", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form,
                        new object[] { 7, 9, "QA Artist", "Responsive Candidate Rows" });
                    apply.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "release_resolved" }, { "artist", "QA Artist" }, { "release", "Responsive Candidate Rows" }, { "fallback", false }
                    } });
                    apply.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "candidates" }, { "items", candidates.ToArray() }, { "hidden_by_source_policy", 4 }, { "fallback", false }
                    } });
                    apply.Invoke(form, new object[] { new Dictionary<string, object>
                    {
                        { "event", "decision_required" }, { "reason", "review" }
                    } });
                    if (capture)
                    {
                        form.BeginInvoke((MethodInvoker)delegate
                        {
                            // The real form begins its normal asynchronous library load on Shown.
                            // Supersede that QA-only load before installing the deterministic
                            // in-memory visual-review tree used by the capture.
                            typeof(MainForm).GetField("reloadVersion", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, Int32.MaxValue);
                            List<AlbumInfo> mediaAlbums = new List<AlbumInfo>
                            {
                                new AlbumInfo { Artist = "QA Artist", Title = "Unprocessed Album", Path = @"C:\QA\Unprocessed", State = AlbumState.New },
                                new AlbumInfo { Artist = "QA Artist", Title = "Processed Album", Path = @"C:\QA\Processed", State = AlbumState.Processed },
                                new AlbumInfo { Artist = "Bypass Artist", Title = "Bypassed Album", Path = @"C:\QA\Bypassed", State = AlbumState.Bypassed }
                            };
                            typeof(MainForm).GetField("albums", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, mediaAlbums);
                            typeof(MainForm).GetMethod("SetMediaFilterExpanded", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { true });
                            form.Controls.Find("mediaArtistFilter", true)[0].Text = "";
                            form.Controls.Find("mediaAlbumFilter", true)[0].Text = "";
                            form.Controls.Find("filteredScanRead", true)[0].GetType().GetProperty("Checked").SetValue(form.Controls.Find("filteredScanRead", true)[0], true, null);
                            typeof(MainForm).GetMethod("BuildTree", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                            typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, false);
                            typeof(MainForm).GetField("loadingLibrary", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, false);
                            typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, false);
                            typeof(MainForm).GetMethod("UpdateSelectionControls", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                            typeof(MainForm).GetMethod("UpdateMediaFilterColors", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                            form.PerformLayout();
                            CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "main-media-filter-idle-qa.png"));
                            typeof(MainForm).GetMethod("SetMediaFilterExpanded", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { false });
                            form.PerformLayout();
                            CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "main-media-filter-collapsed-qa.png"));
                            typeof(MainForm).GetMethod("SetMediaFilterExpanded", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { true });
                            form.PerformLayout();
                            typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, true);
                            typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, true);
                            typeof(MainForm).GetMethod("UpdateSelectionControls", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                            CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "main-candidates-qa.png"));
                            CaptureAppearanceMenu(form, Path.Combine(ConfigStore.AppRoot, "appearance-menu-qa.png"));
                            apply.Invoke(form, new object[] { new Dictionary<string, object>
                            {
                                { "event", "album_completed" }, { "album_path", @"C:\QA\Report" },
                                { "action", "Installed" }, { "destination", @"C:\QA\Report\cover.jpg" }
                            } });
                            typeof(MainForm).GetMethod("FinishActiveAlbumStatistics", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { "Incomplete" });
                            typeof(MainForm).GetMethod("ShowAlbumRunReport", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { "write", 1 });
                            typeof(MainForm).GetField("running", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, false);
                            typeof(MainForm).GetField("awaitingDecision", BindingFlags.Instance | BindingFlags.NonPublic).SetValue(form, false);
                            typeof(MainForm).GetMethod("UpdateSelectionControls", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, null);
                            form.PerformLayout();
                            CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "main-album-report-qa.png"));
                            using (Bitmap icon = form.Icon.ToBitmap())
                                icon.Save(Path.Combine(ConfigStore.AppRoot, "app-icon-qa.png"), ImageFormat.Png);
                            form.Close();
                        });
                    }
                };
                Application.Run(form);
            }
        }

        private static void CaptureAbout()
        {
            using (AboutForm form = new AboutForm())
            {
                form.ShowInTaskbar = false;
                form.StartPosition = FormStartPosition.Manual;
                form.Location = new Point(-32000, -32000);
                form.Shown += delegate
                {
                    form.BeginInvoke((MethodInvoker)delegate
                    {
                        form.PerformLayout();
                        Application.DoEvents();
                        CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "about-qa.png"));
                        form.Close();
                    });
                };
                Application.Run(form);
            }
        }

        private static void CaptureSettings(UiState uiState)
        {
            ConfigState state = ConfigStore.Defaults();
            state.Mode = "write";
            state.MusicLibrary = @"\\server\share\music";
            state.IgnoredSubs.AddRange(new[]
            {
                "[Octo-Fiesta]", "[Artist Singles]", "[no artist]", "[videos]", "[original]",
                "#SyncVersion", "@eaDir", ".stfolder-*"
            });
            state.SourcePolicies["discogs"] = new SourcePolicyState
            {
                SourceOverride = true,
                MinimumRangeType = "Ideal",
                AllowBelowMinimumFallback = true
            };

            using (SetupForm form = new SetupForm(state, false, uiState))
            {
                form.Size = new Size(1500, 980);
                form.ShowInTaskbar = false;
                form.StartPosition = FormStartPosition.Manual;
                form.Location = new Point(-32000, -32000);
                form.Shown += delegate
                {
                    form.BeginInvoke((MethodInvoker)delegate
                    {
                        TabControl primary = (TabControl)typeof(SetupForm).GetField("primaryTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                        TabControl advanced = (TabControl)typeof(SetupForm).GetField("advancedTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                        primary.SelectedIndex = 1;
                        string[] names = { "paths", "artwork", "sources-formats" };
                        for (int index = 0; index < advanced.TabPages.Count; index++)
                        {
                            advanced.SelectedIndex = index;
                            form.PerformLayout();
                            Application.DoEvents();
                            CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "settings-" + names[index] + "-qa.png"));
                        }
                        advanced.SelectedIndex = 0;
                        Application.DoEvents();
                        Control runtime = form.Controls.Find("runtimeSettingsGroup", true)[0];
                        ScrollableControl pathScroll = runtime.Parent == null ? null : runtime.Parent.Parent as ScrollableControl;
                        if (pathScroll != null)
                        {
                            pathScroll.AutoScrollPosition = new Point(0, Math.Max(0, runtime.Top - 8));
                            form.PerformLayout();
                            Application.DoEvents();
                            CaptureWindow(form, Path.Combine(ConfigStore.AppRoot, "settings-runtime-qa.png"));
                        }
                        using (Bitmap icon = form.Icon.ToBitmap())
                            icon.Save(Path.Combine(ConfigStore.AppRoot, "app-icon-qa.png"), ImageFormat.Png);
                        form.Close();
                    });
                };
                Application.Run(form);
            }
        }

        private static void CaptureAppearanceMenu(MainForm form, string path)
        {
            ToolStripMenuItem view = null;
            ToolStripMenuItem appearance = null;
            foreach (ToolStripItem item in form.MainMenuStrip.Items)
            {
                ToolStripMenuItem menu = item as ToolStripMenuItem;
                if (menu != null && String.Equals(menu.Text, "View", StringComparison.Ordinal))
                {
                    view = menu;
                    break;
                }
            }
            if (view != null)
            {
                foreach (ToolStripItem item in view.DropDownItems)
                {
                    ToolStripMenuItem menu = item as ToolStripMenuItem;
                    if (menu != null && String.Equals(menu.Text, "Appearance", StringComparison.Ordinal))
                    {
                        appearance = menu;
                        break;
                    }
                }
            }
            if (appearance == null) throw new InvalidOperationException("View / Appearance menu was not found for visual QA.");
            view.ShowDropDown();
            appearance.ShowDropDown();
            Application.DoEvents();
            ToolStripDropDown dropDown = appearance.DropDown;
            using (Bitmap screenshot = new Bitmap(Math.Max(1, dropDown.Width), Math.Max(1, dropDown.Height)))
            {
                dropDown.DrawToBitmap(screenshot, new Rectangle(Point.Empty, screenshot.Size));
                screenshot.Save(path, ImageFormat.Png);
            }
            appearance.HideDropDown();
            view.HideDropDown();
        }

        private static void CaptureWindow(Form form, string path)
        {
            form.Refresh();
            Application.DoEvents();
            using (Bitmap screenshot = new Bitmap(Math.Max(1, form.Width), Math.Max(1, form.Height)))
            using (Graphics graphics = Graphics.FromImage(screenshot))
            {
                IntPtr hdc = graphics.GetHdc();
                bool printed;
                try { printed = PrintWindow(form.Handle, hdc, 2); }
                finally { graphics.ReleaseHdc(hdc); }
                if (!printed)
                {
                    graphics.Clear(ThemeManager.CurrentPalette.WindowBackground);
                    form.DrawToBitmap(screenshot, new Rectangle(Point.Empty, form.ClientSize));
                }
                else
                {
                    // PrintWindow preserves the native themed title bar and RichTextBox text,
                    // while DrawToBitmap preserves owner-painted cards and TreeView rows more
                    // accurately. Merge those real control renders for deterministic QA output.
                    using (Bitmap printedImage = (Bitmap)screenshot.Clone())
                    using (Bitmap ownerPaintedImage = new Bitmap(Math.Max(1, form.Width), Math.Max(1, form.Height)))
                    {
                        form.DrawToBitmap(ownerPaintedImage, new Rectangle(Point.Empty, ownerPaintedImage.Size));
                        Point clientScreen = form.PointToScreen(Point.Empty);
                        Rectangle clientBounds = new Rectangle(clientScreen.X - form.Left, clientScreen.Y - form.Top,
                            form.ClientSize.Width, form.ClientSize.Height);
                        graphics.DrawImage(ownerPaintedImage, clientBounds, clientBounds, GraphicsUnit.Pixel);
                        foreach (Control activityCard in form.Controls.Find("activityLogCard", true))
                        {
                            Point screenPoint = activityCard.PointToScreen(Point.Empty);
                            Rectangle logBounds = new Rectangle(screenPoint.X - form.Left, screenPoint.Y - form.Top,
                                activityCard.Width, activityCard.Height);
                            logBounds.Inflate(-6, -6);
                            graphics.DrawImage(printedImage, logBounds, logBounds, GraphicsUnit.Pixel);
                        }
                    }
                }
                screenshot.Save(path, ImageFormat.Png);
            }
        }

        [DllImport("user32.dll")]
        private static extern bool PrintWindow(IntPtr window, IntPtr target, uint flags);
    }
}
