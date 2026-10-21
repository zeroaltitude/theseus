# CLOUD_REPORT: onto-r5-extras

Branch `cloud/20261021-onto-r5-extras`. Store format unchanged (no bump).

## Files over a line ceiling

None. The gate's shape phase passed.

## Gate result (honest)

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on head `174d6bd`:
- Passed: door, keel, keel tests, fmt, shape, features, clippy (workspace), clippy-probe, cockpit.
- **Failed in "test build"**: `No space left on device`. The VM's disk allowance cannot hold the whole workspace's test build. The suite and later phases did not run.
- Instead I ran `cargo nextest run -p theseus-core -p theseus-judge -p theseus-ontology --no-fail-fast` (3,896 tests) on the parts 1 to 3 tree: 3,891 passed, 5 failed. Four pinned the full list of wired packs (`tests_judge` x2, `tests_inbound`, `tests_continue`); I added `same_thing.v1: live` and `two_things.v1: live` to their expected lists (`INBOUND_ALONE` 17 to 19); they pass. The fifth, `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`, failed once in that run and passes alone (not in my code). After the guidance, digest and refactor commits I re-ran 92 affected tests (guidance, onto, consolidate, initiative) green and `cargo clippy -p theseus-ontology -p theseus-judge -p theseus-core --all-targets -D warnings` clean. The full suite was **not** re-run on the final head: the maintainer should.
- Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, rc 0.

## What changed

1. **`other` is recorded (theseus-81zx7), 5205b78.** `Step::Other { names, confidence }` in `decide.rs` for `kind_of_thing = other` at the act band (at most ten mined names); `onto.filed` row `{"kind":"kind_of_thing","outcome":"other","names":[...],"confidence":c}`; no record, card or footer. A plan with only this step is not empty. Such a turn's frame gains one `onto.filed` row; a listed kind, or other below the act band, writes nothing new. Tests `tests_onto_other.rs`. Plant (act check lowered to confirm): `other_below_the_act_band_is_not_recorded` failed.
2. **The sweep mints a kind (theseus-81zx7), 92424f8.** `judge/onto/mint.rs`: `other` rows of the last 7 days (the design says "the day's"; three sessions a day is rare); 3+ distinct sessions sharing a folded name mint one write `sweep-<ms>-<n>-kind` (kind, one category, memberships). A bad or held name mints nothing and is counted (`Swept.refused`); names a kind lists in `examples`, an undone kind's too, are not clustered again. `Ontology::undo` already had the Kind-with-null-before arm. Tests `tests_onto_mint.rs`. Plant `CLUSTER = 2`: `three_other_sessions_mint_a_kind_and_two_do_not` failed.
3. **Model-judged merge and split (theseus-nzzz4), f20d321.** Packs `same_thing.v1`, `two_things.v1` (0.85/0.60), builder `builders/sweep.rs`, goldens, fixtures, wired live; per-item Nouls ask at most ten, so `in_first` covers the first ten sampled sessions and `in_first_more` the rest; the "first" thing is the one session 1 is about. Crate: `plan_pair_merge` (the survivor rule extracted from `plan_merges`, a pure move), `merge_pairs`, `split_candidates`, `plan_split`, `Step::Split`, `Category::by_machine`. Core `judge/onto/judged.rs`, run by the tender; `[ontology] sweep_usd_per_day` 1.0 (0 turns the model part off, the deterministic sweep still runs), in `config/ontology.rs` and the example template. `Swept` gains `judged_merges`, `splits`, `kinds`, `spent`. Tests `tests_onto_judged.rs`, crate `tests_judged.rs`, a config test; key test `the_sweep_merges_a_pair_jev_calls_the_same_thing` (0.9 merges, undo restores the ontology equal, 0.7 writes nothing). Plants, each shown failing: act band dropped (key test failed); pair builder only folded pairs (key test failed); cap check removed (`the_judged_sweep_stops_at_its_day_cap` failed); all sampled sessions to the first child (`a_large_topic_splits_into_two_named_children` failed). The zero-cap test compares merge count and ontology size with a run of the deterministic sweep alone, not ledger bytes.
4. **The dream writes guidance (theseus-olt3h), 7e45c17.** `consolidate/guidance.rs`, one call in `consolidate/tender.rs` after consolidation's run. Machine-written category of an interpreted kind, no guidance, 5+ private non-task non-import sessions; 3 a night, most held first; profile by `synth_profile`'s rule (`Core::session_profile`, a pure move out of `run.rs`); stops before `[memory] synth_limit_usd_per_day` counting consolidation's spend and its own (META `onto.dream.last`); an answer empty or over 400 characters is refused, never cut; write id `dream-<ms>-<n>`. It changes the compile's bytes for that category's sessions at their next recompile. Tests `consolidate/tests_guidance.rs`. Plants: truncation to 400 (over-400 test failed), limit ignored (limit test failed).
5. **The morning note lists the writes (theseus-0umqz), 45fb035.** `judge/onto/digest.rs`: pure `lines` over the day's `ontology.*` rows, one line a write with `· undo: theseus ontology undo <id>` or `· undone`. No `ontology.swept` row, no new `LedgerKind`. `morning_facts` counts the writes as one more item; `morning_note` appends the lines verbatim after the voiced words, stand-in path too. Tests `tests_onto_digest.rs`. Plant (undo id dropped): the key test and the note test failed.
6. **Docs, 174d6bd.** The ontology bullet of `crates/theseus-core/AGENTS.md`. The spec and `docs/status.md` should say: the five behaviours, `[ontology] sweep_usd_per_day`, no store format change.

## Design choices for the owner
- **No card goes out at night** for the mint, merge, split or guidance. The pacer (`pacer.rs`) is a minimum gap, not quiet hours, so consolidation's path would post at night; per the brief the lines go in the morning note instead. A persona at `quiet` initiative gets no morning note, so those writes then show only in the ledger (`theseus ontology undo` still works).
- Reversible defaults: 7-day `other` window; 3 sessions mint; 3 kinds a run; 12 sessions to ask a split; at least 6 titled sampled sessions; 20 pairs and 3 splits a run; a pair or category asked is not asked again for 30 days, whatever the answer (META `onto.sweep.judged`); `sweep_usd_per_day` 1.0; dream: 5 sessions, 3 a night, 12 sampled, 300 characters, 250 max tokens.
- "Oldest pairs first" has no timestamp; pairs are ordered by the newer category's earliest membership.
- Adding two packs to `WIRED` touches five tests that list every wired pack; other branches adding packs will conflict there (trivial).

## For the maintainer
- `cargo nextest run -p theseus-ontology -p theseus-judge -p theseus-core`, then `scripts/gate.sh`.
- Live check on a scratch daemon with a real Jev key: three sessions whose messages fit no listed kind, then a restart (the first sweep runs 10 minutes after a start); `theseus ontology` shows the new kind; `theseus ontology undo sweep-<ms>-<n>-kind` retires it. `theseus judge list` shows `same_thing.v1` and `two_things.v1` calls.

## Follow-ups
Rename, the `same_kind` Noul and Jev naming a kind stay theseus-63tfk.1.
