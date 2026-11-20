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
