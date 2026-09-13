use clap::Parser;
use splined::candidate::StaticFormat;
use splined::config::{
    Config, Mode, Verbosity, config_path, default_toml, load_config, resolve_sources, samples_dir,
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
    };
    save_fanarttv_credential(&path, &credential)?;

    println!();
    println!("Fanart.tv credential file written successfully.");
    println!("{}", path.display());
    Ok(())
}

fn ensure_musicbrainz_bootstrap(
    config: &splined::musicbrainz::MusicBrainzConfig,
) -> Result<(), String> {
    let path = resolve_token_path(&config.token_file)?;

    if path.exists() {
        let credential = load_musicbrainz_credential(&path)?;
        if credential.client_secret.trim().is_empty() {
            return Err(format!(
                "MusicBrainz OAuth credential file exists but contains no client_secret: {}",
                path.display()
            ));
        }
        return Ok(());
    }

    println!("MusicBrainz OAuth credential file does not exist.");
    println!("Creating:");
    println!("{}", path.display());
    println!();

    let client_secret = rpassword::prompt_password("MusicBrainz client secret: ")
        .map_err(|error| format!("Unable to read MusicBrainz client secret: {error}"))?;
    if client_secret.trim().is_empty() {
        return Err("MusicBrainz client secret cannot be empty.".to_string());
    }

    let credential = OAuthCredential {
        client_secret: client_secret.trim().to_string(),
        access_token: None,
        refresh_token: None,
        token_type: None,
        expires_at_unix: None,
        scope: None,
    };
    save_musicbrainz_credential(&path, &credential)?;

    println!();
    println!("MusicBrainz OAuth credential file created.");
    println!("{}", path.display());
    println!();
    Ok(())
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

fn configured_display(value: &str) -> &str {
    if value.trim().is_empty() {
        "not configured"
    } else {
        value
    }
}

fn print_dynamic_help(config: Option<&Config>) {
    println!("SPLINED artwork discovery and evaluation engine");
    println!();
    println!("Usage: splined.exe [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --config                       Show/create SPLINED configuration");
    println!("  --scan                         Preview configured music library scan");
    println!("  --scan-dir                     Run configured operational scan directory");
    println!("  -p, --preserve-file <BOOL>     Override preserve behavior for this run");
    println!("  -s, --cover-sources <SOURCE>   Replace configured source order");
    println!("  -o, --only-cover-sources       Use only specified sources");
    println!("  -e, --exclude-cover-sources    Exclude specified sources");
    println!("  --release-mbid <MBID>          Discover artwork for a MusicBrainz release");
    println!("  --mb-oauth-login               Authorize MusicBrainz OAuth");
    println!("  --lastfm-credentials           Configure Last.fm API credentials");
    println!("  --lastfm-login                 Authorize Last.fm account");
    println!("  --fanarttv-credentials         Configure Fanart.tv API credentials");
    println!("  -h, --help                     Show this help");
    println!("  -V, --version                  Show version");

    if let Some(config) = config {
        println!();
        println!("Current portable configuration:");
        println!(
            "  Music library:       {}",
            configured_display(&config.library.music_library)
        );
        println!(
            "  Scan directory:      {}",
            configured_display(&config.scan.scan_library_dir)
        );
        println!("  Cache directory:     {}", config.scan.cache_dir);
        println!(
            "  Sample directory:    {}",
            samples_dir(&config.scan.cache_dir).display()
        );
        println!(
            "  Credential directory:{}",
            config.credentials.credential_dir
        );
        println!(
            "  Ignored directories: {}",
            if config.library.ignored_subs.is_empty() {
                "none".to_string()
            } else {
                config.library.ignored_subs.join(", ")
            }
        );
        println!(
            "  Sources:             {}",
            config.sources.cover_sources.join(", ")
        );
        println!(
            "  Excluded sources:    {}",
            if config.sources.exclude_cover_sources.is_empty() {
                "none".to_string()
            } else {
                config.sources.exclude_cover_sources.join(", ")
            }
        );
    }
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

    if !config.musicbrainz.oauth {
        eprintln!(
            "MusicBrainz OAuth is disabled. Set [musicbrainz] oauth = true in SPLINED config first."
        );
        return;
    }

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

    let mut config = match ensure_regular_config() {
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
