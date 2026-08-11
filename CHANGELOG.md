# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.1] - 2026-08-11

### Fixed
- Reduced default logging level from debug to warn for cleaner output on installed binaries

## [0.12.0] - 2026-08-11

### Added
- Search endpoints now use v2 API with cursor-based pagination
- Modularized search and playlist functionality into dedicated modules for better code organization
- Page Up/Down support for faster navigation through lists
- Dynamic page loading for search results when cursor approaches end of list
- Startup log banner for app initialization
- Streaming buffer to reduce stuttering over mobile data

### Changed
- Improved logging clarity while reducing verbosity
- More natural scrolling behavior when navigating upwards
- Better pagination handling to prevent duplicate data fetches

### Fixed
- Toast messages now display for exactly 5 seconds instead of 20+ seconds (switched from tick-based to wall-clock timing)
- Removed ineffective retry logic from stream URL resolution
- Fixed excessive polling of favorites after each API response
- Fixed stale position/duration data persisting between tracks
- Fixed lossless audio streaming with corrected quality validation
- Improved HTTP authentication for FLAC streaming

## [0.11.0] - 2026-08-05

### Added
- New Home tab to display New Arrivals, Mixes, and Daily Discovery

### Changed
- Playlists now use v2 API for better support.
- Pagination now fetches next page of tracks only once when approaching end of list.

## [0.10.0] - 2026-08-04

### Added
- LastFM scrobbling support. Check out [the README](https://github.com/fezzik-the-giant/riptide#lastfm-scrobbling) for more information.

## [0.9.0] - 2026-08-03

### Added
- Automated install script

### Changed
- README to include instructions for automated install

## [0.8.0] - 2026-08-03

### Added
- Structured logging and ability to change logging level through environment variable

### Changed
- Replaced image rendering logic with [ratatui-image](https://crates.io/crates/ratatui-image) to include Sixel support
- Riptide now detects your terminal graphics protocol and renders image accordingly through Kitty, Sixel, or halfblock

## [0.7.3] - 2026-07-27

### Changed
- Updated styling of Queue items to be more legible and easier to distinguish.

## [0.7.2] - 2026-07-23

### Changed
- Merged Github Actions so AUR and Github Releases run in one job in parallel

## [0.7.1] - 2026-07-23

### Added
- Github Action to build binaries for Linux and MacOS and create Github Releases
- Changelog to keep track of changes -> used as release notes in Github Release

### Changed
- Updated README to include information on installing from Release binaries

## [0.7.0] - 2026-07-23

### Added
- Artist navigation: Press 'g' on any track to navigate to artist page
- Multi-artist support: If a track has multiple artists, a modal lets you select which artist to visit
- Track display now shows all artists instead of just the primary artist

### Fixed
- Quality badge alignment in track listings (badges now appear at end of line)

## [0.6.2] - 2026-07-23

### Added
- Artist Tab carousel styling with block rendering
- Artist detail now shows EPs and Singles in addition to Albums
- Share link copy functionality for tracks and albums (keybinds 'c' and 'C')

### Changed
- Updated artist view carousel styling to better indicate tab options
- Quality badges now display on all tracks

### Fixed
- Missing field from Track struct initializer
- Help modal now displays all keybinds accessibly

## [0.6.1] - 2026-07-15

### Added
- Volume control with keybinds ('+'/'-' for volume up/down)
- Help modal showing all available keybinds (press '?')
- Sorting by artist for albums and favorites

### Changed
- Improved keybind clarity and simplified controls

### Fixed
- Keybind display overflow at bottom of screen
