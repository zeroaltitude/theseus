# theseus-voice

The voice engine (44a), a workspace member again since rows 77 and 78 (theseus-drrs). Its songbird line brings DAVE,
symphonia, and libopus (built with cmake), so a cold target's first build of it takes a few minutes. `deny.toml` allows
MPL-2.0 for exactly seven of its crates, by name, and ignores six advisories for it, each with its reason.

A barge-in holds, then decides (theseus-9ln5): the stop at 300 ms holds the queue, and the overlapping utterance's
transcript decides by the pure rules in `src/heard.rs` (wordless, echo, backchannel, resume, words), unit-tested there.
An echo is a near-whole, in-order copy of what played (a run of at least 3 words and 80% of the utterance, over the
candidate sentences joined in the order they played; from 2 words when the run begins at the head of the sentence
playing and the utterance began within 0.5 s of its start, theseus-j2ut), never an answer that reuses its question's
words, and a speaker is echo-prone only after two echoes (theseus-3ug0). A reply queued but not begun, and a question
that ended under an utterance begun on its last word (with nothing begun queued behind it when the utterance closes,
theseus-q4pc), are the tail, where a "yes" is a turn (theseus-1cz8). The backchannels are a short list ("mhmm" in its
spellings, "gotcha", "oh okay"): add only what never answers alone over a reply. A reply begins only on the floor
(theseus-kpa7). A hold's transcripts still due 3 s after the last utterance over it closed decide as a failure does, and
commit the cut; a transcript that comes later is dropped (`Config::transcript_bound`, theseus-aq4t). A reply that has
waited 8 s for the floor, or a hold, held only by open utterances, has each one's audio so far transcribed once
(`Config::floor_bound`): a sound heard as no words no longer holds the floor, and its stop waits for its words; a
person's long sentence is words, and still waited for. A test that holds a floor past 8 s gives the stand-in a
transcript for that probe too, since it takes the speaker's next fixture. `tests/turns.rs` holds all of it through the
seam, in virtual time and at exact times; a test there that needs words or none gives the stand-in a transcript, since
its default (`[utterance 0.6 s]`) is words. The engine speaks `speakable()`'s sentences (`src/sentences.rs`: Markdown
stripped, a link read as its label, a table or a code block one sentence pointing at the text channel), and an echo's
candidates and a cut's quotes are those same strings (theseus-rkvl).

A sound at the join is no turn (theseus-yf9ct): before the first turn, an utterance begun within `JOIN_GUARD` (3 s, an echo canceller's
warm-up) of the join, or while a greeting is queued or playing, is heard as `Join` (no turn, said once in the journal) when it
is under 1.5 s or its transcript is under 3 words; a long, clear one is a turn. Its stop waits for its
words, so the chime over the greeting never stops it. One or two words that are a fuzzy head
or tail of a sentence in the echo tail (`is_fragment_echo`: "seus here" for "Theseus here") are an echo. Both are
`Config::join_guard`, off in the engine so its own tests read as before, on from `[voice] join_guard` (default true).
`songbird_io` sends `[voice] lead_in_ms` (600 ms, in whole frames rounded up, given to `SongbirdIo::join` as `LeadIn`; 0 is off) of silence before the first clip after quiet (`Spell`), so
the head of the greeting isn't eaten. Inside a spell the stream stays warm (theseus-9d9aq, `src/warm.rs`): each clip plays beside a
paused silent track that its end starts (`WARM`, 700 ms, past `SPELL_GAP`), whose own end the engine never hears, and a stop ends it; a
later sentence that starts under it gets no lead-in, and one that starts with nothing warm gets `[voice] gap_lead_in_ms` (200 ms).
`VoiceIo::lead_in` says whichever silence led the clip. A quiet listener closes an utterance after
`[voice] pause_ms` (1200 ms; `Config::end_of_utterance`, which the engine's own default leaves at 700, set only by the binding's
`engine_config`), since a natural pause inside a sentence runs 0.7 to 1.2 s (theseus-bp0lp).
An utterance whose transcript has no `.`, `?` or `!` at its end waits `Config::merge` (`[voice] merge_ms`, 1500 ms; zero, the engine's
default, is off) after it closed for the same speaker's next one, and the two are one `Utterance` in one turn (`start_turn`:
`awaits_its_rest`, `goes_on`); a finished transcript, or one said over Theseus's speech, never waits. The judge's path is untouched:
each utterance is judged alone, before the merge. The hold gates the whole line (turns stay in the order the utterances closed), so
another speaker's transcribed utterance behind it waits too: at most `merge_ms` past the close, or the end of an utterance the
holding speaker began inside that window. An open VAD that began after the window does not hold (it is a new utterance); a
joined utterance keeps the later one's `over`, so a barge-in is never folded away (theseus-we57k).

Theseus cancels its own voice from what a listener sends (theseus-yaxcp, `Config::echo_cancel`, `[voice] echo_cancel`, default
true). `src/echo.rs` is the engine's side: the clips it plays, tick by tick as the seam was told to send them (a clip begun
inside the tick its predecessor ended in shares it, and `VoiceIo::lead_in` says the silence the seam put before one), and one
`Aec` per listener. `src/aec.rs` is pure DSP at 8 kHz: the delay (40 to 500 ms) found by correlating 10 ms energy envelopes
over the last second, an NLMS filter of 48 ms around it on pre-emphasised audio, frozen while the listener is much louder
than the echo expected. It does nothing while Theseus has been silent for a second (`None`; nothing is computed, and the
delay and taps wait for the next reply). Only the barge-in's count of speech frames reads the residual: the VAD, the
hold and the transcript rules read the raw frame as before. A residual is `warming` (not counted) until the delay is found
(800 ms, then the raw frame counts, as on headphones) and for 500 ms after it is found, while the filter converges. Off, the
engine is byte for byte what it was (`tests/aec.rs`, a golden). `Audio::babble` and `WavIo::echo` are the stand-ins for a
voice and a room; a tone has no envelope for a delay to be read from. The journal says the delay found and, when a spell
ends, its mean ERLE (`voice: an echo spell ended`).

A short or unclear transcript is asked of a judge (theseus-s3dwj, `src/unclear.rs`): when the rules would make a turn of a
transcript of at most 3 words, or one the recogniser gave under 0.80 confidence (`Transcript::confidence`, Deepgram's), that began
over Theseus's speech, within the echo tail after it, or at the join, the engine asks its `Judge` (`Engine::with_judge`; none: nothing
is asked and the rules decide alone, today's events and times) and holds that one utterance's handling for at most
`Config::judge_ms` (250 ms), on tokio's timer in its own work future, so the floor and the audio never wait. A `Verdict` that
`acts` (the pack's act band, 0.80) decides: `Echo` is heard as an echo (and counts toward echo-proneness), `Other` as `HeardAs::Other`,
`Addressed` is the rules' turn; any other answer, no answer, or one past the deadline leaves the rules' verdict. The utterance carries
what the judge said (`Utterance::judged`: the judgment's id, answer, probability, whether it acted, how long it took) and the
journal line names the judgment id. The case holds the transcript, Theseus's last 3 sentences (the strings `heard.rs` matches),
when it began against them (`Began`, `Timing`), its length, confidence and echo-proneness. The session supplies the judge
(theseus-discord's `runtime/voice/judged.rs`, over the core's `heard.v1`); `StandInJudge` is the test one. `Engine::has_judge()` says whether one is attached (theseus-sa4m5): the binding's `[voice] judge_unclear` decides it, and
`runtime/voice/tests_judged.rs` reads it. `tests/judged.rs` holds
it all through the seam, in virtual time.

Deepgram Flux can be the call's listener instead of the pause rules (theseus-v774k, `[voice] stt_model = "flux-general-en"`; the
default stays nova-3). `src/listen.rs` is the seam (`Listen`: raw frames in, `Listened` events out; `StandInListen` scripts
events at ticks), `src/flux.rs` the client (`FluxListen`: one v2 socket per listed speaker, opened at their first audio,
`CloseStream` at the call's end; the key only in the handshake's `Authorization` header and scrubbed by
`deepgram::scrub_key`, the one scrub for REST and the socket; 80 ms chunks; 80 ms of silence every 5 s to a quiet speaker as the
keep-alive; an outbound queue of 8 s that drops the oldest audio, said once a spell; a reconnect backoff of 1 s doubling to
30 s), and `src/engine/flux_path.rs` the engine's side (`Engine::with_listen`). With a listener, a speaker's utterance opens
at `Start` and closes at `End` with Flux's words (the mean of the words' confidences, never the end-of-turn probability):
no VAD, no `pause_ms`, no `merge_ms` (a Flux `Pending` is never merged), no second call; from the `Pending` on the engine is
the same. A `Start` over Theseus's speech holds the clip at once if the residual (raw with `echo_cancel` off) shows 3 frames
of speech since 200 ms before the turn and the speaker is not echo-prone, wordless or at the join; the `End`'s words then
decide as ever. `Event::StopLatency` (start received to the stop sent, and to the clip's end) comes just before the
`BargeIn` it belongs to, and `first word after the end of turn` is journaled on both paths. Eager end of turn (set by
`eager_eot_threshold`): an `Eager` makes the turn, a `Resumed` before its reply plays takes the floor back and the reply is
superseded by the `End`'s whole sentence, an unresumed `End` starts nothing. A stream that drops (`Down`) hands its turn in
progress (a ring of at most 30 s) to the pre-recorded recogniser once and the speaker to the VAD until `Up`; a listener that
cannot be reached (`Unavailable`) is off for the call. Both are `Failure::Flux`. Flux has no `smart_format`: dates, times
and numbers come unformatted. Tests: `tests/flux.rs` (the engine in virtual time), `tests/flux_socket.rs` (the client against a
local socket server); plant the residual gate's removal and `a_start_of_turn_on_the_echo_alone_does_not_stop_the_clip` goes red.
