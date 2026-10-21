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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-hold`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: tail-hold, five small harness follow-ups: a failed spawn's argv file, a held reply's footer, progress ages on every surface, two nits (theseus-vymxh, theseus-2rvwv, theseus-ztkhg.1, theseus-c93va, theseus-jjnud)

Branch: `cloud/20261021-tail-hold`. Every commit's subject carries the Beads id of what it changes (`theseus-vymxh`,
`theseus-2rvwv`, `theseus-ztkhg.1`, `theseus-c93va`, `theseus-jjnud`). Deadline for the report: 5 hours after you
start.

**Why.** Five follow-ups the reviews of the last harness batch left open, each small, each a thing a person sees or a
file left on disk:
- **A failed job spawn leaves its argv file behind (theseus-vymxh).** Since the job-truths row, a job's argv goes by the
  spool, not by the wrapper's command line: `spawn_detached` writes `<spool>/argv/<id>` (mode 0600) before it spawns
  the wrapper. Only `Spool::remove_pid` removes that file, and only the wrapper's end and the daemon's settle paths
  call it. If the spawn itself fails (a missing binary, `EAGAIN` under a pids limit, a full process table), nothing
  ever removes it: the job's whole command text (which can hold paths and arguments a person typed) stays on disk for
  the life of the spool.
- **A held reply's footer names a model that never ran (theseus-2rvwv).** In a shared place, a reach for the owner's
  personal details is held: the turn makes no model call and the requester is answered `I don't share that here.`
  (`turn/personal_step.rs`, `hold_reply`, theseus-i1z13). The turn's result still carries the target's model
  (`turn.rs` builds `TurnSubmitResult.model` from the last response, else `t.target.model`), so the Discord reply's
  footer reads e.g. `default · claude-sonnet-5-5 · 1 loop · 0.4 s`, and the CLI's line `[default → anthropic/claude-…`:
  both say a model answered. A person reading the place sees a model credited with a refusal it never made.
- **Old words show no age on three surfaces (theseus-ztkhg.1).** The rough-edges row (theseus-ztkhg, commit
  `dab58548`) made the model's last words carry their age when they are older than what the task does now and over a
  minute old: `Round 1 done. Continuing. (2 min ago)` (`theseus_protocol::work::progress_line::aged_said`). It is used
  by `theseus tasks` (`crates/theseus/src/render/task_tree.rs`) and `theseus work` (`crates/theseus/src/work.rs`)
  only. The TUI's Job row, the CLI's `theseus status` long form and the Discord task view still show stale words as if
  they were said just now.
- **The recall scorer's abstention check has no fallback (theseus-c93va).** In `bench/recall/score.py`,
  `abstained_as`, when every word of the asked subject is shared with another fact's subject, `own` ends up empty, so
  `said & own` is always empty and a reply giving a value next to only shared words is never `misleading`. The recall
  row's report claimed shared words count as the asked subject's; the code does not do that. Plus a nit in
  `toolrun/resume.rs` (below).
- **An empty `else if` body (theseus-jjnud).** `crates/theseus/src/interactive.rs`, `on_note`: `} else if again {`
  with an empty body works but reads like a bug.

**Risk: low.** Each piece is local. The one behaviour change a person sees is the held reply's footer; the one data
change is a new `stop_reason` value on a held turn (no store layout change).

**Prerequisite: `main`.**

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail-hold` | theseus-vymxh, theseus-2rvwv, theseus-ztkhg.1, theseus-c93va, theseus-jjnud | the argv file removed on a failed spawn; a held turn says so in its footer instead of naming a model; `aged_said` on the TUI Job row, `theseus status`'s progress column and the Discord task view's last words; the scorer's fallback and resume's one pid read; the empty branch | `crates/theseus-kernel/src/{job,spool}.rs`, `crates/theseus-core/src/turn/personal_step.rs`, `crates/theseus-core/src/tests_personal_hold.rs`, `crates/theseus-discord/src/render.rs` (`footer` only), `crates/theseus/src/render.rs` (the turn line only), `crates/theseus-tui/src/work.rs`, `crates/theseus/src/status_progress.rs`, `crates/theseus-discord/src/render/task_view.rs`, `bench/recall/{score,test_score}.py`, `crates/theseus-core/src/toolrun/resume.rs`, `crates/theseus/src/interactive.rs` | M |

## What exists (lines at `6918e3c1`; search by name, a line may have moved)
- **The argv file.** `crates/theseus-kernel/src/job.rs` `spawn_detached` (near :134): builds the wrapper's command,
  then `spool.write_argv(&args.correlation_id, &args.argv)?` (near :174), then
  `crate::children::spawn(Kind::Wrapper(..), || cmd.spawn(), ..).context("spawning job wrapper")?` (near :187-193),
  then `spool.write_pid`. `crates/theseus-kernel/src/spool.rs`: `argv_path(&self, id: &str)` (near :107),
  `write_argv` (near :320), `remove_pid(&self, id: &CorrelationId)` (near :314) removes both the pid file and the argv
  file. The wrapper reads the file in `parse_wrapper_args` (`job.rs` near :1498, `Spool::read_argv_in`). Callers of
  `spawn_detached`: `theseus-core/src/toolrun.rs:97` and tests (`kernel/tests/tree.rs`, `theseusd/tests/{cgroup,
  job_line,job_wrapper}.rs`, `theseus-sim/src/jobs.rs`).
- **The hold.** `crates/theseus-core/src/turn.rs` near :1858: after `compile_routed`, `if t.route.held { …
  Self::hold_reply(t, i)?; break; }`. `turn/personal_step.rs` `hold_reply` (near :194) appends an
  `AssistantMessage` node with `blocks` = the held line, `model: t.target.model.clone()`, `provider:
  t.target.provider.clone()`, `stop_reason: None`, pushes `HELD_REPLY` to `t.output`. `turn.rs` near :3257 builds the
  result: `stop_reason` is `t.stop_reason` or `"end_turn"` when empty; `model` is the last response's or
  `t.target.model`. `t.stop_reason` is set elsewhere only to `stopped`, `awaiting_confirm`, `nothing_new`, `budget`.
  Footers: Discord `crates/theseus-discord/src/render.rs` `fn footer` (near :1223) pushes `r.profile`, `r.model`, …,
  and `stopped: {stop_reason}` for any reason but `""`, `no_tool_calls`, `end_turn`; the CLI's turn line in
  `crates/theseus/src/render.rs` (near :531-560, `"[{} → {}/{}{} · {} loop(s)…"` with `r.profile, r.provider,
  r.model`, then `r.stop_reason`). Tests: `crates/theseus-core/src/tests_personal_hold.rs`
  (`a_reach_in_a_shared_place_is_held_and_answered_with_the_fixed_line` and the rest).
- **Ages.** `crates/theseus-protocol/src/work/progress_line.rs` `aged_said(said, at_ms, doing_since_ms, now_ms)`
  (near :76): bare while the words are under a minute old or as new as what the task does, else with `(N min ago)` /
  `(N h ago)`. Used at `crates/theseus/src/render/task_tree.rs` near :252 (read it: the pattern to copy, with
  `doing = v.doing.as_ref().map(|d| d.since_ms)`) and `crates/theseus/src/work.rs` near :112. Not used at:
  - the TUI's Job row: `crates/theseus-tui/src/work.rs` `form_of` (near :258) fills `Form::Job { said, .. }` from
    `v.progress.said` without its `at_ms`; `doing(f, now_ms, hm)` (near :292) prints `job 1:02 of usual 3:00 · {said}`;
  - `theseus status`'s progress column: `crates/theseus/src/status_progress.rs` `of_views` calls `Progress::line()`
    (`crates/theseus-protocol/src/work.rs` near :332, which has no clock);
  - the Discord task view: `crates/theseus-discord/src/render/task_view.rs` near :192, `**Last words**` from
    `p.said`. (The Discord board's working line, `render/board_v2.rs` `working_line`, shows no last words at all: it
    already uses `progress_rest`, the new step/spend/time words; leave it.)
- **The scorer.** `bench/recall/score.py` `abstained_as` (near :391): `own = _subject_words(p.subject)`; `others` =
  the other facts' subject words of the same kind minus `own`; then `own -= union(all same-kind subjects' words)`.
  Tests: `bench/recall/test_score.py`.
- **Resume.** `crates/theseus-core/src/toolrun/resume.rs` near :360-375: the `alive` check reads
  `sp.read_pid(corr)`, and the `alive` branch reads it again for the result's `meta.pid`; between the two reads the
  wrapper can end and remove the file, so the result says a job is running while its `pid` is `null`.
- **The nit.** `crates/theseus/src/interactive.rs` `on_note` (near :612-620): `if self.json { … } else if again { }
  else if n.method == notify::EVENTS_LOST { … } else { self.printer.on(..) }`.

## What to build
1. **theseus-vymxh: no argv file after a failed spawn.** Add `Spool::remove_argv(&self, id: &str)` (best effort, as
   `remove_pid` is) and call it in `spawn_detached` when the spawn fails, before the error returns (e.g. map the
   `children::spawn` result: on `Err`, remove, then return the same error with the same context). `remove_pid` may
   call `remove_argv` so there is one place that knows the file. Nothing else in `spawn_detached` changes; the
   `write_pid` failure after a successful spawn is not this row (the wrapper is running and reads the file).
2. **theseus-2rvwv: a held turn says it was held, and names no model.** The turn's stop reason is `held`:
   `hold_reply` sets `t.stop_reason` (a `pub const STOP_REASON: &str = "held"` in `personal_step.rs`) and the node's
   own `stop_reason: Some("held")`, so the stored node is marked as no model call. The node keeps `model`/`provider`
   as today (history, cost and the cockpit read them; changing them is a store question this row does not open). No
   `turn.rs` change. The footers:
   - Discord `footer`: when `r.stop_reason == "held"`, push `held: no model ran` in place of `r.model`, and do not add
     the generic `stopped: held` bit. Every other turn's footer is byte for byte today's.
   - CLI turn line (`crates/theseus/src/render.rs`): when held, `[{profile} → held, no model ran · …]` in place of
     `{provider}/{model}`; otherwise unchanged. Keep the edit inside that function; the file is 3,115 / 3,128 lines.
   - Before you pick the word: `git grep -n '"end_turn"\|stop_reason' -- crates cockpit/src` and check every reader
     of a turn's stop reason (task reports, the courier, the TUI, herdr, initiative rows, the turn's ledger fact).
     A held turn must not read as a failure or a stop anywhere a person sees it (it is a finished turn whose answer is
     the fixed line). If a reader treats any unknown reason as a failure, fix that reader for `held` with a test; if
     there are more than three such readers, stop, keep the footer change on a different marker you can justify, and
     say so in the report.
3. **theseus-ztkhg.1: old words carry their age on the three surfaces.** Use `aged_said` exactly as `task_tree.rs`
   does (`doing_since_ms` = the view's `doing.since_ms`, none when it does nothing):
   - TUI Job row: `Form::Job` carries the words' `at_ms` (and the doing's since) so `doing(f, now_ms, ..)` prints
     `aged_said(..)`; the row's other words unchanged.
   - `theseus status`: `status_progress::of_views` takes `now_ms` and builds the same wording with the aged words
     (`step 4 of 5 · Round 1 done. Continuing. (2 min ago)`). Do not change `Progress::line()` (other callers rely
     on it): compose `step_words` and `aged_said` in `status_progress.rs`. `lines(conn)` passes the wall clock.
   - Discord task view: `**Last words**` shows the aged words (the age on the first line); the renderer stays pure:
     take `now_ms` from its caller as `board_v2` does, and thread it through the one call site.
   - Out of this row: the issue's third note (a row read `model 1:48` while a `proc.run` ran; not reproduced, likely a
     stale view): it is the maintainer's live walkthrough. Say so in the report.
4. **theseus-c93va: the scorer's fallback, and resume's one read.**
   - `abstained_as`: after `own -= …`, `if not own: own = _subject_words(p.subject)`, so a probe whose subject has no
     distinctive word still counts its own (shared) words; update the docstring's last sentence to say what the code
     does (shared words name neither subject, unless the asked subject has no word of its own, when they name it).
     Rescoring any kept run is not this row.
   - `resume.rs`: read the pid once (`let pid = …read_pid(corr)`), test `alive` on it, and use the same value for
     `meta.pid`. No behaviour change beyond the race.
5. **theseus-jjnud:** fold `again` so the branch reads as intended, e.g. `} else if !again && n.method ==
   notify::EVENTS_LOST {` … with the last `else` becoming `else if !again`, or keep the branch with a one-line comment
   saying a re-told question is on the screen already and prints nothing. Pick the clearer; no behaviour change.

## Tests (each with a planted revert; the first is the key test)
- **`a_failed_spawn_leaves_no_argv_file`** (in `crates/theseus-kernel/src/job.rs`'s tests or `kernel/tests/`): call
  `spawn_detached` with a `self_exe` that does not exist and a scratch spool; it returns `Err`, and
  `spool.argv_path(id)` does not exist afterwards; and a sibling case: a successful spawn of a real wrapper still finds
  its file while it runs (reuse an existing test's setup if one spawns a wrapper; name it). Plant: drop the
  `remove_argv` call; the key test must fail.
- `a_held_reply_names_no_model` (in `tests_personal_hold.rs`): a held turn's result has `stop_reason == "held"`, the
  stored assistant node's `stop_reason` is `Some("held")`, and the existing assertions (no request, the fixed line)
  still hold. Plant: leave `t.stop_reason` unset.
- Footer tests: Discord `footer` on a held result contains `held: no model ran` and not the model's name; on an
  `end_turn` result it is byte for byte today's (compare with an existing footer test's expected string). The CLI's
  turn line likewise. Plant: drop the `held` branch in each.
- Ages: one test per surface with views whose words are 2 minutes older than what the task does (`(2 min ago)`
  appears) and one where they are 30 seconds old (no age): `crates/theseus-tui/src/work.rs`'s tests (Job row),
  `status_progress.rs`'s tests (extend `a_session_gets_its_count_and_its_line` or add one), and the task view's tests
  (where `render/task_view.rs` keeps them). Plant: the plain words on each surface.
- `bench/recall/test_score.py`: a probe whose subject's words are all shared with another fact's subject, and a reply
  giving a value in a clause with those words, scores `misleading`; the existing cases unchanged. Plant: drop the
  fallback. Run with `python3 -m pytest bench/recall -q` (or the way `bench/README.md` says).
- `resume.rs` and `interactive.rs`: no behaviour change, no plant; say so. The existing tests that cover them must
  pass (`cargo nextest run -p theseus-core resume`, `cargo nextest run -p theseus interactive`, or the names you find).

## Not in this row
- theseus-cljlz (a held turn still pays its compile, and a compile failure fails it): the fix sits in
  `turn/route_step.rs`'s `compile_first`, which another lane is changing now; it waits for that lane.
- theseus-1qexl (owner-side docs and rescoring a kept bench run): the maintainer's.
- Any change to `turn.rs`, `turn/route_step.rs`, `theseus-protocol/src/lib.rs`, `Progress::line()`, the Discord
  runtime (`crates/theseus-discord/src/runtime*`), `render/board_v2.rs`, the store layout, the cockpit.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure refactor needs no plant: say so.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model-facing text and
  no tool. FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`), nothing on the path before a request leaves.
- **Line ceilings** (`scripts/long-files.txt`, length / ceiling at `6918e3c1`): `crates/theseus-discord/src/render.rs`
  2,976 / 3,009, `crates/theseus/src/render.rs` 3,115 / 3,128, `crates/theseus-core/src/turn.rs` 3,427 / 3,525 (you
  add nothing there). Never raise a ceiling. Put new code in the files named above.
- **One file shared with a sibling row.** Another row of this batch adds one `Buttons` variant near the top of
  `crates/theseus-discord/src/render.rs` (the enum near :53-80). Touch only `fn footer` there (at most 3 added lines),
  reformat nothing else in the file, and do not run `cargo fmt` over the whole crate if it would move other lines.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  (format 48 on your base): a new value in an existing `stop_reason` string field is not one.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`theseus-kernel`, `theseus-core`, `theseus-discord`,
  `theseus`, `theseus-tui`, and `bench/README.md`). Keep CLOUD_REPORT.md honest: what you ran, what passed, what you
  could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM; this
  row needs none of them, so say which you ran). Run each touched crate's tests (`cargo nextest run -p <crate>`) and
  `cargo clippy --workspace --all-targets -- -D warnings` before the last commit.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: what changed per Beads id; each test and its plant, with the failing output's line; the
stop-reason readers you checked for `held` and what each does; what could not run on the VM and the exact commands for
the maintainer; the keel guard's output; the store format (unchanged); follow-ups (the live walkthrough note of
theseus-ztkhg.1, and anything you found). Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
