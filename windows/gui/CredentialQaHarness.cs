using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Reflection;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class CredentialQaHarness
    {
        [STAThread]
        private static int Main()
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            string output = Path.Combine(ConfigStore.AppRoot, "credential-qa");
            Directory.CreateDirectory(output);
            UiState ui = ConfigStore.LoadUi();
            ui.Theme = "Dark";
            ui.ShowStatusOnLaunch = true;
            ConfigStore.SaveUi(ui);

            ConfigState state = ConfigStore.Defaults();
            state.CredentialDir = Path.Combine(output, "credentials");

            CredentialStore.Save(state, "lastfm", new Dictionary<string, object> { { "api_key", "qa-key" } });
            Dictionary<string, object> fanart = new Dictionary<string, object>
            {
                { "api_key", "qa-key" }, { "client_key", "" }, { "api_version", "v3.2" }
            };
            CredentialStore.ApplyValidationMetadata(fanart, new CredentialValidationResult
            {
                Success = true, Endpoint = "Fanart.tv v3.2 album endpoint", ApiVersion = "v3.2"
            });
            CredentialStore.Save(state, "fanarttv", fanart);
            CredentialStore.Save(state, "discogs", new Dictionary<string, object> { { "token", "qa-token" } });
            CredentialStore.Save(state, "musicbrainz", new Dictionary<string, object>
            {
                { "oauth_enabled", true }, { "client_id", "qa-client" }, { "client_secret", "qa-secret" },
                { "callback_uri", "urn:ietf:wg:oauth:2.0:oob" }, { "oauth_scope", "profile" },
                { "access_token", "qa-access" }, { "refresh_token", "qa-refresh" },
                { "options", new Dictionary<string, object>
                    {
                        { "retry_max", 4 }, { "min_delay", 1.05 }, { "recording_timeout", 7 }
                    }
                }
            });
            state.SourcePolicies["musicbrainz"] = new SourcePolicyState { Enabled = true, SourceOverride = true };

            Render(new CredentialsForm(state), Path.Combine(output, "credentials-status-qa.png"));
            Render(new StatusForm(state), Path.Combine(output, "application-status-qa.png"));
            Render(new CredentialEditForm(state, "fanarttv", "Fanart.tv"), Path.Combine(output, "fanart-v32-qa.png"));
            Render(new CredentialEditForm(state, "musicbrainz", "MusicBrainz OAuth"), Path.Combine(output, "musicbrainz-oauth-qa.png"));
            SetupForm musicBrainzSettings = new SetupForm(state, false);
            SelectMusicBrainzSource(musicBrainzSettings);
            Render(musicBrainzSettings, Path.Combine(output, "musicbrainz-source-policy-qa.png"));
            SetupForm sourcePrioritySettings = new SetupForm(state, false);
            SelectArtworkSourcePriority(sourcePrioritySettings);
            Render(sourcePrioritySettings, Path.Combine(output, "artwork-source-priority-qa.png"));
            Console.WriteLine("PASS: rendered credential and provider-status QA windows to " + output);
            return 0;
        }

        private static void SelectMusicBrainzSource(SetupForm form)
        {
            TabControl primary = (TabControl)typeof(SetupForm).GetField("primaryTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            TabControl advanced = (TabControl)typeof(SetupForm).GetField("advancedTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            ComboBox selector = (ComboBox)typeof(SetupForm).GetField("sourceSelector", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            primary.SelectedIndex = 1;
            for (int index = 0; index < advanced.TabPages.Count; index++)
                if (advanced.TabPages[index].Text == "Sources & Matching") advanced.SelectedIndex = index;
            for (int index = 0; index < selector.Items.Count; index++)
                if (Convert.ToString(selector.Items[index]) == "MusicBrainz") selector.SelectedIndex = index;
        }

        private static void SelectArtworkSourcePriority(SetupForm form)
        {
            TabControl primary = (TabControl)typeof(SetupForm).GetField("primaryTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            TabControl advanced = (TabControl)typeof(SetupForm).GetField("advancedTabs", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
            primary.SelectedIndex = 1;
            for (int index = 0; index < advanced.TabPages.Count; index++)
                if (advanced.TabPages[index].Text == "Sources & Matching") advanced.SelectedIndex = index;
            Control group = form.Controls.Find("artworkSourcePriorityGroup", true)[0];
            Panel scroll = group.Parent.Parent as Panel;
            if (scroll != null) scroll.ScrollControlIntoView(group);
        }

        private static void Render(Form form, string path)
        {
            using (form)
            {
                form.ShowInTaskbar = false;
                form.StartPosition = FormStartPosition.Manual;
                form.Location = new Point(-32000, -32000);
                form.Show();
                Application.DoEvents();
                form.PerformLayout();
                using (Bitmap bitmap = new Bitmap(form.Width, form.Height))
                {
                    form.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size));
                    bitmap.Save(path, ImageFormat.Png);
                }
                form.Hide();
            }
        }
    }
}
