# scripts

- `gate.sh`: the commit gate. Every check must pass, and the first failure stops it.
- `smoke.sh`: an end-to-end check of a built daemon, with real secrets and real model calls.
- `build.sh`: the one way to build a release or an install. `repro.sh`: two builds of one commit, compared byte for byte.

`gate.sh` is a shared file: it changes only at a join, one change at a time.

## The gate

Run it before every commit, in the tree you will commit, under the shared lock, and chain the commit with `&&`
(a `;` once let an unformatted commit through):

```bash
flock -o ~/.cache/theseus-gate.lock scripts/gate.sh && git commit -S -F <message file>
```

It runs, in order:

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
7. The web apps' lint and build, each when its `node_modules` exists, and then a check that the Observatory's
   committed build is current.

It ends with `gate: ok`. Each step runs under `phase`, which times it: the gate prints a table of seconds before it
ends, a failed run's too (with `<- failed here` on the phase that stopped it, and `gate: FAILED in <phase>`), and
appends it to `~/.cache/theseus/gate-times.csv` (time, label, phase, seconds, status; `$THESEUS_GATE_TIMES`). The
gate's own cost has a history now: read it before calling a gate slow.

### How long it takes

On a warm target, a few minutes: the suite is about 90 s and the bench about 7 s of it, plus any wait for a quiet
machine. A lane's niced gate on a warm target took 135 s once it had the lock (2026-10-01: the suite 105 s at 4
threads, a 15 s wait to settle, the bench 7 s). A fresh worktree's first gate also compiles the whole workspace,
dependencies at opt-level 2 included, which takes far longer: about 22 minutes at nice 19 beside busy neighbours. Waiting for the lock behind another gate, and for a quiet machine, can each add minutes, so
give the gate's command a timeout of 30 minutes or more (a `proc.run` call can ask for up to its
`proc_timeout_max_secs`).

### Running it beside other work

- **The lock** keeps one gate's tests out of another's timing bench. `flock -o` closes the lock's fd in the gate, so
  nothing it starts can keep the lock after it. Never take it with `exec N>lock; flock N` in a shell that starts
  anything long-lived.
- **A lane's gate runs niced**, with its own `CARGO_TARGET_DIR`, and `CARGO_BUILD_JOBS=4` and
  `NEXTEST_TEST_THREADS=4` exported (one or the other, never also nextest's `--test-threads`, which it then refuses
  as given twice): `nice -n 19 ionice -c3 flock -o ~/.cache/theseus-gate.lock scripts/gate.sh`. The bench runs
  `target/debug/theseus-sim` by a relative path, so a worktree needs a `target` symlink to its target dir.
- **A lane's gate skips the bench** (`THESEUS_GATE_NO_BENCH=1`). The bench's settle step waits for a quiet machine
  while holding the shared lock, so every other agent's gate queued behind it; the join's gate on `main` benches
  instead. A lane whose work touches the start path runs `target/debug/theseus-sim bench lifecycle --runs 10
  --check` alone, at normal priority, once before its join, and quotes it.
- **A niced gate whose only miss is the bench**, beside busier neighbours, counts as green when the bench rerun alone
  at normal priority passes. Quote both runs.
- **A gate that sits at 0% CPU** is waiting on a lock: this one (held by another gate, or by an orphan that
  inherited it: scan `/proc/*/fd` for it, since `/proc/locks` hides a dead owner), or cargo's package cache. Find the
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
  idempotent. It also regenerates the marks after a rebase conflict in them: take the other side of the conflict, and run it
  again, instead of resolving the attributes by hand.

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
- **`repro.sh [--rev REV] [--profile P] [--keep] [--bench]`** extracts the commit twice with `git archive`, into two
  directories whose names differ in length, builds each with `build.sh` into a fresh target and with no compile cache (a
  cached object would copy, and prove nothing), and `cmp`s `theseusd`, `theseus`, `theseus-tui`, and `theseus-sim`. Two
  full builds of the workspace from scratch: about an hour beside other work (`THESEUS_REPRO_BUILD_ARGS=--shipped` halves it),
  so run it niced and detached. It exits 1 when a binary differs, and keeps the trees. With `--bench`, a reproducible
  result is followed by `bench size`, `turn`, and `idle` on the first build's binaries, recorded in the bench history
  under the profile and the commit: the numbers that mean something only on an optimized build, which the gate never
  makes. Run it after a toolchain bump, a dependency change that adds a build script, and before a release; a nightly
  job is not installed.
- **The cockpit** (`cockpit/dist`) is not committed, so a binary built with it depends on an npm build. `repro.sh` builds
  from the committed tree, with the page that says the cockpit is missing. Whether `npm run build` is itself reproducible is
  not checked.

## smoke.sh

It needs the vault's service-account token (`OP_SERVICE_ACCOUNT_TOKEN`, or `THESEUS_OP_TOKEN_FILE`) and a config
(`THESEUS_CONFIG`), and spends a little on real model calls. It starts its own daemon on a temporary socket and
state dir, and kills it at the end.

- **Its web check probes `WEB_PORT`, by default 7433: the operator's daemon.** Run it only with a scratch config
  whose `[web]` is off or on a port of its own, and `WEB_PORT` set to match.
