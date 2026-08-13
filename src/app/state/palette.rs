// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Command palette and the artist-picker modal.


// ── Artist selection modal ────────────────────────────────────────────────────

pub struct ArtistSelection {
    pub active: bool,
    pub artist_names: Vec<String>,
    pub selected: usize,
    pub searching_for: Option<String>,
}

impl Default for ArtistSelection {
    fn default() -> Self {
        Self { active: false, artist_names: Vec::new(), selected: 0, searching_for: None }
    }
}

// ── Command palette ───────────────────────────────────────────────────────────

pub struct CommandState {
    pub active: bool,
    pub input: String,
    pub selected: usize,
}

impl Default for CommandState {
    fn default() -> Self {
        Self { active: false, input: String::new(), selected: 0 }
    }
}

impl CommandState {
    pub const COMMANDS: &'static [&'static str] =
        &["home", "favorites", "artists", "albums", "playlists", "search"];

    pub fn matches(&self) -> Vec<&'static str> {
        let q = self.input.to_lowercase();
        Self::COMMANDS.iter()
            .filter(|&&c| c.starts_with(q.as_str()))
            .copied()
            .collect()
    }
}
