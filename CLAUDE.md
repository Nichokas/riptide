# Claude Code Context for Riptide

This file documents conventions, patterns, and preferences for working on the Riptide music player codebase with Claude.

## Project Overview

**Riptide** is a terminal-based music player for Tidal with a TUI interface built in Rust. It's undergoing an API modernization effort to migrate from Tidal v1 API to v2 API endpoints.

**Key Technologies:**
- Language: Rust
- TUI Framework: ratatui
- API: Tidal (v1 and v2)
- Player: mpv (via FFI)
- Build: cargo, flake.nix (Nix support)

## API Modernization Strategy

### Current Status
- Favorites (albums, tracks) → ✅ v2 API
- Follow/unfollow (artists) → ✅ v2 API  
- Search → ✅ v2 API
- Playlists → ✅ v2 API
- Albums → ✅ v2 API
- Artists → ✅ v2 API
- Radio → ✅ v2 API (dedicated module)
- Remaining: Stream URLs, Lyrics, Artist bio (low priority)

### Do NOT Attempt Large Refactors on Long-Lived Branches

**Why:** Previous attempt to migrate multiple endpoints on a feature branch that diverged from master resulted in cascading conflicts during rebase/merge attempts. Structural changes to shared types (StatefulList, Config) on different branches are nearly impossible to reconcile.

**Better Approach:**
1. Start from **latest master**
2. Migrate **one endpoint at a time** in self-contained commits
3. Test and merge back to master **immediately**
4. Repeat for next endpoint
5. This keeps branches short-lived and merges clean

### API v2 Migration Patterns

**POST to collection (add favorite/follow):**
```rust
let body = serde_json::json!({"data": [{"id": id.to_string(), "type": "type_name"}]});
self.post_openapi_json("/userCollection{Type}/me/relationships/items", &body).await
```

**DELETE from collection (remove favorite/unfollow):**
```rust
let body = serde_json::json!({"data": [{"id": id.to_string(), "type": "type_name"}]});
self.delete_openapi_json("/userCollection{Type}/me/relationships/items", &body).await
```

**Key Constants Required:**
- `pub const OPENAPI_BASE: &str = "https://openapi.tidal.com/v2";`
- `const CLIENT_VERSION: &str = "2025.7.16";` (for radio module)

### Pagination

**CRITICAL: Tidal API v2 does NOT support `page[size]` parameters.**

- Fixed result count per request (typically 40 for initial, 20 for pagination)
- Uses cursor-based pagination with `page[cursor]` parameter
- Response includes `nextCursor` in meta/links for subsequent pages
- Always include `include` parameters on pagination requests to get full objects

**Pattern:**
```rust
let min_items = if next_link.is_none() { 40 } else { 20 };
// Use cursor-based pagination, not offset-based
```

See memory: `pagination_strategy.md` and `tidal_api_pagination.md` for full details.

## Git & Commit Preferences

### User Preference: No Auto-Commits
- **DO NOT create commits** unless explicitly asked
- Let user handle all versioning and commit messages
- Exception: Only commit if user says "commit this" or similar explicit request

### Commit Message Format
- Omit `Co-Authored-By: Claude` trailer (user preference)
- Use clear, imperative messages following project style
- Reference issue numbers when applicable

### Branch Strategy for Changes
- For bug fixes / small changes: work on feature branch, test, then present for commit
- For large features: use incremental approach (see API Modernization section above)
- Always sync with master before major work via rebase or merge

## Code Style & Conventions

### File Organization
- API functions: `src/api/client.rs`
- Data models: `src/api/models.rs`
- State management: `src/app/state.rs`
- UI rendering: `src/ui.rs`
- Event handling: `src/app/responses.rs`

### Error Handling
- Use `Result<T>` with `anyhow::Context` for error propagation
- Add debug logs at API boundaries for troubleshooting

### Logging
- Always add debug logs for:
  - API parsing (especially JSON:API responses)
  - Pagination cursor changes
  - Data extraction from responses
- Use `tracing::debug!()` macro

### Pagination in Lists
- Use `StatefulList<T>` with `pagination_cursor: Option<String>` field
- Load initial: 40 items, subsequent pages: 20 items
- Implement `should_load_more()` pattern for scroll-based loading

### UI Patterns
- Use `ListViewport` for scroll management (interior mutability with Cell)
- Render functions receive `&Frame` for double-buffering
- Use ratatui's Layout/Constraint system for responsive design

## Testing Approach

- Focus on API parsing (serialize/deserialize JSON responses)
- Test pagination cursor flow
- Manual testing in terminal is primary validation for TUI features
- No mocking of database/API for critical integration tests

## Things to Avoid

### Don't:
1. **Create large feature branches** - They diverge from master and cause merge hell
2. **Assume page[size] works** - Tidal v2 only supports cursor-based pagination
3. **Omit include parameters on pagination** - Subsequent pages will return IDs only
4. **Add unnecessary error handling** - Trust framework/API guarantees at internal boundaries
5. **Remove unused code speculatively** - Delete only when certain it's unused
6. **Add feature flags for backwards compatibility** - Just change the code
7. **Mock external APIs in critical tests** - Use real API responses
8. **Add half-finished implementations** - Complete the feature or don't commit

### Do:
1. **Start fresh from master** for each new API migration
2. **Test in the TUI** before considering work done
3. **Add debug logs** at API boundaries
4. **Keep pagination cursor in responses** for subsequent page requests
5. **Use JSON:API format** for v2 API payloads (`{"data": [...]}`)
6. **Verify build succeeds** before submitting work

## Troubleshooting Common Issues

**"Pagination not working / only 20 items load"**
- Check if `page[size]` parameter is being used (remove it)
- Verify `page[cursor]` is being used instead
- Ensure cursor value is being passed to next request

**"Newly favorited item doesn't show cover art"**
- Check if track object includes full album data
- Verify `include=albums,artists` parameters are present in requests
- Track objects from favorites may need album data refresh

**"Merge conflicts during rebase"**
- **Stop and use incremental approach instead**
- Rebase large branches only if unavoidable
- Better: merge and resolve conflicts in one go, then fix any issues

## Architecture Notes

### Interior Mutability Patterns
- `RwLock<String>` for token management (async-safe)
- `Cell<usize>` for scroll offset tracking in ListViewport (single-threaded)

### State Management
- StatefulList manages both UI selection and API pagination state
- Separate pagination_cursor for API pagination vs next_offset for UI display
- Views on stack (ArtistDetail, PlaylistDetail, AlbumDetail) maintain independent state

### API Response Handling
- Parse JSON:API format into domain models
- Extract relationships and included objects manually
- Build maps for efficient lookups during transformation

## Remaining V1 API Usage & Refactoring Opportunities

### Complete V1 API Usage Inventory

**Still using v1 API endpoints:**

| Function | Endpoint | File | Priority |
|----------|----------|------|----------|
| get_favorite_artists | `/users/{uid}/favorites/artists` | client.rs:408 | Medium |
| get_artist_top_tracks | `/artists/{id}/toptracks` | client.rs:420 | High |
| get_artist_albums | `/artists/{id}/albums` | client.rs:428 | High |
| get_artist_eps | `/artists/{id}/albums?filter=ep` | client.rs:436 | High |
| get_artist_singles | `/artists/{id}/albums?filter=single` | client.rs:447 | High |
| get_artist_bio | `/artists/{id}/bio` | client.rs:458 | Low (see note) |
| get_user_playlists | `/users/{uid}/playlists` | client.rs:464 | Medium |
| get_favorite_playlists | `/users/{uid}/favorites/playlists` | client.rs:476 | Medium |
| get_favorite_albums (old) | `/users/{uid}/favorites/albums` | client.rs:530 | Deprecated |
| get_favorite_tracks (old) | `/users/{uid}/favorites/tracks` | client.rs:557 | Deprecated |
| search | `/search` | client.rs:573 | Deprecated (v2 exists) |
| get_album_tracks | `/albums/{id}/tracks` | client.rs:600 | Medium |
| get_track_radio | `/tracks/{id}/radio` | client.rs:614 | Medium |
| get_artist_radio | `/artists/{id}/radio` | client.rs:622 | Medium |
| get_track_lyrics | `/tracks/{id}/lyrics` | client.rs:632 | Low |
| get_stream_url | `/tracks/{id}/playbackinfopostpaywall` | client.rs:702 | Critical |

**Note:** Artist bio endpoint returns empty include array for all tested artists (v1 API limitation). See memory: `artist_biography_v2_incomplete.md`

**Hybrid Approach:** The codebase currently:
- Uses v1 for user favorites and artist catalog data
- Uses v2 for user collection playlists (newer playlists from web/mobile)
- Merges both sources in responses.rs (line 151-168)

### Pagination Refactoring Opportunities

**Problem:** Pagination logic repeated 4+ times across `loading.rs` and `responses.rs` with inconsistent batch sizes.

**Current Implementation Pattern (Duplicated):**
```rust
// In loading.rs - repeated 4+ times
if self.list.loading || self.list.exhausted { return; }
self.list.loading = true;
let _ = self.api_tx.send(ApiRequest::LoadXxx { offset: self.list.next_offset });

// In responses.rs - repeated 4+ times  
ApiResponse::Xxx(items, total) => {
    self.xxx.append(items, total);
    if self.xxx_sort.is_none() {
        self.xxx.items.sort_by(...);
    }
}
```

**Batch Size Inconsistencies:**
- `get_favorite_artists`: 50 items (mod.rs:129)
- `get_user_playlists`: 100 items (mod.rs:142)
- `get_favorite_albums`: 50 items (mod.rs:177)
- `get_favorite_tracks`: 50 items (mod.rs:189)
- Artist catalog requests: 20-30 items (varies)

**Refactoring Opportunity:** Create centralized pagination helper with configurable limits

### Code Duplication & Standardization Opportunities

**1. Query Parameter Building (4 instances)**
- Files: `client.rs` lines 240-247, 297-298, 383-393, 658-668
- Pattern: Build `all_params` vec, add countryCode, optionally sessionId, extend with params
- **Opportunity:** Extract to `build_api_params()` helper

**2. Error Response Parsing (2 instances)**
- Files: `client.rs` lines 281-290 (300 char snippet), 311-314 (400 char snippet)
- Pattern: Log error with body snippet, return formatted error
- **Opportunity:** Extract to `parse_error_response()` helper

**3. Favorite/Collection Management (4 instances)**
- Files: `responses.rs` lines 35-39, 43-48, 281-286, 288-293
- Pattern: `.retain()`, `total.saturating_sub()`, adjust selection index
- **Opportunity:** Extract to `remove_item_from_list()` helper

**4. Duplicate Deduplication (2 instances)**
- Files: `responses.rs` lines 20-24, 59-64
- Pattern: Filter out items already in collection using HashSet
- **Opportunity:** Extract to `deduplicate_by_id()` helper

**5. UI List Item Rendering (75+ instances)**
- Files: `ui.rs` lines 654-1244+
- Pattern: Calculate visibility, build ListItem with prefix + title + badge, apply styles
- **Opportunity:** Extract to `create_list_item()` or styling component helper

**6. Data Filtering & Transformation (8 instances)**
- Files: `responses.rs` lines 20-24, 59-64, 105-107, 122-124
- Pattern: `.iter().map()`, `.filter()`, `.collect()` with custom predicates
- **Opportunity:** Extract filter predicates to reusable functions

**7. Queue Extension (2 instances)**
- Files: `responses.rs` lines 200-222, `playback.rs` lines 21, 48
- Pattern: Extend queue with shuffle handling, track source/offset
- **Opportunity:** Extract to `extend_queue_with_shuffle()` helper

### Refactoring Priority Matrix

| Helper | Impact | Effort | Files Affected | Lines Saved | Priority |
|--------|--------|--------|-----------------|------------|----------|
| Pagination | High | Medium | loading.rs, responses.rs | 40+ | **P0** |
| Query Param Builder | High | Low | client.rs (4 places) | 60+ | **P0** |
| Error Parser | Medium | Low | client.rs (2 places) | 20+ | P1 |
| Collection Manager | Medium | Low | responses.rs (4 places) | 30+ | P1 |
| Deduplication | Medium | Low | responses.rs (2 places) | 15+ | P1 |
| UI List Item | High | High | ui.rs (75+ places) | 200+ | P2 |
| Queue Extension | Low | Medium | responses.rs, playback.rs | 25+ | P2 |

### Recommended Refactoring Sequence

1. **Phase 1 (Quick Wins):** Query param builder + Error parser (P0 items)
   - Low effort, high code clarity improvement
   - Estimated: 2-3 hours

2. **Phase 2 (Core Stability):** Pagination helper + Collection manager
   - Medium effort, eliminates major duplication
   - Estimated: 4-5 hours

3. **Phase 3 (Polish):** UI rendering helper (if time permits)
   - High effort, significant code reduction
   - Estimated: 6-8 hours

## Related Documentation

See project memory for detailed notes:
- `api_refactor_lessons.md` - Lessons from v1→v2 migration attempts
- `api_v2_patterns.md` - Working patterns for v2 API
- `pagination_strategy.md` - Consistent pagination approach
- `tidal_api_pagination.md` - Tidal API specifics
- `logging_strategy.md` - Debug logging guidelines
