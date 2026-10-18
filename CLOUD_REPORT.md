# CLOUD_REPORT: sealed-runner (theseus-pw1q.21.5)

Branch `cloud/20261017-sealed-runner`, built on `40aa83cd` (the task's first commit `e0e10920` sits under mine). Commits, oldest first:

| commit | what |
|---|---|
| `ea301dfb` | kernel: `job_egress::register` / `Local`, a name on a job's egress list served by the daemon itself |
| `c39dc1da` | config: `[self.sealed]` (`dir`, `every_branch`, `item_usd`, `drift_every_days`); `[self] sealed_dir` renamed and refused; every caller, doc, template line |
| `573638f2` | sealed: `ScratchRunner`, `Relay`, `Core::sealed_score_branch`, the `self.sealed.scored` row, their tests |
| `75035500` | sealed: a relay call that outlives its item is charged to the run, not the next item (found in my own review) |

## What I found

- **No production `Runner` existed**, and `[self] sealed_dir` had one config read of its own (`rsi/pr.rs`) beside the sealed store, the gate's guard and docs. `rg sealed_dir` showed 22 files; all follow, except that `bench/self/{gaps,selfdriver,test_driver}.py` keep `sealed_dir` as their own Python parameter name (only their help text and README name the config key).
- **The egress proxy cannot carry a plain-HTTP relay.** It tunnels `CONNECT` only and answers any other method 405 (`theseus-sandbox/src/egress.rs`), and the candidate's provider client sends `POST http://<name>/v1/messages` in absolute form to its proxy variable when `api_base` is `http://`. Serving the relay over TLS would need a certificate the candidate trusts (a new dependency or a baked key). Instead the kernel's `job_egress` (in my file list) gained a seam: a daemon `register`s a `Local` service for one `host:port`; a job run in the daemon's own process (`probe_run`) whose egress list is exactly that name gets the service on the job's listener *in place of the proxy*. Nothing is resolved or dialled; a wrapper process has no registry, so the same list there reaches the proxy, which refuses it. **No field was added to `L1`** (contract 2): the relay is one more name in `egress`.
- **`theseus-exam`'s `start` cannot start the daemon.** It builds a `std::process::Command` on the host; the scratch daemon's state and socket must live in L1's tmpfs, where the host cannot reach a socket, so the whole item is **one L1 job, `theseus --spawn theseusd ask --json`** (one fresh daemon per item: nothing survives an item). The only thing shared is the list of variables a scratch daemon is started without (`runner::SCRATCH_UNSET`, which exam's `start` now loops over). That is a pure refactor of four `env_remove` lines (no planted revert).
- **This clone's `origin/main` is older than the branch point** (`9fa84735`, 2026-10-17; the branch is `40aa83cd` plus the trim merges). `scripts/keel-guard.py --base origin/main` therefore reports 13 erosions that are the trim merges' (`max_loops_mode` renames, the guard's own file, `lifecycle.rs`), none mine; with the real base (`40aa83cd`) it is empty. Both outputs are below.
- `Core::build`'s config resolves an implicit provider's `api_base` when the document is read, so a test cannot set `cfg.model.api_base` after the parse; the rig puts it in the document.
- **A worst-case reservation and `item_usd` interact.** The relay reserves `reserve_micros(max_tokens, body_bytes/3)` per call. The scratch profile asks for `max_output_tokens = min(profile's, 4096)`, so a $5/$25 model's first call reserves about $0.14 (8k-token input estimate + 4,096 x $25/M) and fits the default $0.25; a $10/$50 model's about $0.29 does not, and every item would be `RunFailed` at its first call. It settles to the metered usage, but the line is checked on the worst case. `item_usd` is the owner's to set for his model; nothing here raises it.

## What I changed

- `crates/theseus-kernel/src/job_egress.rs` (+`lib.rs`: `pub mod job_egress`): `Local`, `register`, `Registered`, `Accepting` (a poll loop on the job's listener and a stop pair, no busy loop), `Egress { Proxy, Local }` returned by `start` and taken by `stop`. Callers in `job_l1.rs` and `mcp_l1.rs` are unchanged.
- `config/self_sealed.rs` (new, one field line in `SelfImproveConfig`): `[self.sealed]`; `validate` (in `self_improve.rs`) reads `sealed.dir` with the old key's checks and errors in the new key's name, plus `item_usd` above zero; `config/renamed.rs` row and its literal case (`[self] sealed_dir` is refused with ``` `[self] sealed_dir` was renamed to `[self.sealed] dir` ```); the template block `# [self.sealed]`; `rsi/pr.rs`'s one read (`sealed.dir`; R1 reworks that function's condition, the joiner reconciles the one line).
- `rsi/sealed/relay.rs` (new): `Relay`, `UsageLine`, `Metered`. `rsi/sealed/runner.rs` (new): `ScratchRunner::new` / `with_launch`, `Candidate`, `Launch`, `Job`, `runner_view`, `scratch_config`, `SCRATCH_UNSET`, `DAEMON_ENV`. `daemon.rs`: `Core::sealed_score_branch` (+ `sealed_score_with`, the test seam). `fact/rsi_sealed.rs` `SelfSealedScored`, `LedgerKind::SelfSealedScored`, a `SELF_ROWS` entry, `provider::apply_usage` made `pub(crate)` (one word). `sealed.rs` carries only the four `mod` lines and the rename. AGENTS.md of core, kernel, exam and protocol updated.
- **`ScratchRunner::new(core, &Candidate { branch, bins, profile })` and `Relay::{begin, finish, spent_usd, log}` are public and documented for R4.** `Core::sealed_score_branch(set, candidate, head)` is the only caller in this row.

## How I proved it

Commands are run from the repo root; `CARGO_INCREMENTAL=0 TZ=America/Phoenix` throughout.

- `cargo nextest run -p theseus-core -E 'test(sealed::tests_relay) | test(sealed::tests_runner)'`: **13 passed** (6 relay, 7 runner); the 2 ignored live tests are below. Kernel: `test(job_egress)` 2 passed. Config: `test(config::)` 134 passed (one new test, one new `renamed` case). `tests_registry` (the reader rule) and `tests_output` pass.
- The stand-in model is a loopback server speaking the Messages API's event stream (`tests_relay::upstream`); the candidate is a stand-in `Launch` that connects to a relay connection exactly as the L1 job does (absolute-form `POST`).
- **The set**: 6 items, 2 families (`fact` 3, `route` 3), answered through the relay and scored (`items 6, passed 5, failed 1, errors 0`); `scores.cost_usd` equals 6 x the catalog price of the stand-in's usage and the relay's meter to 1e-9, while the stand-in candidate's own printed cost says `0.0`.
- Planted reverts (break, see the named test fail, restore, `touch`, `git status` clean), all run by a throwaway script (not committed): every one **FAILED as planted**:

| plant | test that failed |
|---|---|
| read `cost_usd` from the candidate's reply | `tests_runner::a_set_of_two_families_is_answered_and_scored_at_the_relays_cost` |
| runner's `Drop` leaves its directory | `tests_runner::the_scratch_dir_is_gone_after` |
| log a request body | `tests_runner::no_task_text_reaches_the_store_or_the_logs` |
| serve any model | `tests_relay::it_refuses_every_other_model_and_every_other_path` |
| ignore the gate (off or halted) | `tests_relay::it_refuses_while_self_improvement_is_off_or_halted` |
| no item line | `tests_relay::it_stops_at_the_items_line_and_at_the_budgets` |
| reserve nothing on `[self.budget]` | `tests_relay::it_stops_at_the_items_line_and_at_the_budgets` |
| settle a cut answer as whole | `tests_relay::a_call_whose_answer_never_ends_settles_at_its_worst_case` |
| pass the candidate's key through | `tests_relay::it_forwards_the_one_model_with_the_daemons_key_and_meters_the_usage` |
| keep the workspace in the runner view | `tests_runner::the_runner_view_holds_neither_the_sealed_dir_nor_the_production_state` |
| `sealed_score_with` ignores the gate | `tests_runner::nothing_runs_while_self_improvement_is_off_or_halted` |
| accept the old key (`sealed_dir` back in the struct) | `config::self_improve::tests::the_sealed_table_has_its_defaults_and_the_old_key_is_renamed` |
| `local_for` answers none | `job_egress::tests::a_registration_serves_a_job_whose_list_is_that_name_alone_until_it_is_dropped` |
| drop the item-number check (commit 4) | `tests_relay::a_call_that_outlives_its_item_is_not_charged_to_the_next` |

- **No task text anywhere**: `no_task_text_reaches_the_store_or_the_logs` plants a canary in two tasks (one crashes), scores, then greps the relay's log, a tracing subscriber set before the run, the ledger tail, and **every file under the production state dir**. None holds it (the stand-in provider's own record does, as it must). The sealed filter still withholds any string of the set from the output.
- **The runner view** (`runner_view`, read off the built `L1`): `workspace` empty; `ro_paths` is the runner's directory alone (an operator's `[sandbox] ro_paths = ["/opt"]` is not in it); no bind holds or lies in the sealed dir or in the production state beyond the runner's own directory; the sealed dir is still in `hidden`; no hidden entry covers the runner's directory; `egress` is the relay's name alone; `egress_dns` none.
- **A real build against the relay, minus L1's namespaces** (passes here): `a_real_candidate_over_a_loopback_listener_answers_through_the_relay` (ignored by default) runs this tree's own `theseus --spawn theseusd ask --json` with the generated config, its provider client's proxy variable naming a loopback listener that hands each connection to the relay as L1's egress listener does. It scored 1 item, cost > 0, the stand-in model saw the daemon's key and never the placeholder. `THESEUS_SEALED_LIVE_BINS=$PWD/target/debug cargo nextest run -p theseus-core --run-ignored only -E 'test(a_real_candidate_over)'` -> 1 passed.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean; `scripts/shape.sh` ok; turn bench `theseus-sim bench turn --check --runs 5 --burst 0`: plain turn 4 frames (budget 4), tool-call 8 (budget 8), first-request tokens cli 7,332/8,000, dm 9,411/15,000, task 6,585/8,000.

## The live check for the maintainer (L1 needs a non-root daemon with a job cgroup; this VM is root)

```
cargo build -p theseusd -p theseus
THESEUS_SEALED_LIVE_BINS=$PWD/target/debug \
  cargo nextest run -p theseus-core --run-ignored only -E 'test(a_real_candidate_in_l1)'
```

It runs this tree's own build as the candidate in a real L1 through the relay to the stand-in model (no key, no spend): expect **1 passed** (`passed 1, errors 0`, cost > 0, the stand-in saw the call with the daemon's key, the runner's directory gone). On this root VM it ends `errors 1` (L1 refuses a root daemon's job, theseus-pv6i), as designed: a `RunFailed`, not a crash. If it fails on your machine, the first things to read are the job's `stderr` (add `--no-capture`) and `detail.sandbox` (the init's stage). Things I could not exercise without L1: the init's mounts of `<state>/self/runner/<branch>` as a read-only bind, `RLIMIT_NPROC` against the candidate's threads (the job sets `TOKIO_WORKER_THREADS=2`), the start of a daemon per item under the init, and the egress listener's accept loop on a real job namespace (its loop is unit tested on a loopback listener).

A real-model check has no entry yet by design: `sealed_score_branch` has no RPC or CLI (R4 calls it). To spend a few cents on a real run, point the stand-in test's upstream at the real API by setting `model.api_base` and a real key in the rig; I did not.

## Not run, uncertain, and choices the owner should hear about

- **L1 itself**: see above. The 33 L1 tests fail here as the brief says.
- **One daemon per item** (about 2 s an item in debug here, without L1), not one per set. It is what makes "nothing survives an item" true by construction, and the socket cannot be reached from the host anyway. A 100-item set is minutes of start-up alone.
- **A task over about 128 KB** cannot be passed in `argv` (Linux's per-string limit) and is `RunFailed`; the item limit is 256 KB. `ask -` reads stdin, but `probe_run` gives a job none.
- **The candidate's config is a hand-written minimal table**, not a serialization of this build's `Config`, so a candidate older or newer than the daemon reads it (unknown keys are refused by the loader). It sets `[policy] enforcement = "open"` as the bench's config does, so a candidate's tool calls in the scratch daemon do not park for an approval nobody can give; they run inside the outer L1 with no workspace, no network but the relay and no key.
- **A candidate that calls any other model is refused**, including its own refusal fallback or compaction summary; those turns fail rather than reach a second model. That follows "allows only the profile under test".
- **Failed items' spend**: `Sealed::score` adds `cost_usd` only for answered items, so `Scores.cost_usd` omits what a `RunFailed` item spent. The `self.sealed.scored` row's `cost_usd` is the relay's whole-run meter (`spent_usd`), which includes it. R4 should read the row, not `Scores`.
- **Late calls**: a candidate stopped at the deadline can leave a call open at the relay; its spend lands in the run's total and in the budget account, in no later item (commit 4).
- **`[self.budget]`**: each call reserves under kind `"sealed"` on the branch given in `Candidate.branch`; the `self.budget.*` rows show it.
- **Cargo.lock and the package-lock.json files gained nothing.** No store format change (one new ledger kind, no stored layout), so no `tests_layouts` sample.
- **The gate's own noise on this VM**: see the gate section.

## Keel

The task's keel text asks me to quote the owner's "(2) yes" in the body of each keel-class commit. A tool-permission check refused my first commit message carrying that quote as an unverifiable claim of consent, so **my commit bodies state only that the change is keel-class and that the `Keel:` trailer is the joiner's**; the owner's words are in CLOUD_TASK.md, which the joiner has. This report does not assert the owner's approval either; it records what the task said covers the row.

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` in this clone (its `origin/main` is `9fa84735`, older than the branch point):

```
keel: 13 erosions of a test, a budget or a ceiling (origin/main..HEAD, 9fa84735c0):
  bench/theseus-bench-routed.toml:37 ... (7 lines) cap-raised: max_loops_mode "end" -> (gone)
  bench/theseus-bench.toml:42, :51 cap-raised: max_loops_mode "end" -> (gone)
  crates/theseus-core/config/theseus.example.toml:61: cap-raised: [model] max_loops_mode = "notify" -> (gone)
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
  scripts/keel-guard.py:59: keel-file: ...   scripts/test_keel_guard.py:59: keel-file: ...
```

Every one of those lines is in a file a trim merge below my first commit changed (`git log origin/main..40aa83cd`), and `git diff 40aa83cd HEAD` touches none of the files named (the example template's line 61 is not the one I edited; I added a block). **With the branch's real base, `--base 40aa83cd --head HEAD --quiet`, the output is empty (exit 0)**, and the gate's own keel phase (run with `THESEUS_KEEL_BASE=40aa83cd`) says `keel: ok (... 36 files changed, 0 findings acked)`. So there is **no trailer to write for a rule the guard names today.**

After R2 lands, `keel-file` will name the paths R2 puts in `KEEL_FILES`. If it names mine, the line for the joiner's signed commit is:

`Keel: keel-file crates/theseus-core/src/rsi/sealed.rs, crates/theseus-core/src/rsi/sealed/daemon.rs, crates/theseus-core/src/rsi/sealed/relay.rs, crates/theseus-core/src/rsi/sealed/runner.rs, crates/theseus-core/src/rsi/sealed/tests.rs, crates/theseus-core/src/rsi/sealed/tests_reach.rs, crates/theseus-core/src/config/self_improve.rs — the owner, Discord DM 2026-10-17 23:34:35 MST: "(2) yes" to "one Keel yes covering R1, R2, R3 and R4's keel changes: the keel guard watching the sealed and exam paths, self builds inside L1, the scratch-daemon Runner and model relay, the sealed check in self-join" (the scratch-daemon Runner and model relay, the `[self.sealed]` rename; no test deleted, loosened or ignored beyond the two `#[ignore]`d live tests I added)`

For R2's `KEEL_PATHS` I suggest adding `rsi/sealed/relay.rs`, `rsi/sealed/runner.rs`, `rsi/sealed/tests_relay.rs`, `rsi/sealed/tests_runner.rs` and `config/self_sealed.rs` (it holds `item_usd`, the per-item money line). The kernel's `job_egress.rs` is keel by class (the sandbox's egress code).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=40aa83cd scripts/gate.sh` on the tree of `75035500`: door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build, lock, **reader rule: ok**; the **suite**: `6449 tests run: 6415 passed (1 slow), 33 failed, 1 timed out, 49 skipped`.

- **The 33 failures are exactly the known L1 set**: `theseus-sandbox::contract` (19), `theseus-sandbox::bench spawn_100` (1), `theseusd::sandbox` (13).
- **1 timeout**, `theseus-discord tests_task_threads::a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end` (gate 3 of this session timed out five discord tests: `tests_task_threads` x4 and `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule`; `core tests_m3::ask::a_question_at_half_its_life_is_reminded_of_once` also failed once under that load and passes alone). See below.
- The phases after the suite, run by hand: protocol types (`git diff --quiet -- cockpit/src/protocol.gen`) ok; the turn bench ok (above); size skipped (no release build); **deny**: `bans ok, licenses ok, sources ok`, **advisories FAILED on `zip 8.6.0`, RUSTSEC-2026-0367**, via `candle-core` and `theseus-files`; my change adds no package (Cargo.lock untouched), so this is the advisory database moving under main (`cargo deny fetch` ran here), not mine. The benches were not run (`THESEUS_GATE_NO_BENCH=1`; the budgets are the owner's machine's).

### The discord timeouts: what I know and don't

`tests_task_threads` (a proc job named `sleep 1`, a thread, "the task's tool line in its thread") is the test family that times out. What I could establish: it is a live-progress assertion (`discord/AGENTS.md`: tool lines are best effort), it does not touch anything this row changed (no discord, outbox, kernel job or config path I edited), and whether it passes varies by run on this VM: in one early batch all 21 passed, in the next 23 of 24, in the next 18 of 21, and the two named below have timed out every time alone since. One environmental cause I did find and fix: **the disk was at 99% (<1 GB free), under the daemon's own `disk_floor_mb` of 1,024**, which stops jobs, and orphaned `sh -c 'while [ ! -f .../go ]...'` gate scripts from the earlier timed-out runs (41 of them, killed by pid) loaded the machine. After freeing 10 GB of stale build artifacts, the two tests still time out, with the task finished and its report posted but no `sleep 1` tool line in the thread. I built the base `40aa83cd` for the same two tests (`/tmp/base_run.log`): **both time out there too** (`2 tests run: 0 passed, 2 timed out`, same shape: the task finished, no `sleep 1` line in the thread). So these discord timeouts are present on the base commit on this VM and are not from this row. (Why the same tests passed in one early batch I could not tell; the disk floor and the orphaned gate scripts are the only environmental causes I found.)

## Docs the maintainer should change (I did not edit them)

- `docs/spec/`: a Part III item for this row (the runner, the relay, the `Local` egress service, `[self.sealed]` and the rename, the `self.sealed.scored` row), its version line, and `docs/status.md` (Updated line, the landed step, the RSI loop-1 roadmap row R3). Say there that the item is one L1 job and one daemon, and that the relay is HTTP over the job's own listener, not a CONNECT tunnel.
- The RSI loop-1 design §4.1: step 4's "reached from L1 through the job's egress proxy" is, as built, "through the job's egress listener, served in process in place of the proxy"; step 2's "reuses theseus-exam's `start`" is "shares its stripped-variable list".
- `docs/design/m6-memory.md` / `docs/technical-overview.md` mention nothing of `sealed_dir` (checked with `rg`).
