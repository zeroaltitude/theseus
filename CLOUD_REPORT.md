# CLOUD_REPORT: self-join (theseus-pw1q.9)

Branch `cloud/20261013-self-join`, cut from `f129391` (main as cloned, store format 32). The work is one step,
in one commit: `b489dee`. This report is the commit after it.

## The step: the self-join

### What I found

- self-pr's `rsi::tree` has everything the join needed (the worktree, its own build tree, main's gate through the
  door, `close`), with one gap: `tree::gate` always sets `THESEUS_GATE_NO_BENCH=1`, so the join's gate on the
  merge could not run the benches. I added `tree::gate_with_benches`, and `gate` keeps its old behaviour. This is
  the only edit to self-pr's code besides making `pr::Board::place` `pub(crate)`.
- `check.rs` cannot be reused as a `task.create { check_of }` here, because a self branch's builder is a session
  with no task report to check. The reviewer reuses its admission instead. The claim (the diff, the frozen report
  and the item's evidence) is rendered through `check::render` with a `check::Claim`, in a fresh `"self"`
  session, so the reviewer sees nothing of the builder's transcript.
- The Discord courier refuses post kinds it does not know. Self-pr's `self_keel` is already in its owner's-DM arm,
  and I added `self_join` to the same arm. That is one word in theseus-discord (discord-calls' area), and
  without it the cards and the notice would never reach Discord.
- **No defect found** in the code I worked in.

### What I changed (`b489dee`)

- **Config** (`config/rsi.rs`, and the template's `[self]` block, each with a template test):
  - `reviewer_profile = "fable"`.
  - `join_lock`: unset means `<state>/self/join.lock`.
  - `install = false`.
- **Protocol:**
  - `LedgerKind`: `SelfReviewVerdict`, `SelfJoined`, `SelfInstalled` and `SelfJoinStopped`, each written by a fact in
    `fact/rsi_join.rs`.
  - One `SELF_ROWS` entry, `self.join.stopped { branch, head, phase }`. It is a new row kind, not a stored-record
    field, so the store format does not change.
- **`rsi/join.rs`**: `Core::self_join_pass(&Seams)`.
  - It asks `rsi::gate` before each phase: planted, review, keel, lock, merge, gate, sign, push and install. A refusal
    writes `self.join.stopped` naming the phase.
  - A stop that waits on the owner holds that head, marked `numbers.held`: a keel class, a conflict or a head that is
    already on main, a red gate, no key, a refused push. A halt, the lock, the planted revert and the review are
    tried again.
  - `Seams` holds the reviewer, the test runner, the installer, the lock's pace, and two test hooks.
- **`join/reviewing.rs`**:
  - **Independence.** The reviewer's model is compared with the builder session's `last_target.model`. The same
    model, an unknown builder or an unresolvable profile writes a `refused` verdict with its reason. A refusal is
    written once per reviewer model, so a new `reviewer_profile` asks again.
  - **The planted revert** runs first. Unproved is a reject, and no reviewer is paid.
  - **The reviewer's turn**, then one `self.review.verdict { branch, head, reviewer, model, verdict, planted_revert,
    reasons }`.
  - **A reject** puts the backlog task back to `proposed` (session cleared, version +1), with an evidence entry
    `review:<branch>@<head>` that holds the reasons, in the same frame. The branch is kept.
- **`join/planted.rs`**: the revert is done in code.
  - It works in a detached tree at the head.
  - The branch's new and changed tests are:
    - the Rust `#[test]` functions whose lines the branch touched, in test files;
    - any other test file, run whole.
  - They must pass at the head. Then every non-test path goes back to the merge-base's version (an added one is
    removed), and at least one test must fail.
  - The outcome is `failed` (proved), `passed`, `no-tests` or `base-red`.
  - The real runner is `cargo nextest` at nice 19 in the tree's own build tree, with `--no-tests=pass`, so a filter
    that matches nothing can never prove a branch.
- **`join/review.rs`**: the `Reviewer` trait.
  - `CheckTurn`, the real one, opens a `"self"` session and runs one turn on `reviewer_profile`'s target.
  - The verdict is the reply's last `VERDICT: accept|reject` line, with its reasons after it. A reply without one is
    a reject.
- **`join/keel.rs`**: the classes.
  - `CLASSES` is one const of path globs: sandbox, policy, aws-iam, secrets, keel (gate.sh, keel-*, long-files.txt,
    shape.sh, nextest.toml, deny.toml), and self (the kill switch, the budget, `rsi/tree.rs`, the join itself).
  - Beside it: `sealed::KEEL_PATHS`, a `MANIFEST_FORMAT` +/- line, a spend key on a config line, and main's
    `scripts/keel-guard.py --base --head`, run from main's blob. A guard that cannot run counts as a finding.
  - A hit posts a `self_join` card to the owner's DM with the hits and `git diff --stat`. Nothing in the join reads
    an answer to the card, so a `"self"` principal can never answer it.
- **`join/lock.rs`**: the lock is a `mkdir` lock.
  - Its `owner` file holds the holder on line 1, then `pid:` and `started:`. Release appends `done: <outcome> at
    <time>`, then removes the lock.
  - An existing lock whose `owner` file ends with `done:` counts as free (the outer loop's lock left in place), and
    so does one with no `owner` file after 60 s.
  - A held lock is waited for on tokio's timer, and a halt is checked between tries.
- **`join/git.rs` and `join/joining.rs`**: one join a pass, in this order.
  1. Fetch `[self] remote` (`origin` when unset), whose main is the base.
  2. Check that the head is not already on main.
  3. Dry-run with `merge-tree --write-tree`.
  4. Make a plain `--no-ff` merge as Theseus in a detached tree.
  5. Run main's gate through the door, with the benches.
  6. Sign with `commit --amend -S` using `[self] repo`'s `user.signingkey`. The tree and parents are unchanged, so
     the gate's verdict still holds, and the signature is verified to be present. With no key: a card, a held
     stop, nothing pushed.
  7. Push `<merge>:refs/heads/main`, never forced. Before pushing, it fetches again; if main moved, it redoes steps
     3 to 5, at most 3 tries (`TRIES`). An unsigned commit is refused at the push itself too.
  8. Release the lock with its `done:` line, and write `self.joined { branch, head, merge, gate, cost_usd }`.
- **`join/install.rs`**: the install.
  - With `install = true`, a detached tree at the merge and a script, run by a transient unit:
    `systemd-run --user --collect --unit=theseus-self-install-<short> /bin/bash <script>`.
  - The script runs `scripts/build.sh --profile release-thin` with `THESEUS_COMMIT=<merge>`, copies in each of
    `build.sh --shipped` by copy-then-rename, and runs `systemctl --user restart theseusd.service` last.
  - Once the new daemon is serving, `self_installed_after_serving` finds the `self.joined` row whose merge is
    `crate::build().commit` and that has no `self.installed` row. It writes `self.installed` and the notice in one
    frame: what, why, the review, the gate, the cost, and `theseus self revert <merge>`. So the notice fires once.
- **`join/tender.rs`** is started from theseusd's after-serving list, and only with `mode = "act"`, so with the mode
  off there is no task and no timer. It first gives the install's notice (retrying while the index is built), then
  runs a pass at 10 minutes and every 15 after.
- `crates/theseus-core/AGENTS.md` has a paragraph on the self-join.

### How I proved it

- **The join's tests:** 27, all pass. They run on a throwaway bare origin plus a clone, `tests_tree`'s stand-in gate,
  a stand-in gpg the repo's config names, stand-in `tests/*.sh`, and a stand-in reviewer (and the real `CheckTurn`
  on a fake provider). They cover each case the brief names:
  - `a_green_branch_is_reviewed_joined_signed_and_noticed_once`: a real check turn, run on fable in a self session,
    is given the claim and the diff. Main becomes a signed merge whose parents are base then head, the lock is
    released, a second pass does nothing, and the notice is given once and never twice.
  - `the_same_model_for_builder_and_reviewer_is_refused`: refused once, and no reviewer is asked.
  - `an_unproved_branch_is_rejected_and_its_item_returns_to_the_backlog`, and `a_branch_without_tests_is_rejected`.
  - `a_keel_class_stops_at_the_owners_card_and_waits`: covers `long-files.txt` and a stand-in keel guard's finding,
    each one card, main untouched, never retried. Each class has its own unit test in `keel::tests` (sandbox,
    policy, aws-iam, secrets, keel, sealed, self, store-format, spend-key, plus a plain change being no class).
  - `a_held_lock_waits`.
  - `main_moving_mid_join_redoes_the_dry_run`: 2 tries, and the merge's first parent is the new main.
  - `nothing_is_pushed_without_a_signature`: a card, and the push refuses an unsigned commit directly too.
  - `a_red_gate_on_the_merge_leaves_main_untouched`: also checks that its trees are closed.
  - `a_halt_between_phases_stops_it_and_says_where`: phase `sign`; the next pass is halted and writes nothing.
  - `mode_off_does_nothing`: no row, no lock, no tree, no notice.
  - Units for the lock, the verdict parser, the test discovery and the install's unit and script.
- **Under load:** the join's 27 tests at `nice -n 19` beside four busy loops at nice 0, three runs, 27/27 each (33 s,
  43 s, 33 s). The loops were killed by their pids.
- **Planted reverts:** twelve, each with the file restored and `touch`ed after, and `git status` clean of plants.
  Each test failed with its bug planted:

  | Bug planted | Test that failed |
  |---|---|
  | the same-model refusal off | `the_same_model…` |
  | the planted revert undoing nothing | `a_green_branch…` (the proved branch went unproved) |
  | `Passed` read as proved | `an_unproved_branch…` |
  | the keel hits ignored | `a_keel_class…` |
  | a held lock read as free | `a_held_lock_waits` |
  | `TRIES = 1` | `main_moving…` |
  | any signing key taken | `nothing_is_pushed…` |
  | a red gate read as green | `a_red_gate…` |
  | the `self.installed` check off | `…noticed_once` |
  | `phase()` ignoring the gate | `a_halt_between_phases…` |
  | the start's mode check off | `mode_off_does_nothing` |

  Two of my first plants did not fail their tests, and both were the wrong plant, not gaps in the tests:
  - "no undo" leaves nothing reverted, which is still unproved. I re-planted it against the green test, where it
    fails.
  - Dropping the pre-push main check is still caught: git refuses the non-fast-forward push, and the code fetches
    again and sees main moved. I re-planted it as `TRIES = 1`, which fails.
- **Clippy** `-D warnings` is clean on the four crates. To keep the shape budget, the phases are split into
  `reviewing.rs` and `joining.rs`, with no `#[allow]` or `#[expect]`.
- **Keel guard** (`python3 scripts/keel-guard.py`): `keel: ok (a branch: merge-base(HEAD, origin/main)..HEAD,
  9fa84735c0; 307 files changed, 0 findings acked, 1.4 s)`. No findings.
- **FAST:** the turn bench's `frames_plain` is 4 and `frames_tool` is 8, both within budget, so a plain turn's frames
  are unchanged. A pass with the mode off or halted reads the cached switch and returns.

### The live check the maintainer should run

I ran no scratch daemon here. Two things are missing on this VM: a model key for the reviewer's turn, and any RPC
that seeds a `self.branch.built` row or starts a pass (the tender's first pass is 10 minutes after serving). The
in-core test `a_green_branch_is_reviewed_joined_signed_and_noticed_once` does that path end to end with a fake
provider.

On the owner's machine:

1. `cargo nextest run -p theseus-core -E 'test(/rsi::join/)'`: 27 pass.
2. A scratch daemon with this branch's build and a fresh state dir:

   ```toml
   [self]
   mode = "act"
   repo = "<clone of a local bare origin>"
   remote = "origin"
   reviewer_profile = "fable"
   join_lock = "<tmp>/join.lock"
   ```

   The clone needs `git config user.signingkey <a test key>`. Then, after `theseus --socket <s> self resume`:
   - Let the builder make a branch with `self.propose_change` (a backlog item, or a self session).
   - Wait out the tender's 10 minutes.
   - It should show:
     - `self.review.verdict` (verdict `accept`, `planted_revert` `failed`) in `theseus self log`;
     - the bare origin's main as a signed `--no-ff` merge (`git log --show-signature -1`);
     - the lock directory gone;
     - a `self.joined` row with `undo: theseus self revert <merge>`.
3. `install` stays `false` on a scratch daemon. On the owner's machine, `install = true` should show a transient unit
   `theseus-self-install-<short>` in `systemctl --user list-units`. After the restart, it should show one
   `self.installed` row and one DM notice, and nothing more on a second restart.

### What is left or uncertain, and choices the owner should hear about

- **Inline tests cannot prove a branch.** The revert puts whole non-test files back, so a branch whose only new
  tests are inline `#[cfg(test)]` code in a changed source file is `no-tests` and rejected. Its tests have to go in a
  `tests_*.rs`.
- **Rust test selection is coarse.** It is by function name (`test(/(^|::)name$/)`), so a name shared across modules
  runs more tests than the branch's own. That only makes a proof harder, never easier.
- **A held head is never tried again.** That covers a keel hit, no key, a red gate on the merge and a conflict.
  Setting the key later does not re-try that head. The owner joins it by hand, or the builder makes a new head
  (with no new `self.branch.built` row, nothing notices that head either). A small "retry" verb for the owner
  would close this.
- **The notice fires even while halted.** It is written whenever the mode is `act`, whatever the switch says: it
  reports a change already installed and starts nothing. A strict reading of "stop on anything but Allowed" would
  hide it; I judged the owner would want to know.
- **Installed binaries follow `build.sh --shipped`.** The install copies that list (five today), and the
  `self.installed` row names the five from a const.
- **The restart assumes the service install.** It names `theseusd.service`, `scripts/user-service.sh`'s default. A
  daemon run bare (as AGENTS.md says the owner's is) would not restart from it.
- **The base is `[self] remote`'s main.** Self-pr's door reads `origin/main` by name, so a remote other than
  `origin` would have the gate's blob and the join's base on different mains. Keep `remote = "origin"`, or teach
  the tree its remote.
- **The reviewer can call `self.propose_change`.** It runs in a `"self"` session, so the tool is offered to it. It
  has no reason to open a tree; denying the tool to review sessions (by label or a field) would be cleaner.
- **No CLI verb.** `theseus self log` shows every new row, and `theseus self revert` is auto-revert's.
- **Docs to update at review:** Part III's item, and in `docs/status.md` the self-join row of the self-improvement
  roadmap. Also `docs/design/rsi-plan.md` §1's table row `self-join` as built, with the join's class list
  (`join/keel.rs`) beside `sealed::KEEL_PATHS`.

### Keel findings expected

None.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run with `CARGO_INCREMENTAL=0`, see below).

- **Passed:** door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build and reader rule.
- **Failed in the suite:** 4427 run, 4392 passed, 35 failed, 47 skipped.
  - **33 are the known L1 failures** on a root VM (theseus-pv6i): theseus-sandbox's contract tests, its bench's
    `spawn_100`, and theseusd's `sandbox` tests.
  - **`theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends`** is
    not on the list. Its read window came back empty (`left: []`, lines 1 to 600 expected). It passed when run
    alone. Disk on this VM was at its allowance during the run (below), and I can't tell load from disk here. It
    touches nothing of this branch.
  - **`theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`** is not on
    the list either. It took 21.5 s in the suite and passed alone in 1.2 s. It is a timing test.
- **The phases after the suite**, run by hand, all pass:
  - protocol types: `cockpit/src/protocol.gen` unchanged;
  - the turn bench (`--check --runs 5 --burst 0`): every row ok;
  - `cargo deny --offline check`: advisories, bans, licenses and sources ok.
  - The lifecycle and jobs benches are skipped under `NO_BENCH`.
- **Disk:** the first gate run died in its test build with `No space left on device`, because the session's disk
  allowance was spent and the debug build's incremental cache alone was 18 GB. I removed `target/debug/incremental`
  (a regenerable cache in the repo) and reran the gate with `CARGO_INCREMENTAL=0`.
- **Python:** bench/'s suites were not run, since this branch touches nothing under bench/.
