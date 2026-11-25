# A thinking block is bound to what came before it

Issue 20, "A turn on the newest model fails at its second step". On 2026-11-16 the self loop's reviewer, pinned to
`claude-fable-5-2`, made one call, ran a tool, and was refused on its second call: "Invalid `signature` in `thinking`
block. The block is bound to a different conversation." This is the map of why, what holds it now, and how to see it.

## The rule the provider keeps

A thinking block the model wrote is bound to the system blocks, the tools, every message before it, and the blocks
before it in its own message. A request that replays the block after any of that changed is not the conversation the
block was written in. What happens then depends on the model:

- the older models (Sonnet 5.5, Opus 5.5, Haiku 5.5 on an account that predates the rule) report it in the answer's
  `input_transformations` as `thinking_mismatch_allowed` and show the block to the model anyway: nothing fails and
  nothing is said, except that the prompt cache is missed;
- `claude-fable-5-2` refuses the request with a 400 unless the request says what to do
  (`thinking.block_binding.prefix_mismatch_behavior = "drop_block"`, which removes the failing blocks and reports each
  as `thinking_dropped`).

The vendored SDK's types describe both entries and the request field (`BetaThinkingBlockBindingParam`,
`BetaThinkingMismatchAllowedInputTransformation`, `BetaThinkingDroppedInputTransformation`); they do not say whether a
`cache_control` mark is part of the binding. Theseus moves its own mark between a turn's steps by design, and the test
stand-in leaves marks out of what it binds; whether the real provider does is shown only by a live turn.

## The cause found: a tail that did not stay

Every request ends with a tail of the harness's own blocks, glued to its last message after the cached prefix: the date
and the git line, the task view, the standing records, the persona line, a hint (`task_graph::view::attach_text`, all
of them). They are new at each step, so at the next step the message that carried them had lost them: the first user
message of a turn read `[words, date]` at step one and `[words]` at step two, under the thinking block of step one's
answer. That is the `messages.0.content.0` of the refusal, and the `messages.2`, `messages.6`, `messages.44` of the
canary's reports on the older models: the message just before a thinking block, in a turn that had a step before it,
or in a conversation that had a turn.

`turn/sent_tail.rs` keeps each request's tail with the message that carried it, by what that message rendered as, and
puts it back on the turn's later requests. A block the latest tail already holds, unchanged, is not sent again, so a
turn of many steps grows by what changed. **Tails are kept within a turn only.** A new turn's request carries its own
tail whole, and nothing an earlier turn sent is put back: a turn sends no earlier turn's thinking (the view strips it,
`compiler::view`), so no earlier message needs to stay as it was under a block, and a kept tail was never behind the
gate. A tail is kept with where it was sent (the place class, the surface, where the session speaks, what the turn
answers) and put back only on a request for the same, and a request for a model of restricted origin gets none. A tail
is kept once routing is done with the request (an effort message, a persona line), and found again by what its message
rendered as, not by its index. A kept tail whose message no longer renders as it did (a new chain) is let go, and all
of them are when a recompile strips the thinking. The census's token count leaves every tail out, this turn's
included. They are kept in memory, 256 sessions at most: a daemon restart in the middle of a turn forgets them, and that
turn's next request meets the net below once.

A look back's result kept by reference (issue 49) stores no words: the one renderer (`quotes::render`) makes them
where it is laid, and they could differ between two steps (a quoted message marked wrong since gains a line). So a
turn renders each once: the window's when its view is frozen, its own at the first call that lays them, kept on the
turn's view (`quoted::freeze`) and laid as they are by every later call. A quoted message erased during the turn makes
the view gone: it is laid again on a new chain, the turn's answers so far without their thinking, so the erased words
are not sent again and no thinking block is bound to the change (`tests_binding.rs`,
`an_erase_between_steps_starts_a_new_chain_not_a_400`).

## The net

When the provider refuses a request with that 400 and no other, the call is made once more with the remedy
(`provider::binding::repeat_for`), the turn goes on, and the answer is marked `prefix_repeated`. The next compile
of the turn starts a new chain without the old thinking, so the step after is not made twice. The remedy is
never sent on a first call: where a mismatch is allowed the model keeps its thinking and the canary keeps reporting.
The request the compiler rendered is not changed, so what is recorded and replayed is what it was.

## How to see it

`theseus ledger -k provider.call` carries, on each row:

- `thinking_mismatch_allowed`: blocks the provider reported and let through (the older models);
- `thinking_dropped`: blocks it removed on a repeat (the newest model); present only when above zero;
- `prefix_repeated`: the call was refused and made again; present only when it was.

**A count above zero is a bug to file.** Nothing Theseus sends after a thinking block within a turn may change, so none
of the three should ever be non-zero on a turn that was not interrupted by a restart. After a restart one
`prefix_repeated` on that turn's next call is expected (above). A count that persists is a new cause: the test over
every kind of turn (`tests_binding.rs`) is where to add its case.

## The test that holds it

`provider::binding` gives the stand-in provider the rule: it stamps each thinking block it answers with a hash of
everything before it, and checks the blocks a request replays against it, reporting, or refusing like the newest model.
`tests_binding.rs` drives whole turns of two or more steps against it and asserts that each request repeats the one
before it, block for block, within the turn: a person's turn in a private place, the self join's review turn, a
task's turn, a recall note that arrives after the first compile, a refusal's fallback, and a retry; and the net, once,
narrowly, and not at all where a mismatch is allowed. `tests_thinking_writer.rs` holds the render's rule that another
model's thinking is never sent, and that a refusal's fallback model, inside the turn, keeps it.

## What it does not do

- It does not cover the bench's replay of a recorded request (`own_bench/replay.rs`), which calls the provider
  without the net: a recorded request that drifted will meet the newest model's 400 there. The bench's rebuild of a
  request (`own_bench/rebuild.rs`) is from the nodes alone and never had the tails.
- It does not keep the tails across a restart, or across turns. A restart in the middle of a turn costs one repeat;
  keeping them would mean a record of each request's tail in the store.
- It does not change what a tail says, or when: a block that changed is sent where the request ends, as it was.
