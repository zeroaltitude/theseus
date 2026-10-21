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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-open-request`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**Store format: your base is integ/cn, where it is already 48 (C14's SessionRecord.data); on main it is 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
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
## Your task: cn-open-request, per-request clearance: a message judged open may reach a China-origin model, carrying only the open run in the open view, with a pass for that one request (theseus-a1wvq.8)

Branch: `cloud/20261021-cn-open-request`. Every commit's subject carries `theseus-a1wvq.8`. Deadline for the report: 5
hours after you start.

## Your base: `integ/cn` with the open view in (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. Where the note above says your base is
`main`, or that the store format is 47, this section is right. `integ/cn` is `main` plus these China-origin rows, each
its own merge:
- the data floor and the session's data state (theseus-a1wvq.3, .5): `crate::data` (`DataPass`, `DataWhy`,
  `Clearance`, `DataState { opened, run, taint }`, `OpenRun { from, through }`, `Taint`), `data_gate.rs` (the
  chokepoint), `turn/data_step.rs` (`Floor { sensitive, cleared }`, `RouteState::floor()`, `floor_refuses`,
  `data_floor`, `floor_unrouted`, `data_pass`), store format 48;
- the private mark for background steps (theseus-a1wvq.10, first slice);
- routing's compose (theseus-a1wvq.4): `Profile.restricted`, `Ask.cleared`, `ModeProfiles.cn_profiles` /
  `.cn_after`; the load refuses a restricted name in any class's `profiles`, and `cn_profiles` except on `trivial`;
- open tasks (theseus-a1wvq.6): `open` on `task.create` / `task.start`, `data::open_task`; an open task carries its
  brief alone, and `open` is refused for a private parent;
- **the open view (theseus-a1wvq.7)**, which designs the seam this row is written against: `crates/theseus-core/src/
  open_view.rs` (`Open`, `Open::of`, `Open::offers`, `Open::refusal`, `open_roots`, `open_view::OpenRequest { view,
  spec, compiled }`, `TurnRunner::open_of`, `TurnRunner::open_request(&self, session, target)`), `PlaceView.open` and
  `PlaceView::content_class()` (`ceiling.rs`), `TurnCtx.open` (`toolrun.rs`), recall's `Place::Open` and
  `TurnRunner::drawn_place` (`recall.rs`), `[routing.data] open_paths` (`config/routing_data.rs`).

**Check first**: `git log --oneline | grep -E 'theseus-a1wvq\.(3|4|5|6|7)'` shows commits of each, and
`grep -n 'pub fn open_request' crates/theseus-core/src/open_view.rs` finds the seam. If `.7` or the seam is missing,
stop, write that in `CLOUD_REPORT.md`, push it, and end. If the seam's name or signature differs from the above, use
what the code has and say so in the report's first section.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48). **This row bumps nothing**:
`DataState.run` and `taint` are already stored; this row only fills them.

## Why
Today no request reaches a China-origin model: every turn's clearance is `Clearance::closed(DataWhy::Unchecked)`, and
every `routing::Ask` is built with `cleared: false` (`decide_route`, `decide_brief`, `decide_steer`). So `trivial`'s
`cn_profiles` never run, an open task on `dsflash` always moves to the `[model]` default, and the open view has no
caller but its test. The owner's update (2026-10-17, verbatim): "I think that Chinese models in dms can be evaluated by
content rather than by the nature of the channel. My understanding is also that they do have coding skills that are
worth applying at their cost -- I'd always want to do final review and fixes with a more competent model." So
clearance is **per request**: a China-origin (or unknown-origin) request may be built anywhere, the owner's DM
included, for a message Jev judged `open` at act; it carries only the **open run** (the newest stretch in which every
node is open, ending at that message) under the **open view**; its `DataPass` covers that one request's digest.

## Risk
High: the first row that lets content leave for a model outside the owner's trust. Anything missing, late or unknown
means "not cleared", and the turn runs on the `[model]` default or ends. Jev is never the only guard: the open view,
the run's chain and the chokepoint's per-digest pass each hold alone.

## Prerequisite: `integ/cn` (not `main`), as "Your base" says.

## Your row

### 1. `data::admits`: is this input open, and what is its run (pure, `data.rs`)
`pub fn admits(s: Option<&DataState>, c: &Check<'_>) -> Result<OpenRun, DataWhy>`, with `pub struct Check<'a>`
carrying: the input node's id and the id of the node just before it; the data answer (`routing::data::DataAnswer {
choice, confidence, band }`, read from `Verdict.data`), or none; whether the verdict was late; whether the route pack
acts for the session (`live` in `compile_first`: canary or live and the breaker closed); `sealed` (the thread is sealed
or a seal is pending: `ThreadInfo.sealed` / `.seal_pending`, as `rpc/thread_home.rs` reads them) and the verdict's seal
answer (`Verdict.seal`, `SealAnswer::recommends()`); and whether the input carries an attachment. The input is admitted
only when all hold, in this order, each failure its own `DataWhy`:
1. the pack acts (`pack_not_live`); a shadow or pinned judgment is recorded and never permits;
2. no attachment (`attachment`: Jev cannot see one);
3. not sealed, no seal pending, no seal recommended (`sealed`);
4. a verdict came (`no_verdict`), in time (`late`: a late verdict closes only its own message; there is no grace);
5. its answer is `open` (`verdict_private`, `verdict_unclear`) in band `Act` (`below_act`).

Then the run, by the chain: when the node just before the input is the stored `run.through`, the run is
`{ from: run.from, through: <input> }`; otherwise it is the input alone. Only blessed writers extend a run: an open
request's own assistant nodes (its turn stores the run, item 5) and, from the tool-screen row on, its screened tool
results. Anything else after `run.through` (a private-view turn's reply, `channel.read` text, a quote, a merge, a
split, a join steer, a task's report, a wake) cuts it, so a writer nobody thought of can never extend one.

A turn with no new input in an open session (an open task's later turn) is admitted with the stored run when its last
node is `run.through`, else `unvetted`.

### 2. The turn's admission (`turn/data_step.rs`, `route_step.rs`)
- Replace `RouteState.clearance: Clearance` with `RouteState.admitted: Result<OpenRun, DataWhy>` (default
  `Err(DataWhy::Unchecked)`), and `Floor.cleared` reads `admitted.is_ok()`. A turn-level value never mints a pass:
  only item 4's `data::clearance` builds a cleared `Clearance`. Keep the hunk small: an effort row on another lane adds
  fields beside `RouteState.ask`.
- Compute it in `compile_first` right after `read_verdict` and before `decide_floored`; for a turn that asked no route
  (`compile_routed`'s no-wait branch, before `floor_unrouted`), the input has no verdict, so `no_verdict`, except item
  1's open-session rule.
- `cleared: false` in `decide_route`, `decide_brief` and `decide_steer` becomes `t.route.floor().cleared` (the
  routing-compose review's note). A steered turn has no verdict, so it stays uncleared. Leave `correcting` (outside a
  turn, `correct.rs` "Outside a turn: no clearance") and `routing/checkpoint.rs` at `false`: fail closed.
- **Where an open request can be built.** A restricted target is cleared at the floor only when the turn is admitted
  **and** its request will be compiled in the open view: the session is open (`open_of(session)` is some: an open
  task), or the decision is a detour (`Decision.detour`: `trivial`, `quick`). In `data_floor`, a restricted profile
  for an admitted turn that is neither moves to the `[model]` default with `Reason::DataGate`, as an uncleared one does,
  and the turn's data reason is `private_view`. `cn_profiles` exist only on `trivial` today, so this mostly guards a
  session's own restricted base, a pin or a hold.

### 3. Building the open request
- **An open session** (an open task): its normal compile is already the open view (`.7`: `t.tc.open =
  self.open_of(session)`). Nothing new to build; item 4 checks it.
- **A detour in any session, the owner's DM included**: in `compile_detour`, when the turn's target (after
  `route_to`) is restricted and the turn is admitted, compile through `TurnRunner::open_request` instead of the
  session's own spec, set `t.tc.open = view.open` so the gate checks its calls against `Open::refusal`, and keep the
  detour's tail (`unadmitted`, `standing_dropped`, `LoopStarted`). Extend `open_request` to carry the run rather than
  the newest message alone: its nodes run from the transcript index of `run.from` (never before `detour_start`'s
  bound) to the end of the transcript, so a later loop carries its own assistant and tool nodes. Add the run as a
  parameter; the seam's only caller today is its test, which you update. Keep the detour's filter (no summary; no
  recall, since `open_request` draws none).
- A **later loop** of an open detour whose carried nodes leave the run (a tool result: nothing is screened before the
  tool-screen row) is not cleared. Move that loop to the `[model]` default with `Reason::DataGate`, as
  `floor_unrouted` does at loop 0, and compile it as a plain detour; the turn then stores no run (item 5). If a
  mid-turn move will not fit cleanly, end the turn with the floor's refusal (`refusal` in `data_step.rs`) instead and
  say so in the report. Either way, nothing goes to the restricted model.

### 4. The pass, per request (`data.rs`, `data_step.rs`)
`pub fn clearance(run: &OpenRun, c: &Carried) -> Clearance`, where `pub struct Carried` names whether the compile used
the open view (`t.tc.open.is_some()`) and the transcript positions of every node the request carries (from
`compiled.compilation.includes` and the tail after `as_of`; find the exact set the compiler renders) and of
`run.from`. Cleared only when the view is open (`private_view` otherwise) and every carried node is at or after
`run.from` (`not_in_run`), and, past the input, written by this request's own loop and not a tool result (`unvetted`,
`tool_result`). **Name note:** `.7` already took `OpenRequest` for the compiled open request, so this row's input type
is `data::Carried`, not the older plan's `data::OpenRequest`; say so in the report's first section, as a contract for
the later rows.

`TurnRunner::data_pass` (called in `turn.rs` before `call_model`) builds the `Carried` of the request it is about to
send and puts `data::clearance(&run, &carried).pass(&digest)` on it: **each loop's request gets its own pass**, and a
request that fails gets none, so the chokepoint (`data_gate.rs`) refuses it if anything still sends it to a restricted
model. Record each request's clearance on the turn so item 5 and the route row can read it (no new frame).

### 5. Store it, in the turn's own session write
No frame of its own. When the turn's requests to a restricted model all went out with a pass, set `session.data.run =
Some(OpenRun { from: run.from, through: <the turn's last node> })`; `DataState::merge` already does the compare and set
(`session.rs` `take_turns_fields`). A private-view turn writes nothing: the chain cuts the run by itself. When an open
session's input or request is not admitted, record its taint once (`Taint { why, node, at_ms }`, `data::tainted_row`):
bookkeeping only, never a gate.

### 6. An open task's brief is judged before its first compile
D2 says Jev's data question is asked about the brief itself. `brief_point` (`turn/brief_step.rs`) asks the route pack
about a brief only for a task marked to choose its base (`marked`). For an **open** task (`data.opened` set), ask it
always while the pack acts, and let the first compile wait for that verdict without the `[routing] max_wait_ms` cut (a
task's first turn is not latency-bound; the judge client's own timeout bounds it). The base is still chosen from the
verdict only when `marked`. A brief not admitted leaves the task uncleared: its turn runs on the floor's `[model]`
default, and its taint row records why.

### 7. The parent's hold copies no content into an open task (review finding F4 of cn-open-tasks)
`external::taken` copies the parent's `url` and `query` into the task's hold, and `external::gate` puts them in a
call's reason (`why_at`), which reaches the task's model. For an open task (`task.rs::create`'s `parent_hold` arm and
`rpc/task_start.rs` `hold_of`), take the hold with `url` empty and `query` none: the hold still raises the posture,
and its reason names only the tool and the time. `.7` did not touch this.

### 8. The recall spread (review finding F3 of cn-open-tasks): already closed by `.7`; add its test
The brief's `DerivedFrom VIA_BRIEF` edge to the parent's reply (`task.rs`) is spread both ways
(`recall/adjacency.rs` `mapped`). `.7` closes it: every hit, the spread's included, gets `drawn_place`, and a session
that is not open is `Place::Unknown`, which `may_draw_at` drops. Add a memory-rig test of it; if the spread's hits
bypass the place filter after all, skip the edge for an open task instead and say so.

### 8b. A tainted open session sends nothing after its taint (review finding F1 of cn-open-view)
`.7`'s `open_of` (`open_view.rs`) returns the open view for an opened session whether or not `data.taint` is set, so
a tainted open task keeps compiling in the open view with its post-taint history and tool results, and
`content_class()` says Shared. The open view is a shape, not a gate, and the floor on `integ/cn` reads only
`google::private::session` (`turn/data_step.rs`). This row is the gate: no node at or after a session's taint may reach
a restricted-origin target. Either `admits` refuses a restricted target for a session whose `data.taint` is set, or the
request carries only the open run that ends before the taint node; pick one, say which in the report, and keep the
taint row itself bookkeeping (it never blocks a Claude target). Test:
`a_tainted_open_task_sends_nothing_after_its_taint_to_a_restricted_model` (a planted revert must make it fail).

### 9. Docs
`crates/theseus-core/AGENTS.md`: one paragraph in the data floor's passage (where an open request is built, the run,
the pass per request, what ends a run).

## Tests (each with a planted revert; the first is the key test)
Put them in a new `crates/theseus-core/src/tests_open_request.rs` (one `mod` line in `lib.rs`), reusing the rigs of
`tests_data_floor.rs` and `tests_open_view.rs`. Test config: a restricted profile named `dsflash` on a fake provider the
catalog judges `cn` (the rig's zai fake is one), no Haiku in `trivial.profiles`, `trivial.cn_profiles = ["dsflash"]`,
and the route pack acting (canary or live) with a scripted verdict.
1. **`a_dm_open_message_reaches_dsflash_with_only_the_open_run`** (key; replaces T18). In the owner's DM: a message
   with a marker (judged private), a reply, then a message judged `open` at act whose verdict's mode is `trivial`. The
   restricted fake gets exactly one request; it holds the open message, no node from before it (no marker), no owner
   material (a context file not marked public, a standing fact: their markers absent), and its `data_pass` covers its
   digest. Plants: (a) bound the open detour at `detour_start` alone, so the old nodes ride along: the pass is refused
   (`not_in_run`) and the fake gets none; (b) mint the pass from the turn's admission without `Carried`: the marker
   reaches the fake.
2. `a_private_message_in_the_dm_cuts_the_open_run`: open, open (the run grows across the open request's own reply),
   private, open: the last request starts at the last message. Plant: make `admits` ignore the chain.
3. `a_non_detour_mode_in_a_private_view_session_never_builds_an_open_request`: an admitted DM turn whose base is
   restricted and whose mode is not a detour runs on the `[model]` default with `data_gate`; the fake gets nothing.
4. T7 `fail_closed_matrix`: `admits` and `clearance` as units, one assertion per `DataWhy` they return; only open +
   act + the structure admits.
5. T8 `a_taint_is_bookkeeping`: an open task's private node writes one `tainted` row; a later admitted request is
   still cleared over its own run.
6. T10 `an_unknown_writer_breaks_the_chain`: a node from an unblessed writer after `run.through`: the run restarts at
   the next open message, and a loop continuation after such a node is `unvetted`.
7. T14, cleared vs uncleared: an uncleared turn never reaches the restricted fake on any path (base, pin, hold,
   correction, detour); a cleared trivial detour does, with a pass.
8. T17 `an_open_task_reaches_dsflash_and_a_private_node_ends_its_run`.
9. T20, the clearance half: a brief judged open clears the open task's first request; a brief judged private leaves
   it on the `[model]` default with its taint row; the first compile waited past `max_wait_ms` for the brief's verdict.
10. `an_open_tasks_hold_names_no_url_or_query` (item 7), and `an_open_tasks_recall_never_spreads_into_its_parent`
    (item 8).
11. `a_later_loop_after_a_tool_result_leaves_the_restricted_model` (item 3's last point).
Run `tests_first_request.rs`, `tests_data_floor.rs`, `tests_open_view.rs`, `tests_open_task.rs`, `tests_route.rs` and
the routing tests unchanged: they must pass.

## Not in this row
- **Jev judging Theseus's replies (the owner's answer Q5, 2026-10-17, verbatim: "Yes let's say it gets the whole
  conversation if jev judges the same as ok").** Filed as theseus-a1wvq.15. In this row a reply written by a
  private-view turn ends the run (fail closed), so in the DM a China-origin request carries the newest open message,
  plus earlier ones only across an open request's own replies.
- The tool-result screen (theseus-a1wvq.9): until it lands, a tool result ends the run.
- Background calls on a restricted target, drafts and their Claude review, the footer and bench columns, config
  guidance (theseus-a1wvq.10, .13, .11, .12).
- The late grace and `session.data_clear`: retired by the owner's update; do not build them.

## Beside you
An effort row on another lane adds `RouteState` fields after `ask` in `route_step.rs`: keep your `RouteState` edit to
the one field. The open view's join may bring small fixes to `open_view.rs`: take the names as on your base.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing, late or unparsable is closed, never open. Never loosen a check, a
  threshold, a test or a ceiling to make a test pass.
- **On, measured, with a named rollback.** The behaviour ships on by default. The rollback is the config: with no
  restricted profile in any `cn_profiles` and no open task, no request is built open, and every existing test passes
  unchanged.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new
  on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the
  gate's turn bench must not move): `admits` is pure and reads only what the turn already holds; nothing on the path
  before a request leaves grows with history beyond the detour's existing transcript read.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The key test must be shown failing on both
  of its planted reverts.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `git fetch origin integ/cn` and then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet`
  and paste its output in the report. It must be empty. (A `main` base would flag the earlier rows' changes as yours.)
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name, company or private
  project name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture names are
  invented ("Acme", "Widgetco"); topics live only in `[routing.data]`.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its size
after the commit, it must not be empty). First section: the contract names as built (`data::admits`, `data::Check`,
`data::Carried`, `data::clearance`, `RouteState.admitted`, `open_request`'s new signature) for the tool-screen and
background rows. Then: what changed, file by file; each test and its plant (the line broken, the failure seen, the
restore); the key request's token count; what could not run on the VM and the exact commands for the maintainer; the
keel guard's output; "store format: unchanged (48)"; the maintainer's live check after the join on a scratch daemon
(`trivial.cn_profiles = ["glm"]`, no Haiku: an open DM message runs on glm with only the open run; after a private
message the next open one's request starts past it; an open task runs on glm), with the rows to read; the follow-ups.
Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
