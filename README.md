# ytm-cli

YouTube Music in your terminal. A Rust TUI over YouTube Music's internal API,
with playback through `mpv`.

Browse your playlists, library songs, albums and artists; search; play; edit the
queue; create, rename and delete playlists; add and remove tracks. Album art in
graphics-capable terminals, media keys via MPRIS.

## Terms of service

**This uses YouTube Music's internal (`youtubei/v1`) API and `yt-dlp`. That is
against YouTube's Terms of Service, whether or not you pay for Premium.** It is
built as a personal tool for a single account. There is no telemetry and nothing
leaves your machine. Use it knowing that, or don't use it.

## Requirements

| What | Version verified | Install |
|---|---|---|
| Rust | 1.96+ (tested 1.96.1) | [rustup](https://rustup.rs) |
| libmpv | 2.5.0 | Arch `mpv` · Debian/Ubuntu `libmpv-dev` · macOS `brew install mpv` |
| yt-dlp | 2026.08.19 | Arch `yt-dlp` · `pipx install yt-dlp` · `brew install yt-dlp` |

`yt-dlp` is called as a subprocess and needs to stay current — YouTube breaks
stream extraction regularly. If playback stops working, update it first.

## Install

```bash
git clone <this repo> && cd kiro
cargo build --release
./target/release/ytm-cli
```

The binary is `ytm-cli`. Symlink it to `ytm` if you want the shorter name:

```bash
ln -s "$PWD/target/release/ytm-cli" ~/.local/bin/ytm
```

## Configuration

Copy `config.example.toml` to the platform config dir and edit it:

```bash
mkdir -p ~/.config/ytm-cli
cp config.example.toml ~/.config/ytm-cli/config.toml
```

- Linux: `~/.config/ytm-cli/config.toml`
- macOS: `~/Library/Application Support/ytm-cli/config.toml`

Every key is documented in that file with its default, so a fully commented-out
copy behaves the same as no config at all. Press `,` in the app to open it in
`$EDITOR` — keybindings and theme reload when you save and exit.

## Authentication

Two paths. **Cookie auth is the one that works today** — see the note under
OAuth below.

### Cookie auth (recommended)

1. Open <https://music.youtube.com> in a browser, signed in.
2. DevTools → Network → click any request to `/youtubei/v1/...`.
3. Copy the **entire value** of the `Cookie:` request header.
4. Paste it into a file, e.g. `~/.config/ytm-cli/cookies.txt`.
5. Set in `config.toml`:

```toml
[auth]
kind = "cookie"
cookie_file = "~/.config/ytm-cli/cookies.txt"
```

The file must hold the raw header value and contain `SAPISID=`. It is **not**
Netscape `cookies.txt` format — the contents are sent verbatim as the header.

Cookies expire every few weeks. When your library suddenly reads empty, re-export
before assuming a bug.

### OAuth device flow

1. Open the [Google Cloud Console](https://console.cloud.google.com), create or
   pick a project.
2. Enable **YouTube Data API v3**.
3. Credentials → Create OAuth client → application type
   **TVs and Limited Input devices**.
4. Under Data Access, add the `https://www.googleapis.com/auth/youtube` scope.
5. Add your music account as a test user.
6. Put the client id and secret in `config.toml`:

```toml
[auth]
kind = "oauth"
client_id = "….apps.googleusercontent.com"
client_secret = "…"
```

7. Run `ytm-cli login` and follow the code and URL.

Tokens go to the OS keyring, never to disk or the log.

> **Known limitation:** Google currently rejects device-flow tokens on the
> InnerTube endpoints this app uses, so login succeeds but the library reads
> empty. The code path is correct and kept in place; use cookie auth until that
> changes.

## Commands

```bash
ytm-cli                  # the TUI
ytm-cli config           # write config.toml with every default, then open it
ytm-cli login            # OAuth device-code sign-in
ytm-cli logout           # clear the keyring entry
ytm-cli playlists        # print playlists and exit — the fastest auth check
ytm-cli cache clear      # delete cached metadata
YTM_LOG=debug ytm-cli    # verbose logging (to a file, not the screen)
```

### Changing keybindings and settings

Run this once:

```bash
ytm-cli config
```

It writes `config.toml` to the platform config dir and opens it in `$EDITOR`.
The file that lands lists **every setting and every keybinding at its default
value, commented out** — so changing one is uncommenting a line and editing it.
Nothing has to be written from scratch, and no schema has to be looked up.

```toml
[keys]
# open_filter = "/"      # <- uncomment, change to what you want
# add_to_queue = "a"
```

An existing `config.toml` is never overwritten; it is opened as it is. Add
`--no-edit` to write and print the path without opening an editor. The file is
validated when you close the editor, so a typo is reported rather than silently
ignored.

`,` inside the app does the same thing, and keybindings and the theme reload the
moment you save and exit — no restart.

From a checkout, `cargo run -p ytm-cli -- <subcommand>` works the same.

Nothing is ever printed over the TUI by design. Logs land in the platform cache
dir under `ytm-cli/logs/` — read them when the UI misbehaves.

## Keybindings

Press `?` in the app for the live list, which reflects your rebinds. Defaults:

### Navigation

| Key | Action |
|---|---|
| `j` / `k` or ↓ / ↑ | down / up |
| `h` / `l` or ← / → | up a level / into the selection |
| `g` / `G` | first / last row |
| `Ctrl+d` / `Ctrl+u` | half page down / up |
| `PageDown` / `PageUp` | half page down / up |
| `zz` | centre the selected row |
| scroll wheel | scroll the focused list |
| `1`–`7` | jump to a source (home, playlists, fav, albums, artists, search, queue) |
| `Tab` | next source |
| `Enter` | play a track, or open a playlist / artist |
| `q` / `Ctrl+c` | quit |

### Playback

| Key | Action |
|---|---|
| `Space` | play / pause |
| `n` / `p` | next / previous track |
| `f` / `b` | seek forward / back |
| `+` / `-` | volume up / down |
| `m` | mute |
| `s` | shuffle |
| `r` | repeat off / one / all |

### Queue

| Key | Action |
|---|---|
| `u` | show the queue |
| `a` | add to queue |
| `e` | play next |
| `J` / `K` | move the selected entry down / up |
| `x` | remove the selected entry (in the queue) |
| `C` | clear the queue |

### Playlists and selection

| Key | Action |
|---|---|
| `v` | mark the current row |
| `V` | visual block select — extend a range with the arrows or `j`/`k` |
| `Esc` | cancel a visual selection |
| `A` | add marked/selected tracks to a playlist |
| `x` | remove marked/selected tracks from the open playlist |
| `N` | new playlist |
| `R` | rename playlist |
| `D` | delete playlist |
| `L` | reload the current pane |

### Search and filter

Two different things: `/` narrows the rows already on screen without asking the
server, and `S` searches YouTube Music.

| Key | Action |
|---|---|
| `/` | filter the current list (title, artist, album) |
| `S` | search YouTube Music |
| `Esc` | in a filter, abandon it and restore the full list |
| `Enter` | in a filter, keep it and move to the rows |
| `Ctrl+w` | delete the previous word |
| `Ctrl+←` / `Ctrl+→` | move a word at a time |
| `Ctrl+a` / `Ctrl+e` | start / end of line |

### Appearance

| Key | Action |
|---|---|
| `t` | cycle theme |
| `,` | edit config in `$EDITOR`, reloading on exit |
| `?` | help overlay |

## Themes

Six built in: `tokyonight`, `gruvbox`, `nord`, `dracula`, and the light
`dawn` and `paper`. `theme = "auto"` reads the terminal background and falls
back to dark when it cannot tell. Press `t` to cycle at runtime.

Override one role, or write a whole theme file:

```toml
[ui]
theme = "gruvbox"
accent = "#fabd2f"
# theme_file = "~/.config/ytm-cli/theme.toml"
```

A theme file takes `accent`, `fg`, `fg_dim`, `fg_bright`, `bg_sel`, `error`,
`success`, and `preset` to start from a built-in.

## Album art

Rendered in terminals with a graphics protocol — kitty, iTerm2, or sixel.
Everywhere else it is absent, which is by design rather than a fallback: the
track list keeps its full width. Inside tmux it degrades to halfblocks.

## Development

```bash
./scripts/check.sh                      # the gate: fmt + clippy -D warnings + tests
cargo test --workspace                  # tests only
cargo test --workspace -- --ignored     # network/audio/keyring tests, by hand
```

`./scripts/check.sh` must be clean before any commit. Tests never touch the
network; anything that does is `#[ignore]`d.

Architecture, in one direction only:

```
ytm-cli → ytm-tui → ytm-player → ytm-core
```

`ytm-tui` sees only the `MusicSource` and `Player` traits — never `ytmapi_rs` or
`libmpv2`. That is what makes the whole UI testable with no network and no audio
device. See `CLAUDE.md` for the rest of the rules.

## Not included

Deliberately out of scope: lyrics, podcasts, radio, uploads, liking/rating,
multi-account, and server-side playlist reordering. Several are one API call
away, which is exactly why they are written down rather than half-built.
