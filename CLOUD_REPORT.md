# Cloud report: CONTINUE's candidate signals and `continue.v1` in shadow, step 25b (theseus-0j2.7)

Branch `cloud/20261004-continue-shadow`, from `ed0a070` (main at `3add3f5` plus the task commit). Started 08:46 UTC,
code pushed 09:38, live check on scratch daemons 09:41 to 09:45, report about 09:50 (deadline 12:46).

## Commits

| Commit | Subject |
|---|---|
| `5a29842` | judge: an ask carries the judgment id the core mints at its dispatch (theseus-0j2.7) |
| `88d4d05` | compiler: CONTINUE's candidate signals, and continue.v1 in shadow at compile (theseus-0j2.7) |

The step is one piece of work, so it went in as these two commits, not one per sub-step. Pulling the signals apart
from the compile point would have meant splitting the health-list tests and `tests_continue.rs` across commits.

## What I found

- `compile()` already had everything the signals need: the session's nodes with their times, each answer's usage
  (cache reads) and `compilation_id`, the window, and the rendered request. The one thing missing was where the
  tail's messages begin in the request. `render_messages` now returns that index (`tail_from`, on `Messages` and
  `Rendered`), and the tail is sized from it with the existing `Census::of_messages(..).tokens(rates)`. It costs
  no extra render: one census pass over the tail, which is cheaper than the estimate's pass over the whole request.
- `Compilation.created_at_ms` is read from the real clock inside `make` (that code was already there). My signals read only
  the clock passed in (`SignalsAt.now_ms`), so the compilation's age is "passed-in now − created_at_ms".
- `theseus_judge::Ask` had no id, and `Judgment::pending` minted its own. I added the convention as the brief
  describes it.
- Health's pack list was pinned to `["loop.v1: shadow"]` by two core tests and by the daemon's
  `tests/judge.rs::a_start_with_the_judge_on_builds_nothing_of_it`. The first gate run caught the daemon one, and
  all three now list `continue.v1` too.

## What I changed

1. **`theseus-judge`** (`5a29842`): `Ask.id: Option<String>` (`Ask::new` sets `None`). `Judgment::pending` uses it
   when set, and otherwise calls the new `theseus_judge::new_id()`, the same `jdg_<uuid v7>` as before.
2. **The signals**, in `crates/theseus-core/src/signals.rs`, beside `compiler.rs` (`88d4d05`). Each one is measured
   against what was written since the model's last answer, so an input's signals fire once, at its turn's first
   compile:
   - `dormancy`: the first `UserMessage` since the last answer came more than `dormancy_minutes` after the node
     before it. Value: the minutes.
   - `tail_band`: the tail's band rose. Band 0 is under `tail_band` of the window, band 1 starts at it, and each
     further quarter adds one. "Before" is the tail through the last assistant message in the request. Value: the
     edge passed, in percent (50, 75, 100, …).
   - `report` / `wake`: a `UserMessage` from `Origin::Harness` authored `task:<short>` / `wake:<short>` arrived
     since the last answer. Value: how many.
   - `cache_miss`: the last answer read 0 cache tokens, the one before it read some, and both came from the
     compilation the request was rendered from (an unchanged prefix). Value: the earlier read.

   `CompileInput` gains one field (`signals: Option<SignalsAt>`, holding the thresholds and the clock) and
   `Compiled` gains one (`signals: Signals`: what fired, plus the window, the prefix and tail tokens, the last
   cache read, and the compilation's age). `None` reads nothing, which is what every existing compiler test passes.
3. **On `context.compiled`**: the new protocol type `theseus_protocol::signals::CompileSignal { name, value: u64,
   detail }` lives in a module of its own. `ContextCompiled.signals` is optional, absent when empty, and generated
   into the cockpit's TypeScript (`CompileSignal.ts`). It rides on the row, the notification and the span. The
   stored `Compilation` is untouched, so there is no format bump.
4. **`[judge.signals]`** in `config/judge.rs`: `dormancy_minutes = 360` and `tail_band = 0.5`, with template lines
   and checks (tail_band must be above 0 and at most 1; `dormancy_minutes = 0` fires at any gap, for tests and live
   checks). The template's prose now names both packs. The signals are computed whether the judge is on or off.
5. **The compile point**, `crates/theseus-core/src/judge/compile.rs`: `JudgeService::at_compile(&compiled,
   AtCompile{..})`.
   - Nothing is asked unless the compile appended (`!new_compilation`, so every trigger, overflow included, asks
     nothing) and fired a signal, and the pack is not off and is sampled (keyed `turn#loop`).
   - It mints the id, spawns the judgment, and returns the mark's attributes. turn.rs then marks the trace with
     `judge`, kind `mark`, attributes `pack`, `point`, `mode`, `judgment`.
   - The spawned task, off the turn's path, reads the execution's budget left (through a `Weak<Kernel>`) and the
     last operator message. It then builds and scrubs `ContinueInput`'s state, writes the blob, reserves, calls
     and settles, mirroring `loop.v1`'s path.
   - Its context is `decision`/`baseline: "append"`, scope `judge:continue`. 23a's loop path is unchanged.
     `judge/mod.rs` gains the `mod` line and the `WIRED` line.
6. **turn.rs**: 16 lines (the `CompileInput` field, the summary field, the dispatch and its mark), now 3,468 of its
   3,500 ceiling. compiler.rs is 2,391 lines, protocol lib.rs 2,555 of 2,618.
7. `crates/theseus-core/AGENTS.md`: a paragraph under Context naming the signals, the mark, and their tests.

## How I proved it

- **The new tests**:
  - **`tests_continue.rs`**, 10 tests:
    - Pure `compile()`, one per signal, each beside its near miss:
      - dormancy at 361 minutes fires; 360, 359 and 0 do not; a later loop of the same turn does not.
      - tail at 55 % of a 100k window fires `tail_band` 50; 45 % does not; 60 %→80 % fires 75; 60 %→65 % does
        not.
      - a report and a wake fire; a report already answered does not, nor an operator message named `task:…`.
      - a cache miss under the same prefix fires; a never-warm cache does not, nor one from another compilation,
        nor a warm one.
    - The signals change no digest or decision, and the compilation's age is by the clock passed in.
    - Through the core against `FakeJev`:
      - a signal and no trigger: one `judge:continue` row, keyed by the id the trace mark names, with its state
        blob (the signals, the strategy and trigger, the last human message, the window); health lists both packs.
      - **no signal, no judgment** (Jev saw two connections, both `loop.v1`).
      - **a deterministic trigger, no judgment** (a `manual_transcript` recompile still carries `dormancy` on its
        row).
      - **the request bytes are the same with the judge on and off** (three requests each, compared as JSON).
      - **a judged turn keeps its frame budget** (≤ 5 frames, with the mark present).
  - **theseus-judge**: `a_judgment_carries_the_id_minted_at_its_dispatch`.
  - **`config::judge`**: `[judge.signals]` parses, rejects an unknown key, and checks its band (0.0, 1.5 and −0.5
    are refused), and the template's section and defaults match.
- **Targeted run**: `cargo nextest run --workspace -E 'package(theseus-core) and (test(tests_continue) or
  test(config::judge) or test(tests_judge) or test(compiler::) or test(example_template))'` → 46 passed.
- **Under load** (four `while :; do :; done` loops at nice 0, the tests at `nice -n 19`, the binary prebuilt):
  `tests_continue` and `tests_judge`, 19 tests, three runs: 19/19 passed each time, about 29 s a run. The loops
  were stopped by their own pids. A first attempt also rebuilt theseus-core at nice 19, starved, and I stopped it
  by its pids; it proved nothing.
- **Planted reverts** (each file was restored from a copy, `touch`ed, and `git status` checked after):
  - Dormancy firing at any gap (`gap > 0 || dormancy_minutes > 0`):
    `a_dormancy_gap_fires_past_its_minutes_and_not_at_them` failed at its near-miss assertion
    (tests_continue.rs:171, "360 minutes fired"). Restored.
  - Dispatching even when a trigger fired (dropped `compiled.new_compilation ||`):
    `a_deterministic_trigger_asks_no_judgment` failed at tests_continue.rs:531 (a `judge:continue` row
    appeared); the other nine passed. Restored.
- **Goldens**:
  - Output golden (`THESEUS_GOLDEN=write`, under `TZ=America/Los_Angeles`): 6 lines change, all where a signal
    fires and nowhere else. They are the golden's two appends that take a task report or a wake (the `report` and
    the `wake` scenarios): each one's `context.compiled` row, compile span and notification gain `"signals":[…]`.
    No judge is on in those scenarios, so no other line moves.
  - Wire fixture `context_compiled_append.json` gains a `dormancy` signal, on purpose: the append sample now
    shows the shape. The recompile fixture keeps its bytes, since the field is absent when empty.
- **A local live check on scratch daemons** (this VM, fresh state dirs, Discord and web off; a fake model from
  `theseus-sim fake-model`; `[judge] enabled = true` with Jev at `http://127.0.0.1:9`;
  `[judge.signals] dormancy_minutes = 1`; secrets from `env:`):
  - Turn 1, a 125 s wait, then turn 2 in the same session.
  - Turn 2's `context.compiled` row was `append` with `signals: [{name: dormancy, value: 2}]`.
  - `theseus judge log` listed `continue.v1 (shadow) … failed: network` at turn 2's time, beside the two
    `loop.v1`s.
  - The turn's `turn.trace` held one zero-length `judge` mark whose `judgment` equals the `continue.v1` row's id.
  - A judge-off daemon with the same `projects_dir` (the system block names it), run the same way, gave
    request digests equal turn for turn: `407174d1…` for the recompile and `1ec0f8e2…` for the append.
  - Every daemon was stopped with `theseus shutdown` and the fake model by its pid.

## The live check for the maintainer (real Jev)

On a scratch daemon with a fresh state directory, its own socket, and a GLM profile, with the real key under
`[secrets] jev_api_key`:

```sh
S=/tmp/theseus-25b; mkdir -p $S/on $S/off $S/projects
# $S/on/config.toml: the scratch config you use for GLM checks (fresh state_dir and socket, [discord] and
# [web] off, [model] live = "glm", projects_dir = "$S/projects"), plus:
#   [judge]
#   enabled = true
#   [judge.signals]
#   dormancy_minutes = 1
# $S/off/config.toml: the same with [judge] enabled = false, its own state_dir and socket, the same projects_dir.
theseusd --config $S/on/config.toml --socket $S/on/sock --state-dir $S/on/state &
theseusd --config $S/off/config.toml --socket $S/off/sock --state-dir $S/off/state &
theseus --socket $S/on/sock health | grep judge     # judge: loop.v1: shadow, continue.v1: shadow …
for v in on off; do theseus --socket $S/$v/sock --json ask "Note the plan for the garden shed." > $S/$v/t1.json; done
sleep 125
for v in on off; do
  sid=$(jq -r .session_id $S/$v/t1.json)
  theseus --socket $S/$v/sock --json ask -s "$sid" "And the paint colours?" > $S/$v/t2.json
done
theseus --socket $S/on/sock ledger --json -k context.compiled -n 2
theseus --socket $S/on/sock judge log
theseus --socket $S/off/sock ledger --json -k context.compiled -n 2
theseus --socket $S/on/sock ledger --json -k turn.trace -n 1   # the second turn's trace
for v in on off; do theseus --socket $S/$v/sock shutdown; done
```

What it should show:

- The second turn's `context.compiled` (on and off alike) is `"decision": "append"` with
  `"signals": [{"name": "dormancy", "value": 2, "detail": "the new input came 2 minutes after the node before it"}]`.
- `theseus judge log` on the judge-on daemon shows a `continue.v1 (shadow)` judgment at the second turn, answered,
  with its `decision` choice. It sits beside two `loop.v1` judgments. No `continue.v1` appears at the first turn
  (`new_session` is a trigger).
- The second turn's trace has one `judge` mark (`pack: continue.v1`, `point: compile`, `mode: shadow`) whose
  `judgment` is that row's id.
- The `digest` of each turn's `context.compiled` row is equal across the two daemons. This needs the same
  `projects_dir` (and the same system and context files), because the system block names it.

## What is left, uncertain, or for the owner to hear

- **Design choices the owner should hear about:**
  - *A signal's value is a number plus words.* `CompileSignal` carries `value: u64` (minutes, percent, count, or
    tokens) and `detail` (words). Jev's `SignalInput.value` gets the words. The number is there for the learning
    ledger (25c) and the soft bands' tuning.
  - *"Since the last answer".* Every signal is measured against what was written since the model's last answer.
    So `tail_band` and `cache_miss` can fire at a later loop of a turn (tool results crossing a band), and
    `continue.v1` is then asked mid-turn, in shadow. Dormancy and arrivals fire only at the loop that first sees
    the input.
  - *Dormancy uses the first input after the last answer.* If a turn failed before answering, a later turn
    measures the older, unanswered input's gap, not its own.
  - *The sizes are estimates from bytes* at the catalog's figures, with images left out. `prefix_tokens` is the
    request's estimate (counted where the provider counted it) less the tail's estimate.
  - *`budget_left_usd`* is the execution's `Budget::available()`, read off the turn's path. A task's is its own
    execution's.
  - *Tasks' turns are judged too.* Nothing restricts `continue.v1` to conversations.
- **FAST.** The signals add one census pass over the tail's messages and two scans over the session's nodes per
  compile. On the path, the dispatch adds a `ContinueInput` built from numbers and a few short strings, and a
  spawn. I did not add a bench row for the signals; the turn bench's frames are unchanged (5 plain, 9 tool-call).
  The sink's frame can land inside a later turn's window, as 23a found (theseus-0j2.3, 0j2.8). A turn's own frames
  never change.
- **Docs to write at review:**
  - The spec's Part III item for 25b.
  - `docs/design/m5-judgment.md`:
    - §2.4: CONTINUE's signals are measured since the last answer, and a signal is a value plus words.
    - §2.15: `[judge.signals]`'s `dormancy_minutes = 0` fires at any gap, and `tail_band` must be in (0, 1].
    - §3 25b: done.
  - `docs/status.md`: the roadmap row, and "`continue.v1` in shadow, the signals on `context.compiled`".
- **Merges:** the other sessions that dispatch judgments in turns will each add a `WIRED` line, so health's pack
  list (and the three tests that pin it) will grow at each join. `ContextCompiled` gained a field, and the
  TypeScript needs regenerating at the merge.

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run on the final tree:

- **Up to the suite**, every phase passed: fmt, shape, features, clippy, cockpit, test build, and the reader
  rule (9/9).
- **The suite**: 1,972 tests run, 1,939 passed, 33 failed, 17 skipped. Every failure is an L1 sandbox test that
  needs a non-root user (theseus-pv6i: "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC"):
  - theseus-sandbox::contract: `a_job_that_cannot_start_says_why`, `clause_01` to `clause_12`, the three
    `egress_18b_*`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`,
    `sigterm_is_forwarded_to_the_command`.
  - theseus-sandbox::bench: `spawn_100`.
  - theseusd::sandbox: 13 tests (`a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace`, `l1_argv_routes_a_call_to_l1`,
    `health_reports_the_last_real_l1_launch`, `the_jobs_bench_l1_row`, and the other L1 job tests).
- **The phases after the suite**, run by hand: protocol types ok (the generated TypeScript was added); the turn
  bench (`theseus-sim bench turn --check --runs 5 --burst 0`) gave 5 frames plain against a budget of 5 and 9
  tool-call against 9, ok; `cargo deny check` gave advisories, bans, licenses and sources all ok. The lifecycle
  and jobs benches were skipped (`THESEUS_GATE_NO_BENCH`).
- **`TZ`**: the output golden's wake line prints the local UTC offset, and the committed golden holds a negative
  one. On this UTC VM that test fails whatever the change (`-#:#` becomes `+#:#` on two wake lines). So I ran the
  gate, and wrote the golden, under `TZ=America/Los_Angeles`. On the owner's machine no `TZ` is needed. The golden
  should probably pin its time zone (a follow-up, not done here).

So the commit counts as green under the brief's rule: the suite failed only on the known cases, and every phase
after it passed.
