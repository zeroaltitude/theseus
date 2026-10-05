# theseus-discord

The Discord binding (spec P5, M3): text and voice channels in one or more guilds, and direct messages, in the
daemon's process. Read by theseusd.

Key modules: `runtime.rs`, `courier.rs`, `render.rs`. Read by: theseusd.

## What's here

- `src/runtime.rs`: the gateway loop and the places (a text channel or a DM, each backed by one session), with the
  slash commands and the confirm buttons. `Routes::resolve` finds a message's or an interaction's place.
- `src/courier.rs`: durable delivery, the binding's side: one lane per place, and one for the operator's notices.
- `src/render.rs`: a session's events as Discord messages. Pure: events in, messages out.
- `src/bindings.rs` (the bindings file; `bindings.example.toml` is its format), `src/files.rs` (attachments),
  `src/viewers.rs` (who can view a channel), and `src/rpc_client.rs` (the in-process protocol connection).
- **Places** (the place rule, theseus-nbsh): a `[[channel]]` with `private = true` is a private place (its session
  gets everything); any other guild channel is shared (the public tools alone). The binding tells the core its places
  as it starts (`Core::bind_places`), before it reads a message, and reads each private channel's viewers once, after
  the gateway connects (`check_private`), so health warns when anyone besides the owner can view it. It reads no
  viewers before a turn or a post, and loops stream everywhere. One walk of a channel's viewers (`runtime/audience.rs`,
  `view`) serves that read and the approval check (theseus-sgh). `/publish` (`runtime/publish.rs`) goes to the core's
  `place.publish` as the presser, which the core judges: only the owner, from a private place.
  `/prompt` (`runtime/prompt.rs`, M7 36c): `name:<server/prompt>` with autocomplete from `mcp.prompt.list` (25 choices
  at most), then a modal with an input per argument (up to 5; more is one `args` input of `name=value` lines), whose
  submit is `turn.submit { prompt }` through the place's line of turns. The core refuses a shared place's prompt.
  `/extensions` (`runtime/extensions.rs`, M7 43b): the loaded extensions from `extend.list`, ephemeral, each with a
  Revoke button (`ext-revoke:<name>`); the text and a press go through the place (`Control::Extensions`,
  `Control::Revoke`), a press as `extension.revoke` with the presser's ids, which the core judges.
  Jev's live notices (`runtime/jev.rs`, theseus-0j2.13): the operator lane posts a `jev_notice` to the owner's DM
  alone (`Lane::owner_dm`: never `to_operator`'s fallback, which may be a shared place) with right / wrong / noise,
  each a `judge.label` with the presser's `DiscordOrigin`, which the core judges (the owner, from a private place).
  A refused press tells the presser alone; a counted one comes back as the core's `jev_labeled` post, which edits
  the notice and clears its buttons. `judge.noticed` draws nothing in a place.
- **Guilds and ceilings** (step 38a, theseus-ext.3): `bindings.rs` reads format 1 (a top-level `guild_id`, its
  `private` beside it) and format 2 (a `[[guild]]` each, with its own `private`, and each `[[channel]]` naming its
  `guild`); a file that mixes them is refused, naming the line. `runtime/guilds.rs` tells the core each guild's word
  (`Core::trust_guilds`) and each place's guild and ceiling (`BoundPlace`), keeps them for health and the
  `discord.bound` row (`PlaceBits`), reads the bot's roles in every guild, and gives each place's session its spend
  limit at each start (`Core::place_spend`). Routing needs no guild: a channel id is unique across guilds. Slash
  commands stay global. A voice channel may be in any bound guild; its call joins in that guild.

## Invariants

- **What a person does goes through the protocol** (a message is `turn.submit`, a press `action.confirm`, `/stop`
  `execution.stop`, `/trust` `policy.trust`), so the core judges Discord as it judges the CLI and the web UI. What
  the binding delivers and reports, it reads and writes in the core directly: the outbox, the cards' questions, its
  ledger rows (Item 30).
- **What must be seen is an outbox post**, written by the core when it happens: a reply, a card, a card's settle, a
  failed turn. Live progress (streamed text, tool lines, typing) is best effort and never replayed (Item 6).
- **The reply shows before its frames are synced** (theseus-ck0n): a loop's first text goes out at once, the rest of
  its stream at the edit tick, and its whole text the moment its stream ends (`model.answered`, which the core sends
  before the settle's frame), all live ops under the post's keys, so the post, durable and exactly once, only seals
  them and adds the footer. A create's `discord.message.out` row goes through `Core::binding_ledger_soon`, so the
  lane's next write never waits on that row's sync.
- **One lane per place is the only writer of its messages**: posts first, in order, then live progress. A create
  carries a nonce from its message's key, with `enforce_nonce`, so a retry after a crash returns the first message.
- **A place answers only where its bindings file binds it.** An interaction in an unbound place gets no answer, so
  daemons on one bot token with disjoint bindings each answer their own places (Item 11). A card in a guild channel
  mentions exactly its answerers, and nothing else mentions anyone (Item 15).
- **Slash commands are bare names** (`/new`, `/stop`, `/trust`, …), and each control has one effect (Item 9).

## Tests

- `src/tests_outbox.rs` drives the binding against `theseus_sim::fake_discord` (REST: it honours a nonce as
  Discord does, and can be down, hang creates, or fail). Point a daemon at it with `[discord] rest_proxy` and
  `gateway_proxy`.
- `src/tests_gateway.rs` drives it through the stand-in's gateway too (theseus-6g62): `FakeDiscord::say` types a
  message as a user, and `press` presses a button the binding posted, each sent as Discord sends it; `replies()`
  is what the binding answered each press, and each message keeps every version (theseus-qifw). A guild set on
  the fake (`set_guild`) answers the viewer check, so a card in a trusted channel is tested end to end
  (theseus-ck0k). Its core starts the continuation driver, as the daemon does, or an approved call never runs.
- `theseus-sim discord proof --theseusd <bin>` runs a typed message, a card, a refused press, and an Approve
  against a real daemon on the stand-ins in about 3 s; theseusd's `tests/discord_proof.rs` runs it in the gate.
  A step that changes the binding uses it as its live check. Slash commands, select menus, attachments, and a
  dropped gateway are not driven through the stand-in yet: tests drive those through `on_interaction` and
  `place_for_tests`.
- `split_text` (`src/render.rs`) has property tests: a message split past Discord's 2,000-character limit must
  never loop or panic.

## Traps

- **Never bind the operator's places from a second daemon.** A scratch daemon on Discord binds only a test channel,
  runs on a fresh state dir (never a copy of the operator's store, theseus-c3e), and carries no copy of the
  operator's bindings file.
- Registering commands is global to the bot: a scratch daemon's command list replaces the installed one's until the
  operator's daemon next starts.
- A test that reads the channel waits for every message it reads: the outbox draining doesn't mean the live tool
  line has landed.
- A ledger row names a Discord id as a string. Clippy's `cmp_owned` turns `row["author_id"] == ID.to_string()`
  into `== ID`, a JSON string against a number that never matches: bind the string first.
