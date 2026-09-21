using System;
using System.IO;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class Program
    {
        [STAThread]
        private static void Main()
        {
            try
            {
                UiState uiState = ConfigStore.LoadUi();
                ThemeManager.Initialize(uiState.Theme);
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                ConfigState state = ConfigStore.Load();
                if (!File.Exists(state.ConfigPath))
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
