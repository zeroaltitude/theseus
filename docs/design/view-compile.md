# The view in the compiler (issue 44)

Every conversation's request is built by the view. There is nothing to set: the `[context] assembly` setting
that once chose between the view and the old transcript is retired (a settings file that still names it loads,
says so once at startup, and `theseus health` lists it among the settings keys ignored). A task's turns are
the view's too, at the task's own place, `task:<task id>`.

## A turn

`turn::view_step` assembles the turn's view once, on its first call (`crate::view::wire::assemble`: every
selector from one snapshot, the gate on each piece, the budgeter, the renderer, the ladder's read). The result is
frozen in the turn (`compiler::view::Frozen`) and every call of the turn is built from it
(`compiler::view::compile`):

1. The system part, with the memory page after the session's block.
2. The fixed blocks (who and where, his older words) and the ladder's nodes, as harness text heading the first
   user message.
3. The window: the place's exchanges as real turns, read from the store across the sessions that ran them,
   rendered by the compiler's own renderer (calls paired with their results). Their thinking is not carried.
4. The block beside the new message, then the message, then the turn's own calls and results.

The window's end carries a cache mark; the conversation's automatic mark stays at the end; a system layer's
mark gives way if the request would carry more than the provider's four.

**No hand-back.** A turn is never handed back to the old transcript. Each case that once was has its own
answer in the view: a turn no message opened is assembled with an empty message and a line saying what woke
it; the turn's place is read once, for its exchange and its gate alike, and a place that cannot be read fails
the turn plainly; history the ledger holds no exchange for is out of the view's reach; a request too large
makes its own room (`turn::view_step::room`) or fails on its overage; a read that fails during assembly fails
the turn with its words (never a partial view); and a gate that keeps moving while a call is prepared fails the
call after its second reassembly. Old `context.view_fallback` rows still read; none is written.

**Frozen within a turn.** The front, the window and the beside block are rendered from the frozen nodes on
every call, so a call's request begins with the previous call's bytes, and the harness's own tail blocks go
back where they rode (`turn::sent_tail`). The next turn assembles afresh: its first call is a new compilation
(strategy `view`, trigger `view_turn`) with its thinking stripped, so nothing of the turn before is held to its
bytes but the window, which only appends. A change of the system text (a membership, guidance, a context file)
starts a new chain at the next turn: there is no recompile to ask for.

**The rendered front, window and beside blocks are not copied into the session.** They live in the turn, and
recall reaches the model as a selector's piece. The provenance is durable: `turn::recall_step` keeps a `Recall`
node and its `derived_from` edges to the sources, and those records and the recall rows ride the provider
action's plan frame (`turn::model_call`). Citations, reach and retraction use those links; they are not an old
transcript-builder fallback. The `Recall` node itself is kept out of the view window and the turn's tail.

**Tool output** in the window is cleared by the build's own pass (`compiler::clearing`), with this strategy's
numbers: past 80,000 tokens of results not yet cleared, the oldest become receipts until the newest 40,000
remain whole. A pass runs only as a turn begins; the next turn carries what it cleared, so a clearing holds.

The compilation such a turn persists names the window's node ids as its `includes`, but the window is read
again from the ledger at the next turn; no reader should take a `view` compilation as a transcript prefix.
