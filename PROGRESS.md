# ytm-cli — Progress

**Read this first. Update it before you stop.** It is the handoff between agents.

Last updated: 2026-08-30 by the implementation agent
Current phase: **Phase 5 complete. Tasks 1-21 committed and green.**
Next action: **Task 22** — the event loop. Start of Phase 6.
Blocked on the owner: nothing. **Both gates are GREEN.**

Cookie auth is the live auth path. The cookie file expires (see "Cookie
expiry" below) — when the library reads empty, re-export it before assuming a
code bug.

---

## Where things stand

Tasks 1-21 are implemented, committed, and pass `./scripts/check.sh`. Phases 0-5
are complete. Phase 6 has not started.

The TUI now renders: `render()` draws the sidebar, a dim vertical rule, a main
pane heading, and the now-playing bar, and the keymap turns key presses into
`InputAction`s. Nothing wires it to a terminal yet — that is Task 22.

**Gate 2 (real audio) is GREEN.** The owner confirmed hearing music twice on
2026-08-30: once through the raw `MpvHandle`, and again through the full player
actor. The actor run also proved the queue path — it played track 1, reported 31
progress events at ~4Hz, honoured `Next`, and moved to track 2.

**Gate 1 (live API) is GREEN** as of 2026-08-30, via browser-cookie auth. The
owner exported cookies and `dump_playlists` printed 10 real playlist titles with
track counts (Eng_songs 87, Funk 90, bengali 61, AFTER EFFECTS TRANSITIONS 400).
Verified twice by two different runs. That exercises the full path: cookie auth
-> InnerTube -> `ytmapi-rs` -> `mapping::playlist_from_library` -> our `Playlist`.
The OAuth path stays broken for external reasons (Open question 3) and is left in
place unchanged.

**Phase 5 was cleared to proceed and is now done.** The CLAUDE.md rule (no TUI code before
both gates are green) is satisfied: Gate 1 green via cookie auth, Gate 2 green
via real audio. Tasks 17-18 were done while Gate 1 was red because they are pure
functions; Task 19 onward was deliberately held until the gate passed, and is now
unblocked.

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
| 2 — Auth ⚠️ | 6–9 | ✅ done, gate GREEN (cookie) | OAuth login, keyring, live library fetch |
| 3 — Audio ⚠️ | 10–12 | ✅ done, gate GREEN | yt-dlp resolver, mpv plays a real track |
| 4 — Player | 13–16 | ✅ done | Actor thread, queue, transport |
| 5 — TUI shell | 17–21 | ✅ done | Keymap, theme, text helpers, sidebar, now-playing bar |
| 6 — Browse | 22–25 | ⬜ not started | Event loop, lists, search with debounce |
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

Both are green as of 2026-08-30, so TUI work is cleared. The rule still applies
to any *future* regression: if a gate goes red, the cause is usually external
(Google changed something, yt-dlp needs updating, cookies expired) — record the
exact error here and try the documented fallback before building on top.

### Gate results

| Gate | Status | Notes |
|---|---|---|
| 1 — live API | ✅ **GREEN 2026-08-30 (cookie auth)** | `dump_playlists` printed 10 real playlist titles + track counts via `auth: browser cookie`. Proves cookie auth -> InnerTube -> `ytmapi-rs` -> `mapping` -> `Playlist`. The **OAuth** path remains broken for external reasons: InnerTube returns `400 INVALID_ARGUMENT` for a token that the official Data API v3 accepts. Diagnosis kept in Open question 3 for the day Google restores it. |
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

**Auth:** A1 ⬜ A2 ⬜ A3 ⬜ A4 ⬜ A5 ⬜ A6 ⬜
  (A5 cookie-auth path proven working by Gate 1; still mark only from the app.)
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
| 9.7 | Gate 1: `cargo run -p ytm-core --example dump_playlists` prints real playlist titles | ✅ **done 2026-08-30** — 10 real titles via cookie auth |
| 9.7b | Cookie auth: save the **raw `Cookie:` header value** from a logged-in `music.youtube.com` request into a file (NOT Netscape cookies.txt — `BrowserToken::from_str` uses the contents verbatim as the header and requires `SAPISID=` in it), then set `auth.kind = "cookie"` and `auth.cookie_file` in config.toml | ✅ done 2026-08-30 — **repeat whenever cookies expire**, see "Cookie expiry" |
| 12.6 | Confirm audio is audible | ✅ done 2026-08-30 |
| 30.5 / 31.5 / 32.5 | Verify playlist edits appear in the YouTube Music web UI | ⬜ |
| 34.5 | Check album art in a graphics-capable terminal | ⬜ |
| 35.5 | Check media keys and `playerctl metadata` | ⬜ |

---

## Cookie expiry — read this before debugging an empty library

Cookie auth is the live path, and the cookie file **expires**. Observed on
2026-08-30: a freshly exported file worked at 23:05 and was dead by 23:15.

**The failure mode is misleading.** An expired cookie does not raise an auth
error. InnerTube returns **HTTP 200** with a signed-out page, so
`ytmapi-rs` parses it faithfully and the library reads as **0 playlists** —
indistinguishable from an empty account unless you look at the raw response.
`dump_playlists` prints "auth may have succeeded with an empty library" for
exactly this case.

**One-command check** — does Google still recognise the session:

```bash
curl -s "https://music.youtube.com/" \
  -H "Cookie: $(tr -d '\n' < ~/.config/ytm-cli/cookies.txt)" \
  -H "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36" \
  | grep -o '"LOGGED_IN":[a-z]*' | head -1
```

`"LOGGED_IN":true` = session good, look elsewhere for the bug.
`"LOGGED_IN":false` = **re-export the cookie**, the code is fine.

Ruled out on 2026-08-30 as *not* the cause of expiry, so don't re-test these:
a bad paste (no `Cookie:` prefix, no devtools `...` truncation, no quotes or
newlines, all 30 pairs present at plausible lengths); our request construction
(a correctly computed `SAPISIDHASH` got the same signed-out shell, and the plain
GET above involves no hashing at all); and a stale `__Secure-*SIDTS` / `SIDCC`
poisoning a good session (retested with those dropped, and with core identity
cookies only — all `LOGGED_IN=false`).

Re-export tip: export from a browser session you then leave logged in. Closing
the window or signing out elsewhere can invalidate the copied cookies.

**Task 22 note (event loop):** the empty-vs-expired ambiguity deserves surfacing
in the UI rather than looking like an empty library. There is no auth error to
catch, so the signal has to be "library call succeeded but returned zero rows"
-> hint the user to re-export. FR-A6 territory; decide when Task 22 lands.

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

**3. RESOLVED 2026-08-30 — cookie auth chosen (option A) and working. InnerTube rejects OAuth tokens.**

The owner exported cookies, Gate 1 went green, and `auth.kind = "cookie"` is the
live configuration. The evidence below is kept because the OAuth code is still in
the tree and correct — if Google restores device-flow tokens on InnerTube, it
should start working with no changes. Do not "fix" it in the meantime.

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
  `YtMusicSource::from_cookie_file` compiles today. Note the file format: it must
  be the raw `Cookie:` header string (one line, `SAPISID=...; HSID=...; ...`),
  because `BrowserToken::from_str` passes the contents straight through as the
  header and greps it for `SAPISID=`. A Netscape `cookies.txt` export will fail
  with an opaque `Error parsing header.` Full YouTube Music API
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

### 2026-08-30 — implementation agent (Gate 1 green, Task 19)

**Gate 1 passed.** The owner exported browser cookies and `dump_playlists`
printed 10 real playlist titles with track counts. Both gates are now green, so
the CLAUDE.md hold on TUI work is lifted and Phase 5 is properly underway.

**Cookies expire fast, and the failure looks like an empty library.** The
owner's first export worked at 23:05 and was dead by 23:15. The symptom is `0
playlists` with no error, because InnerTube answers HTTP 200 with a signed-out
page ("Sign in to listen to your liked tracks") and `ytmapi-rs` parses that
faithfully. I confirmed it with a plain `GET music.youtube.com` returning
`"LOGGED_IN":false`, which depends on nothing we build. Ruled out a bad paste,
our own request construction (correct `SAPISIDHASH` got the same shell), and
stale `SIDTS`/`SIDCC` cookies. Wrote it up under "Cookie expiry" with a
one-command check — read that before debugging an empty library again. A second
export was still live at the end of the session.

Left a note there for **Task 22**: there is no auth error to catch, so if the UI
should distinguish "expired" from "empty", the signal has to be a successful
library call returning zero rows.

**Task 19 done** — `AppState`, `AppEvent`, `InputAction`, and the pure reducers.
9 tests, gate green, committed. Three deviations from the plan's text, all in the
tests rather than the implementation:

- The plan's `selection_moves_and_clamps_at_both_ends` set `s.tracks` while the
  pane was the default `Playlists` with `open_playlist: None`. In that state
  `list_len()` counts `playlists`, so it was 0 and `select_next()` could never
  move — the test could not pass as written. `list_len`'s mapping is what Tasks
  23-24 build on, so I set `pane = Pane::Songs` instead and left a comment.
- Four tests used `AppState::default()` followed by field assignment, which
  clippy rejects under `-D warnings` (`field_reassign_with_default`). Rewritten
  as struct initializers with `..Default::default()`; assertions unchanged.
- `event.rs` needed `cargo fmt` — the plan's compact struct-variant style is not
  what rustfmt produces. Worth knowing for Tasks 20+, which paste similar code.

Reminder that bit again: run `./scripts/check.sh` as its own command, never
chained with `&&` before a commit, or a fmt-only failure gets masked.

### 2026-08-30 — implementation agent (Tasks 20-21, Phase 5 complete)

**Phase 5 done.** Keymap and the render entry point. Two commits, gate green on
each. `cargo test -p ytm-tui` is 37 tests.

- **Task 20** — `KeyMap`, 9 tests. Focus-sensitive by design: in
  `Focus::SearchInput` every `KeyCode::Char` becomes `InputAction::Char`, so 'j'
  types a letter instead of scrolling. Ctrl-C is checked before that branch, so
  quit works while typing. `from_toml_str` makes a binding exclusive — it drops
  any existing char bound to the same action before inserting the new one, or
  `down = "e"` would leave both 'j' and 'e' scrolling down.
- **Task 21** — `progress_bar`, the now-playing bar, the sidebar, and
  `render::render`. 7 tests, including the 8x4 narrow-terminal one.

**One plan test bug, and it matters for Tasks 23-27 which reuse the pattern.**
Task 21's `progress_bar_uses_partial_blocks_for_sub_cell_precision` asserted
`('\u{258F}'..='\u{2588}').contains(&c)`. That range is **inverted** — 258F is
greater than 2588, so it is empty and contains nothing; the test could not pass
against any output. Rewrote it as `EIGHTHS[..7].contains(&c)`, which is what it
meant and is strictly stronger: a bar of solid full blocks now fails it, whereas
a correctly-ordered range would have accepted `█` as a "partial" block.

**One deliberate deviation from the plan's implementation code.** Its row-2
layout computed the bar width with `times.len() + tail.len() + flags.len()`.
`flags` holds `⇄`, `①`, `↻` — three bytes each, one column each — so byte length
over-counted the chrome by up to 6 and shortened the bar. Now uses
`util::text::display_width`, per the CLAUDE.md columns-not-bytes rule. Worth
watching for in Tasks 23-26: the plan uses `.len()` on display strings in a few
more places.

**What renders today.** Sidebar (22 cols, labels `Playlists`/`Songs`/`Albums`/
`Artists`/`Search`/`Queue`, reversed-background selection), a dim `│` rule, a
main pane showing only its heading, and the 3-row now-playing bar. The list
widgets that fill the main pane are Tasks 23-26; `render::draw_main` is the seam
they plug into. Nothing has touched a real terminal yet — `render` is only ever
called from `TestBackend` so far. Task 22 wires it up.

Two notes carried forward for **Task 22**, both already recorded above: surface
the expired-cookie-vs-empty-library ambiguity (a successful library call
returning zero rows is the only signal), and every widget must keep guarding on
a zero-sized `Rect` — the 8x4 test is the regression net for that.
