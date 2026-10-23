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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-ops-act`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
  reads the sensitive level, so a sensitive session lists and stamps `sensitive`. The floor's refusal is
  `crates/theseus-core/src/data_mark.rs` (`refusal_of`, `profile_refusal`, `uncleared`, all `pub(crate)`).
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

## Your task: ops-act, the Operations page's acts: the inspector's sections and its Live stream, the Join composer, the model and mode menus, the ⌘K palette's entities and verbs, the full key table, the dock, and the New task sheet (theseus-jid7t.6)

Branch: `cloud/20261023-ops-act`. Every commit's subject carries `theseus-jid7t.6`. Deadline for the report: 6 hours
after you start.

**Why.** The owner asked for an Operations page that can **act on any task, any session, any model, at any time**:
open, inspect, join and steer, pause, cancel, chat into any session, switch a session's model, answer anything that
waits, start tasks. The shell, the Board and Threads lenses and the queue are on your base; the daemon's new verbs
(`session.profile`, `task.rehome`, `task.reschedule`, the data marks) are on your base. This row is the acting half: the
inspector, the composer, the menus, the palette, the keys and the dock.

**The owner's decisions that bind this row:**
- **Decision 2: a model switch from the web HOLDS for later turns** (`session.profile`, sticky), **with an easy
  revert.** The model menu offers both "this turn only" (`turn.submit { profile }`) and "from now on" (`session.profile`);
  the sticky choice shows on the session's header with a one-click revert (`profile: null`) and the toast's Undo
  (one more call with `previous`).
- **Decision 3:** a web act is one existing protocol call; nothing edits a Discord card as such. A message, a steer, a
  pause or a model switch made here reaches every surface exactly as the same act from any other surface would. No
  Discord anything here.
- **Decision 4** (bulk approve) is ops-graph's; **decision 5:** the Ship stays the landing view.

**Risk: medium.** It adds the page's most powerful acts. Every one goes through `useOpsAct` (`enabled`, the time
machine) and is one protocol call.

## What your base has (search by name)
- **The page:** `cockpit/src/views/Ops.tsx` (the regions, `handlers: Record<Verb, …>`, `focusItem`/`focusRow` with the
  join's focus guard), `ops/Inspector.tsx` (`Selection` = row | item | thread | mark | none; `RowWhole` draws a row's
  card fields and its `ConfirmCard`s), `ops/ThreadPanel.tsx` and `ops/ThreadsLens.tsx` (they use `window.prompt` for a
  new thread's title and slug, an edit's title and goal, a seal's reason: replace each with a sheet), `ops/NewTask.tsx`
  (today's New task form, every field: brief, title, budget with "a quarter of what's left", home, Ctrl+Enter, the
  result line, the refusal in the daemon's words), `ops/CommandBar.tsx` (counters, chips: `FILTER_KEYS`, `chipChoices`),
  `ops/board.ts` (`OpsRow`, `matchesFilters`, `modelOf`), `ops/keys.ts` (`KEYS`, `Verb`, `Ctx`, `BUTTON_ACTS`,
  `enabled(verb, past)`, `resolve`, `typingIn`, `pressTwice`, `overlay()`; `fold_dock` `\` is reserved with no dock),
  `ops/act.ts` (`useOpsAct`; the page test holds that no `useAct`, `Btn` or bare `call` is in `src/ops/`),
  `ops/address.ts` (`lens sel f q w join redact do`; `join` is parsed and carried, with no pane yet), `ops/parity.ts`.
- **The cockpit parts to reuse:** `components/Shell.tsx` (the cmdk palette: groups "Waiting for you", "Act", "Views",
  "Look" stay), `components/Composer.tsx` (the deck's composer), `views/SessionDeck` (the transcript; it reads `label`
  and `tab` params, not `call`), `components/StandingRecords.tsx` (`standing.list`), `components/SelfChanges.tsx`,
  `views/TurnInspect.tsx`, `lib/rpc.ts` (`call`, `useRpc`, `useSessionWatch` (model.delta, tool.*, turn.*, confirm.*),
  `usePush` (`narrative`), `narrative.watch`), `lib/calls.ts` / `lib/derive.ts` (`providerCalls`, `RateLimit`: the
  Bridge's headroom from the newest provider call's rate headers), `lib/history.ts` (`useHistory`), `lib/calm.ts`.
- **ops-verbs' protocol (in `src/protocol.gen/`, imported as `@protocol`):**
  - `session.profile` `{ session_id, profile? }` → `{ session_id, profile: string | null, previous: string | null,
    refused?, note? }`: a refusal is an **ok answer with `refused` set** (the floor's words) and nothing written; `note`
    is set for a restricted-origin profile on an open session ("the floor decides each turn");
  - `SessionInfo.data` and `WorkView.data`: `DataMark` = `open | private | sensitive` (optional on the wire);
  - `ProfileInfo.origin`: `us | eu | cn | unknown` (optional);
  - `task.rehome`, `task.reschedule` (the Graph and the Timeline use them).
  - **The floor's refusal table** (the daemon's `data_mark::refusal_of`, which the route floor shares): an unknown
    profile is always refused; with mark `open`, a known profile is never refused; with mark `private` or `sensitive`,
    a profile is refused **iff its origin is not `us`** (`cn`, `eu` and `unknown` refused).

## The design (its text is below in what you need; you cannot read the original)

**Principles.** Colour is for state; shape always goes with colour. One protocol call per act, through `useOpsAct`,
ledgered as today. The page reads, it doesn't poll. Under the time machine every verb is off, with the same banner.
**The floor is never bypassed, and never surprises:** the page hides nothing the daemon would allow, greys out what the
daemon would refuse, and says why in the daemon's own words.

**Postures.** **do:** one press, a toast, Undo where the act is reversible. **arm:** press twice, or the key twice (the
TUI's `s`/`c`). **ask:** a sheet that names the effect. **floor:** the daemon's own question; the owner's answer is the
act, and nothing on the page can skip it. An owner-initiated act at notify posture needs no approval (starting tasks,
messaging, pausing); irreversible or wide acts ask (cancel, stop all, delete a routine, trust, re-home, the global
profile); anything touching Theseus's own state or secrets stays on the floor.

**The verbs** (the table your verb test pins; "new" methods are on your base):
| verb | on | method | posture |
|---|---|---|---|
| Open (the deck, the thread page) | session, task, thread | navigation (`/session/:id`, `/threads/:slug`) | do |
| Inspect | any | `session.history`, `node.list`, `turn.inspect`, `turn.trace`, `work.peek`, `thread.show`, `task.list` | do |
| Join and chat | session, task, thread | `turn.submit { session_id }` / `{ thread }` | do |
| Steer a running turn | running session or task | `turn.submit` while it runs (`turn.steered`) | do |
| Queue after the turn | running session | `turn.submit` (the composer holds it until `turn.ended`) | do |
| Pause / Resume | task | `task.pause` / `task.resume` | do, Undo |
| Pause / Resume / Done / Reopen | thread | `thread.pause` / `thread.resume` / `thread.done` / `thread.reopen` | do |
| Pause / Resume / Run now / Delete | routine | `routine.pause` / `routine.resume` / `routine.run_now` / `routine.delete` | do / do / ask / ask |
| Snooze / Move / Pause / Resume / Cancel | wake | `wake.snooze` / `wake.pause` / `wake.resume` / `wake.cancel` | do (Undo) / ask for cancel |
| Cancel a watch | watch | `wake.cancel` | ask |
| Stop the turn (keep jobs) | conversation | `execution.stop { keep_jobs }` | arm |
| Stop and cancel every task under it | conversation | `execution.stop { tasks: true }` | ask |
| Cancel | task | `task.cancel` | arm |
| Stop a job | job | `job.stop` | arm |
| Answer | question, approval, budget, change | `work.answer` | floor |
| Trust again | session with a hold | `policy.trust` | ask |
| Retry / Dismiss | failed task / execution | `task.retry` / `execution.dismiss` | do |
| Start a task | anywhere | `task.start` | do |
| New thread / Edit / Seal / Unseal / Merge / Split / Link | thread | `thread.new` / `thread.edit` / `thread.seal` / `thread.unseal` / `thread.merge` / `thread.split` / `thread.link` | do / do / ask / ask / ask / ask / do |
| Move a misrouted message | thread | `thread.move` | do |
| Attach to a project | task, conversation | `project.attach` | do |
| Mode (Ask, Edits, Plan, Auto) | session | `session.mode` | do (Auto: ask) |
| Stop-nudge on/off | session | `session.nudge` | do |
| Recompile / Fork / Retire / Reopen | session | `session.recompile` / `session.fork` / `session.retire` / `session.reopen` | do / do / ask / do |
| Model for the next turn only | session | `turn.submit { profile }` | do (origin-gated) |
| Model from now on (sticky) | session, task | `session.profile { session_id, profile \| null }` | do (origin-gated; the floor refuses what the menu greys out) |
| Live profile (the daemon's default) | daemon | `profile.use` | ask |
| Re-home a task | task | `task.rehome` | ask |
| Reschedule a deferred start | task not started | `task.reschedule` | do, Undo |

**The inspector:** one component, sections by kind:
| kind | sections |
|---|---|
| task / conversation | header and verbs; facts (spend of limit, turns·loops, the forecast `WorkView.forecast`, context tokens, cache warm/cold, reports to, wakes parent, 📎 pieces, check); **Live**; transcript (the last turns, "open the deck" for more); **Record** (TaskRecord: claim, version, owner, deps, evidence, objective, acceptance, a waiting change); **Standing** (`standing.list` for the session); the composer |
| job | the command, the eta (`WorkView.eta`), its output followed live, Stop (`job.stop`) |
| thread | the thread page's fields and the thread verbs (on your base) |
| mark / wake / routine / watch / project | the Agenda's facts and `verbsFor` (on your base) |
**Live** merges three streams: the session's narrative lines (the `narrative.watch` the dock keeps, filtered to the
session); the model's words as they come (`session.watch` → `model.delta`, as the deck does); the running job's output
(`work.peek` for the tail, then `action.output` chunks, and `action.unfollow` on leaving).

**The Join pane** (the Threads lens's chat; the address's `join`): it reuses the deck's transcript and `Composer`.
`⏎` steers a running turn (`turn.submit` while a turn runs becomes `turn.steered`, marked in the transcript as a steer);
`⌘⏎` queues after it (the composer holds it until `turn.ended`, as the TUI's queue does); `⇧⇥` cycles the mode. A
thread with no session yet opens one on its first message (`turn.submit { thread }`). The composer says what `⏎` and
`⌘⏎` do. The thread panel beside it shows the thread's mode, data mark, card and working set.

**The model menu** (`m`; the approved mockup: a menu over the Join pane listing every profile with its origin chip,
cost hint and the sessions on it, two China-origin profiles greyed with "this conversation holds sensitive data: US
models only"): `profile.list` with `ProfileInfo.origin` and the live flag; "this turn only" and "from now on"; a
profile the floor would refuse for the session's `data` mark is **greyed with the reason**, by the refusal table above;
and if the daemon still refuses (its `refused`), the toast shows its words and nothing changes. The sticky profile and
its revert show in the session header. **The mode menu** (`M`): Ask, Edits, Plan, Auto (`session.mode`; Auto asks).

**The palette (⌘K).** The shell's cmdk palette gains **entities** (every session, task, thread, routine, watch,
project, profile; ops-timeline adds events later) and **verbs** (the table above). Grammar: verb then target ("pause …",
"switch model …", "answer …", "start task …"), or target then verb; the **scope** is one (the focused), all visible,
or the selection (`⇥` cycles); refused targets show greyed, with the reason (the approved mockup: ⌘K on "switch model"
with three cards selected shows per profile "1 already on it", "~4× the cost", and "2 hold private data" for the CN
one). The existing groups stay.

**The keys** (one table drives the handler and the `?` overlay, so they can't drift):
`j` `k` / `h` `l` move; `⏎` inspect; `o` open the deck; `i` compose; `esc` back or clear; `tab` `⇧tab` next or previous
needs-you; `y` `t` `n` / `1`-`6` / `a` `e` answer; `r` `d` retry, dismiss; `P` pause or resume; `s` ×2 / `S` stop / stop
and cancel all; `c` ×2 cancel; `l` follow the job; `N` new task; **`m` model menu; `M` mode menu**; `g g` / `g b` /
`g t` / `g h` lenses; `g m` Home; `/` search; **`⌘K` palette**; `␣` / `⌘A` select / select all visible (ops-graph's);
`[` `]` `\` collapse regions; `⇧R` redact (ops-timeline's); `?` the keys. From the TUI: `p` and `⌫` go to the parent
(a task's) and show the Standing tab; `I` inspects the last turn (`TurnInspect`); PgUp / PgDn / End scroll the open
session in the inspector; `/t` in the composer opens the thread picker; ⌘K "self changes" opens `SelfChanges`. Keys
never fire while typing. Most of these exist on your base: add the missing ones.

**The dock** (`\` collapses it): **live streams**, `narrative.watch` over every session, newest first; `␣` pauses it
(only while the dock has focus: `␣` elsewhere is ops-graph's select); it follows the selection when "follow" is on.
**Models · profiles:** `profile.list` with the origin and the live flag; the sessions on each profile (from
`session.list`'s `profile`); headroom from the newest provider call's rate headers (`providerCalls`, from the ledger
tail the Bridge already reads). Click a model to filter by it. `⌥`-click makes it the live profile (`profile.use`, ask:
it changes the daemon's default).

**The New task sheet** (`N`, and in ⌘K): every field the form has today, plus what `task.start` already takes: profile
(origin-gated by the home's data mark, the same greying), start time (`start_when`), quotes, open task. The home
defaults to the focused conversation, as the TUI's `N` does. A caller may pass a pre-selected profile and a "US only"
reason (ops-timeline passes them for a sensitive calendar event): keep that rule.

**Filter chip `data`:** open, private, sensitive (from `WorkView.data` / `SessionInfo.data`). A lock glyph on a card
or row whose data is private or sensitive.

## What to build
1. **The inspector's sections** (for example `ops/InspectorSections.tsx`, `ops/Live.tsx`, `ops/RecordFacts.tsx`) wired
   into `Inspector.tsx`'s `RowWhole` and a job kind: facts, Live, transcript, Record, Standing, the composer. Pure fact
   functions for each section, tested on invented fixtures.
2. **Live:** a pure follow state machine (for example `followOf`) for the job stream: `work.peek` then `action.output`,
   and `action.unfollow` on leaving the selection, on unmount and on a lens switch. Test: **following unsubscribes on
   leave**, and never twice. The model's words through `useSessionWatch`; narrative lines filtered to the session.
   The shell's `?call=` link (`handlers.follow` navigates to `/session/<id>?call=<job>`, which the deck ignores): make
   `l` follow in the inspector instead, and drop the `?call=` link.
3. **The Join pane** in the Threads lens, reading `Address.join`: transcript, `Composer`, steer `⏎`, queue `⌘⏎` (held
   until `turn.ended`), mode `⇧⇥`, a new thread's first message, the steer marked in the transcript. A pure composer
   state (`idle | running | queued`) with tests: `⏎` while running is a steer, `⌘⏎` holds until the end, `esc` drops a
   held message.
4. **The model menu and the mode menu,** with the sticky choice, the revert and the Undo (decision 2).
5. **The floor contract test** (the design's "greyed profiles match the floor"). Write the refusal table once, as data
   the two languages share: a JSON file (for example `crates/theseus-core/src/tests/profile_floor.json`, or beside the
   cockpit's tests: say where) listing every (mark × origin × known) case and whether it is refused. Add one Rust test
   in theseus-core, in a new `tests_<subject>.rs` file with one `mod` line (in a file that is not near its ceiling),
   that asserts `data_mark::refusal_of` agrees with every row (`include_str!`), and a cockpit test that asserts the
   menu's pure greying function (for example `greyOf(mark, profile)`) agrees with every row of the same file. A plant on
   each side (break one case) fails its test. This is the only Rust in this row.
6. **The palette:** entities and verbs, the grammar, the scope, refused targets greyed with the reason, the existing
   groups kept. **Scope seam:** ops-graph builds the selection in parallel (`ops/selection.ts`, a hook such as
   `useSelection()`). Do not create that file. Take the scope's targets as an input (`targets: readonly OpsTarget[]`),
   pass `[focused]` and "all visible" from `Ops.tsx`, and leave one line in `Ops.tsx`, commented
   `// ops-graph: the selection scope`, where the selection will be passed at the join. Say the line in your report.
7. **The verb table as a test:** one pure table (for example `ops/verbs.ts`: verb, kinds it applies to, method,
   posture) that the inspector, the palette and the right-click menus all read; a test that each row of the table above
   maps to its method and posture, and that each `act` key in `KEYS` names a verb of the table.
8. **The full key table:** the missing keys above, each with a handler and an overlay line (the page test checks both),
   conflicts resolved the TUI's way and said in the overlay. Keys never fire while typing (the existing test covers
   new rows: extend its list, don't weaken it).
9. **The dock** (for example `ops/Dock.tsx`), its live streams and its models strip, `\` folding it (persisted, as the
   other regions).
10. **The New task sheet** replacing `ops/NewTask.tsx`'s form (keep every field of T1 and its words), and sheets for
    the thread verbs that use `window.prompt` today (new thread, edit, seal).
11. **The `data` chip:** a `FILTER_KEYS` entry, a `matchesFilters` clause, `chipChoices`; the lock glyph.
12. **Parity:** give homes to B5, B10, B11, B13, B14, B15 (the TUI's open/compose/mode, scroll, thread picker, self
    changes, standing/parent, inspect last turn) and R3, R6 (herdr `tui` and `main`: `R6` → `g m`; `R3`: if the web has
    no honest home for "open the TUI", keep it `stays` with the reason) in `ops/parity.ts`. The parity test must be
    green with no `pending: ops-act` left.
13. `cockpit/AGENTS.md`: a paragraph in the Operations section: the verb table and postures, the floor contract file,
    the composer's states, the dock.
14. **Screenshots** in the dark theme at 1440×900, each address with `&redact=1`: the inspector on a running task with
    Live, the Join pane with the model menu open (a profile greyed with its reason), the dock: a scratch daemon on this
    VM on the stand-in model with **invented data only**, the built cockpit served against it, headless shots (a
    headless Chrome or Chromium if the VM has one or can fetch one without adding a dependency to `package.json`).
    Commit them under `cloud-report/ops-act/<name>.png` (the maintainer drops the directory at the merge). If no browser
    can run here, list the exact addresses instead. No real name, address, id or title in any shot.

## Not in this row
- The Graph lens, the selection, the bulk bar and drag: ops-graph. (Use its selection only through the seam in item 6.)
- The Timeline lens, the calendar, redact mode, the calendar event's inspector section: ops-timeline.
- A read that lists messages the router held before the page opened (`thread.asks`): it needs a new method; name it as
  a follow-up.
- Any protocol type or `src/protocol.gen/` (read it, never edit it); Rust beyond item 5's one test. Anything Discord.
  `views/Actions.tsx` and `views/SessionDeck`: untouched.

## Rules for this row
- Model roles: write it minimal, clear and superb to look at. Every line you add should earn its place. Reuse the
  existing pure libraries and components; move code rather than copy it.
- **No data lost:** every field the old New task form, thread page and Tasks card showed still renders (the parity test
  stays green).
- Every behaviour change gets a test and a planted revert where it can (a pure module's rule broken, its test fails):
  list each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base fd21a228 --head HEAD --quiet` and
  paste its output in the report. It must be empty.
- **Dependencies:** none new in `package.json` or `Cargo.lock`.
- The cockpit must pass `npm run lint`, `npm test` and `npm run build`, and the gate's cockpit phase; the Rust test must
  pass with clippy clean. Run the full gate once at the end (the preamble's line); the known L1 reds are listed there.
- **Commits.** Put the Beads id in each subject. End each message with the same `Co-Authored-By:` trailer line the
  base's recent commits carry (copy it from `git log -3 --format=%B fd21a228`; do not retype it). You cannot sign here.
- **Names:** no person's name anywhere: write "the owner". Fixture and screenshot data are invented.
- Read the root `AGENTS.md`, `cockpit/AGENTS.md` and `crates/theseus-core/AGENTS.md` first.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-14), with the files;
- the verb table as built (verb, method, posture), and any verb you could not wire and why;
- the floor contract: the file, both tests, both plants;
- the scope seam's line for ops-graph's selection;
- the parity rows you moved, and their new homes; the keys you added and each conflict you resolved;
- each test, and its plant with the failing line;
- the screenshots' paths (or the addresses for the maintainer), the data they show, and a line confirming no real
  data and `redact=1` in each address;
- the keel guard's output, the gate's result;
- a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
