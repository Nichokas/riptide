// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Small extractors shared by the per-domain JSON:API parsers.

use std::collections::HashMap;

use crate::api::models::*;

pub(super) fn parse_iso_duration(s: &str) -> u32 {
    // Parse ISO 8601 duration format (e.g., "PT5M10S" = 5 min 10 sec = 310 sec)
    let s = s.trim_start_matches('P').trim_start_matches('T');
    let mut seconds = 0u32;
    let mut current_num = String::new();

    for ch in s.chars() {
        match ch {
            '0'..='9' => current_num.push(ch),
            'H' => {
                if let Ok(hours) = current_num.parse::<u32>() {
                    seconds += hours * 3600;
                }
                current_num.clear();
            }
            'M' => {
                if let Ok(minutes) = current_num.parse::<u32>() {
                    seconds += minutes * 60;
                }
                current_num.clear();
            }
            'S' => {
                if let Ok(secs) = current_num.parse::<u32>() {
                    seconds += secs;
                }
                current_num.clear();
            }
            _ => {}
        }
    }
    seconds
}

/// Every artist credited on a track, in the order the relationship lists them.
///
/// A track's `relationships.artists` routinely holds more than one — features
/// and collaborations — so taking only the first drops a credit the UI shows.
/// Ids the response did not include are skipped rather than rendered blank.
pub(super) fn extract_artists_from_track(
    track_obj: &serde_json::Value,
    artist_map: &HashMap<String, serde_json::Value>,
) -> Vec<ArtistRef> {
    let Some(refs) = track_obj
        .get("relationships")
        .and_then(|v| v.get("artists"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };

    refs.iter()
        .filter_map(|artist_ref| artist_ref.get("id").and_then(|v| v.as_str()))
        .filter_map(|id| artist_map.get(id))
        .filter_map(|artist_obj| {
            artist_obj
                .get("attributes")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
        })
        .map(|name| ArtistRef {
            name: name.to_string(),
        })
        .collect()
}

pub(super) fn extract_artist_from_album(
    album_obj: &serde_json::Value,
    artist_map: &HashMap<String, serde_json::Value>,
) -> Option<String> {
    album_obj
        .get("relationships")
        .and_then(|v| v.get("artists"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|artist_id| artist_map.get(artist_id))
        .and_then(|artist_obj| artist_obj.get("attributes"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub(super) fn extract_cover_from_album(album_obj: &serde_json::Value) -> Option<String> {
    album_obj
        .get("relationships")
        .and_then(|v| v.get("coverArt"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub(super) fn extract_media_metadata(
    attrs: &serde_json::Map<String, serde_json::Value>,
) -> Option<MediaMetadata> {
    attrs
        .get("mediaTags")
        .and_then(|v| v.as_array())
        .map(|tags| {
            let tag_strs: Vec<String> = tags
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect();
            MediaMetadata { tags: tag_strs }
        })
}

pub(super) fn extract_album_id_from_track(track_obj: &serde_json::Value) -> u64 {
    track_obj
        .get("relationships")
        .and_then(|v| v.get("albums"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

pub(super) fn build_artist_map(api_resp: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    let mut artist_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("artists") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    artist_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }
    artist_map
}
