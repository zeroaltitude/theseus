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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-sink-fast`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the judge's writes kept off the turns beside them: a frame's blobs written before its between-turns guard with their syncs batched, a stop's flush in big frames, categorize's mark and the budget's block out of turns, and the busy-bound guard held by a test (theseus-ehkp; also theseus-xkbs, theseus-ju99)

Branch: `cloud/20261006-sink-fast`. Every commit's subject carries the id of the issue it fixes. Deadline for the report:
5 hours after you start.

**Background.** judge-sink (on main) writes the judge's frames only between turns: the writer in judge/sink.rs takes a
`Writing` guard from `memory_pass::turns::Turns::between` for each frame, and `Turns::begin` waits while any writer
holds one. A review on the owner's machine (fdatasync about 7 ms; scratch daemons on the stand-in model and Jev, every
pack as wired) found:
1. **theseus-ehkp (P2, FAST).** The guard (`_writing` in `run`) covers all of `Queue::write_one`: `JudgeService::write`,
   then `write_staged_blobs` (judge/mod.rs), then `Blobs::put` per staged blob, one at a time (blobs.rs: the file's
   `sync_all`, then its directory's), then the frame's append. A turn that begins mid-frame waits for every blob's two
   syncs. Three sessions of back-to-back turns for 180 s: from the quiet bound (120 s) the backlog drained in the gaps
   and the turn wall p50 per 15 s window was 269 to 280 ms, against 142 to 156 ms with main's earlier sink. A clean
   stop with 2,827 queued took 11,452.8 ms (the `stopping: the judge's settled judgments are written` line; 690 blobs);
   200 queued with no blobs, 7 frames, 72 to 121 ms.
2. **theseus-xkbs (P2, FAST).** categorize.v1 writes its mark (`[meta:judge.categorize.<session>]`, `put_meta` in
   `prepare_categorize`) in a frame of its own, through neither the sink nor `Turns::between`, in a task the turn's end
   spawns. Under load one landed inside the next turn, before its answer (219.9 ms against the arm's p50 125.2). The
   budget's lone block frame (`[meta:judge.budget]`: `JudgeService::reserve` writes it when `ShadowBudget::reserve`
   crosses a $0.01 block) has the same shape.
3. **theseus-ju99 (P3).** No test holds the busy-bound guard, `since`'s floor in sink.rs (the pass's start, but never
   more than `quiet_bound` ago): with `since` returning the pass's start, every sink test passes.

**Read first:** the root AGENTS.md (FAST; "The WAL and frames": a fact lands in one frame with the rows that describe
it); theseus-core's AGENTS.md, the judge paragraph and its theseus-otny bullet (`stage_blob`, `reserve_beside`);
judge/sink.rs whole; memory_pass/turns.rs; judge/mod.rs (`write_staged_blobs`, `stage_blob`, `reserve`,
`write_budget`, `Beside`); judge/spend.rs's doc and `reserve`; judge/categorize.rs (`at_exchange_end`,
`prepare_categorize`, `judge_categorize`, `deciding`); blobs.rs (`put`, `hold_puts`); outbox.rs's `finish_stop` and
`flush_judgments`; tests_sink_backlog.rs, tests_sink_flush.rs, tests_sink_between.rs, tests_categorize.rs; theseus-sim's
perf/judge.rs (where `bench turn --judge` places each judge frame).

**What to build,** each a green commit:
1. **ehkp: the blobs before the guard.** Write a frame's staged blobs before the writer takes `Turns::between` (they
   touch no WAL; each is still durable before the row naming it), so the guard covers the append alone. Two hazards:
   the stop's flush must still find every queued judgment (don't take a batch out of the queue and then wait for a
   gap), and must never append a row naming a blob another thread is still writing (`write_staged_blobs` removes a
   blob from `staged_blobs` before its put). **Test** by order: a frame's blobs held mid-put (`hold_puts`), a turn
   `begin`s meanwhile and runs at once; the frame lands after the blobs and between turns. **Plant:** the blobs back
   inside the guard: the test fails.
2. **ehkp: the syncs batched.** Beside `Blobs::put`, whose other callers (attach.rs, each shadow point's prepare,
   learning/) keep its contract, a write of many blobs: every file written, then synced, then renamed, then the
   directory synced once. Say what a crash at each step leaves.
3. **ehkp: the stop.** Check that no turn runs when `finish_stop` flushes (one was held at the model in the review's
   stop). Then write the queue in bigger frames (check the WAL's limits on a frame first) and its blobs as one batch.
   The files' syncs can go out together from a few threads (the journal commits them together), or as one `syncfs`,
   which flushes every dirty page on the filesystem, a neighbour build's too: measure beside a writer before choosing.
4. **xkbs: the mark and the block off the turns.** The mark: carry it in the sink's frame, a META record beside its
   judgment's row (a crash then loses both together; today a mark can outlive its lost row), or write it between turns
   as `Core::warm_ladder` writes its adoptions (rpc/packs.rs: read it only). Either way the next exchange end must see
   the moved mark at once (`deciding` ends after prepare: a mark read only from the store judges the same messages
   twice), and a skipped or failed judgment must move it too. The block must stay durable before the calls it books
   (theseus-otny); `reserve_beside`'s (route.v1, a live rerank: turns wait on them) stays beside its call. Never wait
   for `Turns::between` in a turn's own task (it counts itself, so it waits for the busy bound): check where each
   `reserve` caller runs. A gate judgment's notice should not wait for its turn's end. One option: the next block
   written ahead, between turns, once the current is half spent; a crash then books up to two blocks' rest (spend.rs's
   doc says one): say so. Do what the code clearly allows; report the rest. **Test:** a turn held running while
   categorize prepares (`tests_categorize.rs`'s ten human messages) and while a loop.v1 judgment needs its first block:
   neither frame lands while it runs (the block: in the cases you cover), each lands after (`frames_appended`), the
   block before the fake Jev sees the call. **Plants,** each failing it: the mark's `put_meta` at prepare; the block
   written at once.
5. **ju99: the guard held.** A new tests file beside the sink's: quiet 500 ms, quiet bound 1 s, busy bound 3 s; one
   turn held 14 s while 8 judgments settle every 100 ms: at most ⌈held / (busy_bound − quiet_bound)⌉ = 7 frames inside
   it (the guard writes 6; without it, 36). **Plant:** `since` returns the pass's start: the test fails.

**Measure, A B B A in one session** (freeze each build by copying `theseusd` and `theseus-sim` out of target/):
- a stop with about 3,000 queued and 700 staged blobs: the stop line's `ms`, the frames, and the blob syncs (a count
  in `Blobs` a test reads, or strace if the VM has it);
- a burst: a scratch daemon with the judge on at the stand-in Jev (`FakeJev`, in-process, as perf/judge.rs starts it),
  every pack as wired, three sessions of back-to-back turns for 180 s: the turn wall p50 per 15 s window, and what a
  beginning turn waited on;
- `theseus-sim bench turn --judge --runs 30` on a release-thin build: judge frames before an answer (0) and the p50s.
This VM syncs in microseconds, so the gain shows in counts more than walls: say so.

**Proof, offline:** each plant; the sink and categorize tests, tests_judge*, `memory_pass::`, theseus-core's suite once
(`TZ=America/Phoenix`). No store-format bump: no record changes, only the frame it rides in (say so if not).

**The live check is the maintainer's,** on the owner's machine, A B B A against main: the 180 s burst (turn p50 per
15 s window), a `shutdown` with a backlog (the stop line's `ms`; after a restart, every judgment present), and
`bench turn --judge --runs 30`. Give exact commands.

**Leave alone:** route-wait (batch 10: judge/inbound.rs, tests_route.rs, tests_route_wait.rs; theseus-judge's fake.rs
and tests.rs: use the fake Jev as it is), learned-shadow (batch 10: rpc/packs.rs, rpc/packs_ahead.rs, rpc/learning.rs,
rpc/judge_prove.rs, learning/system.rs, tests_ladder.rs, tests_learning.rs, tests_prove.rs), budget-upper (turn.rs's
reservation lines, rpc/mod.rs), flake-causes (rpc/server.rs, outbound.rs, push.rs), discord-tests (outbox.rs: keep
yours to `flush_judgments`), bench-rows (batch 11: theseus-sim perf.rs's turn bench; a measure of yours goes in a
module of its own or an ignored test), and theseus-core's AGENTS.md (four branches edit it: one sentence of yours at
most).
