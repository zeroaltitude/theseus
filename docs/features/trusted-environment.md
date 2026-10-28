# It trusts its environment and keeps a true record

*One page on the principles. This is landing this week, in version 1.0.3; each piece below says whether it is in
`main` yet. Updated 2026-10-28.*

## What it is

Theseus is built for a machine its owner set up and trusts. That decides how it treats safety: trust and provenance
start high, the guards are light and sit at the exits, and performance, simplicity, speed, scale and power are what
the design spends its complexity on. Four principles follow, and 1.0.3 is the release that makes the code say them.

- **Notify, don't block.** A self-started task, a long turn, a chain of tasks ten deep: it runs, and you hear about
  it. Hard stops exist only at the edges (a runaway burn, a day's ceiling, a limit you set), and turn length alone is
  never a defect. The owner's words: long work over stopped work.
- **The session is the true record.** What is stored is raw and is never rewritten. Secrets are substituted only on
  the copies that leave the machine: the request to a model, a log line, a client's screen.
- **A label you can remove, not a refusal you cannot.** A sensitive name, a Google result or a judged injection
  raises a label on the item; the label gates what acts, and you can tap it away.
- **Models of other origins see only what was judged open.** A request to a model of China or unknown origin
  carries only what Jev cleared for that one request, and that floor has no off switch.

## How it works

- **Content-based security** (`[content_based_security]` in the config; in `main`): one table says what each label
  does and which models are allowed what. A config that would silently refuse every China-origin model refuses to
  start and names both ways out.
- **Replay** (in `main`): every content judgment the daemon ever made replays, so a classifier's new wording is
  measured against your real traffic before it ships. `content.v3` (on the lane) was.
- **Labels, not refusals** (on the lane): a hit on `[privacy] sensitive_names` raises a `confidential` label at the
  item; a Google result raises a removable label; the China-origin chokepoint reads labels only, one record against
  `cn_models`, instead of walking the transcript. `theseus content SESSION` lists a session's labels and
  `theseus content remove` takes one away, and a removed label stays removed.
- **The raw record** (on the lane): the store keeps what was said; the scrubber knows the vault's actual values and
  substitutes exactly those, guessing no shapes, on every exit. `theseus inspect` tells you which secret, by name,
  went in which request to which model.
- **Earned autonomy goes** (on the lane): the mechanism that counted repeated approvals and offered to stop asking,
  and its scoped grants, were 6,593 lines that could never fire on the owner's config. In a trusted environment the
  default is to notify, so there is nothing to earn.
- **The China-origin floor** ([docs/privacy.md](../privacy.md)): the built-in catalog marks each model's origin,
  counted by the model's family, never its host, and a model it cannot place counts as unknown. Such a model drafts
  code only, in its own copy of an open repository, and a Claude model reviews the draft before it is applied.

## What you see on each surface

- **Discord:** a label is a chip on the message, with Remove; a self-started task posts its card and goes on.
- **Terminal:** `theseus content`, `theseus content remove`, `theseus inspect` (landing), `theseus policy explain`.
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

- Until the lane joins, `main` has the config table and the replay; the labels, the raw record and `theseus inspect`
  are on the lane.
- Jev's content verdict is a judgment: it has called open-source work private before, which is why a label can be
  removed and why the floor underneath does not depend on it.

## What is coming

- **1.0.3 (this week):** the lane's last two rows, one gate of the merged lane, one merge, the install, and the tag.
- **1.0.4:** the self-gates inventory applied across every stop rule ([It fixes itself](self-improvement.md)).

Related: [Durable tasks](durable-tasks.md) (a task that notifies rather than asks),
[Jev in the core](jev-in-the-core.md) (the content judgment).
