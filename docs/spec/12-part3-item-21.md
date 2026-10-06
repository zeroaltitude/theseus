# The Ship of Theseus, chapter 12: Part III, A4's Items 21 to 47 ([index](README.md))
### Item 21. Fix batch 2, part 2: words that tell the truth (theseus-46v, theseus-l3m, theseus-qiy, theseus-4uw, theseus-8ye; 2026-10-01 07:46 to 11:14, three runs; 62070eb, 7891c4b, d28f9b3, 41c4c80, 8b27f6b, 76c1e2a, fa62294)

**Why.** This is the roadmap re-cut's row 4, the second half of fix batch 2. It fixes what Theseus *says* (to the
model, in health, on a Discord line, and in a listing) wherever it said something untrue, or nothing.
- A capped result said how much was cut, and never how to get it (theseus-46v). The "stored" claim itself had gone
  with wz2.
- A post for a place no longer bound stayed pending forever, and its age hid real delivery trouble (theseus-l3m).
- Three slips in the external-text hold (theseus-qiy):
  - a search's hold named the API's request, not the query;
  - health gave the hold's time in UTC, beside the reason's local time;
  - an approval's trust named the connection (`sock#32`), where `policy.trust` named the surface.
- A call a `/stop` killed read `❌ … cancelled`, a failure, though the operator asked for it (theseus-4uw).
- Listing tools cut without saying so: `fs.grep` stopped at its cap in silence (theseus-8ye, Appendix F).

**What exists.**
- **46v**: `Tool::rest(left_out)`, each tool's own way to get the rest:
  - `fs.read` names the rows, with the `offset` and `limit` that return them;
  - `fs.grep`, `fs.glob`, `fs.list`, `git.*`, and `text.diff` name their narrower call;
  - `proc.run` says to run it again printing less, or into a file read in ranges.

  `toolrun::cap` cuts on lines' edges and says `…[N lines (M characters) not shown: <rest>]…`. A job's output past
  the 4 MiB the runtime reads says so (§3.24).
- **l3m**: `Outbox::refuse_unbound` runs at the binding's start and at each courier wake. An unsettled post whose
  place has no lane is settled refused, "not bound here any more (<place>)", except a post to `discord:operator`
  (§3.16).
- **qiy** (§3.9):
  - `ExternalText.query` (session schema 5), from the search's result meta, and `ExternalText::what()` names it on
    every surface;
  - `since_local` on health's list and on a trust's result;
  - `Conn::answerer` labels every approval-like act with `Conn::actor`.
- **4uw** (§3.15):
  - `Action::stopped_by()`;
  - `meta.stopped_by` on every result a stop ended (a killed job, a declined wait, a call asked for as the stop
    landed), carried by `tool.ended`;
  - `ToolState::Stopped` on Discord, and `stopped by` in `theseus watch`, the history, and the web UI;
  - a call's Discord line is found in the turn that holds it, so an approved call's line follows it into the turn
    that runs it.
- **8ye**: `fs.glob`, `fs.grep`, and `fs.list` end every result with its scope and what it left out (§3.24's rule).

**How it is proven.**
- **Each part's tests** (`~/reports/theseus-fb2b/fb2b.md`, sections 2 to 6):
  - the cap's every byte accounted for;
  - a real turn's capped `fs.read` naming its rows;
  - two lives over one store for l3m (with the refusal disabled it fails);
  - a real turn's search hold through the fake search API;
  - an approval's trust over a CLI connection;
  - a schema-4 session record read and written back byte for byte;
  - the Discord lines for a stopped call: running, waiting, and approved in an earlier turn;
  - the killed job's late result against the real daemon (with the meta disabled, the core and daemon tests fail);
  - one elision-line test per listing tool.
- **Live, on a scratch daemon of the build:**
  - the 46v marker naming lines 203-1066 and the call that returns them;
  - the 8ye cap line;
  - the qiy hold naming the query, in local time in the confirm and in health, and `session.trusted` by `the CLI`;
  - the l3m refusal after a restart, with nothing sent;
  - the 4uw line on the fake Discord, `⏹️ \`proc.run\` sleep 30 · stopped by the CLI`, and a run on
    `#theseus-test`.

  The live checks found two bugs, both fixed in the follow-ups and checked again:
  - 8ye's cap line overclaimed "the rest of" a file the cap ended on;
  - 4uw's line stayed `👍 approved` for a call approved in an earlier turn.
- **Gates.** The gate was green at each commit: 1,121, 1,122, 1,125, 1,127, 1,130, and 1,131 tests. At the review
  (11:22, on `main` at fa62294) it was green again: 1,131 tests, lifecycle OK on its first run, clean shutdown
  p95 60.2 ms (budget 100), SIGKILL then restart p95 51.4 ms (budget 150).

**Divergence from the brief and the issues.**

| Brief | Built | Why | Keep? |
|---|---|---|---|
| 46v: say "stored" only when a `full_ref` exists | Nothing says stored; the parts are a tool with a range and one without | Since wz2 no result has a `full_ref` | Keep |
| l3m: refuse "when the bindings file changes" | At the binding's start, and at each courier wake | The file is read only at the start (theseus-ocwt) | Keep |
| qiy: `Conn::actor` for both trusts | At `Conn::answerer`, so answers, presses, and undos name the surface too | One labeler for every approval-like act; judgments read the surface, not the label | Keep |
| 4uw: the running call's line | Also the waiting call's line, the narrative, a call asked for as the stop landed, and an approved call's line in a later turn | Each is a call a `/stop` ended; the last was found live | Keep |
| 8ye: the scope line on a cut result | On every result | The walk's own filter is a scope a whole result has too, and an empty result misleads most | Keep |

**Known gaps.**
- The bindings file is read only at the binding's start, so a place removed live stays bound until the next start
  (theseus-ocwt).
- ~~A live check can't read back a posted Discord message: `discord.message.out` keeps its length, and edits write
  no row (theseus-qifw).~~ Built in Item 47, on the stand-in, which keeps every version of every message; on
  real Discord, theseus-w04x.
- No model-facing task or session listing exists yet. When one lands, it follows §3.24's rule, with its test.
- Build-chain tooling, not Theseus. Both were fixed the same day in `tools/theseus-quiet.sh` and the lane recipe:
  - the gate's helper paused a lane's `cargo` while it held cargo's package-cache lock (theseus-xfr1);
  - a gate's orphaned child kept the shared gate lock (theseus-e6xj: the command now runs with the lock's fd closed,
    and lanes take it with `flock -o`).

### Item 22. The `secfix` lane: the git tools stay under the roots, the web port serves only its own user, and the dev page's way in (theseus-bsc, theseus-3qf, theseus-zab; 2026-10-01 10:03 to 10:59, reviewed 11:10, merged after Item 21; adcf189, d1e17a0, 2c2ea86)

**Why.** These are three gaps the hardening lane left open (Item 16's known gaps).
- `git.diff` and `git.log` found their repository with `gix::discover`, which climbs above the roots. A root inside
  a monorepo, or a stray `~/.git` above a root, read the whole repository.
- The web UI's TCP port answered any local user's process, which can send any `Host` and `Origin`.
- H1's checks refused the Vite dev page, whose proxy passes the page's own headers.

**What exists.**
- **bsc** (§3.9):
  - `open()` finds the root that holds the path, discovers the repository, and checks its working tree against
    that root;
  - a root inside a larger repository is limited to its own part, as a pathspec, and says so on the first line;
  - a `core.worktree` outside the roots is refused;
  - every path the diff reads is checked first, in the working tree and in history, and skipped unread and counted
    when it is outside the roots or on the floor (`ToolCtx::floor`, the gate's floor paths).
- **3qf** (§3.14):
  - `theseus_core::peer::client_uid` reads the client socket's owner from `/proc/net/tcp` and `tcp6` (IPv4, IPv6,
    and the mapped forms), on the blocking pool, as each connection is accepted;
  - the web UI's listener (`OwnUser`) serves only the daemon's uid, and turns any other away with a 403 before
    reading the request;
  - health counts `refused_peer`, and the ledger has `web.refused` kind `peer` with the uid.
- **zab** (§3.14): `[web] dev_origin`, off by default, which only a loopback `http://` origin with a port passes.
  - While it's set, `/ws` serves that origin (through the proxy's `Host`, or straight to the UI's own address),
    counted and ledgered as `web.dev_origin`.
  - One WARN line at start says it's on.

**How it is proven.**
- **The tests.** Fourteen tests, each shown at the lane to fail when its fix is reverted
  (`~/reports/theseus-lane-secfix/secfix.md`). Among them:
  - a root inside a larger repository, a stray repository above a root, a floor file, and a `core.worktree`
    elsewhere;
  - the uid tables' fixtures (same uid, other uid, a missing row, a closed client, tcp6, both mapped forms);
  - the real tables;
  - the listener refusing another uid with a 403 and one refusal;
  - the real daemon on IPv4 and on `::1`;
  - the dev page served on `/ws` only, and only while set;
  - 5 good and 17 bad dev origins.
- **Live at the lane** (port 7436):
  - this user was served (101; 100 connections at 3.95 ms p50 on a debug build);
  - probes were uncounted;
  - the dev origin as designed;
  - The owner's vault config loads unchanged.
- **Live at the review** (Tabitha/Claude, 11:06, with sudo): a client run as `nobody` (uid 65534) got
  `403 refused: the web UI serves only the user that runs the daemon` on `/ws` and on `GET /`. Health said
  `refused_peer: 2`, and the ledger had one `web.refused` row naming uid 65534. uid 65534 had no processes before
  or after, and the daemon was stopped by its socket.
- **Gates.** The gate was green at each lane commit (1,127, 1,131, 1,134 tests), and on `main` after the rebase
  (11:29, 1,143 tests; the bench's first run missed on one clean-shutdown outlier, p95 112.8 ms, and its rerun
  passed at 74.1 ms; `~/reports/theseus-merge/secfix-gate.log`). The rebase had one conflict, in health's
  `rpc/methods.rs`; both lines were kept.

**Decisions at the review** (Tabitha/Claude):
- A client that closed before the accept is dropped uncounted. It is never served either way. Counting it would
  ledger every port probe as another user's refusal.
- An approved `git.diff` or `git.log` on a path outside every root reads only under that path: what was approved
  is what is read. Widening it is the owner's call.
- The dev page connects straight to the daemon, with no proxy. The cockpit does so from the start (Item 23), which
  closes theseus-88im's relay for it.

**Divergence from the brief.**

| Brief | Built | Why | Keep? |
|---|---|---|---|
| gix's ceiling directories, or a check of the discovered working tree | The check, with the limit | A ceiling at the root makes a root inside a monorepo "not a repository" | Keep |
| — | A `core.worktree` outside the roots is refused | gix honours it, so the diff would read tracked paths there; found while writing the fix | Keep |
| A file the diff reads is under a root and not on the floor | Checked in history too (`A..B`) | A repository that tracked a floor file has its bytes in history | Keep |
| — | An approved path outside every root is its own root | Otherwise an approved call reads nothing, or the whole repository above it | Keep (Tabitha/Claude) |
| When the row can't be found, refuse and say why | A client that closed first is dropped uncounted | Port probes were ledgered as another user's | Keep (Tabitha/Claude) |
| — | The lookup runs on the blocking pool | The table read takes about 2 ms | Keep |
| An explicit, off-by-default dev origin in `[web]` | That, with the dev `Host` passed on `/ws` only | The proxy passes the dev server's `Host` | Keep |
| "After Part 2, a process of another uid is refused anyway" | Not through a dev server's proxy | The daemon sees the dev server's socket, the operator's uid | The dev page connects straight (Item 23) |

**Known gaps.**
- A dev server with a `/ws` proxy would relay another uid's process while `dev_origin` is set. Neither dev page
  has one now: both connect straight (Item 23, and the Observatory's at the docs commit; theseus-88im, closed).
  A squatter on the dev port still serves a page with the dev origin. Hence: off by default, ledgered, and "unset
  it when you are done".
- ~~The owner lookup reads the whole table (about 2 ms), and the accept loop waits on each one in turn
  (theseus-u6xg).~~ Built in Item 46: one `sock_diag` request, 11.6 µs.
- ~~Refusals held within the ledger's one-minute span are lost at shutdown (theseus-sqpx; H1's).~~ Built in Item 55.
- Health's `web` section isn't in the CLI's text summary or the Observatory (theseus-jxau; the cockpit shows it).

### Item 23. The new experience: the cockpit at `/cockpit/` (theseus-45n5; 2026-10-01 from 09:52, Tabitha/Claude in the foreground while the chain ran; d561792 to 8bea68c)

**Why.** The owner, 2026-10-01 09:52: a new UI, linked from the existing one as "see the new experience", not replacing
it. It is "as deeply focused on visibility and utility at managing theseus as the original", but "gorgeous, rich,
and make you feel like you're in the cockpit of the most powerful agent harness cockpit in the world", with
"an amazing amount of clarity on how theseus is running, with the ability to drill down to exquisite detail". It
"should feel alive, sleek, modern, powerful and engaging." Utility over flair; large libraries welcome.

**What exists.**
- **The app.** `cockpit/` is a second Vite app beside `web/`:
  - React 19.3, Vite 8.3, TypeScript 6, and Tailwind 4.3. It imports `web/src/protocol.ts` through an alias, so the
    two clients never drift apart.
  - ECharts 6.1 (tree-shaken) for every series, the Sankey, the treemap, and the flame chart.
  - React Flow 12 with elkjs 0.12, loaded only for a graph tab.
  - TanStack Query 5 and Virtual, zustand, cmdk, motion 13, lucide, Radix, react-resizable-panels 4,
    CodeMirror 6 for JSON, react-markdown with remark-gfm, and react-router 8.
- **Seven views:**
  1. **Bridge.** The header's heartbeat:
     - the link's round trip, uptime, executions running, a flow line of ledger rows a second, and the model;
     - the kernel, Discord, config, secrets, and web dots;
     - the cache hit rate, the spend, and the clock.

     Then KPI tiles with sparklines, and the pulse (ledger events by family, 30 minutes to all). Then the fleet
     ring, provider headroom, per-call latency, a spend Sankey (provider, model, session), the last start's
     phases, turns, tools, context size per compile, and the token mix. The activity river runs along the bottom
     (narrative and ledger).
  2. **Fleet.** Sessions and executions with state chips, cost bars, and budget gauges, and the task tree as a
     graph.
  3. **Session deck**, the drill-down:
     - the streaming transcript (thinking, and tool calls with their JSON);
     - a Gantt of model and tool time per loop, and the trace's flame chart with a cursor;
     - the context compilation manifests, and tokens and cost per loop;
     - the budget, and the pending confirms (approve, decline, trust);
     - a composer (`turn.submit`), the session's content graph, and a turn's replay on its own clock.
  4. **Actions.** The tool calls' state machine, and the approvals.
  5. **Ledger.** A treemap of kinds, a histogram with a brush, and search and kind filters kept in the address,
     with JSON drill-down over a virtual list.
  6. **Economics.** Spend by provider, model, and session, and the ledger's billed calls beside the sessions' totals.
  7. **Systems.**
     - The daemon, the kernel, and the last start.
     - Config, and secrets (names only).
     - The children, the broker's grants, and the Discord binding.
     - Approval, context files, profiles, and the catalog.
     - A read-only protocol console.
     - The web UI's access: refusals by kind, the owner check, and the dev origin, with links to their ledger rows.
- **Also:**
  - a ⌘K palette with actions, `g` shortcuts, and deep links (every view keeps its state in the address);
  - desktop notices for approvals, and an approvals badge in the nav;
  - a crash boundary and a not-found view.
- **Serving.**
  - `theseusd` embeds `crates/theseusd/cockpit/dist/` (a second `rust_embed` folder, `allow_missing`) at
    `/cockpit/`, with its own app-shell fallback, under the web UI's rules. `/cockpit` redirects.
  - The build isn't committed. `scripts/gate.sh` lints and builds it, and the install builds it first.
  - The Observatory's header links to it.
- **Dev loop.** The dev page connects straight to a scratch daemon whose `[web] dev_origin` names it. There is no
  proxy (Item 22).

**How it is proven.**
- `web::tests::the_cockpit_serves_its_shell_or_says_how_to_build_it`.
- The gate in the worktree (10:44, 1,123 tests), and on `main` after the merge (11:32, 1,144 tests, lifecycle OK on
  its first run, with the cockpit's lint and build). The rebase onto Items 21 and 22 had two conflicts. In
  `web.rs`, two new tests sat side by side, and both were kept. The Observatory's `web/dist` was rebuilt from the
  merged sources.
- An audit from the daemon: every view loads with no console or page errors, the first panel in 0.4 to 0.9 s.
- Two live GLM turns (about $0.002) were watched frame by frame, and the composer submitted a turn from the deck.
- The access panel was checked against a build from before 3qf ("not in this build") and against 5b42f03 with the
  dev origin set ("on · this user only", "open · 1 served"). A real refusal from uid 65534 read in the river and in
  the filtered Ledger.
- **Size:** 4.16 MB in 32 files, embedded uncompressed. elk's 1.43 MB loads only for a graph tab.

**Divergence from the design** (`contexts/theseus-cockpit.md`).

| Design | Built | Why | Keep? |
|---|---|---|---|
| TanStack Table for dense tables | Plain tables, with TanStack Virtual for the long one | The tables are small; the unused dependency was dropped | Keep |
| The build committed like `web/dist` | Not committed; the gate and the install build it | Hashed bundles stay out of history; a build without it says how to build it | Keep |
| The dev server proxies `/ws` to a scratch daemon, rewriting `Host` and `Origin` | The dev page connects straight; no proxy | The proxy was a relay for other pages and users (theseus-88im) | Keep |
| The fleet pushed by `executions.watch` | Polled | The spine's 9b hasn't landed | Until 9b |

**Known gaps.**
- Fleet polling until 9b's all-sessions push. The ledger is re-read by its newest rows each poll; an `after`
  position would read only what's new (theseus-xo0m).
- No auth beyond the web UI's rules, the same as the Observatory.
- ~~The Observatory's own dev config still proxies `/ws` (theseus-88im).~~ Closed at the docs commit: its dev
  page connects straight too, checked live (`ws://127.0.0.1:7434/ws`, 101).
- Found while building it: theseus-lluv, a failed turn's cost missing from a store's session total. It was closed:
  the turn ran under a binary from before theseus-hco's install, and today's code counts it.

### Item 24. Two lanes on `main`: a faster gate with a history, and recall's weights and forgetting (theseus-1hk; theseus-3onf: theseus-emc, theseus-jz8, theseus-64x; 2026-10-01 10:03 to 11:31, reviewed 11:36 to 11:41, merged after Item 23)

**Why.** Both lanes ran beside fix batch 2 and merged once reviewed (the recipe's rule 3).
- fastgate is review 2's S3, "do first": a gate whose timing bench measured unoptimized dependencies, and had no
  memory of past runs.
- recall is the hybrid recall's next step. It has three parts:
  - an exam probe that asks a running tender;
  - fusion weights chosen on the exam;
  - forgetting that reaches the vector files.

**What landed.**

| Lane | What | Landed |
|---|---|---|
| fastgate (1hk) | `[profile.dev.package."*"] opt-level = 2`. `theseus-sim bench lifecycle --record F --label L` appends a CSV row per run, and `theseus-sim bench history [--last N]` reads it with each phase's headroom. A passing run warns at 10% headroom. `scripts/gate.sh` records both runs of a miss and its rerun. §9 says so. | 8fbd6d5, adbeda4 |
| recall (3onf) | `theseus-exam probe --tender` asks a running tender per arm of sources and weights. Weighted rank fusion, `Σ w_s / (60 + rank_s)`: weights in `index.query`'s params over the tender's defaults (vectors 6, BM25 and entities 1), where weights of 1 are 29c's fusion bit for bit. `index.forget { nodes, texts }` takes nodes or chunks out of the index, and their vectors out of every vector file, rewritten atomically. A record whose text loses its last chunk is dead at once, and a file is compacted past a quarter dead. | 837f846, 9645403, f7f7607 |

**The fastgate lane's review** (`~/reports/theseus-lane-fastgate/review.md`).
- **What opt-level 2 buys**, from ten interleaved pairs:
  - every lifecycle phase's median p50 is 10 to 20% lower (a debug cold start 25.6 → 21.7 ms; the daemon's own
    time to serving 23.2 → 20.9);
  - the test suite runs in 90.7 s instead of 96.9;
  - the target is 6.8 GB instead of 7.8, and a debug `theseusd` is 104 MB instead of 144.
- **What it costs:** a cold build of the tests takes 3.0× the CPU (1,838 s against 610), once per worktree, and
  sccache shares it.
- **What it doesn't fix:** a stalled fsync still sets a ten-sample p95. 9 of the lane's 20 A/B runs missed under
  four builders. The margins don't change for now (theseus-zay1).

**The recall lane's review** (`~/reports/theseus-lane-recall/review.md`).
- **The weights.** The choice rule was written before the grid ran, and the default was committed before the
  held-out half was asked; held-out was judged once.
  - Held-in: 23 of 34 items had all their gold in the top 6 at vector 6.
  - Held-out, at k = 6: **22 of 34**, against 16 for equal weights and 14 for BM25. That ties vectors alone (22),
    which lead at k = 1 and k = 3.

  So the weights clearly beat equal-weight fusion. Over vectors alone, they buy robustness, not a measured gain.
  Seen after the choice, and not tuned on: a gold that only BM25 finds sinks under w = 6 (7th → 72nd). A reranker
  (32c) should add BM25's own tops to its candidates.
- **The query's latency, for recall's wire-in (30a).** The design's query takes **320 to 350 ms to embed on one
  thread** (213 ms on four), past recall's 250 ms deadline. That query is the turn's text plus 500 characters of the
  previous reply, about 115 word pieces. A short query takes 83 to 95 ms. 30a has to choose:
  - embed only the new text;
  - reuse the reply's stored vector as a second source;
  - or give each arm its own deadline in the tender.

  Fusion itself costs 74 µs at p50, at k = 40.
- **Forgetting**, live on the real model:
  - one node was forgotten in 42 ms, its record's bytes left every file, and no arm returns it;
  - a quarter-dead compaction of 100,000 records takes 216 ms;
  - a crash at each step of the atomic rewrite leaves the old file or the new one (tested).
- **No store schema, index format, shared file, or dependency changed.** One behaviour change: a node re-written
  with nothing to index now leaves the index. Today's core never re-writes a node, so nothing changes until
  redaction writes erased payloads.

**Gates.** Each lane's gate was green in its worktree:
- fastgate: Part 2 and Part 1;
- recall: 1,136 tests at each of three commits, with the bench passing on its first run.

Each was gated again on `main` after its rebase (`~/reports/theseus-merge/<lane>-gate.log`).

**Known gaps.**
- The bench's margins (theseus-zay1, above).
- The recall wire-in (row 51) moves the protocol's new shapes into `theseus-protocol`, and puts the weights, the
  dead records, and the compactions in health (`recall.md`, "What 30a and row 51 inherit"). _(Row 51 moved the tender's wire shapes into `theseus-protocol`: Item 50.)_
- An incident in passing: the recall lane's tender inherited the gate lock from 11:01 to 11:13 (theseus-e6xj,
  fixed in Item 21's tooling note).

### Item 25. Fix batch 2, part 3: the jobs' output (theseus-102, theseus-bzq part 1, theseus-2ij; 2026-10-01 11:56 to 12:59, reviewed 13:09 to 13:21; f2ce5c6, f38bc3e, 78521c1)

**Why.** This is the roadmap re-cut's row 4, and its last step: what a job's output costs, and what stops a job.
- One chatty job could take the daemon's memory, or the disk (Review 2's R3, theseus-102):
  - a job's output went uncapped into `spool/results/<id>.out`;
  - the runtime read the whole file to keep its last 4 MiB;
  - and on a full disk every WAL append fails, including the rows that would say so.
- A cancel or a `/stop` ran `job::terminate` for each job in turn, sleep-polling on a runtime worker for up to 2.5 s
  each (Review 2's S2 (1), theseus-bzq). Looking closer, fb2c found the wait watched the wrapper. The wrapper dies at
  SIGTERM, so the wait ended at once. A command that traps SIGTERM ran on, orphaned, while the cancel said
  `termination_verified`.
- Since H3 a job's raw output is deleted once its result is written, but three kinds had no end (theseus-2ij):
  - a cancelled or ended execution's job;
  - a crash between the result's frame and the unlink;
  - every pre-H3 file.

**What exists.**
- **102** (§3.16, §3.24, §6, §9):
  - Every job's output goes through the wrapper's copy (l0d's), capped at `[tools] job_output_max_bytes` (64 MiB).
    Past the cap the copy reads on and counts, and never stops the command.
  - The completion says `truncated`, `dropped`, and the cap. The result says what was printed and what was dropped,
    and the tool's way to get the rest.
  - The runtime reads a job's output by seek, 4 MiB at most.
  - Health's `disk` comes from `statvfs` under the state dir. It is `low` under `[server] disk_warn_mb` (5,120) and
    `below_floor` under `[server] disk_floor_mb` (1,024). Below the floor a job is refused, with its reason and a
    `job.refused` row. `theseus health` prints `disk:`.
- **bzq (1)** (§3.16): `job::Stopping`.
  - Every job's group gets SIGTERM at once, then one 2 s grace, then SIGKILL for the stragglers together.
  - A job is gone when no live process is left in its process group. _(A `setsid` descendant left the group and ran on, theseus-hcc; since 18a a job is gone when its wrapper's whole tree is, Item 60.)_
  - `terminate_all` is async and waits on the runtime's timer. Cancel, stop, and `task.cancel` are async through the
    dispatcher.
- **2ij** (§6): `sweep::sweep`, a tender after serving, at start and then hourly.
  - A raw output goes once its result is written, once its execution ended, or, when no action owns it, once it is
    a day old.
  - It stays while its wrapper lives, while its action is unsettled, or while its result may still be absorbed.
  - Each sweep is reported in `spool.swept` rows (counts and bytes), in health's `spool.last_sweep`, and in
    `theseus health`'s `spool:` line.

**How it is proven.**
- **Each part's tests**, each proved against a revert of its fix (`~/reports/theseus-fb2c/fb2c.md`, the parts' test
  tables):
  - 102:
    - a wrapper process printing 5 MiB past a 64 KiB cap: the file stops at the cap, the exit is kept, and
      `dropped` is reported;
    - the cap with a grant;
    - the copy, byte-exact at the cap and against `/dev/full`;
    - a 40 MiB reader that reads 4 MiB, and a sparse 8 GiB file's tail;
    - a real turn's truncated result, and a real turn refused below the floor;
    - health's states through a stand-in `statvfs`.
  - bzq:
    - a wrapper whose command traps SIGTERM, waited out and killed;
    - three such jobs in one grace;
    - the real daemon on one runtime worker, stopping three such jobs in 2 to 3.5 s with `health` under 500 ms
      throughout.

    The reverts: the wrapper-only wait took 146 ms and left the jobs alive; serial took 6.24 s; a blocking wait
    held health at 1.93 s.
  - 2ij: real turns with every fate, the rule's table, and the real daemon's tender at start. The fates: absorbed
    after a simulated crash, ended, unknown pre-H3, running, running with a live wrapper in a cancelled execution,
    pending, and young.
- **Live, fb2c's check, on a scratch daemon of the release build**, A/B against the 11:38 release:
  - a 200 MB job's file stopped at 64 MiB, and the daemon's VmHWM rose 9 MB (the build before: a 209 MB file, and
    VmHWM up 211 MB);
  - the result named the dropped bytes;
  - a floor above the free space refused a job, with its reason;
  - three jobs that trap SIGTERM stopped in 2.27 s, with `health` at 4 to 9 ms meanwhile and none left alive.
    Before: 0.37 s, with three left running, each `termination_verified`;
  - a start swept a cancelled execution's output and a stale 0644 file, and kept three pending and one running.
- **Live, the review's check, with different inputs** (`~/reports/theseus-fb2c/review/review.md`):
  - with the cap set to 1 MiB, a 5 MiB job's file stopped at exactly 1,048,576 bytes, and the result's dropped
    count was exact (4,194,313);
  - a stop of two jobs whose bash and subshell both ignore SIGTERM left 0 of their 10 processes, in 2.27 s, with
    `health` at 3 to 32 ms;
  - planted orphans went as `unknown` (three days old) and stayed as `young` (new);
  - the stopped jobs' outputs were `pending` until the next turn wrote their cancelled results, and then went as
    `absorbed`;
  - `low` warned while a job still ran.
- **Gates.** Green at each commit: 1,177, 1,180, and 1,184 tests, with lifecycle within its budgets on each first
  run. The review's rerun on `main` at 78521c1 was green too: 1,184 tests, cold start p50 22.7 ms.

**Divergence from the brief and the issues.**

| Brief or issue | Built | Why | Keep? |
|---|---|---|---|
| 102: below the floor, new jobs *wait* (the issue) | refused, with the reason as the call's result | a wait needs a wake when space returns, and nothing provides one (the brief's call) | Keep; theseus-f337 |
| 102: a cap in the wrapper "with a marker" in the file, or `RLIMIT_FSIZE` | the copy counts; no marker in the file; the completion and the result say it | `RLIMIT_FSIZE` kills the writer (SIGXFSZ), and a marker would grow the file past the cap | Keep |
| l0d: a job without a grant writes its own file "at no cost" | every job goes through the pipe and the copy | nothing else can cap a file without killing the job | Keep (cost measured live: 20 to 40 ms on a 32 MiB job) |
| bzq (1): `spawn_blocking`, or async with the timer | async with the timer, and the wait is on the job's process group | the wrapper-only wait was the bug above; the group wait is what makes "one grace" true | Keep |
| 2ij: "a sweep after serving" | after serving, then hourly | otherwise a long-running daemon's cancelled jobs' output waits for the next start | Keep |
| 2ij: "ledger one row per sweep" | a row when a sweep removed a file, or when a start's first sweep found one | an hourly row on an empty spool is an fsync and noise, and health shows every sweep | Keep (the review's call) |
| 2ij (the issue's alternative): "terminal and older than a stated age" | age only for files no action owns (24 h) | a known job's result held behind a budget question for days must not be lost | Keep |

**Known gaps.**
- ~~theseus-vni9 (P2): S2 (2), a single store-writer thread. The cancel's own kernel transitions still fsync on a
  worker.~~ Built in Item 52.
- ~~theseus-avvb (P2): S2 (3), the periodic index checkpoint in a tender.~~ Built in Item 52, on the store's writer.
- theseus-f337 (P3): the disk's `low` and `below_floor` reach no one unless they read health, and nothing wakes a
  refused job when space returns.
- ~~theseus-ht82 (P3): a running job can still fill the disk through files it writes itself. The floor only refuses
  the next job.~~ Built in Item 54: below the floor every running job stops.
- theseus-51v8 (P3): the Observatory and the cockpit don't show `disk` or `spool.last_sweep` yet.
- ~~theseus-gsn9 (P3, the review): the cap keeps the head, so a job that overruns it loses its end, where builds and
  tests print their verdict.~~ Built in Item 34: both ends are kept.
- ~~theseus-ewev (P3, the review): a job stopped before its completion keeps its raw output until the sweep after its
  cancelled result is written, since `result_ref` is unset.~~ Built in Item 34: it goes once the cancelled result is written.
- Health sees only the filesystem Linux reports. Under WSL, C: can fill first. This is documented, not detected.

### Item 26. Two lanes on `main`: FSRS-6 checked against its reference, and the telemetry corrections (theseus-3ht; theseus-yf1; 2026-10-01 12:00 to 13:06, reviewed 12:45 and 13:22, merged 13:31 and 13:33)

**Why.**
- **fsrscheck** (32a's prerequisite). The math lane wrote theseus-memory's FSRS-6 from memory, and its report
  called five points of it "fairly sure". Before the `+retention` arm's wire-in (32a) puts it under real nodes,
  each point was to be checked against the published reference.
- **telemetry**: theseus-hee's open questions, which had no consumer yet. Every turn over 10 s fell in the last
  histogram bucket. §3.20 and §3.23 promised a tool metric by family, backend, and outcome, with a duration, for
  the shell-fallback ratio, and no exporter ever had one. `gen_ai.response.model` carried the requested model. A
  continuation turn's failure was counted nowhere. The provider call's time had no attributes. This is the
  roadmap's row 13a, built ahead in a worktree while fix batch 2 part 3 ran on `main`.

**What exists.**
- **FSRS-6, against the `fsrs` crate 6.6.2** (§5.1; `crates/theseus-memory/src/fsrs.rs`, 5c6ec86 and 2d51323):
  - Four of the five points match: the 21 defaults digit by digit, E4's unclamped D0(4), E6's lapse cap, and
    E9's bounds.
  - **E7's same-day floor** now holds from Hard up, as the reference's model and optimizer have it. A same-day
    Hard used to cut stability by 4 % to 69 %. That matters here: an access of unknown outcome is graded Hard, so
    a node used twice in a day lost 39 % of its stability at S = 1 day.
  - Two differences the math lane hadn't modelled: the prior's S and D are clamped before a review (a prior at
    S = 0 gave NaN), and parameters are clipped into the reference's bounds. A non-finite parameter is still an
    error.
  - The oracle beside math.md was corrected and rerun. One golden row changed: sequence B's day-5.5 Hard now leaves
    S at 3.2515.
- **Telemetry** (§3.20, §3.23; 393c705 and c9d4985):
  - **Bounds.** Every duration histogram shares one set: the SDK's up to 10 s, then 20 s, 30 s, 1, 2, 5, and
    10 minutes.
  - **Tool metrics.** `theseus.tool.calls` and the new `theseus.tool.duration_ms` carry the turn's attributes
    and the tool's name, family, backend, and outcome. They are read from the turn's tool spans, which now record
    them. A failed turn's calls count too, and `theseus.tool` became `theseus.tool.name`.
  - **Models.** `gen_ai.response.model` is the served model, and there is none on a failed call. A turn's metrics
    name the requested model, as its failure and its provider calls do. The live check found that half: an
    alias's answer named a dated id.
  - **A failed continuation is counted** like a client's failed turn, through `Core::count_failed_turn`: in
    health's `provider_errors`, in `theseus.turns`, and in `theseus.provider.errors`.
  - **The provider call's time** carries its provider and model.

**How it is proven.**
- **fsrscheck:**
  - each value checked against the reference, with file and line, and no code copied;
  - new tests written first, each failing on the old code;
  - `matches_the_reference_crates_own_numbers` pins the reference's own test numbers to f32's rounding;
  - a second, independent transcription (`xcheck.py`, from the reference's `model.rs`) agrees with the
    corrected oracle on all 14 rows, to the 15th significant digit.
- **telemetry:**
  - 8 new tests, each shown to fail when its fix is undone (10 probes);
  - the golden file changed only where the corrections change the picture;
  - live, on a scratch daemon with a local OTLP receiver:
    - a 28.7 s GLM turn in (20 s, 30 s], and a 12.3 s `proc.run` in (10 s, 20 s];
    - the tool metrics with their attributes;
    - a `claude-haiku-4-5` call answered as `claude-haiku-4-5-20251001`;
    - a refused model's input turn and the driver's retry, counted as 2 failed turns and 2 provider errors,
      with health saying 2.
- **The review's check of the merged release build** (b6be80d, installed 13:46; a scratch daemon with the lane's
  receiver):
  - a 29.5 s GLM tool turn in (20 s, 30 s], with 21 bounds up to 600 s;
  - its `proc.run` (12.1 s, in (10 s, 20 s]) and `fs.read`, counted and timed with name, family, backend, and outcome;
  - a refused model's turn and the driver's retry: 2 failed turns and 2 provider errors, each in one series, and
    health's `provider errors 2`;
  - the provider call's time by provider and model.
- **Gates.** Each lane's gate was green in its worktree: fsrscheck at both commits (1,169 tests), and telemetry
  at both (1,172 and 1,173). Each was gated again on `main` after its rebase (`~/reports/theseus-merge/<lane>-gate.log`):
  fsrscheck at 2139b49 with theseus-m2lt (13:31:09, 1,188 tests; its first try at d047483 missed the bench under the rotation's IO, below), and telemetry at 71698b0 (13:33:19, 1,196 tests). Each bench passed on its first run.

**Divergence from the briefs.**

| Brief | Built | Why | Keep? |
|---|---|---|---|
| fsrscheck: check five points | three differences fixed, two beyond the five | the reference's clamp and clip are the algorithm too, and a prior at S = 0 gave NaN | Keep |
| fsrscheck: clip or refuse out-of-bounds parameters (the lane's choice) | clip, as the reference does | the same parameters make the same model; `params()` shows the clipped values; a non-finite one is an error | Keep; theseus-3fjv says what was clipped once parameters load from config |
| telemetry: five corrections | six: a finished turn's metrics name the requested model | found live: a turn's series and its provider calls' disagreed on an alias | Keep |
| telemetry: bounds "up to about 600 s" for turn and provider durations | one set for all four duration histograms, to 600 s exactly | the default timeout of a provider call and of `proc.run`; one set compares across instruments | Keep |
| telemetry: `theseus.tool.calls` by family, tool, backend, and outcome | and the turn's four attributes; `theseus.tool` renamed `theseus.tool.name` | the ratio per model; OTel's naming rules | Keep |
| telemetry: count a failed continuation in `theseus.turns` and `theseus.provider.errors` | in health's `provider_errors` too | "as an ordinary failed turn is": `turn.submit` counts both | Keep |

**A tooling change at the join** (theseus-m2lt, 2139b49). Another agent's full openclaw rotation (38 branches) started at
13:22 and held the kernel's IO pressure at 26 to 49 %. fsrscheck's first gate on `main` then missed the bench
twice: a restart's p95 was 2,257 ms, on a tree that had passed at 13:13 with 41 ms. Every test passed. The gate now
waits before each bench run until IO and CPU pressure settle (§9, "A quiet machine"). The budgets are unchanged.

**Known gaps.**
- fsrscheck: theseus-b9l4 (P3: confirm E7's Hard floor against the FSRS wiki and a second scheduler, since the
  reference disagrees with itself), and theseus-3fjv (P3: the loader says which parameters it clipped).
- telemetry:
  - ~~theseus-b85w: a failed turn's tokens and dollars;~~ built in Item 67;
  - theseus-8pei: a confirmed call's run and a background job's end aren't timed;
  - ~~theseus-8u02: the first-token histogram has only each turn's last call;~~ built in Item 67;
  - ~~theseus-iu3a: a failed tool call's span has no error status;~~ built in Item 67;
  - theseus-lmhp: failed calls share the provider-call series;
  - theseus-ksfu: a timing test that fails a debug gate under load.

  All are P3.

### Item 27. The cockpit's second round: every call opened whole, and the controls it lacked (theseus-45n5; 2026-10-01 from 11:40, Tabitha/Claude in the foreground; b4ed64f to 454709b, rebased as d0aa674 to abc8411, merged 13:36:52)

**Why.** The owner's brief for the new experience (09:52) asked for drill-down "to exquisite detail". Item 23's cockpit
showed each turn's loops, calls, tokens, cost, and context, but one tool call's story was still spread across the
transcript and the ledger, and one model call's cost and cache were spread across a node and a row. The cockpit also
couldn't open or recompile a session, list or cancel tasks, or change the live profile, though the protocol has had
each since M1 or DD7.

**What exists** (§3.14, "The cockpit"):
- **The call inspector** (`?call=<tool_use_id or correlation id>`), a drawer on the session deck:
  - what the model asked: the input, and the planned resources;
  - what the gate said: its word, toned, the posture, and the reason;
  - the call's life as timed phases from its ledger rows: planned, awaiting approval (and who answered),
    authorized to dispatched, starting, running, and until recorded (a late result waiting for the next turn);
  - the job (pid, argv, cwd, timeout, exit, output bytes), and what came back (status, truncation, lateness,
    external text, the text with a copy button, the meta);
  - the raw nodes.

  It opens from a tool card's inspect button, and from the Actions view's rows by the action's id. A call the gate
  denied says that no action was planned for it. The ledger and the activity river now say
  `action.confirmed`, `action.confirm_answered`, and `discord.confirm` in plain words.
- **The model-call inspector** (`?msg=<node_id>`):
  - where its time went: to the first byte, then to the first token, then the stream;
  - its tokens, the share from the cache, and what the cache saved at the catalog's prices;
  - its cost recomputed line by line at today's prices beside the record, amber when they differ;
  - the provider's rate-limit headroom at that moment;
  - the context it saw, what it said, and the calls it asked for, each of which opens the call inspector;
  - its life as a kernel action.

  Opening one inspector closes the other. **Each reads the ledger only while it is open.** Before this fix, both
  inspectors read it ahead of their early return, so the deck polled 5,000 rows every 3 s with neither open. That
  was counted from the page's own WebSocket frames, before and after.
- **Controls, each confirmed first:**
  - Fleet's "New session" (`session.open`) lands on the new deck;
  - the deck's "Recompile" menu (`session.recompile`, `transcript` or `fresh`);
  - a Tasks panel in Actions (`task.list`, newest first, with state, waits, questions, parent, spend under its
    carve, turns, report target, and ended reason), with Cancel (`task.cancel`) for a live task. Ended tasks fold
    away;
  - "make live" on each profile in Systems (`profile.use`).
- **Economics** gained a "Last 24 hours" tile (with the 7-day total and daily average). The note on two differing
  totals now says truly why they differ: turns from builds before theseus-hco missed a failed turn's cost
  (theseus-lluv, closed), so the calls are the record.
- `protocol.ts` gained `TaskInfo` and `TaskListResult`, the Rust types' fields. The change is types only, so
  `web/dist` doesn't change.

**How it is proven.** Each commit was checked live with real clicks (puppeteer, `~/reports/theseus-cockpit/shots/`)
on a scratch daemon over a copy of the owner's store:
- a confirmed `proc.run`: approval 3.73 s (a Discord press), a run of 45.0 s, and 7.9 s until recorded;
- a call the gate denied;
- a Sonnet call: 780 ms to the first byte, 97.9 % cached, saving $0.0077, and the recomputed $0.0024 matching the
  record;
- "New session" ledgered `session.opened` and `execution.opened`; "Recompile → transcript" ledgered
  `context.recompile_requested`;
- a GLM task with a $0.05 budget waiting on its budget, then cancelled by its Cancel, "cancelled by the web UI";
- every view loading clean (`audit.mjs`).

The review's check of the installed build (b6be80d): every cockpit view loads clean from the binary, on a fresh
copy of the owner's store. The catalog card shows the 1-hour write price (Item 29); its columns were spaced and the
profiles moved under each model's name, in the docs commit. The cockpit's lint and build run in the gate. Its merge gate on `main`: green at abc8411 (13:36:15, 1,196 tests). The bench's first run missed on one restart outlier (p95 296 ms, p50 41 ms), and its rerun passed.

**Divergence from the design.** None from `contexts/theseus-cockpit.md`. The inspectors were the owner's "drill down
to exquisite detail", made concrete.

**Known gaps.**
- theseus-cny7 (P3): an operator act other than an approval ledgers `by` as the connection's label (`web#35`), not
  the surface.
- theseus-51v8 (P3): health's `disk` and `spool.last_sweep` (Item 25) aren't shown yet.
- The cockpit's build isn't committed. The install builds it before the release build (Item 23).

### Item 28. A README for newcomers, and the design documents' markdown in `docs/` (theseus-4i61; 2026-10-01 13:00 to 13:08, the owner's request at 12:59; 74ef515, rebased as 4961916, merged 13:38:21)

**Why.** The owner (12:59): keep the highly technical README, but move it into `docs/`. The README should be a quick
how-to-set-up that starts with a very end-user-friendly explanation: what we did and why, why another harness,
what we wanted to accomplish, and why Theseus is interesting. And the design documents whose PDFs he had seen
should be in `docs/` as markdown.

**What exists.**
- **`README.md`**, new:
  - what Theseus is, in plain words;
  - "Why another agent harness?": the questions other tools answer softly, and the guarantees Theseus makes
    instead (money as a gate, speed as a tested contract, reading the web stops the hands, nothing lost or done
    twice, memory that has to pass an exam). It claims uniqueness only for the four that the landscape research
    found nowhere else;
  - what it's like to use, why it might interest you, and where it stands;
  - a quick start that matches the binaries' own help: build the web apps and the release, install,
    `theseusd example-config`, `THESEUS_CONFIG`, the vault token, `theseusd check`, then a first `theseus ask`;
  - where the documentation is.
- **`docs/technical-overview.md`**, the previous README, moved with `git mv`:
  - its links fixed for the new place;
  - its stale lines corrected: fjall and the store's benches were removed in batch C, so the commands are now
    `crash-test`, `bench lifecycle`, and `bench history`;
  - the Daily Driver's four toollets named;
  - the cockpit described, and the build steps running `scripts/gate.sh` and building the cockpit.
- **`docs/design/review-2.md`** (Review 2, 2026-09-30) and **`docs/research/`**: "Theseus among the harnesses"
  (`harness-landscape.md`), its two research reports, and a README.
- **`docs/README.md`**, a reading guide. `docs/design/README.md` lists Review 2.

**How it is proven.**
- The owner saw a rendered preview, `Theseus-README-preview.pdf`, before the merge.
- Every relative link resolves.
- The scrub used the design docs' patterns (`~/reports/theseus-docs-design/verify/*.pat`). Local paths became
  plain words or links. The remaining hits are public sources, standard tool paths (`~/.theseus`, `~/.claude`,
  `~/.ssh`), and token prefixes named as patterns.
- The merge gate on `main`: green at 4961916 (13:38:13, 1,196 tests).

**Known gaps.**
- ~~theseus-8d1b (P2): `theseusd example-config`'s template and `--config`'s default still name the operator's own
  vault and items. A newcomer following the quick start sees them. The fix is placeholders that read as
  instructions, and a generic default.~~ Built in Item 55.
- ~~The README names what is next on the roadmap. Each spec version that lands a step should check that paragraph,
  as it checks `docs/the-ship-of-theseus.md`.~~ Since 2026-10-01 (the owner), the README stays stable and links to
  `docs/status.md`, which each spec version that lands a step updates (theseus-5d96).

### Item 29. Caching, part 2: two breakpoints on the system, the caching minimum, a TTL per profile, and 1-hour writes priced (theseus-ev1, row 14 (13c) built ahead in the `cache2` lane; 2026-10-01 12:31 to 13:19, reviewed 13:35, joined 13:42:19; 047a477, c970d36, 86adced, and the join b6be80d)

**Why.** M3 put one breakpoint on the system (Part III A3; the cache lane, Item 16, held the header byte-identical), so any context-file edit rewrote the whole
prefix: the tools and the header with it. Appendix F's rule (theseus-3nk), that nothing retractable goes in the
shared header, made a split possible. The stage-2 design's 13c added:
- a minimum below which a breakpoint can't cache;
- which models cache at all;
- a TTL per profile;
- and the must-not-miss: Anthropic prices a 1-hour write at 2 × input, against 1.25 × for 5 minutes, and Theseus
  priced every write at the 5-minute rate.

**What exists** (§4.5):
- **Two blocks.** The header (the built-in persona, the tools note, the profile's `system`) and the context files,
  each with a breakpoint, and the automatic one follows the conversation: 3 of the provider's 4. Without context
  files the system is one block, with its old bytes and digest.
- **The minimum.** A block whose prefix (the tools and the system through it) is under 2 bytes a token of the
  model's `cache_min_tokens` gets no breakpoint, since the provider could never cache it. The bound errs toward
  placing one: a breakpoint the provider skips costs nothing, and a dropped one that would have cached costs a
  rewrite in every session. `Manifest.cache` records the layout, `context.compiled` rows carry `cache`, and the
  narrative says when a block went unmarked.
- **`caches`** per model in the catalog, true for every built-in. A model whose row says false gets no breakpoint.
- **`cache_ttl = "5m" | "1h"`** per profile (default `5m`, the old bytes). `1h` goes on every breakpoint, the
  automatic one included. A task's own conversation stays `5m`, after the header's `1h` entries, so a longer-lived
  entry never follows a shorter one.
- **1-hour writes priced.** `Usage.cache_creation_1h_input_tokens` carries Anthropic's split, and the catalog's
  `cache_write_1h_per_mtok` prices it wherever a call is priced: the `provider.call` row, the node, the session,
  the dollar budget's settlement, and health. The built-in catalog is `2026-10-01.1`. Both web apps price it too
  (the join).
- Sonnet 5.5's caching minimum is 512, the claude-api reference's value (theseus-o388 confirms it).

**How it is proven.**
- **Tests:**
  - the header test on a real `theseusd`, for both template profiles: two blocks, the header byte-identical
    across sessions, and after an edit only the context block changed;
  - the compiler's layout, with the minimum's bound exactly (8,192 bytes marked, 8,191 not, on Haiku 4.5), the
    TTL's order, a model that doesn't cache, and an old manifest;
  - the 1-hour price end to end through the real runner: the budget, the session, the row, the turn, and health
    all at $0.173, against $0.128 at the old pricing;
  - the template's un-commented `cache_ttl` parsing;
  - a test that reads each old layout.
- **Live, the lane's check** (real API, $0.057):
  - a second session read 5,118 tokens of the header and the context from the cache;
  - after a context-file edit, a third session still read 5,045 tokens (the tools and the header) and wrote only
    168;
  - a `1h` call wrote 5,249 tokens, all of them 1-hour, priced at $4.00 per million on Sonnet 5.5. By hand that is
    $0.021564; the old pricing was 36.5 % short;
  - a task's `1h, 1h, 5m` request was accepted, and read its parent's 1-hour header;
  - GLM accepts `ttl: "1h"` and caches as before.
- **Found live, and acted on.** The compiler's chars/4 estimate runs 25 to 52 % low against Anthropic's tokenizers,
  so the minimum's check doesn't use it. Haiku 4.5 counts today's header just under its 4,096 minimum, so its
  breakpoint is sent and skipped until the header grows. The second commit's message (c970d36, rebased from 1531169) says Haiku counts the header at about 4,150,
  over its minimum. That was an extrapolation, and the second Haiku check measured it under; 86adced corrected the
  code's comments, and the message stays as pushed.
- **The review's check of the release build** (b6be80d, installed 13:46; the lane's driver on a scratch daemon,
  $0.022):
  - a second session read 5,122 tokens from the cache;
  - after an edit, a third session still read 5,045 tokens and wrote 172;
  - the 1-hour profile's session wrote 347 tokens, all 1-hour, and cost $0.0040518. By hand that is 347 × $4.00 +
    10,309 × $0.20 + 6 × $2 + 59 × $10 per million;
  - its task ran `1h` with a `5m` conversation, and GLM was unchanged.

  The 1-hour session also read 10,309 tokens: the lane's 1-hour header entries, written at 13:04, were still
  cached 43 minutes later.
- **Gates.** Green in the worktree at each commit (1,189 tests). The join's gate on `main`: green at b6be80d (13:42:09, 1,208 tests).

**The join** (b6be80d):
- The TypeScript half (theseus-wz9e): `protocol.ts`, the Observatory's Context panel, and the cockpit's Economics.
- By hand, for the cockpit2 round's surfaces, which the lane's patch predates: the model-call inspector's Cost
  table splits 5-minute and 1-hour writes, and Systems' catalog shows the 1-hour price.
- Session schema 6 and compilation schema 3: an older binary would drop the new fields when it rewrote those
  records. Nodes and ledger rows aren't rewritten, so they stay.
- `RENDERER_VERSION` 2.

**Divergence from the plan.**

| Planned | Built | Why | Keep? |
|---|---|---|---|
| The minimum's check at chars/4 | At 2 bytes a token | chars/4 counted the header at 3,350 tokens, and Sonnet 5.5 at 5,045; a dropped breakpoint costs, a skipped one doesn't | Keep |
| Haiku 4.5's header gets no breakpoint | It is sent, and the provider skips it | Haiku counts the header just under 4,096 today; it caches once the header grows | Keep |
| Sonnet 5.5's minimum, 1,024 | 512 | the claude-api reference's value, which flags it to confirm | Keep; theseus-o388 |
| `caches` per provider | per model, in the catalog | the catalog is keyed by model | Keep |
| A TTL for every breakpoint | yes, but a task's own conversation keeps 5 minutes | its loops run seconds apart, so 1 hour would only add the write premium | Keep |
| Join after C3 (the chain's plan) | before C3 | C3 hadn't started, and its ts-rs types then include `Usage`'s new field from the start | Keep |

**What the install changes for the owner.**
- A session with context files rewrites its cached prefix once, on its next turn (about $0.013 on Sonnet 5.5), and
  drops that prefix's thinking. The split changes the system's blocks, which a thinking block's signature
  records.
- A 1-hour TTL is his call (`cache_ttl = "1h"`). The cache lane's break-even is a 5 to 60 minute pause.
- An older binary now refuses the store (session schema 6), so a rollback restores the backup.

**Known gaps.**
- theseus-f5hf (P2): the overflow ring still uses chars/4.
- theseus-o388 (P3): confirm Sonnet 5.5's minimum.
- theseus-4v1z (P3): the CLI's catalog and health lines show no 1-hour price or count.

### Item 30. Batch C, part 3: the CLI's `run()`, one typed definition per wire shape with the web apps' types generated, and the Discord runtime (theseus-0g4; 2026-10-01 13:53 to 15:22, reviewed 15:25 to 15:28; 5522f85, b3b1ae1, e6992a8, 0dcceed, d6609a3, d59d990)

**Why.** The roadmap re-cut's row 5, the complexity review's last batch: the three findings that waited for the
Daily Driver. Finding 11, the CLI's `run()`, had grown to 873 lines at cognitive complexity 123. Finding 12: ten of
the 21 notifications were `json!` read back by string keys in three renderers, the gate record a `Value` read by
key in four places, and the web apps shared 400 hand-written lines of types that had drifted. Finding 19: two
copies of the Discord route lookup, a 171-line `serve`, and a crate doc that called the binding a protocol client.
The owner approved generating the web types with ts-rs (12:17, again 12:59). Behaviour and the wire stay byte for byte.

**What exists.**
- **Finding 11** (5522f85 tests first, b3b1ae1): `main.rs` is the arguments, `Conn`, and a 25-line `run`; `cmd.rs`
  one async fn per subcommand and `output()` (the daemon's answer as sent under `--json`, else the decoded lines);
  `render.rs` the renderers, with `Printer` taking a `Mode` (Text, Quiet, Watch, Json) plus `thinking`.
- **Finding 12** (e6992a8 tests first, 0dcceed, d6609a3): a struct per notification and `Event` in
  `theseus-protocol`; `GateRecord`, with `Plan`, `Resource`, `Access`, `Proposal`, `Notice`, and `ContextFileRef`
  moved there as single definitions; a tool-call node's gate typed and written canonically; senders build events;
  the Discord renderer, its routing, and the CLI match on them. The web apps' types are generated into
  `web/src/protocol.gen/` (119 types), `protocol.ts` keeps the client and re-exports them, and the gate fails when
  they are stale.
- **Finding 19** (d59d990): `Routes::resolve` for both handlers; `serve` as `connect`, `start_places`,
  `event_loop`; the crate doc says the binding runs in-process, its acts through the protocol (13 methods), its
  delivery and reporting direct (53 uses).

| measure | before | after |
|---|---:|---:|
| CLI `run`, lines and cognitive complexity (clippy) | 873, 123 | 25, 22 (its 20 `.await`s) |
| longest CLI subcommand | (inside `run`) | 65 (`ask`) |
| notifications built with `json!` | 10 of 21 | 0 |
| gate record readers by string key | 4 sites | 0 |
| `web/src/protocol.ts` | 516 lines, hand-written | 114, the types generated |
| Discord `serve` | 171 lines | 12, with `connect` 48, `start_places` 73, `event_loop` 81 |
| route lookups in the Discord handlers | 2 | 1 (`Routes::resolve`) |
| tests (gate) | 1,240 | 1,258 |

**How it is proven.**
- **The CLI:** 32 golden scenarios written against the old code pass unchanged; all 33 `--help` pages are
  byte-identical; every subcommand ran live with the old and the new CLI, and on one shared state 33 of 34 outputs
  were byte-identical (the 34th: the daemon's connection counter).
- **The wire:** 43 fixtures captured from today's senders equal the typed structs byte for byte and round-trip; a
  live session's notifications from the old daemon (166) and the new (104) decode and re-encode byte for byte, every
  method with the same keys and types; all 32 tool-call nodes of a copy of the owner's store, the 4 older `mode` rows
  among them, re-encode unchanged.
- **The web apps:** both type-check, lint, and build against the generated types with no `any` added and
  `web/dist` unchanged; the gate's check fails on a Rust change without its TypeScript (probed); on a scratch daemon
  of the build the cockpit's six views and the Observatory's two tabs load clean.
- **Discord:** the resolve test; 11 of 11 live steps through local stand-ins (a channel, an unlisted author, an
  unbound channel, a mention-only channel, a DM, a card's press refused and then approved); on `#theseus-test`, the
  commands, READY, and the bind notice.
- **Gates** green at each of the six commits: 1,240, 1,241, 1,250, 1,256, 1,257, and 1,258 tests.
- **The review's gate rerun** on `main` at d59d990 was green: 1,258 tests. The lifecycle bench passed with three
  phases within 10 % of their limits. Every phase ran about 2× slow, including code the commits never touched
  (the config parse, 1.01 ms against 0.49; restore, 233 against 107): the openclaw rotation's typecheckers had
  load at 14 on 16 cores, with little CPU pressure. theseus-611s makes the settle step wait on the load average
  too.

**Divergence from the brief and the review.**

| Brief or review | Built | Why | Keep? |
|---|---|---|---|
| `Printer` takes a Mode (text, quiet, JSON) plus `thinking` | Four modes: Text, Quiet, Watch, Json | `watch` also shows each turn's start and end and every context decision; a fourth mode says so instead of a flag | Keep |
| Cognitive complexity at most 15 per function (review) | `run` 22, `Printer::on` 23 | `run` is a flat match whose 20 `.await`s clippy counts; `Printer::on` is one flat match over 15 events | Keep |
| `theseus confirm` with no id uses `confirm.list` | Already did (C2, 750ec12) | | n/a |
| A typed `GateRecord`; stored rows decode unchanged | Typed, written through `canonical` (sorted keys) | Stored records are sorted maps; the typed struct's field order would otherwise change new records' bytes | Keep |
| (not asked) | `Plan`, `Resource`, `Access`, `Proposal`, `Notice`, `ContextFileRef` moved into `theseus-protocol` | One definition per wire shape: the gate record and `context.compiled` carry them | Keep |
| A job's `tool.started` fields | Optional fields on `ToolStarted`, `granted` absent, null, or a name | ts-rs cannot flatten an optional struct; the bytes are unchanged | Keep |
| Notification structs strict | Each reads a missing field as its default | The renderers always read them so; serialization is unchanged | Keep |
| Optional TS fields `field?: T` | Exactly that, through `#[ts(optional)]` on 129 fields | ts-rs alone writes `field?: T \| null` | Keep |
| `binding.report`, or say the binding runs in-process | The doc, with the reason | One method would remove neither the courier's outbox access nor the start's gate, secrets, and log | Keep |
| A live message and a press on `#theseus-test` | Connect, READY, and the bind notice on real Discord; the message and the press through local stand-ins | A bot can neither type nor press | theseus-kl8m |

**Known gaps.**
- `NodeInfo.detail` is still a JSON map per node kind; its TypeScript is `Record<string, unknown> | null`
  (theseus-a6be).
- ~~`theseus-sim fake-discord` has no gateway and no interaction callback; C3's stand-in lives in the report
  (theseus-6g62).~~ Built in Item 47.
- Two tests race under load: the sweep test checks a spawned wrapper before it exec'd (theseus-uev6), and
  `tests_outbox::a_card_in_a_channel_mentions_its_answerers…` (already theseus-50p).
- ~~Stored floats re-parse one ULP off without `float_roundtrip`, so 5 of the owner's 105 nodes re-encode differently
  (theseus-k52m; nothing re-encodes a node today).~~ Built in Item 54.
- ~~The message and the press on real Discord wait for the owner (theseus-kl8m).~~ Item 47 proves both through the
  stand-in, in the gate; real Discord's own half stays with the owner.

### Item 31. An honest token estimate, and a timing test that holds under load (theseus-f5hf, theseus-ksfu; the `tokens` lane; 2026-10-01 13:55 to 14:55, reviewed 15:04, joined after Item 30; f9509f3, 7bbd21c, ce19342, rebased as 7335e15, 4aa13db, 042ff07)

**Why.** The compiler's chars/4 estimate ran 29 to 35 % low against Sonnet 5.5's count of Theseus's requests
(the cache2 lane found it; this lane measured it again, live and on the owner's DM): their tool schemas and tool
results are dense JSON. The overflow ring and the budget's reservation trusted it, so a conversation heavy in tool
output could pass a 1M window in the provider's count while the estimate read about 650 k. And a serve-first test
held a debug build's health to 50 ms of wall time, which a busy machine failed (403.6 ms in the telemetry lane's
gate).

**What exists** (§4.4a, §4.5):
- the estimate counts on the provider's own count from a compilation's second call on, and estimates only what
  is new, by class at the catalog's `bytes_per_token` (Claude 2.4 and 3.3, Haiku 4.5 2.9 and 4.0, GLM 3.7 and
  4.4; a config may override them);
- the ring rings on the estimate's bound (the estimated part × 1.4, the worst measured being ×1.36);
- `context.compiled` carries `estimate`; the reservation and the narrative follow it;
- the health test runs on tokio's paused clock, and proves health waits for nothing instead of timing it.

**How it is proven.** Four compiler tests and one catalog test (the ring for JSON that chars/4 reads as fitting,
the counted estimate by hand, nineteen loops of the owner's DM, eight live first requests, a GLM session not rung
early), and the reservation's test. Live, on a scratch daemon of the lane's build (the owner's 15 tools, the lane's
own context files, $0.029): every first request within 7 % of the provider's count, every counted one within
1.7 %, where chars/4 was 29 to 35 % low on Sonnet 5.5. The health test passed 20 of 20 runs twice under 32 busy
loops at nice 5, where the old one failed 3 of 20 twice, and it fails at once on a health that waits for the
vault.

**Divergences.** The brief's options were a catalog figure, a calibrated in-memory ratio, or a safety factor;
the lane did the first and a stronger form of the second (the provider's own count, per session, from the store,
so nothing lives in memory and a restart loses nothing), and kept a margin on the estimated part only.

**The join** (Tabitha/Claude). Item 30 had made `context.compiled` a typed struct, so the lane's `estimate` field
became `EstimateSummary`, with `CensusSummary`, in theseus-protocol: the one definition of its shape. It is
built by `Estimate::summary`, optional and absent in older rows. The wire test's literal gained
`estimate: None`, so its fixtures keep their bytes, and both types joined the generated TypeScript. The join
is folded into the lane's first commit (7335e15), and the message says so.

**The review's check of the release build** (042ff07, installed 15:42; the lane's driver on a scratch daemon,
$0.028):
- Sonnet 5.5's first requests, from bytes, were 3.5 to 4.3 % over the count: 5,473 against 5,248; 7,461 against
  7,208; 5,487 against 5,265;
- its counted requests were within 0.5 %: 6,337 against 6,370, and 6,390 against 6,386;
- GLM's first requests were 1.9 to 2.3 % over;
- GLM's counted ones were within 1.7 %: 4,288 against 4,363, 4,381 against 4,375, and 5,136 against 5,211;
- every bound covered its count.

**Known gaps.**
- ~~theseus-9p88, raised to P2 at the review for the next fix batch: a provider's "prompt is too long" and
  `model_context_window_exceeded` have no handling, so a session past its window is refused every turn until a
  manual recompile.~~ Built in Item 34.
- theseus-c5ba (Haiku 4.5's figures from one request), theseus-kdkv (the Observatory's view of the estimate),
  theseus-vj9q (a recompile's bytes-only estimate rings at about 71 %), and theseus-p171 (hex and base64 as a
  class), all P3.

### Item 32. The reader rule: a registry test in the gate, and a marker on every crate ahead of its reader (theseus-wjy; row 6 of the re-cut; 2026-10-01 15:47 to 16:20, reviewed 16:33 to 16:37; 70bc0b4, cf015ef, aebd530)

**Why.** The owner's decision 3 of theseus-vmh (option A, 2026-09-27 21:10), P0's rule 3: nothing declared without
its reader. The lane recipe already merged crates ahead of their readers (Item 16) and said Part III lists each.
Nothing checked it, and 13 of `main`'s 21 crates sat unread.

**What landed.**
- **The markers** (70bc0b4). Each of the 13 crates that `theseusd` and `theseus` don't reach says so in its own
  manifest, under `[package.metadata.theseus]`. Twelve carry `reserved_for = "row <n> (<step>), <milestone>: <its
  reader>"`. `theseus-sim` carries `tool = "<what runs it>"`: an installed binary of its own, and so a reader of
  its own.
- **The registry test** (cf015ef), `tests_registry` in theseus-core. `scripts/gate.sh` runs it alone after clippy.
  Its 8 tests take 0.11 s. It checks:
  - **crates**: reached by normal dependencies from `theseusd`, `theseus`, or a tool, or else marked. It reads the
    `Cargo.toml` files, because `cargo metadata` takes 0.84 s;
  - **methods**: an in-process core answers every `method::ALL` with anything but "method not found";
  - **notifications**: every `notify::ALL` has its `Event`, and every `Event` a construction in theseus-core's
    non-test code;
  - **edge kinds and labels**: `graph::EdgeKind` and `graph::Label` are new and empty. Each variant needs a
    `match` arm, a pattern, or an `==` in code the binaries run. _(`graph::Label` was removed in theseus-i25g, Item 70: a label is a field of the node.)_

  A marker on an item that has its reader fails as stale. The scan reads Rust tokens (proc-macro2), so strings and
  comments never count, and `#[cfg(test)]` code is skipped. To make the lists complete by construction, `method`
  and `notify` are each one table now, with `ALL` built from it (`method::ALL` was hand-kept). `Event::VARIANTS`
  comes from `events!`'s table.
- **Its words** (aebd530): each failure names what to add (an arm, a sender, a reader) and the marker that would
  reserve it instead, and the gate's step reports every miss at once.

**Reserved, as of this item** (`print_the_reserved_list`):

| Item | Row | Milestone | Its reader |
|---|---|---|---|
| crate `theseus-sandbox` | row 17 (17b) | M4 | proc.run's L1 path in the job wrapper, under [sandbox]; the egress proxy at row 19 (18c) |
| crate `theseus-ontology` | row 26 (21b) | M4 | the ontology's records, its snapshot, and the compile walk in theseus-core |
| crate `theseus-aws-catalog` | row 29 (C1, 14a) | AWS | theseus-aws's operation models, under aws.call and aws.describe |
| crate `theseus-aws` | row 29 (C1, 14a) | AWS | aws.call for reads, aws.describe, aws.whoami, and aws.s3.list |
| crate `theseus-aws-guard` | row 30 (C2, 14b) | AWS | the gate's check of each AWS write and template, from stacks and theseus aws bootstrap on |
| crate `theseus-judge` | row 37 (23a) | M5 | JudgeService under [judge] in theseus-core, with loop.v1 in shadow |
| crate `theseus-follow` | row 51 (29b's wire-in) | M6 | the index tender's WAL follower; the durability tender's at row 32 (15) |
| crate `theseus-index` | row 51 (29b's wire-in) | M6 | theseusd spawns it as the index tender after serving; theseus index status and search |
| crate `theseus-memory` | row 52 (30a) | M6 | the recall step in shadow, and memory.search; FSRS-6 and activation at rows 58 and 59 (32a, 32b) |
| crate `theseus-exam` | row 55 (34b's wire-in) | M6 | the exam's harness over the real pipeline, through turn.submit's memory_arm |
| crate `theseus-mcp` | row 66 (36b) | M7 | McpBoard and [mcp.servers], MCP tools in turns; the server side at row 72 (41b) |
| crate `theseus-voice` | row 77 (44b) | M7 | /join, and utterances into turns; speech as spend at row 78 (45b) |

_Read since (v0.77): `theseus-aws-catalog` and `theseus-aws` at C1 (Item 49), `theseus-follow` and `theseus-index` at row 51 (Item 50), and `theseus-sandbox` at 17b (Item 58)._ _And since v0.80: `theseus-aws-guard` at C2 (Item 78)._

Tools, readers of their own: `theseus-sim` (the gate's lifecycle bench, the crash test, kernel-sim, and the fake
Discord). No method, notification, edge kind, or label is reserved: every one has its reader.

**The proof.** Four planted failures, each run once and reverted, each failed with its item and its fix named:
- a crate with no reader;
- a method with no arm;
- a notification with no sender;
- an edge kind with no reader.

Three more failed too: a notification name with no `Event`, and a stale marker on `theseus-store`, which the test
reported with its path (`theseusd → theseus-store`). The third extra, an edge kind with a reader, passed.

**Divergences from the plan.**
- Hooks are gone (theseus-hco), so the test enumerates crates, the protocol, edge kinds, and labels instead.
- Tools are roots: what an installed tool runs counts as read. At row 51 the index tender, a spawned binary, reads
  `theseus-follow`.
- AWS's crates name `AWS` as their milestone. Block D has none.
- Config keys (the brief's optional fourth check) aren't checked: theseus-obmo.
- Four spellings fail open: theseus-g7qp. _(Two since 2026-10-05, Part III Item 182: holes 2 and 4 closed; holes 1 and 3 stay open.)_
- Rule 1 (routes) stays the reviewer's.

**The review** (`~/reports/theseus-wjy/review/review.md`). Both calls are kept: tools are roots, and the AWS crates
name their own milestone. The gate rerun on `main` at aebd530 was green: the registry's 8 tests, the suite's 1,271,
and the bench on its rerun after one swap outlier. Carried forward:
- `session.wait` (row 10) must reject empty params, or the methods check would wait out its bound (on theseus-in3);
- ~~12a's first edge goes into `graph::EdgeKind` (on theseus-n4m);~~ done in Item 38 (`derived_from`);
- 19a's first labels go into `graph::Label`. _(Not so: 19a made a label a field of the node, `Node.label`, of the protocol's `Label` type, and `graph::Label` is still empty; Item 61. It was removed in theseus-i25g, Item 70.)_


### Item 33. The protocol push: `attention()`, `execution.changed` and `executions.watch`, `session.wait` (theseus-in3; rows 8 to 10 of the re-cut, steps 9a, 9b, 9c; 2026-10-01 17:13 to 18:54, reviewed 18:55 to 18:57; d9944e7, d14248f, f26c8dd, ed80fba; installed 19:06 at 1970bc6)

**Why.** Stage B's first step, and Appendix F's four adoptions: `execution.changed`, a watch over every session, a
daemon-owned `session.wait`, and one `attention()` for every surface. Before it, an execution's changes were
reported nowhere:
- every client polled, and the web UI polled `session.list` every 5 s, each call decoding every action ever
  written;
- no client learned of another session's change;
- each connection's queue was unbounded, so a stuck client grew the daemon's memory without bound and never
  learned it had fallen behind.

The TUI and the herdr adapter need exactly this.

**What landed** (§3.14, §3.15, §3.18, §9).
- **9a** (d9944e7): `Level`, `Attention`, `WaitingOn`, `PendingConfirm`, `ExecutionView`, and `attention()` (the
  design's 14 rules, first match wins) in `theseus-protocol`, generated into the web apps' TypeScript; `attention`
  on `session.list`, `execution.list`, `task.list`, and `session.history`'s session; the typed `waiting_on` on
  `ExecutionInfo`; the pills (● ◐ ○ ·) in the CLI, the web UI, and the cockpit.
- **9b** (d14248f):
  - `Kernel::observe`, one observer, shared by every view and run after each append. Until something watches,
    the commit path pays one `OnceLock::get`;
  - the board (`theseus-core/src/push.rs`), seeded on the first watch, with an overlap rule per entity, so a frame
    landing during the seed is neither lost nor applied twice;
  - `execution.changed` to the session's watchers and every all-session watcher, once each;
  - `executions.watch` / `unwatch` and `session.list { ids }`;
  - health's `push`, the metrics `theseus.push.events` and `theseus.push.delay_ms`, `theseus watch --all`, and the
    lifecycle bench's `seed` phase.
- **9c** (f26c8dd, ed80fba):
  - `session.wait` (blocked, settled, terminal; `already`, `after_position`, a timeout, 64 a connection);
  - the backlog cap and `events.lost` (`outbound.rs`), with `theseus.push.lost`;
  - `theseus wait` (exit 4 on a timeout) and `theseus executions explain`;
  - the web UI and the cockpit's lists on the push;
  - two fixes 9b's live check found: the board applies only a frame's last record per action, and a job its turn
    left running keeps its session working (`waiting on 1 call`), not ready.

**How it is proven.**
- **The prove** (`theseusd/tests/push.rs`): a turn, a job past `proc_sync_secs`, a confirm, a task, a wake, a stop,
  and a cancel, with an all-session watcher connected first, checked against the WAL's own frame boundaries (62
  state rows over 5 executions). Every row that changes an execution's state has its event from its own frame, and
  every event has its row. A planted observer that hears only frames with an `execution.*` row fails it; so does
  the old first-record rule.
- **The seed and the race:** a question parked before a restart is in the seed. Sixteen turns racing a watch leave
  the client equal to the board. A plain turn's frame budget (5) holds while the push watches.
- **The waits:** each condition, `already`, a timeout, `terminal` refused for a conversation, empty params refused
  at once (the reader rule's methods check), a closed connection's waits ended, and the cap of 64.
- **The lag prove:** a client that stops reading while 5,000 events pass gets 4,135, then one `events.lost` for the
  other 865, and its re-snapshot equals a fresh client's.
- **Live, on a scratch daemon** (GLM, under $0.01): `theseus watch --all` showed a turn, a confirm, a job and its
  late result, a task, and a stop, with their positions. `theseus wait` returned 16 ms after the asking frame's
  row, and settled 11 ms after the last turn's. The web page sent nothing in 30 idle seconds with its pane hidden.
  Every cockpit view loaded clean.
- **The seed in release:** on the 10,000-session synthetic store, p50 37.1 ms, p95 41.5 ms; on a copy of the owner's
  store at the install, 285 µs.
- **Gates** green at each commit (1,282, 1,291, 1,299, 1,299 tests), and the review's rerun at ed80fba (1,299;
  the bench on its first run).

**Divergences** (the report's table, all kept): the caller passes how to write a time of day, since the protocol
crate reads no clock; `wake_at_ms` and `outstanding` in the view; rule 8 widened to a conversation waiting on input
with calls dispatched; the observer keeps a frame's ledger rows and node positions too; the seed's two reads with a
position per entity; the prove against frames, not a row's position alone; a `seed` bench phase, with no budget
yet; `LIMIT` (-32007); a `policy` stream in `events.lost`; once one notification drops, every one drops until the
queue drains, so a client always hears what it lost; the Observatory's tables keep their timer.

**Known gaps.**
- ~~theseus-jj9f (P2): an answer takes two kernel frames, so between them the session reads `● waiting on you`, and a
  `session.wait --until settled` parked before an answer wakes at the answer. Merging the frames is the kernel
  transaction Review 2 proposed (C6).~~ Built in Item 41.
- theseus-7ovb (what still polls), ~~theseus-hanu (the board falls seconds behind a burst of 5,000 requests; nothing
  lost)~~ (built in Item 46), theseus-2xep (a job's result queues its execution with no `execution.queued` row), ~~theseus-j7xi (the
  Discord binding's test)~~ (built in Item 37), all P3.
- The cockpit reads its four lists whole again on any change, at most every 250 ms. That's fine today, and it goes
  with stage C's paged lists (theseus-lv2). _(lv2's reads by state were built in Item 46; the lists' own reads are theseus-96w2.)_

### Item 34. A session past its window, and a job's output at its edges (theseus-9p88, theseus-gsn9, theseus-ewev; the `overflow` lane; 2026-10-01 15:50 to 16:51, and a finish run for its report 17:13 to 17:32; reviewed 17:35 to 17:45; joined at 714bfc9 and 9558ba8; installed 19:06 at 1970bc6)

**Why.** Three gaps the day's reviews found.
- A session that passed its model's window was refused on every turn until an operator recompiled it by hand: the
  provider's 400 was an invalid request, and nothing read `model_context_window_exceeded` (the tokens review, P2).
- The output cap kept a job's head, so a job that overran it lost its end, where builds and tests print their
  verdict (the fb2c review).
- A job stopped before its completion kept its raw output, which may hold a printed secret, until the hourly sweep
  after its cancelled result (the fb2c review).

**What landed** (§3.16, §3.24, §4.4a).
- **The provider's word** (714bfc9): a refusal ("prompt is too long") and a cut answer are an overflow trigger: a
  ring by the provider's window and count, and one retry. A cut answer is left out of the retry and every later
  request. A retry past the window too, or a ring with nothing to drop, fails the turn as `context_window` and
  parks on input at once, where the driver's one retry would have sent the same request.
- **Both ends of a job's output** (9558ba8): the head in the file as it is read, the last 4 MiB in a ring in the
  wrapper (never the daemon), written at the pipe's end after a marker. The result names the dropped middle, and a
  killed wrapper's lost end. Every byte kept is redacted, the end included. _(Since 18a, Item 60, a stop no longer loses the end: the wrapper outlives the job's tree, so its ring reaches the file.)_
- **A stopped job's raw output** (9558ba8) goes once its cancelled result is written, unless its wrapper lives, and
  the result shows what it printed first, where it said "(no output)".

**How it is proven.**
- Five turn tests through the whole core with a scripted provider, three compiler tests, and a parsing test
  (Part 1).
- The copy byte for byte around a cap, the ring's wraps, a granted job's two ends withheld, the real wrapper past
  64 KiB, a real turn's result ending with the job's verdict (Part 2).
- A real daemon's stop of a job past its head, whose next turn says the end was lost and deletes the file (Part 3).
- 13 of 14 revert probes caught their fix. The one that missed, deleting while the wrapper lives (E2), shows a
  missing test, not a defect: the guard reads right (theseus-667d).
- Live, on Haiku 4.5 with a catalog that said 400,000: Anthropic refused a 209,425-token request against 200,000.
  The turn failed at once with the window's error, and the next message rang and answered, for $0.000278 in all.
  The build before was refused four times and never answered.
- The join's gate on `main` at 9558ba8: 1,312 of 1,312.

**Divergences:** the lane report's tables. The ring's end is min(cap / 2, 4 MiB), so a small cap keeps both ends;
a marker sits between the ends, where fb2c had none; the provider's maximum isn't remembered past the turn.

**Known gaps.** A GLM refusal is read only in Anthropic's wording (theseus-kucs). ~~Two pieces no test proves: the
guard that keeps a stopped job's raw output while its wrapper lives (theseus-667d), and the result's `held` line
(theseus-z3de). All P3, and the last two are in lane `tidy`.~~ Both are proven in Item 37, where the `held` test
found and fixed a wrong count; theseus-kucs is P3.

### Item 35. The `steady` lane: a wrapper caught in its exec, a channel's order, and disk and spool on screen (theseus-mi6a, theseus-50p, theseus-51v8; 2026-10-01 17:47 to 18:39, reviewed 18:57 to 19:00; joined at bc2fdb9, aea37c3, da92475, 1970bc6; installed 19:06 at 1970bc6)

**Why.** Gates now run beside heavy neighbours for hours, and some races show only there.
- A process in the middle of its exec has an empty command line, as a zombie has: 748 of 3,000 reads made right
  after a spawn came back empty at load 14. The sweep test flaked on it (theseus-uev6; its test fixed on `main` in
  1fcb990, looking until the stand-in reads as the wrapper). In the product, a stop that came a moment after a
  job's spawn took the wrapper for another process holding the pid, sent no signal, said the job was gone, and the
  job ran on.
- A call that waits on approval had its card written at once, and its tool line drawn only at the place's next
  tick (1.2 s). The lane sends posts before live progress. So a channel read card, reply, then tool line: 8 of 8
  runs, once the test asserted the order.
- Health's `disk` and `spool.last_sweep` (Item 25) were on the CLI only.

**What landed** (§3.16, §3.14, Delivery).
- **mi6a** (bc2fdb9): `job::holder` tells what holds a wrapper's pid: the wrapper, a process still starting
  (live, not a zombie, not a kernel thread, its command line empty), another process, or none. `wrapper_alive`
  counts a process still starting as alive, so every reader that keeps or waits inherits it, lane overflow's
  `remove_job_output` included. A stop signals a job only once its pid reads as the wrapper, or as no live process.
- **50p** (aea37c3, 1970bc6): the renderer draws a call that asks at once, on `ConfirmRequested`. The place then
  tells its lane (`Asked`), and the lane holds a card written while it runs, for a session a place here renders,
  until that word comes, at most 5 s, and then sends it anyway with a WARN. A channel reads the turn's text, the
  call, then its card. The reply's footer rides on the text above them, as it did for any turn that ended after a
  tool call.
- **51v8** (da92475): the cockpit's Systems view has a Disk · spool card (a free-space meter with the warning's and
  the floor's lines, toned by state, and the last sweep by why). Its status strip has a disk dot and, while the
  disk is low or below the floor, an attention item. The Observatory's health panel has the CLI's two lines, the
  disk as a native meter.

**How it is proven.**
- The decision over every state: the wrapper, another program, an empty command line in each run state, a kernel
  thread, a zombie, no process.
- Two stops of a child held in the exec's state, its argument pages unmapped to the stack's end: one becomes the
  real wrapper, which the stop ends with SIGTERM; one becomes `sleep`, which it leaves running. Each fails against
  `main`'s rule, and against a stop that signals at once (which kills the unrelated process).
- The channel test asserts the order: 8 of 8 failures against `main`'s code, 10 of 10 passes, and 6 of 6 at nice 19
  beside busy loops at nice 5. The card's hold has its own test as a pure rule.
- Live, on a scratch daemon of the lane's build on port 7439, with a stand-in `op` and no secrets: low, below the
  floor, and ok, each screenshotted, and every cockpit view clean.
- The join's gate on `main` at 1970bc6: 1,317 of 1,317.

**Divergences.** The issue offered the wrapper's start time in the spool. The lane kept the spool's format and
looks again instead, since the exec's window closes on its own (84 ms at the worst, measured starved).

**Known gaps.** ~~A stop that lands before a job's pid is in the spool, while `run_job` awaits the broker, settles the
call cancelled, and the job then launches anyway (theseus-36to, P2; in lane `tidy`).~~ Built in Item 37; the window
was the launch's own work, not the broker's wait.

**The install** (19:06:14 at 1970bc6, Items 33 to 35). Store backup first, then a release live check on a copy of
the owner's store:
- health's push, disk, and spool lines;
- `watch --all`'s snapshot (5 executions, seeded in 285 µs);
- the pills;
- a GLM turn ($0.0005) and `theseus wait --until settled`;
- `executions explain`;
- all six cockpit views clean, and the Observatory.

It surfaced one thing: the owner's Discord DM session from 2026-09-29 still reads `● budget exhausted`, from the old
units limit, with $99.58 of its $100 left (theseus-3ebd, P3). _(Item 53 reopens such an execution at the next start.)_

### Item 36. The CLI's client library: the connection and the renderers move out of the binary (theseus-7yx, step 10a; row 11 of the re-cut; 2026-10-01 19:11 to 19:56, reviewed 19:59 to 20:01; 7e0325f, 458e5aa, a0f594f; installed 20:08 at 9637142)

**Why.** Step 10a of the stage 2 design (§2.9). The TUI (10b to 10e) and herdr's `watch --interactive` (11a) are
clients of `theseusd` too, and they needed what the CLI kept in its binary: the connection (`Conn`, in `main.rs`)
and the words for each event, node, and question (`render.rs`, whose `Printer` printed as it rendered). The step
ran in the spine's slot because it restructures `main.rs`, which every spine step touches, and both lanes branch
from it.

**What landed** (§3.18).
- **A library in the `theseus` package, `theseus_client`** (`lib.rs`), beside the binary:
  - `client`: `Conn` (`socket`, `spawn`, `over`, `send`, `next`, `call`, `request`) and `CallError`, moved from
    `main.rs` with their error text unchanged. It prints nothing and has no clap. `over` takes any pair of
    streams, so a test scripts a daemon over a duplex. `next` is cancel-safe: it reads with `read_until` into a
    buffer the `Conn` keeps, where `read_line`, dropped mid-line by a `select!` that another branch won, lost
    what it had read.
  - `render`: `Line { tag, text }`, with ten tags drawn from the `Printer`'s own marks (`Plain`, `Reply`,
    `Thinking`, `Tool`, `Ask`, `Ok`, `Warn`, `Bad`, `Dim`, and a pill's `Level`). `event(e, Show)` is the
    `Printer`'s match without the printing. The renderers that wrote to a writer return lines, split at their
    text's newlines, so the bytes are the same.
- **The binary** keeps `main.rs` (clap and dispatch, 537 lines to 400), `cmd.rs`, and `print.rs`, a thin printer:
  the CLI prints a line's text, and the TUI styles it by its tag.
- **`tests/connect.rs`**: `--spawn`, with a shell script standing in for `theseusd --stdio`, and exit 3 for a
  daemon that can't be reached. No test had covered either.

**How it is proven.**
- **Goldens first** (7e0325f): four for the branches of the `Printer`'s match that no golden reached. After the
  move, all 40 golden tests (87 files) passed unchanged.
- **Unit tests** of the tags, and of the client: a `next` cut mid-line by a timeout loses nothing; a call's
  notifications and an error answer over a duplex.
- **Revert proofs**, each restored with a fresh mtime: the `Thinking` label changed failed three goldens and the
  printer's test; `next`'s old `read_line` failed the cancel-safety test ("bad line from server").
- **The reader rule**, alone, 8 of 8. The CLI's package built static-pie for `x86_64-unknown-linux-musl`.
- **Live**, on a scratch daemon over a copy of the owner's store: 16 read commands gave the same bytes with the
  installed CLI and this one (32 files, a 2,589-line history among them). Both CLIs watched one session through
  two GLM turns and printed the same bytes. `--spawn` worked with both.
- **Gates** green at each commit (1,319, 1,326, and 1,328 tests; the third's bench passed on its rerun after a
  miss at load 15.6), and the review's rerun at a0f594f (1,328 of 1,328, the bench on its first run).

**Divergences** (the report's table, all kept at the review). The library is `theseus_client`, not `theseus`: a
lib and a bin of one name collide in `cargo doc --bins` (cargo#6313). `Conn::over`, a public `send`, and the
cancel-safe `next` were added for the TUI's loop. A line's parts and the one-line answers only the CLI prints stay
`String`s. The `Printer`'s `✗` is two tags, `Warn` for a decline (the operator's own choice) and `Bad` for a
failure, and health is all `Plain` until a surface shows it.

**Known gaps.** `theseus watch` leaves a reply's open line unended when the daemon closes the connection, which two
goldens pin (theseus-1n2l, P3). The install builds glibc binaries, not the static musl binaries §3.18 said:
v0.76 corrects §3.18's wording.

### Item 37. The `tidy` lane: a stop during a job's launch, and three proofs (theseus-36to, theseus-667d, theseus-z3de, theseus-j7xi; 2026-10-01 19:11 to 19:58, reviewed 19:59 to 20:01; joined at de3c3d5 and 9637142; installed 20:08 at 9637142)

**Why.**
- The steady lane (Item 35) found that a `/stop` or a cancel that landed after a job's dispatch, and before its
  pid was in the spool, settled the call cancelled and signalled nothing. The job then launched anyway and ran
  unstopped (theseus-36to, P2).
- Three earlier lanes each left a rule without a test: a stopped job's raw output kept while its wrapper lives
  (theseus-667d, from theseus-ewev), the result's `held` line (theseus-z3de, from theseus-gsn9), and the Discord
  binding ignoring `events.lost` and `execution.changed` (theseus-j7xi, from theseus-in3).

**What landed** (§3.16, §3.19).
- **36to** (de3c3d5), in `run_job` alone. The start reads the job's action just before the launch, and again once
  the pid is written.
  - A job that a stop or a cancel reached before its launch is never started. Its result reads "Not run: stopped
    by …", as for any call a stop caught before it ran, and a `job.not_started` row records it.
  - A stop that came during the launch, and found no pid, is carried out by the start itself (`Stopping`), with a
    `job.stopped_at_launch` row.
  - The stop writes its mark, then reads the pid; the launch writes the pid, then reads the mark. So one side
    always sees the other.
- **The issue's premise, corrected.** A turn waits for every secret's first round before it asks the model,
  since the scrubber needs each value. So the broker's wait on a secret is never reached through a turn. The
  window was the launch's own work after the dispatch: the disk check, the broker's read, the wrapper's
  arguments, and the spawn, a few milliseconds.
- **667d, z3de, and j7xi** (9637142): a test each. z3de's test found a real defect. A job past its head whose
  child still held its output reported no ends, so its result's cap line counted the wrong size ("it printed
  100,087 bytes" for 100,005). The wrapper's open-output report now names its two ends as they stand (9 lines in
  `job.rs`).

**How it is proven.**
- Two in-process tests hold the job's start with a rendezvous, at its disk check or inside its launch, and send
  the stop through the real stop path at that point. Both fail on the previous `toolrun.rs`, and each half's
  revert fails only its own test.
- Live, on scratch daemons with a fake model and a fake `op`, their stores on tmpfs, stops sent at random times:
  on the previous build every stop that landed in the window (3 in 1,000 rounds) left its job running to its
  end. On the fix, the two that landed there were caught, one by each half, and no job ran.
- Each of the three new tests is revert-proved. The overflow lane's probe E2, which had passed every test, now
  fails 667d's.
- The lane's gates green on their first run (1,319 and 1,323 tests); the join's gate on `main` at 9637142, 1,334
  of 1,334.

**Divergences.** The brief's live check, a stop while a job waits on a slow secret, can't be built, since a turn
never reaches that wait; the live check raced the launch's own window instead. `stop_launched` repeats about 15
lines of `terminate_all`'s job stop; folding them is optional.

**Known gaps.** ~~A job that reported with its output still open has its raw output deleted under its lingering
wrapper (theseus-5wgd).~~ (Built in Item 54.) ~~A `/stop` handled before the turn of an input sent just before it
is admitted doesn't stop that turn (theseus-hmwv; seen live). Both P3, for lane robust2.~~ (Built in Item 55.)

**The install** (20:08:11 at 9637142, Items 36 and 37). Store backup first, then a release live check: health,
the pills through `theseus_client`, a GLM turn and a wait, six cockpit views, and no errors.

### Item 38. Epidemiology, step 1: reach (theseus-n4m; row 12 of the re-cut, step 12a; 2026-10-01 20:09 to 20:57, reviewed 21:26 to 21:30; 24f5b02, 516bbfc, 715bf80; `theseusd` installed 21:43 at 22670d6)

**Why.** Appendix F's context epidemiology starts with one question: where did a node go? Before 12a no code wrote
an EDGE record, every compilation admitted only its own session's nodes, and the one route between sessions, a
task's report relayed into its parent, left the copy with no link to its source. "Who saw X" and "what came from
X" had no answer but a scan, and the scan couldn't cross a session. Redaction (§5.6) assumes the lookup, and M4's
integrity labels ride the same edge.

**What landed** (§3.18, §6.1).
- **The first edge kind, `derived_from`** (`graph::EdgeKind::DerivedFrom`): keyed `derived_from|<copy>|<source>`
  and scoped `in:<source>`, so one scope scan lists every copy of a node, in WAL order. That scope is the reverse
  column, on the scope index the store already keeps: no new table, and no change of layout. EDGE stays at
  schema 1, which every manifest marks already, so the first edge rewrites none.
- **Two routes write it**, each in the frame that writes the copy: a task's report relayed into its parent (from
  the task's last message), and a task's brief (from the parent's reply that holds the `task.create` call).
- **`node.reach { node_id, max_generations? }`** (`reach.rs`): the compilations of the node's own session whose
  `includes` hold it, and the loops whose context held it; then its copies, generation by generation, each with
  the same; totals of contexts and sessions; and `partial` at the generation cap (3 by default, 16 at most) or at
  256 copies. It is derived when asked, off the serving workers.
- **Surfaces**: `theseus reach <node> [--generations N]`, and a reach cell in the Observatory's Nodes panel and in
  the cockpit's node inspector.

**Why derived, not stored.** §6.1 planned reverse `includes` edges in each compilation's frame. But every
`includes` today is a run of the compilation's own session's nodes, which its record holds, so one read of the
session answers "which compilations include this node". A loop held the node when its compilation's `includes`
holds it, or it sat in the tail. About 300 reverse edges per recompile would have repeated the record in every
recompile's frame. The first route that admits a node from outside its session (M6's recall, M7's borrowing) lands
with its own reverse entries (P0's rule 3).

**How it is proven.**
- **The prove** (`tests_reach`): a task's report relayed into its parent, whose next turn recompiles. `node.reach`
  on the task's last message names both sessions (generations 0 and 1), exactly the one compilation that holds the
  copy, and its 2 loops: 3 contexts in 2 sessions. The edge sits at the copy's position + 1, and `MANIFEST.json`
  is byte-identical after it.
- **Beside it**: the brief's edge; a session with no edges; a turn that reads a report writes no extra frame for
  its edge; over the protocol, an unknown node is `-32002` and empty params `-32602`, at once; an older binary's
  store takes an edge, stays format 2, and opens; a walk over hand-written edges reaches generation 2, walks a
  cycle once, and stops at both caps; the registry sees `derived_from` read; `theseus reach`'s goldens.
- **Live**, on a scratch daemon over a copy of the owner's store: a GLM task reported, and `theseus reach` named both
  sessions. The report's frame was `turn.started+execution+node+edge+task.reports_read`. After a recompile and two
  more turns it named the one compilation that can hold the copy: 4 contexts in 2 sessions. The brief's edge
  reached 7 contexts in 2 sessions. Both web apps' reach cells were clicked and shot. About $0.0024.
- **Gates** green at each commit (1,341, 1,342, and 1,343 tests). The review's first gate missed only the bench,
  at load 11.7, with every p50 about 10 ms up, untouched phases included. The bench alone passed with room, and
  the rerun was green at 21:30:10, 1,343 of 1,343.

**Divergences** (the report's table, all kept). EDGE's first write marks no manifest (the design said once). A
copy's compilations are a list, not a count. `via` is joined by `route` and `from`. The times are absent when
nothing held a node. The brief's edge points at the parent's reply, since a call node renders into no request
and no context ever holds it. No reverse `includes` edges are written. `node.reach` runs on `spawn_blocking`.

**Known gaps.** `theseus reach` takes a node's full id, which only JSON output shows (theseus-glyw, P3). No route
writes a copy of a copy yet, so generation 2 and the caps are tested over hand-written edges only. `node.reach` is
untimed on a long session: stage C's scale work should time it on a large synthetic store.

**The install** (21:43:23 at 22670d6): `theseusd` and `theseus-sim`. The CLI and the TUI waited for 10f's commit,
since the spine was editing both during the release build (Item 42's install). The live check on port 7437:
health, `node.reach`'s refusal of empty params, six cockpit views, and no errors.

### Item 39. The terminal UI: every session in one sidebar, what needs you a key away, answers inline, and done until seen (theseus-7yx, steps 10b to 10e; the `tui` lane; 2026-10-01 20:04 to 21:24, reviewed 21:26 to 21:32; joined at 1879dc9, 7be154f, 22f93c4, 8912b1c, 22670d6)

**Why.** P5c item 7 and the stage 2 design's §2.9 (decision 5 of theseus-vmh, the owner, 2026-09-27: "'A' -- first we
try everything!"). herdr's one bet, never hunt for the stuck one, made a client of the push (Item 33): the TUI
shows each session's attention as the server computes it, and puts the session that needs you a key away.

**What landed** (§3.18; P5c item 7).
- **`crates/theseus-tui`**, the binary `theseus-tui`: ratatui 0.30 with only its crossterm backend. It links
  `theseus-protocol` and `theseus_client` (Item 36), never the core.
- **One connection, one loop**: the cancel-safe `next`, the terminal's events (read on a thread of their own), and
  one deadline, in one `select!`. It redraws on events, at most 30 frames a second, and its only timers are a
  card's countdown, a notice's second, and the seen file's write. A closed connection is tried again with backoff,
  500 ms doubling to 10 s.
- **The board**: `executions.watch`'s snapshot and its events, by the position rule. The first snapshot after a
  connect is the truth, since a daemon restarted on another store has lower positions. `events.lost` reads the
  board again.
- **The sidebar**: each session's pill (● needs you, ◆ done until seen, ◐ working, ○ ready, · idle), its name, and
  its spend. A task sits under its parent and folds once idle and seen, and each tree takes its place by its most
  urgent session. Keys filter it.
- **The detail pane** (`session.history`'s newest 200 nodes, then `session.watch`, in the CLI's words and tags),
  the input line, and stop and cancel, each on a second press.
- **The queue and the card**: `tab` walks what needs you, the longest waiting first, then what is done. The card
  shows the question whole and counts down its expiry. `y`, `t`, and `n` with a note answer it as `action.confirm`
  on the `cli` channel, with `author: "the TUI"`.
- **Done until seen**: a seen file under `$XDG_STATE_HOME/theseus/`, the client's alone, never sent to the daemon.
  A notice (the bell by default) fires once, after a second, for a session that comes to need you or finishes out
  of sight, and the title says `theseus (n)`. Below 100 columns the session's pane takes the screen.

**How it is proven.**
- **25 tests** in the gate, `TestBackend` buffers over a scripted daemon on a duplex (`Conn::over`): the snapshot
  and its events, a stale one dropped; reconnects (the backoff; another store); `events.lost`; the keys; the pane;
  the queue's order; the card and its countdown; each answer; refusals; a budget card; done until seen across two
  restarts; one bell; the debounce; the wrap.
- **Revert proofs**, each restored with a fresh mtime: the reply stream, the answer's author (two tests), and the
  seen file.
- **Gates** green at each commit (1,344, 1,348, 1,353, 1,359, and 1,359 tests); the join's gate on `main` at
  22670d6, 1,368 of 1,368.
- **Live**, on a scratch daemon with glm-5.3-flash, in tmux at 160×48 and 80×24. A `proc.run` tightened to ask
  showed ● and its card; `tab` landed on it; `y` ran the job, and the session went ready. `n` with a note reached
  the model, which repeated the note. An unfocused session's end rang once, with the title at `theseus (1)`. Done
  until seen held across two restarts. The check found one flaw, fixed in 22670d6: a long reason hid the card's
  countdown.

**Divergences** (the report's table, all kept at the review). The arrows move the cursor, and `enter` focuses. `/`
types a text filter, and `b w r d a` are keys of their own. The header's clock is the last redraw's. The seen file
keeps the view last known beside the position displayed. The title counts needs you plus done. Trees sort by their
most urgent session. The thinking isn't shown. The crate's reader-rule marker said `M3`, since the marker's grammar
takes no stage name (theseus-pu9p); Item 41's `tool` marker made that moot.

**Known gaps.** Only a session's newest 200 nodes load (theseus-kym3), and a message typed on another surface can
show after the reply it started (theseus-v6yc). Both P3.

### Item 40. The repository's guides for agents: `AGENTS.md` and `CLAUDE.md` (theseus-7gkd; the `agents` lane, docs only; 2026-10-01 20:26 to 21:03, reviewed by 21:30; joined at d6793c7 and 0f03769)

**Why.** The owner, 2026-10-01 at 20:23: a `CLAUDE.md` and an `AGENTS.md` in the repository that map its files and the
principles discovered while building it, for a smooth move to developing Theseus on Theseus. Every agent that
programs the repository, Theseus included, needs the map and the rules in the repository, not in one agent's
private notes.

**What landed.** 26 new files, and no existing file edited.
- **The root `AGENTS.md`** (19,800 bytes, under the 20 KB that a developing session carries in every turn). It
  holds where the truth lives; the map (every crate with its role, key modules, and readers; the crates merged
  ahead of their readers, with the row that wires each in; the directories; the generated files; where the big
  things live); the principles that bind code, each with its section; the principles discovered while building
  it, each traced to the Item that taught it; the workflow; this machine; and keeping the files current.
- **`CLAUDE.md`** imports it, with two lines only for Claude Code.
- **Twelve folder guides**, for the protocol, store, kernel, tools, core, Discord, `theseusd`, `theseus`, and
  `theseus-sim` crates, the two web apps, and `scripts/`: what is there, its invariants, its tests, and its traps,
  each with a one-line `CLAUDE.md`. They total about 38 KB, read on demand.
- **Public by construction**: the operator's private tools are described, never named by path.

**How it is proven.** A script checked every backticked path and Rust name in the 26 files against the tree: 388
checked, and the 5 it couldn't place were checked by hand. The commands, flags, and protocol names were checked by
hand against clap and the protocol's tables. Proofreading corrected five claims, among them that the start path
writes one frame of its own. Two gates were green in the worktree (1,334 tests), and the join's gate on `main` at
0f03769 passed 1,343 of 1,343. The review found the root accurate and public-safe.

**For Theseus on Theseus.** The report gives a persona block for a builder daemon, a persona whose file is the root
guide. `[context] default_persona` is daemon-wide today, so it goes on a builder daemon, never the operator's daily
one. The root asks each step to update the nearest guide, in its own commit or its docs commit, and each review to
check it: 10f added the TUI's row and its guide (Item 41).

**Known gaps.** A persona per place or per session (theseus-9xli, P2). A directory's guide named the first time a
session's tools touch it, as Claude Code loads a nested `CLAUDE.md` (theseus-ug9i, P3).

### Item 41. `theseus tui`, the kernel transaction, and an answer in one frame (theseus-7yx, step 10f, row 15; theseus-0owd, Review 2's C6; theseus-jj9f; 2026-10-01 21:37 to 22:05, and after a usage limit 22:11 to 23:03; reviewed 23:07 to 23:15; 4c60dc7, f5cb680, b5df0d7; installed 23:26 at 9ac009d)

**Why.**
- 10f joins the TUI (Item 39) as an installed binary that the CLI runs.
- Merging frames had meant one more combined kernel transition per merge (F2, F2b, theseus-l6y): `kernel.rs` had
  57 public functions, and each merge grew the API. Review 2's C6 proposed a transaction instead. The owner accepted
  it with the rest of Review 2 (2026-10-01, 19:54), to open stage C.
- Its first use is Item 33's gap, theseus-jj9f. An answer took two kernel frames (up to four, with a trust), so
  between them every surface read `● waiting on you`, and a `session.wait --until settled` parked before an answer
  woke at the answer.

**What landed** (§3.15, §3.16, §3.18, §8; P5c item 7).
- **10f** (4c60dc7): `theseus tui [ARGS]` execs `theseus-tui`, found beside the `theseus` binary or else on
  `PATH`, as git runs its subcommands, with the CLI's socket first and then every argument as written. `--spawn`
  exits 2. Found nowhere, it says where it looked and how to install it, and exits 2. The CLI gains no
  dependency. The crate's marker became `tool`, which made theseus-pu9p moot. The install copies four binaries
  (`theseus`, `theseusd`, `theseus-sim`, `theseus-tui`), CI's musl artifact gains `theseus-tui`, and the root
  `AGENTS.md` gains its row, beside a guide of the crate's own.
- **The kernel transaction** (f5cb680, `tx.rs`). `Kernel::frame(&[ids], |k| …)` reads each named execution's
  parent, then locks them all in id order. Its transitions stage their records in memory (`Staged`, read before
  the store), each reading what the earlier ones staged. It commits one frame, indexed and observed once, or
  nothing on an error or a panic. Nested, it joins, and a failure there takes back only its own part. A
  transition inside takes no lock: one on the kernel itself panics ("locked twice"), and so does one on an
  execution the transaction didn't name. `Kernel::stage` adds a caller's records.
- **The combined transitions are compositions** of the ordinary ones, their names kept as wrappers for their
  callers. `admit_frame`, `plan_frame`, `dispatch_frame`, `accept_locked`, and `lock_two` are gone. No new combined
  transition is needed again: M4's and M5's frames compose.
- **jj9f** (b5df0d7): an answer is one transaction: the bind or the decline, an approval's trust (under the
  session's lock, taken first), the `action.confirm_answered` row, and the wake. So is the budget question's
  answer.
- **kernel-sim**: a racing thread runs transactions against a turn, and the operator's answer is one frame.

**How it is proven.**
- **The golden**: every frame the kernel commits through a scripted run of its API, every combined transition on
  every branch (411 lines). It was taken on the kernel before the move, checked there, and is byte-identical
  after.
- **`tests_tx.rs`** (7): one observed frame; nothing written on a failure (a planted revert that commits what a
  failed closure staged fails it); a nested part's rollback; staged reads in key order; the lock guards; a turn
  freed after its frame; and no deadlock between two transactions in opposite orders, 2,000 times each on two
  threads.
- **jj9f**: a settled wait parked before a decline, and one before an approval, return after the continuation,
  never at the answer, and each answer is one frame. The test fails on the old answer path, and against a planted
  revert that wakes in a second frame.
- **kernel-sim**, 400 seeds at `--p-race 0.5`: 1,344 racing transactions, 305 one-frame answers, and 1,256
  crashes, with every invariant held. The long crash test, 40 iterations × 3 restarts with torn writes, lost
  nothing committed.
- **10f**: three CLI tests with a stand-in TUI (an exec, not a child: it prints the CLI's own pid), and live,
  `theseus tui` in tmux at 80×24 on a scratch daemon. The static musl build of `theseus-tui` is 2.2 MB,
  static-pie.
- **Live, C6**: a settled wait parked before the approval of a `proc.run` question returned 4,488 ms after it, at
  the continuation's end (`ready`), not at the answer. The answer was one WAL frame of 5 records.
- **Gates** green at each commit (1,371, 1,379, and 1,380 tests). The review's first gate lost one load-sensitive
  test, whose seven stand-in calls must all start within 100 ms (theseus-i1i4, P2, the gate's reliability). It
  passed 5 of 5 alone, and the rerun was green at 23:15:38, 1,380 of 1,380.

**Divergences.** S5, a plain turn in 2 or 3 frames, was in the brief "only if it fits", and it doesn't: a
transaction can't span an `.await`, since its locks belong to the thread that took them (theseus-2uby). Two race
tests pause one read later, for the family read a transaction makes before it locks. The `Store` trait's doc now
promises `latest_of_kind` in key order, which the transaction relies on, and a `debug_assert` checks it. An
approval's trust rides in the answer's frame. `kernel.rs` has 58 public functions, not fewer: what stops is the
growth.

**Known gaps.** theseus-2uby (S5); theseus-6qwr (jj9f's second shape: a late result's wake comes in a frame after
the turn's end); theseus-oqxw (a lock taken while the thread holds another isn't caught, only the same id). All P3.

### Item 42. herdr: the reporter in `theseus watch`, `watch --interactive`, and `theseus herdr sync` (theseus-l1l; steps 11a and 11b, the `herdr` lane; 2026-10-01 21:37 to 23:04, two runs (the first ended at the 22:05 usage limit); reviewed 23:16; joined at 66d6880 and 9ac009d, step 11c; installed 23:26 at 9ac009d)

**Why.** herdr, a terminal workspace manager, shows each pane as working, blocked, or done, and lets a program report
its own state instead of reading its screen. theseusd knows each session's state exactly (Item 33's `attention()`),
so a session in a herdr pane can show its true state beside the operator's other agents. The research behind
Appendix F recommended a thin adapter, with the TUI as the product.

**What landed** (§3.18; P5c item 4). All of it is in the CLI: nothing in theseusd knows herdr.
- **11a** (66d6880). Inside a herdr pane, `theseus watch <session>` reports the session's attention as the pane's
  state, over herdr's socket API (`pane.report_agent`, source `custom:theseus`): needs you is `blocked`, working is
  `working`, and ready and idle are `idle` (herdr shows `done` until it is seen).
  - It reports only a change, with a `seq` from the clock, and metadata beside it: the title, `theseus: <label>`,
    and the tokens `session`, `cost`, and `confirm`.
  - It releases the pane on Ctrl-C, SIGTERM, a hangup, the daemon's end, stdin's end, and a panic.
  - `theseus watch --interactive` asks each waiting question, `approve? [y/N/t/note]`, and sends any other line as
    a message, saying `working` at once. Its answers go through the daemon's socket, the CLI's channel.
- **11b** (9ac009d). `theseus herdr sync [--pin SESSION]... [--close-finished]` gives each session that needs you
  or is working, and each one pinned, a pane in herdr's `theseus` workspace, running that watch.
  - A pane is found by its `$session` token and checked by its foreground process. A dead watch's stale state is
    swept, and the watch is restarted in its own pane.
  - Sync types only at a shell's prompt, never makes a second pane for a busy one, leaves another daemon's panes
    alone, and closes finished sessions' panes only when asked. A sync with nothing missing only reads.
- **herdr 0.9.1**, built from a reviewed checkout, installed beside `theseus` with its update and manifest checks
  off, so it fetches nothing.

**How it is proven.**
- **The tests**: a fake herdr socket records every NDJSON line it gets: the mapping, change-only reports, `seq`,
  the release on each way out, the answers and the messages. A fake herdr with a model of its panes runs sync
  against a scripted scene (the design's prove): exactly the 19 expected requests, then only reads; a busy pane, a
  dead watch, another daemon's pane, and `--close-finished`. 37 planted reverts each failed their tests (16 for
  11a, 21 for 11b).
- **Live**, with the real herdr in a private tmux and a scratch daemon of the lane's build. A GLM turn's
  `proc.run`, waiting for approval, showed `blocked` 2 ms after its `tool.confirm_requested` row (the prove asks for
  1 s). `y` in the pane approved it over the daemon's socket, and herdr went working, then done. A second sync
  changed nothing (3 reads). After a `kill -9` of the watch, the next sync swept its state and restarted it in the
  same pane. The check found one bug, a pane titled by the first prompt instead of the label, fixed with a test.
  $0.0142 in all.
- **Gates**: 11a's green (1,384 tests). 11b's niced gate passed its suite (1,393) and missed only the bench beside
  nice-0 neighbours; the bench alone at normal priority passed. The join's gate on `main` at 9ac009d, 1,405 of
  1,405.

**Divergences** (from the design's §2.10). `seq` comes from the clock, since herdr keeps a source's last `seq` for
the pane's life, past a release. The watch names its own pane, since herdr names only an agent. The watch's first
read is `session.wait { until: settled, timeout_ms: 0 }`, since `session.watch` alone doesn't seed the push. One
source carries the state and the metadata. A hangup releases the pane too. Sync types only at a shell's prompt (a
new shell's startup files may ask for a passphrase first), and runs this binary by its whole path, on this
daemon's socket. A split takes the largest pane.

**Known gaps.** ~~theseus-tq04 (P2): `session.watch` alone doesn't seed the daemon's push, so a watcher on a daemon
that nothing else watches gets no `execution.changed`. The CLI works around it, and the TUI and herdr both rely on
the push.~~ (Built in Item 54.) Then theseus-d5oc (a pin isn't remembered), theseus-pkaq (no herdr plugin: nothing runs sync after herdr
restarts, and no key answers the focused pane's question), and ~~theseus-nu3z (a message typed in a pane takes the
daemon's live profile, not the session's; it goes with the persona work, theseus-9xli)~~ (built in Item 54), all P3.

**The install** (23:26:54 at 9ac009d, Items 38, 39, 41, and 42): the CLI (`tui`, `reach`, and `herdr`),
`theseusd` (C6 and jj9f), `theseus-sim`, and `theseus-tui`. The live check: a GLM turn, a wait, and `theseus tui`
drawing the owner's sessions from a store copy. With it, stage B was complete: rows 8 to 16.

### Item 43. One typed fact per thing that happened (theseus-j6qn; Review 2's C2; 2026-10-01 23:28 to 2026-10-02 01:22; reviewed 07:36 to 07:48; b658257, a653cc0, be0deba, 7e2a89d, 5a22ed0, 4eb6db2; installed 07:47 at 4eb6db2)

**Why.** Review 2's C2, accepted for before M5. Each event's ledger row, notification, narrative sentence, and span
were written by hand at its site, often at different points, so they could drift apart, and a reader gathered one
fact from five channels across a file. `turn.started` alone was written three ways. M4's labels and M5's judgment
rows would have added more sites.

**What landed** (§3.18, §3.20, P2b).
- **The output golden first** (b658257, `tests_output.rs`). Two scripted worlds run through whole cores, driven as
  the daemon drives them: a plain turn, tool calls, a background job and its late result, an approval and a
  decline, a stop, a refused provider and its retry, a task and its report, a wake, and a budget question. It
  records every frame read back from the WAL, with its ledger rows and outbox posts whole, every notification,
  every narrative line, and each turn's span tree, masked for clocks and aliased for ids. That is 1,434 lines,
  covering 37 ledger kinds, 14 notification methods, and all 8 narrative parts.
- **Facts** (`crates/theseus-core/src/fact/`). The `Fact` trait is implemented by one struct per fact: its
  associated constants are its ledger kind and its notification's method, and its methods build its row, its
  notification, its sentences, and its span. The recorder, `Rec`, says whose a fact is, who hears it, and where
  its row goes. Recording writes no frame, so a row rides in the frame it rode in before. There are 89 facts in
  four files: the turn (47), tool calls (25), answers (9), and the driver (8).
- `turn.rs`, `toolrun.rs`, `rpc/confirms.rs`, `rpc/driver.rs`, `task.rs`, and `wake.rs` write no channel by hand.
  The six went from 9,274 lines to 8,249, and `fact/` is 3,276.
- **The ledger's kinds as a registry** (5a22ed0): `theseus_protocol::LedgerKind`, 103 kinds, one line each, with
  the old names of renamed kinds. Every row writer in the workspace names a variant, the kernel's included. A row
  stores its kind's name, so its bytes don't change and no store version is needed; an old or unknown kind still
  decodes by name. The registry test fails a kind that nothing writes.
- The metrics stay one projection of the turn's end (`Telemetry::record_turn`), made from its result and the trace
  the facts drew.

**How it is proven.**
- **The golden** stayed byte-identical through all five conversions: it was written once, at b658257. It is
  deterministic: five runs byte-identical, one of them under a 60-character `TMPDIR`, beside the lanes' builds.
- **Revert proofs**: a row kind renamed and a sentence changed each failed the golden at its line; so did the
  recorder dropping every row (line 15) and a fact's kind renamed (line 16); and an unwritten registry kind failed
  `every_ledger_kind_is_written`.
- The frame budget tests, the CLI's goldens, the wire fixtures, and C6's kernel golden are unchanged, and the bench
  passed at every gate.
- **Live**: one GLM turn with an approved `proc.run`, on scratch daemons of the installed build and of C2's, each
  over a copy of the owner's store. The 39 ledger rows and the 27 narrative lines were identical, and the
  notifications too, but for where GLM chose to stream thinking in one run.
- **Gates** green at each commit (1,406 to 1,410 tests), and the review's rerun at 4eb6db2 (1,410 of 1,410, the
  bench on its first run).

**Divergences.** One struct per fact behind a trait, not one enum: a fact's projections sit in one `impl` block,
and `FACTS` lists every fact's kind and method without building one. Where one event's channels were written on
both sides of a frame or an `.await`, it is two facts, each recorded where its channels were (`TurnBooked` and
`TurnEnded`; `LoopOpened` and `LoopStarted`), which kept every row in its frame. One error-path line moved: a call
superseded by new input is declined, then said, so a decline that fails no longer says "declined".

**Known gaps.** The core's last hand-written channel sites, about 30 (startup, the config gate, the web UI,
restore, policy and trust, `rpc/methods.rs`), each to move once the golden drives it (theseus-056u). Two record
nits kept byte-identical on purpose: no `loop.ended` row on a budget-question end, and stray spaces in the
vault-wait line (theseus-nhg4). Both P3, post-v1.

**The install** (2026-10-02, 07:47:53 at 4eb6db2). Store backup first, then a live check on a store copy: health
clean, a GLM turn whose 14 ledger rows came through facts, a wait, the web UI and the cockpit, and no errors in the
log.

### Item 44. The `hardening2` lane: Review 2's security items (theseus-8dg0; R9, H9, H7, and considerations 2, 3, and 5; 2026-10-01 21:37 to 23:55, two runs (the first ended at the 22:05 usage limit); reviewed 2026-10-02 07:38 to 08:25; rebased onto 4eb6db2 as 8637134 to a704a77, with two join fixes, a26d04c and 59a72b8; joined 08:25 at 59a72b8; not yet installed)

_Correction (v0.77): installed with 9f4035b on 2026-10-02 at 14:23, the install that also made the owner's daemon a systemd user service (Item 56)._

**Why.** Six of the Review 2 items the owner accepted on 2026-10-01 at 19:54:
- R9: an `fs.read` of a FIFO waited forever for a writer, holding a pool core and a thread for the daemon's life;
- H9: the scrubber caught exact values and seven token prefixes, but not a value encoded, an AWS key, a private-key
  block, or a JWT;
- H7: a launcher's grant reached whatever a call could make the program run: a `gh` alias or extension, `git -c`,
  a helper URL;
- consideration 2: with no `[approval]` section, every surface answered;
- consideration 3: nothing said when jobs can write the running `theseusd`;
- consideration 5: `shellexpand` brought MPL-2.0 in, through `option-ext`.

**What landed** (§1, §3.9, §3.19, §3.24).
- **R9** (8637134). Every tool that reads a file opens it through one function, which refuses a FIFO, a socket, or a
  device by name, opens without blocking, and checks again once open: `fs.read`, `fs.edit`, `fs.patch`, `fs.grep`,
  `text.diff`, and `git.diff`'s reads. `fs.read` reads through its cap, so a file that grows past 16 MiB is refused,
  never read whole.
- **H9** (1c4001d). The scrubber also withholds each board value in base64 (either alphabet, at any offset inside
  a longer encoding, wrapped across lines) and percent-encoded; AWS access key ids, and the secret keys and session
  tokens beside or after them; private-key blocks (a public key is kept); and JWTs.
- **H7** (6982113). A grant reaches the program the call names, and what that program runs on its own account,
  never a program the call names to it. A call that sets its own environment gets no grant. `gh` gets its grant
  only for its own commands that use a token, and `git` only for the commands that reach a remote, never through an
  alias, `-c`, an option that names a program, or a URL git hands to a helper. The call still runs, without the
  variable, and its result says why.
- **Consideration 2** (3bf3097). With no `[approval]` section, only the owner answers, through the CLI or a Discord
  DM the bindings file binds. The web UI and a guild channel answer once the section names them, and health says
  `approval: open` while more than that may answer.
- **Consideration 3** (a4d740d). Health's `binary` line says when this daemon's user, as which jobs run, can write
  the running `theseusd`, or rename another over it. It is read when asked, never on the start path.
- **Consideration 5** (a704a77). `shellexpand` is gone: a few lines expand `~` and `$NAME` in configured paths, the
  gate's path check keeps `~` alone, and the CLI has its own `tilde`. MPL-2.0 is off `deny.toml`'s general list,
  but the voice engine's songbird brings seven MPL-2.0 crates, which are allowed by name until the owner decides
  (theseus-yl5w, P2).
- **At the rebase** (a26d04c; the review named its first form, 346f2a3): lane herdr's `herdr sync`, which landed
  after this lane branched, called `shellexpand`; it uses the CLI library's `tilde` now.

**How it is proven.**
- **Each item is its own commit, with a test that fails against a planted revert**: `fs.read` put back to
  `fs::read` fails at the test's 5 s guard (which opens the FIFO's other end, so a revert fails rather than hangs
  the suite); no encoded pass and no shapes fail all six new scrubber tests; the launcher check switched off fails
  its unit test and the daemon's; the old open default fails three tests; a binary check that never says
  `jobs_can_write` fails its test; and the core's path expansion in the gate fails the path test.
- **Live**, on a scratch daemon with a fake model and a fake `op`. Health without `[approval]` said only the CLI
  and a Discord DM answer, and its binary line was loud. An `fs.read` of a FIFO returned its refusal in 0.12 s. A
  job that printed an invented AWS-shaped key pair returned both redacted. With the section naming the web UI,
  health said `approval: open: web may answer`.
- **The lane's gates** green at each commit (1,370 to 1,386 tests). Five runs lost one timing test or the bench
  beside nice-0 neighbours, and each passed alone or on its rerun.

**Divergences.** H7 also withholds a grant from a call that sets its own environment, which Review 2 didn't name: a
list of dangerous variables can't be finished. R9 covers every file reader, not `fs.read` alone. Consideration 2
also made a section with no `channels` key mean the CLI and a trusted user's DM, so the web UI is named either way.
Consideration 5 keeps the gate's path check to `~` alone, and couldn't make §1's licence rule hold: the voice
engine, merged after the review, brings MPL-2.0 crates of its own.

**Known gaps.** theseus-yl5w (P2: the licence, the owner's decision). ~~theseus-ur1t: a granted git's hooks and the
programs its config names, and the git that gh runs, still get the variable; at L0 a job that can write those can
also read the process's environment, and L1 is the boundary.~~ (Built in Item 55: a granted git runs no hooks; what
its config names still reaches the variable at L0, theseus-ngz5.) ~~theseus-txvt (grants to other launchers)~~
(built in Item 55), ~~theseus-ck0k (no end-to-end test of a card that stays in a trusted guild channel)~~ (built
in Item 47), theseus-5b9m (R9 ends a FIFO-based harness
for cancel races), and theseus-od13 (the web apps don't show the two new health lines), all P3.

**The join** (2026-10-02 08:25, at 59a72b8). The rebase onto C2 met two conflicts, both in the core's `AGENTS.md`,
and kept both sides. It took three join gates. The first failed clippy (`needless_borrow`) in the rebase fix's
first form, 346f2a3, which was made again as a26d04c. The second failed on C2's output golden (Item 43), which
recorded a test's bare answerer as `"via":"unnamed"`. Consideration 2 makes that label answer as the CLI, so three
`action.confirm_answered` rows (the approval, the decline with a note, and the budget reset) now say `"via":"cli"`
(59a72b8). The third passed at 08:24:54: 1,428 of 1,428 tests, and the lifecycle bench in 6.8 s. The wait for the
shared gate lock cost about 25 minutes across the three (theseus-rx91; Item 51).

**Before the install.** Without an `[approval]` section, the owner's web UI's answers would be refused, though his
cards would still reach his DM. So three lines go into his config first (sent to him at 07:49), and the install waits for them. Then
health will say `binary: JOBS CAN WRITE …` of his `theseusd`, which holds until the builder runs under
`theseusd install --separate` (theseus-2r70).

### Item 45. The v1.1 roadmap (theseus-empf; the `v11` lane, docs only; 2026-10-01 23:35 to 2026-10-02 00:32; reviewed 07:48 to 07:49; 394459e, rebased as 4315d70 and again onto hardening2's join; joined 08:25 at d7358f3)

**Why.** The owner, 2026-10-01 at 23:11: "a nice ~1week roadmap for after v1 to v1.1". The critical work he named at the
same time, security, performance, and proof points, moved ahead of v1 into lanes. This plans the week after.

**What landed.** `docs/design/roadmap-v1.1.md` (459 lines), indexed in `docs/design/README.md`:
- about 16.5 spine slots in 12 steps (V1 to V12, with V13 as stretch) and 24 lane slots in 12 lanes: about 58 to 62
  agent-hours over a week;
- seven themes, in order: the kernel's last frames (S5 and the transaction's follow-ups), visibility, the
  operator's surfaces, cost accuracy, the codebase's shape (C5, C7, S6), proof and the turn's edges, and Discord;
- the order argued: first the two protocol steps the lanes read (V1, history paged both ways; V2, the typed node
  detail); the kernel's steps while C6, C2, and S2 are fresh; S5, the riskiest, after lane `sim2` has taught
  kernel-sim the transitions its merged frames must survive;
- if v1 is called around October 20 or 21, v1.1 lands around October 28.

**How it was checked.** Each of the 51 issues labelled `post-v1` appears once in its appendix: 40 in v1.1 (2 of them
stretch), 8 riding with their v1 rows, and 3 out. Each of Review 2's deferred items is placed. S6's "serialize once"
was checked in the code and found not done (each watcher gets a clone, and each connection serializes again), so it
is in v1.1. Every Part III "Known gaps" block was read for its issues, each checked open or closed. A mechanical
check found no private names, ids, or URLs, and the lane's gate passed (1,405 tests).

**What it asks of the owner.** Four questions, each with a default, so nothing blocks: S5's accounting on a crash, the
herdr plugin, the test voice channel, and a Discord `/queue`.

**Found on the way.** Part III gaps that later work closed but never struck: Batch C part 1's two `otel` gaps (moot
since Step O1), the narrative's unbounded queues (bounded since Item 33), and A0's dollars, hooks, and GLM. v0.76
strikes them, with pointers.

### Item 46. The `perf1` lane: reads by state, the start's and the stop's disk work, and the memory target (theseus-cvd0: theseus-lv2, 2qt, 0dq, 02k, 26r, hanu, byu, u6xg, and ndw; 2026-10-01 23:34 to 2026-10-02 01:31, and a second run, 07:35 to 08:15, after the usage limit; reviewed 08:25 to 08:34; rebased onto d7358f3 as a24c1d8, c21f531, and 4ea47e1; joined 08:34 at 4ea47e1; installed 14:23 with 9f4035b)

**Why.** The owner, 2026-10-01 at 23:11: the critical work now, end to end, with the memory target as a proof point. At
10,000 parked sessions, every start read every execution, and every health answer decoded every execution, action,
and session. The driver's 500 ms tick read every open execution, and a parked conversation is an execution waiting
on input, so that was all of them. The history check after serving re-read the whole WAL at every start, a stop paid
two durable commits, and a burst of writes stalled the push's board for as long as it lasted. §9 promised 10,000
parked and 50 active sessions in under 1 GB, and nothing had measured it.

**What landed** (§9).
- **Reads by state** (lv2, 2qt). The store's index keeps a projection of each key's latest record: an execution's
  terms (its state; `due`, when a due time or a task's report may wake it; `w`, when it holds wakes; its limit, when
  that follows the config's; its parent), an action's state, and each session's numbers (turns, five token counts,
  and cost). The start reads the executions a crash interrupted, the driver's tick the queued and the due ones, the
  reconcile the sent actions, and a stop, a cancel, or a task's end its own execution's unsettled actions. Health
  counts from the projection and reads no record. The terms are built after serving: a store an older build wrote
  last is read as before until its projection is whole.
- **The history check from its mark** (0dq): the last check's last frame is checked again, then only what follows.
  The whole history is checked once a day, and a mark that doesn't check is not believed.
- **A stop's one commit** (02k): the stop's checkpoints sync nothing of their own, and redb's close makes them
  durable: 5 syncs a stop, where it paid 6 or 7. Each phase of a stop is logged at debug (26r). The rare slow stop,
  caught at 402.7 ms, was the machine's writeback in one of the stop's two waits on the disk (138 ms in the row's
  fdatasync, 258 ms in redb's close), and neither wait can go.
- **The board on a thread of its own** (hanu): the push applies frames on the blocking pool, so it keeps up while
  every runtime worker waits on the disk.
- **Restore** (byu) prints its phases (copy, open, counts, record, swap) and builds the index in bulk from a replay
  of 4,096 records or more. **The web UI's owner lookup** (u6xg) is one `sock_diag` request, 11.6 µs, where reading
  `/proc/net/tcp` took 2,050 µs. **The bench's `inflight` phase** (ndw) times a clean stop with a reply's post held
  in flight.

**How it is proven.**
- **A counting store**: 42 executions (40 parked, one queued, one mid-turn with a job), then a crash. The start reads
  1 execution and 1 action, the driver's tick 2, health none, and a stop 1. **kernel-sim**: at every step, through
  its crashes, reopens, and races, every read by state equals a full read. Revert proofs for lv2, 0dq, hanu, and the
  bulk build; strace counted the stop's syncs; a burst of 5,000 `session.open` left the board current, where
  9ac009d's held 400 views beside 4,975 sessions.
- **The 10,000-session A/B**, release, alone under the gate lock, 9ac009d against the lane: a cold start's p50 119.1
  to 21.5 ms; a kill's restart 149.9 to 48.0 ms; a swap 149.2 to 45.6 ms; health 63.6 to 1.67 ms; an idle daemon
  5.05 % of a core to 0.10 %.
- **The memory target passes, about tenfold.** A release daemon on 10,000 parked sessions idles at 16 MB (9ac009d:
  62 MB). With 50 turns in flight at once on a stand-in model, it peaks at 104 MB (9ac009d: 231 MB, most of it held
  in glibc's per-thread arenas). Most of the lane's peak is the harness's own `session.list` of every session.
- **Gates**: gate 7 on the rebase onto C2, 1,425 of 1,425, and the bench alone after it, every budget met. **The
  join's gate** at 08:34:26: 1,443 of 1,443, the bench in 10.6 s.

**Divergences.** The issue said "O(open)"; the projection is by state, since a parked session is open. The terms
were built at open in the first commit, which put about 100 ms on an upgraded store's first start, so they moved
after serving. 26r was not in the stop's code: it is the disk. The push's seed is about 5 ms slower at 10,000
sessions, since the start no longer reads every execution before it. Restore's index is built inside the restore,
since the restored store's first start must find it whole, and a draft that hard-linked the sealed segments was
withdrawn (theseus-j1ly).

**Known gaps.** The list methods still read every record: at 10,000 sessions a `session.list` takes 149 ms and
answers 5.4 MB, the one memory peak left (theseus-96w2, P2). The bench's own Discord binding never binds, so
`driver_before_token` checks nothing (theseus-l21m, P2). Restore's budget and one for a stop with a post in flight
were proposed for the owner's call, and are measured, not gated, until he answers (theseus-fsug; §9). glibc's arenas
(theseus-4qou), restore's segment links (theseus-j1ly), and a web test's wall clock under load (theseus-ioq7), all P3.
The request handlers' waits on the disk were S2's (Item 52).

### Item 47. The `proofs1` lane: Discord proven end to end without a person (theseus-9kjv, with theseus-6g62, theseus-ck0k, theseus-qifw, and theseus-kl8m's steps; 2026-10-01 23:34 to 2026-10-02 08:00, with the usage limit's pause from 01:37 to 07:35; reviewed from 08:26; 88099bb, a25adb0, and d98ed87, merged as d051779, 2d68c88, and 699927d; joined 08:38 at 699927d; installed 14:23 with 9f4035b)

**Why.** theseus-kl8m asked the owner to prove the Discord binding by hand in the test channel: type a message, press a
card's buttons, and read what came back. A bot can neither type nor press, and the binding's logic needs no person.

**What landed** (§3.1).
- **A stand-in for all of Discord the binding talks to**, in `theseus-sim`: the gateway (`fake_gateway.rs`: HELLO,
  READY, RESUMED, heartbeat ACKs, dispatches, and nothing kept of an IDENTIFY), and the REST fake grown to answer
  interaction callbacks, original-response edits, and follow-ups, to keep every version of every message and every
  answer to an interaction, and to hold a guild that answers the viewer check (the members intent, the channel's
  overwrites, the guild's owner, roles, and members).
- **A user's part, sent as Discord sends it**: `say` types a message as a user, and `press` presses a button the
  binding posted (ck0k's viewer check answered, qifw's posts read back).
- **The proof**, `theseus-sim discord proof --theseusd <bin>`: kl8m's steps against a real `theseusd` on a scratch
  state dir, its binding on the stand-ins, a scripted model, and a fake `op` that answers every secret with an
  invented value. Twelve steps: the daemon serves; the gateway identifies; a channel and a DM bind; the viewer check
  trusts the channel; a typed message gets its reply; an unlisted user is ignored; a write outside the roots waits
  with its card naming the listed user; the unlisted user's Approve is refused and the call keeps waiting; the
  listed user's press runs it; the card settles (approved, buttons gone); the resumed turn's reply posts; and the
  daemon stops cleanly, with no token in anything the stand-in kept. theseusd's `tests/discord_proof.rs` runs it in
  the gate. For a live check across processes: `fake-discord --gateway --guild`, and `discord rig|model|say|press|read`.

**How it is proven.**
- The fake's payloads decode as twilight-model's own types, which caught READY's user without `mfa_enabled` and a
  follow-up's zero channel id before any binding test ran.
- Three binding tests through the gateway, and four planted reverts in the binding (a press, and a message, from
  someone the place doesn't list; the viewer check without overwrites; a settle that keeps its buttons), each caught
  by those tests and by the gate's proof. A fifth, in the fake, is caught by the fake's own test.
- **Timings**: debug, 12 of 12 in 2.9 s; the lane's release build, 12 of 12 in 2.3 s, and 2.6 s at the last commit;
  across processes, driven only through the CLI, 10 of 10, twice.
- The lane's gate 5: 1,418 of 1,418. Merged into C2, and into the chain's `main` with hardening2, it built and passed
  its 105 tests on each. **The join's gate** at 08:37:32 passed.

**Divergences.** The stand-in, not the owner, types and presses, in the gate. It is written in Rust on tungstenite
rather than taken from C3's Node prototype: one binary, no Node in a check. The REST fake's `answer` was split
(`read_request`, `bot_route`, `interaction_route`) to fit bench2's 100-line budget (Item 48).

**Known gaps.** The stand-in drives no slash command, select menu, attachment, `GUILD_CREATE`, or dropped gateway
yet (theseus-ymi3); read-back on real Discord (theseus-w04x); three hand-written Messages-API stand-ins to unify
(theseus-luxx), all P3. Four gate tests that flake beside nice-0 neighbours (theseus-zfaq, 1m3s, 535n, 1n5f), and an
internal ENOENT naming no path from two tests (theseus-46ya, P2; its likely cause found in Item 73). Real Discord's half of kl8m, the bot's permissions
and intents, stays with the owner: kl8m is closed as superseded, and he keeps the option of one human run at v1's call.

