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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-tool-screen`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

This task was written against an older `main`. Your branch starts from `main` at `87696a7c` (2026-10-21), unless your task names another base (an integration branch). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,405 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- Google part 2 rows: gw-mail, gw-calendar, gw-drive, gw-sheets-slides (`crates/theseus-google/`, each its own
  service module, `fake/<service>.rs`, one registration line each).
- Harness rows: job-truths (job command lines, `theseus-kernel/src/job.rs`, fs/file read results), effort-on-evidence
  (turn checkpoints' effort), turn-end follow-ups and long blind waits, recall abstention and the `service` orphan.
- Threads rows: routing (`crates/theseus-core/src/thread_route.rs`, working sets, claims) and cross-stream reach
  (`theseus-memory/src/threads.rs`, `thread_recall.rs`, a place digest, Jev's near-miss check, thread links).
- Ontology follow-ups (`crates/theseus-ontology/`, the sweep, the dream's guidance, the replay), test-infrastructure
  rows (Discord task-thread tests, leaking shell loops), the China-origin model rows (`data_gate.rs`, session data),
  rough edges in progress words, detours and `theseus why`.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: cn-tool-screen, a tool result joins an open request's run only when it passes a screen; one that fails ends the run, never the session (theseus-a1wvq.9)

Branch: `cloud/20261021-cn-tool-screen`. Every commit's subject carries `theseus-a1wvq.9`. Deadline for the report: 3
hours after you start.

## Your base: `integ/cn` with cn-open-request in (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. Where the note above says your base is
`main`, or that the store format is 47, this section is right. `integ/cn` is `main` plus the China-origin rows, each
its own merge:
- the data floor and the session's data state (theseus-a1wvq.3, .5): `crate::data` (`DataPass`, `DataWhy` with
  `tool_result` already among its words, `Clearance`, `DataState { opened, run, taint }`, `OpenRun { from, through }`,
  `Taint`, `data::tainted_row`), `data_gate.rs` (the chokepoint), `turn/data_step.rs` (`Floor`, `data_floor`,
  `floor_unrouted`, `data_pass`, `refusal`), store format 48;
- routing's compose (.4: `Profile.restricted`, `Ask.cleared`, `cn_profiles`), open tasks (.6: `open` on
  `task.create`, `data::open_task`), and the open view (.7: `crates/theseus-core/src/open_view.rs` with `Open`,
  `Open::offers`, `Open::refusal`, `runs_tool`, `open_roots`, `TurnRunner::open_of`, `TurnRunner::open_request`;
  `TurnCtx.open`; the gate's open check in `toolrun/order.rs` `ToolRuntime::refusal`);
- **cn-open-request (theseus-a1wvq.8)**, the row this one finishes. It made clearance per request:
  - `data::admits(..) -> Result<OpenRun, DataWhy>` and `data::Check`;
  - `RouteState.admitted: Result<OpenRun, DataWhy>` (it replaced `RouteState.clearance`; `Floor.cleared` reads it);
  - `data::Carried` (the nodes one request carries) and `data::clearance(run: &OpenRun, c: &Carried) -> Clearance`,
    which mints each loop's pass in `TurnRunner::data_pass`;
  - the open detour in `compile_detour` through `open_request`, now carrying the run;
  - the run stored in the turn's own session write.
  
  It left one placeholder for this row: **a later loop whose request would carry a tool result is not cleared**
  (`clearance` says `tool_result`), and it moves to the `[model]` default with `Reason::DataGate`, or, if that did not
  fit, the turn ends with the floor's refusal. This row replaces exactly that rule, and nothing else of `.8`.

**Conductor note (2026-10-21 15:10, from `.8`'s reading review):** `.8` as built MOVES the later loop; it does not end the turn. `data_move` (in `turn/open_step.rs`, called from `turn.rs` at every loop > 0, after `checkpoint_apply`, before the compile) moves a restricted target to the `[model]` default through a fourth `OnceLock` slot when any node after `run.through` was not written by this turn's loop (a tool result is `Writer::ToolResult`). It records `route.decided` reason `data_gate`, `est_tokens` 0. A detour goes on as a plain detour in the session's own view. In an open session the moved request's `closed(ToolResult)` becomes the session's stored taint (in `data_end`). So build item 2's MOVE branch, and make a screened result count as `Writer::Loop` in `open_step::writer` (which both `data_move` and `data_end` read); the `data_end` taint path must then not fire for a screened result. Sessions tainted `tool_result` before this row lands stay closed (the taint is stored): say so in the report.

**Check first**:
- `git log --oneline | grep -E 'theseus-a1wvq\.8'` shows `.8`'s commits;
- `grep -n 'pub fn clearance\|pub struct Carried\|pub fn admits' crates/theseus-core/src/data.rs` finds the three;
- `grep -rn 'admitted' crates/theseus-core/src/turn/route_step.rs crates/theseus-core/src/turn/data_step.rs` finds
  the field;
- find `.8`'s later-loop handling: the place where a carried tool result leaves the run. Search `ToolResult` and
  `tool_result` in `data.rs`, `turn/data_step.rs` and `turn/compile_step.rs`, and read `.8`'s commit messages.

If `.8`'s commits, `clearance`, `Carried` or `admitted` are missing, stop. Write that in `CLOUD_REPORT.md`, push it,
and end. If the join renamed any of them, or if the later-loop handling takes another shape (a move, or an end), use
the code's names and shape. Say which in the report's first section. Read `CLOUD_REPORT.md`'s history if it is
still in `git log` (`git log --all -- CLOUD_REPORT.md`); its first section names `.8`'s contract as built.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48). **This row bumps
nothing.** `DataState.run` and `.taint` are stored already. The screen's word goes in the stop line and the turn's
facts, not in a stored record.

## Why
After `.8`, an open request can reach a China-origin model, but only until its model calls a tool. Take an open task
on `dsflash` that reads a file under `[routing.data] open_paths`. Its second loop always leaves the restricted model,
because nothing judged the result. So a China-origin draft never sees its own reads, and the drafts row
(theseus-a1wvq.13) cannot loop over tool results.

The fix cannot be "let tool results through". The result is the one part of an open request that no person wrote and
Jev never judged. A file can hold a secret. A fetch can reach the owner's private network. A program can print any
file the daemon's user can read. The open view's review found that last one: `Open::offers` gives `proc.run`, `job.*`
and `term.*` to an open session whose directory is under the open roots, and `Open::refusal` checks only the call's
working directory, so the program itself can read `~/.ssh` and its output becomes a tool result.

So a tool result joins the run only once it passes a screen. A result that fails ends the run there. The session is
**not** made private from now on, since that rule was retired with the sticky gate. The person's next open message
starts a fresh run.

## Risk
High in kind (tool output reaches a model outside the owner's trust), small in size. A result the screen cannot vouch
for ends the run. The gate's open check, the screen, `clearance` and the chokepoint's pass each hold alone.

## Prerequisite: `integ/cn` with `.8` merged (not `main`), as "Your base" says.

## Your row

### 1. `data::screen` (pure, `data.rs`)
```rust
pub enum Unscreened { Program, Tool, Secret, Outside }   // as_str: program, tool, secret, outside
pub fn screen(tool: &str, plan: Option<&Plan>, redactions: u64, open: &Open) -> Result<(), Unscreened>
```
Here `plan` is the call's `theseus_protocol::Plan` (`None` when the call does not plan), `redactions` is the result
node's `meta["redactions"]` (0 when absent), and `open` is the request's `open_view::Open`. The checks run in this
order:
1. **`Program`.** `open_view::runs_tool(tool)` (`proc.run`, `job.*`, `term.*`). A program can read any path the
   daemon's user can read, so its output cannot be screened by its text. This closes the open view's F3. A program's
   result never extends a run in this row; a sandboxed program run is the follow-up, below.
2. **`Tool`.** Only two kinds of tool are on the screen's list, and every other tool's result is `Tool`:
   - the file tools (`places::files_tool`: `fs.*`, `git.*`, `text.*`);
   - `web.search` and `http.fetch`.

   This keeps out `task.*` (a task list or a report names the owner's other work), `wake.at`, `tools.find` (it names
   the owner's configured tools) and anything added later. One `matches!` is the list; say it in a doc line.
3. **`Secret`.** `redactions > 0`. The runtime already scrubs every result before it is stored
   (`toolrun.rs` `result_node_in`, `Scrubber::scrub`, which records `meta.redactions`). So a result in which the
   scrubber found a secret value or a secret shape is out, even with the value replaced.
4. **`Outside`.** This is the path half, checked again after the gate (do not trust that the gate ran in the open
   view). It covers four cases:
   - a file tool with no plan;
   - a file tool with an empty `plan.resources`;
   - a file tool with a resource whose canonical path (`theseus_tools::paths::canonical_best_effort`) is not within
     any of `open.roots` (`paths::within`, as `Open::refusal` does);
   - an `http.fetch` whose `plan.url` is a private address (`crate::web::net::private_url`). A private address waits
     for approval (DD5), but an approved page from the owner's network is still not open.

Pass: `Ok(())`. Keep the module's doc in step: one paragraph on the screen.

### 2. The screen step (new `crates/theseus-core/src/turn/screen_step.rs`, one `mod` line in `turn.rs`)
`TurnRunner::screen_loop(&self, t: &mut Turn<'_>, session: &mut SessionRecord, uses: &[ToolUse], i: u32) ->
Option<Unscreened>`, or the shape that fits `.8`'s code. Call it **once per loop in `turn.rs`, right after
`run_tools` returns** (`let answered = boxed(|| self.run_tools(..))`), before the next loop's compile. Keep it to a
line or two in `turn.rs` (see "Line room").

- **It runs only for an open request's loop.** All three must hold: the turn is admitted
  (`t.route.admitted.is_ok()`), its current target is restricted (`self.restricted_profile(&t.target.profile)`),
  and `t.tc.open` is `Some`. Otherwise it returns at once and touches nothing. A plain turn, a Claude turn and a
  private-view turn pay one branch. If the target is restricted and admitted but `t.tc.open` is `None`, that is a
  bug: fail closed, as if every result failed.
- **For each result node this loop wrote** (a `Body::ToolResult` answering one of `uses`' ids, read back as the next
  compile reads the transcript), plan its call again:
  - find the tool with `self.tools.tool_by_wire(&u.name)`;
  - call `tool.plan(&u.input, &self.tools.ctx_in(&tc))` (`toolrun/session_dir.rs`; a pure parse, the call's own
    resources);
  - pass the result to `data::screen` with the open view's `Open`.

  Calls without a result node (`AwaitingConfirm`, a `Background` call's later result) are not screened here. Their
  results are written later by another writer, so `.8`'s chain rule ends the run at them. Say so in a comment.
- **Pass.** Add the node's id to the turn's screened set: `RouteState.screened: Vec<String>`, one field beside
  `admitted`, default empty. An effort row on another lane adds fields after `RouteState.ask`, so keep your hunk to
  that one line.
- **Fail** (the first failure; stop screening the rest):
  - The result node stays written: the record stays whole, and the loop's other results stay too.
  - The run ends at the last node before it. That happens by itself: the node is not in the screened set, so `.8`'s
    `clearance` refuses the next request (`tool_result`), and the turn stores no run past it (`.8` item 5).
  - In an **open session** (an open task: `session.data.opened` is set), record the taint once with `.8`'s helper:
    `Taint { why: DataWhy::ToolResult, node: Some(<result id>), at_ms }` and its `session.data` `tainted` row. It is
    bookkeeping only. A private-view session (the owner's DM) records nothing: "no 'private from now on'".
  - Then do what `.8` does for a later loop that leaves the run:
    - **If `.8` moves the loop** to the `[model]` default with `Reason::DataGate`, keep that move. The turn goes on
      on the default, and the route row says `data_gate`. Write no extra line.
    - **If `.8` ends the turn**, end it with the new line, as a refusal through `data_step.rs` `refusal`'s class:
      "This turn stopped: a `<tool>` result can't go to <profile>, a model of <origin> origin (<word>), so it wasn't
      sent. Send again to go on."
      - `<origin>` is `cn` or `unknown` (`Config::profile_origin`).
      - `<word>` is `Unscreened::as_str`.
      - The 10-17 line's "The session is private from now on" is gone.
- **Record the outcome** with no new frame: the existing `LoopCut { decision: "data_gate" }` when the loop moves or
  ends. Never a new fact on a plain turn.

### 3. `clearance` admits a screened result
Change `.8`'s rule for a carried tool-result node in `data::clearance` / `Carried`:
- A tool result past the input is in the run when this turn screened it, i.e. its id is in `t.route.screened`. Give
  `Carried` the screened ids, or mark each carried node, whichever matches `.8`'s shape.
- A tool result inside the stored run (`run.from..=run.through`, from an earlier turn of an open task) is in the run:
  it was screened when it was written, because only screened results extend a run.
- Any other tool result stays `tool_result`.

Everything else in `clearance` is unchanged: open view, `not_in_run`, `unvetted`. Each loop still gets its own pass
for its own digest in `data_pass`.

### 4. The run extends over screened results
`.8` item 5 stores `run.through = <the turn's last node>` when every request to the restricted model went out with a
pass. With the screen, an open task that reads, gets a screened result and answers stores a run through its answer.
Its next turn is admitted on the stored run (`.8`'s "a turn with no new input in an open session" rule). Check that
this holds, and fix the store step only if `.8` stopped the run at the first tool result.

### 5. Docs
`crates/theseus-core/AGENTS.md`: two sentences in the data floor's passage. Say what the screen checks, that a
program's result never extends a run yet, and that a failed screen ends the run, not the session.

## Tests (each with a planted revert; the first is the key test)
Put them in a new `crates/theseus-core/src/tests_tool_screen.rs` (one `mod` line in `lib.rs`), reusing `.8`'s rig in
`tests_open_request.rs` and the rigs of `tests_data_floor.rs` and `tests_open_view.rs`:
- `dsflash`, a restricted profile on a fake provider the catalog judges `cn`;
- an open task, or an open detour in the DM;
- `[routing.data] open_paths` pointing at a temporary directory;
- the route pack acting, with a scripted verdict.

For a secret, use a value the rig's scrubber knows (a board value, as `tests_m3`'s rig sets one) or a shape that
`scrub/tests_shapes.rs` already uses. Never add a new real-looking key.

1. **T19 `a_private_tool_result_stops_a_cleared_turn`** (key). An open task runs on `dsflash`. Loop 0's scripted
   answer calls `fs.read` on a file under `open_paths` that holds the secret. Assert:
   - the result node is written, with `meta.redactions`;
   - the restricted fake gets exactly one request (loop 0) and no loop-1 request;
   - the turn moved to the `[model]` default with `data_gate`, or ended with the new line (whichever `.8` built);
   - one `session.data` `tainted` row, with `tool_result` and the result's node id;
   - the stored run does not reach the result.

   Repeat the case for a `proc.run` in a session directory under the roots (`program`).

   Plant: make `data::screen` return `Ok(())` first thing. The fake must then receive a loop-1 request that carries
   the result, and the test fails.
2. `the_screen_runs_only_for_an_open_request`. Script the same calls (the secret read and a `proc.run`) in two
   turns:
   - a private-view DM turn on a Claude profile;
   - an open task whose profiles are all Claude.

   Neither writes a taint row, moves or ends early. Loop 1 is sent as before, and the turn's facts and rows are the
   same as on the base. Plant: drop the open-request condition in `screen_loop`. A taint row or a move appears.
3. `a_screened_result_extends_the_run`. An open task on `dsflash` calls `fs.read` on a clean file under `open_paths`.
   Assert:
   - loop 1 goes to `dsflash`, carries the result, and its `data_pass` covers its digest;
   - the session's stored `run.through` is the turn's last node;
   - the task's next turn is admitted on that run.

   Plant: skip the push into `RouteState.screened`. Loop 1 then leaves `dsflash`.
4. `a_failed_screen_in_the_dm_ends_the_run_not_the_session`. In the DM, an open detour's loop reads a file outside
   the roots. Its call is refused at the gate, so let the fake call it through a file-tool plan the rig makes outside
   the roots; or use `http.fetch` of a loopback address (127.0.0.1), approved in the rig. The loop leaves the restricted model,
   and no taint row is written. The person's next message, judged `open` at act, reaches `dsflash` with a run that
   starts at that message. Plant: write the taint for a private-view session too. The test asserts no row.
5. `screen_matrix` (pure, `data.rs` tests): one assertion per `Unscreened` word, plus the passes. Cases:
   - `fs.read` under a root passes; `fs.read` outside fails `outside`;
   - a symlink out of a root fails `outside` (canonical);
   - a file tool with no plan, or with empty resources, fails `outside`;
   - `http.fetch` of a public URL passes; of `127.0.0.1` it fails `outside`;
   - `web.search` passes;
   - `proc.run`, `job.output` and `term.send` fail `program`;
   - `task.list`, `wake.at` and `tools.find` fail `tool`;
   - redactions 1 fails `secret`, even on a file under a root.

Run these unchanged; they must pass: `tests_open_request.rs` (its test of a later loop after a tool result now holds
for an **unscreened** result; if it used a clean file, change its fixture to a failing one and say so: that is not a
weakening), `tests_data_floor.rs`, `tests_open_view.rs`, `tests_open_task.rs`, `tests_first_request.rs`,
`tests_route.rs`, and `tests_m3::a_plain_turn_stays_within_its_frame_budget`.

## Line room (check before you edit)
`scripts/long-files.txt` sets the ceilings: run `wc -l` on each file you touch and compare.
- `crates/theseus-core/src/turn.rs`: ceiling 3,525. It was 3,409 before `.8`, and `.8` added some lines. Your hunk is
  the `mod` line and the call (2-4 lines). Everything else goes in `turn/screen_step.rs`.
- `crates/theseus-core/src/toolrun.rs` is about 2,491 lines and **has no entry**. A file not listed must stay at
  2,500 lines or fewer, so do not add to it: use `tool_by_wire` and `ctx_in` as they are.
- New tests go only in `tests_tool_screen.rs`; `tests_m3.rs` is near its ceiling.

## Not in this row
- **A program's output in an open run.** It needs a program run whose view reads only the system and the open roots
  (the L1 sandbox's read-only view, `theseus-sandbox`), so that its output can be vouched for by construction. It is
  filed as theseus-a1wvq.16. Until it lands, a China-origin draft (theseus-a1wvq.13) edits and reads
  under its tree, and its Claude reviewer runs the tests.
- Screening results answered after their loop (a confirm, a background job, a result absorbed at the next turn's
  start). They come from other writers, and `.8`'s chain ends the run at them.
- A mid-turn move to another provider that `.8` did not build; Jev judging replies (theseus-a1wvq.15); background
  calls (theseus-a1wvq.10); the footer and bench columns (theseus-a1wvq.11).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing, late or unparsable is closed, never open. Never loosen a check, a
  threshold, a test or a ceiling to make a test pass.
- **On, measured, with a named rollback.** The behaviour ships on by default. The rollback is the config: with no
  restricted profile in any `cn_profiles` and no open task on a restricted profile, no open request runs, the screen
  never runs, and every existing test passes unchanged.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new
  on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's
  turn bench must not move). The screen step is one branch for any turn that is not an open request, and on an open
  request it reads only the loop's own results.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The key test must be shown failing on its
  planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `git fetch origin integ/cn` and then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet`
  and paste its output in the report. It must be empty. (A `main` base would flag the earlier rows' changes as yours.)
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name, company or private
  project name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture names and paths
  are invented ("Acme", "Widgetco"); topics live only in `[routing.data]`.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, never a shell heredoc, and check
after the commit that it is not empty. Its sections, in order:
1. The contract names as built (`data::screen`, `Unscreened`, `RouteState.screened`, `screen_loop`), and any of
   `.8`'s names the code had under another name. The drafts row (theseus-a1wvq.13) reads this section.
2. What `.8`'s later-loop handling was (a move or an end), and which branch of item 2 you built.
3. What changed, file by file, with each file's line count against its ceiling.
4. Each test and its plant: the line broken, the failure seen, the restore.
5. What could not run on the VM, with the exact commands for the maintainer.
6. The keel guard's output.
7. "store format: unchanged (48)".
8. The maintainer's live check after the join, on a scratch daemon with `trivial.cn_profiles = ["glm"]` and an open
   task on glm:
   - a read of a clean file under `open_paths` stays on glm;
   - a read of a file holding a fake secret leaves glm, writes the `tainted` row, and the next request has no pass.

   Name the rows to read.
9. The follow-ups.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
