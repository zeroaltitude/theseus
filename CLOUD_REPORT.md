# CLOUD_REPORT: aws-fixes (theseus-6hkx, snhr, rx7m, i7bz)

Branch `cloud/20261005-aws-fixes`. Each step's tests were run alone; one full gate (below) covered the tree, then the four commits were made from it (the steps touch disjoint files, but the commits were not each gated on their own).

## 1. theseus-6hkx: runaway mode yields to a raised line (9bb3df98)
- Found: `current()` returned the mark while its period held, never comparing it with the config.
- Chose the first fix (read at admission), not the start-time drop: it keeps the start path free of writes (FAST), keeps a lowered line's mark, and needs no row. `current(store, &Account, now)` ignores a mark whose line is no longer set, or whose configured line x factor exceeds the mark's. `reached` then decides again; if the figure is still past the new line a fresh mark is written. `observe` (health) uses the same rule.
- Tests (new `hands/tests_runaway.rs`, accounts rebuilt from an edited config over the same store): raised hourly line admits the same group; raised factor admits it (and `observe` finds none); lowered line still refuses, no second row; day's line removed ends its mark.
- Planted revert (`current()` blind to the config): the three "ends the mark" tests fail, the lowered-line test passes, as it should. File restored and touched.
- Existing runaway tests pass (tests_part2).

## 2. theseus-snhr: unknown [policy.aws] keys (ea31d70f)
- New `aws/policy_keys.rs`; `Aws::check_policy_keys` runs in `spawn_blocking` at the top of theseusd's `aws.check` phase task, after serving. It keeps the unknown keys for `AwsStatus.unknown_policy_keys` (absent when empty), warns once per key, and adds `policy_check_ms` and `unknown_policy_keys` to the phase detail. Phase names unchanged. The start is never failed.
- Difference from the brief: a call matches by the catalog's canonical names (`checked.service`, `checked.operation`), so an alias (`states`, `monitoring`) or a case-loose operation (`s3:listbuckets`) loads and never matches. Both are named, with the matching name in the log. I did not report "ambiguous" separately: `entry()` returns none for it, so it is named as no service.
- Health: `aws: [policy.aws] "a", "b" name no service or operation, so their lines never apply; ...` (render/aws.rs, with a test). Protocol: `AwsStatus` field (no ts(optional): ts-rs rejects it on a Vec; matches the other skip-if-empty Vecs), `scripts/long-files.txt` 2709 -> 2713 with reason; render.rs test literal gets `..Default::default()` (+1 line). `cockpit/src/protocol.gen/AwsStatus.ts` regenerated.
- Used the catalog through `theseus_aws::catalog` (re-export): no new dependency, Cargo.lock unchanged.
- Measured (debug build, 5 keys, decoding ec2, cloudformation and s3): 20.5 ms, RSS 17.2 -> 22.4 MB. Run on a blocking thread, after serving.
- Tests: typo'd operation and misspelt service named, right ones not; alias and loose operation named; status lists the keys and is absent when empty; health line. Planted revert (operations unchecked): 3 of 5 policy_keys tests fail.

## 3. theseus-rx7m: blackhole route test (f3169c92)
- `route_tables(unrouted, blackhole)` and `State.blackhole`; new test `a_blackhole_route_is_no_way_out_and_the_subnet_is_refused`: subnet refused and named, no RunTask, one route read.
- Planted revert (the `blackhole` check dropped in network.rs): that test fails. Restored and touched.

## 4. theseus-i7bz: build.sh (e73ecb3e)
- `grep -qE 'statically linked|static-pie linked'`, and `out=$(docker run ... || true)` then grep. New `infra/aws/test/test_hand_build.py` (5 tests, stdlib, scratch git repo under a temp dir, stub `scripts/build.sh`, `file`, `docker`; never `--push`).
- Proof: `python3 -m unittest discover -s infra/aws/test -p 'test_*.py'`: 41 tests OK (1 skipped, before) and the same after, including the 5 new. Planted reverts: old grep fails 3 tests; old pipeline fails 2.

## Live checks for the maintainer
1. Scratch daemon (fresh state dir, `theseus-sim fake-model --rules`, Discord and web off, an `[aws.accounts."<account>"]`) with `[policy.aws]` `"ec2:TerminateInstance" = "approve"`, `"cloudformaton" = "notify"`, `"s3:ListBuckets" = "notify"`: `theseus --socket <s> health` should show one `aws: [policy.aws] "ec2:TerminateInstance", "cloudformaton" name no service or operation...` line (not s3); `theseus --socket <s> --json health` lists them under `aws.unknown_policy_keys`; the log has one warning each; the daemon serves.
2. `infra/aws/hand/build.sh` (no `--push`): builds, prints "answers as a hand", then the image name.
3. Runaway, with a deployed hands stack and the owner's go: a line low enough that one small group enters runaway mode; raise `hourly_alert_usd`, restart within the hour: the same group runs.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (first run failed fmt only; fixed with `cargo fmt`): fmt, shape, features, clippy, cockpit, test build and the reader rule pass. The suite failed only the known L1 cases (theseus-sandbox contract tests, theseusd sandbox tests; root VM, theseus-pv6i): 66 failures, none outside them. Of the later phases I ran the protocol-types check (protocol.gen is staged and committed; protocol tests 30 passed). The gate's cargo-deny phase was not run separately. No benches (NO_BENCH). Cargo.lock unchanged.

## Docs to change at review
aws-toolset.md §3.9 (runaway: a restart onto a higher line ends the mark; the refusal's "raise and restart" now works in-period) and §3.10 (the post-serving policy-key check; health's `aws:` line).
