<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

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
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n*). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report. Batch 9's branches, under review on the owner's machine and joining main today, fix the ones marked *; batch 8's timing-flakes is on main and fixed those marked † (their subjects name theseus-cs71, ynia, 1n2y and qjd6), so those four don't fail on your clone.
  - theseus-core: the output golden's 30 s wait for "wake due" under CPU starvation (theseus-23wh*);
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb*);
    `tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`'s 3 s bound (theseus-vbju*);
    `tests_route`'s verdicts that come `late` under starvation (theseus-biy3*); `tests_lsp_edits::the_block_adds_no_frame`
    (theseus-xx6w*); `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia†: typed-ahead input can
    land on the prompt line; it fails alone too, about one run in two on this VM) and
    `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y†); `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is`
    (theseus-qjd6†: the exporter's retry counted as a fourth trace);
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs*);
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `job_approval::a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too` (theseus-cs71†: a 1.5 s wall
    bound on the cancel's round trip).
- A negative assertion ("nothing of X reached Y", "no process is left") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: theseusd's stop on a SIGTERM or a SIGINT, theseus-xbtr*, and a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at 79be3213 or later, with **store format 23**. Your clone has, besides v1's milestones and batch 9's base (4a449460: route's gaps, situations, the reader rule's closures, memory's paced warm build and consolidation's heading rule, and bench/'s measured arms):
- **batch 8's joins (2026-10-05 and 06):**
  - timing-flakes and telemetry3: four timing tests fixed at their causes; the `theseus.cancel` metric, the index tender's gauges and restarts;
  - cli-tests and history-pages: health's 1-hour words, `judge prove`'s bytes, `watch`'s last line; `after` and `before` on the history reads, and a node's short id;
  - turn-stack, queue-frames and smalls: the turn's future boxed at `TurnRunner::run`; a late result's wake in its turn's end frame and a completion's `execution.queued` row; the secrets board's settle race, musl's `time_t`, the TUI's message order, a budget question's `loop.ended`;
  - wal-mark-skip, crash-hold and durability-on: a start skips the WAL directory's sync when a mark vouches; a crashed call's held reservation booked as spent; the durability sessions list only their prefix, and health's durability line in every surface;
  - **soul-import (79be3213): `theseus import openclaw|list|erase`, imported sessions with their provenance, erase by tag, recall's provenance; store format 23** (core import/, node.rs's bodies, recall.rs, rpc/import.rs, the index's tender and extract);
- batch 9's bench stack: the async record's spend and calls, the recall bench's retraction rule and its overhead plan (bench/ only).

**Other changes in flight.** About fifteen other changes are reviewed and merging into `main` while you work, one at a time on the owner's machine. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.
- learning-fixes (batch 8): replay's yes-or-no rightness, the audit off the low thread, the prove's learned versions (learning/, rpc/judge_prove.rs);
- discord-live (batch 8): the bindings file read live, the courier's maps bounded (theseus-discord runtime.rs, runtime/live.rs, courier.rs);
- voice-turns (batch 9): a barge-in held until words decide, the floor, `Cut` and `Resumed` (theseus-voice, and the pump in theseus-discord's runtime/voice.rs);
- scrub-escaped, gate-tests, approvals-batch (batch 9): the scrubber matches a secret printed JSON-escaped; the gate's layers held by tests; a declined call ends its batch's waits, and a repeated tool-use id no longer reads as answered (core secrets/scrub, toolrun/, approvals; the core golden moves 74 lines);
- judge-tests, judge-reads, judge-turn-cost (batch 9): judge tests, `judge.list` and the notices' brake reading only what they need, a judge-on turn bench and the ladder's first read off the turn path (theseus-judge, core routing and judge/);
- core-waits, route-tests, daemon-proofs (batch 9): four core tests fixed at their causes (the golden's wake, its UTC offset mask), route's tests proof against load, a stdio daemon's stop as `drop(rt)`, the lifecycle bench's `cancel` phase;
- memory-tests, telemetry-tests (batch 9): memory search and telemetry tests.

These are other cloud sessions like you, batch 10, each on its own branch:
- imported-skip: session lists skip imported sessions without reading them (theseus-7087);
- voice-heard: the next voice turn says what was heard, cut and never said; replies shaped for speech; a failed voice turn said aloud;
- voice-echo: an echo and a closing "yes" told apart from a real answer in a voice call.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,715) and crates/theseus-discord/src/render.rs (3,001); near it, crates/theseus/src/render.rs (3,099 of 3,100), crates/theseus-kernel/src/kernel.rs (3,025 of 3,030), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-core/src/turn.rs (3,496 of 3,523), crates/theseus-discord/src/runtime.rs (3,453 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,800 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's toolrun.rs (2,494) and telemetry/tests.rs (2,491), theseus-sim's kernel_sim.rs (2,465), theseus-store's wal.rs (2,430) and theseus-kernel's tests.rs (2,384). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-judge-sink`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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
## Your task: judge-turn-cost's sink, fixed: drain a backlog between turns without stalling it, and flush at a stop (theseus-s1am, theseus-ych4; also theseus-3bl9)

Branch: `cloud/20261006-judge-sink`. **It starts from judge-turn-cost's head 54de79da**, not today's main. That is
branch `cloud/20261005-judge-turn-cost`, cut from main 4a449460 at store format 22. So the preamble's "What main holds"
is not your base: don't merge or rebase onto main. Your commits go on top of judge-turn-cost's, and the maintainer
merges the whole branch into main.

Every commit's subject carries its issue id. Deadline for the report: 3 hours after you start.

**Background.** judge-turn-cost (theseus-0j2.8, theseus-289c) made three changes:
- it added a turn bench with the judges on (`bench turn --judge`);
- it moved the judge sink's writes between turns, the FAST win: no judge frame before an answer;
- it took the ladder's first read off the turn path (289c).

The maintainer's reviewer accepted the bench and 289c, and found two defects in the sink, both seen live against main's
sink:
- **theseus-s1am (P2): the sink's between-turns clock restarts for every 32-row frame.**
  `between(Instant::now(), …)` per batch measures each frame's bounds from a fresh clock, where the pass should keep
  one clock for a backlog. So a busy daemon writes 32 judgments per 120 s, or 600 s beside a turn, while the backlog
  grows. Live: 32 rows written of 1,551 settled after 180 s of busy turns; main's sink wrote 501+ (labels 8 against
  895).
- **theseus-ych4 (P2): a stop doesn't flush the sink.** A settled judgment was gone after a restart, where main kept
  it. Every settled judgment must be written before the store closes, on the one clean-stop path (theseusd's AGENTS.md:
  "every clean stop is one path").
- **theseus-3bl9 (P3): the adoptions test can't tell a between-turns wait from a sleep.** `tests_ladder_unread`'s test
  looks at 300 ms, and the warm read sleeps 500 ms first. So a plant that drops the warm read's between-turns wait
  isn't caught.

**Read first:**
- the root AGENTS.md (FAST, the store's version rule) and theseus-core's AGENTS.md (the judge paragraph), and
  theseusd's on the stop;
- the branch's own CLOUD_REPORT.md, the commit "cloud report (not for main)" (judge-turn-cost's report: what step 2
  built and why);
- the sink, and where it calls `between` per batch;
- how the core's other tenders flush at a clean stop;
- `tests_ladder_unread.rs`;
- the judge families' tests.

**What to build,** each a green commit:
1. **s1am: one clock per backlog pass.**
   - A pass measures its between-turns bounds from its own start, or from the last turn's end, not from each frame.
     It drains a backlog at about main's rate when no turn runs, and yields to turns as designed. Keep the 32-row
     frame size or change it, and say why.
   - **Test:** about 1,500 settled judgments, with turns starting and ending through the pass. The backlog drains
     within a stated bound near main's sink, and no frame lands inside a turn's window between its start and its
     answer.
2. **ych4: a clean stop flushes the sink.** Every settled judgment still in the sink is written before the store
   closes, in as few frames as it takes, on the same stop path. **Test:** settle N judgments, stop cleanly, restart:
   all N are there. State what a SIGKILL loses, which is acceptable: say so, don't fix it.
3. **3bl9: an adoptions test that tells a wait from a sleep.** Hold a turn running across the warm read's 500 ms
   sleep, and show the read waits for that turn's end.
4. **Keep "between turns".** The branch's no-frame-before-the-answer test stays green.

**Proof, offline:**
- the new tests, the judge families, theseus-core's suite and the whole workspace suite once (`TZ=America/Phoenix`);
- `bench turn --judge --runs 30` on a release-thin build: judge frames before the first answer must be 0, with the p50s
  reported;
- planted reverts, each naming the test it breaks:
  - the clock reset per frame again: the backlog test fails;
  - the stop's flush removed: the restart test fails;
  - the adoptions wait replaced by a 500 ms sleep: the strengthened test fails;
  - a judge frame written before the answer: the branch's test fails.

**FAST:** report the turn bench's frames and p50s with judges on, and the time a stop adds with N judgments pending.
Nothing goes on the start path.

**The live check is the maintainer's.** Give exact commands and what each should show:
- a scratch daemon with the judges on and the stand-in model, busy with turns for 3 minutes: judgments settled
  against written, read the reviewer's way (`judge log`, the label counts);
- a stop and restart with judgments pending: all present.

**Leave alone:** everything outside the judge sink, its stop path and the two tests. At the merge the maintainer
resolves theseus-core AGENTS.md's judge paragraph with judge-reads' text (a resolve.py exists). Don't edit it beyond
what your change needs, and say what you changed.

Its siblings, judge-tests and judge-reads, join main today without judge-turn-cost. route-tests (B9-core) conflicts
with judge-turn-cost in `tests_route.rs`'s rig; a resolve.py exists for that too.
