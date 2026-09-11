// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Input handling and the main event loop.
//!
//! [`run_app`] drives the draw/poll cycle. Key dispatch in `handle_key` takes
//! the text boxes first, then rewrites `j`/`k` into arrow keys, then works
//! through the remaining contexts that capture input — help, the queue, the
//! pickers — before falling through to the global bindings and list navigation.

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::ApiResponse;
use crate::app::{App, Tab};
use crate::mpris::MprisCmd;
use crate::player::PlayerEvent;

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

/// Watches for SIGINT, SIGTERM and SIGHUP and reports them through the flag.
///
/// Closing the terminal window used to kill the process outright, losing the
/// session's preference changes (sorts, volume, queue visibility) because those
/// are only written on a clean exit.
///
/// Unix only, deliberately: the crate does not build elsewhere — the player
/// talks to mpv over a `UnixStream` on a hardcoded socket path.
pub fn spawn_signal_watcher(shutdown: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    use tokio::signal::unix::{Signal, SignalKind, signal};

    /// `recv` on a handler that failed to register must never complete, or
    /// `select!` would read the missing signal as having just fired.
    async fn recv(slot: &mut Option<Signal>) {
        match slot {
            Some(s) => {
                s.recv().await;
            }
            None => std::future::pending().await,
        }
    }

    std::thread::spawn(move || {
        // A signal wait needs no worker pool; current_thread suffices.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            // Panicking here would trip the panic hook, which restores the
            // terminal from under a TUI that is still running and leaves the
            // user typing into a garbled screen.
            Err(e) => {
                tracing::warn!("signal watcher unavailable ({e}); quitting will not be graceful");
                return;
            }
        };
        rt.block_on(async {
            // Registered one at a time and kept even if a sibling fails. Tokio
            // installs its handler process-wide on the first `signal()` for a
            // kind and never removes it, so dropping a receiver that did
            // register is what silently swallows that signal — the partial set
            // has to stay alive, not be thrown away.
            let register = |kind: SignalKind, name: &str| match signal(kind) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!(
                        "{name} handler unavailable ({e}); it will not shut down cleanly"
                    );
                    None
                }
            };
            let mut term = register(SignalKind::terminate(), "SIGTERM");
            let mut hup = register(SignalKind::hangup(), "SIGHUP");
            let mut int = register(SignalKind::interrupt(), "SIGINT");

            if term.is_none() && hup.is_none() && int.is_none() {
                return;
            }

            tokio::select! {
                _ = recv(&mut term) => {}
                _ = recv(&mut hup) => {}
                _ = recv(&mut int) => {}
            }
            shutdown.store(true, Ordering::Relaxed);

            // Tokio's handler stays installed for the process's lifetime, so
            // every kind has to go on being read: a repeat has no default
            // action left to fall back on, and would be swallowed rather than
            // forcing past a stalled save or worker join. Watching only SIGINT
            // left `systemctl stop` waiting out its whole TimeoutStopSec.
            let signum = tokio::select! {
                _ = recv(&mut term) => SIGTERM,
                _ = recv(&mut hup) => SIGHUP,
                _ = recv(&mut int) => SIGINT,
            };
            tracing::warn!("repeated signal during shutdown; exiting immediately");
            // Preferences are lost, but `save_config` writes through a
            // temporary file, so the credentials it shares a file with survive
            // being cut off mid-write.
            std::process::exit(128 + signum);
        });
    })
}

/// Signal numbers, for the 128+N exit-code convention. POSIX fixes these three,
/// so they are not worth a `libc` dependency.
const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

pub fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut api_rx: mpsc::UnboundedReceiver<ApiResponse>,
    mut player_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    mut mpris_rx: mpsc::UnboundedReceiver<MprisCmd>,
    lastfm_evt_tx: mpsc::UnboundedSender<PlayerEvent>,
    signal_shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    loop {
        // Checked before painting: SIGHUP takes the pty with it, so a frame
        // drawn after the signal fails rather than showing anything.
        if signal_shutdown.load(Ordering::Relaxed) {
            break;
        }

        if let Err(e) = terminal.draw(|f| crate::ui::draw(f, app)) {
            // The signal can land between that check and this draw, and then
            // the write fails with EIO. Closing a terminal window is an
            // ordinary way to quit, not a failure to report — reporting it
            // exits non-zero, which makes a `Restart=on-failure` unit respawn
            // a TUI that has no terminal to draw on.
            if signal_shutdown.load(Ordering::Relaxed) {
                break;
            }
            return Err(e.into());
        }

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
                MprisCmd::Play => app.mpris_play(),
                MprisCmd::Pause => app.set_paused(true),
                MprisCmd::PlayPause => app.mpris_play_pause(),
                MprisCmd::Stop => app.stop_playback(),
                MprisCmd::Quit => app.should_quit = true,
                MprisCmd::SetVolume(v) => {
                    app.set_volume_percent((v.clamp(0.0, 1.0) * 100.0).round() as u8)
                }
                MprisCmd::SetShuffle(on) => app.set_shuffle(on),
                MprisCmd::Seek(offset_us) => app.seek_by_us(offset_us),
                MprisCmd::SetPosition(track_id, position_us) => {
                    app.set_position_us(track_id, position_us)
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

/// `j`/`k` stand in for `Down`/`Up`. `h`/`l` already move between panes, so the
/// same hand should move within one.
///
/// Rewriting the event once, here, reaches every list — the tabs, the detail
/// views, the queue and the overlays — where adding a `Char('j')` arm beside
/// each of the fifteen `KeyCode::Down` arms would leave the two spellings free
/// to drift apart. Modified presses are left alone: the queue gives `Ctrl+Up`
/// and `Ctrl+Down` a meaning of their own, and terminals send `Ctrl+J` as Enter.
fn vim_arrows(mut key: KeyEvent) -> KeyEvent {
    if !key.modifiers.is_empty() {
        return key;
    }
    key.code = match key.code {
        KeyCode::Char('j') => KeyCode::Down,
        KeyCode::Char('k') => KeyCode::Up,
        code => code,
    };
    key
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // Any keystroke means the user is working the list, so restart the marquee
    // and let them read the row they just landed on from its beginning.
    app.marquee_epoch = std::time::Instant::now();

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

    // Past the text boxes, letters are commands again.
    let key = vim_arrows(key);

    if app.help_active {
        handle_help_input(app, key);
        return;
    }

    // Fullscreen art is a presentation layer over the active view. Only global
    // controls apply while it is open, so list navigation cannot mutate the
    // view hidden beneath it. It also outranks queue focus so Esc dismisses
    // art instead of the queue.
    if app.art_fullscreen {
        handle_global_key(app, key);
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
    use crate::api::models::Artist;
    use crate::app::test_support::{TestApp, test_app};
    use crossterm::event::KeyModifiers;

    fn app_on_artists_tab() -> TestApp {
        let mut t = test_app();
        t.app.current_tab = Tab::Artists;
        t.app.artists.append_page(
            (0..3)
                .map(|id| Artist {
                    id,
                    name: format!("Artist {id}"),
                    added_at: None,
                })
                .collect(),
            None,
        );
        t
    }

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn j_and_k_move_the_selection() {
        let mut t = app_on_artists_tab();

        handle_key(&mut t.app, press('j'));
        assert_eq!(t.app.artists.selected, 1);

        handle_key(&mut t.app, press('k'));
        assert_eq!(t.app.artists.selected, 0);
    }

    /// The queue used to be checked before the palette and ate everything typed
    /// into one opened from it.
    #[test]
    fn the_command_palette_outranks_the_focused_queue() {
        let mut t = app_on_artists_tab();
        t.app.queue_focused = true;

        handle_key(&mut t.app, press(':'));
        handle_key(&mut t.app, press('c'));

        assert!(t.app.command.active);
        assert_eq!(t.app.command.input, "c");
    }

    /// The half that is easy to break: inside a text box they are letters again.
    #[test]
    fn the_filter_box_reads_j_and_k_as_letters() {
        let mut t = app_on_artists_tab();
        t.app.filter_active = true;

        handle_key(&mut t.app, press('j'));
        handle_key(&mut t.app, press('k'));

        assert_eq!(t.app.active_filter(), "jk");
        assert_eq!(t.app.artists.selected, 0);
    }

    #[test]
    fn fullscreen_art_preempts_queue_focus_for_escape_and_navigation() {
        let mut t = test_app();
        t.app.art_fullscreen = true;
        t.app.queue_focused = true;

        handle_key(
            &mut t.app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert!(t.app.art_fullscreen);
        assert!(t.app.status.is_none());
        assert!(t.app.view_stack.is_empty());

        handle_key(&mut t.app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!t.app.art_fullscreen);
        assert!(t.app.queue_focused);
    }
}
