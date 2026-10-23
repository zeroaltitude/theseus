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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-ops-graph`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the lane `integ/ops` at `fd21a228` (2026-10-23). That is your base: **not** `main`. The release
preamble above is older than this note; where they disagree about `main`, the base, ceilings or the work in flight,
this note wins. Line numbers below may be off by a few: search by name. Run the keel guard against `fd21a228`, not
against `origin/main`.

**This wave is patch2 (release 1.0.2): the Operations page, second half.** The lane `integ/ops` was cut from `main`
`23328545` (release 1.0.1). At `fd21a228` it holds the first three rows, merged with their join fixes:
- **ops-cal** (R1): `[google] calendars`, the memory-only calendar cache, `calendar.window` (owner only, on a private
  connection), the `calendar.changed` push, and the sensitive level beside private (`google::private::level`: US-only
  floors, never a shared place, no ledger text), with the morning brief over every calendar; join fixes: a Calendar
  read through `google.call` marks the session sensitive, and recall raises a private digest's mark to sensitive;
- **ops-shell** (R3): the cockpit's `/ops` page (route, rail, redirects, regions, command bar, needs-you queue, Board
  and Threads lenses, the inspector's first part, the key table, the time machine, the parity table); join fix: the
  page's keys answer only the selected card's own item;
- **ops-verbs** (R2): `session.profile` (sticky, `null` reverts, `previous` is the undo), `task.rehome`,
  `task.reschedule`, `SessionInfo.data` / `WorkView.data` (`DataMark`), `ProfileInfo.origin`; join fix: the data mark
  reads the sensitive level, so a sensitive session lists and stamps `sensitive`.
`cockpit/src/protocol.gen/` on your base holds every type of the three rows; the maintainer regenerates it once more at
the lane's gate. Read it; never edit it.

Three rows now start in parallel on the lane and join it again:
- **ops-graph** (R4): the Graph lens, multi-select, the bulk bar, drag to a project or a lane;
- **ops-timeline** (R5): the Timeline lens with the calendar layer, redact mode, drag with Undo;
- **ops-act** (R6): the inspector's live sections, the Join composer, the model and mode menus, the palette, the full
  keys, the dock, the New task sheet.
Your task's "Not in this row" section lists what is the others': leave it for them.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). These rows are
cockpit rows and should need no bump. If yours truly does, bump to 50, add the old layout's sample to
`crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and say so in your report's first section.

**Line ceilings** (`scripts/long-files.txt`; length / ceiling): `crates/theseus-protocol/src/lib.rs` about 2,577 / 2,733
and `crates/theseus-core/src/turn.rs` about 3,470 / 3,525: add nothing to either. An unlisted file stays at 2,500 lines
or fewer; every cockpit file is unlisted. Never raise a ceiling yourself (the keel guard flags it).

**The owner's rules for this page** (your task repeats the ones it touches):
1. Nothing the Agenda, Tasks, Threads and Actions pages showed is lost: the parity table (`cockpit/src/ops/parity.ts`,
   `cockpit/test/ops-parity.test.ts`) must stay green, and every row you take over moves its home to your code.
2. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical, and no
   row adds a Discord message. A web act is one existing protocol call, so a task change made here reaches every task
   card exactly as the same change from any other surface would; nothing edits a Discord card as such.
3. Calendar data from a sensitive calendar never reaches a model of China origin (nor any model not of US origin),
   never reaches a shared place, and no event title, place, attendee or call link appears in any ledger row.
4. The repository is public: no person's name, no real address, no real calendar id anywhere. Fixtures use invented
   ids under `example.test`. Write "the owner".

**Shared files between this wave's three rows** (each adds; none rewrites another's part):
- `cockpit/src/views/Ops.tsx` (345 lines): each row replaces only its own part (ops-graph the `a.lens === 'graph'`
  branch, ops-timeline the `a.lens === 'timeline'` branch, ops-act the inspector and dock regions) and adds its
  `handlers` entries. Put your logic in new files under `cockpit/src/ops/` and keep your `Ops.tsx` edit to the lines
  that mount it.
- `cockpit/src/ops/keys.ts` (`KEYS`, `Verb`, `BUTTON_ACTS`): each row appends its own rows, together, in one block
  marked with a comment naming the row.
- `cockpit/src/ops/parity.ts`: each row moves only the ids its task names.
- `cockpit/src/ops/Inspector.tsx`: ops-act owns it; ops-timeline adds one `Selection` kind (`event`) and its case.
- `cockpit/AGENTS.md`: each row adds a paragraph to the Operations section.
Expect the maintainer to merge adjacent additions at the join.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: ops-graph, the Operations page's Graph lens: the pure `opsLayout`, React Flow nodes and edges, lanes per thread, multi-select, the bulk bar with its client fan-out, and drag to a project or a lane (theseus-jid7t.4)

Branch: `cloud/20261023-ops-graph`. Every commit's subject carries `theseus-jid7t.4`. Deadline for the report: 6 hours
after you start.

**Why.** The owner asked for the cockpit's Agenda to grow into an Operations page: the task graph and every live task,
session and thread on one web surface, where any of them can be acted on at any time, and which looks superb: beauty
that carries information. Until now the Graph lens hosts the old task graph (`TaskGraphView`); this row makes it the
page's centrepiece: every session, task, step and job as a living graph in lanes, with selection and bulk acts.

**The owner's decisions that bind this row:**
- **Decision 3:** a web act is one existing protocol call; nothing edits a Discord card as such. A re-home or a bulk
  pause reaches every task card exactly as the same act from any other surface would. No Discord anything here.
- **Decision 4: bulk Approve only for approvals that are not of the always-ask kind, and only after a review sheet.**
  The always-ask kind is `ConfirmRequest.floor === true` ("the floor asks: the call touches Theseus's own binary or
  state, or a secret store; it asks at every posture", as `ConfirmRequest.floor`'s doc says). Bulk Approve therefore: offers only items whose
  `floor` is false, that are tool approvals (not a `budget` ask, not a task `change`), and whose `may_answer` is not
  `false`; opens a review sheet that lists each one (its tool, what it does in the card's own `plain` words or
  `reason`, its session, its age) with a box per item, all ticked; and sends one `work.answer` per ticked item. A floor
  approval in the selection is listed in the sheet as "asks one by one" and is never sent.
- **Decision 5:** the Ship stays the landing view. The Board stays the default lens.

**Risk: medium.** A busy graph turns into a hairball; a bulk act can surprise. The guards are below.

## What your base has (search by name)
- **The page:** `cockpit/src/views/Ops.tsx`; its `a.lens === 'graph'` branch renders
  `<TaskGraphView records={records} past={past ?? undefined} shown />` (420 px tall). `ops/board.ts` (`OpsRow`,
  `RowKind = task | conversation | job | wake | routine`, `boardRows`, `columnsOf`, `modelOf` (prefers `moved_to`),
  `projectOf`, `matchesFilters`, `shown`, `selects`), `ops/facts.ts` (`cardFacts`, `threadFacts`, ...),
  `ops/BoardLens.tsx` (the Board's cards; "a selection box" on each card is this row's), `ops/queue.ts` (`QueueItem`,
  `queueOf`, `answerOf`), `ops/keys.ts` (`KEYS`, `Verb`, `Ctx`, `BUTTON_ACTS`, `enabled(verb, past)`, `resolve`,
  `typingIn`, `pressTwice`), `ops/act.ts` (`useOpsAct`: every act goes through `enabled`; the page test holds that no
  `useAct`, `Btn` or bare `call` is in `src/ops/`), `ops/address.ts` (`lens sel f q w join redact do`), `ops/parity.ts`.
- **The graph code to build on:** `components/TaskGraph.tsx` (`TaskGraphView`, `TaskTree`), `lib/taskgraph.ts`
  (`layout`, `taskTree`, `cardOf`, tested by `test/taskgraph.test.ts`), `components/FullGraph.tsx` (`useFullGraph`,
  `FIT`: minimap, zoom and fit), `lib/fullgraph.ts`. `@xyflow/react` (React Flow 12) is already a dependency: DOM/SVG,
  no WebGL.
- **Live state:** `lib/threadwatch.ts` (`useThreadBoard`: the `work.watch` views), `task.list` on `task.changed`,
  `confirm.list` on the confirm pushes, `session.list`. `WorkView` has `parent`, `thread`, `place`, `state`,
  `attention`, `moved_to`, spend, `question`; **`WorkView.data`** and **`SessionInfo.data`** (`DataMark` =
  `open | private | sensitive`) are new on your base (ops-verbs). `TaskRecord` has `deps`, `retry_of` where present.
- **ops-verbs' `task.rehome`** `{ task, home }` → `{ task, from, to, note }`: `home` is a conversation's session id or
  `thread:<slug>`; it moves where the task's report is posted; **the budget stays carved from the original parent** and
  the answer's `note` says so. Refused (an RPC error with the daemon's words): an ended task; a missing, retired,
  task-kind or shared-place home; a missing or done thread; a home whose data mark is weaker than the task's (a private
  or sensitive task cannot report into an open conversation).
- `project.attach` (existing): attaches a task or conversation to a project.
- The shell's results feedback: the page's toasts (`B17 flash lines → toasts`), `lib/calm.ts` (Calm mode stops motion;
  `html.calm`, `prefers-reduced-motion`).

## The design (its text is below in what you need; you cannot read the original)

**Principles.** Colour is for state (the cockpit's tones: live, ok, wait, fault, think, money); shape always goes with
colour (a glyph or word beside every colour; a dash for "waits on"; a lock for private or sensitive data). Glows only on
what needs attention (amber: needs you), what is selected (gold) and what is live (cyan). One protocol call per act;
**bulk acts are client fan-outs of single calls, so each item has its own ledger row and its own refusal** (no batch
method). The page reads, it doesn't poll. Under the time machine every lens shows the folded past and every verb is
off, with the same banner. The floor is never bypassed: the page greys out what the daemon would refuse and says why in
the daemon's own words.

**The Graph lens** (the approved mockup: lanes across a deck-navy canvas, each labelled in engraved Cinzel with the
thread's warmth, context and today's $; conversation anchors at each lane's left; task nodes with a state header,
spend bar and model chip; a running edge flowing in cyan; a "waits on" dashed edge with an arrow; a needs-you node
glowing amber with its keys printed on it; a "✓ 12 done" folded node; a minimap bottom right).
- **Nodes:** conversations (the lane's anchor), tasks, records' steps (`tsk_`), jobs (pill satellites of their
  session); with the "+ future" toggle: wakes, routines, watches and projects. A question is a badge on its owner, not
  a node.
- **Edges:** `WorkView.parent` (solid); `TaskRecord.deps` (dashed, with an arrow: "waits on"); `retry_of` (dotted). A
  running edge flows (a CSS dash animation, which Calm mode stops, as `lib/calm.ts` stops the others).
- **Lanes:** one per live thread, grouped by `WorkView.thread`, ordered needs-you first; a **home / no thread** lane for
  the rest. A lane's label carries the thread's warmth, context, today's $ and seal state (the Threads lens's
  `threadFacts` has them).
- **Layout:** a pure `opsLayout(views, records, lanes, prev)`, tested like `taskgraph.layout`:
  - within a lane, columns by depth, rows by start time;
  - **positions are sticky by id:** a live change never reshuffles the graph; new nodes append and fade in;
  - ended tasks older than an hour fold into one "✓ N done" node per lane, which expands on click;
  - **the node cap is 400;** past it, subtrees fold (deepest and oldest first), with a "+N folded" node.
- **Node:** the state header with glyph and word, short id, title, the progress line (`progressLine`), the spend bar
  (spent against limit, in money), a model chip (the profile, or `moved_to` when a checkpoint moved it: `modelOf`),
  place, a lock when `data` is private or sensitive. Needs-you glows amber, with its keys.
- **Interaction:** click selects (sets `?sel=`, as every lens does), double-click opens the deck; ⇧/⌘-click adds to the
  selection; ⇧-drag lassos; right-click opens the verb menu (the verbs valid for that node, the same table the bulk bar
  uses); drag as below; minimap, zoom and fit, as `FullGraph` does. `?sel=` pans to the node.

**Multi-select and bulk acts.**
- **The selection** is a set of ids of mixed kinds (in memory; `?sel=` keeps the focused one). `␣` toggles the focused
  node or card, `⌘A` (and `ctrl+a`) selects all visible (after filters and search), `esc` clears. It works on the Graph
  **and on the Board** (a selection box on each Board card, `BoardLens.tsx`).
- **The bulk bar** offers each verb valid for at least one selected item, with counts ("Pause 2 · resume 1", "Cancel
  3", "Re-home 2", "Approve 4", "Decline 1", "Attach to project…"). A verb shows greyed, with the reason, when no
  selected item admits it.
- **Destructive verbs ask once,** with the list: Stop, Cancel, Re-home, bulk Decline. Approve follows decision 4 above.
- **Execution:** sequential with a concurrency of 4, each item one call through `useOpsAct`. Results land in one results
  toast: ✓, or refused in the daemon's words, per item. **Failed items stay selected,** so a retry targets exactly them.
- **Under the time machine** the bulk bar is off with the banner's words.

**Drag: what may move, and what may not** (the Graph and the Board):
| drag | onto | does | method | posture |
|---|---|---|---|---|
| task node or card | another conversation, or a thread's lane | its report goes there from now on | `task.rehome { task, home }` (`home` = the conversation's session id, or `thread:<slug>` for a lane) | ask: a sheet that names the effect and says "the budget stays charged to <original parent>" |
| task or conversation | a project chip or a project node (with "+ future") | attaches it | `project.attach` | do |
Refused, with the reason on the drag cursor: ended work, jobs, questions, the system row, routine runs, a running
conversation (it has no parent), a home the page can already tell is weaker in data mark than the task (`data`), and
anything under the time machine. The daemon's own refusal, if it still refuses, shows in the results toast in its words.

**Risks and guards:** a hairball (lanes, sticky positions, done-folding, the 400-node cap, filters in the address); two
surfaces disagreeing (read the same `work.watch` / `task.list` / `confirm.list`; no state of the page's own beyond the
selection and the node positions); bulk surprises (ask once with the full list; per-item results; failures stay
selected).

## What to build
1. **`opsLayout`,** pure (for example `ops/graph.ts`), with tests: stable positions by id under live updates (a node
   added, a node's state changed, a node removed: no other node moves); done-folding at one hour; the 400-node cap
   folding subtrees; deps edges dashed with an arrow; `retry_of` dotted; lanes per thread, needs-you first; the
   home/no-thread lane; jobs as satellites; "+ future" nodes.
2. **The Graph lens** (for example `ops/GraphLens.tsx`, `ops/GraphNode.tsx`) replacing the graph branch in `Ops.tsx`:
   React Flow nodes and edges, lane labels, flowing running edges (stopped in Calm mode and under reduced motion),
   minimap, zoom and fit, `?sel=` panning, the needs-you glow, the lock. The lens fills the canvas region's height.
3. **The selection,** pure (for example `ops/selection.ts`: `toggle`, `selectAll(visible)`, `clear`, `bulkVerbs(items)`
   → each verb with its count and the items that admit it, `afterRun(selection, results)` → the failed ids). **This
   module is the seam for ops-act's palette:** ops-act builds the ⌘K palette with a "scope" (one, all visible, the
   selection) and will read your selection through it at the join; export a small hook (for example `useSelection()`)
   and say its signature in your report.
4. **The bulk bar** (for example `ops/BulkBar.tsx`) with the counts, the ask-once sheet for destructive verbs, the
   decision-4 review sheet for Approve, the fan-out (a pure `fanOut(items, run, 4)` that runs at most 4 at a time and
   returns per-item results; tested with a fake `run` that refuses some), and the results toast.
5. **Drag** on the Graph and the Board: a pure `dropOf(source, target, past)` → `{ method, params, posture, words }` or
   `{ refused: words }`, tested for every row of the drag table and every refusal; the rehome sheet with the budget
   sentence (the daemon's `note` after it answers).
6. **Keys** (appended to `KEYS` in one block, each with an overlay line and a `handlers` entry: the page test checks
   both): `␣` select, `⌘A`/`ctrl+a` select all visible, `esc` clears the selection before it clears anything else,
   and the bulk verbs are buttons (`BUTTON_ACTS`) gated by `enabled`. Keys never fire while typing.
7. **Parity:** move X7's home (the task graph view: now the Graph lens; Actions' `?taskgraph=1` keeps working and gains
   an "open in Operations" link to `/ops?lens=graph&sel=<id>`) and B1's (every session as a tree: `here`, in the Graph
   and the Board) in `ops/parity.ts`. Also the review's nit for whoever lands first: H1's `fields` gain `items` and
   H6's gain `openWork`, and the Threads lens and thread panel draw them from the fact functions' `f.items` /
   `f.openWork` (one line each), so the parity test guards them. The parity test must be green.
8. `cockpit/AGENTS.md`: a paragraph in the Operations section: the Graph's layout rules (sticky by id, folding, the cap),
   the selection and the bulk rules (decision 4).
9. **Screenshots** of the Graph lens (with two threads' lanes, a needs-you node, a running edge, a folded done node)
   and of the Board with three cards selected across two columns and the bulk bar showing, in the dark theme at
   1440×900, each address with `&redact=1`: a scratch daemon on this VM on the stand-in model with **invented data
   only**, the built cockpit served against it, headless shots (a headless Chrome or Chromium if the VM has one or can
   fetch one without adding a dependency to `package.json`). Commit them under `cloud-report/ops-graph/<name>.png` (the
   maintainer drops the directory at the merge). If no browser can run here, list the exact addresses instead. No
   real name, address, id or title in any shot.

## Not in this row
- The Timeline lens and anything calendar: ops-timeline.
- The inspector's Live, transcript, Record and Standing sections, the Join composer, the model and mode menus
  ("switch model" for a selection is ops-act's palette, through your selection seam), the palette's entities and
  verbs, the dock, the New task sheet: ops-act.
- Any Rust, any protocol type, `src/protocol.gen/` (read it, never edit it). Anything Discord. `views/Actions.tsx`
  beyond the one "open in Operations" link.

## Rules for this row
- Model roles: you design and write it; make it minimal, elegant and superb to look at. Every line you add should earn
  its place. Reuse the existing pure libraries and components; move code rather than copy it.
- **No data lost:** the parity test stays green; the Actions page's task graph keeps working.
- Every behaviour change gets a test and a planted revert where it can (a pure module's rule broken, its test fails):
  list each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base fd21a228 --head HEAD --quiet` and
  paste its output in the report. It must be empty.
- **Dependencies:** none new in `package.json`.
- The cockpit must pass `npm run lint`, `npm test` and `npm run build`, and the gate's cockpit phase. Run the full gate
  once at the end (the preamble's line); the known L1 reds are listed there.
- **Commits.** Put the Beads id in each subject. End each message with the same `Co-Authored-By:` trailer line the
  base's recent commits carry (copy it from `git log -3 --format=%B fd21a228`; do not retype it). You cannot sign here.
- **Names:** no person's name anywhere: write "the owner". Fixture and screenshot data are invented.
- Read the root `AGENTS.md` and `cockpit/AGENTS.md` first.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-9), with the files;
- the layout's rules as built, and how it stays readable at 400 nodes;
- the bulk verbs, their postures, and how decision 4 is enforced (the test that pins it);
- the selection seam's signature for ops-act;
- the parity rows you moved, and their new homes; the keys you added and any conflict you resolved;
- each test, and its plant with the failing line;
- the screenshots' paths (or the addresses for the maintainer), the data they show, and a line confirming no real
  data and `redact=1` in each address;
- the keel guard's output, the gate's result;
- a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
