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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-ops-shell`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `2d1d8d6f` (2026-10-23: install #75a, a hotfix on release stage 7). That is your base.
Line numbers quoted below are at `2d1d8d6f` but may be off by a few: search by name. The release preamble above is
older than this note; where they disagree about `main`, ceilings or the work in flight, this note wins.

**This wave is patch2 (release 1.0.2): the Operations page.** Three rows start together on `2d1d8d6f` and join a new
lane, `integ/ops`:
- **ops-cal** (R1): the calendars, the calendar cache, `calendar.window`, the sensitive mark, the brief over all
  calendars;
- **ops-verbs** (R2): `session.profile`, `task.rehome`, `task.reschedule`, the data marks on `SessionInfo`/`WorkView`,
  `ProfileInfo.origin`;
- **ops-shell** (R3): the cockpit's `/ops` page, its Board and Threads lenses, the needs-you queue, the parity test.

Three more rows (the Graph, the Timeline, the inspector and the keys) come after these join. Your task's "Not in this
row" section lists what is theirs: leave it for them.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Prefer a META key
or a serde-default field: most of this wave needs no bump. If your row adds a field or a variant to a stored record,
bump it to 50, add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says,
and say so in your report's first section. Several rows may bump; the maintainer renumbers at the join. Never write
the number as a literal anywhere but the constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733. The method table and `SessionInfo`/`ProfileInfo` live here.
  Put every new type in a module file of its own and add to `lib.rs` only the method constants, the `mod`/`pub use`
  line and a field line. Your task names your row's line budget here: keep to it.
- `crates/theseus-core/src/turn.rs` 3,467 / 3,525: add nothing; put turn logic in a module beside it.
- `crates/theseus/src/render.rs` 3,126 / 3,128 and `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500: add nothing.

An unlisted file stays at 2,500 lines or fewer. Never raise a ceiling yourself (the keel guard flags it). If a line
you truly need will not fit, leave the file one line over and say so in your report's first section.

**The owner's rules for this page** (each row's task repeats the ones it touches):
1. Nothing the Agenda, Tasks, Threads and Actions pages show today is lost (a data-parity inventory comes first).
2. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical, and no
   row adds a Discord message. A task change made from the web reaches every task card through the same path the same
   change from any other surface takes; nothing edits a Discord card as such.
3. Calendar data from a sensitive calendar never reaches a model of China origin (nor any model not of US origin),
   never reaches a shared place, and no event title, place, attendee or call link appears in any ledger row.
4. The repository is public: no person's name, no real address, no real calendar id anywhere. Fixtures use invented
   ids under `example.test`. Write "the owner".

**Shared files between this wave's rows** (each row adds; none rewrites):
- `crates/theseus-protocol/src/lib.rs`: ops-cal and ops-verbs each add method constants (ops-cal `calendar.window`
  and the `calendar.changed` notification; ops-verbs `session.profile`, `task.rehome`, `task.reschedule`) and
  ops-verbs adds two field lines. Put your constants on lines of their own, next to the family they belong to.
- `crates/theseus-protocol/src/ledger.rs` (ops-verbs' three row kinds), `crates/theseus-protocol/tests/wire.rs` and
  `tests/wire/*.json`, `crates/theseus-protocol/src/ts.rs` (the type list), `crates/theseus-core/src/rpc/server.rs`
  (the dispatch `match`), and the generated `cockpit/src/protocol.gen/`. Expect the maintainer to merge adjacent
  additions at the join: keep yours together and in one place.
- ops-shell touches no Rust and no generated file.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: ops-shell, the cockpit's Operations page: `/ops`, its rail item and redirects, the regions, the command bar, the needs-you queue, the Board and Threads lenses, the time machine across all of it, and the data-parity test (theseus-jid7t.3)

Branch: `cloud/20261023-ops-shell`. Every commit's subject carries `theseus-jid7t.3`. Deadline for the report: 6 hours
after you start.

**Why.** The owner asked for the cockpit's Agenda to grow into an **Operations** page: one web surface for the task
graph and every live task, session and thread, where any of them can be acted on at any time, and which looks superb:
beauty that carries information. A design was written and approved, with five owner decisions. This row is the shell
and the first two lenses; three later rows add the Graph lens, the Timeline lens (with the calendars), and the
inspector, palette and full keys. It uses **existing protocol methods only**: two rows beside it add new methods
(ops-cal: `calendar.window`; ops-verbs: `session.profile`, `task.rehome`, `task.reschedule`, `SessionInfo.data`,
`ProfileInfo.origin`), and nothing here may depend on them.

**The owner's decisions that bind this row:**
1. `/agenda`, `/tasks` and `/threads` **redirect** to the matching Operations lens, keeping their query
   (`/agenda` → `/ops?lens=timeline`, `/tasks` → `/ops?lens=board`, `/threads` → `/ops?lens=threads`; `?w=` and
   `?sel=` kept). `/threads/:slug` stays a page.
2. **The Ship stays the landing view.** `/` still goes to the Ship. Ops is the rail's second item, after the Ship.
3. A web act never edits a Discord card as such; every act here is one existing protocol call, so a task change made
   here reaches every card exactly as the same change from any other surface would. No Discord anything in this row.
4. (Bulk approve is a later row's; this row has no bulk acts.)

And the owner's standing rule for the cockpit: **no data lost, maximum awesome.** Nothing the Agenda, Tasks, Threads or
Actions pages show today may be lost. That is why the parity test comes first.

**Risk: medium.** It replaces three pages' addresses. The parity test is what makes it safe.

**Prerequisite: base `origin/main` at `2d1d8d6f`.** Your branch joins the lane `integ/ops`. Run the keel guard against
`origin/main`.

## The design (its text is below in what you need; you cannot read the original)
**Principles.** Colour is for state (the cockpit's tones: live, ok, wait, fault, think, money); shape always goes with
colour (a glyph or a word beside every colour); glows only on what needs attention (amber: needs you), what is
selected (gold) and what is live (cyan); Calm mode (`lib/calm.ts`) stops every motion. One protocol call per act,
through `useAct`, ledgered as today. The page reads, it does not poll: live state comes from the `work.watch` the
Threads views keep (`lib/threadwatch.ts`), `task.list` on `task.changed`, `confirm.list` on the confirm pushes; a page
left alone makes no new request. The floor is never bypassed and never surprises: the page greys out what the daemon
would refuse and says why in the daemon's own words.

**Route and address.** `/ops`, labelled "Ops", the second item of the Live group after the Ship. The address carries:
`lens` (`graph` · `board` · `timeline` · `threads`), `sel` (the selected id: `exe_`, `ses_`, `act_`, `wak_`, `tsk_`, a
thread slug, later an event key), `f` (filters), `q` (search), `w` (the week offset), `join` (a later row's), `redact`
(`1`; a later row gives it meaning: parse it and carry it in every link now). A link always reopens exactly what was on
screen. Default lens: `board` (the Graph lens is a later row's).

**Regions.**
```
┌ rail ┬ header (unchanged) ─────────────────────────────────────────────────────────────┐
│      ├ command bar: counters · lens · filter chips · search · New task ─────────────────┤
│      ├ needs-you queue ─┬ canvas (lens) ───────────────────────────┬ inspector ─────────┤
│      │ approvals, Qs,   │ Graph | Board | Timeline | Threads        │ the selection's    │
│      │ failures, seals, │                                           │ facts and verbs    │
│      │ holds; coming up │                                           │                    │
└──────┴──────────────────┴───────────────────────────────────────────┴────────────────────┘
```
`[` collapses the queue, `]` the inspector (`\` will collapse a later row's dock: reserve the key). The choice persists,
as `lib/prefs.ts` does. Below 1280 px the inspector overlays the canvas; below 900 px the queue is the page.

**Command bar.** Counters, each also a filter (click or key): needs you `b`, working `w`, paused·queued `r`, still
today, done today `d`, spent today. Each counter has a 1-hour sparkline (the Bridge's `useHistory`) in its tone; the
needs-you counter glows while above zero and pulses once when it rises. Lens switch: `g g` graph, `g b` board, `g t`
timeline, `g h` threads (`g` then a letter; `1`-`6` are answers). Filter chips: thread (`WorkView.thread`), model
(`SessionInfo.profile`/`model`, `WorkView.moved_to`), place (`WorkView.place`), kind (conversation, task, job,
routine, watch, project), state (`WorkState`), project, ended within. (A `data` chip is a later row's.) Search: title,
brief, report line, progress line; it filters every lens and says "n of m". New task opens the existing New task form
(today's `Tasks.tsx` `NewTask`, every field kept).

**The needs-you queue.** One list of everything waiting on the owner, **longest first by `attention.since_ms`** (the
TUI's tab order):

| item | source | answered with |
|---|---|---|
| tool approval | `confirm.list` | `work.answer` (the card's options) |
| budget ask | `confirm.list` (`budget`) | `work.answer` |
| task change (objective, acceptance, abandon) | `confirm.list` (`change`) and `TaskRecord.proposal` | `work.answer` |
| question with choices | `WorkView.question` | `work.answer` (keys `1`-`6`) |
| failure, not dismissed | `ExecutionView.state` failed / blocked / budget_exhausted | `task.retry`, `execution.dismiss`, open |
| seal recommendation | `ThreadInfo.seal_pending` | `thread.seal`, `thread.seal_dismiss` |
| external-text hold | `health.external_text` | `policy.trust` (asks, as today) |
| misrouted message held | the thread's held rows / view | `thread.move` |

Compact cards (the verb and what it does, its owner, its age, the expiry bar); `⏎` expands to the full card (the same
`ConfirmCard` component, every field). "Coming up": the next 3 hours of marks (the calendar's events join it in a later
row). Stable order: a new item never moves the cursor; it slides in and the counter pulses. A question is answered **in
place** (`work.answer`), not "on the task's page".

**Board lens.** Columns: Needs you, Working, Paused·queued, Scheduled (deferred starts, wakes and routine runs due
today), Done today (then older, paged: "more (N older)"). Rows from `lib/taskboard.ts` (`rowsOf`, `split`), extended
with the paused/queued and scheduled sections and with conversations and jobs (the TUI's board shows every session).
Cards: every field of today's `TaskCard`, plus a model chip (the profile, or `moved_to` when a checkpoint moved it).
Keys `j`/`k` within a column, `h`/`l` across (`l` follows a job only on a job row, as in the TUI).

**Threads lens.** The thread list (every field of today's Threads page), its title "N live · N need you on the board",
the loose-conversations footer ("N conversations are in no thread: Fleet"), the empty state's words plus a **New
thread** button (`thread.new`), and a thread panel with the thread page's fields (goal, details, context tokens, today's
$, seal pending, sealed by and why, topics, history links, card, working set, open work, recent turns) and the thread
verbs (pause, resume, done, reopen, seal, unseal, edit: existing methods). (The Join chat pane is a later row's.)

**Timeline lens, for now.** Until its row lands, the Timeline lens hosts **today's Agenda unchanged** (the week strip,
marks, Selected panel with its verbs, Routines, Watches, Projects, Wakes), so nothing is lost when `/agenda` redirects.

**Graph lens, for now.** Until its row lands, it hosts today's task graph (`TaskGraph.tsx`, as Actions' `?taskgraph=1`
shows it), so the lens is real, not a stub.

**The inspector (this row's part).** The selection's facts and verbs using existing components: a task's or
conversation's full card fields and verbs, a mark's Selected panel (`verbsFor`), a thread's panel, a question's full
`ConfirmCard`. (The live stream, transcript, Record and Standing sections are a later row's.)

**The time machine works everywhere.** Under it (`TimeMachine.tsx`), every lens shows the folded past, as Actions and
Threads do now, and every verb is off, with the same banner.

**Keys (this row's).** One table drives both the key handling and the `?` overlay, so they cannot drift: `j` `k` / `h`
`l` move; `⏎` inspect; `o` open the deck (`/session/:id`); `esc` back or clear; `tab` `⇧tab` next or previous
needs-you; `y` `t` `n` (approve, approve + trust, decline with a note: the note field opens on `n`), `1`-`6` a choice,
`a` `e` the card's grant; `r` `d` retry, dismiss on a failure; `P` pause or resume a task; `s` ×2 stop the turn, `S`
stop and cancel every task under it (asks); `c` ×2 cancel a task; `N` new task (home = the conversation under the
cursor); `g g`/`g b`/`g t`/`g h` lenses; `g m` Home; `/` search; `[` `]` collapse; `?` the keys; `b` `w` `r` `d`
counter filters where no card verb claims the key (resolve conflicts the way the TUI does: a key is a card verb when a
card is focused and a filter otherwise, and say so in the overlay). Keys never fire while typing. `x` keeps the TUI's
meaning (cancel) wherever the TUI had it.

**Visual system.** The cockpit's own tokens: Inter for text, JetBrains Mono for numbers and ids, Cinzel for engraved
labels (lane names, conversation titles), on the hull and deck navy with brass hairlines. Every colour is paired with
a glyph or word; focus rings visible; every chart or strip keeps the cockpit's "table of the same numbers" toggle.

## The data-parity inventory (your `test/ops-parity.test.ts` covers EVERY row)
Every datum and act of the old pages and the TUI board, and its home. **After this row, every row of 1a-1d must have
a home that renders it now** (`here`: on Operations; `stays`: the old place keeps it, e.g. Actions). Rows of 1e and
herdr may be `pending` on a later row (the TUI keeps them meanwhile), named by row. The test fails if a row has no home.
Its pattern is `wakeFacts` against the old Wakes panel (`lib/agenda.ts`, `test/agenda.test.ts`): a pure table module
(for example `src/ops/parity.ts`: id, datum, home component, home kind, and for card/queue/panel fields, the pure
fact function whose output must include the field), with the test asserting every id below is present and every
field the old component showed is produced by the new home's fact function.

**1a. Agenda** (`views/Agenda.tsx`, `WeekStrip`, `RoutinesTable`, `FutureLists`, `Wakes`, `lib/agenda.ts`). A1 week
strip (7 local days by wall-clock hour, DST days `23h`/`25h`, today highlighted, now line, ‹ Today › nav, `?w=`); A2
marks (● reminder, ○ check-in, │ run due, ✓ ✗ – ◐ past runs with skipped dimmed, ▷ deferred start, ⚑ milestone, ▬ watch
bar); A3 hover line (`markLine`); A4 "the daemon would say more than the strip holds"; A5 selected mark (`?sel=`): title,
line, Open, Snooze 10m / 1h / tomorrow 9:00, Move…, Pause/Resume, Cancel; routine Run now/Pause/Resume/Cancel; watch
Cancel; deferred start Cancel; A6 "This week · N items"; A7 the routines table (every column, the 30-run expansion and
tally); A8 watches (every field); A9 projects (every field); A10 wakes (every field and verb, the tip); A11 "the agenda
could not be read: …"; A12 the read discipline (`work.agenda`/`wake.list` only when the week changes or
`touchesFuture`). Home now: the Timeline lens hosting the Agenda unchanged.

**1b. Tasks** (`views/Tasks.tsx`). T1 New task box (brief, title, budget validated with "a quarter of what's left",
home conversation, Ctrl+Enter, the result line, a refusal in the daemon's words); T2 search, "n of m"; T3 sections Needs
you / Working / Done with counts; T4 the task card (paused or state pill, title link, place pill, short id, progress
line, doing label; done: outcome, report line or "no report line", cost; activity ago; waits on; open · join;
Pause/Resume; Stop; its approvals inline, or the question with choices); T5 Done 50 at a time, "more (N older)", report
lines read lazily (`reportLineOf`); T6 `?sel=` scroll-to and glow. Home: the Board lens, the command bar, the queue.

**1c. Threads** (`views/Threads.tsx`, `lib/threadboard.ts`). H1 per thread: sealed lock, title link, state pill,
warm/cold with its tooltip, "● N need you" (and the paused-thread tooltip), details (persona, last activity, context,
today's $), the seal-recommended box, open items (attention pill, title, kind, link); H2 "N live · N need you on the
board"; H3 loose conversations and the Fleet link; H4 the time machine's "Threads then" fold with its caveat; H5 the
empty state's words; H6 the thread page `/threads/:slug` (stays). Home: the Threads lens (and the queue for seals).

**1d. Actions** (`views/Actions.tsx`, `TaskGraph.tsx`, `ConfirmCard.tsx`). X1 approvals waiting (`ConfirmCard`, every
field, read-only under the time machine): queue, and stays; X2 tool calls: stays; X3 external text holds with Trust:
queue, and stays; X4 the wakes link: points to the Timeline lens; X5 the tasks panel: stays (the Graph and inspector
take it in later rows); X6 the task tree: stays; X7 the task graph view: hosted by the Graph lens now, and stays; X8
tool postures: stays; X9 the time machine's banner and disabled acts: global on Operations.

**1e. The TUI board and keys** (`crates/theseus-tui`: `board.rs`, `app.rs`, `ui.rs`'s help). B1 every session as a
tree, questions waiting; B2 `b` `w` `r` `d` `a` filters, `/`; B3 `tab`/`⇧tab`; B4 `y` `t` `n`, `1`-`6`, `a` `e`; B5
`enter` open, `i` type to the session, `⇧tab` its mode; B6 `s` ×2, `S`, `c` ×2; B7 `N`; B8 `P`; B9 `l` follow the job;
B10 PgUp/PgDn/End; B11 `ctrl-t` thread picker, `/t`, `T`; B12 `r` `d` `o` on a failure; B13 `L` self changes; B14 `p`
standing records / parent; B15 `I` the newest turn; B16 `H` the Horizon (`j` `k` `z` `p` `r` `x` `enter`); B17 flash
lines (toasts); B18 narrow screens. herdr: `answer`, `next`, `tui`, `stop`, `new`, `main`; the links
`theseus://answer|history|open/<id>` (unchanged), and deep links `/ops?sel=<id>` and `&do=answer` (build `do=answer`:
it focuses the item's answer in the queue). Mark each `here` or `pending: ops-act` / `pending: ops-graph` /
`pending: ops-timeline`.

## What exists (at `2d1d8d6f`; search by name)
- Router: `cockpit/src/main.tsx` (`createBrowserRouter`; `tasks`, `agenda`, `threads`, `threads/:slug` routes near
  :63-68; the Ship's `wrapShip`, and the index going to the Ship).
- Rail and keys: `components/Shell.tsx` (`NAV` near :35, the `GO` go-keys map near :60 with `d: '/agenda'`,
  `t: '/tasks'`; the cmdk palette), `lib/navgroups.ts` (`NAV_GROUPS`, the Live group).
- Views: `views/Agenda.tsx` (93 lines; its `Detail` panel), `views/Tasks.tsx` (166: `Block`, `TaskCard`, `NewTask`),
  `views/Threads.tsx` (192: the list and `ThreadView`), `views/Actions.tsx` (312).
- Libraries: `lib/agenda.ts` (`allMarks`, `pastMarks`, `verbsFor`, `weekWindow`, `wakeFacts`, ...), `lib/agendawatch.ts`
  (`useFuture`, `useRoutineRuns`), `lib/taskboard.ts` (`rowsOf`, `split`, `reportLineOf`), `lib/threadboard.ts`,
  `lib/threadwatch.ts` (`useThreadBoard`), `lib/progressLine.ts`, `lib/prefs.ts`, `lib/calm.ts`, `lib/rpc.ts`
  (`call`, `useRpc`), `lib/format.ts`.
- Components: `ConfirmCard.tsx` (`ConfirmCard`, `useAct`), `TimeMachine.tsx`, `TaskGraph.tsx`, `WeekStrip.tsx`,
  `RoutinesTable.tsx`, `FutureLists.tsx`, `Wakes.tsx`, `ui.tsx` (`Btn`, `Empty`, `Panel`, `Pill`, `StatePill`).
- Tests run under `node --test` with type stripping (`npm test`); there is no DOM in them, so test pure modules (the
  existing `test/*.test.ts` are the model). `npm run lint` (oxlint), `npm run build` (tsc + vite).
- The protocol's TypeScript is `src/protocol.gen/` (imported as `@protocol`): read it, never edit it.

## What to build
1. The `/ops` route (lazy, like `tasks`), the rail item (second in the Live group; remove the Agenda, Tasks and Threads
   rail items, since their addresses now redirect), the `GO` keys `d` and `t` pointing at the matching lens, and the
   three redirects keeping the query. `/threads/:slug` unchanged. A test: each redirect maps its path and keeps `?w=`
   and `?sel=` (a pure `redirectOf(path, search)`).
2. The address model: one pure module that parses and writes `lens`, `sel`, `f`, `q`, `w`, `join`, `redact`, with a
   round-trip test (a link reopens what was on screen).
3. The regions and their collapse (persisted), the narrow-screen rules.
4. The command bar: counters as filters with sparklines, the lens switch, the filter chips, search with "n of m",
   New task.
5. The needs-you queue: every item kind above, ordered by `attention.since_ms` longest first, stable under live
   updates (a pure `queueOf(...)` with an ordering test and a "new item does not move the cursor" test), compact and
   expanded cards, the answers in place, "Coming up".
6. The Board lens (columns, cards with every `TaskCard` field plus the model chip, conversations and jobs, keys).
   Extend `lib/taskboard.ts` (or a new `lib/opsboard.ts` beside it) and test the new sections.
7. The Threads lens (list, title, footer, empty state + New thread, the thread panel and verbs).
8. The Timeline and Graph lenses hosting today's Agenda and task graph, as above. Move the Agenda page's body into a
   component the lens renders (no behaviour change; `test/agenda.test.ts` stays green untouched).
9. The inspector's part above.
10. The time machine across every lens and region: every verb off, the same banner. A test: under the time machine,
    every verb in the key table and every button's act resolves to disabled (a pure `enabled(verb, timeMachine)` used
    by both).
11. The key table: one module drives the handler and the `?` overlay, with a test that every key in the table has a
    handler and an overlay line, that no two entries claim the same key in the same focus context, and that keys do
    not fire while an input has focus.
12. `test/ops-parity.test.ts` as specified above.
13. `cockpit/AGENTS.md`: a short Operations section (the address, the regions, the parity table and how to keep it
    true, the key table).
14. **Screenshots** of each lens (Board, Threads, Timeline, Graph) with the queue open, in the dark theme at 1440×900:
    run a scratch daemon on this VM on the stand-in model with **invented data only** (two threads, a few tasks, one
    question, one approval, one failure), serve the built cockpit against it, and take headless shots (a headless
    Chrome or Chromium if the VM has one or can fetch one without adding a dependency to `package.json`). Open each
    with `&redact=1` in the address. Commit them under `cloud-report/ops-shell/<lens>.png` (the maintainer drops the
    directory at the merge, as `CLOUD_TASK.md`). If no browser can run here, say so and list the exact addresses
    for the maintainer to shoot instead. No real name, address, id or title may appear in any shot.

## Not in this row
- The Graph lens proper (layout, lanes, multi-select, the bulk bar, drag): ops-graph. Bulk acts of any kind.
- The Timeline lens proper (week strip v2, the calendar layer, clashes, redact mode's behaviour, drag): ops-timeline.
- The inspector's live stream, transcript, Record and Standing sections; the Join composer; the model and mode menus;
  the palette's entities and verbs; the dock; the rest of the key table: ops-act.
- Any Rust, any protocol type, `src/protocol.gen/`: ops-cal and ops-verbs are adding methods in parallel; use none of
  them. `views/Actions.tsx` and `views/SessionDeck`: untouched.
- The Discord DM task flow and anything Discord.

## Rules for this row
- Model roles: you design and write it; make it minimal, elegant and superb to look at. Every line you add should earn
  its place. Reuse the existing pure libraries and components; move code rather than copy it.
- **No data lost.** Before you remove a rail item or add a redirect, the parity test must be green with every 1a-1d
  row `here` or `stays`.
- Every behaviour change gets a test and a planted revert where it can (a pure module's rule broken, its test fails):
  list each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new in `package.json` (React Flow, cmdk, lucide and react-query are already there).
- The cockpit must pass `npm run lint`, `npm test` and `npm run build`, and the gate's cockpit phase.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture and screenshot data are invented.
- Read the root `AGENTS.md` and `cockpit/AGENTS.md` first.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-14), with the files;
- the parity table as built: every row id, its home, and its kind (`here` / `stays` / `pending: <row>`), with any row
  you could not give a home and why;
- the key table, and each key conflict you resolved and how;
- what each later row (ops-graph, ops-timeline, ops-act) should replace or extend, by file and function;
- each test, and its plant with the failing line;
- the screenshots' paths (or the addresses for the maintainer), the data they show, and a line confirming no real
  data and `redact=1` in each address;
- the keel guard's output;
- a `docs/status.md` sentence for the integrator;
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
