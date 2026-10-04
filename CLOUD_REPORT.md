# Cloud report: three gate fixes on top of the cockpit's move (theseus-7ykr, -dr2x, -i5xo)

Branch `cloud/20261004-gate-speed`, from `origin/lane/cockpit-parity` (c7ae697). A 4-core VM, 15 GB, as root.
Started 03:00 UTC; this report written about 04:15 UTC.

| Commit | Issue | What |
|---|---|---|
| 655cd35 | theseus-dr2x | gate: a `features` phase fails when the workspace widens a shipped crate's features |
| d67f88e | theseus-7ykr | gate: the bench build goes; the test build proves it built the bench binaries |
| 6cd2d4c | theseus-i5xo | gate: the cockpit phase installs missing modules, or fails saying why |

Files touched: `scripts/gate.sh`, `scripts/build.sh`, `scripts/AGENTS.md`, and two lines of `cockpit/AGENTS.md` (its
"Building and checking" bullet said the gate runs the npm steps "when `cockpit/node_modules` exists"; that is the
cockpit lane's area, so the edit is kept to that one sentence). No Rust code, no dependency, no lockfile change.

## The gate's phases, before and after (this VM; `THESEUS_GATE_NO_BENCH=1`)

Cold (`cargo clean`, and for the after run node_modules and the cockpit's build removed too):

| phase | before (c7ae697) | after (6cd2d4c) |
|---|---:|---:|
| fmt | 4 | 3 |
| shape | 2 | 2 |
| features | – | 1 |
| clippy | 342 | 328 |
| cockpit | 18 | 24 (includes `npm ci --offline`) |
| bench build | 384 | – |
| test build | 262 | 488 |
| reader rule | 2 | 2 |
| suite | 107 | 108 |
| **total** | **1,121** | **956** |

The test build grows because it now compiles the dependencies the bench build compiled first. Bench build plus test
build: 646 s before, 488 s after. **158 s saved cold (14 % of the gate)**, after paying 8 s for the npm install.

Warm, after a `touch` of `crates/theseus-core/src/lib.rs` (the shape of a gate after an edit): the bench build cost
9.0 s, and the test build after it 20.9 s; the test build alone, after another touch, 23.1 s. The owner's
30 to 90 s is a content change, which costs codegen too. With nothing changed the bench build costs 0.75 s (cargo
swaps the hardlinks back). Each gate after the change: fmt 3 to 4, shape 1 to 2, features 1 to 2, clippy 1 to 3,
cockpit 16, test build 2 to 9, suite 104 to 108.

## 1. theseus-7ykr: the redundant bench build

**Found.** I logged the inode and mtime of each `target/debug/<bin>` every 0.5 s through the cold baseline gate, and
which executables ran:

- The bench build linked all five at 03:20:33 to 03:20:51 (inodes theseusd 622461, theseus-sim 621679,
  theseus-index 620000, theseus 622226, theseus-tui 621955).
- The test build then replaced four: theseus-index → 1683580 (03:23:12), theseusd → 1685567 (03:23:51), theseus →
  1699495, theseus-sim → 1700265 (03:24:38). theseus-tui kept the bench build's copy, because it has no integration
  test needing it, and no bench runs it.
- `theseus-sim bench turn --check --runs 5 --burst 0` ran theseus-sim 1700265, theseusd 1685567, and theseus-index
  1683580 (the tender its daemon starts beside itself). `bench lifecycle --runs 2` ran theseus-sim 1700265 and
  theseusd 1685567. So the benches only ever ran the test build's binaries. The bench build's inodes never ran.
- A test build after the bench build relinked them again, with new inodes, so the two builds are different units:
  the test build's binaries have the workspace's features plus dev-dependencies, as the gate's comments already said.

**Changed (d67f88e).** The `bench build` phase and `bench_build` go. `test build` becomes the function `test_build`:
the same `cargo nextest run --workspace --no-run`, with `--cargo-message-format json-render-diagnostics` and its JSON
saved to `$gate_tmp/test-build.json`. Cargo names every binary the build produced, fresh or relinked, so the phase
fails naming a binary in `bench_bins` (theseusd, theseus-sim, theseus-index) that is missing from that list, or whose
`target/debug/<bin>` is not that file (`-ef`, so a lane's `target` symlink is covered). That keeps the guarantee that
the bench binaries exist, and adds one the bench build never gave: they are current, not left over from an older
build. The bench build also checked something by the way: that the five compile alone with their own features. The
features phase (step 2) now covers that.

**Proved.**
- `test_build` alone passes on the tree. With `theseus-tui` added to `bench_bins` it fails: `target/debug/theseus-tui`
  exists, stale from the bench build, and the test build didn't build it.
- Planted: `crates/theseus-sim/tests/sim.rs` moved aside, so cargo no longer builds theseus-sim's binary and the old
  one stays on disk. The gate fails in `test build` after 28 s: "the test build did not build
  target/debug/theseus-sim, which the benches run, so they would run a stale one…". Restored, `touch`ed, `git status`
  clean.
- Gate at d67f88e: only the known failures (see "The gate"). No "compiled under the lock" note. The turn bench
  passes after it (5 frames).

**Live check for the maintainer** (the owner's machine, main's gate with benches):
```bash
scripts/gate.sh                       # no "bench build" row; "features" and "test build" rows; benches pass
stat -c '%n %i %y' target/debug/theseusd target/debug/theseus-sim target/debug/theseus-index
```
Compare `gate-times.csv`: the `bench_build` rows disappear, and `test_build` grows by less than they cost.

**Left, and choices.** `bench_bins` is a second list beside build.sh's five. It names what the benches run, not what
an install ships, so I kept it separate. If a future bench runs `theseus` or `theseus-tui`, add it there. theseus-tui
needs a build step then, because no integration test builds it. The benches still run binaries with dev-dependency
features, not an install's. That was already true, and the gate's comments said so. Building the five
with their own features for the benches would mean a second build, the cost this step removes.

## 2. theseus-dr2x: the shipped-features recheck

**Found.** The recheck in scripts/AGENTS.md prints nothing on this tree (still true at 6cd2d4c).

**Changed (655cd35).** A `features` phase after `shape`, before `clippy`. It runs `cargo tree -q -e normal,build
--prefix none -f '{p} {f}'` for the five (`-p` each) and for `--workspace`. It strips the parenthesised path and
`(proc-macro)`/`(*)` marks and sorts. Any workspace line the five lack, for a package the five link, is a widening,
and the phase names the package and the features it adds. A package can appear twice (a build dependency's feature
set and a normal one's), so the added features are measured against every set of the five's for that package. It
takes about a second (1.26 s measured; 7 s while cargo was compiling beside it). The five come from
`scripts/build.sh --shipped`, a new option that prints build.sh's list and builds nothing, so the list lives in one
place. build.sh builds from the same array.

**Proved.**
- In a scratch copy of the tree, theseus-ontology (outside the five) asks serde_json for `preserve_order`. The check
  prints `serde_json v1.0.151: the workspace builds it with indexmap, preserve_order` and exits 1.
- The same plant in the real tree fails the whole gate in `features` after 6 s, with that line. Restored the manifest,
  and Cargo.lock from a copy: `cargo tree` had added indexmap to it. `touch`ed both, `git status` clean.
- Gate at 655cd35: only the known failures.

**Live check:**
```bash
scripts/build.sh --shipped            # theseusd theseus theseus-tui theseus-sim theseus-index, one a line
scripts/gate.sh                       # a "features" row of about 1 s
```

**Left.** `cargo tree -e normal,build` resolves features without dev-dependencies, as an install builds. A feature
that only a dev-dependency adds reaches the tests alone, and the check doesn't see it. That is by design, and
scripts/AGENTS.md now says so. A widening that brings in a new package (indexmap above) shows as the widened package,
not the newcomer, which is the one to act on.

## 3. theseus-i5xo: the cockpit's build when node_modules is missing

**Found.** On the old code, with `cockpit/node_modules` and `crates/theseusd/cockpit/dist` both removed, the cockpit
phase printed its skip line and returned 0. `web::tests::the_cockpit_serves_its_shell_at_the_root_or_says_how_to_build_it`
then passed, on its not-built branch.

**Changed (6cd2d4c).** `cockpit` is now `cockpit_modules && lint && test && build && cockpit_built`.
`cockpit_modules` does nothing when node_modules exists. Otherwise it runs `npm ci --offline` (npm's cache), then
`npm ci` over the network. If both fail it removes the half-installed tree and fails with the last 40 lines of npm's
output, and if there is no npm it fails saying so. `cockpit_built` fails when the build leaves no
`crates/theseusd/cockpit/dist/index.html`. The log lives in `$gate_tmp/cockpit-ci.log`.

**Proved.**
- New code, with node_modules and the build removed: the gate installed offline ("cockpit/node_modules is missing;
  installing it from npm's cache"), built, and the test of `/` passed on the built branch. That run was the
  cold gate above, and an earlier warm one.
- The test reads that build: a planted `<title>Planted</title>` in `dist/index.html` fails it. With the file
  restored it passes.
- npm's cache empty (`npm_config_cache=<empty dir>`) and the registry unreachable (`npm_config_registry=http://127.0.0.1:9/`):
  the gate fails in `cockpit` after 10 s, printing npm's `ENOTCACHED` error, and no `cockpit/node_modules` is left.
- No npm on PATH (`PATH=/usr/bin:/bin`, the function alone): "cockpit/node_modules is missing, and there is no npm to
  install it…", exit 1.

**Live check:**
```bash
mv cockpit/node_modules /tmp/nm.aside && rm -rf crates/theseusd/cockpit/dist
THESEUS_GATE_NO_BENCH=1 scripts/gate.sh   # "installing it from npm's cache (npm ci --offline)"; cockpit row ~+8 s
ls crates/theseusd/cockpit/dist/index.html
rm -rf /tmp/nm.aside
```

**Left.** Only a *missing* node_modules is installed. A tree that is present but stale after a package-lock.json
change is not reinstalled. Comparing mtimes would reinstall on every branch switch, and the lint or build fails then
anyway. The theseusd test still has its not-built branch, which a plain `cargo test` outside the gate needs.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at each commit: fmt, shape, features, clippy, cockpit, and test build pass.
The suite runs 1,783 tests: 1,750 pass, 10 skip, and 33 fail, the same 33 at every commit and at the baseline (c7ae697):

- **32 L1 sandbox tests**: 21 in `theseus-sandbox` (`bench spawn_100` and the `contract` clauses) and 11 in
  `theseusd::sandbox`. Each says "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC" (theseus-pv6i,
  known on this VM).
- **`theseus-core tests_output::the_cores_output_matches_its_golden`.** This one is new to the list, and it predates
  this branch. The golden holds a wake's time with a negative UTC offset (`-#:#`), and this VM runs in UTC, so it
  prints `+#:#`. It passes under `TZ=America/New_York`. The test is not hermetic in the timezone. The fix belongs to
  whoever owns tests_output: pin `TZ` in the test, or normalise the sign in the golden's masking.

No flaky retries. The phases after the suite, run by hand at each commit: protocol types ok; turn bench 5 frames
against a budget of 5, ok; `cargo deny --offline check` (advisory database fetched this session) ok. The lifecycle and
jobs benches are skipped in a lane's gate. I ran `bench lifecycle --runs 2` by hand once (LIFECYCLE OK); the jobs
bench's L1 row cannot pass as root.

## For the maintainer's docs commit

- docs/status.md and the Part III item: the gate loses its bench build (158 s of a cold gate here; a second feature
  set of the shared crates on every warm gate after an edit), gains `features` (theseus-dr2x), and builds the cockpit
  whenever it runs.
- Root AGENTS.md needs no change. scripts/AGENTS.md is updated in these commits.
