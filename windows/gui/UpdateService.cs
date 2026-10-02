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

    internal static class WindowsUpdateService
    {
        private const long MaximumUpdaterBytes = 200L * 1024L * 1024L;
        private static readonly Regex CommitPattern = new Regex("^[0-9a-fA-F]{40}$", RegexOptions.Compiled);
        private static readonly Regex ShaPattern = new Regex("^[0-9a-fA-F]{64}$", RegexOptions.Compiled);

        public static bool UsesDevChannel
        {
            get { return String.Equals(BuildInfo.UpdateChannel, "dev", StringComparison.OrdinalIgnoreCase); }
        }

        public static async Task<WindowsUpdateCheck> CheckAsync()
        {
            if (!UsesDevChannel)
                return new WindowsUpdateCheck { Available = false, Manifest = null };

            byte[] bytes = await DownloadBytesAsync(ReleaseInfo.DevUpdateManifestUrl, 1024L * 1024L);
            string json = System.Text.Encoding.UTF8.GetString(bytes).TrimStart('\uFEFF');
            WindowsUpdateManifest manifest = new JavaScriptSerializer().Deserialize<WindowsUpdateManifest>(json);
            ValidateManifest(manifest);
            bool currentKnown = CommitPattern.IsMatch(BuildInfo.Commit ?? "");
            bool available = !currentKnown
                || !String.Equals(BuildInfo.Commit, manifest.commit, StringComparison.OrdinalIgnoreCase);
            return new WindowsUpdateCheck { Available = available, Manifest = manifest };
        }

        public static async Task<string> DownloadAndStageAsync(WindowsUpdateManifest manifest)
        {
            ValidateManifest(manifest);
            byte[] bytes = await DownloadBytesAsync(AddCommitCacheBuster(manifest.asset_url, manifest.short_commit), MaximumUpdaterBytes);
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

        private static async Task<byte[]> DownloadBytesAsync(string url, long maximumBytes)
        {
            Uri uri = ValidateDownloadUri(url);
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

        private static void ValidateManifest(WindowsUpdateManifest manifest)
        {
            if (manifest == null || manifest.schema != 1)
                throw new InvalidOperationException("The Windows update manifest is missing or unsupported.");
            if (!String.Equals(manifest.channel, "dev", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update manifest is not for the dev channel.");
            if (!CommitPattern.IsMatch(manifest.commit ?? "") || !ShaPattern.IsMatch(manifest.sha256 ?? ""))
                throw new InvalidOperationException("The Windows update manifest contains invalid commit or checksum metadata.");
            string expectedShort = manifest.commit.Substring(0, 7);
            if (!String.Equals(expectedShort, manifest.short_commit, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The Windows update manifest commit identity is inconsistent.");
            if (manifest.size <= 0 || manifest.size > MaximumUpdaterBytes)
                throw new InvalidOperationException("The Windows update manifest contains an invalid updater size.");
            ValidateDownloadUri(manifest.asset_url);
        }

        private static Uri ValidateDownloadUri(string value)
        {
            Uri uri;
            if (!Uri.TryCreate(value, UriKind.Absolute, out uri)
                || !uri.Scheme.Equals("https", StringComparison.OrdinalIgnoreCase)
                || !uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
                || !uri.AbsolutePath.Equals(
                    "/scottia/S-P-L-I-N-E-D/releases/download/windows-dev/setup-splined.exe",
                    StringComparison.Ordinal))
                throw new InvalidOperationException("The Windows update URL is not an approved SPLINED release asset.");
            return uri;
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
