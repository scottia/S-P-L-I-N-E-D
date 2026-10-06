using System;
using System.Diagnostics;
using System.Net;
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
        public string release_url { get; set; }
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
        public string html_url { get; set; }
        public GitHubReleaseAsset[] assets { get; set; }
    }

    internal sealed class WindowsUpdateLocation
    {
        public string ManifestUrl;
        public string ReleaseUrl;
        public string Tag;
    }

    internal static class WindowsUpdateService
    {
        private const long MaximumReleaseListBytes = 4L * 1024L * 1024L;
        private const string StableApiPath = "/repos/scottia/S-P-L-I-N-E-D/releases";
        private const string StableManifestName = "windows-update.json";
        private const string StableArchiveName = "splined-windows-x86_64.zip";
        private static readonly Regex CommitPattern = new Regex("^[0-9a-fA-F]{40}$", RegexOptions.Compiled);

        public static async Task<WindowsUpdateCheck> CheckAsync()
        {
            byte[] releases = await DownloadBytesAsync(
                ReleaseInfo.StableReleasesApiUrl,
                MaximumReleaseListBytes,
                IsApprovedStableReleasesApiUrl);
            string releasesJson = System.Text.Encoding.UTF8.GetString(releases).TrimStart('\uFEFF');
            WindowsUpdateLocation location = SelectStableUpdateLocation(releasesJson);
            byte[] bytes = await DownloadBytesAsync(
                location.ManifestUrl,
                1024L * 1024L,
                IsApprovedStableManifestUri);
            string json = System.Text.Encoding.UTF8.GetString(bytes).TrimStart('\uFEFF');
            WindowsUpdateManifest manifest = new JavaScriptSerializer().Deserialize<WindowsUpdateManifest>(json);
            ValidateManifest(manifest, location);
            bool currentKnown = CommitPattern.IsMatch(BuildInfo.Commit ?? "");
            bool available = !currentKnown
                || !String.Equals(BuildInfo.Commit, manifest.commit, StringComparison.OrdinalIgnoreCase);
            return new WindowsUpdateCheck { Available = available, Manifest = manifest };
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
            using (WebClient client = new WebClient())
            {
                client.Headers[HttpRequestHeader.UserAgent] = "SPLINED-Windows-GUI/" + ReleaseInfo.SemanticVersion;
                client.Headers[HttpRequestHeader.CacheControl] = "no-cache";
                byte[] bytes = await client.DownloadDataTaskAsync(uri);
                if (bytes.LongLength == 0 || bytes.LongLength > maximumBytes)
                    throw new InvalidOperationException("The SPLINED release metadata download has an invalid size.");
                return bytes;
            }
        }

        internal static void ValidateManifest(WindowsUpdateManifest manifest, WindowsUpdateLocation location)
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

        internal static bool IsApprovedStableManifestUrl(string value)
        {
            return IsApprovedDownloadUrl(value, IsApprovedStableManifestUri);
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
                    bool hasWindowsArchive = false;
                    foreach (GitHubReleaseAsset asset in release.assets)
                    {
                        if (asset == null) continue;
                        if (String.Equals(asset.name, StableManifestName, StringComparison.Ordinal)
                            && IsApprovedStableManifestUrl(asset.browser_download_url))
                            manifestUrl = asset.browser_download_url;
                        else if (String.Equals(asset.name, StableArchiveName, StringComparison.Ordinal))
                        {
                            Uri archiveUri;
                            hasWindowsArchive = Uri.TryCreate(asset.browser_download_url, UriKind.Absolute, out archiveUri)
                                && IsApprovedStableAssetUri(archiveUri, StableArchiveName);
                        }
                    }
                    if (manifestUrl != null && hasWindowsArchive
                        && AssetMatchesTag(manifestUrl, release.tag_name)
                        && release.html_url.EndsWith("/" + release.tag_name, StringComparison.Ordinal))
                        return new WindowsUpdateLocation
                        {
                            ManifestUrl = manifestUrl,
                            ReleaseUrl = release.html_url,
                            Tag = release.tag_name
                        };
                }
            }
            throw new InvalidOperationException("No official SPLINED release contains the Windows portable archive and update notification metadata.");
        }

        private static Uri ValidateDownloadUri(string value, Func<Uri, bool> approval)
        {
            Uri uri;
            if (!Uri.TryCreate(value, UriKind.Absolute, out uri) || !approval(uri))
                throw new InvalidOperationException("The Windows update metadata URL is not an approved SPLINED release asset.");
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

        private static bool AssetMatchesTag(string assetUrl, string tag)
        {
            Uri asset = new Uri(assetUrl);
            string[] segments = asset.AbsolutePath.Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
            return segments.Length == 6 && segments[4].Equals(tag ?? "", StringComparison.Ordinal);
        }
    }
}
