# It trusts its environment and keeps a true record

*One page on the principles. Updated 2026-11-23; the proof below records the 1.0.3 work.*

## What it is

Theseus is built for a machine its owner set up and trusts. That decides how it treats safety: trust and provenance
start high, the guards are light and sit at the exits, and performance, simplicity, speed, scale and power are what
the design spends its complexity on. Four principles follow.

- **Notify, don't block.** A self-started task, a long turn, a chain of tasks ten deep: it runs, and you hear about
  it. Hard stops exist only at the edges (a runaway burn, a day's ceiling, a limit you set), and turn length alone is
  never a defect. The owner's words: long work over stopped work.
- **The session is the true record.** What is stored is raw and is never rewritten. Secrets are substituted only on
  the copies that leave the machine: the request to a model, a log line, a client's screen.
- **Labels describe content.** A sensitive name, a Google result or a judged injection raises a label you can
  remove. Labels do not keep context off particular models. At the shipped default, `injection` only tells you
  (`[content_based_security] injection = "notify"`); `injection = "refuse"` gates calls that act until you clear it.
- **One memory in trusted places.** Every configured model uses the same context path. Place and category labels
  do not withhold memory; the owner must trust the providers and places they connect.

## How it works

- **Content-based security** (`[content_based_security]`): `injection` controls the response to an injection label.
  The former `cn_models` and `wait_ms` settings load with a warning and do nothing; there is no model-origin floor.
- **Replay:** content judgments can be replayed to measure a classifier's new wording against recorded traffic.
- **Labels, not provider filters:** `theseus content SESSION` lists a session's labels, `theseus content remove`
  takes one away, and a removed label stays removed. Labels are still written even though origin restrictions are gone.
- **The raw record**: the store keeps what was said; the scrubber knows the vault's actual values and
  substitutes exactly those, guessing no shapes, on every exit. `theseus inspect` tells you which secret, by name,
  went in which request to which model.
- **Earned autonomy was removed**: the mechanism that counted repeated approvals and offered to stop asking,
  and its scoped grants, were 6,593 lines that could never fire on the owner's config. In a trusted environment the
  default is to notify, so there is nothing to earn.
- **Act gates remain** ([privacy](../privacy.md)): approvals, the floor, the shared-place tool ceiling and publish
  approval are separate from context selection. There is no required draft-only sandbox or reviewer by model origin.

## What you see on each surface

- **Discord:** a label is a chip on the message, with Remove; a self-started task posts its card and goes on.
- **Terminal:** `theseus content`, `theseus content remove`, `theseus inspect`, `theseus policy explain`.
- **Cockpit:** the Policy page.

## The proof

- **Nine of eleven rows are joined on the lane** as of 2026-10-28, each gated green and each reviewed by a
  different model; three of them are in `main` and installed on the owner's daemon. Every review returned findings,
  and four caught things that would have shipped, among them a Remove that did not stick because the pack put the
  label back nineteen milliseconds later, and an exit that let an encoded value through.
- **The classifier:** `content.v3` at precision 0.956 against `content.v2`'s 0.875, recall held, on the owner's 28
  replayed corrections.
- **Tests:** the lane's red-then-green tests for each row, among them `crates/theseusd/tests/exits.rs` for what
  leaves the machine.

## The limits today

- A label is not a data boundary. Jev can misclassify content, and removing a label does not change the raw record
  or limit which model receives it. Vault-value substitution at exits is a separate mechanism.
- **The four content packs' header comments (`crates/theseus-judge/packs/content.v1.toml` through `v4.toml`) say
  that raising `injection` "freezes a session until a person taps a button".** That describes one of the two
  settings and reads as though it described both: what the label costs a session is
  `[content_based_security] injection`'s answer, and only at `refuse` are the session's calls that act refused.
  The comments are left as they are on purpose — **a pack is a judged artefact, and editing one means a version
  bump and a re-evaluation of the whole labelled set, which is not a price worth paying for a comment.** They
  are inert: nothing reads a pack's comments, and the rule itself lives in the core. The wording moves the next
  time one of those packs is bumped for a reason of its own.

## The 1.0.3 plan (historical)

- **1.0.3:** the lane's last two rows, one gate of the merged lane, one merge, the install, and the tag.
- **1.0.4:** the self-gates inventory applied across every stop rule ([It fixes itself](self-improvement.md)).

Related: [Durable tasks](durable-tasks.md) (a task that notifies rather than asks),
[Jev in the core](jev-in-the-core.md) (the content judgment).
