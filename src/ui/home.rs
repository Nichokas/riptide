// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! The Home tab: new releases, daily mixes and discovery mixes.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::*;
use crate::app::{App, HomeSectionFocus};

/// Narrowest art column worth drawing into. Below this the cover is a smudge
/// and the mixes are better off with the width.
const HOME_ART_MIN_W: u16 = 12;

pub(super) fn render_home(f: &mut Frame, app: &App, area: Rect) {
    let labels = [
        section_label(app, HomeSectionFocus::NewReleases),
        section_label(app, HomeSectionFocus::DailyMixes),
        section_label(app, HomeSectionFocus::DiscoveryMixes),
    ];

    // The art only gets a column once the tab strip has the room it needs;
    // otherwise the section names truncate away and navigation loses its map.
    let art_w = area
        .width
        .saturating_sub(carousel_width(&labels))
        .min(area.width / 4);
    let list_area = if art_w >= HOME_ART_MIN_W {
        let cols = Layout::horizontal([Constraint::Length(art_w), Constraint::Min(0)]).split(area);
        render_home_art(f, app, cols[0]);
        cols[1]
    } else {
        area
    };

    let Some(inner) = render_carousel(f, list_area, &labels) else {
        return;
    };

    render_home_section(f, app, app.home_section(), inner);
}

/// A section's tab label: its own spinner while loading, its count once it has
/// arrived. Each section is fetched separately, so one still in flight no longer
/// holds up the two that are ready.
fn section_label(app: &App, section: HomeSectionFocus) -> (String, bool) {
    let (name, state) = match section {
        HomeSectionFocus::NewReleases => ("New Releases", &app.home_new_releases),
        HomeSectionFocus::DailyMixes => ("Daily Mixes", &app.home_daily_mixes),
        HomeSectionFocus::DiscoveryMixes => ("Daily Discovery", &app.home_discovery_mixes),
    };
    let label = if state.loading {
        format!("{name} {}", spinner_char(app.tick))
    } else {
        format!("{name} ({})", state.items.len())
    };
    (label, app.home_section_focus == section)
}

fn render_home_art(f: &mut Frame, app: &App, area: Rect) {
    let art_h = (area.width.saturating_sub(2) / 2).max(3) + 2;
    let rows = Layout::vertical([
        Constraint::Length(art_h.min(area.height)),
        Constraint::Min(0),
    ])
    .split(area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(rows[0]);
    f.render_widget(block, rows[0]);

    if let Some(bytes) = &app.home_art.bytes {
        render_image(f, bytes, inner);
    } else if app.home_art.loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
    }

    if let Some(mix) = app.selected_home_mix() {
        f.render_widget(
            Paragraph::new(mix.title.as_str())
                .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Center),
            rows[1],
        );
    }
}

pub(super) fn render_home_section(
    f: &mut Frame,
    app: &App,
    section: &crate::app::HomeSection<crate::api::models::Playlist>,
    area: Rect,
) {
    if section.loading {
        let text = format!("{} Loading...", spinner_char(app.tick));
        f.render_widget(Paragraph::new(text).style(Style::default().fg(DIM)), area);
        return;
    }

    if let Some(ref error) = section.error {
        f.render_widget(
            Paragraph::new(format!("Error: {error}")).style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    if section.items.is_empty() {
        f.render_widget(
            Paragraph::new("No items").style(Style::default().fg(DIM)),
            area,
        );
        return;
    }

    let height = area.height as usize;
    let start = section.selected.saturating_sub(height.saturating_sub(1));

    let visible_items: Vec<ListItem> = section.items[start..]
        .iter()
        .take(height)
        .enumerate()
        .map(|(i, item)| {
            let is_selected = start + i == section.selected;

            let title_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(simple_row(
                app,
                &item.title,
                area.width,
                is_selected,
                title_style,
                "",
            ))
        })
        .collect();

    f.render_widget(List::new(visible_items), area);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Playlist;
    use crate::app::test_support::test_app;
    use ratatui::{Terminal, backend::TestBackend};

    fn mix(n: usize, name: &str) -> Playlist {
        Playlist {
            uuid: format!("uuid-{n}"),
            title: name.to_string(),
            number_of_tracks: None,
            description: None,
            cover: None,
            added_at: None,
        }
    }

    /// The counts here are what the live endpoints return: one new-release mix,
    /// eight daily mixes, one discovery mix.
    fn home_app() -> App {
        let mut t = test_app();
        t.app.home_new_releases.items = vec![mix(0, "My New Arrivals")];
        t.app.home_daily_mixes.items = (1..=8).map(|n| mix(n, &format!("My Mix {n}"))).collect();
        t.app.home_discovery_mixes.items = vec![mix(9, "My Daily Discovery")];
        t.app.home_new_releases.loading = false;
        t.app.home_daily_mixes.loading = false;
        t.app.home_discovery_mixes.loading = false;
        std::mem::forget(t.api_rx);
        t.app
    }

    fn top_row(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_home(f, app, Rect::new(0, 0, w, h)))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..w)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn the_tab_strip_names_every_section_and_its_count() {
        let strip = top_row(&home_app(), 100, 20);
        assert!(strip.contains("New Releases (1)"), "{strip}");
        assert!(strip.contains("Daily Mixes (8)"), "{strip}");
        assert!(strip.contains("Daily Discovery (1)"), "{strip}");
    }

    /// Two boxes on the top row means the art got a column, one means it was
    /// dropped so the strip keeps its labels.
    #[test]
    fn the_art_column_yields_when_the_strip_needs_the_width() {
        let app = home_app();
        assert_eq!(top_row(&app, 100, 20).matches('┌').count(), 2);
        assert_eq!(top_row(&app, 60, 14).matches('┌').count(), 1);
    }
}
