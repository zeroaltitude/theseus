# Cloud report: `route.v1`, Jev's model per interaction mode (step 25e, theseus-0j2.11)

Branch `cloud/20261004-route`, on `main` at 802f913. Started 18:14 UTC, report 19:25 UTC.

| Commit | What |
|---|---|
| 7cf6f81 | judge: route.v1, Jev's model per interaction mode, live at inbound (theseus-0j2.11) |

The step is one commit. Its parts (pack, config, the pure rule, the turn's wiring, learning, CLI) only make sense
together, so I proved them together rather than committing a pack nothing reads (the reader rule). The full gate ran
on exactly the tree that was committed.

## What I found

- `at_inbound` (judge/inbound.rs) spawned classify.v1 and role.v1 in shadow with `Urgency::Shadow` and returned
  nothing, so there was no way to hand a verdict back to the turn.
- The turn's `Target` is fixed before `Turn::start` (`&'a Target` in `Turn`, and in `TurnCtx`), and the provider is
  fixed in `run_inner`. Switching mid-turn needed a target that outlives the `Turn`. I used a
  `OnceLock<Target>` declared in `run_inner` before the `Turn`: the routed target is set once and borrowed for `'a`.
- `compile_step` persisted every new compilation inside the step, so "persist only the compilation the call uses"
  needed a defer flag (`RouteState.defer_persist`).
- The compiler stripped thinking only from messages **another provider** wrote. A detour to another Anthropic
  model (for example a `cheapest` that is Haiku) would have sent that model's thinking signatures to the session's
  model. Nodes record the **requested** model (`provider.rs`: `model: req.model.clone()`), so comparing models is
  deterministic. I made the render drop thinking when the model differs too.
- `TurnSubmitParams` had no way to tell a chosen profile from one the CLI's pane carries from the last turn.
- No `TurnRequest` field was needed: the pin rides on `Target` (one constructor).

## What I changed

**1. The pack.** `packs/route.v1.toml`, in `pack.rs`'s `EMBEDDED`. Its state is classify.v1's (`state = "inbound"`,
cap 3000, `jev-1.13.0`); a test checks the state is byte-identical and the model and cap match. It has one deciding
Choice, `mode`: trivial · chat · sophisticated · deep_coding · routine_coding · other. It adds a new `Baseline::SessionProfile`
and `Action::Route`, a golden request (`fixtures/golden/route.v1.request.json`), and a row in the shape test.
`WIRED` gives it `PackMode::Live`. A pack with an action must name rollback rules (the loader's rule). I chose
`labels_per_day "wrong model" 3` and `on_path_p95 max 250 ms over 20`. **The owner should confirm these.**

**2. `[routing]`** (`config/routing.rs`, sparse, in the template with its own section and template test):
`enabled` (true), `mode` (live|shadow), `max_wait_ms` (200), `trivial_context_turns` (2),
`cold_switch_tokens` (30000), `switch_confidence` (0.6), and `[routing.modes.<mode>] profiles` with the step's
defaults. Each mode table is optional, so setting one keeps the other defaults. `cheapest` is reserved (a profile
named so is refused). A mode may name a profile that isn't configured; it is skipped as unusable, because the
defaults name `opus`, `fable` and `glm53`, which a sparse note may lack. `[judge.packs."route.v1"] mode` lowers it
too. The template gains `[profiles.opus]`, `[profiles.fable]` and `[profiles.glm53]`.

**3. The pure rule** (`routing.rs`):
- **Usable:** the provider is built, its key is `Ready` or not on the board at all (as `await_secrets` treats an
  absent key), its model has a catalog row, and it can read images when the turn has one.
- **`cheapest`:** by a short turn's cost at catalog prices (4,000 uncached input tokens, 500 output).
- **`pick`:** walks a mode's list. Empty list: the session's own. None usable: `fallback`.
- **The place cap:** a profile dearer than the place's own is passed over, with reason `capped`. The next profile
  under the cap still wins, so a detour below the cap still happens.
- **`decide`:** detour or switch, the cold-switch hold, and the confidence check (which applies to detours too, the
  default the step suggested).

**4. The turn** (`turn/route_step.rs`):
- `inbound_step` gives `route.v1` its mode: live, or shadow when the owner pinned the turn.
- `at_inbound` returns a oneshot. A live call uses `Urgency::live(total_secs)`, so it waits for a permit instead of
  being shed.
- `compile_routed` replaces the first loop's `compile_step`. It compiles on the session's profile, then waits at
  most `max_wait_ms` (`beside`), then decides:
  - **Stay:** persist the first compile, as before.
  - **Switch:** set the routed target, rebuild the spec, compile again (the model change recompiles and strips the
    prefix's thinking), and use the routed provider. `session.routed.profile` and `last_target` move.
  - **Detour:** compile the persona header and the last `trivial_context_turns` exchanges plus the message
    (`detour_start`), with no context files and no ontology walk, never persisted. The finish keeps the session's
    own `last_target` (`ran_on`).
- **When no verdict is used:** a late verdict is kept by the judge, in memory and per session, and the next message
  takes it when its own verdict doesn't arrive in time. A pinned turn, shadow mode, or an open breaker never waits.
- **The base:** an unpinned input turn runs on `session.routed.profile` (`route_base`).
- `SessionRecord.routed {profile, hold}` bumps `MANIFEST_FORMAT` to **14**, with a layout sample in `tests_layouts`.

**5. The owner's choice.**
- `TurnSubmitParams.carried` is a new field. The pane sets it on the profile it carries. `routing::chosen` treats
  any profile/provider/model named without `carried` as a pin (`Target.chosen`), so older clients' `-P` stays a pin.
- `profile.use` is untouched and is no pin.
- `theseus prompt -P` and the cockpit's picker send `profile` without `carried`, so both are pins.

**6. Records.**
- The `route.decided` fact (`fact/route.rs`, `LedgerKind::RouteDecided`) rides in the turn's next frame. It records
  mode, confidence, judgment, from, profile, reason, detour, switch, est_tokens and wait_ms, plus a `route` mark in
  the trace and a narrative line.
- `TurnSubmitResult.route {mode, reason, from}` is a new protocol module (`route.rs`); the TypeScript is regenerated.
- The CLI's status line reads `[glm → zai/glm-5.3-flash · trivial (detour) · …]`.
- Judgments are scoped `judge:route`. Their context has `pinned` and, for a pin, `chosen`.
- `report.rs ACTING` gains `("route", "mode", [trivial, sophisticated, deep_coding, routine_coding])`.
- The system label `chosen` (weight 0.5) is in `learning/system.rs`. When the session's next judged message, within
  10 minutes after a routed turn, is pinned:
  - if the chosen profile is in exactly one mode's list, that mode is the label;
  - otherwise the label is `{"not": mode}`;
  - if that one mode is the routed mode itself, no label.

**Other files touched:**
- 25a's tests turn route.v1 off (`inbound_only`, the health lists in tests_judge, tests_continue and theseusd's
  tests/judge.rs), so they keep testing their two packs.
- The frame-budget test now expects 4 marks.
- theseusd's versions test now uses format 14/15.
- The herdr test expects `carried: true`.
- crates/theseus-core/AGENTS.md has a Routing bullet.
- Long files:
  - `compiler.rs` ceiling raised 2540 → 2550 (the thinking rule).
  - `protocol/lib.rs` ceiling raised 2678 → 2690 (`carried`, `route`, `pub mod route`).
  - `turn.rs` is 3,436 lines, under its ceiling of 3,523.

## How I proved it

**Unit tests**
- `routing::tests` (7): each mode's first usable profile, then the next, then the session's own; unconfigured
  names; `cheapest` (with an image, glm-5.3 is skipped); the detour; `cold_switch_tokens` and agreement; nothing
  under `switch_confidence` (0.0, 0.3, 0.599; 0.6 routes); the place cap (a detour below it, `capped` above it,
  the next profile under the cap); the break-even numbers.
- `config::routing::tests` (4), and the template's section test.
- `turn::route_step::tests` (3), on tokio's **paused clock**:
  - with a 100 ms compile and a 200 ms bound, a verdict that never comes releases the call at exactly 300 ms;
  - one 50 ms after the compile (at 150 ms) releases it at 150 ms;
  - one during the compile (40 ms) releases it at 100 ms, so the compile is not delayed;
  - 299 ms gives 299, 301 ms gives 300 and `late`;
  - a dropped sender ends the wait at once.

**Core tests** (`tests_route.rs`, 9 tests). The fake Jev, with profiles on two fake providers: `anthropic` (sonnet,
opus, fable) and `zai` (glm, glm53).
- One request asks `classify.v1/kind`, `role.v1/role` and `route.v1/mode`, and its row says `call.packs = 3`, mode
  live.
- A hard question goes to Opus: the session moves, and its one stored compilation is Opus's.
- Routine coding goes to glm-5.3 on the zai fake.
- The detour (trivial goes to glm, which is 5.3 Flash):
  - it sends 3 messages;
  - the session's `compilation_id`, `last_target` and `routed` are unchanged;
  - the next request on Sonnet starts with the earlier request's system, tools and messages, byte for byte.
- `cold_switch_tokens = 1`: the first turn says `cache_hold`, the second agreeing turn switches, and a turn under
  the confidence says `unsure`.
- A pinned turn stays on Sonnet. Its judgment is shadow with `pinned: true` and `chosen: "profile sonnet"`.
- The pane's `carried` is no pin; `-P`, `-p` and `-m` are (through `TurnSubmitParams` JSON).
- Each fake mode leaves the request byte-identical to the judge-off request on Sonnet, under 2 s:
  - Down, 429 and Malformed say `no_verdict`.
  - Slow (3 s) says `late`.
  - With the judge off, the result has no `route`.
- A late verdict (Slow 500 ms against the 200 ms bound) leaves turn one on Sonnet, says `late`, and moves turn two
  to Opus on turn one's judgment.
- The system label: a pinned `-P glm53` 9 minutes after a routed `deep_coding` turn labels it `routine_coding`
  (rule `chosen`); 11 minutes after labels nothing.

**Judge crate:** 122 tests, including the new shape row, the golden request, and the byte-identical state.

**Under load:** `nice -n 19` with four busy loops at nice 0 (pids 21558 to 21561, killed by pid afterwards). Three
runs of `tests_route`, `route_step` and `tests_inbound`: 19/19 passed each time.

**Planted reverts** (each restored and `touch`ed; `cmp` against the saved copy, and `git status` clean of them):
1. The verdict awaited before the compile (in `beside`). `the_wait_runs_beside_the_compile_and_never_past_its_bound`
   failed with `left: (200, Late) right: (300, Late)`.
2. The detour's compile sent through the session's compile step, which writes the session's compilation.
   `a_trivial_message_detours_and_the_next_prefix_is_byte_identical` failed with "the detour wrote no compilation"
   (`cmp_…595e` vs `cmp_…a9`).

**Turn bench, judge off** (`theseus-sim bench turn --check --runs 5 --burst 0`): frames_plain is 5 (budget 5) and
frames_tool is 9 (budget 9). The frame-budget test with the judge on still counts 5 frames of its own; the
`route.decided` row rides in the turn's next frame.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passed fmt, shape, features, clippy, cockpit, test build and the reader
rule. The suite ran 2331 tests: 2297 passed and 34 failed, all on this VM:
- **32 of them are `theseus-sandbox::contract`, `theseusd::sandbox` and `theseus-sandbox::bench spawn_100`:** the
  daemon runs as root, and Linux exempts root from `RLIMIT_NPROC` (theseus-pv6i; spawn_100's message says so).
- **`theseus-core tests_output::the_cores_output_matches_its_golden`:** the VM runs in UTC. The golden holds a wake
  time's offset as `-#:#`, and the VM prints `+#:#` (lines 1182 and 1202; the diff is only that sign). It passes
  with `TZ=America/Los_Angeles`. This is not mine; the golden was left as it is.

The phases after the suite, run by hand:
- protocol types: ok, after `git add`;
- turn bench: ok (as above);
- `cargo deny --offline check`: advisories, bans, licenses and sources all ok;
- lifecycle and jobs benches: skipped under `THESEUS_GATE_NO_BENCH`.

No new dependency. `Cargo.lock` and the package-lock files are unchanged.

## The live check (the maintainer's)

Build the step and use a fresh state dir:
```sh
scripts/build.sh --profile release-thin
S=/tmp/route-live && mkdir -p $S && cd $S
cat > config.toml <<'EOF'
[server]
state_dir = "/tmp/route-live/state"
socket = "/tmp/route-live/theseus.sock"
[model]
live = "sonnet"
[profiles.sonnet]
provider = "anthropic"
model = "claude-sonnet-5-5"
[profiles.glm]
provider = "zai"
model = "glm-5.3-flash"
[profiles.opus]
provider = "anthropic"
model = "claude-opus-5-5"
[profiles.fable]
provider = "anthropic"
model = "claude-fable-5-1"
[profiles.glm53]
provider = "zai"
model = "glm-5.3"
[providers.zai]
api_base = "https://api.z.ai/api/anthropic"
api_key_secret = "zai_api_key"
[secrets]
anthropic_api_key = "op://<vault>/<Anthropic item>/notesPlain"
zai_api_key = "op://<vault>/<Z.ai item>/notesPlain"
jev_api_key = "op://<vault>/<Jev item>/notesPlain"
[judge]
enabled = true
[discord]
enabled = false
[web]
port = 7436
EOF
~/projects/theseus/target/release-thin/theseusd --config $S/config.toml --socket $S/theseus.sock --state-dir $S/state &
T="$HOME/projects/theseus/target/release-thin/theseus --socket $S/theseus.sock"
```
`[routing]` is at its defaults (no table needed). Then run one session with no `-P`:

1. Send `$T ask "Weigh two designs for a crash-safe write-ahead log: a single append-only file with checksummed frames, against segment files with a manifest. Which recovers faster after a crash, and why?"`.
   The status line should read `[opus → anthropic/claude-opus-5-5 · sophisticated (verdict) · …]`. Take the session
   id from it as `$SID`.
2. Send `$T ask -s $SID "thank you!"`. It should read `[glm → zai/glm-5.3-flash · trivial (detour) · …]`. Then
   `$T sessions` should still name opus as `$SID`'s profile.
3. Send `$T ask -s $SID "rename these twelve call sites the same way: read_frame to read_record"`. It should read
   `[glm53 → zai/glm-5.3 · routine_coding (verdict) · …]`. Its context is under 30,000 tokens, so the switch is at
   once. Above it you would see `(cache_hold)` first.
4. In a session with an open task (ask the model to `task.create` one, then keep the conversation going), send two
   routine-coding turns on glm53. The second's status line `cache r…` should be non-zero. **If it shows `cache r0`,
   GLM may ignore the block-level breakpoint behind the moved task view: report it.**

Then check the records:
- `$T judge log` should list three `route.v1` judgments. Each should share its call id with a `classify.v1` and a
  `role.v1` (`judge log --json`: the same `call.id`, `call.packs = 3`), and route.v1's mode should be `live`.
- `$T ledger --kind route.decided` should show reasons `verdict`, `detour`, `verdict`,
  with `wait_ms` at most 200.
- The cockpit at http://127.0.0.1:7436 should show `route.v1` live in the Judgment section, with the three rows.

Stop it with `$T shutdown`.

## What is left, or uncertain

- **Two new reasons.** I added `unsure` (a verdict under `switch_confidence`) and `no_verdict` (Jev failed or
  skipped, the budget, the breaker) beside the brief's eight. The brief's list had no word for either.
- **The late verdict is in memory** (`JudgeService.late`, per session), so a restart loses it. Its judgment row is
  durable. It is a hint for the next message, so I judged best effort enough.
- **A switched turn records loop 0's `context.compiled` and `loop.started` twice.** The first names a compilation
  that is never stored. A detour records `loop.started` twice too. Readers of `context.compiled` that fetch the
  compilation by id should tolerate a missing one. Recording the first compile only when it is kept is a small
  follow-up.
- **The detour offers the trivial profile's tools.** History in the last exchanges may hold `tool_use` blocks, which
  the API needs tools defined for. A detour that calls a tool keeps its later loops on the detour, compiled the same
  way.
- **The base of an unpinned turn is the session's routed profile**, even when `profile.use` later changes the live
  profile. Routing state outranks the live profile for that session. The owner may want `profile.use` to clear
  `routed`.
- **Health's pack list shows `route.v1: live` from the judge's ladder alone.** `[routing] mode = "shadow"` or
  `enabled = false` is not reflected there. The turn itself follows `[routing]`.
- **Spend.** Live route judgments are paid from the judge's shadow day budget (the point's one reservation), as
  rerank.rs does, until 26b's kernel actions. When the day's limit is reached, nothing is sent and the turn doesn't
  wait.
- **The place cap is proved at the unit level** (`routing::tests`) and wired through `TurnCtx.ceiling`. No core test
  binds a place with a ceiling profile.
- **The cockpit's session header does not show the route yet.** It is cheap now that `TurnSubmitResult.route` exists,
  but it is cockpit work in another lane's area.
- **The thinking rule now also compares models.** It is deterministic (nodes store the requested model). Its one
  cost: a session whose tail holds a `-P` answer from another model on the same provider re-renders it without
  thinking on its next request after the upgrade, which is one cache miss.
- **Break-even of the switch rule.** The cold write is priced at the target's cache-write rate; the saving per turn
  is the difference in cache-read cost over the context plus the difference in output cost.

  | Switch | Context | Output/turn | Cold write | Saved/turn | Break-even |
  |---|---|---|---|---|---|
  | Sonnet → GLM 5.3 | 30k | 1k | $0.042 | $0.0038 | 11 turns |
  | Sonnet → GLM 5.3 | 30k | 2k | $0.042 | $0.0094 | 4.5 turns |
  | Sonnet → GLM 5.3 | 100k | 1k | $0.14 | negative | never |
  | Sonnet → GLM 5.3 | 100k | 2k | $0.14 | $0.0052 | 27 turns |
  | Opus 5.5 → GLM 5.3 | 30k | 1k | $0.042 | $0.0138 | 3 turns |
  | Opus 5.5 → GLM 5.3 | 100k | 1k | $0.14 | $0.0096 | 15 turns |

  GLM 5.3's cache read ($0.26/M) is dearer than Sonnet's and Opus's ($0.20/M), so only output pays a switch back.
  Sonnet → Opus never pays in money: it is a quality choice. The rule's "two agreeing turns" above 30k is a
  confidence guard, not a payback: from Sonnet, at 1k output a turn, a switch to GLM 5.3 needs about 11 turns at 30k
  to pay. If the owner wants money to decide, a per-switch break-even check from these rates is a small addition to
  `routing::decide`.
- **Docs to write at review** (I touched none):
  - m5-judgment.md:
    - §2.4's table: a `route.v1` row (inbound, batched with classify.v1; Choice `mode`; baseline: the session's
      profile; live action: route).
    - §2.7's rollback table: route.v1's two rules.
    - §2.15: `[routing]`.
    - §2.16: a "Turn, live" row for routing (at most `max_wait_ms` after the first compile).
  - The spec's Part III item for 25e, and docs/status.md.
- **A follow-up for 30c:** compaction's `summary_profile = "jev"` waits on this step (as the brief says). It could
  now use routing's `cheapest`.
