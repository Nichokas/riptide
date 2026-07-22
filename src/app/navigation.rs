// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist};
use super::{App, ArtistDetail, ArtistDetailFocus, AlbumDetail, PlaylistDetail, StatefulList, Tab, View};

impl App {
    // ── Tab switching ─────────────────────────────────────────────────────────

    pub fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Favorites => Tab::Artists,
            Tab::Artists   => Tab::Albums,
            Tab::Albums    => Tab::Playlists,
            Tab::Playlists => Tab::Search,
            Tab::Search    => Tab::Favorites,
        };
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.active = true;
            self.search.query.clear();
        }
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.active = true;
            self.search.query.clear();
        }
    }

    // ── View stack ────────────────────────────────────────────────────────────

    pub fn go_back(&mut self) {
        if self.search.active {
            self.search.active = false;
            return;
        }
        if !self.view_stack.is_empty() {
            self.view_stack.pop();
        }
    }

    /// Reset Kitty placement state on the current ArtistDetail so art is re-placed after returning.
    pub fn reset_artist_art_placed(&mut self) {
        if let Some(View::ArtistDetail(detail)) = self.view_stack.last() {
            *detail.art_placed.borrow_mut() = None;
        }
    }

    // ── Opening views ─────────────────────────────────────────────────────────

    pub fn open_selected_artist(&mut self) {
        let Some(artist) = self.artists.selected_item().cloned() else { return };
        self.open_artist(artist);
    }

    pub fn open_artist(&mut self, artist: Artist) {
        let id = artist.id;
        let picture_id = artist.picture.clone();
        let has_picture = picture_id.is_some();
        let detail = ArtistDetail {
            artist,
            tracks: StatefulList::default(),
            albums: StatefulList::default(),
            eps: StatefulList::default(),
            singles: StatefulList::default(),
            focus: ArtistDetailFocus::Tracks,
            art_bytes: None,
            art_loading: has_picture,
            art_cache: std::cell::RefCell::new(None),
            art_placed: std::cell::RefCell::new(None),
            bio: None,
            bio_loading: true,
            bio_scroll: 0,
        };
        self.view_stack.push(View::ArtistDetail(detail));
        let _ = self.api_tx.send(ApiRequest::LoadArtistTopTracks { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistAlbums   { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistEPs      { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistSingles  { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistBio      { artist_id: id });
        if let Some(picture_id) = picture_id {
            let _ = self.api_tx.send(ApiRequest::FetchArtistArt { artist_id: id, picture_id });
        }
    }

    pub fn open_album(&mut self, album: Album) {
        let album_id = album.id;
        let cover = album.cover.clone();
        let has_cover = cover.is_some();
        self.view_stack.push(View::AlbumDetail(AlbumDetail {
            album,
            tracks: StatefulList::default(),
            art_bytes: None,
            art_loading: has_cover,
            art_cache: std::cell::RefCell::new(None),
            art_placed: std::cell::RefCell::new(None),
        }));
        let _ = self.api_tx.send(ApiRequest::LoadAlbum       { album_id });
        let _ = self.api_tx.send(ApiRequest::LoadAlbumTracks { album_id });
        if let Some(cover_id) = cover {
            let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id });
        }
    }

    pub fn open_selected_album(&mut self) {
        let album = if let Some(View::ArtistDetail(detail)) = self.view_stack.last() {
            match detail.focus {
                ArtistDetailFocus::Albums => detail.albums.selected_item().cloned(),
                ArtistDetailFocus::EPs => detail.eps.selected_item().cloned(),
                ArtistDetailFocus::Singles => detail.singles.selected_item().cloned(),
                _ => None,
            }
        } else {
            None
        };
        if let Some(album) = album { self.open_album(album); }
    }

    pub fn open_selected_fav_album(&mut self) {
        if let Some(album) = self.fav_albums.selected_item().cloned() {
            self.open_album(album);
        }
    }

    pub fn open_playlist(&mut self, playlist: Playlist) {
        let uuid = playlist.uuid.clone();
        let mut tracks: StatefulList<crate::api::models::Track> = StatefulList::default();
        tracks.loading = true;
        let detail = PlaylistDetail { playlist, tracks };
        self.view_stack.push(View::PlaylistDetail(detail));
        let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks { uuid, offset: 0 });
    }

    pub fn open_selected_playlist(&mut self) {
        let Some(playlist) = self.playlists.selected_item().cloned() else { return };
        self.open_playlist(playlist);
    }
}
