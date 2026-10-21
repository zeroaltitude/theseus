# CLOUD_REPORT: cn-floor-session (theseus-a1wvq.3, theseus-a1wvq.5)

Branch `cloud/20261021-cn-floor-session`, from `main` at `1d092ded`. Started 07:54 UTC, report written 12:05 UTC.
Both parts are done and pushed. No connector or MCP tool was used; nothing went to a GLM or DeepSeek endpoint.

## 1. Files over a line ceiling

None. `scripts/shape.sh` passes. No ceiling was raised. `compiler.rs`'s `render_kept` was at clippy's 100-line
limit; to add `data_pass: None` I took out its six-line tuple destructure and read `rendered`'s fields where they're
used (a pure move, the same request bytes).

## 2. What changed

### Part A, theseus-a1wvq.3: the floor. Commit `d1678d7`

- **`crate::data`** (`data.rs`, new): `DataPass` (private digest field, `covers(&str)`, no `Clone` or `Default`;
  tests use `#[cfg(test)] DataPass::for_test(digest)`), `DataWhy` (all 13 words, snake_case, `as_str`), and
  `Clearance` (opaque: `closed(DataWhy)`, `is_cleared`, `why`, `pass(digest)`). Nothing outside `data.rs` can build a
  cleared one. `Clearance::default()` is `closed(Unchecked)`, which fails closed.
- **`ProviderRequest.data_pass: Option<DataPass>`**, `#[serde(skip)]`. `Clone` and `PartialEq` are now written by hand:
  a copy carries no pass (it is another request), and equality ignores the pass. `Clone` destructures every field, so
  another row adding a field gets a compile error and must say how the field copies. Each literal gains
  `data_pass: None,` on a line of its own.
- **The chokepoint** (`data_gate.rs`):
  - `Gated::refuse` refuses a model whose `origin_of(..)` is restricted unless the request's pass covers its own
    digest. The digest is taken only for a restricted model.
  - Each model's origin is memoised in `Gated` (`RwLock<HashMap>`).
  - The names check and the Google mark are unchanged, and a pass never lifts them. With no names configured, the
    names walk is skipped.
  - The refusal's words are "<model> is of restricted origin (<origin>) and this request has no pass for its data;
    nothing was sent", class `data_gate`.
  - Each refusal writes one `data_gate.blocked` row: `LedgerKind::DataGateBlocked` and `fact/data.rs`'s
    `DataGateBlocked {provider, model, origin}`. The row carries the session from a new task-local,
    `data_gate::for_session`, which the turn's call and compaction's summary call set.
  - `guard(providers, cfg, catalog, store)` wraps **every** provider, always; the early return is gone.
- **Routing** (`turn/route_step.rs`, `turn/data_step.rs`):
  - `route_profiles(Floor)` replaces `route_profiles(bool)`. `Floor` is `{sensitive, cleared}`, from
    `RouteState::floor()`.
  - It marks a profile `unusable = Some("data_gate")` when `floor_refuses` says so: restricted and not cleared, or
    named and not US.
  - Every caller passes the floor: route_step, brief_step, checkpoint_step, correct.rs ×2. The correction resolve
    outside a turn passes a closed floor.
  - Nothing in `routing.rs` changed.
- **`data_floor`** acts on the restricted case as it did on the names. The decision's profile (else `t.target.profile`)
  is checked. The `[model]` default runs with `Reason::DataGate` when `route_profiles(floor)` finds it usable, which
  includes the floor's own check. Otherwise the turn fails before any model call with "No model cleared for this
  request's data is usable: <why>".
  - **New path:** `floor_unrouted` runs at the first loop of a turn that asked no route: routing off, the judge off, a
    wake, a continuation, a task report. It moves the turn with `route_to` and records its `route.decided` row
    (`compile_routed`).
  - **New path:** an owner's steer to a profile the floor refuses (`decide_steer`, in correct.rs) returns that profile
    instead of letting `steer_to` fall back silently, so `data_floor` moves the turn and the row says `data_gate`.
- **The turn's clearance**: `RouteState.clearance: Clearance`, closed(Unchecked) at every turn's start (the default).
- **The config** (`config/data_floor.rs`, one call in `validate`): refuses a restricted profile for the seven keys of
  item 6, and a `[judge] api_base` whose origin is `cn`. The words are "<key> may not name a profile of restricted
  origin: its input is the owner's private material (`<profile>`)". A profile that is not configured is left to its
  own check.
- `crates/theseus-core/AGENTS.md`: the sensitive-names passage now says `guard` wraps always, and a new passage covers
  the restricted-origin floor.
- **Rollback:** revert `d1678d7`. The floor ships on, with no switch.

### Part B, theseus-a1wvq.5: the session's data state. Commit `54fb4bf`

- **The types**, in `data.rs`:
  - `DataState { opened, run, taint }`. A copy read from the store also remembers, unstored, the run it read
    (`#[serde(from = "Stored")]`); `PartialEq` ignores that remembered run.
  - `Opened { how: OpenHow, at_ms }`, with `OpenHow` `task` | `draft`.
  - `OpenRun { from, through }`. Its doc is the chain of blessed writers, word for word from the brief.
  - `Taint { why, node, at_ms }`, documented as bookkeeping only.
  - `DataState::new`, `merge`, `line`.
- **`SessionRecord.data: Option<Box<DataState>>`**, `serde(default, skip_serializing_if)`. Absent reads as the private
  view with no open run, with no migration.
- **The write-back**: `take_turns_fields` calls `DataState::merge` and never copies. A stored taint always wins, and
  `opened` is never cleared. The turn's run is taken only when the stored run equals the run the turn read at its
  start, or already equals the turn's: compare and set.
- **Tasks and forks**:
  - task.rs sets `rec.data = None` beside the `external::hold` call. Both task-opening paths (`task.create`,
    `task.start`) already build a fresh record.
  - `fork::data_through` copies `opened` and `taint`, and cuts the run at the fork point; rpc/fork.rs calls it.
- **Ledger**:
  - `LedgerKind::SessionData` (`session.data`).
  - The facts `SessionDataOpened {how}` and `SessionDataTainted {why, node}`, with rows `{"row":"opened","how"}` and
    `{"row":"tainted","why","node"}`.
  - The writers `data::opened_row` and `data::tainted_row`, `pub`; their callers come with later rows.
- **Reader**: `turn.inspect`'s result has `data: Option<String>`, the state's line, for a private place only. It is in
  `theseus-protocol/src/inspect.rs`, not lib.rs, and its TypeScript is regenerated. There is no health count (no room
  without a lib.rs line).
- **Store format**: 47 → 48, with the constant's doc line. The format's own tests' literals moved to 48:
  - `tests_layouts.rs`'s "moved to this build's format" asserts;
  - theseus-core `store.rs`'s `a_write_moves_an_older_store_to_this_builds_format`;
  - theseusd `tests/versions.rs` (48, and its "newer" store 49).
- The core AGENTS.md has a passage on the data state.

## 3. Tests and plants

New tests (all pass):

| Test | What it shows |
|---|---|
| `tests_data_floor::the_provider_refuses_a_restricted_model_without_a_pass` (T24, **key**) | No names and no `[google]`. A plain request to `glm-5.3` through `providers["zai"]` gets class `data_gate` and words naming the model. The `zai` fake records 0 requests, and one `data_gate.blocked` row (zai, glm-5.3, cn) is written. Claude is sent the same request, with no row. |
| `tests_data_floor::a_pass_covers_only_the_request_it_was_minted_for` | A's pass sends A. B (one changed byte) with A's pass is refused. The fake records A alone. A clone has no pass. |
| `tests_data_floor::data_pass_never_changes_the_request_digest` (T25) | The digest, the serialized body and `==` are the same with and without a pass. |
| `tests_data_floor::a_restricted_base_moves_to_the_fallback_with_data_gate` (T14) | Ten cases: no verdict (Jev down), late (held), unsure, shadow (route pack shadow), `-P glm` pin, an owner's correction to glm, a hold on glm, a session routed to glm, a `trivial` detour to glm, and routing off (no judge). Each runs on `default` / claude-sonnet-5-5, the last `route.decided` reason and the result's route say `data_gate`, `zai` records nothing, and no `data_gate.blocked` row is written: the route moved it before the chokepoint. |
| `tests_data_floor::no_usable_fallback_fails_the_turn_and_sends_nothing` (T15) | The default is made cn, routed and routing off. Both fail with "No model cleared for this request's data is usable" and `data_gate`, and both fakes record nothing. |
| `config::data_floor::tests::background_profiles_refuse_a_restricted_profile` (T6) | Each of the 7 keys set to `glm` is refused in words, and `haiku` loads. A DeepSeek `[judge] api_base` is refused, and another loads. |
| `tests_data_floor::the_trivial_default_skips_glm` | The code's default list `["haiku","glm","cheapest"]`, with `haiku` unpriced so the list reaches glm, runs a trivial verdict on `haikuhi` (claude-haiku-5-5). glm is skipped and zai is empty. |
| `tests_data_floor::a_named_request_still_needs_a_us_model` | A named message with Claude served from a host vouched EU ends before any call (sensitive_names). A pass for a named GLM request is still refused for the names. |
| `tests_data_floor::a_data_gate_failure_is_parked_never_retried` | `Failing::after(.., "data_gate", ..)` is `Park`, settled or not, so the driver's one retry (of `internal` only) never asks the model again. |
| `data::tests` ×2 | A closed clearance mints no pass; a pass covers its own digest alone. |
| `tests_data_state::turn_write_back_never_erases_a_stored_taint_or_moves_a_changed_run` (T11, **key**) | Through the stored bytes, as a turn reads them. |
| `tests_data_state::a_plain_turn_keeps_the_sessions_data_state` | Through a whole turn. |
| `tests_data_state::a_task_never_inherits_its_parents_run` (T20) | `task.start` from a parent with a run. The task's record has no data, the report lands (the parent's `reports`), and the parent's state is unchanged. |
| `tests_fork::fork_copies_data_state_through_the_fork_point` (T21) | Through `session.fork`. A `from` before the point gives `from..=point`, a `from` after it gives none, `opened` and `taint` are copied, and the source is unchanged. |
| `tests_data_state::a_forks_run_is_cut_at_the_fork_point` | The pure cut. A node with no position cuts the run (fails closed). |
| `tests_data_state::session_data_ledger_rows_round_trip` | Both rows, decoded back to `OpenHow` and `DataWhy`. |
| `tests_data_state::the_inspect_line_names_the_opening_the_runs_ends_and_the_taint` | The inspect line. |
| `tests_layouts::a_format_47_session_reads_with_no_data_state_and_takes_one` (T30) | The sample `SESSION_BEFORE_DATA` (also in `SAMPLES`) reads with none, writes back byte for byte, survives a write-back unchanged, and a state round trips. |

Planted reverts. Each was restored with `touch`, and `git status` was checked after each:

| Plant | Test that failed |
|---|---|
| A1: `guard`'s early return put back | `the_provider_refuses_a_restricted_model_without_a_pass` (line 81: GLM was sent it) |
| A2: `refuse`'s restricted-origin check off (`if false &&`) | the same key test (line 81) |
| A3: `DataPass::covers` returns true | `a_pass_covers_only_the_request_it_was_minted_for` (line 111) |
| A4: `data_floor` treats every turn as cleared | `a_restricted_base_moves_to_the_fallback_with_data_gate` (a case's turn errored at the chokepoint) and `no_usable_fallback_fails_the_turn_and_sends_nothing` |
| A5: no floor on the routing-off path (`floor_unrouted` skipped) | the same two |
| A6: `validate_data_floor` not called | `background_profiles_refuse_a_restricted_profile` |
| B1: `take_turns_fields` copies `data` plainly | `turn_write_back_never_erases…` at line 65 (the taint half) |
| B1b: `merge` takes the turn's run always | the same test at line 80 (the moved-run half) |
| B2: `task.start` copies the parent's `data` into the task | `a_task_never_inherits_its_parents_run` |
| B3: a fork copies the source's `data` whole | `fork_copies_data_state_through_the_fork_point` |

The relay change was also tested, then dropped (see §5).

FAST:
- `tests_m3::a_plain_turn_stays_within_its_frame_budget` and `tests_first_request.rs` pass unchanged; they are in the
  core suite, and `tests_m3`/`tests_first_request` were not edited.
- The gate's turn bench on the final commit, run by hand after the suite (`theseus-sim bench turn --check --runs 5
  --burst 0`): plain 4 frames (budget 4), p50 55.5 ms; tool-call 8 (budget 8); batch 14 (budget 14); syncs before the
  first byte 2 (budget 2). After part A alone it was the same frames, with plain at p50 35.9 ms. The wall times are
  this VM's and vary with load.
- First request at the p95: channel 7,391 tokens (budget 7,900), cli 7,332 (8,000), dm 9,411 (15,000), headless 7,846
  (8,600), task 6,585 (8,000).
- A plain turn writes nothing new (part B adds no write), and an unrestricted model's request adds one memoised map
  lookup.

## 4. The test sort

**Kind 1: the subject is a restricted model or the data floor.** These are rewritten to assert the floor.

- `data_gate.rs` `a_restricted_model_is_never_sent_a_sensitive_name`: its plain DeepSeek half reversed. The plain
  request is refused without a pass and sent with one for its digest, and a named request with a pass is still
  refused.
- `tests_data_gate.rs` `a_sensitive_name_skips_restricted_profiles`: the "plain" case moved to `glm53` as a Claude
  served from a host vouched **EU** (unrestricted, not US), so it still tells the two floors apart. A plain message
  runs there and a named one on sonnet.
- `tests_data_gate.rs` `a_sensitive_name_with_only_restricted_profiles_sends_nothing`: the default is made cn after
  the load (`rig_parts`), since the new config check refuses the owner-material keys on a cn provider. Its doc points
  to T15 for the plain half.
- `google/tests_private.rs` `a_private_session_is_routed_off_restricted_profiles`: the plain session's first profile
  is the EU-vouched glm53 (`rig_on(.., eu: true)`). The private session is still kept off it.
- `google/tests_config.rs` `no_google_section_registers_nothing_and_reads_nothing`: its "every provider stays
  unwrapped" half is reversed to "every provider is wrapped" (the floor is always on), and it now checks the plain
  turn was sent.

**Kind 2: GLM is only a second, cheaper model.**

Its profiles now run an unrestricted model on the same `zai` provider and fake: `Config::serve_glm_unrestricted`
(`#[cfg(test)]`; zai vouched `us` on an invented proxy host, and `glm`/`glm53` on claude-haiku-5-5), and theseusd's
`common::unrestricted_glm`. Profile names, providers, fakes and every assertion keep their meaning. Model literals
became `crate::config::UNRESTRICTED_SECOND`. The tests:

- `tests_route.rs`: `glm_placements` calls the helper, so every test that uses it in `tests_route`,
  `tests_route_model`, `tests_route_effort`, `tests_route_base`, `tests_standing_ring`, `tests_offer` and
  `tests_onto` keeps its detour or switch. `restricted_placements` is the same lists on real GLM, for
  `tests_data_floor`. Literals changed in `a_trivial_message_detours_and_the_next_prefix_is_byte_identical` and
  `profile_use_moves_a_routed_session_and_pins_nothing`.
- `tests_route_base.rs`: `a_routed_panes_p_profile_comes_back_once_routing_stops` and
  `a_switch_back_to_the_base_ends_the_move`. Their restarted rigs apply the helper, and their literals changed.
  `a_changed_place_profile_clears_the_move` has literals only.
- `tests_route_model.rs` `a_routed_turns_metrics_name_the_model_it_ran_on`: literals only.
- `tests_route_effort.rs` `a_model_without_effort_ignores_it`: the catalog override that turns effort off is on the
  stand-in model's row. It is still "a model whose row takes no effort".
- `tests_onto.rs`: `rig_with`'s namer, all its tests. The namer reads session text, so it must be unrestricted
  anyway under the new config check.
- `tests_compaction.rs`: `config()`, all its tests.
  - The summary model literals became the `GLM` const, now the stand-in.
  - The settled cost changed to Haiku 5.5's prices ($0.10/$0.50 a million): `9_000*10/100 + 120*50/100`.
  - `session_rig`'s catalog override gives the stand-in GLM's tokenizer rates (`bytes_per_token: GLM`), so
    `summary_profile_session_follows_a_one_message_override` stays in the band its window was tuned for.
- The summarizer rigs of `tests_standing_ring.rs`, `tests_projects.rs`, `tests_situation.rs` and `tests_one_turn.rs`
  (one line each).
- `tests_continuations.rs`: `config()`. This includes `glms_thinking_never_reaches_anthropic`: thinking stays with the
  provider (zai) that wrote it, and that still holds.
- `rpc/tests.rs`: `per_turn_provider_and_model_selection` and `live_profile_switch_persists_and_routes`, through
  `second_model()`, with literals changed.
- theseusd `tests/continuations.rs` (3 tests) and `tests/cache_header.rs`
  `the_glm_profiles_sessions_and_task_share_one_header`: `common::unrestricted_glm`.
- theseusd `tests/files.rs` `an_attached_pdf_reaches_each_model_as_it_reads_pdfs`: `Rig::start` gives the glm profile
  the stand-in, with a catalog override `pdf = false`. The half is still "a model that reads no PDFs gets the text page
  by page".
- `tests_route_correct.rs`: `ask` made `pub(crate)` for T14's correction case. No test changed.

The keel guard reported no findings for any of these (§8): no test was deleted and no assertion was dropped.

## 5. Every place a provider is built

- `Core::new` (`rpc/mod.rs`) builds one `Anthropic` per `cfg.all_providers()` and hands them to `Core::build` in
  `Parts`.
- `Core::build` puts them through `data_gate::guard` before anything else holds them, and `TurnRunner.providers` is
  that map. Turns, compaction, keep-warm (`keep_warm_send`), the census (`count_tokens`), replay and whole-turn replay
  (`own_bench`), the background calls (people, card, initiative, onto naming, the threads import's card, the learning
  writer, the audit, self-exams review, consolidation) all reach providers through `core.runner.providers`, so through
  `guard`.
- Tests build `Parts::for_tests` (and insert fakes into `parts.providers`), which also goes through `Core::build`.
- **Outside `guard`, and not fixed here:**
  - `rsi/sealed/relay.rs` (the sealed scorer's relay) POSTs `/v1/messages` to the profile's provider `api_base` with
    its own `reqwest` client. A restricted profile there would be sent sealed items without a pass. I added a refusal
    in `Relay::new`, but the keel guard treats that file as a keel file (it needs the owner's signed `Keel:` ack), so I
    took it out again. Follow-up below.
  - Jev's client (`theseus-judge`) calls `[judge] api_base` outside the core's providers. The new config check refuses
    a China-origin `api_base`.

## 6. The seam where the open-request row mints a pass

`turn/data_step.rs`, `TurnRunner::data_pass(t, &mut compiled)`, called in `turn.rs`'s loop right before
`call_model`. Today it is a no-op: a cleared turn's request would take `t.route.clearance.pass(&request.digest())`.
The row that judges a compiled request sets `RouteState.clearance` to a cleared `Clearance`; that needs a checked
constructor added in `data.rs`, since only `data.rs` can build one. A later loop's request is another request with
another digest, so it needs its own pass.

## 7. What could not run on the VM

- The 33 known L1 failures (theseus-pv6i: a root daemon's job has no job cgroup): the 20 theseus-sandbox contract
  tests and `bench spawn_100`, and theseusd's `tests/sandbox.rs` (13). Nothing in this row touches L1. The maintainer
  runs:
  `cargo nextest run --workspace -E 'package(theseus-sandbox) | binary(sandbox)'`.
- The lifecycle and jobs benches (`THESEUS_GATE_NO_BENCH`). The turn bench ran (§3).
- `cargo deny check advisories` fails offline on two advisories in today's fetched database, for `rustls-webpki
  0.103.15` and `webpki-roots 1.0.9`. Cargo.lock is unchanged by this row, so it fails on `main` too. Bans, licences
  and sources are ok.
- The live check, for the maintainer, on a scratch daemon of this build with the owner's keys:
  1. Start a scratch daemon with a config whose `[profiles.glm]` is on `zai` (api.z.ai). Run
     `theseus --socket <its socket> ask -P glm "Rename the call sites in the billing module."`.
     It should answer from the `[model]` default (the footer or `theseus why` says the route reason `data_gate`).
     `theseus --socket <its socket> ledger tail --kind route.decided` shows `"reason":"data_gate"`, and nothing is
     sent to z.ai (no `provider.call` row with provider `zai`).
  2. Same daemon with `[routing] enabled = false`, and `profile.use glm`, then a plain `ask`. It runs on the default,
     with a `route.decided` row whose reason is `data_gate`.
  3. Set `[ontology] name_profile = "glm"` in that config. `theseusd check --config …` refuses it: "[ontology]
     name_profile may not name a profile of restricted origin: …".
  4. `theseus --socket <its socket> ledger tail --kind data_gate.blocked` stays empty through 1 and 2. A row appears
     only if something reaches the chokepoint, such as a keep-warm or compaction on a GLM summary profile.

## 8. The keel guard

The gate's keel phase on the working tree before the part A commit:
`keel: ok (1d092dede7b6e1470ace3923f79d891fa80407c3..the working tree, 1d092dede7; 52 files changed, 0 findings acked, 1.5 s)`.

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` before the report commit:

```
(no output; exit status 0: no findings)
```

## 9. The store format

From 47 to **48** (`MANIFEST_FORMAT`, its doc line; the maintainer renumbers at the join). The sample is
`SESSION_BEFORE_DATA` in `crates/theseus-core/src/tests_layouts.rs`: a format-47 session with `project` and `dir`, in
`SAMPLES` with `Kept::All`, and its round trip `a_format_47_session_reads_with_no_data_state_and_takes_one`.

## 10. Follow-ups

- The sealed relay (`rsi/sealed/relay.rs`) should refuse a profile of restricted origin in `Relay::new`. It forwards
  outside `guard`. This needs the owner's `Keel:` ack, since the file is keel-guarded.
- The code's `trivial` default `["haiku","glm","cheapest"]` still names glm (theseus-a1wvq.4). Today a trivial verdict
  skips it.
- Compaction, consolidation, keep-warm and replay on a restricted profile fail at the chokepoint instead of falling
  back (theseus-a1wvq.10). Each failure is now a `data_gate.blocked` row, which can be counted to size that row.
- `[memory] summary_profile` and `synth_profile` naming a restricted profile still load; the chokepoint stops them.
  The later row gives them a fallback.
- `docs/status.md` and the spec should say that the data floor is on with no switch, that `data_gate.blocked` and
  `session.data` exist, and that the store format is 48 (renumbered at the join).
- Design note: the fallback is the `[model]` default only. A turn that is on a restricted profile could instead take
  the mode's next usable profile (`route_profiles` already skips it for verdicts). With the default unrestricted, that
  matters only for cost, not for the rule.
- Design note: the fork cut uses `through` when it lies before the fork point, not always the fork point. The brief's
  `from..=fork point` is the same when `through` is at or after the point, and never wider. This fails closed.
- `DataState` keeps, unstored, the run it was read with (`serde(from = "Stored")`) to make the write-back compare and
  set without changing `take_turns_fields`'s signature. If later rows hand the turn's start state over explicitly,
  that field can go.

## The gate

Final: `TZ=America/Phoenix CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `54fb4bf`.

The env lines are there because the VM's 39 GB disk allowance filled twice with incremental and debug artifacts. I ran
`cargo clean` once.

**Phases:**
- door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build, and reader rule: ok.
- suite: 7,268 tests run, 7,233 passed, 35 failed, 52 skipped.
  - 33 of the failures are the known L1 tests (theseus-pv6i): theseus-sandbox's 20 contract tests and `bench
    spawn_100`, and theseusd's `tests/sandbox.rs`.
  - The other 2 passed alone, three runs each, and neither touches this row's code. Both are load-sensitive and not
    on the flaky list:
    - `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`: "security.v3 took 606 ms",
      a time cap it misses under the suite's load on 4 cores; it takes about 0.58 s alone.
    - `theseus-discord tests_gateway::cards::a_reply_answers_the_question_it_replies_to`: a timed wait at
      cards.rs:77.

**The phases after the suite (`machine_checks`), run by hand:**
- protocol types: ok. `cockpit/src/protocol.gen/TurnInspectResult.ts` is committed.
- turn bench: ok (§3).
- size: skipped, no release build.
- lifecycle and jobs benches: skipped (`THESEUS_GATE_NO_BENCH`).
- deny: advisories FAILED, on rustls-webpki 0.103.15 and webpki-roots 1.0.9 from today's database fetch. Bans,
  licences and sources are ok. Cargo.lock is unchanged, so this is `main`'s state as well.

**Earlier gates:**
- On the part A tree: 7,227 passed; the only failures were the 33 L1 tests.
- An intermediate part B run also flaked, each passing alone:
  - `theseus::herdr a_line_sent_on_to_main_does_not_leave_the_pane_sending`;
  - `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` (once "dispatched 2,
    want 1"; it also failed once in a part A rerun and passed in the full part A gate);
  - the builders cap again.
- The same run caught `theseus-core store::tests::a_write_moves_an_older_store_to_this_builds_format`, whose format
  literal I then moved to 48.
