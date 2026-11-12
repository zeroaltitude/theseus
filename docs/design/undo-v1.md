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

## Where the question itself is asked: 0d sits AFTER the outright refusals

Card `p5kln-p0-judge-egress`, row B6 `theseus-2yowd`, P0 (security). Written against the same base.

The section above settles where the *answer* is read. This one settles where the *question* is asked,
which is a separate and a security question, because asking it sends a tool call's whole content to a
model.

### What the question ships

`crates/theseus-judge/src/builders/undo.rs:156 struct UndoInput` is the payload, and it is the most
revealing state any pack has:

| field | line | what it is |
|---|---|---|
| `argv` | `:167` | a program call's whole command line, up to 24 entries, 200 characters each |
| `steps` | `:170` | a batch's every command line, up to 16 of them |
| `targets` | `:173` | every path the call names, **in full**, up to 12 |
| `url` | `:176` | a network tool's address |
| `other_args` | `:179` | **everything else the model wrote, as it wrote it** — for `fs.write`, that is the content |
| `cwd` | `:181` | where a program call runs |
| `terminal_tail` | `:190` | the last lines on the screen a `term.send` types into |

Nothing in the product sends that today. `undo.v1` is `action = "none"`, no point dispatches it, and
nothing anywhere constructs `Input::Undo`, so "off" is the current state and keeping it off costs
nothing. This card is the one that defines the new egress; the wiring row is the one that makes it
live.

### The decision

> **The judge's check (0d) runs AFTER the outright refusals.**

A call the gate was going to refuse outright must not have its content, its target paths, its
terminal tail and the bytes it was about to write serialised and sent to a model first. The set that
refuses outright is, in `crates/theseus-core/src/toolrun/batch.rs:35 ToolRuntime::judge`:

1. `self.sealed.refusal_of_input(input)` — the sealed holdout sets, on the call's raw input;
2. `self.refusal_at(at, tool, plan)` (`crates/theseus-core/src/policy/modes.rs:136`), which is
   `ToolRuntime::refusal` (`crates/theseus-core/src/toolrun/order.rs:73`: the sealed holdout again on
   the plan, then `places::refusal`, then the place's ceiling) followed by `plan_refusal`,
   `helper_refusal` and `content_refusal` — plan mode, a helper's narrowed set, and the injection
   freeze, whose reader is `At::labels` (`order.rs:31`).

Each of those says "this call is not going to run" for a reason that has nothing to do with
reversibility, and three of them — the sealed holdout, the place rule and the injection freeze — are
reasons the material itself must not travel. 1.0.4 decided that; a classifier placed before them
would undo it quietly, and the first anyone would know is a judge spend row against a refused call.

So 0d sits between the refusals and `order`: after everything that refuses outright, and before the
§3.9 order, so a `reshapeable` verdict can short-circuit beside the refusals as the section above
requires and a held call still makes no card.

### What the new path must not bypass

`crates/theseus-core/src/data_gate.rs` is the provider chokepoint: every provider is wrapped once
when the core is built, so one place covers every caller, and it refuses a request to a model of
restricted origin for a session whose labels disqualify it. `crates/theseus-core/src/turn/data_step.rs`
is the turn's side of the same rule. A judge request is a provider request and goes through that
wrapper like any other; the thing to get wrong would be a path that reaches a model some other way,
and there must not be one. The gate's own `[content_based_security] injection` freeze is already in
the refusal set above, so a frozen session never reaches 0d at all.

### Switched off by default

The pack's `action = "none"` is the off position and is the current state. The wiring row turns it on
— the owner decided on 2026-11-12 that the classifier lands wired and ON — but the *order* decided
here is what makes that safe: on or off, a refused call is never sent.

### The bar, and why there is no code test for it yet

The card's bar is a test in `crates/theseus-core/src/toolrun/` that plans a call the sealed holdout
refuses and asserts that **no `undo.v1` request is built**. That test cannot be written honestly on
this base: nothing constructs `Input::Undo`, so the assertion is met by an empty set and would pass
against a tree with the layer in the wrong place. The test is owed by the row that builds the layer,
and it needs both halves, which is what makes it a real check rather than a vacuous one:

* a call the sealed holdout refuses produces **no** `undo.v1` request — asserted against a recording
  stand-in judge, so "no judge spend row" is read from what was asked and not from what was billed;
* a call nothing refuses produces **exactly one** — the positive count, without which the first
  assertion passes on a tree where 0d was never wired at all.
## `remote_keeps` for v1: `unknown`, said out loud

Card `p5kln-remote-keeps`, row B6 `theseus-2yowd`.

Whether a bucket keeps versions or a document keeps revisions is an API call. The classifier answers
synchronously inside the tool gate (`ToolRuntime::judge`, above, is not async), so there is nowhere
to put that call that does not add a provider round trip and its latency to every write-classified
tool call — on a path that has neither today.

**The decision for v1: accept `unknown`.** That is the current behaviour, not a change:
`crates/theseus-judge/fixtures/golden/undo.v1.request.json` already carries
`"remote_keeps":"unknown"`, and `undo.rs:186`'s `remote_keeps` is a serde-default input nothing
resolves. The three options were paying for the call on the tool path, caching per target with a
staleness bound, and accepting `unknown`; the first two are the ones that widen what Theseus reaches,
and neither has been measured.

What this costs, stated rather than hidden: the two identical-call hard pairs that turn on a remote
store's own retention are **not measurable on the live path** in v1. They are
`fixtures/eval/undo.v1.json`'s versioned-against-unversioned bucket pair (`remote_keeps: "versions"`
against `"none"`) and the document-revisions pair (`"revisions"` against `"none"`). They still
measure in the labelled set, which states the word, so the banked whole-set figure is unaffected; it
is the wired run that cannot claim them. A remote call whose store's retention is unknown is read by
the pack as keeping nothing, which is the safe direction: it reads as a loss and the call is held or
raised rather than run.

The release note this owes is **not written here**: `docs/status.md` is the maintainer's at review.
The line it needs is in `CLOUD_REPORT.md`, ready to paste.
