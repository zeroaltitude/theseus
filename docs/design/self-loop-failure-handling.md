# The self-improvement loop's failure handling

Issue 6 of release 1.0.7 (the loop as a machine that keeps going unattended). One night the loop made eight red builds
or rejections for two items and announced every one, learned each reason one whole gate at a time, lost two builds to a
tree that vanished under a running gate, started builds on a red main, and left an item "in progress" with no build
behind it. This is the map of what stops it now. Each rule has its tests beside its module (a tests file of its own,
so a planted revert can prove it).

**A loop that stops itself** (issue 6 of 1.0.7; `rsi/backlog/attempts.rs`, `brief.rs`, `aside.rs`, `rsi/pipeline/main_red.rs`,
  `stale.rs`, `rsi/tree_fmt.rs`, `rsi/tree_lease.rs`). **Tries:** an item has `attempts::MAX_ATTEMPTS` (3, a constant: the `[self]` config is
  a keel file); a try is a `gate:red`, a `review:` rejection, or an `ended:no-branch` with no `gate:` since its `session:`
  (read from the item's evidence, nothing stored; a gate `infra::of_attempt` calls infrastructure leaves `gate:unjudged` after its `gate:red`, which takes the try back); the try that reaches
  the cap writes `failed:cap`, gives ONE notice (`attempts::notice`), `propose` refuses the item, and the night keeps its cause out
  (`Work::cause_is_capped`, counted over every item of the cause since the owner's last `retried:owner`) until `self retry`. A red
  inside 12 hours of another for the item is not posted again (`red_tonight`; the note lists it). **The rules** a builder is told
  first are `brief::rules`; its own test run in its sandbox is not held (`own_tree::l1_in_own_tree`, called from `toolrun/order.rs`:
  a self session, an L1 call, every place inside its own tree). **Before the gate** the attempt formats each changed `.rs` file through
  `rustfmt` on stdin, never `cargo` (a tree's alias or toolchain file would run its code at L0) and never by path (a path follows
  `#[path]` modules and symlinks out of the tree): the daemon writes back only regular files inside the tree, and the commit says
  which (`tree_fmt`). **No start on a red main** (`pipeline::Seams.main`, `main_red::Recorded` reads the gate-times log the
  revert watch reads; a hold named `main_red`; no record means nothing is known and builds start; tests use `Unrecorded`). **A tree
  stays under its gate** (`tree_lease::Lease` taken by `tree::gate_with`; `tree::close` refuses, the tick's `clear` leaves it for a
  later tick). **A stale in-progress item** (in no queue: `Queues::stale`) is settled as failed (`pipeline/stale.rs`). **Set aside**
  (`aside.rs`): `self.drop` with `set_aside`: `waiting_human` plus `set-aside:owner` evidence, no remembered drop, a build under way stops;
  `self.retry` (`theseus self bring-back`) puts it back. **A join leaves no tree** (`join/guard.rs`, asserted before each phase and at the
  end of a pass, a failure in tests; the install's build tree is reaped by `install::reap_target` when its build fails or its
  `self.installed` row is written, and `reap_installed_targets` clears earlier leftovers); a hold on the slots names the trees open.

**Main moved under a finished change** (issue 20; `rsi/join/moved.rs`, `rsi/tree_main.rs`, `backlog/brief.rs`). A green branch whose dry run
  on main's tip conflicts is found at the head of the join pass, before its review is spent (the pass holds `Shared::joins`, which
  a pipeline tick does not, and it owns the conflict path's card and bound). The item goes back to `proposed`, the head is held as a
  `conflict`, and the builder's tree is made on the branch's own commits, on a new `-N` branch, with main merged in and not committed
  (`tree_main::bring_main_in`): what clashes is left in the files, and the builder's next `self.propose_change` commits the merge. Its
  first turn says so (`brief::main_moved`, read from the `main-merged:` evidence newer than the item's last `session:`). None of it is
  a try (`attempts` reads no `conflict:` or `main-merged:` evidence). The bound is the old one: a third conflict since the owner's last
  retry fails the item, and the stop's row says so. Where the working copy cannot be made (the item has a tree open, or git says no) the
  old path stands: the tree is closed and the next build cuts a fresh branch from the new main.

**A failure that is not the change's fault** (issue 20; `rsi/rerun.rs`, `rsi/reach.rs`, `tree::rerun_tests`, `infra::judge`). **An aborted check**
  (the gate's exit 75, "THE TREE MOVED", which it calls no verdict on the code) is run again at once, `rerun::ABORT_RERUNS` (2) more times; past
  that the build ends saying "its check kept aborting", which is **a red of that item**: it costs one try, as a red does, and
  is no part of the kill switch's streak (`infra::judge` reads exit 75 as red, whatever the seconds; an abort is about one working copy,
  something wrote in it while its gate ran, not about the machine, so a branch whose own test leaves a file in its checkout fails that item
  and halts nothing). The re-runs inside the bound are free. **A red suite's failures, once more**, when the gate
  failed in its suite and nowhere else, the log reads whole (one summary, a distinct `FAIL` line per failed test and that many in the
  summary, no timeout, signal or leak), every failed test's package is **outside what the change reaches**, and those tests alone, run again
  (`tree::rerun_tests`: L1, nice 19, the tree's own build folder, with the gate's `CARGO_INCREMENTAL=0` so nothing is rebuilt), all pass: then the build is green, and the report and the row's
  `numbers.reran` name the tests. Anything else is red as before, the first time. **Outside** is read from the workspace's own manifests
  (`reach::read`, never `cargo metadata`, which reads the branch's `.cargo/config.toml` and would run its compiler setting at L0): a package
  is inside when the diff edits it or when it depends on one the diff edits, through any chain, in the normal, dev and build tables
  (`[target.*]` included), and any path in no package (`Cargo.lock`, the root manifest, `scripts/`, `docs/`, the nextest config) reaches every
  package; a package it cannot place is inside. What it does not judge: the phases after a failed suite did not run, so the join's gate on
  the merge is what finally judges them, and a test that reads another package's files at run time (a source-scanning test) is not in the graph.

**A held change, checked again** (issue 20; `rsi/join/joining.rs`, `rsi/pipeline/queues.rs::join_held`, `backlog/word.rs::self_recheck`). A change
  the join stops with `numbers.held` (the keel card, a keel finding or red on the merge, no signing key, a push that lost its race, a head
  on main already) is held for the owner, and nothing looked at it again, through any number of restarts: `self retry` refused an item that
  is in progress, and a drop forbids the cause for good. Now `theseus self retry ITEM` on an in-progress item whose newest green branch has a
  held stop writes one row, `self.join.reopened` (item, branch, head, who), and the item's `rechecked:owner` evidence, in one frame, and
  kicks the pipeline. **That is all it does.** The row makes the head read as it did before the hold: a branch with its review (the accept is
  kept, as the review is once for a head and model), so the next join pass runs it as it runs any accepted branch, from the keel check
  on, under the join lock, and the merge's own gate, signature and push are the ones every join has. Nothing is skipped and nothing is
  remembered of the hold, so a change that still touches the keel posts a new card and is held again (a later `self.join.stopped` row
  beats the reopened one), and a change that is clean joins. A restart reopens nothing: only the owner's word writes the row, and the
  answer is judged as every `self.retry` is (`judge_act`). **A head already on main** (the owner joined it by hand) is found by the merge
  step; for a reopened head it closes the item as done (`joined-by-hand:` evidence, only if the item is still in progress) and writes a
  stop row with `numbers.by_hand`, which the queues and the pass read as joined, not held; its cause is not forbidden. For a head that was
  not reopened the old stop stands. **Not a try:** the evidence is none of the identities `attempts::tries` counts, and not
  `retried:owner`, which would start the count again, so the item's attempts are as they were. The merge is of the head named in the built
  row, never of the branch's tip now, so a branch moved after its review is not what a retry joins. The pipeline's line for a held item
  gives the command first (the owner's view cuts a long line at its end), and the three cards that hold a head say it too. **What it does not reopen:** the sealed hold, which sends the item back to its builder (a new head is measured afresh, and `join_held` offers only a green row written at or after the item's last `session:`, so the old head is not reopened while its rebuild runs), a conflict with main (the same), a hold that is not a join's (a review's refusal), and an item that is not in progress.

**A joined change is not what runs afterwards** (issue 20; `rsi/join/layout.rs`, `install.rs`, `verify.rs`, `words.rs`). The first
  self join to reach its install copied the build into the user's own program directory and restarted a service whose unit runs a
  root-owned build elsewhere: the daemon came back on the build before, and nothing said so. Now the install first reads where the
  service's program is (`layout::read`: the unit's `ExecStart`, else the running daemon's own path, and whether the loop can write
  that directory). **Writable:** the programs are copied there, the unit leaves a marker (`<place>/install-pending`) naming the
  merge, and restarts; the new daemon, serving, compares its `build.commit` with the marker (`self_installed_running`): the same,
  `self.installed` and `self.install.verified` in one frame with the "installed and running" notice; another, or none, a
  `self.install.mismatch` row and an alert to the owner, once. **Not writable** (or not readable): nothing is copied and no unit
  runs; the build is made and kept in its build folder for the owner's step, `self.install.not_installed` is written with the one
  command (`layout::step`), and the owner gets one action note (`verify::not_installed_notice`), once for the merge. A revert's
  install goes the same way. `self pipeline`, `self log` (`last_join`) and `health` (`joined_not_running`) read the ledger
  (`words::last_join`) and say joined and installed and running, or joined but not installed with the step, or that the install
  did not take. Nothing here writes the administrator's directory: that is a line of issue 22.

## What this does NOT do

- A self-join's own gate is not read by the main-is-red hold: its row lands in the sandbox's tmpfs `HOME` and is lost,
  so only gates run on the owner's machine are in the log (`pipeline/main_red.rs`). A tagged commit's `git describe` is
  the bare tag and names no commit, so such a row is not read either.
- The pause (`rsi/pause.rs`) is the state and its two moves; its command, its RPC method and its chat tool were not
  built in 1.0.7, as they add a method and a tool to files that were being split.
- The cap is a constant (`attempts::MAX_ATTEMPTS`) because the `[self]` config is a file the keel guards.
