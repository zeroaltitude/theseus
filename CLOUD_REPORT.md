# p2-card-drafting (theseus-ly75u): cloud report

Branch `cloud/20261023-p2-card-drafting`, base `origin/main` at `23328545`. Started 2026-10-23 22:23 UTC.

**Store format: unchanged (49).** No new scope kind, no stored field: the counts are on the run's result (a
wire type, not a stored record), and a re-scope writes a standing record and a proposal of the shapes that
exist. **Protocol `lib.rs`: untouched.**

## Design

The six findings, and where each is fixed. Every option is weighed on three things: does it work with no
model (testable, free), does it cost tokens, and can it be wrong in a way the owner would not see.

### 1. Agent-setup files are not facts about the owner

Options:
- **(a) A code rule before the call** (`evidence.rs`): drop the lines that are an assistant's instructions to
  itself before they are clustered. Free, deterministic, testable with no model; it never sends those lines
  anywhere. It can miss an instruction worded in a way the rule does not know.
- **(b) An instruction to the model**: "do not state an assistant's instructions to itself as the owner's".
  Costs a sentence; catches wordings a rule misses; but the lines are still sent and paid for, and Haiku may
  ignore it.
- **(c) A check after the call**: compare the statement with its lines. Needs the same rule as (a), applied
  later, after the tokens were spent.

**Pick: (a) plus one line of (b).** The rule has two parts, both on the lines alone, in `evidence.rs`:
- A line **addressed to an assistant about running itself** is no evidence in any episode: it speaks to "you"
  as the reader (it starts `You`, `Your`, `You're`), or names the machinery of a session (read this file, each
  session, every session, wake up, bootstrap, heartbeat, memory file, daily log, long-term memory, your human).
  A note the owner wrote to agents in the imperative ("Never commit before the gate is green") is not
  touched: an order the owner gives is evidence of his preference; what the rule drops is the assistant's own
  scaffolding.
- An episode whose **file is an assistant's setup file** (by its name: soul, identity, bootstrap, agents,
  memory, heartbeat, tools, or a dated daily log) keeps only the lines whose subject is the owner (they start
  with the owner, the user, he, she, they, his, her, their, or a third-person verb such as "Prefers" or
  "Lives"). A USER file is not a setup file by name: its owner lines stay, and its assistant lines go by the
  first part.

What the rule drops is counted as `CardProposeResult.setup`, beside `withheld`, and the CLI's propose line says
it. The instruction line tells the model the same thing for what slips through.

### 2. One-task instructions keep a narrow scope

Options:
- **(a) A code rule before the call**: a line that says "in this task", "for this repo", "this branch", "this
  run", "this PR" is dropped. Free and certain for the words it knows; blind to a merge procedure that never
  says "this".
- **(b) The model marks it**: the tool's `scope` takes one more value, `one_task` ("the lines speak of one task,
  one repository, one branch or one run"), and code maps it to the narrowest scope that exists, or drops it.
  One enum value in the schema; catches a procedure by what it is, not by its words.
- **(c) A check after**: the same phrase rule over the statement and its lines.

**Pick: (a) and (b).** (a) drops the lines with an explicit one-task phrase before clustering (counted in
`one_task`), so "no builds in this task" never reaches a call. (b) is the model's mark for the rest. The
mapping: **no standing scope is narrower than everywhere and wider than a session for "one repository" or "one
task"**, and the card does not know the session the words were said in (an imported episode is not a live
session), so a `one_task` statement is **dropped and counted** in `one_task`, never proposed `everywhere`.
A follow-up could map it to a repository scope if one is ever added (see 6).

### 3. Meaning kept, wording full

The instructions say: keep who acts and on whose behalf exactly as the source says (an instruction not to
speak *on the owner's behalf* must never become one not to speak *as oneself*); each statement says what to do
or not do and when, in words a turn could act on; a statement that cannot be made specific is not given.
Two cheap checks after parsing, no model call:
- **On whose behalf**: a statement whose cited lines speak of acting on someone's behalf ("on behalf", "on his
  behalf", "in his name", "speak for") and which itself does not keep that phrase is dropped: the subject moved.
- **Thin**: a statement with fewer than two content words (by `cluster::words`: "Have a personality" has one)
  is dropped.
A negation-flip check (the statement negated, its lines not) was considered and **left out**: restating
"Never commit before the gate is green" as "wants the gate green before any commit" is a correct flip of
polarity, and the check would drop it.

### 4. No contradictory pair in one batch

Options:
- **(a) A code check by shared subject words**: two statements that share content words and differ in negation.
  "Ask before acting" and "acts on his own" share no word and carry no negation: the finding's own pair slips
  through.
- **(b) The tool says it**: each statement may name the clusters (`K2`, …) of the batch it conflicts with. The
  model has every cluster of the call in view, so this costs one optional field and no call.
- **(c) A second call** to judge pairs: costs a call per batch, for a case that is rare. Not needed.

**Pick: (b), settled in code** (`run.rs`, `settle_conflicts`, pure): when two kept statements of one call
conflict (either names the other), the one whose cluster has more support stays (a tie: the better-ranked
cluster), the other is not offered, and the run's result notes it (`CardProposeResult.conflicts`, one line
each: "kept '…' over '…'"). Limits: a conflict across two calls of one run, or with a record kept earlier, is
not caught by this (a live record's restatement is already dropped as a repeat).

### 6. The owner sets the scope

**Scope words**: one function, `StandingScope::from_words` (protocol `standing.rs`), reads exactly the forms
`words()` writes: `everywhere`, `place:<target>`, `surface:<name>` (one of `SURFACES`), `session:<id>`,
`persona:<key>`; anything else is refused with that list. The CLI's `standing add --scope` now calls it too, so
there is one parser (its words unchanged).

**Surfaces it changes**:
- `card keep <id> --scope <words>` and `card edit <id> --scope <words> <text>`: `CardAnswerParams.scope:
  Option<String>` (serde-default, skipped when `None`). The daemon parses it before anything is written; an
  unknown form is refused and nothing is written; the record and the proposal carry the owner's scope.
- **A kept record's scope**: `card scope <id> <words>`, which is `card.answer` with the option `scope` on a
  *kept* proposal. The smallest path that is one act: `standing.add` with `replaces` (the record the proposal
  kept; the add ends it in the same frame), the same kind, the same words (the kept ones) and the same source
  `card:<id>`; then the proposal is written again with the new record's id and a `card.answered` row whose
  option is `scope`. Same authority as `card.answer` (the owner, from a private place; the CLI already refuses
  `card.answer` inside a job). No new method, so protocol `lib.rs` is untouched.
- **A coding scope: not added.** The owner's case was a record about merge conflicts that should hold for
  coding work. The kind of work is the route pack's `mode` (`routine_coding`, `deep_coding`, `review`), and
  at this base it is decided *after* the turn's standing block is fixed (`turn.rs` stands the spec before
  routing; only a switch re-stands it), and not at all while the route pack is in shadow or the judge is off.
  A `mode:` scope would therefore hold on no turn of a daemon whose routing is in shadow: a record the owner
  kept that silently never shows is worse than one a little wide. The existing kinds that fit today: a
  coding record scoped `surface:terminal` (the CLI and the TUI, where the owner's coding sessions run) or
  `persona:<key>` for a coding persona. **OWNER CALL** (not a message change, a design one): a `work:coding`
  scope is worth adding once the route verdict is read before the standing tail is attached (it would ride the
  tail block, as a surface record does, so the cached prefix stays); it needs store format 50 and the turn's
  mode at `standing_here`.

### 5 (the batch's other checks) in one place

Order of the drafting checks, cheapest first: the setup and one-task line rules (before clustering, no
tokens); the call; `parse` (uncited, paste, one a cluster, `one_task`, thin, on-whose-behalf); the run's
conflicts; the repeats of live records and earlier proposals.
