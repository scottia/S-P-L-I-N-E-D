use crate::config::{CURRENT_CONFIG_VERSION, Config, config_path, default_toml, parse_config};
use crate::safe_write::{recover_backup_if_needed, replace_text_file};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MigrationReport {
    pub added_keys: Vec<String>,
    pub from_version: u32,
    pub to_version: u32,
}

impl MigrationReport {
    pub fn changed(&self) -> bool {
        !self.added_keys.is_empty() || self.from_version != self.to_version
    }
}

pub fn migrate_config_if_needed() -> Result<MigrationReport, String> {
    let path = config_path()
        .ok_or_else(|| "Unable to determine SPLINED configuration path.".to_string())?;

    recover_backup_if_needed(&path, "SPLINED configuration")?;

    if !path.exists() {
        return Ok(MigrationReport {
            from_version: CURRENT_CONFIG_VERSION,
            to_version: CURRENT_CONFIG_VERSION,
            ..MigrationReport::default()
        });
    }

    migrate_config_path(&path)
}

fn migrate_config_path(path: &Path) -> Result<MigrationReport, String> {
    recover_backup_if_needed(path, "SPLINED configuration")?;

    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("Unable to read SPLINED configuration: {error}"))?;

    let mut current: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("Unable to parse SPLINED configuration: {error}"))?;
    let defaults: toml::Value = toml::from_str(
        &default_toml().map_err(|error| format!("Unable to generate SPLINED defaults: {error}"))?,
    )
    .map_err(|error| format!("Unable to parse SPLINED default configuration: {error}"))?;

    let from_version = current
        .get("config_version")
        .and_then(toml::Value::as_integer)
        .map(|value| value.max(0) as u32)
        .unwrap_or(0);

    if from_version > CURRENT_CONFIG_VERSION {
        return Err(format!(
            "SPLINED configuration version {from_version} is newer than supported version {CURRENT_CONFIG_VERSION}."
        ));
    }

    let had_scan_cache_dir = current
        .get("scan")
        .and_then(toml::Value::as_table)
        .is_some_and(|scan| scan.contains_key("cache_dir"));

    let mut added_keys = Vec::new();
    merge_missing(&mut current, &defaults, "", &mut added_keys)?;

    if from_version < 2 {
        set_empty_string(
            &mut current,
            &["fanarttv", "credential_file"],
            "fanarttv.json",
        )?;
        set_empty_string(&mut current, &["lastfm", "credential_file"], "lastfm.json")?;
        set_empty_string(
            &mut current,
            &["musicbrainz", "token_file"],
            "musicbrainz.json",
        )?;
    }

    if from_version < 3 {
        move_library_scan_to_scan(&mut current)?;
    }

    if from_version < 4 {
        migrate_v4_cache_and_samples(&mut current, had_scan_cache_dir)?;
    }

    let table = current
        .as_table_mut()
        .ok_or_else(|| "SPLINED configuration root must be a TOML table.".to_string())?;
    table.insert(
        "config_version".to_string(),
        toml::Value::Integer(CURRENT_CONFIG_VERSION as i64),
    );

    let intermediate = toml::to_string_pretty(&current)
        .map_err(|error| format!("Unable to serialize migrated SPLINED configuration: {error}"))?;
    let canonical_config: Config = toml::from_str(&intermediate)
        .map_err(|error| format!("Unable to canonicalize SPLINED configuration: {error}"))?;
    let migrated = toml::to_string_pretty(&canonical_config)
        .map_err(|error| format!("Unable to serialize canonical SPLINED configuration: {error}"))?;

    parse_config(&migrated)?;

    let report = MigrationReport {
        added_keys,
        from_version,
        to_version: CURRENT_CONFIG_VERSION,
    };

    if report.changed() {
        replace_text_file(path, &migrated, "SPLINED configuration", |candidate| {
            parse_config(candidate).map(|_| ())
        })?;
    } else {
        parse_config(&text)?;
    }

    Ok(report)
}

fn migrate_v4_cache_and_samples(
    current: &mut toml::Value,
    had_scan_cache_dir: bool,
) -> Result<(), String> {
    let legacy_sample_dir = current
        .get("read")
        .and_then(toml::Value::as_table)
        .and_then(|read| read.get("sample_dir"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let root = current
        .as_table_mut()
        .ok_or_else(|| "SPLINED configuration root must be a TOML table.".to_string())?;

    if !had_scan_cache_dir && let Some(legacy_sample_dir) = legacy_sample_dir {
        let migrated_cache_dir = legacy_sample_cache_root(&legacy_sample_dir);
        let scan = root
            .get_mut("scan")
            .ok_or_else(|| "SPLINED configuration [scan] section is missing.".to_string())?
            .as_table_mut()
            .ok_or_else(|| "SPLINED configuration [scan] must be a table.".to_string())?;
        scan.insert(
            "cache_dir".to_string(),
            toml::Value::String(migrated_cache_dir),
        );
    }

    root.remove("read");
    Ok(())
}

fn legacy_sample_cache_root(sample_dir: &str) -> String {
    let trimmed = sample_dir.trim().trim_end_matches(['\\', '/']);
    let lower = trimmed.to_ascii_lowercase();

    if matches!(lower.as_str(), "sample" | "samples" | "_sample_covers") {
        return "cache".to_string();
    }

    for suffix in [
        r"\_sample_covers",
        "/_sample_covers",
        r"\samples",
        "/samples",
    ] {
        if lower.ends_with(suffix) {
            let parent_len = trimmed.len().saturating_sub(suffix.len());
            let parent = &trimmed[..parent_len];
            return if parent.is_empty() {
                "cache".to_string()
            } else {
                parent.to_string()
            };
        }
    }

    trimmed.to_string()
}

fn move_library_scan_to_scan(current: &mut toml::Value) -> Result<(), String> {
    let previous = {
        let root = current
            .as_table_mut()
            .ok_or_else(|| "SPLINED configuration root must be a TOML table.".to_string())?;

        let Some(library) = root.get_mut("library") else {
            return Ok(());
        };

        let library = library
            .as_table_mut()
            .ok_or_else(|| "SPLINED configuration [library] must be a table.".to_string())?;

        library.remove("library_scan")
    };

    let Some(previous) = previous else {
        return Ok(());
    };

    if !previous.is_bool() {
        return Err("SPLINED library.library_scan must be true or false.".to_string());
    }

    let root = current
        .as_table_mut()
        .ok_or_else(|| "SPLINED configuration root must be a TOML table.".to_string())?;
    let scan = root
        .get_mut("scan")
        .ok_or_else(|| "SPLINED configuration [scan] section is missing.".to_string())?
        .as_table_mut()
        .ok_or_else(|| "SPLINED configuration [scan] must be a table.".to_string())?;
    scan.insert("library_scan".to_string(), previous);

    Ok(())
}

fn value_at_path_mut<'a>(
    current: &'a mut toml::Value,
    path: &[&str],
) -> Result<&'a mut toml::Value, String> {
    let mut value = current;

    for key in path {
        let table = value
            .as_table_mut()
            .ok_or_else(|| format!("SPLINED configuration section for {key} is not a table."))?;
        value = table
            .get_mut(*key)
            .ok_or_else(|| format!("SPLINED configuration key {} is missing.", path.join(".")))?;
    }

    Ok(value)
}

fn set_empty_string(
    current: &mut toml::Value,
    path: &[&str],
    replacement: &str,
) -> Result<(), String> {
    let value = value_at_path_mut(current, path)?;

    if value.as_str().is_some_and(|text| text.trim().is_empty()) {
        *value = toml::Value::String(replacement.to_string());
    }

    Ok(())
}

fn merge_missing(
    current: &mut toml::Value,
    defaults: &toml::Value,
    prefix: &str,
    added: &mut Vec<String>,
) -> Result<(), String> {
    match (current, defaults) {
        (toml::Value::Table(current_table), toml::Value::Table(default_table)) => {
            for (key, default_value) in default_table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };

                match current_table.get_mut(key) {
                    Some(current_value) => {
                        if current_value.is_table() && default_value.is_table() {
                            merge_missing(current_value, default_value, &path, added)?;
                        }
                    }
                    None => {
                        current_table.insert(key.clone(), default_value.clone());
                        added.push(path);
                    }
                }
            }
            Ok(())
        }
        _ => Err("SPLINED configuration structure is incompatible with defaults.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn legacy_config_migrates_without_inventing_source_exclusions() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("config.toml");
        let original = r#"
config_version = 2
mode = "read"
verbosity = "info"

[library]
library_scan = false

[sources]
cover_sources = ["deezer", "itunes", "fanarttv", "lastfm", "coverartarchive", "discogs"]
exclude_cover_sources = []

[range]
min = 1200
ideal = 1800
max = 2400
ladder = 3600
"#;
        std::fs::write(&path, original).expect("fixture should write");

        migrate_config_path(&path).expect("migration should succeed");
        let migrated = std::fs::read_to_string(&path).expect("migrated config should read");
        let parsed = parse_config(&migrated).expect("migrated config should parse");

        assert!(parsed.sources.exclude_cover_sources.is_empty());
        assert_eq!(parsed.config_version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn legacy_sample_default_becomes_portable_cache_default() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("config.toml");
        let original = r#"
config_version = 1
mode = "read"
verbosity = "info"

[read]
sample_dir = "sample"

[range]
min = 1200
ideal = 1800
max = 2400
ladder = 3600
"#;
        std::fs::write(&path, original).expect("fixture should write");

        migrate_config_path(&path).expect("migration should succeed");
        let migrated = std::fs::read_to_string(&path).expect("migrated config should read");
        let parsed = parse_config(&migrated).expect("migrated config should parse");

        assert_eq!(parsed.scan.cache_dir, "cache");
        assert!(!migrated.contains("[read]"));
    }

    #[test]
    fn current_config_is_not_rewritten() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("config.toml");
        let original = default_toml().expect("defaults should serialize");
        std::fs::write(&path, &original).expect("fixture should write");

        let report = migrate_config_path(&path).expect("validation should succeed");
        let after = std::fs::read_to_string(&path).expect("config should read");

        assert!(!report.changed());
        assert_eq!(after, original);
    }
}
