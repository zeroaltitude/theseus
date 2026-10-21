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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-onto`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `6918e3c1` (2026-10-21, after the patch1 stage-1 landing), unless your task names another base (an integration branch). Where the
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
## Your task: tail-onto, the ontology replay, undo rate and nightly writes made exact (theseus-oswj4, theseus-hdqkt, theseus-bpsnj, theseus-ukmbn, theseus-2tqqs, theseus-5kd37, theseus-23wd8)

Branch: `cloud/20261021-tail-onto`. Every commit's subject carries the Beads id of the item it does (`theseus-oswj4`,
`theseus-hdqkt`, ...). Deadline for the report: 6 hours after you start.

**Why.** The ontology landed with its labelled replay (`theseus judge replay --onto`), the owner's undo rate, the
cockpit's Ontology view and a nightly sweep and dream. Its reviews left seven follow-ups. Each one is a number a person
reads that is wrong or ambiguous, a flag that does nothing without saying so, or a night job that can wait or lose work
for no reason:
- **The replay drops cases from its count (oswj4).** When the replay's spend limit stops a run partway, only the
  first session it did not reach goes into `left_out`. Every later session disappears from both the scores and
  `left_out`, so the Markdown's line "N labelled sessions were left out" is too low.
- **`--onto` ignores flags without saying so (hdqkt).** `theseus judge replay --onto --split train --errors` runs the
  whole labelled replay as if those flags were never given. clap only enforces `--limit requires --onto`.
- **The replay can see the session's own later picks (bpsnj).** `hide_labels` only hides the import's gold
  categories. Any other category the session held at replay time (Jev's own live filing, the operator's) still shows
  as `session: true` in the state Jev reads. That pushes a wrong answer toward "Jev repeats its own earlier pick" and
  makes precision look better than it is.
- **Two undo rates for one thing (ukmbn).** The daemon computes one in `learning::onto_undo::undo_rates`: distinct
  `jdg_…` writes per record kind over a 7-day window, used in the replay's report. The cockpit computes another in
  `cockpit/src/lib/ontostats.ts`: the `onto.filed` acts of the report's day, per kind, and the undos of those. The
  same ledger can give two different "undo rate by kind" numbers.
- **The stored learning report lacks the ontology's line (2tqqs).** The cockpit draws the `onto.v1` line from raw
  ledger history. The daemon's stored `learning.report`, and the CLI's rendering of it, do not have that line.
- **The Ontology view's fold buttons stop working under a filter (5kd37).** While an origin filter is on, `gopen`
  forces every group open, so a fold button does nothing. `toggleGroup` also copies that forced-open set into
  `groupsOpen`, so after the filter is cleared every group stays open.
- **Nightly sweep and dream (23wd8), four low findings from the review:** (1) synchronous store scans on the async
  runtime; (2) one unreadable session makes a whole category wait, every night; (3) the sweep's day mark is written
  once, at the end, so a stop mid-run loses that run's spend and asked keys; (4) the dream's guidance has no off
  switch of its own.

**Risk: low to medium.** Every change stays in the replay, the learning report, the nightly ontology jobs and the
cockpit. Nothing touches a turn's path. The one stored change is a new field on the learning report (see Store
format).

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| T1 | `tail-onto` | theseus-oswj4, hdqkt, bpsnj, ukmbn, 2tqqs, 5kd37, 23wd8 | the seven items below | `crates/theseus-core/src/learning/{replay_onto,replay,onto_undo,tests_replay_onto}.rs`, `crates/theseus-core/src/rpc/learning.rs` (where `pack_report` is called), `crates/theseus-core/src/learning/report.rs` (only if the line is built there), `crates/theseus-protocol/src/learning.rs` (`PackReport`), `cockpit/src/protocol.gen/PackReport.ts` (regenerated), `crates/theseus/src/judge_runs.rs`, `crates/theseus/src/render/learning.rs`, `cockpit/src/lib/ontostats.ts`, `cockpit/src/components/LearningReport.tsx`, `cockpit/src/views/Ontology.tsx`, a new `cockpit/src/lib/groupfold.ts`, `cockpit/test/{ontostats,groupfold}.test.ts`, `crates/theseus-core/src/judge/onto/judged.rs`, `crates/theseus-core/src/consolidate/guidance.rs`, `crates/theseus-core/src/judge/onto/digest.rs` (item 23wd8.1 only), `crates/theseus-core/src/config/ontology.rs`, the `[ontology]` section of `crates/theseus-core/config/theseus.example.toml` (one key line) | M |

## What exists (lines at `6918e3c1`; search by name)
- `crates/theseus-core/src/learning/replay_onto.rs` `onto_replay` (near :96-184). The loop at :138-150 is
  `match caller.ask(ask) { Some(j) => judgments.push(j), None => { left_out.push(ReplayLeftOut { judgment:
  case.session.clone(), reason: caller.left_reason() }); break; } }`. After the loop, `reached = judgments.len()`
  and only `cases[..reached]` is scored. `unreadable = left_out.len()` feeds `onto_result`, which writes the
  Markdown's left-out line (near :340-350). `onto_case` (near :236-283) builds each case's input with
  `crate::judge::onto::onto_input(self, o, &msg)`.
- `crates/theseus-judge/src/replay.rs` `hide_labels` (near :368) sets `item.session = false` only for pool items in
  `case.labels` (gold). `crates/theseus-core/src/judge/onto/mod.rs` `onto_input` (near :209) sets each pool item's
  `session` from `o.member_list(sid, &k.name)`, whatever the membership's origin. It sets `used_ms` from every
  session's lists, the replayed session's included, and `suggested` from `name::suggested(core, sid)`.
  **Do not edit `judge/onto/mod.rs` or `judge/onto/name.rs`** (another lane in flight edits `name.rs`). Do the
  hiding in `replay_onto.rs`, on the input `onto_case` builds.
- `crates/theseus/src/judge_runs.rs` `ReplayArgs` (near :20-46): `split`, `errors`, `judgments`, `report`,
  `onto`, and `limit` (`#[arg(long, requires = "onto")]`). The daemon branches in
  `crates/theseus-core/src/learning/replay.rs` `judge_replay` (near :341: `if p.onto { return
  self.onto_replay_run(p, who).await; }`).
- `crates/theseus-core/src/learning/onto_undo.rs` (96 lines): `WINDOW_MS` (7 days), `undo_rates(rows,
  since_ms)` (a write is Jev's when its rows carry `undo.write` starting `jdg_`; its kinds are its rows'
  `data.kind`; it is undone when an `ontology.undo` row names it), `rows_since(store, since_ms)`. Its doc says it is
  pure "so the learning report's line can call it". Nothing calls it but the replay.
- `cockpit/src/lib/ontostats.ts` (103 lines): `ontoStatsOf(rows, {day})` counts the day's `onto.v1` judgments, acts
  (an `onto.filed` row with a step `filed` or `attached`), confirms, and undos (an `ontology.undo` naming an act's
  judgment). It also counts `kinds`, a per-kind tally of acts and undos. `ontoLine` and `ontoKindWords` render
  them. `cockpit/src/components/LearningReport.tsx` (near :47-50) calls it for `ONTO_PACK`. Tests are in
  `cockpit/test/ontostats.test.ts` (`npm test` in `cockpit/` runs `node --test test/*.test.ts`. There are no
  component tests, so testable logic lives in `src/lib/`).
- The learning report: `crates/theseus-protocol/src/learning.rs` `LearningReport { date, at_unix_ms, trigger,
  labels, packs: Vec<PackReport> }`, `PackReport` (near :122). Built in `crates/theseus-core/src/rpc/learning.rs`
  (near :309: `packs.push(report::pack_report(name, seen, &scope.labels, window))`). Rendered by
  `crates/theseus/src/render/learning.rs` `learning_report_lines` (near :123). `UndoRate { kind, writes, undone,
  rate }` already exists in `theseus_protocol::judge_runs`.
- `cockpit/src/views/Ontology.tsx` (near :45-47): `groupsOpen` state, `gopen` memo (adds the selected group, and
  every group while `wanted.size`, i.e. while filtering), `toggleGroup` (`setGroupsOpen(() => { const s = new
  Set(gopen); … })`). `filtering = wanted.size > 0` (near :54).
- Nightly: `crates/theseus-core/src/judge/onto/judged.rs`. `held_by` (near :206) scans every member list of the
  kind and calls `self.store.get_session` for each, with no `blocking`. `sweep_category` (near :231) calls it, and
  the split path calls it again (`self.core.held_by(o, &parent)`, near :444). `onto_sweep_judged` (near :254-291)
  reads the `onto.sweep.judged` META mark (`MARK`), runs `run.pairs` then `run.splits`, and writes the mark once
  after both. `crates/theseus-core/src/consolidate/guidance.rs`: `dream_sessions` (near :174) and `dream_wanted`
  (near :193) read sessions without `blocking`. `dream_request` (near :225) collects
  `Option<Vec<Seen>>` (near :255), so one session whose `human_end` is `None` makes the whole category wait.
  `dream_guidance` (near :265) has only `[memory] synth_limit_usd_per_day` as a limit.
  `crates/theseus-core/src/judge/onto/digest.rs` `onto_night_lines` (near :227) reads rows and the snapshot
  synchronously and is called from the async `morning_note`. `crates/theseus-core/src/config/ontology.rs` has
  `sweep` and `sweep_usd_per_day` (near :54-57) as the models for a new key.
- Store format at `6918e3c1`: **48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`).

## What to build
1. **oswj4: every unreached case is left out.** When `caller.ask` returns `None`, push a `ReplayLeftOut` for that
   case **and for every later case**, each with the same `caller.left_reason()`, then stop. `judgments` (the
   result's count of cases) stays `cases.len()`. Scored plus left-out now covers every case.
2. **hdqkt: `--onto` refuses what it ignores.** In `ReplayArgs`, give `onto` `conflicts_with_all = ["split",
   "errors", "judgments", "report", "candidate", "pack_file"]`. Before you add each name, check whether the onto
   path reads it: `onto_replay` does read `candidate`/`pack_text` (it defaults to `onto.v1`), so keep any argument
   it honours and refuse only what it ignores. Also refuse them in the daemon (`judge_replay`'s `p.onto` branch),
   because a client can call the RPC directly. Refuse with one sentence naming the flag, e.g. "--onto replays the
   import's labelled sessions; --split is for a report's holdout and is not used with it". Nothing is sent.
3. **bpsnj: the replay's state is the session as it stood at its first message. Decided:** hide **every**
   membership of the replayed session, whatever its origin (gold, Jev's, the operator's). At the live point (the
   first human message) the session held none, so any `session: true` is knowledge from the future. In
   `onto_case`, after `onto_input`, set `session = false` on every pool item of every kind. Leave `hide_labels` as
   it is: it is the judge crate's own guard. Also check `used_ms` and `suggested`:
   - If the replayed session's own memberships raise a category's `used_ms`, recompute it without that session's
     lists (you have `o` and `sid` in `onto_case`).
   - If `suggested` reads naming calls made after the first message, clear it for the replay.
   Update the module doc (near :7-12) to say what is hidden. State in the report what each of these did on the
   fixtures.
4. **ukmbn: one undo-rate definition. Decided:** the daemon's `onto_undo::undo_rates` is the one definition:
   - a write is a Jev judgment's (`jdg_…`) records;
   - its kinds are the kinds it wrote;
   - it is undone when an `ontology.undo` names it;
   - the window is stated next to the number.

   The cockpit stops computing its own per-kind undo rate. Its day line keeps judgments · acts · confirms as the
   day's activity, and its undo count and "% of acts" go. The per-kind undo words come from the stored report (item
   5), labelled with their window: `undo rate by kind (Jev's writes, 7 days to 2026-10-21): person 1 of 1 (100%) ·
   topic 1 of 2 (50%)`. Before you settle, check one difference: the cockpit counts a project **attach** as an act,
   but `undo_rates` counts only rows with `data.kind`. If an attach is a Jev write that `ontology.undo` can take
   back, make `undo_rates` count it under `project` (a test for it). If it cannot be undone, the cockpit's
   exclusion of it is right; write one sentence on it in `onto_undo.rs`'s doc. Record the decision and the attach
   finding in the report.
5. **2tqqs: the stored report carries the line.** `PackReport` gains `#[serde(default, skip_serializing_if =
   "Vec::is_empty")] pub undo: Vec<UndoRate>` (the existing type: **no new protocol type**, so
   `protocol.gen/index.ts` does not change). Fill it only for the ontology pack (`onto.v1`, matched by the pack's
   name as `ONTO_PACK` is). Use `undo_rates(&rows_since(store, at - WINDOW_MS)?, at - WINDOW_MS)` where `at` is the
   report's `at_unix_ms`, in `rpc/learning.rs` beside the `pack_report` call. `report.rs` is pure and stays pure.
   Regenerate `cockpit/src/protocol.gen/PackReport.ts` the way AGENTS.md says. `learning_report_lines` prints one
   line under the pack, the same words as the cockpit's. `LearningReport.tsx` reads `pack.undo` and `ontostats.ts`
   loses its kinds tally (`ontoKindWords` takes `UndoRate[]` and the window's end date).
6. **5kd37: folds are the user's.** Move the open-set logic into a new pure `cockpit/src/lib/groupfold.ts`:
   - `openGroups(groupsOpen, selectedKey, filtering, allKeys)` returns what `gopen` returns today;
   - `toggled(groupsOpen, key, filtering)` returns `groupsOpen` unchanged while filtering, and otherwise toggles
     `key` in `groupsOpen` (not in the forced set).

   `Ontology.tsx` uses both. While filtering, a fold button is disabled (`disabled` plus its `aria-label` saying the
   filter shows matches whole). After the filter clears, the groups are as the user left them.
7. **23wd8: the nightly jobs.**
   1. Wrap `held_by`'s scan, `dream_wanted` (with `dream_sessions`), and `onto_night_lines`'s reads in
      `theseus_store::blocking`, as the files' other reads already are. Compute `held_by` once per split candidate.
      Do not change what any of them returns.
   2. In `dream_request`, skip a session whose first or last human text cannot be read, instead of making the whole
      category wait. The category waits only when fewer than the sample's readable sessions remain. Use the
      existing minimum (`MIN_SESSIONS`), checked on the readable count.
   3. In `onto_sweep_judged`, write the mark (spend and asked keys) after each paid question, not once at the end.
      `Run` holds the mark, so a helper that `put_meta`s it under `blocking` after each answer is enough. A stop
      mid-run keeps what was spent and asked.
   4. Add `[ontology] dream_guidance = true` (`config/ontology.rs`, beside `sweep`, documented in the module doc,
      and one line in the template's `[ontology]` section). `false` makes `dream_guidance` return
      `Dreamed::default()` before any read: that is its rollback.

## Tests (each with a planted revert; the first is the key test)
- **`a_replay_cut_short_leaves_out_every_later_case`** (in `learning/tests_replay_onto.rs`, using the file's
  existing replay-limit rig, near :296): three labelled cases and a limit that admits exactly one call. The result
  has one scored session and `left_out` naming the other two, each with the limit's reason, and the Markdown says 2
  were left out. Plant: put the single `push` + `break` back. The test must fail.
- `onto_refuses_flags_it_ignores`: a `ReplayArgs` parse test in `judge_runs.rs` (`#[cfg(test)] mod tests` with
  clap's `try_parse_from` through a tiny `Parser` wrapper). `--onto --split train`, `--onto --errors` and
  `--onto --judgments jdg_a` are errors, and `--onto --limit 5` parses. Add a daemon test that the RPC with
  `onto: true, errors: true` is refused and sends nothing. Plant: drop `conflicts_with_all`.
- `the_replay_hides_the_sessions_own_memberships`: a labelled session that also holds a non-gold category filed by
  Jev and one by the operator. The request the fake Jev receives has no `session: true` item. Plant: remove the
  clearing in `onto_case`.
- `undo_rates_*`: if attaches count, add a test that an undone attach shows under `project`. The existing
  `undo_rates` tests must stay green.
- `the_learning_report_carries_the_onto_undo_line`: a report run over rows with two Jev writes, one undone, gives
  `onto.v1`'s `PackReport.undo` the right kind, writes, undone and rate. A non-onto pack's `undo` is empty and its
  JSON has no `undo` key. Plus a render test in `render/learning.rs`. Plant: leave `undo` empty.
- `cockpit/test/groupfold.test.ts`: under a filter every group is open and `toggled` returns the same set. After
  clearing, the user's own open set returns. Without a filter, a toggle folds an open group. Plant: return to
  copying the forced set.
- `cockpit/test/ontostats.test.ts`: update for the removed tally (a test that only checked the removed tally is
  replaced by the stored-report words' test, not deleted without one). Name each changed test in the report.
- `the_dream_skips_an_unreadable_session` (in the guidance tests): six sessions, one with no readable operator
  message. The category is still written for, from the five. Plant: put back `collect::<Option<…>>`.
- `the_sweep_mark_survives_a_stop`: a sweep stopped (the test's provider errors or the cap is hit) after one paid
  question leaves a mark whose spend and asked keys include that question. Plant: write the mark only at the end.
- `dream_guidance_off_writes_nothing`: with `dream_guidance = false` no call is made and no row is written. A config
  test covers the default (true) and the template key.
- The `blocking` wraps are a pure move: no plant, say so.

## Store format
`PackReport.undo` is a new field on the stored `judge.report` row. Read AGENTS.md's rule on stored layouts
(`crates/theseus-core/src/tests_layouts.rs`):
- If that row is covered there, bump `MANIFEST_FORMAT` by one from your base and add the old layout's literal
  sample, as AGENTS.md says. The maintainer renumbers at the join.
- If it is not covered (a `serde(default)` field on a ledger fact), do not bump, and say why in the report.

## Not in this row
- `judge/onto/mod.rs`, `judge/onto/name.rs`, `tests_onto.rs`, `learning/audit.rs`, `learning/propose.rs` and
  `learning/rebuild.rs`: other rows in flight edit them.
- Any change to the onto pack's prompt or to `theseus-ontology`'s planners.
- The dream's owner card (the morning note's line stays its surface).
- Docs: `docs/status.md`, the spec and `docs/design/` are the maintainer's. Write in the report what should
  change: the one undo-rate definition, the new `[ontology] dream_guidance`, and the replay's hidden memberships.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. Where the row names a switch
  (`[ontology] dream_guidance`), it is the rollback: a test shows that with it off the old behaviour (no dream
  guidance) is back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool and no
  model-facing text. FAST: nothing new on a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget`),
  nothing on the path before a request leaves, and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty. A test you rewrite because a function's shape changed (the ontostats tally) must keep checking the
  same behaviour through the new shape: say which.
- **Line ceilings** (`scripts/long-files.txt`): none of this row's files is listed. Keep each under 2,500 lines. Never
  raise a ceiling.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the
  owner". Fixture names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md`'s ontology and
  learning sections, `crates/theseus-protocol/AGENTS.md`, `cockpit/AGENTS.md` if present). Keep CLOUD_REPORT.md
  honest: what you ran, what passed, and what you could not run in the cloud with the exact commands for the
  maintainer. Run `cargo test -p theseus-core learning::`, the `onto` and `guidance` tests, `cargo test -p theseus
  judge_runs`, `cargo test -p theseus-protocol`, and `npm test` plus `npx tsc --noEmit` in `cockpit/`.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty). It says:
- what changed, per Beads id;
- the decisions for bpsnj (what was hidden, and what `used_ms`/`suggested` did) and ukmbn (the attach finding);
- each test and its plant, with the test that failed;
- what could not run on the VM, with the exact commands for the maintainer;
- the keel guard's output;
- the store format (bumped or not, and why);
- the docs the maintainer should update;
- follow-ups.

Push the branch, then end with a final message that starts `CLOUD REPORT COMPLETE`.
