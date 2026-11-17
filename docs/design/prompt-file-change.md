# The owner asks Theseus to change a prompt file, or a setting too long for a card (issue 23)

A second kind of change through issue 22's door (`config-change.md`), not a second door: the same wall, the same
question (`config.ack`), the same second proof, the same kept copies, history and undo. What the daemon changes here
is a file the settings name as **prompt text**, and what the owner sees is built from that file itself. Code:
`crates/theseus-core/src/config_change/prompt_file/` (the tools, the card, the write, undo, history),
`config_change/shown.rs` (the difference and the whole text), `config_change/show.rs` (`config.show`),
`crates/theseus/src/config_cmd.rs` (`theseus config show`), the cockpit's `ConfirmCard.tsx`.

## What prompt text is, and when it is read

- The system level is `[context] files`; a persona's files are the ontology's (`personas.rs`), not the config's.
  `Config::context_paths(book, persona, engaging)` joins them. An entry is a path or `{ path, readers }`.
- **Each request reads them again** (`context_files.rs`: a stat per file, a read when its size, mtime or inode changed
  or it is under two seconds old). So a change takes effect with the **next message**; nothing restarts, and the card
  says so. Only the first 64 KiB of a file reach a prompt (`MAX_BYTES`); the card says so when a new text is longer.
- A persona's soul is also capped by `[context] persona_max_tokens`: a change may not grow a persona's file past it.

## The tools (`config.propose_file`, `config.read_file`)

Both are the owner's own turn's, behind issue 22's wall (`wall.rs`): a private place only the owner writes in, their
own session, never a task, a job, the self loop, an MCP client's session or a shared place. A call from any of those
is refused in words that name the tool, and there is no way to name it from outside a turn.

- `config.read_file { file? }` lists the files that can be changed, or gives one whole and exactly, so a change can be
  proposed with exact replacements. It gives the owner's own turn any named file, a persona's that is not in play
  included: that turn is the owner's alone, and the files are already in its prompt where a message can engage
  another persona.
- `config.propose_file { file, text | replace: [{old, new}], reason }`. The model never writes a "before", a count, a
  fingerprint or a line of the card. `replace` is applied by the daemon to the file as it is now: each `old` must be
  found **exactly once** (an occurrence that overlaps another counts), no two replacements may touch the same text,
  at most 40. The whole `text` is also accepted.

## Which file (`prompt_file::named`, `resolve`)

A request is accepted only when all of these hold:

1. After the settings' own expansion (`config::expand`: `~`, `$VAR`), it is absolute, with no `..` and no `.`.
2. It is, written that way, the path a named file is spelled as or its canonical path. A second name for the same
   file (a link, a hard link, another case, a path through a mount) is not the file.
3. `lstat` says a regular file: no link at its end, never a directory.
4. It is not on the floor (`ToolRuntime::ctx.floor`: the settings file and its copies, the bindings file, the
   authenticator's file, the stores, the token file, the sealed sets) and not a secret kept in a file
   (`[secrets] … = "file:…"`), whatever the settings name.
5. It is a text file of at most 256 KiB (`MAX_FILE_BYTES`), before and after.

The yes checks all of it again.

## The card (`prompt_file/card.rs`, `shown.rs`)

Built from the two texts alone. It names the file and which level names it, says each text's size, lines and
**fingerprint** (SHA-256, the first 16 hex digits, as a context file's digest in the prompt's manifest), counts lines
added and removed, and shows **the changed lines themselves, whole, with two lines of context**, in a fenced block, when
the whole card fits `CARD_CHARS` (1,450: what a Discord message carries besides its heading, the second proof's clause
and its footer). When they do not fit, or a line holds a triple backtick, the card says "too long to show on a card
whole, so this is a summary and not the text", gives the line ranges, and names `theseus config show`. It never presents
cut text as whole. Unseen characters (controls, separators,
zero-width, direction marks, hidden tag and variation characters: `shown::unseen`) are written `\u{..}` and counted on
the card and in the whole text; a backslash is doubled. The model's reason is shown as the model's, one line, with its
unseen characters written out. A card says when the turn read outside text since the owner's last message.

## The whole text (`config.show`, `theseus config show`)

While the question waits, the daemon keeps the file as the card was built from it and the text it would become, by their
digests, in `<state>/store/config-changes/` (on the floor). `config.show` (owner only, judged as `config.history`) gives
both, every line, and says whether the file has changed since the card. The cockpit's card has a button for it. **The
Discord binding has no outbound file upload**: there the card is the changed lines or the summary with the command.

## The yes (`prompt_file/write.rs`)

`action.confirm` on the `config.ack` question, judged by the place rule as for any config card, **with the second
proof** (`Needs::Proof`: the same window rule as a sensitive setting; nothing stricter, nothing looser). The daemon
re-reads the file; one whose digest is not the card's is not written. It keeps the text it replaces as
`previous-NNNN.txt` (the last ten, numbered from the same count as the settings' copies, so one number names one
change), then writes the new text in one step: a temporary file in the same directory, synced, the old permissions,
renamed over it, the directory synced. The ledger's `config.changed` row carries the file, its two digests and both
sizes. No restart marker is written.

## History and undo

`config.history` lists a file change with its path, sizes, fingerprints, lines added and removed, who asked, who
approved and from where. `config.undo` on it builds the same card (the file as it is now against the kept text before)
and puts the text before back **exactly**, by the same proof; if the file changed again since, the card says so and
shows those edits as the lines it removes.

## A setting too long for a card (line 2)

`config.propose` used to refuse a value whose shown form is over 400 characters (`diff::SHOWN`). Neither limit moved:
a value between 400 characters and the proposal's 4,096 bytes (`MAX_VALUE_BYTES`) now goes on the same kind of card
(`config_change/long_value.rs`): size and fingerprint of the value before and after, its changed lines whole when they
fit, else a summary, and the whole value readable by `config.show`. A short value keeps today's card exactly.

## What the floor holds

The floor does not name the files a config's prompt text lives in (see `docs/settings.md`, "What this does not protect
against"): a file tool that writes one is judged by its posture, as outside the project folder. Naming them is an edit
to `toolrun/guarded.rs`, a keel file, and is the owner's to sign.
