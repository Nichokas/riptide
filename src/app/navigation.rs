// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use super::{
    AlbumDetail, App, ArtistDetail, ArtistDetailFocus, HomeSection, HomeSectionFocus,
    PlaylistDetail, PlaylistDetailFocus, StatefulList, Tab, View,
};
use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist};

impl App {
    // ── Tab switching ─────────────────────────────────────────────────────────

    pub fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Home => Tab::Favorites,
            Tab::Favorites => Tab::Artists,
            Tab::Artists => Tab::Albums,
            Tab::Albums => Tab::Playlists,
            Tab::Playlists => Tab::Search,
            Tab::Search => Tab::Home,
        };
        self.view_stack.clear();
        self.on_tab_entered();
    }

    pub fn prev_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Home => Tab::Search,
            Tab::Favorites => Tab::Home,
            Tab::Artists => Tab::Favorites,
            Tab::Albums => Tab::Artists,
            Tab::Playlists => Tab::Albums,
            Tab::Search => Tab::Playlists,
        };
        self.view_stack.clear();
        self.on_tab_entered();
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
        self.view_stack.clear();
        self.on_tab_entered();
    }

    /// Landing on Search drops straight into the query box, but only when there
    /// are no results to come back to. Otherwise the previous results stay
    /// browsable and `/` reopens the box. The query is deliberately left intact
    /// so tabbing away and back doesn't discard what was typed; `/` is the
    /// explicit "start a new search" action and clears it.
    fn on_tab_entered(&mut self) {
        // Switching tabs means the user wants the new tab's content, so release
        // the queue — otherwise it keeps capturing the arrow keys from behind.
        self.queue_focused = false;

        if self.current_tab == Tab::Search && !self.search.has_results() {
            self.search.modal_open = true;
        }

        if self.current_tab == Tab::Home {
            self.sync_home_art();
        }
    }

    // ── View stack ────────────────────────────────────────────────────────────

    pub fn go_back(&mut self) {
        if !self.view_stack.is_empty() {
            self.view_stack.pop();
        }
    }

    // ── Opening views ─────────────────────────────────────────────────────────

    pub fn open_selected_artist(&mut self) {
        let Some(artist) = self.artists.selected_item().cloned() else {
            return;
        };
        self.open_artist(artist);
    }

    pub fn open_artist(&mut self, artist: Artist) {
        tracing::debug!(
            "Fetching details for artist {} [{}]",
            artist.name,
            artist.id
        );
        let id = artist.id;
        let mut tracks = StatefulList::default();
        tracks.loading = true;
        let detail = ArtistDetail {
            artist,
            tracks,
            albums: StatefulList::default(),
            eps: StatefulList::default(),
            singles: StatefulList::default(),
            focus: ArtistDetailFocus::Tracks,
            art_bytes: None,
            art_loading: false,
            bio: None,
            bio_loading: true,
            bio_scroll: 0,
        };
        self.view_stack.push(View::ArtistDetail(detail));
        let _ = self
            .api_tx
            .send(ApiRequest::LoadArtistTopTracks { artist_id: id });
        let _ = self
            .api_tx
            .send(ApiRequest::LoadArtistBio { artist_id: id });
        let _ = self
            .api_tx
            .send(ApiRequest::LoadArtistPicture { artist_id: id });
    }

    pub fn open_album(&mut self, album: Album) {
        let album_id = album.id;
        self.view_stack.push(View::AlbumDetail(AlbumDetail {
            album,
            tracks: StatefulList::default(),
            art_bytes: None,
            art_loading: true,
        }));
        let _ = self.api_tx.send(ApiRequest::LoadAlbum { album_id });
        let _ = self.api_tx.send(ApiRequest::LoadAlbumTracks { album_id });
        // Don't fetch cover here - let AlbumLoaded handler fetch with the correct cover ID
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
        if let Some(album) = album {
            self.open_album(album);
        }
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
        // Show spinner for mixes (from Home tab) even if cover isn't loaded yet
        let is_home_mix = self.current_tab == Tab::Home;
        let has_cover = playlist.cover.is_some();
        let art_loading = has_cover || is_home_mix;
        let detail = PlaylistDetail {
            playlist: playlist.clone(),
            tracks,
            focus: PlaylistDetailFocus::Tracks,
            art_bytes: None,
            art_loading,
            description_scroll: 0,
        };
        self.view_stack.push(View::PlaylistDetail(detail));
        // Use v2 API for mixes from Home tab, v1 for regular playlists
        if self.current_tab == Tab::Home {
            let _ = self.api_tx.send(ApiRequest::LoadMixTracks { uuid });
        } else {
            let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks {
                uuid,
                next_url: None,
            });
        }
        if let Some(cover_url) = playlist.cover {
            let uuid_for_art = playlist.uuid.clone();
            let _ = self.api_tx.send(ApiRequest::FetchPlaylistArt {
                uuid: uuid_for_art,
                cover_url,
            });
        }
    }

    pub fn open_selected_playlist(&mut self) {
        let Some(playlist) = self.playlists.selected_item().cloned() else {
            return;
        };
        self.open_playlist(playlist);
    }

    pub fn open_selected_home_item(&mut self) {
        if let Some(playlist) = self.selected_home_mix().cloned() {
            self.open_playlist(playlist);
        }
    }

    // ── Home tab ──────────────────────────────────────────────────────────────

    pub fn home_section(&self) -> &HomeSection<Playlist> {
        match self.home_section_focus {
            HomeSectionFocus::NewReleases => &self.home_new_releases,
            HomeSectionFocus::DailyMixes => &self.home_daily_mixes,
            HomeSectionFocus::DiscoveryMixes => &self.home_discovery_mixes,
        }
    }

    pub fn selected_home_mix(&self) -> Option<&Playlist> {
        self.home_section().selected_item()
    }

    pub fn is_home_mix(&self, uuid: &str) -> bool {
        [
            &self.home_new_releases,
            &self.home_daily_mixes,
            &self.home_discovery_mixes,
        ]
        .iter()
        .any(|section| section.items.iter().any(|mix| mix.uuid == uuid))
    }

    pub fn home_next(&mut self) {
        match self.home_section_focus {
            HomeSectionFocus::NewReleases => self.home_new_releases.next(),
            HomeSectionFocus::DailyMixes => self.home_daily_mixes.next(),
            HomeSectionFocus::DiscoveryMixes => self.home_discovery_mixes.next(),
        }
        self.sync_home_art();
    }

    pub fn home_prev(&mut self) {
        match self.home_section_focus {
            HomeSectionFocus::NewReleases => self.home_new_releases.prev(),
            HomeSectionFocus::DailyMixes => self.home_daily_mixes.prev(),
            HomeSectionFocus::DiscoveryMixes => self.home_discovery_mixes.prev(),
        }
        self.sync_home_art();
    }

    /// Move to the section on the right, or off the end of the carousel into the
    /// queue — the same thing `l` does past the last artist detail pane.
    pub fn home_section_next(&mut self) {
        self.home_section_focus = match self.home_section_focus {
            HomeSectionFocus::NewReleases => HomeSectionFocus::DailyMixes,
            HomeSectionFocus::DailyMixes => HomeSectionFocus::DiscoveryMixes,
            HomeSectionFocus::DiscoveryMixes => return self.focus_queue(),
        };
        self.sync_home_art();
    }

    pub fn home_section_prev(&mut self) {
        self.home_section_focus = match self.home_section_focus {
            HomeSectionFocus::NewReleases => HomeSectionFocus::DiscoveryMixes,
            HomeSectionFocus::DailyMixes => HomeSectionFocus::NewReleases,
            HomeSectionFocus::DiscoveryMixes => HomeSectionFocus::DailyMixes,
        };
        self.sync_home_art();
    }

    /// Bring the cover art in step with the selected mix. Cheap and idempotent,
    /// so anything that can change that selection may just call it.
    pub fn sync_home_art(&mut self) {
        let Some((uuid, cover_url)) = self
            .selected_home_mix()
            .map(|mix| (mix.uuid.clone(), mix.cover.clone()))
        else {
            self.home_art.clear();
            return;
        };
        if self.home_art.uuid.as_deref() == Some(uuid.as_str()) {
            return;
        }
        let Some(cover_url) = cover_url else {
            self.home_art.clear();
            self.home_art.uuid = Some(uuid);
            return;
        };
        if self.home_art.select(&uuid) {
            let _ = self
                .api_tx
                .send(ApiRequest::FetchPlaylistArt { uuid, cover_url });
        }
    }

    pub fn go_to_artist_from_track(&mut self, track: &crate::api::models::Track) {
        let artist_names: Vec<String> = if !track.artists.is_empty() {
            track.artists.iter().map(|a| a.name.clone()).collect()
        } else if let Some(ref artist) = track.artist {
            vec![artist.name.clone()]
        } else {
            Vec::new()
        };

        if artist_names.is_empty() {
            self.set_status(
                "No artist information available".to_string(),
                crate::app::StatusLevel::Error,
            );
            return;
        }

        if artist_names.len() == 1 {
            let name = artist_names[0].clone();
            self.artist_selection.searching_for = Some(name.clone());
            let _ = self
                .api_tx
                .send(ApiRequest::SearchArtistByName { query: name });
        } else {
            self.artist_selection.artist_names = artist_names;
            self.artist_selection.selected = 0;
            self.artist_selection.active = true;
        }
    }

    pub fn open_selected_artist_from_selection(&mut self) {
        if let Some(name) = self
            .artist_selection
            .artist_names
            .get(self.artist_selection.selected)
            .cloned()
        {
            self.artist_selection.active = false;
            self.artist_selection.searching_for = Some(name.clone());
            self.artist_selection.artist_names.clear();
            let _ = self
                .api_tx
                .send(ApiRequest::SearchArtistByName { query: name });
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::{TestApp, test_app};

    fn mix(n: usize) -> Playlist {
        Playlist {
            uuid: format!("uuid-{n}"),
            title: format!("My Mix {n}"),
            number_of_tracks: None,
            description: None,
            cover: Some(format!("https://example.invalid/{n}.jpg")),
            added_at: None,
        }
    }

    fn home_app() -> TestApp {
        let mut t = test_app();
        t.app.home_new_releases.items = vec![mix(0)];
        t.app.home_daily_mixes.items = (1..=3).map(mix).collect();
        t.app.home_discovery_mixes.items = vec![mix(9)];
        t.drain_api();
        t
    }

    fn art_requests(t: &mut TestApp) -> Vec<String> {
        t.api_requests()
            .into_iter()
            .filter_map(|req| match req {
                ApiRequest::FetchPlaylistArt { uuid, .. } => Some(uuid),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_last_section_hands_off_to_the_queue() {
        let mut t = home_app();
        t.app.now_playing.queue = vec![crate::app::test_support::track(1)];

        t.app.home_section_next();
        assert_eq!(t.app.home_section_focus, HomeSectionFocus::DailyMixes);
        t.app.home_section_next();
        assert_eq!(t.app.home_section_focus, HomeSectionFocus::DiscoveryMixes);

        t.app.home_section_next();
        assert!(
            t.app.queue_focused,
            "past the last section `l` reaches the queue"
        );
        assert_eq!(
            t.app.home_section_focus,
            HomeSectionFocus::DiscoveryMixes,
            "and does not wrap back to the first"
        );
    }

    /// Selection changes on every keypress, so a cover already fetched must not
    /// be requested again on the way back up the list.
    #[test]
    fn covers_are_fetched_once_per_mix() {
        let mut t = home_app();
        t.app.home_section_focus = HomeSectionFocus::DailyMixes;
        t.app.sync_home_art();
        assert_eq!(art_requests(&mut t), vec!["uuid-1"]);

        t.app.home_next();
        assert_eq!(art_requests(&mut t), vec!["uuid-2"]);

        t.app.home_art.store("uuid-1".to_string(), b"one".to_vec());
        t.app.home_art.store("uuid-2".to_string(), b"two".to_vec());

        t.app.home_prev();
        t.app.home_next();
        assert!(
            art_requests(&mut t).is_empty(),
            "both covers had already arrived"
        );
        assert_eq!(t.app.home_art.bytes.as_deref(), Some(&b"two"[..]));
    }
}
