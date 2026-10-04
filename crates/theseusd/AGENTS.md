# theseusd

The daemon binary: the kernel behind the protocol, on a Unix socket (the default) or `--stdio`. Also its
subcommands: `job-wrapper`, `check`, `config`, `example-config`, `example-bindings`, `restore`, and `install`.
`example-config` prints the template with an operator's private overlay in place when
`~/.config/theseus/template-overlay.toml` exists or `--overlay FILE` names one (`theseus_core::config_overlay`,
theseus-dxgb). **A test or tool that builds on the template runs `example-config --plain`**, or it reads the
operator's overlay on his machine.

Key modules: `main.rs`, `web.rs`, `install/`. Read by: (a binary).

## What's here

- `src/main.rs`: the start, in the order serve-first requires: the token, the config, secrets resolving in the
  background, the store, the kernel's startup, then serving. The network, and every fsync but the kernel's one, go
  in `after_serving`, as do the actors (the harness loop, the driver, telemetry, the web UI, Discord), which start
  as soon as the socket answers. Also the signal arms (SIGINT and SIGTERM are one clean stop) and the reaper.
- The index tender (row 51): `after_serving` starts the core's supervisor (`theseus_core::tender`) as soon as the
  socket answers, the socket daemon only, and the supervisor starts a tender 2 s later (one an exec kept is taken
  over at once); the reaper hands it each tender's exit; a stop sends the tender SIGTERM and never waits. It runs
  the `theseus-index` beside this binary, never one on PATH.
- The L1 role (M4 17b): `job-sandbox`, an L1 job's init, dispatched first in `main`, before the umask, clap, and
  tracing. Nothing probes L1 at the start: `check` runs its self-test on demand (`/bin/true` in L1 from the
  check's own process, `Sandbox::self_test`), and health reports the last real L1 launch (theseus-gyin). An L1 job
  has no cgroup, so the units ask for no `Delegate=` and have no stop hook. `tests/sandbox.rs` runs real L1 jobs,
  its state dir and socket inside the workspace so the view's hiding is what keeps them out.
- `hand` (step 40, theseus-mgw.6): a hand, the job wrapper inside AWS, run in the hand image (`infra/aws/hand/`) as
  Lambda's bootstrap or a Fargate task's command, never by hand. It reads no config, store, or secret: its spec (in
  the Lambda event, or `THESEUS_HAND`) is its whole input. Its code is `theseus_core::aws::hands::hand`.
- `job-wrapper` catches SIGTERM from its first moments (M4 18a): a cancel asks it alone, and it stops its job's
  whole tree (`theseus_kernel::tree`), an L1 job through its init, then answers in the spool.
  `tests/job_wrapper.rs` stops real trees, a `setsid` sleeper included; `tests/sandbox.rs` an L1 job's.
- `src/web.rs`: the web server for both apps. It embeds `web/dist` and `cockpit/dist` (with `allow_missing`), and
  refuses a wrong `Host` or `Origin` and any uid but the daemon's own.
- `src/install/`: `theseusd install`, the daemon as a systemd service (`--user`, or `--separate` as root). It prints
  a plan and changes nothing; `--apply` performs it, `--check` compares, and `--remove` is the inverse. A `--user` unit's
  token file is checked by `stat` alone and never opened (`token.rs`): a regular file, the operator's, mode 0600 or
  stricter, not empty, else the plan refuses and says the fix, and so does `--apply`, before it writes anything.
  `--op-token-file` is a global flag, so it works before the subcommand and after it; the plan's hint is the command as
  typed with it added. `scripts/user-service.sh` wraps all of this (`docs/user-service.md`).
- `web/dist/`: the Observatory's committed build. `cockpit/dist/`: the cockpit's build, ignored.

## Invariants

- **Serve first.** Nothing on the start path waits for a secret, the network, or a model, and no new work joins it
  without its bench row (`theseus-sim bench lifecycle`).
- **A config in the vault** starts from its last-known-good copy, and acts on it at once when its sha256 is the one
  the daemon recorded in the store as it wrote it (theseus-zmgb); an edited copy makes the start exec itself to read
  the vault first. After serving, the vault is read once, and a changed note restarts the daemon in place: an exec
  of `/proc/self/exe`, with the same pid.
- **Every clean stop is one path**: the `shutdown` method, SIGINT, SIGTERM, and a restart onto a changed note. Each
  writes `server.stopping` and checkpoints, so the next start replays nothing. The stop's answer is written before
  the daemon stops.
- **A serving daemon is a child subreaper** (`children::adopt`): a job's orphans are its to adopt and reap.
  `job::DAEMON_VALUE_FLAGS` must match clap's options; a test here holds them together.
- **One index tender per state dir**, and none outlives its daemon: it holds `<state>/index/LOCK`, exits when the
  daemon's pid does (`--parent`), and after a restart in place the new image takes it over (`children::relearn`
  knows it by its command line). A test config from the template turns `[index]` off (`common::safe_note`): a
  tender would read the operator's model files. `tests/tender.rs` turns it on, with the real binary.
- **Files are the operator's alone**: umask 077 before anything is created, and the state dir, store, and spool
  0700. A job's command gets the operator's own umask back.
- **A panic writes a crash file, then aborts** (`panic = "abort"`, Review 2's consideration 1). `daemon()` installs
  the hook once the state dir exists; it writes `crash-<mode>.json` beside the store. `after_serving` takes it into
  `crashes/`, logs it, writes `server.crashed`, and health reports the newest. A debug build panics there when
  `THESEUS_TEST_PANIC=after_serving` (`crash::planted`), for `tests/crash.rs`; a release build has no plant.
- **Code that runs as root** (`install --separate`): every deletion is one planned file, an empty directory, or a
  socket, never recursive; ownership changes use `lchown`; account tools run by absolute path; `userdel` never
  takes the state dir.

## Tests

- `tests/*.rs` run the real binary. `tests/common/mod.rs` has `Daemon`, which kills and reaps its process when
  dropped: use it for every spawned daemon (an explicit stop alone once leaked one for 45 minutes).
- `tests/common/model.rs` is a stand-in Messages API: tool calls per prompt, and `FakeModel::requests()` keeps
  every request, so a test reads which model each turn asked for.
- `tests/user_service_script.rs` runs `scripts/user-service.sh` and the real `theseusd install --user` against stand-in
  `systemctl`, `loginctl`, `journalctl`, `theseus`, and `op` in a scratch `HOME` (the config tests also stand in for
  `theseusd`'s plan). It touches no real unit or daemon.
- The rigs that need a job's real environment (`job_approval.rs`, `reaping.rs`, `broker.rs`) use a fake `op` and a
  file config, never real secrets.

## Traps

- A test script's wait loop must end when its temp dir is gone, or a failing test leaves it looping on a deleted
  file.
- A daemon test's live peer is traced through `/proc`: run inside a job, an approval it sends is refused. Such tests
  skip that part inside a job, and say so on stderr.
- A start right after a stop waits up to 3 s for the store's lock. A check that reads the store's files waits for
  the old process to exit first.
