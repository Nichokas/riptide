// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Input handling and the main event loop.
//!
//! [`run_app`] drives the draw/poll cycle. Key dispatch in `handle_key` takes
//! the text boxes first, then works through the remaining contexts that capture
//! input — help, the queue, the pickers — before falling through to the global
//! bindings and list navigation.

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::ApiResponse;
use crate::app::{App, Tab};
use crate::mpris::MprisCmd;
use crate::player::{PlayerCmd, PlayerEvent};

mod filter;
mod global;
mod navigation;
mod overlays;
mod queue;
mod search;

use filter::*;
use global::*;
use navigation::*;
use overlays::*;
use queue::*;
use search::*;

pub fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut api_rx: mpsc::UnboundedReceiver<ApiResponse>,
    mut player_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    mut mpris_rx: mpsc::UnboundedReceiver<MprisCmd>,
    lastfm_evt_tx: mpsc::UnboundedSender<PlayerEvent>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| crate::ui::draw(f, app))?;

        // Drain API responses
        while let Ok(resp) = api_rx.try_recv() {
            app.handle_api_response(resp);
        }

        // Drain player events and forward to Last.fm
        while let Ok(evt) = player_rx.try_recv() {
            let _ = lastfm_evt_tx.send(evt.clone());
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
                MprisCmd::Stop => {
                    let _ = app.player_tx.send(PlayerCmd::Stop);
                }
            }
        }

        app.tick();

        if app.should_quit {
            break;
        }

        // Poll for key events with a short timeout to keep animations smooth
        // Drain all pending key events and only process the last one to avoid lag from key repeat
        let mut last_key_event: Option<KeyEvent> = None;
        while event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                last_key_event = Some(key);
            }
        }
        if let Some(key) = last_key_event {
            handle_key(app, key);
        }

        // Small delay to keep animations smooth
        if !event::poll(Duration::from_millis(16))? {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // A text box outranks every other context, because to it a keystroke is a
    // character and nothing else may claim it first. The queue used to be
    // checked ahead of the command palette and swallowed everything typed into
    // a palette opened from it, which made `:` in there a dead end.
    if app.command.active {
        handle_command_input(app, key);
        return;
    }

    if app.filter_active {
        handle_filter_input(app, key);
        return;
    }

    // The search box captures all keys while open, regardless of current tab.
    if app.search.modal_open {
        handle_search_input(app, key);
        return;
    }

    if app.help_active {
        handle_help_input(app, key);
        return;
    }

    if app.queue_focused {
        handle_queue_input(app, key);
        return;
    }

    if app.sort_palette.active {
        handle_sort_palette_input(app, key);
        return;
    }

    if app.artist_selection.active {
        handle_artist_selection_input(app, key);
        return;
    }

    // Open search modal when on Search tab
    if app.current_tab == Tab::Search {
        if key.code == KeyCode::Char('/') {
            app.search.modal_open = true;
            app.search.query.clear();
            return;
        }
    }

    if !handle_global_key(app, key) {
        handle_navigation(app, key);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiRequest;
    use crate::api::models::Artist;
    use crate::app::Preferences;
    use crate::lastfm::LastfmCmd;
    use crate::mpris::MprisState;
    use crossterm::event::KeyModifiers;
    use tokio::sync::watch;

    /// The receivers have to outlive the app: dropping them makes its sends fail.
    type Channels = (
        mpsc::UnboundedReceiver<ApiRequest>,
        mpsc::UnboundedReceiver<PlayerCmd>,
        watch::Receiver<MprisState>,
        mpsc::UnboundedReceiver<LastfmCmd>,
    );

    fn app_on_artists_tab() -> (App, Channels) {
        let (api_tx, api_rx) = mpsc::unbounded_channel();
        let (player_tx, player_rx) = mpsc::unbounded_channel();
        let (mpris_tx, mpris_rx) = watch::channel(MprisState::default());
        let (lastfm_tx, lastfm_rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
            Preferences::default(),
        );
        app.current_tab = Tab::Artists;
        app.artists.append_page(
            (0..3)
                .map(|id| Artist {
                    id,
                    name: format!("Artist {id}"),
                    added_at: None,
                })
                .collect(),
            None,
        );
        (app, (api_rx, player_rx, mpris_rx, lastfm_rx))
    }

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// The queue used to be checked before the palette and ate everything typed
    /// into one opened from it.
    #[test]
    fn the_command_palette_outranks_the_focused_queue() {
        let (mut app, _channels) = app_on_artists_tab();
        app.queue_focused = true;

        handle_key(&mut app, press(':'));
        handle_key(&mut app, press('c'));

        assert!(app.command.active);
        assert_eq!(app.command.input, "c");
    }
}
