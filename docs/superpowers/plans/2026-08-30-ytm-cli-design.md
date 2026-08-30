# ytm-cli — Product Design & Specification

**Status:** Approved 2026-08-30
**Owner:** pranab
**Plan:** `docs/superpowers/plans/2026-08-30-ytm-cli-plan.md`

---

## 1. Product Summary

A terminal YouTube Music client in Rust. The user logs in with their own
Google account, browses their real library and playlists, plays audio, and
performs create/read/update/delete on their playlists — all inside a
keyboard-driven TUI that looks deliberate rather than utilitarian.

**One-sentence goal:** Play and manage my YouTube Music library without
leaving the terminal.

### Non-goals

Explicitly out of scope. Do not build these; do not "helpfully" add them.

- Video playback. Audio only.
- Downloading or offline audio storage. We cache *metadata*, never media.
- Podcasts, radio/mix auto-generation, YouTube (non-Music) browsing.
- Social features, sharing, collaborative playlists.
- A web UI, a daemon other users connect to, or a plugin system.
- Multi-account switching (single account per config profile; a second
  profile is a future change, not v1).

### Legal note — carry this forward

YouTube Music exposes no official API for library or streaming. This
project uses the internal `youtubei/v1` endpoints and `yt-dlp` stream
extraction. Both violate YouTube's Terms of Service regardless of a
Premium subscription. This is acceptable for a personal tool by explicit
owner decision. Consequences an agent must respect:

- Never add telemetry, analytics, or crash reporting that leaves the machine.
- Never commit real cookies, tokens, client secrets, or account identifiers.
- Keep the ToS note in `README.md` when that file is created.

---

## 2. Users & Core Flows

Single user persona: a developer who lives in a terminal, uses vim keys,
and already pays for YouTube Music.

**Flow A — first run.** App starts, finds no credentials, shows a login
pane with a URL and a device code. User authorizes in a browser. App polls,
stores the token in the OS keyring, fetches the library, and lands on the
library view.

**Flow B — daily use.** App starts, renders the cached library in under
300ms, refreshes in the background, user arrows to a track and presses
Enter. Audio plays. Now-playing bar shows title, artist, elapsed/total, and
a progress bar.

**Flow C — playlist editing.** User opens a playlist, selects tracks, adds
them to another playlist, removes some, renames the playlist, and creates a
new one. Each edit appears instantly and silently reconciles with the
server; a failure rolls the row back and shows a toast.

---

## 3. Architecture

### 3.1 Crate layout

A Cargo workspace. Four crates, dependency arrows point down only.

```
ytm-cli      binary. clap args, config load, wiring, panic/terminal restore
   |
ytm-tui      ratatui views, input handling, keymap, theme, app state
   |
   +-- ytm-player   Player trait, mpv backend, queue, resolver (yt-dlp)
   |
ytm-core     domain models, MusicSource trait, ytmapi-rs impl, auth, cache
```

Rules an agent must not violate:

- `ytm-core` knows nothing about audio, terminals, or ratatui.
- `ytm-player` knows nothing about ratatui.
- `ytm-tui` never calls `ytmapi_rs` or `libmpv2` directly — only the traits.
- No crate depends on a crate above it. No cycles.

### 3.2 The two seams that matter

Everything fragile is hidden behind a trait, because the fragile parts are
the parts Google can break without notice.

**`MusicSource`** (`ytm-core`) — every read and write against YouTube Music.
Implemented by `YtMusicSource` (wraps `ytmapi-rs`) and `MockSource` (used by
every test in `ytm-tui` and most in `ytm-player`).

**`Player`** (`ytm-player`) — transport control. Implemented by
`MpvPlayer` and `MockPlayer`.

Because the TUI only ever sees traits, the entire UI is testable with no
network and no audio device. This is the single most important structural
decision in the project.

### 3.3 Concurrency model — actor + single event loop

The UI thread never blocks. Not for network, not for audio, not for disk.

```
 ┌──────────────┐  PlayerCommand   ┌────────────────────┐
 │              │ ───────────────► │  Player actor      │
 │  Event loop  │                  │  (own OS thread,   │
 │  (tokio,     │ ◄─────────────── │   owns Mpv handle) │
 │   single      │   PlayerEvent   └────────────────────┘
 │   owner of   │
 │   AppState)  │  ◄── AppEvent ── tokio tasks (API calls)
 │              │  ◄── Crossterm events (EventStream)
 └──────────────┘  ◄── 250ms render tick
```

- One `tokio::select!` over four sources: terminal events, player events,
  app events (results of async work), and a render tick.
- `AppState` is owned by the loop. No `Arc<RwLock>`, no shared mutation.
  Simpler to reason about and impossible to deadlock.
- Every network call is `tokio::spawn`ed and posts an `AppEvent` back.
- The player actor owns the `Mpv` handle on its own thread because
  `libmpv2::Mpv::wait_event` blocks; it must never run on the runtime.

### 3.4 Playback pipeline

```
VideoId → StreamResolver (yt-dlp -f bestaudio -g) → direct URL
        → mpv loadfile → audio out
```

`yt-dlp` is invoked as a subprocess. Resolved URLs are cached in memory with
a 4-hour TTL (Google's URLs expire ~6h; 4h is the safety margin). A 403 on
playback means the URL went stale: drop it from cache, re-resolve once,
retry. This retry is required, not optional — it is the single most common
runtime failure.

### 3.5 Auth

Two token types, OAuth is primary.

**OAuth device flow (primary).** The user creates a Google Cloud OAuth
client of type *TV and Limited Input*, and puts the id/secret in config.
App calls `generate_oauth_code_and_url`, shows code + URL, polls
`generate_oauth_token`. Refresh via `YtMusic::refresh_token`. Tokens live
in the OS keyring under service `ytm-cli`, never on disk in plaintext.

**Browser cookie (fallback).** User pastes their `music.youtube.com`
cookie header into a file; `YtMusic::from_cookie_file` consumes it. Same
`MusicSource` trait, selected by config. Exists because OAuth clients
occasionally get rejected and this always works.

### 3.6 Cache

SQLite via `rusqlite` (bundled). Purpose: instant cold start, nothing more.

- Tables: `playlists`, `tracks`, `playlist_tracks`, `meta`.
- Read on startup to paint the first frame; refresh over the network right
  after and diff into the UI.
- The cache is disposable. Any schema change may drop and rebuild it.
- No audio, no tokens, no secrets in SQLite.

### 3.7 Optimistic writes

Playlist edits apply to `AppState` immediately, then fire the API call. On
failure, revert that specific change and toast the error. Each in-flight
mutation carries a token so a late failure reverts the right row even if the
user has since moved on.

---

## 4. Functional Requirements

Every requirement has an ID. Plan tasks reference these; `PROGRESS.md`
tracks them.

### Auth
- **FR-A1** OAuth device-code login with code + verification URL displayed.
- **FR-A2** Token persisted to OS keyring; reused silently on next start.
- **FR-A3** Automatic token refresh on expiry, transparent to the user.
- **FR-A4** `ytm logout` clears the keyring entry.
- **FR-A5** Browser-cookie auth selectable via config as a fallback.
- **FR-A6** Missing/invalid credentials produce an actionable message that
  names the config key and the setup doc — never a raw error dump.

### Browse
- **FR-B1** Library playlists list.
- **FR-B2** Library songs (liked/saved).
- **FR-B3** Library albums and artists.
- **FR-B4** Playlist detail: tracks with title, artist, album, duration.
- **FR-B5** Paginate/continue long lists on scroll.

### Search
- **FR-S1** Search songs, albums, artists, playlists.
- **FR-S2** Input debounced 280ms; a newer query cancels the older one.
- **FR-S3** Play a result directly, or add it to a playlist or the queue.

### Playback
- **FR-P1** Play / pause / toggle.
- **FR-P2** Next / previous.
- **FR-P3** Seek relative (±5s, ±30s) and absolute.
- **FR-P4** Volume up/down/mute, persisted across runs.
- **FR-P5** Shuffle on/off; repeat off/one/all.
- **FR-P6** Stream 403 triggers exactly one re-resolve and retry.
- **FR-P7** Progress and duration update at least 4×/second while playing.

### Queue
- **FR-Q1** View the queue with the current track marked.
- **FR-Q2** Append, play-next, remove, clear.
- **FR-Q3** Reorder by moving a selected entry up or down.

### Playlist CRUD
- **FR-C1** Create a playlist with title, description, privacy.
- **FR-C2** Rename / edit description / change privacy.
- **FR-C3** Delete, behind a confirmation modal.
- **FR-C4** Add tracks (single and multi-select).
- **FR-C5** Remove tracks, behind a confirmation modal.
- **FR-C6** All of the above are optimistic and roll back on failure.

### UX
- **FR-U1** Vim-style keymap by default, fully remappable via config.
- **FR-U2** `?` opens a help overlay listing active bindings.
- **FR-U3** Toasts for success/error, auto-dismissing after ~4s.
- **FR-U4** Spinner on every in-flight network operation.
- **FR-U5** Album art in supported terminals, gracefully absent elsewhere.
- **FR-U6** Media keys and OS now-playing integration via MPRIS.
- **FR-U7** Theme loaded from a TOML file; a built-in default ships.

---

## 5. Non-Functional Requirements

- **NFR-1** Cold start to first painted frame < 300ms, served from cache.
- **NFR-2** The UI thread never blocks on I/O. Hard rule, no exceptions.
- **NFR-3** Input-to-visible-response < 50ms for local actions.
- **NFR-4** No logging to stdout/stderr ever — it corrupts the frame.
  All `tracing` output goes to a rotating file.
- **NFR-5** Terminal is always restored, including on panic and on SIGTERM.
- **NFR-6** Secrets never touch disk unencrypted and never enter logs.
- **NFR-7** Search debounced ≥280ms; no unbounded request fan-out.
- **NFR-8** `cargo clippy --all-targets -- -D warnings` passes clean.
- **NFR-9** Every network error surfaces as a human sentence, not a Debug dump.
- **NFR-10** Runs on Linux and macOS. Windows is best-effort, untested.

---

## 6. Visual Design

The UI should look composed. Concrete rules, not vibes:

- **Layout.** Left sidebar (sources: Library, Playlists, Albums, Artists,
  Search, Queue) ~22 cols. Main pane fills the rest. Now-playing bar pinned
  to the bottom, 3 rows. Optional art panel right of main when supported.
- **Color.** One accent, three neutrals (dim / normal / bright), plus red
  and green reserved strictly for error and success. Never more.
- **Borders.** No heavy boxes. A single dim vertical rule between sidebar
  and main; whitespace does the rest of the separating.
- **Progress bar.** Eighth-block glyphs (`▏▎▍▌▋▊▉█`) for sub-cell
  resolution, not `=` or `#`.
- **Truncation.** Always via `unicode-width`, never `str::len` — CJK and
  emoji titles otherwise shred column alignment.
- **Selection.** Reversed background, not a `>` marker.
- **Density.** One row per track. No blank spacer rows in lists.

Text is the interface; treat alignment and truncation as correctness bugs.

---

## 7. Tech Stack (versions verified on crates.io 2026-08-30)

| Concern | Crate | Version | Note |
|---|---|---|---|
| YT Music API | `ytmapi-rs` | 0.3.3 | `simplified-queries` + `rustls` |
| Audio | `libmpv2` | 6.0.0 | needs system libmpv (2.5.0 present) |
| Stream extract | `youtube_dl` | 0.10.0 | wraps yt-dlp 2026.08.19 |
| TUI | `ratatui` | 0.30.2 | |
| Terminal | `crossterm` | 0.29.0 | `event-stream` feature |
| Runtime | `tokio` | 1.53.1 | multi-thread |
| HTTP | `reqwest` | 0.13.4 | rustls only, no OpenSSL |
| Album art | `ratatui-image` | 11.0.6 | kitty/sixel/halfblock |
| Text input | `tui-textarea` | 0.7.0 | |
| Spinner | `throbber-widgets-tui` | 0.11.1 | |
| CLI | `clap` | 4.6.6 | derive |
| Cache | `rusqlite` | 0.40.2 | `bundled` |
| Keyring | `keyring` | 4.2.0 | |
| MPRIS | `souvlaki` | 0.8.3 | |
| Fuzzy | `nucleo` | 0.5.0 | |
| Errors | `thiserror` 2.0.20 / `color-eyre` 0.6.5 | | lib / bin |
| Logging | `tracing` + `tracing-appender` 0.2.5 | | file only |
| Paths | `directories` | 6.0.0 | |
| Width | `unicode-width` | 0.2.2 | |

Toolchain: rustc 1.96.1. Edition 2024. Verified present on this machine:
libmpv 2.5.0, mpv 0.41.0, yt-dlp 2026.08.19, sqlite 3.53.4, git 2.55.0.

---

## 8. Verified API Reference

Read from crate sources, not memory. Use these exact signatures.

### ytmapi-rs 0.3.3 — auth
```rust
generate_oauth_code_and_url(client: &Client, client_id: impl Into<String>)
    -> Result<(OAuthDeviceCode, String)>
generate_oauth_token(client: &Client, code: OAuthDeviceCode,
    client_id: impl Into<String>, client_secret: impl Into<String>)
    -> Result<OAuthToken>
generate_browser_token<S: AsRef<str>>(client: &Client, cookie: S)
    -> Result<BrowserToken>

YtMusic::from_cookie_file<P: AsRef<Path>>(path: P) -> Result<Self>
YtMusic::from_cookie<S: AsRef<str>>(cookie: S)    -> Result<Self>
YtMusic::new_unauthenticated()                     -> Result<Self>
YtMusicBuilder::new_with_client(client).with_auth_token(token).build()
yt.refresh_token(&mut self) -> Result<OAuthToken>
```

### ytmapi-rs 0.3.3 — queries (`simplified-queries` feature)
```rust
get_library_playlists(&self) -> Result<Vec<LibraryPlaylist>>
get_library_songs(&self)     -> Result<<GetLibrarySongsQuery as Query<A>>::Output>
get_library_albums(&self)    -> Result<Vec<SearchResultAlbum>>
get_playlist_tracks<'a, T: Into<PlaylistID<'a>>>(&self, playlist_id: T)
    -> Result<Vec<PlaylistItem>>
get_playlist_details<'a, T: Into<PlaylistID<'a>>>(&self, playlist_id: T)
    -> Result<GetPlaylistDetails>
search_songs<'a, Q: Into<SearchQuery<'a, FilteredSearch<SongsFilter>>>>(
    &self, query: Q) -> Result<Vec<SearchResultSong>>
create_playlist(&self, query: CreatePlaylistQuery<'_, T>)
    -> Result<PlaylistID<'static>>
edit_playlist(&self, query: EditPlaylistQuery<'_>) -> Result<ApiOutcome>
delete_playlist<'a, T: Into<PlaylistID<'a>>>(&self, playlist_id: T) -> Result<()>
add_video_items_to_playlist<'a, T: Into<PlaylistID<'a>>>(&self, playlist_id: T,
    video_ids: impl IntoIterator<Item = VideoID<'a>>)
    -> Result<Vec<AddPlaylistItem>>
remove_playlist_items<'a, T: Into<PlaylistID<'a>>>(&self, playlist_id: T,
    video_items: impl IntoIterator<Item = SetVideoID<'a>>) -> Result<()>
get_song_tracking_url<'a, T: Into<VideoID<'a>>>(&self, video_id: T)
    -> Result<SongTrackingUrl<'static>>
```

Removal needs `SetVideoID`, which is per-playlist-entry and comes from
`get_playlist_tracks` — not the `VideoID`. Keep it on the track model or
removal cannot be implemented.

Enums available: `PrivacyStatus`, `ApiOutcome`, `LikeStatus`,
`LibraryStatus`, `Explicit`, `AlbumType`.

### libmpv2 6.0.0
```rust
Mpv::new() -> Result<Mpv>
Mpv::with_initializer<F: FnOnce(MpvInitializer) -> Result<()>>(f) -> Result<Mpv>
mpv.command(&self, name: &str, args: &[&str]) -> Result<()>
mpv.set_property<T>(&self, name: &str, data: T) -> Result<()>
mpv.get_property<T>(&self, name: &str) -> Result<T>
mpv.observe_property(&self, name: &str, format: Format, id: u64) -> Result<()>
mpv.wait_event(&self, timeout: f64) -> Option<Result<Event<'_>>>   // BLOCKS
```
`Event` variants: `Shutdown`, `StartFile`, `EndFile(EndFileReason)`,
`FileLoaded`, `Seek`, `PlaybackRestart`, `PropertyChange { name, change,
reply_userdata }`, `LogMessage {..}`, `AudioReconfig`, `QueueOverflow`,
`ClientMessage`, `GetPropertyReply`, `SetPropertyReply`, `CommandReply`,
`VideoReconfig`, `Deprecated`.

Set `vid=no` and `video=no` in the initializer — we are audio-only.

---

## 9. Testing Strategy

Strict TDD: a failing test precedes every implementation, in every crate.

- **`ytm-core`** — models, cache, and mapping are unit-tested. API parsing is
  tested against recorded JSON fixtures in `ytm-core/tests/fixtures/`,
  scrubbed of account identifiers. Never hit the live network in a test.
- **`ytm-player`** — queue logic (shuffle, repeat, advance) is pure and
  fully unit-tested. mpv backend is exercised by one `#[ignore]`d
  integration test run manually.
- **`ytm-tui`** — every view is rendered into a `TestBackend` buffer and
  asserted. Reducers are pure functions over `AppState`, tested directly.
- **Gate for every task:** `cargo test --workspace` and
  `cargo clippy --all-targets -- -D warnings` both clean before commit.

Fixture capture is a manual step, documented in the plan, run once by a
human against their own account.

---

## 10. Risks

| Risk | Mitigation |
|---|---|
| Google changes internal API | All access behind `MusicSource`; fixtures make the break loud and local |
| yt-dlp extraction breaks | Pinned invocation, isolated in `StreamResolver`, one seam to fix |
| Stream URLs expire mid-session | 4h TTL + mandatory re-resolve-once on 403 (FR-P6) |
| OAuth client rejected by Google | Cookie auth ships as a first-class fallback (FR-A5) |
| Rate limiting / soft ban | Debounce, cache aggressively, no polling loops |
| libmpv absent on user's machine | Detect at startup, exit with an install hint, not a panic |
| Terminal left broken by a panic | `color-eyre` hook + guard that restores on unwind (NFR-5) |
