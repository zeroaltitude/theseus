# Undo, and the history of changes to Theseus's settings (issue 22)

Built on `config-change.md`: the owner asks, the daemon writes the settings file after a card and a yes, keeps the text
it replaced as a numbered copy (the last ten), and restarts with a way back. Code: `crates/theseus-core/src/config_change/`
(`history.rs`, `undo.rs`), the RPC in `rpc/config_history.rs`, the CLI in `crates/theseus/src/config_cmd.rs`.

## The history: `config.history`

The last changes, newest first, each with a number (the number of the copy kept when it was made, which is how a
change is named): when, each setting with its value before and after, where the owner asked, who approved and through
what, whether it needed a restart, and whether it was rolled back by the restart's way back or undone since.

It is **built by the daemon from what it keeps**, as the card is. The ledger's `config.changed` rows say which settings
and who (they never hold a value). The values come from the kept copies, and only from a copy whose digest is the one
the row recorded: a change's "before" is its own copy, and its "after" is the next copy that names the digest the row
recorded, or the file as it is now if that is the one. A change whose copy is gone or was altered is listed by its
settings with a line that says so, and cannot be undone. Nothing else supplies a value.

Who reads it: the tool `config.history` in the owner's own conversation (the same wall as `config.propose`,
`wall.rs`: a private place only the owner writes in, not a task, an execution whose principal is the operator's), and
the RPC `config.history`, judged as an act (`Act::Settings`, the place rule: the owner, from a private place). The CLI
is `theseus config history` and is refused inside a job. The MCP server does not offer the RPC, and
`routine::grant::NEVER` names both new tools. The words carry settings and the names of secrets, which is why none of
this is open to a job, a task, the self loop, an MCP client or a shared place.

## The undo: `config.undo { change?, reason? }`

`change` is a number from the history (default: the newest). The undo is a `config.propose` in disguise, and goes
through the same code (`config_change::put`): the candidate, its check that the whole file still loads, the card, the
sensitive list and the proof seam, the question, the yes, the daemon's atomic write, the numbered copy, the marker, the
restart and the way back. Nothing here writes a file.

**It is not "write the old file back".** `undo.rs` reads the file from before the change only for the values of the
settings that change touched, and turns them into `Set` and `Remove` changes on the file as it is now. A setting the
change did not touch keeps its value now, so what was changed since, to other settings, stays. The card is built from
the two parsed files as every card is, so it shows exactly what will move and nothing else can.

- A setting that already holds its value from before the change is left out, and the card says so. If nothing is left,
  there is nothing to undo, and it says that.
- A setting that was changed again since is put back too, and the card says so: "it set X; it is now Y; this puts back
  Z". When what the change set is no longer kept, the card says that instead of guessing.
- A change already undone is refused ("undo #N to have it back"); undoing an undo is the way to redo.
- Undoing a sensitive change is a sensitive change: the same list, the same refusal until the second proof is wired in
  (`proof_seam.rs`), and the same card line after.
- A yes writes `config.changed` as every change does, and beside it `config.undone`, naming the change undone, the
  settings put back and the ones that had been changed again.

## The floor

`<state>/config-changes/` (the copies, the candidates and the restart's marker) is on the floor
(`toolrun/guarded.rs`), through the same `write::dir` the writer uses: no tool reads or writes a copy, and no L1 view
shows the directory. A copy is read by the daemon only: `history.rs`, `undo.rs` and the restart's way back. Nothing
copies one out of the state directory.
