// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist, Track};
use super::{App, SortField, StatusLevel, Tab};

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

    fn unfavorite_track(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::UnfavoriteTrack { track_id: track.id });
        self.set_status(format!("Removed '{}' from favorites", track.title), StatusLevel::Info);
    }

    pub fn toggle_favorite_track(&mut self, track: &Track) {
        if self.favorites.items.iter().any(|t| t.id == track.id) {
            self.unfavorite_track(track);
        } else {
            self.favorite_track(track);
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
        }
        self.set_status(format!("Added '{}' to albums", album.title), StatusLevel::Info);
    }

    fn unfavorite_album(&mut self, album: &Album) {
        let _ = self.api_tx.send(ApiRequest::UnfavoriteAlbum { album_id: album.id });
        self.set_status(format!("Removed '{}' from albums", album.title), StatusLevel::Info);
    }

    pub fn toggle_favorite_album(&mut self, album: &Album) {
        if self.fav_albums.items.iter().any(|a| a.id == album.id) {
            self.unfavorite_album(album);
        } else {
            self.favorite_album(album);
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

    pub fn open_sort_palette(&mut self) {
        self.sort_palette.active = true;
        self.sort_palette.selected = 0;
    }

    pub fn apply_sort(&mut self, field: SortField) {
        self.sort_palette.active = false;
        match self.current_tab {
            Tab::Home => {},
            Tab::Favorites => {
                self.favorites_sort = Some(field);
                match field {
                    SortField::Alphabetical => self.favorites.items
                        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                    SortField::LastAdded => self.favorites.items
                        .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
                    SortField::ByArtist => self.favorites.items
                        .sort_by(|a, b| a.artist_name().to_lowercase().cmp(&b.artist_name().to_lowercase())),
                }
            }
            Tab::Artists => {
                self.artists_sort = Some(field);
                match field {
                    SortField::Alphabetical => self.artists.items
                        .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                    SortField::LastAdded => self.artists.items
                        .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
                    _ => {},
                }
            }
            Tab::Albums => {
                self.fav_albums_sort = Some(field);
                match field {
                    SortField::Alphabetical => self.fav_albums.items
                        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                    SortField::LastAdded => self.fav_albums.items
                        .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
                    SortField::ByArtist => self.fav_albums.items
                        .sort_by(|a, b| a.artist_name().to_lowercase().cmp(&b.artist_name().to_lowercase())),
                }
            }
            Tab::Playlists => {
                self.playlists_sort = Some(field);
                match field {
                    SortField::Alphabetical => self.playlists.items
                        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                    SortField::LastAdded => self.playlists.items
                        .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
                    _ => {}
                }
            }
            Tab::Search => {}
        }
    }
}
