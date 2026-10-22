# CLOUD_REPORT: cn-observability (theseus-a1wvq.11)

Branch `cloud/20261021-cn-observability`, from `integ/cn` with `.8` merged (`7addcb0`). Commits: `39553d6` (the row), `fb29f10` (clippy's argument limit: `est_tokens` rides in `Timed`; rustfmt of the new tests).
Store format: **unchanged (48)**. No new dependency. `crates/theseus-protocol/src/lib.rs` untouched.

## 1. The contract as built (for the rows after this one)

`.8`'s names were used as they stand: `RouteState.admitted` (`Admitted(Result<OpenRun, DataWhy>)`), `data::Carried`, `data::clearance`, `TurnRunner::data_pass`. Nothing was renamed by the join. `data::Carried` is not recorded on the turn by `.8` (only each request's `Clearance`), so this row adds one field, `Held.run_nodes` (`turn/open_step.rs`): the last request's carried-node count, set by `data_pass` from the same `Carried` it already builds.

### `route.decided` keys (`fact/route.rs`: `Ran`, `RouteData`; written in `row()` as one group)
- Always (every row, checkpoint rows too): `provider`, `model` (null if the profile does not resolve), `origin` (`us|eu|cn|unknown`) of the row's `profile`.
- Route rows only (`data: Some(RouteData)`): `clearance` = `admitted` when `RouteState.admitted` is `Ok`, else `closed`. One exception, on purpose: when the floor turned an admitted turn into `private_view` (`data_floor` overwrites `admitted`), the row still says `admitted` (Jev's admission), with `data_gate: private_view`.
- `data`, `data_confidence`: `Verdict.data` choice/confidence, only when the verdict carries one.
- `data_gate` (open string, only when written): (1) `RouteState.gated`, set where the floor moves the decision: `data_floor` (the admission's `DataWhy` word if clearance moved it, else `sensitive`) and `data_move` (`tool_result`); else (2) the admission's `DataWhy` word when the verdict's mode has non-empty `cn_profiles` and the turn was not admitted.
- Checkpoint rows: `provider/model/origin` of the profile it moves to; no data keys.
- `narrate`: `Kept off a restricted-origin model: <why>.` only when `data_gate` is set.
- `record_route` is now a `&self` method; `Timed` also carries `est_tokens`.

### `TurnData` / `footer()` (`theseus-protocol/src/route.rs`)
`TurnRoute.data: Option<TurnData { verdict, clearance, gate, origin }>`. `clearance` is `admitted|closed` from `record_route`, and `open` once any request of the turn carried a pass (`TurnRunner::note_open` in `turn/data_step.rs`, called right after `data_pass`; it creates a `TurnRoute` with reason `no_verdict` for a turn that asked no route). A later `record_route` (a loop moved off the restricted model) keeps `open`. A checkpoint keeps `data` as the turn had it. `footer()`: `open` when clearance is `open`; `data gate: <gate>` when `gate` is set; else `None`. Discord `routed()` and the CLI `status_line` append ` · <footer>`; with `data: None` or `footer() == None` both are byte-for-byte unchanged (tests).

### `provider.call` (`fact/turn.rs`)
`origin` always; for a restricted origin, `pass` (from `compiled.request.data_pass.is_some()`, the request sent) and, when `pass`, `run_nodes` (`Held.run_nodes`). A US or EU call writes `origin` only.

### Metric
`theseus.route.data_gate` (IntSum; attributes `theseus.data_gate.what`, `theseus.data_gate.why`; named `theseus.route.*` like its neighbours, not the design's `theseus.routing.data_gate`). `Telemetry::record_data_gate(what, why)`, and `JudgeService::record_data_gate` (the turn's route to the handle). Called by `record_route` (`what = "route"`). **The chokepoint (`data_gate.rs`) is not wired**: `Gated` holds no telemetry handle, so reaching it needed new plumbing; the background row (.10) calls `record_data_gate` with its own `what`. `INSTRUMENTS` array length 49 to 50.

### Bench report (`theseus-protocol/src/own_bench.rs`, `own_bench/report.rs`)
`RouteCell.origin` (`us|eu|cn|unknown`, `unrecorded`, `mixed`, empty for a cell with no turn); `RouteReportResult.data: Vec<DataCell {origin, data, turns, spend_usd}>` (data = `open|private|unclear|none`) and `gates: Vec<GateCount {why, turns}>`, both sorted, both `#[serde(default)]`. `Core::route_data_since` (one extra `route.decided` scan in the report only) takes each turn's origin from its last row that names one (the model the turn ended on, which is the cell's model), the first data answer and first gate word. A row from before the keys is `unrecorded`, never `unknown`. CLI `report_lines`: ` [origin]` on each cell line, a line per data cell, one `kept off a restricted-origin model: why n, ...` line; nothing extra when every cell is `unrecorded`.

## 2. What changed, by file
- `theseus-core`: `fact/route.rs`, `fact/turn.rs`, `turn/route_step.rs` (record_route, `RouteState.gated`, `Timed.est_tokens`), `turn/data_step.rs` (`ran_of`, `data_gate_why`, `note_open`, gated word in `data_floor`), `turn/open_step.rs` (`run_nodes`, `tool_result` gate word), `turn/checkpoint_step.rs`, `turn.rs` (provider.call origin/pass; `note_open` call, 9 lines), `judge/inbound.rs`, `telemetry.rs`, `telemetry/metrics.rs`, `own_bench/report.rs`, `data_gate.rs` (`word` is `pub(crate)`), `tests_observability.rs` (new), `tests_open_request.rs` (rig helpers made `pub(crate)`, plus `rig_telemetry`), `lib.rs` (one `mod`), `AGENTS.md` (the two sentences), `tests/golden/core_output.txt` (24 `provider.call` lines gain `"origin":"us"`, nothing else; diff read).
- `theseus-protocol`: `route.rs` (`TurnData`, `footer`, test), `own_bench.rs`, `ts.rs` (types added to the existing line).
- `theseus-discord/src/runtime/route.rs`; `theseus/src/render.rs` (+3 lines: now 3,120 of 3,128), `render/own_bench.rs`, `render/route_data.rs` (new, test-only).
- Cockpit types regenerated by the protocol test and added: `RouteCell.ts`, `RouteReportResult.ts`, `TurnRoute.ts`, `index.ts`, new `DataCell.ts`, `GateCount.ts`, `TurnData.ts`.

## 3. Tests and plants (each plant restored, `touch`ed, `git status` clean)
| test | plant | failure seen |
|---|---|---|
| `route_decided_records_data_clearance_gate_provider_model_origin` | drop `row["origin"]` | fails (a and b) |
| same | `data_gate` only when `reason == DataGate` | fails on (b) (`verdict_private` missing) |
| `a_plain_turns_route_row_gains_only_where_it_ran` | gate whenever not admitted | fails |
| `provider_call_records_origin_and_the_pass_per_request` | `pass` from admission | fails |
| `the_turns_footer_says_open_only_when_a_request_carried_a_pass` | `open` from `admitted` in `record_route` | fails |
| `turn_route_data_round_trips`, Discord `the_footer_says_when_a_request_went_open_...`, CLI `the_status_line_ends_its_route_words_with_the_data_rules` | `footer()` None for `open` | all three fail |
| `the_report_counts_turns_per_origin_and_data_and_gate_skips` | `unrecorded` read as `unknown` | fails |
Passing unplanted: `the_floor_move_names_its_why`, `a_data_gate_turn_counts_once_on_the_route_data_gate_metric`, the CLI `the_report_says_origin_data_and_gate_skips`.
Deviations from the brief: test 2 asserts `data_confidence` present (the fake Jev always answers `data`) and no `data_gate`; test 3 covers `private_view` and `verdict_private` end to end but **not the `sensitive` word** (a sensitive-name turn with no usable US profile ends before any row; with one, routing picks it without `Reason::DataGate`), so `sensitive` is untested end to end; no run under load (the new tests are not timing-sensitive, and a loaded rerun was not done).

## 4. Runs
- `cargo nextest run -p theseus-core -p theseus-protocol -p theseus-discord -p theseus` (`TZ=America/Phoenix`) on the final tree: **5,159 run, 5,159 passed**, 26 skipped. That includes `tests_registry`, `tests_first_request`, `tests_data_floor`, `tests_open_*`, `tests_route`, own-bench, Discord route, render, and `a_plain_turn_stays_within_its_frame_budget`.
- `cargo fmt --check`, `scripts/shape.sh` ok, `cargo clippy --workspace --all-targets -D warnings` clean.
- **The lane gate did not complete**: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in its `keel` phase, because it compares against `1d092ded` (main's older base) and flags earlier `integ/cn` rows' changes (`rsi/sealed/relay.rs`, moved sim tests), none of mine. Keel guard against the right base:
  `git fetch origin integ/cn && python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` printed nothing (exit 0).
  I then ran the later phases by hand: fmt, shape, clippy (workspace), the four-crate suite. **Not run here**: `clippy-probe`, `cockpit` build, the full-workspace suite (theseusd, sandbox), `deny`, the benches. The maintainer's gate on the owner's machine covers them. `cargo deny fetch` was not run (the install and build chain stalled on a full disk; a plain `cargo build` was used).
- Disk: this VM's writable allowance filled twice; I deleted `target/debug/incremental` and some theseusd/sim/exam/sandbox test binaries (rebuildable) under `target/`. Nothing outside the repo.

## 5. The maintainer's live check (after the join, scratch daemon, fresh state dir)
Config: `trivial.cn_profiles = ["glm"]`, no Haiku in `trivial.profiles`, the owner's DM bound, route pack live.
1. An open DM message (Jev: `data = open` at the act band, mode `trivial`): the Discord footer ends `· open`; `theseus ledger tail` rows: `route.decided` has `origin: "cn"`, `clearance: "admitted"`, `data: "open"`, no `data_gate`; `provider.call` has `origin: "cn"`, `pass: true`, `run_nodes: N`.
2. A private message: footer ends `· data gate: verdict_private`; `route.decided` has `origin: "us"`, `clearance: "closed"`, `data_gate: "verdict_private"`.
3. `theseus model bench`: each cell line carries `[cn]`/`[us]`/`[unrecorded]`, then `cn · data open: N turns, $x`, and `kept off a restricted-origin model: verdict_private n`.
4. `theseus.route.data_gate` appears in the metrics export after (2), `what=route`, `why=verdict_private`.
5. Rollback check: with no restricted profile in any `cn_profiles`, a plain Claude turn's footer is unchanged and its row gains no `data_gate`, `pass` or `run_nodes`.

## 6. Left over and for the owner
- Chokepoint counter (`data_gate.blocked` caller) not wired: needs a telemetry handle in `Gated`.
- `sensitive` gate word untested end to end (above).
- `clearance` says `admitted` for a floor-moved `private_view` turn (the brief's test 7 asks for it); a reader wanting the request-level view should read `data_gate` or the `provider.call` row.
- Docs to change at review: spec Part III item, `docs/status.md`; `cn-draft-review` adds its `draft_skip` key beside the `RouteData` group in `fact/route.rs`'s `row()`.
- `render.rs` is 3,120 of 3,128 lines; `turn.rs` grew by about 10 lines.
