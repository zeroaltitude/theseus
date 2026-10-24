# CLOUD_REPORT: p2f-fanout (theseus-1kr2g)

Branch `cloud/20261024-p2f-fanout`, base `23328545` (release 1.0.1). Commits on top of the task commit:

- `2c1c971` toolrun: prove a fan-out's operands and its jobs keep one response's programs together (theseus-1kr2g)
- `2158047` toolrun: a cd both scripts begin with is no file they share, so their programs run at once (theseus-1kr2g)

## 1. For the maintainer first

**Verdict.** At 1.0.1 Theseus still runs one response's slow program calls at once: argv calls, `bash -c` lines, an
exit-code echo, a `tee` log, a `timeout` prefix, a retry loop, `part-N` names, and real `proc.run` jobs that pass
`proc_sync`. A wall-clock-free test shows all five in flight together for each of these shapes, and it passes at the
base. **The measured drop is the model's call shape plus n=2.** No 1.0.1 trial put its parts in one response. Each
wrote one serial shell line, a `for` loop, or `steps`, and Theseus ran each of those as asked (section 4).

The search turned up **one real false share**, and it is now fixed. Five `bash -c "cd /srv/app && load-part N"` calls
ran one after another, each saying `[started after call N, which names \`app\` too]`. The grouping's file-name keys
read the directory a script `cd`s into as a file that both calls name. No measured trial used that shape, so it does
not explain this drop. But models write `cd /app && …` often, and on that shape any fan-out ran serially. The fix
keeps a script's `cd`/`pushd` target as a `dir:` key. Two calls that `cd` into one directory now share nothing by that
alone, just as two calls with the same `cwd` already shared nothing. A call that names that directory as a file or
argument still waits.

- **Store format:** unchanged at 49. No stored record was touched.
- **Ceilings:** `toolrun.rs` stays at 2,500, untouched. `tests_m3.rs` stays at 7,981, untouched (no new `mod` line:
  the tests went into `order.rs`, `runs.rs`, `parallel.rs` and `operands.rs`). `parallel.rs` is at 465 lines and
  `operands.rs` at 294, both far under 2,500. No ceiling was raised.
- **OWNER CALL:** none. The DM flow golden is byte-identical (section 6).
- **No protocol, config, prompt, or tool-description change.** `cockpit/src/protocol.gen/` is untouched. No new
  dependency.

## 2. Each suspect

**Suspect 1: the operand rule and file-name keys (`01bb34bd`). IN, for one shape, fixed in `2158047`.**
The grouping in `parallel.rs` holds a program back for two reasons: a shared *key* (`name:` from `file_names`,
`term:`, `mcp:`) or a shared *operand* (`operands::shared`). I ran each listed shape through both:

| shape (part 2 vs part 3) | operand at base | key at base | after the fix |
|---|---|---|---|
| `load-part 2` vs `load-part 3` (argv) | none | none | together |
| `bash -c "load-part 2"` (`command`) | none | none | together |
| `cd /srv/app && load-part 2` vs `… 3` | none (both begin with `/srv/app`: shape) | **`name:app`** | together |
| `load-part 2; echo "exit $?"` | none | none | together |
| `load-part 2 2>&1 \| tee part-2.log` | none | none | together |
| `load-part part-2` vs `part-3` | none | none | together |
| `timeout 600 load-part 2` (line and argv) | none | none | together |
| `until load-part 2; do sleep 1; done` | none | none | together |
| `which load-part; load-part 2` | none | none | together |
| mixed: `cd /srv/app && load-part 2` vs `load-part 3 --out /srv/app` | none | `name:app` | **still waits** (a dir key meets a name key) |
| mixed: `cd /srv/app && load-part 2` vs `load-part /srv/app/3` | none | none | together |

So the operand rule itself was never the cause: `operands.rs` is unchanged. The false share was in `parallel.rs`'s
`file_names`, which reads any word with a `/` as a file. The fix is in `parallel.rs`. `file_names` now skips a
script's `cd`/`pushd` commands. A new `dir_names` names their targets, and `Touch::of` keys them as `dir:`. In
`shared`, a new `meets` treats `dir:x` vs `dir:x` as nothing shared, while `dir:x` vs `name:x` (either way round) is
shared. Every other key matches by equality, as before. These true shares still wait, and the table asserts each:
`cd /srv/app && …` vs `--out /srv/app`; `> /srv/app/p2.out` then `cat /srv/app/p2.out`; `mkdir -p /srv/wt` then
`cd /srv/wt && make`; `git worktree add ../wt-a` then `cd ../wt-a && make`.

- Tests: `toolrun::parallel::tests::a_fan_out_of_parts_shares_nothing_and_a_true_share_still_waits` (the whole rule,
  keys and operands, through `Touch::of` with the real `proc.run` tool) and
  `toolrun::operands::tests::a_fan_out_of_parts_shares_no_operand` (the operand side alone).
- Red at the base: the `parallel.rs` table fails at its `cd` pair, `left: Some("app") right: None`
  (`parallel.rs:478`). The fan-out test fails at its `cd` shape, `most_at_once` 1 against 5 (`order.rs:301`).
- Planted revert 1: `file_names`'s `.filter(|c| !changes_dir(c))` changed to `.filter(|_| true)`, so `cd` targets are
  file names again. Both tests above go red at the same lines with the same values. Restored and `touch`ed;
  `git status` showed only my intended changes.
- Planted revert 2: `SHAPE_WORDS = 0` in `operands.rs`. The operand table fails at
  `cd /srv/app && load-part 2 / cd /srv/app && load-part 3`, `left: Some("/srv/app")` (`operands.rs:276`). The
  operand table passes at the base, which shows the operand rule was not the cause.

**Suspect 2: `parallel_runs` default 16 (`c816b01b`). OUT.** `config/limits.rs` gives `default_parallel_runs()` = 16,
`default_parallel_reads()` = 32 and `default_parallel_writes()` = 16. `run_group` in `toolrun/calls.rs` picks the cap
by the group's class, `Run` → `parallel_runs`, through one semaphore of that size. Five calls sit well under 16. The
existing `sixteen_programs_run_in_one_wave_and_a_seventeenth_waits`, `the_cap_of_programs_is_the_config` and
`config/tests.rs` all hold these values, and all pass.

**Suspect 3: the job path past `proc_sync`. OUT.** `run_group` starts every call of a group as futures under
`join_all`. A call that passes `proc_sync` returns its placeholder from `wait_in_turn` (`toolrun/job.rs`) without
blocking the others. The admission lend/retake is per execution, and only the first wait to pass `LEND_AFTER` lends.
A later retake awaits inside that call's own future, so it delays no other call's start. The new test
`tests_m3::parallel::runs::five_jobs_of_one_response_meet_at_once_past_proc_sync` uses real `proc.run`, real jobs and
`proc_sync_secs = 8`. Each of five `command` calls touches its own marker and waits for all five. It gives up after
40 polls of 0.1 s (about 4 s, before `proc_sync`) and writes `alone`. Otherwise it writes `met` and sleeps 10 s, so
it becomes a job. The test asserts all five results are `Background` and say "Still running as job j…" and
`[ran at once with 4 other calls of the same response]`, and all five jobs wrote `met`. It passes at the base, so it
is in the first commit. Planted revert: in `Open::admit`, `&& t.class != ToolClass::Run` puts every program in a group
of its own. Call 1 waits alone and exits 1, failing with `left: Error right: Background` (`runs.rs:341`).

**Suspect 4: deferred tools, tool search, the shorter tools note. OUT for dispatch.** These change what the model
reads, not how calls run:
- `git grep disable_parallel_tool_use 23328545 -- crates` finds nothing. I could not check `026dce3f`: this clone is
  shallow (739 commits) and does not hold it. The host's check found none there either.
- `proc_run`'s description is static, from `theseus_tools::proc::Run::description` (`proc.rs:147`): "The proc_run
  calls of one response run at once, each its own job, except that one naming a file or other thing (a service, a
  container) an earlier one names waits for it; programs that must run in order go in one call as `steps`."
  `toolrun/catalogs.rs`'s `built_for` takes definitions from the registry and rewrites only `job_wait`'s text
  (`say_wait_cap`). Nothing varies it by place.
- The `commands` family, which holds `proc_run`, is in `[tools.defer] always_loaded` (`config/defer.rs`:
  `["files", "commands"]`), so every place class carries it on its first request. A trivial detour carries
  `tools_find` only (`c2d757e1`), but its own commit says a model with tool search is unchanged, and the bench's task
  is not trivial. The existing `four_sleeps_of_one_response_end_in_about_one_sleep` asserts the wording on the rig's
  first request. I did not add a per-place test: with the family always loaded and the description static, it would
  repeat that one.

**Suspect 5: `steps`. OUT (the model's choice).** `steps` runs in order and stops at the first failure, as its schema
says ("The run stops at the first step that exits non-zero"). Trial A put independent parts in `steps`, and the first
failure skipped the other four. I did not change `steps.rs`.

## 3. The fan-out tests

- `tests_m3::parallel::order::a_fan_out_of_five_slow_programs_runs_at_once` (`order.rs`, paused clock). It uses the
  existing rig (`Prog`, `run`, `most_at_once`, `results`). `Prog` now reads an optional `ms` hold from its input,
  defaulting to the old 100 ms, and a small `slow(id, argv, secs)` builds the call. It runs eight shapes, five calls
  each, each held 20 s: argv `load-part N`; argv `load-part part-N`; `bash -c "load-part N"`;
  `bash -c "cd /srv/app && load-part N"`; `… ; echo "exit $?"`; `… 2>&1 | tee part-N.log`; `timeout 600 load-part N`;
  and `cd /srv/app && fetch-shard N 2>&1 | tee /tmp/shard-N.log; echo "exit=$?"`. It asserts `most_at_once() == 5`,
  that every result contains `[ran at once with 4 other calls of the same response]`, and that none says
  "started after". It makes no assertion on elapsed time.
  - At the base it is red on the `cd` shape, `most_at_once` 1, `order.rs:301`. The five results said
    `[started after call 1, which names \`app\` too]` … `call 4`. With the two `cd` shapes left out, it passes at the
    base: I checked by a temporary edit, then restored the file.
  - Planted revert (item 4): every program in its own group (`Open::admit`, above). It fails on the first shape:
    `load-part 1: ["ran load-part 1", …]`, `left: 1 right: 5`.
  - Planted revert (the fix): `cd` targets counted as file names again. It fails on the `cd` shape as at the base.
- `tests_m3::parallel::runs::five_jobs_of_one_response_meet_at_once_past_proc_sync` (`runs.rs`, real jobs, a file
  barrier) is the `proc_sync` case; see suspect 3. Its barrier's bound fails the test instead of hanging, and no
  assertion reads a clock. The design keeps the barrier's bound (about 4 s) under `proc_sync` (8 s). Otherwise a
  serial dispatch could hand off at `proc_sync` and still meet the barrier late.

## 4. The measured drop explained

My tests prove that when a response holds N independent program calls, they run at once at 1.0.1, including past
`proc_sync`. Only the `cd /srv/app && …` shape was serialized, and that is now fixed. Set the trials against that:

- **1.0.1 trial A (183 s):** every response had one call. The fan-out was `steps` (in order by design; its first
  failure skipped the rest), then one serial shell line `ingest 2; echo ---; ingest 3; …` (one program, one job, the
  parts back to back), then `ingest 6` alone. Theseus had nothing to run at once.
- **1.0.1 trial B (191 s):** one call per response: a serial `for` loop in one `command`, then an `until` loop
  retrying parts 2 and 3. Again one job.
- **Oct 12 trial D (176 s):** the same shape as A and B, one serial `for … until` loop, and the same wall.
- **Oct 12 trial C (110 s):** the only trial that put five calls in one response, `{command: "ingest N"}` × 5. All
  five started at 44.0-44.1 s, as the fan-out test's `bash -c "load-part N"` shape asserts today.

The Oct 12 mean of 143 s is (110 + 176) / 2: one trial batched and one did not. Neither 1.0.1 trial batched, and both
match trial D. With two trials a side, P(0 of 2 batch | a 50 % rate) = 25 %, so this cannot tell a change in the
model's habit from luck. The other reading, that dispatch changed, is ruled out by the tests above and by the same
wave's live evidence: three `bench-batch` calls and three `build-config` calls each started at the same instant, and
15 results said "ran at once with 2 other calls". The calls-per-response drop (1.5 → 1.0) and the multi-call share
(0.2 → 0) are the model's choices. The first request also fell from 14,859 to 7,256 tokens (deferral and the shorter
tools note), which may have shifted that habit, but n=2 cannot show it.

## 5. Proposals (OUT: not done, for the owner's call)

**`proc_run`'s description.** Today the description says how a batch runs but not to make one. When a model has N
independent parts, it reaches for a loop or `steps`, and both run serially. A proposal (one sentence added after
"…go in one call as `steps`."):

- Before: "The proc_run calls of one response run at once, each its own job, except that one naming a file or other
  thing (a service, a container) an earlier one names waits for it; programs that must run in order go in one call as
  `steps`."
- After: "The proc_run calls of one response run at once, each its own job, except that one naming a file or other
  thing (a service, a container) an earlier one names waits for it; programs that must run in order go in one call as
  `steps`. For independent work (parts, shards, files), make one proc_run call each in the same response: a shell
  loop or `steps` runs them one after another, and `steps` stops at the first failure."

The evidence: three of the four trials (A, B, D) wrote a serial loop or `steps` for independent parts and took
176-191 s. The one trial that issued one call per part took 110 s. The cost is about 40 tokens on every first
request. Its effect needs live runs.

**The tools note.** `5d63b5d2` dropped "proc_run calls that take longer than N seconds continue in the background".
The description still says it ("A run past its time limit goes on as a background job…"), so I propose no change
there.

**Bench rerun (follow-up for the maintainer):** the async bench's fan-out family, k = 4 or more, on the Theseus
plain arm (Sonnet 5.5, `proc_sync` 60 s), at the tip of `integ/p2-follow` once this row is merged. Then, if the owner
wants it, the same with the description above, to measure the batching rate (calls per response, multi-call share)
and not only the wall.

## 6. Gate, keel, DM golden

**The gate**, `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at `2158047`:
- door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule: all ok.
- **suite: FAILED only on known cases.** 8,044 tests ran: 8,008 passed, 36 failed, 53 skipped.
  - 34 failures are the known L1 tests that fail on a root VM (theseus-pv6i): theseus-sandbox's `contract` tests, its
    bench's `spawn_100` / `l1` rows, and theseusd's `sandbox` tests.
  - 2 others passed when rerun alone with `cargo nextest run -p theseus-core -p theseus -E …` (2 passed). Neither is on
    the flaky list, and neither touches `toolrun`:
    - `theseus::markdown rendering_costs_little_per_hundred_lines` is a timing test, slowed by a loaded 4-core VM.
    - `theseus-core tests_route::a_switched_turns_compilation_names_its_recall_drops` is routing, not dispatch.
- After the suite I ran the `machine_checks` phases myself:
  - protocol types: ok (`cockpit/src/protocol.gen` untouched).
  - lifecycle and jobs: skipped under `THESEUS_GATE_NO_BENCH`.
  - turn: `theseus-sim bench turn --runs 5 --burst 0 --check` exits 0; a plain turn is 4 frames.
  - size: "skipped: no release build".
- **deny:** bans, licenses and sources ok. **Advisories FAILED** on RUSTSEC-2026-0387 (`crypto-bigint` 0.5.5, via
  `p384`), from the advisory database fetched in this session. This branch changes no `Cargo.lock` or `Cargo.toml`, so
  the finding is on the base, not this row's. The maintainer's own database decides; if it flags the same, it needs its
  own issue.
- The first gate run died in its test build on this session's disk allowance (`ld … Bus error`, then "No space left
  on device"): the whole-workspace test build is about 21 GB of test binaries. I deleted the stale test executables
  and `.rcgu.o` files under `target/debug/deps` (rebuildable) and ran the gate again; the second run is the one above.
- Under load: I did not run the new tests under the nice-19 recipe. The two paused-clock tests read no clock, and the
  barrier test's only time bound is its give-up (about 4 s, under `proc_sync`'s 8 s). They passed inside the full
  suite on a loaded 4-core VM.

- Targeted, before each commit (`CARGO_INCREMENTAL=0`, see the note below):
  - `cargo test -p theseus-core --lib -- parallel:: operands:: steps jobs` → 90 passed, 0 failed, before the job test
    was added. After it: `parallel:: operands::` plus the three new fan-out tests, all passed.
  - `cargo clippy --workspace --all-targets -- -D warnings` → clean. The first try flagged `too_many_lines` (102/100)
    on the new table test, which I rewrote to read its calls from strings.
  - `cargo fmt --check` → clean.
- `python3 scripts/keel-guard.py --base 23328545 --head HEAD --quiet` → empty output, exit 0.
- `git diff --exit-code 23328545 -- crates/theseus-discord/src/golden/dm_flow.txt` → empty, exit 0.
- **Disk note:** the first `cargo build --workspace --all-targets` died with `ld terminated with signal 7 [Bus
  error]`: this session's disk allowance was spent, 10 GB of it in `target/debug/incremental`. I deleted that
  directory (rebuildable, inside the repository) and built with `CARGO_INCREMENTAL=0` from then on.

## 7. Join notes and follow-ups

- **Files touched:** `crates/theseus-core/src/toolrun/parallel.rs` (the fix and its table test),
  `toolrun/operands.rs` (tests only), `tests_m3/parallel/order.rs` (the fan-out test, and `Prog`'s optional `ms`
  hold), and `tests_m3/parallel/runs.rs` (the job barrier test). No sibling row of `integ/p2-follow` owns these. I
  touched no turn, provider or request-builder file.
- **AGENTS.md (suggested for the docs commit):** in `crates/theseus-core/AGENTS.md`, "Tool calls", after "sharing an
  operand (… `toolrun/operands.rs`, the rule in parallel.rs's header, theseus-0hufe) in order", add: "; a script's
  `cd`/`pushd` target is where it runs (a `dir:` key, as a call's `cwd`), so two calls that change into one directory
  run together, and one naming that directory as a file waits (theseus-1kr2g)". I left it to the maintainer so the
  gated commits stay as gated.
- **Part III / status:** a short item for theseus-1kr2g: the drop was the model's call shape (n=2); one false share
  (`cd` targets as file names) was fixed; the description proposal and the k ≥ 4 rerun are open.
- **Uncertain:** a `cd` inside a subshell, `$(cd x && pwd)`, also counts as a directory change, not a file. That
  errs, slightly, toward running together; the same `cd` target named as a file elsewhere still waits. Brace groups
  (`{ cd x; …; }`) are not split at `{`, so their `cd` still counts as a file name, which errs toward waiting, as
  before.
