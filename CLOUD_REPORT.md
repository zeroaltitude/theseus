# CLOUD REPORT: self-card (theseus-slvl.8)

Branch `cloud/20261013-self-card`; the feature is commit d59d749.

## What I found
`system_note_for` (toolrun.rs, 2,477 lines) has no config and no bindings, so the card could not live there. The core
learns the bindings only through `PlaceRule` (told at the binding's start), and `PlaceView` had no voice information.

## What I changed (d59d749)
- `crates/theseus-core/src/self_card.rs` (new): `card(cfg, tools, place)`, one sentence per power, each only when it
  holds: `/join` (private Discord place, a voice channel bound, `[voice] enabled`); wakes and tasks (`wake.at` or
  `task.create` offered here); looking up past conversations (`memory.lookup` offered here, so private only); the
  cockpit address (private place, `[web] enabled`; IPv6 bracketed). Built from config, catalog and place class only.
- `context_parts.rs` `header_parts`: appends the card to the tools note (one `\n`). `toolrun.rs` gets one line only
  (`voice: false` in `TurnCtx::place`).
- `PlaceView.voice`; `PlaceRule::bind_voice`; `Core::bind_voice`; the Discord binding calls it in `guilds.rs`
  `tell_core` (`b.channel.iter().any(|c| c.voice)`).
- Three tests in tests_m3.rs rebuilt the system block by hand from `system_note()`; they now use a `noted()` helper
  that adds the card.
- No store, protocol or config-key change: no format bump, no template change.

## Proof
- `tests_self_card.rs`, 7 tests, all pass: private with voice (exact golden), without voice or with `[voice]` off,
  web UI off, shared place (wakes and tasks only), IPv6, same bytes across two runtimes of a config and a changed
  config changes them, and the place rule's `voice` flag (DM yes once bound; shared channel, CLI and unbound: no).
- Planted revert: dropped `place.voice &&` from the voice line. `a_shared_place_hears_only_what_it_may_use` and
  `without_a_bound_voice_channel_it_does_not_offer_join` failed. File restored and touched.
  I did not plant a per-request value: the card's inputs are only `cfg`, `tools` and the place class by signature.
- `cargo test -p theseus-core --lib`: 2154 passed, 4 failed before the tests_m3 fix. Three were the system-block
  tests (fixed, now pass); `tests_reminders::a_reminder_in_a_placeless_session_posts_to_the_owners_dm_when_it_fires`
  failed under load and passes alone (not explained; unrelated to the card).
- fmt, clippy (`-p theseus-core -p theseus-discord --all-targets`) clean; keel guard: ok, 0 findings.
- Full gate (`THESEUS_GATE_NO_BENCH=1`, incremental off after the disk filled once): door, keel, fmt, shape, features,
  clippy, cockpit, test build, reader rule pass. Suite: 66 failures, all theseus-sandbox contract tests and theseusd
  sandbox tests (the known L1/root failures). The phases after the suite were not run through the gate.
- `theseus-sim bench lifecycle --runs 10 --check` (debug build, this VM): MISSED its `health` row (p95 9.5 ms against
  5 ms); the other rows shown ok. This VM is not the budgets' machine and the change is off that path; it needs
  the maintainer's run. I did not run the jobs or turn benches, nor the protocol-types phase.

## Tokens
Full card (private, voice bound, web on): about 290 characters, roughly 70 tokens. Without voice about 45; a shared
place about 15. Under the 80-token aim, so no line was dropped. Earns its place: it is the `/join` gap (theseus-0pp3).

## Live check for the maintainer
1. Scratch daemon with `[voice] enabled = true`, a bindings file with a `[[channel]] voice = true`, a fresh state dir.
   `theseus --socket <s> context explain <session>`: the tools note ends with the "You can join a voice channel…" card.
2. Remove the voice channel and restart: the `/join` sentence is gone. Set `[web] enabled = false`: no cockpit sentence.
3. In the owner's DM, paste a voice invite and ask "can you join the call?": the reply should tell the person to type
   `/join`. I did not run this: no scratch daemon or stand-in run here.

## Left / uncertain
- The voice line is true for any bound voice channel, not per guild (`/join` works from a private place into any bound
  voice channel), so it is not fixed per guild.
- `TurnCtx::place()` leaves `voice` false; only `PlaceRule::place` sets it. Nothing but the card reads it.
- A change to the bindings' voice channels reaches a session as a header change on its next recompile.

## Keel findings expected
None.
