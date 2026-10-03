# The Ship of Theseus, chapter 10: Part III, A3c, M3.5 Fast ([index](README.md))
## A3c. M3.5 Fast (theseus-qa0)

M3.5 (P5b) runs as these steps:
- F1: serve first, and the lifecycle bench.
- F1b: start from a last-known-good copy of the config note (theseus-2fo, accepted by Eddie 2026-09-29).
- F2: fewer frames and one transcript read per turn.
- F3: parallel tool calls (theseus-a60).
- F4, in two steps:
  - F4a: versioned readers, a newer store refused, and a tail-only store open (theseus-8ni). Built
    2026-09-30.
  - F4b: the swap and restore phases, the store-lock race, and F2's remaining frame merges (theseus-l6y). Built
    2026-09-30. **M3.5 closed** at F4b's review.

Beside them, in the same chain:
- K1: the kernel never loses an update (theseus-id9), found by F3.
- O1: the native OTel exporter (theseus-hee), with theseus-gi7.
- J1: a job cannot answer approvals (theseus-6qy).
- Z1: the daemon reaps its job wrappers, and adopts a job's orphans (theseus-z4b).
- B1: the secret broker (theseus-dcy), with theseus-6uo.

Eddie's order (2026-09-29) is:
1. F1, F2, F1b, and F3;
2. then the native OTel exporter, the self-approval fix, and the secret broker;
3. then the Daily Driver's items 5 to 8, before his end-to-end testing.

F4 comes after that.

### Step F1. Serve first, and the lifecycle bench in the gate (theseus-qa0; 2026-09-29, 08:54–09:18 and 09:49–10:52; 10c35c4, 269642f, 36e2173, 460a35b)

**Why.** Cold start took 1.3 s to the first `health` answer, and 1.0 s of it was `op read`s before the
store opened (A3, lifecycle timings). Eddie made FAST a primary goal on 2026-09-27 (§2).

**What exists.**
- **Serve first.** Secrets resolve in the background into a board:
  - one `op inject` for every reference, `op read` per reference after a failed injection;
  - a retry of what failed at 5 s, doubling to 60 s.

  Each consumer waits for its own secret, and none runs without it. A turn waits for its key, bounded
  at 30 s, or is refused with `secret_failed` or `secret_resolving`. The Discord binding, the GitHub
  check, and the telemetry exporter wait for theirs. Health says `secrets: resolving | ready | failed
  <names>`. The ledger gets `secrets.resolved` and `secrets.failed`, and the Observatory has a Startup
  section.
- **The start path pays one fsync of its own.** The kernel's five `startup.step` rows share a frame.
  `server.started` goes after serving, in one frame with `server.serving`. The harness loop and the
  driver start once the socket answers. redb's open adds its two.
- **The store's read path.** A read is one `pread` on a kept segment handle, and a batch of positions
  shares one index transaction. Startup reads every execution once, not twice.
- **The lifecycle bench**, `theseus-sim bench lifecycle`:
  - phases: cold start; clean shutdown with executions waiting and a job running; SIGKILL, then restart;
  - the job is a real `proc.run` from a real turn against a stand-in for the Messages API;
  - secrets come from a fake `op` that answers after 1 s;
  - stores: empty, a copy, or synthetic (`--sessions N`, 250 sessions to a frame);
  - `--config` runs the operator's own config with the real `op`.
- **The gate** runs it on every commit: debug, empty store, ten runs a phase, 4.7 s. Each p95 is held
  to §9 plus a measured per-phase margin (7, 4, and 25 ms). A throwaway 100 ms sleep in cold start
  failed it.

**How it is proven.** 256 tests in the gate, 20 of them new:
- serve first under a vault that hangs, answers late, or fails;
- the binding without its token;
- the bench's arithmetic, budgets, and config;
- the synthetic store read back through the kernel and the core.

The frame-budget test holds at 17. Release, at 460a35b:

| Store | Cold p50 / p95 | Shutdown p50 / p95 | Kill p50 / p95 |
|---|---|---|---|
| empty | 17.2 / 31.3 ms | 33.5 / 44.9 ms | 36.3 / 56.1 ms |
| Eddie's copy | 18.4 / 21.7 ms | 33.6 / 38.7 ms | 35.3 / 45.4 ms |
| 10,000 sessions | 124.7 / 142.7 ms | 22.5 / 34.4 ms | 147.0 / 154.3 ms |

With Eddie's config and real secrets, the first answer took 1 075–1 104 ms before and 18.3–18.7 ms
after, with `secrets: resolving` at each. A GLM turn sent at once waited 1.32 s for its key and then ran.

**Review** (11:18–11:21). The gate reran at 256 tests, with LIFECYCLE OK (cold p95 24.6 ms of 50, shutdown 49.2 of 100, kill 45.1 of 150). Tabitha's own check on the release build, over a store copy, with Eddie's real secrets: the first `health` answer came at 20–21 ms warm (57 ms on the copy's first open), with `secrets: resolving`. A GLM turn sent right after the start showed `secrets.wait 1.02 s` in its trace, then answered, and health then said `ready · 7 ready 1071 ms after start (inject)`. Installed at 11:21. The first run (08:54–09:18) was killed when an abort in its parent DM session cascaded to it; its uncommitted work was backed up, and the second run continued from the tree.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The process refuses to start if a referenced secret cannot be resolved (§3.19) | The process serves. Each consumer refuses to run without its secret, and health, the ledger, and the Observatory name the failure and why | FAST (§2): the socket never waits on the network | §3.19 amended |
| References resolve concurrently through `op read` (§3.19) | One `op inject` for every reference; `op read` per reference only after a failed injection | Measured: the same wall time, a sixth of the CPU, one process | Keep |
| Resolution happens once at startup (§3.19) | Once, then again for what failed, at 5 s doubling to 60 s | The process no longer exits on a failure, so it must fetch again | Keep |
| The bench has five phases (P5b) | Three; binary swap and restore are F4's | A swap under load needs F4's upgrade path | Built in F4b: swap and restore |
| Nothing on the path to serving waits on the network (§2) | When the config is `op://` (Eddie's), the note is read before serving: about 1 s | Everything after it needs the config | Built in step F1b (theseus-2fo): the daemon starts from a last-known-good copy of the note, and nothing acts until the vault confirms it |
| Store open grows with the WAL tail, never with history (§9) | `Wal::open` reads and checks every segment: 47 ms at 10,000 sessions | Since M1 | F4, with the versioned readers (theseus-8ni) |
| The gate measures §9 on every commit (P5b) | On debug binaries and an empty store; Eddie's store and 10,000 sessions are release bench runs | A release build takes 3 min, and debug is never faster than release | Keep; revisit if a debug-only slowdown fails the gate |

**Known gaps carried forward:**
- health, the heartbeat's reconcile, and the driver's 500 ms tick each read every execution, and health
  also reads every session and action: O(all), where O(open) would do;
- redb's open costs two fsyncs, and its repair after a SIGKILL about 26 ms;
- the Observatory re-lists every execution and session every 2.5 s.

### Step F2. Fewer frames per turn, and one transcript read (theseus-qa0; 2026-09-29, 11:22–12:18; 6402e80, 49c4db4)

**Why.** A plain one-loop turn wrote 17 WAL frames, each an fdatasync of about 7 ms here. Every turn also
decoded its whole transcript at least twice, plus once per loop (the complexity review, findings 3 and 8;
Part III A3b).

**What exists.**
- **A turn's own handles.** `Store::for_turn` and `Kernel::view` give each turn a store handle, and a
  kernel that commits through it. The turn's observability rows wait there. They ride at the front of
  the next frame that either one commits for the turn, in order. `provider.call` is in its completion's
  frame. The turn flushes whatever still waits at its end.
- **`Kernel::plan_and_dispatch`.** The provider call, and every tool call the policy runs, are planned,
  authorized, and dispatched in one frame. The kernel simulator plans half its actions this way. It holds
  a new invariant: such an action is never found planned or authorized, crash or no crash.
- **One transcript read per turn.** The turn's handle keeps its session's transcript
  (`Vec<(u64, Arc<Node>)>`). It is read at the first reader, and extended with every node that the
  turn's frames write. A debug build compares it with the store at every read, and panics on a
  difference. Every node write in a turn goes through the turn's handles, so no writer can miss the list.

**How it is proven.**
- The gate passed at each commit: 260 tests, then 262 (6 new), with the lifecycle bench within its
  budgets both times.
- The batch C output-diff probe, with each frame's grouping added, ran across its 26 scenarios. After
  masking, every record, notification, and result is identical. Frames went from 1,116 to 624 (−44%).
  The second commit's dump is byte-identical to the first's.
- `kernel-sim` (18 seeds, 105 crashes, every invariant held) and `crash-test` (zero committed records
  lost).
- A copy of Eddie's store was read under the old and the new binaries: every session list, history, and
  confirm list identical. A GLM tool turn wrote 13 frames where it would have written 30, and the old
  binary reads its session identically.

Release, medians of three interleaved rounds:

| Turn | Before | After | Frames | Transcript reads |
|---|---|---|---|---|
| plain | 123.0 ms | 58.0 ms | 17 → 8 | 2 → 1 |
| two loops, one tool | 211.6 ms | 90.0 ms | 29 → 12 | 4 → 1 |
| plain, 200-node session | 135.7 ms | 65.9 ms | 17 → 8 | 2 → 1 |
| two loops, 200-node session | 235.8 ms | 104.6 ms | 29 → 12 | 4 → 1 |

**Reviewed** (Tabitha, 2026-09-29, 12:23 to 12:31).
- The gate rerun passed: 262 tests, with the bench's cold start at p95 35.4 ms against 50.
- On the release build, over a fresh copy of Eddie's store, the old and the new binaries gave identical
  output: the session list, and all five histories, as text and as JSON.
- A GLM turn with one `fs_read` wrote 13 frames, and a plain follow-up turn wrote 8. Each outbox (plan,
  authorize, dispatch) was one fsync of about 7 ms.
- The old binary reads the new session byte for byte.
- Installed at 12:30.
- Found in passing: `theseus shutdown` removes the socket before the process releases the store's lock.
  So a start that follows at once can fail with "Database already open", which F4's swap phase must
  handle.

**Divergence from Parts I and II, and from the review.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Each node and row is written in the frame of the kernel transition that produced it (§4.6) | Nodes, yes; observability rows ride in the turn's next frame | FAST: each frame is an fdatasync | §4.6 amended |
| The review: `turn.ended`, `turn.trace`, and the session write in one frame | `turn.ended` and `turn.trace` ride with `end_turn`'s frame | The trace times the session write; the count is the same | Keep |
| The review: `take_results` returns early when nothing is queued (one frame) | It always did; the plain turn's `results_consumed` frame consumes the provider call's own queue entry | Dropping the entry changes what a fault leaves | Drop it, with an explicit wake on a fault (Tabitha at review): theseus-l6y, with F4 |
| The review: `plan_and_dispatch`, with the provider call as its first user | Also every tool call the policy runs | A loop with one tool: 12 frames → 4 | Keep |
| The review: push each node the turn writes onto the list | The turn's handle adds every node its frames write, from the bytes written | No writer can forget one; a debug build checks the list at each read | Keep |
| Per-turn harness overhead under 5 ms (§9) | About 58 ms for a plain turn: 8 fdatasyncs of about 7 ms | The disk | Recorded; theseus-l6y takes a plain turn to about 5 frames |

**Known gaps.** These are theseus-l6y's:
- the plain turn's `results_consumed` frame;
- `wake_input` and `admit`, and a confirmed call's `authorize` and `dispatch`, which are two frames each;
- the session write, which could ride in `end_turn`'s frame;
- `confirm.list`, which still reads a transcript for a question's reason.

_(The first three merged in F4b; `confirm.list`'s read is theseus-ef0.)_

Beyond those, each loop still renders the whole transcript into its request. That is §4.5's rendered
cache, which is not built.

### Step F1b. Start from a last-known-good copy of the config note (theseus-2fo; 2026-09-29, 12:33–13:39; 98bce09, 06b8d53, 2b654d4, 6299536)

**Why.** Eddie's daemon runs as a bare `theseusd`, so its config is the vault note, read with one `op read`
(about 1.0 s) before anything else. F1 moved every secret behind the socket, which left the note as the last
network wait on the start path. Everything after it needs the config, so it could not simply move. Eddie
accepted the fix on 2026-09-29 at 11:52 ("That's perfect in practice"). Tabitha's refinements made the vault
the only authority.

**What exists.**
- **The copy**, `<state dir>/config.last-good.toml`: the note's exact text under a line naming its reference,
  mode 0600, written after serving. It is never kept for a note whose URLs could carry a credential, and it
  is on the tool floor.
- **Serve from the copy; act only on the vault's word.** One check in the dispatcher:
  - 8 methods that act (`turn.submit`, `session.open`, `profile.use`, `session.recompile`, `action.confirm`,
    `policy.tighten`, `policy.untighten`, `execution.cancel`) wait for the vault, bounded at 30 s, then fail
    with `config_unconfirmed` (-32006);
  - the 16 reads, and `shutdown`, answer at once;
  - the actors start on the confirmation, and each also waits on the gate itself;
  - a turn's trace shows `config.wait` when it waited.
- **The vault's answer:**
  - the same text, or only comments, confirms (`config.confirmed`);
  - a changed note is ledgered (`config.changed`), rewrites the copy, and restarts the daemon in place: an
    `exec` of `/proc/self/exe`, marked so that it never restarts twice;
  - an invalid note or a silent vault holds (`config.invalid`, `config.unreachable`), and retries at 5 s,
    doubling to 60 s;
  - health, the Observatory, and the narrative (a new `config` part) say which.
- **The restart keeps the process:** the same pid, terminal, arguments, and name. The name, `theseusd`, is
  written back to `/proc/self/comm` after the exec, since the kernel would name the image `exe`.
- **Startup writes nothing a copy decides.** The kernel had one config-dependent startup write: the dollar
  limit given to executions stored with unit budgets. It is refused under an unconfirmed config, and that
  start reads the vault first.
- **`[web] bind`** must be a loopback address, or the config fails to load.
- **The bench's `vault` phase:** cold starts from the copy, with a fake `op` that answers after 1 s. Each
  first answer must say `confirming`. It adds about 0.6 s to the gate.

**How it is proven.** 280 tests in the gate, 18 of them new:
- the gate under a vault that hangs, answers late, answers the same text, only comments, a changed note, an
  invalid note, or nothing;
- no restart loop;
- the security test: a copy widened to let the CLI approve never judges an approval, and the vault's version
  refuses it;
- the copy on the tool floor;
- the kernel's refusal of unit budgets under an unconfirmed config;
- the real binary: the copy kept, the start from it, the restart by `exec` in the same process under the
  same name, the vault-first exec, a comment-only change, and a silent vault.

The bench's `vault` phase on an empty store, release: p50 17.5–19.2 ms and p95 27.8–42.3 ms over three runs,
with every first answer `confirming`.

With Eddie's real note, through a shim `op` that turned the web UI and Discord off:
- A first start with no copy answered in 1,094 ms. Three starts from the copy answered in 18.2, 20.2, and
  21.5 ms, and the vault confirmed each 1.02–1.05 s after its spawn.
- A GLM turn sent at a start waited 1.01 s at the gate (`config.wait`), then ran.
- A changed note restarted the daemon in place: 148 ms from `config.changed` to serving again, and confirmed
  in 992 ms, with the vault's limit.
- A comment-only change confirmed, with no restart.
- An unreachable vault held, said why, refused an acting method after 30 s, and confirmed on its fifth read.

**Reviewed** (Tabitha, 2026-09-29, 14:03 to 14:13).
- The gate rerun passed: 280 tests, and all four bench phases within budget (from the copy, p95 25.0 ms).
- On the release build, with Eddie's real note through Tabitha's own shim, and an empty scratch state dir:
  - a first start answered in 1,065 ms ("read before serving … there was no copy yet"), and kept a 0600
    copy of 13,781 bytes under its reference line;
  - the next start answered in 31 ms, saying `confirming`. A GLM turn sent at once waited 1.02 s at the gate
    (`config.wait`), then answered, and the daemon said `confirmed in 1041 ms`;
  - a note changed through the shim (`[kernel] spend_limit_usd = 60.0`) gave `config.changed`
    (`["kernel"]`) and the restart. The pid stayed the same (2992727), `/proc/<pid>/comm` stayed `theseusd`,
    and the vault's limit, $60, took effect.
- Installed at 14:13.
- Found in the review:
  - F1b's real-binary test leaks its daemon when an assertion fails before its explicit `stop()`. One
    restarted debug daemon from a deliberately failing probe ran for 45 minutes, named `exe`. The review
    stopped it, and the fix, a guard that kills on drop, goes with the OTel step.
  - `theseusd config | head` panics on the broken pipe (theseus-gi7, with the same step).

**Divergence from Parts I and II, and from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Resolution happens once at startup, and again on an explicit `config.reload` (§3.19) | No `config.reload`; a restart is the reload, triggered by the vault's answer or by the operator | A config the process holds in memory is the config it started with; a restart is routine (§3.22) and cheap | §3.19 amended |
| The config note is read before serving (§3.19, F1) | Served from the last-known-good copy; the vault is read behind the socket, and nothing acts until it confirms | FAST (§2), with the vault as the only authority | §3.19 amended |
| The issue's first proposal: "restart to apply" (a stale copy keeps serving) | The daemon restarts itself onto the vault's version; a restarted one that finds another change holds | Tabitha's refinement: nothing may act on a file an agent can write | Keep |
| The brief: clients waiting at the gate see their connection close | Each is answered first with `config_unconfirmed` (state `restarting`: "send this again once it answers"), then its connection closes | A stdio client would otherwise wait forever, and a socket client learns why | Keep |
| The brief's list of methods that act (7) | 8: `session.open` too | It writes a session and opens an execution whose spend limit is the config's | Keep |
| Kernel startup writes depend on no config value that decides policy (the brief's expectation) | One did: the unit-budget migration's dollar limit | Since theseus-0sg | Fixed: refused under an unconfirmed config, and read from the vault first |
| The Discord DM after a restart | Implemented (2b's approval-DM path, one call), not proven live | A scratch daemon cannot bind while Eddie's holds the bot token | Prove at Eddie's next restart onto a changed note |

**Known gaps.**
- The Observatory's config line was not seen in a browser, because the live checks kept the web UI off
  while Eddie's daemon held port 7433.
- A job that can write the copy can hold the daemon, but never make it act. That is a denial of service,
  not an escalation, and L1's sandbox (M4) takes the write away.
- The restart's 100 ms grace is a delay, not a handshake.
- The copy is written without fsync, so a copy lost in a crash costs one slow start.
- A start spawned the moment `theseus shutdown` returns can still fail with "Database already open" (F4). _(Fixed in F4b: the start waits for the lock.)_

### Step F3. Parallel tool calls (theseus-a60; 2026-09-29, 14:15–15:10; a579f63, d421e1b, 59b3d9f)

**Why.** Eddie, 2026-09-28 23:57: "the most performant way to run trivially parallelizable tasks is to avoid os
threads and use truly async code." A response's tool calls ran one after another, so five reads and two greps
took the sum of their times. In-process toollets ran on tokio's blocking pool, bounded only by its 512
threads.

**What exists.**
- **Gate first, then run in groups** (`ToolRuntime::run_calls`).
  - The calls are gated in order. An unknown tool or invalid input is answered at once.
  - The first call that waits for the operator asks after the calls before it, and the calls after it wait
    for the continuation.
  - Consecutive reads run as futures in the turn's task (`join_all`, no task per call). A write or a program
    runs alone.
  - Each call keeps its own two frames, and every kernel call stays in the turn's task, one at a time.
  - A continuation runs its ungated calls the same way.
- **The request does not change.** The compiler already placed each result after the call it answers,
  whatever its position.
- **The trace shows the overlap.** A group's calls sit under a `tools` span, and `theseus ask --trace` draws
  them on that span's own time.
- **A fixed CPU pool** (`CpuPool`): a semaphore of `available_parallelism()` permits, held by every
  in-process toollet while it runs. A call's deadline, and its time, are its run's own.
- **`fs.grep` borrows free cores** for a big tree. The walk stays sequential. Past the first 256 files,
  chunks of 64 go to every core that is free at that moment. The merge, in walk order, keeps the output
  identical.
- **The kernel simulator** dispatches 3 to 6 actions at once in a quarter of its turns, with crashes after
  the dispatch and between the completions.

**How it is proven.**
- The gate passed at each commit: 288, 290, then 291 tests, 11 of them new, with the lifecycle bench within
  its budgets at all three.
- **The output-diff probe,** with each provider request's digest added:
  - its 26 scenarios are byte-identical to the parent's after masking, and so are all their requests;
  - a 27th scenario, five reads and two greps in one response, sends the same requests;
  - its records, notifications, frames, and history reorder only inside the batch.
- **The simulators:** `kernel-sim` (22 seeds, 746 crashes, 83 of them inside a batch, every invariant held),
  and `crash-test`.
- **A copy of Eddie's store** under the old and the new binaries, with GLM turns. The old binary reads the
  new binary's session byte for byte.

| Measure (release) | Before | After |
|---|---|---|
| a GLM response of 5 reads and 2 greps: the calls' wall time | 407–411 ms (their sum) | 168–184 ms (the slowest, 135–149 ms, plus its start) |
| one `fs.grep` over `~/projects/openclaw` (49,370 files), in the daemon | 291–294 ms | 123–126 ms |
| the same, in the benchmark: a full scan / the walk alone | 284 ms / 82 ms | 114 ms / 82 ms |
| 32 CPU-bound calls in 4 sessions on 16 cores: most at once | | 15–16, never more |

**Reviewed** (Tabitha, 2026-09-29, 15:17 to 15:25).
- The gate rerun passed: 291 tests, and all four bench phases within budget.
- On the release build, over a fresh copy of Eddie's store, a GLM response of four `fs.read`s and one
  `fs.grep`:
  - ran its five calls together, under one 69.0 ms `tools` span, each call about 41 ms;
  - answered right: the `stable` channel, and 3 files for the grep, as `grep -rl` says;
  - wrote 24 frames, with the session's open.
- The old binary reads the new session byte for byte: 2,568 B of text, and 16,638 B of JSON.
- Installed at 15:24.
- **At review, the kernel's lost update (section 6 of the report) was filed as theseus-id9 (P1)**, and put
  next in the chain, before the OTel step. Concurrency makes the race likelier, and task sessions and wakes
  will add writers.
- **F3's two decisions for Eddie, taken at review as engineering calls:**
  - one plan frame per group joins theseus-l6y (F4);
  - a tightening pressed mid-batch applies from the next response, which is the right grain for a
    response that was gated as a whole.

**Divergence from Parts I and II, and from the issue and the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The issue: calls that write a path another call touches run in call order | Every write and every program is a barrier | Ruthlessly simple (the brief); path-level analysis is described in the report, and not built | Keep |
| The issue: `fs.glob` uses the parallel walker too | `fs.glob` unchanged; `fs.grep`'s walk sequential, its search parallel | The walk must keep its order, and `fs.glob` is all walk | Keep; a sorted parallel walk would change today's output order |
| The brief: order results by their `ToolCall` nodes in the renderer | Nothing changed there | The renderer already placed each result after its call | Keep |
| Concurrent completions share an fsync through group commit | Not within a turn, whose frames are committed from its one task; across turns, half of them | Commits stay in the turn's task, because the kernel rewrites an execution's record from what it read | One plan frame per group (theseus-l6y); the kernel lock first (theseus-id9) |
| A call's `duration_ms` is its run (implicit) | Timed on its core since 59b3d9f | Timed from its future, a read in a group said 45–50 ms | Fixed |

**Known gaps.**
- A group of k calls writes 2k frames, one after another from the turn's task: about 100 ms for seven
  instant reads on this disk (theseus-l6y).
- A kernel transition read and rewritten on two threads can lose an update. That is old, but likelier
  during a batch's commit bursts (theseus-id9, next).
- `session.history` lists a group's results in the order they finished.

### Step K1. The kernel never loses an update to an execution (theseus-id9; 2026-09-29, 15:26–16:15; 920f732, 5e52a63)

**Why.** The kernel read an execution record, changed it, and wrote it back, with no lock, and the store
indexes a frame only after its fsync. So two writers of one execution on two threads could lose an update:
a turn's commit, read before a cancel's frame was indexed, put `running` back over `cancelled`. The bug was
as old as M2. F3's commit bursts made it likelier, and task sessions (DD7) and wakes (DD8) add writers. F3's
report found it, and it was filed at F3's review (P1) and put before the OTel step.

**What exists.**
- **One writer at a time per execution** (`theseus-kernel/src/locks.rs`).
  - The locks are the set of executions being written, under one mutex, with a condvar for the waiters.
  - An id is in the set exactly while it is held, so nothing needs pruning when an execution ends.
  - A map of mutexes was rejected: it loses the lock when an entry is pruned under a waiter.
  - Stripes were rejected: they make unrelated executions wait.
- **Every transition that reads and writes back holds the lock from its read until its frame is indexed**,
  the fsync included. An action's transition reads the action to find its execution, then locks that and
  reads the action again. `Kernel::view` shares the locks, so a turn's view and the kernel are one.
- **Decisions read outside the lock are read again inside it:**
  - the reconciler's due wake, from its scan;
  - `mark_unknown`'s "still dispatched";
  - startup's step 2 rewrites (`lock_all`, in id order).

  A reconcile no longer fails when an overdue action settles under it.
- **A lock taken twice on one thread panics**, instead of waiting on itself.
- **The ordered two-lock helper**, `lock_two`, for DD7's carved budgets. Nothing calls it yet.
- **Nothing else changed:** no record, frame, or row. A plain turn still writes 8 frames.

**How it is proven.**
- The gate passed at both commits: 300 tests, 9 of them new, with the lifecycle bench within budget.
  `crash-test`, 20 iterations × 3 restarts, lost zero committed records.
- **Deterministic races.** A test store stops one thread between a transition's read and its write while a
  second writer runs. Without the lock, a throwaway break, six races lose an update:
  - `running` over `cancelled`;
  - a dispatched call dropped;
  - `queued` over `cancelled`, twice;
  - `waiting` over `cancelled`;
  - a decline undone.

  All six pass with the lock, and writers of different executions never wait. `lock_two` in opposite orders
  never deadlocks. Without its id order, it deadlocks at the test's 20 s timeout.
- **`kernel-sim` with a second OS thread.** Raced turns (`--p-race`) put a cancel, completions arriving,
  wakes, input, and the heartbeat on the other thread, with crashes inside them.
  - A new invariant: a cancelled execution never runs again, read from the ledger in WAL order.
  - Five runs held: 38 seeds, 145 crashes, and 614 raced turns, with fsync on in one run and every turn
    raced in another.
  - Without the lock, every seed group failed within a few steps.
- **Live, over a copy of Eddie's store.** GLM turns of seven reads, four of them FIFOs that hold the batch
  open:
  - a cancel while the batch ran ended `cancelled`, stayed so across a restart, and left nothing
    dispatched;
  - a cancel released together with the FIFOs landed in the burst of completion frames. On the parent
    build it was lost, 2 of 2: the model answered a second loop, the execution ended `waiting`, and three
    reads that succeeded were recorded `cancelled`. On K1, 3 of 3 stayed cancelled, with nothing lost.

| Measure (release, medians of three interleaved rounds) | Parent | K1 |
|---|---|---|
| frames: a plain turn / two loops with one read / with five reads at once | 8 / 12 / 20 | 8 / 12 / 20 |
| a plain turn | 56.2 ms | 58.0 ms |
| two loops, one read / five reads at once | 86.1 / 148.8 ms | 89.4 / 145.8 ms |
| lifecycle p50: cold / from the copy / shutdown / kill | 17.4 / 17.1 / 35.3 / 34.1 ms | 16.9 / 17.0 / 37.0 / 34.4 ms |
| an uncontended lock and unlock; the extra action read | | 216 ns; 1.9 µs (about 3 µs a plain turn) |

Load was 3.6 to 5.5, with other sessions compiling. The differences go both ways, and each is inside the
spread of its own rounds.

**Reviewed** (Tabitha, 2026-09-29, 16:18 to 16:26).
- **The first gate rerun failed on one bench run.** Clean shutdown had p95 148.3 ms against 100 + 4, with
  p50 a normal 38.3 ms, while 1.3 GB of dirty pages from other sessions' builds were being written back.
  Four standalone bench runs right after passed at about 50 ms, and the gate passed after a `sync`: 300
  tests, with shutdown p95 50.9 ms. So the bench measured the machine's writeback, not K1. The fix, a
  `sync` before the bench and a second run before a miss fails the gate, goes into the OTel step.
- **Tabitha's own race, on the release build, over a fresh copy of Eddie's store.**
  - Seven reads, four held by FIFOs.
  - The FIFOs were released and `theseus executions cancel` sent at once. The cancel landed in the burst,
    after the first FIFO completion, and asked 3 calls to stop.
  - The turn's next loop was refused (`kernel`: not running, state cancelled).
  - The execution ended `cancelled` with nothing outstanding, and stayed so across a restart.
  - K1's WAL-order checker found no problems.
- Installed at 16:25.
- **Found by K1, and filed at review:** theseus-xeo. The core's `SessionRecord` has the same
  read-and-write-back shape: a `session.recompile` during a turn can be lost. It is folded into DD7, and
  DD7 also gives `lock_two` its first caller.

**Divergence from the issue and the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The issue: a lock held from the read to the WAL write, released before the fsync | Held through the fsync and the index update (the brief) | The index moves only after the fsync; a lock released before it lets the next writer read the old record | Keep; group commit across one execution's frames needs an index that moves before the fsync |
| The brief: a map of `Arc<Mutex<()>>` pruned when an execution ends, or striped locks | The set of executions being written, with a condvar | Nothing to prune, no pruning race, and no false sharing | Keep |
| The brief: a test hook between a transition's read and its write | A test store that stops one thread at one read | No hook in the product | Keep |
| `kernel-sim` reproducible from its seed (M2) | Only up to its first raced turn; `--p-race 0` is reproducible throughout | The OS picks the interleaving | Keep |
| The ordered two-lock helper | `lock_two`, and `lock_all` for startup's rewrites | Startup rewrites many executions in one frame | Keep |

**Known gaps.**
- A turn's frames, and one execution's frames in general, still take one fsync each. Group commit over a
  group's frames needs the index to move before the fsync (theseus-l6y).
- The core's `SessionRecord` read-and-write-back (theseus-xeo, with DD7).

### Step O1. OTel without the SDK: a native OTLP exporter in every build (theseus-hee, theseus-gi7; 2026-09-29, 16:27–17:20; 6ad89a2, e7d8495, b51252f, 5240563)

**Why.** Eddie, 2026-09-29, 08:52: "I'd like it built in by default, but 19 crates is surprising"; and at 09:21,
"let's do cheaper otel". The exporter was the `otel` cargo feature, off by default, over the OpenTelemetry SDK:
19 crates, among them prost and a second reqwest (0.13) with its own rustls stack and aws-lc.

**What exists.**
- **A native exporter** (`theseus-core/src/telemetry/`). It speaks OTLP/HTTP with the JSON encoding over the
  workspace's reqwest 0.12: lowerCamelCase fields, hex ids, integer enums, and 64-bit integers as decimal
  strings. It is in every build, and `theseusd`'s dependency tree is the default build's own set.
- **The same picture as the SDK's.** The trace walk, attributes, events, status, limits (128), and flags were
  ported from `otel.rs`. So were the eight instruments, with their units, descriptions, and attributes,
  aggregated in the process with cumulative temporality.
- **Off the hot path.**
  - A turn records its metrics and queues its trace. One sender task posts the trace, and the metrics every
    interval.
  - A failure, a 429, or a 5xx gets one retry after a second; then the batch is dropped and counted.
  - A full queue (64) drops its oldest.
- **Health** says `off`, `waiting` (for the vault or the headers secret), `exporting` (with what was sent
  and dropped, and the last error), or `failed` (the exporter could not be built). `theseus health` prints it.
- **Built after serving**, once the vault confirms the config and the headers secret resolves. A stopping
  daemon flushes, bounded at 1 s.
- **Removed:** the feature, its dependencies, the warning about a build without it, and the gate's second
  clippy run.
- **theseus-gi7.** `theseusd`'s printing subcommands end with success on a closed pipe. SIGPIPE stays
  ignored, so no pipe write can kill the daemon.
- **No leaked daemons.** Every `theseusd` that a real-binary test or the lifecycle bench spawns is held by a
  guard that kills and reaps it on drop.
- **The gate** runs `sync` before the lifecycle bench, and a second run before a miss fails it.

**How it is proven.** 322 tests in the gate, 22 of them new.
- **Conformance.** What is posted is read back through `opentelemetry-proto`'s own types: a dev-dependency,
  built without tonic. Every field comes back unchanged, and every encoding is checked.
- **The old picture.** A golden file dumped from the SDK exporter itself at 964411f matches span for span
  and metric for metric.
- **Failures:**
  - a 503 then a 200 delivers once;
  - a failing or absent receiver gets one retry, then the batch is dropped and counted in health;
  - a receiver that hangs costs a turn 63–73 µs.
- **Headers**, in both forms, with no value shown.
- **Throwaway probes** of each test failed as they should: a wrong field case, kind, integer encoding,
  temporality, or id encoding; SIGPIPE's default; a panic or a `bail!` before a stop. The temporality probe
  failed only once the tests wrote OTLP's enum values out, instead of reading the code's own constants.
- A 100 ms sleep on the start path still failed the gate, on both runs.
- **Live, on the release build** over a copy of Eddie's store, with his note and a local receiver:
  - a GLM tool turn's spans arrived with the trace's durations to the tenth of a millisecond, and the
    metrics after one interval;
  - with the receiver stopped, the next turn ran unchanged, and health counted 7 batches dropped, with the
    last error;
  - with the receiver back, the counters were still cumulative.

| Measure | Before | After |
|---|---|---|
| crates in `theseusd`'s build (`cargo tree -e normal`, name and version) | 292, or 311 with the exporter | 292 with the exporter |
| release `theseusd` | 19,702,816 B, or 23,942,128 B with the exporter | 19,830,208 B with the exporter |
| a clean shutdown with telemetry on, after a turn (release) | | 27.1 ms (24.9 ms off) |

**Reviewed** (Tabitha, 2026-09-29, 17:48 to 17:54).
- The gate rerun passed: 322 tests, and all four bench phases within budget. The gate has no
  `--features otel` run left.
- `theseusd`'s normal dependency tree was counted by name: 281 names, the same as before this step. None of
  them is opentelemetry, prost, or aws-lc.
- On the release build (19,830,208 B), over a fresh copy of Eddie's store, with `otlp_endpoint` pointed at a
  local receiver:
  - a GLM tool turn posted one trace (12,862 B of JSON, 7 spans), whose durations match `--trace` (turn
    11,560.8 ms, the tool 14.0 ms);
  - the metrics arrived every 5 s, cumulative;
  - health read `sent 2 (1 traces, 1 metrics) · dropped 0`.
- `theseusd example-config` and `example-bindings` into a closed pipe exit 0.
- Installed at 17:54.
- **Filed at review** as theseus-yf1, all small, with no consumer yet: the histogram bounds past 10 s; tool
  metrics by family, backend, and outcome, with a duration; the served model in `gen_ai.response.model`;
  failed continuations counted; and the provider-call histogram's empty attributes, which Tabitha found
  live and which the SDK exporter had too. All five were built in the telemetry lane (Item 26).

**Divergence from Parts I and II, and from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| OTLP over HTTP/protobuf (§3.20) | OTLP/HTTP with the JSON encoding | JSON needs no protobuf library, and the Collector's receiver takes it | §3.20 amended; a hand-written protobuf encoder is the fallback for a receiver that won't |
| The exporter is the `otel` build feature, off by default (§3.20, theseus-0g4) | In every build, with no feature and no added crate | Eddie, 2026-09-29 | §3.20 amended |
| §3.20's table: root attributes `theseus.turn_id`, `theseus.session_id`, `theseus.profile` | The root's attributes as recorded (`turn_id`, `session_id`, `profile`, …), as the SDK exporter emitted them | The same picture, so a dashboard built on it still works | Table corrected |
| §3.20's table: `theseus.tool.calls` by family, tool, backend, and outcome, and `theseus.tool.duration_ms` | By tool, with the turn's attributes; no duration | Neither exporter had them (A3 recorded it) | Table corrected; theseus-yf1, built in Item 26 |
| The SDK's batching (2,048 spans, a 5 s delay) and its blocking `force_flush` inside `shutdown` | One post per turn, a queue of 64, and a flush after the serving loop, bounded at 1 s | Simpler, and a stop never blocks a task | Keep |
| theseus-gi7: restore SIGPIPE's default for the printing subcommands, or end quietly | End quietly, with exit 0 | A default SIGPIPE would expose `check`'s write into `op inject`'s stdin | Keep |
| The brief: the lifecycle bench "already kills on its error paths" | Not on all of them: a failed stop, confirmation, copy wait, or health read after a start leaked | Found by reading, and shown by a probe | Fixed |

**Known gaps.**
- Vendors' acceptance of OTLP JSON (Honeycomb, Datadog, Tempo, ADOT) was checked only against a local
  receiver. Eddie's first real endpoint will tell.
- `opentelemetry-proto` 0.33 reads `asInt` only as a number, so a receiver built on that crate's serde would
  refuse our sums. Ours follow the protobuf JSON mapping.
- No gzip.

### Step J1. A job cannot answer approvals (theseus-6qy; 2026-09-29, 17:56–19:00; 4bd8bdc, d567d05, 0b23036)

**Why.** At L0 a `proc.run` job runs as the operator's own user. So it could reach the CLI socket and the
loopback web UI, and answer an approval its own session waited on, whenever `cli` or `web` was a trusted
channel, which is the default. This was found in the 2b part 1 review, and Eddie accepted the peer-pid check
on 2026-09-29 at 09:21.

**What exists.**
- **The wrapper** (`theseusd job-wrapper`) is a child subreaper.
  - It reaps every child that exits while its command runs, and reports as before.
  - Then it lingers, marked in `spool/lingering/<id>`, until no descendant remains. It kills nothing.
  - Health's `kernel.lingering_wrappers` counts the lingering wrappers, and `theseus health` and the
    Observatory show it.
- **A zombie is not alive.** `pid_alive` reads `/proc/<pid>/stat`. Before this, the daemon never reaped its
  wrappers (theseus-z4b), so a cancel took the wrapper it had just killed for alive, and settled
  `OutcomeUncertain` after 2.5 s.
- **Who is asking** (`peer.rs`). Each connection keeps its peer:
  - the socket reads `SO_PEERCRED` and the pid's start time at accept;
  - `--stdio` names its parent;
  - the web UI keeps the connection's two addresses, and finds the processes that hold the client's end
    (`/proc/net/tcp`, then `/proc/*/fd`) only when a judged act arrives;
  - Discord has no process.
- **The check**, in `Core::judge_act`. An answer (approve, decline, the spend reset) and an undo trace the
  asker's parent chain to pid 1. An asker under a live wrapper, known by its command line, is refused, and
  so is one that cannot be traced. This holds with or without `[approval]`. A press is not traced.
- **Loud.**
  - `approval.refused` carries `asker` (pid, `argv0`, job, wrapper pid, `trace_us`) and `from_job`.
  - A narrative line.
  - An `approval.refused` notification to every connection. The Discord binding posts it to the approval DM,
    or else the session's place, and `theseus watch` prints it.
  - The CLI exits 1.
  - An accepted answer records its asker too, and `theseus confirm` takes `--approve`.

**How it is proven.** 346 tests in the gate, 24 of them new:
- the real wrapper: a double fork stays under it, it reaps while the command runs, it lingers, and a cancel
  kills its group as before;
- the parent-chain walk and the loopback lookup, on real process trees with a stand-in wrapper (`flock`);
- the acts in the core: approve, decline, and the web UI from a job's process are refused; the spend reset is
  refused; the undo is refused; a press is accepted;
- end to end, with the real daemon, wrappers, and CLI, and a stand-in Messages API:
  - a job's `theseus confirm --approve` for its own session's call is refused;
  - so is its double-forked grandchild, before and after the job's main process exits;
  - a job's WebSocket answer through the web UI is refused;
  - the operator's answers count;
  - a cancel still kills what it killed.

Throwaway probes: without the subreaper, the grandchild approved the call after the job's main process
exited. Without the trace, all six act tests failed.

| Measure (release) | p50 | p95 |
|---|---|---|
| each socket connection at accept | 3.9 µs | 4.6 µs |
| a judged act's trace: the operator's CLI / a job's process | 62.5 / 21.6 µs | 105.4 / 24.2 µs |
| a judged web act's owner lookup (120 processes, 1,153 fds) | 3.08 ms | 3.58 ms |

**Reviewed** (Tabitha, 2026-09-29, 19:18 to 19:29).
- The gate rerun passed: 346 tests, and all four bench phases within budget.
- On the release build, over a fresh copy of Eddie's store, with `[approval] channels = ["cli"]`:
  - `theseus policy tighten fs.write` made a GLM turn's write wait.
  - A second GLM session was asked to approve it with `proc.run`. It declined twice on its own judgment, and
    once investigated the binary's strings. When it ran `theseus confirm --approve`, the daemon refused it:
    "from a Theseus job's process (job act_…, pid 3452497, theseus)", exit 1, a 46 µs trace, and
    `approval.refused` with `from_job` in the ledger.
  - Tabitha's own `theseus confirm --approve` from her shell then counted, with a 126 µs trace, and the file
    was written.
- **Eddie restarted his daemon at 18:50** (pid 3431665), on the O1 build. Every check kept to its own socket
  and state dir, and never touched his.
- Installed at 19:28.
- **Taken at review:**
  - The one hole left before M4 (a job that kills its own wrapper, whose orphans go to init) is closed next,
    with theseus-z4b's reaping: the daemon becomes a child subreaper, and refuses answers from its own
    descendants (step Z1).
  - Refusing another local account's web client, and counting any Theseus daemon's job, are right for a
    one-operator machine.
  - A web banner for a refusal isn't needed now: the Observatory's ledger and the narrative show it, and the
    DM carries it.

**Divergence from Parts I and II, and from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The brief: refuse when the asker or an ancestor is a live job wrapper | Any Theseus daemon's wrapper, known by its command line | The command line names the job, needs no spool state, and holds for as long as the wrapper lives | Keep |
| §3.9: the web UI's member is anyone with an account on the machine | Another account's client cannot be traced, so it is refused | The brief: a pid that cannot be read counts as a job's | §3.9 amended |
| The brief: after the command exits, the wrapper keeps reaping | It also reaps while the command runs | Orphans reparented to a subreaper would otherwise wait as zombies for the whole job | Keep |
| — | `pid_alive` treats a zombie as gone | A cancel of a real job settled `OutcomeUncertain` after 2.5 s | Fixed; the zombie leak is theseus-z4b (Z1) |
| The brief: `theseus confirm --approve <id>` | The flag added: the default, and it conflicts with `--decline` | The CLI had only `--decline` | Keep |
| The brief: a notice where approvals go (the DM) | The DM when one is trusted, else the session's place, else the log; `theseus watch` too | The web UI has no banner; its Observatory shows the row, highlighted | Keep |

**Known gaps.**
- A user systemd unit, a tmux server already running, cron, and a process started outside the job are M4's.
- A job that kills its own wrapper is closed next (Z1, theseus-z4b).
- A Windows browser, through a Hyper-V firewall rule for 7433 that does not exist today, would be refused as
  untraceable.
- The web lookup's `/proc/net/tcp` read takes about 1 ms; `sock_diag` would make it tens of µs.

### Step Z1. The daemon reaps its job wrappers, and adopts a job's orphans (theseus-z4b; 2026-09-29, 20:30–21:28; d0212c0, 0e15190)

**Why.**
- The daemon spawned each job's wrapper and forgot it. Every wrapper that exited stayed a zombie child of
  `theseusd` until the daemon exited, and an exec restart (F1b) kept them. Eddie's daemon runs about 160 jobs
  a day. J1 found this.
- A job could kill its own wrapper (`kill -9 $PPID`). Its processes then went to init, out of the
  parent-chain walk's reach, and an orphan's `theseus confirm` counted. This was J1's one hole before M4.
- The first run was cancelled at 19:32, two minutes in and before any change, so that Eddie could restart
  the OpenClaw gateway. It was relaunched at 20:30.

**What exists.**
- **Who reaps what** (`theseus-kernel/src/children.rs`): a registry of the daemon's children, by pid.
  - A job wrapper is registered with its job as it is spawned, and is reaped by its own pid once it exits.
  - An `op` process is registered as tokio's, with its start time, and is never reaped here, since tokio
    waits for each of its children by pid.
  - Any other child is an orphan the daemon adopted, and is reaped once it exits.
  - A spawn holds the registry's lock through its registration, and a sweep holds it through its scan and
    its reaps. So a sweep never takes a new `op` for an orphan. Nothing waits for "any child".
- **The reaper** is a task woken by SIGCHLD, and every 10 s, that sweeps on the blocking pool. It runs in the
  socket daemon and in `--stdio` alike.
- **The daemon adopts a job's orphans.** The socket daemon is a child subreaper, set in `main` before the
  runtime starts.
- **An orphan can't answer.** The walk also stops at the daemon.
  - An asker under `theseusd` with no live wrapper between is refused: `from a process under theseusd itself
    (pid <n>, <argv0>), which is a job's orphan`, with `under_daemon` in its `asker`. The Discord text says so.
  - The web UI's owner lookup still skips the daemon's own fds.
- **After an exec restart,** the new image sets the flag again and learns its children, recognizing a wrapper
  by its command line.
- **A reaped wrapper's pid may be reused.** So a wrapper is alive only while its pid's command line names its
  job (the reconciler, the restart check), and a cancel leaves alone a pid that is now another process.
- **Health's `children`**: wrappers running and lingering, orphans adopted, zombies (0 in steady state), the
  `op` processes, and what was reaped. A `theseus health` line and an Observatory line show it.

**How it is proven.** 356 tests in the gate, 10 of them new:
- the registry, in a test binary of its own: a wrapper and an orphan are reaped, an owned child is left for
  its owner (whose `wait()` still gets its status), and the children are learned again after an exec;
- the orphan rule on real process trees, on the socket and through the web UI;
- a cancel leaves alone a process that took a reaped wrapper's pid;
- end to end, with the real daemon:
  - 200 jobs leave no zombie;
  - an `op` run through a burst of 50 jobs keeps its exit status;
  - a job that kills its wrapper leaves a grandchild that the daemon adopts. Its answer is refused, and the
    operator's counts;
  - after an exec restart, the daemon is a subreaper again, and reaps what the old image left.

Throwaway runs:
- the parent's daemon left 200 zombies after 200 jobs;
- without the subreaper, the orphan approved the call;
- with `op` unregistered, the sweep took tokio's children, and every `op read` failed with `ECHILD`.

Live, on a copy of Eddie's store:
- 20 GLM turns with a job each left no zombie.
- A GLM job ran a script that killed its own wrapper. The orphan it left went to the daemon, and its `theseus
  confirm --approve` was refused with the new reason, after an 82 µs trace. The operator's own answer then
  counted.

| Measure (release) | p50 | p95 |
|---|---|---|
| a sweep: 2 wrappers / 100 children, 25 threads | 47.6 / 264 µs | 68.8 / 404 µs |
| a census (health): 2 wrappers / 100 children | 54.9 / 462 µs | 77.2 / 682 µs |

The lifecycle bench is unchanged within its noise: cold start p50 18.9 ms, against the parent's 18.7.

**Reviewed** (Tabitha, 2026-09-29, 21:48 to 21:55).
- The gate rerun passed: 356 tests, and all four bench phases within budget.
- On the release build, over a fresh copy of Eddie's store:
  - five GLM turns each ran a `proc.run` job, and each answered with its own echo;
  - the daemon then had 0 zombie children by a `/proc` scan;
  - health said `subreaper: true`, `reaped_wrappers: 5`, `zombies: 0`.
- Installed at 21:55.
- **Taken at review:**
  - Z1's two follow-ups are theseus-6uo, folded into the secret broker's step:
    - refuse the descendants of any serving `theseusd`, not only this daemon's, and make `--stdio` a
      subreaper;
    - ledger a job's killed wrapper as `job.wrapper_lost`, and mark its action unknown at once.
  - A job's orphans keep running until they exit, as a lingering wrapper's descendants do.

**Divergence from Parts I and II, and from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The brief: reap from the heartbeat, or from a task woken on SIGCHLD | A SIGCHLD task, with a 10 s tick | The heartbeat is 60 s, starts only once the config gate opens, and a wrapper pokes it before it exits | Keep |
| The brief: a registry at the one spawn site (`OpReader`) | One registry of every child: wrappers with their job, `op` as tokio's with its start time | The sweep reaps wrappers by pid, and must tell tokio's children from orphans; the start time guards a reused pid | Keep |
| — | `wrapper_alive(pid, job)`, and `terminate` takes the job | Once reaped, a wrapper's pid can belong to another process | Keep |
| — | `--stdio` reaps its own wrappers too, and adopts nothing | The brief sets the flag for the socket daemon only; reaping by pid is safe in every mode | theseus-6uo makes `--stdio` a subreaper |

**Known gaps.**
- The rule covers this daemon's own descendants. An orphan of another Theseus daemon (a scratch daemon's,
  or a `--stdio` daemon's) that answers this one is not caught. That is theseus-6uo.
- A job's orphans run under the daemon until they exit, and nothing kills them. A cancel still reaches the
  ones that stayed in the job's process group.
- A wrapper killed before it reported leaves its action `dispatched` until its deadline, when the reconciler
  marks it unknown. theseus-6uo marks it at once.

### Step B1. The secret broker, and any daemon's descendants refused (theseus-dcy, theseus-6uo; 2026-09-30, 00:12–00:57; 096aa10, 5857e21)

**Why.**
- `op` ran 125 times in the audit's 30 days (55 in the DM), and the floor makes each one wait for approval.
  Eddie accepted the broker on 2026-09-29 at 09:39: "we don't want to continuously query op anyway, so this
  might get broader use."
- Z1's two follow-ups (theseus-6uo) were folded in, since both touch job spawning.
- The first run started at 23:27 and was ended at 23:36 by a gateway stop, while it was still reading. It
  wrote nothing. This entry is the second run.

**What exists.**
- **Config** (`theseus-core/src/broker.rs`, `config.rs`, the template).
  - `[broker.programs.<program>] env = { VAR = "<secret>" }`, and `[broker.secrets.<name>] posture`,
    `notify` when absent.
  - Validation names each problem: a program name with a `/`, a bad variable name, a secret that is not a
    `[secrets]` entry, an empty `env`, and an unknown key or posture.
  - An empty broker is skipped, so a note without `[broker]` loads and prints as it did.
- **Direct argv** (`Broker::program_for`), and the gate and the spawn: §3.19 and §3.9.
- **Surfaces.** One `decision.granted` on the gate record reaches every surface:
  - Discord's tool line (`🔑 gh got GH_TOKEN`), the notice card's Secrets field, and `policy.notified`;
  - the web UI's pill, the Observatory's row, the CLI's notice line, and the confirm's reason.
  - `tool.started` says what the job actually got, so a withheld secret replaces the gate's word.
- **Never written.** The wrapper's arguments carry no value: the environment goes by `env_clear` and
  `envs`. `WrapperArgs`' Debug prints names only, and the spawner's copies are zeroized after the launch.
- **Toollets.** `ToolCtx::secret(name)` over a broker bound to the call (`Bound`), granted by wiring
  (`grant_tool`). The M4 hook is a one-line comment above `secret_for_tool`.
- **Health and the Observatory.** `health.broker[]` lists the kind, the target, the variable, the secret,
  the posture, and the uses since start. `theseus health` prints `broker: gh gets GH_TOKEN (github_token,
  notify), used 1 time`.
- **theseus-6uo** (`peer.rs`, `job.rs`, `rpc/driver.rs`, the reaper): §3.9's "Any serving daemon's
  descendants", and §3.16's two bullets. A drift test keeps the core's list of the daemon's value-taking
  options equal to the CLI's, so `--config check` is not read as a subcommand.
- **Test support.** The in-process wrapper now gives its command the job's environment, as a wrapper
  process has it. Before, a core test's job saw the whole test process's environment.

**How it is proven.** 382 tests in the gate, 14 of them new or extended:
- the broker's unit tests: direct argv against `sh -c`, `env`, `bash -lc`, `./gh`, and a call-set PATH;
  the bounded wait; the withheld secret; the posture at the gate and at the spawn; and a toollet's grant;
- `a_program_run_by_its_own_argv_gets_its_secret_and_nothing_else_does`, against the real daemon with stub
  programs:
  - the job's environment holds exactly one vault value, and no `AWS_` name;
  - an `approve` secret waits;
  - a failed secret is withheld;
  - no file under the state dir, and not the log, holds the value;
- `an_orphan_of_another_daemon_cannot_answer_this_one`: with the check disabled, a probe shows the answer
  used to count;
- `a_job_that_kills_its_wrapper_…`: one `job.wrapper_lost`, and the action unknown at once. A cancel's
  kill gives none.

**Reviewed** (Tabitha, 2026-09-30, 01:01 to 01:07).
- The gate rerun passed: 382 tests, and all four bench phases within budget (cold start p50 36.3 ms).
- On the release build, over a fresh copy of Eddie's store, with his note and the grant, and a shim `op`
  that logs each run:
  - a GLM turn's `["gh", "api", "user", "--jq", ".login"]` answered `zeroaltitude`, notified with
    `🔑 gh got GH_TOKEN`, and never waited;
  - **the token came from the broker.** With gh's own stored login hidden (`GH_CONFIG_DIR` set to an
    empty directory), direct `gh` still answered `zeroaltitude`. `sh -c "gh …"` got the broker's note and
    gh's exit 4, "To authenticate, please run `gh auth login`";
  - `op` ran once in all, at startup;
  - the ledger held 2 `secret.granted` rows and no confirm; health said `used 2 times`;
  - the token's value was in none of the 6 files under the copy's state (the WAL segment, `index.redb`,
    the manifest, and the three jobs' spool outputs), and in neither the log nor the CLI's output. The
    control matched.
- Eddie's unchanged note loads under the new binary.
- Installed at 01:06.
- **Taken at review:**
  - The raw spool output keeps what a program prints (`gh auth token`). Filed as a follow-up: the wrapper
    redacts its own granted values from its output file.
  - DD5 adds one comment under the template's `[broker]`: a granted program passes its variable to what it
    runs (gh's extensions and shell aliases, or a hook), so the posture is the control.
  - Put to Eddie: gh's stored login authenticates any job's gh, broker or not, and whether
    `github_token` should stay `notify`.

**Divergence from Parts I and II, and from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| `broker.secret_for_tool("web.search", name)` | That call, and `ToolCtx::secret(name)` over a `Bound` broker for the call | A toollet runs on a core and cannot wait, and needs the call's posture; the wiring grants the tool (`grant_tool`) | Keep |
| Toollet grants in the config | Granted by wiring code, from the consumer's own `…_secret` key (DD5) | Config keys are not defined before code honors them, and no toollet needs one yet | DD5 adds the key |
| "The tool line and the notice say gh got GH_TOKEN" | So do the web UI's pill, the CLI's notice, the Observatory's row, and the confirm's reason | One `decision.granted` on the gate record reaches every surface | Keep |
| — | A spawn withholds a secret whose posture is stricter than the call ran at | The program's resolution could change between the gate and the spawn | Keep |
| — | `secret.withheld` rows | A withheld grant is as auditable as a given one | Keep |
| — | The in-process wrapper applies the job's environment | Core tests saw the test process's whole environment | Keep |

**Known gaps.**
- A granted program that runs other programs passes the variable on: gh's extensions and shell aliases
  (`gh alias set --shell`), or git's hooks, if git were ever granted.
- The spool's raw output file keeps whatever a program prints. The node, the model, and every surface get
  the scrubbed text. The follow-up above closes it.
- At L0 a job of the same user can read another job's `/proc/<pid>/environ`, and gh's own stored login
  (`~/.config/gh/hosts.yml`) authenticates any job's gh, through a shell too. L1 (M4) closes both.

### Step F4a. Versioned readers, a newer store refused, and a tail-only open (theseus-qa0, theseus-8ni; 2026-09-30, 13:03–13:48; 1da73ee, 6d3df58)

**Why.** M3.5's exit test says a store in the previous format serves at once under the new binary, and
nothing stopped the other direction. DD8 found that an older binary reads pending wakes and drops them
at its next write. T1's hold on external text would be dropped the same way, which lifts a safety rule.
`Wal::open` also read and checked every segment on every start, and the store's open then read the WAL
a second time to replay its tail (theseus-8ni).

**What exists** (§6's storage kernel; §9's migration row; P5b's standing rule).
- **Per-kind schemas** (`kinds::SCHEMAS`). Six kinds are at 2 for the fields they gained since M2:
  session, execution, action, completion, node, and compilation. Every writer takes a record's schema
  from the table through `NewRecord`.
- **A format-3 manifest** names each kind's newest schema. It is marked lazily and durably, before the
  first newer record: a start writes nothing, so an older binary still opens a store the newer one has
  only read.
- **The refusal** comes before the WAL or the index opens, so nothing is written. It names the kind and
  both schemas, and says to install the newer theseusd. Builds before F4a refuse format 3 with their own
  message.
- **The fixture**: a store written by 460a35b (155 KB), checked in with a README, served at once and read
  whole.
- **The tail-only open.** The index opens first. The WAL is checked from the frame after the index's
  checkpoint, and falls back to the full check when the log is not what the index says. Every record
  read checks the position it was asked for.
- **The history check after serving** (`store.verify`, about 5 % of a core). A corrupt frame is loud: an
  ERROR line, a `store.corrupt` row, a `store: CORRUPT` line in `theseus health`, and its reads refused.
- **A checkpoint excludes appends** (the `appending` lock). This closes a latent race in which a
  checkpoint could claim a frame that was written but not yet synced or indexed.
- **The reaper's `CORE` is a `Weak`.** Since B1, a static that owned the core kept redb open past every
  exit, so every start repaired the index. Health says when an open repaired it (`index_repaired`).

**How it is proven.**
- The gate at 6d3df58 ran 519 tests, 11 of them new:
  - 5 in the store: the refusal with nothing written, the lazy mark, the tail-only open over an old
    corrupt and an unreadable segment, the fallback, and the history check;
  - 1 in the core: the fixture read whole;
  - 4 against the real daemon: the fixture served, the refusal, a corrupt history, and a clean stop
    repairing nothing;
  - 1 in the CLI.
- Numbers (release, p50):
  - the store phase at 10,000 sessions went 63.2 → 10.1 ms, and on Eddie's copy 21.4 → 7.2 ms;
  - M3.5's exit test on 10,000 parked sessions meets every §9 phase: cold start 121.7 ms of 250,
    from the copy 112.7, clean shutdown 24.8 of 100, and SIGKILL then restart 138.6 of 350.
  - A clean stop now pays redb's close: 23.3 → 36.9 ms on the empty store, within its 100 ms.
- The step's live check, on a copy of Eddie's store:
  - the sessions read identically under T1 and the new build;
  - `session.open` marked the store format 3;
  - T1 then refused it, and left every file byte-identical;
  - the new build served it again.

**Reviewed** (Tabitha, 2026-09-30, 14:00 to 14:12).
- The gate rerun passed on the first try: 519 tests, cold start p95 33.3 ms (T1's was 41.2), and a clean
  shutdown p50 of 35.7 ms.
- **Reading the code.**
  - Appends hold the `appending` lock shared, and the checkpoint runs after the lock is released, so it
    never waits on its own thread.
  - Group commit waits only for a sync that is already in flight, never for new arrivals, so a queued
    checkpoint cannot deadlock the appenders.
  - The manifest is rewritten under the marks' write lock, rechecked there, so two appenders mark it once.
  - `tail_after` requires the checkpoint's record to decode at its indexed location with its position. It
    walks every later segment, cuts a torn frame only in the last segment, and falls back otherwise.
- **Crash tests on the release build.**
  - `theseus-sim crash-test`, whose worker checkpoints every 200 records, ran 40 iterations × 3 restarts,
    then 25 × 8, with stores up to 487 records, so most restarts took the tail-only path. The tail-only
    open cut 24,555 bytes of half-written frames, and lost zero committed records.
  - With `--tear true` (bytes truncated or flipped after each kill), T1's build and F4a's both passed the
    same seed, 25 × 4, with zero committed records lost.
- **The kernel simulator**, 30 seeds × 1,500 steps with half the turns raced by a second thread (`--p-race
  0.5`): 129 injected crashes, 540 raced turns, and no deadlock or broken invariant.
- **A live check on the release build of 6d3df58**, over a fresh copy of Eddie's store with his note:
  - the first start served in 27.7 ms: no repair, 20 records replayed, and the 1 MB history checked
    after serving in 0.7 ms;
  - a GLM turn ran `sleep 25` through `proc.run`, and the daemon was SIGKILLed while the job ran. The
    wrapper and the `sleep` survived. The restart served in 43 ms, repaired the index (expected after a
    kill), and replayed only the 47 records past the checkpoint. The job's result arrived, first as the
    restart's placeholder and then as `exit 0`;
  - after a clean stop, the next start served in 17.0 ms: store phase 7.1 ms, no repair, nothing
    replayed, and the manifest at format 3.
- Eddie's unchanged note loads under the new binary.
- **Installed at 14:09**, after a snapshot of Eddie's store (format 2) at
  `~/reports/theseus-f4a/eddie-store-pre-f4a/`. His store becomes format 3 at its first new session
  record, and T1 or older cannot open it after that, so a rollback is `theseusd restore --from` that copy.
- **Taken at review:**
  - **A continuation runs on the live profile** (theseus-kol, P2, pre-existing: T1's build does the
    same). The GLM turn's continuation after the restart ran on `default` (claude-sonnet-5-5).
    Anthropic refused GLM's replayed thinking block ("Invalid `signature` in `thinking` block"), and the
    driver's retry ended `nothing_new`, so the job's result was never answered. Three defects: the
    continuation's profile, a thinking block replayed across providers, and a failed continuation that
    consumes its input. It goes into fix batch 1.
  - `theseusd`'s release builds differ in about 40 bytes between two builds of the same commit: an
    embedded file time (13:19 against 13:41) and the build id. They are functionally identical.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Schema versions stamped on every segment (§6) | On every record, by kind; the manifest keeps the newest per kind | The kind is the unit that gains fields, and a segment mixes kinds | §6 amended |
| A tender rewrites an old layout in the background (§9, P5b) | Old layouts are read in place; nothing needed a rewrite | serde's defaults read every schema-1 record | Keep; the first layout that needs one lands with its tender |
| Recovery verifies every frame (§6) | The open checks from the checkpoint's frame on; the history is checked after serving | Store open grows with the tail, never with history (§9) | §6 amended |
| The full check in `theseusd check` or a tender (brief) | A tender, `store.verify`, once per start at about 5 % of a core | It runs itself, and finds a bad segment the day it goes bad | Keep |
| An older binary is told what to do (brief) | Builds from F4a on name the kind and both schemas; older ones refuse format 3 with their own message | They cannot learn a new message | Keep |
| — | Every start repaired redb's index since B1 (the static `CORE`); fixed | A clean close is what the start budget assumes | Keep |

**Known gaps.**
- ~~theseus-lv2 (P2): startup, health, the reconcile, and the driver's tick read every execution, action,
  and session. A projection by state in the index would make them O(open). At 10,000 sessions this is
  most of what is left of a cold start.~~ Built in Item 46.
- ~~theseus-0dq (P3): the history check re-reads the whole WAL at every start. It could keep a
  verified-to mark, with a slower full re-check for bit rot.~~ Built in Item 46.
- ~~theseus-15g (P3): a corrupt frame's refused reads fail whole list reads, such as a `ledger.tail` that
  reaches them.~~ Built in Item 53.
- theseus-q49 (P3): records carry two schema numbers, the header's per kind and some payloads' own.
- ~~theseus-02k (P3): a clean stop pays redb's close after its own durable checkpoint.~~ Built in Item 46.
- A restore by a build older than the WAL it restores cannot be stopped by this one. Restore with the
  newer build.

### Step F4b. The swap and restore phases, the store-lock race, and a plain turn in 5 frames (theseus-qa0, theseus-l6y; 2026-09-30, 14:12–14:26 and 14:31–15:31; de880fc, 27e1237)

**Why.** P5b's bench had three of its five phases; a binary swap and a restore were F4's. F1b found that
`theseus shutdown` answers before the daemon has closed its store, so a start at once failed with "Database
already open". F2 left four frame merges (theseus-l6y), with a plain turn at 8 frames.

**What exists.**
- **The store's open waits for another process's lock**, at most 3 s (`LOCK_WAIT`). It tries again every
  0.5 ms and reads the manifest again on each try. Health's store phase reports `lock_wait_ms`. The wait is
  in the new process, not in the old one's ordering, because the CLI returns before the old process removes
  its socket or closes its store.
- **The bench's `swap` phase**: the stop's answer, then the other build at once on the same store, timed to
  that process's first answer (the new process is known by `SO_PEERCRED`). The job's wrapper must run
  through every swap, and be ended at last by a daemon that never started it. Budget 200 ms, margin 2 ms.
- **The `restore` phase**: `theseusd restore` from a copy of the WAL, with its pages dropped first, beside a
  cold read of the same bytes. It is measured, and the restored store must serve.
- **A plain turn writes 5 frames**:
  - an input's wake and admission are one frame (`Kernel::admit_input`);
  - a result the turn reads itself gets no queue entry (`Kernel::turn_of`), and a turn that faults after
    settling one is woken explicitly (`execution.queued`, why `fault`);
  - a confirmed call is authorized and dispatched in one frame (`Kernel::authorize_and_dispatch`);
  - the session write rides in `end_turn`'s frame (`Store::defer_session`, under a `SessionHold`).

**How it is proven.**
- 523 tests in the gate.
- **The lock:**
  - a daemon test of five stops, each followed at once by a start (a probe with no wait failed it);
  - on release builds, 20 of 20 starts at once failed with the installed build, and 0 of 20 with this one
    (each waited about 15 ms).
- **The frames:**
  - F2's output-diff probe (623 → 495 frames): after normalizing what dropping the queue entry removes,
    only the provider-failure scenario's explicit wake differs;
  - a fault-injection test that faults the frame planning a tool call, and resumes the call after a restart;
  - kernel-sim with new invariants: a turn's own result is never queued, and a two-in-one action is never
    found authorized-only;
  - the crash test.
- **The step's live check**, on a copy of Eddie's store with his note:
  - a swap from the installed build to this one, mid-way through a GLM turn's `sleep 20` job, took 56.9 ms to
    the new build's first answer, 14.5 ms of it waiting for the lock;
  - the wrapper survived, and the new daemon accepted the job's result, which the next turn read;
  - a plain GLM turn wrote 5 frames;
  - a restore of the copy's WAL served all 7 sessions identically.

Release, p50 / p95 (ms):

| Store | Cold | Vault | Shutdown | Kill | Swap | Restore |
|---|---|---|---|---|---|---|
| Eddie's copy (1.24 MB) | 19.6 / 22.1 | 19.3 / 23.9 | 31.7 / 37.1 | 35.0 / 60.6 | 48.2 / 49.8 | 82.5 / 105.0 |
| 10,000 sessions (36.5 MB) | 113.3 / 119.4 | 120.1 / 133.1 | 26.2 / 44.2 | 143.0 / 176.0 | 138.7 / 150.7 | 342.2 / 386.2 |

**Reviewed** (Tabitha, 2026-09-30, 16:00 to 16:06).
- **The gate rerun passed on the first try**: 523 tests, and all six phases within budget. Swap p95 was
  76.2 ms of 202, and cold p95 31.1.
- **Reading the code.** `defer_session`'s lock span:
  - `finish` is `run_inner`'s last expression, and nothing between it and the hold's drop in `run` awaits
    or takes a session lock again;
  - `end_turn`'s closure only reads and stages outbox records;
  - the order stays session, then execution;
  - the hold releases its lock on drop, and flushes what waits first.
- **A live check on the release build of 27e1237, over a fresh copy of Eddie's store with his note**
  (Discord and the web UI off, and glm live, for theseus-kol). It went through the upgrade path, with T1:
  - on the installed F4a build, a GLM turn fetched a page. Its `proc.run echo f4b-review` waited, because the
    hold raised it to approve;
  - the daemon was **SIGKILLed while the call waited**. This build then started on the same store: 37.8 ms to
    serving, with the index repaired, as expected after a kill. The pending approval and the hold both
    survived;
  - an approval from the CLI: the call's authorization and dispatch rode in **one frame** (`turn.started +
    action.authorized + action.dispatched`). It ran (`f4b-review`, exit 0), and GLM finished the turn. The
    session write rode in the turn's last frame;
  - **`theseus shutdown`, then an immediate start, 5 times: 5 of 5 served**, each waiting 6.6 to 14.1 ms for
    the lock, with no repair.
- Eddie's unchanged note loads under the new binary.
- **Installed at 16:05** from 27e1237. The format is unchanged since F4a (format 3), and his store, still
  format 2 as of this review, is snapshotted at `~/reports/theseus-f4a/eddie-store-pre-f4a/`.
- **M3.5 (theseus-qa0) closed.** Every §9 lifecycle budget is met at p95, on both stores, and the gate fails
  a commit that misses one. A store in the previous format serves at once (F4a). What stays open is filed:
  theseus-lv2, ur0, ez3, byu, and ef0.
- **Taken at review:**
  - A continuation that a confirm answer starts still writes its queue and admission as two frames
    (`execution.queued`, then `execution.running`). Only an input's are merged. It is small, and folded into
    theseus-ef0.
  - The job's result in the live check was accepted by the driver's drain, not the turn's own wait, so it was
    queued and consumed (`execution.results_consumed`). That is by design.

**M3.5's exit.** Every §9 lifecycle budget is met at p95 on both stores, the gate fails a commit that misses
one, and a store in the previous format serves at once (F4a). Restore is measured, as §9 says "to measure".

**Divergence from Parts I and II, and from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The lock race: wait, bounded "well inside the budget", or release the lock before the socket (brief) | The new start waits, bounded at 3 s | The CLI returns before either. A bound under 200 ms would fail a stop whose telemetry flush takes its whole second | Keep; the bench holds the swap itself to 200 ms |
| Restore at the disk's sequential read speed (§9) | 39 to 64 times a cold read (and the read is WSL's host cache) | A restore copies, then checks every frame and indexes every record | theseus-byu |
| F2's option (b): wake when a turn faults "with a call unanswered" (theseus-l6y) | Wake when it faults after settling a result it reads itself | The queue entry also requeued a turn whose calls were all answered, and a failed provider call's retry; the narrower test would change both | Keep |
| theseus-l6y's item 5, `confirm.list`'s read | Not built | It changes the action record's layout (a schema bump) | theseus-ef0 |
| The session record is synced before the call that wrote it returns (§4.6) | A turn's end-of-turn session write rides in `end_turn`'s frame, under its lock | One frame per turn | §4.6 amended |

**Known gaps.**
- A stop's answer can be lost, so `theseus shutdown` fails though the daemon stopped (theseus-ur0).
- A restore does not sync its copies (theseus-ez3).
- Restore's budget (theseus-byu). _(Item 46 built the index in bulk and proposed a budget, which waits on Eddie's call: theseus-fsug.)_
- `confirm.list`'s read, and a confirm continuation's two frames (theseus-ef0).
- ~~O(open) reads (theseus-lv2, F4a's), which are most of the 10,000-session swap's 139 ms.~~ Built in Item 46.

