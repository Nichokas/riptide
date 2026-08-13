// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, ArtistDetailFocus, StatusLevel, Tab, View, KeybindGroup};
use crate::search::SearchPane;
use crate::playlist::PlaylistDetailFocus;
use crate::api::models::Track;


mod theme;
use theme::*;

mod image;
use image::*;

mod tabs;
use tabs::*;

mod queue;
use queue::*;

mod overlays;
use overlays::*;

mod footer;
use footer::*;

mod home;
use home::*;

mod now_playing;
use now_playing::*;

mod search;
use search::*;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let rows = Layout::vertical([
        Constraint::Length(3),  // tab bar (boxed active tab needs 3 rows)
        Constraint::Min(0),     // content + queue
        Constraint::Length(16), // now-playing bar (art stacked above track info)
        Constraint::Length(1),  // help hint
    ])
    .split(area);

    render_tab_bar(f, app, rows[0]);

    if app.queue_visible {
        let cols = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(QUEUE_W),
        ])
        .split(rows[1]);
        render_content(f, app, cols[0]);
        render_queue(f, app, cols[1]);
    } else {
        render_content(f, app, rows[1]);
    }

    render_now_playing(f, app, rows[2]);
    render_footer(f, app, rows[3]);

    if app.command.active {
        render_command_overlay(f, app, area);
    }

    if app.sort_palette.active {
        render_sort_overlay(f, app, area);
    }

    if app.artist_selection.active {
        render_artist_selection_modal(f, app, area);
    }

    if app.help_active {
        render_help_modal(f, app, area);
    }

    render_toast(f, app, area);
}

// ── Tab bar ───────────────────────────────────────────────────────────────────


// ── Queue panel ───────────────────────────────────────────────────────────────


// ── Command overlay ───────────────────────────────────────────────────────────


// ── Sort overlay ──────────────────────────────────────────────────────────────


// ── Artist selection modal ────────────────────────────────────────────────────


// ── Help modal ────────────────────────────────────────────────────────────────


// ── Main content area ─────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, app: &App, area: Rect) {
    // If there's a view on the stack, render it
    if let Some(view) = app.view_stack.last() {
        match view {
            View::ArtistDetail(detail) => {
                render_artist_detail(f, app, detail, area);
                return;
            }
            View::PlaylistDetail(detail) => {
                render_playlist_detail(f, app, detail, area);
                return;
            }
            View::AlbumDetail(detail) => {
                render_album_detail(f, app, detail, area);
                return;
            }
        }
    }

    match app.current_tab {
        Tab::Home => render_home(f, app, area),
        Tab::Artists => render_artist_list(f, app, area),
        Tab::Albums => render_fav_albums_list(f, app, area),
        Tab::Playlists => render_playlist_list(f, app, area),
        Tab::Favorites => {
            let title = format!(" Tracks ({}){} ", app.favorites.items.len(), sort_suffix(app));
            render_track_list(f, app, &app.favorites, true, area, &title);
        }
        Tab::Search => render_search_results(f, app, area),
    }
}

// ── Home tab ──────────────────────────────────────────────────────────────────


// ── Artists list ──────────────────────────────────────────────────────────────

/// Trailing " · A-Z" for a list title, showing how the list is ordered. Empty on
/// tabs that don't sort, so it can be appended unconditionally.
fn sort_suffix(app: &App) -> String {
    app.active_sort()
        .map(|f| format!(" · {}", f.label()))
        .unwrap_or_default()
}

fn render_artist_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.artists.loading && app.artists.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Artists {spinner} ")
        } else {
            format!(" Artists ({}){} ", app.artists.total, sort_suffix(app))
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let items: Vec<ListItem> = visible_artist_items(&app.artists, height)
        .iter()
        .map(|(abs_idx, artist)| {
            let selected = *abs_idx == app.artists.selected;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{}", artist.name)).style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new("No followed artists found.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Saved albums list ─────────────────────────────────────────────────────────

fn render_fav_albums_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.fav_albums.loading && app.fav_albums.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Albums {spinner} ")
        } else {
            format!(" Albums ({}){} ", app.fav_albums.total, sort_suffix(app))
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.fav_albums.items.is_empty() && !loading {
        f.render_widget(
            Paragraph::new("No saved albums found.")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let selected = app.fav_albums.selected;
    let offset = app.fav_albums.scroll_offset(height);

    let items: Vec<ListItem> = app.fav_albums.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(idx, album)| {
            let is_sel = idx == selected;
            let bg = if is_sel { HIGHLIGHT_BG } else { Color::Reset };
            let prefix = if is_sel { "▶ " } else { "  " };
            let artist = album.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();

            let title_style = Style::default()
                .bg(bg)
                .fg(Color::White)
                .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() });
            let sub_style = Style::default().bg(bg).fg(DIM);
            let badge_style = Style::default().bg(bg).fg(ACCENT).add_modifier(Modifier::BOLD);

            let line = Line::from(vec![
                Span::styled(format!("{prefix}{}", album.title), title_style),
                Span::styled(if artist.is_empty() { String::new() } else { format!("  {artist}") }, sub_style),
                Span::styled(badge, badge_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

// ── Artist detail (tracks + albums split) ─────────────────────────────────────

fn render_artist_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let art_col_w: u16 = 22;
    let art_inner_w = art_col_w.saturating_sub(2);
    let art_h = art_inner_w / 2;
    let art_box_h = art_h + 2;

    let cols = Layout::horizontal([
        Constraint::Length(art_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

    render_artist_art(f, app, detail, left_rows[0]);

    render_artist_bio(f, app, detail, left_rows[1]);
    //use Render carousel tabs to render 
    render_carousel_tabs(f, app, detail, cols[1]);
}

fn render_artist_art(f: &mut Frame, app: &App, detail: &crate::app::ArtistDetail, area: Rect) {
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));
    let inner = art_block.inner(area);
    f.render_widget(art_block, area);

    let w = inner.width;
    let h = inner.height;
    if w == 0 || h == 0 {
        return;
    }

    if let Some(bytes) = &detail.art_bytes {
        render_image(f, bytes, inner);
    } else if detail.art_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
    }
}

fn render_artist_bio(f: &mut Frame, app: &App, detail: &crate::app::ArtistDetail, area: Rect) {
    let focused = detail.focus == ArtistDetailFocus::Bio;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) });
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Artist name always at the top.
    f.render_widget(
        Paragraph::new(detail.artist.name.as_str())
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height < 3 {
        return;
    }

    let bio_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 2);

    if detail.bio_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            bio_area,
        );
    } else if let Some(bio) = &detail.bio {
        // Strip HTML tags that Tidal sometimes includes.
        let clean: String = {
            let mut out = String::with_capacity(bio.len());
            let mut in_tag = false;
            for ch in bio.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => out.push(ch),
                    _ => {}
                }
            }
            out
        };
        f.render_widget(
            Paragraph::new(clean)
                .style(Style::default().fg(Color::Rgb(180, 180, 180)))
                .wrap(Wrap { trim: true })
                .scroll((detail.bio_scroll, 0)),
            bio_area,
        );
    } else {
        f.render_widget(
            Paragraph::new("No biography available.")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            bio_area,
        );
    }
}

fn render_carousel_tabs(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    if area.height < 2 {
        return;
    }

    let tabs = vec![
        (format!(" Top Tracks ({})", detail.tracks.items.len()), ArtistDetailFocus::Tracks),
        (format!("Albums ({})", detail.albums.items.len()), ArtistDetailFocus::Albums),
        (format!("EPs ({})", detail.eps.items.len()), ArtistDetailFocus::EPs),
        (format!("Singles ({})", detail.singles.items.len()), ArtistDetailFocus::Singles),
    ];

        // Spans + Line seperators. can be changed or removed completely.
    let mut line_spans = Vec::new();
    for (i, (name, focus)) in tabs.iter().enumerate() {
        if i > 0 {
            line_spans.push(Span::styled(" - ", Style::default().fg(DIM)));
        }

        let selected = detail.focus == *focus;
        let style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        line_spans.push(Span::styled(name.clone(), style));
    }
    //block styling (made it dim cuz that fit better with the surrounding UI)
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Line::from(line_spans));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height > 0 {
        match detail.focus {
            ArtistDetailFocus::Tracks => render_artist_tracks_full(f, app, detail, inner),
            ArtistDetailFocus::Albums => render_artist_albums(f, app, detail, inner),
            ArtistDetailFocus::EPs => render_artist_eps(f, app, detail, inner),
            ArtistDetailFocus::Singles => render_artist_singles(f, app, detail, inner),
            ArtistDetailFocus::Bio => {}
        }
    }
}

fn render_artist_tracks_full(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.tracks.loading;
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.tracks.scroll_offset(height);
    let items: Vec<ListItem> = detail.tracks.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, track)| {
            let selected = i == detail.tracks.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = app.now_playing.track.as_ref().map(|t| t.id == track.id).unwrap_or(false);
            let indicator = if playing { "♪ " } else { "" };
            // `i` stays 0-based for selection; only the displayed ordinal is 1-based.
            let n = i + 1;

            let title_span = Span::styled(
                format!("{prefix}{indicator}{n:>2}. {} ({})", track.title, track.duration_display()),
                style
            );

            let badge = track.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_track_ids.contains(&track.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_artist_albums(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.albums.loading;
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.albums.scroll_offset(height);
    let items: Vec<ListItem> = detail.albums.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.albums.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style
            );

            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_album_ids.contains(&album.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_artist_eps(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.eps.loading;
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.eps.scroll_offset(height);
    let items: Vec<ListItem> = detail.eps.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.eps.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style
            );

            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_album_ids.contains(&album.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_artist_singles(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.singles.loading;
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.singles.scroll_offset(height);
    let items: Vec<ListItem> = detail.singles.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.singles.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style
            );

            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_album_ids.contains(&album.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}
// ── Playlists ─────────────────────────────────────────────────────────────────

fn render_playlist_list(f: &mut Frame, app: &App, area: Rect) {
    let spinner = spinner_char(app.tick);
    let loading = app.playlists.loading && app.playlists.items.is_empty();

    let block = Block::default()
        .title(if loading {
            format!(" Playlists {spinner} ")
        } else {
            format!(" Playlists ({}){} ", app.playlists.total, sort_suffix(app))
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let offset = app.playlists.scroll_offset(height);
    let items: Vec<ListItem> = app.playlists.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, pl)| {
            let selected = i == app.playlists.selected;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{} ({} tracks)", pl.title, pl.number_of_tracks.unwrap_or(0)))
                .style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new("No playlists found.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Generic track list ────────────────────────────────────────────────────────

fn render_track_list(
    f: &mut Frame,
    app: &App,
    tracks: &crate::app::StatefulList<Track>,
    focused: bool,
    area: Rect,
    title: &str,
) {
    let selected = tracks.selected;
    let block = Block::default()
        .title(title)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let offset = tracks.scroll_offset(height);

    let items: Vec<ListItem> = tracks.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, track)| {
            let is_selected = i == selected && focused && !app.help_active;
            let is_playing = app.now_playing.track.as_ref().map(|t| t.id == track.id).unwrap_or(false);
            let style = if is_selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };
            // `i` stays 0-based for selection; only the displayed ordinal is 1-based.
            let n = i + 1;

            let title_span = Span::styled(
                format!(
                    "{prefix}{playing}{n:>3}. {} — {} ({})",
                    track.title,
                    track.all_artist_names(),
                    track.duration_display()
                ),
                style
            );

            let badge = track.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_track_ids.contains(&track.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    if items.is_empty() {
        let p = Paragraph::new("No tracks.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Album detail ──────────────────────────────────────────────────────────────

fn render_album_detail(f: &mut Frame, app: &App, detail: &crate::app::AlbumDetail, area: Rect) {
    // Left column: art (top) + metadata (below).  Right column: full-height track list.
    let art_cols = (area.width / 4).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7)); // cap so metadata fits
    let art_box_h = art_rows + 2; // +2 borders
    let left_col_w = art_cols + 2;

    // Horizontal split: left sidebar | tracks
    let cols = Layout::horizontal([
        Constraint::Length(left_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    // Left sidebar: art (fixed) + metadata (remainder)
    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

    // Alias for clarity — art area is left_rows[0], metadata is left_rows[1]
    let header_cols = [left_rows[0], left_rows[1]];

    // ── Album art ─────────────────────────────────────────────────────────────
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let art_inner = art_block.inner(header_cols[0]);
    f.render_widget(art_block, header_cols[0]);

    if let Some(bytes) = &detail.art_bytes {
        render_image(f, bytes, art_inner);
    } else if detail.art_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(format!("{spinner}"))
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            art_inner,
        );
    }

    // ── Album metadata ────────────────────────────────────────────────────────
    let year = detail.album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
    let n_tracks = detail.album.number_of_tracks.unwrap_or(0);
    let artist_name = detail.album.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("");

    let quality_badge = detail.album.quality_badge();

    let mut meta_lines = Vec::new();

    // Wrap title across multiple lines if needed
    let max_width = (header_cols[1].width as usize).saturating_sub(2); // account for borders
    let mut title_line = String::new();
    for word in detail.album.title.split_whitespace() {
        if title_line.len() + word.len() + 1 <= max_width {
            if !title_line.is_empty() {
                title_line.push(' ');
            }
            title_line.push_str(word);
        } else {
            if !title_line.is_empty() {
                meta_lines.push(Line::from(Span::styled(
                    title_line.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
                title_line.clear();
            }
            title_line.push_str(word);
        }
    }
    if !title_line.is_empty() {
        meta_lines.push(Line::from(Span::styled(
            title_line,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
    }

    meta_lines.push(Line::from(Span::styled(artist_name, Style::default().fg(Color::White))));

    let mut counts_spans = vec![
        Span::styled(format!("{year}  •  {n_tracks} tracks"), Style::default().fg(DIM)),
    ];
    if let Some(badge) = quality_badge {
        counts_spans.push(Span::styled("  ", Style::default()));
        counts_spans.push(Span::styled(
            badge,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    meta_lines.push(Line::from(counts_spans));

    let info = Paragraph::new(meta_lines)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(DIM)));
    f.render_widget(info, header_cols[1]);

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.items.len())
    };
    render_track_list(f, app, &detail.tracks, true, cols[1], &title);
}

fn render_playlist_detail(f: &mut Frame, app: &App, detail: &crate::app::PlaylistDetail, area: Rect) {
    // Layout: left sidebar (art + metadata) | right (track list)
    let art_cols = (area.width / 4).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7));
    let art_box_h = art_rows + 2;
    let left_col_w = art_cols + 2;

    let cols = Layout::horizontal([
        Constraint::Length(left_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

    let header_cols = [left_rows[0], left_rows[1]];

    // ── Playlist cover art ────────────────────────────────────────────────────
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let art_inner = art_block.inner(header_cols[0]);
    f.render_widget(art_block, header_cols[0]);

    if let Some(bytes) = &detail.art_bytes {
        render_image(f, bytes, art_inner);
    } else if detail.art_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(format!("{spinner}"))
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            art_inner,
        );
    }

    // ── Playlist metadata (title + description merged) ────────────────────────
    let n_tracks = detail.playlist.number_of_tracks.unwrap_or(0);
    let focused = detail.focus == PlaylistDetailFocus::Description;
    let meta_area = header_cols[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) });
    let inner = block.inner(meta_area);
    f.render_widget(block, meta_area);

    if inner.height < 3 {
        return;
    }

    // Split inner area into sections: title, track count, description
    let sections = Layout::vertical([
        Constraint::Max(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    // Playlist title (wrapped)
    f.render_widget(
        Paragraph::new(detail.playlist.title.as_str())
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center),
        sections[0],
    );

    // Track count
    f.render_widget(
        Paragraph::new(format!("{} tracks", n_tracks))
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center),
        sections[1],
    );

    // Description with scrolling (if it exists)
    if let Some(desc) = &detail.playlist.description {
        if !desc.is_empty() {
            f.render_widget(
                Paragraph::new(desc.as_str())
                    .style(Style::default().fg(DIM))
                    .wrap(Wrap { trim: true })
                    .scroll((detail.description_scroll, 0)),
                sections[2],
            );
        }
    }

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.items.len())
    };
    let tracks_focused = detail.focus == PlaylistDetailFocus::Tracks;
    render_track_list(f, app, &detail.tracks, tracks_focused, cols[1], &title);
}

// ── Search results (three-pane layout) ───────────────────────────────────────


// ── Now playing bar ───────────────────────────────────────────────────────────


// ── Help hint ─────────────────────────────────────────────────────────────────


// ── Toast ─────────────────────────────────────────────────────────────────────


// ── Helpers ───────────────────────────────────────────────────────────────────


fn visible_artist_items(
    list: &crate::app::StatefulList<crate::api::models::Artist>,
    height: usize,
) -> Vec<(usize, &crate::api::models::Artist)> {
    let offset = list.scroll_offset(height);
    list.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .collect()
}


