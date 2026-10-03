# Cloud report: three things the cockpit needs from the daemon

Branch `cloud/20261003-cockpit-core`, from `main` at 54e4083. Written 2026-10-03 08:01 UTC, about 80 minutes into
the session. Four commits, unsigned, for the maintainer to re-sign:

| Commit | Issue | Subject |
|---|---|---|
| 35344bd | theseus-9o5n | server: server.started and health name the binary's build, and the ship's log marks an install |
| f5898c2 | theseus-kpz1 | sandbox: sandbox.usage names a running L1 job's command before its row is written |
| e751f24 | theseus-wz4y | turn: each turn counts its own frames on its trace, and the turn bench checks the count against the WAL |
| 5058ef6 | theseus-wz4y | cockpit: the speed wall's live frames dial leaves out a session's first turn |

The fourth commit is a fix-up of the third that the live check found after e751f24 was pushed. I couldn't amend it
without a force-push, so it is a commit of its own. Squash it into e751f24 at the merge if you want one commit per
issue.

No new dependency, no new protocol method, no new frame, and no new store write on a turn's path. The plain turn stays
at 5 frames, both in `tests_m3::a_plain_turn_stays_within_its_frame_budget` and in the turn bench, every run.

---

## 1. theseus-9o5n: `server.started` names no build

**What I found.** `announce_serving` wrote `{"startup": …}` and nothing about the binary. Health had `version`, but not
the commit. The cockpit's `marksOf` (in `timemachine.ts`) knew only start, crash, and stop.

**What changed (35344bd).**
- `theseus_protocol::Build { version, commit? }` is in `health.rs`, not `lib.rs`, because `lib.rs` sits at its
  2,600-line shape ceiling. `HealthResult.build` is new, and so is `server.started`'s `data.build`. The TypeScript
  is regenerated.
- `crates/theseusd/build.rs` sets `THESEUS_COMMIT` at compile time. An explicit `THESEUS_COMMIT` in the build's
  environment wins. Otherwise it takes `git rev-parse HEAD`, or nothing when there is no git. It reruns when HEAD,
  the branch HEAD names, or `packed-refs` changes, each found with `git rev-parse --git-path`, so worktrees work. It
  watches only paths that exist; cargo would rerun the script on every build otherwise.
- `theseusd`'s `main` calls `theseus_core::set_commit(env!("THESEUS_COMMIT"))` first thing (a `OnceLock`).
  `theseus_core::build()` is what health and the row read. Nothing is computed at a start.
- **Why the build script lives in theseusd, not the core:** a commit then rebuilds theseusd's own crate, not
  theseus-core and everything above it. The cost is that a core in a test names no commit unless it sets one, as
  the core's test does.
- In the cockpit, `marksOf` moves to a pure `src/lib/marks.ts` and gains an `install` mark: a start whose build
  differs from the start before it. A build named after starts that named none counts as an install too, since the
  older binary could not name one. Starts that all name none do not count. `TimeMachine.tsx` gives install its own
  colour and word.
- `cockpit/test/marks.test.ts` is the cockpit's first test. It runs on node's own runner with node's type
  stripping (`npm test`, no dependency). **`scripts/gate.sh` changed**: its `cockpit` phase now runs
  `npm run -s test` between the lint and the build. That is a shared file, so move or drop the line at the join if
  you'd rather. `scripts/AGENTS.md` and `cockpit/AGENTS.md` say so.

**How I proved it.**
- `rpc::tests::server_started_and_health_name_the_build`: passes. With `"build"` planted out of the row, it fails.
- `theseusd versions::each_start_names_its_build_and_a_restart_of_the_same_binary_names_the_same_one` runs the real
  binary on the old fixture store:
  - the fixture's own starts name no build;
  - the new start names `CARGO_PKG_VERSION` and the 40-hex `THESEUS_COMMIT`, and health names the same;
  - after a SIGTERM and a second start, the build is the same.

  It passes. With `set_commit` planted out, it fails with `left: {"version":"0.0.1"}` and `right: {…, "commit":
  "54e4083…"}`. With the row's field planted out, it fails too (no build in 40 s).
- `npm test`: 5 of 5 pass. With the install branch planted out, 4 fail; the fifth has no install in it.
- Under load (four `yes` loops at nice -5): the versions test passed 5 of 5.
- **Live.** I kept the turn bench's store with `--dir` and served it again with the web UI on port 7434. The first
  two starts named `f5898c2`. I rebuilt the daemon at e751f24 and started it again on the same store, and the ledger
  read `['f5898c2', 'f5898c2', 'e751f24']`. The ship's log's title read "14 marks: 2 starts, 9 turns, 2 stops, 1
  installs (a new build)".

**What's left or uncertain.**
- **A dirty tree names the commit it sits on.** The live check showed it: a binary built from `f5898c2` plus the
  uncommitted wz4y code named `f5898c2`. Installs are built from reviewed commits, so this should not matter there.
  If it does matter, a `-dirty` suffix (from `git status --porcelain`) would need the script to watch the index and
  every source file.
- `theseusd --version` still prints only the version. Adding the commit there would be a one-line follow-up.

## 2. theseus-kpz1: a running L1 job's command arrives only with its turn's next frame

**What I found.** The command was already in memory, in the job's call: `sandbox::started` runs at the launch and
already has the argv. The row it waits for is written in the frame that answers the call, whether the job ended
(`answer_job`) or went to the background (`answer`), and both happen before the call returns.

**What changed (f5898c2).**
- `Sandbox` keeps an in-memory `running` map: correlation id → `RunningJob { correlation_id, session_id, tool, argv,
  started_at_ms }`. `sandbox::started` adds an L1 job to it and returns a guard. The job's call holds the guard
  (`let _running = …` in `toolrun/job.rs`), and dropping it removes the entry when the call returns.
- `sandbox.usage` reports `running`, which is new on `SandboxUsage`, skipped when empty, with its TypeScript. It is
  there in every case, with or without a delegated cgroup. On this VM no cgroup is delegated, so the board shows no
  gauges, but it still names the job.
- The boundaries board takes the command from `running` while there is no `tool.job_started` row. It lists a running
  job that has no cgroup, without gauges and saying why, and polls every second while anything runs.
- This adds no frame, no write, and no method. `action.list` is unchanged, since actions keep no arguments.

**How I proved it.**
- `theseusd sandbox::a_running_l1_jobs_command_is_read_before_its_row_and_matches_it` uses a real L1 job on the real
  daemon. While the job runs, `running` names its argv, tool, and session, and the ledger has no `tool.job_started`
  row yet. After it ends, the row has the same argv and correlation id with class `l1`, and the job is no longer
  listed.
- With the list planted out of `usage()`, the test fails: "the job was never listed". Restored and touched, it
  passes.
- Under load (four `yes` loops at nice -5): 6 of 6 passed, about 0.4 s each.
- **A trap I hit.** The first version released the job by creating a file in its workspace. The job's overlay
  ("scratch") kept its first, negative look at that name, so under load the job missed the file and the turn sat out
  its 30 s `proc_sync_secs` bound. It still passed, but slowly. The test now ends the job's `sleep 600.<test pid>`
  through `/proc`, as the cancel test finds its sleepers.
- **Leftovers.** The planted-revert runs left their jobs running (the test panicked before ending them). I stopped
  them by pid, and nothing of mine is left running.
- `rpc::tests::sandbox_usage_reads_each_jobs_cgroup` still passes.

**What's left or uncertain.**
- The background path (a job that outlives `proc_sync_secs`) has no test of its own. Its row is written by
  `self.answer(…)` before the guard drops, by construction.
- Only L1 jobs are listed, since the board is about L1. L0 jobs could be added the same way if wanted.
- The board's "running" row (no cgroup, no gauges) was not seen live: the scratch daemon had no model to run a turn.
  All eleven views loaded clean, the boundaries board included.

## 3. theseus-wz4y: frames per live turn are not in the record

**Which way, and why.** I took the first option: a count the turn's existing trace row carries (`frames` on the root
span of `turn.trace`, and on the result's trace). It is computed as the turn runs and writes nothing.

A read-only method counting from the WAL's tail would have to pick one turn's frames out of everything else the daemon
writes. A window by time can't do that once two sessions run at once, while the turn knows its own frames exactly.

**What I found.** A turn's frames come from three places:
- **Its store handle.** Every write the turn makes after admission goes through `TurnState::commit`.
- **The frames before the handle exists:** opening the execution on a session's first turn, a superseded budget
  question, and the admission with its wakes.
- **A job's completion that the spool's drain accepted.** The drain is woken by the job's wrapper, and when it gets
  there before the turn's 50 ms look, it writes that frame on the driver's path. The turn bench caught this one: a
  tool-call turn counted 10 where the WAL held 11.

**What changed (e751f24, 5058ef6).**
- `TurnState` keeps a frame counter. `Store::turn_frames()` reads it, and `Store::count_frames(n)` adds frames
  written for the turn by another path.
- `theseus_store::frames_written_here()` is a per-thread count of the frames the store's writer answered for that
  thread's callers. It is bumped in `WalStore::append` on the caller's side.
  - It can't be counted in `Wal::write`, because the writer thread writes every frame. My first try did exactly that
    and counted nothing.
  - `run` and `admit` read it around each stretch with no `.await` (`execution_for`, the supersede, `admit_input`,
    `wake_input`, the stop at admission), so the difference is the turn's own, whatever other threads write.
- `job.rs` checks whether the turn's own look settled the job. If its handle wrote no frame and the action is
  settled but not cancelled, the turn adds 1: the drain's frame.
- `finish` writes `"frames": turn_frames() + 1` into the root span's attributes. The +1 is `end_turn`'s frame, which
  carries the trace row. A failed turn's trace has no count.
- `theseus-sim bench turn` now fails if any measured turn's trace count differs from walcount's count of the same
  turn. That puts the agreement check in every gate.
- The speed wall's frames dial reads this daemon's last 50 plain turns (one loop, `no_tool_calls`) at p95, with the
  gate's bench as its pointer, or the bench alone when no turn has a count.
  - **5058ef6** leaves each session's first traced turn out, the way the bench leaves out its warm-ups. The live check
    showed why: a session's first turn writes 6 frames in the daemon (8 in the core test, which also opens the
    execution), so the dial read p95 6 against a budget of 5.
- The core's `AGENTS.md` frame-budget invariant says how the count works.
- **The output golden:** each traced turn's span gains `"frames":N`, and nothing else changes. I wrote it with
  `THESEUS_GOLDEN=write` and then put back the one-byte digit drift this VM writes (theseus-6a7o), so it matches
  main's golden with only the counts added. I checked that with `diff` after stripping `"frames":N,`. On this VM the
  test still fails at the same line 1041 as on main.

**How I proved it.**
- `tests_m3::frames_counted::each_turn_reads_its_own_frames_and_the_store_agrees`: a first turn reads 8, a plain turn
  5, and a turn with an `fs_read` call 9. Each equals the store's own `frames_appended` across the turn, on both the
  result and the `turn.trace` row.
  - With the admission's count planted out, it fails (`warm up: the store wrote 8`, left 5, right 8).
  - Under load (four `yes` at nice -5): 5 of 5 passed.
- `theseus-sim bench turn --check`: plain turns are 5 and tool-call turns 11 by both counts. I ran 5 turns of each
  kind 3 times, the join's form (10 runs, a burst of 30) once, and 3 more runs under load. All passed.
  - With the drain's count planted out of `job.rs`, all 3 runs fail: "a tool-call turn's trace counts Some(10)
    frames, and the WAL holds 11".
- **Live**, on the bench's kept store served again: the traces read `(loops, frames)` = `(1, 6)` for the session's
  first turn, `(1, 5)` for the plain turns, and `(2, 10)` for the tool-call turns. The dial read "live p95 5 of 4
  turns · no bench history".

**What's left or uncertain.**
- **A tool-call turn writes 10 or 11 frames, depending on who accepts the job's completion.** In the turns I saw,
  the drain's acceptance was the 11-frame case (the turn's frames plus the driver's one). In the live run, the turn
  accepted it itself and the WAL held 10. Both counts agree every time (the bench checks), but the history's
  `frames_tool` can move between 10 and 11.
- **A cancel's frame during a turn is not counted** to the turn; the bench never cancels. A deadline's reconcile
  that settles the turn's own job is counted (it settles as `OutcomeUnknown`, not `Cancelled`).
- **The first turn's count leaves out `session.open`'s frame,** which comes before the turn.

---

## The gate

I ran `THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Each time, fmt, shape,
clippy, the bench and test builds, and the reader rule passed, and the suite failed. I then ran the phases after it by
hand (`machine_checks`' remaining phases and the unlocked ones): protocol types, the turn bench (5 runs, no burst),
`cargo deny --offline check`, web, cockpit (with its new `npm test`), and the web dist check. All passed for each
commit.

**Setup.** This fresh VM needed `cargo fetch --locked` and `cargo deny fetch` once before the offline deny check could
run; I changed nothing in the repository for that.

The last gate (5058ef6) ran 1,720 tests: 1,715 passed, 5 failed, 13 skipped.

**The three known failures, every run:**
- `theseus-sandbox::contract clause_09_limits` (root and `RLIMIT_NPROC`, theseus-pv6i).
- `theseus-core tests_m3::parallel::five_reads_and_two_greps_take_the_slowest_call_not_the_sum` (4 cores,
  theseus-i1i4).
- `theseus-core tests_output::the_cores_output_matches_its_golden` (the digit drift at line 1041, theseus-6a7o).

**Intermittent, not on the flaky list, and none in code I touched.** Six full suite runs: one before the split, the
four gates, and one more after gate 3.
- `theseusd::cred a_job_that_runs_across_a_restart_asks_and_is_answered`: in 2 of 6 suite runs and 2 of 5 runs alone.
  It fails with "not served again". The test waits for the job's socket to come back with a *new inode*, but on this
  VM's ext4 a socket bound again at the same path takes the same inode. A Python check rebound one 20 times and got
  the same inode 20 out of 20. So when the inode is reused, the test never sees the new socket. A fix would be to
  compare something else (the socket's ctime, or a connect that the new daemon answers). This is in `cred.rs`'s test
  area, which I was told to leave alone.
- `theseusd::tender a_stop_does_not_wait_for_the_tender`: in 2 of 6 suite runs. The tender is a zombie ('Z') where
  the test expects it stopped ('T').
  - Run alone, 10 times under four `yes` loops at nice -5, it fails 1 of 10 on this branch and 1 of 10 on main
    54e4083 under the same conditions.
  - An earlier 8 of 10 on this branch ran it beside the discord test below, which made the load heavier. It is a
    load flake on main too.
- `theseus-discord tests_gateway::a_post_whose_place_gained_a_viewer_is_held_and_its_answer_decides`: once in 6
  suite runs (at `tests_gateway.rs:535`). It passed 5 of 5 alone and 10 of 10 under load. This is the labels area,
  which other sessions are changing.

**Not run here:** the lifecycle bench (`THESEUS_GATE_NO_BENCH`). The start path gains only one `OnceLock::set` of a
`&'static str` before anything else, and the row's new field rides the frame `announce_serving` already writes.

## For the docs (I did not edit them)

- **The spec's Part III:** one item for the three issues. 9o5n's build constant and its build script in theseusd.
  kpz1's in-memory `running` list. wz4y's count on the trace, with the first-option reasoning above, and the bench's
  agreement check. Its findings:
  - The store's writer thread writes every frame, so a per-thread count must be taken on the caller's side.
  - The spool's drain can take a turn's job completion.
  - A session's first turn writes more than 5 frames (6 in the daemon).
  - An L1 job's overlay can keep a negative lookup of a path created under it, so a file is no signal to a running
    job.
  - ext4 reuses a socket's inode when it is bound again.
- **`docs/status.md`:** what works today gains health's `build` and `server.started`'s `build`; the ship's log's
  install mark; `sandbox.usage`'s `running`; each turn's `frames` on its trace and the speed wall's live frames dial;
  and the cockpit's first test (`npm test`, in the gate).
- **`docs/technical-overview.md`:** where it shows a turn's trace or `bench turn`, it can mention the trace's `frames`
  and that the bench checks it against the WAL.
- **The README:** no change. Its 5-frame promise is unchanged and now visible live.
