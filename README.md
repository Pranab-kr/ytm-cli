# ytm-cli

YouTube Music in your terminal. A Rust TUI over YouTube Music's internal API,
with playback through `mpv`.

## Features

**Browse**

- **Home** — YouTube Music's own recommendation shelves: Quick picks, Covers and
  remixes, Heard in Shorts, Listen again. One carousel mixes tracks, playlists,
  albums and artists, so each row is tagged with what `Enter` will do with it.
- **Playlists** — yours, with track counts. `Enter` or `l` opens one.
- **Fav** — your liked and saved songs.
- **Albums** — the ones you saved, or recommendations when you saved none (most
  accounts).
- **Artists** — the artists you follow, and `S` searches YouTube Music for any
  other. `Enter` on an artist plays their top tracks.
- **Search** — songs, albums, artists and playlists, debounced so typing sends
  one request per pause rather than one per keystroke.

**Play**

- Play/pause, next/previous, relative and absolute seek, volume and mute.
- Shuffle that restores the original order when you turn it off, and repeat
  off/one/all.
- A queue you can reorder, append to, play-next into, remove from, and clear.
- Album art in graphics-capable terminals; media keys and OS now-playing via
  MPRIS.

**Edit**

- Create, rename, and delete playlists; add and remove tracks, single or
  multi-selected.
- Every edit is optimistic — the list changes under your hands and rolls back on
  its own if the server refuses.

**Get around**

- Vim keys or arrows, `1`-`7` to jump straight to a source, `zz` to centre a row,
  half-page and full-page motion, and a scroll wheel.
- `/` filters the rows in front of you; `S` searches the server. Two different
  things, deliberately on two different keys.
- Six themes, light and dark, cycled with `t`.
- Every keybinding is remappable, and `ytm-cli config` writes a file with all of
  them already listed.

## Terms of service

**This uses YouTube Music's internal (`youtubei/v1`) API and `yt-dlp`. That is
against YouTube's Terms of Service, whether or not you pay for Premium.** It is
built as a personal tool for a single account. There is no telemetry and nothing
leaves your machine. Use it knowing that, or don't use it.

### Educational-use declaration

This project is provided for educational and personal-use purposes: learning
about Rust, terminal user interfaces, media playback, and API integration. It is
not built or intended to bypass access controls, abuse YouTube's services,
infringe copyright, collect user data, or enable commercial exploitation. Users
are responsible for complying with applicable laws, YouTube's Terms of Service,
and the rights of content owners. The maintainers do not encourage or accept
responsibility for misuse of this software.

## Requirements

| What | Version verified | Why |
|---|---|---|
| Rust | 1.96+ (tested 1.96.1) | building it |
| libmpv | 2.5.0 | audio playback |
| yt-dlp | 2026.08.19 | resolving stream URLs |

**Arch / CachyOs / Manjaro / EndeavourOS:**

```bash
sudo pacman -S --needed rust mpv yt-dlp git base-devel
```

`mpv` brings `libmpv.so` with it, so there is no separate `-dev` package to
install. If you would rather manage Rust through rustup than pacman, use
[rustup](https://rustup.rs) and drop `rust` from that line.

**Debian / Ubuntu:**

```bash
sudo apt install libmpv-dev yt-dlp git build-essential
# Rust via https://rustup.rs — the packaged rustc is usually too old
```

**macOS:**

```bash
brew install mpv yt-dlp
# Rust via https://rustup.rs
```

<details>
<summary><b>Windows</b> — via WSL2 (click the arrow to expand)</summary>

<br>

**Nobody has run this on Windows yet.** The spec calls Windows best-effort and
untested (NFR-10). What follows is reasoned from the dependency graph, not from a
working install — corrections welcome if you try it.

**Use WSL2.** From the app's point of view a WSL2 install *is* Linux, so nothing
about the build changes:

```bash
sudo apt install libmpv-dev yt-dlp git build-essential
# Rust via https://rustup.rs — the packaged rustc is usually too old
```

Then follow [Install](#install) below, unchanged.

Two things decide whether it is usable:

- **Audio needs WSLg** — Windows 11, or a Windows 10 build new enough to have it.
  WSLg ships PulseAudio and mpv finds it with no configuration. Without WSLg
  there is no audio device inside WSL at all, and bridging one to the Windows
  host by hand is more trouble than it is worth.
- **The cookie file lives on the Windows side.** You export it from a Windows
  browser, so `auth.cookie_file` points across the mount:

  ```toml
  cookie_file = "/mnt/c/Users/<you>/Downloads/music.youtube.com_cookies.txt"
  ```

**Media keys do not work in WSL** — there is no D-Bus session bus for MPRIS to
attach to. That is handled rather than fatal: the app logs it and runs without
them. Album art depends on the terminal — Windows Terminal 1.22+ has sixel, and
anything older falls back to half-blocks on its own.

### Native Windows, without WSL

Possible, unproven, and the linker is the hard part. `libmpv2-sys`'s build script
is one line — `cargo:rustc-link-lib=mpv` — with no pkg-config, no vcpkg, and no
platform handling. An MSVC build therefore needs `mpv.lib` on your `LIB` path and
`libmpv-2.dll` beside the binary at runtime; the
[shinchiro mpv builds](https://github.com/shinchiro/mpv-winbuild-cmake/releases)
ship both. `rusqlite` is vendored (`bundled`), so the MSVC build tools are
required. `yt-dlp.exe` on `PATH` works as-is, and `crossterm`, `ratatui`,
`reqwest` (rustls) and `directories` are all fine — config lands in `%APPDATA%`.

Three things degrade rather than break, each already handled in code:

| What | Why | What happens |
|---|---|---|
| Media keys | `souvlaki`'s Windows backend needs a real window handle; the app passes none | logged at info, app runs without them |
| Album art | no kitty/sixel support in most Windows terminals | falls back to half-blocks |
| Cookie-file hardening | the `0600` chmod on the temp cookie jar is `#[cfg(unix)]` | `%TEMP%` is already per-user and ACL'd, but the explicit guard is absent |

</details>

`yt-dlp` is called as a subprocess and needs to stay current — YouTube breaks
stream extraction regularly. If playback stops working, update it first.

**Not required:** `playerctl` is not a dependency. Media-key and now-playing
support is built in over MPRIS (via `souvlaki`), so your desktop's own media keys
and now-playing widget work with nothing extra installed. `playerctl` is just a
convenient way to *test* it from a shell:

```bash
playerctl -p ytm_cli metadata     # optional, for checking MPRIS works
```

If there is no D-Bus session bus at all, the app runs exactly as before, without
media keys. Nothing else changes.

## Install

```bash
git clone https://github.com/Pranab-kr/ytm-cli.git
cd ytm-cli
cargo build --release
./target/release/ytm-cli
```

The binary is `ytm-cli`. Symlink it to `ytm` if you want the shorter name:

```bash
ln -s "$PWD/target/release/ytm-cli" ~/.local/bin/ytm
```

### Getting running, in order

```bash
ytm-cli config          # 1. write config.toml, opens in $EDITOR
                        # 2. set auth.kind and auth.cookie_file (see below)
ytm-cli playlists       # 3. check auth without starting the TUI
ytm-cli                 # 4. go
```

Step 3 is worth doing: it prints your playlists and exits, so an auth problem
shows up as a plain error message instead of an empty pane.

## Configuration

One command:

```bash
ytm-cli config
```

It writes `config.toml` and opens it in `$EDITOR`. The file lists **every
setting and every keybinding at its default value, commented out** — so changing
anything is uncommenting a line. Nothing has to be written from scratch and no
schema has to be looked up.

- Linux: `~/.config/ytm-cli/config.toml`
- macOS: `~/Library/Application Support/ytm-cli/config.toml`

An existing file is never overwritten; it is opened as it is. `--no-edit` writes
and prints the path without opening an editor. The file is validated when you
close the editor, so a typo is reported rather than silently ignored.

`,` inside the app does the same, and keybindings and the theme reload the moment
you save and exit — no restart.

### What you can change

| Setting | Does |
|---|---|
| `ui.start_pane` | Which source the app opens on. `playlists` by default; `home` costs a moment more, since it fetches several pages of recommendations |
| `ui.theme` | `auto` or a built-in name; `t` cycles at runtime |
| `ui.mouse` | Wheel scrolling and click support |
| `ui.album_art` | Art in graphics-capable terminals |
| `ui.tick_ms` | Redraw interval — lower is smoother and busier |
| `playback.volume` | Starting level. `+`/`-` changes are remembered in `state.toml` in the cache dir |
| `behaviour.seek_step_secs` | How far `f`/`b` jump |
| `behaviour.volume_step` | How much `+`/`-` move |
| `behaviour.confirm_on_quit` | Ask before quitting |
| `[keys]` | Rebind any of 40 actions to a single character |

Rebinding looks like this — uncomment and change:

```toml
[keys]
open_filter = "f"      # filter the list with `f` instead of `/`
add_to_queue = "a"
```

## Authentication

Browser cookies, and they are optional — see "Without signing in" below. Cookies
are the only way to reach your own account; the OAuth device flow was removed
because Google stopped honouring device-flow tokens on the endpoints this app
uses, so it could never reach your library.

### Without signing in

Run `ytm-cli` with no config file, or with no `auth.cookie_file`, and it starts in
**guest mode**. You can search YouTube Music, play what you find, and use the
queue. Home, Playlists, Fav, Albums, Artists, and playlist editing need an
account: they are dimmed in the sidebar, and pressing `1`-`5` says so rather than
opening an empty pane. Add a cookie file later and everything appears — nothing
else to change.

Guest playback depends on YouTube not bot-checking your IP. If it does, the app
says so when you press Enter on a track; export cookies as below and restart.

### Use a private / incognito window

**Do this in a private window, and it matters.** A normal browser session keeps
rotating its cookies, so an export from your everyday window can stop working
within hours. A private window's session is frozen the moment you stop using it,
so the export keeps working for far longer.

1. Open a **private / incognito** window.
2. Go to <https://music.youtube.com> and **sign in**.
3. Open DevTools (F12) → **Network** tab.
4. **Hold Shift and click the reload button.** Without this the request list is
   often served from cache and shows no `Cookie:` header at all — this is the
   step people get stuck on.
5. Select the `music.youtube.com` request in the **Name** column, open
   **Request Headers**, and copy the **entire value** of the `Cookie:` header.
6. Paste it into a file, e.g. `~/.config/ytm-cli/cookies.txt`.
7. **Close the private window — do not sign out.** Signing out invalidates the
   session you just exported. Closing it leaves the cookies valid.

Then in `config.toml`:

```toml
[auth]
kind = "cookie"
cookie_file = "~/.config/ytm-cli/cookies.txt"
```

Check it worked without starting the TUI:

```bash
ytm-cli playlists
```

**Format matters.** The file holds the raw header value — one line, starting
something like `VISITOR_INFO1_LIVE=...; SAPISID=...` — and must contain
`SAPISID=`. It is **not** Netscape `cookies.txt` format; the contents are sent
verbatim as the header. A Netscape export fails with an opaque
`Error parsing header.`

The same file is used for playback: `yt-dlp` needs cookies too, or YouTube
answers stream requests with *"Sign in to confirm you're not a bot"*. The app
converts the header into the format yt-dlp wants automatically — you only export
once.

### When your library suddenly reads empty

The cookie expired. An expired cookie is **not** an auth error: YouTube answers
HTTP 200 with a signed-out page, so your library parses as zero rows. Re-export
before assuming a bug — the app also shows a hint when it sees this.

## Commands

```bash
ytm-cli                  # the TUI
ytm-cli config           # write config.toml with every default, then open it
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
| `c` | focus and centre the currently playing song |
| scroll wheel | scroll the focused list |
| left click | select a row, or switch source in the sidebar |
| right click | add the row under the pointer to the queue |
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
| `S` | search YouTube Music — in the Artists pane, searches artists |
| `Esc` | leave filter input; press again to clear the filter |
| `Enter` | keep the filter and selected song, then move to the rows |
| `Ctrl+w` | delete the previous word |
| `Ctrl+←` / `Ctrl+→` | move a word at a time |
| `Ctrl+a` / `Ctrl+e` | start / end of line |

### Appearance

| Key | Action |
|---|---|
| `t` | cycle theme |
| `,` | edit config in `$EDITOR`, reloading on exit |
| `?` | help overlay |

## Mouse

The wheel scrolls the focused list. Left click selects a row (or a sidebar
source); right click adds the row under the pointer to the queue.

Clicking deliberately never *plays* — a misplaced click starting audio is worse
than one that costs a keypress, and `Enter` is one key away once the row is
selected. Set `ui.mouse = false` to turn capture off entirely if you would rather
keep your terminal's own click-drag text selection inside the app.

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
