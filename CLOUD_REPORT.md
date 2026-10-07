# Cloud report: cloud/20261006-voice-holds

Branch `cloud/20261006-voice-holds`, from `main` at ea34457e (task commit f3c70fc1). Started 2026-10-06 22:43 UTC;
code done 2026-10-07 00:18 UTC. Main holds voice-echo's `ECHO_PRONE` (engine.rs) and voice-heard's `speakable`
(sentences.rs), so the precondition held.

Everything is in theseus-voice: `src/engine.rs`, `src/heard.rs`, `src/sentences.rs`, `src/vad.rs`, `tests/turns.rs`,
and its `AGENTS.md`. No protocol, config-file, store, discord or core change. No new dependency. No stored record
changes, so `MANIFEST_FORMAT` stays 23. The types the binding destructures (`Event`, `Cut`, `Resumed`, `CutWhy`,
`HeardAs`, `Failure`) are unchanged. `Config` gains two pub fields; the binding builds it with `Config::new`, so
nothing outside the crate changes.

Commits, oldest first:

| commit | issue | what |
|---|---|---|
| c4f3147e | aq4t | the hold's bound: a hold's transcripts due 3 s after the last utterance closed decide as failed |
| ec411738 | zcxx | `speakable()`: six shapes fixed, each with its test, and the `+`/`•` bullets tested |
| 62159cf0 | aq4t | the floor's bound: 8 s, then one probe of the open utterance's audio so far |
| 229d46d5 | e6mj | two rules pinned: another speaker's words supersede a waiting reply; `Leave` while held |
| 6af519b7 | qrwx | a report still waiting at the call's end gets its `Cut { CallEnded }` |
| a1a0ed9e | j2ut | the joined run, and the 2-word head-run rule |
| 452faec2 | q4pc | a "yes" on a closing question's last word is a turn with another reply queued behind it |
| 222a389b | e6mj | the per-speaker echo count, pinned |
| c7099112 | zcxx | the echo judged against what was played: three regression tests |

theseus-voice's suite went from 85 tests to 103 (turns.rs from 23 to 40).

---

## 1. aq4t: the hold's bound (c4f3147e)

**Found.** As the brief says: `resume` waits while any VAD is open or a Speech-overlap utterance is still
`Transcribed::Waiting`, and nothing counts time. A transcript that never comes holds the reply for the whole call. A
provider that fails at 20 s commits the cut 20 s after the laugh closed. `heard` acted on any result for a pending
`seq`, whatever its state.

**Changed.** A new field, `Config::transcript_bound`, defaults to 3 s in `Config::new`. `Engine::overdue` runs first in
`advance`. Under a hold, if the Speech-overlap transcripts are still due `transcript_bound` after the *last* of them
closed (call time, by ticks), each is marked `Dropped`. Each emits a
`Failed { Transcribe(speaker), "no transcript 3000 ms after the utterance closed" }`, and then the cut is committed
(`commit`: `Cut { Words }`, `BargeIn`).

- **Commit, not resume.** That's what a failure does today, for the same reason: a "stop" that wasn't heard must not
  be talked over. A wrong commit costs a cut reply, and the session keeps the text and gets a `Cut` note. A wrong
  resume talks over someone who asked it to stop.
- **A transcript after the bound is dropped, as a failure's is.** `heard` now acts only on a pending that is still
  `Waiting`. So a late transcript emits nothing (no `Utterance`, no turn), and the provider's own failure later emits
  no second `Failed`.
- **Why 3 s.** A real STT round trip on an utterance of 30 s or less is well under a second (45a's live checks). 3 s
  is far past a slow answer and far short of the 20 s request bound, and it is the brief's number.

**Proved.**
- `a_held_reply_whose_transcript_never_comes_is_cut_at_the_bound`: the laugh from 1.5 s stops the reply at 1.8 s,
  closes at 2.6 s, and its transcript never comes. At 5.6 s it gets exactly one `Failed` and the cut is committed
  (`into: 600 ms`). Robin's words from 7.0 s are turn 1 at 8.2 s, and its reply plays at 8.2 s, so the call goes on.
- `the_providers_own_bound_no_longer_sets_the_holds_wait`: the provider fails at 22.6 s. The cut still comes at 5.6 s,
  with exactly one `Failed` (at 5.6 s), and there is no turn.
- **Plant: `overdue` removed.** Both fail.
- **Plant: `heard`'s `Waiting` guard removed.** The suite passes. The dropped pending leaves the line on the same
  `advance` (`start_turn` pops it when it is at the front), and every order I could build puts it at the front. The
  guard is defensive, for a dropped utterance that sits behind a non-Speech one still waiting. I found no reachable
  test for it.

**Live check (maintainer).** Not separately reachable without a stalled provider. The floor live check below
exercises the same `advance` path.

**Left / uncertain.**
- A late transcript that is dropped was billed by the provider, and its usage is not booked: the drop emits no
  `Utterance`, which is what books `speech.stt`. This only happens past the 3 s bound, which is rare.
- The bound covers only a hold. A non-Speech utterance whose transcript never comes still blocks `start_turn` and a
  reply waiting on it (`contends`) until the provider's own bound. The brief scoped this to the hold.

## 2. aq4t: the floor's bound (62159cf0)

**Found.** `play_next` waits while any VAD is open, and `resume` does too. A steady sound closes at the 30 s max and
reopens on the next frame, so the floor is never free.

**Changed.** A new field, `Config::floor_bound`, defaults to 8 s.

- **When it fires.** `Engine::probe` runs in `advance`. It fires when a reply's front item `opens`, nothing plays, and
  it has waited `floor_bound` since it was `asked`. It also fires for a hold that has waited `floor_bound` since it
  began (`Hold::at`). In both cases, only when no transcript that would decide first is still due (a contending
  `Waiting` pending for the floor, a Speech one for the hold).
- **What it does.** It transcribes each open, unprobed utterance's audio so far, once (`Vad::so_far`, up to its last
  speech frame), as `Done::Probed`.
  - Heard as no words: the utterance's `Opening.sound` becomes `Wordless`. `floor_held()` (used by `play_next`,
    `resume` and the pause test) ignores it, so the reply plays.
  - The stop exemption: that speaker's 300 ms barge-in is not counted while the mark stands, as for an echo-prone
    speaker. So the reply plays to its end instead of the sound's next 300 ms moving the wait into a hold.
  - Heard as words, or a failed probe: it holds the floor as before, so a person's long sentence is never talked
    over. A failed probe also emits `Failed { Transcribe }`.
- **The same sound across the VAD's maximum.** A `Wordless` utterance that the VAD's maximum closes (it closed with
  speech that tick) and that opens again on the next tick keeps the mark (`Engine::steady`). Otherwise the 30 s
  reopen would stop a playing reply.
- **Its words still decide.** When a marked utterance closes, its transcript decides as any does. If it holds words
  over speech, it cuts late, as an echo-prone speaker's do.

**What it costs.**
- One STT request per open utterance per wait that reaches the bound, of up to about 8 s of audio. For a 40 s fan
  that is one request for the first wait, since the mark carries across the 30 s reopen.
- One copy of that audio, once.
- Per tick, a comparison, and once past the bound a walk of the open VADs with a map lookup.

**FAST, by code path.**
- The bound checks are in `advance`, after the tick's VAD work, not on the audio path.
- The stop still comes on the tick of the 300th ms: `tick()` → `hold()` is unchanged, and the exemption only removes
  the counting for a marked speaker.
- A resume replays the held audio with no new synthesis: `resume` touches no audio, and `Item.audio` is kept through
  the hold. The existing `a_sound_with_no_words_pauses_the_reply_...` still asserts 3 syntheses, and the new
  hold-under-a-sound test replays S1 from its held audio.

**Proved.**
- `a_reply_waits_8_s_for_a_floor_held_by_a_steady_sound_then_plays_whole`: a 40 s fan from 0.6 s.
  - The answer comes at 1.5 s and is `asked` on the tick from 1.48 s. It plays at **9.48 s**, whole (not stopped).
  - No cut, no resume, one turn.
  - 4 transcriptions: the question, one probe, and the fan's two utterances (both `Wordless`, at 30.6 s and 41.3 s).
  - Before the change it would have played after 41.3 s.
- `a_hold_under_a_steady_sound_resumes_at_the_floors_bound`: a fan from 1.5 s stops the reply at 1.8 s. It gets
  `Resumed { Wordless, held: 8 s }` at 9.8 s, and S1, S2 and S3 all play, unstopped.
- `a_speaker_talking_10_s_in_one_breath_is_still_waited_for`: Robin talks from 1.0 s to 11.0 s. The probe at 9.48 s
  hears words, so the reply waits. His utterance closes at 11.7 s and supersedes it (`Cut { Superseded }`). His words
  are turn 1, and nothing plays.
- **Plant: the floor's bound removed** (`self.probe()` out). All three fail. The third fails only because its probe
  no longer takes Robin's first fixture: the stand-in hands fixtures out in order, which AGENTS.md now says.
- **Plant: the stop's exemption removed.** The first two fail (the reply is stopped 300 ms after it starts).
- **Plant: every probe heard as no words.** The 10 s speaker test fails: he is talked over at 9.48 s.

**Live check (maintainer).**
1. A fan or music at a listed speaker's microphone, then a question from another listed speaker. The answer plays
   about 8 s after it is ready, whole, not when the sound stops.
2. A long question in one breath (over 8 s): not talked over. The answer comes after it ends, to the whole question.

**Left / uncertain, for the owner.**
- **The probe's spend is not booked.** It emits no event on success, because a new `Event` variant would break the
  binding's exhaustive match, which this task leaves alone. Booking it needs either a new event, or an `Utterance`
  for the probe, which would double-count `heard_ms` and the notes. I recommend a new `Event::Probed { speaker,
  usage, latency, heard_as }` read by the binding's ledger, in a step that may touch runtime/voice.rs.
- **What health's voice line needs to count the bounds:**
  - the hold's bound: a count of `Failed { Transcribe }` whose error is the bound's (today it lands in `failures` with
    `last_error` "no transcript 3000 ms after the utterance closed");
  - the floor's bound: a count of probes and of probes heard as no words. Both need the event above.
- A TV with speech under an open microphone is heard as words, so a reply still waits for its 30 s close, and its
  words then become a turn, as before. That is unchanged.
- A report waiting for a pause doesn't trigger a probe: only a queued front item or a hold does. A pause under a
  sound already probed counts as quiet (`floor_held`), so the acknowledgment and reports do play then.

## 3. Their tests

Bundled with steps 1 and 2 above, each in its commit, in turns.rs's style: virtual time, a `Stalled` transcriber
beside `Deaf` (a speaker's first N transcriptions never come, or fail after a delay). No existing turns.rs test
changed.

## 4. e6mj: two rules pinned (229d46d5)

- **(a) `another_speakers_words_supersede_a_waiting_reply`.** The owner's turn is at 1.2 s, and the reply comes at
  3.0 s. Robin talks from 2.5 s to 3.5 s. At 4.2 s the reply gets `Cut { Superseded }`, nothing plays, and Robin's
  words are turn 1.
  - Plant: `contends` honouring only the reply's turn's speakers. It fails, and so do
    `a_yeah_after_a_closing_question_with_another_reply_queued_is_a_turn` and the 10 s speaker test, which lean on the
    same rule.
- **(b) `leaving_while_held_cuts_once_and_resumes_nothing`.** A laugh stops the reply at 1.8 s, and `Leave` comes at
  2.1 s. There is one `Cut { CallEnded, into: 600 ms }` at 2.1 s, no `Resumed`, and one clip played (stopped).
  - Plant: `call_ended` skipping a held queue. It fails.
- The driver gains `call_leaving`. `call_with` is now `run(.., None)`; its behaviour is unchanged.

**Live check (maintainer).** A laugh over a reply, then `/leave` before it resumes: one `voice.cut` row (`call_ended`)
on the session, and no `voice.resumed`.

## 5. qrwx: a waiting report cut at the call's end (6af519b7)

**Changed.** `call_ended` first cuts the queue as before. Then, for each report in `reports`, it emits:

```
Cut { what: Report, why: CallEnded, sentences: count, heard: first, into: 0,
      last_heard: said[Report] when first > 0, cut: its first sentence }
```

`said` keeps a report's last whole sentence when the report is sent back (`cut_items`), so `last_heard` is that
sentence. The binding's notes return early on `CallEnded`; its ledger row and metric take it as they take any cut.

**Proved.**
- `a_report_cut_by_words_and_waiting_at_the_calls_end_is_cut`: the owner's words cut the report in its second
  sentence at 2.5 s, his turn is still in flight, and `Leave` comes at 4 s. The report gets
  `Cut { CallEnded, sentences 3, heard 1, last_heard r1, cut r2 }` at 4 s.
- `a_report_never_begun_is_cut_at_the_calls_end`: `heard 0`, `last_heard None`, its first sentence as `cut`.
- **Plant: the new loop removed.** Both fail.

**Live check (maintainer).** A report cut by words, then `/leave` before the next pause: a `voice.cut` row with
`what: report`, `why: call_ended`, `heard: 1`.

## 6. j2ut: the joined run and the head-run rule (a1a0ed9e)

**Changed.**
- **The joined run.** `is_echo` measures the in-order run over the candidate sentences joined in the order they
  played, which holds each one's run too. heard.rs's `!is_echo("it is want the", ..)` flips to an echo.
- **The head-run rule.** A run that begins at the head of the sentence playing when the utterance began, from an
  utterance begun within 0.5 s of that sentence's start (`ECHO_HEAD`), is an echo from 2 words. It still needs 80% of
  the utterance. Elsewhere the floor stays 3.
- `is_echo` and `classify` take `head: Option<&str>`. The engine keeps it on `Opening.head`.

**Proved.**
- `an_echo_prone_speakers_echo_of_a_whole_reply_is_an_echo` (E1): after two echoes, Robin's 7.7 s echo of S1 and S2
  is an echo. No cut, all sentences play.
- `the_first_echo_cut_short_by_its_own_stop_is_an_echo` (E2): "This first" from 1.3 s, 0.1 s into S1, is an echo,
  resumed at 2.4 s.
- `an_echo_across_a_sentence_boundary_is_an_echo` (E3): "to talk over the second sentence", from 0.1 s into S2, is an
  echo.
- `two_words_not_at_the_playing_sentences_head_are_a_turn`: "Monthly view." begun 0.3 s into the question is a turn.
- A unit test of the head rule in heard.rs.
- **Plants:**
  - the joined run removed (each sentence alone): E1, E3 and heard.rs's flipped assertion fail;
  - the position rule removed: E2 and the unit test fail;
  - the position rule over any run, not the head's: the two-word test and the unit test fail.
- No existing turns.rs test moved.

**Differs from the brief.** "The monthly view." said over "Do you want the daily or the monthly view?" is **not** a
turn, before this change or after it. "the monthly view" is a 3-word in-order run of the question, and 100% of the
utterance, so the 3-word rule calls it an echo. I checked this in the engine: that test failed with `Echo`. It is not
in the suite.

**Decision for the owner.** An either-or answer that repeats the last option verbatim is dropped as an echo. The
brief forbids raising the floor to 4. Possible fixes:
- an echo must also come within the playing sentence's span of the utterance's own start;
- a run that ends at a question's end, said after its last word began, is an answer.

## 7. q4pc: "yes" with another reply queued (452faec2)

**Changed.** `what_is_over`: `last = item.index + 1 == item.count`. The close's flip to the tail now applies when the
front, if there is one, hasn't begun (`front.opens`).

**Proved.** In both tests the "Yes." from 3.4 s (300 ms, on the question's last word, which ends at 3.6 s) is turn 2
at 4.4 s, and Robin's reply gets `Cut { Superseded }` at 4.4 s.
- T1 (`..._with_a_reply_queued_before_it_is_a_turn`): Robin's reply was queued at 2.4 s.
- T2 (`..._with_a_reply_queued_under_it_is_a_turn`): Robin's reply was queued at 3.5 s.
- **Plants:** `last` by the queue's length fails T1; the flip only on an empty queue fails T1 and T2.

## 8. The per-speaker echo count (222a389b)

`one_echo_each_from_two_speakers_leaves_both_stops_on`: Robin echoes at 1.5 s and the owner at 3.2 s, and each is
resumed. Robin's words at 5.0 s still stop the reply at 5.3 s.
- Plant: one count shared by all speakers. It fails.

## 9. zcxx: speakable() and the echo's regressions (ec411738, c7099112)

**`speakable()` fixes, one test each:**
- a prose line opening with `|`: a row needs a separator next, a table's shape, or both outer pipes;
- prose with a pipe after a table: rows go on only while they keep the separator's shape (outer pipe, or for a table
  without outer pipes, the separator's pipe count);
- a `---`/`***`/`___` rule: dropped;
- `- [ ]` / `- [x]` / `- [X]` checkboxes: dropped with the bullet;
- a year that opens a line: list numbers have at most 3 digits, so "2024." stays;
- `>` directly before a digit: said as "more than" (">500 ms" becomes "more than 500 ms"); "> 5" with a space is still
  a quote mark;
- the `+ ` and `• ` bullets: tested.

Each of the 7 tests fails on a planted revert of its own fix, and on no other: 7 plants, 7 single failures.

**Decision for the owner.** I chose "more than" for a `>` before a number. Keeping the `>` leaves it to the TTS, which
may read it or skip it.

**Echo regressions in turns.rs.** Three tests:
- a table's one sentence heard back is an echo, over sentence 1;
- a link-and-emphasis sentence heard back as played is an echo;
- a cut has `last_heard` = the table sentence and `cut` = "Monday was the most." (raw `**Monday**`).

Plant: `Engine::reply` splitting the raw text (`sentences`) instead of `speakable`. All three fail.

---

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran before every commit; the last ran on c7099112's tree.

- **Every run, the suite phase failed on exactly the 33 known L1 tests**: theseus-sandbox's contract tests, its bench's
  `spawn_100`, and theseusd's sandbox tests (theseus-pv6i: a root daemon's job with no job cgroup). Nothing else
  failed, with one exception below.
  - Last run: 3,039 tests, 3,006 passed, 33 failed, 24 skipped.
- The phases before the suite all passed: fmt, shape, features, clippy, cockpit, test build, reader rule.
- I then ran the phases after the suite (`machine_checks`) by hand, and they passed:
  - protocol types unchanged;
  - turn bench `--runs 5 --burst 0`: frames_plain 5/5 ok, frames_tool 9/9 ok;
  - `cargo deny --offline check`: advisories, bans, licenses, sources ok. `cargo deny fetch` succeeded at setup.
- **The exception.** In the qrwx commit's gate, `theseus-core learning::tender::tests::
  a_pool_thread_started_from_the_idle_thread_keeps_its_policy` failed once ("the pool thread took the idle thread's
  policy", 0 vs 5). It is on the known list (theseus-1g8j‡), passed when rerun alone, and this branch doesn't touch
  theseus-core.
- **One commit wasn't gated alone.** zcxx's sentences.rs (ec411738) was in the tree of the first gate, together with
  c4f3147e: one gate covered both commits.

## Other runs

- **theseus-voice under the load recipe, 3 runs** (four busy loops at nice 0, the suite at `nice -n 19`,
  `TZ=America/Phoenix`): 102/103, 102/103, 103/103.
  - The failure both times was `theseus-voice::deepgram a_call_with_no_answer_ends_at_its_timeout`. Six more runs
    under load caught it once more:
    `assertion left == right failed: both reached the stand-in, left: 1, right: 2` (tests/deepgram.rs:394).
  - **Not this branch's.** That test drives `DeepgramSpeech` against its fake server with a 300 ms client timeout,
    which covers connect and send. Under CPU starvation, the second request can time out before the fake has read it,
    so the fake sees one request. It touches no engine code, and this branch doesn't change deepgram.rs or its test.
    It is not on the known list, so I'm naming it: a timing flake whose fix is to assert the count only for requests
    that were sent, or to give the fake's count a moment. It passes unloaded.
  - None of the new turns.rs tests failed under load. They run on tokio's paused clock.
- `TZ=America/Phoenix cargo nextest run -p theseus-discord -p theseus-voice`: 240/240 passed.
- The workspace suite: in every gate, as above.

## Docs the maintainer should write

- **Part III / status.md:**
  - the two bounds (`transcript_bound` 3 s, `floor_bound` 8 s) as `Config` fields, not config-file keys: the binding
    uses `Config::new`'s defaults;
  - the probe's cost;
  - qrwx's report cut;
  - j2ut's two rules;
  - q4pc;
  - zcxx's shapes.
- **The M7 design** (`docs/design/m7-surface.md`), if it describes barge-in's hold or the floor: both waits are now
  bounded.
- theseus-voice's AGENTS.md is updated in these commits: the bounds, the probe's fixture trap, the echo's run and
  head rule, and q4pc's tail.
