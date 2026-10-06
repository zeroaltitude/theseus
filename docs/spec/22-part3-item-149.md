# The Ship of Theseus, chapter 22: Part III, A4's Items 149 to 158 ([index](README.md))
### Item 149. Card 2: a job's wrapper and its L0 command spawn without a fork, so a job's start no longer grows with the daemon's size (theseus-ypqg; the Linux survey's card 2, theseus-779n, one of the picks the owner approved with the survey's review at 15:19; the `linux-jobs` lane, spawned 2026-10-04 17:55, in a worktree, on origin as `lane/linux-jobs`; 01a90846 and bff95fef on d5a4b808, with card 5's two commits after them on the lane; reviewed 19:53 by the DM thread; joined 20:18 at 3c85ecee, a signed merge of bff95fef alone onto 4cef410c (b5's docs, merged first), by the card-2 joiner; installed 21:22 at 645769d2, install #3)

**Why.** The Linux survey (theseus-779n, reviewed 15:19) found three spawn paths in the daemon that each passed a
`pre_exec` closure to std's `Command`: `setsid()` in the daemon's spawn of every job wrapper, `umask()` in the
wrapper's spawn of every L0 command (every job carries the operator's umask, so this ran on every L0 job), and
terminals' `setsid()`, `TIOCSCTTY` and `umask()`. Any `pre_exec` makes std fall back from `posix_spawn` (glibc's
`clone3` with `CLONE_VM|CLONE_VFORK`, which copies nothing) to `fork()`, which copies the spawner's page tables and
write-protects its every page. The survey measured std's two paths at 0.58 against 2.32 ms for a 25 MB spawner and
0.97 against 41.72 ms at 1 GB, and a fork leaves a tax behind it: after one fork, rewriting 1 GB took 351 ms and
262,145 faults, against 123 ms after a `posix_spawn`. `children::spawn` holds the child registry's lock across the
spawn, so every spawn in the daemon queued behind the slowest fork. The card's modified goal: the same session and
group semantics, with a spawn cost that does not depend on the daemon's size. The owner approved picks 1 to 4 and 6 as
recommended (the survey's review, 15:19), card 2 among them.

**What landed** (two commits; the merge 5 files, +61 −32; production −3 lines and tests +1 by the lane's
`countlines.py`, which counts Rust code lines net, without comments, blanks or tests).
- **The wrapper makes its own session** (`crates/theseus-kernel/src/job.rs`). `spawn_detached` drops its `pre_exec`,
  so std takes `posix_spawn`, and `setsid()` is the first act of `run_wrapper_process`. A `posix_spawn`ed child leads
  no group, so the call succeeds; a failure is recorded in the completion's detail with the subreaper's and the
  handlers'.
- **The wrapper takes the operator's umask around the spawn** of an L0 command and puts its own back after, in place
  of a `pre_exec` in the child. The wrapper makes no file in between (its copy thread writes to a file already open),
  so nothing it writes is created under the operator's mask. A test's in-process thread still sets it in the child.
- **The bench can hold memory** (01a90846, `crates/theseus-sim/src/jobs.rs`): `theseus-sim bench jobs --hold-mb N`
  touches N MB before dispatching, as a grown daemon would, and its jobs carry an operator's umask, so the bench takes
  the daemon's real path. Bench only.
- **Tests.** The tree tests' stand-ins for a wrapper from before 18a and for a deaf one now make their own session, as
  the daemon that started such a wrapper made it (`crates/theseus-kernel/tests/tree.rs`,
  `crates/theseusd/tests/job_wrapper.rs`); the kernel's `AGENTS.md` says where the session is made.
- **Terminals keep their `pre_exec`** (`crates/theseus-core/src/term/pty.rs`): they open rarely, and their three
  calls want the survey's small `pty-exec` role, a later step.

**How it is proven.**
- **strace** (`probes/strace-spawn.sh`: `strace -f -e trace=clone,clone3,fork,vfork` on `theseus-sim bench jobs
  --class l0 --runs 1`, whose dispatch is the daemon's own `spawn_detached`). Before (01a90846), the dispatcher's
  spawn of each wrapper and each wrapper's spawn of its command were fork-style `clone(child_stack=NULL,
  flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD)`; after (bff95fef), both are `clone3({flags=CLONE_VM|
  CLONE_VFORK, exit_signal=SIGCHLD, …})`.
- **The lane's A/B** (`benchjobs.sh ab1`: settled first, 165 s, then the shared gate lock held for every run, in
  palindrome order, twice; `bench jobs --class l0,l1 --runs 50`; debug builds copied out of the target; each cell the
  median of four runs). At 1 GB held, an L0 job's total p50 went from **26.61 ms to 14.11 ms**, the same as at 0 MB
  (14.18), and an L1 job's total from 32.32 to 15.79 ms: the fork's 12.4 ms on an L0 job and 16.5 ms on an L1 job's
  total were gone. At 0 MB nothing moved (14.18 against 14.31). §2.2's target, an L1 start p95 under 25 ms, read 6.6
  to 8.3 ms in every arm. Most of an L0 job's 14 ms is the spool's syncs (the wrapper's completion is written durably
  before it is read), which Item 151 cut from two syncs to one.
- **Tests.** Card 2's 176 targeted tests (kernel, wrapper, sandbox, stops, reaping) before its lane gate, which was
  green in 522 s (99 of them waiting for the lock behind a review step on main).

**The join** (the card-2 joiner, spawned by the DM thread; lock `lane-linux-jobs-card2-join` taken at 20:06:44, in one
guarded call with the first merge, after install #2's done line at 20:04:15). Both merges were dry-run first with
`merge-tree --write-tree` on 3085f71a, and both were clean, with no join fix. 4cef410c merged lane/b5's docs commit
(Item 148); 3c85ecee merged card 2 at **bff95fef**, never at the lane's tip, so card 5's c3b25a1c and 20c7f106 are
not ancestors of main and the merge holds no card-5 file. The joiner's tests, at nice 10 under load 3.9 to 6.8: the kernel
suite three times (143 of 143 each, theseus-g11i's tree test passing in 1.05 s each time), theseus-sim's 49 of 49, and
theseusd's four job-spawn binaries, 33 of 33. On the merged build, `bench jobs` gave an L0 job 14.60 ms p50 at 0 MB
held and 14.80 ms at 1 GB, against **46.21 ms** at 1 GB for the build before card 2 (under this load the fork cost
more than on the lane's quiet machine). **The gate** (20:12:37 to 20:17:29, 292 s, strict: the settle needed no
wait): suite **2,486 of 2,486**, one passing on a retry (theseus-sim's seeded-faults test, theseus-81ig's listed
flake: a positive count of what a raced kernel sim happened to take, "all invariants held"; the kernel sim uses no
job spawn); cold start p50 23.1 and p95 27.1 ms; clean shutdown with a job running 33.1/50.8; SIGKILL and restart
27.5/28.6; swap 50.6/68.0; jobs' L1 start 5.95/6.61 ms, as the gate before; turn frames **5 and 9**, the tool-call
turn (which starts a job, so card 2's spawn is on its path) at p50 169.5 ms, the lowest of the last 12 gates. Pushed
3085f71a..3c85ecee at 20:18:39, the done line 20:19:03. theseus-ypqg and theseus-n88g.5 closed; theseus-a5nv was
noted that a later merge of the lane's tip brings only card 5's two commits. Reviewed by the DM thread at 20:50.

**The install** (install #3, 21:22:21 to 21:22:30, at 645769d2, with the route fix of Item 150 and
b5's docs; no key and no store format change, the store staying at 16). `theseusd check` exited 0 (config from `/etc/theseus/theseus.toml`, 9 secrets resolved, the L1 self-test
worked, start 5.7 ms); after the restart, health: secrets ready (9 ready 1,052 ms after start), discord ready, index
ready, no job wrapper running, `route.v1`, `rerank.v1` and `security.v3` live. Startup read serving at 417 ms (store
234, kernel 180) with two lanes compiling at load 10.6 and IO pressure (avg300) 7.7 %: install #2 had read 29.1 ms on
a quiet machine, and 645769d2's join gate passed the lifecycle bench strictly, so the DM thread read it as neighbour
IO, not the code, and left a quiet restart to re-check it.

**Divergences.** The survey offered `process_group(0)` as the simpler path; the lane kept the full `setsid()` in the
wrapper, which gives the job a session as well as a group, as before. The umask moved into the wrapper process (the
survey's first option) rather than explicit modes on the spool files.

**Known gaps.** Terminals still fork the daemon: the survey's `pty-exec` role is a later step. theseus-g11i (P2), the
kernel tree test whose deadline stop once left a job's two sleepers running past its completion at batch 5's
smalls-tools gate, stayed open with four passes noted on card 2's merge (card 2 moves one step on its path: the
wrapper's `setsid`); it closed at batch 7's kernel-fixes join (Item 173).

### Item 150. Routing's sticky state, and three of b5's harness losses: a routed session keeps its move only while `route.v1` acts, and the bench profile gets its model's whole output, open private addresses and a transient failure's retry inside its turn (theseus-9yyr item 1, theseus-7gir.19, .20 and .21; the `route-bench-fixes` lane, in a worktree from 3085f71a, one lane with two joins; 6e59dac0, then 4f8c7c56, 7e7ef6b8 and 7c77c698; joined first at 645769d2, a signed merge onto 3c85ecee made 20:56 and pushed after its gate (21:04:43), then at 42a8222f, a signed merge onto e6378de8 made 21:32 and pushed after its gate (21:43:02), both by the lane itself; reviewed 21:46 by the DM thread; the route fix installed 21:22 at 645769d2, install #3; the bench fixes installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Two sets of fixes in one lane, the route fix first, since routing went live on the owner's daemon at 20:00
(install #2). **The route fix.** R3's review of `route.v1` (Item 139) filed theseus-9yyr (P2, raised to P1 with
the owner's routing decision of 18:02): `route_base` ran every unpinned message of a routed session on `session.routed.profile`, whatever
`[routing]`, `[judge]`, the ladder or Jev said afterwards, so a session routing had moved stayed moved with routing
off, in shadow, rolled back, or with Jev unreachable. The lane found a second sticky path R3 had not named: the CLI
pane (`theseus watch --interactive`) carries the profile the last turn ran on with each message (`carried: true`,
theseus-nu3z), which after a switch is the routed one. **The bench fixes.** b5's loss analysis (theseus-7gir.1,
reviewed 19:53) put five of b5's six strong losses on mechanical causes and filed four fixes; three were the
harness's own and came here, the fourth (.18, the refusal fallback) waited for the owner (Item 154):
- **.19, the output cap.** `bench/theseus-bench.toml` capped output at 32,000 tokens twice, so regex-chess and
  schemelike-metacircular-eval ended on `stop_reason: max_tokens` (exit 8) in all of arm A's four and arm B's two
  trials, where Claude Code, with the catalog's 128,000, solved both.
- **.20, a private address.** The bench profile's header promised that no headless call waits for approval, but an
  `http.fetch` of `localhost/vnc.html` in install-windows-3.11 waited on the gate's private-address rule
  (`needs_confirm`, exit 6).
- **.21, a transient failure.** hf-model-inference's trial ended on `provider timeout (FirstByte) after 60001 ms
  [transient=true]`, exit 1: a one-shot `ask` had no retry inside its turn, where Claude Code retries.

**What landed.**
- **The route fix** (6e59dac0; 10 files, +394 −19; `crates/theseus-core/src/turn/route_step.rs`, `rpc/methods.rs`,
  `judge/inbound.rs`, `lib.rs` and the tests).
  - **`route_base` reads routing's state at the turn's start.** An unpinned message runs on the routed profile only
    while `route.v1` acts live for the session: its mode is canary or live (`[routing]` enabled and live, the judge
    on, and the ladder's rung, which includes its rollback, under the config's ceiling: the same `route_mode` the
    inbound point reads), and Jev is reachable (`JudgeService::reachable`, new: its key settled or still settling,
    and its breaker not open). A key still resolving after a start counts as reachable, so a restart does not reset
    every routed session.
  - **Clear, not ignore.** Once `route.v1` does not act, the turn sets `session.routed = None` in its own copy of the
    record, and the turn's own session write at its end carries it (`take_turns_fields`): no frame of its own. Nothing
    of the old move can come back when routing acts again (with "ignore", the old profile would have returned at the
    next midnight, when the ladder's day brake lapses, without any verdict, on exactly the sessions the owner's pins
    had rolled back). A turn that dies before its write is cleared again at the next message.
  - **`profile.use` moves a routed session.** It writes META `live_profile.switched_ms`, a new key (no store format
    change), in the frame that switches the live profile; a routed session whose last turn began before that time has
    `routed` cleared at its next turn, of any kind, and runs on the new live profile. When a turn began is read from
    its id: `new_id` mints UUIDv7 ids, whose first 48 bits are Unix milliseconds (`crate::id_ms`, new). Using the
    start, not `last_active_ms`, closes the race of a `profile.use` landing while a turn runs. The mark is read
    lazily, at the first routed session's turn after a start, never on the start path. `profile.use` stays no pin, as
    25e's task file says, so routing may move the session again on a later verdict.
  - **The pane's carried routed profile names nothing** (`turn_submit`): a carried profile equal to the session's
    routed one, with no provider or model named, is dropped before the target resolves.
  - **FAST.** A session routing never moved does nothing new (`route_base` returns at its first `Option` check). A
    routed session's message reads `route.v1`'s mode once, at the turn's start, and the inbound point takes that
    reading (`RouteState.read`), so the read moved rather than being added; what is new is the breaker's status, one
    secrets-board lookup, an id's time parse and an atomic load, microseconds in all.
- **The bench fixes** (21 files, +616 −21).
  - **.19** (4f8c7c56): the bench profile names no `max_output_tokens`, so a call asks for its model's catalog
    maximum, 128,000 for Sonnet 5.5, as Claude Code does; the adapter's model choice carries its own cap.
  - **.20** (7e7ef6b8): `[policy] private_addresses = "ask" | "open"`. `ask`, the default, is the rule as it was, so
    a config without the key (the owner's) behaves as before; `open` judges a private address as any other, leaving it to
    the tool's posture. `http.fetch`'s own guard below the gate follows the knob (`Web.private`): under `open` it
    reaches private hosts, follows private hops and uses the unchecked client. `policy.explain` lists the condition
    only when it asks. The template documents the key, commented; the bench profile sets it open.
  - **.21** (7c77c698, `turn/retry_step.rs`, two lines in `turn.rs`): `[model.retries] transient`, how many times a
    call that failed with a class that passes with time is made again inside its turn, by the loop's next call; 0, the
    default, makes none, so an interactive daemon is unchanged. `backoff_ms` (2,000) and `backoff_max_ms` (30,000)
    double on tokio's timer, and a `/stop` ends a wait at once. Each failed call keeps its `provider.error` row; each
    retry is a `retry` span, a narrative line (`ModelRetried`) and its loop's cut (`transient_retry`). The bench profile
    sets `transient = 4`, waits of 2, 4, 8 and 16 s (Claude Code's exact defaults were not checked; one line to match).

**How it is proven.**
- **The route fix's tests** (`tests_route.rs`, 4, and a unit test): a ladder rollback returns a session on Opus to
  Sonnet at its next message and clears `routed`, and after a re-promotion a chat verdict leaves it on Sonnet; four
  cases after a restart (`[routing] enabled = false`, `mode = "shadow"`, `[judge] enabled = false`, Jev's key failed)
  each return a session from Opus to Sonnet; `profile.use glm` through the protocol server runs the next chat verdict
  on GLM-5.3 Flash, and a later sophisticated verdict moves it to Opus again; the pane's carried Opus stays only while
  routing acts; an id's time is when `new_id` minted it. **Planted reverts** (`route-plants.log`), each failing as it
  should: the liveness check off (tests 1, 2 and 4: `left: ("opus", "claude-opus-5-5") right: ("sonnet", …)`); Jev's
  reachability left out (test 2's key case); `profile.use`'s mark never read (test 3: `right: ("glm",
  "glm-5.3-flash")`); the pane's carried profile kept (test 4). The whole theseus-core suite in the lane: 1,088 of
  1,089, the miss a timing bound in `tests_security` (3.27 s against 3 s with neighbours building) that passed on its
  rerun and in the gate.
- **The bench fixes' tests**, each the issue's reproduction with no live key and no model call:
  `bench_profile.rs::a_bench_call_asks_for_the_models_whole_output` (a headless `theseus --spawn theseusd --json ask`
  on the bench profile against the stand-in model ends 0, and the recorded request carries `max_tokens: 128000`);
  `policy::tests::a_private_address_waits_unless_the_policy_opens_it`, `web::tests::an_open_policy_reaches_private_
  addresses_unapproved` (127.0.0.1, a redirect to it, and a name answering loopback) and the end-to-end trial
  `a_fetch_of_this_machines_page_runs_and_waits_for_no_one` (exit 0, not 6, and the page server saw the GET);
  `a_first_byte_timeout_is_retried_inside_the_headless_turn` (the stand-in model learned `stall_next`; the trial ends
  0 on its second call, and 1 with one call under `transient = 0`, as b5's trial did) and
  `tests_failures::a_transient_failure_is_retried_inside_its_turn_as_the_config_allows` (two 529s then an answer give
  one turn of three calls, two `provider.error` rows and two `retry` spans; past `transient` the turn fails to the
  driver's backoff as before; a 400 is never made again). **Planted reverts**: .19's 32,000 line restored (`left:
  (…, Some(32000)) right: (…, Some(128000))`) and nine for .20 and .21 (`b2021-plants.log`): the gate asking whatever
  the knob (unit `left: Approve right: Open`; the trial exits 6), the fetch refusing an unapproved private host (unit
  and trial), the bench profile without the knob, the step never retrying (core and trial, exit 1), the profile
  without `[model.retries]`, and a class that will not pass retried too; every one failed as it should. Before the
  second join the lane's whole-workspace run passed 1,205 tests with no failure, stopped there so the gate would run
  on a quieter machine.

**What the lane found.** At the 128,000 cap one Sonnet 5.5 call reserves about $1.28 of a trial's $2.00 spend limit
(every call reserves its whole output cap before it runs), so a trial that has spent about $0.60 to $0.70 has its
next call refused (exit 5), where at 32,000 the line was about $1.60; b5's arm B solved regex-chess at $0.60 and
schemelike at $0.52, the very tasks .19 is for. Claude Code's `max_budget_usd = 2.0` counts real spend. `profile.use`
is daemon-wide, and only it un-routes on a change of base: a change of `[model] live` in the config, or of a place's
bound profile, leaves a routed session routed (fixing it needs a stored field, a format bump and a one-way install,
kept out of a hotfix), and a pane session started with `ask -P glm` returns to the live profile, not to glm, once
routing stops. `private_addresses = "open"` lifts the rebinding protection too, by design, for a container with no
operator. The existing route tests are timing-fragile under IO pressure: with a neighbour lane's store plants running
(IO full avg10 58 %), four failed on a verdict past the 200 ms wait, and passed in a quiet window.

**The joins.**
- **The route fix** (lock `lane-route-fix-join`, taken and merged in one guarded call at 20:56:17, the queue free and
  main equal to origin/main 3c85ecee). `git merge --no-ff -S` of the lane at 6e59dac0, clean, giving **645769d2**
  (good signature); `turn.rs` merged at 3,452 lines against its 3,523 ceiling. The warm exited 0 at 20:58:38. **The
  gate** (exit 0 at 21:04:43, the lock held 297 s): suite **2,491 of 2,491** (one slow); lifecycle ok in 22.6 s; turn
  frames **5 and 9**, plain p50 75.0 and p95 84.2 ms, tool call 164.2 and 181.3 ms (main before, 3c85ecee: 78.3 and
  81.7; 169.5 and 262.2); deny ok. Pushed, the done line; theseus-9yyr closed with the hash. The chain log's install
  line gives the join as 21:05.
- **The bench fixes** (lock `lane-bench-fixes-join`, taken 21:26:56 queued behind `lane-linux-io-join`, whose merge
  e6378de8 was gating; the lane waited only on that lock, and merged in one guarded call once linux-io had pushed and
  written its done line). `git merge --no-ff -S` at 7c77c698 onto e6378de8, giving **42a8222f** (good signature); the
  only textual overlap, `crates/theseus-core/AGENTS.md`, auto-merged; shape ok (`turn.rs` 3,458 of 3,523, `config.rs`
  2,827 of 2,910). The warm exited 0 at 21:35:10. **The gate** (exit 0 at 21:43:02, after 165 s waiting for the lock
  behind linux-io's lifecycle A/B): suite **2,510 of 2,510** (one slow); lifecycle ok in 22.5 s, cold start p50 21.1
  and p95 27.0 ms, clean shutdown 30.5/43.7, SIGKILL and restart 25.3/26.5, swap 49.9/58.5; turn frames **5 and 9**,
  plain p50 74.6 and tool call 166.6 ms. Pushed, the done line; theseus-7gir.19, .20 and .21 closed, and theseus-7gir's
  notes say a held-out rerun is owed, with .19's spend warning. The worktree, the branch and the lane's 23 GB target
  were removed.
- **The review** (the DM thread, 21:46, accepted): the route fix as above ("no pin, per 25e"); for the bench fixes,
  the owner's daemon's defaults unchanged (`private_addresses` asks, no retries).

**The install.** The route fix went in at install #3 (21:22:21 to 21:22:30, at 645769d2, with card 2 and b5's docs:
Item 149), the one install the night allowed: health ok, `route.v1`, `rerank.v1` and `security.v3` live, 9
secrets ready, discord ready; startup serving at 417 ms with two lanes compiling (neighbour IO; 29.1 ms at install
#2). The owner had run no turn since 20:00 (turns 30), so `route.v1` had no live decision yet. The bench fixes change no
default of his daemon; they were installed 2026-10-05 13:07 at 60b43fb6, install #4. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** "Clear" rather than "ignore" for a routed session once routing stops acting (the lane's choice, for
the reasons above). A new META key rather than a stored per-session field, so no format change. `.20`'s knob is the
issue's option 1 (a config key) and `.21`'s the same; the lane changed only the cap for .19, as its brief and the
issue asked, and left the spend rule to the DM thread.

**Known gaps.**
- **theseus-17jn** (9yyr's item 2, P2, waiting for the owner): while `route.v1` acts, `chat`, `other` and a fallback
  resolve to `Ask.base`, which for a routed session is its routed profile, so it stays where routing last put it (the
  cache-warm reading of 25e's "else the session's own"). The morning notes' section 31 settled it as built, for the owner
  to overrule.
- **theseus-0j2.17** (P2): a routed session following its base when the live profile or its place's profile changes,
  and a pane's `-P` profile coming back after routing (the lane's item 3), taken up by batch 7's route-gaps row.
- **theseus-7gir.22** (P1): the held-out rerun of the landed loss fixes, at spend parity with Claude Code; the DM
  thread settled the spend rule (the rerun counts actual spend, as Claude Code does; the morning notes' section 31).

### Item 151. linux-io: a job's completion takes one sync, background passes wait while the machine is busy, and language servers watch their own files (theseus-yxiv, theseus-tood and theseus-m9hj, the Linux survey's cards 1, 4 and 6, with the cause of theseus-c6hv; the `linux-io` lane, a subagent of the DM thread, branched from 3c85ecee once card 2 had joined, in a worktree; ab94330e, b334e485 and 6e46cd05; joined 21:32 at e6378de8, a signed merge onto 645769d2 made 21:23:00, by the lane itself; reviewed 21:46 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Three more of the Linux survey's cards (theseus-779n, reviewed 15:19, when the owner approved picks 1 to 4 and 6
as recommended):
- **Card 1, one sync instead of two.** A job's wrapper wrote `<id>.json.tmp`, fsynced it, renamed it, then fsynced
  the spool directory: two syncs before it poked the daemon, with the daemon's WAL write a third. At the gate's 6.5 ms
  fdatasync, about 13 ms of an L1 job's 16.7 ms dispatch-to-completion p50 was those two syncs. O_TMPFILE saves
  nothing; the lever is the number of syncs.
- **Card 4, background work yields.** The background passes had fixed throttles that never looked at the machine: the
  learning loop at nice 19 on a 5 % duty cycle, the durability check at 5 %, the terms and shape stretches at normal
  priority on the blocking pool, the index tender in the idle I/O class, which does nothing here, since every disk's
  scheduler is `none`.
- **Card 6, servers learn about files that jobs change.** The client advertised `didChangeWatchedFiles` with
  `dynamicRegistration: true` and recorded the servers' registrations, but watched nothing, so a server that trusted
  it (rust-analyzer's default) stayed stale for every file the client had not opened after a job's `git checkout`,
  `cargo fmt` or code generator.

The brief also gave the lane theseus-c6hv, R4's finding that rust-analyzer gave no diagnostics through Theseus at all.

**What landed** (three signed commits; the merge 25 files, +1,130 −116).
- **Card 1** (ab94330e; `crates/theseus-kernel/src/spool.rs`, `kernel.rs`, `tests_spool.rs`). `Spool::write` is
  `write_synced` (the tmp written, `flock`ed and fsynced) then `Synced::publish` (the rename), with no directory sync.
  On ext4 (this disk is `data=ordered`) and xfs a new file's fsync commits the journal transaction that made its name,
  so after it the tmp is durable under its own name and a machine crash can lose only the rename. **The recovery**
  (`Spool::drain_recovering`, the start's step 3, in place of `drain`) takes a `.json.tmp` only when its lock is free
  (its writer has gone), it parses as a whole `Completion`, and that completion names the file's job; it renames it
  into place and drains it with the rest, and leaves a torn tmp, a live writer's and one naming another job as they
  are (a torn one is unknown at the reconcile, as before). The recovery syncs nothing: a rename a crash loses is made
  again at the next start. The heartbeat's `drain` never reads a tmp. A lock, not the pid file, because the daemon
  writes a wrapper's pid file only after spawning it; the lock is exact, needs no `/proc` read, and costs one `flock`
  a job. **A settled completion is a no-op at the start:** step 3 used `accept_completion`, which wrote a
  `completion.duplicate` frame for a redelivered completion before serving; it now uses `take_completion_with`, as the
  heartbeat's drain did, and writes nothing. `StartupReport.spool_recovered`, step 3's ledger row and the start's log
  line count the recovered ones. `bench jobs` reports `notified` (the dispatch to the wrapper's poke) beside `total`,
  which stops at the rename and so never saw the directory sync.
- **Card 4** (b334e485; `crates/theseus-store/src/pressure.rs`, new, 265 lines with its tests). `busy()` reads `some
  avg10` of `/proc/pressure/cpu` and `/io`, busy at or over the gate's own settle thresholds (CPU 20 %, IO 10 %), and
  never without PSI; `quiet(bound)` waits on tokio's timer and `quiet_blocking(_unless)` on a thread of its own, each
  looking every second up to `BOUND`, 10 s a chunk (one avg10 window), so a machine that stays busy still gets its
  passes done, at most 10 s later a chunk. Four passes wait **between** chunks (a pass's first chunk goes at once, so a
  one-chunk pass never waits): the WAL's history check between 4 MiB stretches (`store.verify` gains `yielded_ms`),
  the index's terms and shape builds (their outcomes gain `yielded_ms`; a wait holds neither the core nor the store),
  the nightly learning run between packs (ending a wait at a stop), and the index tender's embedding thread between
  two pieces of work (`VectorConfig::yield_bound`, zero in the vector tests, whose own load is the pressure). Only the
  nightly learning run's thread takes SCHED_IDLE (`learning::tender::on_idle_thread`, nice 19 then SCHED_IDLE).
  `theseusd check` gains a line: "background: background passes wait while CPU pressure is 20 % or more, or IO 10 %
  (PSI); the state's disk, sdd, schedules with none, which ignores I/O priorities: the index tender's idle I/O class
  changes nothing there". It lives in theseus-store because theseus-core and theseus-index both depend on it, and adds
  `libc` (already locked).
- **Card 6** (6e46cd05; `crates/theseus-lsp/src/client.rs`). `client_capabilities()` declares
  `didChangeWatchedFiles: { "dynamicRegistration": false }`, so rust-analyzer runs its own watcher; the client still
  announces its own writes (`file_changed`). The fake records the client's capabilities.

**How it is proven.**
- **Card 1.** `a_completions_write_syncs_once` (a thread-local counter reads 1, was 2); the recovery's guards (a live
  writer's synced tmp, a torn one and one naming another job stay; the dead writer's is recovered; the heartbeat reads
  none); and **a real kill -9** (`tests_spool.rs`: the test binary runs itself again as a wrapper stand-in, calls the
  real `write_synced` and `publish`, stops itself with SIGSTOP at the point, and is SIGKILLed there): killed before its
  rename, the tmp is whole, the start reports recovered 1 and drained 1, the action Succeeded with `completions_seen`
  1, and the next start recovered 0, drained 0, writing one frame; killed after it, drained as always; a recovered
  completion whose job was settled writes nothing (the action byte-equal, no `completion.duplicate`). strace of the
  real wrapper, one `/bin/true` job: main 2 syncs, 1 rename, 0 flocks; the lane 1 sync, 1 rename, 1 flock.
- **Card 4.** `pressure::tests` (the PSI line read as the kernel writes it; 19.99/9.99 quiet, 20.0 CPU and 10.0 IO
  busy; on the paused clock 0 s quiet, exactly 3 s for three busy looks, 10 s busy for good, 2.5 s at a 2.5 s bound;
  SCHED_IDLE on one thread only); a fake-PSI test on the real `/proc/pressure` (the binary under `unshare -rm` with a
  temp directory bind-mounted over it: busy then quiet after 1.5 s went at its next look, 2.0 s; IO 80 for good went
  at its 2.5 s bound; quiet, no wait); and `the_run_takes_a_low_priority_thread_of_its_own`, reading the policy from
  `/proc/self/task/<tid>/stat`: the nightly run's thread SCHED_IDLE (5) at nice 19, an owner's run's SCHED_OTHER at
  nice 19, the caller's untouched.
- **Card 6.** A Python LSP probe against rust-analyzer 1.98.1 first proved the premise: with main's capabilities it
  registered two watchers with the client, and after another process rewrote `ledger.rs` the open `lib.rs`'s pull
  stayed empty for 10 s; with the claim off, `E0308 expected i32, found u64` came 0.5 s after the write. Then the
  fake's test, a live test (`live_rust_analyzer_sees_a_file_a_job_wrote`, ignored like the others: passed, the error 5
  ms after the write; with the old claim planted back, "lib.rs still clean 30 s after the job rewrote ledger.rs"), and
  a live check on scratch daemons (transient user units, GLM live, `[lsp] enabled`): a real `proc.run` job's `sed`
  rewrote `ledger.rs`, and the next `lsp.diagnostics` on `lib.rs` read "no diagnostics at all" on main 3c85ecee and
  `error [E0308]: expected i32, found u64 (rust-analyzer)` on the lane (6 GLM turns, about $0.007).
- **Planted reverts, eight, all caught** (`harness/revert.py`, each restored with a fresh mtime): the start's drain
  without recovery (3 tests), the start back on `accept_completion`, the directory sync put back, the writer's lock
  removed, the old watch claim (card 6), the async wait and the thread wait made never to wait, and no SCHED_IDLE.
- **The A/B** (main 3c85ecee against the lane, frozen copies, A B B A A B B A in one exclusive hold of the gate lock a
  round, two rounds, 40 jobs a class a run; the disk's 4 KiB fdatasync 6.38 then 6.51 ms; medians of eight p50s, exact
  Mann-Whitney): an L0 job's dispatch to the poke **16.91 to 10.31 ms** (−6.61), an L1 job's **23.21 to 16.52** (−6.69),
  each one of this disk's fdatasyncs, and the turn bench's tool-call turn, which runs a job, **179.55 to 160.55 ms**
  (−19.0), each at U 0, p 0.0002; the plain turn 78.05 against 76.90 (p 0.63, noise). On the lane `notified` equals
  `total` to 0.01 ms. Why the tool-call turn gains more than one sync: the job's directory fsync forced a journal
  commit in the middle of the daemon's own WAL syncs (an inference, not measured). Frames held at 5 and 9.
- **The lane gate** (`THESEUS_GATE_NO_BENCH=1`, niced): 2,499 of 2,499 in a 367 s suite, every check passing. A first
  run was stopped by the lane in its lock wait, once it found that the owner's runs share the learning thread's
  helper (below).

**What the lane found.**
- **Departures from the brief, each with its reason.** The embedding thread is **not** in SCHED_IDLE: it serves (a
  waiting query sets `want`, and the thread loads the model for it), and candle's matmuls run on rayon's global pool,
  whose threads take the policy of the thread that first uses them, so SCHED_IDLE, which is one-way, could slow query
  embeddings and recall on exactly the busy machine card 4 is about; it keeps nice 19 and the waits. The owner's runs
  that share the learning thread's helper (`judge.replay`, `.audit`, `.backfill`) keep nice 19 alone, since the owner
  awaits them. A pass's first chunk goes at once.
- **theseus-c6hv's cause** (not fixed here). rust-analyzer declares pull diagnostics, so `Client::diagnostics` took
  the pull path and never read pushed lists; its pull carries only its own analysis, which reports neither of R4's
  errors (`a + "b"`, E0277, and `a + c`, E0425: pulled `full`, 0 items); rustc's errors come only by push, from its
  `cargo check` (flycheck), versioned, about 150 ms after a `didSave`. Two more parts of the gap: a check runs only after
  a save, and `file_changed` saved only an open file; and the edit's wait pulled right after the save, before the check
  began. The fix, about 120 lines with tests, was designed in the issue's notes at 20:52 and became its own lane
  (Item 153); until it landed, the owner's `[lsp.servers.rust-analyzer] start_on_edit = false` stayed.
- A trap, now in theseus-lsp's `AGENTS.md`: under `cargo nextest`, rustup's `rust-analyzer` proxy inherits
  `RUSTUP_TOOLCHAIN` (the pinned 1.98.1, which has no rust-analyzer), so the live tests' server exits at once; point
  `THESEUS_LSP_RUST_ANALYZER` at the stable toolchain's binary.

**The join.** Lock `lane-linux-io-join`, taken 21:21:09, queued behind the DM thread's `install-3-645769d2` until its
done line at 21:22:53. One guarded call (main equal to origin/main, a clean tree, no `MERGE_HEAD`), then `git merge
--no-ff -S`: **e6378de8** on 645769d2 (the route fix of Item 150), `crates/theseus-core/AGENTS.md`
and `rpc/mod.rs` auto-merging with no conflict, as merge-tree had said. The warm ran 21:23:14 to 21:26:47 (the test
build 2 min 25 s; clippy clean). **The gate** (21:26:47 to 21:31:36, 289 s; the lock held 272 s with no wait; the
settle found the machine quiet): suite **2,504 of 2,504** (one slow, the floor-repeat wake test at 63.6 s); every check
passing; cold start p50 20.8 and p95 23.1 ms; the start's spool step 0.03/0.05 ms, unmoved; jobs' L1 start 5.04/5.55
ms; turn frames **5 and 9**, plain p50 76.7 and tool call **153.5 ms** (164.2 at the route fix's gate 27 minutes
earlier). The stop-side phases read slower at this gate (clean shutdown with a job 53.5/66.7 ms, SIGKILL and restart
32.1/37.1, swap 61.0/82.7), each inside its budget, though nothing of the lane runs on a stop; the same gate's start
clocks showed disk stalls (the store phase's p95 25.6 against 14.1 ms). So the lane ran **a lifecycle A/B** after the
join (main 3c85ecee against the lane, `bench lifecycle --runs 10`, four runs an arm in one hold, 21:35:27 to 21:38:29):
cold start 22.45 against 21.75 ms, clean shutdown 32.25 against 32.00, SIGKILL and restart 26.05 against 25.35, swap
47.70 against 46.75: no slower anywhere, the gate's readings the machine's. Pushed 21:32:41, the done line 21:32:46;
theseus-yxiv, theseus-tood and theseus-m9hj closed at 21:33; theseus-c6hv's cause, evidence and design appended to its
notes. The DM thread's review at 21:46 accepted it and cleaned the lane; "keep `start_on_edit = false` until
theseus-c6hv lands"; not installed that night.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. On a start after it, a completion's tmp whose writer died
before its rename is finished and drained; `theseusd check` prints the background line; rust-analyzer watches its own
files. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The three departures above (the embedding thread and the owner's runs keep nice 19 without
SCHED_IDLE; first chunks go at once; a zero yield bound for the vector tests). Card 1's `total` could not drop, as the
survey expected; `notified` is where the sync shows.

**Known gaps.** The kernel sim writes completions with `Spool::write` but has no crash point between a completion's
sync and its rename; the kill-9 stand-in covers it. Two possible follow-ups, not filed: the embedding thread's backfill
on a SCHED_IDLE thread of its own (rayon's pool built from a normal thread first), and whether rayon's workers already
inherit nice 19 from that thread. `theseusd check`'s line says "background" twice (a wording nit).

### Item 152. Card 5, the light job cgroup: each L0 job in a threaded cgroup of its own where the daemon's is delegated, born inside by `clone3`, with a process cap and an exact stop (theseus-a5nv; the Linux survey's card 5, approved by the owner at 15:14 on a budget of about 250 to 300 production lines, and taken as built at 22:12 at 520; the `linux-jobs` lane, spawned 2026-10-04 17:55, in a worktree; c3b25a1c and 20c7f106 on card 2's bff95fef; reviewed 19:53 by the DM thread and held for the owner's call; joined 22:41 at 23fb7f37, a signed merge onto 42a8222f made 22:21, by the card-5 joiner; the join reviewed 22:50 and 23:20 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4, the unit taking `Delegate=yes`)

**Why.** The sandbox trims of 2026-10-03 (Tier 4.3, Item 77) cut L1's delegated cgroup, about 800 lines across four
crates: a daemon moved into a leaf, `memory` and `pids` for the jobs, a `cgroup-release` stop hook, and the
219/CGROUP failure class the hook existed for (with `+memory` on, a restart while a job lived failed "Failed to attach
to cgroup ...: Device or resource busy"). Since then L0, the default class, had no process cap and stopped by the
wrapper's tree walk (SIGTERM, a SIGSTOP freeze rescanning for up to 500 ms, SIGKILL), and L1 relied on its pid
namespace and `RLIMIT_NPROC`, from which root is exempt (theseus-pv6i). The survey's card 5 proposed a light cgroup:
`Delegate=yes` on the unit and nothing else, an exact O(1) stop and a process cap for every job, L0 included, with no
memory controller (the part that needs the leaf), at about 250 lines with tests. The owner approved it at 15:14 "on the
condition it is lightweight and its weight is paid for by its utility": about 250 to 300 production lines, no new unit
hooks, job start within its target.

**What the lane found: two of the survey's premises do not hold on this kernel** (probes in delegated scopes and
transient services, all removed after).
1. **A job's cgroup must be threaded, and a threaded cgroup has no `cgroup.kill`.** With the daemon in its unit's
   cgroup and `+pids` on, the kernel makes that cgroup a thread root, and a domain child of a thread root cannot take a
   process ("domain invalid"). The survey's "`+pids` worked" was the enable alone: its probe turned `+pids` on and off
   at once and never populated a child with it on. In a threaded child, `cgroup.kill` and reading `cgroup.procs` are
   ENOTSUP, while `cgroup.freeze`, `cgroup.events`, `cgroup.threads`, `pids.max` and `cpu.stat` work. A crash and a
   `systemctl --user restart` both start the next main process in the root with the job alive and its `pids.max` kept
   (the restart's attach checks `cgroup_can_be_thread_root`, which only populated *domain* children fail).
2. **"One write of `0` to `cgroup.procs`" is not cheap.** The first migration after a quiet spell waits for an RCU grace
   period (`rcu_sync_enter`, on `cgroup_threadgroup_rwsem` taken for writing; this kernel has
   `CONFIG_CGROUP_FAVOR_DYNMODS` unset and cgroup2 mounted without `favordynmods`): a move in after 400 ms idle took p50
   15.8 ms, p95 35.5, max 39.9 (threaded). A job's start would pay that every time, so the command is born inside with
   `clone3(CLONE_INTO_CGROUP)`, which takes the rwsem for reading only, and which std's `Command` cannot do.

**What landed** (c3b25a1c, 31 files, +1,295 −94, and 20c7f106, the tests' cleanup, +21; the merge 31 files, +1,322
−95). **520 Rust code lines of production**, net, without comments, blanks or tests (498 without the bench tool's 22),
and 395 of tests (the lane's `countlines.py`):
- **The spawn** (`crates/theseus-kernel/src/spawn.rs`, +155): `clone3(CLONE_VM|CLONE_VFORK|CLONE_INTO_CGROUP)`, a PATH
  search, the umask, and an errno pipe. The clone is inline asm in `spawn()` itself, for x86_64 and aarch64 (the
  sandbox's two arches): a vfork-style `clone3` called through libc's `syscall()` crashed the parent with SIGILL, since
  the child returned from `syscall()` and its next call pushed over the return address the parent took on waking. It
  replaces card 2's umask around the spawn (Item 149), so every L0 command starts the same way, delegated or not,
  and every job test exercises it.
- **The job cgroup** (`crates/theseus-kernel/src/cgroup.rs`, +156): `ready`, make (`job-<id>`, threaded, `pids.max`),
  usage, the stop loop on `cgroup.events`, remove. **The stop** is a loop, since there is no `cgroup.kill`: SIGTERM to
  each process, the grace, `pids.max` 0, then SIGKILL to each listed task until `populated 0`, waiting on the pollable
  `cgroup.events` with no rescans and no sleeps.
- **The wrapper's wiring** (`job.rs`, `lib.rs`, `job_l1.rs`, +66): `WrapperArgs.cgroup`, `--cgroup` and `--pids-max`,
  `spawn_l0`, the stop's cgroup branch (`stop_tree` stops by the cgroup when there is one), the completion's counts
  (`cgroup_usage` adds `cpu_us`, `pids_refused` and `pids_max` to the detail), and the cgroup's removal after the copy's
  end. The daemon and the wrapper stay in the unit's root; only the command is born inside, so a cgroup stop never
  kills the writer of the completion.
- **The daemon's check** (`crates/theseus-core/src/cgroup.rs`, `lib.rs`, `theseusd/src/main.rs`, +72): `systemctl show
  -p Delegate` 2 s after serving, `ready`, and a `cgroup` startup phase, from which health's line is read (no protocol
  change). A restarted daemon's `ready` removes only empty `job-*` directories.
- **`[tools] job_pids_max`** (default 4,096, 0 for no cap) and the result's words (`toolrun::job::cap_line`), +20.
- **Health's `cgroup:` line** (`theseus/src/render.rs`, `render/sandbox.rs`, +26): `cgroup: delegated · each L0 job in
  its own, up to <n> processes and threads · <the unit's cgroup>`, or what it fell back to and why (`cgroup: none · a
  job stops by its process tree · <unit> is not delegated (Delegate=no): its unit needs Delegate=yes, as theseusd install
  writes`), and the startup line's `after: … cgroup N ms`.
- **The units' `Delegate=yes`** (`theseusd/src/install/layout.rs`, +3): `SERVICE_COMMON` (both `--user` and
  `--separate`) gains two comment lines and the line, and nothing else; the three goldens gain the same three lines; the
  installer's and `user_service_script.rs`'s tests assert `Delegate=yes` and still no `ExecStopPost=`;
  `docs/user-service.md` says what the line gives, and that a unit written between theseus-gyin and this step has no
  `Delegate=` and needs `user-service.sh install` again. **No new unit hooks.**
- **`bench jobs --cgroup`** (`theseus-sim`, +22, a tool), and **the proofs** (`theseusd/tests/cgroup.rs`, +314 and the
  cleanup's 21).
- **Not counted**: the template line, the goldens, the docs, the AGENTS.md files and the regenerated cockpit type
  (`CancelVerdict.ts`).

Without premise 2 (a move instead of the spawn) the card would have been about 365 lines; with `cgroup.kill` as well,
about 320. L1 jobs get no cgroup: they keep their pid namespace's stop and `RLIMIT_NPROC`.

**How it is proven.**
- **The three tests** (`theseusd/tests/cgroup.rs`, each in a transient delegated unit or scope): a fork loop that
  ignores SIGTERM (40 sleepers, one `setsid`, one double-forked) is stopped `verified_by: cgroup`, `scope: cgroup`, 0
  survivors, at least 43 killed, its cgroup removed; a unit restarts while a job lives in its cgroup (`theseusd` as a
  transient user service with `Delegate=yes`, `KillMode=process`, `Restart=on-failure`, SIGKILLed: `NRestarts=1`, a new
  main pid, `ExecMainStatus` not 219, `delegated` again, the job alive); a job past its cap of 10 with 30 sleepers has
  `pids_refused` > 0, `pids_max` 10 and `cpu_us` in its detail. The cleanup commit makes them wait for the job's cgroup
  and remove an empty one: card 5's first gate run of the cap test had left its scope's empty cgroup.
- **Live, in the lane** (a scratch daemon in `systemd-run --user --scope -p Delegate=yes`, a fake model scripting
  `proc.run`): the four proofs the joiner repeated on the merged build (below), the stop reading `verified: cgroup, 19
  processes` with its 2 s the grace the loop ignores. Also the fallback: the same daemon started plainly, inside
  another service's unit, let the uncapped loop reach 208 processes and stopped it `verified: process tree, 208
  processes`.
- **strace**: in a delegated scope with `--cgroup`, the wrapper's spawn of its command is `clone3({flags=CLONE_VM|
  CLONE_VFORK|CLONE_INTO_CGROUP, …, cgroup=9}, 88)`: born in its job's cgroup, nothing copied, nothing moved.
- **The lane's A/B** (four arms in one settled hold of the gate lock, palindrome order, twice; medians of four runs):
  the cgroup cost **+0.20 ms** (0 MB held) and **+0.22 ms** (1 GB) on an L0 job's p50 (14.29 against 14.09 and 14.20
  against 13.98 ms), within the run-to-run spread; §2.2's L1 start p95 read 6.6 to 8.3 ms in every arm.
- **Lane gates**: card 5's first run passed all 2,450 tests and failed only its check that the regenerated cockpit type
  was staged, then green in 386 s, frames 5 and 9; the cleanup commit green in 390 s, leaving no test cgroup behind.

**The owner's call** (asked as 2 of 5 in the walk-through he asked for at 21:56, with the morning notes' sections 30 and
31): take it as built at 520 lines; move the process instead of the custom spawn (−150 lines, +16 ms p50 and up to 40
ms on every L0 start: not FAST); or drop the cap (−50 lines, loses the cap and a root daemon's). At 22:12, "A please":
as built, all four abilities (the exact stop, a restart with a job running, the per-job process cap, the health line)
at +0.2 ms a job. The same minute the operator's install script was changed to accept `Delegate=yes`
(refusing any other `Delegate=` value or an `ExecStopPost=`) and to show health's `cgroup` line.

**The join** (the card-5 joiner, spawned at 22:14; lock `lane-linux-jobs-card5-join` taken 22:15:36 in one guarded
call). `git merge --no-ff --no-commit lane/linux-jobs` at 20c7f106 onto 42a8222f, the merge base bff95fef (card 2), so
it brought card 5's two commits; **23fb7f37**, signed, no lane commit rebased or re-signed. **One textual conflict**,
`crates/theseus-sim/src/jobs.rs`, kept both: main's `Rig` and `Run` (linux-io's `notified`, Item 151) with
card 5's `job_cgroup` and a `(sandbox, cgroup)` pair passed down; `git rerere` recorded it. **`job.rs` needed nothing**:
main had not touched it since card 2, so the merged file is card 5's byte for byte, and the interaction with card 1 is
semantic: the wrapper writes its completion through card 1's single-sync `Spool::write` after the command's spawn and
exit, the copy's wait and `cgroup_usage`, so no job process can hold the tmp's descriptor or lock, and a stop's path
writes `write_stop` and no completion. Shape ok (`config.rs` 2,835 of 2,910; `render.rs` 3,032 of 3,100; no ceiling
raised). The warm (22:17:19 to 22:20:47) clean. **No join fix.**
- **The joiner's tests** (nice 10, queued 3 minutes behind lane lsp-checks' gate): the kernel suite three times (150
  passed each, theseus-g11i's tree test at 1.05 s each), card 1's four crash tests, and theseusd's `cgroup`,
  `job_wrapper`, `sandbox` and `user_service_script` tests, 54 of 54; the three cgroup tests really ran.
- **The four live proofs on the merged build** (a scratch `theseusd` as the transient unit `theseus-c5j-live-unit`
  with `Delegate=yes`, `KillMode=process`, `Restart=on-failure`): health `cgroup: delegated · each L0 job in its own, up
  to 24 processes and threads · …`, the startup's `cgroup 4.1 ms` (3.4 and 3.3 after restarts), the unit's cgroup
  `domain threaded` with `subtree_control pids` and the daemon alone in it; the fork loop stopped `verified: cgroup, 18
  processes` (its row `{"killed": 18, "ms": 2001, "scope": "cgroup", "survivors": 0, "verified_by": "cgroup"}`), 0
  survivors anywhere on the machine, its cgroup gone; a job starting 40 sleepers under a cap of 24: `← proc.run error ·
  exit 2 · 214 ms · 19 B: [exit code 2] ⏎ [its cap of 24 processes and threads ([tools] job_pids_max) refused it 1 new
  one: what it did may have failed for that] ⏎ sh: 0: Cannot fork`, its detail `pids_refused 1`, `pids_max 24`, `cpu_us
  16238`; `kill -9` of the main pid with `sleep 307` alive: the journal's "Unit process …
  remains running after unit stopped", "Found left-over process … Ignoring", `Started` one second later, no 219 line,
  `NRestarts=1`, serving at 33.7 ms, the job alive, then stopped `verified: cgroup, 1 process`.
- **An extra proof, card 1's recovery with card 5's spawn**: a real wrapper whose command was born in its cgroup, cut
  between its sync and its rename (a directory planted at its `.json` path, the daemon stopped alone under
  `KillMode=process`), left a whole, synced tmp with its flock free; the next start's step 3 read `{"drained": 1,
  "malformed": 0, "recovered": 1}`, the action settled once, the continuation read `← proc.run ok (late) · exit 0 ·
  20001 ms`, and the start after it recovered nothing and wrote no row naming the job; no `completion.duplicate` row.
- **`bench jobs` against main's** (A main 42a8222f from a throwaway worktree, B the merge without `--cgroup`, C with
  it; order A B C C B A twice in each of two holds; medians of eight p50s): the cgroup's own cost, B to C, **+0.26 ms**
  on an L0 job (10.74 to 11.00 ms, p 0.15), unchanged on L1, which gets no cgroup; `notified` equal to `total` to 0.01
  ms on every arm; L1 start p95 6.8 to 7.8 ms. A to B (+0.24 ms L0, +0.66 L1, neither significant at 8 against 8) read
  as a build-to-build offset between debug builds from different trees, since card 5 changes nothing on an L1 job's
  path.
- **The gate** (22:34:17 to 22:39:21, 304 s, quiet at once, so strict): suite **2,516 of 2,516** (one slow); cold start
  p50 21.2 and p95 23.9 ms; clean shutdown 35.7/44.6; SIGKILL and restart 25.5/26.5; swap 47.5/51.6; the spool step
  0.03/0.06 ms; jobs' L1 start 7.18/9.23 ms; turn frames **5 and 9** (plain 74.3, tool call 153.9 ms). The jobs bench's
  L1 total read **24.13 ms** p50 against the last gate's 16.54: the turn bench's first fdatasync probe, just after it,
  read 14.9 ms against the usual 6.3 to 6.6, and the gate's own command rerun with a 50-sync probe before each run read
  12.6 to 12.9 ms fdatasyncs and 17.4 to 20.9 ms totals, then, once the disk was back, main against the merge in one
  hold 15.95 against 16.66 ms (the same offset as above; the lane's own frozen builds, card 2 against card 5, read 16.67
  against 16.43, no difference). Pushed 22:41:59, the branch deleted on origin and locally, the done line 22:42:05;
  theseus-a5nv closed; theseus-g11i noted (4 of 4 on the merge). Lane lsp-checks, queued behind this lock since
  22:28:17, merged at once (Item 153).
- **The review** (the DM thread, 22:50 and again 23:20, accepted): the cut proved live with a real wrapper; for the
  morning install, `user-service.sh install` puts `Delegate=yes` in the unit (the install script accepts it), health must
  read `cgroup: delegated ... up to 4096 processes and threads`, and `[tools] job_pids_max` is new.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change; one new key, `[tools] job_pids_max` (4,096). The install's unit step (`theseusd install --user --apply`, then a `daemon-reload`) gave the unit
`Delegate=yes`, and after the restart (13:07:43) health read `cgroup: delegated · each L0 job in its own, up to 4096
processes and threads · <the unit's cgroup>`. Until then it had said `cgroup: none … not delegated`, and jobs ran as
before. Restarts with jobs running are safe (no 219/CGROUP). Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** 520 production lines against the 250 to 300 budget, taken as built by the owner: the two premises above,
not extras, made the weight. Threaded job cgroups and a stop loop in place of domain cgroups and `cgroup.kill`; a spawn
of the daemon's own in place of a move by `cgroup.procs`. The cgroup is `job-<id>` beside the daemon, not the survey's
`jobs/<corr>`. A cgroup that cannot be made falls back to none, with `cgroup_error` in the detail; a clone into the
job's cgroup that fails fails the job (`spawn: <error>` in its result), where falling back on the clone too would be a
few lines.

**Known gaps.**
- **Terminals** keep their `pre_exec` (card 2's gap). **L1 jobs** get no cgroup; putting the init in the job's cgroup
  (`CLONE_INTO_CGROUP` in theseus-sandbox's clone) would give a root daemon's L1 jobs a process cap, the missing piece
  for lifting theseus-pv6i's refusal.
- glibc 2.39's `pidfd_spawnp` with `posix_spawnattr_setcgroup_np` would replace `spawn.rs` with about 20 lines; this
  machine has 2.35.
- Not filed, from the joiner: a wrapper cut between its sync and its rename never reaches `remove_pid`, so its
  `spool/pids/<id>` outlives the recovered completion (harmless; it predates both cards, and card 1's recovery could
  remove it); and a `sync` and a settle before the gate's jobs bench, as the lifecycle bench has, would steady its
  totals.
- The survey's card 5 needs correcting on both premises (the lane's note for theseus-779n).

### Item 153. lsp-checks: rust-analyzer's compiler errors reach the harness, a saved document waiting for the check after its save (theseus-c6hv; local reviewer R4's finding at batch 5's smalls-tools review, its cause found by lane linux-io, its design the issue's note of 20:52; the `lsp-checks` lane, 2026-10-04 21:47 to 22:52, in a worktree; 21c73294; joined 22:49 at 7ad8a8bd, a signed merge onto 23fb7f37 made 22:42:17, by the lane itself, queued behind card 5's join; reviewed 23:20 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4, with the owner's `start_on_edit = false` for rust-analyzer taken out)

**Why.** R4, reviewing batch 5's smalls-tools, saw rust-analyzer give no diagnostics through Theseus at all, on main
and on the branch alike: an edit making `a + "b"` (E0277) and one making `a + c` (E0425) both read "no errors" and "no
diagnostics" (filed as theseus-c6hv, 17:53). Install #2 therefore set `[lsp.servers.rust-analyzer] start_on_edit =
false` in the owner's config, "until theseus-c6hv". Lane linux-io found the cause (Item 151): rust-analyzer
declares pull diagnostics, so `Client::diagnostics` took the pull path and never read pushed lists; its pull carries
only its own analysis, which reports neither error, and rustc's errors come only by push, from its `cargo check`
(flycheck), versioned, about 150 ms after a `didSave`. Two more parts of the gap: a check runs only after a save, and
`file_changed` saved only an open file, so the first edit of an unopened file triggered none; and the edit's wait
pulled right after the save, before the check had begun. Its design, about 120 lines with tests, was appended to the
issue at 20:52, and the lane built it.

**What landed** (one signed commit, all in `crates/theseus-lsp`; 10 files, +816 −31: the client side +297 −15 with its
doc comments and a unit test, the fake +166, the tests +332; theseus-core needed no change; tokio's `test-util` in the
dev-dependencies, no new crate, `Cargo.lock` unchanged).
- **`Options::check_token`** (`client.rs`, `servers.rs`): the progress-token prefix of the check a server runs after a
  save. rust-analyzer's preset sets `rust-analyzer/flycheck/`; ty, pyright, basedpyright, tsgo,
  typescript-language-server and an operator's own server have none.
- **The bookkeeping.** `Doc::saved` (`docs.rs`): the `didSave` of the document's current version, as the count of
  messages sent with it and its instant, cleared by the next `didChange`; `State::last_save`, the connection's last
  save of any document; and `Checks` (`diagnostics.rs`), filled by `Shared::progress` for tokens with the prefix: those
  running, each with the sent-count at its begin, the latest begin, and the sent-count when the latest end arrived.
- **`Client::diagnostics`**, after its pull or push, for a server with a check token and a document whose current
  version was saved: it waits, inside the call's own deadline, until every check begun since the last save has ended;
  when none has begun within `CHECK_GRACE` (1 s), counted from the save or from the server's readiness if that came
  later, it takes the server to run none and returns the pull's answer; then it adds the list pushed for this version
  to the pulled list, each item once (on the push path the latest current push replaces the earlier one); at the bound
  it returns `Stale` with what it has, which L3 already turns into a pending wait. A merged answer is `Pulled`, so
  theseus-core's matches did not change.
- **`Client::file_changed`**: for a server with a check token, a file not open is opened and saved, so a check covers
  its first edit; the watcher is still told (`Created`).
- **The fake** gains `Diagnostics::PullAndCheck` (its pull answers `ERROR` lines; on a save, after a 50 ms debounce, a
  progress begins under `fake/check/0`, then each open document's `ERROR` and `CHECK` lines are pushed with its
  version, then the progress ends; a save during a check cancels it at once, as rust-analyzer's flycheck does), with
  `check_end_first` and `load_ms`, and the binary takes `--pull-and-check`, `--check-end-first` and `--load-ms`.
- **theseus-lsp's `AGENTS.md`**: a new invariant, "a saved document waits for its server's check", the traps (the
  c6hv line no longer open), and the tests. rust-analyzer's experimental native diagnostics stay off (they report both
  errors by pull, but are experimental, with known false positives).

**Where the code differs from the note** (the note says the code and `AGENTS.md` win; each difference has its test).
The grace counts from the server's readiness too: rust-analyzer starts no check while it loads its workspace (about 2
s in the live test, 4.6 s in the scratch daemon), so a grace from the save alone would have missed R4's first edit. The
wait follows the connection's last save, not only the document's: a save during a running check cancels it, and the
cancelled check's end must not answer the first document. A pull is asked again after the check's end, unless the
first went out after it, since a server may write a check's end before that check's push (on rust-analyzer one more
pull per edit, a few milliseconds). Size: about 300 client lines with their doc comments, against the note's 120 with
tests, and nine fake tests, not five.

**How it is proven.**
- **theseus-lsp's suite**, 41 tests, among them nine new ones against the fake on tokio's paused clock, each wait
  asserted exactly: a saved change waits for its check, 350 ms (the 50 ms debounce and the 300 ms check), and gets the
  pull's `ERROR` and the check's two `CHECK` items, the `ERROR` both reported appearing once; an unsaved resync waits 0;
  a 3 s check against a 500 ms bound is `Stale` at exactly 500 ms with the pull's list, and the next call has both; a
  server running no check returns at exactly `CHECK_GRACE`; a first `file_changed` opens (`didOpen` v1) and saves; a
  later save during a check waits for the next check (450 ms); a check whose end comes before its push still gets the
  push; a save while the server loads (2 s, by `serverStatus`) waits for the check after the load, exactly 2,150 ms;
  and, for FAST, a server without a check token waits 0 and opens nothing. A unit test holds that only rust-analyzer's
  preset names a check token.
- **Planted reverts, 15, all compiled and all caught** (each applied, the suite run, the file put back with a fresh
  mtime, checked byte for byte): the preset's token; a check's begin, its end, the save not kept, a change keeping the
  old save; the wait keeping its first save; `diagnostics` never waiting (main's behaviour, 7 tests); no deduplication;
  no re-pull; not stale at the bound; the grace from the save alone; no grace; `file_changed` opening nothing (6 tests);
  and the two FAST plants (a server with no token waiting, or having files opened). The first run of the revert script
  logged every plant as not proved, reading nextest's closing `error: test run failed` as a build error; its classifier
  now matches only compiler errors.
- **Live against rust-analyzer 1.98.1** (`live_rust_analyzer_reports_rustcs_errors_after_an_edit`, R4's two edits of a
  scratch crate, the first file not open and the server still loading): edit 1 `Pulled after 2.816406139s (version 1)`
  with `2:7 error [E0277] cannot add `&str` to `i32` (rustc)`; edit 2 after 146 ms with `error [E0425] cannot find value
  `c` in this scope (rustc)` and its hint. Three runs passed (edit 1 2.82, 2.27 and 2.24 s; edit 2 146, 141 and 137
  ms). Built with the fix switched off (main's behaviour), it fails as R4 saw: `edit 1: no E0277 from rustc: []`. The
  two older rust-analyzer live tests still pass.
- **R4's live check replayed on a scratch daemon** of the lane's build (a transient unit on a fresh state dir, GLM live,
  `[lsp] enabled = true` and **no** rust-analyzer line, so the preset starts it on an edit; R4's crate and four prompts
  in one session; about $0.005): edit 1 was `pending` (rust-analyzer started on the edit and was ready in 4.6 s, past
  the 1.5 s edit wait), and the next `lsp.diagnostics` had `error [E0277]: cannot add &str to i32 … (rustc)`, where R4
  had "no diagnostics"; edit 2's own result carried `error [E0425] … (rustc)` within the 1.5 s wait, where R4 had
  "(rust-analyzer): no errors"; the last `lsp.diagnostics` listed E0425 and its hint. Health at the end:
  `rust-analyzer on …/crate1 (… ready in 4.6 s, 630.5 MB, 4 requests, 2 edit results with its errors)`.

**FAST.** A server without a check token pays a lock read and an `Instant::now()` per `diagnostics` and a field write
per save, waits for no check, and opens nothing new. A turn that edits no file saves nothing and so waits for nothing.
Neither of L3's bounds moved: an edit still times out at `[lsp] edit_wait_ms` (1.5 s), and the check's wait runs in
L3's per-file task inside `diagnostics`' own bound (`request_timeout_secs`, 30 s). `[lsp]` is off by default and in
every bench, so the benches' paths do not see it. `lsp.request`'s span now includes the wait for the check for a saved
document; an operator who turns off rust-analyzer's `checkOnSave` waits out the 1 s grace on each saved document.

**The join.** Lock `lane-lsp-checks-join` taken 22:28:17, queued behind the card-5 joiner's (22:15:36); merged at
22:42:17 once card 5 was done (Item 152): `git merge --no-ff` of 21c73294 onto 23fb7f37, **7ad8a8bd**, signed.
The warm in main's tree (clippy `-D warnings` 41 s, the test build 86 s). **The gate** (22:44:44 to 22:49:39, 295 s;
strict, no rerun and no busy allowance): suite **2,526 passed** (19 skipped); shape ok (`client.rs` the largest at
1,022 lines); deny ok; lifecycle cold start p50 22.5 and p95 37.7 ms, clean shutdown 29.8/41.7, SIGKILL and restart
25.7/29.4, swap 73.4/92.6 (all within budget); jobs' L1 start 6.00/15.75 ms; turn frames **5 and 9**, plain 76.1 and
tool call 159.0 ms p50 (card 5's gate 74.3 and 153.9; the bench runs with `[lsp]` off). The lane's own gate had read
2,520 passed in 261 s. Pushed 22:49:55; theseus-c6hv closed with the hash and a note; the worktree, target and branch
removed. **The review** (the DM thread, 23:20, accepted): "rust-analyzer's pushed rustc diagnostics merge into
lsp.diagnostics and the edit's wait (a check token, the save's version, the grace from server readiness, the last save
of any file, a re-pull after a check ends; first edit of an unopened file opens and saves it)"; the owner's
`start_on_edit = false` comes out at install #4 (its plan of 23:20).

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change and no new key. Install #4 removed `[lsp.servers.rust-analyzer] start_on_edit = false` from
the owner's config (theseus-c6hv fixed), so the preset starts rust-analyzer on an edit again. What he was told first: on a large workspace most
edits will say `pending`, since most checks run past the edit's 1.5 s wait, and the errors ride on the session's next
edit or `lsp.*` result (L3's existing path); and rust-analyzer costs what it did before, about 4.2 GB resident on this
repository (`idle_stop_mins` frees it), its `cargo check` building in `target/rust-analyzer` (theseus-ext.12). The fix
adds a check only for the first edit of a file the client had not opened. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The three departures from the note above, and its size. Experimental native diagnostics stay off.

**Known gaps.** A pending edit's own result is left out of a following `lsp.diagnostics` that lists the file itself
(theseus-ext.12). rust-analyzer's memory and its check's build directory, as before.

### Item 154. The refusal fallback: a request Sonnet 5.5 refuses is made once more on Sonnet 5, inside its turn, and every surface says so (theseus-7gir.18; b5's loss analysis, and the owner's "A" of 2026-10-04 22:32; the `refusal-fallback` lane, a subagent of the DM thread, spawned 22:34, in a worktree; 0ea9e50e; joined 23:53 at e6f90af3, a signed merge onto 7ad8a8bd made 23:38, by the lane itself; on main's first-parent line it is the second parent of 80ef1dea, lane files' merge of `main`, which main fast-forwarded to; reviewed 2026-10-05 00:21 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** b5's loss analysis (theseus-7gir.1, reviewed 19:53) found three tasks Theseus lost on a provider refusal:
Sonnet 5.5 declined a request on cyber grounds (`stop_reason: refusal`, `stop_details.category: cyber`), the turn
ended (exit 7 headless), and Claude Code, which falls back to Sonnet 5 on a refusal, solved all three. Theseus's
catalog marked the provider's own server-side fallback (`fallbacks: "default"`) only for Fable 5.1 and Opus 5, so
Sonnet 5.5 needed a client-side one. It was the owner's product call (the original goal: every turn on the chosen model),
asked as 3 of 5 in the walk-through of 21:56 (the morning notes' sections 30 and 31). At 22:32 he chose "A": a refused
request is retried once on Sonnet 5, as Claude Code does, the reply says so and the log records it; no prompt change.

**What landed** (one signed commit; 40 files, +1,217 −61, most of it tests).
- **The catalog** (`catalog.rs`). A new field, `refusal_fallback_model`, apart from the server-side
  `refusal_fallbacks` flag. Only `claude-sonnet-5-5` sets it, to `claude-sonnet-5`, and its server-side flag stays
  false; the built-in table's version moves to `2026-10-04.1`. A `[catalog."<id>"]` table can name a fallback for
  another model (none does by default). The config refuses a fallback that is not another catalog model served by the
  same provider. When an entry has both, the server-side one wins: if the provider's own fallback rides the request
  (`compiler::server_fallbacks`: the profile asks for it, Anthropic's API, a model that takes it), the client-side one
  is not used.
- **The turn** (`turn/fallback_step.rs`, about 60 lines, and a hook in the loop). When an answer stops with `refusal`
  on a model that names a fallback, the same request is made once more on that model, by the same provider, in the
  next loop of the same turn, and the rest of the turn runs there. Its call is a provider call like any other: its own
  reservation at its own model's price, settled at its real cost, counted against the turn's spend. A refusal on the
  fallback ends the turn as a refusal did (exit 7 headless): one fallback per turn, never a chain, even where the
  fallback names one of its own, and a fallback's request never carries the provider's own fallback. The refused
  answer's "not run" results are written as before, and its text leaves the reply. The session keeps its own target,
  so its next turn starts on Sonnet 5.5 again (Claude Code's fallback is session-wide; this one is per turn, the same
  for a one-turn headless trial).
- **The request**, following the provider's guidance for a hand-rolled fallback (re-send the conversation as is, do not
  strip thinking blocks, keep using the fallback model afterwards): the compilation stays the profile model's, so the
  change triggers no `model_changed` recompile and writes nothing to the session's compilation; the fallback's
  requests name its model and take that model's own request parameters (`RequestSpec::fallback`); they carry both
  models' thinking unchanged (the API drops what Sonnet 5 cannot read); and they leave out the refused answer and its
  "not run" results. So the fallback's first request is the refused one with only `model` changed. Later turns leave
  the refused answer out by the rule that already left out an answer cut at the window (`replaced_answers`).
- **The switch.** `[model.retries] refusal = true` by default, a sparse key (absent means on), in the template beside
  the transient retries; `false` lets a refusal end its turn.
- **What the owner sees.** One ledger row, `provider.fallback` `{from, to, category, loop, refused}`; a `fallback`
  trace mark and a narrative line; `provider.refusal` as before, its narrative no longer saying "the turn ends". The
  turn's model is the fallback's (the served model of its last answer). The result gains an optional `fallback`
  (`from`, `to`, `category`, `answered`), which `turn.ended` carries only when set, so every other turn's wire bytes are
  unchanged. One plain line, worded once (`TurnFallback::line` in theseus-protocol): `Sonnet 5.5 declined (cyber);
  Sonnet 5 answered.`, or `… declined (cyber), and so did Sonnet 5.`, or `…; the request went to Sonnet 5.`; `theseus
  ask` prints it on stderr above the status line, `theseus watch` and the TUI's pane through the CLI's renderer,
  Discord's reply as `-#` text above its footer, and the cockpit's turn view under the turn (`cockpit/src/lib/
  fallback.ts` mirrors the words), its ledger view summarizing the row.
- **Line ceilings**: `compiler.rs` to 2,560 (+13) and theseus-protocol's `lib.rs` to 2,709 (+4), each with its reason
  in `scripts/long-files.txt`; the tests in a new `tests_fallback.rs`. The bench profile runs Sonnet 5.5 and gets this
  unchanged.

**How it is proven** (every test on a stand-in provider, no key: the core's fake learned `Scripted::Refused`, and the
daemon tests' stand-in Messages API now refuses with category `cyber`, as b5's did).
- **The core's eight** (`tests_fallback.rs`): a refusal on Sonnet 5.5 answered by Sonnet 5, the second request equal
  to the first in everything but `model`, one `provider.fallback` row with its exact fields, `turn.ended`'s `fallback`,
  the outbox post's line; the fallback's call spent and reserved at its own price (Sonnet 5 priced apart, $30 output,
  so the books show which model priced which call), and the session's next turn back on Sonnet 5.5; a refusal on both
  ending the turn after two requests and no third, even with Sonnet 5 given a fallback of its own; Opus 5.5 refusing as
  before, and Opus 5 with the server-side flag carrying `fallbacks: "default"` and not retried client-side; the switch
  off; b5's vulnerable-secret shape (a refusal on loop 2 after a tool call: the fallback's first request carries Sonnet
  5.5's thinking, the next extends it with Sonnet 5's, the refused answer never appears); a fallback's request taking
  its own model's parameters (Haiku 4.5 as Opus 5.5's: no adaptive thinking, 64,000 tokens at most) and no
  server-side fallback of its own; and the config's same-provider check.
- **Elsewhere**: the catalog's tests; the protocol, the CLI's renderer, the TUI's pane, the Discord footer and the
  cockpit's `node --test` each hold the line's exact words; and, through the real CLI and `theseusd --stdio`,
  `bench_profile::a_refused_request_is_answered_by_its_fallback_inside_the_headless_turn`, the issue's reproduction (a
  headless trial on the bench profile exits 0 on Sonnet 5's answer; with the switch off it exits 7 after one request,
  as b5's three did), `headless::a_refusal_its_fallback_answers_exits_0_and_says_so`, and `headless::a_refused_turn_
  exits_7`, now a refusal on both.
- **Planted reverts, 16 of 16 caught** (each rebuilt and run; one that does not build counts as not proved, never as
  caught; `git status` clean after): the turn never falling back; a chain; a model without a fallback getting Sonnet 5;
  the client-side fallback running where the provider's own rides; the switch ignored; no row; no `fallback` in the
  result; the line's words changed ("answered" to "replied", caught by six surfaces' tests); the refused answer riding
  the fallback's request; the fallback's own thinking stripped; a later turn rendering the refused answer; the
  fallback reserving as the refusing model; the refusing model's parameters; the provider's own fallback riding a
  fallback's request; the refused text staying in the reply; and the fallback never marked answered.
- **The lane gate** (`THESEUS_GATE_NO_BENCH=1`): 2,525 of 2,525, plain turn 5 frames p50 77.0 ms, tool call 9 frames
  160.8 ms (two earlier runs failed on a clippy finding and the cockpit's regenerated types not yet staged, both fixed).
- **No live check.** While the lane prepared one (resolving a model key for a scratch daemon, with a request shaped
  like b5's crack-7z-hash task), a safety classifier stopped its response, and it did not pursue it in any other form.
  So there is no live evidence that the provider refuses such a request today, nor of the switch on a real call; the
  daemon tests stand in, running the real CLI and daemon on b5's bench profile against a stand-in API that refuses as
  the API does. The DM thread's review accepted it so ("a safety classifier stopped the lane's cyber-shaped probe; the
  stand-in proves the path").

**FAST.** A turn that is not refused does nothing new: no frame, no record, no I/O on its path; the only new code on it
is two in-memory checks per loop (the stop reason, and whether the turn has fallen back).

**The join.** A merge-tree dry run showed no conflict and every ceiling holding; `git merge --no-ff -S` of 0ea9e50e
onto 7ad8a8bd (lsp-checks' join), **e6f90af3**, five files auto-merging (`crates/theseus-core/AGENTS.md`, the template,
`config.rs`, `lib.rs`, the CLI's `render.rs`); the warm clean. **The gate** (23:45:42 to 23:52:39, after 97 s waiting
for the shared lock, held by the lane's own plant run and another lane's tests): suite **2,541 of 2,541** (19 skipped)
in 245 s; lifecycle ok with no busy allowance (cold start p50 24.0 and p95 29.7 ms, from the config copy 22.8/30.9,
clean shutdown 31.2/45.8, SIGKILL and restart 27.6/35.4, swap 62.0/85.7, restore 147.5 measured only); jobs' L1 start
6.29/7.40 ms; turn frames **5 and 9**, plain p50 76.5 and tool call 158.9 ms (the night's three joins before it: 74.6,
75.0 and 76.1; 166.6, 164.2 and 159.0). Pushed at 23:53; theseus-7gir.18 closed; the worktree, branch and target
deleted. Lane files then merged this `main` into its branch (80ef1dea) and main fast-forwarded to it, so e6f90af3 is
that merge's second parent (Item 155). **The review** (the DM thread, 00:21, accepted): the catalog's
`refusal_fallback_model` (client-side; the provider's own fallback wins where on), one retry per refused request for
the rest of that turn, `[model.retries] refusal = true` by default, the `provider.fallback` row and a one-line notice
on every surface.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change; one new key, `[model.retries] refusal`, on when absent.
Nothing to configure for Sonnet 5.5. The owner was told after the review,
and offered Opus 5.5 to Opus 5 by config at install #4 unless he objected; he did not, and install #4 added
`[catalog."claude-opus-5-5"] refusal_fallback_model = "claude-opus-5"` to his config. The provider-side fallbacks for
the 5.5 models get a live probe in the held-out rerun (theseus-7gir.22), which also adds b5's three refusal tasks. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** Per turn, not per session as Claude Code's is. Client-side, as the owner chose; the lane changed neither
model's server-side flag. No metric of its own (the provider calls' spans and metrics already carry each call's
model).

**Known gaps.** Text already streamed stays streamed: a refusal mid-stream may have streamed some text to `ask`'s
stdout in stream mode or to Discord's live message, which the reply's final form, the result and every later request
leave out (b5's refused answer streamed none). The first fallback call writes a cold cache (caches are per model; the
fallback-credit beta is not used). Opus 5.5 would want the same (routing sends it the hard questions, and its
classifiers are broader: `bio` joins `cyber` and `reasoning_extraction`); turning it on needs no code. The
server-side alternative (the provider's `fallbacks: "default"`, listed for Opus 5.5 and Sonnet 5.5 on the Claude API,
retrying `cyber` and `frontier_llm` declines but not `bio`, `reasoning_extraction` or `general_harms`) wants a live
probe before anyone flips its flag.

### Item 155. Files: Theseus reads the files people give it, PDFs natively for Claude and as text for GLM, then Office files, notebooks, EPUB, RTF, archives, recordings, video and the text in pictures; store format 17 (theseus-c9l6; the owner's 22:53 "I attached a PDF for Theseus and he couldn't read it. We want him to understand a wide range of files as you do."; the `files` lane, a subagent of the DM thread, spawned 2026-10-04 22:56, in a worktree, in two joins; join 1: e71e96cd (00:41) on 7ad8a8bd and 80ef1dea (00:53), the lane's signed merge of `main` for refusal-fallback, which main fast-forwarded to, pushed 01:11; join 2: 44eacbaf and ff87839c, with 68353d80 merging `main` again, joined at 198e2229, a signed merge onto faaa9df6 made 02:11, pushed 02:24, by the lane itself; reviewed 02:51 by the DM thread; store format 16 to 17 at join 1; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** At 22:53 a PDF the owner attached in his Theseus DM was not read: the Discord binding downloaded text and images
only and listed every other attachment by name, and `http.fetch` skipped PDFs. The DM thread spawned the lane at
22:56, PDFs first and then the wide range, to install once join 1 landed and to tell him when PDFs worked.

**What landed.**
- **Join 1, PDFs** (e71e96cd; 52 files, +3,466 −321; a new crate, `crates/theseus-files`, with `pdf.rs` and
  `convert.rs`).
  - **Any file, from every surface.** Every surface accepts any file up to `[tools] max_attachment_bytes` (new,
    32 MiB): Discord downloads any attachment as its bytes (text files still come as text); `theseus ask --attach`
    sends any file the same way; the cockpit has no upload, so it is unchanged. The core keeps the file whole in the
    store's blobs, `AttachmentContent::File`, the reason for **store format 17**.
  - **Read once, by digest, in a capped child.** A child of the daemon's own image (`theseusd files-convert`) with 30
    s of wall clock, 1 GiB of address space, CPU time past the wall clock, no file writes, 64 descriptors, an empty
    environment and death with the daemon, after lopdf's own 128 MiB bound on what one stream inflates to; what was
    read is a blob the node names. Every `file.read` row says `capped: true`.
  - **Per model.** On arrival a PDF gives its pages, its text by page, and parts for the page limits. Claude gets a
    `document` block of the file, or the largest part the request has room for, with a line naming the pages left out
    (600 pages a request, 100 on a 200K-window model such as Haiku 4.5; 18 MiB of PDF bytes a request, which base64
    makes 24 MB of the provider's 32 MB request limit). GLM, which takes no PDF input and no images, gets its text page
    by page, and a scanned page says it has no text. PDFs are budgeted in the order they render, so an earlier PDF never
    renders differently because of a later one and the cached prefix holds. A PDF the provider refuses is hidden from
    then on and shows as its text (the images rule, theseus-0s4). The token estimate counts a page at 1,600 tokens
    plus its text (live: the 3-page test PDF cost about 4,900 input tokens on Sonnet 5.5).
  - **By path and by fetch.** `fs.read` reads a PDF's pages by path, as Claude Code's Read does (`pages`, 20 at most),
    and `http.fetch` and `file.read` return a PDF's pages, cut out as a PDF of their own, as a document block in the
    tool result (their text for GLM). **Trust:** a file from a fetch is external text, and takes the session's hold.
  - **The dependency.** `lopdf` 0.45 (MIT, default features off), the one new crate of weight, with 16 more new to the
    workspace (all MIT, Apache or BSD-3, within `deny.toml`): about 43 s of compile CPU alone, 65 s with the others,
    measured cold in release at `-j 4` and nice 19 under load 40. `zip` and `flate2` were already in the tree.
  - **The merge of `main`** (80ef1dea): refusal-fallback had joined meanwhile (Item 154); one
    conflict, a comment line, putting its `Media.also` beside the PDF fields.
- **Join 2, the wide range** (44eacbaf, +4,374 −290 in 27 files, and ff87839c, the live check's fixes; `theseus-files`
  gains `doc.rs`, `archive.rs`, `kind.rs`, `media.rs` and `xml.rs`, a small XML scanner of its own in place of
  quick-xml, and tar read by hand; theseus-core gains `file_read.rs`; theseus-tools `docs.rs`).
  - **On arrival, every model reads the text:** Word .docx, OpenDocument .odt and RTF (headings marked `#`, lists,
    tables as rows); Excel .xlsx and .ods (each sheet a table under its name, 5,000 rows at most); PowerPoint .pptx and
    .odp (each slide with its title and speaker notes); EPUB (chapter by chapter, in the spine's order); Jupyter
    notebooks (each cell with its outputs; output images as images for a vision model, named as not shown for GLM);
    zip, tar and tar.gz (their list). Long text is cut at 256 KiB a file, with words that say where and how to read on.
  - **A new tool, `file_read`**: a PDF's other pages; a document's sections; an archive member, saved under the
    working directory's `.theseus-files/<session>/` and read; a recording's transcript (mp3, wav, m4a, ogg, webm,
    flac) by Deepgram nova-3, **only when the model asks**, kept by digest so it is heard once; a video's audio
    transcript and a strip of four frames as one image (ffmpeg); an image's text by OCR (tesseract), for GLM; and
    `save`, which puts any other file where `proc_run` can use it. Anything else is kept, its name, type and size
    shown.
  - **Spend.** Audio costs $0.0043 a minute on nova-3, **$0.26 an hour**, capped at `[tools] transcribe_max_minutes`
    (new, 60; a longer recording is heard to the cap and says so); the estimate is checked against the session's
    remaining spend first, then booked to its execution as `speech.transcribed`, as a voice call's speech is.
  - **Other bounds.** A zip member unpacks to 64 MiB at most, a document's text to 8 MiB; ffmpeg, ffprobe and tesseract
    run under 90 s, 2 GiB, two threads and an empty environment, each found on `PATH` or Homebrew's prefix, and when
    one is absent Theseus says so and nothing fails.
  - **Trust.** `file.read` in a shared place marks its result external; a shared place's attachment is under the place
    rule, as its typed text is.

**FAST.** A message without such a file runs nothing new (one `any()` over its attachments). Conversions run once per
digest, off the runtime's workers. The start path gains only a `OnceLock` set.

**How it is proven.**
- **Tests, all with fakes**: theseus-files' PDF reader, each document reader, the archive's path rule and member cap,
  the kind sniffer, the XML scanner and the converter's words; the core's attachment keeping, rendering per model, the
  budget order, parts, a refused PDF, an unreadable one, Word, a notebook's images, a notebook sent as text, a
  recording's line and an archive's list; `fs.read`'s pages, a workbook and a zip; `http.fetch`'s PDF; Discord end to
  end through the fake gateway and CDN; and theseusd's `tests/files.rs`, which runs the real converter child under its
  memory cap and its time limit, a turn per model through a stand-in Messages API, and `file.read` with a stand-in
  Deepgram (heard once, booked).
- **Planted reverts, eight sets, each caught**: the render-order budget, fetch's PDF kind and a tool's PDF as a
  document (3 of 3 tests failed); the document block for Claude (3 of 3: the core, the daemon's turn, Discord's); the
  capped child, conversions run in-thread (4 of 4); Discord's PDF download and `fs.read`'s pages (3 of 3); Word's
  headings (3 of 3); the archive's path safety and the transcript cache (2 of 2); a notebook sent as text; the
  transcript's spend booking.
- **Live, on a scratch daemon of the lane's build** (the fake Discord with a CDN, the bot token an `env:` value that
  is not a token, `[web]`, `[index]` and `[voice]` off, a fresh state dir; invented files, so only the file holds the
  answer). **Join 1**, a PDF of a text page, a table page and a scanned page with no text layer: through the fake
  Discord on Sonnet 5.5, all three answers, the scan's included ("cabinet 31, second hook"), $0.0568 with the cache
  write; `ask --attach` on Sonnet 5.5, all three ($0.0170); on GLM-5.3 Flash, answers 1 and 2 from the text, saying page
  3 is a scan with no text ($0.0021); a follow-up in the same session, right, with **22,388 tokens read from cache**
  ($0.0050); `fs.read` of pages 2 to 3, right, the scan included ($0.0171); `http.fetch` of the PDF, right, with
  `session.external_read` taking the hold ($0.0213). Every `file.read` row `capped: true`, in 24 to 71 ms. **Join 2**:
  a .docx, .xlsx and .pptx together (channel 71; K-7 is 23.4 m; the speaker notes' slipway line); a notebook on GLM
  from its cells, and on Sonnet 5.5 "D-12, 31.0 m", found only in an output image; a zip member saved under
  `.theseus-files/<session>/logs.zip.d/`; a .wav transcribed by the real Deepgram, its `speech.transcribed` $0.000387
  booked; an .mp4's spoken memo and its frames' caption; a .png's text by tesseract on GLM; a Discord message with a
  .docx ("Quiet hours begin at 22:30."). About $0.33 for both joins. Not live-checked: EPUB, RTF, OpenDocument, tar
  and tar.gz, covered by unit tests on sample files only (the lane's Beads note had said every kind, and corrected
  itself at 02:26).
- **Found and fixed by the live check** (ff87839c): a notebook is UTF-8 JSON, so the CLI and Discord sent it as text
  and the model saw raw JSON; notebooks and RTF now travel as bytes, and the core reads a text one as the document it
  is. Models copied the line's kind word into a name (`file_read` of "Audio memo.wav"); `file.read` now drops it.

**The joins.**
- **Join 1** (lock `lane-files-join-1`): the lane's branch first merged `main` for refusal-fallback (80ef1dea, one
  comment-line conflict), and main fast-forwarded to it. **The gate** (01:01 to 01:10): **2,562** tests passed;
  lifecycle within budget (cold start p95 35.4 ms against 50 + 7, clean shutdown p95 39.7, SIGKILL restart 24.9, swap
  62.2); turn frames **5 and 9**, plain p50 74.4 and tool call 154.0 ms. Pushed 01:11. Store format **17**, so batch
  6's three format bumps under review (consolidation, task-board, situations) renumbered to 18 to 20 in join order (the
  morning notes' section 33). The context-honesty lane, whose join lock queued behind it, found that the joiners'
  `locks-open.sh` listed only `*-join` locks and so missed `lane-files-join-1`; the DM thread made it read `*-join-*`
  locks too at 01:41 (Item 156).
- **Join 2** (lock `lane-files-join-2`, which waited on cloud retention's join lock): the lane merged `main` again
  first (68353d80, context-honesty's join, clean) and passed the whole suite there, 2,584 tests. `git merge` of
  68353d80 onto faaa9df6 (Item 157), **198e2229**, signed; the warm built clean (the test build 5 min 15 s,
  clippy clean). **The gate** (02:18 to 02:23): **2,595** tests passed; cold start p95 26.6 ms, clean shutdown p95
  40.8, SIGKILL restart 32.5, swap 56.4; turn frames **5 and 9**, plain p50 73.0 and tool call 157.9 ms. Pushed 02:24;
  theseus-c9l6 closed. The lane's worktree, branch and 45 GB target dir were deleted, and its scratch dir, its results
  kept with the lane's report.
- **The review** (the DM thread, 02:51, accepted, with both merges spot-checked on origin/main and the live results
  read): "Conversions run once per digest in a capped child (30 s, 1 GiB, no writes)"; 8 plants, all caught; new keys
  defaulted; audio transcribed only on `file_read`. Follow-ups filed: theseus-lv2u, theseus-qgsv, theseus-w68l.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). Install #4 moved the owner's store from format 16 to 20 at its first write (17 is this join's;
two syncs before serving), after the install's backup, since an older binary refuses the store after that (`setup.sh`
makes no backup). Both new keys are defaulted, and his config needs no edit; transcripts use `[voice]`'s Deepgram key and
settings, which his config already has. ffmpeg, ffprobe and tesseract are found in Homebrew's prefix, though the user
service's `PATH` is short. A large Discord attachment can take up to the 60 s download bound before its turn starts.
What he was to try first: a PDF in his DM, a follow-up in the same thread (the document stays in the prompt cache), a
PDF over 100 pages, then a Word file, a spreadsheet, a deck, a notebook, a zip and a Discord voice message. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** Join 1 landed as a fast-forward to the lane's own merge of `main`, not as a merge commit onto main.
XML is read by the lane's own small scanner, and tar by hand, rather than by new crates.

**Known gaps.**
- **theseus-lv2u** (P2): recall cannot see a kept file's text. The index tender reads node text from the WAL, and a
  file's text lives in a blob.
- **theseus-qgsv** (P3): the cockpit has no upload (a Composer change).
- **theseus-w68l** (P3): a PDF's scanned pages have no OCR on GLM. No page renderer ships (no poppler; PyMuPDF is
  Python); Claude reads them natively. A Rust page rasterizer would be the follow-up.

### Item 156. Context honesty: the system header says how a request is assembled, and recall's preamble names the harness (theseus-fpm2, its context half; the owner's ask of 2026-10-04 23:55; the `context-honesty` lane, a subagent of the DM thread, Opus 5.5, spawned 23:57, in a worktree; 83f326df on e6f90af3; joined 2026-10-05 01:23 at 0ade7d68, a signed merge onto 80ef1dea (lane files' join 1) made 01:11:33, by the lane itself; reviewed 01:41 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** At 23:45 on 2026-10-04 the owner's greeting in his Theseus DM was answered with a narration of the notes recall
had admitted to it, as if they had come in with his message: recall had packed notes about another of the operator's
projects into his greeting's turn, and the model took them for part of what he sent. At 23:55 he asked that
Theseus's system prompt explain that its context is dynamically assembled, so it stops narrating recall as "came in
with your message". The DM thread spawned the lane at 23:57 with three parts: a persona paragraph (60 to 90 words,
cached), recall's preamble, and the Recall node's placement; the small-talk half (recall should not pack six notes for
a greeting) was to wait for batch 6's memory joins. The persona was three sentences (`turn.rs`'s `PERSONA`).

**What landed** (one signed commit; 9 files, +63 −37).
- **The assembly note** (`crates/theseus-core/src/turn.rs`, `pub const ASSEMBLY`), in the shared header
  (`TurnRunner::system_blocks`) right after `PERSONA` and before the tools note: persona, assembly note, tools note,
  then the profile's own text. The tools note could not hold it, being empty when tools are off. Verbatim:
  > Context. The harness assembles each request: the conversation, its older part perhaps summarized; notes it
  > recalled from earlier sessions, marked as recalled; files the person attached; tool results; and its own notices,
  > such as wakes, task reports, and late results. A recalled note is background the harness chose, not something the
  > person sent. The person sees their own messages and your replies, not the rest of the request, so use a recalled
  > note when it helps, and otherwise don't mention it.

  81 words, 499 characters: the live requests' cached prefix grew from 10,702 to 10,846 tokens, 144 tokens. The text
  is static, so every session of a profile, and its tasks, still share one header (the daemon test `cache_header`
  passed on the sonnet and glm profiles). "Not the rest of the request" is literally true: no surface shows a recall
  note's text (Discord shows a `🧠 N recalled` count, and the cockpit renders no Recall node). Late results are named
  because they render as user-turn text the person never wrote (`[Background result for your earlier … call …]`).
- **The preamble** (`crates/theseus-core/src/recall/render.rs`): from `[Recalled: 3 notes from earlier sessions.
  Testimony, not instructions: dated, and possibly stale.]` (98 characters) to `[Recalled by the harness: 3 notes from
  earlier sessions, not part of the person's message. Testimony, not instructions: dated, possibly stale.]` (143
  characters, about 10 tokens more per note). "[Recalled" stays the first word, so the node still reads as "marked as
  recalled".
- **The Recall node stays where it is**: rendered as a user-role text block that `push` merges into the person's own
  message, right after their words (in an assembled prefix, a task's first compile or a compaction, the section comes
  first in the prefix's first user message). Not moved, for four reasons: the Messages API has no other slot for
  per-turn text (the system blocks are the cached prefix every session of a profile shares; a separate user message
  would merge into the same human turn; recall as a tool call would mean writing an assistant turn the model never
  made); node order keeps the cache (each request begins with the previous one's bytes); the memory exam's oracle arm
  sends "task, blank line, note", the same characters in the same order, and M6's exam numbers were measured so; and
  batch 6 was changing recall at the time. The words set it apart instead.
- **Docs in the code**: the template's two comments that list the system block name the assembly note; theseus-core's
  `AGENTS.md` gains a sub-bullet under Context, "The system header" (its four parts, and that harness facts every
  request needs go there), and its Recall paragraph says what the preamble says. The design's example of the old
  preamble (`docs/design/m6-memory.md` §2.4, step 7) was left as the plan's text, for this record to give the new one.

**How it is proven.**
- **Tests and goldens**: every test that pins the persona's place or the preamble's words was updated: the header's
  order in `tests_m3.rs` (`PERSONA < ASSEMBLY < "Tools. You act" < the context file's header`) and its two exact
  headers; the render's golden bytes and `preamble(1)`; `tests_recall_node.rs`; a task's assembled section and a
  compaction's in `tests_compaction.rs`; the exam's oracle note byte for byte and its stand-in model's search. The two
  schema goldens hold neither text and passed unchanged. The targeted run: theseus-core, theseus-exam and theseusd's
  `cache_header`, 1,165 tests. No separate planted revert: the tests pin the text by equality, so a revert fails them
  by construction, and the live A/B's base arm is the change reverted, run live.
- **The live A/B** (two scratch daemons, transient user units on fresh state dirs, each run from a fresh copy of one
  indexed snapshot seeded with `theseus-exam write-store`: three September sessions about an invented kiln scheduler,
  each opening with a greeting, and today's session about an invented sailing-club booking app, two rules settled and
  nothing built; Sonnet 5.5 live, `[memory] mode = "live"`, arm `baseline`, the judge off so there was no detour and
  no rerank; the base arm main's e6f90af3 binaries, `strings` showing the old preamble and no assembly note; about
  $0.15 in all). A greeting continuing the booking app's session, with recall admitting six notes about the kiln
  scheduler, as the incident's notes were admitted to the owner's greeting: **main's build mentioned the notes and set them
  aside in 3 of 3 runs** ("The recalled notes are all about [the kiln scheduler] … I'm not using them here, since they
  don't affect [the booking app]"), the incident's pattern; **this build in 0 of 3**, each reply going straight to the
  booking app's two rules. A word check (the kiln project's name, recalled, note(s), kiln, 7811, "came along", "with
  your message") found the notes in all three base replies and none of the lane's. A question one note answers (which
  port the kiln scheduler listens on these days) was still answered from it on both builds, 7811, the lane's adding
  "That comes from a recalled note, not from anything I checked." The lane's greetings were also shorter, 223 to 298
  output tokens against 406 to 459 ($0.0067 to $0.0075 a greeting against $0.0085 to $0.0090).
- **The caveat, told to the owner at about 01:55** (the morning notes' section 34). In a brand-new session, where the
  recalled notes are the only context, both builds still brought them up (the lane 2 of 2 runs: "My notes from earlier
  sessions say you've been working on [the kiln scheduler] … Are we picking [it] back up tonight?"; the base 1 of 1);
  the new build calls them its own notes from earlier sessions, which is accurate, and neither says they came with the
  message.
  Only the small-talk half (recall not packing notes for a greeting) stops that.
- **The lane gate** (`THESEUS_GATE_NO_BENCH=1`): 2,541 of 2,541, plain turn 5 frames p50 79.4 ms, tool call 9 frames
  166.3 ms, 905 s with 267 s waiting for the lock (its first run failed in fmt on one line, fixed).

**What the lane found** (its report's section 6, a design, not built). Recall packs before routing is known: route.v1's
call starts at inbound and the turn reads the verdict only beside the first compile, while recall in front of the
model runs earlier, under `recall_deadline_ms` plus `rerank_wait_ms`, so the verdict is usually still in flight. **A
trivial detour still wrote the recall it never sent**: the detour's request carried no notes, but the provider call's
plan frame took `t.recall.rides` unconditionally, so the Recall node was written, the reply's `🧠 N recalled` footer
counted notes the model never saw, and the next non-trivial turn rendered them in its tail right after the greeting,
one turn late, where they read as part of the greeting. That was filed as **theseus-n7nc** (P2, fix B: a detour drops
its pending recall), built after lane speed lowered trivial's bar to 0.4 so that every greeting detours
(Item 161). And the seed's greeting admitted all six notes at fused scores near the relevant ones' (0.106
to 0.114): the threshold cannot tell small talk from a question. The design's other parts: recall peeks the route
verdict when it is already in hand and never waits for it, applying a small-talk bar (`[memory] trivial_min_score`)
whose drops are recorded with reason `trivial`; shadow first (`trivial_would_drop` on `recall.ran`), acting only
through the ladder after a week of the owner's labels; no bar without the judge.

**The join.** The commit 83f326df is a docs-only amend of 33096c69, the tree the lane gate passed: lane files' join 1
(lock `lane-files-join-1`, taken 00:56; Item 155) changed the same line of theseus-core's `AGENTS.md`, which
`git merge-tree` showed as the one conflict, so the lane moved its sentence into its own sub-bullet and both merges
came out clean. Lock `lane-context-honesty-join` taken at 01:07:30, queued behind `lane-files-join-1`; the lane's
`joincheck.sh` read `*-join-*` locks too, since `locks-open.sh` listed only `*-join` and so missed lane files' lock
(the DM thread fixed `locks-open.sh` at 01:41). Merged as soon as that lock had its done line and main equalled
origin/main, at 01:11:33: `git merge --no-ff -S`, **0ade7d68** (parents 80ef1dea and 83f326df). The warm passed (the
test build 2 min 17 s, clippy clean). **The gate** (01:14:57 to 01:22:21): **2,562 of 2,562**; lifecycle ok, cold
start p95 33.9 ms, clean shutdown p95 42.6, SIGKILL and restart p95 27.3, swap p95 48.5; jobs' L1 start p95 8.10 ms;
turn frames **5 and 9**, plain p50 74.6 and p95 83.4 ms, tool call 166.4 and 174.4 ms. The prompt grew by the note's
499 bytes plus a 2-byte separator in the header, and 45 bytes per recall note. Pushed by the lane's `land-mine.sh`
(land.sh's guards: the gate log's exit 0, main the lane's own merge on origin/main), the worktree, local branch and
target removed (the branch was never pushed); the done line 01:23:06. theseus-fpm2 left open for the small-talk half.
**The review** (the DM thread, 01:41, accepted): `ASSEMBLY` in `turn.rs` and the new preamble in `recall/render.rs`
checked on origin/main, the gate log's exit 0, and the live A/B's files ("base named [the kiln project] in 3/3
greetings, lane in 0/3; both answered 7811 from the note"); the Recall node stays after the person's words; theseus-n7nc filed, to build
after lane speed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change and no new key. The first turn of each existing session after the install is a `system_changed`
recompile, which writes its cache once and drops its prefix's thinking: the one-time cost the design accepted for
situations' precedence line, which the morning notes' section 33 asked to ship together with this. The `[memory]` and
`[judge]` settings are unchanged. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The brief's example preamble ran to 150 characters; the built one is 143 ("and possibly" became
"possibly", to stay one bracketed line). The node was not moved (the four reasons above).

**Known gaps.**
- theseus-fpm2's small-talk half (the design above, A, C and D), after batch 6's memory joins; fix B is theseus-n7nc.
- **For batch 6's situations join**: that branch adds its precedence line at the same place
  (`parts = vec![PERSONA, situation::PRECEDENCE]`) and edits the same two header tests and the exam's note test, so
  whichever lands second resolves `parts = [PERSONA, ASSEMBLY, PRECEDENCE]` (what a request holds, then how to weigh
  it), the two tests format all three, and the exam's expected note takes this preamble and situations' new header.
- The persona is in the benchmark's arm A, so the held-out rerun (theseus-7gir.22) runs with a header about 144 tokens
  longer; the bench profile has no recall, so the note's recall sentence describes context the bench never gets.

### Item 157. Retention: FSRS-6 folded from memory's own rows, a rank under the `+retention` arm, and health's memory block (theseus-6fn.11; step 32a's wire-in, roadmap row 58; the sixth cloud batch's retention session, fired 2026-10-04 20:00 from 3085f71a, Opus 5.5; 55316c8d, 764154de, d1602aa6, 586fb996 and 02972b6d; reviewed 2026-10-04 23:52 to 2026-10-05 00:52 by local reviewer R5, stack M; joined 02:10 at faaa9df6, a signed merge onto 0ade7d68, by the stack-M joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** `theseus-memory` had FSRS-6 since M6's first steps, checked against its reference in Item 26, and an access
table that grades each use (`access.rs`, `Access::grade`), but nothing read the ledger's memory rows into events, no
projection held a node's retention, and every recall ranked through one `Baseline`; the trait's `rank` saw only the
turn's time. Step 32a's wire-in puts FSRS-6 under real nodes as one arm of §5.5a's experiment: retention folded from
the rows the memory pass (31a) already writes, read only by a turn or a search that names `+retention`, so every
other daemon pays nothing for it.

**What landed** (49 files, +1,896 −80 without the paperwork; the merge 50 files, +1,902 −82; no new package).
- **The events** (55316c8d; theseus-core `recall/retention.rs`, new, 532 lines). `event_of(&LedgerRow)` reads one
  row into one `AccessEvent` at the row's own time (`at_unix_ms`), purely: `memory.labeled` (scope `memory:<session>`,
  its `durability`) is first sight; `memory.used` (scope `recall:<session>`, `used`, and `outcome` only when used) is
  a use; `memory.label` (scope `memory`) is a label. A row that does not read (an unknown durability, outcome or label;
  no node) is no event, and a used row without an outcome reads as `Unknown`. The labels are four: the design's "the
  operator asked Theseus to remember it" has no label, and `Label::Remember` became `ShouldHave`, graded **Easy**, the
  same vouching as `useful`, so that a miss ranks higher next time. Only `wrong` and `stale` exclude.
- **The projection** (`Projection`): per node, its events by WAL position, folded with `Fsrs6::default()`
  (`fsrs6-default`), one projection for every arm, so shadow's uses count too (M6's §5 question 9). A row that the build
  and a write both bring counts once (the same position); one that arrives out of order (the pass's frame and a
  `memory.label` racing) refolds that node, so the kept projection always equals a rebuild, event for event. Its
  phases: `unbuilt`, `building`, `ready`, `failed`.
- **Built after serving, and only when read** (`retention::warm`). `Core::warm_retention` sits beside `warm_labels` in
  theseusd's after-serving list and does nothing unless `[memory]` is on with `arm = "+retention"`; otherwise the first
  `memory.search --arm +retention` or the first turn of the arm starts it. The build is one task on the blocking pool:
  one walk of the three kinds through `Store::ledger_page` (tags `k:memory.labeled`, `k:memory.used`,
  `k:memory.label`, interleaved in position order, 500 rows a page). While the index's shape (Item 91) is still being
  built after a start, `ledger_page` answers `None`, and the build waits 2 s on tokio's timer and asks again. After it,
  `Memory::retention_written(records, positions)` keeps it current, called by the memory pass after each frame's append
  and by `memory.label` after its own.
- **The science** (`theseus-memory/src/retention.rs`, `RetentionRank`): `baseline` in every verb but `schedule`
  (FSRS-6's step) and `rank`, which scores `fused × ((1 − w) + w × R(now))` with `w` = 0.5, `R` the node's
  retrievability at the turn's time. A node with no retention keeps its fused score, as though `R` were 1: every node the
  pass labeled has one, so a node without was written since the pass last ran, seen moments ago. Its digest,
  `retention@<16 hex>`, covers the form, the weight, FSRS-6's 21 parameters and the baseline's own line, so a change to
  any of them is a new version.
- **The arms' seam.** `RankCtx` gained `retention`, `Asker` a borrowed retention map, and the trait `reads_retention()`
  (default `false`); the core fills the map only for a science that says `true`. `MemoryArm::Retention` (serde
  `"+retention"`); `Memory::science_for(arm)`, a match, `baseline` for all but `+retention`; `Scene.science`, read by
  `manifest_ranked`, `refill` and the rerank's `Recalled` (which gained `retention`, and `Reranked` forwards
  `reads_retention`), so a live rerank's repack (Item 141) keeps the arm's order for whatever Jev ties
  or leaves unanswered. Shadow, a canary's control and a search without `arm` rank with `baseline`. The arms are config:
  `[memory] arm = "+retention"`, with nothing added to `turn.submit`. 32b and 31b add their arms as one more variant and
  one more match arm.
- **Surfaces.** `RecallManifest.retention` (`ready`, or why the rank went without: `building`, `unbuilt`, `failed`;
  only under a science that reads retention); `RecallItem.retention` and (586fb996) `RecallDrop.retention`, so a label's
  effect shows on the dropped item (`RecallRetention`: retrievability at the turn, stability, difficulty, last review);
  `MemorySearchParams.arm` and `theseus memory search --arm +retention`; each item's CLI line, e.g. `retention: R 0.987
  · stability 3.17 d · difficulty 5.26 · last review 2026-10-05 04:31:07Z`, in `memory search` and `memory recalled`,
  with a warn line when the rank went without the projection; health's new `memory` block (`MemoryHealth`: mode, arm,
  the projection's state, nodes, events, why), printed `memory: live · arm +retention · retention ready · 2 nodes (5
  events)`; and the gauge `theseus.memory.retention.nodes`. The cockpit's `protocol.gen` was regenerated
  (`MemoryHealth.ts`, `RecallRetention.ts`); the cockpit has no Memory view yet, so it shows none of it.
- **The exam** (d1602aa6): `Arm::Retention` on a daemon of its own, `+retention − baseline` in the report, and the feature
  row "retention (`+retention` against `baseline`)" decided by the plan's rule. It is not in `run`'s default `--arms`, so
  its spend stays the owner's choice.
- **No store format change, rightly:** the new manifest and item fields are JSON in ledger rows, which old readers
  ignore and new ones default. The store stayed at main's 17 (lane files' join 1, Item 155).

**How it is proven.**
- **The session's tests** (`tests_retention.rs`, seven whole-core tests, and `theseus-memory`'s `retention::tests`): the
  rebuilt projection equals the incremental one, across rows written before the build, frames after it, two
  `memory.label`s, two frames handed over in the reverse of their positions, and one handed twice (10 events, 3 nodes;
  one node equal to `Fsrs6::fold` of its events); five `used: false` rows leave a node's retention bit-identical;
  each durability's first sight and each label's grade; two notes the stand-in index ties at 0.02 order by retention
  under `+retention`, in a search, in a live turn's `recall.ran` row and in its request, and the one labeled `stale` is
  then dropped as `labeled_wrong` with its lowered stability on the drop; the repack in Jev's order keeps the arm's
  science, on tokio's paused clock; a recall before the build ranks without it and says `building`; and a shadow turn
  under the arm sends the same bytes as `mode = "off"`. Under the repo's load recipe (the tests at `nice -n 19` beside
  four busy loops), 108 tests a run, 5 of 5 runs green.
- **Planted reverts.** The session's three (a `Shown` graded as a review; the rank ignoring retention; a late row
  stepped, not refolded) each failed the tests it named. R5 planted five on the merged tree: a search under any arm
  building the projection, caught in 274 s; a turn's `scene()` under any arm building it, **not caught** (below); a
  node with no retention ranked as forgotten, caught in 5 s; the out-of-order row, caught in 217 s; `should_have`
  graded Good, caught in 265 s.
- **R5's tests on the merge** (on e6f90af3): 335 of 335 in 75 s, at a load of 45 to 52 (CPU PSI some 72 to 80 %); the
  exam's `tests/arms.rs` ran all five arms on four real daemons.
- **Live, at the review** (the stand-in model, no key, no spend; `[memory] mode = "live"`, `arm = "+retention"`; BM25
  and entities only). The projection built after serving with 0 nodes; three notes about the "Osprey build" and a
  "Thanks." gave C's `memory.used` rows (B used, `outcome: ok`; A shown, not used); the search named
  `retention@cb17a25611f4c498`, the session's digest, with each item's line (stability 2.31 d, difficulty 2.12);
  `memory label <B> stale` dropped B as `labeled_wrong` at stability 0.78 d and difficulty 7.39 (a same-day Again), with
  health at `8 nodes (12 events)`; after a stop and a start on the same store, health read the same, and the admitted
  items' stability, difficulty and last review equalled the ones before the restart: the projection rebuilt from the
  WAL equals the one kept as the rows were written.

**What the session found.** A same-day Good leaves stability where first sight put it, FSRS-6's rule, so a use the
day a node was written moves only its last review. Without a dropped item's retention nothing showed what a label did,
which is why 586fb996 put it on the drop. And the exam cannot show the arm yet: its stores hold no memory rows, so
`+retention`'s daemon ranks exactly as `baseline`'s. The report lists the item kinds that would discriminate once the
fixture writes `memory.labeled` and `memory.used` rows (a superseded fact used then `corrected`; a distractor shown many
times and never used; a preference against a transient at equal scores; a fact stated twice a year apart).

**The join** (stack M's first; reviewed on refusal-fallback's e6f90af3, merged on context-honesty's 0ade7d68). The
signed merge faaa9df6 (0ade7d68 and 3cd6092e) at 02:01:56, 50 files, +1,902 −82; `CLOUD_REPORT.md` and `CLOUD_TASK.md`
removed. One conflict, `scripts/long-files.txt`, replayed by rerere from R5's resolution: theseus-protocol's `lib.rs`
ceiling 2,709 → 2,713, since refusal-fallback (Item 154) had taken 2,709 for its own lines. Two
join fixes, both found by the joiner and neither visible to a textual dry run:
1. `INSTRUMENTS`' length 27 → 28 (`telemetry/metrics.rs`). Lane files added `&FILE_READ` and retention
   `&RETENTION_NODES`, each raising the declared length from 26 to 27 by the same edit, so the line merged clean into an
   array of 28 under a declared 27: a compile error `merge-tree` cannot show.
2. The core's golden test boxes its two halves (`tests_output.rs`, `Box::pin(conversation(&mut out)).await` and the
   same for `budget`). On the merged tree it overflowed its stack 3 of 3 times at the 2 MiB default; it passed at 16 MiB
   with the golden bytes equal, and a bisection had it pass at 2,304 KiB. `#[tokio::test]` pins the test's body on the
   test thread's stack, and lane files' and retention's turn state together passed 2 MiB in a debug build. A daemon
   spawns each turn as a task, on the heap, so this was the test's stack, not a turn's. After it, the golden passed 2
   of 2 and every theseus-core test 1,117 of 1,117 at the default stack. The same overflow was situations'
   theseus-b4sf (P2), of which this is the third option, applied to the golden only.

Its gate (02:02:05 to 02:09:41; 146 s waiting for the shared lock behind two review steps, then 278 s): 2,573 of 2,573
(19 skipped; one slow, the repeating-wake test at 63.5 s); lifecycle ok on the first run, cold start p50 20.9 and p95
22.5 ms, the clean shutdown 32.6 and 45.6, restore (no budget) 141.6 and 152.0; the jobs phase's L1 start p50 5.62 and
p95 6.69 ms; the turn bench at a quiet disk (fdatasync p50 6.5 ms), the plain turn 5 frames at p50 75.4 ms and the
tool-call turn 9 at 157.3, both inside the last 12 gates' spread (74.3 to 81.1 and 153.5 to 180.8). Resident memory
92.0 MB after the 30-turn burst, inside the last 12 gates' 87.4 to 92.4. Pushed; the cloud branch deleted on origin;
the lock's done line at 02:10:45; theseus-6fn.11 closed.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No store format change. One new value, `[memory] arm = "+retention"`, off unless named:
under any other arm, or with `[memory]` off, nothing builds the projection. Health gains the `memory` line and the
gauge. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** `schedule` returns `Option<Retention>` (a science may keep none), not M6's `Retention`. An event's time
is its row's, not the node's. The label set has no "remember"; `should_have` takes its place, graded Easy. 32a's design
named retention as one of `rank`'s weights; as built the weight is 0.5 in one form, `fused × (0.5 + 0.5 R)`, named by
the digest.

**Known gaps.** theseus-cn0b (P2): nothing holds the claim that a shadow turn never builds the projection (R5's second
plant, a turn under any arm building it, passed the suite); the code is right, the test is missing. The weight 0.5 is a
starting point: R5 recommends keeping it for the first canary and tuning it from the exam once its fixture writes memory
rows. The projection keeps every event (about 24 bytes each) to refold exactly; a compaction (keep the fold and the last
position) when a node's uses run to thousands. The memory pass decodes each frame's rows for `retention_written` even on
a daemon without the arm (CPU on the pass's task; a `phase()` check first would skip it, a cleanup). The walk is
unpaced, as `warm_labels` is: R5 left a PSI wait out at the join, since the walk has no stop handle and a wait could hold
a clean stop; if stores grow, page it with `quiet_blocking_unless` and a stop check. The replay (instrument 2) still
recomputes only `none`, `bm25` and `baseline`; `+retention` there needs the projection folded only up to each turn.
R5's recommendations for the owner, each how the code works now: keep 0.5; accept `should_have` graded Easy; accept that a
node with no retention keeps its fused score.

### Item 158. Lane speed: a greeting's reply shows as its model streams, the verdicts it waits on land inside their wait, and trivial routes at 0.4 (theseus-6n5j, theseus-otny and theseus-ck0n; the owner's 23:53 latency items; the `speed` lane, a subagent of the DM thread, 2026-10-04 23:55 to 2026-10-05 02:37, from e6f90af3; 932a3bf8, 8a681c3b and 7e9704ce, with `main` merged in at 783333dc; joined by the lane at 02:36 at a9442c81, a signed merge onto 198e2229; reviewed 02:53 by the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** At 23:45 on 2026-10-04 the owner sent his Theseus DM a greeting. The reply took 5.4 s on Sonnet and cost $0.22.
Its turn trace put the time in three places: route.v1's verdict came late (Jev's call read about 3.2 s from the
message), so the greeting stayed on Sonnet; the reply's text waited for Discord's 1.2 s edit tick; and the whole reply
waited for the settle's frame to be synced (1.38 s on the night's disk). And the section's 0.6 bar would have kept it on
the session's model even in time: on the lane's rig, greetings judged trivial at 0.51 to 0.57 stayed on Sonnet. The owner at 23:53 approved the config half (the Sonnet cache TTL at
1 h, both Jev waits at 300 ms, applied by the DM thread at 23:55) and "please do file and begin the fixes" for the code
half: this lane.

**What landed** (30 files, +1,007 −84 against main; three commits, one an issue).
- **Routing (932a3bf8, theseus-6n5j).** Each mode has its own bar, `[routing.modes.<mode>] switch_confidence`. Unset, it
  is the section's 0.6, except trivial's, which is 0.4 in code, so the owner's config, whose trivial table names only its
  profiles, gets 0.4 with no edit. R3's carry rule (a verdict that comes after its message's wait applies to the
  session's next message alone, when that message's own verdict is late too) now never carries a trivial verdict
  (`routing::carries`): "hi" and "thanks" describe their own message, and carried, a late trivial verdict sent the next
  message, which may be a real question, to Haiku. The switch modes keep carrying, since their verdict describes the
  conversation and the next in-time verdict corrects it.
- **Jev (8a681c3b, theseus-otny).** The judgments a turn waits on (route.v1's batch, a live rerank) write nothing before
  their call: the sink writes the state's blob just before the row that names it, and the budget's block, a frame on the
  first judgment after a start, goes beside the call. A person's message warms Jev's connections as it arrives (two
  `HEAD`s, nothing billed); the client keeps an idle connection 180 s, not reqwest's 90 s; and a Jev known unreachable
  (its last try failed to connect, nothing answered since) is not waited on.
- **Discord (7e9704ce, theseus-ck0n).** The core sends a new notification, `model.answered`, with the loop's whole text,
  the moment the model's stream ends and before the settle's frame is synced. The Discord renderer shows a loop's first
  text at once, and its whole text at once on `model.answered`, as live progress under the keys the reply's post uses.
  The post is still the one durable, exactly-once send: it seals those messages and adds the footer. The
  `discord.message.out` row after each create no longer holds the lane's next write. The core's output golden gained 22
  lines, each a `notify model.answered`, and nothing else; the wire fixture `model_answered.json` is new.

**How it is proven.**
- **The rig** (`theseus-sim fake-discord`, REST and gateway; a paced stand-in for the Messages API, its first text 800 ms
  after the request, then 12 deltas 25 ms apart; the **real Jev**, its key in the scratch daemon's environment only; a
  scratch daemon with the judge on, route.v1 and rerank.v1 live, the 300 ms waits, the 1,200 ms edit tick and trivial
  messages to a `haiku` profile). Before (e6f90af3) and after ran the same config file. On a normal disk, and on a disk
  whose every flush is delayed 1.4 s (device-mapper's `delay` target over a loop device, ext4, the scratch state dir
  only; an `fdatasync` measured 1.42 to 1.52 s), torn down after with block devices, mappings and mounts diffed
  identical. About $0.0025 of Jev (70 judgments, 11 probe calls).
- **The numbers.** First token to visible Discord text: **255 to 514 ms before, 2 to 11 ms after** (before, the edit
  tick's phase). On the stalled disk, the whole reply reached Discord **5.4 s after the stream ended before, 2 ms after**,
  and the first text 263 and 948 ms after the first token before, 2 ms after. **Every greeting detoured to Haiku after,
  7 of 7, against 2 of 6 before** (three judged trivial at 0.51 to 0.57 had stayed on Sonnet). On the stalled disk the
  first message's verdict had been late (the blob's two syncs and the budget's frame before the call, three syncs of
  about 1.4 s), and the second message detoured on the first one's late trivial verdict: 6n5j's carry bug, live. After,
  both verdicts landed inside the 300 ms wait (131 ms on the normal disk, 266 ms on the stalled one for the first
  message after a start).
- **Jev's connection, measured first** (40 connects 2 s apart, no request): DNS p50 11 ms, TCP 25 ms (one try in 40 lost
  its SYN, 1,070 ms), TLS 1.3 45 ms, the whole connect p50 82 ms and p90 143 ms. A cold call took 157 to 434 ms in five
  tries. A kept-alive connection answered after 200 s idle and was closed after 400 s. So the cold connection was not
  the 3.2 s: on the night, route.v1's own call took 288 ms; the rest was the judge's syncs before it. After a 100 s pause
  the call is warm now, 104 ms against 194 before.
- **Tests, each against a planted revert** (all seven reverts at once, each test depending on its own fix): the per-mode
  bar and trivial's 0.4 (config, routing, and a greeting judged trivial at 0.45 detouring, which stayed on Sonnet,
  `unsure`, under the revert); a late trivial verdict never carrying (the next message detoured, `("glm", "detour")`,
  under the revert); the verdict never waiting on its state's blob (`late` under the revert); a message warming Jev's
  connections once (0 warm-ups for 2 under the revert); no wait on an unreachable Jev; a loop's whole answer announced
  before its settle's frame (`tests_m3`); and the renderer's first text and whole answer at once. Plus two theseus-judge
  tests (a warm-up opens connections and a warm client sends none; a failed connect marks Jev unreachable until it
  answers). The files were restored with fresh mtimes and the tree compared byte for byte.
- Two flakes seen under load, neither the change: a telemetry test counted the exporter's retry as a fourth trace POST
  (passed 5 of 5 alone; theseus-qjd6), and two `tests_gateway` tests timed out at 20 s at IO PSI avg60 about 24 % (3 of
  3 alone).

**What the session found.** Moving a write off its caller's path is not moving it off the store's single writer: with
`discord.message.in` written in the background, the admission frame queued behind that row's sync and took 2.9 s instead
of 1.5 s, the same total, so that row stays where it was. A group commit's cap cannot shorten a sync; what decides the
wait is how many syncs lie on the turn's path. ionice cannot help on WSL2's virtual disk. Skipping the wait while
Jev's connection is cold was measured and not built: a cold call landed inside the 300 ms wait each time.

**The join** (by the lane itself, under its lock `lane-speed-join`, taken 02:07:48 behind retention's and lane files'
second join). First origin/main 0ade7d68 was merged into the lane as 783333dc (signed): one conflict,
`crates/theseus-core/src/blobs.rs`, where lane files' PDF-text cache and this lane's test-only hold each added a field,
its init and methods in the same places; both kept; then fmt, shape and clippy clean and the touched packages' tests 453
of 453. The merge at 02:26:39, `git merge -S --no-ff lane/speed` on main 198e2229, gave a9442c81: one conflict,
`scripts/long-files.txt`, where retention and the lane had each raised theseus-protocol's `lib.rs` ceiling to 2,713; the
merged file has 2,717, and the entry keeps both reasons. The warm (test build and clippy) 174 s, clean. The gate
(02:30:10 to 02:36:05, 56 s of it waiting for the lock behind two review steps): 2,607 of 2,607 (19 skipped, 1 slow);
lifecycle ok in 23.9 s, cold start p95 24.3 ms (budget 50 + 7), from the config copy 31.2 ms, a clean shutdown with a
job running 80.4 ms (budget 100 + 4); the L1 start p50 5.92 and p95 6.61 ms; the plain turn 5 frames at wall p50 73.9
ms (daemon p50 52.0), the tool-call turn 9 frames at 157.2, at fdatasync p50 6.4 ms. Pushed at 02:36; the worktree,
branch and target deleted; the done line 02:37:04; the three issues closed with the hash. Main's 203 GB build folder
was moved aside 3 s after this gate exited, at the first quiet moment (theseus-x10i).

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No config edit and no store format change: trivial's 0.4 comes from code, so the owner's
greetings go to Haiku from this install on, and his replies show on Discord as they stream. Detour-recall
(Item 161) ships with it, since every greeting now detours. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** None from the brief's three fixes. The store-write question's answer is a measurement, not a change:
`discord.message.in` is still written before the submit.

**Known gaps.** On a badly stalled disk the provider call still starts about 10 s in, because the turn writes three
frames before it (admission, the input node, the call's plan and dispatch), each a sync, and the judge's and the
binding's frames take the store's one writer between them; fewer frames is C6/S5's (the plain turn writes 5, the aim is
2). The rerank was not exercised live (a fresh store has nothing to recall); its path has the same fix. Follow-ups:
theseus-qjd6 (the telemetry test's retry); theseus-s55d (Jev over HTTP/2 kept warm by pings; it would move every
reqwest client in the workspace, the providers' included, so the DM thread's view is not worth it now) and theseus-ht8b
(a binding's rows riding the turn's next frame, which folds into C6/S5, and a filesystem of the store's own, the owner's
machine and his call), both waiting for the owner. A late switch verdict still carries to the next message alone; letting
it set the hold rather than switch was considered and not built.

