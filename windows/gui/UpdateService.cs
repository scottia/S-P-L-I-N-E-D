using System;
using System.Diagnostics;
using System.Linq;
using System.Net;
using System.Reflection;
using System.Runtime.InteropServices;
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

    internal sealed class MicrosoftStoreUpdateCheck
    {
        public bool Available;
        public int UpdateCount;
        internal object Context;
        internal object Updates;
    }

    internal sealed class MicrosoftStoreInstallResult
    {
        public bool Completed;
        public string State;
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

    [ComImport]
    [Guid("3E68D4BD-7135-4D10-8018-9FB6D9F33FA1")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IInitializeWithWindow
    {
        void Initialize(IntPtr windowHandle);
    }

    internal static class MicrosoftStoreUpdateService
    {
        public const string ProductUrl = "https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US";
        private const string StoreContextTypeName =
            "Windows.Services.Store.StoreContext, Windows, ContentType=WindowsRuntime";

        public static async Task<MicrosoftStoreUpdateCheck> CheckAsync(IntPtr windowHandle)
        {
            object context = CreateContext(windowHandle);
            object operation = Invoke(context, "GetAppAndOptionalStorePackageUpdatesAsync");
            object updates = await GetResultsAsync(operation);
            int count = Convert.ToInt32(ReadProperty(updates, "Count", "Size"));
            return new MicrosoftStoreUpdateCheck
            {
                Available = count > 0,
                UpdateCount = count,
                Context = context,
                Updates = updates
            };
        }

        public static async Task<MicrosoftStoreInstallResult> InstallAsync(
            MicrosoftStoreUpdateCheck update,
            IntPtr windowHandle)
        {
            if (update == null || update.Context == null || update.Updates == null || !update.Available)
                throw new InvalidOperationException("No Microsoft Store update is available for installation.");
            InitializeWindow(update.Context, windowHandle);
            object operation = Invoke(update.Context,
                "RequestDownloadAndInstallStorePackageUpdatesAsync", update.Updates);
            object result = await GetResultsAsync(operation);
            string state = Convert.ToString(ReadProperty(result, "OverallState"));
            return new MicrosoftStoreInstallResult
            {
                Completed = String.Equals(state, "Completed", StringComparison.OrdinalIgnoreCase),
                State = String.IsNullOrWhiteSpace(state) ? "Unknown" : state
            };
        }

        public static void OpenStorePage()
        {
            if (!IsApprovedStorePageUrl(ProductUrl))
                throw new InvalidOperationException("The SPLINED Microsoft Store URL is not approved.");
            Process.Start(new ProcessStartInfo(ProductUrl) { UseShellExecute = true });
        }

        internal static bool IsApprovedStorePageUrl(string value)
        {
            Uri uri;
            return Uri.TryCreate(value, UriKind.Absolute, out uri)
                && uri.Scheme == Uri.UriSchemeHttps
                && uri.Host.Equals("apps.microsoft.com", StringComparison.OrdinalIgnoreCase)
                && uri.AbsolutePath.Equals("/detail/9p8g4gmbbvbs", StringComparison.OrdinalIgnoreCase)
                && uri.Query.Equals("?hl=en-US&gl=US", StringComparison.Ordinal);
        }

        private static object CreateContext(IntPtr windowHandle)
        {
            Type contextType = ResolveStoreContextType();
            MethodInfo getDefault = contextType.GetMethod("GetDefault", BindingFlags.Public | BindingFlags.Static);
            if (getDefault == null)
                throw new PlatformNotSupportedException("Microsoft StoreContext.GetDefault is unavailable.");
            object context = getDefault.Invoke(null, null);
            if (context == null)
                throw new InvalidOperationException("Microsoft Store did not provide an application context.");
            InitializeWindow(context, windowHandle);
            return context;
        }

        private static Type ResolveStoreContextType()
        {
            Type contextType = Type.GetType(StoreContextTypeName, false);
            if (contextType != null) return contextType;
            try
            {
                AssemblyName windows = new AssemblyName("Windows");
                windows.ContentType = AssemblyContentType.WindowsRuntime;
                contextType = Assembly.Load(windows).GetType("Windows.Services.Store.StoreContext", false);
            }
            catch
            {
                contextType = null;
            }
            if (contextType == null)
                throw new PlatformNotSupportedException(
                    "Microsoft Store package update APIs are unavailable on this Windows installation.");
            return contextType;
        }

        private static void InitializeWindow(object context, IntPtr windowHandle)
        {
            if (windowHandle == IntPtr.Zero) return;
            IInitializeWithWindow initializer = context as IInitializeWithWindow;
            if (initializer == null)
                throw new PlatformNotSupportedException(
                    "Microsoft Store update UI could not be associated with the SPLINED window.");
            initializer.Initialize(windowHandle);
        }

        private static async Task<object> GetResultsAsync(object operation)
        {
            if (operation == null)
                throw new InvalidOperationException("Microsoft Store did not return an update operation.");
            while (true)
            {
                string status = Convert.ToString(ReadProperty(operation, "Status"));
                if (String.Equals(status, "Completed", StringComparison.OrdinalIgnoreCase))
                    return Invoke(operation, "GetResults");
                if (String.Equals(status, "Canceled", StringComparison.OrdinalIgnoreCase))
                    throw new TaskCanceledException("The Microsoft Store update operation was canceled.");
                if (String.Equals(status, "Error", StringComparison.OrdinalIgnoreCase))
                {
                    object error = ReadProperty(operation, "ErrorCode");
                    throw new InvalidOperationException("Microsoft Store update operation failed: " + error);
                }
                if (!String.Equals(status, "Started", StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException("Microsoft Store returned an unknown update state: " + status);
                await Task.Delay(100);
            }
        }

        private static object ReadProperty(object target, params string[] names)
        {
            if (target == null) throw new ArgumentNullException("target");
            foreach (string name in names)
            {
                foreach (Type candidate in ReflectionTypes(target.GetType()))
                {
                    PropertyInfo property = candidate.GetProperty(name);
                    if (property != null) return property.GetValue(target, null);
                }
            }
            throw new MissingMemberException(target.GetType().FullName, String.Join(" or ", names));
        }

        private static object Invoke(object target, string name, params object[] arguments)
        {
            if (target == null) throw new ArgumentNullException("target");
            int argumentCount = arguments == null ? 0 : arguments.Length;
            foreach (Type candidate in ReflectionTypes(target.GetType()))
            {
                MethodInfo method = candidate.GetMethods()
                    .FirstOrDefault(item => item.Name == name
                        && item.GetParameters().Length == argumentCount);
                if (method != null) return method.Invoke(target, arguments);
            }
            throw new MissingMethodException(target.GetType().FullName, name);
        }

        private static Type[] ReflectionTypes(Type concrete)
        {
            return new[] { concrete }.Concat(concrete.GetInterfaces()).ToArray();
        }
    }
}
