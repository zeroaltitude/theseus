# Cloud report: the Jev wire-in, step 23a (theseus-0j2.1)

Branch `cloud/20261004-jev-wire-in`, cut from `main` at `d9b0931`. Started 02:30 UTC, finished about 04:05 UTC (the
deadline was 06:30). Setup went as the prompt says: nextest and cargo-deny installed, `npm ci` in `cockpit/` and
`web/`, the workspace built, and `cargo deny fetch` done, so the deny phase ran with advisories.

## Step 23a: `loop.v1` in shadow, recorded, priced, and acted on by nothing

### What I found

- `theseus-judge` already had what the core needed: `JevJudge` (pricing, batching, the breaker, strict parsing),
  `Recording` with a `JudgmentSink`, the `loop` builder over a plain `LoopInput`, `JevPrice::jev_1_13_0`, and the
  fake Jev with every mode the task names. The core change is wiring only. Nothing in the judge crate changed except
  its manifest: the `reserved_for` marker is gone, since theseus-core is now its reader.
- The core's turn end, `TurnRunner::run`, already has one point after the turn's last frame is written and its
  session hold is dropped. The judgment is handed off from there.
- The design's §2.5 needs no new record kind. Judgments are LEDGER rows, with a key and a scope that `NewRecord`
  already carries, and the budget is one META record under a new key. **So `MANIFEST_FORMAT` is unchanged and there
  is no layout sample.** An older binary reads these rows as rows of a kind it doesn't know, which the ledger already
  allows.
- The core's catalog table (`CatalogEntry`) is a model's row (window, cache prices, thinking), not a judge's. The
  template's `[catalog]` copy is being removed by another change. So Jev's price lives beside the speech prices:
  `catalog::judge_prices()`, built in only. The models' table and the template's prices are untouched.
- The time zone matters on this VM (see the gate section): it runs in UTC, and two existing tests assume the owner's
  negative offset.

### What I changed

Commit `a7fd996`, the step itself. Commit `cd18eec` is a fix to one test's timing bound, plus health not reading
the store when the judge is off.

- **`[judge]`** (`crates/theseus-core/src/config/judge.rs`, plus one field, one `validate` line, and one template
  test line in `config.rs`):
  - keys: `enabled` (false), `key_secret` (`jev_api_key`, an existing `[secrets]` entry, so no new secret source),
    `max_mode`, `shadow_limit_usd_per_day` (1.0), `max_in_flight`, `connect_secs`, `total_secs`, `api_base`, and
    `[judge.packs."<pack>"] mode / sample`;
  - `mode_of` takes the lowest of the ladder's mode (shadow for every pack until 26a), `max_mode`, and the pack's
    own line, so the config can lower a pack's mode and never raise it;
  - the loader refuses an unknown key, an unknown pack, a sample outside 0 to 1, and an enabled judge with no key
    entry;
  - the template has the section, held by the three template tests.
- **Jev's price**: `catalog::judge_prices()` holds `jev-1.13.0` at the judge crate's figures. A pack whose model has
  no row is skipped as `unpriced` and never called.
- **`JudgeService`** (`crates/theseus-core/src/judge/mod.rs`):
  - It is built with the core, but reads, builds, and sends nothing then.
  - The first judgment builds the client, the breaker, and the sink's task.
  - The key is read from the secret board at each call (`BoardKey`). An unsettled key skips the judgment with
    `no_key`.
  - `TurnRunner` gets one field (`judge`). `run` gets one call after the turn's last frame: `judge.after_turn(res,
    is_task)`. It spawns `loop.v1` only when the baseline ended the turn (`stop_reason == "no_tool_calls"`), within
    the pack's sample (a sticky hash of the turn id), and returns at once.
  - The spawned task does all the work: it reads the session's nodes, builds the state (scrubbed by the core's
    `Scrubber`, capped by the builder), writes the blob, reserves, calls, and settles.
  - It holds the service only around the two blocking halves, never across the HTTP call, so a stop never waits on
    Jev to release the store.
- **The sink** (`judge/sink.rs`):
  - Every judgment is a `judge.call` row, whether answered, skipped, or failed. The row holds the whole `Judgment`:
    pack and version, the pack's sha256, the state's digest and size, every answer with its band, usage, cost,
    timing, the outcome and its error class, and the core's context (session, execution, turn, loops, baseline,
    workload class, the blob's digest), plus `budget: "shadow"`.
  - Each row is keyed by `jdg_…` and scoped `judge:<pack id>` (`judge:loop`).
  - Rows are written in the sink's own frames: up to 32 rows, or whatever lands within 2 s of the first. A frame
    also carries `judge.circuit` rows when the breaker moves, `judge.shed` rows (one a minute at most), and the
    budget's META record.
  - The state's blob is written before its row.
- **The shadow budget** (`judge/spend.rs`):
  - The limit is per local day, using `wake::local`, the daemon's own time zone.
  - Blocks of $0.01 are written to META `judge.budget` before the calls that draw on them, and settled spend rides
    in the sink's frames.
  - A judgment that would pass the limit is skipped and counted, never queued, and writes one `judge.paused` row a
    day.
  - At this process's first judgment, a reserved rest from earlier today is booked as spent, with one
    `judge.block_booked` row. That covers a crash, and any restart, as the design says: never before serving.
- **Facts** (`fact/judge.rs`): `JudgeCall`, `JudgePaused`, `JudgeBlockBooked`, `JudgeCircuit`, and `JudgeShed`,
  with five new `LedgerKind`s (`judge.call`, `judge.paused`, `judge.block_booked`, `judge.circuit`, `judge.shed`).
  23a gives them rows only; notifications, sentences, and spans are 23b's.
- **Health** (`crates/theseus-protocol/src/judge.rs`, plus one field in `lib.rs`; the TypeScript is regenerated in
  `web/src/protocol.gen/`):
  - The `judge` block reports: enabled, `max_mode`, each wired pack's effective mode, the breaker (`idle` before the
    first judgment, then `closed`, `open (Ns left)`, or `half_open`), calls in flight, the day, calls today, failures
    today, skips today, spend today, the limit, and whether shadow is paused.
  - The CLI prints it as health's `judge:` line (`crates/theseus/src/render/judge.rs`).
- **`theseus judge log [-n N] [--session S]`**: the newest `judge.call` rows through the existing `ledger.tail` (no
  new protocol method; 23b adds `judge.list/get`). One line a judgment: time, pack and mode, session, each answer
  with its probability and band (or `failed: <class>` / `skipped: <reason>`), cost, and Jev's milliseconds.
- **The lifecycle bench** (`theseus-sim/src/lifecycle.rs`, `bench_config`): `[judge] enabled = true`, with Jev at
  `http://127.0.0.1:9`, and its config test asserts it. The turn bench uses the same config.
- **The reader rule**: `crates/theseus-judge/Cargo.toml` lost its `[package.metadata.theseus]` table, and
  `tests_registry` passes. `Cargo.lock` gained one edge (theseus-core → theseus-judge) and no package.

Shared files touched, each with only a field, a call, or a `mod` line:
- `turn.rs`: one field, and three lines in `run`;
- `config.rs`: a `mod`, a field, a validate line, and a template test line;
- `theseus-protocol/src/lib.rs`: one field and a `mod`;
- `rpc/mod.rs`: building the service in `Core::build`;
- `rpc/methods.rs`: health's field;
- the CLI's `main.rs`: the `Judge` subcommand and its dispatch arm;
- the config template: the `[judge]` section.

### How I proved it

**The tests.** `crates/theseus-core/src/tests_judge.rs` has 9 tests, run against the fake Jev:
- `a_turn_that_ends_with_no_tool_calls_is_judged_once_in_shadow` checks that:
  - there is one row: kind `judge.call`, keyed by its id, scoped `judge:loop`, with the session and turn,
    `loop.v1` v1, mode and budget `shadow`, outcome answered, and `work_state` = complete in the act band;
  - the blob exists at the state's digest and holds the ask and the final text;
  - Jev got a bearer of the key's length, and the key appears nowhere in the row;
  - `cost_micros > 0`, and the session's cost equals the turn's;
  - health shows `loop.v1: shadow`, one call, the same spend, and the breaker closed.
- `a_judged_turn_keeps_its_frame_budget`: a plain turn with the judge on still writes at most 5 frames.
- `a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`: `Down` → `network`, `Slow(10s)` → `timeout`, `429`
  → `rate_limited`, `Malformed` → `malformed`. For each mode:
  - the turn's output, stop reason, loops, usage, and cost, and the whole provider request (as JSON), equal a
    judge-off core's;
  - the turn takes under 3 s;
  - health counts 1 call and 1 failure.
- `a_jev_that_is_down_opens_the_breaker_and_later_judgments_skip`: outcomes are 5 × `network`, then 2 ×
  `circuit_open`, with exactly 5 connections, one `judge.circuit` row (opened, failures 5), and health's breaker
  `open`.
- `an_off_judge_or_pack_calls_nothing`: with the judge disabled, a pack's `mode = "off"`, or `max_mode = "off"`, Jev
  gets 0 connections and there are 0 rows.
- `an_unpriced_jev_model_is_never_called`: with empty prices, the outcome is `unpriced` and there are 0 connections.
- `a_tiny_day_limit_pauses_shadow_with_one_row`: with a $0.0002 limit and 6 turns, some are judged, the rest skipped,
  with exactly one `judge.paused` row (`limit_micros` 200). Health shows paused, calls plus skips = 6, and spend ≤
  $0.0002.
- `a_restart_books_the_blocks_rest_at_its_first_judgment`, on a second core on the same store:
  - before any judgment, health reads the store's settled spend and no row is booked;
  - after the first judgment, there is one `judge.block_booked` row with `reserved_micros` 10,000, and spend is over
    $0.01.
- `the_loop_input_reads_the_turns_calls_and_the_ask`: the ask, the minutes since it, and the calls and their class.

Unit tests elsewhere:
- `config::judge` (3): never raises; off by default, and the template says so; the loader's checks.
- `catalog::every_packs_jev_model_is_priced`.
- `judge::tests::a_share_samples…`.
- The CLI's `render::judge` (2).

**The daemon test.** `crates/theseusd/tests/judge.rs` (3 tests) runs the real `theseusd` with the stand-in Messages
API and the fake Jev:
- `a_start_with_the_judge_on_builds_nothing_of_it`: with Jev at 127.0.0.1:9, health answers with the breaker `idle`
  and `loop.v1: shadow`.
- `a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes`:
  - one `judge.call` row, answered, with its cost;
  - health counts it, with the same spend;
  - the model's request body is byte-identical with the judge on and off, once each rig's temp dir is replaced by
    `<dir>`.
- `a_kill_mid_block_books_the_rest_at_the_next_starts_first_judgment`:
  - Jev is slow; the test sends SIGKILL once the call is out, then restarts;
  - health shows $0 spent and no booked row before a judgment;
  - after one turn, there is one `judge.block_booked` row with `booked_micros` 10,000, and spend is over $0.01.

**Planted reverts.** Each file was restored and `touch`ed, and `git status` was clean after each:
1. *The turn waits on the judgment*: `rt.spawn(judge_loop(..))` → `block_in_place(|| rt.block_on(judge_loop(..)))`.
   `a_failing_jev_is_recorded_by_its_class_and_changes_no_turn` failed with "Slow(10s): the turn took 5.027584766s"
   (and with the earlier bound, "Slow(3s): the turn took 1.021788297s").
2. *The sink's write is dropped*: the `self.store.append(&records)` in `sink.rs` replaced by a no-op.
   `a_turn_that_ends_with_no_tool_calls_is_judged_once_in_shadow` failed with "0 of 1 judgments recorded".
3. *The config raises a mode*: `mode_of` returns `own.min(self.max_mode)`.
   `the_config_lowers_a_packs_mode_and_never_raises_it` failed.

**Under load** (AGENTS.md's recipe: the tests at `nice -n 19`, beside four busy loops at nice 0, killed by pid):
- The suite was the 13 judge tests in theseus-core plus the 3 daemon tests: 16 tests, 5 rounds.
- The first time, every round failed one test: `a_failing_jev…` had an absolute 900 ms bound on the turn, and under
  load a judge-off-speed turn took 1.2 s ("Down: the turn took 1.215249381s").
- I changed the bound to a margin that still catches the planted revert: Jev slow for 10 s with a 5 s total, and
  the turn must finish under 3 s. Revert 1 was proven again against it.
- Then 5 rounds out of 5 passed: 16 of 16 each.

**The lifecycle bench** with `[judge]` enabled and Jev at 127.0.0.1:9: `theseus-sim bench lifecycle --runs 10 --check` printed LIFECYCLE OK, with every budgeted phase's p95 well under its budget (cold start 15.1 ms, start from the config copy 17.0 ms, clean shutdown 8.4 ms, SIGKILL then restart 16.4 ms, binary swap 18.0 ms). These are this VM's
numbers, for the shape only; the maintainer measures on the owner's machine.

**The turn bench** (`theseus-sim bench turn --check --runs 5 --burst 0`, on the same bench config, judge on): a plain
turn's frames are 5 at the p95, within the budget of 5. The frame list is unchanged, with no judge row in any of the
turn's frames.

### The live check (the maintainer's, with the real key)

On a scratch daemon with a fresh state directory. The config is the template from `theseusd example-config --plain`
with these changes:
- `[discord] enabled = false`, `[web] enabled = false`, `[index] enabled = false`;
- the model endpoints on the simulator's fake model (or GLM);
- `[judge] enabled = true`, and `[secrets] jev_api_key` pointing at the real key.

There is no standalone fake-model subcommand in `theseus-sim` today; the daemon tests use
`crates/theseusd/tests/common/model.rs`. If the maintainer's fake model has no CLI, the same check runs with GLM,
as the design's 23a live check does.

```bash
S=/tmp/jev-scratch; rm -rf $S && mkdir -p $S
# write $S/config.toml as above, then:
theseusd --config $S/config.toml --state-dir $S/state --socket $S/sock 2>$S/log & echo $! > $S/pid
theseus --socket $S/sock health | grep '^judge:'
#   judge: loop.v1: shadow · max live · breaker idle · 0 calls today (0 failed, 0 skipped) · $0.000000 of $1.00 today
theseus --socket $S/sock ask "Say the word done, and nothing else."
sleep 3   # the sink's window
theseus --socket $S/sock judge log
#   <time> loop.v1 (shadow) ses_… · work_state=complete 0.9x act · announced_unfinished=no … · $0.0000xx · ~350 ms
theseus --socket $S/sock health | grep '^judge:'
#   judge: loop.v1: shadow · max live · breaker closed · 1 call today (0 failed, 0 skipped) · $0.0000xx of $1.00 today
theseus --socket $S/sock --json ledger --kind judge.call   # (or: rpc ledger.tail '{"n":5,"kind":"judge.call"}')
#   the row: key jdg_…, its answers, usage, cost_micros, timing, and context.blob; the blob is $S/state/store/blobs/<digest>
```

The request digest with the judge on and off:
1. Note the assistant node's `request_digest`:
   `theseus --socket $S/sock rpc session.history '{"session_id":"<ses>"}' | jq '.. | .request_digest? // empty'`.
2. Shut down: `theseus --socket $S/sock shutdown`.
3. Set `[judge] enabled = false`, delete `$S/state`, start the daemon again, and ask the same prompt.
4. The `request_digest` should be the same. If the scratch config names the state dir in the prompt, keep the same
   path in both runs.

### What is left, or uncertain

- **The sink's window loses rows at a stop.** In-flight shadow calls are dropped at a stop, as the design says, and
  so are the rows still in the sink's 2 s window. Their spend is not lost: the next start books the block's rest.
  This matches §2.5 ("a crash loses at most 2 s of shadow rows"), but it holds for clean stops too.
- **The ladder isn't built.** `WIRED` gives `loop.v1` shadow; 26a replaces it with the `pack.mode` rows. A config
  line of `canary` or `live` parses, and stays a ceiling: it never raises a pack.
- **Not built in 23a, and filed by the design for later steps:**
  - `judge.resumed` (the day roll simply resets the pause);
  - the trace's `mark` span, notifications, and narrative lines (23b);
  - a `judge.list/get` method (23b);
  - the frame-budget test's judged variant for live calls (26b).
- **A design choice for the owner: which turns `loop.v1` judges.**
  - It judges every turn the baseline ends with `no_tool_calls`, a task's turns included.
  - It skips a turn that ends on the loop cap, a refusal, `max_tokens`, or a confirm.
  - It doesn't yet sample `continue` decisions (§2.4 says "a sample of `continue` decisions"); that needs a judgment
    mid-turn, which 23a avoided to keep the turn's path untouched.
- **The ask is approximate.** It is the newest operator message in the transcript (for a task, its first message,
  the brief). The design says "the exchange's first human message". For a run of several operator messages before
  a reply, these differ.
- **Docs to change at review** (not edited here):
  - the spec's Part III item for 23a;
  - `docs/status.md`;
  - `docs/design/README.md`'s list of reserved crates: theseus-judge is no longer reserved;
  - `crates/theseus-core/AGENTS.md`: a "Judgments" bullet (`judge/`, `fact/judge.rs`, `config/judge.rs`, the
    tests). I left AGENTS.md alone to stay inside the step, and the maintainer may prefer to word it.
  - theseus-judge has no `AGENTS.md`, though the prompt named one.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `a7fd996`'s tree and again on `cd18eec`'s: fmt, shape, clippy, bench build, test build, and the
reader rule all pass. The suite: 1,770 tests; 1,736 passed and 34 failed at `a7fd996`, and 1,737 passed and 33 failed at `cd18eec`. Each failure, and why:
- **The theseus-sandbox contract and bench tests, and theseusd's `sandbox` tests** (about 31): the VM runs as root,
  and L1 refuses every job of a root daemon (theseus-pv6i), as the prompt expects.
- **`theseus-core tests_output::the_cores_output_matches_its_golden`**:
  - The only difference is a wake's offset printed `+#:#` where the golden has `-#:#`. The VM is in UTC; the
    golden was written at a negative offset.
  - It passes with `TZ=America/Los_Angeles`.
  - The judge is off in the template, so 23a adds nothing to that output.
  - The golden's dependence on the machine's time zone is worth a follow-up: pin `TZ` in the test, or mask the sign.
- **`theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults`**:
  - In gate 2 it failed with "no series was put back" (37a's wakes-repeat check), with every invariant held.
  - Run alone, it passes in UTC and in America/Los_Angeles.
  - 23a touches nothing the kernel sim runs. It looks like a load-dependent or time-dependent outcome of the
    wakes-repeat sim. It is not on the flaky list.
  - In the gate at `cd18eec`, it passed.

The phases after the suite, run by hand (the gate's own commands): the protocol types (current, staged), the turn
bench (5 frames, within 5), `cargo deny --offline check` (advisories, bans, licences, and sources ok), the web app's
lint and build, the cockpit's lint, test, and build, and the web dist (unchanged). All pass.
