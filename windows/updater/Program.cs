using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Splined.WindowsUpdater
{
    internal sealed class UpdateManifest
    {
        public int schema { get; set; }
        public string channel { get; set; }
        public string version { get; set; }
        public string commit { get; set; }
        public string short_commit { get; set; }
        public string release_url { get; set; }
        public string archive_url { get; set; }
        public string archive_sha256 { get; set; }
        public long archive_size { get; set; }
        public string updater_url { get; set; }
        public string updater_sha256 { get; set; }
        public long updater_size { get; set; }
        public string gui_sha256 { get; set; }
        public long gui_size { get; set; }
        public string core_sha256 { get; set; }
        public long core_size { get; set; }
    }

    internal sealed class UpdateOptions
    {
        public string AppRoot;
        public string ArchivePath;
        public string ManifestPath;
        public string UpdateDirectory;
        public int GuiProcessId;
        public int CoreProcessId;
        public bool SelfTest;
        public bool FailAfterCoreReplacement;
    }

    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            if (args != null && args.Length == 1 && args[0] == "--self-test")
                return UpdateInstaller.RunSelfTest();
            try
            {
                UpdateOptions options = ParseOptions(args);
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new UpdateForm(options));
                return 0;
            }
            catch (Exception error)
            {
                MessageBox.Show(
                    error.Message,
                    "S:P:L:I:N:E:D Update",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
                return 1;
            }
        }

        private static UpdateOptions ParseOptions(string[] args)
        {
            if (args == null || args.Length != 13 || args[0] != "--apply")
                throw new InvalidOperationException("The SPLINED update helper received invalid arguments.");
            Dictionary<string, string> values = new Dictionary<string, string>(StringComparer.Ordinal);
            for (int index = 1; index + 1 < args.Length; index += 2)
                values[args[index]] = args[index + 1];
            int guiProcessId;
            int coreProcessId;
            if (!Int32.TryParse(Value(values, "--wait-gui-pid"), out guiProcessId) || guiProcessId <= 0
                || !Int32.TryParse(Value(values, "--wait-core-pid"), out coreProcessId) || coreProcessId < 0)
                throw new InvalidOperationException("The SPLINED update helper received invalid process identities.");
            return new UpdateOptions
            {
                AppRoot = Value(values, "--app-root"),
                ArchivePath = Value(values, "--archive"),
                ManifestPath = Value(values, "--manifest"),
                UpdateDirectory = Value(values, "--update-dir"),
                GuiProcessId = guiProcessId,
                CoreProcessId = coreProcessId
            };
        }

        private static string Value(Dictionary<string, string> values, string key)
        {
            string value;
            if (!values.TryGetValue(key, out value) || String.IsNullOrWhiteSpace(value))
                throw new InvalidOperationException("The SPLINED update helper is missing " + key + ".");
            return value;
        }
    }

    internal sealed class UpdateForm : Form
    {
        private readonly UpdateOptions options;
        private readonly Label status;
        private readonly ProgressBar progress;

        public UpdateForm(UpdateOptions options)
        {
            this.options = options;
            Text = "S:P:L:I:N:E:D Update";
            StartPosition = FormStartPosition.CenterScreen;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = false;
            MinimizeBox = false;
            ShowInTaskbar = true;
            ClientSize = new Size(480, 118);
            status = new Label
            {
                Text = "Waiting for SPLINED to close...",
                AutoSize = false,
                Location = new Point(18, 18),
                Size = new Size(444, 42)
            };
            progress = new ProgressBar
            {
                Location = new Point(18, 72),
                Size = new Size(444, 22),
                Style = ProgressBarStyle.Marquee
            };
            Controls.Add(status);
            Controls.Add(progress);
            Shown += ApplyUpdate;
        }

        private async void ApplyUpdate(object sender, EventArgs e)
        {
            try
            {
                string stageDirectory = await Task.Run(delegate
                {
                    UpdateInstaller installer = new UpdateInstaller(options);
                    return installer.Apply(delegate(string message)
                    {
                        if (!IsDisposed) BeginInvoke((Action)delegate { status.Text = message; });
                    });
                });
                status.Text = "Update verified. Restarting SPLINED...";
                ProcessStartInfo start = new ProcessStartInfo
                {
                    FileName = Path.Combine(Path.GetFullPath(options.AppRoot), "splined.exe"),
                    WorkingDirectory = Path.GetFullPath(options.AppRoot),
                    UseShellExecute = false,
                    Arguments = "--cleanup-update " + QuoteArgument(Path.GetFullPath(options.UpdateDirectory))
                        + " --update-stage " + QuoteArgument(stageDirectory)
                        + " --wait-update-pid " + Process.GetCurrentProcess().Id
                };
                Process.Start(start);
                Close();
            }
            catch (Exception error)
            {
                progress.Style = ProgressBarStyle.Blocks;
                progress.Value = 0;
                status.Text = "The update was not installed.";
                MessageBox.Show(this,
                    error.Message + "\r\n\r\nExisting SPLINED data was not changed. Recovery files are retained if rollback was required.",
                    "S:P:L:I:N:E:D Update",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
                Close();
            }
        }

        private static string QuoteArgument(string value)
        {
            if ((value ?? "").IndexOf('"') >= 0)
                throw new InvalidOperationException("Update paths containing quotes are not supported.");
            return "\"" + (value ?? "") + "\"";
        }
    }

    internal sealed class UpdateInstaller
    {
        private const string GuiRelativePath = "splined.exe";
        private const string CoreRelativePath = "runtime\\splined-core.exe";
        private const string UpdaterFileName = "splined-update.exe";
        private static readonly Regex CommitPattern = new Regex("^[0-9a-fA-F]{40}$", RegexOptions.Compiled);
        private static readonly Regex ShaPattern = new Regex("^[0-9a-fA-F]{64}$", RegexOptions.Compiled);
        private readonly UpdateOptions options;

        public UpdateInstaller(UpdateOptions options)
        {
            this.options = options;
        }

        public string Apply(Action<string> report)
        {
            string appRoot = Path.GetFullPath(options.AppRoot).TrimEnd(Path.DirectorySeparatorChar);
            string updateDirectory = Path.GetFullPath(options.UpdateDirectory).TrimEnd(Path.DirectorySeparatorChar);
            string archivePath = Path.GetFullPath(options.ArchivePath);
            string manifestPath = Path.GetFullPath(options.ManifestPath);
            ValidatePathInside(archivePath, updateDirectory, "release archive");
            ValidatePathInside(manifestPath, updateDirectory, "update manifest");
            if (!options.SelfTest)
            {
                ValidatePathInside(Path.GetFullPath(Application.ExecutablePath), updateDirectory, "update helper");
                if (!String.Equals(Path.GetFileName(Application.ExecutablePath), UpdaterFileName, StringComparison.Ordinal))
                    throw new InvalidOperationException("The SPLINED update helper does not have its required stable filename.");
            }

            UpdateManifest manifest = new JavaScriptSerializer().Deserialize<UpdateManifest>(
                File.ReadAllText(manifestPath, Encoding.UTF8));
            ValidateManifest(manifest);
            VerifyFile(archivePath, manifest.archive_size, manifest.archive_sha256, "Windows release archive");
            VerifyFile(Application.ExecutablePath, manifest.updater_size, manifest.updater_sha256, "Windows update helper");

            string guiTarget = Path.Combine(appRoot, GuiRelativePath);
            string coreTarget = Path.Combine(appRoot, CoreRelativePath);
            if (!File.Exists(guiTarget) || !File.Exists(coreTarget))
                throw new InvalidOperationException("The installed SPLINED GUI/core pair is incomplete. No files were replaced.");

            report("Waiting for the exact SPLINED GUI and core processes to exit...");
            WaitForProcess(options.CoreProcessId, "processing core");
            WaitForProcess(options.GuiProcessId, "GUI");

            string stageDirectory = Path.Combine(appRoot, ".splined-update");
            if (Directory.Exists(stageDirectory))
                throw new InvalidOperationException("A previous .splined-update recovery directory exists. Preserve it and resolve that update before retrying.");
            string newDirectory = Path.Combine(stageDirectory, "new");
            string backupDirectory = Path.Combine(stageDirectory, "backup");
            Directory.CreateDirectory(newDirectory);
            Directory.CreateDirectory(backupDirectory);

            try
            {
                report("Extracting and verifying the approved release package...");
                ExtractApprovedFiles(archivePath, newDirectory);
                string newGui = Path.Combine(newDirectory, GuiRelativePath);
                string newCore = Path.Combine(newDirectory, CoreRelativePath);
                VerifyFile(newGui, manifest.gui_size, manifest.gui_sha256, "Staged SPLINED GUI");
                VerifyFile(newCore, manifest.core_size, manifest.core_sha256, "Staged SPLINED core");

                string backupGui = Path.Combine(backupDirectory, GuiRelativePath);
                string backupCore = Path.Combine(backupDirectory, CoreRelativePath);
                Directory.CreateDirectory(Path.GetDirectoryName(backupCore));
                File.Copy(guiTarget, backupGui, false);
                File.Copy(coreTarget, backupCore, false);

                report("Replacing the verified GUI/core pair with rollback protection...");
                try
                {
                    ReplaceFromStage(newCore, coreTarget);
                    if (options.FailAfterCoreReplacement)
                        throw new InvalidOperationException("Injected updater self-test replacement failure.");
                    ReplaceFromStage(newGui, guiTarget);
                    VerifyFile(guiTarget, manifest.gui_size, manifest.gui_sha256, "Installed SPLINED GUI");
                    VerifyFile(coreTarget, manifest.core_size, manifest.core_sha256, "Installed SPLINED core");
                }
                catch (Exception replacementError)
                {
                    Exception rollbackError = RollBack(guiTarget, coreTarget, backupGui, backupCore);
                    if (rollbackError != null)
                        throw new InvalidOperationException(
                            "SPLINED replacement failed and rollback also failed. Recovery copies remain in "
                            + backupDirectory + ". Replacement error: " + replacementError.Message
                            + " Rollback error: " + rollbackError.Message,
                            replacementError);
                    throw new InvalidOperationException(
                        "SPLINED replacement failed. The previous GUI/core pair was restored. " + replacementError.Message,
                        replacementError);
                }
                return stageDirectory;
            }
            catch
            {
                throw;
            }
        }

        private static void WaitForProcess(int processId, string role)
        {
            if (processId == 0) return;
            try
            {
                using (Process process = Process.GetProcessById(processId))
                {
                    if (!process.WaitForExit(60000))
                        throw new InvalidOperationException("Timed out waiting for the exact SPLINED " + role + " process (PID " + processId + ") to exit.");
                }
            }
            catch (ArgumentException) { }
        }

        private static void ExtractApprovedFiles(string archivePath, string destinationRoot)
        {
            HashSet<string> allowed = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
            {
                GuiRelativePath,
                CoreRelativePath,
                "README-WINDOWS.txt"
            };
            HashSet<string> extracted = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            using (ZipArchive archive = ZipFile.OpenRead(archivePath))
            {
                foreach (ZipArchiveEntry entry in archive.Entries)
                {
                    if (String.IsNullOrEmpty(entry.Name)) continue;
                    string relative = entry.FullName.Replace('/', '\\');
                    if (!allowed.Contains(relative) || relative.Contains("..") || Path.IsPathRooted(relative))
                        throw new InvalidOperationException("The release archive contains an unexpected file: " + entry.FullName);
                    if (!extracted.Add(relative))
                        throw new InvalidOperationException("The release archive contains a duplicate file: " + entry.FullName);
                    string destination = Path.Combine(destinationRoot, relative);
                    Directory.CreateDirectory(Path.GetDirectoryName(destination));
                    using (Stream source = entry.Open())
                    using (FileStream target = new FileStream(destination, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                        source.CopyTo(target);
                }
            }
            if (!extracted.Contains(GuiRelativePath) || !extracted.Contains(CoreRelativePath))
                throw new InvalidOperationException("The release archive does not contain the required SPLINED GUI/core pair.");
        }

        private static void ReplaceFromStage(string source, string destination)
        {
            string displaced = destination + ".splined-previous";
            if (File.Exists(displaced))
                throw new InvalidOperationException("A previous replacement file already exists: " + displaced);
            File.Move(destination, displaced);
            try
            {
                File.Move(source, destination);
                File.Delete(displaced);
            }
            catch
            {
                if (File.Exists(destination)) File.Delete(destination);
                if (File.Exists(displaced)) File.Move(displaced, destination);
                throw;
            }
        }

        private static Exception RollBack(string guiTarget, string coreTarget, string backupGui, string backupCore)
        {
            try
            {
                Restore(guiTarget, backupGui);
                Restore(coreTarget, backupCore);
                return null;
            }
            catch (Exception error) { return error; }
        }

        private static void Restore(string target, string backup)
        {
            if (!File.Exists(backup))
                throw new FileNotFoundException("The rollback copy is unavailable.", backup);
            if (File.Exists(target)) File.Delete(target);
            Directory.CreateDirectory(Path.GetDirectoryName(target));
            File.Copy(backup, target, true);
        }

        private static void ValidateManifest(UpdateManifest manifest)
        {
            if (manifest == null || manifest.schema != 2
                || !String.Equals(manifest.channel, "stable", StringComparison.OrdinalIgnoreCase)
                || String.IsNullOrWhiteSpace(manifest.version)
                || !CommitPattern.IsMatch(manifest.commit ?? "")
                || !String.Equals(manifest.short_commit, manifest.commit.Substring(0, 7), StringComparison.OrdinalIgnoreCase)
                || !IsOfficialReleaseUrl(manifest.release_url, manifest.version)
                || !IsOfficialAssetUrl(manifest.archive_url, manifest.version, "splined-windows-x86_64.zip")
                || !IsOfficialAssetUrl(manifest.updater_url, manifest.version, UpdaterFileName)
                || !ValidPayload(manifest.archive_sha256, manifest.archive_size)
                || !ValidPayload(manifest.updater_sha256, manifest.updater_size)
                || !ValidPayload(manifest.gui_sha256, manifest.gui_size)
                || !ValidPayload(manifest.core_sha256, manifest.core_size))
                throw new InvalidOperationException("The SPLINED automatic-update manifest is invalid.");
        }

        private static bool IsOfficialReleaseUrl(string value, string version)
        {
            Uri uri;
            return OfficialGitHubUri(value, out uri)
                && uri.AbsolutePath.Equals(
                    "/scottia/S-P-L-I-N-E-D/releases/tag/" + version,
                    StringComparison.Ordinal);
        }

        private static bool IsOfficialAssetUrl(string value, string version, string assetName)
        {
            Uri uri;
            return OfficialGitHubUri(value, out uri)
                && uri.AbsolutePath.Equals(
                    "/scottia/S-P-L-I-N-E-D/releases/download/" + version + "/" + assetName,
                    StringComparison.Ordinal);
        }

        private static bool OfficialGitHubUri(string value, out Uri uri)
        {
            return Uri.TryCreate(value, UriKind.Absolute, out uri)
                && uri.Scheme.Equals("https", StringComparison.OrdinalIgnoreCase)
                && uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
                && uri.IsDefaultPort
                && String.IsNullOrEmpty(uri.UserInfo)
                && String.IsNullOrEmpty(uri.Query)
                && String.IsNullOrEmpty(uri.Fragment);
        }

        private static bool ValidPayload(string sha, long size)
        {
            return ShaPattern.IsMatch(sha ?? "") && size > 0;
        }

        private static void VerifyFile(string path, long expectedSize, string expectedSha, string label)
        {
            FileInfo file = new FileInfo(path);
            if (!file.Exists || file.Length != expectedSize)
                throw new InvalidOperationException(label + " size does not match the update manifest.");
            string digest = FileSha256(path);
            if (!digest.Equals(expectedSha, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException(label + " failed SHA-256 verification.");
        }

        private static string FileSha256(string path)
        {
            using (SHA256 sha = SHA256.Create())
            using (FileStream stream = File.OpenRead(path))
                return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
        }

        private static void ValidatePathInside(string path, string parent, string label)
        {
            string normalizedPath = Path.GetFullPath(path);
            string normalizedParent = Path.GetFullPath(parent).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (!normalizedPath.StartsWith(normalizedParent, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The " + label + " is outside the approved update directory.");
        }

        internal static int RunSelfTest()
        {
            string root = Path.Combine(Path.GetTempPath(), "splined-updater-self-test-" + Guid.NewGuid().ToString("N"));
            try
            {
                string appRoot = Path.Combine(root, "app");
                string updateDirectory = Path.Combine(root, "update");
                Directory.CreateDirectory(Path.Combine(appRoot, "runtime"));
                Directory.CreateDirectory(updateDirectory);
                File.WriteAllText(Path.Combine(appRoot, GuiRelativePath), "old-gui");
                File.WriteAllText(Path.Combine(appRoot, CoreRelativePath), "old-core");
                string source = Path.Combine(root, "source");
                Directory.CreateDirectory(Path.Combine(source, "runtime"));
                File.WriteAllText(Path.Combine(source, GuiRelativePath), "new-gui");
                File.WriteAllText(Path.Combine(source, CoreRelativePath), "new-core");
                string archivePath = Path.Combine(updateDirectory, "splined-windows-x86_64.zip");
                ZipFile.CreateFromDirectory(source, archivePath, CompressionLevel.Optimal, false);
                UpdateManifest manifest = new UpdateManifest
                {
                    schema = 2,
                    channel = "stable",
                    version = "1.0.0-test",
                    commit = "0123456789abcdef0123456789abcdef01234567",
                    short_commit = "0123456",
                    release_url = "https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/1.0.0-test",
                    archive_url = "https://github.com/scottia/S-P-L-I-N-E-D/releases/download/1.0.0-test/splined-windows-x86_64.zip",
                    archive_sha256 = FileSha256(archivePath),
                    archive_size = new FileInfo(archivePath).Length,
                    updater_url = "https://github.com/scottia/S-P-L-I-N-E-D/releases/download/1.0.0-test/splined-update.exe",
                    updater_sha256 = FileSha256(Application.ExecutablePath),
                    updater_size = new FileInfo(Application.ExecutablePath).Length,
                    gui_sha256 = FileSha256(Path.Combine(source, GuiRelativePath)),
                    gui_size = new FileInfo(Path.Combine(source, GuiRelativePath)).Length,
                    core_sha256 = FileSha256(Path.Combine(source, CoreRelativePath)),
                    core_size = new FileInfo(Path.Combine(source, CoreRelativePath)).Length
                };
                string manifestPath = Path.Combine(updateDirectory, "windows-update.json");
                File.WriteAllText(manifestPath, new JavaScriptSerializer().Serialize(manifest), new UTF8Encoding(false));
                UpdateInstaller installer = new UpdateInstaller(new UpdateOptions
                {
                    AppRoot = appRoot,
                    ArchivePath = archivePath,
                    ManifestPath = manifestPath,
                    UpdateDirectory = updateDirectory,
                    GuiProcessId = 0,
                    CoreProcessId = 0,
                    SelfTest = true
                });
                string stage = installer.Apply(delegate { });
                bool valid = File.ReadAllText(Path.Combine(appRoot, GuiRelativePath)) == "new-gui"
                    && File.ReadAllText(Path.Combine(appRoot, CoreRelativePath)) == "new-core"
                    && File.Exists(Path.Combine(stage, "backup", GuiRelativePath))
                    && File.Exists(Path.Combine(stage, "backup", CoreRelativePath));
                if (!valid) throw new InvalidOperationException("Transactional replacement did not preserve the expected pair and rollback copies.");
                Directory.Delete(stage, true);
                File.WriteAllText(Path.Combine(appRoot, GuiRelativePath), "old-gui");
                File.WriteAllText(Path.Combine(appRoot, CoreRelativePath), "old-core");
                UpdateInstaller rollbackInstaller = new UpdateInstaller(new UpdateOptions
                {
                    AppRoot = appRoot,
                    ArchivePath = archivePath,
                    ManifestPath = manifestPath,
                    UpdateDirectory = updateDirectory,
                    GuiProcessId = 0,
                    CoreProcessId = 0,
                    SelfTest = true,
                    FailAfterCoreReplacement = true
                });
                bool failedAsExpected = false;
                try { rollbackInstaller.Apply(delegate { }); }
                catch (InvalidOperationException) { failedAsExpected = true; }
                valid = failedAsExpected
                    && File.ReadAllText(Path.Combine(appRoot, GuiRelativePath)) == "old-gui"
                    && File.ReadAllText(Path.Combine(appRoot, CoreRelativePath)) == "old-core";
                if (!valid) throw new InvalidOperationException("Injected paired-replacement failure did not restore the previous GUI/core pair.");
                Directory.Delete(Path.Combine(appRoot, ".splined-update"), true);
                Directory.CreateDirectory(Path.Combine(source, "_cache"));
                File.WriteAllText(Path.Combine(source, "_cache", "splined.db"), "must-not-stage");
                File.Delete(archivePath);
                ZipFile.CreateFromDirectory(source, archivePath, CompressionLevel.Optimal, false);
                manifest.archive_sha256 = FileSha256(archivePath);
                manifest.archive_size = new FileInfo(archivePath).Length;
                File.WriteAllText(manifestPath, new JavaScriptSerializer().Serialize(manifest), new UTF8Encoding(false));
                bool databaseRejected = false;
                try
                {
                    new UpdateInstaller(new UpdateOptions
                    {
                        AppRoot = appRoot,
                        ArchivePath = archivePath,
                        ManifestPath = manifestPath,
                        UpdateDirectory = updateDirectory,
                        GuiProcessId = 0,
                        CoreProcessId = 0,
                        SelfTest = true
                    }).Apply(delegate { });
                }
                catch (InvalidOperationException error)
                {
                    databaseRejected = error.Message.IndexOf("unexpected file", StringComparison.OrdinalIgnoreCase) >= 0;
                }
                valid = databaseRejected
                    && !File.Exists(Path.Combine(appRoot, ".splined-update", "new", "_cache", "splined.db"))
                    && !File.Exists(Path.Combine(appRoot, "_cache", "splined.db"))
                    && File.ReadAllText(Path.Combine(appRoot, GuiRelativePath)) == "old-gui"
                    && File.ReadAllText(Path.Combine(appRoot, CoreRelativePath)) == "old-core";
                if (!valid) throw new InvalidOperationException("The updater accepted or staged a database file from the release archive.");
                Console.WriteLine("PASS: updater verified staging, paired replacement, rollback, and executable-only targets.");
                return 0;
            }
            catch (Exception error)
            {
                Console.Error.WriteLine("FAIL: " + error);
                return 1;
            }
            finally
            {
                try { if (Directory.Exists(root)) Directory.Delete(root, true); }
                catch { }
            }
        }
    }
}
