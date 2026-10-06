# CLOUD_REPORT: gate-tests (branch `cloud/20261005-gate-tests`)

All six items were still open on main (4a44946), so none was dropped. Each commit's subject carries its issue id.

## Step 1: theseus-x1jj, an edit's server start (L3) through the gate (e8ad151)

- **Found.** `lsp::edits::gate`'s own test calls the function; no turn exercised its line in `toolrun/order.rs`.
- **Changed.** New `tests_gate_layers.rs` (its `mod` line in lib.rs). `tests_lsp_edits.rs`'s rig helpers (`rig`, `session`, `turn`, `edit`, `result_of`, `Rig`) became `pub(crate)` so it can reuse them (that visibility change is in 8316c22).
- **Test:** `an_edit_that_starts_its_server_waits_in_a_turn_as_proc_run_would`. With `start_on_edit` on, no server up, `proc.run` on approve and `fs.edit` open:
  - A private place's `fs.edit` leaves one `tool.confirm_requested` card, its reason containing `starts fake on`, and no server is spawned.
  - In a shared place (`public_paths` set) there is no card, and the edit runs.
- **Plant.** The L3 line removed from order.rs fails it: `the edit waits: []`. It also fails t2xr's test (`fs.edit — open`). Restored, touched, `git status` clean.

## Step 2: theseus-xx6w, count the turn's frames (8316c22)

- **Found.** The `lsp.ready` row lands on the board's own task (`lsp::readiness`) after `lsp.diagnostics` returns. The test waited for a store still for 100 ms, so a late-scheduled task wrote the row inside the counted turn.
- **Could not reproduce naturally.** Under the load recipe (4 busy loops, test at nice 19) the old test passed 30 of 30 runs. With 8 loops it passed 40 of 40. The failure needs the row held back past the 100 ms still-wait.
- **Reproduced deterministically.** I planted a 130 ms sleep before `b.record(&LspReady…)` in `readiness()` (lsp/mod.rs). The old test then failed 3 of 3 runs with exactly the reported `with the block 10 frames, without 9`. The record was the `lsp.ready` ledger row.
- **Changed.** The test now waits (`until_ledgered`, real clock, 60 s bound) until the ledger holds an `lsp.ready` row, then counts the turn's frames. A failure prints the turn's ledger row kinds (`ledger_kinds`).
- **Proof.**
  - New test with the 130 ms plant still in: 3 of 3 pass.
  - New test with the plant removed: 30 of 30 runs under the recipe pass.
- **Plant, block in its own frame.** In `lsp_onto` (lsp/edits.rs), where `text.push_str(&a.text)` attaches the block, I added `tc.store.put_meta("plant.block", &1u32)`. The test failed: `with the block 10 frames, without 9; the turn's rows: ["execution.queued", … "execution.waiting"]`. Restored, touched.

## Step 3: theseus-sh9w, an extension's load floor through the gate (934a3ad)

- **Changed.** `extend/tests_load/floor.rs` is a child module of tests_load, so it reuses its private rig (`mod floor;` at the end of tests_load.rs).
- **Test:** `an_extension_loaded_under_a_floor_waits_in_another_place_until_loaded_without_one`.
  - `#pier` is bound with no ceiling when `extend.propose` runs, so the propose call itself is not floored. It is re-bound with `posture_floor = approve` before the ack, so the load records that ceiling.
  - `[policy.mcp] ext-wordcount = open`.
  - A turn in `#den` (private, no ceiling) calls the tool. It waits, reason `… approve (#pier's ceiling sets a floor of approve)`, and nothing ran.
  - The card is declined and the extension revoked, then proposed and loaded again from a no-ceiling place with different content. The same call then runs `Ok`, with no second card.
- **Plant.** The `Floors` line removed from order.rs fails it: `the call waits for approval: []`.

## Step 4: theseus-grms, a batch's floor step outranks an earlier listed step (f65b4e9)

- **Test, in tests_steps.rs:** `a_floor_step_outranks_an_earlier_step_on_the_approve_list`. Step 1 `touch first` is on `approve_argv`, step 2 is `op whoami`, both approve. The batch has posture approve, `floor` true, reason `step 2 of 2 (\`op whoami\`): `, and `first` was not created.
- **Plant.** `stricter` comparing `a.posture > b.posture` alone fails it at the floor assertion (`floor` false). Restored, touched.

## Step 5: theseus-cxqj, a ceiling's `mcp:<server>` (90f830b)

- **Changed.** `mcp/tests/ceilings.rs`, a child of mcp/tests.rs (`mod ceilings;`).
- **Test:** `a_ceilings_mcp_entry_decides_which_places_are_offered_the_servers_tools`.
  - `#lab` has `tools = ["mcp:fake"]` and `#den` has `tools = ["web"]`; both are private. This test binds them with `Core::bind_places`, as tests_ceilings does, not with a bindings file.
  - `#lab` is offered exactly the fake's 5 tools, its tools note has the MCP line, and its `echo` runs.
  - `#den` is offered `["web_search"]`, has no MCP line, and its `echo` is refused with the brief's exact words. Only one call reached the server.
- **Plants.**
  - `definitions_for`'s MCP filter ignoring the ceiling fails it: `#den is offered web only`, with the 5 MCP tools listed.
  - `mcp_note` ignoring it fails it at the note assertion.
  - Both restored and touched.

## Step 6: theseus-t2xr, explain's L3 row (8906ae3)

- **Found.** Beyond what the brief says, `policy.explain`'s probe call for any edit is the first workspace root, a directory with no extension, so `board.server_for` can never match and L3 never fires there. A row or condition alone would have been dead code.
- **Changed.** In `rpc/explain.rs`:
  - `starts_on_edit` lists the servers an edit by this tool can start in a private place. It requires an `lsp` board, `[lsp] edit_diagnostics` on, and `start_on_edit` servers. It excludes `lsp.rename`, whose family is `lsp`.
  - `edit_probe` points an edit's probe file at `<root>/explain-probe.<ext>`, with the extension taken from the first such server.
  - A row `lsp` appears where L3 raised the posture.
  - A condition `lsp_start` names those servers and `proc.run`'s posture.
  - The order itself is unchanged, and nothing copies it.
- **Design note for the owner.** Presets that start on edit by default (rust-analyzer, ty, tsgo) appear in the condition's entries whenever `[lsp]` is enabled, installed or not. This follows the brief ("a server with start_on_edit is configured"). The gate itself skips servers whose program is not installed, so the entry list can over-claim. Say so if you want it filtered through `Board::server_for`.
- **Test:** `explain_names_an_edits_server_start_and_what_it_raised`, in tests_gate_layers.rs. The presets are switched off in the test so the entries are exactly `["fake"]`. With `start_on_edit`: result approve, the `lsp` row is raised/approve, the condition exists. Without it: result open, neither.
- **Plants.** The new row's arm disabled fails it (`on.layers` has no `lsp` row). Removing the L3 line from order.rs also fails it.

## Proof, offline

- **New tests under load.** Each of the five other new tests (steps 1, 3 to 6) passed 5 of 5 under the recipe. Step 2's changed test passed 30 of 30 (above).
- **Families.** `tests_lsp_edits`, `extend::`, `mcp::`, `tests_steps`, `tests_explain`, `tests_ceilings` and `gate_layers`: 69 passed.
- **theseus-core, whole.** `cargo nextest run -p theseus-core`, with `TZ=America/Phoenix`: 1284 passed, 4 skipped, 0 failed.
- **Doc changes.** The maintainer might note in docs/spec (Part III) that `policy.explain` names L3. I changed no doc.

## Live check (maintainer, on a scratch daemon)

Use a fresh state dir, Discord and the web off, and `theseus-sim fake-model` as the model, with a bindings file of two private places.

1. **L3.** Configure a language server with `start_on_edit = true` and `[lsp] edit_diagnostics` on, with `proc.run = "approve"` and none running.
   - An edit in a private place should ask, with the card's reason naming `starts <server> on <root>`.
   - `theseus policy explain --tool fs.edit` should show an `lsp` row (raised, approve) and an `lsp_start` condition whose entries list the servers.
2. **MCP ceilings.** Make one place's ceiling `tools = ["mcp:fake"]` (`theseus-sim fake-mcp` as the server) and the other's `tools = ["web"]`.
   - The first should be offered the fake's tools.
   - The second should be offered none of them, and a call to one should be refused with `…its ceiling in the bindings file offers only web`.
3. **Batch floor.** A `proc.run` batch whose first step is on `approve_argv` and whose second is `op whoami` should show a card saying floor, with the reason starting `step 2 of 2 (\`op whoami\`): `.

## Gate

I ran `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the union tree, which is the same content as HEAD. Per-commit, I ran the new tests with fmt on the intermediate trees. Clippy and the whole suite ran only on the union; two commits (e8ad151, 8906ae3) were split from it by moving test text, not code.

- **fmt, shape, features, clippy, cockpit, test build, reader rule:** ok. A first clippy run caught a redundant `.to_string()` in my step-2 helper; fixed before the green run.
- **suite:** 2835 run, 2802 passed, 33 failed. All 33 are the known L1 cases (as root, with no job cgroup, theseus-pv6i):
  - `theseus-sandbox::contract`
  - `theseus-sandbox::bench`
  - `theseusd::sandbox`
- **Disk.** A first gate run hit `No space left on device`, which also failed `theseusd::bench_profile::theseusd_check_passes_on_the_bench_profile_with_no_vault`. I deleted `target/debug/incremental` (in-repo build cache) and reran with `CARGO_INCREMENTAL=0`; that test then passed in the suite.
- **Phases after the suite, run by hand.**
  - protocol types: ok (`cockpit/src/protocol.gen` clean).
  - turn bench `--check --runs 5 --burst 0`: frames 5 and 9 at the p95, within budget.
  - `cargo deny --offline check`: ok (advisories, bans, licenses, sources).
  - Lifecycle and jobs benches were skipped, as `THESEUS_GATE_NO_BENCH=1` asks.
