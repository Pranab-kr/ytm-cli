# ytm-cli — Progress

**Read this first. Update it before you stop.** It is the handoff between agents.

Last updated: 2026-08-30 by the implementation agent
Current phase: **Phase 4 — done. Tasks 1-16 committed and green.**
Next action: **Task 17** — unicode-safe text helpers (start of Phase 5, the TUI).
Blocked on the owner: Gate 1 (see "Manual steps") needs OAuth credentials.
Tasks 17-18 are pure helpers with no API dependency and are safe to do first,
but do not build browse/library UI on an unproven API — see the note below.

---

## Where things stand

Tasks 1-16 are implemented, committed, and pass `./scripts/check.sh`. Phases 0,
1, 3, and 4 are complete. Phase 2's code is complete but its live gate is not
run.

**Gate 2 (real audio) is GREEN.** The owner confirmed hearing music twice on
2026-08-30: once through the raw `MpvHandle`, and again through the full player
actor. The actor run also proved the queue path — it played track 1, reported 31
progress events at ~4Hz, honoured `Next`, and moved to track 2.

**Gate 1 (live API) is still untested** — no OAuth client exists yet. The code
is written and the two examples are ready to run; see "Manual steps".

**On starting Phase 5 with Gate 1 red.** CLAUDE.md says do not write TUI code
before both gates are green, and that rule stands for anything that renders
account data. Tasks 17 (unicode truncation) and 18 (theme) are pure functions
with no `MusicSource` involvement, so they cannot be invalidated by an auth
failure. Everything from Task 19 (`AppState`) onward assumes the shapes
`YtMusicSource` returns, so a Gate 1 failure that forces the cookie fallback —
or reveals a different response shape — would mean reworking it. Prefer getting
Gate 1 green first.

All ignored tests pass on this machine (`cargo test --workspace -- --ignored`):
keyring round-trip against the live Secret Service, live yt-dlp resolution, and
mpv audio-only init.

---

## Read these before writing code

| Document | What it is |
|---|---|
| `docs/superpowers/plans/2026-08-30-ytm-cli-design.md` | The spec. Requirements (FR-*/NFR-*), architecture, verified API signatures. |
| `docs/superpowers/plans/2026-08-30-ytm-cli-plan.md` | 38 tasks with test code and commands. Work them in order. |
| `CLAUDE.md` | How to work in this repo. Rules that are not obvious from the code. |

The plan's §8 (in the spec) holds **verified** `ytmapi-rs` and `libmpv2`
signatures, read from crate sources on 2026-08-30. Use them instead of
guessing, and re-verify against the vendored source if a call does not compile.

---

## Phase status

| Phase | Tasks | Status | Delivers |
|---|---|---|---|
| 0 — Workspace | 1–2 | ✅ done | Cargo workspace builds, gate script, logging, terminal guard |
| 1 — Core models | 3–5 | ✅ done | Models, `MusicSource` trait, `MockSource` |
| 2 — Auth ⚠️ | 6–9 | 🟡 code done, gate untested | OAuth login, keyring, live library fetch |
| 3 — Audio ⚠️ | 10–12 | ✅ done, gate GREEN | yt-dlp resolver, mpv plays a real track |
| 4 — Player | 13–16 | ✅ done | Actor thread, queue, transport |
| 5 — TUI shell | 17–21 | ⬜ not started | Event loop, sidebar, now-playing bar |
| 6 — Browse | 22–25 | ⬜ not started | Lists, search with debounce |
| 7 — Queue UI | 26–27 | ⬜ not started | Queue view, toasts, help |
| 8 — CRUD | 28–32 | ⬜ not started | Playlist create/rename/delete, add/remove tracks |
| 9 — Polish | 33–38 | ⬜ not started | Cache, album art, MPRIS, CLI, README |

⚠️ = risk phase. See below.

Status marks: ⬜ not started · 🟡 in progress · ✅ done · ⛔ blocked

---

## The two gates that decide the project

Everything above these is ordinary application code. Everything depends on them.

**Gate 1 — Task 9, Step 7.** Real playlist titles from the owner's account
print to the terminal. Proves `ytmapi-rs` + auth works.

**Gate 2 — Task 12, Step 6.** Audio comes out of the speakers via
yt-dlp → mpv. Proves the playback path works.

Do not write TUI code before both are green. If either fails, the failure is
external (Google changed something, yt-dlp needs updating, OAuth client
rejected) — record the exact error here and try the documented fallback
before building anything on top.

### Gate results

| Gate | Status | Notes |
|---|---|---|
| 1 — live API | ⛔ **FAILED — external cause** | OAuth login itself succeeds (token stored, keyring round-trip OK, correct scope). But **InnerTube rejects the OAuth token**: `POST music.youtube.com/youtubei/v1/browse` returns `400 INVALID_ARGUMENT`. The same token succeeds on the official Data API v3, which returned the owner's 5 real playlists — so the token is valid and the failure is Google-side, not ours. Full diagnosis below. |
| 2 — real audio | ✅ GREEN 2026-08-30 | Owner confirmed audible music. yt-dlp resolved a stream, mpv played it, clock advanced 0→9s of 213s. PipeWire sink at 39%. |

---

## Decisions already made — do not re-litigate

Settled with the owner on 2026-08-30. Reopening these wastes a turn.

- **Auth:** OAuth device-code is primary, browser-cookie is a first-class
  fallback. Both behind `MusicSource`.
- **OAuth client:** owned by `pranabm406@gmail.com`; the authorizing/library
  account is `ashtami009@gmail.com`, added as a test user. Owner and test user
  differing is fine — test-user membership is what grants access in Testing mode.
  Scope granted: `https://www.googleapis.com/auth/youtube` (write access; the
  read-only scope cannot satisfy FR-C1..C6).
- **7-day token expiry is expected, not a bug.** While the consent screen is in
  "Testing" with user type "External", Google expires refresh tokens after 7
  days, so `login_spike` must be re-run about weekly. FR-A2 holds for 7 days at a
  stretch, not indefinitely. `SourceError::TokenRefreshFailed` and
  `OAuthError::TimedOut` are the errors to expect when it lapses — treat them as
  "re-authorize", not as a code defect. Clicking "Publish app" would lift the cap
  but likely triggers Google verification for the sensitive scope; the owner
  decided that is not worth it for a personal tool.
- **Playback:** mpv via `libmpv2` only. No rodio/symphonia fallback — Opus
  decode in Symphonia is the known weak spot and mpv is already installed.
- **Scope:** full app, all 9 phases. Not an MVP.
- **Testing:** strict TDD everywhere, including the TUI via `TestBackend`.
- **Language/edition:** Rust 1.96.1, edition 2024.

Out of scope is listed at the end of the plan. Do not add those.

---

## Environment — verified present on this machine

Checked 2026-08-30. No system dependencies need installing.

| Tool | Version |
|---|---|
| rustc / cargo | 1.96.1 |
| libmpv (pkg-config) | 2.5.0 |
| mpv | 0.41.0 |
| yt-dlp | 2026.08.19 |
| sqlite3 | 3.53.4 |
| git | 2.55.0 |
| TERM | `tmux-256color` |

Note on `TERM`: the session runs under tmux, which can block the kitty
graphics protocol used for album art (Task 34). If art does not render,
try `set -g allow-passthrough on` in tmux config; if it still fails, record
it here as a known limitation rather than fighting it.

---

## Requirement checklist

Fill in during Task 38. Mark a requirement done only after pressing the keys
in the running app — not because the code looks right.

**Auth:** A1 ⬜ A2 ⬜ A3 ⬜ A4 ⬜ A5 ⬜ A6 ⬜  (all pending Gate 1)
**Browse:** B1 ⬜ B2 ⬜ B3 ⬜ B4 ⬜ B5 ⬜
**Search:** S1 ⬜ S2 ⬜ S3 ⬜
**Playback:** P1 ⬜ P2 ⬜ P3 ⬜ P4 ⬜ P5 ⬜ P6 ⬜ P7 ⬜
  (code + actor spike done for all seven; P1/P7 observed working in the spike.
   Mark them only after pressing keys in the running app, per the rule above.)
**Queue:** Q1 ⬜ Q2 ⬜ Q3 ⬜
**CRUD:** C1 ⬜ C2 ⬜ C3 ⬜ C4 ⬜ C5 ⬜ C6 ⬜
**UX:** U1 ⬜ U2 ⬜ U3 ⬜ U4 ⬜ U5 ⬜ U6 ⬜ U7 ⬜
**Non-functional:** NFR-1 ⬜ 2 ⬜ 3 ⬜ 4 ⬜ 5 ⬜ 6 ⬜ 7 ⬜ 8 ⬜ 9 ⬜ 10 ⬜

---

## Manual steps only a human can do

These need the owner's Google account or their eyes. An agent that reaches
one should stop, note it here, and ask.

| Task | What is needed | Status |
|---|---|---|
| 8.5 | Create a Google Cloud OAuth client ("TVs and Limited Input devices"), add the `.../auth/youtube` scope under Data Access, add the music account as a test user, put id/secret in `~/.config/ytm-cli/config.toml`, run `cargo run -p ytm-core --example login_spike` | ✅ done 2026-08-30 — login works |
| 9.1 | Capture a real fixture: `… cargo run -p ytm-core --example dump_playlists -- --raw > /tmp/raw.json`, scrub account ids/emails/personal browseIds, replace `crates/ytm-core/tests/fixtures/library_playlists.json` (currently **SYNTHETIC**) | ⬜ |
| 9.7 | Gate 1: `cargo run -p ytm-core --example dump_playlists` prints real playlist titles | ⛔ **FAILS with 400** — needs the owner's auth decision, see Open question 3 |
| 9.7b | If choosing the cookie path: export `music.youtube.com` cookies from a logged-in browser to a Netscape-format `cookies.txt`, set `auth.kind = "cookie"` and `auth.cookie_file` in config.toml | ⬜ **BLOCKING Gate 1** |
| 12.6 | Confirm audio is audible | ✅ done 2026-08-30 |
| 30.5 / 31.5 / 32.5 | Verify playlist edits appear in the YouTube Music web UI | ⬜ |
| 34.5 | Check album art in a graphics-capable terminal | ⬜ |
| 35.5 | Check media keys and `playerctl metadata` | ⬜ |

---

## Open questions

**1. `set_video_id` is missing from `ytmapi-rs` 0.3.3's playlist reads — FR-C5 cannot work as specified.**

Verified by reading the vendored source, not inferred. `PlaylistSong`
(`src/parse/playlist.rs:74`) has no `setVideoId` field; the only place it exists
upstream is `AddPlaylistItem` (`:56`), the *result of adding* a track.
`remove_playlist_items` requires `SetVideoID`. So a track read from a playlist
currently cannot be removed from it.

`mapping::track_from_playlist_item` therefore sets `set_video_id: None`, and
`Track::is_removable()` returns false for playlist tracks. The mapping test
covers the field's behaviour, so the contract is pinned either way.

Options for the owner:
- Parse `setVideoId` ourselves from `raw_json_query` — the field IS in the wire
  JSON, upstream just drops it during parsing. Most work, no forking.
- Patch or fork `ytmapi-rs` to expose it.
- Drop FR-C5 (remove-track-from-playlist).

Not blocking until **Task 32**. Decide before then.

**3. Gate 1 fails: InnerTube rejects OAuth tokens. The owner must pick an auth path.**

Diagnosed 2026-08-30 by isolating each layer with raw `curl`, one variable at a
time. Established facts, not guesses:

- OAuth device-code login **works**. Token stored, keyring round-trip true.
- The token is **valid**: `oauth2.googleapis.com/tokeninfo` reports the right
  scope (`.../auth/youtube`) and the right `aud` (our client id), unexpired.
- The token **works on the official API**: `GET
  www.googleapis.com/youtube/v3/playlists?mine=true` returned **HTTP 200 and the
  owner's 5 real playlists**.
- The token **fails on InnerTube**: `POST
  music.youtube.com/youtubei/v1/browse` with `browseId=FEmusic_liked_playlists`
  returns `400 INVALID_ARGUMENT`, in Google's API-gateway error envelope rather
  than InnerTube's own — i.e. rejected before reaching the music backend.
- Ruled out by direct test, all still 400: with and without the `key=` param;
  `clientVersion` current (`1.20260830.01.00`) and older (`1.20240826.01.00`);
  Firefox and Cobalt user agents; `X-Goog-Request-Time` present and absent;
  `hl`/`gl` added. `ANDROID_MUSIC` and `IOS_MUSIC` give a *different* error
  ("Precondition check failed"), and `TVHTML5` returns 200 but only a
  `tvBrowseRenderer` navigation shell — zero playlist ids, no `musicShelfRenderer`.
  So no client-name substitution rescues it.

**This is not a bug in this codebase and not a bad OAuth client.** It is the same
breakage the Python `ytmusicapi` project hit: Google stopped honouring
device-flow OAuth tokens on the InnerTube endpoints. `ytmapi-rs` 0.3.3's OAuth
path cannot reach the library, whatever we pass it.

Options for the owner (nothing further can be verified without this decision):

- **A — browser-cookie auth (recommended).** Already a settled first-class
  fallback (FR-A5) and already implemented:
  `YtMusicSource::from_cookie_file` compiles today. Full YouTube Music API
  access, so every FR stays reachable. Cost: the owner exports cookies from a
  logged-in browser, and re-exports when they eventually expire.
- **B — official Data API v3.** Proven working with this exact token. But it is
  not the YouTube Music API: no `FEmusic_*` library browsing, no music-specific
  album/artist shapes, and a 10k units/day quota. Would mean rewriting
  `ytm-core`'s source layer and abandoning `ytmapi-rs`. Large spec deviation.
- **C — patch/fork `ytmapi-rs`.** Only worth it if upstream has a fix; the
  evidence says the block is Google-side, so a fork likely cannot help either.

The OAuth code stays as written either way — it is correct, it is tested, and it
will work again if Google restores the path. `AuthKind::Cookie` already exists in
config and `MusicSource` already abstracts both.

**2. The test fixture is SYNTHETIC.** `crates/ytm-core/tests/fixtures/library_playlists.json`
is hand-written to the documented shape. It validates JSON parsing only, not the
real response shape. Replace it via manual step 9.1.

---

## Log

Newest last. One entry per work session — what you finished, what broke,
what the next agent should know. Keep entries short.

### 2026-08-30 — planning agent
Wrote the design spec (405 lines) and the 38-task implementation plan
(~7000 lines). Verified every crate version against crates.io and read the
`ytmapi-rs` 0.3.3 and `libmpv2` 6.0.0 sources directly for real signatures —
spec §8 records them. Confirmed the toolchain and all system deps are present.
No code written yet. A git repo exists on branch `main` with zero commits, so
Task 1's commit is the initial one.

### 2026-08-30 — implementation agent (Tasks 1-12)

Implemented Tasks 1-12 with TDD throughout: failing test, watched it fail,
minimal implementation, watched it pass, gate, commit. Twelve commits, each one
green on `./scripts/check.sh`.

**Gate 2 (real audio) passed.** The owner confirmed hearing music. This plus the
live yt-dlp test in Task 10 means the playback path is proven end to end.

**Five plan/spec bugs found and fixed.** Every one verified against vendored
crate sources rather than guessed. The spec's §7 version table is correct; it is
the *feature names* that were wrong.

1. `keyring` 4.2.0 has no `apple-native`/`linux-native` features. Real names end
   in `-keyring-store`, and an API-surface feature (`v1` or `cli`) is also
   mandatory or the crate `compile_error!`s. Now plain `keyring = "4.2.0"`, whose
   default `v1` pulls in the zbus secret-service store. Chose secret-service over
   `linux-keyutils` deliberately: keyutils is memory-backed, so a token there
   would not survive a reboot, breaking FR-A2. Verified working against the live
   gnome-keyring (`auth -- --ignored` passes).
2. `reqwest` 0.13.4's TLS feature is `rustls`, not `rustls-tls`.
3. `youtube_dl` 0.10.0's is `downloader-rustls-tls`, not `rustls-tls`.
4. **`tui-textarea` 0.7.0 removed from the workspace.** It requires ratatui
   ^0.29, which pins `unicode-width =0.2.0` — unresolvable against the spec's
   ratatui 0.30.2 + unicode-width 0.2.2. Downgrading ratatui instead would break
   the `ratatui-image` 11.0.6 and `throbber-widgets-tui` 0.11.1 pins. The plan
   never actually used it: Task 29's prompt modal edits a plain `String` with
   `push`/`pop`. This is a real deviation from spec §7 — flagging rather than
   burying it.
5. `ratatui-image`'s default `chafa-dyn` feature needs system libchafa, which is
   not installed. Disabled that one feature; kept `image-defaults` + `crossterm`.
   The spec asks for kitty/sixel/halfblock, none of which need chafa. Revisit at
   Task 34 if art does not render.

**Two plan test bugs fixed.**
- Task 7's redaction test asserted `!d.contains("at")` against a Debug string
  containing the field name `expires_at` — it could never pass, no matter how
  correct the redaction. Now uses distinctive secret values and also asserts the
  `<redacted>` markers appear, which is what it was trying to check.
- Task 6's config test used `r#"…"#` around a body containing `accent = "#7aa2f7"`,
  whose `"#` closed the raw string early. Now `r##"…"##`.

**Two implementation notes the next agent needs.**
- `YtMusicSource` is **not** one generic `impl<A: LoggedIn>`. Upstream's
  `AuthToken::headers` returns an opaque `impl IntoIterator` with no `Send`
  bound, so `Send` is unprovable through a generic `A` and the `BoxFut` cast
  fails. I proved in isolation that a *concrete* token type does leak `Send`, so
  the impl body is written once in a macro and invoked for `BrowserToken` and
  `OAuthToken`. Do not "simplify" this back into a generic impl.
- `oauth::oauth_token_from_stored` is an addition beyond the plan, needed by
  FR-A2: after a restart only `StoredToken` survives, but the API handle needs an
  `OAuthToken`, whose fields are private. Goes through JSON (it derives
  `Deserialize`). Also switched off the deprecated `from_oauth_token` to
  `from_auth_token`.
- Other verified-source details: `OAuthDeviceCode` has no `Display`, only
  `get_code()`; `generate_oauth_code_and_url` discards the `user_code` and
  `interval` the login pane needs, so `begin_device_login` uses
  `OAuthTokenGenerator::new` directly. Durations arrive as display strings
  (`"2:29"`), so `mapping::parse_duration` exists with its own test. `edit_playlist`
  returns `ApiOutcome`, which reports `Failure` instead of an HTTP error —
  `check_outcome` converts that to a `SourceError` so a rejected edit cannot look
  like success.

One process slip, corrected: Task 3 briefly committed with a rustfmt diff
because I chained `./scripts/check.sh && git commit`, which masked the gate's
exit code. Caught it, formatted, amended. Every commit in the log is now
gate-clean. Do not chain the gate with `&&`.

Left undone and why: Task 9 steps 1 and 7 need the owner's Google account. The
`login_spike` and `dump_playlists` examples are written, compile, and are ready
to run — see "Manual steps" for exact commands.

### 2026-08-30 — implementation agent (Tasks 13-16, Phase 4)

**Phase 4 complete.** Queue logic, the player actor, and `MockPlayer`. Four more
commits, gate green on each.

- **Task 13** — `Queue`: 15 tests, all passing, including the index-tracking
  cases around `remove` and `move_item` that the plan flagged as most likely to
  break. Verified `rand` 0.9's API (`rand::rng()`, `rand::seq::SliceRandom`) in
  the vendored source before using it. One fix to the plan's code: its `remove`
  kept the saved unshuffled order in sync via a `position()` predicate that
  scanned for a track missing from `items` — wrong when a track appears twice.
  Now it removes by the id actually removed.
- **Task 14** — the actor. Restructured the plan's ten-argument helper functions
  into an `Actor` struct so the borrow checker and the reader both cope. Verified
  end-to-end: 31 progress events at ~4Hz, `Next` honoured, second track started,
  owner heard audio.
- **Task 15** — `MockPlayer`, 3 tests. Gated behind `mock` like `MockSource`.
- **Task 16** — this entry, plus `cargo test --workspace -- --ignored` green.

**One real plan bug, worth reading before touching the actor.** The plan detects
the FR-P6 stale-URL 403 via `Event::LogMessage`. That branch could never fire:
mpv only emits log messages after `mpv_request_log_messages`, and **libmpv2 6.0.0
exposes no wrapper for it** — `src/mpv.rs:359` mentions it as "(unimplemented)".
So the 403 arrives as `EndFile` with the ERROR reason instead, and that is where
the retry now lives. If a future libmpv2 adds the wrapper, `LogMessage` becomes
the more precise signal and `is_stale_url_error` is already written for it.

**Second verified-source trap in the same area.** `EndFileReason` is **not** a
Rust enum — libmpv2 re-exports it as `libmpv2_sys::mpv_end_file_reason`, a
`c_uint` alias with integer constants (EOF=0, STOP=2, QUIT=3, ERROR=4,
REDIRECT=5). `map_end_reason` therefore matches by value against the `libmpv2-sys`
constants, and has its own test so a constant changing upstream fails loudly.
Unknown values map to `Error`, never `Eof`: guessing "clean finish" would silently
skip a track. This added `libmpv2-sys = "4.0.1"` to `ytm-player` — a transitive
dep of libmpv2 already, now direct, and the only way to name those constants.

Note for Task 26 (queue UI): `PlayerCommand::PlayNow` currently inserts after the
current track and steps onto it, so the queue grows rather than being replaced.
That matches "replacing whatever is playing" for audio purposes but leaves
history in the queue. If the queue view should show something else, that is a UI
decision, not an actor bug.

### 2026-08-30 — implementation agent (Tasks 17-18, Gate 1 diagnosis)

Tasks 17 (unicode text helpers, 7 tests) and 18 (theme, 5 tests) done and
committed — both pure functions with no `MusicSource` dependency, so a Gate 1
failure cannot invalidate them. Held at Task 19 (`AppState`) deliberately.

Also moved the spike examples off env vars onto `~/.config/ytm-cli/config.toml`
(mode 600, outside the repo) so the client secret never crosses a shell command,
a transcript, or a process listing. Verified no credential values are tracked in
git — the only matches are key *names* in source and docs.

**Gate 1 failed, and the cause is external.** See Open question 3 for the full
evidence. Short version: OAuth login works and the token is provably good (it
returns the owner's 5 real playlists from the official Data API v3), but
InnerTube answers `400 INVALID_ARGUMENT` for every variant tried. Google no
longer honours device-flow OAuth on those endpoints — the same wall the Python
`ytmusicapi` project hit.

Method note for whoever picks this up: I isolated it with raw `curl` against the
live endpoint, varying one field per request, after dumping the access token to a
mode-600 temp file. That file and the throwaway `dump_token.rs` example were
shredded and deleted afterwards — do not leave either lying around if you repeat
the exercise.

Do not "fix" the OAuth code. It is correct. The next step is the owner's auth
decision, and if that is the cookie path, `YtMusicSource::from_cookie_file`
already exists and needs only a `cookies.txt`.
