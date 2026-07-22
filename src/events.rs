// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::{ApiRequest, ApiResponse};
use crate::app::{App, ArtistDetailFocus, SearchPane, SortPalette, Tab, View};
use crate::mpris::MprisCmd;
use crate::player::{PlayerCmd, PlayerEvent};

pub fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut api_rx: mpsc::UnboundedReceiver<ApiResponse>,
    mut player_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    mut mpris_rx: mpsc::UnboundedReceiver<MprisCmd>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| crate::ui::draw(f, app))?;

        // Drain API responses
        while let Ok(resp) = api_rx.try_recv() {
            app.handle_api_response(resp);
        }

        // Drain player events
        while let Ok(evt) = player_rx.try_recv() {
            app.handle_player_event(evt);
        }

        // Drain MPRIS control commands
        while let Ok(cmd) = mpris_rx.try_recv() {
            match cmd {
                MprisCmd::Next => app.next_track(),
                MprisCmd::Previous => app.prev_track(),
                MprisCmd::Play => app.set_paused(false),
                MprisCmd::Pause => app.set_paused(true),
                MprisCmd::PlayPause => app.toggle_pause(),
                MprisCmd::Stop => { let _ = app.player_tx.send(PlayerCmd::Stop); }
            }
        }

        // Check for more data to load
        check_load_more(app);

        app.tick();

        if app.should_quit {
            break;
        }

        // Poll for key events with a short timeout to keep animations smooth
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key);
            }
        }
    }
    Ok(())
}

fn kitty_delete_album_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=2\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

fn kitty_delete_artist_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=3\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

fn leaving_album(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::AlbumDetail(_)))
}

fn leaving_artist(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::ArtistDetail(_)))
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if app.help_active {
        handle_help_input(app, key);
        return;
    }

    if app.queue_focused {
        handle_queue_input(app, key);
        return;
    }

    if app.command.active {
        handle_command_input(app, key);
        return;
    }

    if app.sort_palette.active {
        handle_sort_palette_input(app, key);
        return;
    }

    // Search overlay captures all keys while active, regardless of current tab.
    if app.search.active {
        handle_search_input(app, key);
        return;
    }

    // Global bindings
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.help_active = true;
            app.help_scroll = 0;
        }
        KeyCode::Char('/') => {
            app.command.active = true;
            app.command.input.clear();
            app.command.selected = 0;
        }
        KeyCode::Tab => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.next_tab();
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('n') => app.next_track(),
        KeyCode::Char('p') => app.prev_track(),
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Esc => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.go_back();
        }
        KeyCode::Char('+') | KeyCode::Char('=') => { let _ = app.player_tx.send(PlayerCmd::ChangeVolume(5)); },
        KeyCode::Char('-') => { let _ = app.player_tx.send(PlayerCmd::ChangeVolume(-5)); },
        _ => handle_navigation(app, key),
    }
}

fn handle_command_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.command.active = false;
        }
        KeyCode::Enter => {
            let matches = app.command.matches();
            let cmd = matches.get(app.command.selected)
                .or_else(|| matches.first())
                .copied();
            if let Some(cmd) = cmd {
                execute_command(app, cmd);
            } else {
                app.command.active = false;
            }
        }
        KeyCode::Tab => {
            // Accept ghost-text completion.
            if let Some(&first) = app.command.matches().first() {
                app.command.input = first.to_string();
                app.command.selected = 0;
            }
        }
        KeyCode::Up => {
            if app.command.selected > 0 {
                app.command.selected -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.command.matches().len();
            if app.command.selected + 1 < len {
                app.command.selected += 1;
            }
        }
        KeyCode::Backspace => {
            app.command.input.pop();
            app.command.selected = 0;
        }
        KeyCode::Char(c) => {
            app.command.input.push(c);
            app.command.selected = 0;
        }
        _ => {}
    }
}

fn execute_command(app: &mut App, cmd: &str) {
    app.command.active = false;
    app.command.input.clear();
    let cleanup = |app: &App| {
        if leaving_album(app) { kitty_delete_album_art(); }
        if leaving_artist(app) { kitty_delete_artist_art(); }
    };
    match cmd {
        "favorites" => {
            cleanup(app);
            app.set_tab(Tab::Favorites);
        }
        "artists" => {
            cleanup(app);
            app.set_tab(Tab::Artists);
        }
        "albums" => {
            cleanup(app);
            app.set_tab(Tab::Albums);
        }
        "playlists" => {
            cleanup(app);
            app.set_tab(Tab::Playlists);
        }
        "search" => {
            cleanup(app);
            app.set_tab(Tab::Search);
        }
        _ => {}
    }
}

fn handle_sort_palette_input(app: &mut App, key: KeyEvent) {
    let options = SortPalette::get_options(app.current_tab);
    let count = options.len();
    match key.code {
        KeyCode::Esc => {
            app.sort_palette.active = false;
        }
        KeyCode::Up => {
            if app.sort_palette.selected > 0 {
                app.sort_palette.selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.sort_palette.selected + 1 < count {
                app.sort_palette.selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some((_, field)) = options.get(app.sort_palette.selected){
                app.apply_sort(*field);
            }
        }
        _ => {}
    }
}

fn handle_help_input(app: &mut App, key: KeyEvent) {
    use crate::app::KeybindGroup;

    let max_scroll = {
        let total_lines = KeybindGroup::total_help_lines() as i16;
        let visible_lines = 22i16; // Modal inner height (24 - 2 for borders) in render_help_modal
        (total_lines - visible_lines).max(0) as u16
    };

    match key.code {
        KeyCode::Esc => {
            app.help_active = false;
            app.help_scroll = 0;
        }
        KeyCode::Up => {
            app.help_scroll = app.help_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            app.help_scroll = (app.help_scroll + 1).min(max_scroll);
        }
        KeyCode::PageUp => {
            app.help_scroll = app.help_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.help_scroll = (app.help_scroll + 10).min(max_scroll);
        }
        _ => {}
    }
}

fn handle_navigation(app: &mut App, key: KeyEvent) {
    // First pass: mutate the view's own list state (navigation within a detail view).
    // Collect any "play" action as data so we can call app methods after the borrow ends.
    enum Action {
        None,
        PlayTracks(Vec<crate::api::models::Track>, usize),
        OpenAlbum,
        AddToQueue(crate::api::models::Track),
        ToggleFavoriteTrack(crate::api::models::Track),
        ToggleFollowArtist(crate::api::models::Artist),
        ToggleFavoriteAlbum(crate::api::models::Album),
        TrackRadio(crate::api::models::Track),
        ArtistRadio(crate::api::models::Artist),
        FocusQueue,
        PlayPlaylistTracks(Vec<crate::api::models::Track>, usize, String),
        CopyUrl(String),
    }

    let action: Action = if let Some(view) = app.view_stack.last_mut() {
        match view {
            View::ArtistDetail(detail) => {
                match key.code {
                    KeyCode::Up => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.prev(),
                            ArtistDetailFocus::Albums => detail.albums.prev(),
                            ArtistDetailFocus::Bio => {
                                detail.bio_scroll = detail.bio_scroll.saturating_sub(1);
                            }
                        }
                        return;
                    }
                    KeyCode::Down => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.next(),
                            ArtistDetailFocus::Albums => detail.albums.next(),
                            ArtistDetailFocus::Bio => {
                                detail.bio_scroll = detail.bio_scroll.saturating_add(1);
                            }
                        }
                        return;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        detail.focus = match detail.focus {
                            ArtistDetailFocus::Albums => ArtistDetailFocus::Tracks,
                            ArtistDetailFocus::Tracks => ArtistDetailFocus::Bio,
                            ArtistDetailFocus::Bio    => ArtistDetailFocus::Bio,
                        };
                        return;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if detail.focus == ArtistDetailFocus::Albums {
                            Action::FocusQueue
                        } else {
                            detail.focus = match detail.focus {
                                ArtistDetailFocus::Bio    => ArtistDetailFocus::Tracks,
                                ArtistDetailFocus::Tracks => ArtistDetailFocus::Albums,
                                ArtistDetailFocus::Albums => unreachable!(),
                            };
                            return;
                        }
                    }
                    KeyCode::Enter => {
                        if detail.focus == ArtistDetailFocus::Tracks {
                            let idx = detail.tracks.selected;
                            let tracks = detail.tracks.items.clone();
                            Action::PlayTracks(tracks, idx)
                        } else if detail.focus == ArtistDetailFocus::Albums {
                            Action::OpenAlbum
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('a') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::ToggleFavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::Albums => {
                        match detail.albums.items.get(detail.albums.selected).cloned() {
                            Some(a) => Action::ToggleFavoriteAlbum(a),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => Action::ToggleFollowArtist(detail.artist.clone()),
                    KeyCode::Char('r') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => Action::ArtistRadio(detail.artist.clone()),
                    KeyCode::Char('c') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') if detail.focus == ArtistDetailFocus::Albums => {
                        match detail.albums.items.get(detail.albums.selected) {
                            Some(a) => Action::CopyUrl(a.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') => Action::CopyUrl(detail.artist.share_url()),
                    KeyCode::Char('C') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.album.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('C') if detail.focus == ArtistDetailFocus::Albums => {
                        Action::CopyUrl(detail.artist.share_url())
                    }
                    _ => return,
                }
            }
            View::PlaylistDetail(detail) => {
                match key.code {
                    KeyCode::Up => { detail.tracks.prev(); return; }
                    KeyCode::Down => { detail.tracks.next(); return; }
                    KeyCode::Right | KeyCode::Char('l') => Action::FocusQueue,
                    KeyCode::Enter => {
                        let idx = detail.tracks.selected;
                        let tracks = detail.tracks.items.clone();
                        let uuid = detail.playlist.uuid.clone();
                        Action::PlayPlaylistTracks(tracks, idx, uuid)
                    }
                    KeyCode::Char('a') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::ToggleFavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('C') => Action::CopyUrl(detail.playlist.share_url()),
                    _ => return,
                }
            }
            View::AlbumDetail(detail) => {
                match key.code {
                    KeyCode::Up => { detail.tracks.prev(); return; }
                    KeyCode::Down => { detail.tracks.next(); return; }
                    KeyCode::Right | KeyCode::Char('l') => Action::FocusQueue,
                    KeyCode::Enter => {
                        let idx = detail.tracks.selected;
                        let tracks = detail.tracks.items.clone();
                        Action::PlayTracks(tracks, idx)
                    }
                    KeyCode::Char('a') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::ToggleFavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('C') => Action::CopyUrl(detail.album.share_url()),
                    _ => return,
                }
            }
        }
    } else {
        Action::None
    };

    // Apply any collected action (borrow of view_stack has ended)
    match action {
        Action::PlayTracks(tracks, idx) => { app.play_tracks(tracks, idx); return; }
        Action::OpenAlbum => { kitty_delete_album_art(); kitty_delete_artist_art(); app.reset_artist_art_placed(); app.open_selected_album(); return; }
        Action::AddToQueue(track) => { app.add_to_queue(track); return; }
        Action::ToggleFavoriteTrack(track) => { app.toggle_favorite_track(&track); return; }
        Action::ToggleFollowArtist(artist) => { app.toggle_follow_artist(&artist); return; }
        Action::ToggleFavoriteAlbum(album) => { app.toggle_favorite_album(&album); return; }
        Action::TrackRadio(track) => { app.start_track_radio(&track); return; }
        Action::ArtistRadio(artist) => { app.start_artist_radio(&artist); return; }
        Action::FocusQueue => { app.focus_queue(); return; }
        Action::PlayPlaylistTracks(tracks, idx, uuid) => { app.play_playlist_tracks(tracks, idx, uuid); return; }
        Action::CopyUrl(url) => { app.copy_url(url); return; }
        Action::None => {}
    }

    // Top-level tab navigation (no active detail view)
    match key.code {
        KeyCode::Up => match app.current_tab {
            Tab::Artists => app.artists.prev(),
            Tab::Albums => app.fav_albums.prev(),
            Tab::Playlists => app.playlists.prev(),
            Tab::Favorites => app.favorites.prev(),
            Tab::Search => app.search.pane_prev(),
        },
        KeyCode::Down => match app.current_tab {
            Tab::Artists => app.artists.next(),
            Tab::Albums => app.fav_albums.next(),
            Tab::Playlists => app.playlists.next(),
            Tab::Favorites => app.favorites.next(),
            Tab::Search => app.search.pane_next(),
        },
        KeyCode::Left | KeyCode::Char('h') if app.current_tab == Tab::Search => {
            app.search.prev_pane();
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab == Tab::Search => {
            app.search.next_pane();
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab != Tab::Search => {
            app.focus_queue();
        }
        KeyCode::Enter => match app.current_tab {
            Tab::Artists => app.open_selected_artist(),
            Tab::Albums => app.open_selected_fav_album(),
            Tab::Playlists => app.open_selected_playlist(),
            Tab::Favorites => {
                let idx = app.favorites.selected;
                let tracks = app.favorites.items.clone();
                if !tracks.is_empty() {
                    app.play_tracks(tracks, idx);
                }
            }
            Tab::Search => {
                match app.search.pane {
                    SearchPane::Tracks => {
                        let idx = app.search.track_sel;
                        if let Some(track) = app.search.tracks.get(idx).cloned() {
                            app.play_track(track);
                        }
                    }
                    SearchPane::Artists => {
                        let idx = app.search.artist_sel;
                        if let Some(artist) = app.search.artists.get(idx).cloned() {
                            app.open_artist(artist);
                        }
                    }
                    SearchPane::Playlists => {
                        let idx = app.search.playlist_sel;
                        if let Some(playlist) = app.search.playlists.get(idx).cloned() {
                            app.open_playlist(playlist);
                        }
                    }
                }
            }
        },
        KeyCode::Char('a') => match app.current_tab {
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.add_to_queue(track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.add_to_queue(track);
                }
            }
            _ => {}
        },
        KeyCode::Char('f') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.toggle_follow_artist(&artist);
                }
            }
            Tab::Playlists => {
                if let Some(playlist) = app.playlists.selected_item().cloned() {
                    app.toggle_save_playlist(&playlist);
                }
            }
            Tab::Albums => {
                if let Some(album) = app.fav_albums.selected_item().cloned() {
                    app.toggle_favorite_album(&album);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.toggle_favorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.toggle_favorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.toggle_follow_artist(&artist);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Playlists => {
                if let Some(playlist) = app.search.playlists.get(app.search.playlist_sel).cloned() {
                    app.toggle_save_playlist(&playlist);
                }
            }
            _ => {}
        },
        KeyCode::Char('c') => match app.current_tab {
            Tab::Artists => {
                if let Some(url) = app.artists.selected_item().map(|a| a.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Albums => {
                if let Some(url) = app.fav_albums.selected_item().map(|a| a.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Playlists => {
                if let Some(url) = app.playlists.selected_item().map(|p| p.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Favorites => {
                if let Some(url) = app.favorites.selected_item().map(|t| t.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(url) = app.search.tracks.get(app.search.track_sel).map(|t| t.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(url) = app.search.artists.get(app.search.artist_sel).map(|a| a.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Playlists => {
                if let Some(url) = app.search.playlists.get(app.search.playlist_sel).map(|p| p.share_url()) {
                    app.copy_url(url);
                }
            }
            _ => {}
        },
        // Shift+C copies the *parent* of the selected item. Only tracks have a
        // reachable parent (their album); artist/album/playlist rows have no
        // parent id in the API models, so they no-op.
        KeyCode::Char('C') => match app.current_tab {
            Tab::Favorites => {
                if let Some(url) = app.favorites.selected_item().map(|t| t.album.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(url) = app.search.tracks.get(app.search.track_sel).map(|t| t.album.share_url()) {
                    app.copy_url(url);
                }
            }
            _ => {}
        },
        KeyCode::Char('s') => match app.current_tab {
            Tab::Favorites | Tab::Artists | Tab::Albums | Tab::Playlists => {
                if app.view_stack.is_empty() {
                    app.open_sort_palette();
                }
            }
            _ => {}
        },
        KeyCode::Char('r') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.start_artist_radio(&artist);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.start_track_radio(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.start_track_radio(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.start_artist_radio(&artist);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => {
            app.search.active = false;
            app.next_tab();
        }
        KeyCode::Esc => {
            // Close overlay, stay on current view.
            app.search.active = false;
        }
        KeyCode::Enter => {
            let query = app.search.query.clone();
            app.search.active = false;
            if !query.is_empty() {
                // Now we're committing to showing search results — navigate away.
                if leaving_album(app) { kitty_delete_album_art(); }
                app.view_stack.clear();
                app.current_tab = Tab::Search;
                app.search.loading = true;
                let _ = app.api_tx.send(ApiRequest::Search { query });
            }
        }
        KeyCode::Backspace => {
            app.search.query.pop();
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
        }
        _ => {}
    }
}

fn handle_queue_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.unfocus_queue();
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_queue_track_up();
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_queue_track_down();
        }
        KeyCode::Up => {
            if app.queue_cursor > 0 {
                app.queue_cursor -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.now_playing.queue.len();
            if len > 0 && app.queue_cursor + 1 < len {
                app.queue_cursor += 1;
            }
        }
        KeyCode::Enter => {
            let cursor = app.queue_cursor;
            app.play_from_queue(cursor);
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let cursor = app.queue_cursor;
            app.remove_from_queue(cursor);
        }
        KeyCode::Char('f') => {
            if let Some(track) = app.now_playing.queue.get(app.queue_cursor).cloned() {
                app.toggle_favorite_track(&track);
            }
        }
        KeyCode::Char('c') => {
            if let Some(url) = app.now_playing.queue.get(app.queue_cursor).map(|t| t.share_url()) {
                app.copy_url(url);
            }
        }
        KeyCode::Char('C') => {
            if let Some(url) = app.now_playing.queue.get(app.queue_cursor).map(|t| t.album.share_url()) {
                app.copy_url(url);
            }
        }
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        _ => {}
    }
}

fn check_load_more(app: &mut App) {
    // Playlist detail tracks — checked before tab-level lists so the guard below
    // (`view_stack.is_empty()`) doesn't shadow it.
    if let Some(View::PlaylistDetail(detail)) = app.view_stack.last() {
        if detail.tracks.should_load_more() {
            app.load_more_playlist_tracks();
            return;
        }
    }

    match app.current_tab {
        Tab::Artists if app.view_stack.is_empty() => {
            if app.artists.should_load_more() {
                app.load_artists();
            }
        }
        Tab::Albums if app.view_stack.is_empty() => {
            if app.fav_albums.should_load_more() {
                app.load_fav_albums();
            }
        }
        Tab::Playlists if app.view_stack.is_empty() => {
            if app.playlists.should_load_more() {
                app.load_playlists();
            }
        }
        Tab::Favorites if app.view_stack.is_empty() => {
            if app.favorites.should_load_more() {
                app.load_favorites();
            }
        }
        _ => {}
    }
}
