using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;

namespace Splined.WindowsGui
{
    internal sealed class BackupSelection
    {
        public bool Settings = true;
        public bool Credentials = true;
        public bool Database;
        public bool Interface = true;
        public bool Diagnostics;
    }

    internal sealed class SplinedBackupPayload
    {
        public int version = 1;
        public string created_utc;
        public string splined_version;
        public string settings;
        public string interface_settings;
        public Dictionary<string, string> credentials = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        public string database;
        public string diagnostics;
    }

    internal sealed class SplinedBackupEnvelope
    {
        public string format = "SPLINED-BACKUP";
        public int version = 1;
        public bool password_protected;
        public int iterations;
        public string salt;
        public string iv;
        public string payload;
        public string authentication;
    }

    internal static class BackupService
    {
        private const int PasswordIterations = 150000;
        private const string FileClass = "SPLINED.Backup.v1";
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = Int32.MaxValue, RecursionLimit = 128 };

        public static void RegisterFileAssociation()
        {
            string executable = Path.Combine(ConfigStore.AppRoot, "splined.exe");
            if (!File.Exists(executable)) return;
            try
            {
                using (RegistryKey extension = Registry.CurrentUser.CreateSubKey(@"Software\Classes\.spl"))
                    if (extension != null) extension.SetValue("", FileClass, RegistryValueKind.String);
                using (RegistryKey fileClass = Registry.CurrentUser.CreateSubKey(@"Software\Classes\" + FileClass))
                {
                    if (fileClass == null) return;
                    fileClass.SetValue("", "SPLINED backup", RegistryValueKind.String);
                    using (RegistryKey icon = fileClass.CreateSubKey("DefaultIcon"))
                        if (icon != null) icon.SetValue("", "\"" + executable + "\",0", RegistryValueKind.String);
                    using (RegistryKey command = fileClass.CreateSubKey(@"shell\open\command"))
                        if (command != null) command.SetValue("", "\"" + executable + "\" \"%1\"", RegistryValueKind.String);
                }
            }
            catch
            {
                // File association is convenience only; settings and backup remain usable.
            }
        }

        public static void Export(string path, ConfigState state, UiState uiState, BackupSelection selection, string password)
        {
            if (String.IsNullOrWhiteSpace(path)) throw new InvalidOperationException("Choose a backup destination.");
            if (!path.EndsWith(".spl", StringComparison.OrdinalIgnoreCase)) path += ".spl";
            SplinedBackupPayload payload = new SplinedBackupPayload
            {
                created_utc = DateTime.UtcNow.ToString("o"),
                splined_version = ReleaseInfo.SemanticVersion,
                settings = selection.Settings ? ConfigStore.ExportConfigText(state) : null,
                interface_settings = selection.Interface ? ConfigStore.ExportUiText(uiState) : null,
                diagnostics = selection.Diagnostics ? "verbosity=" + state.Verbosity + ";log_retention_days=" + state.LogRetentionDays : null
            };
            if (selection.Credentials && Directory.Exists(state.CredentialDir))
            {
                foreach (string file in Directory.GetFiles(state.CredentialDir, "*.json", SearchOption.TopDirectoryOnly))
                    payload.credentials[Path.GetFileName(file)] = Convert.ToBase64String(File.ReadAllBytes(file));
            }
            if (selection.Database)
            {
                string database = Path.Combine(state.CacheDir, "splined.db");
                if (File.Exists(database)) payload.database = Convert.ToBase64String(File.ReadAllBytes(database));
            }

            byte[] plain = Encoding.UTF8.GetBytes(Json.Serialize(payload));
            SplinedBackupEnvelope envelope = Protect(plain, password ?? "");
            string parent = Path.GetDirectoryName(Path.GetFullPath(path));
            if (!String.IsNullOrWhiteSpace(parent)) Directory.CreateDirectory(parent);
            ConfigStore.WriteTextAtomic(path, Json.Serialize(envelope));
        }

        public static SplinedBackupPayload Read(string path, string password)
        {
            if (!File.Exists(path)) throw new FileNotFoundException("SPLINED backup was not found.", path);
            SplinedBackupEnvelope envelope = Json.Deserialize<SplinedBackupEnvelope>(File.ReadAllText(path, Encoding.UTF8));
            if (envelope == null || envelope.format != "SPLINED-BACKUP" || envelope.version != 1)
                throw new InvalidDataException("This is not a supported SPLINED .spl backup.");
            byte[] plain = Unprotect(envelope, password ?? "");
            SplinedBackupPayload payload = Json.Deserialize<SplinedBackupPayload>(Encoding.UTF8.GetString(plain));
            if (payload == null || payload.version != 1) throw new InvalidDataException("The SPLINED backup payload is invalid.");
            return payload;
        }

        public static void Restore(SplinedBackupPayload payload, BackupSelection selection, ConfigState current)
        {
            if (payload == null) throw new ArgumentNullException("payload");
            if (selection.Settings && !String.IsNullOrWhiteSpace(payload.settings))
                ConfigStore.ImportConfigText(payload.settings, true);
            ConfigState destination = ConfigStore.HasSavedSettings ? ConfigStore.Load() : current;
            if (selection.Interface && !String.IsNullOrWhiteSpace(payload.interface_settings))
                ConfigStore.ImportUiText(payload.interface_settings);
            if (selection.Credentials && payload.credentials != null && payload.credentials.Count > 0)
            {
                Directory.CreateDirectory(destination.CredentialDir);
                foreach (KeyValuePair<string, string> item in payload.credentials)
                {
                    string safeName = Path.GetFileName(item.Key);
                    if (!String.Equals(item.Key, safeName, StringComparison.Ordinal) || !safeName.EndsWith(".json", StringComparison.OrdinalIgnoreCase))
                        throw new InvalidDataException("Backup contains an unsafe credential filename.");
                    WriteBytesAtomic(Path.Combine(destination.CredentialDir, safeName), Convert.FromBase64String(item.Value));
                }
            }
            if (selection.Database && !String.IsNullOrWhiteSpace(payload.database))
            {
                Directory.CreateDirectory(destination.CacheDir);
                WriteBytesAtomic(Path.Combine(destination.CacheDir, "splined.db"), Convert.FromBase64String(payload.database));
            }
        }

        private static SplinedBackupEnvelope Protect(byte[] plain, string password)
        {
            SplinedBackupEnvelope envelope = new SplinedBackupEnvelope();
            if (password.Length == 0)
            {
                envelope.payload = Convert.ToBase64String(plain);
                using (SHA256 hash = SHA256.Create()) envelope.authentication = Convert.ToBase64String(hash.ComputeHash(plain));
                return envelope;
            }

            envelope.password_protected = true;
            envelope.iterations = PasswordIterations;
            byte[] salt = RandomBytes(16);
            byte[] iv = RandomBytes(16);
            byte[] keyMaterial;
            using (Rfc2898DeriveBytes derive = new Rfc2898DeriveBytes(password, salt, PasswordIterations)) keyMaterial = derive.GetBytes(64);
            byte[] cipher;
            using (Aes aes = Aes.Create())
            {
                aes.KeySize = 256;
                aes.Mode = CipherMode.CBC;
                aes.Padding = PaddingMode.PKCS7;
                aes.Key = keyMaterial.Take(32).ToArray();
                aes.IV = iv;
                using (ICryptoTransform encryptor = aes.CreateEncryptor()) cipher = encryptor.TransformFinalBlock(plain, 0, plain.Length);
            }
            byte[] authenticated = iv.Concat(cipher).ToArray();
            byte[] mac;
            using (HMACSHA256 hmac = new HMACSHA256(keyMaterial.Skip(32).Take(32).ToArray())) mac = hmac.ComputeHash(authenticated);
            envelope.salt = Convert.ToBase64String(salt);
            envelope.iv = Convert.ToBase64String(iv);
            envelope.payload = Convert.ToBase64String(cipher);
            envelope.authentication = Convert.ToBase64String(mac);
            Array.Clear(keyMaterial, 0, keyMaterial.Length);
            return envelope;
        }

        private static byte[] Unprotect(SplinedBackupEnvelope envelope, string password)
        {
            byte[] payload = Convert.FromBase64String(envelope.payload ?? "");
            if (!envelope.password_protected)
            {
                byte[] expected = Convert.FromBase64String(envelope.authentication ?? "");
                byte[] actual;
                using (SHA256 hash = SHA256.Create()) actual = hash.ComputeHash(payload);
                if (!FixedTimeEquals(expected, actual)) throw new InvalidDataException("The SPLINED backup failed its integrity check.");
                return payload;
            }
            if (password.Length == 0) throw new InvalidOperationException("This SPLINED backup requires a password.");
            byte[] salt = Convert.FromBase64String(envelope.salt ?? "");
            byte[] iv = Convert.FromBase64String(envelope.iv ?? "");
            byte[] keyMaterial;
            using (Rfc2898DeriveBytes derive = new Rfc2898DeriveBytes(password, salt, envelope.iterations)) keyMaterial = derive.GetBytes(64);
            byte[] expectedMac = Convert.FromBase64String(envelope.authentication ?? "");
            byte[] actualMac;
            using (HMACSHA256 hmac = new HMACSHA256(keyMaterial.Skip(32).Take(32).ToArray())) actualMac = hmac.ComputeHash(iv.Concat(payload).ToArray());
            if (!FixedTimeEquals(expectedMac, actualMac)) throw new InvalidOperationException("The backup password is incorrect or the backup is damaged.");
            try
            {
                using (Aes aes = Aes.Create())
                {
                    aes.KeySize = 256;
                    aes.Mode = CipherMode.CBC;
                    aes.Padding = PaddingMode.PKCS7;
                    aes.Key = keyMaterial.Take(32).ToArray();
                    aes.IV = iv;
                    using (ICryptoTransform decryptor = aes.CreateDecryptor()) return decryptor.TransformFinalBlock(payload, 0, payload.Length);
                }
            }
            finally { Array.Clear(keyMaterial, 0, keyMaterial.Length); }
        }

        private static bool FixedTimeEquals(byte[] left, byte[] right)
        {
            if (left == null || right == null || left.Length != right.Length) return false;
            int difference = 0;
            for (int index = 0; index < left.Length; index++) difference |= left[index] ^ right[index];
            return difference == 0;
        }

        private static byte[] RandomBytes(int length)
        {
            byte[] bytes = new byte[length];
            using (RandomNumberGenerator random = RandomNumberGenerator.Create()) random.GetBytes(bytes);
            return bytes;
        }

        private static void WriteBytesAtomic(string path, byte[] bytes)
        {
            string parent = Path.GetDirectoryName(path);
            Directory.CreateDirectory(parent);
            string staged = Path.Combine(parent, ".splined-" + Guid.NewGuid().ToString("N") + ".tmp");
            string backup = path + ".splined-backup";
            try
            {
                File.WriteAllBytes(staged, bytes);
                if (File.Exists(backup)) File.Delete(backup);
                if (File.Exists(path)) File.Move(path, backup);
                File.Move(staged, path);
                if (File.Exists(backup)) File.Delete(backup);
            }
            catch
            {
                if (!File.Exists(path) && File.Exists(backup)) File.Move(backup, path);
                throw;
            }
            finally { if (File.Exists(staged)) File.Delete(staged); }
        }
    }

    internal sealed class BackupForm : FluentForm
    {
        private readonly ConfigState state;
        private readonly UiState uiState;
        private readonly bool importMode;
        private readonly TextBox path;
        private readonly TextBox password;
        private readonly CheckBox protect;
        private readonly CheckBox settings;
        private readonly CheckBox credentials;
        private readonly CheckBox database;
        private readonly CheckBox interfaceSettings;
        private readonly CheckBox diagnostics;

        public BackupForm(ConfigState current, UiState ui, bool import, string initialPath)
        {
            state = current;
            uiState = ui;
            importMode = import;
            Text = import ? "SPLINED - Import Backup" : "SPLINED - Export Backup";
            StartPosition = FormStartPosition.CenterParent;
            Size = new Size(700, 520);
            MinimumSize = new Size(620, 460);
            Font = ThemeManager.UiFont(ThemeFontRole.Body);
            AutoScaleMode = AutoScaleMode.Dpi;

            TableLayoutPanel root = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(16), ColumnCount = 1, RowCount = 6 };
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 46));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 74));
            root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 40));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 42));
            root.RowStyles.Add(new RowStyle(SizeType.Absolute, 52));
            Controls.Add(root);
            root.Controls.Add(new Label { Dock = DockStyle.Fill, Font = ThemeManager.UiFont(ThemeFontRole.PanelTitle), Text = import ? "Restore selected components from a .spl backup." : "Create a selective .spl backup." }, 0, 0);

            TableLayoutPanel pathRow = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2 };
            pathRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            pathRow.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 105));
            path = new FluentTextBox { Dock = DockStyle.Fill, Text = initialPath ?? "" };
            Button browse = new FluentButton { Dock = DockStyle.Fill, Text = "Browse..." };
            browse.Click += BrowseClicked;
            pathRow.Controls.Add(path, 0, 0); pathRow.Controls.Add(browse, 1, 0);
            root.Controls.Add(pathRow, 0, 1);

            GroupBox components = new FluentGroupBox { Text = import ? "Restore components" : "Include components", Dock = DockStyle.Fill, Padding = new Padding(12) };
            TableLayoutPanel choices = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 3 };
            choices.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50)); choices.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
            settings = Choice("Portable Config v5 settings", true);
            credentials = Choice("Provider credentials", true);
            database = Choice("SQLite media database", false);
            interfaceSettings = Choice("Interface preferences and layout", true);
            diagnostics = Choice("Diagnostic configuration", false);
            choices.Controls.Add(settings, 0, 0); choices.Controls.Add(credentials, 1, 0);
            choices.Controls.Add(database, 0, 1); choices.Controls.Add(interfaceSettings, 1, 1);
            choices.Controls.Add(diagnostics, 0, 2);
            components.Controls.Add(choices); root.Controls.Add(components, 0, 2);

            protect = new FluentCheckBox { Text = import ? "Backup is password protected" : "Protect backup with a password", Dock = DockStyle.Fill, Checked = false, Enabled = !import };
            root.Controls.Add(protect, 0, 3);
            password = new FluentTextBox { Dock = DockStyle.Fill, UseSystemPasswordChar = true, Enabled = import };
            password.TextChanged += delegate { };
            protect.CheckedChanged += delegate { password.Enabled = protect.Checked || importMode; };
            root.Controls.Add(password, 0, 4);

            FlowLayoutPanel actions = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft, WrapContents = false, Padding = new Padding(0, 8, 0, 0) };
            Button run = new FluentButton { Text = import ? "Validate and Restore" : "Export .spl Backup", Width = 165, Height = 34, Tag = "primary" };
            Button cancel = new FluentButton { Text = "Cancel", Width = 100, Height = 34, DialogResult = DialogResult.Cancel };
            run.Click += RunClicked;
            actions.Controls.Add(run); actions.Controls.Add(cancel); root.Controls.Add(actions, 0, 5);
            AcceptButton = run; CancelButton = cancel;
            ThemeManager.Apply(this, uiState.Theme);
            ThemeManager.PrepareForFirstShow(this, uiState.Theme);
        }

        private static CheckBox Choice(string text, bool value)
        {
            return new FluentCheckBox { Text = text, Checked = value, Dock = DockStyle.Fill };
        }

        private BackupSelection Selection()
        {
            return new BackupSelection { Settings = settings.Checked, Credentials = credentials.Checked, Database = database.Checked, Interface = interfaceSettings.Checked, Diagnostics = diagnostics.Checked };
        }

        private void BrowseClicked(object sender, EventArgs e)
        {
            if (importMode)
            {
                using (OpenFileDialog dialog = new OpenFileDialog { Filter = "SPLINED backup (*.spl)|*.spl|All files (*.*)|*.*", CheckFileExists = true })
                    if (dialog.ShowDialog(this) == DialogResult.OK) path.Text = dialog.FileName;
            }
            else
            {
                using (SaveFileDialog dialog = new SaveFileDialog { Filter = "SPLINED backup (*.spl)|*.spl", DefaultExt = "spl", AddExtension = true, FileName = "splined-" + DateTime.Now.ToString("yyyy-MM-dd") + ".spl" })
                    if (dialog.ShowDialog(this) == DialogResult.OK) path.Text = dialog.FileName;
            }
        }

        private void RunClicked(object sender, EventArgs e)
        {
            try
            {
                if (importMode)
                {
                    SplinedBackupPayload payload = BackupService.Read(path.Text.Trim(), password.Text);
                    BackupService.Restore(payload, Selection(), state);
                    MessageBox.Show(this, "Selected SPLINED settings and files were restored.", "SPLINED backup", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
                else
                {
                    if (protect.Checked && password.Text.Length == 0) throw new InvalidOperationException("Enter a password or turn off password protection.");
                    BackupService.Export(path.Text.Trim(), state, uiState, Selection(), protect.Checked ? password.Text : "");
                    MessageBox.Show(this, "SPLINED backup created successfully.", "SPLINED backup", MessageBoxButtons.OK, MessageBoxIcon.Information);
                }
                DialogResult = DialogResult.OK;
                Close();
            }
            catch (Exception error)
            {
                MessageBox.Show(this, error.Message, "SPLINED backup", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }
    }
}
