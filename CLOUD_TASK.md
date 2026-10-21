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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-open-tasks`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
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
## Your task: cn-open-tasks, an open task: `task.create` and `task.start` take `open`, the task carries only its brief, and its session records that it was opened open (theseus-a1wvq.6)

Branch: `cloud/20261021-cn-open-tasks`. Every commit's subject carries `theseus-a1wvq.6`. Deadline for the report: 3
hours after you start.

## Your base: `integ/cn` with the floor row in (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. `integ/cn` is `main` (`87696a7c` or
later) plus the China-origin floor row, theseus-a1wvq.3 and theseus-a1wvq.5 (it was built on branch
`cloud/20261021-cn-floor-session`). Where the note above says your base is `main`, or that the store format is 47, this
section is right:
- **Check first** that the floor row is in: `git log --oneline | grep -E 'theseus-a1wvq\.(3|5)'` shows both a `.3`
  commit (the floor: `data.rs`, `Gated`, `data_floor`) and a `.5` commit (the session's data state). If the `.5`
  commit is missing, `SessionRecord.data` does not exist and this row cannot be built: stop, write that in
  `CLOUD_REPORT.md`, push it, and end.
- **Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`. With `.5` in it is 48
  (`SessionRecord.data`, the `SESSION_BEFORE_DATA` layout sample). **This row bumps nothing**: it fills a field that
  already exists.

## Why
The owner's words (2026-10-17 09:57, verbatim): "I think that Chinese models in dms can be evaluated by content rather
than by the nature of the channel. My understanding is also that they do have coding skills that are worth applying at
their cost -- I'd always want to do final review and fixes with a more competent model." The design that came from it:
a model of restricted origin (`cn` or `unknown`) only ever **drafts** code, and the draft is an **open task**: a task
whose session starts from nothing but its brief, is compiled in the open view (no owner material), and is judged on its
own content. A Claude profile then reviews and fixes the draft before anything is reported or applied (a later row).

Today there is no such thing as an open task. Every task started by `task.create` must carry an **arrangement**:
exact quotes of the parent conversation's messages, written into the task's session after the brief
(`task.rs`, `arranged`; "Every task needs an `arrangement`" in the tool's description). For an ordinary task that is
right (the task reads the owner's words verbatim). For an open task it is the leak: the quoted messages are the
owner's conversation, never judged open. `task.start`'s `quotes` do the same from the owner's side. And nothing marks a
task's session as opened open, so the open view (theseus-a1wvq.7) and the open-request row (theseus-a1wvq.8) have
nothing to read.

**Risk: low.** One optional wire field and one optional tool field, both off by default; a task without `open` is
byte-for-byte as today. Until theseus-a1wvq.8 joins, an open task still never reaches a restricted model (the floor
row's `data_floor` keeps every turn uncleared): it runs on its parent's model, and once theseus-a1wvq.7 joins, in the
open view.

**Prerequisite: `integ/cn` with theseus-a1wvq.3 and .5 (above).**

## What exists on your base (search by name; lines move)
- **`task.create`** (`crates/theseus-core/src/task.rs`): `Input` (fields `brief`, `title`, `arrangement`,
  `fidelity_ack`, `check_of`, `profile`, `start_at`, …), `input_of` (the field checks, each refusal a sentence the
  model reads), `TaskCreate::input_schema` (the `properties` object the model sees), and `create`, which calls
  `crate::check::prepare` for a check or `arranged(tc, &i, &nodes, holder)` for every other task, then opens the
  child in one kernel frame (`tc.kernel.open_task_at(..)`): it builds the task's `SessionRecord::with_id(..)`, the
  arrangement node (`Node::arrangement`), the brief node (`Node::relayed`), the `DerivedFrom` edges, the parent's
  external hold (`crate::external::hold(rec, taken, None)`), and the graph rows. `arranged` refuses a missing
  arrangement with class `missing` and writes `task.arrangement_refused`.
- **`task.start`** (`crates/theseus-core/src/rpc/task_start.rs`): `task_start(..)` reads
  `theseus_protocol::task_start::TaskStartParams`, turns `quotes` into an arrangement of role `context` held as outside
  text (`external::VIA_QUOTE`), and writes the task in one frame through `owned_task_records(..)`
  (`Kernel::open_task_owned`). Tests: `tests_task_start.rs` (its `start(brief)` helper builds params with
  `..Default::default()`), `tests_task_brief.rs`, `tests_task_report.rs`, `tests_task_search.rs`.
- **The wire type** `TaskStartParams` (`crates/theseus-protocol/src/task_start.rs`; `Default`, `ts_rs::TS` under
  test). The cockpit's generated type is `cockpit/src/protocol.gen/TaskStartParams.ts`, regenerated by the protocol
  crate's TS test (see `crates/theseus-protocol/AGENTS.md`). Other builders of the params:
  `crates/theseus/src/task_start.rs` `params_of`, `crates/theseus-discord/src/runtime/start_task.rs` `params`, and
  the cockpit's `views/Tasks.tsx`. None of them changes in this row except where a struct literal without
  `..Default::default()` needs the new field.
- **From the floor row (on your base):** `crate::data` (`crates/theseus-core/src/data.rs`) with `DataState { opened,
  run, taint }`, `Opened { how, at_ms }` (`how` names `task` or `draft`), `OpenRun`, `Taint`, `DataWhy`, `Clearance`,
  `DataPass`; `SessionRecord.data: Option<Box<DataState>>`; the ledger kind `session.data` with its fact in
  `fact/data.rs` and `pub` writer functions in `data.rs` for its `opened` and `tainted` rows; the inspect line
  (`rpc/inspect.rs`). In `task.rs`, beside the `external::hold` call, the floor row made a task start with **no**
  `run`, whatever its parent's (theseus-a1wvq.5's test `a_task_never_inherits_its_parents_run`). Read `data.rs` first
  and use its names exactly as written there: if `how` is an enum, use its task variant; if the `opened` writer has
  another name than you expect, use the one that exists. Never add a second type or writer for the same thing.

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| 6 | `cn-open-tasks` | theseus-a1wvq.6 | `open` on `task.start` and `task.create`; an open task's session gets `data.opened` and one `session.data` `opened` row in its opening frame; an open task carries only its brief (no arrangement, no quotes, from any parent); `open` refuses `check_of`, `arrangement` and `quotes` in words | `theseus-protocol/src/task_start.rs`, `cockpit/src/protocol.gen/TaskStartParams.ts` (generated), `theseus-core/src/task.rs`, `theseus-core/src/rpc/task_start.rs`, tests in a new `theseus-core/src/tests_open_task.rs` | S |

1. **The wire field.** `TaskStartParams.open: bool`, `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`
   with `#[cfg_attr(test, ts(optional))]` (search the protocol crate for that pairing: `places.rs` and `import.rs`
   use it), doc: "An open task: its session starts from the brief alone and is compiled in the open view; its data
   may go to a model of any origin once its content is judged open." Regenerate the cockpit's TS with the protocol
   test. `crates/theseus-protocol/src/lib.rs` gains **no** line.
2. **The tool field.** `task.create`'s `Input.open: bool` (`#[serde(default)]`) and one property in
   `TaskCreate::input_schema`: `"open": {"type": "boolean", "description": "..."}`, the description **12 words at
   most**, stating facts, e.g. "Brief only, no arrangement; compiled without owner material; may leave Claude." Do
   **not** change the tool's long description (it is model-visible text; the first-request tests must pass
   unchanged, and an extra sentence there costs every turn that loads it).
3. **The refusals, in words** (in `input_of` for `task.create`, and at the top of `task_start` for `task.start`;
   each is a sentence the caller reads, as the module's other refusals are):
   - `open` with `check_of`: "an open task cannot be a check: a check reads its task's claim, which is not judged
     open". (`open` and `profile` then also refuse each other through the existing rule that `profile` needs
     `check_of`.)
   - `open` with `arrangement`: "an open task carries only its brief: leave out `arrangement` and write what it needs
     into `brief`".
   - `open` with `quotes` (`task.start`): "an open task carries only its brief: leave out `quotes`".
   - `open` without `brief` (a plan item): "`open` is for a task session: give it a `brief`" (in the `let Some(brief)
     = &i.brief else` branch beside the `check_of` refusal).
4. **Brief only, from any parent.** In `create`, an open task skips `arranged(..)` (pieces empty, humans 0) and writes
   **no** arrangement node and no `VIA_ARRANGEMENT` edges; `TaskOf.arrangement` is `None`. The brief node and its
   `DerivedFrom` edge to the holder reply stay (the edge is provenance, not content the task reads). Check
   `brief_text(..)`: whatever it adds around the brief is the harness's own words, never the parent's content (say so
   in the report, or cut it to the harness's words for an open task). In `task_start`, an open task has no quotes,
   so `nodes_of` writes no arrangement. The graph record (`graph_record`) for an open task takes its objective from
   the brief and no pieces. The parent's external hold stays exactly as today (the brief may carry outside text;
   that is a different rule and stays on).
5. **Opened open, in the opening frame.** For an open task, both paths set, on the `SessionRecord` they build inside
   the frame closure, `rec.data = Some(Box::new(DataState { opened: Some(Opened { how: <task>, at_ms: now }), run:
   None, taint: None }))`, and push the `session.data` `opened` row (the floor row's writer) into the same `records`
   vector, so the session and its row are written in one frame or not at all. A task without `open` writes neither
   (its `data` stays `None`, which reads as the private view).
6. **Nothing else changes.** The task runs on the model it would run on today (the parent's, or `task.start`'s
   `profile`): choosing a restricted draft profile is theseus-a1wvq.13's, and judging the brief before the first
   compile is theseus-a1wvq.8's. Do not touch routing, `data_floor`, `Gated`, the compiler or the view.

## Tests (each with a planted revert; the first is the key test)
Put the new tests in a new `crates/theseus-core/src/tests_open_task.rs` (one `mod` line where the other `tests_task_*`
modules are declared), reusing `tests_task_start.rs`'s and `tests_task_brief.rs`'s helpers (make one `pub(crate)` if
you need it, never copy it).
- **`each_open_task_writes_opened`**: `task.start { open: true }` and a model's `task.create { open: true }` (drive it
  as the existing `task.create` tests do, through the fake provider's tool call) each give a task session whose stored
  `SessionRecord.data` has `opened` set (`how` = task, `at_ms` the kernel's now), `run: None` and `taint: None`, and
  exactly one `session.data` row `opened` naming that session. The same calls without `open` leave `data` `None` and
  write no `session.data` row. Plant: skip the row (and the field) on the `task.start` path; this test must fail. A
  second plant on the `task.create` path must fail it too. Show both.
- `an_open_task_carries_only_its_brief`: a parent conversation with three messages (invented text), one of them
  carrying a marker string; an open task started from it (both paths). The task session's transcript holds the brief
  node and nothing else: no arrangement node, no node whose text holds the marker, no `VIA_ARRANGEMENT` edge;
  `TaskOf.arrangement` is `None`. Compile its first request (as `tests_task_brief.rs` does) and assert the marker is
  absent from the request body. Plant: let an open task call `arranged` as before (with an arrangement given): it
  must fail.
- `open_and_check_of_refuse_each_other`: `task.create { open, check_of }` is refused with the sentence above and
  opens nothing (no session, no row); likewise `open` with `arrangement`, `task.start { open, quotes }`, and `open`
  with no brief. Plant: drop the `check_of` refusal.
- The protocol crate's TS test passes with the regenerated `TaskStartParams.ts`; a JSON round trip of
  `TaskStartParams` without `open` is byte-identical to today's (the field is skipped when false).
- The first-request tests (`tests_first_request.rs`) and `tests_m3::a_plain_turn_stays_within_its_frame_budget` pass
  unchanged. Say the size of `task.create`'s schema before and after (bytes of its JSON) in the report.

## Beside you (other sessions on other branches; stay out of their files)
- **cn-open-view (theseus-a1wvq.7)**, branch `cloud/20261021-cn-open-view`, also from `integ/cn`: it makes the
  compiler, recall and the tool offer read `data.opened` (the field you set) and choose the open view. It owns
  `ceiling.rs`, `places.rs`, `context_parts.rs`, `standing.rs`, `ontology.rs`, `compiler/`, `prefix*`, `recall*`,
  `toolrun*`, `turn/compile_step.rs`, `turn/route_step.rs`, `config/routing_data.rs`. **The contract between you is
  the field alone:** an open task's session has `data.opened` set in its opening frame, and nothing else. Do not read
  or test the view.
- **cn-routing-compose (theseus-a1wvq.4)**, branch `cloud/20261021-cn-routing-compose`, from `main`: `routing.rs`,
  `config/routing.rs`, a new `config/routing_cn.rs`, one `cleared: false` line each in `turn/route_step.rs`,
  `turn/route_step/correct.rs` and `turn/brief_step.rs`, one call line in `Config::validate`, the routing passage of
  `crates/theseus-core/AGENTS.md`. You touch none of them.
- If a change you want sits in one of those files, leave it and name it in the report.

## Not in this row
Judging the brief (theseus-a1wvq.8: Jev's data question on the brief before the first compile, and the pass); the open
view itself (theseus-a1wvq.7); a draft task on a restricted profile, its tree and its Claude review
(theseus-a1wvq.13); the footer and route-row fields (theseus-a1wvq.11). Open places, open threads, a session-wide
`--open` flag, a cockpit toggle and a Discord `/thread open` were **dropped** by the owner's update; do not build them
(a CLI `theseus task start --open` flag is a follow-up: name it in the report, do not add it).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing or unparsable is private, never open. A task is open only when
  `open` is exactly `true`.
- **On, measured, with a named rollback.** The field is off unless a caller sets it, so the rollback is reverting this
  row's commits (name them in the report); a test shows a task without `open` is as before.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). The one model-visible addition is the
  `open` property's description, 12 words at most. FAST: nothing new on a plain turn.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` (your base, not `main`) and paste its
  output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **The lane gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passes on your last commit (run the
  touched crates' tests first: `cargo nextest run -p theseus-core -p theseus-protocol`). Say what it printed.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Never write the owner's company or private project names. Every fixture is invented ("Acme", "Widgetco").
- **The data rule binds this session too.** Your own model is Claude. No request of yours goes to a GLM or DeepSeek
  endpoint, and there is no key for one on the VM; never add one.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md` has the data rule's
  invariants and the task rules; `crates/theseus-protocol/AGENTS.md` has the wire and TS rules). Keep CLOUD_REPORT.md
  honest: what you ran, what passed, what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup:
  about 33 L1 tests fail on this VM; this row should need none of them: say so).
- Out of scope: the owner's config, daemon and machine; any benchmark run; `docs/status.md`, the README,
  `docs/design/` and the spec (the maintainer writes those: say in the report what should change).

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its size
after the commit, it must not be empty) with: what changed, file by file; each test and its plant (the line broken, the
failure seen, the restore); the `task.create` schema's size before and after; what could not run on the VM and the exact
commands for the maintainer; the keel guard's output; "store format: unchanged"; the follow-ups (the CLI flag, anything
you left in another row's file). Push the branch, and end with a short final message that starts
`CLOUD REPORT COMPLETE`.
