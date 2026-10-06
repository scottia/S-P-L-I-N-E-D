namespace Splined.WindowsGui
{
    // Direct C# QA harnesses do not run the Rust build script that generates
    // BuildInfo.cs. Production builds always use the generated commit/channel.
    internal static class BuildInfo
    {
        public const string Commit = "0000000000000000000000000000000000000000";
        public const string ShortCommit = "0000000";
    }
}
