using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class Program
    {
        [STAThread]
        private static void Main(string[] args)
        {
            bool nativeSmoke = args != null && Array.Exists(args,
                value => String.Equals(value, "--native-smoke", StringComparison.Ordinal));
            bool packageIdentitySmoke = args != null && Array.Exists(args,
                value => String.Equals(value, "--package-identity-smoke", StringComparison.Ordinal));
            try
            {
                NativeCore.Initialize(ConfigStore.AppRoot, ConfigStore.RelativePathRoot);
                if (packageIdentitySmoke)
                {
                    if (!ConfigStore.IsPackaged)
                        throw new InvalidOperationException("SPLINED does not have Windows package identity.");
                    Directory.CreateDirectory(ConfigStore.RuntimeStateRoot);
                    File.WriteAllText(
                        Path.Combine(ConfigStore.RuntimeStateRoot, "package-identity-smoke.ok"),
                        "package identity and native core initialization passed",
                        new UTF8Encoding(false));
                    return;
                }
                if (nativeSmoke) return;
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
                Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
                ThreadExceptionEventHandler threadExceptionHandler = ReportUiThreadException;
                UnhandledExceptionEventHandler domainExceptionHandler = ReportDomainException;
                Application.ThreadException += threadExceptionHandler;
                AppDomain.CurrentDomain.UnhandledException += domainExceptionHandler;
                try
                {
                    Application.Run(new MainForm(state, uiState));
                }
                finally
                {
                    Application.ThreadException -= threadExceptionHandler;
                    AppDomain.CurrentDomain.UnhandledException -= domainExceptionHandler;
                }
            }
            catch (Exception error)
            {
                RuntimeLog.WriteException("windows.gui.startup_unhandled", error);
                if (nativeSmoke || packageIdentitySmoke)
                {
                    Environment.ExitCode = 1;
                    return;
                }
                MessageBox.Show(
                    error.Message,
                    ReleaseInfo.DisplayName,
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            }
        }

        private static void ReportUiThreadException(object sender, ThreadExceptionEventArgs args)
        {
            Exception error = args == null ? null : args.Exception;
            RuntimeLog.WriteException("windows.gui.thread_unhandled", error);
            string diagnosticPath = RuntimeLog.Path;
            string detail = error == null || String.IsNullOrWhiteSpace(error.Message)
                ? "No additional error message was provided."
                : error.Message;
            string diagnostic = String.IsNullOrWhiteSpace(diagnosticPath)
                ? "The configured run log was unavailable."
                : "Diagnostic details were written to:\r\n" + diagnosticPath;
            MessageBox.Show(
                "An unexpected interface error occurred. SPLINED can continue, but the current action may not have completed.\r\n\r\n"
                + detail + "\r\n\r\n" + diagnostic,
                ReleaseInfo.DisplayName,
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
        }

        private static void ReportDomainException(object sender, UnhandledExceptionEventArgs args)
        {
            RuntimeLog.WriteException("windows.gui.domain_unhandled", args == null ? null : args.ExceptionObject as Exception);
        }
    }
}
