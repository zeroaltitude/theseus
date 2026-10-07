# Cloud report: bench-fair (theseus-n6p5 and five more)

Branch `cloud/20261006-bench-fair`. Commits: ea6aa6aa (Theseus's profile effort), 78a620d7 (report: bpeg, p6kd q2), 08a76b79 (arms: n6p5 for Claude Code and Pi, 7gir.23, sgpx, p6kd q3 and turns, a5we). Steps 1 and 2 share the record's `stamp` and steps 3, 5 and 6 share files, so they are one commit rather than six; the three commits each pass their tests.

## What I found and changed
1. **n6p5 effort medium.** `effort = "medium"` in `[profiles.bench]`; `a_bench_call_asks_for_the_models_whole_output` asserts `output_config.effort == "medium"`. `MeasuredClaudeCode` defaults `reasoning_effort`, `MeasuredPi` defaults `thinking`, both to `measure.EFFORT`; an `--ak` wins; a host's `CLAUDE_CODE_EFFORT_LEVEL` no longer picks it. Every record gains `effort`; Pi's also `effort_ran` (`thinking_level_change`, answers' `providerThinkingLevel`).
2. **7gir.23 pin.** `claude_code_agent.PINNED_VERSION = "2.1.290"`. Tests cover both install branches (bootstrap.sh and npm) and `--ak version=`. `measure.record_version` writes `get_version_command`'s output to `agent/version.txt` at install for all three arms; records gain `version` and `version_asked`.
3. **sgpx stop at timeout.** `measure.stop_agent_script` (plain sh over /proc, finds the tree before any signal, SIGTERM, SIGKILL after 3 s, skips zombies), run as root on `CancelledError` before the sampler's stop, then re-raised. Used by both arms; the async arms get it through `super().run`.
4. **bpeg.** `load_trial` names `PiProviderError` or `PiAbortedError` from `end.stop_reason` when Harbor recorded no exception (Harbor's own wins); both in `draft.ENDINGS`.
5. **p6kd.** Tests for q2 (trial past `max_turns` alone) and q3 (record from `pi.txt` alone). `pi_end` gains `turns` (answers not `error`), `limits.turns`; `over_turns` uses it; `answers` and `model_calls` unchanged.
6. **a5we Pi offline.** `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1`, `PI_TELEMETRY=0` in Pi's run env, through an `exec_as_agent` override matching `pi --print` or `pi --mode rpc`. The RPC match matters: `PiAsync` rewrites the command before calling `super()`, so a `--print`-only match would have left the async arm online (test added). Pi 1.0.4's docs (`docs/environment-variables.md`): "`PI_OFFLINE` | Disable automatic network activity, including model catalog refreshes". Its code (`model-runtime`) gates only catalog refresh, version check, package updates and tool downloads (`fd`, `rg`) on the flag; the Anthropic provider chunk never reads it.

## Proof
- Python before: harbor 89, report 36, async 36+44, all green. After: system python3 harbor 111 (25 skipped), report 39, async 47 (11 skipped); Harbor venv (3.12, harbor 0.23.0) harbor 111, report 39, async with ASYNC_HARBOR=1 47 (1 skipped), all OK. (One run of `test_sampler`'s cost-per-process bound failed while a cargo build loaded the VM; it passed alone and on rerun.)
- Records from the existing fixtures (`/tmp` script, main against branch): Claude Code's identical; Pi's differ only by added keys `effort_ran`, `end.turns`, `limits.turns`. Stamped keys `effort`, `version`, `version_asked` are added by the agents.
- Planted reverts, each failing: toml line removed (`a_bench_call_asks_for_the_models_whole_output`); Claude and Pi effort defaults (their effort tests); Claude pin removed (install test); stop removed in Claude and Pi (`test_harbors_timeout_still_stops_the_sampler` each) and in both async arms (`AsyncTimeoutStopsTheAgent`); `PI_OFFLINE` dropped (`test_the_command_line_model_and_key`); RPC match narrowed (`test_pi_async_runs_pi_offline_in_rpc_mode_too`); `end` read removed (report and draft tests); caps counting `over_budget` only (new q2 test); `end` cut to `pi_end(entries)` (stream-only test); every answer counted (retried-failure test plus three more); SIGKILL removed and descendant search removed (real /proc tests in `test_measure.py`, which use their own process name, never `pi` or `claude`).
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): fmt, shape, features, clippy, cockpit, reader rule pass; suite fails only on 133 known L1 root tests (theseus-sandbox contract, theseusd sandbox). I ran the later phase that applies without benches, theseus-protocol (31 passed). Benches skipped by design. `bench_profile`: 6 of 6 pass.
- `cargo deny fetch` and the deny phase were not separately run beyond the gate.

## Live check (the maintainer's)
```bash
export PYTHONPATH=$PWD/bench/harbor HARBOR_TELEMETRY=0
for a in theseus_agent:Theseus claude_code_agent:MeasuredClaudeCode pi_agent:MeasuredPi; do
  .venv/bin/harbor run -d terminal-bench@2.0 -i fix-git -a $a -m anthropic/claude-sonnet-5-5 \
    --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name fair-${a%%:*}; done
```
(the Theseus arm takes no `--ak`; drop them there.) Each `agent/efficiency.json` should show `effort: "medium"`; Claude Code `version: "2.1.290"` equal to `version_asked`; Pi `version: "1.0.4"`, `effort_ran` all medium. Timeout: rerun with `--agent-timeout-multiplier 0.05` (small enough to time out) on the Claude Code and Pi arms; while the verifier runs, `docker exec <c> ps` shows no `claude` or `pi`, and the session log has no entry later than about 3 s after the cancel.

## Left, uncertain
- `report/charts.py` `ARMS` has no `pi` key, so `draft.py harbor --arm pi=…` refuses it; out of scope, flagged.
- Docs to change at review: none beyond bench/README.md (done); `docs/benchmarks.md` may want the effort and pin noted.
- The stop runs `exec(user="root")`; I could not run it against Docker here.
- The version read is best-effort: a failed read leaves `version` null.
