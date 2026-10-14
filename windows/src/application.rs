//! Rust-owned application state for the Windows Slint presentation layer.
//!
//! Widgets render projections of this module and send actions back to it. The
//! selection set, scan queue, review state, and editing values never live only
//! in Slint controls.

use crate::config::{Config, Mode, resolve_sources};
use crate::media_database::{MediaAlbumSnapshot, MediaSnapshot};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
        self.snapshot.compilation
            && self.snapshot.compilation_track_art_eligible
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
    pub selected: bool,
    pub eligible: bool,
    pub album_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationTrackRow {
    pub album_key: String,
    pub path: String,
    pub file_name: String,
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
        }
    }

    pub fn selected_count(&self) -> usize {
        self.albums.iter().filter(|album| album.selected).count()
    }

    pub fn compilation_track_rows(&self) -> Vec<CompilationTrackRow> {
        let focused = self.focused_album_key.as_deref();
        self.albums
            .iter()
            .filter(|album| {
                self.show_tracks
                    || focused.is_some_and(|album_key| {
                        album.snapshot.album_key.eq_ignore_ascii_case(album_key)
                    })
            })
            .flat_map(|album| {
                album
                    .snapshot
                    .compilation_tracks
                    .iter()
                    .map(move |track| CompilationTrackRow {
                        album_key: album.snapshot.album_key.clone(),
                        path: track.path.clone(),
                        file_name: if self.show_tracks {
                            format!(
                                "{} — {} — {}",
                                album.snapshot.artist,
                                album.snapshot.title,
                                std::path::Path::new(&track.path)
                                    .file_name()
                                    .and_then(|value| value.to_str())
                                    .unwrap_or(&track.path)
                            )
                        } else {
                            std::path::Path::new(&track.path)
                                .file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or(&track.path)
                                .to_string()
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

    fn status_matches(&self, album: &AlbumEntry, artist_state: ArtistState) -> bool {
        let matches = |filter| match filter {
            StatusFilter::All => true,
            StatusFilter::Unprocessed => album.state == AlbumState::New,
            StatusFilter::Incomplete => album.state == AlbumState::Incomplete,
            StatusFilter::Processed => album.state == AlbumState::Processed,
            StatusFilter::Bypassed => album.state == AlbumState::Bypassed,
            StatusFilter::PartialTimeout => {
                album.state == AlbumState::TimeoutActive || artist_state == ArtistState::Partial
            }
            StatusFilter::ArtistComplete => artist_state == ArtistState::Complete,
            StatusFilter::ArtistContainsBypass => artist_state == ArtistState::ContainsBypass,
        };
        self.status_filters.iter().copied().any(matches)
    }

    fn album_visible(&self, index: usize, artists: &BTreeMap<String, Vec<usize>>) -> bool {
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
        let state = artists
            .get(&album.snapshot.artist)
            .map(|indices| Self::artist_state(indices.iter().map(|value| &self.albums[*value])))
            .unwrap_or(ArtistState::Unprocessed);
        self.status_matches(album, state)
    }

    pub fn rows(&self) -> Vec<LibraryRow> {
        let artists = self.artists();
        match self.view {
            LibraryView::Artists => artists
                .iter()
                .filter_map(|(artist, indices)| {
                    let visible = indices
                        .iter()
                        .copied()
                        .filter(|index| self.album_visible(*index, &artists))
                        .collect::<Vec<_>>();
                    if visible.is_empty() {
                        return None;
                    }
                    let state =
                        Self::artist_state(indices.iter().map(|index| &self.albums[*index]));
                    Some(LibraryRow {
                        key: artist.clone(),
                        primary: artist.clone(),
                        secondary: format!(
                            "{} album{}",
                            indices.len(),
                            if indices.len() == 1 { "" } else { "s" }
                        ),
                        status: state.label().to_string(),
                        selected: indices.iter().any(|index| self.albums[*index].selected),
                        eligible: indices
                            .iter()
                            .any(|index| self.albums[*index].eligible_by_default()),
                        album_count: indices.len(),
                    })
                })
                .collect(),
            LibraryView::Albums | LibraryView::Folders => self
                .albums
                .iter()
                .enumerate()
                .filter(|(index, _)| self.album_visible(*index, &artists))
                .map(|(_, album)| LibraryRow {
                    key: album.snapshot.album_key.clone(),
                    primary: if self.view == LibraryView::Albums {
                        album.snapshot.title.clone()
                    } else {
                        album.snapshot.path.clone()
                    },
                    secondary: if self.view == LibraryView::Albums {
                        format!("{} · {}", album.snapshot.artist, album.snapshot.path)
                    } else {
                        format!("{} · {}", album.snapshot.artist, album.snapshot.title)
                    },
                    status: album.state.label().to_string(),
                    selected: album.selected,
                    eligible: album.eligible_by_default() || album.state == AlbumState::Processed,
                    album_count: 1,
                })
                .collect(),
        }
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
        if value == StatusFilter::All {
            return self.albums.len();
        }
        if matches!(
            value,
            StatusFilter::ArtistComplete | StatusFilter::ArtistContainsBypass
        ) {
            return self
                .artists()
                .values()
                .filter(|indices| {
                    let state =
                        Self::artist_state(indices.iter().map(|index| &self.albums[*index]));
                    matches!(
                        (value, state),
                        (StatusFilter::ArtistComplete, ArtistState::Complete)
                            | (
                                StatusFilter::ArtistContainsBypass,
                                ArtistState::ContainsBypass
                            )
                    )
                })
                .count();
        }
        self.albums
            .iter()
            .filter(|album| match value {
                StatusFilter::Unprocessed => album.state == AlbumState::New,
                StatusFilter::Incomplete => album.state == AlbumState::Incomplete,
                StatusFilter::Processed => album.state == AlbumState::Processed,
                StatusFilter::Bypassed => album.state == AlbumState::Bypassed,
                StatusFilter::PartialTimeout => album.state == AlbumState::TimeoutActive,
                StatusFilter::All
                | StatusFilter::ArtistComplete
                | StatusFilter::ArtistContainsBypass => false,
            })
            .count()
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

    pub fn clear_artist_scope(&mut self) {
        self.focused_artist = None;
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
        if album.selected {
            album.selected = false;
            album.bypass_override = false;
            let track_paths = album
                .snapshot
                .compilation_tracks
                .iter()
                .map(|track| track.path.to_ascii_lowercase())
                .collect::<Vec<_>>();
            self.selected_compilation_track_paths
                .retain(|path| !track_paths.contains(path));
            return SelectionResult::Changed;
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
        let Some(album) = self
            .albums
            .iter_mut()
            .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(album_key))
        else {
            return SelectionResult::NotFound;
        };
        if !album
            .snapshot
            .compilation_tracks
            .iter()
            .any(|track| track.path.eq_ignore_ascii_case(track_path))
        {
            return SelectionResult::NotFound;
        }
        if album.state == AlbumState::TimeoutActive {
            return SelectionResult::BlockedByTimeout(album.snapshot.title.clone());
        }
        if album.state == AlbumState::Bypassed && !include_bypassed {
            return SelectionResult::NeedsBypassOverride(album.snapshot.title.clone());
        }
        self.focused_album_key = Some(album.snapshot.album_key.clone());
        let normalized = track_path.to_ascii_lowercase();
        if !self.selected_compilation_track_paths.remove(&normalized) {
            self.selected_compilation_track_paths.insert(normalized);
        }
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

    pub fn select_filtered(&mut self) {
        let artists = self.artists();
        let visible = self
            .albums
            .iter()
            .enumerate()
            .filter(|(index, _)| self.album_visible(*index, &artists))
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
        {
            return Err(
                "Select [FILTERED] requires Artist, Album, or library search text.".to_string(),
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
        for album in &mut refreshed.albums {
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
        self.source.eq_ignore_ascii_case("local") || !self.local_origin.trim().is_empty()
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
    pub release_title: String,
    pub release_artist: String,
    pub release_date: String,
    pub release_class: String,
    pub country: String,
    pub resolution: String,
    pub release_mbid: String,
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
    pub edit_preview_path: String,
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
    pub musicbrainz_back_available: bool,
    pub musicbrainz_artist_filter: Option<String>,
    pub musicbrainz_release_type_filter: Option<String>,
    pub musicbrainz_authority: MusicBrainzAuthority,
    pub musicbrainz_results: Vec<MusicBrainzResult>,
    pub selected_musicbrainz_result: Option<usize>,
    pub adjustments: AdjustmentState,
    pub primary_adjustments: Vec<String>,
    pub media_selector_expanded: bool,
    pub candidate_filter_expanded: bool,
    pub artwork_editing_expanded: bool,
    pub show_artwork: bool,
    pub upscale_preview_only: bool,
    pub log: Vec<String>,
    pub last_error: Option<String>,
    pub service_status: String,
    pub run_total: usize,
    pub run_completed: usize,
    pub run_failed: usize,
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
        let sources = std::iter::once("local".to_string())
            .chain(config.sources.cover_sources.iter().cloned())
            .map(|name| SourceState {
                enabled: name == "local"
                    || enabled_source_names
                        .iter()
                        .any(|enabled| enabled.eq_ignore_ascii_case(&name)),
                name,
                candidate_count: 0,
                status: "Ready".to_string(),
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
            edit_preview_path: String::new(),
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
            musicbrainz_back_available: false,
            musicbrainz_artist_filter: None,
            musicbrainz_release_type_filter: None,
            musicbrainz_authority: MusicBrainzAuthority::default(),
            musicbrainz_results: Vec::new(),
            selected_musicbrainz_result: None,
            adjustments,
            primary_adjustments: default_primary_adjustments(),
            media_selector_expanded: true,
            candidate_filter_expanded: true,
            artwork_editing_expanded: true,
            show_artwork: true,
            upscale_preview_only: true,
            log: vec!["SPLINED Rust application initialized.".to_string()],
            last_error: None,
            service_status: "Ready".to_string(),
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
        self.sources = std::iter::once("local".to_string())
            .chain(config.sources.cover_sources.iter().cloned())
            .map(|name| {
                let previous = existing.get(&name.to_ascii_lowercase());
                SourceState {
                    enabled: name.eq_ignore_ascii_case("local")
                        || enabled
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(&name)),
                    candidate_count: previous.map_or(0, |source| source.candidate_count),
                    status: previous
                        .map(|source| source.status.clone())
                        .unwrap_or_else(|| "Ready".to_string()),
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
        let changed = !self
            .current_album_path
            .eq_ignore_ascii_case(&album.snapshot.path);
        self.current_album_path = album.snapshot.path.clone();
        self.current_artist = album.snapshot.artist.clone();
        self.current_album = album.snapshot.title.clone();
        self.current_cover_path = album.snapshot.cover_path.clone();
        if changed {
            self.edit_preview_path.clear();
        }
        true
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
        }
        found
    }

    pub fn visible_candidates(&self) -> Vec<&CandidateState> {
        self.candidates
            .iter()
            .filter(|candidate| {
                self.candidate_source_filter
                    .as_ref()
                    .is_none_or(|source| candidate.source.eq_ignore_ascii_case(source))
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
        let Some((included, excluded)) = self.candidate_filter_sets(dimension) else {
            return true;
        };
        if excluded
            .iter()
            .any(|key| candidate.matches_filter(dimension, key))
        {
            return false;
        }
        included.is_empty()
            || included
                .iter()
                .any(|key| candidate.matches_filter(dimension, key))
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

    pub fn candidate_filter_count(&self, dimension: &str, key: &str) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.matches_filter(dimension, key))
            .count()
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
        let configured = self
            .config
            .sources
            .cover_sources
            .iter()
            .find(|item| item.eq_ignore_ascii_case(source))
            .cloned()
            .ok_or_else(|| format!("Unknown artwork source: {source}"))?;
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
            let local = self
                .sources
                .iter()
                .find(|item| item.name == "local")
                .cloned();
            let by_name = self
                .sources
                .iter()
                .map(|item| (item.name.to_ascii_lowercase(), item.clone()))
                .collect::<HashMap<_, _>>();
            self.sources.clear();
            if let Some(local) = local {
                self.sources.push(local);
            }
            self.sources.extend(
                self.config
                    .sources
                    .cover_sources
                    .iter()
                    .filter_map(|name| by_name.get(&name.to_ascii_lowercase()).cloned()),
            );
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
        for fallback in catalog {
            if normalized.len() >= 5 {
                break;
            }
            if !normalized
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(fallback))
            {
                normalized.push(fallback.to_string());
            }
        }
        normalized.truncate(5);
        self.primary_adjustments = normalized;
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
        if self
            .primary_adjustments
            .iter()
            .any(|value| value.eq_ignore_ascii_case(canonical))
        {
            return false;
        }
        if self.primary_adjustments.len() >= 5 {
            self.primary_adjustments.pop();
        }
        self.primary_adjustments.insert(0, (*canonical).to_string());
        self.set_primary_adjustments(&self.primary_adjustments.clone());
        true
    }

    pub fn candidate_decision(&self, action: &str) -> Result<Value, String> {
        match action {
            "use" => {
                let candidate = self
                    .selected_candidate()
                    .ok_or_else(|| "Select an artwork candidate before applying it.".to_string())?;
                let mut decision = self.adjustments.as_decision_fields();
                let object = decision
                    .as_object_mut()
                    .expect("adjustment decision is an object");
                object.insert("action".to_string(), json!("use"));
                object.insert("index".to_string(), json!(candidate.index));
                Ok(decision)
            }
            "keep_local" => {
                let candidate = self
                    .candidates
                    .iter()
                    .find(|candidate| candidate.source.eq_ignore_ascii_case("local"))
                    .ok_or_else(|| "No Local artwork candidate is available.".to_string())?;
                Ok(json!({"action": "use", "index": candidate.index}))
            }
            "bypass" => Ok(json!({"action": "bypass"})),
            "clear_bypass" => Ok(json!({"action": "clear_bypass"})),
            "skip_album" => Ok(json!({"action": "skip_album"})),
            "retry_musicbrainz" => Ok(json!({"action": "retry_musicbrainz"})),
            "back_to_musicbrainz" => Ok(json!({"action": "back_musicbrainz"})),
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
        if !self
            .musicbrainz_results
            .iter()
            .any(|item| item.index == index)
        {
            return false;
        }
        self.selected_musicbrainz_result = Some(index);
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
            &self.musicbrainz_authority.release_mbid,
        )
    }

    pub fn musicbrainz_authority_decision_with_ids(
        &self,
        recording_mbid: &str,
        artist_mbids: &str,
        release_mbid: &str,
    ) -> Result<Value, String> {
        let recording_mbid = recording_mbid.trim();
        let release_mbid = release_mbid.trim();
        let artist_mbids = artist_mbids
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !valid_optional_mbid(recording_mbid)
            || !valid_mbid(release_mbid)
            || artist_mbids.iter().any(|value| !valid_mbid(value))
        {
            return Err(
                "Release requires one MusicBrainz UUID. Recording accepts one UUID, and Artist accepts one or more comma-separated UUIDs."
                    .to_string(),
            );
        }
        Ok(json!({
            "action": "use_musicbrainz_authority",
            "recording_mbid": recording_mbid,
            "artist_mbids": artist_mbids.join(","),
            "release_mbid": release_mbid,
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
                self.current_album_path = string_field(event, "album_path");
                self.edit_preview_path.clear();
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
                self.push_log(format!(
                    "MusicBrainz authority: {} — {}",
                    self.musicbrainz_authority.artist, self.musicbrainz_authority.album
                ));
            }
            "candidates" => self.apply_candidates(event),
            "decision_required" => {
                self.phase = RunPhase::AwaitingCandidate;
                self.musicbrainz_back_available = bool_field(event, "musicbrainz_back_available");
                self.push_log("Artwork decision required.".to_string());
            }
            "musicbrainz_matches" => self.apply_musicbrainz_matches(event),
            "album_completed" => {
                self.phase = RunPhase::Running;
                self.run_completed = self.run_completed.saturating_add(1);
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
                self.push_log(format!("ERROR: {message}"));
            }
            "activity" => self.push_log(string_field(event, "message")),
            "source_results_restored" => {
                self.phase = RunPhase::AwaitingCandidate;
                self.push_log("Returned to the cached artwork source results.".to_string());
            }
            "provider_timing" => {
                let source = string_field(event, "source");
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
        self.musicbrainz_back_available = bool_field(event, "musicbrainz_back_available");
        for source in &mut self.sources {
            source.candidate_count = self
                .candidates
                .iter()
                .filter(|item| item.source.eq_ignore_ascii_case(&source.name))
                .count();
        }
        self.push_log(format!(
            "Found {} artwork candidates.",
            self.candidates.len()
        ));
    }

    fn apply_musicbrainz_matches(&mut self, event: &Value) {
        self.phase = RunPhase::AwaitingMusicBrainz;
        self.selected_musicbrainz_result = None;
        self.musicbrainz_authority = MusicBrainzAuthority {
            artist: string_field(event, "album_artist"),
            album: string_field(event, "album"),
            recording_mbid: string_field(event, "authority_recording_mbid"),
            artist_mbids: string_field(event, "authority_artist_mbids"),
            release_mbid: string_field(event, "authority_release_mbid"),
            release_title: string_field(event, "authority_release_title"),
            release_artist: string_field(event, "authority_release_artist"),
        };
        self.musicbrainz_results = event
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(MusicBrainzResult {
                    index: item.get("index")?.as_u64()? as usize,
                    release_title: string_field(item, "release_title"),
                    release_artist: string_field(item, "release_artist"),
                    release_date: string_field(item, "release_date"),
                    release_class: string_field(item, "release_class"),
                    country: string_field(item, "country"),
                    resolution: string_field(item, "resolution"),
                    release_mbid: string_field(item, "release_mbid"),
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
        self.musicbrainz_results.sort_by(|left, right| {
            musicbrainz_family_rank(left)
                .cmp(&musicbrainz_family_rank(right))
                .then_with(|| {
                    if musicbrainz_family_rank(left) == 0 {
                        left.release_artist
                            .to_ascii_lowercase()
                            .cmp(&right.release_artist.to_ascii_lowercase())
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .then_with(|| right.release_date.cmp(&left.release_date))
                .then_with(|| country_rank(&left.country).cmp(&country_rank(&right.country)))
                .then_with(|| {
                    left.release_title
                        .to_ascii_lowercase()
                        .cmp(&right.release_title.to_ascii_lowercase())
                })
        });
        self.push_log(format!(
            "MusicBrainz returned {} release results.",
            self.musicbrainz_results.len()
        ));
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

fn string_field(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn default_primary_adjustments() -> Vec<String> {
    ["Picture", "Sharpen", "Contrast", "Brightness", "Color"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn valid_optional_mbid(value: &str) -> bool {
    value.is_empty() || valid_mbid(value)
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

fn musicbrainz_family_rank(item: &MusicBrainzResult) -> u8 {
    if item.release_class.eq_ignore_ascii_case("soundtrack") {
        1
    } else if item.release_class.eq_ignore_ascii_case("compilation") {
        2
    } else if item.release_artist.eq_ignore_ascii_case("Various Artists") {
        3
    } else if item.release_artist.trim().is_empty() {
        4
    } else {
        0
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
                embedded_artwork_recorded: true,
            },
            MediaTrackSnapshot {
                path: r"C:\Music\Alpha\Done\02.flac".to_string(),
                title: "Two".to_string(),
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
            state.musicbrainz_authority_decision().unwrap()["action"],
            "use_musicbrainz_authority"
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
            vec!["local", "itunes", "discogs"]
        );
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
            vec!["local", "itunes"]
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
            vec!["local", "discogs"]
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
            state.cycle_candidate_filter("type", "FrontCover").unwrap(),
            CandidateFilterDisposition::Included
        );
        assert_eq!(state.visible_candidates().len(), 1);
        assert_eq!(
            state.cycle_candidate_filter("type", "FrontCover").unwrap(),
            CandidateFilterDisposition::Disregarded
        );
        assert_eq!(state.visible_candidates().len(), 1);
        assert_eq!(state.visible_candidates()[0].index, 2);
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
                {"index":2,"release_title":"Compilation","release_artist":"Various Artists","release_date":"2020","release_class":"compilation","release_mbid":"44444444-4444-4444-4444-444444444444","country":"GB"},
                {"index":1,"release_title":"Fresh","release_artist":"Alpha","release_date":"2024","release_class":"album","release_mbid":"33333333-3333-3333-3333-333333333333","country":"US","current":true}
            ]
        }));
        assert_eq!(state.musicbrainz_results[0].index, 1);
        assert!(state.select_musicbrainz_result(1));
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
        assert!(
            state
                .musicbrainz_authority_decision_with_ids("bad", "", "bad")
                .is_err()
        );
    }

    #[test]
    fn primary_adjustments_are_exactly_five_and_reorderable() {
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
        assert_eq!(state.primary_adjustments.len(), 5);
        assert_eq!(state.primary_adjustments[0], "Gamma");
        assert!(state.move_primary_adjustment("Gamma", 1));
        assert_eq!(state.primary_adjustments[1], "Gamma");
        assert!(state.promote_primary_adjustment("Brightness"));
        assert_eq!(state.primary_adjustments[0], "Brightness");
        assert_eq!(state.primary_adjustments.len(), 5);
    }
}
