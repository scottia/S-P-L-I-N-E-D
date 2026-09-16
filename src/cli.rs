use clap::{ArgAction, Parser};

fn bare_command_scan_dir() -> bool {
    std::env::args_os().len() == 1
}

#[derive(Parser, Debug)]
#[command(
    name = "splined",
    version,
    about = "SPLINED artwork discovery and evaluation engine",
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Cli {
    /// Open SPLINED configuration
    #[arg(long)]
    pub config: bool,

    /// Scan the configured music library
    #[arg(long, conflicts_with = "scan_dir")]
    pub scan: bool,

    /// Scan the configured scan directory; bare `splined` scans the current directory
    #[arg(long, conflicts_with = "scan", default_value_t = bare_command_scan_dir())]
    pub scan_dir: bool,

    /// Preserve existing artwork instead of replacing it
    #[arg(short = 'p', long, value_name = "BOOL")]
    pub preserve_file: Option<bool>,

    /// Replace configured cover sources for this run
    #[arg(short = 's', long, value_delimiter = ',', value_name = "SOURCE")]
    pub cover_sources: Option<Vec<String>>,

    /// Use only these cover sources for this run
    #[arg(short = 'o', long, value_delimiter = ',', value_name = "SOURCE")]
    pub only_cover_sources: Option<Vec<String>>,

    /// Exclude cover sources for this run
    #[arg(short = 'e', long, value_delimiter = ',', value_name = "SOURCE")]
    pub exclude_cover_sources: Vec<String>,

    /// MusicBrainz release MBID for artwork discovery
    #[arg(long, value_name = "MBID")]
    pub release_mbid: Option<String>,

    /// Authorize SPLINED with MusicBrainz OAuth
    #[arg(long)]
    pub mb_oauth_login: bool,

    /// Configure SPLINED Last.fm API credentials
    #[arg(long)]
    pub lastfm_credentials: bool,

    /// Authorize SPLINED with a Last.fm user account
    #[arg(long)]
    pub lastfm_login: bool,

    /// Configure SPLINED Fanart.tv API credentials
    #[arg(long)]
    pub fanarttv_credentials: bool,

    /// Help, Tips & Config Assistance
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    pub help: bool,

    /// Version
    #[arg(short = 'V', long = "version", action = ArgAction::SetTrue)]
    pub version: bool,
}
