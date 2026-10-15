# CLOUD_REPORT: named-voice (theseus-slvl.7)

Branch `cloud/20261013-named-voice`. Commits: `17908992` (the feature, tests, AGENTS.md notes), the protocol.gen commit after it.

## What I found
- `persona-data` had joined: `PersonaConfig.tts_voice` and `name` existed but nothing read them for a call; `DeepgramSettings` copied the global voice once at `/join`.
- Core has no dependency on theseus-voice and the Deepgram key is the daemon's alone, so the audition runs in the daemon (new RPCs) and the CLI only writes files.

## What I changed
1. **Voice follows the persona.** `Core::voice_of(session)` (core `rpc/voice_pick.rs`) = persona in play (`persona_of`) -> its `tts_voice`, else `[voice] tts_voice`. The binding's `Metered::synthesize` asks it at every sentence and calls the new `DeepgramSpeech::synthesize_as(voice, text)`, so a persona change mid-call is heard at the next sentence, no reconnect. The voice id is recorded per call (`voice.joined`: `voice`, `persona`, `greeting`) and per sentence (`speech.synthesized` detail `voice`). No stored record changed: **no store-format bump**.
2. **Greeting.** `[voice] greeting` (default `{name} here. Go ahead.`, empty = none; in the example template). `begin()` queues it as the engine's first command, after `songbird.join` succeeded (`start_call`), so it is spoken before anything heard is answered, through the same Deepgram path and booking.
3. **`theseus voice` / `theseus voice audition [--voices a,b,c] [--text ..] [--out dir]`.** New RPCs `voice.show`, `voice.audition` (owner's run, `Act::JudgeRun`, in the CLI's `OPERATORS`, refused in a job). Default six Aura-2 voices (max 8 named, 500 chars); WAV files `<n>-<voice>.wav`; prints paths, failures and the estimated cost; refuses without a configured/resolved Deepgram key (names the entry, never the value; the key is cut from error bodies).
4. **`persona edit --voice ID`**: alias of `--tts-voice`.

Header tokens added: none (no prompt text; the greeting is spoken, not sent to the model).

## Proof
- New tests: core `rpc::tests_voice_pick` (6), discord `runtime::voice::tests_persona` (5), CLI `voice_cmd::tests` (4) + `persona_cmd` alias test. All pass.
- Planted reverts, each failed its test and was restored + touched:
  - global voice used despite persona -> `voice_show_names...` and `the_request_carries_the_personas_voice...` failed;
  - greeting not queued first -> `the_greeting_is_spoken_before_anything_heard_is_answered` failed (`["heard"]`);
  - Metered ignoring the persona (mid-call change) -> `the_request_carries...` failed (andromeda vs orion);
  - speech before the join completes (a synthesize ahead of the gateway check) -> `a_join_that_fails_says_nothing` failed.
- Not driven: a real songbird join (needs a gateway); covered by the live check.
- `cargo fmt`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -D warnings`, cockpit lint/test/build: pass. Reader-rule registry tests: 15/15.
- Full suite (`cargo nextest run --workspace`): 5463 passed, 34 failed: 33 are the known L1 sandbox/theseusd-sandbox failures; the 34th, `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` (dispatched count 2, wanted 1), **fails identically on the base commit `e6708e4e`** (checked by stashing my work), so it is not mine.
- Lifecycle bench (`bench lifecycle --runs 10 --check`, debug, this 4-core VM): cold-start p50 25.5 ms; "from the config copy" p95 201 ms (one outlier, p50 25.5) and "health after seed" p95 5.4 vs 5 ms MISSED; the other budgets ok. Nothing on the start path was touched (no timer, no startup work); I take these for VM noise, but the maintainer should rerun on the owner's machine.

## Gate
`scripts/gate.sh` stops at the **keel** phase (and the door): `budget-raised crates/theseus-sim/src/lifecycle.rs:155` from main's commit `1c2beb2747`, whose `Keel:` ack signature cannot be verified on this VM (`%G? is E`). Not my change. The phases after it I ran by hand as above (fmt, shape, clippy, cockpit, registry, suite, protocol types via the committed protocol.gen); `deny` and `features` not run. **Keel findings expected from my commits: none.**

## Live check (maintainer, owner's keys)
1. `theseus voice` -> voice in play, source (persona / `[voice] tts_voice`), greeting, key resolved.
2. `theseus voice audition --out /tmp/aud` -> six WAVs `1-aura-2-andromeda-en.wav`..., paths and "estimated cost: $0.0013"-ish (a few tenths of a cent); play them, pick one.
3. `theseus persona edit <persona> --voice aura-2-<pick>-en`, restart the daemon, `/join` the owner's test voice channel: the greeting "<Name> here. Go ahead." is heard in that voice before anything else.
4. `theseus ledger tail --kind voice.joined -n 1` shows `voice`; `--kind speech.synthesized` shows `voice` on each sentence. Change the session's persona mid-call and the next sentence's row names the new voice.

## Left / for the owner
- The audition books to no session (it has no session); it is bounded (8 voices x 500 chars, at most ~12 cents) instead. Say if it should book to a session or count against the day ceiling.
- The greeting plays through the engine as a "report"; the owner can talk over it (barge-in). It is spoken in two sentences ("Name here." "Go ahead.").
- Docs to update at review: spec Part III item, `docs/status.md`; AGENTS.md notes for core and discord are in the commit.
- The VM's disk allowance filled twice during builds (I deleted only `target/` artifacts).
