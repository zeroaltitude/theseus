# CLOUD_REPORT: why, `theseus why [TURN]` and Discord `/why` (theseus-ighf)

Branch `cloud/20261011-why`, from main at fdd14925. No store format change (store format stays 27), no new dependency.

## What was found

- `context.explain` and `policy.explain` already hold everything the row asks for except three things the renderer needed and the daemon did not give a terminal client:
  - **The turn's tool calls.** `tool.started` is a notification only, not a ledger row, so the tools a turn called come from its `tool_call` nodes (`node.list`), whose `detail.gate` also holds the gate's own result and reason for that call. `policy.explain` then says the layer that set the posture.
  - **The route decision.** It is a `route.decided` ledger row (`ledger.tail`), with no turn filter, so the reader keeps the rows whose `turn_id` is the turn's.
  - **A recalled note's own date.** `RecallItem` carries a WAL position and no time (and it is a stored record, so adding one would be a store format bump). `context.explain` now answers `noted` (node id to unix ms, optional, absent from an older daemon), read from each admitted node's record through the heat cache. It is no text, so a shared place gets it too (the renderer hides it there).
- The compile's own ring/compaction budget report (`Compilation.budget`) is not on the wire (`CompilationInfo.manifest` is the manifest alone), so "what the compile dropped" is today the recall's drops by reason (budget, place rule, labeled wrong, external text, already in context, ...), a recall's overage, and the context files carried as headers alone in a shared place. Adding `budget` to `ContextExplainResult` is a small optional field when the owner wants the ring's cuts there too. Not done: it is outside the row's three listed reasons.
- Real turns carry the tools names as canonical (`fs.read`), the stand-in's tool calls as wire names (`fs_read`) in one test; the renderer matches both.

## What was built

1. **The renderer, once**: `theseus_protocol::why` (crates/theseus-protocol/src/why.rs; a pure module like `notices`, so the CLI and the Discord binding both link it without a new dependency, and `chat-commands` can call it). `why_lines` is all of it; `why_short` is each section's top items and a count of the rest, with the blocks biggest first, each line clipped to 150 characters and the whole cut to a character budget by whole lines, with a closing line that counts what it left. Plain lines, no colour (term-render can take them over). `hide_sources` (forced on when the explained place's class is not `private`) says only that an imported or another session's note was there.
2. **`theseus why [TURN] [--session S] [--json]`** (crates/theseus/src/why.rs). The session is picked as `history` picks it. TURN is a whole id or a unique start or end of one of at least four characters, resolved from `context.explain`'s turn list (the newest 50 turns that compiled a context). It reads `context.explain`, `policy.explain`, `node.list` and `ledger.tail`; `--json` prints the four answers as one object.
3. **Discord `/why`** (crates/theseus-discord/src/runtime/why.rs, registered in `commands()`, dispatched through `extensions_interaction`'s chain because `on_interaction` is at clippy's cognitive-complexity limit). The deferred acknowledgement carries the ephemeral flag; the answer is the short form in one code fence within 2,000 characters; a shared place never shows a private or imported source's text. It writes no frame and no ledger row, not even the `discord.command` audit row every other command writes.

Commits (all carry `theseus-ighf`): see `git log fdd1492..HEAD`.

## How it was proved

- Tests, all new:
  - `crates/theseus-protocol/tests/why.rs` (5): goldens `tests/why/{full,shared,short}.txt` (a turn with a route decision with effort, two recalls with dates, sources and reasons, a drop for the budget and one for the place, a context file carried as its header, and a gated tool); the shared-place golden asserts no word of the imported source, its episode or its date reaches it; the short form's top items, count and limit; a long note clipped; a session with no turn.
  - `crates/theseus/tests/why.rs` (4, a scripted daemon; goldens `tests/golden/why{,_older,_json}.txt`): the last turn, an older turn by a prefix with `--session`, an unknown and a too-short prefix refused before anything else is read, and `--json` as the daemon answered with the requests being reads alone (`session.list context.explain policy.explain ledger.tail node.list`).
  - `crates/theseus-discord/src/runtime/tests_why.rs` (4): registered once; a press through `on_interaction` and a real core with a scripted turn whose call asks first: the callback is a deferred message with the ephemeral flag (the fake gateway's `replies()`), the edit is the text, it is at most 2,000 characters and fenced, it names the session, the tool and its `approve`, and the core's `frames_appended` and `last_position` are unchanged; a shared place hides an imported source (and the text fits with 40 recalled items and 30 parts, `… and 37 more`, and a 3,000-character title).
  - `crates/theseusd/tests/why.rs` (2, the real daemon and the real CLI on the stand-in model): two turns, the first reading a file; `theseus why` names the last turn, `theseus why <end of the first turn's id>` names the older one with `fs.read` and its `open` posture, `--json` parses, and the daemon's newest ledger row and its count of rows are the same after as before. The second seeds a recall: BM25-only index (no model files), `[memory] mode = "live"`, a first session saying a fact, a second session asking about it, and `theseus why` shows `recalled: 1 of 1`, the source session, `why: ranked 1 of 1 hits, ...: bm25 #1 (4.45)`.
  - `crates/theseus-core/src/import/tests.rs`: the existing `a_turn_s_context_says_what_it_recalled_and_from_which_episode` now also asserts `noted` (each admitted node's own time, an imported message's inside its episode's span, and the same away from a private place).
- Planted reverts (each switched on, the named tests failed, then `git checkout` of the file and `touch`):
  - hide off in `source_words` -> `a_shared_place_shows_that_a_source_was_there_and_nothing_of_it` and Discord's `a_shared_place_hides_a_private_source_and_the_text_fits_the_limit`;
  - the short form's biggest-first sort off -> `the_short_form_keeps_the_top_items_and_counts_the_rest_inside_its_limit`;
  - no cut to the limit -> that test and Discord's limit test;
  - the CLI's prefix match reduced to an exact id -> `a_prefix_names_an_older_turn_and_a_session_flag_names_the_session`;
  - the route rows of every turn instead of the turn's -> the same prefix test (the older turn would show the newer one's route);
  - the ephemeral flag removed -> `why_answers_ephemerally_with_the_last_turn_and_writes_nothing`;
  - `why::command()` not registered -> `why_is_registered_with_the_other_commands` and the voice test's count;
  - a `discord.command` ledger row written in `Place::why` -> `why_answers_ephemerally_with_the_last_turn_and_writes_nothing` (frames and last position moved);
  - `noted_times` answering nothing -> `a_turn_s_context_says_what_it_recalled_and_from_which_episode`.
- Load: nothing here is a timing test, so no load run; the new tests ran alone and in the suite.
- A defect found in code I was in, fixed with its test: the voice test `join_names_a_voice_channel_and_leave_nothing` counted the binding's own commands as ten; it now counts eleven (`/why` is the eleventh), same assertion shape, count 13.

## Live check for the maintainer

On this VM the two theseusd tests above are the live check against a scratch daemon on the stand-in: `cargo build -p theseus -p theseusd -p theseus-index && cargo nextest run -p theseusd --test why` (2 pass, about 3 s). On the owner's machine, to see the text:

1. Scratch daemon on a fresh state dir (never the operator's): `D=$(mktemp -d); mkdir -p $D/projects; echo "low water at six" > $D/projects/notes.txt; theseusd example-config > $D/config.toml`; set `[model]` and every `[providers.*]` `api_base` to the stand-in model, each `[secrets]` entry to `env:SCRATCH_KEY`, `[tools] projects_dir = "$D/projects"`, `[discord]`, `[web]` off, `[index] enabled = true`, `weights_dir = "/nonexistent"`, `[memory] mode = "live"`; stand-in: `theseus-sim fake-model --addr 127.0.0.1:0 --rules rules.json` with `[{"when":"read the notes","calls":[{"name":"fs_read","input":{"path":"notes.txt"}}]},{"when":"","text":"ok"}]`; `SCRATCH_KEY=x theseusd --config $D/config.toml --state-dir $D/state --socket $D/sock &`.
2. `S="theseus --socket $D/sock"`; `$S ask "Remember: the tide table for harbour zeta puts low water at six."`; wait until `$S memory search "tide table harbour zeta"` lists it; `$S ask "What does the tide table for harbour zeta say about low water?"`; `$S ask "read the notes, then say what they hold"`.
3. `$S why` should print the last turn (model and profile; `routing:` says no decision here; the parts biggest cost; `tools called` with `fs.read open set by posture ...`). `$S why <last 6 characters of the second turn's id>` should print `recalled: 1 of 1 candidates`, the note's date, its source session, and `why: ranked 1 of 1 hits ...: bm25 #1 (...)`. `$S --json why` prints `{context, policy, route, nodes}`. `$S ledger -n 1` before and after any of these is the same row.
4. `/why` in the owner's DM on a scratch bot, if one is configured: a scratch daemon with `[discord] enabled` and a bindings file binding only the owner's DM on the scratch bot (never the operator's places, AGENTS.md's trap), after one message to it type `/why`: only the presser sees an ephemeral code block with the sections above, in one message of at most 2,000 characters; `$S ledger -n 3` has no `discord.command` row for it. Registering commands is global to the bot, so the scratch bot's list replaces the installed one's until the operator's daemon next starts.

## Left, and design choices the owner should hear about

- `/why` writes no `discord.command` audit row (the row's job is audit of commands that act; `/why` is the first that only reads, and the row is a frame). If the owner wants every command audited, it is one `binding_ledger` call in `why_interaction`, and the frame test would then expect one.
- A shared place's `/why` shows the place's own conversation's notes with their dates and kinds, and for any other session's note only that one was there. It shows the parts' names (context-file paths are as `context.explain` gives them in a shared place) but not a recalled part's header.
- A recalled native note has its date and its source session (and title in a private place), but never its text: the reason is the rank and each source's rank and score, as the cockpit shows.
- The ring's budget cuts are not on the wire (above).
- For `chat-commands`: call `theseus_protocol::why::{why_short, why_lines, tools_called, turn_rows, WhyInput}` with the four reads (`context.explain`, `policy.explain`, `node.list` kind `tool_call`, `ledger.tail` kind `route.decided`); `crates/theseus/src/why.rs::ask` and `crates/theseus-discord/src/runtime/why.rs::why_text` are the two callers to copy.
- Docs for the maintainer to write: a Part III item and `docs/status.md` for `theseus why` and `/why`; crates' AGENTS.md (theseus, theseus-protocol, theseus-discord) are updated in the commits.
- The VM's disk allowance (about 40 GB) filled twice during the repeated rebuilds (the second and third gate runs); `rm -rf target/debug/incremental` and deleting stale `target/debug/deps` files fixed it, and the gates ran with `CARGO_INCREMENTAL=0`.
- The gate was run with `THESEUS_KEEL_BASE=fdd1492`: on this VM `origin/main` is an older commit than the one the task cloned (merge-base a9ad950f), so the keel guard judged the five erosions that main itself landed since, whose signed acks do not verify without the owner's key. Against fdd1492: 0 findings.

## Keel findings expected

None. (`python3 scripts/keel-guard.py --base fdd1492`: `keel: ok, 0 findings`; the voice test's count change keeps its assertion count.)

## The gate

`CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=fdd1492 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the tree of the last code commit (81f0c7e): keel (0 findings), keel tests, fmt, shape, features, clippy, cockpit and the test build all pass, as does the reader rule. The suite phase: 3,759 tests, 3,726 passed, 33 failed, 44 skipped. The 33 are the known L1 failures on this VM (32 in theseus-sandbox's `contract` and theseusd's `sandbox`, and theseus-sandbox's bench `spawn_100`, root without a job cgroup, theseus-pv6i); no other test failed. The phases after the suite I ran myself: the protocol types (`cockpit/src/protocol.gen` has the one regenerated file, `ContextExplainResult.ts`, committed) and `cargo deny --offline check` (advisories, bans, licenses, sources ok). The lifecycle, jobs and turn benches were not run (`THESEUS_GATE_NO_BENCH=1`; the speed budgets are measured on the owner's machine). The CLI's seen-file, welcome and `--help` goldens are unchanged; `theseus --help` gains a `why` line.

Earlier gate runs failed for reasons since fixed: clippy's `is_multiple_of` and the complexity limit of `on_interaction` (the `/why` dispatch moved into `extensions_interaction`'s chain), the voice test's command count, two ENOSPC runs, and a Discord test of mine that depended on a long line leaving room for the tools line (fixed by clipping lines in the short form, with the test's assertions made deterministic).
