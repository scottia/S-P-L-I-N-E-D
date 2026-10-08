using System;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;

namespace Splined.WindowsGui
{
    internal static class RuntimeLog
    {
        private static readonly object Sync = new object();
        private static readonly Regex Authorization = new Regex(
            "(?i)(\\\"?authorization\\\"?\\s*[:=]\\s*\\\"?)(?:(?:bearer|basic)\\s+)?([^\\s,;\\\"}]+)",
            RegexOptions.Compiled);
        private static readonly Regex Secret = new Regex(
            "(?i)(authorization|access[_-]?token|refresh[_-]?token|client[_-]?secret|api[_-]?key|password)(\\s*[:=]\\s*|\\\"\\s*:\\s*\\\")([^\\s,;\\\"}]+)",
            RegexOptions.Compiled);
        private static string logPath;

        public static string Path { get { lock (Sync) return logPath ?? ""; } }

        public static void Initialize(ConfigState state)
        {
            if (state == null || String.IsNullOrWhiteSpace(state.LogDir)) return;
            lock (Sync)
            {
                string runDirectory = System.IO.Path.Combine(state.LogDir, "run");
                Directory.CreateDirectory(runDirectory);
                // Windows run logs are a single-session diagnostic surface.
                // Clear completed prior sessions before creating this one;
                // SQLite, not log retention, owns Album status/history.
                foreach (string existing in Directory.GetFiles(runDirectory, "splined-*.log"))
                {
                    try { File.Delete(existing); }
                    catch { }
                }
                string verbosity = String.IsNullOrWhiteSpace(state.Verbosity) ? "info" : state.Verbosity.Trim().ToLowerInvariant();
                string timestamp = DateTime.UtcNow.ToString("yyyyMMdd-HHmmss-fffffff");
                logPath = System.IO.Path.Combine(runDirectory,
                    "splined-" + verbosity + "-" + timestamp + "-p" + System.Diagnostics.Process.GetCurrentProcess().Id + ".log");
                File.WriteAllText(logPath, "", new UTF8Encoding(false));
                WriteUnlocked("info", "windows.gui.start mode=" + state.Mode
                    + " verbosity=" + verbosity
                    + " sqlite_shared=" + state.SqliteShared);
            }
        }

        public static void Write(string level, string message)
        {
            lock (Sync)
            {
                if (String.IsNullOrWhiteSpace(logPath)) return;
                WriteUnlocked(level, message);
            }
        }

        internal static string Redact(string message)
        {
            string safe = Authorization.Replace(message ?? "", "$1[REDACTED]");
            return Secret.Replace(safe, "$1$2[REDACTED]");
        }

        private static void WriteUnlocked(string level, string message)
        {
            try
            {
                string safe = Redact(message)
                    .Replace("\r", "\\r")
                    .Replace("\n", "\\n");
                if (safe.Length > 2048) safe = safe.Substring(0, 2026) + "...[TRUNCATED]";
                string record = DateTime.UtcNow.ToString("o") + " "
                    + (String.IsNullOrWhiteSpace(level) ? "info" : level.Trim().ToLowerInvariant())
                    + " " + safe + Environment.NewLine;
                File.AppendAllText(logPath, record, new UTF8Encoding(false));
            }
            catch
            {
                // Logging is diagnostic and must never interrupt artwork or SQL work.
            }
        }
    }
}
