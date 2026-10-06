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
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report. Batch 9's sessions (your siblings, below) are fixing the ones marked *; batch 8's timing-flakes, which joins main about now, fixes those marked †: if your clone holds its commits (their subjects name theseus-cs71, ynia, 1n2y and qjd6), those four don't fail.
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

**What main holds.** You clone main at 4a449460 or later, with store format 22. Your clone has, besides v1's milestones:
- from the last three days: route.v1, the live rerank, security.v3's notices, replay, the ladder (`pack.mode`, `mode_for`, `pack_arm`), the Linux lanes, the refusal fallback; files (every surface accepts any file); speed (streamed Discord replies, a warm Jev connection, a confidence bar per routing mode); memory (retention, activation, consolidation into cited `Synthesis` nodes, tiering's stubs); the task board and the cockpit's tabs; the learning loop and `theseus judge prove`; the kernel's verified tree kills and caught nested locks; kernel-sim's crashes; the store's cut back to the last good sync and synced-only durability; one serialization per notification; `proc.run`'s steps; health's `web:`, 1-hour cache and disk lines; telemetry's error types, and each tool call counted once at its answer;
- **since batch 8 launched (60b43fb6):**
  - route's gaps: a routed session keeps the base it was moved from and follows a change of that base (the live profile, a place's bound profile, a pane's `-P`), and each loop records one `context.compiled` and one `loop.started` (store format 21);
  - situations: a turn's situation is a compiler input, and a check after each compile fails a turn whose context admits what its situation doesn't, enforcing from day one; the precedence line after the persona; testimony headers (the place, a reply's model, a summary's positions); volatile values shown "as of <date>, unverified" (store format 22);
  - the reader rule's tool-marker and same-name closures, and `theseus-index --version`;
  - memory: the adjacency projection's warm build waits between pages while the machine is busy and never past a clean stop (`startup::stop_has_begun`); tests of retention's shadow rule, activation's additions and a stub's kind; a synthesis's leading heading set aside before consolidation's checks and kept off its node, and a cluster rejected for its form proposed again, once;
  - bench/: the sampler's CPU counted once; both async arms measured end to end (the Theseus record from its ledger, the Claude Code arm on `MeasuredClaudeCode`); the recall bench's bulk reads sized by the compiler's rule, plain abstentions admitted, `--stale retracted` as an option, compactions counted by outcome.

**Other changes in flight.** About thirty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Batch 8's, merging into `main` one by one before any batch-9 branch (each reviewed on the owner's machine):
- timing-flakes and telemetry3 (accepted, joining next): four timing tests fixed at their causes (core term/tests.rs, telemetry/tests.rs; theseusd tests/job_approval.rs); the `theseus.cancel` metric, the index tender's gauges and restarts, and two resumed-span tests (core telemetry/, cancel.rs, tender/; one call in theseusd main.rs's `after_serving`);
- turn-stack, queue-frames, smalls: the turn's future boxed at `TurnRunner::run`; a late result's wake in the turn's end frame and a completion's `execution.queued` row (turn.rs, turn/end_step.rs, push.rs, the kernel's kernel.rs, both goldens, kernel-sim); the secret board's settle race, musl's `time_t`, the TUI's message order, a budget question's `loop.ended` (secrets.rs, wake.rs, fact/turn.rs, theseus-tui, theseusd main.rs);
- wal-mark-skip, crash-hold, durability-on: a start skips the WAL directory's sync when a mark vouches (theseus-store wal.rs, store.rs); a crashed call's held reservation booked as spent (theseus-kernel earlier.rs, kernel.rs; theseus-sim's fake model); the durability sessions list only their prefix, and health's durability line in every surface (core aws/durable*, the CLI's render/aws.rs, the cockpit's Systems cards);
- cli-tests, history-pages: health's 1-hour words, `judge prove`'s bytes, `watch`'s last line (crates/theseus cmd.rs and its goldens); `after` and `before` on the ledger and history reads, and a node's short id (the protocol, rpc/methods.rs, rpc/pages.rs, theseus-store's index, the CLI's history);
- learning-fixes: replay's yes-or-no rightness, the audit off the low thread, the prove's learned versions (learning/, rpc/judge_prove.rs);
- discord-live: the bindings file read live, and the courier's maps bounded (theseus-discord);
- soul-import, still being built: `theseus import`, imported sessions with their provenance, erase by tag, and recall's provenance (core import/, node.rs's bodies, recall.rs, rpc/, the index's tender and extract); it bumps the store format.

These are other cloud sessions like you, batch 9, each on its own branch:
- core-waits: four core tests that fail by the machine's speed or clock, fixed at their causes;
- route-tests: route's tests proof against load, a second switch's base, a routed turn's model in its metrics, a switched turn's recall drops;
- daemon-proofs: a stdio daemon's SIGINT stop, the store's page property test, a cancel row in the gate's jobs bench, the MCP sandbox's daemon watch;
- gate-tests: the gate's layers held through the gate (an edit's language-server start, an extension's floor, a batch's floor step, a ceiling's MCP tools, explain's L3 row);
- judge-tests: a reservation's sentences, a slow Jev at the compile point, the memory pass's links in a rerank, inbound and compile judgments' workload class;
- judge-reads: `judge.list`, the notices' brake and the learning ledger read what the answer needs, not all of history;
- judge-turn-cost: a judge-on turn bench, and the ladder's first read off the turn path;
- memory-tests: a search's own build unpaced, a search during a warm build answering at once, a recalled source read by position, the summary's profile and reservation;
- telemetry-tests: the daemon's telemetry path and the index gauges' changes held by tests;
- scrub-escaped: the scrubber matches a secret printed JSON-escaped;
- approvals-batch: a declined call ends its batch's waits, and a repeated tool-use id no longer reads as answered;
- bench-recall-plan: the recall bench's retraction rule and its plan's room for overhead;
- bench-hygiene: the async record's missing spend and calls, and bench tests that leave nothing behind and hold under load.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (22 on main today; soul-import takes the next at its merge), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,727) and crates/theseus-discord/src/render.rs (3,001); near it, crates/theseus/src/render.rs (3,098 of 3,100), crates/theseus-core/src/turn.rs (3,461 of 3,523), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-kernel/src/kernel.rs (3,008 of 3,030), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-discord/src/runtime.rs (3,453 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,799 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's toolrun.rs (2,494) and telemetry/tests.rs (2,484), theseus-sim's kernel_sim.rs (2,451), theseus-kernel's tests.rs (2,381) and theseus-store's wal.rs (2,352). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-memory-tests`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: a search's adjacency build held unpaced and never queued behind the warm build, a recalled source read by position after a restart, and the compaction summary's model and reservation (theseus-e21m; also theseus-6fn.14, theseus-x875, theseus-6fn.9, theseus-6fn.8)

Branch: `cloud/20261005-memory-tests`. Every commit's subject carries the id of the issue it fixes. Deadline for the
report: 4 hours after you start.

**Background.** Five follow-ups from M6's memory reviews, all in crates/theseus-core. Main paces activation's warm
adjacency build by the machine's pressure, never past the start of a clean stop (`adjacency::pace`). Two items are
searches beside that build, one a recall test a cache masks, two the compaction summary's call. In e21m, x875 and 6fn.9
the code is right and a planted revert passes every test: each new test must fail on its plant.

**Read first:** theseus-core's AGENTS.md; recall/activation.rs (`Adjacent`, `Ask::run`, `Memory::activated`);
recall/adjacency.rs (`pace`, `PACES`, `PAGE`); startup.rs (`stop_has_begun`, `stop_began`); theseus-store's
pressure.rs; tests_activation_pace.rs whole; tests_activation_arm.rs (`kestrel`); rpc/memory.rs (`memory_search`);
recall/render.rs (`source`); tests_recall_node.rs and tests_recall.rs (`rig_built`); turn/compaction.rs (`own`,
`plan_summary`); compiler.rs (`Estimate`); tests_compaction.rs whole (`settled_as_reserved`).

**What the code says** (the code wins; report each difference):
- **Nothing holds the search's unpaced build.** `Ask::run` builds with `build(&self.store, false)`. The pace test calls
  `Adjacent::build(&store, false)` itself, so it holds the flag, not the search's choice of it: with `true` in
  `Ask::run`, every activation test passes. `PACES` is thread-local, and a search's build runs on the blocking pool
  (`spawn_blocking` in `Memory::activated`), where the test thread can't count it.
- **A search queues behind the warm build.** `build` holds the projection's mutex for the whole build, paces included.
  A search (`build: true`) that finds the projection unbuilt calls `build` and waits on that mutex; `activated` gives up
  at its deadline (the larger of `recall_deadline_ms` and `SEARCH_DEADLINE`, 2 s) and answers `deadline`, while its
  blocking thread still waits. A turn answers `building` at once. Only `warm` sets `building`.
- **The source cache masks a read by id.** `source()` answers from `Memory`'s `sources` cache (by node id) before it
  reads the store by position. The x875 test runs both turns in one Core, so its second render hits the cache, and a
  `source()` that reads the latest record by id passes. A restarted daemon's cache is empty.
- **The summary's model.** Under `summary_profile = "session"`, the summary's target is the turn's own (`own` is
  `t.target`), so a one-message override (`turn.submit`'s `provider` and `model`) summarizes on the override.
- **The summary's reservation** is `reserve_micros(max_tokens, est.tokens)`, with the target's `max_tokens` capped at
  `SUMMARY_MAX_TOKENS` (4,096; the issue says 2,000). `tokens` has no margin; `upper` adds `MARGIN_PERCENT` (40 %) to
  the estimated part, and a summary's request is all estimated. A provider that counts 25 % over the estimate and writes
  most of its output settles above the reservation. The turn's own call reserves on `est.tokens` too (turn.rs, the
  `reserve` before its dispatch).

**What to build,** each a green commit:
1. **theseus-e21m: a search's own build never paces.** A test that runs `memory_search` under `+activation` on a store
   of `PAGE` + 1 nodes with the projection unbuilt, and shows the search's build did not pace. Either count paces on
   the projection (a count on `Adjacent` that every build adds to, which a blocking-pool thread reaches), or run it in
   the pace test's namespace with `/proc/pressure` faked busy and assert the search answered `ran` well inside `BOUND`.
   The count runs on any VM; the namespace only where it can be made (see 2). Say which you chose. Plant:
   `build(&self.store, true)` in `Ask::run`; the test fails.
2. **theseus-6fn.14: a search during the warm build answers `building` at once.** In `Ask::run`'s wait, a search that
   finds a warm build holding the projection returns `building` without waiting on the mutex, and the manifest's
   activation row says why. Use `building()`, or a `try_lock` that tells the warm build's hold from a turn's short
   refresh; say which. Test: run it as tests_activation_pace.rs's namespaced test runs. That test reruns its own binary
   under `unshare` with fake pressure files bound over `/proc/pressure`, saying busy. In that run, a store of
   2 × `PAGE` + 1 nodes and `warm` waiting in its first pace; a search under `+activation` answers `building` well under
   its deadline (state your bound and why). Then `stop_began()` ends the build, and a search after it answers `ran`.
   Plant: the check removed, so the search waits on the mutex again; the test fails at `deadline`. If the namespace
   can't be made here (the test prints `skipped`), say so, and hold the build by a hook the test controls too.
3. **theseus-x875: a recalled source read by position after a restart.** In
   `the_next_request_begins_with_the_previous_requests_bytes`, run the second turn on a fresh Core over the same store,
   or clear the source cache; say which, and why. Assert the prefix bytes again. Plant: `source()` reading the latest
   record by id alone (`store.get_node`); the test fails.
4. **theseus-6fn.9: the summary follows a one-message override.** In tests_compaction.rs (the session's provider, and
   `glm` on `zai`), `summary_profile = "session"`. A compacting turn submitted with a one-message override to glm's
   model sends the summary call to glm. The Summary node, its header and the `context.compacted` row name the
   override's model. The session's next turn, with no override, runs on its own model again. Plant: `own` resolved
   from the session's own profile instead of `t.target`; the test fails.
5. **theseus-6fn.8: the summary reserved on the estimate's upper bound.** `reserve_micros(max_tokens, est.upper)` in
   `plan_summary`, with a comment saying why. Test: a summary provider whose usage counts its input 25 % over the
   estimate's tokens and its output at `max_tokens`. Then settled ≤ reserved, read from the row's `reserved_micros`
   and `settled_micros` as `settled_as_reserved` reads them. Plant: `est.tokens` again; the test fails. Report, with
   numbers, whether the turn's own call should take `upper`; change nothing in turn.rs (three siblings edit it).

**Proof, offline:** each new test fails on its plant and passes restored (quote each failure); every activation,
recall, recall-node, retention and compaction test; tests_activation_pace and your new tests 5 times under load (their
waits are their subject: give the durations); theseus-core's suite whole.

**The live check is the maintainer's.** A scratch daemon: Discord and the web off, the stand-in model
(`theseus-sim fake-model --rules`), and a store from `theseus-sim synth-store` (10,000 nodes). theseusd runs inside
`unshare` with `/proc/pressure` faked busy, as tests_activation_pace.rs fakes it:
1. `[memory] mode = "canary"`, `arm = "+activation"`. While the warm build waits (its "adjacency projection is built"
   log line not yet written), `theseus memory search --arm +activation "<a word the store holds>"` answers at once with
   activation `building`. On main it answered `deadline` after 2 s.
2. `arm = "baseline"`, so no warm build. The same search answers `ran` in under a second, and the build's log line
   says `waited_ms=0`.
3. Two profiles, a session that compacts (a small `context_window`), `theseus ask -m <the other's model>`: the
   `context.compacted` row names that model, and its `reserved_micros` ≥ `settled_micros`.

**Leave alone:** soul-import, still running: recall.rs, recall/render.rs (your plant only, restored),
memory_pass/mod.rs, and activation.rs's `hit_of` arms; keep activation.rs edits to `Adjacent` and `Ask::run`. Also
turn.rs (turn-stack, queue-frames and smalls edit it: report only) and stub.rs.
