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
`songbird_io` sends `LEAD_IN_FRAMES` (5, 100 ms; 0 is off) of silence before the first clip after quiet (`Spell`), so
the head of the greeting isn't eaten; the next sentence of a reply gets none.

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
(theseus-discord's `runtime/voice/judged.rs`, over the core's `heard.v1`); `StandInJudge` is the test one. `tests/judged.rs` holds
it all through the seam, in virtual time.
