// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc;

mod api;
mod app;
mod events;
mod lastfm;
mod manifest;
mod mpris;
mod player;
mod playlist;
mod search;
mod ui;
mod update;

use api::ApiWorker;
use app::App;
use lastfm::auth as lastfm_auth_module;
use mpris::MprisServer;
use player::PlayerWorker;

fn lastfm_auth() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(lastfm_auth_module::authenticate())?;
    Ok(())
}

fn setup_panic_hook() {
    let original = std::panic::take_hook();
    let main_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        // Only restore the terminal if the panic is on the main thread.
        // Actor-thread panics are caught via `catch_unwind` and must not tear
        // down the TUI's alternate screen/raw mode.
        if std::thread::current().id() == main_thread {
            let _ = disable_raw_mode();
            let _ = execute!(
                std::io::stderr(),
                LeaveAlternateScreen,
                crossterm::cursor::Show,
            );
        }
        original(info);
    }));
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.iter().any(|a| a == "--lastfm-auth") {
        return lastfm_auth();
    }

    if args.get(1).is_some_and(|a| a == "update") {
        return update::run_update_cli();
    }

    setup_panic_hook();

    // Initialize logging to file
    if let Ok(home) = std::env::var("HOME") {
        let log_dir = std::path::PathBuf::from(home).join(".local/share/riptide");
        let _ = std::fs::create_dir_all(&log_dir);
        let file_appender = tracing_appender::rolling::daily(&log_dir, "riptide.log");

        let log_level = std::env::var("RIPTIDE_LOG_LEVEL")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| "warn".to_string());
        let env_filter = tracing_subscriber::EnvFilter::new(&log_level);

        let _ = tracing_subscriber::fmt()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_env_filter(env_filter)
            .with_thread_ids(false)
            .with_target(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .init();
    }

    tracing::info!("╔══════════════════════════════════════════════════════════════╗");
    tracing::info!("║                      🎵 RIPTIDE STARTING 🎵                  ║");
    tracing::info!("╚══════════════════════════════════════════════════════════════╝");
    tracing::info!("Loading configuration...");
    let mut config = api::auth::load_config()?;
    api::auth::ensure_auth(&mut config)?;
    tracing::info!("Authentication successful (v{})", env!("CARGO_PKG_VERSION"));

    // Channels: TUI → ApiWorker and TUI → PlayerWorker
    let (api_req_tx, api_req_rx) = mpsc::unbounded_channel();
    let (api_resp_tx, api_resp_rx) = mpsc::unbounded_channel();
    let (player_cmd_tx, player_cmd_rx) = mpsc::unbounded_channel::<crate::player::PlayerCmd>();
    let (player_evt_tx, player_evt_rx) = mpsc::unbounded_channel();
    let (player_evt_lastfm_tx, player_evt_lastfm_rx) = mpsc::unbounded_channel();

    // Channels for MPRIS: TUI → MPRIS server (state updates) and MPRIS → TUI (control commands)
    let (mpris_state_tx, mpris_state_rx) =
        tokio::sync::watch::channel(mpris::MprisState::default());
    let (mpris_cmd_tx, mpris_cmd_rx) = mpsc::unbounded_channel::<mpris::MprisCmd>();

    // Channels for Last.fm worker: TUI → Last.fm and Last.fm → TUI
    let (lastfm_cmd_tx, lastfm_cmd_rx) = mpsc::unbounded_channel::<lastfm::LastfmCmd>();
    let (_lastfm_evt_tx, _lastfm_evt_rx) = mpsc::unbounded_channel::<lastfm::LastfmEvent>();

    // Spawn async workers on a dedicated Tokio thread.
    // We keep the handle so we can join it on exit and let PlayerWorker kill mpv cleanly.
    let worker_config = config.clone();
    let worker_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let api_worker = ApiWorker::new(worker_config.clone(), api_req_rx, api_resp_tx);
            let player_worker = PlayerWorker::new(player_cmd_rx, player_evt_tx);
            let mpris_server = MprisServer::new(mpris_state_rx, mpris_cmd_tx);
            let lastfm_worker = lastfm::worker::LastfmWorker::new(
                worker_config.lastfm,
                lastfm_cmd_rx,
                player_evt_lastfm_rx,
                _lastfm_evt_tx,
            );
            tokio::spawn(manifest::run_server());
            tokio::join!(
                api_worker.run(),
                player_worker.run(),
                mpris_server.run(),
                lastfm_worker.run()
            );
        });
    });

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Build app state and run
    let mut app = App::new(api_req_tx, player_cmd_tx, mpris_state_tx, lastfm_cmd_tx);

    // Reap any staged files left by a previous cancelled update.
    update::cleanup_stale_artifacts();

    // Self-update actor: checks GitHub once (shortly after startup, so it
    // never delays the first frame), then stays alive to run a TUI-triggered
    // install. Skipped entirely for pacman/nix/cargo installs. Not started in
    // App::new so tests stay offline.
    if let Some(actor) = app.take_update_actor() {
        let cancel = app.update_cancel.clone();
        std::thread::spawn(move || {
            let mut go_rx = actor.go_rx;
            let check_tx = actor.check_tx;
            let checking_tx = actor.checking_tx;
            let result_tx = actor.result_tx;
            std::thread::sleep(std::time::Duration::from_secs(3));
            // install_method is evaluated here, not on the main thread, so a
            // stuck ALPM db (pacman -Syu) never blocks the first frame.
            if update::install_method() != update::InstallMethod::Script {
                tracing::debug!("self-update disabled for this install method");
                // No availability check; checking stays false so `U` reports
                // "Updates are handled by your package manager".
                return;
            }
            let _ = checking_tx.send(());
            let check_result = std::panic::catch_unwind(update::check_for_update_assuming_script);
            let to_send = match check_result {
                Ok(r) => r,
                Err(_) => Err("update check panicked".to_string()),
            };
            let _ = check_tx.send(to_send);
                // Loop to handle multiple install attempts (e.g. retry after failure).
                while let Some(()) = go_rx.blocking_recv() {
                    // Catch panics so the TUI never hangs in Working. Reset the
                    // cancel flag per attempt so a retried update can proceed.
                    cancel.store(false, std::sync::atomic::Ordering::Relaxed);
                    let outcome = std::panic::catch_unwind(|| update::self_update_with_cancel(&cancel));
                    let result = match outcome {
                        Ok(Ok(update::UpdateOutcome::Updated(tag))) => Ok(tag),
                        Ok(Ok(update::UpdateOutcome::AlreadyCurrent)) => {
                            Err("Already up to date".to_string())
                        }
                        Ok(Err(e)) => Err(format!("{e:#}")),
                        Err(_) => Err("update panicked".to_string()),
                    };
                    // If TUI closed, send will fail — exit loop.
                    if result_tx.send(result).is_err() {
                        break;
                    }
                }
                tracing::debug!("update actor: TUI closed or channel dropped");
            });
    }

    let result = events::run_app(
        &mut terminal,
        &mut app,
        api_resp_rx,
        player_evt_rx,
        mpris_cmd_rx,
        player_evt_lastfm_tx,
    );

    // Restore terminal unconditionally
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Dropping app closes the command channels, which causes both workers to exit
    // their loops. Joining ensures PlayerWorker reaches child.kill() before we return.
    drop(app);
    let _ = worker_thread.join();

    if result.is_ok() {
        tracing::info!("Application shutdown complete");
    } else {
        tracing::error!("Application error: {:?}", result);
    }

    result
}
