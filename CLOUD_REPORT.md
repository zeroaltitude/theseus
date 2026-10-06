# CLOUD_REPORT: voice-turns (theseus-9ln5, theseus-kpa7)

Branch `cloud/20261006-voice-turns`, from `a068f8f` (main at `acf2621`, store format 22, untouched). Started
05:34 UTC; report at 06:45 UTC.

| Commit | Step |
| --- | --- |
| `74750d9` | 1. Hold, then decide (theseus-9ln5) |
| `751789d` | 2. The floor (theseus-kpa7) |
| `141d19d` | 3. The module doc, and the crate's AGENTS.md (theseus-9ln5) |
| `c2fe6f7` | Two more tests: a cut acknowledgment isn't replayed; `Leave` cuts what's unsaid (theseus-9ln5) |

Files: `crates/theseus-voice/src/{engine.rs,heard.rs,lib.rs}`, `crates/theseus-voice/tests/{turns.rs,pipeline.rs}`,
`crates/theseus-voice/AGENTS.md`, and in theseus-discord only `pump`'s match and the tests' `heard()` in
`src/runtime/voice.rs`. No protocol, store, config, or dependency change; `Cargo.lock` is untouched. engine.rs is now
1,247 lines (no ceiling near).

## Step 1: hold, then decide (`74750d9`)

**Found.** `barge_in` cleared the whole queue at the 300 ms stop and bumped the synthesis generation, so a laugh, a
cough or an echo threw away the reply and everything queued behind it, though its transcript later came back empty
and never became a turn. `ended` popped the item of a stopped clip. Only the first item knew its reply's sentence
count, and gave it up at its first play.

**Changed.**
- `heard.rs` (new, pure, unit-tested): `words` (lower-cased runs of letters, digits, apostrophes, `’` read as `'`),
  `is_echo` (at least 2 words, at least 60% of the distinct words among the given sentences' words), `is_backchannel`
  (1 to 3 words, segmented into the task's list; "uh-huh" is the words `uh huh`), `is_resume` (the whole utterance is
  one of the task's phrases), and `classify(text, overlap, sentences)`: empty is `Wordless`; `Overlap::Speech` checks
  echo, backchannel, resume, else `Words`; `Overlap::Tail` (began within 1.2 s after the last queued sentence ended)
  checks echo only; `Overlap::None` is `Words`.
- engine.rs:
  - The 300 ms stop (`hold`) stops the clip as fast as before (same tick, same `Out::Stop`) but keeps the queue:
    `Hold { since, seq, what, into, why }`. The held item's `clip` is cleared at once, so its stopped clip's `Ended`
    doesn't pop it, and it keeps its audio. `synthesize_next` and `play_next` do nothing while held; a synthesis in
    flight finishes and keeps its audio (no generation bump).
  - At each VAD's first speech frame (its opening, not the pre-roll), `what_is_over` records an `Opening`: `over`
    (`Saying { what, sentence, text }` for the queue's front, playing, held or waiting; else `Preparing { turn }`
    when a turn is in flight; else `None`), the overlap, and the sentences it may echo (the one playing or held, the
    ones ended in the last 1.2 s, and any that start while it is open).
  - At the transcript (`heard`): `Utterance { over, heard_as }` is emitted for every transcript (echo included, since
    the pump books the transcription's spend from it); only `Words` becomes a turn. An echo makes its speaker
    echo-prone: their 300 ms stop is off for the call. Over speech, `Words` commits; anything else records the hold's
    `why` (the plainest heard: resume, then backchannel, then echo, then wordless).
  - `resume` (in `advance`): when held, no listed VAD open, and no over-speech utterance still waiting for its
    transcript: the hold ends, a cut acknowledgment is dropped (not replayed), and `Resumed { what, why, held }` is
    emitted; `play_next` then plays the cut item from its held audio.
  - `commit` (words over speech, a hold's words, a failed transcription over speech, and the late cut: words over
    speech with no hold): `cut(Words)` emits a `Cut` per reply or report queued, stops a clip still playing, and
    then `BargeIn { speaker, what, dropped }` with `dropped` as before (everything queued). A report among them goes
    back to the front of the reports from its cut sentence (`QueuedReport { sentences, first, count }`; reports are
    now split once, at `Command::Report`). With an empty queue (the reply already ended) nothing is cut: it is just a
    turn.
  - Items carry `index` and `count`; `last_heard` comes from a map of each queued reply's or report's last whole
    sentence.
  - The call's end (`Leave`, the handle's drop, or the seam's end) with anything queued emits `Cut { why: CallEnded }`.
- theseus-discord: `Event::Resumed { .. } | Event::Cut { .. } => {}` in `pump` (voice-heard writes their rows), and
  `over: None, heard_as: HeardAs::Words` in the tests' `heard()`.

**The existing test that changed:** `a_barge_in_stops_playback_within_300_ms`. Its stop is still at 2.3 s, 300 ms
into Eddie's speech, and the second sentence still never plays. Robin's 200 ms "hm" now has the transcript `hm` (the
stand-in's default `[utterance 0.2 s]` is words, which would now cut late at 2.4 s, while held). Its `BargeIn` moved
from 2.3 s to 3.3 s: it is the commit, at Eddie's utterance's close and transcript (2.6 s + 700 ms), the same moment
his turn starts, which the test still asserts. No other existing test changed.

## Step 2: the floor (`751789d`)

**Found.** `play_next` started a clip as soon as its audio was ready; only the acknowledgment and reports waited for
quiet, in `advance`.

**Changed.**
- Items carry `asked` (the call time they were queued) and `opens` (the first item of a reply, report, report back
  from a cut, or the acknowledgment, until it plays; inherited by its successor when its synthesis fails).
- A turn records each speaker's last speech end (`started + length`); when its reply arrives it is kept in `split`
  until the reply begins or is cut.
- `contends(p, item)`: utterance `p` closed at or after `item.asked` (it held the floor, or began over it), or `p` is
  by one of the turn's speakers and began within 1.5 s of their last speech end in the turn (`SPLIT_THOUGHT`).
- `play_next`: a front item that `opens` doesn't start while any listed VAD is open, or while a contending utterance's
  transcript is still due. So the reply waits for the transcript too, and plays if it isn't words.
- `words` (an utterance heard as words, not over speech): each waiting reply or report it contends for is
  superseded: `Cut { why: Superseded, heard: 0, into: 0 }`, no `BargeIn`; the utterance is the next turn.
- `reply`: a reply that arrives with a contending utterance already transcribed as words is superseded at once (a
  thought split by a pause, with the continuation closed before the reply came).
- `commit`: words over speech when nothing queued has begun (front `opens`, no hold) supersede everything queued
  instead of counting as a barge-in, so `voice.barge_in` rows stay real cuts.
- Turns still start while a reply plays (unchanged `start_turn`).

## Step 3: the module doc (`141d19d`)

engine.rs's module doc now says what turns, send and barge-in do (only words make a turn; the floor and the split
thought; hold, then decide; the late cut; the echo-prone speaker; a report back from its cut sentence). The crate's
AGENTS.md names `heard.rs` and `tests/turns.rs`, and that a test needing words or none gives the stand-in a
transcript.

## Two more tests (`c2fe6f7`)

`a_cut_acknowledgment_isnt_said_again` (a laugh over the chime: `Resumed` for the acknowledgment, and the reply, which
came during the laugh, plays when it closes, with no chime again) and `leaving_with_a_reply_unsaid_cuts_it`
(`Cut { CallEnded, sentences 3, heard 0, into 500 ms }` at the `Leave`).

## Proof

All tests in `crates/theseus-voice/tests/turns.rs` run through the seam in virtual time
(`#[tokio::test(start_paused = true)]`, WAV fixtures, the stand-in speech, `Config::new([EDDIE, ROBIN])`), each
asserting exact times. Eddie asks from 0 to 0.5 s; his turn is at 1.2 s, and the reply's first sentence plays from
1.2 s unless a test says otherwise.

| Test | What it asserts |
| --- | --- |
| `a_sound_with_no_words_pauses_the_reply_and_it_resumes_from_the_cut_sentence` | 400 ms laugh at 1.5 s, transcript empty: stopped at 1.8 s; S1 again from its start at 2.6 s; all 3 played; 3 syntheses; one `Resumed` (wordless, held 800 ms); no `Cut`/`BargeIn`; one turn; `Spoke` at the end |
| `its_own_sentence_heard_back_is_an_echo_and_one_word_never_is` | S1's words heard back: `Resumed` (echo) at 2.8 s, no turn, 4 clips; "Stop." over the same sentence: `Cut` (3, heard 0, into 600 ms) and `BargeIn` (dropped 3) at 2.8 s, and the next turn |
| `a_yeah_over_a_reply_goes_on_and_a_short_mm_hm_stops_nothing` | 600 ms "Yeah.": `Resumed` (backchannel) at 2.8 s; 200 ms "Mm-hm." at 3.5 s: no stop; both `Utterance`s carry `over: Saying(0, S1)` and `heard_as: Backchannel`; no turn |
| `a_yeah_after_a_closing_question_is_a_turn` | "Yeah." 0.5 s after "Should I deploy it now?" ended: a turn, `over: None`, `heard_as: Words` |
| `words_over_the_second_of_four_sentences_cut_it` | "wait, which account" from 2.3 s over sentence 2: stopped at 2.6 s; at 3.6 s `Cut` (4, heard 1, into 300 ms, both texts) then `BargeIn` (dropped 3); the next turn's utterance has `over: Saying(1, sentence 2)` |
| `go_on_after_a_stop_plays_on_from_the_cut_sentence` | A laugh stops it at 1.8 s; Eddie's "Go on." while held closes at 3.2 s: `Resumed` (resume, held 1.4 s); S1 from 3.2 s, then S2, S3; no turn |
| `a_short_no_cuts_late_at_its_transcript` | 200 ms "No." at 1.5 s: no stop at the VAD; at 2.4 s the clip stops, `Cut` (into 1.2 s) and `BargeIn`; over a reply that has already ended, just a turn |
| `a_failed_transcription_over_speech_commits` | A test-only `Speech` (`Deaf`) fails Robin's transcription: `Failed`, then `Cut` and `BargeIn` at 2.8 s; no turn |
| `a_report_cut_by_words_comes_back_from_its_cut_sentence` | A 3-sentence report from 0.1 s cut in sentence 2 by Eddie: `Cut` (Report, heard 1, into 300 ms), `BargeIn` (dropped 2); his turn is answered at 3.0 s; at the pause after (3.4 s) the report again from sentence 2, then 3; one `Speaking` for it |
| `after_an_echo_its_speaker_doesnt_stop_it_and_their_words_cut_late` | Echo stops it once; a second echo doesn't stop it; words over S2 cut late at their transcript (`Cut` heard 1, `BargeIn` dropped 2) |
| `a_reply_waits_for_the_speaker_and_is_superseded_by_their_words` | Reply ready at 3.0 s while Eddie talks 2.5 to 3.5 s: a cough, so it plays at 4.2 s (`over: Preparing(0)`, wordless); words, so it never plays: `Cut` (superseded, heard 0) at 4.2 s and his words are turn 1 |
| `a_thought_split_by_a_pause_gets_one_answer` | "Can you make yourself a" / 800 ms / "tool to order": turn 0's reply (at 4.2 s) superseded, "tool to order" turn 1, one answer played at 4.7 s; a new question 3 s after the last word doesn't supersede the answer (it plays at 6.2 s, the question is turn 1) |
| `a_cut_acknowledgment_isnt_said_again` | Laugh over the chime: chime stopped at 3.54 s, not replayed; `Resumed` (acknowledgment); the reply plays when the laugh closes |
| `leaving_with_a_reply_unsaid_cuts_it` | `Leave` 500 ms into S1: `Cut` (call ended, into 500 ms) |

heard.rs's 7 unit tests cover words, wordless anywhere, echo (60% boundary both sides, one word never), the
backchannel list (and 4 words, "yeah but no", "okay wait" refused), resume phrases (and "go on to the next one"
refused), every rule over speech, and echo-only in the tail.

**Runs.**
- `cargo nextest run -p theseus-voice`: 62 passed (41 before this branch, plus heard.rs's 7 and turns.rs's 14).
- Under load (AGENTS.md's recipe: the run at `nice -n 19`, four busy loops at nice 0, killed by their pids):
  5 runs of the crate's suite, 62 of 62 each time (about 20 s a run under load, 0.67 s unloaded). The same 5 runs at
  step 2 (60 tests) also all passed.
- theseus-discord: 122 of 122 (alone after step 1, and in each gate).
- clippy `-D warnings` on both crates: clean.

**Planted reverts** (each run with `cargo nextest run -p theseus-voice --no-fail-fast`, the file restored and
`touch`ed after, `git status` clean of it after each):

| Planted bug | Tests that failed |
| --- | --- |
| The stop clears the queue again (`self.queue.clear()` in `hold`) | the sound, echo, backchannel, "go on" tests, and also the four-sentence cut, failed transcription, report cut, echo-prone, and the pipeline's barge-in test (9) |
| Every transcript classified as words | the sound, echo, backchannel, "go on" tests, and also the echo-prone, waiting-reply, cut-acknowledgment, and pipeline barge-in tests (8) |
| Resume from the sentence after the cut (the held item always dropped) | the sound test (on order), and the echo, backchannel, "go on", echo-prone tests (5) |
| The late cut removed (over-speech words commit only when held) | `a_short_no_cuts_late_at_its_transcript`, and the echo-prone test (2) |
| Backchannels counted in the echo tail | `a_yeah_after_a_closing_question_is_a_turn`, and heard.rs's tail unit test (2) |
| `play_next` ignoring the VADs | `a_reply_waits_for_the_speaker_and_is_superseded_by_their_words` (1) |
| The 1.5 s window at 0 | `a_thought_split_by_a_pause_gets_one_answer` (1) |
| A cut acknowledgment replayed | `a_cut_acknowledgment_isnt_said_again` (1) |
| No `Cut` at the call's end | `leaving_with_a_reply_unsaid_cuts_it` (1) |

**FAST.**
- The stop is as fast as today: `tick` counts the same speech frames against the same `barge_frames` and pushes the
  same `Out::Stop` on the same tick (`hold` replaces `barge_in`). The pipeline test's stop is still at 2.3 s.
- A committed cut makes the next turn when today's code does: the commit runs in `heard` when the utterance's
  transcript arrives (`Done::Transcribed`), and `advance` → `start_turn` makes the turn in the same loop iteration,
  as the utterance's `Got` did before. The pipeline test's turn is still at 3.3 s. No reply's first audio moves,
  except by design under the floor (a reply now waits for a speaker who is talking).
- A resume replays the held `Item::audio`: `synthesize_next` is not called for it (it has audio), and the sound test
  asserts 3 syntheses for 3 sentences.
- The rules are a few string splits and set lookups per utterance, in `heard` at its transcript, in the voice engine's
  own task, off the core's turn path.
- Holding stops synthesis ahead: `synthesize_next` returns while `hold.is_some()`, and a resume needs none, so what a
  wordless stop used to discard (synthesized, never played) isn't made.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before each commit: fmt, shape, features, clippy,
cockpit, test build, reader rule all ok; the suite failed only on the 33 known L1 tests (VM runs as root,
theseus-pv6i): 20 `theseus-sandbox::contract` clauses and egress tests, `theseus-sandbox::bench spawn_100`, and 12
`theseusd::sandbox` tests. Nothing else failed and no retry was needed (2,859 run, 2,826 passed, 21 skipped at steps
2 and 3; 2,861 run, 2,828 passed at `c2fe6f7`). The phases after the suite, run by hand each time: protocol types ok (no protocol change); `bench
turn --check --runs 5 --burst 0`: 5 and 9 frames, budgets 5 and 9, ok; `cargo deny --offline check`: advisories,
bans, licenses, sources ok. The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.

## Live check (the maintainer's, after the join and an install)

In the test voice channel, with `theseus voice` and the ledger tail open beside it:
1. Ask for a long answer; cough and laugh over it. It stops within 300 ms and starts the cut sentence again when the
   sound's utterance closes (700 ms after it ends, plus the transcript). `voice.barge_in` rows: none for these.
2. Say "yeah" or "mm-hm" over it: a long one stops and resumes; a short one doesn't stop it; no new answer follows.
3. After a laugh, say "go on" while it is stopped: it goes on from the cut sentence, and no turn.
4. Say "wait, stop" over it: it stops; one `voice.barge_in` row; the next turn is the words.
5. Start a sentence, pause about a second, finish it: one answer, to the whole (the first reply is never voiced).
6. Keep talking as an answer becomes ready: it waits until you stop (and 700 ms), and if you said words it answers
   those instead.
7. On loudspeakers: no answer to its own words; after the first echo it stops only for words, cut at their
   transcript, so no stutter.

## Left, uncertain, and choices the owner should hear about

- **The floor applies to a reply's or report's first clip only** (and a report back from a cut), not between the
  sentences of one already playing. The task says `play_next` starts no clip while any listed VAD is open; applied
  between sentences, a speaker on loudspeakers (whose VAD hears Theseus) would hold every sentence boundary for
  700 ms, the stutter step 1 removes, and a short "mm-hm" over a sentence would hold the next one. Between
  sentences, an utterance that began over speech is decided by step 1's rules anyway.
- **What contends for a waiting reply:** any listed speaker's words that closed after it was queued, not only its
  turn's speakers ("any listed speaker's VAD"), plus the split-thought rule for the turn's own speakers. The split
  window is measured from the utterance's `started`, which includes up to 200 ms of pre-roll, so it is up to 200 ms
  generous.
- **An echo still emits `Event::Utterance`** (with `heard_as: Echo`): the pump books the transcription's spend from
  that event. "Dropped" here means no turn.
- **`Resumed.why`** when several utterances overlapped a hold: the plainest (resume over backchannel over echo over
  wordless). With nothing classified (cannot happen today: the stopping utterance is always one) it says wordless.
- **`Over::Saying`** is the queue's front: the sentence playing, held, or about to play in a gap (including a reply
  that hasn't begun). `Preparing` only when nothing is queued and a turn is in flight.
- **A failed transcription over a reply that hasn't begun** supersedes it (no barge-in): "a stop that wasn't heard"
  isn't talked over, and the utterance is no turn, so the session hears nothing of that reply's turn.
- **The call's end** emits `Cut { CallEnded }` at `Leave`, at the handle's drop, and at the seam's end, a test WAV's
  end included, not only a dropped connection.
- **A clip's natural end racing the stop** on songbird: if a sentence ended just as the stop was sent, its `Ended`
  is taken for the stopped clip's, and the sentence replays whole on resume. Harmless, and rare.
- **The backchannel list is exactly the task's.** "Mm-hmm" passes (as `mm` then `hmm`); "mhmm" and "uh huh huh" don't.
- No bench: the rules run per utterance in the engine's task, not on any measured path.

**Docs for the maintainer to change:** docs/design/m7-surface.md §2.8 Send ("stops the track and drops the rest"
becomes the hold and the decision; the floor and the split thought under Send or Receive; `Resumed` and `Cut` as
facts beside `voice.barge_in`, for voice-heard's rows); §3's test list (a barge-in's `BargeIn` now comes at the
commit); the spec's Part III item and `docs/status.md`.
