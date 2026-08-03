// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use super::{App, View};

impl App {
    pub fn load_artists(&mut self) {
        if self.artists.loading || self.artists.exhausted { return; }
        self.artists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadArtists { offset: self.artists.next_offset });
    }

    pub fn load_fav_albums(&mut self) {
        if self.fav_albums.loading || self.fav_albums.exhausted { return; }
        self.fav_albums.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadFavAlbums { offset: self.fav_albums.next_offset });
    }

    pub fn load_playlists(&mut self) {
        if self.playlists.loading || self.playlists.exhausted { return; }
        self.playlists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadPlaylists { offset: self.playlists.next_offset });
    }

    pub fn load_favorites(&mut self) {
        if self.favorites.loading || self.favorites.exhausted { return; }
        self.favorites.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadFavorites { offset: self.favorites.next_offset });
    }

    pub fn load_more_playlist_tracks(&mut self) {
        if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
            if !detail.tracks.loading && !detail.tracks.exhausted {
                let uuid = detail.playlist.uuid.clone();
                let offset = detail.tracks.next_offset;
                detail.tracks.loading = true;
                let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks { uuid, offset });
            }
        }
    }

    pub(crate) fn fetch_now_playing_metadata(&mut self) {
        self.fetch_now_playing_art();
        self.fetch_lyrics();
    }

    fn fetch_now_playing_art(&mut self) {
        let (album_id, cover_id) = match &self.now_playing.track {
            Some(t) => (t.album.id, t.album.cover.clone()),
            None => return,
        };
        self.now_playing.art_bytes = None;
        if let Some(cover_id) = cover_id {
            self.now_playing.art_loading = true;
            let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id });
        } else {
            self.now_playing.art_loading = false;
        }
    }

    fn fetch_lyrics(&mut self) {
        let Some(track) = &self.now_playing.track else { return };
        let track_id = track.id;
        self.now_playing.lyrics_synced = Vec::new();
        self.now_playing.lyrics_plain = Vec::new();
        self.now_playing.lyrics_loading = true;
        let _ = self.api_tx.send(ApiRequest::FetchLyrics { track_id });
    }
}
