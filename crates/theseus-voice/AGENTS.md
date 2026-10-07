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
