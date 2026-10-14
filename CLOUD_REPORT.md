# CLOUD_REPORT: persona-data (theseus-slvl.2)

Branch `cloud/20261013-persona-data`. Head before this report: b3b798d.

## What is built (commit b3b798d)
- **Fields**: `[personas.<name>]` gains `name`, `emoji`, `card` (<=600 chars), `tts_voice`, `avatar`, `initiative` (quiet|normal|chatty, default normal); bad values are refused naming the key/allowed list (config/persona.rs, config/tests_persona.rs). Template documents them.
- **Built-in default**: `crates/theseus-core/persona/default.md`, 665 bytes (~165 tokens by chars/4) against the old three sentences (~55 tokens): about +110 tokens in the cached header, only for installs/sessions with no readable persona file. An owner's persona file replaces it whole: the header then carries none of it.
- **Resolution**: `Config::persona_for(session, place) -> PersonaView` and `persona_key_for`; precedence session > place > default_persona > built-in; unknown names pass to the next. `TurnRunner::persona_of` (memory only, nothing while no persona is configured). Bindings `persona = "<name>"` on a channel or DM; `session.open {persona}` (validated); a task takes its parent's. Stored as META keys `persona.session.<id>` / `persona.seen.<name>`: **no store-format bump**.
- **persona.changed**: new ledger kind; at each compile the persona's file digests and field digests are compared with the last sighting; a difference is one row per field, riding the turn's next frame with the new state (no extra frame, no watcher). First sighting writes state only.
- **Presence**: persona file unreadable -> `context.file_missing` row (now with `persona`), one operator `notice`, a health `context.warnings` line, and the header on the built-in default.
- **Surfaces**: `persona.list` (protocol `persona` module, TS regenerated); `theseus persona` (show), `persona edit [name] [--name|--emoji|--card|--tts-voice|--avatar|--initiative]` (line-edits the config file named by health, refused in a job, vault configs print the lines; no flags opens the file in $EDITOR), `persona history` (ledger.tail of persona.changed).
- Moved `target_for_session`'s old body to `turn/persona_step.rs::target_as_ran` to stay under turn.rs's ceiling (no ceiling raised).

## NOT built (say so plainly)
- Discord `/persona` and the cockpit Context page's persona panel. `persona.list` is the read both would use. No per-session switch after open (only at `session.open`).
- The `tts_voice`/`avatar`/`initiative` fields are carried and shown but read by no turn path yet (seams for named-voice, initiative-voice, surface-tag).

## Proof
- New tests: `tests_persona.rs` (11: default header, replaced whole, precedence incl. unknown name, task inherits, session.open + refusal + restart read, file edit once, field edit once, missing file with one notice + health warning, two personas' cached blocks), `config/tests_persona.rs` (3), CLI unit tests in persona_cmd.rs (4). All pass.
- Planted reverts: `owned()` forced false (built-in merged into owner's file) -> `an_owners_persona_file_replaces_the_default_whole` failed; `owned()` ignoring digest (missing file compiling thinner) -> `a_missing_persona_file_compiles_on_the_default_with_one_notice` failed. Restored, touched.
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): keel, fmt, shape, features, clippy, cockpit, test build, reader rule pass. Suite failures: the known L1 `theseusd::sandbox` tests; `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (**fails identically with my tracked changes stashed**, so not mine); `tests_standing::the_records_rebuilt_after_a_restart...` (same-millisecond record order flake; passes alone 2 of 3 runs, unrelated). Phases after the suite: protocol types test and `cargo deny check` pass by hand. Benches skipped (NO_BENCH); `theseus-sim bench lifecycle --runs 10 --check` NOT run.
- Existing tests changed because the header text changed: `tests_tiering` window 41,100->41,300 and `tests_route_keep` WINDOW 42,200->42,400, BILLED 36,700->36,900 (header grew ~110-200 tokens; same calibration practice as earlier rows); `cache_header.rs` asserts the owner's persona replaces the default (assertions rewritten/added, none deleted); `ContextStatus` literal in render.rs test got `..Default::default()`.
- Keel guard: 0 findings acked; its only line is a store test rename from the base, not mine. Keel findings expected: none.

## Live check (not run here)
Scratch daemon with `[personas.a]`/`[personas.b]` files and a bindings DM `persona = "b"`, then `theseus persona`, a turn in each, `theseus context explain`/manifests naming each persona's file digest, `theseus persona history` after editing b's file and a restart-applied `--card` edit.

## Notes for the maintainer
- Frames: no new frame on a plain turn (state and rows ride the next frame); not re-measured with the turn bench.
- Docs to update: spec Part III item, AGENTS.md of theseus-core (persona.rs, turn/persona_step.rs), cockpit/theseus-discord guides, status.md.
- This VM's disk allowance filled twice; I deleted stale target/ artifacts only.
