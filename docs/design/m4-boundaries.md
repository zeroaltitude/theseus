# Theseus M4, Boundaries apart from AWS: the design (roadmap steps 17 to 22)

_Checked in 2026-09-30 from the design lanes. Scrubbed for this public repository: local paths to the repo's checkout and to the agents' operating notes._

_Design lane `m4` (theseus-zaz.2). Tabitha/Claude, started 2026-09-30 15:26 MST. Docs only: nothing in
the repo was changed. Beads: theseus-7ve (the M4 epic), theseus-3vu (Appendix F's M4 additions),
theseus-8kk (the ontology). [Spec](../the-ship-of-theseus.md) read at v0.61 (§1, §2, §3.9, §3.16, §3.19, §4.1 to §4.4a, §5.5, §5.6, §7,
Appendix B, Appendix F, P6, Part III B1, J1, Z1, and A4 item 10). Code read at `de880fc`._

## Outline

0. The short version
1. Scope and principles
2. The design
   - 2.1 What exists today
   - 2.2 L1, the native sandbox
   - 2.3 Cancellation verified per backend
   - 2.4 Egress, and credential brokering under L1
   - 2.5 Labels: one model for integrity and confidentiality
   - 2.6 Integrity: how the labels take in T1's hold (the key question)
   - 2.7 Confidentiality: audience-safe compilation, disclosure, graduation
   - 2.8 The ontology's first slice
   - 2.9 Control-plane separation
   - 2.10 FAST
   - 2.11 EXQUISITE VISIBILITY
   - 2.12 Catalog: config, protocol, store, ledger
3. The build plan
4. What it needs from Eddie
5. Open questions, with defaults
6. Risks, and what would change the plan
7. Appendix: probes of this machine (2026-09-30)

## 0. The short version

- **L1 is native, in the job wrapper.** It is the same binary in a role (`theseusd job-sandbox`), with no
  bubblewrap and no sidecar. The job gets:
  - user, pid, mount, network, uts, and ipc namespaces;
  - a tmpfs root with read-only binds of the system and the workspace, and an overlay whose writes are
    scratch;
  - a fresh `/proc` and `/sys`, masked as Docker masks them, and four device nodes;
  - a nested user namespace, so the job's uid is the operator's and it holds zero capabilities;
  - `no_new_privs` and a seccomp deny list;
  - an init process whose exit kills the whole tree.

  Every piece was probed working, unprivileged, on Eddie's machine today (§7).
- **Nothing on the start path.** The sandbox costs something per job (the target is a start p95 under 25 ms,
  benched), and its host probe runs after serving.
- **L0 stays the default** (§1). A job goes to L1 in three cases: the model asks (`proc.run { sandbox: true }`),
  the operator's `[sandbox] l1_argv` names it, or `[sandbox] default = "l1"` is set (the open-source default).
  A call that asked for L1 never falls back to L0.
- **Cancellation says how it was verified**, per backend:
  - L1: the pid namespace;
  - a cgroup, when the daemon's cgroup is delegated;
  - L0: the wrapper's walk of its tree.

  Today a cancel kills only the process group, and a deadline kills only the direct child, so a `setsid`
  grandchild survives both. Both are found in the code (§2.3).
- **Credentials under L1 follow decision 15.** A run-time request comes over a per-job socket.
  - It runs at the posture of the tool that started the job, and a secret's own stricter posture wins.
  - A fetch from 1Password itself always waits.

  Egress is an allowlist through a CONNECT proxy that the wrapper runs. It uses DD5's public-only resolver, so
  no allowed name can lead to the metadata service or localhost.
- **Labels.** Every node gets one small label with two parts:
  - **integrity**: trusted or untrusted, by origin and by transmission, never by exposure;
  - **readers**: who may see it.

  **T1's hold becomes the session's latch**, which is what it already is in substance: the integrity high-water
  mark of what the session's context admitted since the operator last trusted it. One gate step reads it, T1's
  rules are unchanged, and its tests stay as the floor's contract.

  The labels close two paths T1 leaves open today:
  - a file written in a latched session latches whoever reads it (a hash carries it);
  - an L1 job that reached the network gives an external result, which closes theseus-20f for L1.
- **Confidentiality is enforced at compile time.** The compiler admits a node only when its readers cover the
  session's audience, and a withheld tool result keeps its pairing as a placeholder.
  - On Eddie's deployment nothing is withheld: the DM, the CLI, the web UI, and the private test channel all
    have him alone as their audience.
  - The disclosure simulator proves the rule over synthetic shared channels.
  - Widening an audience is **graduation**: a new node with a warrant, never a relabel.
- **The ontology's first slice.**
  - A kinds table in the store: channel, guild, and person are given; topic is interpreted.
  - Topics are declared by the operator.
  - Guidance is composed by the `chain` and `intent_line` rules, in a compile walk over an in-memory snapshot.
  - Memberships route context and never grant access.
- **Control-plane separation.** `theseusd install` prints a plan by default, and applies it with `--apply` as
  root. It creates a `theseus` user that owns the store, the vault token, and the config copy.
  - L0 jobs still run as the operator, through a **job host** (`theseusd job-host`, a user service) that the
    daemon drives through the existing `JobLauncher` seam.
  - The chain proves it in a container, since sudo on this machine needs a password.
- **16 steps: 10 SPINE and 6 LANE.** The lanes can be built in worktrees beside the spine: the sandbox crate,
  the egress proxy, the disclosure simulator, the ontology crate, the web UI's view, and the installer.

## 1. Scope and principles

### What M4 is for

The spec's P6: "Make the durability and safety claims true, and measure them." [The AWS lane](aws-toolset.md) owns durability
(steps 14 to 16). This lane owns the safety claims, which the spec has so far only pointed at:
- **The environment is the boundary.** §3.9: "it is not a sandbox. For arbitrary commands, the operator's
  control is `proc.run`'s posture, and the boundary is the environment (§7; L1 in M4)". L1 turns J1's "speed
  bump before L1, not a boundary" into a boundary, and closes B1's two known gaps: another job's
  `/proc/<pid>/environ`, and gh's stored login in `~/.config/gh/hosts.yml`.
- **Information-flow labels.**
  - Integrity by transmission and the external origin (Appendix F, adopted; theseus-3vu).
  - Confidentiality with audience-safe compilation (§3.9 "Information flow": "Enforcement is at compile time,
    not at output time").
- **The fungible ontology's first slice.** §2 FUNGIBLE ONTOLOGY, and §4.1a, whose playback Eddie confirmed
  on 2026-09-27 at 20:26, including both guardrails.
- **Control-plane separation as an option.** §1 says "strongly recommended in the documentation and the
  installer, not a default".

### Eddie's terms, which bind every step

- **Notify over block.** "Asked, sure, but a hard no, almost never." Nothing in M4 refuses a call, so the
  boundaries live in the environment, not in the gate.
- **Decision 14** (2026-09-29, 09:23): a label-class outward check waits for approval instead of refusing.
- **Decision 15** (2026-09-29, 09:39): in the sandbox, a job's run-time credential request follows the posture
  of the tool that started it (open grants, notify grants with a notice, approve waits). 1Password itself
  always waits, and per-secret postures carry over from the broker. In his words, "transitivity of auth
  settings is generally appropriate."
- **FAST is a primary goal**, and **tracer-bullet** is the roadmap's rule: one thin path end to end, proved
  live, and everything else filed.

### What v1 needs from this phase (the happy path), and what waits

| Capability | v1's happy path (built) | Waits (filed, not built) |
|---|---|---|
| L1 sandbox | GLM runs `proc.run { sandbox: true }` of a script: no network, no credentials, zero capabilities, and the tree killed on exit. The contract tests prove each denial | Promoting overlay writes back to the tree; Landlock as a second layer; PTYs in L1; CPU and io limits where they are not delegated |
| Cancellation | `/stop` and `theseus executions cancel` verify, per backend, that the job's whole tree is gone, and say how | AWS backends' `cancel_unsupported` (M7, A1) |
| Egress | An operator allowlist through the wrapper's proxy. A call that names a host beyond it waits. A job that connected out gives an external result | Recognizing request shapes (receive-pack, publish); plain-HTTP forwarding beyond CONNECT |
| Credentials under L1 | Spawn grants work in L1 as at L0. A run-time request over the job's socket is judged by decision 15 | An ad hoc `op://` fetch (always waits, per decision 15), STS session tokens (the AWS lane mints them; M4 leaves the seam) |
| Integrity | The `external` origin; labels by transmission (a brief, a report, a file); T1's hold as the latch; T1's rules unchanged | The `Advisory`, the `quarantined` level, and the one-step-stricter rule; "text shaped like instructions" (Jev, M5) |
| Confidentiality | Readers on every new node; the session's audience; the compile filter with placeholders; graduation; the outbox check; the disclosure simulator | Recall across sessions (M6); gliding (M7); MCP responses (M7); a fetch's URL as outward text (T1's accepted gap) |
| Ontology | The kinds table; topic as the first new kind; operator-declared memberships; guidance; the compile walk; CLI and web UI | Jev's `categorize.v1` (M5); embeddings, sweeps, dreams, and the `ranked` and `recall_only` rules (M6); §5.5's namespaces as kinds (M6) |
| Control plane | `theseusd install` plans and applies; the daemon as `theseus`; the job host; proved in a container | Switching Eddie's own daemon (his call, with sudo); several operators' job hosts |

### Principles as constraints on this phase

- **Nothing loosens without the owner.** L1 does not loosen a call's posture in M4, and labels only tighten.
  Whether L1 should earn a looser posture is Eddie's to say (§5, question 1).
- **Deterministic.** No model judgment decides L1's contract, a label, or the latch. Jev may tighten later
  (M5), and never loosen.
- **FAST.** Nothing new runs before serving. Per-job and per-compile costs land with a bench row (§2.10).
- **The reader rule** (theseus-wjy): nothing is declared without its reader.
  - There is no `quarantined` level before the `Advisory` that writes it.
  - There is no `ranked` composition rule before M6's lessons.
  - No config key exists before the code that honors it.
- **One static binary.** The sandbox init, the job host, and the credential helper are `theseusd` in a role,
  never a sidecar. `/usr/bin/bwrap` is installed here, and is used only as a behavioral reference in review.
- **T1 stays the floor** (the lane's key question). The labels generalize T1's hold without changing what
  waits, and T1b's answers carry over unchanged: `wake.at` is exempt, and a Discord `/trust` clears a hold.

## 2. The design

### 2.1 What exists today (code at `de880fc`)

| Area | What exists | What M4 changes |
|---|---|---|
| Job launch | `JobLauncher` (`theseus-core/src/toolrun.rs:44`), with `WrapperLauncher` and the tests' `InlineLauncher`. `spawn_detached` (`theseus-kernel/src/job.rs:63`) starts `theseusd job-wrapper` with `setsid`, a cleared environment, `[tools] proc_env` (PATH, HOME, USER, LANG, LC_ALL, TERM, TZ, CARGO_HOME, RUSTUP_HOME), then the call's env, then the broker's grant. The wrapper is a child subreaper, and lingers until its descendants exit | The wrapper gains an L1 path (`WrapperArgs.sandbox`). The launcher seam also carries the job host (§2.9) |
| Deadline | The wrapper's own deadline kills its direct child (`child.kill()` in `run`), then lingers until the descendants exit | The whole tree is killed, per backend (§2.3) |
| Cancel | `Driver::terminate_all` (`theseus-core/src/rpc/driver.rs:187`) calls `job::terminate(pid, corr, 2 s)`: SIGTERM, then SIGKILL, to the wrapper's process group. It counts as verified once the wrapper's pid is gone. `CancelState` is `requested`, `acknowledged`, `termination_verified`, `unsupported`, or `outcome_uncertain` (`types.rs:411`). In-process calls are `unsupported` | Verified means the tree is gone, and the record says how (§2.3) |
| The broker | `broker.rs`: grants by direct argv (`for_job`), toollet grants (`ToolCtx::secret`), and per-secret postures. The M4 hook is the comment above `Broker::secret_for_tool` | A per-job socket for run-time requests (§2.4) |
| The gate's order | `ToolRuntime::gate` (`toolrun.rs:817`): `decide_with` (floor, lists, allow list, posture, tightening), then `brokered`, then `external::gate`. A `Decision` carries its posture, reason, notice, `floor`, `granted`, and `external` | `external::gate` becomes the integrity step (§2.6). The class (L0 or L1) is added to the decision and to what a confirm binds |
| T1 | `external.rs`: `SessionRecord.external: Option<ExternalText>`, written by `hold` in three frames (a result's completion, `open_task`, and `take_reports`), with `session.external_read`. It is cleared by `policy.trust` or a trusted approval | Kept, and fed by labels (§2.6) |
| Nodes | `node.rs`: `Origin` is `operator`, `agent`, `tool`, or `harness`. `ToolResult.external: Option<External { url }>` (DD5). There is no label | A `label` on every new node, and origin `external` (§2.5) |
| Edges | `EDGE` records are no longer written (theseus-hco). A compilation carries `derived_from`. Transmissions between sessions are nodes: a task's brief (`Node::relayed`, origin `agent`) and its report (origin `harness`) | The label rides the transmitted node. No edge record is needed |
| Compiler | `compiler.rs` compiles a session's own transcript. Its triggers are deterministic, and its `Manifest` records versions, profile, model, the system digest, tools, and context files. There is no audience | The audience, readers, the integrity in play, what was withheld, and memberships (§2.7, §2.8) |
| Places | `outbox.rs`: `target(session)` is `discord:<place>` for a bound place, or where a task reports; `place_session`, `bind_place` | This is where a session's audience comes from (§2.7) |
| Viewers | `theseus-discord/src/viewers.rs` works out who can view a guild channel, for trusted channels (theseus-sgh). It needs the Server Members intent | Reused as the audience of a guild channel |
| Context files | `[context] files` and `[personas.<name>] files`, compiled into every session's system block | They get readers (§2.7) |
| Answers | `approval.rs` (trusted channels and users) and `peer.rs` (J1's trace, `judge_act`) | Graduation and ontology writes are judged by `judge_act` |
| Store kinds | SESSION, LEDGER, META, EXECUTION, ACTION, COMPLETION, NODE, EDGE (no longer written), COMPILATION, and OUTBOX (11). Numbers 10 and 255 were reserved and never written. NODE and SESSION are at schema 2 | NODE 3; COMPILATION 3, then 4; ACTION 3, then 4; SESSION unchanged. Each bump lands with its reader (§2.12) |
| Tools that leave | Only replies, through the outbox. No typed tool pushes or posts: the tools are fs.read, write, edit, patch, list, glob, and grep; git.diff and git.log; text.diff; proc.run; http.fetch; web.search; and the harness's task.create and wake.at | `labels::may_leave`, with the outbox as its first caller (§2.7) |
| Install | Manual: `cp` into `~/.local/bin`. There is no `[owner]` section; `[approval] trusted_users` lists Discord users | `theseusd install` (§2.9) |
| Dependencies | `libc` in the kernel crate. The local registry has `nix` and `rustix`, and no seccomp or Landlock crate | One new crate, `seccompiler` (Apache-2.0 OR BSD-3-Clause; `deny.toml` allows both), or a hand-built BPF filter (§5, question 7) |

### 2.2 L1, the native sandbox

**Which class a job runs in.** A `proc.run` call's class is decided at plan time, deterministically, and it is
decided at most one way: toward L1.

| Source | Effect |
|---|---|
| `[sandbox] default` | `"l0"` built in, so an upgrade changes nothing. The template documents `"l1"`, and `theseusd install` writes `"l1"` into a config it generates. So the open-source default is L1 (§1), and Eddie's note stays L0 |
| `[sandbox] l1_argv` | The operator's argv prefixes that always run in L1 (for example `["npm", "install"]`), matched as `allow_argv` matches |
| The model's `sandbox: true` | Asks for L1. `sandbox: false` cannot override the list or the default |
| Jev's `shell.v1` | M5, choosing within the permitted set. Never wider |

- **No fallback.** When L1 cannot run on this host (the probe failed, or a spawn fails), the call fails with the
  reason: "L1 is not available here: … The call did not run." It never runs at L0 instead.
- **The class is bound.** It goes into the gate record, the proposal digest that a confirm binds (so an
  approved L1 run cannot dispatch as L0), `tool.started`, the tool-call node, and every surface.
- **The gate is unchanged.** The floor, the lists, and the postures judge an L1 call as they judge an L0 one. In
  M4, L1 earns no looser posture; that is Eddie's call (§5, question 1).

**The spawn chain.** It uses one binary in three roles:

```
theseusd                        the daemon
└─ theseusd job-wrapper         host namespaces, setsid, child subreaper: spool, deadline, proxy (18b)
   └─ theseusd job-sandbox      clone(NEWUSER|NEWPID|NEWNS|NEWNET|NEWUTS|NEWIPC): pid 1 of the job
      │                         user ns 1 maps 0 to the operator's uid; builds the view, then drops
      │                         its own capabilities and takes the seccomp filter
      └─ <the command>          user ns 2 maps the operator's uid to ns 1's 0: the same uid as at L0,
                                no capabilities, no_new_privs, seccomp
```

- **Why two user namespaces.** An unprivileged process may map one uid.
  - The init must keep its capabilities across its `execve` to mount, so it is root in user namespace 1.
  - The job must see the operator's own uid, or tools that check it misbehave (Appendix B: npm, pip, and
    anything that "wants the real user id"). So the command runs in a nested user namespace 2, which maps the
    operator's uid back. This is what bubblewrap does for `--uid`.
- **Why the init execs at once.** The wrapper will have threads (18b's proxy), and a clone of a threaded
  process may run only async-signal-safe code before `execve`.
- **The wrapper writes user namespace 1's maps**: `setgroups deny`, then one uid line and one gid line. It then
  releases the init over a pipe. The init writes namespace 2's maps for the command in the same way.
- **The init stays as pid 1.** It reaps, forwards SIGTERM and SIGINT to the command, and exits with the
  command's status. When it exits, the kernel kills everything left in the pid namespace, so the tree dies
  with the job by construction.

**What the job sees.**

| Path | In L1 |
|---|---|
| `/` | A fresh tmpfs, after `pivot_root` |
| `/usr`, `/bin`, `/sbin`, `/lib*`, `/etc` | Read-only binds, so programs run as they do at L0 |
| `[sandbox] ro_paths` | More read-only binds, such as `~/.cargo` and `~/.rustup` for offline Rust builds. The default is the system directories alone |
| The workspace roots | At the same absolute paths: the tree read-only, under an overlay whose upper is tmpfs. Writes succeed, into scratch, capped by `scratch_mb` |
| `$HOME` | An empty tmpfs at the same path: no `~/.ssh`, `~/.aws`, `~/.config/gh`, or 1Password token |
| `/tmp` | A tmpfs, capped |
| `/proc` | A new proc for the pid namespace, so only the job's own processes, masked as Docker masks it: `kcore`, `keys`, `timer_list`, and `sched_debug` covered by `/dev/null`; `sys`, `sysrq-trigger`, `irq`, `bus`, and `fs` read-only. `cpuinfo`, `meminfo`, and `stat` stay, since toolchains read them. `subset=pid` is the stricter choice, if 17a shows the toolchains do without them |
| `/sys` | A fresh sysfs, read-only, mounted inside the job's network namespace, so it lists `lo` alone. `firmware`, `kernel/security`, and `fs/cgroup` are masked. The probe showed that the host's old `/sys` would list the host's interfaces. If the kernel refuses the mount (`mount_too_revealing`), L1 mounts no `/sys`, and says so |
| `/dev` | A tmpfs with `null`, `zero`, `random`, and `urandom` bound from the host, the `fd` and std links, and a `shm` tmpfs |
| Theseus's state, spool, socket, bindings file, and binary directory | Absent |
| Network | A namespace with `lo` alone. 18b and 18c add the proxy on 127.0.0.1:3128 |
| `/run/theseus/` | 18d: the credential helper (this binary, read-only) and the job's broker socket |
| Hostname | `theseus-l1` |

**The contract of §7, each clause with its test.** Each probe runs inside L1, in `crates/theseus-sandbox/tests/`.

| §7 clause | Mechanism | Contract test |
|---|---|---|
| User, pid, mount, uts, ipc, and net namespaces | Clone flags | Each `/proc/self/ns/*` inode differs from the host's |
| No network by default | A network namespace with `lo` alone | A connect to a public address fails, `ENETUNREACH` |
| No metadata service | The same, and the proxy's public-only resolver (18b and 18c) | A connect to 169.254.169.254:80 fails, `ENETUNREACH`. Through the proxy, a name that resolves to it is refused |
| No localhost services, Theseus's own included | The network namespace, and socket paths never mounted | A listener the test opens on the host's 127.0.0.1 is refused inside. The daemon's socket path is `ENOENT`, and a host abstract socket is refused |
| Capabilities: none | User namespace 2, an exec as non-root, and the bounding set dropped | `CapInh`, `CapPrm`, `CapEff`, `CapBnd`, and `CapAmb` are all 0, and `NoNewPrivs` is 1 |
| A default seccomp profile | The filter below | `Seccomp: 2`. `unshare`, `mount`, `ptrace`, `bpf`, `keyctl`, and `io_uring_setup` each give `EPERM` |
| No device nodes beyond null, zero, and random | The `/dev` tmpfs | `/dev` lists exactly the allowed set |
| `/proc` and `/sys` masked | A fresh proc and sysfs, with Docker's masks | `/proc/kcore` reads empty, `/proc/sys` refuses a write, `/proc/1` is the init, and `/sys/class/net` lists `lo` alone |
| cgroup limits on CPU, memory, pids, and disk, with output caps | A delegated cgroup where there is one (below), tmpfs `size=`, and `RLIMIT_FSIZE` | A fork loop stops at `pids.max`. A scratch write past its cap gives `ENOSPC`. Output past its cap is cut, with a note |
| The whole tree killed on timeout or cancel | The pid namespace | A job that `setsid`-forks a sleeper is cancelled, and afterwards no host process remains in the job's pid namespace |
| Anything not granted is denied | A view built from an empty root | A write outside scratch gives `EROFS`. `~/.config/gh/hosts.yml` is `ENOENT`. The environment holds only `proc_env` and granted names |
| No ambient credentials | An empty HOME, a cleared environment, and no agent sockets | `SSH_AUTH_SOCK` is unset, and `gh auth status` says it is not logged in |

**The seccomp filter.** It allows by default and denies a list with `EPERM`, never with a kill, so a program
gets an error it can report. Its shape follows Docker's default profile.
- Namespaces and mounts: `unshare`, `setns`, `mount`, `umount2`, `pivot_root`, the new mount API calls, and
  `clone` with any `CLONE_NEW*` flag. `clone3` gets `ENOSYS`, so the C library falls back to `clone`, whose
  flags the filter can read.
- The kernel: module loading, `kexec_*`, `reboot`, `swapon`, `swapoff`, `acct`, `quotactl`, and the time
  setters.
- Introspection: `ptrace`, `process_vm_readv` and `process_vm_writev`, `perf_event_open`, `bpf`,
  `userfaultfd`, `open_by_handle_at`, `name_to_handle_at`, `iopl`, and `ioperm`.
- Keys: `keyctl`, `add_key`, and `request_key`.
- `io_uring_*`, whose operations would bypass the filter.
- A foreign ABI (x32, i386) kills the process.

The init takes the same filter once it has built the view, so a job cannot use pid 1's capabilities through
any call the filter denies.

**Limits.**
- **With a delegated cgroup**, the job gets its own cgroup, `<daemon cgroup>/jobs/<corr>`:
  - `memory.max` is `[sandbox] memory_mb`, 2048 by default, and `pids.max` is `pids`, 512 by default;
  - `cpu.max` applies only where the cpu controller is delegated. It is not by default: this machine's user
    manager delegates `memory` and `pids` only.

  A daemon's cgroup is delegated when the daemon runs as a systemd user service with `Delegate=yes` (§2.9's
  `--user` install). The daemon moves itself into a leaf, `daemon/`, since cgroup v2 puts no processes in an
  inner node. It does this lazily, at the first L1 job, never on the start path.
- **Without one**, the namespaces and seccomp still hold. That is how a daemon started from a shell is placed:
  the one found running during the probe (§7) sat in the OpenClaw gateway's cgroup, reparented to `systemd
  --user`. The result, health, and the Observatory then say "limits: none; the daemon's cgroup is not
  delegated".
- **Scratch and `/tmp`** are tmpfs mounts with `size=`, and are charged to the cgroup's memory when there is
  one.
- **Output** is capped by `RLIMIT_FSIZE` on the command, at `[sandbox] output_mb`, 64 by default.

**Scratch writes are reported and discarded.** Before it exits, the init walks the overlay's upper directory
and sends a summary to the wrapper over a pipe: the count, the bytes, and the first 20 paths. It rides in the
completion's `detail.scratch` ("wrote 3 files, 41 KB, to scratch: target/…; discarded"). Promoting them back to
the tree is filed, and needs a gated, labeled write (§2.6).

**Cost and the probe.**
- A start is one clone, two execs, about 30 mount calls (binds, the overlay, and the masks), and a filter. The target is a p95 under 25 ms from
  the dispatch to the command's exec, against the spec table's "~100 ms". It is measured by a bench row:
  `theseus-sim bench jobs --class l0|l1`.
- After serving, and again after an exec restart, a background probe runs `/bin/true` in L1 once. It records
  `sandbox.probe`: ok or why not, the features found, the start's latency, and the cgroup mode. A job never
  waits for it; a job that finds L1 broken says why.

### 2.3 Cancellation verified per backend

The lifecycle already exists (§3.16; `CancelState`). What changes is what **verified** means: every process of
the job is gone, and the record says how it knows.

**Two gaps in the code today.**
- A cancel signals the wrapper's process group, and counts as verified once the wrapper's pid is gone. A
  descendant that called `setsid` or `setpgid` is in another group, so it survives. The wrapper, a member of
  the group, dies too, and the daemon adopts the survivor as an orphan, reaps it only once it exits, and never
  kills it.
- A deadline kills the wrapper's direct child alone, and the wrapper then lingers until the rest exit.

| Backend | How it stops | How it is verified | When it cannot be |
|---|---|---|---|
| In process (fs, git, text: the CPU pool) | It cannot be stopped. It runs to its end, within the 120 s in-process deadline | — | `unsupported`. The call's real outcome is still recorded when it ends, and revives nothing |
| Async (http.fetch, web.search) | The task is aborted (`JoinHandle::abort`) | The handle reports it cancelled | `termination_verified`, `verified_by: task` |
| L0 job, no cgroup | The daemon signals the **wrapper alone**, not the group. The wrapper, a subreaper, stops its whole tree: it SIGSTOPs every descendant it finds, rescans until the set is stable, then SIGKILLs them all and reaps them | The wrapper finds no descendant left, and its completion carries the verdict (`killed: 4, survivors: 0`) | A survivor (a process in `D` state), or a wrapper that did not answer within the grace. Then the daemon kills the group, as today, and the state is `outcome_uncertain`, with why |
| L0 job, with a cgroup | `cgroup.kill` on the job's cgroup | `cgroup.events` reads `populated 0` | As above |
| L1 job | SIGKILL to the pid namespace's init | The wrapper reaps the init, and the kernel has killed the namespace | Always verifiable |
| Job through the job host (§2.9) | The host runs the L0 or L1 path | The host's report | `outcome_uncertain` when the host is gone |
| AWS classes (M7) | Stop the task | The service API | `unsupported` for a running Lambda invocation (§3.16) |

- **A deadline uses the same stop.** The wrapper's own deadline stops the whole tree, not only its child.
- **What L0 cannot see.** A job can ask a process outside its tree to act for it: a user systemd unit, a tmux
  server already running, cron (J1's known gap). At L0, verified means "every descendant of the wrapper", and
  the record says so (`scope: descendants`). L1 closes this path, since no session bus and no tmux socket
  exist in the view.
- **An older wrapper.** A wrapper started before an install runs the old image, and dies at the first SIGTERM.
  The daemon then kills the group, as today, and records `verified_by: group`.
- **Records.** The action's cancel gains `verified_by` (`pidns | cgroup | tree | group | task | none`) and
  `survivors`. The ledger gets `action.cancel_verified` with the counts, and `action.cancel_unsupported` and
  `action.cancel_uncertain` with why.
- **Surfaces.**
  - `theseus executions` shows "⏹️ cancelled (verified: pid namespace, 4 processes)".
  - Discord's tool line keeps W1's `⏹️ stopped` and adds "(verified)" or "(not verified: …)".
  - Health counts each backend's outcomes since the daemon started.
- W1's `/stop` and every cancel go through the same `terminate_all`, so both get this. theseus-w98, a cancel
  that leaves a planned call planned, is in fix batch 1, before this step.

### 2.4 Egress, and credential brokering under L1

**Egress: an allowlist through the wrapper's proxy.**
- **The list.** `[sandbox] egress = ["github.com:443", "*.crates.io:443"]` is the operator's stated will, as
  `allow_argv` is. It is empty by default, so L1 has no network.
- **A call may name more:** `proc.run { sandbox: { egress: ["pypi.org:443"] } }`. A host beyond the list makes
  the call wait at step 2 of the order, as a path outside the roots does. Its approval reaches only the hosts it
  named, as a private origin's does (DD5).
- **Enforcement.** The job's network namespace holds `lo` alone.
  1. The init opens a listening socket on 127.0.0.1:3128 inside the namespace, and hands its descriptor to the
     wrapper over a socket pair (`SCM_RIGHTS`) before the command starts.
  2. The wrapper, in the host's namespace, serves HTTP `CONNECT` on it. Plain-`http://` forwarding is filed.
  3. The job's environment gets `HTTPS_PROXY`, `HTTP_PROXY`, and `ALL_PROXY`.
- **Each CONNECT**, in order:
  1. `host:port` is matched against the job's list: a glob on the host, and the exact port.
  2. The name is resolved by DD5's public-only rule. The address classification moves from
     `theseus-core/src/web` to `theseus-tools`, so the wrapper can use it. A name that resolves to a private
     address is refused, so no metadata service and no localhost can be reached through the proxy either.
  3. The proxy connects, and copies bytes both ways.
  4. It logs the host, port, address, bytes each way, and milliseconds.

  A refused CONNECT gets a `403` whose body says why, and a `sandbox.egress_refused` row.
- **A program that ignores the proxy variables** has no route, and fails to connect. That is the contract's "no
  network by default".
- **What it records.** The completion's `detail.egress` lists the connections, and `sandbox.egress` rows are
  written, one per host per job. **A result whose job connected out has the `external` origin** (§2.6), so it
  latches its session, and theseus-20f is closed for L1.
- **Recognizing request shapes** (a receive-pack, a registry publish) would need TLS interception, and is
  filed. The list of hosts is M4's control.

**Credentials under L1 (decision 15).**
- **Spawn grants are unchanged.** `[broker.programs.<p>]` grants by direct argv, and the value crosses into L1
  as the job's environment. In L1 the grant is a boundary at last: no other job can read its
  `/proc/<pid>/environ` (it has its own pid namespace and proc), and gh's stored login is not in the view. This
  closes B1's two known gaps.
- **Run-time requests are an L1 feature.** Their identity is the socket.
  - The daemon creates `<spool>/broker/<corr>.sock` before the launch, and serves it until the job settles.
    The socket is bind-mounted into the job at `/run/theseus/broker.sock`.
  - Only that job's view contains the socket, so whoever connects is that job. At L0 any job could reach
    another job's socket path, so L0 keeps spawn grants only.
- **The helper** is this binary, bind-mounted read-only at `/run/theseus/bin/theseus-cred`, and `argv[0]`
  picks the role. `theseus-cred get <secret>` prints the value, for a script's `$(…)`. A git credential
  protocol form (a host mapped to a secret) is filed.
- **The judgment is decision 15's.**
  - The name must be a secret the broker may hand out: one with a grant or its own `[broker.secrets.<name>]`
    entry. Any other name is an error, as invalid input is (§3.9: only validation stops a call).
  - The posture is the stricter of the one the job's call ran at (`ran_at`) and the secret's own:
    - `open` grants;
    - `notify` grants, with a notice: "🔑 job a1b2c3 (`cargo publish`, L1) asked for `crates_io_token`:
      granted";
    - `approve` posts a card: "Job a1b2c3 (`cargo publish`, L1) asks for `crates_io_token`", answered as any
      approval is (`judge_act`: a trusted channel, and not a job's process).
  - The job blocks on its socket until the answer, within its own deadline. A decline, or a lapse, is an error
    the helper prints.
- **An ad hoc `op://` fetch, which decision 15 says always waits,** is filed with its design: the request names
  the reference and waits on a card; once approved, the daemon runs one `op read` and hands the value over,
  never keeping it.
- **Mechanism.** A request is a kernel action whose tool is `cred.request`, planned in the job's execution with
  the job's correlation id as its parent. The execution is waiting on the job, and no turn runs.
  - `open` and `notify` plan and authorize the action in one frame.
  - `approve` plans it with a confirm, and its card is an outbox post, as a call's card is. The budget question
    is the precedent for a question outside a tool call.
  - `Core::confirm_action` routes the answer, and the socket's handler waits on the action's settle.
- **Records.** The ledger gets `secret.requested` (the job, the secret, and the posture), then
  `secret.granted { via: request }` or `secret.declined`. The value is never written: it goes from the board,
  over the socket, into the helper's stdout.
- **In a latched session** (§2.6), the job's call already waited under T1 and was approved, so it ran at
  `approve`. By decision 15 its requests wait too. That is the literal rule, and it is consistent: the hold
  covers everything the latched session's jobs do.
- **The AWS seam.** The request carries a kind (`secret`, and later `aws`). The AWS lane's per-execution
  session credentials (a `credential_process` answer) arrive through the same socket, at the same posture
  rule.

### 2.5 Labels: one model for integrity and confidentiality

Each node written from M4 on carries one small label, set in the frame that writes the node and never
rewritten. The shape goes in `theseus-protocol`, so every surface reads the same thing:

```rust
pub enum Integrity { Trusted, Untrusted }       // M4. `Quarantined` comes with the Advisory (the reader rule).

pub enum Readers {                              // who may see it; the owner always may
    Public,                                     // anyone: a public page, a tree declared public
    Place(String),                              // whoever can view a guild channel: "discord:<channel id>"
    People(BTreeSet<String>),                   // named people, "discord:<user id>"; a DM is People{user}
    Owner,                                      // the owner alone
}

pub struct Label {
    pub integrity: Integrity,
    pub source: Option<ExternalText>,           // why untrusted: T1's shape, reused (tool, url, node, from, via)
    pub readers: Readers,
}
```

- **Integrity joins by the maximum.** `untrusted` wins.
- **Readers meet by intersection**, and are conservative wherever the intersection cannot be computed:
  - `Public ∧ x = x`, and `Owner ∧ x = Owner`;
  - `People(s) ∧ People(t) = People(s ∩ t)`, which is `Owner` when that is empty;
  - `Place(a) ∧ Place(b)` with `a ≠ b` is `Owner`;
  - `Place(a) ∧ People(s)` is `People(s ∩ viewers(a))` when the viewers are known, else `Owner`.
- **The owner** is the local operator (the CLI and the web UI) and the people in `[labels] owner`. That list
  defaults to `[approval] trusted_users` (§5, question 5).
- **A session's audience** comes from its place (`outbox.target`):
  - none, meaning the CLI or the web UI, or a task that reports nowhere: the owner;
  - a DM with `u`: `People{u}`;
  - a guild channel: its viewers, from the binding's `viewers.rs`. Without the Server Members intent they
    cannot be read, and the channel counts as `Public`, the worst case.
- **`covers(readers, audience)`** holds when every member of the audience is a reader or the owner. A node
  labeled `Place(c)` always covers place `c`'s own audience, whether or not its viewers can be read, since what
  was said in a channel may be said there again. Without that rule, a channel whose viewers are unknown would
  withhold its own messages from its own session.

**Who labels what, at write.**

| Node | Integrity | Readers |
|---|---|---|
| An operator's message, from the CLI or the web UI | trusted | `Owner` |
| An operator's message in a Discord DM with `u` | trusted | `People{u}` |
| An operator's message in guild channel `c`, from one of the binding's listed users | trusted | `Place(c)` |
| An attachment | As its message | As its message |
| The model's message, and its tool-call nodes | trusted: the origin is `agent`, and **never by exposure** | The meet of the readers of what the compile admitted, which the manifest records |
| `fs.*`, `git.*`, and `text.*` results | trusted, unless a file hash says otherwise (§2.6) | `Owner`, unless `[labels] public_paths` names the tree (a public repository) |
| `http.fetch` and `web.search` results | untrusted, with origin **`external { url }`** | `Public` |
| `proc.run` at L0 | trusted, unless `[policy] external_programs` names `argv[0]` (§5, question 4) | `Owner` |
| `proc.run` at L1 | untrusted, with origin `external { hosts }`, exactly when the job connected out (§2.4) | `Owner` |
| A task's brief, in the task | The parent's latch (T1's `via: task.create`) | The meet of the parent's context |
| A task's report, in the parent | The task's latch (T1's `via: task.report`) | The meet of the task's context |
| The harness's own lines (a wake, a notice) | trusted | The session's audience |
| Context files (in the system block; not nodes) | trusted | `Owner` by default. A `[context] files` entry may set `readers = "public"` |
| A graduated node (§2.7) | Its source's: graduation never touches integrity | The widened readers, with a warrant |

- **Old nodes** have no label. They read as their origin says (a result with DD5's `external` is untrusted),
  and as disclosable **only within their own session**. Old sessions compile only their own nodes, so nothing
  that works today changes.
- **Storage.** `Node.label` and the origin `external { source }` are NODE schema 3, with a reader for schema 2
  (§2.12). DD5's `ToolResult.external` stays readable, and new writes put the URL in the label's `source`.

### 2.6 Integrity: how the labels take in T1's hold (the key question)

**The problem.** T1's hold is session-level and sticky: "a result marked external entered the context". Appendix
F's integrity labels are node-level, inherited by transmission, with an exposure rule scoped to the compiled
context. Built side by side, they would be two systems that disagree. For example, a recompile drops the page
from the context: the labels say "nothing untrusted in play", while T1 says "held".

**The answer: one model, in three pieces, each with a single owner.**

| Piece | What it is | Owner | Relation to T1 |
|---|---|---|---|
| **The label** | Per node and immutable: integrity, with its source, set by origin and by transmission, never by exposure | Whoever writes the node (§2.5's table) | New. It is the data T1 lacked, and DD5's `external` marker becomes its first case |
| **The latch** | Per session: "this session admitted an untrusted node since the operator last trusted it" | The frame that admits the node | **It is T1's hold**: the same field (`SessionRecord.external`), the same `session.external_read` row, and the same write sites. Its input changes from "the result has `external`" to "the admitted node's label is untrusted" |
| **The gate step** | One step, `integrity::gate` (today's `external::gate`), after the whole order and the broker's posture | `toolrun.rs`'s `gate()` | T1's rule, unchanged: with the latch, every call that is not a `Read` waits (`[policy] external_text = ask`) or is notified (`notify`). `wake.at` is exempt (T1b), and reads keep their posture. Later rules only add to this step, and only stricter: the Advisory's quarantine (filed) and Jev's `security.v1` (M5) |

**So T1 stays the floor.**
- The floor is the latch, and only the operator's trust clears it: `policy.trust`, an approval with `trust`, or
  T1b's Discord `/trust`.
- Exposure's "only while that node is in the compiled context" scopes the Advisory's extra strictness, not the
  floor.
- **The latch stays sticky, on purpose.** The model may have acted on the text in ways the transcript no longer
  shows (a plan, a file, a task), and a recompile must never be a way to launder a hold.

**Transmission: every path the latch travels.** Each is deterministic, and each is written in the frame that
writes the transmitted node.

| Path | Today (T1) | With labels |
|---|---|---|
| A result from `http.fetch` or `web.search` | Latches | The same, by the `external` origin |
| A task's brief, from a latched session | Latches the task (`via: task.create`) | The same: the brief is untrusted by transmission |
| A report, from a latched task | Latches the parent (`via: task.report`) | The same |
| **A file** written by `fs.write`, `fs.edit`, or `fs.patch` in a latched session, then read by any session | Not caught | **Caught**: the write records the file's hash, and a read that matches it is untrusted (`via: file`) and latches the reader |
| **An L1 job that connected out** | Not caught (theseus-20f) | **Caught**: its result has the `external` origin (§2.4) |
| An L0 job whose program fetches (`gh issue view`) | Not caught (theseus-20f) | Caught only when `[policy] external_programs` names the program. It is empty by default (§5, question 4) |
| **A session opened from a latched job's process** | Not caught (theseus-d64) | **Caught**: J1's trace at `session.open` and `turn.submit` gives the new session the job session's latch (`via: job`). theseus-d64 folds into step 20a |
| A summary (M6's compaction) | — | The reader rule, for M6: a summary takes its inputs' integrity by `summarizes`, a transmission |
| A graduated node | — | Keeps its source's integrity |

**Why not by exposure.** The model's message in a latched session stays `trusted`, because the latch already
covers that session. Labelling every node the model writes after a read would saturate the graph, and Appendix
F rejected integrity inherited by exposure. The latch is how exposure is handled: once, at the level of the
session.

**Files carry the latch** (theseus-3vu's hashes, and Appendix F's fomites).
- **The write side.** `fs.write`, `fs.edit`, and `fs.patch` record the SHA-256 of the file's new content in
  the result's `meta`. In a latched session they also write `fomite:path:<canonical path>` and
  `fomite:sha:<sha256>` (META records: the latch's source, the session, the time, the size, and the mtime).
  This happens in the result's frame. A trusted session writes no fomite record.
- **The read side.** `fs.read` records its hash too.
  - **A path hit** counts when the file's size and mtime still match and its hash confirms it. It catches
    partial reads and `fs.grep` hits.
  - **A content hit** counts on a full read's hash. It catches a copy of the file.

  A hit makes the result untrusted (`via: file`, the path, and the session that wrote it), and the result's
  frame latches the reader.
- **What escapes, said plainly.** A file changed afterwards by `proc.run` has a new hash. An L0 program that
  reads the file itself is never seen. Promoting L1's scratch writes, when that is built, must write fomites
  for a latched job.
- **The cost** is one index read per `fs.read`, and SHA-256 over at most `[tools] max_read_bytes`. It is
  benched as part of step 20b (§2.10).

**The invariants, as tests.**
- **I1.** A session holds the latch exactly when it admitted an untrusted node, or a transmission of one, since
  its last trust. This is a property test in the simulator (19b).
- **I2.** T1's decisions are unchanged: its 13 tests (`tests_external.rs` and the unit tests) pass untouched,
  and are the floor's contract.
- **I3.** Nothing loosens a label. Only the operator's trust clears the latch, and graduation never changes
  integrity.
- **I4.** A latch is written in the frame that admits its node, so no crash leaves untrusted text in a context
  without its hold. This is T1's rule.

**What waits, with its design (filed P2, theseus-3vu).**
- The **Advisory**: an append-only node, with a level (`annotate`, `quarantine`, or `redact`) and a status chain
  (`open → traced → contained → closed`). Each status names its predecessor, and a stale one is refused.
  Quarantine makes the node's effective integrity `quarantined`, a projection.
- The compile records `quarantined_in_play`. While any quarantined node is in play, the same gate step makes
  **every** call one posture stricter, reads included (Exposure, §3.9). T1's floor is unchanged beneath it.
- `quarantined` joins `Integrity` only then, by the reader rule.

### 2.7 Confidentiality: audience-safe compilation, disclosure, and graduation

**Enforced at compile time** (§3.9: "Once private material is in the model's context there is no reliable
deterministic test of whether generated prose reveals it"). Each compile admits a node only when
`covers(node.readers, session.audience)` holds.

- **A withheld node keeps the request valid:**
  - a **tool result** keeps its `tool_use` pairing, as a placeholder result: `[withheld: fs.read's result is
    labeled owner-only, and this session's audience is #general (12 people). The operator can graduate it:
    theseus graduate trs_… --to place]`;
  - a **message** becomes one line that says so;
  - a **model message** can be withheld only when the audience grew after it was written. Its thinking is then
    dropped, at the recompile boundary where the provider allows it (§4.4);
  - a **context file** becomes its header with the reason: `# Context file (system): notes.md — withheld:
    owner-only, and this session's audience is #general`.
- **When it is decided.** An audience or label change in play is a deterministic recompile trigger (§4.4a
  already lists it).
  - The binding pushes a guild channel's viewer set to the core when it changes. It already re-checks viewers
    when a card posts.
  - A session whose admissions would change is marked `pending_recompile` with `audience` as the reason.
  - An append adds no other judgment: each new tail node gets the same one-comparison check as it is rendered.
- **What the manifest records:**
  - `audience`, as evaluated (the owner; `People{…}`; or a place and its viewers' count);
  - `readers`, the meet of what was admitted, which becomes the label of what the model writes next;
  - `integrity`, the latch and whether any untrusted node is in play;
  - `withheld`, the node ids with their reasons.

  `context.compiled` gets `audience` and `withheld: n`.
- **On Eddie's deployment nothing is withheld.** His DM, the CLI, the web UI, and #theseus-test (Eddie and the
  bot alone) all have Eddie as their whole audience. What changes for him is the manifest, the Observatory's
  badges, and one health line.

**Where content leaves: `labels::may_leave(readers, audience)`** (decision 14: it waits, and never refuses).
- **The first caller is the outbox.** Before a reply is posted, its label (its turn's manifest `readers`) is
  checked against the place's audience *now*.
  - If the audience no longer fits, because the channel gained a viewer since the compile, the post is held as
    a card to the owner: "This reply draws on material labeled owner-only, and #general's audience changed (now
    13 people). Post it?"
  - Approve posts it. Decline leaves a note in the place: "a reply was held back".
  - On the happy path it never fires.
- **Later callers** use the same function and the same rows: MCP responses (M7), posts to another channel and
  gliding (M7), and any typed push or PR tool.
- **Not in M4:** a fetch's URL (T1's accepted gap, which Eddie kept on 2026-09-30: "Yes, keep it!"), and
  `proc.run`'s arguments. The environment and the egress list are the controls for `proc.run`.

**Graduation: the only way an audience widens** (Appendix F: "relabelling in place is not an operation").
- **How:**
  - `theseus graduate <node> --to public|place|people:<ids> --why "<warrant>"`;
  - the web UI's **Graduate** button on a withheld placeholder;
  - protocol `label.graduate`.
- **Judged by `judge_act(Act::Graduate)`**: a trusted channel, and not a job's process, as a trust is.
- **What it writes.** A new node: origin `operator`, `graduated_from: <node>`, and a warrant (who, how, why, and
  when). Its readers are widened, and its integrity is the source's. It is ledgered as `label.graduated`.
- **When it takes effect.** The graduated node is in the session's tail, so the next compile admits it, as an
  append. The placeholder stays where it was, since its pairing needs it.

**Disclosure tests in the simulator** (step 19b; P6: "private material never reaches a public audience's
context").
- `theseus-sim disclosure --seed N --steps M` builds a synthetic world:
  - people, and guild channels whose viewer sets change;
  - sessions bound to DMs and channels;
  - operator messages, owner-only and public tool results, and fetches;
  - tasks with briefs and reports;
  - context files, graduations, and a test-only "foreign node" input, which stands in for M6's recall so the
    filter is exercised before M6 exists.
- After every compile and every post, it asserts:
  - every admitted node covers its audience;
  - every post covers its place's audience at the moment it is posted, or it was held;
  - every `tool_use` is still paired;
  - the latch invariant I1 (§2.6) holds.
- A short set of seeds (about 2 s) runs in the gate, and longer runs in review. A seeded failure reproduces
  exactly, as `kernel-sim`'s do.

### 2.8 The ontology's first slice

**What M4 builds** (§4.1a's M4 row): the kinds table, declared memberships, guidance, and the compile walk,
with topic as the first new kind.

**The kinds table** is data: store records, versioned, with `added_by` on each row. M4 seeds only the rows it
has a reader for:

| Kind | Given or interpreted | Membership comes from | Per session | Parent kind | Precedence | Rule |
|---|---|---|---|---|---|---|
| `guild` | given | The transport (the place's guild) | 1 | — | 10 | `chain` |
| `channel` | given | The transport (the place) | 1 | `guild` | 20 | `chain` |
| `person` | given | The transport (the DM's user; a channel's listed users) | many | — | 30 | `intent_line` |
| `topic` | interpreted | The operator in M4; `jev` in M5; `sweep` and `dream` in M6 | 3 | `topic` (topics nest) | 40 | `chain` |

- **`culture` and `expertise`** (§4.1a's other seed rows) are added with their reader, Jev (M5).
- **Composition rules.** M4 implements `chain` and `intent_line`. A row naming `ranked` or `recall_only` is
  refused until M6 builds them (the reader rule), and the error says so.

**Data shapes.** They live in a new crate, `theseus-ontology` (LANE: pure types, validation, and composition).
The core stores them as META records, with a ledger row per change:

| Record | Key | Holds |
|---|---|---|
| Kind row | `onto:kind:<name>` | The table's columns, `version`, `added_by` |
| Category | `onto:cat:<id>` | The kind, name, parent, description, `added_by`. Given kinds' categories are made from the transport at a place's first bind |
| Membership (interpreted) | `onto:member:<session>:<kind>` | The current list, each entry with `origin`, `confidence`, and `as_of`. A change is a new record that supersedes the old (the WAL keeps the history) |
| Guidance | `onto:guide:<category>` | Text, `version`, digest, `added_by` |

- Given memberships (guild, channel, person) are never stored per session. They are read from the session's
  place at compile, with origin `transport`. By the guardrail they cannot be re-associated: setting one through
  the API is an input error.

**The compile walk.** It runs only when a compilation is made, never on an append.
1. Take the session's memberships: given, from its place; interpreted, from the in-memory snapshot.
2. For each, walk the category's parents. `chain` admits the farthest ancestor's guidance first, so the
   nearest comes last. `intent_line` admits one line per category.
3. Order the kinds by precedence, lowest first, so a higher kind's guidance comes later and governs.
4. Render the result into the system block, after the persona's files, under headers such as
   `# Guidance (topic theseus › rust-harness)`.

- **The manifest records** every membership used (the kind, category, origin, and `as_of`) and each block's
  digest. "Why did it know that?" always has an answer.
- **Memberships wait for the next recompile** (§4.1a), so the prompt cache survives. While a session appends,
  it uses the memberships its manifest recorded; a recompile, for any reason, takes the current ones.
  `theseus session recompile` applies a change at once.
- **A guidance edit in play is one `system_changed` recompile**, as a context file's edit is.
- **FAST.** The snapshot is built lazily after serving, by one META prefix scan, and is kept current in memory
  on every write. A compile reads memory only: no embedding search, no Jev, no store read (§4.1a).

**The guardrails, in code.**
- *Given versus interpreted*: only interpreted kinds take API writes.
- *Interpretations route context and never grant access*:
  - the admission filter (§2.7) runs before guidance, and never reads a membership;
  - the ontology crate does not depend on labels or policy;
  - a test compiles the same session with and without memberships, and asserts identical admissions.

**Writes are judged.** Guidance steers every session in its category, so a job's process that writes it would
be an injection path. Ontology writes therefore go through `judge_act(Act::Ontology)`, with the answers' rule
(a trusted channel, and not a job's process).

**Surfaces.**
- CLI: `theseus ontology kinds`, `categories`, `topic add <name> [--parent p] [--desc …]`, `guide <category>
  -` (from stdin), and `member <session> +topic -topic`.
- Protocol: `ontology.list`, `ontology.category.add`, `ontology.guidance.set`, `ontology.membership.set`.
- The web UI's Ontology view (LANE, 21c): the kinds, a category tree, a guidance editor, and each session's
  memberships.
- The Observatory's session view: memberships, with their origin and as-of.
- Discord's `/topic` is filed.

### 2.9 Control-plane separation (an installer option)

**The option** (§1): the runtime and its storage run under their own OS identity, `theseus`, and L0 jobs run as
the operator. It is strongly recommended, and never the default.

**What it buys.** The floor becomes the OS's.
- An L0 job, as the operator's uid, cannot read or write the store, the WAL, the config copy, the bindings file,
  the tightenings, or the vault token, even if the operator approves a floor call by mistake.
- `op` runs as `theseus`.
- The socket is reachable by a group, and J1's refusal of a job's answer still holds on it.

**The layout.**

| Path | Owner and mode | Holds |
|---|---|---|
| `/var/lib/theseus/` | `theseus:theseus` 0700 | The store (WAL, index, blobs), `config.last-good.toml`, and `bindings.toml` |
| `/etc/theseus/op-token` | `theseus` 0600 | The 1Password service-account token (`THESEUS_OP_TOKEN_FILE`) |
| `/usr/local/lib/theseus/theseusd` | `root` 0755 | The binary. It is part of the floor, so the operator cannot replace it |
| `/run/theseus/theseus.sock` | `theseus:theseus-ops` 0660 | The protocol socket. The operator is in `theseus-ops` |
| `/run/theseus/jobs.sock` | `theseus:theseus-ops` 0660 | The job host's connection |
| `~operator/.local/state/theseus/spool/` | the operator, 0700 | The job wrappers' spool: results, completions, and pids |
| Units | `/etc/systemd/system/theseusd.service` (`User=theseus`, `Delegate=yes`); `~/.config/systemd/user/theseus-job-host.service` | |

**The job host** (`theseusd job-host`, the operator's user service; the same binary in a role).
- **Connection.** It connects to `/run/theseus/jobs.sock`. The daemon admits it only when its `SO_PEERCRED` uid
  is `[control_plane] operator_uid`.
- **`launch { WrapperArgs }`.** The environment, granted secrets included, exists only in memory. The host
  calls `spawn_detached` as today, and returns the pid.
- **`terminate { corr }`** runs §2.3's stop, and **`evidence { corr }`** answers the reconciler.
- **It relays completions.** Its wrappers poke its own notify socket. It sends each completion to the daemon,
  and deletes the file only after the daemon's ack, which comes once the frame is committed. That is the
  spool's crash rule, carried across a socket.
- **It reaps.** It is a child subreaper, and reaps its wrappers with Z1's registry.
- **The daemon's side** is a `RemoteLauncher: JobLauncher`, the seam that exists, and `terminate_all` goes
  through it.
- **With no job host connected**, a job does not run. Its result says "the job host is not connected; the call
  did not run", and health says `job host: disconnected`.
- **L1 under separation** is unchanged: the host runs the sandbox as the operator, unprivileged.

**Two of J1's rules must follow the split** (found reading `peer.rs`).
- **The web UI's trace breaks across users.** J1 finds the browser behind a web answer by reading
  `/proc/<pid>/fd`, and "only this account's processes can be read" (`peer.rs:385`). As `theseus`, the daemon
  cannot trace the operator's browser, so every web answer would be refused as untraceable.
  - The fix, under separation only: the web UI's judged acts need a login minted through the traced socket.
    `theseus web` asks the socket for a one-time code and opens the UI with it, and the login cookie is bound
    to that code.
  - The socket's trace reads only command lines and `stat`, which any user can read, so it works across users.
- **An orphan under the job host** must be refused, as one under the daemon is. `job::daemon_in_cmdline`
  recognizes a serving `theseusd` by its having no subcommand, so `theseusd job-host` must be added
  (`asker.under_job_host`).

**The installer: `theseusd install`.** It prints a plan by default, and `--apply` performs it. It is
idempotent, and logs every action. `--check` compares the machine with the layout, and prints what differs.

| Mode | Needs | What it does |
|---|---|---|
| `--user` | Nothing but the operator | Writes `~/.config/systemd/user/theseusd.service` with `Delegate=yes`, so L1 gets cgroup limits (§2.2). The state dir stays where it is |
| `--separate` | root | Creates the `theseus` user and the `theseus-ops` group, adds the operator to the group, makes the layout above, and installs the binary and both units. It writes a generated config's paths and `[sandbox] default = "l1"`. `--migrate-state <dir>` copies a stopped daemon's store (the F4 rules: never deleted, never written by an older build) |

- **The vault note stays the config's source.** Only the token file moves.
- **Proof.** Sudo here needs a password, so the chain proves `--separate` in a throwaway container. Eddie's
  user is in the `docker` group. The container runs with `--network none`, the release binaries mounted
  read-only, and the fake model and fake `op` from the test rigs. So no real secret enters it (§4, item 5).

### 2.10 FAST: nothing new before serving, and every per-job cost benched

| New work | When it runs | On the start path? | Bench row, and its target |
|---|---|---|---|
| The L1 host probe | After serving, in the background, once per image | No | `sandbox.probe`'s milliseconds, in health |
| An L1 job's setup | At dispatch | No | `theseus-sim bench jobs --class l0\|l1`: from dispatch to the command's exec, p95 under 25 ms for L1 and no worse than today for L0 |
| The cgroup leaf and the daemon's move into it | Lazily, at the first L1 job | No | — |
| The egress proxy | Per connection, in the wrapper | No | A CONNECT's first byte against a direct connection: under 2 ms added |
| A stop by tree walk or pid namespace | At a cancel or a deadline | No | From the cancel to verified, per backend, in the step's report |
| Labels at write | Each node's frame | No | The frame budget test holds at 8. A label adds about 40 bytes to a node |
| The compile filter and the audience cache | Each compile and append | No | A compile of 1,000 nodes: under 50 µs added |
| File hashes and the fomite index | `fs.read`, `fs.write`, `fs.edit`, `fs.patch` | No | SHA-256 of a 2 MiB file plus one index read: p95 under 5 ms |
| The ontology snapshot | Lazily after serving, by one META prefix scan | No | A scan of 10,000 records under 20 ms, off the start path. The compile walk reads memory only |
| The schema bumps (NODE 3; COMPILATION 3 and 4; ACTION 3 and 4) | A write takes the new layout, and old ones are read in place | No: F4a's open is unchanged | The lifecycle bench, unchanged |
| The job host | It connects after serving, and the daemon never waits for it | No | The lifecycle bench, run as the separated daemon in the container, within §9 |

- The gate's lifecycle bench (cold start, clean stop with a job, SIGKILL and restart, and F4b's swap and restore)
  holds at every step, as it does today.
- A step that finds itself adding to the start path stops and moves the work after serving (`after_serving`),
  as F1 taught.

### 2.11 EXQUISITE VISIBILITY: where each piece shows

| Piece | Discord | Web UI and Observatory | CLI | Ledger | Narrative | Telemetry and health |
|---|---|---|---|---|---|---|
| L1 | The tool line gets `🛡️ L1`, then "no network", or "egress: github.com", and what it wrote to scratch | An L1 pill on the call. A Sandbox section: the probe, the jobs by class, the cgroup mode, egress and refusals, and limit hits | `theseus health`'s `sandbox:` line; the notice line names the class | `sandbox.started` (class, limits, egress list), `sandbox.limit_hit` (pids, memory, fsize, from `pids.events` and `memory.events`), `sandbox.probe` | "proc.run `cargo test` ran in L1: no network, 2 GiB, 512 pids; 3 files to scratch, discarded" | `theseus.job.start_ms{class}`, `theseus.sandbox.limit_hits{limit}` |
| Cancellation | `⏹️ stopped (verified)` or "(not verified: why)" | The execution's actions show `verified_by` and `survivors` | `theseus executions` | `action.cancel_verified`, `_unsupported`, and `_uncertain` | "Stopped job a1b2c3: its pid namespace (4 processes) is gone" | `theseus.cancel{backend,state}` |
| Egress | The tool line lists the hosts reached | Egress per job, with bytes | The completion's `egress` in `executions --json` | `sandbox.egress`, `sandbox.egress_refused` | "reached github.com:443 (2 connections)" | `theseus.sandbox.egress.bytes{host}` |
| Credential requests | `🔑 job a1b2c3 asked for crates_io_token: granted`, or a card when it waits | The card, and the Secrets field | `confirm` and `watch` show the request | `secret.requested`, `secret.granted { via: request }`, `secret.declined` | "Job a1b2c3 asked for crates_io_token, at proc.run's notify" | Health's `broker[]` counts requests |
| Integrity | T1's lines and buttons, unchanged; the reason names `via: file`, `via: job`, or egress | The External text section becomes "Integrity": holds, their paths, and untrusted nodes badged 🌐 in the transcript | `theseus node <id>` shows the label and its source | `session.external_read` (with the new `via` values), `fomite.recorded`, `session.trusted` | T1's lines, plus "this session read a file that session s1 wrote after reading external text" | `theseus.integrity.latched` (a gauge of sessions) |
| Confidentiality | A held post's card | A 🔒 or 👥 badge on each node; the manifest's audience; Graduate on a placeholder | `theseus labels <session>`, `theseus graduate` | `label.withheld` (once per compile, with counts), `label.graduated`, `label.held_post` | "Withheld 2 owner-only results: this session's audience is #general" | `theseus.compile.withheld` |
| Ontology | (`/topic` is filed) | The Ontology view; a session's memberships | `theseus ontology …` | `ontology.kind`, `ontology.category`, `ontology.membership`, `ontology.guidance` | "Topic set to theseus › rust-harness; it applies at the next recompile" | — |
| Control plane | — | Health's `control_plane` card | `theseus health`: `control plane: separated (daemon uid theseus; job host uid 1000, connected)` | `jobhost.connected`, `jobhost.lost` | "The job host connected" | `theseus.jobhost.connected` (a gauge) |

### 2.12 Catalog: config, protocol, store, and ledger

**Config.** Each key lands with the code that honors it, and goes into the tested template.

| Key | Default | Step |
|---|---|---|
| `[sandbox] default` | `"l0"` built in; the installer writes `"l1"` | 17b |
| `[sandbox] l1_argv` | `[]` | 17b |
| `[sandbox] ro_paths` | The system directories | 17b |
| `[sandbox] memory_mb`, `pids`, `scratch_mb`, `output_mb` | 2048, 512, 1024, 64 | 17b |
| `[sandbox] egress` | `[]` | 18c |
| `[labels] owner` | `[approval] trusted_users` | 19a |
| `[labels] public_paths` | `[]` | 19a |
| `[context] files` entries' `readers` | `"owner"` | 19a |
| `[policy] external_programs` | `[]` | 20a |
| `[control_plane] operator_uid`, `jobs_socket` | Absent, meaning not separated | 22b |

**Protocol** (`theseus-protocol`).
- New fields:
  - `ToolStarted.class`, `egress`, and `limits`;
  - the action's `cancel.verified_by` and `survivors`;
  - `Node.label`;
  - the manifest's `audience`, `readers`, `integrity`, `withheld`, and `memberships`;
  - health's `sandbox`, `control_plane`, and `integrity` (holds);
  - `ExternalText.via`'s new values: `file`, `job`, and `egress`.
- New methods: `label.graduate`, `ontology.list`, `ontology.category.add`, `ontology.guidance.set`,
  `ontology.membership.set`, and `web.login_code` (under separation).
- New `judge_act` acts: `Graduate` and `Ontology`.

**Store** (F4a's standing rule: each bump lands with its reader and a test that reads the old layout).

| Kind | Schema | Why, and which step |
|---|---|---|
| NODE | 2 → 3 | `label`, and the `external` origin (19a) |
| SESSION | 2 (unchanged) | The latch keeps T1's field; `via` gets new values of an existing string |
| COMPILATION | 2 → 3 → 4 | The manifest's `audience`, `readers`, `integrity`, and `withheld` (19a), then `memberships` (21b) |
| ACTION | 2 → 3 → 4 | `cancel.verified_by` and `survivors` (18a), then the `cred.request` action's parent (18d) |
| META | 1 (unchanged) | `fomite:*` and `onto:*` keys are new keys, not a new layout |

No new record kind is needed. The ontology and the fomites are META keys, and a credential request is an
ACTION.

**Ledger rows:** `sandbox.started`, `sandbox.probe`, `sandbox.limit_hit`, `sandbox.egress`,
`sandbox.egress_refused`, `action.cancel_verified`, `action.cancel_unsupported`, `action.cancel_uncertain`,
`secret.requested`, `secret.declined`, `fomite.recorded`, `label.withheld`, `label.graduated`,
`label.held_post`, `ontology.kind`, `ontology.category`, `ontology.membership`, `ontology.guidance`,
`jobhost.connected`, and `jobhost.lost`.

## 3. The build plan

Sixteen steps. Each is about an hour of one agent, and each is proved live on a scratch daemon over a copy of
Eddie's store, with Discord and the web UI off unless the step needs them.
- **SPINE** steps touch the kernel, the store, the core's gate or turn loop, or the protocol. They run one at a
  time on `main`.
- **LANE** steps live in their own crate or binary. They are built in a worktree, with its own
  `CARGO_TARGET_DIR` (per the agents' operating notes for this repo: a shared `target/` poisons the main tree), and are joined by the next
  SPINE step.

### The order at a glance

| Step | Roadmap | Kind | Builds | Depends on |
|---|---|---|---|---|
| 17a | 17 | LANE | `theseus-sandbox`: the namespaces, the view, seccomp, capabilities, the init; §7's contract tests | — |
| 17b | 17 | SPINE | L1 for `proc.run`: the class choice, `[sandbox]`, the wrapper's L1 path, the probe, limits, surfaces | 17a |
| 18a | 18 | SPINE | Cancellation verified per backend: the tree stop, pid namespace, cgroup, async abort, `verified_by` | 17b; fix batch 1 (theseus-w98) |
| 18b | 18 | LANE | The egress proxy in `theseus-sandbox`: the listener handoff, CONNECT, the allowlist, the public-only resolver | 17a |
| 18c | 18 | SPINE | Egress wired in: `[sandbox] egress`, a call's extra hosts wait, `detail.egress`, a result that connected out is external (T1 latches it) | 17b, 18b |
| 18d | 18 | SPINE | Credential brokering under L1: the per-job socket, `theseus-cred`, the `cred.request` action, decision 15 | 17b, 18c |
| 19a | 19 | SPINE | Labels on nodes, the audience, the compile filter with placeholders, the manifest, context files' readers | T1; F4a's rules |
| 19b | 19 | LANE | The disclosure simulator in `theseus-sim` | 19a |
| 19c | 19 | SPINE | Graduation, and `may_leave` in the outbox (a held post) | 19a |
| 20a | 20 | SPINE | Integrity by labels: the latch fed by labels, origin `external`, `external_programs`, theseus-d64 folded in | 19a; T1b (theseus-q4t) |
| 20b | 20 | SPINE | File hashes and fomites: `via: file` | 20a |
| 21a | 21 | LANE | `theseus-ontology`: the kinds table, validation, `chain` and `intent_line`, the seed rows | — |
| 21b | 21 | SPINE | The ontology wired in: store records, the snapshot, judged writes, the compile walk, CLI, Observatory | 21a, 19a |
| 21c | 21 | LANE | The web UI's Ontology view | 21b's protocol |
| 22a | 22 | LANE | `theseusd install`: plan, apply, check; `--user` and `--separate`; the container test script | — |
| 22b | 22 | SPINE | The job host, `RemoteLauncher`, the web login code, `job-host` in J1's walk, `[control_plane]` | 22a, 18a |

- **The spine's order:** 17b, 18a, 18c, 18d, 19a, 19c, 20a, 20b, 21b, 22b.
- **The lanes can start at once:** 17a first, since 17b waits on it, then 21a and 22a beside it. 18b follows
  17a's API, 19b follows 19a, and 21c follows 21b.
- **18a's L0 half does not need L1**, and can run before 17b if the spine is idle.
- **Store bumps by step:** ACTION 3 (18a) and 4 (18d); NODE 3 (19a); COMPILATION 3 (19a) and 4 (21b). Each
  lands with its reader and an old-layout test (F4a).

### Each step: its tests and its live check

**17a, the sandbox crate (LANE).**
- *Builds:* `crates/theseus-sandbox`:
  - `spawn(spec, stdio) -> SandboxChild`, for the wrapper, and `init_main(args) -> !`, for the
    `job-sandbox` role;
  - the two user namespaces' maps, the view (§2.2's table), `pivot_root`, the seccomp filter (`seccompiler`,
    or a hand-built BPF), the dropped bounding set, and `no_new_privs`;
  - the init's reap, signal forwarding, and exit, and its scratch summary over a pipe.
- *Tests:* `tests/contract.rs`, with `harness = false` so the test binary can re-exec itself as the init and
  as the probe. **One test per §7 clause** (§2.2's table), and a spawn micro-bench: 100 spawns, p50 and p95.
- *Live check:* the contract suite on this machine's WSL2 kernel (6.18). The report lists each clause's
  result, and the spawn timing.
- *Depends on:* nothing.

**17b, L1 for `proc.run` (SPINE).**
- *Builds:*
  - the `[sandbox]` config (`default`, `l1_argv`, `ro_paths`, and the limits) in the tested template;
  - `proc.run`'s `sandbox` input;
  - the class in the gate record and in the digest a confirm binds;
  - `WrapperArgs.sandbox`, and the wrapper's L1 path;
  - the hidden `theseusd job-sandbox` role;
  - the probe after serving, and health's `sandbox`;
  - the lazy cgroup leaf and the limits, where the cgroup is delegated;
  - `detail.scratch`;
  - the surfaces: Discord's `🛡️ L1`, the web UI's pill, the CLI's line, `tool.started.class`,
    `sandbox.started`, and the narrative.
- *Tests:* in `theseusd/tests/sandbox.rs`, with a real daemon and the stand-in model:
  - a `sandbox: true` probe script's result shows the operator's uid, zero capabilities, an empty HOME, no
    route out, and gh not logged in;
  - `l1_argv` routes a call to L1;
  - with L1 made to fail, the call fails with the reason, and never runs at L0;
  - an approved L1 call cannot be dispatched as L0 (the digest);
  - the template test;
  - the jobs bench row.
- *Live check:* a scratch daemon over a copy of Eddie's store, with his note.
  - A GLM turn runs `proc.run { sandbox: true }` of a probe script, and its result shows the contract.
  - An L0 run in the same session shows the contrast.
  - Health's `sandbox:` line and the ledger's rows are shown.
- *Depends on:* 17a.

**18a, cancellation verified per backend (SPINE).**
- *Builds:*
  - the wrapper's SIGTERM handler and tree stop (stop, rescan, kill, reap), with its verdict in the completion,
    also used on its deadline;
  - L1's stop through the init;
  - `cgroup.kill` where there is a cgroup;
  - the async tools' abort;
  - the driver signals the wrapper alone and reads its verdict, and falls back to the group kill for an older
    wrapper;
  - `verified_by` and `survivors` (ACTION 3), the rows, and the surfaces.
- *Tests:*
  - a kernel test binary of its own (it forks trees, as `children.rs`'s does): `sh -c 'setsid sleep 300 & exec
    sleep 300'` is cancelled, and a `/proc` scan finds no sleeper; the same through the deadline; an older
    wrapper's fallback;
  - a daemon test of the L1 row;
  - a core test of the async abort.
- *Live check:* GLM runs a job that `setsid`-forks a sleeper. `theseus executions cancel` says it was verified,
  with counts, and a `/proc` scan agrees. `theseus stop` does the same.
- *Depends on:* 17b for the L1 row; fix batch 1's theseus-w98.

**18b, the egress proxy (LANE).**
- *Builds:*
  - in `theseus-sandbox`: the listener opened inside the namespace and handed to the wrapper over
    `SCM_RIGHTS`, the CONNECT server, and matching against the allowlist;
  - DD5's address classification, moved into `theseus-tools` with a test that keeps the two in step.
- *Tests:*
  - an allowed host is reached, and a host off the list gets a 403 with its reason;
  - a name that resolves to 127.0.0.1 or 169.254.169.254 is refused at connect;
  - bytes and timing are logged;
  - a program without the proxy variables has no route.
- *Live check:* the crate's suite on this machine, and a `curl`-free probe: a small Rust test client through
  the proxy to a public host on the list.
- *Depends on:* 17a.

**18c, egress wired in (SPINE).**
- *Builds:*
  - `[sandbox] egress`, and a call's `sandbox.egress`;
  - the gate's step 2 for hosts beyond the list, whose approval reaches only the named hosts;
  - `detail.egress`, and the `sandbox.egress` rows;
  - a result whose job connected out gets DD5's `external` marker, so T1 latches its session (theseus-20f is
    closed for L1).
- *Tests:*
  - a daemon test through a local stand-in host (DD5's `Dns.hosts` pattern);
  - a latched session after an L1 job's egress;
  - a call naming an extra host waits.
- *Live check:* GLM runs `proc.run { sandbox: { egress: ["api.github.com:443"] } }` of a small fetch script.
  - It connects, and the session holds external text, with the reason naming the egress.
  - An unlisted host is refused, with its reason in the result.
- *Depends on:* 17b, 18b.

**18d, credential brokering under L1 (SPINE).**
- *Builds:*
  - the per-job socket, served by the daemon and bind-mounted into the job;
  - the `theseus-cred get` helper;
  - the `cred.request` action with its parent (ACTION 4);
  - decision 15's posture rule, with the secret's stricter posture winning;
  - the card, the notice, and the rows.
- *Tests:*
  - `open`, `notify`, and `approve` each behave as decision 15 says;
  - an `approve` secret waits even under an `open` call;
  - a decline returns an error to the helper;
  - an unknown name is an input error;
  - the value is in no file under the state dir, nor the log (B1's check);
  - a job's process cannot answer the card (J1).
- *Live check:* a GLM job in L1 runs `gh api user` with `GH_TOKEN="$(theseus-cred get github_token)"` and gh's
  stored login hidden. It answers `zeroaltitude`, with a `🔑` notice. The token is counted absent from the
  store and the log, as in B1's review.
- *Depends on:* 17b, 18c.

**19a, confidentiality labels (SPINE).**
- *Builds:*
  - `Label` on nodes (NODE 3), and the origin rules of §2.5's table;
  - the audience, and the binding's push of a channel's viewer set;
  - the compile filter, with pairing-preserving placeholders;
  - the manifest fields (COMPILATION 3), and `context.compiled`;
  - the audience recompile trigger;
  - context files' readers, and `[labels]`;
  - the Observatory's badges, and `theseus labels`.
- *Tests:*
  - a session in a synthetic two-viewer channel withholds an owner-only result, with its `tool_use` still
    paired;
  - a DM admits everything;
  - an audience change recompiles;
  - a context file is withheld with its reason;
  - F4a's old-store fixture compiles exactly as before;
  - the frame budget holds at 8.
- *Live check:* on Eddie's store copy, his DM compiles with nothing withheld, and its manifest reads
  `People{eddie}`. With the fake Discord, a synthetic channel whose member list has a second user withholds an
  owner-only `fs.read`. The step adds the members endpoint to the fake if it lacks one.
- *Depends on:* T1 (built), F4a's rules.

**19b, the disclosure simulator (LANE).**
- *Builds:* `theseus-sim disclosure --seed --steps` (§2.7), over the core as a library, with a short run in
  `scripts/gate.sh`.
- *Tests:* the simulator itself. It also plants a bug on purpose: a test build whose filter skips attachments
  must fail within the seed set.
- *Live check:* 40 seeds of 2,000 steps. The report has the counts of what was compiled, withheld, held, and
  graduated.
- *Depends on:* 19a.

**19c, graduation and the held post (SPINE).**
- *Builds:*
  - `label.graduate`, with `judge_act(Graduate)`, the graduated node, and its warrant;
  - `theseus graduate`, and the web UI's button;
  - `labels::may_leave` in the outbox, with the held post's card, and its approval or decline.
- *Tests:*
  - a graduated result is admitted at the next compile;
  - a job's process cannot graduate;
  - a post whose place gained a viewer is held, and approving it posts it.
- *Live check:* on the fake Discord rig of 19a, graduate the withheld result. Then grow the channel's audience
  mid-turn, and the reply is held, then approved.
- *Depends on:* 19a.

**20a, integrity by labels (SPINE).**
- *Builds:*
  - origin `external { source }`;
  - `external::gate` becomes `integrity::gate`, and the latch's three sites read `label.integrity`;
  - `[policy] external_programs`;
  - a session opened, or a turn submitted, from a latched job's process takes the latch (`via: job`), which
    closes theseus-d64.
- *Tests:*
  - **T1's 13 tests, untouched, pass**;
  - an `external_programs` job latches;
  - a job-opened session latches;
  - the L1 egress latch of 18c now comes through labels.
- *Live check:* T1's own live check, repeated word for word: a fetch, then `proc.run echo hi` waits with the
  same reason, and trust clears it. Then a job's `theseus ask` opens a session that holds the latch.
- *Depends on:* 19a, and T1b (so the unified step keeps `wake.at`'s exemption).

**20b, file hashes and fomites (SPINE).**
- *Builds:*
  - SHA-256 on `fs.read`, `fs.write`, `fs.edit`, and `fs.patch` (the result's `meta`);
  - `fomite:*` records written by a latched session's writes;
  - the read side's path and content lookups, and `via: file`.
- *Tests:*
  - a latched session's file latches a clean reader, whose reason names the file and the session;
  - a trusted session's write records nothing;
  - a file changed since does not match;
  - a full-read copy matches;
  - the bench row.
- *Live check:*
  1. Session A fetches a page, and writes `notes.md` (approved).
  2. Session B reads `notes.md`, and its next `proc.run` waits: "… a file (notes.md) that session a1b2c3
     wrote after it read external text …".
- *Depends on:* 20a.

**21a, the ontology crate (LANE).**
- *Builds:* `crates/theseus-ontology`:
  - the types, and the kinds table's validation (closed rules, with `ranked` and `recall_only` refused until
    built);
  - the four seed rows, and the category tree;
  - `compose()` for `chain` and `intent_line`, and the manifest entries.
- *Tests:* golden renders; the precedence order; a cycle in the parents refused; a row naming an unbuilt rule
  refused.
- *Live check:* none. It is a pure crate, and 21b proves it.
- *Depends on:* nothing.

**21b, the ontology wired in (SPINE).**
- *Builds:*
  - the META records, and the lazy snapshot;
  - the protocol's methods, with `judge_act(Ontology)`;
  - the compile walk, at recompile only, with the manifest's `memberships` (COMPILATION 4);
  - `theseus ontology …`, the Observatory's memberships, and the rows.
- *Tests:*
  - a topic's guidance is in the system block, under its header;
  - a membership change waits for the next recompile;
  - a guidance edit forces one `system_changed`;
  - given memberships refuse writes;
  - admissions are identical with and without memberships;
  - a job's process cannot write guidance.
- *Live check:* on Eddie's store copy:
  1. Add the topic `theseus`, with a line of guidance.
  2. Assign it to his DM session, and run `theseus session recompile`.
  3. The next manifest lists the membership, and GLM's answer follows the guidance.
- *Depends on:* 21a, and 19a (the manifest's bump comes first).

**21c, the web UI's Ontology view (LANE).**
- *Builds:* in `web/`: the kinds, the category tree, a guidance editor, and a session's memberships, over 21b's
  methods.
- *Tests:* the web lint and build in the gate, and the web app's own tests.
- *Live check:* on a scratch daemon, with its web UI on a port other than 7433: add a topic and guidance from
  the browser, and see them in `theseus ontology`.
- *Depends on:* 21b's protocol.

**22a, the installer (LANE).**
- *Builds:* `theseusd install` (a module in `theseusd`): plan, `--apply`, `--check`, `--user`, and
  `--separate`; and `scripts/install-test.sh`, which runs `--separate` in a throwaway container.
- *Tests:*
  - the plan's paths, modes, and unit files;
  - a second plan after an apply is empty (idempotent);
  - `--check` names a wrong mode.
- *Live check:* `--user` in plan mode on this machine, and applied with a scratch `HOME`. The container run of
  `--separate`, if Eddie allows Docker for it (§4, item 5).
- *Depends on:* nothing.

**22b, the job host (SPINE).**
- *Builds:*
  - `theseusd job-host`, and `RemoteLauncher`;
  - the completion relay (deleted after the daemon's ack);
  - terminate and evidence through the host;
  - `[control_plane]`;
  - the web login code;
  - `job-host` in J1's walk;
  - health's `control_plane`.
- *Tests:*
  - daemon tests with a job host as the same user: every job goes through the host, a restart of either side
    loses nothing, and a cancel is verified through the host;
  - the two-uid behavior in the container script.
- *Live check:* in the container:
  - the daemon runs as `theseus`, and the host as the operator;
  - a job's `id -u` is the operator's, and its `ls /var/lib/theseus` gets `EACCES`;
  - a cancel is verified;
  - a web answer counts with the login code, and without it is refused;
  - the lifecycle bench is within §9.
- *Depends on:* 22a, 18a.

### Dependencies on other phases

| Direction | Phase and step | What |
|---|---|---|
| Before M4 | T1b (roadmap 4b) | `wake.at`'s exemption, and Discord's `/trust`. 20a keeps both |
| Before M4 | Fix batch 1 (roadmap 5) | theseus-w98, a cancel that leaves a planned call planned, before 18a |
| Before M4 | theseus-e89 (with T1b) | Makes #theseus-test safe, so any step can check its Discord lines on real Discord |
| Beside M4 | The AWS lane (roadmap 14 to 16) | Little overlap: the tender ships WAL segments whatever their records hold, so M4's schema bumps do not touch it. The AWS lane's credentials enter L1 through 18d's socket (`kind: aws`) |
| After M4 | M5, steps 24 and 28 | `security.v1` joins the integrity step, and only tightens. `categorize.v1` writes topic memberships with origin `jev` |
| After M4 | M6, steps 30 and 35 | Recall is filtered by readers. A summary takes its inputs' integrity. Testimony renders labels |
| After M4 | M7, steps 36, 38, 40, 41, and 43 | MCP results have the `external` origin, and MCP responses use `may_leave`. Gliding meets two audiences. The AWS shells are rows in 18a's table. A self-extended MCP server runs in L1, which needs a long-running stdio job there |

## 4. What it needs from Eddie

None of these blocks the chain. Each says when it matters, and the default the chain takes without an answer.

| # | What | Blocks? | When it matters | Default without an answer |
|---|---|---|---|---|
| 1 | **Decisions** on open questions 1 to 4 (§5): L1's posture, T1 and L1, which jobs go to L1, and marking L0 output | No | 17b and 20a | The defaults in §5 |
| 2 | **`[sandbox]` lines in the vault note**, for L1 to carry his real work: `l1_argv`, `ro_paths` (`~/.cargo` and `~/.rustup` for Rust builds), and `egress` hosts. Install first, then paste (an older binary cannot load the table) | No | After 17b and 18c, when he wants L1 used | Built-in defaults: L0, no L1 list, no egress. The chain collects the lines in the queue file |
| 3 | **Run his daemon as a systemd user service** (`theseusd install --user`, 22a), so L1 gets memory and pids limits, and cancels are verified by cgroup | No | Any time after 22a | L1 runs without cgroup limits, and health says so. A daemon started from a shell lands in that shell's cgroup; the one running during the probe was in the OpenClaw gateway's (§7) |
| 4 | **The bot's Server Members intent** (the developer portal), if it is off. Without it, a guild channel's viewers cannot be read, and the channel counts as public for disclosure | No | 19a's checks in #theseus-test | The channel counts as public, and owner-only results are withheld there. DMs, the CLI, and the web UI are unaffected |
| 5 | **Docker for the separation test.** His user is in the `docker` group, so the chain can run a throwaway container (`--network none`, the binaries read-only, fake secrets) for 22a and 22b's two-user check. Docker is root-equivalent on this machine, so this is asked, not assumed | Only 22a and 22b's two-user live check | 22a | Use the container, with those limits. If he says no, the two-user check is his own run with sudo, and the chain proves the job host as one user |
| 6 | **Switching his own daemon to separated mode** needs sudo, and is his choice | Never | Whenever he likes, after 22b | His daemon stays as it is |
| 7 | **The owner set for labels**: confirm that it is `[approval] trusted_users` (his Discord id) and the local surfaces | No | 19a | As stated |
| 8 | **Any secret he wants L1 jobs to request at run time** (for example a crates.io token), as a `[secrets]` line with a `[broker.secrets.<name>]` posture | No | After 18d | `github_token` alone, at `notify` |
| 9 | **Noted, not asked:** at L0, any job of his user can use Docker, which is root on the host. L1 has no Docker socket. He may want Docker work to wait for approval (`approve_argv = [["docker"]]`) | No | — | Unchanged |

## 5. Open questions, with defaults

The build never waits on these: each step takes the default, and Eddie can overturn it later.

1. **Should L1 earn a looser posture?** For example, `proc.run` at `open` in L1 while L0 stays at `notify`.
   This is where L1 pays off: the tags "mean what they say again" (Appendix B).
   *Default: no. The same posture at both, since nothing loosens without the owner.* A later
   `[policy.tools] "proc.run@l1"` line is a small addition.
2. **In a session that holds external text, should an L1 job with no egress and no secret keep its own
   posture?** It cannot reach the network or a credential, and its writes are scratch, so the hold's worry,
   that a page steers a command, is contained by construction.
   *Default: no. T1's floor is unchanged in M4.* A yes is a few lines in 20a's gate step.
3. **Which jobs go to L1 in his deployment?**
   *Default: none by list. The model's `sandbox: true` alone.* Appendix B's mapping (agent-authored code and
   package installs in L1) comes back with Jev's `shell.v1` in M5, or sooner through his `l1_argv`.
4. **Should an L0 job's output latch its session by program** (theseus-20f's L0 half)? `gh` is the broker's
   first program, and 43% of the DM's calls are `proc.run`. Latching every `gh` call would hold his DM most of
   the day.
   *Default: `[policy] external_programs = []`, so nothing at L0 latches by program.* L1's egress is the
   deterministic answer (§2.4).
5. **The owner set for labels.**
   *Default: the local surfaces and `[approval] trusted_users`.*
6. **A guild channel whose viewers cannot be read.**
   *Default: it counts as public (the worst case).* A declared audience would be stated will, but a wrong one
   leaks, so it is not offered.
7. **Seccomp: `seccompiler`, or a hand-built BPF filter?** `seccompiler` is Firecracker's filter compiler:
   pure Rust, Apache-2.0 OR BSD-3-Clause, and one new dependency. A hand-built filter is about 150 lines, with
   no dependency.
   *Default: `seccompiler`, for its tested BPF generation.*
8. **L1's scratch writes: discard, or promote?**
   *Default: discard, and list what was written.* Promotion is filed, and needs a gated write that carries the
   latch (§2.6).
9. **The readers of fetched pages.**
   *Default: `Public`.* A page is public text, and its integrity is what makes it untrusted.
10. **Where the ontology and the fomites live: META keys, or new record kinds?**
    *Default: META keys.* They need no new layout, and the history stays in the WAL. A record kind can come
    with M6's scans if one is needed.
11. **When an operator's topic takes effect: at once, or at the next recompile?**
    *Default: at the next recompile* (§4.1a, so the prompt cache survives), with `theseus session recompile`
    to apply it at once.
12. **When the job host is disconnected: fail the job, or queue it until the host connects?**
    *Default: fail it, with the reason.* Queuing is filed.
13. **An ad hoc `op://` fetch from a job**, which decision 15 says always waits: build it in M4?
    *Default: filed.* M4's run-time requests cover the board's secrets, and the design is in §2.4.
14. **CPU limits where the cpu controller is not delegated** (it is not by default on systemd 249). It needs a
    root drop-in, `Delegate=cpu cpuset io memory pids` on `user@.service`.
    *Default: no CPU limit, and health says so.*
15. **The open-source default of L1**: should the installer write `[sandbox] default = "l1"` into the configs
    it generates?
    *Default: yes* (§1: "the open-source distribution ships L1 as default").

## 6. Risks, and what would change the plan

**Risks.**
- **WSL kernel drift.** WSL updated itself this morning (06:03), and a new kernel could disable a feature L1
  uses.
  - The probe after serving says so, in health and the ledger.
  - A call that asked for L1 fails loudly, and never falls back to L0.
- **Toolchain friction in L1.** Cargo wants a writable `CARGO_HOME`, and a `target/` in scratch rebuilds from
  cold. npm and pip keep caches too.
  - The mitigation is `ro_paths` with overlays. A read-write escape hatch (`rw_paths`) is filed.
  - 17b's report times a real offline build in L1.
- **The seccomp list, too strict or too lax.** It returns `EPERM`, so a program reports the error rather than
  dying. 17b runs cargo, git, python3, and node under it.
- **Masking `/proc` and `/sys` trades safety against compatibility.** Docker's masks keep `cpuinfo`, `meminfo`,
  and a read-only `/sys`, which runtimes read for CPU counts and memory. `subset=pid` hides more, and would
  break some of them. A fresh sysfs in the namespace can meet the kernel's `mount_too_revealing` check, as
  rootless Podman's does. 17a settles both by test, and says which it chose.
- **The two-namespace chain is fiddly**: the maps, `setgroups`, and the order of handoffs. The contract tests
  pin each clause, and bubblewrap's behavior is the reference in review.
- **T1 regressing through 20a.** T1's 13 tests are the floor's contract and are not edited, and 20a's live
  check repeats T1's word for word.
- **Schema bumps.** An older binary refuses the store after the first newer write, as F4a designed. Before
  installing each step that bumps a schema, take a snapshot of Eddie's store, as F4a's review did.
- **Cross-user assumptions under separation.** One is already found (J1's web trace; §2.9). A hardened host
  that mounts `/proc` with `hidepid` would break J1's socket trace too, and answers would then be refused:
  closed and loud, not open.
- **The tree walk can race a fork loop.** It SIGSTOPs first and rescans in bounded rounds. If the set does not
  settle, the state is `outcome_uncertain`, with why.
- **The proxy is a new network surface.** It listens only inside the job's namespace, and dials only
  allowlisted hosts after the public-only check.
- **A card with no turn running** (18d) is a new path through the outbox and confirm. The budget question is
  the precedent, and it is tested.
- **The fomite index only grows** (META keys, one per latched write). It stays small in practice, and a tender
  can compact it later.
- **Confidentiality friction in shared channels** (M7): owner-only results are withheld there, and graduation
  is manual. Eddie's deployment is unaffected.

**What would change the plan.**
- **L1 becomes his default** (Appendix B's revisit). Promoting scratch writes becomes the happy path, one more
  step after 17b, and L1's view needs his real toolchains.
- **A yes to question 2.** A few lines in 20a.
- **A host where unprivileged user namespaces are restricted**, such as Ubuntu 24.04's AppArmor setting. The
  open-source L1 then needs an AppArmor profile for `theseusd`, which 22a's `--separate` would install.
- **A shared, multi-person channel becomes real before M7.** 19a's viewer machinery becomes live-critical,
  including the intent and viewer changes.
- **The AWS lane settles on another credential model**, such as IAM Identity Center. Only 18d's `kind: aws`
  seam changes.

## 7. Appendix: probes of this machine (2026-09-30, about 15:20 MST)

These were read-only, and left nothing behind.
- **The kernel:** `6.18.40.1-microsoft-standard-WSL2`. **systemd 249** is pid 1.
- **cgroup v2**, unified. The root's controllers are `cpuset cpu io memory hugetlb pids rdma`. The user
  manager (`user@1000.service`) delegates **`memory` and `pids` only**. `cgroup.kill` exists.
- **User namespaces:** `max_user_namespaces` is 96105. There is no `unprivileged_userns_clone` knob, and no
  AppArmor restriction knob.
- **An unprivileged `unshare` of the user, net, pid, and mount namespaces, with a fresh `/proc`, works.**
  Inside it:
  - the uid is 0 and the pid is 1;
  - `/proc/net/dev` shows `lo` alone;
  - `/sys/class/net` still shows the host's interfaces, since `/sys` was not remounted, which is why L1 mounts
    a fresh sysfs inside its own network namespace (or none);
  - `CapEff` and `CapBnd` are full within the namespace, which is why the job needs the nested namespace and a
    dropped bounding set.
- **Kernel config:** `USER_NS`, `PID_NS`, `NET_NS`, `SECCOMP`, `SECCOMP_FILTER`, `OVERLAY_FS`, `VETH`,
  `NF_TABLES`, `CGROUP_BPF`, and `BPF_SYSCALL` are built in. `TUN` is a module, and `/dev/net/tun` exists.
  `SECURITY_LANDLOCK` is on, and Landlock is first in `CONFIG_LSM`, which makes it a later second layer.
  `unprivileged_bpf_disabled` is 2.
- **Several uids, unprivileged, are not possible here.** `/etc/subuid` gives `zeroaltitude` 100000:65536, but
  `newuidmap` and `newgidmap` are not installed. Hence the two single-uid namespaces.
- **`sudo -n`** needs a password. The user is in the **`docker`** group. `/usr/bin/bwrap` is installed.
- **The `theseusd` running during the probe** (pid 607505, since exited, so whose it was is not known) had
  `systemd --user` as its parent, and lived in the cgroup `…/app.slice/openclaw-gateway.service`: not a
  delegated cgroup of its own.
- **This shell runs under a seccomp filter** of its own (`Seccomp: 2`), which does not bear on the daemon.

<!-- REPORT COMPLETE -->
