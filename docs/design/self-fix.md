# The self loop repairs a small fault in place (issue 36)

The owner, 2026-11-20: make the reviewer a reviewer and fixer. Before this, a self build whose gate was red was never
seen by the reviewer, and a review that rejected for something small sent the item back: a one-line repair cost a whole
new build, a whole new gate and one of the item's tries. The code is `crates/theseus-core/src/rsi/join/fix.rs` and the
module under it, and the gate's half is `rsi/pr_fix.rs`.

## The rule that does not move

Nobody approves their own work. The fixer runs on the reviewer's profile, resolved by the review's own rule and refused,
as the review is, when it is the model that built the branch (checked again on the model that answered). Its commit is
read alone by the builder's profile, which must accept it, and the approval is refused when the model that answered is
the fixer's. At review, the whole branch then needs an accept from a fresh, independent review on the reviewer's profile.

## The two points

**At the gate** (inside `self.propose_change`, after the gate's own reading, `judged`). Only a red in the change's own
work goes to a fixer: the gate's `FAILED in` phase is one of `fix::OWN_WORK` (fmt, clippy, clippy-probe, shape, test
build, suite, reader rule, protocol types). Never a keel stop, a red main (`pipeline::main_red`), a fault of the machine
(`infra::of_attempt`), a check that kept aborting, or a gate that named no phase (one cut off at its ceiling). The fix's
commit is gated again by the same reading; on green the builder's profile reads it; accepted, the attempt goes on green
as any does. Anything else ends the attempt red, as before: one try, never two, with the words of the fix in the
model's result, the item's evidence and the frozen report; the item's next attempt starts from before the fix.

**At review.** The reviewer may answer `VERDICT: fix` (offered only while `[self_fix]` is on and the head was not fixed
at review yet). The join pass reopens the item's tree on the branch at that head (`tree::reopen`), makes the fix, runs
main's gate on it, has the builder's profile read it, pushes the branch and closes the tree. The accepted fix's
`self.fix.verdict` row is the branch's new head: the next pass runs the planted revert and a fresh review on it, told
what was fixed; `fix` there is a reject. Anything else returns the item as a reject does.

## One fix

`fix::make::Job::make`: the fixer resolved, once a point (read from the ledger), a `"self"` session of its own whose
root is the item's, so `own_tree` gives it exactly the builder's confinement: its tree to write, L1 for its commands,
the item's build folder. It ends `FIX: done` and a sentence, or `FIX: none`. The git state is checked unchanged; the
CODE formats and commits what it left (`self: fix at the gate by <model> for <phase> (<item>)`). Then the bounds, each a
refusal with its own bound's name:

- `size`: over `[self_fix] lines` (added and removed together) or `files`;
- `second`: a fix at this point of this branch was tried already;
- `guarded-path`: a path the join's keel table holds (`join::keel::classify`), or the loop's own judge
  (`fix::bounds::JUDGE`: gate.sh, keel-guard.py, `rsi/join`, `rsi/pipeline`, `rsi/pr.rs` and its module
  `rsi/pr_fix.rs`, `config/self_fix.rs`);
- `tests`: main's own keel guard run on the fix's commit alone (`--base <before> --head <fix>`), AND a narrow rule that
  needs no guard: no test file on the merge-base changed at all, and in any file on the merge-base no assertion or
  test attribute removed or changed and no `#[ignore]` added. Tests the branch added are the fixer's to repair;
- `history`: the fixer moved the head, the branch or the stash, or left a merge under way;
- `not-independent`: the models above.

## Seen

Rows `self.fix.started`, `.made` (lines, files, head, the head it came from), `.refused` (the bound), `.none` and
`.verdict` (the approver); `self log` shows each in its `what`. `theseus self pipeline` names a branch being fixed
("Fixing at the gate, by …" on a building item; "at review" on a reviewing head). The frozen report has a "The fix"
section; a fix at review, whose branch's report is already frozen, has a frozen report of its own beside it
(`<branch>-fix-review.md`). The week's digest counts fixes made, approved and refused. Both model calls are `"self"`-principal turns, so
the turn's own self-budget step reserves and settles each against the day's self spend, as the review's are.

## Settings

`[self_fix] mode = "on" | "off"` (on by default), `lines = 60`, `files = 6`. A section of its own, not `[self]`,
because `config/self_improve.rs` is in the keel guard's `KEEL_FILES`: a field there is the owner's signature. With
`mode = "off"` the loop is what it was.

## What each session the loop opens is offered (round 2)

A fixer, an approver and a reviewer are `"self"`-principal sessions, so before round 2 each was offered everything a
builder is, `self.propose_change` among it (the review's finding B1: a fixer whose item's tree was open could build the
branch itself). Now each session is registered for its role before its first turn (`rsi/roles.rs`; a META record per
session, so a restart keeps it), and two layers hold, either alone:

- **What it is offered**: `TurnRunner::view_of` gives the role's session the role's own ceiling, the mechanism a place's
  ceiling already is, so the model is offered only these and the gate refuses any other call:
  - a fixer: the families `fs`, `proc`, `git`, `job` (its tree's file and command tools; `git` is `git.diff` and
    `git.log`);
  - an approver: no tool (it is given the commit's diff, its reason and the fault);
  - a reviewer: `fs.read`, `fs.list`, `fs.glob`, `fs.grep`, `git.diff`, `git.log`, and `proc.run` with `job.wait`,
    `job.read`, `job.stop`, which its instructions use for a checkout and a build under its own scratch.
  A builder is offered the whole self catalog (`self.*`, `extend.*`, `channel.*`, `task.*`, …); none of it reaches these.
- **What the builder's verb does**: `self.propose_change` refuses a role's session before anything else.

## No fixer's commit goes green unapproved (round 2)

The rule, held at the places a head is declared green, whatever path led there (`fix::unapproved`): a fixer's commit (one
that carries the `Self-Fix-Point:` trailer the code writes, or that a `self.fix.made` row names) is in a green head only
when a `self.fix.verdict` accept row names that commit. A gate that would be green is red if it is not so
(`pr_fix::gated`), and a head that reaches a review is rejected before any reviewer is asked (`reviewing::not_a_builders`,
which also rejects a head a role's session built). On the paths themselves:

- a tree resumed after a restart whose newest fix at the gate has no end (`started`, or `made` and never approved) has
  it ended as cut short (`self.fix.none`) and is put back to the head before it, before anything is committed or gated;
- a fix at review cut short is ended at the next pass, and the tree it reopened is closed;
- a fix at the gate that does not hold (a bound's refusal, red again, a rejection, no fix made, a fixer turn that
  failed) puts the tree back to the head before the fix at once, before the builder reads the red: the builder's next
  repair is made in the tree it will build on, and the next attempt is cut from where the tree stands, its edits kept
  (round 3; round 2 reset at the next attempt, which threw the builder's own repair away).

## The test bound in an ordinary file (round 2)

Beside main's keel guard on the one commit and the line rule, a fix may not change any line of a `#[cfg(test)]` module or
a `#[test]` function that was in a file on the merge-base, nor put a line right above one (`fix/regions.rs`): an early
`return;` and a `#[cfg(any())]` are refused. A changed value outside the test that its assertion reads cannot be told
from a repair by its lines; the approver is told to look for it.

## Turn limits, and a join main moves under (round 2)

`[self_fix] turn_minutes` (60) bounds one turn of a fixer, of an approver, and, while the mode is on, of a reviewer
(a review had no limit, and with the mode off it still has none). Past it the turn's execution is stopped and the turn
fails with the class `turn_limit`, a failure that does not clear by itself: a fixer's is a fix that did not hold (its
`self.fix.none`, and the attempt's red is the try); an approver's is a rejection; a review's is a `self.join.stopped`
row saying the review ran past its limit, and the third such stop in a row holds the change for the owner with the card
the review already has for such failures (`join::unreviewable`, `HOLD_AFTER`). A join whose gate starts again because main moved before
its push writes `self.join.again` (its gate's runs so far); the joining queue says "its gate has run N times: main moved
under it", the `self.joined` row says how many times its gate ran, and `TRIES` (3) bounds it, as before. The branch's
frozen report is written before the review and never again, so the count is in those rows, not in it.

## What the mode off still changes (round 3)

With `[self_fix] mode = "off"` no fix is tried, no `self.fix.*` row is written, the builder's brief and the reviewer's
prompt are main's, a review's `fix` answer is a reject, a build does not wait on the ledger's index, and a review's turn
has no limit. What still differs from the loop before issue 36, and why each is harmless:

- **A reviewer's session is offered a reviewer's tools** (the read-only file and git tools, `proc.run`, `job.*`), not
  the whole self catalog. Its instructions use no other tool, and it cannot reject a change by itself.
- **`self.propose_change` refuses a fixer's, an approver's or a reviewer's session, and a builder's call for another
  item's tree.** Neither is a path the loop takes when it works as meant.
- **A join that main moves under writes `self.join.again`, and its `self.joined` row says how often its gate ran.** True
  words; no decision reads them.
- **The green rule** (`fix::unapproved`) runs at the gate and before a review. A branch with no fix commit passes it,
  and a builder's `why` that quotes the fix commit's trailer has the line marked as quoted, so it does not trip it.
- **A ceiling may name a tool whole.** A bindings file still cannot (`check_entry`).
