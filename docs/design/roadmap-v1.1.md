# Theseus v1.1: the week after v1 (roadmap v1.1)

_Beads theseus-empf. Written by Tabitha/Claude on 2026-10-01, from 23:35 MST, to Eddie's request at 23:11 for "a
nice ~1week roadmap for after v1 to v1.1". Sources, read at `main` 9ac009d: every open Beads issue labelled
`post-v1` (51 of them when this was written), [Review 2](review-2.md)'s deferred items with Eddie's answers to it
(2026-10-01, 19:54), the open questions of the six designs this directory holds, [the status page](../status.md)'s
known limits, and the "Known gaps" of [the spec](../the-ship-of-theseus.md)'s Part III. Docs only: nothing was built,
filed, or changed in Beads. Every size and date here is an estimate._

**What this plan leaves out on purpose.** The critical work Eddie named at 23:11 (security, performance, and proof
points) is done before v1: by lanes `hardening2`, `perf1`, `proofs1`, `bench2`, `security3`, `robust2`, and
`robust3`, and by the spine's C2 (one typed event per fact) and S2 (the store-writer thread). Nothing those carry is
planned here. The gaps they file as `post-v1` while they work join the matching theme (§4's reserve).

## Contents

0. The answer on one screen
1. The themes
2. The spine, in order
3. The lanes
4. The week, day by day
5. What is not in v1.1, and why
6. What v1.1 needs from Eddie
7. Appendix: every input, and where it went

## 0. The answer on one screen

- **The size.** About **16.5 spine slots and 24 lane slots**, plus 2 slots of stretch. A slot is roadmap-v2's: one
  step, about an hour of agent time plus its review; a join counts half. At roadmap-v2's rates (1.3 to 1.55 hours a
  spine slot, 1.5 a niced lane slot) that is **about 58 to 62 agent-hours**: five days at 8.5 to 10 agent-hours,
  then two lighter days that hold the reserve and the stretch.
- **The themes:**
  1. **The kernel's last frames** (6.5 spine): S5's plain turn in fewer frames, and what the kernel transaction
     left: a late result's wake in the turn's end frame, an `execution.queued` row for every queue, a nested lock
     caught by a test, and a waiting call's reason kept on its action under one schema rule.
  2. **Visibility** (3 spine, 2.5 lane): the five telemetry gaps; disk, web, and approval health that reaches the
     operator in words; every operator act ledgered by who did it.
  3. **The operator's surfaces** (2 spine, 6.25 lane): history and the ledger read incrementally, in both
     directions; nothing polled that the daemon can push; the TUI scrolls to a session's start; a Stop button in
     the web UI; herdr remembers a pin.
  4. **Cost accuracy** (4 lane): the estimate's last known errors (a recompile's density, dense tool output, Haiku's
     figures, Sonnet 5.5's cache minimum) and GLM's word for a prompt past its window.
  5. **The codebase's shape** (3.5 spine, 7 lane): C5's port seam for the Discord binding, C7's shared rig and the
     split of `tests_m3.rs`, S6's "serialize once", the typed node detail, the reader rule's cheap gaps, and the
     build's hygiene.
  6. **Proof and the turn's edges** (1.5 spine, 3 lane): kernel-sim drives `/stop`, scheduled wakes, report wakes,
     and the outbox under crashes; an image-heavy turn recovers within `max_loops`; `fs.patch` takes a miscounted
     hunk.
  7. **Discord** (1.5 lane): a bindings file read while the daemon runs, and lane maps that stop growing.
- **The week's shape.** The spine takes 2.5 to 3.5 slots a day for five days (3 to 5 agent-hours), then lightens.
  Two to four lanes take the rest, never more than roadmap-v2's three heavy and one light at once.
  - **First, on day 1, the two protocol steps the lanes read:** history and the ledger paged in both directions
    (V1), and the typed node detail (V2).
  - **The kernel's steps on days 2 to 4,** while C6, C2, and S2 are fresh. S5, the riskiest change, comes on day 4,
    after lane `sim2` has taught kernel-sim the transitions S5's merged frames must survive.
  - **Telemetry and cost from day 1:** the soak will have raised the questions their numbers answer.
  - **Discord and the codebase's shape last:** C5 waits on the voice wire-in, and the `tests_m3.rs` split needs a
    quiet tree.
- **When.** If v1 is called around October 20 or 21 (the status page's estimate), v1.1 lands around October 28.
  Lanes could start during the soak, and merge once v1 is called, so the build being soaked doesn't move.
- **What v1.1 proves:**
  - a plain turn at the frame floor the machine allows, and the gate holding it there;
  - OTel telling the same story as the ledger;
  - no web or terminal surface polling what the daemon can push;
  - the estimate measured on every model in the catalog;
  - kernel-sim covering every transition a turn can take.
- **Not in v1.1 (§5).**
  - Eight issues ride with their v1 rows: d64, 3nk, 646, w6b, 3fjv, b9l4, 7zm, and 0pd.
  - 2r70 is Eddie's own, and part of it is due before v1. rnx waits for a PTY that no roadmap builds.
  - Recommend closing obmo now, and 4htx and cox0 once their checks have run.
  - The designs' "filed" items wait for their triggers: the soak, M4 to M7 landing, or Eddie.
- **From Eddie (§6), nothing blocks the week.** S5's accounting call (may a crash between the answer and the turn's
  end leave the call unknown, its reservation held?), the herdr plugin in his own setup, and the test voice channel,
  which decides where C5 lands.

## 1. The themes

Each theme names its inputs by id (Beads ids drop the `theseus-` prefix), its size in slots, and what it proves when
done. §2 and §3 group the same items into spine steps and lanes; §7 traces every input.

### 1. The kernel's last frames (6.5 spine slots)

C6, the kernel transaction (`Kernel::frame`, f5cb680), is on `main`, and S2's writer thread follows it before v1.
What remains is Review 2's S5, and five issues on the kernel and the store.

| Ids | What | Slots |
|---|---|---|
| 6qwr | A turn whose background result lands during it ends parked, and a second frame queues the late result, so for one frame every surface reads the session ready (jj9f's second shape). The wake rides in the end frame | in V3 (1.5) |
| 2xep | A job's completion queues its execution with no `execution.queued` row, only the action row's `execution_state`. Add `{ why: "result" }` in the completion's frame | in V3 |
| oqxw | A transaction's closure that calls the kernel itself, on an execution the transaction didn't name, takes a lock out of order: a deadlock no test catches. Panic on any lock taken while the thread holds another | V4 (1) |
| q49, ef0 | Records carry two schema numbers (the header's per kind, and some payloads' own): fold them, or write the rule down. Then keep a waiting call's reason and floor on its action, so `confirm.list` reads no transcript (ACTION's schema bump), and merge a confirm continuation's queue and run frames (ef0's note) | V5 (1.5) |
| 2uby; Review 2 S5 | A plain turn in fewer than 5 frames. A kernel transaction cannot span an `.await`, so measure the middle way first (the admission with `turn.started`), then the lock held through the compile; the completion rides the end frame only with Eddie's yes (§6). Also S5's second half, not in 2uby: cache the manifest's digests per request spec | V7 (2.5) |

**Proves:** a plain turn writes the fewest frames the machine's rules allow, and bench2's frames-per-turn budget is
lowered to hold it; every transition that queues an execution says so in an `execution.*` row in its own frame; a
nested lock fails a test instead of hanging a daemon; one rule says which schema number a record change bumps.

### 2. Visibility (3 spine, 2.5 lane)

EXQUISITE VISIBILITY (§2) asks that every turn, call, and failure be timed and attributed as it happens. The
telemetry lane (Item 26) left five gaps between what the ledger knows and what OTel is told.

| Ids | What | Slots |
|---|---|---|
| b85w | A failed turn's tokens and dollars are not in `theseus.tokens` or `theseus.cost.usd` | lane `telemetry2` (2.5) |
| 8u02 | `theseus.provider.first_token_ms` records only each turn's last call | `telemetry2` |
| lmhp | `theseus.provider.call.duration_ms` mixes failed calls with answered ones: add `error.type` | `telemetry2` |
| iu3a | A tool span whose call failed has no ERROR status | `telemetry2` |
| 8pei | A confirmed call's run and a background job's end are not timed or counted as tool calls; decide whether a call counted `awaiting_confirm` is counted again, or moved | `telemetry2` |
| jxau, 4v1z, od13 | Health's `web` section is JSON only; the CLI's catalog and health lines show no 1-hour cache-write price or count; the web apps show neither `approval.open` nor `binary` (od13, filed by hardening2) | V6 (1) |
| cny7 | Operator acts other than approvals ledger `by` as the connection's label (`web#35`), not the surface. **Done 2026-10-05** (Part III Item 172) | V8 (1) |
| f337 | Free disk space crossing the warning or the floor reaches no one who doesn't read health | V10 (1) |

C2's recorder projects each fact to OTel once. Lane `telemetry2` re-reads each issue against it first, since some may
close by construction.

**Proves:** OTel tells the same story as the ledger: a failed turn's spend, each call's first token, failed calls
apart from answered ones, and every tool call's run, confirmed or in the background, counted once. Disk, web, and
approval states reach the operator in words, on every surface, when they change. Every row names who acted.

### 3. The operator's surfaces (2 spine, 6.25 lane)

| Ids | What | Slots |
|---|---|---|
| xo0m, kym3, glyw | `ledger.tail` and `session.history` take an `after` position, and `session.history` a `before` one; `node.reach` takes a node's short id, resolved in the daemon, and `theseus history` prints it | V1 (1.5) |
| 7ovb | What still polls in the web apps after the push: the Observatory's tables, and the cockpit's other reads | lane `web2` (3.5, with the next three) |
| nkt | A Stop control in the web UI that calls `execution.stop`, beside a running turn | `web2` |
| kdkv | The Observatory's Context panel shows the estimate's method, its counted and estimated parts, and the ring's bound | `web2` |
| Part III step 2b, part 1 | The web UI shows Approve and Decline where the web may not answer. hardening2's fail-closed `[approval]` default makes that the default case: show where to answer instead | `web2` |
| kym3, v6yc | The TUI pane reads a session's newest 200 nodes only; another surface's message lands after the reply it started | lane `tui2` (2) |
| 1n2l, d5oc | `theseus watch` leaves its last line unended at EOF; `herdr sync --pin` is not remembered | lane `cli` (0.75) |
| pkaq | A herdr plugin: a startup hook that runs `theseus herdr sync`, and a key that answers the focused pane's question | `cli`, stretch (1) |
| — | The web lane's join (`App.tsx`, `protocol.ts`) | V11 (0.5) |

**Proves:** no surface polls what the daemon can push: the page's frames, idle for 30 seconds, show no request but
the ones a push or the operator caused (7ovb's own check). Every part of a session's history is readable from the
TUI, the web, and the CLI, and `theseus reach` works from what `theseus history` prints.

### 4. Cost accuracy (4 lane)

The tokens lane (Item 31) made the estimate honest, and named what it couldn't measure yet.

| Ids | What | Slots |
|---|---|---|
| c5ba, o388 | Haiku 4.5's bytes-per-token figures come from one request; measure them by class (well under a cent). Confirm Sonnet 5.5's prompt-cache minimum (512) against the caching docs, which Tabitha fetches for the lane | lane `tokens2` (4, all five) |
| vj9q | A recompile's estimate is bytes only, so its ring can ring at about 71% of the budget by the provider's count. Carry the session's density from its last counted request | `tokens2` |
| p171 | Tool output denser than 1.76 bytes a token (hex, base64, digests): measure it on a scratch daemon, then a class of its own if the numbers want one | `tokens2` |
| kucs | Z.ai's error for a GLM prompt past its window is unknown, so such a session may be refused every turn. Learn its shape from Z.ai's documented codes, or one probe (about $0.16 if a refusal is billed) | `tokens2` |

kdkv, which shows the estimate to the operator, is in `web2` (theme 3).

**Proves:** the estimate is measured by class on every model in the catalog; a recompile rings at the session's
measured density; a GLM session past its window recovers by itself, as an Anthropic one does since Item 34.

### 5. The codebase's shape (3.5 spine, 7 lane)

| Ids | What | Slots |
|---|---|---|
| Review 2 C5 | The Discord binding reaches into `Core` directly (37 calls at the review). Name the seam as a `BindingPort` trait that `Core` implements, and correct the crate's doc. Eddie's answer: deferred "until the voice binding needs it" | lane `discord2` (2 of its 3.5) |
| Review 2 C7 | A shared rig builder in `theseusd/tests/common` (the config, a fake `op`, the fake model, the fake Discord, and the socket, each opted into). At 9ac009d, 14 of the daemon's 19 test files carry their own `#!/bin/sh` scripts, 18 in all (Review 2 counted 12, in nine files) | lane `tests2` (2) |
| Review 2 C7 | Split `tests_m3.rs` by subject: 7,374 lines at 9ac009d, up from 5,422 at the review | V12 (1.5) |
| Review 2 S6 | Serialize once. The push added the backlog cap and `events.lost`, but `SessionBus::publish` still clones each message per watcher (`bus.rs:88-117`), and each connection serializes it again (`rpc/server.rs:368-380`). **Done 2026-10-05** for serialization (theseus-celu.36; Part III Item 172); latest-wins and the slow client's disconnect remain | lane `push2` (1) |
| a6be | `NodeInfo.detail` is still a JSON map per node kind, read by key in three renderers. Type it per kind, generate its TypeScript, and match today's output byte for byte | V2 (2) |
| g7qp | Four spellings pass the reader rule's registry test though nothing reads the item. Close the two cheap ones: an install list in the repo, checked against `tool` markers; and a reader counted only where `theseus_core::graph` is in scope. **Done 2026-10-05** for holes 2 and 4, with theseus-t7ra (Part III Item 182): the two install scripts' lists read as text, and a read counted only where the type it names is ours; holes 1 and 3 stay open | lane `reader` (1) |
| zay1, 4htx, cox0 | The bench's noise margins re-derived from `main`'s gate history, or `--runs 20`; dependency variants counted after the workspace-only rule (close 4htx if few, by its own rule); `cargo deny` run without the advisory ignores, and each one whose fix has shipped dropped | lane `hygiene` (1) |

**Proves:** the binding has a seam for the voice binding to follow; the daemon's tests share one rig; `tests_m3.rs` is
split by subject; one serialization serves every watcher; the renderers read typed fields; two of the reader rule's
four holes are closed.

### 6. Proof and the turn's edges (1.5 spine, 3 lane)

| Ids | What | Slots |
|---|---|---|
| Part III Items 6, 8, and 9 (and A3) | kernel-sim's random operations never drive `/stop`, a scheduled wake, a report wake, or the outbox, and inject no crash around the outbox's transitions. Still true at 9ac009d: its step picks an open, a turn, a limit change, a cancel, a budget or confirm answer, and the heartbeat (`kernel_sim.rs:498-596`) | lane `sim2` (2) |
| 6hk, 8gf | An image's retry takes a loop index, so a turn can make one call past `max_loops`; a request with several refused images recovers one per turn. The vision limits are checked against the live docs, which Tabitha fetches for the step | V9 (1.5) |
| inw | `fs.patch` rejects a hunk whose header counts are off, and the pilot's builder gave up on the tool. Recount from the hunk's body, as `git apply --recount` does, and say so | lane `tools2` (1) |

**Proves:** kernel-sim drives every transition a turn can take, the newer ones included, under crashes and with
`--p-race` over many seeds, before S5 changes the frames; an image-heavy turn recovers in one turn, within
`max_loops`; a patch with miscounted headers applies, and one whose context doesn't match still fails.

### 7. Discord (1.5 lane)

| Ids | What | Slots |
|---|---|---|
| ocwt | The binding reads its bindings file only at start, so a place removed while the daemon runs stays bound. Watch the file (inotify, or the heartbeat's mtime check), diff the places, stop the removed ones' lanes, and refuse their unsettled posts. **Done 2026-10-06** as a 2 s stat of the file's mtime, size and inode, a removed place's lane retired so its post in flight settles as sent (theseus-ocwt; Part III Item 207) | lane `discord2` (1) |
| Part III Item 6 | A lane's maps grow for the process's life. Still true at 9ac009d: `sent` and `sealed` (`courier.rs:250-254`) are never pruned. Bound them. **Done 2026-10-06:** a lane keeps its newest 256 keys of `msgs`, `sent` and `sealed`, the task board's key exempt (theseus-celu.37; Part III Item 207); a cross-turn edit past 192 later keys posts a held turn's tool line again (theseus-6809) | `discord2` (0.5) |

**Proves:** a place removed from the bindings file stops being served without a restart, and a long-running
binding's memory stays flat.

## 2. The spine, in order

**16.5 slots in 12 steps**, with one more as stretch. A step is spine where one file is everyone's: the kernel, the
store, the protocol, `turn.rs`, the RPC methods, `tests_m3.rs`, the CLI's `main.rs`, or the web apps' `App.tsx`
and `protocol.ts`. The steps are numbered V1 to V13 so as not to collide with roadmap-v2's rows.

| # | Step | Ids | Slots | Waits on |
|---|---|---|---|---|
| V1 | History and the ledger page both ways: `after` on `ledger.tail` and `session.history`, `before` on `session.history`; short node ids for `node.reach`, and in `theseus history`. **Done 2026-10-05** (theseus-xo0m, glyw, and kym3's protocol half; Part III Item 193): `ledger.tail` already paged both ways, so `session.history` took its shape; the TUI's older pages wait for kym3's TUI half, with `tui2` | xo0m, kym3's protocol half, glyw | 1.5 | v1 |
| V2 | `NodeInfo.detail` typed per node kind, with today's output captured first | a6be | 2 | V1 (both change the protocol crate). Before `web2` and `tui2`, whose readers it changes |
| V3 | Every queue writes its row, in its frame: a late result's wake in the turn's end frame; `execution.queued { why: "result" }` in a completion's frame. **Done 2026-10-06** (theseus-6qwr and 2xep; Part III Item 194): the row is written in `accept_completion`, so every caller writes it, and the end and the late result's wake are one frame (`end_and_wake`, which reads the stop under the frame's lock) | 6qwr, 2xep | 1.5 | C2 (`turn.rs`) |
| V4 | A lock taken while the thread holds another panics, after checking that nothing nests on purpose (the outbox's keys, the observer, startup's scans) | oqxw | 1 | robust2's R7 (`!Send` locks) |
| V5 | One schema rule (fold or document); then a waiting call's reason and floor on its action, and a confirm continuation in one frame | q49, ef0 | 1.5 | robust2's R8 (the golden-schema test). ACTION's schema takes its number when V5 lands; snapshot Eddie's store before the install |
| V6 | Health and the catalog in every surface's words: the `web:` line, the 1-hour cache writes, `approval.open` and `binary`. **Done 2026-10-05** (theseus-jxau, 4v1z, and od13's `binary` half; Part III Item 177); `approval.open` went with theseus-zmgb | jxau, 4v1z, od13 | 1 | v1 (hardening2's health facts). Smaller if hardening2's join already showed od13's lines |
| V7 | S5: a plain turn in fewer frames, measured step by step; the manifest's digests cached per request spec | 2uby, Review 2 S5 | 2.5 | V3; `sim2` merged; S2's writer thread (vni9); bench2's frames-per-turn gate, which V7 lowers; `tokens2` merged (both touch `compiler.rs`) |
| V8 | Every operator act's `by` from `Conn::actor`, with a test per act. **Done 2026-10-05**, in push2's session (theseus-cny7; Part III Item 172): `profile.use` and `session.recompile` were the acts left; `turn.submit`'s author still falls back to the label, a step of its own | cny7 | 1 | C2; `push2` merged (both near `rpc/server.rs`) |
| V9 | The image retry keeps its loop index; retries go on while each 400 names an image not yet hidden, bounded by the images in the request | 6hk, 8gf | 1.5 | C2 (`turn.rs`); the vision limits fetched beforehand |
| V10 | One row and one notice per disk crossing (`disk.low`, `disk.below_floor`, `disk.ok`), from the heartbeat. **Done 2026-10-05** (theseus-f337; Part III Item 177) | f337 | 1 | C2 |
| V11 | The web lane's join | `web2` | 0.5 | `web2` |
| V12 | `tests_m3.rs` split by subject, on a quiet tree: the test count before and after is the same, and no lane holds an edit to the file | Review 2 C7 | 1.5 | every v1.1 step and lane that adds to it, merged |
| V13 (stretch) | Name a directory's `AGENTS.md` the first time a session's tools touch it, once per guide per session | ug9i | 1 | C2 (`toolrun.rs`) |

- V3, V4, V5, and V7 are the kernel's and the store's steps. V5's schema bump is the week's only one.
- V7 is the one step that changes crash semantics. Its validation is 2uby's: the frame-budget test lowered to the
  new count, the crash test, kernel-sim's `--p-race` over many seeds, and the lifecycle bench.

## 3. The lanes

**12 lanes, 24.25 slots, plus 1 of stretch.** A lane is one worktree on one crate or directory, by the lane recipe.
Within `theseus-core`, a lane owns named files, as the telemetry, tokens, and cache lanes did in v1. Every lane
merges when it is reviewed. Weight: **H** is a Rust build, **L** a web or light lane.

| Lane | Steps, in order | Depends on | Joins | Slots | W |
|---|---|---|---|---|---|
| **telemetry2** (core's `telemetry/`) | re-read each issue against C2's recorder → a failed turn's spend (b85w) → each call's first token, and `error.type` (8u02, lmhp) → a failed tool span's ERROR status (iu3a) → a resumed call's run and a late result timed and counted once (8pei). **Done 2026-10-05** (b85w, 8u02 and iu3a on 2026-10-03, Part III Item 67; lmhp, Item 178; 8pei, Item 180) | C2. 8pei rebases over V3 if it needs `turn.rs` | merges | 2.5 | H |
| **tokens2** (core's `compiler.rs`, `provider.rs`'s `Census`, `catalog.rs`) | Haiku's figures and Sonnet 5.5's cache minimum (c5ba, o388) → a recompile's density (vj9q) → dense tool output, measured first (p171) → Z.ai's overflow (kucs) | the caching docs and Z.ai's error codes, fetched by Tabitha | merges, before V7 | 4 | H |
| **sim2** (`crates/theseus-sim`) | kernel-sim drives `/stop`, scheduled wakes, and report wakes → crashes around the outbox's transitions | v1 | merges, before V7 | 2 | H |
| **web2** (`web/`, `cockpit/`) | a Stop control (nkt) → the estimate's panel (kdkv) → Approve only where the web may answer (step 2b's gap) → the rest of the polls follow the push, with V1's `after` (7ovb) | V1, V2 | V11 | 3.5 | L |
| **tui2** (`crates/theseus-tui`) | older pages at the pane's top, with V1's `before` (kym3) → another surface's message in its place (v6yc) (v6yc **done 2026-10-06** in smalls, Part III Item 195; kym3's older pages remain) | V1, V2 | merges | 2 | H |
| **push2** (core's `bus.rs`, `rpc/server.rs`) | one serialization per notification, shared by every watcher (S6). **Done 2026-10-05** (theseus-celu.36; Part III Item 172) | v1 | merges, before V8 | 1 | H |
| **tools2** (`crates/theseus-tools`) | `fs.patch` recounts a hunk's header from its body (inw). **Done 2026-10-05**, in the proc-steps session (theseus-inw; Part III Item 175) | — | merges | 1 | H |
| **discord2** (`crates/theseus-discord`, and `Core`'s side of the port) | the bindings file read live (ocwt) → the lane maps bounded (Item 6) → C5's `BindingPort`, last. **The first two done 2026-10-06** (theseus-ocwt, theseus-celu.37; Part III Item 207); C5's `BindingPort` is left | C5: right before 44b, wherever 44b lands | merges | 3.5 | H |
| **reader** (core's `tests_registry`) | g7qp's two cheap closures. **Done 2026-10-05** (theseus-g7qp holes 2 and 4, with theseus-t7ra; Part III Item 182) | — | merges | 1 | H |
| **cli** (`crates/theseus`, never `main.rs`) | `watch` ends its line (1n2l: two goldens change by one newline) → a remembered pin (d5oc: a pane token, the smallest of its three options) → stretch: the herdr plugin (pkaq) | pkaq: Eddie's herdr (§6) | merges | 0.75, + 1 stretch | H |
| **tests2** (`crates/theseusd/tests`) | C7's shared rig, and the daemon's test files moved onto it | `discord2` merged, or rebased over its tests | merges | 2 | H |
| **hygiene** (`theseus-sim`'s bench, the dependency tree, `deny.toml`) | zay1, then 4htx's count, then cox0's check | bench2 merged (it owns the bench and `gate.sh`); a `gate.sh` line rides V11 | V11, if `gate.sh` changes | 1 | L |

- **If 44b (the voice wire-in, roadmap-v2's row 77) lands in v1,** C5 goes with it there, and `discord2` shrinks to
  1.5 slots.
- **sim2 is a proof point.** If Tabitha wants it before v1, it fits beside the spine now: it touches only
  `theseus-sim`.

## 4. The week, day by day

Agent-hours use roadmap-v2's rates: 1.3 to 1.55 a spine slot with its review, and 1.5 a niced lane slot.

| Day | Spine (slots) | Lanes (slots) | Agent-hours |
|---|---|---|---|
| 1 | V1, V2 (3.5) | telemetry2 1.5, tokens2 1.5 (3) | 9.1 to 9.9 |
| 2 | V3, V4 (2.5) | telemetry2 1, tokens2 1.5, sim2 1 (3.5) | 8.5 to 9.1 |
| 3 | V5, V6 (2.5) | tokens2 1, web2 1.5, sim2 1 (3.5) | 8.5 to 9.1 |
| 4 | V7 (2.5) | web2 1, tui2 1, push2 1, tools2 1 (4) | 9.3 to 9.9 |
| 5 | V8, V9 (2.5) | web2 1, tui2 1, discord2 2 (4) | 9.3 to 9.9 |
| 6 | V10, V11 (1.5) | discord2 1.5, reader 1, cli 0.75 (3.25) | 6.8 to 7.2 |
| 7 | V12 (1.5); stretch V13 | tests2 2, hygiene 1 (3); stretch pkaq | 6.5 to 6.8, and up to 3 of stretch |
| **Total** | **16.5** | **24.25** | **about 58 to 62** |

- **At most three heavy lanes and one light lane at once,** beside the spine, as roadmap-v2 measured the machine
  (memory binds first, then Tabitha's review).
- **Review:** about 6 hours of spine reviews (30 minutes a step) and 6 of lane reviews (15 minutes a step, about 25
  steps): under 2 hours a day.
- **The reserve.** Days 6 and 7 leave about 5 agent-hours. What the soak files, and what the v1 lanes file as
  `post-v1` while they work, comes first; the stretch (V13, pkaq) yields to it. Roadmap-v2 expected a fix batch
  every 10 to 15 steps, and v1.1 has about 37 steps.

## 5. What is not in v1.1, and why

### Rides with its v1 row

Each is tied to a step roadmap-v2 already places in v1. Recommend naming each in its row's brief, so the row
decides it, and dropping the `post-v1` label from d64 and 3nk, which roadmap-v2 already schedules. A row that lands
without its issue sends the issue to v1.1's reserve.

| Id | Its v1 row | Why there |
|---|---|---|
| d64 | row 24 (20a) | Built on M4's labels (roadmap-v2 §2, and conflict 9); the issue's note folds Review 2's consideration 4 into it |
| 3nk | rows 53 (30b's `BudgetReport`), 62 (35a), and 63 (35b) | M6's own steps, from Appendix F |
| 646 | row 51 (29b's wire-in), or 29c | The issue: "fold into row 51's wire-in or 29c" |
| w6b | row 72 (41b) | The issue: "decide at 41b" |
| 3fjv | row 58 (32a's wire-in) | FSRS's parameters become configurable there |
| b9l4 | before row 58 | Which same-day rule FSRS keeps matters once `+retention` runs. It needs the FSRS wiki and a second scheduler's code, which Tabitha fetches |
| 7zm | row 49 (28b) | The issue: measure "when categorize.v1 is wired" |
| 0pd | row 37 (23a), or later | The issue: "at 23a or later", and low priority while Jev's tables are small |

### Eddie's, and part of it is due before v1

- **2r70**, Review 2's operational changes. None is code, and each touches Eddie's own setup: his daemon under the
  user unit, the `[approval]` lines in his config, and the builder under `--separate` before it works unattended.
  The `[approval]` paste is due before his daemon restarts onto a build with hardening2's fail-closed default, or his
  web UI's approvals are refused (the issue's note, 23:25). That is v1's business, not v1.1's.

### Waits for work no roadmap builds

- **rnx:** when the PTY path (`shell.open`, §7) is built, it must withhold granted values as the job wrapper does.
  Neither roadmap builds it. Keep the issue open as the reminder it says it is.

### Recommend closing

- **obmo** (the reader rule and config keys). The loader's `deny_unknown_fields` and the template test already refuse
  undeclared keys. A declared field nothing reads is left to review, and each way to catch it (a sentinel test per
  struct, a macro-built mirror, rust-analyzer in CI) costs more than the gap. Close it, and reopen it if a dead key
  ever ships.
- **4htx** (cargo-hakari), by its own rule: lane `hygiene` counts the dependency variants after the workspace-only
  rule has run for weeks. Close it if the count is small.
- **cox0** (the seven advisory ignores), once bench2's daily fetch-and-check (Review 2's SC2) runs: give that job one
  more `cargo deny` without the ignores, and it says each day which can go. Until then, lane `hygiene` checks once.

### Merged into one step

xo0m, kym3's protocol half, and glyw (V1); 6qwr and 2xep (V3); q49 and ef0 (V5); jxau, 4v1z, and od13 (V6); 2uby and
S5's digest cache (V7); 6hk and 8gf (V9); the five telemetry gaps (`telemetry2`); c5ba and o388 (`tokens2`'s first
step).

### The designs' filed and later items

Each waits on a trigger its design names. Apart from the AWS questions, which live with the AWS epic (mgw), none
has a Beads issue: a title search of all 285 issues, open and closed, finds none. Recommend filing one only when its
trigger fires; the design is its record until then.

| Item | Source | Its trigger |
|---|---|---|
| A Discord `/queue` of what needs Eddie, with buttons | stage2 §5 question 15, and §6 | The soak shows he answers from Discord, not the TUI or the web |
| MCP servers started on first use | M7 §5 question 7, and §6 | A server's RSS against §9's 1 GB |
| Per-client MCP tokens | M7 §6 | A second MCP client |
| Role grants and per-author executions | M7 §6 | A place with several people in it |
| Slash commands in threads, and a thread under a listed channel trusted | M7 §6; Part III step 2b, part 1 | Threads turn out to matter |
| Tighten-only bindings edits kept in the store | M7 §5 question 16 | After bindings format 2 (row 68) |
| A task's mechanical veto, and repeating wakes in tasks | M7 §5 questions 13 and 11 | After the task graph (rows 70 and 71) |
| L1's looser posture, scratch promotion, `rw_paths`, the job host's queue, ad hoc `op://` fetches, CPU limits | M4 §5 questions 1, 8, 12, 13, and 14, and §6 | M4 in daily use |
| Pack overrides from the state dir, the confirm band asking a person, JUDGE_STOP live in conversations, Jev choosing the persona | M5 §5 questions 10, 13, 8, and 5 | M5's prove, on the soak's data |
| The AWS design's open questions (where the hard limits live, the NAT, the monthly amount) | AWS §6 | Eddie, as roadmap-v2 §7 has them |

### Known gaps left as they are

The Part III gaps that name no issue, apart from the three this plan takes (kernel-sim's coverage, the lane maps,
and the web's Approve buttons):

- **By design, or M4's to close:** the argv layer's limits (steps 2a to 2a.3: M4's boundary closes them); a job's
  `/proc/<pid>/environ` and gh's stored login at L0 (B1: L1); a job's orphans (Z1: L1's cgroup); the notify socket's
  stance (A2); a fetch's URL as a way out (Item 10: a notice per fetch, by design); `/stop` leaving a session's tasks
  running (Item 7, and W1's rule).
- **This machine's quirk:** health sees the filesystem Linux reports, and WSL's C: can fill first (Item 25). The
  chain's own disk guard covers this machine, and Theseus is a Linux product.
- **Small, and nobody has hit them:** a page cut from its head with no offset to read on, UTF-8-only pages, and
  private names such as `printer.lan` (Item 5); blobs never collected (Item 4); `at` needing an RFC 3339 offset
  (Item 8); a vanished profile's output cap (Item 12); the Observatory's plain-text transcript (A3; the cockpit
  renders markdown); a quiet notice's 23 bytes (Item 3). Any the soak trips becomes an issue.
- **Closed by later work, though Part III doesn't mark them (for the next spec fold):** batch C part 1's gaps about
  the otel feature (O1 put the exporter in every build); narration's unbounded subscriber queues (every connection's
  queue has a backlog cap since Item 33); A0's dollars (Item 1), its hooks (deleted in A3b), and its unseen GLM reply
  (GLM replies are seen live, Item 26's check among them).

### The status page's known limits

- **"Linux only. One daily user so far."** No issue asks for another platform, and the soak is the daily use.
- **"A stop that lands earlier, while a job still waits for its secrets ... A fix is in progress."** That fix is
  36to, now closed. The line is stale, for Tabitha's next docs commit. A different early stop, hmwv, is in lane
  `security3`.
- **The memory target** (10,000 parked and 50 active sessions in under 1 GB) is lane `perf1`'s proof point, before
  v1.

## 6. What v1.1 needs from Eddie

Nothing blocks the week: each item has a default.

| # | What | Blocks | Default without an answer |
|---|---|---|---|
| 1 | **S5's accounting call** (2uby): may a crash between the provider's answer and the turn's end frame leave the call `unknown`, holding its reservation (the worst case), where today its cost is recorded? | V7's last merge only | No. The completion keeps its own frame, and the plain turn stops at 3 or 4 frames |
| 2 | **The herdr plugin** in his herdr: a startup hook and a key | pkaq, which is stretch | Built and documented; he installs it when he likes |
| 3 | **The test voice channel** for 44b's live check (roadmap-v2 §7) | Where C5 lands | C5 is `discord2`'s last step |
| 4 | **After the soak: a Discord `/queue`?** (stage2 question 15) | Nothing | Not built |

## 7. Appendix: every input, and where it went

### The 51 issues labelled `post-v1`

40 are in v1.1 (2 of them stretch), 8 ride with a v1 row, and 3 are out. Two of the 40, 4htx and cox0, are checks
that end by closing their issue.

| Id | What | Where |
|---|---|---|
| 3nk | M6's additions from Appendix F | §5: rows 53, 62, and 63 |
| nkt | A web Stop control | `web2` |
| d64 | A session opened from a job's process holds none of its job session's external text | §5: row 24 |
| q49 | Two schema numbers per record | V5 |
| ef0 | `confirm.list`'s reason on the waiting action | V5 |
| 0pd | Jev's builders' dropped items, recorded | §5: row 37 or later |
| 7zm | Jev's output allowance on a full topic table | §5: row 49 |
| inw | `fs.patch` and miscounted hunks | `tools2` |
| 646 | A crc-valid but wrong frame stalls the index tender | §5: row 51 |
| w6b | The MCP server ignores a client's cancel | §5: row 72 |
| 8gf | Several refused images recover one a turn | V9 |
| 6hk | The image retry takes a loop index | V9 |
| rnx | The PTY path must withhold granted values | §5: waits for `shell.open` |
| cox0 | `deny.toml`'s seven advisory ignores | `hygiene`, then close |
| 4htx | One dependency build per variant | `hygiene`, then close if few |
| xo0m | `after` positions for `ledger.tail` and `session.history` | V1 |
| jxau | Health's `web` section in the CLI and the Observatory | V6 |
| ocwt | The bindings file read only at start | `discord2` |
| zay1 | The bench's noise margins | `hygiene` |
| cny7 | Operator acts ledgered by connection label | V8 |
| b9l4 | FSRS-6's same-day floor for Hard | §5: before row 58 |
| 3fjv | FSRS-6's clipped parameters, said | §5: row 58 |
| f337 | Disk crossings, told | V10 |
| b85w | A failed turn's spend in OTel | `telemetry2` |
| 8pei | A confirmed call's run, and a background job's end | `telemetry2` |
| 8u02 | First token of every call | `telemetry2` |
| iu3a | A failed tool span's status | `telemetry2` |
| lmhp | `error.type` on failed provider calls | `telemetry2` |
| o388 | Sonnet 5.5's cache minimum | `tokens2` |
| 4v1z | The 1-hour cache writes in the CLI | V6 |
| c5ba | Haiku 4.5's figures by class | `tokens2` |
| kdkv | The estimate in the Observatory | `web2` |
| vj9q | A recompile's density | `tokens2` |
| p171 | Dense tool output as a class | `tokens2` |
| a6be | `NodeInfo.detail` typed | V2 |
| obmo | The reader rule and config keys | §5: recommend closing |
| g7qp | The reader rule's four open spellings; two closed 2026-10-05 (Part III Item 182) | `reader` (two of them) |
| kucs | Z.ai's word for a prompt past the window | `tokens2` |
| 7ovb | What still polls in the web apps | `web2` |
| 2xep | `execution.queued` for a completion | V3 |
| 1n2l | `theseus watch`'s last line | `cli` |
| 2r70 | Review 2's operational changes, with Eddie | §5: Eddie's, before v1 |
| glyw | Short node ids for `theseus reach` | V1 |
| ug9i | A directory's guide, named on first touch | V13 (stretch) |
| kym3 | The TUI's older history | V1 and `tui2` |
| v6yc | Another surface's message in the TUI | `tui2` |
| 2uby | S5 after C6 | V7 |
| 6qwr | A late result's wake in the end frame | V3 |
| d5oc | A remembered herdr pin | `cli` |
| pkaq | A herdr plugin | `cli` (stretch) |
| oqxw | A lock taken while holding another | V4 |

### Review 2's deferred items

| Item | Eddie's answer (19:54) | Where |
|---|---|---|
| C5, the binding's port seam | Deferred until the voice binding needs it | `discord2`, last; with 44b if 44b lands in v1 |
| C7, the shared rig and the split of `tests_m3.rs` | Deferred; C7's lighter half (gate phase timings, a flaky-test list) went to bench2 | `tests2` and V12 |
| S5, the 2-frame turn | Filed as 2uby after C6 | V7, with S5's digest cache |
| S6, "serialize once" | "Covered by the push step", with serialize once for its review | Not covered, by the code at 9ac009d: `push2` |

### Taken from elsewhere

| Input | Source | Where |
|---|---|---|
| od13: `approval.open` and `binary` in the web apps | Filed by hardening2, P3, without the `post-v1` label (recommend adding it) | V6 |
| kernel-sim's missing operations | Part III Items 6, 8, and 9, and A3's gaps | `sim2` |
| The Discord lane maps that grow | Part III Item 6's gap | `discord2` |
| Approve and Decline where the web may not answer | Part III step 2b, part 1's gap | `web2` |

The last three have no issue yet. Recommend filing each when its lane is briefed, owned by `main`, with this file as
its source.

*— written by Tabitha/Claude*

<!-- REPORT COMPLETE -->
