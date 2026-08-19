// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Terminal image rendering.
//!
//! Protocols are cached by (content, size); see PROTOCOL_CACHE for why that
//! matters for render latency.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_image::{Image, Resize, picker::Picker, protocol::Protocol};

thread_local! {
    /// Cached terminal-image protocols, keyed by (image content, target size).
    ///
    /// Building a protocol decodes the source image and re-encodes it for the
    /// terminal's graphics protocol — ~760 µs for a 320x320 JPEG under Kitty.
    /// Worse, every rebuild produces a *different* payload (a fresh image id),
    /// so ratatui's buffer diff cannot skip it and the whole ~135 KiB escape
    /// sequence is written to the terminal again. With two images on screen —
    /// the now-playing art plus a detail view's art — that came to ~16 MiB/s at
    /// 60 fps, which is what made the cursor lag in the artist, album and
    /// playlist views.
    ///
    /// Cached protocols render byte-identical cells on repeat frames, so the
    /// diff emits nothing at all once the image has been sent.
    static PROTOCOL_CACHE: std::cell::RefCell<
        std::collections::HashMap<(u64, u16, u16), Protocol>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

// Initialize picker once at startup to avoid blocking on every frame
pub(super) fn get_picker() -> &'static Picker {
    static PICKER: std::sync::OnceLock<Picker> = std::sync::OnceLock::new();
    PICKER.get_or_init(|| {
        let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".to_string());
        let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        tracing::info!(
            "Terminal: TERM={}, COLORTERM={} → Image protocol: {:?}",
            term,
            colorterm,
            picker.protocol_type()
        );
        picker
    })
}

/// Only a couple of (image, size) pairs are ever live at once, so a small cap
/// with a wholesale clear is enough to bound growth as the user browses.
pub(super) const PROTOCOL_CACHE_CAP: usize = 8;

/// The terminal's cell size in pixels, queried once at startup.
///
/// Needed to frame an image snugly: `Resize::Fit` preserves the source's aspect
/// ratio, so a box laid out on a guessed cell ratio leaves the border floating
/// clear of the picture on any font that is not exactly twice as tall as wide.
pub(super) fn cell_size() -> (u16, u16) {
    let size = get_picker().font_size();
    (size.width.max(1), size.height.max(1))
}

/// How many whole cells `bytes` covers at its natural size.
///
/// `Resize::Fit` only ever scales an image *down* — an image smaller than its
/// area renders at native size and leaves the rest blank. So a frame drawn
/// around one has to be capped at this, or the border floats clear of the
/// picture. Reads the header only, not the pixels.
pub(super) fn image_cells(bytes: &[u8]) -> Option<(u16, u16)> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    let (cell_w, cell_h) = cell_size();
    Some((
        (w / cell_w as u32).max(1) as u16,
        (h / cell_h as u32).max(1) as u16,
    ))
}

pub(super) fn render_image(f: &mut Frame, bytes: &[u8], area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Hash the content rather than keying on the slice's address: art buffers
    // are freed and reallocated as the user browses, and an allocator reusing
    // an address for a same-sized image would otherwise serve a stale picture.
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        (hasher.finish(), area.width, area.height)
    };

    PROTOCOL_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        if !cache.contains_key(&key) {
            let Ok(img) = image::load_from_memory(bytes) else {
                return;
            };
            let Ok(protocol) = get_picker().new_protocol(img, area.into(), Resize::Fit(None))
            else {
                return;
            };
            if cache.len() >= PROTOCOL_CACHE_CAP {
                cache.clear();
            }
            cache.insert(key, protocol);
        }

        if let Some(protocol) = cache.get(&key) {
            f.render_widget(Image::new(protocol), area);
        }
    });
}
