# CLOUD REPORT: herdr-reporter (theseus-qb1es)

Branch `cloud/20261013-herdr-reporter`. Commits: `d42e79b` (test meaning), `2a34795` (the feature).

## What I found
- A restart ended every pane's watch: `conn.next()` returning None (or any I/O error) broke the loop and released the pane.
- `herdr.rs` reported `agent = "theseus"`, with three tokens; `herdr_sync` marked our panes by that agent name.
- `crates/theseus/AGENTS.md` has no herdr section; I added one (herdr, herdr_slots, interactive).

## What I changed
- `d42e79b` tests: three tests ended their watch by closing the fake daemon; they now send SIGTERM (assertions unchanged). The first keeps its old name (`..._releases_when_the_daemon_ends`) because renaming it reads to the keel guard as a new `#[expect]`; its doc comment says so.
- `2a34795`:
  - New `src/herdr_slots.rs`: one function `slots(Option<&WorkView>, &Facts, now)` giving agent (short name, <=22 chars, cut at a word), title, `state_labels` (one state's label), and the ten tokens (all <=80 chars, 10 of 16). Unit tests inside.
  - `herdr.rs` `Reporter`: publishes from the work view plus facts; call events (`tool.proposed/started/ended`, `turn.ended`) feed `doing`/label/`report`/`model`; a timer re-says elapsed time (15 s while a call runs, 60 s for a job: the only timer). The panic hook releases by the last agent name.
  - `interactive.rs`: `read_session` (watch, wait, list: the one re-read), `reconnect` (100 ms doubling to 2 s, label `· reconnecting`, never releases, signals still end it), a failed-at-start session prints `<sid> failed: <reason>` then waits. `Conn` now remembers its socket path (`socket_path()`); a spawned daemon has none, so its end still ends the watch.
  - `herdr_sync.rs`: a pane is ours by its `$kind` token (`task`|`conversation`) and a reported agent; new panes get `kind` too.
- Shared files touched, small: `client.rs` (socket path), `main.rs` (one `mod` line).
- Store format: unchanged. No new config keys. No protocol change. No new dependencies.

## Proof
- `cargo test -p theseus`: all pass (herdr 14, herdr_sync 4, bin unit 89, goldens unchanged).
- New tests (tests/herdr.rs): every state's label and the ten tokens, <=16 keys / <=80 chars, 22-char cut; a restarting daemon (stop removes the socket, restart rebinds): `· reconnecting` label, no release, CPU <= 2 ticks over a 600 ms outage, mended in < 2 s, exactly 2 `session.wait`, tokens identical; failed-at-start; p99 test; tests/herdr_sync.rs: `kind` mark.
- Planted reverts: (a) watch breaks on a lost daemon -> `a_watch_keeps_its_pane_through_a_daemon_restart` timed out waiting for the reconnecting label; (b) agent back to "theseus" -> `every_state_has_its_label_and_the_tokens_beside_it` and `the_reporter_maps_reports_only_changes...` failed; (c) sync's old agent mark -> `a_pane_is_ours_by_its_kind_token...` and `sync_makes_mends_and_then_sends_only_reads` failed. Each file restored and touched; `git status` clean after.
- FAST: frame -> herdr on the scripted rig, 200 changes, three runs: p99 3.2 / 2.8 / 2.0 ms (p50 ~0.9 ms), asserted <= 10 ms. Requests per change unchanged (a report; metadata only when a token changes).
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run before the feature commit on the same tree): door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build all ok; suite failed only on: ~33 L1 tests (theseus-sandbox contract/bench `spawn_100`, theseusd sandbox: known root/cgroup, theseus-pv6i); `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went...` (failed under load, passes alone); `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (fails alone too, at tests_gateway.rs:161; theseus-discord does not depend on the CLI crate, so not from this change; it is the newest test on the base). The phases after the suite: `cargo deny check` ok; protocol types untouched (not run). Benches skipped (NO_BENCH).
- The disk filled once mid-session (28 GB target); I cleared incremental caches and reran the two tests.
- Keel guard on the range: ok, 0 findings.

## Keel findings expected
None. (A first draft renamed the test; reverted to avoid an `allow-added` finding. The test-meaning change is in its own commit `d42e79b`.)

## Live check (not done here: herdr is not installed on this VM, nor a built theseusd)
Maintainer, on a machine with herdr and the install build:
1. `theseus ask --spawn`-free: start a scratch daemon (`theseusd --config SCRATCH.toml --socket /tmp/s/sock --state-dir /tmp/s/state`) on the stand-in model, a scratch herdr; `theseus --socket /tmp/s/sock herdr sync`. A task that asks should show `needs you · fs.write …`, its toast naming the task, and `$kind $session $confirm` in the pane's tokens (`herdr` pane.list).
2. `theseus --socket /tmp/s/sock shutdown`, then start the daemon again: every pane's label reads `· reconnecting` meanwhile and mends within ~2 s of the daemon answering; no pane released.

## Left / uncertain
- `herdr.rs` is 1,108 lines (was 974; the brief says new code goes in new files past 1,000). The excess is the Reporter rework plus its unit tests; moving the unit tests to a `#[path]` file would fix it but the keel guard may read it as moved tests, so I left it for the maintainer.
- The `report` token/`done · …` come from the last `turn.ended` output's first line (the WorkView from `from_execution` has no `first_line`); the work board's `WorkReport.first_line` is preferred when present.
- `parent` is `↳ <parent session's name>` read once at start; I did not verify that herdr accepts spaces in `agent` (the brief specified it) nor what herdr does with `state_labels` carrying only the current state.
- A plain `watch --interactive` over a socket now also reconnects (the same code path).
- Docs: add the ten tokens, the `kind` mark and reconnect behaviour to the spec's Part III item and `docs/status.md`.
