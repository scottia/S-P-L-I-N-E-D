namespace Splined.WindowsGui
{
    /// <summary>Single authoritative Windows GUI release identity.</summary>
    internal static class ReleaseInfo
    {
        public const string NumericVersion = "3.0.0";
        public const string SemanticVersion = "3.0.0";
        public const string Channel = "Stable";
        public const string PackageName = "SPLINED-Windows-GUI-3.0.0-Stable-v3";
        public const string WindowTitle = "S:P:L:I:N:E:D";
        public const string VersionLabel = "v" + NumericVersion + " " + Channel;
        public const string DisplayName = WindowTitle + " " + VersionLabel;
        public const string RepositoryUrl = "https://github.com/scottia/S-P-L-I-N-E-D";
        public const string HelpUrl = RepositoryUrl + "/blob/main/docs/README.md";
        public const string ReleasesUrl = RepositoryUrl + "/releases/latest";
    }
}
