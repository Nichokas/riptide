// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist, Track};
use super::{App, SortField, SortPalette, StatusLevel, Tab};

impl App {
    // ── Home ──────────────────────────────────────────────────────────────────

    pub fn load_home(&mut self) {
        self.home_new_releases.loading = true;
        self.home_daily_mixes.loading = true;
        self.home_discovery_mixes.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadNewReleases);
        let _ = self.api_tx.send(ApiRequest::LoadDailyMixes);
        let _ = self.api_tx.send(ApiRequest::LoadDiscoveryMixes);
    }

    // ── Favorites ─────────────────────────────────────────────────────────────

    fn favorite_track(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::FavoriteTrack { track_id: track.id });
        if !self.favorites.items.iter().any(|t| t.id == track.id) {
            self.favorites.items.insert(0, track.clone());
            self.favorites.total = self.favorites.total.saturating_add(1);
            self.favorites.selected = self.favorites.selected.saturating_add(1);
            self.rebuild_favorite_track_ids();
        }
        self.set_status(format!("Added '{}' to favorites", track.title), StatusLevel::Info);
    }

    #[allow(dead_code)]
    fn unfavorite_track(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::UnfavoriteTrack { track_id: track.id });
        self.set_status(format!("Removed '{}' from favorites", track.title), StatusLevel::Info);
    }

    pub fn toggle_favorite_track(&mut self, track: &Track) {
        if self.favorite_track_ids.contains(&track.id) {
            self.set_status(
                format!("'{}' is already a favorite — press 'd' to remove, 'u' to undo", track.title),
                StatusLevel::Info,
            );
            return;
        }
        self.favorite_track(track);
    }

    pub fn remove_favorite_track_with_undo(&mut self, track: &Track) {
        if !self.favorite_track_ids.contains(&track.id) {
            return;
        }
        if let Some(idx) = self.favorites.items.iter().position(|t| t.id == track.id) {
            let removed = self.favorites.items.remove(idx);
            self.undo_entry = Some(crate::app::UndoEntry::Track { track: removed.clone(), index: idx });
            self.favorites.total = self.favorites.total.saturating_sub(1);
            if self.favorites.selected > idx {
                self.favorites.selected = self.favorites.selected.saturating_sub(1);
            } else if self.favorites.selected >= self.favorites.items.len() && !self.favorites.items.is_empty() {
                self.favorites.selected = self.favorites.items.len() - 1;
            } else if self.favorites.items.is_empty() {
                self.favorites.selected = 0;
            }
            self.rebuild_favorite_track_ids();
            self.pending_track_removals.insert(removed.id);
            let _ = self.api_tx.send(ApiRequest::UnfavoriteTrack { track_id: removed.id });
            self.set_status(format!("Removed '{}' — press 'u' to undo", removed.title), StatusLevel::Info);
        } else {
            let track_clone = track.clone();
            self.undo_entry = Some(crate::app::UndoEntry::Track { track: track_clone.clone(), index: 0 });
            self.favorites.total = self.favorites.total.saturating_sub(1);
            self.pending_track_removals.insert(track.id);
            let _ = self.api_tx.send(ApiRequest::UnfavoriteTrack { track_id: track.id });
            self.set_status(format!("Removed '{}' — press 'u' to undo", track.title), StatusLevel::Info);
            self.rebuild_favorite_track_ids();
        }
    }

    pub fn undo_last(&mut self) {
        let Some(entry) = self.undo_entry.take() else {
            self.set_status("Nothing to undo".to_string(), StatusLevel::Info);
            return;
        };
        match entry {
            crate::app::UndoEntry::Track { track, index } => {
                let id = track.id;
                let title = track.title.clone();
                let len_before = self.favorites.items.len();
                let insert_at = index.min(len_before);
                self.favorites.items.insert(insert_at, track);
                self.favorites.total = self.favorites.total.saturating_add(1);
                if len_before > 0 && insert_at <= self.favorites.selected {
                    self.favorites.selected = self.favorites.selected.saturating_add(1);
                }
                self.favorites.selected = self.favorites.selected
                    .min(self.favorites.items.len().saturating_sub(1));
                self.rebuild_favorite_track_ids();
                if self.pending_track_removals.contains(&id) {
                    self.suppressed_track_removals.insert(id);
                    self.pending_refavorite_tracks.insert(id);
                } else {
                    let _ = self.api_tx.send(ApiRequest::FavoriteTrack { track_id: id });
                }
                self.set_status(format!("Restored '{title}'"), StatusLevel::Info);
            }
            crate::app::UndoEntry::Album { album, index } => {
                let id = album.id;
                let title = album.title.clone();
                let len_before = self.fav_albums.items.len();
                let insert_at = index.min(len_before);
                self.fav_albums.items.insert(insert_at, album);
                self.fav_albums.total = self.fav_albums.total.saturating_add(1);
                if len_before > 0 && insert_at <= self.fav_albums.selected {
                    self.fav_albums.selected = self.fav_albums.selected.saturating_add(1);
                }
                self.fav_albums.selected = self.fav_albums.selected
                    .min(self.fav_albums.items.len().saturating_sub(1));
                self.rebuild_favorite_album_ids();
                if self.pending_album_removals.contains(&id) {
                    self.suppressed_album_removals.insert(id);
                    self.pending_refavorite_albums.insert(id);
                } else {
                    let _ = self.api_tx.send(ApiRequest::FavoriteAlbum { album_id: id });
                }
                self.set_status(format!("Restored '{title}'"), StatusLevel::Info);
            }
        }
    }

    // ── Following ─────────────────────────────────────────────────────────────

    fn follow_artist(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::FollowArtist { artist_id: artist.id });
        if !self.artists.items.iter().any(|a| a.id == artist.id) {
            let pos = self.artists.items
                .partition_point(|a| a.name.to_lowercase() < artist.name.to_lowercase());
            self.artists.items.insert(pos, artist.clone());
            self.artists.total = self.artists.total.saturating_add(1);
            if pos <= self.artists.selected {
                self.artists.selected = self.artists.selected.saturating_add(1);
            }
        }
        self.set_status(format!("Following {}", artist.name), StatusLevel::Info);
    }

    fn unfollow_artist(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::UnfollowArtist { artist_id: artist.id });
        self.set_status(format!("Unfollowed {}", artist.name), StatusLevel::Info);
    }

    pub fn toggle_follow_artist(&mut self, artist: &Artist) {
        if self.artists.items.iter().any(|a| a.id == artist.id) {
            self.unfollow_artist(artist);
        } else {
            self.follow_artist(artist);
        }
    }

    // ── Albums ────────────────────────────────────────────────────────────────

    fn favorite_album(&mut self, album: &Album) {
        let _ = self.api_tx.send(ApiRequest::FavoriteAlbum { album_id: album.id });
        if !self.fav_albums.items.iter().any(|a| a.id == album.id) {
            self.fav_albums.items.insert(0, album.clone());
            self.fav_albums.total = self.fav_albums.total.saturating_add(1);
            self.fav_albums.selected = self.fav_albums.selected.saturating_add(1);
            self.rebuild_favorite_album_ids();
        }
        self.set_status(format!("Added '{}' to albums", album.title), StatusLevel::Info);
    }

    #[allow(dead_code)]
    fn unfavorite_album(&mut self, album: &Album) {
        let _ = self.api_tx.send(ApiRequest::UnfavoriteAlbum { album_id: album.id });
        self.set_status(format!("Removed '{}' from albums", album.title), StatusLevel::Info);
    }

    pub fn toggle_favorite_album(&mut self, album: &Album) {
        if self.favorite_album_ids.contains(&album.id) {
            self.set_status(
                format!("'{}' is already a favorite — press 'd' to remove, 'u' to undo", album.title),
                StatusLevel::Info,
            );
            return;
        }
        self.favorite_album(album);
    }

    pub fn remove_favorite_album_with_undo(&mut self, album: &Album) {
        if !self.favorite_album_ids.contains(&album.id) {
            return;
        }
        if let Some(idx) = self.fav_albums.items.iter().position(|a| a.id == album.id) {
            let removed = self.fav_albums.items.remove(idx);
            self.undo_entry = Some(crate::app::UndoEntry::Album { album: removed.clone(), index: idx });
            self.fav_albums.total = self.fav_albums.total.saturating_sub(1);
            if self.fav_albums.selected > idx {
                self.fav_albums.selected = self.fav_albums.selected.saturating_sub(1);
            } else if self.fav_albums.selected >= self.fav_albums.items.len() && !self.fav_albums.items.is_empty() {
                self.fav_albums.selected = self.fav_albums.items.len() - 1;
            } else if self.fav_albums.items.is_empty() {
                self.fav_albums.selected = 0;
            }
            self.rebuild_favorite_album_ids();
            self.pending_album_removals.insert(removed.id);
            let _ = self.api_tx.send(ApiRequest::UnfavoriteAlbum { album_id: removed.id });
            self.set_status(format!("Removed '{}' — press 'u' to undo", removed.title), StatusLevel::Info);
        } else {
            let album_clone = album.clone();
            self.undo_entry = Some(crate::app::UndoEntry::Album { album: album_clone.clone(), index: 0 });
            self.fav_albums.total = self.fav_albums.total.saturating_sub(1);
            self.pending_album_removals.insert(album.id);
            let _ = self.api_tx.send(ApiRequest::UnfavoriteAlbum { album_id: album.id });
            self.set_status(format!("Removed '{}' — press 'u' to undo", album.title), StatusLevel::Info);
            self.rebuild_favorite_album_ids();
        }
    }

    // ── Playlists ─────────────────────────────────────────────────────────────

    fn save_playlist(&mut self, playlist: &Playlist) {
        let _ = self.api_tx.send(ApiRequest::SavePlaylist { uuid: playlist.uuid.clone() });
        if !self.playlists.items.iter().any(|p| p.uuid == playlist.uuid) {
            self.playlists.items.insert(0, playlist.clone());
            self.playlists.total = self.playlists.total.saturating_add(1);
        }
        self.set_status(format!("Saved '{}' to playlists", playlist.title), StatusLevel::Info);
    }

    fn remove_playlist(&mut self, playlist: &Playlist) {
        let _ = self.api_tx.send(ApiRequest::RemovePlaylist { uuid: playlist.uuid.clone() });
        self.set_status(format!("Removed '{}' from playlists", playlist.title), StatusLevel::Info);
    }

    pub fn toggle_save_playlist(&mut self, playlist: &Playlist) {
        if self.playlists.items.iter().any(|p| p.uuid == playlist.uuid) {
            self.remove_playlist(playlist);
        } else {
            self.save_playlist(playlist);
        }
    }

    // ── Radio ─────────────────────────────────────────────────────────────────

    pub fn start_track_radio(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::TrackRadio { track_id: track.id });
        self.set_status(format!("Loading radio for '{}'…", track.title), StatusLevel::Info);
    }

    pub fn start_artist_radio(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::ArtistRadio { artist_id: artist.id });
        self.set_status(format!("Loading radio for {}…", artist.name), StatusLevel::Info);
    }

    // ── Sort ──────────────────────────────────────────────────────────────────

    /// The sort in effect for the active tab, or `None` on tabs that don't sort.
    ///
    /// An unset field means alphabetical — the same fallback the `sort_*`
    /// helpers use — so this reports what the list is actually ordered by rather
    /// than whether the user has explicitly chosen anything.
    pub fn active_sort(&self) -> Option<SortField> {
        let field = match self.current_tab {
            Tab::Favorites => self.tracks_sort,
            Tab::Artists   => self.artists_sort,
            Tab::Albums    => self.fav_albums_sort,
            Tab::Playlists => self.playlists_sort,
            Tab::Home | Tab::Search => return None,
        };
        Some(field.unwrap_or(SortField::Alphabetical))
    }

    pub fn open_sort_palette(&mut self) {
        self.sort_palette.active = true;
        // Land on the sort that's already applied, so the palette reflects the
        // current state and Enter re-confirms it instead of silently switching
        // to whichever option happens to be listed first.
        let current = self.active_sort();
        self.sort_palette.selected = SortPalette::get_options(self.current_tab)
            .iter()
            .position(|(_, field)| Some(*field) == current)
            .unwrap_or(0);
    }

    pub fn apply_sort(&mut self, field: SortField) {
        self.sort_palette.active = false;
        match self.current_tab {
            Tab::Home | Tab::Search => {}
            Tab::Favorites => { self.tracks_sort = Some(field); self.sort_favorites(); }
            Tab::Artists   => { self.artists_sort = Some(field);   self.sort_artists(); }
            Tab::Albums    => { self.fav_albums_sort = Some(field); self.sort_fav_albums(); }
            Tab::Playlists => { self.playlists_sort = Some(field); self.sort_playlists(); }
        }
    }

    // ── Sorting ──────────────────────────────────────────────────────────────
    //
    // Each list's ordering lives in one place so it can be applied both when the
    // user picks from the sort palette and when fresh data arrives. The response
    // handlers used to inline "sort alphabetically if no sort is set", which
    // meant a sort restored from preferences suppressed the default without ever
    // applying itself — leaving the list in raw API order.
    //
    // `None` means "never chosen", which sorts alphabetically.

    pub(crate) fn sort_favorites(&mut self) {
        match self.tracks_sort.unwrap_or(SortField::Alphabetical) {
            SortField::Alphabetical => self.favorites.items
                .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
            SortField::LastAdded => self.favorites.items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            SortField::ByArtist => self.favorites.items
                .sort_by(|a, b| a.artist_name().to_lowercase().cmp(&b.artist_name().to_lowercase())),
        }
    }

    pub(crate) fn sort_artists(&mut self) {
        match self.artists_sort.unwrap_or(SortField::Alphabetical) {
            SortField::LastAdded => self.artists.items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            // Artists have no album/artist axis to sort on, so anything else
            // falls back to name order.
            _ => self.artists.items
                .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        }
    }

    pub(crate) fn sort_fav_albums(&mut self) {
        match self.fav_albums_sort.unwrap_or(SortField::Alphabetical) {
            SortField::Alphabetical => self.fav_albums.items
                .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
            SortField::LastAdded => self.fav_albums.items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            SortField::ByArtist => self.fav_albums.items
                .sort_by(|a, b| a.artist_name().to_lowercase().cmp(&b.artist_name().to_lowercase())),
        }
    }

    pub(crate) fn sort_playlists(&mut self) {
        match self.playlists_sort.unwrap_or(SortField::Alphabetical) {
            SortField::LastAdded => self.playlists.items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            _ => self.playlists.items
                .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        }
    }
}
