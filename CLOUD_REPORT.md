# CLOUD_REPORT: self-repair-fixes (theseus-qh2tt, theseus-czlzg)

Branch `cloud/20261019-self-repair-fixes`; code commit `7a4a12b` (steps 1-4 in one commit: they share `pipeline.rs`, the
pipeline test rig and the AGENTS.md lines, so I did not split them; the subject names both issues).

## Step 1: the tree first, its path in the brief (theseus-qh2tt)
- **Found.** The tree opened lazily in `self.propose_change`, so the first turn had no path. `backlog_start` opened the
  session with no `dir`.
- **Changed.** `rsi/backlog/start.rs`: `self_tree_open` (find the open tree, else `tree::open`) runs before
  `open_session_as`; the session's `dir` is the tree; the Started objective ends "Your tree is <path> on branch <branch>;
  read and edit there, by absolute path." (the task record's own objective is unchanged). Failure: `NoTree` /
  `Night.held`; the item stays proposed, and the pipeline's next start is held as `tree` with the reason, once
  (`pipeline_held`, shared with the guard's holds). The night's own start path (`backlog_starts`) shares the code, so it
  opens a tree too. A mint start (`backlog_mint_start`) is separate and opens no tree (out of scope).
- **Also found (design choice).** Every started item now has a tree, so a halted build's leftover tree filled its own
  slot and blocked its restart. `pipeline_hold` no longer counts a ready item's own tree as a taken slot
  (`waiting` argument; `view.rs` passes `q.ready`).
- **Proof.** `rsi::pipeline::tests_trees::a_started_item_has_its_tree_open_and_named_before_its_first_turn` (KEY; real
  tree off a throwaway repo, session dir, objective text), `an_item_whose_tree_cannot_be_opened_is_held_with_the_reason`.
  The pipeline test rig and the two backlog rigs (`tests_self_backlog`, `tests_self_graduate`) now use the
  `tests_tree::fixture` repo; `Rig::built` closes the tree as `propose_change` does on green.
- **Plant (KEY).** `rsi/backlog/start.rs`, `self_tree_open`: replace `None => tree::open(&place, item, now_ms),` with
  `None => Ok(tree::Tree { slug: tree::slug(item), branch: "self/x".into(), dir: place.trees().join(tree::slug(item)),
  target: place.targets().join(tree::slug(item)) }),`. Fails: `tests_trees.rs:47:46` (`no tree open at start`).

## Step 2: read-only in the trees root (theseus-qh2tt)
- **Changed.** `rsi/own_tree.rs`: `open_to(rt, session, tool)` gives `ToolPolicy::decide_in` the trees root for a
  `Read`-class `fs.*`/`git.*` tool of a `"self"` principal, the session's own tree for a writing `fs.*` tool, else none;
  `toolrun/order.rs` calls it (the lsp edit gate keeps `own_tree::of`). Module doc rewritten. The catalog's read-only
  tools are fs.read/glob/grep/list and git.log/diff (there is no git.status).
- **Proof.** `tests_pr::own_tree::a_self_session_reads_the_trees_root_without_asking` (fs.grep and fs.list of the root,
  git.log in its own tree run without asking; a link to the store asks; an operator's grep asks);
  `nothing_else_is_open_to_it` (another tree's fs.write asks). The existing `policy::tests` own-tree test still passes.
- **Two existing assertions changed with the rule** (not weakened: same cases now assert a write asks):
  `nothing_else_is_open_to_it` no longer lists "another task's tree" among READS that ask; and
  `a_session_that_is_not_self_asks_in_the_tree_too` uses an `fs_write` for the "self session of no task" half.
- **Plant.** `own_tree.rs`: `ToolClass::Read if name.starts_with("fs.") || name.starts_with("git.") =>` to
  `ToolClass::Read if false && name.starts_with("fs.") =>`. Fails `tests_pr.rs:588:9` (the root search asks).

## Step 3: an ended item leaves no tree (theseus-czlzg)
- **Changed.** `pipeline/queues.rs`: `closed` = failed or done items with a tree or build tree on disk, minus items whose
  branch is reviewing, joining, held or joined, and any `.keel`-marked; `pipeline.rs` `pipeline_moves` calls `clear` on
  each (one `cleared` row, reason `closed`). `clear` now also deletes the branch when `tree::commits_beyond_base` is 0
  (`tree::delete_branch`, `self/*` only); a branch with commits stays. This applies to every `clear` (drop, ended, join).
- **Proof.** `tests_trees::an_item_failed_by_the_sweep_leaves_no_tree` (real sweep: executions cancelled,
  `backlog_night` fails the items; sweep alone leaves the trees; the tick closes both, `trees_open` falls to the one the
  third item then starts in; branch with a commit stays, empty one gone), `a_building_items_tree_is_left_alone`.
- **Plant.** `pipeline.rs`: `for e in &q.closed {` to `for e in q.closed.iter().take(0) {`. Fails `tests_trees.rs:121:5`.

## Step 4: `theseus self retry <item>` (theseus-czlzg)
- **Changed.** Protocol `SelfRetryParams/Result` (`rsi.rs`), `method::SELF_RETRY` (added to an existing line; `lib.rs`
  line count unchanged), `ts.rs` (existing line), generated `cockpit/src/protocol.gen/` regenerated; `Core::self_retry`
  in `backlog/word.rs` (`Act::SelfBacklog`, so a job's shell is refused; Failed -> Proposed, session cleared, evidence
  `retried by the owner`, ranks reassigned, pipeline kicked; any other state refused "only a failed item can be
  retried"); `rpc/rsi.rs` route; CLI `SelfCmd::Retry` and `OPERATORS` (88 -> 89). Discord has no `self move`, so no slash
  command. AGENTS.md: theseus-core (backlog start, own tree, retry) and theseus (CLI).
- **Proof.** `tests_trees::retry_queues_a_failed_item_and_refuses_the_rest` (building, proposed, done refused with words;
  from a job refused and unchanged; then the pipeline starts it again in a new tree).
- **Plant.** `word.rs`: `if w.tasks[&id].state != TaskState::Failed {` to `if !w.tasks[&id].state.is_closed() {`.
  Fails `tests_trees.rs:222:35` (a done item was accepted).
- A retried item that already had two review rejections would be parked again by the pipeline (existing rule): said here,
  not changed.

## Checks
- `cargo fmt --all --check`, `bash scripts/shape.sh` (ok), `cargo clippy -p theseus-core -p theseus --all-targets -- -D
  warnings` clean (after splitting `closed()` out of `pipeline_queues`, which hit the 100-line limit).
- `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(/rsi|self|own_tree|backlog|pipeline/)'`: 397/397.
  `... -p theseus-core -p theseus`: 4189/4189. `-p theseus-protocol`: 185 passed (writes the TS).
- `python3 scripts/keel-guard.py --base 64b3b10 --head HEAD --quiet`: nothing.
- No planted-revert run under load: none of these is a race.
- **Full gate** (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): door, keel, keel tests, fmt, shape,
  features, clippy, cockpit, test build, reader rule passed; **suite failed** on the known VM cases only:
  theseus-sandbox contract tests and `bench::spawn_100`, theseusd `sandbox` tests (L1 refuses a root daemon,
  theseus-pv6i); plus `theseus-discord` `tests_task_threads::*`, `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule`
  and `tests_task_channels::a_short_task_gets_the_thread_it_always_did_and_no_channel` (each ~101 s: they hang on
  this VM; the same tests fail the same way on the base commit 64b3b10, checked), and `theseus::herdr
  a_line_sent_on_to_main_does_not_leave_the_pane_sending` (20 s timeout; passed on a rerun alone). I did not run the
  phases after the suite (benches) here; the protocol-types phase is covered by the protocol run above.
- The VM's disk allowance ran out twice during builds (incremental cache, test binaries); I deleted only `target/`
  artifacts. Set `CARGO_INCREMENTAL=0`.

## Live check for the maintainer
With self-improvement on (`[self] mode = "act"`, `repo` set):
1. `theseus self backlog` shows a proposed item; wait for the pipeline start (or `theseus self pipeline`), then
   `theseus history <its session>`: the first turn's input ends "Your tree is <state>/self/trees/<slug> on branch
   self/<date>-<slug>; ..." and the transcript holds no question for an fs.grep/fs.list of the trees root or git.log.
2. `theseus self pipeline` after an item fails: no tree under `<state>/self/trees`, and the next ready item starts.
3. `theseus self retry <failed item>` then `theseus self pipeline`: the item is ready. `theseus self retry <proposed
   item>` refuses with "only a failed item can be retried". Inside a job's shell it is refused.

## Docs to change at review (not edited here)
Part III item and `docs/status.md`: the tree-first start, the read-only trees-root rule, the closed-item sweep, `self
retry`. `docs/design/` roadmap rows if listed.

## Open / uncertain
- Opening the tree before L1 is known to run: an item on a host where L1 cannot run now opens a tree, its build fails,
  and the tick closes it (step 3), instead of failing at `propose_change` first.
- The two backlog test rigs leak a small temp repo per core for the process (`mem::forget` of the fixture).
