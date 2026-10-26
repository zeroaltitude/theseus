# CLOUD REPORT: theseus-3cf5c, half 1 (a whole-call timeout switches models)

Branch `cloud/20261026-p3n-timeout-switch2`, base `92e3dbed`. Code commit `905ab6bb`.

## Half 2 (not touched)
Present on the base: `Retries.transient` defaults to 2 (`config.rs:1235`, template `theseus.example.toml:88`). Left alone. The previous attempt's
branch (`origin/cloud/20261025-p3m-timeout-switch`) does not exist on the remote, so its report could not be read; I worked from the task text.

## What I found
- `retry_after` is called from `turn_body` at `turn.rs:1950` (base), as the task said; the `fallback: &'a OnceLock<Target>` slot is in the
  `turn_body` tuple (`turn.rs:1684-1711`; named `fallback`, second element of the destructured tuple at :1707).
- The seam: `fall_back` needs a `ModelResponse` and a refused `Node`. A timeout has neither. `RequestSpec::fallback` was
  `Option<(String, String)>`: the second string is the refused answer id, read as `retrying` by `render_kept` (compiler.rs ~:947), `ring/sums.rs:53`,
  and from there `kept::render` (`kept.rs:192`: `a.retrying.is_some()` bypasses the kept render, and bypassed renders are not counted or stored).
  An empty id would therefore have set the kept render aside on every loop after a timeout switch, with nothing to leave out.

## What I changed (905ab6bb)
- `turn/fallback_step.rs`: target selection moved into a shared private `switch_model` (catalog lookup, `max_tokens.min(cap)` clamp, `TurnFallback`
  record, `provider.fallback` row, `t.fallback = Some`, one-way door `t.fallback.is_some()`). `fall_back`'s behaviour on a refusal is unchanged
  (the existing 9 `tests_fallback` tests pass). New sibling entry `fall_back_on_timeout` (honours `[model.retries] refusal`; not while stopping).
- `turn/retry_step.rs`: in `retry_after`, a `Total` timeout with a transient failure and no whole-call retry spent tries `fall_back_on_timeout`
  first; on a switch it spends the turn's whole-call retry (so a fallback that is cut too ends the turn: no chain, no same-model retry on the
  fallback) and records `LoopCut` decision `timeout_fallback`. No wait, since it is a different model. Otherwise the existing path is untouched.
- **Refused id**: `RequestSpec::fallback` is now `Option<(String, Option<String>)>`; the timeout path sets `None`. Readers changed:
  `compiler.rs` (field + 2 lines), `compiler/ring/sums.rs` (1 line). `fact::turn::FellBack.refused` is `Option<&str>` (row shows `null` for a timeout).
  Test fixture `tests_thinking_writer.rs` updated to the new type.
- **Category**: reuses `TurnFallback.category`, set to `"timeout"` (a refusal's is its provider category). **No stored record gained a field; no format
  bump; no new config key, no new model list.** `MANIFEST_FORMAT`/kernel `SCHEMA` untouched.
- `turn.rs`: the single call-site line, `retry_after(t, &failure, &mut retried)` -> `.retry_after(t, &failure, &mut retried, &mut spec, fallback)`.
  **Not line-neutral:** rustfmt's chain width (60) wraps it onto 4 lines. `wc -l turn.rs`: 3510 before, **3513** after (ceiling 3525, no ceiling touched).
  Diff:
  ```
  -                    if self.retry_after(t, &failure, &mut retried).await? {
  +                    if self
  +                        .retry_after(t, &failure, &mut retried, &mut spec, fallback)
  +                        .await?
  +                    {
  ```
- Tests: `tests_fallback/timeout.rs` (child module of `tests_fallback.rs`, which gained a `try_turn` helper returning the Result; `turn` wraps it)
  and `compiler/tests_kept.rs` (`compile_on` helper + one test). Reused the existing scripted `FakeProvider`.

## Proof
`cargo nextest list -p theseus-core -E 'test(/tests_fallback::timeout/)'`:
```
theseus-core tests_fallback::timeout::a_stream_idle_cut_does_not_switch_models
theseus-core tests_fallback::timeout::a_timeout_fallbacks_requests_keep_the_whole_transcript
theseus-core tests_fallback::timeout::a_timeout_on_the_fallback_too_ends_the_turn_and_never_chains
theseus-core tests_fallback::timeout::a_whole_call_timeout_goes_to_the_fallback_and_finishes
theseus-core tests_fallback::timeout::no_fallback_named_the_one_same_model_retry_stands
```
plus `compiler::tests_kept::a_timeout_fallback_keeps_the_kept_render`.

Red at the base (non-test source reverted to `92e3dbed`, tests kept):
- test 1 `a_whole_call_timeout_goes_to_the_fallback_and_finishes`: `assertion left == right failed  left: "claude-sonnet-5-5"  right: "claude-sonnet-5"` (timeout.rs:31, res.model).
- test 2 `a_timeout_on_the_fallback_too_ends_the_turn_and_never_chains`: `left: ["claude-sonnet-5-5","claude-sonnet-5-5"]  right: ["claude-sonnet-5-5","claude-sonnet-5"]`.
- tests 4 and 6 pass at the base by nature (they pin existing behaviour); their proof is the plants below.
- test 7 does not compile at the base (the `Option` id type does not exist); proof is its plant.

Planted reverts (each restored, `touch`ed, `git status` checked):
- P1 (same-model retry restored, switch disabled): tests 1 and 2 red (`left: "claude-sonnet-5-5" right: "claude-sonnet-5"`).
- P2 (do not spend the whole-call retry after the switch, so a second attempt is allowed): test 2 red, `left: ["claude-sonnet-5-5","claude-sonnet-5","claude-sonnet-5"]`.
  Note: a second *switch* is already impossible by the `OnceLock` slot as well as `t.fallback`; P2 plants the reachable form (a third request).
- P4 (fall back to a model not in the catalog when none is named): test 4 `no_fallback_named_the_one_same_model_retry_stands` red.
- P5 (`is_whole_call` matches `Total | StreamIdle`): test 6 `a_stream_idle_cut_does_not_switch_models` red.
- P7 (absent id read as `Some("")` in `render_kept`): `a_timeout_fallback_keeps_the_kept_render` red, `left: (0, 0)  right: (19, 1)`.
Test 7 is at the compiler level on purpose: in the end-to-end rig the kept render's hit count is (1, 3) for a plain 4-loop turn with no cut at all
(control run), so it cannot tell the cases apart. The end-to-end `a_timeout_fallbacks_requests_keep_the_whole_transcript` checks the fallback's
requests still carry the first call and its result (nothing wrongly left out).
Negative assertion (test 2): never failed once the code was right; no retries used.

Suites (`TZ=America/Phoenix`, `CARGO_INCREMENTAL=0`):
- `theseus-core` full: 4651 run, 4649 pass; the 2 failures were `tests_route::a_switched_turns_compilation_names_its_recall_drops` and
  `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` (both on the known-flaky list); both pass alone.
- `theseusd --test bench_profile`: 7/7 pass (needs `cargo build -p theseus -p theseusd -p theseus-sim` first).
  `a_first_byte_timeout_is_retried_inside_the_headless_turn` passes with its asserted count unchanged (2 requests); this row did not move it.
- theseus-protocol 210/210; reader-rule `tests_registry` 15/15. Not run: theseus-kernel and the full theseusd suite (no code of theirs changed;
  disk on this VM was not the limit this time, I did not spend the time).
- `cargo fmt --check`, `scripts/shape.sh` and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Keel
`python3 scripts/keel-guard.py --base 92e3dbed --head HEAD --quiet` -> no output, exit 0. Keel findings expected: none.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in the door/keel phases, not on my range**: it judges against an older base
(`2d1d8d6f`) and reports eight erosions plus ~20 "refused the ack … its signature does not verify (%G? is E)" lines for commits that predate this
branch (persona_cmd, voice flux, rsi sealed, first_request, keel-guard). This clone cannot verify the owner's signatures, so the gate cannot pass here.
The later phases I ran by hand are above (fmt, shape, clippy, registry, protocol, core suite, bench_profile).

## Live check for the maintainer
On a scratch daemon (own `--state-dir`/`--socket`, not the operator's), with a model whose catalog entry names `refusal_fallback_model`
(Sonnet 5.5 -> Sonnet 5) and the provider cut at the whole-call bound (e.g. `[provider.timeouts] total_secs` very small with a stalled stream, or the
fake server in theseus-sim returning a stalled stream):
1. `theseus --socket <sock> ask "read harbor.txt"` -> the turn finishes; the reply's line should read like "Sonnet 5.5 declined (timeout); Sonnet 5 answered."
   (the `TurnFallback::line` text for category `timeout` is the refusal wording, see below).
2. `theseus --socket <sock> ledger --kind provider.fallback` (or the history) -> one row, `category: "timeout"`, `refused: null`.
3. With the fallback also stalling, the turn must fail after exactly two model requests (driver retries later as before).

## Open / for the owner
- `TurnFallback::line` (theseus-protocol route.rs) words every fallback as "declined (<category>)"; for category `timeout` that reads "declined (timeout)".
  Left as is (a protocol file, shared). A one-line wording branch there would say "timed out" instead. Docs to update: `crates/theseus-core/AGENTS.md`
  (the fallback paragraph: now also a whole-call timeout, category `timeout`, no refused id), and the spec's Part III item for this row.
- The switch is taken even with `transient = 0` (it is a different action from a transient retry); say if it should respect that.
- `turn.rs` grew 3 lines (rustfmt), under its ceiling.
