# CLOUD_REPORT: tail7d2-misroute-run (theseus-qu41j)

Branch `cloud/20261021-tail7d2-misroute-run`, base `bb6a43e5`. Files touched: only the row's three
(`toolrun/misroute.rs`, `rpc/tests_thread_route.rs`, core `AGENTS.md`). Store format 49, no bump. No new dependency.

**For the integrator:** `crates/theseus-core/AGENTS.md` lines 604-608 were rewrapped to ~118 columns in the same hunk
as the edit (the 4gb4w line was ~216 columns). It conflicts with 4gb4w's join fix 2: take this side whole.

## Cause and rule
A `proc.run` plan's resource is its cwd (`Access::Exec`). A program run from the session's directory, the parent of
every relative root, was outside every root, so it was held with a false question.
`Guards::check` (`toolrun/misroute.rs`) now sorts each write/program resource into:
- **inside**: a root is a prefix of it (as before);
- **undecided**: an `Exec` that is an ancestor of some root (whole components);
- **outside**: anything else, a write at an ancestor included.

Any outside resource is held (first one, same `Held`, guard lets go). If every resource is undecided, nothing is held
and `armed` goes back into the map. Otherwise the guard lets go as before. `Held::why` is unchanged. No new state,
config key or ledger row.

**Trade:** a misplaced turn that only runs programs from the session's directory is no longer asked; its first write
outside the roots, or a program run inside another tree, still is (stated in the module doc).

## Tests (commit 421821a7)
Red first, on the base code (3 failed, 7 passed): `a_program_run_from_the_session_directory_neither_holds_nor_lets_go`
(misroute.rs:380), `a_program_run_in_an_unrelated_or_other_threads_directory_is_still_held` (:425),
`rpc::tests_thread_route::a_guess_placed_program_from_the_session_directory_is_not_held` (:1017).
`a_write_at_the_session_directory_still_decides` passes on the base by design; it is guarded by plant 2.

Green after the fix: `-E 'test(misroute) | test(thread_route) | test(session_directory)'` 60 passed;
`cargo nextest run -p theseus-core` 4136 passed, 0 failed.

Plants (each restored, `touch`ed, `git status` clean afterwards):
1. Undecided Exec lets go (the `armed` re-insert switched off): the key unit test failed (misroute.rs:399) and the rpc
   test failed (tests_thread_route.rs:1017).
2. Drop the `Exec` check from "undecided": `a_write_at_the_session_directory_still_decides` failed (misroute.rs:423).

**rpc policy choice:** `proc_run {"argv":["true"]}` is not confirmed by the rig's policy on its own, so the test
asserts on `awaiting_confirm` and the `thread.misroute_held` rows directly. The test runs two cases: program then
write into `tidewatch/` (no row, file written) and program then write into `vault/` (one row naming the vault path,
file not written).

## Checks
- `python3 scripts/keel-guard.py --base bb6a43e5 --head HEAD --quiet`: empty output, exit 0.
- `cargo fmt --check` and `cargo clippy -p theseus-core --all-targets -- -D warnings`: clean.
- **The full `scripts/gate.sh` was not run** (the cockpit build, the other crates' suites and deny were not run here).
  Only theseus-core's whole suite, clippy for that crate and fmt were. The maintainer should run the gate.
- Signing: I passed `-S`; this VM could not verify the signature (`gpg.ssh.allowedSignersFile` is not configured), so
  say it is unverified here. The commit trailer is the task brief's `Co-Authored-By: Claude (cloud session, Sonnet 5.5)`
  line; `main`'s recent commits carry `Tabitha/Claude`, so the integrator may want to adjust it.

## Live check for the maintainer
On a scratch daemon with a thread whose working set has relative root `tidewatch`, place a turn below the act bar
(0.60 to 0.90) that runs a program from the session's directory, then writes into `tidewatch/`: no
`thread.misroute_held` row. Repeat with a write into another thread's tree: exactly one row and one card.

## docs/status.md sentence
"The misroute guard treats a program run from a directory above the thread's roots (the session's directory) as
undecided: it holds nothing and stays armed, and the turn's next write or program decides (theseus-qu41j)."

## After install
A guess-placed turn that runs a program from the session's directory gets no card; its next write outside the
thread's roots still asks once. The guard is still disarmed at the turn's end.

## Follow-ups
None needed. `keep_apart_by_dir` in `home_claim.rs` stays its own row.
