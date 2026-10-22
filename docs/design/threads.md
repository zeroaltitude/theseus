# Threads: the router's rules and the lane's decisions

What the threads lane (theseus-98bz and its follow-ups) decided, in one place, beside the code that holds it. The
router's order is the module doc of `crates/theseus-core/src/thread_route.rs`; this note says why the rules are what
they are, and where each one can be turned back.

## The router's order

An unaddressed message in a place with live threads meets, in order (`thread_route.rs`'s module doc;
`rpc/thread_flow.rs`'s `place_unaddressed`):

1. **Addressing**: the place a thread alone lists, an explicit word (`#slug`, `/t slug`, "in the … thread:"), a reply
   to a thread's turn. Certain, before any judge.
2. **The pre-router**: entity hits in each live thread's working set, stickiness to the place's last thread, the reply
   chain, combined as independent evidence and capped below the act bar together. Then a learned correction (the owner
   once moved a message like this one: the thread is lifted to `LEARNED`, 0.92).
3. **The directory rule** (theseus-vw8k7, below).
4. **Fusion** with `classify.v4`'s `thread` answer, which rides the inbound request the message makes anyway.
5. **The decision**, at the pack's bars: quiet at 0.90 and clear, visible with a move handle from 0.60, else one
   question.

## The directory rule

The walkthrough's case: a plain dry run typed in a project directory that is no thread's was placed in the place's
last thread. Nothing in the message named that thread; what placed it was the owner's earlier move of a dry run like
it (the learned correction, which alone reaches the act bar), with stickiness behind it.

The rule: when the turn's directory is known (the submit's `dir`, else the session's own) and a thread has roots, none
of which holds that directory or lies under it, the thread keeps only its entity score. Stickiness, the reply chain
and a learned correction name nothing of the thread, and the message comes from somewhere else, so they do not count
for it. An entity the message names still counts, and Jev's pick still fuses. A thread with no roots, or a turn with
no directory, routes exactly as before. A relative root is read under the thread's own session's directory (where the
working set grew it); with none it is not read.

`may_route` is unchanged: a message from elsewhere can still name a thread's PR or path, and Jev still reads every
message, so the router keeps placing it; only the priors stop speaking for a thread the directory is apart from.

To turn it back: remove the one call, `keep_apart_by_dir`, in `place_unaddressed`. There is no switch.

## The ask and the sticky rule

With `[threads] sticky_alone_asks` on, a top thread that stands on stickiness and the reply chain alone, which Jev did
not pick, is held just below the confirm bar, so the message is asked about rather than routed. The `new` option rides
that ask alone (theseus-q3ggn) when the message names two or more entities no working set holds, and any ask in which
Jev raised `new`. An ask between two close threads that no cap made does not offer `new` for entities alone: the
owner's ask (theseus-eqp1v) was for the cap's ask. The one-thread offer (`lone_offer`, theseus-98bz.7) is its own rule
and unchanged.

## The bench's clock

`TurnSubmitParams.route_at_ms` carries the clock the threadbench planned a message at, so stickiness and the chain
decay as they would have between real messages. It is read only under `[threads] bench_clock = true` (a test hook,
false by default), and a submit carrying it to a daemon without the switch is refused, not ignored.

## Deferred tools past the budget

For a model without the provider's tool search, a first request is held to its place kind's budget. A family the
message names, or one `families.v1` judges relevant, may load past it by exactly its own size while the whole stays at
or under `budget × [tools.defer] admit_ceiling` (2.0; `config/defer.rs` says why 2.0). What passed the budget rides
`TurnSubmitResult.raised`: the families, each with its tokens and who admitted it (`named` or `jev`), and a line the
surfaces show after the turn's harness lines. `admit_ceiling = 1.0` and `judge_families = false` are the rollbacks.

The `families.v1` call is made on every routed prompt, a model with tool search included, unless the route pack is
below canary. That is the owner's decision of 2026-10-21: always try to categorize each toolset. It is not narrowed.

## The home claim and a split beside it

A routed turn that ran at home but wrote only one thread's files has its nodes keyed to that thread (theseus-98bz.6).
The claim reads each node's key, keeps a split's, and writes the rest. A split writes its keys in a kernel frame under
the source session's execution lock, and refuses while a turn runs there; the claim runs right after the home turn
ends, when a split may begin. So the claim and its undo read the keys and write in one frame under the same lock
(theseus-l4n39): a split lands wholly before the read, and its value is kept, or after the write, and it takes the
claimed nodes, as a split may. No new lock. `thread.unclaim`'s `kept` counts only keys holding another claim's or a
split's value; a key an unsplit nulled is neither restored nor kept.
