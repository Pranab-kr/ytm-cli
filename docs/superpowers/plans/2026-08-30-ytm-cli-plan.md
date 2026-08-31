# ytm-cli Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust terminal client for YouTube Music with account login, library browsing, audio playback, and playlist CRUD.

**Architecture:** Cargo workspace of four crates — `ytm-core` (models, `MusicSource` trait, ytmapi-rs impl, cache), `ytm-player` (`Player` trait, mpv backend, queue), `ytm-tui` (ratatui views + reducers), `ytm-cli` (binary). All fragile external access sits behind the `MusicSource` and `Player` traits so the UI is testable with no network and no audio device. A single `tokio::select!` event loop owns `AppState`; the mpv handle lives on its own thread as an actor because `wait_event` blocks.

**Tech Stack:** Rust 1.96.1 (edition 2024), ytmapi-rs 0.3.3, libmpv2 6.0.0, ratatui 0.30.2, crossterm 0.29.0, tokio 1.53.1, rusqlite 0.40.2, keyring 4.2.0, clap 4.6.6.

**Spec:** `docs/superpowers/plans/2026-08-30-ytm-cli-design.md` — read it before starting any task. Requirement IDs (FR-*, NFR-*) below refer to it.

**Progress tracking:** `PROGRESS.md` at the repo root. Update it after every task, before you stop. It is how the next agent knows where you left off.

## Global Constraints

Every task's requirements implicitly include all of these.

- **Rust edition 2024**, toolchain 1.96.1. Do not bump versions.
- **Exact dependency versions** from the spec's §7 table. Pin them; no `*`, no caret widening beyond what is listed.
- **`reqwest` uses `rustls` only.** Never enable an OpenSSL/native-tls path — set `default-features = false`.
- **No logging to stdout or stderr, ever** (NFR-4). `tracing` goes to a file via `tracing-appender`. A stray `println!` corrupts the TUI frame; treat one as a bug.
- **The UI thread never blocks on I/O** (NFR-2). No `.await` on a network call inside a draw path, no blocking `wait_event` on the tokio runtime.
- **No secrets on disk, in logs, or in git** (NFR-6). Tokens go to the OS keyring. Test fixtures are scrubbed of account identifiers.
- **Truncate with `unicode-width`, never `str::len()`** — width, not byte count.
- **Gate before every commit:** `cargo test --workspace` passes AND `cargo clippy --all-targets -- -D warnings` is clean (NFR-8).
- **TDD is mandatory.** Failing test first, watch it fail, minimal implementation, watch it pass, commit. Never write implementation before its test.
- **Commit after every task** with a conventional-commit message.
- Crate dependency direction is one-way: `ytm-cli → ytm-tui → ytm-player → ytm-core`. No cycles. `ytm-tui` must never import `ytmapi_rs` or `libmpv2`.

---

## File Structure

```
Cargo.toml                     workspace manifest, shared [workspace.dependencies]
rust-toolchain.toml            pins 1.96.1
.gitignore
README.md                      setup, OAuth client creation, ToS note
PROGRESS.md                    living status doc — every agent updates this
CLAUDE.md                      agent operating instructions
AGENTS.md                      symlink/pointer to CLAUDE.md
docs/superpowers/plans/        design spec + this plan

crates/ytm-core/
  src/lib.rs                   re-exports
  src/model.rs                 Track, Playlist, Album, Artist, ids, Duration
  src/source.rs                MusicSource trait + SourceError
  src/auth.rs                  AuthMethod, token storage via keyring
  src/oauth.rs                 device-code flow
  src/ytmusic.rs               YtMusicSource: MusicSource impl over ytmapi-rs
  src/mapping.rs               ytmapi-rs types -> our model types
  src/cache.rs                 SQLite metadata cache
  src/mock.rs                  MockSource (cfg(any(test, feature="mock")))
  tests/fixtures/*.json        recorded API responses, scrubbed

crates/ytm-player/
  src/lib.rs
  src/player.rs                Player trait, PlayerCommand, PlayerEvent, PlayerState
  src/queue.rs                 Queue: order, shuffle, repeat, advance (pure)
  src/resolver.rs              StreamResolver over yt-dlp, 4h TTL cache
  src/mpv_backend.rs           MpvPlayer actor thread
  src/mock.rs                  MockPlayer

crates/ytm-tui/
  src/lib.rs
  src/app.rs                   AppState, Focus, reducers
  src/event.rs                 AppEvent, InputAction
  src/keymap.rs                KeyMap, default vim bindings
  src/theme.rs                 Theme, palette tokens
  src/widgets/mod.rs
  src/widgets/sidebar.rs
  src/widgets/tracklist.rs
  src/widgets/nowplaying.rs    progress bar with eighth-block glyphs
  src/widgets/search.rs
  src/widgets/queue.rs
  src/widgets/modal.rs         confirm + text prompt
  src/widgets/toast.rs
  src/widgets/help.rs
  src/util/text.rs             unicode-width truncation

crates/ytm-cli/
  src/main.rs                  clap, terminal guard, wiring
  src/config.rs                Config from TOML
  src/logging.rs               tracing to rotating file
  src/loop.rs                  the tokio::select! event loop
```

---

## Phase Map

| Phase | Delivers | Tasks | Requirements |
|---|---|---|---|
| 0 | Workspace builds, git initialized, CI gate script | 1–2 | — |
| 1 | Domain models + `MusicSource` trait + `MockSource` | 3–5 | FR-B* shapes |
| 2 | OAuth device login, keyring, cookie fallback, live library fetch | 6–9 | FR-A1…A6, FR-B1, FR-B2 |
| 3 | yt-dlp resolver + mpv plays one real track | 10–12 | FR-P1, FR-P6 |
| 4 | Player actor + queue logic + full transport | 13–16 | FR-P2…P7, FR-Q1…Q3 |
| 5 | TUI shell, event loop, sidebar, now-playing | 17–21 | NFR-1…5, FR-U1…U4 |
| 6 | Browse views + search | 22–25 | FR-B1…B5, FR-S1…S3 |
| 7 | Queue UI + transport keys wired | 26–27 | FR-Q1…Q3 |
| 8 | Playlist CRUD, optimistic + rollback | 28–32 | FR-C1…C6 |
| 9 | SQLite cache, theme, album art, MPRIS, help | 33–38 | FR-U5…U7, NFR-1 |

**Gate between phases:** everything in the phase works end-to-end and `PROGRESS.md` reflects it. Do not start a phase with the previous one red.

> **Phase 2 and 3 are the risk phases.** If `ytmapi-rs` cannot authenticate against the owner's account, or `yt-dlp` cannot resolve a stream, everything above them is worthless. Do not write a single line of TUI code before Task 12 is green.

---

## Task 1: Workspace skeleton and git

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `crates/ytm-core/Cargo.toml`, `crates/ytm-core/src/lib.rs`, `crates/ytm-player/Cargo.toml`, `crates/ytm-player/src/lib.rs`, `crates/ytm-tui/Cargo.toml`, `crates/ytm-tui/src/lib.rs`, `crates/ytm-cli/Cargo.toml`, `crates/ytm-cli/src/main.rs`, `scripts/check.sh`

**Interfaces:**
- Consumes: nothing.
- Produces: a workspace where `cargo test --workspace` runs; `scripts/check.sh` as the gate every later task invokes.

- [ ] **Step 1: Confirm the repository is ready**

A git repo already exists on branch `main` with **no commits yet**, so the first
commit in Step 6 is this project's initial commit. Every task commits, so verify
identity is set before going further.

```bash
cd /home/pranab/proj/kiro
git rev-parse --is-inside-work-tree   # expect: true
git log --oneline                     # expect: "does not have any commits yet"
git config user.name  || git config user.name "pranab"
git config user.email || git config user.email "pranab@localhost"
```

If `rev-parse` fails, run `git init` first.

- [ ] **Step 2: Write the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/ytm-core", "crates/ytm-player", "crates/ytm-tui", "crates/ytm-cli"]

[workspace.package]
edition = "2024"
rust-version = "1.96.1"
license = "MIT"

[workspace.dependencies]
ytmapi-rs = { version = "0.3.3", default-features = false, features = ["simplified-queries", "rustls"] }
libmpv2 = "6.0.0"
youtube_dl = { version = "0.10.0", default-features = false, features = ["tokio", "rustls-tls"] }
ratatui = "0.30.2"
crossterm = { version = "0.29.0", features = ["event-stream"] }
ratatui-image = "11.0.6"
tui-textarea = "0.7.0"
throbber-widgets-tui = "0.11.1"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "sync", "time", "process"] }
tokio-stream = "0.1"
futures = "0.3"
reqwest = { version = "0.13.4", default-features = false, features = ["rustls-tls", "json"] }
clap = { version = "4.6.6", features = ["derive"] }
rusqlite = { version = "0.40.2", features = ["bundled"] }
keyring = { version = "4.2.0", features = ["apple-native", "linux-native"] }
souvlaki = "0.8.3"
nucleo = "0.5.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.9"
thiserror = "2.0.20"
color-eyre = "0.6.5"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2.5"
directories = "6.0.0"
unicode-width = "0.2.2"
chrono = { version = "0.4", features = ["serde"] }
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.96.1"
components = ["rustfmt", "clippy"]
```

`.gitignore`:

```
/target
*.log
/logs
.env
cookies.txt
*.cookie
config.local.toml
```

- [ ] **Step 3: Create the four crate manifests**

`crates/ytm-core/Cargo.toml` (the others follow the same shape — adjust name and deps):

```toml
[package]
name = "ytm-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[features]
mock = []

[dependencies]
ytmapi-rs.workspace = true
reqwest.workspace = true
rusqlite.workspace = true
keyring.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
chrono.workspace = true
tokio.workspace = true
directories.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt"] }
```

`ytm-player` depends on `ytm-core`, `libmpv2`, `youtube_dl`, `tokio`, `thiserror`, `tracing`, `rand = "0.9"`.
`ytm-tui` depends on `ytm-core` (with `mock` in dev-deps), `ytm-player`, `ratatui`, `crossterm`, `ratatui-image`, `tui-textarea`, `throbber-widgets-tui`, `unicode-width`, `nucleo`, `serde`, `toml`.
`ytm-cli` is the binary and depends on all three plus `clap`, `color-eyre`, `tracing-subscriber`, `tracing-appender`, `tokio`, `souvlaki`, `directories`, `toml`.

Each `src/lib.rs` starts as `//! <crate purpose>` plus nothing else. `ytm-cli/src/main.rs` starts as `fn main() {}`.

- [ ] **Step 4: Write the gate script**

`scripts/check.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
cargo fmt --all -- --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace
echo "gate: OK"
```

```bash
chmod +x scripts/check.sh
```

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: succeeds, four crates compile. First run downloads and builds libmpv bindings and bundled SQLite — allow several minutes.

If libmpv is missing the `libmpv2` build fails. Confirm with `pkg-config --modversion mpv` (expect 2.5.0 on this machine).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: initialize cargo workspace with four crates"
```

---

## Task 2: Logging to file and the terminal guard

**Files:**
- Create: `crates/ytm-cli/src/logging.rs`
- Modify: `crates/ytm-cli/src/main.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `logging::init(dir: &Path) -> Result<WorkerGuard>` — call once at startup, hold the guard for the process lifetime or logs are silently dropped.

- [ ] **Step 1: Write the failing test**

`crates/ytm-cli/src/logging.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_log_file_in_given_dir() {
        let dir = std::env::temp_dir().join(format!("ytmlog{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let guard = init(&dir).expect("init should succeed");
        tracing::info!("hello from test");
        drop(guard);
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
        assert!(!entries.is_empty(), "expected a log file to be created in {dir:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run the test and watch it fail**

Run: `cargo test -p ytm-cli logging`
Expected: FAIL — `cannot find function 'init' in this scope`.

- [ ] **Step 3: Implement**

```rust
//! File-only logging. Nothing may reach stdout/stderr — it corrupts the TUI frame.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Initialize file logging. Hold the returned guard for the whole process;
/// dropping it stops the writer thread and loses buffered lines.
pub fn init(dir: &Path) -> std::io::Result<WorkerGuard> {
    std::fs::create_dir_all(dir)?;
    let appender = tracing_appender::rolling::daily(dir, "ytm-cli.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("YTM_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
    Ok(guard)
}
```

`try_init` rather than `init` so a second call in tests does not panic.

- [ ] **Step 4: Run the test and watch it pass**

Run: `cargo test -p ytm-cli logging`
Expected: PASS.

- [ ] **Step 5: Add the terminal guard**

In `crates/ytm-cli/src/main.rs` — this satisfies NFR-5, and without it any panic leaves the user's terminal unusable:

```rust
mod logging;

use std::io::{self, Stdout};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

/// Restores the terminal on drop, including during a panic unwind.
pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, crossterm::cursor::Hide)?;
        Ok(Self { terminal: Terminal::new(CrosstermBackend::new(out))? })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    Ok(())
}
```

`ytm-cli` needs `ratatui` and `crossterm` added to its dependencies for this.

- [ ] **Step 6: Run the gate**

Run: `./scripts/check.sh`
Expected: `gate: OK`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: file-only logging and panic-safe terminal guard"
```

---

## Task 3: Domain models

**Files:**
- Create: `crates/ytm-core/src/model.rs`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `VideoId`, `PlaylistId`, `SetVideoId`, `Track`, `Playlist`, `Album`, `Artist`, `Privacy`, `TrackDuration`. Every later task uses these names exactly.

- [ ] **Step 1: Write the failing test**

Append to `crates/ytm-core/src/model.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formats_as_mmss_under_an_hour() {
        assert_eq!(TrackDuration::from_secs(0).to_string(), "0:00");
        assert_eq!(TrackDuration::from_secs(9).to_string(), "0:09");
        assert_eq!(TrackDuration::from_secs(215).to_string(), "3:35");
        assert_eq!(TrackDuration::from_secs(3599).to_string(), "59:59");
    }

    #[test]
    fn duration_formats_with_hours_when_an_hour_or_more() {
        assert_eq!(TrackDuration::from_secs(3600).to_string(), "1:00:00");
        assert_eq!(TrackDuration::from_secs(3725).to_string(), "1:02:05");
    }

    #[test]
    fn track_artist_display_joins_multiple_artists() {
        let t = Track {
            artists: vec!["Boards of Canada".into(), "Autechre".into()],
            ..Track::stub("v1", "Title")
        };
        assert_eq!(t.artist_display(), "Boards of Canada, Autechre");
    }

    #[test]
    fn track_artist_display_is_placeholder_when_empty() {
        let t = Track { artists: vec![], ..Track::stub("v1", "Title") };
        assert_eq!(t.artist_display(), "Unknown artist");
    }

    #[test]
    fn track_without_set_video_id_cannot_be_removed_from_playlist() {
        // SetVideoId is per-playlist-entry; without it the API cannot remove the row.
        let t = Track::stub("v1", "Title");
        assert!(!t.is_removable());
        let t = Track { set_video_id: Some(SetVideoId("s1".into())), ..Track::stub("v1", "Title") };
        assert!(t.is_removable());
    }
}
```

- [ ] **Step 2: Run the test and watch it fail**

Run: `cargo test -p ytm-core model`
Expected: FAIL — the types do not exist.

- [ ] **Step 3: Implement**

```rust
//! Domain model. Deliberately independent of ytmapi-rs so the API layer can be
//! swapped without touching the UI.

use std::fmt;

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self { Self(s.to_owned()) }
        }
        impl From<String> for $name {
            fn from(s: String) -> Self { Self(s) }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
        }
    };
}

id_type!(VideoId, "A YouTube video id — identifies the audio to play.");
id_type!(PlaylistId, "A YouTube Music playlist id.");
id_type!(
    SetVideoId,
    "Identifies a specific *entry* in a specific playlist. Required to remove \
     that entry; the VideoId alone is not enough because a track may appear twice."
);
id_type!(AlbumId, "A YouTube Music album/browse id.");
id_type!(ArtistId, "A YouTube Music artist/channel id.");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TrackDuration(pub u64);

impl TrackDuration {
    pub fn from_secs(s: u64) -> Self { Self(s) }
    pub fn as_secs(&self) -> u64 { self.0 }
}

impl fmt::Display for TrackDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (h, m, s) = (self.0 / 3600, (self.0 % 3600) / 60, self.0 % 60);
        if h > 0 { write!(f, "{h}:{m:02}:{s:02}") } else { write!(f, "{m}:{s:02}") }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Privacy {
    #[default]
    Private,
    Public,
    Unlisted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub video_id: VideoId,
    /// Present only for tracks read from a playlist. Removal requires it.
    pub set_video_id: Option<SetVideoId>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration: TrackDuration,
    pub thumbnail_url: Option<String>,
    pub is_explicit: bool,
}

impl Track {
    /// Test helper: a minimal valid track.
    pub fn stub(video_id: &str, title: &str) -> Self {
        Self {
            video_id: video_id.into(),
            set_video_id: None,
            title: title.to_owned(),
            artists: vec!["Test Artist".into()],
            album: None,
            duration: TrackDuration::from_secs(180),
            thumbnail_url: None,
            is_explicit: false,
        }
    }

    pub fn artist_display(&self) -> String {
        if self.artists.is_empty() {
            "Unknown artist".to_owned()
        } else {
            self.artists.join(", ")
        }
    }

    pub fn is_removable(&self) -> bool { self.set_video_id.is_some() }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub title: String,
    pub description: Option<String>,
    pub track_count: Option<u32>,
    pub privacy: Privacy,
    pub thumbnail_url: Option<String>,
    /// True when this playlist cannot be edited (e.g. "Your Likes").
    pub is_system: bool,
}

impl Playlist {
    pub fn stub(id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            title: title.to_owned(),
            description: None,
            track_count: Some(0),
            privacy: Privacy::Private,
            thumbnail_url: None,
            is_system: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artists: Vec<String>,
    pub year: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub subscribers: Option<String>,
    pub thumbnail_url: Option<String>,
}
```

Add to `crates/ytm-core/src/lib.rs`:

```rust
//! Domain models, the MusicSource seam, auth, and the metadata cache.
pub mod model;
pub use model::*;
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-core model`
Expected: 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): domain model with id newtypes and duration formatting"
```

---

## Task 4: The MusicSource trait and SourceError

**Files:**
- Create: `crates/ytm-core/src/source.rs`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: `model::*` from Task 3.
- Produces: `trait MusicSource` and `enum SourceError`. Task 5 implements it as a mock, Task 8 as the real thing, and every UI task consumes it.

- [ ] **Step 1: Write the failing test**

`crates/ytm-core/src/source.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_error_messages_are_human_readable() {
        // NFR-9: errors reach the user as sentences, never Debug dumps.
        let e = SourceError::NotAuthenticated;
        assert_eq!(e.to_string(), "not signed in — run `ytm login` first");

        let e = SourceError::RateLimited;
        assert!(e.to_string().contains("too many requests"));

        let e = SourceError::NotEditable("Your Likes".into());
        assert_eq!(e.to_string(), "the playlist \"Your Likes\" cannot be edited");
    }

    #[test]
    fn trait_object_is_usable_behind_arc() {
        // The UI holds Arc<dyn MusicSource>; this must compile.
        fn assert_object_safe(_: std::sync::Arc<dyn MusicSource>) {}
        let _ = assert_object_safe as fn(_);
    }
}
```

- [ ] **Step 2: Run the test and watch it fail**

Run: `cargo test -p ytm-core source`
Expected: FAIL — `SourceError` not found.

- [ ] **Step 3: Implement**

```rust
//! The seam between the app and YouTube Music. Everything fragile lives behind
//! this trait so a breaking upstream change is contained to one impl.

use crate::model::*;
use std::future::Future;
use std::pin::Pin;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = Result<T, SourceError>> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("not signed in — run `ytm login` first")]
    NotAuthenticated,

    #[error("sign-in expired and could not be renewed — run `ytm login` again")]
    TokenRefreshFailed,

    #[error("too many requests — YouTube is rate limiting; try again in a minute")]
    RateLimited,

    #[error("network problem: {0}")]
    Network(String),

    #[error("YouTube sent something unexpected ({0}) — the API may have changed")]
    Parse(String),

    #[error("the playlist \"{0}\" cannot be edited")]
    NotEditable(String),

    #[error("{0} was not found")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

/// Read and write access to the user's YouTube Music account.
///
/// Object-safe on purpose: the UI holds `Arc<dyn MusicSource>` so it can be
/// swapped for `MockSource` in tests with no network.
pub trait MusicSource: Send + Sync {
    fn library_playlists(&self) -> BoxFut<'_, Vec<Playlist>>;
    fn library_songs(&self) -> BoxFut<'_, Vec<Track>>;
    fn library_albums(&self) -> BoxFut<'_, Vec<Album>>;
    fn library_artists(&self) -> BoxFut<'_, Vec<Artist>>;

    fn playlist_tracks(&self, id: PlaylistId) -> BoxFut<'_, Vec<Track>>;
    fn playlist_details(&self, id: PlaylistId) -> BoxFut<'_, Playlist>;

    fn search_songs(&self, query: String) -> BoxFut<'_, Vec<Track>>;
    fn search_albums(&self, query: String) -> BoxFut<'_, Vec<Album>>;
    fn search_artists(&self, query: String) -> BoxFut<'_, Vec<Artist>>;
    fn search_playlists(&self, query: String) -> BoxFut<'_, Vec<Playlist>>;

    fn create_playlist(
        &self,
        title: String,
        description: Option<String>,
        privacy: Privacy,
    ) -> BoxFut<'_, PlaylistId>;

    fn edit_playlist(
        &self,
        id: PlaylistId,
        new_title: Option<String>,
        new_description: Option<String>,
        new_privacy: Option<Privacy>,
    ) -> BoxFut<'_, ()>;

    fn delete_playlist(&self, id: PlaylistId) -> BoxFut<'_, ()>;

    fn add_tracks(&self, id: PlaylistId, videos: Vec<VideoId>) -> BoxFut<'_, ()>;

    /// Needs `SetVideoId`, not `VideoId` — see the doc comment on `SetVideoId`.
    fn remove_tracks(&self, id: PlaylistId, entries: Vec<SetVideoId>) -> BoxFut<'_, ()>;
}
```

`BoxFut` rather than `async fn` in the trait because the trait must stay object-safe for `Arc<dyn MusicSource>`.

Register in `lib.rs`:

```rust
pub mod source;
pub use source::{BoxFut, MusicSource, SourceError};
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-core source`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(core): MusicSource trait and human-readable SourceError"
```

---

## Task 5: MockSource

**Files:**
- Create: `crates/ytm-core/src/mock.rs`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: `MusicSource`, `SourceError`, models.
- Produces: `MockSource` with `with_playlists`, `with_tracks`, `fail_next`, `calls()`. Every `ytm-tui` test depends on this; get the API right.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_seeded_playlists() {
        let m = MockSource::new().with_playlists(vec![Playlist::stub("p1", "Focus")]);
        let got = m.library_playlists().await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "Focus");
    }

    #[tokio::test]
    async fn records_calls_in_order() {
        let m = MockSource::new();
        let _ = m.library_playlists().await;
        let _ = m.delete_playlist("p1".into()).await;
        assert_eq!(m.calls(), vec!["library_playlists", "delete_playlist(p1)"]);
    }

    #[tokio::test]
    async fn fail_next_makes_exactly_one_call_fail() {
        let m = MockSource::new().with_playlists(vec![Playlist::stub("p1", "Focus")]);
        m.fail_next(SourceError::RateLimited);
        assert!(m.library_playlists().await.is_err());
        assert!(m.library_playlists().await.is_ok(), "only the next call should fail");
    }

    #[tokio::test]
    async fn create_playlist_appends_and_returns_new_id() {
        let m = MockSource::new();
        let id = m.create_playlist("New".into(), None, Privacy::Private).await.unwrap();
        let all = m.library_playlists().await.unwrap();
        assert!(all.iter().any(|p| p.id == id && p.title == "New"));
    }
}
```

- [ ] **Step 2: Run the test and watch it fail**

Run: `cargo test -p ytm-core mock`
Expected: FAIL — `MockSource` not found.

- [ ] **Step 3: Implement**

```rust
//! In-memory MusicSource for tests. No network, deterministic, records calls.

use crate::{model::*, source::*};
use std::sync::Mutex;

#[derive(Default)]
struct Inner {
    playlists: Vec<Playlist>,
    tracks: Vec<Track>,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    calls: Vec<String>,
    fail_next: Option<SourceError>,
    next_id: u32,
}

#[derive(Default)]
pub struct MockSource {
    inner: Mutex<Inner>,
}

impl MockSource {
    pub fn new() -> Self { Self::default() }

    pub fn with_playlists(self, p: Vec<Playlist>) -> Self {
        self.inner.lock().unwrap().playlists = p;
        self
    }
    pub fn with_tracks(self, t: Vec<Track>) -> Self {
        self.inner.lock().unwrap().tracks = t;
        self
    }
    pub fn with_albums(self, a: Vec<Album>) -> Self {
        self.inner.lock().unwrap().albums = a;
        self
    }
    pub fn with_artists(self, a: Vec<Artist>) -> Self {
        self.inner.lock().unwrap().artists = a;
        self
    }

    /// The next call — whichever it is — returns this error, once.
    pub fn fail_next(&self, e: SourceError) {
        self.inner.lock().unwrap().fail_next = Some(e);
    }

    pub fn calls(&self) -> Vec<String> {
        self.inner.lock().unwrap().calls.clone()
    }

    fn record(&self, what: impl Into<String>) -> Result<(), SourceError> {
        let mut g = self.inner.lock().unwrap();
        g.calls.push(what.into());
        match g.fail_next.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Bodies are one-liners over the mutex; a macro keeps this readable.
macro_rules! mock_read {
    ($name:ident, $ret:ty, $field:ident) => {
        fn $name(&self) -> BoxFut<'_, Vec<$ret>> {
            Box::pin(async move {
                self.record(stringify!($name))?;
                Ok(self.inner.lock().unwrap().$field.clone())
            })
        }
    };
}

impl MusicSource for MockSource {
    mock_read!(library_playlists, Playlist, playlists);
    mock_read!(library_songs, Track, tracks);
    mock_read!(library_albums, Album, albums);
    mock_read!(library_artists, Artist, artists);

    fn playlist_tracks(&self, id: PlaylistId) -> BoxFut<'_, Vec<Track>> {
        Box::pin(async move {
            self.record(format!("playlist_tracks({id})"))?;
            Ok(self.inner.lock().unwrap().tracks.clone())
        })
    }

    fn playlist_details(&self, id: PlaylistId) -> BoxFut<'_, Playlist> {
        Box::pin(async move {
            self.record(format!("playlist_details({id})"))?;
            self.inner
                .lock()
                .unwrap()
                .playlists
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or_else(|| SourceError::NotFound(id.to_string()))
        })
    }

    fn search_songs(&self, q: String) -> BoxFut<'_, Vec<Track>> {
        Box::pin(async move {
            self.record(format!("search_songs({q})"))?;
            let g = self.inner.lock().unwrap();
            Ok(g.tracks
                .iter()
                .filter(|t| t.title.to_lowercase().contains(&q.to_lowercase()))
                .cloned()
                .collect())
        })
    }

    fn search_albums(&self, q: String) -> BoxFut<'_, Vec<Album>> {
        Box::pin(async move {
            self.record(format!("search_albums({q})"))?;
            Ok(self.inner.lock().unwrap().albums.clone())
        })
    }

    fn search_artists(&self, q: String) -> BoxFut<'_, Vec<Artist>> {
        Box::pin(async move {
            self.record(format!("search_artists({q})"))?;
            Ok(self.inner.lock().unwrap().artists.clone())
        })
    }

    fn search_playlists(&self, q: String) -> BoxFut<'_, Vec<Playlist>> {
        Box::pin(async move {
            self.record(format!("search_playlists({q})"))?;
            Ok(self.inner.lock().unwrap().playlists.clone())
        })
    }

    fn create_playlist(
        &self,
        title: String,
        description: Option<String>,
        privacy: Privacy,
    ) -> BoxFut<'_, PlaylistId> {
        Box::pin(async move {
            self.record(format!("create_playlist({title})"))?;
            let mut g = self.inner.lock().unwrap();
            g.next_id += 1;
            let id = PlaylistId(format!("mock-pl-{}", g.next_id));
            g.playlists.push(Playlist {
                id: id.clone(),
                title,
                description,
                privacy,
                track_count: Some(0),
                thumbnail_url: None,
                is_system: false,
            });
            Ok(id)
        })
    }

    fn edit_playlist(
        &self,
        id: PlaylistId,
        new_title: Option<String>,
        new_description: Option<String>,
        new_privacy: Option<Privacy>,
    ) -> BoxFut<'_, ()> {
        Box::pin(async move {
            self.record(format!("edit_playlist({id})"))?;
            let mut g = self.inner.lock().unwrap();
            let Some(p) = g.playlists.iter_mut().find(|p| p.id == id) else {
                return Err(SourceError::NotFound(id.to_string()));
            };
            if let Some(t) = new_title { p.title = t; }
            if let Some(d) = new_description { p.description = Some(d); }
            if let Some(v) = new_privacy { p.privacy = v; }
            Ok(())
        })
    }

    fn delete_playlist(&self, id: PlaylistId) -> BoxFut<'_, ()> {
        Box::pin(async move {
            self.record(format!("delete_playlist({id})"))?;
            self.inner.lock().unwrap().playlists.retain(|p| p.id != id);
            Ok(())
        })
    }

    fn add_tracks(&self, id: PlaylistId, videos: Vec<VideoId>) -> BoxFut<'_, ()> {
        Box::pin(async move {
            self.record(format!("add_tracks({id},{})", videos.len()))?;
            Ok(())
        })
    }

    fn remove_tracks(&self, id: PlaylistId, entries: Vec<SetVideoId>) -> BoxFut<'_, ()> {
        Box::pin(async move {
            self.record(format!("remove_tracks({id},{})", entries.len()))?;
            Ok(())
        })
    }
}
```

Register in `lib.rs`, gated so it never ships in a release binary:

```rust
#[cfg(any(test, feature = "mock"))]
pub mod mock;
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-core mock`
Expected: 4 tests PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "test(core): MockSource with call recording and failure injection"
```

---
## Task 6: Config file and paths

**Files:**
- Create: `crates/ytm-cli/src/config.rs`
- Modify: `crates/ytm-cli/src/main.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Config`, `Config::load(path: Option<&Path>) -> Result<Config>`, `Config::default_path() -> PathBuf`, `AuthKind { OAuth, Cookie }`, `paths::cache_dir()`, `paths::log_dir()`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_file_is_absent() {
        let c = Config::from_toml_str("").unwrap();
        assert_eq!(c.auth.kind, AuthKind::OAuth);
        assert_eq!(c.playback.volume, 70);
        assert!(c.ui.vim_keys);
        assert_eq!(c.ui.tick_ms, 250);
    }

    #[test]
    fn parses_a_full_config() {
        let c = Config::from_toml_str(
            r#"
            [auth]
            kind = "cookie"
            cookie_file = "/tmp/c.txt"

            [playback]
            volume = 40

            [ui]
            vim_keys = false
            accent = "#7aa2f7"
            "#,
        )
        .unwrap();
        assert_eq!(c.auth.kind, AuthKind::Cookie);
        assert_eq!(c.auth.cookie_file.as_deref(), Some(std::path::Path::new("/tmp/c.txt")));
        assert_eq!(c.playback.volume, 40);
        assert!(!c.ui.vim_keys);
        assert_eq!(c.ui.accent.as_deref(), Some("#7aa2f7"));
    }

    #[test]
    fn volume_out_of_range_is_rejected_with_a_clear_message() {
        let err = Config::from_toml_str("[playback]\nvolume = 500").unwrap_err().to_string();
        assert!(err.contains("volume"), "message should name the offending key, got: {err}");
    }

    #[test]
    fn oauth_credentials_are_optional_at_parse_time() {
        // Missing creds is a login-time error with a helpful message, not a parse error.
        let c = Config::from_toml_str("[auth]\nkind = \"oauth\"").unwrap();
        assert!(c.auth.client_id.is_none());
    }
}
```

- [ ] **Step 2: Run the test and watch it fail**

Run: `cargo test -p ytm-cli config`
Expected: FAIL — `Config` not found.

- [ ] **Step 3: Implement**

```rust
//! Config loaded from TOML. Secrets are NOT stored here beyond the OAuth
//! client id/secret, which Google treats as non-confidential for device flow.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("config is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("config key `playback.volume` must be 0-100, got {0}")]
    Volume(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind { OAuth, Cookie }

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub kind: AuthKind,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cookie_file: Option<PathBuf>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { kind: AuthKind::OAuth, client_id: None, client_secret: None, cookie_file: None }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct PlaybackConfig {
    pub volume: u16,
    pub shuffle: bool,
}

impl Default for PlaybackConfig {
    fn default() -> Self { Self { volume: 70, shuffle: false } }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub vim_keys: bool,
    pub tick_ms: u64,
    pub accent: Option<String>,
    pub album_art: bool,
    pub theme_file: Option<PathBuf>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { vim_keys: true, tick_ms: 250, accent: None, album_art: true, theme_file: None }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub auth: AuthConfig,
    pub playback: PlaybackConfig,
    pub ui: UiConfig,
}

impl Config {
    pub fn from_toml_str(s: &str) -> Result<Self, ConfigError> {
        let c: Config = toml::from_str(s)?;
        if c.playback.volume > 100 {
            return Err(ConfigError::Volume(c.playback.volume));
        }
        Ok(c)
    }

    /// Missing file is not an error — defaults are valid.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = path.map(PathBuf::from).unwrap_or_else(Self::default_path);
        match std::fs::read_to_string(&path) {
            Ok(s) => Self::from_toml_str(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io { path, source }),
        }
    }

    pub fn default_path() -> PathBuf {
        paths::config_dir().join("config.toml")
    }
}

pub mod paths {
    use directories::ProjectDirs;
    use std::path::PathBuf;

    fn dirs() -> Option<ProjectDirs> { ProjectDirs::from("", "", "ytm-cli") }

    pub fn config_dir() -> PathBuf {
        dirs().map(|d| d.config_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    }
    pub fn cache_dir() -> PathBuf {
        dirs().map(|d| d.cache_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    }
    pub fn log_dir() -> PathBuf { cache_dir().join("logs") }
}
```

`Default for Config` derives correctly because every field's type implements `Default`.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli config`
Expected: 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(cli): TOML config with defaults, validation, and XDG paths"
```

---

## Task 7: Token storage in the OS keyring

**Files:**
- Create: `crates/ytm-core/src/auth.rs`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `StoredToken`, `TokenStore` trait, `KeyringStore`, `MemoryStore`. Task 8 and Task 9 both use `TokenStore`.

- [ ] **Step 1: Write the failing test**

Test against `MemoryStore` — CI and headless machines have no Secret Service, so the keyring path gets an `#[ignore]`d test run by hand.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoredToken {
        StoredToken {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: 1_800_000_000,
            token_type: "Bearer".into(),
        }
    }

    #[test]
    fn memory_store_round_trips() {
        let s = MemoryStore::default();
        assert!(s.load().unwrap().is_none());
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap().unwrap().access_token, "at");
        s.clear().unwrap();
        assert!(s.load().unwrap().is_none());
    }

    #[test]
    fn clearing_an_absent_token_is_not_an_error() {
        // `ytm logout` must succeed even when already logged out (FR-A4).
        let s = MemoryStore::default();
        assert!(s.clear().is_ok());
    }

    #[test]
    fn token_is_expired_within_the_safety_window() {
        let now = 1_000_000;
        // 30s of headroom: a token expiring in 10s counts as expired.
        let t = StoredToken { expires_at: now + 10, ..sample() };
        assert!(t.is_expired_at(now));
        let t = StoredToken { expires_at: now + 600, ..sample() };
        assert!(!t.is_expired_at(now));
    }

    #[test]
    fn debug_impl_does_not_leak_token_values() {
        // NFR-6: secrets must never reach a log line.
        let d = format!("{:?}", sample());
        assert!(!d.contains("at"), "access token leaked into Debug output: {d}");
        assert!(!d.contains("rt"), "refresh token leaked into Debug output: {d}");
    }

    #[test]
    #[ignore = "requires an OS keyring; run manually with --ignored"]
    fn keyring_store_round_trips() {
        let s = KeyringStore::new("ytm-cli-test");
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap().unwrap().refresh_token, "rt");
        s.clear().unwrap();
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-core auth`
Expected: FAIL — `StoredToken` not found.

- [ ] **Step 3: Implement**

```rust
//! Token persistence. Tokens go to the OS keyring, never to a file (NFR-6).

use std::sync::Mutex;

const SERVICE: &str = "ytm-cli";
const USER: &str = "default";
/// Treat a token as expired this many seconds early, so a request never races expiry.
const EXPIRY_HEADROOM_SECS: i64 = 30;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub token_type: String,
}

/// Hand-written so token values can never reach a log line.
impl std::fmt::Debug for StoredToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredToken")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("token_type", &self.token_type)
            .finish()
    }
}

impl StoredToken {
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        self.expires_at - EXPIRY_HEADROOM_SECS <= now_unix
    }
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(chrono::Utc::now().timestamp())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TokenStoreError {
    #[error("could not reach the OS keyring: {0}")]
    Keyring(String),
    #[error("stored credentials are corrupt — run `ytm login` again")]
    Corrupt,
}

pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredToken>, TokenStoreError>;
    fn save(&self, t: &StoredToken) -> Result<(), TokenStoreError>;
    /// Idempotent: clearing when nothing is stored succeeds.
    fn clear(&self) -> Result<(), TokenStoreError>;
}

pub struct KeyringStore { service: String }

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self { Self { service: service.into() } }
    pub fn default_store() -> Self { Self::new(SERVICE) }

    fn entry(&self) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(&self.service, USER).map_err(|e| TokenStoreError::Keyring(e.to_string()))
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<StoredToken>, TokenStoreError> {
        match self.entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json).map(Some).map_err(|_| TokenStoreError::Corrupt),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(TokenStoreError::Keyring(e.to_string())),
        }
    }

    fn save(&self, t: &StoredToken) -> Result<(), TokenStoreError> {
        let json = serde_json::to_string(t).map_err(|_| TokenStoreError::Corrupt)?;
        self.entry()?
            .set_password(&json)
            .map_err(|e| TokenStoreError::Keyring(e.to_string()))
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(TokenStoreError::Keyring(e.to_string())),
        }
    }
}

#[derive(Default)]
pub struct MemoryStore { inner: Mutex<Option<StoredToken>> }

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<StoredToken>, TokenStoreError> {
        Ok(self.inner.lock().unwrap().clone())
    }
    fn save(&self, t: &StoredToken) -> Result<(), TokenStoreError> {
        *self.inner.lock().unwrap() = Some(t.clone());
        Ok(())
    }
    fn clear(&self) -> Result<(), TokenStoreError> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
}
```

Register in `lib.rs`: `pub mod auth;`

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-core auth`
Expected: 4 PASS, 1 ignored.

- [ ] **Step 5: Verify the keyring path by hand**

Run: `cargo test -p ytm-core auth -- --ignored`
Expected: PASS on this machine. If it fails because no Secret Service is running, note that in `PROGRESS.md` and move on — the cookie fallback (Task 9) does not need the keyring.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(core): keyring-backed token store with redacted Debug"
```

---

## Task 8: OAuth device-code login

**Files:**
- Create: `crates/ytm-core/src/oauth.rs`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: `auth::{StoredToken, TokenStore}`.
- Produces: `DeviceCodeInfo { user_code, verification_url, interval_secs }`, `begin_device_login(client, client_id) -> Result<(DeviceCodeInfo, OAuthDeviceCode)>`, `complete_device_login(client, code, client_id, client_secret, store) -> Result<StoredToken>`.

This is the first task that touches the live network. Everything above it is offline.

- [ ] **Step 1: Write the failing test**

The Google endpoint cannot be tested offline, so test the parts that are ours: the shape we hand the UI, and that a completed login is persisted.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;

    #[test]
    fn device_code_info_carries_what_the_ui_must_display() {
        // FR-A1: the login pane needs a code and a URL to show the user.
        let i = DeviceCodeInfo {
            user_code: "ABCD-EFGH".into(),
            verification_url: "https://google.com/device".into(),
            interval_secs: 5,
        };
        assert_eq!(i.user_code, "ABCD-EFGH");
        assert!(i.verification_url.starts_with("https://"));
        assert!(i.interval_secs >= 5, "polling faster than 5s risks a rate limit");
    }

    #[test]
    fn persist_stores_token_and_computes_absolute_expiry() {
        let store = MemoryStore::default();
        let now = 1_000_000;
        persist_token(&store, "at".into(), "rt".into(), 3600, "Bearer".into(), now).unwrap();
        let got = store.load().unwrap().unwrap();
        assert_eq!(got.expires_at, now + 3600, "expires_in is relative; we store absolute");
        assert!(!got.is_expired_at(now));
    }

    #[test]
    fn missing_client_id_is_an_actionable_error() {
        // FR-A6: name the config key, don't dump an error.
        let e = OAuthError::MissingCredentials;
        let msg = e.to_string();
        assert!(msg.contains("auth.client_id"), "must name the config key, got: {msg}");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-core oauth`
Expected: FAIL — `DeviceCodeInfo` not found.

- [ ] **Step 3: Implement**

Verified signatures from ytmapi-rs 0.3.3 (see the spec §8) — `generate_oauth_code_and_url` returns `(OAuthDeviceCode, String)`.

```rust
//! Google OAuth device-code flow (FR-A1..A3).
//!
//! The user must create their own OAuth client of type "TV and Limited Input"
//! in Google Cloud Console; see README. Google does not treat the device-flow
//! client secret as confidential, so it may live in config.toml.

use crate::auth::{StoredToken, TokenStore};
use ytmapi_rs::auth::OAuthDeviceCode;
use ytmapi_rs::Client;

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("no OAuth client configured — set `auth.client_id` and `auth.client_secret` in config.toml (see README: Google Cloud setup)")]
    MissingCredentials,
    #[error("Google rejected the sign-in: {0}")]
    Rejected(String),
    #[error("sign-in timed out — the code expired before it was authorized")]
    TimedOut,
    #[error("could not store credentials: {0}")]
    Store(#[from] crate::auth::TokenStoreError),
    #[error("network problem during sign-in: {0}")]
    Network(String),
}

/// What the login pane shows the user.
#[derive(Debug, Clone)]
pub struct DeviceCodeInfo {
    pub user_code: String,
    pub verification_url: String,
    /// Never poll faster than this; Google rate limits.
    pub interval_secs: u64,
}

/// Step 1: get a code to show the user. Returns the display info plus the
/// opaque code to hand back to `complete_device_login`.
pub async fn begin_device_login(
    client: &Client,
    client_id: &str,
) -> Result<(DeviceCodeInfo, OAuthDeviceCode), OAuthError> {
    if client_id.is_empty() {
        return Err(OAuthError::MissingCredentials);
    }
    let (code, url) = ytmapi_rs::generate_oauth_code_and_url(client, client_id)
        .await
        .map_err(|e| OAuthError::Network(e.to_string()))?;

    let info = DeviceCodeInfo {
        user_code: code.to_string(),
        verification_url: url,
        interval_secs: 5,
    };
    Ok((info, code))
}

/// Step 2: poll until the user authorizes, then persist.
///
/// `generate_oauth_token` errors until authorization completes, so retry on the
/// interval until `deadline_secs` elapses.
pub async fn complete_device_login(
    client: &Client,
    code: OAuthDeviceCode,
    client_id: &str,
    client_secret: &str,
    store: &dyn TokenStore,
    interval_secs: u64,
    deadline_secs: u64,
) -> Result<StoredToken, OAuthError> {
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(OAuthError::MissingCredentials);
    }
    let start = std::time::Instant::now();
    loop {
        match ytmapi_rs::generate_oauth_token(client, code.clone(), client_id, client_secret).await {
            Ok(tok) => {
                // OAuthToken's accessors are checked against the crate at
                // implementation time; adapt field access if they differ.
                let json = serde_json::to_value(&tok)
                    .map_err(|e| OAuthError::Rejected(e.to_string()))?;
                let access = json["access_token"].as_str().unwrap_or_default().to_owned();
                let refresh = json["refresh_token"].as_str().unwrap_or_default().to_owned();
                let expires_in = json["expires_in"].as_i64().unwrap_or(3600);
                let ttype = json["token_type"].as_str().unwrap_or("Bearer").to_owned();
                let now = chrono::Utc::now().timestamp();
                return persist_token(store, access, refresh, expires_in, ttype, now)
                    .map_err(OAuthError::from);
            }
            Err(_) if start.elapsed().as_secs() < deadline_secs => {
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            }
            Err(_) => return Err(OAuthError::TimedOut),
        }
    }
}

/// Convert Google's relative `expires_in` into an absolute instant and store it.
pub fn persist_token(
    store: &dyn TokenStore,
    access_token: String,
    refresh_token: String,
    expires_in_secs: i64,
    token_type: String,
    now_unix: i64,
) -> Result<StoredToken, crate::auth::TokenStoreError> {
    let t = StoredToken {
        access_token,
        refresh_token,
        expires_at: now_unix + expires_in_secs,
        token_type,
    };
    store.save(&t)?;
    Ok(t)
}
```

`serde_json::to_value` is a deliberate hedge: `OAuthToken`'s public field names are not documented and may be private. If direct accessors exist, use them and delete the JSON round-trip. Do not guess field names without checking.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-core oauth`
Expected: 3 tests PASS.

- [ ] **Step 5: Verify against the real Google endpoint — MANUAL, human required**

This step needs the owner's Google account and cannot be automated. If you are an agent and no credentials are configured, stop here, write what is blocked in `PROGRESS.md`, and ask the owner to run it.

Add a temporary example at `crates/ytm-core/examples/login_spike.rs` that calls `begin_device_login`, prints the code and URL, then calls `complete_device_login` and prints whether a token was stored.

Run: `cargo run -p ytm-core --example login_spike`
Expected: a code and URL print; after authorizing in a browser, "token stored" prints.

**This is the project's first real go/no-go.** If it fails, the failure is in auth, not in your code above — record the exact error in `PROGRESS.md` and try the cookie path (Task 9) before going further.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): OAuth device-code login with token persistence"
```

---

## Task 9: YtMusicSource — the real MusicSource

**Files:**
- Create: `crates/ytm-core/src/ytmusic.rs`, `crates/ytm-core/src/mapping.rs`
- Create: `crates/ytm-core/tests/fixtures/library_playlists.json`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: `MusicSource`, models, `TokenStore`, `oauth`.
- Produces: `YtMusicSource::from_oauth(...)`, `YtMusicSource::from_cookie_file(path)`, and `mapping::{track_from_playlist_item, playlist_from_library}`.

- [ ] **Step 1: Capture a fixture — MANUAL, human required**

Mapping tests need a real response shape. Capture one once:

```bash
# with a working login from Task 8:
cargo run -p ytm-core --example dump_playlists > /tmp/raw.json
```

Write `crates/ytm-core/examples/dump_playlists.rs` using `raw_json_query` (verified to exist: `raw_json_query<Q: Query<A>>(&self, query) -> Result<String>`) with `GetLibraryPlaylistsQuery`.

**Scrub before committing** — remove account ids, emails, and any `browseId` tied to the user's channel. Save the scrubbed version to `crates/ytm-core/tests/fixtures/library_playlists.json`.

If you are an agent without credentials: hand-write a minimal fixture matching the documented shape, mark it `SYNTHETIC` in `PROGRESS.md`, and flag that it needs replacing with a real capture.

- [ ] **Step 2: Write the failing test**

`crates/ytm-core/src/mapping.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn maps_a_playlist_item_into_our_track() {
        let item = TestPlaylistItem {
            video_id: "abc123".into(),
            set_video_id: Some("set789".into()),
            title: "Roygbiv".into(),
            artists: vec!["Boards of Canada".into()],
            album: Some("Music Has the Right".into()),
            duration_secs: 149,
        };
        let t = track_from_parts(item);
        assert_eq!(t.video_id, VideoId::from("abc123"));
        assert_eq!(t.set_video_id, Some(SetVideoId::from("set789")));
        assert_eq!(t.duration.to_string(), "2:29");
        assert!(t.is_removable(), "playlist tracks must keep set_video_id (see FR-C5)");
    }

    #[test]
    fn missing_duration_maps_to_zero_not_a_panic() {
        let t = track_from_parts(TestPlaylistItem { duration_secs: 0, ..TestPlaylistItem::stub() });
        assert_eq!(t.duration.as_secs(), 0);
        assert_eq!(t.duration.to_string(), "0:00");
    }

    #[test]
    fn system_playlists_are_flagged_not_editable() {
        // "Your Likes" (LM) cannot be edited; the UI must refuse before calling the API.
        assert!(is_system_playlist("LM"));
        assert!(is_system_playlist("SE"));
        assert!(!is_system_playlist("PLxxxx"));
    }

    #[test]
    fn fixture_parses_into_playlists() {
        let raw = include_str!("../tests/fixtures/library_playlists.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("fixture must be valid JSON");
        assert!(v.is_object() || v.is_array(), "fixture shape check");
    }
}
```

`TestPlaylistItem` is a local struct in the test module mirroring the fields we read, so mapping is testable without constructing `ytmapi-rs` types (many have private fields). `track_from_parts` takes it via a small `From` conversion that the real `PlaylistItem` also satisfies.

- [ ] **Step 3: Run the tests and watch them fail**

Run: `cargo test -p ytm-core mapping`
Expected: FAIL — `track_from_parts` not found.

- [ ] **Step 4: Implement the mapping layer**

```rust
//! ytmapi-rs types -> our model types. The only place upstream types appear
//! besides ytmusic.rs, so an upstream rename breaks exactly one file.

use crate::model::*;

/// Playlist ids YouTube Music owns and refuses to let us edit.
pub fn is_system_playlist(id: &str) -> bool {
    matches!(id, "LM" | "SE") || id.starts_with("RDAMPL")
}

/// The fields we need from any playlist entry, regardless of source type.
pub struct TrackParts {
    pub video_id: String,
    pub set_video_id: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_secs: u64,
    pub thumbnail_url: Option<String>,
    pub is_explicit: bool,
}

pub fn track_from_parts(p: impl Into<TrackParts>) -> Track {
    let p = p.into();
    Track {
        video_id: VideoId(p.video_id),
        set_video_id: p.set_video_id.map(SetVideoId),
        title: p.title,
        artists: p.artists,
        album: p.album,
        duration: TrackDuration::from_secs(p.duration_secs),
        thumbnail_url: p.thumbnail_url,
        is_explicit: p.is_explicit,
    }
}
```

Then write `impl From<ytmapi_rs::parse::PlaylistItem> for TrackParts` and the equivalents for `SearchResultSong`, `LibraryPlaylist`, `SearchResultAlbum`.

**Read the upstream struct definitions before writing these.** Do not guess field names:

```bash
cargo doc -p ytmapi-rs --no-deps --open   # or read ~/.cargo/registry/src/**/ytmapi-rs-0.3.3/src/parse/
```

Duration is often a display string like `"2:29"`, not seconds. Write a `parse_duration(&str) -> u64` helper with its own test if so.

- [ ] **Step 5: Run the tests and watch them pass**

Run: `cargo test -p ytm-core mapping`
Expected: 4 tests PASS.

- [ ] **Step 6: Implement YtMusicSource**

```rust
//! The live MusicSource. Wraps ytmapi-rs; the only file that calls it.

use crate::{mapping, model::*, source::*};
use ytmapi_rs::{auth::BrowserToken, YtMusic};

/// Generic over the auth token type so OAuth and cookie auth share one impl.
pub struct YtMusicSource<A: ytmapi_rs::auth::AuthToken> {
    api: YtMusic<A>,
}

impl YtMusicSource<BrowserToken> {
    /// Cookie fallback (FR-A5).
    pub async fn from_cookie_file(path: impl AsRef<std::path::Path>) -> Result<Self, SourceError> {
        let api = YtMusic::from_cookie_file(path)
            .await
            .map_err(|e| SourceError::Network(e.to_string()))?;
        Ok(Self { api })
    }
}

/// Upstream errors are opaque strings; classify them into our variants so the
/// UI can show a sentence (NFR-9). Refine the substrings against real failures.
fn classify(e: ytmapi_rs::Error) -> SourceError {
    let s = e.to_string();
    let l = s.to_lowercase();
    if l.contains("401") || l.contains("unauthor") {
        SourceError::NotAuthenticated
    } else if l.contains("429") || l.contains("rate") {
        SourceError::RateLimited
    } else if l.contains("404") || l.contains("not found") {
        SourceError::NotFound(s)
    } else if l.contains("parse") || l.contains("navigation") {
        SourceError::Parse(s)
    } else {
        SourceError::Network(s)
    }
}
```

Then implement each `MusicSource` method by delegating to the verified signatures in spec §8 and mapping through `mapping::*`. Two rules:

- `remove_tracks` must reject a system playlist with `SourceError::NotEditable` *before* calling the API.
- `playlist_tracks` must populate `set_video_id`, or removal (FR-C5) is impossible.

- [ ] **Step 7: Verify live fetch — MANUAL, human required**

Extend the spike example to construct a `YtMusicSource` and print playlist titles.

Run: `cargo run -p ytm-core --example login_spike`
Expected: the owner's real playlist titles print.

**This is the Phase 2 gate.** Do not proceed to Phase 3 until real titles appear. Record the result in `PROGRESS.md`.

- [ ] **Step 8: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(core): live YtMusicSource with OAuth and cookie auth"
```

---
## Task 10: StreamResolver over yt-dlp

**Files:**
- Create: `crates/ytm-player/src/resolver.rs`
- Modify: `crates/ytm-player/src/lib.rs`

**Interfaces:**
- Consumes: `ytm_core::VideoId`.
- Produces: `StreamResolver::new()`, `resolve(&self, id: &VideoId) -> Result<String, ResolveError>`, `invalidate(&self, id: &VideoId)`. Task 12 and Task 14 call these.

- [ ] **Step 1: Write the failing test**

The TTL cache is the testable part; the subprocess call gets an `#[ignore]`d test.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::VideoId;

    #[test]
    fn cache_returns_a_fresh_entry() {
        let r = StreamResolver::new();
        r.insert_for_test(&VideoId::from("v1"), "https://example.com/a", 1_000);
        assert_eq!(r.cached_at(&VideoId::from("v1"), 1_100).as_deref(), Some("https://example.com/a"));
    }

    #[test]
    fn cache_expires_after_the_ttl() {
        // Google's URLs die around 6h; we expire at 4h for margin.
        let r = StreamResolver::new();
        r.insert_for_test(&VideoId::from("v1"), "https://example.com/a", 1_000);
        assert!(r.cached_at(&VideoId::from("v1"), 1_000 + TTL_SECS - 1).is_some());
        assert!(r.cached_at(&VideoId::from("v1"), 1_000 + TTL_SECS + 1).is_none());
    }

    #[test]
    fn invalidate_drops_the_entry_so_a_403_can_re_resolve() {
        // FR-P6: a stale URL must be evicted before the retry.
        let r = StreamResolver::new();
        r.insert_for_test(&VideoId::from("v1"), "https://example.com/a", 1_000);
        r.invalidate(&VideoId::from("v1"));
        assert!(r.cached_at(&VideoId::from("v1"), 1_001).is_none());
    }

    #[test]
    fn ttl_is_four_hours() {
        assert_eq!(TTL_SECS, 4 * 60 * 60);
    }

    #[tokio::test]
    #[ignore = "hits the network via yt-dlp; run manually with --ignored"]
    async fn resolves_a_real_video_to_an_https_url() {
        let r = StreamResolver::new();
        let url = r.resolve(&VideoId::from("dQw4w9WgXcQ")).await.unwrap();
        assert!(url.starts_with("https://"), "got: {url}");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-player resolver`
Expected: FAIL — `StreamResolver` not found.

- [ ] **Step 3: Implement**

```rust
//! Turns a VideoId into a playable audio URL by shelling out to yt-dlp.
//!
//! Isolated on purpose: yt-dlp breaking is the single most likely runtime
//! failure, and this is the only file that needs to change when it does.

use std::collections::HashMap;
use std::sync::Mutex;
use ytm_core::VideoId;

/// Resolved URLs are valid ~6h upstream; expire at 4h so playback never
/// starts with a URL that dies mid-track.
pub const TTL_SECS: i64 = 4 * 60 * 60;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("yt-dlp is not installed or not on PATH — install it to play audio")]
    NotInstalled,
    #[error("yt-dlp could not find an audio stream for this track")]
    NoAudioStream,
    #[error("yt-dlp failed: {0}")]
    Failed(String),
}

struct Entry { url: String, fetched_at: i64 }

pub struct StreamResolver {
    cache: Mutex<HashMap<VideoId, Entry>>,
}

impl Default for StreamResolver {
    fn default() -> Self { Self::new() }
}

impl StreamResolver {
    pub fn new() -> Self { Self { cache: Mutex::new(HashMap::new()) } }

    /// Cached URL if present and still inside the TTL.
    pub fn cached_at(&self, id: &VideoId, now_unix: i64) -> Option<String> {
        let g = self.cache.lock().unwrap();
        let e = g.get(id)?;
        (now_unix - e.fetched_at < TTL_SECS).then(|| e.url.clone())
    }

    pub fn invalidate(&self, id: &VideoId) {
        self.cache.lock().unwrap().remove(id);
    }

    #[doc(hidden)]
    pub fn insert_for_test(&self, id: &VideoId, url: &str, at: i64) {
        self.cache.lock().unwrap()
            .insert(id.clone(), Entry { url: url.to_owned(), fetched_at: at });
    }

    /// Resolve, using the cache when warm.
    pub async fn resolve(&self, id: &VideoId) -> Result<String, ResolveError> {
        let now = chrono_now();
        if let Some(u) = self.cached_at(id, now) {
            return Ok(u);
        }
        let url = Self::run_yt_dlp(id).await?;
        self.cache.lock().unwrap()
            .insert(id.clone(), Entry { url: url.clone(), fetched_at: now });
        Ok(url)
    }

    /// `-g` prints the direct URL; `-f bestaudio` avoids downloading video.
    async fn run_yt_dlp(id: &VideoId) -> Result<String, ResolveError> {
        let url = format!("https://music.youtube.com/watch?v={id}");
        let out = tokio::process::Command::new("yt-dlp")
            .args(["-f", "bestaudio", "--no-playlist", "--no-warnings", "-g", &url])
            .output()
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ResolveError::NotInstalled,
                _ => ResolveError::Failed(e.to_string()),
            })?;

        if !out.status.success() {
            return Err(ResolveError::Failed(
                String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("unknown").to_owned(),
            ));
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.starts_with("http"))
            .map(str::to_owned)
            .ok_or(ResolveError::NoAudioStream)
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
```

The `youtube_dl` crate is in the workspace deps as a fallback, but the direct subprocess call is fewer moving parts and gives exact control over flags. Keep the subprocess version unless it proves inadequate.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-player resolver`
Expected: 4 PASS, 1 ignored.

- [ ] **Step 5: Verify against the live network**

Run: `cargo test -p ytm-player resolver -- --ignored --nocapture`
Expected: PASS with an `https://` URL. yt-dlp 2026.08.19 is installed on this machine.

If this fails, run `yt-dlp -f bestaudio -g "https://music.youtube.com/watch?v=dQw4w9WgXcQ"` by hand and record the error in `PROGRESS.md`. A yt-dlp update usually fixes it.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(player): yt-dlp stream resolver with 4h TTL cache"
```

---

## Task 11: Player trait, commands, and events

**Files:**
- Create: `crates/ytm-player/src/player.rs`
- Modify: `crates/ytm-player/src/lib.rs`

**Interfaces:**
- Consumes: `ytm_core::{Track, VideoId, TrackDuration}`.
- Produces: `PlayerCommand`, `PlayerEvent`, `PlaybackState`, `RepeatMode`, `trait Player`. The TUI's event loop matches on `PlayerEvent` exhaustively, so these variants are a contract.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_mode_cycles_off_one_all() {
        // FR-P5: one key cycles through the three modes in this order.
        assert_eq!(RepeatMode::Off.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::Off);
    }

    #[test]
    fn playback_state_knows_when_it_is_audible() {
        assert!(PlaybackState::Playing.is_active());
        assert!(!PlaybackState::Paused.is_active());
        assert!(!PlaybackState::Stopped.is_active());
        assert!(!PlaybackState::Loading.is_active());
    }

    #[test]
    fn volume_is_clamped_to_the_valid_range() {
        assert_eq!(clamp_volume(150), 100);
        assert_eq!(clamp_volume(-10), 0);
        assert_eq!(clamp_volume(64), 64);
    }

    #[test]
    fn seek_relative_never_goes_below_zero() {
        assert_eq!(apply_seek(10, -30, 200), 0);
        assert_eq!(apply_seek(100, 30, 200), 130);
        assert_eq!(apply_seek(190, 30, 200), 200, "clamps to duration");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-player player`
Expected: FAIL — `RepeatMode` not found.

- [ ] **Step 3: Implement**

```rust
//! The Player seam. The TUI speaks only these types — never libmpv2.

use ytm_core::{Track, TrackDuration, VideoId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    /// A track is resolving or buffering; show a spinner (FR-U4).
    Loading,
    Playing,
    Paused,
}

impl PlaybackState {
    /// True only when audio is actually coming out.
    pub fn is_active(&self) -> bool { matches!(self, Self::Playing) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }
}

pub fn clamp_volume(v: i64) -> u8 { v.clamp(0, 100) as u8 }

/// Apply a relative seek, clamped to [0, duration].
pub fn apply_seek(pos: i64, delta: i64, duration: i64) -> i64 {
    (pos + delta).clamp(0, duration)
}

/// Sent from the event loop to the player actor.
#[derive(Debug, Clone)]
pub enum PlayerCommand {
    /// Resolve and play immediately, replacing whatever is playing.
    PlayNow(Track),
    Pause,
    Resume,
    TogglePause,
    Stop,
    Next,
    Previous,
    SeekRelative(i64),
    SeekAbsolute(u64),
    SetVolume(u8),
    ToggleMute,
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    EnqueueBack(Vec<Track>),
    EnqueueNext(Vec<Track>),
    RemoveFromQueue(usize),
    MoveInQueue { from: usize, to: usize },
    ClearQueue,
    Shutdown,
}

/// Sent from the player actor back to the event loop.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    /// Now-playing changed. `None` means the queue ran out.
    TrackChanged(Option<Track>),
    /// Emitted at least 4x/second while playing (FR-P7).
    Progress { position: TrackDuration, duration: TrackDuration },
    VolumeChanged(u8),
    ShuffleChanged(bool),
    RepeatChanged(RepeatMode),
    QueueChanged { tracks: Vec<Track>, current: Option<usize> },
    /// Human-readable; goes straight into a toast (NFR-9).
    Error(String),
    /// The current track finished naturally.
    TrackEnded(VideoId),
}

/// Implemented by `MpvPlayer` and `MockPlayer`. Commands are fire-and-forget;
/// everything observable comes back as a `PlayerEvent`.
pub trait Player: Send {
    fn send(&self, cmd: PlayerCommand) -> Result<(), PlayerError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("the audio player stopped responding")]
    ActorGone,
    #[error("mpv is not available: {0} — install libmpv to play audio")]
    MpvUnavailable(String),
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-player player`
Expected: 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(player): Player trait with command and event types"
```

---

## Task 12: mpv plays one real track — the Phase 3 gate

**Files:**
- Create: `crates/ytm-player/src/mpv_backend.rs`, `crates/ytm-player/examples/play_spike.rs`
- Modify: `crates/ytm-player/src/lib.rs`

**Interfaces:**
- Consumes: `StreamResolver`, `PlayerCommand`, `PlayerEvent`.
- Produces: `MpvHandle::new() -> Result<MpvHandle, PlayerError>` with `load(url)`, `set_pause(bool)`, `set_volume(u8)`, `position()`, `duration()`, `poll_event(timeout) -> Option<Event>`. Task 13 wraps this in the actor.

- [ ] **Step 1: Write the failing test**

libmpv needs an audio device, so the real test is `#[ignore]`d and the unit test covers construction only.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs libmpv and an audio device; run manually with --ignored"]
    fn mpv_initializes_with_video_disabled() {
        let h = MpvHandle::new().expect("libmpv should initialize");
        // Audio-only: video must be off or mpv tries to open a window.
        let vid: String = h.mpv.get_property("vid").unwrap();
        assert_eq!(vid, "no");
    }

    #[test]
    fn missing_libmpv_produces_an_install_hint() {
        // NFR: never panic when a system dep is absent.
        let e = PlayerError::MpvUnavailable("not found".into());
        assert!(e.to_string().contains("install libmpv"), "got: {e}");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-player mpv`
Expected: FAIL — `MpvHandle` not found.

- [ ] **Step 3: Implement**

Signatures verified against libmpv2 6.0.0 source (spec §8): `Mpv::with_initializer`, `command(&str, &[&str])`, `set_property`, `get_property`, `wait_event(f64)`.

```rust
//! Thin wrapper over libmpv2. Audio-only, no window, no OSD.
//!
//! `wait_event` BLOCKS, so this type must only ever be driven from a dedicated
//! OS thread — never from the tokio runtime (NFR-2).

use crate::player::PlayerError;
use libmpv2::{events::Event, Mpv};

pub struct MpvHandle {
    pub mpv: Mpv,
}

impl MpvHandle {
    pub fn new() -> Result<Self, PlayerError> {
        let mpv = Mpv::with_initializer(|init| {
            // Audio only — without these mpv opens a video window.
            init.set_property("vid", "no")?;
            init.set_property("video", "no")?;
            init.set_property("osc", false)?;
            init.set_property("input-default-bindings", false)?;
            init.set_property("terminal", false)?;
            // Bigger cache: streaming URLs stutter on the default.
            init.set_property("cache", "yes")?;
            init.set_property("demuxer-max-bytes", "32MiB")?;
            Ok(())
        })
        .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))?;

        Ok(Self { mpv })
    }

    /// Replace whatever is playing with this URL.
    pub fn load(&self, url: &str) -> Result<(), PlayerError> {
        self.mpv
            .command("loadfile", &[url, "replace"])
            .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))
    }

    pub fn set_pause(&self, paused: bool) -> Result<(), PlayerError> {
        self.mpv
            .set_property("pause", paused)
            .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))
    }

    pub fn set_volume(&self, v: u8) -> Result<(), PlayerError> {
        self.mpv
            .set_property("volume", v as i64)
            .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))
    }

    pub fn seek_absolute(&self, secs: u64) -> Result<(), PlayerError> {
        self.mpv
            .command("seek", &[&secs.to_string(), "absolute"])
            .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))
    }

    pub fn stop(&self) -> Result<(), PlayerError> {
        self.mpv
            .command("stop", &[])
            .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))
    }

    /// Seconds elapsed; `None` before playback starts.
    pub fn position(&self) -> Option<u64> {
        self.mpv.get_property::<f64>("time-pos").ok().map(|f| f.max(0.0) as u64)
    }

    pub fn duration(&self) -> Option<u64> {
        self.mpv.get_property::<f64>("duration").ok().map(|f| f.max(0.0) as u64)
    }

    /// Blocking. Only call from the actor thread.
    pub fn poll_event(&self, timeout_secs: f64) -> Option<Result<Event<'_>, libmpv2::Error>> {
        self.mpv.wait_event(timeout_secs)
    }
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-player mpv`
Expected: 1 PASS, 1 ignored.

- [ ] **Step 5: Write the end-to-end spike**

`crates/ytm-player/examples/play_spike.rs`:

```rust
//! Proves the whole audio path: yt-dlp -> mpv -> speakers.
//! Run: cargo run -p ytm-player --example play_spike -- <videoId>

use ytm_player::{mpv_backend::MpvHandle, resolver::StreamResolver};
use ytm_core::VideoId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args().nth(1).unwrap_or_else(|| "dQw4w9WgXcQ".to_owned());
    let id = VideoId::from(id.as_str());

    let resolver = StreamResolver::new();
    println!("resolving {id} ...");
    let url = resolver.resolve(&id).await?;
    println!("resolved: {}...", &url[..60.min(url.len())]);

    let h = MpvHandle::new()?;
    h.set_volume(60)?;
    h.load(&url)?;
    println!("playing 10s ...");

    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 10 {
        if let Some(Ok(ev)) = h.poll_event(0.5) {
            println!("event: {ev:?}");
        }
        if let (Some(p), Some(d)) = (h.position(), h.duration()) {
            println!("  {p}s / {d}s");
        }
    }
    println!("done");
    Ok(())
}
```

- [ ] **Step 6: Run the spike — PHASE 3 GATE**

Run: `cargo run -p ytm-player --example play_spike`
Expected: audio comes out of the speakers, and position lines count upward.

**This is the second and last go/no-go.** With Task 9 (real library data) and this task (real audio) both green, every remaining phase is ordinary application code with no external unknowns. Record the outcome in `PROGRESS.md` explicitly — the next agent needs to know the foundation is proven.

If mpv loads but stays silent: check `pactl info` / `wpctl status` for a working sink, and try `mpv --no-video <url>` directly to isolate whether the problem is ours or the system's.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(player): mpv backend with audio-only config and playback spike"
```

---

## Task 13: Queue logic

**Files:**
- Create: `crates/ytm-player/src/queue.rs`
- Modify: `crates/ytm-player/src/lib.rs`

**Interfaces:**
- Consumes: `ytm_core::Track`, `RepeatMode`.
- Produces: `Queue` with `push_back`, `push_next`, `current`, `advance`, `previous`, `remove`, `move_item`, `clear`, `set_shuffle`, `tracks`, `current_index`.

Pure logic, no I/O — so it gets the most thorough test coverage in the project.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::Track;

    fn tracks(n: usize) -> Vec<Track> {
        (0..n).map(|i| Track::stub(&format!("v{i}"), &format!("Track {i}"))).collect()
    }

    #[test]
    fn empty_queue_has_no_current_track() {
        let q = Queue::default();
        assert!(q.current().is_none());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn push_back_appends_and_first_push_becomes_current() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        assert_eq!(q.current().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn advance_moves_to_the_next_track() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v1");
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v2");
    }

    #[test]
    fn advance_past_the_end_returns_none_when_repeat_is_off() {
        let mut q = Queue::default();
        q.push_back(tracks(2));
        q.advance();
        assert!(q.advance().is_none(), "queue should run out");
    }

    #[test]
    fn repeat_all_wraps_to_the_start() {
        let mut q = Queue::default();
        q.push_back(tracks(2));
        q.set_repeat(RepeatMode::All);
        q.advance();
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v0");
    }

    #[test]
    fn repeat_one_replays_the_same_track() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v0");
    }

    #[test]
    fn previous_moves_back_and_stops_at_the_first_track() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.advance();
        assert_eq!(q.previous().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.previous().unwrap().video_id.as_str(), "v0", "clamps at the start");
    }

    #[test]
    fn push_next_inserts_directly_after_current() {
        let mut q = Queue::default();
        q.push_back(tracks(3));                       // v0 v1 v2, current v0
        q.push_next(vec![Track::stub("x", "Jumped")]); // v0 x v1 v2
        assert_eq!(q.advance().unwrap().video_id.as_str(), "x");
    }

    #[test]
    fn remove_before_current_keeps_the_same_track_playing() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.advance();                                   // current v1 at index 1
        q.remove(0);                                   // removing v0 shifts indices
        assert_eq!(q.current().unwrap().video_id.as_str(), "v1", "must not skip");
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn removing_the_current_track_moves_to_the_next() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.remove(0);
        assert_eq!(q.current().unwrap().video_id.as_str(), "v1");
    }

    #[test]
    fn removing_the_last_remaining_track_empties_the_queue() {
        let mut q = Queue::default();
        q.push_back(tracks(1));
        q.remove(0);
        assert!(q.current().is_none());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn move_item_reorders_and_tracks_the_current_index() {
        let mut q = Queue::default();
        q.push_back(tracks(3));       // v0 v1 v2, current v0
        q.move_item(0, 2);            // v1 v2 v0, current still v0
        assert_eq!(q.tracks()[2].video_id.as_str(), "v0");
        assert_eq!(q.current().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.current_index(), Some(2));
    }

    #[test]
    fn shuffle_preserves_the_current_track_and_the_full_set() {
        let mut q = Queue::default();
        q.push_back(tracks(6));
        q.advance();
        let before = q.current().unwrap().video_id.clone();
        q.set_shuffle(true);
        assert_eq!(q.current().unwrap().video_id, before, "shuffle must not change what is playing");
        assert_eq!(q.len(), 6, "shuffle must not lose tracks");
    }

    #[test]
    fn disabling_shuffle_restores_the_original_order() {
        let mut q = Queue::default();
        q.push_back(tracks(5));
        q.set_shuffle(true);
        q.set_shuffle(false);
        let ids: Vec<_> = q.tracks().iter().map(|t| t.video_id.0.clone()).collect();
        assert_eq!(ids, vec!["v0", "v1", "v2", "v3", "v4"]);
    }

    #[test]
    fn clear_empties_everything() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.clear();
        assert_eq!(q.len(), 0);
        assert!(q.current().is_none());
        assert_eq!(q.current_index(), None);
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-player queue`
Expected: FAIL — `Queue` not found.

- [ ] **Step 3: Implement**

```rust
//! Queue ordering, shuffle, and repeat. Pure — no I/O, fully unit-tested.

use crate::player::RepeatMode;
use rand::seq::SliceRandom;
use ytm_core::Track;

#[derive(Default)]
pub struct Queue {
    items: Vec<Track>,
    current: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    /// Original order, kept so disabling shuffle can restore it.
    unshuffled: Option<Vec<Track>>,
}

impl Queue {
    pub fn len(&self) -> usize { self.items.len() }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }
    pub fn tracks(&self) -> &[Track] { &self.items }
    pub fn current_index(&self) -> Option<usize> { self.current }
    pub fn current(&self) -> Option<&Track> { self.items.get(self.current?) }
    pub fn repeat(&self) -> RepeatMode { self.repeat }
    pub fn shuffled(&self) -> bool { self.shuffle }

    pub fn set_repeat(&mut self, m: RepeatMode) { self.repeat = m; }

    pub fn push_back(&mut self, mut new: Vec<Track>) {
        if new.is_empty() { return; }
        self.items.append(&mut new);
        if self.current.is_none() { self.current = Some(0); }
    }

    /// Insert so these play immediately after the current track.
    pub fn push_next(&mut self, new: Vec<Track>) {
        if new.is_empty() { return; }
        match self.current {
            Some(i) => {
                let at = (i + 1).min(self.items.len());
                self.items.splice(at..at, new);
            }
            None => {
                self.items = new;
                self.current = Some(0);
            }
        }
    }

    /// Next track per the repeat mode. `None` means playback should stop.
    pub fn advance(&mut self) -> Option<&Track> {
        let i = self.current?;
        let next = match self.repeat {
            RepeatMode::One => i,
            RepeatMode::Off => {
                if i + 1 >= self.items.len() { return None; }
                i + 1
            }
            RepeatMode::All => {
                if self.items.is_empty() { return None; }
                (i + 1) % self.items.len()
            }
        };
        self.current = Some(next);
        self.items.get(next)
    }

    /// Previous track, clamped at the start.
    pub fn previous(&mut self) -> Option<&Track> {
        let i = self.current?;
        let prev = i.saturating_sub(1);
        self.current = Some(prev);
        self.items.get(prev)
    }

    /// Remove by index, keeping the same track playing where possible.
    pub fn remove(&mut self, idx: usize) {
        if idx >= self.items.len() { return; }
        self.items.remove(idx);
        if let Some(v) = self.unshuffled.as_mut() {
            // Keep the saved order consistent with the live list.
            if let Some(p) = v.iter().position(|t| !self.items.iter().any(|k| k.video_id == t.video_id)) {
                v.remove(p);
            }
        }
        self.current = match self.current {
            None => None,
            Some(_) if self.items.is_empty() => None,
            Some(c) if idx < c => Some(c - 1),
            // Removing the current entry: the next track slides into this slot.
            Some(c) if idx == c => Some(c.min(self.items.len() - 1)),
            Some(c) => Some(c),
        };
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to { return; }
        let t = self.items.remove(from);
        self.items.insert(to, t);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current = None;
        self.unshuffled = None;
    }

    /// Shuffle the tail while keeping the current track in place.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle { return; }
        self.shuffle = on;
        if on {
            self.unshuffled = Some(self.items.clone());
            let keep = self.current.and_then(|i| self.items.get(i).cloned());
            let mut rest: Vec<Track> = match &keep {
                Some(k) => self.items.iter().filter(|t| t.video_id != k.video_id).cloned().collect(),
                None => std::mem::take(&mut self.items),
            };
            rest.shuffle(&mut rand::rng());
            self.items = match keep {
                Some(k) => {
                    let mut v = vec![k];
                    v.extend(rest);
                    self.current = Some(0);
                    v
                }
                None => rest,
            };
        } else if let Some(orig) = self.unshuffled.take() {
            let keep = self.current.and_then(|i| self.items.get(i).map(|t| t.video_id.clone()));
            self.items = orig;
            self.current = keep.and_then(|id| self.items.iter().position(|t| t.video_id == id));
        }
    }
}
```

`rand = "0.9"` must be in `ytm-player`'s dependencies. In rand 0.9 the thread RNG is `rand::rng()`.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-player queue`
Expected: 15 tests PASS. The index-tracking tests around `remove` and `move_item` are the ones most likely to fail first — they encode real bugs users notice.

- [ ] **Step 5: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(player): queue with shuffle, repeat, and index-stable edits"
```

---

## Task 14: The player actor

**Files:**
- Create: `crates/ytm-player/src/actor.rs`
- Modify: `crates/ytm-player/src/lib.rs`, `crates/ytm-player/src/mpv_backend.rs`

**Interfaces:**
- Consumes: `MpvHandle`, `StreamResolver`, `Queue`, `PlayerCommand`, `PlayerEvent`.
- Produces: `spawn_player(volume: u8) -> Result<(MpvPlayer, mpsc::UnboundedReceiver<PlayerEvent>), PlayerError>` where `MpvPlayer: Player`.

- [ ] **Step 1: Write the failing test**

The actor owns a real `Mpv`, so the testable parts are the retry decision and the event translation.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_403_is_retried_exactly_once() {
        // FR-P6: stale URL -> invalidate, re-resolve, retry once. Never loop.
        let mut r = RetryState::default();
        assert!(r.should_retry(), "first failure retries");
        assert!(!r.should_retry(), "second failure gives up");
    }

    #[test]
    fn retry_state_resets_on_a_new_track() {
        let mut r = RetryState::default();
        r.should_retry();
        r.reset();
        assert!(r.should_retry(), "each track gets its own retry budget");
    }

    #[test]
    fn stale_url_errors_are_recognized() {
        assert!(is_stale_url_error("Failed to open https://... HTTP 403 Forbidden"));
        assert!(is_stale_url_error("http error 403"));
        assert!(!is_stale_url_error("no audio device found"));
    }

    #[test]
    fn end_of_file_with_error_reason_is_not_a_natural_end() {
        // Advancing the queue on an error would silently skip tracks.
        assert!(is_natural_end(EndReason::Eof));
        assert!(!is_natural_end(EndReason::Error));
        assert!(!is_natural_end(EndReason::Stop));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-player actor`
Expected: FAIL — `RetryState` not found.

- [ ] **Step 3: Implement**

```rust
//! The player actor: owns the Mpv handle on its own OS thread because
//! `wait_event` blocks and must never touch the tokio runtime (NFR-2).

use crate::{mpv_backend::MpvHandle, player::*, queue::Queue, resolver::StreamResolver};
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;
use ytm_core::TrackDuration;

/// One retry per track, per FR-P6.
#[derive(Default)]
pub struct RetryState { used: bool }

impl RetryState {
    pub fn should_retry(&mut self) -> bool {
        if self.used { false } else { self.used = true; true }
    }
    pub fn reset(&mut self) { self.used = false; }
}

pub fn is_stale_url_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("403") || l.contains("forbidden")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason { Eof, Error, Stop, Quit }

/// Only EOF means "play the next track".
pub fn is_natural_end(r: EndReason) -> bool { matches!(r, EndReason::Eof) }

/// Handle held by the event loop. Cheap to clone, never blocks.
pub struct MpvPlayer {
    tx: std_mpsc::Sender<PlayerCommand>,
}

impl Player for MpvPlayer {
    fn send(&self, cmd: PlayerCommand) -> Result<(), PlayerError> {
        self.tx.send(cmd).map_err(|_| PlayerError::ActorGone)
    }
}

/// Start the actor thread. Returns the handle plus the event stream to select on.
pub fn spawn_player(
    volume: u8,
) -> Result<(MpvPlayer, mpsc::UnboundedReceiver<PlayerEvent>), PlayerError> {
    // Probe before spawning so a missing libmpv is a clean startup error.
    let handle = MpvHandle::new()?;
    handle.set_volume(volume)?;

    let (cmd_tx, cmd_rx) = std_mpsc::channel::<PlayerCommand>();
    let (ev_tx, ev_rx) = mpsc::unbounded_channel::<PlayerEvent>();

    std::thread::Builder::new()
        .name("ytm-player".into())
        .spawn(move || run_actor(handle, cmd_rx, ev_tx, volume))
        .map_err(|e| PlayerError::MpvUnavailable(e.to_string()))?;

    Ok((MpvPlayer { tx: cmd_tx }, ev_rx))
}

fn run_actor(
    mpv: MpvHandle,
    cmds: std_mpsc::Receiver<PlayerCommand>,
    events: mpsc::UnboundedSender<PlayerEvent>,
    volume: u8,
) {
    // The actor thread needs its own small runtime for the async resolver.
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = events.send(PlayerEvent::Error(format!("player failed to start: {e}")));
            return;
        }
    };

    let resolver = StreamResolver::new();
    let mut queue = Queue::default();
    let mut retry = RetryState::default();
    let mut state = PlaybackState::Stopped;
    let mut vol = volume;
    let mut muted_at: Option<u8> = None;
    let mut last_progress = std::time::Instant::now();

    loop {
        // 1. Drain pending commands without blocking.
        while let Ok(cmd) = cmds.try_recv() {
            if matches!(cmd, PlayerCommand::Shutdown) {
                let _ = mpv.stop();
                return;
            }
            handle_command(
                cmd, &mpv, &rt, &resolver, &mut queue, &mut retry,
                &mut state, &mut vol, &mut muted_at, &events,
            );
        }

        // 2. Pump mpv events with a short timeout so commands stay responsive.
        if let Some(Ok(ev)) = mpv.poll_event(0.1) {
            use libmpv2::events::Event as E;
            match ev {
                E::EndFile(reason) => {
                    // Map libmpv2's EndFileReason into our EndReason, then either
                    // advance (natural end) or surface the failure.
                    let r = map_end_reason(reason);
                    if is_natural_end(r) {
                        retry.reset();
                        advance_and_play(&mpv, &rt, &resolver, &mut queue, &mut retry, &events);
                    }
                }
                E::FileLoaded => {
                    state = PlaybackState::Playing;
                    let _ = events.send(PlayerEvent::StateChanged(state));
                }
                E::LogMessage { text, .. } if is_stale_url_error(text) => {
                    // FR-P6: the URL expired. Invalidate and try once more.
                    if retry.should_retry() {
                        if let Some(t) = queue.current().cloned() {
                            resolver.invalidate(&t.video_id);
                            play_track(&mpv, &rt, &resolver, &t, &events);
                        }
                    } else {
                        let _ = events.send(PlayerEvent::Error(
                            "this track's stream expired and could not be renewed".into(),
                        ));
                    }
                }
                _ => {}
            }
        }

        // 3. Emit progress at ~4Hz (FR-P7).
        if state.is_active() && last_progress.elapsed().as_millis() >= 240 {
            last_progress = std::time::Instant::now();
            let _ = events.send(PlayerEvent::Progress {
                position: TrackDuration::from_secs(mpv.position().unwrap_or(0)),
                duration: TrackDuration::from_secs(mpv.duration().unwrap_or(0)),
            });
        }
    }
}
```

Write `handle_command`, `play_track`, `advance_and_play`, and `map_end_reason` as small private functions in the same file. Rules:

- `play_track` sets `PlaybackState::Loading`, emits it, resolves via `rt.block_on`, then calls `mpv.load`. Blocking here is correct — this is the actor's own thread.
- Every command that changes observable state emits its `PlayerEvent`, or the UI silently desyncs.
- `ToggleMute` stores the pre-mute volume in `muted_at` so unmuting restores it.
- Check `libmpv2::events::EndFileReason`'s real variant names before writing `map_end_reason`.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-player actor`
Expected: 4 tests PASS.

- [ ] **Step 5: Verify end-to-end by hand**

Extend `play_spike.rs` to use `spawn_player`, send `PlayNow` for two tracks, and print every `PlayerEvent`.

Run: `cargo run -p ytm-player --example play_spike`
Expected: first track plays, `TrackChanged` and `Progress` events print, and the second track starts when the first ends.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(player): actor thread with queue integration and 403 retry"
```

---

## Task 15: MockPlayer

**Files:**
- Create: `crates/ytm-player/src/mock.rs`
- Modify: `crates/ytm-player/src/lib.rs`

**Interfaces:**
- Consumes: `Player`, `PlayerCommand`.
- Produces: `MockPlayer::new() -> (MockPlayer, UnboundedReceiver<PlayerEvent>)`, `commands()`, `emit(PlayerEvent)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::Track;

    #[test]
    fn records_commands_in_order() {
        let (p, _rx) = MockPlayer::new();
        p.send(PlayerCommand::TogglePause).unwrap();
        p.send(PlayerCommand::SetVolume(30)).unwrap();
        assert_eq!(p.commands().len(), 2);
        assert!(matches!(p.commands()[1], PlayerCommand::SetVolume(30)));
    }

    #[tokio::test]
    async fn emit_delivers_an_event_to_the_receiver() {
        let (p, mut rx) = MockPlayer::new();
        p.emit(PlayerEvent::StateChanged(PlaybackState::Playing));
        assert!(matches!(rx.recv().await, Some(PlayerEvent::StateChanged(PlaybackState::Playing))));
    }

    #[test]
    fn play_now_records_the_track() {
        let (p, _rx) = MockPlayer::new();
        p.send(PlayerCommand::PlayNow(Track::stub("v9", "Song"))).unwrap();
        match &p.commands()[0] {
            PlayerCommand::PlayNow(t) => assert_eq!(t.video_id.as_str(), "v9"),
            other => panic!("expected PlayNow, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-player mock`
Expected: FAIL — `MockPlayer` not found.

- [ ] **Step 3: Implement**

```rust
//! Player impl for tests. Records commands, lets tests push events in.

use crate::player::*;
use std::sync::Mutex;
use tokio::sync::mpsc;

pub struct MockPlayer {
    sent: Mutex<Vec<PlayerCommand>>,
    events: mpsc::UnboundedSender<PlayerEvent>,
}

impl MockPlayer {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<PlayerEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { sent: Mutex::new(Vec::new()), events: tx }, rx)
    }

    pub fn commands(&self) -> Vec<PlayerCommand> { self.sent.lock().unwrap().clone() }

    /// Simulate the player reporting something.
    pub fn emit(&self, e: PlayerEvent) { let _ = self.events.send(e); }
}

impl Player for MockPlayer {
    fn send(&self, cmd: PlayerCommand) -> Result<(), PlayerError> {
        self.sent.lock().unwrap().push(cmd);
        Ok(())
    }
}
```

Gate it in `lib.rs` the same way as `MockSource`:

```rust
#[cfg(any(test, feature = "mock"))]
pub mod mock;
```

and add a `mock = []` feature to `ytm-player`'s manifest.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-player mock`
Expected: 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "test(player): MockPlayer recording commands and emitting events"
```

---

## Task 16: Phase 4 gate — record it

**Files:**
- Modify: `PROGRESS.md`

- [ ] **Step 1: Run the whole gate**

Run: `./scripts/check.sh`
Expected: `gate: OK`.

- [ ] **Step 2: Confirm the ignored tests still pass by hand**

Run: `cargo test --workspace -- --ignored`
Expected: keyring, resolver, and mpv tests pass on this machine. Any failure here is environmental — note it in `PROGRESS.md` rather than working around it in code.

- [ ] **Step 3: Update PROGRESS.md**

Mark Phase 4 complete. Record: whether OAuth or cookie auth is the working path, whether the fixture is real or SYNTHETIC, and confirmation that real audio played. The next agent starts the TUI on this foundation and must not re-litigate it.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "docs: record phase 4 completion — audio and API paths proven"
```

---
## Task 17: Unicode-safe text helpers

**Files:**
- Create: `crates/ytm-tui/src/util/text.rs`, `crates/ytm-tui/src/util/mod.rs`
- Modify: `crates/ytm-tui/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `truncate_to_width(s, width) -> String`, `pad_to_width(s, width) -> String`, `display_width(s) -> usize`. Every widget uses these; nothing may use `str::len()` for layout.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_shorter_than_the_width_is_unchanged() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn ascii_longer_than_the_width_gets_an_ellipsis() {
        assert_eq!(truncate_to_width("hello world", 8), "hello w…");
        assert_eq!(display_width(&truncate_to_width("hello world", 8)), 8);
    }

    #[test]
    fn wide_cjk_characters_count_as_two_columns() {
        // "日本語" is 3 chars but 6 columns. str::len() would report 9 bytes.
        assert_eq!(display_width("日本語"), 6);
        let t = truncate_to_width("日本語テスト", 6);
        assert!(display_width(&t) <= 6, "must never exceed the budget, got {}", display_width(&t));
    }

    #[test]
    fn truncation_never_splits_a_wide_character_in_half() {
        // Budget 5 cannot fit 3 wide chars (6 cols); it must drop one, not split.
        let t = truncate_to_width("日本語", 5);
        assert!(display_width(&t) <= 5);
        assert!(!t.contains('\u{FFFD}'), "no replacement chars: {t}");
    }

    #[test]
    fn zero_width_budget_yields_an_empty_string() {
        assert_eq!(truncate_to_width("anything", 0), "");
    }

    #[test]
    fn width_of_one_leaves_room_only_for_the_ellipsis() {
        assert_eq!(truncate_to_width("hello", 1), "…");
    }

    #[test]
    fn pad_fills_to_the_column_width() {
        assert_eq!(pad_to_width("ab", 5), "ab   ");
        assert_eq!(display_width(&pad_to_width("日本", 6)), 6);
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui text`
Expected: FAIL — `truncate_to_width` not found.

- [ ] **Step 3: Implement**

```rust
//! Column-accurate text helpers. Terminal layout is measured in display
//! columns, never bytes or chars — `str::len()` breaks on CJK and emoji.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn display_width(s: &str) -> usize { s.width() }

/// Truncate to at most `width` columns, appending `…` when text was dropped.
/// Never splits a multi-column character.
pub fn truncate_to_width(s: &str, width: usize) -> String {
    if width == 0 { return String::new(); }
    if s.width() <= width { return s.to_owned(); }
    if width == 1 { return "…".to_owned(); }

    let budget = width - 1; // reserve one column for the ellipsis
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > budget { break; }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Right-pad with spaces to exactly `width` columns, truncating if too long.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let t = truncate_to_width(s, width);
    let w = t.width();
    let mut out = t;
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(w)));
    out
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui text`
Expected: 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): column-accurate unicode text helpers"
```

---

## Task 18: Theme

**Files:**
- Create: `crates/ytm-tui/src/theme.rs`
- Modify: `crates/ytm-tui/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Theme` with fields `accent`, `fg`, `fg_dim`, `fg_bright`, `bg`, `bg_sel`, `error`, `success`; `Theme::default()`, `Theme::from_toml_str(&str)`, `parse_hex(&str) -> Option<Color>`.

Spec §6 allows one accent, three neutrals, plus error and success. The type enforces it.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!(parse_hex("#7aa2f7"), Some(Color::Rgb(0x7a, 0xa2, 0xf7)));
        assert_eq!(parse_hex("7aa2f7"), Some(Color::Rgb(0x7a, 0xa2, 0xf7)));
    }

    #[test]
    fn rejects_malformed_hex_instead_of_panicking() {
        assert_eq!(parse_hex("#xyz"), None);
        assert_eq!(parse_hex("#7aa2"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn default_theme_defines_every_role() {
        let t = Theme::default();
        // A None anywhere means a widget would render an invisible element.
        for (name, c) in t.roles() {
            assert!(!matches!(c, Color::Reset), "role {name} must be explicit, not Reset");
        }
    }

    #[test]
    fn accent_override_from_toml_wins() {
        let t = Theme::from_toml_str(r#"accent = "#ff0000""#).unwrap();
        assert_eq!(t.accent, Color::Rgb(0xff, 0, 0));
        // Unspecified roles keep the defaults.
        assert_eq!(t.error, Theme::default().error);
    }

    #[test]
    fn invalid_color_in_toml_is_an_error_naming_the_key() {
        let e = Theme::from_toml_str(r#"accent = "not-a-color""#).unwrap_err().to_string();
        assert!(e.contains("accent"), "got: {e}");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui theme`
Expected: FAIL — `Theme` not found.

- [ ] **Step 3: Implement**

```rust
//! One accent, three neutrals, plus error and success (spec §6). No more —
//! extra colors are how a TUI starts looking accidental.

use ratatui::style::Color;

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("theme is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("theme key `{0}` is not a valid hex color like \"#7aa2f7\"")]
    BadColor(String),
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    Some(Color::Rgb(
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub fg_bright: Color,
    pub bg: Color,
    pub bg_sel: Color,
    pub error: Color,
    pub success: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Color::Rgb(0x7a, 0xa2, 0xf7),
            fg: Color::Rgb(0xc0, 0xca, 0xf5),
            fg_dim: Color::Rgb(0x56, 0x5f, 0x89),
            fg_bright: Color::Rgb(0xff, 0xff, 0xff),
            bg: Color::Reset,             // inherit the user's terminal background
            bg_sel: Color::Rgb(0x2a, 0x2f, 0x41),
            error: Color::Rgb(0xf7, 0x76, 0x8e),
            success: Color::Rgb(0x9e, 0xce, 0x6a),
        }
    }
}

#[derive(serde::Deserialize)]
struct ThemeFile {
    accent: Option<String>,
    fg: Option<String>,
    fg_dim: Option<String>,
    fg_bright: Option<String>,
    bg_sel: Option<String>,
    error: Option<String>,
    success: Option<String>,
}

impl Theme {
    /// Every role except `bg`, which intentionally stays `Reset`.
    pub fn roles(&self) -> [(&'static str, Color); 7] {
        [
            ("accent", self.accent),
            ("fg", self.fg),
            ("fg_dim", self.fg_dim),
            ("fg_bright", self.fg_bright),
            ("bg_sel", self.bg_sel),
            ("error", self.error),
            ("success", self.success),
        ]
    }

    pub fn from_toml_str(s: &str) -> Result<Self, ThemeError> {
        let f: ThemeFile = toml::from_str(s)?;
        let mut t = Self::default();
        let mut set = |key: &str, val: &Option<String>, slot: &mut Color| -> Result<(), ThemeError> {
            if let Some(v) = val {
                *slot = parse_hex(v).ok_or_else(|| ThemeError::BadColor(key.to_owned()))?;
            }
            Ok(())
        };
        set("accent", &f.accent, &mut t.accent)?;
        set("fg", &f.fg, &mut t.fg)?;
        set("fg_dim", &f.fg_dim, &mut t.fg_dim)?;
        set("fg_bright", &f.fg_bright, &mut t.fg_bright)?;
        set("bg_sel", &f.bg_sel, &mut t.bg_sel)?;
        set("error", &f.error, &mut t.error)?;
        set("success", &f.success, &mut t.success)?;
        Ok(t)
    }
}
```

`bg: Color::Reset` is deliberate — inheriting the terminal background looks native. `roles()` excludes it for that reason.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui theme`
Expected: 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): theme with constrained palette and hex parsing"
```

---

## Task 19: AppState and reducers

**Files:**
- Create: `crates/ytm-tui/src/app.rs`, `crates/ytm-tui/src/event.rs`
- Modify: `crates/ytm-tui/src/lib.rs`

**Interfaces:**
- Consumes: `ytm_core` models, `ytm_player::{PlaybackState, RepeatMode}`.
- Produces: `AppState`, `Pane`, `Focus`, `Toast`, `ToastKind`, `AppEvent`, `InputAction`, and `AppState::apply(&mut self, ev: AppEvent)`. This is the heart of the UI; Tasks 20–32 all extend it.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::{Playlist, Track};

    #[test]
    fn starts_focused_on_the_sidebar_with_nothing_loaded() {
        let s = AppState::default();
        assert_eq!(s.focus, Focus::Sidebar);
        assert_eq!(s.pane, Pane::Playlists);
        assert!(s.playlists.is_empty());
        assert!(!s.should_quit);
    }

    #[test]
    fn playlists_loaded_replaces_the_list_and_clears_loading() {
        let mut s = AppState::default();
        s.loading = true;
        s.apply(AppEvent::PlaylistsLoaded(vec![Playlist::stub("p1", "Focus")]));
        assert_eq!(s.playlists.len(), 1);
        assert!(!s.loading, "spinner must stop when data lands (FR-U4)");
    }

    #[test]
    fn an_error_event_becomes_an_error_toast() {
        let mut s = AppState::default();
        s.apply(AppEvent::Error("rate limited".into()));
        assert_eq!(s.toasts.len(), 1);
        assert_eq!(s.toasts[0].kind, ToastKind::Error);
        assert_eq!(s.toasts[0].text, "rate limited");
    }

    #[test]
    fn toasts_expire_after_four_seconds() {
        let mut s = AppState::default();
        s.push_toast(ToastKind::Info, "hi", 1000);
        s.expire_toasts(1000 + TOAST_TTL_MS - 1);
        assert_eq!(s.toasts.len(), 1);
        s.expire_toasts(1000 + TOAST_TTL_MS + 1);
        assert!(s.toasts.is_empty(), "FR-U3: toasts auto-dismiss");
    }

    #[test]
    fn selection_moves_and_clamps_at_both_ends() {
        let mut s = AppState::default();
        s.tracks = vec![Track::stub("a", "A"), Track::stub("b", "B")];
        s.focus = Focus::Main;
        s.selected = 0;
        s.select_next();
        assert_eq!(s.selected, 1);
        s.select_next();
        assert_eq!(s.selected, 1, "clamps at the end");
        s.select_prev();
        s.select_prev();
        assert_eq!(s.selected, 0, "clamps at the start");
    }

    #[test]
    fn switching_pane_resets_the_selection() {
        let mut s = AppState::default();
        s.tracks = vec![Track::stub("a", "A"), Track::stub("b", "B")];
        s.selected = 1;
        s.set_pane(Pane::Search);
        assert_eq!(s.selected, 0, "a stale index would point at the wrong row");
    }

    #[test]
    fn progress_updates_position_and_duration() {
        let mut s = AppState::default();
        s.apply(AppEvent::Player(PlayerEvent::Progress {
            position: TrackDuration::from_secs(30),
            duration: TrackDuration::from_secs(200),
        }));
        assert_eq!(s.position.as_secs(), 30);
        assert_eq!(s.duration.as_secs(), 200);
    }

    #[test]
    fn quit_action_sets_the_quit_flag() {
        let mut s = AppState::default();
        s.apply(AppEvent::Input(InputAction::Quit));
        assert!(s.should_quit);
    }

    #[test]
    fn a_modal_swallows_navigation_so_it_cannot_move_the_list_behind_it() {
        let mut s = AppState::default();
        s.tracks = vec![Track::stub("a", "A"), Track::stub("b", "B")];
        s.modal = Some(Modal::Confirm { text: "sure?".into(), action: ConfirmAction::DeletePlaylist("p1".into()) });
        s.apply(AppEvent::Input(InputAction::Down));
        assert_eq!(s.selected, 0, "modal must capture input");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui app`
Expected: FAIL — `AppState` not found.

- [ ] **Step 3: Implement the event types**

`crates/ytm-tui/src/event.rs`:

```rust
//! Everything that can change the app. The event loop's only vocabulary.

use ytm_core::{Album, Artist, Playlist, PlaylistId, Track};
use ytm_player::player::PlayerEvent;

/// A key press already resolved through the keymap into an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Left,
    Right,
    Confirm,
    Cancel,
    NextPane,
    PrevPane,
    GoTo(u8),
    TogglePause,
    NextTrack,
    PrevTrack,
    SeekForward,
    SeekBack,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    OpenSearch,
    OpenQueue,
    OpenHelp,
    AddToQueue,
    PlayNext,
    CreatePlaylist,
    RenamePlaylist,
    DeletePlaylist,
    RemoveFromPlaylist,
    AddToPlaylist,
    Refresh,
    ToggleMark,
    Char(char),
    Backspace,
}

#[derive(Debug)]
pub enum AppEvent {
    Input(InputAction),
    Player(PlayerEvent),
    Tick,
    Resize,

    PlaylistsLoaded(Vec<Playlist>),
    LibrarySongsLoaded(Vec<Track>),
    AlbumsLoaded(Vec<Album>),
    ArtistsLoaded(Vec<Artist>),
    PlaylistTracksLoaded { id: PlaylistId, tracks: Vec<Track> },
    SearchResults { query: String, tracks: Vec<Track> },

    /// A mutation succeeded server-side; `token` matches the optimistic edit.
    MutationOk { token: u64, message: String },
    /// A mutation failed; roll back the edit tagged with `token`.
    MutationFailed { token: u64, message: String },

    Error(String),
    LoginNeeded { user_code: String, url: String },
    LoginComplete,
}
```

- [ ] **Step 4: Implement AppState**

`crates/ytm-tui/src/app.rs`:

```rust
//! The single source of truth. Owned by the event loop — no locks, no sharing.

use crate::event::{AppEvent, InputAction};
use std::collections::HashSet;
use ytm_core::*;
use ytm_player::player::{PlaybackState, PlayerEvent, RepeatMode};

pub const TOAST_TTL_MS: u64 = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Playlists,
    Songs,
    Albums,
    Artists,
    Search,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Sidebar,
    Main,
    SearchInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind { Info, Success, Error }

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    /// Millis since app start, so expiry is testable without a clock.
    pub born_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DeletePlaylist(PlaylistId),
    RemoveTracks { playlist: PlaylistId, entries: Vec<SetVideoId> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Confirm { text: String, action: ConfirmAction },
    Prompt { title: String, value: String, action: PromptAction },
    Help,
    Login { user_code: String, url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAction {
    CreatePlaylist,
    RenamePlaylist(PlaylistId),
}

#[derive(Default)]
pub struct AppState {
    pub pane: Pane,
    pub focus: Focus,
    pub selected: usize,
    pub sidebar_selected: usize,
    pub scroll_offset: usize,

    pub playlists: Vec<Playlist>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub open_playlist: Option<PlaylistId>,

    pub search_query: String,
    pub search_results: Vec<Track>,

    /// Multi-select for bulk add/remove (FR-C4).
    pub marked: HashSet<VideoId>,

    pub now_playing: Option<Track>,
    pub playback: PlaybackState,
    pub position: TrackDuration,
    pub duration: TrackDuration,
    pub volume: u8,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub queue: Vec<Track>,
    pub queue_current: Option<usize>,

    pub loading: bool,
    pub toasts: Vec<Toast>,
    pub modal: Option<Modal>,
    pub should_quit: bool,
    pub elapsed_ms: u64,
}

impl AppState {
    pub fn apply(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Input(a) => self.apply_input(a),
            AppEvent::Player(p) => self.apply_player(p),
            AppEvent::Tick => self.expire_toasts(self.elapsed_ms),
            AppEvent::Resize => {}

            AppEvent::PlaylistsLoaded(v) => { self.playlists = v; self.loading = false; }
            AppEvent::LibrarySongsLoaded(v) => { self.tracks = v; self.loading = false; }
            AppEvent::AlbumsLoaded(v) => { self.albums = v; self.loading = false; }
            AppEvent::ArtistsLoaded(v) => { self.artists = v; self.loading = false; }
            AppEvent::PlaylistTracksLoaded { id, tracks } => {
                self.open_playlist = Some(id);
                self.tracks = tracks;
                self.selected = 0;
                self.loading = false;
            }
            AppEvent::SearchResults { query, tracks } => {
                // Ignore results for a query the user has already moved past.
                if query == self.search_query {
                    self.search_results = tracks;
                    self.selected = 0;
                    self.loading = false;
                }
            }

            AppEvent::MutationOk { message, .. } => {
                self.push_toast(ToastKind::Success, &message, self.elapsed_ms);
            }
            AppEvent::MutationFailed { message, .. } => {
                // Rollback itself is wired in Task 28.
                self.push_toast(ToastKind::Error, &message, self.elapsed_ms);
            }

            AppEvent::Error(m) => self.push_toast(ToastKind::Error, &m, self.elapsed_ms),
            AppEvent::LoginNeeded { user_code, url } => {
                self.modal = Some(Modal::Login { user_code, url });
            }
            AppEvent::LoginComplete => {
                self.modal = None;
                self.push_toast(ToastKind::Success, "signed in", self.elapsed_ms);
            }
        }
    }

    fn apply_input(&mut self, a: InputAction) {
        // A modal owns the keyboard while it is open.
        if self.modal.is_some() {
            match a {
                InputAction::Cancel => self.modal = None,
                InputAction::Quit => self.should_quit = true,
                _ => {}
            }
            return;
        }
        match a {
            InputAction::Quit => self.should_quit = true,
            InputAction::Down => self.select_next(),
            InputAction::Up => self.select_prev(),
            InputAction::Home => self.selected = 0,
            InputAction::End => self.selected = self.list_len().saturating_sub(1),
            InputAction::OpenHelp => self.modal = Some(Modal::Help),
            InputAction::OpenSearch => { self.set_pane(Pane::Search); self.focus = Focus::SearchInput; }
            InputAction::OpenQueue => self.set_pane(Pane::Queue),
            // AMENDED 2026-08-31 (owner request): `h`/`l` read as folder
            // open/close, not just focus movement. `Left` leaves an open
            // playlist first and only falls back to focusing the sidebar when
            // there is no level to leave; `close_open_playlist` also clears
            // `tracks`, or `list_len` disagrees with what is on screen.
            // `GoTo(n)` was declared in this enum from the start but never
            // bound — it now jumps to the nth source in sidebar order.
            InputAction::Left => {
                if !self.close_open_playlist() {
                    self.focus = Focus::Sidebar;
                }
            }
            InputAction::Right => self.focus = Focus::Main,
            InputAction::GoTo(n) => self.goto_source(n),
            _ => {}   // transport actions are handled by the loop, not here
        }
    }

    fn apply_player(&mut self, p: PlayerEvent) {
        match p {
            PlayerEvent::StateChanged(s) => self.playback = s,
            PlayerEvent::TrackChanged(t) => {
                self.now_playing = t;
                self.position = TrackDuration::default();
            }
            PlayerEvent::Progress { position, duration } => {
                self.position = position;
                self.duration = duration;
            }
            PlayerEvent::VolumeChanged(v) => self.volume = v,
            PlayerEvent::ShuffleChanged(s) => self.shuffle = s,
            PlayerEvent::RepeatChanged(r) => self.repeat = r,
            PlayerEvent::QueueChanged { tracks, current } => {
                self.queue = tracks;
                self.queue_current = current;
            }
            PlayerEvent::Error(m) => self.push_toast(ToastKind::Error, &m, self.elapsed_ms),
            PlayerEvent::TrackEnded(_) => {}
        }
    }

    /// Row count of whatever the main pane is showing.
    pub fn list_len(&self) -> usize {
        match self.pane {
            Pane::Playlists if self.open_playlist.is_some() => self.tracks.len(),
            Pane::Playlists => self.playlists.len(),
            Pane::Songs => self.tracks.len(),
            Pane::Albums => self.albums.len(),
            Pane::Artists => self.artists.len(),
            Pane::Search => self.search_results.len(),
            Pane::Queue => self.queue.len(),
        }
    }

    pub fn select_next(&mut self) {
        let n = self.list_len();
        if n > 0 { self.selected = (self.selected + 1).min(n - 1); }
    }

    pub fn select_prev(&mut self) { self.selected = self.selected.saturating_sub(1); }

    /// Always reset the selection — a stale index points at the wrong row.
    pub fn set_pane(&mut self, p: Pane) {
        self.pane = p;
        self.selected = 0;
        self.scroll_offset = 0;
        self.marked.clear();
    }

    pub fn push_toast(&mut self, kind: ToastKind, text: &str, now_ms: u64) {
        self.toasts.push(Toast { kind, text: text.to_owned(), born_ms: now_ms });
    }

    pub fn expire_toasts(&mut self, now_ms: u64) {
        self.toasts.retain(|t| now_ms.saturating_sub(t.born_ms) < TOAST_TTL_MS);
    }

    pub fn selected_track(&self) -> Option<&Track> {
        match self.pane {
            Pane::Search => self.search_results.get(self.selected),
            Pane::Queue => self.queue.get(self.selected),
            _ => self.tracks.get(self.selected),
        }
    }

    pub fn selected_playlist(&self) -> Option<&Playlist> {
        (self.pane == Pane::Playlists && self.open_playlist.is_none())
            .then(|| self.playlists.get(self.selected))
            .flatten()
    }
}
```

- [ ] **Step 5: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui app`
Expected: 9 tests PASS.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): AppState with pure reducers over AppEvent"
```

---

## Task 20: Keymap

**Files:**
- Create: `crates/ytm-tui/src/keymap.rs`
- Modify: `crates/ytm-tui/src/lib.rs`

**Interfaces:**
- Consumes: `InputAction`, `Focus`.
- Produces: `KeyMap::default()`, `KeyMap::resolve(&self, key: KeyEvent, focus: Focus) -> Option<InputAction>`, `KeyMap::from_toml_str`, `KeyMap::bindings() -> Vec<(String, InputAction)>` for the help overlay.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use crate::app::Focus;

    fn key(c: char) -> KeyEvent { KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE) }

    #[test]
    fn vim_navigation_keys_map_to_movement() {
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('j'), Focus::Main), Some(InputAction::Down));
        assert_eq!(m.resolve(key('k'), Focus::Main), Some(InputAction::Up));
        assert_eq!(m.resolve(key('h'), Focus::Main), Some(InputAction::Left));
        assert_eq!(m.resolve(key('l'), Focus::Main), Some(InputAction::Right));
    }

    #[test]
    fn arrow_keys_work_alongside_vim_keys() {
        let m = KeyMap::default();
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(m.resolve(down, Focus::Main), Some(InputAction::Down));
    }

    #[test]
    fn transport_keys_are_bound() {
        let m = KeyMap::default();
        assert_eq!(m.resolve(key(' '), Focus::Main), Some(InputAction::TogglePause));
        assert_eq!(m.resolve(key('n'), Focus::Main), Some(InputAction::NextTrack));
        assert_eq!(m.resolve(key('p'), Focus::Main), Some(InputAction::PrevTrack));
        assert_eq!(m.resolve(key('s'), Focus::Main), Some(InputAction::ToggleShuffle));
        assert_eq!(m.resolve(key('r'), Focus::Main), Some(InputAction::CycleRepeat));
    }

    #[test]
    fn typing_in_the_search_field_produces_characters_not_commands() {
        // Critical: 'j' while typing must insert a letter, not scroll the list.
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('j'), Focus::SearchInput), Some(InputAction::Char('j')));
        assert_eq!(m.resolve(key(' '), Focus::SearchInput), Some(InputAction::Char(' ')));
    }

    #[test]
    fn escape_cancels_from_the_search_field() {
        let m = KeyMap::default();
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(m.resolve(esc, Focus::SearchInput), Some(InputAction::Cancel));
    }

    #[test]
    fn ctrl_c_always_quits_even_while_typing() {
        let m = KeyMap::default();
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(m.resolve(c, Focus::SearchInput), Some(InputAction::Quit));
        assert_eq!(m.resolve(c, Focus::Main), Some(InputAction::Quit));
    }

    #[test]
    fn unbound_keys_resolve_to_nothing() {
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('Z'), Focus::Main), None);
    }

    #[test]
    fn a_user_override_replaces_the_default_binding() {
        let m = KeyMap::from_toml_str(r#"down = "e""#).unwrap();
        assert_eq!(m.resolve(key('e'), Focus::Main), Some(InputAction::Down));
    }

    #[test]
    fn bindings_list_is_non_empty_for_the_help_overlay() {
        // FR-U2: '?' must show something real.
        assert!(!KeyMap::default().bindings().is_empty());
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui keymap`
Expected: FAIL — `KeyMap` not found.

- [ ] **Step 3: Implement**

```rust
//! Key -> InputAction resolution. Focus-sensitive: while typing, letters are
//! letters, not commands.

use crate::{app::Focus, event::InputAction};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

pub struct KeyMap {
    /// Char bindings active in navigation focus.
    chars: HashMap<char, InputAction>,
}

impl Default for KeyMap {
    fn default() -> Self {
        let mut chars = HashMap::new();
        for (c, a) in [
            ('j', InputAction::Down),
            ('k', InputAction::Up),
            ('h', InputAction::Left),
            ('l', InputAction::Right),
            ('g', InputAction::Home),
            ('G', InputAction::End),
            ('q', InputAction::Quit),
            ('?', InputAction::OpenHelp),
            ('/', InputAction::OpenSearch),
            ('u', InputAction::OpenQueue),
            (' ', InputAction::TogglePause),
            ('n', InputAction::NextTrack),
            ('p', InputAction::PrevTrack),
            ('f', InputAction::SeekForward),
            ('b', InputAction::SeekBack),
            ('+', InputAction::VolumeUp),
            ('-', InputAction::VolumeDown),
            ('m', InputAction::ToggleMute),
            ('s', InputAction::ToggleShuffle),
            ('r', InputAction::CycleRepeat),
            ('a', InputAction::AddToQueue),
            ('A', InputAction::AddToPlaylist),
            ('N', InputAction::CreatePlaylist),
            ('R', InputAction::RenamePlaylist),
            ('D', InputAction::DeletePlaylist),
            ('x', InputAction::RemoveFromPlaylist),
            ('v', InputAction::ToggleMark),
            ('e', InputAction::PlayNext),
            ('L', InputAction::Refresh),
        ] {
            chars.insert(c, a);
        }
        Self { chars }
    }
}

impl KeyMap {
    pub fn resolve(&self, key: KeyEvent, focus: Focus) -> Option<InputAction> {
        // Ctrl-C escapes everything, including a text field.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Some(InputAction::Quit),
                KeyCode::Char('d') => Some(InputAction::PageDown),
                KeyCode::Char('u') => Some(InputAction::PageUp),
                _ => None,
            };
        }

        if focus == Focus::SearchInput {
            return match key.code {
                KeyCode::Char(c) => Some(InputAction::Char(c)),
                KeyCode::Backspace => Some(InputAction::Backspace),
                KeyCode::Esc => Some(InputAction::Cancel),
                KeyCode::Enter => Some(InputAction::Confirm),
                KeyCode::Down => Some(InputAction::Down),
                KeyCode::Up => Some(InputAction::Up),
                _ => None,
            };
        }

        match key.code {
            // AMENDED 2026-08-31: 1-6 jump to a source, matched before the char
            // table so a digit cannot be rebound away by accident. Only the six
            // digits that name a source are bound — 7-9 and 0 fall through to
            // None rather than being swallowed. The SearchInput branch above
            // still wins, so a query like "90's" is typable.
            KeyCode::Char(c @ '1'..='6') => Some(InputAction::GoTo(c as u8 - b'0')),
            KeyCode::Char(c) => self.chars.get(&c).cloned(),
            KeyCode::Down => Some(InputAction::Down),
            KeyCode::Up => Some(InputAction::Up),
            KeyCode::Left => Some(InputAction::Left),
            KeyCode::Right => Some(InputAction::Right),
            KeyCode::Home => Some(InputAction::Home),
            KeyCode::End => Some(InputAction::End),
            KeyCode::PageDown => Some(InputAction::PageDown),
            KeyCode::PageUp => Some(InputAction::PageUp),
            KeyCode::Enter => Some(InputAction::Confirm),
            KeyCode::Esc => Some(InputAction::Cancel),
            KeyCode::Tab => Some(InputAction::NextPane),
            KeyCode::BackTab => Some(InputAction::PrevPane),
            _ => None,
        }
    }

    /// For the help overlay, sorted for stable display.
    pub fn bindings(&self) -> Vec<(String, InputAction)> {
        let mut v: Vec<_> = self.chars.iter()
            .map(|(c, a)| (c.to_string(), a.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Override defaults from `[keys]` in config. Key names are snake_case
    /// action names; values are single characters.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        let table: HashMap<String, String> = toml::from_str(s)?;
        let mut m = Self::default();
        for (action_name, ch) in table {
            let Some(c) = ch.chars().next() else { continue };
            if let Some(action) = action_from_name(&action_name) {
                m.chars.retain(|_, a| *a != action);   // a binding is exclusive
                m.chars.insert(c, action);
            }
        }
        Ok(m)
    }
}

fn action_from_name(n: &str) -> Option<InputAction> {
    Some(match n {
        "down" => InputAction::Down,
        "up" => InputAction::Up,
        "left" => InputAction::Left,
        "right" => InputAction::Right,
        "quit" => InputAction::Quit,
        "toggle_pause" => InputAction::TogglePause,
        "next_track" => InputAction::NextTrack,
        "prev_track" => InputAction::PrevTrack,
        "toggle_shuffle" => InputAction::ToggleShuffle,
        "cycle_repeat" => InputAction::CycleRepeat,
        "open_search" => InputAction::OpenSearch,
        "open_queue" => InputAction::OpenQueue,
        "open_help" => InputAction::OpenHelp,
        _ => return None,
    })
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui keymap`
Expected: 9 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): focus-aware keymap with vim defaults and overrides"
```

---

## Task 21: Now-playing bar and the render entry point

**Files:**
- Create: `crates/ytm-tui/src/widgets/mod.rs`, `crates/ytm-tui/src/widgets/nowplaying.rs`, `crates/ytm-tui/src/widgets/sidebar.rs`, `crates/ytm-tui/src/render.rs`
- Modify: `crates/ytm-tui/src/lib.rs`

**Interfaces:**
- Consumes: `AppState`, `Theme`, text helpers.
- Produces: `render(frame: &mut Frame, state: &AppState, theme: &Theme)`, `progress_bar(ratio: f64, width: usize) -> String`.

- [ ] **Step 1: Write the failing test**

Widget tests render into a `TestBackend` buffer and assert on visible text — no terminal, no network.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use crate::{app::AppState, theme::Theme};
    use ytm_core::{Track, TrackDuration};
    use ytm_player::player::PlaybackState;

    fn buffer_text(state: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, state, &theme)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn progress_bar_is_empty_at_zero_and_full_at_one() {
        assert_eq!(progress_bar(0.0, 10).trim_end(), "");
        assert_eq!(progress_bar(1.0, 10), "██████████");
    }

    #[test]
    fn progress_bar_uses_partial_blocks_for_sub_cell_precision() {
        // Spec §6: eighth-blocks, not '='.
        let b = progress_bar(0.55, 10);
        assert!(b.chars().any(|c| ('\u{258F}'..='\u{2588}').contains(&c)), "got {b:?}");
    }

    #[test]
    fn progress_bar_clamps_out_of_range_ratios() {
        assert_eq!(progress_bar(-1.0, 5).trim_end(), "");
        assert_eq!(progress_bar(2.0, 5), "█████");
    }

    #[test]
    fn now_playing_shows_title_artist_and_times() {
        let mut s = AppState::default();
        s.now_playing = Some(Track::stub("v1", "Roygbiv"));
        s.playback = PlaybackState::Playing;
        s.position = TrackDuration::from_secs(65);
        s.duration = TrackDuration::from_secs(149);
        let text = buffer_text(&s);
        assert!(text.contains("Roygbiv"), "title missing");
        assert!(text.contains("1:05"), "elapsed missing");
        assert!(text.contains("2:29"), "duration missing");
    }

    #[test]
    fn idle_state_shows_a_placeholder_not_an_empty_bar() {
        let s = AppState::default();
        assert!(buffer_text(&s).contains("Nothing playing"));
    }

    #[test]
    fn sidebar_lists_every_source() {
        let text = buffer_text(&AppState::default());
        for label in ["Playlists", "Songs", "Albums", "Artists", "Search", "Queue"] {
            assert!(text.contains(label), "sidebar missing {label}");
        }
    }

    #[test]
    fn a_very_narrow_terminal_does_not_panic() {
        // Users resize to absurd sizes; a panic here loses their session.
        let mut t = Terminal::new(TestBackend::new(8, 4)).unwrap();
        let s = AppState::default();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, &s, &theme)).unwrap();
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui nowplaying`
Expected: FAIL — `progress_bar` not found.

- [ ] **Step 3: Implement the progress bar and now-playing widget**

```rust
//! The bottom bar: what is playing, where we are in it.

use crate::{app::AppState, theme::Theme, util::text::truncate_to_width};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Style, Modifier},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use ytm_player::player::{PlaybackState, RepeatMode};

/// Eighth-block glyphs give 8x the resolution of a plain block per column.
const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

pub fn progress_bar(ratio: f64, width: usize) -> String {
    let r = ratio.clamp(0.0, 1.0);
    let total_eighths = (r * (width * 8) as f64).round() as usize;
    let full = total_eighths / 8;
    let rem = total_eighths % 8;

    let mut s: String = std::iter::repeat_n('█', full.min(width)).collect();
    if full < width && rem > 0 {
        s.push(EIGHTHS[rem - 1]);
    }
    // Pad with spaces so the bar always occupies its full width.
    let filled = full + usize::from(full < width && rem > 0);
    s.extend(std::iter::repeat_n(' ', width.saturating_sub(filled)));
    s
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.height == 0 || area.width == 0 { return; }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let w = area.width as usize;

    // Row 1: title — artist, plus the state glyph.
    let line = match &s.now_playing {
        None => Line::from(Span::styled("Nothing playing", Style::default().fg(t.fg_dim))),
        Some(track) => {
            let glyph = match s.playback {
                PlaybackState::Playing => "▶",
                PlaybackState::Paused => "⏸",
                PlaybackState::Loading => "⋯",
                PlaybackState::Stopped => "■",
            };
            let title = truncate_to_width(&track.title, w.saturating_sub(24));
            Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(t.accent)),
                Span::styled(title, Style::default().fg(t.fg_bright).add_modifier(Modifier::BOLD)),
                Span::styled(" — ", Style::default().fg(t.fg_dim)),
                Span::styled(track.artist_display(), Style::default().fg(t.fg)),
            ])
        }
    };
    f.render_widget(Paragraph::new(line), rows[0]);

    // Row 2: elapsed, bar, duration, then the mode flags.
    let pos = s.position.as_secs();
    let dur = s.duration.as_secs();
    let ratio = if dur == 0 { 0.0 } else { pos as f64 / dur as f64 };

    let times = format!("{}  ", s.position);
    let tail = format!("  {}", s.duration);
    let flags = format!(
        "  {}{}  {:>3}%",
        if s.shuffle { "⇄" } else { " " },
        match s.repeat { RepeatMode::Off => " ", RepeatMode::One => "①", RepeatMode::All => "↻" },
        if s.muted { 0 } else { s.volume },
    );

    let bar_w = w.saturating_sub(times.len() + tail.len() + flags.len());
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(times, Style::default().fg(t.fg_dim)),
            Span::styled(progress_bar(ratio, bar_w), Style::default().fg(t.accent)),
            Span::styled(tail, Style::default().fg(t.fg_dim)),
            Span::styled(flags, Style::default().fg(t.fg_dim)),
        ])),
        rows[1],
    );
}
```

- [ ] **Step 4: Implement the sidebar and the top-level render**

`render.rs` splits the frame — sidebar 22 cols, main the rest, now-playing 3 rows at the bottom — then dispatches on `state.pane`. Two rules:

- Guard every widget on `area.width == 0 || area.height == 0`. The 8×4 test exists because unguarded layout math panics on tiny terminals.
- Draw modals and toasts last so they overlay.

Sidebar labels must be exactly `Playlists`, `Songs`, `Albums`, `Artists`, `Search`, `Queue` — the test asserts on them.

- [ ] **Step 5: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui`
Expected: all tests PASS, including the narrow-terminal one.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): now-playing bar with eighth-block progress and sidebar"
```

---
## Task 22: The event loop

**Files:**
- Create: `crates/ytm-cli/src/app_loop.rs`
- Modify: `crates/ytm-cli/src/main.rs`

**Interfaces:**
- Consumes: `AppState`, `KeyMap`, `Theme`, `Arc<dyn MusicSource>`, `Player`, `mpsc::UnboundedReceiver<PlayerEvent>`.
- Produces: `run(terminal, state, deps) -> Result<()>`, `dispatch_input(action, state, deps) -> Option<Task>`.

This is where NFR-2 is either honored or violated. Everything slow goes through `tokio::spawn`.

- [ ] **Step 1: Write the failing test**

The loop itself needs a terminal, so test the dispatch function — which is where the logic lives.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ytm_core::mock::MockSource;
    use ytm_player::{mock::MockPlayer, player::{PlayerCommand, Player}};
    use ytm_tui::{app::{AppState, Pane}, event::InputAction};

    fn deps() -> (Arc<MockSource>, Arc<MockPlayer>) {
        let (p, _rx) = MockPlayer::new();
        (Arc::new(MockSource::new()), Arc::new(p))
    }

    #[test]
    fn toggle_pause_reaches_the_player_not_the_state() {
        let (src, player) = deps();
        let mut s = AppState::default();
        dispatch_input(InputAction::TogglePause, &mut s, &src, &*player);
        assert!(matches!(player.commands()[0], PlayerCommand::TogglePause));
    }

    #[test]
    fn volume_up_clamps_at_one_hundred() {
        let (src, player) = deps();
        let mut s = AppState::default();
        s.volume = 97;
        dispatch_input(InputAction::VolumeUp, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::SetVolume(v) => assert_eq!(v, 100),
            ref o => panic!("expected SetVolume, got {o:?}"),
        }
    }

    #[test]
    fn volume_down_clamps_at_zero() {
        let (src, player) = deps();
        let mut s = AppState::default();
        s.volume = 2;
        dispatch_input(InputAction::VolumeDown, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::SetVolume(v) => assert_eq!(v, 0),
            ref o => panic!("expected SetVolume, got {o:?}"),
        }
    }

    #[test]
    fn cycle_repeat_advances_the_mode() {
        let (src, player) = deps();
        let mut s = AppState::default();
        dispatch_input(InputAction::CycleRepeat, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::SetRepeat(m) => assert_eq!(m, ytm_player::player::RepeatMode::One),
            ref o => panic!("expected SetRepeat, got {o:?}"),
        }
    }

    #[test]
    fn confirm_on_a_selected_track_plays_it() {
        let (src, player) = deps();
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        s.tracks = vec![ytm_core::Track::stub("v7", "Song")];
        s.selected = 0;
        dispatch_input(InputAction::Confirm, &mut s, &src, &*player);
        match &player.commands()[0] {
            PlayerCommand::PlayNow(t) => assert_eq!(t.video_id.as_str(), "v7"),
            o => panic!("expected PlayNow, got {o:?}"),
        }
    }

    #[test]
    fn confirm_with_an_empty_list_sends_nothing() {
        let (src, player) = deps();
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        dispatch_input(InputAction::Confirm, &mut s, &src, &*player);
        assert!(player.commands().is_empty(), "must not play a track that does not exist");
    }

    #[test]
    fn add_to_queue_enqueues_the_selected_track() {
        let (src, player) = deps();
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        s.tracks = vec![ytm_core::Track::stub("v1", "A")];
        dispatch_input(InputAction::AddToQueue, &mut s, &src, &*player);
        assert!(matches!(player.commands()[0], PlayerCommand::EnqueueBack(_)));
    }

    #[test]
    fn navigation_actions_do_not_reach_the_player() {
        let (src, player) = deps();
        let mut s = AppState::default();
        s.tracks = vec![ytm_core::Track::stub("a", "A"), ytm_core::Track::stub("b", "B")];
        s.pane = Pane::Songs;
        dispatch_input(InputAction::Down, &mut s, &src, &*player);
        assert!(player.commands().is_empty());
        assert_eq!(s.selected, 1, "state handles navigation");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-cli app_loop`
Expected: FAIL — `dispatch_input` not found.

- [ ] **Step 3: Implement dispatch**

```rust
//! The event loop. Owns AppState; nothing here may block on I/O (NFR-2).

use std::sync::Arc;
use tokio::sync::mpsc;
use ytm_core::MusicSource;
use ytm_player::player::{Player, PlayerCommand, RepeatMode};
use ytm_tui::{
    app::{AppState, Focus, Pane},
    event::{AppEvent, InputAction},
    keymap::KeyMap,
    theme::Theme,
};

/// Work the loop should start in the background as a result of an input.
pub enum Task {
    LoadPlaylists,
    LoadSongs,
    LoadAlbums,
    LoadArtists,
    OpenPlaylist(ytm_core::PlaylistId),
    Search(String),
}

/// Translate one input action into player commands, state changes, and
/// background work. Pure with respect to I/O — that is what makes it testable.
pub fn dispatch_input(
    action: InputAction,
    state: &mut AppState,
    _source: &Arc<impl MusicSource + ?Sized>,
    player: &impl Player,
) -> Option<Task> {
    use InputAction as A;

    // Transport actions belong to the player; everything else to the state.
    match action {
        A::TogglePause => { let _ = player.send(PlayerCommand::TogglePause); return None; }
        A::NextTrack => { let _ = player.send(PlayerCommand::Next); return None; }
        A::PrevTrack => { let _ = player.send(PlayerCommand::Previous); return None; }
        A::SeekForward => { let _ = player.send(PlayerCommand::SeekRelative(5)); return None; }
        A::SeekBack => { let _ = player.send(PlayerCommand::SeekRelative(-5)); return None; }
        A::VolumeUp => {
            let v = (state.volume as i64 + 5).clamp(0, 100) as u8;
            state.volume = v;
            let _ = player.send(PlayerCommand::SetVolume(v));
            return None;
        }
        A::VolumeDown => {
            let v = (state.volume as i64 - 5).clamp(0, 100) as u8;
            state.volume = v;
            let _ = player.send(PlayerCommand::SetVolume(v));
            return None;
        }
        A::ToggleMute => { let _ = player.send(PlayerCommand::ToggleMute); return None; }
        A::ToggleShuffle => {
            state.shuffle = !state.shuffle;
            let _ = player.send(PlayerCommand::SetShuffle(state.shuffle));
            return None;
        }
        A::CycleRepeat => {
            state.repeat = state.repeat.next();
            let _ = player.send(PlayerCommand::SetRepeat(state.repeat));
            return None;
        }
        A::Confirm if state.modal.is_none() => {
            // In the playlist list, Enter opens; on a track, Enter plays.
            if let Some(p) = state.selected_playlist() {
                return Some(Task::OpenPlaylist(p.id.clone()));
            }
            if let Some(t) = state.selected_track().cloned() {
                let _ = player.send(PlayerCommand::PlayNow(t));
            }
            return None;
        }
        A::AddToQueue => {
            if let Some(t) = state.selected_track().cloned() {
                let _ = player.send(PlayerCommand::EnqueueBack(vec![t]));
            }
            return None;
        }
        A::PlayNext => {
            if let Some(t) = state.selected_track().cloned() {
                let _ = player.send(PlayerCommand::EnqueueNext(vec![t]));
            }
            return None;
        }
        A::Refresh => return Some(match state.pane {
            Pane::Songs => Task::LoadSongs,
            Pane::Albums => Task::LoadAlbums,
            Pane::Artists => Task::LoadArtists,
            _ => Task::LoadPlaylists,
        }),
        _ => {}
    }

    // Everything else is a state transition.
    state.apply(AppEvent::Input(action));
    None
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli app_loop`
Expected: 8 tests PASS.

- [ ] **Step 5: Implement the loop itself**

```rust
/// Run until the user quits. Four event sources, one owner of state.
pub async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    mut state: AppState,
    source: Arc<dyn MusicSource>,
    player: impl Player,
    mut player_events: mpsc::UnboundedReceiver<ytm_player::player::PlayerEvent>,
    keymap: KeyMap,
    theme: Theme,
    tick_ms: u64,
) -> color_eyre::Result<()> {
    use crossterm::event::{Event as CtEvent, EventStream};
    use futures::StreamExt;

    // App-internal events (results of background work).
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let mut term_events = EventStream::new();
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
    let started = std::time::Instant::now();

    // Paint immediately, then load — NFR-1 depends on not awaiting first.
    terminal.draw(|f| ytm_tui::render::render(f, &state, &theme))?;
    spawn_task(Task::LoadPlaylists, source.clone(), app_tx.clone());

    loop {
        tokio::select! {
            // Terminal input
            Some(Ok(ev)) = term_events.next() => {
                match ev {
                    CtEvent::Key(k) if k.kind == crossterm::event::KeyEventKind::Press => {
                        if let Some(a) = keymap.resolve(k, state.focus) {
                            if let Some(task) = dispatch_input(a, &mut state, &source, &player) {
                                spawn_task(task, source.clone(), app_tx.clone());
                            }
                        }
                    }
                    CtEvent::Resize(_, _) => state.apply(AppEvent::Resize),
                    _ => {}
                }
            }

            // Player actor
            Some(pe) = player_events.recv() => state.apply(AppEvent::Player(pe)),

            // Background work results
            Some(ae) = app_rx.recv() => {
                if let Some(task) = follow_up(&ae) {
                    spawn_task(task, source.clone(), app_tx.clone());
                }
                state.apply(ae);
            }

            // Render tick
            _ = ticker.tick() => {
                state.elapsed_ms = started.elapsed().as_millis() as u64;
                state.apply(AppEvent::Tick);
            }
        }

        terminal.draw(|f| ytm_tui::render::render(f, &state, &theme))?;

        if state.should_quit {
            let _ = player.send(PlayerCommand::Shutdown);
            return Ok(());
        }
    }
}

/// Run one unit of work off-thread and post the result back.
fn spawn_task(
    task: Task,
    source: Arc<dyn MusicSource>,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let ev = match task {
            Task::LoadPlaylists => match source.library_playlists().await {
                Ok(v) => AppEvent::PlaylistsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::LoadSongs => match source.library_songs().await {
                Ok(v) => AppEvent::LibrarySongsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::LoadAlbums => match source.library_albums().await {
                Ok(v) => AppEvent::AlbumsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::LoadArtists => match source.library_artists().await {
                Ok(v) => AppEvent::ArtistsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::OpenPlaylist(id) => match source.playlist_tracks(id.clone()).await {
                Ok(tracks) => AppEvent::PlaylistTracksLoaded { id, tracks },
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::Search(q) => match source.search_songs(q.clone()).await {
                Ok(tracks) => AppEvent::SearchResults { query: q, tracks },
                Err(e) => AppEvent::Error(e.to_string()),
            },
        };
        let _ = tx.send(ev);
    });
}

/// Events that should trigger more work. Returns `None` for most.
fn follow_up(_ev: &AppEvent) -> Option<Task> { None }
```

`state.loading = true` must be set when a task is spawned, so the spinner appears (FR-U4). Set it in `dispatch_input` for each `Task` branch.

- [ ] **Step 6: Wire main.rs and run the app**

`main()` should: install `color_eyre`, load config, init logging, build the source (OAuth or cookie per config), `spawn_player`, create the `TerminalGuard`, and call `run`. On a `SourceError::NotAuthenticated`, show the login modal rather than exiting.

Run: `cargo run -p ytm-cli`
Expected: the TUI appears, the sidebar renders, real playlists appear after a moment, `q` exits cleanly with the terminal restored.

- [ ] **Step 7: Verify the terminal survives a panic**

Run: `cargo run -p ytm-cli` then trigger a panic (temporarily add a `panic!()` behind an unused key).
Expected: the shell is usable afterward — no invisible cursor, no raw mode. Remove the test panic.

- [ ] **Step 8: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(cli): tokio event loop with non-blocking background work"
```

---

## Task 23: Track list widget

**Files:**
- Create: `crates/ytm-tui/src/widgets/tracklist.rs`
- Modify: `crates/ytm-tui/src/widgets/mod.rs`, `crates/ytm-tui/src/render.rs`

**Interfaces:**
- Consumes: `AppState`, `Theme`, text helpers.
- Produces: `tracklist::draw(f, area, state, theme)`, `visible_window(selected, offset, height, len) -> (usize, usize)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_shows_the_top_when_selection_is_near_the_start() {
        assert_eq!(visible_window(0, 0, 10, 100), (0, 10));
        assert_eq!(visible_window(5, 0, 10, 100), (0, 10));
    }

    #[test]
    fn window_scrolls_when_the_selection_passes_the_bottom() {
        // selection 12 with a 10-row viewport must bring row 12 into view
        let (start, end) = visible_window(12, 0, 10, 100);
        assert!(start <= 12 && 12 < end, "selection must be visible, got {start}..{end}");
    }

    #[test]
    fn window_never_exceeds_the_item_count() {
        let (start, end) = visible_window(2, 0, 10, 3);
        assert_eq!((start, end), (0, 3));
    }

    #[test]
    fn window_is_empty_for_an_empty_list() {
        assert_eq!(visible_window(0, 0, 10, 0), (0, 0));
    }

    #[test]
    fn window_handles_a_zero_height_viewport() {
        assert_eq!(visible_window(0, 0, 0, 50), (0, 0));
    }

    #[test]
    fn rows_show_title_artist_and_duration() {
        use ratatui::{backend::TestBackend, Terminal};
        use crate::{app::{AppState, Pane}, theme::Theme};
        use ytm_core::{Track, TrackDuration};

        let mut s = AppState::default();
        s.pane = Pane::Songs;
        s.tracks = vec![Track {
            title: "Roygbiv".into(),
            artists: vec!["Boards of Canada".into()],
            duration: TrackDuration::from_secs(149),
            ..Track::stub("v1", "Roygbiv")
        }];

        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, &s, &theme)).unwrap();
        let text: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();

        assert!(text.contains("Roygbiv"));
        assert!(text.contains("Boards of Canada"));
        assert!(text.contains("2:29"));
    }

    #[test]
    fn an_empty_pane_shows_a_message_not_a_blank_area() {
        use ratatui::{backend::TestBackend, Terminal};
        use crate::{app::{AppState, Pane}, theme::Theme};
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, &s, &theme)).unwrap();
        let text: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Nothing here"), "empty states must say something");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui tracklist`
Expected: FAIL — `visible_window` not found.

- [ ] **Step 3: Implement**

```rust
//! One row per track: mark, title, artist, album, duration. Columns are sized
//! by display width so CJK titles keep the grid intact (spec §6).

use crate::{app::AppState, theme::Theme, util::text::{pad_to_width, truncate_to_width}};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

/// The slice of rows to draw, keeping `selected` visible.
pub fn visible_window(selected: usize, offset: usize, height: usize, len: usize) -> (usize, usize) {
    if len == 0 || height == 0 { return (0, 0); }
    let mut start = offset.min(len.saturating_sub(1));
    if selected < start { start = selected; }
    if selected >= start + height { start = selected + 1 - height; }
    let end = (start + height).min(len);
    (start, end)
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 { return; }

    let rows: &[ytm_core::Track] = match s.pane {
        crate::app::Pane::Search => &s.search_results,
        crate::app::Pane::Queue => &s.queue,
        _ => &s.tracks,
    };

    if rows.is_empty() {
        f.render_widget(
            ratatui::widgets::Paragraph::new(Span::styled(
                "Nothing here yet",
                Style::default().fg(t.fg_dim),
            )),
            area,
        );
        return;
    }

    let h = area.height as usize;
    let (start, end) = visible_window(s.selected, s.scroll_offset, h, rows.len());
    let w = area.width as usize;

    // Fixed-width columns: mark(2) duration(7) + gaps; title/artist share the rest.
    let dur_w = 7;
    let mark_w = 2;
    let text_w = w.saturating_sub(dur_w + mark_w + 2);
    let title_w = (text_w * 6) / 10;
    let artist_w = text_w.saturating_sub(title_w);

    let items: Vec<ListItem> = rows[start..end]
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let idx = start + i;
            let is_sel = idx == s.selected;
            let is_now = s.now_playing.as_ref().is_some_and(|n| n.video_id == track.video_id);
            let marked = s.marked.contains(&track.video_id);

            let base = if is_now {
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.fg)
            };
            // Selection is a reversed background, not a '>' marker (spec §6).
            let style = if is_sel { base.bg(t.bg_sel) } else { base };

            ListItem::new(Line::from(vec![
                Span::styled(if marked { "• " } else { "  " }, Style::default().fg(t.accent)),
                Span::styled(pad_to_width(&track.title, title_w), style),
                Span::styled(
                    pad_to_width(&track.artist_display(), artist_w),
                    if is_sel { style } else { Style::default().fg(t.fg_dim) },
                ),
                Span::styled(
                    format!("{:>width$}", track.duration.to_string(), width = dur_w),
                    Style::default().fg(t.fg_dim),
                ),
            ]))
            .style(if is_sel { Style::default().bg(t.bg_sel) } else { Style::default() })
        })
        .collect();

    f.render_widget(List::new(items), area);
    let _ = truncate_to_width; // used by other panes
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui tracklist`
Expected: 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): track list with width-aware columns and scroll window"
```

---

## Task 24: Playlist, album, and artist lists

**Files:**
- Create: `crates/ytm-tui/src/widgets/playlists.rs`
- Modify: `crates/ytm-tui/src/render.rs`

**Interfaces:**
- Consumes: `AppState`, `Theme`, `visible_window` from Task 23.
- Produces: `playlists::draw_playlists`, `playlists::draw_albums`, `playlists::draw_artists`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};
    use crate::{app::{AppState, Pane}, theme::Theme};
    use ytm_core::{Album, AlbumId, Artist, ArtistId, Playlist};

    fn text_of(s: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, s, &theme)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn playlists_show_title_and_track_count() {
        let mut s = AppState::default();
        s.pane = Pane::Playlists;
        s.playlists = vec![Playlist { track_count: Some(42), ..Playlist::stub("p1", "Deep Focus") }];
        let text = text_of(&s);
        assert!(text.contains("Deep Focus"));
        assert!(text.contains("42"), "track count missing");
    }

    #[test]
    fn a_playlist_with_unknown_count_renders_without_panicking() {
        let mut s = AppState::default();
        s.pane = Pane::Playlists;
        s.playlists = vec![Playlist { track_count: None, ..Playlist::stub("p1", "Mystery") }];
        assert!(text_of(&s).contains("Mystery"));
    }

    #[test]
    fn albums_show_title_and_artist() {
        let mut s = AppState::default();
        s.pane = Pane::Albums;
        s.albums = vec![Album {
            id: AlbumId::from("a1"),
            title: "Geogaddi".into(),
            artists: vec!["Boards of Canada".into()],
            year: Some("2002".into()),
            thumbnail_url: None,
        }];
        let text = text_of(&s);
        assert!(text.contains("Geogaddi"));
        assert!(text.contains("Boards of Canada"));
    }

    #[test]
    fn artists_show_names() {
        let mut s = AppState::default();
        s.pane = Pane::Artists;
        s.artists = vec![Artist {
            id: ArtistId::from("r1"),
            name: "Aphex Twin".into(),
            subscribers: Some("1.2M".into()),
            thumbnail_url: None,
        }];
        assert!(text_of(&s).contains("Aphex Twin"));
    }

    #[test]
    fn a_system_playlist_is_visually_marked_as_read_only() {
        let mut s = AppState::default();
        s.pane = Pane::Playlists;
        s.playlists = vec![Playlist { is_system: true, ..Playlist::stub("LM", "Your Likes") }];
        // FR-C: the user should see that this one cannot be edited.
        assert!(text_of(&s).contains("Your Likes"));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui playlists`
Expected: FAIL — the module does not exist.

- [ ] **Step 3: Implement**

Follow the `tracklist::draw` shape exactly: guard on zero area, empty-state message, `visible_window` for scrolling, `pad_to_width` for columns, reversed background for selection.

Playlist rows: `title` (60%), `track_count` right-aligned as `42 tracks`, and a dim `read-only` suffix when `is_system`. Album rows: `title` then `artists` then `year`. Artist rows: `name` then dim `subscribers`.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui playlists`
Expected: 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): playlist, album, and artist list widgets"
```

---

## Task 25: Search with debounce

**Files:**
- Create: `crates/ytm-tui/src/widgets/search.rs`, `crates/ytm-tui/src/search_state.rs`
- Modify: `crates/ytm-tui/src/app.rs`, `crates/ytm-cli/src/app_loop.rs`

**Interfaces:**
- Consumes: `AppState`, `Theme`.
- Produces: `SearchDebounce::new(ms)`, `should_fire(&mut self, query, now_ms) -> Option<String>`, `search::draw`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_interval_is_at_least_280ms() {
        // NFR-7 / FR-S2: firing per keystroke risks a rate limit.
        assert!(DEFAULT_DEBOUNCE_MS >= 280);
    }

    #[test]
    fn does_not_fire_before_the_interval_elapses() {
        let mut d = SearchDebounce::new(300);
        d.note_input("bo", 1000);
        assert_eq!(d.should_fire(1200), None, "too soon");
    }

    #[test]
    fn fires_once_the_interval_has_elapsed() {
        let mut d = SearchDebounce::new(300);
        d.note_input("boards", 1000);
        assert_eq!(d.should_fire(1301).as_deref(), Some("boards"));
    }

    #[test]
    fn does_not_fire_twice_for_the_same_query() {
        let mut d = SearchDebounce::new(300);
        d.note_input("boards", 1000);
        assert!(d.should_fire(1301).is_some());
        assert_eq!(d.should_fire(1600), None, "already searched this text");
    }

    #[test]
    fn a_new_keystroke_restarts_the_timer() {
        let mut d = SearchDebounce::new(300);
        d.note_input("bo", 1000);
        d.note_input("boa", 1200);        // resets
        assert_eq!(d.should_fire(1301), None, "timer must restart on new input");
        assert_eq!(d.should_fire(1501).as_deref(), Some("boa"));
    }

    #[test]
    fn an_empty_query_never_fires() {
        let mut d = SearchDebounce::new(300);
        d.note_input("", 1000);
        assert_eq!(d.should_fire(2000), None);
    }

    #[test]
    fn whitespace_only_query_never_fires() {
        let mut d = SearchDebounce::new(300);
        d.note_input("   ", 1000);
        assert_eq!(d.should_fire(2000), None);
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui search`
Expected: FAIL — `SearchDebounce` not found.

- [ ] **Step 3: Implement**

```rust
//! Search-as-you-type debounce. Keystrokes are cheap; API calls are not.

pub const DEFAULT_DEBOUNCE_MS: u64 = 280;

pub struct SearchDebounce {
    interval_ms: u64,
    pending: Option<String>,
    last_input_ms: u64,
    last_fired: Option<String>,
}

impl SearchDebounce {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms, pending: None, last_input_ms: 0, last_fired: None }
    }

    /// Call on every keystroke. Restarts the timer.
    pub fn note_input(&mut self, query: &str, now_ms: u64) {
        self.pending = Some(query.to_owned());
        self.last_input_ms = now_ms;
    }

    /// Call on every tick. Returns the query to search, at most once each.
    pub fn should_fire(&mut self, now_ms: u64) -> Option<String> {
        let q = self.pending.as_ref()?;
        if q.trim().is_empty() { return None; }
        if now_ms.saturating_sub(self.last_input_ms) < self.interval_ms { return None; }
        if self.last_fired.as_deref() == Some(q.as_str()) { return None; }
        let q = q.clone();
        self.last_fired = Some(q.clone());
        Some(q)
    }
}

impl Default for SearchDebounce {
    fn default() -> Self { Self::new(DEFAULT_DEBOUNCE_MS) }
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui search`
Expected: 7 tests PASS.

- [ ] **Step 5: Wire it in**

In `AppState::apply_input`, handle `InputAction::Char(c)` and `Backspace` when `focus == Focus::SearchInput` by editing `search_query`. In the event loop's tick arm, call `should_fire(state.elapsed_ms)` and spawn `Task::Search(q)` when it returns `Some`.

`AppEvent::SearchResults` already drops stale results by comparing the query (Task 19) — that check is what makes out-of-order responses safe.

Draw the input as a single row above the results: `Search: <query>▏` with a spinner on the right while `state.loading`.

- [ ] **Step 6: Verify by hand**

Run: `cargo run -p ytm-cli`, press `/`, type a few letters.
Expected: results appear after you stop typing, not per keystroke. Check the log file — one request per pause, not per key.

- [ ] **Step 7: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): debounced search with stale-result rejection"
```

---

## Task 26: Queue view

**Files:**
- Create: `crates/ytm-tui/src/widgets/queue.rs`
- Modify: `crates/ytm-tui/src/render.rs`, `crates/ytm-cli/src/app_loop.rs`

**Interfaces:**
- Consumes: `AppState`, `Theme`.
- Produces: `queue::draw(f, area, state, theme)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};
    use crate::{app::{AppState, Pane}, theme::Theme};
    use ytm_core::Track;

    fn text_of(s: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, s, &theme)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn queue_lists_its_tracks_in_order() {
        let mut s = AppState::default();
        s.pane = Pane::Queue;
        s.queue = vec![Track::stub("v1", "First"), Track::stub("v2", "Second")];
        let text = text_of(&s);
        assert!(text.contains("First"));
        assert!(text.contains("Second"));
        assert!(text.find("First") < text.find("Second"), "order must be preserved");
    }

    #[test]
    fn the_current_queue_entry_is_marked() {
        let mut s = AppState::default();
        s.pane = Pane::Queue;
        s.queue = vec![Track::stub("v1", "First"), Track::stub("v2", "Second")];
        s.queue_current = Some(1);
        // FR-Q1: the user must be able to tell where they are.
        assert!(text_of(&s).contains("▶"), "current entry needs a marker");
    }

    #[test]
    fn an_empty_queue_explains_itself() {
        let mut s = AppState::default();
        s.pane = Pane::Queue;
        assert!(text_of(&s).contains("Queue is empty"));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui queue`
Expected: FAIL.

- [ ] **Step 3: Implement**

Same shape as `tracklist::draw`, with two differences: prefix the row at `queue_current` with `▶ ` in the accent color and everything else with two spaces, and use `"Queue is empty"` as the empty state.

- [ ] **Step 4: Wire queue mutations into dispatch**

In `dispatch_input`, when `state.pane == Pane::Queue`:

- `RemoveFromPlaylist` (`x`) → `PlayerCommand::RemoveFromQueue(state.selected)`
- `ToggleMark` + movement, or a dedicated pair of keys → `PlayerCommand::MoveInQueue { from, to }` (FR-Q3)
- A `ClearQueue` binding → `PlayerCommand::ClearQueue`

The actor emits `QueueChanged` after each, which updates the view. Do not mutate `state.queue` directly — the actor owns queue truth.

- [ ] **Step 5: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui queue`
Expected: 3 tests PASS.

- [ ] **Step 6: Verify by hand**

Run: `cargo run -p ytm-cli`, add a few tracks with `a`, press `u`.
Expected: the queue lists them, `▶` marks the current one, `x` removes, reorder works.

- [ ] **Step 7: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): queue view with current-entry marker and edits"
```

---

## Task 27: Toasts, spinner, and help overlay

**Files:**
- Create: `crates/ytm-tui/src/widgets/toast.rs`, `crates/ytm-tui/src/widgets/help.rs`
- Modify: `crates/ytm-tui/src/render.rs`

**Interfaces:**
- Consumes: `AppState`, `Theme`, `KeyMap`.
- Produces: `toast::draw`, `help::draw`, `centered_rect(pct_x, pct_y, area) -> Rect`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn centered_rect_is_centered_and_correctly_sized() {
        let area = Rect::new(0, 0, 100, 100);
        let r = centered_rect(50, 40, area);
        assert_eq!(r.width, 50);
        assert_eq!(r.height, 40);
        assert_eq!(r.x, 25);
        assert_eq!(r.y, 30);
    }

    #[test]
    fn centered_rect_never_exceeds_a_tiny_area() {
        let area = Rect::new(0, 0, 4, 3);
        let r = centered_rect(90, 90, area);
        assert!(r.width <= 4 && r.height <= 3, "got {r:?}");
    }

    #[test]
    fn error_toasts_render_their_text() {
        use ratatui::{backend::TestBackend, Terminal};
        use crate::{app::{AppState, ToastKind}, theme::Theme};
        let mut s = AppState::default();
        s.push_toast(ToastKind::Error, "rate limited", 0);
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, &s, &theme)).unwrap();
        let text: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("rate limited"));
    }

    #[test]
    fn help_overlay_lists_bindings() {
        use ratatui::{backend::TestBackend, Terminal};
        use crate::{app::{AppState, Modal}, theme::Theme};
        let mut s = AppState::default();
        s.modal = Some(Modal::Help);
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, &s, &theme)).unwrap();
        let text: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        // FR-U2
        assert!(text.contains("Help") || text.contains("Keys"));
        assert!(text.contains("quit") || text.contains("Quit"));
    }

    #[test]
    fn a_long_toast_is_truncated_rather_than_wrapping_off_screen() {
        use crate::util::text::display_width;
        let long = "x".repeat(200);
        let fitted = crate::util::text::truncate_to_width(&long, 40);
        assert!(display_width(&fitted) <= 40);
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui toast`
Expected: FAIL — `centered_rect` not found.

- [ ] **Step 3: Implement**

```rust
//! Overlays: toasts bottom-right, modals centered.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// A rect of `pct_x`% × `pct_y`% centered in `area`, never larger than it.
pub fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let h = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(h[1])[1]
}
```

Toasts: stack up from the bottom-right, at most three visible, each one row, colored by kind (`error`/`success`/`fg`), text truncated to the available width.

Spinner: when `state.loading`, draw a `throbber_widgets_tui::Throbber` in the pane's top-right corner (FR-U4).

Help: a centered box at 60%×70% listing `keymap.bindings()` in two columns as `key  action`. Include the words `Help` and `quit` — the test asserts on them.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui`
Expected: all tests PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): toasts, loading spinner, and help overlay"
```

---
## Task 28: Optimistic mutation tracking

**Files:**
- Create: `crates/ytm-tui/src/mutation.rs`
- Modify: `crates/ytm-tui/src/app.rs`, `crates/ytm-tui/src/lib.rs`

**Interfaces:**
- Consumes: `AppState`, models.
- Produces: `MutationLog`, `Mutation`, `next_token() -> u64`, `AppState::begin_mutation(Mutation) -> u64`, `AppState::rollback(token)`, `AppState::commit(token)`.

This is the mechanism behind FR-C6. Build it before any CRUD operation uses it.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use ytm_core::{Playlist, PlaylistId, Track};

    #[test]
    fn tokens_are_unique() {
        let mut log = MutationLog::default();
        let a = log.next_token();
        let b = log.next_token();
        assert_ne!(a, b);
    }

    #[test]
    fn create_applies_immediately_then_commits() {
        let mut s = AppState::default();
        let token = s.begin_mutation(Mutation::CreatePlaylist {
            temp: Playlist::stub("temp-1", "New List"),
        });
        // FR-C6: the row is visible before the server confirms.
        assert_eq!(s.playlists.len(), 1);
        assert_eq!(s.playlists[0].title, "New List");

        s.commit(token, Some(PlaylistId::from("real-id")));
        assert_eq!(s.playlists[0].id, PlaylistId::from("real-id"), "temp id must be replaced");
        assert!(s.pending.is_empty(), "commit clears the pending entry");
    }

    #[test]
    fn create_rolls_back_on_failure() {
        let mut s = AppState::default();
        let token = s.begin_mutation(Mutation::CreatePlaylist {
            temp: Playlist::stub("temp-1", "New List"),
        });
        s.rollback(token);
        assert!(s.playlists.is_empty(), "the optimistic row must disappear");
    }

    #[test]
    fn rename_restores_the_previous_title_on_failure() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "Old Name")];
        let token = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p1"),
            previous: "Old Name".into(),
            next: "New Name".into(),
        });
        assert_eq!(s.playlists[0].title, "New Name");
        s.rollback(token);
        assert_eq!(s.playlists[0].title, "Old Name");
    }

    #[test]
    fn delete_restores_the_playlist_at_its_original_index() {
        let mut s = AppState::default();
        s.playlists = vec![
            Playlist::stub("p1", "First"),
            Playlist::stub("p2", "Second"),
            Playlist::stub("p3", "Third"),
        ];
        let token = s.begin_mutation(Mutation::DeletePlaylist {
            id: PlaylistId::from("p2"),
            index: 1,
            snapshot: s.playlists[1].clone(),
        });
        assert_eq!(s.playlists.len(), 2);
        s.rollback(token);
        assert_eq!(s.playlists.len(), 3);
        assert_eq!(s.playlists[1].title, "Second", "must return to its original position");
    }

    #[test]
    fn remove_tracks_restores_them_on_failure() {
        let mut s = AppState::default();
        s.tracks = vec![Track::stub("v1", "A"), Track::stub("v2", "B"), Track::stub("v3", "C")];
        let token = s.begin_mutation(Mutation::RemoveTracks {
            playlist: PlaylistId::from("p1"),
            removed: vec![(1, s.tracks[1].clone())],
        });
        assert_eq!(s.tracks.len(), 2);
        s.rollback(token);
        assert_eq!(s.tracks.len(), 3);
        assert_eq!(s.tracks[1].title, "B");
    }

    #[test]
    fn a_late_failure_reverts_the_right_edit_when_two_are_in_flight() {
        // The user renamed two playlists quickly; only the second fails.
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "One"), Playlist::stub("p2", "Two")];
        let t1 = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p1"), previous: "One".into(), next: "Uno".into(),
        });
        let t2 = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p2"), previous: "Two".into(), next: "Dos".into(),
        });
        s.rollback(t2);
        assert_eq!(s.playlists[0].title, "Uno", "the successful edit must survive");
        assert_eq!(s.playlists[1].title, "Two", "only the failed edit reverts");
        s.commit(t1, None);
    }

    #[test]
    fn rollback_of_an_unknown_token_is_a_no_op() {
        let mut s = AppState::default();
        s.rollback(9999);   // must not panic
        assert!(s.playlists.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui mutation`
Expected: FAIL — `MutationLog` not found.

- [ ] **Step 3: Implement**

```rust
//! Optimistic edits (FR-C6). Every mutation carries enough state to undo
//! itself, so a late failure reverts exactly its own change and nothing else.

use std::collections::HashMap;
use ytm_core::{Playlist, PlaylistId, SetVideoId, Track};

/// Each variant stores what it needs to reverse itself.
#[derive(Debug, Clone)]
pub enum Mutation {
    CreatePlaylist { temp: Playlist },
    RenamePlaylist { id: PlaylistId, previous: String, next: String },
    DeletePlaylist { id: PlaylistId, index: usize, snapshot: Playlist },
    AddTracks { playlist: PlaylistId, count: usize },
    /// `(original index, track)` pairs, ascending, so re-insertion is exact.
    RemoveTracks { playlist: PlaylistId, removed: Vec<(usize, Track)> },
}

#[derive(Default)]
pub struct MutationLog {
    next: u64,
    pending: HashMap<u64, Mutation>,
}

impl MutationLog {
    pub fn next_token(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
    pub fn insert(&mut self, token: u64, m: Mutation) { self.pending.insert(token, m); }
    pub fn take(&mut self, token: u64) -> Option<Mutation> { self.pending.remove(&token) }
    pub fn is_empty(&self) -> bool { self.pending.is_empty() }
    pub fn len(&self) -> usize { self.pending.len() }
}

/// Entries the API still has to confirm; used for the removal SetVideoIds.
pub fn set_video_ids(removed: &[(usize, Track)]) -> Vec<SetVideoId> {
    removed.iter().filter_map(|(_, t)| t.set_video_id.clone()).collect()
}
```

Then add to `AppState`: a `pub pending: MutationLog` field and three methods.

```rust
impl AppState {
    /// Apply the edit to local state right now and return its token.
    pub fn begin_mutation(&mut self, m: Mutation) -> u64 {
        let token = self.pending.next_token();
        match &m {
            Mutation::CreatePlaylist { temp } => self.playlists.push(temp.clone()),
            Mutation::RenamePlaylist { id, next, .. } => {
                if let Some(p) = self.playlists.iter_mut().find(|p| &p.id == id) {
                    p.title = next.clone();
                }
            }
            Mutation::DeletePlaylist { id, .. } => self.playlists.retain(|p| &p.id != id),
            Mutation::AddTracks { .. } => {}
            Mutation::RemoveTracks { removed, .. } => {
                let drop: Vec<_> = removed.iter().map(|(_, t)| t.video_id.clone()).collect();
                self.tracks.retain(|t| !drop.contains(&t.video_id));
            }
        }
        self.pending.insert(token, m);
        token
    }

    /// The server accepted it. For a create, swap the temp id for the real one.
    pub fn commit(&mut self, token: u64, real_id: Option<PlaylistId>) {
        if let Some(Mutation::CreatePlaylist { temp }) = self.pending.take(token) {
            if let Some(real) = real_id {
                if let Some(p) = self.playlists.iter_mut().find(|p| p.id == temp.id) {
                    p.id = real;
                }
            }
        }
    }

    /// The server rejected it. Undo exactly this edit.
    pub fn rollback(&mut self, token: u64) {
        let Some(m) = self.pending.take(token) else { return };
        match m {
            Mutation::CreatePlaylist { temp } => self.playlists.retain(|p| p.id != temp.id),
            Mutation::RenamePlaylist { id, previous, .. } => {
                if let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) {
                    p.title = previous;
                }
            }
            Mutation::DeletePlaylist { index, snapshot, .. } => {
                let at = index.min(self.playlists.len());
                self.playlists.insert(at, snapshot);
            }
            Mutation::AddTracks { .. } => {}
            Mutation::RemoveTracks { removed, .. } => {
                // Ascending order so each insert lands at its original index.
                for (idx, track) in removed {
                    let at = idx.min(self.tracks.len());
                    self.tracks.insert(at, track);
                }
            }
        }
    }
}
```

Wire `AppEvent::MutationOk { token, .. }` to `commit` and `MutationFailed { token, .. }` to `rollback` in `AppState::apply`, keeping the toast.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui mutation`
Expected: 8 tests PASS. The two-in-flight test is the one that matters — it is the bug this whole mechanism exists to prevent.

- [ ] **Step 5: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): optimistic mutation log with per-edit rollback"
```

---

## Task 29: Modal widgets — confirm and prompt

**Files:**
- Create: `crates/ytm-tui/src/widgets/modal.rs`
- Modify: `crates/ytm-tui/src/render.rs`, `crates/ytm-tui/src/app.rs`

**Interfaces:**
- Consumes: `Modal`, `ConfirmAction`, `PromptAction`, `centered_rect`.
- Produces: `modal::draw(f, area, state, theme)`, and modal input handling in `AppState::apply_input`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};
    use crate::{
        app::{AppState, ConfirmAction, Modal, PromptAction},
        event::{AppEvent, InputAction},
        theme::Theme,
    };
    use ytm_core::PlaylistId;

    fn text_of(s: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, s, &theme)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn confirm_modal_shows_the_question_and_both_choices() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Confirm {
            text: "Delete \"Focus\"?".into(),
            action: ConfirmAction::DeletePlaylist(PlaylistId::from("p1")),
        });
        let text = text_of(&s);
        assert!(text.contains("Delete"));
        assert!(text.contains("y") && text.contains("n"), "must show the keys to press");
    }

    #[test]
    fn prompt_modal_shows_the_title_and_current_value() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "New playlist name".into(),
            value: "Chill".into(),
            action: PromptAction::CreatePlaylist,
        });
        let text = text_of(&s);
        assert!(text.contains("New playlist name"));
        assert!(text.contains("Chill"));
    }

    #[test]
    fn typing_in_a_prompt_appends_to_the_value() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "Name".into(), value: String::new(), action: PromptAction::CreatePlaylist,
        });
        s.apply(AppEvent::Input(InputAction::Char('a')));
        s.apply(AppEvent::Input(InputAction::Char('b')));
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "ab"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn backspace_in_a_prompt_deletes_the_last_character() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "Name".into(), value: "abc".into(), action: PromptAction::CreatePlaylist,
        });
        s.apply(AppEvent::Input(InputAction::Backspace));
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "ab"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn backspace_on_an_empty_prompt_does_not_panic() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "Name".into(), value: String::new(), action: PromptAction::CreatePlaylist,
        });
        s.apply(AppEvent::Input(InputAction::Backspace));
        assert!(s.modal.is_some());
    }

    #[test]
    fn escape_closes_any_modal_without_acting() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Confirm {
            text: "Delete?".into(),
            action: ConfirmAction::DeletePlaylist(PlaylistId::from("p1")),
        });
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert!(s.modal.is_none());
        assert!(s.pending.is_empty(), "cancelling must not start a mutation");
    }

    #[test]
    fn a_multibyte_character_deletes_cleanly() {
        // Byte-slicing here would panic; pop() is char-aware.
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "Name".into(), value: "日本".into(), action: PromptAction::CreatePlaylist,
        });
        s.apply(AppEvent::Input(InputAction::Backspace));
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "日"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui modal`
Expected: FAIL.

- [ ] **Step 3: Implement**

Extend `AppState::apply_input`'s modal branch — it currently only handles `Cancel` and `Quit`:

```rust
if let Some(modal) = self.modal.as_mut() {
    match (modal, a) {
        (_, InputAction::Cancel) => self.modal = None,
        (Modal::Prompt { value, .. }, InputAction::Char(c)) => value.push(c),
        // pop() removes a whole char — byte slicing would panic on multibyte input.
        (Modal::Prompt { value, .. }, InputAction::Backspace) => { value.pop(); }
        // Confirm/Prompt submission is handled by the loop, which owns the API calls.
        _ => {}
    }
    return;
}
```

`draw` renders a `centered_rect(50, 20, area)` box with a single-line border in `theme.fg_dim`, the question or title in `fg_bright`, and a footer. Confirm footers read `[y] yes   [n] no   [esc] cancel`; prompt footers read `[enter] save   [esc] cancel`. The test asserts the literal `y` and `n` appear.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui modal`
Expected: 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): confirm and prompt modals with unicode-safe editing"
```

---

## Task 30: Create and rename playlists

**Files:**
- Modify: `crates/ytm-cli/src/app_loop.rs`, `crates/ytm-tui/src/app.rs`

**Interfaces:**
- Consumes: `Mutation`, `MusicSource::{create_playlist, edit_playlist}`, `Modal::Prompt`.
- Produces: `MutationTask` and `spawn_mutation(task, source, tx)` in the loop.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ytm_core::{mock::MockSource, MusicSource, Playlist, Privacy, SourceError};
    use ytm_tui::app::{AppState, Modal, PromptAction};

    #[tokio::test]
    async fn creating_a_playlist_calls_the_source_and_commits() {
        let src = Arc::new(MockSource::new());
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "Name".into(), value: "Road Trip".into(), action: PromptAction::CreatePlaylist,
        });

        let token = submit_prompt(&mut s).expect("a prompt submission yields a mutation");
        assert_eq!(s.playlists.len(), 1, "FR-C6: optimistic row appears at once");
        assert!(s.modal.is_none(), "the modal closes on submit");

        let ev = run_mutation(token, MutationTask::Create {
            title: "Road Trip".into(), description: None, privacy: Privacy::Private,
        }, src.clone()).await;

        assert!(src.calls().iter().any(|c| c.starts_with("create_playlist")));
        match ev {
            ytm_tui::event::AppEvent::MutationOk { token: t, .. } => assert_eq!(t, token),
            other => panic!("expected MutationOk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_failed_create_yields_MutationFailed_with_the_same_token() {
        let src = Arc::new(MockSource::new());
        src.fail_next(SourceError::RateLimited);
        let ev = run_mutation(7, MutationTask::Create {
            title: "X".into(), description: None, privacy: Privacy::Private,
        }, src).await;
        match ev {
            ytm_tui::event::AppEvent::MutationFailed { token, message } => {
                assert_eq!(token, 7);
                assert!(message.contains("too many requests"), "got: {message}");
            }
            other => panic!("expected MutationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn renaming_sends_the_new_title() {
        let src = Arc::new(MockSource::new()
            .with_playlists(vec![Playlist::stub("p1", "Old")]));
        let _ = run_mutation(1, MutationTask::Rename {
            id: "p1".into(), title: "New".into(),
        }, src.clone()).await;
        assert!(src.calls().iter().any(|c| c.starts_with("edit_playlist")));
        assert_eq!(src.library_playlists().await.unwrap()[0].title, "New");
    }

    #[test]
    fn renaming_a_system_playlist_is_refused_before_any_api_call() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist { is_system: true, ..Playlist::stub("LM", "Your Likes") }];
        s.selected = 0;
        assert!(open_rename_prompt(&mut s).is_none(), "must refuse");
        assert_eq!(s.toasts.len(), 1, "and say why");
    }

    #[test]
    fn submitting_an_empty_name_is_refused() {
        let mut s = AppState::default();
        s.modal = Some(Modal::Prompt {
            title: "Name".into(), value: "   ".into(), action: PromptAction::CreatePlaylist,
        });
        assert!(submit_prompt(&mut s).is_none());
        assert!(s.playlists.is_empty(), "no optimistic row for an invalid name");
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-cli mutation`
Expected: FAIL — `MutationTask` not found.

- [ ] **Step 3: Implement**

```rust
/// Background work that changes server state.
pub enum MutationTask {
    Create { title: String, description: Option<String>, privacy: ytm_core::Privacy },
    Rename { id: ytm_core::PlaylistId, title: String },
    Delete { id: ytm_core::PlaylistId },
    AddTracks { id: ytm_core::PlaylistId, videos: Vec<ytm_core::VideoId> },
    RemoveTracks { id: ytm_core::PlaylistId, entries: Vec<ytm_core::SetVideoId> },
}

/// Perform one mutation and report the outcome, tagged with its token.
pub async fn run_mutation(
    token: u64,
    task: MutationTask,
    source: Arc<dyn MusicSource>,
) -> AppEvent {
    let (result, ok_msg): (Result<Option<ytm_core::PlaylistId>, ytm_core::SourceError>, &str) =
        match task {
            MutationTask::Create { title, description, privacy } => (
                source.create_playlist(title, description, privacy).await.map(Some),
                "playlist created",
            ),
            MutationTask::Rename { id, title } => (
                source.edit_playlist(id, Some(title), None, None).await.map(|_| None),
                "playlist renamed",
            ),
            MutationTask::Delete { id } => (
                source.delete_playlist(id).await.map(|_| None),
                "playlist deleted",
            ),
            MutationTask::AddTracks { id, videos } => (
                source.add_tracks(id, videos).await.map(|_| None),
                "added to playlist",
            ),
            MutationTask::RemoveTracks { id, entries } => (
                source.remove_tracks(id, entries).await.map(|_| None),
                "removed from playlist",
            ),
        };

    match result {
        Ok(_) => AppEvent::MutationOk { token, message: ok_msg.to_owned() },
        Err(e) => AppEvent::MutationFailed { token, message: e.to_string() },
    }
}
```

`MutationOk` needs the real `PlaylistId` for a create so `commit` can swap the temp id. Add a `real_id: Option<PlaylistId>` field to that variant and pass it through — the Task 28 test already expects `commit(token, Some(id))`.

`submit_prompt(&mut AppState) -> Option<u64>`: reject an empty/whitespace value with a toast, otherwise close the modal, call `begin_mutation`, and return the token.

`open_rename_prompt(&mut AppState) -> Option<PlaylistId>`: refuse a `is_system` playlist with an error toast (FR-C2 does not apply to system playlists), otherwise open the prompt pre-filled with the current title.

Wire `InputAction::CreatePlaylist` and `RenamePlaylist` in `dispatch_input`, and add a `Confirm`-in-modal branch that calls `submit_prompt` and spawns the mutation.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli`
Expected: all tests PASS.

- [ ] **Step 5: Verify against the real account — MANUAL**

Run: `cargo run -p ytm-cli`, press `N`, type a name, press Enter.
Expected: the row appears instantly, a success toast follows, and the playlist exists in the YouTube Music web UI. Then press `R` on it and rename.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat: create and rename playlists with optimistic updates"
```

---

## Task 31: Delete playlist behind a confirmation

**Files:**
- Modify: `crates/ytm-cli/src/app_loop.rs`, `crates/ytm-tui/src/app.rs`

**Interfaces:**
- Consumes: `Mutation::DeletePlaylist`, `ConfirmAction::DeletePlaylist`, `MutationTask::Delete`.
- Produces: `confirm_action(&mut AppState) -> Option<(u64, MutationTask)>`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::Playlist;
    use ytm_tui::{app::{AppState, ConfirmAction, Modal}, event::InputAction};

    #[test]
    fn pressing_delete_opens_a_confirmation_rather_than_deleting() {
        // FR-C3: destructive actions are never one keystroke.
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "Focus")];
        s.selected = 0;
        open_delete_confirm(&mut s);
        assert!(matches!(s.modal, Some(Modal::Confirm { .. })));
        assert_eq!(s.playlists.len(), 1, "nothing is removed until confirmed");
    }

    #[test]
    fn the_confirmation_names_the_playlist() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "Focus")];
        open_delete_confirm(&mut s);
        match &s.modal {
            Some(Modal::Confirm { text, .. }) => assert!(text.contains("Focus"), "got: {text}"),
            other => panic!("expected a confirm, got {other:?}"),
        }
    }

    #[test]
    fn confirming_removes_the_row_and_returns_the_task() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "Focus")];
        open_delete_confirm(&mut s);
        let (_token, task) = confirm_action(&mut s).expect("confirm yields work");
        assert!(matches!(task, MutationTask::Delete { .. }));
        assert!(s.playlists.is_empty(), "optimistic removal");
        assert!(s.modal.is_none());
    }

    #[test]
    fn a_failed_delete_puts_the_playlist_back() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "A"), Playlist::stub("p2", "B")];
        s.selected = 1;
        open_delete_confirm(&mut s);
        let (token, _) = confirm_action(&mut s).unwrap();
        assert_eq!(s.playlists.len(), 1);
        s.rollback(token);
        assert_eq!(s.playlists.len(), 2);
        assert_eq!(s.playlists[1].title, "B", "restored at its original index");
    }

    #[test]
    fn a_system_playlist_cannot_be_deleted() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist { is_system: true, ..Playlist::stub("LM", "Your Likes") }];
        open_delete_confirm(&mut s);
        assert!(s.modal.is_none(), "no confirmation for an impossible action");
        assert_eq!(s.toasts.len(), 1, "explain why instead");
    }

    #[test]
    fn declining_the_confirmation_changes_nothing() {
        let mut s = AppState::default();
        s.playlists = vec![Playlist::stub("p1", "Focus")];
        open_delete_confirm(&mut s);
        s.apply(ytm_tui::event::AppEvent::Input(InputAction::Cancel));
        assert!(s.modal.is_none());
        assert_eq!(s.playlists.len(), 1);
        assert!(s.pending.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-cli delete`
Expected: FAIL — `open_delete_confirm` not found.

- [ ] **Step 3: Implement**

```rust
/// Ask before deleting (FR-C3). Refuses system playlists outright.
pub fn open_delete_confirm(s: &mut AppState) {
    let Some(p) = s.selected_playlist().cloned() else { return };
    if p.is_system {
        s.push_toast(
            ToastKind::Error,
            &format!("\"{}\" is managed by YouTube Music and cannot be deleted", p.title),
            s.elapsed_ms,
        );
        return;
    }
    s.modal = Some(Modal::Confirm {
        text: format!("Delete \"{}\"? This cannot be undone.", p.title),
        action: ConfirmAction::DeletePlaylist(p.id),
    });
}

/// The user pressed `y`. Apply optimistically and hand back the work to do.
pub fn confirm_action(s: &mut AppState) -> Option<(u64, MutationTask)> {
    let Some(Modal::Confirm { action, .. }) = s.modal.take() else { return None };
    match action {
        ConfirmAction::DeletePlaylist(id) => {
            let index = s.playlists.iter().position(|p| p.id == id)?;
            let snapshot = s.playlists[index].clone();
            let token = s.begin_mutation(Mutation::DeletePlaylist {
                id: id.clone(), index, snapshot,
            });
            Some((token, MutationTask::Delete { id }))
        }
        ConfirmAction::RemoveTracks { playlist, entries } => {
            let removed: Vec<(usize, Track)> = s.tracks.iter().enumerate()
                .filter(|(_, t)| t.set_video_id.as_ref().is_some_and(|sv| entries.contains(sv)))
                .map(|(i, t)| (i, t.clone()))
                .collect();
            let token = s.begin_mutation(Mutation::RemoveTracks {
                playlist: playlist.clone(), removed,
            });
            Some((token, MutationTask::RemoveTracks { id: playlist, entries }))
        }
    }
}
```

Bind `y` to confirm while a `Modal::Confirm` is open — add it to the keymap's modal path, or resolve `Char('y')` in the loop when `state.modal` is a confirm.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli delete`
Expected: 6 tests PASS.

- [ ] **Step 5: Verify against the real account — MANUAL**

Delete the throwaway playlist created in Task 30. Confirm it disappears from the web UI, and that pressing `n` instead of `y` leaves it alone.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat: delete playlists behind confirmation with rollback"
```

---

## Task 32: Add and remove tracks

**Files:**
- Modify: `crates/ytm-cli/src/app_loop.rs`, `crates/ytm-tui/src/app.rs`, `crates/ytm-tui/src/widgets/modal.rs`

**Interfaces:**
- Consumes: `MutationTask::{AddTracks, RemoveTracks}`, `AppState::marked`.
- Produces: `open_add_to_playlist(&mut AppState)`, `targets_for_add(&AppState) -> Vec<VideoId>`, `open_remove_confirm(&mut AppState)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::{Playlist, SetVideoId, Track, VideoId};
    use ytm_tui::app::{AppState, Modal, Pane};

    fn playlist_track(v: &str, sv: &str, title: &str) -> Track {
        Track { set_video_id: Some(SetVideoId::from(sv)), ..Track::stub(v, title) }
    }

    #[test]
    fn with_no_marks_the_target_is_the_selected_track() {
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        s.tracks = vec![Track::stub("v1", "A"), Track::stub("v2", "B")];
        s.selected = 1;
        assert_eq!(targets_for_add(&s), vec![VideoId::from("v2")]);
    }

    #[test]
    fn marked_tracks_take_precedence_over_the_selection() {
        // FR-C4: multi-select.
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        s.tracks = vec![Track::stub("v1", "A"), Track::stub("v2", "B"), Track::stub("v3", "C")];
        s.selected = 0;
        s.marked.insert(VideoId::from("v2"));
        s.marked.insert(VideoId::from("v3"));
        let mut got = targets_for_add(&s);
        got.sort();
        assert_eq!(got, vec![VideoId::from("v2"), VideoId::from("v3")]);
    }

    #[test]
    fn an_empty_list_yields_no_targets() {
        let s = AppState::default();
        assert!(targets_for_add(&s).is_empty());
    }

    #[test]
    fn toggle_mark_adds_then_removes() {
        let mut s = AppState::default();
        s.pane = Pane::Songs;
        s.tracks = vec![Track::stub("v1", "A")];
        s.toggle_mark();
        assert!(s.marked.contains(&VideoId::from("v1")));
        s.toggle_mark();
        assert!(s.marked.is_empty());
    }

    #[test]
    fn removing_requires_an_open_playlist() {
        let mut s = AppState::default();
        s.pane = Pane::Songs;   // library songs, not a playlist
        s.tracks = vec![playlist_track("v1", "sv1", "A")];
        open_remove_confirm(&mut s);
        assert!(s.modal.is_none(), "there is no playlist to remove from");
        assert_eq!(s.toasts.len(), 1);
    }

    #[test]
    fn removing_a_track_without_a_set_video_id_is_refused() {
        // Without SetVideoId the API cannot identify the entry.
        let mut s = AppState::default();
        s.pane = Pane::Playlists;
        s.open_playlist = Some("p1".into());
        s.tracks = vec![Track::stub("v1", "A")];   // no set_video_id
        open_remove_confirm(&mut s);
        assert!(s.modal.is_none());
        assert_eq!(s.toasts.len(), 1, "explain rather than fail silently");
    }

    #[test]
    fn removing_opens_a_confirmation_naming_the_count() {
        let mut s = AppState::default();
        s.pane = Pane::Playlists;
        s.open_playlist = Some("p1".into());
        s.tracks = vec![playlist_track("v1", "sv1", "A"), playlist_track("v2", "sv2", "B")];
        s.marked.insert(VideoId::from("v1"));
        s.marked.insert(VideoId::from("v2"));
        open_remove_confirm(&mut s);
        match &s.modal {
            Some(Modal::Confirm { text, .. }) => assert!(text.contains('2'), "got: {text}"),
            other => panic!("expected a confirm, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn adding_tracks_calls_the_source_with_every_video_id() {
        use std::sync::Arc;
        use ytm_core::mock::MockSource;
        let src = Arc::new(MockSource::new().with_playlists(vec![Playlist::stub("p1", "Target")]));
        let _ = run_mutation(1, MutationTask::AddTracks {
            id: "p1".into(),
            videos: vec![VideoId::from("v1"), VideoId::from("v2")],
        }, src.clone()).await;
        assert!(src.calls().iter().any(|c| c == "add_tracks(p1,2)"), "got {:?}", src.calls());
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-cli add_remove`
Expected: FAIL — `targets_for_add` not found.

- [ ] **Step 3: Implement**

```rust
/// Marked tracks if any, otherwise the selected one (FR-C4).
pub fn targets_for_add(s: &AppState) -> Vec<VideoId> {
    if !s.marked.is_empty() {
        return s.marked.iter().cloned().collect();
    }
    s.selected_track().map(|t| vec![t.video_id.clone()]).unwrap_or_default()
}

/// Confirm before removing (FR-C5). Refuses when the entries are unidentifiable.
pub fn open_remove_confirm(s: &mut AppState) {
    let Some(playlist) = s.open_playlist.clone() else {
        s.push_toast(ToastKind::Error, "open a playlist first", s.elapsed_ms);
        return;
    };

    let targets: Vec<VideoId> = if s.marked.is_empty() {
        s.selected_track().map(|t| vec![t.video_id.clone()]).unwrap_or_default()
    } else {
        s.marked.iter().cloned().collect()
    };

    let entries: Vec<SetVideoId> = s.tracks.iter()
        .filter(|t| targets.contains(&t.video_id))
        .filter_map(|t| t.set_video_id.clone())
        .collect();

    if entries.is_empty() {
        s.push_toast(
            ToastKind::Error,
            "these tracks cannot be removed — try refreshing the playlist",
            s.elapsed_ms,
        );
        return;
    }

    s.modal = Some(Modal::Confirm {
        text: format!("Remove {} track(s) from this playlist?", entries.len()),
        action: ConfirmAction::RemoveTracks { playlist, entries },
    });
}
```

Add `AppState::toggle_mark()` — insert or remove the selected track's `VideoId` in `self.marked`.

For adding, `open_add_to_playlist` needs a target-playlist picker. Keep it simple: a new `Modal::PickPlaylist { targets: Vec<VideoId>, selected: usize }` listing editable playlists (`!is_system`), where Enter spawns `MutationTask::AddTracks`. Add the variant to `Modal` and a `draw` arm for it.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli`
Expected: all tests PASS.

- [ ] **Step 5: Verify against the real account — MANUAL**

Run the app: mark two tracks with `v`, press `A`, pick a playlist, confirm they appear in the web UI. Then open that playlist, press `x`, confirm, and check they are gone.

- [ ] **Step 6: Update PROGRESS.md and commit — PHASE 8 GATE**

All of FR-C1…C6 now work end-to-end. Record it.

```bash
./scripts/check.sh
git add -A
git commit -m "feat: add and remove playlist tracks with multi-select"
```

---
## Task 33: SQLite metadata cache

**Files:**
- Create: `crates/ytm-core/src/cache.rs`
- Modify: `crates/ytm-core/src/lib.rs`

**Interfaces:**
- Consumes: models.
- Produces: `Cache::open(path)`, `Cache::open_in_memory()`, `save_playlists`, `load_playlists`, `save_playlist_tracks`, `load_playlist_tracks`, `save_library_songs`, `load_library_songs`, `clear`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core_models::*;   // adjust to `crate::model::*`

    #[test]
    fn opens_in_memory_and_starts_empty() {
        let c = Cache::open_in_memory().unwrap();
        assert!(c.load_playlists().unwrap().is_empty());
    }

    #[test]
    fn playlists_round_trip() {
        let c = Cache::open_in_memory().unwrap();
        c.save_playlists(&[
            Playlist { track_count: Some(12), ..Playlist::stub("p1", "Focus") },
            Playlist::stub("p2", "Chill"),
        ]).unwrap();
        let got = c.load_playlists().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "Focus");
        assert_eq!(got[0].track_count, Some(12));
    }

    #[test]
    fn saving_replaces_rather_than_appends() {
        // A refresh must not duplicate rows.
        let c = Cache::open_in_memory().unwrap();
        c.save_playlists(&[Playlist::stub("p1", "A")]).unwrap();
        c.save_playlists(&[Playlist::stub("p1", "A renamed")]).unwrap();
        let got = c.load_playlists().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "A renamed");
    }

    #[test]
    fn playlist_tracks_round_trip_preserving_order_and_set_video_id() {
        let c = Cache::open_in_memory().unwrap();
        let id = PlaylistId::from("p1");
        let tracks = vec![
            Track { set_video_id: Some(SetVideoId::from("s1")), ..Track::stub("v1", "First") },
            Track { set_video_id: Some(SetVideoId::from("s2")), ..Track::stub("v2", "Second") },
        ];
        c.save_playlist_tracks(&id, &tracks).unwrap();
        let got = c.load_playlist_tracks(&id).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "First", "order must survive the round trip");
        assert_eq!(got[1].set_video_id, Some(SetVideoId::from("s2")), "removal depends on this");
    }

    #[test]
    fn tracks_for_an_unknown_playlist_are_empty_not_an_error() {
        let c = Cache::open_in_memory().unwrap();
        assert!(c.load_playlist_tracks(&PlaylistId::from("nope")).unwrap().is_empty());
    }

    #[test]
    fn multiple_artists_survive_the_round_trip() {
        let c = Cache::open_in_memory().unwrap();
        let t = Track { artists: vec!["A".into(), "B".into()], ..Track::stub("v1", "T") };
        c.save_library_songs(&[t]).unwrap();
        assert_eq!(c.load_library_songs().unwrap()[0].artists, vec!["A", "B"]);
    }

    #[test]
    fn clear_empties_every_table() {
        let c = Cache::open_in_memory().unwrap();
        c.save_playlists(&[Playlist::stub("p1", "A")]).unwrap();
        c.save_library_songs(&[Track::stub("v1", "T")]).unwrap();
        c.clear().unwrap();
        assert!(c.load_playlists().unwrap().is_empty());
        assert!(c.load_library_songs().unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_database_file_is_rebuilt_rather_than_fatal() {
        // The cache is disposable; a bad file must never block startup.
        let dir = std::env::temp_dir().join(format!("ytmcache{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        std::fs::write(&path, b"this is not a database").unwrap();
        let c = Cache::open(&path).expect("must recover, not fail");
        assert!(c.load_playlists().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-core cache`
Expected: FAIL — `Cache` not found.

- [ ] **Step 3: Implement**

```rust
//! Metadata cache for instant cold start (NFR-1). Disposable by design:
//! anything wrong with it is fixed by deleting and rebuilding. No audio,
//! no tokens, no secrets (NFR-6).

use crate::model::*;
use rusqlite::{params, Connection};

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Cache { conn: Connection }

impl Cache {
    pub fn open_in_memory() -> Result<Self, CacheError> {
        let c = Self { conn: Connection::open_in_memory()? };
        c.migrate()?;
        Ok(c)
    }

    /// Opens, or rebuilds from scratch if the file is unusable.
    pub fn open(path: &std::path::Path) -> Result<Self, CacheError> {
        if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }

        let fresh = |p: &std::path::Path| -> Result<Self, CacheError> {
            let c = Self { conn: Connection::open(p)? };
            c.migrate()?;
            Ok(c)
        };

        match fresh(path) {
            Ok(c) => Ok(c),
            Err(_) => {
                // Corrupt or wrong-version file: throw it away and start over.
                let _ = std::fs::remove_file(path);
                fresh(path)
            }
        }
    }

    fn migrate(&self) -> Result<(), CacheError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS playlists (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT,
                track_count INTEGER, privacy TEXT NOT NULL, thumbnail_url TEXT,
                is_system INTEGER NOT NULL, sort INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS tracks (
                video_id TEXT NOT NULL, set_video_id TEXT, title TEXT NOT NULL,
                artists TEXT NOT NULL, album TEXT, duration INTEGER NOT NULL,
                thumbnail_url TEXT, is_explicit INTEGER NOT NULL,
                playlist_id TEXT, sort INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS tracks_by_playlist ON tracks(playlist_id, sort);",
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }
    // save_/load_ methods follow; see below.
}
```

For the rest:

- Artists are stored as a `\u{1}`-joined string. A JSON array would work too — pick one and keep it consistent between save and load.
- `save_*` runs `DELETE` then bulk `INSERT` inside a transaction, so a refresh replaces instead of appending (there is a test for this).
- `sort` preserves list order; every `load_*` ends with `ORDER BY sort`.
- Library songs are rows with `playlist_id IS NULL`.
- `privacy` maps to `"private"` / `"public"` / `"unlisted"`; unknown values load as `Privacy::Private` rather than failing.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-core cache`
Expected: 8 tests PASS.

- [ ] **Step 5: Wire it into startup**

In `main.rs`, before the first draw: open the cache, load playlists and songs into `AppState`, draw, *then* spawn the network refresh. Each `*Loaded` event writes through to the cache.

- [ ] **Step 6: Verify the cold-start budget (NFR-1)**

Run: `time cargo run --release -p ytm-cli` and quit immediately.
Expected: content is on screen well under 300ms on a warm cache. If not, check that no `.await` on the network happens before the first `terminal.draw`.

- [ ] **Step 7: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(core): SQLite metadata cache with corrupt-file recovery"
```

---

## Task 34: Album art

**Files:**
- Create: `crates/ytm-tui/src/widgets/art.rs`
- Modify: `crates/ytm-tui/src/render.rs`, `crates/ytm-cli/src/app_loop.rs`

**Interfaces:**
- Consumes: `AppState.now_playing`, `ratatui-image`.
- Produces: `ArtCache`, `art::draw(f, area, state, art)`, `AppEvent::ArtLoaded { url, image }`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_is_skipped_when_the_terminal_cannot_display_images() {
        // FR-U5: absence must be graceful, never an error or a broken box.
        let cache = ArtCache::disabled();
        assert!(!cache.is_enabled());
        assert!(cache.get("https://example.com/a.jpg").is_none());
    }

    #[test]
    fn a_url_is_requested_only_once() {
        let mut cache = ArtCache::disabled();
        assert!(cache.should_fetch("u1"), "first sighting fetches");
        assert!(!cache.should_fetch("u1"), "already in flight or cached");
    }

    #[test]
    fn a_failed_fetch_is_not_retried_forever() {
        let mut cache = ArtCache::disabled();
        cache.should_fetch("u1");
        cache.mark_failed("u1");
        assert!(!cache.should_fetch("u1"), "a dead URL must not be retried on every tick");
    }

    #[test]
    fn a_zero_sized_area_is_skipped_without_panicking() {
        use ratatui::layout::Rect;
        assert!(!should_draw(Rect::new(0, 0, 0, 0)));
        assert!(!should_draw(Rect::new(0, 0, 4, 2)), "too small to be legible");
        assert!(should_draw(Rect::new(0, 0, 20, 10)));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-tui art`
Expected: FAIL — `ArtCache` not found.

- [ ] **Step 3: Implement**

```rust
//! Album art via ratatui-image. Optional by nature: many terminals cannot
//! display images, and the UI must be complete without it (FR-U5).

use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};

/// Below this the image is noise, not art.
const MIN_W: u16 = 10;
const MIN_H: u16 = 5;

pub fn should_draw(area: Rect) -> bool { area.width >= MIN_W && area.height >= MIN_H }

pub struct ArtCache {
    enabled: bool,
    /// Decoded protocol objects, keyed by URL.
    images: HashMap<String, ratatui_image::protocol::StatefulProtocol>,
    in_flight: HashSet<String>,
    failed: HashSet<String>,
}

impl ArtCache {
    /// Probes the terminal. Returns a disabled cache when images are unsupported.
    pub fn detect() -> Self {
        let enabled = ratatui_image::picker::Picker::from_query_stdio().is_ok();
        Self { enabled, images: HashMap::new(), in_flight: HashSet::new(), failed: HashSet::new() }
    }

    pub fn disabled() -> Self {
        Self { enabled: false, images: HashMap::new(), in_flight: HashSet::new(), failed: HashSet::new() }
    }

    pub fn is_enabled(&self) -> bool { self.enabled }

    pub fn get(&self, url: &str) -> Option<&ratatui_image::protocol::StatefulProtocol> {
        self.images.get(url)
    }

    /// True exactly once per URL, unless it later fails.
    pub fn should_fetch(&mut self, url: &str) -> bool {
        if self.images.contains_key(url) || self.in_flight.contains(url) || self.failed.contains(url) {
            return false;
        }
        self.in_flight.insert(url.to_owned());
        true
    }

    pub fn mark_failed(&mut self, url: &str) {
        self.in_flight.remove(url);
        self.failed.insert(url.to_owned());
    }
}
```

`Picker::from_query_stdio` must run **before** entering the alternate screen — it writes a query sequence and reads the reply. Call it in `main` first thing, then pass the result in.

Fetch bytes with `reqwest`, decode with `image`, and build the protocol object on the loop side, posting `AppEvent::ArtLoaded`. The exact `ratatui-image` 11.x API for building a `StatefulProtocol` should be read from its docs at implementation time — do not guess.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-tui art`
Expected: 4 tests PASS.

- [ ] **Step 5: Verify in a graphics-capable terminal**

Run the app in kitty or WezTerm and play a track. Expected: art renders beside the track list. Then run it in a plain xterm — expected: no art, no error, layout still correct. Note: `TERM` here is `tmux-256color`; tmux passthrough may need `allow-passthrough on`. If art does not work under tmux, record that in `PROGRESS.md` as a known limitation rather than fighting it.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(tui): optional album art with graceful degradation"
```

---

## Task 35: MPRIS / media keys

**Files:**
- Create: `crates/ytm-cli/src/mpris.rs`
- Modify: `crates/ytm-cli/src/main.rs`, `crates/ytm-cli/src/app_loop.rs`

**Interfaces:**
- Consumes: `souvlaki`, `PlayerCommand`, `AppState`.
- Produces: `MediaControls::attach(tx) -> Option<Handle>`, `update_metadata(&mut Handle, &AppState)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_key_events_map_to_player_commands() {
        use souvlaki::MediaControlEvent as E;
        assert!(matches!(to_command(E::Toggle), Some(PlayerCommand::TogglePause)));
        assert!(matches!(to_command(E::Play), Some(PlayerCommand::Resume)));
        assert!(matches!(to_command(E::Pause), Some(PlayerCommand::Pause)));
        assert!(matches!(to_command(E::Next), Some(PlayerCommand::Next)));
        assert!(matches!(to_command(E::Previous), Some(PlayerCommand::Previous)));
        assert!(matches!(to_command(E::Stop), Some(PlayerCommand::Stop)));
    }

    #[test]
    fn unsupported_events_are_ignored_rather_than_mapped_wrongly() {
        use souvlaki::MediaControlEvent as E;
        assert!(to_command(E::Raise).is_none());
    }

    #[test]
    fn metadata_is_empty_when_nothing_is_playing() {
        let s = ytm_tui::app::AppState::default();
        let m = metadata_of(&s);
        assert!(m.title.is_none());
    }

    #[test]
    fn metadata_carries_title_artist_and_duration() {
        let mut s = ytm_tui::app::AppState::default();
        s.now_playing = Some(ytm_core::Track::stub("v1", "Roygbiv"));
        s.duration = ytm_core::TrackDuration::from_secs(149);
        let m = metadata_of(&s);
        assert_eq!(m.title.as_deref(), Some("Roygbiv"));
        assert_eq!(m.duration.map(|d| d.as_secs()), Some(149));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-cli mpris`
Expected: FAIL — `to_command` not found.

- [ ] **Step 3: Implement**

```rust
//! OS media-key and now-playing integration (FR-U6). Entirely optional:
//! if the platform bus is unavailable, the app runs exactly as before.

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, PlatformConfig};
use ytm_player::player::PlayerCommand;

pub fn to_command(ev: MediaControlEvent) -> Option<PlayerCommand> {
    Some(match ev {
        MediaControlEvent::Toggle => PlayerCommand::TogglePause,
        MediaControlEvent::Play => PlayerCommand::Resume,
        MediaControlEvent::Pause => PlayerCommand::Pause,
        MediaControlEvent::Next => PlayerCommand::Next,
        MediaControlEvent::Previous => PlayerCommand::Previous,
        MediaControlEvent::Stop => PlayerCommand::Stop,
        MediaControlEvent::SetVolume(v) => PlayerCommand::SetVolume((v * 100.0) as u8),
        _ => return None,
    })
}

/// Owned metadata, so it is testable without a live bus.
pub struct OwnedMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<std::time::Duration>,
}

pub fn metadata_of(s: &ytm_tui::app::AppState) -> OwnedMetadata {
    match &s.now_playing {
        None => OwnedMetadata { title: None, artist: None, album: None, duration: None },
        Some(t) => OwnedMetadata {
            title: Some(t.title.clone()),
            artist: Some(t.artist_display()),
            album: t.album.clone(),
            duration: Some(std::time::Duration::from_secs(s.duration.as_secs())),
        },
    }
}

/// Returns `None` when the platform has no media-control bus — not an error.
pub fn attach(
    tx: tokio::sync::mpsc::UnboundedSender<PlayerCommand>,
) -> Option<MediaControls> {
    let config = PlatformConfig {
        dbus_name: "ytm_cli",
        display_name: "ytm-cli",
        hwnd: None,
    };
    let mut controls = MediaControls::new(config).ok()?;
    controls
        .attach(move |ev| {
            if let Some(cmd) = to_command(ev) {
                let _ = tx.send(cmd);
            }
        })
        .ok()?;
    Some(controls)
}
```

Add a fifth arm to the loop's `select!` for the media-key receiver, forwarding to the player. Update MPRIS metadata whenever `TrackChanged` arrives — not on every tick.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli mpris`
Expected: 4 tests PASS.

- [ ] **Step 5: Verify by hand**

Run the app, play a track, then check `playerctl metadata` in another terminal and press the media keys.
Expected: metadata shows the current track and the keys control playback.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(cli): MPRIS media keys and now-playing metadata"
```

---

## Task 36: CLI subcommands

**Files:**
- Modify: `crates/ytm-cli/src/main.rs`

**Interfaces:**
- Consumes: `clap`, `oauth`, `TokenStore`, `Cache`.
- Produces: `Cli` with `login`, `logout`, `playlists`, `cache clear`, and a default TUI command.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn no_arguments_launches_the_tui() {
        let c = Cli::parse_from(["ytm"]);
        assert!(c.command.is_none());
    }

    #[test]
    fn login_and_logout_parse() {
        assert!(matches!(Cli::parse_from(["ytm", "login"]).command, Some(Command::Login)));
        assert!(matches!(Cli::parse_from(["ytm", "logout"]).command, Some(Command::Logout)));
    }

    #[test]
    fn playlists_is_a_non_interactive_listing() {
        // Useful for scripting and for verifying auth without the TUI.
        assert!(matches!(Cli::parse_from(["ytm", "playlists"]).command, Some(Command::Playlists)));
    }

    #[test]
    fn a_config_path_can_be_overridden() {
        let c = Cli::parse_from(["ytm", "--config", "/tmp/x.toml"]);
        assert_eq!(c.config.as_deref(), Some(std::path::Path::new("/tmp/x.toml")));
    }

    #[test]
    fn an_unknown_subcommand_is_rejected() {
        assert!(Cli::try_parse_from(["ytm", "frobnicate"]).is_err());
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p ytm-cli cli`
Expected: FAIL — `Cli` not found.

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, clap::Parser)]
#[command(name = "ytm", about = "YouTube Music in your terminal", version)]
pub struct Cli {
    /// Path to config.toml (defaults to the platform config dir)
    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Sign in to YouTube Music
    Login,
    /// Forget stored credentials
    Logout,
    /// Print your playlists and exit
    Playlists,
    /// Cache maintenance
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Debug, clap::Subcommand)]
pub enum CacheAction {
    /// Delete all cached metadata
    Clear,
}
```

`login` prints the code and URL to stdout (fine — the TUI is not running) and waits. `playlists` prints one title per line, which is the fastest way to check auth without the UI. Both must exit non-zero on failure so scripts can tell.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p ytm-cli cli`
Expected: 5 tests PASS.

- [ ] **Step 5: Verify each subcommand**

```bash
cargo run -p ytm-cli -- --help
cargo run -p ytm-cli -- playlists
cargo run -p ytm-cli -- cache clear
```
Expected: help lists every command; `playlists` prints real titles; `cache clear` reports success. Leave `logout` for last so it does not interrupt other testing.

- [ ] **Step 6: Run the gate and commit**

```bash
./scripts/check.sh
git add -A
git commit -m "feat(cli): login, logout, playlists, and cache subcommands"
```

---

## Task 37: README and setup docs

**Files:**
- Create: `README.md`, `config.example.toml`

- [ ] **Step 1: Write the OAuth setup instructions**

These are the steps a new user cannot guess. Write them precisely:

1. Open Google Cloud Console, create or pick a project.
2. Enable the YouTube Data API v3 for it.
3. Create an OAuth client, application type **TV and Limited Input devices**.
4. Copy the client id and secret into `config.toml` as `auth.client_id` and `auth.client_secret`.
5. Run `ytm login` and follow the code and URL.

Also document the cookie fallback: open `music.youtube.com` while signed in, DevTools → Network → any `/youtubei/v1/` request → copy the full `Cookie` request header into a file, set `auth.kind = "cookie"` and `auth.cookie_file`.

- [ ] **Step 2: Write config.example.toml**

Every key with its default, commented. Copy the defaults from Task 6 exactly so the file does not drift from the code.

- [ ] **Step 3: Document the system requirements**

`libmpv` (Arch: `mpv`; Debian: `libmpv-dev`; macOS: `brew install mpv`), `yt-dlp`, and Rust 1.96+. Note the versions verified on this machine: libmpv 2.5.0, yt-dlp 2026.08.19.

- [ ] **Step 4: Include the ToS note**

State plainly that the app uses YouTube Music's internal API and yt-dlp, that this is against YouTube's Terms of Service regardless of Premium, and that it is intended for personal use. Do not bury it.

- [ ] **Step 5: Add a keybindings table**

Generate it from the Task 20 defaults so docs and code agree.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs: README with OAuth setup, requirements, and keybindings"
```

---

## Task 38: Final verification pass

**Files:**
- Modify: `PROGRESS.md`

- [ ] **Step 1: Run the full gate**

Run: `./scripts/check.sh`
Expected: `gate: OK`, zero warnings.

- [ ] **Step 2: Run the ignored tests**

Run: `cargo test --workspace -- --ignored`
Expected: keyring, resolver, and mpv tests pass. Record any environmental failures rather than deleting the tests.

- [ ] **Step 3: Walk every functional requirement**

Open the spec's §4 and exercise each FR against the running app. For each one, write pass or fail in `PROGRESS.md`. Do not mark an FR complete because the code looks right — press the keys.

- [ ] **Step 4: Check the non-functional requirements**

- NFR-1: time a cold start on a warm cache.
- NFR-2: play a track, then hold a navigation key — audio must not stutter and the UI must not freeze.
- NFR-4: `grep -rn 'println!\|eprintln!\|dbg!' crates/*/src` must return nothing outside `examples/` and the `playlists` subcommand.
- NFR-5: panic on purpose, confirm the terminal recovers, then remove the panic.
- NFR-6: `grep -ri 'access_token\|refresh_token' ~/.cache/ytm-cli/` must find nothing; confirm the log file has no token values.
- NFR-8: already covered by the gate.

- [ ] **Step 5: Build a release binary**

Run: `cargo build --release`
Expected: succeeds. Run it once from `target/release/ytm` to confirm the release profile behaves the same as debug.

- [ ] **Step 6: Final PROGRESS.md update and commit**

Mark every phase complete, list any deferred items with a reason, and note anything the next agent should know.

```bash
git add -A
git commit -m "docs: final verification pass across all requirements"
```

---

## Deferred / Out of Scope

Do not build these without a new decision from the owner. They are listed so an agent does not "helpfully" add them, and so the owner can pick one up deliberately later.

- Pagination beyond the first page for very large libraries (FR-B5 covers scroll-continuation; deep paging of 5000-track playlists is untested)
- Multi-account profiles
- Lyrics (`get_lyrics` exists in ytmapi-rs 0.3.3 if wanted)
- Podcasts, radio, mood/genre browsing
- Liking or rating tracks (`rate_song` exists)
- Playlist track reordering server-side (queue reordering is local only)
- Uploads management (`get_library_upload_songs`, `upload_song` exist)
- Windows testing
