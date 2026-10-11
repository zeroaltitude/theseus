# CLOUD_REPORT: turn-retry-fix (theseus-posww)

Branch `cloud/20261011-turn-retry-fix`. One code commit (a17f80dd), then this report. Not split per step: the steps
share test edits and the golden.

## Findings and changes

1. **has_news** skipped only tool calls, so a Recall note after the waiting message made the retry end `nothing_new`.
   `Kind::is_note` (ToolCall, Recall) is now skipped (stub.rs, turn.rs). Recall is the only compile-written node kind.
2. **No-op continuation.** `turn/noop_step.rs`: a continuation with 0 loops, stop `nothing_new` or `awaiting_confirm`,
   and no wakes posts no reply. If the session record holds a run of failed turns (`failing`), one `notice` post says
   "Nothing to retry: <why> (the turn failed with <class>). Send a message to go on."; else nothing. Done in the core
   (`stage_turn_post`), so Discord and the push need no change (the Discord render of `reply` is untouched).
3. **No retry into the same wall.** `Failing::after`: a non-transient class parks at once (the existing park notice says
   the next message retries). Design choice: `internal` (a harness fault) keeps its one retry; the brief named
   invalid_request and transient classes only. `Then::Retry` stays for that and for old rows. The fact's park line now
   reads "the same request would fail the same way".
4. **Per-message effort.** `default_of` names only `claude-fable-5-1`, comment cites the 2026-10-11 400. Fallback:
   `turn/effort_step.rs` + `RequestSpec.effort_refused`: a 400 naming `output_config` on a request with effort-only
   messages is asked again once (same turn) without them, level moved to top-level, model marked in
   `TurnRunner.effort_refusals` for the daemon's life, ledger row `provider.effort_refused` (new LedgerKind, fact
   `EffortRefused`). No store format change. compiler.rs is at its ceiling (2560): I condensed two doc comments to fit one field.

## Proof
- New: `tests_turn_retry.rs` (3 tests), `tests_turn_effort::a_400_on_the_messages_asks_again_without_them_and_remembers`,
  `session::tests::a_harness_fault_is_retried_once_then_parks_with_one_notice`.
- Planted reverts: has_news back to ToolCall-only plus noop disabled together: all 3 tests_turn_retry tests failed
  (not separated). Fallback disabled (`true ||` in effort_step): the 400 test failed. Files restored and touched.
- Tests rewritten for the new rules (rename/rewrite, assertion counts kept): tests_failures (400 parks at once; the
  internal-fault test is unchanged), session.rs rule tests, tests_continuations (400 case parks), tests_output
  scenario (one scripted 400 not two) and its golden (`THESEUS_GOLDEN=write`; large diff is id renumbering),
  theseusd `tests/failures.rs` 400 test. tests_turn_effort's three Sonnet tests moved to a Fable live profile
  (`c.model.live = "fable"`; the Opus/Haiku one now asserts they get none); tests_route_effort's Sonnet expectation is now None.
- Full suite on this VM: 3801 passed, 36 failed at first run; after fixes, a rerun of core, discord, cli, protocol and
  theseusd: 2555 passed, 13 failed, all `theseusd::sandbox` (L1 as root, known). Earlier-run L1 failures in theseus-sandbox are the known set.
  clippy `-D warnings`, fmt and shape clean; deny ok; cockpit build and registry ok.
- Gate: `scripts/gate.sh` stops in the keel phase on findings from the base range, not mine (tui test assertion,
  two allow-added, two ceilings in long-files.txt; unverifiable signed acks). I ran the later phases by hand as above.
  The turn bench was not run (no daemon bench here).

## Keel findings expected
None from this branch. Base findings listed above need the owner's ack at the merge.

## Live check (owner)
Scratch daemon on a fresh state dir, DM or CLI session on Sonnet 5.5 with a history holding Fable effort messages:
send a message; it should answer (no `output_config` 400). Make a 400 (bad model) and confirm one park notice, no retry.
Retry-with-nothing-to-do: `theseus` on a session with a failed turn already answered, wake its execution; expect the
one-line "Nothing to retry" notice, no footer-only reply.

## Left / uncertain
- Opus 5.5 and Opus 5 now get top-level effort until proven live.
- Docs to update by the maintainer: core AGENTS.md "effort::default_of" line (Fable 5.1 only) and the retry rule
  (theseus-ljr: lasting classes park at once); spec Part III item.
- The `provider.effort_refused` mark is in memory; a restart costs one failed call per model.
