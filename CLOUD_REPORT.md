# CLOUD_REPORT: surface-tag (theseus-slvl.3)

Branch `cloud/20261013-surface-tag`; one step, one code commit (`6bfbea9`) on the task commit (`e354320`).
Store format: main as cloned is **33**, not 32 as the brief said; this step makes it **34**.

## What I found

- The model is never told which surface a message came from. A person's input node records only `author` (the
  connection's label: `sock#3`, `web#1`, `discord:<name>`), which cannot tell `theseus ask` from the TUI, a DM from a
  channel, or a channel's class, and a place can move after the node is written. So the tag cannot be derived at
  compile time from what the node holds: the surface has to be recorded when the node is written (a stored field,
  hence the format bump).
- `theseus history`, the TUI (it renders history through the same `render::node_lines`) and the cockpit printed the
  bare model id for a reply.
- Every stand-in model in the tests (`tests_external`'s `asked`, `theseusd/tests/common/model.rs`, theseus-sim's
  `fake_model::last_user_text`) matches the person's prompt with `starts_with`; the tag at the head of the text breaks
  them exactly as the task view would, so they strip it the way they already skip the task view (new
  `surface::strip_tag`).

## What I changed (`6bfbea9`)

1. **The tag** (`crates/theseus-core/src/surface.rs`, new). `Node.surface: Option<String>` (skipped when absent, so
   stored nodes keep their bytes; **store format 34**). The code is settled by `Core::surface_of` in `turn.submit` from
   the listener's `approval::Surface`, the client's own word (new optional `TurnSubmitParams.via`: `ask`, `tui`,
   `voice`; anything else reads as none), and on Discord the session's place (`outbox.try_target`) and its class
   (`PlaceRule`). It rides the turn's `EventSink.surface` (new field, builder `from_surface`) into `Turn::user_node`
   (`turn/surface_step.rs`) and a steered message's `Steered.surface`. The compiler's `user_blocks` calls
   `surface::lead`, which puts `[<words>] ` at the head of the message's text block (a text block of the tag alone for
   a message of attachments only). Codes and words, one table (`surface::tag`):

   | code | tag |
   |---|---|
   | `terminal:ask` | `[Terminal · theseus ask]` |
   | `terminal:tui` | `[Terminal · TUI]` |
   | `terminal` (any other CLI client) | `[Terminal]` |
   | `discord:dm` | `[Discord DM · phone or desktop; markdown renders]` |
   | `discord:channel:<shared\|private>:#name` | `[Discord #name · shared]` / `· private]` |
   | `discord` (any other Discord place) | `[Discord]` |
   | `cockpit` | `[Cockpit]` |
   | `mcp` | `[MCP · another program reads this]` |
   | `voice` | none (recorded, rendered with no tag; the framing line stays) |

   A connection no listener named (tests') gets no code and no tag. A channel name is cut to letters, digits and `-_.`
   (40 characters), so a name can never close the bracket or add a line. A tag is fixed when its node is written, so a
   session whose place moves keeps each earlier message's tag and the cached prefix holds. Only a person's own
   message (`Origin::Operator`) is tagged; a wake, a report, a task brief or an MCP prompt message has no code.
   The `via` word is a label for the model's register, never an authority (the field's doc says so).
2. **One factual sentence** at the end of `turn::ASSEMBLY`: "A person's message may start with a bracketed tag that
   names where they wrote from." About 17 tokens, once, in the cached shared header (one recompile per session it
   reaches). No instruction about style.
3. **"Theseus" on replies**: `theseus_protocol::surface::{REPLY_NAME, reply_label}` (`Theseus (sonnet-5-5)`; a
   `claude-` prefix is dropped), used by `render::node_lines` (so `theseus history` and the TUI), and the cockpit's
   `lib/speaker.ts` (`replySpeaker`) in the transcript's reply card ("Theseus" with the model beside it). `theseus ask`'s
   stdout is unchanged (the bare reply). The persona's `name`/`emoji` are not joined (persona-data has not): the literal
   stays until `persona_for` exists; `reply_label` and `replySpeaker` are the two places to change.
4. Voice: `theseus-discord`'s utterance turn sends `via: "voice"`; its `FRAMING` line is untouched.
5. Plumbing: `via: None` at the other `TurnSubmitParams` sites; `theseus ask` sends `via: "ask"`, the TUI `"tui"`;
   `cockpit/src/protocol.gen/TurnSubmitParams.ts` regenerated; `crates/theseus-core/AGENTS.md` has a bullet.
6. Tests updated for the new bytes: the CLI history goldens (4 files; the label), `theseus/tests/cite.rs`, the TUI's
   history and cite tests, Discord's PDF test (the typed message is tagged), `theseus-exam`'s arms test (the request's
   text ends with the task), `theseusd/tests/fork.rs` and `cache_header.rs` (they read raw request text), `versions.rs`
   (a "newer" store is now format 35), the layouts' format-33 node sample, and the format 33 -> 34 literals.

**Cost** (est. at 3.5 characters a token; no tokenizer here): the header sentence ~17 tokens once per cache entry;
a tag ~3 to 14 tokens a turn: `[Cockpit]` 3, `[Terminal · TUI]` 5, `[Terminal · theseus ask]` 8,
`[Discord #lab · shared]` 8, `[MCP · another program reads this]` 9, `[Discord DM · phone or desktop; markdown renders]` 14.
The brief's "10 to 20" holds for the longer ones; the short ones are below it because the words are short. No call, no
wait, no timer, no extra frame: the tag is read from the sink the turn already holds.

## How I proved it

Tests (nextest, `CARGO_INCREMENTAL=0`):

- `rpc::tests_surface` (3 tests, through `serve_connection` with a real listener surface each): the same prompt from
  `ask`, the TUI, a Discord DM, a shared Discord channel, the cockpit and an MCP client reaches the (recording) model
  with its own tag; the system blocks are identical across the five private-place surfaces, and the shared channel's
  system equals an untagged message's in the same shared place (a shared place's tools note differs by design); the
  node's stored text is as typed and its code is recorded; a voice turn keeps its framing, untagged; an unnamed
  connection and an unknown `via` get none/`[Terminal]`; a session that moves from a DM to a channel keeps the DM tag on
  its first message and the channel tag on its second.
- `surface::tests` (5): the table, the voice/unnamed rules, a hostile channel name, `lead` (text, attachments only,
  a relayed message), `strip_tag` over every code.
- `protocol::surface::tests` (the label), `cockpit/test/speaker.test.ts` (2), the TUI's updated history test, the CLI
  goldens, the layouts' format-33 sample and a new `a_user_message_records_its_surface_and_an_old_one_has_none`.
- The plain-turn frame budget: `bench turn --check` (below) and the suite's `a_plain_turn_stays_within_its_frame_budget`
  pass.

**Planted reverts** (restored and `touch`ed after each; `git status` clean):

| plant | caught by |
|---|---|
| the tag also written into the header (`stand` prepends it to the second system block) | `each_surface_is_told_where_the_person_wrote_from_and_the_header_is_the_same`: "the tag is not in the shared header" |
| the tag stored in the node's text (`Turn::user_node` builds the text with it) | the same test: the rendered text doubled the tag (`left: "[Terminal · theseus ask] [Terminal · theseus ask] quick one…"`), and the stored-text assertion after it |
| `tag("voice")` returns words | `surface::tests::a_voice_turn_is_recorded_and_not_tagged_…` and `tests_surface::a_voice_turn_keeps_its_framing_alone_…` (`left: "[Voice call] [Voice call: they hear your reply.]…"`) |
| the cockpit transcript reverted to the bare model id | `cockpit/test/speaker.test.ts` test 2 |
| `reply_label` (history) | the four CLI history goldens and the TUI/cite tests failed before I rewrote their expectations |

A first plant of the stored-text bug did not apply (rustfmt had re-wrapped the line I matched) and the suite passed; I
re-planted against the real text and it failed as above. Recorded because the first run proved nothing.

**Live check** (scratch daemons on the stand-in model, this VM; all processes stopped by pid):

```bash
# 1. scratch daemon A (config: file: key, [model] api_base = the fake, [mcp_server] on at 7435, [web] port 7434)
target/debug/theseus-sim fake-model --addr 127.0.0.1:9461 --rules /tmp/slvl/rules.json --dump /tmp/slvl/dump &
target/debug/theseusd --config /tmp/slvl/config.toml --socket /tmp/slvl/theseus.sock --state-dir /tmp/slvl/state &
theseus --socket /tmp/slvl/theseus.sock ask "quick one: is the tide in?"      # stdout: the bare reply
python3 /tmp/slvl/rpc.py /tmp/slvl/theseus.sock tui "quick one: is the tide in?"   # raw socket, via=tui
python3 /tmp/slvl/rpc.py /tmp/slvl/theseus.sock -   "quick one: is the tide in?"   # raw socket, no via
node /tmp/slvl/ws.mjs                                                          # the cockpit's websocket
# MCP: initialize, conversation_open, conversation_send over curl to :7435/mcp with the bearer key
# 2. scratch daemon B from `theseus-sim discord rig` (fake-discord, bindings: a DM and a private #lab), then
theseus-sim discord say --user 900000000000000101 --name ana "quick one: is the tide in?"                         # the DM
theseus-sim discord say --channel 900000000000000010 --user 900000000000000101 --name ana "quick one: is the tide in?"
```

What the fake model's request dumps (`/tmp/slvl/dump/request-00N.json`) held, last user message's head:

| request | head |
|---|---|
| 000 CLI `ask` | `[Terminal · theseus ask] quick one: is the tide in?` |
| 001 socket, `via=tui` | `[Terminal · TUI] …` |
| 002 socket, no via | `[Terminal] …` |
| 003 cockpit websocket | `[Cockpit] …` |
| 004 MCP `conversation_send` | `[MCP · another program reads this] …` |
| 005 fake Discord DM | `[Discord DM · phone or desktop; markdown renders] …` |
| 006 fake Discord #lab | `[Discord #lab · private] …` |

System blocks: on daemon A, block 1 (the header) is byte-identical in 000 to 004 (sha256 prefix `2e64061b14`); block 2 is
identical in 001 to 004, and 000's differs only because `theseus ask` sends its own `--dir`, so its "Directory:" block
differs (not the tag). On daemon B, requests 005 to 008 (DM, #lab, CLI, TUI on the same daemon) have identical system
blocks (`acfb1c4cca`, `65dd998dc2`). Replies' stdout from `theseus ask` was the bare reply. The maintainer should repeat
it with the owner's real config only if wanted; the commands above are the whole check on a stand-in.

Not run live: a real model's reading of the tag (no model here), the TUI in tmux (its history label is a unit test), a
voice call (`via: "voice"` is proved in-process; `FRAMING` is untouched).

**FAST** (debug build, this 4-core VM): `bench turn --check`: `frames_plain 4 (budget 4)`, `frames_tool 8 (8)`,
`syncs_plain 2 (2)`, `frames_batch 14 (14)`, `first_request_kb 38.65 (40)`, every line ok. `bench lifecycle --runs
10 --check`, run twice: cold start p50 24.0 ms (50), config-copy start 25.8 (50), clean shutdown 11.7 (100), SIGKILL
restart 31.2 (150), swap 30.8 (200), push seed 1.1 (250), cancel 12.8 (100) all ok; **"health, after the seed, each call
its own connection" p50 4.8 ms, p95 7.6 ms against a 5 ms budget MISSED in both runs**. Health and the seed are
untouched by this step (the step adds no field to health, no frame, no start-path work); the budget is the owner's
machine's and this is a 4-core VM debug build, so I did not touch it, and I could not rerun the base build for this phase
in the time left. Treat it as the VM, but the maintainer's gate on the owner's machine is the proof.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`, the
disk here filled twice otherwise) passes door, keel, keel tests, fmt, shape, features, clippy (no findings), cockpit
(lint, tests, build), test build, and the reader rule; the suite: **4483 tests, 4449 passed, 34 failed**:

- 33 are the known L1 failures (20 in `theseus-sandbox`, 13 in `theseusd`'s sandbox tests: a root daemon with no job
  cgroup, theseus-pv6i).
- 1 is `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: it **fails the
  same way on the base commit** (`e354320`, built and run alone in a scratch worktree: the scripted answer arrives in
  under a second here, so the held "thinking" create never happens and `pending > 0` never holds; the log shows the
  folded `-# 💭 thought for 1 s` and the answer). Not in the brief's list; unrelated to this step; the fast-host order
  the brief mentions for theseus-wpv2g looks like the same cause. Left as is.

I ran the phases after the suite myself: `cargo deny --offline check` ok; generated TypeScript matches the commit (the
suite regenerated `TurnSubmitParams.ts`, committed); `bench turn --check` ok; lifecycle as above. The jobs bench's L1 row
needs L1 and was not run. The gate was not rerun after the report commit (it changes no code).

**Keel findings expected**: none. `scripts/keel-guard.py`: `keel: ok (a branch: merge-base(HEAD, origin/main)..HEAD,
9fa84735c0; 435 files changed, 0 findings acked)`, with one informational line (a checkpoint test renamed and rewritten by
another branch). I rewrote assertions, never deleted one: the history expectations (`glm-x:` -> `Theseus (glm-x):`),
the Discord PDF text, `fork.rs`'s three, `versions.rs`'s newer-format literal.

## Left, uncertain, for the owner

- **The persona's name and emoji** are not wired: the label is the literal "Theseus" in `theseus_protocol::surface` and
  `cockpit/src/lib/speaker.ts`. When `persona_for` exists, change those two.
- **Tag placement**: I put the tag at the head of the message's own text (`[Terminal · TUI] quick one…`), one block, not a
  block of its own, because a block of its own costs a provider block per turn. Revisit if the persona's voice section
  wants it on its own line.
- **A moved place** keeps its old messages' tags (decided: the cached prefix beats accuracy about the past).
  A session that changes surface mid-conversation therefore shows both tags in its history, which is true.
- **TUI live replies** stream without a label line (they never had one); the history and a reload show "Theseus (model)".
  The TUI's own input line is `via: "tui"`; `theseus watch --interactive` and other CLI turn senders are `[Terminal]`.
- **A newer stand-in** that reads the person's text with `starts_with` must strip the tag (`theseus_core::surface::strip_tag`);
  three do now. A future one that does not will see a prefix.
- **Docs for the maintainer to write**: Part III item for this step (store format 34, `Node.surface`, `via`, the tag
  table, the ASSEMBLY sentence), `docs/status.md`, and the spec's text of the assembly paragraph (one sentence longer).
  The persona-voice section may name the tag words from `surface::tag`.
- Disk note for anyone running this on a cloud VM: the default debug profile filled a 40 GB allowance; use
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.
