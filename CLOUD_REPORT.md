# CLOUD_REPORT: tail7d-misroute (theseus-4gb4w)

Files touched outside the named list: none. (`rpc/thread_flow.rs` `arm_guard` and its one call, `toolrun/misroute.rs`, `rpc/tests_thread_route.rs`, core `AGENTS.md`.)

## Cause, option, base
- `thread_grow` stores roots relative to the session's directory (`tarnlib`); the guard compared them with no base against absolute plan paths, so nothing matched and the first write below the act bar was always held.
- **Option (a)**: resolve at arm time. `arm_guard` takes the run session record and passes `self.tools.cwd_for(run.dir)`; `Guards::arm(session, title, roots, base, alt)` reads each root with `thread_route::entry_path(r, base, home)`. The base is the one the grower used (`cwd_for(session.dir)`, with the `[tools] cwd` fallback). No stored data changes, no migration.
- **Relative root with no base**: left out (cannot happen via `cwd_for`); if no root is left, the session is disarmed. Tested at the end of `a_relative_root_is_read_under_the_session_directory`.

## The question
- Old: "Go on, or move to the a path-traversal fix in a small web service?" / "isn't in the the pipeline thread".
- New: "Go on, or move to a path-traversal fix in a small web service?" / "isn't in the pipeline thread". A small `the()` helper adds "the " unless the name's first word is a/an/the (any case).
- A title with no article is byte-identical: `the_first_write_outside_the_roots_is_held_once` still asserts the exact old string and passes.

## Item 4: `keep_apart_by_dir`
It reads a thread's relative roots under the thread's own session `dir` with no `[tools] cwd` fallback. A thread whose session has no `dir` has its relative roots skipped there (the thread is scored as before). That is the safe direction, but not the grower's base. Not changed (the base differs by thread, so it is more than a line or two). **Follow-up**: use `cwd_for(session.dir)` there as well.

## Tests and plants
- Unit (misroute.rs): `a_relative_root_is_read_under_the_session_directory`, `a_path_outside_every_root_is_still_held`, `the_question_has_no_doubled_article`.
- rpc: `a_guess_placed_turn_inside_its_threads_relative_root_writes_without_a_hold` (no hold, file written, no row) and `a_guess_placed_write_outside_its_relative_roots_is_still_held` (one row).
- Plant 1: arm reads roots with no base. Failed: `a_relative_root_is_read_under_the_session_directory` (misroute.rs:325), `a_path_outside_every_root_is_still_held` (:365), `a_guess_placed_turn_inside_..._without_a_hold` (tests_thread_route.rs:935). That is the base-code behaviour, so it is also the red-first run; I wrote the fix before the tests, so red-first was shown by plant, not by a separate base run.
- Plant 2: old `format!`. Failed: `the_question_has_no_doubled_article`.
- Both restored, `touch`ed, `git status` clean of stray changes.
- Runs: `cargo test -p theseus-core --lib -- thread misroute`: 252 passed. Whole `-p theseus-core --lib` run: 3 failures (`card::tests_job::the_budget_stops_the_job`, `import::tests_catalog::a_quiet_stretch_after_work_gives_the_free_pages_back`, `rpc::tests_self_mint::a_validation_runs_its_tiers_...`), all passed on rerun alone (load). I did not compare against the base. I used `cargo test`, not nextest, and did not run the full `scripts/gate.sh` or the runs under load: nextest was still installing and time went to builds.
- `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean (after folding `arm_guard`'s call to stay under `route_to_thread`'s 100-line limit).
- Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0 (run before the report commit). Store format: 49, unchanged. Commits are unsigned (no key).

## Commit
bb6a43e5 `misroute: read a thread's relative roots under the session's directory ...`

## docs/status.md sentence
"theseus-4gb4w: the misroute guard reads a thread's relative roots under the session's directory (the grower's base), so a guess-placed turn inside its own thread is no longer held; its question no longer doubles an article."

## After the install
A guess-placed turn that writes inside its thread's roots goes through with no card; one outside them still asks once.

## Live check
Not run (no owner keys). Suggested: on a scratch daemon with two threads (roots `tidewatch`, `vault`) and a session in their parent directory, send a message Jev places at 0.75 and have the model write into `tidewatch/`: expect no confirm and no `thread.misroute_held` row; a write into `vault/` should show one.

## Follow-ups
- Item 4 above.
- Full gate and nextest runs still owed by the maintainer.
