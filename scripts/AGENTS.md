# scripts

- `gate.sh`: the commit gate. Every check must pass, and the first failure stops it.
- `smoke.sh`: an end-to-end check of a built daemon, with real secrets and real model calls.
- `build.sh`: the one way to build a release or an install. `repro.sh`: two builds of one commit, compared byte for byte.
- `user-service.sh`: the daemon as a systemd user service (`check`, `install`, `status`, `logs`, `restart`, `stop`, `start`,
  `uninstall`, and `--dry-run`). The how-to is `docs/user-service.md`; see "user-service.sh" below.

`gate.sh` is a shared file: it changes only at a join, one change at a time.

## The gate

Run it before every commit, in the tree you will commit, and chain the commit with `&&` (a `;` once let an unformatted
commit through). Its tests and benches take the shared machine-wide lock, and `THESEUS_GATE_LOCK` says who takes it (see
"The lock: outer and inner"):

```bash
# a lane's gate: the gate takes the lock itself, around its tests only. NO `flock` around it.
THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh && git commit -S -F <message file>
# the chain's gate on main: the caller takes the lock for the whole run (outer, the default).
flock -o ~/.cache/theseus-gate.lock scripts/gate.sh && git commit -S -F <message file>
```

In outer mode it runs, in order:

1. `cargo fmt --all -- --check`, then `scripts/shape.sh` (no Rust file over 2,500 lines unless listed), then `cargo clippy
   --workspace --all-targets -- -D warnings`, which also holds the shape budget's functions (see "The shape budget").
2. The reader rule's registry test alone (`tests_registry` in theseus-core), so a miss stops the gate in seconds
   and names every fix at once.
3. The whole suite, `cargo nextest run --workspace` (about 1,300 tests).
4. The generated TypeScript: it fails when `web/src/protocol.gen/` differs from the commit.
5. The lifecycle bench, on debug builds of `theseusd` and `theseus-sim`, ten runs a phase against §9's budgets. It
   first flushes dirty pages and waits, up to 5 minutes, until IO and CPU pressure are low and the load is under the
   core count. A miss reruns once, and only a second miss fails. Every run goes to the bench history
   (`$THESEUS_BENCH_HISTORY`, by default `~/.cache/theseus/bench-history.csv`). With `THESEUS_GATE_NO_BENCH=1` (a
   lane's gate, which the lane recipe sets), this step is skipped: the gate that joins the lane to `main` runs it.
   Then the turn bench (`theseus-sim bench turn --check`): a plain turn's frames, counted from the daemon's WAL,
   against §9's per-turn overhead restated as frames (5; the floor is 2). A count needs no quiet machine, so it runs
   in a lane's gate too, with five runs of each kind and no burst (about 5 s); at the join it runs ten runs and a
   burst of 30 turns and records the row (about 11 s). A miss reruns once.
6. `cargo deny --offline check`: licences, advisories, bans, and sources. Offline: advisories come from the database as
   its last fetch left it (a gate that fetched failed when GitHub or crates.io did, and once when a crate was yanked
   between two gates), and the gate says when that database is more than 7 days old. `deny-daily.sh` refreshes it.
7. The web apps' lint and build (and the cockpit's `npm test`), each when its `node_modules` exists, and then a check that the Observatory's
   committed build is current.

It ends with `gate: ok`. Each step runs under `phase`, which times it: the gate prints a table of seconds before it
ends, a failed run's too (with `<- failed here` on the phase that stopped it, and `gate: FAILED in <phase>`; a signal's
too, after `gate: stopped by a signal`), and appends it to `~/.cache/theseus/gate-times.csv` (time, label, phase,
seconds, status; `$THESEUS_GATE_TIMES`). The gate's own cost has a history now: read it before calling a gate slow.

### The lock: outer and inner

(theseus-rx91.) The suite's timing-sensitive tests and the benches need the machine to themselves, so one gate's tests
never land in another's bench: they run under a machine-wide lock, `~/.cache/theseus-gate.lock`
(`$THESEUS_GATE_LOCK_FILE`). Until 2026-10-02 the caller held it for the whole gate, so every gate's `fmt`, `clippy`,
and test build (minutes) kept every other gate waiting too: that morning six lanes' gates and the chain's join gates
queued behind each other's compiles, and three join gates lost about 25 minutes. `THESEUS_GATE_LOCK` says who takes it:

- **outer** (the default): the caller takes the lock for the whole run, as `flock -o ~/.cache/theseus-gate.lock
  scripts/gate.sh` or the chain's `theseus-quiet.sh` does, and the gate takes none. The chain's gate on `main` runs this
  way: it wants the whole machine, and runs rarely. Its phases are the list above, unchanged.
- **inner** (`THESEUS_GATE_LOCK=inner`): the gate takes the lock itself, only around the phases that need the machine.
  A lane's gate runs this way. The order is:
  1. *Without the lock*, every compile: `fmt`, `shape`, `clippy`, then `bench build` (`cargo build -p theseusd -p
     theseus-sim -p theseus-index`, the binaries the benches run: the gate's `bench_build` function lists them) and
     `test build` (`cargo nextest run --workspace --no-run`, which builds what the suite runs).
  2. *With the lock*, the locked part: the reader rule, the suite, the protocol-types check, the lifecycle bench (skipped
     under `THESEUS_GATE_NO_BENCH`), and the turn bench. Nothing compiles here: step 1 built everything it runs (a
     `gate: NOTE` says so when something does, which means the tree changed after step 1). Cargo links the binaries of
     the build it ran last, and the suite's cargo links the test build's `theseusd` and `theseus-sim`, so in inner
     mode the benches run those (the workspace's features, which an install has too); outer mode's run the `-p`
     build's.
  3. *Without it again*: `deny`, `web`, `cockpit`, and the `web dist` check.

  **Never wrap an inner-mode gate in `flock`, or in `theseus-quiet.sh`.** The wrapper would hold the lock that the gate
  then waits for, and the gate would wait for itself forever. The gate checks: it refuses to start (exit 2, in a
  second) when a process above it holds the lock.

  **Check the worktree's gate has inner mode before you rely on it:** `grep -q THESEUS_GATE_LOCK scripts/gate.sh`. A
  `gate.sh` from before this change ignores the variable, and with no `flock` around it runs with no lock at all. A
  worktree cut before it keeps `flock -o ~/.cache/theseus-gate.lock scripts/gate.sh` until it rebases onto a `main` that has it.

How inner mode takes the lock: after its compile phases the gate runs itself again, as `flock -o LOCK scripts/gate.sh
--locked-part …` (an internal option, never a caller's). `-o` closes the lock's fd before that part starts, so nothing
its tests start (a daemon that outlives its test) can keep the lock past the part (theseus-e6xj), and no function of the
gate ever holds a lock fd. While the part runs, the one process with the lock open is its `flock`; once the gate ends,
none is (the proof: scan `/proc/*/fd`). The part hands its phase times back in a file, so one table covers both halves,
and its exit status is the gate's.

The log says what queued behind what, and for how long, and the table has a `lock wait` row after `test build`:

```text
gate: the compiles are done; taking the shared gate lock (~/.cache/theseus-gate.lock) for the tests and benches
gate: waiting, the lock is held by:
  pid 2943171, in ~/projects/theseus-wt/<another lane>
gate: queued for it ahead of this gate, in order:
  pid 2951376, in ~/projects/theseus-wt/<a third lane>
gate: lock taken after waiting 19 s
…the locked part's output…
gate: lock released after holding it 128 s
```

The wait is the one cost the gate cannot shorten, so read it first when a gate is slow. To stop an inner-mode gate, stop
its process group (`timeout` and the harness do): a signal to the gate's own pid ends it at once, but its locked part
runs on to the end under the lock, and a signal to its `flock` alone frees the lock under a running part.

### How long it takes

On a warm target, a few minutes: the suite is about 90 s and the bench about 7 s of it, plus any wait for a quiet
machine. A lane's niced gate on a warm target took 135 s once it had the lock (2026-10-01: the suite 105 s at 4
threads, a 15 s wait to settle, the bench 7 s). A fresh worktree's first gate also compiles the whole workspace,
dependencies at opt-level 2 included, which takes far longer: about 22 minutes at nice 19 beside busy neighbours. In
inner mode that build runs without the lock, so it holds nobody up. Waiting for the lock behind another gate's tests,
and for a quiet machine, can each add minutes, so give the gate's command a timeout of 30 minutes or more (a
`proc.run` call can ask for up to its `proc_timeout_max_secs`).

### Running it beside other work

- **The lock** keeps one gate's tests out of another's timing bench (see "The lock: outer and inner"). Whoever takes
  it takes it with `flock -o`, which closes the lock's fd in the gate, so nothing it starts can keep the lock after it.
  Never take it with `exec N>lock; flock N` in a shell that starts anything long-lived.
- **A lane's gate runs niced**, with its own `CARGO_TARGET_DIR`, and `CARGO_BUILD_JOBS=4` and
  `NEXTEST_TEST_THREADS=4` exported (one or the other, never also nextest's `--test-threads`, which it then refuses
  as given twice): `THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 nice -n 19 ionice -c3 scripts/gate.sh`, with no
  `flock`. The bench runs `target/debug/theseus-sim` by a relative path, so a worktree needs a `target` symlink to its
  target dir.
- **A lane's gate skips the bench** (`THESEUS_GATE_NO_BENCH=1`). The bench's settle step waits for a quiet machine
  while holding the shared lock, so every other agent's gate queued behind it; the join's gate on `main` benches
  instead. A lane whose work touches the start path runs `target/debug/theseus-sim bench lifecycle --runs 10
  --check` alone, at normal priority, once before its join, and quotes it.
- **A niced gate whose only miss is the bench**, beside busier neighbours, counts as green when the bench rerun alone
  at normal priority passes. Quote both runs.
- **A gate that sits at 0% CPU** is waiting on a lock: this one (held by another gate, or by an orphan that
  inherited it: scan `/proc/*/fd` for it, since `/proc/locks` hides a dead owner), or cargo's package cache. An
  inner-mode gate names the holders in its log (`gate: waiting, the lock is held by:`, with each one's worktree). Find the
  holder before waiting longer, and never kill another agent's process.

### The shape budget

(theseus-goa8; review 2's C1.) Nothing held the shape, so every split the first review made regrew. Now:

- **Functions.** clippy's `too_many_lines` (over 100 lines of code) and `cognitive_complexity` (over 25) are on for the whole
  workspace (`[workspace.lints.clippy]`; the thresholds are in `clippy.toml`), and the gate's `clippy -D warnings` holds them.
  Today's offenders (134 functions on 2026-10-02) carry `#[expect(clippy::..., reason = "...")]`, so nothing was rewritten and
  the list can only get shorter: once a function is under the limit, its `expect` is an unfulfilled lint expectation and
  fails the gate until the attribute is deleted. A new function over the limit fails the gate: split it. One that cannot be
  split (a table, a generated match) gets an `expect` with its own reason.
- **Files.** `shape.sh` (the gate's `shape` phase) fails a Rust file over 2,500 lines unless `long-files.txt` lists it with a
  ceiling and why. A listed file past its ceiling fails; a listed file at or under 2,500 lines fails too (delete its entry), and
  so does an entry for a file that is gone. The list can only shrink, or be raised on purpose, with the reason in the commit.
- **Tightening, and a rebase.** Lower a threshold in `clippy.toml`, run `cargo clippy --workspace --all-targets
  --message-format=json -q | python3 scripts/shape-expect.py` to mark the new offenders, and commit both. The script is
  idempotent. It also deletes its own marks that clippy reports as unfulfilled (a function that another change shortened),
  and regenerates the marks after a rebase conflict in them: take the other side of the conflict, and run it again,
  instead of resolving the attributes by hand. A merge with main needs exactly this: the spine's C2 (4eb6db2) merged
  with no conflict, shortened two marked functions in `turn.rs`, and added one test function over 100 lines, and one run
  of the script marked the one and removed the two.

### The daily deny job

`deny-daily.sh` is what keeps the commit gate's offline `cargo deny` honest (theseus-goa8; review 2's SC2). It fetches the
advisory database and the registry index, runs the whole `cargo deny check` against them, and, when that fails, files one
Beads issue (owner `main`, waiting for an available agent, labels `security` and `deny-daily`) unless one is already open.
A failed fetch is an outage, not a finding: it checks what is cached and files nothing. Exit status: 0 all clear; 1 the check
failed; 2 the fetch failed and the check was clean on what is cached. It reads the repository and never builds or changes it
(`--locked`), so it runs in the chain's tree. Its log is `~/.cache/theseus/deny-daily.log`.

It is not installed. To schedule it as the operator's user, once:

```ini
# ~/.config/systemd/user/theseus-deny.service
[Unit]
Description=Theseus: the daily cargo deny check (theseus-goa8)
[Service]
Type=oneshot
Nice=19
IOSchedulingClass=idle
ExecStart=%h/projects/theseus/scripts/deny-daily.sh

# ~/.config/systemd/user/theseus-deny.timer
[Unit]
Description=Theseus: the daily cargo deny check
[Timer]
OnCalendar=*-*-* 05:15:00
Persistent=true
RandomizedDelaySec=15m
[Install]
WantedBy=timers.target
```

then `systemctl --user daemon-reload && systemctl --user enable --now theseus-deny.timer`. A unit that shows as failed
(`systemctl --user --failed`) means the check failed, or the fetch did and the outage is lasting.

### The flaky list

A test that fails under load by construction, and has an issue that fixes it, is listed in `.config/nextest.toml` as
an override with `retries = 2`. A flake then costs a rerun of that test, not of the gate, and is not hidden: nextest
names each test that passed only on a retry (`FLAKY 2/3`), and the gate prints them after the suite and appends them
to `~/.cache/theseus/flaky.csv` (time, label, test, attempt; `$THESEUS_FLAKY_LOG`).

- **Onto the list**: in the commit that files the flake's issue, add one `[[profile.default.overrides]]` for the one
  test: `filter = 'package(<crate>) & test(=<its full name>)'` and `retries = 2`, with the issue's id in a comment.
  Check the filter with `cargo nextest list --workspace -E '<filter>'`: it must name exactly one test. Never a
  blanket `--retries`: a retry that covers every test hides a real race.
- **Off the list**: in the commit that fixes the test, proven under load (the load-flake recipe: loops at a lower nice
  than the test) and against a planted revert. Delete the override with it.
- **A test not on the list that flakes fails the gate**, as before. The flaky log shows what has been passing on luck.

## Releases: the pinned toolchain, one build script, and a reproducibility check

(theseus-goa8; review 2's SC1, SC3, and S7.)

- **The toolchain is one release**, pinned in `rust-toolchain.toml`, never `stable`, so a new clippy cannot fail a commit
  that changed nothing and two machines build with one compiler. The file says how to bump it: install the release,
  change `channel`, run the gate, fix what the new clippy finds in the same commit, run `repro.sh`, and commit it alone as
  `toolchain: bump to <version>`. A machine without the pinned release downloads it on its first `cargo` call (rustup's
  auto-install): run `rustup toolchain install` in the tree once, before the gate. CI does.
- **`build.sh [--profile release|release-thin] [--shipped] [cargo args]`** is the one way to build for an install or a
  release. It builds with `--locked`, rewrites every path rustc would embed (the tree, the cargo home, the rustup home, the
  target directory) to a fixed one, and sets `SOURCE_DATE_EPOCH` to the commit's time. It builds **the whole workspace**,
  as the gate and the tests do, because cargo unifies a dependency's features over the packages it builds: the four
  binaries built alone get fewer features on 29 of their 334 shared crates (one is a TLS trust setting,
  `twilight-gateway`'s `rustls-native-roots`, which the voice crate turns on), so a binary built from its own packages is
  not the one that was tested. `--shipped` builds only `theseusd`, `theseus`, `theseus-tui`, and `theseus-sim` (335 of the
  workspace's 607 crates; the rest, candle, tantivy, the voice stack, and the AWS clients, link into no shipped binary
  yet): a cold build about 40 % shorter, for looking, not installing. rust-embed's `deterministic-timestamps` (theseusd's
  manifest) gives the embedded web files no modification time. A plain `cargo build --release` still works, and embeds
  the directory it was built in.
- **The profiles.** `release` is fat LTO with one codegen unit: a tagged release. `release-thin` (cargo reserves the name
  `install`) is thin LTO with 16 codegen units: the install profile, for the chain's installs and for anyone building for
  themselves. Its output is `target/release-thin/`. Measured on 2026-10-02 (commit 364b82e, the four binaries, uncached,
  one 16-core machine under a load of 10 to 30, the two builds interleaved):
  - **Build time.** A cold build costs the same: 10 min 47 s against 10 min 44 s for the first of each pair, and thin
    used about 10 % more CPU (1,600 against 1,460 CPU-seconds: 16 codegen units repeat work). An incremental rebuild after
    a change to `theseus-core` is where it pays: 4 min 30 s and 2 min 11 s, against 7 min 37 s and 12 min 27 s.
  - **Size.** `theseusd` 28.8 MB against 23.4 MB (+23 %); `theseus` 3.5 against 2.7, `theseus-tui` 2.5 against 2.2,
    `theseus-sim` 4.6 against 3.7. Every one is under §9's 60 MB.
  - **Speed.** CPU-bound work is slower. `theseus-sim kernel-sim --seeds 10 --p-race 0`, ten runs of each, interleaved:
    3.13 s of CPU at its best against 2.84 s (+10 %; the medians, +8 %; a noisier earlier pass read +18 %). The daemon's
    own benches cannot resolve it. A cold start is 24 to 38 ms on either. A plain turn is 40 to 80 ms on either, 5 frames
    each, and its five `fdatasync`s (6 to 15 ms apiece, swinging with the neighbours' load) outweigh the CPU: thin's turn
    read slower in three of four interleaved passes (by 3 to 27 ms) and faster in the fourth (by 8 ms), far more than 10 %
    of a turn's 15 ms of harness CPU could make, so it is noise. An idle daemon's CPU is 7.2 ms against 7.5 ms in 30 s.
    Resident memory is higher: 17.5 MB against 14.1 MB at idle (+24 %), and 23.2 against 21.3 MB after a burst of 30
    turns.
  - **The call.** Review 2's S7 expected an install "a little slower" and asked for a measurement. Ten percent of
    CPU-bound work is more than a few percent, but a daemon that waits on its disk and its model does not show it, and the
    install is rebuilt after every reviewed step: so the chain installs `release-thin`, and a tagged release keeps
    `release`. If a bench ever blames the profile, tell by interleaving the two builds' `kernel-sim` runs and comparing
    the least user time.
- **glibc or static musl.** An install is the host's glibc build. Static musl is the portable build, which CI builds on every
  push and `build.sh --target x86_64-unknown-linux-musl` builds anywhere (§3.18 says "static musl"; the recipe had always built
  glibc, and nothing had measured the difference). Measured on 2026-10-02: one commit, `release-thin`, the whole workspace, one
  driver for both daemons, interleaved on a busy machine.
  - musl is **lighter and quicker to start and stop**: 14.4 against 17.2 MB resident after the start and 17.5 against 23.3 MB
    after a burst (steady over three rounds), 20.8 against 47.9 MB with 10,000 parked sessions; a clean shutdown in 44 ms against
    63, a SIGKILL restart in 38 against 64, a binary swap in 58 against 72 (medians; a static binary has no dynamic loader to run).
  - musl is **slower on allocation-heavy work**, its allocator taking one global lock: `kernel-sim` takes 36 to 38 % more user time
    and 3 to 5 times the system time (eight interleaved runs), a burst of turns 85 ms a turn against 75, and a daemon with 10,000
    parked sessions, which never goes quiet (review 2's S1), spends 7.5 % of a core against 4.5 %.
  - Sizes are equal (musl is 0.5 to 4.6 % bigger). The glibc binaries need glibc 2.34 or newer (Ubuntu 22.04, Debian 12, RHEL 9);
    musl's are static-pie. musl's libc and start files come with the toolchain and glibc's from the host's libc6-dev, so the musl
    build is the more hermetic across machines (both still compile ring's C with the host's gcc).
  - **So the install is glibc.** The gate, the review's live check, and the benches all run glibc builds, so what is installed is
    what was tested, and musl's CPU cost would land on the paths that grow (candle and tantivy, once the index tender is wired in).
    **What would flip it:** a musl build with a better global allocator that closes the CPU gap in this comparison (build both with
    `build.sh`, run `theseus-sim bench turn` and `idle` against each `theseusd` with one driver, interleaved, and compare
    `kernel-sim` user and system time), and a musl run in the gate or a nightly job, so that what ships is tested.
- **`repro.sh [--rev REV] [--profile P] [--keep] [--bench]`** extracts the commit twice with `git archive`, into two
  directories whose names differ in length, builds each with `build.sh` into a fresh target and with no compile cache (a
  cached object would copy, and prove nothing), and `cmp`s `theseusd`, `theseus`, `theseus-tui`, and `theseus-sim`. Two
  full builds of the workspace from scratch: 23 minutes for `release-thin` beside other work (9 min 53 s and 13 min 11 s, 1,850
  CPU-seconds a build), a little more for `release` (`THESEUS_REPRO_BUILD_ARGS=--shipped` is about 40 % shorter), so run it
  niced and detached. It exits 1 when a binary differs, and keeps the trees. With `--bench`, a reproducible
  result is followed by `bench size`, `turn`, and `idle` on the first build's binaries, recorded in the bench history
  under the profile and the commit: the numbers that mean something only on an optimized build, which the gate never
  makes. Run it after a toolchain bump, a dependency change that adds a build script, and before a release; a nightly
  job is not installed.
- **The cockpit** (`cockpit/dist`) is not committed, so a binary built with it depends on an npm build. `repro.sh` builds
  from the committed tree, with the page that says the cockpit is missing. Whether `npm run build` is itself reproducible is
  not checked.

## user-service.sh

(theseus-w1nf.) The operator's script for running `theseusd` under systemd: it checks the machine, runs `theseusd install
--user` (which writes the unit), and wraps `systemctl --user` and `journalctl --user`. What to keep true when you change it:

- **It never opens the token file.** The file is checked with `stat` and nothing else; the same rules as the install plan's
  (`crates/theseusd/src/install/token.rs`): a regular file, the operator's, mode 0600 or stricter, not empty.
- **`--dry-run` prints every command and runs none.** Every command goes through `run` (changes something) or `probe` (reads),
  which print it in a dry run; a command outside them breaks that.
- **A daemon is found by its socket answering `theseus health`**, within a bound, never by matching process names. Every wait
  is bounded.
- **The token flag goes before the subcommand** (`theseusd --op-token-file F install --user`): the order every installed
  build reads, including those from before it became a global flag.
- **`check` reads the config from the plan's `config:` line**, never from `THESEUS_CONFIG` alone: the unit gets what the plan
  prints (the variable, else the build's built-in default, made absolute), and a build's default can be a file the machine
  does not have. A file must be readable and a vault reference well formed, or `check` fails and `install` stops before its
  first question. That holds on a build before theseus-8d1b (a vault reference as the default) and after it (a file).
- **Its tests** are `crates/theseusd/tests/user_service_script.rs`. They run the real script and the real `theseusd install
  --user` in a scratch `HOME`, with `systemctl`, `loginctl`, `journalctl`, `theseus`, and `op` replaced by one stand-in that
  logs each call and keeps its state in files. A command the script starts to run needs a case in that stand-in, and a line
  in the dry-run test. The config tests swap `theseusd` for a stand-in plan (`PLAN_STANDIN`) that names a file as its default,
  so they hold before and after theseus-8d1b changes the real default.

## smoke.sh

It needs the vault's service-account token (`OP_SERVICE_ACCOUNT_TOKEN`, or `THESEUS_OP_TOKEN_FILE`) and a config
(`THESEUS_CONFIG`), and spends a little on real model calls. It starts its own daemon on a temporary socket and
state dir, and kills it at the end.

- **Its web check probes `WEB_PORT`, by default 7433: the operator's daemon.** Run it only with a scratch config
  whose `[web]` is off or on a port of its own, and `WEB_PORT` set to match.

## This machine

Theseus is built on one WSL2 machine, beside the operator's own running daemon and other agents' work.

- **The operator's daemon** runs as a bare `theseusd` on `~/.theseus`, its default socket, and web port 7433. Never
  touch it: no signal, no connection to its socket or port, and never a copy of its bindings file (two daemons would
  answer on Discord). Kill only the pids you started, never by name with `pkill` or `killall`.
- **Your own scratch daemon** has its own `--config`, `--socket`, and `--state-dir`: a fresh state dir, or a copy of
  the operator's store alone. Turn `[discord]` and `[web]` off in its config unless the check needs them. A
  Discord-enabled scratch daemon runs only on a fresh state dir (theseus-c3e). Stop it with
  `theseus --socket <its socket> shutdown`, and wait for its pid to exit.

  | Port | Whose |
  |---|---|
  | 7433 | the operator's daemon (web UI and cockpit) |
  | 7434 to 7439 | scratch daemons' web UIs, one each (7434 is the cockpit's dev default) |
  | 5173, 5174 | the Observatory's and the cockpit's dev servers |
- **The disk.** WSL's disk is a file on the Windows drive (C:), and it only grows. When C: fills, the whole VM
  pauses, while Linux's `df` still shows hundreds of GB free. Check C: with the operator's disk guard before a heavy
  build, and don't build under 30 GB. Keep one target dir per worktree. Delete build caches outright, never to the
  Trash, which keeps every byte on C:.
- **sccache.** No server survives a restart, and by default it exits after 10 idle minutes. Its client then hangs.
  If `pgrep -x sccache` finds none, run `SCCACHE_IDLE_TIMEOUT=0 sccache --start-server`, or unset `RUSTC_WRAPPER`.
- **A connect to a loopback port with no listener hangs**: the packet is dropped, not refused. Bound every connect,
  and test a service that is down with a fake, never with a closed port.
- **`pgrep -f`, `pkill -f`, and `ps | grep` match your own shell**, whose command line holds the pattern. Find a
  process by pid (`$!`, `pgrep -P`) or by `/proc/<pid>/comm`.
- **`git stash` is shared by every worktree.** Set work aside as a patch file instead.
- **The gate lock.** An inner-mode gate takes it itself; any other holder uses `flock -o`. A gate at 0% CPU is waiting
  on a lock (cargo's package cache, or this one): find the holder before waiting longer.
- **`/tmp` is wiped by a WSL restart.** Keep harnesses, logs, and reports where they survive, and commit and push at
  every green point: a restart, or an account's usage limit, can end a run at any moment.
