using System;
using System.Diagnostics;
using System.Linq;
using System.Net;
using System.Text;
using System.Text.RegularExpressions;
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
    }

    internal sealed class WindowsUpdateCheck
    {
        public bool Available;
        public WindowsUpdateManifest Manifest;
        public WindowsUpdateLocation Location;
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
        private const string StableManifestName = "windows-update.json";
        private static readonly Regex CommitPattern = new Regex("^[0-9a-fA-F]{40}$", RegexOptions.Compiled);

        public static async Task<WindowsUpdateCheck> CheckAsync()
        {
            byte[] releases = await DownloadBytesAsync(
                ReleaseInfo.StableReleasesApiUrl,
                MaximumReleaseListBytes,
                IsApprovedStableReleasesApiUrl);
            string releasesJson = Encoding.UTF8.GetString(releases).TrimStart('\uFEFF');
            WindowsUpdateLocation location = SelectStableUpdateLocation(releasesJson);
            byte[] bytes = await DownloadBytesAsync(location.ManifestUrl, 1024L * 1024L,
                IsApprovedStableManifestUri);
            string json = Encoding.UTF8.GetString(bytes).TrimStart('\uFEFF');
            WindowsUpdateManifest manifest = new JavaScriptSerializer().Deserialize<WindowsUpdateManifest>(json);
            ValidateNotificationManifest(manifest, location);
            bool currentKnown = CommitPattern.IsMatch(BuildInfo.Commit ?? "");
            return new WindowsUpdateCheck
            {
                Available = !currentKnown
                    || !String.Equals(BuildInfo.Commit, manifest.commit, StringComparison.OrdinalIgnoreCase),
                Manifest = manifest,
                Location = location
            };
        }

        public static void OpenReleasePage(string releaseUrl)
        {
            if (!IsApprovedReleasePageUrl(releaseUrl))
                throw new InvalidOperationException("The SPLINED release page URL is not approved.");
            Process.Start(new ProcessStartInfo(releaseUrl) { UseShellExecute = true });
        }

        internal static WindowsUpdateLocation SelectStableUpdateLocation(string releasesJson)
        {
            GitHubRelease[] releases = new JavaScriptSerializer().Deserialize<GitHubRelease[]>(releasesJson)
                ?? new GitHubRelease[0];
            foreach (GitHubRelease release in releases.Where(item => item != null && !item.draft && !item.prerelease))
            {
                if (!IsApprovedReleasePageUrl(release.html_url)) continue;
                GitHubReleaseAsset manifest = (release.assets ?? new GitHubReleaseAsset[0])
                    .FirstOrDefault(asset => asset != null
                        && String.Equals(asset.name, StableManifestName, StringComparison.OrdinalIgnoreCase)
                        && IsApprovedStableManifestUrl(asset.browser_download_url));
                if (manifest == null) continue;
                return new WindowsUpdateLocation
                {
                    ManifestUrl = manifest.browser_download_url,
                    ReleaseUrl = release.html_url,
                    Tag = release.tag_name
                };
            }
            throw new InvalidOperationException("No official stable SPLINED Windows notification release was found.");
        }

        internal static void ValidateNotificationManifest(
            WindowsUpdateManifest manifest,
            WindowsUpdateLocation location)
        {
            if (manifest == null || manifest.schema != 2)
                throw new InvalidOperationException("The Windows update notification manifest is missing or unsupported.");
            if (!String.Equals(manifest.channel, "stable", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update notification is not for the stable release channel.");
            if (!CommitPattern.IsMatch(manifest.commit ?? ""))
                throw new InvalidOperationException("The Windows update notification contains an invalid commit identity.");
            if (!String.Equals(manifest.commit.Substring(0, 7), manifest.short_commit,
                    StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update notification commit identity is inconsistent.");
            if (location == null || String.IsNullOrWhiteSpace(location.Tag)
                || !String.Equals(manifest.version, location.Tag, StringComparison.Ordinal)
                || !IsApprovedReleasePageUrl(manifest.release_url)
                || !String.Equals(manifest.release_url, location.ReleaseUrl, StringComparison.Ordinal))
                throw new InvalidOperationException("The Windows update notification does not match its official GitHub release page.");
        }

        internal static bool IsApprovedStableManifestUrl(string value)
        {
            Uri uri;
            return Uri.TryCreate(value, UriKind.Absolute, out uri) && IsApprovedStableManifestUri(uri);
        }

        internal static bool IsApprovedReleasePageUrl(string value)
        {
            Uri uri;
            return Uri.TryCreate(value, UriKind.Absolute, out uri)
                && uri.Scheme == Uri.UriSchemeHttps
                && uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
                && uri.AbsolutePath.StartsWith("/scottia/S-P-L-I-N-E-D/releases/tag/", StringComparison.Ordinal);
        }

        private static bool IsApprovedStableReleasesApiUrl(Uri uri)
        {
            return uri != null && uri.Scheme == Uri.UriSchemeHttps
                && uri.Host.Equals("api.github.com", StringComparison.OrdinalIgnoreCase)
                && uri.AbsolutePath.Equals("/repos/scottia/S-P-L-I-N-E-D/releases", StringComparison.Ordinal);
        }

        private static bool IsApprovedStableManifestUri(Uri uri)
        {
            return uri != null && uri.Scheme == Uri.UriSchemeHttps
                && uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
                && uri.AbsolutePath.StartsWith("/scottia/S-P-L-I-N-E-D/releases/download/", StringComparison.Ordinal)
                && uri.AbsolutePath.EndsWith("/" + StableManifestName, StringComparison.OrdinalIgnoreCase);
        }

        private static async Task<byte[]> DownloadBytesAsync(
            string url,
            long maximumBytes,
            Func<Uri, bool> approval)
        {
            Uri uri;
            if (!Uri.TryCreate(url, UriKind.Absolute, out uri) || !approval(uri))
                throw new InvalidOperationException("The SPLINED update URL is not approved.");
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
    }
}
