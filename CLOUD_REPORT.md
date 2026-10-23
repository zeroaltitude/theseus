# CLOUD_REPORT: tail7b-tiers (theseus-d9je7 rows 2, 4, 5)

Branch `cloud/20261021-tail7b-tiers`, based on `ae420a49`. The session started at 2026-10-23 01:41 UTC.

## First: the template line for the signed merge, and the long files

The keel guard counts the template's `# recall_budget_tokens = 1500` as a cap key, so I left it alone. The Rust
default (`config/memory.rs`, `default_budget`) is now 3,000. In the lane's signed merge, the maintainer applies this
line to `crates/theseus-core/config/theseus.example.toml` (line 1056 on this branch). The owner approved it on
2026-10-22:

```
# recall_budget_tokens = 3000             # the pack's tokens
```

Until that line lands, one test stays red for that reason alone, as the brief allows:
`config::tests::example_template_uncommented_still_parses`. It reaches `config::memory::the_templates_memory_section`,
which asserts that the uncommented `[memory]` section equals the defaults, and the only difference is
`recall_budget_tokens: 1500` against `3000`. I did not change the test.

No file is over its line ceiling, and I raised no ceiling. `turn.rs` is 3,469 / 3,525 (+2 lines: the `mod tier;`
line and one call). `theseus-protocol/src/lib.rs` is untouched.

## The seam: `InjectionBudget` and the tier fold

The new module is `crates/theseus-core/src/turn/tier.rs`, with one `mod` line in `turn.rs`.

- **`Tier`** is `Small`, `Ordinary` or `Deep`. **`tier_of(Seen, &Config)`** folds the verdict:
  - `trivial` → small;
  - `sophisticated`, `deep_coding`, `review` → deep;
  - any other mode → ordinary;
  - a verdict under its mode's bar (`RoutingConfig::confidence_for`) → ordinary, fallback `unsure`;
  - a pending verdict → `verdict late`;
  - Jev gave none or can't be reached → `no verdict`;
  - routing doesn't decide the turn (off, pinned, shadow, steered, continuation) → `routing off`.
- **`InjectionBudget { tier, fallback, recall_tokens, recall_items, admit_ceiling }`** is built by
  `InjectionBudget::of`. `pack()` gives the recall sizes: small packs at ordinary's sizes, because its trivial detour
  drops the pack, as today (theseus-n7nc). A trivial verdict that doesn't detour (target already the base, capped)
  therefore keeps today's recall. `row()` is the ledger form.
- **Where it lives.** `RouteState.injection: Option<InjectionBudget>` in `route_step.rs`. It is set at the recall pack
  (`TurnRunner::injection(t, fix=true)`), or in `record_route`, beside `result`, when no pack ran
  (`injection_read`).
- **Two other `RouteState` changes.** I made `wait` and `mode` `pub(super)`, so the peek can read them. That is a
  visibility change only; tail7-threads may also touch `RouteState`.
- **The ledger.** The `route.decided` row gains `injection`:
  `{tier, fallback, recall_tokens, recall_items, admit_ceiling}`. I added a field to the `RouteDecided` fact
  (`fact/route.rs`); the checkpoint's row passes `None` (one line in `checkpoint_step.rs`).
- **`theseus why`** says it in one line, from the row (`theseus-protocol/src/why.rs`, `injection_words`):
  `injection: deep: recall 8,000 tokens, 16 items; tools to 3x the place's budget`. A fallback reads
  `injection: ordinary (verdict late): recall 3,000 tokens, 8 items; tools to 2x the place's budget`. Small reads
  `injection: small: no recall (a trivial detour carries none); tools to 1x the place's budget`.

## How recall gets the tier in time

Recall still runs before `compile_routed` (no reorder in `turn.rs`). In `recall_live`, once the index has answered and
before the pack (`recall_reranked`), `self.injection(t, true)` peeks at the route verdict's oneshot:

- If the verdict has landed, it is read and put back in a fresh oneshot, so the compile's own wait (`beside`) takes it
  exactly as before. `turn::tier::tests::a_peek_leaves_the_verdict_for_the_wait` proves the verdict, `at` and
  `cost_micros` come through unchanged.
- If it hasn't landed, the pack is ordinary with `verdict late`.

The budget is then fixed for the turn. Nothing waits: no new model call, no new await. The sizes ride
`Scene.budget_tokens` (already a per-turn override) and a new `Scene.max_items`. `Memory::params_of` and `refill` take
`max_items` too, so the live rerank's and the cross judge's repacks keep the deep item count. Without that, a live
rerank would have cut a deep pack back to 8.

**How often it will fall back.** The peek comes after the index's answer, so the deep pack applies whenever the verdict
lands before the index does. On the audit's figures (index about 190 ms, verdict p50 129 ms, p90 205 ms), somewhat
more than half to most turns should get their tier; the late tail packs ordinary. I can't measure the live
distribution here: the fake Jev answers in about 7 ms and the stand-in index in under 1 ms, so the tests set the order
explicitly. After a day of real turns, the maintainer can count it with the first live check below.

## The cache rule

The tier changes only:

- the recall pack, which is in the tail, after the message;
- the families a recompile loads. An append keeps the tools its compilation recorded (`tools_defer::forks`).

It writes nothing to the persona layer, the context files, or the header. `the_tier_never_changes_a_cached_prefix_byte`
runs two turns in one session, ordinary (`chat`) then deep (`deep_coding`, on the session's own profile so it is no
switch). It checks that the deep pack really applied (budgets 3,000 then 8,000), that the system JSON and tools are
byte-equal, that the first message is equal, and that the second compile is an `append`.

## Config keys and defaults

| key | default | was |
|---|---|---|
| `[memory] recall_budget_tokens` (ordinary) | 3,000 | 1,500 |
| `[memory] recall_max_items` (ordinary) | 8 | 6 |
| `[memory] deep_recall_tokens` (new) | 8,000 | — |
| `[memory] deep_recall_items` (new) | 16 | — |
| `[memory] session_recall_cap_tokens` | 64,000 | 12,000 |
| `[tools.defer] admit_ceiling` (ordinary) | 2.0 | 2.0 |
| `[tools.defer] admit_ceiling_small` (new) | 1.0 | — |
| `[tools.defer] admit_ceiling_deep` (new) | 3.0 | — |

- **Validation.** Both token keys must be > 0 and ≤ `session_recall_cap_tokens`. Both item keys must be 1 to 40. Each
  multiple must be 1.0 to 3.0. Tests: `config::memory`'s bad-value list and `config::defer::tests`.
- **Why flat keys beside `admit_ceiling`.** They read simplest, and an old note that sets only `admit_ceiling` keeps
  meaning what it meant (ordinary).
- **The rollback.** Deep's recall keys at ordinary's numbers, and the three multiples alike. This is in core
  AGENTS.md and the template comments.
- **The template.** `[tools.defer]` has the two new lines (`the_templates_defer_section_is_the_default` asserts them).
  `[memory]` has 64000 and 8, plus the two deep lines commented. The 1500 line is untouched (see the first section).

## Admission by tier, and what deep 3.0 admits

`deferral_in` now takes the tier's ceiling, and `Fit::of` gets `ceiling.unwrap_or(admit_ceiling)`. The census and keep-warm
callers pass `None`, so the census's fresh request is ordinary and unchanged.

I had to change `deferral_in`'s signature, not only the `Fit::of` line: `ask` and `jev` are now one tuple with the
ceiling. tail7b-budgets also edits `deferral_in` (its trivial case), so expect a small textual conflict there.

The first compile's tools are settled before the pack. `defer_tools` therefore uses the budget when it is already
fixed, which happens when a peek just before it (`self.injection(t, false)` in `turn.rs`) finds the verdict landed. For
a model without tool search, `families_ready` has just waited for Jev's families answer, so the verdict has usually
landed by then. Otherwise admission is ordinary, today's. A routed recompile (`route_to`) takes the fixed budget.

What deep 3.0 admits on the test shape (`admission_follows_the_tier`): the cli place (budget 8,000), the owner's
shape without its context file, sonnet and opus with no tool search, and every hidden family judged relevant. The
deep verdict routes to opus, so its tools are settled again on the routed spec after the verdict.

- ordinary (2.0, ceiling 16,000): aws, aws-hands, aws-logs, aws-review, aws-s3, aws-stacks, channels, projects,
  threads. Request 15,769 tokens.
- deep (3.0, ceiling 24,000): the same, plus extensions, reminders, routines and tasks. Request 23,711 tokens.

**A choice the owner should hear.** Small at 1.0 means that, until tail7b-budgets lands, a trivial detour that names a
family (for example "call aws_whoami") no longer loads it past the place's budget. The model reaches it with
`tools_find` instead: the owner's "find-a-tool" for a trivial request. Four existing tests prove the raise mechanism
itself (theseus-xlri5) on trivial detours:

- `tests_families::a_cli_detour_naming_aws_loads_it_past_the_budget_by_exactly_its_size`;
- `tests_families::a_family_jev_judges_relevant_loads_without_being_named`;
- `tests_families::jev_late_or_unreachable_falls_back_to_the_named_rule`;
- `tests_route_defer::a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget`.

I changed no assertion in them. Their rigs now pin the small multiple to the one under test:
`admit_ceiling_small = admit_ceiling` in `tests_families`' `detour()` and `rig_on`, and `admit_ceiling_small = 2.0` in
`tests_route_defer`'s `rig_detouring_to`, each with a comment. The keel guard does not flag this. If the owner would
rather small stay at 2.0 until tail7b-budgets lands, change one default (`admit_ceiling_small` in `config/defer.rs`,
and its template line and test).

## The walk, measured (row 5)

- `ContextCompiled.walk_tokens: u64` (protocol `events.rs`; serde default, absent when 0). I regenerated only
  `cockpit/src/protocol.gen/ContextCompiled.ts`, and the wire fixture builder in `tests/wire.rs` gained the field.
- `compile_step.rs` fills it (`TurnRunner::walk_tokens`): it composes the walk over the memberships the compile used,
  and counts each section with `context_parts::text_tokens` at the model's rates. That is the same count
  `context.explain` gives its `guidance` parts, and the test asserts the two are equal (558 tokens on the test's
  guidance).
- `theseus why` already lists each guidance part with its tokens (from `context.explain`), so I added no `why` line
  for the walk.
- No walk budget was added.

## Request sizes before and after (the test rig)

`the_request_sizes_before_and_after`: a fresh session, twenty harbour notes of about 120 tokens each, `context.compiled`
`est_tokens`. "Before" means the old 1,500 tokens and 6 items for every prompt.

| prompt | before | after |
|---|---|---|
| ordinary (`chat`) | 19,968 | 20,334 |
| deep (`deep_coding`) | 19,968 | 21,787 |

## Tests and their planted reverts

New tests in `crates/theseus-core/src/tests_injection_tiers.rs` (9) and `turn/tier.rs` (3). Each plant was restored
from a backup copy and `touch`ed, and I ran `git status` after each.

| test | plant | what failed |
|---|---|---|
| `a_deep_prompt_gets_the_deep_recall_pack` (key) | `scene()` ignores the tier: `false => (None, None)` | `left: (3000, 8, "ran")  right: (8000, 16, "ran")` |
| `a_late_verdict_packs_ordinary_and_says_so` | the pack waits for the verdict (peek in a 50 ms loop, up to 3 s, before fixing) | `left: (8000, 16)  right: (3000, 8)` |
| `an_ordinary_prompt_gets_three_thousand_and_eight` | `default_budget` back to 1,500 | `left: (1500, 8)  right: (3000, 8)` |
| `a_trivial_detour_still_has_no_recall` | the fold sends `trivial` to ordinary | `left: ("ordinary", 8, 2.0)  right: ("small", 0, 1.0)` |
| `the_tier_never_changes_a_cached_prefix_byte` | `compile_step` appends "(a deep prompt)" to the session block for a deep tier | the system JSON differs (`…Directory: …\n(a deep prompt)`) |
| `admission_follows_the_tier` | `Fit::of` gets `cfg.admit_ceiling` for every tier | `left: 16000  right: 24000` (deep's ceiling) |
| `the_walks_tokens_are_on_context_compiled` | `summary.walk_tokens = 0` | `left: 0  right: 558` |
| `why_names_the_tier_and_its_pack` | `why.rs` drops the injection line | the expected line is missing from the lines |

- `the_request_sizes_before_and_after` is a measurement; it asserts that after is larger than before, ordinary < deep.
- The `turn::tier` unit tests are `each_mode_folds_to_its_tier_and_the_rest_to_ordinary`,
  `the_defaults_are_the_approved_numbers` and `a_peek_leaves_the_verdict_for_the_wait`.
- **My first prefix plant didn't bite.** I first planted in `defer_tools`, keyed on the ceiling, and the test passed.
  On a session's second turn the tier isn't fixed yet at `defer_tools` (it is fixed at the pack), so the plant never
  ran. I moved the plant to the compile, where the tier is known, and the test failed as it should.
- **One existing test followed the new default.** `tests_recall_node::a_canary_turn_puts_its_recall_in_front_of_the_model`
  asserted the pack's limit was 1,500; I changed it to 3,000.

**How the rig orders the verdict.** In `after_the_verdict`, the stand-in index answers a turn's query only after Jev
has read that turn's route request, plus 150 ms. Turns are counted by distinct `as_of`, because one turn can ask the
index more than once. This is order, not timing. The late test uses the fake's `Held`, released 1 s into the turn.

**Suites.** `cargo nextest run -p theseus-core` whole (`TZ=America/Phoenix`): 4,135 run, 4,129 passed, 6 failed on the
first run. Those were the template test (above), the four trivial-detour raise tests, and `tests_recall_node`. After
the fixes, those plus the tiers ran 23 of 23 green. `-p theseus-protocol`: 200 of 200 green. The suite's run inside the gate is under "The gate" below.

**Under load** (AGENTS.md's recipe: `nice -n 19`, beside four busy loops at nice 0, killed by their pids):

- **The first version of `admission_follows_the_tier` failed in all three runs, and the fault was the test's.** It held
  Jev's families answer until the route verdict had landed, and under load the release came after the families wait's
  2 s bound, so nothing was raised at either tier.
- **The fix.** I reshaped the test: the deep verdict now routes to opus (no tool search), and the routed recompile
  settles the tools after the verdict, every time. That fix is commit `3485d62f`. Its plant (one ceiling for all) still
  fails: `left: 16000  right: 24000`.
- **The three runs after it**, over the tiers, `turn::tier`, `tests_families`, `tests_route_defer` and
  `tests_recall_node`: 26 run each time.
  - Two runs: 26 of 26 passed.
  - One run: 25 of 26. The failure was `tests_recall_node::a_stalled_index_holds_a_canary_turn_no_longer_than_its_deadline`,
    a test this row doesn't touch. It asserts that a turn on a single-threaded runtime ends in under 2 s of wall time,
    and starvation at nice 19 passes that. It isn't on the flaky list; it passed the other two runs and the unloaded
    suite..

## Keel guard

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing and exited 0:

```
(no output; exit 0)
```

## Store format

Unchanged at 49. New fields are on a ledger row (`route.decided`'s `injection`) and a notification or row
(`context.compiled`'s `walk_tokens`), both JSON with serde defaults and absent at their defaults. No stored record
layout changed, so there is no bump and no old-layout sample.

## Live check for the maintainer (a scratch daemon of this build, never the operator's)

1. One message of each kind on a scratch daemon with recall live and the route pack live. Then:
   `theseus --socket <its socket> why`. It should show one `injection:` line per turn: `deep: recall 8,000 tokens,
   16 items; tools to 3x …` for a design question, `ordinary: recall 3,000 tokens, 8 items; tools to 2x …` for a chat,
   `small: no recall …` for "thanks!".
2. How often the pack falls back:
   `theseus --socket <sock> ledger tail --kind route.decided --json | jq -r '.rows[].data.injection | "\(.tier) \(.fallback)"' | sort | uniq -c`.
   The share of `ordinary verdict late` is how often a deep or small verdict came after the index. Expect most
   verdicts on time at the audit's figures.
3. `theseus --socket <sock> memory recalled <session>` on a deep turn: `budget_tokens` 8000, up to 16 admitted.
4. `context.compiled` rows in a session whose category has guidance carry `walk_tokens` equal to `theseus why`'s
   guidance parts.

## Left, uncertain, and choices

- **Small at 1.0 before tail7b-budgets** (see admission above). It is the approved number, but it removes the
  named-family raise from trivial detours until that row lands.
- **Admission on a session's later turns** usually runs before the verdict lands for a model with tool search. That
  doesn't matter, because the `Find` raise applies only to models without tool search, where `families_ready` has
  already waited. A routed recompile takes the fixed tier.
- **`InjectionBudget::of` reads `self.cfg`.** The recall sizes come from `Config.memory`, while the pack's defaults come
  from `memory.cfg()`. They are the same config at a start; a live reload that changed one and not the other would
  mix them for a turn.
- **One commit.** The step is one commit, not one per sub-step: the seam, recall, admission and the walk share the
  test file and the config change. The VM has no signing key, so commits are unsigned (`git commit -S` not possible).
- **Follow-ups.**
  - A walk budget after a week of `walk_tokens` numbers (the owner's ask).
  - Whether small's 1.0 should become moot by tail7b-budgets' trivial-tool change.
  - Whether the cockpit should show `injection` and `walk_tokens`.

## Docs for the integrator

- **A `docs/status.md` sentence.** "Recall and tool admission are sized to the prompt (theseus-d9je7): the route pack's
  mode folds into small, ordinary and deep; recall packs 3,000 tokens and 8 items for an ordinary prompt and 8,000 and
  16 for a deep one, and never waits for the verdict (a late one packs ordinary and says so on `route.decided` and in
  `theseus why`); a session's tail may hold 64,000 tokens of recall; admitted families may raise a deep prompt's
  first request to 3x the place's budget; and `context.compiled` carries the ontology walk's tokens, measured only."
- **Spec.** Part III item for d9je7 rows 2, 4, 5. The design doc of the audit, if there is one, should name the three
  tiers and the fold.
- **core AGENTS.md.** The recall and deferral paragraphs are updated in this commit.

## The gate

Commits:

- `fbda205b`: the step.
- `3485d62f`: the admission test made load-proof.
- This report.

I ran the gate as `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit.
`CARGO_INCREMENTAL=0` was needed because the first run filled this VM's disk allowance in the test build. I removed
`target/debug/incremental` and ran it again.

**The suite phase failed on known cases only.** 7,971 tests ran: 7,936 passed, 35 failed, 53 skipped.

- 34 are the L1 tests that fail as root here (theseus-pv6i):
  - theseus-sandbox's `contract` tests (19);
  - its bench's `spawn_100` (1);
  - theseusd's `sandbox` tests (14).
- 1 is `theseus-core config::tests::example_template_uncommented_still_parses`, for the one template line the signed
  merge applies (see the first section).

Every phase before the suite passed: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test
build, and reader rule.

**The phases after the suite, which I ran myself** (`machine_checks`):

- protocol types: `cockpit/src/protocol.gen` is staged with the change, so clean;
- turn bench: `theseus-sim bench turn --check --runs 5 --burst 0` exited 0, and a plain turn writes 4 frames;
- size: skipped, as there is no release build of this tree;
- deny: `cargo deny --offline check` reports advisories, bans, licenses and sources ok;
- the keel guard: exited 0 with no output.

`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` are clean. No new dependencies:
`Cargo.lock` and the package locks are unchanged. Commits are unsigned (no key on this VM).
