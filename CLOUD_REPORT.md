# CLOUD_REPORT: trim-security-v1 (theseus-rwpbg.6)

Branch `cloud/20261017-trim-security-v1`, based on `432bbbeb`. Commits: `10fa0534` (the change), `6da2aa7b` (a restored assertion), `e141889b` (the daemon ladder test's vehicle).

## What I found
- Today the gate asked `security.v1` (shadow) and `security.v3` (live) of every acting call. The two states differ, so the batcher cannot merge them.
- The ladder already gives every `security` version the notice brake's two rules (`ladder::adopt::rules("security")`: 30 notices a day, 3 `noise` labels). `rules_of(pack)` merges them with the pack file's own and dedups.
- Health's pack lines come from `WIRED` alone, so dropping v1 from `WIRED` removes its line. Several tests named it.

## What I changed
- `judge/gate.rs`: `SECURITY_PACK` is now `"security.v3"`; `SECURITY_CANDIDATE` and `GATE_PACKS` are gone. This was the smaller diff, since every v3 caller renamed in place and every v1 caller had to change anyway. `at_gate` and `of_call` handle one pack. `prepare_gate` always builds `Input::Security2`. The score of a notified call is v3's answered `risky`, and `mode` is the judgment's real mode (`live`), no longer the literal `"shadow"`. `input()` stays, because v3 builds on it; `LAST_CALLS` stays too.
- `judge/mod.rs`: `WIRED` drops the v1 line.
- `judge/notice.rs`: `rules()` is `ladder::rules::rules_of(SECURITY_PACK)`.
- Cockpit: `scores.ts` scores from v3 rows and from stored v1 rows; its test and the pill's tooltip are updated. Discord and the CLI use only `percent` and `mode`, so a v3 score reads `risk 37% (live)` there. `JudgeScored`'s docs changed, so `protocol.gen/JudgeScored.ts` is regenerated.
- Docs in code: the `gate.rs`, `mod.rs` and `notice.rs` module docs, `crates/theseus-core/AGENTS.md`'s gate lines, the config template's gate comment, and a few other comments.
- v1 stays in `pack::EMBEDDED`, so replay can still find it. A `[judge.packs."security.v1"]` table still loads; a test holds that.

## Deviation: `security.v3.toml` is not changed
`pack.rs` says "Any change ... is a new version", and v3's own header repeats it. You said to stop and explain in that case, so I did not add `[[rollback]]` blocks to v3. The brake reads the same rules from the ladder's adoption table instead (same numbers, same mechanism that already guards v3). No pack sha256 changes. If you want the blocks in the file as you described, say so; it is a one-line change in `rules()` plus the TOML, and it needs the owner's call on whether that is a v4.

## Tests removed or changed (and why)
- Removed/changed because v1 is not asked: health pack lists in `tests_judge.rs` (2), `tests_continue.rs`, `tests_inbound.rs` (also `INBOUND_ALONE` 16 -> 15), `theseusd/tests/judge.rs`; `GATE_PACKS` loops in `tests_inbound.rs` and `tests_route.rs`; `tests_ladder_unread.rs` no longer waits for a v1 judgment.
- `tests_security.rs` and `tests_notices.rs`: judgment counts halved (one per call, not two); the v1-score test is now `a_notified_call_is_scored_by_v3s_risky` ("risk 37% (live)"); marks are one; the v1-mode assertion in `tests_notices.rs` is now `None`.
- `theseusd/tests/judge.rs`: the ladder test used v1 as a security pack to promote; it promotes `security.v2` and reads the `pack.mode` rows.
- `cockpit/test/scores.test.ts`: "only security.v1 scores" is now "v3 scores, stored v1 rows too, v2 does not".
- New: `the_gate_asks_v3_alone`, `the_brake_reads_v3s_rollback_rules` (also asserts v1's rules are identical), `v1_judgments_already_stored_still_read`, plus a config assertion.

## Proof nothing was lost
- Brake: the existing tests are unchanged in behaviour (`tests_notices`: the 31st notice, three noise labels with a restart, a ladder rollback) and pass. Rules are equal to v1's.
- Score: `a_notified_call_is_scored_by_v3s_risky`.
- Decision: `with_jev_the_gate_decides_as_it_does_without` (property test) passes.
- One ask per call: `the_gate_asks_v3_alone` counts Jev requests (2 calls -> 2 asks) and asserts every judgment is v3's.
- Stored v1 judgments list and get: `v1_judgments_already_stored_still_read`; replay of v1: the existing `tests_replay` (v1 against v2) passes.
- Planted reverts: I did not run them. Reverting `rules()` to v1's gives identical numbers by design, so no test can fail on it; I note that. Not run: planting two packs back into the gate (the one-ask test would fail on the row-pack and count assertions) and the v1 score filter.

## Results
- `cargo nextest` for theseus-core, theseus-judge and theseus-protocol: 3,370 passed, 22 skipped (`TZ=America/Phoenix`). theseusd `--test judge`: 4 passed. Cockpit `npm test`: 454 passed. `cargo clippy --workspace --all-targets`: no warnings. `cargo fmt` applied.
- NOT run: `scripts/gate.sh` as a whole, the cargo-deny phase, the rest of the workspace suite, and a live check. Disk ran out twice and the full gate was not attempted; run it before merging.

## Keel
`python3 scripts/keel-guard.py --base 432bbbeb --head HEAD --quiet` prints nothing for this branch: the one dropped assertion it first flagged (`in_a_holding_session_a_call_scored_one_percent_still_waits`) was restored in `6da2aa7b`. Against `origin/main` the guard also lists three findings from unrelated commits that main gained since; they are not this row's. So no `Keel:` trailer is strictly required, but the owner's approval, for the joiner's commit, is:
`Keel: assert-removed crates/theseus-core/src/tests_security.rs — the owner, Discord DM 2026-10-17 16:19:39 MST: "Agree" to "retire it, after moving the brake and the score over to v3"`
(use it if the guard asks for it at the join).

## Live check (owner's machine)
Start a scratch daemon with a notify-posture `proc.run` and Jev on; run one command. Expect one `security.v3` judgment (`theseus judge list --pack security`), `judge.scored` "risk N% (live)" after the notice, and no `security.v1` row. `theseus health` lists no `security.v1` line.

## Left over / for docs
- Spec Part III and status.md should say v1 is retired from the gate. `docs/design` names v1 at the gate in places; I did not touch docs.
- The pack docs' "any change is a new version" is stricter than the loader; worth a decision on rollback blocks in v3.
