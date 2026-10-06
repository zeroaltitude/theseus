# The Ship of Theseus, chapter 14: Part III, A4's Items 67 to 78 ([index](README.md))
### Item 67. The first cloud batch: L1's contract on a root VM, literal old layouts, and five telemetry gaps (theseus-celu.1 to .4; Claude cloud sessions fired 2026-10-02 22:45 from bfbe47b; l1-vm and store-literals reviewed and joined 23:29 at 54e4083, as 67b006c, a9b1aa7 and 54e4083; metrics reviewed 2026-10-03 00:51 to 01:05 and joined 01:00 at 5526591, as 39e4de0, d7ca3a7, e8c0f04, 54d99e3 and 5526591; installed 01:19 at c641ae4)

**Why.** At 20:44 the owner offered his Claude cloud sessions for Theseus work (theseus-celu). Work that needs only the
public repository and its tests can run there beside the spine: gate flakes, small code paths, metrics, and what a
different machine finds. A probe went first: a Firecracker VM with 4 cores and 15 GB, running as root, with no
compile cache. It built the workspace cold in 9 m 12 s, clippy passed, and 1,680 of 1,686 tests passed: L1's
contract clauses 2, 3, 8 and 9 failed there, and two timing tests. It can push a new branch but not delete one.

**How a cloud session's work joins.** Each session works on its own `cloud/` branch from `main` as it stood at the
fire, and commits its report as the branch's last commit. Here, its work commits are cherry-picked onto `main` and
re-signed, keeping each one's own trailer; the report commit is dropped; a planted revert or two is re-run; the full
gate runs on this machine; and the cloud branch is deleted from here. Four sessions fired at 22:45.

**l1-vm** (theseus-celu.2; 67b006c). The contract's failures on the VM were the VM's, except one.
- The kernel was booted with `ipv6.disable=1`, so an IPv6 connect fails with `EAFNOSUPPORT`, stronger than the
  `ENETUNREACH` clauses 2 and 3 required; and it has no `/proc/kcore` to mask (clause 8). Each clause now asks the
  host first and expects what the host makes possible. Four planted reverts each failed as they should.
- **Clause 9 still fails on a root VM, as it should, and says why.** Linux exempts real uid 0 in the initial user
  namespace from `RLIMIT_NPROC`, and L1's user namespaces map a root operator's job back to that uid, so without a
  job cgroup (`pids.max`) a root daemon's L1 job has no process limit: its fork loop forked 4,096 times against a
  limit of 16. As an ordinary user on the same VM, all 19 cases pass. Filed theseus-pv6i (P2; fixed in Item 77, which refuses such a job). The owner's daemon runs as
  his own user with a delegated cgroup, so he is not exposed; a root install is.

**store-literals** (theseus-celu.3; a9b1aa7 and 54e4083).
- **theseus-djfj, finished** (its first part rode with 19c's join, Item 63): every old-layout read test takes its old
  bytes as a literal. A NODE schema-4 literal was added for an L1 call's node, the session tests compare strings as
  well as values, and each literal's comment names the build that wrote it. A scratch test at each commit before a
  bump checked that its build writes the literal back byte for byte. Each `skip_serializing_if` removed on one field
  fails at least one old-layout test.
- **theseus-gf00**: a new store's own directory name, and any new directory above it, is synced with its first
  frame, as the WAL's directory was. A test counts the syncs: none before a frame, 3 after the first, 5 with two new
  directories above the store.

**The first join** (23:29). `store.rs` met 18d's ACTION 4 literals and kept both. The join's gate (23:28:31): 1,716
of 1,716, lifecycle OK, a plain turn 5 frames; pushed 23:28:42.

**metrics** (theseus-celu.4; five commits, one per issue, cherry-picked onto 81f3dee with no conflict).
- theseus-b85w: a failed turn's tokens and dollars reach `theseus.tokens` and `theseus.cost.usd`, with the outcome
  `failed`.
- theseus-iu3a: a failed tool call's span has ERROR status. An unknown, cancelled, or declined call stays unset: the
  first may have run, and the others are the operator's choice, not the call's failure.
- theseus-8u02: `theseus.provider.first_token_ms` is recorded for every provider call, with the call's provider and
  model, no longer once a turn with the turn's profile and outcome.
- theseus-63xf: `theseus.compile.withheld{theseus.withheld.reason}` counts the nodes and context files each compile
  left out, from a `label.withheld` event on the loop's trace.
- theseus-gagg: the compile filter's bench runs in an optimized build. Its figure of record, taken here (release-thin,
  a quiet machine, two runs): a 1,000-node compile takes 1,775 µs with no judge; judging for the owner, with nothing
  withheld, adds nothing measurable (−45 µs), within the 50 µs budget; withholding 250 nodes for a channel of two
  adds 194 to 216 µs, four times the budget (theseus-2lvc, P3, parked on the simplification review's Tier 2; moot since the filter went with the labels, Item 76).

  Planted reverts of b85w and 63xf were re-run here. The join's gate (01:00:34): 1,720 of 1,720, lifecycle OK (cold
  start p95 34.7 ms), a plain turn 5 frames; pushed 01:00:48.

**The install**: with 19d and gh7h at 01:19:02 (Item 69's install).

**Divergences.** The sessions cloned `main` at the fire (bfbe47b), not at the launch. The VM's suite fails a few
tests this machine passes, and the joins judged them here: clause 9 (root), the overlap test on fewer than seven
cores (the CPU pool sizes to the host, and the test's seven calls need seven permits: theseus-i1i4's cause), and the
output golden's one-digit duration (theseus-6a7o).

**Still out.** core-flakes (theseus-celu.1), with fixes for seven timing tests, ran its load passes for hours and
pushed its branch at 04:03 on October 3. It was reviewed and joined at 9a8f537 at 04:26, after this Item was
written; the next docs pass records it (Item 73).
`TurnSubmitResult.first_token_ms` is still on the wire, and telemetry no longer reads it.

### Item 68. The disclosure simulator's two findings, closed (theseus-7ve.8, with theseus-42ub and theseus-jpff; M4 step 19d; spine, in a worktree; 2026-10-02 23:16 to 2026-10-03 00:03; reviewed by 00:24; rebased onto 54e4083 as 81f3dee; joined 00:28 at 81f3dee; installed 01:19 at c641ae4)

**Why.** 19b's simulator found two places where 19a's labels let owner-only material reach a wider audience (Item 66):
an answer labeled without the context files its request carried (theseus-42ub), and a task's report carrying its
brief's first line without the brief's readers (theseus-jpff). Both were P2 disclosure paths on v1's features,
counted as known gaps until fixed. Neither reached the owner.

**What landed** (§3.9; the M4 design's §2.5).
- **An answer's readers include its context files** (theseus-42ub). `render_request` meets the readers of each
  context file the system block carries whole (the owner's unless its entry says public; a withheld or missing file
  carries nothing) into what the prefix and the whole request admitted. So the answer's label, the manifest's
  readers, `context.compiled`'s readers (19c's quiet loop), the reply's post readers, and a task's brief all take
  the file in.
- **A report carries its brief's readers** (theseus-jpff). The parent's report node is labeled by the meet of the
  task's last answer's readers and its brief's, and the report post's readers meet the brief's label in too, read
  through the task's session (`Store::first_node`, which reads a session's first records, never all of them). The
  meet, not a title that carries nothing: it is the design's rule, it changes a report only when its task's own
  request withheld the brief, and the owner keeps the title.
- **The simulator's `KNOWN_GAPS` is empty**, so a leak now fails its default run as `--strict` does. The gate's
  disclosure seeds were chosen again (seeds 7 and 10 at 40 steps, 34 at 3, about 1.3 s): the fixes moved what seeds 3
  to 6 did, and they had lost 19b's quiet-loop plant.
- No store schema changed; a plain turn writes 5 frames; nothing is on the start path.

**How it is proven.**
- **Tests, each against a planted revert.** An owner-only context file makes its answer the owner's in an owner-only
  channel (the loop quiet, a read at post time, both withheld once the channel grows). A report takes its brief's
  readers when the channel grows before the task runs, and fails against either half of the fix reverted. `theseus-sim
  disclosure --seed 7 --steps 447 --strict` fails at step 3 against 42ub's revert, and at step 446 against either half
  of jpff's. The gate's seeds fail against 42ub's revert, jpff's node revert, and 19b's three plants.
- **The gate**: 1,715 of 1,715, a plain turn 5 frames.
- **Live**: `--strict` on 40 seeds of 2,000 steps passed 40 of 40 in 388 s, with no gap counted: 93,924 compiles
  checked, 336,072 nodes withheld, 701 posts held (419 released), 3,727 quiet loops, 7,230 tasks, and 492,490
  invariant checks; 99 more posts were held, and 256 more loops quiet, than in 19b's run. On a copy of the owner's store
  with his note, his DM compiled for his person with nothing withheld; his store has no context file, task, or report
  node.
- **Benches, alone**: every lifecycle budget held (cold start p95 26.2 ms), and a plain turn wrote 5 frames.

**Old nodes.** Labels are written once. A report from before 19d whose task's request withheld its brief, or an
answer that carried an owner-only context file in a channel the owner alone viewed, keeps its wider label, and no
recompile changes that. `/new` moves a place past such a session. A report post is checked by the brief's label at
post time, so even a post an earlier build staged takes it.

**The join** (00:24 to 00:28). The review read the code: context files meet into the request's admitted readers; a
report's brief readers meet into its post and its parent node; the first-node read is right; and the only path that
creates a task writes a brief authored by the parent session, so the brief is found. The rebase onto 54e4083 was
clean (81f3dee), and `main` fast-forwarded. The join's gate (00:27:08): 1,716 of 1,716, a plain turn 5 frames, the
benches within budget, cargo-deny clean. The install waited for gh7's re-scoped step, for one restart.

**The install**: with the metrics and gh7h at 01:19:02 (Item 69's install). theseus-7ve.8, theseus-42ub and
theseus-jpff are closed.

**Divergences.** None from the brief's fixes. The gate's disclosure seeds changed, which the brief did not foresee: a
change to the core's labels moves every seed, as a change to the world does. Seed 2 at 10 steps, 42ub's first
reproduction, no longer reaches it.

**Known gaps** (P2), found by following where relayed text goes: a fired wake's note is labeled with its session's
readers, not its setter's (theseus-nbln); `/tasks` and `/wakes` post titles and notes into a guild channel without
their readers (theseus-ntx5). Neither reaches the owner, and both are parked on the simplification review's Tier 2. _(Both closed as moot by the place rule, Item 76.)_

### Item 69. Credentials: the stand-ins dropped, and the harness-only keys said aloud (theseus-gh7, with theseus-3m11 and theseus-7y9y; spine; gh7 2026-10-02 23:17 to 23:39, blocked, reviewed 23:42 to 23:49; re-scoped by the owner at 00:00; gh7s 2026-10-03 00:03 to 01:02, e473963 and 868b683; split at the join, its harness-only half joined 01:14 as gh7h at c641ae4; installed 01:19 at c641ae4)

**Why.** P6 promised credentials as stand-ins (the owner, 2026-10-01 15:13: in place before the AWS hands' first account
write). A job granted a secret would see a stand-in, never the value: the L1 egress proxy would terminate TLS for the
granted hosts, through a per-job CA the job trusts, and swap the value in only on those connections.

**gh7, blocked** (23:17 to 23:39). The step wrote its design before any code, as its brief asked: a per-job CA made by
the wrapper at the job's start, its key in the wrapper's memory alone; the job trusting it through the view's
certificate bundle; which connections are terminated (a granted secret's hosts only); the swap; the stand-in's shape;
what the upstream connection verifies; the rows; and SigV4's re-signing as a second step. It wrote two files, the
certificates and the stand-in table, never compiled. While it wrote the next, the reader of a terminated connection's
requests that puts the value in place of its stand-in, a safety classifier stopped the session's output. That file
was the step's core, and the TLS relay after it the same kind of code, so the run stopped there: nothing built,
nothing committed. The review (23:42 to 23:49) did not retry it or work around it, and asked the owner to choose.

**Re-scoped** (the owner, 00:00, under the default-trust principle he set at 23:57, §2). TLS interception at the proxy is
dropped for v1: it is heavyweight for a default-trusted harness, and its shape is a credential-intercepting proxy's.
The lightweight strategy is what the code mostly had already: the AWS keys and the providers' keys are the harness's
alone, read only by Theseus's own tools; a job gets a secret only by the operator's `[broker]` grant; and the egress
list (Item 62) bounds where a held value can go. The step was re-briefed to make that explicit, close theseus-3m11,
and record theseus-7y9y, with no CA, no TLS termination, no request parsing, and no stand-ins. gh7's two files were
deleted, never built.

**What gh7s built** (00:03 to 01:02).
- **The harness-only keys, said aloud.** `broker::harness_only` gives `HarnessOnly`: the secrets `[broker]` names,
  which a job may be handed; each bound account's two key names; the providers' keys; and any of those `[broker]`
  names after all, which is said aloud rather than refused. `theseusd check` prints its line after `ok:`, and `theseus
  health` after the broker line (`HealthResult.harness_only`). It is computed from the config, so nothing runs before
  serving. A test driven by the template holds it: no AWS or provider key the un-commented template uses can be
  handed out, and a grant that names one makes it `exposed`. The template's broker section says it plainly.
- **theseus-3m11**: the daemon told an L1 job's wrapper each value the job asked for before the job got it, through a
  second private socket per job, and the wrapper withheld it from the raw output file.
- **theseus-7y9y**, recorded: spawn grants stayed withheld in L1 (superseded by Item 71).
- An L1 result's head said "no secret at its start", since a job could ask for one later.

**How it was proven.** Eight planted reverts, each failing its tests. The gate: 1,722 tests, a plain turn 5 frames, no
schema moved. Two load failures on the way were fixed or filed: 18d's restart test waited for its socket's inode to
change, and on this machine's ext4 `/tmp` a socket bound where a removed one was keeps its inode number (200 of 200
rebinds in a probe), so it now waits for a listener that answers; and a store test's two reads that are not one
snapshot (theseus-tphr, P2, on the retry list). Live, on a scratch daemon over a copy of the owner's store with his note:
GLM's L1 job got the 93-byte token and echoed it, its raw output file held `got [redacted:github_token] here`, and
the token was in none of the run's 8 files, during the run, after it, and after the stop.

**The split at the join** (to 01:14). The cut-list sent to the owner at 00:29 offered granting at launch with 18d's
socket deleted (Tier 3), which would make 3m11's second socket unneeded. So, as told to the owner at 01:05, the join
split the lane by hand. The harness-only half landed alone as gh7h (c641ae4): the line and its test, the template's
words, 7y9y recorded in the core's guide, the egress proxy's docs no longer promising a stand-in outcome, the
header's words, the restart test's wait, and tphr on the retry list. The 3m11 half stayed parked. It was checked first
that without 3m11 the model, the store, and the clients never see such a value: the daemon's scrubber removes every
value the secret board holds from a job's result, and the raw output file is 0600 in the private spool and deleted
once the result is written. The join's gate (01:12:49): 1,721 of 1,721, a plain turn 5 frames.

**The install** (01:19:02, at c641ae4: 19d, the first cloud batch with its metrics, and gh7h). The store and the
binaries backed up (78 MB); `theseusd check` ok; the restart ok, the secrets ready 1,035 ms after the start and the
vault's confirmation at 1,032 ms; L1 working (its start 8.4 ms); the unit active, with no restarts. Health's new line:
`broker: a job may be handed no secret; harness-only: the AWS keys (aws_access_key_id, aws_secret_access_key) and the
providers' keys (anthropic_api_key, zai_api_key)`. The cockpit served, and the owner's note needed no change.
theseus-gh7 and theseus-7y9y are closed.

**After.** The owner took Tier 3 at 01:42, and the grants step (Item 71) deleted 18d's socket, with it 3m11's reason and
the restart test. theseus-3m11 closed as moot, and the parked branch was retired. An L1 result's head now names what
its grants gave it, or "no secret".

**Divergences.** The step built no part of its brief's original design. The harness-only line names only the AWS
keys and the providers' keys; the harness's other secrets (the Discord bot's token, the search key, Jev's) are never
handed to a job either, which "a job may be handed …" already implies.

**Known gaps.** A job's printed value is withheld from its raw output file verbatim only: an encoded form reaches the
file, though the node is scrubbed of it (theseus-ej7i, P3; hygiene). The Observatory does not show the harness-only
line yet (theseus-tzbl, P3). theseus-tphr (P2, `gate-flake`).

### Item 70. The second cloud batch: two cleanups, the cockpit's third round, and the build in health (theseus-celu.5 to .10; Claude cloud sessions fired 2026-10-02 23:40 from 54e4083; reviewed 2026-10-03 01:34 to 01:58; joined at b642d96 (01:38), 883df95 (01:41) and d337276 (01:57); installed 03:02 at 8067161, with the grants step; three branches and one change parked)

**Why.** At 23:33 the owner asked for the next eight or so tasks to go to cloud sessions too. Six fired at 23:40, each
chosen to stay off the files the running spine steps touched, and joined as Item 67's were: cherry-picked onto
`main`, re-signed, the report commit dropped, a planted revert re-run here where a change has behaviour, the full
gate on this machine, and the cloud branch deleted from here. The owner's picks on the simplification cut-list (01:42)
decided which could join that night: three did, and three wait on tiers he has yet to decide.

**cleanups** (theseus-celu.8; joined at b642d96 as 3780def and b642d96).
- theseus-i25g: `graph::Label`, the vocabulary Item 32 kept for 19a's first labels, is removed with its registry row
  and its reserved kind. 19a made a label a field of the node, so no graph-level label is coming. No behaviour
  changed, and the compiler holds the removal.
- theseus-ltaq: the root `AGENTS.md` went from 22,219 bytes to 16,949, under its own 20 KB rule. Its depth moved
  verbatim into the directories' guides: each crate's key modules and readers to the crate's own `AGENTS.md` (four
  crates gained a short one), the reserved crates to `docs/design/README.md`, "Where the big things live" to
  theseus-core's guide, and "This machine" to `scripts/AGENTS.md`. The root keeps a pointer to each, and the rule
  about the operator's daemon.
- The join's gate (01:38:04): 1,721 of 1,721, a plain turn 5 frames, lifecycle OK.

**cockpit-p3** (theseus-celu.10; cockpit only; joined at 883df95 as 26f4c10, fa40334, 4239804 and 883df95).
- theseus-j4qe: the time machine also folds the boundaries board and the session deck, which cut their rows at the
  needle's moment and hide what only the present has (the cgroup gauges, the composer).
- theseus-93ey: the Ship reads an old call's L1 shield from its node's gate decision, with the job rows as the
  fallback. Its planted revert was re-run here.
- theseus-uovv: the Ship's console, key, and porthole fit below 1280 px.
- theseus-7mcu: the reach cap is shown, not lifted: a vessel's card says its currents cover its newest 160 nodes.
- The join's gate (01:41:39): 1,721 of 1,721, lifecycle OK.

**cockpit-core** (theseus-celu.9; joined at d337276 as ffc88fb and d337276).
- theseus-9o5n: `theseusd` names its build, taken at compile time (an explicit `THESEUS_COMMIT`, or git's `HEAD`), in
  health's `build` and `server.started`'s `data.build`, so the cockpit's ship's log marks an install apart from a
  restart. The cockpit gained its first test (`npm test`, on node's own runner, no dependency), which the gate's
  cockpit phase runs.
- theseus-kpz1: `sandbox.usage` lists running L1 jobs with their commands, from a map the daemon keeps in memory from
  the job's start, before its row is written. No frame, write, or method was added.
- Its board conflicted with j4qe's in `Boundaries.tsx`, merged by hand: in the past the board shows the fold and no
  live jobs. Three planted reverts were re-run here. The join's gate (01:57:27): 1,724 of 1,724, lifecycle OK, a
  plain turn 5 frames.

**Parked: frames per live turn** (theseus-wz4y). Its count rides the turn's trace with no write of its own, and its
first join gate failed in the turn bench, 2 of 2: the trace counted 11 frames where the WAL held 12. The spool's
drain had accepted the job's completion, then the turn's own look accepted it again as a duplicate, and the count took
the drain's frame only when the look wrote none. This machine hits that order often; the VM had not. The fix waits on
the cut-list's Tier 7.1, which rewrites that path, and `main` was pushed at d337276 without it.

**Parked on the owner's pending tiers**, their branches kept: root-l1 (theseus-celu.5: a root daemon's L1 job with no job
cgroup refused, theseus-pv6i; Tier 4); ksim-questions (theseus-celu.6: kernel-sim drives the held post's question,
theseus-tbv2; Tier 2; its credential-request half went with Item 71); discord-p3 (theseus-celu.7: a cheaper read at
post time and quiet loops' tool lines, theseus-zupl and theseus-033g; Tier 2). Not launched: the simulator's gaps
(theseus-843s, -0yz6; Tier 2). Dropped: the printed-secret scrub (theseus-3m11), moot after Item 71. _(Since then: root-l1's intent was built by the sandbox trims, Item 77, and its branch deleted; ksim-questions and discord-p3 went moot with the place rule, Item 76, their branches deleted; 843s and 0yz6 closed as moot.)_

**The install** (03:02:17, at 8067161, with the grants step: Item 71's install).

**Divergences.** None in what joined. The grants and tier0 lanes had branched from a7c15cf, the join head that
carried wz4y, and were re-pointed to d337276 before their first commits.

**Known gaps.** The flaky tests left by cockpit3 (theseus-f6f5, -so1a, -mll1) waited for the first batch's
core-flakes session, joined at 9a8f537 after this was written, and a stop test of the index tender seen flaking under load joins them
(theseus-ux8g). _(All four fixed in flakes-2, Item 73.)_

### Item 71. L1 credentials granted at launch, and 18d's run-time socket deleted (theseus-w5op; the simplification cut-list's Tier 3, C1; spine; 2026-10-03 01:47 to 02:39; one commit on d337276, 8067161; reviewed 02:49 to 02:55; joined 02:57 at 8067161; installed 03:02 at 8067161)

**Why.** The owner's cut-list pick at 01:42 ("a worthy simplification"), under the default-trust principle (§2). An L0 job
always took its program's broker grant at its launch. 18d (Item 65) had given an L1 job a socket to ask for a secret
while it ran instead, and that socket cost a listener per job, a role of the binary, binds, a re-serve after every
restart, an action kind, two ledger kinds, and a second socket to keep a printed value out of the raw output
(theseus-3m11, Item 69). This step makes L1 take its grants as L0 does, and deletes the rest. It supersedes
theseus-7y9y, which had kept spawn grants out of L1.

**What landed** (§3.19, §7; P5b, P6).
- **The gate.** L1's decision runs through the same broker step as L0's (`ToolRuntime::brokered`): `Broker::at_gate`
  works out the grant by `argv[0]`, and the call is raised to the secret's posture, so it runs at the stricter of its
  own (L1's notify, or approve where the operator's word, egress beyond the list, or the hold on outside text asks)
  and the secret's. Decision 15 thus holds at launch, for both classes, and any approval comes before the launch.
- **The spawn.** `Broker::for_job` in both classes: it waits for an unsettled secret as L0 does, withholds one whose
  posture outranks the call's, and pins a granted git's or gh's hooks off. The value goes into the wrapper's
  environment and over the init's pipe into the job's, never onto a command line; the wrapper withholds it from the
  job's output by name, and the daemon's scrubber scrubs every result, as before.
- **The words.** An L1 result's head says what its grants gave it (`[ran in L1, the sandbox: no network, given
  GITHUB_TOKEN; …]`) or `no secret`, from names the wrapper already held; `sandbox.started`'s narrative line says the
  same. L1 now gets L0's broker notes too, such as why a program run by `sh` got no grant. `proc.run`'s description
  lost 18d's clause, 39 estimated tokens off every request that carries the tools.
- **Deleted**, about 2,460 lines net across ten crates and the web UI (+1,066 −3,618 in all): the socket and its
  listener per L1 job, the re-serve after serving, the `theseus-cred` role, the `/run/theseus` binds, the request
  action's confirm and push hooks, health's `cred_requests`, the `secret.requested` notification, two ledger kinds,
  the renderers and the web UI's card, the simulator's `l1-cred` bench class, and L1's special case that withheld
  every grant.
- **Kept**: `HarnessOnly` and its line (Item 69), unchanged, and `Action.parent`, so ACTION 4 and OUTBOX 3 stay and
  no schema moved. A stored request reads whole, and 18d's two ledger kinds read as unknown kinds, byte for byte.
- **Records**: the template's broker section says a job gets only what a program's grant names, at L0 and in L1
  alike; the roadmap's row 34 (the AWS grant in L1) is re-pointed to a grant at launch.

**How it is proven.**
- **Six planted reverts**, each failing its tests: L1's gate taking the broker's posture (six rigs of call and
  secret postures), the grant at launch, the wrapper naming the job's grants, the head's words, 18d's kinds kept out
  of the registry with their stored rows still read, and the moved template test.
- The daemon's test runs a real L1 job given its grant: the value it prints comes back `[redacted:github_token]`,
  and neither the state directory nor the daemon's log holds it.
- **The gate**: 1,715 of 1,715 (13 of 18d's tests out, 4 in), a plain turn 5 frames. The core output golden moved in
  numbers only. **The lifecycle bench, alone**: cold start p95 32.7 ms, a binary swap's 99.6 ms; the start path only
  lost work.
- **Live**, on a scratch daemon over a copy of the owner's store with his note and a scratch-only grant: GLM's L1 job got
  `github_token` at launch (its length and sha256 prefix matched the vault's value, and what it printed came back
  `[redacted:github_token]`); its environment held the grant and nothing of the AWS or providers' keys. At approve,
  the call waited before its launch: approved, it ran with the grant; declined, it never ran. The value was in none
  of the run's 8 files. A scratch grant that named an AWS key made the check line say so aloud. His store held no
  `cred.request` action and no 18d row: his deployment had never asked.

**The join** (02:49 to 02:57). The review accepted the brief's one correction: nothing refuses a harness-only key at
the gate, since the operator's grant is the only path and the line says it aloud. `main` fast-forwarded. The join's
gate (02:53:43): 1,715 of 1,715, a plain turn 5 frames; pushed 02:57.

**The install** (03:02:17, at 8067161, with the second cloud batch's joins). `theseusd check` ok; the restart ok, the
secrets ready 1,522 ms after the start; L1 working (3.9 ms); the unit active, with no restarts. Health still read
`broker: a job may be handed no secret; harness-only: …`: the owner's note has no `[broker]`, so no job gets any secret,
as before. theseus-w5op is closed, and theseus-3m11 as moot; gh7s's parked branch was retired.

**Divergences.** L0 has no syntax for secrets a call names, so none was added. `Broker::may_hand_out`, unchanged, now
over-approximates: a `[broker.secrets.<name>]` posture entry alone hands nothing to any job, yet the line counts it.
An empty `<spool>/broker/` may remain where 18d ran. The branch first sat on a7c15cf, the parked wz4y join head (Item
70), and was rebased onto d337276.

**What it costs, said plainly.** A job holds its secret for its whole run, as at L0. Nothing is approved mid-run. A
script can't ask for a secret it didn't get at its start.

**Known gaps.** theseus-ej7i's other half stays (P3): a value is withheld from the raw output verbatim only, at L0 and
in L1 alike. The helper's git form (theseus-9oyy) is moot.

### Item 72. The `tier0` lane: the cut-list's housekeeping, and an install that builds only what ships (theseus-o8nk; the simplification cut-list's Tier 0 and 5.2; 2026-10-03 01:47 to 03:15; six commits on d337276; reviewed 03:19; rebased onto 8067161 as 12e3c6b, 6ba6dbd, da50b71, ab67751, 12f4725 and aa2e199; joined 03:27 at aa2e199; installed 03:32 at aa2e199)

**Why.** The simplification review (theseus-vm3n) found weight that bought nothing on the owner's deployment: a parked
voice engine compiled into every gate, lane, and install; a spec PDF committed 76 times; a CI job red in 190 of its
last 200 runs; dead and one-off code; a gate step that hid its own errors; and an install that compiled crates no
shipped binary links. The owner took the whole of Tier 0 at 01:42 ("Tier 0 looks awesome") and Tier 5.2 ("sounds
generally good"). He kept `scripts/repro.sh`, the reproducible-build check the cut-list had listed: simplicity must
not mean no attention to security.

**What landed**, one commit an item.
- **Voice is parked outside the workspace** until its row (77, 44b). The root `Cargo.toml` excludes
  `crates/theseus-voice`; its tree stays, and its guide says how to build and test it by hand (`--manifest-path`;
  its manifest writes out the fields an excluded crate cannot inherit). `Cargo.lock` dropped 192 packages with no
  version changed, and `deny.toml` lost the seven MPL-2.0 exceptions and six advisory ignores only voice needed (the
  licence call, theseus-yl5w, moves to row 77). The Discord gateway's TLS roots stay as they were: voice's stack had
  turned on twilight-gateway's `rustls-native-roots` through feature unification, and theseus-discord now names it.
- **The spec's PDF is git-ignored**, rendered and sent as before. History is untouched: its 76 versions keep their
  99 MB, most of the repository; purging them is a force-push, the owner's call.
- **CI runs what a stock runner can pass**: fmt, clippy as the gate runs it, the Observatory's dist check, and
  cargo-deny's licences, bans, and sources. The suite, which needs this machine, stays in the gate.
- **Dead and one-off code is gone**: `Secrets::resolve_all` (no caller) and the contract test's toolchain survey
  (17b's one-off).
- **The gate's npm steps print their errors**: a failing lint, test, or build shows its name and the last 40 lines of
  its output above the phase table.
- **`scripts/build.sh` builds the five binaries an install ships** (`theseusd`, `theseus`, `theseus-tui`,
  `theseus-sim`, and `theseus-index`), and the gate's bench build the same five. `--shipped` went, since nothing used
  it.

**How it is proven.**
- **Feature sets, by cargo's unit graph**, walked from the five binaries. Against the install's build until then,
  voice's exit changed 22 units in 18 crates. Four are the TLS stack: pinned, identical after. The other 18, in 14
  crates, are features voice's stack asked for that nothing shipped uses (APIs nothing calls, getrandom's wasm and
  RDRAND backends, a glob matcher's Unicode tables); the gate's bench build already built three of the binaries
  without them. Building the five alone then changed no unit: 571 units, alone or with the whole workspace.
- **Cold builds** (no compile cache, 4 jobs, nice 19, release-thin): the install's build until then took 1,395
  CPU-seconds (5 min 55 s); the five take 1,139 (about 4 min 50 s), 18 % less: 10 points from voice's exit, and the
  rest from building the five.
- **The same bytes**: the five binaries built alone equal the five built with the whole workspace from one tree
  (`cmp`), and `theseus` and `theseus-tui` equal the install's build from before the lane.
- The gate's new npm step, run on a cockpit copy with a planted type error, printed the error with its file and line.
  Voice still builds by hand (327 crates). Six gates in the lane, all green (1,696, then 1,695 tests), and one on the
  lane merged into 8067161 before the join (1,686, after the grants step's deletions).

**The join** (03:19 to 03:27). The review accepted the lane's judgment on pinning (below). The rebase onto 8067161 was
clean, and `main` fast-forwarded. The join's gate (03:24:11): 1,686 of 1,686, theseus-tphr's known flake passing on
its retry, a plain turn 5 frames. The chain tree's untracked copy of the spec's PDF was set aside before the
fast-forward and restored after it. The push ran the trimmed CI for the first time.

**The install** (03:32:29, at aa2e199). The first through the new `build.sh`, which built the five in 2 m 18 s.
Discord ready, so the pinned TLS roots connect; the secrets ready 1,088 ms after the start; L1 working; the cockpit
served; the unit active, with no restarts. theseus-o8nk is closed.

**Divergences.** The brief asked to pin any feature the shipped binaries lost. Only the TLS roots are pinned: the
other 18 units do nothing on Linux, and pinning them would have put wasm-only getrandom features into theseusd's
manifest. The brief's `cargo tree -e features -i … -p theseusd` cannot see the leak (with `-p`, cargo resolves
features for theseusd alone), so the proof is the unit graph. The cut-list's "about 40 % off a cold build" for 5.2
predates voice's exit and theseus-index's install; with voice gone, the five skip 11 of the workspace's 582 units.
The lane started from a7c15cf, the parked wz4y join head (Item 70), and was re-pointed to d337276 before its first
commit.

**Known gaps** (P3, post-v1): nothing runs the shipped-features check automatically, so a crate waiting for its row
could widen a shared dependency's features in the tested build and not in the install (theseus-dr2x); theseus-index
has no `--version`, unlike the other four (theseus-t7ra). The PDF's old versions stay in history.

### Item 73. Two cloud sessions on the gates' load-sensitive tests: ten made deterministic or rebounded, a product fault in `op inject`, and the gate's L1 bench (theseus-celu.1, the first batch's core-flakes, and theseus-celu.11, flakes-2; Claude cloud sessions; core-flakes fired 2026-10-02 22:45 from bfbe47b, reviewed 2026-10-03 04:15 to 04:30, joined 04:26 at 9a8f537 as 17289b0 to f3854ac with one join commit, 9a8f537; flakes-2 fired 04:40 from d85660e, reviewed from 07:50, joined 07:59 at a59b7c1 as c99e47f, e16626f, 0d2c2fb and a59b7c1; installed 13:47 at 57a3759)

**A note on Items 73 to 78.** They landed on 2026-10-03 and reached the operator's daemon in one install, at 13:47
at 57a3759. Their joins were made as before: a cloud branch's work commits cherry-picked onto `main` and re-signed,
and a lane's commits rebased onto `main` and re-signed, or fast-forwarded where `main` had not moved. From 13:50 on, a
join is a plain merge (the owner, 13:18: "Is branch merge not enough?"): a fast-forward, or a signed merge commit, never a
rebase, a cherry-pick, or a re-signing, so a branch shows as merged and its commits keep their ids.

**Why.** Timing tests that pass on a quiet machine and fail beside a busy one fail joins for noise. Item 67 left the
first cloud batch's core-flakes session out: it fixed seven of theseus-core's, and joined after that Item was written.
The flakes-2 session took the three that the cockpit3 lane's gates had hit (theseus-f6f5, -so1a, -mll1; Item 64), and
the index tender's stop test (theseus-ux8g; Item 70), and it investigated theseus-nuna: a held-post test that had once
shown the owner's text in a shared channel on the cloud VM. Each session reproduced a test under load before fixing it,
as Theseus's rule is (priority, not count: the test at `nice -n 19` beside four busy loops at nice 0; Item 31), then
ran it 20 times under the same load, and planted a revert of what it guards.

**core-flakes** (theseus-celu.1; eight commits, cherry-picked onto aa2e199 and re-signed, and one join commit).
- **theseus-i1i4** (17289b0, with bea133a and f3854ac). The seven-calls test failed all three tries in the VM's full
  suite, and not only from load: the in-process toollets share a CPU pool with one permit per core, so on 4 cores seven
  calls can't run at once. `rpc::Parts` gains `cpu_cores`, a test seam (`None` in the daemon: a permit per core), which
  the test sets to 7. Each stand-in call waits until all seven have begun (a rendezvous that gives up after 4 s), so no
  call ends before the batch began, and the overlap is asserted as the peak number in flight, in the runs and in the
  spans. The wall time keeps its floor, and its ceiling is the sum of the delays. `tests_m3.rs`'s line ceiling rose
  from 8,000 to 8,050, and theseus-discord's four `Parts` literals name the new field; at the join, so did
  theseus-sim's disclosure rig's (9a8f537), which was new since the session's base.
- **theseus-ioq7** (d9b00c1). The two-fetches test drops its 780 ms ceiling; its order check (each request reaches the
  server before either is answered) proves the overlap.
- **theseus-535n** (ed9e9f5). The approval-in-the-middle test asserts the transcript's order, not which of two
  concurrent calls ends first.
- **theseus-56r7** (53f8065). The web UI's two refusal-span tests run on tokio's paused clock. No product change.
- **theseus-vy7y** (30fee4b). The hung-receiver bound is 5 s, half the export timeout, not 2 s.
- **theseus-lc4n** (32a17e4). Health from the config's copy is bounded by half the vault's wait (750 ms), not 50 ms.
  The 50 ms budget is the lifecycle bench's, on a settled machine.

**flakes-2** (theseus-celu.11; 1 h 28 min; four commits, cherry-picked onto d85660e, the session's own base, with no
conflict, and re-signed).
- **theseus-f6f5, a product fault** (c99e47f). The reaping test's failing line was the product's own, and no exit
  status was lost. `OpReader::inject` wrote the template to `op`'s stdin with a `?` before it waited for `op`. So an `op`
  that refuses at once, without reading its input (the fake's first injection; a real sealed vault, or a bad token),
  surfaced as `running op inject: Broken pipe (os error 32)`, and `op`'s status and its own words were never read. The
  write now takes a broken pipe as `op` having closed its input, and goes on to `op`'s status and stderr. A new unit
  test makes the race certain: 2,000 references put the template past a pipe's 64 KiB, so the write always meets the
  closed pipe. The reaping test's real guard grows stronger with it: `op`'s status is always read now, so a reaper
  that took `op`'s child shows as `No child processes`.
- **theseus-so1a** (e16626f). The store's open-waits test proves its order instead of timing it. A test-only count of
  held tries (`HELD_TRIES`, `cfg(test)`) lets the holder close only once the open has found the store held, and the
  open must return after the close, with a nonzero `lock_wait_us`. The store's own measure was right: the test's sleep
  had begun before the open.
- **theseus-mll1** (0d2c2fb). The suite measures an L1 start and bounds nothing. `scripts/gate.sh` gains a `jobs`
  phase after the lifecycle bench: `theseus-sim bench jobs --class l1 --runs 20 --check`, a p95 under 25 ms (the M4
  design's target), with one rerun after a flush and a settle on a miss. A lane's gate skips it with the lifecycle
  bench, so the join's gate runs it (§9). The suite's 250 ms had been a load allowance that a tenfold regression on an
  idle machine would still pass.
- **theseus-ux8g** (a59b7c1). The tender's stop test waits until the tender reads `T`, stopped, before it stops the
  daemon. In every failing run the SIGSTOP had not yet taken hold, and the stop's SIGTERM ended a running tender.
  Nothing in the product escalates: a stop sends the tender SIGTERM once and never waits.
- **theseus-nuna, investigated only.** No failure in 41 runs on the VM: 20 loaded runs alone, 20 full suites, and one
  gate. Instrumented runs saw the same event order every time, and no post in the shared channel held the owner's text.
  One unseen gap was named (the Discord renderer does nothing with a compile for a loop it has no view of, and that
  loop would stream), with a fail-closed fix proposed. Nothing was joined for it, and it closed as moot when the place
  rule removed quiet loops (Item 76).

**How it is proven.**
- **Before the fixes, under load:** f6f5 failed 4 of 40, two of them on `running op inject` (the other two were
  theseus-46ya's fault, below), and a failed run's log named the broken pipe; ux8g failed 3 of 20, each with the tender
  still running. so1a and mll1 did not reproduce: 60 of 60 and 40 of 40.
- **After:** each test 20 of 20 under load, in both sessions.
- **Planted reverts.** core-flakes: serial dispatch fails i1i4 ("1 calls overlapped, not 7") and ioq7's order check;
  the refusals' dedupe off fails both 56r7 tests (3 rows for 1, and 100 for 3); a 6 s sleep in the export queue fails
  vy7y; health handled as an acting method fails lc4n. flakes-2: f6f5's `?` back on the write fails the new unit test
  (`left: "running op inject: Broken pipe (os error 32)"`, `right: "the vault is sealed"`); `op` spawned without its
  `Kind::Owned` registration fails the reaping test on `No child processes`; so1a's held branch off fails ("the open
  never found the store held"); a stop that polls until its tender is gone fails ux8g 3 of 3; a 250 ms sleep before the
  sandbox's spawn makes the jobs bench report a p95 of 264.04 ms, `MISSED`, exit 1. The reviews re-ran the first two
  of core-flakes' and the first three of flakes-2's, here, each failing as it should and passing restored.
- **The joins' gates.** core-flakes (04:25:24, 126 s): 1,686 of 1,686, with no retry at all, a plain turn 5 frames;
  pushed 04:25:56. flakes-2 (07:58:53, 122 s): 1,687 of 1,687, theseus-tphr's listed flake passing on its retry,
  lifecycle ok (cold start p95 27.1 ms), **the new jobs phase p50 4.90 ms and p95 5.60 ms**, a plain turn 5 frames;
  pushed 07:59:17. The jobs bench by hand before it: p95 5.77 ms.

**The joins.** core-flakes: one conflict, in `.config/nextest.toml`, where `main` had added theseus-tphr's retry entry
since the session's base. tphr's entry stayed, and the retry entries of i1i4 and 56r7 went, as the commits intend. One
join commit gave the disclosure simulator's rig the new field. flakes-2 joined clean. Each session's report commit was
dropped.

**The install** (13:47, at 57a3759, with Items 74 to 78). f6f5's fix to the secrets reader is the one change here that
a running daemon behaves differently by (i1i4's seam is `None` there); the install's `theseusd check` resolved the
operator's 8 secrets in 2,038 ms.

**Divergences.** i1i4's fix is a seam in product code (`Parts.cpu_cores`, `None` in the daemon). mll1 moved a bound
out of the suite into the gate: a lane's gate no longer bounds an L1 start at all, and a join's gate bounds it ten
times tighter (25 ms, not 250). Each session's commits were gated at their heads only, in the cloud and at the join, as
the earlier cloud joins were.

**Known gaps.** 535n has no planted revert its new assertions catch: serial dispatch passes it by design, and no
one-line plant broke the transcript's order. vy7y is still a wall-clock bound (5 s, against a 2.2 s loaded turn). Found
on the way, and recorded: theseus-46ya's likely cause, confirmed in the code (`Spool::read_completion` checks that a
completion exists and then reads it with a `?`, while the daemon's spool drain reads and removes the same files, so a
completion the drain takes in between faults the whole turn with a bare `ENOENT`; P2, on v1's path), and a new gate
flake, theseus-amr2 (P2: the push's position-rule test failed once in 20 suites on the VM). Both went to the third
cloud batch. Two failures in every VM suite are known: theseus-6a7o (the output golden's one-digit duration, on the
flaky list) and clause 9 as root (theseus-pv6i, closed by Item 77).

### Item 74. The integrity lane: a listed program's output, and a job's session, hold the latch (theseus-b5cl; the simplification cut-list's Tier 1.1; 2026-10-03 11:32 to 12:29; two commits on a59b7c1, dc027ae and 53d32e0; reviewed 12:50 to 12:56; joined 13:04 at 53d32e0, a fast-forward, pushed 13:18; installed 13:47 at 57a3759)

**Why.** The owner's decision on the cut-list's integrity tier, at 11:24: "Integrity: perfect! Yes, Jev should cover it. If
it's failing, we boost its context for good classification. And I like the latch being per session." The goal is
unchanged: a stranger's text must not steer Theseus into acting (§3.9). The plan to feed T1's latch from integrity
labels with an `external` origin (20a), the fomites (20b: a hash on every `fs.read` and write), and the Advisory's
quarantine levels (theseus-3vu) are dropped. T1's latch stays as built, per session, fed by DD5's own `external`
marker, and trust clears it; it gets its two cheap missing pieces. Laundering through files is Jev's `security.v1`
(row 39), with its context boosted if it misses. Under default trust (§2) both pieces are light guards, not
boundaries: a job can strip its own environment.

**What landed** (§3.9).
- **`[policy] external_programs`**, `["gh"]` by default in the loader and the template, since `gh issue view` prints a
  stranger's text.
  - A `proc.run` whose `argv[0]` file name is listed, or that runs a shell or a launcher (`sh`, `bash`, `env`, `xargs`,
    `timeout`, `python`, `make`, `npx`, and the broker's other launchers) whose command names a listed program in any
    word, gets DD5's `external` marker on its result, at L0 and in L1 (`external::Listed`). The words are split at
    spaces and the shell's punctuation, so `cd x && gh issue view 1 | head` and `$(command -v gh)` count. A program
    that is neither listed nor a launcher has its arguments left unread: `grep gh notes.txt` is not marked.
  - Every path that writes a job's result reads it from the call's input: a result within a turn, a late result, a
    resumed call after a restart, and a cancel's sweep. Egress's marker comes first, for a job that connected out of L1.
  - The session holds the latch `via: program`, and the hold names the command as the narrative does (`gh issue`),
    never the rest of the argv. The result's first line says why: `[it runs gh, which [policy] external_programs
    lists: what it printed may hold outside text]`.
- **A job carries its session.**
  - Every job, at L0 and in L1, gets `THESEUS_SESSION`, its session's id (`theseus_protocol::JOB_SESSION_ENV`), set
    after the call's own variables; no call can set a `THESEUS*` name. The CLI sends it as `opened_from`, a new
    optional field of `session.open` and `turn.submit`, absent from the bytes when unset, so every wire fixture keeps
    its bytes.
  - A session that a holding session's job opens holds its text from the frame that writes it (`via: job`, the holder
    in `from_session`, no node), and `session.opened` names `opened_from`.
  - A turn such a job sends to a named session gives that session the hold first, in a frame of its own, under the
    record's lock, before the turn writes its input. A plain turn is still 5 frames.
  - An `opened_from` that names a clean session, or none, opens a clean one, never a refusal, since a job could as well
    strip the variable. Trust clears each session alone: the holder keeps its hold.
  - This is theseus-d64, built by the job's own environment rather than J1's process trace.
- **Records.** The roadmap's rows 24 and 25 (20a, 20b) are replaced and dropped, row 31 marks AWS text with DD5's
  marker, row 72 (the MCP server) waits on `opened_from`, and row 39 (`security.v1`) is now the integrity path for
  text laundered through files. The root `AGENTS.md` and two crate guides say both pieces are light guards.
  theseus-3vu's unbuilt parts (its reverse columns among them, whose readers were the Advisory and the fomites) and
  theseus-d64 are closed.

**How it is proven.**
- The latch's existing tests pass unchanged.
- Ten new tests: the matching (direct, by path, through `sh`, `env`, `xargs`, `bash -c $(…)`, `python3 -c`, and
  `timeout`; not `git log`, `grep gh …`, or `ghost`), the hold's and the label's words, the default, through the core a
  listed run, a shell's run naming `gh` and an unlisted one, the job's variable, sessions opened and turns sent from a
  holding session's job, trust on the child alone, and the real CLI sending `opened_from` only inside a job.
- **Nine planted reverts**, one per new guard, each failing its tests: a result never marked by its program; a
  launcher's words unread; the hold saying egress, not program; the label's source saying egress; a job with no
  session; a session opened from a holder taking nothing; a turn sent to a named session taking nothing; the CLI never
  naming its job's session; the built-in default listing nothing. Restored, all pass.
- **The lane's gates**: 1,697 of 1,697, a plain turn 5 frames, at each commit.
- **Live**, on scratch daemons over a copy of the owner's store with his config:
  - at L0, `gh --version` latched its session `via: program`, and health listed it;
  - in that session, a `proc.run` of the CLI waited (`proc.run gh, at 12:10`); approved, the session its job opened
    held the text `via: job`, from the first; trusting it left the holder held;
  - in L1, a job printed its own session, and `gh --version` latched its session.

**The join.** The review (12:50 to 12:56) read `open_session`, `take_from_job`, the job's environment and
`job_result`, and the revert log, and accepted the lane. `main` fast-forwarded to 53d32e0. The join's gate (13:03:59,
126 s): 1,697 of 1,697, lifecycle ok (cold start p95 39.7 ms), the jobs phase's L1 start p95 5.94 ms, a plain turn 5
frames. The run that joined it ended before its push, and a later wake of the chain pushed `main` at 13:18. At the
place rule's join (Item 76) the label half of the by-program mark went with the labels: the hold's `via: program`, the
result's line, its `meta.external_program`, the ledger row and the next call's wait stay, with their test. At the
sandbox trims' join (Item 77) the listed-program marker became the fallback after egress's: a `gh` that reached only
listed hosts still holds its session.

**The install** (13:47, at 57a3759). The owner's note has no `external_programs` line, so his daemon lists `gh` by the
built-in default: the first `gh` call in a session holds it, and that session's next call that acts waits, until a
`/trust`. The CLI installed beside the daemon sends `opened_from` from inside jobs.

**Divergences.** Through a launcher the rule reads words, not what runs (`sh -c 'echo gh'` counts), and a
non-launcher's arguments are not read (`git -c alias.x='!gh …'` is not caught). An unknown or clean `opened_from`
opens a clean session rather than refusing. A job-taken hold names no node. `config.rs`'s line ceiling rose to 2,910.

**What it costs, said plainly.** A job can strip `THESEUS_SESSION`, or reach the daemon by a tool other than the CLI.
A program not on the list can print a stranger's text unmarked, and text laundered through a file is not followed.
Those are Jev's to judge (row 39).

**Known gaps.** The terminal UI does not send `opened_from` (no job runs it), and Discord's turns come from people.
The owner's M4 answer that a held session doesn't hold an L1 job with no egress and no secret was 20a's (Item 58), and is
not built: such a job waits under the hold like any call that acts (theseus-oaf9, P3).

### Item 75. The `gate-mode` lane: one gate lock mode, and a busy allowance for the gate's timing budgets (theseus-lew7; the simplification cut-list's Tier 5.3; 2026-10-03 11:40 to 12:35; 6b6fcb5 and 7e2de99 on a59b7c1; reviewed 12:56; rebased onto 53d32e0 as ce42bba and 4c74f75; joined 13:23 at 4c74f75; installed 13:47 at 57a3759, in `theseus-sim`)

**Why.** The simplification review's L3: the gate had two lock modes. The outer one, the default, had the caller hold
the shared lock for the whole run. The chain's join gate ran that way under `theseus-quiet.sh`, which SIGSTOPped the
lanes' compilers while it ran, and its paused processes wedged gates more than once (theseus-xfr1, theseus-e6xj). The
inner mode (Item 51) already held the lock only around the tests and benches, settled on PSI before each bench, and
reran a missed bench once. The owner approved one mode at 11:39, and added: "If it slows down the wall time of our
progress, I'm also fine with having a business wiggle room parameter, that basically says, gate performance checks
have an overage allowance budget that satisfies real world observation of deltas to that performance on busy
machines."

**What landed** (§9; `scripts/AGENTS.md`, "The lock" and "The busy allowance").
- **One mode.** The gate takes the lock itself, around the reader rule, the suite, and the benches, for every gate,
  the chain's join gate on `main` included, after every compile has run without it. `THESEUS_GATE_LOCK=inner` is
  accepted and changes nothing; `outer` is refused with exit 2 and a message saying it is gone, and so is any other
  value.
- **Kept:** `flock -o`'s re-exec, the lock's holders and queue in the log, the PSI settle, the flaky list, the one
  rerun, and the deadlock guard. The guard now covers every gate: one under a wrapper that holds the lock (`flock -o`,
  or a shell holding it on a descriptor, as `theseus-quiet.sh` did) exits 2 in a second, instead of compiling and then
  waiting for itself.
- **The busy allowance.** When settle's 5 minutes pass without a quiet window, the lifecycle and jobs benches get
  `--allowance`: `THESEUS_GATE_BENCH_ALLOWANCE` per cent of each limit (budget plus margin), 65 by default, 0 strict;
  a value that isn't a whole number is refused. A phase over its limit by no more than that passes, and says so:
  `busy: allowance +65% applied to cold start (measured 61.2 ms, limit 57.08 ms)`, after the bench's own strict
  `MISSED`.
  - It applies to times only. The turn bench's frames never get one, the bench refuses an allowance for any verdict
    not in milliseconds, and the run's other checks (the socket serving before the secrets, the swap's job kept and
    adopted, the restored store serving) are never excused.
  - A quiet window keeps every budget strict, as before, and so does a machine without PSI. Each settle decides for
    the run after it, so the rerun after a miss decides again.
  - The history keeps `passed` as the strict verdict, and a new last column, `allowance`, holds the percentage a run
    passed on. `bench history` counts those runs (`3 run(s), 2 missed (1 of them passed on the busy allowance)`) and
    marks each phase the allowance carried. Older binaries read the new rows, since they read columns by name.
- **The calibration.** The bench history's 170 lifecycle runs (2026-10-01 10:30 to 2026-10-03 07:58) and the 76 join
  gates' logs, classed by load and by priority.
  - At a load of 12 or more, at normal priority (the join gates' benches, and the lanes' benches run alone), the code
    otherwise healthy, there were 22 runs. In 21 of them every phase was within 63 % over its limit, and in all 22
    within 75 %; 65 is the 95th percentile rounded up. In milliseconds it takes the cold start's 57.08 to 94.2, a
    clean stop's 104 to 171.6, and an L1 start's 25 to 41.25.
  - An IO storm's two runs and four one-sample stalls over twice a limit are left out. No allowance should cover
    those, and the rerun does.
  - Strictly, 9 of those 22 runs missed a phase, against 4 of the 68 runs on a quiet machine (a load under 8), which
    the rerun covers, as before.

**How it is proven.**
- **The harness:** 19 cases of the gate, byte for byte, on a fake cargo and a fake bench, in seconds. Settle's PSI and
  load are files the harness writes, in a user and mount namespace. The cases:
  - the lock free, held, and failing inside and outside it; a leaked daemon that holds no lock; a compile under the
    lock; a TERM while waiting; an unwritable lock path;
  - `inner` accepted, `outer` and bad values refused, all before a single cargo call;
  - the gate under `flock -o`, and under a shell holding the lock on fd 9, as `theseus-quiet.sh` does: exit 2 in a
    second;
  - a quiet machine: no allowance anywhere;
  - a machine busy by IO pressure, and one busy by load: `--allowance 65` to the lifecycle and jobs benches, never to
    the turn bench;
  - an allowance of 0 (strict) and of 30;
  - a quiet first run that misses, the machine turned busy, and the allowance on the rerun only.
- **Unit tests:** the allowance's edge exact to the microsecond (94.18 ms is within 57.08 plus 65 %, and 94.19 is
  not); a count never carried, even at 100 %; 0 strict; the history row and `bench history`'s counts and marks. All 50
  of the sim binary's tests passed.
- **The bench on real measurements:** with the limits pulled 22 ms tight and `--allowance 200`, the cold start's 30.4
  ms against 28.08 printed `busy: allowance +200% applied to cold start (measured 30.4 ms, limit 28.08 ms)`, exited 0,
  and recorded `false,200`. With them pulled 30 ms tight and `--allowance 65`, a cold start 242 % over and a 717 ms
  stall failed, exit 1: a miss past the allowance still fails.
- **Whole gates:** the lane's whole gate on a quiet window was strict and green, 1,689 tests, in 231 s (cold start p95
  28.1 ms, an L1 start's 5.69 ms). The whole gate forced busy, by 16 niced `yes` processes: settle's 5 minutes passed
  and gave the benches the allowance. The first run's cold-start stall (+182 %) was past it, so it reran. The rerun's
  settle ended after 75 s at load 15.97, so the rerun was strict, and it missed by +56 %, inside the allowance. That
  gate failed, after holding the lock 582 s; it is the known gap below.

**The join** (12:56 to 13:23). The review accepted the lane as built and took one decision to the owner (below; he answered at 14:20). The
chain's wrappers retired with it: the join's own wrapper now runs `scripts/gate.sh` with nothing around it, and
`theseus-quiet.sh` is gone, with the notes that taught it. Under the old wrapper, this commit's gate exits 2 at once.
The join's gate, the first in the one mode (13:23:03, 166 s): 1,699 of 1,699, lifecycle ok in 10.0 s (cold start p95
31.8 ms), the jobs phase's L1 start p95 5.59 ms, a plain turn 5 frames; the lock taken after 0 s and held 117 s.

**The install** (13:47, at 57a3759). The allowance lives in `theseus-sim`, one of the five binaries an install ships;
nothing in the daemon changed.

**Divergences.**
- The deadlock guard stays. The brief would have cut it if it guarded only the outer mode. It guards every gate that
  takes the lock itself, so after this change it guards every gate, and it turns the chain's old wrappers into a
  refused gate instead of a gate that waits for itself.
- The jobs bench writes no history row, so its allowance passes show in the gate's log only.
- `settle()`'s bars are unchanged (see the known gaps).
- The shared bench history's file gained a new header; until every worktree has the change, old and new binaries
  write their own headers in turn. Each row reads under the header above it, so nothing is lost.

**Known gaps.**
- Settle's quiet bar (a load under the core count) judges strictly a band, loads of 12 to 16, where 4 of 6
  normal-priority runs beside unpaused lanes missed, and a second miss there fails a join. The owner's call, made at
  14:20: yes, a load bar at three quarters of the cores (12) and a 2-minute wait. Not built yet.
- The jobs bench's L1 start has no history row, so its allowance passes show in the gate's log only (no issue filed).

### Item 76. The place rule replaces labels on nodes, and the owner publishes (theseus-nbsh; the simplification cut-list's Tier 2; spine, in a worktree; 2026-10-03 11:03 to 13:11; four commits on a59b7c1, b0abb1f, deb7cf2, 9d80bd5 and 049ef27; reviewed 13:19 to 13:30; rebased onto 4c74f75 as 908f95b, c0b6b39, 8055609 and d2fd4d9; joined 13:31 at d2fd4d9; installed 13:47 at 57a3759)

**Why.** The owner's pick at 10:58, under the default-trust principle (§2): "Your place rule tactics are very innovative
-- I think this is a great idea. I for sure believe that's a better start than owner labels. We still might need a way
to 'graduate' private conversation content into publicly allowable, but the core there is superior." So graduation
stays in a light form, the owner's publish. He was open to a light map of nodes to what they are, asserted by Jev, and
at 11:24 preferred categories to labels: that map is the ontology's memberships (§4.1a), which route context and
never grant access.

19a to 19d kept private material out of a wider audience node by node. On the owner's setup labels mattered only in
`#openclaw` (8 viewers, 7 not the owner). There they withheld every owner node, including the session's own file, git,
command and AWS results, so those tools were useless. Held posts, graduation and quiet loops never fired: his store
held one `label.audience` row and no release. Four leaks surfaced in a day (42ub and jpff fixed, nbln and ntx5 parked),
and a fifth was possible (nuna).

The goal is unchanged: private material never reaches a shared place. It is now kept per place.

**What landed** (§3.9; P6).
- **The classes** (`places.rs`). Private: the CLI and the web UI (a session with no place), a DM with an owner, and a
  guild channel the bindings file binds with `private = true` (the operator's word, trusted). Shared: every other
  guild place, and a DM with someone who is not an owner. A private place gets everything, as before. A shared place
  gets its own conversation; the tools whose results are public by nature (`web.search`, `http.fetch`, `wake.*`,
  `task.*`); `fs.*`, `git.*` and `text.*` only under `[places] public_paths`, taken canonically (none by default, and
  then no file tools); no `proc.run`, no `aws.*`, and nothing else, since the list is an allow list; and only the
  context files marked `readers = "public"`.
- **The owner** is `[places] owner`, else `[approval] trusted_users`, else a bound DM's person, as approval takes the
  owner. 19a's `[labels]` section, which had the same two keys, still loads as `[places]`.
- **A turn's class** follows where its words go: the session's place; for a task, its parent's, through the tasks
  index; for a session no place runs any more (after `/new`), the place its wakes and reports answer in (theseus-4lx);
  else the CLI's or the web UI's. If that cannot be read, the turn is a shared place's. It is fixed for the turn after
  catch-up, as the request's spec is.
- **Enforcement:**
  - the catalog offers a shared place's model only those tools (`definitions_for`), and its system note says why;
  - the gate refuses the rest, as a backstop, through the invalid-input path: the record's reason is `place: …`, and
    the model reads `Not run: fs.write is not offered in a shared place: one others can read gets only the public
    tools`;
  - the compile drops a shared place's non-public context files, and the manifest and `context.compiled` record the
    class and what was withheld.
- **The binding** tells the core its places when it starts (`bind_places`): each `[[channel]]` with its new `private`
  key (false by default), and each `[[dm]]`. Nothing is stored, so a place's class always matches this run's bindings
  file. A guild place it has not named (Discord off, or not started yet) is shared. After serving it reads once who can
  view each channel bound private, writes a `place.viewed` row, and health warns when anyone besides the owner can.
  Nothing reads viewers before a turn or a post.
- **Surfaces:** health's `places:` line; `theseus places`; the cockpit Boundaries board's Places panel and a
  shared-places count; the Observatory's class badge on a session's transcript.
- **Publish** (`place.publish`), light graduation and the one way the owner's material reaches a shared place: a node
  by id, a file the owner can read, or a message, with an optional note, from `theseus publish NODE|FILE --to PLACE
  [--note …]` or `--text …`, Discord's `/publish`, or the cockpit's publish control on an answer or a result. Only the
  owner, from a private place: it is judged as an approval is (a Theseus job's process never may), then by the place
  rule, and both checks come before anything it names is read. One frame, never while a turn holds the place's
  session, writes the item into the place's conversation as the owner's message, under a header that says what it is,
  with his note; a `derived_from` edge when it copies a node (`publish`, so `node.reach` follows it); a
  `place.published` row (who, through what, the source, a digest, the bytes, the place); and a notice in the place. A
  refusal is an `approval.refused` row, and nothing else is written.
- **M6:** recall and the books, when built, draw in a shared place only on that place's own sessions
  (`docs/design/m6-memory.md`).
- **Deleted:**
  - the work of 19a, 19b and 19c, and 19d's fixes, which lived in the meet: labels, the compile filter and its
    placeholders, the audience recompile, held posts and the kernel's held-post stage, quiet loops (loops stream
    everywhere again), the post-time viewer read, graduation (`theseus graduate`, the web UI's Graduate), and the
    disclosure simulator (10 files, and its run in the gate);
  - `theseus labels` (now `theseus places`), health's `labels:` line (now `places:`), the `theseus.compile.withheld`
    metric and its bench, the Observatory's label and held badges, and the cockpit's labels and held panels;
  - Discord's per-turn audience push and its viewer reads before guild turns. Its viewer walk stays, for the approval
    check and the start-time read.

  The removal commit is −8,850 lines (+621 −9,471); the step is net −6,426 (+2,999 −9,425). NODE 7 drops `label`, and
  COMPILATION 5 drops the manifest's `audience`, `readers`, `integrity` and `withheld`; both readers ignore the old
  fields. 19a's five `label.*` ledger kinds read as unknown kinds, byte for byte. Confidentiality no longer depends on
  Discord's Server Members intent. T1's hold on outside text is untouched.

**How it is proven.**
- **Ten planted reverts**, each failing its test and passing restored: the catalog offering every tool; the gate
  refusing nothing; a shared place carrying every context file; a task classed by its own place; a moved-on session
  classed by its own place; a channel bound private taken as shared; publish taking a shared place for a private one;
  publish taking anyone approvals trust for the owner; publish skipping the place check; publish reading what it names
  before it judges who asks. The six place-class reverts ran twice, with 19a's labels still on (commit 1 turned the
  class gate on before commit 2 removed them, so no commit let a shared place receive the owner's material) and after
  the removal.
- **Stored data:** a test stores 19a's five ledger kinds and reads them back byte for byte. Literal NODE 5 and 6 and
  COMPILATION 3 and 4 records still read.
- **The lane's gates:** 1,697, 1,662, 1,665 and 1,665 of as many, a plain turn 5 frames at each.
- **Live, on a copy of the owner's store** with his config, Discord off: his `#openclaw` session, unbound and so shared,
  compiled with 14 tools and no `proc.run`, his own context file withheld and the public one carried; a CLI session
  compiled private, with 15 tools and both files; his 19a-labeled nodes and audience manifests read whole.
- **Live, on the fake Discord:** a write in a shared channel was not run (`place: fs.write is not offered in a shared
  place…`, and no file); a published file reached the channel with its notice, its row and its digest, and the next
  turn read it back; with the channel bound private, health named it private; open to another member, health read
  `#lab (⚠ bound private, but 1 person besides the owner can view it: cy)`.

**The join** (13:19 to 13:31). The review read the code (the class is fail-closed; the allow list; every file tool's
plan names its base path, defaults included, so the path check covers a call that gives no path; publish judges before
it reads) and re-ran two planted reverts on the rebased code, the gate's refusal and publish's order, both failing as
they should. One conflict, with the integrity lane (Item 74) in `toolrun.rs`'s `result_node`, took this step's side, a
result with no label: integrity's by-program mark on the label's source went with the labels, and only its test's
label assertion (6 lines) and three doc lines went with it. The hold's `via: program` and the rest stay. The join's
gate (13:31:16, 242 s): 1,677 of 1,677, lifecycle ok in 10.8 s (cold start p95 34.2 ms), the jobs phase's L1 start p95
7.22 ms, a plain turn 5 frames. Closed as moot: theseus-nbln, -ntx5, -2lvc and -nuna (by the lane), theseus-celu.6,
-celu.7, -tbv2, -zupl and -033g (at the join; two parked cloud branches deleted), and theseus-0yz6 and -843s (the
simulator's gaps).

**The install** (13:47, at 57a3759). Health on the owner's daemon read `places: private: CLI, web, DM @zeroaltitude · shared:
#openclaw (public tools only)`, as the step's report had predicted: his DM private, since his id is a trusted user, and
`#openclaw` shared, with the public tools alone and no file tools, since his note names no public trees.

**Divergences.** The brief's "read again when the binding changes" is the next start, since a bindings file is read
only then. A DM with someone who is not an owner is shared, which the brief did not say. Publish judges who asks before
it reads (049ef27, found while the report was written: before it, a non-owner's refused `/publish` in a shared channel
could tell whether a path existed, or a file's size). The class follows a moved-on session's wake target (theseus-4lx),
which the brief did not name. A class change keeps the conversation: a channel rebound from private to shared keeps
what was said while it was private, the operator's act, and `/new` starts afresh.

**What it costs, said plainly.** Fine-grained mixing is gone: one owner item visible to named people across places is
now an explicit publish. A private channel that gains a member is noticed at the next start. `#openclaw` gets the
public tools alone.

**Known gaps.** theseus-94a6 (P3): an `http.fetch` of a private address, approved from a shared place, brings that
page into the shared conversation; the approval rule makes it wait, but not on the place. The owner's call, at 14:20:
leave it to the approver, whose card is to say it is a private address in a shared place. Not built yet.

### Item 77. The sandbox trims: listed egress doesn't latch, no probe at the start, no delegated cgroup, and no L1 job as root (theseus-gyin; the simplification cut-list's Tier 4, 4.1 to 4.3, with theseus-pv6i's intent; the `sandbox-trims` lane; 2026-10-03 11:36 to 12:55; one commit on a59b7c1, ba7a8c6; reviewed 13:19 to 13:37; rebased onto d2fd4d9 as 1378382; joined 13:37 at 1378382; installed 13:47 at 57a3759)

**Why.** The owner approved the explanation at 11:25: "4.1, 4.2, 4.3, and the keeps: I follow your recommendation for
all". This is the default-trust principle (§2) applied to L1's add-ons. The core of the sandbox stays whole: the
namespaces, the view, scratch, no capabilities, the egress allowlist, seccomp, and the pid namespace's verified stop.
Kept as approved: seccomp, the broker's per-program parsers, the scrubber's heuristics, and `[policy] external_text`,
which stays `ask`. At 11:26 he confirmed that web text stays at ask, and that Jev's security judge earns relaxing it
("see if it works well in practice"; theseus-hnc8 brings him the numbers once row 39 runs).

1. **Egress to listed hosts no longer counts as outside text.** Counting it had made a sandboxed build that fetched
   its crates stop to ask.
2. **No self-test after every start.** It cost a subprocess, an L1 clone, `systemctl show`, and a frame, 3 s after
   every start.
3. **No delegated cgroup.** It covered only the L1 jobs the model opted into, while L0, the default, has no memory
   limit. It tied the sandbox to the unit's settings, and brought a bug class of its own.

Without the cgroup, a root daemon's L1 job would have no process limit, since Linux exempts root from `RLIMIT_NPROC`
(theseus-pv6i, found on the first cloud batch's root VM, Item 67), so it is refused.

**What landed.**
- **4.1.**
  - The rule: a result is marked external only when its job reached a host beyond `[sandbox] egress`, which only the
    approval of its call's own list let it reach (`egress::external`, `marker`). The marker names those hosts alone.
    A job that left no report is marked only when its bound list held a host beyond the operator's. An operator list
    that fails to parse counts every host as beyond it.
  - Unchanged: the egress rows, health's counts, and the result's lines.
- **4.2.**
  - Gone: the probe after serving (`PROBE_AFTER` and its three helpers), `Sandbox::probe`, the kernel's probe mode,
    theseusd's `sandbox-probe` role, the `SandboxProbed` fact, and `sandbox.probe`. A stored row still reads, as an
    unknown kind: the copy of the owner's store returned its 9 old rows.
  - Health's line now reports the last real L1 launch, from its job's completion (`Sandbox::launched`): "no L1 job yet
    since start", "the last L1 launch worked (start 8.1 ms)", or why it failed. The Observatory's and the cockpit's
    sandbox lines say the same.
  - `theseusd check` runs the self-test on demand, `/bin/true` through the real job path (`job::self_test`, then
    `job_l1::run`), and prints its verdict; a failure fails nothing, since L1 is optional.
- **4.3.**
  - Gone: `theseus-sandbox`'s `cgroup.rs` (306 lines) and `Spec::cgroup`; the core's `find_cgroup`, `systemctl_show`
    and `judge`; the daemon's move into a `daemon/` leaf; the `cgroup-release` role; the job cgroup, its limit-hit
    lines (`pids_refused`, `oom_kills`, `cgroup_error`) and the cgroup branch of a stop. An L1 stop is verified by its
    pid namespace alone; `VerifiedBy::Cgroup` stays so stored verdicts still read.
  - The units (`--user`, `--separate`, and the job host's) lose `Delegate=yes` and the `ExecStopPost` stop hook, and
    keep `KillMode=process`. `user-service.sh` loses its cgroup check, and `docs/user-service.md` says what an older
    unit's leftover lines do.
  - `[sandbox] memory_mb` leaves the template; a config that sets it still loads, with one warning per load
    (`sandbox.memory_mb is retired and ignored`).
  - `sandbox.usage` lists only the L1 jobs running now, each with its command and running time; the cockpit's live
    list lost its gauges.
  - Unchanged: each job's `RLIMIT_NPROC` and `RLIMIT_FSIZE`, the scratch caps, and the pid namespace.
- **Root.** `theseus_sandbox::spawn` refuses a root operator's job before it makes anything, at the stage "checking the
  job's process limit": "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC, so an L1 job would have no
  process limit: run theseusd as an ordinary user". Every L1 start passes through spawn: the wrapper's, `check`'s, and
  the tests'. Health says `sandbox: L1 is unavailable: <why>`. This is the parked root-l1 branch's intent, lighter: no
  process-limit module, no cgroup branch, and no narrative line for a refused job, whose result already says why.
- **Net −764 lines** (+950 −1,714), the Rust sources −727; the debug `theseusd` is 569,512 bytes smaller. Health's
  wire shape dropped `memory_mb`, `probe` and `cgroup` and added `last_launch` and `refuses`, so an older CLI can't read
  a newer daemon's health: every binary installs together.

**How it is proven.**
- **Five planted reverts**, each failing its tests and passing restored:
  - 4.1 both ways, through the core and the daemon's real proxy: every host latching again (4 tests fail), and no
    host latching, not even one beyond the list (4 fail);
  - the refusal's rule (`refusal()` never refuses);
  - the refusal's place, run as root through the contract's root case (with the binary copied where root's user
    namespace can execute it): planted, "a root operator's job started, with no process limit"; restored, refused with
    its words;
  - health's last launch (`job_result` records none).
- **The lane's gate**: 1,690 of 1,690, a plain turn 5 frames.
- **The lifecycle bench, alone, in two A/B rounds** (debug builds, one hold of the shared lock, a palindrome order, PSI
  checked before each run; round 2 with one driver and a third build of the lane's commit in the first arm's tree): the
  start is unchanged within noise, as the code requires (cold start p50 25.0, 24.1 and 27.3 ms), and every budget held
  in every run. A start's aftermath lost one frame (the WAL went from frame 8 to 9, not 10, with no `sandbox.probe`) and
  one subprocess with its L1 clone.
- **Live**, on a scratch daemon over a copy of the owner's store with his config, run as a plain `systemd-run --user` unit
  with `Delegate=no`:
  - health said "no L1 job yet since start", then "the last L1 launch worked";
  - `theseusd check` said "L1: the self-test worked (start 10.4 ms)";
  - GLM's L1 job to a listed stand-in host left its session clear, and a job to a host beyond the list, once approved,
    held its session (`via: egress`);
  - the unit's cgroup had no child cgroups, and the log gave the one `memory_mb` warning his note causes.

**The join** (13:19 to 13:37). The review read `egress::external` and `marker` (fail-closed on an unparsable list) and
the root refusal, and re-ran two planted reverts on the rebased code: every host latching (8 tests failed) and the
refusal's rule (2 failed). The rebase onto the place rule met one conflict, in `toolrun/job.rs`: this step's marker
with the operator's list comes first, then the integrity lane's listed-program marker when egress marked nothing, so a
`gh` that reached only listed hosts still holds its session, `via: program` (Item 74), and a plain job reaching listed
hosts holds nothing. The protocol's types and the web UI's build were regenerated, and one formatting fix amended
before the gate. The join's gate (13:37:45, 211 s): 1,680 of 1,680, lifecycle ok in 9.9 s (cold start p95 33.7 ms), the
jobs phase's L1 start p95 5.97 ms, a plain turn 5 frames. theseus-gyin, theseus-pv6i and theseus-celu.5 are closed,
and root-l1's cloud branch was deleted.

**The install** (13:47, at 57a3759). `scripts/user-service.sh install` rewrote the owner's unit, with no `Delegate=` and
no stop hook, and `KillMode=process` kept, before a restart taken with no turn held and no L1 job running. `theseusd
check`: `L1: the self-test worked (start 4.2 ms)`. Health: `sandbox: no L1 job yet since start · default l0 · jobs: 0
at L0, 0 in L1 · an L1 job gets 512 processes, 1024 MB of scratch, files up to 64 MB · no egress listed`. The journal
held the one expected warning, `sandbox.memory_mb is retired`: his note sets it, and a re-paste of the template clears
it.

**Divergences.**
- One commit, since 4.1, 4.2 and 4.3 share files.
- The self-test reuses the job path instead of the old probe function.
- root-l1's narrative line for a refused job was dropped.
- `memory_mb`'s warning lives in `SandboxConfig`, because `config.rs` is at its line ceiling.

**What it costs, said plainly.** An L1 job may use all the machine's memory, as an L0 job may (`RLIMIT_AS` is a
one-line limit, if one is wanted). Text injected through a listed host no longer holds a session: 4.1's approved risk,
latent while the owner's list is empty. Health can't say "L1 works" before the first L1 job; `theseusd check` can. The root
refusal was proved by the contract as root, not on a root daemon.

### Item 78. AWS's C2: the stacks, owner-role sessions, writes behind the floor, and the budget, in the lean posture (theseus-nyzn; row 30, step 14b, stage D; spine, in a worktree; 2026-10-03 11:32 to 13:16; 3fc88ec and 1cbf640 on a59b7c1; reviewed 13:30 to 13:41; rebased onto 1378382 as 438a33d and 57a3759; joined 13:44 at 57a3759; installed 13:47 at 57a3759; the bootstrap's apply cleared by the owner at 14:20)

**Why.** C1 (Item 49) bound the account and its reads; C2 brings the writes, with AWS's own guards in front of them.
It waited on the owner's go-ahead for the first writes to his account, and he gave it at 11:24, with a cap under $1 a
month on what the stacks themselves cost. A read-only cost check first (AWS's Pricing API) put the design as written at
about $2.80 a month (two customer keys $2.00, 17 alarms $0.70, the trail's writes $0.07, GuardDuty about $0.02), and a
lean posture at about $0.10. The account had no trail, alarms, GuardDuty detector or customer keys, one budget, and
about 50 management events a day. C2 was built lean.

**What landed** (§3.25).
- **The templates** (`infra/aws`), lean by default.
  - **`theseus-foundation`**: the Theseus bucket on SSE-S3 and the durability table on DynamoDB's own key, so the
    foundation's `TheseusKey` and its alias are gone. The alerts topic has no SSE, on purpose: SNS encrypts only with
    KMS, and EventBridge, Budgets and CloudWatch can publish to an encrypted topic only through a customer key's policy
    ($1 a month); a message rests there only until delivered, and the queue it feeds is SSE-SQS. The template says so,
    and the rule tests check that it does. The guards and the boundary are now the documents `guardrails.toml`
    generates, one minified line each, and the guard crate's test fails when a line is out of step. `MonthlyBudgetUsd`
    defaults to 50.
  - **`theseus-posture`**: the trail's bucket on SSE-S3, with a `TrailKey` parameter (`aws-managed`, the default, or
    `customer`, which creates the trail's own KMS key). The 17 checks are EventBridge rules on CloudTrail's management
    events, into the alerts topic, each alert naming its rule; they replace the 17 metric filters, the 17 alarms, the
    log group and its role. The root-of-trust rule fires on anything the vault key signs except `sts:GetCallerIdentity`
    and an `sts:AssumeRole` into `theseus-owner`, so an AssumeRole into any other role alerts too. Three rules
    (refused calls, the root user, the root-of-trust key) see read-only events as well. Access Analyzer, GuardDuty and
    their findings rules, and the snapshot block are as before.
  - **`theseus-posture-relay`**, new, in us-east-1 when the home region is another: rules are regional, and the global
    services (IAM, Organizations, Budgets, the console's sign-in, STS's global endpoint) record their events in
    us-east-1 only. One rule forwards that region's CloudTrail events to the home region's default bus, through a role
    allowed only `events:PutEvents` there.
  - The stack policies follow. `check.sh`: cfn-lint on five templates, 0 violations, and 32 rule tests ok.
- **The guard crate, wired in** (`theseus-aws-guard`, merged ahead of its reader in Item 16, read now). `alarm-tamper`
  became `alert-tamper` (the posture rules and the alerts topic). `planned_resources` reads a template's resources under
  its parameters, an undecidable condition a maybe. Its `reserved_for` marker is removed.
- **Who signs** (`aws/session.rs`). Until the config names `owner_role`, the key signs, as in C1. Once it does, the key
  signs only STS, and each call signs in a role session minted by `sts:AssumeRole` into the owner role: **work**, named
  by the execution, with the guards then `theseus-allow-all`, for 12 h, refreshed; **job**, named by the job's
  correlation id, adding `theseus-guard-stacks`, for the job's deadline; **floor**, `<execution>.floor`, with
  `theseus-allow-all` alone, for 15 minutes, never cached, minted only for a call the operator approved at the floor;
  and the **tender**'s, with an inline policy that allows exactly the budget's change set, its read, and GuardDuty's
  reads. Every session's source identity is the deployment, its tags name the execution, the kind and the deployment,
  it is held in zeroizing memory and never written, and each mint is an `aws.session.minted` row, never a credential.
- **`aws.call` writes.** Its plan reads the guard list: a direct guardrail is the floor at every posture; a stack
  write, or durable infrastructure, is invalid input that points to `aws.stack.plan`, and nothing is sent; any other
  guardrail is the floor; a deletion of what holds state waits. The verdict rides the plan in memory only
  (`AwsPlan.guardrail` and `destructive`, `serde(skip)`), and the gate's record keeps it, so no node layout changes.
  `[policy.aws]` reads the call's own class: the operation, the service, then `read`, `write` or `run`. An approved
  floor call that AWS's guards would refuse runs in a floor session. `aws.call` is not repeated after a crash: a write
  cut by one is unknown. Secret-bearing reads stay invalid until C3.
- **The stack tools**, refused until the owner role is named: `aws.stack.plan` (a change set under
  `theseus-cfn-deployer`, its diff, its floor and approval lines, and its digest); `aws.stack.apply {stack, digest}`
  (exactly that change set, at the floor when it touches a guardrail, asking when state would be lost, re-described
  and refused if its digest changed, waiting up to 30 minutes to settle: `Tool::deadline` is new); `aws.stack.status`
  (the stacks, or one with its outputs, drift status and recent events); and `aws.stack.delete` (always asks, and says
  what `Retain` keeps).
- **The bootstrap**, `theseus aws bootstrap` (the method `aws.bootstrap`), the CLI's alone: refused from Discord, the
  web UI and a job's process, before anything is read. Its plan is read-only: the account check, one probe for an owner
  session, a new stack's resources from its template (no change set), an existing stack compared by `GetTemplate`, the
  singletons (a GuardDuty detector, an account analyzer, other trails), and a digest over each stack's template,
  parameters and action. Its apply plans again and refuses a changed digest, then makes the foundation by a change set
  signed with the key, takes a floor session of the new role (retried for up to a minute while IAM learns it), makes the
  posture and the relay through the deployer, each change set checked against the plan, and sets each stack's policy
  and termination protection. The operator's one approval covers its guarded changes. On a terminal it asks `[y/N]`
  after a plan with changes.
- **The tenders** (`aws/tend.rs`), after serving, once the account is bound and names an owner role: the budget's
  reconcile once a start (a change set of the foundation with the config's `monthly_budget_usd`, executed only if its
  one change is `MonthlyBudget` modified with no replacement, else deleted and health says STOPPED; ledgered as
  `aws.budget.reconciled`); the budget's line every six hours (free); and GuardDuty's usage weekly (free), projected to
  a month, health warning past $1.
- **`aws.cost`**: Cost Explorer's month to date, or last month, by service, at $0.01 a call, so an answer is kept a
  day.
- **`[broker.programs.aws] aws_account`**: the `aws` CLI, run by its own argv, gets a job session at its launch (its
  key, secret and token, the region, and `/dev/null` for both profile files), never the key, and nothing before the
  owner role exists. The harness-only line then ends `; jobs get short-lived AWS sessions, never the key (aws)`.
- **The config and the surfaces.** New keys: `[aws.accounts.<id>] owner_role`, `deployment` and
  `monthly_budget_usd`; `[policy.aws]` takes `write` and `run`; `[broker.programs.<p>] aws_account`. Health and
  `aws.whoami` say what signs each account and give the budget line (`$3.21 of $50 this month (forecast $4.10)`), and
  health gives GuardDuty's projection and the reconcile. `config/aws.rs` and the CLI's `render/aws.rs` split out of
  files that sat at their line ceilings since C1 (theseus-sk47, theseus-wdcw).

**How it is proven.**
- **Tests** (`aws/tests_c2.rs` and beside it): the floor for each guardrail group at every posture (public ingress, the
  trail, encryption at rest, long-lived credentials, the budget, the guards), each near miss open; IaC-only and stack
  writes invalid at plan and at run, the fake seeing no request; deletions waiting, and each class taking its line; the
  session policies composing as their kinds say, by an IAM evaluation of the generated documents, within STS's limits;
  the key signing only STS, against a fake STS and service; the reconcile idempotent and touching only the budget, and
  the bootstrap planning read-only, applying once and planning no change after, both against a fake CloudFormation with
  state; the lean templates making no key, alarm, metric filter or log group by default, and exactly one key with
  `TrailKey=customer`; a program's AWS grant a job session, never the key.
- **Five planted reverts**, each failing its tests and passing restored: the floor not asking, IaC-only cleared, the
  approve list off, a work session without the guards, a reconcile that applies a change beyond the budget. The
  sessions plant was caught by the composition test; the signing test passes under it, since both its sides read the
  same list.
- **The lane's gates**: 1,702 of 1,702, twice, a plain turn 5 frames.
- **Live, read-only, no write of any kind**: `ValidateTemplate` on the three templates; `TestEventPattern` on all 20
  rules, 51 cases, 0 wrong (among the misses an AssumeRole into `theseus-owner` by the key; among the matches an
  AssumeRole into another role); and the bootstrap's plan through a scratch daemon of this build: three stacks to
  create, 22, 25 and 2 resources, and no warnings. CloudTrail's event history showed 95 events by the key since 11:30,
  every one read-only, and no successful write.

**The join** (13:30 to 13:41, joined 13:44). The review read the bootstrap's refusals (the CLI only, then not a job's
process, before anything is read), the reconcile's one allowed change, the floor session's use, and the `aws` CLI's
job session, and re-ran two planted reverts on the rebased code, the floor and the sessions, both caught. The rebase
onto the trims was clean: the RPC server's act list holds 14, with the place rule's `PLACE_PUBLISH` and this step's
`AWS_BOOTSTRAP`. The join's gate (13:44:25, 233 s): 1,695 of 1,695, lifecycle ok in 9.7 s (cold start p95 26.4 ms), the
jobs phase's L1 start p95 5.37 ms, a plain turn 5 frames.

**The install** (13:46:59 to 13:47:08, at 57a3759: one install of everything since 03:32, Items 73 to 78 and docs
v0.79). A `release-thin` build of the five binaries (1 m 58 s), the store backed up first, `theseusd check` (8 secrets
resolved in 2,038 ms, the L1 self-test 4.2 ms), the unit rewritten by `scripts/user-service.sh install` for the
sandbox trims, then a restart with no turn held and no L1 job. Health: the places line and the sandbox line (Items 76
and 77); the config confirmed by the vault in 975 ms; the secrets ready 1,025 ms after the start; Discord ready; the
index ready; the unit active, with no restarts; the cockpit answering. C2 itself shows nothing yet: the owner's note binds
no AWS account, so his daemon has no `aws:` line until the bootstrap is done.

**The bootstrap, cleared.** Its apply is a write to the owner's account, and waited on two answers from him: the address
the alerts topic emails (committed nowhere), and lean or the trail's own key (`--trail-key customer`, $1 a month more).
He gave both at 14:20: lean, with no trail key. What follows, after this record: a fresh cost check of the exact stacks; the plan, from a scratch daemon of the installed build, showing three
creates of 22, 25 and 2 resources and no warnings; the apply; his note's `[aws.accounts.<id>]` table with
`owner_role`, after which health should say the account signs with role sessions; the design's write checks, with his
go-ahead; and S3's account-level Block Public Access and EBS encryption by default, set by hand.

**Divergences.** `owner_role` is a config key the operator adds after the bootstrap: the design had the bootstrap set
it, but the config is a note no agent writes, and an AssumeRole probe in the daemon could fall back to the key silently
on a passing failure. `aws_account`, not the design's `account`. The relay stack is new. The verdict rides `AwsPlan` in
memory only, so NODE's schema stands. The bootstrap's one approval covers its guarded changes, in one floor session
that lasts an hour. The alerts topic is unencrypted, on purpose (above).

**What it costs, said plainly.** About $0.08 to $0.10 a month at this account's activity: the trail's log and digest
writes about $0.07, GuardDuty $0.01 to $0.02 after its 30-day trial, storage and the relay under $0.01. The trail
records every region, but the rules alert only in us-west-2 and us-east-1, the account's allowed regions, and
GuardDuty runs in the home region alone; the alarms' log group had seen every region. That is the lean posture's
trade. SCPs would make the other regions unusable instead, and the owner set them aside at 14:20: "for now, budgets and
notify are fine -- visibility first".

**Known gaps** (not built, as the design allows): the 100 % budget wait, the $5-a-day and $1-an-hour tripwires (they
come with the hands' reservations, row 40; until then AWS spend is bounded by the $50 budget, its stop at 100 %, which
lags spend by hours, and the six-hourly line), the weekly drift tender (`aws.stack.status` shows drift), the key's
90-day age warning, the inventory (C3), the plan's cost estimates, a job's narrowing (`aws_policy`), the
`aws.stack.planned` and `aws.stack.applied` rows, and committing a planned template to git. The stack tools' writes
and the bootstrap's apply are tested against fakes only, and GuardDuty's usage read was not exercised live (the
account has no detector yet).

