# The view in the compiler (issue 44)

`[context] assembly = "transcript" | "view"`, for every place or by place, read only by
`ContextConfig::assembly_for` (`crates/theseus-core/src/config/assembly.rs`). Absent, or `"transcript"`: the
compiler is as it was, byte for byte (`tests_view_compile::the_transcript_assembly_sends_the_golden_requests_byte_for_byte`).
A task's turn has no place and is always the transcript's.

## A turn in a place set to `view`

`turn::view_step` decides per turn (`assembly_here`, the place as the exchange ledger writes it) and, on the
turn's first call, assembles its view once (`crate::view::wire::assemble`: every selector from one snapshot, the
gate on each piece, the budgeter, the renderer, the ladder's read). The result is frozen in the turn
(`compiler::view::Frozen`) and every call of the turn is built from it (`compiler::view::compile`):

1. The system part as the transcript's, with the memory page after the session's block.
2. The fixed blocks (who and where, his older words) and the ladder's nodes, as harness text heading the first
   user message.
3. The window: the place's exchanges as real turns, read from the store across the sessions that ran them,
   rendered by the compiler's own renderer (calls paired with their results). Their thinking is not carried.
4. The block beside the new message, then the message, then the turn's own calls and results.

The window's end carries a cache mark; the conversation's automatic mark stays at the end; a system layer's
mark gives way if the request would carry more than the provider's four.

**Frozen within a turn.** The front, the window and the beside block are rendered from the frozen nodes on
every call, so a call's request begins with the previous call's bytes, and the harness's own tail blocks go
back where they rode (`turn::sent_tail`), as in a transcript turn. The next turn assembles afresh: its first
call is a new compilation (strategy `view`, trigger `view_turn`) with its thinking stripped, so nothing of the
turn before is held to its bytes but the window, which only appends.

**Nothing of the view is written into the session.** The beside block lives in the turn alone. A recall note
is not written either (`recall.withheld`, why `view`): recall reaches the model as a selector's piece.

**Tool output** in the window is cleared by the build's own pass (`compiler::clearing`), with this strategy's
numbers: past 80,000 tokens of results not yet cleared, the oldest become receipts until the newest 40,000
remain whole. A pass runs only as a turn begins; the next turn carries what it cleared, so a clearing holds.

**Keep-warm** is off for a place in `view`: its next request is built afresh, with no transcript prefix to read.

The compilation such a turn persists names the window's node ids as its `includes`, but the window is read
again from the ledger at the next turn; no reader should take a `view` compilation as a transcript prefix.
