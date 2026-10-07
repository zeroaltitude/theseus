# The Ship of Theseus, chapter 13: Part III, A4's Items 48 to 66 ([index](README.md))
### Item 48. The `bench2` lane: what runs, measured; the toolchain pinned; builds that repeat; an offline deny; a shape budget (theseus-goa8; Review 2's S4, S7, SC1 to SC3, C1, C7, and consideration 8; 2026-10-01 23:19 to 2026-10-02 09:25, two runs (the first ended at the usage limit at about 01:30); reviewed from 09:32; rebased onto c72baf8 as 9a0e602, 02dbee7, 0cd880c, 0a6805f, 94c753b, 18cca8d, 9e675c0, ede2add, 821e054, and 34be7e2; joined 09:42 at 34be7e2, a fast-forward; installed 14:23 with 9f4035b)

**Why.** Review 2, accepted on 2026-10-01 at 19:54, found that the benches measured the start and the stop and nothing
between them (S4, consideration 8); that every install paid for a fat-LTO build (S7); that the toolchain floated on
`stable` (SC1), the gate's `cargo deny` needed the network (SC2), and two builds of one commit differed (SC3); that
nothing held the code's shape (C1); and that the gate's own cost and its flaky tests went unrecorded (C7).

**What landed** (§1, §3.18, §9).
- **Measuring what runs** (9a0e602, 18cca8d): `theseus-sim bench turn` (a plain turn and a tool-call turn on the
  stand-in model, with the disk's `fdatasync` and the daemon's memory beside them), `bench idle` (a daemon with
  nothing to do, over 30 s: its CPU, wakeups, frames, and memory), and `bench size`. Frames per turn are counted from
  the WAL by a read-only tail, since the daemon reports none, and the plain turn's 5 are a gated budget in every
  gate, a lane's too: a count needs no quiet machine.
- **The install profile** (0cd880c, ede2add): `[profile.release-thin]` (thin LTO, 16 codegen units; cargo reserves
  the name `install`). The chain installs it, and a tagged release keeps `release`.
- **One exact toolchain** (0cd880c): `rust-toolchain.toml` pins 1.98.1, and CI installs from the file.
- **Builds that repeat** (0cd880c, ede2add): `scripts/build.sh` is the one release build. It builds the whole
  workspace (`--shipped` builds the four binaries, for looking), `--locked`, with embedded paths rewritten, the
  commit's time as `SOURCE_DATE_EPOCH`, and rust-embed's `deterministic-timestamps`. `scripts/repro.sh [--bench]`
  builds a commit twice and compares the binaries.
- **An offline deny** (0a6805f): the gate runs `cargo deny --offline`, and says when the advisory database is over 7
  days old. `scripts/deny-daily.sh` fetches and files an issue on a finding; its timer is not installed.
- **The shape budget** (94c753b, 821e054): clippy's `too_many_lines` and `cognitive_complexity` across the workspace,
  134 existing offenders marked `#[expect]`, `scripts/shape.sh`'s ceiling on a file's length, and
  `scripts/shape-expect.py`, which marks new offenders and removes the marks a change made unfulfilled.
- **Phase timings and the flaky list** (02dbee7, 9e675c0): the gate times each phase and prints the table, a failed
  run's too. nextest retries three named tests (theseus-i1i4, a2ec, 3dsz), and the gate prints and logs each that
  passed only on a retry. A new flake fails the gate until it is filed and listed.

**How it is proven.**
- **The frames budget**: one more frame planted in `Core::run` made `bench turn --check` read 6 against 5, name the
  extra `[meta]` frame, and exit 1. The plain turn's five frames match
  `tests_m3::a_plain_turn_stays_within_its_frame_budget` row for row.
- **The gate's changes, by planting**: an unformatted file stopped it in `fmt` with the table; a test that fails its
  first try passed as `1420 passed (1 flaky)`, printed and logged; with no network at all, the old `cargo deny`
  failed and the new step passed in 2 s, and an advisory not ignored still failed; a 114-line function failed
  clippy, an unfulfilled `expect` failed it, and `shape.sh` failed on four plants.
- **Reproducible builds**: two uncached builds of one commit, in two directories, gave identical binaries under
  `release`, `release-thin`, and the whole-workspace `release-thin`. rust-embed's timestamp was the cause (21 bytes
  differed in a one-file crate without the feature, none with it).
- **The install profile**: a cold build costs the same (10 m 47 s thin against 10 m 44 s fat), a rebuild after a
  change to the core is 2 to 6 times faster, `theseusd` is 23 % bigger (28.8 MB against 23.4 MB), and CPU-bound
  `kernel-sim` work is about 10 % slower. The daemon's own benches can't tell the two apart.
- **glibc or static musl**, measured on one commit: musl uses 16 to 25 % less resident memory and stops, restarts,
  and swaps 20 to 40 % quicker, but its allocator costs allocation-heavy work 36 % more user CPU and 3 to 5 times
  the system time. So the install stays glibc, which the gate and the live checks run (§3.18). _(The retrospective's report of these runs, 2026-10-06, Part III Item 220, reads them by pairs: release-thin's CPU cost is about 7 %, not 10 % from the least run; its rebuilds are 1.69 and 5.70 times faster; musl costs 28 to 67 % more CPU for 16 to 25 % less memory, and its lifecycle gain holds only for the swap.)_
- **Gates**: nine green before the rebase, one per commit, and gate 12 on the rebased tip, 1,471 tests. **The join's
  gate** at 09:42:12: 1,471 of 1,471.

**Divergences.** The profile is `release-thin`, not `install`. The review counted 36 shape offenders; there are 134,
since the tree grew and `--all-targets` reads the tests. `quinn` is only in `Cargo.lock`; what compiles twice is
songbird's `hls_m3u8` branch. `build.sh` builds the whole workspace, not the four binaries: cargo gives 29 of their
334 shared crates more features in a whole-workspace build, one of them a TLS root set (theseus-dn7p), so a
four-binary build is not the build that was tested. The profile's 10 % is more than "a few percent", and was adopted
anyway: the chain rebuilds after every reviewed step.

**Found on the way.** An idle daemon at 10,000 parked sessions never went quiet (about 5 % of a core with no frames
written): Review 2's S1, which perf1's reads by state fixed (Item 46). The nightly prune deleted run 1's scratch build
directory, which wasn't a worktree's, so scratch builds now stay inside a lane's own target.

**Known gaps.** Two flaky tests on the list (theseus-a2ec and 3dsz, P2). P3, post-v1: the rest of S4 (theseus-qlbq),
an installed binary checked against its commit (theseus-w441), the cockpit's uncommitted build (theseus-63gf),
duplicate versions in `deny.toml` (theseus-tcdl), a nightly reproducibility job (theseus-at7b), the TLS roots
(theseus-dn7p), a better allocator for musl (theseus-w6hg), and a musl build that something runs (theseus-3yu1). The
daily deny timer waits on the owner's decision; until then the deny script is run by hand at each install.

### Item 49. AWS's C1 = 14a: the bound account and its read tools (theseus-ppsd; row 29, stage D's first row; built in the `aws-c1` worktree beside C2, 2026-10-02 00:10 to 01:35, rebased onto C2 from 07:35, and onto 699927d with NODE schema 3 from 08:41; reviewed from 08:40; rebased at the join onto 34be7e2 as 79e1b49, with 32fd483 for the shape budget; joined 09:57 at 32fd483; installed 14:23 with 9f4035b)

**Why.** The AWS design's first slice on the spine (its §5, "C1 = step 14a. The bound account, and reads: the
tracer bullet"), on the client and the catalog the aws-client lane merged ahead of their reader (Item 18). Their
`reserved_for` markers are gone: they are read now.

**What landed** (§3.9; AWS).
- **The account**: `[aws.accounts.<id>]` names the key's two `[secrets]` entries (`credentials`; by default
  `aws_access_key_id` and `aws_secret_access_key`), a region, and the regions a call may name. The key resolves on
  the secrets board after serving, like every secret. The account is checked once, after serving, as the
  background startup phase `aws.check`: `sts:GetCallerIdentity` must name the table's account. Until it does, no
  call signs: a call waits for the check at most 30 s, then fails closed, saying why. A check that failed for a
  reason that may pass runs again at a call 30 s later; a key of another account, or one AWS refused, does not.
  Health has an `aws:` line per account.
- **Four tools**: `aws.call` (any read of any service, from the catalog compiled from the AWS CLI's models),
  `aws.describe` (the catalog, local, with no network), `aws.whoami` (STS, now), and `aws.s3.list` (the buckets
  with their regions, or the folders and objects under a prefix, capped, with a summary; a bucket in another
  allowed region is found and listed there).
- **Reads only, enforced before anything is sent**: `Client::check` runs in the plan, before the gate, with no
  network. A write, a run, a secret-bearing read (`GetSecretValue`, `GetParameter` with decryption), a region the
  account doesn't allow, and a bad input are invalid input, each naming the step that brings it, and nothing is
  sent.
- **The posture** (§3.9's step 4): `[policy.tools]`'s line for the tool, then `[policy.aws]`'s for the operation
  (`"s3:ListBuckets"`), the service (`"s3"`), and the class (`read`), then `enforcement`.
- **Visibility**: every request is attributed in its user agent (`exec/<execution>` and `call/<correlation id>`,
  which CloudTrail keeps). Each is an `aws.called` row with AWS's request id, never a credential or a result (a C2
  fact since run 2, `LedgerKind::AwsCalled`), and a span under its call's, in OpenTelemetry's AWS names. The CLI's
  `☁` line, the Discord tool line, and the Observatory's account line show it.
- **The store**: a tool call's gate record may hold `plan.class` and `plan.aws`, both optional, so NODE goes from
  schema 2 to 3, with a test that reads a schema-2 node. The first node a later build writes marks the store, and no
  earlier build opens it again: a rollback is a restore of a copy taken before the upgrade.

**How it is proven.**
- **Tests**: the template's account and `[policy.aws]` lines; the account table's ten ways to fail to load; nothing
  reaching AWS until a call; fifteen plans that must fail (writes, runs, secrets, regions, inputs), with nothing
  sent; a key of another account failing closed, and a call waiting 30 s for a key on tokio's paused clock, then
  failing closed; a whole core's AWS call as a row, a span, and a result; and a real daemon whose check begins only
  after the socket answers, with one request.
- **Three planted reverts**, each caught: fail closed off, reads only off, and the check moved before serving.
- **Live and read-only on the Home account**, in runs 1 and 2: a GLM turn made `aws.whoami`, `cloudformation
  DescribeStacks`, and `aws.s3.list`, and a bucket in another region was found through S3's redirect and listed
  there. CloudTrail's event history showed every call within four minutes, each with its row's request id and its
  execution and correlation ids. Nothing wrote to an AWS account, and no key was printed.
- **Gates**: 1,423 tests (run 1), 1,427 on C2 (run 2), 1,474 on 699927d (run 3). Run 2's gate found a race in the
  daemon's AWS start test (its poll could land between the account's binding and its phase's end): reproduced 8 in
  80 under load, fixed in the test, then 80 of 80.
- **The lifecycle bench**, alone: run 1's pair passed both builds at load 8 to 10. Run 3's missed p95s on a busy
  machine (IO pressure 8 to 19 %) with every p50 within its budget, and C1 adds only `[aws]`'s parsing to the start
  path. The review accepted it on the recipe's rule, and the join's gate benched it again.

**Divergences.** `credentials` names two `[secrets]` entries, as every other secret consumer does, not an `op://`
reference. The account's check is a startup phase and health, not a ledger row, like the GitHub token's.
`aws.describe` takes its posture from `[policy.tools]`, since it makes no AWS request. No metrics by service and
operation (the spans carry the facts), no idempotency key until there are writes, and no operating manual until 14b.
One fix outside the step: `[policy.tools]` lines for `task.create` and `wake.at` now load.

**At the join.** The rebase onto bench2's 34be7e2 met the shape budget: C1 took `config.rs` past 2,500 lines and the
CLI's `render.rs` past its ceiling. 32fd483 lists `config.rs` at 2,900, raises `render.rs`'s ceiling to 3,100 with
the splits filed (theseus-sk47, theseus-wdcw), and adds two `#[expect]` marks.

**Known gaps.** A `[policy.aws]` key that names no service or operation loads and governs nothing, so a typo loosens
a posture: harmless with reads only, a security problem with 14b's writes (theseus-snhr, P2, in 14b's brief). P3,
post-v1: metrics (theseus-ku5f), the manual (theseus-0e53), a gate check of the AWS layer's size (theseus-jsl5), the
key's age (theseus-nte0), and request spans for a call that runs after an approval (theseus-0zm4).

### Item 50. Row 51, 29b's wire-in: the index tender comes alive (theseus-u55z; stage F's first row, M6; built in the `m6-29b` worktree beside C2, 2026-10-02 from 00:10, rebased onto C2 from 07:35 (run 2), and onto 32fd483 from 09:58 (run 3); reviewed from 08:58; 3fe0d13, 6a21931, and 371725d; joined 10:40 at 371725d, with the turn bench's fix 3a4beff after it; installed 14:23 with 9f4035b)

**Why.** `theseus-index` (BM25, exact entities, and vectors over the WAL) and `theseus-follow` were on `main`, merged
ahead of their reader (Items 16, 20, and 24), and nothing in the daemon, the core, or the CLI ran them. Row 51 makes
the index tender the first tender that runs, so recall (30a) has something to ask.

**What landed** (§5.3, §6, §3.18).
- **The tender**: the socket daemon runs the `theseus-index` installed beside its own binary, and only that one, as
  `theseus-index serve --store <state>/store --index <state>/index --parent <its pid>`, at nice 10 and the idle I/O
  class, with an environment that holds no secret. A fresh tender starts 2 s after the socket answers; a tender a
  restart in place kept is taken over at once, so there is never a second. It exits with its daemon (`--parent`, a
  pidfd), so no crash leaves one holding the index's lock.
- **The supervisor** (`crates/theseus-core/src/tender.rs`): it restarts the tender at each exit, after 1 s, doubling
  to 60 s while it keeps failing, and 1 s again after a minute's run. It restarts a kept tender whose `[index]`
  settings changed, and sends it SIGTERM at a stop without waiting. A `Tender` child kind in the kernel's registry
  reaps it and reports its exits, and `relearn` knows it by its command line after an exec.
- **The record**: each start, take-over, settings change, start failure, and exit is a C2 fact (`fact/index.rs`,
  five facts under `LedgerKind::IndexTender`), written off the runtime's workers.
- **Health and the protocol**: health gains `index` (the tender's own status: its state, BM25-only or hybrid, nodes
  and chunks, its lag behind the WAL, the vectors, its memory; or `off`, `starting`, or `down` and why) and
  `children.tenders`. Health asks only a tender its supervisor runs, under 100 ms, else shows the last answer and
  its age. The protocol gains `index.status` and `index.query`, both reads, forwarded and bounded, and the CLI
  `theseus index status` and `theseus index search` (`--sources`), with goldens. The tender's wire shapes moved into
  `theseus-protocol`.
- **`[index]`**: `enabled`, `weights_dir`, `threads`, and `idle_unload_mins`, uncommented in the template. The install
  copies `theseus-index` beside `theseusd`, and `theseus-index` is a tool now, under the reader rule.

**How it is proven.**
- **With the real binaries**: health says `starting` after serving, and the tender's `started` row comes 2 s after
  `server.serving`; a SIGKILL brings the next 1 s later; a restart in place onto a changed note keeps the same
  tender, `adopted`; a daemon whose tender is SIGSTOPped still stops in well under a second. On tokio's paused clock:
  the start's wait, a take-over, a stop during the wait, the backoff's sequence and reset, and a failed start. Every
  behaviour has a planted revert that fails its test.
- **The lifecycle bench found a real cost.** Run 1 started the tender at serving, so every short-lived bench daemon's
  stop, kill, and swap met a tender just starting: the clean stop was about 40 ms slower at the median, and two p95
  budgets were missed in one round. Run 2's 2 s wait, and health asking only a tender that runs, took it off: the
  bench then saw one tender start against 51 daemon starts. At the join's prep, the bench alone matched `main`'s
  medians within 2.1 ms in every phase.
- **Live, twice, on a copy of the owner's store** with the real Nomic weights: it backfilled 94 nodes (145 chunks in
  164 ms in run 1), lag 0 B; `theseus index search` found a phrase from one of his sessions, first by BM25; a
  `kill -9` restarted the tender 1.002 and 1.003 s after the exit, from its cursor; and a stop ended both in about
  90 ms.
- **Gates**: 1,430 and 1,432 tests (runs 1 and 2), and gate 11 at the join's prep, 1,511 of 1,511.

**Divergences.** The tender is a binary of its own, not a role of the daemon's, so the daemon links neither tantivy
nor candle (about 4.4 MiB). The registry knows it by its command line after an exec, with no socket round trip. Its
exit 3 (the index held by another) is an exit like any other, on the same backoff. One connection per call, under a
deadline: recall brings the persistent one. `index.rebuild` through the core waits (theseus-y6za). The idle I/O class
is a no-op here: WSL2's disks use the `none` scheduler.

**At the join.** Run 3 met bench2's shape budget by splits alone, with no marks: the CLI's index lines moved to
`render/index.rs`, `TenderStatus` moved into the protocol's `index` module, five functions were split, and the
protocol's `lib.rs` is listed at 2,600 lines (its split, theseus-pf8a). `gate.sh`'s pre-bench build gained `-p
theseus-index`, so the bench runs the tender of the commit judged. After the join, gatelock's first join gate found
the tender's `started` row landing inside a measured plain turn (6 frames): the turn bench now waits for that row
before it times a turn (3a4beff).

**At the install** (14:23). The owner's daemon backfilled his store into `~/.theseus/index`, and health said the index
tender ran in the unit's cgroup, ready, 94 nodes, 0 B behind.

**Known gaps** (all P3, post-v1). `index rebuild` through the core (theseus-y6za); the tender as telemetry metrics
(theseus-gfi4); the default fusion burying exact matches while the vectors are still being made, for 30a
(theseus-pv7m); the Observatory showing the tender (theseus-43g1); whether the tender's commits slow a turn's syncs,
still to measure (theseus-dpqq); and the protocol's `lib.rs` split (theseus-pf8a).

### Item 51. The `gatelock` lane: the gate takes the shared lock only around its tests and benches (theseus-rx91; 2026-10-02 09:42 to 10:56; reviewed from 11:06; 78255f9 and 51a456f, on 371725d; joined 11:16 at 3a4beff, with the turn bench's fix)

**Why.** On the morning of 2026-10-02, six lanes' gates and the chain's join gates queued up to 25 minutes behind
one another: every gate held the shared gate lock (`~/.cache/theseus-gate.lock`) through its `fmt`, clippy, and
builds, a cold target's included, though only the tests and the benches need a quiet machine. hardening2's join
alone lost about 25 minutes to the queue (Item 44).

**What landed** (§9; `scripts/AGENTS.md`).
- **Two modes**, `THESEUS_GATE_LOCK=outer|inner`. Outer is the default and unchanged: the old phases in the old order,
  under a lock the caller holds. The chain's join gate stays outer, under theseus-quiet.sh.
- **Inner**: `fmt`, the shape check, clippy, the bench build, and the test build run without the lock. The gate then
  runs itself under `flock -o` for the reader rule, the suite, the protocol types, and the benches, and lets go for
  `deny` and the web builds. One table covers both halves, with a `lock wait` row, and the history records it.
- **The log** names the lock's holder and the gates queued ahead, in order, from `/proc/locks`, since a process
  waiting in `flock` has the file open too. It says when the lock was taken and when released, and after how long.
- **A deadlock guard**: an inner gate under a lock its caller already holds (an outer `flock`, or theseus-quiet.sh's
  fd 9) exits 2 at once, so wrapping it can't make it wait for itself. A NOTE when the locked part compiled
  anything, and a signal fix: a gate ended by a signal now prints its table (bash's EXIT trap had read `$?` as 0).
- At the rebase onto 371725d, the bench's build moved into one function both modes call, so the next binary the
  benches start is a one-line change.

**How it is proven.**
- **Inner mode with the lock held elsewhere**: the compiles finished 19 s in, and the gate waited only for the
  locked part. After the rebase, 192 s of compile and lint ran unlocked, and the lock was held for 130 s; the old
  gate would have held it for all of them.
- **Outer mode** kept the history's phase order. Planted clippy and test failures exited non-zero with the table, and
  the lock was free after each.
- **Thirteen harness cases** on a fake cargo, in seconds: a free lock, a held one, a queue, a holder on fd 9, outer
  mode, failures inside and outside the lock, the deadlock guard (exit 2 in under a second), a daemon that outlives
  the suite and holds no lock, an unwritable lock, a bad mode, a compile under the lock, and a TERM while waiting.
- **No function of the gate holds the lock's fd**, measured: while the locked part ran, only its own `flock` had the
  file open, and after the gate, nothing. A negative control, a lock taken the forbidden way, showed the scan would
  see a leak.
- Whole gates in inner mode on the real lock, before and after the rebase, both green.

**Divergences.** None from the plan's shape. In inner mode the benches run the test build's binaries, since cargo
relinks on every call, so the `-p` bench build is redundant there (theseus-7ykr). That build is the one the suite
tested, with an install's features.

**The join** (11:16). The first join gate failed in the turn bench: the index tender's `started` row (Item 50) landed
inside a measured plain turn, which then counted 6 frames. The turn bench now waits for that row after serving
(3a4beff), and the next gate passed. Lanes switched to inner mode only after rebasing onto this `main`: a worktree
with an older `gate.sh` ignores the variable and, without its `flock`, would run with no lock at all, so the lane
rules check for it first.

**Known gaps** (all P3, post-v1). theseus-quiet.sh pauses only compilers, while a lane's `fmt`, `deny`, and web
builds now run beside the chain's bench (theseus-fsvv); the redundant bench build (theseus-7ykr); and no priority
for the chain's gate in the lock's queue (theseus-rwhf). _(Since Item 75 the inner mode is every gate's only mode, and theseus-quiet.sh is retired.)_

### Item 52. S2: one writer thread owns the WAL, and the commit path's discipline (theseus-vni9, theseus-avvb, theseus-xprd; Review 2's S2, R7, and R8; spine; 2026-10-02 08:35 to 10:26, and the join's run 2 from 10:41, cut by the usage limit at 11:32; reviewed from 10:29; rebased onto 3a4beff as b9ce20a, 0c57746, 4d07398, 87ea9c7, and 85e5294; joined 11:49 at 85e5294; installed 14:23 with 9f4035b)

**Why.** Review 2's S2, the last of its spine items: every request handler waited on the disk on a runtime worker,
so a burst of writes starved every other task (a health request waited 17 s in perf1's burst, Item 46). Each appender
ran its own fdatasync, or joined a condvar's group sync, from its own thread. R7: nothing stopped a lock guard from
being held across an `.await`. R8: `mark()` didn't take the append lock, and P5b's version rule was held only by
review.

**What landed** (§6, §9, P5b).
- **One writer** (Part 1, b9ce20a): the store owns a thread, `store-writer`. An append hands it the frame and waits;
  the writer writes every frame queued back to back, runs one fdatasync for the batch, indexes the batch in one redb
  transaction, and then answers each caller. So K1's lock still spans a transition's read to its frame indexed, held
  and released on one thread, and the kernel's transitions stay synchronous.
- **The wait holds no worker**: it is a declared blocking section in the store's append, `theseus_store::blocking`
  (`block_in_place`, which hands the worker's run queue to another thread first), and so are the waits for K1's and
  the session's locks. The WAL's append is now a write and a sync, and a write cut short is cut back off.
- **The checkpoint** (Part 2, 0c57746): the periodic checkpoint runs on the writer, after it has answered the batch
  that crossed 1,000 records, so no append's own call pays it.
- **R7**: `ExecLock`, `SessionLock`, and `SessionHold` are `!Send`, each with a compile-time assertion beside it and
  a `compile_fail` doctest. **R8**: `mark()` takes the append lock, and theseus-core's `tests_schemas` records each
  record kind's shape under its schema number, so a changed shape fails until its number moves.
- **A segment is as durable as its frames** (theseus-xprd, 87ea9c7): a new segment's name, and a new log's
  directory, are synced before any frame in it is reported durable. Before, a power loss right after a roll could
  lose the new segment and every acknowledged frame in it. That cost a directory sync, 6.6 ms p50, once every
  64 MiB of WAL; a start of an existing store pays nothing.

**How it is proven.**
- **Revert proofs** for all six fixes: group commit (13 syncs for 12 frames), the worker-free wait, `mark()`'s lock,
  the checkpoint on the append (1.5 s for the 1,000th append), each guard made `Send`, and the directory sync. The
  checkpoint's test holds the checkpoint on a latch, so no timing decides it (85e5294).
- **The crash test** with torn tails, C6's 40 × 3 and a four-writer variant: zero committed records lost, on both
  parts. **kernel-sim**, 400 seeds at `--p-race 0.5` on each part: every invariant held.
- **At 32 concurrent turns** (the rig): wall time 30 s became 4.5 s; frames per fdatasync 2 became 16 to 17; a
  probe's worst wait for a worker 1.9 s became 3.5 ms; commit latency p50 27 ms and p99 59 ms. §9's commit row is a
  number now.
- **The index tender's follower** reads the page cache and never sees a sync: a new test runs it beside the batching
  writer, with rolls inside its batches, and it reads every record once, in order.
- **Live**, on a copy of the owner's store: `kill -9` during four GLM turns. Every frame acknowledged before the kill
  was there after it, byte for byte, and all four executions were requeued and continued, their cut-off calls
  settled unknown at their deadline.
- **The lifecycle bench**: run 2 benched both builds in one hold of the lock, in palindrome order, four rounds:
  medians level in every phase (cold start p50 23.6 ms before, 24.4 after), and the start's one frame, now through
  the writer, costs about 0.2 ms. A slow mode S2's cold starts showed only in the hold's second and third places;
  Tabitha/Claude's reverse-order run (11:42 to 11:44) swapped the places, and it followed the place, not the build.
- **Gates**: 1,448 and 1,449 tests (run 1), 1,518 and 1,519 (run 2). **The join's gate**, 11:44:47 to 11:48:19:
  1,519 of 1,519, lifecycle OK in 10.3 s (cold p50 23.5 ms), a plain turn's frames 5 of 5.

**Divergences.** The await sits in the store's append, not at the call sites: an `async` transition would hold K1's
thread-keyed lock across an await, which R7 now forbids, and an `.await` at every call site was a hundred edits in
files three lanes were editing. The checkpoint's tender is the writer itself: wherever it runs it holds the append
lock and redb's one write transaction, so queued appends wait either way, and on the writer it needs no thread of its
own and covers every store, the sim's included. Under a burst each blocked append holds a thread, so the blocking
pool can grow toward tokio's 512, bounded in practice by admission (8 turns by default): a measurement for v1.1.

**The join.** Run 2 (from 10:41) rebased onto 371725d: `tests_schemas` refused the tree until it recorded `node @ 3`
(C1's two fields, Item 49), the store's `store.rs` moved its tests to `store/tests.rs` for the shape budget, and
xprd was fixed. The usage limit cut it at 11:32, after its third gate and second bench. Tabitha/Claude ran the
reverse-order bench, rebased it onto 3a4beff (clean), and joined it.

**Known gaps.** P3, post-v1: a failed fdatasync's frames come back at the next open (fsyncgate; theseus-ljgm); an open
that replayed ends with a checkpoint on the start path, about 14 ms (theseus-4ra3); META has no recorded shape
(theseus-ybnx); the periodic checkpoint still stalls the appends queued behind it (theseus-zsu9); a killed process's
in-flight provider calls wait out their 600 s deadline (theseus-m9iy); and ~~a brand-new store's own directory name is
never synced (theseus-gf00)~~ (built in Item 67).

### Item 53. The `robust2` lane: one exit for a turn, a corrupt record that degrades only what reads it, and a crash file (theseus-xonq: Review 2's R1, R4, R5, and consideration 1; theseus-15g; theseus-3ebd; 2026-10-02 07:42 to 10:49; reviewed from 11:02; rebased onto S2 by Tabitha/Claude, with the repair flag's join patch and a shape fix; 364e8d9, 17e206b, 006a956, 0283a02, ad3590a, and 520b1bc; joined 11:53 at 520b1bc; installed 14:23 with 9f4035b)

**Why.** Review 2 found that a turn faulting after a paid loop skipped its books (R1); that one corrupt record failed
every list read, so the driver silently ran no continuation and no wake for any session, on every tick (R4, with
theseus-15g's repair path); and that Discord's splitter needed a progress guarantee (R5). The owner, 2026-10-01 at 19:54:
keep `panic = "abort"` with a crash file (consideration 1), and option (a) for an execution the old unit budget ended
(theseus-3ebd).

**What landed** (§3.3, §3.13, §3.22, §6).
- **A turn has one exit** (R1, 364e8d9): after a turn begins, every way it ends closes its books, a fault as a
  failure does (the session's numbers, `turn.failed`, the error, telemetry; class `internal`). A charged call whose
  answer's frame fails settles alone, so the budget holds nothing for a call that has ended.
- **Reopened under dollars** (3ebd, 17e206b): startup's step 2 reopens an execution the unit budget ended that is
  under its dollar limit, whether this start or an earlier build migrated it, as `budget.reopened`. It reads them by
  their state term (perf1's projection, Item 46).
- **R5** (006a956) was already fixed by theseus-s68 (Item 13); the lane adds the brief's adversarial cases as a
  property test, proven against both reverts.
- **A corrupt record degrades what reads it** (R4 and 15g, 0283a02): a list read (`read_many`, `scan`, and what
  calls them) skips a record whose read is refused, logs it once, and counts it; a read of that record alone is
  still refused. The driver logs a tick that can't list its executions, once per failure. Health, `theseus health`,
  and the Observatory show the count with its repair. **The repair** (ad3590a), `theseusd restore --repair --from
  <copy>`, takes each frame that doesn't check whole from the same offset of a copy, keeps every other byte, opens
  the result to check it, and keeps the repaired store aside: nothing written after the backup is lost.
- **A crash file** (consideration 1, 0283a02): a panic hook writes `crash-<mode>.json` beside the store (0600: the
  thread, the location, the message), then the abort follows. The next start moves it into `crashes/`, logs it, and
  writes a `server.crashed` fact without the message; health and the Observatory show the newest crash.

**How it is proven.**
- **Twelve planted reverts**, each failing its test: among them R1's three, list reads that fail whole again, a
  driver that never logs, a repair that doesn't copy the frame, a hook that writes nothing, a start that doesn't
  take the file, and R5's two. R4's were proven again after the rebase.
- **A skipped record never loosens a check** (the review): the outside-text hold and an execution's budget are read
  by key, which is still refused, and the kernel's counts come from the index's terms. Each list a skip feeds
  degrades to less shown or done, never to more allowed.
- **Live**: a scratch daemon on a copy of the owner's store reopened his DM session's execution (`budget_exhausted` to
  `waiting`, $0.42 of $100), again on the rebased build. A debug build with a planted panic aborted with a 0600
  crash file, and the next start reported it in health and one row.
- **Gates**: the last on the rebased branch at 10:43, 1,504 tests; the lifecycle bench alone at 10:48: LIFECYCLE OK
  (cold start p95 24.9 ms of 50, a clean stop 33.3 of 100, a kill and restart 39.7 of 150, a swap 53.7 of 200).
  **The join's gate** at 11:53:03: 1,534 of 1,534, lifecycle OK in 9.8 s, frames 5.

**Divergences.** 3ebd reopens at every start, not only at the migration, since the owner's store was migrated by an
earlier build. R4's skip covers `scan` too. 15g's repair is frame by frame from a copy, which loses nothing written
after the copy; `restore --from` already replaced the whole store. R7 and R8 moved to S2 (Item 52). The planted panic
marks the process non-dumpable, so its test leaves no core on C:.

**The join.** The run ended at 10:59 on the gateway's 8 MB output limit, after its report; nothing was lost. Tabitha/Claude
rebased it onto S2 at the join (keep-both hunks in the store's `store.rs`, its read path into S2's `Inner`, its test
into `store/tests.rs`, the TypeScript regenerated), applied the repair flag's patch, a dispatch match kept out of the
branch (ad3590a), and moved the CLI's store lines into `render/store.rs` for the shape budget (520b1bc).

**Known gaps.** `Wal::open` cuts a bad frame in the last segment as a torn tail, with every good frame after it, and
reports it only as `truncated_bytes`; the repair refuses rather than cut (theseus-gt12, P2). The output golden's mask
keeps a duration's unit, so it fails under load (theseus-6a7o, P2; on the flaky list since Item 55). P3, post-v1:
the splitter's property tests abandon a looping thread at their timeout (theseus-dsmp); an execution at or over its
dollar limit has no way to reopen (theseus-x3m9); a crash loop under the user unit restarts every 5 s forever
(theseus-0v8s); and an execution record that doesn't decode still fails every list of executions (theseus-q6nt).

### Item 54. The `robust3` lane: eleven correctness gaps on v1's path (theseus-7f7k: theseus-tq04, 0o8, ni5, 6g6, 4lx, 5wgd, ht82, p7q, k52m, nu3z, and yey; 2026-10-02 08:35 to 10:25; reviewed from 11:07; rebased onto robust2 by Tabitha/Claude (99d7d97), with a shape fix after the first join gate; e85efb0, 99d7d97, and 22b0877; joined 12:03 at 22b0877; installed 14:23 with 9f4035b)

**Why.** Eleven gaps filed against the core by earlier steps and reviews, each on v1's path: a call a cancel left
without a result, a reset that seemed to free money it couldn't, a running job that could fill the disk, a float that
didn't read back as written, and the rest below.

**What landed** (§3.13, §3.15, §3.16, §6, DD8).
- **tq04**: `session.watch` seeds the push itself, as `executions.watch` does, lazily, after serving.
- **0o8**: a cancel answers every call it ends, once: one sweep under the execution's lock, run when no turn holds
  it. A call never sent reads as not run, a stopped job keeps its output so far, a call that may have run is
  `unknown` (Item 18's rule), and a call that finishes after the sweep keeps its real result in the transcript.
- **ni5**: a call planned before a restart and never asked is declined and answered "not run" in the continuation,
  not parked on a question no card was posted for.
- **6g6**: a reset leaves held what it can't free, money reserved for calls in flight and money held unknown. A
  question says when a reset can't make the call fit, with the held amount and the remedies; an approved reset
  tries the call once, then fails (`over_limit`) instead of asking again.
- **4lx**: a wake's or report's retry keeps its line and posts where the take kept the target, a META record
  (`wake.target.<session>`) written in the taking frame; no record layout changes.
- **5wgd**: a lingering wrapper's output is never removed under it (`Spool::wrapper_lives`).
- **ht82**: below `[server] disk_floor_mb` no job starts, and every running job is stopped, with the numbers in its
  reason and a `job.stopped_below_floor` row, and its conversation woken to read it.
- **p7q**: `theseusd --stdio` stops cleanly on SIGINT and SIGTERM, as the socket daemon does.
- **k52m**: serde_json's `float_roundtrip`, one line in the workspace's `Cargo.toml`, so a stored cost reads back as
  the value it was written from, about 2.3 ns more a float.
- **nu3z**: `theseus watch --interactive` continues a session on its last turn's profile.
- **yey** (99d7d97): `/stop` drops the model's stream. The turn writes no answer and plans no call, and the cut call
  settles as failed at an estimate, never above its reservation and never held unknown (the input reserved, plus
  three characters of streamed text to an output token, thinking included), with a `provider.cut` row.

**How it is proven.**
- **Thirteen planted reverts** in five batches, each failing its item's test (the lane's `proofs.md`).
- **Live**, on a scratch daemon over a copy of the owner's store, with Discord and the web UI off and GLM turns only: a
  watch seeded the push and received `execution.changed`; a line typed in `watch --interactive` ran on the session's
  `glm-5.3-flash`, not the daemon's live Sonnet profile; a cancelled execution with every call answered; a stdio
  daemon's clean stop on each signal and its clean reopen; a cut stream booked. $0.0026 in all.
- **Gates**: 1,459 and 1,461 tests at the lane's two commits. **The join's gate** at 12:03:17: 1,552 of 1,552,
  lifecycle OK in 10.2 s, frames 5.

**Divergences.** ht82 changes §6: the floor used to leave room for what already runs to finish, which held only for
captured output, not for files a job writes itself, and at L0 nothing names the writer. The review called it blunt
but the lesser harm (a full disk paused the whole machine on 2026-10-01), told the owner at 10:42, and asked whether he'd
rather it only refused new jobs; until he says otherwise it stays, and M4's per-job quota makes it precise
(theseus-dszu). yey books an estimate now, where the brief left booking to a follow-up: no cost under-books, and
holding unknown would pile up a reservation per stop that a reset can't free. nu3z took neither of the issue's two
options: the watch follows the session's last profile, and the daemon's default for `ask -s` is the owner's call. ni5
decides in the continuation, not in a startup pass. 4lx keeps the target in META, not in the node. k52m turned the
feature on rather than changing a node's layout.

**The join.** Tabitha/Claude rebased it onto robust2 (Item 53), keeping both sides in `toolrun.rs` (main's AWS rows, then
0o8's accepted completion) and in the stdio block, and regenerated the output golden, keeping only the intended
lines. The first join gate failed the shape budget at 11:53:30: `tests_m3.rs` reached 8,266 lines against 8,000, and
the kernel's `kernel.rs` 3,052 against 3,010. 22b0877 moved the answer tests into `tests_m3/every_call_answered.rs`
and `stop_call` into `stops.rs`, raised `kernel.rs`'s ceiling to 3,030 with its reason, and split yey's test for
clippy's 100 lines. k52m's feature rebuilds every crate that uses serde_json, once per target.

**Known gaps** (all P3, post-v1). A daemon that dies between a cancel and its sweep leaves the calls unanswered
(theseus-fvxh); a cut stream's exact usage, from the provider's own events (theseus-k48c); a per-job disk quota, so the
floor stops only the writer (theseus-dszu); and no operator verb settles or forgives a call held unknown, a decision
for the owner (theseus-cv3v).

### Item 55. The `security3` lane: the security gaps left on v1's path (theseus-fa4m: theseus-txvt, ur1t, ewi, 830, 8d1b, hmwv, c3e, and sqpx; 2026-10-02 08:25 to 10:46; reviewed from 11:10; rebased onto robust3 at the join; 3c040d1, 35bd10d, 52607c2, 7d4cd98, 711744a, a8df541, 2b08632, edecc20, c10b92d, 1c0ddd4, e93287e, and the join's 5892683; joined 12:14 at 5892683; installed 14:23 with 9f4035b)

**Why.** Eight gaps on v1's path that hardening2 (Item 44) and other steps left open: grants that still reached
launchers and a granted git's hooks, writes outside the roots that `proc.run` made at its posture while `fs.write`
asked, a card that said it expired while its request never did, a public template and default that named the
operator's own vault, and three smaller holes around a stop and the operator's notices.

**What landed** (§3.1, §3.9, §3.14, §3.15, §3.19).
- **No grant to a launcher** (txvt, 7d4cd98): a grant to a shell, an interpreter, a wrapper that runs the command it
  is given, or a runner of a project's scripts fails to load, naming the rule, and a granted name that resolves to one
  gets nothing at the call. cargo and npm get their own rules: a grant only for registry commands that build and run
  nothing else (cargo's `login`, `logout`, `owner`, `search`, `yank`, and `publish --no-verify`; npm's commands that
  run no package's scripts).
- **A granted git runs no hooks** (ur1t, 7d4cd98): a git or gh given a secret runs with `core.hooksPath=/dev/null` and
  an empty `core.fsmonitor` pinned at the command-line scope, so neither a cloned project's hooks nor its fsmonitor
  sees the variable. What still reaches it at L0 is listed for M4 (theseus-ngz5).
- **A write outside the roots takes its tool's posture** (ewi, 52607c2), whichever tool makes it: the owner's rule of
  2026-09-30 at 23:45, not the brief's suggestion. A read or a working directory outside the roots, the approve list,
  and the floor still ask. The Discord proof's write now waits on the approve list, so it keeps its twelve steps
  (1c0ddd4).
- **A question expires** (830, a8df541 and edecc20) at the time its card gives, within a driver tick: declined by
  `expiry` in one frame with an `action.expired` row, the card settles "Expired", and the model reads that nobody
  answered. A budget question waits until it is answered.
- **No one's vault in the public text** (8d1b, 2b08632 and e93287e): the default config is the local file
  `~/.theseus/theseus.toml`, the template's references are placeholders, and the operator's Discord, guild, and bot ids
  in tests are invented ones.
- **hmwv** (711744a): a `/stop` also stops an input sent before it whose turn wasn't yet admitted. **c3e** (35bd10d):
  the operator's notices fall back only to a place this daemon's bindings name, and are otherwise refused with the
  reason. **sqpx** (3c040d1): a clean stop writes the web refusals still held in their minute.

**How it is proven.**
- **Planted reverts**: with each fix off, all 14 of the lane's new tests failed as their bugs did, on the first tree
  and again on the rebased one, and passed with the fixes back.
- **Live**, on a scratch daemon over a copy of the owner's store, with Discord off, a fake `op`, a fake Messages API, and a
  stand-in `cargo` that says only whether the variable reached it: a grant to bash refused at load; `cargo build`
  without the variable and `cargo publish` with it; a granted git's fetch with a planted hook that never ran;
  `fs.write` outside the roots at `notify`; a question on a 3-second TTL expired and its call didn't run; held web
  refusals written at the stop; the repository's example config checked, all eight references placeholders. Again on
  the final rebased build: every step the same.
- **The lifecycle bench**, an A/B against its base under the lock: every phase passes for both, with equal p50s.
- **Gates**: 1,443, then 1,487 tests on the rebase onto 34be7e2. **The join's gate**, rerun 12:11:00 to 12:13:30:
  1,568 of 1,568, lifecycle OK in 9.8 s, frames 5.

**Divergences.** ewi went the owner's way, not the brief's (which suggested classing such a `proc.run` as `fs.write`). 830
made the card true rather than changing it. ur1t pins hooks off for every brokered git, the operator's own hooks
included, which hardening2 had declined; it pins an empty `core.fsmonitor`, not `false`, since before git 2.36 the
value is a program's path. 8d1b left the exam's item name, whose rename would move the exam's pinned BM25
reproduction (theseus-e663). Two semantic conflicts with `main` were found and fixed in the lane: perf1's
runnable-only read (Item 46) would have silenced 830's expiry, and the Discord proof (Item 47) waited on a write that
ewi makes run.

**The join.** Rebased onto robust3: both lanes added a `TurnRunner::stops` of different types, so security3's became
`latest_stops`, keeping both behaviours, and `toolrun.rs` kept C1's `aws` beside `question_due`. The first join gate
(12:06 to 12:08) failed only the output golden: the parent's report turn compiles 17,220 bytes in a full suite and
17,219 alone, and the rewrite had run alone (theseus-6a7o). The golden keeps the full suite's numbers, and its test
went on the flaky list under 6a7o (5892683).

**Before the owner's daemon ran it.** A bare `theseusd` no longer defaults to his vault note, so his daemon needed
`THESEUS_CONFIG` or a unit naming `--config` (Item 56 gave it the unit); his cards now expire after
`confirm_ttl_secs`, 900 s in his note. Both were told him at 10:48.

**Known gaps** (all P3, post-v1). What a granted program runs on its own account still gets the variable at L0: a
credential helper, `core.sshCommand`, ssh's and gh's config, cargo's credential provider, and `/proc/<pid>/environ`;
L1 closes them (theseus-ngz5). `fs.read` outside the roots asks while `proc.run cat` of the same path doesn't, a
decision for the owner (theseus-2tgm). The exam's item name and guard, and the index's prefixes as config (theseus-e663).
The history's private names are theseus-s2o7's.

### Item 56. The `userunit` lane: the daemon as a systemd user service (theseus-w1nf; 2026-10-02 10:02 to 11:11, and its follow-up, run 2, 11:15 to 11:30, whose announcement the 11:31 usage limit cut; reviewed from 11:14, and run 2 at 11:47; 827fd3f, cbe8093, 3ba8f3b, and 517c900; joined 12:16 at 517c900; installed 14:23 with 9f4035b, when the operator's daemon became the unit)

**Why.** The owner, 2026-10-02 at 09:39, answering M4's open questions: his daemon as a systemd user service, with a
script and a how-to. M4's job limits and confirmed cancels need a delegated cgroup (`Delegate=yes`), a crash should
be restarted (Item 53's crash file), and the journal should hold the log, with no terminal kept open. The installer's
`--user` unit (Item 18) existed, with two wrinkles: `--op-token-file` worked only before `install`, and the plan
didn't check the token file at all.

**What landed** (§3.22; `docs/user-service.md`).
- **The two fixes** (827fd3f): `--op-token-file` is a global flag, so it works in either order; a guard test holds
  that a job command's own `--op-token-file` stays its argument. The `--user` plan checks the token file by `stat`
  alone (a regular file, the operator's, mode 0600 or stricter, not empty), never opens it, and `--apply` refuses
  until it is right. Its hint is the exact command to rerun, the operator's own flags quoted.
- **`scripts/user-service.sh`** (cbe8093): `check` (read-only: the platform, the user manager, linger, WSL's
  `systemd=true`, cgroup v2 with `memory` and `pids` delegated, the binaries, the token file, the config the unit
  would get, the unit, and who answers on the socket); `install` (check, plan, y/N, apply, `daemon-reload`, linger, a
  clean stop of a hand-started daemon, `enable --now`, wait, health, and a cheat sheet); `status` (it says when the
  binary was replaced under the running daemon), `logs`, `restart`, `stop`, `start`, `uninstall`, `--dry-run`, and
  `--yes`. No terminal and no `--yes` means every answer is no. It never opens the token file, finds the daemon by its
  socket, never by a process pattern, and bounds every wait.
- **`docs/user-service.md`**, linked from `docs/README.md`: why, step 0 (name your config), the token file without the
  token on a command line, the one command and its steps, daily use, the unit line by line, undoing it, and the WSL
  notes.
- **The follow-up** (run 2, 3ba8f3b and 517c900): after security3's default became a local file (Item 55), an
  install from a shell without `THESEUS_CONFIG` would have written a unit naming a file that isn't there, after
  stopping the hand-started daemon. `check` now reads the plan's own `config:` line, fails on a file that isn't
  readable with both fixes, and `install` stops before its first question after such a failure.

**How it is proven.**
- **Planted reverts**: 13 of the Rust changes, 19 of the script, and 11 of the follow-up, each caught. 20 stand-in tests
  run the real script and the real `theseusd install --user` in a scratch `HOME` against stand-in `systemctl`,
  `loginctl`, `journalctl`, `theseus`, and `op`, in the gate; a sentinel in the token file shows no output carries it.
- **A transient unit**, `theseus-userunit-test`, with the installer's settings: the cgroup delegated (`memory pids`); a
  SIGKILL restarted by systemd; a copy-then-rename leaving the running image `(deleted)` until a restart; a second
  daemon on one store refused; and `theseus shutdown`, a clean exit, staying stopped.
- **Found by running it**: the first real `check` misread the user manager's controllers (a glob that couldn't match
  two names sharing one space). Fixed, and a test fails if it comes back.
- Six gates, the last 149 s. **The join's gate** at 12:16:01: 1,598 of 1,598, lifecycle OK in 9.5 s, frames 5. The owner's
  daemon and unit were never touched by the lane.

**Divergences.** `uninstall` stops the service before it removes the unit, and touches only a unit `theseusd` wrote.
`--yes` is the lane's own. `check` doesn't run `theseusd check`, which reads the vault: the vault's wiring shows at the
end of `install`, in health. The script puts the token flag before the subcommand, so it worked with the installed
build of the morning, which predates the fix.

**The install** (2026-10-02 at 14:23, with 9f4035b; the owner, 13:03 to 13:29: "go to build", and "I'd like you to be in
charge of managing our runtime"). His daemon had been down since 2026-09-29 at 18:50, the store's last write. Before
the first start, his store, a copy of his config, and his bindings were backed up. `scripts/user-service.sh install
--yes` wrote `theseusd.service` (`Delegate=yes`, `KillSignal=SIGINT`, `KillMode=process`, `Restart=on-failure` after
5 s), enabled, with linger on. Health at 14:22: serving at 22.6 ms, secrets ready, Discord ready, approval open to
his trusted user, the index tender running in the unit's cgroup and ready. An upgrade is now a build, a
copy-then-rename, and `scripts/user-service.sh restart`.

**Known gaps** (all P3, post-v1). `install --user --apply` with no token file named still writes a unit that can't
start (theseus-4xyj); the service's start with the distro after a WSL restart is untried (theseus-gfj5); `check` in a
shell without `THESEUS_CONFIG` judges what an install from that shell would write, not what the installed unit names
(theseus-a7gx); a crash loop restarts every 5 s forever (theseus-0v8s, Item 53). The unit's `KillMode=process` met
L1's job limits at 17b's join (Item 58).

### Item 57. `theseusd example-config` with an operator's private overlay (theseus-dxgb; built on `main` by Tabitha/Claude, with no lane, 2026-10-02 12:34 to 13:04; 93fbb6d and 9f4035b; installed 14:23 at 9f4035b)

**Why.** The owner, 2026-10-02 at 12:34: "Please reset the config from example so i can once again wholesale from
template." His config note was the 07:47 build's template, word for word. Since security3's 8d1b (Item 55) the public
template carries placeholders where a deployment's own values go (its vault's references, its people's ids), so a
wholesale copy of the next build's template would have broken his daemon. Reverting 8d1b would put his values back in
a public repository, so the template stayed generic, and the overlay was built instead. Then, at 12:36: "I'm just
asking for example-config command to output the fixed up config for 1password."

**What landed** (§3.19).
- **The overlay** (93fbb6d): `theseusd example-config --overlay FILE` prints the template with a private overlay's
  values in place. The overlay is a TOML document of only what the public template can't carry. Every line of the
  template is kept but those the overlay sets: a live line for a key gets the overlay's value, its trailing comment
  kept at its column; else the first commented line for the key is switched on, with its table's header when that is
  commented too; else the key goes after its table's last key, and a table the template lacks goes at the end. The
  result is parsed, checked to hold every overlay value where it was set, and validated as a config before it is
  printed, so a misspelt key fails with its name and prints nothing. The render is `theseus_core::config_overlay`.
- **With no flag** (9f4035b): when `~/.config/theseus/template-overlay.toml` exists, plain `theseusd example-config`
  prints the template with it in place, the deployment whole and ready for the vault's note, and its first line names
  the overlay. `--overlay FILE` names another, and `--plain` prints the template alone, byte for byte. Without the
  file it prints the template alone, as on any other machine.
- **Nothing that builds on the template reads an operator's overlay**: theseusd's test helper `safe_note`, the
  closed-pipe test, theseus-exam's daemon reads, and the Discord proof all ask for `--plain`, and theseusd's
  `AGENTS.md` says so for what comes next.

**How it is proven.** Eight tests of the render: an empty overlay changes nothing; an operator's overlay on the real
template changes exactly its 11 lines and loads; comments keep their column; a missing key or table lands where it
belongs; a live line wins over a commented one; prose that looks like a header or a key is left alone; a misspelt key,
an array of tables, and bad TOML each fail by name. Through the binary: `~` in the path, nothing printed on a failure,
the default path, `--plain`, the two flags' conflict, and the first line. Each commit's gate on `main` (12:56:45 and
13:03:55): 1,607 of 1,607, lifecycle OK, frames 5. **Against his note**: the owner's overlay holds his eight `[secrets]`
references and his `[approval]` section; its render's references equal his note's, and against his note only
`[approval]`, `[index]`, and `[policy.aws]` are new, with no value changed. It was checked again with the debug build
under his home directory. He pasted the render into his note himself at 13:02, ahead of the install.

**Divergences.** The first form needed `--overlay`; the owner asked for the plain command, so the second commit made the
overlay's default path the default.

**At the install** (14:23). `theseusd check` on his note resolved all 8 secrets, and the unit started on it (Item 56).
The workflow from here: change the overlay, run `theseusd example-config`, and paste its output into the note.

### Item 58. L1 for `proc.run` (theseus-7ve.1; M4 row 17, step 17b, stage C's first row; spine; 2026-10-02 11:42 to 12:59, run 2 (run 1, 11:25 to 11:31, read and warmed the target, then met the usage limit); reviewed 15:30 to 15:34, with a second read 15:50 to 16:00; rebased onto 9f4035b as c1ac351 and d052ebd, with two join commits, b503b2c and 4fc7ddc; joined 16:34 at 4fc7ddc; installed 16:38 at 4fc7ddc)

**Why.** M4's first spine step. The sandbox crate (17a, merged ahead of its reader in Item 16) ran nothing yet, and
`proc.run` ran every job at L0, as the operator. The owner answered M4's open questions on 2026-10-02 at 09:39: "notify
only is the global default here" for an L1 job; only the jobs the model asks for go to L1, with no `l1_argv` list by
default; `[sandbox]` gets templated defaults; and an L1 job with no egress and no secret will be exempt from the
outside-text hold (at 20a; 20a was dropped, Item 74, and the exemption is not built: theseus-oaf9).

**What landed** (§7, as amended; §3.9).
- **The class goes toward L1 alone** (`Sandbox::l1_for`, at plan time): `[sandbox] default = "l1"`, then
  `l1_argv`, then the model's `sandbox: true`. `sandbox: false` undoes neither, so a call can talk its way into L1,
  never out of it. `l1_argv` is empty and `default` is `l0` unless the operator says otherwise.
- **L1 at notify**: an L1 call's decision is built whole, `notify`, in place of the L0 order (the floor, the approve
  lists, the allow list, the tool's posture and its tightening, the broker's grant), and the outside-text gate still
  runs after it. The premise that L1 can reach nothing is made true by construction: the job's view hides the floor,
  the approve list's paths, and the daemon's socket, whatever binds them (`Spec::hidden`). No secret reaches an L1 job
  until 18d; what L0 would grant is named as withheld.
- **The class is bound** in the gate record and in the proposal a confirm binds, so an approved L1 call runs in L1,
  and an L0 call's digest is unchanged. **No fallback**: a job that can't start in L1 fails with its stage and
  error, and never runs at L0.
- **The limits and the cgroup** (§7): `pids` 512, `memory_mb` 2048 with a delegated cgroup, `scratch_mb` 1024, and
  `output_mb` 64; writes go to scratch and are discarded. The cgroup is judged by systemd's own answer, never a config
  key, and each L1 job gets its own. A probe 3 s after serving records `sandbox.probe`, and health's `sandbox` line
  says whether L1 works. Every surface marks an L1 call `🛡️ L1`, and its result's head says where it ran.

**How it is proven.**
- **13 new tests and the template test's new asserts**, each against a planted revert, in three batches: among them L1
  at notify where L0 waits, routes into L1 that `sandbox: false` can't undo, an approved L1 call that runs in L1 and
  never as L0, the contract in a real L1 job, no secret, and no fallback (batch C alone shows the no-fallback test is
  the one that catches it).
- **Live**, on a scratch daemon over a copy of the owner's store with his note, GLM's probe script in L1 against L0: in L1,
  hostname `theseus-l1`, no routes, a HOME of 3 entries, `gh` not logged in, and the socket and the store absent though
  both sat inside the workspace root; at L0, 13 routes, 153 entries, everything present. A real offline `cargo build`
  ran in L1 in 1.03 s (0.82 s at L0), and the host's `target/` stayed absent. Under a transient user service with
  `Delegate=yes`, each L1 job got its own cgroup with 2 GiB and 512 pids.
- **Benches, alone**: lifecycle OK (cold start p95 28.1 ms of 50; the probe comes after serving); a plain turn still 5
  frames; an L1 start's p95 22.4 ms against a 25 ms target, with a debug `theseusd` as the init.
- **Gates**: gate 7, 1,526 of 1,526. Gates 1 to 6 each failed for a stated reason and were fixed: the `sandbox`
  input's description grew every request (shortened to +105 tokens); theseus-6a7o's byte; the capped files (the L1
  logic moved into `sandbox.rs`; `toolrun.rs`'s ceiling is 3,330); and a new load flake (theseus-56r7).

**The join** (16:04 to 16:34). A scheduled review run rebased the lane onto 9f4035b (15:31 to 15:34), then hung on a
`pgrep -f` that matched its own command line and was killed at 15:40; Tabitha/Claude took the join over at 16:04.
- **The stop hook** (b503b2c). The second read found a restart hazard, proven on throwaway user units at 16:03 to
  16:05. The first L1 job turns job limits on in the unit's own cgroup, and a unit that keeps its jobs across a stop
  (`KillMode=process`, as the owner's does, Item 56) then can't start its next daemon while an old job runs: systemd 249
  starts the main process in the unit's own cgroup, which the kernel refuses with controllers on ("Device or resource
  busy", `status=219/CGROUP`). 17b's own check couldn't see it, since its transient unit had the default
  `KillMode=control-group`. The fix: `cgroup::release` turns the controllers off, `jobs/` first; a hidden `theseusd
  cgroup-release` role runs it, only from a readied service's `.control`; the installer writes `ExecStopPost=-theseusd
  cgroup-release` into the user, system, and job-host units; and the daemon delegates only under a unit that has the
  hook. With the hook, the same unit restarted cleanly with a job still running, and by itself after a `kill -9`.
  Five new tests failed against five planted faults.
- **cargo's credentials files** are hidden in every view: the template binds `~/.cargo` whole, and a token left by
  `cargo login` would otherwise be readable at notify.
- **NODE schema 3 to 4** (4fc7ddc): the gate record's `decision.class` changed a tool-call node's shape, and main's
  store-version rule (R8's `tests_schemas`, Item 52) caught it at the join. The bump came with a schema-3 read test (one that builds its old bytes through the build's own serializer, so it can't catch a change to how a node serializes: theseus-djfj, Item 61; rewritten on a literal at 19c's join, Item 63);
  the goldens were rewritten in a full suite, numbers only, and the web dist rebuilt.
- **The join's gate**, 16:28:55 to 16:33:48: 1,626 of 1,626, lifecycle OK in 10.6 s, frames 5.

**The install** (16:38). First, the hook live on the release build: a scratch daemon over a copy of the owner's store,
under a transient unit shaped like his, ran an L1 `sleep 40`, then `systemctl --user restart` with the job running:
exit 0, the hook released memory and pids, the job ran on, and the new daemon answered. Its journal showed an `op`
helper left in the cgroup too, so without the hook even a restart with no job running would have failed once an L1
job had run. Then the backup, the copy-then-rename, and `scripts/user-service.sh install --yes`, which rewrote his unit
with the hook. Health 30 s after the restart: serving at 21.4 ms, secrets 8 of 8, Discord and the index ready, and
`sandbox: L1 works (start 5.2 ms)`, the cgroup delegated, 2048 MB a job. NODE is 4 from this build on, so a rollback
is a restore of the backup.

**Divergences.** The approve list's paths and the socket are hidden as well as the floor, which the design named. An
explicit tightening (`[policy.tools] "proc.run" = "approve"`, or "should have asked") didn't reach an L1 call, by
the owner's answer read literally, so the model could step around it with `sandbox: true`. Asked at 16:01, the owner chose at
16:03 that it should, and it does since Item 59 (theseus-jfs6). A job tries L1 itself
after a failed probe, rather than failing at once. `output_mb` caps a file, not what a job prints, which `[tools]
job_output_max_bytes` caps as at L0.

**Known gaps.** A load flake in the web UI's dev-origin test (theseus-56r7, P2, on the flaky list). P3, post-v1: a
hand-built seccomp filter (theseus-75z9); `sandbox.limit_hit` rows, the start metric, and the Observatory's Sandbox
section (theseus-zupk; the section was built in Item 62); no generated config with `default = "l1"` (theseus-2ujy); the root `AGENTS.md` over its
20 KB rule (theseus-9y38; cut to 16,949 bytes in Item 70). The `sandbox` input said an L1 job never waits for approval, which wasn't true in a
session holding outside text; Item 59 dropped the phrase. The rest of L1 is ~~18a (cancellation per backend)~~ (built in Item 60), ~~18c
(egress)~~ (built in Item 62), ~~18d (credentials)~~ (built in Item 65, and replaced by grants at launch in Item 71), and ~~20a~~ (dropped; the integrity lane instead, Item 74). _The probe after serving, the delegated cgroup and its stop hook went with the sandbox trims (Item 77)._

### Item 59. The operator's own word about `proc.run` reaches L1 (theseus-jfs6; built on `main` by Tabitha/Claude, with no lane, 2026-10-02 16:03 to 16:50; 04a9fb3; installed 16:57 at 04a9fb3)

**Why.** Found at 17b's second read (Item 58): an L1 call ran at notify in place of the whole L0 order, so neither an
explicit `[policy.tools] "proc.run" = "approve"` nor a "should have asked" tightening reached it, and the model could
step around either with `sandbox: true`. Asked at 16:01, with three options: keep it; let an explicit per-tool
approve or a tightening reach L1 calls; or the design's own key, `"proc.run@l1"`. The owner, at 16:03: "Let's start with
yes", the second.

**What landed** (§3.9, §7).
- An L1 call's decision takes the tool's own `[policy.tools]` line and its tightening, the stricter winning, as at
  L0. One that asks makes the L1 call wait, and its reason names both what chose L1 and what asked. The call still
  names L1, so it runs there once approved.
- The inherited `[policy].enforcement` is not that word, and never makes L1 wait: L1 still earns its notify over
  it. A looser line never makes L1 quieter than notify. The floor and the approve lists still don't apply to L1,
  whose view hides what they guard.
- `proc.run`'s `sandbox` description drops "It never waits for approval", which wasn't true here, nor in a session
  holding outside text before 20a (which was dropped: Item 74, and theseus-oaf9). The template's `[sandbox]` comment and the core's `AGENTS.md` say so.

**How it is proven.** A new test, `the_operators_own_word_about_proc_run_reaches_l1` (an explicit approve line, and a
tightening, each make an L1 call wait and name L1), fails against a planted revert; the existing L1 tests keep the
inherited approve and an explicit open at notify. The core output golden moved by the description's tokens,
rewritten inside a full suite (377 lines, numbers only). The gate on `main`, 16:51:12 to 16:53:50: 1,627 of 1,627,
lifecycle OK in 11.1 s, frames 5.

**The install** (16:57:29). A release-thin build in 3 m 14 s, the store backed up, `theseusd check` 8 of 8 secrets,
the unit unchanged, and its stop hook ran with nothing to release. Health 9 s after the restart: the config confirmed,
Discord and the index ready, L1 working (start 5.7 ms), the cgroup delegated. theseus-jfs6 is closed.

**Divergences.** None: the option the owner chose, as asked.

### Item 60. Cancellation verified per backend (theseus-7ve.2, with theseus-hcc; M4 row 18, step 18a; spine; 2026-10-02 16:43 to 18:00; reviewed 18:17; rebased onto 08b595d as b77ffe9 and cbcc378, with two join commits, 2702a39 and 1d33622; joined 18:33 at 1d33622; installed 18:37 at 1d33622)

**Why.** theseus-hcc: a cancel reached only the wrapper's process group, so a `setsid` descendant ran on while the
cancel read `termination_verified`, and a deadline killed the command alone while the wrapper lingered on the rest.
§3.16's `termination_verified` is meant to say the job is gone.

**What landed** (§3.15, §3.16, §7; P5b).
- **The wrapper stops its own tree** (`theseus-kernel/src/tree.rs`). The wrapper is a child subreaper, so a double
  fork or a `setsid` stays in its tree. The stop has three phases: SIGTERM to every process and the grace; a freeze
  (SIGSTOP, rescanning until nothing new appears and every process reads stopped, bounded at 500 ms); then SIGKILL
  and the reap. Every signal goes through a pidfd checked against the start time the scan read, so a pid reused since
  is never signalled, and a `D` process that outlives the kill is named as a survivor.
- **The ask.** The daemon asks each wrapper alone, by `sigqueue` with the grace in the signal's value. The wrapper
  stops its tree, writes its verdict to the spool's `stops/<id>`, and exits 0 with no completion. A plain SIGTERM
  stops the tree too, then re-raises, so a killed wrapper still reads as `job.wrapper_lost` (theseus-6uo). The
  deadline uses the same stop, its verdict in the completion's `detail.stop` (hcc's second gap). A wrapper from
  before the install, told by `/proc`'s `SigCgt`, is stopped by its group as before. One that never answers is
  killed with its group after the grace and 2.5 s, and the verdict is uncertain.
- **L1** stops through its init (SIGTERM, which the init forwards, then SIGKILL), by `cgroup.kill` where the job
  has its delegated cgroup (verified at `populated 0`), or by the init's reap, since a pid namespace's init finishes
  exiting only after every other process in it. An async tool's task is aborted and verified once its handle
  finishes; an in-process toollet can't be stopped (`unsupported`).
- **One `terminate_all`** serves a cancel, a task's cancel, `/stop`, the disk floor (Item 54), and the stop at a
  job's launch (Item 37).
- **The verdict** is on the action (ACTION schema 3; OUTBOX schema 2, since a post is an action): `verified_by`
  (`pidns`, `cgroup`, `tree`, `group`, `task`, or `none`), `killed`, `survivors`, `scope`, `ms`, and why when it
  isn't verified, with `action.cancel_verified`, `_uncertain`, and `_unsupported` rows. `theseus executions
  cancel`, `theseus cancel`, and `theseus stop` print it (`⏹️ cancelled proc.run …a1b2c3 (verified: pid namespace, 3
  processes)`), and so do a cancelled job's result, `tool.ended`, Discord's stopped line, and health's `cancels
  since the start:`.
- **A stopped job keeps the end of its output.** The wrapper outlives its tree, so the ring reaches the file at a
  stop (Item 34's lost end).

**How it is proven.**
- **New tests, each against a planted revert** (three batches): a kernel test binary of its own, `tests/tree.rs` (a
  cancel of a job with a `setsid` sleeper, the deadline, an older wrapper, a deaf one, and a plain SIGTERM, each
  checked by a `/proc` scan); the L1 cancel through the daemon (`pidns`, 3 killed); the async abort in the core
  (verified by its task within 2 s); and a read test of a schema-2 action and a schema-1 post, their bytes unchanged
  (the owner's store has schema-1 posts). Ten existing tests that pinned the old behaviour (a `setsid` sleeper surviving
  a cancel, a stopped job's end lost) now assert 18a's; the review read each change, and none was weakened.
- **Gates.** Gate 1 failed four tests for stated reasons, each fixed: the store-version test caught OUTBOX's new
  shape, the first `terminate_all` would have let a job stopped at its launch run on, and two results now carried
  the verdict. Gates 3 and 4: 1,639 of 1,639.
- **Live**, on a scratch daemon over a copy of the owner's store with his note, GLM's jobs (`setsid sleep & exec
  sleep`): at L0, cancelled and stopped, verified by the process tree (2 processes); in L1, by the pid namespace (3);
  and in L1 under a transient unit shaped like his, by the cgroup (3). Each `/proc` scan was empty. Each CLI call
  took 0.08 to 0.13 s, the wrapper's own stop 10 ms.
- **Benches, alone**: lifecycle OK at 17:47; at 17:57 one row missed under the neighbours' IO (the start from the
  config copy, p95 72.3 ms of 57), and its rerun alone was OK (cold start p95 29.5 ms). A plain turn: 5 frames.

**The join** (18:17 to 18:33). The rebase onto 08b595d conflicted only in the core output golden, which Item 59 had
rewritten too. Main's side was taken and rewritten inside a full suite (2702a39, 1,640 of 1,640): 92 lines differ
from main only in their numbers, and one as 18a means it, a stop's `"verdicts":[]`. The first join gate (18:29)
failed one pre-existing timing test, the web UI's refusals burst. Like theseus-56r7's dev-origin test, it needs its
bursts in one 300 ms span of the wall clock, and under load the later burst landed in the earlier span: 4 rows, not
5. It joined 56r7's entry on the flaky list (`.config/nextest.toml`, `retries = 2`; 1d33622), and the rerun was
green at 18:33:26: 1,640 of 1,640, lifecycle OK in 11.2 s, frames 5.

**The install** (18:37:20). A release-thin build in 3 m 34 s. The store was backed up with the five binaries this
time (59 MB), so a rollback needs no rebuild; ACTION is 3 and OUTBOX 2 from this build on, so a rollback also
restores the store. `theseusd check` found 8 of 8 secrets; the unit was unchanged, and its stop hook ran with
nothing to release. Health at 5 s: the config confirmed, secrets 8 of 8, Discord and the index ready, L1 working
(start 7.4 ms), the cgroup delegated, no error in the journal. theseus-7ve.2 and theseus-hcc are closed.

**Divergences**, all eight accepted at the review. SIGTERM comes before the freeze, where the design had SIGSTOP
first: a `git` killed mid-write leaves `index.lock` behind, and one shared grace still covers N stubborn jobs. A
cancelled job writes no completion, since one would race the spool's drain into a `failed` settle; its verdict goes
to `stops/<id>`, and only the deadline's rides in a completion. A plain SIGTERM stops the tree and re-raises. An
older wrapper is told by `SigCgt`, not a marker. `verified_by` names what was tried when a cancel isn't verified,
and `none` is kept for a call nothing can stop. A pid another process holds at the first look is uncertain, not
gone. OUTBOX moved with ACTION. A stopped job's output keeps its end.

**Known gaps** (P3, post-v1; in each the job's processes are gone, and only a record or a surface says less than it
could): the web UI shows no verdict (theseus-aqor; the cockpit's boundaries board does, since Item 64); the `theseus.cancel{backend,state}` metric (theseus-qdk5); a
restart mid-cancel marks the action unknown without reading `stops/<id>`, and old verdicts aren't swept
(theseus-1og8); an L0 job has no cgroup of its own, so the design's "L0 with a cgroup" row isn't built
(theseus-yfdj); a job a stop reached before its pid was written keeps `unsupported`, though its launch's stop
verified it (theseus-vn4d). At L0, verified means the wrapper's descendants (`scope: descendants`): a process
outside the tree acting for the job, a user unit or a tmux server already running, is out of its sight, which L1's
view closes. theseus-56r7 (P2) now covers two web UI tests on the flaky list. _(56r7 was fixed in the cloud, Item 73. Since Item 77 an L1 stop is verified by its pid namespace alone.)_

### Item 61. Confidentiality labels (theseus-7ve.3; M4 row 21, step 19a; spine; 2026-10-02 17:12 to 18:47; reviewed 18:49 to 18:50; rebased onto 1d33622 as 42a27af and 3e79aac, with one join commit, 8b82da6; joined 19:08 at 8b82da6; installed 19:13 at 8b82da6)

**Why.** M4's confidentiality half (the M4 design's §2.5 and §2.7). Theseus will speak where more than the owner
listens, and once private material is in the model's context nothing reliably keeps it out of what the model says
(§3.9). So it is kept out of the context: each compile admits a node only when the session's audience may read it.

**What landed** (§3.9, §4.4a, §4.4b; P5b, P6).
- **Labels on nodes** (NODE schema 5). A label is a node's integrity (`trusted`, or `untrusted` with T1's
  `ExternalText` as its source) and its readers (anyone, a place's viewers, named people, or the owner alone). The
  writer sets it in the frame that writes the node (`labels.rs`), and nothing rewrites it:
  - the operator's words are labeled by the connection's surface, never by the author a client claims: the owner's
    from the CLI and the web UI, the place's through Discord;
  - a fetched page is untrusted and anyone's;
  - a file, a diff, or a text tool's result is the owner's, unless every path the call names is in a `[labels]
    public_paths` tree, and a program's output (L0 and L1 alike), AWS's, and the harness's tools' are the owner's;
  - the model's answer is trusted, read by the meet of what its request admitted, so an answer that drew on
    owner-only material is owner-only.

  A node from before has no label and is read only in its own session.
- **The audience** comes from where the session posts: the owner alone, a DM's person, or whoever can view a guild
  channel, as the binding last read it, and public when that can't be read (no Server Members intent). The
  binding's one walk of a channel's viewers serves the approval check too, at connect, on a channel or role change,
  and before a turn there when its last read is a minute old; META keeps it, so a restart keeps every audience.
  **The owner** is the local surfaces and `[labels] owner`, which defaults to `[approval] trusted_users`; with
  neither section, a bound DM's person is the owner, as approval takes them.
- **The compile filter.** A `Judge` per turn, fixed as the spec is, judges each node where it renders. A withheld
  node is a one-line placeholder, so every call keeps its result (`[withheld: fs.read's result is labeled
  owner-only, and this session's audience is #lab (3 people)]`), and a context file becomes its header and why.
- **The manifest** (COMPILATION schema 4) records the audience, the meet of what the prefix admitted, the integrity
  in play, and the withheld nodes, and a compile for another audience recompiles (trigger `audience`).
  `context.compiled` carries the audience and the withheld count, beside `label.withheld` and `label.audience` rows.
  `theseus labels` shows a session's audience, what its prefix withheld, and each node's label (🔒, 👥, 🌐); the
  Observatory badges each message; health has a `labels:` line; the template documents `[labels]`.

**How it is proven.**
- **Tests, each against a planted revert.** `tests_labels.rs` covers a two-viewer channel that withholds an
  owner-only result and keeps its call paired, a DM that admits everything, an audience change that recompiles and
  withholds what the prefix held (the answer that drew on it too), a context file withheld with its reason, a
  channel nobody can read counting as public, a public tree, and the old-store fixture compiling exactly as before.
  Beside it: the binding's push end to end through the fake Discord, and the NODE 4 and COMPILATION 3 read tests.
  Two proofs needed a second plant: proof 8's first did not compile, and proof 6's passed (below). Gates: 1,645 of
  1,645 at e3e8929, and 1,646 of 1,646 at 1e02040, a plain turn at 5 frames in each.
- **Live.** On a copy of the owner's store with his note, his DM compiled for his person alone, nothing withheld, with an
  owner-only file read admitted, and his older DM session recompiled for its system and tools, never its audience.
  On the stand-ins, with `#lab` open to a third member the bot answered with the placeholder, and private to the
  owners with the file; health said `#lab: 3 can view it, 1 not the owner: owner-only material is withheld there`.
- **Benches, alone**: every lifecycle budget held (cold start p95 43.6 ms). In the dev profile, the compile filter
  adds nothing measurable to a 1,000-node compile that withholds nothing, and about 1 ms to withhold 250.

**A lesson: an old-layout read test takes the old bytes as a literal.** 19a's NODE 4 read test first built its
"old" bytes with the build's own serializer, so a planted change to how a node serializes moved both sides, and the
test passed against its plant (proof 6). 1e02040 rewrote it on a literal layout, and it now fails against the plant.
The schema-3 read test of 17b's join (Item 58) has the same shape and stands as written; theseus-djfj tracks it and
every other old-layout read test.

**The join** (18:50 to 19:08). The rebase onto 18a (1d33622) met it in seven files and kept both sides in each: the
store's schema table (ACTION 3, NODE 5, COMPILATION 4, OUTBOX 2) and read tests, the tool runtime's fields (18a's
stops, 19a's public paths), and health in the protocol, the CLI, and the TypeScript. The goldens took main's side
and were rewritten in a full suite (1,659 of 1,659): against main, the core output differs in 60 lines only in
numbers and in 73 that gain the manifest's audience, and the kernel frames in 17 lines, numbers only. 18a's cancel
test gained 19a's `from_discord`. `toolrun.rs` reached 3,362 lines, so its shape ceiling went to 3,370, with its
split filed as theseus-5gw9. The Observatory's dist was rebuilt with main's packages, since the lane had no vite.
The join's gate (19:07:38): 1,659 of 1,659, lifecycle OK in 10.4 s (cold start p95 30.5 ms), frames 5.

**The install** (19:13:02). A release-thin build, the store and the five binaries backed up (59 MB); NODE is 5 and
COMPILATION 4 from this build on, so a rollback also restores the store. `theseusd check` found 8 of 8 secrets; the
unit was unchanged, and its stop hook ran with nothing to release. Health at 5 s and 115 s: the config confirmed,
secrets 8 of 8, Discord ready with the DM and #openclaw bound, the index ready (94 nodes), L1 working (21.8 ms),
serving at 22.9 ms, and no warnings in the journal. The new `labels:` line: the owner is the CLI, the web UI, and 1
person on Discord; #openclaw has 8 viewers, 7 of them not the owner, so owner-only material is withheld there (the
Server Members intent works). The owner's DM, the CLI, and the web UI withhold nothing, and #openclaw's session had never
run a turn. theseus-7ve.3 is closed.

**Divergences**, all accepted at the review. The audience recompile is a compile trigger (the manifest's audience
against the session's now), not a stored `pending_recompile` mark: that needs no SESSION bump, and it catches a
change made while the daemon was down. Any change of audience recompiles, not only one that changes an admission. A
compilation from before 19a keeps appending until something in its session would be withheld. The placeholders name
no command until 19c brings `theseus graduate`. A bound DM's person is the owner without `[labels]` or
`[approval]`. The binding asks for no member events, so it reads a channel's viewers before a turn there. NODE is 5,
not the design's 3.

**Known gaps** (P3, post-v1): ~~the `theseus.compile.withheld` metric (theseus-63xf)~~ (built in Item 67); a guild member who joins between
viewer reads isn't in the audience until the next read (theseus-4qiz; 19c's held post re-checks a reply at post
time, Item 63); ~~the filter's bench in an optimized build, against its 50 µs budget (theseus-gagg)~~ (built in Item 67: with nothing withheld it adds nothing measurable, and withholding 250 of 1,000 nodes adds about 200 µs, theseus-2lvc); the harness's own tools'
results and a report with no answer are owner-only, where the session's audience would do (theseus-el4l); ~~read tests
on literal old layouts (theseus-djfj)~~ (built in Items 63 and 67); a recompile only when an admission changes (theseus-osl1). `graph::Label`,
the vocabulary Item 32 kept for 19a's first labels, is still empty, since a label is a field of the node; it was removed in theseus-i25g (Item 70). Next in
M4: ~~the disclosure simulator (19b), graduation and the held post (19c)~~ (built in Items 66 and 63), ~~and the latch fed by labels (20a)~~ (dropped, the cut-list's Tier 1.1: Item 74). _The labels themselves were removed by the place rule on 2026-10-03 (Item 76)._

### Item 62. Egress wired in (theseus-7ve.4; M4 row 19, step 18c; spine; 2026-10-02 19:19 to 20:30; reviewed 20:43 to 20:44; rebased onto 65562b3 as 415f669 and 02c4bcf, with one join commit, 79f2d1d; joined 20:50 at 79f2d1d; installed 21:08 at f79d52e, with 19c)

**Why.** L1 jobs had no network since 17b. 18b built the egress proxy in `theseus-sandbox` (merged ahead of its
reader at 1fe9c0e), and nothing ran it. This step lets an L1 job reach the hosts the operator lists, and the extra
hosts a call names once that call is approved, and makes what such a job brings back count as outside text (T1),
closing theseus-20f for L1.

**What landed** (§7's egress paragraph; the M4 design's §2.4).
- **The list and a call's hosts.** `[sandbox] egress`, checked at load. `proc.run`'s `sandbox` takes `true` or
  `{ egress: [...] }`, and the plan checks each entry (a bad one is invalid input). `Allow` moved to
  `theseus_tools::net`, and the core's resolver reads the one address classification there (18b's copy is gone).
- **The gate's step 2.** Hosts beyond the list make an L1 call wait; the reason names them, and the approval
  reaches them alone. `sandbox::Bound { class, egress }` is what a proposal binds (`policy_context.egress`), so the
  gate record and the confirm's digest hold the hosts, and a confirmed call runs with exactly its list.
- **The wrapper runs the proxy** (`job_egress.rs`) for a job with a list, and stops it once the job has ended.
  `Running::finish` ends a tunnel a server holds open, so every one is recorded. The completion's `detail.egress`
  is a summary: one entry per host reached (connections, bytes each way, ms) and per refusal (why, how many). No
  store bump: it rides in the detail JSON, and `via: "egress"` is a new value of an existing string.
- **Outside text.** A result whose job connected out is marked external (`url`: the hosts it reached), its
  session's hold says `via: egress`, and its node is untrusted, its readers the owner's. The hold rides in the frame
  that writes the node on every path: read in the turn (`external::with_hold`), read late by the next turn, and
  swept after a cancel (`external::under_hold`, which takes the session's lock only when a frame carries outside
  text). A job that left no report (a stop) and printed something counts as outside text when its proposal bound a
  list.
- **Rows and surfaces.** `sandbox.egress` and `sandbox.egress_refused` in the result's frame; `ToolStarted.egress`
  and `ToolEnded.reached`; Discord's `🛡️ L1 · egress: …` with the hosts reached; the web UI's L1 pill and a new
  Observatory Sandbox section; health's egress counts and the CLI's line; the completion's `egress` on
  `action.list` (and in `theseus executions explain --json`); and the narrative's "reached …" line.
- **The seam for gh7.** Each `CONNECT`'s outcome is a value (`Outcome::Tunnel | Refuse`) the proxy acts on in one
  match. Credentials as stand-ins would have added a third; they were dropped for v1 (Item 69), and the proxy's docs
  no longer promise it.
- **The template** gains a commented egress list for crates.io and GitHub, with the two things an operator should
  know before uncommenting it.

**How it is proven.**
- **18 new tests, a bench case, and the template test's new asserts**, each against a planted revert. Twelve plants,
  each caught at the assertion it targets: the gate's step 2, the proposal binding the list, the marker, the
  label's readers, T1's hold for an L1 call with egress, the late result's hold, the rows riding in the result's
  frame, the template, the wired proxy, the list match, the public-only resolver, and the stop's recording. The
  daemon's tests run real L1 jobs through a real proxy to a stand-in host on 127.0.0.1: a listed host reached with
  its bytes in `detail.egress` and the rows; an unlisted host's 403; names that resolve to loopback or to
  169.254.169.254, and that address, refused through the wired path; a deadline that ends a job mid-tunnel, its
  connection still recorded; no list, no proxy.
- **Live**, on a scratch daemon over a copy of the owner's store with his note (no egress listed). GLM's `proc.run {
  sandbox: { egress: ["api.github.com:443"] } }` of a Python fetch waited for its host, ran once approved, and
  connected (731 B up, 4.7 KB down, status 200). The session then held outside text, `via: egress`. The same call to
  PyPI got the proxy's 403, its reason in the result and in a `sandbox.egress_refused` row. A later call in that
  session waited on the hold: "this session read external text (proc.run's egress to api.github.com:443, at
  20:19)".
- **Benches, alone**: lifecycle OK (cold start p95 32.9 ms); a plain turn 5 frames; an L1 call's turn writes the
  same frames with egress or without; an L1 start's p95 8.6 ms, 11.3 ms with a list (budget 25); a `CONNECT`'s first
  byte 0.5 to 0.6 ms slower through the proxy at p50 (§2.10's budget: 2 ms).
- **Gates**: 1,676 of 1,676 at each commit. The second commit trimmed `proc.run`'s schema from +196 estimated tokens
  a request to +103, since every request pays for it.

**The join** (20:44 to 20:50). `main` had moved only in docs (65562b3), so the rebase was clean. The Observatory's
dist was rebuilt with main's packages (79f2d1d), since the lane had no `web/node_modules`. The join's gate
(20:50:01): 1,676 of 1,676. Its first lifecycle run missed the cold start's p95 on one 69.8 ms outlier; the rerun
passed (33.4 ms). The install waited for 19c, so that one restart carried both.

**The install** (21:08:10, at f79d52e, with 19c). A release-thin build (3 m 44 s), the store and the five binaries
backed up (60 MB). NODE is 6 from this build on (19c, Item 63), so a rollback also restores the store. `theseusd
check` found 8 of 8 secrets, the unit was unchanged, and its stop hook ran with nothing to release. Health at 5 s and
30 s: the config confirmed, secrets 8 of 8, Discord ready with the DM and #openclaw bound, the index ready (98
nodes), L1 working (5.8 ms) with "no egress listed", the labels line unchanged, serving at 21.6 ms, and no warnings
in the journal. His note lists no egress, so his L1 jobs reach nothing until it does. theseus-7ve.4 is closed.

**Divergences**, all accepted at the review. `detail.egress` keeps one entry per host reached and per refusal, not
every connection, so a crate fetch of hundreds of connections stays small. The rows ride in the frame that writes
the job's result, where T1's hold must ride, not in the kernel's completion frame. `executions --json` lists
executions, which carry no completion, so the completion's egress is on `action.list`. A stopped job, which leaves
no record of its connections, counts as outside text by its list when it printed something. The Observatory's
Sandbox section, 17b's gap, is built here.

**Known gaps** (P3, post-v1): a stopped job's egress is not recorded, so its result is held by its list
(theseus-fdxy); the egress bytes metric (theseus-zupk). The rest of theseus-20f, outside text at L0, was 20a's (dropped: a listed program's output is marked since Item 74). Two
facts for an operator's list: L1's view has `python3.12` but no `python3` on this machine, and `~/.cargo` is
read-only in L1, so a `cargo` that fetches crates needs a writable `CARGO_HOME` (untested). _(Since the sandbox trims, Item 77, only a host beyond the operator's list makes a result outside text.)_

### Item 63. Graduation and the held post (theseus-7ve.5; M4 row 23, step 19c; spine; 2026-10-02 19:21 to 20:41; reviewed 20:43 to 20:56; rebased onto 79f2d1d as 751780d and ec4d9bb, with two join commits, 3561360 and f79d52e; joined 21:04 at f79d52e; installed 21:08 at f79d52e, with 18c)

**Why.** M4's confidentiality story had two pieces left after 19a (the M4 design's §2.7): a way for the operator to
widen who may read something, and a check where content leaves, since a guild channel's audience can grow between a
compile and its post (theseus-4qiz's window).

**What landed** (§3.9; P5b, P6).
- **Graduation**: `label.graduate`, `theseus graduate <node> --to public|place|people:<ids> --why "<warrant>"`, and the
  web UI's Graduate, judged by `judge_act`'s `Graduate` as an approval is (never from a job's process; under
  `[approval]`, only a trusted user through a trusted channel). It writes a new operator node in one frame: the
  source's content and integrity, the wider readers, and a warrant (the node it came from, who, through what, why,
  and when; NODE schema 6), with a `derived_from` edge (`via: graduate`) and `label.graduated`. It is never a
  relabel, and it is refused while a turn holds the session. The next compile appends it; the source's placeholder
  stays where it was, its call still paired, and a placeholder names the command.
- **The held post**: `labels::may_leave`, asked by the binding before a reply or a task's report leaves for a guild
  channel, on a fresh read of who can view it. Readers that fit any audience the channel can have (public, or the
  channel's own words) need no read. A post that may not leave is held, never refused: a question (`label.release`,
  a kernel action like the budget question, so every surface's confirm answers it) whose card goes to the approvals
  DM, never the channel itself, with `label.held_post`. Approve posts it; decline leaves "a reply was held back". It
  has no expiry, and the place's later posts wait behind it. Health counts held posts.
- **Quiet loops**, the lane's addition: `context.compiled` carries the request's readers. In a guild channel a loop
  whose request drew on what not every viewer may read streams no text, and its tool lines show 🔒 for their input,
  so the checked post is where its words first appear. A DM's audience is fixed: its posts are never checked, and its
  loops always stream.

**How it is proven.**
- **Tests, each against a planted revert** (nine proofs): a graduated result admitted at the next compile, its
  placeholder paired; a job's process and an untrusted channel refused; a held post approved and declined end to end
  through the fake Discord, the channel opened mid-turn; a fitting post with no new frame; an unreadable audience
  counted as public; the NODE 5 read test on literal bytes; the quiet loop; the plain turn at 5 frames. Frames: a
  graduation 1, a fitting check 0, a hold 1, an answer 2. Gates: 1,668 of 1,668, then 1,669 of 1,669.
- **Live.** On a copy of the owner's store, a graduated node was written and read back after a restart. On the
  stand-ins: a fitting post read its channel in 4.35 ms and wrote nothing; a channel opened mid-turn held the reply,
  with nothing of it in the channel, its card reached the owner's DM, and Approve posted it; a withheld result,
  graduated to the place, reached the next turn's request as an append. The web UI's Graduate button was built and
  type-checked, not clicked.
- **Benches, alone**: lifecycle OK (cold start p95 47.0 ms); a plain turn 5 frames.
  `may_leave` costs 326 µs a call at worst (a channel of 1,000 viewers, all of them owners, unoptimized), and under a
  microsecond for a channel of a few. It runs once a post, never once a node.

**The join** (20:46 to 21:04). The rebase onto 18c met it in four files and kept both sides: the Discord renderer
(18c's L1 reach and 19c's quiet flag), the protocol's `AGENTS.md`, and the core output golden and the dist, which were
regenerated. theseus-djfj's first part rode along (3561360): 17b's NODE 3 read test (Item 58) takes its old bytes as
a literal, and a planted removal of the decision class's `skip_serializing_if` now fails it. The goldens were rewritten
in a full suite (1,686 of 1,686) and checked line by line. The join's gate (21:03:51): 1,686 of 1,686, cold start p95
29.5 ms, a plain turn 5 frames. `toolrun.rs` reached 3,368 lines of its 3,370 and the renderer 2,926 of 2,930.

**The install**: with 18c at 21:08:10 (Item 62's install). NODE is 6 from that build on. theseus-7ve.5 is closed.
Nothing of the owner's is held: his DM's audience is fixed, and #openclaw's requests already leave his owner-only
material out, so its loops carry the channel's own readers and stream.

**Divergences**, all accepted at the review. The hold needed no record layout: its question is an action of an
existing shape and the hold its card's `held` field, so OUTBOX stays 2 and ACTION 3. Quiet loops close the stream's
half of the window, which a check at post time alone could not; their cost lands only in a guild channel whose
viewers are all owners, where a loop that read the owner's files posts whole after a fresh check instead of
streaming (theseus-42st asks for member events to lift it). A card in a guild channel relies on the approval check,
which reads the channel fresh already. Graduation is refused while a turn runs. The held post blocks its place's lane.

**Known gaps** (P3, post-v1): a card is not checked by `may_leave` (theseus-dqs9); graduation during a turn
(theseus-b97e); kernel-sim does not drive the held post's question (theseus-tbv2); the post-time read walks a guild's
whole member list (theseus-zupl); a quiet loop's tool lines keep their lock after the post (theseus-033g); the
metrics, added to theseus-63xf. tbv2, zupl and 033g were built in the cloud and are parked on the simplification
review's Tier 2 (Item 70). _(Graduation and the held post were removed by the place rule, Item 76, and tbv2, zupl and 033g closed as moot.)_

### Item 64. The cockpit's Ship, its time machine, and its boards (theseus-logs; the `cockpit3` lane, two rounds in one Item; round one 2026-10-02 18:15 to 20:16, reviewed 21:11, joined 21:21 at 2224c5d as c38f014, afd7972 and 2224c5d, installed 21:24; round two 20:17 to 22:27, reviewed 22:29 to 22:36, joined 22:40 at bfbe47b as 3806da3, 0f71a8f and bfbe47b, installed 22:45)

**Why.** The owner asked for a hero terminal for the cockpit, with "steampunk and cyberpunk vibe stylings", and at 18:14
approved four more views for a second round ("all of 1-5 plus everything else you said is golden"). The two rounds
are one lane's, so they are one Item.

**What landed in round one** (`cockpit/` only: no Rust, and no daemon method).
- **The Ship**: one agent over one graph, drawn as a fleet at night. Places sit in engraved brass rings; a session is
  a galley whose keel lights are its messages and model calls, with an oar for each tool call, a shield for an L1
  call, and the hold's chain when it holds outside text. A task is a boat on a tether, under sail while it runs, and
  a gold run marks its report landing; a lantern waits on the operator, and a flare is a failure.
- **The bridge** around it: brass gauges on real health (the compass, gate pressure, fuel, the engine telegraph, the
  chronometer, and nixie tubes for tokens a minute). _(Since 2026-10-07, Part III Item 239: the console keeps the engine and tokens a minute, with a sea gauge in words; the compass and the chronometer retired on the owner's C3 (the top bar's profile chip and UP hold their data, and the header's "then" their moment's), and the gate and fuel had gone into the watch.)_
- **The restyle**: every view takes the brass-and-neon look through the shared tokens and components.
- **Real data only**: every element maps to an existing read or push. three.js is drawn directly, with no React
  reconciler between the data and the GPU, and with a post-processing chain of the lane's own: the `postprocessing`
  package is Zlib-licensed, and three's addons were too slow on a CPU rasteriser. three.js loads only with the Ship.
  The dist grew from 4.20 to 4.90 MB, and the release binary, which embeds it, stays near 39 MB of its 60 MB budget.

**What landed in round two.**
- **The time machine**: a brass "ship's log" strip under every page. The Ship, Fleet, Actions, the gauges, and the
  money river show the state at the needle's moment, with an AS OF badge and the moment in the address, folded in
  the browser from the whole ledger.
- **The boundaries board**: the latch with its log and the Trust button; approvals and tightenings with Undo; live L1
  gauges from each job's cgroup; finished and cancelled jobs with 18a's verdicts; the broker by name only; 19a's
  labels and 19c's held posts; and 18c's egress as a harbour chart. The Ship's shield collapses on a verified cancel.
- **The money river**: a Sankey from sessions to models to token kinds to dollars, each call scaled to its recorded
  cost, beside the pace, the budgets with their held pools, and the cache's saving.
- **The speed wall**: the README's six promises as dials against their budgets, the last start's phases, every
  start, each turn's harness overhead, the write path, and every gate's bench history.
- **Three read-only protocol additions**, none taking a path from the caller: `ledger.tail` gains `after` and answers
  `next` (an index range read by kind, so the ledger can be read whole, a page at a time); `bench.history` (only
  `last`; the CSV on the daemon's own machine); and `sandbox.usage` (no parameters; each running L1 job's cgroup).

**How it is proven.**
- **Round one**: gates of 1,627 tests; a live check of 13 GLM turns ($0.017) on a copy of the owner's store that drove
  every state (shields, the hold's chain, boats under sail, lanterns, a flare, the gold run); the reviewer read the
  screenshots. This machine's headless Chrome has only SwiftShader, a CPU rasteriser, so every frame rate is a floor:
  49 fps on his store with the hologram (adaptive), 57 in calm mode, 37 at 10,000 nodes, and a first frame in 0.59 to
  0.85 s. The engine's CPU side is 0.4 to 0.6 ms a frame, and the loop draws only while something moves. _(Since
  the cockpit-swell lane, 2026-10-04, theseus-wp2d: the sea's swell is ambient. In Live mode an idle Ship draws the
  swell alone at a low idle rate, one composite pass a frame; everything else still draws only while something
  moves, Calm draws one frame and stops, and a hidden tab draws nothing.)_
- **Round two**: gates of 1,690 tests. A scrub redraws in under 100 ms at p95 on every folding view (the Ship 83 ms,
  Fleet 83, Actions 50, the river 50), and the fold itself takes 0.1 ms at the median. At the present, the fold
  agreed with the daemon's own lists on every execution's state, the waiting question, and the holds; the one
  session that differed is older builds' accounting (theseus-lluv). An A/B in palindrome order found the ship's log
  slowing the Ship's first frame by 280 ms, fixed (909 ms against round one's 891). Live, on a scratch daemon run as
  a transient user unit with `Delegate=yes`: a job's gauges at 156 to 158 MiB of 2 GiB and 6 of 512 processes; a
  stop verified by the cgroup (5 processes, 11 ms); an egress reach and a refusal; a tightening and its undo; a
  trust. The dist is 4.99 MB.

**The joins.** Round two was working in the lane's worktree, so round one joined from a throwaway branch, rebased
cleanly onto f79d52e (31 files, all `cockpit/`). Its first join gate failed only in the cockpit step: the chain
tree's `cockpit/node_modules` lacked three.js, and `npm ci --offline` from the same lock fixed it. The rerun (21:21:16)
passed: 1,686 of 1,686, lifecycle OK, a plain turn 5 frames. Round two fast-forwarded `main`. Its first join gate died
with the exec call that started it, leaving two of spine 18d's compilers stopped by its pauser, which were resumed by
pid; the rerun, a managed background job, passed at 22:40:40.

**The installs.** 21:24:50, at 2224c5d: a release-thin build (3 m 12 s), `theseusd check` 8 of 8, health clean, and
`/cockpit/` answering 200 with its assets, the Ship's 630 KB chunk among them. 22:45:20, at bfbe47b: a release-thin
build (4 m 06 s), the store and binaries backed up (61 MB), no store bump, check 8 of 8, the unit unchanged; health
with the config confirmed, secrets 8 of 8, Discord and the index ready, L1 working (15.0 ms), serving at 37.3 ms, and
no warnings in the journal; the cockpit served its new entry chunk and the Ship's. theseus-logs is closed.

**Divergences**, accepted at the reviews: three.js without a reconciler; the lane's own post-processing; the dist
left uncommitted, as the repo already ignores it; three read-only methods in round two.

**What the speed wall says.** On this machine a turn's harness overhead reads far over the README's 5 ms: 194 ms in
the review's screenshot, mostly the disk's fsyncs on WSL, on debug-built turns. The wall shows it and splits the
disk from the rest; a release-build measurement and a decision on that promise are theseus-4w1h (P2).

**Known gaps.** A real-GPU frame rate (theseus-n2hd) and the history held whole in the page (theseus-rtoq), P3. Built
since in the cloud (Item 70): old calls' shields from the node (theseus-93ey), the Ship below 1280 px (theseus-uovv),
the reach cap shown (theseus-7mcu), the board and the session deck under the time machine (theseus-j4qe), an install
named in the log (theseus-9o5n), and a running job's command (theseus-kpz1); frames per live turn (theseus-wz4y) is
parked. Five timing tests failed the lane's gates under load (theseus-f6f5, -so1a, -mll1, -lc4n, -vy7y; P2,
`gate-flake`). _(All five fixed in the cloud: Item 73.)_

### Item 65. Credential requests at run time, built and then removed (theseus-7ve.6, with theseus-5gw9; M4 row 20, step 18d; spine; 2026-10-02 21:09 to 22:58; reviewed 22:59 to 23:04; rebased onto bfbe47b as c496f80, b981439, 1c59652 and 328e635, with one join commit, b63b483; joined 23:09 at b63b483; installed 23:15 at 74a009b, with 19b; removed 2026-10-03 at 8067161, Item 71)

**Why.** Decision 15 (2026-09-29): in the sandbox, a job's run-time credential request follows the posture of the
tool that started it. M4's design gave an L1 job no secret at its start and a way to ask for one while it ran, and
17b withheld spawn grants in L1 until this step.

**What it built.** First, `toolrun.rs` split below its ceiling (theseus-5gw9): 3,368 lines to 1,817, its long-files
entry gone, with no change in behaviour. A job's call went to `toolrun/job.rs`, the continuation to
`toolrun/resume.rs`, and late and cancelled results to `toolrun/late.rs`. Then an L1 job could ask for a secret
while it ran:
- `theseus-cred get <name>` inside the job (the daemon's own binary, bound read-only at
  `/run/theseus/bin/theseus-cred`, its role picked by `argv[0]`) asked through a socket the daemon served for that
  job alone, `<spool>/broker/<job>/sock`, its directory bound at `/run/theseus/broker`, from before the job's launch
  until its call settled. Binding the directory, not the file, let a daemon restarted under a running job serve it
  again. Each connection was traced to the job's own wrapper, so another process of the operator's user was refused.
- Decision 15 judged it: a name `[broker]` names, at the stricter of the call's posture and the secret's. Open
  granted; notify granted with a 🔑 notice; approve waited on a card until the job's deadline. A call the operator
  had approved ran at approve, so its job's requests waited too; the owner kept that reading (theseus-zjxd, 23:04: "keeping
  the secrets at approve if the command that required them required approve").
- A request was a `cred.request` action whose `parent` was the job's call (ACTION schema 4, OUTBOX 3), decided in its
  own frame and never dispatched, with `secret.requested`, then `secret.granted { via: request }` or
  `secret.declined`. Its value went from the board, over the socket, to the helper's stdout, and nowhere else.
- Discord's notice, the CLI's watch line, the web UI's card, health's `cred_requests`, and the narrative showed it.

**How it was proven.** 15 planted reverts, each failing its tests: decision 15's table, notify's one frame and its
notice, approve's card and decline, J1, an input error, a latched session, the socket's life, an L0 job refused, the
ACTION 4 read test on literal bytes, the value in no file and no log, the template, the helper's role, and a restart's
re-serve. The daemon's tests ran real L1 jobs and the real helper. Gates: 1,699 tests, then 1,700, a plain turn 5
frames. Live, on a scratch daemon over a copy of the owner's store with his note: GLM's L1 job ran `gh api user` with
`GH_TOKEN="$(theseus-cred get github_token)"` and answered `zeroaltitude`, with the notice; an egress-approved call's
request waited on its card and, approved, answered; at approve a declined card was the error the job reported; the
93-byte token was in none of the run's 7 files. A request's round trip at notify was p50 16.2 ms (about 7 ms of it
one frame's fdatasync), and an L1 start with the socket bound in was unchanged (p50 7.6 ms against 7.7).

**The join** (23:00 to 23:09). The rebase onto the cockpit's round two (bfbe47b) met it only in the protocol's
TypeScript export list, where both sides were kept. The Observatory's dist was rebuilt at the join (b63b483), which
was the first type check of the web UI's credential card; the goldens needed no rewrite. The join's gate: 1,704 of
1,704, lifecycle OK, a plain turn 5 frames; pushed 23:09:37.

**The install** (23:15:48, at 74a009b, with 19b). A release-thin build (2 m 58 s), the store and binaries backed up
(61 MB); ACTION 4 and OUTBOX 3 from this build on, so a rollback also restores the store. `theseusd check` 8 of 8, the
unit unchanged. Health: the config confirmed, secrets 8 of 8, Discord and the index ready, L1 working (4.5 ms),
serving at 18.3 ms, no warnings in the journal, and the cockpit served. theseus-7ve.6 and theseus-5gw9 are closed.

**Removed the same night.** At 23:57 the owner set the default-trust principle (§2): nothing extraordinary or complex
for trust, safety, or provenance. The simplification review that followed (theseus-vm3n) found the socket was the
heavier way to the same end: a listener and a socket directory per L1 job, a role of the binary, the `/run/theseus`
binds, a re-serve after every restart, an action kind, two ledger kinds, and a health block. Its one known gap
(theseus-3m11: a requested value the job printed sat in its raw output file until its result was written) took a
second private socket per job to close (Item 69). Granting at launch, as L0 always had, needs none of it. The owner
approved that cut at 01:42 ("a worthy simplification"), and the grants step deleted the socket, the helper, the
request action's hooks, and the records, about 2,460 lines net (Item 71). The owner's deployment never made a request:
his store holds no `cred.request` action and no `secret.requested` row. `Action.parent` stays, so ACTION 4 and OUTBOX
3 stay, and a stored request still reads whole.

**Divergences**, accepted at the review: the directory's socket, the request never dispatched, each connection
traced, an approved call's requests waiting, and spawn grants still withheld in L1 (theseus-7y9y, since superseded:
Item 71 grants at launch in both classes).

**Known gaps, as they stand.** theseus-3m11 closed as moot with the socket; theseus-zjxd answered; theseus-6ype
(kernel-sim's request) closed, superseded; the helper's git form (theseus-9oyy) is moot and waits on the operator; the
root `AGENTS.md`'s size (theseus-ltaq) was cut in the cloud (Item 70).

### Item 66. The disclosure simulator (theseus-7ve.7; M4 row 22, step 19b; the `disclosure` lane; 2026-10-02 21:09 to 22:47; reviewed 22:59 to 23:04; rebased onto b63b483 as 36a72c1, 47d2d11 and 74a009b; joined 23:12 at 74a009b; installed 23:15 at 74a009b, with 18d)

**Why.** M4's Prove says "Disclosure tests pass", and P6 says private material never reaches a public audience's
context (the M4 design's §2.7). 19a built the compile filter, and 19c graduation, the held post, and quiet loops. A
property test holds them to that over every interleaving a seed can make.

**What landed** (§3.9; P6).
- **`theseus-sim disclosure --seed N --steps M`** (`crates/theseus-sim/src/disclosure/`, ten files): a whole core in
  process (an unsynced temporary store, stand-ins for `http.fetch` and `proc.run`), with the simulator playing the
  Discord binding and the driver. Its world, made from the seed, has the owner and others, guild channels whose
  viewers change (mid-turn too), DMs and the CLI, a private and a public tree of files, context files of both kinds,
  attachments, fetches, jobs that connected out, tasks with briefs and reports, graduations, held posts and the
  owner's answers, trusts, and foreign nodes standing in for recall.
- **A leaky model and an independent oracle.** Every piece of content carries a marker, and the model repeats every
  marker its request carried, the worst case. The oracle keeps its own copy of the design's rules and never reads the
  core's labels; it judges the bytes that actually left: each request, each streamed edit, each post.
- **The invariants**, after every compile, streamed edit, post, and step: a request carries only what its audience
  may read, whole nodes and placeholders both; a post's content may be read by who views its place when it goes, or
  the owner released it; no text streams into a guild channel unless every viewer it could have may read it; every
  tool call keeps its result; and a session holds external text exactly when T1's sites say. A failure names its
  seed, step, invariant, markers, and nodes, and the seed reproduces it.
- **The gate's short run**: four fixed seeds of 30 steps in `tests/sim.rs`, about 2 s, beside kernel-sim's.
- **Known gaps** (`atoms::KNOWN_GAPS`): a filed spine gap the run counts instead of failing on, printed after every
  run, until its fix deletes the entry; `--strict` fails on them.
- `Store::open_unsynced` is no longer test-only; only a test and the simulator reach it.

**How it is proven.**
- **Planted bugs**, each a one-line change to the core, built and run, then put back: a compile filter that renders a
  withheld message's files, caught in 8 of 12 seeds within 40 steps (the first at step 1); a held post's waiting
  question read as released, 7 of 12; a loop's readers taken from the prefix alone, 5 of 12; a reply's readers from
  its first loop only, 1 of 12. Each fails in the gate's four seeds.
- **Unit tests** for the world, the oracle's `covers`, a graduation's grant, the pairing check, a report's title, what
  each known gap excuses, and that a seed reproduces its run exactly. Gates: 1,694 of 1,694, then 1,697 of 1,697.
- **Live**: 40 seeds of 2,000 steps passed in 533 s, four at a time, with the two gaps below counted: 93,974 compiles
  checked, 330,736 nodes withheld, 602 posts held (352 released, 250 held back), 3,231 graduations, 3,471 quiet
  loops, 7,206 tasks, and 493,662 invariant checks.

**What it found.** Two spine gaps, both P2, both reproduced from one seed. The meet that labels an answer left out
the context files its request carried, so an owner-only file could stream into an owner-only channel, skip the read
at post time, and stay admitted once the channel grew (theseus-42ub, on the first runs). A task's report carried its
title, cut from its brief, with only the task's answer's readers (theseus-jpff, 14 of 40 seeds). Neither reached
the owner: he has no context files, and #openclaw's requests already leave his material out. 19d fixed both (Item 68).

**The join** (23:09 to 23:12). The lane rebased cleanly onto 18d's join (b63b483) and fast-forwarded `main`. Its
gate: 1,715 of 1,715, one known flake passing on its retry; pushed 23:12:39. Nothing the daemon runs changed, and
the install at 23:15:48 (Item 65's) carried the new `theseus-sim`. theseus-7ve.7 is closed.

**Divergences**, accepted at the review. The design's simulator drives the filter; this one drives a whole core and
plays the binding and the driver, so it checks what left the core rather than the filter's verdicts. It adds the
streamed edit and the release's approval to the invariants. Its integrity check is the build's (T1's sites at
write) until 20a. Counting filed gaps instead of failing on them was accepted, since keeping context files and tasks
out of the world would test less.

**Known gaps** (P3, post-v1): the core is never restarted mid-run, and no approval card is driven
(theseus-843s); the binding's tool lines are not replayed (theseus-0yz6). Both wait on the simplification review's
Tier 2. _(The simulator was removed with the labels, Item 76, and both closed as moot.)_

