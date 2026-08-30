# CLAUDE.md — how to work in this repo

A terminal YouTube Music client in Rust. Read this, then `PROGRESS.md`, then
start where `PROGRESS.md` says.

## Start of every session

1. **`PROGRESS.md`** — current phase, next action, gate results, open questions.
2. **`docs/superpowers/plans/2026-08-30-ytm-cli-design.md`** — the spec.
   Requirements are IDs (FR-A1, NFR-2, …) and the plan references them.
3. **`docs/superpowers/plans/2026-08-30-ytm-cli-plan.md`** — 38 tasks, in order,
   with real test code and exact commands.

Do not re-plan. The design is settled and the tasks are written. If you think a
task is wrong, say so in one or two sentences and ask — do not silently
substitute your own approach.

## End of every session

Update `PROGRESS.md` before you stop: phase status, gate results, the log entry,
and anything the next agent would otherwise have to rediscover. An agent that
finishes work without updating `PROGRESS.md` has not finished.

## Executing the plan

Use `superpowers:subagent-driven-development` (fresh subagent per task) or
`superpowers:executing-plans` (inline, batched). Either way:

- Work tasks in numerical order. They build on each other.
- Within a task, work steps in order. Do not skip the "watch it fail" step —
  a test that passes before the implementation exists is testing nothing.
- Check off `- [ ]` boxes as you complete steps.
- Commit at the end of every task with the message the task specifies.

## Non-negotiables

These come from the spec's Global Constraints. Violating one is a bug even when
the code compiles and the tests pass.

- **TDD.** Failing test first, always. Every task in the plan is written this way.
- **Never `println!` / `eprintln!` / `dbg!`** outside `examples/` and the
  `playlists` subcommand. Stdout corrupts the TUI frame. Logging goes to a file
  via `tracing`.
- **The UI thread never blocks on I/O.** Network calls are `tokio::spawn`ed and
  post an `AppEvent` back. `mpv.wait_event` blocks, so it lives on its own OS
  thread — never on the runtime.
- **Truncate with `unicode-width`, never `str::len()`.** Terminal layout is
  columns, not bytes. CJK and emoji titles break byte-based math.
- **Secrets never touch disk, logs, or git.** Tokens go to the OS keyring.
  `StoredToken`'s `Debug` impl is hand-written to redact — keep it that way.
- **Exact dependency versions** from the spec's §7 table. No version bumps, no
  `*`, no adding crates that aren't listed without asking.
- **Gate before every commit:** `./scripts/check.sh` — fmt, clippy with
  `-D warnings`, and the full test suite. All three clean.

## Architecture rules

```
ytm-cli → ytm-tui → ytm-player → ytm-core
```

Dependencies point one way only. Specifically:

- `ytm-tui` must never import `ytmapi_rs` or `libmpv2`. It sees only the
  `MusicSource` and `Player` traits. This is what makes the whole UI testable
  with no network and no audio device — do not break it for convenience.
- `ytm-core` knows nothing about audio or terminals.
- `ytm-player` knows nothing about ratatui.
- `AppState` is owned by the event loop. No `Arc<RwLock>`, no shared mutation.

## Verified API signatures

The spec's §8 holds real `ytmapi-rs` 0.3.3 and `libmpv2` 6.0.0 signatures, read
from the crate sources on 2026-08-30. Use them.

If a call does not compile, read the vendored source rather than guessing:

```bash
ls ~/.cargo/registry/src/*/ytmapi-rs-0.3.3/src/
ls ~/.cargo/registry/src/*/libmpv2-6.0.0/src/
```

Guessing at field names in these crates has already cost time once. Two known
traps: `ytmapi-rs` duration fields are often display strings (`"2:29"`) rather
than seconds, and `OAuthToken`'s fields may be private.

## The two things that can kill this project

**Task 9 Step 7** — real playlist titles print from the owner's account.
**Task 12 Step 6** — real audio comes out of the speakers.

Both need the owner's Google account and their ears. Do not write TUI code
before both are green. If one fails, the cause is external — Google changed
something, yt-dlp needs updating, the OAuth client was rejected. Record the
exact error in `PROGRESS.md` and try the documented fallback. Do not build on
top of an unproven foundation, and do not paper over a failure with a mock.

## When you need the owner

Stop and ask for: OAuth client creation, browser authorization, capturing a
real API fixture, confirming audio is audible, and verifying playlist edits in
the YouTube Music web UI. These are listed in `PROGRESS.md` under "Manual steps".

Everything that does not depend on the answer should be finished first. Do not
block the whole task on one question.

## Test fixtures

Live API responses in `crates/ytm-core/tests/fixtures/` must be scrubbed of
account ids, emails, and user-specific `browseId`s before committing. If you
hand-write a fixture instead of capturing one, mark it `SYNTHETIC` in
`PROGRESS.md` so it gets replaced later.

Tests never hit the live network. Anything that does is `#[ignore]`d and run by
hand with `-- --ignored`.

## Legal context

This uses YouTube Music's internal API and yt-dlp, which is against YouTube's
Terms of Service regardless of a Premium subscription. The owner decided that
is acceptable for a personal tool. Practical consequences:

- No telemetry, analytics, or crash reporting that leaves the machine. Ever.
- No real cookies, tokens, client secrets, or account identifiers in git.
- Keep the ToS note in `README.md`.

## Scope

The plan's final section lists what is deliberately out of scope — lyrics,
podcasts, uploads, multi-account, server-side track reordering. Several are one
`ytmapi-rs` call away, which is exactly why they are written down. Do not add
them without a new decision from the owner.

Build what the task says. A bug fix does not need the surrounding code cleaned
up; a widget does not need a configuration system.

## Commands

```bash
./scripts/check.sh                      # the gate: fmt + clippy + tests
cargo test --workspace                  # tests only
cargo test -p ytm-core model            # one module
cargo test --workspace -- --ignored     # network/audio/keyring tests, by hand
cargo run -p ytm-cli                    # the app
cargo run -p ytm-cli -- playlists       # fastest auth check, no TUI
YTM_LOG=debug cargo run -p ytm-cli      # verbose logs (to the file)
```

Logs land in the platform cache dir under `ytm-cli/logs/`. When the TUI
misbehaves, read the log — you will not see errors on screen by design.
