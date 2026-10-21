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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-onto-r8-r6`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `1d092ded` (2026-10-20). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (67 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
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
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,404 / 3,525
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
- On the maintainer's machine: task questions and reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI
  chat (`crates/theseus-tui`), herdr (`contrib/herdr`).

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: onto-r8-r6, measure the ontology for real: the labelled replay over the store, the +onto report's plan and format, a sharper decisions column, and a session's thread and project in the compile's manifest (theseus-nngor, theseus-ri5up, theseus-2e6c1, theseus-w0vyp)

Branch: `cloud/20261021-onto-r8-r6`. Every commit's subject carries the Beads id of the part it builds
(`theseus-nngor`, `theseus-ri5up`, `theseus-2e6c1` or `theseus-w0vyp`). Deadline for the report: 6 hours after you
start.

**Why.** Jev, the fast classifier, now files every private message into the ontology (`onto.v1`), and the owner
asked for it to be measured as evidence that fast in-loop classification makes the harness stronger, in a report
whatever it says. The measuring parts exist only as pure code (`580c8c37`), so today:
- **Accuracy cannot be read.** `crates/theseus-judge/src/replay.rs` scores `onto.v1` against labels, proven on a
  scripted model only. The owner's import (`theseus import topics`) already labelled hundreds of past sessions, and
  nothing reads those labels into it: there is no `theseus judge replay --onto`.
- **The undo rate is nowhere.** An undo is labelled `wrong` (`ontology.undo`), but nothing says what share of Jev's
  writes per kind the owner took back.
- **The +onto bench arm (A1o) has no plan or report text**, and shares chart slot 5 with Pi, so a figure holding both
  would draw two series in one colour.
- **The decisions column is coarse.** One `onto.v1` judgment that filed a topic, a resource and a privacy answer
  counts as one filing, and privacy counts only `security.*` packs, never `onto.v1`'s `personal` and `shows_*`.
- **A turn cannot say it was in a thread or a project.** The manifest records each membership the compile used
  (`MembershipUsed`) so "why did it know that?" has an answer; a session's thread and project (which reach the
  request through the thread's card and the project's slice) are never in it, nor in `theseus ontology`.

**Risk: low.** Measurement code, report text, a manifest addition (no request byte changes). The replay spends real
Jev money only when the owner runs it, inside `[judge] replay_limit_usd`.

**Prerequisite: `main`.**

## What exists on `main` (search by name; lines move)
- `crates/theseus-judge/src/replay.rs` (from "onto.v1's labelled replay" down): `LabelledCase { session, input:
  InboundInput, labels }`, `hide_labels`, `replay_onto(judge, pack, cases)` (one decision point, labels hidden),
  `Pick`, `Scored`, `Floor`, `KindScore`, `score_onto`, `OntoReplay`, `onto_report`, `onto_markdown`. Tests:
  `tests_replay_onto.rs` (read it whole: it shows how a case's input is built so the labels are offered as
  candidates but the session's flag is hidden).
- The core's replay of other packs: `crates/theseus-core/src/learning/replay.rs` (989 lines: add nothing to it but a
  call if you must). Its rules you follow: each call a `judge.call` row with `purpose: replay`, scoped
  `judge.replay:<pack id>` (the nightly report never reads it); the run a `judge.replay` row (`rpl_…`) in the same
  frame; spend priced and reserved, checked first against `[judge] replay_limit_usd` and refused with the numbers;
  never the shadow day budget. `learning/rebuild.rs` rebuilds inputs from the record.
- The live point's input functions: `judge/inbound.rs` `pub fn input(nodes, msg, live_tasks) -> InboundInput`, and
  `judge/onto/mod.rs` `fn onto_input(core, o, msg) -> OntoInput` (private: make it `pub(crate)`, no other change).
- Import labels: `crates/theseus-core/src/import/topics.rs` `assign` writes memberships with `Origin::Import`; the
  held snapshot (`runner.ontology.snapshot(&store)`, `Ontology::memberships(session)`) carries each membership's
  origin. Use the snapshot, not a ledger scan.
- Undo rows: `fact/ontology.rs`: a machine write's records' rows carry `undo: {write, key, wrote, before}`;
  `OntologyUndo` (`ontology.undo`) names `write`, `label` (`wrong`), `keys`.
- The CLI: `crates/theseus/src/judge_runs.rs` `ReplayArgs` and `replay`; the wire: `crates/theseus-protocol/src/
  judge_runs.rs` `JudgeReplayParams`, `JudgeReplayResult` (exported for the cockpit through `ts.rs`); the render:
  `render::judge_replay_lines`. Backfill sends the owner's history to Jev only under `[judge] backfill_consent = true`
  and the client refuses inside a job (`client::refuse_in_a_job`).
- `bench/threads/`: `th_drive.py` (`ARMS`, A1o defined; `check_plan` holds a run to a committed plan),
  `th_threads.py` (`OVERRIDES`, `judgment_row`), `th_score.py` (`decision_class`, `decisions`), `report.py`
  (`CHART_KEY`, `figures`, `decisions_section`, the "What T12 (the after run) needs" text, `reproduction`),
  `bench/plans/threadbench-t12.json` (k 1, arms A1 A1n A2 A3, the dev, held-out and prefix digests), tests
  `test_th_onto.py`, `test_th_threads.py` (it checks the T12 plan), `test_th_score.py`, `test_th_smoke.py`.
- The compile walk: `crates/theseus-core/src/ontology.rs`: `given(place, snapshot, now_ms)` (guild and channel from
  the place, origin transport, never stored), `Walk::of`, `Walk::with_topics`, `Walk::compose`,
  `TurnRunner::walk(session, class)` (it already reads the session's thread through `self.thread_topics(session)`,
  `thread_context.rs`). `crates/theseus-ontology/src/compose.rs`: `Composition { sections, memberships, guidance,
  skipped }`, `MembershipUsed`. The kinds `thread` (parent channel, precedence 25) and `project` (precedence 28) are
  seeded as given kinds whose descriptions say "read at compile". `category_recall.rs` `CategoryScene::keys_of`
  already names a session's thread `thread:<slug>` and its project `project:<id>` (the session record's `project`,
  a `tsk_…`). `rpc/ontology.rs` (near the `crate::ontology::given(` call) lists a session's memberships for
  `theseus ontology`.

## Your row
Four parts, each its own commit, in this order.

**1. The labelled replay over the store (theseus-nngor).** In a new module `crates/theseus-core/src/learning/
replay_onto.rs`:
- **The cases**: every private session the snapshot holds at least one `Origin::Import` membership for (a kind
  `onto.v1` asks: topic, project, event, resource, capability, expertise), newest first, at most `limit` (default 50).
  For each, the session's first human message, read from its nodes, made into the `Inbound` the live point would
  have made and passed to `judge::inbound::input` and `onto_input`, with the ontology as it stands now. Labels: per
  kind, the ids of its import memberships. A session whose first message cannot be read is left out with its reason
  (the result lists them). Never a shared place's session, never a task's.
- **The run**: the wired `onto.v1` (or a candidate version by name, as the existing replay takes one), through the
  judge's own client and breaker in shadow (`replay_onto` with the live judge), under `[judge] replay_limit_usd`
  (the estimate refused with its numbers before any call) and only when `[judge] backfill_consent = true` (else
  refused with the words the backfill uses: the replay sends the owner's history to Jev). Rows as the existing replay
  writes them (`judge.call` with `purpose: replay` scoped `judge.replay:onto`, one `judge.replay` row for the run).
- **The undo rate**: a pure function `undo_rates(rows, since_ms) -> Vec<UndoRate>` (per kind: Jev's writes in the
  window, how many the owner undid, the rate) over the `ontology.*` rows whose `undo.write` is a judgment id
  (`jdg_…`) and the `ontology.undo` rows naming them; a write's kind is its records' kind. Default window: the last
  7 days. Keep it `pub(crate)` and pure so the learning report's line (another lane's) can call it.
- **The wire and CLI**: `JudgeReplayParams` gains `onto: bool` and `limit: Option<u32>`; the result gains
  `onto: Option<OntoReplayReport>` (a new struct in `judge_runs.rs`: the `onto_report` numbers per kind at both
  floors, the unanswered and left-out counts, the undo rates, and `onto_markdown`'s text). `theseus judge replay
  --onto [--limit N]` prints the Markdown; `--json` prints the result. Export the new type in `ts.rs` and regenerate
  the cockpit's generated types under `cockpit/src/protocol.gen/` as `crates/theseus-protocol/AGENTS.md` says
  (`the_cockpits_types_are_generated_from_the_rust_ones`; a generated-file change only: no cockpit code). Add nothing to `crates/theseus-protocol/src/lib.rs` (three lines of room; the method already exists).
- **Docs**: a short section in `bench/threads/README.md` or `docs/benchmarks/README.md` (whichever already describes
  report generation) saying how the replay's Markdown becomes the accuracy part of the +onto report (part 2).

**2. The first +onto report's plan and format (theseus-ri5up).** The bench run itself is the maintainer's, after the
join, with live Jev; this part makes it a mechanical run.
- **The plan**: `bench/plans/threadbench-t13-onto.json`, committed before the run, in T12's form: `k` 3, model and
  effort as T12, `arms` A1o and A1 (the pair that differs by the ontology alone), with A3 and A5 named in an
  `about`/`comparison` field as read from the T12 runs on the same frozen binaries (rerun only if the binaries
  differ: say so in `about`), the dev and held-out progression digests copied from T12 (the held-out run "once,
  after the development run is read"), `budget_usd` expected 33 to 40 for A1o and A1 at k 3 on dev, a pause line,
  and `spend_limit_usd_per_arm`. A test beside T12's in `test_th_threads.py` checks it the same way (`hp.check`, the
  digests, A1o named).
- **Jev's overlay**: `bench/threads/jev-overlay.example.toml`: the `[judge]` keys the daemon needs to reach Jev
  (read `crates/theseus-core/src/config/judge.rs` for the names), every secret as a secret reference placeholder,
  never a value; the README says the maintainer copies it and passes `--config-overlay`.
- **`report.py`**: the "What T12 needs" block and `reproduction` list A1o; a new run-text block for the +onto run (the
  plan above, the overlay, the commands: generate, drive A1o and A1 per seed, score, report, held-out after); a new
  section **"The ontology arm against its control"** when both A1o and A1 are in the scores: per-stream correctness,
  contamination, routing accuracy, context tokens and dollars per turn, latency, each A1o minus A1, and the decisions
  column; and `--onto-replay <file>` that puts the replay's Markdown (part 1's output) under "Accuracy against the
  import's labels", or says "not run" when absent. Nothing tuned, reported whatever it says.
- **The palette**: `figures` refuses (a clear error naming both arms) to draw one figure in which two runs map to
  one chart slot (A1o and A7 both on slot 5); a test shows it, and one that A1o with A1 to A6 draws. Do not touch
  `bench/report/charts.py`.

**3. The decisions column, sharper (theseus-2e6c1).** New logic in a new module `bench/threads/th_decisions.py`;
in the existing files change only `th_threads.judgment_row`, `th_score.decisions` (it delegates) and
`report.decisions_section`:
- `judgment_row` keeps, per answer, its question id, its band, and whether the answer chose something (a Choice not
  `none`/`other`, a Noul yes) beside today's fields; it also keeps `context.turn` and `context.session` and the
  judgment's time when the row has them. Never the state or any text.
- **A decision is a decision question answered at the act band**, by class: filing = `onto.*` kind questions
  (topic, project, event, resource, capability, expertise) and `categorize.*`; routing = `route.*`, `classify.*`
  (its `thread` question); privacy = `security.*` and `onto.*`'s `personal`, `elicits` and `shows_*`. The per-pack
  count stays beside it as `judgments` so old scores still read.
- **Per-turn attribution**: a judgment is counted to the turn its `context.turn` names when `turns.jsonl` carries
  that id; else by session and time (the turn whose window holds the judgment's time); else "between turns". The
  column reports per turn: mean, and the share of turns with at least one decision.
- The main model's dollars stay modelled (a floor), worded as today. A run whose rows lack the new fields (an old
  run) still scores, with the per-question numbers marked "not recorded".

**4. A session's thread and project in the manifest (theseus-w0vyp).** The owner had asked whether a compile-time
reader is worth it; the patch1 goal line takes it in the smallest form: a record of what the compile already used,
changing no request byte.
- A function beside `given` (e.g. `harness_given(thread: Option<&str>, project: Option<&str>, now_ms) ->
  Vec<MembershipUsed>`) gives a session's thread as `thread:<slug>` and its project as `project:<tsk id>` (the same
  keys `CategoryScene::keys_of` uses), kind `thread`/`project`, origin transport, no confidence.
- `TurnRunner::walk` passes them to the walk (it already reads the thread board for `with_topics`: read the thread
  once), and the walk appends them to the composition's `memberships` after `compose`, so the manifest records them;
  they add no section, no guidance and no `skipped` entry. A shared place's walk records them too (they are the
  harness's facts, like the channel). `rpc/ontology.rs`'s session listing shows them the same way.
- Nothing is stored, no category record is made for a thread or a project, and `thread_route.rs`,
  `thread_recall.rs` and `category_recall.rs` are untouched.

## Tests (each with a planted revert; the first is the key test)
- **`an_import_labelled_store_replays_through_onto`** (core, `FakeJev` scripted per session to answer the import's
  labels): a store with three imported sessions labelled by topic and resource and one labelled by nothing; the run
  scores precision and recall 1.0 for those kinds, the unlabelled session is not a case, the state sent to Jev never
  names a label as the session's membership (assert on the state JSON the fake judge received), and the rows are
  scoped `judge.replay:onto`. Plant A: build labels from Jev-origin memberships instead of import's (the score
  changes). Plant B: skip `hide_labels` (the state assertion fails).
- `the_onto_replay_is_refused_without_backfill_consent` and `the_onto_replay_is_refused_over_replay_limit_usd` (no
  call made, the numbers in the error). Plant: drop the consent check.
- `undo_rates_count_the_owners_undos_per_kind`: Jev writes of two kinds, one undone, one sweep write undone (not
  counted: not Jev's), a write outside the window. Plant: count every `ontology.undo` row.
- `judge replay --onto` CLI render test (the Markdown lines), and the protocol round trip of the new params/result.
- Python: `test_th_onto` or a new `test_th_decisions.py`: one `onto.v1` judgment with a topic, a resource and a
  `personal` answer at the act band counts two filing decisions and one privacy; a confirm-band answer counts none;
  attribution by `context.turn` and by time; an old run's rows score with "not recorded". Plant: count per judgment.
- `report.py`: the A1o-against-A1 section from two fixture runs (signs and arithmetic checked by hand), the
  `--onto-replay` section present and absent, the slot-collision refusal, and the new plan's check. Plant: drop the
  collision check.
- `the_manifest_records_the_sessions_thread_and_project` (core): a session in a thread and attached to a project
  compiles a manifest naming `thread:<slug>` and `project:<tsk>`, and the system block's bytes are equal to the same
  compile without them. Plant: drop the append (the first assertion fails); second plant: give them to `compose`
  (the bytes or `skipped` change).
- Run the whole `bench/threads` suite (`python3 -m unittest discover -s bench/threads`); `test_th_threads.Ask` has
  two known BrokenPipe flakes on some hosts that fail the same on the base: if you see them, show they fail on the
  base too.

## Not in this row
- **The cockpit** (an ARM lane does the Ontology view's extras: the tree's origin filter, the add-a-kind form, the
  Observatory line, and the learning report's ontology line with the undo rate per kind). You add
  `undo_rates` for it to call; do not edit `cockpit/` (beyond the regenerated `protocol.gen/` files) or
  `learning/report.rs`.
- **The sweep and the dream** (cloud row onto-r5-extras: `judge/onto/{decide,sweep}.rs`, new packs, `consolidate/`,
  the morning note). Do not touch them.
- **Other bench work tonight**: the threads scorer fix (`th_oracle.py`, the check functions in `th_score.py`,
  `th_beats.py`) is being done on the maintainer's machine, and a threads routing row passes the plan's message times
  through `th_threads.py`. Edit only the functions this task names in those files, keep each edit small and additive,
  and put new logic in `th_decisions.py`.
- Running any benchmark with a live model (the maintainer's, after the join); anything under `docs/benchmarks/` except
  the README note of part 1.
- Threads files in `crates/theseus-core` (`thread_route.rs`, `thread_recall.rs`, `thread_context.rs` beyond reading
  through `thread_topics`' board), `theseus-memory`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The replay runs only when the owner asks; the manifest record is the
  compile's default, its rollback the part's revert (no request byte changes: the test shows it).
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). A new model tool is deferred by
  default (this row adds none). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the
  path before a request leaves (the walk reads the thread board once, as it already does), and nothing on the start
  path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`,
  then `git status`); list each plant in the report with the test that failed. The row's **key test** (named first
  under Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last
  commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the
  report. It must be empty unless this task names keel findings.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  (none is needed: new rows' fields and new wire fields with serde defaults only).
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push, a state sent to Jev) through `core.tools.scrubber`. No person's
  name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture paths and names are
  invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first, where it has one (`theseus-core` and
  `theseus-protocol` do), and `bench/threads/README.md`. Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could
  not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM; none of
  yours should need L1), and give the maintainer the exact commands for the rest.

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its
size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. The report has: any file left over a line ceiling (first section, with why); what changed,
per part and Beads id; each test and each plant with the test that failed; the exact commands the maintainer runs
for the first +onto report (the replay: `theseus judge replay --onto --limit 50` on the owner's daemon, with its
cost estimate; the bench: the plan's commands), and the dollar estimate for each; what could not run on the VM and
the commands for it (`cargo nextest run -p theseus-core -p theseus-judge -p theseus-protocol -p theseus`,
`python3 -m unittest discover -s bench/threads`, `scripts/gate.sh`); the keel guard's output; the store format (none
expected); follow-ups (for example the measured main-model price once a real run exists).
