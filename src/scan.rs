use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumDirectory {
    pub path: PathBuf,
    pub audio_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanInventory {
    pub root: PathBuf,
    pub albums: Vec<AlbumDirectory>,
    pub ignored_directories: Vec<PathBuf>,
}

pub fn inventory_album_directories(
    root: &Path,
    ignored_subs: &[String],
) -> Result<ScanInventory, String> {
    if !root.exists() {
        return Err(format!(
            "SPLINED scan directory does not exist: {}",
            root.display()
        ));
    }

    if !root.is_dir() {
        return Err(format!(
            "SPLINED scan path is not a directory: {}",
            root.display()
        ));
    }

    let mut albums = Vec::new();
    let mut ignored_directories = Vec::new();
    visit_directory(
        root,
        ignored_subs,
        true,
        &mut albums,
        &mut ignored_directories,
    )?;

    albums.sort_by(|left, right| left.path.cmp(&right.path));
    ignored_directories.sort();

    Ok(ScanInventory {
        root: root.to_path_buf(),
        albums,
        ignored_directories,
    })
}

fn visit_directory(
    directory: &Path,
    ignored_subs: &[String],
    is_root: bool,
    albums: &mut Vec<AlbumDirectory>,
    ignored_directories: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if !is_root
        && directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| should_ignore_directory(name, ignored_subs))
    {
        ignored_directories.push(directory.to_path_buf());
        return Ok(());
    }

    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "Unable to read SPLINED scan directory {}: {error}",
            directory.display()
        )
    })?;

    let mut audio_files = Vec::new();
    let mut child_directories = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Unable to read an entry in SPLINED scan directory {}: {error}",
                directory.display()
            )
        })?;

        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "Unable to inspect SPLINED scan entry {}: {error}",
                path.display()
            )
        })?;

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            child_directories.push(path);
        } else if file_type.is_file() && is_supported_audio_file(&path) {
            audio_files.push(path);
        }
    }

    audio_files.sort();
    child_directories.sort();

    if !audio_files.is_empty() {
        albums.push(AlbumDirectory {
            path: directory.to_path_buf(),
            audio_files,
        });
    }

    for child in child_directories {
        visit_directory(&child, ignored_subs, false, albums, ignored_directories)?;
    }

    Ok(())
}

pub fn should_ignore_directory(name: &str, ignored_subs: &[String]) -> bool {
    ignored_subs
        .iter()
        .any(|pattern| wildcard_match_ascii_case_insensitive(pattern, name))
}

fn wildcard_match_ascii_case_insensitive(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    let value = value.to_ascii_lowercase();

    if pattern.is_empty() {
        return false;
    }

    if pattern == "*" {
        return true;
    }

    match (pattern.starts_with('*'), pattern.ends_with('*')) {
        (true, true) if pattern.len() > 2 => value.contains(pattern.trim_matches('*')),
        (true, false) => value.ends_with(pattern.trim_start_matches('*')),
        (false, true) => value.starts_with(pattern.trim_end_matches('*')),
        (false, false) => value == pattern,
        _ => false,
    }
}

fn is_supported_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| {
            matches!(
                extension.as_str(),
                "mp3" | "flac" | "m4a" | "mp4" | "ogg" | "opus" | "wav" | "aiff" | "aif"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};

    #[test]
    fn wildcard_ignore_matching_is_case_insensitive() {
        let ignored = vec!["metadata".to_string(), ".cache-*".to_string()];

        assert!(should_ignore_directory("METADATA", &ignored));
        assert!(should_ignore_directory(".cache-123", &ignored));
        assert!(!should_ignore_directory("Artist", &ignored));
    }

    #[test]
    fn inventory_finds_audio_directories_and_skips_ignored_trees() {
        let temp = tempfile::tempdir().expect("temp directory");
        let root = temp.path();

        let album = root.join("Artist").join("Album");
        let ignored = root.join("metadata").join("Hidden Album");
        let nested_album = root.join("Artist").join("Disc 2");

        fs::create_dir_all(&album).expect("album directory");
        fs::create_dir_all(&ignored).expect("ignored directory");
        fs::create_dir_all(&nested_album).expect("nested album directory");

        File::create(album.join("01 - Track.mp3")).expect("audio file");
        File::create(album.join("cover.jpg")).expect("non-audio file");
        File::create(ignored.join("01 - Hidden.mp3")).expect("ignored audio file");
        File::create(nested_album.join("02 - Track.FLAC")).expect("audio file");

        let inventory = inventory_album_directories(root, &["metadata".to_string()])
            .expect("inventory should succeed");

        assert_eq!(inventory.albums.len(), 2);
        assert!(inventory.albums.iter().any(|item| item.path == album));
        assert!(
            inventory
                .albums
                .iter()
                .any(|item| item.path == nested_album)
        );
        assert_eq!(inventory.ignored_directories, vec![root.join("metadata")]);
    }

    #[test]
    fn missing_root_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let missing = temp.path().join("missing");

        let error = inventory_album_directories(&missing, &[]).expect_err("missing root rejected");
        assert!(error.contains("does not exist"));
    }
}
