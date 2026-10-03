# theseus-sandbox

L1, the native sandbox (design `docs/design/m4-boundaries.md` §2.2, §2.4, §7): a job in its own user, pid, mount,
network, uts, ipc, and cgroup namespaces, over a view built from an empty root, as the operator's own uid with no
capabilities, under `no_new_privs` and a seccomp deny list, below an init whose exit kills the whole tree. Built by
lane 17a; read by the job wrapper's L1 path (`theseus-kernel`'s `job_l1.rs`, step 17b). Its egress proxy
(`egress.rs`, 18b) waits for 18c.

## What's here

- `spawn.rs`: the wrapper's side. `spawn(spec, init, stdio)` clones the init with a pidfd, writes user namespace
  1's maps, moves it into the job's cgroup when there is one, releases it, hands it the spec over a pipe, and
  returns once the command has exec'd, or with `SpawnError { stage, error }`. `SandboxChild` kills, waits, and
  `reaped(status)` reads the init's report for a wrapper that reaps with `waitpid(-1)`.
- `init.rs`: `init_main`, the `job-sandbox` role (`theseusd job-sandbox`): pid 1 of the job.
- `view.rs`: the view (system binds, `ro_paths`, overlays over the workspace, HOME, `/tmp`, `/dev`, `/proc`,
  `/sys`), and `Spec::hidden` (17b), the paths covered whatever binds them: Theseus's floor and socket, and the
  approve list's paths.
- `seccomp.rs` (hand-built classic BPF; `seccompiler` is not in the offline registry), `cgroup.rs` (`own`,
  `delegate`, `JobCgroup`: its limits, `kill`, `populated`, and `procs`, which an 18a stop counts), `report.rs` (`Started`, `Exit`, `Scratch::summary`), `spec.rs`, `egress.rs`.

## Invariants

- **Between the clone and the init's exec, raw system calls only**, on memory prepared before the clone, with
  every signal blocked: the wrapper may have threads.
- **The init runs first in `theseusd`'s `main`**, before anything else: one thread, the operator's umask from its
  wrapper, its stderr the job's.
- **Call `spawn` from the thread that lives as long as the job**: the init's `PR_SET_PDEATHSIG` fires when the
  spawning thread exits, not the process.
- **`cgroup::delegate` only on a cgroup that is the daemon's own and delegated**: it moves every process in it.
  The core asks systemd (`Delegate=yes`, and the unit's main process is the daemon) before it calls it, and, for a
  unit that keeps its jobs across a stop (`KillMode=process`), that the unit's `ExecStopPost=` runs `theseusd
  cgroup-release` (`cgroup::release`). systemd starts the next daemon in the unit's own cgroup, which the kernel
  refuses while its children have controllers on and a job of the old daemon still runs.
- **The view shows nothing it was not given.** A path a `Spec` names must be absolute; a hidden path the view does
  not hold is skipped.

## Tests

- `tests/contract.rs` (`harness = false`): one test per §7 clause in real L1 jobs, unprivileged; the binary
  re-execs itself as the init and the probe. `tests/bench.rs`: 100 spawns, p50 and p95, against 25 ms.
- 17b's daemon tests (`crates/theseusd/tests/sandbox.rs`) run real L1 jobs through the whole daemon.

## Traps

- A test's helper binary must be bound into the view (`ro_paths`) when it lives under HOME, which is an empty tmpfs
  inside.
- `RLIMIT_FSIZE` (`output_mb`) caps every file the command writes, scratch included; printed output goes through a
  pipe, which it does not cap.
