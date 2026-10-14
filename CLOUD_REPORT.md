# Cloud report: watches (theseus-f2r2b), branch cloud/20261013-watches

## What I found
`WakeRun::Probe` and `Purpose::Watch` were declared and refused. A wake already has a list, a cancel, a daemon cap
(`[kernel] max_watches`, 20), the commitments block and a drop at its conversation's end, so a watch is a wake and
needs **no store format bump** (no stored field added; `WakeProbe.{kind,subject,until}` carry the check, its
condition and a JSON `subject` with `then`; the note is the title, `repeat.every` the interval, `repeat.until_ms`
and `due_at_ms` the expiry).

## What I changed (commit 3c93c0d, one commit, gate-proven except the known failures below)
- Protocol: `watches.rs` (`watch.create`, `watch.list`), ledger kinds `watch.checked|met|expired|failing`, the ping
  rule (`wake_urgency`: purpose `watch.met` is an Interrupt with sound; `watch.expired`/`watch.failing` Inform), TS regenerated.
- Kernel: `watch.rs` (`probe_result`: one frame per news, state in META `watch.state.<wake id>`), `probe_run.rs` (an
  argv in L1 from a calling thread, no job record, head of the output only), watches may repeat every minute,
  `wake.snooze`/`wake.pause` refuse a watch.
- Core: `watch/{cond,spec,step,check,board,run}.rs`, `fact/watch.rs`, `watch_tool.rs` (the model's `watch`),
  `rpc/watch.rs` (`Act::Watch`: the owner, from a private place), `[watches] default_every|min_every|max_days`
  with template and tests, the commitments line (`👁 c3d4e5 PR 123 merged? (OPEN at 14:05)`), `Web::get_text`,
  `theseus_tools::rx` (regex without a new dependency), the driver's tick calls `Core::due_without_turns`.
- CLI: `theseus watches add|list|rm` (`watch_cmd.rs`; `theseus watch SESSION`, the follower, is unchanged);
  `watch.create` is in `client::OPERATORS`. `--field PATH` is the JSON path (`--json` clashes with the global flag).
- Daemon test `theseusd/tests/watches.rs`.

## Design choices for the owner
- `max_watches` stays `[kernel] max_watches` (already declared and in the template); `[watches]` holds the rest.
- A `run` check needs L1: a root daemon's refuses (theseus-pv6i), so on this VM a `run` watch fails with that
  reason (three failures inform once). No L0 fallback was built.
- `then: task {brief}` makes the conversation's turn carry the brief ("You asked for this when it held: ..."): a real
  task needs an arrangement quoted from a transcript, which only a turn can write. `then: turn` is the spec's node text.
- The command approval (D-H9) is `Plan.authority`, so the gate asks at every posture, with the command shown, and the
  answer binds the exact input; no verb edits a watch. `task_graph::tools::authority_for` leaves the watch's alone.
- The model's `watch` is not offered in a task or a shared place. The commitments value is the last frame's state.
- Adding the tool widened three calibrated windows: tests_route_keep (+600), tests_tiering (+700); the `watch`
  tool description is kept short for that reason.

## Proof
- Pure: 16 unit tests (cond, spec, step, check). Core integration: 8 tests in `watch/tests_watch.rs` (a file met within
  an interval in real time on the real driver; 20 unchanged checks write 0 frames, measured with `frames_written_here`;
  planted `ghp_` token scrubbed from rows, state and post; three failures inform once; expiry; restart checks once and the
  `changes` baseline holds; cap refuses with words; bounds) and 2 in `tests_watch_tool.rs` (a command watch asks once with
  the command shown, runs exactly the approved argv every time, `wake.move` refuses it, cancel stops it; `then: turn` gives
  the conversation a turn reading `👁 watch met: ... `). Daemon test: real `theseusd` + real CLI, passes.
- Planted reverts: an unchanged value written as news -> `twenty_unchanged_checks_are_news_once` and
  `twenty_unchanged_checks_write_no_frame` fail; scrubber skipped -> `a_secret_in_a_checks_output_is_scrubbed_everywhere`
  fails. Both restored and touched.
- Live check on a scratch daemon (this VM; model api_base on a dead port): `theseus watches add "deploy landed?" --file F
  --until 'equals MERGED' --every 1m`; file written 09:01:30; the `watch.met` row is at 09:01:31.69 (text `👁 deploy landed
  → MERGED (after 3 min 2 s, 4 checks, $0)`); the ledger holds 2 watch frames (`watch.checked` at the first look, `watch.met`)
  and 0 `provider.`/`turn.`/`loop.` rows over 4 checks.
  For the owner's machine: `theseus watches add "PR 123 merged?" --run 'gh pr view 123 --json state -q .state' --until
  'equals MERGED' --every 5m --for 3d` (needs L1 and `[sandbox] egress` for api.github.com), then `theseus watches`.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel (0 findings, see below), fmt, shape, clippy,
test build, reader rule pass; protocol types needs `git add` of the regenerated TS (done in the commit). Suite failures,
all outside this change: the known L1/root class (theseus-sandbox contract and spawn_100, theseusd sandbox, job and reaping
tests, tests_jobs and tests_job_handles: jobs cannot start as root without a cgroup); `theseusd::stop_checkpoint
a_clean_stop_with_every_service_on_leaves_nothing_to_replay` failed once under the gate's load and passed alone;
`theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` fails alone here too (it
holds the reply's post; I touched nothing in Discord, and did not run it on base, so it is unexplained: the base's
`wpv2g` row touched it). The benches were skipped (`THESEUS_GATE_NO_BENCH=1`).
Disk note: this VM's allowance is small; I built with `CARGO_PROFILE_DEV_DEBUG=0` after a full disk.

## Keel findings expected
- crates/theseus-kernel/src/tests_wake_owner.rs: `a_task_or_a_probe_is_refused_with_words_and_nothing_is_written` is
  renamed `a_task_is_refused_with_words_and_a_probe_is_a_watch` and rewritten (a probe is now accepted, in one frame),
  because the row builds what it refused. `keel-guard.py` printed it as "the same test, renamed and rewritten", 0 findings.
- Calibrated windows widened in tests_route_keep and tests_tiering (not an erosion).

## Left / uncertain
- `docs`: spec Part III item, `docs/status.md` and the horizon report §5 should say watches landed, no format bump.
- No cockpit view; `horizon-board` can read `watch.list`. `watch-replies` adds its `reply` check in `watch/cond.rs`/`spec.rs`.
- Checks counted in memory are persisted only on a frame, so a restart's count can lag by the unwritten checks.
