# CLOUD_REPORT: voice-jev (theseus-s3dwj), voice layer 4

Branch `cloud/20261019-voice-jev`, on `main` at `74a797ed`. Commits: `2ce7fb3` (judge), `0f0d4c7` (voice, binding, config).

## What I found
- theseus-voice has no judge dependency, and none was needed. The engine takes a `Judge` trait (`Engine::with_judge`). The binding (theseus-discord, which already holds `Arc<Core>`) implements it over a new `JudgeService::judge_heard`. **No new dependency, no Cargo.lock change.** theseus-core re-exports `HeardInput`/`HeardOverlap` for the binding.
- `Transcript` carried no confidence. It now has `confidence: Option<f32>` (Deepgram's value is parsed). `Transcript`, `Utterance` and `Event` lost `Eq` (floats). Nothing needed it.
- `sentences` in a case are exactly what `heard.rs` matches: what ended within the echo tail, plus the one playing. So a case may carry fewer than 3.

## What I changed
**`2ce7fb3` judge:**
- Pack `heard.v1` (point `voice`, act 0.80, confirm 0.60, action none, WIRED live), builder `heard` (word count computed, scrubbed and clipped), goldens, `Point::Voice`, `Builder::Heard`.
- Core `judge/heard.rs`: state and blob staged, budget beside the call, verdict over a oneshot, every call a `judge.call` row scoped `judge:heard`, late ones too. `acts` needs the pinned model, a non-shadow mode and the act band on the chosen option.
- Labelled set `fixtures/eval/heard.v1.json`: 24 invented cases (echoes, fragments, answers, noise), with `eval_heard.rs` tests over the wire against the fake Jev, and `jev-probe --eval heard.v1`.
- Four pack-list tests and theseusd's `tests/judge.rs` gained `"heard.v1: live"`.

**`0f0d4c7` voice:**
- `unclear.rs` (`Judge`, `Case`, `Verdict`, `Judged`, `StandInJudge`) and the engine change. Asked only when the rules say `Words`, the utterance began in speech, the tail or the join, and it has at most 3 words or confidence under 0.80 (nova-3 gives clear speech 0.9 or more).
- The utterance is held `Waiting` for at most `Config::judge_ms` (250 ms) in a work future, so the floor and audio never wait.
- At the act band: `echo` becomes `HeardAs::Echo` and counts toward echo-proneness; `other` becomes the new `HeardAs::Other`; `addressed` is the rules' turn. Anything else, no answer or a timeout leaves the rules' verdict.
- The journal line names the judgment id. `Utterance::judged` and the `voice.utterance` row's `judged` carry id, answer, p, acted and ms.
- `[voice] judge_unclear = true` (rollback) and `judge_ms = 250` (1 to 2000, refusal names the switch), both in the template. Off means no judge is attached. Docs updated in the voice and discord AGENTS.md.
- Core `tests_heard.rs`: the live path against the fake Jev.

## How I proved it
- `cargo nextest run -p theseus-voice -p theseus-judge`: 294 passed (9 new in `tests/judged.rs`, 1 unit). `-p theseus-discord`: 566 passed at the time. Core filtered run (judge, inbound, continue, first_request, example template, voice, layouts, registry): 158 passed. Later: `tests_heard` (4) and the config test pass. `jev-probe --fake --eval heard.v1`: 24/24.
- fmt, `scripts/shape.sh`, and `clippy -p theseus-voice -p theseus-discord -p theseus-judge -p theseus-core --all-targets -D warnings` are clean, as is `clippy -p theseus-judge --features probe`.
- Planted reverts (each file restored and `touch`ed, `git status` clean):
  1. **KEY** `engine.rs` `judged()`: `(true, Said::Echo) => (HeardAs::Echo, …)` and `(true, Said::Other) => (HeardAs::Other, …)` changed to `HeardAs::Words`. Failed: `an_echo_the_judge_is_sure_of_is_no_turn_and_the_journal_names_the_judgment` (judged.rs:142, left `[(1.2s,"what's the plan?"),(5s,"Yeah.")]`, right `[(1.2s,"what's the plan?")]`), `noise_the_judge_is_sure_of_is_no_turn` (left 2, right 1), `a_long_transcript_with_low_confidence_is_asked` (Words vs Echo).
  2. Deadline: `tokio::time::timeout(deadline, judge.judge(case)).await.ok().flatten()` replaced by `judge.judge(case).await`. Failed: `a_judge_too_slow_costs_the_deadline_and_the_rules_decide`: the late echo decides and the second turn is missing.
  3. Ask on all: `unclear::asked` body replaced by `true`. Failed: the `unclear` unit test, `noise_the_judge…` (left 0, right 1), `a_long_transcript…` and `a_judge_too_slow…` (calls 2 vs 1). The output was cut by `head`. I expect `what_is_clear_or_far_from_the_speech_is_not_asked` to fail too but did not see it.
  4. Confirm band: `match (v.acts, v.said)` changed to `match (true, v.said)`. Failed: `an_answer_below_the_act_band_leaves_the_rules_verdict`.
  5. Drop the overlap condition (`true &&`). Failed: the unit test, `noise_the_judge…`, `a_long_transcript…`, `a_judge_too_slow…`.
- Off-switch: `with_no_judge_the_events_and_times_are_the_rules_alone` holds today's events and times. The binding-level `attach` off path has no test (the engine hides its judge), and the key itself is covered only by the config test.

## Live check (maintainer)
- Accuracy on live Jev: `THESEUS_JEV_KEY=… cargo run -p theseus-judge --features probe --bin jev-probe -- --eval heard.v1`. It prints each case, `accuracy N/M`, and the answers wrong at the act band (false turns or missed ones).
- On a live speakers call (scratch daemon, fresh state dir): talk over and just after Theseus's speech, then read the rows with `theseus judge log` (pack `heard.v1`: answers and `heard.ms`) and the `voice.utterance` rows' `judged` field. Turns saved are the utterances with `heard_as` echo or other and `acted: true`.

## Uncertain, and for the owner
- `judge.call` rows now carry `point: "voice"`. If a build older than this one reads them, it may not know the value; I did not check each reader. No store format bump: no stored record changed.
- An utterance waiting on the judge is still `Waiting`, so the hold's 3 s transcript bound could drop it if the transcript itself arrived about 2.9 s late. This is an edge case and I left it alone.
- A judged echo counts toward echo-proneness (two make a speaker's stop wait for words). That is my choice.
- One small unfinished cleanup: `echo_prone` is set after `unclear::case` via struct update. It works; it could be a parameter.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule passed. The **suite failed**: 7033 run, 6996 passed, 37 failed.
- 33 are the known L1 failures (theseus-sandbox contract and theseusd sandbox tests, root daemon, theseus-pv6i).
- 4 are theseus-discord tests that wait out the 100 s kill: `tests_task_threads::{a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it, a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end, react::in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing}` and `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule`. The thread is never created. They passed in my earlier discord run (566/566) but fail now, and `a_shared_place_refuses…` **also fails on a clean checkout of `74a797e`**, run in a worktree. So I do not think they are mine, but I do not know why they changed. Later runs of the two-file filter failed 4 to 6 tests (a fifth, `the_owners_message_for_an_ended_task…`, also failed), not just these four.
- I did not run `machine_checks` after the suite.
- The first gate attempt died of disk (the 28 GB `target`; deleted `incremental`, stale test binaries); the second ran fully.
