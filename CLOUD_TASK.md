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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-imported-skip`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: session lists skip imported sessions without reading them (theseus-7087)

Branch: `cloud/20261006-imported-skip`. Every commit's subject carries `theseus-7087`. Deadline for the report: 3 hours
after you start.

**Background.** soul-import (on main at 79be3213, store format 23) imports the owner's earlier assistant history as
**imported sessions**: `theseus import openclaw|list|erase`, `crates/theseus-core/src/import/` (`mod.rs`'s
`is_imported`, `write.rs`, `episode.rs`), `rpc/import.rs`. Imported sessions are kept out of `session.list`'s answer,
but both of its paths still read past every one of them. The owner's import will hold **21,151 imported sessions**.
The maintainer's reviewer measured, on a scratch store from a debug build (BM25 only, one real session before the
import), with the CLI's `theseus sessions` (the whole list) and `session.list {"n": 20}`, five calls each:
- before the import: the whole list p50 23.3 ms (max 35.9), a page of 20 p50 19.7 ms (max 21.3);
- after importing 21,151 sessions: the whole list p50 **501.0 ms** (max 1,555.6), a page of 20 p50 **210.0 ms** (max
  212.3). `health` was unchanged: it counts keys.

A release build is faster, but the ratio holds. FAST is a primary goal of this project, and the callers are hot:
- the whole list: the cockpit's ship view every 3 s and its money view every 5 s (`cockpit/src/ship/useShipData.ts`,
  `views/Money.tsx`); Discord's `/status`, `/stop` and `/trust`, and each place's start (`theseus-discord`'s
  `session_info`); the CLI's `theseus sessions` and its interactive start; herdr sync;
- the pages: the TUI's sidebar.

**What reads past them today:**
- **The whole list** (`Core::session_list`, `crates/theseus-core/src/rpc/methods.rs`): `sessions_by_activity()` decodes
  every session record with `store.list_sessions()`, then `.filter(|r| r.imported.is_none())` drops the imported ones:
  21,151 records read and decoded per call, each dropped. `methods.rs` has another `list_sessions()` read (search for
  it); `rpc/confirms.rs` (`confirm.list`) walks `sessions_by_activity()`; the learning tender's `task_briefs`
  (`learning/system.rs`) reads `list_sessions()` whole.
- **The pages** (`sessions_paged`, `rpc/pages.rs`): imported sessions are skipped by key, undecoded, but the walk steps
  over them `n` at a time (`newest_keys(kinds::SESSION, cursor, n)` in a loop). An import writes the newest births, so
  any page that reaches back past the import walks every imported key in steps of `n` (21,151 / 20 = about 1,060
  reads for a page of 20).

**Read first:** the root AGENTS.md (FAST, the store's version rule, the reader rule); theseus-core's and
theseus-store's AGENTS.md; `import/mod.rs` and `write.rs` (how an imported session's key and birth are written, and
what `is_imported` tests); `rpc/methods.rs` (`sessions_by_activity`, `session_list`), `rpc/pages.rs`
(`sessions_paged`), `rpc/confirms.rs`; theseus-store's `newest_keys` and the SESSION kind's births; `import/tests.rs`
(how the tests build an import).

**What to build,** each a green commit:
1. **The whole list and every whole read skip imported sessions by key, before decoding.** An imported session's key
   is its id, and `is_imported` is a string test. Give the store (or the core's store wrapper) a read that returns the
   session records whose keys pass a predicate, decoding only those; use it in `sessions_by_activity`, the other
   `methods.rs` read, `confirm.list` and `task_briefs`, each with the predicate its answer needs (most want live
   sessions only; keep any that must see imported ones, and say which and why).
2. **The pages step over the imported run in one read, not `n` at a time.** Either a range read past the run, or keep
   imported sessions' births out of the SESSION kind's births (their own key space or kind), so neither path ever meets
   them. Choose the smaller change that keeps the store at format 23 if you can. If yours must change a stored record
   or kind, bump `MANIFEST_FORMAT` by one from main's number as you cloned it (23 today), with the old layout's sample
   in `tests_layouts.rs`, as the rule says, and say so in the commit body. The answers must not change: the same
   sessions, in the same order, with the same cursors, for any store with or without an import.
3. **Tests**, each holding a behaviour:
   - after an import of a few thousand synthetic sessions (build them the way `import/tests.rs` does), the whole list
     and a page of 20 read **no imported record**: count decodes (a counter the test can read), or make the imported
     payloads unreadable while their keys stay (as theseus-gk93 proposes for `keys_ending`) and show the reads still
     answer;
   - a page's walk past the imported run costs a bounded number of reads, not one per `n` imported keys;
   - the answers before and after are equal: the same sessions, order and cursors, with live sessions born before,
     between and after the imported run;
   - an erase still hides, and a re-import of an erased tag is still refused (the import's existing tests, passing).

**Proof, offline:** the new tests; theseus-core's `rpc::`, `import::` and `learning::` tests; theseus-store's; the
whole workspace suite once (`TZ=America/Phoenix`). Planted reverts, each naming the test it breaks:
- decode-then-filter back in `sessions_by_activity`: the no-imported-decode test fails;
- the page walk stepping `n` at a time again: the bounded-reads test fails;
- the predicate inverted in one caller (it lists only imported sessions): the equal-answers test fails;
- `confirm.list` back on the whole decode: its own test fails (write one if none would).

**FAST: measure it.** In a release build, on a scratch store with 21,151 synthetic imported sessions and a handful of
live ones: the whole list and a page of 20, p50 and max over 20 calls each, before your change (main) and after.
Report both, and the time to import the synthetic set. Nothing here is on the start path or a turn's path: say so with
the code paths; the lifecycle bench is untouched.

**The live check is the maintainer's.** Give exact commands:
- a small script, in the report or as an ignored test helper, that writes N synthetic episodes in the import's input
  format (invented names and text only);
- a scratch daemon on a fresh state dir, `theseus import openclaw <file>`, then `theseus sessions` and
  `session.list {"n": 20}` timed 20 times each, and `theseus import erase <tag>` with the same timings after it;
- what each should show.

**Docs:** don't edit `docs/`. In the report, give the text for any spec or design item that says session lists read
every session.

**Leave alone (siblings in flight):** learning-fixes (`learning/`; your `task_briefs` edit should be the one read: keep
it small), discord-live (`theseus-discord`'s runtime), approvals-batch and gate-tests (`toolrun/`, approvals, the core
golden), the judge stack (`theseus-judge`, routing), core-waits and route-tests (core tests), daemon-proofs (theseusd's
stdio), voice-turns, voice-heard and voice-echo (`theseus-voice`, `theseus-discord`'s `runtime/voice.rs`).
