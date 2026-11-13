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
    let mut albums = Vec::new();
    let ignored_directories = walk_album_directories(root, ignored_subs, |album| {
        albums.push(album);
        Ok(())
    })?;
    Ok(ScanInventory {
        root: root.to_path_buf(),
        albums,
        ignored_directories,
    })
}

/// Visit one Album directory at a time in deterministic path order.
///
/// Unlike [`inventory_album_directories`], this does not retain the complete
/// library inventory.  The coordinator can inspect and checkpoint an Album,
/// release its paths, and then continue to the next directory.  Memory is
/// therefore bounded by one directory plus the traversal stack.
pub fn walk_album_directories<F>(
    root: &Path,
    ignored_subs: &[String],
    mut on_album: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(AlbumDirectory) -> Result<(), String>,
{
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

    let mut ignored_directories = Vec::new();
    visit_directory_streaming(
        root,
        ignored_subs,
        true,
        &mut ignored_directories,
        &mut on_album,
    )?;
    ignored_directories.sort();
    Ok(ignored_directories)
}

/// Discover Album folders for the startup new-media notice without retaining
/// every track path. Directory entries must still be enumerated to find child
/// folders, but no audio file is opened or statted here.
pub fn audio_containing_directories(
    root: &Path,
    ignored_subs: &[String],
) -> Result<Vec<PathBuf>, String> {
    if !root.is_dir() {
        return Err(format!(
            "SPLINED scan directory does not exist or is not a directory: {}",
            root.display()
        ));
    }
    let mut albums = Vec::new();
    visit_audio_directories(root, ignored_subs, true, &mut albums)?;
    albums.sort();
    Ok(albums)
}

fn visit_audio_directories(
    directory: &Path,
    ignored_subs: &[String],
    is_root: bool,
    albums: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if !is_root
        && directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| should_ignore_directory(name, ignored_subs))
    {
        return Ok(());
    }
    let mut has_audio = false;
    let mut child_directories = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| {
        format!(
            "Unable to read SPLINED scan directory {}: {error}",
            directory.display()
        )
    })? {
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
        } else if !has_audio && file_type.is_file() && is_supported_audio_file(&path) {
            has_audio = true;
        }
    }
    if has_audio {
        albums.push(directory.to_path_buf());
    }
    child_directories.sort();
    for child in child_directories {
        visit_audio_directories(&child, ignored_subs, false, albums)?;
    }
    Ok(())
}

fn visit_directory_streaming<F>(
    directory: &Path,
    ignored_subs: &[String],
    is_root: bool,
    ignored_directories: &mut Vec<PathBuf>,
    on_album: &mut F,
) -> Result<(), String>
where
    F: FnMut(AlbumDirectory) -> Result<(), String>,
{
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
        on_album(AlbumDirectory {
            path: directory.to_path_buf(),
            audio_files,
        })?;
    }

    for child in child_directories {
        visit_directory_streaming(&child, ignored_subs, false, ignored_directories, on_album)?;
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

pub fn is_supported_audio_file(path: &Path) -> bool {
    // Interrupted atomic writes may leave a same-directory MP3 stage behind.
    // It is not a song, even though its suffix is a supported audio format.
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().starts_with(".splined-stage-"))
    {
        return false;
    }
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
        assert_eq!(
            audio_containing_directories(root, &["metadata".to_string()]).unwrap(),
            inventory
                .albums
                .iter()
                .map(|album| album.path.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn missing_root_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let missing = temp.path().join("missing");

        let error = inventory_album_directories(&missing, &[]).expect_err("missing root rejected");
        assert!(error.contains("does not exist"));
    }

    #[test]
    fn staging_files_are_not_inventoried_as_tracks() {
        let temp = tempfile::tempdir().expect("temp directory");
        let album = temp.path().join("Artist").join("Album");
        fs::create_dir_all(&album).expect("album directory");
        File::create(album.join("01 - Song.mp3")).expect("song");
        File::create(album.join(".splined-stage-orphan.mp3")).expect("orphan stage");
        let inventory = inventory_album_directories(temp.path(), &[]).expect("inventory");
        assert_eq!(inventory.albums.len(), 1);
        assert_eq!(
            inventory.albums[0].audio_files,
            vec![album.join("01 - Song.mp3")]
        );
    }

    #[test]
    fn streaming_walk_calls_back_before_the_remaining_tree_is_visited() {
        let temp = tempfile::tempdir().expect("temp directory");
        let first = temp.path().join("A Artist").join("A Album");
        let later = temp.path().join("Z Artist").join("Z Album");
        fs::create_dir_all(&first).expect("first Album");
        fs::create_dir_all(&later).expect("later Album");
        File::create(first.join("01.mp3")).expect("first track");
        File::create(later.join("01.mp3")).expect("later track");
        let mut visited = Vec::new();

        let error = walk_album_directories(temp.path(), &[], |album| {
            visited.push(album.path);
            Err("stop after first streamed Album".to_string())
        })
        .expect_err("callback stops traversal");

        assert_eq!(error, "stop after first streamed Album");
        assert_eq!(visited, vec![first]);
    }

    #[test]
    fn streaming_walk_discovers_each_large_fixture_album_once() {
        let temp = tempfile::tempdir().expect("temp directory");
        let mut expected = Vec::new();
        for artist in 0..20 {
            for album in 0..25 {
                let path = temp
                    .path()
                    .join(format!("Artist {artist:03}"))
                    .join(format!("Album {album:03}"));
                fs::create_dir_all(&path).expect("Album directory");
                File::create(path.join("01.mp3")).expect("track");
                expected.push(path);
            }
        }
        let mut actual = Vec::new();
        walk_album_directories(temp.path(), &[], |album| {
            actual.push(album.path);
            Ok(())
        })
        .expect("streaming walk");

        expected.sort();
        actual.sort();
        actual.dedup();
        assert_eq!(actual, expected);
    }
}
