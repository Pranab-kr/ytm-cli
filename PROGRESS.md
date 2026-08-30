# ytm-cli — Progress

**Read this first. Update it before you stop.** It is the handoff between agents.

Last updated: 2026-08-30 by the planning agent
Current phase: **Phase 0 — not started**
Next action: **Task 1, Step 1** — `git init` in the repo root

---

## Where things stand

Planning is done. No code has been written. The repo contains only
`.claude/settings.local.json` and the four documents below. Git is initialized
(branch `main`) but has no commits yet.

Nothing is blocked. The next agent starts at Task 1 and works down.

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
| 0 — Workspace | 1–2 | ⬜ not started | Cargo workspace builds, gate script, logging, terminal guard |
| 1 — Core models | 3–5 | ⬜ not started | Models, `MusicSource` trait, `MockSource` |
| 2 — Auth ⚠️ | 6–9 | ⬜ not started | OAuth login, keyring, live library fetch |
| 3 — Audio ⚠️ | 10–12 | ⬜ not started | yt-dlp resolver, mpv plays a real track |
| 4 — Player | 13–16 | ⬜ not started | Actor thread, queue, transport |
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
| 1 — live API | ⬜ untested | |
| 2 — real audio | ⬜ untested | |

---

## Decisions already made — do not re-litigate

Settled with the owner on 2026-08-30. Reopening these wastes a turn.

- **Auth:** OAuth device-code is primary, browser-cookie is a first-class
  fallback. Both behind `MusicSource`.
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
**Browse:** B1 ⬜ B2 ⬜ B3 ⬜ B4 ⬜ B5 ⬜
**Search:** S1 ⬜ S2 ⬜ S3 ⬜
**Playback:** P1 ⬜ P2 ⬜ P3 ⬜ P4 ⬜ P5 ⬜ P6 ⬜ P7 ⬜
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
| 8.5 | Create a Google Cloud OAuth client ("TV and Limited Input"), put id/secret in config, authorize in a browser | ⬜ |
| 9.1 | Capture a real API response as a test fixture, scrubbed of account ids | ⬜ |
| 12.6 | Confirm audio is audible | ⬜ |
| 30.5 / 31.5 / 32.5 | Verify playlist edits appear in the YouTube Music web UI | ⬜ |
| 34.5 | Check album art in a graphics-capable terminal | ⬜ |
| 35.5 | Check media keys and `playerctl metadata` | ⬜ |

---

## Open questions

None. Raise new ones here rather than guessing, and keep working on
everything that does not depend on the answer.

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
