# CLOUD_REPORT: tail4-route (theseus-4qdh8, theseus-rcb2u)

Branch `cloud/20261021-tail4-route`, base `8260dbab`. Store format unchanged (49); no stored field, no new dependency, no
protocol type, no config key. No file went over a ceiling.

## theseus-rcb2u: the place digest never rides an open request (8a01342)
- Found: `tail_blocks` gated the task view and project slice with `(!open)`, but `digest_line` ran for any turn.
- Changed: `crates/theseus-core/src/turn/compile_step.rs`: `if !open { Self::digest_line(..) }`, with a comment.
- Test: `rpc::tests_place_digest::an_open_request_carries_no_place_digest` (same `place` turn in a thread's session
  shows a digest, then none once the session is opened with `tests_open_view::open`).
- Plant: `if !open` -> `if true`: the test failed at `assertion left == right failed: an open request`.

## theseus-4qdh8: title evidence in the pre-router (c665b69)
- Found: the pre-router scores working-set entities only, so a message naming two threads by title scored both 0.0.
- Changed: new `thread_title.rs` (pure; `title_hit(text, title) -> Option<f64>`, `TITLE` = 0.35) plus one `mod` line
  in `lib.rs`; `thread_route.rs::pre_route` calls it, adds it to the noisy-or beside `entity` (not discounted by
  `SHARED`) and puts the matched title in `hits` (so the `thread.routed` row shows it; `Pre` is unchanged); module doc
  paragraph 2; one sentence in `crates/theseus-core/AGENTS.md`. Tests live in a child module of
  `tests_thread_route` (`rpc/tests_thread_route/title.rs`) behind one `mod title;` line, so the end of that file,
  which integ/threads round 4 touches, is not edited.
- **Title rule.** A title is named when the message holds every content word of it, in any order, and the title has
  at least two content words. Words fold by lower case, splitting on non-alphanumerics (hyphens), and a plural `s`
  off (words over 3 letters not ending `ss`). Stop words: a, an, and, at, by, for, from, in, of, on, or, the, to,
  with. "notes" and "work" are content words. A title with one content word never hits (a lone common word is a topic
  many threads share).
  I chose "every content word, any order" over "a contiguous phrase": the rehearsal's message says "the word census
  of the sample docs", which is not a phrase of "Docs word census".
- **Strength 0.35**: above `CONTEND` (0.25) so one title named contends, below `CONFIRM` (0.60) so a title alone never
  routes. Fused with Jev's p=0.29 pick (`1-(1-0.8*0.35)(1-0.29)` = 0.49 for the pick; 0.85*0.35 = 0.30 for the other),
  both contend and the top is under 0.60, so the router asks. Before: both pre scores 0.0, `how=home`, `ask=null`.
- Tests (in `rpc/tests_thread_route/title.rs` and `thread_title::tests`):
  - `a_message_naming_two_threads_by_title_asks_which` (the rehearsal's message and threads, Jev 0.29 docs-census): asks,
    question names both titles. Plant: `title_hit` returns `None`: the test failed (30 s timeout waiting for the ask).
  - `a_plainly_new_request_still_runs_at_home` (Jev 0.41): home, no ask. It passes with the plant below too, since
    one lone-word hit gives only one contender; the guard for "a shared word is no hit" is the unit test:
    plant `all` -> `any` and the length rule dropped: `a_lone_common_word_is_no_hit` failed.
  - `title_hit` unit tests: phrase, content words in another order, plural/case/hyphen folding, lone word, one-word and
    all-stop-word titles.
- The e31an tests (`a_new_request_no_thread_claims_runs_at_home_without_asking` and the ask tests) pass unchanged.

## Known limit
`apart_by_dir` recomputes a out-of-directory thread's score from its entity hits only, so a title hit does not
survive the directory rule (the message "comes from somewhere else"). I left `apart_by_dir` alone because integ/threads
round 4 changes it. Say so if the owner wants a title to survive it.

## Proof
- `TZ=America/Phoenix cargo nextest run -p theseus-core thread_title by_title place_digest plainly_new`: all pass.
- Gate (`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TZ=America/Phoenix
  THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`; the debug-info settings were needed because this VM's disk allowance ran
  out twice): door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build, reader rule all
  ok. Suite: 7825 run, 7791 passed, 34 failed, all in the known L1 set (`theseus-sandbox::contract` 38 lines incl.
  retries, `theseus-sandbox::bench`, `theseusd::sandbox` 28). The phases after the suite (protocol types,
  lifecycle/jobs/turn/size benches) I did not run: this row touches no protocol type and the budgets are the owner's
  machine's. The gate's first clippy run had caught one `unused_async` in my test helper; fixed before the commit.
- Keel guard `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty output, exit 0.
- Commits are not signed (no key on this VM).

## Live check for the maintainer (after install)
On a scratch daemon with two young threads titled "Docs word census" and "Harbour log notes", send the rehearsal
message: expect a `thread.asked` "Is this about the Docs word census or the Harbour log notes?" and a `thread.routed`
row whose `hits` hold both titles. Then send "start two tasks: a census of the disk, and a slow load": expect `how=home`,
no ask.

## docs/status.md sentence
"A message that names a live thread by its title (every content word of it) gives that thread 0.35 pre-router
evidence, so two titles named ask which; an open request carries no place digest (theseus-4qdh8, theseus-rcb2u)."

## Follow-ups
Title evidence in the directory rule (above). The chain term (`hitting == 0`) still ignores title hits.
