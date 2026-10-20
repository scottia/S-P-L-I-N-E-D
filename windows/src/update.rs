//! Safe semantic-version update notification for portable and Store builds.

use serde::Deserialize;

pub const STABLE_RELEASES_API: &str = "https://api.github.com/repos/scottia/S-P-L-I-N-E-D/releases";
pub const STORE_PRODUCT_URL: &str = "https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US";
const MANIFEST_NAME: &str = "windows-update.json";
const MAX_RELEASE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub available: bool,
    pub installed_version: String,
    pub available_version: String,
    pub published_at: String,
    pub release_url: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    schema: u32,
    channel: String,
    version: String,
    commit: String,
    short_commit: String,
    #[serde(default)]
    published_at: String,
    release_url: String,
}

pub async fn check_portable_update() -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("SPLINED-Windows/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("Unable to create the update client: {error}"))?;
    let releases_bytes = download(&client, STABLE_RELEASES_API, MAX_RELEASE_BYTES).await?;
    let releases: Vec<Release> = serde_json::from_slice(&releases_bytes)
        .map_err(|error| format!("The stable release list is invalid: {error}"))?;
    let (release, asset) = releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .find_map(|release| {
            if !approved_release_url(&release.html_url) {
                return None;
            }
            release
                .assets
                .iter()
                .find(|asset| {
                    asset.name.eq_ignore_ascii_case(MANIFEST_NAME)
                        && approved_manifest_url(&asset.browser_download_url)
                })
                .map(|asset| (release, asset))
        })
        .ok_or_else(|| {
            "No official stable SPLINED Windows notification release was found.".to_string()
        })?;
    let manifest_bytes = download(&client, &asset.browser_download_url, 1024 * 1024).await?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("The Windows update notification is invalid: {error}"))?;
    validate_manifest(&manifest, release)?;
    let installed = env!("CARGO_PKG_VERSION").to_string();
    let available = is_newer_semantic_version(&installed, &manifest.version)?;
    Ok(UpdateInfo {
        available,
        installed_version: installed.clone(),
        available_version: manifest.version.clone(),
        published_at: manifest.published_at.clone(),
        release_url: manifest.release_url.clone(),
        message: if available {
            format!(
                "A newer SPLINED version is available. Installed: {installed}; available: {}.",
                manifest.version
            )
        } else {
            "No SPLINED update is available.".to_string()
        },
    })
}

#[cfg(windows)]
pub async fn check_store_updates(window_handle: usize) -> Result<u32, String> {
    let context = store_context(window_handle)?;
    let operation = context
        .GetAppAndOptionalStorePackageUpdatesAsync()
        .map_err(|error| format!("Microsoft Store update query could not start: {error}"))?;
    let updates = operation
        .await
        .map_err(|error| format!("Microsoft Store update query failed: {error}"))?;
    updates
        .Size()
        .map_err(|error| format!("Microsoft Store update count is unavailable: {error}"))
}

#[cfg(not(windows))]
pub async fn check_store_updates(_window_handle: usize) -> Result<u32, String> {
    Err("Microsoft Store updates are available only on Windows.".to_string())
}

#[cfg(windows)]
pub async fn install_store_updates(window_handle: usize) -> Result<String, String> {
    use windows::Services::Store::StorePackageUpdateState;

    let context = store_context(window_handle)?;
    let updates = context
        .GetAppAndOptionalStorePackageUpdatesAsync()
        .map_err(|error| format!("Microsoft Store update query could not start: {error}"))?
        .await
        .map_err(|error| format!("Microsoft Store update query failed: {error}"))?;
    let count = updates
        .Size()
        .map_err(|error| format!("Microsoft Store update count is unavailable: {error}"))?;
    if count == 0 {
        return Ok("Microsoft Store reports no available SPLINED update.".to_string());
    }
    let result = context
        .RequestDownloadAndInstallStorePackageUpdatesAsync(&updates)
        .map_err(|error| format!("Microsoft Store installation could not start: {error}"))?
        .await
        .map_err(|error| format!("Microsoft Store installation failed: {error}"))?;
    let state = result
        .OverallState()
        .map_err(|error| format!("Microsoft Store installation status is unavailable: {error}"))?;
    if state == StorePackageUpdateState::Completed {
        Ok("Microsoft Store update installation completed. Restart SPLINED if Windows does not reopen it automatically.".to_string())
    } else {
        Err(format!(
            "Microsoft Store update installation ended with state {}.",
            store_update_state_label(state)
        ))
    }
}

#[cfg(not(windows))]
pub async fn install_store_updates(_window_handle: usize) -> Result<String, String> {
    Err("Microsoft Store updates are available only on Windows.".to_string())
}

#[cfg(windows)]
fn store_context(window_handle: usize) -> Result<windows::Services::Store::StoreContext, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::IInitializeWithWindow;
    use windows::core::Interface;

    let context = windows::Services::Store::StoreContext::GetDefault()
        .map_err(|error| format!("Microsoft Store context is unavailable: {error}"))?;
    if window_handle == 0 {
        return Err(
            "Microsoft Store operations require the active SPLINED window handle.".to_string(),
        );
    }
    let initialize: IInitializeWithWindow = context
        .cast()
        .map_err(|error| format!("Microsoft Store window integration is unavailable: {error}"))?;
    unsafe {
        initialize
            .Initialize(HWND(window_handle as *mut core::ffi::c_void))
            .map_err(|error| format!("Microsoft Store window integration failed: {error}"))?;
    }
    Ok(context)
}

#[cfg(windows)]
fn store_update_state_label(
    state: windows::Services::Store::StorePackageUpdateState,
) -> &'static str {
    use windows::Services::Store::StorePackageUpdateState;
    match state {
        value if value == StorePackageUpdateState::Pending => "Pending",
        value if value == StorePackageUpdateState::Downloading => "Downloading",
        value if value == StorePackageUpdateState::Deploying => "Deploying",
        value if value == StorePackageUpdateState::Completed => "Completed",
        value if value == StorePackageUpdateState::Canceled => "Canceled",
        value if value == StorePackageUpdateState::ErrorLowBattery => "Low battery",
        value if value == StorePackageUpdateState::ErrorWiFiRecommended => "Wi-Fi recommended",
        value if value == StorePackageUpdateState::ErrorWiFiRequired => "Wi-Fi required",
        _ => "Other error",
    }
}

pub fn is_newer_semantic_version(installed: &str, available: &str) -> Result<bool, String> {
    Ok(parse_semantic_version(available, "available")?
        > parse_semantic_version(installed, "installed")?)
}

fn parse_semantic_version(value: &str, role: &str) -> Result<(u64, u64, u64), String> {
    let mut parts = value.split('.');
    let values = [parts.next(), parts.next(), parts.next()];
    if parts.next().is_some() || values.iter().any(Option::is_none) {
        return Err(format!("The {role} SPLINED semantic version is invalid."));
    }
    let parse = |part: Option<&str>| {
        let part = part.unwrap_or_default();
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        part.parse::<u64>().ok()
    };
    match (parse(values[0]), parse(values[1]), parse(values[2])) {
        (Some(major), Some(minor), Some(patch)) => Ok((major, minor, patch)),
        _ => Err(format!("The {role} SPLINED semantic version is invalid.")),
    }
}

async fn download(client: &reqwest::Client, url: &str, maximum: usize) -> Result<Vec<u8>, String> {
    if url != STABLE_RELEASES_API && !approved_manifest_url(url) {
        return Err("The SPLINED update URL is not approved.".to_string());
    }
    let response = client
        .get(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|error| format!("Unable to download SPLINED release metadata: {error}"))?
        .error_for_status()
        .map_err(|error| format!("SPLINED release metadata request failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length == 0 || length > maximum as u64)
    {
        return Err("The SPLINED release metadata download has an invalid size.".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Unable to read SPLINED release metadata: {error}"))?;
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("The SPLINED release metadata download has an invalid size.".to_string());
    }
    Ok(bytes.to_vec())
}

fn validate_manifest(manifest: &UpdateManifest, release: &Release) -> Result<(), String> {
    if manifest.schema != 2 || !manifest.channel.eq_ignore_ascii_case("stable") {
        return Err("The Windows update notification is missing or unsupported.".to_string());
    }
    parse_semantic_version(&manifest.version, "available")?;
    if manifest.commit.len() != 40
        || !manifest
            .commit
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
        || !manifest.commit[..7].eq_ignore_ascii_case(&manifest.short_commit)
    {
        return Err(
            "The Windows update notification contains an invalid commit identity.".to_string(),
        );
    }
    if manifest.version != release.tag_name
        || manifest.release_url != release.html_url
        || !approved_release_url(&manifest.release_url)
    {
        return Err(
            "The Windows update notification does not match its official release.".to_string(),
        );
    }
    Ok(())
}

fn approved_release_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url
                .path()
                .starts_with("/scottia/S-P-L-I-N-E-D/releases/tag/")
    })
}

fn approved_manifest_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url
                .path()
                .starts_with("/scottia/S-P-L-I-N-E-D/releases/download/")
            && url.path().ends_with(&format!("/{MANIFEST_NAME}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_versions_compare_without_commit_metadata() {
        assert!(is_newer_semantic_version("1.0.87", "1.0.88").unwrap());
        assert!(!is_newer_semantic_version("1.1.0", "1.0.99").unwrap());
        assert!(is_newer_semantic_version("01.0.0", "1.0.1").is_err());
    }

    #[test]
    fn update_urls_are_pinned_to_the_official_repository() {
        assert!(approved_release_url(
            "https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/1.0.87"
        ));
        assert!(approved_manifest_url(
            "https://github.com/scottia/S-P-L-I-N-E-D/releases/download/1.0.87/windows-update.json"
        ));
        assert!(!approved_manifest_url(
            "https://example.com/windows-update.json"
        ));
    }
}
