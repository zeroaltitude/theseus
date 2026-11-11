# scripts

**This file would be CUT at 64 KiB. Something fails first now** (theseus-4vsrz).
`context_files::MAX_BYTES` is `64 * 1024` and this is a `Level::Project` context file, so past that its tail
silently stops reaching a session. **The gate's `context ceiling` phase holds it**
(`scripts/context-ceiling.py`, which reads the ceiling out of `context_files.rs` rather than copying the
number): a file over it that `scripts/context-files.txt` does not list fails the gate, and a listed file
past its own allowance fails too. Run `python3 scripts/context-ceiling.py` to see every file's headroom
before you add to one.

The warning that used to stand here said "nothing fails" and gave a figure from one branch, and it was
right about both: on 2026-11-10 this file reached **71,910 bytes — 6,374 over** — and the loader had been
cutting everything from `## setup.sh` to the end out of every session that read it, for an unknown number
of days, with no gate phase, no test and no `long-files.txt` entry measuring it. Most of the release half of
the file was moved to `scripts/RELEASES.md` verbatim to get back under — `## setup.sh` below stayed, for the
reason that file's header gives — and the phase above exists so the next breach is loud. **Put depth in the
design docs, or in `scripts/RELEASES.md` or `scripts/GATE.md`, neither of which has a ceiling.**

**STANDING INSTRUCTION, and it is why this file has siblings: a new paragraph goes in a sibling document,
not here.** This file carries the script list and the gate's phases; anything longer than a few lines about
*one* script, one environment variable or one incident belongs in `scripts/RELEASES.md` (releases, the
service, the smokes, the host), `scripts/GATE.md` (running the gate on a busy machine) or `docs/design/`,
with one line here pointing at it. The headroom this file has is the measured difference between its size
and 65,536 bytes and nothing else: it was spent twice on 2026-11-10 alone, **+6,695 bytes across two
departures in about 34 minutes**, so a trim buys days only if the pace stops. `python3
scripts/context-ceiling.py` prints the number before you add; do not quote a headroom figure anywhere,
including in a commit message, because it is stale by the next departure.

- `gate.sh`: the commit gate. Every check must pass, and the first failure stops it.
- `smoke.sh`: an end-to-end check of a built daemon, with real secrets and real model calls.
- `size-step.sh`, `tree-stamp.sh` (theseus-2m7jg): the binary's size check. `build.sh` runs `theseus-sim bench size --check`
  on what it built and writes `target/<profile>/.theseus-build-stamp` (`tree-stamp.sh`'s "commit digest" and theseusd's
  bytes); the gate's size phase (`size-step.sh`) runs the same check when that stamp matches the tree under test, else
  says "size: skipped: no release build of <commit>". It fails on growth over 1 MB, not on §9's 60 MB.
  `THESEUS_SIZE_ACCEPT_GROWTH=1` accepts a growth. `size-breakdown.py` (an unstripped
  build in its own target directory) prints the top crates by bytes and the cockpit bundle's.
- `prompt-step.sh` (theseus-4vsrz, B7): the gate's `prompt` phase, which **never fails it**. It runs
  `theseus-sim bench prompt` on the debug binaries after the test build (like `recall-tests.sh`, and for the same
  reason: a stale daemon counts the previous head's prompt) and says `prompt: skipped: no <path>` when either
  binary is missing. Besides the first request's total per place, the bench records the **session's own context**
  and its share of `[routing] cold_switch_tokens` — the figure the deleted `tests_route_cliff` assertion used to
  watch and that nothing watched after 2026-11-08 — warned of on movement off
  `crates/theseus-sim/baselines/prompt.json` and never gated. **The first run of it found two divergences nothing
  had been watching** (2026-11-10): `channel` +8.3 % over its baseline, and `dm` at 9,700 against a committed 14,586,
  a third below it. Neither baseline was moved by the row that added the phase — moving one is a deliberate act. **Every failure path here prints and exits 0 on purpose:** `shape.sh` owns the hard ceilings, and
  a second hard gate on a share nobody has calibrated would redden departures for figures legally inside their
  limits. Tested by `crates/theseus-sim/tests/prompt_step.rs`; its stand-in for `test_gate_tree.py` is in `COMMITTED`.
- `scrub-added.sh`, `test_scrub_added.py` (theseus-ev3ir): the gate's `scrub` phase. The name scrub (`scrub_check.py --strict`)
  over the lines a branch ADDS since the range's base (committed, uncommitted, and untracked new files, which is what a new
  `.cases.toml` is) and its commit messages (at a staged join, MERGE_HEAD's too); a hit in a blocking family (employer, the
  vault's names, a person's name, the configured `[privacy] sensitive_names`) exits 1 and prints the SOURCE file:line
  (a message: its commit and line), never the match. The scrubber is outside the repo, because its patterns are the names:
  `THESEUS_SCRUB_CHECK` (default `~/reports/theseus-docs-v076/scrub_check.py`), and only the WSL host has it. **Present: the
  phase runs and must pass; absent (L1 self-build view, ARM, cloud): it SKIPS LOUDLY, `gate: scrub SKIPPED`, and the
  table reads `scrub skipped` where a pass reads `scrub ok`. A join gate showing `scrub skipped` is not evidence for a push:
  the guard is the scrub run on the WSL host at the join** (`scripts/scrub-branch.sh` over the staged merge).
  `THESEUS_GATE_NO_SCRUB=1` skips it by choice (the ARM runner sets it). The driver's exit codes: 0 clean, 1 a hit, 2 no
  base or unreadable names, 3 nothing added (main's own tip: accepted, `scrub empty`), 4 no scrubber. A branch is judged by
  MAIN's copy of the driver when main has one, and both files are in the keel guard's `KEEL_FILES`. `test_scrub_added.py`
  plants a FAKE name (through `SCRUB_EXTRA_NAMES`) in a scratch repo, runs gate.sh's own `scrub_added` under `set -e`,
  and runs the cloud reviewers' `scrub-branch.sh` (`THESEUS_SCRUB_BRANCH`; its tests are skipped where it is absent). Its
  git calls ignore the developer's global config (a signing agent that is locked hung it). Never put a real name in a test.
- `line-pressure.py`, `tests/test_line_pressure.py` (theseus-4vsrz): functions within 5 lines of clippy's `too_many_lines`
  ceiling (100), closest first, as `path:line fn N/100`. Always exits 0 on a report (a warning, not a gate: clippy holds the
  ceiling); exits 2 only when it scanned nothing. Counts as clippy does (body without braces, blank and comment-only lines
  free; a plain `async fn` counts 2 more than its body, as clippy reads it, unless it carries a `test` attribute; only the
  attribute block directly above a `fn` is read for an `allow`/`expect`); its header says where it can differ. Run it before growing a function; extract a helper, never an `allow`.
  **The gate runs it, at margin 5, as the `line pressure` phase between `shape` and `clippy`** (the owner's
  ruling, 2026-11-10; before that it existed and nothing ran it). It prints the 20 tightest and names the command
  for the rest, and its count and margin are repeated under the phase table, so a 30-line `ec2.sh tail` still
  carries them. **Its exit status never decides the gate's**: shape.sh holds the file ceiling and clippy the
  function one, and a second threshold on the same quantity is one more thing a merge has to argue with. The
  margin came from the distribution on `5e71c15a` (37,654 functions: margin 0 lists 17, 3 lists 93, 5 lists 142,
  10 lists 260), 5 being the narrowest window that still covers two branches each adding three lines to a
  function at 95, and the widest whose output reads as a list. **On 2026-11-10 seventeen functions were at exactly
  100** — the row's older figure of thirty-two predates this week's test-move lands.
- `tests/*`: the helpers' own unit tests, which the gate runs as the `script tests` phase beside the one above and
  **also never fatally**. The argument for making them fatal — they are deterministic unit tests of a script's
  correctness, not a threshold on the tree — is recorded in `gate.sh`'s own comment on that function, for the owner.
- `env-fail-set.py`, `tests/test_env_fail_set.py` (theseus-4vsrz): the **4-core cloud routine VM's**
  environmental-failure set, derived from the tree rather than counted by hand. **It is that VM and not a fleet
  box**: a fleet gate is a `c7i.8xlarge` whose gate runs in a delegated `systemd --user` unit, and it comes back
  with `clause_01` to `clause_12` green and `reds:` empty — anything written about this set must name its host, or
  the next reader waves away a real red. Three places, three parsers (a `harness = false` binary's `const CASES`
  with `const IGNORED` excluded; the same declared inline in `fn main()`; an ordinary `#[test]`/`#[tokio::test]`
  file with `#[ignore]` excluded), which is the third reason the hand count drifted. `--markdown` prints the cloud
  preamble's block ready to paste, `--names` the bare names, `--expect N` exits 1 when the tree no longer totals
  N. **The count had been wrong in four successive briefs (4, then 35, then 40, then 36), and the tree totalled 40
  on `5e71c15a`**: regenerate the block, never edit the number. **It is deliberately NOT a gate phase** — an
  `--expect` there would fail a branch over a host's measured number that nobody has recalibrated.
- `invariant-pressure.py`, `tests/test_invariant_pressure.py` (theseus-4vsrz): every per-file numeric invariant and
  its headroom in one second, for a cut to run on the STAGED MERGE before the gate -- each `long-files.txt` entry and
  each unlisted Rust file within `--margin` lines of its limit (`path n/limit  N left`, or `OVER by N`), the functions
  at or over clippy's 100 (by calling `line-pressure.py`, never counting again), and the `MustBeComplete` censuses as a
  ROSTER (`item`, `least`, `stale_fails`), named and not measured: a second walk of the same world here would be a
  second copy of the rule, free to disagree with the test that holds it. `--since REV` marks `(+N in this range)`
  against each invariant the range moved, which answers "which branch spent the room". Like `line-pressure.py` it
  **always exits 0 on a report** (2 only when it read nothing): a second gate on these numbers would be a second
  ceiling the next merge has to argue with, and `shape.sh` and clippy are the ones that hold them. Why these numbers
  are a property of the BATCH and not of the branch, with the inventory and the two shapes that are worse, is
  `docs/design/invariant-merge-class.md`. Its own unit test runs in the `script tests` phase with the rest of
  `scripts/tests/` and never fatally (`tests/*` above); alone it is `python3 -m unittest -q
  scripts/tests/test_invariant_pressure.py`.
- `context-ceiling.py`, `context-files.txt`, `tests/test_context_ceiling.py` (theseus-4vsrz, B7): the gate's
  **`context ceiling`** phase, and the one numeric invariant on this list that is **HARD**. No instruction file the
  loader reads may pass `context_files::MAX_BYTES` (64 KiB) unless `context-files.txt` lists it with an allowance and
  a reason, held with `shape.sh`'s exact discipline: unlisted and over fails, listed and past its allowance fails,
  listed and back under fails as a stale entry. **The ceiling and the file names are READ OUT OF THE RUST SOURCE**
  (`MAX_BYTES` from `context_files.rs`, `NAMES` from `project_context.rs`), never copied, so the two cannot drift; it
  exits **2** rather than 0 if it cannot find either, because a scan that read nothing must not look like a clean
  scan. Coverage is every git-tracked `AGENTS.md` and `CLAUDE.md` anywhere in the tree, because
  `project_context::chain()` reads both names in every directory from the repository root down to the session's own
  working directory. **Not covered**: files outside the repository (the owner's user-level file, anything
  `[context] files` names), and `project_context::MAX_TOKENS`, the PROJECT level's separate 8,000-token budget, which
  can still cut or drop a file that is comfortably inside `MAX_BYTES`. Why it is hard where `line-pressure.py` and
  `invariant-pressure.py` only warn: **this file was 6,374 bytes over on 2026-11-10 with its own "`wc -c` this file"
  warning at the top**, so a printed warning is the control that had already been tried and had already failed. The
  answer to a file over the ceiling is to move depth out of it (`RELEASES.md` is the worked example); **raising
  `MAX_BYTES` is not a fix** -- it spends context on every session for every file. Run it by hand as
  `python3 scripts/context-ceiling.py` for the full headroom table.
- `build.sh`: the one way to build a release or an install. `repro.sh`: two builds of one commit, compared byte for byte.
- `user-service.sh`: the daemon as a systemd user service (`check`, `install`, `status`, `logs`, `restart`, `stop`, `start`,
  `uninstall`, and `--dry-run`). The how-to is `docs/user-service.md`; see "user-service.sh" in `RELEASES.md`.
- `openclaw-export.py` (theseus-pjmj7): OpenClaw's own session stores (`<state>/agents/<agent>/agent/openclaw-agent.sqlite`,
  opened read only) as episodes of the import's format 1, one per session, for `theseus import sync` (which embeds it) or
  `theseus import openclaw -`. Python's standard library only; a zstd event is decoded by Python's own codec, the `zstd`
  program or Node's zlib (OpenClaw's own runtime). Its module comment is the design: the id, the author and integrity rules,
  the place, the cursor (per agent: the newest event and archive rowids and rewrite-watermark time; a floor under every
  session). Tests: `scripts/tests/test_openclaw_export.py` over small stores `scripts/tests/openclaw_fixture.py` builds
  with OpenClaw's own column names, pinned there; the gate runs them through theseusd's `tests/import_sync.rs`.
- `setup.sh`: one command from a checkout to a running daemon: preflight, build, install, `/etc/theseus/theseus.toml` from
  the template, `theseusd check`, then `user-service.sh install`. The how-to is `docs/setup.md`; see "setup.sh" below.

`gate.sh` is a shared file: it changes only at a join, one change at a time.

## The gate

Run it before every commit, in the tree you will commit, and chain the commit with `&&` (a `;` once let an unformatted
commit through). Its tests and benches take the shared machine-wide lock, and the gate takes it itself (see "The lock"
in `scripts/GATE.md`):

```bash
# a lane's gate: no timing bench. NO `flock` around it.
THESEUS_GATE_NO_BENCH=1 scripts/gate.sh && git commit -S -F <message file>
# the chain's gate on main: the same gate, with its benches. NO `flock`, and no theseus-quiet.sh, around it either.
scripts/gate.sh && git commit -S -F <message file>
```

A `Keel:` line goes in the commit message's last paragraph, beside the `Co-Authored-By` trailer: the guard refuses an ack in
an earlier paragraph.

It runs, in order:

0. *Without the lock, before any compile*, the keel guard (`keel`, then `keel tests`; see "The keel guard"): a branch
   that deletes or loosens a test, a budget or a ceiling fails in about a second, unless the owner's signed ack passes it.
1. *Without the lock*, every compile: `cargo fmt --all -- --check`, then `scripts/shape.sh` (no Rust file over 2,500
   lines unless listed), then `features` (the five shipped binaries get no fewer features built alone than in the
   workspace; see "Releases" in `RELEASES.md`, and "Features"), then `cargo clippy --workspace --all-targets -- -D warnings`, which also
   holds the shape budget's functions (see "The shape budget"); then the cockpit's `npm run lint`, `npm test`, and
   `npm run build`, before the suite, whose tests of `/` read that build. When `cockpit/node_modules` is missing, the
   gate installs it first (`npm ci --offline` from npm's cache, then `npm ci` over the network), and fails, saying
   why, when it can't or there is no npm (theseus-i5xo: it used to skip, and the tests of `/` then passed on their
   not-built branch); it also fails when the build leaves no `crates/theseusd/cockpit/dist/index.html`. A failing
   npm step prints its name and the last 40 lines of its output above the table. Then
   `test build` (`cargo nextest run --workspace --no-run`), which builds what the suite runs, and with it the debug
   `theseusd`, `theseus-sim`, and `theseus-index` the benches run (cargo builds a package's binaries for its
   integration tests). It fails naming a bench binary that cargo's messages say it did not build, which the benches
   would otherwise run stale (the gate's `bench_bins`). The `bench build` before it (`cargo build -p` of the five
   shipped binaries) is gone (theseus-7ykr): cargo links `target/debug/<bin>` from the build it ran last, so the test
   build replaced its binaries before any bench ran, and it cost a second feature set of the shared crates (384 s of
   a cold 1,121 s gate on a 4-core VM, 9 s after a touch of theseus-core). Last, `recall tests`
   (`scripts/recall-tests.sh`, bench/recall's Python tests; Python >= 3.11 or it says it skipped), **after** the test
   build and never before it (theseus-a3woz): `test_drive.py` drives `target/debug`'s four binaries and skips itself
   when one is missing, so from ahead of the build a warm lane tested the previous head's daemon and said nothing.
   `bench/recall/test_gate_order.py` holds the order.
2. *With the lock*, the locked part. Nothing compiles here: step 1 built everything it runs (a `gate: NOTE` says so
   when something does, which means the tree changed after step 1).
   - The reader rule's registry test alone (`tests_registry` in theseus-core), so a miss stops the gate in seconds
     and names every fix at once.
   - The generated TypeScript: it runs the generator test (`ts::the_cockpits_types_are_generated_from_the_rust_ones`,
     alone, which must run once and pass) and then fails when `cockpit/src/protocol.gen/` differs from the commit, so
     a stale file fails the gate that carries it, not the next one (theseus-5epuo).
   - The lifecycle bench, on debug builds of `theseusd` and `theseus-sim`, ten runs a phase against §9's budgets. It
     first flushes dirty pages and waits, up to 2 minutes, until IO and CPU pressure are low and the load is under
     three quarters of the cores; when the 2 minutes pass with the machine still busy, the timing budgets get the busy
     allowance (see "The busy allowance" in `scripts/GATE.md`). A miss reruns once, and only a second miss fails. Every run goes to the
     bench history (`$THESEUS_BENCH_HISTORY`, by default `~/.cache/theseus/bench-history.csv`; **read it by column
     name, per header segment, never positionally**: the file holds many headers and `inflight` was inserted
     mid-array at one of them, so a positional read across it is silently wrong. The reader is
     `crates/theseus-sim/src/history.rs`, whose module doc says why). With
     `THESEUS_GATE_NO_BENCH=1` (a lane's gate, which the lane recipe sets), this step is skipped: the gate that joins
     the lane to `main` runs it.
   - Then, skipped with it, the jobs bench's L1 row (`theseus-sim bench jobs --class l1 --runs 20 --check`): an L1
     start's p95 under §2.2's 25 ms, on the machine the lifecycle bench settled, with the allowance when that settle
     found no quiet window; a miss reruns once. The suite's `the_jobs_bench_l1_row` measures the row and bounds
     nothing, since the suite runs under any load (theseus-mll1).
   - Then the turn bench (`theseus-sim bench turn --check`): a plain turn's frames, counted from the daemon's WAL,
     against §9's per-turn overhead restated as frames (4; the floor is 2). A count needs no quiet machine, and never
     gets the allowance, so it runs in a lane's gate too, with five runs of each kind and no burst (about 5 s); at the
     join it runs ten runs and a burst of 30 turns and records the row (about 11 s). A miss reruns once.
   - The whole suite, `cargo nextest run --workspace`, last, so its red hides none of the checks
     above it (theseus-zsddv). A phase that fails ends the gate, so when one does, the locked part
     prints `gate: NOT RUN, no verdict, after the failure in <phase>: <phases>` naming what never
     ran, and still reports the compile note. Before it the gate says what the suite will not run
     (theseus-4vsrz): `gate: suite filters: none`, or each `default-filter` it finds in `.config/nextest.toml` (its
     profile, line and expression, and whether HEAD has it or only the working tree). A line under this run's profile
     (or under `[profile.default]` while the run's profile has none) is ACTIVE: the phase is called `suite filtered` in
     the table. A line parked under another profile is named and marked `NOT this run's`, and the phase keeps its name.
     The suite passes no `-E`, so a default-filter is the only filter there is, and nextest counts what it skips with the
     `#[ignore]`d tests, so a stale line was invisible in the result. The gate never refuses one.

   The benches run the test build's `theseusd`, `theseus-sim`, and `theseus-index`: the workspace's features, which
   the `features` phase holds an install's to.
3. *Without it again*:
   - `cargo deny --offline check`: licences, advisories, bans, and sources. Offline: advisories come from the database
     as its last fetch left it (a gate that fetched failed when GitHub or crates.io did, and once when a crate was
     yanked between two gates), and the gate says when that database is more than 7 days old. The database is the
     gate's pin (theseus-4vsrz): only the owner's `cargo deny fetch db` moves it, and `deny-daily.sh` keeps a database
     of its own, so a fetch never reddens a gate.

It ends with `gate: ok`. Each step runs under `phase`, which times it: the gate prints a table of seconds before it
ends, a failed run's too (with `<- failed here` on the phase that stopped it, and `gate: FAILED in <phase>`; a signal's
too, after `gate: stopped by a signal`), and appends it to `~/.cache/theseus/gate-times.csv` (time, label, phase,
seconds, status; `$THESEUS_GATE_TIMES`). The gate's own cost has a history now: read it before calling a gate slow.

### A tree that moves under the gate (theseus-4vsrz)

The gate compiles one tree and then runs the tests of whatever tree is there. On 2026-11-07 a builder detached a join's
worktree and restored it inside the join gate's window: the gate reported four red tests that passed 15 of 15 on the
identical commit, a false result that looked exactly like a real one. So the gate reads the tree's identity before anything
runs (`tree_begin`) and checks it again before the lock, when it takes the lock (the wait can be hours), before the suite,
and at the end; a gate that fails is checked once more on its way out.

- **A tree that moved ABORTS the gate with exit status 75**, never a test failure (nextest's own is 100, the gate's 1 and
  2). The log says `gate: ABORTED, THE TREE MOVED (<where>)`, the tree it built and the tree it found (the label and the
  identity), what differs (HEAD, the paths whose state changed, HEAD's reflog entries since the start), and ends
  `gate: ABORTED`, where a failed run says `gate: FAILED`. No test result of an aborted run is a verdict: rerun it, on a tree
  nobody touches. `arm.sh result` shows it as `gate exit=75`.
- **The identity** is the commit, a digest of the edits beyond it (tracked and untracked: `tree-stamp.sh`'s digest, minus
  two paths (`cockpit/src/protocol.gen`, `bench/smoke/runs`), computed inside `gate.sh` because the gate must not depend on a file of the tree it judges) and the length and
  digest of HEAD's reflog. HEAD alone misses an edit, and the stamp alone misses a detach and a restore (the same commit
  and the same files after it); the reflog writes a line for each. Both are left out: the
  protocol phase rewrites `cockpit/src/protocol.gen` itself and fails saying `git add it` (that stays a failure, exit 1), and
  `scripts/release-smoke.sh` writes a run's report under `bench/smoke/runs` (tracked, so not gitignored), and a smoke run beside a gate is no move.
- **What it cannot see:** an edit made and undone inside the window (no ref moves, nothing is left), and a move in the
  seconds between two checks that is undone before the next. The guard is the rule beside it: never edit, check out,
  reset or build in a tree a gate is running in (the join's worktree is the install tree and nobody else's workspace).
- **A gate that rewrites its own expectations aborts too:** `THESEUS_GOLDEN=write` under the gate changes tracked files,
  which is a moved tree, correctly (a test that rewrites its golden proves nothing). A stale `Cargo.lock` does the same:
  the first cargo rewrites it, and the log names it among the paths.
- **`CARGO_INCREMENTAL=0`** from the gate's first line, for all of its cargo, unless the caller sets its own value (an
  edit loop that wants the cache says `CARGO_INCREMENTAL=1`). A lane is built once and gated once, so an incremental cache
  is paid for and never reused: 47 GB of one lane's 68 GB target was `debug/incremental`. Flipping it half way builds a
  second complete set of artifacts, so a caller wraps the whole run, never part of it. The first gate in a warm lane
  that was built with it on builds once more; `rm -rf target/debug/incremental` after it.
- **`scripts/test_gate_tree.py`** holds all of this: it runs the real gate in a throwaway repository with stand-in cargo,
  node and npm (`$HOME/.cargo/bin` first on the gate's PATH is where they go) and moves the tree after the test build,
  inside the suite, and by a checkout away and back. The gate's `gate tests` phase runs it, after `scrub`, when the tree
  differs from main in `gate.sh` or the test; a gate of main's own tree says so and costs a diff. A
  new phase of the gate that reads a command's output needs its stand-in there.

### Running the gate on a busy machine: in `scripts/GATE.md`

**Five sections stood here and were moved verbatim to `scripts/GATE.md` on 2026-11-10** (theseus-4vsrz, B7),
to get this file back under the context loader's 64 KiB ceiling with room to grow. They are, by name:
**A disk too full to build** (`THESEUS_GATE_MIN_FREE_GB`, and why the gate deletes nothing itself), **The
lock** (the shared machine-wide lock, who holds it, and never wrapping the gate in `flock` or
`theseus-quiet.sh`), **The busy allowance** (`THESEUS_GATE_BENCH_ALLOWANCE`, and a bench that misses under
a neighbour's load), **How long it takes**, and **Running it beside other work**. Nothing was cut: if you
are looking for a paragraph that used to be here, it is there, byte for byte.

### The keel guard

(theseus-pw1q.1; the owner's decision on self-improvement, 2026-10-09: deleting or loosening a test, a budget or a
ceiling needs the owner's yes.) `scripts/keel-guard.py` (standard library only) judges a range's net change and prints
each erosion as `path:line: rule: old -> new`; its header names every rule and every file it watches:
`test-removed`, `test-ignored`, `assert-removed`, `assert-weakened` (an assertion loosened in place, which keeps the
count: an equality made a truth test, a constant bound loosened, an assertion made vacuous, commented out or
discarded), `allow-added` (an `allow` or an `expect`, a lint level, a clippy.toml
threshold), `budget-raised` (the lifecycle, turn and jobs benches' limits, the busy allowance's default), `cap-raised`
(bench/'s spend and loop caps, the config template's spend and day lines, and a cap's code default: a `fn default_*`
or a constant that names a spend or loop cap), `ceiling-raised` (long-files.txt, shape.sh's limit, nextest's retries,
timeouts and overrides), `ci-loosened` (a CI step's command gone or changed, `continue-on-error`, an `if:` that is
false) and `keel-file` (the guard itself, its door, a gate phase gone). The test rules read Rust, Python and the
cockpit's TypeScript tests (`node:test`'s `test(...)`, its `.skip`/`.todo`/`.only` and `{ skip: }` options, and
`assert.*` calls).

- **Two halves, and a non-keel file is not exempt (theseus-4vsrz).** A brief or a builder must say which it means.
  (a) *Where*: `KEEL_FILES`, `KEEL_PREFIXES` and `LOCKED_FLOW_PREFIXES` in the guard name files whose edit needs an ack
  whatever it does (`keel-file`, `locked-flow`). (b) *What*: the erosion rules (`test-removed`, `test-ignored`,
  `assert-removed`, `assert-weakened`, `allow-added`, `budget-raised`, `cap-raised`, `ceiling-raised`, `ci-loosened`)
  judge what the change does, in ANY file: a file outside `KEEL_FILES` that loses one assertion fails the gate in
  about a second, before a test runs. Clearing (a) says nothing about (b), so "the file is not keel-class" never means
  "no `Keel:` trailer is needed". A ruling that deletes or weakens an assertion comes with the owner's quote, or is
  marked as needing one. Never ack across a batch boundary: when a finding names a file the change never touched,
  `git grep` the symbol at the base and at the head first, and judge at the branch's own base (`--base $(git
  merge-base <base> HEAD)`): a base that is not an ancestor of HEAD reads what the base's side added as removed.
  `scripts/keel-batch-repro.sh` shows a real `test-removed` re-attributed to an unrelated file in a batch.
- **The range.** Main is `origin/main` (the local `main` only where there is no `origin/main`): a join's local main is
  ahead of it by exactly what the join's gate judges. On a branch, merge-base(HEAD, main)..the working tree, so the gate
  before a commit judges what it will hold, and a join whose fix is a commit after its merge is judged whole (until
  theseus-pw1q.18 the local main won there, and the range was HEAD..HEAD: the people join's gate judged nothing). On a
  join's merge commit not yet on origin/main, HEAD^1..HEAD; a branch cut at a merge already on origin/main is judged
  from it. `THESEUS_KEEL_BASE=<rev>` names the base. The gate finds the base in its own shell and runs the base's copy
  of the guard, so a branch's copy decides neither. There is no switch to skip it.
- **The door** (theseus-pw1q.18). A join runs the merge's `gate.sh`, which is the branch's, so a branch that dropped
  its keel phase (or any check) was judged by its own weakened copy. Now the gate's first phase, `door`, compares the
  running copy with main's (`origin/main:scripts/gate.sh`): when they differ and main's has the door, main's copy runs
  first, from `$TMPDIR/theseus-gate-door-<blob>/scripts/gate.sh` (so what knows a gate by `scripts/gate.sh
  --locked-part` in its argv, `review-step.sh` among them, knows it) with `THESEUS_GATE_ROOT` naming this tree, and judges it with
  main's keel; if it fails, the gate fails `in door`. Then, only when the branch itself changes `gate.sh`, the tree's
  own copy runs on, so its new checks are proved before they land; otherwise the gate ends `gate: ok (main's gate
  judged this tree)` (a lane behind main's gate.sh is judged by main's alone). The guard calls any change to the
  door's lines, or a phase gone from `gate.sh` (by name), a `keel-file` finding.
  The door in the tree's copy holds only while that copy is honest: a branch could delete the door too. **A caller that
  trusts nothing in the tree (a join, and Theseus's own self-join) starts main's copy itself**, which judges the tree
  and then runs the tree's own changed copy after it:

  ```bash
  blob=$(git rev-parse origin/main:scripts/gate.sh) && git cat-file blob "$blob" >"/tmp/gate-$blob.sh" &&
    THESEUS_GATE_ROOT="$PWD" bash "/tmp/gate-$blob.sh"
  ```

  `THESEUS_GATE_DOOR` is the door's word to the copies it starts (`judge`, `own`), not an option.
  **A caller that names the keel's base owes the door both halves itself.** "Does this branch change `gate.sh`?"
  is asked through the keel's base (`keel_gate_changed` diffs `scripts/gate.sh` against `keel_range_base`), so a
  caller that sets `THESEUS_KEEL_BASE=HEAD` leaves that question answered "no" in every clean checkout: main's copy
  judges the tree, the branch's own copy never runs a phase, and the gate ends `gate: ok (main's gate judged this
  tree)` for a branch that rewrote the script under test. A fleet box does name that base -- it holds no keyring, so
  it cannot verify an owner's `Keel:` ack and would refuse a legitimate one (`~/reports/theseus-fleet/host/run.sh`,
  the gate kind) -- and so its runner asks the door's question where no keel base can reach it, comparing
  `git hash-object scripts/gate.sh` with `origin/main:scripts/gate.sh` exactly as `gate_tests` does, then running
  `THESEUS_GATE_DOOR=judge` (main's copy, from `/tmp`, `THESEUS_GATE_ROOT` naming the lane) and
  `THESEUS_GATE_DOOR=own` (the tree's own copy) in turn, and naming the blob of the script that ran in the one-line
  summary of every gate. A caller that names a base and leaves the door to the predicate proves nothing about a
  change to `gate.sh` (found on a fleet box, 2026-11-10;
  `~/reports/theseus-105-kickoff/logistics/fleet-gate-door-fix.md`).
- **The ack.** A finding passes only when a commit in the range, signed by a key that `scripts/keel-signers` lists
  at the base, carries `Keel: <rule> <path>[, <path>…] — <why>` (a path may be a glob; the gate prints the exact
  lines to fill in). An unsigned trailer, or one signed by any other key, counts for nothing and is said so. Only a
  GPG key's signature counts: an ssh or x509 one is refused by name even where it verifies (an ssh signature verifies
  against whatever `gpg.ssh.allowedSignersFile` the machine names, and its SHA256 fingerprint would match a listed
  one; theseus-pw1q.18). At a
  join the owner's signed merge commit carries the trailers in its message.
- **Moved and renamed tests** are the same test: matched by name, then by body, then a test that went and one that
  came at least half alike (difflib over their words; the guard prints each pairing). An assertion made a wait
  (`wait_for(`, `.until(`) keeps its check.
- **A pairing found only by similarity, landing in another file, is a GUESS, and in a batch it misattributes
  (theseus-4vsrz).** Measured at `12889bcf` with `scripts/keel-batch-repro.sh`: the deleted routing test and the
  unrelated new voice test are paired because their bodies are **0.63 alike** by difflib over their words, over
  `SIMILAR`, which is 0.5. So "the pairing crosses files whose contents share nothing" is the wrong diagnosis, and a
  fix that refused a share-nothing pairing would not catch this one: the two bodies share their whole skeleton. What
  goes wrong is downstream of the pairing, in what the pair is then judged to prove. The finding changes **rule**,
  `test-removed` to `assert-removed`, and **file**, from the one that held the deleted test to the paired one, which
  eroded nothing — so a trailer that named the real file stops covering it, and the guard's own remedy text asks for
  a trailer against the innocent file. Writing that trailer would be a false statement that the owner authorised an
  erosion where none exists; never write it. When a finding names a file your change never touched, `git grep` the
  symbol at the base and at the head first: here it is in two files at the base and in none at the head, which
  proves the erosion real and its attribution false.
- **`scripts/keel-batch-repro.sh` takes a candidate guard as its one argument** (`scripts/keel-batch-repro.sh
  path/to/keel-guard.py`, default the tree's). That is the seam for judging a proposed fix to the pairing without
  editing the guard, which is `keel-file` and needs the owner's signed yes.
- **Its suite**, `scripts/test_keel_guard.py`, builds a throwaway repository per case (signed cases with throwaway
  keys in their own `GNUPGHOME`, and the real `gate.sh` run in a throwaway repository for its range and its door); the
  gate runs it when the guard, the suite or `gate.sh` differs from the base. Run it alone
  with `python3 -m unittest scripts/test_keel_guard.py`.

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

### A file that fell under 2,500 and needs its ceiling entry back (theseus-4vsrz)

A row that grows a Rust file with **no** `long-files.txt` entry past 2,500 lines — whether its entry was deleted
when it shrank, or it never had one — does exactly what a row raising an existing ceiling does: **one commit that
grows the file and writes the entry together, with the `ceiling-raised` trailer.** There is no two-step land, and
the claim in circulation that such a file "cannot get its entry back in one step" is **false**. Measured at
`42b9212e` by growing `crates/theseus-protocol/src/lib.rs` from 2,497 to 2,617 lines with filler comments and
reverting it, three scenarios over the same tree:

| the tree as it stands | `bash scripts/shape.sh` says | exit |
|---|---|---|
| grown to 2,617 **and** `crates/theseus-protocol/src/lib.rs 2700 <reason>` added, **together** | `shape: ok (no file over 2500 lines but the 6 listed)` | **0** |
| grown to 2,617 only, no entry | `shape: crates/theseus-protocol/src/lib.rs has 2617 lines, over 2500, and no entry in scripts/long-files.txt: split it, or list it with a ceiling and the reason` | 1 |
| the entry added only, file still 2,497 | `shape: crates/theseus-protocol/src/lib.rs has 2497 lines, at or under 2500: delete its entry in scripts/long-files.txt` | 1 |

The cause is `shape.sh`'s own control flow: it measures the tree **as it stands after the change** (`wc -l` over
`git ls-files --cached --others`), so the "at or under 2,500: delete its entry" refusal at `shape.sh:38` is reached
only while the file is **still small**. Grow it in the same change and that branch is never taken; the `n > c` test
at `:41` then compares the new length against the new ceiling and passes. **The two separate orders are the only
ones that red**, so a two-step land means a deliberately red intermediate commit — which "one commit per green
sub-step" forbids and which a later bisect walks into. One commit, or nothing.

What does bite is the keel guard, once:

```
scripts/long-files.txt:12: ceiling-raised: (no entry) -> crates/theseus-protocol/src/lib.rs 2700
```

`(no entry) -> <path> <ceiling>` is the guard's own rendering of a **re-founded** entry; an existing entry raised
renders as `<path> <old> -> <path> <new>` (measured on `thread_route.rs` 2600 -> 2700). **Same rule, same trailer,
same exit 1** — which is the whole argument that the two cases are one case and must not be given two procedures.
The finding is a property of the **list alone**, not of the file: it is byte-identical whether or not the commit
grows the file (scenarios 1 and 3 above print the same line, which is why the entry-only step of a two-step land
buys nothing — it reds twice). The trailer that clears it is the one `turn.rs` has been raised under four times:

```
Keel: ceiling-raised scripts/long-files.txt — <the owner's authorising words>
```

For our own development limits those words are the owner's standing ones of 2026-10-25, quoted in `long-files.txt`
beside the `turn.rs` and `thread_route.rs` reasons. The commit body names the file's length before and after, the
ceiling and why, and why the lines could not go in a sibling module — the same body `turn.rs`'s four raisings carry.

**Deleting an entry is the opposite act and needs no trailer.** A listed file whose test module moves out and falls
to 2,500 lines or fewer MUST lose its entry in the same commit — `shape.sh:39` requires it — and the guard says
`keel: ok` on the deletion (measured: `thread_route.rs`'s entry removed, `keel: ok ... 0 findings`, exit 0). A
ceiling going away is not erosion. This direction is also the one `docs/design/invariant-merge-class.md` puts out
of the merge class: no sibling branch can shrink a file the shrinking branch did not shrink.

**A row that cannot sign** — every cloud row, and any agent without a key in `scripts/keel-signers` — does not
apply the grow-and-list change at all, because the guard fails the gate in a second and the trailer cannot be made
true. It puts the new lines in a sibling module instead (`crates/theseus-protocol/src/work.rs` and its siblings are
where protocol types go; three lines is one struct field), or, where that is genuinely impossible, writes the hunk
and the draft trailer into its report with the owner's words left blank, and lands everything else.

**The two files in this position at `42b9212e`**, so the rule has a referent: `crates/theseus-discord/src/render.rs`
at **1,327 lines**, whose explicit 3,009 ceiling was deleted when its 1,643-line test module moved out to
`render/tests.rs` at `5e71c15a` (it was 2,970 of 3,009 before), so its effective ceiling is now the default 2,500;
and `crates/theseus-protocol/src/lib.rs` at **2,497 lines**, three under the default and never listed.

**Rejected, with the reason.** A `shape.sh` flag for an entry being re-founded: it buys nothing, because `shape.sh`
already passes the case at exit 0, and both `shape.sh`'s keel lines and `keel-guard.py` itself are keel-class, so the
complexity would cost an ack as well as a reader. A deliberate refusal: there is nothing to refuse — the act is
legal at both guards, has a four-times precedent in `turn.rs`, and already has its trailer. A rule that made these
two near-identical acts behave differently is a rule the next row would get wrong.

### The daily deny job

`deny-daily.sh` is the fresh half of two contracts (theseus-goa8; theseus-4vsrz). The gate's `cargo deny --offline` judges
the tree against the database in `~/.cargo/advisory-dbs`, which only the owner's `cargo deny fetch db` moves, so main is
never red tomorrow with no commit in between. This job fetches into its own database (`THESEUS_DENY_DB`, default
`~/.cache/theseus/deny-daily-db`, reached through a generated config that sets `db-path`), so its fetch never changes the
gate's. It fetches that database and the registry index, runs the whole `cargo deny check` against them, and, when that fails, files one
Beads issue (owner `main`, waiting for an available agent, labels `security` and `deny-daily`) unless one is already open.
A failed fetch is an outage, not a finding: it checks what is cached and files nothing. Exit status: 0 all clear; 1 the check
failed; 2 the fetch failed and the check was clean on what is cached; 3 the `deny.toml` in use (the tree's, or
`THESEUS_DENY_CONFIG`) has no `[advisories]` table to point at the job's database, so nothing was fetched or checked (the
unit shows as failed). It reads the repository and never builds or changes it
(`--locked`), so it runs in the chain's tree. Its log is `~/.cache/theseus/deny-daily.log`.

The gate's offline check is where a move of its pin shows up: that database moves only when something fetches into it (the
owner's `cargo deny fetch db`; never a branch, never this job), so a red `cargo deny` on an untouched tree means the pin
moved. When bans, licences and sources are clean and only advisories fail,
the gate says so (`deny: ONLY advisories failed ...`) and names the owner's action: ignore each id in `deny.toml` with its
reason, or bump the crate. The gate never fetches and never writes an ignore.

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
- **A forced order or a planted revert establishes a fix; a rate only corroborates.** Three times a confident rate
  did not survive a paired control: a fix lane against a plain-main control read 2/24 against 1/24 (p = 0.50), and
  only pooled across both control arms did it reach 5/48 against 0/48 (p = 0.03). Plan a deterministic plant first
  (force the bad order so the recorded failure returns every run, then make the order not matter), and quote rates
  as corroboration.
- **Five passes alone are underpowered.** A true 10 % rate shows nothing in 5 runs 59 % of the time, and in 20 runs
  12 % of the time. A test that "passed 5 of 5 alone" has not been cleared, and alone is where a load race passes.
- **A wrong answer is never a flake.** A count off by one, or a state that reads the wrong value, is a bug whatever
  the load: it goes to a fix, never onto the list. A timeout under load is a different thing. Instances, by test
  name (commit ids change at the re-sign): `tests_security::a_press_with_a_call_labels_its_judgments_in_its_frame`
  (a store-wide frame counter beside a live judge) and
  `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` (order forced, then a seam in the rig).

- **The `timing` group is a different instrument.** A test that measures a small time under load goes in
  `[test-groups.timing]` (`.config/nextest.toml`), with an override that names it, and the group runs its members one at
  a time. It sets no retry: no race is hidden, and a failure still fails the gate. `cargo nextest show-config
  test-groups` lists its members, so nothing here counts them.
- **`flakehunt` is a profile the gate does not run.** `[profile.flakehunt]` in the same file is the default profile with
  `retries = 2` and `flaky-result = "fail"`, so a test that passed only on a retry fails the run. It is for a whole-suite
  hunt by hand (`cargo nextest run --workspace --profile flakehunt`); the gate runs the default profile. Since
  2026-11-10 a nightly run of it exists as well: a user timer on the main machine fires at 03:05 local and sends
  `origin/main`'s tip to an ARM lane for the whole suite. That timer, its unit and its script are not in this
  repository, so this file cannot say whether it is installed and does not try to; `systemctl --user list-timers
  theseus-flakehunt.timer` answers that.

### A test of the hour or the local day takes a test clock

A gate runs for minutes from any start, so a suite can walk through midnight or the top of an hour (theseus-4vsrz:
two tests of the local day failed a gate that began at 23:41, and one of the hour a gate that ran across 13:00). A test
whose answer depends on the hour's or the day's turn starts with `let _clock = crate::test_support::pin_mid_period();`
in theseus-core: the clock (`theseus_protocol::time::move_clock_to`, which `now_unix_ms` and the kernel's `RealClock`
both read) is moved to the middle of a UTC hour near noon of the real local day, so its seconds cross neither, and the
guard gives the wall clock back. The move is the process's: nextest, which the gate runs, gives each test a process of
its own. A new test that takes `local_day(now)` or an hour's start from the clock, and then reads rows stamped after
it, takes the pin; the gate no longer relies on a start window to keep it from the boundary.

## setup.sh

(theseus-00me.) One command, from a checkout to a running daemon; `docs/setup.md` is its how-to. What to keep true:

- **`--dry-run` changes nothing, and runs nothing but the preflight's reads.** In a dry run it never runs `theseusd`, and
  never connects to a socket: on this machine the default socket is the operator's daemon. It prints `user-service.sh
  --dry-run install` under its own step 6.
- **It never writes a secret.** A new config is the template's sparse cut, and before it is written every `[secrets]`
  value must be an `op://<…>/<…>/…` placeholder and no run of 40 or more token characters may appear. It never overwrites
  a config: it says which keys differ from the template (`flat` and `differs`), never a value. Output from `theseusd check`
  and `theseus health` goes through `mask`.
- **Idempotent.** Binaries are compared with `cmp` and only a differing one is copied and renamed; `npm ci` is skipped while
  `node_modules/.package-lock.json` is newer than the lock file, and `npm run build` while its `dist/index.html` is newer
  than every file in `cockpit/`; cargo decides the rest. The service step does nothing when the unit matches its plan
  (`theseusd install --user --check`), is active, and answers; it restarts when a binary changed; else it runs
  `user-service.sh install --yes`.
- **`--unit NAME` needs `--state-dir` and `--socket`** (a usage error without them), for the same reason as above.
- **The Jev key's question** (theseus-is8t). A new config is written after one question, asked on a terminal and
  without `--yes` only: the key's reference, `op://<vault>/<item>/<field>` or `env:NAME` (`jev_reference_ok`; anything
  else, a key typed by mistake included, is refused and asked again, three times, then taken as a skip), written into
  `[secrets]`'s `jev_api_key`. An empty answer removes the entry (so the judge, which follows its key, stays off, and
  the run says so); with no question the placeholder stays. `--jev-key REF|skip` answers it in advance; the value is
  never on the command line or in the config. Its tests drive a pty from `python3` (`run_on_a_terminal` in
  `crates/theseusd/tests/setup_script.rs`), and like the others they skip as root or without cgroup v2: to run
  them on a root VM, run the test binary as `nobody` (`setpriv`) with the preflight's cgroup line bypassed in a scratch
  copy of the script, never in the tree.
- **Exit status 3** is a stop for the operator (placeholders in `[secrets]`, or no token file), not a failure: an agent
  running it reads the last lines.
- **Its proof** is a scratch run: `--prefix`, `--config`, `--state-dir`, and `--socket` under one `/tmp` directory,
  `--unit theseus-scratch-<name>`, a config with a stand-in model (a `file:` key) and `[discord]` off, a web port that
  `ss -ltn` shows free, and a stand-in token file (mode 0600). Run it twice (the second changes nothing), then
  `user-service.sh --unit <name> uninstall --yes` and delete the directory. Never with the defaults on this machine,
  except `--dry-run`.

## Releases, the service, the smokes, and this machine

`scripts/RELEASES.md` -- moved out of this file on 2026-11-10 (theseus-4vsrz, B7) to get this one back
under `context_files::MAX_BYTES`, verbatim and byte for byte. It holds: **Releases** (the pinned toolchain,
`build.sh`, the reproducibility check), **`user-service.sh`**, **`smoke.sh`**, **`release-smoke.sh`**, and
**This machine** (the WSL host's traps: the disk, sccache, the gate lock, `pgrep -f`, `/tmp`). **`setup.sh`
above stayed here** on purpose -- `RELEASES.md`'s own header says why. Nothing was cut: if you are looking
for a paragraph that used to be here, it is there. `scripts/RELEASES.md` is not a context file, so it has
no 64 KiB ceiling and is the right place for depth that would otherwise push this file over.
