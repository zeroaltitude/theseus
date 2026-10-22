<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

## Read this first: the state of your base (it wins over anything below)

Your branch starts from the integration branch `integ/tail` at `7f428787` (2026-10-21: `main` `6918e3c1` plus the
patch1 tail rows tail-hold, tail-mail, job-wait-status, tail-discord, tail-onto, tail-flakes, tail-tooling,
tail-herdr-cancel, tail-effort-400, tail2-mailauth, tail2-google). That branch, not `main`, is your base, and your
branch joins back into it. Line numbers below are at `7f428787`; search by name if one has moved.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row changes
no stored record. If you find you must, bump to 49 and add the old layout's literal sample to
`crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says; never write the number as a literal anywhere but the
constant and its tests.

**Line ceilings** (`scripts/long-files.txt`): none of your files is listed. Each must stay at or under 2,500 lines
(`theseus-discord/src/runtime/voice.rs` is 1,888, `theseus-voice/src/engine.rs` 1,776, `songbird_io.rs` 1,052,
`theseus-core/src/voice.rs` 510, the template 1,756). Put new code in a new file if a module would grow past that.
Never raise a ceiling.

**Work in flight beside you:** no other row is coding in `crates/theseus-voice/`, `crates/theseus-core/src/voice.rs`
or `crates/theseus-discord/src/runtime/voice*` tonight. Rows coding elsewhere join `integ/threads` and `integ/cn`;
stay out of `turn.rs`, `turn/*`, the China-origin data rule files, `thread_*` and the TUI.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: voice-knobs, a natural pause before a turn ends and a whole first word, as `[voice]` keys (theseus-bp0lp)

Branch: `cloud/20261021-voice-knobs`. Every commit's subject carries `theseus-bp0lp`. Deadline for the report: 4 hours
after you start.

**The owner approved the defaults (2026-10-21 23:36):** `pause_ms` 1200, `lead_in_ms` 400, a shorter fade, and the
unfinished-utterance merge. They are fixed here; do not pick others.

**Why.** The owner's live voice re-test on the installed daemon (2026-10-21), verbatim: "Much much better on voice!
The first "go ahead" sounds like "gaaah", so at the beginning there's a little clipping or jitter. Sometimes, it
doesn't wait as long during a pause as would be natural, so it ends up chopping up sentences. Are those tunable
theseus knobs, tunable deepgram knobs?"

Both are Theseus constants today: no config key reaches them, and neither is a Deepgram setting (Theseus uses no
Deepgram endpointing; its own voice-activity detector decides where an utterance ends).
1. **Sentences are chopped.** `Config::new` in `crates/theseus-voice/src/engine.rs` (~:117) sets
   `end_of_utterance: Duration::from_millis(700)`; `Config::vad` (~:136) turns it into `VadSettings::quiet_frames`
   (35 frames of 20 ms), and `vad.rs` (~:110) closes an utterance on that many quiet frames. A natural mid-sentence
   pause runs 0.7 to 1.2 s, so 700 ms closes the utterance mid-thought and the rest becomes a second turn.
2. **The first word is clipped.** Two candidates; find which, and say so in the report:
   - **The head.** `songbird_io.rs` (~:89-101) sends `LEAD_IN_FRAMES = 5` (100 ms) of silence before the first clip of
     a speaking spell, so the listener's jitter buffer and Discord's speaking-state flip don't eat its head (the
     comment cites "Theseus here" heard as "seus here", theseus-yf9ct). 100 ms is evidently not always enough for the
     greeting, the first clip after the join.
   - **The tail.** `deepgram.rs` (~:36-44, ~:280-305): Aura-2 sometimes ends a short sentence mid-word ("Go ahead."
     garbled 3 of 8 on 2026-10-18, theseus-sro04); it is asked again up to `SYNTHESES = 3` times, and a clip still
     cut off is faded out over `FADE` (30 ms). "Go ahead" heard as "gaaah" may be a cut-off "ahead" whose fade ate
     the rest, not a clipped head. The owner said "at the beginning", so the head is the likely one, but read both.
     `FADE` is a fade-*out* on a cut-off clip, not a fade-in.

**Risk: low to medium.** Three numbers become config with new defaults, plus validation and docs; the merge is the one new path. The longer pause adds about
0.3-0.4 s of latency to every turn: say that in the template comment, so the owner can trade it back.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `voice-knobs` | theseus-bp0lp | `[voice] pause_ms`, `lead_in_ms` and `merge_ms` as config with the owner's defaults; a 10 ms fade; the unfinished-utterance merge, validated, plumbed into the engine and the songbird IO; template and docs; the tail cut-off examined | `crates/theseus-core/src/voice.rs` (+ its tests), `crates/theseus-core/config/theseus.example.toml` (the `[voice]` block only), `crates/theseus-discord/src/runtime/voice.rs` (`engine_config` and the `SongbirdIo` join's call only), `crates/theseus-discord/src/runtime/voice/tests_config.rs`, `crates/theseus-voice/src/songbird_io.rs`, `crates/theseus-voice/src/engine.rs` (the merge, its config field, its tests), `crates/theseus-voice/src/deepgram.rs` (`FADE` only), `crates/theseus-voice/AGENTS.md` (the lead-in line ~:33 and the pause), `crates/theseus-core/AGENTS.md` (its `[voice]` paragraph, if it lists the keys), `docs/status.md` (one dated entry) | M |

## What exists (at `7f428787`)
- **`[voice]`** is `VoiceConfig` in `crates/theseus-core/src/voice.rs`: serde defaults per field (`default_judge_ms`
  is the model for a millisecond key), `impl Default`, and `Config::validate_voice` (~:150), which bails with the key,
  the value, the range and the default, as `judge_ms` does (`1..=2_000`, "(250 is the default)"). Its tests are in the
  same file (~:330-410); one lists every boolean switch so that none reads false unseen (~:339-346).
- **The call's engine settings** come from `engine_config` in `crates/theseus-discord/src/runtime/voice.rs` (~:895):
  `EngineConfig::new(users)` and then the keys `[voice]` owns (`join_guard`, `echo_cancel`, `judge_ms`). Its tests
  are `runtime/voice/tests_config.rs` (58 lines).
- **The engine's own default** (700 ms) is what the engine's own tests read (`vad.rs`'s
  `an_utterance_closes_on_the_35th_quiet_frame_without_its_silence` uses its own `VadSettings`, not `Config::new`).
- **The lead-in** is a `pub const` in `songbird_io.rs`, read by `Spell::begin` (`LEAD_IN_FRAMES > 0`), `led_in`, and two
  tests (~:584, ~:765-771). `SongbirdIo::join(&songbird, guild, chan)` is called once, at `runtime/voice.rs` ~:428;
  the `SongbirdIo` struct (~:326) holds the `Spell` (~:481).

## What to build
1. **`[voice] pause_ms`** (default **1200**): how long a quiet listener closes an utterance. Validate it as `judge_ms`
   is: a range with a reason (say `300..=3_000`; below 300 every breath is a turn, above 3 s a reply waits too long),
   the bail naming the key, the value, the range and the default. `engine_config` sets `end_of_utterance` from it.
   Leave `engine::Config::new`'s 700 ms as it is, so the engine's own tests read as they did; this is the one place the
   config sets it, as for `join_guard`.
2. **`[voice] lead_in_ms`** (default **400**): the silence before the first clip of a speaking spell. `0` turns it off.
   Validate `0..=1_000`. Replace `LEAD_IN_FRAMES` with a value the `SongbirdIo` is given at its join (a parameter or a
   small settings struct; whichever reads simplest) in whole 20 ms frames, rounded up, as `engine::frames` does. Keep
   the name `lead_in` everywhere: one right name, no alias of the old const. Its two tests take the value they set.
3. **The tail: a shorter fade.** `FADE` in `deepgram.rs` goes from 30 ms to **10 ms** (the owner: "a shorter fade"),
   so a clip still cut off after every ask loses less of its last sound. Keep it a const (no key). Read the cut-off path
   and say in the report whether "Go ahead." can still reach the listener cut off (all `SYNTHESES` asks cut off). Any
   other change there is its own commit and its own test with a plant.
4. **Template.** Add `pause_ms` and `lead_in_ms` to the commented `[voice]` block of
   `crates/theseus-core/config/theseus.example.toml`, in its style (one line each and a continuation line if needed):
   what each does, the default, and the trade ("longer: fewer chopped sentences, each reply a little later";
   "longer: a whole first word, a later first sound"). Comments only: no key goes live in the template.
5. **Docs.** `crates/theseus-voice/AGENTS.md` ~:33 names `LEAD_IN_FRAMES (5, 100 ms; 0 is off)`: make it the config
   key and its default; add a sentence for the pause. One dated entry in `docs/status.md`'s voice section.
6. **The unfinished-utterance merge (approved; build it after 1-5 are green and committed).** An utterance whose
   transcript looks unfinished (no terminal punctuation: `.`, `?`, `!`) waits up to **1.5 s** for the same speaker's next
   utterance; if one starts in that window, the two transcripts are joined (one space) into one turn. A finished-looking
   transcript becomes a turn at once, exactly as today, so the common case gains no latency. Make the window
   `[voice] merge_ms` (default 1500; 0 turns the merge off; validate `0..=5_000`) with a template line. Find the seam
   where a transcript becomes a turn in `engine.rs` and keep the judge's path (`judge_unclear`) as it is: a merged
   transcript is judged as any other. A barge-in over Theseus's speech is never held. If the seam cannot take this without
   a new wait on a finished transcript, stop, and write the seam and the blocker in the report's first section.

## Tests (each with a planted revert; the first is the key test)
- **`a_call_closes_an_utterance_after_the_configured_pause`** (in `tests_config.rs`): a config with no `[voice]`
  section gives the call's engine `end_of_utterance` 1200 ms; `[voice]\npause_ms = 1400` gives 1400 ms. Plant: drop
  the line in `engine_config` that sets it. The test must fail; paste the failing assertion.
- **`a_spell_leads_in_with_the_configured_silence`** (in `songbird_io.rs`'s tests, beside the existing lead-in test): a
  lead-in of 400 ms puts 20 silent frames before the clip; 0 puts none and `Spell::begin` reports no lead. Plant: keep
  a hard-coded 5 frames. Paste the failure.
- In `voice.rs`'s tests: the three defaults (1200, 400, 1500); a set value read back; each out-of-range value refused with the key
  named (as `judge_ms`'s test does). Plant: remove the range check for `pause_ms`.
- **`an_unfinished_utterance_waits_for_its_rest`** (engine tests): "so I was thinking" then, 0.8 s later, "we
  should go" is ONE turn "so I was thinking we should go"; "Go ahead." then the same follow-on is TWO turns; with
  `merge_ms = 0` the unfinished pair is two turns; a follow-on 2 s later is two turns. Plant: drop the merge. Paste the
  failure.
- The fade: a test that a cut-off clip's last 10 ms ramps to silence and the samples before it are untouched. Plant:
  restore 30 ms.
- The existing lead-in test (~:765-771) and the `vad.rs` tests pass. Do not weaken an assertion; if one must change
  because the value is now a parameter, say which and why.
- Run `cargo nextest run -p theseus-voice`, `cargo nextest run -p theseus-core voice`, `cargo nextest run -p
  theseus-discord voice`, then the template tests (`git grep -n EXAMPLE_TOML crates/theseus-core/src` finds them).

## Not in this row
- The live check (the owner says "go ahead" back after a `/join`, and talks with natural pauses): the owner's, on the
  installed daemon. Write the exact things to listen for in the report.
- Deepgram's own endpointing (`endpointing`, `utterance_end_ms`): Theseus does not stream to it; not this row.
- `voice/judged.rs`, the judge, `heard.v1`, echo cancel, the join guard.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- No new always-loaded model text and no new tool.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer: if a change would need one, leave it out and say so. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/integ/tail --head HEAD --quiet` and paste its output in the report. It
  must be empty.
- **Dependencies.** None new.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so in the report). Put `theseus-bp0lp` in
  each subject. End each message with the same `Co-Authored-By: Tabitha/Claude` trailer the branch's recent commits
  carry (`git log -3 --format=%B origin/integ/tail` shows it).
- **Names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
- Read `crates/theseus-voice/AGENTS.md` and the `[voice]` paragraph of `crates/theseus-core/AGENTS.md` before you edit.
  Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit; do not
  run `cargo fmt` in a way that moves lines in files you were not asked to edit. L1 tests need a non-root daemon with a
  job cgroup and fail on this VM; this row needs none of them, so say which tests you ran.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit: it must not be empty. It holds:
- what changed, by step;
- the three keys: name, default, range, and the template lines as written;
- what you found about the head and the tail of "Go ahead." (step 3), with the code you read;
- the merge (step 6): the seam you used, or the blocker;
- each test and its plant, with the failing output's line;
- what could not run on the VM, with the exact commands for the maintainer;
- the owner's live-check script (what to say, what to listen for);
- the keel guard's output;
- the store format (unchanged);
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
