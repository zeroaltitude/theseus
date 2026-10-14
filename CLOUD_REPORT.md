# CLOUD_REPORT: size-rss-budgets (theseus-2m7jg)

Branch `cloud/20261013-size-rss-budgets`, built on main as cloned (store format 32, untouched by this row). 4 cores,
15 GB, root, no sccache. Started 04:13, finished about 06:00 (UTC, 2026-10-14).

## What I found

- `bench size --check` existed and nothing ran it. The release build is not made by the gate (debug only), so the only
  place a release binary exists is `scripts/build.sh`, which ended in `exec cargo build`.
- **`theseusd` is 83.12 MB** (`release-thin`, this tree, 2026-10-14; 83.03 MB stripped by `strip --strip-all` from the
  unstripped build below). The other binaries: `theseus` 9.04 MB, `theseus-tui` 3.13, `theseus-sim` 9.02. §9's 60 MB is
  missed by 23.12 MB.
- Idle RSS of that daemon on the bench config is 41.2 MB (matches "40 MB at start").
- A defect of my own, caught by its test before commit: matching the baseline by "label contains `size:release`" also
  matched `size:release-thin`. Now a whole word of the label.

## Step 1: the check where a release binary exists (commits 8f2e0b5 sim, d0abe94 scripts)

- `scripts/build.sh` builds, then writes `target/<profile>/.theseus-build-stamp`, then runs
  `theseus-sim bench size --check --dir target/<profile> --record <history> --label "<branch> <describe> size:<profile>"
  --baseline-tag size:<profile>` and fails the build when it exits 1. `THESEUS_SIZE_ACCEPT_GROWTH=1` passes
  `--accept-growth`. A `-p` build with no `theseusd` and `theseus-sim` says "size check skipped" and builds as before.
- **How a binary proves its commit.** The binary has no hash of its own that the gate can read cheaply
  (`THESEUS_COMMIT` is in the daemon, and only `theseusd` carries it). So the proof is the stamp:
  `scripts/tree-stamp.sh` prints `<HEAD sha> <sha256 of git diff HEAD --binary plus the untracked, unignored files, 16
  hex | clean>`, build.sh appends theseusd's byte count, and the gate recomputes the same string for the tree under
  test and `stat`s the binary. Equal strings mean this commit and these edits, built by build.sh. Any edit after the
  build changes the digest, so the gate never judges a stale binary (it skips instead).
- The gate gets one phase, `size` (`size_step` in gate.sh, which calls `scripts/size-step.sh <history> <label>`), last in
  `machine_checks`, after `turn`. It runs in a lane's gate too (a size is a count, not a time). With no stamped release
  build of this tree it prints, and returns 0:
  `size: skipped: no release build of d0abe94+edits (scripts/build.sh builds and stamps one)`.
- Timed: the whole of `size-step.sh` including `tree-stamp.sh` and the check, running: **0.048 s** (limit 1 s). Skipping
  costs the `tree-stamp.sh` (git diff + sha256, a few ms). build.sh's added check is one 83 MB `stat`: well under 1 s.

## Step 2: growth fails, the 60 MB line prints (8f2e0b5, `perf/budgets.rs`)

- `--check` now exits 1 only when a binary is **more than 1 MB over the last recorded size** (`budgets::GROWTH_MB`).
  The baseline is the last history row that **passed** for the same `--baseline-tag` word (a build profile), so a
  failed build is never the baseline of the next one: the same bloated binary fails again until it shrinks or the owner
  runs with `--accept-growth` (records the row as passed; one accepted growth becomes the new baseline).
- With no baseline (first run of a profile, or no history) it says "no recorded size of this build to compare with;
  this run becomes it" and passes. Today the owner's history has size rows with no `size:<profile>` tag in the label, so
  the first tagged build records the baseline and the second one is the first to be judged.
- §9's 60 MB is printed per binary: `size_theseusd: 83.12 MB is 23.12 MB over §9's 60 MB target: a miss, not a failure
  (only growth fails)`. `BINARY_MB` is still 60.0; `run_size`'s verdicts and its test `sizes_are_judged_against_sixty_megabytes`
  are unchanged. The history's `limit` for a size column is now the growth limit (last size + 1 MB), the thing the check
  judges, and `passed` is the growth verdict. `warns_near` leaves `size_*` out, since that limit is always within 10%.

## Step 3: the breakdown (commit after the gate: `scripts/size-breakdown.py`)

`scripts/size-breakdown.py [--profile release-thin] [--top 20]`, standard library only. It builds
`theseusd` with `CARGO_PROFILE_RELEASE_THIN_STRIP=false` into `target/size-breakdown` (so the stripped binaries and their
stamps are untouched), uses `cargo bloat --crates` where installed (I installed it for this) and otherwise `nm -S
--size-sort -C` grouped by first path segment, and prints the cockpit bundle. Shares are of the stripped size. Today's
table (`release-thin`, this commit, 108,020,744 bytes unstripped, **83,030,008 stripped**):

| crate | MB | share |
|---|---:|---:|
| theseus_core | 17.06 | 20.5% |
| std | 4.80 | 5.8% |
| theseus_discord | 4.24 | 5.1% |
| twilight_gateway | 2.40 | 2.9% |
| songbird (vendored) | 1.67 | 2.0% |
| theseus_protocol | 1.64 | 2.0% |
| theseus_kernel | 1.27 | 1.5% |
| similar | 0.91 | 1.1% |
| lopdf | 0.88 | 1.1% |
| davey (DAVE, voice) | 0.87 | 1.0% |
| serde_json | 0.80 | 1.0% |
| redb | 0.78 | 0.9% |
| tokio | 0.70 | 0.8% |
| reqwest | 0.66 | 0.8% |
| theseus_judge | 0.66 | 0.8% |
| h2 | 0.52 | 0.6% |
| theseus_store | 0.52 | 0.6% |
| toml | 0.52 | 0.6% |
| rustls | 0.51 | 0.6% |
| theseus_tools | 0.51 | 0.6% |
| **[not in symbols]** | **29.17** | **35.1%** |

The last row is data no symbol covers: `.rodata` is 20.26 MB (of the stripped file: `.text` 54.39 MB, `.rodata` 20.26,
`.eh_frame` 4.07, `.rela.dyn` 2.13, `.data.rel.ro` 1.50). In `.rodata`: **the embedded cockpit bundle,
`crates/theseusd/cockpit/dist`, 438 files, 13.83 MB**, the AWS catalog's `aws-catalog.bin` (2.2 MB, `include_bytes!`), the
CloudFormation templates, and string tables. So the embedded cockpit is the second-biggest single item after
theseus_core, and bigger than voice and AWS code together. Candidate cuts I did not make: compress the bundle (rust-embed
`compression` needs a dependency; or build-time gzip with the standard library in a build script, served with
`Content-Encoding`), and look at theseus_core's 17 MB (nm by module: `rpc` 4.2, `aws` 2.7, `config` 1.2, `toolrun` 0.8,
`judge` 0.8).

## Step 4: the soft RSS line (8f2e0b5)

`bench turn --check` with a burst (`--burst > 0`) compares `rss_burst` with the **median of the last 5 recorded
`rss_burst` readings** in the history (`--record`'s file, else `$THESEUS_BENCH_HISTORY`, else the default file). More than
10% over prints
`WARN rss_burst 110.3 MB is 100.7% over the median 54.9 MB of its last 5 recorded runs (soft line: 10%; never fails)`
and records the percentage in a new history column `rss_burst_over` (written only on a warning; `OTHER` is 29 columns now,
and the history writes a new header by itself when the columns change). It never touches the exit status. The gate's
lane runs (`--burst 0`) cannot warn, since a burst is what it measures; the join's run (burst 30, `--record`) can.

Live: seeded a history with `rss_burst` 54.94 MB (half of a real run), then `target/debug/theseus-sim bench turn --check
--runs 3 --burst 10 --record /tmp/hist.csv`: WARN printed, row recorded with `rss_burst_over_p95` 100.71, exit 0.

## Step 5: Proposal, voice and AWS as on-demand helpers (not built)

**What is already a role.** `theseusd` already execs itself for `job-wrapper`, the L1 init, the MCP-in-L1 role, the file
converter, and `theseusd hand`. **`hand` is the Fargate/Lambda image's entry point, not a local helper**: the AWS
*callers* (`aws.call`, stacks, cost, the durability tender, bootstrap) run in-process in the daemon today, through
`theseus-aws`, and so does voice (songbird via `theseus-voice`, linked into `theseus-discord`).

**Savings in `theseusd` (from the breakdown, `release-thin`, stripped 83.03 MB):**

- Voice: songbird 1.67 + davey 0.87 + openmls 0.39 + libopus_sys 0.34 + theseus_voice 0.17 + rustfft 0.11 + hpke/k256 and
  friends ≈ **3.9 MB (4.7%)**. `twilight_gateway` stays: Discord text needs it.
- AWS: theseus_core's `aws` module about 2.7 MB (nm, by module name) + theseus_aws 0.31 + theseus_aws_guard 0.44 +
  catalog crate 0.07 + sigv4 0.06 + the catalog's data 2.2 MB + the CloudFormation templates ≈ **5.7 MB (6.9%)**.
- Together ≈ **9.6 MB, 12%**. That is not the 49-point growth since Oct 2 (28.9 → 77.85 MB): the cockpit bundle (13.8 MB)
  and theseus_core (17 MB) are each bigger than both helpers together, and I could not bisect the growth in the time
  (the history has no size rows of its own from Oct 2 on this VM).

**Idle RSS, measured** (release-thin daemon, the bench config, 25 s after start, two runs each, the index tender's ~10
MB child not included; `scratchpad` script, not committed). Note the bench config already has an `[aws.accounts.*]` table
with `durability = true` at a dead endpoint:

| config | daemon RSS, MB |
|---|---|
| no `[aws]`, no `[voice]` | 31.8, 33.3 |
| `[voice] enabled` only | 31.9, 33.8 |
| AWS account, `durability = false` | 33.8, 34.5 |
| AWS account, `durability = true` (the bench's) | 40.4, 39.7 |
| AWS (durable) + voice | 40.0, 39.6 |

So **configured voice costs nothing at idle** (songbird's driver is built at the first `/join`); its cost is on the call,
not the idle daemon. AWS costs +1 to +2 MB configured and **+8 MB with durability**, because the durability tender runs
for the daemon's life. A helper for AWS therefore removes the ~8 MB only if the durability tender moves into it, and then
the helper is long-lived whenever `durability = true` (so no idle saving for that owner).

**Cost.**
- Start latency: the same binary as a role costs `theseusd example-config` p50 3.1 ms / p90 3.8 ms, `theseusd hand` with
  no spec p50 3.9 / p90 5.2 ms (30 runs, warm), and 12.6 ms cold after dropping the page cache. A `/join` is dominated by
  Discord's voice handshake (hundreds of ms), so +4 ms (+13 ms cold) is invisible; the first AWS call already waits on
  STS/network, so the same.
- IPC: voice needs audio frames both ways (20 ms Opus frames, 48 kHz stereo PCM to the STT provider) and the gateway's
  voice state events, so a framed local socket or a pipe pair plus a small protocol, and the helper must hold the
  Discord voice state it gets from the gateway (the binding's, owned by the daemon). AWS needs a request/response
  protocol (call spec + SigV4 session names + response bodies up to the tool cap) and credentials handed over at spawn:
  the keys are harness-only (theseus-gh7), so they would cross a pipe to a child; the child must be started with the
  job's hygiene (no environment inherited, its own process group, the children.rs reaper).
- Crash containment: a voice crash drops the call and the daemon posts "voice ended" and carries on (today a panic in
  songbird aborts the whole daemon, since release is `panic = "abort"`); an AWS helper crash fails the in-flight call as
  `outcome unknown` (an attempt that may have run is unknown, AGENTS.md) and the durability cursor is already on disk.
  The price is two more things to supervise, a protocol to version, and the tests `tests/hands.rs`-style that exercise
  `kill -9` of the helper mid-call.
- The code stays in the same binary, so **the on-disk size does not fall** unless the helper is a second binary
  (`theseus-voice`, `theseus-aws`), which changes the install list (`build.sh` shipped list, `setup.sh`, the registry
  test) and the "five binaries" rule. A helper that is the same binary saves RSS only, not MB.

**The question to ask the owner:** the 83 MB is 12% voice and AWS and 17% the cockpit's bundle; moving voice and AWS
saves about 10 MB on disk **only as separate binaries** and about 1 to 8 MB of idle RSS (nothing for voice). Do you want
that (two more binaries and two protocols), or the cheaper first cut: compress the cockpit bundle (about 14 MB) and look
at theseus_core's 17 MB, before touching voice and AWS?

## Tests and proofs

- `cargo test -p theseus-sim --bin theseus-sim -- budgets history perf::tests`: 26 passed (6 new in `perf/budgets.rs`:
  growth 1.5 MB fails / 0.5 MB passes / exactly 1.0 passes, the baseline is the last passed run of the same profile word,
  the 60 MB miss prints a gap and is no verdict, RSS warns at +11% and is silent at +9% and with no history, the median
  is over the last five; one in `history.rs`: no "near the limit" line for a size).
- `cargo test -p theseus-sim --test size_step`: 2 passed (`tests/size_step.rs`, runs the real `scripts/size-step.sh`): a
  tree with no stamped release build, and one stamped for another tree, print `size: skipped: no release build of` and
  record nothing; a 77 MB binary passes with the 60 MB miss printed, +0.5 MB passes, +2.0 MB fails with `GREW: FAILED`,
  and the same binary fails again (the failed row is not the baseline).
- **Planted reverts** (each restored and `touch`ed, `git status` clean after):
  - growth verdict `ok: mb <= limit` → `ok: true`: `budgets::growth_fails_at_one_and_a_half_megabytes_and_passes_at_half_one`
    FAILED and `size_step::growth_over_a_megabyte_fails_and_the_sixty_megabyte_miss_does_not` FAILED.
  - RSS threshold `> RSS_WARN_PERCENT` → `> 20.0`: `the_rss_warning_fires_at_eleven_percent_and_is_silent_at_nine` FAILED.
  - skip line replaced by `true` in `size-step.sh`: `size_step::no_release_build_of_this_tree_is_a_skip_that_says_so`
    FAILED (and the growth test, which needs the output).
  - `--check` judged by `report.ok()` (the 60 MB) instead of growth: `size_step::growth_over_a_megabyte_fails_...` FAILED
    at its first, 77 MB run.
- Live: `scripts/build.sh --profile release-thin` built in 11 m 39 s, wrote the stamp, ran the check and recorded
  `size:release-thin` rows; `scripts/size-step.sh` then ran the check on that build (+0.00 MB: ok) and, after a stray
  untracked file, skipped with the line above.

## The live check for the maintainer

On the owner's machine, with the real history:

```
scripts/build.sh --profile release-thin       # builds, stamps, runs the check: the first tagged run records the baseline
scripts/build.sh --profile release-thin       # unchanged tree: "+0.00 MB: ok", exit 0
scripts/size-step.sh ~/.cache/theseus/bench-history.csv "x"   # prints the check if the stamp matches this tree, else the skip line
scripts/gate.sh                               # the `size` row of the phase table: skipped, or ok, in under a second
python3 scripts/size-breakdown.py --profile release-thin     # about 10 min the first time (its own target dir); prints the table above
target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --check --burst 30 --record ~/.cache/theseus/bench-history.csv --label x
                                              # WARN appears only if rss_burst is >10% over the median of the last 5 recorded
```

To see the failure: add 2 MB to a dependency or `dd` it onto the binary and run `target/release-thin/theseus-sim bench
size --check --dir target/release-thin --history <history> --baseline-tag size:release-thin` (exit 1, "GREW: FAILED").

## Keel findings expected

`python3 scripts/keel-guard.py` printed `keel: ok ... 0 findings` before every commit (the range is judged by a base copy
of the guard and my edits are additive), so there are no findings to ack. For the owner to know, in case the guard's
rules change the verdict, these are the edits to keel-watched or shared files, each additive:

- `scripts/gate.sh` (keel-file, shared file): one function `size_step` and one line `phase size size_step` in
  `machine_checks`. Nothing removed, no phase renamed.
- `scripts/build.sh` (shared file): the final `exec cargo build` became `cargo build` followed by the stamp and the check.
  The check fails the build where `exec` could not. This is the behaviour change the owner should hear about: a release
  build that grows a binary more than 1 MB now **fails** (with `THESEUS_SIZE_ACCEPT_GROWTH=1` as the accepting move).
- The growth rule itself is a decision the keel exists for: `bench size --check` stops failing on the 60 MB and starts
  failing on growth (stricter than "nothing", looser than "60 MB"). `BINARY_MB` is still 60.0 and the 60 MB is printed.
  If the owner wants the 60 MB to fail again, it is the `report.grew_ok()` → `report.ok()` line in `size_cmd`.
- No budget, cap or ceiling was raised; no test deleted, ignored or weakened; `scripts/keel-guard.py` untouched.

## Status short board (theseus-8kn12)

`bench status` could share this change: it is a release-binary measurement (p90 5 ms, "meant for a release-thin build on
the owner's machine"; a debug build reads 6 ms), so it belongs in the same stamp-gated phase, not in the debug bench
phases. One extra line in `size-step.sh` (`bench status --check --theseusd .../theseusd --theseus .../theseus` on the
stamped dir) would do it. I did not add it; my lines are only the size check.

## Docs the maintainer should change

- Spec Part III: an item for this row (the growth rule, the stamp, the `rss_burst_over` column, the breakdown table above),
  and §9's binary-size line: "60 MB is the target, growth over 1 MB fails the build". `docs/status.md`: the measured sizes
  (83.12 MB `theseusd` release-thin), and the roadmap row.
- `docs/benchmarks.md`: nothing from me (no benchmark run here).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run twice on the final tree contents (once before the
scripts commit, once before the last two). The keel, keel tests (92 ok), fmt, shape, features, clippy (`-D warnings`),
cockpit, test build and the reader rule pass. The suite phase fails, so the phases after it were run by hand:
`cargo deny --offline check` (advisories, bans, licenses, sources ok after `cargo deny fetch` and `cargo fetch`),
the generated-TypeScript check (`git diff` of `cockpit/src/protocol.gen` clean), `bench turn --check --runs 5 --burst 0`
(frames 4 / 8 / 14, all ok), and the new size phase (skipped / ok as above). Lifecycle and jobs benches are skipped by
`THESEUS_GATE_NO_BENCH`.

The second run: 4,389 tests, 4,355 passed, **34 failed**, none in code this branch touches:
- 33 in L1: `theseus-sandbox::contract` (clause_02..12, egress_18b, scratch, exit_status, sigterm) 19, `theseus-sandbox::bench`
  `spawn_100` 1, `theseusd::sandbox` 13. The known root-VM L1 refusal (theseus-pv6i).
- 1 outside the known list: `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
  ("timed out: the thinking's create, held, and the reply's post" after 20 s). Run alone it fails the same way (21 s), so
  it is not load. This branch cannot reach it: `theseus-discord` links `theseus-sim`'s library only
  (`fake_discord`, `fake_gateway`, `discord_proof`), and this branch changes only the sim *binary's* `perf` and `history`
  modules, scripts and a test. Not run on the base, so "pre-existing" is by that argument, not by a run.

The first gate run (before the idle benches and with the disk nearly full from the debug builds) failed 156 tests: the
same L1 set plus `theseusd` job tests (the daemon here refuses jobs: `ledger:job.refused` shows in the turn bench's tool
frames), `theseus-core` `tests_self_backlog` (5), `telemetry::tests_resumed` (3), `aws::tests_l1` (2) and
`theseus-sim::sim the_restore_rows_store_is_the_same_with_the_cancel_row_selected`; the second run passed all of those,
so they were load/disk effects of the first run (a concurrent release build was compiling), not this branch. I did not
rerun the first set singly.

Two environment notes: the disk allowance (about 39 GB) ran out once under the debug and release builds together
("No space left on device" in the first release build attempt); I deleted `target/debug/incremental` and my own
scratch target dirs only.
