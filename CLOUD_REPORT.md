# CLOUD_REPORT: herdr-horizon-digest (theseus-peqbl)

Branch `cloud/20261016-herdr-horizon-digest`. One step.

## What I found
- The task names `cli/herdr_up.rs` and `cli/herdr_slots.rs`; the files are `crates/theseus/src/herdr_up.rs` (+ `herdr_up/{state,wire}.rs`). `herdr_slots.rs` is the per-pane reporter and needed no change.
- The Space row (`workspace.report_metadata`) carried `needs/working/done/spend` only. `work.digest` and the seen file (`seen::looked`) already existed; `status.short` has a next-reminder field, but the follower already holds the board's wake and routine views from `work.watch`, so no extra request is needed for the horizon counts.

## What I changed (commit ff489e19)
- Space row tokens `since ↺N` (digest lines after the machine's seen position, owner's own answers not counted; read only, never records a look), `next ⏰HH:MM` (next reminder), `routines 🔁N` (routines due now). Empty strings when there is nothing, so a stale token is cleared.
- The digest is read on its own short connection (so the watch's notifications are not read past) on every daemon connect, after `events.lost`, and when a task ends. A first start (no seen position) says nothing, as `theseus status` does.
- New `herdr_up/horizon.rs` (pure) holds wake(`purpose=remind`) and routine views; no tabs for horizon kinds. One timer for the next routine due (`Follower::horizon_deadline`).
- `Counts` gained fields; `counts(now_ms)` now takes the clock. AGENTS.md of crates/theseus updated.
- No protocol change, no store format, no new dependency, no config.

## Proof
- `cargo nextest run -p theseus --test herdr_up`: 9/9. New: `the_space_row_carries_the_digest_and_the_horizon_after_a_disconnect` (scripted daemon + herdr: empty `since` at first; daemon goes down, digest gains 3 lines incl. one answered, daemon returns; row shows `↺2`; routine due `🔁1`; `⏰` present; only 3 tabs, none for the wake/routine).
- Unit tests: `horizon::tests::the_next_reminder_and_the_routines_due`, `wire::tests::the_space_row_words_its_counts` (extended, incl. cleared tokens).
- Planted revert: `counts()` sets `since: 0` -> the new integration test fails ("timed out waiting for the digest on the Space row"); restored and touched.
- `cargo nextest run -p theseus` (TZ=America/Phoenix): 606/606 passed. `cargo clippy -p theseus --all-targets -D warnings` clean. `cargo fmt --check` clean.

## Live check for the maintainer
With a daemon and herdr: `theseus digest` (records a look), let a task finish, restart the daemon, then in the herdr `theseus` workspace read the Space row's tokens (`herdr api` workspace metadata): `since` should read `↺1`; `theseus remind 10m x` should show `next ⏰HH:MM`. The herdr token shapes (and that an empty token clears one) follow the existing design and were not checked against herdr's schema.

## Uncertain / design notes
- "Routines due" means next run time already reached and not yet run (usually 0 or briefly 1); say if you prefer "due today".
- The digest is not re-read on a timer, only on the events above, so after the owner runs `theseus digest` elsewhere the count stays until the next trigger.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` FAILED in the door/keel phase, before building: it flags three erosions (`crates/theseus-sim/src/lifecycle.rs` seed budget, `scripts/keel-guard.py`, `scripts/test_keel_guard.py`) and refuses three `Keel:` acks whose signatures cannot be verified on this VM (`%G? is E`), all in commits already on the base branch, not my diff. I ran the later checks myself: fmt, clippy, full `theseus` package suite (above) all green. Not run: whole-workspace suite, cockpit build, cargo deny (fetch not run).
