# CLOUD_REPORT: sealed-hold (theseus-pw1q.21.7)

Branch `cloud/20261017-sealed-hold`. Base `b95a2558` (R3's `ScratchRunner` present). One step, one code commit: `2e4f8a45`.

## What I found
- A branch's own build tree is deleted by self-pr's `tree::close`, so the candidate's binaries do not exist at join time. Main's gate on the **merge** tree builds `target/debug/{theseusd,theseus}`, so the phase runs inside each try, **after main's gate and before the signature** (not "after review, before the merge"), and scores the merge's build. It then also reruns when main moves (candidate rescored, main cached).
- The local `main` branch in this clone is stale (96c592c); the gate's keel phase used it as base and listed erosions from other work. I ran the gate with `THESEUS_KEEL_BASE=origin/main`.

## What I changed (2e4f8a45)
- `rsi/sealed/hold.rs` (new): `BEHAVIOUR` globs (`*` = any run; test files never count), `touches`, `judge` (family >=5 items loses >1, or a set's total falls: hold; else reported), `Scorer` seam, `Scratch`, `Kept`.
- `rsi/sealed/daemon.rs`: `Core::sealed_score`, `sealed_main` (META `self.sealed.main.<commit>.<digest>`, first kept is baseline `self.sealed.baseline`), `sealed_drift` (META `self.sealed.drift.at`).
- `rsi/sealed.rs`: `Sealed::digest`, `Sealed::allows` (the filter over any text), `mod hold`.
- `rsi/join/sealing.rs` (new; the brief named join.rs/reviewing.rs): the phase. `reviewing.rs`: `reopened` helper shared with a review's reject. `join.rs` (`Seams.sealed`), `joining.rs` (one call), `tender.rs` (drift tick).
- `fact/rsi_sealed.rs`, protocol `ledger.rs` + `rsi.rs`: kinds `self.sealed.checked`, `self.sealed.drift` (SELF_ROWS entries). `rsi/log.rs`: `sealed_line(before, after, total)` (R8 reads it for "Fixed": `sealed 38/40 → 39/40`). `backlog/read.rs`: drift cause `sealed.drift:<family>` (`_total` for a total).
- Cockpit: no change needed (`selfchanges.ts` shows rows by their `what`; protocol.gen unchanged after the generator test). `crates/theseus-core/AGENTS.md` updated.
- No config key, no store format change, no dependency; `Cargo.lock` untouched.

### Behaviour globs (all match a file; test `every_behaviour_glob_matches_a_file`)
turn.rs, turn/{spec_step,standing_step,route_step,recall_step,rerank_step,advance_step,nudge_step,budget_step}.rs, route_step/*, context_files.rs, standing.rs, persona.rs, theseus-tools/src/*, tools_defer(.rs,/*), native.rs, routing(.rs,/*), config/routing.rs, config/theseus.example.toml, theseus-judge/src/*, core judge/*, theseus-memory/src/*, recall(.rs,/*), memory_lookup(.rs,/*), compiler(.rs,/*), prefix(.rs,/*), advancer.rs.

## Proof
- New tests: 11 in `rsi/sealed/tests_hold.rs`, 5 in `rsi/join/tests_sealed.rs`: all pass. Suite: `TZ=America/Phoenix cargo nextest run --workspace --no-fail-fast`: 6709 run, 6675 passed, 34 failed = the 33 known L1 failures (theseus-sandbox contract + spawn_100, theseusd sandbox) plus `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`, which passes alone (load; not mine).
- `cargo fmt --check`, `shape.sh`, `cargo clippy --workspace --all-targets -D warnings`: clean. Cockpit lint/test ran (disk filled during the run, result not captured).
- Planted reverts (each restored, `touch`ed, `git status` checked):
  1. hold only on total (family rule off): `tests_hold::a_family_drop_holds_the_join`, `tests_sealed::a_family_drop_holds_the_join` FAIL.
  2. hold on any family: `a_family_under_five_items_is_reported_never_held`, `the_total_falling_holds` FAIL.
  3. cache key without the set digest: `mains_score_is_cached_per_commit_and_set` FAIL.
  4. drift cause per run: `drift_files_one_cause` FAIL.
  5. reply text in the hold row: `tests_sealed::a_family_drop_holds_the_join` (greps the ledger) FAIL.
  6. every path a behaviour path: `wildcards_match_a_run_and_a_behaviour_path_is_not_a_test`, `a_branch_touching_no_behaviour_path_skips_the_phase` FAIL.
- Off/halted: `off_or_halted_scores_nothing_and_spends_nothing`, `tests_sealed::off_and_halted_score_nothing`.

## Gate
`gate.sh` stops in `keel` (below) because I cannot sign; I ran its other phases by hand: fmt, shape, clippy, suite (above). The benches were skipped (`THESEUS_GATE_NO_BENCH`). **`cargo deny --offline` could not run** (advisory DB not fetched here); no dependency changed.

keel-guard (`--base origin/main --head HEAD --quiet`):
```
crates/theseus-core/src/rsi/sealed.rs:41: keel-file: pub mod relay; -> pub mod hold;
crates/theseus-core/src/rsi/sealed/daemon.rs:10: keel-file: use super::relay::Relay; -> use super::hold::{...}
crates/theseus-core/src/fact/rsi_sealed.rs:12, rsi/join.rs:95, :110, rsi/join/sealing.rs:25: sealed-reach: (none) -> uses of sealed::hold
```
Trailers for the joiner:
```
Keel: keel-file crates/theseus-core/src/rsi/sealed.rs, crates/theseus-core/src/rsi/sealed/daemon.rs — the owner, Discord DM 2026-10-17 23:34:35 MST: "(2) yes" to "one Keel yes covering R1, R2, R3 and R4's keel changes: the keel guard watching the sealed and exam paths, self builds inside L1, the scratch-daemon Runner and model relay, and the sealed check in self-join" (the sealed hold: a mod line, the score, cache and drift calls)
Keel: sealed-reach crates/theseus-core/src/fact/rsi_sealed.rs, crates/theseus-core/src/rsi/join.rs, crates/theseus-core/src/rsi/join/sealing.rs — the owner, Discord DM 2026-10-17 23:34:35 MST: "(2) yes" to "one Keel yes covering R1, R2, R3 and R4's keel changes: the keel guard watching the sealed and exam paths, self builds inside L1, the scratch-daemon Runner and model relay, and the sealed check in self-join" (the sealed check in self-join and its rows)
```

## Live check for the maintainer (needs a non-root daemon with a job cgroup; none of this ran here)
1. Scratch daemon, `[self] mode="act"`, `[self.sealed] dir=<dir>` with a set of >=5 items per family. `theseus self resume`.
2. Self-join a branch touching e.g. `crates/theseus-core/src/compiler.rs`: `theseus self log` should show a `self.sealed.checked` line `sealed N/T → M/T for <branch>: passed|held`, and a hold a `self.join.stopped` with phase `sealed`.
3. Drift: `Core::sealed_drift` has no CLI verb; the join tender runs it every `drift_every_days` after its first pass. To force: lower the config to 0 days and watch for `self.sealed.drift` rows after a later score.

## Uncertain / for the owner
- Main is scored from the **installed** binaries (R3's rule) but cached under the try's base commit; if the install lags main, the comparison is installed-vs-merge. 
- Total-fall is judged per set; a total drop in drift files `sealed.drift:_total` beside the family's cause.
- A skipped phase (no dir, empty set) writes one `self.sealed.checked` row per join of a behaviour-path branch.
- Spend: only through R3's relay, booked on the joining branch's line for both main and the candidate (main's scored row carries the branch name but the base commit as `head`).
- Docs to change at review: spec Part III item, `docs/status.md`.
