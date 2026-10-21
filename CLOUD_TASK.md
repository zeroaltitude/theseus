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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-threads-routing-rest`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**Store format on your base (integ/threads, not main): 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
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
## Your task: threads-routing-rest, a thread's working set grows from its own turns, the bench routes at real gaps again, and a lone thread offers a new one (theseus-98bz.5, theseus-98bz.3, theseus-98bz.7)

Branch: `cloud/20261021-threads-routing-rest`. Every commit's subject carries the `theseus-<id>` of the row it
implements (each row below names its id). Deadline for the report: 5 hours after you start.

**Why.** Activity threads split one private place into separable streams of work, each with its own context. An
earlier routing row (C9) made stickiness alone unable to overrule Jev, made a thread's drop name its rule, and made an
ask list every thread; it ran out of time before the rest. What a person still hits on your base:
- **A thread's working set never fills.** A thread starts with an empty working set and nothing grows it: in two
  synthetic threadbench runs (plan T12, seed 11, k=3), 0 of 547 pre-router entries had an entity hit. So `entity_hits`
  never fires, the lens's naming waiver never applies, Jev's thread inputs carry no entities, and C9's new rule that
  offers `new` when a message names two entities "no live thread holds" counts every entity as new. A PR-round thread
  that edited `tarnlib/reviews/comment-1.md` for "PRR-4142" still scores 0 on "comment-2.md under tarnlib/reviews".
- **The threadbench no longer measures routing at all.** Since `0c52c114` (theseus-0rfjx, on your base), a session
  named with `theseus ask -s` is the owner's explicit choice and is never routed: `crates/theseus-core/src/rpc/
  thread_flow.rs` `may_route` returns false for `p.via == Some("ask")`, and the CLI's `ask` always sends
  `via: "ask"` (`crates/theseus/src/cmd.rs`, near :102). The bench's A1 and A1o arms send every message as
  `theseus ask -s <home> -` (`bench/threads/th_threads.py` `drive`, `target = ["-s", home]`), so every message runs
  at home and no `thread.routed` row is written. On top of that, the bench always routed at zero gap (the plan's gaps:
  median 6.2 minutes, 55% over 5), so stickiness plus the reply chain was 0.88 on every message; at a 23-minute gap
  stickiness alone is about 0.12 (`sticky`: 0.6 x 0.5^(23/10) with the default `sticky_half_life_secs = 600`).
- **A place with one thread never offers a new one.** With one live thread, `decide_single` routes to it or runs the
  message at home, with no Jev wait and no ask, so a new stream's first message ("homesvc maintenance: branch
  chore/bump-1-8, ticket OPS-4214 ...") lands at home with no thread offered.

**Risk: medium.** Row A writes to threads' records at a turn's end; row B adds a test-only clock to the wire and lets a
bench submit be routed; row C adds an ask where today a lone-thread place runs at home. Each behaviour has its own
switch, and each switch off restores today's behaviour byte for byte. Isolation between streams is the reason threads
exist: nothing in this row may let one stream's facts reach another stream's recall that does not reach it today.

**Prerequisite: the threads lane (`integ/threads`), not `main`.** See "Your base".

## Your base

Your branch starts from the threads integration branch `integ/threads` at `6d4e0dd9` or later, not from `main`. It
holds `main` at `88f2e48f` plus five rows: quick-vocab, detours-why, threads-cross (C10: the place digest, Jev's
near-miss judge, `own_top`), threads-routing (C9: `Reason::Thread(Cause)`, `hold_sticky`, `new_entities`,
`ask_options(.., every)`, `Choice::Other`, `Pending.slugs`, `ThreadAsk.others`) and threads-links (`ThreadInfo.links`,
`theseus thread link`). **This section wins over the preamble above** wherever they differ about your base, its store
format or its line ceilings. Names below are as they are on `integ/threads`; if one moved, search by name.

- **Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`; threads-links
  took 48). No row here should need a bump: row A writes an existing field (`ThreadInfo.working_set`), row B adds a
  wire parameter only, row C stores nothing. If you find you must store a new field, bump to 49, add the old layout's
  sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and say why in the report: another lane also
  holds 48 and the maintainer renumbers at the release join.
- **Line ceilings on your base** (`scripts/long-files.txt`): `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500
  (add no line), `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (add no line), `crates/theseus-core/src/turn.rs`
  3,414 / 3,525 (you need no line there). Unlisted files may reach 2,500: `rpc/methods.rs` is 1,925,
  `thread_route.rs` 1,734, `rpc/thread_flow.rs` 768. Put new code in new files.
- **What C9 left for you** (its report named these call sites): `route_to_thread` takes `now` from
  `theseus_protocol::now_unix_ms()`; `thread_routed_turn` (`rpc/thread_flow.rs`) runs after the turn and records
  `state.replied` at the real clock; `turn_submit` (`rpc/methods.rs`, near :911) calls it once after
  `self.runner.run(..)`; `rewind::written(execution_id)` is dropped when the turn's `end_note_step::Held` drops,
  before `runner.run` returns, so it is empty by then.

## Order and size
Three rows. **In this order: A, then B, then C.** Commit each row separately (its own commits, its own id in the
subject), with its tests and plants green, before you start the next. A row you do not reach is named in the report as
not started; a row half done is reverted from the branch, never committed half done. Plan row 7 (a home turn that
served a thread is claimed by it, theseus-98bz.6) is **not in this row**: it stays open as its own follow-up.

## Your rows (names as on `integ/threads`)

### Row A: a thread's working set grows from its own turns (theseus-98bz.5)
- **What already exists.** `crates/theseus-core/src/thread_split.rs` has a private
  `working_set_of(nodes, idx, texts)` that reads tool-call nodes (`Body::ToolCall { input, .. }`: keys `path`,
  `paths`, `file`, `files` give files; `cwd`, `dir` give roots; `url` gives a host) and `#123` refs from texts. Reuse
  it: make it `pub(crate)` or move it into your new module and call it from the split (a pure move needs no plant).
  `crates/theseus-core/src/threads_import/extract.rs` `working_set(texts)` reads a message's entities (C9 taught it
  tracker tickets like `OPS-4214`). `crates/theseus-core/src/thread_join.rs` has `MAX_SET` (64) and `union_set`.
- **A pure part:** a new module `crates/theseus-core/src/thread_grow.rs` (one `mod` line in `lib.rs`) with
  `grow(current, touched, named, held_elsewhere) -> WorkingSet`. A relative path, or one under the session's directory
  made relative to it, gives its file and its first component as a root (`tarnlib/reviews/comment-1.md`: root
  `tarnlib`, file `tarnlib/reviews/comment-1.md`). An entity any other live thread holds (sealed ones included; compare
  case-insensitively, as `thread_route::new_entities` does) is not added; the other thread keeps it (no removal here).
  A list at `MAX_SET` takes nothing more (eviction is a follow-up: say so).
- **The call:** one method (in a new `crates/theseus-core/src/rpc/thread_grow.rs`, or beside `thread_routed_turn`)
  called once in `turn_submit` after `runner.run`, next to the `thread_routed_turn` call. The thread is
  `routing.slug` when the router placed the turn, else the thread whose session ran it (`Board::of_session`), which
  covers `ask --thread`, a Discord thread-bound turn and a thread's own session. Capture the run session's id before
  `session` moves into `TurnRequest`. A home turn grows nothing (and a place with no live thread does no work: check
  the in-memory board first, before any store read).
- **Touched:** the turn's own tool-call nodes, read as a bounded page of the session's nodes after the position the
  turn started from, filtered by `turn_id`, as `rpc/inspect.rs` `turn_nodes` does. Never read the whole transcript.
  **Named:** the user message's entities through `extract::working_set`.
- **Write** only when the set changed, through `self.runner.threads.write(&self.store, ..)` as `thread_edit`
  (`rpc/threads.rs`, near :483) does after its checks, with a `thread.edited` row whose `changed` names
  `working_set` and whose author is the harness. Do not call `thread_edit` itself (it judges the owner's permission).
  Check `crates/theseus-discord/src/runtime/guild_thread.rs` `thread_changed`: a change of the working set alone must
  post nothing to a place and rename nothing; test it if it would.
- **Switch:** `[threads] working_set_grows = true` (`crates/theseus-core/src/config/threads.rs` and the template
  `crates/theseus-core/config/theseus.example.toml`, right after `ask_every_thread`).
- Do not touch `turn/end_note_step.rs`, the turn-end facts code or `thread_recall.rs`.

### Row B: the bench routes again, at the plan's real gaps (theseus-98bz.3)
- `crates/theseus-protocol/src/submit.rs` `TurnSubmitParams` gains `route_at_ms: Option<u64>`
  (`#[serde(default, skip_serializing_if = "Option::is_none")]`, `ts(optional)`): the bench's clock, milliseconds.
  Literals that list every field gain `route_at_ms: None`; `crates/theseus-discord/src/runtime.rs` uses
  `..Default::default()` and gains no line.
- The daemon honours it only when `[threads] bench_clock = true` (default **false**: a test hook, off in every real
  config). With the key off, a submit that carries it is refused as invalid params naming `[threads] bench_clock`.
- With the key on, a submit carrying `route_at_ms` stands for a message typed in its session's place: `may_route`
  places it even with `via: "ask"`. Without `route_at_ms`, `ask -s` stays never routed
  (`tests_thread_route.rs` `a_session_named_with_ask_s_is_never_placed_in_a_thread` passes unchanged).
- `route_to_thread` uses it as `now` for `pre_route`, `state.arrived` and `Routed.at_ms`; `Routing` carries the bench
  clock and an `Instant`, and `thread_routed_turn` records `state.replied` at that clock plus the turn's real
  duration. Nothing else reads the bench clock.
- The CLI: `theseus ask --route-at-ms N`, a hidden flag (`AskArgs`, `crates/theseus/src/main.rs`, sent from
  `crates/theseus/src/cmd.rs`).
- The bench: `bench/threads/th_threads.py` passes each message's plan time (`m.t`, minutes since the run's first
  message, `th_format.py` `Msg`) as milliseconds from a fixed base, and `arm_config` sets `[threads] bench_clock =
  true` for every threads arm. Update `bench/threads/README.md`'s line on gaps. Do not touch `th_oracle.py`,
  `th_score.py` or `report.py`.

### Row C: a lone thread offers a new one (theseus-98bz.7)
- `thread_route.rs` `decide_single` (near :907) and its tests (near :1646) stay as they are. A new pure function
  beside it (for example `lone_offer(threads, pre, new_entities, new_title) -> Option<Decision>`) gives, with
  `[threads] single_offers_new = true` (after `working_set_grows`), an `Ask` offering the thread, `new` (with
  `propose`'s title) and `home` when the thread's pre score is below `CONFIRM` and the message names two or more
  entities no working set holds (`new_entities`, computed in the caller as C9's `Rules.new_entities` is); otherwise
  `None`, and `decide_single` decides as today.
- `rpc/thread_flow.rs` `place_unaddressed`: the `live.len() < 2` branch (near :322) sends that `Ask` to
  `ask_thread` as the other branch does, and a spoken turn runs at home at once, as there. No Jev wait is added.
- A place with no live thread is not routed at all today (a plain turn starts no timer): leave that as it is.

## What already exists (do not redo)
C9's rows on your base (`hold_sticky`, `Rules`, `new_entities`, `ask_options(.., every)`, `Choice::Other`, the drop
causes) and C10's and threads-links' work. Tests that must pass unchanged: `thread_route.rs`
`the_decision_table_by_band` and `the_decision_table_is_the_same_with_the_sticky_rule_on`, the fixture
`crates/theseus-core/fixtures/thread_route_40.json` (43 messages) with
`forty_unaddressed_messages_find_their_thread_and_ask_rarely`, `rpc/tests_thread_mix.rs` (the contamination set),
and the 0rfjx test above. If a fixture case moves because a lone thread now asks, say which and why.

## Tests (each with a planted revert; the first is the key test)
- **`a_threads_turn_grows_its_working_set`** (row A; a whole-core test in a new `rpc/tests_thread_grow.rs`, with
  the scripted provider `rpc/tests_thread_home.rs` uses): a thread `pr-round` with an empty working set; a turn sent
  with `thread: Some("pr-round")` whose scripted model writes `tarnlib/reviews/comment-1.md`, the message naming
  "PRR-4142"; after it the thread holds root `tarnlib`, that file and ref `PRR-4142`, and `pre_route("comment-2.md
  under tarnlib/reviews", ..)` gives `pr-round` an entity hit. Plant: skip the call. This test must be shown failing
  on that plant.
- Row A also: `grow` unit tests (an entity another live thread holds is not added, a sealed thread's included; a full
  list takes nothing; an unchanged set writes no `thread.edited` row); a home turn grows no thread; with
  `working_set_grows = false` no row is written and the set is byte-for-byte as before.
- Row B: with `bench_clock` off, a submit carrying `route_at_ms` is refused naming the key; with it on, two messages
  23 bench-minutes apart sent with `via: "ask"` are both routed, and the second's `thread.routed` row records a pre
  score below 0.15 for the sticky thread; `test_th_threads.py` checks the ask command carries `--route-at-ms` with the
  plan time and the arm config sets the key. Plant: ignore `route_at_ms` (use the real clock).
- Row C: one live thread holding root `tarnlib`; "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214" gives
  an `Ask` with the thread, `new` and `home` (pure, and once through the whole core: a `thread.asked` offering
  `new`); "retry the flaky test in tarnlib" routes as today; a spoken turn runs at home; with `single_offers_new =
  false` the decision is today's. Plant: drop the offer.

## Not in this row
- Plan row 7 (theseus-98bz.6, a home turn that served a thread is claimed). Name it in the report as left.
- The threadbench run. **Bench measurement (threadbench, any arm, any k) is run by the maintainer after the join,
  never by this session: the VM has no API keys.** Python unit tests under `bench/threads/` are fine to run.
- Narrowing C9's entity-based `new` offer to the asks its cap made (a separate follow-up), the Discord select's
  gateway test, eviction from a full working set.
- `docs/`, the spec, `docs/status.md`, the README: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** Rows A and C ship on by default; row B's `bench_clock` is a test hook and
  ships off. Each row's switch is its rollback: a test shows that with it off the old behaviour is byte-for-byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget` passes unchanged, and the
  gate's turn bench must not move); a place with no live thread does no routing work and no growth work; nothing on
  the start path grows with history; row A's work runs after the turn's reply has streamed (say in the report how long
  it takes on a test turn).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` (your base; if that ref is not
  in your clone, use the commit your branch started from, `6d4e0dd9` or later) and paste its output in the report. It
  must be empty. Updating an existing test's expected decision (a lone-thread case that now asks) is a behaviour
  change: say which assertions changed and why.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as "Your base" says.
- **Shared files, kept mergeable.** In `config/threads.rs` and the template put your keys after `ask_every_thread`, in
  the order `working_set_grows`, `bench_clock`, `single_offers_new`, and your config test as a new function at the
  end of the tests module. Add no line to `crates/theseus-protocol/src/lib.rs`. Regenerate `cockpit/src/protocol.gen/`
  with the protocol crate's test after row B (`TurnSubmitParams` gains a field); the maintainer regenerates again at
  the join. Add AGENTS.md sentences as their own lines, never by rewording another row's.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented (the bench's
  `tarnlib`, `datatidy`, `homesvc`, `PRR-4142`, `OPS-4214` are fine).
- Read the AGENTS.md/CLAUDE.md of every crate you touch first, and `bench/threads/README.md`. Keep CLOUD_REPORT.md
  honest: what you ran, what passed, what you could not run on the VM (about 33 L1 tests fail here; none is yours),
  and the exact commands for the maintainer.

## Your report
Commit `CLOUD_REPORT.md` at the repository root, written with your editor tools: what changed, per row (done, not
started, or reverted); each test and each plant with the test that failed; what could not run on the VM and the exact
commands for the maintainer; the keel guard's output; the store format (unchanged at 48, or why it moved); and the
follow-ups (row 7, eviction, the docs), with the threadbench targets for the maintainer: A1 writes a `thread.routed`
row for every unaddressed message again; entity hits on 30% or more of pre-router rows; the home-server stream gets a
thread in 3 of 3 A1 runs; top-1 on clear messages 92% or more; asked-on-clear 15% or less; contamination not above
its bar. Push the branch, check `CLOUD_REPORT.md` is not empty in the pushed commit, and end with a final message
that starts `CLOUD REPORT COMPLETE`.
