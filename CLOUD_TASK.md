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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-draft-review`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from the integration branch `integ/cn` at `0bf2dfee` (2026-10-21; integ/cn after its round 3: the open request, cn-tool-screen, cn-observability, cn-bg-rest), not from `main`. Where the
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
## Your task: cn-draft-review, a China-origin model drafts code in its own tree and a Claude model always reviews and fixes it before anything is applied or reported (theseus-a1wvq.13)

Branch: `cloud/20261021-cn-draft-review`. Every commit's subject carries `theseus-a1wvq.13`. Deadline for the report: 5
hours after you start.

## Your base: `integ/cn` with the open request, the tool screen, the background rest and observability in
Your branch starts from the integration branch `integ/cn`, **not** from `main`. Where the note above says your base is
`main`, this section is right. `integ/cn` is `main` plus the China-origin rows, each its own merge. You build on:
- the data state (.3, .5): `crate::data` (`DataState { opened, run, taint }`, `Opened { how: OpenHow, at_ms }` with
  `OpenHow::Task` and **`OpenHow::Draft`**, declared for this row and unused so far; `data::opened_row`,
  `data::open_task`), `turn/data_step.rs`;
- routing (.4): `[routing.classes.<class>]` is `ClassProfiles` in `config/routing.rs` (`deny_unknown_fields`),
  `RoutingConfig::table(mode)`, and `config/routing_cn.rs` `validate`, whose refusal already names "`cn_draft`, a
  later key";
- open tasks (.6) and the open view (.7, `open_view.rs`: `Open { roots, within }`, `Open::offers`, `Open::refusal`,
  `runs_tool`, and `open_roots(cfg, session)`, whose doc says "theseus-a1wvq.13 adds a draft task's own tree here,
  from the task's id");
- the open request (.8): `data::admits`, `RouteState.admitted`, `data::clearance`, and `turn/open_step.rs`
  (`data_move`: a later loop whose request leaves the run **moves** to the `[model]` default with
  `Reason::DataGate`; `data_end`: the stored run and an open session's taint);
- the tool screen (.9): `data::screen`, `Unscreened`, `RouteState.screened`, `turn/screen_step.rs`. **A program's
  result (`proc.run`, `job.*`, `term.*`) never passes the screen** (theseus-a1wvq.16 is the fix, not yours);
- observability (.11): `route.decided`'s data keys, grouped in `record_route` (`fact/route.rs` `RouteDecided`).

**Check first**:
- `git log --oneline | grep -E 'theseus-a1wvq\.(8|9|11)'` shows commits of each;
- `grep -n 'pub fn screen\|pub enum Unscreened' crates/theseus-core/src/data.rs` finds the screen;
- `grep -n 'Draft' crates/theseus-core/src/data.rs` finds `OpenHow::Draft`;
- `grep -n 'data_gate\|clearance' crates/theseus-core/src/fact/route.rs` finds .11's keys.

If `.8`'s or `.9`'s commits, `data::screen`, or `OpenHow::Draft` are missing, stop: write that in `CLOUD_REPORT.md`,
push it, and end. If `.11`'s keys are missing, go on and put `draft_skip` beside the row's other optional keys; say so.
If a join renamed a name below, use the code's name and say which in the report's first section (each earlier row's
`CLOUD_REPORT.md`, in `git log --all -- CLOUD_REPORT.md`, names its contract as built). The background rest (.10) may
not be in; go on without it and say so.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`. **This row bumps nothing.** A
draft is known by its stored data state (`data.opened.how = draft`), its review by `TaskOf.check`, and the rest goes in
ledger rows (JSON, read by key). If you find you must add a field to a stored record, stop and say why in the report.

## Why
The owner's update of 2026-10-17 (verbatim): "My understanding is also that they do have coding skills that are worth
applying at their cost -- I'd always want to do final review and fixes with a more competent model." Today a
China-origin model answers only a `trivial` detour: `routing_cn::validate` refuses `cn_profiles` on the coding classes
by design, so no coding ask reaches one. The cheap drafts the owner asked for do not exist, and the bench arm the
owner wants (China-origin draft plus Claude review, against Claude alone) cannot run.

## Risk
High in kind: a China-origin model's code reaches the owner's working tree. Three guards each hold alone:
- the draft works only in its own git worktree;
- nothing is applied, and the draft's own report is never posted, until a Claude review has ended;
- the apply never touches the owner's index or leaves conflict markers.

The rest is the existing floor: the draft's requests are open requests, with a pass per request and the screen on
its tool results.

## Prerequisite: `integ/cn` with `.8`, `.9` and `.11` merged (not `main`), as "Your base" says.

## Your row

### 1. Config: `cn_draft` and `final` (`config/routing.rs`, `config/routing_cn.rs`)
Add to `ClassProfiles`, each skipped when unset:
```rust
pub cn_draft: Option<String>,          // TOML `cn_draft`
#[serde(rename = "final")]
pub final_review: Vec<String>,         // TOML `final`
```
So the operator writes `[routing.classes.routine_coding]` `cn_draft = "dsflash"` and `final = ["sonnet"]`. **These
two key names are a contract:** the config-guidance row (theseus-a1wvq.12) builds its template and docs on them, and
its key test plants a misspelt `cn_draft`. Never rename them. The code defaults leave both unset everywhere.

Load checks (in `routing_cn::validate`, or a sibling file if that one grows past its room), each refusal naming the
key and the way out:
- both keys only on `routine_coding` and `deep_coding`;
- `cn_draft` names a configured profile of **restricted** origin (`Config::profile_origin(..).restricted()`). A Claude
  profile there is refused: it belongs in `profiles`;
- `final` is required and non-empty when `cn_draft` is set, and refused when it is not;
- every `final` entry is a **Claude** profile: configured, origin `us`, and its model a Claude catalog row. Find how
  the catalog marks a Claude row and use that mark; if there is none, use the provider endpoint's origin plus a model
  id beginning `claude-`, and say so. The refusal reads: "routing.classes.routine_coding.final: dsflash is not a Claude
  profile: a Claude model reviews every draft".

`final` is a first-usable list, as a class's `profiles` is: `["sonnethi", "opus"]` means Sonnet high, else Opus.

### 2. Eligibility (new `crates/theseus-core/src/draft.rs`, one `mod` line in `lib.rs`; the turn hook in a new `turn/draft_step.rs`)
After the route decision of a turn's first loop, read the verdict's class. When its table has `cn_draft`, the turn
drafts only when all of these hold:
- the input is admitted (`RouteState.admitted.is_ok()`);
- the session is not a task, and the turn is not a wake's or a report's turn;
- the session's `dir` is under the open roots (`open_view::open_roots`, canonical, `paths::within`);
- the dir is a git work tree;
- the `cn_draft` profile is usable now (`Profile::runs` for a cleared ask);
- a `final` profile is usable now.

When one fails, the turn runs exactly as today, and its `route.decided` row gains `draft_skip`, the first failing
word: `not_admitted`, `in_task`, `wake`, `dir_not_open`, `not_git`, `draft_unusable`, `no_reviewer`. Write
`draft_skip` only when the class has `cn_draft`. A plain turn, and any turn whose class has no `cn_draft`, pays one
branch and writes nothing new. The config-guidance row reads `draft_skip` into the bench report, so keep the words
exactly.

### 3. The draft: an open task in its own tree, with no model call in the parent turn
The eligible turn makes **no model call**. It opens the draft and ends with one core-written line. Find how the core
already ends a turn with its own words and no call (a refusal's line, a `/stop` answer), and use that path.

The draft is a task of the session, opened as `task.start` opens one (`rpc/task_start.rs`, `Kernel::open_task_owned`:
the carve, the depth rule and the rows in one frame). It runs on the `cn_draft` profile, with:
- **its data state** `Opened { how: OpenHow::Draft }` and its `session.data` `opened` row (generalise
  `data::open_task` to take the `how`; do not copy it);
- **its own tree**: a git worktree of the session's dir at `HEAD`, at `<state>/drafts/<task short id>` on a new branch
  `draft/<task short id>`. Follow `rsi/tree.rs`'s pattern: every git call through `children::spawn` on the blocking
  pool, never the owner's tree. Record the base commit (`HEAD`'s sha) in the task's `opened` facts or a `task.draft`
  start row, your call. The task's `dir` is the tree;
- **the tree as an open root**: `open_roots` adds `<state>/drafts/<task>` for a draft session, from its id, as its doc
  promises. No other session gets it;
- **its brief, built by the core only from open content.** A fixed header, then the open run's **person** messages
  (`RouteState.admitted`'s run, `from..=through`), verbatim, oldest first. No model writes the brief. Its first turn
  is judged like any open task's brief (`admits` on the brief). The header, exactly:
  "Draft this change in your tree. You can read, search and edit files there; you cannot run programs, so don't try to
  build or test: a Claude reviewer will run the tests, check and finish your work. When you are done, report what you
  changed and what you could not do."

The parent turn's one line: "Drafting on <draft profile> (<draft model>); <reviewer profile> (<reviewer model>)
reviews and finishes it." The catalog has no display names, so use the profile and model names.

### 4. What a draft may and may not do (theseus-a1wvq.16)
A program's result never passes the screen. So if a draft ran `cargo test`, its next loop would leave the draft's
model (`.8`'s move) and run on the `[model]` default. The draft would quietly become a Claude task, its cost and its
measure would be wrong, and the open session would be tainted for good. So, until theseus-a1wvq.16 runs a draft's
programs in a sandbox that sees only the open roots:
- **May**: the file tools in its tree (`fs.read`, `fs.write`, `fs.edit`, `fs.patch`, `fs.glob`, `fs.grep`, `fs.list`,
  and `git.status`, `git.diff`, `git.log`, `git.show`), `web.search`, and `http.fetch` of a public URL, all through
  the open view's gate and the screen as today.
- **May not**: `proc.run`, `job.*`, `term.*`. A draft session is **not offered** them, even though its dir is under
  the open roots, and the gate refuses them if called (one condition in `Open::offers`, or a field on `Open` for a
  draft; the narrowest change that keeps other open tasks as they are). It cannot commit (no git tool writes), so the
  core commits for it (item 5).
- **Never moves.** Wherever the floor (`data_floor`, `floor_unrouted`) or `.8`'s `data_move` would move a draft
  session's request to the `[model]` default, the draft's turn **ends instead**, with no model call. Reasons: a brief
  not admitted, a screen failure, a taint. The draft has ended; its end opens the review (item 5). A Claude model never
  runs inside a draft session.

### 5. The review: a check on a Claude profile, mandatory
When the draft ends, in any way, the core opens the review. That covers its report, a failed turn, a cancel, a stop
by the screen, or a brief not admitted. The core opens it in the frame right after the draft's end frame, never as a
model turn.
- **The draft's report is never posted to the place.** `TaskOf::target_for` (`session.rs`) returns none for a draft
  session, as it does for a quiet routine run, and the cancel path (`task::cancelled_report`) obeys the same rule.
- **First the core commits the tree**: `git add -A` and a commit "draft by <profile>" in the tree. No commit is made
  when nothing changed. This is the draft commit.
- **The review** is a check of the draft (`TaskOf.check`, a `TaskCheck` basis and its `task.check_opened` row, so it
  shows as a check everywhere). It runs on the first usable `final` profile, its `dir` the draft's tree, under its
  parent's view: it is a Claude model, so in the DM it has the private view and its programs. Reuse `check.rs`. A
  core entry beside `prepare` that takes no `TurnCtx` is fine. The rule that a checked task must have reported does
  not hold here: a draft with no report has the claim "no usable draft: <outcome> (<reason>)".
- **Its brief** is a fixed header, then the same open-run person messages. The header, exactly: "Review the draft's
  change against the request below: read the diff (git diff <base sha>), run the tests, fix what is wrong, and
  commit. If there is no usable draft, do the work yourself. End with one line: `Changed: <n> lines; tests fixed:
  <m>`."
- **Open it once.** Use a correlation id derived from the draft's id, so a rerun finds the review and writes nothing.
  The kernel's open already works that way.
- **No usable reviewer** at that moment (no usable `final` profile, or the parent's spend limit leaves nothing to
  carve): nothing is applied and nothing is reviewed. The place gets one post: "The draft by <draft profile> is on
  branch draft/<short> (not reviewed: <why>), so it wasn't applied. Ask me to review it when that clears." The tree
  stays.

### 6. Apply after the review only
When the review ends, before the frame that ends it:
- the core commits the tree again ("review by <profile>"), unless nothing changed. This is the review commit, the
  tree's final state;
- if the review completed, the core applies `git diff <base>..<final>` to the session's dir: `git apply --check` first,
  then `git apply`. Use **no `--index` and no `--3way`**: never stage in the owner's index, and never leave conflict
  markers. If the check fails, nothing is applied, and the line says where the reviewed change is. An empty diff
  applies nothing ("no change");
- a review that failed or was cancelled applies nothing;
- then the core closes the tree (the worktree is removed; the branch stays).

The review's report reaches the place, as a check's does, followed by one line: "Drafted by <draft profile>,
reviewed and fixed by <review profile>; applied to <dir>." or "...; not applied: <why> (branch draft/<short>)". If the
line cannot ride the report's own post, post it in the next frame, and say so.

### 7. Measure it: the `task.draft` row (`fact/draft.rs`, `LedgerKind::TaskDraft = "task.draft"` in theseus-protocol `ledger.rs`)
Write one row in the frame that ends the review, or with the no-reviewer post. Its keys:
- `draft_task`, `draft_profile`, `draft_model`, `draft_cost_usd`, and `draft_outcome` (`complete`, `failed`,
  `cancelled`, `stopped`, `not_admitted`);
- `review_task`, `review_profile`, `review_model`, `review_cost_usd` (all null when no review ran);
- `lines_changed`: **measured**, `git diff --numstat` from the draft commit to the final, added plus deleted;
- `tests_fixed_reported`: parsed from the reviewer's last line, absent when the line is missing. It is reported, not
  measured, so name it that way;
- `applied` (bool) and `why`.

Costs come from each task's execution budget (`spent_micros`). Narrate the row in one line. The config-guidance row
builds the bench report's draft columns on these keys, so keep them exactly. Regenerate `protocol.gen` if the ledger
kind reaches it.

### 8. Docs
`crates/theseus-core/AGENTS.md`, beside the data floor's passage: one paragraph. A China-origin model only drafts, in
its own tree, with no programs; a Claude check always reviews; nothing is applied or posted before it. Name the two
keys.

## Tests (each with a planted revert; the key test first)
Put them in a new `crates/theseus-core/src/tests_draft.rs` (one `mod` line in `lib.rs`). Reuse the rigs of
`tests_open_task.rs`, `tests_open_request.rs`, `tests_tool_screen.rs` and `tests_check.rs`:
- a fake `dsflash` judged `cn` and a fake Claude `sonnet`;
- `[routing.data] open_paths` at a temp dir, holding a scratch git repo (`git init`, one commit; set a local
  `user.name`/`user.email` in the fixture so commits work on a bare host);
- the route pack acting, with a scripted `routine_coding` verdict whose `data` answer is `open` at act.
1. **`a_cn_coding_draft_is_never_reported_without_a_claude_review`** (key). An open coding ask in the DM drafts on
   `dsflash`: its scripted answer edits a file in its tree with `fs.edit` and reports. Assert:
   - no post of the draft's report reaches the place;
   - a check task on `sonnet` opens with `check_of` the draft;
   - the place's only report is the review's, with the "Drafted by" line;
   - the owner's dir changes only after the review ends.
   Plant: let `target_for` return the target for a draft session. The draft's report is then posted and the test
   fails.
2. `no_usable_reviewer_leaves_the_draft_unapplied_and_asks`: make `sonnet` unusable once the draft has opened. The
   owner's dir is byte-identical; one "not reviewed" post; the branch exists; the `task.draft` row has `applied:
   false`. Plant: apply the tree's diff when no reviewer runs.
3. `a_draft_is_never_offered_a_program`: the draft's request carries no `proc.run`, `job.*` or `term.*` in its tools,
   and a scripted `proc.run` call is refused at the gate. An ordinary open task in a dir under the roots is still
   offered them. Plant: drop the draft condition in the offer.
4. `a_draft_stopped_by_the_screen_ends_and_is_reviewed`: the draft reads a file holding a secret the rig's scrubber
   knows (`secret`). The draft's turn ends with no request to the `[model]` default, and the review opens with "no
   usable draft". Plant: let `data_move` move a draft (the default fake gets a request).
5. `draft_and_review_costs_are_recorded`: the `task.draft` row has both tasks, profiles, models and costs,
   `lines_changed` equal to a fixture diff's numstat, and `tests_fixed_reported` from the reviewer's last line.
   Plant: count only added lines.
6. `final_must_name_claude_profiles` and `cn_draft_only_on_coding_classes` (pure, config): `final = ["dsflash"]`,
   `cn_draft = "sonnet"`, `cn_draft` without `final`, and `cn_draft` on `chat` are each refused with the key named;
   the template still loads. Plant: skip the Claude check.
7. `an_ineligible_coding_turn_runs_as_before`: a dir outside `open_paths`, a dir that is not a git tree, a private
   verdict, and a task's own turn each run on the class's `profiles` and write the right `draft_skip`. A class with no
   `cn_draft` writes no `draft_skip`. Plant: skip the `dir_not_open` check.
8. `the_draft_brief_holds_only_the_open_run`: an earlier DM message judged private is absent from the draft's brief
   and from every request to `dsflash`. Plant: build the brief from the whole transcript.
9. `a_review_conflict_applies_nothing`: the owner edits the same line in the dir while the draft runs. `git apply
   --check` fails, the dir is unchanged, the owner's index is unchanged (`git diff --cached` is empty), and the line
   names the branch. Plant: apply with `--3way`.

Run these unchanged; they must pass: `tests_open_task.rs`, `tests_open_request.rs`, `tests_tool_screen.rs`,
`tests_open_view.rs`, `tests_check.rs`, `tests_task_start.rs`, `tests_first_request.rs`, `tests_route.rs`, and
`tests_m3::a_plain_turn_stays_within_its_frame_budget`.

## Line room (check before you edit)
`scripts/long-files.txt` sets the ceilings; an unlisted file stays at 2,500 lines or fewer. `turn.rs` (ceiling 3,525)
gets at most a `mod` line and one call; the rest goes in `turn/draft_step.rs` and `draft.rs`. Other files take small
hunks; new tests go only in `tests_draft.rs`.

## Not in this row
- **Running a draft's programs (its tests and builds)**: theseus-a1wvq.16, a **local** row (it needs the L1 sandbox,
  a non-root daemon and a job cgroup). It runs an open request's programs in a view that reads only the system and the
  open roots, and lets the screen pass their results. Until it lands, a draft has no programs (item 4), and its Claude
  reviewer runs the tests. When .16 lands it lifts item 4's withholding and the brief's "you cannot run programs".
- The owner's first-pass bug review of a draft by DeepSeek Flash at max effort, whose findings go to the Claude
  reviewer (theseus-a1wvq.17).
- The bench report's draft columns and their test `the_report_counts_drafts_and_their_review_cost`, the template's
  guidance, and the live config: cn-config-guidance (theseus-a1wvq.12), built on items 1, 2 and 7.
- A line for drafts in `theseus tasks` and the cockpit beyond the check's existing line; a sweep that opens a review
  for a draft whose end frame landed but whose review did not (a daemon death between the two frames): name it in
  the report's follow-ups.
- Jev judging replies (theseus-a1wvq.15); background calls (theseus-a1wvq.10).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias.
- **Fail closed, always.** Anything unknown, missing, late or unparsable is closed, never open. If you are unsure
  whether a step may apply, it does not apply. Never loosen a check, a threshold, a test or a ceiling to make a test
  pass.
- **On, measured, with a named rollback.** The code ships on, and the config is the rollback: with no `cn_draft` in
  any class (the code default) no turn drafts, and every existing test passes unchanged.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). The two headers are task briefs. No
  new model tool. FAST: nothing new on a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the
  gate's turn bench must not move).
- Every behaviour change gets a test and a planted revert: break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed. The key test must be shown failing on
  its planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `git fetch origin integ/cn` and then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name, company or private
  project name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture names and paths
  are invented ("Acme", "Widgetco").
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  and what you could not run in the cloud. L1 needs a non-root daemon with a job cgroup, and about 33 L1 tests fail on
  this VM: say which of your tests you could run, and give the maintainer the exact commands for the rest.
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, never a shell heredoc, and check
after the commit that it is not empty. Its sections, in order:
1. The contract as built: the two config keys and their fields, the `draft_skip` words, the `task.draft` keys, the
   draft's tree path and branch, how the review is opened, and any earlier row's name that the code had under another
   name. The config-guidance row (theseus-a1wvq.12) reads this section.
2. What changed, file by file, with each file's line count against its ceiling.
3. Each test and its plant: the line broken, the failure seen, the restore.
4. What could not run on the VM, with the exact commands for the maintainer.
5. The keel guard's output.
6. "store format: unchanged (<n>)".
7. The maintainer's live check after the join, on a scratch daemon with `routine_coding.cn_draft = "glm"` (standing
   in for dsflash) and `final = ["sonnet"]`, in a scratch repo under `open_paths`:
   - an open coding ask drafts on glm, the Sonnet check reviews, and the change is applied with the one line;
   - with Sonnet made unusable, nothing is applied and the owner is asked;
   - the `task.draft` row;
   - the draft's requests carry no program tool.
   Name the rows to read.
8. The follow-ups.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
