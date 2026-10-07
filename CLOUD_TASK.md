<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, the owner's machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (since d279767f), so none retries: rerun a failing one alone, and name it in the report. Batch 10's branches, reviewed or in review and joining main one at a time, fix those marked ‡; batch 11's sibling session daemon-flakes (below) works on those marked §.
  - theseus-core: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` can pass nextest's 120 s
    kill (theseus-0u6g‡); `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`
    (theseus-1g8j‡); `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`'s 20 ms bound
    (theseus-b38m‡); `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits` (theseus-9o2o‡); and
    `term::tests::a_close_leaves_no_child_behind` (theseus-d006‡, a negative assertion: keep its output);
  - theseus-core's aws/hands `runaway_mode_refuses_at_ten_times_the_hours_line` and
    `ten_times_refuses_and_a_cancel_still_runs` fail when a run crosses an hour boundary (they read the wall clock's
    hour, theseus-5a50): rerun them mid-hour;
  - theseus-kernel's `children::a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` (theseus-r4hn‡: a count of
    how processes were classed, so its failure is a finding) and theseus-aws-catalog's
    `one_service_decodes_in_under_5_ms` (theseus-rnl3‡: a wall-clock decode bound in a debug build);
  - theseus-discord: tests_outbox's `a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1‡, sj0t‡),
    `a_refused_pin_leaves_the_board_unpinned_and_edited` (theseus-o2tm‡), `live_edits_of_one_message_coalesce_into_the_last`
    and `a_shared_channels_card_goes_to_the_dm_and_nothing_mentions_anyone` (theseus-3ae1, pb3l), and tests_gateway's
    `a_jev_notice_goes_to_the_owners_dm_and_a_press_there_labels_it` (theseus-3ae1);
  - theseusd: `bench_profile::a_first_byte_timeout_is_retried_inside_the_headless_turn`, which saw 2 model requests
    with transient retries off (theseus-jtrc§, a count: keep its output), and
    `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`, which read an empty SIGTERM
    file (theseus-y0lm§).
- A negative assertion ("nothing of X reached Y", "no process is left", "no retry") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. Any failure outside the lists above is yours to explain.

**What main holds.** You clone main at 57f265f2 or later, with **store format 23**. Your clone has v1's milestones, and:
- **batch 8's and batch 9's joins (2026-10-05 and 06):** timing-flakes, telemetry3, cli-tests, history-pages, turn-stack, queue-frames, smalls, wal-mark-skip, crash-hold, durability-on, learning-fixes and discord-live; **soul-import (79be3213): `theseus import openclaw|list|erase`, imported sessions with their provenance, erase by tag, recall's provenance; store format 23**; the bench stack, voice-turns, scrub-escaped, gate-tests, approvals-batch, judge-tests and judge-reads, memory-tests and telemetry-tests; core-waits, route-tests and daemon-proofs (d279767f: the core golden's wake and its offset's sign masked, so it passes in any zone; route's tests proof against load; a `--stdio` daemon's stop that drops its runtime; the lifecycle bench's `cancel` row; the flaky list emptied);
- **batch 10's joins and the local lanes (2026-10-06):**
  - voice-echo and voice-heard (f589d9cb, 4db4cfc0): in a voice call an echo and a closing "yes" are told from a real answer; the next voice turn is told what was heard, cut and never said; replies shaped for speech; a failed voice turn said aloud (theseus-voice engine.rs, heard.rs; theseus-discord runtime/voice/);
  - the docs lane v0.83 (ceba1520, docs only) and the names lane (ea34457e): no person's name in the tree (below);
  - bench-pi (cacad5c4): Pi as the benchmarks' fourth arm, Harbor's own Pi pinned at 1.0.4, measured as the other arms are (bench/harbor/pi_agent.py, pi_atif.py, efficiency.py; the report, recall and async benches);
  - imported-skip (5ac5c23c): the session lists read live sessions by key and never decode an imported record, and a page steps over the import's births in one walk (core store.rs's `live_sessions`, rpc/pages.rs, rpc/methods.rs; theseus-store index.rs);
  - the bench-reports lane (351680a1): docs/benchmarks/ holds a report for every benchmark run, with bench/report's charts, statistics and drafting tool (bench/report/charts.py, stats.py, draft.py);
  - judge-sink with judge-turn-cost (938a8dc8): the judge's sink writes its frames only between turns, with one clock a backlog pass and a busy-bound guard (480 s), a clean stop writes every settled judgment before the store closes, no judged point reads the ladder or the lineage (a warm read does), and `theseus-sim bench turn --judge` measures the judge's cost on a turn (core judge/sink.rs, judge/mod.rs, judge/ladder/, memory_pass/turns.rs's between-turns guard; theseus-sim perf/judge.rs).
  - learned-shadow (57f265f2): `pack.promote` warns while a learned version stands in the moved version's place, the prove's window names it, and the learning cut's mark is never ahead of the newest judgment it read (core rpc/packs.rs, rpc/packs_ahead.rs, rpc/learning.rs, rpc/judge_prove.rs, learning/system.rs).

**Other changes in flight.** Batch 10's other branches are reviewed and merged into `main` one at a time on the owner's machine while you work; a few are still being built. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.
- flake-causes: five load flakes at their causes (core term/tests.rs, tests_push.rs, push.rs, outbound.rs, rpc/server.rs, learning/tender.rs; theseus-kernel children.rs and its test; theseus-aws-catalog tests/catalog.rs and Cargo.toml);
- scrub-encodings: twice-escaped JSON, YAML and repr escapes, base64 inside an escaped string (core scrub.rs, scrub/);
- discord-bound and discord-tests: a lane and the renderer forget a dropped turn's messages; the outbox's load flakes, a binding's event loop ended at the stop (theseus-discord courier/, render/, runtime.rs, tests_outbox.rs; core outbox.rs);
- discord-watch: the bindings file's watch retries a failed bind and keeps the start's note (theseus-discord runtime/live.rs, tests_live.rs);
- core-gaps: a failed routed turn counted where it ran, four test gaps (core rpc/methods.rs's `count_failed_turn`, tests_route_keep.rs, tests_route_model.rs, tests_m3.rs, tests_audit.rs, tests_stack.rs);
- budget-upper: a turn's own call reserved on the estimate's upper bound (core turn.rs's reservation lines, fact/turn.rs, recall/adjacency.rs, rpc/mod.rs, the core golden);
- bench-bounds: bench/'s sampler bounds and the recall bench's retraction and overhead checks; it moves the recall plan's default overhead to 13,640 and repins the smoke's digest (bench/harbor/sampler.py and its test, bench/recall/);
- cancel-fast: `execution.cancel`'s waits event-driven and its frames fewer (theseus-kernel job.rs, tree.rs; core cancel.rs, rpc/driver.rs, toolrun/job.rs, toolrun/late.rs; theseus-sim lifecycle.rs's cancel row; theseusd tests/cancel_frames.rs);
- daemon-stops: a `--stdio` daemon stops on the `shutdown` method, a restart in place drops its runtime first, an import or erase stops between frames (theseusd main.rs, stdio.rs; core rpc/import.rs, import/write.rs; theseus-store store.rs's `upgrade_manifest`);
- route-wait: route.v1's verdict without a wait on every message, and the judge's connection warmed before the first (core judge/inbound.rs, a new judge/warm.rs, tests_route*.rs; theseus-judge fake.rs);
- voice-holds and voice-dave: the voice hold and floor bounded; a deaf voice call noticed and rejoined (theseus-voice engine.rs, heard.rs, vad.rs, tests/turns.rs, songbird_io.rs; theseus-discord runtime/voice/; core voice.rs, telemetry; protocol voice.rs, ledger.rs);
- a local lane reworking the cockpit's ship and charts (cockpit/src/ship/, the Economics, SessionDeck, Ship and Speed views).

These are other cloud sessions like you, batch 11, each on its own branch:
- aws-mints: the AWS catalog's three missing credential mints made secret-bearing, and a rule over every operation's output shape that catches the next one (theseus-ye7o; theseus-aws-catalog tables.rs, a new test file and the generator's doc; theseus-core aws/secret.rs and a new aws/ test file);
- sink-fast: the judge sink's staged blobs written before its between-turns guard with their syncs batched, categorize's mark and the budget's block off the turn, and a test of the busy-bound guard (theseus-ehkp; core judge/sink.rs, judge/mod.rs, blobs.rs, judge/categorize.rs, judge/spend.rs, outbox.rs's `flush_judgments`, new tests_sink_*.rs files);
- bench-fair: the benchmark's arms made comparable: reasoning effort medium on every arm, Claude Code pinned, a timed-out agent stopped, Pi's provider failures counted, Pi offline (theseus-n6p5; bench/harbor/, bench/report/, bench/async's tests, bench/README.md, bench/theseus-bench.toml, one assertion in theseusd's tests/bench_profile.rs);
- recall-fair: the recall bench's arms made comparable: Pi planned at its own overhead and offline, effort medium on every arm (theseus-a5we; bench/recall/; after bench-bounds joins);
- daemon-flakes: theseusd's two load reds at their causes (theseus-jtrc; theseusd tests/bench_profile.rs's first-byte test, tests/stops.rs, an accessor on tests/common/model.rs's `FakeModel`, and whatever each cause is);
- imported-walk: the session lists skip the import's key range without visiting it, and imported-skip's untested claims held (theseus-26jo; theseus-store index.rs, store.rs's `Store` trait and its WAL override, lib.rs; core store.rs's `live_sessions`, rpc/methods.rs's `compilation_list`, rpc/tests_imported.rs);
- health-imported: health counts the owner's own sessions apart from imported and erased ones (theseus-revl; core rpc/methods.rs's `health` and `session_totals`, protocol `HealthResult` with a type in import.rs and lib.rs's ceiling, the CLI's health line through a render/ helper, the cockpit's Systems view);
- bench-rows: the turn bench prints each run's wall and its slowest frame, and the lifecycle bench's restore row runs before the cancel row again (theseus-w7dk; theseus-sim perf.rs, lifecycle.rs's `run`; maybe theseus-store store.rs beside `frames_written_here` and turn.rs's trace attrs).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. No batch-11 task expects one: if yours needs it, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,715) and crates/theseus-discord/src/render.rs (3,009); near it, crates/theseus/src/render.rs (3,099 of 3,100), crates/theseus-kernel/src/kernel.rs (3,025 of 3,030), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-core/src/turn.rs (3,496 of 3,523), crates/theseus-discord/src/runtime.rs (3,428 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,837 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's telemetry/tests.rs (2,491) and toolrun.rs (2,418), theseus-sim's kernel_sim.rs (2,465), theseus-store's wal.rs (2,430) and theseus-kernel's tests.rs (2,407). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-bench-fair`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, the owner's machine, accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. The names lane cleared the tree at ea34457e: never bring a name back from memory, an old branch or a fixture you copy.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, docs/benchmarks/, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: the three Harbor arms made comparable: every arm at effort medium, Claude Code's version pinned, a timed-out agent stopped, Pi's provider failures counted, Pi's caps held by tests, and Pi run offline (theseus-n6p5; also theseus-sgpx, theseus-7gir.23, theseus-bpeg, theseus-p6kd, theseus-a5we)

Branch: `cloud/20261006-bench-fair`. Every commit's subject carries the id of the issue it fixes. Deadline for the report:
4 hours after you start.

**Background.** bench/ measures Theseus, Claude Code and Pi through Harbor 0.23 on the same tasks, model and limits
(bench/README.md, "Fair limits"). Pi is the newest arm (bench/harbor/pi_agent.py; if your clone lacks it, stop and
report). Its review found six gaps in the arms' fairness; the next published run waits on them. You change Python,
Markdown and tests under bench/ (bench/'s suites under both Pythons are their gate) and one Rust assertion, which
needs the gate: this task's toml line and that assertion are the exception to the preamble's "leave that file as it
is". Run no benchmark: the maintainer runs it.

**Read first:** bench/README.md (whole); bench/harbor/claude_code_agent.py, pi_agent.py, theseus_agent.py (its
`CancelledError` branch, `theseus_bench.stop_script`), efficiency.py (`ARMS`, `pi_calls`, `pi_end`,
`pi_limits`, `pi_record`, `SCHEMA`) and the arms' tests; bench/report/efficiency.py (`load_trial`, the caps row) and
draft.py (`ENDINGS`); bench/async/async_agents.py (`ClaudeCodeAsync`, `PiAsync`); bench/theseus-bench.toml and
crates/theseusd/tests/bench_profile.rs; Harbor's own agents in its venv: installed/claude_code.py (`reasoning_effort`
is `Cli("--effort", fallback="CLAUDE_CODE_EFFORT_LEVEL")`; `version` picks both install branches) and installed/pi.py
(`thinking` is `Cli("--thinking")`; `run` passes `dict(access.env)` to its exec).

**The six,** each a green commit:

1. **theseus-n6p5: every arm at effort medium.** On Sonnet 5.5 Theseus sends no `output_config.effort`: the bench
   profile sets none (config.rs: "Omitted: the model's default", high for Sonnet 5.5; compiler.rs sends `output_config`
   only when the profile sets `effort` and the catalog entry takes it), while Claude Code 2.1.290 and Pi 1.0.4 send
   medium by their own defaults. Set it explicitly and equally:
   - Theseus: `effort = "medium"` in bench/theseus-bench.toml's `[profiles.bench]`, held by one assertion in
     `a_bench_call_asks_for_the_models_whole_output`: the request carries `output_config.effort` "medium".
   - Claude Code: `MeasuredClaudeCode` defaults `reasoning_effort` to medium (a host's `CLAUDE_CODE_EFFORT_LEVEL` no
     longer picks it); Pi: `MeasuredPi` defaults `thinking` to medium. An explicit `--ak` still wins, as an ablation.
   - Each trial's record names the effort it asked for, under the same key for all three arms; Pi's session log also
     says what ran (`thinking_level_change`, each answer's `providerThinkingLevel`): record that beside it. Add keys
     only, and keep `SCHEMA` (the report rebuilds a record of another schema from its files): old records still read.
   - **Plants:** the toml line removed (the Rust test fails); each arm's default removed (its test fails).
2. **theseus-7gir.23: Claude Code's version pinned.** Harbor installs the latest release unless `version` is set, so
   each run's Claude Code is whatever npm serves that day (b5 ran 2.1.288; 2.1.290's requests were captured). Pin
   it in `MeasuredClaudeCode` at 2.1.290, one named constant the maintainer moves, as
   `pi_agent.PINNED_VERSION` is; `--ak version=` still wins. Test both of Harbor's install branches, as Pi's
   `test_the_install_is_pinned` does. Then p6kd's last item, for every arm: the version as read in the container
   (`get_version_command`) written to its logs and named in the record, so a pin that didn't take shows (Pi's
   `pi --version` reaches only Harbor's debug log today).
3. **theseus-sgpx: a timed-out agent stopped.** At a task's agent timeout Harbor cancels `run`, and its Docker
   environment ends only its `docker compose exec` client. The agent runs on in the container Terminal-Bench's
   verifier shares, spending and changing files while the tests run, unrecorded. Only the Theseus arm stops its
   agent. In `MeasuredPi.run` and `MeasuredClaudeCode.run`, on `asyncio.CancelledError`: a SIGTERM to the agent's
   processes by name (`pi`, `claude`, as `/proc/<pid>/comm` shows them: `efficiency.ARMS`) and to what they started (a
   build left running), a SIGKILL after a short grace, then the sampler's stop, then re-raise. One function both arms
   use, in plain sh over /proc (a task's image may lack pkill), outside sampler.py. **Test** as
   `test_harbors_timeout_still_stops_the_sampler`: the agent's stop, then the sampler's, in order, before the re-raise,
   for both arms and for the async arms, which run them through `super().run` (`ClaudeCodeAsyncRun`, `PiAsyncRun` in
   bench/async/test_driver.py, ASYNC_HARBOR=1). **Plant:** the stop removed: each fails.
4. **theseus-bpeg: Pi's provider failures counted.** Pi's print mode exits 0 when the provider fails, so Harbor
   records no exception, and bench/report's "Trials with an error" row and trials.csv's `error` miss the trial (its
   record's `end` has it). In `load_trial`, when Harbor recorded none and the record's `end.stop_reason` is `error` or
   `aborted`, give the trial an error name of its own, and add it to draft.py's `ENDINGS`. Test beside
   `test_a_pi_arm_counts_its_trials_past_the_others_caps`: the row, the CSV cell, the ending. **Plant:** the `end`
   read removed.
5. **theseus-p6kd: Pi's caps held.** Two of the review's plants pass today. q2: the report's caps row counting
   `over_budget` only (the caps test sets `over_turns` False everywhere): add a trial past `max_turns` alone. q3:
   `pi_record`'s `end` cut to `pi_end(entries)`: add a record built from `pi.txt` alone, no session log (its `end`,
   answers and `over_turns`). And `over_turns` counts every answer, a failed request Pi retried included (each is
   persisted as an answer with `stopReason: "error"`), where Claude Code's `--max-turns` counts no retry: count answers
   whose `stopReason` isn't `error`, keeping the failed ones in `model_calls`, with a test of retried failures (plant:
   every answer counted again). Each test fails on its plant.
6. **theseus-a5we, part 1: Pi offline.** Pi 1.0.4 overlays newer model-catalog data from its project's server unless
   offline, so the pin doesn't pin its prices (the record's dollars are Pi's own `cost.total`), its thinking map or
   its compat flags. First check in Pi 1.0.4's own docs (its npm package, unpacked under /tmp) that offline mode
   leaves model calls alone, and quote it. Then `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1` and `PI_TELEMETRY=0` in
   MeasuredPi's run environment (an `exec_as_agent` override for the run's command is one way; `PiAsync` inherits
   it). Test in `test_the_command_line_model_and_key` (today: the env is the key alone). **Plant:** `PI_OFFLINE`
   dropped.

**Docs:** bench/README.md is bench/'s own, yours to update: the fair-limits rows Thinking (medium on every arm),
Version (Claude Code pinned), A provider's failure (Pi's an error) and A timeout (every agent stopped), and Pi's
offline run.

**Proof, offline:** bench/harbor's, bench/report's and bench/async's suites under the system python3 and Harbor's venv
(async with ASYNC_HARBOR=1 there), before and after; each plant and the test it breaks; theseusd's bench_profile tests
and the gate for step 1. The records the existing fixtures make differ from main's only by the added keys: show it.

**The live check is the maintainer's.** Exact commands in bench/README.md's shape, and what each should show: fix-git
on each of the three arms with the same model and the README's limits (each record's effort medium, Claude Code's
version 2.1.290 and Pi's 1.0.4 as read in the container); then one task with an
`--agent-timeout-multiplier` small enough to time out, on the Claude Code and Pi arms: no `claude` or `pi` process
left while the verifier runs, and no session entry later than the stop's grace.

**Leave alone:** bench-bounds (batch 10, done, not joined): bench/harbor/sampler.py, test_sampler.py and
bench/recall/, where recall-fair (batch 11) sets the recall arms' effort and Pi's offline line;
daemon-flakes (batch 11): bench_profile.rs's first-byte test and the helpers it uses; everything outside bench/ but
that assertion.
