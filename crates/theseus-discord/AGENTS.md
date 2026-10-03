# theseus-discord

The Discord binding (spec P5, M3): one guild's text channels and direct messages, in the daemon's process. Read by
theseusd.

Key modules: `runtime.rs`, `courier.rs`, `render.rs`. Read by: theseusd.

## What's here

- `src/runtime.rs`: the gateway loop and the places (a text channel or a DM, each backed by one session), with the
  slash commands and the confirm buttons. `Routes::resolve` finds a message's or an interaction's place.
- `src/courier.rs`: durable delivery, the binding's side: one lane per place, and one for the operator's notices.
- `src/render_cred.rs` (M4 18d): an L1 job's credential request granted at notify posts its 🔑 notice in the job's
  place; one that waits is a card, which the outbox posts.
- `src/render.rs`: a session's events as Discord messages. Pure: events in, messages out.
- `src/bindings.rs` (the bindings file; `bindings.example.toml` is its format), `src/files.rs` (attachments),
  `src/viewers.rs` (who can view a channel), and `src/rpc_client.rs` (the in-process protocol connection).
- **The audience** (M4 19a): one walk of a guild channel's viewers serves the approval check and the labels. The
  binding tells the core (`Core::place_viewers`) every bound guild channel's viewers at connect, on a channel or role
  change, and before a turn there when the last read is a minute old. Without the Server Members intent it says they
  cannot be read, and the channel counts as public. It never asks for the intent on the gateway.
- **The check at post time** (M4 19c): a lane posts a reply or a task's report in a guild channel only after a
  fresh read of who can view it (`read_audience_now`) and the core's `check_post`, unless its readers fit any
  audience the channel can have. A report's readers (`post_readers`) are its last message's met with its
  brief's, since `render::report` shows the task's title, the brief's first line (theseus-jpff). A held post waits, and the posts after it wait; its card goes to the approvals DM
  from the operator's lane (`owner_card`); approved, it posts; declined, its place gets `courier::HELD_BACK`. A read
  Discord refuses counts as public. In a guild channel a loop whose request drew on restricted material is quiet
  (`context.compiled`'s `readers`): its text does not stream, and its tool line shows 🔒 for its input, so nothing
  of it reaches the channel before the check.

## Invariants

- **What a person does goes through the protocol** (a message is `turn.submit`, a press `action.confirm`, `/stop`
  `execution.stop`, `/trust` `policy.trust`), so the core judges Discord as it judges the CLI and the web UI. What
  the binding delivers and reports, it reads and writes in the core directly: the outbox, the cards' questions, its
  ledger rows (Item 30).
- **What must be seen is an outbox post**, written by the core when it happens: a reply, a card, a card's settle, a
  failed turn. Live progress (streamed text, tool lines, typing) is best effort and never replayed (Item 6).
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
