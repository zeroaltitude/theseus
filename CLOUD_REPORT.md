# CLOUD_REPORT: cn-route-v6 (theseus-a1wvq.2)

Branch `cloud/20261017-cn-route-v6`. Code commit `eda66f4c`.

## Step: route.v6, the `data` question, `[routing.data]` names
**Found.** Nothing beyond the brief. One thing the design did not say: `inbound()` in
`builders.rs` builds the state by hand, so `data_names` needed its own block
(`data_names_state`, a never-dropped scalar, at most 30 names of 60 characters each) as well as the field.
The list of names also had to reach `Inbound` (new field `data_names`, filled at the two construction sites
`turn/inbound_step.rs` and `rpc/thread_flow.rs` from `cfg.routing.data.names()`).

**Changed (eda66f4c).**
- `packs/route.v6.toml` (v5 word for word + `data`), `pack.rs` (EMBEDDED, shape row, rules row, count 23, T26, T28).
- `builders.rs` (`DataNames`, `InboundInput.data_names`, state block), names fixture `fixtures/inputs/inbound.names.json`, golden `fixtures/golden/route.v6.request.json`.
- core: `ROUTE_PACK = "route.v6"`; `config/routing_data.rs` (`RoutingData`, re-exported from `config/routing.rs`, field `RoutingConfig.data`; template block commented in `theseus.example.toml`); `routing/data.rs` (`DataAnswer`, re-exported as `routing::DataAnswer`); `Verdict.data`; `inbound.rs::verdict` reads it; `adopt.rs` `WHY_ROUTE_V6`; `lineage.rs` LEFT_ALONE gains route.v6 (the route pack is left alone for every version).
- tests that name the pack moved to v6 (`tests_inbound`, `tests_continue`, `tests_route_wait` (now 3 questions), `tests_route`, `tests_route_base`, `tests_route_effort`, `tests_judge`, `tests_ladder`, `tests_ladder_unread`, `tests_engage`, theseusd `route_owner`, `judge`, `bench_routed_live`).
- New tests: `route_v6_is_v5_word_for_word_and_a_data_question` (T26), `the_pack_text_names_no_organisation` (T28), golden (T27), `data_names_reach_the_state_only_when_given`, `routing_data_topics_parse`, `the_verdict_reads_the_data_answer` (+3 siblings), `the_operators_names_ride_the_state_only_when_configured`.
- Pack count: 22 -> 23. The Discord task-offer-chip row also adds classify.v3, so the join makes it 24.
- No store format change. No new dependency (Cargo.lock untouched).

**Proof.**
- `cargo nextest run -p theseus-judge`: 154/154. Existing goldens, `inbound.state.json` included, unchanged (only the new `route.v6.request.json` is added).
- Full `cargo nextest run --workspace` (`TZ=America/Phoenix`): 6245 tests, 34 fail, 3 time out. All 33 failures other than the one below are the known L1 ones (theseus-sandbox contract + `spawn_100`, theseusd `sandbox`). The remaining failure, `tests_engage::the_persona_question_adds_no_call`, was my miss (a `route.v5/` string); fixed, passes. The 3 TIMEOUTs are theseus-discord `tests_task_threads::*`; they pass alone (18/18 with tests_engage), so load.
- Planted reverts: (1) T26: changed "almost nothing" to "next to nothing" in v6's `trivial` option; `route_v6_is_v5_word_for_word_and_a_data_question` failed with `mode word for word` (pack.rs:1635); restored, touched, `git status` clean, passes. (2) T28: appended `# acme private plans` to the pack; `the_pack_text_names_no_organisation` failed (`left: ["acme-private"]`); restored. Not done: planted reverts for the other new tests (verdict read, names in state) beyond the above.
- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -D warnings` clean after splitting `data_names_state` out of `inbound` (it hit 107 lines); `scripts/shape.sh` ok; protocol.gen unchanged (no protocol type changed).

**Gate.** `scripts/gate.sh` failed in its **keel** phase (and "door") and never reached the later phases. Cause is not this change: the guard refuses `Keel:` acks on commits already in the range (`d424da19e4`, `238ecead06`, `1c2beb2747`, "signature does not verify (%G? is E)", the keys are not in this VM) and reports 3 erosions in files I did not touch (`lifecycle.rs`, `keel-guard.py`, `test_keel_guard.py`). I ran the later phases by hand as above (fmt, shape, clippy, suite, registry (inside the suite), protocol types). `deny`, `cockpit` and `features` phases were not run. Disk: the VM's allowance ran out twice (28 GB debug target); I rebuilt with `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0`.

**Live check for the maintainer** (needs Jev's key; I could not run it):
1. Scratch daemon of this build with `[routing.data] open_topics=["Widgetco"] private_topics=["Acme"]`; send "Summarise the Widgetco release notes" and "Draft Acme's payroll plan".
2. `theseus judge list` (or the judgment's blob): the state carries `data_names`; the route.v6 judgment has a `data` answer: `open` at act for the first, `private` for the second. `theseus judge`/health should show `route.v6: live (owner: the owner's decision of 2026-10-17)`.
3. With no `[routing.data]` names, the state blob has no `data_names` key.

**Left / uncertain.**
- Nothing acts on `Verdict.data` (as briefed). `DataAnswer` has `choice`, `confidence`, `band` only.
- T28's deny list holds only the digest of the made-up "acme-private"; the maintainer adds the real digests (lowercased word, or two words joined by `-`, SHA-256 of the string).
- route.v6 is in `lineage::LEFT_ALONE` (array size 8 -> 9), which the brief did not list.
- Docs to change at review: spec Part III item and version line, `docs/status.md`, `crates/theseus-core/AGENTS.md` ("Routing" bullet still says route.v5 is the acting pack).
