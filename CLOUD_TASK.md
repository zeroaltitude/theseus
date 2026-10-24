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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2f-ops-ui`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from the lane `integ/ops` at `3796ea45` (2026-10-23). That is your base: **not** `main`. Line numbers
below are at `3796ea45` but may be off by a few: search by name. Run the keel guard against `3796ea45`.

**This wave is patch2 (release 1.0.2), its follow-ups.** The lane `integ/ops` was cut from `main` `23328545` (release
1.0.1). At `3796ea45` it holds the Operations page's six rows, each merged with its join fixes:
- **ops-cal**: the calendar cache, `calendar.window` (owner only, in a private place; anyone else gets `REFUSED`, code
  `-32005`), the `calendar.changed` push, the sensitive level;
- **ops-shell**: `/ops`, its regions, queue, Board and Threads lenses, inspector, key table, time machine, parity table;
- **ops-verbs**: `session.profile` (sticky, `null` reverts, `previous` is the undo; a refusal is an ok answer with
  `refused` set), `task.rehome`, `task.reschedule`, `SessionInfo.data` / `WorkView.data`, `ProfileInfo.origin`;
- **ops-timeline**: the Timeline lens, the calendar layer (`ops/calendar.ts`, `ops/timeline.ts`, `ops/calendarwatch.ts`),
  redact mode (`?redact=1`), drag with Undo; join fix `59b3d141`: no Open in Google under redact for a sensitive event;
- **ops-act**: inspector sections and Live, the Join pane, model and mode menus, the palette, the dock, the sheets (no
  `window.prompt` left in `src/ops/`), the `?call=` link dropped; join fix `d1c335a7`: the Timeline's Move asks in a sheet;
- **ops-graph**: the Graph lens (`ops/graph.ts` `opsLayout`, `GraphLens.tsx`, `GraphNode.tsx`), the selection
  (`ops/selection.ts`, `ops/useSelection.ts`), the bulk bar, drag (`ops/rehome.ts`, `DropLayer.tsx`); join fix
  `dcb44a44`: the palette's selection scope is the picked rows.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This is a cockpit
row: no Rust, nothing stored, no bump. If you think you need one, stop that part and say so in your report's first section.

**Line ceilings.** No cockpit file is in `scripts/long-files.txt`; an unlisted file stays at 2,500 lines or fewer. Your
files are far under: `views/Ops.tsx` 499, `ops/graph.ts` 335, `ops/timeline.ts` 282, `ops/calendar.ts` 261,
`ops/GraphLens.tsx` 226, `ops/GraphNode.tsx` 169, `ops/BoardLens.tsx` 146, `ops/DropLayer.tsx` 93, `ops/EventDetail.tsx`
87, `ops/calendarwatch.ts` 75. Never raise a ceiling.

**The other rows of lane `integ/p2-follow` start now, from other bases, and join with yours.** Two share your base and
the cockpit; stay additive:
- **p2f-ops-act** (same base) owns `cockpit/src/ops/floor.ts`, `ModelMenu.tsx`, `JoinPane.tsx`, the palette core
  (`palette.ts`, `paletteStore.ts`, `PaletteGroups.tsx`, `useRunEntry.ts`), `views/SessionDeck*`, `verbs.ts`, and the new
  protocol methods (`job.stop`, `thread.asks`). It may also touch `Ops.tsx` (the palette's watches) and `keys.ts`.
- **p2f-ops-daemon** (same base) owns Rust only: `crates/theseus/src/client.rs`, the rehome / `session.profile` handlers,
  `calendar_cache.rs`, the Google calendar tests.
- You own the Timeline, Graph, Board, Bridge, parity and shell files. Files you share: `views/Ops.tsx`, `ops/keys.ts`
  (only if you add a key row), `cockpit/AGENTS.md`. Keep your `Ops.tsx` edits to small, separate hunks.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2f-ops-ui, the Operations page's Timeline, Graph and shell follow-ups (theseus-i08xr, theseus-n17am, theseus-uy015)

Branch: `cloud/20261024-p2f-ops-ui`.

Every commit's subject carries its Beads id (`theseus-i08xr`, `theseus-n17am` or `theseus-uy015`). Deadline for the
report: 4 hours after you start. Model role: you write the code; keep it small and plain.

## Your row

### theseus-i08xr: Timeline follow-ups (from the ops-timeline review)

- **F3 IN: Ask Theseus keeps its US-origin profile after the first turn (data-related).** `cockpit/src/ops/EventDetail.tsx`
  `ask()` (~:45-55): it picks a profile with `profileChoices(…, p.usOnly ? US_ONLY_WORDS : undefined, …)`, calls
  `send('event_ask', 'session.open', { kind: 'conversation', label })`, then
  `send('event_ask', 'turn.submit', { session_id, input, ...(pick && { profile: pick }) })`. The profile rides only on
  that first `turn.submit`, so the next turn the owner types in that chat runs on the live profile. The daemon's
  sensitive mark after the calendar read is the backstop; close the gap on the page too. Fix: when there is a `pick`,
  call `session.profile { session_id, profile: pick }` (sticky) **after `session.open` and before `turn.submit`**. Its
  answer is ok even when refused: if `refused` is set, show its words, send no first turn, and still open the deck
  (the chat exists); never fall back to another profile. Keep `profile` on `turn.submit` too. Put the call order in one
  small pure function (say `askCalls(sessionId, p, pick)` in `ops/calendar.ts`, returning the ordered
  `{ method, params }` list) that `ask()` walks, so a test can pin it. All three calls go through `send('event_ask', …)`.
- **F4 IN: no calendar read where nothing shows it, and no repeat after a refusal.** `ops/calendarwatch.ts`
  `useOpsCalendar(timeline, weeks, day, now)` is called on every lens (`views/Ops.tsx` ~:94) and reads today and
  tomorrow for Coming up (`readSpans(null, now)`); a connection that is not the owner's (another device, a shared place)
  gets one `REFUSED` `calendar.window` every five minutes, its error shown only in the Timeline. Fix, two parts:
  1. **Read only where shown.** Outside the Timeline lens, read today and tomorrow only while Coming up is on screen
     (the queue region not folded); with nothing showing it, ask for no span (the hook already reads nothing for an
     empty span list). Mind the order in `Ops.tsx`: `reg` (~:182, `regionsFor(width, qFold, iFold, selection…)`) is
     computed after `cal`, and `selection` reads `cal`. Break that loop plainly (for example, decide "Coming up shows"
     from `qFold` and the width the same way `regionsFor` folds the queue, through one exported helper in
     `ops/regions.ts`, not a copy of its rule).
  2. **A refusal latches.** When `calendar.window` answers `REFUSED` (`code === -32005`), the hook keeps the banner and
     stops its five-minute re-reads; a span change does not retry either. It reads again only when the connection
     re-opens (the `open` flag going false then true) or on the `calendar.changed` push. Any other error behaves as today.
  Keep the one-timer shape: `ops-calendar.test.ts` (~:190-194) pins one `setInterval(` and the exact line
  `if (document.visibilityState === 'visible') every = setInterval(() => void read(), EVERY_MS)`; keep that line as it is.
  Make the decisions pure (for example `readSpans` given "Coming up shows: no" returns `[]`, and `latches(e)` true only
  for the refusal code) so the tests need no React.
- **F5 IN: the night fold opens one hour too many for a mark on the hour.** `ops/timeline.ts` `nightFold` (~:140-156):
  a one-off mark calls `keep(h, h + 0.01)`, so a mark at exactly 22:00 sets `late = ceil(22.01) = 23` and opens
  22:00-23:00. The early side already treats a mark at exactly 07:00 as on the fold's edge (it opens nothing). Make the
  late side match: a mark exactly on the hour at the fold's edge holds that edge (no extra hour), is drawn on the edge
  line (check `yOf` draws it there), and is not counted as folded; a mark at 22:30 still opens through 23:00. Change
  `count` to match the same rule, so a mark is either drawn or counted, never both and never neither.
- **F2 OUT (watch item).** The test `the clash sweep is quick on a busy week: 400 meetings and 400 marks in under 20 ms`
  (`cockpit/test/ops-timeline.test.ts` ~:149) is a wall-clock bound (best of five). It cannot be made deterministic
  without changing what it asserts, which needs a Keel ack. Do not touch it; if it goes red on your VM under load, say so.
- **F6 OUT.** Renaming the test `the Timeline lens hosts the Agenda unchanged, and the Graph lens the task graph`
  (`cockpit/test/ops-parity.test.ts` ~:78) needs a Keel ack the owner has not given. Leave its name.
- **The live check OUT** (the maintainer runs it).

### theseus-n17am: Graph follow-ups (from the ops-graph review). All IN except the live check.

1. **Keys move between Graph nodes.** `views/Ops.tsx` `move(dx, dy)` (~:210) handles only the Board and Threads; on the
   Graph `j`/`k` walk the queue. The layout lives inside `GraphLens.tsx` (`memory.lay(…)`, keep that call: a parity
   needle reads it). Add a pure `stepNode(graph, fromId, dx, dy)` in `ops/graph.ts`: `j`/`k` the nearest node below/above
   in the same column (by `y`, across lanes), `h`/`l` the nearest node in the next column left/right (by `y` distance);
   it skips `done`, `folded` and future nodes; from no focus it takes the first needs-you node, else the top-left one.
   `GraphLens` hands its last layout to the page through a ref (no state, so no render loop); `move` calls `stepNode`
   and `pick`s the id. Left from column 0 goes to the queue, as on the Board.
2. **Palette "switch model" over the selection.** Already wired at the base (`dcb44a44`): `feed.targets.selection` is
   `vis.rows.filter((r) => picked.has(r.id)).map(targetOf)` and `verbEntries` makes one `session.profile` plan per
   picked session, greyed by the floor. What is left: the palette opens on scope `one` even with cards picked. Smallest
   change, in `Ops.tsx` only: while the selection is not empty, set the palette store's scope to `selection`; when it
   empties, back to `one` (`useOpsPalette.setState(…)` in one effect keyed on `picked.size > 0`). Do not edit
   `palette.ts`, `paletteStore.ts` or `PaletteGroups.tsx` (p2f-ops-act's). Test: picked graph rows reach
   `scopeTargets('selection', …)` and `verbEntries` gives a `model:<name>` entry with one plan per picked session (put
   it in `test/ops-selection.test.ts`, not the palette's test file).
3. **An opening for "+N folded" that keeps the 400 cap.** `ops/graph.ts` `opsLayout` (~:197-215) folds subtrees past
   `NODE_CAP` into `fold:<root>` nodes; `GraphNode.tsx` `Fold` (~:123) enables its button only for `done`. Add
   `opened?: ReadonlySet<string>` to `LayoutOpts` (fold roots the owner opened): an opened root's subtree is never
   chosen to fold, and the cap folds other subtrees instead. The cap still holds: if the graph cannot get under 400
   with that subtree open, it folds back and the toolbar says why ("could not open: the graph is at its cap; narrow it
   with the filters or the search"). The `+N folded` button opens it; "tidy" forgets the openings, as it forgets
   `expanded`. Never raise `NODE_CAP`.
4. **`?redact=1` masks node titles.** The Graph does not read `redact` today. Pass `a.redact` to `GraphLens`; under
   redact a node whose data mark is private or sensitive (the marks `GraphNode` already locks, `locked(n.data)`) shows
   its kind and short id instead of its title, in the card, its `title=` tooltip, its progress line and a job pill; a
   lane whose `data` is private or sensitive shows "a <mark> thread" instead of its title. One pure helper
   (say `shownTitle(n, redact)` in `ops/graph.ts`), used everywhere a node's or lane's title is drawn.
5. **The Graph's right-click uses ops-act's `VerbMenu`.** `GraphLens.tsx` has its own `Menu` (~:217) fed by
   `offersFor`/`onVerb` (the bulk offers for one node). The Board already opens `VerbMenu` through `onContext(id, x, y)`
   and the page's `ctxMenu` (`Ops.tsx` ~:462). Give `GraphLens` the same `onContext` prop, open it only for a node with
   a row, delete `Menu`, `offersFor` and `onVerb` from the lens, and keep the page's one `VerbMenu` mount. Use
   `VerbMenu` as it is; do not edit it.
6. **Homes leave out ended and retired conversations.** `Ops.tsx` `homes` (~:140) offers every conversation row as a
   re-home target; the daemon then refuses in the toast. Filter with `board.ts`'s `alive(r)` and `!r.session?.retired`.
7. **Only row-backed nodes are picked.** `GraphLens.tsx` `onNodeClick` calls `onPick(n.id)` for any node, and the lasso
   (`onSelectionEnd`) adds any id: a `step` (`tsk_…`), `done:` or `fold:` id inflates "N picked" while every verb ignores
   it. Pick only nodes with a row (`byId.has(n.id)`: work and job nodes the board holds), in both paths.
8. **Space picks the card whose pick box was clicked.** `Ops.tsx` `select` (~:312) picks `focusRow ?? row`; a clicked
   pick box (`BoardLens.tsx` `Card`'s checkbox, ~:98, its click stops propagation, so it does not focus the card) keeps
   the DOM focus, and the page's capture handler `preventDefault`s, so space then toggles the focused card instead.
   Fix: `select` prefers the pick box that holds the DOM focus (mark it, say `data-pick={r.id}`, and read
   `document.activeElement?.closest('[data-pick]')`), else the focused card. Keep the checkbox line the page test pins
   (`<input type="checkbox" checked={picked} onChange={() => onPick(r.id)}`). The dock's own space (`dock_pause`, ctx
   `dock`) stays as it is.
9. **Board drag hover, throttled if cheap.** `BoardLens.tsx` `over` calls `drag.hover` on every `dragover`, and
   `DropLayer.tsx` `hover` (~:41) sets the hint state each time: one page render per pointer move. Keep the verdict per
   event (a `dragover` must `preventDefault` each time), but set the hint only when its target, its verdict or its words
   change, or the pointer moved at least a few pixels. A pure `hintChanged(prev, next)` makes it testable. If this turns
   out not to be cheap, leave it and say so.
- **The live check OUT** (the maintainer runs it, with space pressed while the dock has the focus).

### theseus-uy015: ops-shell follow-ups. Check each at the base; expect nothing to do.

At `3796ea45` each of F3-F6 looks done already; confirm with a read and list it in the report as "already done at the
base", with the evidence. Change nothing that is already right.
- **F3** (`?call=` meant nothing): dropped by ops-act; `follow` now opens the inspector's Live
  (`Ops.tsx` `follow:`), and `test/ops-page.test.ts` asserts `!page.includes('?call=')`.
- **F4** (parity H1/H6 fields): `parity.ts` H1 `fields` has `items`, H6 has `openWork`; `ThreadsLens.tsx` draws
  `f.items`, `ThreadPanel.tsx` draws `f.openWork` (ops-graph).
- **F5** (`window.prompt` in ThreadsLens/ThreadPanel): none left in `src/ops/` (ops-act's sheets; the page test scans for it).
- **F6** (Bridge Wakes tile): `views/Bridge.tsx` ~:131 navigates to `/ops?lens=timeline` and says "on the Timeline".
- **F8 OUT (owner call):** every address write is `replace: true`, so Back does not step lenses. Do not change it.
- Note for the report (the owner should hear it): the inspector is a slim tab while nothing is selected, by design.

## Tests (each red at the base, with a planted revert)

The cockpit's tests are node's own runner over `cockpit/test/*.test.ts` on pure modules (no React). Test the pure
helper; pin a wiring line in `test/ops-page.test.ts` only where no pure test can see it.
- **F3** `ask on a sensitive event holds its US profile: session.open, then session.profile, then turn.submit`
  (`test/ops-calendar.test.ts`): `askCalls` gives the three methods in that order, `session.profile` carries the
  picked US profile, and for an open event with no pick there is no `session.profile`. Also: a refused
  `session.profile` sends no first turn (pure, or a pinned line). Red at the base: no `askCalls`, no sticky call. Plant:
  drop the `session.profile` step.
- **F4** `Coming up's read: nothing when it is not shown, and a refusal stops the re-reads` (`test/ops-calendar.test.ts`
  or `test/ops-timeline.test.ts`): `readSpans` with Coming up hidden outside the Timeline returns `[]`; `latches` is true
  for code `-32005` and false for other errors. Plants: return today's span anyway; latch on every error.
- **F5** `a mark on the fold's edge opens no extra hour` (`test/ops-timeline.test.ts`): a reminder at 22:00 leaves
  `late` at 22 and is not counted; one at 22:30 opens to 23 and is not counted; one at 23:30 with nothing else is
  counted. Plant: back to `keep(h, h + 0.01)`.
- **n17am 1** `the keys walk the graph's nodes` (`test/ops-graph.test.ts`): `stepNode` over a fixture layout (down,
  up, left, right, from none, skips folds). Plant: return the same id.
- **n17am 2** (see item 2), in `test/ops-selection.test.ts`. Plant: the page's scope effect removed (pin the effect line
  in `ops-page.test.ts`).
- **n17am 3** `an opened fold shows its subtree and the cap still holds` (`test/ops-graph.test.ts`, beside the 541-node
  cap test): the opened root's members are drawn, `nodes.length <= 400`, another subtree folded instead. Plant: ignore
  `opened`.
- **n17am 4** `redact masks a private or sensitive node's title, not an open one's` (`test/ops-graph.test.ts`). Plant:
  `shownTitle` returns the title.
- **n17am 5-8** in `test/ops-page.test.ts`: `GraphLens.tsx` has no `function Menu(` and passes `onContext`; `homes`
  filters with `alive(`; pick paths check `byId.has(`; `select` reads `[data-pick]`. Plant: the old line back.
- **n17am 9** `the drag hint changes only when its words or place change` (pure `hintChanged`). Plant: always true.
- Keep every existing test as it is, including the parity test's needles and the pins named above.

## Not in this row

- **p2f-ops-act**: `ops/floor.ts`, `ModelMenu.tsx`, `JoinPane.tsx`, `palette.ts`, `paletteStore.ts`, `PaletteGroups.tsx`,
  `useRunEntry.ts`, `VerbMenu.tsx`, `verbs.ts`, `views/SessionDeck*`, `protocol.gen`, Rust floor tests. **Join notes:**
  your item 2 sets the palette store's scope from `Ops.tsx` without editing the palette files; your item 5 mounts
  `VerbMenu` for Graph nodes through the page's existing `ctxMenu`. If p2f-ops-act changes `VerbMenu`'s props or the
  store's shape, the integrator adapts these two call sites.
- **p2f-ops-daemon**: Rust only (`client.rs`, rehome and `session.profile` handlers, `calendar_cache.rs`). You call
  `session.profile` as it is on the base.
- Other p2f rows (turn, caps, threads, onto, bench, fanout) are on other bases: no shared files.

## Rules for this row

- **Locked Discord DM task flow.** Nothing here touches Discord. Still run
  `git diff --exit-code 3796ea45 -- crates/theseus-discord/src/golden/dm_flow.txt` (must be empty) and paste it.
- **Keel.** No removed or weakened test, no raised budget, cap or ceiling (`NODE_CAP` stays 400), no edit of
  `scripts/keel-guard.py` or its tests, no renamed test (F6 stays as it is). A cloud row cannot carry a `Keel:`
  trailer. Before your last commit run `python3 scripts/keel-guard.py --base 3796ea45 --head HEAD --quiet` and paste
  its output (it must be empty).
- **The data floor is never loosened.** Under the owner's rules, sensitive calendar data reaches only US-origin
  models, never a shared place, and no event title, place, attendee or call link reaches a ledger row or a verb
  payload. F3 tightens this; nothing may loosen it: `profileChoices`' US-only rule, `eventVerbPayload`, `redactEvent`
  and their tests stay as they are. Because F3 and F4 are near data verdicts, end your report with a
  `Data-routing review` section: every call `EventDetail.tsx` and `calendarwatch.ts` make after your change, with
  their params, and what a sensitive event sends.
- **Public repo.** No person's name, no real organisation, address, calendar id or email. Fixtures use invented names
  and ids under `example.test`. Write "the owner".
- **Red first.** Each fix comes with a test red at the base and green after, and a planted revert: break it, see the
  named test fail, restore, then `git status`. Real behaviour only; no benchmark name anywhere.
- **Right over compatible; one helper, not two.** Reuse `alive`, `locked`, `regionsFor`, `send`, `VerbMenu`,
  `scopeTargets`/`verbEntries`; do not copy their logic.
- **No new dependency**, no `package.json` change, no protocol change (so no `protocol.gen` regeneration).
- **Gate.** Read `cockpit/AGENTS.md` (the Operations section) first. In `cockpit/`: `npm ci`, `npm run lint`,
  `npm test`, `npm run build` (`tsc -b && vite build`); then `scripts/shape.sh`. No Rust changes, so no cargo run.
- **Commits.** Sign each (`git commit -S`; if the VM has no key, say so). Beads id in each subject. End each message
  with the same `Co-Authored-By:` trailer the base's recent commits carry (copy it from `git log -3 --format=%B
  3796ea45`; do not retype it).
- `cockpit/AGENTS.md`: one short paragraph in the Operations section for the Graph's keys, openings and redact, and a
  line for the calendar read's latch. Add, never rewrite another row's paragraph.

## Your report

Commit `CLOUD_REPORT.md`, written with your editor tools, never a heredoc; check it is non-empty after the commit.
Sections, in order:
1. **For the maintainer first:** store format (49, untouched), any file near a ceiling (none expected), any OWNER CALL,
   anything you could not do.
2. **Per issue and item** (i08xr F2-F6, n17am 1-9, uy015 F3-F8): done / OUT / already done at the base, why, the
   test, the planted revert and the line that failed.
3. **The gate**: each command and its result (pass/fail counts).
4. **Keel**: the guard's output (empty).
5. **DM golden**: the `git diff --exit-code` result (empty).
6. **Join notes for `integ/p2-follow`**: every file you share with p2f-ops-act (`views/Ops.tsx` hunks, the palette
   store call, `VerbMenu` use, `cockpit/AGENTS.md`), with the line each hunk touches.
7. **Follow-ups** and a `docs/status.md` sentence for the integrator.
8. **Data-routing review** (last): as in the rules above.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
