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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-threads-routing`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: threads-routing, a message reaches the thread it belongs to, and the thread keeps the work it did (theseus-eqp1v, theseus-98bz.2, theseus-98bz.3, theseus-98bz.4, theseus-98bz.5, theseus-98bz.6)

Branch: `cloud/20261021-threads-routing`. Every commit's subject carries the `theseus-<id>` of the plan row it
implements (each row below names its id). Deadline for the report: 6 hours after you start.

**Why.** Activity threads split one private place into separable streams of work (a PR round, a security fix, a data
pipeline, a home-server chore), each with its own context. The synthetic threadbench (plan T12, seed 11, k=3, on the
build that added the thread lens) shows where a person's message ends up in the wrong place, and once it is in the
wrong place the right thread never sees it:
- **Stickiness alone overrules Jev.** A new stream's first message ("homesvc maintenance: branch chore/bump-1-8,
  ticket OPS-4214 ...") scores the previous thread 0.88 from stickiness and the reply chain only (exactly
  `noisy_or(STICKY 0.6, CHAIN 0.7)` at zero gap, no entity hit). Jev picks another thread, or `new_thread` at p 0.52.
  Fused, the sticky thread keeps 0.88 x `OTHERS` 0.85 = 0.748, at or above `CONFIRM` 0.60, so `decide` routes it
  `visible` with no question. That stream got no thread in 6 of 6 runs and is 84% of all misroutes (top-1 78%).
- **An ask never offers the right thread.** "Security view: second comment is in, comment-2.md. Do that one too."
  belongs to the PR-round thread. The router asked, but `ask_options` lists only the two best-ranked threads, which
  were the security fix and the CLI flag: the right thread was not an option. The owner answered `home`, the work ran
  at home, and later the PR thread wrote "Comment 2: I didn't write that change." Same for "The cutoff for flagging
  changed: use 0.55 from now on." (6 of 6 runs), after which the pipeline thread answered "you haven't given me the
  new cutoff" in 6 of 6.
- **Working sets never fill.** 0 of 547 pre-router entries in two runs had an entity hit: threads start with an empty
  working set and nothing grows it, so `entity_hits`, the lens's naming waiver and Jev's thread inputs have no
  entities.
- **The drop reason says only `thread`**, whichever of the lens's four rules dropped the note.
- **The bench routes every message at zero gap** (the plan's gaps: median 5.1 min, p90 41 min), so stickiness plus
  chain is 0.88 on every message; at a 23-minute gap stickiness alone would be about 0.12.

**Risk: medium.** Rows 4 and 5 change what the router does with a sticky-only message and what an ask offers; rows 6
and 7 write to threads' records and to node ownership at a turn's end. Each behaviour has its own switch, and each
switch off restores today's behaviour byte for byte. Isolation between streams is the reason threads exist: nothing in
this row may let one stream's facts reach another stream's recall that does not reach it today, except a home turn's
nodes that row 7 claims for exactly one thread.

**Prerequisite: `main`.**

## Order and size
Six plan rows is a lot for one session. **Must do, in this order: rows 2, 4, 5.** **Stretch, in this order, each
only after the one before is committed: rows 3, 6, 7.** Then, only if time is left, row 12. Commit each row
separately (its own commit or commits, its own id in the subject), with its tests and plants green before you start
the next. A row you do not reach is named in the report as not started; a row half done is reverted from the branch,
never committed half done.

## Your rows (names as on `origin/main`)

### Row 2: say why the lens drops a candidate (theseus-98bz.2)
- `crates/theseus-memory/src/threads.rs` `ThreadLens::judge` returns `Verdict::Distant` for four different rules.
  Give it a cause: `bar` (share at or under `bar`), `own` (`beats_own` and `paid <= own_top`), `entity`
  (`needs_entity` and `Tie::Apart`), `base` (no thread claims the node: the place's own conversation under
  `base_distance`; base wins over the other three). Carry `share` and `paid` with it.
- `crates/theseus-memory/src/recall.rs`: `Reason::Thread` (near :86, `as_str` near :122, the lens arm near :399)
  becomes one variant with the cause, `Reason::Thread(ThreadCause)`, whose names are `thread_bar`, `thread_own`,
  `thread_entity`, `thread_base`. Keep it one variant: another row matches thread drops by that variant.
- `crates/theseus-protocol/src/memory.rs` `RecallDrop` gains `own_top`, `share`, `paid` as optional fields
  (`skip_serializing_if = "Option::is_none"`, `ts(optional)`), set only on a thread drop. Regenerate
  `cockpit/src/protocol.gen/` with the protocol crate's test, and check whether any stored record carries
  `RecallDrop` (search `crates/theseus-core/src/tests_layouts.rs`); if one does, follow the store-format rule above.
- Search `cockpit/`, `bench/` and `crates/` for readers of the drop reason `"thread"` and update each.

### Row 4: stickiness alone cannot overrule Jev (theseus-eqp1v)
- `crates/theseus-core/src/thread_route.rs` `fuse` (near :565) and `decide` (near :730). When the top choice is a
  thread whose `Pre` has no entity hit and was not raised by a learned correction (`learned_thread` in
  `crates/theseus-core/src/rpc/thread_flow.rs` lifts a score to `LEARNED`; mark that on the `Pre`), and Jev answered
  (`Voice::Jev`) with a pick other than that thread, cap that thread's fused score just below `CONFIRM`, so the
  decision falls to the ask.
- In that ask, offer `new` when Jev raised `new` (as today) **or** the message names two or more concrete entities
  that no live thread's working set holds (a branch, a ticket or PR/issue id, a host, a file). Reuse the extractor
  `crates/theseus-core/src/threads_import/extract.rs` `working_set` over the message; compute the count in the caller
  and pass it to the pure `decide` (a small struct argument is fine; keep `decide` pure).
- `Decision::OfferNew` for Jev `new` at or above `CONFIRM`, `decisive`, the `Voice::Alone` branch and
  `decide_single` stay as they are.
- Switch: `[threads] sticky_alone_asks = true` (`crates/theseus-core/src/config/threads.rs`, the template
  `crates/theseus-core/config/theseus.example.toml`). Off gives today's decision table.

### Row 5: an ask always offers the right thread (theseus-98bz.4)
- `thread_route.rs` `ask_options` (near :640): list every live, unsealed thread when there are four or fewer, ranked
  by fused score; above four, the top three plus an `other` option. Then `new` (when raised) and `home` as today.
  `question()` (near :706) stays one line and says "or another thread" when not every thread is listed.
- `crates/theseus-core/src/thread_flow.rs` `answer` (near :205): besides the option keys, accept any live thread's
  slug as the answer (the pending ask keeps the live slugs it was asked over); an unknown word still errors with the
  keys and says a thread's slug is accepted.
- Surfaces: the CLI's ask line (`crates/theseus/src/thread/ask.rs`), the Discord card
  (`crates/theseus-discord/src/runtime/thread_ask.rs`: up to five buttons a row, so six options take two rows; for
  `other`, a string select of every live unsealed thread). Do not touch `crates/theseus-discord/src/runtime.rs` (one
  line of room).
- The simulated owner (`bench/threads/th_owner.py` `OPTION`, `parse_ask`): it must still parse the line, and when
  the right thread's title is not listed but `other` is, answer with the stream's slug. Its Python tests too.
- The Jev half of plan row 5 (classify's `thread` question saying that a persona cue such as "Security view:" picks
  a voice, not a stream) is **not yours**: the cross-stream row bumps `classify` to v4 for its own question and
  carries that sentence. Change no pack.

### Row 3 (stretch): the bench routes at the plan's real gaps (theseus-98bz.3)
- `crates/theseus-protocol/src/submit.rs` `TurnSubmitParams` gains `route_at_ms: Option<u64>` (optional on the wire,
  `ts(optional)`). The daemon honours it only when `[threads] bench_clock = true` (default **false**); otherwise a
  submit that carries it is refused as invalid params, with a message naming the key.
- `crates/theseus-core/src/rpc/thread_flow.rs` `route_to_thread` (its `now`, near :169) uses it as `now` for
  `pre_route` and `state.arrived`; `thread_routed_turn` (near :688) records `state.replied` at that clock plus the
  turn's real duration. Nothing else reads the bench clock.
- The CLI: `theseus ask --route-at-ms N` (in `crates/theseus/src/`, where `ask`'s flags are). `bench/threads/
  th_threads.py` passes each message's plan time (`m.t`, minutes from the plan's start, `th_generate.py`; as milliseconds from
  a fixed base) and sets
  `[threads] bench_clock = true` in the arms' config (`arm_config`). Do not touch `th_oracle.py`, `th_score.py` or
  `report.py`: the scorer fix is done separately.

### Row 6 (stretch): a thread's working set grows from its own turns (theseus-98bz.5)
- A new module (e.g. `crates/theseus-core/src/thread_grow.rs`, one `mod` line in `lib.rs`) with a pure
  `grow(current, touched, named, held_elsewhere) -> WorkingSet`, and one call at the turn end of every turn that ran
  in a thread: routed turns end in `thread_routed_turn` (`rpc/thread_flow.rs` near :688); find where an explicitly
  addressed turn (`ask --thread`) ends and make it the same call. Do not use `turn/end_note_step.rs` or the turn-end
  facts code: another row in flight edits them.
- Touched: the turn's tool calls' paths (read, edit, write; a command's working directory). Files written are in
  `crates/theseus-core/src/rewind.rs` `written(execution_id)` until `forget_written` drops them at the turn's end:
  check the order, or read the paths from the turn's tool-call nodes. A path under the workspace gives its file and
  its top directory as a root (`tarnlib/reviews/comment-1.md`: root `tarnlib`, file `tarnlib/reviews/comment-1.md`).
- Named: the user message's entities through `threads_import/extract.rs` `working_set`.
- An entity that any other live thread holds is added to neither. Stop adding a kind at the cap that
  `working_set_of` enforces (`MAX_SET`, `crates/theseus-core/src/rpc/threads.rs` near :911); evicting is a follow-up.
- Write through the internal path `thread.edit` uses (near :497-541), as the harness, only when the set changed. A
  sealed thread's entities never reach another thread's card or Jev's thread inputs (`thread_flow.rs` `inputs`).
- Switch: `[threads] working_set_grows = true`.

### Row 7 (stretch): a home turn that served a thread is claimed (theseus-98bz.6)
- After a turn in the place's home (asked and answered `home`, or a first message before the thread existed), when
  its touched files (row 6's) fall in exactly one live thread's working set (roots or files), mark the turn's nodes as
  that thread's: write `crate::moved::moved_of_key(node)` = `MovedTo { thread, split: "claim_<id>" }`
  (`crates/theseus-core/src/moved.rs`). The lens already reads that key (`thread_recall.rs` `moved_to`), so the
  claimed nodes pass for that thread and stay distant for every other. **Do not edit `thread_recall.rs`.** Writing
  only the META key leaves the home's compile alone (the compile reads `Body::Moved` nodes, not the key): keep it so.
- Check every reader of `MovedTo.split` and of `moved.of.*` (`thread.unsplit`, `history`) so a claim is never taken
  for a split. A node a split already moved is never claimed.
- A turn touching two threads' files is not claimed. A `thread.claimed` ledger row (new kind after `ThreadUnmerged`
  in `crates/theseus-protocol/src/ledger.rs`) names the claim, thread, session and nodes; an undo (`theseus thread
  unclaim ID`, an RPC) writes the keys back to null and a `thread.unclaimed` row.
- Jev's post-turn project answer as a second trigger (plan row 7) only if a thread's project is readable today;
  otherwise name it as a follow-up.
- Switch: `[threads] claim_home_turns = true`.

### Row 12 (only if time is left; plan row 12, theseus-98bz.7)
With one live thread (`decide_single`, no Jev wait), offer `new` when the message names two or more concrete entities
no working set holds and the single thread's pre score is below `CONFIRM`. A place with no live thread is not routed
at all today (a plain turn starts no timer): leave that as it is.

## What already exists (do not redo)
The lens and its four rules (`eb43b3d4`), the home split and `moved.of.<node>` (`fd1f2e51`), `classify.v3`'s
`thread` question. Tests: `thread_route.rs` `the_decision_table_by_band`, the fixture
`crates/theseus-core/fixtures/thread_route_40.json`, `rpc/tests_thread_route.rs`, and `rpc/tests_thread_mix.rs`
(the contamination set: it must pass unchanged).

## Tests (each with a planted revert; the first is the key test)
- **`stickiness_alone_does_not_overrule_jev`** (row 4, `thread_route.rs` tests; the msg-5 shape): a sticky-only
  thread at 0.88, Jev picks another thread at p 0.3 → `Ask`, with `new` offered when the message names two new
  entities (a branch and a ticket); Jev `new` at p 0.52 → `Ask` including `new`. Plant: remove the cap. This test must
  be shown failing on that plant.
- Row 4 also: a sticky thread **with** an entity hit still routes `visible`; the existing "Jev overrules a sticky
  router when sure" case still holds; with `sticky_alone_asks = false` every row of `the_decision_table_by_band` is
  as before; two fixture cases added to `thread_route_40.json` (msg 5 and msg 111 of the plan: "Make the report
  rerunnable from a script." after a security-fix probe); one `rpc/tests_thread_route.rs` test where a fake Jev
  dissents from a sticky place and the ledger shows `thread.asked`, not a `visible` route.
- Row 2: one unit test per cause in `crates/theseus-memory/src/tests_threads.rs` (or `threads.rs` tests); a recall
  test for the msg-113 shape: a home node with share 0.93, paid 0.063, and the thread's own in-context note at 0.104 on
  top gives `thread_own` with those three numbers. Plant: map every cause to `bar`.
- Row 5: an ask over five live threads lists three plus `other` plus `home`, and the fifth thread's slug is accepted
  as the answer; an ask over four lists all four; a sealed thread is never listed; `th_owner.py`'s test parses the
  new line and answers by slug. Plant: back to two options.
- Row 3: a 23-minute gap leaves stickiness below 0.15 (router unit test); a submit with `route_at_ms` and the key off
  is refused; with the key on, two messages 23 bench-minutes apart record a pre score below 0.15 for the sticky thread
  in `thread.routed`. Plant: ignore `route_at_ms`.
- Row 6: after an edit to `tarnlib/reviews/comment-1.md` with "PRR-4142" in the message, the thread holds root
  `tarnlib` and ref `PRR-4142`; "comment-2.md under tarnlib/reviews" then has an entity hit; an entity two threads
  touch is in neither; a sealed thread's entities never reach another thread's card or Jev's inputs. Plant: skip the
  call.
- Row 7: a home turn editing `tarnlib/tarn/retry.py` is claimed by the thread holding root `tarnlib`, and that
  thread's next recall passes the node; a home turn touching nothing threaded is not claimed; a turn touching two
  threads' files is not claimed; the undo restores the lens's old verdict. Plant: never claim.

## Not in this row
- The threadbench run. **Bench measurement (threadbench, any arm, any k) is run by the maintainer after the join,
  never by this session: the VM has no API keys.** Python unit tests under `bench/threads/` are fine to run.
- The scorer (`bench/threads/th_oracle.py`, `th_score.py`, `report.py`): fixed separately, in plan row 1.
- The cross-stream row's files, which run beside you on another branch from the same base: `own_top` in
  `crates/theseus-core/src/thread_recall.rs` and anything else in that file, a place-digest layer under
  `crates/theseus-core/src/compiler/`, the `classify` pack bump and the `scope` question, the Jev cross judge
  (`recall.cross.v1`) after the lens, thread links (`ThreadInfo.links`, `theseus thread link`). In
  `crates/theseus-memory/src/threads.rs` you own `Verdict` and the rules in `judge`'s body; that row changes only
  the doc comment of the `own_top` field, adds one field at the end of `ThreadLens` and one early return at the very
  top of `judge` (for nodes Jev admitted). Leave those two places alone so the join merges. Shared files, kept mergeable: in `config/threads.rs` and the template put your keys
  right after `ask_wait_secs` (that row puts its keys after `base_distance`); in `ledger.rs` add your kinds after
  `ThreadUnmerged` (that row adds after `ThreadUnsealed`); if you add an RPC method, its name goes on the one
  `THREAD_*` line of `crates/theseus-protocol/src/lib.rs` (three lines of room: add none); the regenerated
  `cockpit/src/protocol.gen/` is regenerated again at the join.
- The TUI chat (`crates/theseus-tui`) and the turn-end facts code: other rows in flight.
- `docs/`, the spec, `docs/status.md`, the README: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** Each behaviour ships on by default (row 3's `bench_clock` is a test hook
  and ships off). Each row's switch is its rollback: a test shows that with it off the old behaviour is byte-for-byte
  back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget` passes unchanged, and the
  gate's turn bench must not move); a place with no live thread does no routing work; nothing on the start path grows
  with history; the turn-end work of rows 6 and 7 runs after the reply.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings. Updating an existing test's expected options (row 5's two-option
  asks) is a behaviour change: say which assertions changed and why.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as the section above says.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented (the bench's
  `tarnlib`, `datatidy`, `homesvc`, `PRR-4142`, `OPS-4214` are fine).
- Read the AGENTS.md/CLAUDE.md of every crate you touch first, and `bench/threads/README.md`. Keep CLOUD_REPORT.md
  honest: what you ran, what passed, what you could not run on the VM, and the exact commands for the maintainer.

## Your report
Commit `CLOUD_REPORT.md` at the repository root, written with your editor tools: what changed, per row (done, not
started, or reverted); each test and each plant with the test that failed; what could not run on the VM and the exact
commands for the maintainer; the keel guard's output; the store format if bumped; and the follow-ups, with the plan's
threadbench targets for the maintainer (the home-server stream gets a thread in 3 of 3 A1 runs; top-1 on clear
messages 92% or more; asked-on-clear 15% or less; entity hits on 30% or more of pre-router rows; contamination not
above its bar). Push the branch, check
`CLOUD_REPORT.md` is not empty in the pushed commit, and end with a final message that starts `CLOUD REPORT COMPLETE`.
