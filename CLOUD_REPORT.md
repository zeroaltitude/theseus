# CLOUD_REPORT: fork-a, `session.fork` by reference (theseus-2qzx)

Branch `cloud/20261011-fork-a`, cut from main at 7005d130 (store format 29 on main today, not 27: standing's bump
had joined). Four commits on top of the task commit, then this one.

## fork-a: `session.fork`

### What I found

- No fork, branch or rewind method existed. A compilation admitted only its own session's nodes
  (`Store::transcript`, `compiler::situation::selected`), and `reach.rs`'s header already promised the first route that
  admits a foreign node would bring its own reverse entries.
- `Store::transcript(session)` is the one list the compiler reads, so a fork can be done in the store's read, not in
  the compiler: the transcript of a fork is the source's nodes through the fork point, then its own. `selected` (the
  prefix/tail split) needed one rule for the inherited run. Nothing else in `compiler.rs` changed beyond a field.
- Reading the source's compilation chain (`derived_from` back from the session's current compilation, at most 512
  steps) finds "the compilation that held `at`" without touching the source's nodes; that is what keeps the fork flat.
- A short node name (`msg·a1b2c3`) is resolved by `reach::resolve`, which walks the index's keys: O(N). A whole id is
  one lookup. The CLI therefore asks `node.reach` (generation 0) first, which names the node whole, and forks by that.

### What I changed (commits)

- `0c8cd3ec` core: the mechanism (protocol, store format, core, tests, TypeScript).
- `d5dad590` cli: `theseus fork`, history marks, reach lines, goldens, and theseusd's live-check test.
- `8af571de`, `aeb64101` tests only: the flatness test's p50/p95 line; a recompile and the ring over the prefix.

Design, in the order the task numbers it:

1. **`session.fork {session, at, title?, profile?, budget_usd?}`**: `theseus-protocol/src/fork.rs` (re-exported by one
   line in lib.rs, `SESSION_FORK` in the method table, routed through `rpc/sessions.rs`'s `OWN` list because `dispatch`
   is at clippy's 100 lines); logic in `theseus-core/src/fork.rs` and `rpc/fork.rs`. One frame writes: the execution
   (the source execution's authority; a pinned limit of `budget_usd`, else the daemon's), the session record
   (`forked_from`), the first compilation (`trigger = "fork"`, `includes` empty, `inherits` the run, the source's
   manifest with `strip_thinking`, `derived_from` the source's compilation in effect at the node), the fork edge
   (`derived_from`, `via = "fork"`, into that compilation), the source's reverse entry, and a `session.forked` row.
   `at` is a node (id or its end, as `node.reach` names one), or `@<WAL position>` (the last node at or before it), and
   must close its calls: a message of the person, or an answer with no tool call (anything else is refused saying so).
2. **The compile admits the foreign prefix**: `Store::transcript` (`transcript_records`), `fork::prefix_records` (a
   fork's own prefix first, so a fork of a fork works), `selected` takes the run from `Compilation.inherits`. A
   recompile, the ring and a compaction list ids as every compilation does (their compilations come from `make`, so
   `inherits` is None there). The source is never written; a turn in the fork writes the fork's records alone.
3. **Reach's reverse entries**: one `includes` edge (`EdgeKind::Includes`, from the fork's compilation into the source
   *session*, scope `in:<session>`) per fork, for the run. `reach::forks_of` reads them level by level
   (`NodeReachResult.forks`: session, compilation, generation, from, `at_position`, and the fork's own exposure of the
   node). A fork of a fork shows as generation 2 on a node of the original.
4. **The CLI**: `theseus fork <node> [--title] [--profile] [--budget] [--session]` prints the mockup's two lines.
   `theseus history` of a fork prints `── inherited from ses_…6664, by reference …`, the inherited nodes, then
   `── this session's own turns`. `theseus reach` lists the forks. Goldens: `tests/golden_fork.rs`, `golden/fork*.txt`.
5. **The proof bench** is a test (`tests_fork::a_fork_costs_the_same_at_1_000_nodes_as_at_100_000`), so it runs in the
   gate and adds no lifecycle phase; numbers below.

Hooks left for the others (written in `crate::fork`'s header and `crates/theseus-core/AGENTS.md`): R20 (file
pre-images, `--worktree`, theseus-23rk) reads `SessionRecord.forked_from` and the result's `not_restored`; R21's
surfaces (TUI `f`, Discord "Fork from here", the Ship's mark) call `session.fork` with a node and print `not_restored`.

### The store's format

**29 to 30.** Records changed, all additive and absent from the bytes when unset:
- SESSION: `forked_from` (`ForkOrigin`: session, node, position, compilation, at_ms).
- COMPILATION: `inherits` (`Inherits`: session, through_node, through_position).
- EDGE: a new kind name, `includes` (no layout change; an older build ignores an unknown kind, and `copies_of` skips it).
- LEDGER: a new kind, `session.forked`.
Old-layout samples in `tests_layouts.rs`: a session at format 29 (`SESSION_BEFORE_FORK`, with its `dir`) and a compilation
at format 29 (`COMPILATION_BEFORE_FORK`, with a standing list), both byte-exact round trips; plus
`a_format_29_store_opens_with_no_forks_and_takes_one` (the manifest moves to 30 on the first write; a fork record reads
back). The version tests (`theseusd/tests/versions.rs`, `store.rs`, the format-28 layouts test) say 30; the "newer
format refused" test uses 31. The maintainer renumbers at the merge if another row lands first.

### How I proved it

Commands (`CARGO_INCREMENTAL=0`; see "This VM" for why):
- `cargo nextest run --workspace -E 'package(theseus-core) & test(tests_fork)'`: **16 tests, 16 passed.** They cover:
  the fork's first request equals the source's messages through `at` (checked against the source's own third request),
  a fork at a person's message, the source's later nodes never appear (a "three" marker absent), nothing written to the
  source (its scope record count, its session record bytes, its execution bytes before and after two turns in the
  fork), one frame (`frames_written_here` +1) with `includes` empty and `inherits` set, the fork edge, reach on a source
  node (fork listed, its compilation and one loop; a later node not reached; the fork's own node reaches none), a fork
  of a fork (generations 1 and 2, prefix of 8 nodes: 4 + 2 + 2), a restart (a second core on the same store: the fork
  compiles, still one compilation, reach still lists it), the fork point by id, by `msg·` name, by `@position` and
  refused for an unknown node, another session's node, and a non-closing node; title, budget (pinned limit), profile
  refused when unknown; refusals (Discord and unnamed surfaces on a private source, MCP on a non-MCP source, an imported
  session) and the MCP fork under the source's authority and its external-text hold; a shared place's session forked
  from a Discord caller (fork is private, bound to no place, nothing posted to the shared place); a fork's history
  paged across the join (newest page, a page back, a page forward); a recompile in the fork (ids listed, `inherits`
  None); the ring over prefix and own turns on a 40,000-token window.
- Flatness (same test file): 40 forks each at 1,000 and at 100,000 nodes (debug build). **Records read per fork: 4 and
  4. Bytes written: 3,193 and 3,199. p50 2.0 ms and 2.7 ms; p95 3.6 ms and 4.1 ms.** Under the load recipe
  (`nice -n 19` beside four busy loops, three runs): p50 2.1 to 2.7 ms, p95 3.2 to 4.2 ms at 1,000 and 100,000, once
  5.9 ms at 100,000 (debug build, loaded); all three runs passed (the test bounds the p50 ratio at 2x plus 2 ms, and
  reads and bytes exactly). Before I used a whole id, a short name cost 4.3 ms and 43 ms (the key walk, above): that run
  failed the test as it should.
- Planted reverts (file restored and `touch`ed after each; `git status` clean):
  1. *Copy instead of reference* (`includes` = the prefix's ids, `inherits` None): failed
     `a_fork_is_one_frame_of_a_few_hundred_bytes_by_reference`, `reach_on_a_source_node_lists_the_fork`,
     `a_fork_of_a_fork_works_and_reach_lists_both_generations`, `a_restart_keeps_the_fork_and_it_still_compiles`, and the
     flatness test (14.8 s instead of 7 s: the copy reads the whole prefix).
  2. *No reverse entry* (the `includes` edge dropped from the frame): failed `reach_on_a_source_node_lists_the_fork`,
     `a_fork_of_a_fork_works_and_reach_lists_both_generations`, `a_restart_keeps_the_fork_and_it_still_compiles`.
  3. *A source node leaking past `at`* (no position bound in `own_through`): failed
     `a_fork_compiles_the_prefix_exactly_and_never_the_sources_later_nodes`,
     `a_fork_of_a_fork_works_and_reach_lists_both_generations`,
     `a_forks_history_is_the_prefix_then_its_own_turns_and_pages_across_the_join`.
- CLI goldens: `cargo nextest run -E 'package(theseus) & binary(golden_fork)'`: 3 tests, three runs, stable. (The
  history golden answers the `session.wait` the CLI makes to record what it showed; without it the daemon thread
  closed first and the CLI said `Broken pipe`.)
- Live check on a real daemon (the stand-in model, the real CLI spawning the real `theseusd --stdio`):
  `cargo nextest run -E 'package(theseusd) & binary(fork)' --no-capture` passes. Its output:
  ```
  $ theseus fork msg·ce25b6 --title "the other road"
  forked ses_…c5fe at msg·ce25b6 (position 71) → ses_…f5f5 "the other road" in 2 ms, 3.1 KB
  not restored: files and what programs did after the fork point (they ran in the original)
  $ theseus reach msg·0c6abb
  msg_01a1…c6abb: seen by 6 contexts in 2 sessions · …
    generation 0 · ses_…c5fe · msg_… at 18: 1 compilation, 3 loops · …
    generation 1 · ses_…f5f5 · forked from ses_…c5fe at position 71 by reference (compilation cmp_…): 1 compilation, 1 loop
  ```
  and the fork's history prints the inherited turns one and two, then its own; the fork's request was 5 messages (turn
  one, its answer, turn two, its answer, the new message), nothing of turn three.
- Whole suite: `cargo nextest run --workspace`, 4,242 tests, 4,208 passed, 34 failed, all in the lists below.

### Live check for the maintainer

The test above is the check, on the stand-in: `cargo nextest run -E 'package(theseusd) & binary(fork)' --no-capture`
(about a second; it prints the three CLI outputs). By hand, on a scratch daemon of the install build:
```
theseusd example-config > /tmp/fork.toml   # edit: [model] and each [providers.*] api_base to the stand-in,
                                           # every [secrets] entry to env:FORK_KEY, [discord] [web] [index] [judge] off
theseus-sim fake-model --addr 127.0.0.1:9591 --rules rules.json &    # rules.json: a rule that answers any text
theseusd --config /tmp/fork.toml --socket /tmp/fork.sock --state-dir /tmp/fork-state &
theseus --socket /tmp/fork.sock ask "turn one"      # note the session; then: ask -s <session> "turn two" / "turn three"
theseus --socket /tmp/fork.sock history <session>   # the six nodes as msg·xxxxxx
theseus --socket /tmp/fork.sock fork msg·<turn two's answer> --title "the other road"
#   -> forked ses_…xxxx at msg·… (position N) → ses_…yyyy "the other road" in a few ms, ~3 KB / not restored: …
theseus --socket /tmp/fork.sock ask -s <fork> "the other road, please"   # a turn in the fork (--dump on the stand-in
#   shows 5 messages in its request, none of turn three)
theseus --socket /tmp/fork.sock history <fork>      # "── inherited from ses_…xxxx", turns one and two, "── this session's own turns"
theseus --socket /tmp/fork.sock history <session>   # all three turns, no inherited mark, nothing of the fork
theseus --socket /tmp/fork.sock reach msg·<turn one> # a "generation 1 · <fork> · forked from … by reference" line
theseus --socket /tmp/fork.sock shutdown
```

### What is left or uncertain, and design choices the owner should hear about

- **Bytes: 3.2 KB, not 2 KB** (3.1 KB on a real daemon). Breakdown: the execution 0.5 KB and its `execution.opened` row,
  the session record 0.7 KB, the compilation 1.3 KB (0.84 KB of it the copied manifest: its `tools` list is 0.4 KB), two
  edges, the `session.forked` row. The target needs a smaller manifest (drop `tools` and `cache`, which nothing reads
  back) and a thinner session record; I kept the manifest faithful. The bytes are flat in N, which is the property.
  Time p95 is under the 5 ms target in a debug build, except one loaded run.
- **Budget is a pinned limit, not a carve.** A task's carve is taken from its parent's execution, which would write to
  the source; the spec's "the source is never written" wins. The fork's limit is `budget_usd` (pinned) else the
  daemon's, and its spend is its own.
- **Place**: a fork speaks in no place (the CLI's, private). The task says "the source's place when private"; binding the
  fork into the source's DM or channel would supersede the source there (`bind_place_to`), which is R21's decision for
  Discord. A fork of a shared place's session never reaches the shared place (tested).
- **Surfaces**: Discord and unnamed connections may fork only a shared place's session; the MCP surface forks only a
  conversation it opened (`mcp_server::allowed` admits `session.fork`, `Core::fork_allowed` holds the rest). R21's
  "Fork from here" in a private DM will need a decision there (the Discord binding reads no private text today).
- **`at` kinds**: a tool call or a tool result is refused (their calls would be left without results); fork at the
  message of the person or the answer that made no call. A person's message as the point leaves the new turn's input
  beside it (two user messages in a row, which the renderer already handles).
- **Kind**: a fork is always a Conversation; forking a task session gives a conversation with the task's authority.
- **Thinking** of the inherited nodes is stripped in the fork's first compilation (it was thought in another context).
- **File reads by name** (`file.read`'s lookup among the session's nodes) read the session's own nodes: an attachment
  from the inherited prefix is not found by name in the fork. Not addressed.
- **A short name costs a key walk**, as `node.reach` does; a whole id is a lookup. `session.fork` accepts either.

Docs for the maintainer to change (I touched none of the spec, status, README or design docs): the spec's Part III
(a new item for this row: store format 30, the records above, the divergences above) and the version line in
`docs/spec/01-front.md`; `docs/status.md` (Updated, the landed step, R19's row); `docs/design/roadmap-v2.md`'s R19
row; the killer-features plan §4.6. I updated `crates/theseus-core/AGENTS.md`, `crates/theseus-protocol/AGENTS.md` and
`crates/theseus/AGENTS.md`, and the store's format list (`theseus-store/src/store.rs`, 30).

**Keel findings expected:** none. `python3 scripts/keel-guard.py --base HEAD` printed `keel: ok … 0 findings` on my
diff before the first commit, and the gate's keel phase (base set as below) said `ok, 0 findings` on each gate run. No test was deleted, ignored or loosened, no `#[allow]` or `#[expect]` added (two functions
were split instead), no ceiling or cap changed (`compiler.rs` ended at 2,559 of 2,560 lines; `protocol/lib.rs` at
2,627).

### This VM

- The disk allowance filled at 27 GB of debug target (jobs then refuse to start under the state dir's 1,024 MB floor,
  which failed about 120 tests until I freed space). I deleted `target/debug/incremental` and built with
  `CARGO_INCREMENTAL=0` after that.
- The gate's keel phase, run plain here, fails on six findings that are not this branch's: the clone's `origin/main`
  ref is a9ad950f, so its range (merge-base..tree) holds main's later commits (a test's assertion removed in
  theseus-tui, two `allow`s, `daily_spend_ceiling_mode`, two long-files ceilings, and acks whose signatures do not
  verify here). I ran the gate with `THESEUS_KEEL_BASE=c69ef700` (the task commit), which judges this branch alone.

## The gate

`THESEUS_KEEL_BASE=c69ef700 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the tree of `aeb64101`:
door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build and the reader rule passed; **the suite failed
only on the known and the pre-existing failures (4,242 run, 4,208 passed, 34 failed):**

- 33 L1 tests, known (theseus-pv6i: root, no job cgroup): theseus-sandbox's `contract` (19 tests) and `bench
  spawn_100`, and theseusd's `sandbox` (13 tests).
- 1 not on the task's list: `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
  (a 20 s timeout waiting for the reply's post in the fake channel). It fails the same way at the base commit: I stashed
  my work, ran it on c69ef700 and it failed in 21 s; five runs on my branch failed the same, and it passes with
  `--no-capture`. Not this row's; I did not look further.

Run by hand after the suite (the gate's later phases): the generated TypeScript is added and unchanged (`git diff` of
`cockpit/src/protocol.gen` empty after `git add`); `target/debug/theseus-sim bench turn --check --runs 5 --burst 0`
passed (frames and syncs within budget); `cargo deny --offline --log-level error check`: advisories, bans, licenses,
sources ok. The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH=1`; this row adds nothing to the
start path.

One more failure seen once outside the final gate: `theseusd::gone_jobs
a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` ("the live wrapper's job runs on": 3 jobs marked
unknown, 1 expected) failed in an earlier gate run and once in three runs alone, then passed six runs in a row and in the
final gate; it concerns wrappers' liveness, not forks. I did not run it at the base.

An earlier gate on the same code stopped at `fmt` (my new test files were unformatted); `cargo fmt` fixed it.
