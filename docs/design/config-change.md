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
   is written. Else it keeps the text it replaces as a numbered copy (`<state>/config-changes/previous-NNNN.toml`,
   the last ten), then writes the candidate in one step (temporary file in the same directory, synced, the old
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
- Limit: a start that no exec carried (a supervisor's restart after a crash) finds the marker only in the default
  state directory, so a daemon whose state directory is set in its settings file and not by a flag rolls back only
  in the exec'd start.
