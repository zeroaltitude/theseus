# `undo.v1`: where the reversibility verdict is consumed

Row B6 `theseus-2yowd`; cards `p5kln-p0-raise-cancelled`, and the wiring row `theseus-p5kln`.
Written against base `75f19f3c`, where the pack is on main and nothing calls it.

The pack itself is `crates/theseus-judge/packs/undo.v1.toml`, and its header is the design of the
*question*: what it asks, what it refuses to ask, the three verdicts, the fail-open rule, and the
measured 48/54 and 14/15. This document is the design of the *consumption*: where in the gate the
answer is read, and what happens to it there. The two are separate because the pack can be — and was
— measured without any of this existing.

## The error this document corrects

The first sketch of the wiring said the reversibility layer goes **before** `Layer::Mode` in the
gate's order. That is wrong, and it is wrong in a way that silently deletes the only half of the
feature a person ever sees.

Read `crates/theseus-core/src/policy/modes.rs:214 fn gate`, which `crates/theseus-core/src/toolrun/order.rs:123`
runs as `Layer::Mode`. In a session in `edits` or `auto` mode, in a private place, it takes a
decision whose posture is `Approve` and **lowers** it — that is the whole purpose of a mode. Its
guards are listed in one condition, and the only ones that save an `Approve` from being lowered are:
the posture is not `Approve` already, the job is L1, `d.floor` is set, the place is not private, the
operator has tightened that tool, the call carries a granted environment, or the policy did not
decide by posture alone.

An `irreversible_final` verdict raises the gate's posture to `approve` (the pack header: "the gate's
posture is raised to `approve` ... the owner pays"). A raise placed before `Layer::Mode` is an
`Approve` with `floor: false`, in a private place, on a tool the operator has not tightened — which
is exactly the shape `modes::gate` exists to lower. So in `auto` mode, which is the mode a long
coding session runs in, the owner would never be asked. The guard would read as present in the code,
measure as present in the pack's score, and be absent in the product.

Nor can the raise simply set `floor: true`. The floor is a named thing with its own meaning
(`crates/theseus-core/src/policy.rs:145`: "the call touches Theseus's own binary or state, or the
1Password CLI or token. No setting makes it run unasked"), the floor has readers of its own, and a
reversibility verdict is not that. And the generic raise does not offer the choice anyway:
`Decision::at_least` (`crates/theseus-core/src/policy.rs:201`) builds each of its three arms through
`Decision::new`, which sets `floor: false` (`policy.rs:175`) and carries forward only `granted`. A
raise made with `at_least` is lowerable by construction.

## The corrected shape: ask once, consume twice

One judgment, two consumption points, on opposite sides of the order.

```
ToolRuntime::judge  (crates/theseus-core/src/toolrun/batch.rs:35)
  |
  |-- the outright refusals: sealed.refusal_of_input, then refusal_at
  |     (the sealed holdout, the place rule, the place ceiling, plan mode,
  |      the helper's set, the injection freeze)
  |
  |-- 0d: ask undo.v1, ONCE, for this call            <-- the one judgment
  |
  |-- CONSUMPTION POINT ONE: irreversible_reshapeable
  |     short-circuits here, beside the refusals, and `order` never runs.
  |     The call is HELD, the model re-plans next loop, and no card is made.
  |
  |-- order(...)   the whole §3.9 order: Policy, Mode, Grant, Lsp,
  |                SharedFetch, Floor, Glide, Routine, McpClient
  |
  `-- CONSUMPTION POINT TWO: irreversible_final is applied to what `order`
        RETURNED. Every layer has already had its say, so no mode, ceiling or
        glide can lower it, and no `floor: true` has to be invented to protect it.
```

### Point one — `irreversible_reshapeable` short-circuits beside the refusal

A reshapeable call never reaches `order` at all. This is the right place for three reasons.

It costs no attention. A hold is answered by the model on its next loop, so it must not travel
the path that makes an approval card; running `order` first and then holding would make a decision
that nothing consumes.

It cannot be lowered, because there is nothing to lower: a hold is not a posture. The mode's
contract is about postures, and a hold is a different kind of answer — the call did not run and the
model was told why. `auto` mode is a statement about who answers questions, not a statement that
loss is acceptable.

And it is where the other "the call did not run" answers already live. `judge`'s `Err(Turned::…)`
arm is the gate's existing shape for "this call is not going to run, and here is the sentence the
model reads". A hold belongs beside it, not inside the posture ladder.

### Point two — `irreversible_final` raises what `order` returned

The raise is applied to `order`'s return value, after the last layer. Three consequences, all of
them the point:

* No later layer can lower it. `Layer::Mode` has already run and already done whatever lowering
  `edits` or `auto` entitled it to; the raise lands on the result.
* The raise needs no `floor`. It does not have to borrow the floor's meaning to survive, so the
  floor keeps meaning what `policy.rs:145` says it means.
* Every layer that could *raise* the posture already has. The order's own invariant is that each
  layer after the first only raises (`order.rs`'s header). A raise that lands after all of them
  therefore cannot weaken anything: `irreversible_final` can only take an already-stricter decision
  and leave it alone, which is what a raise must do.

The raise does not refuse. The pack's header gives the reason and it is unchanged here:
"irreversible and necessary" must be able to happen, and a guard that cannot be satisfied is a
guard that gets turned off. `policy.rs` already states the rule the whole gate keeps — nothing the
gate decides refuses a call; the strongest answer is to wait for the operator.

### `reversible`, and everything that is not an answer

A `reversible` verdict changes nothing: the call goes into the ordinary order and the gate is the
gate it was in 1.0.4. So does every non-answer — the judge off, the credential unresolved, the
breaker open, the call shed, the call timed out, or an answer under the question's confirm bar. That
is the pack's fail-open rule, and it is what makes the pack a net addition rather than a new
dependency of writing a file.

## What this constrains, and what it does not

This document fixes **where** the verdict is read. It does not decide whether the pack is on, what
computes the recovery words, or how the owner overrides a hold; those are the wiring row's own
pieces and its siblings'. What it does fix is that the wiring row cannot implement the raise as one
more `Decision::at_least` inside the ladder, because the ladder's own `Layer::Mode` would undo it.

The eventual bar is the wiring row's test `a_final_verdict_survives_auto_mode`: a session in `auto`
mode, a call whose verdict is `irreversible_final`, and the owner still asked. There is no code test
today because there is no layer; this document and the pack's own comment are the whole of the bar
for this card.
