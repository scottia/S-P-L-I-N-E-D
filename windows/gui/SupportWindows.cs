using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Net;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class HelpWindows
    {
        public static void ShowSetupHelp(IWin32Window owner)
        {
            OpenHelp(owner);
        }

        public static void OpenHelp(IWin32Window owner)
        {
            try
            {
                Process.Start(new ProcessStartInfo
                {
                    FileName = ReleaseInfo.HelpUrl,
                    UseShellExecute = true
                });
            }
            catch (Exception error)
            {
                MessageBox.Show(owner,
                    "SPLINED could not open Help in the default browser.\r\n\r\n"
                    + ReleaseInfo.HelpUrl
                    + "\r\n\r\nPress Ctrl+C to copy this message.\r\n\r\n"
                    + error.Message,
                    "Unable to open SPLINED Help",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Warning);
            }
        }
    }

    internal static class CredentialStore
    {
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer();

        public static string PathFor(ConfigState state, string provider)
        {
            return Path.Combine(state.CredentialDir, provider + ".json");
        }

        public static Dictionary<string, object> Load(ConfigState state, string provider)
        {
            try
            {
                string path = PathFor(state, provider);
                if (!File.Exists(path)) return new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
                Dictionary<string, object> values = Json.Deserialize<Dictionary<string, object>>(File.ReadAllText(path));
                return values ?? new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            }
            catch { return new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase); }
        }

        public static bool Save(ConfigState state, string provider, Dictionary<string, object> values)
        {
            Directory.CreateDirectory(state.CredentialDir);
            Dictionary<string, object> merged = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            string path = PathFor(state, provider);
            if (File.Exists(path))
            {
                Dictionary<string, object> existing = Json.Deserialize<Dictionary<string, object>>(File.ReadAllText(path));
                if (existing == null) throw new InvalidOperationException("The existing " + provider + " credential JSON is not an object.");
                DeepMerge(merged, existing);
            }
            DeepMerge(merged, values);
            string body = Json.Serialize(merged);
            bool protectedFile = ConfigStore.WriteTextAtomic(
                path,
                PrettyJson(body) + Environment.NewLine,
                ApplyUserOnlyAcl);
            if (!protectedFile)
            {
                string warning = "Credential file created; user-specific filesystem ACL protection\r\n"
                    + "is unavailable on this storage location.\r\n\r\n" + path;
                if (Application.MessageLoop)
                    MessageBox.Show(warning, "SPLINED credential protection",
                        MessageBoxButtons.OK, MessageBoxIcon.Warning);
                else
                    Console.Error.WriteLine(warning);
            }
            return protectedFile;
        }

        internal static bool ApplyUserOnlyAcl(string path)
        {
            try
            {
                SecurityIdentifier user = WindowsIdentity.GetCurrent().User;
                if (user == null) return false;
                FileSecurity security = new FileSecurity();
                security.SetAccessRuleProtection(true, false);
                security.SetOwner(user);
                security.AddAccessRule(new FileSystemAccessRule(
                    user,
                    FileSystemRights.FullControl,
                    AccessControlType.Allow));
                security.AddAccessRule(new FileSystemAccessRule(
                    new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null),
                    FileSystemRights.FullControl,
                    AccessControlType.Allow));
                File.SetAccessControl(path, security);
                return true;
            }
            catch
            {
                return false;
            }
        }

        public static MusicBrainzRuntimeOptions LoadMusicBrainzOptions(ConfigState state)
        {
            Dictionary<string, object> values = Load(state, "musicbrainz");
            Dictionary<string, object> options = NestedObject(values, "options");
            return new MusicBrainzRuntimeOptions
            {
                RetryMax = IntegerOption(options, "retry_max", 4),
                MinDelay = DecimalOption(options, "min_delay", 1.05),
                RecordingTimeout = IntegerOption(options, "recording_timeout", 7)
            };
        }

        public static void MergeMusicBrainzOptions(ConfigState state, MusicBrainzRuntimeOptions options)
        {
            string path = PathFor(state, "musicbrainz");
            if (!File.Exists(path))
                throw new InvalidOperationException("Configure MusicBrainz credentials before saving MusicBrainz runtime options.");
            Dictionary<string, object> optionValues = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase)
            {
                { "retry_max", options.RetryMax },
                { "min_delay", options.MinDelay },
                { "recording_timeout", options.RecordingTimeout }
            };
            Save(state, "musicbrainz", new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase)
            {
                { "options", optionValues }
            });
        }

        public static string Status(ConfigState state, string provider)
        {
            if (provider == "itunes" || provider == "coverartarchive" || provider == "deezer") return "READY";
            Dictionary<string, object> values = Load(state, provider);
            if (provider == "discogs") return Has(values, "token") ? (WasValidated(values) ? "TESTED" : "SAVED") : "PENDING";
            if (provider == "fanarttv")
            {
                if (!Has(values, "api_key")) return "PENDING";
                object configuredVersion;
                if (values.TryGetValue("api_version", out configuredVersion)
                    && !String.IsNullOrWhiteSpace(Convert.ToString(configuredVersion))
                    && !String.Equals(Convert.ToString(configuredVersion), "v3.2", StringComparison.OrdinalIgnoreCase))
                    return "VERSION ERROR";
                return WasValidated(values, "v3.2") ? "v3.2 TESTED" : "v3.2 SAVED";
            }
            if (provider == "musicbrainz")
            {
                bool tokenPresent = Has(values, "access_token") || Has(values, "token");
                if (!Enabled(values, "oauth_enabled") && !tokenPresent) return "DISABLED";
                if (tokenPresent) return WasValidated(values) ? "OAUTH TESTED" : "TOKEN SAVED";
                if (!Has(values, "client_id") || !Has(values, "client_secret")
                    || !Has(values, "callback_uri") || !Has(values, "oauth_scope")) return "INCOMPLETE";
                if (!Has(values, "refresh_token")) return "AUTH NEEDED";
                return WasValidated(values) ? "OAUTH TESTED" : "TOKEN SAVED";
            }
            if (provider == "lastfm")
            {
                // album.getInfo is a read method and requires only the API key.
                return Has(values, "api_key") ? (WasValidated(values) ? "TESTED" : "SAVED") : "PENDING";
            }
            return "PENDING";
        }

        public static void ApplyValidationMetadata(Dictionary<string, object> values, CredentialValidationResult result)
        {
            values["validated_at_utc"] = DateTime.UtcNow.ToString("o");
            values["validated_endpoint"] = result.Endpoint;
            if (!String.IsNullOrWhiteSpace(result.ApiVersion)) values["validated_api_version"] = result.ApiVersion;
            else values.Remove("validated_api_version");
        }

        public static void ClearValidationMetadata(Dictionary<string, object> values)
        {
            // Null is an explicit merge instruction: the previous successful
            // validation must not be resurrected from the on-disk document.
            values["validated_at_utc"] = null;
            values["validated_endpoint"] = null;
            values["validated_api_version"] = null;
        }

        public static bool IsConfiguredStatus(string status)
        {
            return !String.Equals(status, "PENDING", StringComparison.OrdinalIgnoreCase);
        }

        public static Color StatusColor(string status)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            if (status.IndexOf("TESTED", StringComparison.OrdinalIgnoreCase) >= 0 || status == "READY")
                return palette.Success;
            if (status == "INCOMPLETE" || status == "VERSION ERROR") return palette.Error;
            if (status == "DISABLED") return palette.TextDisabled;
            return palette.Warning;
        }

        public static void StyleTestLink(LinkLabel link)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            link.LinkColor = palette.Link;
            link.ActiveLinkColor = palette.LinkActive;
            link.VisitedLinkColor = link.LinkColor;
        }

        private static bool Has(Dictionary<string, object> values, string key)
        {
            object value;
            return values.TryGetValue(key, out value) && value != null && Convert.ToString(value).Trim().Length > 0;
        }

        private static void DeepMerge(Dictionary<string, object> target, Dictionary<string, object> update)
        {
            foreach (KeyValuePair<string, object> pair in update)
            {
                Dictionary<string, object> incoming = pair.Value as Dictionary<string, object>;
                object currentValue;
                Dictionary<string, object> current = target.TryGetValue(pair.Key, out currentValue)
                    ? currentValue as Dictionary<string, object>
                    : null;
                if (incoming != null && current != null)
                {
                    DeepMerge(current, incoming);
                    continue;
                }
                if (incoming != null)
                {
                    Dictionary<string, object> copy = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
                    DeepMerge(copy, incoming);
                    target[pair.Key] = copy;
                }
                else target[pair.Key] = pair.Value;
            }
        }

        private static Dictionary<string, object> NestedObject(Dictionary<string, object> values, string key)
        {
            object nested;
            return values.TryGetValue(key, out nested) && nested is Dictionary<string, object>
                ? (Dictionary<string, object>)nested
                : new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
        }

        private static int IntegerOption(Dictionary<string, object> values, string key, int fallback)
        {
            object raw;
            int value;
            return values.TryGetValue(key, out raw)
                && Int32.TryParse(Convert.ToString(raw, CultureInfo.InvariantCulture), NumberStyles.Integer, CultureInfo.InvariantCulture, out value)
                ? value : fallback;
        }

        private static double DecimalOption(Dictionary<string, object> values, string key, double fallback)
        {
            object raw;
            double value;
            return values.TryGetValue(key, out raw)
                && Double.TryParse(Convert.ToString(raw, CultureInfo.InvariantCulture), NumberStyles.Float, CultureInfo.InvariantCulture, out value)
                ? value : fallback;
        }

        private static bool Enabled(Dictionary<string, object> values, string key)
        {
            object value;
            if (!values.TryGetValue(key, out value) || value == null) return false;
            bool parsed;
            return value is bool ? (bool)value : Boolean.TryParse(Convert.ToString(value), out parsed) && parsed;
        }

        private static bool WasValidated(Dictionary<string, object> values, string apiVersion = null)
        {
            if (!Has(values, "validated_at_utc") || !Has(values, "validated_endpoint")) return false;
            if (String.IsNullOrWhiteSpace(apiVersion)) return true;
            object version;
            return values.TryGetValue("validated_api_version", out version)
                && String.Equals(Convert.ToString(version), apiVersion, StringComparison.OrdinalIgnoreCase);
        }

        private static string PrettyJson(string json)
        {
            int indent = 0;
            bool quoted = false;
            bool escaped = false;
            System.Text.StringBuilder output = new System.Text.StringBuilder();
            foreach (char character in json)
            {
                if (quoted)
                {
                    output.Append(character);
                    if (escaped) escaped = false;
                    else if (character == '\\') escaped = true;
                    else if (character == '"') quoted = false;
                    continue;
                }
                if (character == '"') { quoted = true; output.Append(character); }
                else if (character == '{' || character == '[') { output.Append(character).AppendLine(); indent++; output.Append(new string(' ', indent * 2)); }
                else if (character == '}' || character == ']') { output.AppendLine(); indent--; output.Append(new string(' ', indent * 2)).Append(character); }
                else if (character == ',') { output.Append(character).AppendLine().Append(new string(' ', indent * 2)); }
                else if (character == ':') output.Append(": ");
                else if (!Char.IsWhiteSpace(character)) output.Append(character);
            }
            return output.ToString();
        }
    }

    internal sealed class MusicBrainzRuntimeOptions
    {
        public int RetryMax = 4;
        public double MinDelay = 1.05;
        public int RecordingTimeout = 7;

        public MusicBrainzRuntimeOptions Clone()
        {
            return (MusicBrainzRuntimeOptions)MemberwiseClone();
        }
    }

    internal sealed class CredentialValidationResult
    {
        public bool Success;
        public string Message;
        public string Endpoint;
        public string ApiVersion;
    }

    internal static class ProviderCredentialValidator
    {
        private const string FanartReleaseGroup = "1b022e01-4da6-387b-8658-8678046e4cef";
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer();

        public static Task<CredentialValidationResult> ValidateAsync(string provider, ConfigState state, Dictionary<string, object> values)
        {
            return Task.Run(delegate
            {
                if (provider == "fanarttv") return ValidateFanartTv(values);
                if (provider == "lastfm") return ValidateLastFm(values);
                if (provider == "discogs") return ValidateDiscogs(values);
                if (provider == "musicbrainz") return ValidateMusicBrainz(values);
                return Failure("No live credential test is available for this provider.", "");
            });
        }

        internal static bool IsFanartV32AlbumResponse(string body, out int coverCount)
        {
            coverCount = 0;
            Dictionary<string, object> root = DeserializeObject(body);
            object albumsValue;
            if (root == null || !root.TryGetValue("albums", out albumsValue)) return false;
            IEnumerable albums = albumsValue as IEnumerable;
            if (albums == null || albumsValue is string || albumsValue is Dictionary<string, object>) return false;
            foreach (object item in albums)
            {
                Dictionary<string, object> album = item as Dictionary<string, object>;
                if (album == null || !String.Equals(Get(album, "release_group_id"), FanartReleaseGroup, StringComparison.OrdinalIgnoreCase)) continue;
                object coversValue;
                if (album.TryGetValue("albumcover", out coversValue)) coverCount += CountNonEmptyUrls(coversValue);
                return true;
            }
            return false;
        }

        private static CredentialValidationResult ValidateFanartTv(Dictionary<string, object> values)
        {
            string apiKey = Get(values, "api_key");
            string clientKey = Get(values, "client_key");
            if (apiKey.Length == 0) return Failure("Enter the Fanart.tv project API key first.", "Fanart.tv v3.2 album endpoint", "v3.2");
            string apiVersion = Get(values, "api_version");
            if (apiVersion.Length > 0 && !apiVersion.Equals("v3.2", StringComparison.OrdinalIgnoreCase))
                return Failure("This credential is marked " + apiVersion + ". Open Change and save it as Fanart.tv v3.2 before testing.", "Fanart.tv v3.2 album endpoint", "v3.2");
            Dictionary<string, string> headers = new Dictionary<string, string> { { "api-key", apiKey } };
            if (clientKey.Length > 0) headers["client-key"] = clientKey;
            HttpResult response = GetJson(
                "https://webservice.fanart.tv/v3.2/music/albums/" + FanartReleaseGroup,
                headers);
            if (response.Status == 0) return Failure(NetworkFailure("Fanart.tv", response), "Fanart.tv v3.2 album endpoint", "v3.2");
            if (response.Status == 401 || response.Status == 403)
                return Failure("Fanart.tv rejected the project/client key combination.", "Fanart.tv v3.2 album endpoint", "v3.2");
            if (response.Status == 429)
                return Failure("Fanart.tv accepted the request but rate-limited it" + RetryText(response) + ". Try again later.", "Fanart.tv v3.2 album endpoint", "v3.2");
            if (response.Status != 200)
                return Failure("Fanart.tv v3.2 returned HTTP " + response.Status + ".", "Fanart.tv v3.2 album endpoint", "v3.2");
            int covers;
            if (!IsFanartV32AlbumResponse(response.Body, out covers))
                return Failure("Fanart.tv answered, but the response was not the required v3.2 albums-array format.", "Fanart.tv v3.2 album endpoint", "v3.2");
            return Success("Fanart.tv v3.2 accepted the saved key(s); the v3.2 album response was verified and returned " + covers + " cover image(s).", "Fanart.tv v3.2 album endpoint", "v3.2");
        }

        private static CredentialValidationResult ValidateLastFm(Dictionary<string, object> values)
        {
            string apiKey = Get(values, "api_key");
            if (apiKey.Length == 0) return Failure("Enter the Last.fm API key first.", "Last.fm API 2.0 album.getInfo");
            string url = "https://ws.audioscrobbler.com/2.0/?method=album.getinfo&artist=Cher&album=Believe&autocorrect=1&format=json&api_key="
                + Uri.EscapeDataString(apiKey);
            HttpResult response = GetJson(url, null);
            if (response.Status == 0) return Failure(NetworkFailure("Last.fm", response), "Last.fm API 2.0 album.getInfo");
            if (response.Status != 200) return Failure("Last.fm returned HTTP " + response.Status + ".", "Last.fm API 2.0 album.getInfo");
            Dictionary<string, object> root = DeserializeObject(response.Body);
            if (root == null) return Failure("Last.fm returned invalid JSON.", "Last.fm API 2.0 album.getInfo");
            object error;
            if (root.TryGetValue("error", out error))
                return Failure("Last.fm rejected the API test (error " + Convert.ToString(error) + "): " + Get(root, "message"), "Last.fm API 2.0 album.getInfo");
            object albumValue;
            Dictionary<string, object> album;
            if (!root.TryGetValue("album", out albumValue) || (album = albumValue as Dictionary<string, object>) == null)
                return Failure("Last.fm answered without the expected album.getInfo payload.", "Last.fm API 2.0 album.getInfo");
            object imagesValue;
            int imageCount = album.TryGetValue("image", out imagesValue) ? CountNonEmptyUrls(imagesValue) : 0;
            return Success("Last.fm accepted the API key through album.getInfo; the sample album returned " + imageCount + " artwork URL(s). The shared secret/session are not required for artwork reads.", "Last.fm API 2.0 album.getInfo");
        }

        private static CredentialValidationResult ValidateDiscogs(Dictionary<string, object> values)
        {
            string token = Get(values, "token");
            if (token.Length == 0) return Failure("Enter the Discogs personal access token first.", "Discogs API v2 /oauth/identity", "v2");
            HttpResult response = GetJson(
                "https://api.discogs.com/oauth/identity",
                new Dictionary<string, string> { { "Authorization", "Discogs token=" + token } });
            if (response.Status == 0) return Failure(NetworkFailure("Discogs", response), "Discogs API v2 /oauth/identity", "v2");
            if (response.Status == 401 || response.Status == 403)
                return Failure("Discogs rejected the personal access token.", "Discogs API v2 /oauth/identity", "v2");
            if (response.Status == 429)
                return Failure("Discogs rate-limited the test" + RetryText(response) + ". Try again later.", "Discogs API v2 /oauth/identity", "v2");
            if (response.Status != 200)
                return Failure("Discogs returned HTTP " + response.Status + ".", "Discogs API v2 /oauth/identity", "v2");
            Dictionary<string, object> root = DeserializeObject(response.Body);
            if (root == null || Get(root, "username").Length == 0)
                return Failure("Discogs answered without a valid identity payload.", "Discogs API v2 /oauth/identity", "v2");
            return Success("Discogs accepted the personal access token through /oauth/identity.", "Discogs API v2 /oauth/identity", "v2");
        }

        private static CredentialValidationResult ValidateMusicBrainz(Dictionary<string, object> values)
        {
            object enabledValue;
            bool enabled;
            bool tokenPresent = Get(values, "access_token").Length > 0 || Get(values, "token").Length > 0;
            if ((!values.TryGetValue("oauth_enabled", out enabledValue)
                || !Boolean.TryParse(Convert.ToString(enabledValue), out enabled) || !enabled) && !tokenPresent)
                return Failure("MusicBrainz OAuth is disabled in credentials\\musicbrainz.json. Public metadata lookups remain anonymous.", "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            string accessToken = Get(values, "access_token");
            if (accessToken.Length == 0) accessToken = Get(values, "token");
            if (accessToken.Length == 0)
            {
                string suffix = Get(values, "refresh_token").Length > 0
                    ? " A refresh token exists, but a live access token is required for this direct test."
                    : " Complete MusicBrainz OAuth authorization first.";
                return Failure("The MusicBrainz credential file contains no access token." + suffix, "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            }
            HttpResult response = GetJson(
                "https://musicbrainz.org/oauth2/userinfo",
                new Dictionary<string, string> { { "Authorization", "Bearer " + accessToken } });
            if (response.Status == 0) return Failure(NetworkFailure("MusicBrainz", response), "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            if (response.Status == 401 || response.Status == 403)
                return Failure("MusicBrainz rejected the saved OAuth access token. It may be expired or revoked; reauthorize or allow the core to refresh it.", "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            if (response.Status == 429)
                return Failure("MusicBrainz rate-limited the test" + RetryText(response) + ". Try again later.", "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            if (response.Status != 200)
                return Failure("MusicBrainz OAuth userinfo returned HTTP " + response.Status + ".", "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            Dictionary<string, object> root = DeserializeObject(response.Body);
            if (root == null || (Get(root, "sub").Length == 0 && Get(root, "username").Length == 0 && Get(root, "preferred_username").Length == 0))
                return Failure("MusicBrainz answered without the expected OAuth user identity payload.", "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
            return Success("MusicBrainz accepted the saved OAuth Bearer token through /oauth2/userinfo. Public metadata lookups and authenticated requests are available.", "MusicBrainz OAuth2 /oauth2/userinfo", "OAuth2");
        }

        private static HttpResult GetJson(string url, Dictionary<string, string> headers)
        {
            try
            {
                ServicePointManager.SecurityProtocol |= (SecurityProtocolType)3072;
                HttpWebRequest request = (HttpWebRequest)WebRequest.Create(url);
                request.Method = "GET";
                request.Accept = "application/json";
                request.UserAgent = "SPLINED-Windows-GUI/" + ReleaseInfo.SemanticVersion;
                request.Timeout = 20000;
                request.ReadWriteTimeout = 20000;
                if (headers != null)
                    foreach (KeyValuePair<string, string> header in headers) request.Headers[header.Key] = header.Value;
                using (HttpWebResponse response = (HttpWebResponse)request.GetResponse()) return ReadResponse(response);
            }
            catch (WebException error)
            {
                HttpWebResponse response = error.Response as HttpWebResponse;
                if (response != null) using (response) return ReadResponse(response);
                return new HttpResult { Status = 0, Body = "", NetworkError = error.Status.ToString() };
            }
            catch (Exception error)
            {
                return new HttpResult { Status = 0, Body = "", NetworkError = error.GetType().Name };
            }
        }

        private static HttpResult ReadResponse(HttpWebResponse response)
        {
            string body = "";
            using (Stream stream = response.GetResponseStream())
            using (StreamReader reader = new StreamReader(stream)) body = reader.ReadToEnd();
            return new HttpResult
            {
                Status = (int)response.StatusCode,
                Body = body,
                RetryAfter = response.Headers["Retry-After"]
            };
        }

        private static Dictionary<string, object> DeserializeObject(string body)
        {
            try { return Json.DeserializeObject(body) as Dictionary<string, object>; }
            catch { return null; }
        }

        private static int CountNonEmptyUrls(object value)
        {
            IEnumerable items = value as IEnumerable;
            if (items == null || value is string) return 0;
            int count = 0;
            foreach (object item in items)
            {
                Dictionary<string, object> entry = item as Dictionary<string, object>;
                if (entry != null && (Get(entry, "url").Length > 0 || Get(entry, "#text").Length > 0)) count++;
            }
            return count;
        }

        private static string Get(Dictionary<string, object> values, string key)
        {
            object value;
            return values != null && values.TryGetValue(key, out value) && value != null ? Convert.ToString(value).Trim() : "";
        }

        private static string RetryText(HttpResult result)
        {
            return String.IsNullOrWhiteSpace(result.RetryAfter) ? "" : " (retry after " + result.RetryAfter + " seconds)";
        }

        private static string NetworkFailure(string provider, HttpResult result)
        {
            return "Unable to reach " + provider + " for the credential test"
                + (String.IsNullOrWhiteSpace(result.NetworkError) ? "." : " (" + result.NetworkError + ").");
        }

        private static CredentialValidationResult Success(string message, string endpoint, string apiVersion = null)
        {
            return new CredentialValidationResult { Success = true, Message = message, Endpoint = endpoint, ApiVersion = apiVersion };
        }

        private static CredentialValidationResult Failure(string message, string endpoint, string apiVersion = null)
        {
            return new CredentialValidationResult { Success = false, Message = message, Endpoint = endpoint, ApiVersion = apiVersion };
        }

        private sealed class HttpResult
        {
            public int Status;
            public string Body;
            public string RetryAfter;
            public string NetworkError;
        }
    }

    internal sealed class CredentialsForm : FluentForm
    {
        private readonly ConfigState state;
        private readonly TableLayoutPanel rows;

        public CredentialsForm(ConfigState state)
        {
            this.state = state;
            Text = "SPLINED - Credentials";
            StartPosition = FormStartPosition.CenterParent;
            MinimumSize = new Size(660, 430);
            Size = new Size(760, 470);
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(20), RowCount = 4, ColumnCount = 1 };
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 44));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            Controls.Add(root);
            root.Controls.Add(new Label { Text = "Credentials and Live Status", Dock = DockStyle.Fill, Font = ThemeManager.UiFont(ThemeFontRole.AppTitle) }, 0, 0);
            root.Controls.Add(new Label { Text = "SAVED means a credential file exists. TESTED means the provider accepted that saved credential through the endpoint shown in its test result.", Dock = DockStyle.Fill }, 0, 1);
            rows = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 4, RowCount = 4, Padding = new Padding(0, 8, 0, 8) };
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 25));
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 25));
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 22));
            rows.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 28));
            for (int i = 0; i < 4; i++) rows.RowStyles.Add(new RowStyle(SizeType.Percent, 25));
            root.Controls.Add(rows, 0, 2);
            AddProviderRow(0, "Last.fm", "lastfm");
            AddProviderRow(1, "Fanart.tv", "fanarttv");
            AddProviderRow(2, "Discogs", "discogs");
            AddProviderRow(3, "MusicBrainz OAuth", "musicbrainz");
            Button close = new FluentButton { Text = "Close", Width = 110, Height = 34, Anchor = AnchorStyles.Right };
            close.Click += delegate { Close(); };
            root.Controls.Add(close, 0, 3);
            ThemeManager.Apply(this, ThemeManager.CurrentTheme);
            foreach (Label label in Controls.Find("credentialStatusLabel", true).OfType<Label>())
                label.ForeColor = CredentialStore.StatusColor(Convert.ToString(label.Tag));
            ThemeManager.PrepareForFirstShow(this, ThemeManager.CurrentTheme);
        }

        private void AddProviderRow(int row, string display, string provider)
        {
            rows.Controls.Add(new Label { Text = display, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, row);
            string status = CredentialStore.Status(state, provider);
            Label statusLabel = new Label { Name = "credentialStatusLabel", Tag = status, Text = "[" + status + "]", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft };
            rows.Controls.Add(statusLabel, 1, row);
            LinkLabel test = new LinkLabel { Name = provider + "TestLink", Text = "Test saved", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft, Enabled = CredentialStore.IsConfiguredStatus(status) && status != "DISABLED" && status != "INCOMPLETE" && status != "AUTH NEEDED" };
            CredentialStore.StyleTestLink(test);
            test.LinkClicked += async delegate
            {
                test.Enabled = false;
                statusLabel.Text = "[TESTING]";
                statusLabel.ForeColor = ThemeManager.CurrentPalette.Warning;
                Dictionary<string, object> saved = CredentialStore.Load(state, provider);
                CredentialValidationResult result = await ProviderCredentialValidator.ValidateAsync(provider, state, saved);
                if (result.Success)
                {
                    CredentialStore.ApplyValidationMetadata(saved, result);
                    CredentialStore.Save(state, provider, saved);
                }
                string refreshed = CredentialStore.Status(state, provider);
                statusLabel.Text = "[" + refreshed + "]";
                statusLabel.Tag = refreshed;
                statusLabel.ForeColor = result.Success ? CredentialStore.StatusColor(refreshed) : ThemeManager.CurrentPalette.Error;
                test.Enabled = true;
                MessageBox.Show(this, result.Message + "\r\n\r\nEndpoint: " + result.Endpoint,
                    result.Success ? display + " credential passed" : display + " credential failed",
                    MessageBoxButtons.OK, result.Success ? MessageBoxIcon.Information : MessageBoxIcon.Warning);
            };
            rows.Controls.Add(test, 2, row);
            Button configure = new FluentButton { Text = CredentialStore.IsConfiguredStatus(status) ? "Change" : "Configure", Dock = DockStyle.Fill, Margin = new Padding(12, 7, 12, 7) };
            configure.Click += delegate
            {
                using (CredentialEditForm edit = new CredentialEditForm(state, provider, display)) edit.ShowDialog(this);
                string refreshed = CredentialStore.Status(state, provider);
                statusLabel.Text = "[" + refreshed + "]";
                statusLabel.Tag = refreshed;
                statusLabel.ForeColor = CredentialStore.StatusColor(refreshed);
                test.Enabled = refreshed != "PENDING" && refreshed != "DISABLED" && refreshed != "INCOMPLETE" && refreshed != "AUTH NEEDED";
            };
            rows.Controls.Add(configure, 3, row);
        }
    }

    internal sealed class CredentialEditForm : FluentForm
    {
        private readonly ConfigState state;
        private readonly string provider;
        private readonly Dictionary<string, object> values;
        private readonly Dictionary<string, TextBox> fields = new Dictionary<string, TextBox>();
        private readonly string initialFingerprint;
        private string testedFingerprint;
        private CredentialValidationResult successfulValidation;
        private Button testButton;
        private Label validationStatus;
        private CheckBox musicBrainzOAuthEnabled;

        public CredentialEditForm(ConfigState state, string provider, string display)
        {
            this.state = state;
            this.provider = provider;
            values = CredentialStore.Load(state, provider);
            if (provider == "fanarttv") values["api_version"] = "v3.2";
            if (provider == "musicbrainz")
            {
                if (!values.ContainsKey("oauth_enabled"))
                    values["oauth_enabled"] = HasCredentialValue(values, "access_token") || HasCredentialValue(values, "token");
                if (!values.ContainsKey("callback_uri")) values["callback_uri"] = "urn:ietf:wg:oauth:2.0:oob";
                if (!values.ContainsKey("oauth_scope")) values["oauth_scope"] = "profile";
            }
            Text = "SPLINED - " + display;
            StartPosition = FormStartPosition.CenterParent;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = false;
            MinimizeBox = false;
            bool musicBrainz = provider == "musicbrainz";
            Size = musicBrainz ? new Size(720, 680) : new Size(680, 500);
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            int rowCount = musicBrainz ? 13 : 9;
            int validationRow = musicBrainz ? 10 : 6;
            int actionRow = musicBrainz ? 12 : 8;
            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(20), ColumnCount = 2, RowCount = rowCount };
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 190));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 45));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 62));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            for (int i = 3; i < validationRow; i++) root.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 58));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            Controls.Add(root);
            Label title = new Label { Text = display + " Credential", Dock = DockStyle.Fill, Font = ThemeManager.UiFont(ThemeFontRole.AppTitle) };
            root.Controls.Add(title, 0, 0); root.SetColumnSpan(title, 2);
            Label note = new Label { Text = provider == "musicbrainz"
                ? "All MusicBrainz OAuth settings and tokens are stored only in credentials\\musicbrainz.json. Config v5 stores the credential directory, never provider filenames or credential values."
                : "Existing fields are loaded and preserved. Test performs a live provider request; SAVED alone does not mean the credential was accepted.", Dock = DockStyle.Fill };
            root.Controls.Add(note, 0, 1); root.SetColumnSpan(note, 2);
            string endpoint = provider == "fanarttv" ? "Fanart.tv API version" : provider == "lastfm" ? "Read test" : provider == "discogs" ? "Identity test" : "OAuth test";
            string endpointValue = provider == "fanarttv" ? "v3.2 (current)" : provider == "lastfm" ? "API 2.0 · album.getInfo" : provider == "discogs" ? "API v2 · /oauth/identity" : "OAuth2 · /oauth2/userinfo";
            root.Controls.Add(new Label { Text = endpoint, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, 2);
            root.Controls.Add(new Label { Name = provider + "ApiIdentity", Text = endpointValue, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft, Font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold) }, 1, 2);
            int row = 3;
            if (provider == "lastfm")
            {
                AddField(root, row++, "API key", "api_key", true);
                AddField(root, row++, "Shared secret (optional for artwork)", "shared_secret", true);
            }
            else if (provider == "fanarttv")
            {
                AddField(root, row++, "API key", "api_key", true);
                AddField(root, row++, "Client key (optional)", "client_key", true);
            }
            else if (provider == "discogs") AddField(root, row++, "Personal access token", "token", true);
            else if (provider == "musicbrainz")
            {
                musicBrainzOAuthEnabled = new FluentCheckBox { Text = "Enable MusicBrainz OAuth", Dock = DockStyle.Fill, Checked = CredentialBoolean(values, "oauth_enabled") };
                root.Controls.Add(musicBrainzOAuthEnabled, 0, row); root.SetColumnSpan(musicBrainzOAuthEnabled, 2); row++;
                AddField(root, row++, "Client ID", "client_id", false);
                AddField(root, row++, "Client secret", "client_secret", true);
                AddField(root, row++, "Callback URI", "callback_uri", false);
                AddField(root, row++, "OAuth scope", "oauth_scope", false);
                AddField(root, row++, "Access token", "access_token", true);
                AddField(root, row++, "Refresh token", "refresh_token", true);
            }
            FlowLayoutPanel validation = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.LeftToRight, WrapContents = false };
            testButton = new FluentButton { Name = provider + "CredentialTestButton", Text = provider == "fanarttv" ? "Validate v3.2" : "Test credentials", Width = 135, Height = 34, Tag = "primary" };
            testButton.Click += ValidateClicked;
            validationStatus = new Label { Name = provider + "CredentialTestStatus", Text = "Not tested in this window", AutoSize = false, Width = 360, Height = 34, TextAlign = ContentAlignment.MiddleLeft, AutoEllipsis = true };
            validation.Controls.Add(testButton);
            validation.Controls.Add(validationStatus);
            root.Controls.Add(validation, 0, validationRow); root.SetColumnSpan(validation, 2);
            FlowLayoutPanel actions = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft };
            Button save = new FluentButton { Text = "Save", Width = 110, Height = 34, Tag = "primary" };
            Button cancel = new FluentButton { Text = "Cancel", Width = 110, Height = 34, DialogResult = DialogResult.Cancel };
            save.Click += SaveClicked;
            actions.Controls.Add(save); actions.Controls.Add(cancel);
            root.Controls.Add(actions, 0, actionRow); root.SetColumnSpan(actions, 2);
            AcceptButton = save; CancelButton = cancel;
            initialFingerprint = Fingerprint(CurrentValues());
            ThemeManager.Apply(this, ThemeManager.CurrentTheme);
            ThemeManager.PrepareForFirstShow(this, ThemeManager.CurrentTheme);
        }

        private void AddField(TableLayoutPanel root, int row, string label, string key, bool secret)
        {
            root.Controls.Add(new Label { Text = label, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, row);
            TextBox box = new FluentTextBox { Dock = DockStyle.Fill, UseSystemPasswordChar = secret };
            object value;
            if (values.TryGetValue(key, out value) && value != null) box.Text = Convert.ToString(value);
            fields[key] = box;
            root.Controls.Add(box, 1, row);
        }

        private static bool CredentialBoolean(Dictionary<string, object> current, string key)
        {
            object value;
            bool parsed;
            return current.TryGetValue(key, out value) && value != null
                && (value is bool ? (bool)value : Boolean.TryParse(Convert.ToString(value), out parsed) && parsed);
        }

        private static bool HasCredentialValue(Dictionary<string, object> current, string key)
        {
            object value;
            return current.TryGetValue(key, out value) && value != null
                && Convert.ToString(value).Trim().Length > 0;
        }

        private async void ValidateClicked(object sender, EventArgs e)
        {
            Dictionary<string, object> current = CurrentValues();
            testButton.Enabled = false;
            validationStatus.Text = "Testing live endpoint...";
            validationStatus.ForeColor = ThemeManager.CurrentPalette.Warning;
            CredentialValidationResult result = await ProviderCredentialValidator.ValidateAsync(provider, state, current);
            if (result.Success)
            {
                successfulValidation = result;
                testedFingerprint = Fingerprint(current);
                validationStatus.Text = "PASSED — " + result.Endpoint;
                validationStatus.ForeColor = ThemeManager.CurrentPalette.Success;
            }
            else
            {
                successfulValidation = null;
                testedFingerprint = null;
                validationStatus.Text = "FAILED — " + result.Message;
                validationStatus.ForeColor = ThemeManager.CurrentPalette.Error;
            }
            testButton.Enabled = true;
            MessageBox.Show(this, result.Message + "\r\n\r\nEndpoint: " + result.Endpoint,
                result.Success ? "Credential test passed" : "Credential test failed",
                MessageBoxButtons.OK, result.Success ? MessageBoxIcon.Information : MessageBoxIcon.Warning);
        }

        private Dictionary<string, object> CurrentValues()
        {
            Dictionary<string, object> current = new Dictionary<string, object>(values, StringComparer.OrdinalIgnoreCase);
            foreach (KeyValuePair<string, TextBox> field in fields) current[field.Key] = field.Value.Text.Trim();
            if (provider == "fanarttv") current["api_version"] = "v3.2";
            if (provider == "musicbrainz") current["oauth_enabled"] = musicBrainzOAuthEnabled != null && musicBrainzOAuthEnabled.Checked;
            return current;
        }

        private string Fingerprint(Dictionary<string, object> current)
        {
            string[] keys = provider == "fanarttv" ? new[] { "api_key", "client_key", "api_version" }
                : provider == "lastfm" ? new[] { "api_key", "shared_secret" }
                : provider == "discogs" ? new[] { "token" }
                : new[] { "oauth_enabled", "client_id", "client_secret", "callback_uri", "oauth_scope", "access_token", "refresh_token" };
            return String.Join("\u001f", keys.Select(key => current.ContainsKey(key) && current[key] != null ? Convert.ToString(current[key]).Trim() : ""));
        }

        private void SaveClicked(object sender, EventArgs e)
        {
            Dictionary<string, object> current = CurrentValues();
            values.Clear();
            foreach (KeyValuePair<string, object> pair in current) values[pair.Key] = pair.Value;
            string currentFingerprint = Fingerprint(values);
            if (!String.Equals(currentFingerprint, initialFingerprint, StringComparison.Ordinal)) CredentialStore.ClearValidationMetadata(values);
            if (successfulValidation != null && String.Equals(currentFingerprint, testedFingerprint, StringComparison.Ordinal))
                CredentialStore.ApplyValidationMetadata(values, successfulValidation);
            if (provider == "lastfm")
            {
                if (!values.ContainsKey("username")) values["username"] = "";
                if (!values.ContainsKey("session_key")) values["session_key"] = "";
                if (!values.ContainsKey("subscriber")) values["subscriber"] = false;
            }
            try
            {
                CredentialStore.Save(state, provider, values);
                DialogResult = DialogResult.OK;
                Close();
            }
            catch (Exception error)
            {
                MessageBox.Show(this, error.Message, "Unable to save credential", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }
    }


    internal sealed class AboutForm : FluentForm
    {
        private readonly PictureBox mark;

        public AboutForm()
        {
            Text = "About S:P:L:I:N:E:D";
            StartPosition = FormStartPosition.CenterParent;
            Size = new Size(590, 390);
            MinimumSize = new Size(540, 360);
            MaximizeBox = false;
            MinimizeBox = false;
            ShowInTaskbar = false;
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;

            TableLayoutPanel root = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                Padding = new Padding(24),
                ColumnCount = 2,
                RowCount = 2
            };
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 92));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 52));
            Controls.Add(root);

            Image iconImage = Icon == null ? null : Icon.ToBitmap();
            mark = new PictureBox
            {
                Width = 72,
                Height = 72,
                Margin = new Padding(0, 8, 16, 0),
                SizeMode = PictureBoxSizeMode.Zoom,
                Image = iconImage
            };
            root.Controls.Add(mark, 0, 0);

            TableLayoutPanel details = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                ColumnCount = 1,
                RowCount = 4,
                Margin = Padding.Empty
            };
            details.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            details.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            details.RowStyles.Add(new RowStyle(SizeType.Absolute, 82));
            details.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            details.Controls.Add(new Label
            {
                Text = "S:P:L:I:N:E:D",
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.AppTitle),
                TextAlign = ContentAlignment.MiddleLeft
            }, 0, 0);
            details.Controls.Add(new Label
            {
                Text = ReleaseInfo.VersionLabel,
                Dock = DockStyle.Fill,
                Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle),
                TextAlign = ContentAlignment.MiddleLeft
            }, 0, 1);
            details.Controls.Add(new Label
            {
                Text = "Native Windows artwork discovery, evaluation, review,\r\nand installation for music libraries.",
                Dock = DockStyle.Fill,
                AutoEllipsis = true
            }, 0, 2);
            details.Controls.Add(new Label
            {
                Text = "Config v5  •  Portable credential-directory architecture\r\n•  Fluent Compact + SPLINED identity",
                Dock = DockStyle.Fill,
                ForeColor = ThemeManager.CurrentPalette.TextSecondary
            }, 0, 3);
            root.Controls.Add(details, 1, 0);

            FlowLayoutPanel actions = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.RightToLeft,
                WrapContents = false,
                Padding = new Padding(0, 7, 0, 0)
            };
            Button close = new FluentButton { Text = "Close", Width = 120, Height = 34, Tag = "primary" };
            Button help = new FluentButton { Text = "Help", Width = 120, Height = 34 };
            close.Click += delegate { Close(); };
            help.Click += delegate { HelpWindows.OpenHelp(this); };
            actions.Controls.Add(close);
            actions.Controls.Add(help);
            root.Controls.Add(actions, 0, 1);
            root.SetColumnSpan(actions, 2);

            AcceptButton = close;
            CancelButton = close;
            FormClosed += delegate { if (mark.Image != null) mark.Image.Dispose(); };
            ThemeManager.Apply(this, ThemeManager.CurrentTheme);
            ThemeManager.PrepareForFirstShow(this, ThemeManager.CurrentTheme);
        }
    }

    internal sealed class StatusForm : FluentForm
    {
        private readonly ConfigState state;
        private readonly CheckBox hideOnLaunch;

        public StatusForm(ConfigState state)
        {
            this.state = state;
            Text = "SPLINED - Status";
            StartPosition = FormStartPosition.CenterParent;
            Size = new Size(700, 590);
            MinimumSize = new Size(620, 520);
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(20), ColumnCount = 4, RowCount = 11 };
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 31));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 24));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 20));
            root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 25));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 44));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 55));
            for (int i = 2; i < 9; i++) root.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 48));
            Controls.Add(root);
            Label title = new Label { Text = "SPLINED Status", Dock = DockStyle.Fill, Font = ThemeManager.UiFont(ThemeFontRole.AppTitle) };
            root.Controls.Add(title, 0, 0); root.SetColumnSpan(title, 4);
            Label summary = new Label { Text = "READY needs no credential. SAVED only means local data exists; TESTED means the provider accepted a live request.", Dock = DockStyle.Fill };
            root.Controls.Add(summary, 0, 1); root.SetColumnSpan(summary, 4);
            string[,] providers = { { "iTunes", "itunes" }, { "Discogs", "discogs" }, { "Last.fm", "lastfm" }, { "Fanart.tv", "fanarttv" }, { "Cover Art Archive", "coverartarchive" }, { "Deezer", "deezer" }, { "MusicBrainz OAuth", "musicbrainz" } };
            for (int index = 0; index < providers.GetLength(0); index++)
            {
                string key = providers[index, 1];
                string providerKey = key;
                string providerDisplay = providers[index, 0];
                string status = CredentialStore.Status(state, key);
                root.Controls.Add(new Label { Text = providers[index, 0], Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 0, index + 2);
                root.Controls.Add(new Label { Name = "providerStatusLabel", Tag = status, Text = "[" + status + "]", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft }, 1, index + 2);
                if (key == "lastfm" || key == "fanarttv" || key == "discogs" || key == "musicbrainz")
                {
                    LinkLabel test = new LinkLabel { Name = "status" + key + "TestLink", Text = "Test saved", Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleLeft,
                        Enabled = status != "PENDING" && status != "DISABLED" && status != "INCOMPLETE" && status != "AUTH NEEDED" };
                    CredentialStore.StyleTestLink(test);
                    Label liveStatus = (Label)root.GetControlFromPosition(1, index + 2);
                    test.LinkClicked += async delegate
                    {
                        test.Enabled = false;
                        liveStatus.Text = "[TESTING]";
                        Dictionary<string, object> saved = CredentialStore.Load(this.state, providerKey);
                        CredentialValidationResult result = await ProviderCredentialValidator.ValidateAsync(providerKey, this.state, saved);
                        if (result.Success)
                        {
                            CredentialStore.ApplyValidationMetadata(saved, result);
                            CredentialStore.Save(this.state, providerKey, saved);
                        }
                        liveStatus.Text = "[" + CredentialStore.Status(this.state, providerKey) + "]";
                        liveStatus.ForeColor = result.Success ? CredentialStore.StatusColor(CredentialStore.Status(this.state, providerKey)) : ThemeManager.CurrentPalette.Error;
                        test.Enabled = true;
                        MessageBox.Show(this, result.Message + "\r\n\r\nEndpoint: " + result.Endpoint,
                            result.Success ? providerDisplay + " credential passed" : providerDisplay + " credential failed",
                            MessageBoxButtons.OK, result.Success ? MessageBoxIcon.Information : MessageBoxIcon.Warning);
                    };
                    root.Controls.Add(test, 2, index + 2);
                    Button configure = new FluentButton { Text = status == "PENDING" ? "Configure" : "Change", Dock = DockStyle.Fill, Margin = new Padding(18, 6, 18, 6), Tag = key };
                    configure.Click += delegate(object sender, EventArgs args) { using (CredentialsForm form = new CredentialsForm(this.state)) form.ShowDialog(this); Close(); };
                    root.Controls.Add(configure, 3, index + 2);
                }
            }
            hideOnLaunch = new FluentCheckBox { Text = "Do not show Status on launch", Dock = DockStyle.Fill, Checked = !ConfigStore.LoadUi().ShowStatusOnLaunch };
            root.Controls.Add(hideOnLaunch, 0, 10); root.SetColumnSpan(hideOnLaunch, 3);
            Button close = new FluentButton { Text = "Close", Dock = DockStyle.Fill, Margin = new Padding(18, 6, 18, 6) };
            close.Click += CloseClicked;
            root.Controls.Add(close, 3, 10);
            ThemeManager.Apply(this, ThemeManager.CurrentTheme);
            foreach (Label label in Controls.Find("providerStatusLabel", true).OfType<Label>())
                label.ForeColor = CredentialStore.StatusColor(Convert.ToString(label.Tag));
            ThemeManager.PrepareForFirstShow(this, ThemeManager.CurrentTheme);
        }

        private void CloseClicked(object sender, EventArgs e)
        {
            UiState ui = ConfigStore.LoadUi();
            ui.ShowStatusOnLaunch = !hideOnLaunch.Checked;
            ConfigStore.SaveUi(ui);
            Close();
        }
    }
}
