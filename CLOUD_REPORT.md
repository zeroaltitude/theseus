# CLOUD_REPORT: self-tree-acts (theseus-kq4z4)

Branch `cloud/20261019-self-tree-acts`, base `5e0bd195`. Code commit: `08c5efb6`.

## Found
The own-tree rule (`rsi/own_tree.rs`) already let the self session's `fs.edit` itself through. What asked was the edit hook's
language-server start (`lsp/edits.rs` `gate` -> `judge_start`): it judged the edit a second time as `proc.run` on the tree's
root, with no `own`, so the state dir's approve-path hold asked. I reproduced the owner's exact reason text with a planted
revert (below).

## Changed (`08c5efb6`)
Option (a), per the maintainer's lean: a self session's `fs.write|edit|patch` in its OWN tree starts no language server.
I found no confinement of the server (it is spawned by `lsp::Board` through `children::spawn`, L0), and rust-analyzer runs
build scripts and proc macros, so (b) would run the model's code unconfined.
- `lsp/edits.rs`: `gate` takes `own` and skips `judge_start` for a written resource inside it (`in_tree`, canonical on both
  sides, so a link out of the tree is not in it). `attach`/`after_edit` have `_in` forms (`attach_in` takes `Who {session,
  own}`) whose `targets` start nothing inside `own`; a server already up still gives diagnostics; when nothing else
  answers, `meta.lsp.not_started` = `"self tree"`. The old `attach`/`after_edit` stay as thin wrappers (tests use them).
- `toolrun/order.rs`: passes the already-computed `own_tree` to `edits::gate` (no other change; `own` still only computed for `fs.*`).
- `rpc/explain.rs`: the `lsp_start` condition's `when` names the self-tree case.
- `tests_lsp_edits.rs`: one call site gets the new `None` argument (no test weakened).
- `rsi/tests_pr.rs`: module `own_tree_edits` (5 tests). `crates/theseus-core/AGENTS.md`: own-tree paragraph names the case.
- No config key, no store format change, no new dependency. `L2` (`lsp.*` tool's own start) is unchanged: `lsp.hover` in the
  tree is still held by the approve path as today (`own` is only given to `fs.*`).
- Keel at the self-join (`rsi/join.rs`, `rsi/join/keel.rs`) and `toolrun/guarded.rs` are untouched: the join's keel and gate
  still judge every self branch.

## Proof
- `cargo nextest run --workspace -E 'test(/own_tree/)'`: 11 passed (4 old `own_tree` + 5 new + 2 lsp-adjacent names).
- Wider: `package(theseus-core) & (rsi:: | lsp | explain | policy | gate | tests_m3 | output)` with `TZ=America/Phoenix`:
  609 passed, 0 failed.
- `cargo fmt --all --check` ok; `scripts/shape.sh` ok; `cargo clippy -p theseus-core --all-targets -- -D warnings` clean.
- `python3 scripts/keel-guard.py --base 5e0bd195 --head HEAD --quiet`: empty output, exit 0. (Against a newer `origin/main` it
  lists main's later Google files as erosions: that is the base moving, not this branch.) My first draft had a
  `#[allow(clippy::too_many_arguments)]`; the guard caught it and I removed it by shrinking the signature before pushing.
- Not run: the full `scripts/gate.sh`, `cargo-deny` (skipped per the task), benches.

## Planted reverts (replay: edit, run `cargo nextest run --workspace -E 'test(/own_tree/)'`, restore, `touch`, `git status`)
1. KEY: `lsp/edits.rs` `gate`: delete the line `|| in_tree(own, &r.path)`. FAIL
   `own_tree_edits::a_self_tasks_edit_in_its_own_tree_does_not_ask`, `left: Some("edit …/lib.rs: this call starts rust-analyzer
   on …/trees/sails (`theseus-lsp-fake`), a program, judged as proc.run: … proc.run — approve (… is on the approve list)")`:
   the owner's reason.
2. `rsi/own_tree.rs` `of`: replace the `principal_of(..) != SELF_PRINCIPAL { return None; }` block with `let _ = &execution;`.
   FAIL `own_tree_edits::a_user_sessions_edit_in_a_self_tree_still_asks` (that test's session has operator principal but a
   self-origin task; `root_of` alone does not stop it, so the execution-principal check matters).
3. `rsi/own_tree.rs` `of`: `Some(trees.join(tree::slug(&root)))` -> `{ let _ = root; Some(trees) }`. FAIL
   `own_tree::nothing_else_is_open_to_it` and `own_tree_edits::a_self_tasks_edit_outside_its_tree_still_asks`.
All restored from git and touched; `git status` clean afterwards.
- `a_self_tasks_proc_run_and_lsp_start_in_its_tree_are_judged_as_today` and `explain_names_the_self_tree_rule` assert the
  unchanged decisions; they guard against loosening `proc.run`/`lsp.*`, with no plant of their own (`own` is not passed there).

## Live check for the maintainer (owner's machine)
Re-run a self task that edits in `<state>/self/trees/tsk-<id>/`. In the ledger, its first `fs.edit` should show
`tool.notified`/an open decision with NO `tool.confirm_requested`, and the call's result `meta.lsp.not_started == "self tree"`.
Watch that a later `fs.edit` of a file outside the tree, or in another task's tree, still produces `tool.confirm_requested`
with reason `fs.edit — approve (… is protected: <state> is on the approve list)`. `theseus policy explain --session <self sid> --tool fs.edit`
shows the `lsp_start` condition naming the self-tree case.

## Uncertain / for the owner
- Self-repairs get no rust-analyzer diagnostics in their own tree (only a server already up for that root would). That is the
  price of keeping unconfined code off the host; a future step could start the server inside L1.
- `policy.explain` models a probe call, not a session's own tree (its `At.session` is None), so it names the rule in the
  condition text but does not itself decide an own-tree edit.
- Docs to change at review: spec Part III item for theseus-kq4z4 (this step), `docs/status.md`.
- Environment note: this VM's disk allowance filled twice during the plants; I deleted only rebuildable files under `target/`.
