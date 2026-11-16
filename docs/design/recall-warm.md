# The meaning search after an idle stretch: how recall meets an unloaded model

_Issue 21 of 1.0.7, line "More than half of recalls run without the meaning search". Notes for the core crate that would
have gone in its AGENTS.md, which this release leaves alone._

## The problem

The index tender unloads its embedding model after ten idle minutes (`[index] idle_unload_mins`) and a load takes 0.6 to
1.1 s. A recall asked the tender with `wait_ms = 0`: a model not loaded was sent to load and the recall answered at once
without vectors. In the last 30 days of real recalls, 235 of 421 were answered by words and names alone, and a third of
what only the meaning search finds is over 30 days old.

## What is built

Two parts, both in the core's recall path (`recall/loading.rs`, `recall.rs`'s `race`, and one call in `turn.rs`). The
index itself is unchanged: its `index.warm` and its `wait_ms` already did what is needed.

1. **Load at the first sign of a turn.** A person's message, at its arrival and beside Jev's connections, sends the tender
   `index.warm` in a task of its own (`Memory::warm_on_message`). The turn does not wait for it. A first keystroke
   (`session.typing`) already sent the same call; the message's arrival catches every surface that sends no keystroke.
   One such call is out at a time. With `[memory] mode = "off"` nothing is sent. A slash command sends nothing, as for Jev.
2. **Recall's wait covers a load, up to `LOAD_BOUND`.** A recall's whole query (the one with the vector source) carries
   `wait_ms = LOAD_BOUND` (2 s), so the tender holds it for a load in progress. Recall's own deadline is unchanged
   (`[memory] recall_deadline_ms`, 250 ms by default, at most 5 s). When that deadline passes with the words' answer in and
   the whole answer still out, recall asks the tender once, with `index.warm` and a 100 ms bound, whether the model is
   `loading`, and only then waits on until `LOAD_BOUND` from the recall's start. The extension happens at most once.

## The bounds, and who never waits

| Case | What the turn waits | What answers |
|---|---|---|
| Model loaded, embedding fast | the embedding | words and vectors |
| Model loaded, embedding slow | the ordinary deadline (250 ms) | words alone, as before |
| Model loading | until it loads and embeds, at most `LOAD_BOUND` (2 s) from the recall's start | words and vectors, or words alone past the bound |
| Load never finishes | `LOAD_BOUND` | words alone, the row saying "within 2000 ms" |
| Load failed or refused (`no_weights`, `refused`) | nothing: the tender answers at once with `skipped["vector"]` | words alone |
| Tender off, down, or silent | nothing, or the deadline, as before; the probe is bounded at 100 ms | words alone |

The words' query is never held: it carries `wait_ms = 0` and goes first on the connection.

## What it does to time to first word

A turn whose model was unloaded may now wait for the load before its model call, up to `LOAD_BOUND` less what its other
early steps already took. That is the price of its recall having the meaning search, and it is paid only after an idle
stretch of ten minutes or more, and not at all after a keystroke that warmed the model. A turn in a warm session is
unchanged. `LOAD_BOUND` is a constant, not a setting: one blessed number.

## What keeps the model loaded, and what does not

The model unloads ten idle minutes after its last use. `index.warm` restarts that clock, so a person's message keeps it
loaded, as that message's recall would anyway. Nothing but a turn's message sends the arrival warm: a person's, or the
objective a self-improvement run submits as its turn's input, whose recall loads the model in any case. Background passes
(the memory pass's `index.embed`) already load the model through their own calls and are unchanged. The probe at a
recall's deadline touches the clock too, inside a recall that is itself using the model.

## Known limit

The tender holds a waiting query inside its model's wait without looking at its caller: a recall that gives up at its
bound leaves the tender's thread waiting for the load until the load ends or the query's own `wait_ms` (the same 2 s) is
up, and the connection then closes. The wait is the same bound, and the thread is one of sixteen.

Tests: `tests_recall_loading.rs` (the stand-in tender with the model's states), and in the index
`a_query_that_may_wait_is_answered_with_vectors_after_the_idle_unload`.
