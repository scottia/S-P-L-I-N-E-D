using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using System.Web.Script.Serialization;

namespace Splined.WindowsGui
{
    internal sealed class NavidromeCredential
    {
        public string ServerUrl = "";
        public string Username = "";
        public string Password = "";
        public string ApiKey = "";
        public string BearerToken = "";

        public static NavidromeCredential FromValues(Dictionary<string, object> values)
        {
            return new NavidromeCredential
            {
                ServerUrl = Value(values, "server_url").TrimEnd('/'),
                Username = Value(values, "username"),
                Password = Value(values, "password"),
                ApiKey = Value(values, "api_key"),
                BearerToken = Value(values, "bearer_token")
            };
        }

        public void Validate()
        {
            Uri uri;
            if (!Uri.TryCreate(ServerUrl, UriKind.Absolute, out uri)
                || (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps))
                throw new InvalidOperationException("Navidrome Server URL must be an absolute HTTP or HTTPS address.");
            if (String.IsNullOrWhiteSpace(BearerToken))
                throw new InvalidOperationException("Navidrome Bearer Token is required by SPLINED.");
            // The required bearer header is a complete Navidrome authentication
            // path. API-key and username/password fields are optional compatible
            // OpenSubsonic alternatives and are never substituted into that header.
        }

        private static string Value(Dictionary<string, object> values, string key)
        {
            object value;
            return values != null && values.TryGetValue(key, out value) && value != null
                ? Convert.ToString(value).Trim() : "";
        }
    }

    internal sealed class NavidromeCapabilities
    {
        public bool Connected;
        public bool LibraryAvailable;
        public bool ArtistArtworkAvailable;
        public bool AlbumArtworkAvailable;
        public bool PathEnrichmentAvailable;
        public bool OpenSubsonic;
        public string ServerType = "";
        public string ServerVersion = "";

        public string UserMessage()
        {
            return (Connected ? "✓ Server connected" : "✗ Server connection failed")
                + Environment.NewLine + (LibraryAvailable ? "✓ Library available" : "✗ Library unavailable")
                + Environment.NewLine + (ArtistArtworkAvailable ? "✓ Artist artwork available" : "○ Artist artwork not reported")
                + Environment.NewLine + (AlbumArtworkAvailable ? "✓ Album artwork available" : "○ Album artwork not reported")
                + Environment.NewLine + (PathEnrichmentAvailable ? "✓ Path enrichment available" : "○ Path enrichment not available");
        }
    }

    internal sealed class NavidromeArtist
    {
        public string Id = "";
        public string Name = "";
        public string CoverArtId = "";
        public string ArtistImageUrl = "";
        public string MusicBrainzId = "";
        public int AlbumCount;
    }

    internal sealed class NavidromeAlbum
    {
        public string Id = "";
        public string ArtistId = "";
        public string Artist = "";
        public string Title = "";
        public string CoverArtId = "";
        public string MusicBrainzId = "";
        public string Year = "";
        public string Path = "";
        public int SongCount;
    }

    /// <summary>
    /// Read-only OpenSubsonic-first Navidrome projection used by the Windows
    /// shell. It never writes workflow state and never logs credential values.
    /// </summary>
    internal sealed class NavidromeClient
    {
        private const string ClientName = "SPLINED";
        private const string ProtocolVersion = "1.16.1";
        private readonly NavidromeCredential credential;
        private readonly JavaScriptSerializer json = new JavaScriptSerializer { MaxJsonLength = Int32.MaxValue };

        public NavidromeClient(NavidromeCredential credential)
        {
            if (credential == null) throw new ArgumentNullException("credential");
            credential.Validate();
            this.credential = credential;
        }

        public static bool IsConfigured(ConfigState state)
        {
            try
            {
                NavidromeCredential value = NavidromeCredential.FromValues(
                    CredentialStore.Load(state, "navidrome"));
                value.Validate();
                return true;
            }
            catch { return false; }
        }

        public static NavidromeClient FromState(ConfigState state)
        {
            return new NavidromeClient(NavidromeCredential.FromValues(
                CredentialStore.Load(state, "navidrome")));
        }

        public Task<NavidromeCapabilities> TestCapabilitiesAsync()
        {
            return Task.Run(delegate
            {
                Dictionary<string, object> ping = RequestObject("ping", null);
                NavidromeCapabilities capabilities = new NavidromeCapabilities
                {
                    Connected = true,
                    OpenSubsonic = BooleanValue(ping, "openSubsonic"),
                    ServerType = Text(ping, "type"),
                    ServerVersion = Text(ping, "serverVersion")
                };
                Dictionary<string, object> folders = RequestObject("getMusicFolders", null);
                capabilities.LibraryAvailable = NestedItems(folders, "musicFolders", "musicFolder").Count > 0;
                List<NavidromeArtist> artists = GetArtists();
                capabilities.ArtistArtworkAvailable = artists.Any(artist =>
                    !String.IsNullOrWhiteSpace(artist.CoverArtId)
                    || !String.IsNullOrWhiteSpace(artist.ArtistImageUrl));
                NavidromeArtist first = artists.FirstOrDefault();
                if (first != null)
                {
                    List<NavidromeAlbum> albums = GetArtistAlbums(first.Id);
                    capabilities.AlbumArtworkAvailable = albums.Any(candidateAlbum =>
                        !String.IsNullOrWhiteSpace(candidateAlbum.CoverArtId));
                    NavidromeAlbum firstAlbum = albums.FirstOrDefault();
                    if (firstAlbum != null)
                    {
                        Dictionary<string, object> details = RequestObject("getAlbum",
                            new Dictionary<string, string> { { "id", firstAlbum.Id } });
                        Dictionary<string, object> albumObject = ObjectValue(details, "album");
                        List<Dictionary<string, object>> songs = ArrayObjects(albumObject, "song");
                        capabilities.PathEnrichmentAvailable = songs.Any(song =>
                            !String.IsNullOrWhiteSpace(Text(song, "path")));
                    }
                }
                return capabilities;
            });
        }

        public Task<List<NavidromeArtist>> GetArtistsAsync()
        {
            return Task.Run((Func<List<NavidromeArtist>>)GetArtists);
        }

        public Task<List<NavidromeAlbum>> GetArtistAlbumsAsync(string artistId)
        {
            return Task.Run(delegate { return GetArtistAlbums(artistId); });
        }

        public Task<string> CacheCoverArtAsync(string coverArtId, string cacheDirectory,
            int size)
        {
            return Task.Run(delegate
            {
                if (String.IsNullOrWhiteSpace(coverArtId)) return "";
                Directory.CreateDirectory(cacheDirectory);
                string identity = StableHash(credential.ServerUrl + "\n" + coverArtId
                    + "\n" + size.ToString(CultureInfo.InvariantCulture));
                string path = Path.Combine(cacheDirectory, identity + ".img");
                if (File.Exists(path) && new FileInfo(path).Length > 0) return path;
                Dictionary<string, string> parameters = new Dictionary<string, string>
                {
                    { "id", coverArtId }
                };
                if (size > 0) parameters["size"] = size.ToString(CultureInfo.InvariantCulture);
                byte[] bytes = RequestBytes("getCoverArt", parameters);
                if (bytes == null || bytes.Length == 0) return "";
                string staged = path + ".tmp-" + Guid.NewGuid().ToString("N");
                File.WriteAllBytes(staged, bytes);
                if (File.Exists(path)) File.Delete(path);
                File.Move(staged, path);
                return path;
            });
        }

        internal static NavidromeAlbum MatchAlbum(AlbumInfo local,
            IEnumerable<NavidromeAlbum> remoteAlbums)
        {
            if (local == null || remoteAlbums == null) return null;
            List<NavidromeAlbum> values = remoteAlbums.ToList();
            if (!String.IsNullOrWhiteSpace(local.MusicBrainzReleaseId))
            {
                NavidromeAlbum byId = values.FirstOrDefault(album =>
                    String.Equals(album.MusicBrainzId, local.MusicBrainzReleaseId,
                        StringComparison.OrdinalIgnoreCase));
                if (byId != null) return byId;
            }
            string artist = NormalizeIdentity(local.Artist);
            string title = NormalizeIdentity(local.Title);
            List<NavidromeAlbum> exact = values.Where(album =>
                NormalizeIdentity(album.Artist) == artist
                && NormalizeIdentity(album.Title) == title).ToList();
            if (exact.Count != 1) return null;
            if (!String.IsNullOrWhiteSpace(local.ReleaseYear)
                && !String.IsNullOrWhiteSpace(exact[0].Year)
                && !String.Equals(local.ReleaseYear, exact[0].Year,
                    StringComparison.OrdinalIgnoreCase)) return null;
            return exact[0];
        }

        internal static string NormalizeIdentity(string value)
        {
            if (String.IsNullOrWhiteSpace(value)) return "";
            StringBuilder result = new StringBuilder();
            foreach (char character in value.Normalize(NormalizationForm.FormKC).ToLowerInvariant())
                if (Char.IsLetterOrDigit(character)) result.Append(character);
            return result.ToString();
        }

        private List<NavidromeArtist> GetArtists()
        {
            Dictionary<string, object> root = RequestObject("getArtists", null);
            Dictionary<string, object> artists = ObjectValue(root, "artists");
            List<NavidromeArtist> result = new List<NavidromeArtist>();
            foreach (Dictionary<string, object> index in ArrayObjects(artists, "index"))
                foreach (Dictionary<string, object> value in ArrayObjects(index, "artist"))
                    result.Add(new NavidromeArtist
                    {
                        Id = Text(value, "id"),
                        Name = Text(value, "name"),
                        CoverArtId = Text(value, "coverArt"),
                        ArtistImageUrl = Text(value, "artistImageUrl"),
                        MusicBrainzId = Text(value, "musicBrainzId"),
                        AlbumCount = Integer(value, "albumCount")
                    });
            return result;
        }

        private List<NavidromeAlbum> GetArtistAlbums(string artistId)
        {
            Dictionary<string, object> root = RequestObject("getArtist",
                new Dictionary<string, string> { { "id", artistId ?? "" } });
            Dictionary<string, object> artist = ObjectValue(root, "artist");
            return ArrayObjects(artist, "album").Select(value => new NavidromeAlbum
            {
                Id = Text(value, "id"),
                ArtistId = Text(value, "artistId"),
                Artist = Text(value, "artist"),
                Title = FirstNonEmpty(Text(value, "album"), Text(value, "title"), Text(value, "name")),
                CoverArtId = Text(value, "coverArt"),
                MusicBrainzId = Text(value, "musicBrainzId"),
                Year = Text(value, "year"),
                SongCount = Integer(value, "songCount")
            }).ToList();
        }

        private Dictionary<string, object> RequestObject(string method,
            Dictionary<string, string> parameters)
        {
            byte[] bytes = Request(method, parameters, "application/json");
            string body = Encoding.UTF8.GetString(bytes ?? new byte[0]);
            Dictionary<string, object> document = json.DeserializeObject(body)
                as Dictionary<string, object>;
            Dictionary<string, object> response = ObjectValue(document, "subsonic-response");
            if (!String.Equals(Text(response, "status"), "ok",
                StringComparison.OrdinalIgnoreCase))
            {
                Dictionary<string, object> error = ObjectValue(response, "error");
                throw new InvalidOperationException("Navidrome/OpenSubsonic request failed"
                    + (String.IsNullOrWhiteSpace(Text(error, "message")) ? "."
                        : ": " + Text(error, "message")));
            }
            return response;
        }

        private byte[] RequestBytes(string method, Dictionary<string, string> parameters)
        {
            return Request(method, parameters, "*/*");
        }

        private byte[] Request(string method, Dictionary<string, string> parameters,
            string accept)
        {
            ServicePointManager.SecurityProtocol |= (SecurityProtocolType)3072;
            Uri uri = BuildRestUri(method, parameters);
            HttpWebRequest request = (HttpWebRequest)WebRequest.Create(uri);
            request.Method = "GET";
            request.Accept = accept;
            request.UserAgent = "SPLINED-Windows-GUI/" + ReleaseInfo.SemanticVersion;
            request.Timeout = 20000;
            request.ReadWriteTimeout = 20000;
            request.Headers["X-ND-Authorization"] = "Bearer " + credential.BearerToken;
            try
            {
                using (HttpWebResponse response = (HttpWebResponse)request.GetResponse())
                using (Stream stream = response.GetResponseStream())
                using (MemoryStream buffer = new MemoryStream())
                {
                    stream.CopyTo(buffer);
                    return buffer.ToArray();
                }
            }
            catch (WebException error)
            {
                HttpWebResponse response = error.Response as HttpWebResponse;
                int status = response == null ? 0 : (int)response.StatusCode;
                if (response != null) response.Dispose();
                throw new InvalidOperationException(status == 401 || status == 403
                    ? "Navidrome rejected the saved authentication credentials."
                    : "Unable to reach Navidrome/OpenSubsonic"
                        + (status > 0 ? " (HTTP " + status + ")." : "."));
            }
        }

        private Uri BuildRestUri(string method, Dictionary<string, string> parameters)
        {
            Dictionary<string, string> query = new Dictionary<string, string>
            {
                { "v", ProtocolVersion },
                { "c", ClientName },
                { "f", "json" }
            };
            if (!String.IsNullOrWhiteSpace(credential.ApiKey))
                query["apiKey"] = credential.ApiKey;
            else if (!String.IsNullOrWhiteSpace(credential.Password))
            {
                string salt = Guid.NewGuid().ToString("N").Substring(0, 12);
                query["u"] = credential.Username;
                query["s"] = salt;
                query["t"] = Md5Hex(credential.Password + salt);
            }
            else
            {
                // Bearer-only authentication stays in X-ND-Authorization. Keeping
                // it out of the query prevents secrets from entering URLs, proxy
                // history, or exception messages.
            }
            if (parameters != null)
                foreach (KeyValuePair<string, string> pair in parameters) query[pair.Key] = pair.Value;
            string encoded = String.Join("&", query.Select(pair => Uri.EscapeDataString(pair.Key)
                + "=" + Uri.EscapeDataString(pair.Value ?? "")));
            return new Uri(credential.ServerUrl + "/rest/" + method + ".view?" + encoded);
        }

        private static string Md5Hex(string value)
        {
            using (MD5 md5 = MD5.Create())
                return String.Concat(md5.ComputeHash(Encoding.UTF8.GetBytes(value))
                    .Select(valueByte => valueByte.ToString("x2", CultureInfo.InvariantCulture)));
        }

        private static string StableHash(string value)
        {
            using (SHA256 sha = SHA256.Create())
                return String.Concat(sha.ComputeHash(Encoding.UTF8.GetBytes(value))
                    .Select(valueByte => valueByte.ToString("x2", CultureInfo.InvariantCulture)));
        }

        private static Dictionary<string, object> ObjectValue(
            Dictionary<string, object> values, string key)
        {
            object value = null;
            return values != null && values.TryGetValue(key, out value)
                ? value as Dictionary<string, object> : null;
        }

        private static List<Dictionary<string, object>> ArrayObjects(
            Dictionary<string, object> values, string key)
        {
            object value = null;
            IEnumerable items = values != null && values.TryGetValue(key, out value)
                ? value as IEnumerable : null;
            List<Dictionary<string, object>> result = new List<Dictionary<string, object>>();
            if (items == null || value is string) return result;
            foreach (object item in items)
            {
                Dictionary<string, object> dictionary = item as Dictionary<string, object>;
                if (dictionary != null) result.Add(dictionary);
            }
            return result;
        }

        private static List<Dictionary<string, object>> NestedItems(
            Dictionary<string, object> values, string objectKey, string arrayKey)
        {
            return ArrayObjects(ObjectValue(values, objectKey), arrayKey);
        }

        private static string Text(Dictionary<string, object> values, string key)
        {
            object value;
            return values != null && values.TryGetValue(key, out value) && value != null
                ? Convert.ToString(value, CultureInfo.InvariantCulture) : "";
        }

        private static int Integer(Dictionary<string, object> values, string key)
        {
            int parsed;
            return Int32.TryParse(Text(values, key), NumberStyles.Integer,
                CultureInfo.InvariantCulture, out parsed) ? parsed : 0;
        }

        private static bool BooleanValue(Dictionary<string, object> values, string key)
        {
            bool parsed;
            return Boolean.TryParse(Text(values, key), out parsed) && parsed;
        }

        private static string FirstNonEmpty(params string[] values)
        {
            return values.FirstOrDefault(value => !String.IsNullOrWhiteSpace(value)) ?? "";
        }
    }
}
