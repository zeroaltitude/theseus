# The Ship of Theseus, chapter 28: Part III, A4's Items 216 to 224 ([index](README.md))
### Item 216. Voice heard: in a voice call the next turn is told what was heard, cut and never said, replies are shaped for speech (a framing line on every voice turn, and the engine speaks `speakable()`'s sentences, so no Markdown, table or code block is read aloud), and a failed voice turn says so aloud (theseus-qb8o, theseus-rkvl and theseus-9zft; the voice lane's designs 2 and 4 and one of its small designs; the tenth cloud batch's voice-heard session, launched by the DM thread at 09:49 on voice-turns' join and fired 2026-10-06 09:53 from f33f0eca, Opus 5.5, finished 12:21; 3a3d7564, 82e37509, 511d20b1 and a46a94df; reviewed 12:45 to 13:40 by local reviewer R33, and accepted at 14:03; joined 14:32 at 4db4cfc0, a signed merge onto ceba1520, by the batch-10 voice-heard joiner; installed 2026-10-06 17:22 at 938a8dc8, install #8)

**Why.** The owner's voice call of 2026-10-05 21:02 (Item 202's Why) showed what voice-turns left: the session recorded
every reply as if spoken whole, so the model built on text the owner never heard. It corrected a table that was never
heard, and took a "Yeah." said mid-table as consent to an offer it never voiced. Voice turns carried only the `🎙️`
prefix, so replies were written for a screen: 13 sentences on average and 41 at most in that call, tables read row by
row, a twelve-digit number read for 8.8 s, and asterisks voiced. And `submit_voice` spoke nothing when a turn failed,
which the place said only in text, so a caller not watching the text heard silence. The voice lane (theseus-vk84) wrote
these up as its gap 2 and design 2 (qb8o: the heard line), gap 3 and design 4 (rkvl: speech-shaped replies) and one of
its small designs (9zft). voice-turns' join (Item 202) gave the engine the facts the work needed: `Cut`, `Resumed`,
`Utterance.over` and `Utterance.heard_as`, which `pump` dropped ("voice-heard writes their rows", its comment said).

**What landed** (theseus-discord's voice runtime, theseus-voice's `sentences.rs` and two calls in `engine.rs`,
theseus-core's telemetry, theseus-protocol's ledger kinds and `VoiceStatus`, the cockpit's generated `VoiceStatus.ts`;
the merge 13 files, +1,207 −32, without the cloud files; one additive protocol field, no config key, no package, no
store format change (23 stays 23)).
- **The heard line** (3a3d7564, qb8o). `runtime/voice/notes.rs` (new, 206 lines, a child of `runtime/voice.rs`, which
  gains only the `mod` lines, a `notes` field on `Call`, three calls in `pump` and the line at submit). `Notes` keeps
  each voice turn's first words by `TurnId` (clipped at 40 characters; the newest 32 turns) and the notes that wait. In
  `pump`, `Event::Turn` records the first words; an utterance heard as `Backchannel` or `Resume` adds `while you spoke
  they said "<words>"`; and `Event::Cut` adds one of: by words with some heard, `they cut in on your reply to
  "<first>": they heard "<last heard>" (H of N sentences); you were saying "<cut>" when they spoke, and the rest was
  not said`; by words with none heard and none started, `your reply to "<first>" (N sentences) was never said aloud`;
  superseded, `your reply to "<first>" was not said: they kept talking before it began`; at the call's end, nothing.
  `submit_voice` drains the notes into one line before the input, `[Voice: ` the notes joined by `; ` `]`, adding
  `they said this before your reply to "<first>" had been spoken` for each utterance whose `over` is `Preparing`.
  Quotes are clipped at 80 characters on a word with `…`; the line holds at most the 3 newest notes and stays under
  400 characters (the newest note always kept); no notes, no line. The session decided two cases the brief did not
  spell out: a cut by words between sentences (none being said) says `"<cut>" was next, and the rest was not said`,
  and a report is named `your report`, its note saying `the rest comes back at the next pause`, since the engine brings
  a cut report back then.
- **The rows, the metrics, health's resumes** (82e37509, qb8o). `LedgerKind::VoiceCut` (`voice.cut`: what, why,
  sentences, heard, into_ms) and `LedgerKind::VoiceResumed` (`voice.resumed`: what, why, held_ms), written in `pump` on
  the place's session as `voice.barge_in` is; a cut's `why` is `words`, `superseded` or `call_ended`, a resume's
  `wordless`, `echo`, `backchannel` or `resume`. `speech.transcribed`'s detail gains `heard_as` and `over`
  (`{"saying": "reply 3", "sentence": 2}`, `{"preparing": "reply 4"}`, or null). `VoiceStatus.resumes` (serde default
  0) puts `· N resumed` after the barge-ins on health's voice line. Two metrics, in `theseus.cancel`'s pattern:
  `theseus.voice.cuts` and `theseus.voice.resumed`, IntSums with `theseus.voice.why`, counted where the rows are
  written (`INSTRUMENTS` 40 to 42). The store's version rule does not apply: a ledger row is stored as its kind's name
  and a JSON `data`, which a new kind or detail key does not change in layout, and `VoiceStatus` is health's live
  status, never stored.
- **Replies shaped for speech** (511d20b1, rkvl). `FRAMING`, the first line of every voice turn's input (then the
  heard line, then the `🎙️` lines): "[Voice call: they hear your reply, they don't read it. Answer in one to three
  short sentences of plain speech: no lists, tables, code, markdown or long numbers. If you were cut off, don't assume
  they heard the rest.]" (216 characters, about 45 tokens). `speakable(text)` in theseus-voice's `sentences.rs` (91 to
  325 lines, exported with `TABLE` and `CODE`), which the engine's `reply` and `Command::Report` now call instead of
  `sentences`: it strips `**` and `__` anywhere, `*` and `_` with a word on one side only (so `snake_case`, `my_file`
  and `2 * 3` stay), backticks, `#` heading marks, `>` quote marks, and bullets and list numbers at a line's start;
  reads `[label](url)` as its label; drops a bare web address (punctuation after it kept); turns a run of table rows
  into "There's a table in the text channel." and a fenced block into "There's code in the text channel."; then splits
  as `sentences` does. The text lane is untouched; a `Cut` quotes the engine's items, which are now the speakable
  sentences.
- **A failed voice turn said aloud** (a46a94df, 9zft). `FAILED_TURN`, "Sorry, that didn't work. The details are in the
  text channel.", is the reply the engine gets when `submit_voice`'s `turn.submit` fails, where it got the empty
  string; the place's failure post is unchanged.

**How it is proven.**
- **The session's tests:** `runtime/voice/tests_heard.rs` (482 lines) drives `pump` with engine events and the place
  with the voice turn, then reads the session's history: a cut reply named on the next voice turn and not after; a reply
  never said and a superseded reply named (a `CallEnded` cut adds nothing); a backchannel and words before a reply named
  (a wordless utterance adds nothing); the line's bounds; both rows' exact JSON and session, both transcriptions'
  `heard_as` and `over`, and `status.resumes == 1` with its `· 1 resumed ·`; the framing line leading every voice turn's
  input; and a model answering `ProviderError::Auth`, so the submit fails and the engine is sent `Reply { turn: 0, text:
  FAILED_TURN }`. `sentences.rs` has a unit test per construct, and `tests/pipeline.rs` plays a reply with prose, a
  9-row table and `**Thursday**` as 3 syntheses and 3 clips (the prose, `TABLE`, "Thursday was the most.").
  `telemetry::tests_voice` counts a cut and a resumed stop by `why` through a pipeline to a receiver;
  `tests_registry::every_ledger_kind_is_written` passes with the two kinds. The session's five planted reverts each
  failed their tests (a cut's note never written; the notes not drained; the framing line removed; the engine back on
  `sentences`; the failed turn's sentence back to the empty string). Under load, theseus-voice 5 of 5 runs (68 of 68);
  theseus-discord failed one test in 2 of 10 full-suite runs
  (`tests_outbox::a_post_for_a_place_no_longer_bound_is_refused_at_the_next_start`, 17.0 and 18.0 s), a file the branch
  does not touch, passing 6 runs alone; its output was not kept. The cloud gate failed only on the 33 known L1 tests
  (the VM runs as root).
- **The review** (R33, review commit 5640e539 on f589d9cb, so with voice-echo's engine change in it): fmt, the test
  build, clippy, theseus-protocol's 31 tests (protocol.gen exactly what ts-rs writes) and shape clean; the branch's
  crates and tests **320 of 320** (theseus-voice 76, theseus-discord 137, theseus-protocol 31, theseus-core's telemetry
  and registry tests 76); **the whole workspace suite, 3,012 of 3,012** (1 slow by design, 24 skipped, `--retries 0`).
  **24 planted reverts** (the report's 5 and 19 of R33's, two of them FAST claims: the hold dropping the held audio, and
  the stop one 20 ms frame late): the branch's tests catch 20, R33's probes 3 of the other 4, and the fourth is an
  untested bullet rule (theseus-zcxx). **21 probes** of R33's: 13 through the voice engine's seam and `speakable`, 8 in
  theseus-discord's runtime, 7 of them through the place's real `submit_voice`. R27's probes from voice-turns: the four
  `finding_*` tests of theseus-3ug0 and 1cz8 now fail (voice-echo fixed them), and all five `holds_*` pass.
  theseus-discord under the report's load recipe, **8 of 8 runs** (137 of 137 each), the outbox test 0.9 to 6.6 s;
  beside a busy loop per core, two more runs passed (the outbox test 7.4 and 11.4 s) and a third was killed at nextest's
  120 s on a courier test that takes 14 s alone (theseus-z9nq), not this branch's. **The rollback:** main's build from
  before the merge (f589d9cb, built from `git archive`) started a scratch daemon on a copy of a store the merged build
  had written `voice.cut`, `voice.resumed` and `speech.transcribed` rows into; its `theseus ledger` (unfiltered and `-k`
  each kind) and `theseus health` exited 0 and showed the rows whole, so no format bump is owed. The public repository
  scrub was clean.
- **The echo against what was played** (the DM thread's first check). One `Item.text` per sentence feeds synthesis,
  the echo's candidates, `Over::Saying` and `Cut`'s quotes, and the heard line quotes `Cut` verbatim, so none of them
  can see a reply's raw text. R33's probes on the merged tree: the table sentence carried back whole by a caller's
  microphone is an echo (held at 300 ms and replayed whole, with no cut, no barge-in and no turn), where against the raw
  rows those words match nothing; a sentence that had a link and emphasis in the reply is an echo too; and a cut's
  `last_heard` is the table sentence, its `cut` the last sentence with no `**`.
- **`FAILED_TURN`'s reach** (R33 walked every error `turn.submit` can return, before and during the turn). A model
  error says `FAILED_TURN`. A `/stop` during the model call and one while the voice turn waits for admission end ok
  with no output: silence, rightly. A barge-in never fails a turn. An approval and a budget question end ok, not
  failures, but nothing says an answer waits (theseus-b6vz). A stopping daemon drops the task. Only `StoppedAtStep`, a
  stop whose next step the stop refuses, would be said as a failure while the text says nothing (by the code; not
  provoked; b6vz).
- **The heard line's bounds:** one note is at most about 341 characters, so the newest, always kept, cannot reach 400
  alone; it is drained once; there is no race with the next turn (the engine starts no turn while one is in flight,
  emits the superseded `Cut` before the next `Turn`, and `pump` writes the note before it forwards that turn); a
  call's notes end with it, and an old call's late events reach no other call (`pump`'s writes and `submit_voice`'s
  read both check the call's serial).
- **FAST.** Nothing on the start path: `Notes::default()` is built at a `/join`, the notes per engine event in `pump`,
  the line per voice turn. The stop is still on the same tick and a resume still replays the held audio with no new
  synthesis: the engine change is two `sentences(` calls that became `speakable(`, and both FAST plants are caught.
  `speakable` takes 114 µs on a 4,266-byte reply with tables, code, links and lists (`rustc -O`; 736 µs in the debug
  test build), once per reply, before the first synthesis, between 20 ms ticks. The framing line costs about 45 tokens
  on every voice turn; the heard line, under 400 characters, rides only a turn after something went unheard.

**What the review found** (none blocks the join; all P3, assigned to main). **theseus-zcxx:** `speakable` mangles a
prose line that opens with `|` (read as a table), a `---` rule (spoken as a sentence), a task checkbox (its `[ ]`
kept), a year opening a line ("2024. That was the year." loses the year, taken for a list number), `>` before a number
("> 5 GB is free." loses its "more than"), prose with a pipe just after a table (swallowed into it), and "(see
<address>)" (leaves "(see )"); the `+ ` and `• ` bullets are untested. **theseus-nthu:** three runtime rules the
branch's tests do not pin, each caught only by R33's probes (an old call's events reaching the next call's notes,
`FAILED_TURN` spoken for a stopped turn, the 32-turn cap), and `Notes.waiting` has no cap of its own (only 3 are ever
read). **theseus-b6vz:** a voice turn held for the operator (a budget question, an approval) says nothing aloud, and a
stop a step refuses would be said as a failure; proposed, one constant "I need your answer in the text channel.", and
the failure line decided where `/stop` is known. **theseus-ved2** (pre-existing since 10-03): a rebind after a failed
turn returns before `voice_next`, so a voice turn queued behind it is never submitted and the call takes no more turns
until someone types. **theseus-z9nq:** courier's `tests_bound` lane test takes 14 s alone and passes nextest's 120 s
kill under a busy loop per core. R33's calls, adopted by the DM thread (14:03): keep the framing line per turn for the
live check, beside the words it governs, then move it out of the stored input (a 50-turn call stores 50 copies, about
2,300 tokens); keep the heard line stored as the session's only record of what was not heard (later as its own node
beside the input, shown apart in the cockpit, never context-only, which would forget it after one turn); rely on the
framing for long numbers; fix b6vz and ved2 before voice is used for anything that asks.

**The join** (batch 10, the branch alone). At 14:03 the queue was clear (only stale and review-only locks) and main =
origin/main = ceba1520, docs-v083's join. The joiner's dry run before the lock (14:05:33) was clean: `MANIFEST_FORMAT`
main 23, branch 23, merged 23; both sides had changed `telemetry.rs` and `engine.rs` since the base f33f0eca, and
merge-tree merged each; the four crates and the cockpit identical to R33's review commit; and against 5640e539 the
merged tree differed in 23 files, exactly main's own changes since f589d9cb (docs-v083's), R33's expectation for a
moved main. The guarded take (14:05:38 to 14:05:50) took `cloud-voice-heard-join`, found the queue clear and merged
`--no-ff`: `telemetry.rs` and `engine.rs` auto-merged, no conflict, no resolve.py, no join fix, the cloud files
removed. `git diff --cached --check` had one hit, `VoiceStatus.ts` line 28's trailing space, as ts-rs writes it (278 of
main's 354 generated protocol files end a line the same way); the joiner's take filters exactly that kind (trailing
spaces in protocol.gen's `.ts` files or a CLI golden) and stops on any other. Staged: 13 files, +1,207 −32, the tree
the dry run's less the cloud files. The warm (14:06:00 to 14:16:59: the test build 5 m 59 s at load 24 to 34 beside
three other trees, then clippy clean); R33's suites and `tests_output`, **323 of 323** in 35.6 s (theseus-voice 76,
theseus-discord 137, theseus-protocol 31 with protocol.gen regenerated byte for byte, theseus-core 79); `INSTRUMENTS`
42 entries. The signed merge **4db4cfc0** (ceba1520 and 68cc18fe), 14:20:19. Its gate (14:20:24, minute 20, to
14:32:31, ok, 726 s, of which 251 s waiting for the gate lock behind three review steps): **3,012 of 3,012** (1 slow
by design, 24 skipped) in 302.2 s, no hour crossed, no known load flake red; lifecycle in every budget (cold start p50
27.1 / p95 37.9 ms against 50 + 7; from the config copy 25.7 / 26.9; clean shutdown 33.1 / 48.8; a reply's post in
flight 76.6 / 86.2; SIGKILL then restart 29.8 / 31.1; binary swap 51.7 / 57.3; restore 251.1 / 262.6; a cancel's
round trip 103.9 / 108.0 against 250; serving 20.02 / 28.61); L1 start 7.35 / 8.03 ms; turn frames 5 and 9, plain p50
81.4 ms (p95 133.4), tool call 168.1 (233.6), resident memory 69.2 MB after the start and 93.5 MB after 30 turns.
Pushed 14:32:45, the branch deleted, done line 14:32:50; theseus-qb8o, rkvl and 9zft closed with the hash; R33's tree
and its 9.1 GB target removed. The joiner noted two things, filing nothing: the cold start's p95 (one slow start of
ten) and a p50 that had drifted from 21.6 ms at voice-echo's gate to 25.5 at docs-v083's, a code-free join, so the
machine's load, not the code; and the turn bench's lone slow run again (theseus-w7dk's case). 27 min from the take to
the done line. The store stays at format 23.

**The install** (installed 2026-10-06 17:22 at 938a8dc8, install #8). In a voice call on the owner's daemon, the next
turn is told what was heard, cut and never said; replies are shaped for speech (the framing line, `speakable()`'s
sentences); a failed voice turn says so aloud; `voice.cut` and `voice.resumed` rows are written; health's voice line
counts resumes; and `theseus.voice.cuts` and `theseus.voice.resumed` reach an OTLP endpoint when one is set. No config
key. R33's theseus-b6vz and ved2 are marked in the install plan "fix before voice is used for anything that asks". The
live check is the report's four steps and R33's four (a "yeah" while an answer is prepared, an approval in voice, a
`/stop` while an answer is prepared, a reply with a `---` rule or checkboxes). Health after the restart (17:22:53; the
build 17:07:24 to 17:22:23, release-thin 13 m 54 s at load 35 to 48; the install 17:22:48 to 17:23:01): `theseusd
check` exit 0, startup serving at 53.6 ms (store 21.4, kernel 23.9 ms, at load 38 to 48 with CPU pressure at 63 %), 9
secrets ready 1.33 s after the start, Discord ready, the judge's live packs as before (`security.v3`, `route.v1`,
`rerank.v1`), memory live on the `baseline` arm, voice ready with `0 resumed` (this join's count), `cgroup: delegated`,
the unit active with NRestarts 0, and no error or warning in the journal; no config change, the old daemon's stop 298
ms, the S3 backup caught up at position 5,759 from 17:23:06 (12 s), and the store at format 23.

**Divergences.** The framing line rides every voice turn's stored input, not a voice place's system block, which is the
compiler's area (R33: keep it per turn for the live check, then move it out of the stored input). The heard line is
stored in the input, so `theseus history` and the cockpit show it as the caller's words (the Discord text channel shows
only the `🎙️` transcript); a node of its own is later work. `speakable` reads long numbers whole: the framing asks the
model not to write them, and a rounding rule would change what was said. A table with no pipe line next to a separator
is still read row by row (accepted as rare).

**Known gaps.** theseus-zcxx, nthu, b6vz, ved2 and z9nq (all P3). At the review the DM thread folded ved2, b6vz and
nthu into batch 10's voice-dave task (Item 230), and zcxx with R33's three echo probes (`holds_e1`,
`holds_e2`, `holds_c1`, the regression for "the echo is judged against what was played") into its voice-holds task
(Item 231), both launched after this join. The `tests_outbox` refusal test's two failures under load in
the cloud remain unexplained without their output (not reproduced in 8 runs of the recipe and 2 harder ones).
Owed by the review and not written at the join: theseus-discord's AGENTS.md (`runtime/voice/notes.rs`, the framing
line, `FAILED_TURN`) and theseus-voice's (`speakable` is what the engine speaks).

### Item 217. Names: no person's name in the public tree; the owner is written "the owner" (zeroaltitude where an account or identity is meant), the assistant Tabitha/Claude, and the exam's persona zeroaltitude (theseus-2n9k and theseus-1xgs; the owner's ruling of 2026-10-06 12:55, amended at 13:31; the `names` lane, a subagent of the DM thread, run 1 spawned at about 13:09 and cancelled at 13:39 with nothing joined, run 2 from 13:39 to 14:52 on the amended brief; b96d1c41, the rename re-run on the newest main at the join; joined 14:48 at ea34457e, a signed merge onto 4db4cfc0, by the lane itself; reviewed 15:37 by the DM thread; installed 2026-10-06 17:22 at 938a8dc8, install #8)

**Why.** Asked at 12:47 whether the owner's first name should stay in the public repository, the owner ruled at 12:55:
"any reference to names in the source code, or any visible text on github, should be revised to not mention a name at
all. at most, 'zeroaltitude'. for other names, you could say collaborator". Measured then on origin/main f589d9cb: the
owner named on 1,713 lines and the assistant on 121, the full name once (LICENSE-MIT's copyright line); every hit a
comment, a doc, a fixture, a golden or the exam's fixtures, none a product string. The same hour the DM thread gave the
docs scrub an opt-in "a person's name" family, set it in the reviewers' branch scrub, and put the no-names rule in batch
10's preamble. At 13:31 the owner amended the rule for the assistant ("You can feel free to name yourself with /Claude")
and for history ("No need to revise history … don't worry about rewriting history, i'm not that concerned"), so the
assistant keeps its name, always written Tabitha/Claude, commit trailers name Tabitha/Claude as co-author, and the
history-rewrite issue (theseus-up4k) closed as won't do. Run 1 of the lane (the rename applied uncommitted in its
worktree, nothing joined) was cancelled at 13:39 and re-spawned as run 2 with the amended brief. theseus-1xgs (the
exam's fixtures) was folded into theseus-2n9k.

**What landed** (docs, code comments, fixtures, goldens, the exam, the cockpit's sources, scripts and LICENSE-MIT's
holder; the merge 144 files, +1,986 −1,902; no behaviour change, no store format change (23 stays 23), no protocol
type, config key or package). One commit, b96d1c41, made by a rename script kept outside the repo, so the tree holds
no list of names; the script ran on f589d9cb for the proof and again on 4db4cfc0 at the join.
- **The rules and their counts at the join:** "the owner" 523 and "the owner's" 674 in prose; the identity written
  `zeroaltitude` in code strings and data (470), in docs' code spans (37) and in prose handles (`@`, `discord:`, slugs:
  12); 154 identifier tokens in code; 15 format captures (`{OWNER}`); LICENSE-MIT's holder `zeroaltitude`; the
  assistant gaining "/Claude" (79) or becoming "Tabitha/Claude's" (30), a mention already followed by "/Claude" left
  alone; and nine fixes by hand, anchored on the rules' output so the script holds no name: the spec's definition of
  the owner (§0's "Owner vs operators": "whoever runs the Theseus runtime, whether that is zeroaltitude, a company's
  CTO, or a single person on their own laptop", where the rules had written a circular "whether that is the owner"), an
  appendix of the AWS toolset note ("The owner tied §2's owner model to …"), two references to a store snapshot whose
  directory was named after the owner (now its parent), three public figures in a research note named by their roles,
  and two test comments in `viewers.rs`.
- **Before and after** (lines with a word match of the owner's names, on 4db4cfc0): 1,800 lines (plus 3 inside longer
  identifiers) to 2. By area: docs/spec 803 lines in 27 files, docs/design 239, theseus-exam 163, theseus-core 157,
  theseus-discord 140, theseus-kernel 117, theseus-voice 107, and the rest under 12 each, all to 0 but
  theseus-judge's 2 (below). The assistant stays on 121 lines, every mention now followed by "/Claude" (109 gained it,
  20 had it).
- **Renamed identifiers:** a constant becomes `OWNER` in theseus-core's `tests_m3.rs`, theseus-discord's `runtime.rs`
  (with its imports in `extensions.rs`, `guilds.rs` and `publish.rs`), `runtime/voice.rs` (with a helper `owner()` and
  `in_owners_dm`), voice-heard's new `tests_heard.rs` (renamed at the join), and theseus-voice's `pipeline.rs` and
  `turns.rs`; in `viewers.rs` it is `OPERATOR`, since that test has the guild's own `OWNER`; an approval test's local
  binding is `owner`. No test name and no tracked path held a name, so nothing was moved. rustfmt's reflow of the
  longer handle took theseus-discord's `render.rs` from 3,001 to 3,009 lines, and its ceiling in
  `scripts/long-files.txt` rose to 3,009 with the reason.
- **Goldens:** the kernel's `kernel_frames.txt`, 69 lines, each only the principal's name, the renamed fixture; no
  other golden held a name. The doc comments of 8 generated protocol files changed with their Rust sources and
  regenerate byte for byte.
- **The exam** (1xgs): `exam-v2.toml`'s persona is `zeroaltitude` in 141 `who` fields, the world comment and one
  expected text ("assigned to zeroaltitude"), with theseus-exam's `generate.rs`, `item.rs`, `render.rs`, `tender.rs`
  and `tests/arms.rs` agreeing; the house-style trailer and its grader already read Tabitha/Claude. The exam's digest
  (the file's sha256) changes, so a store written from the old exam is refused ("the store was written from … not this
  exam") and old result records do not count on a resume, the designed behaviour for a changed exam; the ablation
  plan's digest is unchanged.
- **What reaches a running daemon:** glide's two tool descriptions and two error messages, whose example place label is
  now `DM @zeroaltitude` (the tools take a place's label from the places list, so nothing depends on it);
  `guardrails.toml`'s comment and two `note` strings (nothing reads them; the generated policies are unchanged); the
  Discord bindings example's DM name; and the exam's digest. The CloudFormation templates, stack policies, the config
  template and the judge packs are untouched.

**How it is proven.** **On f589d9cb, the rename alone:** fmt clean; the test build (13 m 23 s, cold); the kernel golden
as renamed, 1 of 1, and its writer gives back the same file; the core's golden 1 of 1; clippy clean; theseus-protocol 31
of 31 with protocol.gen regenerated identical; **the whole workspace suite, 2,997 of 2,997** (1 slow, 24 skipped, 418
s); the cockpit's `npm ci --offline`, lint, tests (90 of 90) and build; shape ok, the tree unchanged after. The exam's
62 tests pass, among them `the_committed_exam_v2_has_its_shape` and `the_v2_checks_score_known_answers`. The lane read
the whole diff (1,872 view lines, then v0.83's 84 renamed lines and voice-heard's four files), and a scan of capitals
raised 59 flags, each checked (parenthetical attributions such as "(the owner, 2026-09-25)"). **At the join:** 95 of the
99 changed code files equal the proven changes byte for byte, the other 4 being files voice-heard changed; in main's
tree on the merge, fmt clean, a 2 m 11 s test build and 310 of 310 targeted tests (voice, discord, protocol, the core's
telemetry and golden, the kernel's golden). **The scrub** with the names family on, over every tracked text file: "a
person's name" 1,886 on 4db4cfc0 and 2 on ea34457e (`security.v1.toml`), every other family identical between the two
trees. A word grep of the owner's three names finds 2 lines, both in `security.v1.toml`; the script's own check finds 0
word matches, 0 inside longer words and 0 encoded (base64, hex, `\u`) outside that exempt file, and a second run changes
nothing.

**The join** (the lane's own, through the queue). docs-v083's done line came at 13:56:20 (ceba1520); the lane took
`names-join` at 14:29:09, queued behind voice-heard's lock, and voice-heard joined at 14:32:50 (4db4cfc0). The lane
re-ran the rename on 4db4cfc0 and committed b96d1c41 (signed, parent 4db4cfc0, its message naming no one). The take
passed every check: the dry run clean (merge-tree's tree is the commit's own), the names check on the lane's tree, no
conflict, the staged tree the dry run's, every changed `.rs` file within its ceiling, `MANIFEST_FORMAT` 23, shape ok.
The signed merge **ea34457e**, 14:35:07. Its gate (14:38:41, minute 38, to 14:47:49, ok in 548 s, 71 s of it waiting
for the lock): **3,012 of 3,012** (1 slow, 24 skipped) in 319 s; lifecycle ok (cold start p95 29.0 ms of 50; from the
config copy 27.4; clean shutdown 71.1 of 100; SIGKILL then restart 69.3 of 150; binary swap 100.9 of 200; a cancel's
round trip 122.7 of 250); L1 start p95 17.75 ms; turn frames 5 and 9, plain p50 111.5 ms and tool call 215.7 ms with
one other tree building when the gate started; deny ok. Pushed 14:48:08 (`lane/names` was never on origin), done line
14:48:14; theseus-2n9k and 1xgs closed with ea34457e. Right after the merge commit the same script renamed the spec's
master copy (27 of its 28 files), and `diff -rq` against docs/spec at ea34457e is empty. The docs scrub's names family
is on by default since (off only with `SCRUB_NAMES=0`): it reads 0 over the README and docs/, and 2 over every tracked
file. The lane's worktree, branch and 8.4 GB target were removed. The store stays at format 23.

**The install** (installed 2026-10-06 17:22 at 938a8dc8, install #8). No behaviour change on the owner's daemon: the
installed binaries carry glide's renamed example label and the renamed guardrail notes, and nothing reads either. The
example bindings file's DM name changed, not the owner's bindings. Health after the restart (17:22:53): `theseusd check`
exit 0, startup serving at 53.6 ms (store 21.4, kernel 23.9 ms, at load 38 to 48 with CPU pressure at 63 %), 9 secrets
ready 1.33 s after the start, Discord ready, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`),
memory live on the `baseline` arm, voice ready with `0 resumed`, `cgroup: delegated`, the unit active with NRestarts 0,
and no error or warning in the journal; no config change, the old daemon's stop 298 ms, the S3 backup caught up at
position 5,759 from 17:23:06 (12 s), and the store at format 23.

**Divergences.** Two comment lines in `crates/theseus-judge/packs/security.v1.toml` still hold the owner's first name,
on purpose: the pack is compiled in and runs in shadow at every acting call, its sha256 is taken over the whole file
(comments included) and recorded with every judgment, and a pack's name must always mean one text
(`same_name_same_text`), so an edit in place would record security.v1 under a second sha256 in the owner's store, a
behaviour change. "Collaborator" is used nowhere: no one stood in a collaborator's place. Left as they are, as no
person named in prose: an eponymous date algorithm and two eponymous statistical intervals in code comments, the
README's classical quotation about the ship of Theseus, invented placeholders in tests and demos, and two web
addresses whose path is an individual's handle (a project's repository cited in the spec's Appendix F, and one in a
web test's fixture).

**Known gaps.** Two questions went to the owner (the DM thread, 15:37), both answered that evening with the
recommendation: `security.v1.toml`'s two comments (a: leave them until security.v1 is retired; b: edit in place and
accept a second sha256 for security.v1 in the owner's store; c: a new pack version with the comments rewritten):
**(a)**, 22:57, "take your recommendation", so the next real security pack version drops them (the question walk's S1);
and the signing key, whose user ID names the owner, so every signed commit carries it: **leave it**, 22:59, and if the
repository ever needs the name out entirely, the key and the history change together (S2). History keeps the names, by
the owner's decision of 13:31 (of origin/main's 1,372 commits, 145 messages name the owner and one commit's author name
holds the full name; no branch, tag or path does). The account's own profile is the owner's to check.

### Item 218. Pi as the benchmarks' fourth arm: Harbor's own Pi adapter measured as the other arms are (the version pinned at 1.0.4, the harness sampler around the run, an ATIF trajectory and the efficiency record from Pi's session log), the other arms' caps recorded and flagged for Pi, not enforced, and the report, recall and async benches taking it (theseus-jp9p; the owner's "yes!" of 2026-10-06 11:47; the tenth cloud batch's bench-pi session, launched by the DM thread at 11:49 and fired 11:52 from 2bf9e7d3, Opus 5.5, finished 12:56; 956a2f0a, 6fbed317, 47c1efcd and bac6d794; reviewed 13:35 to 14:08 by local reviewer R35, and accepted with no join fix at 14:32; joined 15:51 at cacad5c4, a signed merge onto ea34457e, by the batch-10 bench-pi joiner on its second gate, resumed after the account's session limit stopped its first run at 14:52; bench/ only, in the tree at install #8, 2026-10-06 17:22 at 938a8dc8)

**Why.** The benchmarks compared Theseus with Claude Code (and with OpenClaw through Harbor's own adapter, unmeasured).
Pi, the minimal coding agent, was the natural fourth: asked at 11:47 whether Pi should be the benchmarks' fourth arm,
the owner answered "yes!", and the DM thread launched the task two minutes later. The task: an arm measured exactly as
the Claude Code arm is (Items 167 to 169), so that every arm's score, dollars, tokens, CPU and memory are read the same
way, and Pi in the report, the recall bench and the async bench too, where cheap.

**What landed** (bench/ only: 14 files, the merge +1,865 −22 without the cloud files; nothing outside bench/, and
`bench/theseus-bench.toml` untouched; no Rust, no package, no store format change (23 stays 23)).
- **What Pi is, learned from its package, its docs and runs of the real CLI** against a local stand-in endpoint (no key
  used): a Node CLI (`pi`, which sets its process title to `pi`, the sampler's harness name), version 1.0.4 the latest
  (published 2026-10-05); `pi --print --mode json` runs one task and writes JSONL events, and `--mode rpc` reads JSONL
  commands (`prompt` with `streamingBehavior` `steer` or `followUp`, `abort`, …) until its input closes; thinking
  `off` to `max`, default `medium`; no limit on turns, spend or wall time; it retries a failed request itself and
  compacts itself when the context passes the window less `reserveTokens` (16,384), keeping `keepRecentTokens`
  (20,000); four tools (`read`, `bash`, `edit`, `write`) under a system prompt of about 2,300 tokens; its session log
  (JSONL version 3) gives every answer's `model`, `stopReason`, `errorMessage` and `usage` with Pi's own
  `cost.total`, and a failed request is persisted as an answer with `stopReason: "error"`. In print mode it exits 0
  even when the provider fails. Harbor 0.23 ships a Pi agent (`-a pi`) with no trajectory, and counters from the
  stream that leave the cache write out of the input.
- **The arm** (956a2f0a). `bench/harbor/pi_agent.py`, `-a pi_agent:MeasuredPi`, subclasses Harbor's `Pi` as the Claude
  Code arm subclasses `ClaudeCode`: Harbor's install, command line, key, `thinking` and name stay Harbor's;
  `PINNED_VERSION = "1.0.4"` (a `version` kwarg wins); `max_budget_usd` and `max_turns` are plain option fields, never
  flags; the sampler is uploaded after Harbor's install, started before Harbor's run and stopped in a `finally`; and
  after the run the arm writes the ATIF trajectory, the record, and Harbor's three counters from the record (the cache
  write inside the input, as the other arms count it). `pi_atif.py` writes Pi's session log as ATIF v1.7 (an agent
  step per answer with its thinking as `reasoning_content`, tool calls, metrics and results; a compaction, summary or
  usage entry a step of its own; a tool's nested usage a step marked `nested`). `efficiency.py`, additive only, gains
  `ARMS["pi"]` and the record's readers: spend from the session log (each entry once by its id), else the stream's
  `message_end`s, else the trajectory; `end`, the last answer's `stopReason` and error; and `limits`.
- **Fair limits.** Pi has neither a spend cap nor a turn cap, and none was added inside it. The arm takes `--ak
  max_budget_usd=2.0 --ak max_turns=200` as Claude Code's does, passes neither to Pi, and records them under
  `limits` with `enforced: false`, `over_budget` (dollars past the cap) and `over_turns` (answers past it). The
  README's "Fair limits" table sets the rest: the model, attempts and wall clock the same for all; each harness's
  thinking at its own default (Pi's `medium`); Pi pinned; a provider's failure read from the record's `end`; a
  timeout Harbor's, as for Claude Code.
- **The report** (6fbed317): `--arm pi=<job>` like any arm; an old Pi trial with no record is rebuilt from its files;
  where an arm's records carry `limits`, the per-arm table gains "Trials past the others' caps (not enforced)" and a
  note, and a report without such an arm is byte-identical. **Recall** (47c1efcd): `drive.py --arm pi`, compaction at
  Pi's own threshold set at the progression's window (`reserveTokens` the model's window less the progression's,
  `keepRecentTokens` the lesser of 20,000 and a quarter of the window, since the session found that Pi 1.0.4 silently
  skips a compaction when `keepRecentTokens` exceeds the whole context). **Async** (bac6d794): `PiAsync` runs Pi in RPC
  mode on a FIFO, the injection a steering `prompt`, the input closed once `agent_settled` follows the last message;
  Harbor's command block-buffers its `grep -v` filter, so the arm runs it under `stdbuf -oL`.

**How it is proven.**
- **The session:** harbor's suite 89 tests (15 skipped without Harbor; 89 under Harbor's venv), `test_pi_agent.py` 20
  of them, each behaviour with a planted revert that fails it (15 plants: entries not deduplicated, a nested usage
  counted as a call, no stream fallback, an unpinned install, `max_turns` passed as a flag, the sampler stopped
  outside the `finally` or started after the run, Harbor's counters kept, no trajectory, results not tied to their
  calls, the cache write left out, summaries not steps, `over_budget` always false, `end` from the first answer, the
  arm misnamed). The real Pi 1.0.4 on the stand-in: its record from the session log equal to its record from the
  stream (2 calls, 1 tool call, $0.00579), the trajectory valid in Harbor's model, the sampler seeing `pi` as the
  harness. Theseus's and Claude Code's records byte-equal to the old module's. report 11, recall 71 (plants r1 to r8,
  each failing a driver test; by hand the smoke ran 30 turns with 9 of 9 facts delivered, and at a 10,000-token window
  Pi compacted at turn 11 with the summary's cost booked to that turn), async 44 (plants a1 to a7; by hand the steer
  came mid-tool-call, Pi answered `queued`, delivered it before its next model call and settled once). The cloud gate
  failed only the 33 known L1 tests (the VM runs as root).
- **The review** (R35, review commit 79ecd199 on f589d9cb; no Rust built). The four suites on the merged tree at the
  report's counts under both Pythons (the host's 3.14 and Harbor's venv), async's and recall's Theseus drivers on
  frozen copies of main's debug binaries. **Byte-equal records,** old `efficiency.py` against new: every record call
  the tests make (157 comparisons under the venv, 140 under the host, 0 mismatches) and **725 real records** from
  earlier runs' jobs (365 Theseus, 179 Claude Code, 179 Claude Code async, 2 ledger records: 0 mismatches); the report
  without a Pi arm byte-identical in all five files. The caps' arithmetic: `over_budget` is Pi's whole bill against the
  cap, `over_turns` answers against the cap (Claude Code's `num_turns` equals its distinct answers in 118 of 171
  earlier trials). Pi's exit 0 on a provider failure: the record's `end` catches it (a built trial reads `end:
  {stop_reason: "error", …}`), a failure before any answer is a Harbor error through `pipefail`, and the report never
  counts one as a pass. **8 planted reverts of R35's, 5 caught;** the three misses are test gaps (theseus-p6kd).
  **Thinking, measured:** Theseus sends adaptive thinking and no effort, so Sonnet 5.5 runs at its default, `high`;
  the host's Claude Code 2.1.290 sends effort `medium` (captured on a loopback stand-in); Pi 1.0.4 maps its default
  to `medium`. **The timeout, settled without a model:** Harbor's Docker environment ends a cancelled exec by
  terminating its `docker compose exec` client, and a probe's loop ran on; Terminal-Bench verifies in the same
  container; an earlier run's one rewarded Claude Code timeout wrote 5 session entries after its cancel; only the
  Theseus arm stops its agent. **Live, one fix-git trial on the Pi arm** (cap $0.50, guarded at $0.40): **reward 1.0
  for $0.028861** (setup 52 s, agent 15.6 s, verifier 5.2 s); the record from the session log, input 14, cache read
  18,477, cache write 5,971 and output 1,021 tokens, 6 model calls and 5 tool calls, the harness one process at 0.54
  CPU-s and 128 MB peak RSS, the sampler `ok` over 62 samples, `limits` unenforced and both flags false; the cost equal
  three ways (Harbor's, the record's, and the catalog's $2 / $10 / $0.20 / $2.50 per million, which is Theseus's
  catalog price for Sonnet 5.5); Harbor's input counter equal to input + read + write; the trajectory valid (7 steps);
  all six answers at thinking `medium`.
- **FAST.** Untouched: Python under bench/ only, nothing on Theseus's start or turn path. The Pi arm's own cost in the
  live trial: the harness 0.54 CPU-s, the sampler 0.24 % of a core.

**What the review found** (none a fault in what the branch built; all to settle before Pi's first published run).
**theseus-sgpx (P2):** a timed-out Pi, or Claude Code, keeps working in its container while the verifier runs,
unrecorded; the fix is a SIGTERM to the agent on `CancelledError`, as the Theseus arm does. **theseus-bpeg (P2):** the
report's "Trials with an error" row and trials.csv miss a Pi trial that failed at the provider, since Pi exits 0;
read the record's `end`. **theseus-n6p5 (P2, waiting for the owner):** the comparison is not level, Theseus at `high`
and the other two at `medium` on the same model, likely under the first full run's published numbers too.
**theseus-p6kd (P3):** the three uncaught plants (the caps row on turns alone, a stream-only trial's `end`, recall's
`aborted` rule), a retried request counted toward `over_turns`, and `pi --version` kept nowhere. **theseus-a5we
(P3):** Pi overlays newer model-catalog data (prices, the thinking map) from its own site unless run offline, so run
every Pi arm with `PI_OFFLINE=1`; and plan Pi's recall run at Pi's own measured overhead (on Theseus's plan Pi's
context stays about 11.4k tokens short of the window, so it never compacts on the smoke and its after-compaction probes
come free). R35's recommendations, which the DM thread adopted unless the owner objected (14:32): keep Pi as shipped,
with the caps recorded and flagged, and publish its score with the "past the others' caps" count beside it (and, when
that count is not zero, the score with those trials counted unsolved); set effort explicitly and equally, `medium`
on every arm; keep the steer for the async injection; pin Claude Code's version for published runs.

**The join** (batch 10, the branch alone; two runs of the joiner). The first run's dry runs before the lock (14:43:17)
were clean on origin/main 4db4cfc0 and on local main ea34457e (the names lane's merge, not yet pushed): format 23 on
every side, no file changed on both sides since the base 2bf9e7d3. Its first take (14:43:32) found `names-join` open and
queued behind it, touching nothing. The second take (14:48:22 to 14:48:26), with main = origin/main = ea34457e: the dry
run's merged bench/ is R35's review commit's, 0 files differing; the merge clean, no conflict, no resolve.py, no join
fix; staged 14 files, +1,865 −22, the tree the dry run's less the cloud files; the scrub's one hit the README's link to
Pi's repository, known. bench's suites on the merged tree (14:48:51 to 14:57:12, at load 11 to 16 beside compiling
trees, on frozen copies of ea34457e's debug binaries): report, recall and async green under both Pythons at R35's
counts; harbor red only on `test_sampler`'s two cost bounds (a core share of 0.0085 against 0.0080; a read's cost 11.88
µs against 9.32), theseus-ufe5's known load flake, which bench-bounds fixes (Item 227); the DM thread's rerun of harbor
alone at 15:27, at load 1.1: 89 of 89 under both. The account's session limit stopped the first run at 14:52 with the
merge staged; the second run (15:32, on the DM thread's resume message) found main's tree as left (the staged tree
unchanged, no lock file, origin unchanged) and committed the signed merge **cacad5c4** (ea34457e and 59316822) at
15:36:25. **Gate 1** (15:36:35, minute 36, to 15:42:30) was red on one test of 3,012, `theseusd::bench_profile
a_first_byte_timeout_is_retried_inside_the_headless_turn` ("no retry inside the turn", left 2, right 1), theseus-jtrc's
known load flake (P2), at load about 10; alone it passed 3 of 3. **Gate 2** (15:44:27, minute 44, to 15:50:52, ok in 385
s): **3,012 of 3,012** (1 slow, 24 skipped) in 307.8 s; lifecycle ok (cold start p50 26.2 / p95 28.3 ms; from the config
copy 26.5 / 29.6; clean shutdown 36.6 / 47.3; a reply's post in flight 74.0 / 80.5; SIGKILL then restart 30.0 / 34.4;
binary swap 50.0 / 61.3; restore 482.2 / 696.3, unbudgeted; a cancel's round trip 106.5 / 120.7 against 250; serving
20.81 / 28.06); L1 start p50 10.78 / p95 18.25 ms; turn frames 5 and 9, plain p50 84.8 ms and tool call 161.3. Pushed
15:51:03, the branch deleted, done line 15:51:20; theseus-jp9p closed, with notes added to theseus-ufe5 and
theseus-jtrc. The joiner's two observations: the lifecycle bench's unbudgeted restore roughly doubled from the names
gate on (231.2 ms p50 at voice-echo's gate, 251.1 at voice-heard's, 426.8 at the names lane's, 482.2 here, every phase
growing), while neither join changed behaviour, so the machine's IO beside the gate is the likelier cause (IO pressure's
5-minute average 5.5 % some, 4.3 % full); and jtrc had now failed a gate, a negative assertion, so its P2 stands. The
store stays at format 23.

**The install** (bench/ only; in the tree at install #8, 2026-10-06 17:22 at 938a8dc8). Nothing for the owner's
daemon: no installed binary, config key, package or store format change. Install #8's health is as told in
Item 216.

**Divergences.** Pi's caps are recorded, not enforced, by the brief: an enforced cap would make it not Pi as shipped.
`keepRecentTokens` at a quarter of a small window is the session's choice, recorded in each run's `run.json`. Recall
plans Pi's run at Theseus's overhead for now (theseus-a5we). The report expected `pi --version` in `agent/setup`;
under Harbor 0.23 that directory is empty, and the version is the pin (`agent_info.version`).

**Known gaps.** theseus-sgpx, bpeg and n6p5 (P2), p6kd and a5we (P3): after this join the DM thread wrote batch 11's
bench-fair task for them, with Claude Code's arm pinned (Item 234). No Pi run had landed in this range, so
there is no Pi score yet. Owed by the review and not written at the join: a Pi paragraph and results row once a Pi run
lands (since the bench-reports lane, Item 220, a run's write-up is its own report under
`docs/benchmarks/`); the bench program plan's arms (principle 5's three arms become four; Pi in the async bench's
"own means" and the recall bench's "own memory"), a plan kept outside the repository; and bench/README's fair-limits
row "A timeout", once theseus-sgpx is fixed.

### Item 219. Imported skip: the session lists read the live sessions by key and never decode an imported record, a page steps over the import's births in one walk, and the learning tender's task-brief walk steps over imported sessions by key; the precondition the owner's history import met right after install #8 (theseus-7087; R24's finding on soul-import, Item 201; the tenth cloud batch's imported-skip session, launched by the DM thread at 09:36 as the import's precondition and fired 2026-10-06 09:38 from 79be3213, Opus 5.5, finished 12:11; 4a2dda64, 592a9a43 and 1be86613; reviewed 12:45 to 14:41 by local reviewer R32, with a resolution and a join-fix test, and accepted at 15:37; joined 16:24 at 5ac5c23c, a signed merge onto cacad5c4, by the batch-10 imported-skip joiner; installed 2026-10-06 17:22 at 938a8dc8, install #8, the import following at 17:26)

**Why.** R24's review of soul-import (Item 201) measured what a full import would do to the session lists: with
21,151 imported sessions in a debug build, the whole `session.list` went from 23.3 to 501.0 ms p50 and a page of 20
from 19.7 to 210.0 ms, since both read past every imported session (theseus-7087, P2). The cockpit polls the whole
list every 3 s. `Core::session_list` read and decoded every SESSION record (`Store::list_sessions`, `latest_of_kind`)
and dropped the imported ones after; `confirm.list`, `compilation.list`, `session_page`'s fallback and the learning
tender's task briefs did the same, and a page's loop over `newest_keys` read the records of every key it met,
imported or not. The owner's decision of 09:42 (the history imported whole, with no staging) kept one condition, its
timing: the import follows this fix.

**What landed** (theseus-store and theseus-core: the merge 12 files, +671 −48 with the resolution and the join fix,
without the cloud files; no store format change (23 stays 23: no record, field or kind changed, and both new reads
walk tables the index already keeps, `bykey` and `bybirth`), no protocol type, config key or package).
- **The whole lists skip imported sessions by key, unread** (4a2dda64). theseus-store gains
  `Store::latest_of_kind_where(kind, keep)`: the trait's default filters `latest_of_kind`; `WalStore` walks the kind's
  key table alone (`RedbIndex::positions_of_keys_where`) and reads only the kept keys' records (`read_many`), so a
  failed key costs its index row and its record is never read. It also gains `records_read_here()`, a per-thread count
  of the records read from the log (in `Inner::read` and `Inner::listed`), beside `frames_written_here`, which the
  tests count. theseus-core's `Store::live_sessions` is `latest_of_kind_where(SESSION, !import::is_imported)` (an
  imported id starts `ses_ep`), read by `sessions_by_activity` (the whole `session.list` and `confirm.list`),
  `session_page`'s fallback, `compilation_list` and the learning tender's `task_briefs`; `session_list`'s own filter is
  gone. Health's fallback `session_totals` keeps the whole read on purpose: the projected count is the index's sum
  over every SESSION key, imported ones included, and the fallback must give the same number.
- **A page steps over the import's births in one walk** (592a9a43). `RedbIndex::keys_by_birth_where` walks `bybirth`
  once in one read transaction and steps over a key the predicate fails before its `bykey` lookup;
  `Store::newest_keys_where` reads only the kept keys' records, and `newest_keys` is it with every key kept, so
  `sessions_paged` is one call and a decode. **The answers are unchanged:** `more`, and so the `older` cursor, still
  counts every key past the page, kept or not, as the old loop's `(more || !last)` did, so a full page whose only older
  keys are imported still gives a cursor and the next page is empty with none. The session chose this range read over a
  key space of imported sessions' own, which would have been a format bump and a change to how the importer writes and
  erases them.
- **The tests** (1be86613 and the two before): theseus-store's `store/tests_keyed.rs` (19 live keys read and exactly 19
  records, past 600 skipped; every page for n = 1, 2, 3, 5, 7, 20 and 1,000 equal to the old walk's keys and cursor,
  each page reading only its own records); theseus-core's `rpc/tests_imported.rs` (live sessions before, between and
  after two imports of 1,000 under two tags, a session parked on a question, and an erase: at each stage the whole list
  equals the old read and every page of n = 1, 2, 3, 4, 20 and 1,000 the old walk's, the whole list, a page of 20, the
  page past the first import and `confirm.list` each read under 60 records, and the second import and the erase add not
  one record to any of them). theseus-core's AGENTS.md names `Store::live_sessions` and `newest_keys_where`.

**How it is proven.**
- **The session:** the targeted set, 22 passed; five planted reverts each failed (the whole list or `confirm.list`
  back on the whole decode: the import's records read, `[3029, 29, 4, 3029]`; the page back to the n-at-a-time loop;
  the page's predicate inverted; the store's walk without its skip). Under the 4-core cloud VM's load recipe the
  heavy test first met nextest's 120 s kill, so the last commit cut its imports to 2 × 1,000 (3 of 3 passed, 72 s).
  **FAST, release-thin on the cloud VM**, scratch daemons of main and the branch, 21,151 synthetic episodes imported:
  the whole list p50 **146.4 → 2.2 ms**, a page of 20 **46.4 → 2.5 ms**, `theseus sessions` 145.9 → 5.2 ms; after the
  erase 136.9 → 2.4 and 46.9 → 2.7 ms; before the import equal. Nothing on the start path or a turn's path; the one
  change on every read path is the thread-local count, a `Cell` add per record read. The cloud gate failed only the 33
  known L1 tests (the VM runs as root).
- **The review** (R32, review commit 26847cce on f589d9cb, with the resolution and the join fix; in its own tree):
  fmt, the test build, clippy, theseus-protocol 31 (protocol.gen unchanged) and shape clean; targeted 161 of 161;
  **the whole workspace suite 2,999 of 3,001** (both failures load flakes outside the branch, each 5 of 5 alone:
  theseus-0bq1's outbox card, and bench_profile's first-byte test, which R32 filed as theseus-jtrc); under the load
  recipe 3 of 3, the heavy test 1.5 to 4.4 s on the owner's 16 cores, so no cut was needed. **R32's live A/B** (debug
  builds, arms A B B A in one hold, PSI read beside each block, 21,151 synthetic imported sessions): after the import
  the whole list p50 **600.9 → 16.0 ms**, a page of 20 **160.8 → 23.2 ms**, `theseus sessions` **460.3 → 31.4 ms**;
  after the erase 448.2 → 12.4, 183.0 → 15.3 and 553.8 → 25.5 ms; before the import equal within the noise; the
  answers the same on both arms in all four runs (5 live sessions, each page walk taking each live session once,
  health 21,156 sessions), and a re-import after the erase refused 21,151 of 21,151 on both. **Five probes** of R32's
  (an import older than every live session, where the kept odd answer shows; `compilation.list`'s and `confirm.list`'s
  answers equal to the whole read's; health's projected, fallback and keyed counts equal; the count staying on its
  thread; 100,000 live ids of each kind, none starting `ses_ep`). **11 planted reverts**: the report's five and R32's
  R1, R5 and R6 fail as they should; R3 (`more` over kept keys only, an answer that changes) fails only R32's probe;
  R2 (an index lookup per imported key) and R4 (`compilation.list` on the whole read) pass everything (theseus-ve34).
  R32 also checked the walk's consistency (one snapshot, then one `read_many`: a session created during a page's walk
  is listed by the next first page, never twice) and that an erased imported session stays out (the erase's receipt
  keeps the `ses_ep` key).

**What the review found** (none blocks the join; all assigned to main). **theseus-jtrc (P2):** bench_profile's
first-byte test saw two model requests with transient retries off, under load, not this branch's code. **theseus-revl
(P3):** health's session count includes imported and erased sessions, so after the import "sessions" reads about
21,151 higher; R32 recommends showing live and imported apart. **theseus-26jo (P3):** the residual walk, about 2 to 2.5
ms in a release build at 21,151 and linear in what is imported (12 to 32 ms in a debug build, where a page allocates
every imported key it walks): imported keys are one contiguous run in key order, so the whole list can skip them as a
range with no format change, and a page can test a key before allocating it. **theseus-ve34 (P3):** the three plants
the branch's tests do not catch. R32's calls: keep the empty page after a full page (no surface in the repository
reads pages today, and changing it would move the walk onto every first page), with one doc line on
`SessionListResult.older`; accept the residual walk for the import; no cut of the heavy test; leave the importer's
overwrite of a live session's key, impossible while the id formats differ; and let the import go once this joins.

**The join** (batch 10, the branch alone). The joiner's dry run before the lock (16:01:58, on origin/main cacad5c4):
merge-tree's one conflict, `crates/theseus-core/src/learning/system.rs`, as R32 expected (judge-reads' join, Item 209,
had replaced `task_briefs`' whole read with `sessions_from`, a walk of the births that decoded every session and skipped
an imported one after its decode); theseus-core's AGENTS.md, changed on both sides (the names lane), merged clean; R32's
`resolve.py` (keep main's walk, and make it walk `newest_keys_where(SESSION, before, 64, !is_imported)` so no imported
record is read, with `live_sessions()` as its fallback, and move AGENTS.md's sentence) and `joinfix.py`
(`tests_learning::the_task_briefs_walk_reads_no_imported_record`: the learning read past an import of 300 sessions reads
exactly what it reads past an import of 1; R32's plant of main's decode-then-skip fails it alone, 378 records read
against 79) left no marker and rustfmt clean; the resolved tree 35daa212, differing from R32's review commit only in
main's own 170 changes since f589d9cb; `MANIFEST_FORMAT` 23 everywhere. The guarded take (16:03:11 to 16:03:31) took
`cloud-imported-skip-join` with the queue clear (main = origin/main = cacad5c4); at the merge, git's rerere replayed
R32's recorded resolution of `system.rs` (the resolution cache is shared by every worktree), `resolve.py` found its two
`system.rs` steps already applied and applied its AGENTS.md step, and `joinfix.py` added its test; staged 12 files, +671
−48, **the staged tree exactly the dry run's resolved tree**, built from the markers with no rerere. The scrub, with the
names family on: 0 hits. The warm (16:04:22 to 16:11:15, the test build 4 m 55 s at load 21 to 35); the targeted suites,
**246 of 246** in 40.4 s (theseus-store 83; theseus-core's rpc, learning, tests_learning with both walk tests, import
and the golden, 80; theseus-protocol 31; the CLI's goldens 52). The signed merge **5ac5c23c** (cacad5c4 and 3a3c6290),
16:13:09. Its gate (16:13:16, minute 13, to 16:24:20, ok in 664 s, 181 s of it waiting for the lock behind two review
steps): **3,016 of 3,016** (bench-pi's 3,012 and the branch's 4; 1 slow, 24 skipped) in 299.1 s, no hour crossed, no
known load flake red; lifecycle ok (cold start p50 24.7 / p95 27.5 ms; from the config copy 26.0 / 29.1; clean shutdown
39.0 / 52.9; a reply's post in flight 75.9 / 81.0; SIGKILL then restart 31.6 / 37.9; binary swap 51.9 / 53.5; restore
255.9 / 298.5, back from bench-pi's gate's 482.2, which bears out that the doubling was the machine; a cancel's round
trip 104.4 / 109.3 against 250; serving 20.94 / 28.64); L1 start 7.46 / 8.78 ms; turn frames 5 and 9, plain p50 84.7 ms
and tool call 183.7 (bench-pi's gate read 161.3; nothing of this branch is on a turn's path, so a later quiet gate
decides). Pushed 16:24:36, the branch deleted, done line 16:24:53; theseus-7087 closed; R32's tree and its 8.4 GB target
removed. The joiner's note for later joins: rerere replays a reviewer's resolution, so a joiner keeps comparing the
staged tree with a dry run built without it. The store stays at format 23.

**The install** (installed 2026-10-06 17:22 at 938a8dc8, install #8). On the owner's daemon, `session.list`,
`confirm.list`, `compilation.list` and `theseus sessions` read the live sessions by key and never an imported record,
and the learning tender's task-brief walk steps over imported sessions by key. No config key. Health after the restart
(17:22:53): `theseusd check` exit 0, startup serving at 53.6 ms (store 21.4, kernel 23.9 ms, at load 38 to 48 with CPU
pressure at 63 %), 9 secrets ready 1.33 s after the start, Discord ready, the judge's live packs as before
(`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready with `0 resumed`, `cgroup:
delegated`, the unit active with NRestarts 0, and no error or warning in the journal; no config change, the old
daemon's stop 298 ms, the S3 backup caught up at position 5,759 from 17:23:06 (12 s), and the store at format 23. The
installer's baseline with the 6 live sessions (17:24:37, load 38.7, 10 runs each, p50 / max): `theseus sessions` 22.1 /
50.5 ms, a page of 20 on the socket 0.3 / 5.6 ms, the whole list on the socket 0.2 / 0.3 ms.

**The import this join made possible.** Right after the install (whose backup of binaries and store, taken before it, is
the rollback), the owner's prior assistant history went in through `theseus import openclaw` (17:26:22 to 17:28:26, 2
min 4 s from the first start to the last exit): **21,779 episodes imported, 0 skipped, 0 rejected, 115,346 nodes**, with
no restart (the startup line unchanged). Health answered before every file and every 2 s throughout (p50 30.5 ms, max
113.1 ms), and the journal from 17:26 held no error or warning. **The session lists stayed in the low milliseconds**
(17:34, BM25 and durability caught up, load 36; before → after, p50): the whole list on the socket 0.2 → 4.0 ms (max
5.6), a page of 20 on the socket 0.3 → 4.8 ms (max 6.8), `theseus sessions` 22.1 → 12.5 ms (its own process start
dominating, the load having fallen); every answer held only the 6 live sessions, and the worst single reading of a list
on the socket was 14.9 ms, during the index's backfill. Health's session count went from 6 to 21,785 (theseus-revl).
BM25 caught up 75 s after the last file; the vectors embed in the background over days (about 0.9 s of CPU per text on
one thread, paced by CPU pressure); durability shipped the import's frames and caught up 3 min 46 s after the last file
(position 5,759 → 143,081). A BM25 search, with no model call, finds imported episodes with the source episode's session
first. `theseus import erase` was never run; an erase hides and does not delete (Item 201).

**Divergences.** The whole list's key walk and the page's births walk still visit each imported key's index row
(no record read): a few ms at 21,151 in a release build, flat per row and growing with the import (theseus-26jo).
Skipping the run without walking it would need imported births kept apart, a format bump the brief did not ask for.
The page's cursor rule is kept exactly (one empty page after a full page whose only older keys are imported), by
the brief's "answers unchanged" and R32's call.

**Known gaps.** theseus-revl (health's count, P3), theseus-26jo (the residual walk, P3), theseus-ve34 (the three test
gaps, P3), and theseus-jtrc (P2, a load flake outside this branch, later batch 11's daemon-flakes task,
Item 236). Owed by the review and not written at the join: theseus-store's AGENTS.md
(`latest_of_kind_where` and `newest_keys_where` beside `keys_ending` as reads that skip by key, and `records_read_here`
beside `frames_written_here`), and the doc line on `SessionListResult.older` ("a page may be empty when every older
session is imported; it has no `older`").

### Item 220. Benchmark reports: `docs/benchmarks/`, a report for every benchmark run, eighteen from the retrospective (Terminal-Bench, Harbor, the gate's speed benches and their A/Bs, the memory exams, retrieval, recall, async and the token estimate), with the index, the house palette, and bench/report's charts, statistics and drafting tool; every run ends in a report (theseus-qla4; the owner's ask of 2026-10-06 13:50, a standing rule from then; the `bench-reports` lane, a subagent of the DM thread spawned at 13:56, Opus 5.5, with three drafting helpers, stopped by the account's session limit at 14:52 and resumed at 15:34; 25e2f525, a7fb6ad1 and 1b2ff047; joined 16:36 at 351680a1, a signed merge onto 5ac5c23c, by the lane itself; reviewed 17:06 by the DM thread; docs/ and bench/ only, in the tree at install #8, 2026-10-06 17:22 at 938a8dc8)

**Why.** The owner, 13:50: "a novel, high quality report for any and every benchmark we run, delivered as a markdown
file with associated plots as necessary in docs/benchmarks in the theseus repo; retrospective from past benchmarks
encouraged". Until then the only published write-up was `docs/benchmarks.md`, the first full Terminal-Bench run's
(Item 148); every other measurement (the gate's speed benches, the lanes' A/Bs, the memory exams, the recall and async
smokes) lived in lane reports outside the repository. The DM thread made it a standing rule at once: every benchmark run
ends in a report in `docs/benchmarks/`, written into bench/README.md and the bench program plan.

**What landed** (docs/benchmarks/, `docs/benchmarks.md`, `docs/README.md`, bench/README.md and the async and recall
READMEs, and bench/report/; the merge 180 files, +50,647 −256, most of it SVG; no Rust, no package, no store format
change (23 stays 23)).
- **18 reports in `docs/benchmarks/`**, one per past run, 2026-09-30 to 2026-10-06, each with the answer first, then
  the question, the setup, the results with their uncertainty and plots, the analysis, the threats to validity, the
  cost and the commands that run it again; each with its data file (`<name>.json`, the summary its tables use and its
  figures' specs), its figures as SVG in light and dark (116 SVGs, plus the palette's 2), and a CSV where it has trials
  or runs. Among them: the first full Terminal-Bench run (Claude Code 81.5 % [75.1, 86.5], Theseus 71.9 % [64.9,
  78.0], with the batching paragraph 73.6 %; by task, Claude Code better on 16 and Theseus on 5, p = 0.027; 13 of
  Theseus's 21 trials that ended early were harness faults), which folds in the old page; the worth spike; Harbor's
  efficiency checks (the harness apart from the model: Theseus 24 to 28 MiB and 16 to 25 ms of CPU a tool call, Claude
  Code 209 to 430 MiB and 175 to 394 ms, Pi 125 MiB and 108 ms); the async smokes; the gate's speed-bench history over
  240 gates on main from 10-01 to 10-06 (every phase's median far inside its budget; the slowest of ten crossed a
  limit in 32 of 240 gates, 13.3 % [9.6, 18.2], more with load); six speed A/Bs (opt-level 2, the install builds, the
  start path at 10,000 sessions, the ledger reads, one sync per job, the IO-stall fixes); the memory exams (v1's
  headroom, v2, the arms); retrieval in two reports (the embedding engines, and fusion); the recall smokes; and the
  token estimate. One suite word was added, `context`, for the context compiler measured against the provider's own
  count.
- **Numbers that differ from what was said at the time**, each report saying where its own differ: the first full run
  took 18.8 h of wall clock, not "4.0 h" (the driver rewrote `started_at` at the resume after the machine's restart);
  the published "error 2 / 2 / 4" splits into a provider timeout and an adapter stop overrun, two stop overruns, and
  four Claude Code install failures that never started; release-thin's CPU cost is about 7 % by pairs, not 10 %, and
  its rebuilds 1.69 and 5.70 times faster; the gate's lone-slow-run count is 16 by the report's threshold (theseus-w7dk
  said 17 of 191); the gate comment's miss rates by load (41 %, 10 %, 6 %) are 56.5 %, 27.5 % and 10.1 % over the week;
  exam v1's headline is its rescore with no model calls, +74.2 (+72.5 as run, both given); and the token estimate's
  "25 to 52 % low" is 20 to 34 % low.
- **The index**, `docs/benchmarks/README.md`: every run newest first with a headline each, the house palette with its
  figure and validation, how a report is written, how one is drafted, and what a Harbor run records.
  **`docs/benchmarks.md`** is now a pointer to the index and to the first full run's report, keeping that run's
  headline table, so every link into it still resolves.
- **The rule**, in bench/README.md ("Every run gets its report": where a report goes, what it holds, how it is drafted;
  a run is not done until its report joins), pointed to from the async and recall READMEs, and in the drafting tool.
- **bench/report/, standard library only:** `charts.py` (1,389 lines), eight chart forms, each in light and dark, every
  mark with its hover text; `stats.py`, Wilson intervals, a seeded bootstrap, the exact McNemar test and quantiles;
  `draft.py` (839 lines), whose `harbor`, `history`, `recall`, `async`, `plot` and `palette` commands write a report's
  data file, CSV, figures and an eight-section skeleton from a run's outputs; `efficiency.py`'s three Pareto charts now
  drawn by `charts.py`; bench/report's suite 36 tests (`test_charts.py`, `test_draft.py`, `test_report.py`).
- **The house palette:** the dataviz method's validated eight-slot categorical palette in its fixed order, each slot
  with a light and a dark step: `theseus`, `claude-code`, `theseus-batching`, `openclaw` (reserved), `bm25`,
  `vector`, `fused`, `entity`; `none`, `context` and `other` in a neutral gray, `oracle` in the secondary ink.

**How it is proven.** The lane's checker passes all 18 reports with 0 problems (the sections in order, the answer
first, every link and image resolving, every figure in both modes, data files under 150 KB). The palette's validator
passes every check in both modes: adjacent slots' worst colour-vision-deficiency ΔE 9.1 light and 8.4 dark (target 8)
and worst normal-vision ΔE 19.6 and 19.3 (floor 15); the three harness arms 9.2 and 9.4, and 24.0 and 20.9; three
light-mode slots under 3:1 against the surface take the method's relief rule (every figure has a table twin, its marks
labelled or keyed). Every figure in the tree was rendered by the shipped renderer and looked at, light and dark.
`draft.py` was proved on real past outputs, writing outside the repository: `harbor` over the first full run's 534
trials in 5 s, its tables matching the published numbers (128 of 178 = 71.9 % [64.9 %, 78.0 %], 131 of 178 = 73.6 %,
145 of 178 = 81.5 %; $24.72, $24.23 and $22.65); `history` over main's bench history since 10-01 in 0.8 s; `recall` and
`async` over earlier smokes. Both scrubs over the 180 files: the docs scrub (the names family on) exit 0, its 123 web
address hits each read (121 the SVG namespace, Harbor's public address and a loopback example, both already on main),
every other family 0; the organisation-program scrub 0; a grep by hand for agent names, home paths and "subagent", no
hit. Left out on purpose: measurements that are not benchmark runs, two that are neither public nor reproducible
(they ran over the owner's private data), runs not yet made (the held-out Terminal-Bench rerun, theseus-7gir.22; the
recall bench's full run; the 34b replay arm), and protocol checks and annotations, folded in as text.

**What the lane found** (none filed; for the owner or the DM thread). Stopping a timed-out Theseus turn on a CPU-bound
Terminal-Bench task overran Harbor's 60 s command timeout and ended 3 trials before their tests ran (harmless in the
first full run, where every arm failed that task). Claude Code's install fails on Debian bullseye task images, so four
of its trials never started, a fairness note for a rerun. The hard-task gap has no issue: without the harness faults,
Claude Code beat Theseus on 7 hard tasks to Theseus's 1. On 10-04, with no join, a plain turn's p50 rose from 67.5 to
75.8 ms (+8.3 [+3.7, +10.4]) and the daemon's resident memory after a start from 45.0 to 68.1 MB, while the frames
and every lifecycle phase held. The memory exam's decision rule cannot count a disclosure on a private-family item
that the comparison arm also failed. theseus-70vi looks closable (a measured Claude Code interrupt trial settled in
192 s on the fixed driver). One lane's own record of the token estimate is missing from disk, so its report rebuilds
the record from the lane's live rows. Harbor's `SamplerCost` cost-bound test is load-sensitive (theseus-ufe5).

**The join** (the lane's own, under `bench-reports-join`, taken 16:07:25 and queued behind imported-skip's lock, which
joined at 16:24:53). The lane waited for bench-pi's join (15:51:20) and dry-ran onto it: no conflict (bench-pi's
hunks in `efficiency.py`, `test_report.py` and the three READMEs disjoint from the lane's), both sides' intent standing
in the merged tree (Pi's record and its "past the others' caps" row; the Pareto charts drawn by `charts.py`, whose
legend names Pi), and imported-skip touching nothing under docs/ or bench/. bench's suites on the dry-run tree under
Python 3.14 and 3.12: report 36 of 36, recall 71 and async green under both; harbor 88 of 89 under both, the red one
`SamplerCost`'s timing test at load 25 to 29 (alone 2 of 6 at that load). **Take 1** (a7fb6ad1, 16:25:35) merged
clean, but `git diff --cached --check` found 1,070 lines: CRLF line ends in the 11 CSVs (the helpers' `csv` module
default; `draft.py` writes LF) and a blank line at the end of the first full run's report; the lane aborted its own
merge at 16:26:15, before any commit, with main untouched, and normalized the files in 1b2ff047. **Take 2** (16:26:45):
merge-tree clean, 180 files all under docs/ or bench/ and no `.rs`, no conflicted path, the staged tree the dry run's,
`--check` 0 lines, the store format 23. The signed merge **351680a1** (5ac5c23c and 1b2ff047), 16:26:58. Its gate
(16:27:01, minute 27, to 16:36:12, ok in 551 s, 104 s of it waiting for the lock behind three reviewers' builds):
**3,016 of 3,016** (1 slow, 24 skipped) in 294 s; lifecycle ok (cold start p50 36.0 / p95 47.5 ms against 50 + 7; from
the config copy 40.5 / 43.2; clean shutdown 46.3 / 83.0; SIGKILL then restart 34.9 / 51.8; binary swap 50.2 / 63.3; a
cancel's round trip 97.5 / 104.0 against 250; unbudgeted, the post in flight 77.4 / 92.0, a restore 236.5 / 247.0);
turn frames 5 and 9, plain 75.9 / 88.9 ms, tool call 153.4 / 171.2; deny ok. On 351680a1 itself: report and async green
under both Pythons, recall 71; harbor 89 under 3.14, and under 3.12 only `SamplerCost`'s timing test red at load 25,
main's own load-flaky check (the lane's bench/harbor/ is main's, 0 diff lines). Pushed 16:36:26 (`lane/bench-reports`
was never on origin), done line 16:36:31; theseus-qla4 closed. The store stays at format 23.

**The install** (docs/ and bench/ only; in the tree at install #8, 2026-10-06 17:22 at 938a8dc8). Nothing for the
owner's daemon. Install #8's health is as told in Item 216.

**Divergences.** Retrieval is two reports, not one: the vectors lane's probe and the recall lane's fusion grid are one
measurement in two stages (102 of 102 ranks agree across their harnesses), and the embedding spike asks a different
question on different data. **Pi has no colour of its own:** it became a measured arm when bench-pi joined
(Item 218), all eight slots were taken with slot 4 held for OpenClaw, so Pi is drawn as `other` with its
name on its point and in the legend.

**Known gaps.** Pi's colour is the owner's call: release OpenClaw's reserved slot, or validate a ninth colour. The
lane's findings above are unfiled. The bench program plan's "every benchmark run ends in a report" line is kept outside
the repository.

### Item 221. Judge turn cost and the judge sink: the judge's frames are written only between turns, with one clock a backlog pass and a busy-bound guard; a clean stop writes every settled judgment before the store closes; no point reads the ladder or the lineage (the warm read does, and a pack that would act judges in shadow until it); and `theseus-sim bench turn --judge` measures the judge's cost on a turn's path (theseus-0j2.8, theseus-289c, theseus-s1am, theseus-ych4 and theseus-3bl9; two cloud sessions in one merge: the ninth batch's judge-turn-cost, fired 2026-10-05 20:06 from 4a449460, Opus 5.5, its report at 22:54, e6e30a33, f93fbc38 and 8885cea0, reviewed 2026-10-06 01:24 to 01:52 and, relaunched after the account's weekly limit stopped it, 08:07 to 10:02 by local reviewer R28, stack B9-judge, and not accepted for its sink; and its fix round, the tenth batch's judge-sink, launched by the DM thread at 10:35 on judge-turn-cost's head 54de79da and fired 10:38, Opus 5.5, finished 13:15, e04d6943, 0afe76fd, a760b094 and 44f0a42b; the two reviewed together 13:35 to 15:49 by local reviewer R34, resumed after the account's session limit stopped it at 14:53, and accepted at 16:05; joined 16:57 at 938a8dc8, a signed merge of judge-sink onto 351680a1, by the batch-10 judge-sink joiner; installed 2026-10-06 17:22 at 938a8dc8, install #8)

**Why.** Two FAST questions the judge's wire-in left open. **theseus-0j2.8:** with `[judge]` on, the sink writes its
`judge.call` rows (with the breaker's moves, the shed count and the shadow budget's META record) in frames of its own,
up to 32 rows or every 2 s (§2.5 of the judgment design, Item 105); at jev-wire-in's join such a frame landed inside a
measured tool-call turn and failed the gate, so the gate's turn bench has run with the judge off since (decision (a))
while the lifecycle bench keeps it on, and nothing measured what a judged turn pays. **theseus-289c** (R4's review of
the ladder, Item 145): the ladder's first read is meant to happen after serving, on the blocking pool
(`Core::warm_ladder`), but every judged point asks `JudgeService::mode_for`, which loaded the ladder (and wrote missing
adoptions) on whatever thread asked first, a tool call's gate, a loop's end, an inbound message, a compile, under a
std Mutex; `placed` read the ladder and the lineage the same way, and the day's rollover re-read the day. A turn
submitted the moment a fresh daemon answers had 2 or 3 `pack.mode` frames before its answer. The owner's daemon runs
the judge with three packs live (`security.v3`, `route.v1`, `rerank.v1`, the owner's decision of 2026-10-04).

**What landed** (theseus-core's judge (`judge/sink.rs`, `judge/mod.rs`, the ladder, the lineage), `outbox.rs`'s stop,
the ladder's RPCs and the route step; theseus-sim's turn bench; theseus-discord's gateway test rig; the merge 31 files,
+1,883 −115, without the cloud files; Cargo.lock one line (theseus-sim on theseus-judge with its `fake` module), no
package, no protocol type, no config key, no store format change (23 stays 23: nothing stored changed)).
- **The measure** (e6e30a33, 0j2.8). `theseus-sim bench turn --judge` (`perf/judge.rs`, new, 738 lines): three arms,
  each its own scratch daemon on the stand-in model: `off`, the gate's turn bench; `loop`, the judge on at
  theseus-judge's fake Jev in process with the inbound packs off; and `packs`, every pack as wired, route.v1 live and
  scripted to a confident switch. Each frame is told the judge's (every record a `judge.*` or `pack.*` row, or a
  `judge.*` META record) or the turn's (checked against its trace as `bench turn` checks it), and each judge frame is
  placed before a turn's answer, after it (within 50 ms) or between turns; a first turn is submitted the moment the
  fresh daemon answers; blobs are counted, two syncs each. `--record` writes six new history columns. **What it
  found** (release-thin on the cloud VM, 3 runs of 10 turns of each kind): judge frames did land before a turn's
  answer (1 or 2 per arm a run), and the judge-on plain p50 moved (9.7 to 11.5 ms against 9.7); under strace the judge
  added about 20 WAL frames per arm, and its blobs (each judged state's file and directory, 258 to 306 syncs and 100 to
  110 ms per arm) were most of its disk cost, off the WAL's writer but on the same disk.
- **The sink writes only between turns** (f93fbc38, 0j2.8). Each frame waits for a moment between turns through the
  memory pass's writer handshake (`memory_pass::turns`, `Turns::between`, as consolidation writes: no turn running and
  none for 500 ms; past the 120 s quiet bound any gap; past the 600 s busy bound beside a turn), and judgments that land
  meanwhile join the frame, up to 32. The core hands the judge its running turns as it builds
  (`JudgeService::write_between`); the sink holds the turns, never the service, across the wait. A judgment's row and
  its facts stay in one frame; a press still finds an unwritten judgment in `pending`. What waits longer is a
  judgment's row, its sentences and its metric. The session rejected riding a turn's frames: that would put one
  session's judgments into another's turn and grow the 5 and 9 frame budgets.
- **One clock a backlog pass** (e04d6943 and 0afe76fd, s1am: R28 found the sink restarting its clock for every 32-row
  frame, so a busy daemon wrote 32 judgments per 120 s, live 32 rows of 1,551 settled after 180 s against 501 or more
  on main's sink). `sink::Queue` holds the settled, unwritten judgments (a `VecDeque` under a std Mutex, a `Notify`, a
  sender count, a writer lock and a `closed` flag), pushed by the recording's channel, which never blocks. A **pass**
  begins when a judgment lands on an empty queue and ends when a frame leaves it empty; every frame of the pass waits
  from the pass's start, so past the quiet bound the backlog goes out in the next gaps, frame after frame, each still
  waiting for no turn to run. **The busy-bound guard:** the clock's start is never older than the quiet bound before
  now, so a backlog that never empties writes a frame beside running turns only after 480 s of gapless turns, then
  one per 480 s (without it, every frame after 600 s). The frame stays at 32 rows, since a turn that begins mid-append
  waits for that append.
- **A clean stop flushes the sink** (a760b094, ych4: R28 found a stop losing every settled judgment not yet written,
  where main's sink lost at most its 2 s window). `Queue::flush` and `JudgeService::flush_sink` write everything queued
  in frames of 32 under the writer's lock, then close the sink, so a later batch is dropped with a debug line and
  nothing lands after the last checkpoint to be replayed. `Core::finish_stop`, the one clean-stop path (the `shutdown`
  method, SIGINT, SIGTERM, a restart onto a changed note; theseusd's socket and `--stdio` paths both), calls it after
  the posts settle and before the late rows and the last checkpoint, on `theseus_store::blocking`, with the stop phase
  `judgments written` and an info line (count, ms) when it wrote any. **A SIGKILL or a crash loses the queue**,
  accepted and said in the sink's module doc: a window on a quiet daemon, the backlog on a busy one; the spend is kept,
  since the budget's blocks are written before the calls. theseus-core's AGENTS.md's judge paragraph gains the
  backlog's clock, the flush and the loss.
- **No point reads the ladder or the lineage** (8885cea0, 289c). Before the ladder is loaded, `Ladder::given` answers
  `Ladder::unread` (the wired line under the config, with any pack that would act in shadow) and `JudgeService::placed`
  the root, so nothing is read or written on an asking thread. `Core::warm_ladder` reads the ladder and the lineage on
  the blocking pool after serving and, only if an adoption is missing, writes the adoptions in **one** frame after a
  quiet stretch (500 ms) and a moment between turns (before, one frame each). `judge.label` (whose notices' brake asks
  what acts), `pack.list` and the learning loop read the ladder first. Health's pack lines say the pre-read answer
  (`rerank.v1: shadow (until the ladder is read; wired live)`). `route_base` keeps a session's move while the ladder is
  unread instead of clearing it. A new day starts empty and reads nothing (every event since midnight in this process
  came through `land`). **The design call, decided: shadow until the read**, so a restarted build never acts on a pack
  the owner or a rule rolled back, at the cost of a few ms after serving in which route, rerank and v3's notices do not
  act. The test rigs that judge turns warm the ladder (`tests_judge::warm`).
- **The adoptions test** (44f0a42b, 3bl9: R28's plant dropping the warm read's between-turns wait passed it, since it
  looked at 300 ms and the warm read sleeps 500 ms) now holds its turn 2 s, four times the sleep.

**How it is proven.**
- **The sessions' tests:** `tests_sink_between` (a turn held past the sink's window: no `judge.call` row until it
  ends, and the row 500 ms or more after); `tests_sink_backlog` (with a 3 s quiet bound, 1,500 judgments with no turn
  timed for the sink's own rate, then 1,500 more settling while turns run 300 ms each, 200 ms apart; no frame lands
  inside a turn, and the backlog is written within the quiet bound plus three times the no-turn time plus 3 s, a bound
  measured against the machine since a fixed 8 s failed every loaded run); `tests_sink_flush` (a turn held, 200
  judgments settled and unwritten; `finish_stop` writes all 200 in 7 frames, a judgment settled after the stop is never
  written, and a new core on the store reads all 200; the whole stop with 200 pending took 29 ms in a debug build);
  `tests_ladder_unread` (a store where the owner rolled route.v1 back; a new core judges a turn at the inbound, gate and
  loop-end points with the ladder and the lineage unread, `reads() == 0`, no `pack.mode` row, route, rerank and v3 in
  shadow, health saying so, and the turn's frames equal to a judge-off core's; after `warm_ladder`, one read, route.v1
  rolled back and rerank.v1 live; a day on, still one read). Each fix's planted revert failed its test (the per-frame
  clock: "128 of 1500 written after 6.9 s"; the flush removed; the warm read's wait replaced by its sleep), and the
  between-turns wait removed fails all three sink tests. **The judged bench on release-thin** (judge-sink's head, 30
  runs, twice): the loop arm had no judge frame before any answer in 120 measured turns, every sink frame landing
  after the last turn; the packs arm had one frame before an answer in 3 of 4 kind-runs, by the counts a categorize
  mark or the shadow budget's block frame, not the sink; judged p50s against off, plain +1.0 to +1.8 ms. The cloud
  gates failed only the 33 known L1 tests (the VM runs as root) and, once, a load-sensitive tender test outside the
  branch.
- **R28's review of judge-turn-cost** (full stack, the whole workspace suite 2,911 of 2,911; 293 of 293 in the judge
  families): steps 1 and 3 good, proved by tests, plants and, for 289c, live (main's restarted daemon answered
  `route.v1: live` on a pack the owner had rolled back until its first read; the branch's answered shadow, then rolled
  back 3 s later, with no `pack.mode` row written by the second start). The sink not accepted, both defects shown live
  against main's sink.
- **R34's review of the two** (review commit a9ca8ea4 on f589d9cb, in R28's kept tree): fmt, the test build, clippy,
  theseus-protocol 31 and shape clean; **the whole workspace suite 3,003 of 3,004**, the one failure theseus-jtrc's
  load flake (3 of 3 alone); the sink tests 4.3 to 10.2 s. **Five planted reverts, four caught** (the per-frame clock;
  the flush removed; R34's flush that never closes, so a later judgment is written after the last checkpoint, 201
  against 200; the between-turns wait removed), with R34's own plant for 3bl9 (the wait swapped for a 1 s sleep)
  caught; the busy-bound guard removed passes every sink test (theseus-ju99). R34's probe of the guard (a 14 s turn
  held while 8 judgments settle every 100 ms): 6 frames inside the turn with the guard, 36 without. **Live** (scratch
  daemons of the merged tree and of main, the stand-in model and Jev, every pack as wired; $0 spent): 180 s of
  back-to-back turns in three sessions, the merged sink writing nothing for 120 s as designed, then draining in the
  gaps, done 2.0 s after the last turn, with the same rows per Jev call as main's sink (1.953 against 1.952: nothing
  lost or doubled); a shutdown with 2,827 queued wrote all 2,827 (the phases in order: posts settled 8.4 ms, judgments
  written 11,461.2, last checkpoint 11,461.3), a SIGTERM with 307 more wrote them in 198 ms (3,134 after the restart);
  a SIGKILL after 40 s of turns lost the judgments of 1,023 Jev calls, the documented loss, and no spend ($0.035367
  before the kill, $0.040073 after one new turn once the crashed process's blocks were booked). Every clean-stop path
  ends in `finish_stop`, in the order the brief asked, and the locks do not invert (read in the code; SIGINT, a restart
  and a `--stdio` stop not run live).
- **FAST.** The judged turn against off (`bench turn --judge --runs 30`, release-thin, six runs in two holds, PSI beside
  each): on the quiet runs the judged p50s equal the off arm's (plain −4.6 to +2.7 ms, tool call −8.4 to +8.2); the
  six-run medians plain 76.0 off, 74.7 loop, 82.8 packs, tool call 165.6, 160.5, 165.9; the loaded runs drift 9 to 86 ms
  between arms and are not counted; no sink frame landed inside a measured turn in any run, and the judge frame before
  an answer, named by R34's probe, is categorize.v1's META mark slipping into the next turn under load. The stop's
  cost: `finish_stop` with nothing pending 0.19 to 0.26 ms; the lifecycle bench's post-in-flight stop (2 judgments
  queued), quiet, +10 ms p50 (merged 86.1 and 84.5 ms, main 76.6 and 73.8), one frame's sync on the owner's disk; the
  stop with executions waiting shows no added cost.

**What the review found** (R34; none blocks the join). **theseus-ehkp (P2, FAST):** the sink writes a frame's staged
blobs, two syncs each and one at a time, inside its between-turns guard, so a turn that begins mid-frame waits for them:
in the live burst, once the backlog drained after the 120 s quiet bound, the turn p50 doubled (269 to 280 ms against
142 to 156 on main's sink in the same windows, max 617 ms), and the shutdown with 2,827 queued took 11.5 s writing 690
blobs; spaced turns never meet it, a gapless stretch of 120 s or more does (a burst, or one long turn, since `Turns`
counts the whole daemon's turns). **theseus-xkbs (P2, FAST):** categorize.v1's META mark, and the budget's lone block
frame, are frames of their own written at prepare, outside the handshake, and under load land inside the next turn
before its answer. **theseus-ju99 (P3):** no test holds the busy-bound guard. R34's calls, adopted by the DM thread
(16:05): keep the 480 s guard; accept the SIGKILL loss for v1 (the rows are the judge's records, not the owner's data,
and the spend is safe), with the module doc naming what rides the lost frames; carry categorize's mark in the sink's
frame (and the block frame, keeping its order before the calls it books); join now and fix ehkp next. R28's call, kept:
289c's shadow until the read.

**The join** (batch 10; one merge of judge-sink 8a113f15, which holds judge-turn-cost 54de79da whole). The joiner's dry
run before the lock (16:32, on origin/main 5ac5c23c): merge-tree's two conflicts, theseus-core's AGENTS.md (the judge
paragraph) and `tests_route.rs` (the rig), both resolved by R34's `resolve.py` keep-both (the sink's sentences,
judge-sink's two, the bench line, then judge-reads' paging; route-tests' `parts`, the build, then the ladder's warm read
in `rig_built`, as R28 asked), 0 markers; **each of the branch's 31 files equal to R34's review commit's version with
main's own changes since f589d9cb applied** (in AGENTS.md the names lane's four hunks and imported-skip's moved
sentence). The first take (16:34:06) found `bench-reports-join` open and queued behind it, touching nothing. The second
(16:36:41 to 16:36:47), with main = origin/main = 351680a1: the dry run again (`MANIFEST_FORMAT` main 23, branch 22,
merged 23; 12 files changed on both sides since the base 4a449460, 10 auto-merged; the resolved tree 25cd8565); at the
merge git's rerere replayed R34's recorded resolutions of both files, and `resolve.py` found both already resolved;
staged 31 files, +1,883 −115, **the staged tree exactly the dry run's resolved tree**; the scrub's one hit a test's
loopback address, known. The warm (16:37:13 to 16:45:31, the test build 7 m 25 s at load 17 to 37); the targeted
suites, **316 of 316** in 59.1 s (theseus-judge 136; theseus-core 135 with the four new modules; theseusd's stops,
outbox and judge binaries 10; theseus-protocol 31; theseus-sim's judge tests 4). The signed merge **938a8dc8**
(351680a1 and 8a113f15), 16:47:34. Its gate (16:48:15, minute 48, at load 37, to 16:56:53, ok in 518 s, 69 s of it
waiting for the lock): **3,023 of 3,023** (imported-skip's 3,016 and the branch's 7; 1 slow, 24 skipped) in 296.3 s,
no hour crossed, no known load flake red; lifecycle ok (cold start p50 22.1 / p95 24.9 ms; from the config copy 22.6 /
28.4; clean shutdown with executions waiting 33.0 / 45.2; **a reply's post in flight 82.5 / 87.7**, the stop's flush,
about 5 to 8 ms over the last gates' 74 to 77 at the lowest load; SIGKILL then restart 25.8 / 28.8; binary swap 49.3 /
56.2; restore 237.2 / 255.7; a cancel's round trip 95.6 / 108.4 against 250); L1 start 5.98 / 6.43 ms; turn frames 5
and 9, plain p50 78.9 ms and tool call 153.9 (back from imported-skip's gate's 183.7, so that reading was noise). The
turn bench now says it runs with "Discord, the web UI, and the judge off". Pushed 16:57:08; both branches deleted on
origin, each once an ancestor of main; done line 16:57:29; theseus-0j2.8, 289c, s1am, ych4 and 3bl9 closed; R34's tree
kept for its next review. The store stays at format 23.

**The install** (installed 2026-10-06 17:22 at 938a8dc8, install #8, this merge's own commit). On the owner's daemon,
which has the judge on: the judge's frames are written only between turns, with one clock a backlog pass and the
480 s busy-bound guard; a clean stop writes every settled judgment before its last checkpoint (the stop phase
`judgments written`, with an info line when the flush wrote any); a SIGKILL loses the queue, never the spend; right
after a start, health shows the judged packs as `shadow (until the ladder is read; …)` for a few ms. No config key.
The old daemon's stop at this install was the old build's, so it shows no flush (its phases are logged at debug, which
the unit does not show). Health after the restart (17:22:53; the build 17:07:24 to 17:22:23, release-thin 13 m 54 s at
load 35 to 48; the install 17:22:48 to 17:23:01): `theseusd check` exit 0, startup serving at 53.6 ms (store 21.4,
kernel 23.9 ms, at load 38 to 48 with CPU pressure at 63 %), 9 secrets ready 1.33 s after the start, Discord ready,
the judge's live packs `security.v3`, `route.v1` and `rerank.v1` already live at the first reading, 15 s after the
start (the shadow-until-read window, a few ms long, over), memory live on the `baseline` arm, voice ready with `0
resumed`, `cgroup: delegated`, the unit active with NRestarts 0, and no error or warning in the journal; no config
change, the old daemon's stop 298 ms, the S3 backup caught up at position 5,759 from 17:23:06 (12 s), and the store at
format 23.

**Divergences.** The busy bound is in effect 480 s after the quiet bound, not 600 s from the backlog's start: the
session's choice, kept, so a never-empty backlog does not write every frame beside turns. "0 judge frames before the
answer" holds for the sink, not for every judge frame: categorize's mark and the budget's block frame are written at
prepare (theseus-xkbs). Deferring the writes gained nothing in a gapless burst (the turn wall rose from about 72 to 135
ms in the first 120 s on both sinks), and the drain then cost a frame's blob syncs a turn (theseus-ehkp). If the warm
read cannot read the store, the packs stay in shadow until an RPC or the nightly check reads it (a retry would be
cheap; not filed).

**Known gaps.** theseus-ehkp and xkbs (P2, FAST), the DM thread's next small FAST task: write the staged blobs before
taking the guard, batch the stop's syncs, and carry categorize's mark (and the block frame) in the sink's frame;
theseus-ju99 (P3, the guard's test); theseus-jtrc (P2, a load flake outside the branch). Owed by the review and not
written at the join: theseusd's AGENTS.md "Every clean stop is one path" (the judge's flush between the posts and the
last checkpoint), and the sink's module doc naming what a SIGKILL loses beside the rows (staged blobs, sentences,
metrics, the breaker's moves, rerank's ladder event).

### Item 222. Learned shadow: a promotion names a learned version standing in the moved version's place, with its rollback; the prove's window names one standing from before the window; and the learning cut's mark is never ahead of the judgments it cuts (theseus-nwa5, theseus-clbx and theseus-gf8j; R22's two findings on learning-fixes (Item 203) and R28's on judge-reads (Item 209); the tenth cloud batch's learned-shadow session, launched by the DM thread at 12:37 and fired 2026-10-06 12:43 from d279767f, Opus 5.5, its report at 13:59; 33ae7147, 22939914 and cf55f0b8; reviewed 16:06 to 17:37 by local reviewer R34b, on main plus judge-sink, and accepted at 17:42 with no join fix; joined 17:59 at 57f265f2, a signed merge onto 938a8dc8, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.** Three gaps in the judge's learned versions, two of them found live:
- **theseus-nwa5** (P2; R22, reviewing learning-fixes, Item 203, on a scratch daemon): `JudgeService::placed(root,
  session)` gives the newest learned version the ladder placed, and checks the session's arm only for a learned
  *canary*. A learned version in **shadow** is given for every session, and its mode is shadow, so nothing acts: while
  a learned version of loop.v1 stands in shadow, loop.v1's own canary acts in no session, whatever the ladder says of
  loop.v1. Nothing told the owner this at the promote.
- **theseus-clbx** (P3; R22, the same live stage): `judge.prove`'s window line (`learned_placed`) named a learned
  version only when its placement row fell inside the window, so one placed before the window and still standing in
  it went unnamed, though every task it judged in the window was left out as `learned_version`.
- **theseus-gf8j** (P3; R28, reviewing judge-reads, Item 209): cf5c's cut closes a judgment when its row is at or
  before the last run's `through` and its time plus its window plus the 1 h margin is before that run's clock. A run
  whose clock read ahead closed windows still open in real time: R28's probe, a run at now + 5 h, then "go on" a
  minute after the turn, then a run at now + 20 min that read 0 sessions, closed 1 and wrote 0 labels.

**What landed** (theseus-core only: `rpc/packs_ahead.rs` (new, 136 lines), `rpc/packs.rs`, `rpc/judge_prove.rs`,
`learning/system.rs`, `rpc/learning.rs`, `rpc/mod.rs`, the tests in `tests_ladder.rs`, `tests_prove.rs` and
`tests_learning.rs`, and the crate's AGENTS.md; the merge 10 files, +464 −25, without the cloud files; no package,
protocol type, config key or store format change (23 stays 23)).
- **The promote's warning** (33ae7147, nwa5). `Core::ahead_of` walks a root's learned versions as `placed` does
  (newest first; a canary goes on to the next, a shadow or live version ends the walk), reading `names_of_root`, each
  version's `rows_of` and the ladder's `standing`, without calling `placed`, which is unchanged; `Core::ahead_words`
  makes the sentence, and `promote_with` appends it to the answer's `said` on both paths. The written path says
  "judges": "loop.v101 stands in loop.v1's place in shadow, so loop.v1's canary 0.5 judges in no session until
  loop.v101 moves (`theseus packs rollback loop.v101`)"; a learned canary gives "… as canary 0.3, so loop.v1's canary
  0.5 judges only in loop.v101's control arm …", a live one "… loop.v1 live judges in no session …", and several
  placed versions are each named, with one rollback each. The security card's path says "would judge", since nothing
  is written until the card is approved (`gate.rs`'s `at_gate` and `notices_live`, and `notice.rs`, all read
  `placed(SECURITY_CANDIDATE, …)`, so a learned security version in shadow silences the root's promotion the same
  way). The warning never refuses. Every move goes through `promote_with` (the owner's `pack_promote`,
  `promote_automatic`, `promote_learned`); a learned version's own move names only the newer learned versions ahead
  of it, the same hazard one rung down; with the judge off, `placed` answers the root and there is no sentence.
  `tests_prove::learned_loop` became `pub(crate)` for the ladder test, and the AGENTS.md ladder paragraph gained one
  sentence.
- **The window's earlier placement** (22939914, clbx). `learned_before(rows, learned, since)` in `rpc/judge_prove.rs`
  takes each learned version's latest non-declined row at or before the window's start, newest first; when that row
  put the version in shadow, a canary or live, the line gains "; loop.v101 stood in loop.v1's place from before it
  (canary 0.5 since 2026-10-06)", in `mode_words`'s words, before the existing "inside" clause when both hold. With no
  `since`, nothing is added. The window test's last assertion, which held the old line, now expects the clause (the
  commit body says so), and the test gained a rollback before a later window (not named) and shadow before plus live
  inside (both clauses).
- **The cut's mark** (cf55f0b8, gf8j). The META mark `learning.last_run` gains `cut_ms` (`learning::system::CUT_KEY`):
  `min(the run's clock, the newest judgment row's time among the scopes the run read)`. `Cut::of(mark, now)` reads
  `cut_ms`, not `at_unix_ms`, clamped to the next run's own `now` (a cut later than it reads as `now`). `at_unix_ms`
  stays the run's own clock, so the tender's `due` and the prove's `settled_ms` are unchanged. A mark without `cut_ms`
  (written before this change) reads as **no cut, once**: that run walks everything, as a first run does, and writes
  the key. The newest judgment row's time is real-time evidence, a row the daemon wrote before the run read it, so the
  cut is never ahead of real time; the brief's "plus its longest window" could run up to that window less the margin
  ahead (23 h for classify), and with the newest judgment's own window it cannot pass both the probe and
  `a_second_run_reads_only_what_can_still_change`'s third step, which have the same shape and want opposite answers.
  The cost: judgments within their window and the margin of the newest stay open, and are re-read, until a newer one
  is written (minutes on a live daemon; at most a day's few on a quiet one). `learning/system.rs`'s module doc and
  the AGENTS.md learning-ledger paragraph describe `cut_ms`. The one new stored thing is that key, inside an untyped
  META value.

**How it is proven.**
- **The session's tests:** `tests_ladder::a_promotion_names_the_learned_version_standing_in_its_place` (loop.v101
  written as `learned_loop` writes it; loop.v1 promoted to canary 0.5 with loop.v101 at no row, a declined row only,
  shadow, canary 0.3, live, rolled back, and on loop.v101's own move, each `said` exact);
  `tests_prove::the_window_names_a_learned_version_placed_inside_it`, extended; and in `tests_learning.rs`
  `a_run_whose_clock_read_ahead_closes_no_open_window` (the probe, run twice with the next run at now + 20 min and at
  now + 1 day: each reads the session, closes 0 and writes the continuation label; the mark's `at_unix_ms` now + 5 h,
  its `cut_ms` now) and `a_mark_without_its_cut_walks_everything_once` (an old-layout mark that would have closed a
  3-day-old judgment: the first run closes 0 and reads the session, the next closes 1). `a_second_run…` stays green
  with every count unchanged (201 sessions, then 1 and 200 closed with one label, then 0 and 0), but only with one
  added row, a witness judgment at first_at + 1 day − 1 min with no session, whose time the second run's mark cuts at.
  The five tests three times each under the load recipe: 15 of 15 (2 to 13 s each). The core suite at the session's
  third gate: theseus-core 1,390 passed, one known load flake (theseus-1g8j, 3 of 3 alone); the families tests_judge
  17, tests_ladder 14, tests_learning 11, tests_prove 10, tests_notices 9, tests_learn_loop 9, judge::ladder 6. Its
  three planted reverts failed their tests: the check removed (at the shadow assertion), the old window filter (left
  the window alone), and the mark from the clock alone (first at its `cut_ms` assertion; with that skipped, at the
  behaviour: (closed, sessions) (1, 0) against (0, 1); the old-mark test (2, 0) against (1, 0)).
- **The review** (R34b, R34's tree; review commits e509ddb1 on cacad5c4 with judge-sink merged again, then b177d98a
  on 938a8dc8): fmt, the test build, the bins, clippy `-D warnings`, theseus-protocol's 31 and shape clean; **the
  whole workspace suite 3,021 of 3,023** at load 25 to 33 (the two red theseus-0bq1 and theseus-y0lm, known load
  flakes, each 3 of 3 alone); learned-shadow's and imported-skip's families on 938a8dc8 **67 of 67**, with
  imported-skip's join-fix test beside the three gf8j tests. **Eight planted reverts, four caught**: the report's
  three and R34b's old mark read as today; the four missed (the clamp removed; a learned canary ending the walk,
  which says "only in loop.v102's control arm" where loop.v1's canary judges in no session; the security card's
  answer without the sentence; a declined row before the window counted as a placement) are filed as theseus-04zb.
- **The probes.** (1) `ahead_right_after_a_start`: loop.v101 in shadow, a restart with no warm read (the ladder and
  the lineage unloaded, `placed(loop.v1)` = loop.v1): the promote (8.5 ms; 15.4 ms on 938a8dc8) said the sentence,
  since the promote's own path (`ask_of`, then `promotion_row`'s `standing`) loads the lineage and the ladder before
  `ahead_of` runs; so `ahead_of` needs no explicit `read_ladder` under judge-turn-cost (no join fix). (3)
  `a_judgment_queued_across_a_run`, on 938a8dc8, against judge-sink's deferred writes (Item 221): a
  judgment settled while a turn ran, held in the sink's queue; a run at now + 5 h read meanwhile (its `cut_ms` the
  witness's time, `through` 41); the row was written 547 ms after that run's real time, at position 44; the next run
  read the session and wrote the continuation label. A row's time is its write time and a row written after a run lies
  past its `through`, so the bound holds, more literally under the sink. (4) The first run after the upgrade, on
  1,001 judged sessions ten days old, one task judged ten minutes ago, 21,800 imported one-message episodes and a mark
  as the old build wrote it, debug: **544 ms** for the walk (the next run 372 ms) on 938a8dc8's walk by key
  (Item 219), against 1,613 and 1,223 ms on main's old decode-then-skip walk.
- **Live** (a scratch daemon of e509ddb1's debug build: the stand-in model and the stand-in Jev, every judge pack off
  but loop.v1, loop.v101 and security.v101 placed in shadow by R22's stager adapted): the promote names loop.v101 and
  its rollback; four tasks, then the prove: all 12 `judge.call` rows loop.v101's, the window "…; loop.v101 stood in
  loop.v1's place from before it (shadow since 2026-10-06)", `learned_version` 4 left out; after the rollback no
  sentence, and four more tasks count canary 1 and control 3; loop.v101 as canary 0.3, "only in loop.v101's control
  arm"; `packs promote security.v3 --live`, the card's answer ends "… would judge in no session …"; loop.v101 live, a
  restart, and the promote 0.17 s after the start names it.
- **FAST.** No code on the turn's, the start's or the stop's path: three RPCs (`pack.promote`, `judge.prove`, the
  learning run's read and mark) and `Cut::of`; the run gains one fold over judgments it already reads. R34b's
  palindrome A/B (`bench lifecycle --runs 10 --check`, debug, stack, main, main, stack in one hold, 17:30 to 17:34, CPU
  pressure 49 to 67): every run missed budgets under load, main's as much as the stack's, and the stack's p50s were
  equal to or under main's in every phase; `bench turn --check` frames 5 and 9 on both arms.

**What the review found.** `promote_automatic` has no caller outside the tests, and the learning loop's one call
(`promote_learned`) keeps the answer's question and drops its `said`; nothing is lost, since the loop moves only its
newest candidate and nothing can stand ahead of the newest. theseus-7ezx (P3), from checking gf8j against the sink:
route.v1's `chosen` rule times two judgments by their rows' write times, which judge-turn-cost's between-turns sink
makes the frame's time; probe 2, two judgments more than 3 s apart held to one frame, read 1 ms apart (2,075 ms in
separate frames), so a label's note says "0 s after" and rows built in the same millisecond drop the label; judge-sink's
area, not this branch's. theseus-iz69 (P3): the security card's question, which `theseus confirm` lists and Discord's
card carries ("Promote security.v3 to live? It is short of the bar (…): it would be forced. …"), lacks the warning,
though a security promotion is decided on the card.

**The join** (batch 10's learned-shadow, alone; the long-lived batch-10 joiner's job after the judge pair). The queue
was clear at 17:42 (install #8's done line 17:35:48; main = origin/main = 938a8dc8). The dry run before the lock
(17:43:25, linuxbrew git's `merge-tree`, the CLOUD files dropped, R34b's `resolve.py`, a throwaway index, no rerere)
gave the one expected conflict and a resolved tree a1ac1c09, which is R34b's review commit b177d98a's tree; 10 of 10
branch files as expected. The guarded take (17:44:01, lock `cloud-learned-shadow-join`) found the queue clear and the
same trees; the merge: the conflict in theseus-core's AGENTS.md ladder paragraph (judge-turn-cost's rewrite against
the branch's one added sentence), "Resolved … using previous resolution" by rerere from R34b's review merge, and
`resolve.py` (which keeps judge-turn-cost's paragraph, then the branch's sentence, then the tests line) found it done;
no join fix; staged 10 files, +464 −25, the tree a1ac1c09; the scrub with the names family on, 0 hits. The warm
(17:44:21 to 17:47:05, the test build 2 m 02 s at load 6 to 19, clippy clean); 152 of 152 targeted in 38.8 s
(theseus-core 121, the learning, ladder, prove, tender, rpc and output families; theseus-protocol 31, protocol.gen
unchanged). The signed merge **57f265f2** (938a8dc8 and 047c8959), 17:49:06. Its gate (17:49:24, minute 49, to
17:58:46, ok; 181 s waiting for the gate lock behind one review step): **3,026 of 3,026** (1 slow, 24 skipped; the
judge pair's 3,023 plus the branch's 3) in 297.5 s, no hour crossed, none of the known load flakes red; lifecycle ok
(cold start p50 24.2 / p95 32.5 ms; from the config copy 23.8 / 25.8; clean shutdown 41.5 / 59.8; a post in flight
82.6 / 90.0; SIGKILL then restart 28.2 / 34.8; binary swap 48.7 / 164.1, one slow run inside its 202 ms budget, on no
path of this branch's; restore 233.9 / 316.5; a cancel's round trip 102.2 / 111.0 against 250); L1 start 6.95 /
7.74 ms; turn frames 5 and 9, plain p50 77.1 ms, tool call 157.7 ms. Pushed 17:58:58, the branch deleted on origin,
done line 17:59:14; theseus-nwa5, clbx and gf8j closed with the hash. 15 min from the take to the done line. The
third join in a row where rerere replayed a reviewer's resolution; here the staged tree is both the dry run's, built
without rerere, and the review tree. The store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon a promotion names a
learned version standing in the moved version's place, and its rollback ("would judge" on a security card's answer),
and never refuses; `judge.prove`'s window line names a learned version standing from before the window; and the
learning cut's mark is never ahead of the judgments it cuts. The first learning run after the install (the nightly
one at 2026-10-07 03:00, after the installer had finished) reads the old mark as no cut, once: its log line
`learning: the rules read what can still change` shows `judgments_closed=0` with every judged session read, and later
runs close again (R34b measured about 0.5 s in a debug build on a store of the owner's shape). No config key. Health
after the restart (22:13:05): `theseusd check` exit 0, 9 secrets ready 1.12 s after the start, startup serving at
61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21, over the recipe's 60 ms), `cgroup: delegated`, the
judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready
(`0 resumed`, rejoins 0, `deaf_failed` false), Discord ready, the unit active with NRestarts 0, and no error or warning
in the journal; no config change, the store at format 23, sessions 21,785 with the lists in low milliseconds, and
durability caught up at position 143,096 from 22:13:18 (12 s). The old daemon's stop took 11.87 s (install #8's:
298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** gf8j's bound is `min(clock, newest judgment)`, not the brief's "plus its longest window", and
`a_second_run…` needed a witness row: no bound of the brief's kind can pass that test's third step and the probe
both (R34b: keep it, with the clamp and the old mark's one full walk; adopted by the DM thread at 17:42). The clamp is
"read as `now`", not "no cut". nwa5's warning also covers a learned version's own promotion (kept, as R34b
recommended). The warning reaches the answer's `said`, not the card's question. The other option the report set out,
`placed` keeping the root wherever it acts while the shadow version records beside it (a second `judge.call` row per
dispatch, its own Jev call and spend, roughly doubling the judge's cost at eight reading points, and the prove's arms
read again), was not built: R34b recommends deciding it with m5 §2.17's next revision.

**Known gaps.** theseus-04zb (P3: the four uncaught plants, each with the test it needs); theseus-7ezx (P3: carry
the judgment's own time on its row, for route.v1's `chosen` rule); theseus-iz69 (P3): the owner's answer of 23:18
(S5 in the evening's question walk) is to put the warning into the security card's question itself, so Discord, the
cockpit's Actions and the CLI all show it, with a test that the question carries it, in the next batch. The first
learning run after install #9 was not watched by the installer (it is the 03:00 nightly). Owed by the review and not
written at the join: the spec's learning section (the cut at `cut_ms`), m5 §2.17 (the warning and the option left
open), written in this version's Part I amendments.

### Item 223. Discord bound: a lane keeps the keys of the turns its renderer still holds and forgets a dropped turn's, so a background job's late result edits its one tool line and one notice card instead of posting them again; the renderer's maps hold only its held turns' keys; and the task board's exemption is held through the lane (theseus-6809, theseus-whb0 and theseus-8u7m; the findings on discord-live's lane bound (Item 207); the tenth cloud batch's discord-bound session, launched by the DM thread at 12:37 and fired 2026-10-06 12:43 from d279767f, Opus 5.5, its report at 14:24; 9a246a87, 01c5ef55 and 743e6cdb; reviewed 15:49 to 17:25 by local reviewer R37, the discord stack, with a join fix for the names rule, and accepted at 18:07; joined 18:25 at 4ef8f09b, a signed merge onto 57f265f2, the first of the stack's two merges under one lock and one gate, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.** Discord-live's bound (theseus-celu.37, Item 207) kept a lane's newest 256 keys of `msgs`, `sent` and
`sealed`, the task board's key exempt. Its argument, that a key is forgotten only after 192 others, missed a case:
the renderer's `update_tool` re-renders a line in any of its 8 held turns, so a background job's late result, past
256 later keys and past the nonce window, posted a **second** tool line (theseus-6809, P2), and the job's notice card,
whose create carries no nonce, posted twice. The renderer's own maps (`emitted`, `menus`, `notices`) were never
pruned at all (theseus-whb0, P3). And the bound's test inserted the board's id into `msgs` directly, so it passed with
the board's exemption removed (theseus-8u7m, P2).

**What landed** (`crates/theseus-discord` only; the merge 8 files, +426 −40, without the cloud files: `courier.rs`,
`courier/tests_bound.rs`, `render.rs`, `runtime.rs`, the crate's AGENTS.md, and new `courier/held.rs`,
`render/held.rs` and `runtime/tests_held.rs`; no package, protocol type, config key, ledger kind or store format
change (23 stays 23): nothing here is stored).
- **The held turns** (9a246a87, 6809). `LaneMsg::Held(Vec<String>)` carries the turns the place's renderer holds,
  oldest first. The place's actor sends it after rendering each `TurnStarted` (whose ops are only `Typing`, so it
  reaches the lane before any key of the new turn) and from `rebind`, after the renderer is replaced (an empty list:
  every held turn dropped, as at `/new`). In `courier/held.rs`, `touch` leaves a held turn's key out of `touched`, so it
  never counts against `KEYS_KEPT`; a turn that leaves the list is forgotten from `msgs`, `sent`, `sealed` and
  `touched`, at once, or, when a waiting live op still names its key, once `apply_live` has written that op or `away`
  dropped it (else a late state queued in the same batch as the next turn's start would post again). `Held` is not a
  live op, so `take` handles it while Discord is away. **A notice card's key** moves from `notice:<tool_use_id>` to
  `<turn_id>:notice:<tool_use_id>`, under the newest held turn that shows the call's line (`Renderer::notice_key`),
  so a card is kept while its line is and forgotten with it, under one rule: a held turn's keys all begin
  `<turn_id>:`. A card no held turn shows keeps the old key under recency and goes at the renderer's next drop. Every
  `Op::Notice` producer uses the new key (`ToolEnded`, `JudgeScored`, `PolicyNotified`, `PolicyTightened`); the card's
  line is in the turn before the card exists, since the core announces `policy.notified` after the call's
  `tool.proposed`. `Renderer::streamed`, which had no caller, is removed.
- **The renderer's maps** (01c5ef55, whb0). `drop_past_recent` (`render/held.rs`), from the `TurnStarted` arm, pops
  the turns past `RECENT_TURNS` with their keys from `emitted` and `menus`, and keeps in `notices` only the calls a
  held turn still shows, so `PolicyTightened` re-renders only cards whose ids the lane still keeps.
- **The board through the lane** (743e6cdb, 8u7m): one test, below.
- **The bound,** derived by the session and again by R37 from the code: per lane, the board's key, every key of the 8
  held turns, at most 256 others by recency, and a released turn's keys for the moment a waiting op names them. A
  turn names at most `max_loops` (40) × (its text parts + 1 tool line + its notice cards), plus a footer: 8 × (40 ×
  (P + 1 + C) + 1) + 256 + 1 = **905 keys** at one part a loop and no card; `sent` holds up to 2,000 characters a
  key, so about 1.8 MB a lane at worst. A long answer raises P, but the renderer already holds those turns' text. The
  crate's AGENTS.md bound line says this. render.rs ended 11 lines shorter than it began (2,990 on the branch; 2,998
  in the merged tree, under the 3,009 ceiling the names lane had raised from 3,001); runtime.rs 3,440 of 3,500.

**How it is proven.**
- **The session's tests:** the probe
  `courier::tests_bound::a_held_turns_tool_line_is_edited_after_later_turns_pass_the_bound` (the lane filled with 256
  `glide:` keys, a held job's tool line, 7 turns of 19 loops each applied with `Held` sent as the actor sends it, the
  nonce window closed, then the late result: one tool line, edited; the turn dropped, none of its keys left in any map);
  the actor test `runtime::tests_held::a_turns_start_and_a_rebind_tell_the_lane_the_held_turns` (turn n's start sends
  turns max(0, n − 7) to n, so the ninth start drops turn 0; `/new` sends `[]`); the renderer test
  `render::held::tests::a_renderers_maps_hold_only_its_held_turns_keys`, with notice embeds off and on, over 3 ×
  `RECENT_TURNS` turns (`emitted` only the kept turns' 16 keys, `notices` exactly `use_16` to `use_23`); and
  `courier::tests_bound::the_board_written_through_the_lane_is_edited_past_the_bound` (the board created and pinned
  through `apply_live`, 256 more keys, the nonce window closed, the board written again: one board, edited, pinned, one
  `PUT …/pins/`). Plants: the recency bound alone (two tool lines), the rebind's send removed (`left: [], right: [[]]`),
  all three `retain`s removed (48 keys), the `notices` retain alone, and the board's exemption removed (two boards; the
  old bound test passed under it). Each new test five times at nice 19 beside four busy loops: 20 of 20 (the probe 29.4
  to 30.0 s, the board 13.6 to 15.7 s: the fake's per-request answer over about 550 and 260 writes); theseus-discord
  whole under that load, 133 of 133. Each commit's gate was red only on the 33 known L1 cases (theseus-pv6i), one known
  load flake, and a first run of about 77 job tests that failed on a full disk (878 MB free, 16 GB in
  `target/debug/incremental`; deleted, rerun with `CARGO_INCREMENTAL=0`: 2,960 passed, the 33 L1 red).
- **The review** (R37, the tree cloud-b10discord; review commit 4bdd38c3 on ea34457e): fmt, the workspace test build
  (18 min 41 s cold), the bins, clippy `-D warnings`, theseus-protocol's 31 with protocol.gen unchanged, and shape
  clean; theseus-discord's and theseus-sim's **193 of 193**. **Six planted reverts, four caught** (the report's
  four); R37's two on the lane's deferred forget (a forget at once while a waiting op names the turn; `apply_live`'s
  closing `release()` removed) pass the crate's 141 tests: no test queues a turn's late state in the same batch as the
  `Held` that drops it (theseus-30kd).
- **Live, an A/B on the rig** (`theseus-sim discord rig`, the fake model, a scratch daemon of each build, `[tools]
  proc_sync_secs = 2`, notice embeds on, invented names). R37's first run, with the report's seven later turns of 40
  parts, showed no edit on either arm: a job's late result reaches its place through a wake, which is itself a turn,
  and the ninth `TurnStarted` drops the job's turn before its `ToolEnded` lands (main the same; theseus-9zif). With six
  turns of 46 parts (the wake the eighth): **the branch** edited the one line to "✅ `proc.run` sleep 45 · 🔔 notified
  (enforcement = notify) · 45001 ms", with one `discord.message.out` row for its key among 282 and no create answered
  `deduped` (the stack, with discord-tests on top, the same); **main** (ea34457e) wrote a second create for the line
  (the fake, whose nonce window never closes, answered it `deduped`: past Discord's window, a second line) and posted
  the notice card **twice**. VmRSS over 30 more long turns cannot show the maps' bound (both arms about 2 MB a turn,
  97 to 151 MB, from the session's own growth); the renderer test and its plant are that proof.
- **FAST.** Nothing on the start path or a turn's path: one more lane message per turn start and per `/new`, a prefix
  check against at most eight held turns per lane write, and one walk of the lane's four maps and the renderer's three
  per turn start. The stack's A/B is in Item 224.

**What the review found.** The two cases the report left open are not quite main's behaviour. A reply delivered after
its turn's drop (an outage across eight turn starts, or `/new` with a reply staged but undelivered) posts every part
fresh, replying to the anchor, where main still held the stream's ids and edited them; past Discord's nonce window the
text shows twice (theseus-llo6, P3). A late `ToolEnded` after the drop has no line to update, as on main, and now no
card either (theseus-9zif, P3, with the older half above). The renamed key is safe: nothing parses a row's `part` (the
cockpit prints it whole), the lane's maps are in memory, and the card's create carries no nonce. The join fix: the
branch's new actor test drove `/new` as a person's handle, which the names lane had made `"discord:zeroaltitude"`
everywhere else (`discord-bound/joinfix.py`, the scrub's one name hit).

**The join** (the discord stack's first merge, under the stack's one lock; the queue, the dry run, the warm, the tests
and the one gate are told in Item 224). The merge (18:12:03 onto learned-shadow's 57f265f2): no
conflict; the CLOUD files removed; `joinfix.py` "fixed" `runtime/tests_held.rs`; staged 8 files, +426 −40, the tree
4e6fd3f3, the dry run's; every file the branch changes equal to R37's review commit's with main's own changes since
applied; the scrub's names family 0. The signed merge **4ef8f09b** (57f265f2 and 40c0ce1c). theseus-6809, whb0 and
8u7m closed with 4ef8f09b. The store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's Discord places a background
job's late result edits its one tool line and its one notice card, where it posted them again once a turn's keys had
passed the bound; a lane's memory follows the renderer's 8 turns. The notice key is now `<turn>:notice:<id>`, and
nothing reads it by parsing. No config key. Health after the restart (22:13:05): `theseusd check` exit 0, 9 secrets
ready 1.12 s after the start, startup serving at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21,
over the recipe's 60 ms), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`,
`rerank.v1`), memory live on the `baseline` arm, voice ready (`0 resumed`, rejoins 0, `deaf_failed` false), Discord
ready, the unit active with NRestarts 0, and no error or warning in the journal; no config change, the store at format
23, sessions 21,785 with the lists in low milliseconds, and durability caught up at position 143,096 from 22:13:18
(12 s). The old daemon's stop took 11.87 s (install #8's: 298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** The notice card's key changed shape, to key the card under its line's turn. The deferred forget,
which the brief did not ask, guards a late state queued beside the next turn's start. The report's live recipe (seven
later turns) cannot show the fix; R37's six can. The task board's live check past 256 keys (the report's check 2) was
not run live; the new board test and its plant hold the exemption through the lane.

**Known gaps.** theseus-llo6 (P3: keep a released turn's keys while an open post of the lane names it) and
theseus-9zif (P3: keep a turn held while a call of it runs in the background or waits, capped; until then the wake
turn's reply carries the result), both for the discord lane later (the DM thread, 18:07); theseus-30kd (P3: the
deferred forget's test); the two new tests' cost through the fake's HTTP, noted on theseus-z9nq. Owed at the join:
the status page's Discord memory line, if it names the bound.

### Item 224. Discord tests: the gateway loop and the bindings watch end at the daemon's stop, a refusal of unbound posts runs under the lanes' lock, and three outbox tests wait for what they assert (theseus-yduk, theseus-9ggu, theseus-o2tm, theseus-sj0t and theseus-0bq1; the findings on discord-live (Item 207) and three gate flakes; the tenth cloud batch's discord-tests session, launched by the DM thread at 12:37 and fired 2026-10-06 at about 12:43 from d279767f, Sonnet 5.5, its report at 13:54; 3e48f514, 4e31f233, d04d17e5 and d2e542f7; reviewed 16:40 to 17:40 by local reviewer R37, the discord stack, on discord-bound's review merge, and accepted at 18:07 with no join fix; joined 18:25 at e8120522, a signed merge onto 4ef8f09b, the second of the stack's two merges under one lock and one gate, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.**
- **theseus-9ggu** (P3): a binding task mid-poll when `shutdown_timeout` gave up kept the core, and with it the store,
  open after the runtime was "shut down": theseus-discord's two two-life tests failed under load in the cloud with
  "Database already open".
- **theseus-yduk** (P3): with the bindings file read live (theseus-ocwt, Item 207), `Shared::refuse_unbound` copied the
  lanes' keys under the lanes' lock, released it, then refused every open post for a place not in the copy, so a post a
  place's new lane was sending, after the place was removed and put back, could be settled refused.
- **Three load flakes in `tests_outbox`**: o2tm (the refused-pin test asserted the pin was asked for after a fixed
  300 ms sleep), sj0t (the card-settle test took the first DM message naming `fs.write` as the card, and the tool line
  names it too), and 0bq1 (the same test saw 4 pending posts, not 3, in a join gate of 2026-10-04 at load 30).

**What landed** (the merge 3 files, +199 −14, without the cloud files: theseus-core's `outbox.rs`, theseus-discord's
`runtime.rs` and `tests_outbox.rs`; no package, protocol type, config key or store format change (23 stays 23)).
- **The stop** (4e31f233, 9ggu). `Outbox::stopped()` (core `outbox.rs`, an `async fn` beside `stopping`): a
  `wait_for` on the `flight` watch, no polling, ready at once if the stop already began. The gateway's `event_loop`
  selects the shard's next event against it and returns ("discord gateway loop ended at the daemon's stop", the board
  "disconnected"), and the bindings watch's spawn in `serve` selects against it too (it never ended before, and held
  `Arc<Shared>`). The select adds no wait to a stop: it is woken by the stop the daemon already raises.
- **The refusal under the lock** (d04d17e5, yduk). `Shared::refuse_unbound` holds the lanes' lock across
  `Outbox::refuse_unbound`. Nothing under it takes the binding's locks or calls back; the order is `lanes` before
  `retired` everywhere, the outbox's and the kernel's locks only inside `lanes`, and no caller holds `lanes` at the call
  (`retires` calls `lane_retires`, which takes and drops it, first). With every place bound it holds the lock across one
  in-memory read; only after a removal, with posts open for it, across store reads and a synced settle per post.
- **The tests** (3e48f514 and d2e542f7). o2tm's waits for the refused `PUT` (`until`, 10 s) instead of sleeping; sj0t's
  card is the message whose id the card post's settle recorded (`card_message_id`, from `outbox_actions()` by
  `kind_of == "card"`), so its words are still checked on the card itself; 0bq1's waits for the bind notice's settle
  (`pending == 0`) before the fake goes down, keeping the count at 3, and the count's message now names each pending
  post's kind and target (`pending_kinds`), so a recurrence says which post it is. 9ggu's test
  `the_gateway_loop_ends_at_the_daemons_stop_and_leaves_no_core_behind` (a core and a binding, `run` held, then
  `core.stopping_on("SIGTERM")`: `run` returns within 10 s, and after `shutdown_timeout(30 s)` the core's `Weak` is
  gone); yduk's `a_place_removed_live_and_put_back_has_its_new_post_sent_not_refused` (a channel removed and re-added
  live, health awaited each time: the new post settles sent, nothing refused).

**How it is proven.**
- **The session** could not reproduce the flakes (the old binary in the brief's shape passed 12 of 12 batches, and the
  card-settle test alone 60 runs under load), so the three fixes were argued from the code and proved by planted
  reverts: the board's pin call removed ("timed out: the pin was asked for and refused"), the card's settle edit
  skipped (fails at the card's words, the card found by its id), and `Outbox::closed` writing the settle twice ("left:
  4 right: 3", `[card, reply, settle, settle]` listed); the 9ggu select made `pending` (the binding's run never ends).
  9ggu's first test used `shutdown_timeout(5 s)` and failed once in 30 starved batches with "core outlived its runtime"
  and a twilight rate-limiter actor panic: the lanes, the courier and the route do not end at the stop, so a starved
  runtime needs longer to wind down; at 30 s, 30 batches under load passed. theseus-discord whole, 131 of 131; each
  commit's gate red only on the known L1 cases.
- **The review** (R37, review commit aed9680f on 4bdd38c3): the build clean (the test build 1 min 52 s, incremental);
  **the whole workspace suite on the stack, 3,018 of 3,018** (24 skipped, 1 slow by design), beside two trees
  building. **The flakes, reproduced on main first** (`race.sh`: the crate's test binary, tests_outbox, tests_live and
  tests_gateway in one process at nice 19, arm A main ea34457e, arm B the stack, each frozen in /tmp): in the gates'
  shape (load 33 to 39, CPU pressure 40 to 69 %) six batches of main failed three times: sj0t exactly (the tool line
  "⏸️ `fs.write` a.txt · waiting for approval" taken for the card), the refused-pin test at its 10 s board wait (not
  o2tm's sleep), and a test outside the brief (theseus-pb3l: a shared channel's card test reads the
  `discord.message.out` rows before the lane's off-path write lands); with a busy loop per core (load 37 to 59, CPU
  pressure 68 to 84 %) main failed sj0t's way again and a coalesce test's edit bound. **The stack never failed that
  way** in 10 loaded batches; its one red was tests_gateway's Jev notice 20 s wait, judge-sink's file, seen on main
  before (theseus-3ae1, with the board wait and the coalesce bound). 0bq1 did not show in 16 loaded batches of main, so
  its fix stays argued. **Five plants, three caught** (the report's); R37's two, the bindings watch's select removed
  and main's copy-then-refuse, pass: the 9ggu test's 30 s shutdown drops a watch that never ended anyway (noted on
  theseus-2i39), and yduk's race cannot be ordered by a test.
- **Live** (R37's rig, a scratch daemon of the stack's debug build). The gateway killed, then SIGTERM: the stack's
  daemon exits in **0.10 s**, logging "discord state=disconnected detail=the daemon is stopping" and "discord gateway
  loop ended at the daemon's stop"; main's exits in 0.15 s with neither line (its runtime's drop cancels the loop
  mid-connect). A place removed and put back while its reply's create hangs at the fake (`hang-creates`; health dropped
  it in 1.0 s and listed it again in 1.8 s), then the fake up: the reply on Discord once (the lane's retry carried the
  same nonce), the outbox `0 pending · 3 sent · 0 refused`, and no "not bound here any more" row.
- **The 9ggu mechanism, read in the code.** A lane's Discord call goes through twilight-http's rate limiter (0.17.1),
  whose actor is a spawned task; a request awaits a permit from it and completes the permit after the response, and
  both panic when the actor is gone. When the runtime winds down, tokio (1.53) cancels tasks per worker as each notices
  the shutdown, so the actor can be dropped first and a lane's task polled once more, and panic. A daemon's stop drops
  the runtime with no timeout, but the order is the same, and every release profile has `panic = "abort"`: such a
  panic would abort the process (SIGABRT) after the core's clean stop wrote its records and before the store's last
  close, and the next start would recover it as after a crash. R37's ten SIGTERMs into a 46-part stream (a debug build,
  so a panic would print) printed none, each stop 0.05 to 0.2 s: a narrow race.
- **FAST** (the stack against main, R37's `bench.sh`, debug builds frozen in /tmp, one hold, 17:33 to 17:38,
  lifecycle A B B A A B): the stop rows 9ggu changes, medians of three runs an arm, B − A: a clean shutdown with a job
  running −2.1 ms (45.8 to 43.7), with a reply's post in flight +0.6 ms (85.7 to 86.3), SIGKILL then restart −6.0 ms
  (47.2 to 41.2); every B run met the stop rows' budgets; both arms missed only the debug builds' cold-start budgets.
  `bench turn --check` frames 5 and 9 on both, plain p50 117.5 ms (B) against 119.2 (A). **No cost.**

**What the review found.** theseus-2i39 (P3): 9ggu's change covers the gateway loop and the bindings watch; the lanes,
the courier, the route and each place's actor still run until the runtime drops them, so a lane's request in flight at
the drop can meet the panic above. R37 recommends ending the courier and the route at the stop, and an actor and its
lane between writes, as a retired lane already ends; it adds nothing to the stop's path (adopted by the DM thread at
18:07). theseus-pb3l and theseus-3ae1 (P3): main's starved waits found in the repro, none this branch's.

**The join** (the discord stack: discord-bound, Item 223, then this branch; one lock, one gate).
The two branches are siblings, both cut from d279767f. The queue was clear at 18:07 (learned-shadow's done line
17:59:14; main = origin/main = 57f265f2). The joiner's dry run before the lock (18:08:43) and inside the take
(18:11:57), with linuxbrew git's `merge-tree`: discord-bound clean, the CLOUD files dropped and R37's `joinfix.py`
applied, giving R1 (4e6fd3f3); then discord-tests onto R1 committed as a dangling commit, clean, merge base d279767f,
giving R2 (d3d64b1d); no marker; format 23 on main, R1 and R2; the scrub's names family 0 on both merges' added lines;
both branches' files equal to R37's review commits' with main's own changes since ea34457e applied (10 of 10, core
`outbox.rs` carrying judge-sink's two hunks). The guarded take (18:11:56, lock `cloud-b10-discord-join`): merge 1
staged R1 and was committed as 4ef8f09b at 18:12:03; merge 2, no conflict, staged 3 files, +199 −14, the tree R2,
committed as **e8120522** at 18:12:05 (4ef8f09b and 1778badc), both signed. The warm (18:12:21 to 18:14:06, the test
build 1 m 15 s at load 9, clippy clean); the stack's suites, **233 of 233** in 32.5 s (theseus-discord 143, the four
fixed tests among them; theseus-sim 54; core's outbox and `tests_output` 5; theseus-protocol 31, protocol.gen
unchanged). The one gate, on e8120522 (18:15:25, minute 15, to 18:24:02, ok; 35 s waiting for the gate lock behind
one review step): **3,032 of 3,032** (1 slow, 24 skipped; learned-shadow's 3,026 plus the stack's 6) in 311.0 s, no
hour crossed, sj0t's, 0bq1's, o2tm's and pb3l's tests green and no known flake red; **lifecycle missed, then ok on
gate.sh's own second run**: run 1's one miss was a single start of ten from the config copy at 82.1 ms (p95, against
57), a row the stack does not touch; run 2 met every budget (cold start p50 30.7 / p95 36.1 ms; from the config copy
27.2 / 29.1; clean shutdown 36.5 / 49.7; a post in flight 91.6 / 101.1; SIGKILL then restart 34.2 / 38.5; binary swap
56.2 / 101.3; restore 283.0 / 322.9; a cancel's round trip 109.4 / 123.1 against 250); L1 start 9.93 / 13.87 ms;
turn frames 5 and 9, plain p50 107.9 ms, tool call 218.2 ms. Every row read 15 to 30 % slower than at the last two
gates, rows the stack cannot touch as much as the rest (L1 start, restore, the turn bench with Discord off), with IO
pressure at 6.5 % some and 5.8 % full and this disk's fdatasync at 7.5 ms: the machine, not the stack (R37's same-hold
A/B above); the DM thread filed theseus-4284 (P3, FAST: an A/B of 57f265f2 against e8120522 on a quiet machine) at
18:30. Pushed 18:25:01 (both merges), both branches deleted on origin, done line 18:25:19; theseus-yduk, 9ggu, o2tm,
sj0t and 0bq1 closed with e8120522; R37's tree and its 25 GB target removed. 13 min from the take to the done line.
The store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon the gateway loop and the
bindings watch end at the daemon's stop (on R37's rig, a 0.10 s exit with the gateway down), and a refusal of unbound
posts runs under the lanes' lock, so a place removed and put back live keeps its new post; the outbox tests' changes
are tests only. theseus-2i39 stands: a lane's request polled while the runtime winds down can panic in twilight's rate
limiter, and the release profile aborts on a panic; narrow, and not seen in R37's ten live stops. No config key.
Health after the restart (22:13:05): `theseusd check` exit 0, 9 secrets ready 1.12 s after the start, startup serving
at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21, over the recipe's 60 ms), `cgroup: delegated`,
the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice
ready (`0 resumed`, rejoins 0, `deaf_failed` false), Discord ready, the unit active with NRestarts 0, and no error or
warning in the journal; no config change, the store at format 23, sessions 21,785 with the lists in low milliseconds,
and durability caught up at position 143,096 from 22:13:18 (12 s). The old daemon's stop took 11.87 s (install #8's:
298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** 0bq1's fix is inferred from the code, not observed (the count's new message names the fourth post if
it comes back). 9ggu's test gives the runtime 30 s, which a starved runtime needs. Ending the lanes, the courier, the
route and the actors at the stop was outside the brief, and is theseus-2i39.

**Known gaps.** theseus-2i39 (P3, with the bindings watch's untested select noted on it); theseus-pb3l and
theseus-3ae1 (P3, main's starved waits); theseus-4284 (P3, FAST: the slow gate's A/B). Owed by the reviews and not
written at the join: theseus-discord's AGENTS.md lines for 9ggu (`Outbox::stopped`) and yduk (the refusal under the
lanes' lock), drafted in this version's code-side edits.

