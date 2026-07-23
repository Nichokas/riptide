# Riptide

A terminal UI music player for Tidal, built with Rust.

<img width="1920" height="1080" alt="Screenshot_2026-05-12-092952" src="https://github.com/user-attachments/assets/83e390ce-1884-4d72-8486-2f406faa3d2a" />

## Features

- Browse your Tidal library: favorites, artists, playlists, and albums
- Full-text search across tracks, artists, and playlists
- Synchronized lyrics
- Album art in the sidebar and album detail view (pixel-perfect in Kitty terminal, half-block fallback elsewhere)
- Artist pictures and biography
- Queue management — add tracks, navigate to any position, remove entries, play from any point
- Gapless playback via mpv
- Audio quality indicator (Hi-Res, FLAC, MQA, AAC)
- Animated waveform progress bar

## Requirements

- **Rust** 1.85+ (2024 edition) — to build from source
- **mpv** — used as the audio backend; must be on your `PATH`
- A **Tidal** account (HiFi or HiFi Plus recommended for lossless quality)

### Installing mpv

| Platform              | Command                |
| --------------------- | ---------------------- |
| Linux (Debian/Ubuntu) | `sudo apt install mpv` |
| Linux (Arch)          | `sudo pacman -S mpv`   |
| Linux (Fedora)        | `sudo dnf install mpv` |

## Installation

### Pre-built Binaries

Download the latest binary for your platform from [GitHub Releases](https://github.com/fezzik-the-giant/riptide/releases):

```bash
# Linux x86_64
wget https://github.com/fezzik-the-giant/riptide/releases/download/vX.Y.Z/riptide-vX.Y.Z-x86_64-linux-gnu.tar.gz
tar -xzf riptide-vX.Y.Z-x86_64-linux-gnu.tar.gz
./riptide

# macOS x86_64
wget https://github.com/fezzik-the-giant/riptide/releases/download/vX.Y.Z/riptide-vX.Y.Z-x86_64-apple-darwin.tar.gz
tar -xzf riptide-vX.Y.Z-x86_64-apple-darwin.tar.gz
./riptide

# macOS ARM64 (Apple Silicon)
wget https://github.com/fezzik-the-giant/riptide/releases/download/vX.Y.Z/riptide-vX.Y.Z-aarch64-apple-darwin.tar.gz
tar -xzf riptide-vX.Y.Z-aarch64-apple-darwin.tar.gz
./riptide
```

Or install to your PATH:

```bash
tar -xzf riptide-vX.Y.Z-*.tar.gz
sudo mv riptide /usr/local/bin/
```

Verify checksums with the included `SHA256SUMS` file:

```bash
sha256sum -c SHA256SUMS
```

### Arch (AUR)

Riptide is available on the AUR and can be installed with:

```bash
paru -S riptide

# or if using yay
yay -S riptide
```

### Build from Source

Requires Rust 1.85+ and Cargo:

```bash
git clone https://github.com/fezzik-the-giant/riptide
cd riptide
cargo install --path .
```

The `riptide` binary will be placed in `~/.cargo/bin/`. Make sure that directory is on your `PATH`.

### Nix

Tested on: `x86_64-linux`.

Add riptide as your `flake.nix` input:

```nix
{
  inputs.riptide.url = "github:fezzik-the-giant/riptide";
}
```

Then add it to your home-manager:

```nix
home.packages =  (with pkgs; [
    inputs.riptide.packages.${system}.default
 ];
```

A development shell is also available:

```sh
git clone https://github.com/fezzik-the-giant/riptide
cd riptide/
nix develop
```

You can also run riptide directly:

```sh
nix run github:fezzik-the-giant/riptide
```

## First run & authentication

Riptide uses Tidal's OAuth device-authorization flow. On first launch it will print a URL and a short code:

```
╔══════════════════════════════════════════╗
║           Tidal Authorization            ║
╠══════════════════════════════════════════╣
║  Open:                                   ║
║  https://link.tidal.com/XXXXX            ║
╠══════════════════════════════════════════╣
║  Code: ABCD-1234                         ║
╚══════════════════════════════════════════╝

Waiting for authorization…
```

Open the URL in a browser, log in with your Tidal account, and enter the code. Riptide will save your tokens to the config file and launch immediately. You will not need to authenticate again unless your refresh token expires.

## Configuration

The config file lives at:

| Platform | Path                            |
| -------- | ------------------------------- |
| Linux    | `~/.config/riptide/config.json` |

It is created automatically on first run. Example:

```json
{
  "client_id": null,
  "client_secret": null,
  "access_token": "...",
  "refresh_token": "...",
  "expires_at": "2025-01-01T00:00:00+00:00",
  "user_id": 12345678,
  "country_code": "US",
  "session_id": "..."
}
```

### Using your own OAuth credentials

Riptide ships with built-in fallback credentials (provided by the open-source [tidalapi](https://github.com/tamland/python-tidal) project). If those credentials are ever revoked you can substitute your own:

1. Register a device-authorization client at [developer.tidal.com](https://developer.tidal.com)
2. Add your credentials to `config.json`:

```json
{
  "client_id": "your-client-id",
  "client_secret": "your-client-secret"
}
```

3. Delete `access_token` and `refresh_token` from the file (or delete the file entirely) to trigger a fresh login with your credentials.

## Keybindings

Press `?` in the player to view all keybinds. Here's the complete reference:

### Global

| Key       | Action           |
| --------- | ---------------- |
| `?`       | Show this help   |
| `q`       | Quit             |
| `/`       | Command palette  |
| `Tab`     | Next tab         |
| `Space`   | Play/Pause       |
| `n`       | Next track       |
| `p`       | Previous track   |
| `z`       | Toggle shuffle   |
| `+ or =`  | Volume Up        |
| `-`       | Volume Down      |
| `Esc`     | Back/Go up       |

### Navigation

| Key     | Action                          |
| ------- | ------------------------------- |
| `↑`     | Up                              |
| `↓`     | Down                            |
| `Enter` | Select/Open                     |
| `a`     | Add to queue                    |
| `f`     | Toggle favorite/follow/save     |
| `g`     | Go to artist                    |
| `s`     | Sort                            |
| `r`     | Start radio                     |
| `c`     | Copy share link (song)          |
| `C`     | Copy share link (album/playlist)|
| `→`     | Focus queue                     |

### Queue

| Key     | Action          |
| ------- | --------------- |
| `↑`     | Up              |
| `↓`     | Down            |
| `d`     | Remove track    |
| `c`     | Copy share link (song)          |
| `C`     | Copy share link (album)         |
| `Enter` | Play track      |
| `Esc`   | Close queue     |

### Search

Open search with `/` → `search` or via command palette.

| Key       | Action      |
| --------- | ----------- |
| `↑`       | Up          |
| `↓`       | Down        |
| `Tab`     | Next pane   |
| `Shift+Tab` | Prev pane |
| `Enter`   | Select/Open |
| `Esc`     | Close       |

### Command Palette

Open with `/` and type the start of a destination (Tab to autocomplete):

- `favorites` — Go to Favorites
- `artists` — Go to Artists
- `playlists` — Go to Playlists
- `search` — Open search

## Kitty terminal graphics

If you run Riptide inside [Kitty](https://sw.kovidgoyal.net/kitty/), album art and artist pictures are rendered at full pixel resolution using the Kitty graphics protocol. In any other terminal, a half-block (`▀`) approximation is used instead.

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
