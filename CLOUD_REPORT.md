# CLOUD_REPORT: cn-tool-screen (theseus-a1wvq.9)

Branch `cloud/20261021-cn-tool-screen`. Code commit `57805e8`. Base: `integ/cn` with `.8` merged (checked: `.8`'s commits,
`data::admits`, `Carried`, `clearance`, `RouteState.admitted` are all present).

## 1. The contract names as built

- `data::screen(tool: &str, plan: Option<&Plan>, redactions: u64, open: &Open) -> Result<(), Unscreened>` (`data.rs`).
- `data::Unscreened { Program, Tool, Secret, Outside }`, `as_str` = `program`, `tool`, `secret`, `outside`.
- `RouteState.screened: Vec<String>` (ids of result nodes this turn screened and passed), one line after `admitted`.
- `TurnRunner::screen_loop(&self, t, session, uses)` in `turn/screen_step.rs`. It returns `()`. The word goes to a
  `tracing::info!` line (turn, node, word), because `.8` moves the loop and there is no stop line to put it in.
- `open_step::writer(n, turn, screened)` now takes the screened ids: a screened result is `Writer::Loop`.
  `carried`, `data_move` and `data_end` all read it through that one function.
- `data::clearance` and `Carried` are unchanged. A screened result is carried as `Writer::Loop`, so `clearance`
  admits it by its existing rule. A result inside the stored run is before `run.through`, so it was already in the run.
- `.8`'s names are as the brief says; none was renamed.

## 2. `.8`'s later-loop handling, and the branch built

`.8` MOVES the later loop (`data_move`, `Reason::DataGate`, through the fourth `OnceLock` slot). It does not end the turn.
So I built item 2's MOVE branch: no new stop line. `route.decided` with reason `data_gate` is the record of the move.

- I did not record a new `LoopCut { decision: "data_gate" }`. `LoopCut` is the exit of the loop's span, and the screen runs
  inside the loop, so a second exit would double-close it. The move's `route.decided` row says the same thing. The
  maintainer may want to say so in the spec.
- `data_end`'s taint path does not fire for a screened result, since it counts as the loop's own. A failed result's taint is
  set by `screen_loop` with the result's node id first, and `data_end`'s `taint.is_none()` guard keeps it from a second one.
- **Sessions already tainted `tool_result` before this row lands stay closed**: the taint is stored.

## 3. What changed (lines against ceilings)

- `data.rs` (575, no ceiling): `Unscreened`, `screen`, a module paragraph, and `Writer::ToolResult`'s and `OpenRun`'s docs.
- `turn/screen_step.rs` (new, 81): `screen_loop`.
- `turn/open_step.rs` (408, no ceiling): `writer` takes the screened ids and is `pub(super)`; `Held.taint` is `pub(super)`.
- `turn/route_step.rs`: the `screened` field. `turn.rs` (3,422 / 3,525): a `mod` line and one call line after `run_tools`.
- `lib.rs`: one `mod tests_tool_screen`. `AGENTS.md` (core): the screen's paragraph in the open request's bullet.
- `tests_open_request.rs`: only `pub(crate)` added to the rig helpers the new tests reuse (`config`, `rig_with`, `jev`,
  `dm`, `say`, `tainted`, `open_task`, ...). No test changed or weakened.
- `tests_tool_screen.rs` (new, 398): the five tests below. `screen_matrix` is here, not in a `data.rs` test module,
  to keep `data.rs` from gaining a tests module; the brief names `tests_tool_screen.rs` for all tests.
- Item 4 (store step): checked, no change needed. `data_end` stores `run.through = <last node>` when every request was cleared and
  nothing past the run was written by another writer; a screened result counts as the loop's own, and
  `a_screened_result_extends_the_run` shows the stored `through` is the turn's last node.
- No store format change, no dependency, no config key. `reqwest::Url` (already a core dependency) is used to reject an
  unparsable fetch URL as `outside` (fail closed; `private_url` alone returns none for one).

## 4. Tests and plants

All five in `tests_tool_screen.rs`. Each plant: edit, run `cargo nextest run -p theseus-core -E 'test(tests_tool_screen)'`,
restore from a copy, `touch`, `git status`.

1. `a_private_tool_result_stops_a_cleared_turn` (key; runs the secret `fs.read` and a `proc.run`, both as an open task on
   dsflash). Plant: `data::screen` returns `Ok(())` first. FAILED (`tests_tool_screen.rs:148`, loop 1 went to dsflash with
   the result), as did `screen_matrix` and the DM test.
2. `the_screen_runs_only_for_an_open_request`. Plant: drop the `restricted_profile` condition in `screen_loop`. FAILED
   (`:202`, a taint row naming a node on the Claude open task).
3. `a_screened_result_extends_the_run`. Plant: skip the push into `RouteState.screened`. FAILED (`:216`).
4. `a_failed_screen_in_the_dm_ends_the_run_not_the_session`. It reads a file outside the roots, which the gate refuses;
   the refusal's result node is screened `outside`. Two guards stop a private-view taint (`screen_loop`'s `open_of` check and
   `data_end`'s `filter(|_| open)`); planting either alone passes, so the plant removed both. FAILED (`:246`, a taint row).
5. `screen_matrix`: every word and pass in the brief's list. Covered by plant 1.

Run unchanged and passing: `tests_open_request`, `tests_data_floor`, `tests_open_view`, `tests_open_task`,
`tests_first_request`, `tests_route*`, `a_plain_turn_stays_within_its_frame_budget`, `tests_registry`: 138 of 138 in one
nextest run. `a_later_loop_after_a_tool_result_leaves_the_restricted_model` is unchanged (its `tools_find` result fails as
`tool`). The full `theseus-core` suite: `TZ=America/Phoenix cargo nextest run -p theseus-core --no-fail-fast` 3,680 passed,
25 skipped. I did not run a test under the nice-19-plus-four-loops load; none of the tests is about a race.

**The brief's test 2 differs from what the base does.** An open task on a Claude profile that calls a tool is tainted by
`.8`'s `data_end` (the Claude request's clearance is `tool_result`, which taints in an open session), with `node: None`.
That is `.8`'s behaviour, not this row's. The test asserts the screen's part: no taint row names a node, no move, and
loop 1 is sent as before. The maintainer may want to look at whether an open session on a Claude profile should taint.

**A taint still closes admission** (`.8` F1): in an open session a failed screen records the taint, so that session's later
messages are not admitted. "The next open message starts a fresh run" holds for a DM open detour (no taint), which is test 4.

## 5. What could not run on the VM

- The gate's `keel` phase fails on this VM against `origin/main`'s base, on earlier merges' commits and not mine (moved
  lifecycle tests, and an ack whose signature cannot verify here). I ran the later phases myself: `cargo fmt --all -- --check`
  ok, `scripts/shape.sh` ok, `cargo clippy --workspace --all-targets -q -- -D warnings` ok, and the core suite above. I did not run
  the whole-workspace suite, the cockpit phase, `clippy-probe`, `deny`, or the benches.
- Maintainer: run `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the owner's machine, and
  `cargo nextest run --workspace` for the other crates (L1 tests fail on a root VM).
- The disk allowance ran out twice here; I deleted `target/debug/incremental` and stale test binaries (inside the repo).

## 6. Keel guard

`python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` (after `git fetch origin integ/cn`): no output, exit 0.

## 7. Store format: unchanged (48)

## 8. Live check after the join (scratch daemon, `trivial.cn_profiles = ["glm"]`, an open task on glm)

- Make a clean file under `[routing.data] open_paths`; start an open task on glm that reads it. Read the `route.decided` rows
  of the task's session: none says `data_gate`; the session's `session.data` rows have no `tainted`; its second request went to glm.
- Make a file holding a fake webhook-shaped secret (`whsec_` plus 32 mixed letters and digits) in the same tree; start an open
  task that reads it. Read `route.decided` (a `data_gate` row after loop 0), `session.data` (one `tainted` row, why `tool_result`,
  `node` the result's id), the result node's `meta.redactions`, and `data_gate.blocked` (none: the later loop moved
  before it could reach the chokepoint). The next request has no pass.

## 9. Follow-ups

- A sandboxed program run, so a program's output can pass (theseus-a1wvq.16). Until then the screen refuses `Program`.
- Results answered after their loop (a confirm, a background job) are not screened; `.8`'s chain ends the run at them.
- Doc: the spec's account of `LoopCut` and the screen's word (`tracing` only) should say what is above.
- Decide whether an open session on a Claude profile should taint on a tool result (see section 4).
