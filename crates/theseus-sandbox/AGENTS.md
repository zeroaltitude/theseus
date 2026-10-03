# theseus-sandbox

L1, the native sandbox (design `docs/design/m4-boundaries.md` §2.2, §2.4, §7): a job in its own user, pid, mount,
network, uts, ipc, and cgroup namespaces, over a view built from an empty root, as the operator's own uid with no
capabilities, under `no_new_privs` and a seccomp deny list, below an init whose exit kills the whole tree. Built by
lane 17a; read by the job wrapper's L1 path (`theseus-kernel`'s `job_l1.rs`, step 17b). Its egress proxy
(`egress.rs`, 18b) is wired in by 18c: the wrapper runs it for a job whose list is not empty (`job_egress.rs`).

Key modules: `spawn.rs`, `init.rs`, `view.rs`. Read by: the kernel's `job_l1.rs`.

## What's here

- `spawn.rs`: the wrapper's side. `spawn(spec, init, stdio)` refuses a root operator's job (`refusal`,
  theseus-pv6i), clones the init with a pidfd, writes user namespace 1's maps, releases it, hands it the spec over
  a pipe, and returns once the command has exec'd, or with `SpawnError { stage, error }`. `SandboxChild` kills,
  waits, and `reaped(status)` reads the init's report for a wrapper that reaps with `waitpid(-1)`.
- `init.rs`: `init_main`, the `job-sandbox` role (`theseusd job-sandbox`): pid 1 of the job.
- `view.rs`: the view (system binds, `ro_paths`, overlays over the workspace, HOME, `/tmp`, `/dev`, `/proc`,
  `/sys`), and `Spec::hidden` (17b), the paths covered whatever binds them: Theseus's floor and socket, and the
  approve list's paths.
- `seccomp.rs` (hand-built classic BPF; `seccompiler` is not in the offline registry), `report.rs` (`Started`,
  `Exit`, `Scratch::summary`), `spec.rs`. No cgroup of its own (theseus-gyin, 2026-10-03): a job's processes are
  capped by `RLIMIT_NPROC` in its own user namespace, its files by `RLIMIT_FSIZE`, its scratch by the tmpfs caps, and
  its memory not at all, as an L0 job's is not.
- `egress.rs` (18b, wired at 18c): the job's `Proxy` on the listener the init hands over. `Proxy::decide` makes each
  `CONNECT`'s `Outcome` a value (tunnel, or refuse with a status and why) before the proxy acts. Nothing reads inside a
  tunnel: credentials as stand-ins, which would have ended TLS here, were dropped for v1 (theseus-gh7, Eddie
  2026-10-03), so a secret a job holds is contained by the list alone. `Running::finish` stops it once the job has ended,
  ending any tunnel a server holds open so every one is recorded; `Summary` is the completion's `detail.egress`
  (one entry per host reached, one per refusal). `Allow` (the list's entries) lives in `theseus_tools::net`.

## Invariants

- **Between the clone and the init's exec, raw system calls only**, on memory prepared before the clone, with
  every signal blocked: the wrapper may have threads.
- **The init runs first in `theseusd`'s `main`**, before anything else: one thread, the operator's umask from its
  wrapper, its stderr the job's.
- **Call `spawn` from the thread that lives as long as the job**: the init's `PR_SET_PDEATHSIG` fires when the
  spawning thread exits, not the process.
- **No L1 job as root.** Linux never applies `RLIMIT_NPROC` to the initial user namespace's root, and user
  namespace 1 maps the job back to the operator, so a root operator's job would have no process limit: `spawn`
  refuses it before it makes anything, with the stage "checking the job's process limit" (theseus-pv6i).
- **The view shows nothing it was not given.** A path a `Spec` names must be absolute; a hidden path the view does
  not hold is skipped.

## Tests

- `tests/contract.rs` (`harness = false`): one test per §7 clause in real L1 jobs, unprivileged; the binary
  re-execs itself as the init and the probe. `clause_09_root_is_refused` is the one case to run as root (`sudo
  <binary> --exact clause_09_root_is_refused`); as root every other case fails, since every job is refused.
  `tests/bench.rs`: 100 spawns, p50 and p95, against 25 ms.
- 17b's daemon tests (`crates/theseusd/tests/sandbox.rs`) run real L1 jobs through the whole daemon, and 18c's
  run them through a real proxy to a stand-in host (`THESEUS_TEST_EGRESS_DNS`, which only a debug build reads).
- `tests/bench.rs`'s `connect_first_byte_200` (18c): a `CONNECT`'s first byte through the proxy against a direct
  connection, against §2.10's 2 ms added.

## Traps

- A test's helper binary must be bound into the view (`ro_paths`) when it lives under HOME, which is an empty tmpfs
  inside.
- `RLIMIT_FSIZE` (`output_mb`) caps every file the command writes, scratch included; printed output goes through a
  pipe, which it does not cap.
