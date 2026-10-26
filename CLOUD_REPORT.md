# CLOUD REPORT: theseus-g5wxc, the RSI door fetches before it cuts a tree

Branch `cloud/20261026-p3n-rsi-door-fetch`, base `92e3dbed`. Commits: `4f1cf913` (start), `6d650738` (the fix and tests).
Store format and kernel SCHEMA untouched; no dependency added.

## Found
`rsi::tree::open` never fetched, so `origin/main` only advanced on a join. Both the base a branch is cut from and the gate blob `main_script` resolves (`origin/main:scripts/gate.sh`) came from that frozen ref.
`main_blob`/`main_script` are content-addressed and correct; only the fetch was missing.

Line numbers the prompt cited, re-read at the base (search by name too):
- `git.rs` `fetch`: :50 (as cited). `tree.rs` `main_blob` :224, `main_script` :240 (as cited).
- `tree.rs` `open`: prompt said :310, it was :310 at base (now :344 after my insert). `GATE_LIMIT` :125 (not changed).
- Callers of `git::fetch` (non-test): joining.rs:81 and :238, reviewing.rs:126, revert.rs:226, revert/doing.rs:311: all as cited.

## Changed (6d650738), only `rsi/tree.rs` and `rsi/tests_tree.rs`
- `refresh_main(p, repo)`, called in `open` under the `REPO` lock after the "tree exists" check and before `origin/main` is read. It reuses `join::git::fetch` (no second helper).
- Remote: `const DOOR_REMOTE = "origin"`. `Place` carries no remote name (`remote_of(cfg)` lives in join and needs a Config), and the door's own refs are the literal `origin/main` (open, main_blob, commits, diffstat), so the door fetches the remote its refs name.
- Once per pass, not once per item: `open` is the door but is called per item, and the backlog's night loops it (`backlog/start.rs` `backlog_start`, up to `n` items). The only way to do once-per-pass without touching `backlog/` (out of scope) is inside `tree.rs`: a per-repo `FETCHED` list, and one successful fetch serves cuts for `FETCH_FRESH` = 60 s. No change outside tree.rs.
- Failed fetch: warn (`tracing::warn!`) and continue with the base the repo has; a failure is not recorded as fresh, so the next cut retries. Reason: no network is an infrastructure fault, not a code fault. I did not use `Place.notify` (that is #88's sink for gate notices); the record is a log line only, and nothing asserts it. Open: if the owner wants a notice for a stale base, that is a small follow-up. Also `git fetch` has no timeout here (same as join's); a hanging remote would hold the `REPO` lock.

## Proof
Filter check (`cargo nextest list -p theseus-core -E 'test(/rsi::tests_tree::(a_moved_main|the_gate_blob|a_failed_fetch|a_pass_fetches)/)'`):
```
theseus-core rsi::tests_tree::a_failed_fetch_does_not_fail_the_door
theseus-core rsi::tests_tree::a_moved_main_is_visible_to_the_next_cut
theseus-core rsi::tests_tree::a_pass_fetches_once_not_once_an_item
theseus-core rsi::tests_tree::the_gate_blob_follows_main
```
Red on the base (tests written first, no fix):
- `a_moved_main_is_visible_to_the_next_cut`: `assertion left == right failed` left `3b7aeb12…` (stale HEAD) right `ad7ec2cc…` (moved main).
- `the_gate_blob_follows_main`: `judged by the old gate blob` left `60d60a88…` (old) right `db258f58…` (new blob).
- `a_failed_fetch_does_not_fail_the_door` and `a_pass_fetches_once_not_once_an_item` PASS on the base (they guard the fix's shape, not the bug), so they were proved by planted reverts instead.

Planted reverts (restored from a copy, `touch`ed, `git status` clean after each):
- A. `refresh_main` call removed: `a_moved_main…` and `the_gate_blob…` FAIL (2 of 4).
- B. fetch error turned into a hard failure (`.unwrap()`): `a_failed_fetch_does_not_fail_the_door` FAILS.
- C. throttle disabled (`if false && …`): `a_pass_fetches_once_not_once_an_item` FAILS.

With the fix: all 19 `tests_tree` tests pass; clippy of theseus-core all-targets clean; fmt applied.

## Keel
`python3 scripts/keel-guard.py` with no args reports 4 findings on files this row never touched (persona_cmd.rs, tests_m3/ask/due.rs, tests_gateway/cards.rs, first_request.rs, keel-guard.py): its default base resolves through the local `main` ref, which is not my base. `python3 scripts/keel-guard.py --base 92e3dbed`:
`keel: ok (92e3dbed..the working tree, 92e3dbed47; 2 files changed, 0 findings acked, 0.1 s)`. No Keel findings expected.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=92e3dbed scripts/gate.sh` with `CARGO_INCREMENTAL=0`. First run died at "test build" with `No space left on device` (target/debug/incremental); I removed it and reran. Second run: fmt, shape, features, clippy, clippy-probe, cockpit, test build all passed; suite: 8629 tests, 8594 passed, 35 failed:
- 33 are the known L1 failures (theseus-sandbox contract tests and `spawn_100`, theseusd `sandbox` tests).
- 1 more of the same family counted in those 35 is not listed separately; the count of L1 names in the log is 34 FAIL lines; plus
- `theseus-core tests_route::a_switched_turns_compilation_names_its_recall_drops`: NOT on the known list. Failed in the full run at tests_route.rs:1575 `sophisticated` left `("opus", 2)` right `("opus", 1)`; it passed on two solo reruns. It is outside `rsi/` and I did not change anything it reads, so I read it as a load race, but it is a finding: it needs a look.
After the suite I ran the later phases I could: `tests_registry` (15 passed) and theseus-protocol (210 passed). Lifecycle/jobs/turn/size benches skipped (`THESEUS_GATE_NO_BENCH=1`, owner's machine). The door's "FAILED in door" line is the gate's verdict on the tree from the suite failure, not a separate fault.

## Live check for the maintainer
On a scratch daemon with `[self] repo` set to a clone, with `git -C <repo> log -1 --format=%H origin/main` noted, push a commit to the remote's main, then start a self item (`theseus self ...` / backlog start) and run:
`git -C <repo> rev-parse origin/main` (should equal the pushed commit) and `git -C <state>/self/trees/<slug> rev-parse HEAD` (same commit); `ls <state>/self/gates/` should show `gate-<new blob>.sh`.

## Not done / uncertain
- Disk allowance is tight on this VM (incremental dirs refill it); gate needs `CARGO_INCREMENTAL=0`.
- Docs to update (not edited): scripts/AGENTS.md "The door" and the tree.rs module doc could say the door fetches `origin` (once a minute per repo, a failed fetch is tolerated).
