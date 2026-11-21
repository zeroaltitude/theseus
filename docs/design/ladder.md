# The `did` lines and the ladder

Step 4 of the context refactor (issue 44): a one-line summary of each exchange, and the fold nodes of a place's
ladder. Records written beside the exchanges (`exchanges.md`), never in them. Nothing here is sent to a model that
answers a turn: the renderer reads it. The code is `crates/theseus-core/src/ladder/`.

## What is written

| Record | Key | Written | Holds |
|---|---|---|---|
| A `did` line | `did.<turn>` | once, after the exchange ends, by a background job | at most 200 characters on what Theseus did, found or decided; the model that wrote it and the writer's version; the exchange's marks; its cost |
| A fold node | `fold.<id>` | once, when its place's window or level is full | its level; the exchanges it stands for, by turn; the person's lines by node id; a title (at most 120 characters) and an account (at most 2,400); the union of its sources' marks; its handle, the id |
| A rung | `rung.<place>␁<level>␁<u64::MAX - last ms>␁<id>` | with its node | the node's id, so a level's newest nodes are the first keys of its prefix |
| The day's spend | `ladder.day` | with every call's record | the local day and what the summaries spent in it |

New META families only: no field on an existing record, no store-format step.

## The rules

- **A `did` line reads two things**: the person's message and the final reply, each cut by code at 4,000
  characters. Tool results are read only for the marks (outside text). An exchange with no line is shown as
  "reply not summarised" and its handle (`did::Said`).
- **Sealed and veiled sources are skipped**: a session of a sealed or seal-pending thread, and an imported
  conversation (whose labels the ladder does not read, so it fails closed). Their window still folds, by code:
  the node keeps the person's lines by reference and its handle, and says it is not summarised.
- **A fold is by count**, never by a judge or the clock. The window is the place's exchanges after its newest
  first-level node: at 2N (N = 20) ended exchanges, the oldest N become one node and the window is N again. A level
  of 2N standing nodes folds its oldest N into one node of the level above.
- **No model writes a level above the first.** Code builds it from its children's typed fields: their titles and
  handles, the person's lines that are corrections or made a standing record, and a count of the rest.
- **A node never changes once written**, and a correction rewrites none.
- **`/new`**: only what began at or after the place's latest `/new` counts: the start of the session its binding
  runs on now. A place no binding moves (the terminal) has none.
- **Money**: every call reserves its worst case on what is left of the day's line (a constant, `ladder::DAY_USD`,
  $2) and then on the daemon's day ceiling, and is settled at its price. The model is a background job's
  (`job_model`): Jev's pick for a synthesis over the owner's material, else `live`.

## When it runs

At an exchange's end, the point the card's observer and `people.v1` already use (`judge::categorize`), in a task
of its own: never a task's turn, never on the turn's path. It writes the line, then runs the place's fold step.
One summary job runs at a time. `theseus exchanges ladder <place> [--dry-run] [--cap-usd N]` builds a place's
ladder from its history: the missing lines, oldest first, then the folds. A dry run counts the exchanges, the
lines and the folds, prices them at their worst case, and writes nothing.

## Not yet

- The judge's answer that a message is a correction is not kept per message today, so above the first level only
  the lines that made a standing record are kept by name; the rest are counted.
- The renderer shows none of this yet; no request changes.
