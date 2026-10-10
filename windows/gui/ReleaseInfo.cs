namespace Splined.WindowsGui
{
    /// <summary>Single authoritative Windows GUI release identity.</summary>
    internal static class ReleaseInfo
    {
        public const string NumericVersion = "1.0.73";
        public const string SemanticVersion = "1.0.73";
        public const string Channel = "Stable";
        public const string PackageName = "SPLINED-Windows-1.0.73";
        public const string WindowTitle = "S:P:L:I:N:E:D";
        public const string VersionLabel = "v" + NumericVersion + " " + Channel;
        public const string DisplayName = WindowTitle + " " + VersionLabel;
        public const string RepositoryUrl = "https://github.com/scottia/S-P-L-I-N-E-D";
        public const string HelpUrl = RepositoryUrl + "/blob/main/docs/windows-guide.md";
        public const string ReleasesUrl = RepositoryUrl + "/releases/latest";
        public const string StableReleasesApiUrl = "https://api.github.com/repos/scottia/S-P-L-I-N-E-D/releases?per_page=20";
    }
}
