// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::de::DeserializeOwned;
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

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("HTTP request failed")?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
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

        let bytes = resp.error_for_status()?.bytes().await?;
        serde_json::from_slice::<T>(&bytes).map_err(|e| {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
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
                created: None,
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

    pub async fn get_user_playlists(&self, offset: u32, limit: u32) -> Result<Page<Playlist>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/playlists"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    pub async fn get_favorite_playlists(&self, limit: u32) -> Result<Page<FavoritePlaylistEntry>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/favorites/playlists"),
            &[("limit", limit.to_string()), ("offset", "0".to_string())],
        )
        .await
    }

    pub async fn save_playlist(&self, uuid: &str) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": uuid, "type": "playlists"}]});
        self.post_openapi_json("/userCollectionPlaylists/me/relationships/items", &body).await
    }

    pub async fn remove_playlist(&self, uuid: &str) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": uuid, "type": "playlists"}]});
        self.delete_openapi_json("/userCollectionPlaylists/me/relationships/items", &body).await
    }

    pub async fn get_playlist_tracks(&self, uuid: &str, offset: u32, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/playlists/{uuid}/tracks"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
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
        Ok(self.http.get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
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
        let path = format!("/tracks/{track_id}/playbackinfopostpaywall");
        let debug = std::env::var("RIPTIDE_QUALITY_DEBUG").is_ok();

        for &quality in QUALITIES {
            let result: Result<PlaybackInfo> = self.get(
                &path,
                &[
                    ("audioquality", quality.to_string()),
                    ("playbackmode", "STREAM".to_string()),
                    ("assetpresentation", "FULL".to_string()),
                ],
            ).await;

            match result {
                Ok(info) => {
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

                            if quality == "HI_RES_LOSSLESS" && !has_flac {
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
                Err(e) => {
                    if debug {
                        let status = e.downcast_ref::<reqwest::Error>()
                            .and_then(|re| re.status());
                        eprintln!(
                            "[quality] track {track_id}: {quality} request failed \
                             (status={status:?}): {e}",
                        );
                    }
                    let status = e.downcast_ref::<reqwest::Error>()
                        .and_then(|re| re.status());
                    let entitlement_denied = matches!(
                        status,
                        Some(reqwest::StatusCode::UNAUTHORIZED) | Some(reqwest::StatusCode::FORBIDDEN)
                    );
                    if entitlement_denied {
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(anyhow::anyhow!("no stream URL available for track {track_id}"))
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
