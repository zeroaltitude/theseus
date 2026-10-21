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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-recall-abstain`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: recall-abstain, a job found alive after a restart gets its handle, and the recall bench tells an honest "not that, but this" from a wrong value (theseus-53ujg.8, theseus-53ujg.7)

Branch: `cloud/20261021-recall-abstain`. Every commit's subject carries `theseus-53ujg.8` or `theseus-53ujg.7` (the
part it belongs to). Deadline for the report: 4 hours after you start.

**Why.** Two findings of the v1 benchmark run (2026-10-18), each read to its cause before this task was written:

- **A job the restart found alive is a stranger to the model** (part A, theseus-53ujg.8). The async bench's kill
  family SIGKILLs the daemon mid-chore and restarts it. Theseus repeated no effect in 12 kills, but left one *orphan*,
  in a `service` trial (DME3rjP). Its records (the chore's own effect ledger, the daemon log's counters and the
  history's node metadata) show what happened:
  1. The model ran a blocking `proc_run` poll loop (`for i in $(seq 1 30); do … query-service stock …`), not in the
     background. The SIGKILL landed while it ran.
  2. The restarted daemon found the job's wrapper alive (`jobs an earlier process left probed=1 … alive=1`,
     `interrupted=1`). So the process group *was* adopted: the first guess, "neither adopted nor stopped", is wrong.
  3. The resumed turn answered the interrupted call with `toolrun/resume.rs` `check_dispatched`'s placeholder:
     `Still running as background job act_<long id> (the harness restarted meanwhile). Its result will arrive in a
     later message.`, with `meta: null`. A fresh background job's placeholder says `Started job j1 (bash -c …) in /app
     …; job_read j1 shows its latest output, job_wait j1 waits for it, job_stop j1 stops it.` and has meta
     `{"job": "j1", "pid": …}`. The restart's placeholder has no short id, no pid, and no word of the job tools.
     Because the session's short ids are read back from placeholders' meta (`toolrun/handles.rs` `read`), this job
     has none: `job_read` with no job lists the session's running jobs from that map and leaves it out.
  4. The model then ran `query-service stock` itself (a new process) while the adopted loop's `query-service` still
     ran; the old one finished 10 s later, after the redo. That is the orphan.

  A person's dev server or migration interrupted by a daemon restart is the same case. Whether the model would have
  waited, had it been told, is a hypothesis; that it was not given the job's handle is a fact, and a bug: every other
  still-running job gets one.

- **The recall bench counts an honest answer as a wrong one** (part B, theseus-53ujg.7). Asked about a subject never
  stated ("which port is the cedar relay on?", near a stated "the alder relay is on 4432"), Theseus "gave a value" on
  38 of 60 abstention probes (Claude Code 36, Pi 21). A counts-only pass over the kept run (a script over the
  progression and the replies; no reply read by eye) found the same shape in every arm: **all 38 Theseus misses admit
  it does not know** (the scorer's own `progression.ADMIT` matches) **and then name values of the asked kind that
  belong to other subjects, each beside its own subject's words**: 22 name one such value, 16 two or more; only 8
  name the probe's planned distractor; none is confident-wrong. The check says "no value of the kind, and an
  admission", so "Nothing on the cedar relay; the alder relay is on 4432" fails it. For a person that answer is not
  misleading. What would mislead is a value given *for the asked subject*, or a neighbour's value with no subject.
  The bench cannot tell these apart, so nobody can say whether Theseus has an abstention problem. This part makes the
  scorer say it, for every arm alike; the strict number stays as it is so the series stays comparable.

**Risk: low.** Part A changes one placeholder's words and meta and registers a short id; part B changes bench code
only.

**Prerequisite: `main`.**

## Your row

| Part | Beads | Scope | Files |
|---|---|---|---|
| A | theseus-53ujg.8 | the restart's placeholder for a job found alive gets a short id, meta `{job, pid}`, and the same words as any still-running job | `crates/theseus-core/src/toolrun/resume.rs` (`check_dispatched`'s alive branch), `crates/theseus-core/src/toolrun/jobs_tools.rs` (only if you share one wording function with `started`), a new test in `crates/theseusd/tests/` and one in `theseus-core` |
| B | theseus-53ujg.7 | the recall scorer classifies every abstention reply: `clean`, `neighbours`, `misleading` | `bench/recall/score.py` (`score_probe`, `Scored`, `summarize`, `report`), `bench/recall/test_score.py`, `bench/recall/README.md` ("Each score") |

## What exists (on your base, `origin/main` at `1d092ded` or later; search by name)

- `toolrun/resume.rs` `check_dispatched(tc, u, a)`: for a call dispatched before a restart, a settled job gets its
  result (`job_settled` → `answer_job`); a job whose wrapper is alive (`spool.read_pid(corr)` and
  `theseus_kernel::job::wrapper_alive(pid, corr)`) gets the placeholder above as a `ResultNode` with
  `ResultStatus::Background` and no meta, and the call returns `CallOutcome::Background`; anything else is marked
  unknown (`interrupted_by_restart`).
- The words of every other still-running job: `toolrun/job.rs` `placeholder(...)` (three forms: asked for the
  background, a message ended the wait, past `proc_sync_secs`) and `toolrun/jobs_tools.rs` `started(name, spec,
  output)`. Each names the short id and the job tools; the meta carries `job` (`handles::KEY`) and `pid`.
- `toolrun/handles.rs`: `Handles::assign(store, session, corr, argv) -> "jN"` (the one it has, else the next),
  `of_session`, `name_of`, and `read(store, session)`, which rebuilds a session's map from its tool results that are
  `Background`, not late, and whose meta names `job`. `jobs_tools.rs` `argv_of(store, a)` reads a job's argv from its
  call's proposal; `label(argv)` cuts it to sixty characters.
- `crates/theseusd/tests/continuations.rs` `a_restarts_continuation_runs_on_the_profile_of_the_turn_it_cut`: a real
  daemon (`Rig`, `common::model::FakeModel`), a job inside its turn, `drop(daemon)` (a SIGKILL), a new daemon, and an
  assertion that the model's requests contain `the harness restarted meanwhile`. **Keep that phrase** in the new words,
  so this test passes unchanged.
- `bench/recall/score.py`: `score_probe(run, p, stale_rule)` computes `correct` with `check_of(p)` (an abstention's
  check is `progression.kind_check(p.value_kind)`: "reply lacks <a value of the kind>" and "reply has <ADMIT>"),
  `hedged`, `confident_wrong`; `pg.find_values(kind, text)` finds a reply's values of a kind; `_clauses(text)` cuts a
  reply into clauses (sentences, then at `;` and dashes); `pg.Fact` has `subject` ("the alder relay's port") and
  `value`; `pg.Probe` has `subject`, `value_kind`, `distractors` (fact ids). `summarize(rows)` makes each arm's
  numbers (`abstention_accuracy` and the rest); `report(...)` writes `report.md`; `main` rescoring a kept run is
  `python3 bench/recall/score.py <run dir>… --out <dir>`.

## What to build

### A. The restart's placeholder names its job (theseus-53ujg.8)
1. In `check_dispatched`'s alive branch: give the job its short id with `Handles::assign` (the session's map, the
   call's argv from its proposal as `argv_of` reads it), and answer with meta `{"job": jN, "pid": pid}` (the same keys
   a fresh background placeholder has, so `handles::read` finds it after a later restart too).
2. The text, one fact more than the others and nothing else: `Still running as job jN (<label>) in <dir>: the harness
   restarted meanwhile and found it running. job_read, job_wait or job_stop it; its result also arrives by itself
   when it ends.` (the same tail as `placeholder`'s other forms; exact words yours, the phrase `the harness restarted
   meanwhile` kept). If sharing one function with `placeholder`/`started` is clean, share it; do not refactor them
   otherwise.
3. No change to what the restart adopts or how it decides a wrapper is alive (`wrapper_alive` is job-truths' area,
   below); no change to the unknown branch.

### B. The scorer tells the three apart (theseus-53ujg.7)
1. A new field on `Scored`, `abstained_as`, for abstention probes only (None for others):
   - `clean`: the check passes (admitted, no value of the kind).
   - `neighbours`: the reply admits (`ADMIT`), and every value of the kind it gives sits in a clause (`_clauses`) that
     names a word of *another* fact's subject (the progression's facts of that kind, the probe's distractors among
     them) and no word of the probe's own subject. "Words" are the subject's distinctive words: drop articles,
     possessives and the kind's own noun ("port", "version", "host", "path", "ticket", "date"); match whole words,
     case-folded.
   - `misleading`: anything else that fails (no admission, or a value with no subject, or a value in a clause that
     names the asked subject).
2. `correct`, `confident_wrong` and `abstention_accuracy` do not change. `summarize` adds `abstention_by_shape:
   {clean, neighbours, misleading}` (counts) and `abstention_not_misleading` (clean + neighbours over scored);
   `report.md`'s abstention table adds these, with a one-line definition under it.
3. Every arm is scored by the same code. Nothing in this part reads which arm it is.
4. `bench/recall/README.md` "Each score": the three shapes, in the README's own style, and that the strict
   abstention accuracy is unchanged.

## Tests (each with a planted revert; the first is the key test)
- **`a_job_found_alive_after_a_restart_has_its_handle`** (in `crates/theseusd/tests/`, a new file or beside the
  continuations test, using `common`): a turn runs a blocking `proc_run` (not `background`) whose command waits for a
  file the test creates later; SIGKILL the daemon while the call still blocks (set `proc_sync_secs` long in the rig's
  config, so the call has no short id before the kill, as in the bench); start a new one. Assert: the
  resumed turn's placeholder for that call has meta `job` = a short id and a `pid`; the model's next request contains
  that short id and `the harness restarted meanwhile`; a scripted `job_read` with no job (the FakeModel's next step)
  lists the job; a scripted `job_wait` on the short id returns its result once the test lets the job end. Find the job
  through the ledger, the spool or the handle, **never by its command text in `/proc`**. Plant: drop the `assign` and
  the meta (today's placeholder). It must fail.
- `an_adopted_jobs_handle_survives_a_second_restart` (core or theseusd): after a second restart, `Handles::read` maps
  the same short id to the same correlation id. Plant: meta without `job`.
- `a_restarts_continuation_runs_on_the_profile_of_the_turn_it_cut` stays green, unchanged.
- Part B (`python3 -m unittest` in `bench/recall`): **`test_abstention_shapes`**, one table: an admission with no
  value → `clean`; "No record of the cedar relay; the alder relay is on 4432." → `neighbours`; "The cedar relay is on
  4432." → `misleading`; "I'm not sure; maybe 4432." (no subject) → `misleading`; an admission and a value in a clause
  naming the asked subject ("not sure, but the cedar relay may be on 4432") → `misleading`; two neighbours, each with
  its subject → `neighbours`. Plant: classify every admitted reply as `neighbours` (the "maybe 4432" and "cedar …
  may be" rows must fail). Use invented subjects and values only.
- `test_strict_abstention_accuracy_unchanged`: the existing scoring fixtures give the same `correct`,
  `confident_wrong` and `abstention_accuracy` as before. Plant: let `neighbours` count as correct.
- `test_summary_counts_the_shapes`: `summarize` over a small fixture.

## Not in this row
- Any change to recall itself, the memory lookup, or the model's prompt. The scorer's split, rescored on the kept run
  by the maintainer, says whether a product change is needed; if `misleading` replies remain, they are the next row.
- The `service` chore's other failure (the model polled `query-service` before `start-service` ended: the chore's
  order check). That is the model's choice, not the harness's.
- **job-truths** (theseus-53ujg.2, in flight on another branch) owns `theseus-kernel/src/job.rs` (including
  `wrapper_alive` and how a wrapper's command line looks), `job_l1.rs`, `toolrun/job.rs`'s `ResultStatus::Unknown` arm,
  `rpc/driver.rs` `wrapper_signalled`, `theseus-tools/src/fs.rs` + `magic.rs`, `file_read.rs`, and the test helpers
  `theseusd/tests/{gone_jobs,job_handles}.rs`, `tests_term.rs`: do not edit them. **effort-on-evidence**
  (theseus-53ujg.3) owns `turn/checkpoint_step.rs`, `routing/effort.rs`, `turn/route_step.rs`, `turn/effort_step.rs`.
  **turn-end-followups** (theseus-53ujg.5/.6) owns `toolrun/jobs_tools.rs`'s `wait`, `sweep.rs`, `rewind.rs`,
  `turn/end_note_step.rs`, `config/limits.rs` and `bench/harbor/`: if you share a wording function in `jobs_tools.rs`,
  keep the edit next to `started` and away from `wait`.
- `docs/status.md`, the README, `docs/design/`, `docs/benchmarks/`, the spec: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** Part A ships on; it has no switch (it gives an adopted job what every other
  job has). Its rollback is reverting its commit; say so. Part B is measurement: the strict numbers are unchanged
  (`test_strict_abstention_accuracy_unchanged`).
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool and no
  description text. FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the
  path before a request leaves, and nothing on the start path grows with history (the placeholder is built only for
  a call a restart interrupted).
- **Facts only.** The placeholder states what exists and what happened (its id, its command, its directory, that the
  harness restarted and found it running, which tools act on it). Never what to do, never "must".
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  (the placeholder is an ordinary tool result node with meta, as today's background placeholders are).
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths, subjects and values are invented.
- Line ceilings: `scripts/long-files.txt`. Put new code in new modules; in a long file add only the `mod` line, the
  field or the call. Add no line to `crates/theseus-core/src/toolrun.rs`.
- Read the AGENTS.md of every crate you touch first (`crates/theseus-core/AGENTS.md`: "A job's handle";
  `bench/recall/README.md`), and add one sentence to the job-handle paragraph saying a job a restart found alive gets
  its short id too. Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud (L1
  jobs need a non-root daemon with a job cgroup, which the VM lacks; part A's key test runs L0: say if you could not
  run it, and give the maintainer the exact command).

## Your report
Commit `CLOUD_REPORT.md` at the repository root with: what changed (part A, part B, with the commits); each test and
its plant, and whether the plant failed it; the full test commands you ran and their counts; what could not run on
the VM and the exact commands for the maintainer, including the rescoring of the kept v1 recall run
(`python3 bench/recall/score.py <the three arms' run directories> --out <dir>`: the maintainer has the run) and what
the new shapes should be compared against (Theseus 38 misses, Claude Code 36, Pi 21 under the strict check); the
keel guard's output; the store format (unchanged); follow-ups (anything you found in another row's area, named, not
changed). Then push the branch and end with a final message that starts `CLOUD REPORT COMPLETE`.
