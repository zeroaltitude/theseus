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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3e-ops-oneset`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`). This
row is cockpit TypeScript only: no store, wire, protocol or Rust change. If you find you need one, stop and say so.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At the
base: `cockpit/src/ops/state.ts` 113, `ops/board.ts` 308, `ops/queue.ts` 231, `ops/graph.ts` 415,
`ops/GraphLens.tsx` 230, `views/Ops.tsx` 533, `ops/CommandBar.tsx` 159.

**Your lane: `integ/p3-ops`** (new). **This row is its gate: it lands first and alone**, and four rows are written on
its tip afterwards. Stay inside your write scope below, because they own the rest:
- `theseus-hl0r1` (the Graph's and Board's card forms): `ops/GraphNode.tsx`, `ops/GraphLens.tsx` (plan folds),
  `ops/graph.ts` (plan folds, `progressOf`), `ops/BoardLens.tsx`, a new `test/ops-forms.test.ts`.
- `theseus-gnhnk` (the inspector for a step or plan): `ops/Inspector.tsx`, `ops/InspectorSections.tsx`,
  `ops/sections.ts`, `ops/regions.ts`, `views/Ops.tsx` lines ~183-191, 197, 491, tests `ops-selection`, `ops-page`,
  `ops-sections`.
- `theseus-brcs4` (the Threads lens): `ops/ThreadsLens.tsx`, `lib/threadboard.ts`, `test/threadboard.test.ts`.
- `theseus-0zraw` (the ? explainers' words): `lib/explainers.ts`, `test/explainers.test.ts`,
  `components/TaskGraph.tsx` (heading only).

Do not edit any of those files (where you share `GraphLens.tsx`, `graph.ts` and `Ops.tsx`, keep to the parts named
in your scope). Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3e-ops-oneset, ONE work set for the Ops page (theseus-odj0m)

Branch: `cloud/20261024-p3e-ops-oneset`.

Every commit subject carries `theseus-odj0m`. Deadline for the report: about 3 hours after you start.

**Why (the owner's words, 2026-10-24 20:01):** "how come the needs you, paused... selectors don't match the counts in
the graph UI? ... what's supposed to happen when you click one ... the board and the graph don't agree on what
they're showing, and i can't see the connection to the numbers in those status buttons, and clicking them doesn't
seem to do anything".

**Write scope (do not stray outside it):** `cockpit/src/ops/state.ts`, `ops/board.ts`, `ops/queue.ts` (mapping
only), `ops/graph.ts` (its input from rows), `ops/GraphLens.tsx` (hide, context, chip), `views/Ops.tsx` (lines
~123-143 only: the counts and the context set), `ops/CommandBar.tsx` (labels), and the tests `test/ops-state.test.ts`,
`ops-board.test.ts`, `ops-graph.test.ts`, `ops-graph-state.test.ts`, plus a new `test/ops-counts-parity.test.ts`. A
new small file under `cockpit/src/ops/` (for example the filter chip) is fine; name it in the report.

### The model (owner-approved; read `cockpit/AGENTS.md` and `docs/glossary.md` first)

A **plan** and a **step** are both the existing task record (`TaskRecord`, `crates/theseus-protocol/src/tasks.rs`).
A **step** is a record no other record names as `parent`; a **plan** is a record at least one record names as
`parent` (in any state), or one whose `kind` is `project`. Plan-hood is derived, never stored. A **task** (the
running agent) carries a step or plan: a task-carrying record shares its id tail with its session (`tsk_X` <->
`ses_X`). A taskless step's **carrier** is its claim's holder (`TaskRecord.claim`), else the nearest ancestor whose
record has a task, else its `origin.session`'s conversation. No daemon change: everything below is cockpit-only.

### Why the counters, the Board and the Graph disagree today (read the code until you see each)

1. Three different sets: the counters count the Board's rows (`countersOf`, `board.ts:~268`) plus the queue's
   length for needs (`views/Ops.tsx:~143`); the Board draws rows (`boardRows`, `board.ts:~93-148`); the Graph draws
   rows **plus every record that is not a task's own** (`graph.ts:~144-161`), which are neither rows nor counted.
2. Needs counts asks (`queue.length`), the Board counts rows, the Graph counts nodes with `needs`.
3. The filter can't reach steps: `shownIds` is built from rows (`Ops.tsx:~131`), so under any filter every record
   node is "left out", drawn only as a faint dashed outline (`FILTERED_OUT`, `state.ts:~25`).
4. Jobs are rows counted as working while drawn as pills beside their task: one build counts twice.
5. Two bucket paths: the Board's `columnOfTask` (`board.ts:~67-75`) against the Graph's `bucketOfNode`.

### What to build: the one work set

**`boardRows()` in `ops/board.ts` becomes the one work set: every card once, each with its one bucket and whether
it counts. The counters, the Board, the Graph, the selection and the parity test all read only those rows.**

- `RowKind` gains `'step'`. **A plan is a step row with `plan: true`** (a project is always a plan). One row per
  record that is not a task's own, from `records` and the board's step views (`WorkKind::Step` views carry parent,
  root and state). Row ids are unchanged: a task-carrying record keeps its row id `ses_…` and adds
  `recordId: tsk_…`; a taskless record's row id is its `tsk_…`. `selects()` (`board.ts:~260`) matches both.
- Each row carries `bucket`, `counted`, `recordId`, `carrier` and `plan`.
- `StateBucket` gains `'todo'` (used only when the owner call Q2 below is answered A; see "Owner calls").
- **`state.ts` stays the only place that maps a state to a bucket.** Add `bucketOfCard(row)`, composing today's
  `bucketOf`, `bucketOfWork` and `bucketOfRecord`. `columnOfTask` folds into it, so there is one path (a failure
  not yet dismissed is needs, as `columnOfTask` has it). The record rules, with each owner-call choice marked:

  | record state | bucket |
  |---|---|
  | `waiting_human`, `blocked`, or a `proposal` waiting | needs |
  | `in_progress` with its own task | the task's bucket (`bucketOfWork`, unchanged) |
  | `in_progress` without a task, carrier in flight | working (counted or not: Q3) |
  | `in_progress` without a task, carrier not in flight | working today; paused if Q9 is answered yes |
  | `suspended` | paused |
  | `accepted`, `proposed` | paused today; `todo` if Q2 is answered A |
  | `done`, `failed`, `abandoned` | ended |

- `boardRows()` takes the queue (`queueOf`'s items) as input. **Map each ask to its card**: by view id, record id or
  session, or a seal to its thread's head. A card with an ask is `needs` and carries `asks: n`. An ask with no card
  (a router ask for no drawn session) becomes an **ask row** of its own (`kind: 'ask'`) in its lane (Home if none),
  so it is visible and counted. `queue.ts` changes for the mapping only; the Needs-you list stays as it is.
- `counted` is true for every card except: thread heads (lanes), folds, and (by Q3) jobs and a taskless doing step
  whose carrier is in flight.
- `COLUMN_OF` (`board.ts:~62`) becomes `Record<StateBucket, Column | null>` with `todo: null`; `columnsOf` skips a row
  whose column is null (the time machine's `r.rest` skip folds into this). **`rest: 'done'` stays exactly as it is**
  (see "zo72s" below).
- `opsLayout` (`graph.ts:~119`) lays out **these rows** (each carries its `view` and `record`) instead of re-deriving
  items from views and records. Every Graph node is a row, and every row a node. Keep node ids and the sticky slots.
- `countersOf(rows, now, spent)` computes all six counters from the rows; `Ops.tsx:~143` becomes
  `countersOf(rows, now, spend.usd)` alone (no `queue.length`).

### Each counter, exactly (the design's definitions; carry them verbatim into the code's doc comments)

Every counter counts `counted` rows by bucket, each card once:

| counter | counts | today's difference |
|---|---|---|
| **needs you** | cards waiting on the owner: an ask on them (approval, question, budget, failure not dismissed, a step's goal change), or a record `waiting_human`/`blocked`; plus asks with no card | not `queue.length`: one per piece of work, however many questions (Q1) |
| **working** | tasks in flight (running, or waiting on their own calls, jobs or children) and conversations with a turn in flight | jobs are no longer counted; a doing step is its agent's, not a second piece of work (Q3) |
| **paused·queued** | owner-paused tasks, queued executions, waits on a time after today, suspended records, and a doing step nobody works | `todo` steps are no longer here (Q2) |
| **scheduled today** | cards due before local midnight: deferred starts, wakes and routine runs, sleeping waits | label renamed from "still today" to match the Board's column (Q8); the `still` key stays in the URL |
| **done today** | cards that ended since local midnight: tasks, conversations that ended, steps closed | steps now count; a conversation at rest does not (zo72s decides whether it ever does) |
| **spent today** | dollars, unchanged; its filter keeps cards whose session spent | unchanged |

The "today's difference" column describes the recommended answers. Until the owner answers, the defaults below hold.

### Clicking a counter visibly filters (§3.5)

A counter click sets `f.bucket` (`CommandBar.tsx:~52`, unchanged). `shown()` filters the one set (`inBucket` now
tests `row.bucket`). The page passes `shownIds` (every kept row) and a **context set** (each kept row's ancestors up
to the thread head, plus the jobs and carried steps of kept rows) to the lens:
- **Graph:** kept cards are drawn normally; context cards with the existing `FILTERED_OUT` outline and the word
  "context" on hover, not counted. The rest: outlined as today, or hidden (Q4 A: hidden with its slot kept as a hole,
  so nothing moves; a lane with no kept card shrinks to its head; the view fits to the kept cards once per filter
  change).
- **A filter chip** on the canvas toolbar reads, for example, **"paused·queued · 4 ×"**; its × or `Esc` clears the
  filter (the `back` handler, `Ops.tsx:~278`). Build the chip as a small component the Board can import later
  (BoardLens.tsx is hl0r1's file: do not edit it; name the one import line it needs in your report).
- The Timeline is not filtered by the counters (it has its own), as today.

### Owner calls: the parts you build but do NOT switch on (the most important rule of this row)

The owner has **not** answered the design's questions yet, and some of the above changes what existing tests in
`ops-board.test.ts`, `ops-graph*.test.ts` and `ops-state.test.ts` expect. You may not decide them, and you may never
write a `Keel:` trailer (only the owner's quoted words can ack one, and the maintainer adds it). So:

1. Hold every choice that depends on an owner question in **one place**: an exported `OWNER_CALLS` object in
   `ops/state.ts` (one field per question), read as a defaulted parameter by the functions that need it
   (`bucketOfCard`, `countersOf`, the row builder's `counted`, the lens's hide). Answering a question is then a
   one-line change of its field.
2. **The defaults are the answers that keep every existing test expectation at the base exactly as it is** (in
   practice: Q1 B, Q2 B, Q3 no, Q4 B, Q8 no, Q9 no; read the tests to confirm each). Do not edit an existing
   expectation to fit a recommended answer.
3. **Build the recommended answer of each anyway**, and prove it with NEW tests that pass the recommended setting
   explicitly.
4. For each question, scratch-flip its default in a throwaway commit, run the cockpit tests and
   `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`, note which existing expectations go red
   (file:line, old -> new) and what the guard prints, then drop that commit. **Never push a flipped default.**
5. In `CLOUD_REPORT.md`, for each question write one line, starting at the line's start, exactly:
   `OWNER CALL: <the question, in the words below>` followed by the field, its one-line flip, and the expectations
   that would move (with the guard's output for them).

The questions, in the design's words (quote them exactly):
- **Q1.** "Needs you counts pieces of work: one per card that waits on you, however many questions it has (A,
  recommended), or every question separately, as the Needs-you list's length today (B)?"
- **Q2.** "To-do steps (listed, not started): a state of their own, *to do*, with no counter and no Board column,
  shown in their plan's n/m and on the Graph (A, recommended), or counted under paused·queued (B)?"
- **Q3.** "Working counts agents only: running tasks and turns. A step being done and a running job are drawn working
  but are not counted again. Yes/no? (Recommended yes.)"
- **Q4.** "A counter click hides everything else on the Graph and the Board, keeping only each match's parents as
  faint context, with a "paused·queued · 4 ×" chip (A, recommended), or only outlines the rest, as today (B)?"
- Also, in the same form if your work touches them: **Q8.** "Rename the counter "still today" to "scheduled today" to
  match the Board's Scheduled column. Yes/no? (Recommended yes.)" (leave the label as it is) and **Q9.** "A step left
  "doing" whose agent has stopped counts as paused·queued ("nobody is working it"), not working. Yes/no? (Recommended
  yes.)"

The chip, the context set, records becoming rows, asks mapped to cards and the layout from rows are the issue's own
fix, not owner calls: build them on. **But if the keel guard flags anything** in your final range (a test removed,
renamed, an assertion removed or weakened), stop that part, put the old test back, paste what the guard printed, and
write it as another `OWNER CALL:` line. Never remove, rename, skip or loosen a test.

**zo72s (a separate, unanswered owner call here):** `COLUMN_OF.rest = 'done'` keeps a resting conversation view in
Done and in done today. Do not change it. Keep this property: **answering zo72s is the one-line change
`rest: null` at `COLUMN_OF`, and no other code may test `rest` for columns or counts** (`countersOf` counts only rows
with a column). Prove it in the report: scratch-flip that line, show which three `ops-board.test.ts` expectations move
and that nothing else does, then drop the flip.

## Tests (each red at the base, with a planted revert)

1. **New `test/ops-counts-parity.test.ts`.** One fixture: a thread, a plan with nested steps, a task-carrying plan
   with a doing step and a job, a suspended step, two asks on one task, a loose router ask, a todo backlog (accepted
   records), a wake due today, and a conversation at rest. For **each** bucket, with the defaults AND with every
   owner call set to its recommended answer: the counter equals the number of kept counted rows (under Q1 B, the
   needs counter equals the asks on the kept needs cards plus the loose ask rows: today's `queue.length`); the counted
   Graph nodes drawn normally (not hidden, not outlined) equal the same; the Board's cards in that column (`done` for
   today) equal it too. Plant P1: let the Graph lay out records outside the row set again: red.
2. `ops-board.test.ts`: new tests that records become step/plan rows (a plan by having a child, keeping its id when
   it gains one), asks map to their card (two asks on one task are one needs card with `asks: 2`; a loose ask is an
   ask row in Home), `COLUMN_OF` null skips a row, and `countersOf` reads rows only. Plant P2: drop the ask mapping:
   red.
3. `ops-state.test.ts`: a table test of `bucketOfCard` over every row of the record table above, under the defaults
   and under the recommended settings. Plant P3: route the Board through the old `columnOfTask`: red.
4. `ops-graph.test.ts` / `ops-graph-state.test.ts`: the filter keeps records (a counter click keeps a matching step
   node drawn normally; today it is outlined), the context set, the hide under Q4 A with slots unchanged, the chip's
   text. Plant P4: build `shownIds` from the old rows: red.
- Keep every existing test green and unchanged. Add tests beside them; never rewrite one.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical, and nothing
  in Discord changes (the design's rule for 1.0.3: Discord keeps its words, "Task graph here" and the 📋 board's lines). Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If any change
  would rename a Discord word or alter a message of that flow, STOP that part and write `OWNER CALL: <what and why>`.
- **Keel:** remove no test, rename no test, lower no test's assertion count, weaken no assertion, raise no budget, cap
  or ceiling, do not edit `scripts/keel-guard.py` or its tests. Before the report run
  `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its whole output. No `Keel:` trailer.
- **Words:** owner-facing text says *plan* and *step* for a record and *task* only for the running agent. Wire and
  storage names (`task.record`, `TaskRecord`, `task.list`'s `records`) stay.
- **Data floor:** nothing new is sent to a model or routed anywhere.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each new test red at `ac298666`, green after; prove each with its planted revert); right over
  compatible.
- **The gate** (read `cockpit/AGENTS.md` "Building and checking" first): in `cockpit/`, `npm ci`, `npm run lint`,
  `npm test`, `npm run build`; then `scripts/shape.sh` from the root if it covers the cockpit. No Rust build is needed
  for this row: say so if you skip the cargo setup.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), ceilings, and every `OWNER CALL:` line
(Q1-Q4, plus Q8/Q9 if touched, plus any keel stop), each with its `OWNER_CALLS` field and one-line flip. (2) **The
one set:** the row fields, `bucketOfCard`'s table as built, the ask mapping, `counted`. (3) **Per owner call:** the
scratch-flip result, the exact expectations that would move (file:line, old -> new) and the guard's output for them.
(4) **zo72s:** the scratch-flip of `rest: null` and what moved. (5) **Tests** and plants P1-P4, red at the base and
green after. (6) **Gate** commands and results. (7) **Keel** output, whole. (8) **DM golden** diff result. (9) **Join
notes:** every file touched with line ranges, the chip's import line for `BoardLens.tsx`, and anything hl0r1, gnhnk,
brcs4 or 0zraw must know about the new row fields. (10) **Follow-ups** (for example the Needs-you header's "N asks"
word under Q1 A, which is in `Ops.tsx` outside this row's lines). (11) One line on the data floor.
