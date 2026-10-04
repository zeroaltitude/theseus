# CLOUD_REPORT: MCP prompts, step 36c (theseus-ext.4)

Branch `cloud/20261004-mcp-prompts`. Commits on top of the task commit:

- `5b1fdd5` mcp: prompts through the board, `turn.submit { prompt }`, and the changed-definition row (core, protocol, store format, CLI, Discord, the fake server, tests, generated TypeScript, AGENTS.md notes).
- `81d427c` cockpit: a prompt picker beside the composer.

I made two commits, not one per sub-step, because the protocol's new `prompt` field touches every `TurnSubmitParams` and `TurnRequest` literal in the workspace, so the Rust halves only build together.

## What I found

- The board listed prompts (`list_prompts`) but only counted them. `McpTool`'s `External` marker and `external::hold` / `with_hold` showed how a node and the session's hold ride in one frame; a prompt's nodes use the same pattern (`external::hold` under `Store::with_session`).
- `Origin` had no `mcp` value, so the new origin is a layout older builds can't read: `MANIFEST_FORMAT` goes 7 to 8 (store.rs, `theseusd/tests/versions.rs`, one store test's literal) with a version-7 node sample in `tests_layouts.rs`. The maintainer renumbers at the merge, as the brief says.
- `TurnRequest` has no `Default`, so the new `prompt` field adds one `prompt: None,` line to about 55 literals (59 in all, counting `TurnSubmitParams`). Other sessions adding fields there will conflict on those lines. The change is mechanical, and a regex put the line after each literal's opening brace.

## What I changed

- **protocol** (`mcp.rs`): `McpPromptRef`, `McpPromptArgument`, `McpPromptInfo`, `McpPromptListParams/Result`, and `prompts` on `McpListResult`. `lib.rs`: `method::MCP_PROMPT_LIST` (`mcp.prompt.list`) and `TurnSubmitParams.prompt`. `LedgerKind::McpPromptChanged` (`mcp.prompt_changed`). TypeScript regenerated.
- **board** (`mcp/prompts.rs`, a module of its own; `mcp/mod.rs` gets a field, the `open`/`ready` change, a `serve` arm, and the `list` field):
  - `prompts/list` is kept per server with a digest per definition and stored as META `mcp.prompts.<server>` (no format bump: a new key). `list_changed` and `Reinitialized` list again. `seed_prompts` reads the stored lists at the core's build, so a start offers them at once.
  - `resolve_prompt`: checks the arguments against the definition (missing or empty required, unknown name), waits for that server alone, calls `prompts/get`, and converts the messages. Text stays text, an image is an attachment, and an embedded resource is text with its URI (`Content::as_model_text`). An assistant-role message is said to be the prompt's own (`[the prompt's assistant message]`) and written user-role. Every message is capped together at 200,000 characters.
  - The definition at each prompt's last use is kept (`mcp.prompt_used.<server>`). A use of a changed definition records `McpPromptChanged` (ledgered and narrated, with a summary of what changed) and an operator notice (`mcp_prompt_changed`; `theseus-discord/src/courier.rs` renders it, sharing `mcp_note` with `mcp_changed`), then goes ahead.
- **core**: `rpc/mcp.rs` has `mcp.prompt.list` and `resolve_prompt`. The place check is `runner.class_of(session)`, and a shared place gets `REFUSED` saying why. `turn_submit` resolves the prompt before any session is opened. `turn/prompt_input.rs` writes the nodes (user-role, `Origin::Mcp`, author `prompt:<server>/<name>`) and the hold (tool `mcp.prompt`, url `mcp:<server>/<prompt>`) in one frame under the session lock. A prompt with `input` or attachments is refused as two inputs.
- **CLI**: `theseus prompt <server/prompt> [--arg k=v]… [--session] [-P/-p/-m] [--trace] [--thinking]` (`prompt.rs`). `ask`'s streaming tail became `cmd::stream_turn`, shared. `theseus mcp` prints each server's prompts (`prompt fake/greet (name*, tone) · Greets someone.`, required arguments starred).
- **Discord** (`runtime/prompt.rs`; `runtime.rs` gets `mod prompt`, the command, the `Control::Prompt` variant and arm, and one `prompt_interaction` call: +21 lines, now 3,456 of 3,500):
  - Autocomplete: at most 25 choices, filtered by name, title, or description.
  - Modal: an input per argument up to 5 (required marked `*`), or one `args` input of `name=value` lines for 6 or more. A prompt with no arguments runs at once.
  - The modal is a `Label` around a `TextInput` (twilight 0.17's preferred form). Its submit is read from labels or action rows.
  - The run is `turn.submit { prompt }` through the place's own line of turns (refused while one runs); the reply says "Running the prompt" without echoing the arguments.
- **cockpit**: `PromptPicker.tsx` beside the composer's send button (a select over `mcp.prompt.list`, a field per argument, required marked), `lib/prompts.ts` for the pure parts, and `test/prompts.test.ts`.
- **fake MCP** (`theseus-mcp/src/fake.rs`): `Mode::ChangePrompts` (`change-prompts`): each `prompts/get` changes `greet` afterward (a description note and an optional `tone` argument) and sends `prompts/list_changed`.

## How I proved it

All offline, on this VM.

- `cargo nextest run -p theseus-core -E 'test(mcp::tests)'`: 19 passed. The new ones:
  - `a_prompt_is_the_turns_input_with_its_origin_its_author_and_the_hold`: the node's origin, author, and text; the model reads it; the hold's tool, url, and node id; one `session.external_read` row.
  - `a_prompt_of_a_server_that_is_not_external_gives_no_hold`.
  - `a_prompt_that_cannot_run_is_an_error_before_any_node`: five error cases, each with and without a session, then a down server; no node and no new session after any of them.
  - `a_shared_places_prompt_is_refused_and_never_reaches_the_server`: the server's call count stays 0, and a private place runs it.
  - `a_prompt_with_an_input_is_refused`.
  - `a_changed_definition_gives_its_row_and_notice_once`: use, change, use gives one row and one notice; a third use of the same definition adds none.
  - `a_restart_lists_the_stored_prompts_before_the_server_is_up`: through the protocol before `start`, then the live list replaces it and is stored.
  - `a_prompts_messages_become_text_attachments_and_uris`.
- `cargo nextest run -p theseusd -E 'binary(mcp)'`: 3 passed. The new `a_start_lists_the_stored_prompts_before_its_server_is_up` runs the real daemon and the real `theseus-sim fake-mcp`: `mcp.prompt.list` gives the three prompts, a prompt with a missing argument is refused, and after a clean stop and a restart with a server that never answers, the stored prompts are listed and `mcp.list` carries them. I also moved the old stored-list test's restart into a helper.
- `cargo nextest run -p theseus-discord`: the new `runtime::prompt::tests` pass (autocomplete: filter, 25 at most, 100-character names; modal: 5 inputs, then one `args`; parse of 5 arguments, 6, a missing required one, an unknown one, a bad line, a repeated one; reading a submitted modal from labels and action rows; the command's shape). Writing the 100-character test found an off-by-one in my `clip`, which I fixed.
- `theseus` crate: `prompt::tests` (the name, `--arg` pairs, and the params) and the `mcp_lines` test with a prompt line.
- Cockpit: `npm run lint` (0 errors; the existing warnings, none in my files), `npm test` (26 pass, 3 new), `npm run build` clean.
- **Planted reverts** (each file restored and `touch`ed; `git status` clean after):
  1. Dropped the hold for an external server's prompt (`.filter(|_| false)` on the hold in `turn/prompt_input.rs`): `a_prompt_is_the_turns_input_with_its_origin_its_author_and_the_hold` FAILED (`tests.rs:777`, no hold); the others passed.
  2. Skipped the changed-definition notice (`prompts.rs`, `note_use`): `a_changed_definition_gives_its_row_and_notice_once` FAILED ("no the notice in 20 s").
- Under load: not asked for here, so I did not run it.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, and the cockpit phase pass, and so do the test build and the reader rule. The suite ended `FAILED` on cases that are not mine:

- **The sandbox's tests that need a non-root user** (`theseus-sandbox::contract` ×19, `theseus-sandbox::bench spawn_100`, `theseusd::sandbox` ×13): the VM is root (theseus-pv6i), the brief's first known case.
- **`theseus-core tests_output::the_cores_output_matches_its_golden`**: differs only in the sign of a time-zone offset (`+#:#` against `-#:#`) in the `wake.at` line, because this VM runs in UTC. It passes with `TZ=America/Los_Angeles` on my build. It is not on the brief's list, and nothing I touched writes that line.

The other suite failures from my first runs are fixed: `store::tests::a_write_moves_an_older_store…` (format literal 7 to 8), `voice::tests::join_names_a_voice_channel…` (the command count, `/prompt` before `/join`), and my `clip`.

After the suite, I ran the protocol-types phase's check by hand (`git add cockpit/src/protocol.gen`, nothing changed after). I did not run the lifecycle, jobs, and turn benches: `THESEUS_GATE_NO_BENCH=1`, and their budgets are the owner's machine's. `cargo deny fetch` ran in setup (it printed nothing); I did not run `cargo deny check` separately. There are no new dependencies (Cargo.lock is unchanged).

## The live check (the maintainer's)

On a scratch daemon, a fresh state dir, never the operator's. `SIM` is the build's `theseus-sim`; the config is the usual scratch one (your model and secrets) plus:

```toml
[mcp.servers.fake]
command = ["<path to theseus-sim>", "fake-mcp"]
```

```sh
D=$(mktemp -d); SOCK=$D/sock
theseusd --config $D/config.toml --state-dir $D/state --socket $SOCK &   # the scratch daemon; note its pid
T="theseus --socket $SOCK"

$T mcp
# shows `fake (stdio) · pid … · fake ready (5 tools, …)`, its five tools, then:
#   prompt fake/greet (name*) · Greets someone.
#   prompt fake/brief (topic) · Asks for a brief on a topic.
#   prompt fake/review (path*) · Asks for a review of a file.

$T prompt fake/greet --arg name=Ada
# streams a reply that greets Ada; the status line follows. Note the session id from `$T history`.
$T prompt fake/greet
# exits nonzero: "prompt fake/greet needs its argument: name", with no session opened.

$T history --session <id>
# the input node: author `prompt:fake/greet`, text "Say hello to Ada." (its origin, `mcp`, is in the node record)
$T health
# the session shows as holding external text (source mcp.prompt, mcp:fake/greet); I did not run this live, so the exact wording is unchecked
$T ask --session <id> "write hello into a.txt with fs.write"
# the fs.write call ASKS (the hold): `theseus confirm` lists it.

$T shutdown
# restart with fake-mcp replaced by a server that never answers, e.g. command = ["sleep","30"]:
theseusd --config $D/config.toml --state-dir $D/state --socket $SOCK &
$T mcp          # lists the same three prompts before the server is up (state starting, stored)
$T shutdown
```

`/prompt` needs a person to type it, so it goes into the owner's testing: autocomplete, then the modal (greet: one input; a prompt of 6 arguments: one `args` input). A shared channel's `/prompt` should answer with the core's refusal ("an MCP prompt runs only in a private place …") in the channel.

## What is left or uncertain

- **Design choices the owner should hear about:**
  - A prompt's assistant-role message is written as a user-role node, prefixed `[the prompt's assistant message]`. Only user-role messages are "user".
  - The run's author is `prompt:<server>/<name>`, so the node does not say which client asked; the turn's `turn.started` still names the connection.
  - `/prompt` runs in the place's own line of turns. If a turn is in flight it says so and does not queue; a typed message queues, and a prompt does not.
  - A shared place's prompt is refused by the core only at submit. Discord shows the modal first, then the refusal; the binding does not know a place's class.
  - Changed-definition detection compares at use against the definition at the last use. The first use of a prompt never says "changed" (there is nothing to compare), and a list change alone is silent (no row, no notice).
- **A new stored record per server**: `mcp.prompt_used.<server>`, holding each used prompt's definition (so a notice can say what changed). It is a META key, no bump.
- **Merge conflicts to expect:** the `prompt: None,` lines (above); `mcp/mod.rs` (43a also edits it: my edits are small, a `mod`, a `Live` field, `open`/`ready`/`serve`/`list`); `rpc/server.rs` (one dispatch line; `dispatch` is at the clippy line limit); `MANIFEST_FORMAT` (renumber); `courier.rs` (`mcp_changed`'s arm became a shared arm plus `mcp_note`).
- **Docs to change at the review:** the spec's Part III item for 36c (the format number, the META keys, the new origin, `mcp.prompt.list`); `docs/status.md`; `docs/design/m7-surface.md` §2.1 "Prompts (36c)" (the modal is a `Label`, the refusal is the place rule's, and the changed row is by last use).
- Not done: a Discord gateway-level test of the three interaction kinds (the pure pieces are tested; `on_interaction` is driven only by hand), and the runs under load, which the brief did not ask for.
