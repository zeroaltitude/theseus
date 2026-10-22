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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-effort-400`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: tail-effort-400, a turn's later requests carry the per-message effort beta, and a refusal no longer lasts until restart (theseus-blzt6)

Branch: `cloud/20261021-tail-effort-400`. Every commit's subject carries `theseus-blzt6`. Deadline for the report: 3
hours after you start.

**Why.** In a walkthrough rehearsal on the installed build of `6918e3c1` (a scratch daemon), a Sonnet 5.5 turn in a
thread got a 400 from the provider:
`invalid_request_error: messages.10.output_config: Extra inputs are not permitted` (ledger `provider.error`, **loop
1**, model `claude-sonnet-5-5`). The thread's earlier turns were short detours on Haiku 5.5 at effort `low`. This turn
(a plain chat message, "start two background tasks", routed into the thread) ran on Sonnet 5.5, made two
`task.create` calls, and **the request after those calls** failed. The run-time fallback (theseus-posww) asked again
without the effort-only messages and the turn ended normally 2 s later, so the person sees no failure. What they lose:
- a second, larger request on every such turn (here $0.0794 + $0.0422), and two errors in health;
- worse, the fallback **marks Sonnet 5.5 as refusing per-message effort for the daemon's life**: every later Sonnet
  turn on that daemon goes back to top-level effort, whose change between requests restarts the provider's cache of
  the messages (the reason per-message effort exists: one such turn on a 625k conversation wrote 612k tokens again).
  Only a restart forgets the mark.

The catalog has said Sonnet 5.5 takes per-message effort since a live probe on 2026-10-17 (theseus-rwpbg.4: "a
per-message change keeps the cache, a top-level one restarts it"). An earlier 400 of the same text on 2026-10-11
(`messages.190.output_config`) is why the run-time fallback exists.

**The likely root, found by reading the code at `6918e3c1` (prove or refute it first).** In
`crates/theseus-core/src/compiler/effort.rs`, `place` inserts each earlier answer's recorded effort as an effort-only
system message (`{"role":"system","content":[],"output_config":{"effort":…}}`), then calls `retarget`. **Only
`retarget` calls `sync_beta`**, which adds the beta `mid-conversation-output-config-2026-07-01`
(`BETA_TURN_EFFORT`) to exactly the requests that carry such a message. But `retarget` returns early, before
`sync_beta`, when the last message does not open a turn (`if !opens_turn(&req.messages[at]) { return None; }`), which
is every request after a tool call (its last message is the tool results). `compile` builds `betas` fresh on every
request (`crates/theseus-core/src/compiler.rs` near :1001-1036). So on loop 1 and later, a request whose history holds
any recorded effort message, including the one the turn's own first request placed (its first answer records it, and
`place` renders it again), is sent **with the message and without the beta**. A field the API does not know without its
beta reads exactly as "Extra inputs are not permitted". This fits both 400s (loop 1 here; message 190 deep in a long
conversation on 10-11) and the probe's success (a first request, where `retarget` does sync the beta). The Haiku turns
are then incidental: Haiku has no `per_message_effort`, so its answers record nothing; the level change that placed
the message was this Sonnet turn's own (Jev put it at a level other than the profile's).

**Risk: low-medium.** The request's beta list and the refusal mark. A wrong fix either sends the beta where no message
needs it (harmless, but it changes request bytes) or still sends a message without it (the 400 stays, the fallback
hides it). The fallback stays as a safety net.

**Prerequisite: `main`.**

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail-effort-400` | theseus-blzt6 | every request that carries an effort-only message carries the beta, at every loop; the refusal mark is time-bounded; comments say what was found | `crates/theseus-core/src/compiler/effort.rs`, `crates/theseus-core/src/turn/effort_step.rs`, `crates/theseus-core/src/tests_turn_effort.rs` (new tests; or a new `tests_effort_beta.rs` with one `mod` line), `crates/theseus-core/src/compiler/tests.rs` (a unit test, if it fits better there), comments in `crates/theseus-core/src/catalog.rs` and `crates/theseus-core/AGENTS.md` (the effort paragraph near :1590-1606) | S |

## What exists (lines at `6918e3c1`; search by name)
- `compiler/effort.rs` (209 lines): `BETA_TURN_EFFORT`; `default_of(entry)` (the catalog's `per_message_effort`);
  `takes(spec, entry, model)` (first party, catalog says so, model not in `spec.effort_refused`); `refused_by(req,
  class, error)` (a 400 of class `invalid_request` naming `output_config` on a request that carries a message);
  `output_config` (the top-level effort); `message`, `level_of`, `opens_turn`; `sync_beta`; `place(req, answers, spec,
  entry, tail_from)`; `retarget`; `placed` (what a request placed before its turn's user message: the answer's record,
  store format 27); `effective` (test only).
- `compiler.rs` near :1036: `let tail_from = effort::place(&mut req, &rendered.efforts, spec, entry, tail_from);`
  is the one caller. `rendered.efforts` is each answer's recorded effort at its message's index
  (`compiler/render.rs` near :305).
- `turn.rs` near :2706 records `effort: crate::compiler::effort::placed(&compiled.request)` on the turn's first answer.
  **Do not edit `turn.rs`** (many rows change it now).
- `turn/effort_step.rs` (125 lines): `EffortRefusals(Mutex<BTreeSet<String>>)` with `mark` and `all`;
  `TurnRunner::effort_refused` feeds `RequestSpec::effort_refused` (`turn.rs` near :847);
  `without_message_effort` (asks again once without the messages, marks the model, records `provider.effort_refused`
  via `fact::turn::EffortRefused` and a `LoopCut` with decision `effort_refused`). Its module comment says "A restart
  forgets the mark".
- Catalog (`catalog.rs` near :765-808): `claude-fable-5-1` and `claude-sonnet-5-5` have `per_message_effort: true`;
  Haiku 5.5 does not.
- Tests: `tests_turn_effort.rs` (237 lines, the rig is `tests_route`'s: `rig`, `turn`, `mode`, a `FakeJev` whose
  `reply_effort` you script, `r.claude.requests()` for what was sent). `a_turn_at_jevs_effort_repeats_the_cached_prefix`
  checks a first request's beta (`r2.betas`); the refusal test near :194-237 checks the fallback's second request has
  no message and no beta. **No test sends a tool loop on a per-message model with a placed message**: that is the hole.
  `tests_effort_sticky.rs` near :268-279 covers the sticky path.

## What to build
1. **Show the root first (red).** Write the key test below and run it on the unchanged code. If it fails as predicted
   (the loop-1 request carries an effort-only message and lacks `BETA_TURN_EFFORT`), that is the root: say so in the
   report with the failing assertion's text. If it passes, the hypothesis is wrong: then find which request in the
   scenario (a thread whose earlier turns ran on Haiku 5.5 at `low`, then a Sonnet 5.5 turn at a level Jev chose,
   with two tool calls) carries a message that its model or its betas do not allow, and write that as the key test
   instead. Stop and report if neither shows a request that is invalid by the code's own rules.
2. **The fix at the root.** The beta follows the messages on every request: `place` calls `sync_beta` once at its
   end, on every path where the model takes per-message effort (after the answers' messages and after `retarget`,
   whether or not `retarget` placed one), and `retarget` no longer needs its own call. A request that carries no
   message carries no beta (today's rule, unchanged). The turn's first request's bytes do not change.
3. **Every message is valid for the model it goes to.** Confirm by test that a model without `per_message_effort`
   (Haiku 5.5) never gets an effort-only message, even when its history holds answers recorded under Sonnet or Fable
   (`place` returns early; keep it so), and that a model that takes them gets the records of answers that other
   models gave (today's design: an answer's record renders whatever model runs now; that is valid once the beta
   rides along).
4. **The refusal mark is time-bounded.** With the root fixed, a refusal is rare and may be the provider's change of
   mind, not ours; marking the model for the daemon's life is too broad. `EffortRefusals` keeps, per model, when it
   was marked; `all` (and so `takes`) leaves out marks older than one hour (`const REFUSAL_HOLDS_MS: u64 =
   3_600_000;`, named in the module comment). `turn.rs` has no clock seam for this at `6918e3c1`, so give the time as
   an argument: `mark(model, now_ms)` and `all(now_ms)`, with the callers in `effort_step.rs` passing the wall clock,
   and test the lapse on `EffortRefusals` directly with explicit times (no sleeping). The row in the ledger stays as it
   is. The per-turn retry stays: once per turn, as today.
5. **Words that are true.** The module comments in `effort.rs` and `effort_step.rs`, the catalog comment beside
   `claude-sonnet-5-5`'s `per_message_effort` and the AGENTS.md effort paragraph say what you found: the 10-11 and 10-21
   refusals were requests sent without the beta (if the test shows it), and the mark now holds an hour. Plain facts,
   no history essay.

## Tests (each with a planted revert; the first is the key test)
- **`a_tool_loop_request_with_an_effort_message_carries_the_beta`** (in `tests_turn_effort.rs`, or a new
  `tests_effort_beta.rs` with one `mod` line beside `tests_turn_effort`'s): on the rig with the live model at a
  per-message model (Sonnet 5.5's catalog row, as the owner's case; use the rig's model switch as
  `a_turn_at_jevs_effort_repeats_the_cached_prefix` does for Fable), a first turn at the profile's level, then a turn
  Jev puts at `low` whose scripted answer makes one tool call (any read-only tool the rig allows) and then ends. Assert
  for **every** request sent: it carries `BETA_TURN_EFFORT` exactly when one of its messages is an effort-only system
  message. The loop-1 request must carry both. (The existing tests switch the rig with `c.model.live = "fable"`; use a
  profile whose model is `claude-sonnet-5-5` if the rig's config has one, else Fable 5.1: the rule does not depend on
  the model, and say which you used.) Plant: put `sync_beta` back inside `retarget` only; the test fails on
  the loop-1 request.
- `a_model_without_per_message_effort_gets_no_message_from_another_models_record`: a session whose earlier answer
  recorded `low` under Sonnet 5.5, then a turn on Haiku 5.5: no request carries an effort-only message or the beta.
  Plant: make `place` skip its `takes` check; the test fails.
- `a_refusal_mark_lapses_after_an_hour` (a unit test in `effort_step.rs`): a model marked at `t` is in `all(t +
  REFUSAL_HOLDS_MS - 1)` and not in `all(t + REFUSAL_HOLDS_MS)`; a second model's mark is its own. Plant: ignore the
  time (a mark lasts forever); the test fails. The existing rig test
  `a_400_on_the_messages_asks_again_without_them_and_remembers` (near :196) still shows the next turn, within the
  hour, carries no message; update only its doc comment's "for the rest of the daemon's life".
- The existing effort tests (`tests_turn_effort.rs`, `tests_effort_sticky.rs`, `compiler/tests.rs`'s effort cases,
  `tests_route_effort.rs`) pass unchanged. Run `cargo nextest run -p theseus-core effort` and
  `cargo nextest run -p theseus-core first_request` before your last commit; the first-request golden
  (`own_bench/first_request.golden.json`) must not change (a first request with no message has no beta, as today).

## Not in this row
- Which models take per-message effort (the catalog's rows): no row changes here. A live probe of a loop-1 request
  is the maintainer's (below).
- Routing's effort decisions (`routing/effort.rs`, `turn/route_step.rs`, `turn/checkpoint_step.rs`) and the sticky
  rule (`stick` in `effort_step.rs`: leave it as it is).
- `turn.rs`: no edit. If the clock or the spec needs a new field from `turn.rs`, stop and write it in the report.
- The owner's daemon: reading its ledger is the maintainer's (below).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The fix ships on with no switch; the rollback is reverting the commit. The
  one-hour hold is a constant, not a config key.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). FAST: nothing new on a plain turn (a
  plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`), and nothing measurable on the
  path before a request leaves (one pass over the messages that `sync_beta` already makes is fine).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert, and on the unchanged code before the fix.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless you explain each finding.
- Line ceilings: `scripts/long-files.txt`. `crates/theseus-core/src/turn.rs` is 3,405 / 3,525 and is not yours to
  touch. Never raise a ceiling.
- No new dependency. No store format change is expected (the store format on your base is 48); if you find you must
  store something, bump to 49, add the old layout's sample to `crates/theseus-core/src/tests_layouts.rs`, and say why.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
- Read `crates/theseus-core/AGENTS.md` first (its effort and turn paragraphs). The cloud VM has no provider key: every
  test here runs on the scripted provider. Keep CLOUD_REPORT.md honest about that.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: the root as the red test showed it (the failing assertion before the fix); what changed;
each test and its plant; the keel guard's output; the store format (unchanged, or the bump and why); follow-ups. Give
the maintainer two checks: (1) read-only on a daemon with the fix, `theseus ledger -k provider.error -n 50` shows no
new `output_config` 400 after a Sonnet 5.5 turn at a Jev-chosen level that makes tool calls; (2) one live request pair
on Sonnet 5.5 (a first request that places `low`, then the tool-loop request with the message and the beta) answers
200 and reads the cache. Push, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
