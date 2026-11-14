# Proof points

The measurements behind [the README](../README.md), one section for each thing it says we care about. Every number
here has a date and comes from a run recorded under [benchmarks/](benchmarks/README.md); [the
omnibus](benchmarks/omnibus.md) lists every run and what it changed. The feature pages under
[features/](features/README.md) say how each thing is built.

**Read the dates.** The long benchmarks are parked until the 2.x line, which is where measuring begins again, so
the head-to-head numbers below are the last ones that were run, mostly on the v1 and 1.0.3 builds. They are not a
claim about the build you would install today. The fast benchmarks are different: they run on every commit, and
their budgets are in [the development guide](development.md#budgets-the-code-is-held-to).

## Where Theseus loses

First, because it is the part most pages leave out.

- **Coding against the best harnesses on another model.** On Terminal-Bench 2.0 at high effort Theseus is behind
  the harnesses running GPT-6 Astra (Codex 85.4%, OpenClaw 82.0%, against its own 78.3%), and on the 30 hardest
  tasks at top effort it solved 65.6% to Codex's 86.7% (2026-10-27).
- **Cost.** The 1.0.3 build cost 2.2 to 2.8 times what the v1 build cost a trial, like for like, because a model
  refusal switched the rest of a turn to a fallback model. The cause is narrower now, and what the cost is today
  has not been measured.
- **Dates in memory.** 36.7% right on one 2026-10-26 seed, against 91.3% for paths and 81.8% for hosts.
- **Small samples.** The kill-and-resume result is twelve trials and its intervals are wide. The recall
  head-to-head is one run per harness, and about five points of any one recall number is the seed.
- **The self-improvement loop's verified mark.** The first change the loop landed by itself carried commits the
  forge does not show as verified.

## Work that outlives the conversation

Killed with SIGKILL mid-task and restarted, Theseus resumed 10 of 12 trials with 0 effects done twice; Claude Code
repeated an effect in 4 of 12 (2026-10-18). With 10,000 parked sessions the idle daemon spends 0.37% of one CPU and
117 MB.

More: [durable tasks](features/durable-tasks.md).

## Fast, as a promise the build keeps

Against Claude Code on the same stand-in model (2026-10-18): a ready prompt in 39 ms against 498, 10 ms of CPU a
turn against 1,655, 70 MB idle against 360. The speed is written down as budgets — start to answering 50 ms, crash
and restart 150 ms, a binary swap under load 200 ms — and **every commit's gate fails the commit when one slips**.

More: [fast as a contract](features/fast-as-a-contract.md).

## Judgment in the loop

On all 89 tasks of Terminal-Bench 2.0 at high effort (2026-10-26), Jev ran 41 of 267 trials entirely on Haiku 5.5,
and the run solved 78.3%. Every judgment is recorded with its probabilities and replays against the record, so a
new version of a question is measured before it decides anything.

More: [jev in the core](features/jev-in-the-core.md).

## A prompt you can read

Tests check the layers, that sessions and threads share their first layers byte for byte so the provider's cache
holds, and that a project's slice survives three compactions. On the owner's daemon a `theseus why` listed a slice
of 177 tokens, five context files, five recalled notes with reasons, and 30 left out for the budget (2026-10-19).

More: [compiled context](features/compiled-context.md).

## One memory, with receipts

Over 600 scripted turns in three sessions, Theseus answered 73.1% of recall probes against 56.9% for Pi and 50.7%
for Claude Code (2026-10-18). Two fresh seeds on the 1.0.3 build scored 68.8% and 77.8%, so about five points of
any one number is the seed (2026-10-26).

**Rough edge.** Dates are the weak kind of fact: 36.7% right on one 2026-10-26 seed, against 91.3% for paths and
81.8% for hosts.

More: [memory across sessions](features/memory-across-sessions.md).

## It gets better while you sleep

The nightly loop has been on since 2026-10-18, and in 1.0.5 **it landed a change on `main` by itself, unattended,
for the first time**: it mined a real defect out of its own record, built the fix inside its own sandbox view,
passed the project's full gate there (9,879 of 9,879 tests), was reviewed by a model other than the one that wrote
it, and joined under the join lock. The fix was small and real — `fs.edit` had answered an edit whose old text
equals its new text as invalid input, and now answers "No change made" and leaves the file alone. The keel guard
fails any commit range that weakens a test without a signed, quoted yes, Theseus's own commits included, and the
file that defines what a self build may reach is itself in a class that stops a self-made join for the owner's
word, so the loop cannot widen its own view. One thing it does **not** yet have: the two commits of that merge are
not verified by the forge — the content commit is unsigned and the merge is signed by the machine's own key — and
whether a self-made join should carry the verified mark is an open question, not a solved one.

**Since then.** From 1.0.6 the loop signs its own content commit. No self-made change has landed since to show the
verified mark, so that is built and not yet proven.

More: [self improvement](features/self-improvement.md).

## Many characters, one mind

Tests check that switching personas changes no byte of the cached prompt, that two threads' turns overlap in time,
and that a thread you name always wins. On 40 scripted messages the router places at least 90% and asks about at
most 10%. Five personas and five threads have been live on the owner's daemon since 2026-10-18.

More: [personas and threads](features/personas-and-threads.md).

## It trusts you, and asks only when it matters

Landed in 1.0.3, in twelve reviewed rows: four removable labels on a session read by one switch, and 6,593 lines of
the old earned-autonomy machinery deleted because it never fired. The content classifier that raises the labels was
measured at precision 0.956 at the raising bar against the previous version's 0.875, and recall 0.985 against
0.955.

**Not measured yet.** The question asked before a tool acts (can this be undone?) is new in 1.0.6. Its behaviour is
held by tests; how often it holds something it should have let run, and the reverse, has not been measured on real
use.

More: [trusted environment](features/trusted-environment.md).

## Talk to it where you are

**Not measured yet.** The surfaces have tests and daily use behind them, and no benchmark of their own.

## A real coding agent underneath

On all 89 tasks of Terminal-Bench 2.0 at high effort, the frozen 1.0.3 build solved 78.3% of its trials
(2026-10-26) — level with the harnesses on its own model: Claude Code 75.3%, OpenClaw 74.2%, OpenHands 71.9%. It is
**behind** the harnesses on GPT-6 Astra (Codex 85.4%, OpenClaw 82.0%), and on the 30 hardest tasks at top effort it
solved 65.6% to Codex's 86.7% (2026-10-27). And it was **expensive**: 1.0.3 cost 2.2 to 2.8 times what the v1 build
cost a trial, like for like, because a model refusal switched the rest of a turn to a fallback model. The cause was
named rather than guessed at, and it is narrower now — a refused request is first made again on the model that
refused it, and only a refusal that survives that goes to the fallback. **What the cost is today is not measured**:
the benchmarks are parked until the 2.x line, so this paragraph's numbers are the last ones that were run and not a
claim about the build you would install.

More: [coding](features/coding.md).
