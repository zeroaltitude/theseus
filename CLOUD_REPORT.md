# CLOUD_REPORT: self-budget (theseus-pw1q.3)

Branch `cloud/20261013-self-budget`. Work commit `7636998`.

## What I found
- A self task's session is a task session whose task record (`g::get(store, of_session(sid))`) holds `origin.principal`.
  The branch key is the **root task's id** (a subtask spends on its root's branch, so subtasks cannot evade the $10 line).
  self-pr can alias it to `self/<date>-<slug>` later; the account is keyed by whatever string `reserve` is given.
- `turn.rs` +11 lines (limit was 29): a field on `TurnRunner`, a field on `Turn`, the reserve call in `call_model`, the settle call. Logic is in `turn/self_budget_step.rs`.

## What I changed
- `[self.budget] branch_usd = 10.0`, `day_usd = 30.0` (config/rsi.rs, template lines, template test).
- `rsi/budget.rs`: `Budget::reserve` / `settle` / `view` / `branch_of`; `rsi::SELF_PRINCIPAL`. One META record `self.budget`, written in the same frame as each row. The first use after a start settles an open hold at its worst case (`recovered: true` row). Reading (`self log`, health) counts an open hold as spent and writes nothing. The day follows the local day (`judge::spend::local_day`).
- Rows `self.budget.reserved/settled/stopped` (`fact/rsi_budget.rs`, three `LedgerKind`s). `stopped` is written once per branch and line.
- Seam S2: `turn/self_budget_step.rs`. At a line the call is not planned, the turn fails with class `self_budget` and the budget's words (the reply it was serving); the spend so far is kept. Settle runs right after `call_model` returns, whatever the outcome; a fault or an unknown cost settles at the worst case.
- Surfaces: `SelfLogResult.budget` (`SelfBudgetView`, `SelfBranchSpend`), `SelfState.day_spent_usd/day_line_usd` (all optional; TS regenerated). `theseus self log` prints the day and each branch; health's `self` line appends `· $X of $30.00 today`.
- No store format bump: a new META key and new ledger kinds only.

## Proof
- `rsi/tests_budget.rs` (7): branch stops at $10 with work kept; day stops at $30 across branches; hold counts until it settles; over-line refused before any spend; restart keeps the day and settles an open hold at worst case; new day resets the day only; mode off writes nothing.
- `tests_self_budget.rs` (4, through whole turns): a self turn's call is counted (reserve, settle at the turn's cost) and an ordinary task's and a conversation's are not; a subtask spends on its root; a line stops the call with no request sent; mode off spends nothing, no row.
- CLI render test (day line, branch lines); config test.
- Planted reverts: (1) turn hook switched off: all 4 turn tests failed; (2) day-line check disabled: `the_day_stops_across_two_branches` failed. Both restored and `touch`ed.
- `cargo nextest` of the touched sets: 48/48, protocol 85 passed, frame-budget/golden/registry/layouts 34 passed. `cargo deny check` ok. `keel-guard.py`: 0 findings.

## Live check for the maintainer
```
theseusd --config <scratch> --socket S --state-dir <fresh>   # with [self] mode = "act"
theseus --socket S self resume && theseus --socket S self log   # shows "Budget <day>: $0.00 of $30.00 ..."
theseus --socket S health | grep '^self:'                      # "... · $0.00 of $30.00 today"
```
A self task needs self-backlog/self-pr to open one; until then the unit and core tests above are the proof.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit, test build all passed.
Suite: only the known failures: theseus-sandbox contract tests and theseusd `sandbox` tests (root daemon, theseus-pv6i). Two others:
- `theseus-core tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory`: failed in the full run, passed alone (load).
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: fails alone, and fails the same on the base with my changes stashed. Not mine, not on the known list; worth a look.
Later phases run by hand: protocol types, deny: ok. Benches skipped (NO_BENCH). An earlier gate run hit "no space left" and was rerun after clearing `target/debug/incremental`.

## Keel
Keel findings expected: none. New caps: `[self.budget] branch_usd = 10.0` and `day_usd = 30.0` (code defaults in `config/rsi.rs`; the owner may want keel-guard to watch them).

## Left / uncertain
- The budget does not itself check the kill switch (the switch lives in `Core`, not the turn runner); the other rows call `rsi::gate`. Mode off is enforced here.
- No separate `self.budget` reads are paced: `view()` costs one META read per health call when nothing has used the budget since the start.
- The pack writer, consolidation and replay caps are untouched; callers that spend outside a turn must call `reserve`/`settle` themselves, and must not also reserve for spend inside a self turn.
- `crates/theseus-core/AGENTS.md` updated; spec/status docs for the maintainer: Part III item for pw1q.3, `docs/status.md` row.
