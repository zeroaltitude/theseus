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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-observability`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: cn-observability, the data rule on every route row, footer, provider call and bench report: data verdict, clearance, gate, provider, model, origin (theseus-a1wvq.11)

Branch: `cloud/20261021-cn-observability`. Every commit's subject carries `theseus-a1wvq.11`. Deadline for the report: 4
hours after you start.

## Your base: `integ/cn` with the open request in (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. Where the note above says your base is
`main`, or that the store format is 47, this section is right. `integ/cn` is `main` plus these China-origin rows, each
its own merge:
- the data floor and the session's data state (theseus-a1wvq.3, .5): `crate::data` (`DataPass`, `DataWhy`,
  `Clearance`, `DataState { opened, run, taint }`, `OpenRun`, `Taint`), `data_gate.rs` (the provider chokepoint and
  its `data_gate.blocked` row), `turn/data_step.rs` (`Floor { sensitive, cleared }`, `RouteState::floor()`,
  `floor_refuses`, `data_floor`, `floor_unrouted`, `TurnRunner::data_pass`), store format 48;
- the private mark for background steps (theseus-a1wvq.10, first slice);
- routing's compose (theseus-a1wvq.4): `Profile.restricted`, `Ask.cleared`, `ModeProfiles.cn_profiles` / `.cn_after`,
  `RoutingConfig::effective(mode, cleared)` and `table(mode)` (`config/routing.rs`), `Reason::DataGate`;
- open tasks (theseus-a1wvq.6) and the open view (theseus-a1wvq.7, `open_view.rs`);
- **the open request (theseus-a1wvq.8)**, which this row reads: `data::admits` and `data::Check`, `RouteState.admitted:
  Result<OpenRun, DataWhy>` (it replaced `RouteState.clearance`; `Floor.cleared` reads `admitted.is_ok()`),
  `data::Carried` (the per-request input: whether the compile used the open view, and the transcript positions of the
  nodes the request carries), `data::clearance(run, carried) -> Clearance`, and the per-request pass that
  `TurnRunner::data_pass` puts on each loop's request before `call_model`, with each request's clearance recorded on
  the turn. **Name note:** older plans call the per-request input `data::OpenRequest`; `.8` named it `data::Carried`,
  because `.7` had taken `OpenRequest` for the compiled open request.

**Check first**: `git log --oneline | grep -E 'theseus-a1wvq\.(3|4|5|6|7|8)'` shows commits of each;
`grep -n 'admitted' crates/theseus-core/src/turn/route_step.rs crates/theseus-core/src/turn/data_step.rs` finds
`RouteState.admitted`; `grep -n 'pub struct Carried\|pub fn clearance\|pub fn admits' crates/theseus-core/src/data.rs`
finds the other three. If `.8`'s commits or any of `RouteState.admitted`, `data::Carried`, `data::clearance` or the
per-request pass in `data_pass` are missing, stop, write that in `CLOUD_REPORT.md`, push it, and end. If the join
renamed one (or `.8`'s report changed a signature), use the code's names and say so in your report's first section.
Read `.8`'s commit messages and its `CLOUD_REPORT.md` section "contract names as built" before you start, and its
`crates/theseus-core/AGENTS.md` paragraph on where an open request is built.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48). **This row bumps
nothing**: it adds keys to ledger rows (JSON, read by key) and an optional field to a wire type; `tests_layouts.rs`
holds no `route.decided` or `provider.call` sample. If you find you must change a stored record's layout, stop and say
why in the report instead.

**Beside you** (other rows on the same base, in parallel; stay out of their files where you can, and keep your edits
small and additive where you can't, so the joins merge):
- cn-tool-screen (theseus-a1wvq.9): `data::screen` and the tool loop's boundary in `turn.rs` / `turn/*_step.rs`. A
  screened tool result extends the open run; the word `tool_result` is already a `DataWhy`.
- the rest of cn-background-gate (theseus-a1wvq.10): compaction, consolidation, keep-warm, the own-bench replay and
  the other background callers obey the gate, each skip with its own `DataWhy` and its own row.
- After you: cn-draft-review (theseus-a1wvq.13) adds a `draft_skip` key to the `route.decided` row in
  `record_route`; keep your new keys grouped in one place there so its join adds one line beside them.

## Why
A China-origin model can now answer a request (the open-request row), and nothing says so. The `route.decided` row
(`crates/theseus-core/src/fact/route.rs`, `RouteDecided`, written by `record_route` in `turn/route_step.rs`) names the
mode, the profile and the reason, but not Jev's data verdict, whether the turn was admitted, why a restricted profile
was skipped, or the provider, model and origin it ran on (those live on `turn.started` / `turn.ended`, so a reader
must join). `provider.call` (`fact/turn.rs`, `ProviderCall`) has no origin and does not say whether the request carried
a pass. The footers (Discord `routed()` in `crates/theseus-discord/src/runtime/route.rs`, the CLI's `status_line` in
`crates/theseus/src/render.rs`) say nothing when a turn ran open or was kept off a restricted model. The own-bench
report (`route.report`, `own_bench/report.rs`) counts turns per (model, effort) cell with no origin or data column. So
the benchmark rerun (theseus-lgakz) cannot say how model selection changed: turns per origin, per data verdict, gate
skips per reason, and the spend moved to China-origin profiles. The owner's update (2026-10-17, verbatim): "I think
that Chinese models in dms can be evaluated by content rather than by the nature of the channel. My understanding is
also that they do have coding skills that are worth applying at their cost -- I'd always want to do final review and
fixes with a more competent model." Its third part (D3) is "measure it": this row is that measurement for turns.

## Risk
Low to medium: read-side and record-side only. No gate decision changes: this row only reads `admitted`, the floor
and the per-request pass, and must never compute a clearance of its own. The risks are a footer that changes for a
plain turn (it must not), a row read that costs a plain turn time (it must not), and a merge with cn-draft-review in
`record_route`.

## Prerequisite: `integ/cn` with theseus-a1wvq.8 merged (not `main`), as "Your base" says.

## Your row

### 1. `route.decided` gains the data rule's keys (`fact/route.rs`, `turn/route_step.rs`)
Add to `RouteDecided` (keep the new fields together, e.g. one `data: Option<RouteData<'a>>` beside one `ran:
Ran<'a>`, your call) and write these keys in `row()`:
- `provider`, `model`, `origin`: of the profile the turn runs on (the row's `profile`). Resolve the profile with the
  config (`Config::all_profiles()`, then the profile's `provider` and `model`), and the origin with
  `Config::profile_origin(profile, &catalog)` (`ModelOrigin`, written snake_case: `us`, `eu`, `cn`, `unknown`). A
  profile that does not resolve writes `origin: "unknown"` and null provider/model (fail closed in words too).
  Always written, on every `route.decided` row.
- `data`, `data_confidence`: Jev's data answer (`Verdict.data`: `DataAnswer.choice`, `.confidence`) when the verdict
  carries one; absent otherwise.
- `clearance`: `admitted` when `RouteState.admitted` is `Ok`, else `closed`. The route row is written before the
  request is built (`record_route` runs in `compile_first` and on `compile_routed`'s floored branch, before the
  detour's compile and before `data_pass`), so it records the turn's admission, never a pass. Whether a request
  actually went out with a pass is item 3's `provider.call` key.
- `data_gate`: why the data rule kept the turn off a restricted-origin profile, written **only** when it did:
  - the floor moved the decision (`Reason::DataGate` from `data_floor` / `floor_unrouted`): the turn's `DataWhy`
    word from `admitted`'s `Err`, or `.8`'s reason for an admitted turn that cannot be built open (`private_view`),
    or `sensitive` when the names' floor (`Floor.sensitive`) moved it, not the clearance;
  - or the verdict's mode has restricted profiles in its `cn_profiles` (`cfg.routing.table(mode).cn_profiles`, non
    empty) and the turn was not admitted, so `effective(mode, false)` left them out: the `DataWhy` word.
  The field is an open string, never an exhaustive match: a later row may write its own word (the tool screen's
  `tool_result` is already a `DataWhy`; the background rows add none to this row).
  A turn whose mode has no `cn_profiles` and whose decision did not move for data writes no `data_gate`: a plain
  Claude turn's row gains only `provider`, `model`, `origin` and `clearance`.
- The checkpoint's row (`turn/checkpoint_step.rs`, its `RouteDecided`): fill `provider`, `model`, `origin` of the
  profile it moves to, and leave the data keys absent (a checkpoint has no data verdict).
- `record_route` is an associated function today; give it what it needs (the resolved provider/model/origin and the
  data fields) from the call sites, or make it a method, whichever adds fewer lines. Nothing here may read the store
  or the transcript: it is config and catalog lookups the turn already holds.
- `narrate`: one short clause when `data_gate` is set ("kept off a restricted-origin model: <why>"), nothing else
  new.

### 2. `TurnRoute.data` (`crates/theseus-protocol/src/route.rs`)
Add to `TurnRoute`:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
#[cfg_attr(test, ts(optional))]
pub data: Option<TurnData>,
```
and, in the same module file, `pub struct TurnData { verdict: Option<String>, clearance: String, gate: Option<String>,
origin: String }` (same derives and ts attributes as `RoutePersona`; doc each field in one line). `clearance` is
`admitted` or `closed` as the row says, and becomes `open` when any request of the turn carried a pass (item 3).
`record_route` fills it from the same values as the row; the checkpoint's `TurnRoute` (`checkpoint_step.rs`) keeps
`data` as the turn had it. Every other constructor of `TurnRoute` (the Discord tests in `runtime/route.rs`) takes
`..Default::default()` or `data: None`. `crates/theseus-protocol/src/lib.rs` is not touched (three lines of room):
`TurnData` lives in `route.rs`, which `lib.rs` already declares. Regenerate the cockpit's types with the protocol
test (`cockpit/src/protocol.gen/`, `the_cockpits_types_are_generated_from_the_rust_ones`; read
`crates/theseus-protocol/AGENTS.md` for the update flag) and `git add` the new `TurnData.ts`.

Add one method beside `RoutePersona::words`, so both footers word it the same:
`impl TurnData { pub fn footer(&self) -> Option<String> }`, returning `open` when `clearance == "open"`, `data gate:
<gate>` when `gate` is set and the turn did not run open, and `None` otherwise.

### 3. The pass per request: `provider.call` and the turn's result (`fact/turn.rs`, `turn.rs`)
- `ProviderCall` gains `origin` (the origin of the profile the call ran on, as item 1 resolves it; the turn knows
  its target at `turn.rs`'s `fact::turn::ProviderCall { .. }` construction) and, when the origin is restricted
  (`ModelOrigin::restricted()`), `pass: bool` (the request carried a `data_pass`, read off the request that was sent:
  `compiled.request.data_pass.is_some()`) and, when it did, `run_nodes` (how many transcript nodes the request
  carried, from `.8`'s `Carried` of that request, as `.8` records it on the turn). A request to a US or EU model
  writes `origin` only. Do not mint, rebuild or re-check a pass: read what `data_pass` put on the request.
- After `Self::data_pass(t, &mut compiled)` in the loop (`turn.rs`, before `call_model`), when the request carries a
  pass, set the turn's `TurnRoute.data.clearance` to `open` (create the `TurnData` from the route row's values if a
  turn that asked no route has none yet). This is the footer's "per request" word: a turn shows ` · open` only when a
  request of it actually left with a pass. `turn.rs` has room under its ceiling (3,409 / 3,525 on the base), but
  keep it to a few lines: a helper in `turn/data_step.rs` called from there is best.

### 4. The footers
- Discord `routed()` (`theseus-discord/src/runtime/route.rs`): append ` · <footer>` from `TurnData::footer` to the
  words it builds, and return it on its own (`routed: open`, `routed: data gate: verdict_private`) when the turn has
  no mode, source or persona. Every footer without `data`, or with a `data` whose `footer()` is `None`, is
  byte-for-byte unchanged.
- The CLI's `status_line` (`crates/theseus/src/render.rs`): the same ` · <footer>` at the end of the route words.
  `render.rs` is at 3,115 of 3,128: keep the change to a few lines (a call to `footer()` in the existing `route`
  closure), and its tests beside the existing status-line tests or in a `render/` module file if that is where they fit.

### 5. The metric (`telemetry/metrics.rs`, `telemetry.rs`)
One counter beside `theseus.route.wait` (`ROUTE_WAIT`): `theseus.route.data_gate`, "turns and calls the data rule
kept off a restricted-origin model, by caller and why", attributes `theseus.data_gate.what` and
`theseus.data_gate.why` (open strings). The design named it `theseus.routing.data_gate`; the existing route metrics
are `theseus.route.*`, so take this name and say so. `Telemetry::record_data_gate(&self, what: &str, why: &str)`
beside `record_route_wait`. Callers in this row: `record_route` when it writes `data_gate` (what = `route`), and the
chokepoint's refusal where it writes its `data_gate.blocked` row (`data_gate.rs`; what = that row's caller) if the
telemetry handle reaches it without new plumbing; if it does not, leave the chokepoint and say so. The background row
(.10) calls the same function with its own `what` (`compaction`, `keep_warm`, ...): nothing in this row waits for it.

### 6. The own bench: `origin` and `data` columns (`own_bench/report.rs`, `own_bench/sampler.rs`, protocol `own_bench.rs`)
- The report's turn read (`Core::turns_since` in `sampler.rs`, which already reads `route.decided` for `effort_ran`)
  also reads the row's `origin`, `data` and `data_gate` per turn. Carry them on the report's `Turn` (`report.rs`)
  rather than on the sampler's `Candidate` if the sampler does not need them. A turn whose row predates this row has
  no `origin`: count it as `unrecorded`, never as `unknown` (which is a real, restricted origin).
- `RouteCell` (protocol `own_bench.rs`) gains `origin: String`: the cell's turns' origin, `mixed` if they differ.
- `RouteReportResult` gains `data: Vec<DataCell>` (per origin and data verdict: `origin`, `data` (`open`, `private`,
  `unclear`, or `none` without a data answer), `turns`, `spend_usd`), and `gates: Vec<GateCount>` (`why`, `turns`),
  both sorted for stable output; the restricted origins' spend is the "spend moved to China-origin profiles" line.
  New types in `own_bench.rs` (250 lines; no ceiling), with the same ts attributes as `RouteCell`.
- The CLI's `report_lines` (`crates/theseus/src/render/own_bench.rs`) prints the origin column on each cell line, then
  one line per data cell and one line of gate skips; nothing when the window has no rows with the new keys beyond
  `unrecorded`. Regenerate the cockpit's types.

### 7. Docs
`crates/theseus-core/AGENTS.md`: two sentences in the data floor's passage: which row carries the data rule's keys
(route row: admission and gate; `provider.call`: origin and pass per request) and that `data_gate` is an open word.

## Tests (each with a planted revert; the first is the key test)
Core tests go in a new `crates/theseus-core/src/tests_observability.rs` (one `mod` line in `lib.rs`), reusing `.8`'s
rigs (`tests_open_request.rs`: a restricted profile `dsflash` on a fake provider the catalog judges `cn`, no Haiku in
`trivial.profiles`, `trivial.cn_profiles = ["dsflash"]`, the route pack acting with a scripted verdict) and the
floor's (`tests_data_floor.rs`).
1. **`route_decided_records_data_clearance_gate_provider_model_origin`** (T29, key). Two turns in the owner's DM:
   (a) a message judged `open` at act, mode `trivial`: its `route.decided` row has `data: "open"`, a
   `data_confidence`, `clearance: "admitted"`, `provider` / `model` of `dsflash`, `origin: "cn"`, and no `data_gate`;
   (b) a message judged `private`, mode `trivial`: `data: "private"`, `clearance: "closed"`, `data_gate:
   "verdict_private"`, and the provider, model and origin of the profile it ran on (`us`). Plants: (a) drop `origin`
   from `row()`: the test fails on (a) and (b); (b) write `data_gate` only on `Reason::DataGate`: the test fails on
   (b), whose decision did not move but whose `cn_profiles` were left out.
2. `a_plain_turns_route_row_gains_only_where_it_ran`: a Claude turn with no `cn_profiles` in its mode: the row has
   `provider`, `model`, `origin: "us"`, `clearance`, and no `data`, `data_confidence` or `data_gate`. Plant: write
   `data_gate` whenever the turn is not admitted.
3. `the_floor_move_names_its_why`: a pinned restricted base on an unadmitted turn moves to the `[model]` default
   (`Reason::DataGate`): the row's `data_gate` is the admission's word; on a sensitive-name turn it is `sensitive`.
4. `provider_call_records_origin_and_the_pass_per_request`: the open DM request's `provider.call` row has `origin:
   "cn"`, `pass: true` and `run_nodes` equal to the nodes the fake received; a Claude call's row has `origin: "us"`
   and no `pass` key; a later loop that `.8` moved off the restricted model (after a tool result) writes its own
   origin. Plant: read `pass` from the turn's admission instead of the request: the moved loop's row is wrong.
5. `turn_route_data_round_trips` (protocol `route.rs` tests): `TurnRoute` with and without `data` round-trips; a JSON
   without `data` reads as `None` (old clients' results).
6. The footers: Discord `routed` (beside the existing tests in `runtime/route.rs`): `routed: trivial · open`,
   `routed: trivial · data gate: verdict_private`, `routed: open` with no mode, and the existing strings unchanged
   with `data: None` and with a `data` whose `footer()` is `None`. CLI `status_line`: the same two words at the end of
   the route words, and a plain turn's line byte-for-byte unchanged. Plant: return `None` from `footer()` for `open`.
7. `the_turns_footer_says_open_only_when_a_request_carried_a_pass`: an admitted DM turn whose mode is not a detour runs on
   the `[model]` default (`.8`'s `a_non_detour_mode_in_a_private_view_session_never_builds_an_open_request` shape; no
   pass sent): its result's clearance is `admitted`, not `open`, and its footer has no ` · open`. Plant: set `open`
   from `admitted` in `record_route`.
8. `a_data_gate_turn_counts_once_on_the_route_data_gate_metric` (beside the metrics tests that read `theseus.route.wait`).
9. Bench: `the_report_counts_turns_per_origin_and_data_and_gate_skips` (pure, in `report.rs`'s tests, from planted
   rows: turns and spend per origin and verdict, gate skips per reason, a mixed cell, an old row counted as
   `unrecorded`), and a `report_lines` test in `render/own_bench.rs`. Plant: count `unknown` for a row without
   `origin`: the old row lands in the wrong cell.
Run unchanged and passing: `tests_first_request.rs`, `tests_data_floor.rs`, `tests_open_view.rs`,
`tests_open_task.rs`, `tests_open_request.rs`, `tests_route.rs`, the own-bench tests, the Discord route tests, the
render tests, and `tests_m3::a_plain_turn_stays_within_its_frame_budget`.

## Not in this row
- **The draft columns, moved to cn-config-guidance (theseus-a1wvq.12)**, which runs after cn-draft-review (.13):
  the bench report's draft columns from `task.draft` (draft model and cost, review model and cost, lines the reviewer
  changed, applied), the `draft_skip` key on the route row (cn-draft-review writes it; .12 reads it into the report),
  and the test `the_report_counts_drafts_and_their_review_cost`. Do not build them, and do not read `task.draft`.
- The tool screen (.9): until it joins, a tool result ends the run, so `run_nodes` never counts one; after it, a
  screened result is a carried node and is counted with no change here.
- The background callers' gate (.10): their skips write their own rows with their own `DataWhy`; they call item 5's
  `record_data_gate` with their own `what`. This row adds no word for them.
- The session's data line in `inspect` and health's open and tainted counts (design §7.4): not here.
- Jev judging Theseus's own replies (theseus-a1wvq.15), the late grace, `session.data_clear`: not here.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code).
  Stored data stays readable: an old row without the new keys reads as `unrecorded`, never as an error.
- **Fail closed, always.** This row only records. It never computes a clearance, mints or rebuilds a pass, or
  changes a route: read `RouteState.admitted`, the floor and the request's `data_pass` as `.8` left them. An origin
  that does not resolve is `unknown`.
- **On, measured, with a named rollback.** The keys and columns are on by default. The rollback is the config: with no
  restricted profile in any `cn_profiles` and no open task, every footer is byte-for-byte what it was and no row gains a
  `data_gate`, `pass` or `run_nodes`.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new
  on a plain turn's path but config and catalog lookups (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move); nothing reads the
  store or the transcript on the path before a request leaves.
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
  invented ("Acme", "Widgetco").
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (core, protocol, discord, the CLI). Keep CLOUD_REPORT.md
  honest: what you ran, what passed, what you could not run in the cloud (L1 needs a non-root daemon with a job
  cgroup: about 33 L1 tests fail on this VM, so say which of your tests you could run and give the maintainer the
  exact commands for the rest).
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core -p theseus-protocol -p theseus-discord -p theseus` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its size
after the commit, it must not be empty). First section: the contract as built, for the rows after you: the
`route.decided` keys and their words, `TurnData` and `footer()`, `provider.call`'s `origin` / `pass` / `run_nodes`,
`record_data_gate(what, why)` and the metric's name, the report's `DataCell` / `GateCount`, and any `.8` name the join
renamed. Then: what changed, file by file; each test and its plant (the line broken, the failure seen, the restore);
what could not run on the VM and the exact commands for the maintainer; the keel guard's output; "store format:
unchanged (48)"; the regenerated cockpit types; the maintainer's live check after the join on a scratch daemon
(`trivial.cn_profiles = ["glm"]`, no Haiku: an open DM message's footer ends `· open`, its `route.decided` row says
`origin: cn`, `clearance: admitted`, and its `provider.call` row `pass: true`; a private message's footer ends
`· data gate: verdict_private`; the CLI's route report shows the origin and data lines), with the rows to read; the
follow-ups. Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
