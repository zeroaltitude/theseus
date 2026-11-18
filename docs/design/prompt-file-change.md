# The owner asks Theseus to change a prompt file, or a setting too long for a card (issue 23)

A second kind of change through issue 22's door (`config-change.md`), not a second door: the same wall, the same
question (`config.ack`), the same second proof, the same kept copies, history and undo. What the daemon changes here
is a file the settings name as **prompt text**, and what the owner sees is built from that file itself. Code:
`crates/theseus-core/src/config_change/prompt_file/` (the tools, the card, the write, undo, history),
`config_change/shown.rs` (the difference and the whole text), `config_change/words.rs` (the line a change settles into, shared with issue 22), `config_change/whole.rs` (`config.whole`),
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
4. It is not on the floor (`floor_without_prompt_files`: the settings file and its copies, the bindings file, the
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

## One card, drawn once (issue 22's rules, issue 23's cards)

A prompt file's change, its undo and a long setting's change are **settings questions** in issue 22's sense
(`config-change.md`, "A settings question"): the daemon words each once, from the facts the card was built from.
`PlainWords::detail` is the card whole, as `prompt_file/card.rs` or `card.rs` built it, and `PlainWords::what` is one
line (`words::file`: which file and how many lines it adds and removes, no restart; `words::change` for a long setting).
The model's reason is on the card as the model's and never in the line. A surface knows the kind by the `detail`, not by a
tool name. So every place draws it with the settings form or says there is no yes there, and none has a second form of
its own:

| Place | Drawn by | What it shows |
|---|---|---|
| Discord card | `render/settings.rs` | the card whole; the block of changed lines as it is, everything else as text (`md`); several messages when longer, the buttons on the last |
| Discord `/answer`, Details | `settings::words_here`, `details` | the card whole when one message holds it; else where to read it, and no buttons |
| Discord needs-you post | `settings::needs_you` (the core's card body carries the words) | the card whole, in parts; `[notify] dm_cards = false` gives a line (what it changes) and says to open Theseus, never a cut card |
| cockpit card, queue, bulk approve | `ConfirmCard.tsx`, `ops/queue.ts` | the card whole with its breaks; "Read the whole text" for before and after; no compact approve, never in a batch |
| terminal board card | `theseus-tui/src/card.rs` | no part of the card: where to read it and `theseus config show`; `y` sends nothing |
| terminal chat | `chat/ask.rs` | the dock says to press `v`; `v` writes every line of the card as a row of its own (spaces kept); `y` after |
| `theseus answer`, the event print | `theseus/src/render/ask.rs` | the change in a line, the card whole, Yes and No |
| herdr answer pane | `herdr_answer.rs` | the change in a line, the card whole |
| the model's answer tool | `words::by_press_only` | only a no is offered; a yes is refused (below) |
| voice | none | no card is read or offered |

**A yes is the owner's own press.** `judge_typed_settings` (`rpc/mcp_card.rs`) refuses a yes typed for a question whose
tool is `config.ack`, the one tool all of these kinds ask through, and not any tool name of issue 23's: so a new kind
that asks through the same question is held without a new line. `prompt_file/tests_answer.rs` drives the answer tool
against a prompt file's change, its undo and a long setting's change: each still waits, a no works.

## The whole text (`config.whole`, `theseus config show`)

While the question waits, the daemon keeps the file as the card was built from it and the text it would become, by their
digests, in `<state>/store/config-changes/` (on the floor). `config.whole` (owner only, judged as `config.history`) gives
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
fit, else a summary, and the whole value readable by `config.whole`. A short value keeps today's card exactly.

## What the floor holds (the floor commit)

The prompt files are on the floor, `toolrun/guarded.rs` (a keel file: this is the owner's to sign), so no file tool,
no shell naming one and no L1 job reaches them. It holds a path a call NAMES, as it does for the settings file and the
stores. A search over a directory that holds one, a shell glob (`sed -i dir/*.md`) and a parent directory moved or copied
whole name no protected file, so they are not stopped; that is the floor's own model, not this commit's:

- `prompt_static`: every file `[context] files` names, and both default homes of a soul, whole: `<state>/personas/`
  (`personas/soul.rs: soul_home`) and `<state>/persona/`, the older singular home some machines still have.
- `prompt_souls`: every path `Config::context_paths` returns for the book with every persona engaged: a soul the
  ontology puts anywhere. The book changes while Theseus runs and the floor's lists are built once at a start, so
  `toolrun/prompt_floor.rs` keeps these and reads the book again before each question the floor answers (a comparison
  of the book's address when it is the one it knew); the gate, `policy.explain`, the tools' own context (`ctx_in`) and
  L1's views (`Sandbox::hide`, which only adds) all ask it, so none can disagree.
- `beside`: a file of the same directory named as one of those with a dot and more after it, which is what `persona.soul`
  keeps (`soul.md.<ms>.bak`) and leaves (`soul.md.new`).

Not on the gate: the daemon's own readers and writers (the context compile, `persona.soul`, the card's write, the import
and the persona migration), which are code and not calls, and `config.read_file`, which the owner's own turn uses. A
tool's read of a prompt file waits. A search over a directory that holds one is not stopped (the call names the directory,
as for the rest of the floor); `git` tools skip a floor path they walk. `forbidden()` in `prompt_file/mod.rs` reads the
floor without these entries (`floor_without_prompt_files`), or no named file could be changed.
