using System;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Security.Cryptography;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using System.Web.Script.Serialization;

namespace Splined.WindowsGui
{
    internal sealed class WindowsUpdateManifest
    {
        public int schema { get; set; }
        public string channel { get; set; }
        public string commit { get; set; }
        public string short_commit { get; set; }
        public string published_at { get; set; }
        public string asset_url { get; set; }
        public string sha256 { get; set; }
        public long size { get; set; }
    }

    internal sealed class WindowsUpdateCheck
    {
        public bool Available;
        public WindowsUpdateManifest Manifest;
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
        public GitHubReleaseAsset[] assets { get; set; }
    }

    internal sealed class WindowsUpdateLocation
    {
        public string ManifestUrl;
        public string UpdaterUrl;
    }

    internal static class WindowsUpdateService
    {
        private const long MaximumUpdaterBytes = 200L * 1024L * 1024L;
        private const long MaximumReleaseListBytes = 4L * 1024L * 1024L;
        private const string ManifestPath = "/scottia/S-P-L-I-N-E-D/releases/download/windows-dev/windows-dev-update.json";
        private const string UpdaterPath = "/scottia/S-P-L-I-N-E-D/releases/download/windows-dev/setup-splined.exe";
        private const string StableApiPath = "/repos/scottia/S-P-L-I-N-E-D/releases";
        private const string StableManifestName = "windows-update.json";
        private const string UpdaterName = "setup-splined.exe";
        private static readonly Regex CommitPattern = new Regex("^[0-9a-fA-F]{40}$", RegexOptions.Compiled);
        private static readonly Regex ShaPattern = new Regex("^[0-9a-fA-F]{64}$", RegexOptions.Compiled);

        public static bool UsesDevChannel
        {
            get { return String.Equals(BuildInfo.UpdateChannel, "dev", StringComparison.OrdinalIgnoreCase); }
        }

        public static async Task<WindowsUpdateCheck> CheckAsync()
        {
            WindowsUpdateLocation location;
            if (UsesDevChannel)
            {
                location = new WindowsUpdateLocation
                {
                    ManifestUrl = ReleaseInfo.DevUpdateManifestUrl,
                    UpdaterUrl = ReleaseInfo.DevUpdateAssetUrl
                };
            }
            else
            {
                byte[] releases = await DownloadBytesAsync(
                    ReleaseInfo.StableReleasesApiUrl,
                    MaximumReleaseListBytes,
                    IsApprovedStableReleasesApiUrl);
                string releasesJson = System.Text.Encoding.UTF8.GetString(releases).TrimStart('\uFEFF');
                location = SelectStableUpdateLocation(releasesJson);
            }

            byte[] bytes = await DownloadBytesAsync(
                location.ManifestUrl,
                1024L * 1024L,
                UsesDevChannel ? (Func<Uri, bool>)IsApprovedDevManifestUri : IsApprovedStableManifestUri);
            string json = System.Text.Encoding.UTF8.GetString(bytes).TrimStart('\uFEFF');
            WindowsUpdateManifest manifest = new JavaScriptSerializer().Deserialize<WindowsUpdateManifest>(json);
            ValidateManifest(manifest, location.UpdaterUrl);
            bool currentKnown = CommitPattern.IsMatch(BuildInfo.Commit ?? "");
            bool available = !currentKnown
                || !String.Equals(BuildInfo.Commit, manifest.commit, StringComparison.OrdinalIgnoreCase);
            return new WindowsUpdateCheck { Available = available, Manifest = manifest };
        }

        public static async Task<string> DownloadAndStageAsync(WindowsUpdateManifest manifest)
        {
            ValidateManifest(manifest, manifest == null ? null : manifest.asset_url);
            byte[] bytes = await DownloadBytesAsync(
                AddCommitCacheBuster(manifest.asset_url, manifest.short_commit),
                MaximumUpdaterBytes,
                UsesDevChannel ? (Func<Uri, bool>)IsApprovedDevUpdaterUri : IsApprovedStableUpdaterUri);
            if (manifest.size > 0 && bytes.LongLength != manifest.size)
                throw new InvalidOperationException("The downloaded updater size does not match the published manifest.");
            string digest = Sha256(bytes);
            if (!digest.Equals(manifest.sha256, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The downloaded updater failed SHA-256 verification.");

            string updater = Path.Combine(ConfigStore.AppRoot, "setup-splined.exe");
            string staged = Path.Combine(ConfigStore.AppRoot, ".splined-update-" + manifest.short_commit + ".tmp");
            File.WriteAllBytes(staged, bytes);
            try
            {
                if (File.Exists(updater)) File.Delete(updater);
                File.Move(staged, updater);
            }
            catch
            {
                try { if (File.Exists(staged)) File.Delete(staged); }
                catch { }
                throw;
            }
            return updater;
        }

        public static void LaunchUpdater(string updater)
        {
            if (String.IsNullOrWhiteSpace(updater) || !File.Exists(updater))
                throw new FileNotFoundException("The verified SPLINED updater is unavailable.", updater);
            ProcessStartInfo start = new ProcessStartInfo
            {
                FileName = updater,
                WorkingDirectory = ConfigStore.AppRoot,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden
            };
            start.EnvironmentVariables["SPLINED_UPDATE_RELAUNCH"] = "1";
            Process.Start(start);
        }

        private static async Task<byte[]> DownloadBytesAsync(string url, long maximumBytes, Func<Uri, bool> approval)
        {
            Uri uri = ValidateDownloadUri(url, approval);
            ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12;
            using (WebClient client = new WebClient())
            {
                client.Headers[HttpRequestHeader.UserAgent] = "SPLINED-Windows-GUI/" + ReleaseInfo.SemanticVersion;
                client.Headers[HttpRequestHeader.CacheControl] = "no-cache";
                byte[] bytes = await client.DownloadDataTaskAsync(uri);
                if (bytes.LongLength == 0 || bytes.LongLength > maximumBytes)
                    throw new InvalidOperationException("The SPLINED update download has an invalid size.");
                return bytes;
            }
        }

        private static void ValidateManifest(WindowsUpdateManifest manifest, string expectedUpdaterUrl)
        {
            if (manifest == null || manifest.schema != 1)
                throw new InvalidOperationException("The Windows update manifest is missing or unsupported.");
            string expectedChannel = UsesDevChannel ? "dev" : "stable";
            if (!String.Equals(manifest.channel, expectedChannel, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update manifest is not for the installed channel.");
            if (!CommitPattern.IsMatch(manifest.commit ?? "") || !ShaPattern.IsMatch(manifest.sha256 ?? ""))
                throw new InvalidOperationException("The Windows update manifest contains invalid commit or checksum metadata.");
            string expectedShort = manifest.commit.Substring(0, 7);
            if (!String.Equals(expectedShort, manifest.short_commit, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update manifest commit identity is inconsistent.");
            if (manifest.size <= 0 || manifest.size > MaximumUpdaterBytes)
                throw new InvalidOperationException("The Windows update manifest contains an invalid updater size.");
            Func<Uri, bool> approval = UsesDevChannel
                ? (Func<Uri, bool>)IsApprovedDevUpdaterUri
                : IsApprovedStableUpdaterUri;
            ValidateDownloadUri(manifest.asset_url, approval);
            if (!String.Equals(manifest.asset_url, expectedUpdaterUrl, StringComparison.Ordinal))
                throw new InvalidOperationException("The Windows update manifest does not match its published release asset.");
        }

        internal static bool IsApprovedManifestUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedDevManifestUri);
        }

        internal static bool IsApprovedUpdaterUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedDevUpdaterUri);
        }

        internal static bool IsApprovedStableManifestUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedStableManifestUri);
        }

        internal static bool IsApprovedStableUpdaterUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedStableUpdaterUri);
        }

        internal static WindowsUpdateLocation SelectStableUpdateLocation(string json)
        {
            GitHubRelease[] releases = new JavaScriptSerializer().Deserialize<GitHubRelease[]>(json);
            if (releases != null)
            {
                foreach (GitHubRelease release in releases)
                {
                    if (release == null || release.draft || release.prerelease || release.assets == null)
                        continue;
                    string manifestUrl = null;
                    string updaterUrl = null;
                    foreach (GitHubReleaseAsset asset in release.assets)
                    {
                        if (asset == null) continue;
                        if (String.Equals(asset.name, StableManifestName, StringComparison.Ordinal)
                            && IsApprovedStableManifestUrl(asset.browser_download_url))
                            manifestUrl = asset.browser_download_url;
                        else if (String.Equals(asset.name, UpdaterName, StringComparison.Ordinal)
                            && IsApprovedStableUpdaterUrl(asset.browser_download_url))
                            updaterUrl = asset.browser_download_url;
                    }
                    if (manifestUrl != null && updaterUrl != null
                        && SameReleaseTag(manifestUrl, updaterUrl, release.tag_name))
                        return new WindowsUpdateLocation { ManifestUrl = manifestUrl, UpdaterUrl = updaterUrl };
                }
            }
            throw new InvalidOperationException("No official SPLINED release contains the Windows update assets.");
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
            return Uri.TryCreate(value, UriKind.Absolute, out uri)
                && approval(uri);
        }

        private static bool IsApprovedDevManifestUri(Uri uri)
        {
            return IsApprovedGitHubUri(uri) && uri.AbsolutePath.Equals(ManifestPath, StringComparison.Ordinal);
        }

        private static bool IsApprovedDevUpdaterUri(Uri uri)
        {
            return IsApprovedGitHubUri(uri) && uri.AbsolutePath.Equals(UpdaterPath, StringComparison.Ordinal);
        }

        private static bool IsApprovedStableManifestUri(Uri uri)
        {
            return IsApprovedStableAssetUri(uri, StableManifestName);
        }

        private static bool IsApprovedStableUpdaterUri(Uri uri)
        {
            return IsApprovedStableAssetUri(uri, UpdaterName);
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
                && !segments[4].Equals("windows-dev", StringComparison.OrdinalIgnoreCase)
                && segments[5].Equals(assetName, StringComparison.Ordinal);
        }

        private static bool IsApprovedGitHubUri(Uri uri)
        {
            return uri.Scheme.Equals("https", StringComparison.OrdinalIgnoreCase)
                && uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
                && uri.IsDefaultPort
                && String.IsNullOrEmpty(uri.UserInfo);
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

        private static bool SameReleaseTag(string manifestUrl, string updaterUrl, string tag)
        {
            Uri manifest = new Uri(manifestUrl);
            Uri updater = new Uri(updaterUrl);
            string[] manifestSegments = manifest.AbsolutePath.Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
            string[] updaterSegments = updater.AbsolutePath.Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
            return manifestSegments.Length == 6
                && updaterSegments.Length == 6
                && manifestSegments[4].Equals(updaterSegments[4], StringComparison.Ordinal)
                && manifestSegments[4].Equals(tag ?? "", StringComparison.Ordinal);
        }

        private static string AddCommitCacheBuster(string url, string shortCommit)
        {
            return url + (url.IndexOf('?') >= 0 ? "&" : "?") + "commit=" + Uri.EscapeDataString(shortCommit ?? "");
        }

        private static string Sha256(byte[] bytes)
        {
            using (SHA256 algorithm = SHA256.Create())
            {
                byte[] digest = algorithm.ComputeHash(bytes);
                return BitConverter.ToString(digest).Replace("-", "").ToLowerInvariant();
            }
        }
    }
}
