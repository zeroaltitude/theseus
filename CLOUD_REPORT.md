# Cloud report: auto-revert and the owner's veto (theseus-pw1q.10)

Branch `cloud/20261013-auto-revert`. Code commit: `30f618b1`.

## What I found
- `self.joined`, `self.installed` and the join lock, tree, gate, signing and push pieces were reusable as they stood. I made `join::refused` and `join::blocking` `pub(crate)` and nothing else in join.
- The store's format is unchanged (two new `LedgerKind`s, no stored-record change).
- The brief's "gate records" are `~/.cache/theseus/gate-times.csv` and `bench-history.csv`. The health signal reads the ledger's `turn.failed`, `provider.error` and `execution.failed` rows.

## What I changed (`30f618b1`)
- `[self] revert_window_hours = 24`, with its template line and test.
- `rsi/revert.rs` and `rsi/revert/`:
  - `signals.rs`: the `Signals` seam and the `Files` reader.
  - `attribute.rs`: bisection through the `Prober` seam; `MainsGate` is main's gate on a detached tree.
  - `doing.rs`: the revert under the join lock.
  - `tender.rs`: the periodic pass (20 minutes after serving, then every 10), only in mode act.
- `fact/rsi_revert.rs` with `self.reverted` and `self.vetoed`, and the two `LedgerKind`s.
- The veto: `self.revert` (protocol method, params and result; the generated TypeScript is regenerated and added); `Act::SelfRevert` in `judge_act`; `theseus self revert MERGE`; the cockpit's Revert button.
- `Core::self_drop_vetoed`, added to `backlog/word.rs`, drops the vetoed join's backlog item for good.
- AGENTS.md updates in theseus-core and theseus.

## Design choices the owner should hear about
- The veto drops the backlog item before it runs the revert. A revert that then stops at the owner's card still keeps the item dropped.
- A health red cannot be replayed through the gate. It is blamed on the newest self-join before it, unless an outer-loop commit landed in between.
- A revert that would trip the keel (main's gate says keel), conflicts, is red, has no signing key, or loses the push race 3 times posts the owner's card and writes `self.join.stopped` with `branch "revert <merge>"` and `numbers.held`. The watch then leaves that merge alone; the veto can be run again.
- A red judged once is remembered in memory, so it is not bisected again each pass. After a restart it is judged once more.
- The health thresholds (an hour with at least 5 errors and at least twice the hour before the first join) are constants in `signals.rs`, not config.
- A veto given as a merge of an outer-loop join is refused (`BadMerge`), by the brief's "never touches an outer-loop join".

## How I proved it
- `cargo nextest run -p theseus-core -E 'test(/revert|rsi::/)'`: 101 passed.
- The 9 revert tests are in `rsi/revert/tests.rs`, over a throwaway origin, a stand-in gate (red while `broken.txt` exists), a stand-in gpg and a "world" clone that lands merges:
  - planted regression reverted and noticed (signed, `self.reverted`, notice);
  - install handed the revert's commit;
  - red before the join left alone;
  - outer-loop join never reverted, and the red not re-tested;
  - bisection picks the right one of two self-joins (2 gate runs);
  - veto reverts at once, and a second veto changes nothing;
  - a job's veto refused with an `approval.refused` row, an outer-loop merge refused, a short name refused;
  - mode off and a halt watch nothing and read no signals, while the veto still works;
  - a join past the window not watched;
  - a revert tripping the keel goes to the owner's card with nothing pushed and no tree left.
- `signals.rs` has 4 unit tests (gate and bench CSV reading, label to commit, quoting); `self_cmd/revert.rs` has 1.
- Planted reverts, each failing the named test (file restored and touched):
  - the parent probe off: `a_red_that_was_there_before_the_join_leaves_it_alone` failed;
  - an outer commit blamed on a join: `an_outer_loop_join_is_never_reverted` failed;
  - the owner check off: `a_jobs_veto_is_refused_and_only_self_joins_can_be_vetoed` failed;
  - the mode and halt gate off: `mode_off_and_a_halt_watch_nothing_but_the_veto_still_works` failed;
  - the window off: `a_join_past_the_window_is_not_watched` failed.
  - I did not plant a revert for the bisection test.
- Cockpit: `npm test` 379 passed (including the new `selfchanges` test), `npx tsc -b` clean, `npm run lint` only warnings in files I did not touch, `npm run build` ok.
- theseus-protocol: 89 passed, including the generated-TypeScript test.
- `cargo fmt --check`, `scripts/shape.sh` and `cargo clippy --workspace --all-targets -D warnings` are clean.
- Full suite (`cargo nextest run --workspace --no-fail-fast`): 4627 tests, 33 failures, all L1 sandbox tests on this root VM (theseus-pv6i): 20 in theseus-sandbox (the contract tests and `spawn_100`) and 13 in `theseusd::sandbox`. Nothing outside them failed.

## Live check for the maintainer
1. On a scratch daemon with `[self] mode = "act"`, `repo` and `remote` set, and the switch resumed, run `theseus self log` and find a `self.joined` row. Its undo line is `theseus self revert <merge>`.
2. Run `theseus self revert <merge>`. It should print "Reverted self-join …", main should gain a signed revert commit, and the owner's DM should get the "Your veto" notice. `theseus self log` should show `self.vetoed`.
3. Run `theseus self revert <merge>` again: "reverted already".
4. From a job's shell (`THESEUS_SESSION` set), the same command should be refused, with an `approval.refused` row.
5. In the cockpit's "What Theseus changed about itself" card, a `joined` row should show Revert.
6. For the auto path, make main's gate red after a self-join (a failed `total` row for `main <describe>` in `gate-times.csv`) and wait one pass.

## Keel findings
Nothing in my commit raises a ceiling, removes a test, or adds an `#[allow]`. `python3 scripts/keel-guard.py` still reports findings from commits that were already on the branch before mine: a `budget-raised` in `crates/theseus-sim/src/lifecycle.rs` (the ack on 1c2beb2747 does not verify here, because there is no owner key) and a renamed store test. They are not mine.

## Gate
- `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in `door`/`keel`, only on the findings above. I ran the later phases by hand as the brief allows: fmt, shape, clippy, cockpit lint/test/build, the protocol/TypeScript test and the full suite as described above.
- `features` was not run separately.
- **`cargo deny` did not run**: no advisory database was fetched. No dependency changed (`Cargo.lock` is untouched).
- The benches were skipped (`THESEUS_GATE_NO_BENCH`).
- The disk filled once during the suite build. I deleted `target/debug/incremental` and `target/release-thin` (rebuildable) and reran with `CARGO_INCREMENTAL=0`.

## Left or uncertain
- The `Files` signals and `MainsGate` are tested only through the seams and the CSV parsers, not against the machine's real files.
- The install handoff reuses `join::install::Systemd`; I did not run it.
- The `self.revert` request holds its connection for the whole gate on the revert, which can take minutes.
