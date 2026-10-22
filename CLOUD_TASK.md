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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail2-cn`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from the integration branch `integ/cn` at `0bf2dfee` (integ/cn after its round 3: cn-tool-screen, cn-observability, cn-bg-rest; it is based on an older `main`, `88f2e48f`, so the "landed since" list below describes `main` and not your base); that branch, not `main`, is your base. Where the
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

## Your task: tail2-cn, the China-origin lane's open follow-ups: an open task's last leaks, the open request's reach, a late private mark, the data floor's last key, and the docs (theseus-gmyoy, theseus-5u16e, theseus-mhqlm, theseus-qul8y, theseus-y8k3u, theseus-z1ros, theseus-350xc, theseus-1v7fl)

Branch: `cloud/20261021-tail2-cn`. Every commit's subject carries the Beads id of what it changes (`theseus-gmyoy`,
`theseus-5u16e`, `theseus-mhqlm`, `theseus-qul8y`, `theseus-y8k3u`, `theseus-z1ros`, `theseus-350xc`,
`theseus-1v7fl`). Deadline for the report: 5 hours after you start.

**Why.** The China-origin lane (theseus-a1wvq) built the data floor, open tasks, the open view and the open request,
the tool-result screen and the background gates. Each of its reviews left small findings open. None is a live leak
today, because restricted requests only go out with a pass that the open request mints. They are the last places
where the boundary rests on one line nobody tests, or where an open request reaches more than it needs:
- **A task.create path with no test (theseus-gmyoy, theseus-5u16e F4, theseus-qul8y F3).** `task.rs::create` sets
  `rec.data = None` ("a task never inherits its parent's open run") and strips the parent's external hold for an open
  task (`external::bare`). Both lines are right. But `tests_data_state::a_task_never_inherits_its_parents_run` and
  `tests_open_request::an_open_tasks_hold_names_no_url_or_query` drive `task.start` only. The integrator's plant
  (below) that copies the parent's run into the `task.create` record is missed by the whole suite. The `task.create`
  path is the one a model drives, and it is the data boundary.
- **The rest of the open-task leaks (theseus-5u16e).** F3 (the VIA_BRIEF spread into the parent's private reply) is
  closed by the open view and tested (`an_open_tasks_recall_never_spreads_into_its_parent`). F4 (url/query) is closed
  on both paths by `external::bare`, but only `task.start` is tested; this row adds the twin. F5 (an open task
  inherits `rec.dir` and `last_target`) needs a decision written down: the review judged it harmless, because routing
  and the open view treat workspace reads as private. This row says so in AGENTS.md and adds no code.
- **An open session reaches a little more than it should (theseus-mhqlm, theseus-qul8y).** `file.read` in an open
  session of a private place reads a transcript attachment without the outside-text hold (F2). An open request prints
  the open roots as absolute paths, so the owner's home directory, with its user name, can reach a model outside the owner's trust (F4). And an
  open request is offered every public tool, including `task.create` and `wake.at`. With those, a restricted model can
  start work that runs later, on another model, with the owner's trust (qul8y 1).
- **A private mark taken once (theseus-y8k3u).** A task whose parent is private is marked private in the frame that
  opens it. A parent that reads a Google result after the child started leaves the child plain. Two wired call sites
  (`learning/propose.rs` `write_candidate`, `rpc/thread_import.rs`) have no test.
- **The floor's last key (theseus-z1ros).** `config/data_floor.rs` refuses seven owner-material keys that name a
  restricted profile, but not `[memory] synth_profile` or `summary_profile`. cn-bg-rest (round 3) made both fall back
  at run time. What is left is to decide whether the load also refuses, and to test that decision.
- **Docs (theseus-350xc, theseus-1v7fl, qul8y 2).** The repo docs do not yet say that the floor is always on, which
  ledger kinds it writes, that routing refuses a restricted profile only in mode lists (a base, pin, place or `[model]`
  default is left to the floor), that clearance is per request, or that a taint closes `admits`.

**Risk: medium-low.** Three behaviour changes are visible to a person: an open request loses the tools that start
work later; the roots print as `~/…`; and a restricted `synth_profile` stops the load (as the floor's other seven keys
already do). The rest is tests and docs. Nothing here may loosen the floor. Every change sends less to a restricted
model, or the same.

**Prerequisite: integ/cn at `0bf2dfee`** (round 3 = cn-tool-screen, cn-observability and cn-bg-rest on
`7addcb0e`, with the integrator's `open_step::writer` join fix). Base your branch there, not on `main`. Lines below are
"at 7addcb0e (+ round 3); search by name".

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail2-cn` | theseus-gmyoy, theseus-5u16e, theseus-mhqlm, theseus-qul8y, theseus-y8k3u, theseus-z1ros, theseus-350xc, theseus-1v7fl | task.create's twin tests (no inherited run, a bare hold); an open session's `file.read` holds; roots shown as `~/`; open requests lose the tools that start work later; a child's private mark follows its parent's later mark; two scope tests; `synth_profile` on the floor; repo docs | `crates/theseus-core/src/places.rs` (`shown`, `public_tool` untouched), `crates/theseus-core/src/toolrun/misroute.rs` (its `shown` moves to places), `crates/theseus-core/src/open_view.rs` (`Open::offers` only), `crates/theseus-core/src/toolrun.rs` (one line in `file.read`'s run), `crates/theseus-core/src/turn/data_step.rs` (`private_session` only) or the carrying path you find, `crates/theseus-core/src/config/data_floor.rs`, new `crates/theseus-core/src/tests_cn_follow.rs` + one `mod` line in `lib.rs`, `crates/theseus-core/src/tests_private_background.rs` (two tests), `crates/theseus-core/AGENTS.md`, `docs/privacy.md`, `docs/status.md` | M |

## What exists (lines at 7addcb0e (+ round 3); search by name)
- **task.create's record.** `crates/theseus-core/src/task.rs` `create` (near :640-700): `rec.data = None;` (comment
  "A task never inherits its parent's open run"), then `opened_row` for `i.open`, then the parent's hold:
  `external::taken(..)`, and `if i.open { taken = crate::external::bare(taken); }` (near :665). Then
  `google::private::inherited_on(..)` and the `VIA_BRIEF` edge. `external.rs` `taken` (near :259), `bare` (near
  :281: empties `url`, `query` None).
- **The tests to twin.** `crates/theseus-core/src/tests_data_state.rs` `a_task_never_inherits_its_parents_run` (near
  :111): sets the parent's `DataState` run, calls `task_start`, and asserts `task.data.is_none()` and the parent's
  run unchanged. `tests_open_request.rs` `an_open_tasks_hold_names_no_url_or_query` (near :693): the parent holds a
  `web.search` read, then `task_start` with `open` true and false. **The approval rig for task.create already
  exists:** `tests_external.rs` `a_task_that_a_holding_session_starts_holds_it_too` (near :569): the model calls
  `task_create`, the call waits (`res.awaiting_confirm`), and the test answers `core.confirm_action(&corr, true, None,
  "test")`. qul8y's notes record a first try that stalled on that approval; copy this test's shape and the stall is gone.
- **The integrator's plant for gmyoy** (apply it by hand to your base; the hunk is at `rec.data = None;`):
  ```
  -            rec.data = None;
  +            rec.data = tc
  +                .store
  +                .get_session::<SessionRecord>(tc.session_id)
  +                .ok()
  +                .flatten()
  +                .and_then(|r| r.data);
  ```
- **F3 is tested already:** `tests_open_request.rs` `an_open_tasks_recall_never_spreads_into_its_parent` (near
  :728); `recall.rs` `drawn_place` (near :1256). Do not remove the `VIA_BRIEF` edge for an open task:
  `retract.rs` (near :478) follows it.
- **file.read.** `crates/theseus-core/src/toolrun.rs` (near :1902), in the transcript-file run:
  `let shared = tc.class != crate::places::PlaceClass::Private;`, passed to `self.files.run(.., (shared,
  available))`. `ToolCtx` has `pub open: Option<&'static crate::open_view::Open>` (near :152). `toolrun.rs` is at
  2,491 lines, and the unlisted limit is 2,500.
- **The roots.** `crates/theseus-core/src/places.rs` `pub fn shown(public: &[PathBuf]) -> String` (near :644)
  joins `display()`. Callers: `places.rs` (near :614), `toolrun.rs` (near :742), `open_view.rs` `Open::refusal` and
  `Open::note` (near :100-114). `crates/theseus-core/src/toolrun/misroute.rs` `fn shown(p: &Path)` (near :178)
  already renders a path under `HOME` as `~/…`. `rpc/explain.rs` has its own `shown` (near :484) for the operator's
  view: leave it.
- **The open request's tools.** `open_view.rs` `Open::offers` (near :72): `public_tool(name) || (files_tool(name)
  && !roots.is_empty()) || (runs_tool(name) && within)`. `places.rs` `public_tool` (near :476): `web.search`,
  `http.fetch`, `tools.find`, `wake.at`, any `task.*`. A shared place also uses `public_tool`: do not change it.
  `Open::refusal`'s sentence names "only the public tools".
- **The private mark.** `google/private.rs`: `session`, `session_on`, `any_session` (near :89-115), `inherited`
  / `inherited_on` (near :173-195), `bound`, `for_session`, `over_sessions` (near :200-240). The turn reads the mark
  at each call through `turn/data_step.rs` `private_session(t)` (near :73: `t.recall.private ||
  private::session(store, cfg, t.tc.session_id)`). Its callers are `turn.rs` (the call, near :2465),
  `turn/compaction.rs` (near :400), `turn/inbound_step.rs` (near :122) and `turn/route_step.rs` (near :675). The
  opening frames are `rpc/task_start.rs` (near :367) and `task.rs` (near :680). The task's parent session is on
  the session record's task (`session.rs` `parent_session`, near :271).
- **The two untested scope sites.** `learning/propose.rs` `write_candidate` (near :674; `over_sessions(provider, ..,
  &g.sessions)` near :738), `rpc/thread_import.rs` (near :298, `over_sessions(provider, .., of)`). Pattern to copy:
  `tests_private_background.rs` (`google_on`, `mark`, and
  `a_request_over_several_sessions_is_private_when_any_one_is` near :225).
- **The floor.** `config/data_floor.rs` `owner_material_profiles` (seven keys) and `validate_data_floor`, with
  `KEYS` in its tests. `config/memory.rs`: `summary_profile` (the word `off` = no compaction, and a word for the
  turn's own profile; read the constants near :253-260), `synth_profile` (the word `session` = each source's own
  profile). Round 3 (cn-bg-rest): `consolidate/run.rs` `unrestricted` falls back to `default` for a restricted
  synth profile, or skips with `restricted_origin`. `turn/compaction/gate.rs` `summary_route` sends a range to a
  restricted model only when it is cleared, with its own pass, and otherwise falls back.
- **Docs.** `crates/theseus-core/AGENTS.md`: "China-origin profiles" (near :1501), the floor passage (near
  :1606-1618), `DataState` (near :1620), "The open request, per request" (near :1637), "Background calls" (near
  :1655). `docs/privacy.md` "Who else sees it" (near :25-40, **Models of China origin**). `docs/status.md`: the
  2026-10-19 entry "v1 patch1's data and routing lane" (near :509).

## What to build
1. **theseus-gmyoy, the key test.** In the new `tests_cn_follow.rs`:
   `a_task_the_model_creates_never_inherits_its_parents_run`. The parent has a `DataState` run, as the twin sets one.
   A scripted model calls `task_create` with no hold, so no approval is needed. Assert that the task's record has
   `data == None` and the parent's run is unchanged. Plant: the hunk above. It must go red.
2. **theseus-5u16e F4 + theseus-qul8y F3: the bare hold on task.create.**
   `an_open_task_the_model_creates_holds_no_url_or_query`. Copy `a_task_that_a_holding_session_starts_holds_it_too`
   (the parent holds a `web.search` read with a url and a query, the model calls `task_create` with `open: true`, the
   test answers the approval). Assert that the created task's hold has an empty `url` and no `query` and still names
   its tool. Add a non-open twin that keeps both. Plant: drop the `if i.open { bare }` line in `task.rs`. F5: one
   sentence in AGENTS.md's open-task passage (an open task keeps its parent's directory and last target; its
   workspace reads are private to routing and the open view (`data_step.rs`), so the dir opens nothing). No code.
3. **theseus-mhqlm F2: file.read in an open session holds.** `let shared = tc.class !=
   crate::places::PlaceClass::Private || tc.open.is_some();`, with a comment of one line. Test
   `an_open_sessions_transcript_read_takes_the_outside_text_hold` (copy the shared-place case that tests the hold for
   `file.read`; grep `tests_*` for it). Plant: the old line. If the hold then stops an open task's own first
   read of its brief's attachment, stop and report instead (the review left it to this row to decide).
4. **theseus-mhqlm F4: roots shown under `~`.** Move misroute's `shown(p: &Path)` to `places.rs` as
   `pub fn home_shown(p: &Path) -> String`, use it in misroute, and make `places::shown` map each root through it.
   Every model-facing root (the shared place's note, the open request's note and refusal) then reads `~/…`. Test (pure,
   in `places.rs`'s tests or `tests_cn_follow.rs`): with `HOME` set to a temp dir, a root under it shows as `~/x` and a
   root outside it is unchanged. If a test that sets `HOME` already exists, reuse its guard. Run `tests_first_request`
   and the core golden (`crates/theseus-core/tests/golden/core_output.txt`). If a golden moves only by the roots'
   text, update it and say so in your keel note. Plant: `display()` back in `places::shown`.
5. **theseus-qul8y 1: an open request is not offered the tools that start work later.** In `Open::offers`, a public
   tool is offered unless it starts or schedules work outside this request. Read the `task.*` tool names the
   catalog exposes (`git grep -n '"task\.' crates/theseus-tools crates/theseus-core/src/toolrun*`) and decide each
   one:
   - keep any tool that reads, or acts only on the open task's own record (its progress, its close);
   - drop `wake.at`, and any tool that creates, starts, splits or claims a task.
   Write the kept set as a small named fn in `open_view.rs` (for example `open_public(name)`). Fix `refusal`'s
   sentence so it stays true. Test `an_open_request_is_not_offered_the_tools_that_start_work`: an open request's
   offered tools (the compiled request's tool names, as the open-view tests read them) hold none of the dropped tools
   and still hold `web.search`. A shared place's offer is unchanged (`places::offered` untouched). Plant: `offers`
   back to `public_tool`. If an open task can no longer report or close itself, keep that tool and say why.
6. **theseus-qul8y F2 (no code unless a test shows harm).** Write the test that shows the review's "harmless":
   `a_taint_lost_to_a_failed_turn_is_taken_again`. An open session's turn is refused at admission for a reason that
   taints (`DataWhy::taints`), and the turn fails before `finish` (a scripted provider error). Assert that no taint is
   stored. Then the next turn on the same input is refused again and stores the taint. If the second turn does NOT
   refuse, that is a real gap: fix it in `open_step.rs` by writing the taint in the failure path, in a new fn and with
   at most 3 lines in `turn.rs`, and say so. Otherwise there is no code change.
7. **theseus-y8k3u 1: a child's mark follows its parent's later mark.** Decide one of the two designs and write the
   reason in the report:
   - (a) at the child's call, `private_session` also reads the parent: `private::any_session(store, cfg, [session,
     parent])` when the turn is a task's;
   - (b) where the parent's content later enters a running child (`task.update`'s message to a running task, a steer,
     a parent's answer to the child's question: find them), the child takes the mark in that frame.
   (b) is the precise one, and (a) is the fail-closed one. Prefer (b) if every carrying path is in one or two
   frames. Otherwise take (a): its cost is that the child's later routes stay on allowlisted models. Test
   `a_child_started_plain_is_private_once_its_parent_is` (the parent is marked after the child started; the child's
   next call is held by the floor, as `a_bound_provider_is_held_by_the_floor_on_any_task` checks). Plant: drop the new
   read or write.
8. **theseus-y8k3u 2: two scope tests** in `tests_private_background.rs`:
   `a_skill_proposal_over_a_private_session_is_private` (`write_candidate` with a `Gathered` whose sessions include a
   marked one: the provider call is held, red when `over_sessions` there becomes the bare provider) and
   `a_thread_import_of_a_private_session_is_private` (same, with `thread_import`). If either site cannot be driven
   without a model rig longer than 80 lines, test the scope with a call-capturing provider, as near :245 does. Plant:
   the bare provider at each site.
9. **theseus-z1ros: the floor's decision.** `synth_profile` naming a profile (not the word `session`) of restricted
   origin can never be used: consolidation mixes many sessions, so no pass can cover it. Add it to
   `owner_material_profiles` (8 keys; `KEYS` too), so the load refuses it in words. The word `session` stays the run
   time's job (`unrestricted`). `summary_profile` is NOT added: a compaction in an open request may legitimately
   use a restricted model with its own pass (D1, `gate.rs`). Write that reason as a doc comment beside the key list.
   Test: the existing table test covers the new key once it is in `KEYS`, and a test checks that a restricted
   `summary_profile` still loads. Plant: drop the new key. Check `crates/theseus-core/config/theseus.example.toml`
   and `fixtures/owner_shape.toml` for a restricted `synth_profile`; if one is there, stop and report (do not edit
   the template: another row of this lane edits it).
10. **Docs (theseus-350xc, theseus-1v7fl, theseus-qul8y 2).** Repo parts only. The spec's Part III and the lane's
    context doc are outside the repo, so they are the maintainer's: list in the report the sentences they need.
    - `crates/theseus-core/AGENTS.md`, inside the existing China-origin passages, one or two sentences each:
      - the floor is always on, with no switch;
      - its ledger kinds are `data_gate.blocked` and `session.data`;
      - routing refuses a restricted profile only in mode lists and `cn_profiles`. A restricted session base, `-P`
        pin, place profile or `[model]` default is not refused by routing: the floor moves or fails that turn
        (1v7fl);
      - clearance is per request; a taint closes `admits`; an open session's run starts at its first node;
        `DataWhy::taints`;
      - the open request's tool set (item 5);
      - F5 (item 2);
      - the `synth_profile` key (item 9).
    - `docs/privacy.md`: one short paragraph after **Models of China origin**: the floor is always on, and a request
      to a model of restricted origin carries only what the open request cleared.
    - `docs/status.md`: a new dated bullet right after the 2026-10-19 data-and-routing entry, "2026-10-21: China-origin
      follow-ups", with one line per change. Say the store format stays 48 on integ/cn, and that integ/release
      renumbers one of the two 48s to 49 (theseus-c1ve9). Do not edit the 2026-10-19 entry's own lines: a sibling
      row adds one there.

## Tests (each with a planted revert; the first is the key test)
- **`a_task_the_model_creates_never_inherits_its_parents_run`**, red on the integrator's plant above.
- `an_open_task_the_model_creates_holds_no_url_or_query` (+ the non-open twin), red when `bare` is dropped.
- `an_open_sessions_transcript_read_takes_the_outside_text_hold`, red on the old `shared` line.
- The `~/` roots test, red on `display()`.
- `an_open_request_is_not_offered_the_tools_that_start_work`, red when `offers` uses `public_tool` again.
- `a_taint_lost_to_a_failed_turn_is_taken_again`: it shows the existing behaviour, so it has no plant (say so); if it
  found a gap, plant your fix's revert.
- `a_child_started_plain_is_private_once_its_parent_is`, red without the new read or write.
- The two scope tests, each red on the bare provider at its site.
- The floor's new key (via `KEYS`) and the summary_profile-still-loads test, red when the key is dropped.
- Then run `cargo nextest run -p theseus-core tests_cn_follow tests_private_background tests_open_request
  tests_open_view tests_data_state tests_external tests_data_floor data_floor tests_first_request
  tests_tool_screen tests_bg_gate places misroute` and the whole crate once. Run `cargo test -p theseus-core --test
  golden` if your `places::shown` change moves a golden.

## Not in this row
- mhqlm F5 (`thread_spec` on an open session's turns): open sessions are tasks only (D1) and hold no thread card,
  so nothing reaches the bytes. One sentence in AGENTS.md if it fits beside F5 of item 2; no code. `turn.rs` is
  not yours beyond item 6's fallback.
- The round-3 reviews' own follow-ups (http.fetch screens only the plan's URL; the footer's `verdict_private`; the
  first-loop compaction gate's stored run): the integrator files them; they are not in this row.
- `open_view.rs` `open_roots` (a1wvq.13 adds a draft task's tree there), `config/routing*.rs`, `data.rs`,
  `turn/open_step.rs` (except item 6's fallback), `turn/screen_step.rs`, `draft*`, the template
  `theseus.example.toml`, `own_bench/*`, `rpc/task_start.rs` (except reading it), the Gated chokepoint
  (`data_gate.rs`) and every keel file.
- The live checks and the spec's Part III: the maintainer's.

## Rules for this row
- **Model roles:** you write the code and make two design calls (items 5 and 7), each with its reason in the report.
  Keep the code minimal; every line earns its place. Stored data stays readable.
- **The floor only tightens.** Every change sends a restricted model less, or the same. If a change you need would
  send more, stop and report it.
- Every behaviour change gets a test and a planted revert: break it, watch the named test fail, restore it and
  `touch` the file, then check `git status`. List each plant in the report with the test that failed. The key test
  must be shown failing on its plant. A pure doc or refactor needs no plant: say so.
- **Siblings on the same base.** a1wvq.13 cn-draft-review runs beside you from the same sha, and .12
  cn-config-guidance comes after it. The shared files:
  - `open_view.rs`: .13 edits `open_roots`; you edit only `Open::offers`, `refusal`'s sentence and one new fn.
  - `turn/data_step.rs`: .13 has one hunk; keep your change to `private_session`'s body (at most 3 lines).
  - `lib.rs`: one `mod tests_cn_follow;` line in the test-mod list (the join keeps both).
  - `crates/theseus-core/AGENTS.md`, `docs/privacy.md`, `docs/status.md`: .12 adds lines too. Write yours as their
    own sentences and bullets at the places named above, and reformat nothing around them.
  - `tests_open_request.rs`: not yours; your tests go in `tests_cn_follow.rs`.
- **No new always-loaded model text.** First-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass). The roots' `~` is shorter than before. FAST: nothing new on a plain turn
  (`tests_m3::a_plain_turn_stays_within_its_frame_budget`, 4 frames). Item 7(a) adds a meta read only on a task's
  turn: it must not run when the turn has no task.
- **Line ceilings** (`scripts/long-files.txt`; length / ceiling at your base): `crates/theseus-core/src/turn.rs` about
  3,428 / 3,525. `toolrun.rs` is 2,491 and unlisted, so its limit is 2,500: item 3 changes one line in place.
  `task.rs` 1,438, `places.rs` 968 and `open_view.rs` 234 are all unlisted (limit 2,500). Never raise a ceiling.
  Put new code in new files where it grows.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A golden that moves
  by the roots' text is a changed expectation: justify it in the report. Before your last commit run `python3
  scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` and paste its output in the report. It must be
  empty; if it flags a golden, give the reason in the report and write no `Keel:` trailer (the maintainer decides).
- No new dependency (Cargo.lock and the package-lock.json files gain no third-party package). **Store format on your
  base: 48** (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row changes no stored record. If it
  had to, the bump would go to 49 with the old layout's sample in `tests_layouts.rs`, but do not.
- **Commits.** Signed (`git commit -S`; if the VM has no key, say so in the report), each subject carries its Beads
  id, each body ends with the repo's usual `Co-Authored-By: Tabitha/Claude <…>` trailer (copy it from `git log -1
  --format=%B origin/integ/cn`).
- **Secrets and names.** No person's name anywhere: not in the repo, the commits or the report. Write "the owner".
  Fixture paths, sessions and queries are invented (`/tmp/…`, `widgetco`, `Kestrel relay`).
- Read the AGENTS.md of every crate you touch first (`theseus-core`; the docs' own headers). Keep CLOUD_REPORT.md
  honest. About 33 L1 tests fail on this VM (no job cgroup, root): name what you ran, and give the maintainer the
  exact commands for the rest. Run `cargo nextest run -p theseus-core` and `cargo clippy --workspace --all-targets
  -- -D warnings` before the last commit.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty). It holds:
- what changed per Beads id, and which ids can close (5u16e and gmyoy should, with this row; say for each);
- item 5's kept and dropped tool lists, and item 7's design with its reason;
- each test and its plant, with the failing output's line;
- whether item 6 found a gap;
- any golden that moved, and why;
- what could not run on the VM, with the exact commands for the maintainer;
- the keel guard's output, and the store format (unchanged, 48);
- the sentences the spec's Part III needs (for the maintainer);
- follow-ups.
Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
