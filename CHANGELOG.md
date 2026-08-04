# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
