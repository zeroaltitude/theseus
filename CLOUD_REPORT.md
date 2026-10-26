# CLOUD REPORT: theseus-1q2ho, the weekly self-improvement digest must be honest

**Base: `961044a8` (tip of `cloud/20261026-p3n-halt-on-infra`), NOT `main`.** Merge `halt-on-infra` first. Branch
`cloud/20261026-p3p-digest-honesty`. This report overwrote the sibling's `CLOUD_REPORT.md`; the maintainer drops this
commit at the join. Store format and kernel SCHEMA untouched (no protocol type, no stored record changed; no TS regen).

## Found
- `rsi/digest.rs:226` (base) printed `rows.len()` as "N changes": a count of ledger rows.
- **Confirmed: "2000" was `MAX_ROWS` saturating.** `log::read(.., limit = MAX_ROWS)` (`rsi/log.rs:77-122`) returns
  early with `more = true` once `out.len() == limit`; the digest printed that page size as the week's output and then
  repeated it in the `more` line. A budget-heavy week always saturates.
- `SelfLogRow` carries no `gate` (only `what`/`numbers`), so the log's rows cannot be classified by `infra.rs`. The
  digest now also reads the raw `self.branch.built` / `self.joined` ledger rows (`join::rows_of`, uncapped, few rows;
  filtered to the week), which keeps the outcomes exact whatever the page cap.

## Changed (commit `3d814095`; start commit `727dc2db`)
- New `rsi/digest/outcomes.rs` (`Outcomes`): built / green / red / infrastructure / joined, and the rendered lead.
- `rsi/digest.rs`: `digest_text(rows, outcomes, capped, since, until, state)` (signature gained two args; only tests
  called it); `self_digest` reads outcomes uncapped; wording renamed (ledger rows / log entries, never "changes").
- Tests: new `rsi/digest/tests_digest.rs` (7 tests), `rpc/tests_self.rs` (one new test; the old `"3 changes: ..."`
  assertion in `the_digest_lists_a_seeded_week` was *rewritten* to the new wording, not deleted; two `digest_text`
  callers updated for the new args).

## infra.rs used (read, not changed)
`crate::rsi::infra::{of_row(d: &Value) -> Option<Outcome>, Outcome::{Green, Red, Infrastructure}}`. `of_attempt`,
`unjudged_items`, `HALT_AFTER` were not needed: the digest reads finished rows, and counts per week, not per item.
No parallel classifier.

## Table: number -> counted at base -> counts now
| Number | Base | Now |
|---|---|---|
| headline "N changes" (`digest.rs:226`) | `rows.len()`: ledger rows of the page, capped at 2,000 | removed as a headline |
| "Branches built" (new, `outcomes.rs`) | none | exact count of `self.branch.built` rows in the week |
| "with a gate verdict" | none | built rows `of_row` classes green/red/infra (halted/empty excluded) |
| "Judged / green / red / pass rate" | none | green+red per `infra::of_row`; no rate when judged = 0 |
| "N of N gates never started" | none | infra rows; WARNING with no pass rate when all are infra; "not counted red" otherwise |
| "Joined" | none (only the Joins list, capped) | exact count of `self.joined` rows |
| "Cost" (`digest.rs`, `cost_of` sum) | sum over the page | same sum; prefixed "at least" when the read was capped |
| "Ledger rows read (bookkeeping, not work done)" | the "changes" line | per-kind counts over the page; "at least" when capped |
| "Log entries, newest first" / "and N more" | "Every change, newest first" | renamed; "at least" on the more-count when capped |
| `SelfDigestResult.rows` | `rows.len()` | unchanged (rows read; the field name is the protocol's) |
| the `more` footer | "Read the first 2000 changes" | says the counts are a floor |

## Posts
`POST_CHARS = 1_800` (`digest.rs:36`). The outcomes block is ~350 chars at the top; `outcomes_come_before_bookkeeping_and_fit_one_post`
asserts the posted text (900 pairs of rows, capped) keeps "2 of 2 gates never started" and "Joined" and stays within
the cap plus the post's own head. `crates/theseus-discord` is untouched (`git diff 961044a8 -- crates/theseus-discord` is empty),
so `golden/dm_flow.txt` is byte-identical.

## Proof
- Filter check: `cargo nextest list -p theseus-core -E 'test(tests_digest) | test(bookkeeping_beyond)'` listed 8:
  `rpc::tests_self::a_week_of_bookkeeping_beyond_the_page_cap_is_a_floor_and_no_work` and 7 in
  `rsi::digest::tests_digest::` (a_capped_read_is_a_floor_and_never_exact, a_join_is_counted_from_its_own_row,
  a_row_outside_the_week_is_not_this_weeks_outcome, a_week_of_bookkeeping_reports_no_work,
  an_unjudged_branch_is_not_counted_red, gates_that_never_started_are_flagged_and_give_no_pass_rate,
  outcomes_come_before_bookkeeping_and_fit_one_post).
- `-E 'test(digest)'`: 68/68 pass. `-E 'test(rsi::) | test(tests_self) | test(owner_note)'`: 407/407 pass.
- Planted revert 1 (old `rows.len()` "changes" headline put back): FAIL a_week_of_bookkeeping_reports_no_work,
  the_digest_lists_a_seeded_week, a_week_of_bookkeeping_beyond_the_page_cap_is_a_floor_and_no_work. Restored, touched, clean.
- Planted revert 2 (infrastructure counted as red): FAIL an_unjudged_branch_is_not_counted_red,
  gates_that_never_started_are_flagged_and_give_no_pass_rate, outcomes_come_before_bookkeeping_and_fit_one_post. Restored, touched.
- **Red on the base, not run literally:** the new tests cannot compile against the base's `digest_text`, so the base's
  failure is the planted revert 1, which is the base's behaviour. The base text for a bookkeeping week was
  `N changes: ...` (e.g. a 2,100-row week printed "2000 changes: 2000 self.budget.settled").
- clippy `-p theseus-core --all-targets -D warnings`: clean. `cargo fmt`: applied.

## Gate
- **Baseline on the unmodified base: FAILED in the door/keel phase at 17 s**, before any build. The keel guard refuses
  every owner ack here ("its signature does not verify (%G? is E)": no signing key on this VM) and reports 8
  inherited erosions in earlier commits (persona_cmd.rs tests removed; due.rs and cards.rs assertions weakened;
  first_request.rs CHANNEL_TOKENS 7,900 -> 8,900; keel-guard.py and test_keel_guard.py edited). None is in a file I touched.
- After my change: the keel guard prints the **identical 8 findings** (no new one); the full gate cannot pass its
  door here for the same reason. I ran the later checks by hand: fmt, clippy, and the 407 related core tests. I did not
  run the whole suite or the discord crate's tests. The maintainer should run the gate on the owner's machine.
- Keel guard output after commit: same eight lines as above, none new.

## Left open
- `OWNER CALL:` the digest names unjudged attempts by count only; per-item naming (`unjudged_items`) was left out.
- The Discord DM post shows the new lead as is; a live look at one real week's text is for the maintainer
  (`theseus self digest` on a scratch daemon over a copy of the store).
- Docs to update at review: spec Part III item for the digest and `docs/status.md` (wording "changes" -> outcomes).
- Prompt line citations at this base: `digest.rs:226`, `:221-225`, `:229`, `:232-244`, `:249`, `:56-59`, `MAX_ROWS :29`, `LISTED :30` all matched as described.
