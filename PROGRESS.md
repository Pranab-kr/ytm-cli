# ytm-cli — Progress

**Read this first. Update it before you stop.** It is the handoff between agents.

Last updated: 2026-08-31 by the implementation agent
Current phase: **Phase 9 underway. Tasks 1-36 committed.**
Next action: **Task 37** — README and setup docs. First, the owner should re-test
**delete** and **add-to-playlist** (see "Live verification"), and check album art
in kitty **outside tmux** (see "Album art under tmux").

**Every browse pane works against the live account, and the queue is editable.**
10 real playlists with track counts, Enter opens one and renders its tracks, `/`
searches YouTube Music with one request per typing pause, and `u` shows the
queue with the playing entry marked — reorder, remove, and clear all verified
against real audio. `?` lists every binding. Playlist create, rename, and
delete are wired with optimistic updates and per-edit rollback. 230 tests pass.

**Both owner decisions from the last session are resolved.**
1. **FR-C5 works.** Option A chosen and implemented: `playlist_raw` reads
   `setVideoId` from the wire JSON. Live check: 83 tracks, 83 removable.
2. **Live verification started, and it found three real bugs** that every
   `MockSource` test had passed. Create and rename are confirmed working against
   the account. Delete and add-to-playlist are fixed but not yet retested.

**Tasks 33-36 are done, and running the real app found two bugs that the whole
suite passed.** Both were caught by measuring and driving, not by inspection:
1. **NFR-1 was violated by 8x.** Cold start to first frame was **2480 ms**
   against a 300 ms budget, because `build_source` awaits a cookie-validation
   round trip *before* the terminal opens. Reordered: **62 ms**.
2. **Album art broke playback entirely** — my own Task 34 regression. Probing
   stdio under tmux leaves crossterm's `EventStream` delivering no key presses,
   so Enter did nothing and no track could ever play. Fixed; see "Album art
   under tmux". 297 tests pass.

**MPRIS is live-verified.** `playerctl -p ytm_cli` reports `Playing`, title, and
artist; `play-pause` paused and resumed real audio; `next` was received.

**Both gates are GREEN.**

Cookie auth is the live auth path. The cookie file expires (see "Cookie
expiry" below) — when the library reads empty, re-export it before assuming a
code bug.

---

## Where things stand

Tasks 1-33 are implemented, committed, and pass `./scripts/check.sh`. Phases 0-7
are complete. Phase 8's code is complete and partly live-verified — delete and
add-to-playlist still need a retest (see "Live verification"). Phase 9 has
started: Task 33 (the cache) is done.

The whole vertical slice is now connected: config -> cookie auth -> `MusicSource`
-> `tokio::spawn` -> `AppEvent` -> `AppState` -> `render` -> a real terminal, plus
the player actor on its own thread. Verified by running it, not by inspection
(see the Task 22 log entry). Every pane now renders, and queue edits go through
the actor rather than mutating the view.

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
| 6 — Browse | 22–25 | ✅ done | Event loop, lists, search with debounce |
| 7 — Queue UI | 26–27 | ✅ done | Queue view, toasts, spinner, help overlay |
| 8 — CRUD | 28–32 | 🟡 create+rename live-verified | Playlist create/rename/delete, add/remove tracks. FR-C5 now works (option A). Delete + add need a retest |
| 9 — Polish | 33–38 | 🟡 Tasks 33-36 done | Cache ✅, album art ✅, MPRIS ✅, CLI ✅, README next |

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
| 33.6 | Cold-start budget (NFR-1) | ✅ done 2026-08-31 — 62ms to first frame on a warm cache, measured from the log |
| 34.5 | Check album art in a graphics-capable terminal — **kitty outside tmux**; halfblocks already render inside tmux | ⬜ owner offered to test this |
| 35.5 | Check media keys and `playerctl metadata` | ✅ done 2026-08-31 — `playerctl` reports Playing + title + artist; play-pause and next both work |
| 36.5 | Verify each subcommand | ✅ done 2026-08-31 — `--help`, `playlists` (11 real titles), `cache clear`; bad input exits 2 |

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

## Album art under tmux — and why it once broke playback

Art works inside tmux, as **halfblocks**. It does not use the kitty protocol
there, and trying to was actively harmful.

**`Picker::from_query_stdio` is destructive under tmux.** It writes a query
escape sequence and reads the reply. With `allow-passthrough` off, tmux prints
the sequence as visible text instead of forwarding it, no reply ever arrives,
and — the part that cost the time — **crossterm's `EventStream` stops delivering
key presses afterwards**. The app then looked completely broken in a way that
pointed nowhere near album art: Enter did nothing, no `yt-dlp` process ever
spawned, and MPRIS sat at `Stopped`.

Diagnosed by A/B against the same script, not by reading code:

| `ui.album_art` | Result |
|---|---|
| `false` | status `Playing`, PulseAudio sink live, title `FREAKED OUT` |
| `true` | status `Stopped`, no sink, no yt-dlp, ever |

The `Ptmux;_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\` was sitting in the captured screen
output the whole time, next to "Nothing playing".

`ArtCache::detect` now checks `TMUX`/`TERM` first and uses `Picker::halfblocks()`
there, which needs no probe and no protocol support. **Do not "restore" the
stdio probe under tmux to get kitty graphics** — it trades all playback for a
nicer image.

Two things for the owner's kitty-outside-tmux test (manual step 34.5):
- Outside tmux the probe runs, so kitty/sixel should be detected and the image
  should be sharper than halfblocks.
- The log line `album art enabled protocol=…` names what was detected. Inside
  tmux it instead reads `tmux detected, using halfblocks without probing stdio`.

Also note `from_fontsize` is deprecated in ratatui-image 11.0.6 in favour of
`halfblocks()`; the latter is what we call, and it sets `is_tmux` itself.

---

## Live verification

Create and rename are confirmed against the owner's real account. Two steps are
left, both fixed in code but not yet re-tested by a human:

```
cargo run -p ytm-cli
```

- `D` on `zz-throwaway`, then `n` — must leave it alone. Then `D`, `y` — it
  should disappear from the app and from the web UI.
- `v` on two tracks, `A`, pick a playlist, Enter — both should land.

`zz-throwaway` still exists on the account for exactly this.

**Three bugs came out of the first attempt, and every one of them passed the
whole test suite.** Worth reading before trusting a green gate on UI or API work:

1. **Modal keys resolved against the wrong focus.** `keymap.resolve` was passed
   `state.focus`, which stays Sidebar/Main while a modal is open, and `Char(c)`
   is only produced under `Focus::SearchInput`. So the create prompt could not be
   typed into — `q` quit the app instead. Confirms were worse: `y` matched
   nothing and `n` skipped the track behind the box. Fixed with
   `AppState::input_focus()`. **The Task 29 tests missed it because they fed
   `InputAction::Char` straight into `apply()`, skipping the keymap** — they
   proved the reducer worked while nothing could reach it. New tests go through
   `resolve`.
2. **The prompt had no cursor**, so an empty field rendered as blank space and
   read as a dead box. The value line now ends in a cursor block.
3. **The wrong playlist id form went to every mutation endpoint** — see below.

### The playlist id has two forms, and the endpoints disagree

This one cost the most time and is the most likely to bite again.

| Form | Looks like | Used by |
|---|---|---|
| browse | `VLPLa9OPirWkaJM` | `browse` endpoints; what a library listing reports |
| playlist | `PLa9OPirWkaJM` | `playlist/edit`, `playlist/delete`, add/remove items; what `create_playlist` returns |

We stored the browse form and sent it everywhere, so **every mutation on an
existing playlist got `400 INVALID_ARGUMENT`**. Create was immune because it
takes no existing id. `ytmapi-rs` 0.3.3 forwards whatever it is handed — its
source carries four `TODO: Confirm if processing required to add/remove 'VL'`.

`PlaylistId::browse_form()` / `mutation_form()` convert, both idempotent because
ids genuinely arrive in both forms. **If a new endpoint is added, pick the form
deliberately** — the default of passing `id.as_str()` is wrong half the time.

The same investigation found `is_system_playlist` never matching: it was handed
`VLLM`, not `LM`, so no system playlist was flagged read-only and the
rename/delete guards never fired. It now strips the prefix.

Verified live and reversibly with `examples/verify_edit`: renamed a real
playlist, confirmed the title in the library, restored the original.

---

## Open questions

**1. RESOLVED 2026-08-31 — `setVideoId` is parsed from the raw JSON (option A). FR-C5 works.**

The owner chose option A. `ytm-core/src/playlist_raw.rs` extracts the field from
the wire response; `playlist_tracks` runs upstream's typed parse and our own pass
over **one** request, because `ProcessedResult`'s fields are public and
`parse_into` accepts JSON we already hold. Live check: 83 tracks, 83 removable.

Three things were measured against the live account rather than assumed, and each
one changed the code — do not "simplify" any of them back:
- **Ids are read per row.** An 83-track playlist carried 85 `setVideoId`
  occurrences, so a document-wide scan misaligns.
- **The menu index is not fixed.** The id sat at `items/6`, but nothing
  guarantees it, so each row's menu is searched.
- **Pairing is by `videoId` with a forward cursor, never by position.** The shelf
  returned 85 rows where upstream parsed 83 — it drops rows internally — so a
  positional zip produced *zero* removable tracks on the first attempt. The
  cursor is what lets a video appearing twice get its two distinct entry ids.

The original diagnosis, kept for context:

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

### 2026-08-31 — implementation agent (Task 22, the app runs)

**Task 22 done.** `crates/ytm-cli/src/app_loop.rs` plus a real `main.rs`. 11
tests. Gate green, committed.

**The app runs, verified by running it — not by inspection.** `q` exits cleanly
with the terminal restored, and the log shows
`background task finished event="playlists" rows=10 ms=282`: the owner's real 10
playlists, fetched through cookie auth, off the UI thread, delivered as an
`AppEvent`. The sidebar, the `│` rule, "Nothing playing", and `70%` all render.
The main pane shows only its heading until Tasks 23-26.

**How to run it headless, since this is not obvious and cost me two attempts.**
The app needs a TTY *with a window size*. Piping stdin into `script` leaves the
pty at 0x0, `f.area()` is empty, every widget hits its zero-size guard, and you
get a blank alternate screen and a clean exit — looking exactly like a broken
render. Set the size explicitly:

```bash
( sleep 14; printf 'q' ) | YTM_LOG=debug timeout 30 \
  script -q -c "stty rows 40 cols 120; ./target/debug/ytm-cli" /dev/null
```

Then read `~/.cache/ytm-cli/logs/ytm-cli.log.<date>` — note the **date suffix**,
so `logs/*.log` matches nothing.

**Step 7 (terminal survives a panic) passed, and needed a fix the plan does not
mention.** `TerminalGuard::Drop` alone is not enough: the panic hook prints
*before* unwinding runs `Drop`, so the report would land in the alternate screen
and vanish with it. `main` now installs a hook that restores the terminal and
then chains to the previous one. Verified by byte offsets in the captured pty
output — leave-altscreen at 1564, "panicked" at 1599 — so the message provably
printed after the restore. `restore_terminal()` is idempotent and runs twice
(hook, then `Drop`); that is harmless and intentional. The probe key was
temporary and is out of the tree (`grep "STEP-7 temporary"` is clean).

**Additions beyond the plan's code, each with a reason.**
- `empty_library_hint` — the FR-A6 item PROGRESS.md left for this task. An
  expired cookie is *not* an auth error, so the only signal is a successful
  library call returning zero rows; it raises an Info toast naming both
  possibilities. Cookie-auth only: telling an OAuth user to re-export cookies
  they do not have is noise. 3 tests, including that OAuth is excluded.
- `spawn_task` logs each task's name, row count, and elapsed ms. Nothing about a
  background fetch is visible on screen, so without this there is no way to tell
  a working fetch from a broken one until Task 23 lands. This is what proved the
  gate above.
- `dispatch_input` returns early when a modal is open, so transport keys cannot
  fire behind a confirmation dialog. `AppState` already guards navigation this
  way; the plan's dispatch matched transport *before* consulting the modal.
- `state.loading = true` is set in `start()` at the point a `Task` is returned,
  per the plan's Step 5 note, so the spinner covers the whole round trip.
- Volume/mute/shuffle update `AppState` locally as well as sending the command,
  so the bar moves on the next frame instead of waiting for the actor's echo.
- `send()` logs a dropped command instead of unwrapping. An `unwrap` here would
  panic inside the loop and take the terminal down over a dead actor thread.

**Two notes for Task 23 onward.**
- `Task::Search` is `#[allow(dead_code)]` until Task 25 constructs it.
  `spawn_task` already handles it, so Task 25 only has to emit it. Remove the
  attribute then.
- `render::draw_main` is the single seam the list widgets plug into — it
  currently paints just the pane heading. `dispatch_input`'s `A::Confirm` arm
  already opens a playlist (`Task::OpenPlaylist`) and plays a track, so Task 23
  should find Enter working the moment rows are on screen.

### 2026-08-31 — implementation agent (Task 23, tracks on screen)

**Task 23 done.** `crates/ytm-tui/src/widgets/tracklist.rs` plus the `draw_main`
seam in `render.rs`. 7 tests. Gate green, committed.

**Verified against the real library, not just `TestBackend`.** Pressing Enter on
a playlist logged `event="playlist_tracks" rows=83 ms=1060` and the rows rendered
with titles, artists, and durations. Two things worth seeing in that output,
because they are the whole point of the column rules:

```
Dernière danse                    Indila               3:34
Love Potions (6arelyhuman Remix) (feat. princess pa…  BJ Lips    3:09
```

Accented and Greek characters survive intact, and the over-long title truncates
with `…` at a column boundary rather than mid-character. That is
`pad_to_width`/`truncate_to_width` doing their job — a `str::len()` version would
have shredded the grid here.

**`draw_main` now splits its area.** One row for the heading, the rest for the
list. Track-shaped panes (`Songs`, `Search`, `Queue`, and `Playlists` *once a
playlist is open*) route to `tracklist::draw`; everything else falls through to
the heading alone rather than drawing a list of the wrong shape. Task 24 fills in
the `_ => {}` arm for playlists/albums/artists.

**Two plan deviations, both small.**
- Dropped the plan's trailing `let _ = truncate_to_width;` and simply did not
  import what this widget does not use. The line existed to silence an unused
  import; not importing it is the same fix without the noise.
- Extracted the plan's inline `7`/`2`/`6` into `DURATION_WIDTH`, `MARK_WIDTH`,
  and `TITLE_SHARE`. The plan also subtracted an extra `+ 2` of gap that its own
  column math never spent, which left two columns unused at the right edge; the
  spans now account for exactly the width they occupy.

**Process note for whoever edits `render.rs` next.** I broke it briefly by
running two scripted edits where the first one's assertion failed (rustfmt had
already reformatted the text I was matching) while the second still applied,
stripping an import the first was meant to replace. It compiled again within a
minute, but the lesson is cheap: after `cargo fmt`, re-read the file before
pattern-matching against remembered text, and do not chain edits that depend on
each other in one script.

Next: **Task 24** — playlist, album, and artist lists. The playlist pane
currently shows only its heading, so the app cannot yet be navigated without
already knowing Enter loads the first playlist.

### 2026-08-31 — implementation agent (Task 24, every browse pane renders)

**Task 24 done.** `crates/ytm-tui/src/widgets/playlists.rs` — `draw_playlists`,
`draw_albums`, `draw_artists` — wired into `render::draw_main`. 7 tests. Gate
green, committed.

**Verified against the real library.** The playlist pane lists the owner's 10
playlists with counts: `Eng_songs 87 tracks`, `Funk 90 tracks`,
`AFTER EFFECTS TRANSITIONS 400 tracks` — the same titles recorded when Gate 1
went green. Albums and artists are covered by unit tests only: the owner's
library returns none through `library_albums`/`library_artists` on this account,
so there is nothing live to render there yet. Worth knowing before someone reads
an empty Albums pane as a bug — check the log for `event="albums" rows=0` first.

**No `read-only` marker appeared in the live run** because none of the 10 came
back with `is_system: true`. Both branches are unit-tested, so the marker is
pinned either way; if "Your Likes" should appear in the library list and does
not, that is a `mapping::playlist_from_library` question, not a widget one.

**One shared helper, three widgets.** `draw_list` holds the scaffolding every
list repeats — zero-area guard, empty-state message, `visible_window`, selection
style — and takes a closure that builds one row's spans. The three public
functions are then just column math. This is deliberate: the plan said "follow
the `tracklist::draw` shape exactly", and four hand-copied versions of that
shape would drift the first time one of them changed.

**Two plan deviations.**
- The plan specified a 60% title column for playlists. A playlist row has only
  one text column, so 60% would leave ~40% of every row blank while truncating
  long titles for nothing; the title now takes what the count and marker do not.
  `AFTER EFFECTS TRANSITIONS` fits because of it.
- Its `a_system_playlist_is_visually_marked_as_read_only` test asserted only
  that the title rendered — which the other tests already cover, so it would
  pass with no marker at all, exactly the FR-C requirement it names. It now
  asserts `read-only` is present, plus a new inverse test that an editable
  playlist does **not** show it (or printing the marker unconditionally would
  pass). Also added an empty-state test across all three panes.

**`draw_main`'s match is now exhaustive on `Pane`** — no `_` arm. Adding a pane
will fail to compile rather than silently rendering nothing, which is how the
missing panes went unnoticed between Tasks 21 and 23.

Next: **Task 25** — search with debounce, the last task in Phase 6. Note that
`Task::Search` in `app_loop.rs` is `#[allow(dead_code)]` and `spawn_task` already
handles it; Task 25 emits it and removes the attribute. `Pane::Search` already
routes to `tracklist::draw`, so results will render as soon as they arrive.

### 2026-08-31 — implementation agent (Task 25, Phase 6 complete)

**Task 25 done. Phase 6 complete.** `search_state.rs` (the debounce),
`widgets/search.rs` (the query row), search editing in `AppState::apply_input`,
and the tick wiring in `app_loop.rs`. 21 new tests; 146 pass workspace-wide.
Gate green, committed.

**FR-S2 verified live.** Typing `boards` one key at a time, 50ms apart, produced
**one** request — `event="search" rows=20 ms=623` — and real matches rendered
(`Lords of the Boards`, `You Retreat In Time And Space / Boards of Canada`). Six
keystrokes, one API call.

**A false alarm worth recording, because the next agent will see it too.** My
first run logged *two* searches and then hung until the timeout. Both had the
same cause: the `q` I sent to quit went into the search field, because a focused
text field is supposed to swallow letters. So `"boardsq"` fired as a second query
(0 rows) and the app never quit. That is correct behaviour, not a debounce bug —
**press Esc before `q` when driving the app through a pty.**

**Wiring, and why it is split the way it is.**
- `note_search_input` is called from the *keystroke* arm and only reads
  `state.search_query` — it never takes the character. The reducer owns the
  buffer, so the debounce cannot drift out of sync with what is on screen. It
  ignores keystrokes outside `Pane::Search`, since nothing else edits the query.
- `search_tick` is called from the *tick* arm and returns `Option<Task>`, so the
  loop's only new responsibility is `spawn_task`. Both helpers are pure enough to
  test without a terminal, which is how the three loop tests work.
- `AppEvent::SearchResults` already drops results whose query no longer matches
  (Task 19), so an out-of-order response cannot overwrite newer results. That
  check is load-bearing now rather than theoretical.

**Esc/Enter leave the search field without clearing anything.** Both set
`focus = Focus::Main` and keep the query and results — the user's next move after
searching is to navigate what they found. Tested.

**Four plan deviations.**
- `assert!(DEFAULT_DEBOUNCE_MS >= 280)` fails clippy under `-D warnings`
  (`assertions_on_constants`). Now `const { assert!(…) }`, which is stronger:
  lowering the constant fails to *compile* rather than failing a test run.
- Added `tail_to_width` for the query row. `truncate_to_width` keeps the *start*
  of a string, which would hide the characters just typed; the input keeps the
  end. Covered by a 40-column test with a 120-column CJK query.
- The search pane distinguishes "searched, found nothing" (`No matches`) from
  "not searched yet" (`type to search`). `tracklist`'s generic empty state would
  have shown the same text for both.
- Added `retyping_a_previous_query_fires_again`: `should_fire` compares against
  the last query fired, not a history, so deleting back to `bo` and retyping
  `boards` searches again. Without that test the "don't fire twice" rule could
  have been implemented as a permanent block.

**One `AppState` behaviour worth knowing:** `Backspace` uses `String::pop`, which
removes a whole `char`. Truncating by one byte would split a multi-byte codepoint
and panic — there is a test with `日本` for exactly this.

Next: **Task 26** — queue view, start of Phase 7. Note from Task 14 still stands:
`PlayerCommand::PlayNow` inserts after the current track rather than replacing the
queue, so the queue grows and keeps history. If the queue view should show
something else, that is a UI decision to make in Task 26, not an actor bug.

### 2026-08-31 — implementation agent (Tasks 26-27, Phase 7 complete)

**Tasks 26 and 27 done. Phase 7 complete.** `widgets/queue.rs` (the queue view),
queue-edit dispatch, `widgets/toast.rs` (toasts + spinner), `widgets/help.rs`
(the `?` overlay). 174 tests pass, gate green, two commits.

**Verified against the live account with real audio**, not by inspection. Drove
the app through a pty: opened a playlist, enqueued three tracks with `a`, `u`
showed them with `▶` on the playing one, `J`/`K` reordered, `x` removed the
current entry and playback moved to the next track, `C` emptied the queue and the
bar read "Nothing playing". The owner confirmed hearing the music. `?` renders all
31 bindings in two columns with nothing truncated.

**The pty driver is worth rebuilding rather than rediscovering.** Two traps cost
time:
- **A pty has no window size unless you set one.** Without
  `ioctl(TIOCSWINSZ)` the terminal reports 0x0, every `Rect` guard bails, and the
  app emits ~2.8KB of pure escape codes and no frame. It looks like a broken
  render; it is a broken harness.
- **`x` on the current entry makes the actor block on a yt-dlp resolve** (~5s)
  for the new current track. My first run sent `C` 1.5s later, so clear was still
  queued when the driver quit — and the screen showed the queue unchanged. Not a
  bug. Allow ~8s after any key that changes what is playing.
  (Also still true from Task 25: press Esc before `q`.)

**Task 26 plan deviations.**
- The plan left the reorder keys as "`ToggleMark` + movement, or a dedicated
  pair". Chose dedicated `J`/`K` (plus `C` for clear): overloading `v`+`j` would
  make marking mean two things, and marking is needed unchanged for bulk add in
  Task 31.
- `queue.rs` does not reuse `tracklist::draw`. Same columns, but the left column
  means the play position here and the multi-select bullet there, and the empty
  state differs ("Queue is empty"). Sharing it would have meant threading a mode
  flag through for two divergent behaviours.
- Added `an_entry_that_is_not_current_gets_no_marker`. Without it, printing `▶`
  unconditionally would satisfy the plan's marker test while telling the user
  nothing.
- Added an out-of-range guard test for `J`/`K` at both ends. `Queue::move_item`
  is index-based, so a move off the end would reach the actor as a bad index.

**Task 27 plan deviations, and one that matters for Task 29.**
- **`render` now takes `&KeyMap`.** The help overlay must list the *live*
  bindings — a user who rebinds `quit` must not be told to press `q` — and
  keymap is config, exactly like the `&Theme` already threaded through. It is not
  on `AppState`: state is what changes per event, and this does not. Every call
  site updated; test helpers pass `KeyMap::default()`.
- **The overlay is sized to its content, not to the plan's 60%x70%.** With all
  31 bindings, a fixed 60%x70% box on 80x24 forces three columns of 11 label
  columns, which truncates "play / pause" to "play / pa…". A truncated binding
  might as well not exist, so `layout()` picks the fewest columns that fit the
  height and the widest that fit the width, clamped to 90% of the frame. Two
  tests pin this: every action name appears in full, and the rebound key sits on
  the same row as its action.
- Toasts keep the **newest** three, not the oldest. The last thing that happened
  is what the user is trying to understand. Two tests: three of six visible, and
  the newest survives while the oldest is dropped.
- The spinner draws from `elapsed_ms` rather than a stored `ThrobberState`, so
  rendering stays a pure function of `AppState` and the frame is reproducible in
  a test. `to_symbol_span` + a computed `calc_step`, not `render_stateful_widget`.
- Action labels in `help.rs` are hand-written, not derived from `Debug`. The
  overlay should read as help text, not as Rust. `Confirm`/`Cancel`/`NextPane`
  and the text-entry actions return `None` — they are not keys a user presses on
  purpose.

Next: **Task 28** — optimistic mutation tracking, start of Phase 8. Note that
`AppEvent::MutationOk`/`MutationFailed` already exist and currently only push a
toast; `app.rs` carries the comment `// Rollback itself is wired in Task 28`.
`ConfirmAction` and `PromptAction` also already exist unused, for Task 29's
modals — `render` has an `if let Some(Modal::Help)` where the other arms go.
**Open question 1 (`set_video_id` missing from `ytmapi-rs` playlist reads) must
be decided before Task 32** — FR-C5 cannot work as specified without it.

### 2026-08-31 — implementation agent (Tasks 28-32, Phase 8 code complete)

**Tasks 28-32 done, five commits, gate green on each. 230 tests pass.**
`mutation.rs` (the optimistic log), `widgets/modal.rs` (confirm + prompt +
playlist picker), and the CRUD wiring in `app_loop.rs`.

**Stopped short of the live account on purpose.** Every manual step in Phase 8
writes to the owner's real library, so all of it is `MockSource`-tested and the
asks are collected under "Owner decisions due". Read that section before
assuming Phase 8 is finished.

**Open question 1 came due at Task 32, exactly as the plan predicted.** FR-C5
cannot work: playlist reads carry no `setVideoId`. `open_remove_confirm` refuses
with a toast rather than sending a doomed request, and every layer behind it is
built and tested, so options A/B/C differ only in the parsing. Decision needed.

**`MutationOk` gained a `real_id: Option<PlaylistId>` field**, as Task 30's notes
require: `commit` swaps the temp id for the server's, so the optimistic row is not
left holding `ytm-cli-temp-N` until the next refresh.

**Task 28 additions beyond the plan.**
- `MutationLog::peek_token`. A create needs its token to build the temp playlist
  id *before* `begin_mutation` applies the edit; calling `next_token` for the id
  consumed a token and made the id name a different edit than the one that
  settles it. Peek fixes that, with a test that it does not consume.
- `a_token_cannot_be_settled_twice`. `take` removes on settle, so a duplicated
  `MutationFailed` is a no-op — worth pinning, because the alternative silently
  reverts an unrelated later edit.
- `several_removed_tracks_all_return_to_their_own_indices`. The plan's rollback
  test only removed one track, which cannot catch an ascending-insert bug.

**Task 30-32 decisions worth knowing.**
- **Mutations ride `Task::Mutate` through the existing `spawn_task`**, not a
  separate `spawn_mutation`. The plan sketched the latter; two spawn paths would
  drift, and the loop already had one. `spawn_mutation` was written, then deleted.
- **`y`/`n` are resolved in `dispatch_input`, not bound in the keymap.** They mean
  nothing outside a confirm, and a global binding would shadow real keys (`n` is
  next-track). Tested through `dispatch_input` so the path a user actually takes
  is covered.
- **`Modal::PickPlaylist` carries its own `choices`** rather than reading
  `state.playlists` at draw time, so a background refresh cannot move the row
  under the user mid-decision. System playlists are filtered out — YouTube
  rejects adds to them, so offering one is offering a failure.
- **`submit_pick` clears `marked`.** The marks were the input to the action;
  leaving them set makes the next `A` silently repeat it.
- Refusals happen *before* any optimistic change: renaming or deleting a system
  playlist, and submitting an empty name, all toast and stop, so there is nothing
  to roll back. Tested for each.

**One thing the next agent will trip on.** Adding a `Modal` variant breaks
`widgets/modal.rs`'s match — deliberately, same reasoning as `draw_main`'s
exhaustive `Pane` match. If `PickPlaylist` had been added with a `_` arm the
picker would have rendered nothing at all.

Next: the two owner decisions, then **Task 33** (SQLite cache, Phase 9).

### 2026-08-31 — implementation agent (FR-C5, then three live bugs)

**The owner drove the app against their real account, and it broke three ways.**
Every one had passed the full suite. That is the headline of this session: Phase
8 was "code complete and green" and still could not create a playlist.

**FR-C5 now works** (option A, owner's choice). `playlist_raw.rs`, one request,
83/83 removable live. Details under open question 1 — especially why pairing is
by `videoId` with a cursor and not by position, which took two attempts and a
live measurement to get right.

**Bug 1 — the create prompt could not be typed into.** `keymap.resolve` got
`state.focus`, which is not `SearchInput` while a modal is open, so letters
resolved as commands and `q` quit the app. Confirms were worse: `y` matched
nothing, `n` skipped the playing track. `AppState::input_focus()` fixes both.
**The Task 29 tests fed `InputAction::Char` directly into `apply()`**, so they
tested the reducer and never the path to it. New tests go through `resolve`.

**Bug 2 — no cursor in the prompt.** An empty field was blank space.

**Bug 3 — the wrong playlist id form on every mutation.** Full table under
"Live verification". Short version: the library reports `VLPL…`, the mutation
endpoints want `PL…`, and create only worked because it takes no id.
`is_system_playlist` was broken the same way and had been since Task 5.

**What to take from this.** `MockSource` cannot catch a wrong id format, and a
reducer test cannot catch a keymap that never calls the reducer. Both gaps were
at a **boundary the tests stubbed out**. The remaining manual steps are not
paperwork — they are the only thing exercising those boundaries.

Two read-only diagnostic examples were added on the way and left in the tree
(`dump_playlist_tracks`, `check_removable`), plus `verify_edit`, which is
reversible: it renames a playlist and restores the title. Not in the plan; kept
because each one would otherwise have to be rewritten next time this breaks.
Flagged to the owner.

Next: the owner retests delete and add-to-playlist, then **Task 33** (SQLite
cache, Phase 9).

### 2026-08-31 — implementation agent (Task 33, the cache and an NFR-1 fix)

**Task 33 done.** `crates/ytm-core/src/cache.rs` plus the startup wiring in
`main.rs` and `app_loop.rs`. 14 new tests, 266 pass workspace-wide. Gate green,
committed.

**Measuring the budget found a real violation, and it was not in the cache.**
Step 6 says to check that nothing awaits the network before the first
`terminal.draw`. Something did: `build_source` calls
`YtMusic::from_cookie_file`, which does a **cookie-validation round trip** before
returning a handle, and `main` ran it before the terminal existed. Warm cold
start was **2480 ms** against a 300 ms budget, all of it a blank screen.

Fixed by ordering `main` as: open cache -> preload state -> open terminal ->
**draw** -> build the source -> run the loop. **62 ms** to first frame, measured
from process start against a `first frame drawn` log line (nothing on screen is
observable from outside the process, so that line is the measurement point and is
worth keeping). The log shows `event="playlists" rows=11` arriving *after* the
frame, so the refresh still works behind the reorder.

Two consequences of that move, both deliberate:
- `preload_from_cache` is `pub` and called from `main`, not from `run`. `run`
  still redraws immediately, so it stays correct when called with a cold cache.
- A `build_source` failure now happens with the terminal already in the alternate
  screen, so that arm drops the guard before returning the error — otherwise the
  report prints where it cannot be read. Same reasoning as the panic hook.

**Write-through skips an empty library response, on purpose.** Under cookie auth
an expired cookie answers HTTP 200 with zero rows (see "Cookie expiry"), and
caching that would turn a one-off auth lapse into a wiped cache — the next launch
would come up blank with nothing to fall back on. An account that really is empty
just keeps a stale cache, which is the cheaper mistake. There is a test.
Search results are not cached either: they belong to a query, not to the library.
Playlist tracks *are* written through even when empty, because they are keyed by
playlist id and cannot wipe anything else.

**Two small deviations from the plan's code.**
- `rusqlite` 0.40.2 has no `FromSql`/`ToSql` for `u64`, so `duration` is stored
  and read as `i64` (`.max(0)` on the way back). The plan's `r.get(5)?` into a
  `u64` does not compile.
- `save_*` uses `conn.unchecked_transaction()`, not `conn.transaction()` — the
  latter needs `&mut Connection`, and `Cache`'s methods take `&self` so the whole
  thing stays shareable.

**Note for Task 38 (requirement checklist).** NFR-1 is measured and green at
62 ms, but from the *log*, not from a human watching the screen. The
`cached_tracks=0` in that line is also worth knowing: the library-songs pane is
only fetched when the user visits it, so a first-launch cache holds playlists
only. Nothing is wrong; a cold Songs pane on the second launch is expected.

Next: **Task 34** — album art. Note the tmux/`TERM=tmux-256color` caveat recorded
under "Environment": kitty graphics may be blocked, and the documented response is
to record it as a known limitation rather than fight it.

### 2026-08-31 — implementation agent (Tasks 34-36, and a self-inflicted regression)

**Tasks 34, 35, and 36 done.** Album art, MPRIS, and the CLI subcommands. Four
commits, gate green on each, 297 tests pass.

**The headline is Task 34's regression, because it is the second session running
where "code complete and green" hid a total failure of the app.** Album art broke
playback outright: probing stdio under tmux leaves crossterm delivering no key
presses, so Enter reached nothing and no track could ever play. The full suite
passed throughout — `ArtCache` is unit-tested, and no unit test presses a key in
a real pty. Written up under "Album art under tmux", including the A/B table that
found it. What generalises: **a change that touches terminal I/O cannot be
validated by `TestBackend`**, and the visible `Ptmux;…` escape in the captured
screen was the clue that pointed at it.

**Task 34 — two corrections to the plan, both read from the vendored source.**
- The plan says call `Picker::from_query_stdio` *before* entering the alternate
  screen. Its own doc comment in 11.0.6 says **after**, and that is right — it
  also must be before the event stream exists, or the reply is read as a key press.
- It blocks up to **2000 ms** on a terminal that never answers
  (`STDIN_READ_TIMEOUT_MILLIS`), so it runs after the first draw or it spends the
  entire NFR-1 budget. `main`'s order is now: cache -> preload -> terminal ->
  draw -> probe -> source -> loop.
- `split_for_art` is pure math and unit-tested: no panel unless art is
  displayable, something is playing, *and* the list keeps 48 columns. One of my
  own tests contradicted itself (asserted a 60-column area gets a panel when the
  threshold is 72); the threshold was right and the test was wrong.
- `image` 0.25.10 is now a direct dep — `new_resize_protocol` takes
  `image::DynamicImage` and ratatui-image re-exports only `FilterType`.

**Task 35 — MPRIS, live-verified with `playerctl`** (the owner installed it
mid-session). `playerctl -p ytm_cli` reported `Playing` with title `FREAKED OUT`
and artist `Fat Papi, prodshushy`; `play-pause` paused and resumed real audio;
`next` arrived as `media key cmd=Next`. Notes:
- Metadata rides `TrackChanged`, status rides `StateChanged`, neither rides the
  tick — 4Hz `Progress` would be constant D-Bus traffic for a position the
  desktop widget interpolates itself.
- `SetVolume` is clamped before the `u8` cast. souvlaki documents the value as
  "intended 0.0-1.0, but other values are also accepted", so a negative wraps.
- `PlaybackState::Loading` maps to MPRIS `Playing`, not `Stopped`, or a widget
  flickers on every track change. There is no third MPRIS state.
- `Seek`/`SeekBy`/`SetPosition`/`OpenUri`/`Raise`/`Quit` stay unmapped on purpose.
- `playerctl` showing `Stopped` right after `next` is **correct**, not a bug:
  `PlayNow` leaves a single item in the queue, so Next runs it out.
- Neither `mpd` nor `mpc` nor `ueberzugpp` is needed — we drive mpv directly, and
  ratatui-image speaks the graphics protocols itself. Only `playerctl` was.

**Task 36 — subcommands, each one actually run.** `--help` lists all five;
`playlists` printed 11 real titles with counts; `cache clear` reported success;
`cache` with no action and an unknown subcommand both exit **2**. The TUI body
moved into `run_tui` so a subcommand returns before any terminal setup.
`playlists` treats an empty library as an **error**, not as silence — the same
expired-cookie trap the TUI hint covers. `logout` uses `TokenStore::clear`
(documented idempotent) and deliberately leaves a cookie file alone.
`ytmapi-rs` is now a direct dep of `ytm-cli`: `begin_device_login` takes
`ytmapi_rs::Client`, not reqwest's.

**Harness notes for whoever drives the app next.** Playback needs generous
timing: ~10s before the first Enter, ~12s before the second, and the track does
not reach `Playing` until roughly **45s** in (cookie auth, playlist fetch, then a
yt-dlp resolve). My first four attempts were simply too impatient and looked like
a bug. `pactl list sink-inputs` and `playerctl status` are the two cheap external
checks; neither needs the log.

Next: **Task 37** (README and setup docs), then **Task 38** (the requirement
checklist, which needs the owner pressing keys). Still outstanding for the owner:
retest delete and add-to-playlist, and check art in kitty outside tmux.
