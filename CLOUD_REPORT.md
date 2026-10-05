# CLOUD_REPORT: the cockpit's Budgets, Ledger and Policy tabs (step 42b, theseus-ext.15)

Branch `cloud/20261005-cockpit-tabs`. Cockpit work only: no Rust changed, no protocol type changed (so
`cockpit/src/protocol.gen/` is untouched), no dependency added (Cargo.lock and both package-lock.json files are as they were).

## Step 1: Budgets, in Money (669ead4, and 24413ae for its place)

**Found.** Money's Budgets panel read `execution.list` and `action.list`. Nothing read `budget.list`.

**Changed.**
- `src/components/Budgets.tsx` (new), in place of the old panel: `budget.list`'s rows, each session's tasks indented under it with
  their carves ("its parent holds $X for it"), where each limit comes from (`limit_from`/`limit_by` in words), held, held unknown,
  available, lifetime, burn per hour, the last reset (opened per row, `?budget=<session>`), the questions waiting (the Actions
  view's own `ConfirmCard`, found in `confirm.list` by correlation id), the totals with the daemon's rule, the judge's day line,
  and the AWS hands from health with runaway said first and plainly. It is full width and sits above the river (24413ae),
  because below the 560 px river a waiting question was past a scroll.
- `src/lib/budgets.ts` (new, pure): `flatten`, `sums` (money figures add the top rows only; lifetime adds every row, as
  `budget.rs` states), `burnPerHour` (calls in the window `(end - 1h, end]`, for a set of sessions, scaled to an hour),
  `sessionsOf`, `recentResets` (`budget.reset` rows), `limitWords`, `handsLines`.
- `src/lib/rpc.ts`: `budget.list` and `policy.explain` join `PUSHED`; `bindPush` also reads again on `policy.tightened`,
  `policy.untightened` and `session.trusted` (it read nothing on them before).
- Money's range, measure and table are in the address (`?range=&measure=&table=1`); they were `useState`.
- Under the time machine the panel says "shows the present · its acts are off" and a question is text, not buttons.

**Proved.**
- `npm test`: 6 new tests in `test/budgets.test.ts` (the whole suite 75 pass, 0 fail).
- Planted reverts, each restored and `touch`ed: totals over every row (`flatten(rows)` in `sums`) fails
  `the totals are the top rows: a task is never counted twice` (expected 3.5, actual 4.5); burn over all time (window test
  removed) fails `burn per hour counts the window before the end, and the sessions asked for`.
- A scratch daemon of the build (`fake-model` with a rule calling `task.create` twice, quotes over 20 characters): `/money`
  showed the session with its two tasks and their carves; its totals (3 open, 2 tasks, spent, lifetime) matched
  `theseus budgets`; a later `theseus ask hello` added a line and moved the totals with no reload; no console or page error. A
  second daemon with `[kernel] spend_limit_usd = 0.0006` raised a budget question, which showed as its card with
  "Reset to $0 and continue" / "Keep waiting".

## Step 2: Ledger (33c0ae1)

**Found.** `useLedger(20_000, 5000, ...)` polled `ledger.tail` for the newest 20,000 rows every 5 s, a second copy of the ledger.

**Changed.**
- `Ledger.tsx` reads `useHistoryRows()`, the whole ledger, followed; the poll is gone. The history says when it is partial
  (an older daemon) and while it reads.
- `src/lib/ledgerview.ts` (new, pure): `filterOf`/`filterRows` (the filter the list uses, newest first) and `exportOf`
  (`filterRows` then JSON), so the export is by construction the rows shown. Saved filters: `queryOf` (the address's filter
  keys only: `q kind family session from to`), `applySaved`, `withSaved`/`withoutSaved`, and `readSaved`/`writeSaved`
  for `localStorage` (`theseus.ledger.filters`, tolerant of junk; every access in try/catch). `newerThan` counts rows that
  landed while follow was paused.
- The time brush (`?from=&to=`), the row picked (`?row=`) and follow (`?follow=1`) are in the address too.
- Follow keeps the list at its top (it is newest first) as rows land; scrolling away pauses it and shows "N newer · follow
  paused · jump to the newest". Export is a Blob download made in the page; the button says how many rows.
- The histogram did `Math.min(...rows.map(...))`, which throws on the whole ledger (over about 120k rows): now a loop.
- Under the time machine it says "shows the present" (it has no acts).

**Proved.**
- 5 new tests in `test/ledgerview.test.ts`: the filter, export equal to shown, the saved-filter round trip (query to storage
  text to address and back), junk storage, follow's count.
- Planted revert: `exportOf` of every row (`[...rows].reverse()`) fails
  `the export is exactly the rows shown, in the order shown`; restored and `touch`ed.
- Headless Chrome on a scratch daemon: a filter named and saved, the page reloaded, the chip still there and applying the
  filter to the address; the export held 5 rows, all `startup.*`, the button said "export 5"; follow wrote `follow=1`.

## Step 3: Policy (1d3427e)

**Found.** No view read `policy.explain`. The tightenings list and its undo lived inside Boundaries' `Gate`.

**Changed.**
- `src/views/Policy.tsx` (new, `/policy`, in the nav with a gavel, and `g` then `p`): `policy.explain` for the CLI and each
  bound place (class, ceiling, hold), or `?session=` (a select of sessions) for one; each tool's result and the layers that
  raised it; a tool opens its layers in the gate's order and its conditions with their entries (`?tool=`); `?place=`
  (places opened), `?q=` and `?result=` filter. The tightenings (with their undo, confirmed first, off under the time
  machine) and a link to Systems' approval channels.
- `src/components/Tightenings.tsx` (new): Boundaries' list, now shared. Boundaries uses it (its undo behaves as before).
- `src/lib/policyview.ts` (new, pure): `toolLine`, `placeSummary`, `sortTools` (the strictest first), `filterTools`, `familyOf`.
- `cockpit/AGENTS.md`: the views count, the new modules.

**A quirk of 42a's read the owner should hear about.** In `explain.rs`, the `tightening` layer's `raised` is
`after.posture > config_posture`, the whole decision's, not the tightening's. With no workspace root configured (my scratch
daemons), or for any call that an approve list or the outside-the-roots condition raises, every tool shows its
"tightening" layer as raised ("not tightened", raised). The view therefore counts a tightening only when the layer names who
tightened (its `setting` is set), and tests it (`a tightening layer counts only when it names who tightened`). The right
fix is in `explain.rs` (set `raised` from the tightening alone); I left Rust alone as asked.

**Proved.**
- 5 new tests in `test/policyview.test.ts`.
- On a scratch daemon, headless Chrome: `theseus policy tighten proc.run` showed the tightening on `/policy`; its Undo (the
  confirm accepted) removed it; a `tighten`/`untighten` from the CLI appeared and went with no reload (the push); 7 layers
  and the conditions showed for an opened `proc.run`; `?session=<id>` showed the session's own place, "trusted: no
  outside text held". No console or page error.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the tree as committed (24413ae): fmt, shape, features, clippy,
the cockpit (lint, 75 node tests, build) and the registry pass. The suite: 2486 tests, 2452 pass, 34 fail, all L1 on this
root VM except one that is flaky here:
- 33 known: `theseus-sandbox::contract` (clauses 06 to 12, exit status, sigterm, scratch, three egress ones, and the rest),
  `theseus-sandbox::bench spawn_100`, and `theseusd::sandbox` (11 tests).
- `theseusd::bench_profile theseusd_check_passes_on_the_bench_profile_with_no_vault`: failed in the second gate run with
  "L1: the self-test failed: checking the job: a workspace root, /, is the root", and passed in the first gate run and alone
  (`cargo nextest run -p theseusd -E 'test(theseusd_check_passes_on_the_bench_profile_with_no_vault)'`, 1 passed). It is
  the root-VM L1 self-test, not a cockpit matter.
- `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` was flaky (passed on retry), as the flaky list allows.
The suite's failures stop the gate there, so I ran the phases after it by hand: protocol types (no change under
`cockpit/src/protocol.gen`), the turn bench (5 frames plain, 9 tool, within budget), and `cargo deny check` (advisories,
bans, licences, sources ok; it warns of one unmatched licence allowance, `Unicode-DFS-2016`). The lifecycle and jobs
benches are skipped by `THESEUS_GATE_NO_BENCH`. The golden test passed under `TZ=America/Phoenix`.
The first gate ran on the tree before 24413ae (Money's panel moved up) and a small fix in `policyview.ts` made while it ran;
the second gate ran on the final tree.

## The live check for the maintainer (a browser)

Build first: `cd cockpit && npm ci && npm run build`, then `cargo build -p theseusd -p theseus -p theseus-sim`.
Then the rig of 42a: `target/debug/theseus-sim discord rig --dir /tmp/rig42b` (it prints how to start the fake Discord, the
model stand-in and the daemon; give the bindings file's channel `posture_floor = "approve"`, the daemon `[web]` enabled on
7435, and the stand-in model a rule that calls `task.create` twice, each `arrangement` quoting at least 20 characters
of the message). A simpler rig that I used: `theseus-sim fake-model --rules rules.json`, a config with `[model] api_base =
"http://127.0.0.1:9448"`, `[web] port = 7435`, `[secrets] anthropic_api_key = "env:K"`, and
`theseusd --config c.toml --socket s --state-dir d`; rules: first
`{"when":"errand please","calls":[{"name":"proc_run","input":{"argv":["sleep","300"]}}],"text":"x"}` (so tasks stay open
behind `[policy.tools] "proc.run" = "approve"`), then `{"when":"please start two errands now","calls":[{"name":"task_create",
"input":{"brief":"first errand please","arrangement":{"pieces":[{"quote":"please start two errands now","role":"objective"}]}}},
{"name":"task_create","input":{"brief":"second errand please","arrangement":{"pieces":[{"quote":"please start two errands now","role":"objective"}]}}}],"text":"ok"}`.
1. `/money`: `theseus ask "please start two errands now"`. Budgets shows the session with its two tasks and their carves; its
   totals (open, tasks, limits, spent, held, available, lifetime) equal `theseus --json budgets`' `totals`. `theseus ask hello`:
   a new line and the moved totals appear with no reload. With `[kernel] spend_limit_usd = 0.0006` the first ask raises a
   budget question: its card shows in the panel, and "Keep waiting" leaves it.
2. `/policy`: `theseus policy tighten proc.run`: proc.run's row for the CLI and each place shows "raised by tightening",
   and the tightening is listed at the right; its Undo (confirm) lowers it, with no reload; for the floored channel the
   tools show the floor. `/policy?session=<id>` explains one session; `/policy?tool=proc.run` opens its layers.
3. `/ledger`: name a filter ("name this filter", Enter), reload, click it. Turn on follow, run `theseus ask hello`: the newest
   rows land at the top; scroll down and the "N newer" pill shows and holds the list. "export N" downloads N rows equal to the
   list.
4. Each view loads with no console or page error (`/money /policy /ledger /boundaries /systems /ship` were clean here).

## Left, and for the owner

- Docs for the maintainer to write: `docs/spec` Part III's 42b item; `docs/status.md`; `docs/design/m7-surface.md` §2.6 still
  says `web/`, a table of sessions, and "approval channels and trusted users, moved here": the Policy view links to Systems'
  approval channels rather than moving them (as the brief says).
- `ConfirmCard`'s buttons answer without a `window.confirm` (the card is the Actions view's own); the Budgets panel reuses it
  as the brief says, so a budget reset is one click there, as in Actions.
- The burn per hour is the last hour's calls scaled to an hour, a parent's including its tasks' calls. It moves on a 10 s tick.
- The Ledger now holds the whole ledger in the list; the KindMap and the histogram take all rows. On a very large ledger a
  keystroke in search filters the whole of it (deferred, so typing does not wait); I did not measure beyond the scratch daemons' few hundred rows.
- `Boundaries`' undo is unchanged (live under the time machine, as before); only `/policy`'s is off in the past.
- The merge with 39b: this branch touches `lib/rpc.ts`'s `PUSHED` set and `bindPush`'s one `if`; keep both sides' entries.
