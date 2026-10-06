# The Ship of Theseus, chapter 19: Part III, A4's Items 118 to 128 ([index](README.md))
### Item 118. 43a: `extend.propose`: a proposed MCP server frozen, tried in L1, and put to the operator; MCP servers in L1 (theseus-ext.5, roadmap row 75; the cloud batch 5's extend-propose session, fired 2026-10-04 01:50 from e457555d, Opus 5.5; ec16ef6d, 84fdf451, e39cb9dc and 720784be; reviewed 04:34 to 05:26 by the local reviewer R1, with real L1 and a live check on Sonnet 5.5; joined 06:11 at d3cad454, a signed merge onto 35784d7c with R1's join fixes, by the batch-5 harvest wake 1972d751; installed 14:09 at bddfd407, install #1)

**Why.** §3.21: Theseus may add planks to its ship on the open ocean, never the keel. The agent writes a tool as an
MCP server, runs and tests it in an L1 sandbox, and the operator's ack, never Jev's, decides whether it loads. M7's
design (`docs/design/m7-surface.md` §2.7) split it: 43a proposes, freezes, tests and asks; 43b loads, restarts and
revokes. 36b (Item 106) had refused `sandbox = "l1"` for an MCP server, and "no L1, no step 43": an
agent-written server never runs at L0. Overnight the prompt writer's call was that the cloud proves L1 as uid 65534,
since its VMs run as root, and that a local run proves it if that fails (the morning notes' decision 6).

**What the session found.** The sandbox's `spawn(spec, init, Stdio)` already accepted a stdin, so `theseus-sandbox`
needed no change; what was missing was a long-lived process to hold an L1 server's init and its pipes. Two `rustup`
installs at once raced at setup; run one after the other, they worked.

**What landed** (four commits; the branch 43 files, +4,300 −37 with its two paperwork files; the merge against
35784d7c, 42 files, +3,808 −38; no new package).
- **MCP servers in L1** (ec16ef6d). A new role of the daemon's own binary, `theseusd mcp-sandbox`
  (`theseus-kernel`'s `mcp_l1.rs`), dispatched at the top of `main` beside `job-sandbox`. The board spawns it as any
  stdio server (`mcp/l1.rs`: `children::spawn(Kind::Owned)`, its own process group, the three pipes), the spec in
  `THESEUS_MCP_L1` and the server's environment the spec's alone. The role hands its stdin, stdout and stderr to the
  init and keeps no end of them, so the server talks straight to the board, its stderr goes to
  `<state>/mcp/<name>.log`, and a daemon that closes the pipes ends it. The view is a job's (`Sandbox::job_view`: the
  workspace read-only under overlays, `ro_paths`, the floor and the socket hidden, the job limits), with its own
  `egress` list through `job_egress`'s proxy; an empty list is no listener and no network. The role waits on the
  init's pidfd, a signalfd (SIGTERM to the init, a 2 s grace, then SIGKILL, which ends the namespace), and the
  daemon's pidfd (a `kill -9` of the daemon kills the init at once). `sandbox = "l1"` loads for a stdio server (an
  HTTP server is refused), `egress = [...]` is valid on an L1 server only, and `l0` stays the default. There is no
  fallback: a root daemon's start fails with L1's refusal, and the server never becomes ready.
- **The tool** (`extend/`): `extend.propose { name, dir, command, description, tests?, network? }`, class `Run`,
  `NonRepeatable`, a harness tool, `notify` under the template, waiting under T1's hold like any `Run` call; `dir` must
  resolve inside the workspace roots, checked at the plan and again at the run. The turn awaits the trial as it awaits
  an async tool (`ToolRuntime::run_extend`), as a task a cancel or `/stop` aborts, never on a core.
- **The freeze** (`extend/freeze.rs`): `dir` is copied into `<state>/extensions/<name>/<digest>/`, the digest a SHA-256
  over the sorted tree (each entry's path, kind, mode, length and bytes, the mode kept as 755 or 644 as git keeps it,
  so the read-only copy digests as its source and an executable bit still counts). A symbolic link is refused; a tree
  is capped at 512 entries and 16 MiB; the copy is made beside its final place, sealed read-only, checked against the
  digest, then renamed in.
- **The trial** (`mcp/trial.rs`): the board starts the frozen copy as `ext-<name>` through its own connect, so in L1
  through the role, with the proposal's network list, no secret, no environment beyond the job's, in a new state,
  `proposed`. A server on trial is never one of the board's configured servers, so no turn is offered its tools. The
  trial runs `initialize`, `tools/list` and each test as a `tools/call` (`contains` or `equals`; an error result fails
  it); the server stops when the trial ends, and a guard takes it off the trial list if its start fails or is aborted
  (e39cb9dc).
- **The manifest**, a META record `extend.manifest.<name>.<digest>`: the name, digest, description, command, source and
  frozen path; the tools with their schemas; each test with whether it passed, what came back (cut to 500 characters)
  and why it failed; the capabilities (network, scratch); who proposed it (session, execution, call, principal,
  place); and its state (`proposed`, `failed`, `acked`, `declined`) and who answered. The call's result shows it.
- **The ack** is a planned `extend.ack` action on the proposing execution, its card and the manifest written in one
  frame, so every approval surface answers it unchanged (Discord's buttons, `theseus confirm`, the cockpit's card,
  `confirm.list`, the push), with the reason "Load wordcount 3f2a1c: 5 tools, 2 of 3 tests passed, no network?". It is
  answered by `action.confirm` after `judge_act`'s place rule (`Act::Answer`): an ack writes `extend.acked`, a decline
  or an unanswered question past `confirm_ttl` `extend.declined`. Nothing wakes the execution and nothing loads; a
  server that does not come up records `failed` and asks nothing; a `/stop` or a cancel of the proposing session
  declines it. `theseus confirm` says "nothing resumes: an extension's ack loads nothing in this build".
- **Rows and surfaces:** the facts `extend.proposed`, `extend.tested`, `extend.acked`, `extend.declined`, each with its
  narrative line; the protocol module `extend.rs` and `extend.list`; `theseus extend list`; health's `extensions`
  (proposals, waiting, acked, declined, failed). A restart mid-proposal runs the call again, as harness calls are, and
  answers from its manifest if one exists.
- **The store's format did not move** (11 at the join): the manifest is a META record, and the question an ordinary
  planned action.

**How it is proven.**
- **Tests:** `extend/tests.rs`, 7 through the whole core with the in-process fake MCP server standing in for the frozen
  copy (it picks its mode from a file in the directory it is given, so what runs is what that directory holds): a
  proposal is frozen, tried in L1 and put to the operator (the digest the tree's, the frozen copy alone started as
  `ext-wordcount` with no egress or environment, 5 tools and `[pass, pass, fail]` with the failure's reason, the
  question's exact reason); an edit after proposing changes nothing that runs; no tool is offered before the ack nor
  after it; an ack from a shared place is refused (the owner's DM counts); a decline loads nothing, and neither does an
  expiry; what cannot be tried is said and nothing is asked; a stop during the trial ends it and asks nothing. The
  freeze's 3 unit tests. theseusd's `tests/mcp_l1.rs` (the real daemon, role and init with `theseus-sim fake-mcp`: the
  server ready with its 5 tools, the process tree `mcp-sandbox`, `job-sandbox`, `fake-mcp`, a network namespace of its
  own holding only `lo` and no route, all three gone after `shutdown` and after the daemon's `kill -9`) and
  `tests/extend.rs` (a stand-in Messages API asking for the proposal; `confirm.list` ending ": 5 tools, 2 of 2 tests
  passed, no network?"; `theseus confirm` refused inside a job and acking outside it; no tools offered after). In the
  cloud the L1 tests ran by hand as uid 65534, 5 runs each under load; the core's tests 22 of 22 five times. At the
  review, **on a daemon that is not root, so real L1**: **240 of 240** on 94e01826 with the branch, and **263 of 263**
  on 8e17eea7 with its join fixes.
- **Planted reverts.** The session's: the trial started from the workspace's directory, a proposed server's tools
  offered, and the stop's bug before e39cb9dc's fix. R1's four: the workspace's directory instead of the frozen copy
  (caught: the edited `error` mode ran, `[false, false, false]`); the digest dropping the executable bit (caught); a
  trial aborted in its start left on the trial list (caught: `ext-wordcount` still `proposed`); **the L1 role ignoring
  the daemon's pidfd: not caught**, since the fake server ends by itself when its stdin closes, so the `kill -9` test
  passes (1.6 s instead of 0.2 s). The code is right and the test blind (theseus-grxh, P2).
- **Live, at the review** (a scratch daemon of the 8e17eea7 merge's debug build as a transient unit with
  `Delegate=yes`, a fresh state dir, web and Discord off, the template's `notify`, Sonnet 5.5, about $0.04; not the
  operator's daemon or store). "Write a word-count MCP server … and propose it": the model wrote `server.py` (1,669
  bytes; `fs.write` notified) and called `extend.propose` (notified, "propose extension wordcount from … (3 tests)").
  The call took **76 ms**: frozen, started **in L1**, `count_words` listed, 3 of 3 tests passed in 58 ms; then
  "extend.ack needs your confirmation: Load wordcount 2500b6: 1 tool, 3 of 3 tests passed, no network?". `theseus
  extend list` showed it `proposed`, with its command, frozen path and tool; the frozen copy was mode r-x with
  `server.py` r--r--r--; after the trial the unit's cgroup held the daemon alone. `theseus confirm <id>`: "approved …
  nothing resumes: an extension's ack loads nothing in this build", `extend.acked` by the CLI. A new turn's tools held
  `extend_propose` and the built-ins, and no `mcp__ext-` tool.
- **FAST:** `main` compares its first argument with the role's name before anything else (the start clock begins after
  it); `Core::build` reads `/proc/self/exe` once more and stores two paths; nothing spawns or reads the store at start.
  Health's `extensions` is a META prefix scan per call. Only an `extend.propose` call does anything new on the turn.

**The join** (the batch-5 harvest wake 1972d751, 06:11, on R1's review). The signed merge d3cad454 (35784d7c and the
report commit ee5b77d9). rerere replayed ten files from R1's tree (every hunk keep-both: the template's `[policy.tools]`
count, 42 + 1 = **43**; the registry keeping main's LSP loop under the branch's comment; theseus-protocol's `ts.rs`
dropping the branch's copy of a line main extends), and `fact/mod.rs` kept both beside hands-cancel
(`resolve-35784d7c.py`). **R1's `joinfix.py`:** `Core::dispatch` folded through `or_empty` at 101 lines (byte for byte
mcp-prompts' own fix, so a no-op on this main); `prompt: None` in the branch's test rig, since mcp-prompts gave
`TurnRequest` a `prompt` field (E0063); and the long-file guard, which raised theseus-protocol's `lib.rs` ceiling to
2,636 with its reason (theseus-pf8a splits it). Its gate (06:10:59): 2,152 of 2,152; lifecycle OK at main's level
(cold start p50 20.4, shutdown 32.5, swap 48.8 ms); jobs OK; frames 5 and 9, plain p50 77.1 ms, tool-call 171.3 (main
77.6 and 173.4 at 8e17eea7, so hands-cancel's slower turn numbers had been the load, Item 116); pushed
06:11. theseus-ext.5 closed. It released extensions-load (43b, Item 133), launched at 06:12, which had
to add acked extensions to the catalog itself.

**Eddie's call** (the 9am review's decision 2, 2026-10-04 09:34, "Take your recommendation"): option (a). The trial
keeps running before the ack, with the hosts the model asked for, at `extend.propose`'s own posture, and the notice
names the hosts; under default trust that is no more than a `proc.run` at the same posture. Two small fixes, filed as
theseus-ext.9: the buttons read Load and Don't load, and L1's view gets `python3`.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the install's check ran L1's self-test (start 28.3 ms). The operator's daemon
runs as the operator, not root, so its L1 trials run.

**Divergences.**
- The manifest is a META record, not a node of `kind: extension` with `trust: agent` and a `derived_from` edge (either
  would be a new record kind or field, and a format bump); `proposed_by.correlation_id` stands for the edge.
- The ack is a planned kernel action answered by `action.confirm`, not a new card type, and is judged as
  `Act::Answer`, not a new `Act::Extend`: the rule is the same, the owner from a private place.
- Discord's buttons still read Approve and Decline; the card's text says "Load …?" (theseus-ext.9).
- The digest's modes are git's, and a link in the tree is refused outright.
- The design's web view of a proposal (the frozen files, a diff against the version it would replace, the tests, Load
  and Decline) is a list of what the cockpit should show, not built; reading the frozen files needs a read method.

**Known gaps.** The trial runs before the ack with the model's own host list (kept by Eddie, above). On this machine
L1's view had `/usr/bin/python3.12` but no `python3` (linuxbrew's, outside the view), so a proposal of `["python3",
"server.py"]` failed its trial here, as the report's own live check would have (theseus-ext.9). A second proposal of
the same name and digest overwrites the manifest's question while the older one still waits; two proposals of one
name share a trial-list key (health's display only); a failed proposal leaves its frozen copy; a trial aborted
mid-handshake leaves no `extend.tested` row. theseus-grxh (P2): a test server that ignores its stdin's end, killed with
its daemon. Loading, restart, revocation, `/extensions` and an acked extension's posture were 43b's.

### Item 119. The judgment surfaces: a judgment's id marked in the trace before the turn's last frame, the judge's sentences and metrics, `judge.list` and `judge.get`, `theseus judge show`, and the cockpit's Judgment section (theseus-0j2.4; roadmap row 38, step 23b; the fifth cloud batch's judgment-surfaces session, launched 01:41 and stalled at setup, relaunched 02:22 and fired 02:26 from 3add3f53, Opus 5.5; cef79d36, a1db8c9e, 585e27b0 and dbcc8488; reviewed 04:37 to 05:14 by local reviewer R2, first of a stack of four; joined 06:52 at 8b804784, a signed merge onto d3cad454, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** 23a (Item 105) put Jev in the core: `loop.v1` judges each turn's stop in shadow, spawned from
`TurnRunner::run` after the turn's last frame. Its judgments were rows in the ledger and nothing else. Step 23b of
M5's design (`docs/design/m5-judgment.md` §3) makes a judgment visible everywhere a turn is: in the trace, the
narrative, the metrics, the protocol, the CLI and the cockpit. It is the first of the four judge points the fifth cloud
batch built and a local reviewer took as one stack (23b, 24, 25a, 25b), because all four add the same convention: the
core mints a judgment's id at its dispatch (`theseus_judge::Ask.id`).

**What landed** (the merge: 47 files, +2,270 −124; no new package; `Cargo.lock` and `package-lock.json` unchanged).
- **The marks** (`judge/mark.rs`, new). The trace is finished in `finish`, just before the turn's last frame, so the
  mark is made there. `JudgeService::plan_loop_end` decides the dispatch purely, from the config's mode and the sample
  (a hash of the turn id); `Dispatch::new` mints the id; `mark_turn_end` writes a zero-length span named `judge`, kind
  `mark`, with `pack`, `point`, `mode` and `judgment` (and `loop` and `class` for `loop.v1`). In `turn.rs` this is one
  call in `finish`, before `trace.finish`. `after_turn` reads the marked id back and spawns the judgment with it; a
  trace without a mark re-plans as 23a did, with the same result, since planning is pure. `theseus_judge::Ask` gains
  `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it when set; `new_id()` is now
  public. Cost to the turn: no frame, no read, no await.
- **Sentences** (`fact/judge.rs`). The judge's facts say their lines once the sink's frame is written, as the
  session's and the turn's, with no notification: a judgment landing ("Jev, in shadow, judged the stop: progressing
  (0.95, act band). The baseline ended the turn; recorded, not acted on.", with " Jev disagrees." when it does, a drift
  note when another model answered, "skipped (shed)" or "the call failed (timeout)" otherwise); the pause ("Shadow
  judging paused: today's $1.00 is spent. It resumes at local midnight."); a new `judge.resumed` row and line at the
  first reservation of a new local day after a paused one; a booked block; the breaker opening, re-opening and
  closing; shedding. `spend::Reserve` also returns what its records say (`spend::Said`), so the pause, resume and
  booked-block lines are spoken only after the reservation's frame is written.
- **The headline answer.** A judgment is described by its pack's deciding Choice or Score (`loop.v1`'s
  `work_state`), else a deciding Noul, else any whole answer; the `judge.call` row carries it as `headline`. Pack
  questions load from a sorted map, so a plain "first question" would have been `announced_unfinished`.
- **A disagreement** (`fact::judge::disagrees`, stored as the row's `disagrees`): the judgment was answered by the
  model the pack pins, and the pack's deciding questions reach the **act** verdict (`theseus_judge::decide`, Item 95).
  For `loop.v1`: `announced_unfinished` leaning true in the act band, so the baseline ended a turn whose last message
  promised more. An ask verdict does not count.
- **Metrics** (`Telemetry::record_judgment`, from the sink after its frame; `Telemetry` is now `Clone`, so the judge
  holds the core's pipeline, set at build and again when telemetry is built after serving):
  `theseus.judge.calls {pack, mode, band, class}` (band is the headline's band, or `skipped` or `failed`),
  `theseus.judge.duration_ms {pack, class}` (only calls that reached Jev), `theseus.judge.on_path_ms {pack, class}`,
  `theseus.judge.errors {theseus.error.class}`, `theseus.judge.disagreements {pack}`, and `theseus.cost.usd` with
  `theseus.spend = judge`.
- **The protocol** (`theseus-protocol/src/judge.rs`, served by `rpc/judge.rs`). `judge.list {pack?, session_id?,
  since?, limit}` answers `{scopes, matched, judgments}`: rows without states, oldest first, the newest `limit` kept
  (50 by default, 500 at most); with no pack it reads every embedded pack's scope (seven then); `loop.v1` names one
  version, `loop` every version. `judge.get {id}` answers `{judgment, state, state_missing?}`: the row by key (new
  `Store::ledger_by_key`) and the state from the blob the row names, checked against its digest. Health's judge block
  gains `shed` and `key` (`ready`, `resolving`, `failed: …` or `not configured`, never a value). The TypeScript is
  regenerated.
- **The CLI** (a1db8c9e). `theseus judge log` reads `judge.list`, gains `--pack`, and says when the list is cut;
  `theseus judge show <id>` prints the header, the session and turn, class and baseline, the outcome, model, timing,
  cost and agreement, the state's record and its fields (each cut at 160 characters, saying so), and each answer with
  its band and probability bars, its lean marked; `ask --trace` prints a mark as `loop.v1 shadow at loop_end · reply ·
  jdg_… (theseus judge show jdg_…)` with the id whole; health's `judge:` line adds `N shed`, and the key's state when
  it is not ready.
- **The cockpit** (585e27b0). The Judgment section (`/judgment`, key `j`): per pack its mode, version, calls, cost, and
  the p50 and p95 of Jev's time per workload class over the judgments read, with today's counts from health; a log
  filtered by pack, session and 1 h, 24 h or 7 d; one judgment's fields, its answers as bars with bands, and its state
  as fields. It reads `judge.list` every 4 s only while open and `judge.get` once per judgment, keeps its state in the
  address, and keeps no ledger loop of its own. The time machine folds `judge.call`, `judge.paused`, `judge.resumed`
  and `judge.circuit` into `World.judge`. A session's timeline shows each judgment beside the loop it judged ("loop 1 ·
  Jev (shadow): progressing 0.93 · disagrees", or "judging…" until the row lands), and the flame chart draws a `judge`
  mark as a larger diamond. `src/lib/judgment.ts` is pure, with 4 tests.
- **The store's format stays 11**: only rows were added (`judge.resumed`; `disagrees` and `headline` in the
  `judge.call` row's data).

**How it is proven.**
- **The session's tests:** `tests_judge_surfaces` (the shadow dispatch marks the trace with the id its row carries: one
  mark, zero length, the same mark in the `turn.trace` row written in the last frame, the `judge.call` row's id and key
  the mark's, no mark with the judge off; a judged turn keeps its frames, at most 5, and its provider requests byte for
  byte; a landing says its exact sentence with `disagrees: true`; `judge.list` across all packs, `loop.v1`, `loop`,
  `loop.v2`, `security.v1`, each session, `since`, and `limit` 1; `judge.get` against the stored row and the blob's
  state, and `NOT_FOUND` for an unknown id), the mark's unit test, and
  `telemetry::tests_judge::the_judges_metrics_carry_their_names_and_attributes` (two judged turns through a whole core,
  the second with the fake Jev down, read back from the OTLP test receiver: every attribute, duration and on-path
  counts of 2 with on-path summing to 0, `errors{network}` 1, `disagreements` 1, the judge's `cost.usd` equal to the
  row's `cost_micros`); two CLI renders, among them a full golden of `judge show`.
- **The session's planted reverts:** `at_loop_end` spawning with a fresh `new_id()` (the mark test failed, two
  different `jdg_` ids); `judge.list` ignoring its pack filter (failed at `loop.v2`).
- **The VM's suite:** 1,970 run, 1,937 passed, 33 failed, all L1 sandbox cases of a root VM (theseus-pv6i), 17
  skipped; the output golden ran under `TZ=America/New_York` (the UTC machine prints `+#:#`, theseus-ig6n); the
  kernel-sim flake passed on its retry. The 15 judge tests under load, five rounds: 75 of 75. The lifecycle bench with
  the judge on: OK in 16.7 s on the VM. The cockpit's lint, tests and build passed.
- **At the review** (R2, on main 8e17eea7, every cargo run through the review-step lock): 216 of 216 on the merged
  tree (the judge's tests, the telemetry test, the CLI's renders, theseusd's judge tests, the reader rule, the output
  golden, the frame budget); the cockpit 36 of 36; `clippy --workspace --all-targets -D warnings` clean. Planted
  reverts, 3 of 4 caught: the fresh id at `after_turn`; `judge.list` keeping the oldest `limit`; the on-path metric
  recording Jev's whole time (sum 4.0, not 0.0). The fourth, a reservation's facts never said, passed all 20 judge
  tests: the code says them as the report says, and no test checks the pause, booked-block, resume, breaker or shed
  lines (theseus-02vo, P2).
- **Live, at the review**, on a scratch daemon of the merged tree as a transient unit on a fresh state dir, with the
  real Jev and Sonnet 5.5 (about $0.02 of Sonnet and $0.00017 of Jev): three turns, the traced one's last line the
  mark (`@1.46 s judge [mark] loop.v1 shadow at loop_end · reply · jdg_…`); `judge log` three `loop.v1 (shadow)`
  lines; `judge show` the state's 9 fields and six answers (`answered · model jev-1.13.0 · 104 ms (queued 0, http
  104) · $0.000056 · agrees with the baseline`); `narrative.watch` three "complete (1.00, act band)" lines; the OTLP
  receiver `theseus.judge.calls {band act, class reply, mode shadow, pack loop.v1}` 3, `duration_ms` count 3 (sum
  511), `on_path_ms` count 3 **sum 0**, the judge's cost $0.000169, equal to health's; health's `--json` judge block
  `shed: 0`, `key: "ready"`; the cockpit in headless Chrome (the pack row `loop.v1 · shadow · v1 · 3 · $0.000169 ·
  reply 109 ms / 298 ms (3)`, the log "3 of 3 in 7 scopes", one judgment whole, and the timeline's line above the
  flame chart's diamond). A continued session's turn wrote 5 frames, a new session's first turn 6 (its session's own
  frame).

**What the session found.** The mark has to be made in `finish`, since 23a spawns after the frame that carries the
finished trace. A pack's first question is not its verdict (the sorted map), hence the headline rule. The core's
output golden fails on a UTC machine through a wake line's offset sign (filed at Item 96 as theseus-ig6n).

**The join** (06:52, by the batch-5 harvest wake, on main d3cad454, three joins past R2's base). rerere replayed all
nine of R2's keep-both resolutions (the cockpit's guide, Shell.tsx and main.tsx with the Ontology view beside the
Judgment section; rpc/mod.rs; rpc/server.rs with `memory.label` beside `judge.list` and `judge.get`;
telemetry/metrics.rs with the durability and LSP instruments beside the judge's five, 22 in all; theseus-protocol's
lib.rs and ts.rs; the CLI's render.rs). R2's resolve.py made the cockpit guide's count thirteen views (both sides had
made eleven twelve) and raised theseus-protocol's lib.rs ceiling from 2,636 to 2,643 with its reason. turn.rs stood at
3,495 of its 3,500. `CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped; the cloud branch deleted. Its gate (exit 0 at
06:51:50): 2,161 of 2,161 (17 skipped, 1 slow); lifecycle ok; turn frames 5 and 9, plain p50 83.0 ms (d3cad454's
77.1, inside the last 16 joins' 66.4 to 89.8: the turn's added work is one config check with the judge off). Pushed
06:52; theseus-0j2.4 closed.

**The install** (14:09, at bddfd407, install #1). Eddie's `/etc/theseus/theseus.toml` turned `[judge]` on (every pack
in shadow, as configured), so from then on his own turns carry marks and the Judgment section reads his judgments.
Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on, lsp
on, startup 73.1 ms; the store went from format 6 to 14 on start.

**Divergences.** `on_path_ms` and `duration_ms` carry `{pack, class}` beyond the design's bare names, since §2.2 of the
design wants a p95 per class; errors count under the provider errors' own key, `theseus.error.class`. A disagreement
counts the act verdict only, not a deciding Choice's lean in the act band (the design's nudge rule uses both).
`judge.resumed` is kept in process, not in the `judge.budget` META record (that would have been a format bump). A nit
the review left: the timeline says "loop 1" (counted from 1) above a flame chart whose row says "loop 0".

**Known gaps.** theseus-02vo (P2: no test for the reservation's sentences, the breaker's or shedding's). theseus-wse2
(P3: `judge.list` reads each judge scope whole at every call, and the cockpit asks every 4 s while open). A restart
forgets that yesterday paused, so no `judge.resumed` follows it. The Judgment section's per-pack numbers are over the
newest 500 matching judgments, not all history. No live `judge` spans until the first live pack (26b). The design note
owes `on_path_ms {pack, class}` and the errors' key in its telemetry row (§2.13) and `judge.resumed` in its fact table
(§2.5).

### Item 120. `security.v1` and `security.v3` in shadow at the gate, the press's labels, and the score on notices (theseus-0j2.5; roadmap row 39, step 24; the fifth cloud batch's security-shadow session, launched 01:42 and fired 01:45 from 3add3f53, Opus 5.5; d7971004 and 84c7ea3d; reviewed 05:14 to 05:53 by local reviewer R2, second of its stack; joined 07:04 at 701f0ea6, a signed merge onto 8b804784, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** M5's design puts `security.v1` at the gate: every call whose class is not `read`, and a web fetch or search
in a session that holds external text (the exfiltration path T1 leaves open by design), judged after the gate decides
and never delaying the dispatch (`docs/design/m5-judgment.md` §2.4). Item 95 had added `security.v3` (`steered`
deciding beside `risky`) as a candidate, and Eddie's 23:24 call on 2026-10-03 kept it in shadow to gather data through
the soak. Step 24 wires both packs to the gate in shadow, lets an operator's "should have asked" press label them, and
shows the score on the notices a call already posts (§2.8b).

**What landed** (the merge: 32 files, +1,860 −52; no new package; no store format change, the format stays 11:
judgments and labels are ledger rows).
- **`Ask.id`** (d7971004, theseus-judge): `Option<String>`, `Ask::new` sets `None`, `Judgment::pending` uses it when
  set: the stack's shared convention (Item 119).
- **The gate point** (`judge/gate.rs`, 582 lines). `ToolRuntime::start` plans the call (its `tool.notified` row rides
  in the planning frame), announces the notice, records `GateDecided`, then asks or runs: the one place where every
  gated call has its correlation id and its notice sent, so the point goes there, after the notice and before the ask
  or run. `GATE_PACKS = [security.v1, security.v3]`; v2 is not wired, since its questions are v3's and would be asked
  twice. `judged(class, tool, holds)`: a class not `read`, or `http.fetch`/`web.search` while the session holds; the
  hold is read only for those two, and only when a gate pack is on. On the call's path (`ToolRuntime::judge_at_gate`):
  the mode and sample, the minted ids, the marks, a pending-set insert, and one `rt.spawn`. Everything else runs in
  the spawned `judge_gate`: the session's nodes on the blocking pool, the hold, the input (tool, class, posture and
  reason, argv, paths, URL, other args, the operator's last ask, the hold as tool, host and minutes, the last 20 calls
  with outcomes; v3 also the newest six `fs.read`/`http.fetch`/`web.search` results, which its builder clips),
  `theseus_judge::prepare` (scrub and caps), the blobs, one reservation, the call, and each judgment settling its
  share. v1 and v3 build different states, so one decision point sends two Jev calls. `ToolRuntime` reaches the
  service through a `OnceLock<Arc<JudgeService>>` set in `Core::build`.
- **The record.** Each gate `judge.call` row's `context` names the call (`call`, its correlation id, and
  `tool_use_id`), `tool`, `posture` and `posture_reason`, `floor`, `notified`, `hold` (the session held external text
  when judged), `hold_raised`, `waited_on_hold`, `baseline: posture`, `decision`, `blob`, and `on_path_ms: 0`; its
  `point` is `gate`. A gate judgment's id is deterministic: `jdg_` and the first 16 bytes of sha256(pack, 0, the
  correlation id).
- **The trace.** Each dispatch is a zero-length `judge` mark (`pack`, `point: gate`, `mode`, `judgment`, `call`) under
  the call's tool span: `Ran` carries the marks and `trace_calls` adds them.
- **Labels.** A press of "should have asked" (`theseus policy tighten TOOL --call ID`, and the cockpit's and Discord's
  presses, all through `Core::tighten`) writes a `judge.label` row (new `LedgerKind::JudgeLabel`, `fact::judge::
  JudgeLabel`) for each of the call's judgments, in the tightening's own frame (`Tightenings::insert_with`): `lbl_…`
  id and key, the judgment, the pack, `question: "risky"`, `label: true`, `source: operator`, who and through what,
  `weight: 1.0`, `note: "should have asked"`, the correlation id, scope `judge:security`. Since the ids are a hash, the
  press finds a call's judgments whether they are written (a keyed lookup) or dispatched and not yet written (an
  in-memory set the sink empties as it writes), so a press before Jev answers still labels.
- **The score on notices.** A notified call's `security.v1` score follows its notice as a notification of its own,
  `judge.scored` (`notify::JUDGE_SCORED`, `Event::JudgeScored`, a protocol type with `line()` = `risk 12% (shadow)`):
  the CLI prints `! notified: proc.run · risk 12% (shadow)` after the notice's two lines; Discord adds `· risk N%
  (shadow)` to the tool line and a `Risk` field to the notice card when `notice_embeds` is on; the cockpit shows a
  pill beside "should have asked" (`src/lib/scores.ts`, from the push or from the row once written, so a reload still
  shows it). Health's judge line lists `security.v1: shadow` and `security.v3: shadow`.

**How it is proven.**
- **The session's tests** (`tests_security.rs`, against the fake Jev, a real core and the web tools on a local
  server): a property test (8 cases, fresh seed) over posture {open, notify, approve} × hold × floor × scripted
  `risky`/`steered` in {0, 0.01, 0.5, 0.75, 0.99, 1}, showing that the gate's verdict, its decision record and whether
  it waits equal the judge-off core's, for the judged call and for a second call gated after the first's judgment
  landed; a holding session's call scored 1% still waits (`hold_raised`, `waited_on_hold`); the floor still asks
  (`op whoami` at `open`, scored 1%); a search is judged only in a holding session; a press before any row exists
  writes 1 frame with 2 labels, and after the rows 2 more; a notified call's score follows its notice once (`risk
  37%`); with Jev at 5 s, `tool.started` comes within the judge-off time plus 1.5 s and under 3 s, the turn under 4 s;
  a judged tool loop keeps its frames (at most 9) and marks its trace twice. Plus the CLI's, Discord's and the
  cockpit's renders. Targeted: 359 of 359; the cockpit 24 of 24. Under load (four busy loops at nice 0, the tests at
  nice 19), three runs: 17 of 17 each.
- **The session's planted reverts:** the judgment made on the call's path (`block_in_place` on `judge_gate`): the
  `tool.started` test failed, on 5.03 s against off 19.97 ms; `judged` ignoring the hold: the search test failed
  (`left 4, right 2`).
- **The VM's suite:** 1,975 run, 1,941 passed, 33 L1 sandbox failures of the root VM (theseus-pv6i), the output
  golden under UTC (theseus-ig6n), and the kernel-sim flake passed on its retry; the turn bench by hand, frames 5 and
  9.
- **At the review** (R2, on its review commit of judgment-surfaces): 289 tests on the merged tree, 288 passing until
  join fix 4, then 36 of 36 for the judge's and security's; the cockpit 37 of 37; T1's floor tests unchanged. Planted
  reverts, 4 of 4 caught: the judgment on the call's path; a press's labels in a frame of their own (`left 2, right
  1`); a search judged without the hold; and join fix 2 undone (`left (read, read, true), right (read, tools, true)`),
  so the fix is held.
- **Live, at the review**, on a scratch daemon of the merged tree with GLM (glm-5.3-flash), the real Jev and Brave,
  `[policy] enforcement = "notify"` (about $0.004 of GLM and $0.0009 of Jev): `echo hi` read **risk 6%** after its
  notice; GLM's `bash -c rm -rf` of scratch files **risk 55%** (v1 `destructive` yes 0.95, `risky` yes 0.55, escalate),
  its `ls -la` check 19%; in a session that had read a web search, a later `proc.run ls` **waited** ("this session read
  external text (web.search …), and a call that acts waits for approval") at **risky 0.05**, both rows `posture
  approve, hold true, hold_raised true, waited_on_hold true`; a press on the echo wrote two `judge.label` rows (v1 and
  v3, `risky`, weight 1.0, each naming the echo's judgment and correlation id); every gate row read `class tools`,
  `tool_class run`. Health after: 14 calls, $0.000929. Not run live: Discord's tool line and card and the cockpit's
  pill (their tests ran).

**What the session found.** `ToolRuntime` had no route to the judge service (it lives on `TurnRunner`), hence the
`OnceLock`. Tool spans are built after the calls from `Ran`, so the marks ride on `Ran`. A press can come before Jev
answers, and rows land up to 2 s after a judgment (the sink's window): the hashed ids make both findable.

**What the review found** (two semantic conflicts with 23b that no build catches, fixed at the join).
1. **The sink's pending set:** 23b's `write` returns early when its frame fails, and 24's `pending_remove` had merged
   clean after the whole body, so a lost frame's ids would stay pending for ever and a press would label judgments no
   row will carry. The ids now leave the set as soon as the append is done, written or lost, before the sentences and
   metrics.
2. **What `class` means:** 23b's metrics, `judge show` and the cockpit's p50 and p95 read a judgment's
   `context.class` as the turn's workload class, and 24 wrote the tool's class there (`run`, `read`). Now `class` is
   the workload class (`tools`, or `task`) and the tool's is `tool_class`. A choice of names in a durable row, put to
   Eddie: at 10:21 (decision 5) he kept `tool_class`.
3. The other three: `TurnRequest.prompt: None` in the new test's helper (main's 36c added the field); the marks test
   counts the gate's two beside 23b's loop mark (`left: 3, right: 2` before); the gate's lines became a sub-bullet of
   23b's "The judge" in theseus-core's guide.

**The join** (07:04, by the batch-5 harvest wake, on 8b804784). rerere replayed nine of R2's eleven conflicted hunks;
R2's resolve.py resolved fact/mod.rs and toolrun.rs on the moved main (the merge removes only the lines the branch
removed: 24's facts after 23b's, then main's; the `judge` field beside main's `mcp`, `terms` and `lsp`) and applied
join fixes 1 to 5; theseus-judge's judge.rs keeps 23b's `Ask.id` doc and `new_id()`; turn.rs chains the gate's marks
after main's AWS and LSP children. theseus-protocol's lib.rs ceiling rose from 2,643 to 2,645 (`judge.scored`).
`CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. Its gate (exit 0 at 07:04:04): 2,175 of 2,175; lifecycle missed once
(the cold start from the config copy, p95 309.9 ms, one outlier, beside Terminal-Bench's containers: IO pressure 8 %,
fdatasync 13.7 ms against 6.5), and the gate's own rerun passed every line under its strict budget; turn frames 5 and
9. Pushed 07:04; theseus-0j2.5 closed.

**The install** (14:09, at bddfd407, install #1): with `[judge]` on in Eddie's config, `security.v1` and `security.v3`
judge his gated calls in shadow from then on. Health after the restart: the config from `/etc/theseus/theseus.toml`,
9 secrets ready, discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms; the store from format 6 to 14
on start.

**Divergences.** The score is a notification (`judge.scored`), the one divergence from §2.5 of the design, which gave
judgments none. Only notified calls hear their score; open and asked calls are judged and recorded, never told. The
CLI cannot amend a streamed notice, so the score is its own line, and one that lands after `ask` exits is lost to that
CLI (the row keeps it). Gate ids are a hash, not a uuid v7, so they do not sort by time. The label's question is
`risky` for both packs, though a press on a v3 judgment may be about `steered`. v3's recent reads are the three read
tools' results by name; a job's `cat` through `proc.run` is not one.

**Known gaps.** theseus-opu8 (P3, from the live check: a judgment reads the hold when it is judged, not when its call
was gated, so a call gated in the same loop as a web read is recorded, and sent to Jev, as holding). The
dispatched-not-written set is in memory, so a crash loses it with the rows it names. A judgment whose prepare fails
(budget paused, client not built) leaves its mark naming an id no row will carry. The state build reads the session's
nodes whole, off the call's path. Cost: two Jev calls per acting call (about $0.0002), so the $1 day covers about
5,000 acting calls. Eddie's decision 5 (10:21) made the notices live with the design's brake: the security-notices
session, Item 142.

### Item 121. `classify.v1` and `role.v1` at inbound, in shadow, one request per message (theseus-0j2.6; roadmap row 40, step 25a; the fifth cloud batch's classify-role session, launched 01:42 and fired 01:45 from 3add3f53, Opus 5.5; ff5479d9; reviewed 05:53 to 06:19 by local reviewer R2, third of its stack, one join fix redone 06:26 to 06:29; joined 07:14 at 2cba6c51, a signed merge onto 701f0ea6, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** M5's design has two packs at `inbound`, every human message that starts a turn: `classify.v1` (CLASSIFY: a
new ask, a follow-up, a correction, a control, a message addressed to a task, social; and whether the conversation
should promote to a task) and `role.v1` (ROLE_GUESS, over the roles table), batched into one request since they read
the same state (`docs/design/m5-judgment.md` §2.4). Step 25a wires both in shadow.

**What landed** (the merge: 10 files, +928 −24; no protocol change; no new package; the store's format stays 11).
- **Where the point sits.** Every turn with input reaches the core through `turn.submit`: the CLI, the web UI, and
  Discord's typed and spoken messages. A task's first turn, a wake's turn and a report's turn are continuations (no
  input), so "a turn whose input node is written" already excludes all three. `TurnRunner::inbound_point`
  (`turn/inbound_step.rs`, new) is one call in turn.rs after `node_written`: with the judge off it reads nothing (one
  field); on, it reads the session's place, builds the `Inbound`, and calls `JudgeService::at_inbound`, which filters
  by mode and sample, mints one id per pack, marks the trace (`judge` marks with `point: inbound`), and spawns. The
  session's nodes, the live tasks (non-terminal, at most 20, each brief's text after the harness preamble), the state,
  its one blob, the reservation, one `DecisionPoint` with both asks, and the settle are the spawned task's
  (`judge/inbound.rs`, new, 405 lines).
- **Slash commands are not judged.** Discord's own controls never become turns, but an unknown `/word` from Discord or
  any `/word` at the CLI does; the core tells one by its first word (`/` and a name of letters, digits, `-` and `_`),
  so `/etc/hosts is empty` is still a message.
- **The place kind.** `TurnRequest` does not carry the connection's surface (a field would touch 38 struct literals),
  so the kind comes from the session's place (`discord:dm:…` is `discord_dm`, `discord:channel:…` is
  `discord_channel`), else from the client label (`web#n` is `web`, anything else `cli`), with the place rule's class
  added: `cli (private)`, `discord_channel (shared)`.
- **The author** in the state is `operator` in a private place and `a person in a shared place` in a shared one, so no
  Discord display name is sent to Jev.
- **The roles.** The state carries the spec's twelve seed rows (§3.4) as compiled-in data (`SEED_ROLES`: an id and a
  one-sentence stance with its hints; `operator`, `thought_partner` and `security_analyst` as ids), with
  `current_role` none, until step 26c's versioned table replaces them.
- **The record.** Each judgment's `context` holds the session, execution, turn, node, baseline, `place_kind`, blob and
  `on_path_ms: 0`. Both rows of a message share one call id (`packs: 2`), and the call's cost is split by question
  count (`batch::shares`). Health counts a batched point as one call. `WIRED` gains `classify.v1` and `role.v1`; the
  template's `[judge]` comment names the packs and what is not judged; no new key.

**How it is proven.**
- **The session's tests** (`tests_inbound.rs`, 7): a person's message is judged by both packs in one request (the fake
  counts 1 connection; role's criteria are 13, the 12 seeds and `other`; with no tasks `addressed_task` is not asked;
  two rows, `judge:classify` and `judge:role`, with one state sha256, one blob, one call id; questions 5 and 1, the
  costs `batch::shares(cc+cr, [5,1]) == [cc, cr]` with cc > cr > 0; the state says `operator`, `cli (private)`,
  `current_role: none`; two zero-length marks carrying the rows' ids); the next message's state reads the previous
  message and the reply; `/status` makes no mark and no call, the next message exactly one; a task's first turn, its
  wake's turn and the parent's report turn are not judged (one classify row and one role row, both the parent's); a
  judged plain turn keeps its 5 frames (the WAL grew by 5 or fewer); a failing Jev (down, slow 10 s against a 5 s
  total, 429, malformed) leaves both rows failed with `network`, `timeout`, `rate_limited` and `malformed`, changes no
  turn (under 3 s; the provider request byte-equal as JSON to the judge-off daemon's); the packs off call nothing.
  Focused run 146 passed, theseusd's judge tests 3 of 3. Under load, three runs: 16 of 16 each (the first load run
  caught the exclusion test's 1 s wake firing before the task parked; it uses 3 s).
- **The session's planted reverts:** two decision points, one per ask (4 tests failed on their one-call counts);
  continuations judged (`left: (4, 4), right: (1, 1)`).
- **The VM's suite** failed only on the root VM's L1 cases (theseus-pv6i), the UTC golden (theseus-ig6n) and the
  kernel-sim flake (passed on its retry); theseusd's judge tests failed once on the new packs and were fixed in the
  step.
- **At the review** (R2, on its review commit of 23b and 24): 270 of 270 after join fixes 3 and 4 (267 before);
  clippy clean on the redone merge. Planted reverts, 3 of 3 caught: the inbound judgment on the turn's path
  (`block_in_place`: the failing-Jev test, on the turn's time); a slash command judged; one request per pack (`left
  2, right 1`).
- **Live, at the review**, on a scratch daemon with GLM (glm-5.3-flash) and the real Jev (about $0.003 of GLM and
  $0.0014 of Jev): "Write a shell one-liner that counts the lines in every .rs file under crates/" read `classify.v1
  kind=new_ask 1.00 act, fragment=no`, `role.v1 role=coder 0.99`; "and the tests too" `kind=follow_up 0.54 escalate,
  fragment=yes 0.90`, `role=coder 0.90`; "stop" `kind=control 1.00 act`, `role=coder 0.26 escalate`. Each pair one
  call, the cost split about 5 to 1 (57 and 11 micro-dollars; 66 and 13; 64 and 13). `/status` as a turn's input had
  the gate's and `loop.v1`'s marks and no inbound one; the plain "stop" turn wrote 5 frames; `place_kind` on every row
  `cli (private)`. Not run live: a Discord message and a spoken one (the unit test covers the kinds).

**The join** (07:14, by the batch-5 harvest wake, on 701f0ea6). rerere replayed all five of R2's keep-both hunks
(judge/mod.rs with `pub mod gate;`, `pub mod inbound;` and `WIRED`'s five packs; tests_judge.rs's rig fields and split;
turn.rs's `mod inbound_step;` beside main's `mod prompt_input;`; theseus-judge's judge.rs, 23b's `Ask.id` and
`new_id()` standing; theseusd's judge test's pack list). R2's resolve.py applied four join fixes: the branch's second
`new_id()` and its second `a_minted_id_…` test went (E0428; 23b's and 24's stand); `inbound_only` turns the gate's
packs off too, since with 24 under it the task test counted 5 Jev connections, not 1 (the first form of this fix
pushed a test to 102 lines, past clippy's 100, and the next branch's build caught it, so the review merge was redone
with the list as a const); and the frame-budget test counts 3 marks (the inbound point's two and 23b's loop mark).
turn.rs ended at 3,496 of its 3,500. Its gate (exit 0 at 07:13:50): 2,185 of 2,185; lifecycle passed on the gate's
busy allowance (cold start to the first health answer p95 84.4 ms against 57.1 strict, at CPU pressure 94 % and load
16.6 from Terminal-Bench's containers; the history records the strict verdict, a miss; the branch changes no start or
stop code); turn frames 5 and 9. Pushed 07:14; theseus-0j2.6 closed.

**The install** (14:09, at bddfd407, install #1): with `[judge]` on, Eddie's messages are classified and their role
guessed in shadow from then on. Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets
ready, discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms; the store from format 6 to 14 on start.

**Eddie's calls.** At 11:00 (decision 8, "Take your recommendations") he kept the twelve seed roles.

**Divergences.** The place kind is inferred from the place or the client label, not the connection's surface, so
Discord voice reads as `discord_dm` or `discord_channel`, like typed text (naming `voice` needs a `TurnRequest` field).
A `theseus ask` from inside a job is judged as a person's message: the request does not carry `opened_from`. The seed
roles are compiled in until 26c.

**Known gaps.** theseus-fi5n (P3: the inbound and compile judgments carry no workload class, so 23b's metrics and the
cockpit count them as `unknown`). The point's reservation can write the shadow budget's frame and the sink's frame
inside a long turn's window in a WAL-wide count (the theseus-0j2.3 shape); the turn bench runs with the judge off and
does not see it, and theseus-0j2.8's judge-on bench would. theseus-core's guide owes a line for `judge/inbound.rs`,
`turn/inbound_step.rs` and `tests_inbound.rs` under 23b's judge bullet.

### Item 122. CONTINUE's candidate signals on every compile, and `continue.v1` in shadow at compile (theseus-0j2.7; roadmap row 41, step 25b; the fifth cloud batch's continue-shadow session, launched 01:42 and fired 01:45 from 3add3f53, Opus 5.5; 5a298429 and 88d4d058; reviewed 06:19 to 06:46 by local reviewer R2, last of its stack; joined 07:25 at 4bae0f74, a signed merge onto 2cba6c51, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** §4.4a has the compiler append to a session's compilation unless a deterministic trigger asks for a recompile.
M5's design adds `continue.v1` (CONTINUE: append, or recompile and how) at `compile`, asked only when no trigger fired
and a cheap candidate signal did, so that most turns in a live thread cost nothing: a dormancy gap, the tail crossing
a soft band, a task report or wake arriving, a provider cache miss under an unchanged prefix
(`docs/design/m5-judgment.md` §2.4). Step 25b builds the signals and the point, in shadow; the recompile strategies
CONTINUE would choose between are M6's (30c, Item 131).

**What landed** (the merge: 26 files, +1,285 −27; no new package; the store's format stays 11: the stored
`Compilation` is untouched, and only rows and the protocol gain a field).
- **The signals** (`signals.rs`, new, 229 lines, beside the compiler). Each is measured against what was written
  since the model's last answer, so an input's signals fire once, at its turn's first compile: `dormancy` (the first
  `UserMessage` since the last answer came more than `dormancy_minutes` after the node before it; value, the minutes);
  `tail_band` (the tail's band rose: band 0 is under `tail_band` of the window, band 1 starts there, and each further
  quarter adds one; value, the edge passed in percent); `report` and `wake` (a harness message authored `task:<short>`
  or `wake:<short>` arrived; value, how many); `cache_miss` (the last answer read 0 cache tokens, the one before it some,
  both from the compilation the request was rendered from; value, the earlier read). `render_messages` now returns
  where the tail's messages begin (`tail_from`), and the tail is sized from it with the existing census: one pass over
  the tail, cheaper than the estimate's pass over the whole request. `CompileInput` gains `signals: Option<SignalsAt>`
  (the thresholds and the clock passed in; `None` reads nothing) and `Compiled` gains `signals: Signals` (what fired,
  the window, the prefix and tail tokens, the last cache read, the compilation's age by the clock passed in).
- **On `context.compiled`**: a new protocol type, `CompileSignal { name, value: u64, detail }`, in its own module;
  `ContextCompiled.signals` is optional and absent when empty, rides on the row, the notification and the span, and is
  generated into the cockpit's TypeScript. A signal is a number and words: Jev gets the words; the number is for the
  learning ledger and the bands' tuning.
- **`[judge.signals]`**: `dormancy_minutes = 360` and `tail_band = 0.5`, with template lines and checks (`tail_band`
  in (0, 1]; `dormancy_minutes = 0` fires at any gap, for tests and live checks). The signals are computed whether the
  judge is on or off.
- **The compile point** (`judge/compile.rs`, `JudgeService::at_compile`). Nothing is asked unless the compile appended
  (so every trigger, overflow included, asks nothing) and fired a signal, and the pack is on and sampled (keyed
  `turn#loop`). It mints the id, spawns, and returns the mark's attributes, which turn.rs writes as a `judge` mark. The
  spawned task reads the execution's budget left (through a `Weak<Kernel>`) and the last operator message, builds and
  scrubs the state, writes the blob, reserves, calls and settles, as `loop.v1`'s path does; context
  `decision`/`baseline: "append"`, scope `judge:continue`. `Ask.id` and `new_id()` as the stack's convention.
- turn.rs gains 16 lines (the input's field, the row's field, the dispatch and its mark); theseus-core's guide a
  paragraph under Context.

**How it is proven.**
- **The session's tests** (`tests_continue.rs`, 10): pure `compile()`, each signal beside its near miss (dormancy at
  361 minutes fires, at 360, 359 and 0 not, and not at a later loop of the same turn; a tail at 55 % of a 100k window
  fires `tail_band` 50, 45 % not, 60 % to 80 % fires 75, 60 % to 65 % not; a report and a wake fire, an answered report
  not, nor an operator message named `task:…`; a cache miss under the same prefix fires, a never-warm cache not, nor
  one from another compilation, nor a warm one); the signals change no digest or decision; through the core against the
  fake Jev: a signal and no trigger gives one `judge:continue` row keyed by the id the mark names; no signal, no
  judgment; a deterministic trigger, no judgment (a `manual_transcript` recompile still carries `dormancy` on its row);
  the request bytes equal with the judge on and off (three requests each, as JSON); a judged turn keeps at most 5
  frames. Plus theseus-judge's id test and `config::judge`'s. Targeted 46 passed; under load (busy loops at nice 0,
  the tests at nice 19), three runs: 19 of 19 each. The output golden changed in six lines, each only gaining
  `"signals":[…]` (the report and wake scenarios' rows, spans and notifications); the append wire fixture gained a
  `dormancy` signal on purpose.
- **The session's planted reverts:** dormancy at any gap (the near-miss assertion failed, "360 minutes fired"); a
  dispatch even after a trigger (a `judge:continue` row appeared).
- **The VM's gate**, under `TZ=America/Los_Angeles`: 1,972 run, 1,939 passed, the 33 failures all L1 cases of the root
  VM (theseus-pv6i), 17 skipped; the turn bench by hand, frames 5 and 9. A live check on scratch daemons there with a
  fake model and an unreachable Jev: a 125 s wait made turn 2 an `append` with `dormancy` 2, `continue.v1` was recorded
  `failed: network`, the mark named its row, and a judge-off daemon gave equal request digests turn for turn.
- **At the review** (R2, on its review commit of 23b, 24 and 25a): 340 of 340 on the merged tree after the join fixes
  (334 before). Planted reverts, 2 of 3 caught: a trigger asking anyway; dormancy firing at its minutes. The third, the
  compile point's judgment made on the turn's path (`block_in_place`), passed 25 of 25: the fake Jev answers at once
  and no test times a turn at this point, unlike 24's and 25a's (theseus-bhn2, P2; the code spawns).
- **Live, at the review**, on a scratch daemon of all four branches with GLM (glm-5.3-flash), the real Jev and
  `dormancy_minutes = 1` (Jev $0.000289): turn 1 a `recompile` on `new_session`, no signal, no `continue.v1`; 75 s
  later turn 2 an `append` with `dormancy` 1 ("the new input came 1 minute after the node before it"); `judge log`
  showed `continue.v1 (shadow) · decision=append 0.78 confirm · stronger_model_for_compaction=no 0.20` (37
  micro-dollars) beside the inbound pair and two `loop.v1`s; turn 2's trace marked all four points
  (`classify.v1@inbound`, `role.v1@inbound`, `continue.v1@compile`, `loop.v1@loop_end`) in 5 frames.

**The join** (07:25, by the batch-5 harvest wake, on 2cba6c51, the last of the four judging joins). rerere replayed ten
of R2's eleven conflicted files, the rewritten golden included (it differs from main's in the six `context.compiled`
lines alone); R2's resolve.py resolved config.rs (`SignalsConfig` in main's re-exports, beside hands-cancel's
`pub use aws::…`) and applied the join fixes: the third `new_id()` went; the new tests' literals gained 30b's
`sources` and 36c's `prompt` (three E0063s); the tests where the points meet (the marks helper counts the compile
point's; 25a's `inbound_only` turns `continue.v1` off too; health lists six packs); and two ceilings rose with their
reasons, turn.rs from 3,500 to 3,512 and theseus-protocol's lib.rs from 2,645 to 2,646. compiler.rs ended at 2,499.
Its gate (exit 0 at 07:24:43): 2,196 of 2,196; lifecycle every line under its strict budget; turn frames 5 and 9,
plain p50 85.7 ms and tool-call 193.4 ms (8b804784: 83.0 and 183.1; the daemon's p50 57 and 155 against 56 and 151;
CPU pressure 99 % during the bench; fdatasync 6.9 ms): no cost the bench can see, since the signals run on every
compile with the judge off and this gate is their measure. A frozen A/B would put a precise number on it; none was
run. Pushed 07:25; theseus-0j2.7 closed, and the learning ledger (25c) launched from this main.

**The install** (14:09, at bddfd407, install #1): the signals ride on every `context.compiled` row of Eddie's daemon,
and with `[judge]` on `continue.v1` judges his appends in shadow. Health after the restart: the config from
`/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms; the
store from format 6 to 14 on start.

**Divergences and choices.** Every signal is measured since the model's last answer, so `tail_band` and `cache_miss`
can fire at a later loop of a turn (tool results crossing a band), and `continue.v1` is then asked mid-turn, in shadow.
Dormancy measures the first input after the last answer, so after a turn that failed before answering, a later turn
measures the older input's gap. The sizes are estimates from bytes at the catalog's figures, images left out. Tasks'
turns are judged too: nothing restricts the pack to conversations.

**Known gaps.** theseus-bhn2 (P2: no test that a slow Jev never delays a turn at the compile point). theseus-fi5n (P3:
no workload class on `continue.v1`'s judgments). The sink's frame can land inside a later turn's window (the
theseus-0j2.3 shape). turn.rs stood at its raised ceiling of 3,512, and compaction-roots would push it again: the
morning notes said it wants a split, with no issue then (30c's join moved `compile_step` out, Item 131).

### Item 123. `theseusd restore --from s3://…`: a store rebuilt from what the durability tender shipped, the rows deciding, the next start shipping nothing again (theseus-mgw.10; roadmap row 33, step 16; the fifth cloud batch's restore-s3 session, launched 02:19 and fired 02:22 from 4bb5aac9, the durability tender's join, Opus 5.5; 01e4ed0f; reviewed 05:30 to 06:08 by local reviewer R1; joined 07:40 at cc81333d, a signed merge onto 4bae0f74, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1, unused until a restore)

**Why.** §6 makes rebuilding a node from its S3 segments and the DynamoDB index a first-class, tested path from the
first release, since S3 is presented as disk-failure recovery. Step 15 (Item 108) ships the WAL's
segments, tails and blobs to `s3://theseus-<account>-<region>/durability/<deployment>/` with a row per object in the
`theseus-durability` table. Step 16 reads them back.

**What landed** (the merge: 9 files, +1,653 −32; no new package, no protocol type, no config key; no stored field or
record kind, so the store's format stays 11). The local restore (`restore.rs`) is unchanged: the S3 path fetches into
a directory laid out as a store's, then calls it.
- **The reads** (`aws/durable/read.rs`, 276 lines). `Reader::get`: one `GetObject` with `ChecksumMode` up to 8 MiB,
  S3's whole-object SHA-256 compared when it keeps one, and `Range` reads past that (the client holds a response whole,
  capped at 16 MiB, and a segment is up to 64 MiB); every object is then checked against **its row**, by length and
  SHA-256 (a sealed segment's hex, a tail's base64, a blob's name). `Reader::query`: `Query` on the deployment's key
  with `ConsistentRead`, following `LastEvaluatedKey` over every page.
- **The restore session.** The owner role under an inline policy that only reads: `s3:GetObject` on
  `durability/<dep>/*`, `s3:ListBucket` with `s3:prefix` `durability/<dep>/*`, and `dynamodb:Query` on
  `theseus-durability` with `dynamodb:LeadingKeys` `<dep>#*`. It is named `theseus-restore` (kind
  `Tender("restore")`) and minted for **the URL's deployment**, not the config's; the reads sign with it. An account
  with no `owner_role` is refused, and the key signs only STS.
- **The fetch** (`aws/durable/fetch.rs`, 327 lines): the rows decide what is current, never a listing. A segment comes
  from its sealed object when there is one, and its tails are then not read; otherwise its tails are stitched from
  byte 0, each starting exactly where the last ended, and tails past where the join stops are recorded as unjoined,
  said, and left in S3. A segment with no row, one whose first position (read from its first frame) does not follow
  the last restored position, or a cut segment that a later one follows, is a gap: the restore stops there, says so,
  and never fills it. A blob S3 lacks is said; any object whose bytes differ from its row refuses the whole restore.
- **The orchestration and the seed** (`aws/durable/restore.rs`, 363 lines). `from_s3` checks, before any request, the
  URL (`s3://<bucket>/durability/<deployment>/`, IAM-name characters only), the socket (refused while a daemon
  answers), the account (the config's whose bucket the URL names, with `durability = true`), and an occupied store
  without `--force`; then the session, the rows, a fetch into `<state>/store.from-s3-<ms>/`, and the local restore; the
  staging copy is removed whether it succeeds or not; `<state>/durability/` is moved aside to
  `durability.before-restore-<ms>` (the old store's `blobs.shipped` would otherwise say blobs were shipped that the new
  prefix never got). **The seed**, only when the config's deployment is the one restored: the tender's cursor goes
  just past the last frame at or before the restored last position (before `store.restored`), `rows_to` there, and
  every restored blob is marked shipped, so the next start ships only the restore's own row. The cursor's segment stays
  open to tails, except when `store.restored` rotated into a new segment and S3 already holds this one whole; a sealed
  segment that grew after the restore is shipped again whole when it seals, never lost. The lines say where each
  segment came from ("segment 3 from its object, segment 4 from 7 tail(s)"), the unjoined tails, the gap, the missing
  blobs, and, with the same deployment, warn: "a daemon started on this store ships into s3://…/".
- **theseusd** (`main.rs`, +63): an `s3://` `--from` resolves the secrets and builds the AWS client; `--repair` with an
  `s3://` URL is refused; the local report's lines are `restore_lines()`, shared by both forms.

**How it is proven.**
- **The session's tests** (`aws/tests_restore.rs`, 7; each ships a store with the real tender through the fake, 3 rows
  to a Query page and 100 bytes to a range): a shipped store restores equal to the original (records, last position,
  17 session ids and labels, the blob's bytes; more than 2 Query pages followed; the staging copy gone); the restored
  store's next start sends **exactly one new object** (the `store.restored` frame's tail), and a second restore holds
  both `store.restored` rows; another deployment seeds nothing and keeps the old cursor aside byte for byte; a sealed
  object is preferred and a stale tail is not stitched (a log cut back after tail a, then b' written: the restored
  segment equals the live one, with b' and not c); a corrupted object is refused by its checksum (a blob replaced by
  bytes of the same length, with S3's own checksum updated, so only the row's digest can tell; also a sealed segment's
  flipped byte); a missing blob and a gap are said; the restore session only reads (its actions exactly
  `s3:GetObject`, `s3:ListBucket` and `dynamodb:Query`), and while a listener holds the socket the fake sees **zero**
  requests. The fake gained `GetObject` with ranges and checksum mode and a paged `Query`. 15 of 15 with step 15's;
  under load (busy loops at nice 0, the tests at nice 19), six runs, 15 of 15 each.
- **The session's planted reverts:** no SHA-256 check (a forged blob of the same length restored; the first plant had
  failed only on length, so the test was made to prove the digest itself); tails stitched without the contiguity check
  (the restored sessions included "c, from a log that will be rewound").
- **The VM's gate:** 2,014 run, 1,980 passed, 34 failed: the root VM's 33 L1 cases (theseus-pv6i) and the UTC golden
  (theseus-ig6n).
- **At the review** (R1, on a4da5e1c): 136 of 136 (theseus-core's whole `aws::` suite and the local restore's tests,
  119, and theseusd's AWS, config, hands, tender and versions binaries, 17). Planted reverts, 4 of 6 caught: the seed
  marking no blob shipped; the cursor past `store.restored` (`seeded_at` 41 for the restored 40); no refusal while a
  daemon answers; a sealed segment stitched from its tails (caught twice). Two passed: the fetch's position-gap check
  gone (no test builds a segment whose row is in order but whose first position does not follow; without the guard the
  local open refuses the whole restore instead of restoring the prefix) and the seed's rotated case gone (waste, not
  loss). Both are theseus-b9x6 (P2), with the test each needs.
- **Offline at the review**, the merged build's `theseusd restore --from s3://…` against invented scratch configs whose
  account endpoint was a closed loopback port, so no request left the machine: `--repair` with an `s3://` URL, no
  `[aws]`, another account's bucket, a URL without `durability/<deployment>/`, an account without `durability = true`,
  a store in place without `--force`, and a listener on the socket each refused in its own words; a fresh state dir
  failed at the key's check and left nothing behind. The restore that reads S3 was not run: it spends on the account.

**What the session found.** Without a seeded cursor, a restored store's next start ships everything again: a fresh
state dir has no cursor, a stale one reads as `Rewound` and resets, and `put_once` checks S3 only for an object in
flight, so the tender's doc comment ("every object is checked against S3 before it is sent") overstates it. The old
store's `blobs.shipped` would leak into the next deployment (a step 15 problem). A sealed segment can grow after a
restore, since the local restore appends `store.restored` to the last segment when it fits.

**The join** (07:40, by the batch-5 harvest wake, on 4bae0f74). No conflicts, no resolve.py, no join fix (merge-tree
had also been clean onto 8e17eea7, 35784d7c and d3cad454). `CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. Gate run 1
was red at 07:33:17 on one test the branch does not touch, `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`:
it reads until "42" and asserts quiet, and under load the shell's next prompt lands after that read; the terminal
reports it rightly, so the test races (filed as theseus-y6zr, fixed at Item 126). Run 2, exit 0 at
07:40:26: 2,203 of 2,203, every lifecycle line under its strict budget. Pushed 07:40; theseus-mgw.10 closed.

**The install** (14:09, at bddfd407, install #1): `theseusd restore --from s3://…` is in Eddie's binary; nothing runs
it until a restore. Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord
ready, judge on (every pack shadow), lsp on, startup 73.1 ms; the store from format 6 to 14 on start.

**Divergences.** The command is `theseusd restore --from s3://…`, not `theseus restore`, and it signs in its own
`theseus-restore` session, not a tender's; it reads the rows, not a listing (§5 "Step 16" of the AWS design said
otherwise). A gap restores the prefix before it, said in the output, rather than refusing; the `store.restored` row
names the staging path, not the URL, and not the gap. A corrupt sealed object refuses the whole restore, even where
its tails might tile it.

**Known gaps.** **The live check on the account waits for Eddie** (under a cent): a probe config ships a few turns,
then restores into a fresh state dir; R1 adds one step, to delete one blob object under the probe's own prefix first,
since the session's `s3:ListBucket` carries an `s3:prefix` condition and S3 may answer a missing key with 403, not 404,
so a missing blob or tail would fail the whole restore instead of being said (the fix would be `StringLikeIfExists`;
the fake answers 404 either way). _(It was not needed: theseus-bfk9's live probes showed plain `StringLike` gives 404, and both sessions use it since 2026-10-06; Part III Item 198.)_ theseus-b9x6 (P2: the two blind guards, and R1's narrow case: after a restore whose
last segment came from its sealed object and took `store.restored`, a second restore before that segment seals again
reads the old object and says nothing). The residual stale-tail case: a rewritten frame ending exactly where an old
timeline's tail begins stitches in; an epoch in the tail rows would close it. A local `theseusd restore --from <dir>`
still leaves `<state>/durability/` in place, so its `blobs.shipped` can skip blobs. Blobs are fetched one at a time.
theseus-core's guide and `durable.rs`'s module comment owe the corrections above.

### Item 124. 34b's wire-in: each exam daemon's `[memory] arm`, the four-arm exam over the real recall pipeline, its report and the replay; and the first honest exam on GLM (theseus-6fn.5; roadmap row 55; the fifth cloud batch's memory-arm session, launched 02:59 and fired 03:03 from 760553f7, recall-node's join, Opus 5.5; a5630773, d9356d4c and a676f827; reviewed 05:30 to 06:24 by local reviewer R1; joined 07:50 at 7d0c9534, a signed merge onto cc81333d, by the batch-5 harvest wake; the GLM exam run 11:44 to about 13:04 under Eddie's decision 9, reviewed 13:45; installed 14:09 at bddfd407, install #1)

**Why.** The memory exam (34a, Part III Item 80) measured recall through a stand-in for it; Eddie's 6.2 pick on the
cut-list made the arm a scratch daemon's `[memory] arm`, one daemon per arm, so the exam measures the real recall
pipeline and gives M6 its first honest report (roadmap row 55). Recall itself was 30a and 30b (Item 99,
Item 112).

**What landed** (the merge: 23 files, +3,478 −382; `Cargo.lock` gains one internal edge, theseus-exam on
theseus-memory, and nothing new; the store's format stays 11: the rows name the arm and its sources through 30b's
fields).
- **The `bm25` arm** (a5630773). 30b's `MemoryArm` had `none` and `baseline`, and `Memory::begin` sent the index no
  sources, so `baseline` got the tender's default (BM25 and entities, vectors only in `hybrid` mode) and a tender
  without model files answered silently; and in `live` mode with arm `none` the turn still ran a shadow recall. Now
  `MemoryArm::Bm25` and `MemoryArm::sources()`: `none` asks for nothing, `bm25` for BM25 and entities, `baseline` for
  those and vectors, so a tender without its model says so in the row's `skipped` (`bm25_only: no weights in …`).
  A live arm asks for its own sources; shadow, a canary's control and `memory.search` ask for `baseline`'s; `live` with
  arm `none` asks the index nothing (one task and one query fewer per turn), and still writes its `memory.arm` row.
- **theseus-exam is a tool** (d9356d4c). `theseus-exam run` prepares one daemon per arm (`none`, `bm25`, `baseline`)
  from a base config: a copy of the exam's store, `[memory] mode = "live"` and the arm, Discord, the web UI and the
  MCP server off, no tender for `none`, the environment's `THESEUS_*` variables removed; all start before any is
  waited on, so their tenders index in parallel, and each is stopped once its tender has read through the store's last
  position (and embedded it, in hybrid mode): that stopped state is the arm's snapshot. Each run starts fresh daemons
  from the snapshots (a daemon indexes the turns it serves, so one that lived across runs would recall an item's earlier
  answer), runs its cells with the existing driver (the seeded paired order, the spend cap and resume unchanged), and
  stops them. Oracle cells go to the `none` daemon; every cell reads its session's recall rows and errs if a row names
  another arm or the `none` daemon wrote one. A dropped daemon is shut down, killed after 20 s, and any process naming
  its directory with it; the command fails if any process still names the work directory, and a half-made directory is
  moved aside, never deleted.
- **The oracle's note is the core's render** of a `Recall` node of the gold (each item cut as the pack cuts one, 400
  tokens, with its frozen range and header read from the store), sent after the task, where 30b's note sits.
- **The report** (`theseus-exam report --runs F [--rescore] [--out F]`): the plan's digest (embedded), the exam's,
  the models and the window; each arm's science digest and answering sources; each arm's pass rate with its 95 % t
  interval over items and its n, for all items, held in and held out; the paired differences clustered by item
  (`baseline − none`, `bm25 − none`, `oracle − none`, `baseline − bm25`, `oracle − baseline`), each `gain`, `loss` or
  `insufficient`; cost per pass, latency, recall p95, gold admitted; the decision per feature (recall: `baseline`
  against `none`; vectors: `baseline` against `bm25`) with the clause that decided it; what could not be measured. A
  report written with `--out` is frozen: an existing file is refused.
- **The plan**, `docs/m6-ablation-plan.md` (new): the arms and versions, the metrics and unit, the minimum samples (36
  held-in items × 3 runs per arm, the held-out half once, 120 canary sessions per arm), §2.9's decision rule verbatim
  with the exam's reading of clause 1, and the analysis. It pins no digest of `baseline`.
- **The replay** (a676f827, `theseus-exam replay`): over a copy of a store, every turn with a recall row whose query,
  rebuilt with the core's `query_of`, matches the row's digest and `as_of`; another copy served by a scratch daemon
  (tender on, memory off); `none`, `bm25` and `baseline` recomputed per turn through the real pipeline, **a hit at or
  after `as_of` dropped and counted as a leak**; silver labels (re-supply by an 8-word run, references by issue ids,
  hashes and paths first seen elsewhere, re-derivation, the operator's `should_have`); recall, precision and MRR per
  arm and label, and the stale rate.
- The reader rule: the crate's `reserved_for` became `tool = "run by hand beside theseusd: …"`; the crate gained its
  own AGENTS.md and CLAUDE.md, and the root map a line.

**How it is proven.**
- **The session's tests:** `tests_memory_arm` (live `none` makes zero index queries and writes no recall row; `bm25`
  asks for BM25 and entities and its rows say `bm25`; `baseline` asks for all three; shadow and a canary's control ask
  for `baseline`'s even under `arm = "bm25"`), 10 of 10 under load. `tests/arms.rs` end to end: a two-item exam, all
  four arms, two runs on fresh daemons, three real `theseusd`s with real `theseus-index` tenders and a stand-in
  Messages API model that answers with the note when its request holds `[Recalled:`: all 16 cells with a verdict;
  `none` 0 of 4, `bm25`, `baseline` and `oracle` 4 of 4; every row naming its arm; `bm25` rows never naming vectors;
  **the oracle note byte for byte equal to the baseline daemon's rendered `Recall` block** as the model received it;
  nothing left running (5 of 5 under load). The report's numbers read as computed by hand before the first run (t(1)
  12.706, t(3) 3.1824, t(5) 2.5706: e.g. `none` 17 % [0, 60], `baseline − none` held in +58 [−21, +100]). The replay's
  leakage test: an index offering every node regardless of `as_of` never gets a later node admitted, 2 leaks counted.
  All 61 theseus-exam tests passed in each of the session's three gates.
- **The session's planted reverts** (eight in all): live `none` running the old shadow recall; the live arm given
  `baseline`'s sources; every daemon `baseline`; every daemon `none`; the note before the task; the old header format;
  the replay's `as_of` guard off (the leakage and metrics tests failed). Each failed as it should.
- **A stand-in run on the VM**: the full exam-v2 store (758 sessions, 1,550 keyed nodes, through @5412), all four arms
  over the held-in half once against a fixed-answer fake: 144 cells, 0 errors, 0 kills, about 10 s; recall p95 10 to
  11 ms; **the real pipeline admitted 17 of the 38 held-in gold nodes under `bm25`** at the default pack of 6 items
  (`baseline` equal, with no vectors); a replay of that run's store: 36 turns rebuilt, 0 left out, 0 leaks.
- **At the review** (R1, on d3cad454): 152 of 152 (theseus-exam whole with `tests/arms.rs` through this tree's
  binaries, the core's recall suites, every test with `recall` or `memory` in its name, the config's, the reader rule).
  Planted reverts, 5 of 5 caught: shadow and a control asking for the configured arm's sources; `bm25` asking for
  vectors; the oracle's cells on the `bm25` daemon (caught three times); a daemon counted settled before its tender held
  the store; an interval touching zero deciding a gain. A stand-in run again: 144 cells, 0 errors, about 24 s; gold
  admitted `bm25` 17, `baseline` 18 (most likely each daemon's index of the cells it served); the frozen report refused
  a second `--out`. Under `timeout 900` a clean run **exited 1**, the wrapper counted as a leftover (theseus-qiiq).
- **The first honest exam, on GLM** (glm-5.3-flash), run under Eddie's standing go for paid checks (decision 9,
  11:26), with theseus-qiiq settled in how it ran (a scratch base config, never a copy of a real one; a script file,
  not under `timeout`; the Vectors line watched): the held-in half 3 runs and the held-out half once, **576 cells, 5
  errors** (2 GLM first-byte timeouts, 3 turns with no end in 300 s), **$0.87** of a $5 cap, no daemon left.

  | Arm | All | Held in (3 runs) | Held out (1 run) | Gold admitted, held in | Cost per pass | Recall p95 |
  |---|---|---|---|---|---|---|
  | `none` | 15 % [6, 23] | 15 % [3, 27] | 14 % [2, 26] | – | $0.0104 | – |
  | `bm25` | 46 % [34, 57] | 42 % [25, 58] | 50 % [33, 67] | 18, 18, 16 of 38 | $0.0035 | 8 ms |
  | `baseline` | 67 % [56, 78] | 70 % [55, 85] | 64 % [47, 80] | 26, 25, 27 of 38 | $0.0022 | 515 ms |
  | `oracle` | 100 % [99, 100] | 99 % [97, 100] | 100 % [100, 100] | – | $0.0015 | – |

  `baseline − none` +55 points held in and +50 held out (gain both); `bm25 − none` +26 and +36; `baseline − bm25` +29
  held in (13/1/22, p 0.002, gain) and +14 held out (6/1/29, p 0.125, insufficient); `oracle − none` +85 over all
  items (34a had measured about +74 on exam-v1). `bm25`'s 18, 18 and 16 reproduce the stand-in's 17; real vectors
  admitted about 9 more gold per run. The Vectors line read 4 of 144 recalls without vectors, each the first
  `baseline` recall of a run (the model loading). Both features stay at clause 3 (no canary data), so recall and
  vectors stay off by default. The review (13:45) checked the numbers against the exam's own frozen output.

**The join** (07:50, by the batch-5 harvest wake, on cc81333d). No textual conflict. R1's joinfix.py:
task-arrangement's join had given `Body` an `Arrangement` variant, and the replay's `kind_of` matches `Body`
exhaustively (E0004); it takes the arm main's own `Node::kind_str` has, `"arrangement"`. `CLOUD_TASK.md` and
`CLOUD_REPORT.md` dropped. Its gate (exit 0 at 07:49:54): 2,217 of 2,217; lifecycle's first pass missed on one
cold-start outlier (p95 131.7 ms; the branch adds nothing to the start path), and its rerun passed every line
strictly; turn frames 5 and 9, plain p50 78.6 ms, tool-call 177.9 ms. Pushed 07:50; theseus-6fn.5 closed.

**The install** (14:09, at bddfd407, install #1). Eddie's config set `[memory] mode = "live"` with arm `baseline`, so
his turns recall live through the arm's sources from then on (the exam binary itself is a tool, never installed).
Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on, lsp
on, startup 73.1 ms; the store from format 6 to 14 on start; `theseus memory search` ran on `baseline@8bc51e97` while
the vectors were still loading.

**Divergences.** The exam's driver and scoring stay in theseus-exam rather than becoming `theseus-sim exam`: the exam
needs no code in the shipped binaries, and theseus-sim stays the gate's tool. The replay is `theseus-exam replay` and
the report `theseus-exam report --out`, a file, with no `ablation.report` row and no `<state>/ablation/` (the design's
§2.9 and §3.1 said otherwise). No "asked sources" field in the rows (a format bump would buy one). The core's note is
its own text block after the message's, the oracle's inside the message's one block: the same characters in the same
order.

**Known gaps.** theseus-qiiq (P2: the driver's daemons inherit what a base config turns on beyond Discord, the web UI
and the MCP server, such as `[aws]`, `[mcp.servers]` and `[judge]`; the last check counts a wrapper as a leftover; each
run's daemons start with the embedding model unloaded, and a hybrid tender restarted from a snapshot can read 0 chunks
at first; the fix proposed is to wait for `vectors.chunks > 0`). Within a run a cell may recall another item's cell on
the same daemon (a core `exclude_sessions` would close it). The replay applies no place rule (a store copy has no
outbox), re-supply has no cosine, and "already stale at the turn" cannot be read. Clause 1 is read from the private
family alone. The optional replay over a copy of the operator's store was not run. The exam's fixtures name the
operator by first name (theseus-1xgs, filed 13:07).

### Item 125. The task record and its graph: three layers, compare-and-swap, the task tools, layer 1 on the gate's path, a task session closed by its report, and the graph in the model's view (theseus-ext.6; roadmap row 70, step 39a; the fifth cloud batch's task-record session, launched 04:16 and fired 04:20 from af6790da, task-arrangement's join, Opus 5.5; f79dd155 and 7c3bbb18; reviewed 06:45 to 08:01 by local reviewer R1, on 8b804784 and again on cc81333d; joined 08:20 at 78d749cb, a signed merge onto 7d0c9534, by the batch-5 harvest wake; store format 12; installed 14:09 at bddfd407, install #1)

**Why.** §3.5 makes the task graph a persisted core structure the agent sees every turn and edits through a reserved
action namespace, in three layers of different mutability so the agent cannot redefine success, with versioned
compare-and-swap edits. Until 39a a task was only a session (DD7): `task.create { brief }` opened a child execution,
`task.list` read executions, and nothing held a title past the brief's first line, a plan, acceptance, or evidence.
27's arrangement (Item 113) sat in `task.create`.

**What landed** (the merge: 55 files, +3,765 −56; no new package).
- **The record.** A new protocol module, `theseus_protocol::tasks`: `TaskRecord { id, version, title, objective,
  acceptance, state, parent, deps, owner, session, origin { session, principal }, evidence [{ node, identity, note, by,
  at_ms }], proposal { objective?, acceptance?, abandon, by, card, base_version, at_ms }, created and updated times }`,
  `TaskState` (§3.5's nine), `TaskGetParams`/`TaskGetResult`, `TaskChanged`, `TaskViewSummary`, stored as shown. A new
  store kind, `TASK = 12`; ids `tsk_<tail>` from the creating call's correlation id, and a task session's record shares
  its session's tail (`task_graph::of_session`), so no session record gained a field. **The store's format went from
  11 to 12** at the join (the branch had bumped 9 to 10 on its base; no existing record gained a field, and a format-9
  store still opens).
- **Lock and CAS.** `Store::lock_task`, a second lock map beside the sessions' (order: session, task, execution). Every
  edit names the version it read; a stale one is refused with the record as it is now ("task tsk_x changed since you
  read it: v1 → v2. It is now: …") and a `task.stale_refused` row.
- **The tools** (`task_graph/tools.rs`): `task.update { id, version, patch }`, `task.split { id, version, into }`,
  `task.close { id, version, outcome: done|abandoned, evidence }`, and `task.create` without `brief` as a plan item
  (`title`, `objective?`, `acceptance?`, `parent?`, `deps?`). Edits run in the harness, and their records and rows ride
  in the frame that settles the call (so they are safe to repeat). `task.create` with a `brief` keeps 27's rules and
  writes the record in `open_task`'s frame (`in_progress`, its objective and acceptance from the arrangement's standing
  pieces). The schema's `required: ["brief", "arrangement"]` is gone, since a plan item has neither; a brief without an
  arrangement is still refused in code.
- **Layer 1 on the gate's path.** `Plan` gained `authority` (`serde(skip)`), and `policy::decide_with` asks at every
  posture when it is set, right after the floor: a patch with `objective` or `acceptance`, and a close `abandoned`, set
  it. The asking frame writes the proposal on the record under the task's lock (the version does not move) with a
  `task.change_proposed` row; the owner's yes (`theseus confirm` or the cockpit's card; refused from a shared place,
  and by the CLI inside a job) applies it in one frame (`task.change_accepted`, version + 1, proposal cleared); a no
  clears it in the answer's frame (`task.change_declined`). Abandoning writes `change_accepted` then `closed`. Plan items
  need no arrangement.
- **A session's task.** Its running states are derived when read (`state_now`: queued or running read `in_progress`,
  waiting on a confirm `waiting_human`, a budget question, cancel or exhausted budget `suspended`); the record is written
  at its own changes; the report closes it `done` with `report:<session>` evidence naming the report's node, and a
  failure `failed` with its reason, in the frame that ends the task. A cancel writes nothing.
- **The view** (`task_graph/view.rs`). Scope per §3.5: a conversation sees the tasks it started and their subtrees; a
  task session its parent's line and its subtree. A line per open task (id, title, state, owner, deps, the first
  acceptance line, version), a closed subtree folded to one line with its count, about 1,500 tokens; past that, open
  tasks only, then as many as fit, with "(N more left out …)". It is appended as the last block of the request's last
  message after `compile()` (which stays pure), with the conversation's cache marker copied onto the block before it,
  so the next request still shares a cached prefix; its tokens join the estimate, and `context.compiled` gains `tasks:
  { digest, open, closed, lines, left_out, tokens }`. No task in scope, no change to the request.
- **Surfaces.** `task.list` keeps every field and adds `records`; `task.get` (new); `task.changed` (a new
  notification with the record and the verb); `theseus tasks` prints the tree after the task sessions; ledger rows
  `task.created`, `updated`, `split`, `closed`, `change_proposed`, `change_accepted`, `change_declined`,
  `stale_refused`; a narrative line per change ("task tsk_x split into 2 (v1 → v2)."); metrics
  `theseus.tasks.changes {theseus.task.verb}` and a `theseus.tasks.open` gauge. The template gained commented
  `task.update`/`split`/`close` lines under `[policy.tools]`.

**How it is proven.**
- **The session's tests** (`task_graph::tests` 6; `tests_task_graph` 6): three plan items, a split, a stale edit refused
  with the record as it is now, a close with `commit:…` evidence, a second close refused, every stored version's
  evidence a prefix of the next, the view with versions in the next request, `context.compiled.tasks`, `task.list` and
  `task.get`; a plain turn carries no view; a layer-1 change waits at posture `open`, a guild channel's answer is
  refused with the proposal kept, the CLI's yes applies it (v2, and the next view shows it), a no leaves it, abandoning
  asks; a task session's record closed by its report (and 27's refusal of a brief without an arrangement writes no
  record); a failure closes `failed` and a cancel reads `suspended`; the records survive a reopen, and a format-9 store's
  task sessions still list with no records and no view. The frame budget unchanged (plain 5, tool-call 9). Under load
  (busy loops at nice 0, the tests at nice 19), five runs: 12 of 12 each. Four keyword stand-in models had to learn to
  skip the view's block (a task title had matched a script's keyword).
- **The session's planted reverts:** `check` accepting any version (two tests failed); an objective patch applied at
  once (nothing asked, `left: 0`).
- **The VM's gate:** every phase passed; the suite failed only on the root VM's sandbox tests (theseus-pv6i); the turn
  bench by hand, 5 and 9.
- **At the review** (R1): 1,129 of 1,130 on cc81333d (theseus-core whole, the protocol, the store, the CLI, theseus-sim's
  fake model, and theseusd's versions, config, tasks, wakes and job-approval binaries); the one, theseus-y6zr's pty
  flake, passed 3 of 3 alone. Planted reverts, 5 of 5 caught: the view's scope leaking (every conversation seeing every
  root task); the report's frame closing no record; a declined change leaving its proposal; the breakpoint not moved
  before the view (`cache_control` null where `ephemeral` was due); and the join's own `harness_done` dropping a task
  edit's records (the plan test found 0 records of 3), so the resolution's threading is under test.
- **Live, at the review**, on a scratch daemon with the scripted stand-in model (no key, no cost): three plan items, a
  split (the second v2, two children), a close with `commit:a1b2c3d` (`done` v2), a stale edit refused with one
  `task.stale_refused` row; a layer-1 change (`ask` exits 6; `theseus confirm` lists "task.update — approve (layer 1:
  changing task …'s acceptance is the operator's to accept …)"), accepted to v2 and another declined at v1; `theseus
  tasks` "task graph: 4 open, 1 closed"; the last `context.compiled` carried `tasks` (open 4, closed 1, 5 lines, 237
  tokens). The first try's yes ran nothing: the stand-in names its calls by index (`toolu_fake_0`), so the layer-1 call
  shared an id with an earlier answered call, and resume read it as answered (a rig limit, theseus-w6uh, P3). The GLM
  run of the report's checks (a model's own planning) was left for the maintainer.

**The join** (08:20, by the batch-5 harvest wake, on 7d0c9534). rerere replayed 14 files from R1's tree; R1's
resolve.py the fifteenth (theseus-core's lib.rs, the branch's test module given its own `#[cfg(test)]`) and every
renumbered value from main's side: the store's format main's 11 + 1 = **12** (its pins in the core's store test,
theseusd's versions test and the layout reader's note), the template's `[policy.tools]` count 43 + 3 = 46, the
instruments 22 + 2 = 24, and keep-both beside the judging stack (`signals` and `tasks` in `ContextCompiled`; the
`judge.scored` and `task.changed` variants). The one resolution with substance: main had split `run_harness` (43a's
`harness_started`, `run_extend` and `harness_done`) where the branch threaded its edits through it; the merge keeps
main's split, with `harness_done` taking the completion and the edit together, so the edit's records ride the
completion's frame and its task locks drop after it. R1's joinfix.py: the rig's `BoundPlace` and `TurnRequest`
literals (bindings-v2's and 36c's fields), `Core::dispatch`'s `too_many_lines` expectation dropped (merged, it is 97
lines), the golden's two wake offsets back to main's sign (the branch had regenerated it on a UTC machine), and the
ceilings with their reasons: turn.rs 3,523, theseus-protocol's lib.rs 2,654. Gate run 1 was red at 08:12 only on
theseus-y6zr's pty flake; run 2: 2,230 of 2,230, lifecycle strict, turn plain p50 72.5 ms and tool-call 170.8 ms (main's
level), frames 5 and 9. Pushed 08:19; theseus-ext.6 closed. Its join released independence (28a,
Item 132), launched 08:20.

**The install** (14:09, at bddfd407, install #1). The store went from format 6 to 14 on start, this join's TASK kind
among the steps. Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready,
judge on, lsp on, startup 73.1 ms.

**Eddie's calls.** At 10:38 (decision 6, "Take your recommendation"), option (b): layer 1 (a task's objective and
acceptance, and abandoning it) asks only for tasks the owner created or whose objective and acceptance came from his
brief; the model's own plan items change freely, still versioned and visible; an expired proposal is cleared as a
decline is; the view's O(N) scan waits for an index. Batched for the review-smalls cloud row (smalls-tasks,
Item 146).

**Divergences.** Layer 1 is answered by the owner, not "the requesting principal (or owner)": `judge_act` already
restricts answers to the owner in a private place, and a non-owner requester has no way to answer (left for 39b's
card). A whole patch waits when any of its fields is layer 1. The view is recomputed every loop, so a loop after a
split sees it, and adds one explicit cache breakpoint per request in a session with tasks (within Anthropic's four;
GLM's handling of a block-level `cache_control` was not measured). Leases, the board, `/tasks`, the Discord card and
the cockpit's graph are 39b's.

**Known gaps.** An expired proposal stays on the record, and the view says "a change waits" though nothing does
(decision 6 clears it, in smalls-tasks). The view scans every TASK record each loop (O(N); an index by origin session
would fix it). The metrics count only edits made in turns, and the gauge is the last view's count in scope. theseus-w6uh
(P3: the stand-in's call ids repeat across turns, and whether the core should key a result to its call by more than the
bare id). The CLI's `theseus tasks` help line does not yet say it shows the graph.

### Item 126. `categorize.v1` in shadow at a private conversation's exchange end, its proposals accepted or rejected, and parked tasks in health; with the terminal test's fix gated beside it (theseus-vug.1, with theseus-y6zr; roadmap row 49, step 28b; the fifth cloud batch's categorize-shadow session, launched 01:41 and fired 01:45 from 3add3f53, Opus 5.5; 1cefb9ef; reviewed 06:50 to 08:01 by local reviewer R2, fifth of its stack; joined 08:34 at 911b0437, a signed merge onto 78d749cb, by the batch-5 harvest wake, pushed 08:33 with b332bfd7, theseus-y6zr's fix, a direct commit on top; installed 14:09 at bddfd407, install #1)

**Why.** §4.1a's ontology lets a session belong to topics; M5's design has `categorize.v1` propose a session's topic in
shadow at `exchange_end` (once 10 human messages have arrived since its last judgment, or at the first exchange end
after 30 minutes' quiet), with no live memberships until M6 (`docs/design/m5-judgment.md` §2.4 and §2.12). The same
row carries the parked-task invariant: a task that waits on nothing that will wake it shows in health (`tasks.parked`).

**What landed** (the merge: 40 files, +2,663 −28; no new package, no config key; no stored record gains a field, so the
store's format stays 12: a ledger kind and a META key were added).
- **The point** (`judge/categorize.rs`, 416 lines). 23a's `after_turn` runs after the turn's last frame and its session
  hold, so whatever it spawns is off the turn's path and outside its trace. At a turn the baseline ended with no tool
  calls (the same exchange end as `loop.v1`), the point skips a task and, in the spawned task, any session whose place
  is not private. The decision reads the session's mark, a META record `judge.categorize.<session>` = `{judgment,
  through, through_ms, at_ms}`, then only the session's records after `through`, and asks `due()`. A human message is a
  `UserMessage` of origin `Operator`; a task's brief and report, a wake's note and the harness's notices are not. "The
  first exchange end after 30 minutes' quiet": the run of human messages that began the latest exchange came 30
  minutes or more after the node before it (or the mark's message, right after a judgment); a session's first message
  is never quiet; the mark moves past the run, so one quiet brings one judgment. Once due, the point builds the state
  (the session's title and its last ten human messages), writes the blob, reserves, writes the mark (one small META
  frame, off the turn's path) and calls Jev. **No topic declared, no judgment.** No trace mark: the dispatch runs
  outside every turn; the core still mints the id and the mark names it.
- **Candidates** come from the ontology's snapshot: the categories the kinds table lets the operator assign (topics
  today), each sent as `name: description`; a topic whose local id is `none` or `new_topic` is never offered (it would
  collide with the pack's own options); past 50, the session's own topics first, then described ones, then the rest,
  by name, with `candidates` and `candidates_left_out` in the judgment's context. Up to 5 memberships, newest first,
  for `still_member`. `JudgeService::attach(&Arc<Core>)`, called once in `Core::build`, holds the core by `Weak` (the
  service's only new reach).
- **Proposals** (`rpc/proposals.rs`). `ontology.proposals {session_id?, limit?}` scans scope `judge:categorize` once
  and lists, newest first, each answered judgment whose topic is `new_topic` or one the session is not in, with no
  operator label. `ontology.proposal.accept {judgment, topic?, description?, note?}` and `.reject {judgment, note?}` go
  through `judge_act` first. An accept writes the operator-origin membership with its `ontology.membership` row and the
  `judge.label` row in **one frame** (for `new_topic`, `--topic` names an existing topic or a new one created, with
  `--desc`, in that frame); a reject writes only the label; a second answer is refused. The label: `lbl_<uuid>`, scope
  `judge:categorize`, `question: "topic"`, `label` `accepted` or `rejected`, `answer` (Jev's choice), `topic`, `source:
  "operator"`, who and through what, `weight: 1.0`, `note`, with a narrative line. The CLI: `theseus ontology proposals
  [--session S] [--limit N]`, `accept JUDGMENT [--topic NAME] [--desc TEXT] [--note TEXT]`, `reject JUDGMENT [--note
  TEXT]`; accept and reject are refused inside a job.
- **Parked tasks** (`parked.rs`, `Core::tasks_health`). Health gains `tasks: {parked: [ParkedTask{task_id, short,
  execution_id, title, state, blocker, detail, since_ms}]}`. A task is not parked with a running turn or a queue place,
  a pending resume, report wakes, a wake on actions (a job), a due time, another execution, a pending wake of its own
  while it waits on input (37b), or an approval or budget question younger than 24 h; it is parked when it waits on
  input with no wake (`input`), was stopped (`stopped`), holds a question 24 h old or more (`approval`, `budget`), is
  `blocked`, or waits with no wake at all (`nothing`). `theseus health` prints `tasks parked: N` and a line per task,
  and nothing when none is parked.
- **theseus-y6zr's fix** (b332bfd7, a direct commit on main, `crates/theseus-core/src/term/tests.rs`, +4 −1): the
  terminal test read until `42` and asserted quiet, and under load the shell's next prompt landed after that read; it
  now reads until the prompt on the row after `42`. It had failed restore-s3's and task-record's first gates
  (Item 123, Item 125).

**How it is proven.**
- **The session's tests** (`tests_categorize.rs`, 11, against the fake Jev): nine human messages, each followed by a
  reply and a harness-relayed message, do not trigger and the tenth does; 30 minutes' gap triggers and 29 does not, a
  burst counts from its first message, a first message is never quiet, nothing new since the mark means no judgment;
  candidates from the kinds table (63 topics: 50 offered, 13 left out, the session's own first, undescribed ones
  dropped first); ten messages bring one judgment and its proposal (nine bring no Jev connection; the row keyed by its
  id with trigger `count` and 2 candidates; the mark naming it; Jev offered `harbor`, `garden`, `new_topic` and
  `none`; `topic:harbor` at 0.93, band `act`; an 11th message no second judgment); an accept writes the membership and
  its label in one frame (`frames_appended` + 1) and a second answer is refused; a `new_topic` accept without
  `--topic` is refused, with it one frame makes the topic, the membership and the label, and a reject writes its label
  alone; a refused accept writes nothing (a non-owner on a shared channel and the owner on a shared channel); a shared
  place's session and a task are never judged; a slow or failing Jev (down, 10 s, malformed) changes no turn (the
  request bytes, output, stop reason, loops and cost as judge-off, at most 5 frames, under 3 s); parked detection on
  scripted states (a question at 23 h not parked, at 25 h parked, "an approval unanswered for 25 h"). Under load, five
  runs: 22 of 22 each.
- **The session's planted reverts:** every message counted, not human ones (`left: Some(Count), right: None`); an
  accept skipping `judge_act` (it wrote the membership and a label).
- **The VM's gate:** 1,974 run, 1,940 passed, 34 failed (the root VM's 33 L1 cases and the UTC golden); theseus-sim's
  seeded-fault flake passed on its third try; the turn bench by hand, 5 and 9.
- **At the review** (R2, on its stack of 23b to 25b): 356 of 356 after join fixes 2 and 3 (351 before); the config
  tests 52 of 52 after fix 5; the cockpit 37 of 37. Planted reverts, 6 of 6 caught: a relayed message counted as human;
  an accept skipping `judge_act`; a shared place's session judged; a task's exchange end judged; the mark not moved at a
  dispatch; a task waiting on input with its own pending wake listed as parked.
- **Live, at the review**, on a scratch daemon of all five branches with GLM (glm-5.3-flash) and the real Jev (Jev
  $0.0036 over 55 calls, every pack; GLM about $0.01): ten mooring questions after two topics brought `categorize.v1
  (shadow) · topic=harbor 1.00 act · $0.000032 · 104 ms`, listed by `ontology proposals`; the accept ("ses_… joins
  topic:harbor (yours, operator)") wrote one `judge.label` row (`accepted`, answer `harbor`, via `cli`, weight 1.0).
  With **fifty topics**, a judgment offered 50 candidates and used **1,798 tokens in and 477 out: reserved 82
  micro-dollars, cost 96**, the reservation 17 % under the cost (with 2 topics: 688 in, 54 out, reserved 37, cost 32),
  which answers the pricing's open question on a 52-option Choice (theseus-q0rn). A task waiting on a fresh approval
  was not parked (`tasks.parked` `[]`).
- **The y6zr fix**, before the gate: 15 of 15 under load.

**The join** (08:33 to 08:34, by the batch-5 harvest wake, on 78d749cb). rerere replayed 7 files from R2's tree;
R2's resolve.py resolved 4 more (among them theseus-core's lib.rs, bindings-v2's `mod tests_ceilings;` beside `mod
tests_categorize;`, and the generated index.ts beside task-record's types) and applied its join fixes: **two
`JudgeLabel`s** (E0428: 24's press label and the branch's proposal answer, both §2.5's row) became `JudgeLabel` and
`ProposalLabel`, two facts of one row kind, the rows unchanged; `prompt: None` in the tests (36c); the judge points
met in tests (the rig turns every other wired pack off, since 25a judges every message and inbound's failures against a
down Jev opened the shared breaker before categorize's call; 25a's `inbound_only` turns `categorize.v1` off too); the
guide's words under main's judge bullet; the template's `[judge]` words name seven packs; `BoundPlace`'s two new fields
from `Default` in the rig (a main past bindings-v2 only); theseus-protocol's lib.rs ceiling to 2,663. Then the y6zr
fix as its own signed commit on top, sharing the gate: the harvest bent its rule against batching beside a turn-path
branch, since y6zr had failed two of the last three join gates at five minutes a rerun with about ten joins to come,
the fix touches no bench and no product path, and a terminal failure would point at the fix and any other at the
branch (overnight decision 19). The gate: 2,243 of 2,243; lifecycle measured under the busy allowance (CPU 55 %), every
line inside the strict budgets; turn plain p50 84.3 ms and tool-call 179.8 ms, inside the hour's spread on unchanged
turn code (plain 72.5 to 89.0 ms from cc81333d to 78d749cb; the exchange-end point returns at once with the judge
off); frames 5 and 9. Pushed 08:33; theseus-vug.1 and theseus-y6zr closed.

**The install** (14:09, at bddfd407, install #1): with `[judge]` on, Eddie's private conversations get a
`categorize.v1` judgment once topics exist, and health carries `tasks.parked`. Health after the restart: the config
from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on, lsp on, startup 73.1 ms; the store from
format 6 to 14 on start.

**Eddie's calls.** The two-facts naming (`JudgeLabel` and `ProposalLabel`) was put to him at 9am and stands. At 11:00
(decision 8, "Take your recommendations") he chose `new_topic` discovery on an empty ontology, with theseus-gky0 (read
only what is new) and theseus-q0rn (the reservation's estimate), batched for the review-smalls cloud rows and
built in Item 147.

**Divergences.** The mark is a META record in a frame of its own at dispatch, never riding the sink's frame; a paused
judgment moves no mark (the next exchange end tries again), and a Jev failure after dispatch does not roll it back, so
a failing Jev is not asked on every exchange end. The dispatch marks no trace. An accept refuses a session already in 3
topics (the ontology's `AtMost(3)`), and the proposal stays.

**Known gaps.** theseus-gky0 (P3: the mark is written only at a dispatch, so with no topic declared each exchange end of
a private conversation reads the session from its start, doubling `loop.v1`'s read). theseus-q0rn (P3: a 50-topic
judgment's reservation is 17 % under its cost). `ontology.proposals` scans the whole `judge:categorize` scope at each
call. The cockpit's Ontology view owes a proposals panel (the report says what it should show), and health's
`tasks.parked` a row on a board. The report's live-check step 5 needs a command off the template's `allow_argv`
(`ls` is on it, so a tightening leaves it alone).

### Item 127. L3: a language server's diagnostics in an edit's result, pending ones on the session's next result, and `[lsp]`'s edit keys (theseus-n88g.9; lane L3 of the worth plan; the fifth cloud batch's lsp-diagnostics session, launched 04:27 and fired 04:30 from 94e01826, lsp-board's join, Opus 5.5; a1f966f4, ea388d77 and 91ac2164; reviewed 08:01 to 08:37 by local reviewer R1; joined 08:45 at 94304c3b, a signed merge onto b332bfd7, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1, with `[lsp]` on)

**Why.** The worth spike (Item 93) found diagnostics in edit results to be the main feature of LSP tools, as Claude Code
and OpenCode do it. L1 built the client (Item 96) and L2 the board and the `lsp.*` tools (Item 114); L3 puts
a written file's errors into the edit's own result, so the model sees what its edit broke before its next step.

**What landed** (the merge: 13 files, +1,273 −10; no new package; no stored record changes, so the store's format
stays 12: `meta.lsp` rides in a result node's existing `meta`).
- **The hook** (`lsp/edits.rs`, new, 661 lines; one call in `toolrun::run_inproc`, `ToolRuntime::lsp_onto`).
  `run_inproc` has the toollet's result before it builds the result node that rides in the completion's frame, so the
  hook sits between them, on the turn's task. It does nothing unless the call's place is private (the place rule) and
  the call was not settled by a cancel. The files written come from the fs tools' `meta` (`path` for `fs.write` and
  `fs.edit`, `files[].path` for `fs.patch`, `wrote[]` for L2's `lsp.rename`). For each whose root has a server up, the
  server's other open documents are synced first, the file is announced (`file_changed`), and one task per file waits
  for `diagnostics(path, …)` through `Board::call`, so the request is an `lsp.request` span under the call; the hook
  waits up to the bound. The block, appended to the result:
  ```
  Errors after this edit:
  /w/a.fake (fake): 26 errors
    /w/a.fake:3:1 error [F1]: planted error (fake)
    … (20 lines at most, errors first, then warnings; hints and notes left out)
  …[6 more not shown: lsp_diagnostics with a path lists a file's]
  Other files: 1 new error during this edit (lsp_diagnostics lists the open files').
  ```
  A clean file reads `…: no errors`; one the bound beat reads `…: pending: fake had not answered for it within 1500
  ms`. `meta.lsp` holds `server`, `freshness` (`pulled`, `pushed`, `stale`, `pending`, `failed` or `mixed`), `errors`,
  `errors_shown`, `warnings`, `other_errors`, `waited_ms` and `files[]`, so a later step can count it from the record.
  The logic lives in a method because an inline `if let` pushed `run_inproc` past clippy's cognitive-complexity limit
  (27 of 25).
- **Other files' new errors** come from `Client::pushed_errors()` (a small addition to L1's crate), read before and
  after the wait: only pushes count, so a pull-only server (ty, tsgo) reports 0.
- **Pending diagnostics** (ea388d77). A file the bound beat keeps its wait task (within the server's request timeout)
  in the board's per-session map, at most 32 per session, the oldest dropped. The session's next edit or `lsp.*` result
  first takes the finished waits and renders them under "Diagnostics that arrived since an earlier edit:", with
  `meta.lsp.arrived`; each is taken once. A later edit of the same file aborts the old wait (the client sends
  `$/cancelRequest`), and its own wait replaces it. Another session never sees them. They live in memory, best effort: a
  restart loses them, and the record keeps the edit's result, which said "pending".
- **The keys** (91ac2164): `[lsp] edit_diagnostics` (true), `edit_wait_ms` (1,500, checked 1 to 30,000), and
  `[lsp.servers.<name>] start_on_edit` (false for every preset), with template lines (the uncommented template turns it
  on for ty). With `start_on_edit`, a file whose server is not up gets a wait task that starts it first, within the same
  bound; a slower start leaves the file pending and goes on. **The edit gate** (`lsp::edits::gate`, right after L2's
  `lsp::gate`): an `fs.write`, `fs.edit` or `fs.patch` in a private place whose file's server starts on edit, with none
  started for its root, is judged at `proc.run`'s posture for the server's argv, the stricter winning, once per root
  (L2's start judgment moved unchanged into `lsp::judge_start`). Health's `LspServerStatus.edit_blocks` counts the edit
  results that carried a server's diagnostics; the CLI's lsp line says "N edit results with its errors".

**How it is proven.**
- **The session's tests** (`tests_lsp_edits.rs`, the fake server in-process, whole turns through `Core` with a scripted
  provider; `lsp::edits::tests`): an edit adding 25 errors shows exactly 20 lines, "…[6 more" and "Other files: 1 new
  error" (from an open file changed on disk behind the server's back), `meta.lsp` beside the tool's own meta, then 1
  error, then "no errors", one spawn in all; no server up, no block and no spawn; a shared place's edit gets no block;
  a server that never answers costs the bound (on tokio's paused clock, at least 1,500 ms and under 1,550) and is
  pending; the block adds no frame; pending diagnostics arrive once, on the session's next result only, after 5 s
  (`a.fake (fake): 1 error`, `freshness = "pulled"`); a later edit supersedes a pending wait; an edit that starts its
  server is judged as `proc.run` (`approve` with "starts fake on" in its reason, `open` in a shared place and with the
  knob off); `start_on_edit` starts the server and health counts the block; `edit_diagnostics` off, no block;
  `edit_wait_ms` is the bound (300 ms). The frame budget and the turn bench unchanged (5 and 9). Under load, five runs
  of the LSP set: 61 of 61 each.
- **The session's planted reverts:** the block in a shared place; the wait without its bound ("waited 5s").
- **The VM's gate**, under `TZ=America/Los_Angeles`: 2,093 run, 2,060 passed, 33 failed, all the root VM's sandbox
  cases (theseus-pv6i); the kernel-sim flake passed on its retry.
- **At the review** (R1, on 7d0c9534): 147 of 147 after the join fixes (146 before; every core test with `lsp` in its
  name, the config's, theseus-lsp whole, the CLI's lsp line, the protocol, theseusd's `lsp` and `default_config`
  binaries, the frame budget, `tests_places` and `tests_policy`). Planted reverts, 4 of 4 caught: a later edit not
  dropping its pending wait (2 files arrived for 1); an edit starting its server not judged at `proc.run`'s posture
  (`Open` for `Approve`); `edit_blocks` never counting; the block not capped (26 for 20).
- **Live, at the review**, with a **real rust-analyzer** 1.98.1 (`start_on_edit`, `edit_wait_ms` 1,500) and the
  scripted stand-in model, on a scratch daemon and a two-function crate: health "lsp: none up"; a cold edit (`total(xs)`
  to `total(xs, xs)`) started rust-analyzer, ready in 3.2 s, past the bound, so the result said "pending:
  rust-analyzer had not answered for it within 1500 ms; what it reports rides on this session's next edit or lsp
  result"; the next result (`lsp.diagnostics`) carried "Diagnostics that arrived since an earlier edit: lib.rs
  (rust-analyzer): 1 error … E0107: expected 1 argument, found 2"; a warm edit's block came within the bound ("2
  errors"); the fix read "no errors"; health "rust-analyzer on …/demo (… ready in 3.2 s, 676 MB, 4 requests, 3 edit
  results with its errors)".

**The join** (08:45, by the batch-5 harvest wake, on b332bfd7). No textual conflict. R1's joinfix.py, both in the
branch's own test file: `prompt: None` on the rig's `TurnRequest` (36c's field, E0063); and `the_block_adds_no_frame`,
which failed 3 of 3 on the merged tree ("with the block 10 frames, without 9"): R1's probes showed both turns write 9
frames and the same rows when the board has settled, and the extra frame is the fake server's `lsp.ready`, written
about 300 ms after the test's `open` from the board's own readiness task, landing in the "with" window (a race on the
cloud VM too, which went the other way there); the test now waits for the store to be still (no new frame in 100 ms,
at most 5 s): 3 of 3. `CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. Its gate: 2,255 of 2,255, lifecycle strict, turn
plain p50 76.5 ms and tool-call 175.2 ms (main's level), frames 5 and 9. Pushed 08:45; theseus-n88g.9 closed.

**The install** (14:09, at bddfd407, install #1). Eddie's `/etc/theseus/theseus.toml` turned `[lsp]` on, so his edits
in a private place carry their files' errors where a server is up. Health after the restart: the config from
`/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on, lsp on, startup 73.1 ms; the store from format 6
to 14 on start.

**Eddie's calls.** At 11:00 (decision 8, "Take your recommendations. Add rust to the list of autostarted language
servers"): `start_on_edit` on for ty, TypeScript 7 (tsgo) and rust-analyzer, rust-analyzer with its own target dir
(`cargo.targetDir = true`) so its checks never take the agent's build lock (it used 4.2 GB on Theseus's own repository)
and `idle_stop_mins` (10) to free it; R1's point taken (an `lsp.diagnostics` right after an edit does not repeat the
edit's block); pending errors kept in memory. Batched for the review-smalls cloud row.

**Divergences.** rust-analyzer and "pending": the design expected a Rust edit to say pending and the next result to
carry `cargo check`'s error; as built, the wait accepts the first push current for the new version, and rust-analyzer
pushes its own diagnostics quickly, so an edit may read `pushed` without `cargo check`'s later push. The review's live
run showed the pending path on a cold start and its own E0107 within the bound when warm; whether flycheck's push ever
arrives later was not settled. `lsp.rename` gets the block too (an extra save and watched-files notice).

**Known gaps.** "Other files' new errors" are pushes only, so always 0 for pull-only servers. A written path reached
through a symlink misses an up server's canonical root and gets no block (L2's tools share it). When the session's next
result is `lsp.diagnostics` for the same file, the arrived block repeats what the tool lists (decision 8 fixes it, in
the review-smalls row). theseus-core's and theseus-lsp's guides owe their L3 lines; the LSP design lives in the worth
spike's report, not in the repository's design notes.

### Item 128. The `+rerank` arm in shadow: `rerank.v1` reorders recall's top twenty after a shadow recall, theseus-memory's pure reorder and repack, and its line in `theseus judge log` (theseus-6fn.3; roadmap row 60, step 32c; the fifth cloud batch's rerank-arm session, launched 01:41 and fired 01:45 from 3add3f53, Opus 5.5; 87559768 and c12df91e; reviewed 08:00 to 08:48 by local reviewer R2, last of its stack of six; joined 09:06 at 8f79c753, a signed merge onto 94304c3b, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** M6's design (`docs/design/m6-memory.md` §2.7) puts Jev between recall's fused order and its pack: recall's
top twenty notes that passed every filter, each asked "does this note hold information that would help answer the
message?", reordered by Jev's probability and repacked through the same filters. Step 32c builds that arm through M5's
client, in shadow, so the ledger shows what `+rerank` would have admitted beside what recall did.

**What landed** (the merge: 25 files, +1,850 −31; no new package; the store's format stays 12: a ledger row's context
only).
- **`rerank.v1`** (`packs/rerank.v1.toml`, embedded): a new point `recall`, a new builder `rerank`, a new baseline
  `fused`, action `none`, an 8,000-token cap, sample 1.0, thresholds 0.90 and 0.60. The loader caps a per-item Noul at
  ten items, so the twenty are two questions of one wording: `helps` over the first ten notes and `helps_more` over the
  next ten. The builder (`builders/rerank.rs`): the message (clipped to 4,000 characters, then to 15 % of the cap) and up
  to 20 notes `{note: n, text}` in the fused order, each scrubbed and clipped to 1,000 characters, at 80 % of the cap;
  a note the cap drops gets no Noul; a fixture and goldens.
- **theseus-memory's pure half** (`rerank.rs`, 333 lines). `eligible(science, asker, candidates, params)`: the
  candidates that pass every filter, the place rule first, in the science's order; each runs through `recall::recall`
  alone with an open budget, so recall's filters cannot drift. `reorder(fused_keys, probabilities)`: the top 20
  re-sorted by Jev's probability, ties and unanswered keys (or NaN) keeping their fused slots, the rest after in the
  fused order. `Reranked`, a `MemoryScience` whose rank is a given order, and `repack`, `recall::recall` with it, so
  the same filters and the same pack apply.
- **The core's arm** (`judge/rerank.rs`, 326 lines; one call in `turn/recall_step.rs`; `Memory::manifest_with` returns
  the manifest and the candidates its pipeline read, and `Memory::science_owned()` is new). On the turn's path,
  `JudgeService::at_recall` checks the mode (off returns at once) and the sample, computes `eligible`, and, when
  anything is eligible, mints the id, marks the trace (`point: "recall"`) and spawns. The spawned task builds the state
  from the top 20 through the core's scrubber, writes its blob and reserves from the judge's day budget on a blocking
  thread (the day's limit stops it with no row, a skip counted and `judge.paused` once, as for every shadow
  judgment), then calls Jev with **live urgency and its own 600 ms deadline**, reorders, repacks, and hands the
  judgment to the sink. The row (`judge.call`, scope `judge:rerank`, `budget: "shadow"`) holds `purpose: "recall"`,
  `arm: "+rerank"`, `baseline: "fused"`, the blob, `deadline_ms`, `on_path_ms: 0`, and `rerank: {recall, eligible,
  asked, fused_admitted, reranked_admitted, changed, order_changed, top, fallback, latency_ms, deadline_ms,
  within_deadline, cost_micros}`; `fallback` names the cause (`model_drift`, a skip reason, or a failure class) and then
  the reranked order is the fused one. The message Jev reads is recall's query (the new message, its attachments'
  names, and the first 500 characters of the reply before it).
- **The CLI**: a rerank's `theseus judge log` line replaces its 20 answers: `… rerank.v1 (shadow) <session> · recall
  rcl_… · 3 notes · changed what would be admitted (+1 −1) · $… · 341 ms of 600`, or `kept what would be admitted`, or
  `fell back to the fused order (timeout)`.
- The template's `[judge]` words and a commented `[judge.packs."rerank.v1"]`; `Ask.id` and `new_id()` as the stack's
  convention.

**How it is proven.**
- **The session's tests** (`tests_rerank.rs`, 7, against the fake Jev and a stand-in index; theseus-memory's 4, one a
  property test; three builder tests): a fake Jev reorders what would be admitted (the baseline admits the fused first
  note, `+rerank` the third, scored 0.97; `changed` true; the cost recorded as `purpose: recall` and equal to health's
  spend, the session's cost unchanged; one mark naming the row); a timeout falls back to the fused order (Jev slow 5 s:
  `timeout`, both lists equal, latency between 550 and 3,000 ms against 600); the day's limit skips a rerank (one skip,
  one `judge.paused`, no row, no connection); mode off calls nothing; a slow or failing Jev changes no turn (slow 10 s,
  down, rate-limited, malformed: the model's requests byte-identical to a judge-off core, under 3 s, every row a
  fallback); a reranked turn keeps its frame budget (at most 5); and a property test (16 cases over generated stores and
  places) that the notes Jev receives are exactly the sessions the place rule allows, and nothing when none is. Under
  load, five runs over 192 tests passed every rerank test after one race in the session's own test was fixed (the skip
  is counted under the budget's lock before `judge.paused` is appended).
- **The session's planted reverts:** Jev handed the candidates before the filters (the place test failed, shrunk to "a
  CLI asker with one session in someone else's DM"); the rerank awaited in the turn ("Slow(10s): the turn took
  5.103434536s").
- **The VM's gate**, each commit on its exact tree: the root VM's 33 L1 cases (theseus-pv6i), the UTC golden on the
  first (passing under a western zone), and one L1 self-test once (passing alone and on the rerun); the turn bench by
  hand, 5 and 9. Under load, theseus-judge's two 500 ms builder bounds failed at nice 19 (timing, not this step).
- **At the review** (R2, on its stack of 23b to 28b): 434 of 434 after join fixes 6 and 7 (429 before). Planted
  reverts, 5 of 6 caught: the unfiltered candidates; the rerank awaited; its own 600 ms deadline gone; mode off judged
  anyway; an unanswered note scored 0 (`[b, d, e, a, c]` against `[b, d, c, e, a]`). The sixth, R2's own join fix 3
  reverted (an empty label set), passed 25 of 25: no test puts a labeled note among a rerank's candidates
  (theseus-mm4a, P2).
- **Live, at the review**, on a scratch daemon of all six branches with `theseus-index` beside it, GLM (glm-5.3-flash),
  the real Jev, `[memory] mode = "shadow"` (Jev about $0.0023 for 35 calls, GLM about $0.007). The default 250 ms
  recall deadline missed first (the index did not answer in time), so the rerun used `recall_deadline_ms = 2000`. "Where
  does the grey heron nest?" after a note in another session: `rerank.v1 (shadow) · recall rcl_… · 2 notes · kept what
  would be admitted · $0.000025 · 100 ms of 600` (535 tokens in, 50 out; reserved 33 µ$, cost 25 µ$). Ten more
  questions across sessions: **11 reranks, p50 115 ms, p95 149 ms against 600** (the design expected about 350), 11 of
  11 within the deadline, no fallback, **mean 69 µ$ and at most 137 µ$ a recall** (2 to 25 notes eligible, at most 20
  asked); 4 of 11 changed what would be admitted, 5 the order. With the pack `off` and a restart: `rerank.v1: off` and
  no new row.

**The join** (09:06, by the batch-5 harvest wake, on 94304c3b). Eleven files conflicted: rerere replayed eight from R2's
tree, and R2's resolve.py resolved the other three (theseus-core's guide, the template, and recall.rs beside
memory-arm's `Memory::begin` with sources, `science_owned()` before it) and applied its join fixes, two of them
**semantic conflicts with 30b that no merge marker showed**: the rerank's own asker had none of the operator's labels
(30b's `labeled`), so a note labeled wrong or stale, which the shadow recall drops, would have reached Jev and the
repack, giving a false `changed`: `Recalled` now carries the scene's labels; and 30b's `scene()` borrows the turn,
while the branch used its parts after the row and with `&mut t.trace` (two E0502s): the rerank's place, context and
labels are taken out of the scene right after the manifest. The others: the second `new_id()`; recall's `Asker` built
without labels in theseus-memory's tests; `prompt: None`; the judge points met in tests (the rig turns every other
wired pack off; 25a's `inbound_only` turns `rerank.v1` off too); the template's words (eight packs, and the rerank's
place as it is: after a shadow recall); `BoundPlace`'s fields from `Default`. `CLOUD_TASK.md` and `CLOUD_REPORT.md`
dropped. Its gate (exit 0 at 09:05:13): 2,271 of 2,271; lifecycle OK; turn frames 5 and 9, plain p50 83.8 ms and
tool-call 206.2 ms at load 8.3, against 76.5 and 175.3 on 94304c3b at load 5.3. The branch's turn code runs only after a
shadow recall, and the bench runs memory and the judge off, so the harvest read it as load and pushed, with an A/B of
frozen builds to decide if the next gate stayed up (overnight decision 21); learning-ledger's gate at 09:48 read 77.0
and 173.7, main's level again, and no A/B was needed. Pushed 09:06; theseus-6fn.3 closed.

**The install** (14:09, at bddfd407, install #1). Eddie's config runs memory `live` on arm `baseline`, so only a canary's
control would run the shadow recall this arm follows: his turns write no rerank rows until 32d. Health after the
restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on (every pack shadow), lsp
on, startup 73.1 ms; the store from format 6 to 14 on start.

**Eddie's calls.** At 10:11 (decision 4, "Take your recommendation"), option (c): the rerank goes live, behind three
conditions in one cloud row, theseus-6fn.7 (32d, rerank-live, Item 141): theseus-mm4a's test first, the
rerank's own breaker, a bounded live rerank in recall's live path, and the ledger grading `rerank.v1`'s per-item answers
with a system label from his memory labels ("Live reinforcement learning is exciting").

**Divergences.** **Only shadow recalls are reranked**: the call sits in the shadow path (`recall_end`), so under 30b's
canary only the control's recalls are, and under `live` none; the exam's `+rerank` arm stays unwired (memory-arm
refuses `[memory] arm = "+rerank"`): 32c is the judge's half. Shadow reranks are paid from the judge's day budget, not
the session's (the live arm's session path waits for 26b). The shadow call uses live urgency, waiting up to 600 ms for
a permit instead of being shed, and its timeouts count toward the breaker every pack shares (eight then). Twenty Nouls
are two questions of ten, inside the loader's rule.

**Known gaps.** theseus-mm4a (P2: no test that a note labeled wrong or stale never reaches Jev or the repack; 32d takes it
first). With the judge on, a shadow recall clones the pipeline's candidates and builds the rerank's input before the
mode check (microseconds; checking the mode first would make the judge-off path free). A rerank at the day's limit writes
no row (`Skip` has no day-limit reason). No narrative line or notification of its own. theseus-judge's tests hold four
copies of the one-line `Ask.id` test, one per branch of the stack.

