# The Ship of Theseus, chapter 30: Part III, A4's Items 234 to 243 ([index](README.md))
### Item 234. Bench fair: the three Harbor arms are comparable: medium effort on every arm, Claude Code pinned at 2.1.290 with every arm's version recorded, a timed-out Claude Code or Pi stopped before the sampler and the verifier, Pi's provider failures counted as errors, its turn cap counted without retried failures, and Pi run offline (theseus-n6p5, theseus-7gir.23, theseus-sgpx, theseus-bpeg, theseus-p6kd and theseus-a5we; local reviewer R35's findings on bench-pi, Item 218; the eleventh cloud batch's bench-fair session, fired 2026-10-06 18:10 from 57f265f2, Sonnet 5.5, its report at 18:48; ea6aa6aa, 78a620d7 and 08a76b79; reviewed 21:11 to 21:30 by local reviewer R40, the second of its two, and accepted with a one-line join fix; joined 22:50 at 5973419f, a signed merge onto 47833aa9, the last of the amhb stack's three merges under one lock and one gate, by the batch-11 amhb joiner; not installed yet: on install #10's list)

**Why.** Reviewing bench-pi, local reviewer R35 found the arms unequal in ways that bend a comparison:
- **theseus-n6p5:** on Sonnet 5.5 each arm ran at its harness's default effort: Theseus with no
  `output_config.effort` (the API's default, high), Claude Code 2.1.290 and Pi 1.0.4 at medium. The owner was told at
  14:32 that effort would go medium on every arm unless he objected, and he had not (the chain log, 18:03).
- **theseus-7gir.23:** Harbor installs Claude Code's latest unless asked, so each run's Claude Code was whatever npm
  served that day (b5 ran 2.1.288; R35 captured 2.1.290); Pi's 1.0.4 was pinned.
- **theseus-sgpx:** at a task's agent timeout Harbor 0.23 ends only its local `docker compose exec` client, so a
  timed-out Pi or Claude Code kept running in its container while the verifier ran. The Theseus arm already stopped
  its agent.
- **theseus-bpeg:** Pi's print mode exits 0 when the provider fails, so such a trial was in neither "Trials with an
  error" nor `trials.csv`.
- **theseus-p6kd:** three behaviours of Pi's arm no test held, its answers counting retried failures toward
  `max_turns`, and `pi --version` not kept.
- **theseus-a5we:** Pi overlays newer catalog data from its website at start, so a run could change under it.

**What landed** (bench/ only, plus one assertion in theseusd's `tests/bench_profile.rs` and the bench profile's
`effort` line in `bench/theseus-bench.toml`; the merge 19 files, +874 −47, without the cloud files; no store format,
protocol, config key or package change (23 stays 23)).
- **Effort medium on every arm** (ea6aa6aa, 08a76b79; n6p5). `effort = "medium"` in `[profiles.bench]`, held by
  `a_bench_call_asks_for_the_models_whole_output` (`output_config.effort == "medium"`); `MeasuredClaudeCode`'s
  `reasoning_effort` and `MeasuredPi`'s `thinking` default to `measure.EFFORT`, an `--ak` wins (an ablation), and a
  host's `CLAUDE_CODE_EFFORT_LEVEL` no longer picks it.
- **The pin and the versions** (08a76b79; 7gir.23). `claude_code_agent.PINNED_VERSION = "2.1.290"` on both of Harbor's
  install branches (bootstrap.sh and npm); `measure.record_version` writes `get_version_command`'s output to
  `agent/version.txt` at install for all three arms; every record gains `effort`, `version` and `version_asked`, and
  Pi's `effort_ran` (its thinking-level changes and answers).
- **The stop at a timeout** (08a76b79; sgpx). New `bench/harbor/measure.py` (standard library only):
  `stop_agent_script`, plain sh over `/proc`, finds the tree of the processes named `claude` or `pi` before any
  signal, sends SIGTERM, then SIGKILL after 3 s, and skips zombies; run as root on `CancelledError` before the
  sampler's stop, then re-raised; the async arms get it through `super().run`.
- **Pi's ends and turns** (78a620d7, 08a76b79; bpeg, p6kd). `load_trial` names `PiProviderError` or `PiAbortedError`
  from the record's `end.stop_reason` when Harbor recorded no exception (Harbor's own wins), both in `draft.ENDINGS`;
  `pi_end` gains `turns` (answers that are not `error`) and `limits.turns`, and `over_turns` counts turns, not answers;
  tests for a trial past `max_turns` alone and a record from `pi.txt` alone.
- **Pi offline** (08a76b79; a5we, its Harbor half). `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1` and `PI_TELEMETRY=0`
  through an `exec_as_agent` override matching `pi --print` or `pi --mode rpc`: the async arm rewrites the command
  before calling `super()`, so a `--print`-only match would have left it online (a test holds that). Pi 1.0.4's own
  docs and code: the flag gates the catalog refresh, the version check, package updates and tool downloads.
- bench/README.md says all of it.

**How it is proven.**
- **The session:** before, harbor 89, report 36, async 36 + 44; after, under the system python3 harbor 111 (25
  skipped), report 39, async 47, and under Harbor's venv the same with `ASYNC_HARBOR=1`. Records from the existing
  fixtures, main against the branch: Claude Code's identical, Pi's differing only by the added keys. Every planted
  revert failed (the profile's line, both effort defaults, the pin, each arm's stop and the async arms', `PI_OFFLINE`,
  the narrowed RPC match, the `end` read, the caps on budget alone, every answer counted, SIGKILL and the descendant
  search removed, the last two on real `/proc` with stand-ins under their own names, never `pi` or `claude`).
- **The review** (R40, the stack f0481d8f on health-imported's review commit and join fix, on fc59e7f6; $0: no
  benchmark and no model call). **Fairness end to end:** every arm's request carries effort medium and 128,000 output
  tokens: Theseus by the bench_profile assertion on the profile's own request; Claude Code by a capture of the host's
  2.1.290 on a loopback stand-in with an invented key (medium with the arm's flag, `high` honoured as an ablation);
  Pi by its 1.0.4 catalog and a live trial's log (six answers, all medium). **Records:** Theseus's and Claude Code's
  byte-equal to main's on the fixtures and on 530 real b5 trials (356 and 174 agent directories); Pi's differ only by
  the added keys. **Suites**, host python3 3.14.4 and the venv: after, harbor 112, report 39, recall 75, async 47; the
  one red, test_sampler's load-sensitive bound, fails on main's own bench/ alike (theseus-ufe5). **Plants:** both Rust
  plants caught by `binary_id(theseusd::bench_profile)` (a first run with `test(/bench_profile/)` selected 2 tests by
  name, missed the assertion and passed); 12 of 13 Python plants caught, the miss Claude Code's record under an
  ablation (theseus-p3jl). **The whole workspace suite on the stack: 3,046 of 3,047**, the one red theseus-jtrc's known
  load flake in another test of the same file, passing alone.
- **The stop without Docker** (R40): the real-`/proc` tests 9 of 9, and a probe of the branch's own script, three runs
  a case: an agent and children ignoring SIGTERM left 0 running; `nohup child &` 0; an agent forking every 0.2 s
  through the grace left 21 to 23; a child backgrounded through a subshell that exited left 1 each time (reparented
  before the read). The stop took 4.1 to 5.1 s on this host's 1,100 processes.
- **FAST:** Python and Markdown under bench/, and one Rust test assertion: nothing in the daemon.

**What the review found.**
- **The spend cap is still unequal** (theseus-7gir.22's "spend rule first"): Theseus's `THESEUS_BENCH_SPEND_LIMIT` is
  the kernel's `spend_limit_usd`, which counts reservations (about $1.28 to $1.7 a call at 128,000 output tokens), so
  at $2.00 a trial is refused once its real spend passes about $0.50 to $0.70, while Claude Code's `--max-budget-usd
  2.0` counts actual spend. R40 recommended `THESEUS_BENCH_SPEND_LIMIT=3.75` (two dollars plus one call's largest
  reservation) for the B5 rerun, with each Theseus trial past $2.00 of real spend named; it went to the owner as
  information (walk row F6). Three host variables also reach the Claude Code arm unpinned through `run_harbor.py`;
  the rerun's driver should build its environment fresh, as b5's did.
- **theseus-8xp0 (P3):** the stop reads one moment's tree, so a child an agent backgrounded through an exited shell
  (likely in Claude Code's trials: Harbor sets `FORCE_AUTO_BACKGROUND_TASKS=1`) and children forked in the grace run
  on; no model call follows, but their work can change files. **theseus-p3jl (P3):** the ablation's record untested,
  and the Theseus arm's `version` "theseus 0.0.1" for every build. **theseus-3lqk (P3):** `charts.ARMS` has no `pi`
  key, so `draft.py harbor` refuses a Pi job (the report's own flag).
- R40 wrote the B5 rerun's exact steps (the build, a fresh environment, both arms' command lines, what to check in
  every trial's record).

**The join** (the amhb stack's last, and the stack's one gate; merges 1 and 2 are Items 232 and
233). The batch-11 amhb joiner (2026-10-06 22:05 to 22:57) waited for install #9's done line
(22:18:25), then at 22:19:15 took `cloud-b11-amhb-join` with the queue clear (only review locks open) and main =
origin/main = 02de4b70, and ran its dry run inside the guarded take: three merges deep in linuxbrew git, each join fix
run by path in a copy, every resolved file compared with its review commit (R39's 7e73ab24, R40's 9a87a662 and
f0481d8f) with main's own changes since fc59e7f6 applied, no marker, format 23 and INSTRUMENTS 43 in every tree, no
line past its ceiling, the scrub's names family 0 on each merge. The branches share no file with each other. Merge 3:
nothing to auto-merge, the cloud files removed, `bench-fair/joinfix.py` ("1 blank line(s) cut" at
`bench/async/test_driver.py`'s end); staged 19 files, +874 −47, tree 357a889a, the dry run's, the staged
test_driver.py f0481d8f's less exactly its trailing blank line; `--check` 0 hits. The signed merge **5973419f**
(47833aa9 and 48a31af6), 22:19:33.
- **Before the gate:** the warm (22:20:43 to 22:25:14; the test build 3 m 10 s beside two other trees' builds, clippy
  clean); **535 of 535** targeted tests in 45.4 s (the union of the reviewers' lists: theseus-core 229, the catalog 29,
  the CLI 169, theseus-protocol 32, theseus-sim 55, theseusd 9 with bench_profile's 6, and the rest by the secret,
  handle and redact names); the cockpit's `npm test` **92 of 92** and `tsc -b` clean; protocol.gen and the goldens
  unchanged after. **Bench's suites** on frozen binaries of the merged build, under both Pythons at R40's counts: harbor
  112 (the venv's one failure test_sampler's per-process cost, 24.52 µs at 4N against its 24.0 at load 29.2, then
  112 of 112 on its one rerun alone, noted on theseus-ufe5), report 39, recall 75, async 47.
- **The gate** (from 22:38:47, minute 38, after may-build's wait; to 22:49:18, ok): a 150 s lock wait behind three
  review steps, then the lock held 402 s; 631 s in all (the suite 309, lifecycle 80, the cockpit 43). **3,107 of
  3,107** (1 slow, 24 skipped) in 307.1 s, from about 22:42:37 to 22:47:46, no hour crossed: the voice gate's 3,096
  plus the stack's 11 (aws-mints' 5, health-imported's 6; bench-fair adds an assertion, not a test); no known flake
  red. **Lifecycle passed on gate.sh's own second run:** the first (load reading 4.23) missed only the binary swap's
  p95, 366.7 ms against 200 + 2 (its p50 57.7), one swap's new process having waited 309.0 ms for the stopped one's
  store (that wait's p50 7.4 ms); the second (load 3.53) was in every budget, p50/p95 ms: cold start 27.2/35.4, from
  the config copy 26.9/29.6, clean shutdown with work waiting 37.6/60.9, a post in flight 88.4/93.9, SIGKILL then
  restart 33.9/37.3, binary swap 54.6/64.0, restore 257.6/323.4 (15 of 15 sessions served), the push's seed 0.9/1.3,
  a cancel's round trip 109.9/117.1 against 250. Jobs: L1 start 7.61/8.73 ms. Turn: frames 5 and 9 at their budgets
  (load 4.02, fdatasync p50 6.7 ms), plain p50 79.2 ms (p95 92.4), tool call 167.4 ms, resident memory 69.4 MB after
  the start and 92.2 MB after 30 turns. cargo deny ok. The joiner read the swap miss as one IO stall on the stopping
  process's store, on a path none of the three touches (theseus-1o8i's kind, the chain log 23:00, noted beside install
  #9's 11.87 s stop), settled by gate.sh's own rerun.
- **Pushed** 02de4b70..5973419f at 22:49:37 to 22:49:39 (origin/main 5973419f confirmed), the three branches deleted
  on origin, done line 22:50:05; theseus-7gir.23, sgpx and bpeg closed with 5973419f; n6p5, p6kd and a5we kept open
  until recall-fair joins (the recall arms' effort, recall's aborted rule, recall's Pi offline). The store stays at
  format 23. The joiner started install #10's section in the plan.

**The install** (not installed yet: on install #10's list). Nothing changes on the owner's daemon: bench/ only, one
theseusd test assertion, and the bench profile's effort line, which only the bench reads. bench-fair's join was the B5
held-out rerun's prerequisite (theseus-7gir.22); its hourly check found the machine never quiet enough that night (the
chain log 23:32, 00:33, 01:33 and 02:33), so the rerun had not run when this range ended.

**Divergences.** The six issues landed as three commits, since steps share files and the record's `stamp`. The
report could not run the stop against Docker; R40 proved it on real `/proc` with stand-ins and named its two gaps. The
live Harbor check was not run (it costs API money); the B5 rerun is its live test.

**Known gaps.** The spend cap's parity (R40's 3.75, for the rerun); theseus-8xp0, p3jl and 3lqk (P3); theseus-n6p5,
p6kd and a5we until recall-fair; theseus-ufe5's load-sensitive sampler bounds. docs/benchmarks.md gains the arms'
fairness in this version (Part I); the benchmark reports' index ("What a Harbor run records") does not yet list the
records' new keys (`effort`, `version`, `version_asked`, Pi's `effort_ran` and `end.turns`).

### Item 235. Cancel fast: a cancel of a running job settles on the processes' exits in two synced frames instead of five, the acknowledgement riding with the verdict; with R41's join fix, the tree stop's SIGTERM phase keeps at most 64 pidfds and closes them before the freeze (theseus-dwoj; R29's finding on daemon-proofs, Item 214; the tenth cloud batch's cancel-fast session, fired 2026-10-06 12:43 from d279767f, Opus 5.5, its steps 1 and 2 by 14:38, stopped at the account's session limit at 14:53, relaunched by the DM thread at 18:05 and fired 18:16, its report at 19:32; 223d0382, 4e8a729e and ec89e31b; reviewed 21:07 to 23:07 by local reviewer R41, and accepted with a join fix; joined 23:47 at fad08f32, a signed merge onto 5973419f, by the batch-10 cancel joiner; not installed yet: on install #10's list)

**Why.** Daemon-proofs (Item 214) gave the lifecycle bench a `cancel` row, a real `proc.run` job cancelled through
`execution.cancel`, with a 250 ms budget and no margin, "until theseus-dwoj brings it near 50". R29 read two costs in
the code: a ~40 ms floor from polling (the wrapper's `tree::stop` looked every 10 ms after its SIGTERM, and the
daemon's `terminate_all` slept on `Stopping::poll`'s backoff, looking at about 0, 10, 30, 70 and 120 ms), and, on the
owner's disk, the cancel's five synced frames (`fdatasync` about 9 ms quiet, 23 beside builds). theseus-dwoj (P3,
FAST) asked for both waits event-driven and the frames folded.

**What landed** (theseus-kernel's `tree.rs` and `job.rs`, theseus-core's `cancel.rs`, `rpc/driver.rs` and
`toolrun/{job,late}.rs`, theseus-sim's lifecycle bench, theseusd's new `tests/cancel_frames.rs`, theseus-kernel's new
`tests/tree_fds.rs` by the join fix, and both crates' AGENTS.md; the merge 11 files, +884 −101; no package, protocol
type, config key or store format change (23 stays 23), and the goldens unmoved).
- **Step 1: the waits, event-driven** (223d0382, 4e8a729e). The wrapper's tree stop keeps a pidfd for each process
  its SIGTERM phase signals (checked against its start time, as every signal is) and sleeps on them through the grace
  (`wait_exit`, as the kill already did), woken by the first exit; a child no scan has seen yet is found at the next
  look, at most `LOOK` (10 ms) later. 4e8a729e: a scan an exit woke only checks whether the tree is empty, and a process
  found new is signalled only at a look; without it, a shell's SIGTERM trap had its cleanup's first child SIGTERMed the
  moment it forked (theseusd's `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`
  caught it, its trap's file empty in 3 of 8 runs). In the kernel, `Stopping` opens each asked wrapper's pidfd once its
  pid reads as the job's wrapper and checks again after the open, so the descriptor never names a later process given
  that pid (a wrapper gone in between falls back to `poll`'s backoff, main's behaviour); `Stopping::exits` hands the
  owner every unsettled job's pidfd and the time to the next deadline, or none when any job has no pidfd yet. The
  daemon waits on them through `AsyncFd`, holding no worker; `terminate`, the thread owner, polls them; the daemon's own
  stop takes the same path.
- **Step 2: two frames** (ec89e31b). A cancel of one running job wrote five synced frames on main: the cancel
  (`[execution, action, ledger:execution.cancelled]`), the acknowledgement, the verdict, the verdict's fact
  (`ledger:action.cancel_verified`) and the call's answer (a `node`). Now two: the cancel's own, and
  `[ledger:action.cancel, execution, action, ledger:action.cancel, ledger:action.cancel_verified, node]`.
  `terminate_all` splits into `stop_backends` (as before) and `Stopped::write_verdicts`, which writes each job's
  acknowledgement, verdict and, when asked, its fact's row (`verdict_row`), each job a nested frame so one failure
  takes back only itself; `cancel_execution_judged` runs it inside `answer_after_cancel_with`'s frame, after closing the
  session's terminals. That frame may be built twice under the session's hold (an answer that is outside text), so the
  cancel is counted for health and `theseus.cancel` only once it is written. `Ended::record` only announces a fact
  whose row rode in its step's frame; a stop at a job's launch still records its facts through the turn.
- **The acknowledgement moved** from before the ask to the verdict's frame: on the daemon's job path no store holds a
  job's cancel `acknowledged` while the job still runs. A crash before the verdict leaves `requested`, which recovery
  reads as it read `acknowledged` (every reader asks only `is_some()`); the AWS hands keep the old order
  (`aws/hands/cancel.rs` writes its own acknowledgement, which the Fargate verify alone reads).
- **The bench** prints each cancel's frames (`cancel: N to N frames a cancel`, the minimum and the maximum).
- **R41's join fix** (tree.rs: `WATCHED` = 64, a `term` helper, `drop(termed)` before the freeze; and
  `tests/tree_fds.rs`): the grace keeps at most 64 pidfds to wake its wait; a process past them is signalled as before
  (pidfd opened, used, closed) and its end seen at the next look.

**How it is proven.**
- **The session** (the relaunched one reran every proof on fresh builds; no code changed in it). The cancel row, `bench
  lifecycle --phases cancel --runs 20` in A B B A on the cloud VM: step 1 took the quiet p50 from 41.6 to 10.9 ms (5
  frames each); step 2 from 10.7 to 8.2 and 8.4, with 2 frames; four busy loops at nice 0, base 40.9 and 44.3 against
  10.7 and 9.0; under nice 19 the whole daemon starves (~350 ms on every build, base included) and step 2 still read
  ~10 % lower. The VM's syncs cost microseconds, so step 2's three saved syncs showed as ~2.5 ms there; on a disk at 9
  to 30 ms a sync they are worth 27 to 90 ms. The new `cancel_frames` tests hold the two frames record by record on a
  real daemon with a real `sleep 60` job, and for a `gh` job (outside text, the frame built twice) health counts one
  cancel. Plants: the fact's row in a frame of its own (both tests fail, three frames); the acknowledgement before the
  ask; the count inside the frame's closure (two counted). The frame tests at nice 19 beside four loops, 6 of 6.
- **The review** (R41, review merge c3fed45f on c0ed7ada, the join fix b109e8f3): fmt, clippy `-D warnings`, shape,
  protocol.gen unchanged; the branch's tests and the code they meet **516 of 516**, **518 of 518** with the join fix's
  two; **the whole workspace suite 3,051 of 3,051** (`--retries 0`, 385 s). **The cancel row, settled** (one hold of
  the gate lock, A B P1 P1 B A twice, 20 cancels a block, PSI read around each; frozen debug builds, the disk's
  fdatasync 6.7 to 6.9 ms p50):

  | arm | p50 by block (ms) | median p50 | median p95 | frames a cancel |
  |---|---|---|---|---|
  | A, main c0ed7ada | 122.1, 105.5, 107.8, 113.2 | **110.5** | 191.2 | 5 (one block 5 to 7) |
  | B, the merge | 31.2, 31.3, 28.5, 29.3 | **30.2** | 50.5 | 2 |
  | P1, the merge with step 1's plant | 54.3, 60.4, 63.8, 61.8 | **61.1** | 106.1 | 2 |

  Each pair's Mann-Whitney exact p on the blocks' p50s 0.029, the least four against four can give: the waits are
  worth about 31 ms here, the frames about 50, about 3.7 times faster in all. Beside R41's own workspace build (load 12
  to 21, CPU PSI up to 37 %): main 116.0 and 142.5 p50, the merge 45.6 and 65.9. One 50-cancel run of main **missed
  main's own 250 ms budget** (p95 330.9, max 825.5). `bench lifecycle --runs 10 --check` on the merge: LIFECYCLE OK
  twice (cancel p50 28.5 and 26.6, 2 frames); the turn bench frames 5 and 9 on both builds, the p50s within noise.
- **Plants** (R41): the report's two caught (step 1's by the bench row, 61.1 ms; step 2's three frames); four of R41's
  caught (the fact's row written twice; the acknowledgement dropped; the join fix's two halves); 4e8a729e's revert
  caught only by timing, the SIGTERM-trap test failing 5 of 10 stress runs (cargo-nextest 0.9.146 exited 0 from that
  `--stress-count` run anyway: read a stress run's Summary, never its exit).
- **The new-child rule:** the SIGTERM-trap test passed 50 of 50 (20 quiet, 20 beside two trees' builds, 10 at nice 19
  beside clippy and two builds), job_approval's setsid cancel test 40 of 40 beside them: no survivor.
- **Live** (R41, scratch daemons of the frozen builds, the bench's config, the stand-in model): a job ignoring SIGTERM,
  the daemon SIGKILLed 0.5 s into the cancel's 2 s grace: main's action read `acknowledged`, the merge's `requested`;
  each wrapper finished its stop and wrote its verdict in about 2.0 s; after the restart both ended `outcome_unknown`;
  the same with the daemon's own clean stop at that point (it exited in 74 ms on the merge, 41 on main). Across 50
  cancels, sampled every 20 ms, the daemon's descriptors stayed at 19 to 22 with no climb and at most one pidfd open at
  a time. The answer's text and the `action.cancel*` rows byte-identical on both builds.

**What the review found.**
- **The open-file limit (the join fix).** Step 1 kept a pidfd for every SIGTERMed process until the stop returned, and
  the freeze's and the kill's pidfds came on top, under the wrapper's open-file limit (the daemon's, soft 1,024). So a
  tree of more than about 500 processes that outlive SIGTERM was not killed whole, past about 1,000 nothing was frozen
  or killed, and while the table was full the SIGTERM phase skipped processes and the stop's `/proc` scan failed. R41's
  probe, 40 SIGTERM-ignoring processes under a limit of 64: 20 alive after the stop, which said `killed: 40, survivors:
  []`. With the fix, all 40 gone. Its two tests:
  `a_stop_under_a_tight_open_file_limit_kills_a_tree_that_outlives_sigterm` and `…_gives_every_process_its_sigterm` (50
  trapping shells and their sleeps, all 50 traps run); their plants: the `drop` taken out, 20 of 40 alive and frozen;
  the bound taken out, 41 of 50 traps.
- **theseus-vvyf (P3, main's too):** after a crash or a clean stop inside a cancel's grace, recovery marks the job
  `outcome_unknown` and never reads its wrapper's verified `stops/` verdict. **theseus-43k1 (P3):** the look rule is
  held only by a timing test; a test by order is proposed. **theseus-ie79 (P3):** an unregistrable pidfd sleeps to the
  next deadline, not the backoff, and `write_verdicts`' nested frame can panic on an execution its frame did not name.
- **For the owner:** the cancel row's budget from 250 to **100 ms** (each block of main misses it and none of the
  merge's; two single cancels of about 280 were over 100, so about one gate in fifteen would rerun), and a bench check
  that fails when the *minimum* frames a cancel exceeds 2 (the maximum picks up memory's labelling frames under
  starvation). The DM thread took both as a follow-up separate from this join (the chain log, 23:20). Accept the
  acknowledgement's move and record it here: done.

**The join** (batch 10, the branch alone; the joiner 23:20 to 23:53). The queue clear at 23:26:38 (main = origin/main =
5973419f, only review locks open). The guarded take (23:26:48) took `cloud-b10-cancel-join` and ran the dry run on
5973419f: merge-tree clean, the join fix applied 5 of 5 and a rerun "already applied", the resolved tree 31987141
differing from the merged one only in the cloud files and the fix's two files, b109e8f3's byte for byte; every branch
file equal to b109e8f3's (lifecycle.rs with main's own four hunks, health-imported's `health_keys`, applied); format 23,
INSTRUMENTS 43; the names family 0 (its one hit the new test's fake `op://` reference, the house pattern). The merge:
theseus-core's and theseus-kernel's AGENTS.md and lifecycle.rs auto-merged, no conflict, no resolve.py; the cloud files
removed; `cancel-fast/joinfix.py` (four "applied", one "written"); staged 11 files, +884 −101, the dry run's tree; the
signed merge **fad08f32** (5973419f and 3825e489), 23:27:01. The warm (23:27:16 to 23:35:08, the test build 6 m 15 s
beside two other trees' builds, clippy clean); R41's filter **440 of 440** in 55.9 s (theseus-kernel 165 with
`tree_fds`' 2, theseusd 27 with `cancel_frames`' 2 and the trap test, theseus-core 248).
- **The gate** (23:36:40, minute 36, to 23:45:44, ok; a 56 s lock wait behind two review steps, then 439 s held; 544 s
  in all): **3,111 of 3,111** (1 slow, 24 skipped) in 331.0 s, about 23:38:24 to 23:43:56, the amhb gate's 3,107 plus
  `cancel_frames`' 2 and `tree_fds`' 2; no known flake red. **Lifecycle on gate.sh's own second run:** the first (load
  reading 11.19, about three times the last gates') missed only the config copy's cold start p95, 58.5 ms against 50 +
  7, a path that runs none of the changed code; the second (12.14) was in every budget. **The cancel row: 30.1/37.2 ms
  p50/p95 in the first run, 40.9/72.2 in the second, `cancel: 2 to 2 frames` in both** (the amhb gate's 109.9/117.1
  and the voice gate's 108.0). The rest of the second run, p50/p95 ms: cold start 41.4/53.0, the config copy 46.9/52.5,
  clean shutdown with a job running 51.4/102.8, a post in flight 95.0/110.0, SIGKILL then restart 62.1/151.4, swap
  75.5/118.5, restore 366.0/412.7; jobs' L1 start 15.17/18.34 ms; turn frames 5 and 9, plain p50 114.3 ms, tool call
  220.4 (at load 11.4; R41's turn A/B showed the branch moves neither).
- Pushed 5973419f..fad08f32 at 23:47:09 to 23:47:11, the branch deleted on origin, done line 23:47:29; theseus-dwoj
  closed with fad08f32. 33 min from the take to the done line. The store stays at format 23.

**The install** (not installed yet: on install #10's list). On the owner's daemon, a cancel of a running job settles in
about 30 ms instead of about 110 (R41's settled A/B on the owner's machine: p50 110.5 to 30.2 ms, p95 191 to 50.5), in 2
synced frames instead of 5; a job's cancel reads `acknowledged` only once its stop has ended; a large tree is still
killed whole under the wrapper's open-file limit. No config key. The lifecycle bench's budget stays 250 ms until the
follow-up.

**Divergences.** No unit test for step 1: the brief allowed one only if it proved by order, and the gain is wall
time, guarded by the bench row (R41 asks for the order test, theseus-43k1). The acknowledgement's move changes what
`acknowledged` means on the daemon's job path, accepted as R41 recommended. The cloud session ran in two parts across
the account's session limit.

**Known gaps.** theseus-vvyf, 43k1 and ie79 (P3); the cancel row's 100 ms budget and the minimum-frames check (the
follow-up; lifecycle.rs's `budget_ms` comment moves with it); theseus-kernel's AGENTS.md tree.rs line ("asleep on
their pidfds") does not yet name the 64-pidfd cap (`WATCHED`), owed by the review. cancel-fast changed theseus-kernel's
`job.rs`, which writes the wrapper's lingering mark that theseusd's `job_wrapper` test waits for; after the first gate
red on that test (theseus-sdgl, 2026-10-07 02:48, told in Item 241), the DM thread named this join
among the suspects and held install #10 for R48's verdict.

### Item 236. Daemon flakes at their causes: a stopping daemon begins no retry, model call or in-turn retry (a call reached after the stop began is settled failed unsent, so the next start retries it), and the stop test's trap takes its time in the shell, ignores SIGTERM before anything it forks, and writes whole before visible (theseus-jtrc and theseus-y0lm; the gate's two theseusd load reds, found by local reviewer R32 at imported-skip's review and by the B9-core joiner at daemon-proofs' join, Item 214; the eleventh cloud batch's daemon-flakes session, fired 2026-10-06 18:10 from 57f265f2, Opus 5.5, its report at 20:05; 1857d1e2 and 859b09d5; reviewed 22:08 to 2026-10-07 00:12 by local reviewer R42, and accepted with no join fix; joined 01:00 at c7debb6e, a signed merge onto fad08f32, by the batch-11 flakes joiner; not installed yet: on install #10's list)

**Why.** Two theseusd tests failed under load and sat among the gate's known flakes:
- **theseus-jtrc:** `bench_profile`'s `a_first_byte_timeout_is_retried_inside_the_headless_turn` (theseus-7gir.21) runs
  a headless turn with `[model.retries] transient = 0` against a stand-in that sends no first byte; the turn fails
  with `FirstByte` and exits 1, as it should, but under load (about 1 run in 3 at nice 19) the stand-in saw 2 requests,
  not 1. A second request is a paid call in a bench trial.
- **theseus-y0lm:** `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker` read a job's
  SIGTERM file empty (stops.rs:249) in a pre-gate run of daemon-proofs' merge and again at learned-shadow's review.

**What landed** (theseus-core's driver, turn and retry steps, with a new `turn/stopping_step.rs` and
`tests_stopping.rs`; theseusd's `tests/bench_profile.rs`, `tests/common/model.rs` and `tests/stops.rs`; both crates'
AGENTS.md; the merge 12 files, +333 −19; no package, protocol type, config key or store format change (23 stays 23)).
- **jtrc's cause** (1857d1e2). The second request was the session driver's own retry. A first-byte timeout is a class
  that passes with time, so `Failing::after` says backoff, the turn wakes its execution at once, and the driver
  (`harness.rs` `drive`) starts the continuation as soon as the turn ends: the driver's backoff map is written only when
  a continuation errors, so its first retry never waits. A probe of a real `--stdio` daemon with `shutdown` sent right
  after the answer (as `ask`'s close does) showed, 3 of 3, the continuation beginning before the stop and reaching its
  model call during it; only the runtime's end kept the request off the wire ("A Tokio 1.x context was found, but it is
  being shutdown"), and under load the connect beats the runtime's end. Neither an HTTP resend nor a second submit.
  Nothing on a turn's path read the stop, and `notify_waiters` reached only a driver parked in its `select!`.
- **jtrc's fix.** The stop's per-core mark, `Outbox::stopping()`, set by the stop's first step for a client's
  `shutdown`, SIGINT, SIGTERM and a restart alike, is now read in three places: `Core::continue_execution` begins
  nothing once the stop has begun (`Ok(None)`; the execution stays queued for the next start's driver); the driver's
  loop is `while !core.outbox.stopping()`, so a stop's wake missed while busy still ends it; and a call about to be sent
  (`call_model`, before the stream is first polled) is settled failed **unsent** through the existing `settle_failed`:
  `ProviderError::Network { "not sent: the daemon's stop began before the call went out" }`, $0, its reservation
  freed, transient, so the run says backoff and the next start retries it, exactly how the passing runs already settled
  it. No in-turn retry runs while stopping (`turn/retry_step.rs`), else the bench profile's `transient = 4` would wait
  out four backoffs and settle four unsent calls. The first-byte test's count now prints, when wrong, each request the
  stand-in saw (its arrival, unix time, the client's port, its body) and the daemon's log (`FakeModel::peers()`).
- **y0lm's cause** (859b09d5): neither of the issue's two readings. The wrapper's own stop (`tree::stop`'s phase 1)
  scans the tree every 10 ms through the grace and SIGTERMs every process it has not met, so the trap's `date +%s%N >>
  job-<i>.term` (dash opens the `>>` in the child, then execs `date`) died of SIGTERM whenever a scan caught it, leaving
  the file empty, 0 to 2 s into the grace. Shown directly: a scratch trap's planted `/bin/sleep 0.5` exited 143 in all
  three jobs. No kernel change: cancel-fast's 4e8a729e (Item 235) answers the same red from the
  kernel's side, signalling a new process only at a look; the two are complementary.
- **y0lm's fix.** The job is `bash -c`; its trap takes the time in the shell with no fork (`t=$EPOCHREALTIME`),
  ignores SIGTERM from then on (`trap "" TERM`, so anything it forks inherits the ignore), writes `job-<i>.term.tmp`
  with builtins and `mv`s it, so the file is whole before it is visible; the job is as stubborn (only SIGKILL ends it),
  and the test proves the same order (theseus-3dsz). A missing or empty time panics with `stop_seen`: each job's files,
  the stop's `action.list` rows, its answer and `took`, and the daemon log's lines for the stop. theseusd's AGENTS.md:
  a test trap must not fork what it records.

**How it is proven.**
- **The session** (a 4-core root VM, where neither red reproduced under the brief's recipe: 30 runs each before, 0
  failures, so the probe and a planted trap child are the reproductions): after, 30 of 30 each under load. The new
  tests, both by order: `the_driver_begins_no_retry_once_the_stop_has_begun` (the provider saw 1 request, the execution
  stays queued) and `a_call_asked_after_the_stop_began_is_not_sent` (0 requests, one `provider.error` row, `turn.next`
  backoff, the execution queued and resume-pending). Plants: each of the three checks removed fails a test (the
  in-turn retry's with "a retry inside the turn", left 5, right 1); `terminate_all` stopping each job only after the
  last one's verdict fails the stop test on its spread. At 12 busy loops (3 per core) the stop test never failed on
  its file but its health bound (`slowest < 1500 ms`) failed 17 of 26 (a finding, below).
- **The review** (R42, in R39's warm tree; review commit 260c1b91 on 02de4b70, then **6a90e59c** on fad08f32 once main
  had moved twice, tagged `r42-flakes`): fmt, clippy `-D warnings`, shape, protocol 32 of 32; the selection 63 of 63;
  **the whole suite 3,098 of 3,098 on 02de4b70 and 3,113 of 3,113 on fad08f32** (`--retries 0`). **Plants:** the
  report's four and R42's three all caught (the refused call settled as a lasting class, `Api` 400, fails the backoff
  property; the trap's `trap "" TERM` removed with a slow fork before its `mv`, "job 0 had no SIGTERM", each
  `.term.tmp` whole and no `.term`; a job's time read from a file no trap writes), and R42's control (the same slow fork
  with the ignore kept) passes, as it should.
- **Under the issues' own recipe** (16 busy loops at nice 0, one per core, the tests at nice 19, main and the branch
  interleaved run by run in one window, each arm's own frozen binaries bind-mounted in a user namespace so both met the
  same load): the first-byte test, at load 19 to 33 and CPU PSI 35 to 62 %, **main failed 1 of 20** ("no retry inside
  the turn", left 2, right 1), **the branch 0 of 20**; the stop test, at load 20 to 32, **main failed 2 of 10** with the
  issue's own signature, **the branch 0 of 10**; the health bound held in all 20. Beside the machine's own builds, both
  tests 0 of 20 on the branch.
- **Live** (R42, scratch `--spawn` daemons of each arm's frozen build, the bench profile as the test sets it, a
  loopback stand-in that gives the first request no byte; `theseus --spawn theseusd --json ask "say hello"`, whose
  close sends `shutdown` at once, then the restart): on the branch, 6 of 6, one request by `ask`'s exit, then the
  retry's request 0.60 to 1.69 s after `ask` ended, from the restart's driver, answered ($0.00024); its ledger the input
  turn's `provider.error` (first_byte), `server.stopping`, the continuation's `turn.started`, its `provider.error`
  network "not sent", `turn.next` backoff, then the next start's continuation, completed. On main, 6 of 6 the same by
  count, unloaded, but its continuation's call was made during the stop each time and failed only because the runtime
  was ending (48 to 115 ms after the stop began): the race that load turns into a second request. In every live run
  the continuation had passed the driver's check before the stop began (its `turn.started` 0 to 22 ms after
  `server.stopping`), so the call step's check is the one that closes the bench's red.
- **FAST** (A/B of frozen debug builds in palindrome order, one hold a block, PSI read around each; the machine never
  quiet that night): the turn bench, three windows and 14 blocks, frames 5 and 9 on both arms in every block, the
  medians A 103.9 and B 106.2 ms (the wall tracking the shared disk's fdatasync, 6.8 to 21 ms a sync); the stop rows,
  two windows and 8 blocks, the quiet pair within 5 ms on every row: **no turn slower and no stop longer**. The new
  check is one uncontended `watch` read a call.

**What the review found** (all P3, the first two the owner's decisions, rows F7 and F8 in the question walk, not yet
answered).
- **theseus-36re:** for a session bound to a place, a turn refused for the stop now posts "⚠️ **Turn failed** (network):
  network error: not sent: … — Retrying with backoff…" at the next start (R42's probe: one `failed` post waiting in the
  place's outbox), where main posted it only when its connect lost the race. R42 recommends no notice for this case and
  a failure class of its own (`stopping`), so neither the place nor telemetry calls it a network error; the class stays
  `network` here, since a new `ProviderError` variant is serialized.
- **theseus-zqxv:** the residual window: a client slower to send `shutdown` than the daemon is to reach the retry's call
  lets the retry go out before the stop begins. Measured: `shutdown` came 21 to 121 ms after the failed turn settled,
  and the retry's call 42 to 314 ms after the stop began, so it opens only when the client is starved some 50 ms more
  than the daemon; not seen in 46 runs. R42 recommends the report's option 2 (a `--stdio` daemon leaves a failed client
  turn to its client) over a first retry that waits its backoff, which would slow every recovery on the owner's daemon.
- **theseus-nsaf:** the stop test after y0lm: its health bound at three busy loops a core (keep it, and read a failure
  before loosening it), `stop_seen`'s log part empty at the daemon's default level, and a whole `.tmp` with no `.term`
  (a `mv` cut by the grace's SIGKILL) failing though the time was taken.

**The join** (batch 11, the branch alone; the joiner 00:33 to 01:05). The queue clear at 00:39:08 (main =
origin/main = fad08f32; only review locks open). The guarded take (00:39:17) took `cloud-b11-flakes-join`; the dry
run: merge-tree clean, the resolved tree d68ae758 exactly 6a90e59c's (0 files differ), format 23, INSTRUMENTS 43, the
core's 94 test mods in order, turn.rs 3,500 of its 3,523, the names family 0. The merge: theseus-core's AGENTS.md,
lib.rs, `rpc/driver.rs` and turn.rs and theseusd's bench_profile.rs auto-merged, no conflict, no resolve.py, no join
fix; staged 12 files, +333 −19; the signed merge **c7debb6e** (fad08f32 and a3419f68), 00:39:28. The warm (00:39:42 to
00:45:24, the test build 4 m 51 s beside two reviewers' builds, load up to 38.8, clippy clean); R42's filter **13 of
13** (tests_stopping's 2, bench_profile's 6 with jtrc's test, the stop test, cancel_frames' 2, failures' 2).
- **The gate** (00:46:01, minute 46, to 01:00:05, ok): **a 452 s lock wait** behind two reviewers' A/B bench loops,
  each holding the gate lock shared for a whole loop of blocks (R43's for route-wait, then R44's for discord-watch,
  begun 22 s after the gate while it compiled, so review-step.sh had no waiting gate to yield to); then 367 s held; 844
  s in all. **3,113 of 3,113** (1 slow, 24 skipped) in 311.4 s: cancel-fast's 3,111 plus tests_stopping's 2; **the two
  old reds passed** (the first-byte test in 5.2 s, the stop test in 5.6 s); the suite about 00:53:58 to 00:59:11, 49 s
  before the hour, the AWS runaway-mode tests (theseus-5a50's) passing early in it; no red at all. **Lifecycle on its
  first run** (load reading 5.94), p50/p95 ms: cold start 27.0/34.6, the config copy 30.6/35.8, clean shutdown with a
  job running 41.4/65.0, a post in flight 99.6/119.4, SIGKILL then restart 51.7/66.8, swap 63.2/118.1, restore
  254.0/284.3, a cancel's round trip 29.1/37.9 with `cancel: 2 to 2 frames`. Jobs: L1 start 10.34/12.84 ms. Turn:
  frames 5 and 9, plain p50 79.8 ms, tool call 166.4 (the amhb gate's 79.2 and 167.4).
- Pushed fad08f32..c7debb6e at 01:00:26 to 01:00:28, the branch deleted on origin, done line 01:00:40; theseus-jtrc and
  y0lm closed with c7debb6e. **From this join on, a red in either test is a finding, not a flake**; the known load
  flakes are theseus-3ae1, pb3l, ndg9 and ufe5 (bench only). The store stays at format 23.
- **The joiner's finding** (the chain log, 01:32): a gate can wait behind reviewers' bench holds that each take every
  block at once, and its suite can then run toward the hour though it started inside the timing guard. The R12
  procedure now says a bench hold takes the lock one block at a time and never starts while a join lock is open without
  its done line.

**The install** (not installed yet: on install #10's list). On the owner's daemon, a stop never sends a paid request
nobody reads: once the stop has begun, the driver begins no continuation, a call reached after it is settled failed
unsent at $0 (`network`, transient), and no in-turn retry runs; the next start retries the execution. A turn refused at
a restart posts "Turn failed (network)" to its place at the next start, then the answer follows (theseus-36re, the
owner's call pending). y0lm is test-only. No config key.

**Divergences.** Neither red reproduced in the cloud; each cause was proved directly (a probe for jtrc, a planted
child for y0lm) and each fix by a test that fails without it; R42 then reproduced both on main under the recipe. The
report's answer to the brief: daemon-stops' `shutdown` fix alone would not close jtrc, since the continuation begins
at the failed turn's end, before the `shutdown` arrives; only a check at the call closes it. The class stays
`network`.

**Known gaps.** theseus-36re and theseus-zqxv (the owner's), theseus-nsaf. The driver loop's own `while !stopping()`
has no test of its own (covered in effect by `continue_execution`'s). theseus-core's AGENTS.md's new sentence (line 19,
154 columns) is owed a rewrap. Part I's account of the driver's retry (§3.15, theseus-ljr) is amended in this version.

### Item 237. Route wait: route.v1 asks Jev in a request of its own beside the inbound batch, Jev's connections open after serving and stay warm, and `route.decided` says `late` and `answered_ms`, with the metric `theseus.route.wait` (theseus-ddbi, v1's speed item; local reviewer R3's live check of step 25e, Item 139; the tenth cloud batch's route-wait session, launched by the DM thread and fired 2026-10-06 13:50 from f589d9cb, Opus 5.5, its step 1 by 14:31, stopped at the account's session limit at 14:53, relaunched at 18:05 and fired 18:16, its report at 20:00; 10a2fd8f, 1c68f7d9, e4577bfd, 3d1df6ca and 7c94e983; reviewed 22:08 to 2026-10-07 00:59 by local reviewer R43, and accepted with a resolve.py and no join fix; joined 01:29 at f81c0e62, a signed merge onto c7debb6e, by the batch-10 route joiner; not installed yet: on install #10's list)

**Why.** Step 25e (Item 139) made the turn wait for route.v1's verdict at most `[routing] max_wait_ms` (200) after its
first compile, and route.v1 rode the inbound batch with classify.v1 and role.v1 (7 questions, about 373 output
tokens). R3's live check (2026-10-04, the judge on and Jev live) saw every routed message wait (`wait_ms` 133 to 201),
and four of nine verdicts miss the bound, two on a fresh daemon's first message, two on a warm connection: the batch
answered about 2 s after the turn began, so about half the time the turn ran on the previous message's verdict. With
the judge on, as on the owner's daemon, each message paid up to 200 ms on the turn path, and the gate's benches run the
judge off, so they could not show it. theseus-ddbi (P1) asked for the verdict sooner, Jev's connection warm, and the
wait visible; it was named among v1's conditions (the chain log, 22:07).

**What landed** (theseus-core's `judge/inbound.rs`, new `judge/warm.rs`, `turn/route_step.rs`, `fact/route.rs` and
telemetry; theseus-judge's client and fake; theseusd's `after_serving` and its judge test; three new test modules; the
merge 18 files, +1,091 −98; no package, protocol type, config key or store format change (23 stays 23); 44 instruments).
- **Step 1: route.v1 asks alone** (10a2fd8f, the first session). Jev answers one JSON body, not a stream, so taking
  route.v1's answer as it streamed (way a) would need Jev to stream; the session took way (b): the inbound point's asks
  are partitioned by pack name, route.v1 in a request of its own and the other packs batched, the two sent at once
  (`futures_util::future::join`), and route.v1's verdict goes down the turn's oneshot from inside its own future (an
  `Answered { verdict, at }`), never after the batch's. Each request is reserved as it goes out and has its own
  urgency (live only when one of its packs acts, so a batch of shadow packs can be shed when the eight permits are
  taken); each judgment carries its own request's call id and cost; the point still settles once. The fake Jev gained
  `set_latency(single, batch)`, `keep_alive(handshake)` and `opened()`.
- **Step 2: no first-message handshake** (1c68f7d9). The client's only warm-up (`warm_on_message`, theseus-otny) ran
  beside route.v1's request, so a fresh daemon's first request still paid DNS, TCP and TLS, and reqwest's pool
  (`POOL_IDLE`, 180 s) closed idle connections, so the first message after three quiet minutes paid them again. Now
  `Core::warm_judge`, from theseusd's `after_serving` beside `warm_ladder` (never on the start path), acts when the
  judge is on and an inbound pack or the rerank is on: it builds the client (so the sink's task starts after serving
  too), opens two connections (the two requests go out at once) with unbilled `HEAD`s of the judge's path, and spawns a
  keeper that sends them again after each `KEEP_WARM` (150 s, inside the pool's 180) of Jev's silence; any answer of
  Jev's, a call's included, resets its clock, so a conversation sends no warm-up. It holds the service by `Weak` and
  ends with it. The client gains `refresh` (a warm-up whether warm or not) and `heard_ago`. theseusd's
  `a_start_with_the_judge_on_builds_nothing_of_it` now expects the breaker `closed` after serving; its name stays true
  of the start path only.
- **Step 3: the wait made visible** (e4577bfd). `route.decided` keeps `wait_ms` and gains `late` (on a turn route.v1
  acts on, live or canary, the verdict had not come by the decision: the bound ran out, or no wait because Jev was
  known unreachable; false in shadow or under a pin) and `answered_ms` (when route.v1's request came back, in ms after
  the turn's start, or null when it had not by the decision). The histogram `theseus.route.wait` (ms), by
  `theseus.route.late`, is recorded for each turn route.v1 acts on, never shadow, so shadow's zeros do not dilute it.
- **Step 4: the default bound's table** (3d1df6ca): `tests_route_measure.rs`, ignored, run by name, a fresh rig a cell
  and the fake Jev with keep-alive and a 300 ms set-up. A verdict under the bound now comes in time every time whatever
  the batch's latency (400, 1,000 or 2,000 ms); one over the bound costs the whole 200 ms and buys nothing for that
  message; the warm-up turns a first message from always late (cold) into in time at 80 and 150 ms. The report's rule
  for the default: keep 200 until a week of live `answered_ms`; then keep it if the p80 of `answered_ms` less the
  compile is under about 180, lower it to about 50 if route.v1 alone answers in 300 ms or more at the p50, else set it
  at that p80 rounded up to 50 ms.
- 7c94e983: the first-message test lost a wall-time bound that failed under load in five runs of five; the assertions
  that prove the step (the wait past the compile under 1 s against a 1.5 s set-up, no new connection) held in every run.

**How it is proven.**
- **The session:** the new tests (`tests_route_wait`'s 4, `tests_route_late`'s 3, `route_step`'s paused-clock tests
  checking the instant the verdict came); planted reverts, each caught (the verdict sent only after both requests end:
  `wait_ms` 2,995; `warm_judge` moved into `Core::build`, before serving: "nothing reaches Jev before serving"; the
  keeper calling `warm_up`, which skips a warm client; `late` never set). Each gate failed only on the cloud VM's 33
  known L1 tests, after step 2's fix of the one failure it caused (the judge test above); under load, the route tests
  11 of 11 in five runs. The session found that the first session's "33 failures were the known L1 ones" had not been
  checked against its own change.
- **The review** (R43, review merge a99d0a9b on 02de4b70, then **b6346e9f** on 5973419f, tagged `r43-route`): fmt,
  clippy `-D warnings`, theseus-protocol 32 of 32 with protocol.gen unchanged, shape; the routing, judge, inbound,
  rerank, notices, ladder and telemetry selection **367 of 367**; **the whole workspace suite 3,115 of 3,115** (main's
  3,107 plus route-wait's 8; the four measurements ignored); under load (nice 19 beside four busy loops), 38 of 38 three
  runs of three. Plants: the report's four caught; R43's six, each a claim no test holds though the code is right
  (route.v1's request unreserved, `answered_ms` stamped at send, a keeper deaf to Jev's answers, the metric recording
  shadow turns, an unreachable Jev's skipped wait not recorded `late`, the batch taking route.v1's urgency), not caught:
  theseus-j84t.
- **The wait, before, read-only from the owner's daemon's own ledger** (its 24 routed messages of 10-05 and 10-06): the
  inbound batch answered in http_ms p50 128 on a warm connection and p50 196 (max 286) after the pool sat idle over
  170 s or on a first message; the turn's wait after its compile p50 0, mean 10 ms, 23 of 24 in time; the one late
  message came after 53 minutes of silence (wait 215 ms). The owner's compiles mostly outlast a warm batch, so the wait
  shows after an idle stretch.
- **Before and after, on the fake Jev at the owner's latencies** (main's way rebuilt in the same tree; three fresh rigs
  a cell, six messages each):

  | fake Jev (alone / batch / set-up, ms) | | first message: wait, late | later: mean wait, late | turn wall p50, first / later |
  |---|---|---|---|---|
  | 90 / 130 / 100 (the owner's p50s) | before | 201 ms, 3 of 3 late | 125 ms, 0 of 15 | 324 / 221 ms |
  | | after | 86, 0 of 3 | 86, 0 of 15 | 195 / 156 |
  | 150 / 300 / 150 (a slow day) | before | 201, 3 of 3 late | 202, 15 of 15 late | 379 / 306 |
  | | after | 147, 0 of 3 | 145, 0 of 15 | 275 / 218 |

- **Live** (R43, scratch daemons of main 5973419f and the review commit, with a loopback stand-in for Jev at 90 ms
  alone, 130 batched and 100 a new connection's set-up; four messages 3 s apart): main's first message opened a third
  connection, answered at 236 ms, waited 200 and was **late** (375 ms wall), then waits 125, 106 and 113; the branch's
  keeper sent its two `HEAD`s at serving, route.v1's request (http_ms 94) and the batch (134) rode them, and the waits
  were 92, 87, 88 and 86, every one in time (`answered_ms` 114 to 126); `theseus.route.wait` exported one point,
  late=false, count 4, sum 354.6 ms. **The keeper:** its `HEAD`s about 20 ms after the serving line, never before; none
  during a conversation; after 150 s of silence two on the same two connections; a hanging Jev (SIGTERM with both
  `HEAD`s in flight: stopped in 0.07 s; a message at once waited 201, `late`, then Jev known unreachable and the next
  message did not wait); a dead Jev the same on both builds; the breaker `closed` after serving, a warm-up moving none.
- **The cost of the second request**, the report's open question, answered from the owner's ledger: real Jev bills the
  state once a request (input less the state is 1,476 to 1,586 tokens whatever the state's size), so the split adds
  the state and one request's overhead a message: **about 24 micro-dollars at the owner's median state** (177
  tokens), at most about 142 at the state's 3,000-token cap, **about $0.0024 per 100 messages**.
- **FAST** (A/B of frozen debug builds in palindrome order, a third arm A2 built from main in R43's own target once an
  arm's missing `theseus-index` was found to skew the first runs): the start path with the judge off (the keeper
  returns at its first line) cold-start p50 A 28.5, A2 27.1, B 27.5 ms, the cancel phase's medians A 108.4, A2 113.0,
  B 108.2 (Mann-Whitney p 0.67 to 1.0): **no difference**; with the judge on, the daemon's `serving_ms` p50 A 120, B
  113: nothing new before serving; the judge-off turn frames 5 and 9 in every block, no difference past the noise;
  the judge-on turn bench (judge-turn-cost's, Item 221) frames 5 and 9 and the judge's frames before
  an answer 0 in every block but one (judge-sink's busy bound, in an arm none of route-wait's path runs in).

**What the review found.**
- **theseus-nrcl (P3):** route.v1 asks apart in shadow too, where no turn waits: two requests and the state billed
  twice a message for nothing, on days the ladder rolls route.v1 back or under `[routing] mode = "shadow"`; and health's
  `calls_today` counts the inbound point once though it now sends two requests. R43: ask it apart only when it acts.
- **theseus-j84t (P3):** the six claims no test holds, each with the test that would.
- **For the owner** (R43's, adopted by the DM thread as the install entry's): keep `max_wait_ms` at 200 until a week
  of `late` and `answered_ms` rows, then set it by the report's rule; keep the keeper's idle `HEAD`s always on (about
  1,150 a day, unbilled; the owner's routed messages often come after long silences, 53 minutes, 20 hours, 34 minutes
  among the last 24, exactly when a first message paid the set-up); rename the theseusd test to say "before serving"
  when its file is next touched.

**The join** (batch 10, the branch alone; the joiner 01:03 to 01:34). The queue clear at 01:11:14 (main =
origin/main = c7debb6e). The dry run (built without rerere): merge-tree with exactly R43's two conflicted paths,
`resolve.py` resolving its three blocks, every one keep-both (theseus-core's `lib.rs` test `mod` lines, main's
`tests_route_keep` beside `tests_route_late` and `tests_route_measure` in name order; `telemetry/metrics.rs`'s methods,
main's `voice` then `route_wait`; `INSTRUMENTS`' length, main's 43 plus `ROUTE_WAIT` = 44, counted from the entries),
a rerun "already" 3 of 3; the tree 9cec008d; format 23; the core's 97 test mods in order; the names family 0. The
guarded take (01:11:20) took `cloud-b10-route-join`; `git merge` stopped on exactly the two content conflicts, which
the repo's rerere then replayed from R43's recorded resolution; `resolve.py` found nothing to do and still checked
`INSTRUMENTS` 44 of 44; five more files auto-merged (AGENTS.md, `judge/mod.rs`, `telemetry.rs`, `tests_route.rs`,
`turn/route_step.rs`); staged 18 files, +1,091 −98, the tree the dry run's (so R43's recorded resolution and resolve.py
agree), every file equal to b6346e9f's (lib.rs and AGENTS.md with main's own hunks). The signed merge **f81c0e62**
(c7debb6e and 5242d374), 01:11:26. The warm (01:11:41 to 01:15:51, the test build 2 m 55 s, clippy clean); R43's
filter **300 of 300** (theseus-core 159, theseus-judge 137, theseusd's judge 4).
- **The gate** (01:16:57, minute 16, to 01:28:45, ok): a 292 s lock wait behind R44's A/B bench for discord-watch,
  then 383 s held; 708 s in all. **3,121 of 3,121** (1 slow, 28 skipped: the last gate's 24 and the four measurements)
  in 326.4 s, about 01:22:22 to 01:27:49: the daemon-flakes gate's 3,113 plus 9 new tests less one renamed
  (`one_call_asks_three_packs…` to `two_calls_ask_three_packs…`); no flake red. **Lifecycle on its first run** (load
  reading 8.47), p50/p95 ms: cold start 30.4/43.6, the config copy 37.3/46.5, clean shutdown with a job running
  38.3/59.5, a post in flight 93.5/110.4, SIGKILL then restart 41.2/45.9, swap 58.9/68.5, restore 315.7/356.9, a
  cancel's round trip 30.5/51.8 (`cancel: 2 to 2 frames`); the daemon's own `serving` 27.93/34.82 ms over 51 starts.
  Jobs: L1 start 9.18/10.64 ms. Turn: frames 5 and 9, plain p50 86.5 ms, tool call 193.7 (load 8.24). With the judge
  off, as the gate runs it, the keeper returns at its first line.
- Pushed c7debb6e..f81c0e62 at 01:29:00 to 01:29:02, the branch deleted on origin, done line 01:29:17; theseus-ddbi
  closed with f81c0e62 (the chain log, 02:02: v1's speed item). The store stays at format 23.

**The install** (not installed yet: on install #10's list). On the owner's daemon, with the judge on: Jev's connections
open after serving and stay warm (two unbilled `HEAD`s just after serving and after each 150 s of Jev's silence: **about
1,150 unbilled warm-up requests a day**); route.v1 asks alone, so its verdict reaches the turn when its one question is
answered (a first message after a long silence goes from late, about 200 ms lost, to in time, and later waits fall from
125 to 86 ms on R43's stand-in at the owner's latencies), at one more Jev request a message (about 24 micro-dollars at
the owner's median state); `route.decided` says `late` and `answered_ms`, and `theseus.route.wait` counts live and
canary turns. `max_wait_ms` stays 200 until a week of those rows sets it. No config key.

**Divergences.** Way (b), not way (a): Jev answers no stream. The keeper sends its warm-ups day and night, as the brief
said ("keep it warm"), and R43 recommended keeping it so. The cloud session ran in two parts across the account's
session limit; the second redid step 2 from scratch, the first's never having been committed. The judge-on turn
bench's rerun with `theseus-index` beside both arms was queued behind a join's gate and stopped unrun.

**Known gaps.** theseus-nrcl and theseus-j84t (P3). The default bound waits for a week of live rows. Two of
theseus-core's AGENTS.md's new lines run past its 120-column wrap. m5-judgment.md's route.v1 row and FAST table, and
§3.20's metric list, are amended in this version (Part I).

### Item 238. The cockpit rethought, its prototype and the watch: the Ship draws the operator's units (a ship is a session, a bench a turn, an oar a tool call, its blade the result) and names every shape in words, and the watch, hardened, answers six questions, the sixth "Since you last looked", over `action.list`'s new `unsettled` option; with R45's join fix, the watch's "as of" line says the moment alone (theseus-hnof and theseus-hnof.3; the owner's ask of 2026-10-06 13:50 and the question walk's C1, C2 and C8; the cockpit-rethink lane's phase 1, 13:56 to 16:20, its prototype 81cd948d, 384cad58, 6edec63a, ec4b7abf and 7ceadae4, brought onto main 02de4b70 as the phase-2 base 7d3b87e8 by the DM thread at 22:48; cockpit phase 2's lane B, 22:55 to 2026-10-07 00:24, 747b4df1, cf59cd61, f7d2509e, bfd81d0f and 43280246; reviewed 00:34 to 01:53, and the join fix 02:00 to 02:09, by local reviewer R45; joined 02:37 at 47e92932, a signed merge onto f81c0e62, the first of the stack's two merges under one lock and one gate, by the cockpit B+A joiner; not installed yet: on install #10's list)

**Why.** The owner, 2026-10-06 13:50: "I want to re think the cockpit -- the data pages are amazing, the ship theme is
amazing, but the ship page isn't very understandable as to what the ships and oars are, and I think the entire thing
can be even more animated, exciting, deep, insightful and amazing, all while still delivering the specific data
insights in highly readable ways" (theseus-hnof). The cockpit-rethink lane's critique (main ceba1520 on a seeded
scratch daemon) found the complaint held: the Ship drew nodes ("198 lights"), not the operator's units; turns were
invisible (a session a flat string of lights, nothing saying which end is now); oars read as whiskers; failures did not
last; the key taught colours, not shapes; there was no depth and no insight on the Ship. Its phase-2 plan went to the
owner with eight questions at 16:32; the owner answered C1 to C8 between 22:37 and 22:51: keep the galley mapping
(C1); keep the watch's five plates and add a sixth, "Since you last looked", the last visit kept in the browser (C2);
add the daemon's filter for unsettled actions (C8); all seven steps, in parallel lanes (C7); and a standing rule for
the whole phase: "I'm trusting you to not lose data that the old cockpit had, and, making everything as most
awesomeness as possible". Step 0, an inventory, listed every datum the old cockpit showed (414 rows) and its home in
the new one.

**What landed** (the merge brings `lane/cockpit-p2-base`, the prototype, with lane B: 42 files, +6,560 −586, 35 under
`cockpit/`; Rust in theseus-protocol, theseus-core's `rpc/` and one line of the CLI's `cmd.rs`; one additive protocol
field, no package, config key or store format change (23 stays 23)).
- **The prototype** (phase 1, 30 cockpit files, +4,765 −507, no Rust and no protocol change):
  - **the Ship made legible** (81cd948d): a harbour ring is a place, a ship a session, a boat in tow a task, a **bench**
    across the deck a **turn** (oldest at the stern, newest at the bow), an ivory lamp a message, a violet lamp a model
    call, an **oar** a tool call and its **blade** the result (green ok, rose ✕ failed, amber waiting for you, a turning
    gear while a job runs), a hex shield a sandboxed call; nameplates, bench labels and blade tags say each in words; a
    key that lights what it names, a tour that flies to real examples, hover cards with the real data; a depth gauge for
    fleet, ship, turn and call; a bench opens its turn in the session deck (`?turn=`); "light" gone from the canvas;
  - **the watch** (384cad58): a column of five brass plates, Working now, Waiting for you, Slow, Spent today and Went
    wrong, each with its number, up to three lines, an overlay that lights its ships and oars, and a link to the page
    that explains it; the running turn's bench rows where its job runs;
  - the recording's fixes (6edec63a: harbours stay put as their ships grow, a sticky layout with a test; the sail folds
    away at a turn's depth; the lantern hangs on the stern) and the depth gauge reading any vessel whose keel is on the
    screen (7ceadae4);
  - **Economics and Speed by the chart method** (ec4b7abf): every chart with a table view, the palettes validated in
    dark and light;
  - motion only from the daemon's own events: a turn rows, an oar grows out, a result flashes back, a gear turns, a
    failure raises a pennant that stays on the bench, a waiting approval lights a lantern, a coin flies to "Spent
    today".
- **`action.list { unsettled: true }`** (747b4df1, C8): every action not settled, however old (planned, authorized or
  dispatched: a question waiting, a job running; or of unknown outcome, which a late result can still settle), newest
  first, read from the kernel's open actions (`Kernel::open_actions`, or an execution's by its own term), never every
  action, so it costs what is open; `n` caps it (default every one, at most 2,000), and `total` keeps its meaning, every
  action. `ActionListParams.unsettled` is absent from the bytes when off, so older peers keep theirs; protocol.gen's
  `ActionList{Params,Result}.ts` regenerated. The protocol's action types moved byte for byte from `lib.rs` into a new
  `actions.rs` (`ActionInfo.ts` unchanged), so `lib.rs` fell from 2,719 to 2,654 lines, under its ceiling without a
  raise.
- **The watch hardened** (cf59cd61, plan step 2). Went wrong counts each call once, in its most precise row's words: a
  job's lost wrapper, a job refused or stopped below the disk's floor, a job never started, an unknown outcome (unless
  a later result settled it), a failed call, a daemon crash, failed turns and spend limits; its ledger link filters
  every kind it counts. Slow holds each thing running against its own usual (the median of its kind in the day before
  the moment): a job against its own command's runs (numbers taken out), else its program's (`cargo`, `make`; a
  shell's builtins, wrappers and assignments passed over), else its tool's; a turn against the day's turns; past 3×
  and 5 s it is slow ("450×", its usual in the detail, the plate amber). Keys 1 to 5 toggle the overlays. A day of 23
  or 25 hours draws a bar an hour, each labelled by its local hour. Waiting for you names the sessions that hold web
  text in its caption and lines, not its number. The data hook reads `unsettled` beside the newest 500 and merges them
  (`mergeActions`: each call once, the settled read winning; an older daemon's answer left out).
- **The sixth plate, "Since you last looked"** (f7d2509e, C2; `since.ts`, `Watch.tsx`): its number the time you were
  away (from the end of your last look to the start of this one, so it does not grow while you watch); a tally of
  five small instruments (sessions that worked and how many are new; tasks finished and started; failed; questions you
  missed, asked and answered or expired while you were away, and how many still wait; dollars and calls), each a
  button that lights just its own on the Ship; three lines, one of each kind; show (key 6); its link opens the Ledger
  from that moment to this one; **replay** runs the time machine over the stretch in about twelve seconds, the busy
  minutes slower than the quiet hours, the tally counting up as the moment moves; **seen** quiets it until you are next
  away. The stretch is kept in the browser (`cockpit.watch.looked`): written every half minute while the Ship is in
  sight, five minutes or more away starts a new look, a first visit tells nothing yet.
- **Performance** (bfd81d0f): the lane's own first cut cost up to 4× the base's per recompute (418 ms against 92 at
  200,000 rows); each usual's median is worked out once, a row of no failing kind passed over first, and a walk back
  from a moment starts at it by a binary search (`endOf`). 43280246: the compact strip at 1366×768 stays above the depth
  gauge.
- **R45's join fix** (`B/joinfix.py`, the DM thread's decision of 01:10 on the inventory's stops N2 to N4): lane B had
  given the moment's profile, uptime and "down then" a home on the watch's "as of" line; they live only in the
  header's "then" (lane D's build), so the line and its tooltip say the moment alone, the dead `.watch-asof-more` style
  goes, and a new test reads `Watch.tsx` and holds it. Safe in any join order: until lane A's join the base's compass
  and chronometer still hold them, then lane A's cartouche until lane D's.

**How it is proven.**
- **The prototype** (phase 1): `tsc -b` and lint clean; `npm test` **119 of 119** (new `words.test.ts`,
  `ship-model.test.ts` 5 with the sticky layout's, which fails without its memory, `watch.test.ts` 9, `viz.test.ts`
  10); the scrub 0. Old against new on one scratch daemon, the fleet view, A B B A on SwiftShader (headless Chrome's
  CPU rasteriser, a floor): about 1 ms of main-thread time a frame for both. A 40 s recording found four bugs, all
  fixed.
- **Lane B:** the covering Rust tests 55 of 55 (3 new:
  `actions::tests::unsettled_is_off_unless_asked_and_absent_when_off` holding the bytes both ways;
  `unsettled_actions_stay_listed_however_many_calls_came_after`, a job dispatched before 600 later calls, a question and
  an unknown outcome out of the newest 500 and in the unsettled list, a settled job leaving it; and a randomized one, 40
  queries, against every action read and filtered); a planted revert of the handler failed both core tests; clippy `-D
  warnings` clean. The cockpit **131** (133 after main's merge), from 119: `watch.test.ts` 9 to 17, `since.test.ts` 4
  new. Live on a scratch daemon: after 500 later calls, the base said "Working now 0" while the lane's said 4 jobs, a
  fresh sandboxed one among them; a killed job wrapper was one failure on the lane's Went wrong, none on the base's.
- **The review** (R45, review commit 66f2dddc on c7debb6e, the join fix 081ebd68): fmt, clippy, theseus-protocol 33 of
  33, shape; the lane's selection 89 of 89; **the whole workspace suite 3,116 of 3,116**; the cockpit 133 of 133 with
  `tsc -b`, build and lint; the scrub clean on 6,555 added lines; **all 17 plants caught** (13 in the cockpit, 4 in the
  filter). **The filter:** every unsettled kind and nothing settled, at **0.036, 0.118 and 0.131 ms** with 1,000, 10,000
  and 100,000 settled actions, where a read of every action takes 7, 67 and 765 ms; no in-flight branch met the types'
  move (lanes A, C and D, the bench-reports and tool-tree lanes, and eight cloud branches checked). **Old and new, live,
  on one store:** the base cockpit on the installed daemon (02de4b70) and on the new one, 0 jobs working (the bug); the
  lane's cockpit on the installed daemon, 0 with every other plate whole (it degrades, never blank); on the new daemon,
  **3 jobs**. **The watch live:** a killed wrapper's two rows one failure; keys 1 to 6 and Esc; 25 bars on the day the
  clocks go back (the two 01:00s labelled) and 23 the day they go forward; the sixth plate's tally matched an
  independent count from the ledger exactly on a 1-minute and a 31-minute stretch; seen, the replay and a reload as
  built. **The join fix:** 134 of 134, five plants caught (the copy back, the moment gone, "down then" back, the profile
  in the tooltip, a stale prop by `tsc -b`); a dry run on f81c0e62 with lane A on top, 165 of 165.
- **FAST:** the Ship's main-thread time a frame unchanged (0.8 ms p50 both; the watch renders when its data change,
  the canvas untouched); the watch's pure work at 50,000 rows a day 27 ms against the base's 17, linear (lane B's
  bench). The daemon side costs what is open, on the poll the newest page already used (1 s while anything works, 5 s
  otherwise).

**What the review found.**
- **theseus-qilc (P2, FAST):** lane B's report quoted `watchOf` alone, but `sinceOf` runs on every recompute too: at
  50,000 rows a day the watch's recompute is a 20 to 50 ms main-thread task about once a second while the fleet works
  (the base's about 5); a paired CPU profile gives 433 ms against 90 per 15 s; a replay drops the Ship from 28 to 21
  frames a second. The fix: memoize the stretch, make the replay incremental; after the join, since at typical volumes
  it is a few milliseconds.
- **theseus-cov9 (P3):** Slow holds `sh -c 'echo …; sleep …'` against "`echo` jobs" ("41×"): pass over `echo` and
  `printf`. **theseus-ikwz (P3):** the tally's dollars cell cuts "$0.0066", the 1366 strip cuts "SINCE YOU LAST LO…"
  ("Since last look" fits), and a question answered from the CLI reads "approved by the CLI (cli)".
- **theseus-vej5** (lane B's, R45 recommends P2): a job whose wrapper died while the daemon was down (a WSL crash, an
  OOM kill) stays dispatched until its deadline, up to 6 h, so the watch, now reading every unsettled action, shows it
  working; the reaper hears only wrappers it adopted. The owner's service runs `KillMode=process`, so its restarts keep
  their jobs. **theseus-y3w8** (P3): the last look kept in the daemon, for phone and desktop (C2's "later").
- R45's and R46's small cockpit calls went to the owner as one bundled question (walk row F12, not yet answered).

**The join** (the stack's first; the lock, the dry run and the one gate are told in Item 239).
R45's dry run, rerun on f81c0e62: 165 of 165. The merge (onto f81c0e62, in the guarded take at 02:24:42): no conflict;
`B/joinfix.py` five "applied" (two in `Watch.tsx`, one in `watch.css`, two in `watch.test.ts`); staged 42 files, +6,560
−586, tree 62d09009, the dry run's R1; every one of lane B's 42 files byte for byte R45's 081ebd68 (main changed none
since c7debb6e); `--check`'s four hits ts-rs's trailing spaces in the two regenerated types and the prototype's own
blank line at `tourText.ts`'s end, kept as reviewed; the names family 0. The signed merge **47e92932** (f81c0e62 and
d5365612), 02:24:49. In the stack's checks: R45's Rust selection **89 of 89** (theseus-protocol 33 with the bytes
test, `rpc::tests_lists` 5 with both `unsettled` tests, theseusd's job tests 31), protocol.gen unchanged after; the
gate's suite gained exactly lane B's three Rust tests. Pushed with the stack at 02:36:42; `lane/cockpit-p2-watch` and
`lane/cockpit-rethink` (the prototype, wholly in main through this merge) deleted on origin; theseus-hnof.3 closed with
47e92932. The store stays at format 23.

**The install** (not installed yet: on install #10's list). The cockpit ships inside theseusd, so the owner's cockpit
gains, with lane A's (Item 239): the Ship of benches, oars and blades named in words; the watch with six plates, keys 1
to 6, the sixth "Since you last looked", its "as of" line saying the moment alone; a job running for hours never drops
off the Ship or the watch. `action.list`'s `unsettled` is additive: the new cockpit on an older daemon degrades to the
newest page, never blank (R45 ran old and new cockpits on the installed 02de4b70 and the new daemon). No config key.

**Divergences.** The prototype was never joined on its own: lane B's merge carried it (the plan's "simpler" way,
taking the branch as step 1's base). Holds joined Waiting for you in words, not in its number, since nothing is asked
yet. Lane B's home for N2 to N4 on the "as of" line, built before the DM thread's 01:10 decision, came out at the
join. lib.rs's ceiling in `scripts/long-files.txt` was left at 2,719, so the files stay the review commits'.

**Known gaps.** theseus-qilc (P2), cov9 and ikwz (P3), vej5, y3w8; the Ship's 60 fps on a real GPU (theseus-n2hd); the
bundled small calls (walk row F12). Lanes C (the chart method everywhere) and D (the repairs and a daylight mode)
joined after this stack, at their second take (Items 242 and 243).
Part I §3.18's `action.list` sentence and §3.14's cockpit are amended in this version.

### Item 239. The cockpit's Ship, hardened: every shape's words from one module, the console trimmed to the engine and tokens a minute, cards off the instruments, the tour with "what's new", a motion table nothing moves without, the living sea that is still when nothing happens, and sound off by default (theseus-hnof.2; the question walk's C1, C3, C4, C5 and C6, and the inventory's stops N1 to N5; cockpit phase 2's lane A, 2026-10-06 22:54 to 2026-10-07 00:44, c4e56b32, 12c9b83e, 20cf7055, 1e1470a3, 6617d514, b740fc9b, f4c7abc0, 56a9fc97, 5d1b5e26 and 7b5c6808; reviewed 01:05 to 01:56 by local reviewer R46; joined 02:37 at 3a1cf23d, a signed merge onto 47e92932, the second of the stack's two merges under one lock and one gate, by the cockpit B+A joiner; not installed yet: on install #10's list)

**Why.** Plan steps 1 (the Ship's language, hardened) and 3 (motion and sound), with the owner's answers of 22:37 to
22:45: keep the galley mapping as built (C1); retire the console's compass and chronometer, both repeating the top bar,
and keep the engine and tokens a minute (C3); build sound, off by default, one toggle, three cues on real protocol
events: "an oar out (a soft oar splash), something waiting for you (a ship's bell), a failure (a low horn)" (C4); "the
sea carries information": dead calm when nothing happens, the render loop stopped, the swell rising with real work and
settling as it ends (C5, which retires Item 143's gentle roll, always); the tour on a first visit plus a "what's new"
tour after an update (C6); and the standing rule that no datum the old cockpit showed is lost. The inventory gave lane
A five stops with no home on the base: a session's message count (N1), the profile, uptime and "down then" at the time
machine's moment, which the compass and chronometer held (N2 to N4), and what the gold planks mean (N5).

**What landed** (30 files, all under `cockpit/`, +1,662 −398 at the merge, 14 new: `audio.ts`, `instrumentRects.ts`,
`motion.ts`, `news.ts`, `placement.ts`, `sea.ts`, `ship.css`, `sound.ts` and `useShipSound.ts` in `src/ship/`, and five
tests; no Rust, protocol type, package, config key or store format change (23 stays 23)).
- **The Ship's words from one module** (c4e56b32). `words.ts` holds every shape's plain word, noun and sea word
  (`SHAPES`), the bench labels and states, the nameplates', harbours' and oars' lines, and money; the key, labels,
  hover cards and harbours read it, and a test holds the key to `SHAPES`. The keel's tags say an author or a model only
  where it changes along a ship (it had read "you, from the CLI" eight times and "sonnet-5-5" fifteen; each stays in its
  hover card). The session deck says who wrote a message in the same words ("YOU, FROM THE CLI", "TASK A3380C") with the
  raw label beside it (`sock#45`).
- **The console** (12c9b83e, C3): the engine (in words: "accepting · 3 of 8", "· 1 held") and tokens a minute; the
  compass and chronometer retire, and at the time machine's moment the cartouche says what the top bar said then
  ("then: live profile sonnet (claude-sonnet-5-5) · up 48s", or "the daemon was down" in rose): N2 to N4's home until
  lane D's header. The unused compass, chronometer, gate and fuel gauges leave `instruments.tsx`; the dial kit stays
  for Speed and Money.
- **Cards and labels off the instruments** (20cf7055): `placement.ts` (pure, tested) puts a card beside its point,
  inside the Ship, and off the watch, the console, the key, the depth gauge, the porthole and the vessel card (an oar's
  card had slid under the console with only its first line showing); at 1366×768, four of six places named, against two.
- **The tour as built, plus "what's new"** (1e1470a3, C6): `news.ts` (pure, tested) holds one item per addition with
  its date; after an update the new things get a few skippable stops, once, keyed by the cockpit's version, which the
  browser keeps.
- **Every datum the old Ship showed has a home** (6617d514): the vessel card counts messages again ("19 messages · 33
  model calls · 25 tool calls, 3 failed", N1); the key's lines for the gold planks ("turns in the last hour · gold
  planks", N5) and the chain along the rail ("holds web text") come back; the raw state and attention label are back on
  the card; the key takes the height under the vessel card and scrolls when it must.
- **The motion table** (b740fc9b): `motion.ts` lists 19 motions, each with the one event that starts it, what moves,
  how long and its pace; the engine draws only from it: every display frame for a one-off, a steady 30 a second for a
  state that moves while it lasts, 15 for the sea alone, none otherwise. Every time-driven shader term names its row,
  and a test reads the shaders. What moved with no event is still: the waiting lamp's swing, the lantern's breathing (it
  swells once as it lights, then stands lit) and the pennant's flutter. Calm stills everything.
- **The living sea** (f4c7abc0, C5): the swell's height from `seaTarget(tokens a minute, turns running)` (`sea.ts`,
  pure, tested) eases up in seconds, settles slower, and reaches dead calm at exactly 0, when nothing is drawn for it;
  the shader's harmonics, the rows' rise and the crests' light grow with it; a sea gauge on the console says it in words
  ("THE SEA · a moderate swell"), the full height at 60,000 tokens a minute.
- **Sound** (56a9fc97, 5d1b5e26; C4): off by default, one toggle (Sound, beside Live); `sound.ts`'s cue table maps
  three cues to the daemon's own pushes, and `audio.ts` synthesizes them with Web Audio (no recording, no third-party
  asset; peaks −23 to −18 dBFS): **an oar out** (`tool.started`, at most one splash per 220 ms: a falling sine knock,
  band-passed noise and two rising drops), **something waiting for you** (`confirm.requested`, an execution coming to
  need you, and the ledger's `tool.confirm_requested` and `budget.asked`: a ship's bell struck twice, its inharmonic
  partials on E5), **a failure** (`turn.failed`, an execution failing or over its budget, and the ledger's rows: a low
  horn of three detuned saws through an opening low-pass). A live check found the horn silent for a turn failing on an
  unpriced model (that failure reaches only the session's watchers), so the horn and the bell also hear the ledger's
  rows, once with the push. A failed tool call sounds nothing; turning sound on rings the bell once.
- `cockpit/AGENTS.md` (7b5c6808): the new modules, and the invariants for the motion table, the living sea ("an idle
  Ship draws no frame"), sound and what's new.

**How it is proven.**
- **The lane:** `npm test` **152 of 152** (the base 119; the lane's 31, main's 2): `motion.test.ts` 6 (the shaders'
  audit among them, and an idle fleet drawn once, then nothing), `sea.test.ts` 4 (dead calm exactly 0), `sound.test.ts`
  6 (off by default, the splash spacing, the bell and the horn once, the ledger rows, each cue's source), `news.test.ts`
  5, `placement.test.ts` 5, and `words.test.ts` +6; `tsc -b`, lint (the base's 25 warnings) and the build green. Every
  datum of lane A's rows seen with a screenshot. Recordings of four real events driven through a scratch daemon's CLI:
  a turn with two calls (the sea rose to 0.34 and settled; one splash), an approval (the lantern lit once and stood;
  the bell, then the oar), a turn on an unpriced model (the horn from the ledger row; **no frame from 8 s to 15.5 s,
  idle**), and a stop of a sandboxed job.
- **The review** (R46, review commit 328742de on c7debb6e, tagged `r46-ship`): every commit against its claim, holding;
  152 of 152, `tsc -b`, lint and build; the stack B + A + D with both lane fixes, 180 of 180; the scrub of 6,285 added
  lines 0. **Plants:** 12 against `npm test`, 9 caught (a shader term with no tag, a tag naming no row, Calm stilling
  nothing, the sea never at 0, the swell deaf to running turns, sound on by default, "what's new" repeating, the key's
  own words, the keel tagging every light); 3 not (a question's bell twice; a horn for a failed tool call; the card
  dropping its message count, which no view test holds): theseus-n7ra. **Every inventory row of lane A seen live**: the
  card's and the hover card's message counts agreed for all 12 vessels; the cartouche's "then" line up and inside a
  20 s stop made for it. **The cues live**, each a real event from the CLI: an fs.write waiting, the bell 1.2 s after; a
  turn on an unpriced model, the horn 1.0 s after; a failing `proc.run`, no oar and no horn, as designed.
- **FAST** (SwiftShader, a floor; the base 7d3b87e8 against the review tree in palindrome order, the whole Chrome's CPU
  from `/proc`): first fleet frame and main-thread time a frame no worse (the lane a little better in each pair: first
  frame 1,311 against 1,327 ms; 0.6/1.6 ms a frame against 0.8/1.9). **Idle in Live mode with questions waiting: the
  base 27.8 and 31.7 frames a second (4.57 and 4.83 cores), the lane 0.07 frames a second and 0.97 cores; over 95 s
  idle, 4 frames.** A long job asks for 30 a second (13.1 on SwiftShader at 3.56 cores). The owner's GPU is not yet
  measured (theseus-n2hd).

**What the review found.**
- **theseus-wviw (P3):** the oar cue hears only `tool.started` for sessions the Ship watches, so a quick turn in an idle
  session makes no splash; hear the ledger's `action.dispatched` rows too. **theseus-cbce (P3):** "what's new" is keyed
  by a date, so two updates on one day share a version. **theseus-h70f (P3):** with nothing running the daemon's
  `executions_by_state` has no `running` key, so the engine counts working ships instead ("accepting · 2 of 8" beside
  the header's "RUNNING 0"). **theseus-n7ra (P3):** the three uncaught plants.
- Lane A's own: **theseus-1skt (P2)**, the flare and "failed" motion for a turn failing in a session the Ship does not
  watch (the horn covers it); **theseus-fdvz (P3)**, a stopped job's gear turns on for 1 to 5 s until `action.list` is
  read again; **theseus-7zph**, sound on every page, the owner's question (walk row F9, unanswered; R46 recommends every
  page, the hook mounted in the Shell after lane D's join).
- R46's "for the owner", with the bundled calls (walk row F12): keep no horn for a failed tool call (pinned in a test),
  keep the lantern still after one swell, drop a lasting state from 30 to about 10 frames a second after a minute with
  no event, keep the sea gauge, and still the header's endless CSS sweep and pulsing dots (about 0.7 of a SwiftShader
  core at idle). The sea at rest still draws its lines, so a still frame cannot tell dead calm from a light swell; the
  key's second column clips at 268 px.
- **The overlap of N2 to N4** (both lanes A and B had built a home): the DM thread decided at 01:10 that they live once,
  in lane D's header "then"; R46's `review-A/joinfix.py` removes the cartouche's line **at lane D's join**, not lane
  A's, since in the order B, A, C, D the cartouche is their only home from A's join until D's.

**The join** (lanes B then A under one stack lock and one gate; the joiner 02:11 to 02:43). At 02:12 main =
origin/main = f81c0e62, only review locks open. The dry run (linuxbrew git, never rerere): lane B clean with its join
fix (R1 62d09009); lane A on R1 conflicted in `cockpit/AGENTS.md` alone (a criss-cross: both lanes carry the prototype
base and a merge of fad08f32, and ort merged the two bases first); `review-A/resolve.py` (lane A's list of the Ship's
parts, then lane B's watch bullet) and R45's `resolve-ab.py` gave the same file byte for byte; R45's `joinfix-ab.py`
applied 2 of 2 (R2 5201cbcd); the review stack (R45's 081ebd68 and R46's 328742de merged on their own base with the
same scripts) ecbb80fe; every file against its review commit; format 23; the scrub 0 on both merges. The guarded take
(02:24:42, lock `cockpit-p2-ba-join`) made merge 1 (Item 238), then merge 2: one CONFLICT line,
`cockpit/AGENTS.md`; `resolve.py` resolved it and rerere recorded it; `joinfix-ab.py` ("the tour's watch stop names six
plates" and "what's new gains the sixth plate"); staged 30 files, +1,662 −398, tree 5201cbcd, the dry run's R2, lane
A's other 27 files byte for byte 328742de's. R46's `joinfix.py` was not run: it belongs to lane D's join. The signed
merge **3a1cf23d** (47e92932 and a6258c2b), 02:24:51.
- **Before the gate:** the warm (02:25:07 to 02:29:20, the test build 2 m 41 s, clippy clean); the cockpit in main's
  tree after `npm ci --offline`: **`npm test` 165 of 165**, `tsc -b` 0 errors, lint the base's 25, the build (which
  refreshed `crates/theseusd/cockpit/dist`); R45's Rust selection **89 of 89**.
- **The gate** (02:30:04, minute 30, to 02:36:25, ok; a 1 s lock wait, 358 s held, 381 s in all): **3,124 of 3,124**
  (1 slow, 28 skipped) in 305.5 s, about 02:30:27 to 02:35:34: route-wait's 3,121 plus lane B's three Rust tests (lane
  A adds none); no flake red. **Lifecycle on its first run** (load reading 3.75), p50/p95 ms: cold start 26.9/42.2, the
  config copy 26.7/29.4, clean shutdown with a job running 34.5/44.4, a post in flight 87.7/90.8, SIGKILL then restart
  33.3/36.8, swap 52.1/63.2, restore 241.6/272.9, a cancel's round trip 26.1/31.4 (`cancel: 2 to 2 frames`); the
  daemon's own `serving` 21.63/30.16 ms. Jobs: L1 start 7.50/8.73 ms. Turn: frames 5 and 9, plain p50 75.0 ms, tool
  call 161.3, resident memory 69.2 MB after the start and 92.6 MB after 30 turns. Every row faster than route-wait's
  gate at less than half the load: the quiet machine, not the stack (lane B's only daemon change is a read off the
  start and turn paths; lane A changes no Rust).
- **Pushed** f81c0e62..3a1cf23d at 02:36:42; `lane/cockpit-p2-watch`, `lane/cockpit-p2-ship` and `lane/cockpit-rethink`
  deleted on origin, each an ancestor of main; done line 02:37:02; theseus-hnof.3 closed at 47e92932 and theseus-hnof.2
  at 3a1cf23d. rerere now knows the AGENTS.md resolution. The store stays at format 23.

**The install** (not installed yet: on install #10's list). The owner's cockpit, inside theseusd: the new Ship (words on
every shape from one module, the keel naming an author or model only where it changes, cards off the instruments, the
tour as built with "what's new", whose stops include the watch's sixth plate); **the living sea, so an idle Ship draws
nothing** (R46: 4 frames in 95 s idle with questions waiting, against the base's 28 to 32 a second); a motion table
naming every motion; **sound off by default behind one button**, three cues made in the browser, on the Ship page only
for now (theseus-7zph); the console trimmed to the engine and tokens a minute with a sea gauge in words; the moment's
profile, uptime and "down then" on the cartouche until lane D's join moves them to the header. A long job keeps the Ship
at 30 frames a second while it runs. No config key.

**Divergences.** The console has three instruments, not C3's two: the sea gauge is the lane's addition, to say C5's sea
in words (R46: keep it). The bell rings once when sound is turned on, the one sound not set off by the daemon.
`cockpit/AGENTS.md`'s "an idle Ship draws no frame at all" overstates: it draws a frame when its data refresh, about
one each 25 s. The sea's scale was set on the stand-in model (50 to 600 tokens a minute); the owner's real models run
thousands to a hundred thousand.

**Known gaps.** theseus-1skt (P2), fdvz, wviw, n7ra, cbce, h70f (P3); theseus-7zph and the bundled calls (the owner's,
walk rows F9 and F12); the GPU measurement (theseus-n2hd). R46's `review-A/joinfix.py` ran at lane D's join (Item
243), which moved the moment's profile, uptime and "down then" to the header. Item 143's "the
sea rolls gently, always" and Item 64's console (compass, gate, fuel, chronometer) are superseded, noted where they
stand, and Part I §3.14 is amended (Part I).

### Item 240. Discord watch: the bindings watch acts on no half-written save, tries a failed bind again each tick until it binds, keeps the DMs in the file's order, and measures the board's note from the start's file (theseus-u6v6, theseus-btt4, theseus-sn2z, theseus-nz3q, theseus-02bq and theseus-88cp; local reviewer R23's findings on discord-live, Item 207; the tenth cloud batch's discord-watch session, fired 2026-10-06 12:43 from d279767f, Sonnet 5.5, paused at the account's session limit at 14:53 with nothing pushed, relaunched by the DM thread at 18:05 and fired 18:16, its report at 19:41; 64142406, 686b6d5c, 9eba07be, e8427616, b2e5921a, f0faceac, 6731c667 and 943637f5; reviewed 2026-10-07 00:09 to 01:52 by local reviewer R44, the first of its two, and accepted; joined 03:09 at 80243147, a signed merge onto 3a1cf23d, the first of the dwbr stack's two merges under one lock and one gate, by the C+D and dwbr joiner; not installed yet: on install #10's list)

**Why.** discord-live (Item 207, theseus-ocwt) made the binding re-read its bindings file while the daemon runs: a stat
of its mtime, size and inode every 2 s, read and parsed only when the stamp moves, then the places diffed and bound,
unbound or updated in place. Its review left six issues: a place added live whose session could not be opened was
never retried, since the next change diffed it as bound (**u6v6**); the board's "waits for the next start" note was
dropped at the next unrelated change, since it compared the last two files, not the started one (**btt4**); a save
caught torn at a table boundary was acted on, so the places past the tear unbound until the next tick (**sn2z**); a
`[[dm]]` put back live went last in the DM routes, so approvals and operator notices went to another owner's DM until
a restart (**nz3q**); and no test held a changed place updated in place (**02bq**) or the operator notice's fallback
refusing a retired lane (**88cp**).

**What landed** (theseus-discord's `runtime/live.rs` 555 to 750 lines, `tests_live.rs` 301 to 637, a `#[cfg(test)]`
seam in `rpc_client.rs` and a test-only re-export in `runtime.rs` (3,467 of its 3,500 ceiling), and the crate's
AGENTS.md; the merge 5 files, +592 −31; nothing on the start path or a turn's path; no package, protocol type, config
key or store format change (23 stays 23)).
- **A torn save is not acted on** (9eba07be, sn2z): a stamp (mtime, size, inode) is acted on only when the tick before
  saw the same stamp (held) and its mtime is at least a period old (settled; an mtime ahead of the clock counts as
  settled); a stamp acted on is not read again; the watch's first tick only records the stamp. Two same-size writes in
  one mtime tick are read once they age. The price: a change binds within two periods (about 2 to 4 s), not one. The
  limit: a writer paused longer than a period mid-save still tears.
- **A failed bind is retried** (64142406, u6v6): failed keys live in `failed` (key to why), outside the diff, keeping
  their lanes; each tick `retry` binds them against the file bound now, and a key the file drops, or that binds, leaves.
  The failure is said once (`bound_or_failed` writes one `discord.error` row, "bind place"), and the board's detail says
  "a place did not bind, and is tried again every 2 s: <key>: <why>" meanwhile; a retried DM lands once in the routes
  (`add_dm`); a place that binds late has its viewers read (`check_privates`).
- **The note from the start's file** (686b6d5c, btt4): `waits(&started, &bound)`, computed at each change, each retry
  and each same-revision read; `apply` no longer returns it.
- **The DMs in the file's order** (e8427616, nz3q): `order_dms` rebuilds the DM routes in the file's order from the DMs
  bound now, after `apply` and after a retry, under the routes' lock alone.
- **Tests for a changed place and a retired lane** (b2e5921a and 943637f5, 02bq; f0faceac, 88cp): a changed place keeps
  its turn and its messages (the messages first posted keep their ids, none repeats, and the tool line is edited to its
  end; 943637f5 made the test wait for the live edit, which lands after the posts settle, after the first version
  failed 5 of 5 under load); an operator notice falling back to a retired lane is refused with "is not one this daemon's
  bindings file names". 6731c667: live.rs's header and the crate's AGENTS.md say the retry, the settling, the DM order
  and the start-measured note.

**How it is proven.**
- **The session:** six tests, one an issue, each caught by its plant (the retry switched off: the refused place never
  binds; the note compared with the last file; any moved stamp acted on: a place past the tear gone from health;
  `order_dms` a no-op; a changed place unbound and bound again; `binds` naming a retired lane); `tests_live`'s 9 tests
  five times under load (nice 19 beside four busy loops), 5 of 5. Its gate failed only on the cloud VM's 33 known L1
  tests.
- **The review** (R44, review merge 22b1368b on fad08f32; in R41's warm tree): fmt, clippy `-D warnings`, shape,
  theseus-protocol 32 with protocol.gen unchanged; **theseus-discord whole, 168 of 168**; `tests_live` under load (5
  iterations at nice 19 beside 16 busy loops, load 38), **45 of 45** by the Summary; the whole workspace suite on the
  stack (with bench-rows) 3,122 of 3,124, neither red theseus-discord's (bench-rows' new test, which its join fix mends,
  Item 241; and main's term test, theseus-fps6). **Plants:** the report's six caught; of R44's four, the
  retry's note clearing caught, and three passing every test: `settled` dropped (a held stamp acted on however young),
  `acted` dropped (the file read and parsed every tick once settled), and a DM bound by a retry going last: filed as
  theseus-stks. R44 read the rule beyond the report: `settled` matters when two stats come close together (`retry` runs
  before the stat inside a tick, and a late tick fires at once); a retry costs a `session.open` to the in-process core
  and, for a DM not yet opened, one Discord REST call, well under a millisecond every 2 s.
- **Live** (R44, scratch daemons of main fad08f32 and the merge on the discord rig's stand-ins, a file binding a private
  channel, two owners' DMs and two more channels): a save written in place and fsynced up to a table boundary, held
  **1.0 s**, then the rest, with health read every 0.15 s: **main lost the two places past the tear in 3 of 6 trials**
  (each for 1.7 to 1.9 s, with two "bindings file changed" lines), **the merge in 0 of 6**, nothing refused; held 5.0 s
  (the stated limit), both lost them, the merge refusing nothing. **Latency**, a channel added then removed four times
  each, the save to health: main 0.91 to 1.7 s, the merge **2.91 to 3.02 s**, at one tick phase each (main 0 to 2 s,
  the merge 2 to 4). **btt4:** a voice channel added (named), another place's users changed (main dropped the note,
  the merge kept it), the voice channel removed (main named it again wrongly, the merge cleared it). nz3q was not run
  live (the rig's policy gives a shared place no call that asks); its test and plant hold it.
- **FAST:** the watch is spawned in `serve` after the places bind, a tick is one stat plus a retry only while a bind
  has failed, and the first tick now only records the stamp where main's read and parsed the file. One window, main
  against the merge (turn A B B A A B B A, 30 runs a kind; lifecycle A B B A): every turn and lifecycle row within its
  blocks' spread (Mann-Whitney p 0.67 to 1.0), frames 5 and 9 throughout. The lifecycle bench does run the watch: its
  binding binds a DM on the bench's fake Discord (theseus-l21m).

**What the review found.**
- **theseus-stks (P3):** the three unheld halves (`settled`, one stat a tick, the retry's DM order) and a retry that
  binds overwriting the board's "the bindings file does not load" note while the file still does not load. R44
  recommends a pure decision function with unit tests and one `tests_live` test for a retried DM.
- Two edges: a `[[dm]]` whose bind keeps failing while Discord also refuses to open its DM channel writes an "open DM
  channel" error row on every retry; and `RpcClient::call` has no timeout, so a `session.open` that never answered would
  stop the watch (as `apply` already could on main; the retry only calls it more often).
- **For the owner** (adopted by the DM thread, the chain log 02:33): accept the extra period of latency; **do not make
  removals wait longer than additions** (the report's question): TOML is line-based, so a tear at any line inside a
  table also changes a place, its missing keys falling to their defaults (an absent `private` takes the guild's word, a
  cut `[channel.ceiling]` loses its limits), and such a tear can widen a place, while a removal is the direction that
  fails closed; accept the retry every 2 s, said once. The docs should say to save by rename or in one write, and
  `bindings.example.toml`'s header ("Read at startup; restart theseusd after editing") is stale since theseus-ocwt.

**The join** (the dwbr stack's first; the lock, the dry run and the one gate are told in Item 241).
The merge onto 3a1cf23d (02:56:42): `AGENTS.md` and `runtime.rs` auto-merged (main's discord-tests wraps the watch's
unchanged call in its stop `select!`, so the new watch ends at the daemon's stop too), no conflict, no resolve.py, no
join fix; the cloud files removed; staged 5 files, +592 −31, tree 751a4929, the dry run's, its 5 files R44's 22b1368b
byte for byte. The signed merge **80243147** (3a1cf23d and 53edf412). In the stack's tests before the gate,
theseus-discord's whole suite, 168; the gate's suite gained its six `tests_live` tests. Pushed with the stack at
03:08:56, the branch deleted on origin, done line 03:09:18; theseus-u6v6, btt4, sn2z, nz3q, 02bq and 88cp closed with
80243147. The store stays at format 23.

**The install** (not installed yet: on install #10's list). On the owner's daemon, a bindings save caught half written
no longer drops a place (R44, live: 0 of 6 trials against main's 3 of 6), and a change binds within about 3 s of the
save (2.9 to 3.0 s against main's 0.9 to 1.7; within two periods, about 2 to 4 s); a place whose bind failed is tried
again each tick until it binds, and said once; the DMs keep the bindings file's order across live changes; the board's
"waits for the next start" note holds across unrelated changes. No config key.

**Divergences.** The removals-wait-longer question was left open by the brief and answered no. The cloud session ran
in two parts across the account's session limit; the first had pushed nothing, so the second started over.

**Known gaps.** theseus-stks (P3). `bindings.example.toml`'s header and docs/status.md's "binds or unbinds within 2 s"
are owed (the second becomes "within two periods, about 2 to 4 s"). Part I §3.9's account of the live bindings file
is amended in this version.

### Item 241. Bench rows: the turn bench names each run's slowest frame, the turn's trace carries it, and the lifecycle bench's restore row runs before the cancel row again; with R44's join fix, the restore-order test holds what tells the two orders apart (theseus-w7dk and theseus-ma8r; the voice-echo joiner's and the B9-core joiner's findings, Items 215 and 214; the eleventh cloud batch's bench-rows session, fired 2026-10-06 18:10 from 57f265f2, Sonnet 5.5, its report at 19:51; fd1f578f and 5cb3735f; reviewed 2026-10-07 00:43 to 02:13 by local reviewer R44, the second of its two, and accepted with a join fix; joined 03:09 at 6c05334d, a signed merge onto 80243147, the last of the dwbr stack's two merges under one lock and one gate, by the C+D and dwbr joiner; not installed yet: on install #10's list)

**Why.**
- **theseus-w7dk (P3, FAST):** the gate's turn bench (10 runs of each kind) showed one run far slower than the rest
  (a p95 more than twice its p50) in 17 of 191 gates since 10-02, twice in a row at voice-echo's join (Item 215), and
  printed summaries only: the daemon kept no time per frame, and the WAL holds none. The issue asked each run's wall
  and slowest frame printed, to name the cause.
- **theseus-ma8r (P3):** daemon-proofs' `cancel` row (Item 214) ran before the restore row on the same rig, and its
  runs start jobs through turns, so from d279767f the restore row restored the cancel row's sessions (15 sessions and
  0.85 MB of WAL, against 5 and 0.22 MB) and its unbudgeted p50 moved from 139 to 233 ms.

**What landed** (theseus-store's new `frame_times.rs` and its writer, theseus-core's `store.rs` and `turn.rs` with the
core golden, theseus-sim's new `perf/runs.rs`, `perf.rs`, `lifecycle.rs` and `tests/sim.rs`; the merge 11 files, +456
−28, with R44's join fix; no package, protocol type or config key; no store format change (23 stays 23): `Span.attrs`
is a free-form JSON value inside the stored `TurnEnded`, so a new key adds no field to a stored type).
- **The writer times its batches** (fd1f578f). The store's writer reads its clock at the top of a batch (before any
  manifest upgrade) and again after the index's transaction, and notes each answered frame (its first position, the
  batch's microseconds, the answer's instant) in a 64-entry ring under a mutex, before answering it; a frame's time is
  its batch's (the writes, the one `fdatasync` and redb's index commit). `WalStore::slowest_frame_since(Instant)` reads
  it. A test-only delay (`Inner::commit_delay_ms`) sleeps once inside the timed stretch.
- **The turn's trace carries it.** `attrs.slowest_frame = {first, us}`: the slowest frame answered since the turn's
  input arrived (`Turn::arrived`, the request's own instant, before admission), so admission's frames count, and a
  neighbour session's frames too, which is what a stall is. The root span's closing attributes moved into
  `TurnRunner::end_attrs` (`finish` sat at clippy's 100-line limit); turn.rs went to 3,509 of its 3,523. The core golden
  gains exactly that key in 17 lines. Telemetry flattens a span's attrs, so each turn's exported root span gains two
  OTLP attributes, `slowest_frame.first` and `slowest_frame.us` (R44's reading; 2 of the span's 128-attribute cap).
- **Each run's line** (`perf/runs.rs`): under each kind, a line per run with its wall, the daemon's time, its frames,
  and its slowest frame's time and records; a run over twice its kind's p50 is flagged `<- over 2x the p50`; `--json`
  carries the same; the frame-count check and the history's columns are unchanged. An older daemon's run reads
  "slowest frame not named". The report's reading guide: a large slowest frame is a stalled frame (the sync or the
  index commit), named by its records; a large wall less daemon with a frame about that size is an admission frame.
- **Restore before cancel** (5cb3735f, ma8r): `run` runs the restore phase before the cancel phase, and the module doc
  gains an **Order** paragraph: a phase that writes into the rig runs after the rows that read it. The new
  `tests/sim.rs::the_restore_rows_store_is_the_same_with_the_cancel_row_selected` runs the real bench twice
  (`--phases restore` and `--phases restore,cancel --runs 1`).
- **R44's join fix** (`bench-rows/joinfix.py`, sim.rs): the test's bounds hold what tells the orders apart (the same
  segments, sessions within one, and each run's WAL under twice the other's), not equal sessions within 25 %.

**How it is proven.**
- **The session:** theseus-store's two frame_times tests (the slowest frame since a mark is the one made slow; the ring
  forgets the oldest), theseus-sim's four `runs::` tests; plants: `slowest_since` taking the first frame, and the old
  phase order (the restore row's WAL 135,855 B and 3 sessions against 6,243 B and 0), each failing. Measured on the
  cloud VM (4 cores, quiet, 30 runs): plain p50 39.4 ms, no outlier; tool-call p50 95.3 ms with two outliers, each one
  stalled frame of 122.8 and 182.2 ms (the second the admission frame, `[ledger:execution.queued, execution,
  ledger:execution.running]`, before the daemon's clock starts, so wall less daemon was 195 ms); on that disk a frame
  typically takes about 1.3 ms, so these were sync or commit stalls of 100 times the median; under a gentler load, 3 of
  3 outliers were one slow frame. The restore row before and after: 0.84 MB and 15 sessions (p50 55.7 and 53.0 ms on
  the VM) against 0.20 MB and 5 (27.6 and 23.6). A before-and-after A B B A A B B A of the turn bench: +0.9 ms plain and
  +4.8 ms tool-call against an A-against-A control of 1.4 and 2.5, within the VM's noise; frames 5 and 9 throughout.
- **The review** (R44, review merge 3d69caf6 on discord-watch's 22b1368b, the join fix 71ab1ed6, tagged `r44-dwbr`):
  fmt, clippy `-D warnings`, shape, protocol 32 with protocol.gen unchanged; the branch's tests and the code they meet
  **247 of 247** (the golden at TZ=America/Phoenix); the frame_times tests 20 times at nice 19 beside a writer syncing
  64 MB at a time on the same disk, **40 of 40**; the golden's 17 lines each its old line once the key is taken out,
  checked by script. **The whole suite on the stack 3,122 of 3,124:** this branch's new test (left 0 sessions alone,
  right 1 with the cancel row; the join fix's flake) and theseus-fps6 (main's). **The flake's cause:** since
  theseus-l21m the lifecycle bench's binding binds a DM on its fake Discord once the fake `op` answers, so an unmeasured
  warm-up start that outlives that answer, or a bind cut off by its start's stop, leaves 0 to 2 bench sessions in a
  run's store, by the machine's load (reproduced 1 of 10 under 16 busy loops, the other way round); a first fix (`cold`
  added to both runs) went 3 of 10 and was dropped; the join fix passed **10 of 10** under load, and the old order still
  fails it ("0 sessions alone and 2 with the cancel row selected"). **Plants:** the report's two caught; R44's two not
  (the batch's clock started after the write, sync and index; the trace's slowest frame taken from the turn's start,
  missing admission): theseus-img9.
- **ma8r with cancel-fast** (R44, each arm's own `theseus-sim` against the stack's daemon): main's order restored 0.81
  MB and 15 sessions (p50 243 and 358 ms), the merge's **0.19 MB and 5 sessions (147 and 220 ms)**, the cancel row "2 to
  2 frames a cancel" in both.
- **FAST:** two `Instant::now()` a batch, an uncontended lock and a ring write a frame, and one lock a turn, against a
  sync whose p50 is 6.8 to 7.7 ms. A three-arm window (main, discord-watch, the stack; noisy once a join began building)
  and a quiet B-against-C window (turn bench, 4 blocks each): plain p50 79.1 against 85.8 ms (p 0.34), tool-call 189.2
  against 181.2 (p 0.69), opposite ways, frames 5 and 9 throughout: **no cost shows**. Live, the stack's daemon named
  each run's slowest frame, 12.9 to 15.5 ms (about twice the raw sync), one of them the admission frame; discord-watch's
  daemon printed "slowest frame not named" and flagged three runs over twice the p50 that the stack would have named.

**What the review found.**
- **The last frame is never named** (theseus-67nz, P3): `end_turn`'s frame, which carries the trace, is written after
  the trace takes `slowest_frame`, and the turn answers only after it, so a stall there prints as a long wall, a normal
  daemon time and a small slowest frame, which the report's reading guide sent to "outside the frames": 1 in 5 of a
  plain turn's stalls and 1 in 9 of a tool-call turn's, if stalls fall at random. Fix the guide; R44 would name the
  frame through a read-only health field the bench reads after each run, touching no stored type.
- **theseus-img9 (P3):** the two claims no test holds (a stalled sync inside the timed stretch; an admission frame by
  position). **theseus-2x5y (P3):** the report's own finding: under load a memory pass's frame (`ledger:memory.*` rows
  only) lands in a measured turn's window and fails the whole bench run's frame-count check (4 of 4 loaded tool-call
  runs on the cloud VM); R44 recommends leaving memory-only frames out of the count, bench-side, as theseus-kq4n's
  minimum-frames check for the cancel row would.
- **theseus-fps6 (P2, main's):** `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` failed once ("Ctrl-C did not
  interrupt the sleep"); its `marked("4242")` scans all of `/proc` for any `sleep 4242`, so the same test running in
  another tree at the same moment fails it: a negative assertion, on a machine that runs suites side by side.
- **For the owner:** accept the writer's cost and re-read the turn p50s at the first gates after the join; keep the two
  OTLP attributes and name them in the spec's telemetry section; write a note in the bench history at the join, since
  the restore row moves back about 90 ms.

**The join** (the dwbr stack, the second job of the C+D and dwbr joiner, 2026-10-07 02:33 to 03:13; merge 1 is Item
240). **Job 1 first:** cockpit phase 2's lanes C then D (lock `cockpit-p2-cd-join`, on 3a1cf23d)
merged exactly as R47 reviewed them (951b7ee3 and ce61f0cc, every file R47's byte for byte), the cockpit passed 198 of
198 in main's tree, and its one gate (02:48:34 to 02:54:02) **failed in its suite, 3,123 of 3,124**: theseusd's
`job_wrapper::a_double_fork_stays_under_its_wrapper_which_lingers_until_it_ends`, "timed out waiting for the lingering
mark" (20 s, `job_wrapper.rs:122`), a Rust test the cockpit-only stack does not reach, red in no earlier gate log. The
brief allowed one gate and no exception covered it, so main was reset to 3a1cf23d at 02:55:17, nothing pushed, and the
done line (02:55:27) said NOT joined; the joiner rebuilt main's cockpit into `crates/theseusd/cockpit/dist`, which the
red gate had left holding lanes C and D's bundle (an install from that tree would have embedded a cockpit main does not
have). The DM thread filed **theseus-sdgl (P1)** at 03:02:42 and spawned local investigator R48 at 03:06; the joiner's
own theseus-qfl0 was closed as its duplicate; the test passed 10 of 10 alone afterwards and in this stack's gate.
- **Job 2** (lock `cloud-dwbr-join`, taken 02:56:37 on 3a1cf23d, after job 1's done line): the dry runs (on ce61f0cc
  during job 1's gate, and again on 3a1cf23d after the reset) clean; `joinfix.py` "sim.rs: applied", then "already
  applied"; 0 markers, format 23, INSTRUMENTS 44; runtime.rs 3,467 of 3,500 and turn.rs 3,513 of 3,523; the names
  family 0; discord-watch's 5 files 22b1368b's byte for byte, both branches' 16 files 71ab1ed6's (turn.rs with main's
  own daemon-flakes hunks merged in). Merge 2: turn.rs, `core_output.txt` and lifecycle.rs auto-merged, no conflict; the
  cloud files removed; the join fix; staged 11 files, +456 −28, tree cb5dbb19, the dry run's. The signed merge
  **6c05334d** (80243147 and 590c2ad9), 02:56:44.
- **Before the gate:** the warm (02:57:02 to 03:00:56, the test build 2 m 32 s, clippy clean); R44's filter **415 of
  415** in 54.6 s (theseus-discord 168, theseus-store 85, theseus-sim 40 with the restore-order test under the join
  fix, theseus-core 122 with the golden).
- **The gate** (03:02:17, minute 2, to 03:08:46, ok; no lock wait, 365 s held, 389 s in all): **3,137 of 3,137** (1
  slow, 28 skipped) in 312.0 s, about 03:02:40 to 03:07:52: the B+A gate's 3,124 plus the stack's 13 (discord-watch's
  six, bench-rows' four `perf::runs` tests, its two frame_times tests and the restore-order test); theseus-sdgl's test
  passed in 0.090 s. **Lifecycle on its first run** (load reading 5.26), p50/p95 ms: cold start 30.1/33.9, the config
  copy 27.8/30.4, clean shutdown with a job running 35.0/50.5, a post in flight 86.7/96.8, SIGKILL then restart
  31.2/40.0, swap 48.8/53.1, **restore 149.1/192.4 with 5 of 5 sessions and 0.22 MB of WAL** (241.6 at the B+A gate), a
  cancel's round trip 27.5/41.6 (`cancel: 2 to 2 frames`). Jobs: L1 start 10.74/11.96 ms. **Turn: frames 5 and 9 with
  the per-run blocks** (load 5.07): the plain runs' slowest frames 11.1 to 48.5 ms (the 48.5 run 5's queue frame, its
  wall of 118.4 ms not over twice the p50, so nothing flagged), the tool-call runs' 12.3 to 42.6; plain p50 81.9 ms,
  tool call 170.4.
- Pushed 3a1cf23d..6c05334d at 03:08:56 (confirmed 03:08:58), both cloud branches deleted on origin, done line
  03:09:18; theseus-w7dk and ma8r closed with 6c05334d. The store stays at format 23. The two stacks are independent:
  C+D after dwbr gives tree e4033538, the same as dwbr after C+D.
- Lanes C and D joined at their second take (03:28, after this range's end): Items 242 and
  243.

**The install** (not installed yet: on install #10's list). On the owner's daemon, each turn's trace and exported root
span carry the slowest frame the store answered since its input arrived (`slowest_frame.first` and `slowest_frame.us`);
the store's writer keeps the newest 64 frames' times. `bench turn` names each run's slowest frame, and the lifecycle
bench's restore row is the restore's own again (bench only). No config key; INSTRUMENTS stays 44.

**Divergences.** The report's literal load recipe (the bench at nice 19 beside four loops) starved the daemon too
(plain p50 2,598 ms), so it measured outliers under a gentler load. Its uncommitted measuring patch left memory-only
frames out of the count; the committed bench keeps its check as the brief said (theseus-2x5y). The restore-order test's
bound changed twice: 5 % to 25 % in the session, then R44's sessions-within-one and twice-the-WAL rule at the join.

**Known gaps.** theseus-67nz, img9 and 2x5y (P3); theseus-fps6 (P2, main's). Owed: theseus-sim's AGENTS.md `bench
turn` paragraph (the per-run lines, the last frame's caveat) and its lifecycle line (the order note), `perf/runs.rs`'s
module doc (the last frame's caveat), and a note in the bench history (the operator's
`~/.cache/theseus/bench-history.csv`, outside the repo) for the restore row's move at 6c05334d. Part I §3.20's turn row
gains the two attributes in this version.

### Item 242. The cockpit's charts by the chart method: every chart on the Bridge, Money, the Ledger, the session deck and Flame drawn by the method with a table view for each, DOM tips that stay inside their chart, the palettes checked in CI, every model's spend kept past the eighth, and, with R47's join fix, the session deck's full history (theseus-hnof.4, with theseus-kuzw by the join fix; plan step 4 and the question walk's C7; cockpit phase 2's lane C, 2026-10-06 22:54 to 2026-10-07 00:41, 0c76c009, 91af23fe, f625bf56, 73e2cca5, 364b2e30, f7d6ea52, fefe762c, f061ab99, 62c8a23c and 5c7e99b6; reviewed 01:05 to 02:15 by local reviewer R47, and accepted with a join fix; its first take red-gated at 02:54 on a test it does not reach; joined at the second take, 03:28, at 51ef2fd4, a signed merge onto 6c05334d, the first of the stack's two merges under one lock and one gate, by the C+D joiner's second take; not installed yet: on install #10's list)

**Why.** The cockpit-rethink's critique (Item 238) found the data pages "the cockpit's
substance" but in need of the chart method: the Bridge's pulse stacked eight event families in eight colours on a
30-minute axis; the latency scatter put first token and total on one log axis with no direct labels; the spend flow
cut every session's name ("Sound the channel"); the Ledger's treemap clipped its labels ("star", "trac"); Money's
river sat below the fold at 1080 px; the fleet drew a donut. The prototype had restyled Economics and Speed as the
examples; plan step 4 carried the method to every other page (`lib/chart.ts` on `lib/viz.ts`, a table view for every
chart, the palettes validated), with the pages helper's two notes. The owner took all seven steps, in parallel lanes
(C7, 22:47), under the standing rule that no datum of the old cockpit is lost; the inventory gave lane C one stop, N6
(Economics folding the eighth model on into "other models", its table included).

**What landed** (25 files under `cockpit/` from the base, +2,011 −741; the merge 27 files, +2,078 −743, with R47's
join fix; no Rust, protocol type, package, config key or store format change (23 stays 23)).
- **The base** (0c76c009). `lib/chart.ts` takes its look from `lib/viz.ts` (the method's tip frame and quiet axes
  under every option; a chart's own tooltip laid over the base's in `Echart.tsx`); `lib/palette.ts` computes the
  method's six checks from the standard formulas (OKLab, Machado 2009 at severity 1.0, WCAG 2) and
  `test/palette.test.ts` holds every chart palette to them in CI, matching the method's own validator to a tenth;
  `TONE_MARK` (a state tone drawn as a mark takes its snapped step), `inkOn()`, `tokenTick()`, `usTick()`; Speed's
  benches and starts in the snapped steps (`#b98a00`, `#b8334e`, `#00734f`); `lib/calls.ts`, a billed call's type and
  reading, pure and re-exported by `derive.ts`, so `viz.ts` no longer imports React through `derive.ts`.
- **The Bridge** (91af23fe): the method's stat tiles (each value in the ink at proportional figures, its state as an
  icon in its tone and in words, SVG sparklines); **the pulse a row per family** on one time axis, each row named with
  its rows in range and on its own scale, one crosshair through all; **one range row above the charts** scoping every
  chart under it (`?range=`); **the fleet's donut a bar** of the states' shares over the state pills; the latency
  scatter in the models' Economics slots (three, the rest folded) with the p50s as hairlines said in words; the spend
  flow's session margin measured from its names (a longer one ends in an ellipsis, whole in the tip and table); the
  start waterfall shared with Speed and Systems; turns as dots sized by cost, a failure a triangle; the tools board
  saying failures in words; **the context chart the sessions ranked by their prompt**; the token mix in the five token
  kinds. 62c8a23c: the Bridge reads the shared copy of the ledger (the cockpit's rule), drawing the newest 1,000 rows
  until the whole record is read, then the whole record once (a long read had left it saying "0 of 0 ledger rows read").
- **Money** (f625bf56, f061ab99): **the river first**, where 1080 px shows it, the budgets after it, every line kept;
  names in the ink and the sans; the kinds' key is its legend; its table behind the toggle (`?table=river`; an old
  `?table=1` still opens it); the pace's dial with its last hour, five minutes a column, and its own table.
- **The Ledger** (73e2cca5): the kinds' treemap rebuilt in HTML from a squarified layout (`squarify()`, tested), a name
  in a tile only where it fits (measured), each tile a button; the over-time chart shows the faults in the fault step
  over the rows. `textWidth()` shared with the Bridge's flow.
- **The session deck and Flame** (364b2e30): the flame chart's marks in each kind's step and a DOM tip whose attributes
  are data set as text; where the turn's time went as a labelled part-to-whole bar; failed turns in words; the Spend
  tab's ticks that never repeat ("$0.000" five times before); a table for every chart; `lib/spans.ts` (pure, tested).
- f7d6ea52 removes the old shared axis style, `ink`, the old `Kpi` tile and its `AnimatedNumber` (nothing in any lane
  read them); fefe762c gives **N6** its home: past eight models, Economics' spend table reads the unfolded series
  (every model its own column), and the tip names each folded model with its dollars (a test pins the pure part);
  5c7e99b6 confines every ECharts tip to its chart and wraps a long foot. A toggle's state lives in the address
  (`?table=<id>,…`), so a table view deep-links.
- **R47's join fix** (`joinfix-kuzw.py`, theseus-kuzw): the session deck had read its own `ledger.tail`, which the
  daemon caps at 1,000 rows, so a long session lost its oldest turns from the timeline, the cost per turn, the token
  mix and the context growth, and the deck numbered the turns it kept from one (on R47's rig a 31-turn session drew 17
  chips, its "turn 1" the session's turn 15; Spend said $0.0073 beside the session's $0.013). Older than phase 2 (the
  old cockpit read the same way), but data the cockpit held and dropped. The deck now takes its session's rows from the
  shared history once read, with a session tail standing in until then (lane C's own Bridge pattern), the choice in a
  new pure `lib/sessionrows.ts` (`deckRows()`, 3 tests); it also removes the deck's second ledger loop.

**How it is proven.**
- **The lane:** `npm test` **134** (from 119; `palette.test.ts` 6, `spans.test.ts` 3, `viz.test.ts` +4 with the
  squarified treemap and N6's assertion, main's 2); `tsc -b` and the build; lint 24 warnings (the base's 25, Flame's
  gone); the scrub 0 on 2,024 added lines; no console or page error on any page. The method's validator on every
  palette: the categorical set, the token kinds, the snapped tone steps and the benches' lines pass in dark (the light
  set with contrast warnings the table view relieves); two deliberate FAILs, the starts' ok against fault (5.8 under
  deuteranopia; status always carries a shape and a word) and the flame's span kinds (named in their bars, tips and
  table; theseus-mj2u). Every chart checked against the method's anti-patterns: none left on lane C's pages but those
  filed (theseus-zkh9). **FAST** (production bundles of the base and the lane in alternating runs, SwiftShader, a
  floor): the Bridge's **daemon traffic down 80 to 83 %** (it stopped polling 1,000 rows every 2 s); the Ledger's load
  long tasks and blocking time down 47 and 55 %, its heap 38 %; the deck's first paint about 59 % sooner (4,700 to
  1,914 ms); all JavaScript +27 kB raw (+0.6 %).
- **The review** (R47, lane C merged onto c7debb6e at 9ef8fbb3; the join fix b4947194): every commit against its claim,
  holding; 134 tests, typecheck, lint 24, build; **every inventory row of lane C found in R47's shots** (BR-01 to
  BR-20, MO-01 to MO-17, LE-01 to LE-13, SD-30 to SD-52, SP and SY-09), N6's home in code; **the base was wrong where
  lane C is right**: its "Tokens · last hour" read 36 calls from its 1,000-row read, lane C reading the whole record
  114. The four changes of form judged each better (the pulse's rows, the fleet bar, the context ranks, Money's river
  first). The Bridge's traffic **down 86 to 87 %** on R47's rig (93 % with lane D), its shared copy followed every 2.5
  s against the old 2 s poll, so nothing goes stale by more than half a second. **Plants:** 6 caught (`TONE_MARK`
  taking the light steps, a categorical slot drifting, the treemap's column, the pulse's last bin, words in a fill
  always ivory, a tools span counted beside its calls); 2 missed (the treemap laid in the values' own order; the N6
  table line in the view): theseus-ck2f. **kuzw, live with the fix:** 31 chips from the session's real first turn,
  Spend's $0.013, the deck's traffic down 88 %; both of `sessionrows.ts`'s plants caught.

**What the review found.** theseus-kuzw (above, fixed at the join); **theseus-v6vc (P3)**: with the river first, a
budget question waiting at its limit has its answers below the fold at 1080 px (R47: show a waiting question above the
river; the owner's question, walk row F11, unanswered); **theseus-ck2f (P3)**: the two uncaught plants. Lane C's own:
theseus-mj2u (the flame's untoned span kinds), theseus-9owg (a stand-in model that echoes the profile's model and
cache, so a rig shows several models; the fold past eight is shown on one model only), theseus-0jzp (the row lists set
a kind's name in its tone), theseus-zkh9 (the method's remaining items: Money's range above the tiles, legend toggles,
texture, a refetch holding the frame, the Speed dials' dashed line).

**The join** (the stack's first; the second take's lock and its one gate are told in Item 243).
**The first take** (the C+D and dwbr joiner's job 1, lock `cockpit-p2-cd-join` on 3a1cf23d, Item 241)
merged lane C as 951b7ee3, the same tree its second take would, and its gate failed on theseusd's `job_wrapper` linger
test (theseus-sdgl), a Rust test this cockpit-only stack does not reach; main was reset and nothing pushed. **The second
take** (the C+D joiner, 03:12 to 03:35; lock `cockpit-p2-cd2-join` from 03:17:07, on 6c05334d): the dry run read file
for file as the first take's but for the dwbr stack's 16 files, none under `cockpit/`. Merge 1, `git merge --no-ff
origin/lane/cockpit-p2-charts`: no conflict (merge bases fad08f32 and the prototype base, as for every phase-2 lane);
`joinfix-kuzw.py` created `lib/sessionrows.ts` and its test and edited `SessionDeck.tsx` (2 "created", 3 "done"); lane
C's 24 files byte for byte R47's 9ef8fbb3 and kuzw's 3 b4947194's; staged 27 files, +2,078 −743, tree 95467f40, the
dry run's R1, nothing outside `cockpit/`; the names family 0. The signed merge **51ef2fd4** (6c05334d and 5c7e99b6),
03:17:16. Pushed with the stack at 03:27:38, `lane/cockpit-p2-charts` deleted on origin, done line 03:28:16;
theseus-hnof.4 and kuzw closed with 51ef2fd4. The store stays at format 23.

**The install** (not installed yet: on install #10's list). The owner's cockpit gains, with lane D's (Item 243): the
chart method on the Bridge, Money, the Ledger, the session deck's timeline and Flame, a table view for every chart in
the address (`?table=`), DOM tips inside their charts, the palettes checked in CI; Economics keeps every model's spend
past the eighth; the Bridge reads the shared history, so its daemon traffic falls 86 to 87 % (93 % with lane D); the
session deck holds a session's whole history, no oldest turns lost or misnumbered. No config key.

**Divergences.** Four changes of form, each for the method's reasons and judged better by R47 (the pulse's rows, the
fleet bar, the context ranks, Money's river first); three data moved on Money (the budgets below the river, its table
behind the toggle, its kinds' key into the legend), none gone. The Bridge's first rows still take about 3 to 5 s on a
cold dev page, as the base did. kuzw, older than phase 2, rode with this join on R47's recommendation.

**Known gaps.** theseus-v6vc (the owner's), ck2f, mj2u, 9owg, 0jzp, zkh9 (P3); the owner's GPU (theseus-n2hd).
`cockpit/AGENTS.md` does not yet list lane C's new modules (`lib/calls.ts`, `palette.ts`, `spans.ts`, `chartview.ts`,
`components/instrumentTables.ts`, kuzw's `lib/sessionrows.ts`) or the invariant that every chart has a table view in
the address, owed by R47.

### Item 243. The cockpit's repairs and its daylight mode: the heartbeat bar with words beside its lamps and the moment's "then", Judgment without overlaps, the activity strip folded and remembered, the approval card leading with its decision, and a daylight mode, the brass bridge by day, while the Ship stays a night sea (theseus-hnof.5, with theseus-4g6g's lane C part by a join fix; plan steps 5 and 6, the question walk's C7, and the DM thread's decision of 01:10 on the inventory's stops N2 to N4; cockpit phase 2's lane D, 2026-10-06 23:00 to 2026-10-07 00:45, 56f2145a, db8bfd69, aaa2514f, f0091af3, ff560162, 0517aec4, bdf5b923 and 28a57a89; reviewed 01:15 to 02:15 by local reviewer R47, and accepted with two steps at its join; its first take red-gated at 02:54; joined at the second take, 03:28, at ebb7b09f, a signed merge onto 51ef2fd4, the second of the stack's two merges under one lock and one gate, by the C+D joiner's second take; not installed yet: on install #10's list)

**Why.** The critique's repairs (plan step 5): Judgment was broken at 1080 px, its left column's five panels running
over each other; the activity strip opened at the foot of every data view, took its bottom 180 px, was not remembered
when folded, and was mostly 500 copies of one line; the header carried thirteen readouts whose health dots did not read
(10 px words), and at 1366 px the profile chip shrank to nothing; the approval card's decision was its fourth line. And
a light mode (step 6), which the owner wanted (C7: all seven steps, in parallel lanes). The inventory held lane D to
keep every header readout, four of which had no second home (the link's round trip, the flow, the plank strip, the
clock), and to keep the profile chip, its menu and UP, the only homes of the retired compass's and chronometer's live
data. The DM thread decided at 01:10 that the time machine's past profile, uptime and "down then" (N2 to N4) live once,
in this lane's header "then", and that lanes A's and B's copies go at the joins.

**What landed** (26 files under `cockpit/` from the base, +1,522 −290; the merge 31 files, +1,585 −333, with R47's two
steps and R46's lane-A fix; no Rust, protocol type, package, config key or store format change (23 stays 23)).
- **Judgment's panels** (56f2145a, 28a57a89): from 1280 px, two columns that each scroll (the packs' table under a
  sticky header with Jev's day under it, then the log; on the right the judgment picked, the notices, the ladder, the
  versions and learning); one column narrower; the page scrolls under about 640 px tall. Every panel and number kept.
- **The activity strip** (db8bfd69): folded by default on the Ship and the data pages alike, kept in the browser
  (`cockpit.activity`); lines that say the same thing, numbers aside, fold into one with ×N, "since" and how many
  sessions, every line in exactly one fold (`lib/activity.ts`, pure, 5 tests); a click lays a fold's lines out again;
  folded, its bar still says the newest thing that happened, the counts, and the **flow** (rows a second with its
  sparkline), moved here from the header beside the rows it counts. It reads 120 rows only while open, a dozen folded.
- **The heartbeat bar** (aaa2514f, 0517aec4, bdf5b923; `components/Heartbeat.tsx`): thirteen readouts became a few
  instruments, each an engraved label over its value: the wordmark and version, the live profile chip and its menu
  (every profile with its provider and model, the live one checked, made live after a confirm; it now fits at 1366
  px), up, running, what needs you, the disk's attention item, nine health lamps (link, kernel, provider, Discord,
  config, secrets, web, binary, disk), spent with the cache's share, pause and refresh, the date over the clock, and the
  plank strip under the bar. Each lamp says its state in a word beside its dot ("65 ms", "open", "1 error", "off",
  "dev page", "jobs can write", "595 GB free"; `lib/healthwords.ts`, 5 tests); under 1800 px the worst are said in
  words, and a press opens a card with every lamp's whole sentence. **Under the time machine the bar reads the moment**:
  "live profile · then", "up · then" or "down then", "running · then", "spent · then", the labels' "then" in amber: N2
  to N4's home. It reads the ledger's history only while the time machine is set, and only the clock and the uptime
  tick each second.
- **The approval card** (f0091af3): it leads with its decision in the daemon's own words (`decisionWords()`): "Approve:
  create harbour/log.md, 38 bytes", "Reset to $0 and continue: spent $0.00 of its $0.00001 limit; the waiting call
  needs $0.067 more"; every field kept, reordered; tiny dollars read to their first figure.
- **The daylight mode** (ff560162): `html.light` re-points every token (the papers, page #efe8d8 and panel face
  #fbf8f1; brass hairlines; the inks; each night state tone kept in hue and deepened in OKLCH lightness to the lightest
  step that reads as small text on both papers, 4.5:1 or more, held by a test); `lib/mode.ts` keeps the mode
  (`cockpit.mode`, `?mode=light|dark` for one page), `public/mode.js` sets the class before the first paint, and a sun
  on the rail's foot or "Daylight" in Ctrl+K switches it; night stays the default. Every chart's option goes through
  `daylight()` (`lib/daylight.ts`, 5 tests) as `Echart` draws it; the DOM tips and the legends' swatches read the mode;
  a switch draws the views again. **What stays at night:** the Ship (`.ship-root` takes the night tokens back, so its
  sea, nameplates, key, watch and console read as always inside the daylight frame), the ship's log's track, and the
  instruments' glass.
- **At the join, R46's `review-A/joinfix.py`** removed lane A's cartouche "then:" line (and its `Then` type, `thenOf`
  and the unused `uptime` import), so the header is the one home of N2 to N4; the cartouche keeps its "as of" time.
- **At the join, R47's `resolve.py`** kept both sides of lane D's two conflicts with lane C (`components/Echart.tsx`:
  C's tooltip laid over the base's, and D's `daylight()` of the option; `components/ChartPanel.tsx`'s imports), and
  **R47's `joinfix-day.py`** (28 edits in 8 files) closed four daylight gaps R47 found: lane C's flame chart, an ECharts
  custom series whose `renderItem` computes its own colours out of `daylight()`'s reach, labelled its spans ivory on
  pale washes by day (it now paints by day); the HTML marks set inline from `TONE_MARK` kept their night steps (now
  `MARKS` in `daylight.ts`, and the mode sets `TONE_MARK` as it sets `toneHex`, a test holding that each day mark is
  exactly what `daylight()` gives the night step); a tip row's stroke and the sparkline's newest dot; and, in lane D's
  own scope, the call and model inspectors over the Ship drew their tones at 3.3 to 3.4:1 by day inside its night
  island (now CSS tokens, which the island re-points).

**How it is proven.**
- **The lane:** `npm test` **137** (from 119; `activity` 5, `healthwords` 5, `daylight` 5, `toolwords` +1, main's 2);
  `tsc -b`, lint (26: the base's 25 and one of the same kind in `main.tsx`), the build (the dist holds `mode.js`); the
  scrub 0; every view loading with no console or page error in every screenshot run, 49 shots by night and by day. The
  palettes by day: the method's categorical light steps, the tones' series steps and the token kinds pass on both
  papers (contrast warnings the table views relieve); the daylight state tones as a set fail the categorical checks, as
  the night tones do, but are state colours always with an icon, a shape or a word, and each reads as text at 4.61 to
  5.41:1 (the inks 5.40 to 14.38). Every one of lane D's inventory rows (F-01 to F-29, F-41 to F-46, JU-01 to JU-10,
  IN-01 to IN-17) kept, three moved (the flow to the strip's bar; the link's round trip and the provider errors into
  their lamps). **FAST** (SwiftShader, B H H B): the Bridge's first panels in about 1.1 s against 2.6 to 2.9 (the folded
  strip no longer draws 120 animated rows at load), Economics 1.8 to 1.9 against 2.5 to 5.5; idle browser CPU equal or
  lower; style recalculation +50 to 80 ms in 10 s idle (0.5 to 0.8 % of a core, the header's instruments), after two
  fixes brought it down from about +80 to +100.
- **The review** (R47, lane D on lane C at 1dd8ef08, the join fixes c5302258 and 248cdc89, tagged `r47-cd`): C+D 150
  tests, 154 with the join fixes; the full chain B, A, C, D dry-run on f81c0e62 with every resolve and fix, 197; every
  commit holding. **The header:** every readout of the thirteen has a home, the four with no second home verified (the
  round trip in the link lamp's word over 1800 px and in the card; the flow on the strip's bar; the plank strip; the
  clock with its date); the chip, its menu (and its confirm) and UP kept. **The "then", live on a rig that stopped the
  daemon 20 s and changed profiles twice:** in the gap "up · then: down then", "spent · then $0.019"; while opus was
  live "live profile · then opus", "up · then 13s", the menu saying "as of 01:23:20, opus was live". Judgment without
  overlap at 1366×768 and 1920; numbers-aside folding merges no success with a failure (a result's status is a word,
  not a number). **Plants:** all 8 of lane D's caught (numbers-aside folding; the strip open by default; a fold
  dropping its lines; a day tone under 4.5:1; alpha lost by day; the link lamp without its round trip; the summary not
  worst first; the card's path); of the join fix's two, one caught and one a view-only miss (`mode.ts` imports zustand
  and has no node test; the day shots are its evidence). **FAST:** first panels within the noise; idle CPU equal or
  lower (Judgment 11.4 and 11.0 s on the base, 7.8 and 6.9 with C+D); the Bridge's traffic down 93 %; style
  recalculation +70 to 90 ms per 10 s idle, about 0.8 % of a core (leave it).

**What the review found.** The four daylight gaps (above, fixed at the join); **theseus-bpg0 (P3)**: two cut lists do
not say what they cut. **For the owner:** keep night as the default even when the system is light, since the Ship is a
night sea in either mode and a light default would open a night Ship in a daylit frame, and offer "follow the system"
as a third choice (walk row F10, not yet answered); keep the Ship a night sea by day. theseus-4g6g's lane C part is
covered by `joinfix-day.py`; what is left is a look at the joined main by day.

**The join** (the stack's second, and the stack's one gate; merge 1 is Item 242). **The first
take** (job 1 of the C+D and dwbr joiner, lock `cockpit-p2-cd-join`, on 3a1cf23d, 02:47:06): merge 2 ce61f0cc (tree
a1733a67), the cockpit 198 of 198 in main's tree (R47's chain read 197; the 198th is lane B's join fix's test), the
warm 14.6 s; its gate (02:48:34 to 02:54:02) **failed in its suite, 3,123 of 3,124**, on theseusd's
`job_wrapper::a_double_fork_stays_under_its_wrapper_which_lingers_until_it_ends` ("timed out waiting for the lingering
mark", 20 s), which no file of this stack reaches; one gate allowed and no exception for it, so main was reset at
02:55:17 and the done line said NOT joined (Item 241 tells it, and theseus-sdgl, P1, R48's to find).
**The second take** (the C+D joiner, take 2, spawned by the DM thread; 03:12 to 03:35):
- **The lock** `cockpit-p2-cd2-join` at 03:17:07 on 6c05334d (the dwbr stack joined between the takes; its 16 files
  none under `cockpit/`); the dry run read file for file as the first take's, R1 95467f40 and R2 e4033538, and R2's
  `cockpit/` subtree is the first take's tree for tree.
- **Merge 2**, `git merge --no-ff --no-commit origin/lane/cockpit-p2-repairs`: exactly the two content conflicts,
  which the repo's rerere replayed from R47's recorded resolution (R47's review worktree shares main's `rr-cache`);
  `resolve.py` "already resolved" for both, each file 1dd8ef08's byte for byte; R46's `joinfix.py` (5 "applied", the
  `uptime` import removed, `Watch.tsx` "nothing to do": lane B's join fix had already removed the watch's copy);
  `joinfix-day.py` (28 "done" in its 8 files); the stack's 50 files but R46's two byte for byte 248cdc89's; staged 31
  files against merge 1, +1,585 −333, 52 against origin/main (+3,659 −1,072), tree e4033538, the dry run's R2, all
  under `cockpit/`; the names family 0 (the scrub's one other hit two loopback dev-origin URLs in a test fixture). The
  signed merge **ebb7b09f** (51ef2fd4 and afeeb78d), 03:17:18.
- **Before the gate:** the cockpit in main's tree after `npm ci --offline`: **198 of 198**, `tsc -b` 0 errors, lint
  main's 25, the build; the warm 21.5 s (only theseusd recompiled, for the new dist), clippy clean. No Rust test of the
  stack's own: it changes no Rust.
- **The gate** (03:19:18, minute 19, to 03:27:27, ok): a 52 s lock wait behind R48's probe build (a shared hold the
  brief allows), 373 s held, 488 s in all. **3,137 of 3,137** (1 slow, 28 skipped) in 318.0 s, about 03:21:15 to
  03:26:35, the dwbr gate's same 3,137 names; **theseus-sdgl's linger test passed in 0.089 s, so the sdgl exception was
  not used**. **Lifecycle on its first run** (load reading 5.54), p50/p95 ms: cold start 38.0/47.1, the config copy
  31.5/40.8, clean shutdown with a job running 35.8/46.2, a post in flight 93.5/437.5 (one slow run; the row has no
  budget), SIGKILL then restart 31.7/35.2, swap 54.7/68.7, restore 183.1/234.2 with 5 of 5 sessions, a cancel's round
  trip 26.7/33.9 (`cancel: 2 to 2 frames`). Jobs: L1 start 10.36/12.95 ms. Turn: frames 5 and 9, plain p50 78.5 ms,
  tool call 171.4, the per-run blocks' slowest frames 11.5 to 18.6 ms with no run flagged. The joiner read the higher
  cold start as the machine (theseusd differs from 6c05334d's only in its embedded dist), to be re-read at the next
  gates.
- **Pushed** 6c05334d..ebb7b09f at 03:27:38, both lane branches deleted on origin, done line 03:28:16; theseus-hnof.4,
  kuzw and hnof.5 closed, then **theseus-hnof itself (cockpit phase 2 whole on main)**; theseus-4g6g noted, left
  blocked for its look by day; the six local lane branches and their five worktrees removed (each in main and clean).
  The store stays at format 23. R48's own blocks, meanwhile, had reproduced the linger red once in 50 runs under load
  (the joiner's report), so this gate's pass clears nothing; install #10 stays on hold for R48's verdict.

**The install** (not installed yet: on install #10's list). The owner's cockpit gains the heartbeat bar with words
beside its lamps; the time machine's moment's profile, uptime and "down then" in the header's "then", so the Ship's
cartouche loses its "then:" line; Judgment without overlaps; the activity strip folded, remembered and counting repeats;
the approval card leading with its decision; and the daylight mode behind the sun in the rail and Ctrl+K, night the
default, the Ship at night either way. The dist in main's tree is ebb7b09f's. No config key.

**Divergences.** Three header data moved rather than stayed (the flow to the activity strip's bar, the link's round
trip and the provider errors into their lamps), each verified by R47. The four daylight gaps lane D's own run missed
were closed at the join, not in the branch. Two of lane D's choices are the owner's to change: night as the default
when the system is light (a one-line change), and the Ship's night sea by day.

**Known gaps.** theseus-bpg0 (P3); theseus-4g6g's look at the joined main by day; the owner's question on "follow the
system" (walk row F10). `cockpit/AGENTS.md` does not yet list lane D's new modules (`components/Heartbeat.tsx`,
`lib/activity.ts`, `daylight.ts`, `flow.ts`, `healthwords.ts`, `mode.ts`, `public/mode.js`) or its three invariants (a
colour set outside an ECharts option follows the mode; inside the Ship's night island a tone is its CSS token; the Ship
and the ship's log's track stay at night), owed by R47.
