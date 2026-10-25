# CLOUD_REPORT: p3b-onto-never (theseus-648f8)

Branch `cloud/20261024-p3b-onto-never`, base `ac298666`. Work commit `60c98c7e`.

## 1. For the maintainer

- **Store format: 51, untouched.** The nevers are META records (an existing record kind), so there is no bump and no `tests_layouts` entry.
- **Ceilings:** none raised. `protocol/src/lib.rs` gained 0 lines (the three method constants went onto an existing line). New code is in new files: `rpc/never.rs`, `judge/small_talk.rs`, `tests_never.rs`, `judge/people/tests_never.rs`, `cockpit/src/lib/proposalActions.ts`, `cockpit/src/components/ProposalActions.tsx`.
- **No OWNER CALL, no STOP.**
- **The door and keel phases of `scripts/gate.sh` could not pass here.** `origin/main` (2d1d8d6f07) is far ahead of the base, so the door judges the tree against a newer main and reports erosions that are main's changes, not mine (persona_cmd tests, keel-guard). I ran the phases by hand instead (section 8).
- `[ontology] not_topics` is a new config key; the template and its tests are updated.
- Disk: this VM's quota ran out once (target 28 GB). Not a code issue.

## 2. Config keys and defaults

- `[ontology] not_topics = []` (default empty). Matching is by the proposed topic's name or local id, folded (`judge::people::fold`: lowercase, single-spaced, `@` sigils off), the same normaliser `not_people` uses. Template line added to the `[ontology]` block; test `not_topics_is_a_key_that_is_empty_by_default` covers default, parse, bad key and template.
- Small talk (`judge/small_talk.rs`): `MAX_WORDS = 6`; `PHRASES` (greetings, status checks, acknowledgements: "how is it going", "status", "any update", "what's up", "hi", "hello", "thanks", "ok", "good morning", and so on) and `FILLER` (there, you, so, much, a, lot, again, all, everyone, please, very, just). A message is small talk when it has at most 6 words and every word belongs to a phrase or a filler. One leftover word makes it substantive ("ok let's refactor the auth" is judged). An empty message is not small talk (it may carry a file).

## 3. The never record

- META key `ontology.never.<kind>.<name folded>`; kinds `topic`, `person`, `session` (a session's key is its id: never propose a new topic for it). Value: `{kind, name, who, via, at_ms, note?, undone_ms?}`.
- Wire keys (for undo): `topic:<folded>`, `person:<folded>`, `session:<sid>`.
- Written in the same frame as the rejection label (`reject_with`), so a never and its reject are one effect.
- RPCs (`protocol/src/ontology.rs`): `ontology.proposal.never {judgments, note?}` -> `{rejected, nevers, left}`; `ontology.never.list {}` -> `{nevers, config_topics, config_people}`; `ontology.never.undo {key}` -> `{undone}`. The never and the undo are `judge_act(Act::Ontology)` (the owner, from a private place), as `reject_all` is; the list is a read. The CLI refuses the two writes inside a job (`client::OPERATORS`).
- Undo writes the same record again with `undone_ms` set (the record only grows); a never set again later is a fresh record.
- Applied: `proposals_listed` hides a never'd topic or a new topic of a never'd session and counts it in `hidden`; person nevers join `Core::not_people` (so the existing `excluded` path hides and counts them); `prepare_categorize` removes never'd topics (config and stored) from the candidates it offers Jev.
- CLI: `theseus ontology never JUDGMENT...`, `--list`, `--undo KEY`.

## 4. Where the topic proposals on imported agent sessions came from

Not imports. An imported session is closed and never takes a turn, so `categorize.v1` (which runs only at a live turn's exchange end) never sees one; imports get people proposals only. The agent/subagent/bench rows come from LIVE sessions those agents open through the CLI (`theseus ask`), which `categorize.v1` judges like any private-place conversation. There is no reliable marker (a first line is the kind of hand-written detection AGENTS.md says cannot be finished), so I added no skip rule. Follow-up: tag sessions by the surface that opened them (`Node.surface` is `terminal:ask` for `ask`) or by `opened_from`, and skip those; for now the bulk Never on a session's unnamed new-topic row covers them. Not done, left for the owner's call.

## 5. Small talk

`JudgeService::skip_small_talk` (`judge/categorize.rs`) runs in `prepare_categorize` right after `due()` (before the heavier reads; same effect as after `input()`, cheaper). It looks at the human messages since the mark. If all are small talk it returns no judgment and no spend, and moves the mark in memory (key `skip.small_talk.<sid>` in the `unwritten` map, never written by a sink frame; a restart reads and skips once more). No re-trigger proof: `small_talk_is_not_judged_and_does_not_retrigger` waits for the skip, then shows the next exchange end reads fewer than 100 records (a mark left behind reads about 180+ in the planted run), and that only the later window with one substantive message is judged (exactly 1 judgment and 1 Jev call, after a 3 s wait; Jev calls take over a second on this VM, so an early absence check proves nothing). Note: with the window being the new messages since the mark, a status check in a long real session is also skipped; a window with any substantive message is judged as before. `categorize.v1.toml` is not edited; no goldens moved. Proposed line for a `categorize.v2` `none` option: "or the messages are greetings or status checks".

## 6. The cockpit

New files `lib/proposalActions.ts` (pure) and `components/ProposalActions.tsx`: per topic row a Reject (X) and Never (Ban) cell; a Never beside each person row's check/cross; bulk Reject N and Never N after the header's Accept; a collapsed "nevers" list in the panel footer with undo and the config's lists. Each is confirmed first through `writeEach`, with words saying what Never keeps ("Undo it under nevers"). An unnamed new-topic row reads `a new topic (unnamed)` and its Never says "never propose a new topic for this session". All buttons take `busy` and `disabled` from the panel.

`People.tsx` edits (7 insertions, 2 deletions): the import line; in `answer()` the topics call now picks `accept_all` or `reject_all` by `accept` (the single changed line, `...(topics.length ? [[accept ? ... : ...]`); `<BulkActions .../>` on the line right after the header's `Accept` `<Act>`; `<PersonNever .../>` after the person reject button; `a new topic` -> `a new topic (unnamed)`; `<TopicActions .../>` after the confidence cell; `<NeversList .../>` after the table block. The checkbox lines, `<Refused>` line and `OntologyParts.tsx` are untouched.

## 7. Tests and plants

New: `tests_never.rs` (stored never hides a topic and undo brings it back, counted in `hidden`; configured never topic not offered; new-topic never is the session's; small talk), `judge/people/tests_never.rs` (the word owner; a person never rejects, hides, undoes), `judge/small_talk.rs` unit tests, `config/tests_ontology.rs` (not_topics), `cockpit/test/proposalactions.test.ts`.
- P1 (drop the never-set from `proposals_listed`): `a_stored_never_hides...` and `a_new_topic_never_is...` FAIL.
- P2 (skip off): `small_talk_is_not_judged...` FAIL (the skip is never seen). P2b (skip without moving the mark): FAIL "179 records read after 910". My first version of this test passed under both plants; it was strengthened.
- P3 (owner out of `HOUSE`): `the_word_owner_is_not_a_candidate` FAIL.
All files restored and touched.

## 8. Gate

Done by hand (the door/keel phases fail on the base mismatch above):
- `cargo fmt --check`: ok. `scripts/shape.sh`: ok.
- `cargo clippy --workspace --all-targets -D warnings` and the judge `probe` feature: clean.
- cockpit: `npm run lint` (warnings only, as before), `npm test` 733 pass 0 fail, `npm run build` ok.
- `cargo nextest run -p theseus-protocol -p theseus-judge`: 419 pass. TS bindings regenerated and committed.
- `TZ=America/Phoenix cargo nextest run --workspace`: 8454 run, 8419 pass, 35 fail. Failures: the known L1/root ones (theseus-sandbox contract and `spawn_100`; theseusd sandbox tests incl. `the_jobs_bench_l1_row`), plus `theseus::markdown rendering_costs_little_per_hundred_lines`, a timing test that passes alone. Every theseus-core test passed, including the registry (reader rule) and the template tests.
- `cargo deny`: skipped (the advisory db was not fetched; offline).
- `scripts/recall-tests.sh`, `protocol types` phase (git shows no regenerated diff after the suite), benches: not run separately.

## 9. Keel

`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`: no output, exit 0.

## 10. DM golden

`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no diff.

## 11. Join notes: files touched

core: `config/ontology.rs`, `config/tests_ontology.rs`, `config/theseus.example.toml`, `judge/{categorize,mod,small_talk}.rs`, `judge/people/{mod,house,tests_never}.rs`, `rpc/{mod,never,ontology,ontology_kinds,people,proposals}.rs`, `lib.rs`, `tests_never.rs`. protocol: `lib.rs` (one line), `ontology.rs`, `ts.rs` (existing line). cli: `client.rs` (OPERATORS 107 -> 109), `main.rs`, `onto_cmd.rs`, `ontology.rs`. cockpit: `People.tsx`, `lib/proposalActions.ts`, `components/ProposalActions.tsx`, `test/proposalactions.test.ts`, `protocol.gen/*`. `reject_open` now delegates to `reject_with`; `listed_now` and `not_listed` became `pub(super)`.

## 12. Follow-ups

Group proposals by category; skip sessions opened by agents via `ask` (section 4); a `categorize.v2` prompt line (section 5); the owner's one-time backlog reject with the bulk buttons; the person-never uses `not_people`'s exact-name rule, so "Sable" does not cover "Sable Thorn".
