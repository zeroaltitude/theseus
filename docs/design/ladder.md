# The `did` lines and the ladder

Step 4 of the context refactor (issue 44): a one-line summary of each exchange, and the fold nodes of a place's
ladder. Records written beside the exchanges (`exchanges.md`), never in them. The view renderer lays these summaries in the context sent to the model that
answers a turn. The code is `crates/theseus-core/src/ladder/`.

## What is written

| Record | Key | Written | Holds |
|---|---|---|---|
| A `did` line | `did.<turn>` | once, after the exchange ends, by a background job | at most 200 characters on what Theseus did, found or decided; the model that wrote it and the writer's version; the exchange's marks; its cost |
| A fold node | `fold.<place>␁<level>␁<first ms>␁<range digest>` (handle: `foldh.<id>`) | once per range, when its place's window or level is full | its level; the exchanges it stands for, by turn; the person's lines by node id; a title (at most 120 characters) and an account (at most 2,400); the union of its sources' marks; its handle, the id |
| A rung | `rung.<place>␁<level>␁<u64::MAX - last ms>␁<id>` | with its node | the node's id, so a level's newest nodes are the first keys of its prefix |
| The day's spend | `ladder.day` | a reservation before every call, its settlement after | the local day, what was settled, and the calls reserved and not yet settled (counted as spent) |
| A `/new` | `ladder.new.<place>` | where Discord's `/new` is handled | the position (time, turn) after which the place's ladder counts |
| A correction | `ladder.correction.<turn>` | in the judge's frame, when `classify` calls the turn's message a correction | `true` |

New META families only: no field on an existing record, no store-format step.

## The rules

- **A `did` line reads two things**: the person's message and the final reply, each cut by code at 4,000
  characters. Tool results are read only for the marks (outside text). An exchange with no line is shown as
  "reply not summarised" and its handle (`did::Said`).
- **Sources are summarised without seals or moves.** Since 2026-11-22, an old seal, veil or move
  does not hold a source out of a `did` line or first-level fold. The writer still needs an ended exchange,
  a reply and a laddered place, and keeps its spend reservations.
- **Marks record provenance**, not an audience boundary around context. Authorship still determines how
  quoted words are presented; it does not make another person's words the owner's instructions.
- **Outside text is wrapped**: an exchange that read outside text is sent to the writer inside the outside-text
  wrapper, and the instructions say it is data, not instructions.
- **Places are exact**: every exchange and node read is checked to be of the place (and level) asked; a place whose
  name holds the key separator (U+0001) has no ladder.
- **A fold is by count**, never by a judge or the clock. The window is every exchange of the place after the last one
  a first-level node holds, read page by page (nothing is passed over; a long failure grows it, and the budgeter
  bounds what is shown): at 2N (N = 20) ended exchanges, the oldest N become one node and the window is N again. A level
  of 2N standing nodes folds its oldest N into one node of the level above.
- **No model writes a level above the first.** Code builds it from its children's typed fields: their titles and
  handles, the person's lines that are corrections or made a standing record, and a count of the rest.
- **A node never changes once written**: its range (place, level, exact turns) has one key, and a writer that finds
  it returns it. A correction rewrites none.
- **`/new`**: only what came after the place's latest `/new` mark counts (`ladder::new_mark`). No mark is no reset; a
  mark that does not read is an error; a session merely being replaced is not a reset. The terminal has no `/new`.
- **Money**: every call reserves its worst case in `ladder.day` before it is sent (the day's line is a constant,
  `ladder::DAY_USD`, $2), then on the daemon's day ceiling, and is settled at its price. A build's `--cap-usd` is the
  smaller of it and what is left of the day's line; passing the line takes `--over-day-line` (with a cap). A failed
  call's cost is the build's, and a build whose lines stopped folds nothing. Only the owner in person may build. The model is a background job's
  (`job_model`): Jev's pick for a synthesis over the owner's material, else `live`.

## When it runs

At an exchange's end, the point the card's observer and `people.v1` already use (`judge::categorize`), in a task
of its own: never a task's turn, never on the turn's path. It writes the line, then runs the place's fold step.
One summary job runs at a time. `theseus exchanges ladder <place> [--dry-run] [--cap-usd N]` builds a place's
ladder from its history: the missing lines, oldest first, then the folds. A dry run counts the exchanges, the
lines and the folds, prices them at their worst case, and writes nothing.

## Where it is read

- Every turn uses the view (`view-compile.md`); the old `[context] assembly` and `ledger` keys are ignored
  with a startup warning. The window and the ladder are read from
  these records: the fold nodes stand for the exchanges older than the window, and an exchange with no `did` line
  reads "reply not summarised". A fold is kept only when every exchange it stands
  for still resolves (`view_step::fold_constituents`); its sessions become the view's dependencies. Old seals, veils
  and moved-message records do not withhold it or force a code-only summary.
- **Authors**: each quoted excerpt is shown with its own author, read from the message's mark (the owner, another
  person, author unknown), and the writer is told each author. A node written before authorship was recorded
  (writer version 1) shows its excerpts but no title and no account, unless it is a first-level node whose every
  message is shown as the owner's; a `did` line of that version is not fed to a new summary.

## Reading them (`theseus ledger gists`)

`theseus ledger gists [PLACE] [--lines 50] [--nodes 10] [--seed N]` (`exchange.gists`, `ladder::gists`) prints a
reading sheet: a draw of `did` lines and of first-level fold nodes, from one place or every place. Each item shows
what its writer read (`did::material` for a line; `fold::material` for a node, each exchange with the `did` line it
had when the node was written), then the line or node as the view shows it (`did::shown_line`; the view's fold
block), then a blank verdict: wrong number, name or choice; other error; or ok. So a wrong verdict is the writer's
error, never a cut the reader did not see. The sheet says how many lines and first-level nodes exist, and says so
when fewer exist than were asked. It prints its seed: the same seed over the same records draws the same items.
It is the owner's in person (the check `exchange.ladder` uses): an agent's process, a job and a chat surface are
refused. It stores nothing, no verdict and no sample, and calls no model; the owner keeps his own counts.
