# CLOUD_REPORT: p2f-caps (branch cloud/20261024-p2f-caps)

## 1. For the maintainer first

- **Store format:** 49, unchanged. Nothing is stored (`Dreamed` is in memory; the card changes ride the request).
- **Files over a ceiling:** none. No ceiling raised.
- **OWNER CALLs:** none. `dm_flow.txt` is untouched.
- **L1:** took the **code rule** (second-word arm), not the AGENTS.md sentence.
- **Cross-call conflicts:** **landed**, with no extra model call.
- **Provider adapters stripping `strict`:** none. Every provider builds its body in one place (`ProviderRequest::body`, the
  Anthropic wire; GLM and DeepSeek ride it too), so nothing strips `strict`. I could not check offline that GLM's and
  DeepSeek's endpoints accept `strict` on a tool. They get it only when a profile points one of these calls at them
  (the defaults are Haiku). **Live check worth doing** before such a profile is used.
- **Commits:** two, not three. 0ly2a and yym79 both change `card/ask.rs` and `card/run.rs`, interleaved (the `request`
  signature takes both `earlier` and `how`), so they land as one commit (60810dd). wdzmj is its own (d4cfefe).
- **Disk:** this VM's allowance filled twice during the gate (debuginfo, incremental). Final gate used
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.
- **`cargo deny` is not installed here** (I did not run `cargo install cargo-deny`), so the deny phase was skipped.

## 2. Per issue

### theseus-0ly2a (60810dd)
- **L1** `evidence.rs about_owner`: a first word that is not in `NOT_A_NAME` (setup-file orders and determiners) followed
  by a third-person verb (`third_person`: ends in s but not ss/us, over 3 letters, or is/has/was/does) is kept. Test
  `a_setup_line_that_starts_with_the_owners_name_is_kept`. Plant: drop the second-word arm → test fails.
- **L2** `addressed`: one helper `has_phrase` (boundary before and after) used by `addressed` and `one_task`. "heartbeat"
  and "bootstrap" moved to `BARE_MACHINERY`: count only if the line also has a "you" word. Test
  `a_notes_wake_update_and_heartbeat_line_is_the_owners`. Plant: back to `contains` → fails. The existing `addressed`
  assertions pass.
- **L3** `ask.rs turned(text, s, cites)`: only lines of cited episodes count. Test
  `a_statement_on_a_clusters_other_lines_is_not_turned` (a hand-built `Shown`; the lines I tried did not cluster).
  Plant: all lines → fails.
- **L4** `rpc/card.rs card_answer`: a decline with a scope is refused before anything is read or written. Test
  `a_decline_with_a_scope_is_refused` (invalid; `last_position` unchanged; the proposal can still be kept). Plant → fails.
- **Cross-call:** `Job.earlier` (text, cluster); `ask::request(.., earlier, ..)` lists `P1..` in the user message only;
  `parse(.., earlier)` records `Parsed.earlier_clash` for `P` labels; `Job::statements` drops those statements and notes
  `kept '<earlier>' over '<new>': they conflict`. System text and tool are identical across calls (tested). Tests
  `a_conflict_across_two_calls_of_one_run_is_not_offered`, `the_earlier_statements_sessions_route_the_later_call`
  (uses the fake's request gate to mark the earlier cluster's session private between the two calls). Plants: `parse`
  ignores P labels → fails; session list leaves earlier clusters out → fails. Dry run: `dry_worst_case` counts, for call
  n, `min(n*BATCH, room)` entries of `TEXT_CHARS` characters.
- **OUT:** the 18-declined re-draft, `work:coding`, a conflict with a record kept in an earlier run, the live check.

### theseus-wdzmj (d4cfefe)
- `Wait` enum (Profiles, Profile, Provider, ThinksUnasked(model), Unreadable, NoCategory, NoPrice; the last is a seventh
  cause: the category missing from the snapshot, formerly a silent `?`). `dream_request` returns `Result<_, Wait>`.
  `Dreamed.waits: BTreeMap<String,u64>`; `waiting` stays the total; the tender logs `reasons`.
- Docs: DeepSeek added; `dream_request` doc; `Dreamed.waiting` doc; core AGENTS.md clause.
- Test `a_waiting_category_says_why` (Opus profile names `claude-opus-5-5 thinks unasked`; disagreeing profiles). Plant: one
  reason for every cause → fails.
- OUT: the live check.

### theseus-yym79 (60810dd)
- `Catalog::may_force(model)`: true when `thinking_off` is `disabled`, or none and not `thinks_unasked`. `Catalog::forcing`
  returns `provider::one_tool::Forcing {thinking, force}`. `one_tool(tool, force)` returns `(tool, tool_choice)`; not forced
  gives `auto`, `strict: true` and `additionalProperties:false` on every object (top, `properties`, `items`), `required` untouched.
- The five `request`s take `how: Forcing` in place of `thinking`: card/ask.rs, threads_import/card.rs, judge/people/extract.rs,
  judge/onto/name.rs (only `request`/`tool` plus `pub(crate)` on `request`, `TOOL`, `mod name`), rsi/exams/review.rs
  (its hard-coded `strict` and `additionalProperties` moved into the helper; Fable gets the same bytes as before).
- Tests: `a_tool_is_forced_untouched_or_left_to_auto_strict_and_closed`, `a_tool_may_be_forced_only_where_thinking_is_plainly_off`
  (Haiku 5.5/4.5/Opus 5/unknown yes; Sonnet 5.5, Opus 5.5, Fable 5.1, GLM, DeepSeek no), and one per call via
  `one_tool::assert_both_shapes` (Haiku 5.5 forced+`disabled`; Sonnet 5.5 auto+strict+closed+`between_tools`). These are
  request-level tests with the builtin catalog, not through a core. Plants: the helper always forces; `may_force` always
  true → the helper, catalog, five per-call and the reviewer's tests fail.

## 3. New request shapes
| call | Haiku 5.5 | Sonnet 5.5 |
|---|---|---|
| card, thread card, people, onto namer | `tool_choice` `{"type":"tool","name":T}`, no `strict`, `thinking` `{"type":"disabled"}` | `{"type":"auto"}`, `strict: true`, closed schema, `{"type":"between_tools"}` |
| exam reviewer | forced, no strict, `disabled` | auto, strict, closed, `between_tools` (Fable 5.1: auto, strict, closed, no thinking, as before) |

Cross-call list worst case: 7 earlier calls × 8 entries = 56 entries × (160 chars + "Pn: ") ≈ 9.3k characters, roughly
2.3k to 3k input tokens on the eighth call. The dry-run bound counts it.

## 4. Gate
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`: clean (two functions in run.rs were over 100 lines; split).
- Targeted: `cargo nextest run -p theseus-core card:: guidance catalog exams thread_import people onto one_tool`: 298 passed.
- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit, test build,
  reader rule green. The suite failed only on: the known L1 tests (theseus-sandbox contract and `spawn_100`, theseusd
  `sandbox`), and two theseus-core tests that failed under load and **pass alone**
  (`tests_m3::agent_answers::the_digest_names_an_agent_that_answered_and_says_you_for_the_owner`,
  `tests_route::a_switched_turns_compilation_names_its_recall_drops`). I ran the later phases by hand: protocol/theseus/discord
  1563 passed on rerun (one discord test, `tests_walk1b::a_dm_message_routed_to_a_placeless_thread_still_sees_its_tasks`,
  failed once in the first run and passed alone and on rerun). The generated TypeScript is unchanged (no protocol type
  changed). Benches skipped (NO_BENCH); `cargo deny` not run.

## 5. Keel and DM golden
- `scripts/keel-guard.py --base 9b075d6d --head HEAD --quiet`: `keel: ok (... 0 findings acked)`, no findings.
- `git diff --exit-code 9b075d6d -- crates/theseus-discord/src/golden/dm_flow.txt`: empty (exit 0).

## 6. Join notes
- `judge/onto/name.rs` (p2f-onto): only `request` (now `how: Forcing`, `pub(crate)`), `tool` use, `TOOL` and `mod name` made
  `pub(crate)` in `judge/onto/mod.rs`.
- core `AGENTS.md`: clauses in the card, guidance and context items.
- Changed signatures: `ask::request(model, how, shown, earlier, scrub)`, `ask::parse(content, shown, earlier)`,
  `Job::scrubbed(core, picked, earlier)`, `threads_import::card::request`, `people::extract::request`, `name::request`,
  `review::request` (each `thinking: Option<Value>` → `how: Forcing`). Callers: card/run.rs, rpc/thread_import.rs,
  judge/people/run.rs, name.rs, review.rs.

## 7. docs/status.md sentence and follow-ups
"The owner card keeps a setup line that starts with the owner's name, matches machinery phrases on word boundaries, drops
a statement only for the lines it cites, refuses a decline with a scope, and names a conflict with an earlier call's
entry; dream guidance says why a category waits; and the four forced-tool calls plus the exam reviewer force their tool
only where thinking is off (otherwise auto with a strict, closed tool)."
Follow-up: live check that GLM/DeepSeek endpoints accept `strict`; per-call tests through a core on a Sonnet profile.

## 8. Data-routing review
- Card calls: the request still carries scrubbed lines of the shown clusters, and now also the model-written text of earlier
  proposals of the same run (`P1..`). `card_call` passes `over_sessions` every shown episode's session **and** every episode's
  session of each earlier proposal's cluster (`Job::sessions`), so a private session behind an earlier entry makes the later
  call private (a non-cleared model gets nothing). Test: `the_earlier_statements_sessions_route_the_later_call`.
- Forcing/strictness changes only `tool_choice`, `strict` and the schema; no content changes. Thread-card, people, onto and
  reviewer requests carry the same text and route as before. Guidance waits carry model names only.
