# CLOUD REPORT: mint-deploy-notice (theseus-pw1q.21.8)

Branch `cloud/20261017-mint-deploy-notice`, base `d86beb85`. Commits: `f533f970` (the row), `2b1c0139` (clippy line limits), and this report.

## What I found
R7's base was present (`rsi/mint/validate.rs`). Only flows validated; a pass loaded nothing. `extension_revoke_graduated` has no caller outside tests, so the graduation revoke after a join is not wired for extensions either (so not for flows).

## What I changed
- `rsi/mint/deploy.rs`: flow load after a pass (`mint_deploy_flow`, from the frozen copy, by `"self"`); MCP self-load (`mint_self_load`, `self_load_refusal`: no network, no hands, at least one declared test and all passed); limits `per_night` (last 24 h of `self.mint.deployed`) and `max_live` (a 13th waits; `mint_deploy_waiting` retries from the tender). Rust mints: no deploy here; the item keeps its `mint:<id>` tag and the brief tells the writer to call `self.propose_change` with that item.
- `extend/load.rs`: `minted_ceiling` (place ceiling, floor at least `notify`), used by `load_of` when `by == "self"`. `extend/answer.rs` + `fact/extend.rs`: `extend.acked` carries `mint` for a self-load.
- `rsi/mint/retire.rs`: tender `mint_tend` (every 6 h after serving, wired in `self_after_serving`). Rules: red attributed to its call, >20% of >=5 calls failed, <3 calls in `retire_after_days` once that old, motif by-hand runs not falling (asks R5's `mine_run`). Retire = `flow_retire` / `revoke_loaded` by `self` + `self.mint.retired`; frozen copy kept. `extend/usage.rs` got a generic `tool_calls`.
- `backlog/graduate.rs`: flows graduate by the extensions' rule (`graduate.flow:<name>:<digest>`); a tool retired for no effect is filed as a cause (`mint.retired:...`).
- `rsi/digest/notice.rs`: the morning notice (Fixed/Made/Retired/Waits for you), posted as the digest's `self_digest` post from `[initiative] morning_hour`, once a day, mode act and not halted only, scrubbed; no post when nothing landed. It rides `post_self_digest_if_due`'s tick (harness.rs is at clippy's 100-line limit).
- `rsi/revert.rs`: `theseus self revert ext:<name>` (a minted extension only) beside `flow:<name>`.
- Ledger kinds `self.mint.deployed`, `self.mint.retired` (protocol `ledger.rs`, `rsi.rs` SELF_ROWS, `fact/rsi_mint.rs`). No store format change; no config key added; `config/self_improve.rs` untouched; no dependency added. A validation now keeps its report at `<state>/self/mint/<placeholder>/report.json` (small edit in `rsi/mint/run.rs`, which also now settles MCP mints and leaves Rust mints to self-join).

## How I proved it
- New tests, all passing: `rsi/mint/tests_retire.rs` (4), `rsi/digest/tests_notice.rs` (4, the golden), `extend/tests_load/self_load.rs` (5), `rpc/tests_self_deploy.rs` (10 incl. the end to end).
- Planted reverts (each restored, touched): dropping the network check failed `self_load_is_refused_for_network_secrets_or_failed_tests`; `>= 50%` failure rate failed both `retire_on_the_failure_rate_rule` tests; dropping a notice undo failed `every_line_carries_its_undo_and_nothing_is_in_progress`.
- `cargo nextest run -p theseus-core` (TZ=America/Phoenix): 3335 passed, 2 failed under load (`tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future`, `tests_m3::a_jobs_raw_output_stays_while_its_wrapper_lingers_and_goes_when_it_ends`); both pass alone. `theseus-protocol`: 182 passed; generated TS unchanged.
- End to end uses stand-ins: a planted motif is mined by R5 and its mint prepared; the report's A/B tier is `ab::judge` over stand-in arm facts (seeded sessions cannot be replayed with a model); fixtures and dry replay keep R7's own tests. The deploy, a later scripted-model turn calling `flow__run_scripts`, the tender keeping it, the notice listing it, and the owner's revert all run through the real core.

## Gate
`THESEUS_KEEL_BASE=d86beb85 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passed door, keel (0 findings), fmt, shape, features, clippy, cockpit, test build and reader rule. Its suite phase then ran out of disk on this VM (the first run died with ENOSPC; in the rerun about 238 tests failed, including the 33 known L1 ones and many that failed because the disk filled again mid-run: the suites I re-ran alone passed). I could not rerun the whole workspace suite for lack of disk, so the maintainer should. Keel guard: `python3 scripts/keel-guard.py --base d86beb85 --head HEAD --quiet` prints nothing. Without `THESEUS_KEEL_BASE`, the gate's keel phase compared against this clone's stale `origin/main` and reported findings in other lanes' files, none mine.

## Live check for the maintainer
- `TZ=America/Phoenix scripts/gate.sh` (full suite, on the owner's machine).
- `cargo nextest run -p theseus-core -E 'test(/tests_self_deploy|self_load|tests_retire|tests_notice/)'`: all pass.
- Scratch daemon: with `[self] mode = "act"`, `[self.mint] mine = true`, run a night, then `theseus self log` should show `self.mint.deployed` with `undo: theseus self revert flow:<name>`; `theseus self revert flow:<name>` unloads it and leaves `<state>/self/mint/<name>/<digest>/spec.json`. L1-dependent paths (real MCP in L1) need a non-root daemon with a job cgroup.

## Uncertain / for the owner
- Not built as rules: "a write outside the declared scope" (nothing in the ledger can show one) and "median saving under 10%" (no live saving measure; only the pre-deploy A/B's). Stated in `retire.rs`'s doc.
- `[self.mint]` defaults differ from the design: `max_live` 10 (design 12), `retire_after_days` 30 (design 14), `usd` 2.0 (design 3.0). I changed none.
- MCP/Rust validation beyond the extension's declared tests (a dry replay for them) is not built; `kinds` still defaults to `["flow"]`. `mint_validate` still refuses non-flow kinds; MCP mints settle through `mint_self_load`.
- Graduation retire after a port joins is unwired (see Found).
- A bench-born tool cannot be a flow today (flows need a repository scope); the label path is tested on the notice.
- Small edits outside the row: `extend/usage.rs`, `rsi/pipeline/tender.rs` (one call), `rsi/mod.rs` (one field, one re-export), `rpc/confirms.rs` (call sites).

## Docs the maintainer should write
- `docs/design/rsi-plan.md` §0: "L3 Act now covers minted tools: a validated flow loads in its scope; a no-network, no-secret extension whose declared tests passed is self-loaded under the minted ceiling; a Rust mint goes through self-pr. A tender retires a live tool on failure rate, a red after its call, no use, or a motif that did not fall; one morning notice tells the owner what landed, each line with its undo. Everything stays off with `[self] mode = off`."
- `docs/status.md`: add item 21.8 under recently landed; roadmap row R8 done pending the full-suite gate on the owner's machine.
