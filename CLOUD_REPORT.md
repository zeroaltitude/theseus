# CLOUD_REPORT: p2-turn-budget (theseus-2uy56, theseus-yct32)

Branch `cloud/20261023-p2-turn-budget`, base `origin/main` at `23328545`.

## Design

### What a turn knows today (read before designing)

- **Time.** A turn knows when it started (`Turn::started`) and nothing of a limit. `[kernel] default_deadline_secs`
  (600) is an *action's* deadline (a tool call's, `Kernel::plan`), not a turn's; `--one-shot SECS` is how long a
  spawned run's client follows *after* its turn. A benchmark's 15- or 40-minute limit is enforced from outside (the
  driver stops the turn) and never reaches the daemon. So "the turn's deadline" has no source today: one has to be
  given.
- **Spend.** The turn's own cost (`Turn::cost`) and its execution's budget (`Budget::limit_micros`, the session's limit
  or a task's carve) are both at hand at a loop boundary; under `spend_limit_mode = "notify"` (the default) reaching
  the limit only tells the owner (`budget_step.rs`), by the owner's rule "notify, not restrict".
- **Checks.** Each tool result's status (`ResultStatus::is_error`) is folded into `Checkpoints::failed` for the loop
  (turn.rs `run_tools`), which the checkpoint's `Tally` takes at the loop's end. Which tools a loop called is in its
  `uses`. Commands are `proc.run`; a backgrounded one has no status yet (`Turn::background` grows).
- **Notes to the model.** The harness speaks to the model in its own voice with a harness node appended to the
  session (`Node::relayed`, `Origin::Harness`): the end note and the stop nudge do it. Appended after the tool
  results, such a node is after every cache breakpoint of the request and is repeated byte for byte by every later
  request, so it never rewrites the cached prefix. The system's three layers and the session block are the prefix.
- **CPU.** `cpu.rs` sizes the daemon's in-process pool from `available_parallelism`. Commands run as the daemon's
  job children (`theseusd job-wrapper`, or L1's namespaces, which have no cgroup of their own), so the cgroup that
  limits a command is the daemon's own (`/proc/self/cgroup`'s v2 line). `nproc` in a container counts the host's
  CPUs; std's `available_parallelism` already honours the quota and affinity, but the model never sees it.

### theseus-2uy56: stop paying on work that is not finishing

Options:

1. **A harness progress check with notes (picked).** A pure function at each loop's end over what the turn already
   knows: the share of its deadline used, the share of its spend limit used by this turn, loops since a loop whose
   commands all passed, and failing loops since then. It decides nothing, a steer note (once), or a stop-and-report
   note after which the next loop is the turn's last.
   - Cost: no model call. One `if` and a map lookup per loop for a turn that has no deadline and spends little.
     A steer is one harness note (~60 tokens) once a turn; a stop one more (~70 tokens) and one loop.
   - Cache: the notes are appended history, after the prefix; nothing in the system or tools changes.
   - Fails safe: a turn whose recent loop passed a check is never steered or stopped; spend only steers, never stops
     (the owner's notify rule stands); a stop still lets the model write its report in one more call; a lost or
     unreadable input (no deadline, no limit, a price unknown) is no signal, so nothing fires.
2. **Ask Jev at intervals ("is this converging?")**, a judge pack like the route checkpoint's. Better judgment of
   "progress" than a counter, but a model call (and its latency, budget and pack) every few loops on the hot path, a
   new pack to design and calibrate in shadow first (§2, JEV IN THE LOOP), and it overlaps the route checkpoint that
   another row is changing. Kept as a follow-up: the checkpoint could read the same numbers.
3. **A hard cut at a share of time or spend** (the kernel refuses calls). Cheapest and certain, but it stops work
   that is going well, contradicts the owner's notify rule for tokens, and throws away the work's report.

**Pick: 1.** It is the smallest thing that changes the model's behaviour where the evidence says it goes wrong (a
long run of model turns with nothing passing), and it never stops good work.

What counts as a passing check, from what the turn already sees: a loop that ran a command (`proc.run`) that
answered in the loop, with no tool result an error, passed; a loop that ran one with an error failed; a loop that ran
no command, or only sent one to the background, is not a check. (Reads and edits are file tools, not commands.)

The rule (`turn/progress_step.rs`, `decide`), with its named defaults:

- `MIN_LOOPS_WITHOUT_PASS = 3`: a turn that passed a check within its last 3 loops is going well, and nothing fires.
  So nothing fires while checks pass, and never right after one.
- `STEER_SHARE = 0.5`: at half its deadline, or half its spend limit spent by this turn, a steer note: replan or
  narrow, say what is left. Half leaves time to change course; earlier would nag work that is merely long.
- `STOP_SHARE = 0.85` of the deadline only: a stop-and-report note, and the next loop is the last (its stop reason
  `progress_stop`). At a 15-minute deadline that leaves about 2 minutes, enough for one loop to write the report,
  instead of being killed at the limit with nothing said. Spend never stops a turn.
- Once each per turn; a stop after a steer is allowed, a steer after a stop is not.
- Ledger: one `turn.progress` row (decision, loop, the shares, the counts) on a steer or a stop.

### theseus-yct32: the time left and the CPUs a command really has

Time, options:

1. **A per-loop harness time line (picked)**, "[The harness: 9 minutes left of this turn's 15.]", appended at a loop's
   start when the whole minutes left changed since the last line (and at the first loop). ~15 tokens a minute of
   turn; at most one line per minute, never one per loop. Outside the prefix; the turn's deadline is in memory only,
   so a daemon restart forgets it and the turn simply has none (fails quiet).
2. **The deadline in the system prompt** as a clock time. Stable bytes, but the model sees no clock: it cannot turn
   "by 14:32" into time left. And a per-turn fact in the header breaks every session's shared prefix.
3. **The time in every tool result's text.** Precise, but it is in `toolrun.rs` (at its cap), and it adds bytes to
   every call of every turn whether or not it has a deadline.

**Pick: 1.** The deadline's source is a new optional `turn.submit` field, `deadline_secs` (from the request's
arrival), and `theseus ask --deadline SECS`, which a benchmark driver can pass (follow-up: `bench/` is another
row's). It is a protocol field, not a config key: a turn with none behaves exactly as today.

CPU, options:

1. **One stable fact in the session block (picked)**, beside `Directory:`: "CPUs available to commands: 2 (cgroup
   quota; the host shows 32)." or, with no limit, "CPUs available to commands: 4." Read once per process from the
   daemon's own cgroup (v2: `cpu.max`, `cpuset.cpus.effective`, every ancestor's quota, the tightest wins) and the
   host's online CPUs, min'd with `available_parallelism`. Same bytes every request: the session block changes once
   at the upgrade (one `system_changed` recompile), never per loop. Only where tools run (a private place).
2. **The header's tools note.** Also stable, but the header is shared byte for byte across machines' goldens and
   sessions, and the fact is about where commands run, which is the session block's subject.
3. **Set `MAKEFLAGS`/`CMAKE_BUILD_PARALLEL_LEVEL` in each job's environment.** Silent and partial (every build tool
   has its own knob, and `nproc` still lies); "detection written by hand can't be finished": tell the model the fact
   instead.

**Pick: 1.** Fails safe: any unreadable or malformed file is no limit, and the count falls back to
`available_parallelism`; nothing here can fail a turn.

### What it does not do

- No config key, no raised cap or budget, no store format change (the counters live in memory on the turn's side).
- No edit to `routing/checkpoint.rs`, `turn/checkpoint_step.rs`, `routing/effort.rs`, `turn/effort_step.rs`,
  `toolrun.rs`, `render.rs` or the Discord runtime.
