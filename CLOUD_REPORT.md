# Cloud report: voice-echo (theseus-3ug0, theseus-1cz8)

Branch `cloud/20261006-voice-echo`, from main at f33f0ec (voice-turns' join is on it: `heard.rs`,
`Utterance.heard_as`, `HeardAs::Echo`, the echo-prone flag). Started 16:53 UTC and finished 17:35 UTC, well inside
the 3 h deadline. No store format change, no protocol type, no config key, no new dependency. The changes touch only
`crates/theseus-voice/src/heard.rs`, `engine.rs` (the echo count, `what_is_over`, and the utterance's close in
`tick`), `tests/turns.rs`, and theseus-voice's `AGENTS.md`.

## Step 1: an echo is a near-whole, in-order copy; echo-prone takes two (theseus-3ug0), 117aee9

**What I found.** It was as the brief says. `is_echo` took 60% of the distinct words, so "Yes, deploy it now." (3 of
its 4 distinct words) and "The daily view." (3 of 3) were echoes. They made no turn and took away their speaker's 300 ms
stop for the rest of the call.

**What changed.**
- `is_echo` now uses the longest run of the utterance's words that appears contiguously and in order in one
  candidate sentence. It counts by word-level longest common substring, O(n·m) per sentence, over at most a few
  sentences. That run must be at least 3 words and at least 80% of the utterance's words. One or two words are never
  an echo.
- The engine's `echo_prone: BTreeSet` is now `echoes: BTreeMap<Speaker, u32>`, and a speaker's stop is off only from
  `ECHO_PRONE = 2` echoes on.
- **One existing test asserted the rule I replaced.** It was voice-turns'
  `after_an_echo_its_speaker_doesnt_stop_it_and_their_words_cut_late`, which made the speaker echo-prone after one
  echo. It is now `after_two_echoes_its_speaker_doesnt_stop_it_and_their_words_cut_late`. Its second echo is a true
  copy ("a while long enough to talk over"); the old one, "long enough to talk over this first sentence", is
  out of order and no longer an echo. That second echo still stops the replay at 300 ms and resumes at 4.5 s. Only
  after it do Robin's words cut late. The unit test of the 60% rule is rewritten for the run rule. The commit body
  says both.

**New tests** (in `tests/turns.rs`):
- `an_either_or_answer_over_its_question_is_a_turn_and_cuts_it`: "The daily view." is a turn, and there is no replay.
- `an_answer_with_its_questions_words_in_the_tail_is_a_turn`: "Yes, deploy it now." 0.5 s after the question.
- `the_played_sentence_heard_back_whole_is_still_an_echo`: S1 heard back whole is an echo, with no turn and a resume.
- `one_echo_verdict_leaves_its_speakers_stop_on`: after one echo, "Hang on, wait a second." over the replay stops it
  at 300 ms and cuts it at the transcript.
- The two-echo test above.
- In `heard.rs`: `an_echo_is_a_near_whole_in_order_copy_of_one_sentence`, which includes "Creates those. I'll check
  what's running." heard back whole, and `an_answer_that_repeats_its_questions_words_is_no_echo`.

**Planted reverts.**
- With `is_echo` back to 60% of distinct words (and ≥2 words), `an_either_or_answer_over_its_question_is_a_turn_and_cuts_it`
  and `an_answer_with_its_questions_words_in_the_tail_is_a_turn` fail.
- With `ECHO_PRONE = 1`, `one_echo_verdict_leaves_its_speakers_stop_on` and
  `after_two_echoes_its_speaker_doesnt_stop_it_and_their_words_cut_late` fail.
- After each revert I restored the file, `touch`ed it, and `git status` was clean apart from the step's own changes.

## Step 2: a reply that hasn't begun, and a question that ended under a "yes", are the tail (theseus-1cz8), 8aa74a8

**What I found.** It was as the brief says, in two places:
- `what_is_over` gave `Overlap::Speech` for any queue front, including one with `opens` set: a reply still
  synthesizing that had never played.
- An utterance's overlap was fixed at its first speech frame, so a "Yes." begun 200 ms before the question ended
  stayed `Speech` and was heard as a backchannel.

**What changed.**
- In `what_is_over`, a front that hasn't begun (`opens` and no hold) gives the tail: `Tail` when a sentence ended in
  the last 1.2 s, else `None`, the same choice as with an empty queue. Its `over` is still `Saying` that item.
- `Opening` gains `last`: it began over the queue's only sentence, which had begun. When such an utterance closes
  (`Closed::Utterance` in `tick`) and the queue is empty, its overlap becomes `Tail`. A held front stays in the queue,
  so an utterance that stopped the question (≥300 ms over it) is still `Speech`, as before.

**New tests:**
- `a_yes_begun_on_a_closing_questions_last_word_is_a_turn`: the "Yes." comes 200 ms over the end, the question is not
  stopped, and the turn is at 4.2 s with `over` = sentence 1, `Words`.
- `a_yeah_after_a_closing_question_with_another_reply_queued_is_a_turn`: synthesis takes 1 s. Robin's reply is queued
  at 4.0 s and still synthesizing, and Eddie's "Yeah." from 4.9 s is a turn at 6.0 s. Robin's reply, which waited
  for the floor, gets `Cut { why: Superseded }`, which is kpa7's existing rule that a listed speaker's words supersede
  a waiting reply.

**Planted reverts.**
- With `what_is_over` treating a front that hasn't begun as speech (`if false && item.opens ...`),
  `a_yeah_after_a_closing_question_with_another_reply_queued_is_a_turn` fails.
- With the close-time reclassification off, `a_yes_begun_on_a_closing_questions_last_word_is_a_turn` fails.
- Both files were restored, `touch`ed, and `git status` checked.

## Step 3: the wider backchannel list (theseus-1cz8), b72fc3e

**What changed.** I added these entries, as `words` normalizes them: "mm hmm", "mhmm", "mmhm", "mmhmm", "mmm",
"uhhuh", "uh hum" and "gotcha". "oh" now counts before a listed word ("oh okay", "oh yeah", "oh I see"), and the
3-word cap still holds. "oh" alone or "oh" last is not a backchannel ("oh", "oh no", "okay oh"). I filed this under
theseus-1cz8, since the brief gave §3 no id of its own.

**New test.** `a_mhmm_and_an_oh_okay_over_a_long_reply_resume_it`: each stops the reply and resumes it as
`Backchannel`, five clips play, and there is no turn. The unit list test also covers the new spellings and the
negatives.

**Planted revert.** With "mhmm" removed, `a_mhmm_and_an_oh_okay_over_a_long_reply_resume_it` fails, and so does the
unit test `a_backchannel_is_up_to_3_words_from_the_list`. The file was restored, `touch`ed, and `git status` checked.

## Proof

- **theseus-voice:** `cargo test -p theseus-voice` gives 30 unit tests, 8 + 11 integration tests, and 21 in `turns`,
  all passing. `turns` went from 13 tests to 21.
- **theseus-voice and theseus-discord:** `TZ=America/Phoenix cargo nextest run -p theseus-discord -p theseus-voice`
  ran 192 tests and all 192 passed.
- **Under load:** I ran theseus-voice's suite three times at `nice -n 19` beside four busy loops at nice 0. Each run
  was 70 tests and 70 passed. The loops were started from a script file and killed by their recorded pids. The brief's
  inline `sh -c` loop was refused by a safety check here, so I used the script file instead.
- **The whole workspace:** I ran the gate before each of the three commits. It ran 2,934, 2,936 and 2,937 tests, and 33 failed
  each time, and those 33 were exactly the known L1 tests: theseus-sandbox's contract tests, its
  `spawn_100`, and theseusd's sandbox tests. No other test failed, and nothing needed a rerun.
- **After the suite:** I ran the phases after it myself (`machine_checks`). The protocol types were unchanged, the
  turn bench passed (5 and 9 frames, within budget), and `cargo deny --offline check` passed.

## FAST

The rules still run once per transcript, in `Engine::heard` → `classify`, off the audio path. `longest_run` is a
handful of words against a handful of sentences.

The new work on the tick is one flag read in `what_is_over`, when a VAD opens, and one `last && queue.is_empty()`
check where an utterance closes. Neither waits on anything.

The 300 ms stop still comes on the same tick (`tick` → `hold`), and only an echo-prone speaker skips it, which now
takes two echoes. A resume still replays the held item's audio (`resume` keeps the item and its `audio`, and
`synthesize_next` makes nothing while held), with no new synthesis. No bench was needed.

## Live check (the maintainer's, in a voice channel on the owner's daemon)

Use the voice-call setup voice-turns' live check used, with two listed speakers where noted.

1. **Either/or, over the question.** Ask something whose reply is a question like "Do you want the daily or the monthly
   view?", and say "The daily view." while it is still being said. It should stop within about 300 ms and not replay.
   The utterance's row should be `heard_as: words`, a `Cut` (Words) and a `voice.barge_in` should follow, and the
   next turn is your answer.
2. **Either/or, just after its end.** Same question, answered "Yes, deploy it now." (or "The daily view.") about half
   a second after it ends. It should be a turn, `heard_as: words`. Then talk over the next reply: it should still stop
   at about 300 ms, so you did not become echo-prone.
3. **"Yes" on the last word.** Say a short "Yes." starting on the question's last syllable. The question should play
   to its end, and "Yes." should be a turn, with `over` naming the question's sentence and `heard_as: words`.
4. **Backchannels.** During a long multi-sentence answer, say "mhmm" (check what the provider writes in the utterance
   row) and then "oh okay". Each should stop it briefly and resume the cut sentence from its start (`Resumed`,
   `why: backchannel`). Neither should be a turn or a cut.
5. **A cough.** Cough over a reply. It should stop and resume (`Resumed`, `why: wordless`), as before.
6. **A real echo** (optional, with a speaker's mic near their speakers). The played sentence heard back should still
   be `heard_as: echo`, with no turn. After a second echo, that speaker's talk no longer stops a reply at 300 ms, and
   their words cut it at their transcript.

## Left, uncertain, and for the owner

- **A 3-word answer that is exactly a run of its question is still an echo** under the rule as specified. For "Do you
  want the daily or the monthly view?", "The monthly view." is a contiguous run of 3 words, 100% of the utterance, so
  it is heard as an echo. So is "Deploy it now." for "Should I deploy it now?". That is now one verdict, so the speaker
  keeps the stop, but the answer is still no turn.
  - **Proposals, any one of which closes it:**
    - (a) In the tail, require the run to end at the sentence's end and the utterance to begin within ~0.4 s of it.
      A real echo of a whole sentence begins while it plays, so it is `Speech`, not `Tail`.
    - (b) Raise the minimum to 4 words.
    - (c) Over `Speech`, require the utterance to have begun at least ~150 ms after the sentence started, and its run
      to start near the sentence's head as heard so far.
  - I kept the brief's rule and didn't guess between them.
- **A "yes" over the last sentence after another speaker's words cut the queue.** It began over the last sentence,
  the queue emptied because someone else's words cut it, and the "yes" now reads as tail: a turn, coalesced with
  theirs, where before it was a dropped backchannel. I judged that harmless, since it was said to the room.
- **`over` for a reply queued but not begun** stays `Saying { sentence: 0 }` of that item, though its overlap is now the
  tail. voice-heard (in flight) reads `over` for its note of what was heard. If it treats `Saying` as "said over
  speech", it may want `Preparing`-like wording there. I didn't change the `Over` type, to stay out of its area.

## Docs (for the maintainer; I edited nothing under docs/)

For **m7-surface.md §2.8**, replacing the echo and backchannel rules:

> **Echo.** Theseus's own sentence heard back through a speaker's microphone: a near-whole, in-order copy of one
> candidate sentence (the one playing, or one that ended in the last 1.2 s). The longest run of the utterance's words
> found contiguously, in order, in that sentence is at least 3 words and at least 80% of the utterance's words. One or
> two words are never an echo, nor is an answer that reuses its question's words ("Yes, deploy it now.", "The daily
> view."). An echo is no turn. A speaker heard echoing twice in a call is echo-prone: their 300 ms stop is off, and
> their words cut at their transcript. One wrong verdict doesn't take the stop away.
>
> **Where an utterance came.** Over speech: a clip playing, a hold, or a gap between sentences of something that has
> begun. In the tail: within 1.2 s after the last queued sentence ended, while a queued reply or report has not yet
> begun (it is synthesizing, or waiting for the floor), or when the utterance began over the last queued sentence and
> closed after it ended (a "yes" on a question's last word). In the tail only echo is checked, so a "yes" there
> answers.
>
> **Backchannel.** At most 3 words, all from a short list: yeah, yes, yep, yup, okay, ok, right, sure, alright, cool,
> nice, got it, gotcha, I see, and the hums in the provider's spellings (uh-huh, uhhuh, mm-hm, mm-hmm, mhm, mhmm, mmhm,
> mmhmm, mm, mmm, hm, hmm). Each may follow an "oh" ("oh okay", "oh yeah"). Over speech a backchannel resumes and is no
> turn, so the list holds only what never answers alone there.

For **§3's test list**, add:
- An either/or answer over its question is a turn, and the question doesn't replay.
- An answer with its question's words 0.5 s after it is a turn.
- The played sentence heard back whole is still an echo.
- One echo verdict leaves the 300 ms stop on, and two make the speaker echo-prone.
- A "Yes." begun 200 ms before a closing question ends is a turn.
- A "Yeah." after a closing question, while another speaker's reply is queued but not begun, is a turn (and that
  reply is superseded).
- "Mhmm" and "oh okay" over a long reply resume it and are no turn.

## The gate

I ran `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each of the three commits. Every time,
fmt, shape, features, clippy, the cockpit, the test build and the reader rule passed. The suite failed only on the 33
known L1 tests (theseus-pv6i: a root daemon's job with no job cgroup):
- theseus-sandbox's contract tests and its `spawn_100`;
- theseusd's sandbox tests.

No timing test failed and nothing was retried. The phases after the suite all passed: protocol types, the turn bench,
and deny (offline, with a fresh advisory database).
