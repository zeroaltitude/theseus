# Cloud report: voice-heard (theseus-qb8o, theseus-rkvl, theseus-9zft)

Branch `cloud/20261006-voice-heard`, from main at f33f0ec (voice-turns' join is there: the engine's `Cut`,
`Resumed`, `Utterance.over` and `Utterance.heard_as`). Started 16:53 UTC, report 19:25 UTC. Four commits, one per
step, each gated and pushed:

| Step | Commit | Issue |
|---|---|---|
| 1. The heard line | 3a3d756 | theseus-qb8o |
| 2. The rows, the metrics, health's resumes | 82e3750 | theseus-qb8o |
| 3. Replies shaped for speech (framing + `speakable`) | 511d20b | theseus-rkvl |
| 4. A failed voice turn said aloud | a46a94d | theseus-9zft |

## Step 1: the heard line (3a3d756)

**Found.** `pump` dropped `Cut` and `Resumed` (the comment said "voice-heard writes their rows"), and
`submit_voice` sent only the `🎙️` lines, so nothing told the model what was unheard.

**Changed.**
- New `crates/theseus-discord/src/runtime/voice/notes.rs` (a child of `runtime/voice.rs`, which only gains the
  `mod` lines, a `notes` field on `Call`, three calls in `pump`, and the line at submit). `Notes` keeps each voice
  turn's first words by `TurnId` (clipped at 40 characters; the newest 32 turns kept) and the notes that wait.
- `pump`: `Event::Turn` records the first words, `Event::Utterance` heard as `Backchannel` or `Resume` adds
  `while you spoke they said "<words>"`, and `Event::Cut` adds one of:
  - by words, some heard: `they cut in on your reply to "<first>": they heard "<last heard>" (H of N sentences);
    you were saying "<cut>" when they spoke, and the rest was not said`;
  - by words, none heard, none started: `your reply to "<first>" (N sentences) was never said aloud`;
  - superseded: `your reply to "<first>" was not said: they kept talking before it began`;
  - at the call's end: nothing.
- `submit_voice` drains the notes into one line before the input, `[Voice: ` notes joined by `; ` `]`, adding
  `they said this before your reply to "<first>" had been spoken` for each utterance whose `over` is
  `Preparing`. Quotes clipped at 80 characters on a word with `…`; at most the 3 newest notes; under 400
  characters (the newest note always kept); no notes, no line.
- Two cases the brief didn't spell out, decided here: a cut by words *between* sentences (`into` zero, `heard` > 0)
  says `"<cut>" was next, and the rest was not said`, since nothing was being said; and a report (not a reply) is
  named `your report`, and as the engine brings a cut report back at the next pause its note says `the rest comes
  back at the next pause`.

**Proved.** `runtime/voice/tests_heard.rs`, which drives `pump` with engine events (with the place's channel
routed, so `Event::Turn` arrives as `PlaceMsg::Voice`) and the place with the voice turn, then reads the session's
history:
- `a_cut_reply_is_named_on_the_next_voice_turn_and_not_after` (the exact line, then no line on the turn after);
- `a_reply_never_said_and_a_superseded_reply_are_named` (and a `CallEnded` cut adds nothing);
- `a_backchannel_and_words_before_a_reply_are_named` (a wordless utterance adds nothing);
- `the_line_is_bounded` (40-character first words, quotes clipped with `…`, ≤ 3 notes, < 400 characters);
- `notes::tests::a_quote_clips_on_a_word_with_an_ellipsis`.

Planted reverts: the notes never written (`self.waiting.push(note)` removed from `Notes::cut`):
`a_cut_reply_is_named…`, `a_reply_never_said…` and `the_line_is_bounded` fail (`left: None`). The notes not drained
(`mem::take` → `clone`): `a_cut_reply_is_named…` fails at its second half (`drained: [Voice: they cut in…]`), and
`a_reply_never_said…` fails too. Restored, touched, `git status` clean apart from the step.

## Step 2: the rows (82e3750)

**Changed.**
- `LedgerKind::VoiceCut` (`voice.cut`: what, why, sentences, heard, into_ms) and `LedgerKind::VoiceResumed`
  (`voice.resumed`: what, why, held_ms), written in `pump` on the place's session as `voice.barge_in` is (a helper,
  `held`, keeps `pump` under clippy's line limit). `why` for a cut is `words | superseded | call_ended`; for a
  resume `wordless | echo | backchannel | resume`.
- `speech.transcribed`'s detail gains `heard_as` (the same words) and `over`: `{"saying": "reply 3", "sentence":
  2}`, `{"preparing": "reply 4"}`, or null.
- `VoiceStatus.resumes` (serde default 0), and its line says `· N resumed` after the barge-ins; its test updated;
  `cockpit/src/protocol.gen/VoiceStatus.ts` regenerated.
- Metrics, following `theseus.cancel`'s pattern (an `Instrument`, a `Metrics` method, a `Telemetry::record_*`
  called where the fact is counted): `theseus.voice.cuts` and `theseus.voice.resumed`, IntSum, attribute
  `theseus.voice.why`. `INSTRUMENTS` 40 → 42.

**Store's version rule:** it doesn't apply. No stored record gains a field: a ledger row is stored as its kind's
name and a JSON `data`, which a new kind or detail key doesn't change in layout (the reader reads any kind by its
string), and `VoiceStatus` is health's live status, never stored. `MANIFEST_FORMAT` stays 23.

**Proved.** `tests_heard::the_cut_and_resumed_rows_carry_their_session_and_fields` (both rows' exact JSON and their
session; both transcriptions' `heard_as` and `over`; `status.resumes == 1` and the line's `· 1 resumed ·`);
`telemetry::tests_voice::a_cut_and_a_resumed_stop_are_counted_by_why` (through a pipeline to a receiver);
`tests_registry::every_ledger_kind_is_written` passes with the two kinds; the protocol's TypeScript test passes.

## Step 3: replies shaped for speech (511d20b)

**Changed.**
- `FRAMING` in `runtime/voice.rs`, the brief's sentence exactly, as the first line of every voice turn's input,
  then the heard line, then the `🎙️` lines.
- `speakable(text) -> Vec<String>` in `theseus-voice/src/sentences.rs` (exported with `TABLE` and `CODE`), and
  the engine's `reply` and `Command::Report` now call it instead of `sentences` (two lines in `engine.rs`, plus its
  module doc; `what_is_over` and `heard.rs` untouched). It strips `**`/`__` anywhere, `*`/`_` with a word on one side
  only (so `snake_case`, `my_file` and `2 * 3` stay), backticks, `#` heading marks, `>` quote marks, `- * + •`
  bullets and `1.`/`1)` list numbers at a line's start; reads `[label](url)` as the label; drops a bare `http(s)://`,
  `<http…>` or `www.` address (punctuation after it is kept); turns a run of table rows (a line starting with `|`, a
  separator row, or a pipe line next to a separator) into "There's a table in the text channel."; turns a fenced
  block (```` ``` ```` or `~~~`) into "There's code in the text channel."; then splits as `sentences` does.
- The text lane is untouched; a `Cut` quotes the engine's items, which are now the speakable sentences.

**Proved.** Unit tests in `sentences.rs`: one per construct (emphasis and backticks with snake_case kept;
heading, quote, bullet and list-number marks; links and bare addresses; tables with and without outer pipes;
fenced code). Pipeline test `tests/pipeline.rs::a_reply_with_a_table_speaks_its_prose_and_one_table_sentence`: a
reply with prose, a 9-row table and `**Thursday**` makes 3 syntheses, a `Speaking { sentences: 3 }`, and plays the
three clips of "Here's last week's spend.", `TABLE`, and "Thursday was the most.".
`tests_heard::the_framing_line_leads_every_voice_turns_input` (framing alone; then framing, heard line, input);
`a_voice_turn_is_a_turn_of_the_places_session_authored_by_its_speaker` updated to expect the framing line.

Planted reverts: the engine back on `sentences` (`use crate::sentences::sentences as speakable`): only
`a_reply_with_a_table_speaks_its_prose_and_one_table_sentence` fails (67 of 68 pass). The framing line removed:
`the_framing_line_leads_every_voice_turns_input` and `a_voice_turn_is_a_turn…` fail. Restored, touched, clean.

## Step 4: a failed voice turn said aloud (a46a94d)

**Changed.** `FAILED_TURN` ("Sorry, that didn't work. The details are in the text channel.") is the reply the
engine gets when `submit_voice`'s `turn.submit` fails, instead of the empty string. The place's failure post is
unchanged.

**Proved.** `tests_heard::a_failed_voice_turn_sends_the_constant_sentence`: a model that answers
`ProviderError::Auth`, so the submit fails, and the engine's command is `Reply { turn: 0, text: FAILED_TURN }`. (An
empty script doesn't fail a turn, which the first draft of the test found.) Planted revert: the error arm back to
`String::new()`: that test fails, the other 14 voice tests pass. Restored, touched, clean.

## FAST

Nothing is on the start path: the notes are built in `pump` (string formatting per engine event, during a call) and
the line at `submit_voice` (`Notes::line`, a drain and a join). The heard line is under 400 characters (about 120
input tokens) on a turn after a cut, and the framing line is 216 characters (about 45 tokens) on every voice turn. No
extra model call. `speakable` is one pass over the reply's characters before synthesis (microseconds), and makes the
audio shorter: a table is one sentence instead of one per row. No bench needed.

## Runs under load

AGENTS.md's recipe: each suite at `nice -n 19` beside four busy loops at nice 0 (`sh /tmp/busy.sh`, killed by
their pids; none left after each run), with the test binaries built first.
- theseus-voice: 5 of 5 runs, 68 of 68 passed each.
- theseus-discord: 5 runs, 130 tests each: runs 1, 3 and 5 all passed; runs 2 and 4 failed one test,
  `tests_outbox::a_post_for_a_place_no_longer_bound_is_refused_at_the_next_start` (17.0 s and 18.0 s). I didn't
  keep its output (the first run's filter kept only the FAIL lines). To get it: 6 runs of that test alone under the
  same load all passed (5.7 to 10.3 s), and 5 more full-suite runs under load all passed (130 of 130). So it's 2
  failures in 10 full-suite runs under load, and none alone.
  - It is not on the brief's list. It is an outbox restart test in a file this branch doesn't touch, and nothing it
    runs reaches the voice code (no call is joined; `pump` never runs). Its 17 to 18 s matches its 10 s `until`
    waits ("the channel is bound", "both bind notices", "the post is refused") timing out behind a slow start under
    load, so I read it as a timing bound. But I have no message to prove that, and its name is a refusal
    ("nothing reaches the place"), so it should be looked at. I didn't retry it away; the two failing runs are the
    record. Next step for the maintainer: `nice -n 19 cargo nextest run -p theseus-discord` beside four busy loops,
    a few times, keeping the full output.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Each run:
fmt, shape, features, clippy, cockpit, test build and the reader rule ok; the suite failed only on the 33 known L1
tests (theseus-pv6i: the VM runs as root): theseus-sandbox's contract tests and `spawn_100`, and theseusd's
`sandbox` tests. Final counts: 2,944 run, 2,911 passed, 33 failed, 23 skipped. Then the phases after the suite, run by
hand: protocol types (`git diff --quiet -- cockpit/src/protocol.gen`, after staging) ok; lifecycle, jobs and turn
skipped by `THESEUS_GATE_NO_BENCH`; `cargo deny --offline check`: advisories, bans, licenses, sources ok (the
advisory database fetched at setup). No other failure in any gate run.

Two things about this VM. The second gate run filled the disk (`No space left on device` in clippy): I deleted
`target/debug/incremental` and ran every later cargo command with `CARGO_INCREMENTAL=0`. And on the first gate run
`pump` was over clippy's `too_many_lines`, which I fixed (the `held` helper) before step 2's commit.

No dependency was added (Cargo.lock and the package-lock files are unchanged). No file nears its ceiling: voice.rs
is about 1,590 lines (not listed; limit 2,500), metrics.rs about 1,070, and the new files are small.

## The live check (the maintainer's, after the join and an install)

In the test voice channel:
1. `/join`, ask a question with a long answer, and cut in on it with another question. The next answer shouldn't
   presume the unheard part. Then `theseus history <the voice place's session>` shows the input as three lines: the
   `[Voice call: …]` framing, `[Voice: they cut in on your reply to "…": they heard "…" (H of N sentences); you were
   saying "…" when they spoke, and the rest was not said]`, and `🎙️ <the question>`. Say "yeah" during a reply,
   then ask something: that input's line has `while you spoke they said "Yeah."`.
2. Ask for last week's daily spend: a short spoken answer. If the text has a table, it is spoken as "There's a table
   in the text channel." No "star star" or backticks are voiced; the text channel shows the reply as written.
3. After a few cuts and pauses: `theseus ledger -k voice.cut` shows rows with what, why, sentences, heard and
   into_ms on the place's session; `theseus ledger -k voice.resumed` shows what, why and held_ms;
   `theseus ledger -k speech.transcribed` rows carry `heard_as` and `over`; `theseus health`'s voice line reads
   `… · N barge-in(s) · M resumed · $…`.
4. (Step 4) A voice turn that fails, e.g. with the model's key revoked in a scratch config: the call says "Sorry,
   that didn't work. The details are in the text channel." and the text channel has the failure post.
   With an OTLP endpoint set, `theseus.voice.cuts` and `theseus.voice.resumed` appear with `theseus.voice.why`.

## Left, uncertain, and for the owner

- The `tests_outbox` failure under load above: not this branch's code, but unexplained without its message.
- The heard line rides in the user's input, so it is stored in the session's history and shown by `theseus
  history` and the cockpit as part of the voice turn's text. A context-only node kind would keep the transcript
  clean; that is the later question the brief named.
- The framing line goes on every voice turn, so the model sees it repeated through a long call (45 tokens each).
  Putting it once in the system block for a voice place would be cheaper, but that is the compiler's area, which I
  left alone.
- `speakable` doesn't shorten long numbers (a twelve-digit number is still read whole); the framing line asks the
  model not to write them. Reading numbers aloud is a later rule if the framing isn't enough.
- A table without any pipe line next to a separator (an unusual Markdown) would still be read row by row. The
  `*`/`_` rule keeps a lone `*` between spaces (`2 * 3`), which a voice will say as "star" or skip, depending on
  the synthesizer.
- voice-echo (theseus-3ug0, 1cz8) changes `engine.rs`'s `what_is_over`; this branch changes only the `use` line,
  the module doc and the two `sentences(` calls in `engine.rs`, so a textual conflict is unlikely.
- Docs for the maintainer: `crates/theseus-discord/AGENTS.md` could name `runtime/voice/notes.rs` (the heard line)
  and the framing line; `crates/theseus-voice/AGENTS.md` could name `speakable` as what the engine speaks;
  `docs/status.md` the three issues; the spec's Part III item for the step.
