# CLOUD_REPORT: work-answer (theseus-ds5os)

Branch `cloud/20261013-work-answer`. Commit a08fdf6 (the step), then this report.

## What I found
- `answer_options()` worded every kind's answers but nothing routed them; ids for reminders already existed
  (`wakes::reminder_answers`: done, snooze_10m, snooze_1h, tomorrow_9), so I reused them rather than invent new ones.
- A `confirm.requested` push is serialized once for every connection, so a per-connection `may_answer` cannot ride it.
  `may_answer` is therefore `Option<bool>` on `ConfirmRequest`: set by `confirm.list` for the asking connection
  (optional `{discord}` origin params, so a shared channel's binding reads false), absent (read as yes) on a push.
  A refused answer still says where to answer instead.

## What I changed
- protocol `answer.rs`: `WorkAnswerParams/Result`, `ConfirmListParams`, `reminder_options()`, `approves()`,
  `ConfirmRequest::question()`. `ConfirmRequest` gains `options`, `may_answer` (and derives Default);
  `ConfirmResolved` gains `option`; method `work.answer`.
- core `rpc/answer.rs`: `work_answer`, `pick` (id resolution), `route`, `kind_of_action`, `may_answer`, `offer`
  (words a request's options at every site that builds one). Facts `CallAnswered`/`BudgetAnswered` carry the option on the
  ledger row and the event; `action.confirm` sends it too. `work.answer` is in the MCP `JUDGED` list, `ORDERED`, and the
  CLI's `OPERATORS` (refused inside a job).
- readers: `theseus answer ID OPTION [--note]` (crates/theseus/src/answer.rs), TUI answer keys over `work.answer`,
  cockpit `ConfirmCard` buttons from `options` (`lib/answers.ts`).
- No store change (no format bump); no new config key.

## Proof
- New tests: `tests_m3/answer.rs` (9: approval approve/decline+note/approve_trust, budget reset/decline, unknown option
  lists valid ones, shared place + MCP refused and ledgered, may_answer false for shared binding / unnamed, one answer
  clears session watcher and the all-session push with the option, Ask "not answerable yet", short ids incl. ambiguity
  naming both and under 4 chars, real end answers), plus layer-1 change accept/decline, extension ack/decline, and
  reminder done/snoozes (whole id and short end). CLI `tests/answer.rs` (4, incl. refused inside a job). TUI
  `tests_answer.rs` (3: budget `y` sends reset, may_answer false sends nothing, confirm.resolved clears the card).
  Cockpit `test/answers.test.ts` (5). The herdr interactive test's `confirm.resolved` now carries an option.
- Planted reverts: place check removed from `confirm_action_with` -> `the_place_rule_refuses_a_shared_place_and_the_mcp_surface`
  failed; `option` dropped from `confirm.resolved` -> `an_answer_from_one_surface_clears_every_other` failed. Both restored and touched.
- Full workspace suite (`nextest --workspace`): 4318 pass; failures are the known ~33 L1 tests (sandbox contract, spawn_100,
  theseusd sandbox) and `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`,
  which fails identically on the base (verified with `git stash`), unrelated (hold/ping timing). Fixed in this branch:
  my own race in the clears-everywhere test, and the TUI test needed a pending-empty `execution.changed`.
- Gate (`THESEUS_GATE_NO_BENCH=1`): door, keel, fmt, shape, features, clippy, cockpit, test build pass; suite fails only on
  the cases above. Then by hand: `cargo deny --offline check` ok, `theseus-sim bench turn --check` ok (5 frames), protocol
  types regenerated and committed, `keel-guard.py` ok (0 findings).

## Live check (not run here: needs the stand-in daemon setup)
On a scratch daemon (fresh state dir, stand-in model): make a task whose rule writes a file, and a session at its limit.
`theseus confirm` lists both; `theseus answer <last 4 of the write's id> approve` -> prints `answered act_… · approve`, and
`theseus status` no longer lists it; in `theseus tui` press `y` on the budget question (sends `reset`), `theseus status` clears.

## Notes for the owner / maintainer
- Divergence: `theseus confirm` still calls `action.confirm` (it follows the resumed turn); both reach `confirm_action_with`.
  `work.answer` accepts `watch` to do the same later.
- `approve_trust` is accepted for any call approval (the transcript is not read to know the session holds external text);
  the cards still offer it only when the request says so.
- Keel findings expected: none. The core output golden changed (options on `tool.confirm_requested`).
- Docs to update at review: spec Part III item, status.md; `wakes.rs` comment "a later work.answer will route them" is now true.
- Disk: this VM's allowance filled twice; I deleted `target/debug` (inside the repo).
