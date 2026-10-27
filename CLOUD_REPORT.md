# CLOUD REPORT: theseus-5o3av (pin `neighbour::apply` in recall's refill path)

## Checked first
`git log origin/main` head was 7d39d57 (wf482-fallback), past my base 9470df70; 96cyh and mo1vm are both in. Nothing in this row had landed.

## Citations at my base
- Run site `neighbour::apply`: `recall.rs:869` (brief said :865).
- Refill site: `recall.rs:911` (brief said :897/:902); `current::apply` is at :912, `current::marked` at :906.
- mo1vm's P3 test: `a_live_rerank_keeps_the_superseded_marks_of_a_current_question` in `crates/theseus-core/src/tests_recall_current.rs`.
- Existing run-site test (P2): `tests_recall_neighbour::a_recall_of_another_host_names_it_in_the_frozen_header`.

## Step 1: the test (commit: see branch head, subject "recall: a test pins the neighbour note's re-apply in the refill path")
Added `tests_recall_neighbour::a_live_rerank_keeps_the_neighbour_note_in_the_frozen_header` (tests_recall_neighbour.rs, now 212 lines). Production code unchanged. Shape copied from mo1vm's test: Live mode, `recall_max_items = 2`, FakeJev scripted, rerank wait 600 ms. Candidates: the asked host's chunk and a same-kind chunk about another host (`host:db-stage...`), so only the note tells them apart.
- Refill proven to run: the test asserts `m.rerank.applied` (Jev's order applied, which is the `Memory::refill` path), and both chunks admitted.
- Asserts the near chunk's frozen header on the Recall node ends with the note, and the asked chunk's has none.
- Filter proven: `cargo nextest list -p theseus-core` shows `theseus-core tests_recall_neighbour::a_live_rerank_keeps_the_neighbour_note_in_the_frozen_header`; the run used `test(tests_recall_neighbour::) | test(tests_recall_current::)`, 3 tests run, 3 passed.

## Planted reverts
- **P6 (refill site)**, diff: `-        neighbour::apply(&mut m.admitted, &marks);` (recall.rs:911, in `refill`). Result: new test FAILED (assertion on the near chunk's header, tests_recall_neighbour.rs:206); the other two tests passed. Restored from copy, `touch`ed, `git status` showed only my test file modified; new test PASS again.
- **P2 control (run site)**, diff: `-        neighbour::apply(&mut m.admitted, &neighbour::marks(&candidates));` (recall.rs:869). Result: existing `a_recall_of_another_host_names_it_in_the_frozen_header` FAILED, so the old coverage is intact (run stopped at first failure, 91 passed 1 failed of 92 run). Restored, touched, `git status` clean apart from my test.
Red at base: none; no behaviour changed (the code is correct today).

## Gate / keel
- The VM's disk filled during the first `cargo build --workspace --all-targets` (19 GB target); I removed `target/debug/incremental` and built with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. I did NOT run the full `scripts/gate.sh`. I ran `cargo fmt --check -p theseus-core` (ok), `cargo clippy -p theseus-core --tests` (clean) and the recall tests above. cargo-deny not installed; deny phase not run.
- `python3 scripts/keel-guard.py` printed findings (test-removed, assert-removed, assert-weakened, allow-added, keel-file) on files that my commit does not touch (data.rs, tests_open_view.rs, courier.rs, and so on). They come from the range back to origin/main, which moved past my base (content-flags merge etc.), not from my diff, which only adds one test to tests_recall_neighbour.rs. **Keel findings expected from my change: none.**

## Live check for the maintainer
`cargo nextest run -p theseus-core -E 'test(tests_recall_neighbour::)'` shows 2 passed; delete line 911's `neighbour::apply` in `refill` and the new test fails.

## Not done / open
Full gate, cargo-deny. No OWNER CALL.

CLOUD REPORT COMPLETE
