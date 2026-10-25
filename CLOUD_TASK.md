<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3e-fanout-kernel`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`). The
kernel's record schema is `theseus_kernel::types::SCHEMA = 2`. This row adds two optional fields to a stored record
(below). Use `#[serde(default, skip_serializing_if = ...)]` so an old record reads unchanged, and **do not bump
`SCHEMA`**: `terms.rs` marks every execution with `schema < SCHEMA` as `legacy`, so a bump would relabel every stored
execution. Bump `MANIFEST_FORMAT` only if the store's own version rule (root `AGENTS.md`) truly requires it for an
added optional field; if you do, name it and justify it in your report's first section (the integrator renumbers it
at the join; memory of past joins: a format bump moves two goldens).

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines).
`crates/theseus-kernel/src/kernel.rs` is 3,001 of its 3,030 ceiling; `crates/theseus-core/src/turn.rs` is at 3,498 of
3,525: **do not edit `turn.rs` at all in this row.** At the base: `crates/theseus-kernel/src/tasks.rs` 559,
`types.rs` 799, `tests_tasks.rs` 895.

**Your lane: `integ/p3-turn`.** Three rows fired into it earlier tonight, all on the same base, and this row must stay
additive with them. They own these files, and **you edit none of them**:
- `p3c-steer-keep-going` (theseus-ikr75): `crates/theseus-core/src/turn/progress_step.rs`,
  `crates/theseus-core/src/tests_turn_progress.rs`, `crates/theseus-core/src/fact/progress.rs`, and the progress-check
  paragraph of `crates/theseus-core/AGENTS.md`.
- `p3c-clear-by-tokens` (theseus-5lzox): `crates/theseus-core/src/compiler/clearing.rs`,
  `crates/theseus-core/src/compiler/tests_ring.rs`, the compiler goldens and the context bench.
- `p3c-narrow-reads` (theseus-tdkcn): the `deep_coding` hint's injection under `crates/theseus-core/src/turn/`, and
  `crates/theseus-tools/src/fs.rs`; the "One long instruction" paragraph of `crates/theseus-core/AGENTS.md`.

Nothing in this row touches `progress_step.rs` or `clearing.rs`, so it cannot conflict with them at file level. Do not
edit `crates/theseus-core/AGENTS.md`; if the kernel's own `crates/theseus-kernel/AGENTS.md` describes reports, add
there. Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3e-fanout-kernel, the kernel half of tasks fan-out / fan-in (theseus-z9mze.1)

Branch: `cloud/20261024-p3e-fanout-kernel`.

Every commit subject carries `theseus-z9mze.1`. Deadline for the report: about 2 hours after you start.

**Why (the evidence, from a benchmark the owner ran on 2026-10-24).** In a bug hunt (13 planted bugs in a copy of
this workspace), Theseus fixed 4 of 13 while two rival harnesses fixed about 10 each, at half the cost. The rivals ran
5-13 parallel helper agents per trial, each reading one area in a fresh 50-150K-token context, and folded their
findings back in **within the same turn**. Theseus ran none: one context tried to hold the codebase and sawtoothed
between 270K and 430K tokens through 7-8 clearing passes. A Theseus task today cannot help, because its report lands
in the parent's **next** turn (`turn.rs::read_reports` -> `Kernel::take_reports`), and a headless run is one turn.

The whole feature is eight rows (this is row A, the head of the chain): A the kernel (this row); B `task.wait`, an
in-turn wait tool in theseus-core; C `task.fan_out`, which opens 2-6 helpers and waits; then the cue, Ops and the
bench. **This row builds only what B and C stand on**, and changes no behaviour a user sees.

### What to build

**1. `Kernel::take_reports_of(guard, ids, extra)`** in `crates/theseus-kernel/src/tasks.rs`, beside `take_reports`
(read `take_reports` and its doc first, and `turn.rs::read_reports`, which calls it). It takes **only the named
tasks' reports** off the parent's `reports` (and those tasks off `report_wakes`), in **one frame** with the records
`extra` builds from them and one ledger row `task.reports_read` whose detail adds `"via": "wait"` and the taken ids.
- Named tasks that have not ended (not on `reports`) are not taken and not an error: the result says which were taken,
  so row B can tell the running ones apart. When none of the named are on `reports`, nothing is written.
- Every report it does not name stays on `reports` for the parent's next turn, exactly as today.
- A taken report is never delivered again: a later `take_reports` (the next turn's) does not return it.
- Keep the order of `reports` (oldest first) among the taken ids, as `take_reports` does.
- Read how `report_wakes` drives the parent's wake and its `due` term (`tasks.rs` ~318-360, `terms.rs`). If taking the
  only reports that asked for a wake would leave the parent queued for a turn with nothing to read, decide what is
  right (leave the wake, or drop it with its row), test it, and say which in the report.
- Share code with `take_reports` rather than copying it (for example one private fn taking a filter), keeping
  `take_reports`' behaviour and its ledger row byte-for-byte unchanged.

**2. The helper opening.** `Opening` (`tasks.rs` ~69) gains the helper mark, and `carve_task` stores it on the task's
`Execution` (`types.rs` ~322). The design names two fields, `helper: bool` and `fan_out: Option<String>` (the fan-out
call's correlation id, which groups a call's helpers). A helper always belongs to a fan-out, so **one field
`helper: Option<…>` holding the group id is preferred** (half the touch at every literal site); your call, say which.
- serde default and skip-if-empty, so an old record reads as "not a helper" and a non-helper writes no new key.
- An accessor for rows B, C and E (for example `Execution::helper_of() -> Option<&str>`), with a doc comment saying
  what the mark is for: a helper reads and reports, and **at its carve it ends with stop reason `budget` instead of
  asking the budget question** (nobody answers a helper's question in a headless run). That ending is row C's change
  in theseus-core's `turn.rs`, not yours: this row stores the mark and documents it. Plain tasks are unchanged.
- `Execution` has no `Default`, so every struct literal of it breaks when a field is added. Add the field at each
  literal site (kernel, core tests, `theseus-exam`, `theseus-sim`, wherever the compiler names one) with one line
  each, and no other change at those sites. `Opening` derives `Default`: at its literal sites in theseus-core
  (`task.rs`, `draft.rs`, `draft/first_pass.rs`, `rpc/task_start.rs`, `turn/draft_step.rs`) add the field or
  `..Default::default()`, one line each. List every touched file in the join notes.

### The delivery shape rows B and C build on (pin it now)

The one thing not to get wrong in this feature: while the parent waits on its helpers, **no context-clearing pass may
run**, and the reports must arrive as **relayed harness nodes** (`Node::relayed(…, Origin::Harness, "task:<short>",
&report.node_text())`, as `read_reports` writes them today), which the clearing pass never stubs: clearing stubs only
`Kind::ToolResult` nodes (`compiler/clearing.rs`, read it, do not edit it). The wait itself is row B's; your tests pin
the half this row owns.

## Tests (each red at the base, with a planted revert)

1. `crates/theseus-kernel/src/tests_tasks.rs` (beside the `take_reports` tests near :250-280 and :470-550): a parent
   with three ended tasks a, b, c; `take_reports_of` [a, c] takes a and c in one frame (one commit holding the
   parent's record, `extra`'s records and one `task.reports_read` row with `via: "wait"`); b stays on `reports`; a
   following `take_reports` returns b alone. Plant P1: make `take_reports_of` take every report: red.
2. Same file: a named task still running is not taken, nothing is written when no named task has ended, and the
   wake case of item 1 behaves as you decided. Plant P2: drop the "ended only" check: red.
3. Same file (or `tests_deferred.rs`, whichever holds the carve tests): a task opened with the helper mark stores it,
   reads it back after a reopen of the kernel from the store, and a plain task stores no key (assert the serialized
   record has no such key). A record written without the field reads as not a helper. Plant P3: drop the field from
   `carve_task`'s copy: red.
4. **The delivery shape, in theseus-core:** a new test file `crates/theseus-core/src/tests_task_reports_of.rs` (one
   `mod` line in `lib.rs`, beside `tests_task_report`; follow `tests_task_report.rs`'s setup). It takes one of two
   ended tasks' reports through `take_reports_of` with the same relayed-node builder `read_reports` uses, and asserts:
   the parent's session holds that report as a relayed `Origin::Harness` node titled `task:<short>` whose kind is not
   `Kind::ToolResult`; the other report is still delivered by the parent's next turn (`read_reports`), and the taken
   one is not written a second time. If the builder is private to `turn.rs`, do not move it (that file is at its
   ceiling and rows B and C reshape it): build the node with `Node::relayed` in the test exactly as `read_reports`
   does, and say so. Plant P4: make the next turn re-deliver the taken report: red.
- Keep green: every existing test in `theseus-kernel` (in particular `tests_tasks`, `tests_frames`' `take_reports`
  frame check, `tests_deferred`) and the whole `theseus-core` suite.

## Not in this row

No `task.wait` or `task.fan_out` tool, no turn-loop change, no `turn.rs` edit, no change to the clearing pass or the
progress steer, no new config key, no cap or budget changed, no admission change. No benchmark run and no spend.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If any change
  of yours would alter a message of that flow, STOP that part and write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove no test, rename no test, lower no test's assertion count, weaken no assertion (an exact equality
  stays exact), raise no budget, cap or ceiling, do not edit `scripts/keel-guard.py` or its tests. Before the report
  run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its whole output. If it flags
  anything, or a part of this row would need a raised cap or a changed existing expectation, STOP that part, put the
  old test or value back, paste what the guard printed, and write a line starting `OWNER CALL:` saying what and why.
  Never add a `Keel:` trailer yourself: an unsigned ack counts for nothing.
- **Data floor:** nothing new is sent to a model or routed anywhere.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `ac298666`, green after; prove each with its planted revert); right over
  compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-kernel/AGENTS.md` first): `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` (the literal sites span crates), `cargo nextest run -p
  theseus-kernel -p theseus-core` plus every other crate whose files you touched, then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, and `SCHEMA` 2: untouched or why not), ceilings, any
`OWNER CALL:` or STOP. (2) **The API:** `take_reports_of`'s signature and doc, its ledger row's detail, the wake
decision, and the helper field as built (one field or two, and why). (3) **For rows B and C:** how to call it, what
the result says about running tasks, the relayed-node shape, and the accessor. (4) **Tests** and plants P1-P4, red
at the base and green after. (5) **Gate** commands and results. (6) **Keel** output, whole. (7) **DM golden** diff
result. (8) **Join notes:** every file touched with line ranges (the literal sites listed one per line), and a
statement that none of the `integ/p3-turn` rows' files above is touched. (9) **Follow-ups.** (10) One line on the
data floor.
