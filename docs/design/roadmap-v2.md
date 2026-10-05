# Theseus v1, re-cut: one spine, parallel lanes (roadmap v2)

_Checked in 2026-09-30 from the design lanes. Scrubbed for this public repository: the Home account's id and alias, the operator's employer's name, another project's name, the operator's Discord server name, and local paths to the agents' working files and reports._

_Beads theseus-zaz, under the v1 epic theseus-s1t. Written by Tabitha/Claude on 2026-09-30, from 16:27 MST, to the
recut's brief. Sources: the v1 roadmap, the chain's
queue file, the six designs ([aws-toolset](aws-toolset.md), [stage2](stage2-operator-surfaces.md), [m4](m4-boundaries.md), [m5](m5-judgment.md), [m6](m6-memory.md), and [m7](m7-surface.md)), the agents' operating notes for the repo, and read-only looks at the repo (`main` at
1353a83), Beads, and this machine. Docs only: nothing was built, run, filed, or sent. Every time and duration here
is an estimate unless it says "measured". The five phase designs were still under Tabitha's review while this was
written. If her review re-marks a step, the counts change but the shape does not._

**Status: written section by section; the last line marks the document complete.**

## Contents

0. The answer on one screen
1. The shape: one spine, lanes beside it, and how many at once
2. The spine, in order
3. The lanes
4. A timeline estimate
5. What starts now
6. Conflicts found across the designs, and how to resolve each
7. Eddie's answers still needed
8. Appendix: where the step counts come from

## 0. The answer on one screen

- **The shape.** One spine on `main`, one step at a time, as the chain runs today. Beside it, **three Rust lanes
  and one light lane**. Each lane has its own worktree, `CARGO_TARGET_DIR`, and sccache, and runs at nice 19 and
  idle I/O with 4 jobs. Memory and Tabitha's review time set that limit. Cores and the 8-agent cap don't.
- **The honest size.** The six designs cut the old plan's 42 steps into **about 106 hour-sized slots**. Lanes
  take 29.5 of them off the critical path, and **the spine carries 76.5**.
- **When (an estimate).** The build completes around **Oct 4, 22:00** at the brief's 1.3 hours a slot, or
  **Oct 5, 17:00** at 1.55 hours a slot, the chain's measured pace over the last 37 hours. One step at a time,
  the same scope would end Oct 6 or 7. The old figure of "about 55 hours" undercounted. v1 then closes after
  Eddie's two-week soak, around Oct 19 or 20.
- **The spine is the bottleneck, not the lanes.** The 32.5 lane-steps need only about 16 hours of three lanes.
  What shortens the plan is less spine:
  - the memory headroom test, which could save about 5 slots;
  - the DAVE spike, about 2;
  - Stage 2's scope.
- **The order:**
  - A. Stage 1's remainder: theseus-kol, the dogfood pilot, fix batches 1 and 2, batch C, the reader rule, and
    review 2's proposals;
  - B. Stage 2: the protocol push, with the TUI and herdr as lanes;
  - C. M4, then E. M5, then F. M6, then G. M7;
  - D. AWS floats: it enters when Eddie's go and its lanes allow.
- **Starts today, with nothing needed from Eddie:**
  1. M6's memory exam and headroom test;
  2. the L1 sandbox crate;
  3. the Jev client crate;
  4. in the light slot, the voice spike (DAVE), then the cache measurements.

  Then the index, the ontology, the installer, MCP, and the memory math as slots free. The TUI and herdr start
  when 10a lands, on Oct 1 in the afternoon.
- **Conflicts: 13 found, each resolved in §6.** The sharpest:
  - M4 and M6 both claim NODE 3 and COMPILATION 3, so versions are now assigned when a step lands;
  - the WAL follower and the canary arms were each planned twice, and are now built once;
  - AWS's durability tender left out `blobs/`;
  - M5 waited on AWS, and now follows M4.
- **What only Eddie can unblock** (none of it stops the spine):
  1. a "go" for the three offline AWS lanes;
  2. the go-ahead for the first writes to his AWS account;
  3. consent to send session content to TypeSafe;
  4. speech providers;
  5. a test voice channel.

## 1. The shape

**The spine** is the build chain on `main`, one step at a time, exactly as today:
- a subagent builds the step. Then Tabitha reruns the gate, builds release, checks it live, installs it, folds
  [the spec](../the-ship-of-theseus.md), updates Beads and the queue, and messages Eddie;
- a step is spine when it touches the kernel, the store, the core's gate or turn loop, or the protocol, or a
  shared entry file: the CLI's `main.rs` (3,194 lines), the daemon's role dispatch, or `scripts/gate.sh`.

**A lane** is work that lives in its own crate, binary, or directory:
- its own worktree, `~/projects/theseus-wt/<lane>`, on branch `lane/<lane>` (the AWS design's `aws/<slice>` is
  the same pattern);
- its own `CARGO_TARGET_DIR=~/.cache/theseus-target/<lane>`, because a worktree that shares `target/` poisons
  the main tree (per the agents' operating notes for this repo);
- `RUSTC_WRAPPER=~/.cargo/bin/sccache` (installed 15:15). Third-party crates hit the shared cache across lanes,
  since their registry paths are the same. Workspace crates don't, since each worktree's path differs. Raise
  `SCCACHE_CACHE_SIZE` from its 10 GB default to about 40 GB: tantivy, candle, and the AWS signer are big;
- every cargo command under `nice -n 19 ionice -c3`, with `-j 4`;
- it touches only its own crate or directory, plus one line in the workspace's `members` list;
- it commits after every sub-step. A parent abort kills every subagent at once, so a relaunch resumes from the
  tree;
- before it joins, it rebases onto `main`, regenerates `Cargo.lock` (never hand-merged), and runs the whole
  gate in its own target dir.

**Review.** Every lane step gets a light review: the gate's result, the diff, and the crate's own live check,
with no install (about 15 minutes). The deep review (the live check through a daemon, the install, the spec
fold) happens once, at the wire-in.

**The wire-in.** A lane joins `main` through a spine step, in one of two ways (§2 names which, for every lane):
- **folded in**: the spine step that first uses the lane's crate merges it as its first act (17a joins in
  17b);
- **a join**: a half-slot spine step that merges, runs the gate, and installs (the TUI's 10f).

**How many lanes at once: three Rust lanes plus one light lane, beside the spine.** A light lane builds no heavy
Rust: YAML, web, docs, or a short spike. Why (measured at 16:27 unless marked):
- **Memory is the binding limit.** The machine has 23 GB, 17 GB of it available.
  - Estimated: the spine's gate and release build take several GB, and a lane at 4 jobs about 2 to 4 GB
    (tantivy, candle, and the AWS crates at the top). Three lanes plus the spine come to about 12 to 16 GB.
  - A fourth heavy lane risks swap and the OOM killer.
  - The WSL VM, capped at 24 GB on a 32 GB host, has died several times this month from host-side causes, host
    memory pressure among them. Each death takes every lane and the spine with it.
- **Review is the other limit.** Tabitha reviews every step: about 30 minutes per spine step and 15 per lane
  step. At this load that is about 40 to 45% of her time, and more lanes would make review the bottleneck.
- **CPU: 16 cores.** The spine's gate runs at full priority. Three lanes × 4 niced jobs use only idle cycles.
  More jobs would add cache and memory-bandwidth contention to the bench, and would never speed the spine.
- **Disk: 478 GB free**, with `main`'s `target/` at 100 GB. A lane's target runs about 10 to 20 GB (estimated).
  Delete it when the lane joins.
- **Agents: the gateway caps active subagents at 8** (`maxChildrenPerAgent`, raised 15:25). 3 are in use now
  (T1b, review 2, this re-cut). The spine, three lanes, and one light lane take 5, so agents are not the limit.
- **The bench.** The lifecycle bench's §9 budgets have per-phase margins of 7/4/25/2 ms. Niced lanes can still
  make it miss, and the gate syncs and reruns once. If misses repeat, pause the lane builds (`kill -STOP`, then
  `-CONT`) for the bench's ten seconds.

- The dogfood pilot's builder daemon (theseus-14s) also works in a worktree. For the machine it counts as a lane
  (its own target dir, niced), though it is not an OpenClaw subagent.
- **Three rules every lane follows**, each from a conflict in §6:
  - store schema versions are assigned when a step lands on `main`, never in a design;
  - shared files (`scripts/gate.sh`, `deny.toml`, the web app's `App.tsx` and `protocol.ts`, a dispatch `match`)
    change only in the join, apart from one line to register the lane's own module;
  - lanes join one at a time, in the spine's order.

## 2. The spine, in order

**76.5 slots in 78 rows.** A slot is one spine step: about an hour of agent time plus its review. A pure join
counts half. The blocks:

| Block | Roadmap steps | Slots | Lanes it joins |
|---|---|---|---|
| A. Stage 1's remainder | 4b to 8, plus review 2 | 11.5 | none |
| B. Stage 2: the operator's surfaces | 9 to 13 | 8 | cache, TUI, herdr |
| C. M4 Boundaries | 17 to 22 | 11 | sandbox, disclosure, ontology, installer |
| D. AWS (floats on Eddie) | 14 to 16, 40, and AWS in L1 | 7.5 | the three AWS lanes (P1 to P4) |
| E. M5 Judgment | 23 to 28 | 13.5 | judge |
| F. M6 Memory | 29 to 35 | 10 | exam, index, math |
| G. M7 Surface | 36 to 39, 41 to 45 | 15 | MCP, web tabs, voice |

- **A, B, and C run in this order.** E, F, and G follow C.
- **D floats.** It takes the spine's next slot once its lanes have landed and Eddie has given the go (§7). The
  spine never waits for it. It is drawn after C here.
- **Fillers.** If a lane or an answer is late, these need neither and keep the spine busy: 13a, 18a's L0 half,
  37a, 37b, and 38a (after T1b).
- In "Waits on", names are steps (9c, C2, the reader rule), not row numbers.

**A. Stage 1's remainder** (the chain, theseus-5r9)

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 0 | T1b: `wake.at` exempt from the hold, Discord `/trust`, interaction routing, the #theseus-test check | q4t, e89 | running since 16:08 | — |
| 1 | Fix batch 1, head: a continuation on the live profile, GLM's thinking sent to Anthropic, the retry that drops the job's result | kol | 1 | T1b |
| 2 | **Dogfood pilot**: a builder daemon (theseus-dev persona, Opus 5.5, its own worktree, #theseus-test) builds one small batch-1 item, kks or 0s4. Quality, dollars, time, and friction measured | 14s | 1.5 | kol; e89 |
| 3 | Fix batch 1, the rest: a kill during the first open, the crash test's tear, a cancel that leaves a planned call, and whichever of 0s4 and kks the pilot left | 0b8, 4x6, w98, 0s4 or kks | 2 | kol |
| 4 | Fix batch 2: the capped result's words, a post for an unbound place, spool redaction, listing tools narrow, shutdown lets the outbox settle, `⏹️ stopped` on a stopped call's line, the hold's words | 46v, l3m, l0d, 8ye, pfv, qiy (the stop line has no issue yet) | 3 | batch 1 |
| 5 | Batch C, part 3: the CLI's `run()`, a typed `GateRecord`, the Discord runtime | 0g4 (findings 11, 12, 19) | 1 | batch 2. Before 10a, which moves the same `main.rs` |
| 6 | The reader rule: nothing declared without its reader | wjy (bdn stays deferred) | 1 | batch C |
| 7 | Review 2's accepted proposals. A placeholder, sized when the review lands | 9co | 2 (a guess) | review 2's report |

- theseus-d64 moves out of batch 2 into 20a, where M4 builds it on labels (§6, conflict 9). Since theseus-b5cl
  (2026-10-03), 20a is dropped, and the integrity lane builds d64 by a job's `THESEUS_SESSION` instead.
- Candidates for batch 2, not counted: theseus-ur0 and theseus-ez3, P2 bugs from F4b's review.

**B. Stage 2: the operator's surfaces** ([stage2 design](stage2-operator-surfaces.md); theseus-in3, 7yx, l1l, n4m, yf1, ev1)

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 8 | 9a: `attention()` in the protocol, on every list; the web UI's and the CLI's pills | in3 | 1 | the reader rule |
| 9 | 9b: the kernel's observer, `execution.changed`, `executions.watch`, `theseus watch --all` | in3 | 1 | 9a. If the seed on the 10,000-session store passes 250 ms, theseus-lv2's index of open actions goes first (+1 slot) |
| 10 | 9c: `session.wait`, the backlog cap and `events.lost`, the web UI off its poll | in3 | 1 | 9b |
| 11 | 10a: the CLI's client library, out of `main.rs` (a lane by crate, spine by file) | 7yx | 1 | 9c; batch C |
| 12 | 12a: the first EDGE (`derived_from` on the report route), `node.reach`, `theseus reach` | n4m | 1 | the reader rule |
| 13 | 13a: the telemetry corrections | yf1 | 1 | nothing (a filler) |
| 14 | 13c: caching, part 2: two breakpoints, `cache_min_tokens`, `cache_ttl`. **Joins the cache lane (13b)** | ev1 | 1 | 13b |
| 15 | 10f: **joins the TUI lane**; `theseus tui`; the install recipe | 7yx | 0.5 | the TUI lane. Floats: lands when the lane passes review, about here or early in C |
| 16 | 11c: **joins the herdr lane**; the agents' operating notes | l1l | 0.5 | the herdr lane. Floats the same way |

**C. M4 Boundaries** ([M4 design](m4-boundaries.md); theseus-7ve, 3vu, 8kk)

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 17 | 17b: L1 for `proc.run`: the class choice, `[sandbox]`, the wrapper's L1 path, ~~the probe after serving~~ (removed by the sandbox trims, theseus-gyin, 2026-10-03: `theseusd check` runs the self-test on demand). **Joins 17a** | 7ve | 1 | the sandbox lane (17a) |
| 18 | 18a: cancellation verified per backend (the tree stop, the pid namespace, ~~the cgroup~~ (gone with the sandbox trims, theseus-gyin), `verified_by`) | hcc, 7ve | 1 | 17b; w98 (batch 1). Its L0 half can run earlier, as a filler |
| 19 | 18c: egress wired in; a result that connected out is external (since the sandbox trims, theseus-gyin, 2026-10-03: only one that reached a host beyond the operator's list). **Joins 18b** | 20f (its L1 half) | 1 | 17b; 18b |
| 20 | 18d: credential brokering under L1: the per-job socket, `cred.request`, and a `kind: aws` seam with no AWS code in it (removed by theseus-w5op, 2026-10-03: an L1 job takes its grants at launch) | 7ve | 1 | 17b; 18c |
| 21 | ~~19a: labels on nodes, the audience, the compile filter with placeholders~~ **Replaced by the place rule** (theseus-nbsh; Eddie, 2026-10-03, the cut-list's Tier 2): every place is private or shared, a shared place gets the public tools alone, and the owner publishes into it. 19a's labels were built on 2026-10-02 and removed | 7ve, nbsh | 1 | T1 and F4a (done) |
| 22 | ~~19b's join: the disclosure simulator, and its short run in the gate~~ Built on 2026-10-02, and removed with the labels (theseus-nbsh) | 7ve | 0.5 | the disclosure lane |
| 23 | ~~19c: graduation, and `may_leave` in the outbox~~ Built on 2026-10-02, and removed with the labels: graduation is the owner's publish (theseus-nbsh) | 3vu | 1 | 19a |
| 24 | ~~20a: integrity by labels (T1's hold becomes the latch), origin `external`, `external_programs`; d64 built here~~ **Replaced by the integrity lane** (theseus-b5cl; Eddie, 2026-10-03, the cut-list's Tier 1.1): T1's latch stays as it is, per session, fed by DD5's own `external` marker. The lane adds its two cheap pieces: `[policy] external_programs` (`["gh"]` by default), whose `proc.run` output is outside text, and a job's session, `THESEUS_SESSION`, which the CLI sends as `opened_from`, so a session that a holding session's job opens or sends a turn to holds it too (d64, built here). No labels feed the latch, and there is no `external` origin | b5cl, d64 | 0 (a lane) | — |
| 25 | ~~20b: file hashes and fomites (`via: file`)~~ **Dropped** (theseus-b5cl; Eddie, 2026-10-03), with the Advisory (theseus-3vu's quarantine levels). Laundering through files is Jev's: `security.v1` (row 39) | — | 0 | — |
| 26 | 21b: the ontology wired in: records, the snapshot, the compile walk, the CLI, the ~~Observatory~~ cockpit (the cut-list's 6.4). **Joins 21a**. **Done 2026-10-04** (theseus-8kk.1; Part III Item 100; store format 7); the cockpit's view is 21c | 8kk | 1 | the ontology lane; ~~19a~~ (its labels were removed, theseus-nbsh) |
| 27 | 21c's join: the ~~web UI's~~ cockpit's Ontology view (the cut-list's 6.4: the cockpit replaces the Observatory). **Done 2026-10-04** (theseus-8kk.2; Part III Item 111) | 8kk | 0.5 | 21c (lane) |
| 28 | 22b: the job host, `RemoteLauncher`, `[control_plane]`. **Joins 22a**. _Moves after v1 (Eddie, 2026-10-03 15:26, taking the recommendation)._ | 7ve | 1 | the installer lane; 18a |

**D. AWS** ([AWS design](aws-toolset.md); theseus-mgw). Floats on Eddie's go; drawn here after C.

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 29 | C1 = 14a: the bound account; `aws.call` for reads, `aws.describe`, `aws.whoami`, `aws.s3.list`. **Joins P1 and P2** | mgw | 1 | P1, P2; the SCP conversation (renewed 16:18; its default holds until he answers) |
| 30 | C2 = 14b: stacks, `theseus aws bootstrap`, the owner role, writes, the budget. **Joins P3 and P4**. **Done 2026-10-03** (theseus-nyzn; Eddie's go-ahead at 11:24, with a cap under $1 a month: the lean posture), joined and installed; Eddie cleared the bootstrap's apply at 14:20, lean | mgw | 1 | C1; P3, P4; **Eddie's go-ahead for the first writes to his account** |
| 31 | C3 = 14c: the curated tools, the reaper in report mode, AWS text marked external (by DD5's own `external` marker, as a fetch is: 20a was dropped, theseus-b5cl). **Done 2026-10-04** (theseus-mgw.5, with theseus-9p40's `confirm-alerts` and the daily CloudTrail cross-check; Part III Item 97) | mgw | 1 | C2 |
| 32 | 15: the durability tender: WAL segments **and `blobs/`** to S3, index rows to DynamoDB, on the index lane's WAL follower. **Done 2026-10-04** (theseus-mgw.7; Part III Item 108): in the daemon, through the follower crate, with the open segment's tails | mgw | 1 | C2; the WAL follower (§6, conflict 3) |
| 33 | 16: ~~`theseus restore --from s3://…`~~ `theseusd restore --from s3://…`. **Done 2026-10-04** (theseus-mgw.10; Part III Item 123) | mgw | 1 | 15 |
| 34 | 18e: the `aws` grant under L1: an L1 job's AWS session, granted at its launch as any broker grant is (theseus-w5op; 18d's socket is gone). **Done 2026-10-03** (theseus-mgw.8; Part III Item 94): `~/.aws` hidden in every L1 view, and AWS reached through `[sandbox] egress` or the call's named hosts | mgw | 0.5 | w5op; C2 |
| 35 | 40, part 1: the hand role and image, `aws.hands.run` on Lambda and Fargate, the SQS poller. **Done 2026-10-04** (theseus-mgw.6; Part III Item 107): the role is `theseusd hand` | mgw | 1 | C2; P4's hands stacks |
| 36 | 40, part 2: cancellation per backend, the TTL reaper, budget reservations, the grid; the `kill -9` prove. **Done 2026-10-04** (theseus-mgw.11; Part III Item 116) | mgw | 1 | 40, part 1 |

- The AWS design sizes step 40 as one slice. Its wire list reads as two, so this plan counts two. _(A third piece came beside them, with no row of its own: step 40's network, hands in an existing VPC and never a NAT of their own (theseus-mgw.9, on Eddie's condition of 2026-10-03 23:24), built 2026-10-04: Part III Item 134. Its live check waits for the owner.)_

**E. M5 Judgment** ([M5 design](m5-judgment.md); theseus-0j2, vug)

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 37 | 23a: the wire-in: `[judge]`, `JudgeService`, the sink, the shadow budget, `loop.v1` in shadow. **Joins L1 and L2**. **Done 2026-10-04** (theseus-0j2.1; Part III Item 105), with the turn bench's quiet config turning the judge off (theseus-0j2.3) | 0j2 | 1 | the judge lane; block C (decision 16: Jev after M4), not D (§6, conflict 11) |
| 38 | 23b: the surfaces: trace marks, `judge` spans, `judge.list` and `get`, the Observatory's Judgment section. **Done 2026-10-04** (theseus-0j2.4; Part III Item 119), the section in the cockpit | 0j2 | 1 | 23a |
| 39 | 24: `security.v1` in shadow at the gate; T1's floor tests unchanged. It is the integrity path for text laundered through files, since 20b was dropped (theseus-b5cl): "If it's failing, we boost its context for good classification" (Eddie, 2026-10-03). **Done 2026-10-04** in shadow, `security.v1` with `security.v3` (theseus-0j2.5; Part III Item 120) | 0j2 | 1 | 23a |
| 40 | 25a: `classify.v1` and `role.v1` at inbound. **Done 2026-10-04** (theseus-0j2.6; Part III Item 121) | 0j2 | 1 | 23a |
| 41 | 25b: CONTINUE's candidate signals, `continue.v1` in shadow. **Done 2026-10-04** (theseus-0j2.7; Part III Item 122) | 0j2 | 1 | 23a |
| 42 | 25c: the learning ledger, the nightly report tender, holdouts. **Done 2026-10-04** (theseus-0j2.9; Part III Item 129) | 0j2 | 1 | 24, 25a, 25b |
| 43 | 25d: replay, audit, and backfill (backfill only after consent). **Done 2026-10-04** (theseus-0j2.14; Part III Item 144) | 0j2 | 1 | 25c |
| 44 | 26a: the ladder: `pack.mode`, arms, promote, rollback. **M6 uses the same arms** (memory's canary keeps its own hash for now). **Done 2026-10-04**, with `route.v1`, `rerank.v1` and `security.v3`'s notices on it (theseus-0j2.15, theseus-9j7x; Part III Item 145) | 0j2 | 1 | 25c |
| 45 | 26b: JUDGE_STOP live for tasks, under canary | 0j2 | 1 | 26a |
| 46 | 26c: the roles table, `role.v1` under canary | 0j2 | 1 | 26a; 25a |
| 47 | 27: the arrangement on `task.create`: references, refusal, the fidelity check. **Done 2026-10-04** (theseus-vug.2; Part III Item 113) | vug, vmh | 1 | 23a |
| 48 | 28a: independence: `check_of`, the exclusion set, the basis. **Done 2026-10-04** (theseus-vug.3; Part III Item 132; store format 14) | vug | 1 | 27 |
| 49 | 28b: `categorize.v1` in shadow; `tasks.parked` in health. **Done 2026-10-04** (theseus-vug.1; Part III Item 126) | vug | 1 | 21b; 23a |
| 50 | L3's join: `theseus judge prove`. **Done 2026-10-05** (theseus-0j2.18; Part III Item 179): the prove's records built from the ledger, one per finished task, the generator run over them, and classify.v1 held against the model's own `task_create`; it reads "insufficient" until 26b's canary has run | 0j2 | 0.5 | L3 (lane) |

**F. M6 Memory** ([M6 design](m6-memory.md); theseus-6fn, 3nk)

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 51 | 29b's wire-in: the `Tender` child kind, the spawn after serving, health's `index`, `theseus index status` and `search`. **Joins the index lane** | 6fn | 0.5 | the index lane (29b's crate) |
| 52 | 30a: `theseus-memory`'s trait and baseline; the recall step, in shadow; `[memory]`; `memory.search`. **Joins the math lane** (the crate's first code). **Done 2026-10-04** (theseus-6fn.1; Part III Item 99) | 6fn | 1 | 29b; 19a |
| 53 | 30b: the `Recall` node; `derived_from` EDGEs in 12a's convention; the `BudgetReport`; canary and live on 26a's arms. **Done 2026-10-04** (theseus-6fn.2; Part III Item 112) | 6fn, 3nk | 1 | 30a; 12a; 26a |
| 54 | 30c: compaction roots, `context_overage`, the assembled strategy (what lets M5's CONTINUE act). **Done 2026-10-04** (theseus-6fn.4; Part III Item 131; store format 13), the summary on the session's own model by default (Eddie's decision 3) | 6fn | 1 | 30b |
| 55 | 34b's wire-in: ~~`turn.submit`'s `memory_arm`~~ the exam's scratch daemon's `[memory] arm`, one daemon per arm (the cut-list's 6.2, Part III Item 80). **Joins the exam lane**; the first honest report. **Done 2026-10-04** (theseus-6fn.5; Part III Item 124), and the first honest report run on GLM the same day | 6fn | 0.5 | 34b (lane) |
| 56 | 31a: the memory pass, attribution, `memory.v1` in shadow. **29c joins here**, if not before. **Done 2026-10-04** (theseus-6fn.6, with Eddie's decision 10's three fixes; Part III Item 136) | 6fn | 1 | 30b; 29c; 23a |
| 57 | 31b: consolidation, `Synthesis` nodes, the `+synthesis` arm | 6fn | 1 | 31a |
| 58 | 32a's wire-in: FSRS-6's retention projection, the `+retention` arm, on the math lane's code | 6fn | 0.5 | 31a |
| 59 | 32b's wire-in: activation's adjacency projection, the `+activation` arm | 6fn | 0.5 | 31a; 12a |
| 60 | 32c: the `+rerank` arm, through M5's client. **Done 2026-10-04** in shadow (theseus-6fn.3; Part III Item 128) | 6fn | 0.5 | 23a |
| 61 | 33: tiering: stubs, the bounded heat cache | 6fn | 1 | 30c |
| 62 | 35a: situations, the precedence line, testimony, volatile values as of a time | 3nk | 1 | 30c |
| 63 | 35b: lessons | 3nk | 0.5 | 30b; 35a |

- 34c, the full sweep and the second report, is a lane at the end (§3). Its spec fold is Tabitha's.

**G. M7 Surface** ([M7 design](m7-surface.md); theseus-ext)

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| 64 | 37a: the repeating wake (`every`, `days`, `until`). **Done 2026-10-03** (theseus-d4pt; Part III Item 84; store format 5) | ext | 1 | T1b. A filler: can run earlier |
| 65 | 37b: tasks set one-shot wakes. **Done 2026-10-04** (theseus-7kg; Part III Item 98) | 7kg | 1 | 37a |
| 66 | 36b: MCP tools in turns: the `&str` change, `McpBoard`, `[mcp.servers]`, `theseus-sim fake-mcp`; servers in L1. **Joins 36a**. **Done 2026-10-04** (theseus-ext.1; Part III Item 106), servers at L0 until 43a | ext | 1.5 (M7's biggest) | the MCP lane (36a); 17b |
| 67 | 36c: MCP prompts: `/prompt`, `theseus prompt`, the web picker. **Done 2026-10-04** (theseus-ext.4; Part III Item 115) | ext | 1 | 36b |
| 68 | 38a: bindings format 2: many guilds, per-place ceilings. **Done 2026-10-04** (theseus-ext.3; Part III Item 117) | ext | 1 | e89 (T1b) |
| 69 | 38b: gliding: `channel.post` and `channel.read`, on the place rule. _19a's labels were removed with the place rule (Part III Item 76), and Eddie chose the redesign on 2026-10-04: into a private place a glide always may, and a read from a shared place is outside text; out of a private place, or between two shared places, it asks first, as `/publish` does (M7 §2.3, rewritten; theseus-ypy0)._ **Done 2026-10-04** (21683c9c; Part III Item 140). | ext | 1 | 38a |
| 70 | 39a: the `TASK` record kind, three layers, CAS, the tools; 27's arrangement kept. **Done 2026-10-04** (theseus-ext.6; Part III Item 125; store format 12) | ext | 1 | 37b; 27 |
| 71 | 39b: claim leases, the board, `/tasks`, the web task graph | ext | 1 | 39a |
| 72 | 41b: the MCP server's wire-in: `[mcp_server]`, `Surface::Mcp`. **Joins 41a**. **Done 2026-10-04** (theseus-ext.2; Part III Item 110) | ext | 1 | 41a (lane); 9c; d64 (a job's session and `opened_from`, in the integrity lane, theseus-b5cl) |
| 73 | 42a: `budget.list`, `policy.explain`, `theseus budgets`. **Done 2026-10-04** (theseus-ext.7; Part III Item 130) | ext | 1 | 36b; 38a |
| 74 | 42b's join: the Budgets, Ledger, and Policy tabs | ext | 0.5 | 42b (lane) |
| 75 | 43a: `extend.propose`: freeze, start in L1, test, the ack card. **Done 2026-10-04** (theseus-ext.5; Part III Item 118) | ext | 1 | 36b; 17b |
| 76 | 43b: load on ack, restart, revoke, `/extensions`. **Done 2026-10-04** (theseus-ext.8; Part III Item 133) | ext | 1 | 43a |
| 77 | 44b: the voice wire-in: `/join`, utterances into turns. **Joins 44a**. **Done 2026-10-03** (theseus-drrs; Part III Item 83): a voice channel is a place of its own | ext | 1 | the voice lane; 38a; a test voice channel for its live check (Eddie) |
| 78 | 45b: speech as spend. **Joins 45a**. **Done 2026-10-03** (theseus-drrs; Part III Item 83), with 45a's Deepgram | ext | 1 | 45a (Eddie's providers and keys); 44b; M5's latency table |

- M7 puts 37a and 37b before 36b, against the roadmap's numbering: they are small, and need only T1b.
- Store schema bumps (ACTION in 18a and 18d, NODE and COMPILATION in 19a, 21b, and 30b, EXECUTION in 37a) take
  the next free version when they land, each with its reader and an old-layout test, and a snapshot of Eddie's
  store before the install (F4a's rule).

## 3. The lanes

**17 lanes, 32.5 lane-steps**, grouped so that each lane is one worktree and one crate (or directory). "Row" is
the spine row in §2 where the lane joins. Weight: **H** is a heavy Rust build, **L** a light lane (YAML, web,
docs, or a short spike).

**Can start now, with no answer from Eddie** (in the order §5 gives)

| Lane | Steps, in order | Depends on | Joins (row) | W |
|---|---|---|---|---|
| **exam** (M6) | 34a: the memory exam (40 items, half held out) and the headroom test, `none` against `oracle` → later 34b, the harness over the real pipeline → 34c, the full sweep and the second report | 34a: nothing. 34b: 30b. 34c: all of M6 | 34b's wire-in (55); 34c ends M6 | H |
| **sandbox** (M4) | 17a: `theseus-sandbox` (namespaces, the view, seccomp, the init; one contract test per clause) → 18b: the egress proxy, in the same crate | nothing; 18b follows 17a's API | 17b (17), 18c (19) | H |
| **judge** (M5) | L1: `theseus-judge` (client, breaker, bands, batching, the fake) and `theseus-sim jev-probe` → L2: the six packs and `learn.rs` (calibration, holdouts, arms, rollback rules) → later L3: `theseus judge prove` (_L3's generator built 2026-10-04 as `theseus-judge prove`, Part III Item 101; its join, the CLI's command over the ledger, is row 50_) | nothing (synthetic states only, so no consent needed); L3 after L2 and 26b's data | 23a (37); L3's join (50) | H |
| **cache** (stage2) | 13b: caching, part 1: measure first (the Observatory's figures, the byte-identical header test, the Z.ai probe, the 1-hour TTL's break-even from Eddie's ledger) | nothing | 13c (14) | L |
| **voice** (M7) | 44a's spike (songbird, twilight 0.17, DAVE, a static libopus: a verdict) → 44a's engine, `theseus-voice` (the seam, the pipeline, stand-ins) → later 45a: the chosen speech providers | spike: nothing. The engine's live check: a test voice channel. 45a: Eddie's providers and keys | 44b (77), 45b (78) | L, then H |
| **index** (M6) | 29a's spike (candle against tract for Nomic v1.5, static musl: a verdict) → 29b: `theseus-index` (the WAL follower, the extractor, tantivy, the tender's socket, `index.query`) → 29c: embeddings (the engine 29a picked, int8 flat scan, rank fusion) | nothing. The weights (about 275 MB) are fetched once, outside a build session (the provenance trap) | 29b's wire-in (51); 29c in 31a (56) | H |
| **math** (M6) | 32a and 32b's math: FSRS-6 and spreading activation as pure functions in `theseus-memory`, from the published algorithms (Vestige is AGPL: never its source) | nothing | 30a (52), as the crate's first code; used by 32a and 32b's wire-ins | L |
| **ontology** (M4) | 21a: `theseus-ontology` (the kinds table, validation, `chain` and `intent_line`, the seed rows) → later 21c: the web UI's Ontology view | 21a: nothing. 21c: 21b's protocol | 21b (26); 21c's join (27) | H, then L |
| **installer** (M4) | 22a: `theseusd install` (plan, `--apply`, `--check`, `--user`, `--separate`; a new module in `theseusd`) and the container test script | nothing. Its container check uses Docker (default: yes, with limits; §7) | 22b (28) | H |
| **MCP** (M7) | 36a: `theseus-mcp`, the client (stdio, streamable HTTP) and a fake server → 41a: the server side (`/mcp`, the key, Origin, a rate limit, a fake core) | nothing; 41a after 36a's types settle | 36b (66), 41b (72) | H |

**Start after a spine step lands**

| Lane | Steps, in order | Depends on | Joins (row) | W |
|---|---|---|---|---|
| **TUI** (stage2) | 10b: the crate, the sidebar and trees, reconnecting → 10c: the detail pane, the input line, stop and cancel → 10d: the attention queue, jump-to-next, inline approve, trust, and decline → 10e: done until seen, notices, the title count | 10a (11) | 10f (15) | H |
| **herdr** (stage2) | 11a: `theseus watch --interactive`, the herdr reporter → 11b: `theseus herdr sync`, and a real herdr built from the reviewed clone | 9b and 10a; Zig 0.16 for 11b (installed with linuxbrew by default) | 11c (16) | H |
| **disclosure** (M4) | 19b: `theseus-sim disclosure`, with a planted bug that must fail | 19a (21) | 19b's join (22) | H |
| **web tabs** (M7) | 42b: the Budgets, Ledger, and Policy tabs in `web/` | 42a (73) | 42b's join (74) | L |

**Start after Eddie says go** (the AWS design's parallel slices; all offline: no account writes, and only
read-only live calls)

| Lane | Steps, in order | Depends on | Joins (row) | W |
|---|---|---|---|---|
| **aws-client** | P1: `theseus-aws-catalog`, from the local AWS CLI's botocore models (no network fetch) → P2: `theseus-aws`, the client (six protocols, SigV4, endpoints, retries, pagination) | Eddie's go | C1 (29) | H |
| **aws-guard** | P3: `theseus-aws-guard` (`guardrails.toml`, the evaluator, the template scanner, the guard and SCP generators) | Eddie's go; its catalog test once P1 lands | C2 (30) | H |
| **aws-infra** | P4: `infra/aws/`, the foundation, posture, and hands CloudFormation templates (`cfn-lint`, `ValidateTemplate`) | Eddie's go | C2 (30) | L |

- **The lanes are not the bottleneck.** 32.5 lane-steps at about 1.5 hours each (niced), three at a time, is
  about 16 hours of wall-clock, against about 100 for the spine. No spine step waits on a lane. A lane that
  starts today lands long before the step that consumes it. A lane that waits on a spine step (the TUI, 19b, 21c,
  34b, 42b, L3) joins a row or two later, while the spine moves on. The exceptions are Eddie's: AWS, and voice's
  live checks.
- **What the slack buys:** three lanes, not more, so reviews and memory stay safe; spikes first, since their
  answers reshape the plan (§4).

## 4. A timeline estimate

**This is an estimate.** It assumes the chain runs around the clock, as it has, starting when T1b lands (about
18:30 tonight), and it uses two rates:
- **1.3 hours a slot**, the brief's figure;
- **1.55 hours a slot**, measured: the chain landed 24 reviewed steps in the 37.2 hours from 2026-09-29 02:52
  to 2026-09-30 16:05. That pace already includes an abort, a WSL crash, and Eddie's conversations.

**The first finding is about size, not order.** The old plan counted 42 steps, one per roadmap line: about 55
hours. The six designs cut the same scope into hour-sized steps, and it comes to **about 106 slots** (§8): the
37 roadmap lines they cover became 94 steps. So the old plan counted about 40% of the work, whatever the order.

**When each block completes** (the spine's slots, cumulative)

| Block | Slots | Total | At 1.3 h | At 1.55 h | Bound by |
|---|---|---|---|---|---|
| A. Stage 1's remainder | 11.5 | 11.5 | Oct 1, 09:30 | Oct 1, 12:20 | the spine |
| B. Stage 2 | 8 | 19.5 | Oct 1, 19:50 | Oct 2, 00:45 | the spine; the TUI's join lands 1 to 2 hours later (lane-bound) |
| C. M4 | 11 | 30.5 | Oct 2, 10:10 | Oct 2, 17:45 | the spine |
| D. AWS | 7.5 | 38 | Oct 2, 20:00 | Oct 3, 05:25 | **Eddie**: with no go, D moves to the end, or out of v1 |
| E. M5 | 13.5 | 51.5 | Oct 3, 13:30 | Oct 4, 02:20 | the spine |
| F. M6 | 10 | 61.5 | Oct 4, 02:30 | Oct 4, 17:50 | the spine |
| G. M7 | 15 | 76.5 | Oct 4, 22:00 | Oct 5, 17:05 | the spine; voice's last two rows need Eddie's providers |

**Compared with the serial plans**

| Plan | Slots | At 1.3 h | At 1.55 h |
|---|---|---|---|
| The old serial plan (one step per roadmap line) | 42 | 55 h: Oct 3, 01:05 | — (it undercounted) |
| The designed scope, one step at a time | about 106 | 138 h: Oct 6, 12:20 | 164 h: Oct 7, 14:50 |
| **The designed scope, spine plus lanes** | **76.5** | **99 h: Oct 4, 22:00** | **119 h: Oct 5, 17:05** |

- **Lanes save about 28%**: 29.5 slots move off the critical path, about 1.6 to 1.9 days.
- **After the build**, v1's exit is Eddie's two-week soak, so v1 closes around **Oct 19 to 20**. M5's and M6's
  proves take their data from the soak.
- **The spine is the bottleneck, not the lanes** (§3). More lanes won't shorten it; less spine will.

**What could stretch it**
- **Eddie's answers.**
  - The AWS go (P1, P3, P4) and C2's first writes. Without them, D's 7.5 slots never enter, and v1's AWS part
    waits for him. Late, they cost nothing until D is all that's left. _(Answered 2026-10-03: the go at 11:24, with
    a cap under $1 a month, and C2's bootstrap at 14:20.)_
  - The SCP question: C1 follows the conversation (renewed 16:18). Its default holds, so it delays nothing unless
    he wants the SCPs in place first. _(Answered 2026-10-03 at 14:20: no SCPs yet; budgets and notices.)_
  - Voice: rows 77 and 78 wait for a test voice channel and his providers. Everything else finishes without them.
    _(2026-10-03: they leave v1 until he chooses speech providers, and `theseus-voice` is parked outside the
    workspace until then, theseus-o8nk; the spec's §2 and Part III Item 72. At 14:20 he chose Deepgram for both,
    so they return to v1.)_
  - TypeSafe consent: no build delay. Without it, M5's and M6's shadow data never starts, and their proves slip
    past the soak.
- **Review bandwidth.** About 38 hours of spine reviews (30 minutes each) and 8 of lane reviews (15 each), over
  about 100 to 120 hours. If reviews run 45 minutes, the spine drifts toward 1.55 to 1.75 hours a slot.
- **The 8-agent cap.** Not binding at 1 + 3 + 1. It binds only if design or review lanes are added, and then
  they take a lane's place.
- **Machine and session deaths.** A WSL crash, a gateway restart, or an abort of the DM session's run kills the
  spine and every lane at once: 1 to 3 hours each, going by this month.
- **Future fix batches: none are counted.** F4a's and F4b's reviews alone filed 9 follow-ups. Expect a batch
  every 10 to 15 steps: about 5 to 8 more slots.
- **Store schema bumps** (six steps in M4, M6, and M7, plus the first EDGE in 12a and the `TASK` kind in 39a)
  each need a snapshot of Eddie's store before the install: minutes each.

**What could shorten it**
- **M6's headroom test (34a, which can start today).** If `oracle` is about equal to `none`, memory isn't what
  these tasks lack, and M6 files about 8 of its steps: about 5 spine slots, 6 to 8 hours.
- **The DAVE spike (44a).** If songbird can't join Discord's encrypted voice, voice moves past v1: 2 spine
  slots, and the voice lane.
- **Stage 2's scope.** If herdr isn't in Eddie's daily stack, park 11b (and its join). If the web UI on his
  phone is enough, stop the TUI after 10d.
- **A second spine**, running D and G beside E and F once C has landed. It would cut the critical path by about
  a quarter, but it doubles merge risk and the review load, and a few of G's steps wait on E's (39a on 27). Not
  recommended until the pilot shows that more builders can be reviewed.

## 5. What starts now

**Before the first lane (Tabitha, once, a few minutes):**
- finish the review of the lane's design (the five phase designs' reviews started at 16:14 and 16:15);
- create `~/projects/theseus-wt/` and `~/.cache/theseus-target/`, and set `SCCACHE_CACHE_SIZE=40G`;
- write the lane recipe once (§1), so that every lane brief carries it by reference;
- file a Beads issue per lane under theseus-zaz, and claim it with `bd update <id> --claim` before the spawn.

**Start today, in this order. None needs an answer from Eddie.**

1. **exam: 34a, the memory exam and the headroom test** (heavy slot 1).
   - *Why first:* it is the cheapest step in the plan with the biggest lever. Its answer can file about 8 of M6's
     steps, or say that retrieval, not retention, is where M6 should spend (§4). It needs no M6 code.
   - *Its gate:* the exam's own tests (the fixture writer's store reads identically in an unmodified daemon; the
     check language; the statistics against hand-computed fixtures), and **the headroom report**: 40 items ×
     {`none`, `oracle`} × 3 runs on GLM, with a $30 cap. Tabitha reads the report the day it lands. The code
     joins later, in 34b's wire-in (row 55).
2. **sandbox: 17a, then 18b** (heavy slot 2).
   - *Why:* M4's first spine step (17b) consumes it. MCP's servers (36b) and self-extension (43) run in L1 too. Its
     contract tests retire the plan's biggest platform risk early, on this machine's WSL2 kernel (6.18), which
     updates itself.
   - *Its gate:* one contract test per clause of the M4 design's §7 (`harness = false`, so the test re-execs
     itself as the init). Then the 100-spawn micro-bench's p50 and p95, against the 25 ms start target. 18b adds
     the proxy's CONNECT, allowlist, and public-only resolver tests. Joins in 17b (row 17) and 18c (row 19).
3. **judge: L1, then L2** (heavy slot 3).
   - *Why:* M5's 13 spine steps open with 23a, which consumes both. L1's live check settles Jev's real latency,
     cost, and rate limits before Stage 4. It sends only synthetic states, so it needs no consent. M5's
     question 19 already defaults to starting early.
   - *Its gate:* round trips against fixtures of the verified shape, each malformed case, the bands at their
     edges, batching, the breaker, and each fake mode mapped to its error class. Live: `theseus-sim jev-probe`
     makes three real calls (Choice, Score, and Noul) on a synthetic state. It prints usage, cost at the catalog
     price, and latency, and reads the key from the vault at run time without printing it. L2 adds the six packs'
     loader rules, their golden states, the math on synthetic data, sticky arms, and each rollback rule, plus
     `jev-probe --pack` for each pack (about a tenth of a cent). Joins in 23a (row 37).
4. **The light slot: the voice spike (44a), then cache (13b).**
   - *The spike's why:* about an hour. Its answer decides whether voice is in v1 at all: can songbird speak
     DAVE, does it take twilight 0.17, and does libopus build static under musl within §9's 60 MB. *Its gate:* a
     verdict in a throwaway worktree. Nothing joins.
   - *13b's why:* 13c consumes it in about a day (row 14), and it measures before anything changes.
     *Its gate:* the header is byte-identical across two sessions, and across a session and its task; the web
     build; the Z.ai probe (two identical GLM calls: are cache reads reported?); and a read-only pass over a copy
     of Eddie's ledger for the 1-hour TTL's break-even. Joins in 13c.

**Then, as slots free, in this order:**
- **index** (29a's spike, then 29b's crate, then 29c): fetch the Nomic weights once, outside a build session,
  pinned by SHA-256.
- **ontology** (21a).
- **installer** (22a). Its container check uses Docker with the design's limits, unless Eddie says no.
- **MCP** (36a, then 41a).
- **math** (32a and 32b's math).
- **When 10a lands** (about Oct 1, 14:40 to 18:30): **TUI**, then **herdr**. They go ahead of any
  not-yet-started lane above, since Stage 2's joins come first.
- **When Eddie says go on AWS:** **aws-client** (P1, then P2) and **aws-guard** (P3) take the next two heavy
  slots, and **aws-infra** (P4) the light slot, ahead of everything not yet started. P1 builds from the local
  AWS CLI's botocore models, so no build session fetches anything.

**Every lane passes the same join gate**, beyond its own tests:
- rebased onto `main`, with `Cargo.lock` regenerated and its crate a workspace member;
- the whole gate in its own target dir: fmt, clippy with `-D warnings`, the tests, deny, the web build, and the
  lifecycle bench within §9;
- its live check, and Tabitha's light review for each step;
- at the join, the deep review: the live check through a scratch daemon over a copy of Eddie's store, the
  install, and the spec folded.

## 6. Conflicts found across the designs

Thirteen conflicts, each with a resolution this plan already takes. Items 1 to 11 are one thing planned twice, or
planned differently. Item 12 covers LANE steps that touch the same shared file. Item 13 is one piece of work
filed twice.

1. **NODE 3 and COMPILATION 3, claimed twice.** M4's 19a bumps NODE and COMPILATION to 3 (and 21b takes
   COMPILATION to 4). M6's 30b also claims NODE 3 (the `Recall` node) and COMPILATION 3 (the `BudgetReport`).
   - *Resolution:* a version is assigned on `main` when its step lands, never in a design. In this order 19a
     lands first, 21b takes COMPILATION 4, and 30b takes NODE 4 and COMPILATION 5. ACTION 3 and 4 (18a, 18d)
     and EXECUTION 3 (37a) don't collide today, but follow the same rule. Every brief says "the next free
     version", with its reader, an old-layout test, and a snapshot of Eddie's store before the install.
2. **The EDGE record, resumed by three designs.** Stage 2's 12a writes the first EDGE since theseus-hco
   (`derived_from` on the report route, with a reverse scope). M4's 20a carries integrity labels on "the same
   edge". M6's 30b writes recall edges, plans `same_entity`, `supersedes`, and `contradicts`, and bumps EDGE to 2
   if stage2's payload differs. M6 also names the reverse scope two ways (`in:<to>` in its §2.8, `in:<source>`
   in 30b's tests).
   - *Resolution:* 12a defines the payload and the reverse scope once, reviewed against 20a's and 30b's needs
     (the type, the generations, the compilation, a label to carry). A later edge type is a new `type` value in
     the same convention, and a new field is a serde default, not a new schema.
3. **The WAL follower, built twice.** AWS's step 15 ships WAL segments to S3. M6's 29b builds a WAL follower
   "unless M4's step 15 built it first" (step 15 is in the AWS design, not M4's).
   - *Resolution:* build it once, in the index lane, which starts today (step 15 waits on Eddie). Keep it free of
     tantivy and candle (a module of its own, or a small crate), so step 15 reuses it without the search engine.
4. **`blobs/` left out of the durability tender.** M5's risks ask the tender to ship `blobs/` (images already
   live there, and judgments add state blobs). The AWS design's step 15 ships only WAL segments and index rows.
   - *Resolution:* step 15's brief includes `blobs/`, each blob uploaded once (they are deduplicated by hash),
     and step 16 restores them.
5. **Sticky arms and a canary ladder, built twice.** M5 builds them (L2's sticky, monotone arms; 26a's
   `pack.mode`, promote, and rollback). M6's 30b needs canary and live modes with sticky arms, and falls back to
   "a minimal sticky assignment" without M5's step 26.
   - *Resolution:* in this order 26a lands before 30b, so M6 uses M5's arms and mode table, and M5's nightly
     report (25c). The fallback is never built.
6. **`task.create`, changed by two designs.** M5's 27 adds the arrangement (references, refusal, the fidelity
   check) to DD7's tool. M7's 39a turns it into one tool that records every task, and opens a session only with
   `brief` (M7's question 12).
   - *Resolution:* 27 lands first. 39a extends the same tool and keeps 27's refusal cases in its tests. Eddie
     gets one heads-up about his daily `task.create` flow, at 27's install.
7. **The audience rule, built twice.** M7's 38b builds a subset rule for gliding, to be "later replaced" by
   M4's labels. M6 falls back to owner-only recall until 19a exists.
   - *Resolution:* in this order 19a lands before both. 38b uses 19a's audience rule (`covers`) directly, and
     neither interim rule is built. _Since 2026-10-04: the place rule removed 19a's labels, and 38b takes the
     place rule instead (theseus-ypy0)._
8. **Three new sources of external text.** AWS's C3 marks AWS data-plane reads external (its question 4). M7's
   36b marks MCP results external. M4's 20a moves T1's hold onto integrity labels, with an `external` origin.
   - *Resolution:* after 20a, a source sets the `external` origin label rather than calling T1's hold directly,
     and C3's and 36b's briefs say so. If Eddie's go brings C3 in before 20a, 20a's brief carries AWS reads over.
   - *Since theseus-b5cl (Eddie, 2026-10-03):* 20a is dropped. A source marks its results with DD5's `external`,
     as `http.fetch` does, and T1's hold follows. C3's and 36b's briefs say so.
9. **theseus-d64, planned twice.** The roadmap puts it in fix batch 2. M4 builds it in 20a, on labels (a
   session opened from a job's process takes the job session's hold, through J1's trace). M7's 41b needs it.
   - *Resolution:* build it once, in 20a, and drop it from batch 2. The gap is P3, and stays open about a day
     longer.
   - *Since theseus-b5cl (Eddie, 2026-10-03):* built in the integrity lane instead. Every job carries its session
     in `THESEUS_SESSION`, and the CLI sends it as `opened_from`. A job can strip its environment, so this is a
     light guard under default trust, not a boundary.
10. **AWS credentials inside L1, designed twice.** The AWS design gives an L1 job an AWS session through the
    `aws` program grant, in steps 17 and 18. M4 routes AWS credentials through 18d's per-job socket
    (`kind: aws`).
    - *Resolution:* 18d builds the socket and a `kind: aws` seam with no AWS code, so M4 never waits on Eddie's
      AWS go. A half-slot step, 18e, puts the AWS job session behind it once both 18d and C2 are in.
    - *Since theseus-w5op (Eddie, 2026-10-03):* 18d's socket is gone, and an L1 job takes its grants at launch,
      as an L0 job does. 18e gives the AWS job session the same way, as the AWS design's program grant first said.
11. **M5 waits for "Stage 3 done".** M5's 23a lists Stage 3 (the roadmap's order, decision 16). Stage 3 holds
    AWS, which waits on Eddie, so Stage 4 would idle behind his answer.
    - *Resolution:* M5's spine follows M4's, not AWS's. Decision 16 ("Jev late") still holds: Jev acts in the
      product only after M4's boundaries.
12. **LANE steps that touch the same shared file.**
    - **`crates/theseus-sim`** (one dispatch in a 693-line `main.rs`): `exam` and `ablate` (M6), `jev-probe` and
      `fake-jev` (M5), and `disclosure` (M4). *Resolution:* each subcommand in its own module file with one
      dispatch arm. The second lane to join rebases a one-line conflict.
    - **`web/src`** (`App.tsx`'s tabs, `protocol.ts`'s types, `Observatory.tsx`): the cache figures (13b), the
      Ontology view (21c), M6's Memory panel, and M7's tabs (42b), beside spine steps (9c, 23b, C1, C3).
      *Resolution:* each view in its own `.tsx`. `App.tsx` and `protocol.ts` change only in a join or a spine
      step (the types come from the step that adds the method). Web lanes join one at a time. `web/dist` isn't
      committed, so builds never conflict.
    - **The daemon's role dispatch** (`crates/theseusd/src/main.rs`): the installer lane (22a) is a module in
      `theseusd`, while spine steps add the `job-sandbox`, `job-host`, `tender index`, and `hand` roles.
      *Resolution:* 22a adds its own file and one arm, and rebases after 17b.
    - **`scripts/gate.sh`**: the disclosure lane (19b) adds a short run to the gate. *Resolution:* in its join,
      never on the lane's branch.
    - **The root `Cargo.toml`, `Cargo.lock`, and `deny.toml`**: about 11 new crates each add a member line and
      lockfile entries, and some bring licences deny must accept (tantivy, candle, seccompiler, the AWS signer).
      *Resolution:* the member line on the lane; the lockfile regenerated at the join, never hand-merged; deny's
      changes reviewed in the join.
    - **The CLI's `main.rs`**: stage2 already moved 10a onto the spine for this reason. Batch C's finding 11
      rewrites the same file's `run()`, so batch C part 3 lands first (row 5, before row 11).
    - **`theseus-memory`**: the math lane creates the crate, and 30a adds its trait. *Resolution:* the math lane
      joins in 30a, as the crate's first code.
13. **Stage 2's seed cost is theseus-lv2.** Stage 2's first risk (the seed decodes every action ever written)
    proposes "an index of open actions". F4a's follow-up theseus-lv2 (P2) is that index.
    - *Resolution:* one piece of work. If 9b's seed on the 10,000-session store passes 250 ms, lv2 goes first.

## 7. Eddie's answers still needed

Collected from all six designs and the roadmap, and deduplicated. **Five of them block something.** None stops
the spine: it always has other work, every step takes its default, and Eddie can overturn a default later.

**These block something**
1. **A "go" for the AWS offline lanes P1, P3, and P4**: the catalog, the guardrails, and the templates. They
   make no account writes, and only read-only calls.
   - *From:* the AWS design's §5 and review note E. Asked at 16:18.
   - *What waits:* the three AWS lanes, and so all of block D.
   - *Without an answer:* they don't start, and every other lane goes first.
2. **A go-ahead for the first writes to the Home account.** C2's `theseus aws bootstrap` applies the foundation
   and posture stacks.
   - *What waits:* C2's apply, and C3, 15, 16, 18e, and 40 behind it.
   - *Without an answer:* D stops at C1, which only reads.
3. **Consent to send session content to TypeSafe (Jev).** That means judgment states and node text, scrubbed of
   secrets, his employer's work included, which is a SOC2 vendor question. With it: whose TypeSafe account this is, his or
   his employer's.
   - *From:* M5 items 1 and 8, and M6 item 1.
   - *What waits:* the judge on his daemon and on copies of his store; the backfill; M6's Jev halves (31a's
     labels, 31b's checks, 32c's rerank). **Not the build.**
   - *Without an answer:* the judge stays `enabled = false` in his note, and nothing of his reaches TypeSafe.
     Everything builds and checks on the fake and on scratch content. M5's and M6's proves slip past the soak.
4. **Speech providers and keys.** Recommended: Deepgram for both directions, with one key.
   - *From:* M7 item 1, and the roadmap's item 4.
   - *What waits:* 45a and 45b.
   - *Without an answer:* the stand-ins. Voice comes last, as he asked.
5. **A private test voice channel** in the operator's Discord server, where the bot may Connect and Speak. Later, his voice for the
   receive half.
   - *From:* M7 items 2 and 3.
   - *What waits:* 44's and 45's live checks.
   - *Without an answer:* voice is proved through the seam only.

**The SCP question** (AWS question 1, renewed 16:18) blocks nothing by default. C1 runs on the guards and the
floor, which are soft inside the account, plus the posture stack's alarms to his email. The SCP JSON is
generated and waits for his management account. If he wants the limits hard before the first write, it joins
item 2.

**These block nothing: the default holds**
- **Vault-note lines (standing).** The service account is read-only, so he pastes them, after the install that
  first reads them. Each step's report names its lines, and the queue file collects them:
  - Brave's `[secrets]` line (resent 14:46, still missing);
  - `[judge] enabled = true`, after item 3;
  - `[memory] mode = "canary"`, after M6's first report (34b);
  - `[sandbox]` (`l1_argv`, `ro_paths`, egress hosts), when he wants L1 used;
  - `[mcp.servers.*]`, and `[mcp_server]` with its key, when he wants either;
  - `[kernel] min_repeat_minutes`, optional;
  - the speech catalog rows, with item 4.
- **AWS** (questions 2 to 13):
  - the full-admin `theseus-owner` role, as the owner's own hat → yes;
  - the root of trust → the vault's IAM user key, with a warning at 90 days;
  - text read from AWS is external, so the next acting call waits → yes;
  - the NAT gateway → lazy (on when a hand needs egress, off after an idle hour);
  - one dollar budget per session, for models and hands together → yes;
  - the month's budget → $300, and cost-bearing calls wait at 100%;
  - regions → `us-west-2`, plus `us-east-1`;
  - the operator's employer's accounts → not bound;
  - long-lived credentials → always ask;
  - detective controls → on (GuardDuty too); AWS Config off;
  - another project's resources → never reaped, and a write outside Theseus's inventory posts a notice;
  - **where the IaC lives** → a new private repository, `zeroaltitude/theseus-infra`, created at C2. That is
    outward-facing, so worth a word from him before C2.
- **Stage 2:**
  - Zig 0.16 → installed with linuxbrew before 11b (a heads-up);
  - is herdr in his daily stack? → built anyway; if not, 11b is parked;
  - where he runs the TUI, and how it tells him → the bell and the title's count;
  - answers from the TUI count as the CLI's → yes;
  - the 1-hour cache TTL → stay at 5 minutes until 13b's numbers;
  - an OTLP endpoint → none.
- **M4:**
  - L1's posture → the same as L0's;
  - T1's floor for L1 jobs → unchanged;
  - which jobs go to L1 → none by list; the model's `sandbox: true` alone;
  - latching L0 output by program → no (`external_programs = []`);
  - running his daemon as a systemd user service, for cgroup limits → not until he chooses; health says so;
  - the bot's Server Members intent → if it's off, a guild channel counts as public;
  - **Docker for 22a's and 22b's two-user test**, which is root-equivalent here, so asked, not assumed → a
    throwaway container, `--network none`, with read-only binaries and fake secrets;
  - separated mode for his own daemon → his choice, any time (it needs sudo);
  - the owner set for labels → the local surfaces and `[approval] trusted_users`;
  - secrets an L1 job may request → `github_token` alone, at `notify`;
  - noted, not asked: at L0, any job can use Docker. He may want `approve_argv = [["docker"]]`.
- **M5:**
  - may `security.v1` ever make a call wait → no: notices only;
  - `security.v1`'s promotion → a card for him, in the soak;
  - a learning channel → none; the web UI and the CLI carry it;
  - a heads-up at 27's install: the arrangement changes his daily `task.create`.
- **M6:**
  - moving his daemon from shadow to canary → one paste, after the first report;
  - a private exam from his own sessions → synthetic items only;
  - exam spend → a $30 cap per run, on GLM;
  - which of his sessions may recall each other → his owner-only surfaces;
  - the embedding weights → downloaded once (Nomic v1.5, about 275 MB, pinned);
  - memory science stays off until it earns its place → yes.
- **M7:**
  - the MCP server's key → the server stays off;
  - which MCP servers → the fake and a reference server;
  - more places → the DM and #theseus-test;
  - by default: a repeating wake waits in an external-text session; MCP results are external; acting calls from
    MCP wait; voice shares its text channel's session; extensions run only in L1.
- **Across M5 and M6:** a few labels a week in the web UI (judgments and recalls). The system's and the audit's
  labels stand in.
- **His own testing and the two-week soak:** last, as he said. They never block the chain.

**Already answered, for the record:**
- the Home account is `<home-account-id>`, it is not empty, and Theseus owns it;
- the IAM key is an admin key;
- #theseus-test exists (14:59), and it replaces a second bot;
- the command is `/trust`;
- the child cap is 8;
- the dogfood pilot is his idea (15:17).

## 8. Appendix: where the step counts come from

| Source | Roadmap lines | Designed steps (the design's own count) | Spine slots here | Lane-steps here | One at a time |
|---|---|---|---|---|---|
| Stage 1's remainder (the roadmap, plus review 2) | 5 (steps 5 to 8, and 5b) | — | 11.5 | 0 | 11.5 |
| Stage 2 (stage2 design) | 5 (9 to 13) | 16 (7 spine, 7 lane, 2 joins) | 8 | 7 | 14 |
| AWS (AWS design) | 4 (14 to 16, 40) | 10 (4 parallel, 3 chain, then 15, 16, 40) | 7.5 | 4 | 11.5 |
| M4 (M4 design) | 6 (17 to 22) | 16 (10 spine, 6 lane) | 11 | 6 | 16 |
| M5 (M5 design) | 6 (23 to 28) | 16 (13 spine, 3 lane) | 13.5 | 3 | 16 |
| M6 (M6 design) | 7 (29 to 35) | 17 (an 11-slot spine, 6 lane) | 10 | 7 | 17 |
| M7 (M7 design) | 9 (36 to 39, 41 to 45) | 19 (14 spine, 5 lane) | 15 | 5.5 | 20 |
| **Total** | **42** | **94, plus Stage 1's remainder** | **76.5** | **32.5** | **106** |

**How the slots were sized:**
- One step is one slot. A pure join, or a step its design calls small, is half a slot.
- A few judgment calls:
  - the dogfood pilot is 1.5, for its setup plus one build;
  - M7's 36b is 1.5, which its design calls M7's biggest spine step;
  - AWS's step 40 is split in two, and 18e (AWS in L1) is a half;
  - 44a (the spike and then the engine) is 1.5;
  - review 2 is a guess of 2 slots;
  - fix batch 1's rest is 2 slots for four items, and batch 2 is 3 slots for seven.
- "One at a time" is the spine plus the lanes, less the six pure joins (3 slots), which a serial plan wouldn't
  need.

**The rates:**
- 1.3 hours a slot is the brief's figure.
- 1.55 hours is measured. The queue file records 25 reviewed installs from 2026-09-29 02:52 to 2026-09-30
  16:05, which is 24 intervals in 37.2 hours.

**Machine facts** (measured 16:27): 16 cores; 23 GB of memory with 17 GB available; 478 GB of disk free; `main`'s
`target/` is 100 GB; sccache 21 MB at `~/.cargo/bin/sccache`; the gateway's child cap is 8. `main` is at 1353a83
(spec v0.62).

*— written by Tabitha/Claude*

<!-- REPORT COMPLETE -->
