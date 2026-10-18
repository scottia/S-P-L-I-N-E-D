//! Rust-owned application state for the Windows Slint presentation layer.
//!
//! Widgets render projections of this module and send actions back to it. The
//! selection set, scan queue, review state, and editing values never live only
//! in Slint controls.

use crate::config::{Config, Mode, configured_samples_dir, resolve_sources};
use crate::inspect::inspect_image;
use crate::media_database::{MediaAlbumSnapshot, MediaSnapshot, MediaTrackSnapshot};
use crate::range::Range;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryView {
    Artists,
    Albums,
    Folders,
}

impl LibraryView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Artists => "Artists",
            Self::Albums => "Albums",
            Self::Folders => "Folders",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatusFilter {
    All,
    Unprocessed,
    Incomplete,
    Processed,
    Bypassed,
    PartialTimeout,
    ArtistComplete,
    ArtistContainsBypass,
}

impl StatusFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Unprocessed => "Unprocessed",
            Self::Incomplete => "Incomplete",
            Self::Processed => "Processed",
            Self::Bypassed => "Bypassed",
            Self::PartialTimeout => "Partial / Error",
            Self::ArtistComplete => "Artist Complete",
            Self::ArtistContainsBypass => "Artist Bypass",
        }
    }
}

fn all_status_filters() -> BTreeSet<StatusFilter> {
    [
        StatusFilter::Unprocessed,
        StatusFilter::Incomplete,
        StatusFilter::Processed,
        StatusFilter::Bypassed,
        StatusFilter::PartialTimeout,
        StatusFilter::ArtistComplete,
        StatusFilter::ArtistContainsBypass,
    ]
    .into_iter()
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanScope {
    All,
    Selected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Read,
    LiveWrite,
}

impl LaunchMode {
    pub fn config_mode(self) -> Mode {
        match self {
            Self::Read => Mode::Read,
            Self::LiveWrite => Mode::Write,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumState {
    New,
    Incomplete,
    Processed,
    TimeoutActive,
    Bypassed,
}

impl AlbumState {
    pub fn from_database(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "incomplete" => Self::Incomplete,
            "processed" => Self::Processed,
            "timeout" => Self::TimeoutActive,
            "bypassed" => Self::Bypassed,
            _ => Self::New,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::New => "Unprocessed",
            Self::Incomplete => "Incomplete",
            Self::Processed => "Processed",
            Self::TimeoutActive => "Partial / Error",
            Self::Bypassed => "Bypassed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistState {
    Unprocessed,
    Partial,
    Complete,
    ContainsBypass,
}

impl ArtistState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unprocessed => "Unprocessed",
            Self::Partial => "Partial / Error",
            Self::Complete => "Artist Complete",
            Self::ContainsBypass => "Artist Bypass",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AlbumEntry {
    pub snapshot: MediaAlbumSnapshot,
    pub state: AlbumState,
    pub selected: bool,
    pub bypass_override: bool,
}

impl AlbumEntry {
    pub fn new(snapshot: MediaAlbumSnapshot) -> Self {
        let state = AlbumState::from_database(&snapshot.status);
        Self {
            snapshot,
            state,
            selected: false,
            bypass_override: false,
        }
    }

    pub fn compilation_track_artwork_pending(&self) -> bool {
        self.snapshot.compilation_track_art_eligible
            && !self
                .snapshot
                .selected_source
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("embedded-compilation"))
    }

    pub fn eligible_by_default(&self) -> bool {
        matches!(self.state, AlbumState::New | AlbumState::Incomplete)
            || (self.compilation_track_artwork_pending() && self.state != AlbumState::TimeoutActive)
    }

    pub fn queue_eligible(&self) -> bool {
        match self.state {
            AlbumState::TimeoutActive => false,
            AlbumState::Bypassed => self.selected && self.bypass_override,
            _ => self.selected || self.eligible_by_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRow {
    pub key: String,
    pub primary: String,
    pub secondary: String,
    pub status: String,
    pub artwork_path: String,
    pub kind: String,
    pub depth: usize,
    pub expanded: bool,
    pub selected: bool,
    pub eligible: bool,
    pub album_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationTrackRow {
    pub album_key: String,
    pub path: String,
    pub file_name: String,
    pub status: String,
    pub embedded_artwork_recorded: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionResult {
    Changed,
    NeedsBypassOverride(String),
    BlockedByTimeout(String),
    NotFound,
}

fn release_year_number(value: &str) -> i32 {
    value
        .trim()
        .chars()
        .take(4)
        .collect::<String>()
        .parse()
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct ScanTarget {
    pub album_key: String,
    pub artist: String,
    pub album: String,
    pub path: String,
    pub compilation_track_path: Option<String>,
    pub bypass_override: bool,
}

#[derive(Debug, Clone)]
pub struct LibraryState {
    pub schema_version: i64,
    pub database_path: String,
    pub canonical_root: String,
    pub local_root: String,
    pub albums: Vec<AlbumEntry>,
    pub view: LibraryView,
    pub status_filter: StatusFilter,
    pub status_filters: BTreeSet<StatusFilter>,
    pub text_filter: String,
    pub artist_filter: String,
    pub album_filter: String,
    pub show_tracks: bool,
    pub focused_artist: Option<String>,
    pub focused_album_key: Option<String>,
    pub selected_compilation_track_paths: BTreeSet<String>,
    /// Read-only artwork projected from Navidrome into the Windows UI. These
    /// paths always point at SPLINED's local thumbnail cache; the music
    /// library and Navidrome storage are never modified.
    pub artist_artwork_paths: HashMap<String, String>,
    pub album_artwork_paths: HashMap<String, String>,
    /// Navidrome's stable artist IDs, keyed by the normalized local artist
    /// name. These IDs authorize the explicit Artist Overview write action;
    /// SPLINED never guesses an artist endpoint from display text alone.
    pub artist_navidrome_ids: HashMap<String, String>,
}

impl LibraryState {
    pub fn from_snapshot(snapshot: MediaSnapshot) -> Result<Self, String> {
        if snapshot.schema_version != crate::media_database::SCHEMA_VERSION {
            return Err(format!(
                "SPLINED media database schema {} is not supported by this application.",
                snapshot.schema_version
            ));
        }
        let mut albums = snapshot
            .albums
            .into_iter()
            .map(AlbumEntry::new)
            .collect::<Vec<_>>();
        albums.sort_by(|left, right| {
            left.snapshot
                .artist
                .to_ascii_lowercase()
                .cmp(&right.snapshot.artist.to_ascii_lowercase())
                .then_with(|| {
                    left.snapshot
                        .title
                        .to_ascii_lowercase()
                        .cmp(&right.snapshot.title.to_ascii_lowercase())
                })
                .then_with(|| {
                    left.snapshot
                        .path
                        .to_ascii_lowercase()
                        .cmp(&right.snapshot.path.to_ascii_lowercase())
                })
        });
        // The SQLite snapshot already records the chosen local cover. Publish
        // that path immediately so a restart does not depend on Navidrome
        // returning a thumbnail before the user's own cover can be shown.
        // Missing-MBID fallback albums deliberately have no folder cover.
        let album_artwork_paths = albums
            .iter()
            .filter(|album| !album.snapshot.album_mbid.trim().is_empty())
            .filter(|album| !album.snapshot.cover_path.trim().is_empty())
            .map(|album| {
                (
                    album.snapshot.album_key.to_ascii_lowercase(),
                    album.snapshot.cover_path.clone(),
                )
            })
            .collect();
        Ok(Self {
            schema_version: snapshot.schema_version,
            database_path: snapshot.database_path,
            canonical_root: snapshot.canonical_root,
            local_root: snapshot.local_root,
            albums,
            view: LibraryView::Artists,
            status_filter: StatusFilter::All,
            status_filters: all_status_filters(),
            text_filter: String::new(),
            artist_filter: String::new(),
            album_filter: String::new(),
            show_tracks: false,
            focused_artist: None,
            focused_album_key: None,
            selected_compilation_track_paths: BTreeSet::new(),
            artist_artwork_paths: HashMap::new(),
            album_artwork_paths,
            artist_navidrome_ids: HashMap::new(),
        })
    }

    pub fn empty() -> Self {
        Self {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
            view: LibraryView::Artists,
            status_filter: StatusFilter::All,
            status_filters: all_status_filters(),
            text_filter: String::new(),
            artist_filter: String::new(),
            album_filter: String::new(),
            show_tracks: false,
            focused_artist: None,
            focused_album_key: None,
            selected_compilation_track_paths: BTreeSet::new(),
            artist_artwork_paths: HashMap::new(),
            album_artwork_paths: HashMap::new(),
            artist_navidrome_ids: HashMap::new(),
        }
    }

    pub fn selected_count(&self) -> usize {
        self.albums.iter().filter(|album| album.selected).count()
    }

    pub fn compilation_track_rows(&self) -> Vec<CompilationTrackRow> {
        let focused = self.focused_album_key.as_deref();
        let focused_artist = self.focused_artist.as_deref();
        let filter_to_artist =
            !self.show_tracks && self.view == LibraryView::Artists && focused_artist.is_some();
        self.albums
            .iter()
            .filter(|album| {
                focused.is_some_and(|album_key| {
                    album.snapshot.album_key.eq_ignore_ascii_case(album_key)
                })
            })
            .flat_map(|album| {
                album
                    .snapshot
                    .compilation_tracks
                    .iter()
                    .filter(move |track| {
                        !filter_to_artist
                            || focused_artist.is_some_and(|artist| {
                                Self::track_matches_artist(&track.artist, artist)
                            })
                    })
                    .map(move |track| CompilationTrackRow {
                        album_key: album.snapshot.album_key.clone(),
                        path: track.path.clone(),
                        file_name: std::path::Path::new(&track.path)
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or(&track.path)
                            .to_string(),
                        status: if track.embedded_artwork_recorded {
                            "Processed".to_string()
                        } else {
                            "Incomplete".to_string()
                        },
                        embedded_artwork_recorded: track.embedded_artwork_recorded,
                        selected: self
                            .selected_compilation_track_paths
                            .contains(&track.path.to_ascii_lowercase()),
                    })
            })
            .collect()
    }

    pub fn artist_state<'a>(albums: impl IntoIterator<Item = &'a AlbumEntry>) -> ArtistState {
        let values = albums.into_iter().collect::<Vec<_>>();
        if values
            .iter()
            .any(|album| album.state == AlbumState::Bypassed)
        {
            ArtistState::ContainsBypass
        } else if values.is_empty() || values.iter().all(|album| album.state == AlbumState::New) {
            ArtistState::Unprocessed
        } else if values.iter().all(|album| {
            matches!(
                album.state,
                AlbumState::Processed | AlbumState::TimeoutActive
            )
        }) {
            ArtistState::Complete
        } else {
            ArtistState::Partial
        }
    }

    fn artists(&self) -> BTreeMap<String, Vec<usize>> {
        let mut values = BTreeMap::<String, Vec<usize>>::new();
        for (index, album) in self.albums.iter().enumerate() {
            values
                .entry(album.snapshot.artist.clone())
                .or_default()
                .push(index);
        }
        values
    }

    /// Returns the next or previous Artist that still has work eligible for
    /// processing. Artist Overview navigation deliberately ignores Library
    /// Status so a completed current Artist cannot strand the operator when
    /// the active filter removes it from the ordinary row model.
    pub fn adjacent_pending_artist(&self, direction: i32) -> Option<String> {
        let artists = self.artists();
        let pending = artists
            .iter()
            .filter(|(_, indices)| {
                indices.iter().any(|index| {
                    let album = &self.albums[*index];
                    album.eligible_by_default() && self.text_matches(album)
                })
            })
            .map(|(artist, _)| artist.clone())
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return None;
        }

        let current = self
            .focused_artist
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let current_index = pending
            .iter()
            .position(|artist| artist.eq_ignore_ascii_case(&current));
        let index = if direction < 0 {
            current_index
                .map(|index| (index + pending.len() - 1) % pending.len())
                .unwrap_or_else(|| {
                    pending
                        .iter()
                        .rposition(|artist| artist.to_ascii_lowercase() < current)
                        .unwrap_or(pending.len() - 1)
                })
        } else {
            current_index
                .map(|index| (index + 1) % pending.len())
                .unwrap_or_else(|| {
                    pending
                        .iter()
                        .position(|artist| artist.to_ascii_lowercase() > current)
                        .unwrap_or(0)
                })
        };
        pending.get(index).cloned()
    }

    fn text_matches(&self, album: &AlbumEntry) -> bool {
        let needle = self.text_filter.trim().to_ascii_lowercase();
        let global_matches = needle.is_empty()
            || album.snapshot.artist.to_ascii_lowercase().contains(&needle)
            || album.snapshot.title.to_ascii_lowercase().contains(&needle)
            || album.snapshot.path.to_ascii_lowercase().contains(&needle);
        let artist = self.artist_filter.trim().to_ascii_lowercase();
        let album_title = self.album_filter.trim().to_ascii_lowercase();
        global_matches
            && (artist.is_empty() || album.snapshot.artist.to_ascii_lowercase().contains(&artist))
            && (album_title.is_empty()
                || album
                    .snapshot
                    .title
                    .to_ascii_lowercase()
                    .contains(&album_title))
    }

    fn album_matches_filter(album: &AlbumEntry, filter: StatusFilter) -> bool {
        match filter {
            StatusFilter::All => true,
            StatusFilter::Unprocessed => album.state == AlbumState::New,
            StatusFilter::Incomplete => album.state == AlbumState::Incomplete,
            StatusFilter::Processed | StatusFilter::ArtistComplete => {
                album.state == AlbumState::Processed
            }
            StatusFilter::Bypassed | StatusFilter::ArtistContainsBypass => {
                album.state == AlbumState::Bypassed
            }
            StatusFilter::PartialTimeout => album.state == AlbumState::TimeoutActive,
        }
    }

    fn artist_matches_filter(&self, indices: &[usize], filter: StatusFilter) -> bool {
        let state = Self::artist_state(indices.iter().map(|index| &self.albums[*index]));
        match filter {
            StatusFilter::All => true,
            StatusFilter::Unprocessed => indices
                .iter()
                .any(|index| self.albums[*index].state == AlbumState::New),
            StatusFilter::Incomplete => indices
                .iter()
                .any(|index| self.albums[*index].state == AlbumState::Incomplete),
            StatusFilter::Processed => indices
                .iter()
                .any(|index| self.albums[*index].state == AlbumState::Processed),
            StatusFilter::Bypassed => indices
                .iter()
                .any(|index| self.albums[*index].state == AlbumState::Bypassed),
            StatusFilter::PartialTimeout => {
                state == ArtistState::Partial
                    || indices
                        .iter()
                        .any(|index| self.albums[*index].state == AlbumState::TimeoutActive)
            }
            StatusFilter::ArtistComplete => state == ArtistState::Complete,
            StatusFilter::ArtistContainsBypass => state == ArtistState::ContainsBypass,
        }
    }

    fn artist_matches_active_status(&self, indices: &[usize]) -> bool {
        self.status_filters
            .iter()
            .copied()
            .any(|filter| self.artist_matches_filter(indices, filter))
    }

    fn album_matches_artist_active_status(
        &self,
        album: &AlbumEntry,
        artist_indices: &[usize],
    ) -> bool {
        self.status_filters.iter().copied().any(|filter| {
            if filter == StatusFilter::PartialTimeout
                && Self::artist_state(artist_indices.iter().map(|index| &self.albums[*index]))
                    == ArtistState::Partial
            {
                album.state != AlbumState::Processed
            } else {
                Self::album_matches_filter(album, filter)
            }
        })
    }

    fn album_matches_active_filter(&self, album: &AlbumEntry) -> bool {
        self.status_filters
            .iter()
            .copied()
            .any(|filter| Self::album_matches_filter(album, filter))
    }

    fn album_visible(&self, index: usize, _artists: &BTreeMap<String, Vec<usize>>) -> bool {
        let album = &self.albums[index];
        if !self.text_matches(album) {
            return false;
        }
        if matches!(self.view, LibraryView::Albums | LibraryView::Folders)
            && self
                .focused_artist
                .as_ref()
                .is_some_and(|artist| !artist.eq_ignore_ascii_case(&album.snapshot.artist))
        {
            return false;
        }
        self.album_matches_active_filter(album)
    }

    fn album_matches_active_status(
        &self,
        index: usize,
        artists: &BTreeMap<String, Vec<usize>>,
    ) -> bool {
        let album = &self.albums[index];
        if matches!(self.view, LibraryView::Albums | LibraryView::Folders)
            && self
                .focused_artist
                .as_ref()
                .is_some_and(|artist| !artist.eq_ignore_ascii_case(&album.snapshot.artist))
        {
            return false;
        }
        if !self.show_tracks && self.view == LibraryView::Artists {
            artists
                .get(&album.snapshot.artist)
                .is_some_and(|indices| self.artist_matches_active_status(indices))
        } else {
            self.album_matches_active_filter(album)
        }
    }

    fn album_is_in_active_review_view(&self, album: &AlbumEntry) -> bool {
        !self.show_tracks || Self::is_fallback_album(album)
    }

    fn is_fallback_album(album: &AlbumEntry) -> bool {
        album.snapshot.album_mbid.trim().is_empty()
    }

    fn track_matches_artist(track_artist: &str, artist: &str) -> bool {
        track_artist
            .split(['\0', ';'])
            .any(|value| value.trim().eq_ignore_ascii_case(artist.trim()))
    }

    /// Completion of a fallback appearance is scoped to this artist's tracks,
    /// not the compilation folder's Album status or the other performers.
    pub fn artist_fallback_track_progress(
        album: &AlbumEntry,
        artist: &str,
    ) -> Option<(usize, usize)> {
        if !Self::is_fallback_album(album) {
            return None;
        }
        let matching = album
            .snapshot
            .compilation_tracks
            .iter()
            .filter(|track| Self::track_matches_artist(&track.artist, artist))
            .collect::<Vec<_>>();
        (!matching.is_empty()).then(|| {
            (
                matching
                    .iter()
                    .filter(|track| track.embedded_artwork_recorded)
                    .count(),
                matching.len(),
            )
        })
    }

    /// A featured credit appears after the primary performer in the track's
    /// multi-artist tag. Do not mark a primary credit as FEAT on mixed albums.
    pub fn artist_fallback_is_featured(album: &AlbumEntry, artist: &str) -> bool {
        let mut featured = false;
        for track in &album.snapshot.compilation_tracks {
            let position = track
                .artist
                .split(['\0', ';'])
                .position(|credit| credit.trim().eq_ignore_ascii_case(artist.trim()));
            match position {
                Some(0) => return false,
                Some(_) => featured = true,
                None => {}
            }
        }
        featured
    }

    pub fn rows(&self) -> Vec<LibraryRow> {
        let artists = self.artists();
        match self.view {
            LibraryView::Artists => {
                let mut rows = Vec::new();
                for (artist, indices) in &artists {
                    let visible = indices
                        .iter()
                        .copied()
                        .filter(|index| {
                            self.text_matches(&self.albums[*index])
                                && self.album_matches_artist_active_status(
                                    &self.albums[*index],
                                    indices,
                                )
                        })
                        .collect::<Vec<_>>();
                    if visible.is_empty() {
                        continue;
                    }
                    let state =
                        Self::artist_state(indices.iter().map(|index| &self.albums[*index]));
                    let expanded = self
                        .focused_artist
                        .as_ref()
                        .is_some_and(|focused| focused.eq_ignore_ascii_case(artist));
                    rows.push(LibraryRow {
                        key: artist.clone(),
                        primary: artist.clone(),
                        secondary: format!("( {} Albums )", visible.len()),
                        status: state.label().to_string(),
                        artwork_path: self
                            .artist_artwork_paths
                            .get(&artist.to_ascii_lowercase())
                            .cloned()
                            .unwrap_or_default(),
                        kind: "artist".to_string(),
                        depth: 0,
                        expanded,
                        selected: indices.iter().any(|index| self.albums[*index].selected),
                        eligible: indices
                            .iter()
                            .any(|index| self.albums[*index].eligible_by_default()),
                        album_count: visible.len(),
                    });
                    if expanded {
                        rows.extend(visible.into_iter().map(|index| {
                            self.album_row(&self.albums[index], "album-child", 1, false)
                        }));
                    }
                }
                rows
            }
            LibraryView::Albums | LibraryView::Folders => self
                .albums
                .iter()
                .enumerate()
                .filter(|(index, _)| self.album_visible(*index, &artists))
                .map(|(_, album)| {
                    if self.view == LibraryView::Albums {
                        self.album_row(album, "album", 0, false)
                    } else {
                        LibraryRow {
                            key: album.snapshot.album_key.clone(),
                            primary: album.snapshot.path.clone(),
                            secondary: format!(
                                "{} · {}",
                                album.snapshot.artist, album.snapshot.title
                            ),
                            status: album.state.label().to_string(),
                            artwork_path: self
                                .album_artwork_paths
                                .get(&album.snapshot.album_key.to_ascii_lowercase())
                                .cloned()
                                .unwrap_or_default(),
                            kind: "folder".to_string(),
                            depth: 0,
                            expanded: false,
                            selected: album.selected,
                            eligible: album.eligible_by_default()
                                || album.state == AlbumState::Processed,
                            album_count: 1,
                        }
                    }
                })
                .collect(),
        }
    }

    /// Albums presented inside Artist Overview. `appearance_keys` adds albums
    /// in which Navidrome tags the focused artist on a song even when the
    /// album artist is Various Artists. A fallback album (no Release/Album
    /// MBID) with a matching artist track is always visible here, regardless
    /// of Library Status. Show All bypasses Library Status for the remaining
    /// albums, never the selected artist scope.
    pub fn artist_overview_album_indices(
        &self,
        appearance_keys: &BTreeSet<String>,
        show_all: bool,
    ) -> Vec<usize> {
        let Some(artist) = self.focused_artist.as_deref() else {
            return Vec::new();
        };
        let mut result = self
            .albums
            .iter()
            .enumerate()
            .filter(|(_, album)| {
                let appearance =
                    appearance_keys.contains(&album.snapshot.album_key.to_ascii_lowercase());
                let matching_fallback_track = album.snapshot.album_mbid.trim().is_empty()
                    && album
                        .snapshot
                        .compilation_tracks
                        .iter()
                        .any(|track| Self::track_matches_artist(&track.artist, artist));
                let fallback_appearance = album.snapshot.album_mbid.trim().is_empty()
                    && (appearance || matching_fallback_track);
                let in_artist_scope = album.snapshot.artist.trim().eq_ignore_ascii_case(artist)
                    || appearance
                    || matching_fallback_track;

                in_artist_scope
                    && (fallback_appearance || show_all || self.album_matches_active_filter(album))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        result.sort_by(|left, right| {
            let left_album = &self.albums[*left].snapshot;
            let right_album = &self.albums[*right].snapshot;
            release_year_number(&right_album.release_year)
                .cmp(&release_year_number(&left_album.release_year))
                .then_with(|| {
                    left_album
                        .title
                        .to_ascii_lowercase()
                        .cmp(&right_album.title.to_ascii_lowercase())
                })
        });
        result
    }

    fn album_row(
        &self,
        album: &AlbumEntry,
        kind: &str,
        depth: usize,
        expanded: bool,
    ) -> LibraryRow {
        LibraryRow {
            key: album.snapshot.album_key.clone(),
            primary: album.snapshot.title.clone(),
            secondary: if kind == "album-child" {
                String::new()
            } else {
                album.snapshot.artist.clone()
            },
            status: album.state.label().to_string(),
            artwork_path: self
                .album_artwork_paths
                .get(&album.snapshot.album_key.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
            kind: kind.to_string(),
            depth,
            expanded,
            selected: album.selected,
            eligible: album.eligible_by_default() || album.state == AlbumState::Processed,
            album_count: 1,
        }
    }

    /// One row per Album without MusicBrainz Album authority for the Fallback
    /// tab. Compilation remains relevant only to the optional embedded-art
    /// track list. Tracks stay hidden until their owning Album is focused.
    pub fn fallback_album_rows(&self) -> Vec<LibraryRow> {
        let artists = self.artists();
        let mut seen = BTreeSet::new();
        self.albums
            .iter()
            .enumerate()
            .filter(|(index, album)| {
                let focused = self
                    .focused_album_key
                    .as_ref()
                    .is_some_and(|focused| focused.eq_ignore_ascii_case(&album.snapshot.album_key));
                Self::is_fallback_album(album)
                    && (focused || self.album_visible(*index, &artists))
                    && seen.insert(album.snapshot.album_key.to_ascii_lowercase())
            })
            .map(|(_, album)| {
                let expanded = self
                    .focused_album_key
                    .as_ref()
                    .is_some_and(|focused| focused.eq_ignore_ascii_case(&album.snapshot.album_key));
                self.album_row(album, "fallback-album", 0, expanded)
            })
            .collect()
    }

    pub fn set_view(&mut self, view: LibraryView) {
        self.view = view;
    }

    /// Switches between the three top-level library tabs.
    ///
    /// Artist drill-down uses `set_view` directly so its focused artist remains
    /// in scope. A user clicking Artists, Albums, or Folders expects the full
    /// top-level model, so that explicit navigation must clear the drill-down.
    pub fn set_top_level_view(&mut self, view: LibraryView) {
        self.clear_artist_scope();
        self.set_view(view);
    }

    pub fn set_text_filter(&mut self, value: impl Into<String>) {
        self.text_filter = value.into();
    }

    pub fn set_artist_filter(&mut self, value: impl Into<String>) {
        self.artist_filter = value.into();
    }

    pub fn set_album_filter(&mut self, value: impl Into<String>) {
        self.album_filter = value.into();
    }

    pub fn set_status_filter(&mut self, value: StatusFilter) {
        self.status_filter = value;
        self.status_filters = if value == StatusFilter::All {
            all_status_filters()
        } else {
            [value].into_iter().collect()
        };
    }

    pub fn set_status_filters(&mut self, values: impl IntoIterator<Item = StatusFilter>) {
        let selected = values
            .into_iter()
            .filter(|value| *value != StatusFilter::All)
            .collect::<BTreeSet<_>>();
        self.status_filters = if selected.is_empty() {
            all_status_filters()
        } else {
            selected
        };
        self.status_filter = StatusFilter::All;
    }

    pub fn toggle_status_filter(&mut self, value: StatusFilter) {
        if value == StatusFilter::All {
            self.status_filters = all_status_filters();
        } else if !self.status_filters.remove(&value) {
            self.status_filters.insert(value);
        }
        self.status_filter = StatusFilter::All;
    }

    pub fn status_filter_enabled(&self, value: StatusFilter) -> bool {
        if value == StatusFilter::All {
            self.status_filters == all_status_filters()
        } else {
            self.status_filters.contains(&value)
        }
    }

    pub fn status_count(&self, value: StatusFilter) -> usize {
        if !self.show_tracks && self.view == LibraryView::Artists {
            let artists = self.artists();
            return if value == StatusFilter::All {
                artists.len()
            } else {
                artists
                    .values()
                    .filter(|indices| self.artist_matches_filter(indices, value))
                    .count()
            };
        }

        self.albums
            .iter()
            .filter(|album| !self.show_tracks || Self::is_fallback_album(album))
            .filter(|album| value == StatusFilter::All || Self::album_matches_filter(album, value))
            .count()
    }

    pub fn state_scope_label(&self) -> &'static str {
        if self.show_tracks {
            "Fallback"
        } else {
            match self.view {
                LibraryView::Artists => "Artists",
                LibraryView::Albums => "Albums",
                LibraryView::Folders => "Folders",
            }
        }
    }

    pub fn status_label(&self, value: StatusFilter) -> String {
        match value {
            StatusFilter::ArtistComplete => {
                format!(
                    "{} Complete",
                    self.state_scope_label().trim_end_matches('s')
                )
            }
            StatusFilter::ArtistContainsBypass => {
                format!("{} Bypass", self.state_scope_label().trim_end_matches('s'))
            }
            _ => value.label().to_string(),
        }
    }

    pub fn embedded_art_track_count(&self) -> usize {
        self.albums
            .iter()
            .flat_map(|album| album.snapshot.compilation_tracks.iter())
            .filter(|track| track.embedded_artwork_recorded)
            .count()
    }

    pub fn set_show_tracks(&mut self, enabled: bool) {
        self.show_tracks = enabled;
    }

    pub fn set_artist_artwork_path(&mut self, artist: &str, path: &Path) {
        // Navidrome's cache writer validates the file before publishing this
        // path. Avoid thousands of synchronous metadata calls on the UI
        // thread when a full thumbnail refresh completes.
        if !artist.trim().is_empty() && !path.as_os_str().is_empty() {
            self.artist_artwork_paths.insert(
                artist.trim().to_ascii_lowercase(),
                path.to_string_lossy().into_owned(),
            );
        }
    }

    pub fn set_album_artwork_path(&mut self, album_key: &str, path: &Path) {
        if !album_key.trim().is_empty() && !path.as_os_str().is_empty() {
            self.album_artwork_paths.insert(
                album_key.trim().to_ascii_lowercase(),
                path.to_string_lossy().into_owned(),
            );
        }
    }

    pub fn set_artist_navidrome_id(&mut self, artist: &str, id: &str) {
        if !artist.trim().is_empty() && !id.trim().is_empty() {
            self.artist_navidrome_ids
                .insert(artist.trim().to_ascii_lowercase(), id.trim().to_string());
        }
    }

    pub fn focused_album_is_bypassed(&self) -> bool {
        self.focused_album_key.as_ref().is_some_and(|focused| {
            self.albums.iter().any(|album| {
                album.snapshot.album_key.eq_ignore_ascii_case(focused)
                    && album.state == AlbumState::Bypassed
            })
        })
    }

    pub fn clear_artist_scope(&mut self) {
        self.focused_artist = None;
    }

    pub fn toggle_artist_scope(&mut self, artist: &str) -> bool {
        if !self
            .albums
            .iter()
            .any(|album| album.snapshot.artist.eq_ignore_ascii_case(artist))
        {
            return false;
        }
        if self
            .focused_artist
            .as_ref()
            .is_some_and(|focused| focused.eq_ignore_ascii_case(artist))
        {
            self.focused_artist = None;
        } else {
            self.focused_artist = Some(artist.to_string());
        }
        true
    }

    pub fn focus_album(&mut self, key: &str) -> bool {
        let Some(album) = self
            .albums
            .iter()
            .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        else {
            return false;
        };
        self.focused_album_key = Some(album.snapshot.album_key.clone());
        true
    }

    pub fn focus_artist(&mut self, artist: &str) -> bool {
        let Some(album) = self
            .albums
            .iter()
            .find(|album| album.snapshot.artist.eq_ignore_ascii_case(artist))
        else {
            return false;
        };
        self.focused_artist = Some(album.snapshot.artist.clone());
        true
    }

    pub fn toggle_fallback_album_focus(&mut self, key: &str) -> bool {
        let Some(album) = self
            .albums
            .iter()
            .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        else {
            return false;
        };
        if !Self::is_fallback_album(album) {
            return false;
        }
        if self
            .focused_album_key
            .as_ref()
            .is_some_and(|focused| focused.eq_ignore_ascii_case(&album.snapshot.album_key))
        {
            self.focused_album_key = None;
        } else {
            self.focused_album_key = Some(album.snapshot.album_key.clone());
        }
        true
    }

    pub fn fallback_track_hydration_request(
        &self,
        key: &str,
    ) -> Option<(String, String, Vec<MediaTrackSnapshot>)> {
        let album = self.albums.iter().find(|album| {
            album.snapshot.album_key.eq_ignore_ascii_case(key)
                && Self::is_fallback_album(album)
                && self
                    .focused_album_key
                    .as_ref()
                    .is_some_and(|focused| focused.eq_ignore_ascii_case(&album.snapshot.album_key))
        })?;
        // Re-read a focused fallback folder every time it is expanded. Some
        // older snapshots have a zero/stale track_count or only the tracks
        // already present in the completion ledger; treating that cache as a
        // complete list leaves the album visibly expanded with no rows.
        Some((
            album.snapshot.album_key.clone(),
            album.snapshot.path.clone(),
            album.snapshot.compilation_tracks.clone(),
        ))
    }

    pub fn replace_fallback_tracks(&mut self, key: &str, tracks: Vec<MediaTrackSnapshot>) -> bool {
        let Some(album) = self.albums.iter_mut().find(|album| {
            album.snapshot.album_key.eq_ignore_ascii_case(key)
                && album.snapshot.album_mbid.trim().is_empty()
        }) else {
            return false;
        };
        let completed = album
            .snapshot
            .compilation_tracks
            .iter()
            .filter(|track| track.embedded_artwork_recorded)
            .map(|track| track.path.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        album.snapshot.compilation_tracks = tracks
            .into_iter()
            .map(|mut track| {
                track.embedded_artwork_recorded |=
                    completed.contains(&track.path.to_ascii_lowercase());
                track
            })
            .collect();
        true
    }

    pub fn toggle_row(&mut self, key: &str, include_bypassed: bool) -> SelectionResult {
        match self.view {
            LibraryView::Artists => self.toggle_artist(key, include_bypassed),
            LibraryView::Albums | LibraryView::Folders => self.toggle_album(key, include_bypassed),
        }
    }

    pub fn toggle_artist(&mut self, artist: &str, include_bypassed: bool) -> SelectionResult {
        let indices = self
            .albums
            .iter()
            .enumerate()
            .filter(|(_, album)| album.snapshot.artist.eq_ignore_ascii_case(artist))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return SelectionResult::NotFound;
        }
        self.focused_artist = Some(self.albums[indices[0]].snapshot.artist.clone());
        let selected = !indices.iter().any(|index| self.albums[*index].selected);
        let deselected_track_paths = if selected {
            Vec::new()
        } else {
            indices
                .iter()
                .flat_map(|index| self.albums[*index].snapshot.compilation_tracks.iter())
                .map(|track| track.path.to_ascii_lowercase())
                .collect::<Vec<_>>()
        };
        for index in indices {
            let album = &mut self.albums[index];
            if !selected {
                album.selected = false;
                album.bypass_override = false;
            } else if album.eligible_by_default() {
                album.selected = true;
                album.bypass_override = album.state == AlbumState::Bypassed
                    && album.compilation_track_artwork_pending();
            } else if album.state == AlbumState::Bypassed && include_bypassed {
                album.selected = true;
                album.bypass_override = true;
            }
        }
        if !deselected_track_paths.is_empty() {
            self.selected_compilation_track_paths
                .retain(|path| !deselected_track_paths.contains(path));
        }
        SelectionResult::Changed
    }

    pub fn toggle_album(&mut self, key: &str, include_bypassed: bool) -> SelectionResult {
        let Some(album) = self
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        else {
            return SelectionResult::NotFound;
        };
        self.focused_album_key = Some(album.snapshot.album_key.clone());
        let track_paths = album
            .snapshot
            .compilation_tracks
            .iter()
            .map(|track| track.path.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let had_selected_tracks = track_paths
            .iter()
            .any(|path| self.selected_compilation_track_paths.contains(path));
        if album.selected && !had_selected_tracks {
            album.selected = false;
            album.bypass_override = false;
            self.selected_compilation_track_paths
                .retain(|path| !track_paths.contains(path));
            return SelectionResult::Changed;
        }
        if had_selected_tracks {
            self.selected_compilation_track_paths
                .retain(|path| !track_paths.contains(path));
            album.selected = false;
            album.bypass_override = false;
        }
        match album.state {
            AlbumState::TimeoutActive => {
                SelectionResult::BlockedByTimeout(album.snapshot.title.clone())
            }
            AlbumState::Bypassed if !include_bypassed => {
                SelectionResult::NeedsBypassOverride(album.snapshot.title.clone())
            }
            AlbumState::Bypassed => {
                album.selected = true;
                album.bypass_override = true;
                SelectionResult::Changed
            }
            _ => {
                album.selected = true;
                SelectionResult::Changed
            }
        }
    }

    /// Replaces the current selection with one Album. Ordinary row clicks use
    /// this path; direct radio clicks continue to use `toggle_album` so they
    /// can deliberately build a multi-selection.
    pub fn select_album_exclusive(&mut self, key: &str, include_bypassed: bool) -> SelectionResult {
        let Some(index) = self
            .albums
            .iter()
            .position(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        else {
            return SelectionResult::NotFound;
        };
        match self.albums[index].state {
            AlbumState::TimeoutActive => {
                return SelectionResult::BlockedByTimeout(
                    self.albums[index].snapshot.title.clone(),
                );
            }
            AlbumState::Bypassed if !include_bypassed => {
                return SelectionResult::NeedsBypassOverride(
                    self.albums[index].snapshot.title.clone(),
                );
            }
            _ => {}
        }
        self.select_none();
        let album = &mut self.albums[index];
        self.focused_album_key = Some(album.snapshot.album_key.clone());
        album.selected = true;
        album.bypass_override = album.state == AlbumState::Bypassed;
        SelectionResult::Changed
    }

    /// Replaces the current selection with every manually selectable Album for
    /// one Artist. Row activation is an explicit manual reprocessing action,
    /// so processed Albums remain selectable just as they are in Album view.
    /// Direct artist-radio clicks remain additive and default-eligible through
    /// `toggle_artist`.
    pub fn select_artist_exclusive(
        &mut self,
        artist: &str,
        include_bypassed: bool,
    ) -> SelectionResult {
        if !self
            .albums
            .iter()
            .any(|album| album.snapshot.artist.eq_ignore_ascii_case(artist))
        {
            return SelectionResult::NotFound;
        }
        let indices = self
            .albums
            .iter()
            .enumerate()
            .filter(|(_, album)| album.snapshot.artist.eq_ignore_ascii_case(artist))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        self.select_none();
        self.focused_artist = Some(self.albums[indices[0]].snapshot.artist.clone());
        for index in indices {
            let album = &mut self.albums[index];
            match album.state {
                AlbumState::TimeoutActive => {}
                AlbumState::Bypassed if !include_bypassed => {}
                AlbumState::Bypassed => {
                    album.selected = true;
                    album.bypass_override = true;
                }
                _ => album.selected = true,
            }
        }
        self.focused_album_key = None;
        SelectionResult::Changed
    }

    pub fn select_none(&mut self) {
        for album in &mut self.albums {
            album.selected = false;
            album.bypass_override = false;
        }
        self.selected_compilation_track_paths.clear();
    }

    pub fn toggle_compilation_track(
        &mut self,
        album_key: &str,
        track_path: &str,
        include_bypassed: bool,
    ) -> SelectionResult {
        let Some(index) = self
            .albums
            .iter()
            .position(|album| album.snapshot.album_key.eq_ignore_ascii_case(album_key))
        else {
            return SelectionResult::NotFound;
        };
        let album = &self.albums[index];
        if !album
            .snapshot
            .compilation_tracks
            .iter()
            .any(|track| track.path.eq_ignore_ascii_case(track_path))
        {
            return SelectionResult::NotFound;
        }
        if !self.show_tracks
            && self.view == LibraryView::Artists
            && self.focused_artist.as_ref().is_some_and(|artist| {
                !album.snapshot.compilation_tracks.iter().any(|track| {
                    track.path.eq_ignore_ascii_case(track_path)
                        && Self::track_matches_artist(&track.artist, artist)
                })
            })
        {
            // Artist Overview is intentionally track-scoped for fallback
            // albums. Reject stale UI callbacks or cached rows belonging to a
            // different performer instead of searching MusicBrainz for the
            // wrong track.
            return SelectionResult::NotFound;
        }
        if album.state == AlbumState::TimeoutActive {
            return SelectionResult::BlockedByTimeout(album.snapshot.title.clone());
        }
        if album.state == AlbumState::Bypassed && !include_bypassed {
            return SelectionResult::NeedsBypassOverride(album.snapshot.title.clone());
        }
        let normalized = track_path.to_ascii_lowercase();
        let was_selected = self.selected_compilation_track_paths.contains(&normalized);
        let focused_album_key = album.snapshot.album_key.clone();

        if was_selected {
            self.selected_compilation_track_paths.remove(&normalized);
        } else {
            // A fallback track is a complete, track-scoped processing target.
            // Replace any Artist/Album selection that led the operator here so
            // a normal Artist-folder album cannot run first and overwrite the
            // compilation preview.
            self.select_none();
            self.selected_compilation_track_paths.insert(normalized);
        }
        self.focused_album_key = Some(focused_album_key);

        let album = &mut self.albums[index];
        let selected = album.snapshot.compilation_tracks.iter().any(|track| {
            self.selected_compilation_track_paths
                .contains(&track.path.to_ascii_lowercase())
        });
        album.selected = selected;
        album.bypass_override = selected && album.state == AlbumState::Bypassed;
        SelectionResult::Changed
    }

    pub fn select_all(&mut self) {
        self.select_none();
        for album in &mut self.albums {
            if album.eligible_by_default() {
                album.selected = true;
                album.bypass_override = album.state == AlbumState::Bypassed
                    && album.compilation_track_artwork_pending();
            }
        }
    }

    pub fn select_all_for_focused_artist(&mut self) -> Result<usize, String> {
        let artist = self
            .focused_artist
            .clone()
            .ok_or_else(|| "Select [ALL] requires an Artist.".to_string())?;
        self.select_none();
        for album in &mut self.albums {
            if album.snapshot.artist.eq_ignore_ascii_case(&artist) && album.eligible_by_default() {
                album.selected = true;
                album.bypass_override = album.state == AlbumState::Bypassed
                    && album.compilation_track_artwork_pending();
            }
        }
        Ok(self.selected_count())
    }

    pub fn select_all_for_review(&mut self) -> usize {
        let artists = self.artists();
        let visible = self
            .albums
            .iter()
            .enumerate()
            .filter(|(index, album)| {
                self.album_is_in_active_review_view(album)
                    && self.album_matches_active_status(*index, &artists)
                    && (self.show_tracks
                        || self.focused_artist.as_ref().is_none_or(|artist| {
                            album.snapshot.artist.eq_ignore_ascii_case(artist)
                        }))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        self.select_none();
        for index in visible {
            let album = &mut self.albums[index];
            if album.eligible_by_default() {
                album.selected = true;
                album.bypass_override = album.state == AlbumState::Bypassed
                    && album.compilation_track_artwork_pending();
            }
        }
        self.selected_count()
    }

    pub fn select_filtered(&mut self) {
        let artists = self.artists();
        let visible = self
            .albums
            .iter()
            .enumerate()
            .filter(|(index, album)| {
                self.album_is_in_active_review_view(album) && self.album_visible(*index, &artists)
            })
            .map(|(index, _)| index)
            .collect::<BTreeSet<_>>();
        self.select_none();
        for index in visible {
            let album = &mut self.albums[index];
            if matches!(
                album.state,
                AlbumState::New | AlbumState::Incomplete | AlbumState::Processed
            ) || album.compilation_track_artwork_pending()
            {
                album.selected = true;
                album.bypass_override = album.state == AlbumState::Bypassed
                    && album.compilation_track_artwork_pending();
            }
        }
    }

    pub fn select_filtered_for_review(&mut self) -> Result<usize, String> {
        if self.text_filter.trim().is_empty()
            && self.artist_filter.trim().is_empty()
            && self.album_filter.trim().is_empty()
            && self.status_filters == all_status_filters()
            && !self.show_tracks
        {
            return Err(
                "Select [FILTERED] requires Artist, Album, Library Status, or Fallback scope."
                    .to_string(),
            );
        }
        self.select_filtered();
        Ok(self.selected_count())
    }

    pub fn queue(&self, scope: ScanScope) -> Vec<ScanTarget> {
        let mut queue = Vec::new();
        for album in self.albums.iter().filter(|album| match scope {
            ScanScope::Selected => album.selected && album.queue_eligible(),
            ScanScope::All => album.queue_eligible(),
        }) {
            let selected_tracks = album
                .snapshot
                .compilation_tracks
                .iter()
                .filter(|track| {
                    self.selected_compilation_track_paths
                        .contains(&track.path.to_ascii_lowercase())
                })
                .collect::<Vec<_>>();
            if selected_tracks.is_empty() {
                queue.push(ScanTarget {
                    album_key: album.snapshot.album_key.clone(),
                    artist: album.snapshot.artist.clone(),
                    album: album.snapshot.title.clone(),
                    path: album.snapshot.path.clone(),
                    compilation_track_path: None,
                    bypass_override: album.bypass_override,
                });
            } else {
                queue.extend(selected_tracks.into_iter().map(|track| ScanTarget {
                    album_key: album.snapshot.album_key.clone(),
                    artist: album.snapshot.artist.clone(),
                    album: album.snapshot.title.clone(),
                    path: album.snapshot.path.clone(),
                    compilation_track_path: Some(track.path.clone()),
                    bypass_override: album.bypass_override,
                }));
            }
        }
        queue
    }

    pub fn mark_target_completed(&mut self, target: &ScanTarget) {
        if let Some(track_path) = target.compilation_track_path.as_ref() {
            self.selected_compilation_track_paths
                .remove(&track_path.to_ascii_lowercase());
        }
        if let Some(album) = self.albums.iter_mut().find(|album| {
            album
                .snapshot
                .album_key
                .eq_ignore_ascii_case(&target.album_key)
        }) {
            let has_more_tracks = album.snapshot.compilation_tracks.iter().any(|track| {
                self.selected_compilation_track_paths
                    .contains(&track.path.to_ascii_lowercase())
            });
            album.selected = has_more_tracks;
            if !has_more_tracks {
                album.bypass_override = false;
            }
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: MediaSnapshot) -> Result<(), String> {
        let prior_cover_paths = self
            .albums
            .iter()
            .map(|album| {
                (
                    album.snapshot.album_key.to_ascii_lowercase(),
                    album.snapshot.cover_path.clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        let recorded_compilation_tracks = self
            .albums
            .iter()
            .flat_map(|album| album.snapshot.compilation_tracks.iter())
            .filter(|track| track.embedded_artwork_recorded)
            .map(|track| track.path.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        let selected = self
            .albums
            .iter()
            .filter(|album| album.selected)
            .map(|album| {
                (
                    album.snapshot.album_key.to_ascii_lowercase(),
                    album.bypass_override,
                )
            })
            .collect::<HashMap<_, _>>();
        let view = self.view;
        let status_filter = self.status_filter;
        let status_filters = self.status_filters.clone();
        let text_filter = self.text_filter.clone();
        let artist_filter = self.artist_filter.clone();
        let album_filter = self.album_filter.clone();
        let show_tracks = self.show_tracks;
        let focused_artist = self.focused_artist.clone();
        let focused_album_key = self.focused_album_key.clone();
        let selected_compilation_track_paths = self.selected_compilation_track_paths.clone();
        let artist_artwork_paths = self.artist_artwork_paths.clone();
        let album_artwork_paths = self.album_artwork_paths.clone();
        let artist_navidrome_ids = self.artist_navidrome_ids.clone();
        let mut refreshed = Self::from_snapshot(snapshot)?;
        refreshed.view = view;
        refreshed.status_filter = status_filter;
        refreshed.status_filters = status_filters;
        refreshed.text_filter = text_filter;
        refreshed.artist_filter = artist_filter;
        refreshed.album_filter = album_filter;
        refreshed.show_tracks = show_tracks;
        refreshed.focused_artist = focused_artist;
        refreshed.focused_album_key = focused_album_key;
        refreshed.selected_compilation_track_paths = selected_compilation_track_paths;
        refreshed.artist_artwork_paths = artist_artwork_paths;
        refreshed.album_artwork_paths = album_artwork_paths;
        refreshed.artist_navidrome_ids = artist_navidrome_ids;
        for album in &refreshed.albums {
            let key = album.snapshot.album_key.to_ascii_lowercase();
            let cover_changed = prior_cover_paths
                .get(&key)
                .is_none_or(|previous| previous != &album.snapshot.cover_path);
            if cover_changed && Path::new(&album.snapshot.cover_path).is_file() {
                refreshed
                    .album_artwork_paths
                    .insert(key, album.snapshot.cover_path.clone());
            }
        }
        for album in &mut refreshed.albums {
            for track in &mut album.snapshot.compilation_tracks {
                if recorded_compilation_tracks.contains(&track.path.to_ascii_lowercase()) {
                    track.embedded_artwork_recorded = true;
                }
            }
            if let Some(override_value) =
                selected.get(&album.snapshot.album_key.to_ascii_lowercase())
            {
                album.selected = true;
                album.bypass_override = *override_value;
            }
        }
        *self = refreshed;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceState {
    pub name: String,
    pub enabled: bool,
    pub candidate_count: usize,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateState {
    pub index: usize,
    pub source: String,
    pub dimensions: String,
    pub width: u32,
    pub height: u32,
    pub projected_width: u32,
    pub projected_height: u32,
    pub source_range_class: String,
    pub range_class: String,
    pub policy_status: String,
    pub policy_reason: String,
    pub image_type: String,
    pub cache_path: String,
    pub url: String,
    pub local_origin: String,
    pub acceptable: bool,
    pub strict_override_active: bool,
    pub upscaled: bool,
    pub approved: bool,
    pub recommended: bool,
    pub selected: bool,
}

impl CandidateState {
    pub fn is_local(&self) -> bool {
        self.source.eq_ignore_ascii_case("local")
            || self.source.eq_ignore_ascii_case("webpstill")
            || !self.local_origin.trim().is_empty()
    }

    pub fn is_rejected(&self) -> bool {
        self.policy_status.eq_ignore_ascii_case("reject") || !self.acceptable
    }

    pub fn is_upscalable(&self) -> bool {
        self.upscaled && self.acceptable && !self.is_rejected()
    }

    fn matches_filter(&self, dimension: &str, key: &str) -> bool {
        match dimension {
            "source" => self.source.eq_ignore_ascii_case(key),
            "range" => {
                range_filter_matches(&self.source_range_class, key)
                    || range_filter_matches(&self.range_class, key)
            }
            "policy" if key.eq_ignore_ascii_case("Acceptable") => self.acceptable,
            "policy" if key.eq_ignore_ascii_case("Strict") => self.strict_override_active,
            "type" if key.eq_ignore_ascii_case("Recommended") => self.recommended,
            "type" if key.eq_ignore_ascii_case("Upscalable") => self.is_upscalable(),
            "type" if key.eq_ignore_ascii_case("Rejected") => self.is_rejected(),
            "type" if key.eq_ignore_ascii_case("Local") => self.is_local(),
            "type" if key.eq_ignore_ascii_case("Samples") => {
                self.source.eq_ignore_ascii_case("samples")
            }
            "type" => self.image_type.eq_ignore_ascii_case(key),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFilterDisposition {
    Neutral,
    Included,
    Disregarded,
}

impl CandidateFilterDisposition {
    pub fn label(self) -> &'static str {
        match self {
            Self::Neutral => "Neutral",
            Self::Included => "Included",
            Self::Disregarded => "Disregarded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MusicBrainzAuthority {
    pub artist: String,
    pub album: String,
    pub recording_mbid: String,
    pub artist_mbids: String,
    pub release_mbid: String,
    pub release_title: String,
    pub release_artist: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicBrainzResult {
    pub index: usize,
    pub recording_mbid: String,
    pub recording_title: String,
    pub recording_artist: String,
    pub score: u16,
    pub release_title: String,
    pub release_artist: String,
    pub release_date: String,
    pub release_class: String,
    pub country: String,
    pub resolution: String,
    pub release_mbid: String,
    pub release_group_mbid: String,
    pub release_group_title: String,
    pub url: String,
    pub current: bool,
    pub visited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    Idle,
    Running,
    AwaitingCandidate,
    AwaitingMusicBrainz,
    Stopping,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustmentState {
    pub preserve_source_resolution: bool,
    pub adaptive_defaults: bool,
    pub picture_percent: i32,
    pub sharpen_percent: i32,
    pub softness_percent: i32,
    pub contrast_percent: i32,
    pub exposure_percent: i32,
    pub brightness_percent: i32,
    pub gamma_percent: i32,
    pub color_temperature: i32,
    pub apply_edit_profile: bool,
    pub edit_existing_cover: bool,
}

impl AdjustmentState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            preserve_source_resolution: false,
            adaptive_defaults: config.output.upscale_adaptive_defaults,
            picture_percent: config.output.upscale_picture_percent,
            sharpen_percent: config.output.upscale_sharpen_percent,
            softness_percent: config.output.upscale_softness_percent,
            contrast_percent: config.output.upscale_contrast_percent,
            exposure_percent: config.output.upscale_exposure_percent,
            brightness_percent: config.output.upscale_brightness_percent,
            gamma_percent: config.output.upscale_gamma_percent,
            color_temperature: config.output.upscale_color_temperature,
            apply_edit_profile: true,
            edit_existing_cover: false,
        }
    }

    pub fn as_decision_fields(&self) -> Value {
        json!({
            "preserve_source_resolution": self.preserve_source_resolution,
            "upscale_adaptive_defaults": self.adaptive_defaults,
            "upscale_picture_percent": self.picture_percent,
            "upscale_sharpen_percent": self.sharpen_percent,
            "upscale_softness_percent": self.softness_percent,
            "upscale_contrast_percent": self.contrast_percent,
            "upscale_exposure_percent": self.exposure_percent,
            "upscale_brightness_percent": self.brightness_percent,
            "upscale_gamma_percent": self.gamma_percent,
            "upscale_color_temperature": self.color_temperature,
            "apply_edit_profile": self.apply_edit_profile,
            "edit_existing_cover": self.edit_existing_cover,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ArtistOverviewCacheEntry {
    pub biography: String,
    pub external_url: String,
    pub musicbrainz_id: String,
    pub banner_path: String,
    pub similar_artists: Vec<(String, String)>,
    pub genres: Vec<String>,
    pub roles: Vec<String>,
    pub album_keys: BTreeSet<String>,
    pub track_paths_by_album: HashMap<String, Vec<String>>,
    pub album_ratings: HashMap<String, u32>,
    pub artwork_candidates: Vec<(String, String)>,
    pub top_songs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ApplicationState {
    pub config: Config,
    pub library: LibraryState,
    pub sources: Vec<SourceState>,
    pub scan_scope: ScanScope,
    pub auto_scan_enabled: bool,
    pub launch_mode: LaunchMode,
    pub phase: RunPhase,
    pub current_album_path: String,
    pub current_artist: String,
    pub current_album: String,
    pub current_cover_path: String,
    /// Track currently owned by the running fallback review. Unlike the
    /// library selection set, this authority must survive harmless browsing
    /// while the controller is waiting for a decision.
    pub current_compilation_track_path: String,
    pub edit_preview_path: String,
    /// Embedded artwork extracted from the explicitly selected compilation
    /// track. It remains authoritative while that track is selected, even
    /// when the fallback scan clears the generic edit preview at album start.
    pub compilation_track_preview_path: String,
    pub compilation_track_preview_owner: String,
    /// Stable cached source for the selected MusicBrainz release cover. The
    /// rendered adjustment preview may replace `edit_preview_path`, so this
    /// path remains separate for subsequent preview renders.
    pub musicbrainz_cover_preview_path: String,
    pub musicbrainz_hover_preview_path: String,
    /// Result index owning `musicbrainz_hover_preview_path`. Keeping this
    /// separate prevents a cached hover image from being shown for the next
    /// row while its cover is still loading.
    pub musicbrainz_hover_preview_index: Option<usize>,
    /// At most one hover cover request may be active. A full UI sync rebuilds
    /// result rows, which can briefly repeat hover notifications.
    pub musicbrainz_hover_preview_loading: Option<usize>,
    /// Covers already resolved for the current MusicBrainz result page. The
    /// visible hover slot above remains singular, while this map prevents
    /// revisiting a row from starting another Cover Art Archive request.
    pub musicbrainz_preview_cache: HashMap<usize, String>,
    /// MusicBrainz result indexes with a cover request currently in flight.
    /// This is a set because moving across rows must not forget older work.
    pub musicbrainz_preview_inflight: HashSet<usize>,
    /// User-selected artist image waiting for an explicit Navidrome WRITE.
    /// This is transient UI state and is never treated as an album candidate.
    pub artist_artwork_draft_path: String,
    /// Descriptive metadata projected from the currently focused Navidrome
    /// artist. These fields are transient and never become library authority.
    pub artist_overview_biography: String,
    pub artist_overview_external_url: String,
    pub artist_overview_musicbrainz_id: String,
    pub artist_overview_banner_path: String,
    pub artist_overview_similar_artists: Vec<(String, String)>,
    pub artist_overview_genres: Vec<String>,
    pub artist_overview_roles: Vec<String>,
    pub artist_overview_album_keys: BTreeSet<String>,
    /// Validated Navidrome song paths for the focused artist, kept separate
    /// from the sparse SQLite compilation inventory and never used to scan.
    pub artist_overview_track_paths: HashMap<String, Vec<String>>,
    pub artist_overview_album_ratings: HashMap<String, u32>,
    pub artist_overview_artwork_candidates: Vec<(String, String)>,
    pub artist_overview_top_songs: Vec<String>,
    pub artist_overview_top_songs_loading: bool,
    pub artist_overview_loading: bool,
    pub artist_overview_show_all: bool,
    pub artist_artwork_mode: bool,
    /// Per-session projection of Navidrome's authoritative artist metadata.
    /// Downloaded images live in the disposable runtime cache; no duplicate
    /// persistent metadata database is created by the Windows UI.
    pub artist_overview_cache: HashMap<String, ArtistOverviewCacheEntry>,
    /// Rows already offered to the lazy Navidrome thumbnail loader during
    /// this run. Missing remote images are remembered too, preventing scroll
    /// recycling from repeatedly requesting the same absent artwork.
    pub thumbnail_hydration_attempts: HashSet<String>,
    pub candidates: Vec<CandidateState>,
    pub selected_candidate: Option<usize>,
    pub candidate_source_filter: Option<String>,
    pub candidate_range_filter: Option<String>,
    pub candidate_included_sources: BTreeSet<String>,
    pub candidate_included_types: BTreeSet<String>,
    pub candidate_included_policies: BTreeSet<String>,
    pub candidate_included_ranges: BTreeSet<String>,
    pub candidate_excluded_sources: BTreeSet<String>,
    pub candidate_excluded_types: BTreeSet<String>,
    pub candidate_excluded_policies: BTreeSet<String>,
    pub candidate_excluded_ranges: BTreeSet<String>,
    pub comparison_candidates: BTreeSet<usize>,
    pub hover_enabled: bool,
    pub hovered_candidate: Option<usize>,
    pub fallback_active: bool,
    pub fallback_reason: String,
    pub fallback_artist: String,
    pub fallback_album: String,
    pub musicbrainz_retry_available: bool,
    pub musicbrainz_matches_available: bool,
    pub musicbrainz_back_available: bool,
    pub musicbrainz_artist_filter: Option<String>,
    pub musicbrainz_release_type_filter: Option<String>,
    pub musicbrainz_authority: MusicBrainzAuthority,
    pub musicbrainz_results: Vec<MusicBrainzResult>,
    pub selected_musicbrainz_result: Option<usize>,
    pub hovered_musicbrainz_result: Option<usize>,
    pub musicbrainz_identity_status: String,
    pub musicbrainz_identity_message: String,
    pub musicbrainz_recording_status: String,
    pub musicbrainz_artist_status: String,
    pub musicbrainz_focused_artist_mbid: String,
    pub musicbrainz_friendly_artist: String,
    pub musicbrainz_friendly_title: String,
    pub musicbrainz_used_fuzzy_search: bool,
    pub adjustments: AdjustmentState,
    pub primary_adjustments: Vec<String>,
    pub media_selector_expanded: bool,
    pub candidate_filter_expanded: bool,
    pub artwork_editing_expanded: bool,
    pub show_artwork: bool,
    pub upscale_preview_only: bool,
    pub disable_intro_message: bool,
    pub disable_confirmation_messages: bool,
    pub intro_dismissed: bool,
    pub completion_dialog_visible: bool,
    pub completion_message: String,
    pub log: Vec<String>,
    pub last_error: Option<String>,
    pub service_status: String,
    pub operation_busy: bool,
    pub operation_status: String,
    pub operation_tone: String,
    pub run_total: usize,
    pub run_completed: usize,
    pub run_failed: usize,
}

fn candidate_source_card_names(config: &Config) -> Vec<String> {
    let mut names = vec!["samples".to_string(), "local".to_string()];
    for source in &config.sources.cover_sources {
        if !names.iter().any(|name| name.eq_ignore_ascii_case(source)) {
            names.push(source.clone());
        }
    }
    if !names
        .iter()
        .any(|name| name.eq_ignore_ascii_case("navidrome"))
    {
        names.push("navidrome".to_string());
    }
    names
}

impl ApplicationState {
    pub fn new(config: Config, snapshot: MediaSnapshot) -> Result<Self, String> {
        let enabled_source_names =
            resolve_sources(&config.sources, &config.source_policies, None, None, &[])?;
        let adjustments = AdjustmentState::from_config(&config);
        let launch_mode = match config.mode {
            Mode::Read => LaunchMode::Read,
            Mode::Write => LaunchMode::LiveWrite,
        };
        let sources = candidate_source_card_names(&config)
            .into_iter()
            .map(|name| {
                let is_samples = name.eq_ignore_ascii_case("samples");
                let enabled = !is_samples
                    && (name.eq_ignore_ascii_case("local")
                        || enabled_source_names
                            .iter()
                            .any(|enabled| enabled.eq_ignore_ascii_case(&name)));
                SourceState {
                    enabled,
                    candidate_count: 0,
                    status: if is_samples && !config.samples.sample_write {
                        "Excluded".to_string()
                    } else if is_samples {
                        "No images".to_string()
                    } else if !enabled {
                        "Excluded".to_string()
                    } else {
                        "Ready".to_string()
                    },
                    name,
                }
            })
            .collect();
        Ok(Self {
            config,
            library: LibraryState::from_snapshot(snapshot)?,
            sources,
            scan_scope: ScanScope::Selected,
            auto_scan_enabled: false,
            launch_mode,
            phase: RunPhase::Idle,
            current_album_path: String::new(),
            current_artist: String::new(),
            current_album: String::new(),
            current_cover_path: String::new(),
            current_compilation_track_path: String::new(),
            edit_preview_path: String::new(),
            compilation_track_preview_path: String::new(),
            compilation_track_preview_owner: String::new(),
            musicbrainz_cover_preview_path: String::new(),
            musicbrainz_hover_preview_path: String::new(),
            musicbrainz_hover_preview_index: None,
            musicbrainz_hover_preview_loading: None,
            musicbrainz_preview_cache: HashMap::new(),
            musicbrainz_preview_inflight: HashSet::new(),
            artist_artwork_draft_path: String::new(),
            artist_overview_biography: String::new(),
            artist_overview_external_url: String::new(),
            artist_overview_musicbrainz_id: String::new(),
            artist_overview_banner_path: String::new(),
            artist_overview_similar_artists: Vec::new(),
            artist_overview_genres: Vec::new(),
            artist_overview_roles: Vec::new(),
            artist_overview_album_keys: BTreeSet::new(),
            artist_overview_track_paths: HashMap::new(),
            artist_overview_album_ratings: HashMap::new(),
            artist_overview_artwork_candidates: Vec::new(),
            artist_overview_top_songs: Vec::new(),
            artist_overview_top_songs_loading: false,
            artist_overview_loading: false,
            artist_overview_show_all: false,
            artist_artwork_mode: false,
            artist_overview_cache: HashMap::new(),
            thumbnail_hydration_attempts: HashSet::new(),
            candidates: Vec::new(),
            selected_candidate: None,
            candidate_source_filter: None,
            candidate_range_filter: None,
            candidate_included_sources: BTreeSet::new(),
            candidate_included_types: BTreeSet::new(),
            candidate_included_policies: BTreeSet::new(),
            candidate_included_ranges: BTreeSet::new(),
            candidate_excluded_sources: BTreeSet::new(),
            candidate_excluded_types: BTreeSet::new(),
            candidate_excluded_policies: BTreeSet::new(),
            candidate_excluded_ranges: BTreeSet::new(),
            comparison_candidates: BTreeSet::new(),
            hover_enabled: false,
            hovered_candidate: None,
            fallback_active: false,
            fallback_reason: String::new(),
            fallback_artist: String::new(),
            fallback_album: String::new(),
            musicbrainz_retry_available: false,
            musicbrainz_matches_available: false,
            musicbrainz_back_available: false,
            musicbrainz_artist_filter: None,
            musicbrainz_release_type_filter: None,
            musicbrainz_authority: MusicBrainzAuthority::default(),
            musicbrainz_results: Vec::new(),
            selected_musicbrainz_result: None,
            hovered_musicbrainz_result: None,
            musicbrainz_identity_status: "pending".to_string(),
            musicbrainz_identity_message: String::new(),
            musicbrainz_recording_status: "pending".to_string(),
            musicbrainz_artist_status: "pending".to_string(),
            musicbrainz_focused_artist_mbid: String::new(),
            musicbrainz_friendly_artist: String::new(),
            musicbrainz_friendly_title: String::new(),
            musicbrainz_used_fuzzy_search: false,
            adjustments,
            primary_adjustments: default_primary_adjustments(),
            media_selector_expanded: true,
            candidate_filter_expanded: true,
            artwork_editing_expanded: true,
            show_artwork: true,
            upscale_preview_only: true,
            disable_intro_message: false,
            disable_confirmation_messages: false,
            intro_dismissed: false,
            completion_dialog_visible: false,
            completion_message: String::new(),
            log: vec!["SPLINED Rust application initialized.".to_string()],
            last_error: None,
            service_status: "Ready".to_string(),
            operation_busy: false,
            operation_status: "Ready".to_string(),
            operation_tone: "cyan".to_string(),
            run_total: 0,
            run_completed: 0,
            run_failed: 0,
        })
    }

    pub fn replace_config(&mut self, config: Config) -> Result<(), String> {
        if self.phase != RunPhase::Idle {
            return Err("Config v5 cannot be replaced during an active run.".to_string());
        }
        let enabled = resolve_sources(&config.sources, &config.source_policies, None, None, &[])?;
        let existing = self
            .sources
            .iter()
            .map(|source| (source.name.to_ascii_lowercase(), source.clone()))
            .collect::<HashMap<_, _>>();
        self.sources = candidate_source_card_names(&config)
            .into_iter()
            .map(|name| {
                let previous = existing.get(&name.to_ascii_lowercase());
                let is_samples = name.eq_ignore_ascii_case("samples");
                let enabled = !is_samples
                    && (name.eq_ignore_ascii_case("local")
                        || enabled
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(&name)));
                SourceState {
                    enabled,
                    candidate_count: previous.map_or(0, |source| source.candidate_count),
                    status: if is_samples && !config.samples.sample_write {
                        "Excluded".to_string()
                    } else if is_samples {
                        previous
                            .filter(|source| source.candidate_count > 0)
                            .map(|source| source.status.clone())
                            .unwrap_or_else(|| "No images".to_string())
                    } else if !enabled {
                        "Excluded".to_string()
                    } else {
                        previous
                            .map(|source| source.status.clone())
                            .unwrap_or_else(|| "Ready".to_string())
                    },
                    name,
                }
            })
            .collect();
        self.launch_mode = match config.mode {
            Mode::Read => LaunchMode::Read,
            Mode::Write => LaunchMode::LiveWrite,
        };
        self.adjustments = AdjustmentState::from_config(&config);
        self.config = config;
        Ok(())
    }

    pub fn can_launch(&self) -> bool {
        self.phase == RunPhase::Idle && !self.library.queue(self.scan_scope).is_empty()
    }

    pub fn synchronize_focused_album(&mut self) -> bool {
        let Some(album_key) = self.library.focused_album_key.as_ref() else {
            return false;
        };
        let Some(album) = self
            .library
            .albums
            .iter()
            .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(album_key))
        else {
            return false;
        };
        // `current_*` is the review transaction authority while a controller
        // is active. Library focus is intentionally independent so operators
        // may inspect the library without redirecting preview/write actions.
        if self.phase != RunPhase::Idle {
            return true;
        }
        let changed = !self
            .current_album_path
            .eq_ignore_ascii_case(&album.snapshot.path);
        self.current_album_path = album.snapshot.path.clone();
        self.current_artist = album.snapshot.artist.clone();
        self.current_album = album.snapshot.title.clone();
        self.current_cover_path = album.snapshot.cover_path.clone();
        if changed {
            self.edit_preview_path.clear();
            self.compilation_track_preview_path.clear();
            self.compilation_track_preview_owner.clear();
            self.musicbrainz_cover_preview_path.clear();
            if self.phase == RunPhase::Idle {
                self.candidates.clear();
                self.selected_candidate = None;
                self.comparison_candidates.clear();
                self.hovered_candidate = None;
            }
        }
        if self.phase == RunPhase::Idle {
            self.load_focused_album_samples();
        }
        true
    }

    pub fn synchronize_focused_artist(&mut self) {
        if self.phase != RunPhase::Idle {
            return;
        }
        self.current_artist = self.library.focused_artist.clone().unwrap_or_default();
        self.current_album_path.clear();
        self.current_album.clear();
        self.current_cover_path.clear();
        self.edit_preview_path.clear();
        self.compilation_track_preview_path.clear();
        self.compilation_track_preview_owner.clear();
        self.musicbrainz_cover_preview_path.clear();
        self.candidates.clear();
        self.selected_candidate = None;
        self.comparison_candidates.clear();
        self.hovered_candidate = None;
        self.load_focused_album_samples();
    }

    pub fn set_compilation_track_preview(&mut self, track_path: &str, preview_path: &Path) {
        self.compilation_track_preview_owner = track_path.to_ascii_lowercase();
        self.compilation_track_preview_path = preview_path.to_string_lossy().into_owned();
    }

    pub fn clear_compilation_track_preview(&mut self, track_path: &str) {
        if self
            .compilation_track_preview_owner
            .eq_ignore_ascii_case(track_path)
        {
            self.compilation_track_preview_owner.clear();
            self.compilation_track_preview_path.clear();
        }
    }

    pub fn active_compilation_track_preview(&self) -> Option<&str> {
        (!self.compilation_track_preview_path.trim().is_empty()
            && (self
                .library
                .selected_compilation_track_paths
                .contains(&self.compilation_track_preview_owner)
                || (!self.current_compilation_track_path.trim().is_empty()
                    && self
                        .current_compilation_track_path
                        .eq_ignore_ascii_case(&self.compilation_track_preview_owner))))
        .then_some(self.compilation_track_preview_path.as_str())
    }

    pub fn load_focused_album_samples(&mut self) {
        self.candidates
            .retain(|candidate| !candidate.source.eq_ignore_ascii_case("samples"));
        let Some(source) = self
            .sources
            .iter_mut()
            .find(|source| source.name.eq_ignore_ascii_case("samples"))
        else {
            return;
        };
        source.enabled = false;
        source.candidate_count = 0;
        if !self.config.samples.sample_write {
            source.status = "Excluded".to_string();
            self.activate_available_candidate_filters();
            return;
        }

        let sample_artist_dir = configured_samples_dir(&self.config)
            .join(sanitize_sample_component(&self.current_artist));
        if self.current_album.trim().is_empty() {
            source.candidate_count = count_artist_sample_images(&sample_artist_dir);
            source.status = if source.candidate_count == 0 {
                "No images".to_string()
            } else {
                source.enabled = true;
                format!("{} saved", source.candidate_count)
            };
            self.activate_available_candidate_filters();
            return;
        }

        let sample_album_dir =
            sample_artist_dir.join(sanitize_sample_component(&self.current_album));
        let Ok(entries) = std::fs::read_dir(&sample_album_dir) else {
            source.status = "No images".to_string();
            self.activate_available_candidate_filters();
            return;
        };
        let range = Range {
            min: self.config.range.min,
            ideal: self.config.range.ideal,
            max: self.config.range.max,
            ladder: self.config.range.ladder,
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "jpg" | "jpeg" | "png" | "webp"
                        )
                    })
            })
            .collect::<Vec<_>>();
        paths.sort();
        for (offset, path) in paths.into_iter().enumerate() {
            let Ok(info) = inspect_image(&path) else {
                continue;
            };
            let provider = path
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(|value| value.split(".sample").next())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("saved source")
                .replace('_', " ");
            let range_class = format!("{:?}", range.classify(info.width.min(info.height)));
            self.candidates.push(CandidateState {
                index: 900_000 + offset,
                source: "samples".to_string(),
                dimensions: format!("{} × {}", info.width, info.height),
                width: info.width,
                height: info.height,
                projected_width: info.width,
                projected_height: info.height,
                source_range_class: range_class.clone(),
                range_class,
                policy_status: "accept".to_string(),
                policy_reason: format!("Saved from {provider}"),
                image_type: format!("Sample · {provider}"),
                cache_path: path.to_string_lossy().into_owned(),
                url: String::new(),
                local_origin: String::new(),
                acceptable: true,
                strict_override_active: false,
                upscaled: false,
                approved: true,
                recommended: false,
                selected: false,
            });
        }
        source.candidate_count = self
            .candidates
            .iter()
            .filter(|candidate| candidate.source.eq_ignore_ascii_case("samples"))
            .count();
        source.status = if source.candidate_count == 0 {
            "No images".to_string()
        } else {
            source.enabled = true;
            "Ready".to_string()
        };
        self.activate_available_candidate_filters();
    }

    pub fn dismiss_intro(&mut self) {
        self.intro_dismissed = true;
    }

    pub fn artist_fallback_track_progress(&self, album: &AlbumEntry) -> Option<(usize, usize)> {
        if !album.snapshot.album_mbid.trim().is_empty() {
            return None;
        }
        let album_key = album.snapshot.album_key.to_ascii_lowercase();
        if let Some(paths) = self.artist_overview_track_paths.get(&album_key)
            && !paths.is_empty()
        {
            let completed = paths
                .iter()
                .filter(|path| {
                    album.snapshot.compilation_tracks.iter().any(|track| {
                        track.path.eq_ignore_ascii_case(path) && track.embedded_artwork_recorded
                    })
                })
                .count();
            return Some((completed, paths.len()));
        }
        self.library
            .focused_artist
            .as_deref()
            .and_then(|artist| LibraryState::artist_fallback_track_progress(album, artist))
    }

    pub fn artist_overview_has_pending_work(&self) -> bool {
        if self.library.focused_artist.is_none() {
            return false;
        }
        self.library
            .artist_overview_album_indices(&self.artist_overview_album_keys, true)
            .into_iter()
            .any(|index| {
                let album = &self.library.albums[index];
                self.artist_fallback_track_progress(album)
                    .map(|(completed, total)| completed < total)
                    .unwrap_or_else(|| album.eligible_by_default())
            })
    }

    pub fn clear_completed_workspace(&mut self, cancelled: bool) {
        // Finishing an Album launched from Artist Overview should return to
        // that Artist, including compilation appearances discovered through
        // Navidrome. Only explicit top-level navigation should close the
        // Artist Overview. Album and Fallback views still return to the Media
        // Library exactly as before.
        let focused_artist = (!self.library.show_tracks
            && self.library.view == LibraryView::Artists)
            .then(|| self.library.focused_artist.clone())
            .flatten();
        let artist_overview_has_pending_albums =
            focused_artist.is_some() && self.artist_overview_has_pending_work();
        self.candidates.clear();
        self.selected_candidate = None;
        self.comparison_candidates.clear();
        self.hovered_candidate = None;
        self.clear_candidate_filters();
        self.current_album_path.clear();
        self.current_artist = focused_artist.clone().unwrap_or_default();
        self.current_album.clear();
        self.current_cover_path.clear();
        self.current_compilation_track_path.clear();
        self.edit_preview_path.clear();
        self.compilation_track_preview_path.clear();
        self.compilation_track_preview_owner.clear();
        self.musicbrainz_cover_preview_path.clear();
        self.library.focused_album_key = None;
        self.library.focused_artist = focused_artist.clone();
        self.upscale_preview_only = true;
        self.adjustments.preserve_source_resolution = false;
        self.adjustments.apply_edit_profile = true;
        if !cancelled
            && self.last_error.is_none()
            && !self.disable_confirmation_messages
            && !artist_overview_has_pending_albums
        {
            let destination = focused_artist
                .as_deref()
                .map(|artist| format!("the {artist} Artist Overview"))
                .unwrap_or_else(|| "the Media Library".to_string());
            self.completion_message = format!(
                "S.P.L.I.N.E.D. completed {} album{} and returned to {destination}.",
                self.run_completed,
                if self.run_completed == 1 { "" } else { "s" }
            );
            self.completion_dialog_visible = true;
        } else {
            self.completion_message.clear();
            self.completion_dialog_visible = false;
        }
    }

    pub fn selected_candidate(&self) -> Option<&CandidateState> {
        let index = self.selected_candidate?;
        self.candidates.iter().find(|item| item.index == index)
    }

    pub fn select_candidate(&mut self, index: usize) -> bool {
        let changed = self.selected_candidate != Some(index);
        let mut found = false;
        for candidate in &mut self.candidates {
            candidate.selected = candidate.index == index;
            found |= candidate.selected;
        }
        self.selected_candidate = found.then_some(index);
        if changed && found {
            self.edit_preview_path.clear();
            self.musicbrainz_cover_preview_path.clear();
            self.upscale_preview_only = true;
        }
        found
    }

    pub fn visible_candidates(&self) -> Vec<&CandidateState> {
        self.candidates
            .iter()
            .filter(|candidate| {
                self.candidate_source_filter.as_ref().is_none_or(|source| {
                    if source.eq_ignore_ascii_case("local") {
                        candidate.is_local()
                    } else {
                        candidate.source.eq_ignore_ascii_case(source)
                    }
                })
            })
            .filter(|candidate| {
                self.candidate_range_filter
                    .as_ref()
                    .is_none_or(|range| range_filter_matches(&candidate.range_class, range))
            })
            .filter(|candidate| self.candidate_passes_dimension(candidate, "source"))
            .filter(|candidate| self.candidate_passes_dimension(candidate, "type"))
            .filter(|candidate| self.candidate_passes_dimension(candidate, "policy"))
            .filter(|candidate| self.candidate_passes_dimension(candidate, "range"))
            .collect()
    }

    fn candidate_filter_sets(
        &self,
        dimension: &str,
    ) -> Option<(&BTreeSet<String>, &BTreeSet<String>)> {
        match dimension {
            "source" => Some((
                &self.candidate_included_sources,
                &self.candidate_excluded_sources,
            )),
            "type" => Some((
                &self.candidate_included_types,
                &self.candidate_excluded_types,
            )),
            "policy" => Some((
                &self.candidate_included_policies,
                &self.candidate_excluded_policies,
            )),
            "range" => Some((
                &self.candidate_included_ranges,
                &self.candidate_excluded_ranges,
            )),
            _ => None,
        }
    }

    fn candidate_filter_sets_mut(
        &mut self,
        dimension: &str,
    ) -> Option<(&mut BTreeSet<String>, &mut BTreeSet<String>)> {
        match dimension {
            "source" => Some((
                &mut self.candidate_included_sources,
                &mut self.candidate_excluded_sources,
            )),
            "type" => Some((
                &mut self.candidate_included_types,
                &mut self.candidate_excluded_types,
            )),
            "policy" => Some((
                &mut self.candidate_included_policies,
                &mut self.candidate_excluded_policies,
            )),
            "range" => Some((
                &mut self.candidate_included_ranges,
                &mut self.candidate_excluded_ranges,
            )),
            _ => None,
        }
    }

    fn candidate_passes_dimension(&self, candidate: &CandidateState, dimension: &str) -> bool {
        let Some((_included, excluded)) = self.candidate_filter_sets(dimension) else {
            return true;
        };
        if excluded
            .iter()
            .any(|key| candidate.matches_filter(dimension, key))
        {
            return false;
        }
        // Green means the category is visible for this run. Neutral categories
        // remain visible too; only an explicit red/disregarded category hides
        // matching candidates. This keeps a fresh run complete while allowing
        // the operator to progressively hide result classes during review.
        true
    }

    pub fn candidate_filter_disposition(
        &self,
        dimension: &str,
        key: &str,
    ) -> CandidateFilterDisposition {
        let Some((included, excluded)) = self.candidate_filter_sets(dimension) else {
            return CandidateFilterDisposition::Neutral;
        };
        if included.iter().any(|value| value.eq_ignore_ascii_case(key)) {
            CandidateFilterDisposition::Included
        } else if excluded.iter().any(|value| value.eq_ignore_ascii_case(key)) {
            CandidateFilterDisposition::Disregarded
        } else {
            CandidateFilterDisposition::Neutral
        }
    }

    pub fn cycle_candidate_filter(
        &mut self,
        dimension: &str,
        key: &str,
    ) -> Result<CandidateFilterDisposition, String> {
        let current = self.candidate_filter_disposition(dimension, key);
        let Some((included, excluded)) = self.candidate_filter_sets_mut(dimension) else {
            return Err(format!("Unknown candidate filter dimension: {dimension}"));
        };
        let remove_case_insensitive = |set: &mut BTreeSet<String>| {
            if let Some(existing) = set
                .iter()
                .find(|value| value.eq_ignore_ascii_case(key))
                .cloned()
            {
                set.remove(&existing);
            }
        };
        remove_case_insensitive(included);
        remove_case_insensitive(excluded);
        let next = match current {
            CandidateFilterDisposition::Neutral => {
                included.insert(key.to_string());
                CandidateFilterDisposition::Included
            }
            CandidateFilterDisposition::Included => {
                excluded.insert(key.to_string());
                CandidateFilterDisposition::Disregarded
            }
            CandidateFilterDisposition::Disregarded => CandidateFilterDisposition::Neutral,
        };
        Ok(next)
    }

    pub fn clear_candidate_filters(&mut self) {
        self.candidate_source_filter = None;
        self.candidate_range_filter = None;
        self.candidate_included_sources.clear();
        self.candidate_included_types.clear();
        self.candidate_included_policies.clear();
        self.candidate_included_ranges.clear();
        self.candidate_excluded_sources.clear();
        self.candidate_excluded_types.clear();
        self.candidate_excluded_policies.clear();
        self.candidate_excluded_ranges.clear();
    }

    pub fn activate_available_candidate_filters(&mut self) {
        self.clear_candidate_filters();
        for key in [
            "Recommended",
            "Upscalable",
            "Rejected",
            "Local",
            "Samples",
            "FrontCover",
            "BackCover",
            "ArtistImage",
            "MediaBooklet",
            "Other",
        ] {
            if self.candidate_filter_count("type", key) > 0 {
                self.candidate_included_types.insert(key.to_string());
            }
        }
        for key in ["Acceptable", "Strict"] {
            if self.candidate_filter_count("policy", key) > 0 {
                self.candidate_included_policies.insert(key.to_string());
            }
        }
        for key in [
            "Ideal",
            "UpperRange",
            "Ladder",
            "AboveLadder",
            "LowerRange",
            "BelowMinimum",
        ] {
            if self.candidate_filter_count("range", key) > 0 {
                self.candidate_included_ranges.insert(key.to_string());
            }
        }
    }

    pub fn candidate_filter_count(&self, dimension: &str, key: &str) -> usize {
        let candidate_count = self
            .candidates
            .iter()
            .filter(|candidate| candidate.matches_filter(dimension, key))
            .count();
        if dimension == "type" && key.eq_ignore_ascii_case("Samples") {
            return self
                .sources
                .iter()
                .find(|source| source.name.eq_ignore_ascii_case("samples"))
                .map_or(candidate_count, |source| {
                    source.candidate_count.max(candidate_count)
                });
        }
        candidate_count
    }

    pub fn toggle_candidate_comparison(&mut self, index: usize) -> bool {
        if !self
            .candidates
            .iter()
            .any(|candidate| candidate.index == index)
        {
            return false;
        }
        if !self.comparison_candidates.remove(&index) {
            self.comparison_candidates.insert(index);
        }
        true
    }

    pub fn comparison_candidates(&self) -> Vec<&CandidateState> {
        let mut candidates = self
            .candidates
            .iter()
            .filter(|candidate| self.comparison_candidates.contains(&candidate.index))
            .collect::<Vec<_>>();
        if candidates.is_empty()
            && let Some(selected) = self.selected_candidate()
        {
            candidates.push(selected);
        }
        candidates
    }

    pub fn set_candidate_hover(&mut self, index: usize, active: bool) {
        self.hovered_candidate = if self.hover_enabled && active {
            self.candidates
                .iter()
                .any(|candidate| candidate.index == index)
                .then_some(index)
        } else if self.hovered_candidate == Some(index) || !active {
            None
        } else {
            self.hovered_candidate
        };
    }

    pub fn preview_candidate(&self) -> Option<&CandidateState> {
        self.hovered_candidate
            .and_then(|index| self.candidates.iter().find(|item| item.index == index))
            .or_else(|| self.selected_candidate())
    }

    pub fn set_candidate_source_filter(&mut self, source: &str) {
        self.candidate_source_filter = if source.eq_ignore_ascii_case("all") {
            None
        } else {
            Some(source.to_string())
        };
    }

    pub fn set_candidate_range_filter(&mut self, range: &str) {
        self.candidate_range_filter = if range.eq_ignore_ascii_case("all") {
            None
        } else {
            Some(range.to_string())
        };
    }

    pub fn set_musicbrainz_artist_filter(&mut self, artist: &str) {
        self.musicbrainz_artist_filter = if artist.eq_ignore_ascii_case("all") {
            None
        } else {
            Some(artist.to_string())
        };
    }

    pub fn set_musicbrainz_release_type_filter(&mut self, release_type: &str) {
        self.musicbrainz_release_type_filter = if release_type.eq_ignore_ascii_case("all") {
            None
        } else {
            Some(release_type.to_string())
        };
    }

    pub fn visible_musicbrainz_results(&self) -> Vec<&MusicBrainzResult> {
        self.musicbrainz_results
            .iter()
            .filter(|item| {
                self.musicbrainz_artist_filter
                    .as_ref()
                    .is_none_or(|artist| item.release_artist.eq_ignore_ascii_case(artist))
            })
            .filter(|item| {
                self.musicbrainz_release_type_filter
                    .as_ref()
                    .is_none_or(|release_type| {
                        item.release_class.eq_ignore_ascii_case(release_type)
                    })
            })
            .collect()
    }

    pub fn set_source_enabled(&mut self, source: &str, enabled: bool) -> Result<(), String> {
        if source.eq_ignore_ascii_case("local") {
            return Err("Local artwork is always available for review.".to_string());
        }
        let mut configured = self
            .config
            .sources
            .cover_sources
            .iter()
            .find(|item| item.eq_ignore_ascii_case(source))
            .cloned();
        if configured.is_none() && enabled && source.eq_ignore_ascii_case("navidrome") {
            configured = Some("navidrome".to_string());
            self.config
                .sources
                .cover_sources
                .push("navidrome".to_string());
        }
        let configured = configured.ok_or_else(|| format!("Unknown artwork source: {source}"))?;
        self.config
            .sources
            .exclude_cover_sources
            .retain(|item| !item.eq_ignore_ascii_case(source));
        if !enabled {
            self.config.sources.exclude_cover_sources.push(configured);
        }
        if let Some(item) = self
            .sources
            .iter_mut()
            .find(|item| item.name.eq_ignore_ascii_case(source))
        {
            item.enabled = enabled;
            item.status = if enabled {
                "Ready".to_string()
            } else {
                "Excluded".to_string()
            };
        }
        Ok(())
    }

    pub fn move_source(&mut self, source: &str, direction: isize) -> Result<(), String> {
        if source.eq_ignore_ascii_case("local") {
            return Err("Local artwork has a fixed review position.".to_string());
        }
        let index = self
            .config
            .sources
            .cover_sources
            .iter()
            .position(|item| item.eq_ignore_ascii_case(source))
            .ok_or_else(|| format!("Unknown artwork source: {source}"))?;
        let target = index
            .saturating_add_signed(direction)
            .min(self.config.sources.cover_sources.len().saturating_sub(1));
        if index != target {
            self.config.sources.cover_sources.swap(index, target);
            let by_name = self
                .sources
                .iter()
                .map(|item| (item.name.to_ascii_lowercase(), item.clone()))
                .collect::<HashMap<_, _>>();
            self.sources = candidate_source_card_names(&self.config)
                .into_iter()
                .filter_map(|name| by_name.get(&name.to_ascii_lowercase()).cloned())
                .collect();
        }
        Ok(())
    }

    pub fn persist_adjustments_to_config(&mut self) {
        self.config.output.upscale_adaptive_defaults = self.adjustments.adaptive_defaults;
        self.config.output.upscale_picture_percent = self.adjustments.picture_percent;
        self.config.output.upscale_sharpen_percent = self.adjustments.sharpen_percent;
        self.config.output.upscale_softness_percent = self.adjustments.softness_percent;
        self.config.output.upscale_contrast_percent = self.adjustments.contrast_percent;
        self.config.output.upscale_exposure_percent = self.adjustments.exposure_percent;
        self.config.output.upscale_brightness_percent = self.adjustments.brightness_percent;
        self.config.output.upscale_gamma_percent = self.adjustments.gamma_percent;
        self.config.output.upscale_color_temperature = self.adjustments.color_temperature;
    }

    pub fn set_primary_adjustments(&mut self, requested: &[String]) {
        let catalog = [
            "Picture",
            "Sharpen",
            "Softness",
            "Contrast",
            "Exposure",
            "Brightness",
            "Gamma",
            "Color",
        ];
        let mut normalized = Vec::new();
        for value in requested {
            if let Some(canonical) = catalog
                .iter()
                .find(|candidate| candidate.eq_ignore_ascii_case(value.trim()))
                && !normalized
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(canonical))
            {
                normalized.push((*canonical).to_string());
            }
        }
        if normalized.is_empty() {
            normalized.extend(catalog.into_iter().map(str::to_string));
        }
        self.primary_adjustments = normalized;
    }

    pub fn activate_all_adjustments(&mut self) {
        self.primary_adjustments = default_primary_adjustments();
    }

    pub fn move_primary_adjustment(&mut self, name: &str, direction: isize) -> bool {
        let Some(index) = self
            .primary_adjustments
            .iter()
            .position(|value| value.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        let target = index
            .saturating_add_signed(direction)
            .min(self.primary_adjustments.len().saturating_sub(1));
        if target == index {
            return false;
        }
        self.primary_adjustments.swap(index, target);
        true
    }

    pub fn promote_primary_adjustment(&mut self, name: &str) -> bool {
        let catalog = [
            "Picture",
            "Sharpen",
            "Softness",
            "Contrast",
            "Exposure",
            "Brightness",
            "Gamma",
            "Color",
        ];
        let Some(canonical) = catalog
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        if let Some(index) = self
            .primary_adjustments
            .iter()
            .position(|value| value.eq_ignore_ascii_case(canonical))
        {
            self.primary_adjustments.remove(index);
        } else {
            self.primary_adjustments.push((*canonical).to_string());
        }
        true
    }

    pub fn candidate_decision(&self, action: &str) -> Result<Value, String> {
        match action {
            "use" | "write" => {
                let candidate = self
                    .selected_candidate()
                    .ok_or_else(|| "Select an artwork candidate before applying it.".to_string())?;
                let mut decision = self.adjustments.as_decision_fields();
                let object = decision.as_object_mut().ok_or_else(|| {
                    "Unable to create the artwork adjustment decision.".to_string()
                })?;
                object.insert("action".to_string(), json!("use"));
                object.insert("index".to_string(), json!(candidate.index));
                if action == "write" {
                    object.insert("explicit_write".to_string(), json!(true));
                }
                Ok(decision)
            }
            "keep_local" => {
                let candidate = self
                    .candidates
                    .iter()
                    .find(|candidate| candidate.is_local())
                    .ok_or_else(|| "No Local artwork candidate is available.".to_string())?;
                Ok(json!({"action": "use", "index": candidate.index}))
            }
            "bypass" => Ok(json!({"action": "bypass"})),
            "clear_bypass" => Ok(json!({"action": "clear_bypass"})),
            "skip_album" => Ok(json!({"action": "skip_album"})),
            "retry_musicbrainz"
                if self.musicbrainz_retry_available || self.musicbrainz_matches_available =>
            {
                Ok(json!({"action": "retry_musicbrainz"}))
            }
            "retry_musicbrainz" => {
                Err("MusicBrainz browsing is not available for this artwork decision.".to_string())
            }
            "back_to_musicbrainz" if self.musicbrainz_back_available => {
                Ok(json!({"action": "back_musicbrainz"}))
            }
            "back_to_musicbrainz" => {
                Err("No MusicBrainz results are available to return to.".to_string())
            }
            _ => Err(format!("Unsupported candidate action: {action}")),
        }
    }

    pub fn fallback_retry_decision(&self, artist: &str, album: &str) -> Result<Value, String> {
        if !self.fallback_active {
            return Err(
                "Fallback refinement is available only during fallback review.".to_string(),
            );
        }
        let artist = artist.trim();
        let album = album.trim();
        if artist.is_empty() || album.is_empty() {
            return Err("Fallback retry requires both Artist and Album.".to_string());
        }
        Ok(json!({"action": "retry", "artist": artist, "album": album}))
    }

    pub fn musicbrainz_result_decision(&self, index: usize) -> Result<Value, String> {
        self.musicbrainz_results
            .iter()
            .find(|item| item.index == index)
            .ok_or_else(|| "The selected MusicBrainz result is no longer available.".to_string())?;
        Ok(json!({"action": "use_musicbrainz_match", "index": index}))
    }

    pub fn select_musicbrainz_result(&mut self, index: usize) -> bool {
        let Some(item) = self
            .musicbrainz_results
            .iter()
            .find(|item| item.index == index)
        else {
            return false;
        };
        let release_title = item.release_title.clone();
        let release_artist = item.release_artist.clone();
        let current = item.current;
        self.selected_musicbrainz_result = Some(index);
        self.operation_status = if current {
            format!("Current MusicBrainz authority selected · {release_artist} — {release_title}")
        } else {
            format!(
                "Selected MusicBrainz release · {release_artist} — {release_title} · loading cover…"
            )
        };
        self.operation_tone = "magenta".to_string();
        self.push_log(format!(
            "MusicBrainz release selected: index={index} artist={release_artist} title={release_title}"
        ));
        true
    }

    pub fn selected_musicbrainz_result_decision(&mut self) -> Result<Value, String> {
        let index = self.selected_musicbrainz_result.ok_or_else(|| {
            "Select a MusicBrainz release before choosing Use Release.".to_string()
        })?;
        if let Some(item) = self
            .musicbrainz_results
            .iter_mut()
            .find(|item| item.index == index)
        {
            item.visited = true;
        }
        self.musicbrainz_result_decision(index)
    }

    pub fn musicbrainz_authority_decision(&self) -> Result<Value, String> {
        self.musicbrainz_authority_decision_with_ids(
            &self.musicbrainz_authority.recording_mbid,
            &self.musicbrainz_authority.artist_mbids,
            "",
        )
    }

    pub fn musicbrainz_authority_decision_with_ids(
        &self,
        recording_mbid: &str,
        artist_mbids: &str,
        release_mbid: &str,
    ) -> Result<Value, String> {
        let recording_mbid = recording_mbid.trim();
        let _ = release_mbid;
        let artist_mbids = artist_mbids.trim();
        let artist_parts = artist_mbids
            .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '/' | '\\' | '|'))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if !valid_mbid(recording_mbid)
            || artist_parts.is_empty()
            || !artist_parts.iter().all(|part| valid_mbid(part))
        {
            return Err(
                "Enter one canonical MusicBrainz Recording UUID and valid Artist UUIDs."
                    .to_string(),
            );
        }
        Ok(json!({
            "action": "use_musicbrainz_authority",
            "recording_mbid": recording_mbid,
            "artist_mbids": artist_mbids,
            "release_mbid": "",
        }))
    }

    pub fn musicbrainz_leave_unchanged_decision(&self) -> Result<Value, String> {
        if self.phase != RunPhase::AwaitingMusicBrainz {
            return Err("SPLINED is not waiting for a MusicBrainz decision.".to_string());
        }
        Ok(json!({"action": "leave_unchanged"}))
    }

    pub fn apply_runtime_event(&mut self, event: &Value) {
        let name = event.get("event").and_then(Value::as_str).unwrap_or("");
        match name {
            "album_started" => {
                self.phase = RunPhase::Running;
                self.operation_busy = true;
                self.operation_tone = "cyan".to_string();
                self.current_album_path = string_field(event, "album_path");
                self.current_compilation_track_path.clear();
                self.edit_preview_path.clear();
                self.musicbrainz_cover_preview_path.clear();
                if let Some(album) = self.library.albums.iter().find(|album| {
                    album
                        .snapshot
                        .path
                        .eq_ignore_ascii_case(&self.current_album_path)
                }) {
                    self.current_artist = album.snapshot.artist.clone();
                    self.current_album = album.snapshot.title.clone();
                    self.current_cover_path = album.snapshot.cover_path.clone();
                }
                self.operation_status = format!(
                    "Processing {} — {}",
                    self.current_artist, self.current_album
                );
                self.push_log(format!("Processing {}", self.current_album_path));
            }
            "release_resolved" => {
                self.musicbrainz_authority.artist = string_field(event, "artist");
                self.musicbrainz_authority.album = string_field(event, "release");
                self.musicbrainz_authority.release_mbid = string_field(event, "release_mbid");
                self.fallback_active = bool_field(event, "fallback");
                self.fallback_reason = string_field(event, "reason");
                self.fallback_artist = self.musicbrainz_authority.artist.clone();
                self.fallback_album = self.musicbrainz_authority.album.clone();
                self.operation_status =
                    "Authority resolved · searching artwork sources…".to_string();
                self.operation_tone = "violet".to_string();
                self.push_log(format!(
                    "MusicBrainz authority: {} — {}",
                    self.musicbrainz_authority.artist, self.musicbrainz_authority.album
                ));
            }
            "compilation_track_started" => {
                let track_path = string_field(event, "track_path");
                self.current_compilation_track_path = track_path.to_ascii_lowercase();
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let total = event.get("total").and_then(Value::as_u64).unwrap_or(0);
                self.operation_status = format!(
                    "Processing fallback track {index} of {total} · {}",
                    Path::new(&track_path)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or(&track_path)
                );
                self.operation_tone = "cyan".to_string();
                self.push_log(format!(
                    "Fallback track {index} of {total}: {}",
                    Path::new(&track_path)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or(&track_path)
                ));
            }
            "compilation_track_completed" => {
                let track_path = string_field(event, "track_path");
                let action = string_field(event, "action");
                let written = action.eq_ignore_ascii_case("EmbeddedReplaced")
                    || action.eq_ignore_ascii_case("EmbeddedExisting");
                if written {
                    for album in &mut self.library.albums {
                        for track in &mut album.snapshot.compilation_tracks {
                            if track.path.eq_ignore_ascii_case(&track_path) {
                                track.embedded_artwork_recorded = true;
                            }
                        }
                    }
                }
                self.push_log(format!(
                    "{} fallback track {}",
                    if written { "Saved" } else { "Reviewed" },
                    Path::new(&track_path)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or(&track_path)
                ));
            }
            "compilation_track_skipped" => self.push_log(format!(
                "Skipped fallback track {} ({})",
                Path::new(&string_field(event, "track_path"))
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("unknown track"),
                string_field(event, "reason")
            )),
            "compilation_existing_artwork" => self.push_log(format!(
                "Existing embedded artwork verified for the same recording: {}",
                string_field(event, "source_track_path")
            )),
            "candidates" => {
                self.apply_candidates(event);
                if self
                    .selected_candidate()
                    .is_some_and(|candidate| candidate.source == "existing")
                {
                    self.operation_status = "Existing embedded cover verified for this Recording and Artist · choose Existing to copy it".to_string();
                    self.operation_tone = "green".to_string();
                } else {
                    self.operation_status = format!(
                        "Evaluating {} candidate image{}…",
                        self.candidates.len(),
                        if self.candidates.len() == 1 { "" } else { "s" }
                    );
                    self.operation_tone = "violet".to_string();
                }
            }
            "decision_required" => {
                self.phase = RunPhase::AwaitingCandidate;
                // The runtime is paused on user input here. Keep the status visible, but do
                // not treat the pause as an active operation: candidate comparison, editing,
                // Upscale Preview, and the final decision must remain interactive.
                self.operation_busy = false;
                self.musicbrainz_matches_available =
                    bool_field(event, "musicbrainz_matches_available");
                let existing = self
                    .selected_candidate()
                    .is_some_and(|candidate| candidate.source == "existing");
                self.operation_status = if existing {
                    "Existing embedded cover verified · choose Existing to copy it or MusicBrainz for other art".to_string()
                } else if self.musicbrainz_matches_available {
                    "Choose artwork or open MusicBrainz results".to_string()
                } else {
                    "Waiting for an artwork decision".to_string()
                };
                self.operation_tone = if existing { "green" } else { "orange" }.to_string();
                self.musicbrainz_back_available = bool_field(event, "musicbrainz_back_available");
                self.push_log("Artwork decision required.".to_string());
            }
            "musicbrainz_matches" => self.apply_musicbrainz_matches(event),
            "musicbrainz_authority_error" => {
                let message = string_field(event, "message");
                self.last_error = Some(message.clone());
                self.musicbrainz_identity_status = "invalid".to_string();
                self.musicbrainz_identity_message = message.clone();
                self.operation_status = format!("MusicBrainz ID validation failed · {message}");
                self.operation_tone = "red".to_string();
                self.push_log(format!("MusicBrainz authority rejected: {message}"));
            }
            "album_completed" => {
                self.phase = RunPhase::Running;
                self.run_completed = self.run_completed.saturating_add(1);
                self.operation_status = format!(
                    "Completed {} of {} · preparing the next album…",
                    self.run_completed, self.run_total
                );
                self.operation_tone = "green".to_string();
                self.push_log(format!(
                    "Completed {} ({})",
                    string_field(event, "album_path"),
                    string_field(event, "action")
                ));
            }
            "album_postponed" => self.push_log(format!(
                "Postponed {} until its timeout expires.",
                string_field(event, "album_path")
            )),
            "album_skipped" => self.push_log(format!(
                "Skipped {} ({})",
                string_field(event, "album_path"),
                string_field(event, "reason")
            )),
            "album_error" | "scan_error" => {
                let message = string_field(event, "message");
                self.run_failed = self.run_failed.saturating_add(1);
                self.last_error = Some(message.clone());
                self.operation_status = format!("Processing error · {message}");
                self.operation_tone = "red".to_string();
                self.push_log(format!("ERROR: {message}"));
            }
            "activity" => {
                let message = string_field(event, "message");
                self.operation_status = message.clone();
                self.push_log(message);
            }
            "post_cover_timing" => {
                self.operation_status = "Saving library status and artwork history…".to_string();
                self.operation_tone = "magenta".to_string();
                self.push_log("Saved library status and artwork history.".to_string());
            }
            "source_results_restored" => {
                self.phase = RunPhase::AwaitingCandidate;
                self.operation_busy = false;
                self.operation_status =
                    "Artwork candidates restored · waiting for a decision".to_string();
                self.operation_tone = "orange".to_string();
                self.push_log("Returned to the cached artwork source results.".to_string());
            }
            "provider_timing" => {
                let source = string_field(event, "source");
                self.operation_status =
                    format!("Searching {} · {}", source, string_field(event, "status"));
                self.operation_tone = "violet".to_string();
                if let Some(item) = self
                    .sources
                    .iter_mut()
                    .find(|item| item.name.eq_ignore_ascii_case(&source))
                {
                    item.status = string_field(event, "status");
                }
            }
            _ if !name.is_empty() => self.push_log(name.replace('_', " ")),
            _ => {}
        }
    }

    fn apply_candidates(&mut self, event: &Value) {
        self.comparison_candidates.clear();
        self.hovered_candidate = None;
        let items = event
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.candidates = items
            .iter()
            .filter_map(|item| {
                let index = item.get("index")?.as_u64()? as usize;
                let width = item.get("width").and_then(Value::as_u64).unwrap_or(0);
                let height = item.get("height").and_then(Value::as_u64).unwrap_or(0);
                Some(CandidateState {
                    index,
                    source: string_field(item, "source"),
                    dimensions: format!("{width} × {height}"),
                    width: width as u32,
                    height: height as u32,
                    projected_width: item
                        .get("projected_width")
                        .and_then(Value::as_u64)
                        .unwrap_or(width) as u32,
                    projected_height: item
                        .get("projected_height")
                        .and_then(Value::as_u64)
                        .unwrap_or(height) as u32,
                    source_range_class: string_field(item, "source_range_class"),
                    range_class: string_field(item, "range_class"),
                    policy_status: string_field(item, "policy_status"),
                    policy_reason: string_field(item, "policy_reason"),
                    image_type: string_field(item, "image_type"),
                    cache_path: string_field(item, "cache_path"),
                    url: string_field(item, "url"),
                    local_origin: string_field(item, "local_origin"),
                    acceptable: item
                        .get("acceptable")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    strict_override_active: bool_field(item, "strict_override_active"),
                    upscaled: bool_field(item, "upscaled"),
                    approved: bool_field(item, "approved"),
                    recommended: item
                        .get("recommended")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    selected: false,
                })
            })
            .collect();
        let selected = event
            .get("recommended_index")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .or_else(|| self.candidates.first().map(|item| item.index));
        if let Some(index) = selected {
            self.select_candidate(index);
            self.comparison_candidates.insert(index);
        }
        self.fallback_active = bool_field(event, "fallback");
        self.fallback_reason = string_field(event, "fallback_reason");
        self.musicbrainz_retry_available = bool_field(event, "musicbrainz_retry_available");
        self.musicbrainz_matches_available = bool_field(event, "musicbrainz_matches_available");
        self.musicbrainz_back_available = bool_field(event, "musicbrainz_back_available");
        for source in &mut self.sources {
            source.candidate_count = if source.name.eq_ignore_ascii_case("local") {
                self.candidates
                    .iter()
                    .filter(|item| item.is_local())
                    .count()
            } else {
                self.candidates
                    .iter()
                    .filter(|item| item.source.eq_ignore_ascii_case(&source.name))
                    .count()
            };
        }
        // A running scan emits Samples as real runtime candidates so the visible
        // indexes and the decision queue share one authority. Idle browsing still
        // discovers Samples directly from disk without launching a provider run.
        if self.phase == RunPhase::Idle {
            self.load_focused_album_samples();
        }
        self.activate_available_candidate_filters();
        self.push_log(format!(
            "Found {} artwork candidates.",
            self.candidates.len()
        ));
    }

    fn apply_musicbrainz_matches(&mut self, event: &Value) {
        self.phase = RunPhase::AwaitingMusicBrainz;
        // As with artwork review, this is a user-decision pause rather than background work.
        self.operation_busy = false;
        self.selected_musicbrainz_result = None;
        self.hovered_musicbrainz_result = None;
        // A filter chosen for the previous track or authority must never hide a
        // newly delivered result set. Each lookup opens as a complete All / All
        // page; the operator can then narrow that page deliberately.
        self.musicbrainz_artist_filter = None;
        self.musicbrainz_release_type_filter = None;
        self.edit_preview_path.clear();
        self.musicbrainz_cover_preview_path.clear();
        self.musicbrainz_hover_preview_path.clear();
        self.musicbrainz_hover_preview_index = None;
        self.musicbrainz_hover_preview_loading = None;
        self.musicbrainz_preview_cache.clear();
        self.musicbrainz_preview_inflight.clear();
        self.musicbrainz_authority = MusicBrainzAuthority {
            artist: string_field(event, "album_artist"),
            album: string_field(event, "album"),
            recording_mbid: string_field(event, "authority_recording_mbid"),
            artist_mbids: string_field(event, "authority_artist_mbids"),
            release_mbid: string_field(event, "authority_release_mbid"),
            release_title: string_field(event, "authority_release_title"),
            release_artist: string_field(event, "authority_release_artist"),
        };
        self.musicbrainz_identity_status = string_field(event, "identity_status");
        if self.musicbrainz_identity_status.is_empty() {
            self.musicbrainz_identity_status = "pending".to_string();
        }
        self.musicbrainz_identity_message = string_field(event, "identity_message");
        self.musicbrainz_recording_status = string_field(event, "recording_status");
        self.musicbrainz_artist_status = string_field(event, "artist_status");
        self.musicbrainz_focused_artist_mbid = string_field(event, "focused_artist_mbid");
        self.musicbrainz_friendly_artist = string_field(event, "friendly_artist");
        self.musicbrainz_friendly_title = string_field(event, "friendly_title");
        self.musicbrainz_used_fuzzy_search = bool_field(event, "used_fuzzy_search");
        if bool_field(event, "compilation_track") {
            self.fallback_active = true;
            // The editable search identity must describe the track that produced
            // these results. Leaving values from the previously processed album
            // makes a selected compilation track appear to belong to that album.
            let track_artist = string_field(event, "artist");
            let focused_track_artist = string_field(event, "focused_artist");
            let track_title = string_field(event, "title");
            self.fallback_artist = if !focused_track_artist.trim().is_empty() {
                focused_track_artist
            } else if track_artist.trim().is_empty() {
                self.musicbrainz_authority.artist.clone()
            } else {
                track_artist
            };
            self.fallback_album = if track_title.trim().is_empty() {
                self.musicbrainz_authority.album.clone()
            } else {
                track_title
            };
        }
        self.musicbrainz_results = event
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(MusicBrainzResult {
                    index: item.get("index")?.as_u64()? as usize,
                    recording_mbid: string_field(item, "recording_mbid"),
                    recording_title: string_field(item, "recording_title"),
                    recording_artist: string_field(item, "recording_artist"),
                    score: item
                        .get("score")
                        .and_then(Value::as_u64)
                        .unwrap_or(100)
                        .min(100) as u16,
                    release_title: string_field(item, "release_title"),
                    release_artist: string_field(item, "release_artist"),
                    release_date: string_field(item, "release_date"),
                    release_class: string_field(item, "release_class"),
                    country: string_field(item, "country"),
                    resolution: string_field(item, "resolution"),
                    release_mbid: string_field(item, "release_mbid"),
                    release_group_mbid: string_field(item, "release_group_mbid"),
                    release_group_title: string_field(item, "release_group_title"),
                    url: string_field(item, "url"),
                    current: item
                        .get("current")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    visited: item
                        .get("visited")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect();
        let fuzzy_results = self.musicbrainz_used_fuzzy_search;
        self.musicbrainz_results.sort_by(|left, right| {
            (if fuzzy_results {
                right.score.cmp(&left.score)
            } else {
                std::cmp::Ordering::Equal
            })
            .then_with(|| {
                musicbrainz_release_class_rank(left).cmp(&musicbrainz_release_class_rank(right))
            })
            .then_with(|| {
                musicbrainz_release_family_title(left).cmp(&musicbrainz_release_family_title(right))
            })
            .then_with(|| {
                musicbrainz_release_family_id(left).cmp(&musicbrainz_release_family_id(right))
            })
            .then_with(|| country_rank(&left.country).cmp(&country_rank(&right.country)))
            .then_with(|| left.release_date.cmp(&right.release_date))
            .then_with(|| left.release_mbid.cmp(&right.release_mbid))
        });
        let release_family_count = self
            .musicbrainz_results
            .iter()
            .map(musicbrainz_release_family_id)
            .collect::<HashSet<_>>()
            .len();
        let notice = string_field(event, "notice");
        self.operation_status = if self.musicbrainz_identity_status == "failed" {
            format!(
                "MB Failed · {}",
                if self.musicbrainz_identity_message.trim().is_empty() {
                    "No validated MusicBrainz identity or recovery result"
                } else {
                    self.musicbrainz_identity_message.as_str()
                }
            )
        } else if self.musicbrainz_used_fuzzy_search {
            format!(
                "MusicBrainz ID mismatch · Artist-restricted title recovery ready · {release_family_count} release {}",
                if release_family_count == 1 {
                    "family"
                } else {
                    "families"
                }
            )
        } else if !notice.is_empty() {
            notice.clone()
        } else if self.musicbrainz_results.is_empty() {
            if self.musicbrainz_identity_status == "valid" {
                "MusicBrainz IDs verified · no eligible official releases; MB INFO opens the artist page".to_string()
            } else {
                "MusicBrainz Recording-ID lookup complete · no matching releases found".to_string()
            }
        } else {
            format!(
                "MusicBrainz Recording-ID results ready · {release_family_count} release {} · {} edition{}",
                if release_family_count == 1 {
                    "family"
                } else {
                    "families"
                },
                self.musicbrainz_results.len(),
                if self.musicbrainz_results.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        };
        self.operation_tone = match self.musicbrainz_identity_status.as_str() {
            "valid" => "green",
            "invalid" | "failed" => "red",
            _ => "orange",
        }
        .to_string();
        self.push_log(format!(
            "MusicBrainz Recording-ID lookup returned {release_family_count} release families and {} editions.",
            self.musicbrainz_results.len(),
        ));
        if !notice.is_empty() {
            self.push_log(notice);
        }
    }

    pub fn push_log(&mut self, message: String) {
        if message.trim().is_empty() {
            return;
        }
        self.log.push(message);
        const MAX_LOG_ROWS: usize = 400;
        if self.log.len() > MAX_LOG_ROWS {
            self.log.drain(0..self.log.len() - MAX_LOG_ROWS);
        }
    }
}

fn sanitize_sample_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.trim().chars() {
        if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        {
            sanitized.push('_');
        } else {
            sanitized.push(character);
        }
    }
    let sanitized = sanitized.trim().trim_end_matches([' ', '.']).to_string();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn count_artist_sample_images(artist_dir: &Path) -> usize {
    let Ok(albums) = fs::read_dir(artist_dir) else {
        return 0;
    };
    albums
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|album| fs::read_dir(album).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "jpg" | "jpeg" | "png" | "webp"
                        )
                    })
        })
        .count()
}

fn string_field(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn default_primary_adjustments() -> Vec<String> {
    [
        "Picture",
        "Sharpen",
        "Softness",
        "Contrast",
        "Exposure",
        "Brightness",
        "Gamma",
        "Color",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn valid_mbid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, value)| match index {
            8 | 13 | 18 | 23 => *value == b'-',
            _ => value.is_ascii_hexdigit(),
        })
}

fn range_filter_matches(value: &str, filter: &str) -> bool {
    let normalize = |value: &str| {
        let token = value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>();
        match token.as_str() {
            "upper" => "upperrange".to_string(),
            "lower" => "lowerrange".to_string(),
            "above" => "aboveladder".to_string(),
            "below" => "belowminimum".to_string(),
            _ => token,
        }
    };
    let value = normalize(value);
    let filter = normalize(filter);
    value == filter
}

fn bool_field(value: &Value, name: &str) -> bool {
    value.get(name).and_then(Value::as_bool).unwrap_or(false)
}

fn musicbrainz_release_family_title(item: &MusicBrainzResult) -> String {
    if item.release_group_title.trim().is_empty() {
        item.release_title.trim().to_ascii_lowercase()
    } else {
        item.release_group_title.trim().to_ascii_lowercase()
    }
}

fn musicbrainz_release_class_rank(item: &MusicBrainzResult) -> u8 {
    match item.release_class.trim().to_ascii_lowercase().as_str() {
        "album" => 0,
        "single" => 1,
        "ep" => 2,
        "soundtrack" => 3,
        "compilation" => 4,
        "broadcast" => 5,
        _ => 6,
    }
}

fn musicbrainz_release_family_id(item: &MusicBrainzResult) -> String {
    if item.release_group_mbid.trim().is_empty() {
        item.release_mbid.to_ascii_lowercase()
    } else {
        item.release_group_mbid.to_ascii_lowercase()
    }
}

fn country_rank(country: &str) -> u8 {
    if country.eq_ignore_ascii_case("US") {
        0
    } else if country.trim().is_empty() {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_database::MediaTrackSnapshot;
    use tempfile::TempDir;

    fn album(key: &str, artist: &str, title: &str, status: &str) -> MediaAlbumSnapshot {
        MediaAlbumSnapshot {
            album_key: key.to_string(),
            artist: artist.to_string(),
            tagged_artist: artist.to_string(),
            title: title.to_string(),
            path: format!(r"C:\Music\{artist}\{title}"),
            representative_file: format!(r"C:\Music\{artist}\{title}\01.flac"),
            status: status.to_string(),
            compilation: false,
            compilation_track_art_eligible: false,
            compilation_tracks: Vec::<MediaTrackSnapshot>::new(),
            track_count: 1,
            release_year: "2024".to_string(),
            artist_mbid: String::new(),
            album_mbid: String::new(),
            release_group_mbid: String::new(),
            has_local_artwork: false,
            local_artwork_files: Vec::new(),
            cover_path: String::new(),
            cover_name: String::new(),
            cover_format: String::new(),
            cover_width: 0,
            cover_height: 0,
            root_files: 1,
            cover_files: 0,
            processed_at: None,
            timeout_until: None,
            selected_source: None,
        }
    }

    fn library() -> LibraryState {
        LibraryState::from_snapshot(MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: "splined.db".to_string(),
            canonical_root: "/music".to_string(),
            local_root: r"C:\Music".to_string(),
            albums: vec![
                album("a", "Alpha", "Fresh", "unprocessed"),
                album("b", "Alpha", "Done", "processed"),
                album("c", "Beta", "Paused", "timeout"),
                album("d", "Gamma", "No", "bypassed"),
                album("e", "Delta", "Resume", "incomplete"),
            ],
        })
        .unwrap()
    }

    #[test]
    fn startup_uses_indexed_local_cover_without_projecting_fallback_folder_art() {
        let mut release = album("release", "Alpha", "Release", "processed");
        release.album_mbid = "release-id".to_string();
        release.cover_path = r"C:\Music\Alpha\Release\cover.png".to_string();
        let mut fallback = album("fallback", "Alpha", "Compilation", "processed");
        fallback.cover_path = r"C:\Music\Alpha\Compilation\cover.jpg".to_string();
        let state = LibraryState::from_snapshot(MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: "splined.db".to_string(),
            canonical_root: "/music".to_string(),
            local_root: r"C:\Music".to_string(),
            albums: vec![release, fallback],
        })
        .unwrap();
        assert_eq!(
            state.album_artwork_paths.get("release").map(String::as_str),
            Some(r"C:\Music\Alpha\Release\cover.png")
        );
        assert!(!state.album_artwork_paths.contains_key("fallback"));
    }

    fn application_state() -> ApplicationState {
        ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: crate::media_database::SCHEMA_VERSION,
                database_path: "splined.db".to_string(),
                canonical_root: "/music".to_string(),
                local_root: r"C:\Music".to_string(),
                albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
            },
        )
        .unwrap()
    }

    #[test]
    fn artist_aggregate_states_match_the_existing_contract() {
        let state = library();
        let artists = state.artists();
        assert_eq!(
            LibraryState::artist_state(artists["Alpha"].iter().map(|index| &state.albums[*index])),
            ArtistState::Partial
        );
        assert_eq!(
            LibraryState::artist_state(artists["Beta"].iter().map(|index| &state.albums[*index])),
            ArtistState::Complete
        );
        assert_eq!(
            LibraryState::artist_state(artists["Gamma"].iter().map(|index| &state.albums[*index])),
            ArtistState::ContainsBypass
        );
    }

    #[test]
    fn views_and_filters_use_the_resident_snapshot() {
        let mut state = library();
        assert_eq!(state.rows().len(), 4);
        state.set_view(LibraryView::Albums);
        state.set_text_filter("resu");
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.rows()[0].primary, "Resume");
        state.set_text_filter("");
        state.set_status_filter(StatusFilter::Processed);
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.rows()[0].primary, "Done");
        state.set_view(LibraryView::Folders);
        assert!(state.rows()[0].primary.contains("Done"));
    }

    #[test]
    fn artist_overview_navigation_skips_artists_without_pending_work() {
        let mut state = library();
        state.focused_artist = Some("Alpha".to_string());
        assert_eq!(state.adjacent_pending_artist(1).as_deref(), Some("Delta"));
        assert_eq!(state.adjacent_pending_artist(-1).as_deref(), Some("Delta"));
        state.focused_artist = Some("Delta".to_string());
        assert_eq!(state.adjacent_pending_artist(1).as_deref(), Some("Alpha"));
    }

    #[test]
    fn focused_fallback_album_stays_open_after_status_filter_changes() {
        let mut state = library();
        state.set_show_tracks(true);
        state.set_status_filters([StatusFilter::Unprocessed]);
        state.focused_album_key = Some("b".to_string());
        let rows = state.fallback_album_rows();
        assert!(rows.iter().any(|row| row.key == "b" && row.expanded));
    }

    #[test]
    fn refreshed_snapshot_preserves_completed_embedded_track_state_and_focus() {
        let mut state = library();
        state.focused_album_key = Some("a".to_string());
        state.albums[0].snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Alpha\Fresh\01.flac".to_string(),
            title: "One".to_string(),
            artist: String::new(),
            embedded_artwork_recorded: true,
        }];
        let mut albums = state
            .albums
            .iter()
            .map(|album| album.snapshot.clone())
            .collect::<Vec<_>>();
        albums[0].compilation_tracks[0].embedded_artwork_recorded = false;
        state
            .replace_snapshot(MediaSnapshot {
                schema_version: crate::media_database::SCHEMA_VERSION,
                database_path: "splined.db".to_string(),
                canonical_root: "/music".to_string(),
                local_root: r"C:\Music".to_string(),
                albums,
            })
            .unwrap();
        assert_eq!(state.focused_album_key.as_deref(), Some("a"));
        assert!(state.albums[0].snapshot.compilation_tracks[0].embedded_artwork_recorded);
    }

    #[test]
    fn artist_overview_adds_compilation_appearances_and_show_all_only_overrides_status() {
        let mut state = library();
        state.focused_artist = Some("Alpha".to_string());
        state.albums.iter_mut().for_each(|album| {
            album.snapshot.release_year = match album.snapshot.album_key.as_str() {
                "a" => "2020",
                "b" => "2024",
                "e" => "2023",
                _ => "2019",
            }
            .to_string();
        });
        state.set_status_filters([StatusFilter::Incomplete]);
        let appearances = ["e".to_string()].into_iter().collect();
        let keys = |indices: Vec<usize>| {
            indices
                .into_iter()
                .map(|index| state.albums[index].snapshot.album_key.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(state.artist_overview_album_indices(&appearances, false)),
            vec!["e"]
        );
        assert_eq!(
            keys(state.artist_overview_album_indices(&appearances, true)),
            vec!["b", "e", "a"]
        );
    }

    #[test]
    fn artist_overview_always_shows_matching_fallback_tracks_despite_status_filter() {
        let mut state = library();
        state.focused_artist = Some("Alpha".to_string());
        state.set_status_filters([StatusFilter::Processed]);

        let fallback = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "e")
            .unwrap();
        fallback.snapshot.artist = "Various Artists".to_string();
        fallback.snapshot.album_mbid.clear();
        fallback.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Various Artists\Resume\01.flac".to_string(),
            title: "Alpha Appearance".to_string(),
            artist: "Alpha".to_string(),
            embedded_artwork_recorded: false,
        }];

        let release_id_album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "d")
            .unwrap();
        release_id_album.snapshot.artist = "Various Artists".to_string();
        release_id_album.snapshot.album_mbid = "release-id-present".to_string();
        release_id_album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Various Artists\No\01.flac".to_string(),
            title: "Not Fallback".to_string(),
            artist: "Alpha".to_string(),
            embedded_artwork_recorded: false,
        }];

        let appearances = ["d".to_string()].into_iter().collect();
        let visible = state
            .artist_overview_album_indices(&appearances, false)
            .into_iter()
            .map(|index| state.albums[index].snapshot.album_key.as_str())
            .collect::<Vec<_>>();

        assert_eq!(visible, vec!["b", "e"]);
    }

    #[test]
    fn media_rows_project_names_status_and_artwork_without_library_paths() {
        let mut state = library();
        let artist = state
            .rows()
            .into_iter()
            .find(|row| row.primary == "Alpha")
            .unwrap();
        assert_eq!(artist.secondary, "( 2 Albums )");
        assert!(!artist.secondary.contains(r"C:\Music"));

        state.set_status_filter(StatusFilter::Processed);
        let artist = state
            .rows()
            .into_iter()
            .find(|row| row.primary == "Alpha")
            .unwrap();
        assert_eq!(artist.secondary, "( 1 Albums )");

        state.set_status_filter(StatusFilter::All);
        state.set_view(LibraryView::Albums);
        state
            .album_artwork_paths
            .insert("b".to_string(), r"C:\SPLINED\thumbs\done.img".to_string());
        let album = state
            .rows()
            .into_iter()
            .find(|row| row.primary == "Done")
            .unwrap();
        assert_eq!(album.secondary, "Alpha");
        assert_eq!(album.status, "Processed");
        assert_eq!(album.artwork_path, r"C:\SPLINED\thumbs\done.img");
        assert!(!album.secondary.contains(r"C:\Music"));
    }

    #[test]
    fn artist_rows_expand_albums_inline_without_changing_view_or_selecting_them() {
        let mut state = library();
        assert!(state.toggle_artist_scope("Alpha"));
        assert_eq!(state.view, LibraryView::Artists);
        assert_eq!(state.selected_count(), 0);

        let rows = state.rows();
        let alpha = rows
            .iter()
            .position(|row| row.kind == "artist" && row.primary == "Alpha")
            .unwrap();
        assert!(rows[alpha].expanded);
        assert_eq!(rows[alpha + 1].kind, "album-child");
        assert_eq!(rows[alpha + 1].depth, 1);
        assert_eq!(rows[alpha + 1].secondary, "");
        assert_eq!(
            rows.iter().filter(|row| row.kind == "album-child").count(),
            2
        );

        assert!(state.toggle_artist_scope("Alpha"));
        assert_eq!(state.rows().len(), 4);
    }

    #[test]
    fn expanded_artist_children_honor_album_states_not_the_artist_aggregate() {
        let mut state = library();
        state.albums.extend([
            AlbumEntry {
                snapshot: album("f", "Alpha", "Paused", "timeout"),
                state: AlbumState::TimeoutActive,
                selected: false,
                bypass_override: false,
            },
            AlbumEntry {
                snapshot: album("g", "Alpha", "No", "bypassed"),
                state: AlbumState::Bypassed,
                selected: false,
                bypass_override: false,
            },
            AlbumEntry {
                snapshot: album("h", "Alpha", "Resume", "incomplete"),
                state: AlbumState::Incomplete,
                selected: false,
                bypass_override: false,
            },
        ]);
        state.set_status_filters([
            StatusFilter::Incomplete,
            StatusFilter::Bypassed,
            StatusFilter::PartialTimeout,
            StatusFilter::ArtistContainsBypass,
        ]);
        assert!(state.toggle_artist_scope("Alpha"));

        let rows = state.rows();
        assert!(
            rows.iter()
                .any(|row| row.kind == "artist" && row.primary == "Alpha")
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.kind == "album-child" && row.primary == "Done")
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.kind == "album-child" && row.primary == "Fresh")
        );
        assert!(
            rows.iter()
                .any(|row| row.kind == "album-child" && row.primary == "Paused")
        );
        assert!(
            rows.iter()
                .any(|row| row.kind == "album-child" && row.primary == "No")
        );
        assert!(
            rows.iter()
                .any(|row| row.kind == "album-child" && row.primary == "Resume")
        );
    }

    #[test]
    fn state_counts_and_labels_follow_the_active_library_tab() {
        let mut state = library();
        assert_eq!(state.state_scope_label(), "Artists");
        assert_eq!(state.status_count(StatusFilter::All), 4);
        assert_eq!(state.status_count(StatusFilter::Unprocessed), 1);
        assert_eq!(state.status_count(StatusFilter::Processed), 1);
        assert_eq!(state.status_count(StatusFilter::ArtistComplete), 1);
        assert_eq!(
            state.status_label(StatusFilter::ArtistComplete),
            "Artist Complete"
        );

        state.set_top_level_view(LibraryView::Albums);
        assert_eq!(state.state_scope_label(), "Albums");
        assert_eq!(state.status_count(StatusFilter::All), 5);
        assert_eq!(state.status_count(StatusFilter::Unprocessed), 1);
        assert_eq!(
            state.status_label(StatusFilter::ArtistComplete),
            "Album Complete"
        );

        state.set_show_tracks(true);
        for album in &mut state.albums {
            album.snapshot.album_mbid = format!("release-{}", album.snapshot.album_key);
        }
        state.albums[0].snapshot.album_mbid.clear();
        assert_eq!(state.state_scope_label(), "Fallback");
        assert_eq!(state.status_count(StatusFilter::All), 1);
        assert_eq!(
            state.status_label(StatusFilter::ArtistComplete),
            "Fallback Complete"
        );
    }

    #[test]
    fn ordinary_album_selection_is_exclusive_but_radio_selection_stays_additive() {
        let mut state = library();
        state.set_view(LibraryView::Albums);
        assert_eq!(
            state.select_album_exclusive("a", false),
            SelectionResult::Changed
        );
        assert_eq!(state.selected_count(), 1);
        assert_eq!(
            state.select_album_exclusive("e", false),
            SelectionResult::Changed
        );
        assert_eq!(state.selected_count(), 1);
        assert!(
            !state
                .albums
                .iter()
                .find(|album| album.snapshot.album_key == "a")
                .unwrap()
                .selected
        );
        assert!(
            state
                .albums
                .iter()
                .find(|album| album.snapshot.album_key == "e")
                .unwrap()
                .selected
        );

        assert_eq!(state.toggle_album("a", false), SelectionResult::Changed);
        assert_eq!(state.selected_count(), 2);
    }

    #[test]
    fn fallback_is_album_first_and_projects_tracks_only_for_the_focused_album() {
        let mut state = library();
        for album in &mut state.albums {
            album.snapshot.album_mbid = format!("release-{}", album.snapshot.album_key);
        }
        for (key, file) in [("a", "01.flac"), ("b", "02.flac")] {
            let album = state
                .albums
                .iter_mut()
                .find(|album| album.snapshot.album_key == key)
                .unwrap();
            album.snapshot.compilation = true;
            album.snapshot.album_mbid.clear();
            album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
                path: format!(r"C:\Music\Alpha\{}\{file}", album.snapshot.title),
                title: file.to_string(),
                artist: String::new(),
                embedded_artwork_recorded: key == "b",
            }];
        }

        state.set_show_tracks(true);
        assert_eq!(state.fallback_album_rows().len(), 2);
        assert!(state.compilation_track_rows().is_empty());
        assert!(state.focus_album("a"));
        let tracks = state.compilation_track_rows();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].album_key, "a");
        assert_eq!(tracks[0].file_name, "01.flac");
        assert!(!tracks[0].file_name.contains("Alpha"));
        assert_eq!(tracks[0].status, "Incomplete");
        assert!(state.focus_album("b"));
        assert_eq!(state.compilation_track_rows()[0].status, "Processed");
    }

    #[test]
    fn artist_overview_fallback_projects_only_tracks_tagged_for_that_artist() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.album_mbid.clear();
        album.snapshot.compilation = true;
        album.snapshot.compilation_tracks = vec![
            MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Compilation\01.flac".to_string(),
                title: "Alpha Song".to_string(),
                artist: "Alpha".to_string(),
                embedded_artwork_recorded: false,
            },
            MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Compilation\02.flac".to_string(),
                title: "Other Song".to_string(),
                artist: "Other Artist".to_string(),
                embedded_artwork_recorded: false,
            },
        ];
        state.set_view(LibraryView::Artists);
        assert!(state.focus_artist("Alpha"));
        assert!(state.focus_album("a"));

        let tracks = state.compilation_track_rows();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].file_name, "01.flac");

        state.set_view(LibraryView::Folders);
        assert_eq!(state.compilation_track_rows().len(), 2);
    }

    #[test]
    fn featured_compilation_credit_completes_for_each_artist_without_duplicate_write() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.album_mbid.clear();
        album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Various Artists\Compilation\Talk Dirty.mp3".to_string(),
            title: "Talk Dirty".to_string(),
            artist: "Jason Derulo\x002 Chainz".to_string(),
            embedded_artwork_recorded: true,
        }];
        assert_eq!(
            LibraryState::artist_fallback_track_progress(album, "Jason Derulo"),
            Some((1, 1))
        );
        assert_eq!(
            LibraryState::artist_fallback_track_progress(album, "2 Chainz"),
            Some((1, 1))
        );
        assert!(!LibraryState::artist_fallback_is_featured(
            album,
            "Jason Derulo"
        ));
        assert!(LibraryState::artist_fallback_is_featured(album, "2 Chainz"));
    }

    #[test]
    fn valid_ids_remain_green_when_musicbrainz_has_no_eligible_official_release() {
        let mut state = application_state();
        state.apply_runtime_event(&json!({
            "event": "musicbrainz_matches",
            "compilation_track": true,
            "artist": "Jason Derulo\x002 Chainz",
            "focused_artist": "2 Chainz",
            "title": "Talk Dirty",
            "identity_status": "valid",
            "recording_status": "valid",
            "artist_status": "valid",
            "focused_artist_mbid": "dff0d392-4cd5-4052-9fbb-f485df3891e5",
            "authority_recording_mbid": "b944591d-88d4-4c91-81e0-0a247f26511a",
            "authority_artist_mbids": "6de0f914-3e60-4418-be3b-42e0feb6eb4d, dff0d392-4cd5-4052-9fbb-f485df3891e5",
            "items": []
        }));
        assert_eq!(state.musicbrainz_identity_status, "valid");
        assert_eq!(
            state.musicbrainz_focused_artist_mbid,
            "dff0d392-4cd5-4052-9fbb-f485df3891e5"
        );
        assert_eq!(state.fallback_artist, "2 Chainz");
        assert_eq!(state.operation_tone, "green");
        assert!(state.operation_status.contains("IDs verified"));
    }

    #[test]
    fn artist_overview_accepts_only_the_focused_artist_fallback_track() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.album_mbid.clear();
        album.snapshot.compilation = true;
        album.snapshot.compilation_tracks = vec![
            MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Compilation\01.flac".to_string(),
                title: "Alpha duet".to_string(),
                artist: "Alpha\0Guest".to_string(),
                embedded_artwork_recorded: false,
            },
            MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Compilation\02.flac".to_string(),
                title: "Wrong performer".to_string(),
                artist: "Other Artist".to_string(),
                embedded_artwork_recorded: false,
            },
        ];
        state.set_view(LibraryView::Artists);
        assert!(state.focus_artist("Alpha"));
        assert!(state.focus_album("a"));

        assert_eq!(state.compilation_track_rows().len(), 1);
        assert_eq!(
            state.toggle_compilation_track(
                "a",
                r"C:\Music\Various Artists\Compilation\02.flac",
                false,
            ),
            SelectionResult::NotFound
        );
        assert!(state.selected_compilation_track_paths.is_empty());
        assert_eq!(
            state.toggle_compilation_track(
                "a",
                r"C:\Music\Various Artists\Compilation\01.flac",
                false,
            ),
            SelectionResult::Changed
        );
    }

    #[test]
    fn fallback_rehydration_preserves_completed_track_status() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.album_mbid.clear();
        album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Various Artists\Compilation\01.flac".to_string(),
            title: String::new(),
            artist: String::new(),
            embedded_artwork_recorded: true,
        }];

        assert!(state.replace_fallback_tracks(
            "a",
            vec![MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Compilation\01.flac".to_string(),
                title: "Hydrated".to_string(),
                artist: "Alpha".to_string(),
                embedded_artwork_recorded: false,
            }],
        ));
        let hydrated = state
            .albums
            .iter()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        assert!(hydrated.snapshot.compilation_tracks[0].embedded_artwork_recorded);
        assert_eq!(hydrated.snapshot.compilation_tracks[0].artist, "Alpha");
    }

    #[test]
    fn fallback_tab_track_rows_ignore_stale_artist_overview_focus() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.album_mbid.clear();
        album.snapshot.compilation = true;
        album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Various Artists\Compilation\01.flac".to_string(),
            title: "Other Artist Song".to_string(),
            artist: "Other Artist".to_string(),
            embedded_artwork_recorded: false,
        }];
        state.set_view(LibraryView::Artists);
        assert!(state.focus_artist("Alpha"));
        state.set_show_tracks(true);
        assert!(state.focus_album("a"));

        let tracks = state.compilation_track_rows();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].file_name, "01.flac");
    }

    #[test]
    fn fallback_uses_missing_album_id_authority_and_honors_status_filters() {
        let mut state = library();
        for album in &mut state.albums {
            album.snapshot.compilation = true;
            album.snapshot.album_mbid = format!("release-{}", album.snapshot.album_key);
            album.snapshot.compilation_tracks.clear();
        }

        for key in ["a", "b", "e"] {
            state
                .albums
                .iter_mut()
                .find(|album| album.snapshot.album_key == key)
                .unwrap()
                .snapshot
                .album_mbid
                .clear();
        }

        assert_eq!(state.fallback_album_rows().len(), 3);
        state.set_status_filter(StatusFilter::Processed);
        let processed = state.fallback_album_rows();
        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].primary, "Done");
        state.set_status_filter(StatusFilter::Incomplete);
        let incomplete = state.fallback_album_rows();
        assert_eq!(incomplete.len(), 1);
        assert_eq!(incomplete[0].primary, "Resume");
    }

    #[test]
    fn fallback_album_focus_and_selected_scan_queue_are_independent() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.compilation = true;
        album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Alpha\Fresh\01.flac".to_string(),
            title: "One".to_string(),
            artist: "Alpha".to_string(),
            embedded_artwork_recorded: false,
        }];
        state.set_show_tracks(true);

        assert!(state.toggle_fallback_album_focus("a"));
        assert_eq!(state.compilation_track_rows().len(), 1);
        assert!(state.queue(ScanScope::Selected).is_empty());

        assert_eq!(state.toggle_album("a", false), SelectionResult::Changed);
        let queue = state.queue(ScanScope::Selected);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].album_key, "a");
        assert!(queue[0].compilation_track_path.is_none());

        assert!(state.toggle_fallback_album_focus("a"));
        assert!(state.compilation_track_rows().is_empty());
        assert_eq!(state.queue(ScanScope::Selected).len(), 1);
    }

    #[test]
    fn saved_fallback_track_turns_green_immediately_but_read_only_does_not() {
        let mut snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: r"C:\Music".to_string(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        snapshot.albums[0].compilation = true;
        snapshot.albums[0].compilation_track_art_eligible = true;
        snapshot.albums[0].compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Alpha\Fresh\01.flac".to_string(),
            title: "One".to_string(),
            artist: String::new(),
            embedded_artwork_recorded: false,
        }];
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.library.set_show_tracks(true);
        assert!(state.library.focus_album("a"));

        state.apply_runtime_event(&json!({
            "event": "compilation_track_completed",
            "track_path": r"C:\Music\Alpha\Fresh\01.flac",
            "action": "ReadOnly"
        }));
        assert!(!state.library.compilation_track_rows()[0].embedded_artwork_recorded);

        state.apply_runtime_event(&json!({
            "event": "compilation_track_completed",
            "track_path": r"C:\Music\Alpha\Fresh\01.flac",
            "action": "EmbeddedReplaced"
        }));
        assert!(state.library.compilation_track_rows()[0].embedded_artwork_recorded);

        state.library.albums[0]
            .snapshot
            .compilation_tracks
            .push(MediaTrackSnapshot {
                path: r"C:\Music\Alpha\Fresh\02.flac".to_string(),
                title: "Two".to_string(),
                artist: String::new(),
                embedded_artwork_recorded: false,
            });
        state.apply_runtime_event(&json!({
            "event": "compilation_track_completed",
            "track_path": r"C:\Music\Alpha\Fresh\02.flac",
            "action": "EmbeddedExisting"
        }));
        assert!(
            state.library.compilation_track_rows().iter().any(|track| {
                track.path.ends_with("02.flac") && track.embedded_artwork_recorded
            })
        );
    }

    #[test]
    fn existing_embedded_candidate_is_a_green_explicit_write_choice() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.apply_runtime_event(&json!({
            "event": "candidates", "recommended_index": 1,
            "items": [{"index": 1, "source": "existing", "cache_path": "C:/cache/match.jpg",
                "local_origin": "existing-track", "width": 1000, "height": 1000}]
        }));
        state.apply_runtime_event(
            &json!({"event": "decision_required", "musicbrainz_back_available": true}),
        );
        assert_eq!(state.operation_tone, "green");
        assert!(state.operation_status.contains("Existing embedded cover"));
        assert_eq!(
            state.candidate_decision("write").unwrap()["explicit_write"],
            true
        );
        assert!(state.musicbrainz_back_available);
    }

    #[test]
    fn fallback_status_only_selection_targets_visible_missing_id_albums() {
        let mut state = library();
        for album in &mut state.albums {
            album.snapshot.album_mbid = format!("release-{}", album.snapshot.album_key);
        }
        state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "b")
            .unwrap()
            .snapshot
            .compilation = true;
        state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap()
            .snapshot
            .album_mbid
            .clear();
        state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "e")
            .unwrap()
            .snapshot
            .compilation = true;
        state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "e")
            .unwrap()
            .snapshot
            .album_mbid
            .clear();
        state.set_show_tracks(true);
        state.set_status_filters([StatusFilter::Unprocessed]);

        assert_eq!(state.select_filtered_for_review().unwrap(), 1);
        let queue = state.queue(ScanScope::Selected);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].album_key, "a");

        state.select_none();
        state.set_status_filters([StatusFilter::Unprocessed, StatusFilter::Incomplete]);
        assert_eq!(state.select_all_for_review(), 2);
        assert_eq!(state.queue(ScanScope::Selected).len(), 2);
    }

    #[test]
    fn top_level_folder_navigation_clears_artist_drill_down() {
        let mut state = library();
        assert_eq!(
            state.toggle_artist("Alpha", false),
            SelectionResult::Changed
        );
        state.set_view(LibraryView::Folders);
        assert_eq!(state.rows().len(), 2);

        state.set_top_level_view(LibraryView::Folders);
        assert_eq!(state.rows().len(), 5);
    }

    #[test]
    fn top_level_library_tabs_restore_each_complete_model() {
        let mut state = library();

        state.set_top_level_view(LibraryView::Artists);
        assert_eq!(state.rows().len(), 4);
        state.set_top_level_view(LibraryView::Albums);
        assert_eq!(state.rows().len(), 5);
        state.set_top_level_view(LibraryView::Folders);
        assert_eq!(state.rows().len(), 5);
        state.set_top_level_view(LibraryView::Artists);
        assert_eq!(state.rows().len(), 4);
    }

    #[test]
    fn focused_album_projects_local_context_without_starting_a_scan() {
        let mut state = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: crate::media_database::SCHEMA_VERSION,
                database_path: "splined.db".to_string(),
                canonical_root: "/music".to_string(),
                local_root: r"C:\Music".to_string(),
                albums: vec![{
                    let mut value = album("a", "Beyoncé", "Renaissance", "processed");
                    value.cover_path = r"C:\Music\Beyoncé\Renaissance\cover.jpg".to_string();
                    value
                }],
            },
        )
        .unwrap();
        state.library.set_view(LibraryView::Albums);
        assert_eq!(
            state.library.toggle_album("a", false),
            SelectionResult::Changed
        );
        assert!(state.synchronize_focused_album());
        assert_eq!(state.current_artist, "Beyoncé");
        assert_eq!(state.current_album, "Renaissance");
        assert!(state.current_cover_path.ends_with("cover.jpg"));
        assert_eq!(state.phase, RunPhase::Idle);
    }

    #[test]
    fn fallback_album_start_preserves_the_selected_tracks_embedded_preview() {
        let mut state = application_state();
        let track = r"C:\Music\Alpha\Fresh\01.flac";
        state
            .library
            .selected_compilation_track_paths
            .insert(track.to_ascii_lowercase());
        state.set_compilation_track_preview(track, Path::new(r"C:\Cache\embedded.png"));
        state.edit_preview_path = r"C:\Cache\rendered.png".to_string();

        state.apply_runtime_event(&json!({
            "event": "album_started",
            "album_path": r"C:\Music\Alpha\Fresh"
        }));

        assert!(state.edit_preview_path.is_empty());
        assert_eq!(
            state.active_compilation_track_preview(),
            Some(r"C:\Cache\embedded.png")
        );
    }

    #[test]
    fn active_fallback_transaction_survives_unrelated_library_browsing() {
        let mut state = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: crate::media_database::SCHEMA_VERSION,
                database_path: "splined.db".to_string(),
                canonical_root: "/music".to_string(),
                local_root: r"C:\Music".to_string(),
                albums: vec![
                    album("a", "10,000 Maniacs", "Compilation", "unprocessed"),
                    album("b", "10cc", "Sheet Music", "unprocessed"),
                ],
            },
        )
        .unwrap();
        let track = r"C:\Music\Various Artists\Compilation\01.flac";
        state.current_artist = "10,000 Maniacs".to_string();
        state.current_album = "Compilation".to_string();
        state.current_album_path = r"C:\Music\Various Artists\Compilation".to_string();
        state.current_cover_path = r"C:\Cache\candidate.jpg".to_string();
        state.current_compilation_track_path = track.to_ascii_lowercase();
        state.set_compilation_track_preview(track, Path::new(r"C:\Cache\embedded.png"));
        state.phase = RunPhase::AwaitingCandidate;

        assert_eq!(
            state.library.select_artist_exclusive("10cc", false),
            SelectionResult::Changed
        );
        state.synchronize_focused_artist();

        assert_eq!(state.current_artist, "10,000 Maniacs");
        assert_eq!(state.current_album, "Compilation");
        assert_eq!(
            state.current_album_path,
            r"C:\Music\Various Artists\Compilation"
        );
        assert_eq!(state.current_cover_path, r"C:\Cache\candidate.jpg");
        assert_eq!(
            state.active_compilation_track_preview(),
            Some(r"C:\Cache\embedded.png")
        );
    }

    #[test]
    fn successful_completion_returns_to_the_active_artist_overview() {
        let mut state = application_state();
        state.library.focused_artist = Some("Alpha".to_string());
        state.library.focused_album_key = Some("a".to_string());
        state.current_artist = "Alpha".to_string();
        state.current_album = "Fresh".to_string();
        state.current_album_path = r"C:\Music\Alpha\Fresh".to_string();
        state.run_completed = 1;

        state.clear_completed_workspace(false);

        assert_eq!(state.library.focused_artist.as_deref(), Some("Alpha"));
        assert!(state.library.focused_album_key.is_none());
        assert_eq!(state.current_artist, "Alpha");
        assert!(state.current_album.is_empty());
        assert!(state.current_album_path.is_empty());
        assert!(!state.completion_dialog_visible);
        assert!(state.completion_message.is_empty());
    }

    #[test]
    fn artist_overview_completion_dialog_waits_for_the_last_actionable_album() {
        let mut state = application_state();
        state.library.focused_artist = Some("Alpha".to_string());
        state.library.albums[0].state = AlbumState::Processed;
        state.run_completed = 1;

        state.clear_completed_workspace(false);

        assert_eq!(state.library.focused_artist.as_deref(), Some("Alpha"));
        assert!(state.completion_dialog_visible);
        assert!(
            state
                .completion_message
                .contains("the Alpha Artist Overview")
        );
    }

    #[test]
    fn compilation_completion_waits_for_every_matching_artist_track_only() {
        let mut state = application_state();
        state.library.focused_artist = Some("Alpha".to_string());
        state.library.albums[0].state = AlbumState::Processed;
        let mut compilation = AlbumEntry::new(album(
            "comp",
            "Various Artists",
            "Jaar Radio Top 2000",
            "processed",
        ));
        compilation.snapshot.compilation_track_art_eligible = true;
        compilation.snapshot.selected_source = Some("embedded-compilation".to_string());
        compilation.snapshot.compilation_tracks =
            [("Wallstreet Shuffle", true), ("Another Performer", false)]
                .into_iter()
                .map(|(title, completed)| MediaTrackSnapshot {
                    path: format!(r"C:\Music\Various Artists\Jaar Radio Top 2000\{title}.mp3"),
                    title: String::new(),
                    artist: String::new(),
                    embedded_artwork_recorded: completed,
                })
                .collect();
        state.library.albums.push(compilation);
        state.artist_overview_album_keys.insert("comp".to_string());
        state.artist_overview_track_paths.insert(
            "comp".to_string(),
            ["Wallstreet Shuffle", "Dreadlock Holiday", "Donna"]
                .into_iter()
                .map(|title| format!(r"C:\Music\Various Artists\Jaar Radio Top 2000\{title}.mp3"))
                .collect(),
        );
        state.run_completed = 1;

        assert_eq!(
            state.artist_fallback_track_progress(&state.library.albums[1]),
            Some((1, 3))
        );
        assert!(state.artist_overview_has_pending_work());
        state.clear_completed_workspace(false);
        assert!(!state.completion_dialog_visible);

        state.library.albums[1]
            .snapshot
            .compilation_tracks
            .push(MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Jaar Radio Top 2000\Dreadlock Holiday.mp3"
                    .to_string(),
                title: String::new(),
                artist: String::new(),
                embedded_artwork_recorded: true,
            });
        state.clear_completed_workspace(false);
        assert!(!state.completion_dialog_visible);

        state.library.albums[1]
            .snapshot
            .compilation_tracks
            .push(MediaTrackSnapshot {
                path: r"C:\Music\Various Artists\Jaar Radio Top 2000\Donna.mp3".to_string(),
                title: String::new(),
                artist: String::new(),
                embedded_artwork_recorded: true,
            });
        assert_eq!(
            state.artist_fallback_track_progress(&state.library.albums[1]),
            Some((3, 3))
        );
        assert!(!state.artist_overview_has_pending_work());
        state.clear_completed_workspace(false);
        assert!(state.completion_dialog_visible);
    }

    #[test]
    fn successful_completion_from_album_view_returns_to_the_media_library() {
        let mut state = application_state();
        state.library.set_top_level_view(LibraryView::Albums);
        assert!(state.library.focus_album("a"));
        state.current_artist = "Alpha".to_string();
        state.current_album = "Fresh".to_string();
        state.run_completed = 1;

        state.clear_completed_workspace(false);

        assert!(state.library.focused_artist.is_none());
        assert!(state.library.focused_album_key.is_none());
        assert!(state.current_artist.is_empty());
        assert!(state.current_album.is_empty());
        assert!(state.completion_message.contains("the Media Library"));
    }

    #[test]
    fn direct_additive_and_bulk_selection_preserve_protections() {
        let mut state = library();
        state.set_view(LibraryView::Albums);
        assert_eq!(state.toggle_album("a", false), SelectionResult::Changed);
        assert_eq!(state.toggle_album("b", false), SelectionResult::Changed);
        assert_eq!(state.selected_count(), 2);
        assert_eq!(
            state.toggle_album("c", false),
            SelectionResult::BlockedByTimeout("Paused".to_string())
        );
        assert_eq!(
            state.toggle_album("d", false),
            SelectionResult::NeedsBypassOverride("No".to_string())
        );
        assert_eq!(state.toggle_album("d", true), SelectionResult::Changed);
        assert!(
            state
                .albums
                .iter()
                .find(|item| item.snapshot.album_key == "d")
                .unwrap()
                .bypass_override
        );
        state.select_none();
        assert_eq!(state.selected_count(), 0);
        state.select_all();
        let selected = state
            .albums
            .iter()
            .filter(|item| item.selected)
            .map(|item| item.snapshot.album_key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(selected, vec!["a", "e"]);
    }

    #[test]
    fn artist_cascade_selects_only_default_eligible_albums() {
        let mut state = library();
        assert_eq!(
            state.toggle_artist("Alpha", false),
            SelectionResult::Changed
        );
        assert!(
            state
                .albums
                .iter()
                .find(|item| item.snapshot.album_key == "a")
                .unwrap()
                .selected
        );
        assert!(
            !state
                .albums
                .iter()
                .find(|item| item.snapshot.album_key == "b")
                .unwrap()
                .selected
        );
    }

    #[test]
    fn artist_row_activation_selects_processed_albums_for_manual_reprocessing() {
        let mut state = library();

        assert_eq!(
            state.select_artist_exclusive("Alpha", false),
            SelectionResult::Changed
        );

        let mut selected = state
            .albums
            .iter()
            .filter(|album| album.selected)
            .map(|album| album.snapshot.album_key.as_str())
            .collect::<Vec<_>>();
        selected.sort_unstable();
        assert_eq!(selected, vec!["a", "b"]);
        assert_eq!(state.focused_artist.as_deref(), Some("Alpha"));
        assert!(state.focused_album_key.is_none());
    }

    #[test]
    fn artist_overview_compilation_track_replaces_selected_artist_albums() {
        let mut state = library();
        let track_path = r"C:\Music\Various Artists\Billboard 1975\42 - Alpha.mp3";
        let fallback = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "e")
            .unwrap();
        fallback.snapshot.artist = "Various Artists".to_string();
        fallback.snapshot.compilation = true;
        fallback.snapshot.compilation_track_art_eligible = true;
        fallback.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: track_path.to_string(),
            title: "Fallback Hit".to_string(),
            artist: "Alpha".to_string(),
            embedded_artwork_recorded: false,
        }];
        state.set_view(LibraryView::Artists);

        assert_eq!(
            state.select_artist_exclusive("Alpha", false),
            SelectionResult::Changed
        );
        assert_eq!(state.queue(ScanScope::Selected).len(), 2);
        assert_eq!(
            state.toggle_compilation_track("e", track_path, false),
            SelectionResult::Changed
        );

        let queue = state.queue(ScanScope::Selected);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].album_key, "e");
        assert_eq!(queue[0].compilation_track_path.as_deref(), Some(track_path));
        assert!(
            state
                .albums
                .iter()
                .filter(|album| album.snapshot.artist == "Alpha")
                .all(|album| !album.selected)
        );
    }

    #[test]
    fn scan_queues_honor_scope_manual_reprocessing_and_bypass_override() {
        let mut state = library();
        state.toggle_album("b", false);
        assert_eq!(state.queue(ScanScope::Selected).len(), 1);
        let all = state.queue(ScanScope::All);
        assert_eq!(
            all.iter()
                .map(|item| item.album_key.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a", "e"]
        );
        state.toggle_album("d", true);
        assert!(
            state
                .queue(ScanScope::Selected)
                .iter()
                .any(|item| item.bypass_override)
        );
    }

    #[test]
    fn successful_target_completion_advances_without_clearing_later_albums() {
        let mut state = library();
        state.toggle_album("a", false);
        state.toggle_album("e", false);
        let queue = state.queue(ScanScope::Selected);
        assert_eq!(queue.len(), 2);
        state.mark_target_completed(&queue[0]);
        let remaining = state.queue(ScanScope::Selected);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].album_key, queue[1].album_key);
    }

    #[test]
    fn explicit_compilation_tracks_reopen_individually_and_clear_after_completion() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "b")
            .unwrap();
        album.snapshot.compilation = true;
        album.snapshot.compilation_track_art_eligible = true;
        album.snapshot.compilation_tracks = vec![
            MediaTrackSnapshot {
                path: r"C:\Music\Alpha\Done\01.flac".to_string(),
                title: "One".to_string(),
                artist: String::new(),
                embedded_artwork_recorded: true,
            },
            MediaTrackSnapshot {
                path: r"C:\Music\Alpha\Done\02.flac".to_string(),
                title: "Two".to_string(),
                artist: String::new(),
                embedded_artwork_recorded: false,
            },
        ];
        assert_eq!(state.toggle_album("b", false), SelectionResult::Changed);
        assert_eq!(state.compilation_track_rows().len(), 2);
        assert_eq!(
            state.toggle_compilation_track("b", r"C:\Music\Alpha\Done\01.flac", false,),
            SelectionResult::Changed
        );
        let queue = state.queue(ScanScope::Selected);
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue[0].compilation_track_path.as_deref(),
            Some(r"C:\Music\Alpha\Done\01.flac")
        );
        state.mark_target_completed(&queue[0]);
        assert!(state.queue(ScanScope::Selected).is_empty());
        assert!(!state.compilation_track_rows()[0].selected);
    }

    #[test]
    fn selecting_compilation_album_replaces_explicit_track_target() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.compilation = true;
        album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Alpha\Fresh\01.flac".to_string(),
            title: "One".to_string(),
            artist: String::new(),
            embedded_artwork_recorded: false,
        }];

        assert_eq!(
            state.toggle_compilation_track("a", r"C:\Music\Alpha\Fresh\01.flac", false,),
            SelectionResult::Changed
        );
        assert!(
            state.queue(ScanScope::Selected)[0]
                .compilation_track_path
                .is_some()
        );

        assert_eq!(state.toggle_album("a", false), SelectionResult::Changed);
        let queue = state.queue(ScanScope::Selected);
        assert_eq!(queue.len(), 1);
        assert!(queue[0].compilation_track_path.is_none());
        assert!(state.selected_compilation_track_paths.is_empty());
    }

    #[test]
    fn deselecting_an_artist_clears_its_explicit_compilation_tracks() {
        let mut state = library();
        let album = state
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key == "a")
            .unwrap();
        album.snapshot.compilation = true;
        album.snapshot.compilation_track_art_eligible = true;
        album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: r"C:\Music\Alpha\Fresh\01.flac".to_string(),
            title: "One".to_string(),
            artist: "Alpha".to_string(),
            embedded_artwork_recorded: false,
        }];
        assert_eq!(
            state.toggle_artist("Alpha", false),
            SelectionResult::Changed
        );
        assert_eq!(
            state.toggle_compilation_track("a", r"C:\Music\Alpha\Fresh\01.flac", false,),
            SelectionResult::Changed
        );
        assert_eq!(state.selected_compilation_track_paths.len(), 1);
        assert_eq!(
            state.toggle_artist("Alpha", false),
            SelectionResult::Changed
        );
        assert!(state.selected_compilation_track_paths.is_empty());
    }

    #[test]
    fn candidate_and_musicbrainz_events_are_rust_owned() {
        let mut config = Config::default();
        config.sources.cover_sources = vec!["local".to_string(), "musicbrainz".to_string()];
        config.sources.cover_sources.remove(0);
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(config, snapshot).unwrap();
        state.apply_runtime_event(&json!({
            "event":"candidates",
            "recommended_index":2,
            "items":[
                {"index":1,"source":"local","width":600,"height":600,"range_class":"lower","policy_status":"accept","cache_path":"a.jpg","url":"","recommended":false},
                {"index":2,"source":"musicbrainz","width":1800,"height":1800,"range_class":"ideal","policy_status":"accept","cache_path":"b.jpg","url":"https://example.invalid","recommended":true}
            ]
        }));
        assert_eq!(state.selected_candidate, Some(2));
        assert_eq!(state.candidate_decision("use").unwrap()["index"], 2);
        let explicit_write = state.candidate_decision("write").unwrap();
        assert_eq!(explicit_write["action"], "use");
        assert_eq!(explicit_write["index"], 2);
        assert_eq!(explicit_write["explicit_write"], true);
        state.apply_runtime_event(&json!({
            "event":"musicbrainz_matches",
            "album_artist":"Alpha",
            "album":"Fresh",
            "authority_recording_mbid":"11111111-1111-1111-1111-111111111111",
            "authority_artist_mbids":"22222222-2222-2222-2222-222222222222",
            "authority_release_mbid":"33333333-3333-3333-3333-333333333333",
            "items":[{"index":1,"release_title":"Fresh","release_artist":"Alpha","release_date":"2024","release_class":"album","release_mbid":"33333333-3333-3333-3333-333333333333","url":"https://musicbrainz.org/release/33333333-3333-3333-3333-333333333333","current":true,"visited":false}]
        }));
        assert_eq!(state.phase, RunPhase::AwaitingMusicBrainz);
        assert_eq!(state.musicbrainz_results.len(), 1);
        assert_eq!(
            state.operation_status,
            "MusicBrainz Recording-ID results ready · 1 release family · 1 edition"
        );
        assert_eq!(
            state.musicbrainz_authority_decision().unwrap()["action"],
            "use_musicbrainz_authority"
        );
    }

    #[test]
    fn compilation_musicbrainz_event_replaces_stale_album_search_identity() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album(
                "a",
                "10,000 Maniacs",
                "Love Among the Ruins",
                "processed",
            )],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.fallback_artist = "10,000 Maniacs".to_string();
        state.fallback_album = "Love Among the Ruins".to_string();

        state.apply_runtime_event(&json!({
            "event":"musicbrainz_matches",
            "compilation_track":true,
            "artist":"10,000 Maniacs",
            "title":"Because the Night",
            "album_artist":"Various Artists",
            "album":"Billboard Hot 100 Singles of 1994",
            "items":[]
        }));

        assert_eq!(state.fallback_artist, "10,000 Maniacs");
        assert_eq!(state.fallback_album, "Because the Night");
        assert!(state.fallback_active);
        assert_eq!(state.musicbrainz_authority.artist, "Various Artists");
        assert_eq!(
            state.musicbrainz_authority.album,
            "Billboard Hot 100 Singles of 1994"
        );
    }

    #[test]
    fn decision_pauses_keep_preview_and_review_controls_interactive() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.phase = RunPhase::Running;
        state.operation_busy = true;

        state.apply_runtime_event(&json!({"event":"decision_required"}));
        assert_eq!(state.phase, RunPhase::AwaitingCandidate);
        assert!(!state.operation_busy);
        assert_eq!(state.operation_status, "Waiting for an artwork decision");

        state.operation_busy = true;
        state.apply_runtime_event(&json!({
            "event":"musicbrainz_matches",
            "album_artist":"Alpha",
            "album":"Fresh",
            "items":[]
        }));
        assert_eq!(state.phase, RunPhase::AwaitingMusicBrainz);
        assert!(!state.operation_busy);
        assert_eq!(
            state.operation_status,
            "MusicBrainz Recording-ID lookup complete · no matching releases found"
        );

        state.operation_busy = true;
        state.apply_runtime_event(&json!({"event":"source_results_restored"}));
        assert_eq!(state.phase, RunPhase::AwaitingCandidate);
        assert!(!state.operation_busy);
        assert_eq!(
            state.operation_status,
            "Artwork candidates restored · waiting for a decision"
        );
    }

    #[test]
    fn musicbrainz_candidate_action_follows_the_available_browser_direction() {
        let mut state = application_state();
        assert!(state.candidate_decision("retry_musicbrainz").is_err());
        assert!(state.candidate_decision("back_to_musicbrainz").is_err());

        state.apply_runtime_event(&json!({
            "event": "candidates",
            "items": [],
            "musicbrainz_matches_available": true
        }));
        state.apply_runtime_event(&json!({
            "event": "decision_required",
            "reason": "review",
            "musicbrainz_matches_available": true
        }));
        assert_eq!(state.phase, RunPhase::AwaitingCandidate);
        assert!(state.musicbrainz_matches_available);
        assert_eq!(
            state.candidate_decision("retry_musicbrainz").unwrap()["action"],
            "retry_musicbrainz"
        );

        state.musicbrainz_matches_available = false;
        state.musicbrainz_retry_available = true;
        assert_eq!(
            state.candidate_decision("retry_musicbrainz").unwrap()["action"],
            "retry_musicbrainz"
        );

        state.musicbrainz_retry_available = false;
        state.musicbrainz_back_available = true;
        assert_eq!(
            state.candidate_decision("back_to_musicbrainz").unwrap()["action"],
            "back_musicbrainz"
        );
    }

    #[test]
    fn musicbrainz_release_without_art_returns_a_visible_warning() {
        let mut state = application_state();
        state.apply_runtime_event(&json!({
            "event":"musicbrainz_matches",
            "album_artist":"Various Artists",
            "album":"Rock Star: Music From the Motion Picture",
            "notice":"No usable artwork is available for Rock Star · choose another MusicBrainz release",
            "items":[{
                "index":1,
                "release_title":"Rock Star",
                "release_artist":"Various Artists",
                "release_mbid":"33333333-3333-3333-3333-333333333333",
                "visited":true
            }]
        }));

        assert_eq!(state.phase, RunPhase::AwaitingMusicBrainz);
        assert_eq!(
            state.operation_status,
            "No usable artwork is available for Rock Star · choose another MusicBrainz release"
        );
        assert!(state.musicbrainz_results[0].visited);
        assert!(
            state
                .log
                .iter()
                .any(|line| line.contains("No usable artwork"))
        );
    }

    #[test]
    fn dynamic_sources_and_candidate_filters_use_config_v5_authority() {
        let mut config = Config::default();
        config.sources.cover_sources = vec!["itunes".to_string(), "discogs".to_string()];
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(config, snapshot).unwrap();
        assert_eq!(
            state
                .sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            vec!["samples", "local", "itunes", "discogs", "navidrome"]
        );
        let navidrome = state
            .sources
            .iter()
            .find(|source| source.name == "navidrome")
            .unwrap();
        assert!(!navidrome.enabled);
        assert_eq!(navidrome.status, "Excluded");
        state.move_source("discogs", -1).unwrap();
        assert_eq!(
            state.config.sources.cover_sources,
            vec!["discogs", "itunes"]
        );
        state.set_source_enabled("itunes", false).unwrap();
        assert_eq!(state.config.sources.exclude_cover_sources, vec!["itunes"]);
        state.apply_runtime_event(&json!({
            "event":"candidates",
            "items":[
                {"index":1,"source":"itunes","width":1800,"height":1800,"range_class":"ideal","policy_status":"accept","cache_path":"a.jpg"},
                {"index":2,"source":"discogs","width":1200,"height":1200,"range_class":"lower","policy_status":"accept","cache_path":"b.jpg"}
            ]
        }));
        state.set_candidate_source_filter("discogs");
        state.set_candidate_range_filter("lower");
        assert_eq!(state.visible_candidates().len(), 1);
        assert_eq!(state.visible_candidates()[0].index, 2);
        state.set_source_enabled("navidrome", true).unwrap();
        assert_eq!(
            state.config.sources.cover_sources,
            vec!["discogs", "itunes", "navidrome"]
        );
        assert!(
            state
                .sources
                .iter()
                .find(|source| source.name == "navidrome")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn runtime_samples_and_webp_stills_share_selectable_source_authority() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.phase = RunPhase::Running;
        state.apply_runtime_event(&json!({
            "event":"candidates",
            "recommended_index":1,
            "items":[
                {"index":1,"source":"webpstill","width":1800,"height":1800,"source_range_class":"Ideal","range_class":"Ideal","policy_status":"accept","acceptable":true,"image_type":"FrontCover","local_origin":"cover-file","cache_path":"cover.jpg"},
                {"index":2,"source":"samples","width":1800,"height":1800,"source_range_class":"Ideal","range_class":"Ideal","policy_status":"accept","acceptable":true,"image_type":"Sample · itunes","cache_path":"sample.jpg"}
            ]
        }));

        let local = state
            .sources
            .iter()
            .find(|source| source.name.eq_ignore_ascii_case("local"))
            .unwrap();
        let samples = state
            .sources
            .iter()
            .find(|source| source.name.eq_ignore_ascii_case("samples"))
            .unwrap();
        assert_eq!(local.candidate_count, 1);
        assert_eq!(samples.candidate_count, 1);
        state.set_candidate_source_filter("local");
        assert_eq!(state.visible_candidates()[0].source, "webpstill");
        state.upscale_preview_only = false;
        assert!(state.select_candidate(2));
        assert!(state.upscale_preview_only);
        assert_eq!(state.candidate_decision("use").unwrap()["index"], 2);
    }

    #[test]
    fn config_mode_and_replacement_refresh_rust_owned_runtime_projection() {
        let mut config = Config {
            mode: Mode::Write,
            ..Config::default()
        };
        config.sources.cover_sources = vec!["itunes".to_string()];
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(config, snapshot).unwrap();
        assert_eq!(state.launch_mode, LaunchMode::LiveWrite);
        assert_eq!(
            state
                .sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            vec!["samples", "local", "itunes", "navidrome"]
        );

        let mut replacement = state.config.clone();
        replacement.mode = Mode::Read;
        replacement.sources.cover_sources = vec!["discogs".to_string()];
        state.replace_config(replacement).unwrap();
        assert_eq!(state.launch_mode, LaunchMode::Read);
        assert_eq!(
            state
                .sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            vec!["samples", "local", "discogs", "navidrome"]
        );
    }

    #[test]
    fn adjustments_project_preview_and_apply_modes_into_real_decisions() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.apply_runtime_event(&json!({
            "event":"candidates",
            "recommended_index":1,
            "items":[{"index":1,"source":"local","width":900,"height":900,"range_class":"lower","policy_status":"accept","cache_path":"a.jpg"}]
        }));
        state.adjustments.picture_percent = 7;
        state.adjustments.preserve_source_resolution = true;
        state.adjustments.apply_edit_profile = false;
        let preview = state.candidate_decision("use").unwrap();
        assert_eq!(preview["preserve_source_resolution"], true);
        assert_eq!(preview["apply_edit_profile"], false);
        assert_eq!(preview["upscale_picture_percent"], 7);
        state.adjustments.preserve_source_resolution = false;
        state.adjustments.apply_edit_profile = true;
        let apply = state.candidate_decision("use").unwrap();
        assert_eq!(apply["preserve_source_resolution"], false);
        assert_eq!(apply["apply_edit_profile"], true);
        state.persist_adjustments_to_config();
        assert_eq!(state.config.output.upscale_picture_percent, 7);
    }

    #[test]
    fn mature_media_filters_and_selection_modes_are_composable() {
        let mut state = library();
        state.set_view(LibraryView::Albums);
        state.set_status_filters([StatusFilter::Unprocessed, StatusFilter::Incomplete]);
        assert_eq!(state.rows().len(), 2);
        assert_eq!(state.status_count(StatusFilter::Processed), 1);
        state.set_artist_filter("Alpha");
        state.set_album_filter("Fresh");
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.select_filtered_for_review().unwrap(), 1);
        assert_eq!(state.selected_count(), 1);

        state.select_none();
        state.set_artist_filter("");
        state.set_album_filter("");
        assert_eq!(state.select_filtered_for_review().unwrap(), 2);
        state.select_none();
        state.set_status_filter(StatusFilter::All);
        assert!(state.select_filtered_for_review().is_err());
        state.toggle_artist("Alpha", false);
        state.select_none();
        assert_eq!(state.select_all_for_focused_artist().unwrap(), 1);
        assert!(
            state
                .albums
                .iter()
                .any(|album| { album.snapshot.album_key == "a" && album.selected })
        );
        assert!(
            !state
                .albums
                .iter()
                .any(|album| { album.snapshot.album_key == "b" && album.selected })
        );
    }

    #[test]
    fn candidate_tri_state_comparison_hover_and_fallback_are_rust_owned() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.apply_runtime_event(&json!({
            "event":"candidates",
            "recommended_index":1,
            "items":[
                {"index":1,"source":"local","width":1800,"height":1800,"source_range_class":"Ideal","range_class":"Ideal","policy_status":"accept","acceptable":true,"image_type":"FrontCover","cache_path":"a.jpg"},
                {"index":2,"source":"discogs","width":500,"height":500,"source_range_class":"BelowMinimum","range_class":"BelowMinimum","policy_status":"reject","acceptable":false,"image_type":"BackCover","cache_path":"b.jpg"}
            ]
        }));
        assert_eq!(
            state.candidate_filter_disposition("type", "FrontCover"),
            CandidateFilterDisposition::Included
        );
        assert_eq!(
            state.cycle_candidate_filter("type", "FrontCover").unwrap(),
            CandidateFilterDisposition::Disregarded
        );
        assert_eq!(state.visible_candidates().len(), 1);
        assert_eq!(state.visible_candidates()[0].index, 2);
        assert_eq!(
            state.cycle_candidate_filter("type", "FrontCover").unwrap(),
            CandidateFilterDisposition::Neutral
        );
        assert_eq!(state.visible_candidates().len(), 2);
        state.clear_candidate_filters();
        assert!(state.toggle_candidate_comparison(2));
        assert_eq!(state.comparison_candidates().len(), 2);
        state.hover_enabled = true;
        state.set_candidate_hover(2, true);
        assert_eq!(state.preview_candidate().unwrap().index, 2);
        state.fallback_active = true;
        state.fallback_reason = "no deterministic authority".to_string();
        state.fallback_artist = "Alpha".to_string();
        state.fallback_album = "Fresh".to_string();
        let retry = state.fallback_retry_decision("Alpha", "Fresh").unwrap();
        assert_eq!(retry["action"], "retry");
    }

    #[test]
    fn artist_and_album_samples_are_discovered_and_enabled_for_each_review() {
        let directory = TempDir::new().unwrap();
        let samples = directory.path().join("samples");
        let blonde = samples.join("Bob Dylan").join("Blonde on Blonde");
        let blood = samples.join("Bob Dylan").join("Blood on the Tracks");
        fs::create_dir_all(&blonde).unwrap();
        fs::create_dir_all(&blood).unwrap();
        fs::write(blonde.join("itunes.sample.jpg"), b"artist-count").unwrap();
        fs::write(blood.join("discogs.sample.png"), b"artist-count").unwrap();

        let mut config = Config::default();
        config.samples.sample_write = true;
        config.samples.sample_dir = samples.to_string_lossy().into_owned();
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Bob Dylan", "Blonde on Blonde", "unprocessed")],
        };
        let mut state = ApplicationState::new(config, snapshot).unwrap();
        state.library.focused_artist = Some("Bob Dylan".to_string());
        state.synchronize_focused_artist();

        let source = state
            .sources
            .iter()
            .find(|source| source.name == "samples")
            .unwrap();
        assert!(source.enabled);
        assert_eq!(source.candidate_count, 2);
        assert_eq!(state.candidate_filter_count("type", "Samples"), 2);
        assert_eq!(
            state.candidate_filter_disposition("type", "Samples"),
            CandidateFilterDisposition::Included
        );
    }

    #[test]
    fn musicbrainz_filters_order_and_authority_editing_match_the_mature_contract() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.apply_runtime_event(&json!({
            "event":"musicbrainz_matches",
            "album_artist":"Alpha",
            "album":"Fresh",
            "authority_release_mbid":"33333333-3333-3333-3333-333333333333",
            "items":[
                {"index":2,"release_title":"Compilation","release_artist":"Various Artists","release_date":"2020","release_class":"compilation","release_mbid":"44444444-4444-4444-4444-444444444444","release_group_mbid":"dddddddd-dddd-dddd-dddd-dddddddddddd","release_group_title":"Compilation","country":"GB"},
                {"index":3,"release_title":"Fresh","release_artist":"Alpha","release_date":"2023","release_class":"album","release_mbid":"55555555-5555-5555-5555-555555555555","release_group_mbid":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","release_group_title":"Fresh","country":"GB"},
                {"index":1,"release_title":"Fresh","release_artist":"Alpha","release_date":"2024","release_class":"album","release_mbid":"33333333-3333-3333-3333-333333333333","release_group_mbid":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","release_group_title":"Fresh","country":"US","current":true}
            ]
        }));
        assert_eq!(state.musicbrainz_results[0].index, 1);
        assert_eq!(state.musicbrainz_results[1].index, 3);
        assert!(
            state
                .operation_status
                .contains("2 release families · 3 editions")
        );
        assert!(state.select_musicbrainz_result(1));
        assert_eq!(state.selected_musicbrainz_result, Some(1));
        assert!(
            state
                .operation_status
                .contains("Current MusicBrainz authority selected · Alpha — Fresh")
        );
        assert_eq!(state.operation_tone, "magenta");
        assert_eq!(
            state.selected_musicbrainz_result_decision().unwrap()["index"],
            1
        );
        assert!(state.musicbrainz_results[0].visited);
        state.set_musicbrainz_artist_filter("Various Artists");
        assert_eq!(state.visible_musicbrainz_results().len(), 1);
        let edited = state
            .musicbrainz_authority_decision_with_ids(
                "11111111-1111-1111-1111-111111111111",
                "22222222-2222-2222-2222-222222222222",
                "33333333-3333-3333-3333-333333333333",
            )
            .unwrap();
        assert_eq!(edited["action"], "use_musicbrainz_authority");
        let recording_only = state
            .musicbrainz_authority_decision_with_ids(
                "11111111-1111-1111-1111-111111111111",
                "22222222-2222-2222-2222-222222222222",
                "",
            )
            .unwrap();
        assert_eq!(recording_only["release_mbid"], "");
        let multi_artist = state
            .musicbrainz_authority_decision_with_ids(
                "11111111-1111-1111-1111-111111111111",
                "6de0f914-3e60-4418-be3b-42e0feb6eb4d, dff0d392-4cd5-4052-9fbb-f485df3891e5",
                "",
            )
            .unwrap();
        assert_eq!(multi_artist["action"], "use_musicbrainz_authority");
        assert!(
            state
                .musicbrainz_authority_decision_with_ids(
                    "11111111-1111-1111-1111-111111111111",
                    "6de0f914-3e60-4418-be3b-42e0feb6eb4d, invalid",
                    "",
                )
                .is_err()
        );
        assert!(
            state
                .musicbrainz_authority_decision_with_ids("", "", "")
                .is_err()
        );
        assert!(
            state
                .musicbrainz_authority_decision_with_ids("bad", "", "bad")
                .is_err()
        );
    }

    #[test]
    fn a_new_musicbrainz_result_set_clears_filters_that_would_hide_its_rows() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![album("a", "Alpha", "Fresh", "unprocessed")],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.set_musicbrainz_artist_filter("10cc");
        state.set_musicbrainz_release_type_filter("album");

        state.apply_runtime_event(&json!({
            "event":"musicbrainz_matches",
            "album_artist":"Various Artists",
            "album":"Compilation",
            "items":[
                {"index":1,"release_title":"Pop Classics","release_artist":"Various Artists","release_class":"compilation","release_mbid":"80598939-6881-4c6f-8a93-2a490d5cffa3"},
                {"index":2,"release_title":"Avro Arbeidsvitaminen, volume 8","release_artist":"Various Artists","release_class":"compilation","release_mbid":"e41a52f1-98d9-4614-a05c-e687cdba8b40"}
            ]
        }));

        assert!(state.musicbrainz_artist_filter.is_none());
        assert!(state.musicbrainz_release_type_filter.is_none());
        assert_eq!(state.visible_musicbrainz_results().len(), 2);
    }

    #[test]
    fn all_eight_adjustments_can_be_activated_toggled_and_reordered() {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.set_primary_adjustments(&[
            "Gamma".to_string(),
            "Gamma".to_string(),
            "Softness".to_string(),
        ]);
        assert_eq!(state.primary_adjustments.len(), 2);
        assert_eq!(state.primary_adjustments[0], "Gamma");
        assert!(state.move_primary_adjustment("Gamma", 1));
        assert_eq!(state.primary_adjustments[1], "Gamma");
        assert!(state.promote_primary_adjustment("Brightness"));
        assert_eq!(state.primary_adjustments.len(), 3);
        assert!(state.promote_primary_adjustment("Brightness"));
        assert_eq!(state.primary_adjustments.len(), 2);
        state.activate_all_adjustments();
        assert_eq!(state.primary_adjustments.len(), 8);
        assert!(
            state
                .primary_adjustments
                .iter()
                .any(|name| name == "Exposure")
        );
        assert!(
            state
                .primary_adjustments
                .iter()
                .any(|name| name == "Softness")
        );
    }
}
