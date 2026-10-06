using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;

namespace Splined.WindowsGui
{
    internal sealed class WindowsUpdateManifest
    {
        public int schema { get; set; }
        public string channel { get; set; }
        public string version { get; set; }
        public string commit { get; set; }
        public string short_commit { get; set; }
        public string published_at { get; set; }
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

    internal sealed class WindowsUpdateCheck
    {
        public bool Available;
        public WindowsUpdateManifest Manifest;
        public WindowsUpdateLocation Location;
        public bool AutomaticInstallAvailable;
    }

    internal sealed class WindowsUpdatePackage
    {
        public string WorkingDirectory;
        public string ArchivePath;
        public string UpdaterPath;
        public string ManifestPath;
    }

    internal sealed class GitHubReleaseAsset
    {
        public string name { get; set; }
        public string browser_download_url { get; set; }
    }

    internal sealed class GitHubRelease
    {
        public bool draft { get; set; }
        public bool prerelease { get; set; }
        public string tag_name { get; set; }
        public string html_url { get; set; }
        public GitHubReleaseAsset[] assets { get; set; }
    }

    internal sealed class WindowsUpdateLocation
    {
        public string ManifestUrl;
        public string ArchiveUrl;
        public string UpdaterUrl;
        public string ReleaseUrl;
        public string Tag;
    }

    internal static class WindowsUpdateService
    {
        private const long MaximumReleaseListBytes = 4L * 1024L * 1024L;
        private const long MaximumArchiveBytes = 500L * 1024L * 1024L;
        private const long MaximumUpdaterBytes = 25L * 1024L * 1024L;
        private const string StableApiPath = "/repos/scottia/S-P-L-I-N-E-D/releases";
        private const string StableManifestName = "windows-update.json";
        private const string StableArchiveName = "splined-windows-x86_64.zip";
        private const string StableUpdaterName = "splined-update.exe";
        private static readonly Regex CommitPattern = new Regex("^[0-9a-fA-F]{40}$", RegexOptions.Compiled);
        private static readonly Regex ShaPattern = new Regex("^[0-9a-fA-F]{64}$", RegexOptions.Compiled);

        public static async Task<WindowsUpdateCheck> CheckAsync()
        {
            byte[] releases = await DownloadBytesAsync(
                ReleaseInfo.StableReleasesApiUrl,
                MaximumReleaseListBytes,
                IsApprovedStableReleasesApiUrl);
            string releasesJson = Encoding.UTF8.GetString(releases).TrimStart('\uFEFF');
            WindowsUpdateLocation location = SelectStableUpdateLocation(releasesJson);
            byte[] bytes = await DownloadBytesAsync(
                location.ManifestUrl,
                1024L * 1024L,
                IsApprovedStableManifestUri);
            string json = Encoding.UTF8.GetString(bytes).TrimStart('\uFEFF');
            WindowsUpdateManifest manifest = new JavaScriptSerializer().Deserialize<WindowsUpdateManifest>(json);
            ValidateNotificationManifest(manifest, location);
            bool automaticInstallAvailable = IsAutomaticInstallManifest(manifest, location);
            bool currentKnown = CommitPattern.IsMatch(BuildInfo.Commit ?? "");
            bool available = !currentKnown
                || !String.Equals(BuildInfo.Commit, manifest.commit, StringComparison.OrdinalIgnoreCase);
            return new WindowsUpdateCheck
            {
                Available = available,
                Manifest = manifest,
                Location = location,
                AutomaticInstallAvailable = automaticInstallAvailable
            };
        }

        public static async Task<WindowsUpdatePackage> DownloadAndStageAsync(WindowsUpdateCheck update)
        {
            if (update == null) throw new ArgumentNullException("update");
            ValidateAutomaticManifest(update.Manifest, update.Location);

            string updatesRoot = UpdatesRoot();
            Directory.CreateDirectory(updatesRoot);
            string workingDirectory = Path.Combine(
                updatesRoot,
                update.Manifest.short_commit + "-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(workingDirectory);
            string archivePath = Path.Combine(workingDirectory, StableArchiveName);
            string updaterPath = Path.Combine(workingDirectory, StableUpdaterName);
            string manifestPath = Path.Combine(workingDirectory, StableManifestName);
            try
            {
                await DownloadFileAsync(
                    update.Location.ArchiveUrl,
                    archivePath,
                    MaximumArchiveBytes,
                    IsApprovedStableArchiveUri);
                VerifyFile(archivePath, update.Manifest.archive_size, update.Manifest.archive_sha256, "Windows release archive");

                await DownloadFileAsync(
                    update.Location.UpdaterUrl,
                    updaterPath,
                    MaximumUpdaterBytes,
                    IsApprovedStableUpdaterUri);
                VerifyFile(updaterPath, update.Manifest.updater_size, update.Manifest.updater_sha256, "Windows update helper");

                File.WriteAllText(
                    manifestPath,
                    new JavaScriptSerializer().Serialize(update.Manifest),
                    new UTF8Encoding(false));
                return new WindowsUpdatePackage
                {
                    WorkingDirectory = workingDirectory,
                    ArchivePath = archivePath,
                    UpdaterPath = updaterPath,
                    ManifestPath = manifestPath
                };
            }
            catch
            {
                TryDeleteDirectory(workingDirectory);
                throw;
            }
        }

        public static void LaunchUpdater(WindowsUpdatePackage package, int coreProcessId)
        {
            if (package == null || !File.Exists(package.UpdaterPath)
                || !File.Exists(package.ArchivePath) || !File.Exists(package.ManifestPath))
                throw new InvalidOperationException("The verified SPLINED update package is incomplete.");

            string applicationRoot = Path.GetFullPath(AppDomain.CurrentDomain.BaseDirectory);
            ProcessStartInfo start = new ProcessStartInfo
            {
                FileName = package.UpdaterPath,
                WorkingDirectory = package.WorkingDirectory,
                UseShellExecute = false,
                CreateNoWindow = false,
                WindowStyle = ProcessWindowStyle.Normal,
                Arguments = "--apply"
                    + " --app-root " + QuoteArgument(applicationRoot)
                    + " --archive " + QuoteArgument(package.ArchivePath)
                    + " --manifest " + QuoteArgument(package.ManifestPath)
                    + " --update-dir " + QuoteArgument(package.WorkingDirectory)
                    + " --wait-gui-pid " + Process.GetCurrentProcess().Id
                    + " --wait-core-pid " + Math.Max(0, coreProcessId)
            };
            Process.Start(start);
        }

        public static void DiscardStagedPackage(WindowsUpdatePackage package)
        {
            if (package == null || String.IsNullOrWhiteSpace(package.WorkingDirectory)) return;
            string directory = Path.GetFullPath(package.WorkingDirectory);
            if (IsDescendant(directory, UpdatesRoot()))
                DeleteDirectoryWithRetries(directory);
        }

        public static string[] CleanupCompletedUpdate(string[] args)
        {
            if (args == null || args.Length != 6
                || !String.Equals(args[0], "--cleanup-update", StringComparison.Ordinal)
                || !String.Equals(args[2], "--update-stage", StringComparison.Ordinal)
                || !String.Equals(args[4], "--wait-update-pid", StringComparison.Ordinal))
                return args ?? new string[0];

            int processId;
            if (!Int32.TryParse(args[5], out processId) || processId <= 0) return args;
            string updateDirectory;
            string stageDirectory;
            try
            {
                updateDirectory = Path.GetFullPath(args[1]);
                stageDirectory = Path.GetFullPath(args[3]);
            }
            catch { return args; }
            if (!IsDescendant(updateDirectory, UpdatesRoot()) || !IsApprovedStageDirectory(stageDirectory))
                return args;

            try
            {
                Process updater = Process.GetProcessById(processId);
                bool exited = updater.WaitForExit(15000);
                updater.Dispose();
                if (!exited)
                    throw new InvalidOperationException("The completed update helper is still running; cleanup files were retained.");
            }
            catch (ArgumentException) { }

            string manifestPath = Path.Combine(updateDirectory, StableManifestName);
            if (!File.Exists(manifestPath))
                throw new InvalidOperationException("The completed update manifest is unavailable; update files were retained for recovery.");
            WindowsUpdateManifest manifest = new JavaScriptSerializer().Deserialize<WindowsUpdateManifest>(
                File.ReadAllText(manifestPath, Encoding.UTF8));
            WindowsUpdateLocation location = LocationFromManifest(manifest);
            ValidateAutomaticManifest(manifest, location);
            ValidateInstalledPair(manifest);

            DeleteDirectoryWithRetries(stageDirectory);
            DeleteDirectoryWithRetries(updateDirectory);
            return new string[0];
        }

        private static void ValidateInstalledPair(WindowsUpdateManifest manifest)
        {
            if (manifest == null || manifest.schema != 2 || !CommitPattern.IsMatch(manifest.commit ?? "")
                || !String.Equals(BuildInfo.Commit, manifest.commit, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The restarted GUI does not match the completed update; recovery files were retained.");
            string root = Path.GetFullPath(AppDomain.CurrentDomain.BaseDirectory);
            VerifyFile(Path.Combine(root, "splined.exe"), manifest.gui_size, manifest.gui_sha256, "Installed SPLINED GUI");
            VerifyFile(Path.Combine(root, "runtime", "splined-core.exe"), manifest.core_size, manifest.core_sha256, "Installed SPLINED core");
        }

        public static void OpenReleasePage(string releaseUrl)
        {
            if (!IsApprovedReleasePageUrl(releaseUrl))
                throw new InvalidOperationException("The SPLINED release page URL is not approved.");
            Process.Start(new ProcessStartInfo(releaseUrl) { UseShellExecute = true });
        }

        private static async Task<byte[]> DownloadBytesAsync(string url, long maximumBytes, Func<Uri, bool> approval)
        {
            Uri uri = ValidateDownloadUri(url, approval);
            ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12;
            using (WebClient client = NewClient())
            {
                byte[] bytes = await client.DownloadDataTaskAsync(uri);
                if (bytes.LongLength == 0 || bytes.LongLength > maximumBytes)
                    throw new InvalidOperationException("The SPLINED release metadata download has an invalid size.");
                return bytes;
            }
        }

        private static async Task DownloadFileAsync(string url, string destination, long maximumBytes, Func<Uri, bool> approval)
        {
            Uri uri = ValidateDownloadUri(url, approval);
            ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12;
            using (WebClient client = NewClient())
                await client.DownloadFileTaskAsync(uri, destination);
            long length = new FileInfo(destination).Length;
            if (length <= 0 || length > maximumBytes)
                throw new InvalidOperationException("The SPLINED update download has an invalid size.");
        }

        private static WebClient NewClient()
        {
            WebClient client = new WebClient();
            client.Headers[HttpRequestHeader.UserAgent] = "SPLINED-Windows-GUI/" + ReleaseInfo.SemanticVersion;
            client.Headers[HttpRequestHeader.CacheControl] = "no-cache";
            return client;
        }

        private static void VerifyFile(string path, long expectedSize, string expectedSha, string label)
        {
            FileInfo file = new FileInfo(path);
            if (!file.Exists || file.Length != expectedSize)
                throw new InvalidOperationException(label + " size does not match the official manifest.");
            string digest;
            using (SHA256 sha = SHA256.Create())
            using (FileStream stream = File.OpenRead(path))
                digest = BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
            if (!digest.Equals(expectedSha, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException(label + " failed SHA-256 verification.");
        }

        internal static void ValidateNotificationManifest(WindowsUpdateManifest manifest, WindowsUpdateLocation location)
        {
            if (manifest == null || manifest.schema != 2)
                throw new InvalidOperationException("The Windows update notification manifest is missing or unsupported.");
            if (!String.Equals(manifest.channel, "stable", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update notification is not for the stable release channel.");
            if (!CommitPattern.IsMatch(manifest.commit ?? ""))
                throw new InvalidOperationException("The Windows update notification contains an invalid commit identity.");
            string expectedShort = manifest.commit.Substring(0, 7);
            if (!String.Equals(expectedShort, manifest.short_commit, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update notification commit identity is inconsistent.");
            if (location == null || !IsApprovedReleasePageUrl(manifest.release_url)
                || !String.Equals(manifest.release_url, location.ReleaseUrl, StringComparison.Ordinal)
                || !manifest.release_url.EndsWith("/" + location.Tag, StringComparison.Ordinal))
                throw new InvalidOperationException("The Windows update notification does not match its official GitHub release page.");
        }

        internal static void ValidateAutomaticManifest(WindowsUpdateManifest manifest, WindowsUpdateLocation location)
        {
            ValidateNotificationManifest(manifest, location);
            if (String.IsNullOrWhiteSpace(manifest.version)
                || !String.Equals(manifest.version, location.Tag, StringComparison.Ordinal))
                throw new InvalidOperationException("The Windows automatic-update version is missing or inconsistent.");
            if (String.IsNullOrWhiteSpace(location.ArchiveUrl)
                || String.IsNullOrWhiteSpace(location.UpdaterUrl)
                || !IsApprovedStableArchiveUrl(manifest.archive_url)
                || !IsApprovedStableUpdaterUrl(manifest.updater_url)
                || !String.Equals(manifest.archive_url, location.ArchiveUrl, StringComparison.Ordinal)
                || !String.Equals(manifest.updater_url, location.UpdaterUrl, StringComparison.Ordinal)
                || !AssetsMatchTag(location.Tag, manifest.archive_url, manifest.updater_url))
                throw new InvalidOperationException("The Windows automatic-update asset URLs are missing or inconsistent.");
            if (!ValidPayload(manifest.archive_sha256, manifest.archive_size, MaximumArchiveBytes)
                || !ValidPayload(manifest.updater_sha256, manifest.updater_size, MaximumUpdaterBytes)
                || !ValidPayload(manifest.gui_sha256, manifest.gui_size, MaximumArchiveBytes)
                || !ValidPayload(manifest.core_sha256, manifest.core_size, MaximumArchiveBytes))
                throw new InvalidOperationException("The Windows update contains invalid payload verification metadata.");
        }

        internal static bool IsAutomaticInstallManifest(WindowsUpdateManifest manifest, WindowsUpdateLocation location)
        {
            try
            {
                ValidateAutomaticManifest(manifest, location);
                return true;
            }
            catch (InvalidOperationException) { return false; }
        }

        private static bool ValidPayload(string sha, long size, long maximum)
        {
            return ShaPattern.IsMatch(sha ?? "") && size > 0 && size <= maximum;
        }

        internal static bool IsApprovedStableManifestUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedStableManifestUri);
        }

        internal static bool IsApprovedStableArchiveUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedStableArchiveUri);
        }

        internal static bool IsApprovedStableUpdaterUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedStableUpdaterUri);
        }

        internal static bool IsApprovedReleasePageUrl(string value)
        {
            Uri uri;
            if (!Uri.TryCreate(value, UriKind.Absolute, out uri) || !IsApprovedGitHubUri(uri)) return false;
            string[] segments = uri.AbsolutePath.Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
            return segments.Length == 5
                && segments[0].Equals("scottia", StringComparison.Ordinal)
                && segments[1].Equals("S-P-L-I-N-E-D", StringComparison.Ordinal)
                && segments[2].Equals("releases", StringComparison.Ordinal)
                && segments[3].Equals("tag", StringComparison.Ordinal)
                && !String.IsNullOrWhiteSpace(segments[4]);
        }

        internal static WindowsUpdateLocation SelectStableUpdateLocation(string json)
        {
            GitHubRelease[] releases = new JavaScriptSerializer().Deserialize<GitHubRelease[]>(json);
            if (releases != null)
            {
                foreach (GitHubRelease release in releases)
                {
                    if (release == null || release.draft || release.prerelease || release.assets == null
                        || !IsApprovedReleasePageUrl(release.html_url)) continue;
                    string manifestUrl = null;
                    string archiveUrl = null;
                    string updaterUrl = null;
                    foreach (GitHubReleaseAsset asset in release.assets)
                    {
                        if (asset == null) continue;
                        if (String.Equals(asset.name, StableManifestName, StringComparison.Ordinal)
                            && IsApprovedStableManifestUrl(asset.browser_download_url))
                            manifestUrl = asset.browser_download_url;
                        else if (String.Equals(asset.name, StableArchiveName, StringComparison.Ordinal)
                            && IsApprovedStableArchiveUrl(asset.browser_download_url))
                            archiveUrl = asset.browser_download_url;
                        else if (String.Equals(asset.name, StableUpdaterName, StringComparison.Ordinal)
                            && IsApprovedStableUpdaterUrl(asset.browser_download_url))
                            updaterUrl = asset.browser_download_url;
                    }
                    if (manifestUrl != null && archiveUrl != null
                        && AssetsMatchTag(release.tag_name, manifestUrl, archiveUrl)
                        && release.html_url.EndsWith("/" + release.tag_name, StringComparison.Ordinal))
                    {
                        if (updaterUrl != null && !AssetsMatchTag(release.tag_name, updaterUrl))
                            updaterUrl = null;
                        return new WindowsUpdateLocation
                        {
                            ManifestUrl = manifestUrl,
                            ArchiveUrl = archiveUrl,
                            UpdaterUrl = updaterUrl,
                            ReleaseUrl = release.html_url,
                            Tag = release.tag_name
                        };
                    }
                }
            }
            throw new InvalidOperationException("No official SPLINED release contains Windows notification metadata and the portable archive.");
        }

        private static WindowsUpdateLocation LocationFromManifest(WindowsUpdateManifest manifest)
        {
            return new WindowsUpdateLocation
            {
                ArchiveUrl = manifest == null ? null : manifest.archive_url,
                UpdaterUrl = manifest == null ? null : manifest.updater_url,
                ReleaseUrl = manifest == null ? null : manifest.release_url,
                Tag = manifest == null ? null : manifest.version
            };
        }

        private static Uri ValidateDownloadUri(string value, Func<Uri, bool> approval)
        {
            Uri uri;
            if (!Uri.TryCreate(value, UriKind.Absolute, out uri) || !approval(uri))
                throw new InvalidOperationException("The Windows update URL is not an approved SPLINED release asset.");
            return uri;
        }

        private static bool IsApprovedDownloadUrl(string value, Func<Uri, bool> approval)
        {
            Uri uri;
            return Uri.TryCreate(value, UriKind.Absolute, out uri) && approval(uri);
        }

        private static bool IsApprovedStableManifestUri(Uri uri)
        {
            return IsApprovedStableAssetUri(uri, StableManifestName);
        }

        private static bool IsApprovedStableArchiveUri(Uri uri)
        {
            return IsApprovedStableAssetUri(uri, StableArchiveName);
        }

        private static bool IsApprovedStableUpdaterUri(Uri uri)
        {
            return IsApprovedStableAssetUri(uri, StableUpdaterName);
        }

        private static bool IsApprovedStableAssetUri(Uri uri, string assetName)
        {
            if (!IsApprovedGitHubUri(uri)) return false;
            string[] segments = uri.AbsolutePath.Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
            return segments.Length == 6
                && segments[0].Equals("scottia", StringComparison.Ordinal)
                && segments[1].Equals("S-P-L-I-N-E-D", StringComparison.Ordinal)
                && segments[2].Equals("releases", StringComparison.Ordinal)
                && segments[3].Equals("download", StringComparison.Ordinal)
                && !String.IsNullOrWhiteSpace(segments[4])
                && segments[5].Equals(assetName, StringComparison.Ordinal);
        }

        private static bool IsApprovedGitHubUri(Uri uri)
        {
            return uri.Scheme.Equals("https", StringComparison.OrdinalIgnoreCase)
                && uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
                && uri.IsDefaultPort
                && String.IsNullOrEmpty(uri.UserInfo)
                && String.IsNullOrEmpty(uri.Query)
                && String.IsNullOrEmpty(uri.Fragment);
        }

        private static bool IsApprovedStableReleasesApiUrl(Uri uri)
        {
            return uri.Scheme.Equals("https", StringComparison.OrdinalIgnoreCase)
                && uri.Host.Equals("api.github.com", StringComparison.OrdinalIgnoreCase)
                && uri.IsDefaultPort
                && String.IsNullOrEmpty(uri.UserInfo)
                && uri.AbsolutePath.Equals(StableApiPath, StringComparison.Ordinal)
                && uri.Query.Equals("?per_page=20", StringComparison.Ordinal);
        }

        private static bool AssetsMatchTag(string tag, params string[] assetUrls)
        {
            return !String.IsNullOrWhiteSpace(tag) && assetUrls.All(value =>
            {
                Uri asset = new Uri(value);
                string[] segments = asset.AbsolutePath.Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
                return segments.Length == 6 && segments[4].Equals(tag, StringComparison.Ordinal);
            });
        }

        private static string UpdatesRoot()
        {
            return Path.GetFullPath(Path.Combine(Path.GetTempPath(), "SPLINED", "updates"));
        }

        private static bool IsApprovedStageDirectory(string stageDirectory)
        {
            string root = Path.GetFullPath(AppDomain.CurrentDomain.BaseDirectory);
            return IsDescendant(stageDirectory, root)
                && String.Equals(Path.GetFileName(stageDirectory), ".splined-update", StringComparison.OrdinalIgnoreCase);
        }

        private static bool IsDescendant(string path, string parent)
        {
            string normalizedPath = Path.GetFullPath(path).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            string normalizedParent = Path.GetFullPath(parent).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            return normalizedPath.StartsWith(normalizedParent, StringComparison.OrdinalIgnoreCase)
                && !normalizedPath.Equals(normalizedParent, StringComparison.OrdinalIgnoreCase);
        }

        private static void DeleteDirectoryWithRetries(string directory)
        {
            for (int attempt = 0; attempt < 20 && Directory.Exists(directory); attempt++)
            {
                TryDeleteDirectory(directory);
                if (Directory.Exists(directory)) Thread.Sleep(100);
            }
            if (Directory.Exists(directory))
                throw new InvalidOperationException("SPLINED verified the update but could not remove completed update files: " + directory);
        }

        private static void TryDeleteDirectory(string directory)
        {
            try { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
            catch { }
        }

        private static string QuoteArgument(string value)
        {
            string argument = value ?? "";
            if (argument.IndexOf('"') >= 0)
                throw new ArgumentException("Windows paths containing a quote character are not supported.", "value");
            return "\"" + argument + "\"";
        }
    }
}
