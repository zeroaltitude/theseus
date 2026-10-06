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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-gate-tests`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the gate's layers held through the gate itself: an edit's language-server start, an extension's load floor, a batch's floor step, a ceiling's MCP servers and explain's L3 row, and a frame count that waits for its event (theseus-x1jj; also theseus-xx6w, theseus-sh9w, theseus-grms, theseus-cxqj, theseus-t2xr)

Branch: `cloud/20261005-gate-tests`. Every commit's subject carries the id of the issue it fixes. Deadline for the
report: 4 hours after you start.

**Background.** The tool gate runs each call through its layers in one order (`ToolRuntime::order`,
toolrun/order.rs): the call's policy, a granted secret, a language server's start (L2 by an `lsp.*` call, L3 by an
edit), a shared place's word, the place's floor and an extension's load floor, a glide's rule, T1's hold, an MCP
client's floor. Reviews planted reverts that removed one layer's line from that order, and no test failed: each
layer's test calls the layer's own function. Every test here runs a turn (or the gate's `order`), so a dropped line
fails it. **Check each item against main first:** if a later change closed one, say so and drop it.

**Read first:** theseus-core's AGENTS.md; toolrun/order.rs whole; toolrun/batch.rs (`stricter`); toolrun.rs
(`definitions_for`, `mcp_note`; read only); lsp/edits.rs (`gate`, `EDITS`); extend/load.rs (`Floors`) and
extend/tests_load.rs (its rig, `an_acked_extension_is_offered_from_the_next_turn_never_mid_turn`,
`its_posture_is_the_policy_lines_and_its_loads_ceiling_floors_it`); rpc/explain.rs (`layer_row`, `lsp_condition`);
tests_lsp_edits.rs, tests_steps.rs, tests_explain.rs, tests_ceilings.rs, and mcp/tests.rs's rig.

**What to build,** each a green commit:
1. **theseus-x1jj: an edit's server start (L3), through the gate.** `lsp::edits::gate` judges an `fs.write`,
   `fs.edit`, `fs.patch` or `lsp.rename` that starts its root's language server, in a private place, at `proc.run`'s
   posture. Its test (`tests_lsp_edits::an_edit_that_starts_its_server_is_judged_as_proc_run`) calls that function
   directly, and `start_on_edit_starts_the_server_and_health_counts_the_block` runs a turn whose posture doesn't
   change its outcome. Test: `start_on_edit` on, no server up, `proc.run` on approve and `fs.edit` open: a turn's
   `fs.edit` waits for approval, its reason naming the start (`starts fake on`); in a shared place it keeps its own
   posture. **Plant:** L3's line in order.rs removed.
2. **theseus-xx6w: count the turn's frames, not a quiet store's.** `tests_lsp_edits::the_block_adds_no_frame` compares
   a turn's frames with an edit's diagnostics block and without. After `open()` starts the server it waits for 100 ms
   with no new frame, but the server's `lsp.ready` row lands after the call returns (`lsp::readiness`), and under load
   it landed inside the counted turn ("with the block 10 frames, without 9"). A frame inside the measured turn is a
   finding, not a flake: reproduce it under the load recipe, and say which record it was (print the counted frames'
   kinds on a failure). Then wait for that row itself (or the board's ready state), or count only the turn's own
   frames: no stillness wait. 30 runs under load pass. **Plant:** the block written in a frame of its own (an extra
   append where the block is attached: name it): the test fails.
3. **theseus-sh9w: an extension's load floor, through the gate.** An extension's tools run no looser than the floor of
   the ceiling it was loaded under (`extend::load::Floors`, the line after the place's floor in order.rs). Its test
   calls `Floors::floor` on a `Floors` of its own. Test, with tests_load's rig: an extension loaded under a ceiling
   whose `posture_floor` is `approve`; a turn in another private place whose config says open for `ext-<name>`
   (`[policy.mcp]`): its call waits for approval, the reason naming the loading place; after a revoke and a new load
   with no ceiling, it runs. **Plant:** the `Floors` line removed from order.rs.
4. **theseus-grms: a batch's floor step outranks an earlier listed step.** The batch gate takes the strictest step by
   `(floor, posture)`: the floor outranks an `approve_argv` entry at the same posture, and a tie keeps the earlier
   step. `tests_steps::a_step_on_the_floor_makes_the_batch_wait_as_the_floor` puts the floor step after an open one,
   so posture alone picks it. Test, beside it in tests_steps.rs: a batch whose first step is on `approve_argv` and
   whose second is on the floor (`op whoami`): posture approve, `floor` true, the reason starting "step 2 of 2 (`op
   whoami`): ", and nothing ran. **Plant:** `stricter` comparing `a.posture > b.posture` alone.
5. **theseus-cxqj: a ceiling's `mcp:<server>`.** `definitions_for` keeps an MCP server's tools only where the place is
   offered them (the place rule narrowed by its ceiling), and `mcp_note` names only those servers. A review's probe
   proved it and was not kept. Test, over mcp/tests.rs's rig, with a bindings file: `#lab`'s ceiling `tools =
   ["mcp:fake"]`, `#den`'s `tools = ["web"]`. `#lab` is offered the fake's five tools alone, its tools note has the
   MCP line, and its `echo` runs. `#den` is offered `web_search` alone, has no MCP line, and its `echo` is refused
   "Not run: mcp:fake/echo is not offered in #den: its ceiling in the bindings file offers only web". **Plants:**
   `definitions_for`'s MCP filter ignoring the ceiling; `mcp_note` ignoring it: each fails the test.
6. **theseus-t2xr: explain's L3 row.** `policy.explain` (rpc/explain.rs) lists each layer's row and the conditions that
   can change a call's posture. `layer_row` gives an `lsp` row only for `lsp.*` tools, and the `lsp_start` condition
   is added only for them, so for an edit an L3 raise changes the explained result with nothing naming why. Add, for
   the tools in `lsp::edits::EDITS`, a row when L3 raised the posture and a condition when a server with
   `start_on_edit` is configured with `[lsp] edit_diagnostics` on. Test: `fs.edit` explained with such a server shows
   both, and without `start_on_edit` shows neither. **Plant:** the new row's arm removed.

New tests go in files of their own (for example tests_gate_layers.rs, its `mod` line in lib.rs among the other
`tests_` modules), except grms's, beside its sibling, and xx6w's fix. toolrun.rs is 2,494 lines, and an unlisted file
fails past 2,500: change nothing there. t2xr's code goes in rpc/explain.rs.

**Proof, offline:** each new test fails on its plant and passes restored (quote each failure; restore with a fresh
mtime, then `git status`); xx6w's failures before and its 30 runs after, under load; every `tests_lsp_edits`,
`extend::`, `mcp::`, `tests_steps`, `tests_explain` and `tests_ceilings` test; each new test 5 times under load;
theseus-core's suite whole.

**The live check is the maintainer's.** A scratch daemon (Discord and the web off, the stand-in model `theseus-sim
fake-model`), with a bindings file of two private places:
1. A language server with `start_on_edit = true` and `[lsp] edit_diagnostics` on, none running, `proc.run` on
   approve: an edit in a private place asks, naming the start; `theseus policy explain --tool fs.edit` shows the L3
   row and its condition.
2. One place's ceiling `tools = ["mcp:fake"]` (`theseus-sim fake-mcp` as the server), the other's `tools = ["web"]`:
   the first is offered the fake's tools, the second none, and its call to one is refused, naming its ceiling.
3. A `proc.run` batch whose first step is on `approve_argv` and whose second is `op whoami`: the card says floor and
   names step 2.

**Leave alone:**
- approvals-batch (batch 9, a sibling): a response's batch of waiting calls and its resume, in toolrun.rs and
  toolrun/: plants only there, each restored;
- daemon-proofs (batch 9): theseusd's tests and theseus-sim;
- the gate's and the layers' code, except t2xr's explain change and plants you restore.
