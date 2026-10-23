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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-ops-timeline`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from the lane `integ/ops` at `d01f1a21` (2026-10-23). That is your base: **not** `main`. The release
preamble above is older than this note; where they disagree about `main`, the base, ceilings or the work in flight,
this note wins. Line numbers below may be off by a few: search by name. Run the keel guard against `d01f1a21`, not
against `origin/main`.

**This wave is patch2 (release 1.0.2): the Operations page, second half.** The lane `integ/ops` was cut from `main`
`23328545` (release 1.0.1). At `d01f1a21` it holds two of the first three rows, merged with their join fixes:
- **ops-cal** (R1): `[google] calendars`, the memory-only calendar cache, `calendar.window` (owner only, on a private
  connection), the `calendar.changed` push, and the sensitive level beside private (`google::private::level`: US-only
  floors, never a shared place, no ledger text), with the morning brief over every calendar; join fixes: a Calendar
  read through `google.call` marks the session sensitive, and recall raises a private digest's mark to sensitive;
- **ops-shell** (R3): the cockpit's `/ops` page (route, rail, redirects, regions, command bar, needs-you queue, Board
  and Threads lenses, the inspector's first part, the key table, the time machine, the parity table); join fix: the
  page's keys answer only the selected card's own item.
- **ops-verbs** (R2: `session.profile`, `task.rehome`, `task.reschedule`, `SessionInfo.data`/`WorkView.data`,
  `ProfileInfo.origin`) is reviewed and joins the lane after your branch is cut: it is **not** on your base. Its
  `task.reschedule` is specified below so you can wire the drag to it.

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

## Your task: ops-timeline, the Operations page's Timeline lens: the week strip v2 with the calendar layer, the clashes, the Selected panel's event verbs, redact mode, the calendar's four states, and drag with Undo (theseus-jid7t.5)

Branch: `cloud/20261023-ops-timeline`. Every commit's subject carries `theseus-jid7t.5`. Deadline for the report: 6 hours
after you start.

**Why.** The owner asked for the cockpit's Agenda to grow into an Operations page that also shows **both of his
Google calendars** (a personal one and a work one; the work one is sensitive), told apart at a glance, beside
Theseus's own marks. It must look superb: beauty that carries information. The daemon side is done on your base
(`calendar.window`, `calendar.changed`, the sensitive mark). Until now the Timeline lens hosts the old Agenda
unchanged; this row makes it the real lens.

**The owner's decisions that bind this row:** a web act is one existing protocol call and never edits a Discord card
(decision 3); the Ship stays the landing view (decision 5); `/agenda` already redirects to `/ops?lens=timeline` with its
query kept (decision 1): keep `?w=` and `?sel=` working exactly as they do.

**Risk: medium-high on data.** This is the first page that draws the sensitive calendar. The redaction and the data
rules below are what a reviewer checks first.

## What your base has (search by name)
- **The page:** `cockpit/src/views/Ops.tsx`; the `a.lens === 'timeline'` branch renders `<Agenda />` (the old page's
  body, unchanged) with "the agenda shows the present" under the time machine. `ops/address.ts` parses and writes
  `lens sel f q w join redact do`; `redact` is parsed and carried in every link, and **nothing masks yet**: you give it
  its meaning. `ops/queue.ts` has `comingUp` (the next 3 hours of marks). `ops/keys.ts` is the one key table (`KEYS`,
  `resolve`, `enabled(verb, past)`, `typingIn`, contexts `Ctx`); `ops/act.ts` `useOpsAct` (every act goes through
  `enabled`; the page test holds that no `useAct`, `Btn` or bare `call` is in `src/ops/`). `ops/parity.ts` holds rows
  A1-A12 with homes in the hosted Agenda.
- **The Agenda's parts you reuse:** `views/Agenda.tsx` and its `Detail` panel, `components/WeekStrip.tsx`,
  `RoutinesTable.tsx`, `FutureLists.tsx` (`WatchList`, `ProjectList`), `Wakes.tsx`, `lib/agenda.ts` (`allMarks`,
  `pastMarks`, `verbsFor`, `weekWindow`, `layout`, `glyph`, `toneOf`, `markLine`, `wakeFacts`), `lib/agendawatch.ts`
  (`useFuture`, `useRoutineRuns`), `test/agenda.test.ts`.
- **The calendar's protocol (from ops-cal; in `src/protocol.gen/`, imported as `@protocol`):**
  - `calendar.window` `{ from_ms, to_ms }` (a span of at most 62 days) → `{ calendars: CalendarWindow[] }`;
  - `CalendarWindow` = `{ id, label, color ('#rrggbb'), sensitive, status, detail, events }`;
  - `CalendarStatus` = `ok | busy_only | not_found | forbidden | auth_lapsed | off`; `detail` is the daemon's words for
    a non-ok status, with its one fix;
  - `CalendarEvent` = `{ key (stable, opaque), start_ms, end_ms, all_day, title?, location?, call?, attendees (a
    count, never who), self_response?, organizer_self, html_link, busy }` (a `busy` block has no title, place or call);
  - the push `calendar.changed` `{ labels }`: a refresh found a change; read the window again;
  - only the owner on a private connection may read it; any other connection gets a refusal with no data. With no
    `[google]` every calendar is `off`.
- **ops-verbs' `task.reschedule`** `{ task, start_at_ms | start_after_ms | start_when }` →
  `{ task, starts_at_ms, previous }` (one more call with `previous` as `start_at_ms` is the undo); only for a deferred
  task that has not started; a refusal is an RPC error with the daemon's words. **It is not on your base** (ops-verbs
  joins the lane in parallel): its generated types are not in `src/protocol.gen/` yet, and `call` takes any method
  name.
- `lib/rpc.ts` `call(method, params)`, `useRpc`, the push binding (`bindPush`, `usePush`); `lib/calm.ts`;
  `lib/prefs.ts` (`usePref`).

## The design (its text is below in what you need; you cannot read the original)

**Principles.** Colour is for state (the cockpit's tones: live, ok, wait, fault, think, money); **material is for
context:** the calendars use the hull's materials (brass, verdigris, then two more), never a signal tone, so a meeting
is never mistaken for a state; shape always goes with colour (a glyph or word beside every colour; hatching and a lock
for a sensitive calendar; a dash for "waits on"). The page reads, it doesn't poll: the calendar is read when the week
changes, on `calendar.changed`, and every 5 minutes while the page is visible; a page left alone makes no other request.
The time machine works everywhere: under it the lens shows the folded past and every verb is off, with the same banner.
The floor is never bypassed and never surprises.

**The Timeline lens** (the approved mockup: a dark deck-navy week grid; the personal calendar's meetings as solid brass
blocks, the work calendar's as hatched verdigris blocks with a small lock; Theseus's marks riding in a gutter at each
day's right edge; a thin cyan now line across today; one clash marked ◆ and counted in the command bar; a selected
work meeting showing its privacy rule and its verbs, with a "US-only" chip beside "Start a prep task").
- **Week strip v2**, three layers per day:
  1. calendar events as background blocks, coloured by calendar (`CalendarWindow.color`); a `sensitive` calendar's
     blocks are hatched and carry a lock, whatever their colour; overlapping events sit side by side in lanes;
     all-day events in a band above the hours;
  2. "ran" bars: each execution's start to end (from the work views and `routine.runs`), coloured by outcome, in a
     3 px left gutter;
  3. Theseus's marks in the right gutter, the same glyphs and tones as today (● reminder, ○ check-in, │ run due,
     ✓ ✗ – ◐ past runs with skipped dimmed, ▷ deferred start, ⚑ milestone, ▬ the watch bar at the column's edge).
- **The window code is the Agenda's** (`weekWindow`): 7 local days by wall-clock hour, DST days labelled `23h`/`25h`,
  today highlighted, the now line, ‹ Today › week nav, `?w=` kept (bounded ±52 as the address does). A **Day zoom** is
  added (`d` day / `w` week inside the lens; see Keys). **Night hours fold** (say 22:00-07:00, unless an event or mark
  is there), with a count of the marks and events inside the fold.
- **Clashes:** a mark or deferred start inside a meeting, or two overlapping meetings, gets a ◆; the count shows in
  the command bar as two Timeline-only counters, "meetings this week" and "clashes" (the command bar's counters are in
  `ops/CommandBar.tsx`: add the two shown only when `lens === 'timeline'`).
- **The legend:** each calendar's label, its material swatch (and lock), its status; the Agenda's warning "the daemon
  would say more than the strip holds" (`agenda.more`) in the same words; the title "This week · N items".
- **Under the strip:** the Routines, Watches, Projects and Wakes panels, the **same components, unchanged**
  (`RoutinesTable` with `RunList`, `WatchList`, `ProjectList`, `Wakes`).
- **The Selected panel** (`?sel=`): a mark's facts and verbs exactly as the Agenda's `Detail` panel (`verbsFor`: Snooze
  10m / 1h / tomorrow 9:00, Move…, Pause/Resume, Cancel; routine Run now/Pause/Resume/Cancel; watch Cancel; deferred
  start Cancel; Open). A calendar event gets: its facts (title, time, calendar, attendees count, the owner's response,
  whether the owner organizes it, the call link, the location), the privacy note for a sensitive calendar, and the
  verbs **Join the call** (the call link), **Open in Google** (`html_link`), **Remind me** (`wake.create`), **Start a
  prep task** (`task.start`) and **Ask Theseus** (a private chat about the event). See the data rules for what each may
  carry.
- **The table toggle:** every chart or strip keeps the cockpit's "table of the same numbers" toggle; the Timeline gets
  one (the events and marks of the window as rows).
- **Hover:** the mark's or event's line in the panel header (`markLine` for marks), and a tooltip.
- **"the agenda could not be read: …"** stays as the lens's banner, same words.
- **"Coming up"** in the queue (`ops/queue.ts` `comingUp`): the next 3 hours of marks **and calendar events**, read
  again on `calendar.changed`.

**The calendar's states** (each failure is a designed state with its fix, never an empty grid):
| state | when | the lens shows |
|---|---|---|
| ok | the events were read | blocks, coloured by calendar |
| not_found | not shared yet, or a wrong id | that calendar's legend entry in the wait tone, "not shared yet"; an empty dashed lane; the sharing steps (below) |
| busy_only | events refused, free/busy given | hatched "busy" blocks with no titles; clashes still found |
| forbidden / auth_lapsed / off | Google is off, or consent lapsed | the legend in the fault tone with its one fix (`theseus google auth`, or the Systems page); Theseus's own marks still draw; a stale week is never shown as current |

The sharing steps, in the page's words (generic: no account, address or organisation in the code): "From the
calendar's own account, open Google Calendar → the calendar → Settings and sharing, and share it with the account
Theseus reads (`[google] account`) with 'See all event details'. If only free/busy is offered, the workspace's admin
must allow external sharing. Then add the calendar to `[google] calendars` and restart." Use `detail` where the daemon
says more.

**Redact mode** (`?redact=1`, and `⇧R` toggles it): replaces a **sensitive** calendar's titles with bars, and drops its
attendees, place, call link and description; times and the calendar's colour stay. It applies to every surface the
lens draws an event on: the blocks, tooltips, the header line, the Selected panel, Coming up, the table toggle, search
results. Under redact, search does not match a sensitive event's title. The address carries `redact=1` in every link
(it already does), so a link reopens redacted. The page never turns redaction on or off by itself.

**Drag** (§2.7 of the design):
| drag | onto | does | method | posture |
|---|---|---|---|---|
| reminder / check-in ● ○ | a time on the strip | moves the wake | `wake.snooze { wake, when: <ISO> }` | do, with Undo |
| deferred start ▷ | a time on the strip | moves its start | `task.reschedule { task, start_at_ms }` | do, with Undo (`previous`) |
Refused, with the reason on the drag cursor: ended work, routine runs (a schedule is edited in a form, `routine.update`),
watches, milestones, calendar events (the page never writes to Google), anything under the time machine. Drag snaps to
5 minutes, keeps the typed-time **Move…** of the Agenda for the keyboard, and the Undo puts back exactly the time before.

**Keys (this row's, appended to `KEYS` in one block):** in the Timeline lens `d` and `w` switch Day and Week zoom
(inside the lens they beat the counter filters `d`/`w`, the same rule the shell uses for `r`/`d` on a failure: the more
specific context wins; say so in the overlay); `⇧R` redact; on a focused mark the Horizon keys of the TUI: `j` `k`
move, `z` snooze, `p` pause, `r` run now, `x` cancel, `⏎` open (parity row B16); `,` and `.` the previous and next
week (`[` and `]` already fold regions). Keys never fire while typing. Every key has an overlay line (the key test
checks it).

## The data rules for this row (owner rule 3; the reviewer checks each)
1. **Nothing of a sensitive event's content leaves the page through a verb.** For an event of a `sensitive` calendar:
   - **Remind me** creates the wake with a note that names only the calendar's label and the time ("the <label> event
     at 14:00"), never the title, place, attendees or link: a wake note is stored and listed.
   - **Start a prep task** and **Ask Theseus** send no title, place, attendee or link into the task's brief or the
     chat. They send the time and the calendar's label and say "read it with the calendar tool". The daemon's own read
     (`calendar.events` on a sensitive calendar) is what marks that session sensitive (fail closed), and from then on
     only a model of US origin may see it. The New task sheet pre-selects a profile of US origin
     (`ProfileInfo.origin === 'us'`) and greys out the others with the reason "this event's calendar is sensitive: US
     models only".
   - **Join the call** and **Open in Google** only open the link in a new tab (`rel="noopener noreferrer"`); nothing
     is sent to the daemon.
   For a calendar that is not sensitive, the verbs may quote the event (title and time) as the design says.
2. **Nothing is persisted.** Event data stays in memory in the page (no `localStorage`, no `usePref`, no URL beyond the
   event's opaque `key` in `?sel=`). The address never carries a title.
3. **Nothing is logged.** No `console.*` of event data.
4. A pure function decides each verb's payload (for example `eventVerbPayload(ev, cal, verb)`), and a test pins rule 1
   for every verb with a sensitive and an open calendar (distinctive invented titles: the test scans the payload's
   every string for them).

## What to build
1. **The calendar read** in its own hook (for example `ops/calendarwatch.ts` `useCalendar(window)`): `calendar.window`
   over the shown window (the week, or ±1 day around a Day zoom), read again when the window changes, on
   `calendar.changed`, and every 5 minutes while the document is visible (the one timer this design allows; it stops
   when hidden and on unmount). A refusal or an error becomes the lens's banner, never an empty grid. Do not change
   the page test's no-poll assertion on `Ops.tsx`; your timer lives in the hook's file.
2. **The layout v2,** pure (extend `lib/agenda.ts` `layout`, or a new `ops/timeline.ts` beside it): events and marks
   share a day; overlapping events in side-by-side lanes; all-day band; DST days (23 h and 25 h) keep wall-clock rows;
   the night fold with its count; the ran bars. Tests for each, on invented events.
3. **Clash detection,** pure: a mark or deferred start inside a meeting; two overlapping meetings (busy blocks count);
   all-day events do not clash. Tests.
4. **The lens component** (for example `ops/TimelineLens.tsx` and `ops/WeekStripV2.tsx`) replacing the `<Agenda />`
   branch in `Ops.tsx`, with the legend, the states, the panels under the strip, the Selected panel, the table toggle,
   hover lines, the Day zoom.
5. **The materials:** the calendar's `color` from the daemon (its palette already avoids signal tones); hatching (a
   CSS repeating gradient) and a lock glyph for `sensitive`; the busy-only hatch. Calm mode stops any motion (there
   should be none but the now line's tick).
6. **Redact mode** as a pure `redactEvent(ev, cal, on)` (and whatever the blocks, tooltips, panel, Coming up and table
   draw goes through it), plus `⇧R`. The "DOM scan" test: there is no DOM in `npm test`, so scan **every string** the
   lens's pure fact functions produce for an event (block label, tooltip, header line, Selected facts, Coming up line,
   table row, search text) for the invented title, place, attendee count text, call link and description; under
   redact none may appear for a sensitive event, and all of them may for an open one.
7. **The states:** each `CalendarStatus` renders its words (a pure `stateOf(cal)` → tone, words, fix), tested for all
   six.
8. **The event verbs** under the data rules, through `useOpsAct` (so the time machine turns them off), with the pure
   payload function and its test.
9. **Drag with Undo:** a pure `dropOf(mark, at, past)` → `{ method, params, undo }` or `{ refused: words }`, tested for
   each row of the drag table and each refusal; the drag sends the right `when` (ISO, local time to UTC exactly, DST
   included) and the Undo restores the previous time. For the deferred start, declare `task.reschedule`'s params and
   result in one small local type (for example in `ops/reschedule.ts`, exactly the shape above) with a comment line
   `// ops-verbs: use @protocol's TaskRescheduleParams/Result at the join`, and call it through `useOpsAct`. Until the
   daemon has the method, its refusal ("unknown method") shows in the toast in its words; the drag's pure tests do not
   need the daemon.
10. **Coming up** joins the calendar's events (redacted as the page is).
11. **Parity:** move rows A1-A12's homes (`at`) in `ops/parity.ts` to your new files (the test reads the sources for
    the needles; make each needle real), mark B16 `here`, and keep every other row as it is. The parity test must be
    green.
12. **The Bridge's Wakes tile** (`components/Bridge.tsx`, near :131) still says "the Agenda page" and goes to `/agenda`:
    point it at `/ops?lens=timeline` and say "the Timeline".
13. `cockpit/AGENTS.md`: a paragraph in the Operations section: the calendar layer, the materials rule, redact mode
    and its data rules, the one timer.
14. **Screenshots** of the Timeline lens in the dark theme at 1440×900, each address with `&redact=1`: run a scratch
    daemon on this VM on the stand-in model with **invented data only**, serve the built cockpit against it, and take
    headless shots (a headless Chrome or Chromium if the VM has one or can fetch one without adding a dependency to
    `package.json`). The scratch daemon has no Google, so it shows the `off` state: shoot that. If you can point it at a
    fake Google (the core's test rig fake, only if it runs standalone without code changes), also shoot `ok` with two
    invented calendars (one sensitive) and `not_found`; otherwise list the exact addresses for the maintainer. Commit
    them under `cloud-report/ops-timeline/<state>.png` (the maintainer drops the directory at the merge). No real name,
    address, id or title in any shot.

## Not in this row
- The Graph lens, multi-select, the bulk bar, drag to a project or lane: ops-graph.
- The inspector's Live, transcript, Record and Standing sections, the Join composer, the model and mode menus, the
  palette, the dock, the New task sheet's other fields: ops-act. (You pre-select a US profile in the New task form the
  page has today, `ops/NewTask.tsx`; ops-act rebuilds that sheet and keeps your rule.)
- Any Rust, any protocol type, `src/protocol.gen/` (read it, never edit it). Writing to Google Calendar from the page.
- Editing a routine's schedule by drag (it stays a form). Anything Discord.

## Rules for this row
- Model roles: you design and write it; make it minimal, elegant and superb to look at. Reuse the existing pure
  libraries and components; move code rather than copy it.
- **No data lost:** every A1-A12 datum the old Agenda shows must still render (the parity test and
  `test/agenda.test.ts` stay green; extend, don't weaken).
- Every behaviour change gets a test and a planted revert where it can (a pure module's rule broken, its test fails):
  list each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base d01f1a21 --head HEAD --quiet` and
  paste its output in the report. It must be empty.
- **Dependencies:** none new in `package.json`.
- The cockpit must pass `npm run lint`, `npm test` and `npm run build`, and the gate's cockpit phase. Run the full gate
  once at the end (the preamble's line); the known L1 reds are listed there.
- **Commits.** Put the Beads id in each subject. End each message with the same `Co-Authored-By:` trailer line the
  base's recent commits carry (copy it from `git log -3 --format=%B d01f1a21`; do not retype it). You cannot sign here.
- **Names:** no person's name anywhere: write "the owner". Fixtures: invented calendars under `example.test`
  (`personal.cal.example.test`, `work.cal.example.test`), invented titles.
- Read the root `AGENTS.md` and `cockpit/AGENTS.md` first.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-14), with the files;
- a **"Data-routing review"** section: for each event verb and each surface, what it carries for a sensitive and for
  an open calendar, and the test that pins it; anything you are unsure of;
- the parity rows you moved, and their new homes;
- the keys you added and each conflict you resolved and how;
- each test, and its plant with the failing line;
- the screenshots' paths (or the addresses for the maintainer), the data they show, and a line confirming no real
  data and `redact=1` in each address;
- the keel guard's output, the gate's result;
- a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
