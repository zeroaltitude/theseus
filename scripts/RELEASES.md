# Releases, the installed service, the smokes, and this machine

The release half of `scripts/`: the pinned toolchain and `build.sh`, the reproducibility check,
`user-service.sh`, the two smokes, and the WSL host's own traps. **Every section below was moved here
verbatim from `scripts/AGENTS.md` on 2026-11-10** (theseus-4vsrz, B7), because that file had passed
`context_files::MAX_BYTES` (64 KiB) and the loader was silently cutting its tail out of every session: it
stood 71,910 bytes, 6,374 over, and the cut fell inside its `## setup.sh` section, so nothing from there to
the end of the file had reached a reader since the breach opened. **Not one byte was changed, dropped or
reworded in the move**, and `scripts/context-ceiling.py` now holds the ceiling so this cannot recur unseen.

**`## setup.sh` deliberately stayed in `scripts/AGENTS.md`** and is not here. It is the one section of the
release half that a session is likeliest to need inline, and moving it would have re-presented a line of
pre-existing text as a newly added one to the gate's `scrub` phase, which judges a branch by the lines it
ADDS -- a relocation of text that has been on `main` for weeks would have read as a new disclosure and
reddened the join for no gain. The boundary is drawn where it is for that reason and not a tidier one, and
the finding belongs to theseus-4vsrz's B7 notes.

This file is not an `AGENTS.md` or a `CLAUDE.md`, so no session loads it as project context and it has no
ceiling of its own. Read it when you are cutting a release, installing or running the service, or working
on the WSL host.

## Releases: the pinned toolchain, one build script, and a reproducibility check

(theseus-goa8; review 2's SC1, SC3, and S7.)

- **The toolchain is one release**, pinned in `rust-toolchain.toml`, never `stable`, so a new clippy cannot fail a commit
  that changed nothing and two machines build with one compiler. The file says how to bump it: install the release,
  change `channel`, run the gate, fix what the new clippy finds in the same commit, run `repro.sh`, and commit it alone as
  `toolchain: bump to <version>`. A machine without the pinned release downloads it on its first `cargo` call (rustup's
  auto-install): run `rustup toolchain install` in the tree once, before the gate. CI does.
- **`build.sh [--profile release|release-thin] [cargo args]`** (`release` by default) is the one way to build for an install or a
  release. It builds with `--locked`, rewrites every path rustc would embed (the tree, the cargo home, the rustup home, the
  target directory) to a fixed one, and sets `SOURCE_DATE_EPOCH` to the commit's time. It builds **the five binaries an
  install ships**, `theseusd`, `theseus`, `theseus-tui`, `theseus-sim`, and `theseus-index`, and what they link, and not
  the crates still waiting for their rows (theseus-o8nk; the gate's `features` phase reads the list). A `-p`, `--package`,
  `--workspace`, or `--all` of your own replaces the five. An install copies four (`--installed` prints them): `theseus-sim`
  is built for the size check and the benches, which run the copy in `target/`. It adds `-C force-unwind-tables=no` (no
  `.eh_frame`; the profiles abort on panic and are stripped; theseus-rwpbg.5). Measured on 2026-10-03 (cold, no compile cache, `-j 4`,
  `release-thin`): 1,139 CPU-seconds, about 4 min 50 s, against 1,395 and 5 min 55 s for the whole workspace with the
  voice crate, the install's build until then (18 % less; the voice crate's exit alone was 10 %). Built alone or with the
  whole workspace, from one tree, the five binaries are the same bytes.
  - **Features.** Cargo unifies a dependency's features over the packages it builds, so building some of the workspace
    can give a crate they share fewer features than the whole workspace, which the gate tests, gives it. For the five it
    gives none fewer: on 2026-10-03 cargo's unit graph (`RUSTC_BOOTSTRAP=1 cargo build --unit-graph -Z unstable-options`,
    which prints and builds nothing) held the same units, features included, for everything the five link, whether the
    five were built alone or the workspace whole. The gate's `features` phase holds it on every run (theseus-dr2x):
    it compares `cargo tree -e normal,build -f '{p} {f}'` of the five (`build.sh --shipped` prints the list) with the
    whole workspace's, which compiles nothing and takes about a second, and fails naming each package the five link
    that the workspace builds with more features, and the features it adds:

    ```text
    gate: the whole workspace widens the features of a package the five shipped binaries link, …
      serde_json v1.0.151: the workspace builds it with indexmap, preserve_order
    ```

    Name the feature it finds in the shipped crate that links the dependency, as theseus-discord's manifest names
    twilight-gateway's `rustls-native-roots`, which only the voice crate had turned on until it left the workspace; or
    drop it from the crate outside the five. `cargo tree` resolves the normal and build edges only, as an install
    builds: a feature a dev-dependency adds reaches the tests alone, and the check does not see it.
  - rust-embed's `deterministic-timestamps` (theseusd's manifest) gives the embedded web files no modification time. A
    plain `cargo build --release` still works, and embeds the directory it was built in.
- **The profiles.** `release` is fat LTO with one codegen unit: **the install profile** and a tagged release's (since
  theseus-o2myi; the owner's yes, 2026-10-19). `release-thin` (cargo reserves the name `install`) is thin LTO with 16
  codegen units: the quick build, for the benches, the AWS hand, or a try-out, never an install. Outputs are
  `target/release/` and `target/release-thin/`. `release` also builds the cold crates (Cargo.toml's
  `[profile.release.package.*]`: AWS, Google, file conversion, MCP, LSP) at `opt-level = "s"`, and `release-thin`
  inherits that. The measurements below are the 2026-10-02 pair that made `release-thin` the install; the size and
  speed of the choice now made are in the last bullet. Measured on 2026-10-02 (commit 364b82e, the four binaries, uncached,
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
    install is rebuilt after every reviewed step: so the chain installed `release-thin`, and a tagged release kept
    `release`; theseus-o2myi moved the install to `release` for its size (below). If a bench ever blames the profile, tell by interleaving the two builds' `kernel-sim` runs and comparing
    the least user time.
  - **The install moved to `release` (theseus-o2myi, measured 2026-10-20).** `theseusd` alone, cold, no compile cache,
    `CARGO_INCREMENTAL=0`, at nice 15 on a loaded 16-core host (so the times are upper bounds): `release-thin` at the
    old base 90.2 MB in 500 s; `release-thin` with one cockpit copy and the cold crates at `opt-level = "s"` 86.3 MB in
    424 s; `release` with both 70.5 MB in 1,145 s. So the fat link takes the install from 90.2 to 70.5 MB (-21.8 %) and
    costs about two and a half times the cold build; the cockpit copy and the cold crates together are 4 MB of it
    (4.4 %). The 60 MB target is still 10.5 MB away. Resident memory follows the size: read it with `ps -o rss` on the
    installed `theseusd` after two idle minutes.
- **glibc or static musl.** An install is the host's glibc build. Static musl is the portable build, which
  `build.sh --target x86_64-unknown-linux-musl` builds anywhere (CI built it on every push until theseus-o8nk, and nothing
  used its artifact; §3.18 says "static musl"; the recipe had always built
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
  cached object would copy, and prove nothing), and `cmp`s the five shipped binaries. Two full builds of the five from
  scratch: about 1,140 CPU-seconds a build for `release-thin` (2026-10-03; about 5 minutes each at `-j 4`), a little more
  for `release` (`THESEUS_REPRO_BUILD_ARGS=--workspace` builds the whole workspace instead), so run it niced and
  detached. It exits 1 when a binary differs, and keeps the trees. With `--bench`, a reproducible
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
- **`sync-timer install|uninstall|status`** (theseus-pjmj7) is the nightly OpenClaw sync's timer: a oneshot
  `<unit>-sync.service` running `theseus import sync` and `<unit>-sync.timer` at `--hour`:40 (`Persistent`, so a missed
  night runs at the next start), both marked as this script's, so an uninstall keeps one someone else wrote. It checks
  nothing of the machine (no root refusal); its tests are in `user_service_script.rs`.
- **`--unit NAME`** is a second daemon's unit, `NAME.service` (passed on as `theseusd install --user --unit NAME`, and only
  when it is not `theseusd`, so an older build still installs the default). `install` refuses it unless `THESEUS_SOCKET`
  and `THESEUS_STATE_DIR` are set: on the operator's socket, `--yes` would take the operator's daemon for one started by
  hand and stop it.

## smoke.sh

It needs the vault's service-account token (`OP_SERVICE_ACCOUNT_TOKEN`, or `THESEUS_OP_TOKEN_FILE`) and a config
(`THESEUS_CONFIG`), and spends a little on real model calls. It starts its own daemon on a temporary socket and
state dir, and kills it at the end.

- **Its web check probes `WEB_PORT`, by default 7433: the operator's daemon.** Run it only with a scratch config
  whose `[web]` is off or on a port of its own, and `WEB_PORT` set to match.

## release-smoke.sh

The release smoke (theseus-yf6pj; `bench/smoke/README.md`): five parts, each PASS or FAIL against
`bench/smoke/floor.json`, the verdict the first heading of `bench/smoke/runs/<date>-<sha8>/report.md`. Run it on the
candidate before a release is called, and with `--write-floor` at the tag.

- **It spends real dollars** (about $1, under $8): the TB2 slice needs `ANTHROPIC_API_KEY` and `TYPESAFE_API_KEY` in
  the environment and Docker, and stops once its finished trials pass `SMOKE_TB_STOP_USD` (6). `route` and `dm` talk
  to the live daemon (`SMOKE_SOCKET`, else the CLI's own); `recall` and `resume` cost nothing.
- **It waits for a quiet host** (`bench/harbor/run.py`'s refusal: the gate's lock, CPU pressure) up to
  `SMOKE_QUIET_WAIT_S`, then fails the part. Do not run it beside a gate.

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
  | 7433 | the operator's daemon (its web UI, the cockpit) |
  | 7434 to 7439 | scratch daemons' web UIs, one each (7434 is the cockpit's dev default) |
  | 5174 | the cockpit's dev server |
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
- **The gate lock.** The gate takes it itself; any other holder uses `flock -o`. A gate at 0% CPU is waiting
  on a lock (cargo's package cache, or this one): find the holder before waiting longer.
- **`/tmp` is wiped by a WSL restart.** Keep harnesses, logs, and reports where they survive, and commit and push at
  every green point: a restart, or an account's usage limit, can end a run at any moment.
