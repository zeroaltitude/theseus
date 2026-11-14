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

## What this does NOT do

- A self-join's own gate is not read by the main-is-red hold: its row lands in the sandbox's tmpfs `HOME` and is lost,
  so only gates run on the owner's machine are in the log (`pipeline/main_red.rs`). A tagged commit's `git describe` is
  the bare tag and names no commit, so such a row is not read either.
- The pause (`rsi/pause.rs`) is the state and its two moves; its command, its RPC method and its chat tool were not
  built in 1.0.7, as they add a method and a tool to files that were being split.
- The cap is a constant (`attempts::MAX_ATTEMPTS`) because the `[self]` config is a file the keel guards.
