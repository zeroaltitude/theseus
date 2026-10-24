# CLOUD_REPORT: ops-act (theseus-jid7t.6), branch `cloud/20261023-ops-act`

Base `fd21a228` (lane `integ/ops`). **No store-format bump** (still 49): everything is cockpit code and one Rust test. Nothing
added to `package.json`, `Cargo.lock` or any protocol type; `cockpit/src/protocol.gen/` untouched.

## 1. What I built, item by item

| # | item | files (all under `cockpit/src/` unless noted) |
|---|---|---|
| 1 | Inspector sections: facts, Live, transcript with composer, Record, Standing; a job's own view | `ops/InspectorSections.tsx`, `ops/sections.ts` (pure facts: `taskFacts`, `recordFacts`, `standingLines`, `transcriptTail`, `jobFacts`, `runningJob`, `lastTurnOf`), `ops/Inspector.tsx` |
| 2 | Live and the job follow | `ops/Live.tsx`, `ops/live.ts` (`liveOf`, `appendChunk`, `mergeLive`), `ops/follow.ts` (the pure `followOf` machine), `ops/jobout.ts` (the hook: `work.peek`, `action.output`, `action.unfollow`), `ops/watch.ts` (one `session.watch` however many panes). `l` now only opens the inspector on the job; the `?call=` link is gone |
| 3 | Join pane | `ops/JoinPane.tsx`, `ops/composer.ts` (`idle / running / queued`), `ops/ThreadPicker.tsx`; steer marker in `components/Transcript.tsx` (`↪ steered`, a second user message in one turn) |
| 4 | Model and mode menus, sticky, revert, Undo | `ops/ModelMenu.tsx` (`ModelMenu`, `ModeMenu`, `StickyChip`, `useSticky`), `ops/menus.ts`, `ops/ui.ts` (toast, held choice, this-turn profile), `ops/Toast.tsx` |
| 5 | Floor contract | `cockpit/test/fixtures/profile_floor.json` (24 rows), `ops/floor.ts` (`greyOf`), `test/ops-floor.test.ts`; Rust: `crates/theseus-core/src/tests_profile_floor.rs` (+ one `mod` line in `lib.rs`) |
| 6 | Palette | `ops/palette.ts` (targets, scope, `verbEntries`, `entitiesOf`), `ops/paletteStore.ts`, `ops/PaletteGroups.tsx`, `ops/useRunEntry.ts`; shell: two lines in `components/Shell.tsx` (the group and ⇥) |
| 7 | Verb table as a test | `ops/verbs.ts`, `test/ops-verbs.test.ts`; also `ops/VerbMenu.tsx` (right-click on a card) |
| 8 | Keys | `ops/keys.ts` (rows below), handlers in `views/Ops.tsx` |
| 9 | Dock | `ops/Dock.tsx` (`\` folds it, kept as `cockpit.ops.dock`) |
| 10 | New task sheet; thread sheets | `ops/Sheet.tsx`, `ops/NewTask.tsx` (rewritten as a sheet), `ops/newtask.ts`, `ops/ThreadSheets.tsx` (new, edit, seal), `ops/sheets.ts`; also `ops/TaskSheets.tsx` (re-home, reschedule). No `window.prompt` is left in `src/ops/` |
| 11 | `data` chip and lock | `ops/address.ts` (`FILTER_KEYS`), `ops/board.ts` (`OpsRow.data`, `matchesFilters`, `DATA_MARKS`, `locked`), `ops/CommandBar.tsx`, `ops/BoardLens.tsx`, header pills |
| 12 | Parity | `ops/parity.ts`, `test/ops-parity.test.ts` (below) |
| 13 | AGENTS.md | `cockpit/AGENTS.md` (the acts, the verb table, the floor contract, quiet by construction), `crates/theseus-core/AGENTS.md` (the shared floor file) |
| 14 | Screenshots | `cloud-report/ops-act/*.png` (below) |

## 2. The verb table as built (`ops/verbs.ts`, 43 rows: id, methods, posture)

open nav do · inspect (session.history node.list turn.inspect work.peek thread.show task.list) do · join turn.submit do · steer turn.submit do · queue turn.submit do ·
task_pause (task.pause task.resume) do, Undo · thread_pause (pause resume done reopen) do · routine_pause do · routine_run ask · routine_delete ask · wake_snooze do, Undo ·
wake_pause do, Undo · wake_cancel ask · watch_cancel ask · stop execution.stop arm · stop_all execution.stop ask · cancel task.cancel arm · **job_stop job.stop arm (no such protocol method)** ·
answer work.answer floor · trust policy.trust ask · retry task.retry do · dismiss execution.dismiss do · new_task task.start do · thread_new / edit do · thread_seal (seal, unseal) ask ·
thread_merge (merge, split) ask · thread_link do · thread_move do · attach project.attach do · mode session.mode do · mode_auto ask · nudge session.nudge do · recompile do · retire ask · reopen do ·
model_turn turn.submit do · model_sticky session.profile do, Undo · profile_live profile.use ask · rehome task.rehome ask · reschedule task.reschedule do, Undo.

Differences from the owner's table, said plainly:
- `turn.trace` is not a protocol method (it is a ledger kind); `inspect` lists the six that are.
- **`job.stop` does not exist in the protocol** (the model's `job_stop` tool is not an RPC). The table keeps the row with `missing`, the test names it as the one method the protocol lacks, and the job view shows Stop off with that reason. Stopping the turn that runs a job is `s s` / `S`.
- Wired in the UI (inspector, palette, right-click, sheets, dock, or already on the page): everything except the rows below. **Not wired from this page:** `fork` (needs a node to fork at; the deck's transcript is its home), `thread_merge`, `thread_link`, `thread_move`, `attach` (project attach: ops-graph's drag), `nudge` (palette only), and the routine and wake verbs (they stay the Timeline's `verbsFor`, as the base has them). They are table rows with methods held to the protocol, so the palette can take them as each lands.
- The test pins each row to the owner's method and posture, each method to `crates/theseus-protocol/src` (a typo fails), the reversible rows' Undo, and each `act` key of `KEYS` to a verb (`KEY_VERB`). The palette's entries carry a table verb and its posture, and a test holds each plan's method to that verb's.

## 3. The floor contract

- The file: `cockpit/test/fixtures/profile_floor.json`: every mark (open, private, sensitive) × origin (us, eu, cn, unknown) × known (true, false): 24 rows with `refused`.
- Rust: `crates/theseus-core/src/tests_profile_floor.rs::the_floors_refusals_match_the_shared_table` reads it with `include_str!` and asserts `data_mark::refusal_of` agrees with each row. It passes (shown below).
- Cockpit: `test/ops-floor.test.ts` asserts `greyOf(mark, name, profile)` (the menu's greying, `ops/floor.ts`) agrees with every row, and that the reason is the daemon's words.
- Plants: `ops/floor.ts` `profile.origin === 'us'` → `!== 'cn'` fails `ops-floor.test.ts:13` and `:17`. Rust `refusal_of`'s `(true, DataMark::Open, _)` → `(true, DataMark::Open | DataMark::Private, _)` fails `tests_profile_floor.rs:33`: `Private eu known=true: None`; restored (`touch`ed) it passes, and `git status` is clean after each.

## 4. The scope seam for ops-graph's selection

In `views/Ops.tsx`, inside the `feed` memo that the palette reads:
`selection: [] as ReturnType<typeof targetOf>[], // ops-graph: the selection scope`
Replace the `[]` with `useSelection().map(targetOf)` (or its rows through `targetOf`). `ops/palette.ts` takes `targets` as input only; `ops/selection.ts` is not created. ⇥ in ⌘K cycles one → all visible → selection.

## 5. Parity, keys and conflicts

Parity (`ops/parity.ts`): B5 (enter, `i`, ⇧⇥) → `compose`, `mode_menu`, `JoinPane`'s `composerStep` and ⇧⇥; B9 (`l`) → the inspector's job output (`jobout.ts`); B10 (PgUp/PgDn/End) → `scroll_*`; B11 (thread picker, `/t`, `T`) → `ThreadPicker`, `onPicker` (ctrl-t is the browser's new tab: said in the row); B13 (`L`) → `SelfChangesCard` sheet and ⌘K "Self changes"; B14 (`p`, standing) → `parent` and `StandingBlock`; B15 (`I`) → `last_turn` (the turn inspector route); R6 (`herdr main`) → `g m`. **R3 (`herdr tui`) stays `stays`**: the web has no honest way to open a terminal program; the row says so. No `pending: ops-act` is left, and a test says so. T1 (New task) keeps every needle (brief, title, budget, a quarter of what's left, Ctrl+Enter, the result line, the daemon's refusal).

Keys added (one `KEYS` row each, overlay line and handler; the page test checks both): `m` model menu, `M` mode menu, `i` compose, `I` newest turn, `p` / `⌫` parent (task), PgUp / PgDn / End, `L` self changes, `T` thread picker, `\` folds the dock (now real), `␣` in the dock (ctx `dock`, only while it has focus, outranks everything so ops-graph's `␣` is free elsewhere), `⌘K` (an overlay line; the shell opens it). Contexts added: `session` and `dock`. Conflicts resolved the TUI's way and said in the overlay: `⌫` is the parent only on a task and never in a box; `m`/`M`/`i` need a session; `l` is the job's follow, else "right"; while a sheet or menu is open the page's keys are off (`modal`), so `j`/`k` never move under a menu. The Join pane's header has model and mode buttons because `m` is a letter in its box. The existing "keys never fire while typing" test runs over every new key too.

## 6. Tests and planted reverts (cockpit `npm test`: 619 pass, 0 fail; `npm run lint`: no errors; `npm run build` ok)

New test files: `ops-verbs`, `ops-floor`, `ops-composer`, `ops-follow`, `ops-menus`, `ops-live`, `ops-sections`, `ops-palette`, `ops-newtask`, `ops-sheets`; additions to `ops-board`, `ops-keys`, `ops-page`, `ops-parity`. Each plant: break one rule, see the named test fail at that line, `git checkout` the file and `touch` it (`git status` clean).

| file | plant | test and failing line |
|---|---|---|
| `ops/follow.ts` | leave emits no unfollow | `ops-follow.test.ts:21` |
| `ops/composer.ts` | ⌘⏎ while running sends at once | `ops-composer.test.ts:17` |
| `ops/composer.ts` | a held message goes at the turn's start | `ops-composer.test.ts:19` |
| `ops/verbs.ts` | cancel posture arm → do | `ops-verbs.test.ts:23` |
| `ops/verbs.ts` | a key names no verb | `ops-verbs.test.ts:43` ("stop_all names no verb") |
| `ops/board.ts` | the `data` chip ignores the mark | `ops-board.test.ts:53` |
| `ops/menus.ts` | sticky takes the oldest row | `ops-menus.test.ts:38` |
| `ops/menus.ts` | a refusal still offers Undo | `ops-menus.test.ts:44` |
| `ops/live.ts` | a repeated chunk is appended again | `ops-live.test.ts:28` |
| `ops/live.ts` | a pause does not hold | `ops-live.test.ts:46` |
| `ops/palette.ts` | the floor does not skip a target | `ops-palette.test.ts:18` and `:28` |
| `ops/newtask.ts` | the US-only reason is ignored | `ops-newtask.test.ts:36` |
| `ops/sections.ts` | a lapsed claim is not said | `ops-sections.test.ts:34` |
| `ops/keys.ts` | the dock's `␣` is not first in `PRIORITY` | `ops-keys.test.ts:14` |
| `ops/parity.ts` | B10 pending again | `ops-parity.test.ts:86` |
| `ops/floor.ts` | origin rule broken | `ops-floor.test.ts:13` |
| Rust `data_mark.rs` | a private session may use any known profile | `tests_profile_floor.rs:33` |

Not planted: the React components (read as text by `ops-page.test.ts`: the mounts, `modal`, the seam comment, `post('join', 'turn.submit'`, the leave unfollow, one shared watch) and the transcript's `↪ steered` badge.

## 7. Live checks (scratch daemon, stand-in model, invented data; fresh state dir)

`theseus-sim fake-model` with rules; `[profiles]` sonnet, opus, and glm on a stand-in `zai` provider with `origin = "cn"`; `narrative = true`. I drove the real cockpit in headless Chromium (Playwright from the VM, no dependency added) against `http://127.0.0.1:7434` and saw, with **zero console or page errors** in each run:
- a running task: Live shows its narrative and the job's output lines (`sounding row N`), streamed by `work.peek` then `action.output`; the job's Stop is off with its reason;
- the Join pane: a message sent to a conversation answered in the transcript; ⌘⏎ while a turn runs showed "queued: it goes when the turn ends · esc drops it", esc dropped it and nothing was sent at the turn's end;
- the model menu: `default`/`opus`/`sonnet` (us), `glm` (cn) listed with cost hints and "N on it"; with the conversation's mark rewritten to sensitive in the browser (a WebSocket route; the VM has no Google), `glm` was greyed with "this conversation holds sensitive content, and `glm` is not a model of US origin"; "from now on: opus" toasted with Undo, the header showed "◇ opus held · revert", Undo cleared it;
- ⌘K "switch model deep" listed "Switch model to opus · 1 target · ~2× the cost"; `N` opens the New task sheet; `?` lists the model menu key; right-click lists the card's verbs with reasons.
**Not seen live:** the `↪ steered` badge (the stand-in's turn was in its last call, so the message ran as the next turn, as the daemon says it does); the thread sheets, re-home and reschedule sheets (tested as pure, never driven); the data chip against a real private session.

Maintainer's live check, on the installed build with real keys (never the operator's own state): start a scratch daemon, then
1. `theseus --socket S ask "survey the quay"` style work with a long `proc_run`; open `/ops?lens=board&sel=<task session>&redact=1`: Live shows the job's lines; select another card: `ss -p` shows no stale follow (`action.unfollow` in the ledger/daemon log).
2. Join a task that runs a tool loop (`i`), press ⏎ while it runs: the transcript marks the message `↪ steered`, the ledger has `turn.steered`.
3. Open a session that has read a sensitive calendar and press `m`: every non-US profile is greyed with the floor's words; try `curl`/CLI `session.profile` on one: the daemon's `refused` is the same sentence.
4. `⌘K`, type `pause`, ⇥ to cycle the scope; `⌥`-click a profile in the dock's strip: a confirm, then `profile.use`.

## 8. Screenshots (`cloud-report/ops-act/`, dark theme, 1440×900)

- `inspector-live.png`: `/ops?lens=board&sel=<running task's session>&redact=1`: the inspector on a running task, Live following its job's output, the dock below.
- `join-model-menu.png`: `/ops?lens=board&sel=<conversation>&join=<conversation>&redact=1`: the Join pane and the model menu, `glm` greyed with its reason. The conversation's data mark is rewritten to `sensitive` in the browser for this shot only (no Google on the VM); everything else is the daemon's answer.
- `dock.png`: the dock's streams and models strip (`...&redact=1`).
All data is invented (titles like "Chart the quay soundings", "Tide tables"); no real name, address or id. Every address carries `redact=1` (it is carried and parsed; ops-timeline gives it meaning).

## 9. Keel guard, gate

`python3 scripts/keel-guard.py --base fd21a228 --head HEAD --quiet` prints nothing (the gate's own run: `keel: ok (fd21a2285c..the working tree; 66 files changed, 0 findings acked)`).

The VM's disk filled during the gate (about 40 GB of debug artifacts at two profiles): the gate got through **fmt, shape, features, clippy (`-D warnings`, whole workspace, all targets) and the cockpit's lint, test and build**, and failed in `test build` with `No space left on device`, not on a test. I cleared `target/` and ran what fits:

GATE_RESULT_PLACEHOLDER

## 10. For `docs/status.md`

"The Operations page acts (theseus-jid7t.6): the inspector's Live, Record and Standing sections, the Join pane (steer, queue, `/t`), the model and mode menus with the held model and its revert, the ⌘K verbs over a scope, the dock, the New task sheet, and the floor's table shared between the daemon's test and the cockpit's greying."

## 11. Follow-ups and choices the owner should hear

- **`job.stop` is missing** from the protocol; the job's Stop is off until a method exists.
- **`thread.asks`** (messages the router held before the page opened) is still a new-method follow-up.
- The held model's header chip reads the ledger (`session.profile_set`), which the cockpit reads a few seconds behind the daemon, so a choice made here shows at once from the page's own memory (`stickyWith`) and a newer ledger row wins. It compares the page's clock to the row's.
- "Context tokens" and a cache's warm/cold per task have no cheap read; the cache line is an estimate worded as one (active in the last 5 minutes). A thread's context tokens are `ThreadRow.context_tokens` (not shown beside the Join pane yet).
- `SessionDeck` keeps its own private `useLive`; `ops/live.ts`'s `liveOf` is the same reading as a pure function, and the deck can move to it.
- Watches are not on the board, so the palette's entities do not list them.
- The ⌘K dialog's ⇥ cycles the scope only on the Operations page (the shell checks the page's feed).
- Decision 2 (sticky) is as the owner decided; the "this turn only" choice is held in the page's memory and sent with the Join composer's next message.
