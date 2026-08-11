// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use tokio::sync::RwLock;

use super::auth::refresh_token_async;
use super::models::*;

const BASE: &str = "https://api.tidal.com/v1";
const OPENAPI_BASE: &str = "https://openapi.tidal.com/v2";

// Private types for the openapi.tidal.com/v2 JSON:API collection endpoints.
#[derive(serde::Deserialize)]
struct OpenApiRelPage {
    data: Vec<OpenApiRelItem>,
    #[serde(default)]
    included: Vec<OpenApiIncluded>,
    links: Option<OpenApiLinks>,
}

#[derive(serde::Deserialize)]
struct OpenApiRelItem {
    id: String,
    meta: Option<OpenApiItemMeta>,
}

#[derive(serde::Deserialize)]
struct OpenApiItemMeta {
    #[serde(rename = "addedAt")]
    added_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct OpenApiIncluded {
    id: String,
    attributes: Option<OpenApiPlaylistAttrs>,
}

#[derive(serde::Deserialize)]
struct OpenApiPlaylistAttrs {
    name: String,
    #[serde(rename = "numberOfItems")]
    number_of_items: Option<u32>,
}

#[derive(serde::Deserialize)]
struct OpenApiLinks {
    meta: Option<OpenApiLinksMeta>,
}

#[derive(serde::Deserialize)]
struct OpenApiLinksMeta {
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36";

fn parse_iso_duration(s: &str) -> u32 {
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

fn extract_artist_from_track(
    track_obj: &serde_json::Value,
    artist_map: &HashMap<String, serde_json::Value>,
) -> String {
    track_obj
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
        .unwrap_or("Unknown")
        .to_string()
}

fn build_artist_map(api_resp: &serde_json::Value) -> HashMap<String, serde_json::Value> {
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

fn parse_v2_playlist_tracks(api_resp: &serde_json::Value) -> Result<(Vec<Track>, u32, Option<String>, Option<String>, Option<String>)> {
    // Extract description and cover from playlist attributes
    let description = api_resp.get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract cover art URL from the included array
    let cover = {
        let artwork_id = api_resp.get("data")
            .and_then(|v| v.get("relationships"))
            .and_then(|v| v.get("coverArt"))
            .and_then(|v| v.get("data"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str());

        if let Some(id) = artwork_id {
            // Find the artwork object in the included array
            api_resp.get("included")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().find(|item| {
                    item.get("id").and_then(|v| v.as_str()) == Some(id)
                        && item.get("type").and_then(|v| v.as_str()) == Some("artworks")
                }))
                .and_then(|artwork| artwork.get("attributes"))
                .and_then(|attrs| attrs.get("files"))
                .and_then(|files| files.as_array())
                .and_then(|arr| arr.iter().find(|f| {
                    f.get("meta").and_then(|m| m.get("width")).and_then(|w| w.as_u64()) == Some(320)
                }))
                .and_then(|f| f.get("href"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    };

    // Get track IDs from the playlist's items relationship
    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data")
        .and_then(|v| v.get("relationships"))
        .and_then(|v| v.get("items"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
    {
        tracing::debug!("Found items in data.relationships.items.data, count: {}", items_data.len());
        for (idx, item_ref) in items_data.iter().enumerate() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                tracing::debug!("  Track {}: ID {}", idx, track_id);
                track_ids.push(track_id.to_string());
            }
        }
    } else {
        tracing::debug!("No items found in data.relationships.items.data");
    }

    tracing::debug!("Extracted {} track IDs: {:?}", track_ids.len(), track_ids);

    // Build a map of track IDs to track details from the included array
    let mut track_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("tracks") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    track_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }

    let artist_map = build_artist_map(api_resp);

    let mut tracks = Vec::new();
    for track_id in track_ids {
        if let Some(track_obj) = track_map.get(&track_id) {
            if let Some(attrs) = track_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(title) = attrs.get("title").and_then(|v| v.as_str()) {
                    tracing::debug!("  Track ID {} -> Title: '{}'", track_id, title);
                    if let Ok(id) = track_id.parse::<u64>() {
                        let duration = parse_iso_duration(
                            attrs.get("duration")
                                .and_then(|v| v.as_str())
                                .unwrap_or("PT0S")
                        );

                        let artist_name = extract_artist_from_track(track_obj, &artist_map);

                        let album_title = attrs.get("album")
                            .and_then(|v| v.as_object())
                            .and_then(|obj| obj.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");

                        tracks.push(Track {
                            id,
                            title: title.to_string(),
                            duration,
                            artist: Some(ArtistRef {
                                name: artist_name,
                            }),
                            artists: Vec::new(),
                            album: Album {
                                id: 0,
                                title: album_title.to_string(),
                                number_of_tracks: None,
                                release_date: None,
                                cover: None,
                                artist: None,
                                audio_quality: None,
                                media_metadata: None,
                                added_at: None,
                            },
                            audio_quality: None,
                            media_metadata: None,
                            added_at: None,
                        });
                    }
                }
            }
        } else {
            tracing::debug!("  Track ID {} NOT FOUND in included array!", track_id);
        }
    }

    let total = api_resp.get("meta")
        .and_then(|v| v.get("totalNumberOfItems"))
        .and_then(|v| v.as_u64())
        .unwrap_or(tracks.len() as u64) as u32;

    // Get the next page URL from data.relationships.items.links.next
    let next_url = api_resp.get("data")
        .and_then(|v| v.get("relationships"))
        .and_then(|v| v.get("items"))
        .and_then(|v| v.get("links"))
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match &next_url {
        Some(_) => tracing::debug!("Next page URL available"),
        None => tracing::debug!("No more pages"),
    }

    Ok((tracks, total, next_url, description, cover))
}

fn parse_playlist_relationship_items(api_resp: &serde_json::Value, total: u32) -> Result<(Vec<Track>, u32, Option<String>)> {
    tracing::debug!("Parsing playlist relationship items response");

    // In relationship responses, items are directly in the data array
    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data").and_then(|v| v.as_array()) {
        tracing::debug!("Found items in data array, count: {}", items_data.len());
        for (idx, item_ref) in items_data.iter().enumerate() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                tracing::debug!("  Track {}: ID {}", idx, track_id);
                track_ids.push(track_id.to_string());
            }
        }
    } else {
        tracing::debug!("No items found in data array");
    }

    tracing::debug!("Extracted {} track IDs: {:?}", track_ids.len(), track_ids);

    // Build a map of track IDs to track details from the included array
    let mut track_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("tracks") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    track_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }

    let artist_map = build_artist_map(api_resp);

    let mut tracks = Vec::new();
    for track_id in track_ids {
        if let Some(track_obj) = track_map.get(&track_id) {
            if let Some(attrs) = track_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(title) = attrs.get("title").and_then(|v| v.as_str()) {
                    tracing::debug!("  Track ID {} -> Title: '{}'", track_id, title);
                    if let Ok(id) = track_id.parse::<u64>() {
                        let duration = parse_iso_duration(
                            attrs.get("duration")
                                .and_then(|v| v.as_str())
                                .unwrap_or("PT0S")
                        );

                        let artist_name = extract_artist_from_track(track_obj, &artist_map);

                        let album_title = attrs.get("album")
                            .and_then(|v| v.as_object())
                            .and_then(|obj| obj.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");

                        tracks.push(Track {
                            id,
                            title: title.to_string(),
                            duration,
                            artist: Some(ArtistRef {
                                name: artist_name,
                            }),
                            artists: Vec::new(),
                            album: Album {
                                id: 0,
                                title: album_title.to_string(),
                                number_of_tracks: None,
                                release_date: None,
                                cover: None,
                                artist: None,
                                audio_quality: None,
                                media_metadata: None,
                                added_at: None,
                            },
                            audio_quality: None,
                            media_metadata: None,
                            added_at: None,
                        });
                    }
                }
            }
        } else {
            tracing::debug!("  Track ID {} NOT FOUND in included array!", track_id);
        }
    }

    // Get the next page URL from links.next (at top level for relationship responses)
    let next_url = api_resp.get("links")
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match &next_url {
        Some(_) => tracing::debug!("Next page URL available"),
        None => tracing::debug!("No more pages"),
    }

    Ok((tracks, total, next_url))
}

fn parse_v2_user_playlists(api_resp: &serde_json::Value) -> Result<(Vec<Playlist>, u32)> {
    tracing::debug!("Parsing user playlists from JSON:API response");

    let mut playlist_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data")
        .and_then(|v| v.get("relationships"))
        .and_then(|v| v.get("items"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
    {
        tracing::debug!("Extracting {} playlist references from relationships", items_data.len());
        for item_ref in items_data {
            if let Some(id) = item_ref.get("id").and_then(|v| v.as_str()) {
                playlist_ids.push(id.to_string());
            }
            if let Some(added_at) = item_ref.get("meta")
                .and_then(|m| m.get("addedAt"))
                .and_then(|v| v.as_str()) {
                tracing::debug!("Playlist added at: {}", added_at);
            }
        }
    }

    let mut playlist_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        tracing::debug!("Building playlist map from {} included items", included.len());
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("playlists") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    playlist_map.insert(id.to_string(), item.clone());
                }
            }
        }
        tracing::debug!("Found {} playlists in included array", playlist_map.len());
    }

    let mut playlists = Vec::new();
    for playlist_id in playlist_ids {
        if let Some(playlist_obj) = playlist_map.get(&playlist_id) {
            if let Some(attrs) = playlist_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(name) = attrs.get("name").and_then(|v| v.as_str()) {
                    let number_of_items = attrs.get("numberOfItems")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);

                    playlists.push(Playlist {
                        uuid: playlist_id.clone(),
                        title: name.to_string(),
                        number_of_tracks: number_of_items,
                        description: None,
                        cover: None,
                        added_at: None,
                    });

                    tracing::debug!("Parsed playlist: {} ({} items)", name, number_of_items.unwrap_or(0));
                }
            }
        }
    }

    let total = api_resp.get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("numberOfItems"))
        .and_then(|v| v.as_u64())
        .unwrap_or(playlists.len() as u64) as u32;

    tracing::debug!("Playlist parsing complete: {} playlists parsed (total: {})", playlists.len(), total);

    Ok((playlists, total))
}

fn parse_v2_track_details(api_resp: &serde_json::Value) -> Result<(Track, Option<String>)> {
    let track_data = api_resp.get("data").context("missing data field")?;
    let track_id = track_data.get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .context("missing or invalid track id")?;

    let attrs = track_data.get("attributes").context("missing attributes")?;
    let title = attrs.get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let duration_str = attrs.get("duration")
        .and_then(|v| v.as_str())
        .unwrap_or("PT0S");
    let duration = parse_iso_duration(duration_str);

    let album_id = track_data
        .get("relationships")
        .and_then(|v| v.get("albums"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut album = Album {
        id: album_id,
        title: "Unknown".to_string(),
        number_of_tracks: None,
        release_date: None,
        cover: None,
        artist: None,
        audio_quality: None,
        media_metadata: None,
        added_at: None,
    };

    let mut cover_url: Option<String> = None;
    let mut artist: Option<ArtistRef> = None;
    let mut artists: Vec<ArtistRef> = Vec::new();
    let mut artist_map = HashMap::new();

    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if let Some("artists") = item.get("type").and_then(|v| v.as_str()) {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if let Some(attrs) = item.get("attributes").and_then(|v| v.as_object()) {
                        let name = attrs.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        artist_map.insert(id.to_string(), name);
                    }
                }
            } else if let Some("albums") = item.get("type").and_then(|v| v.as_str()) {
                if let Some(item_id) = item.get("id").and_then(|v| v.as_str()) {
                    if item_id.parse::<u64>().ok() == Some(album_id) {
                        if let Some(attrs) = item.get("attributes").and_then(|v| v.as_object()) {
                            album.title = attrs.get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            album.release_date = attrs.get("releaseDate")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                    }
                }
            } else if let Some("artworks") = item.get("type").and_then(|v| v.as_str()) {
                if let Some(files) = item.get("attributes")
                    .and_then(|v| v.get("files"))
                    .and_then(|v| v.as_array()) {
                    if let Some(file) = files.iter().find(|f| {
                        f.get("meta")
                            .and_then(|m| m.get("width"))
                            .and_then(|w| w.as_u64())
                            .map(|w| w == 320)
                            .unwrap_or(false)
                    }) {
                        cover_url = file.get("href")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                }
            }
        }
    }

    if let Some(artist_ids) = track_data
        .get("relationships")
        .and_then(|v| v.get("artists"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array()) {
        for artist_ref in artist_ids {
            if let Some(id) = artist_ref.get("id").and_then(|v| v.as_str()) {
                if let Some(name) = artist_map.get(id) {
                    let artist_ref = ArtistRef {
                        name: name.clone(),
                    };
                    if artist.is_none() {
                        artist = Some(artist_ref.clone());
                    }
                    artists.push(artist_ref);
                }
            }
        }
    }

    let track = Track {
        id: track_id,
        title,
        duration,
        artist,
        artists,
        album,
        audio_quality: None,
        media_metadata: None,
        added_at: None,
    };

    Ok((track, cover_url))
}

pub struct ApiClient {
    http: reqwest::Client,
    token: RwLock<String>,
    config: Config,
}

impl ApiClient {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client");
        let token = config.access_token.clone().unwrap_or_default();
        Self {
            http,
            token: RwLock::new(token),
            config,
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, params: &[(&str, String)]) -> Result<T> {
        let token = self.token.read().await.clone();
        let url = format!("{BASE}{path}");

        // Build base params that Tidal requires on every request
        let mut all_params: Vec<(&str, String)> = vec![
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }
        all_params.extend_from_slice(params);

        tracing::debug!("API request: GET {}", path);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("HTTP request failed")?;

        let status = resp.status();
        tracing::debug!("API response: {} {}", status.as_u16(), path);

        if status == reqwest::StatusCode::UNAUTHORIZED {
            tracing::info!("Token expired, refreshing...");
            let new_token = refresh_token_async(&self.config, &self.http).await?;
            let new_access = new_token.access_token.clone();
            *self.token.write().await = new_access.clone();

            return Ok(self
                .http
                .get(&url)
                .bearer_auth(&new_access)
                .query(&all_params)
                .send()
                .await?
                .error_for_status()?
                .json::<T>()
                .await?);
        }

        let bytes = resp.error_for_status().map_err(|e| {
            let status = e.status().unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
            tracing::error!("API error {} on {}: {}", status.as_u16(), path, e);
            e
        })?.bytes().await?;
        serde_json::from_slice::<T>(&bytes).map_err(|e| {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
            tracing::error!("JSON parse error on {}: {} — body: {}", path, e, snippet);
            anyhow::anyhow!("{e} — body: {snippet}")
        })
    }


    async fn get_openapi<T: DeserializeOwned>(&self, path: &str, params: &[(&str, String)]) -> Result<T> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{path}");
        let mut all_params = vec![("countryCode", self.config.country_code.clone())];
        all_params.extend_from_slice(params);

        let bytes = self.http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("openapi GET failed")?
            .error_for_status()?
            .bytes()
            .await?;

        serde_json::from_slice::<T>(&bytes).map_err(|e| {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(400).collect();
            anyhow::anyhow!("{e} — body: {snippet}")
        })
    }

    async fn post_openapi_json(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{path}");
        self.http
            .post(&url)
            .bearer_auth(&token)
            .query(&[("countryCode", &self.config.country_code)])
            .json(body)
            .send()
            .await
            .context("openapi POST failed")?
            .error_for_status()?;
        Ok(())
    }

    async fn delete_openapi_json(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{path}");
        self.http
            .delete(&url)
            .bearer_auth(&token)
            .query(&[("countryCode", &self.config.country_code)])
            .json(body)
            .send()
            .await
            .context("openapi DELETE failed")?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_user_collection_playlists(&self, cursor: Option<&str>) -> Result<(Vec<Playlist>, Option<String>)> {
        let mut params = vec![("include", "items".to_string())];
        if let Some(c) = cursor {
            params.push(("page[cursor]", c.to_string()));
        }

        let page: OpenApiRelPage = self.get_openapi(
            "/userCollectionPlaylists/me/relationships/items",
            &params,
        ).await?;

        let attrs: std::collections::HashMap<String, OpenApiPlaylistAttrs> = page.included
            .into_iter()
            .filter_map(|r| r.attributes.map(|a| (r.id, a)))
            .collect();

        let playlists = page.data.into_iter().filter_map(|r| {
            let attr = attrs.get(&r.id)?;
            let added_at = r.meta.and_then(|m| m.added_at);
            Some(Playlist {
                uuid: r.id,
                title: attr.name.clone(),
                number_of_tracks: attr.number_of_items,
                description: None,
                cover: None,
                added_at,
            })
        }).collect();

        let next_cursor = page.links.and_then(|l| l.meta).and_then(|m| m.next_cursor);
        Ok((playlists, next_cursor))
    }

    async fn post_form(&self, path: &str, form: &[(&str, String)]) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{BASE}{path}");

        let mut all_params: Vec<(&str, String)> = vec![
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }

        self.http
            .post(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .form(form)
            .send()
            .await
            .context("HTTP POST failed")?
            .error_for_status()?;
        Ok(())
    }

    fn uid(&self) -> Result<u64> {
        self.config.user_id.context("user_id not set — re-run to re-authenticate")
    }

    // ── Artists ───────────────────────────────────────────────────────────────

    pub async fn get_favorite_artists(&self, offset: u32, limit: u32) -> Result<Page<FavoriteArtistEntry>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/favorites/artists"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    pub async fn get_artist_top_tracks(&self, artist_id: u64, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/artists/{artist_id}/toptracks"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_artist_albums(&self, artist_id: u64, limit: u32) -> Result<Page<Album>> {
        self.get(
            &format!("/artists/{artist_id}/albums"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_artist_eps(&self, artist_id: u64, limit: u32) -> Result<Page<Album>> {
        self.get(
            &format!("/artists/{artist_id}/albums"),
            &[
                ("limit", limit.to_string()),
                ("filter", "EPSANDSINGLES".to_string()),
            ],
        )
        .await
    }

    pub async fn get_artist_singles(&self, artist_id: u64, limit: u32) -> Result<Page<Album>> {
        self.get(
            &format!("/artists/{artist_id}/albums"),
            &[
                ("limit", limit.to_string()),
                ("filter", "EPSANDSINGLES".to_string()),
            ],
        )
        .await
    }

    pub async fn get_artist_bio(&self, artist_id: u64) -> Result<ArtistBioResponse> {
        self.get(&format!("/artists/{artist_id}/bio"), &[]).await
    }

    // ── Playlists ─────────────────────────────────────────────────────────────

    pub async fn get_favorite_playlists(&self) -> Result<Page<FavoritePlaylistEntry>> {
        tracing::debug!("Retrieving user's favorite playlists");

        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}/userCollectionPlaylists/me?locale=en-US&include=items.items");

        tracing::debug!("API request: GET /userCollectionPlaylists/me");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        tracing::debug!("API response: {} /userCollectionPlaylists/me", status);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /userCollectionPlaylists/me: {}", status, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (playlists, total) = parse_v2_user_playlists(&api_resp)?;
        tracing::debug!("Retrieved {} favorite playlists (total: {})", playlists.len(), total);

        let entries = playlists.into_iter().map(|p| FavoritePlaylistEntry {
            created: p.added_at.clone(),
            playlist: p,
        }).collect();

        Ok(Page {
            items: entries,
            total,
        })
    }

    pub async fn save_playlist(&self, uuid: &str) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": uuid, "type": "playlists"}]});
        self.post_openapi_json("/userCollectionPlaylists/me/relationships/items", &body).await
    }

    pub async fn remove_playlist(&self, uuid: &str) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": uuid, "type": "playlists"}]});
        self.delete_openapi_json("/userCollectionPlaylists/me/relationships/items", &body).await
    }

    pub async fn get_playlist_tracks(&self, uuid: &str, next_url: Option<&str>) -> Result<(Vec<Track>, u32, Option<String>, Option<String>, Option<String>)> {
        let token = self.token.read().await.clone();

        let url = if let Some(next) = next_url {
            // Use the next URL provided by the previous response
            // Ensure it includes the include=items.artists parameter
            let base_url = format!("{OPENAPI_BASE}{}", next);
            if base_url.contains("include=") {
                base_url
            } else {
                format!("{}&include=items.artists", base_url)
            }
        } else {
            // Initial request - build the first page URL
            format!("{OPENAPI_BASE}/playlists/{uuid}?countryCode=US&include=items.artists&include=coverArt")
        };

        tracing::debug!("Fetching playlist tracks from: {}", url);

        let resp = self.http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        tracing::debug!("API response: {} /playlists/{uuid}", status);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /playlists/{}: {}", status, uuid, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;


        let (tracks, total, next_url, description, cover) = parse_v2_playlist_tracks(&api_resp)?;
        if let Some(first_track) = tracks.first() {
            if let Some(ref _url) = next_url {
                tracing::debug!("Loaded {} tracks, first: '{}', has next page", tracks.len(), first_track.title);
            } else {
                tracing::debug!("Loaded {} tracks, first: '{}', NO more pages", tracks.len(), first_track.title);
            }
        }
        Ok((tracks, total, next_url, description, cover))
    }

    pub async fn get_playlist_relationship_items(&self, next_url: &str, total: u32) -> Result<(Vec<Track>, u32, Option<String>)> {
        let token = self.token.read().await.clone();

        // Ensure the next URL includes the include=items.artists parameter
        let url = {
            let base_url = format!("{OPENAPI_BASE}{}", next_url);
            if base_url.contains("include=") {
                base_url
            } else {
                format!("{}&include=items.artists", base_url)
            }
        };
        tracing::debug!("Fetching playlist relationship items from: {}", url);

        let resp = self.http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        tracing::debug!("API response: {} /relationships/items", status);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /relationships/items: {}", status, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, _, next_url) = parse_playlist_relationship_items(&api_resp, total)?;
        if let Some(first_track) = tracks.first() {
            if next_url.is_some() {
                tracing::debug!("Loaded {} tracks, first: '{}', has next page", tracks.len(), first_track.title);
            } else {
                tracing::debug!("Loaded {} tracks, first: '{}', NO more pages", tracks.len(), first_track.title);
            }
        }
        Ok((tracks, total, next_url))
    }

    // ── Favorites ─────────────────────────────────────────────────────────────

    pub async fn get_favorite_albums(&self, offset: u32, limit: u32) -> Result<Page<FavoriteAlbumEntry>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/favorites/albums"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("order", "DATE".to_string()),
                ("orderDirection", "DESC".to_string()),
            ],
        )
        .await
    }

    pub async fn add_favorite_album(&self, album_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.post_form(
            &format!("/users/{uid}/favorites/albums"),
            &[("albumId", album_id.to_string())],
        ).await
    }

    pub async fn remove_favorite_album(&self, album_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.delete(&format!("/users/{uid}/favorites/albums/{album_id}")).await
    }

    pub async fn get_favorite_tracks(&self, offset: u32, limit: u32) -> Result<Page<FavoriteTrackEntry>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/favorites/tracks"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("order", "DATE".to_string()),
                ("orderDirection", "DESC".to_string()),
            ],
        )
        .await
    }

    // ── Search ────────────────────────────────────────────────────────────────

    pub async fn search(&self, query: &str, limit: u32) -> Result<SearchResponse> {
        self.get(
            "/search",
            &[
                ("query", query.to_string()),
                ("types", "ARTISTS,ALBUMS,TRACKS,PLAYLISTS".to_string()),
                ("limit", limit.to_string()),
            ],
        )
        .await
    }

    pub async fn search_artists(&self, query: &str, limit: u32) -> Result<Vec<Artist>> {
        let resp: SearchResponse = self.get(
            "/search",
            &[
                ("query", query.to_string()),
                ("types", "ARTISTS".to_string()),
                ("limit", limit.to_string()),
            ],
        )
        .await?;
        Ok(resp.artists.map(|p| p.items).unwrap_or_default())
    }

    // ── Albums ────────────────────────────────────────────────────────────────

    pub async fn get_album(&self, album_id: u64) -> Result<Album> {
        self.get(&format!("/albums/{album_id}"), &[]).await
    }

    pub async fn get_album_tracks(&self, album_id: u64) -> Result<Page<Track>> {
        self.get(
            &format!("/albums/{album_id}/tracks"),
            &[("limit", "50".to_string())],
        )
        .await
    }

    // ── Radio ─────────────────────────────────────────────────────────────────

    pub async fn get_track_radio(&self, track_id: u64, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/tracks/{track_id}/radio"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_artist_radio(&self, artist_id: u64, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/artists/{artist_id}/radio"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    // ── Lyrics ───────────────────────────────────────────────────────────────

    pub async fn get_track_lyrics(&self, track_id: u64) -> Result<LyricsResponse> {
        self.get(&format!("/tracks/{track_id}/lyrics"), &[]).await
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    /// Fetch raw bytes from a public URL (e.g. Tidal's cover art CDN).
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).send().await?;
        let status = resp.status();

        let bytes = resp.error_for_status().map_err(|e| {
            tracing::warn!("Failed to fetch bytes ({}): {}", status.as_u16(), e);
            e
        })?.bytes().await?;

        tracing::debug!("Fetched {} bytes", bytes.len());
        Ok(bytes.to_vec())
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{BASE}{path}");

        let mut all_params: Vec<(&str, String)> = vec![
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }

        self.http
            .delete(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("HTTP DELETE failed")?
            .error_for_status()?;
        Ok(())
    }

    pub async fn add_favorite_track(&self, track_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.post_form(
            &format!("/users/{uid}/favorites/tracks"),
            &[("trackId", track_id.to_string())],
        ).await
    }

    pub async fn follow_artist(&self, artist_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.post_form(
            &format!("/users/{uid}/favorites/artists"),
            &[("artistId", artist_id.to_string())],
        ).await
    }

    pub async fn remove_favorite_track(&self, track_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.delete(&format!("/users/{uid}/favorites/tracks/{track_id}")).await
    }

    pub async fn unfollow_artist(&self, artist_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.delete(&format!("/users/{uid}/favorites/artists/{artist_id}")).await
    }

    pub async fn get_stream_url(&self, track_id: u64) -> Result<String> {
        // Quality fallback chain for streaming.
        //
        // | Quality          | Manifest MIME type         | Container   | Actual codec  |
        // |------------------|----------------------------|-------------|---------------|
        // | LOSSLESS         | application/vnd.tidal.bts  | audio/flac  | FLAC (raw)    |
        // | HI_RES_LOSSLESS  | application/dash+xml       | audio/mp4   | FLAC or AAC   |
        // | HIGH             | application/vnd.tidal.bts  | audio/mp4   | AAC           |
        //
        // LOSSLESS → BTS manifest with `codecs: "flac"` → guaranteed raw FLAC.
        // HI_RES_LOSSLESS → DASH manifest where codecs MAY be "flac" or "mp4a.40.2".
        // Strategy: try LOSSLESS first (guaranteed FLAC), then HI_RES_LOSSLESS
        // (only if its DASH codec is actually FLAC), then HIGH as last resort.
        const QUALITIES: &[&str] = &["LOSSLESS", "HI_RES_LOSSLESS", "HIGH"];
        const MAX_RETRIES: usize = 3;
        let path = format!("/tracks/{track_id}/playbackinfopostpaywall");
        let debug = std::env::var("RIPTIDE_QUALITY_DEBUG").is_ok();
        let token = self.token.read().await.clone();
        let base_url = format!("{BASE}{path}");

        for retry_attempt in 0..=MAX_RETRIES {
            if retry_attempt > 0 {
                let delay_ms = 300 * retry_attempt as u64;
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                if debug {
                    eprintln!("[quality] track {track_id}: retry attempt {retry_attempt}");
                }
            }

            let mut all_failed_with_asset_not_ready = true;

            for &quality in QUALITIES {
            let mut all_params: Vec<(&str, String)> = vec![
                ("countryCode", self.config.country_code.clone()),
                ("audioquality", quality.to_string()),
                ("playbackmode", "STREAM".to_string()),
                ("assetpresentation", "FULL".to_string()),
            ];
            if let Some(sid) = &self.config.session_id {
                all_params.push(("sessionId", sid.clone()));
            }

            let resp = self
                .http
                .get(&base_url)
                .bearer_auth(&token.clone())
                .query(&all_params)
                .send()
                .await
                .context("HTTP request failed")?;

            let status = resp.status();

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                let error_msg = if body.is_empty() {
                    status.to_string()
                } else {
                    // Truncate to first 200 chars for readability
                    let snippet: String = body.chars().take(200).collect();
                    format!("{}: {}", status, snippet)
                };

                if debug {
                    eprintln!("[quality] track {track_id}: {quality} request failed — {error_msg}");
                }

                let entitlement_denied = matches!(
                    status,
                    reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
                );

                if entitlement_denied {
                    tracing::debug!("Track {track_id} ({quality}): {error_msg}");
                    // Check if this is an "asset not ready" error (subStatus 4005)
                    let is_asset_not_ready = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v.get("subStatus").and_then(|s| s.as_u64()))
                        .map(|code| code == 4005)
                        .unwrap_or(false);

                    if !is_asset_not_ready {
                        all_failed_with_asset_not_ready = false;
                    }
                    continue;
                }
                return Err(anyhow::anyhow!("Track {track_id} ({quality}): {error_msg}"));
            }

            let body = resp.text().await?;
            let info: PlaybackInfo = serde_json::from_str(&body)
                .context("parse playback info response")?;

            let mime = info.manifest_mime_type.clone();
            if debug {
                let aq = info.audio_quality.as_deref().unwrap_or("?");
                eprintln!(
                    "[quality] track {track_id}: requested {quality}, \
                     server returned manifestMimeType={mime}, \
                     audioQuality={aq} (200 OK)",
                );
            }

            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&info.manifest)
                .context("base64 decode of manifest")?;

            match mime.as_str() {
                "application/vnd.tidal.bts" => {
                    let manifest: BtsManifest = serde_json::from_slice(&bytes)
                        .context("parse BTS manifest")?;

                    if manifest.urls.is_empty() {
                        if debug {
                            eprintln!("[quality] track {track_id}: BTS manifest has empty urls — skip");
                        }
                        continue;
                    }

                    let codec = manifest.codecs.as_deref().unwrap_or("(missing)");
                    if debug {
                        eprintln!(
                            "[quality] track {track_id}: BTS codecs={codec}, \
                             urls={} segment(s)",
                            manifest.urls.len(),
                        );
                    }

                    // BTS with FLAC codec → real lossless.
                    if manifest.is_flac() {
                        if debug {
                            eprintln!("[quality] track {track_id}: ✓ FLAC stream accepted ({quality})");
                        }
                        if manifest.urls.len() == 1 {
                            return Ok(manifest.urls.into_iter().next().unwrap());
                        }
                        let m3u8 = build_flac_m3u8(track_id, &manifest.urls);
                        return Ok(m3u8);
                    }

                    // BTS with non-FLAC codec.
                    // For LOSSLESS requests: the API downgraded us → skip.
                    // For HIGH requests: this is expected AAC → accept.
                    if quality == "HIGH" {
                        if debug {
                            eprintln!("[quality] track {track_id}: accepting AAC stream (HIGH)");
                        }
                        if let Some(url) = manifest.urls.into_iter().next() {
                            return Ok(url);
                        }
                    } else {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: BTS codec is '{codec}' \
                                 (not flac) for {quality} request — falling through",
                            );
                        }
                        continue;
                    }
                }
                "application/dash+xml" => {
                    let xml = String::from_utf8_lossy(&bytes);

                    let sets = find_adaptation_sets(&xml);
                    let has_flac = sets.iter().any(|s| s.codecs == "flac");

                    if debug {
                        let codecs: Vec<&str> = sets.iter().map(|s| s.codecs.as_str()).collect();
                        eprintln!(
                            "[quality] track {track_id}: DASH with {} AdaptationSet(s), \
                             codecs={:?}, has_flac={has_flac}",
                            sets.len(), codecs,
                        );
                    }

                    if (quality == "LOSSLESS" || quality == "HI_RES_LOSSLESS") && !has_flac {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: DASH has no FLAC codec \
                                 — falling through to next tier",
                            );
                        }
                        continue;
                    }

                    if debug {
                        eprintln!("[quality] track {track_id}: ✓ DASH/FLAC accepted ({quality})");
                    }
                    let hls = dash_to_hls(track_id, &xml)
                        .context("convert DASH manifest to HLS")?;
                    return Ok(hls);
                }
                _ => {
                    if debug {
                        eprintln!(
                            "[quality] track {track_id}: unknown manifest MIME type '{mime}' — skip",
                        );
                    }
                    continue;
                }
            }
            }

            // If not all failures were asset-not-ready, don't retry (permanent error)
            if !all_failed_with_asset_not_ready {
                break;
            }

            // If this was the last retry, break so we can return the error
            if retry_attempt == MAX_RETRIES {
                break;
            }
            // Otherwise continue to next retry
        }

        Err(anyhow::anyhow!("no stream URL available for track {track_id}"))
    }

    pub async fn get_track_details(&self, track_id: u64) -> Result<(Track, Option<String>)> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}/tracks/{track_id}?countryCode=US&include=albums.coverArt&include=artists");

        tracing::debug!("Fetching track details for track {track_id}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("API error {} fetching track {track_id}: {}", status, body);
            anyhow::bail!("HTTP {} fetching track details", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (track, cover_url) = parse_v2_track_details(&api_resp)?;
        Ok((track, cover_url))
    }

    async fn get_mixes(&self, endpoint: &str) -> Result<Vec<Playlist>> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{endpoint}?locale=en-US&include=items.items");

        tracing::debug!("API request: GET {endpoint}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}", resp.status());
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let mut playlists = Vec::new();

        // Build a map of playlist IDs to their details from the included array
        let mut playlist_map = HashMap::new();
        if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
            for item in included {
                if let Some("playlists") = item.get("type").and_then(|v| v.as_str()) {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        playlist_map.insert(id.to_string(), item.clone());
                    }
                }
            }
        }

        // Use the order from the data array to create playlists in the correct order
        if let Some(data) = api_resp.get("data").and_then(|v| v.as_array()) {
            if !data.is_empty() {
                for item_ref in data {
                    if let Some(id) = item_ref.get("id").and_then(|v| v.as_str()) {
                        if let Some(playlist_obj) = playlist_map.get(id) {
                            if let Some(attrs) = playlist_obj.get("attributes").and_then(|v| v.as_object()) {
                                let title = attrs.get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let number_of_tracks = attrs.get("numberOfItems")
                                    .and_then(|v| v.as_u64())
                                    .map(|n| n as u32);

                                playlists.push(Playlist {
                                    uuid: id.to_string(),
                                    title,
                                    number_of_tracks,
                                                        description: None,
                                    cover: None,
                                    added_at: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Fallback: if data array was empty or missing, use all playlists from included
        if playlists.is_empty() {
            for (_, playlist_obj) in playlist_map.iter() {
                if let Some(attrs) = playlist_obj.get("attributes").and_then(|v| v.as_object()) {
                    if let Some(id) = playlist_obj.get("id").and_then(|v| v.as_str()) {
                        let title = attrs.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let number_of_tracks = attrs.get("numberOfItems")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32);

                        playlists.push(Playlist {
                            uuid: id.to_string(),
                            title,
                            number_of_tracks,
                                        description: None,
                            cover: None,
                            added_at: None,
                        });
                    }
                }
            }
            playlists.sort_by(|a, b| {
                let get_num = |s: &str| {
                    s.split_whitespace()
                        .last()
                        .and_then(|w| w.parse::<u32>().ok())
                };
                let a_num = get_num(&a.title);
                let b_num = get_num(&b.title);
                match (a_num, b_num) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    _ => a.title.cmp(&b.title),
                }
            });
        }

        Ok(playlists)
    }

    pub async fn get_daily_mixes(&self) -> Result<Vec<Playlist>> {
        self.get_mixes("/userDailyMixes/me").await
    }

    pub async fn get_discovery_mixes(&self) -> Result<Vec<Playlist>> {
        self.get_mixes("/userDiscoveryMixes/me").await
    }

    pub async fn get_mix_tracks(&self, mix_id: &str, offset: u32) -> Result<(Vec<Track>, u32)> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}/playlists/{mix_id}?countryCode=US&include=items.artists&offset={offset}&limit=100");

        tracing::debug!("API request: GET /playlists/{} with include=items.artists", mix_id);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        tracing::debug!("API response: {} /playlists/{}", status, mix_id);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /playlists/{}: {}", status, mix_id, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, total, _, _, _) = parse_v2_playlist_tracks(&api_resp)?;
        Ok((tracks, total))
    }

    pub async fn get_new_release_mixes(&self) -> Result<Vec<Playlist>> {
        self.get_mixes("/userNewReleaseMixes/me").await
    }
}

// ── Multi-segment FLAC playlist ────────────────────────────────────────────────

/// Build a simple M3U8 playlist for multi-segment raw FLAC URLs so mpv can
/// play them gaplessly in sequence.
fn build_flac_m3u8(track_id: u64, urls: &[String]) -> String {
    let mut m3u8 = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    // Each segment is a standalone FLAC file; mpv handles concatenation natively.
    for url in urls {
        // We don't know exact durations upfront, but mpv will determine them
        // from the FLAC stream headers. Use a generous placeholder.
        m3u8.push_str("#EXTINF:10.0,\n");
        m3u8.push_str(url);
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    let playlist_path = format!("/tmp/riptide_hls_{track_id}.m3u8");
    let _ = std::fs::write(&playlist_path, &m3u8);
    format!("http://127.0.0.1:{}/{track_id}.m3u8", crate::manifest::PORT)
}

// ── DASH → HLS conversion ─────────────────────────────────────────────────────

/// Represents a single `<AdaptationSet>` found in the DASH manifest.
struct DashAdaptationSet {
    /// The `codecs` attribute from the AdaptationSet or Representation element.
    codecs: String,
    /// Position of this AdaptationSet in the original XML (byte offset of opening tag).
    _offset: usize,
}

/// Find all AdaptationSet elements and their codec info.
/// Returns them so we can prefer FLAC over AAC.
fn find_adaptation_sets(xml: &str) -> Vec<DashAdaptationSet> {
    let mut sets = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find("<AdaptationSet") {
        let set_start = pos;
        let fragment = &rest[pos..];
        // Find codecs in the AdaptationSet or its Representation child.
        let codecs = dash_attr(fragment, "codecs").unwrap_or_default();
        let offset = xml.len() - rest.len() + set_start;
        sets.push(DashAdaptationSet { codecs, _offset: offset });
        // Advance past this AdaptationSet to find the next one.
        if let Some(end) = fragment.find("</AdaptationSet>") {
            rest = &fragment[end + "</AdaptationSet>".len()..];
        } else {
            break;
        }
    }
    sets
}

/// Convert a Tidal DASH manifest to an HLS playlist served via local HTTP.
///
/// When the manifest contains multiple AdaptationSets (e.g. AAC and FLAC),
/// we prefer the FLAC one so mpv plays real lossless audio.
fn dash_to_hls(track_id: u64, xml: &str) -> anyhow::Result<String> {
    // If there are multiple AdaptationSets, try to find a FLAC one.
    let adaptation_sets = find_adaptation_sets(xml);

    // Determine which region of the XML to use for attribute extraction.
    // If we have a FLAC adaptation set, extract attributes from within it.
    let search_region = if adaptation_sets.len() > 1 {
        if let Some(flac_set) = adaptation_sets.iter().find(|s| s.codecs == "flac") {
            // Extract from just this AdaptationSet's region of the XML.
            let start = flac_set._offset;
            let rest = &xml[start..];
            if let Some(end) = rest.find("</AdaptationSet>") {
                &rest[..end + "</AdaptationSet>".len()]
            } else {
                xml
            }
        } else {
            xml
        }
    } else {
        xml
    };

    let codecs = dash_attr(search_region, "codecs").unwrap_or_default();

    let init_url = dash_attr(search_region, "initialization")
        .context("no initialization URL in DASH manifest")?;
    let media_tmpl = dash_attr(search_region, "media")
        .context("no media template in DASH manifest")?;
    let timescale: f64 = dash_attr(search_region, "timescale")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let start_num: u64 = dash_attr(search_region, "startNumber")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let durations = dash_segment_durations(search_region, timescale);
    anyhow::ensure!(!durations.is_empty(), "no segments in DASH manifest");

    let target = durations.iter().cloned().fold(0f64, f64::max).ceil() as u64;
    let mut m3u8 = format!(
        "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-TARGETDURATION:{target}\n"
    );
    // Include codec info so mpv knows what to expect.
    if !codecs.is_empty() {
        m3u8.push_str(&format!("#EXT-X-CODECS:{codecs}\n"));
    }
    m3u8.push_str(&format!("#EXT-X-MAP:URI=\"{init_url}\"\n"));

    for (i, dur) in durations.iter().enumerate() {
        m3u8.push_str(&format!("#EXTINF:{dur:.5},\n"));
        m3u8.push_str(&media_tmpl.replace("$Number$", &(start_num + i as u64).to_string()));
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    std::fs::write(format!("/tmp/riptide_hls_{track_id}.m3u8"), &m3u8)
        .context("write HLS playlist")?;
    Ok(format!("http://127.0.0.1:{}/{track_id}.m3u8", crate::manifest::PORT))
}

/// Extract an XML attribute value by name, checking that it isn't a substring
/// of a longer attribute name (e.g. `d` must not match `id`).
fn dash_attr(xml: &str, name: &str) -> Option<String> {
    let needle = format!("{}=\"", name);
    let mut haystack = xml;
    while let Some(pos) = haystack.find(&needle) {
        let before = pos
            .checked_sub(1)
            .and_then(|i| haystack.as_bytes().get(i).copied())
            .map(|b| b as char)
            .unwrap_or(' ');
        if !before.is_alphanumeric() && before != '_' && before != '-' {
            let start = pos + needle.len();
            let end = haystack[start..].find('"')? + start;
            return Some(haystack[start..end].to_owned());
        }
        haystack = &haystack[pos + needle.len()..];
    }
    None
}

/// Parse `<S d="..." r="..."/>` elements inside `<SegmentTimeline>`.
fn dash_segment_durations(xml: &str, timescale: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let tl_start = match xml.find("<SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let tl = &xml[tl_start..];
    let tl_end = match tl.find("</SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let mut rest = &tl[..tl_end];
    while let Some(pos) = rest.find("<S ") {
        let inner_start = pos + 3;
        let inner_end = rest[inner_start..]
            .find("/>")
            .map(|p| p + inner_start)
            .unwrap_or(rest.len());
        let elem = &rest[inner_start..inner_end];
        let d: f64 = dash_attr(elem, "d").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let r: usize = dash_attr(elem, "r").and_then(|s| s.parse().ok()).unwrap_or(0);
        let dur = d / timescale;
        for _ in 0..=r {
            out.push(dur);
        }
        rest = &rest[inner_end..];
    }
    out
}
