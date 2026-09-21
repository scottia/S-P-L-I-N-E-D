use clap::Parser;
use splined::candidate::StaticFormat;
use splined::config::{
    Config, Mode, Verbosity, config_path, default_toml, load_config, load_config_from,
    resolve_sources, samples_dir,
};
use splined::config_migration::{MigrationReport, migrate_config_if_needed};
use splined::credentials::{
    FanartTvCredential, LastFmCredential, resolve_credential_path, save_fanarttv_credential,
    save_lastfm_credential,
};
use splined::musicbrainz::{
    MusicBrainzClient, OAuthCredential, load_credential as load_musicbrainz_credential,
    resolve_token_path, save_credential as save_musicbrainz_credential,
};
use splined::pipeline::{candidate_summary, run_registry_pipeline};
use splined::portable::{bootstrap_portable_install, running_as_setup_executable};
use splined::range::Range;
use splined::scan_runtime::run_scan_library_read_report;
use splined::source::{ArtworkQuery, ProviderContext, ProviderRegistry, lastfm::LastFm};
use std::io::{self, Write};

mod cli;
use cli::Cli;

const LASTFM_AUTH_TIMEOUT_SECS: u64 = 60;
const HELP_COLUMN_WIDTH: usize = 38;

fn setup_only_bootstrap() -> Result<bool, String> {
    if !running_as_setup_executable()? {
        return Ok(false);
    }

    let defaults = default_toml()
        .map_err(|error| format!("Unable to generate default SPLINED config: {error}"))?;
    bootstrap_portable_install(&defaults)?;

    // A setup executable is installation machinery only. It must not continue
    // into config migration, credential discovery, scans, or provider activity.
    Ok(true)
}

fn confirm_replace(path: &std::path::Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(true);
    }

    println!("Credential file already exists:");
    println!("{}", path.display());
    println!();
    print!("Replace stored credentials? [y/N]: ");
    io::stdout()
        .flush()
        .map_err(|error| format!("Unable to flush terminal output: {error}"))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| format!("Unable to read confirmation: {error}"))?;

    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn configure_lastfm_credentials(configured_path: &str) -> Result<(), String> {
    let path = resolve_credential_path(configured_path, "Last.fm")?;

    println!("SPLINED Last.fm Credentials");
    println!();
    println!("Credential file:");
    println!("{}", path.display());
    println!();

    if !confirm_replace(&path)? {
        println!("Last.fm credential update cancelled.");
        return Ok(());
    }

    let api_key = rpassword::prompt_password("Last.fm API key: ")
        .map_err(|error| format!("Unable to read Last.fm API key: {error}"))?;
    if api_key.trim().is_empty() {
        return Err("Last.fm API key cannot be empty.".to_string());
    }

    let shared_secret = rpassword::prompt_password("Last.fm shared secret: ")
        .map_err(|error| format!("Unable to read Last.fm shared secret: {error}"))?;
    if shared_secret.trim().is_empty() {
        return Err("Last.fm shared secret cannot be empty.".to_string());
    }

    let credential = LastFmCredential {
        api_key: api_key.trim().to_string(),
        shared_secret: shared_secret.trim().to_string(),
        ..LastFmCredential::default()
    };
    save_lastfm_credential(&path, &credential)?;

    println!();
    println!("Last.fm credential file written successfully.");
    println!("{}", path.display());
    println!("Run --lastfm-login to authorize your Last.fm account.");
    Ok(())
}

fn configure_fanarttv_credentials(configured_path: &str) -> Result<(), String> {
    let path = resolve_credential_path(configured_path, "Fanart.tv")?;

    println!("SPLINED Fanart.tv Credentials");
    println!();
    println!("Credential file:");
    println!("{}", path.display());
    println!();

    if !confirm_replace(&path)? {
        println!("Fanart.tv credential update cancelled.");
        return Ok(());
    }

    let api_key = rpassword::prompt_password("Fanart.tv API key: ")
        .map_err(|error| format!("Unable to read Fanart.tv API key: {error}"))?;
    if api_key.trim().is_empty() {
        return Err("Fanart.tv API key cannot be empty.".to_string());
    }

    let client_key = rpassword::prompt_password("Fanart.tv client key (optional): ")
        .map_err(|error| format!("Unable to read Fanart.tv client key: {error}"))?;

    let credential = FanartTvCredential {
        api_key: api_key.trim().to_string(),
        client_key: client_key.trim().to_string(),
        ..FanartTvCredential::default()
    };
    save_fanarttv_credential(&path, &credential)?;

    println!();
    println!("Fanart.tv v3.2 credential file written successfully.");
    println!("{}", path.display());
    Ok(())
}

fn ensure_musicbrainz_bootstrap(
    config: &splined::musicbrainz::MusicBrainzConfig,
) -> Result<(), String> {
    let path = resolve_token_path(&config.token_file)?;
    let mut credential = if path.exists() {
        load_musicbrainz_credential(&path)?
    } else {
        println!("MusicBrainz OAuth credential file does not exist.");
        println!("Creating:");
        println!("{}", path.display());
        println!();
        OAuthCredential::default()
    };

    if credential.client_id.trim().is_empty() {
        if !config.client_id.trim().is_empty() {
            credential.client_id = config.client_id.trim().to_string();
        } else {
            print!("MusicBrainz application client ID: ");
            io::stdout()
                .flush()
                .map_err(|error| format!("Unable to flush MusicBrainz prompt: {error}"))?;
            let mut client_id = String::new();
            io::stdin()
                .read_line(&mut client_id)
                .map_err(|error| format!("Unable to read MusicBrainz client ID: {error}"))?;
            credential.client_id = client_id.trim().to_string();
        }
    }
    if credential.client_id.is_empty() {
        return Err("MusicBrainz client ID cannot be empty.".to_string());
    }

    if credential.client_secret.trim().is_empty() {
        let client_secret = rpassword::prompt_password("MusicBrainz client secret: ")
            .map_err(|error| format!("Unable to read MusicBrainz client secret: {error}"))?;
        credential.client_secret = client_secret.trim().to_string();
    }
    if credential.client_secret.is_empty() {
        return Err("MusicBrainz client secret cannot be empty.".to_string());
    }

    credential.oauth_enabled = true;
    if credential.callback_uri.trim().is_empty() {
        credential.callback_uri = config.callback_uri.clone();
    }
    if credential.oauth_scope.trim().is_empty() {
        credential.oauth_scope = config.scope.clone();
    }
    save_musicbrainz_credential(&path, &credential)?;

    println!();
    println!("MusicBrainz OAuth credential file created.");
    println!("{}", path.display());
    println!();
    Ok(())
}

fn green_value(value: &str) -> String {
    use std::io::IsTerminal;

    if io::stdout().is_terminal() {
        format!("\x1b[32m[{value}]\x1b[0m")
    } else {
        format!("[{value}]")
    }
}

fn red_value(value: &str) -> String {
    use std::io::IsTerminal;

    if io::stdout().is_terminal() {
        format!("\x1b[31m[{value}]\x1b[0m")
    } else {
        format!("[{value}]")
    }
}

fn effective_verbosity(configured: Verbosity, mode: Mode) -> Verbosity {
    match mode {
        Mode::Read => match configured {
            Verbosity::Error | Verbosity::Warn | Verbosity::Info => Verbosity::Debug,
            value => value,
        },
        Mode::Write => configured,
    }
}

fn lastfm_authorization_pending(error: &str) -> bool {
    error.contains("API error 14:")
}

fn help_row(label: &str, value: &str) {
    println!("  {label:<HELP_COLUMN_WIDTH$}{value}");
}

fn help_cont(value: &str) {
    help_row("", value);
}

fn configured_display(value: &str) -> String {
    if value.trim().is_empty() {
        "not configured".to_string()
    } else {
        value.to_string()
    }
}

fn print_dynamic_help(config: Option<&Config>) {
    let unavailable = "config unavailable".to_string();
    let library = config
        .map(|config| configured_display(&config.library.music_library))
        .unwrap_or_else(|| unavailable.clone());
    let scan_dir = config
        .map(|config| configured_display(&config.scan.scan_library_dir))
        .unwrap_or_else(|| unavailable.clone());
    let cache_dir = config
        .map(|config| config.scan.cache_dir.clone())
        .unwrap_or_else(|| unavailable.clone());
    let sample_dir = config
        .map(|config| samples_dir(&config.scan.cache_dir).display().to_string())
        .unwrap_or_else(|| unavailable.clone());
    let credential_dir = config
        .map(|config| config.credentials.credential_dir.clone())
        .unwrap_or_else(|| unavailable.clone());
    let ignored_subs = config
        .map(|config| config.library.ignored_subs.join(", "))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "none".to_string());
    let config_version = config
        .map(|config| config.config_version.to_string())
        .unwrap_or_else(|| unavailable.clone());
    let mode = config
        .map(|config| format!("{:?}", config.mode).to_ascii_lowercase())
        .unwrap_or_else(|| unavailable.clone());
    let verbosity = config
        .map(|config| format!("{:?}", config.verbosity).to_ascii_lowercase())
        .unwrap_or_else(|| unavailable.clone());
    let library_scan = config
        .map(|config| config.scan.library_scan.to_string())
        .unwrap_or_else(|| unavailable.clone());
    let scan_mode_value = config
        .map(|config| config.scan.scan_mode.to_string())
        .unwrap_or_else(|| unavailable.clone());
    let sample_write = config
        .map(|config| config.samples.sample_write.to_string())
        .unwrap_or_else(|| unavailable.clone());
    let preserve_file = config
        .map(|config| config.output.preserve_file.to_string())
        .unwrap_or_else(|| unavailable.clone());
    let output_file_name = config
        .map(|config| config.output.file_name.clone())
        .unwrap_or_else(|| unavailable.clone());
    let output_formats = config
        .map(|config| config.output.file_formats.join(","))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| unavailable.clone());
    let configured_sources = config
        .map(|config| config.sources.cover_sources.join(","))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| unavailable.clone());
    let configured_exclusions = config
        .map(|config| {
            if config.sources.exclude_cover_sources.is_empty() {
                "none".to_string()
            } else {
                config.sources.exclude_cover_sources.join(",")
            }
        })
        .unwrap_or_else(|| unavailable.clone());

    println!("SPLINED artwork discovery and evaluation engine");
    println!();
    println!("Usage: splined.exe [OPTIONS]");
    println!();

    println!("Media Directories:");
    help_row("Library:", &green_value(&library));
    help_row("Scan Directory:", &green_value(&scan_dir));
    help_row("Cache Directory:", &green_value(&cache_dir));
    help_row("Sample Directory:", &green_value(&sample_dir));
    help_row("Credential Directory:", &green_value(&credential_dir));
    help_row("Ignore Sub-Directories:", &red_value(&ignored_subs));
    println!();

    println!("Configuration:");
    help_row("--config", "Open SPLINED configuration");
    help_row("Config Version", &green_value(&config_version));
    help_row("Mode", &green_value(&mode));
    help_row("Verbosity", &green_value(&verbosity));
    println!();

    println!("System Modes:");
    help_row(
        "read",
        "Review/discovery mode; album directories are never modified",
    );
    help_cont("[samples].sample_write=true may still write selected review samples");
    help_cont("Error/Warn/Info verbosity is elevated to Debug");
    help_row(
        "write",
        "Live album artwork mutation using the selected candidate",
    );
    help_row("library_scan", &green_value(&library_scan));
    help_cont("Configuration switch for --scan; it does not choose Read/Write mode");
    help_row("scan_mode", &green_value(&scan_mode_value));
    help_cont("Configuration switch for --scan-dir; it does not choose Read/Write mode");
    println!();

    println!("Library Scanning:");
    help_row(
        "--scan",
        "Use [library].music_library with the configured top-level mode",
    );
    help_cont("Current --scan path remains the library scan preview/inventory path");
    help_row(
        "--scan-dir",
        "Run the operational scan of [scan].scan_library_dir",
    );
    help_cont("Obeys top-level mode=read/write, samples, preserve-file and output settings");
    println!();

    println!("Samples:");
    help_row("[samples].sample_write", &green_value(&sample_write));
    help_cont("true writes exactly one selected source image per resolved album");
    help_cont("Works in both Read review cycles and Write runs");
    help_row("Sample Directory", &green_value(&sample_dir));
    help_cont("Derived automatically as <[scan].cache_dir>\\samples");
    help_row("Sample Naming", "<artist>.<album>.sample.<extension>");
    help_cont("Samples preserve the selected source bytes before resize/conversion");
    help_row(
        "Sample Lifecycle",
        "Cleared at the beginning of every scan run",
    );
    println!();

    println!("Output:");
    help_row("-p, --preserve-file <BOOL>", &green_value(&preserve_file));
    help_cont("true  -> preserve differing files as cover-(2), cover-(3), ...");
    help_cont("false -> replace the canonical <file_name>.<format> in Write mode");
    help_cont("CLI value overrides [output].preserve_file for the current run");
    help_row("[output].file_name", &green_value(&output_file_name));
    help_row("[output].file_formats", &green_value(&output_formats));
    println!();

    println!("<SOURCE> Tags:");
    help_row(
        "-s, --cover-sources",
        "Replace configured cover sources for this run",
    );
    help_cont(&green_value(&configured_sources));
    help_row(
        "-o, --only-cover-sources",
        "Use only these cover sources for this run",
    );
    help_cont(&green_value("not set"));
    help_row(
        "-e, --exclude-cover-sources",
        "Exclude cover sources for this run",
    );
    help_cont(&green_value(&configured_exclusions));
    println!();

    println!("API/OAuth:");
    help_row(
        "--release-mbid",
        "MusicBrainz release MBID for artwork discovery",
    );
    help_row(
        "--mb-oauth-login",
        "Authorize SPLINED with MusicBrainz OAuth",
    );
    help_row(
        "--lastfm-credentials",
        "Configure SPLINED Last.fm API credentials",
    );
    help_row(
        "--lastfm-login",
        "Authorize SPLINED with a Last.fm user account",
    );
    help_row(
        "--fanarttv-credentials",
        "Configure SPLINED Fanart.tv API credentials",
    );
    println!();

    println!("Help:");
    help_row("-h, --help", "Help, Tips & Config Assistance");
    help_row("-V, --version", "Version");
}

fn report_migration(report: &MigrationReport) {
    if !report.changed() {
        return;
    }

    println!(
        "SPLINED config migration: version {} -> {}",
        report.from_version, report.to_version
    );
    if !report.added_keys.is_empty() {
        println!(
            "Added missing config keys: {}",
            report.added_keys.join(", ")
        );
    }
    println!();
}

fn print_scan_preview(
    label: &str,
    mode: Mode,
    verbosity: Verbosity,
    directory: &str,
    ignored_subs: &[String],
    sources: &[String],
) {
    println!("SPLINED {label}");
    println!();
    println!("Mode:       {:?}", mode);
    println!("Verbosity:  {:?}", verbosity);
    println!(
        "Mutation:   {}",
        if mode == Mode::Write {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("Directory:  {}", configured_display(directory));
    println!("Sources:    {}", sources.join(", "));
    println!(
        "Ignored:    {}",
        if ignored_subs.is_empty() {
            "none".to_string()
        } else {
            ignored_subs.join(", ")
        }
    );
    println!();
    println!("Use --scan-dir for the operational album scan pipeline.");
}

fn ensure_regular_config() -> Result<Config, String> {
    let report = migrate_config_if_needed()?;
    report_migration(&report);
    load_config()
}

fn load_selected_config(cli: &Cli) -> Result<Config, String> {
    match cli.config_path.as_deref() {
        Some(path) => load_config_from(path),
        None => ensure_regular_config(),
    }
}

async fn run_lastfm_login(config: &Config) {
    println!("SPLINED Last.fm Login");
    println!();

    let client = match LastFm::new(&config.lastfm.credential_file) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    let authorization = match client.begin_authorization().await {
        Ok(authorization) => authorization,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    println!("Open this URL in your browser:");
    println!();
    println!("{}", authorization.authorization_url);
    println!();
    println!("Authorize SPLINED with Last.fm. Waiting up to 60 seconds...");
    println!();

    for remaining in (1..=LASTFM_AUTH_TIMEOUT_SECS).rev() {
        print!("\rWaiting for authorization... {remaining:2}s ");
        if let Err(error) = io::stdout().flush() {
            eprintln!("\nUnable to flush console output: {error}");
            return;
        }

        match client.complete_authorization(&authorization).await {
            Ok(identity) => {
                println!("\rLast.fm authorization successful.          ");
                println!("User: {}", identity.username);
                println!(
                    "Subscriber: {}",
                    if identity.subscriber { "yes" } else { "no" }
                );
                return;
            }
            Err(error) if lastfm_authorization_pending(&error) => {}
            Err(error) => {
                eprintln!("\nLast.fm authorization failed: {error}");
                return;
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    eprintln!(
        "\nLast.fm authorization timed out after {LASTFM_AUTH_TIMEOUT_SECS} seconds. Run --lastfm-login to try again."
    );
}

async fn run_musicbrainz_login(config: &Config) {
    println!("SPLINED MusicBrainz OAuth Login");
    println!();

    if let Err(error) = ensure_musicbrainz_bootstrap(&config.musicbrainz) {
        eprintln!("{error}");
        return;
    }

    let client = match MusicBrainzClient::new(&config.musicbrainz) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    let session = match client.begin_authorization() {
        Ok(session) => session,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    println!("Open this URL in your browser:");
    println!();
    println!("{}", session.authorization_url);
    println!();
    println!("Authorize SPLINED, then paste the MusicBrainz authorization code.");
    print!("Authorization code: ");
    if let Err(error) = io::stdout().flush() {
        eprintln!("Unable to flush terminal output: {error}");
        return;
    }

    let mut code = String::new();
    if let Err(error) = io::stdin().read_line(&mut code) {
        eprintln!("Unable to read MusicBrainz authorization code: {error}");
        return;
    }

    let code = code.trim();
    if code.is_empty() {
        eprintln!("MusicBrainz authorization code was empty.");
        return;
    }

    match client.exchange_authorization_code(&session, code).await {
        Ok(()) => {
            println!();
            println!("MusicBrainz OAuth authorization successful.");
            if let Some(path) = client.token_path() {
                println!("Credential file: {}", path.display());
            }
            println!("Request mode: OAuth Bearer");
        }
        Err(error) => eprintln!("MusicBrainz OAuth authorization failed: {error}"),
    }
}

async fn run_release_discovery(config: &Config, resolved_sources: &[String], release_mbid: &str) {
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let format_order = [StaticFormat::Jpeg, StaticFormat::Png, StaticFormat::Webp];

    let musicbrainz = match MusicBrainzClient::new(&config.musicbrainz) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    let release = match musicbrainz.lookup_release(release_mbid).await {
        Ok(release) => release,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    println!("Artist:      {}", release.artist_credit);
    println!("Release:     {}", release.title);
    if let Some(group_title) = release.release_group_title.as_deref() {
        println!("Release Group: {group_title}");
    }
    if let Some(group_id) = release.release_group_id.as_deref() {
        println!("Release Group MBID: {group_id}");
    }

    let query = ArtworkQuery::release(release_mbid);
    let context = ProviderContext {
        artist_credit: release.artist_credit,
        release_title: release.title,
        release_group_mbid: release.release_group_id,
        release_group_title: release.release_group_title,
    };

    let registry = match ProviderRegistry::from_source_order_with_credentials(
        resolved_sources,
        &config.fanarttv.credential_file,
        &config.lastfm.credential_file,
        &std::path::Path::new(&config.credentials.credential_dir)
            .join("discogs.json")
            .to_string_lossy(),
    ) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    if registry.is_empty() {
        eprintln!("No implemented artwork provider is enabled for --release-mbid.");
        return;
    }

    let result = match run_registry_pipeline(
        &registry,
        &query,
        &context,
        resolved_sources,
        &range,
        &format_order,
        &config.source_policies,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    println!(
        "Candidates: {} downloaded front image(s)",
        result.candidates.len()
    );
    for (index, item) in result.candidates.iter().enumerate() {
        let marker = if Some(index) == result.best_index {
            "*"
        } else {
            " "
        };
        println!(
            "{} [{}] {}",
            marker,
            index + 1,
            candidate_summary(item, &range, &format_order)
        );
    }

    if !result.diagnostics.is_empty() {
        println!();
        println!("Provider diagnostics:");
        for diagnostic in &result.diagnostics {
            if diagnostic.url.is_empty() {
                println!("- {}: {}", diagnostic.source, diagnostic.message);
            } else {
                println!(
                    "- {}: {} ({})",
                    diagnostic.source, diagnostic.message, diagnostic.url
                );
            }
        }
    }

    match result.best() {
        Some(best) => {
            let candidate = &best.downloaded.candidate;
            println!();
            println!("Selected:");
            println!(
                "{}x{} {:?} from {}",
                candidate.width, candidate.height, candidate.format, candidate.source
            );
            println!("URL: {}", best.reference.url);
        }
        None => println!("\nSelected: none"),
    }
}

#[tokio::main]
async fn main() {
    match setup_only_bootstrap() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    }

    let cli = Cli::parse();

    if cli.version {
        println!("SPLINED {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if cli.config {
        let mut config = match ensure_regular_config() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };

        // Keep the resolved config alive so the portable bootstrap is complete,
        // but print the canonical config path rather than a runtime-expanded path.
        config.library.music_library.clear();
        drop(config);

        match config_path() {
            Some(path) => println!("SPLINED config: {}", path.display()),
            None => eprintln!("Unable to determine SPLINED configuration path."),
        }
        return;
    }

    if cli.help {
        match ensure_regular_config() {
            Ok(config) => print_dynamic_help(Some(&config)),
            Err(_) => print_dynamic_help(None),
        }
        return;
    }

    let mut config = match load_selected_config(&cli) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    if let Some(preserve_file) = cli.preserve_file {
        config.output.preserve_file = preserve_file;
    }

    if cli.lastfm_credentials {
        if let Err(error) = configure_lastfm_credentials(&config.lastfm.credential_file) {
            eprintln!("{error}");
        }
        return;
    }

    if cli.fanarttv_credentials {
        if let Err(error) = configure_fanarttv_credentials(&config.fanarttv.credential_file) {
            eprintln!("{error}");
        }
        return;
    }

    if cli.lastfm_login {
        run_lastfm_login(&config).await;
        return;
    }

    if cli.mb_oauth_login {
        run_musicbrainz_login(&config).await;
        return;
    }

    let resolved_sources = match resolve_sources(
        &config.sources,
        cli.cover_sources.as_deref(),
        cli.only_cover_sources.as_deref(),
        &cli.exclude_cover_sources,
    ) {
        Ok(sources) => sources,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    if resolved_sources.is_empty() {
        eprintln!("No SPLINED cover sources remain after exclusions.");
        return;
    }

    if cli.scan {
        print_scan_preview(
            "LIBRARY SCAN",
            config.mode,
            effective_verbosity(config.verbosity, config.mode),
            &config.library.music_library,
            &config.library.ignored_subs,
            &resolved_sources,
        );
        return;
    }

    if let Some(path) = cli.scan_dir_path.as_deref() {
        config.scan.scan_library_dir = path.to_string_lossy().into_owned();
        if let Err(error) = run_scan_library_read_report(&config, &resolved_sources).await {
            eprintln!("{error}");
        }
        return;
    }

    if cli.scan_dir {
        if let Err(error) = run_scan_library_read_report(&config, &resolved_sources).await {
            eprintln!("{error}");
        }
        return;
    }

    let effective = effective_verbosity(config.verbosity, config.mode);
    match config.mode {
        Mode::Read => {
            println!("SPLINED READ MODE");
            println!();
            println!("Range");
            println!("Minimum:   {}", config.range.min);
            println!("Ideal:     {}", config.range.ideal);
            println!("Maximum:   {}", config.range.max);
            println!("Ladder:    {}", config.range.ladder);
            println!();
            println!("Verbosity: {:?}", effective);
            println!("Mutation:  disabled");
            println!("Sources:   {}", resolved_sources.join(", "));

            if let Some(release_mbid) = cli.release_mbid.as_deref() {
                println!();
                println!("Release MBID: {release_mbid}");
                run_release_discovery(&config, &resolved_sources, release_mbid).await;
            } else {
                println!("Use --scan-dir for configured album review and sample output.");
            }
        }
        Mode::Write => {
            println!(
                "SPLINED config loaded: mode={:?}, verbosity={:?}, range={}/{}/{}/{}",
                config.mode,
                effective,
                config.range.min,
                config.range.ideal,
                config.range.max,
                config.range.ladder
            );
        }
    }
}
