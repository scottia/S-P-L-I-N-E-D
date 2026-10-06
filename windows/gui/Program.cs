using System;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class Program
    {
        [STAThread]
        private static void Main(string[] args)
        {
            try
            {
                args = WindowsUpdateService.CleanupCompletedUpdate(args);
                UiState uiState = ConfigStore.LoadUi();
                ThemeManager.Initialize(uiState.Theme);
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                ConfigState state = ConfigStore.Load();
                string restorePath = null;
                if (args != null && args.Length == 1 && args[0].EndsWith(".spl", StringComparison.OrdinalIgnoreCase)) restorePath = args[0];
                else if (args != null && args.Length == 2 && args[0] == "--restore") restorePath = args[1];
                if (!String.IsNullOrWhiteSpace(restorePath))
                {
                    using (BackupForm restore = new BackupForm(state, uiState, true, restorePath))
                        if (restore.ShowDialog() != DialogResult.OK) return;
                    state = ConfigStore.Load();
                    uiState = ConfigStore.LoadUi();
                    ThemeManager.Initialize(uiState.Theme);
                }
                if (!ConfigStore.HasSavedSettings)
                {
                    using (SetupForm setup = new SetupForm(state, true, uiState))
                    {
                        if (setup.ShowDialog() != DialogResult.OK)
                            return;
                    }
                    state = ConfigStore.Load();
                    uiState = ConfigStore.LoadUi();
                    ThemeManager.Initialize(uiState.Theme);
                }
                BackupService.RegisterFileAssociation();
                Application.Run(new MainForm(state, uiState));
            }
            catch (Exception error)
            {
                MessageBox.Show(
                    error.Message,
                    ReleaseInfo.DisplayName,
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            }
        }
    }
}
