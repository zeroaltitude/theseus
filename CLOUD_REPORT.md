# Cloud report: speed-first-request (theseus-nsp9l.1)

Branch `cloud/20261019-speed-first-request`. One step, one commit: `87885465`.

## What I found
`Config::context_paths` carried every persona's files whenever two or more `[personas]` existed, in every place.
That is right where a message can engage a persona (route.v5), and pure cost in a cli, task or headless session,
whose persona is fixed. `owner_shape.toml` had no `[personas]`, so no test saw it. The census named its blocks
(header, context, session) while the compile has four (header, persona, project, session), so the persona files
printed as `session · directory` and the `Directory:` line as `system · directory`.

## What I changed (87885465)
- `Target.engages` (new field), set by `TurnRunner::engages(&SessionRecord)` (`turn/persona_step.rs`): two or more
  personas, and the session's place kind is a DM, a channel, or a thread-bound session. A cli, task or headless
  session does not engage. `Config::context_paths(persona, engaging)` takes the flag. The turn's spec step,
  `context_parts.rs`, `keep_warm.rs` and `census.rs` all read `target.engages`, so they agree byte for byte.
  `persona_choices`, `persona_ctx`, `persona_layer_point` and the header's `owned` follow the same flag.
  A fresh-session census sets it from the place kind asked for (dm, channel).
- Fixture: `owner_shape.toml` (and `owner_shape_old.toml`, which the rename test compares it with) gain five
  `[personas]` over `fixtures/personas/*.md` (invented text, 986/2,984/2,440/2,230/1,954 bytes = 10,594) and
  `default_persona = "alder"`. The path is `/@PERSONAS@/x.md`; `tests_first_request::with_personas` substitutes the dir.
- Census: block names come from the compiled layers (`header`, `context` for the persona and project layers,
  `session`); each persona file is its own part named by its file name; standing orders are a part of the header.
- Existing tests that relied on a person-less (cli) session engaging a persona now run in a DM (`tests_engage.rs`
  local `turn` wrapper, `tests_persona.rs` two tests). `tests_trim_tokens`' DM line (`< 10,500`) measures the owner
  shape without its personas (`without_personas`), since the DM budget test holds the DM to 15,000.
- Clippy's 100-line limit: `speak_as_session` and `census_target`/`peek_files` helpers keep `run_inner_body` and
  `census_request` under it. `AGENTS.md` of theseus-core describes the rule.

## Tokens on the owner-shape fixture (first request, census estimate)
| place | before (planted revert = base behaviour) | after | budget |
|---|---|---|---|
| cli | 10,588 | 7,555 | 8,000 |
| task | not printed before | 6,808 | 8,000 |
| dm | 13,074 (unchanged: it carries all five) | 13,074 | 15,000 |
Defaults (no personas): unchanged, the existing test passes. Step 3 (shorten tool schemas, defer tools) was not
needed: the cli is under 8,000, so I did not touch `proc_run`, any tool text, or `ALWAYS_LOADED`.

## Proof
- `cargo nextest run -p theseus-core` (TZ=America/Phoenix): 3,384 run, 3,384 passed (one earlier run had
  `tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory` fail once; it passed on the
  rerun and in the final run; not on the flaky list; I did not find its cause).
- New tests (`tests_persona_layer.rs`, `tests_census.rs`): `a_cli_and_a_task_session_carry_only_the_persona_in_play`,
  `a_clis_persona_is_the_one_in_play_and_its_change_changes_the_system`,
  `in_the_dm_a_persona_switch_changes_no_byte_of_the_prefix`, `a_keep_warm_replay_matches_the_turns_request_bytes`
  (five personas, cli session, paused clock), `census_names_the_context_block_and_each_persona_file`; and
  `every_kind_of_place_is_within_its_budget` now runs on the five-persona owner shape.
- Planted reverts (restored and `touch`ed, `git status` checked):
  1. `context_paths` always carries every persona: `every_kind_of_place_is_within_its_budget` fails (the owner's cli
     reads 10,588 tokens), and so do `a_cli_and_a_task_session_…`, `a_clis_persona_is_the_one_in_play…`,
     `a_keep_warm_replay_matches_…`.
  2. The census never marks the DM as engaging: `in_the_dm_a_persona_switch_changes_no_byte_of_the_prefix` and
     `a_cli_and_a_task_session_…` (its DM half) fail.
  3. The persona layer's block named `session`: `census_names_the_context_block_and_each_persona_file` fails
     (`["header","session","session"]`).
- `cargo fmt`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `python3 scripts/keel-guard.py --base bbc65530 --head HEAD --quiet`: empty, rc 0.

## What I could not run, and the gate
- `scripts/gate.sh` stops in its keel phase here: this clone's `origin/main` is 96c592c0, older than the cloud
  base bbc65530, so the guard judges other lanes' already-merged commits (4 erosions in rsi/ and scripts/keel-guard.py,
  none of mine). With the real base the guard is empty (above). I ran the later phases by hand: fmt, shape, clippy,
  and the theseus-core suite. Not run: cockpit, deny, the rest of the workspace suite, benches.
- The disk allowance ran out twice, so I removed build output (`cargo clean -p` for several crates). A later
  `-p theseusd` run then failed 3 tests (`headless` x2, `why` x2) because the `theseus` binary was gone, not because
  of the change (`why_…` printed ENOENT for the binary); `cache_header` and `thread_import` passed before that.
  Maintainer: please run `cargo nextest run --workspace` on the owner's machine.
- Live check on the owner's daemon (not run): `theseus census --place cli` should show blocks
  `header, context, session` with one persona file under `context` and a total under 8,000 (about 7,5xx);
  `theseus census --place dm` the five persona files as parts of `context`, total near 13,000;
  `theseus census --place task` under 8,000. Then a DM turn that addresses another persona ("Brook, ...") should
  show no `system_changed` recompile between two turns, and `theseus persona`/a cli session switching persona
  should show one.

## Uncertain, for the owner
- A cli session no longer engages a persona by address or by Jev's verdict (its persona is the session's): the
  persona-choosing path is DM, channel and thread only. That follows the brief; say so if a cli should still engage.
- `PlaceKind` decides the rule (an outbox target with `:dm:` is a DM, any other target a channel; no target is a
  cli unless the session is a thread's).
- Doc changes for the maintainer: spec Part III item for this step; `docs/status.md`'s first-request line (owner-shape
  cli 7,555, task 6,808, DM 13,074).
