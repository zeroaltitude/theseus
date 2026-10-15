# CLOUD_REPORT: card-proposals (theseus-slvl.6)

Branch `cloud/20261013-card-proposals`, based on main `47c4e91c`. Work commit `b8ed8b3b`.

## What I found
- Closest precedents: `threads_import` (one Haiku call, one tool, scrubbed input, cited lines, a day line in META) and
  `judge/people` (budgeted sweep). The job copies their shape. Real sensitivity labels are `personal`,
  `company-confidential`, `partner-confidential`, `public` (the brief says `company`).
- `lib.rs` of the protocol had 4 spare lines: the new module uses 2 (`pub mod card;`, `pub use card::*;`); method
  constants went on the existing long constants line, TS types on the existing `ts.rs` line.
- `rpc::server::RpcFailure` and `confirms` are private to `rpc`, so the methods that judge an act live in
  `rpc/card.rs`; `card/` holds the job and the reads (anyhow only).

## What I changed (all in b8ed8b3b)
- `card/` (core): `evidence` (labels decide; partner-confidential is checked from the session record before any node
  is read; outside text never; conversations give operator text, notes operator+agent), `cluster` (pure; inverted-index
  near-duplicate join, rank by distinct sources then recency, `fresh` = most episodes unseen), `ask` (request, one tool
  `state_cards`, parse: must cite an episode its cluster showed, no pasted line), `run` (`card_run`: <= 8 calls of 8
  clusters, each reserved on `[card] day_usd` and the day ceiling first, one frame per call), `answer`, `state`, `tender`.
- Wire: `theseus_protocol::card`, methods `card.propose|list|answer|done`, ledger kinds `card.proposed` (a spend kind,
  added to `day_ceiling::SPEND_KINDS`) and `card.answered`.
- `[card]` config (`profile`, `day_usd` 2.0, `max_open` 30, `ask_days` 14, `every_days` 7) with template and tests.
- CLI `theseus card [list|propose --dry-run|keep|edit|decline|done]`, writes in `OPERATORS`; cockpit Context "Card" tab.
- Daemon start calls `card_after_serving` (a spawned timer task; off with `day_usd` or `every_days` 0).

## Design choices the owner should hear about
- **Proposals are queue rows, not parked `owner_ask` executions.** They carry owner-ask's words and options
  (`[Keep] [Edit] [Not this]`, default Not this at expiry) but thirty parked executions for a fortnight would be heavy.
  They are not on the work board yet; surfacing them as `QuestionKind::Ask` rows is the follow-up if wanted.
- Expiry is applied when the card is read or the job runs (no timer).
- Sent clusters are remembered (`card.sent.<call>`): a cluster the model found nothing durable in is not re-sent.
  So "declined never returns" is held by this plus the proposal sources plus a word-overlap check on the statement.
- Edit gives only the words (kind/scope are the proposal's). Discord modal not done (module has no room; CLI and cockpit only).
- No store format bump: META keys and two ledger kinds. Header cost: zero tokens (nothing is added to any prompt until
  the owner keeps a record, and then it is standing's existing block).

## Proof
- `cargo nextest -p theseus-core -E 'test(card::) | test(config::tests_card)'`: 38 passed (incl. committed-fixture test).
  CLI unit tests; protocol 149 passed (TS regenerated); cockpit `npm test` 422 pass (incl. 5 new), lint clean.
- Real daemon test `crates/theseusd/tests/card.rs` (real CLI + daemon + stand-in model, fixture archive): dry run asks
  nothing; run proposes 3; model request holds no partner/outside/chatter text; keep one, edit one, decline one; standing
  records have source `card:<id>`; a rerun proposes 0 and makes no call; a job shell is refused. Passes.
- Planted reverts (restored and touched each time; `git status` clean after):
  - partner check removed -> `labels_are_respected_before_any_call` fails ("KESTREL-TERM-SHEET-CLAUSE reached the model");
  - outside-integrity accepted -> same test fails;
  - sources dropped from a proposal -> `a_kept_proposal_writes_the_record_with_its_sources` and `a_run_proposes_cited_questions` fail;
  - standing `source` not `card:<id>` -> kept test fails;
  - sent episodes forgotten -> `a_second_run_proposes_nothing_new` fails (a call is made);
  - budget check removed -> `the_budget_stops_the_job`, `the_days_spend_carries_across_runs` fail;
  - cap removed -> `the_queue_is_capped` fails (panic on the full queue);
  - expiry never due -> `an_unanswered_question_expires_to_not_this` fails;
  - live-record/declined text check removed -> `a_live_record_is_not_proposed_again` and `a_declined_proposal_does_not_come_back` fail;
  - removing only the declined proposals' *sources* from `seen` did NOT fail the declined test (the sent set also holds
    them); the text check is what that test guards, shown by the previous line.
- FAST: `theseus-sim bench turn --check`: plain turn 4 frames, tool turn 4 per loop, syncs 2: unchanged; no turn-path code touched.
  `bench lifecycle --runs 10 --check` on this debug 4-core VM: MISSED on three rows (cold start p95 84 ms from a single
  outlier with p50 29 ms; push seed p95 2.3 vs 2 ms; health p95 7.8 vs 5 ms): this is the VM, the budgets are the owner's machine's; startup work added is one spawned task.

## Live check for the maintainer (owner's keys)
```
theseus import openclaw crates/theseus-core/tests/fixtures/card/archive.jsonl   # on a scratch daemon only
theseus card propose --dry-run     # evidence 6, 1 withheld, clusters 5; at most ~$0.01
theseus card propose               # needs a real [card] profile; 3-ish questions
theseus card; theseus card keep ID; theseus card edit ID words...; theseus card decline ID
theseus card propose               # 0 calls, 0 proposed
```
The real run on the owner's archive (21,779 episodes, ~1,549 decision episodes): `theseus card propose --dry-run`
first; expect at most 8 Haiku calls of about 2-3k input tokens each, so well under $0.10, far under the $2.00 day line.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (debug info off, incremental off, to fit this VM's disk):
door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule green. Suite: 70 failures: 68 theseusd::sandbox
and theseus-sandbox L1 tests (known, root VM), plus `tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future`
(passed alone, 1.6 s) and `tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory` (failed once
in the suite, passed 6 of 6 alone: same-millisecond records ordered by random id; not touched by this row; worth a ticket).
The later phases I ran by hand: protocol types (protocol tests), turn bench (ok), lifecycle bench (misses above, VM). Jobs and size phases not run.
Keel: `python3 scripts/keel-guard.py` reports `ok, 0 findings` against the branch base. (Before I fetched origin/main it
judged 393 commits of main and failed on unverifiable ack signatures: a stale-ref artifact of this clone.)

## Keel findings expected
None.

## Docs to change at review
Spec Part III item, `docs/status.md` (card row), `docs/design/README` roadmap row slvl.6.
