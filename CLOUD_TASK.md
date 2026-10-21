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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-threads-home-claim`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: threads-home-claim, a turn that ran at home but served one thread becomes that thread's memory (theseus-98bz.6)

Branch: `cloud/20261021-threads-home-claim`. Every commit's subject carries `theseus-98bz.6`. Deadline for the report:
5 hours after you start.

**Why.** Activity threads split one private place into separable streams of work, each with its own context, and a
thread's recall is kept to its own stream by the thread lens. Work done for a thread at the place's home is not the
thread's: when the router asks "which thread?" and nobody answers (or the answer is `home`), or the router places a
message at home, the turn runs in the home session and its nodes belong to no thread. The lens then treats them as
the place's own conversation, distant for every thread (`base_distance`). In the synthetic threadbench (plan T12,
seed 11, k=3) the PR-round stream's second review comment ran at home after an ask, and the PR-round thread later
wrote "Comment 2: I didn't write that change." in 6 of 6 runs: the work existed, but its own thread never recalled
it. A person sees the same thing whenever a message lands at home: the right thread later disowns the work.

**Risk: medium.** This row writes node ownership (`moved.of.<node>`) at a home turn's end. A wrong claim would let
one stream's work into another stream's recall, which is what threads exist to prevent, so a claim is made only when
the turn's files fall in exactly one live, unsealed thread's working set, it can be undone, and a switch turns it off
(byte for byte today's behaviour).

**Prerequisite: the threads lane (`integ/threads`), not `main`.** See "Your base".

## Your base

Your branch starts from the threads integration branch `integ/threads` at `6d4e0dd9` or later, not from `main`. It
holds `main` at `88f2e48f` plus quick-vocab, detours-why, threads-cross (C10), threads-routing (C9) and
threads-links. **This section wins over the preamble above** wherever they differ about your base, its store format or
its line ceilings. Names below are as they are on `integ/threads`; if one moved, search by name.

- **Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row writes
  existing value types (`moved::MovedTo` under `moved.of.<node>`) and a claim record under a new META key, as a split
  writes `moved::split_key`. Read AGENTS.md's store version rule: if a new META key family counts as a new record
  kind, bump to 49 with the rule's sample and say why in the report (another lane also holds 48; the maintainer
  renumbers at the release join). Otherwise leave it at 48.
- **Line ceilings** (`scripts/long-files.txt`): `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 and
  `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733: add no line to either. Put new code in new files.
- **What exists that you build on:**
  - `crates/theseus-core/src/moved.rs`: `moved_of_key(node)` = `moved.of.<node>`, `MovedTo { thread, split }`,
    `split_key(id)`. The lens reads that key: `thread_recall.rs` `ThreadScene::moved_to(node)` gives the node its
    thread, which outranks its session's, so a node keyed to thread T passes for T's recall and is another thread's
    (distant, or held if T is sealed) for every other. The home's compile reads `Body::Moved` nodes, not the key, so
    writing only the key leaves the home's compile alone: keep it so. `rpc/tests_thread_mix.rs` (near :192) seeds the
    key directly in a test: the pattern for yours.
  - `rpc/thread_flow.rs` `thread_routed_turn(r: &Routing, res)` runs after every routed turn (`turn_submit` in
    `rpc/methods.rs` calls it after `runner.run`). `r.slug == None` means the turn ran at home (asked and answered
    `home`, an ask that timed out, the router's home pick, a spoken turn); `r.home` is the home session; `res.turn_id`
    the turn.
  - The turn's files: `TurnSubmitResult.files` (`theseus_protocol::files_left::FilesLeft`): `changed` (the files the
    turn's file tools wrote) and `left` (new files its commands left beside them), each a `PathList` of at most 8
    paths with `more` counting the rest, scrubbed, and none in a shared place (threads live in private places only).
  - Thread working sets: `ThreadInfo.working_set` (`roots`, `files`, ...), filled by the owner (`thread.edit`), an
    import or a split today. `toolrun/misroute.rs` `normal` and `fold` expand `~` and fold `.`/`..` without a
    filesystem read: reuse them (make them `pub(crate)`).
  - Readers of `moved.of.*` and `MovedTo.split`: `rpc/methods.rs` `marked_moved` (a node's detail gets `moved_to` and
    `split`; the CLI's `crates/theseus/src/render/history.rs` near :41 prints it; `cockpit/src/lib/progressLine.ts`
    reads a work view's `moved_to`, a different field), `rpc/thread_home.rs` `refuse_moved` (a split refuses a node a
    split already moved) and the unsplit near :1025 (writes null for its own nodes), `turn/recall_step.rs` near :529
    (credits a recalled node to its thread: right for a claim too), `thread_recall.rs` `moved_to` (**do not edit**).

## Overlap
A parallel row from the same base, `threads-routing-rest` (theseus-98bz.5 working sets grow from a thread's own turns,
98bz.3 the bench clock, 98bz.7 a lone thread offers new), touches nearby code. Keep your hunks apart:
- `rpc/thread_flow.rs` `thread_routed_turn`: that row changes the last line (`state.replied(..)`) and the `Routing`
  struct. You add **one call, on the line right after `let Some(res) = res else { return };`**, and change nothing
  else in that file (not `Routing`, not `route_to_thread`).
- `rpc/methods.rs` `turn_submit`: that row adds its growth call after `runner.run`. You add nothing there (except, if
  needed, a claim-aware branch inside `marked_moved`, which that row does not touch).
- `thread_split.rs` (`working_set_of`) and `rpc/inspect.rs` (`turn_nodes`): that row may change or reuse them. **Do
  not edit either file.** Read the turn's paths from `res.files` and page the turn's nodes yourself (below).
- `config/threads.rs` and the template: that row adds keys after `ask_every_thread` and a config test at the end of
  the tests module. Yours: `claim_home_turns` **after `link_proposals`** (the struct's last field, its `Default`
  entry and the template line after `# link_proposals`), and your config test in your own test file.
- `rpc/mod.rs`: that row adds `thread_grow` and `tests_thread_grow`; you add `mod home_claim;` and `mod
  tests_home_claim;` (with `#[cfg(test)]`) in their alphabetical places.
- `crates/theseus-protocol/src/lib.rs`: you add `THREAD_UNCLAIM = "thread.unclaim"` at the end of the existing
  `THREAD_*` method line (no new line). Both rows regenerate `cockpit/src/protocol.gen/`; the integrator regenerates at
  the join.
- `bench/`, `thread_route.rs`, `threads_import/`, `submit.rs`: that row's. You touch none.

## Your row (names as on `integ/threads`)
1. **When.** In `thread_routed_turn`, for a turn that ran at home (`r.slug.is_none()`), with
   `[threads] claim_home_turns` on: call `self.claim_home_turn(r, res)` (new file `rpc/home_claim.rs`). It returns at
   once, before any store read, when `res.files` is empty or absent, when `more > 0` on either list (unseen paths
   could belong to another thread: say so in the report), or when the place has no live, unsealed thread. A turn in a
   thread, or in a place that is not routed, is never claimed.
2. **Which thread (a pure function in `home_claim.rs`, unit-tested).** Each path of `changed` and `left` is matched
   against every live thread whose seal is `None` and `seal_pending` is `None`: a path is a thread's when it equals one
   of its `files`, or lies under one of its `roots` (an absolute or `~/` root, normalised with `normal`, is a path
   prefix by components; a bare root name such as `tarnlib` matches the path's first component relative to the home
   session's directory). Paths that match no thread are ignored. Claim only when at least one path matches and every
   matching path matches the same single thread; a path that matches two threads, or paths matching two different
   threads, claim nothing.
3. **What is claimed.** The turn's own nodes in the home session (its message, the assistant's, its tool calls and
   results): a bounded page of the home session's newest nodes read backwards (`Page { before, .. }`, as
   `rpc/threads.rs` pages backwards near :450), kept while `turn_id` equals `res.turn_id`, stopping at the first older
   node. Never read the whole transcript. A node whose `moved.of.<node>` already holds a value (a split moved it) is
   left as it is.
4. **The write, one batch:** for each node, `moved_of_key(node)` = `MovedTo { thread, split: "claim_<id>" }` (the id
   from `crate::new_id`, so `claim_` marks a claim for every reader); a META record of the claim under a new key
   (for example `thread.claim.<id>`, beside `moved::split_key`) naming the thread, the home session, the turn and the
   nodes, for the undo; and a `thread.claimed` ledger row. Add the kinds `ThreadClaimed = "thread.claimed"` and
   `ThreadUnclaimed = "thread.unclaimed"` after `ThreadUnmerged` in `crates/theseus-protocol/src/ledger.rs`, with
   their facts beside `ThreadUnsplit`'s in `crates/theseus-core/src/fact/thread.rs` and their lines in
   `fact/mod.rs`. The row names the claim, thread, session, turn, node count and the paths that matched (scrubbed:
   they come from `res.files`, which already is). Nothing is posted to a place.
5. **The undo:** `theseus thread unclaim ID` (`crates/theseus/src/thread.rs`, beside `Unsplit`) calls the RPC
   `thread.unclaim` (params type in `crates/theseus-protocol/src/threads.rs`, its name on the `THREAD_*` line, routed in
   `rpc/threads.rs` `rpc_threads` beside the joins). It writes `moved.of.<node>` back to null **only where the key
   still holds this claim's `MovedTo`** (a later split's key is kept), marks the claim record undone, and writes a
   `thread.unclaimed` row. An unknown or already undone claim is refused with a message naming `thread.unclaim`'s
   argument.
6. **Readers.** `marked_moved`: a `claim_` value gives the node's detail `claimed_by` (the thread) and `claim` (the
   id), never `moved_to`, and the CLI's history prints "claimed by «thread»" (render/history.rs). `refuse_moved`: a
   split may take a claimed node (the claim is weaker than a move): the split's `MovedTo` replaces the claim's, and
   the claim's undo then leaves it. Check every other reader you find (`git grep moved_of_key`, `MovedTo`, `"moved."`)
   so a claim is never taken for a split.
7. **Switch:** `[threads] claim_home_turns = true` (`crates/theseus-core/src/config/threads.rs` and the template
   `crates/theseus-core/config/theseus.example.toml`). Off: no key, no record, no row.

Jev's post-turn project answer as a second trigger (plan row 7) is a follow-up: name it in the report.

## Tests (each with a planted revert; the first is the key test)
- **`a_home_turn_that_served_a_thread_is_claimed`** (whole core, new `rpc/tests_home_claim.rs`, with the scripted
  provider `rpc/tests_thread_home.rs` uses and a fake Jev as `rpc/tests_thread_route.rs` has): a private place with
  two threads, `pr-round` (root `tarnlib`) and `sec-fix` (root `secsvc`); a message the router runs at home (an ask
  answered `home`); the scripted model writes `tarnlib/tarn/retry.py`. After the turn every node of that turn is keyed
  to `pr-round` with a `claim_` split, one `thread.claimed` row names it, and the lens (`ThreadScene::lens`, as
  `tests_thread_mix.rs` builds it) passes the turn's note for `pr-round`'s recall and finds it another thread's for
  `sec-fix`'s. Plant: never claim. This test must be shown failing on that plant.
- A home turn writing nothing threaded (`/tmp/scratch.txt`) is not claimed; a turn writing one file under each
  thread's root is not claimed; a sealed thread never claims; a node a split already moved keeps its split's value;
  `more > 0` claims nothing.
- The undo: `thread.unclaim` restores null on every claimed node and the lens's verdict for `pr-round` is the one
  before the claim (`thread_base`); a node a split took after the claim keeps the split's value; a second undo is
  refused.
- Readers: a claimed node's history detail says `claimed_by`, not `moved_to`; a split over a claimed node succeeds.
- With `claim_home_turns = false`: no key, no record, no row.
- `rpc/tests_thread_mix.rs` (the contamination set) and the threads-links tests pass unchanged.

## Not in this row
- Growing working sets (98bz.5), the bench clock (98bz.3) and a lone thread's `new` offer (98bz.7): the parallel row.
  Until 98bz.5 joins, working sets fill only by the owner, an import or a split, so claims are rarer in practice than
  after it: that is expected; say it in the report.
- A turn that only read a thread's files (no write) is not claimed: a follow-up if wanted.
- The threadbench run: **bench measurement is run by the maintainer after the join, never by this session: the VM has
  no API keys.**
- `docs/`, the spec, `docs/status.md`, the README: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. `claim_home_turns` is the rollback: a
  test shows that with it off the old behaviour is byte-for-byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget` passes unchanged, and the
  gate's turn bench must not move); a place with no live thread does no work; nothing on the start path grows with
  history; the claim runs after the turn's reply has streamed, with its in-memory checks first (say in the report how
  long a claim takes on a test turn).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` (your base; if that ref is not
  in your clone, use the commit your branch started from, `6d4e0dd9` or later) and paste its output in the report. It
  must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as "Your base" says.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented (the bench's
  `tarnlib`, `secsvc`, `homesvc`, `PRR-4142` are fine).
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Add AGENTS.md sentences as their own lines, never by
  rewording another row's. Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run on the VM
  (about 33 L1 tests fail here; none is yours), and the exact commands for the maintainer.

## Your report
Commit `CLOUD_REPORT.md` at the repository root, written with your editor tools: what changed; each test and each
plant with the test that failed; what could not run on the VM and the exact commands for the maintainer (a live check:
in a two-thread place, a message answered `home` that edits a file under one thread's root, then `theseus history` on
the home session shows "claimed by", that thread's next turn recalls it, and `theseus thread unclaim ID` undoes it);
the keel guard's output; the store format (unchanged at 48, or why it moved); and the follow-ups (Jev's project answer
as a trigger, read-only turns, docs). Push the branch, check `CLOUD_REPORT.md` is not empty in the pushed commit, and
end with a final message that starts `CLOUD REPORT COMPLETE`.
