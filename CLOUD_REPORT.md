# Cloud report: everything the Observatory shows, in the cockpit (theseus-vm3n.6)

Branch `cloud/20261003-obs-parity`. `web/` and `crates/` are untouched (`git diff a59b7c1 -- web crates` is empty); only `cockpit/` changed.

## Commits

| commit | what |
|---|---|
| 1de1451 | ledger rows in words: every kind the Observatory said (`summary.ts`, new `figures.ts`, `test/summary.test.ts`) |
| 72c3f14 | shared `ConfirmCard` (preview, budget wording, approve+trust); Actions: lifecycle, postures, wakes, holds, `?execution=` |
| a90f346 | session deck: asks inline, tool-card gates, live thinking/tools, turn failure, composer queue, context tab, trace summary |
| 69c7a98 | Fleet executions table; Systems (push, AWS, store/crash, phases, secrets, Discord, approval rows, catalog); shell (need-you, provider errors, river); Ledger `?session=` + should-have-asked; caching by profile / per session |

## The inventory

"Obs" = file:line in `web/src`. "Cockpit" = where it shows now (file in `cockpit/src`). **gap-left** rows are listed again below with the reason. Protocol data in backticks.

### Shell (App.tsx)

| element | Obs | cockpit |
|---|---|---|
| brand, version `health.version` | App 338-339 | Shell.tsx HeartbeatBar (wordmark, version, protocol tooltip) |
| sidebar toggle, session list (title, ago, turns, tools, cost, task limit, attention pill, nesting under parent, `+ new`) | App 336, Sessions 195-244 | Fleet.tsx table (state pill, title, ids, turns, tools, tokens, cache, cost, budget, active, waiting) + FleetGraph (task nesting, reports). New session: Fleet `NewSession`. Task "of $limit" shows in Actions Tasks. |
| live profile picker `profile.list/use` | App 342-351 | Systems.tsx Profiles "make live" (confirmed). One click further than the Observatory's header: partial, left (see below) |
| connection status (reconnecting) | App 352 | Shell link indicator + nav dot |
| "N need you", opens longest waiting `attention` | App 353-358 | **ported** Shell.tsx HeartbeatBar chip. The nav rail's Actions badge (confirm count) already existed |
| totals: sessions, turns | App 361-362 | Systems Daemon card |
| totals: spent, catalog version | App 363 | bar "spent"; Systems catalog title |
| totals: provider errors | App 364 | **ported** bar indicator (only when >0) + Systems Daemon |
| totals: turns held / ceiling | App 365 | **ported** Systems Daemon (Kernel card had it) |
| totals: usage in/out/cache r/w | App 366, 20-28 | **ported** Systems Daemon (tokens in·out, cache read·written) |
| pane toggle observatory/narrative | App 367-374, 419-429 | n/a: views and the river replace the pane |
| session bar (title, turns, tool calls, cost, profile→model, execution state, id) | App 381-391 | SessionDeck Header |
| load error, session gone | App 392, 135-140 | SessionDeck "no session x" (partial, minor) |
| empty states (no messages; pre-M3 turns) | App 393-403 | **ported** SessionDeck Empty |
| draft bubble, "waiting for admission" | App 408-416 | partial gap: the user node lands via push; no pre-admission bubble (left, minor) |
| composer; "Send (queues)" | App 432-451 | Composer.tsx; **ported** send while a turn runs (queues), failed-turn class. Session usage line: SessionDeck Header stats |
| push: `executions.watch`, `execution.changed`, `events.lost` | App 83-121 | rpc.ts `bindPush` (same) |
| per-session `session.watch` live | App 148 | `useSessionWatch` |
| approvals: `action.confirm` incl. note, trust | App 277 | ConfirmCard (Actions, **ported** into SessionDeck) |
| should have asked `policy.tighten` | App 285 | **ported** ShouldHaveAsked (card, ledger row); Actions postures had a no-call version |
| graduate `label.graduate` | App 293 | gap-left (labels) |
| trace loader `ledger.tail turn.trace` | App 299 | SessionDeck Timeline (Flame, replay) |

### Transcript (Transcript.tsx, TraceView.tsx)

| element | Obs | cockpit |
|---|---|---|
| user prompt, author/time hover | 444 | Transcript.tsx UserItem |
| label badge `node.label` | 197-213, 444, 465 | gap-left (labels) |
| graduate button `node.withheld` | 218-245 | gap-left (graduation) |
| continuation divider, background results, "waits for the call before it" | 446-460, 474 | partial: late/background pills on the call card (existing); no divider (left, minor) |
| thinking (collapsed, chars) | 467 | AssistantItem (existing) |
| live thinking, live text, running tools with seconds | 483-489, 360 | **ported** `useLive` + LiveItem (push events only) |
| assistant text | 468 | markdown (existing) |
| tool card: name, per-tool summary | 354, 63-82 | **ported** `toolwords.callSummary` (plan summary kept first) |
| gate pill + reason | 355 | existing, **ported** reason tooltip |
| notified pill; secret grant; L1 pill + egress | 356-358 | **ported** |
| should have asked / asks first now | 359, 304-328 | **ported** ShouldHaveAsked |
| running N s | 360 | **ported** (live) |
| input JSON, policy reason | 362-367 | existing |
| inline confirm card (note, approve, approve+trust, decline, expiry, bound args, id) | 119-151 | **ported**: asks panel in SessionDeck, `waits for you` pill on the card |
| edit/write/patch/proc preview as diff | 101-117 | **ported** `previewOf` in ConfirmCard (Actions too) |
| floor pill; external-text pill | 132-134 | existing in Actions, now both places |
| budget card (reset to $0 / keep waiting) | 156-180 | **ported** wording and buttons in ConfirmCard |
| held post card | 249-270 | gap-left (held posts) |
| result: status, not run, stopped by, late, exit, ms, bytes, truncated(+ref), diff drawing | 277-302 | **ported** exit, size, stopped-by, not run, diff drawing, full_ref |
| result expanded text | 299 | existing |
| turn footer: profile→provider/model, loops, tools, stop reason, usage, cost, elapsed, first token | 498-527 | turn header: elapsed, loops, tools, cost existing; **ported** first token, model, stop reason. Per-turn token usage: Spend tab / ModelInspector (partial) |
| context compile chip per turn (decision, trigger, msgs, tokens) | 514-519 | partial: Context tab compile log (**ported**); withheld part gap-left (labels) |
| turn failed: class, transient/permanent, usage unknown, message | 490-497 | **ported** TurnFailed (class + error from the ledger row; transient/usage-unknown flags are not in the `turn.failed` row, only in the submit error, which the Composer now shows) |
| timing tree: waterfall, by-kind summary, span attrs | TraceView | Flame + replay + attrs (existing); **ported** by-kind summary |

### Observatory panels (Observatory.tsx)

| element | Obs | cockpit |
|---|---|---|
| header: live/pause, refresh, error | 314-320 | n/a (push + polls, no pause); intentional |
| Context: explainer | 324-327 | n/a prose |
| context files counts, persona line `health.context` | 328-335 | Systems Context files card (lists files, persona); counts not shown (cosmetic) |
| cache table: this session / per profile (input, read share, written, saved) | 336-362 | **ported** Economics "Caching · by profile"; session numbers in deck Spend tab |
| recompile fresh/transcript + note | 363-368, 174 | SessionDeck Recompile menu (existing) |
| compile log table (when, loop, decision, prefix+tail, msgs, tokens, repairs, digest + tooltip) | 370-392 | **ported** deck Context tab |
| compilations table (id, trigger, strategy, as of, prefix nodes, model, thinking, session; tooltip) | 393-415 | deck lineage (existing) + **ported** thinking kept/stripped; manifest in expand |
| context files of current compilation (path, level, digest, bytes, missing, cut) | 416-429 | **ported** deck Context tab (readers/withheld columns: gap-left, labels) |
| Tools: roots, counts, shell-fallback | 434-437 | **ported** header of Actions "Tool postures" |
| Tools table: class, backend, posture + why tooltip, calls, description, schema | 438-455 | Actions Postures (existing) + **ported** setting/config tooltip, schema tooltip, `approve` tone |
| tightenings: since, by, via, call, session, undo | 462-478 | **ported** inline detail (since, by, via, call), undo existing; session not shown (minor) |
| policy note after tighten/untighten | 461 | alert on error only (minor) |
| Kernel: accepting, ceiling, exec/action by state, quarantined (+why), lingering | 486-497 | Systems Kernel (existing, **ported** quarantined tooltip) |
| children (running, lingering, orphans, zombies, reaped, subreaper) | 498-506 | Systems Children (existing) |
| secret broker grants | 507-514 | Systems Broker grants (existing) |
| ledger rows / uptime | 515 | Systems Daemon (existing) |
| push line | 516-526 | **ported** Systems Kernel "push" |
| disk / spool | 527 | Systems DiskSpoolCard (existing) |
| AWS accounts | 528 | **ported** Systems AwsCard |
| last startup steps (kernel) | 529-543 | Systems Kernel startup (existing) |
| Startup: serving, between phases, config line, secrets line + failures + retry, store corrupt/refused, crash | 548-1061 | **ported** PhasesCard, StoreCard, secrets/config extras; phase bars: `Startup` chart (existing) |
| phase "found" column | 957-975 | **ported** `phaseOutcome` (tested) |
| Discord: state/detail, bot/guild, bindings+revision, members intent, connected/heartbeat, traffic counters, last error, outbox (+last error) | 553-576 | Systems binding card (existing) + **ported** bindings, intent, outbox error |
| Discord places table (kind, channel, session link, users, last message) | 577-593 | **ported** |
| Discord recent traffic rows | 594-607 | **ported** |
| Approval: trusted users, channels table | 619-636 | Systems Approval (existing) |
| Approval recent rows | 637-650 | **ported** |
| Wakes: id, due (+until/overdue), session link + state, note, cancel, tooltip | 655-673 | Actions Wakes (existing) + **ported** link, state, overdue wording, tooltip |
| External text: session link, since, read (query or url), how, trust | 675-692 | Actions Holds (existing) + **ported** how, query, tooltip |
| Sandbox: probe, jobs L0/L1, default, always-L1 argv, cgroup, egress list, counters | 694-697, Sandbox.tsx | partial in Boundaries (`sandboxLine`, Egress panel show limits, egress list, counters); probe result/why, L0 count, default level, always-L1 argv, ro_paths skipped: **gap-left** (Boundaries) |
| Executions: kind, state, interrupted, turns, outstanding, queued, budget parts, at-limit, updated, cancel, row filter | 699-739 | **ported** Fleet `?view=executions`; filter = links to Actions `?execution=` and Ledger `?session=` |
| Actions: columns, tooltip ids, overdue, seen, resolution/req, reserved, cancel | 741-771 | Actions Lifecycle (existing) + **ported** overdue, seen, resolution, req, reserved, tooltip |
| Ledger: family chips | 774-777 | Ledger treemap by kind (existing; kind-exact rather than prefix) |
| Ledger: this tab's session | 778 | **ported** `?session=` chip |
| Ledger row: position, time, kind, ids, summary, expand JSON | 782-791 | Ledger RowList + detail (existing); words **ported** (summary.ts) |
| should have asked on a notice row | 789 | **ported** in the row detail |
| Nodes: kind filter, loop idx, summary, JSON, reach | 796-816 | partial: deck Graph tab (per-session nodes, JSON, reach existing). Cross-session node list and kind chips: gap-left (low value, see below) |
| Model catalog: model, provider, window, max out, $in, $out, cache r/w, thinking, profiles, tooltip | 818-842 | Systems Catalog (existing) + **ported** provider, max out, thinking, tooltip |
| Sessions table (title, last active, turns, tools, cost, in/out, execution, waiting) | 844-864 | Fleet (existing; tokens-out not a column: minor) |
| Narrative: lines, part, session link, this session, follow | Narrative.tsx | Shell ActivityRiver (existing) + **ported** session link, this-session filter |
| DiskSpool | DiskSpool.tsx | components/DiskSpool.tsx (existing, richer) |
| AwsAccount | AwsAccount.tsx | **ported** |
| Sandbox | Sandbox.tsx | see Sandbox row |

## Gaps left

Left alone as told (another change replaces that model): the label badge on nodes and turns; graduate buttons and `label.graduate`; the held-post card (`label.release`: the session deck filters it out of its asks, Actions still lists it as a generic card as before); the label/withheld parts of the compile chip, and the context file `readers`/`withheld` columns; the Sandbox section's probe line (works / not available + why, start ms, ro_paths skipped), L0/L1 job counts, default level, always-L1 argv, and cgroup string, because the cockpit's place for it is the Boundaries view (`sandboxLine`). Its limits, egress list and counters are there already.

Left, minor or deliberately different: live profile picker in the header (Systems "make live"); pre-admission draft bubble; continuation divider; session link on tightenings; policy note after tighten/untighten; per-turn token usage in the turn header (Spend tab has it); sessions table's tokens-out column; context-files count line; Observatory's cross-session Nodes list and its kind chips; Observatory's pause/refresh (cockpit follows the push and polls). The ledger's kind filter stays per exact kind.

## Shown by the cockpit that the Observatory never did

Ship (3D fleet), time machine, Boundaries, money river, Economics (spend over time, sunburst, latency), speed wall, call and model inspectors, flame replay with an at-instant readout, session graph, Fleet graph, Bridge KPIs and rate-limit fuel, command palette, desktop notices, RPC console, health `index`/`labels` blocks.

## Proof

- `cd cockpit && npm run lint && npm test && npm run build`: lint 0 errors (only the existing `only-export-components` warnings), **20 tests pass** (5 existing + 15 new: summary, toolwords, startupwords, money), build ok.
- Planted revert: renaming the `budget.asked` case in `summary.ts` failed `a budget question, a reset, and a changed limit read in dollars` (9 pass, 1 fail); restored, touched, 10 pass.
- Size: no cockpit size budget exists in the gate. Chunks, before → after (raw): SessionDeck 53.85 → 64.12 kB, Actions 15.98 → 15.08 (ConfirmCard moved to the shared chunk), Fleet 12.86 → 18.49, Systems 18.09 → 30.10, Ledger 32.75 → 33.34, Economics 10.50 → 12.59; index 365.7 → 372.5 kB, ui 1,072 kB unchanged. No new dependency; `package-lock.json` untouched. `ses_synth` is not in the dist.
- By eye: headless Chromium (global Playwright 1.56, `--no-sandbox`) against the vite dev page and a **scratch daemon** (debug build of this tree, state in the scratchpad, scripted fake Messages API, Discord and index off, no real secrets). Seeded: a plain session, two waiting approvals (`fs.write`, `fs.edit`), a `proc.run sleep` that was approved then `/stop`ped (cancelled), and slow streamed turns. Every page loaded with no console or page errors. Seen: the deck with the inline "waiting for you" card and its diff preview, a "waits for you" pill, first token / model / stop reason in the turn header, the timeline's by-kind line; a live turn showing streamed thinking and text in the streaming card; the stopped session's result (size, background pill); the Context tab's compile log; Actions with two cards (write preview, edit diff) and the lifecycle list; Fleet executions with cancel, budget bars and the "2 NEED YOU" chip; Systems with the new Store, Start-phases, push, secrets-failure (the scratch config's AWS keys fail to resolve, shown with their errors), catalog columns. A nit found by eye and fixed: nowrap on the phase times. **Not seen by eye**: the AWS card (no account bound), Discord places/traffic and approval rows (Discord off, no `[approval]`), the context-files table (no context files in the scratch config), the Economics caching table and Ledger `?session=` chip (rendered without errors; screenshots taken but not inspected) and a failed turn's error block and the L1/notified/granted pills (no such call was run). These have type checks and, where pure, unit tests (`startupwords`, `money`, `toolwords`, `summary`) only.
- Live tool seconds ("running N s…") and the failed-turn block rely on push events and the `turn.failed` row's `error`/`class` fields; neither was exercised by eye.

## The gate

Only `cockpit/` changed, so I ran the gate's cockpit phase by hand (lint, test, build: green) and not the Rust phases: `cargo-nextest`, `cargo-deny` and the full `scripts/gate.sh` were not run here (setup time went to the inventory). `cargo build -p theseusd -p theseus -p theseus-index -p theseus-sim` succeeded on this tree, and no Rust or `web/` file differs from main. The known-failure list above was therefore not exercised. The maintainer should run the whole gate on the owner's machine.

## Docs the maintainer may want to change

`cockpit/AGENTS.md` ("What's here"): `src/lib/figures.ts` (pure figures; `format.ts` re-exports), `summary.ts`, `toolwords.ts`, `startupwords.ts`, `money.ts`'s net saving, all tested by `test/*.test.ts`; `components/ConfirmCard.tsx`, `ExecutionTable.tsx`, `ShouldHaveAsked.tsx`, `SystemsCards.tsx`. The "Observatory retirement" step can use the inventory above as its checklist.
