# The owner asks Theseus to change a setting (issue 22)

The config file stays on the short list of paths no tool may write (`crates/theseus-core/src/toolrun/guarded.rs`),
because it sets every permission Theseus has. What this adds is a way for **the daemon** to change it, in its own
code, after the owner has seen exactly what changes and said yes. Code: `crates/theseus-core/src/config_change/`, the
sensitive list in `crates/theseus-core/src/config/sensitivity.rs`, and `crates/theseusd/src/change_back.rs`.

## Line 1: the owner asks, the daemon writes

1. **`config.propose`** (class `Run`, changes nothing by itself). Its input is a list of `{path, value | remove}` and a
   one-line reason in the owner's words: never text for the file. Offered to the owner's own turns only
   (`wall.rs`): a private place only the owner writes in, not a task's session, and an execution whose principal is
   the operator's (an MCP client's is `mcp`, the self loop's `self`, a job has none).
2. **The candidate** (`edit.rs`): the file as it is on disk, with the changes made on its own text so its comments and
   order stay. There is no format-preserving TOML editor in the workspace and none was added: a value is replaced at
   the span the `toml` crate gives, a key is added after its table's last entry, a table is added at the end, a key
   is removed with its line. The result is parsed again and must equal the original tree with the same changes
   applied as data, or the change is refused in words ("edit this setting in the file by hand"). Then
   `Config::parse` loads and checks it exactly as a start does; a candidate that does not load makes no card.
3. **The card** (`diff.rs`, `card.rs`): built by the daemon from the two parsed documents. Each setting that differs,
   its value before and after in words, **whole** (on and off, an escaped one-line literal for text, every item of a
   list), whether it needs a restart, a line when the turn has read outside text since the owner's last message (and a
   stronger one when Theseus's own check flagged it as steering), and the model's reason shown as the model's. A change
   that reaches more than 12 settings, or a value longer than 400 characters, is refused before a card: a card never
   cuts. The model has no field in which to write a "before", the schema is closed, and a text value cannot add a line
   to the card.
4. **The yes** is `action.confirm` on a question of its own, `config.ack`, judged by the place rule as an
   extension's ack is. The daemon re-reads the file: if its digest is not the one the candidate was built on, nothing
   is written. Else it keeps the text it replaces as a numbered copy
   (`<state>/store/config-changes/previous-NNNN.toml`, the last ten), then writes the candidate in one step
   (temporary file in the same directory, synced, the old
   permissions, rename, directory synced). It records `config.changed` with the paths (not the values), who asked
   (session, turn, place), who approved and through what, and the two digests. A decline, an expiry, or a refusal
   records `config.declined`. Candidates, copies and the marker are under `<state>/store/config-changes/`: the
   floor names the store directory, so a tool's read or write there waits for the owner as the store's own files
   do (the state directory itself is not on the floor, which is why they are not beside the store).
5. **The sensitive list** (`config/sensitivity.rs`): `[policy]`, `[secrets]`, `[sandbox]`, `[places]`, `[mcp_server]`,
   `[self]`, `[self_issues]`, `[aws]`, `[google]`, `[broker]`, `[server]`, `[content_based_security]`, `[judge]`,
   `[privacy]`, and by key name anywhere: a secret's source, an owner, a spend or budget, a folder or file, the
   sandbox, a command, an address (including a proxy), whether outside text keeps its label, text put into every
   prompt (`system`, a persona's `card`) and which provider sees the owner's text, which processes count as agents, where
   Theseus posts, and what it is told never to keep. Prompt text is also too long for a card, so it cannot be changed
   through this tool tonight; a file the owner names, shown on a card by its digest and length, is the likely later shape. A table in neither the sensitive list nor the ordinary one is treated as
   sensitive, and `tests_sensitivity` fails on it. A change is judged by the settings that really differ, as well
   as the paths named, so a table written whole, an array, or `mcp.servers.x` carrying an `env` is classified as
   what it reaches.
6. **The seam for the second proof** (`proof_seam.rs`): until `owner_proof` is built in, a change that touches any
   sensitive setting is refused before a card is made, with "this setting needs the second proof, which is not
   installed yet", and again on a yes. Wiring the proof in is one later commit.

Who answers: the yes is `action.confirm` judged by the place rule, and an agent's process on the CLI or the web socket
is refused at this card (its yes is recorded as `approval.refused`; only the owner's own hand counts).

## Line 2: the restart, with a way back

Nothing in the config is read live: every approved change restarts (`sensitivity::LIVE` is empty, with a test, and
a setting joins it only with the code that applies it). The restart is the existing one: `Core::stop`, then
`theseusd` execs its own image. Before the file changes, a marker, `pending.json`, is written beside the copies; the
exec carries its path in `THESEUS_CONFIG_CHANGE`.

- `daemon.restart` is the owner's plain word: the same wall, a card that says what a restart cuts and how many turns are
  running, the same marker and report-back, no change to the settings.
- It waits up to 120 s for a running turn to end, then cuts it (the card says so). A turn a stop interrupts resumes
  after it, as after any stop.
- The start that follows reads the marker (`restart::begin`). If the new settings do not load, `theseusd` puts the
  copy kept before the change back and restarts once more. If it loads, the daemon waits up to 120 s to be healthy:
  it serves its socket, its store is open, and the index tender is off or not `down` or `stalled`; then it tells
  the owner it is back, on which settings, and how long it took. Not healthy in time: the same way back.
- **Never twice, never a loop.** A marker passes `restarting`, `started`, then `confirmed` or `rolled_back`. A
  start that finds `started` found a start that never confirmed and goes back at once; a rolled-back marker is only
  ever told. So one change causes at most two restarts, onto it and back.
- **A rollback cannot be induced to restore what an attacker prefers**: it puts back only the copy whose digest the
  marker recorded, and only over a file whose digest is the one the change wrote. A later edit by the owner is
  never overwritten.
- Where a start looks for the marker: the path the exec carried, else the state directory this daemon serves from
  (`--state-dir`, else the settings file's `[server] state_dir`, else the default). That is the directory the floor
  holds; a marker anywhere else is never read, so a supervisor's restart after a crash finds its marker where a tool
  cannot have written one, and nowhere else (`change_back.rs`, with a test).

## The card in every place it is answered (the first live change)

The first live change showed the owner a card headed by the tool's name and its raw arguments, with the daemon's words
cut in the middle past 300 characters, and a settled line that read as the same machine text. The daemon had built the
card whole; the Discord binding drew it with its form for any tool's approval. So the card is now **worded once, by
the daemon, and read by every place** (`config_change/words.rs`):

- `PlainWords::what` is the change in one line, built from the facts the card was built from (each setting with its
  value before and after, the restart; the server's name for an attach or a removal; "restart"; the commit for an
  install), and never from the model's reason. It is kept in the question's arguments (`settled`), so a question asked
  before an upgrade still gets its first line.
- `PlainWords::detail` is the card whole. A surface that has it draws it in place of the raw call.
- A settings question is recognised by having `detail`, not by a tool's name.

| Place | Before | Now |
|---|---|---|
| Discord card | header of the tool's name and raw arguments; the words cut at 300 | the daemon's words first and whole, with Discord markdown escaped (a wildcard's `*` is drawn, not eaten); the proof's clause; the footer; past one message (1,900 characters) it goes as several, silent, each marked "(i/n)", the buttons on the last (`render/settings.rs`) |
| Discord settled line | the tool's name and raw arguments | "Approved by ... · change `x` from 25 to 26; Theseus then restarts" (`what`, escaped) |
| Discord needs-you post (the owner's DM; always the route of the install card) | one line cut at 300, with Approve under it | the same form as the card: the words whole, in parts, the proof's clause, when it expires, the buttons on the last |
| Discord `/answer` | the words cut at 400, with buttons | the words whole when one message holds them; else where to read them, and no buttons |
| Discord Details press | the summary cut at 300, the rule cut at 400 | the words whole (or where to read them) |
| cockpit card (ConfirmCard), Boundaries list, command palette | the call's raw preview; the list clamped the words to two lines | the change in words, the card whole with its line breaks, no raw call |
| cockpit Operations queue, compact row | `y · Approve` under a two-line clamp | no approve in the row; "read the whole card: it is answered there"; the whole card (⏎) answers it; a no is still there |
| cockpit bulk approve | listed with its summary, ticked by default | never in the batch: listed "asks one by one", with where it is answered, and not sent |
| terminal board card | `confirm config.ack: <raw>`, then the reason, cut to five rows a line | the head in words, and "the whole card is N lines, too long for this pane, so it is not answered here"; `y` sends nothing and says where; `n` declines |
| terminal chat | title raw, a risk line; `y` approved | the change in words; `y` is taken only after `v` has written the whole card into the transcript; the dock says so |
| `theseus answer`, the list | the gate's reason | the change in words, the card whole, Yes and No |
| `theseus answer`, the echo | `answered <id> · approve` | the same, then "approved: <what>" (or "declined: ...") |
| the model's answer tool | named `config.ack (…id)`; a yes was taken from it | named by the change in words; offered only a no for it; a yes is refused in words the model can pass on |
| voice | nothing: no card is read aloud and none is offered by voice | unchanged; a spoken yes reaches only the answer tool, which refuses it for a settings question |

**A settings question, and a call the floor holds, are answered only by the owner's own press or their own terminal.**
`chat.answer_question` carries the owner's words as typed (`Rides::typed`); `judge_typed_settings` in `rpc/mcp_card.rs`
refuses a yes that is typed for a settings question (`config.ack`, `self.install.ack`) or a call whose gate decision is
the floor, in the one place an answer becomes a decision, so it holds for any future caller. A yes for a floor hold
from an agent's process on the CLI or the web socket is refused too, as a settings card's is, and a call whose
transcript cannot be read counts as a floor hold. The tool says so before
asking (`words::by_press_only`) and is told, for such a question, that only a no is offered. A no from words changes
nothing and is allowed. Every other kind (a spend limit, a task's question, a question with options, a reminder, an
ordinary approval) is answered from chat as before.

A card longer than one Discord message is sent as several because the daemon's limits (12 settings, 400 characters a
value) make the longest card about 10,000 characters: 12 settings x (a 400-character value before and after, a path and
about 60 words of restart and proof) is five messages of 1,900. Lowering the limits to fit one message would have made
a list of hosts or a persona's text impossible to change at all.

## One card, not three approvals (the first live change)

Asked to change one ordinary setting, Theseus first looked at the settings file with the shell, twice, and each look was
held by the floor for the owner. Why: in a direct message the family `settings` is deferred (`PlaceKind::Dm.defaults()`),
so `proc.run` was loaded and `config.propose` was not; nothing told the model a setting's value; the standing prompt
says nothing about settings. The floor is untouched. What changed is that the look is no longer the shell's:

- **`config.show`** (`config_change/show.rs`) reads settings by name: the value now, whether the file sets it or it is the
  default, whether a change is sensitive (and why) and needs a fresh code, and whether it restarts. Built by the daemon
  from the file as it would start (parsed and checked, every default filled in), never the file's text. The wall is
  `config.history`'s (the owner alone, in a private place, in their own session). It needs no approval in a clean turn;
  a turn that has read outside text since the owner's last message is **asked** (`ask_if_outside` sets the plan's
  `authority`, which the gate asks for at every posture, as it does for a task's layer 1).
- **It never gives** anything in `[secrets]` or `[broker]`, a value under a key that names where a secret is read from
  (`sensitivity::key_reason`'s first class), any vault reference (`op://`) or URL that carries a user, a password or a
  query, or a value longer than a card shows whole (it says how long). It still says whether such a setting is set.
- **`config.propose`'s result** says each setting's value before, set or default, as the daemon read the file, so a plain
  change needs no look.
- The descriptions of both say that a setting is changed by proposing it, that the file is never opened, listed or
  searched, and that the shell is not the way; the `settings` family's summary line, which is what a deferred family is
  found by, says the same.
