<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-pattern-miner`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: pattern-miner, the nightly miner that finds recurring tool-call work in the ledger and the bench and writes tool skeletons with no model (theseus-pw1q.21.3)

Branch: `cloud/20261017-pattern-miner`. Every commit's subject carries `theseus-pw1q.21.3`. Deadline for the report: 5
hours after you start.

**Source:** the RSI loop-1 design §2 (copied below, complete). **Wave A; joins after R1, before R2** (contract 3: you own
`[self.mint]`, and your one field line in `config/self_improve.rs` must land before R2 makes that file a keel file).
Prerequisite: `main`. Size L. R7 (mint-validate) builds on your types after your join: keep `Motif`, `Shape`,
`SlotKind`, `Occurrence` and the skeleton spec public, documented and stable (contract 7).

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Size |
|---|---|---|---|---|---|---|
| R5 | `pattern-miner` | theseus-pw1q.21.3 | `rsi/mine/`: incremental node read, shapes, slot kinds, result features, stretches, closed n-grams, alignment, branches, support, cost; the bench ATIF reader; pattern causes and their score in the backlog; `theseus self patterns` (read-only, owner-only, works while the mode is off); all of `[self.mint]` | `rsi/mine/{mod,read,shape,slots,motif,align,cost,bench}.rs` (new), `rsi/backlog/{read,rank}.rs`, `config/self_mint.rs` (new) + one field line in `config/self_improve.rs`, the template, the CLI's `self` subcommand, `crates/theseus-protocol/src/patterns.rs` (new) + one `pub mod` line, `fact/rsi_mine.rs` (new) | goldens for normalization; a synthetic ledger with planted motifs found with the right branches (clean, rerere, hand-back); thresholds; cost attribution; a planted secret never reaches a stored shape; **live**: a read-only run on a copy of a real store, its top 10 in the report | L |

## What exists (at `c7ad21f7`)
- Transcript: NODE `Body::ToolCall {tool, input, assistant_node, correlation_id, gate}` and `Body::ToolResult {status,
  is_error, duration_ms, meta, content}` (`crates/theseus-store/src/node.rs:180-240`), paged by body kind across
  sessions (`k:tool_call`, `crates/theseus-store/src/pages.rs:69-136`). The assistant node's `usage`, `cost_usd`,
  `model` (`node.rs:152-178`); `provider.call`'s `timing` (`crates/theseus-core/src/fact/turn.rs:1024`).
- Place and repository: `SessionRecord.dir` and `.project` (`crates/theseus-store/src/session.rs:106-126`); the call's
  `policy_context.cwd` in its gate record (`crates/theseus-core/src/toolrun.rs:952`). Tool input is **not scrubbed at
  rest** (`toolrun.rs:961-981`).
- Bench trials: Harbor's `agent/trajectory.json` in ATIF-v1.7, written by `bench/harbor/theseus_atif.py:32-231`
  (`tool_calls[{function_name, arguments}]`, `metrics{prompt_tokens, completion_tokens, cost_usd}`, timestamps,
  observation `duration_ms`/`status`), and `theseus-history.json`. Read that file to get the exact field names; write a
  small fixture trajectory for tests (invented content).
- Tool use: `tools.used` / `tools.searched` / `tools.missed` (`crates/theseus-core/src/fact/tools_defer.rs:14-98`).
- The backlog: `rsi/backlog/read.rs:31,310` (`Obs` → `Candidate` per cause key), `rank.rs:31` (the milli-point scale),
  `MAX_ROWS`, `backlog/start.rs`. The self gate: `rsi::gate` (`rsi/mod.rs:114`).
- The keyed split used by self-exams: `rsi/exams/pass.rs:363`. The scrubber: `core.tools.scrubber`.

## What to build
1. Everything in §2.1-§2.6 below, inside `rsi/mine/`, with **no model call anywhere in the miner**.
2. **Incremental and FAST:** a cursor in META, a per-day compact shape list (`self.mine.shapes.<date>`, token ids only),
   a 14-day window as the union, a row cap like backlog's `MAX_ROWS`, background priority, nothing on the start path.
   Show the night's cost on a synthetic 14-day ledger of realistic size (say 20,000 calls) in a test or a bench note:
   seconds of CPU and peak memory.
3. **Hygiene:** raw values only in memory; stored shapes hold token ids, slot kinds and typed placeholders; constants
   kept only if they pass the scrubber and the shape check (no absolute path outside `$REPO`, no person-like string, no
   secret shape); occurrence evidence as node/ledger positions, never values. **Never mined** list (contract 9) cuts
   stretches.
4. **Scope key:** place class plus repository (git top level of the cwd, keyed by a hash of its canonical path and its
   origin URL's path); bench scope `bench:<suite>`, never mixed with a private repository's key; bench-born motifs must
   be generic (§2.3 step 5).
5. **Output:** for each surviving motif, a `Motif` (shapes, core/optional steps, branches with arms and hand-back exits,
   slot kinds, support, cost per occurrence, projected saving) and its **skeleton spec** (§2.5's JSON shape: steps with
   bindings, typed inputs, branches, exits, evidence; no name or description yet: those are R7's model step). The
   skeleton is data only: it is not frozen, deployed or offered to any model in this row.
6. **Pattern causes in the backlog:** one `Obs` source and the §2.6 score beside `rank.rs:31` (contract 4), filed under
   a `pattern:<scope>:<digest>` cause key; fix causes rank exactly as before (a test pins a mixed list's order).
   Nothing is started from a pattern cause in this row (R7 adds the mint start), and with `[self.mint] mine = false`
   (the default) the miner doesn't run at all.
7. **`theseus self patterns`**: owner-only, read-only, works while `[self] mode = "off"` (it runs the miner on demand
   over the window and prints the top N with support, scope, cost and projected saving; `--json` for the wire type).
   It writes nothing, not even the cursor. Fit it into the CLI's existing `self` group in the trim-names style
   (noun then verb), with its help line and its `docs/cli.md` entry if that file lists `self` commands.
8. **`[self.mint]`** in `config/self_mint.rs` (contract 3): every key of §6.1's `[self.mint]` block with its default and a
   doc line; `mine = false` by default; the template block; the template tests. Validation: thresholds at least 1,
   `bench_dirs` absolute or `~`-relative.
9. **Ledger:** `self.mine.ran {window, calls_read, motifs, top: [digest, support, saving]}` in `fact/rsi_mine.rs`, one row
   a night, no values.

## Tests (each with a planted revert)
- Normalization goldens (`proc.run` argv with git/cargo/npm/gh subcommands, flags as sorted sets, `$PATH` classes,
  `$REF`, `$NUM`, `$URLHOST`, `$STR`; `steps` arrays; `file.read`, `file.edit` AUTHORED) and result-feature extractors
  (`conflict`, `rerere-resolved`, `rerere-left`, `test-fail`, `nothing-to-commit`, `non-ff`, `auth`). Revert: drop a
  subcommand word.
- A **synthetic ledger** (invented repo and session ids) with the owner's example planted: sync stretch (pull; test
  baseline) and land stretch (test; merge; rerere on a conflict; resolve-or-hand-back; test; push) across 6 sessions
  and 4 days, plus noise. The miner finds both motifs, with the arms `merge→ok→push`,
  `merge→conflict→rerere-resolved→commit→push`, and `merge→conflict→rerere-left→handback`. Revert: count support per
  occurrence instead of per execution (a loop inside one session must not inflate it).
- Thresholds: 4 occurrences or 1 day is not a motif; the closed-pattern rule drops a sub-pattern with equal support.
- Cost attribution: tokens (uncached, cache read, cache write, output kept apart), dollars and model seconds per
  occurrence from the assistant nodes that emitted the calls.
- `a_planted_secret_never_reaches_a_stored_shape` (an `AKIA…`-shaped and a `sk-…`-shaped value in a recorded call's
  argv; grep every stored META/ledger byte). Revert: skip the scrubber.
- Bench reader: a fixture ATIF trajectory gives the same shapes; a bench-born motif with a task path constant is
  rejected as not generic.
- Off: with `mine = false` or `[self] mode = "off"` or the kill switch halted, the nightly entry reads nothing (except
  `theseus self patterns`, which reads and writes nothing).

## Live (in the report)
If this VM can't reach a real store, give the maintainer the exact command: a read-only `theseus self patterns --json
--top 10` against a **copy** of the owner's store (never the live one), and what it should show. Report shapes and
numbers only, never values.

## The design's own words: §2. The pattern miner

### 2.1 What it reads

| Source | Records | What it takes |
|---|---|---|
| Transcript | NODE `Body::ToolCall {tool, input, assistant_node, correlation_id, gate}` and `Body::ToolResult {status, is_error, duration_ms, meta, content}` (`node.rs:180-240`), paged by body kind across sessions | The call's tool and **verbatim input**. Input is *not* scrubbed at rest (`toolrun.rs:961-981`), so the miner holds raw values only in memory and stores shapes, never values (§2.5). Also the result's class |
| Model cost | The assistant node's `usage`, `cost_usd`, `model`; `provider.call`'s `timing` (`fact/turn.rs:1024`) | Tokens, dollars and seconds per model loop, attributed to the calls that loop emitted |
| Place and repository | `SessionRecord.dir` and `.project` (`session.rs:106-126`); the call's `policy_context.cwd` in its gate record (`toolrun.rs:952`) | The **scope key**: the place class (DM, CLI, task, channel) and the repository (git top level of the cwd, keyed by a hash of its canonical path and its origin URL's path) |
| Bench trials | Harbor's `agent/trajectory.json` (ATIF-v1.7: `tool_calls[{function_name, arguments}]`, `metrics{prompt_tokens, completion_tokens, cost_usd}`, timestamps, observation `duration_ms`/`status`; `bench/harbor/theseus_atif.py:32-231`) and `theseus-history.json` | The same normalized shapes, scope `bench:<suite>`, never mixed with a private repository's key |
| Tool use | `tools.used` / `tools.searched` / `tools.missed` (`fact/tools_defer.rs:14-98`) | Whether the model was offered, found and used a minted tool |

**Reading is incremental.** The night reads only nodes after its cursor and folds each session's calls into a compact
per-day shape list (a META record per day: `self.mine.shapes.<date>`, token ids only). The 14-day window is the union of
those lists. A night then costs a few seconds of CPU at background priority, off the start path, with a row cap like
backlog's (`MAX_ROWS`). FAST: nothing about it grows with history.

### 2.2 Normalization: each call becomes a shape token

A token is `(tool, argument shape, result class)`. Arguments become **typed slots**:

| Call | Shape |
|---|---|
| `proc.run {argv:["git","pull","--ff-only"]}` | `proc.run[git pull --ff-only]` |
| `proc.run {argv:["cargo","nextest","run","-p","theseusd"]}` | `proc.run[cargo nextest run -p $CRATE]` |
| `proc.run {argv:["git","merge","--no-ff","self/x"]}` | `proc.run[git merge --no-ff $REF]` |
| `file.read {path:"crates/a/src/b.rs"}` | `file.read[$PATH:crates/*/src/**.rs]` |
| `file.edit {path, old, new}` | `file.edit[$PATH:rs] AUTHORED` |

- **Programs** keep their name and up to two subcommand words for known multi-command programs (git, cargo, npm, gh,
  docker, bd, kubectl and so on). Flags keep their names, as a sorted set. Values become `$PATH` (with its extension
  class and top directory), `$REF`, `$NUM`, `$URLHOST` or `$STR`. `proc.run`'s `steps` arrays become one token per step.
- **Result classes** are ok, or an error with its exit-code class, plus features found by small extractors over the
  result text:

  | Feature | Matches |
  |---|---|
  | `conflict` | `CONFLICT`, "Automatic merge failed" |
  | `rerere-resolved` | "Resolved … using previous resolution" |
  | `rerere-left` | a conflict rerere did not resolve |
  | `test-fail` | nextest `FAIL`, "test result: FAILED" |
  | `nothing-to-commit`, `non-ff`, `auth` | the matching git messages |

  The features are what the branches split on.
- **Slots are classified across occurrences:**

  | Kind | What it is |
  |---|---|
  | CONSTANT | Identical in at least 80% of occurrences |
  | BOUND | Equals an earlier slot, or a value read from an earlier result: the repository root, the branch from `git rev-parse --abbrev-ref HEAD`, a file named in a conflict list |
  | INPUT | Varies, isn't bound, and is short and typed: a ref, a path, a number, a short string. It becomes a tool input |
  | AUTHORED | Long free text the model wrote: an edit's body, a file's content, a long message |

### 2.3 Finding recurring sequences

1. **Segment.** Each execution's call list is split into *stretches* at AUTHORED calls. The creative work (writing the
   code) stays with the model. Tools encapsulate the deterministic scaffolding around it. The owner's example becomes
   two stretches with the model's work between them: `sync` (pull; test baseline) and `land` (test; merge; rerere on a
   conflict; resolve-or-hand-back; test; push).
2. **Count.** Every contiguous n-gram of 3 to 16 tokens in each stretch is hashed, counting result classes as wildcards
   at first. Each is counted once per execution, so a loop inside one session doesn't inflate support. Only **closed**
   patterns are kept: a sub-pattern with the same support as its extension is dropped. The cost is about 16 × the calls
   in the window, which is trivial.
3. **Align.** Each occurrence of a surviving pattern is aligned to its medoid (Needleman–Wunsch over tokens, with gaps).
   Steps present in at least 60% of occurrences are core. The rest are optional.
4. **Branch.** At each core step the occurrences are split by result class. The next-token distribution per class gives
   the arms, for example:
   - `merge→ok→push`
   - `merge→conflict→rerere-resolved→commit→push`
   - `merge→conflict→rerere-left→AUTHORED…`

   An arm that ends in AUTHORED work becomes a **hand-back exit**. The tool stops and returns structured state ("2 files
   in conflict, rerere resolved 1: `a.rs`; left: `b.rs`, hunks 3"). The model decides only what needs judgment.
5. **Support.** The thresholds are configurable (§6):
   - **The real ledger:** at least 5 occurrences in at least 3 distinct executions on at least 2 distinct days, in one
     scope.
   - **The bench:** at least 4 trials across at least 3 tasks.
   - **Bench-born motifs** must be generic: their constants are only program, subcommand and flag tokens, with no task
     paths or strings. Their replays use held-out *tasks* (§3.1). They are labelled "bench-born" in the notice, because
     a tool that helps only a benchmark is a loss ("improve, never max").
6. **Never mined.** Calls that face outward or spend: message, email, Discord posts, AWS writes, hands, web posts,
   `extend.*`, `self.*`, approvals and memory writes. A stretch containing one is cut there. A minted tool is never one
   that speaks for the owner or spends.

### 2.4 Cost per occurrence, and the saving

For an occurrence of k calls emitted by m model loops:
- **Tokens.** Each loop's usage is uncached input, cache reads, cache writes and output (`AssistantMessage.usage`). Each
  is kept as raw tokens and dollars (`cost_usd`), never collapsed into one number.
- **Seconds.** The loops' model time (`provider.call` timing) plus the calls' `duration_ms`.
- **Decisions.** m, the model choice points. A tool replaces m loops with 1, plus one per hand-back taken.
- **The saving** per occurrence is the cost of the loops removed: tokens and dollars, and their model seconds. Tool time
  stays, because the flow runs the same commands. The projection is the saving per occurrence × occurrences per day in
  the scope.

### 2.5 From motif to tool spec

The **miner writes the skeleton deterministically**:
- the steps, with their slot bindings;
- the inputs (the INPUT slots, typed);
- the branches and exits;
- the evidence: occurrence ids as ledger or node positions, never values;
- the cost numbers.

A model is used only after that, in a `"self"` session. Sonnet codes, per the model-roles rule. The model:
- names the tool and writes its description (lever 3: the description states the contract, never persuasion);
- settles ambiguous slot types;
- for MCP or Rust, writes the code.

A flow's spec is therefore mostly **mined, not authored**. That is the determinism the owner asked for.

```json
{ "v": 1, "kind": "flow", "name": "repo_land",
  "scope": { "repos": ["r:5f1c…"], "places": ["cli", "task"] },
  "inputs": { "branch": {"type": "ref"}, "test": {"type": "argv", "default": ["cargo","nextest","run"]} },
  "steps": [
    {"id": "test",  "tool": "proc.run", "args": {"argv": "$test"},                       "on": {"err": "exit:tests-red"}},
    {"id": "merge", "tool": "proc.run", "args": {"argv": ["git","merge","--no-ff","$branch"]},
     "on": {"conflict": "goto:rerere"}},
    {"id": "rerere","tool": "proc.run", "args": {"argv": ["git","rerere","status"]},
     "on": {"rerere-left": "handback:conflicts-left", "ok": "goto:commit"}},
    {"id": "commit","tool": "proc.run", "args": {"argv": ["git","commit","--no-edit"]}},
    {"id": "retest","tool": "proc.run", "args": {"argv": "$test"},                       "on": {"err": "exit:tests-red"}},
    {"id": "push",  "tool": "proc.run", "args": {"argv": ["git","push"]}}
  ],
  "exits": { "ok": "landed $branch", "handback:conflicts-left": {"state": ["conflict_files", "rerere_resolved"]} },
  "evidence": { "occurrences": 11, "sessions": 5, "days": 6, "tokens_per_occ": 41200, "usd_per_occ": 0.21,
                "secs_per_occ": 63, "decisions_per_occ": 7 },
  "digest": "sha256:…" }
```

**Hygiene.**
- Constants survive only if they pass the scrubber (`core.tools.scrubber`) and a shape check: no absolute paths outside
  `$REPO`, no person-like strings, no secret shapes. Otherwise the slot becomes an INPUT.
- The spec is frozen by SHA-256, like an extension (`extend/freeze.rs`), at `<state>/self/mint/<name>/<digest>/`,
  read-only.
- **Data rule.** A motif drawn from private ledger sessions is written and built only on Claude models, failing closed.
  A motif drawn only from bench trials may use any model.

**Tests come from the recorded instances.** The occurrences are split by a keyed hash, as self-exams splits its items
(`exams/pass.rs:363`), into a **build half** the writer sees and a **check half** it never sees. Code guarantees this:
the brief is assembled from build-half ids only, and a test asserts on the brief's bytes. Three tiers:
1. **Fixtures**, one per branch arm. The builder makes them, for example a temp git repository with an upstream and:
   - a clean merge;
   - a conflict rerere knows;
   - a conflict it doesn't, which must hand back;
   - red tests, which must stop with `tests-red`.

   They become the extension's declared tests (up to 20, `extend/mod.rs` `MAX_TESTS`), the flow's test table, or Rust
   tests.
2. **The dry replay**, with no model. For each check-half occurrence, the directory is rebuilt as it was at the
   stretch's start. The whole-turn replay already does this: `own_bench/whole/scratch.rs`, and `undo.rs` puts back the
   recorded edits. The tool is run on that copy with the recorded inputs bound, and its facts at the end are compared
   with the recorded facts: exit codes, paths changed, final test result. Remote steps (pull, push) are stubbed with
   their recorded results, as `hands.rs` stubs tools today. Their behaviour is proved by the fixtures.
3. **The model A/B replay**, at most `[self.mint] replay_turns` check-half turns. These are whole-turn replays
   (`own_bench/whole/runloop.rs`), with arm A the recorded tool set and arm B the same plus the minted tool loaded. Each
   arm is capped by the existing `whole_loops` (24), `whole_seconds` (300) and `whole_usd` (0.50) limits
   (`config/route.rs:10`), reserved on the self budget.

### 2.6 Ranking a pattern beside the fixes

The score uses the same milli-point scale as `rank.rs:31`, so one ordered list works:

```text
1000 × ln(1 + occurrences_14d) × (1 + usd_saved_14d/2 + secs_saved_14d/600 + decisions_removed_per_occ/2)
     × (0.5 + recency) ÷ work           work: flow 1, mcp 2, rust 3
```

Fix and mint starts have separate caps (`backlog_starts`, `[self.mint] per_night`), so a night of rich patterns never
starves fixes. The reverse holds too.

### 2.7 How the model is offered a minted tool

- **Deferred by default**, as every MCP family is today (`mcp:ext-<name>`, `tools_defer/family.rs:231-233`). Flows are
  one family per flow (`flow:<name>`), deferred the same way. A deferred definition costs a constant 93 tokens, and
  tool search finds it.
- **Offered only in scope**: places whose roots hold the repository it was mined from, or everywhere for a generic
  bench-born tool.
- **Learned loading** is the existing rule: at least 10 turns in the window and use at or above `load_share` (0.10)
  (`tools_defer/select.rs:150-196`). `fit.rs` demotes learned families that would push the first request past its token
  budget (8k CLI/task, 15k DM; `first_request.rs:24-30`). The owner's "about 1 in 5 sessions" bar for the
  ALWAYS_LOADED table stays for built-ins. A minted tool never enters that table on its own.
- **Lever 4, measured first.** When a call in scope matches the first two tokens of a deployed tool's motif, the result's
  metadata could carry one factual line: "`flow_repo_land` runs the rest of this sequence (6 steps) in one call". This
  is lever 4 (result metadata), never a system-prompt sentence. It ships **off** (`[self.mint] hint = false`) until a
  bench arm (none / hint) shows it earns its place, per the minimal-prompting rule.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `40aa83cd` (2026-10-17
22:21 MST, after the trim joins) or later, with **store format 45**. The design this row comes from read `c7ad21f7`, one
merge earlier, so every `file:line` below is at `c7ad21f7`; on your base a line may have moved a little: search by name.
The `[self]` config is `crates/theseus-core/src/config/self_improve.rs` (the trim joins renamed the Rust side to
`self_improve`; the TOML table is still `[self]`).

Line ceilings at `40aa83cd` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` **2,729 /
2,733**, `crates/theseus-kernel/src/kernel.rs` **3,026 / 3,030**, `crates/theseus-core/src/turn.rs` 3,354 / 3,525. Never
raise a ceiling in this row. Put new code in new modules; in a long file add only the `mod` line, the field or the call.

## RSI loop 1: the eight rows

The owner's decisions (Discord DM, verbatim):
- 2026-10-17 22:43:21 MST: "this is sort of like a dreaming system, but less around organizing context ... and more
  about fixing yourself and making new tools. I like the MCP, but I also think that new rust tools would be good too --
  of course, that would require the nightly build-in mode, and not on the fly rebuilding of the theseus binaries. ...
  the first switched on loop should be the fix mining ... and pattern mining for new tools and other fixes. act and tell
  you after, as usual. I like the sealed scorer, let's build it and use it where appropriate because self fixing should
  still be in the first iteration".
- 2026-10-17 23:34:35 MST: "RSI loop 1: (1) yes; (2) yes; (3) lets build and switch it on; lets move benchmarks to right
  before the v1 cut, like the fat linker build." That answers: (1) **flows** as a third kind of minted tool; (2) **one
  Keel yes** covering the sealed-scorer work (the keel guard watching the sealed and exam paths, self builds inside L1,
  the scratch-daemon Runner and model relay, the sealed check in self-join); (3) build all eight rows, then switch loop 1
  on.

One nightly run, after consolidation: **fix mining** (the existing backlog, `rsi/backlog/read.rs`) and **pattern mining**
(new: recurring tool-call sequences per repository and place, plus bench trajectories) feed one ranked backlog. Fixes go
through self-pr, a gate in L1, the planted revert, independent review, the keel check and, for behaviour paths, the
**sealed hold**. Patterns become **minted tools** of the cheapest kind that works: a **flow** (a frozen JSON spec over
existing tools, run by one built-in, live the same night), an **MCP extension** (read-only computation in L1, live the
same night), or a **Rust built-in** (self-pr, nightly build and install). Minted tools are checked by **open replays** of
held-out recorded instances. One morning DM lists what landed, each line with its undo. Everything ships **off** by
default: `[self] mode = "off"` stays the default, and nothing here acts until the owner switches it on.

| Row | Slug | Beads | Wave | Depends on | Area |
|---|---|---|---|---|---|
| R1 | `l1-self-build` | theseus-pw1q.21.1 | A | none | kernel L1 `rw` binds, theseus-sandbox, core sandbox.rs, rsi/tree.rs, rsi/join/{planted,install}.rs, rsi/revert/doing.rs, rsi/pr.rs |
| R2 | `keel-watch-sealed` | theseus-pw1q.21.2 | A (joins last of wave A, before R3) | none | scripts/keel-guard.py, scripts/test_keel_guard.py, rsi/sealed.rs `KEEL_PATHS`, rsi/sealed/tests.rs |
| R5 | `pattern-miner` | theseus-pw1q.21.3 | A | none | rsi/mine/ (new), rsi/backlog/{read,rank}.rs (pattern causes), the CLI's `self patterns`, its protocol module |
| R6 | `flow-runner` | theseus-pw1q.21.4 | A | none | theseus-core/src/flow/ (new), toolrun.rs (registration only), tools_defer/family.rs, fact/flow.rs (new), rsi/revert.rs (one verb arm), its protocol module |
| R3 | `sealed-runner` | theseus-pw1q.21.5 | B (built beside wave A; joins after R1 and R2) | R1 (overlap: kernel L1 types), R2 (keel trailer) | rsi/sealed/{runner,relay}.rs (new), rsi/sealed.rs, config/self_improve.rs (`[self.sealed]`), theseus-exam/src/daemon.rs, theseus-kernel/src/job_egress* |
| R7 | `mint-validate` | theseus-pw1q.21.6 | B (staged) | R5, R6 joined | rsi/mint/{mod,spec,brief,kind,validate}.rs (new), own_bench/whole/{runloop,hands}.rs |
| R4 | `sealed-hold` | theseus-pw1q.21.7 | C (staged) | R3 joined | rsi/sealed/{daemon,hold}.rs, rsi/join.rs, rsi/join/reviewing.rs, fact/rsi_sealed.rs, rsi/backlog/read.rs, rsi/log.rs |
| R8 | `mint-deploy-notice` | theseus-pw1q.21.8 | C (staged) | R7 joined | rsi/mint/{deploy,retire}.rs, extend/{answer,load}.rs, rsi/backlog/graduate.rs, rsi/digest.rs, config/self_improve.rs (`[self.mint]`) |

**Join order:** R1, R5, R6 (wave A, as each is ready), then **R2** (last of wave A), then R3 and R7 (wave B), then R4
and R8 (wave C). Joins are plain merges. Then the switch-on (theseus-pw1q.17) once all eight are joined and installed.

**Why R2 joins after R5:** `sealed::KEEL_PATHS` lists `config/self_improve.rs` (the `[self]` config, budgets included),
and R2 puts every `KEEL_PATHS` entry into the guard's `KEEL_FILES`. From R2's join on, any edit to that file is a
`keel-file` finding that only the owner's yes clears. The owner's 23:34:35 yes covers the sealed-scorer rows (R1-R4),
not the mint rows. So the one field line `[self.mint]` needs in `self_improve.rs` lands with R5, before R2, and R7 and
R8 never edit a `KEEL_PATHS` file.

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do not
   refactor code you do not need, and do not reformat files you only read.
2. **The kernel's `L1` struct** (`theseus-kernel/src/job.rs:76`): R1 adds exactly one field, `rw: Vec<PathBuf>`
   (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`), and a pids value for self builds. R3 adds nothing to
   `L1`: its runner view is built from the fields that exist (plus R1's `rw` left empty), and its relay is one more name
   in `egress`. If R3 needs a field, it adds it after the last existing field with the same serde attributes, and says
   so in its report.
3. **`config/self_improve.rs`** (a `KEEL_PATHS` file):
   - **R5 owns all of `[self.mint]`**, every key in the design's §6.1 (`mine`, `bench_dirs`, `window_days`,
     `min_occurrences`, `min_sessions`, `min_days`, `per_night`, `max_live`, `usd`, `replay_turns`,
     `retire_after_days`, `hint`, `kinds`), with defaults that act on nothing (`mine = false`), in a new file
     `config/self_mint.rs`, plus **one** field line in `self_improve.rs` and the template's `[self.mint]` block. Keys
     that only R7 or R8 read carry a doc line saying which step reads them. R7 and R8 add no config key; if one needs a
     new key, it goes in `config/self_mint.rs` (not a keel path) and the report says so.
   - **R3 owns all of `[self.sealed]`** (`dir`, `every_branch`, `item_usd`, `drift_every_days`) and the rename of
     `[self] sealed_dir` to `[self.sealed] dir` (the old key fails the load: "[self] sealed_dir was renamed to
     [self.sealed] dir"; no alias). R4 reads those keys and adds none.
   - **R1** adds at most `[self] build_pids` (default 8192), and reads `sealed_dir` by today's name in `rsi/pr.rs`. At
     R3's join the joiner renames that one call site.
   - **No mint row edits another `KEEL_PATHS` file** (`rsi/sealed*`, `toolrun/guarded.rs`, `config/self_improve.rs`
     beyond R5's one line). R6 registers `flow.run` in `toolrun.rs` and leaves `toolrun/guarded.rs` alone.
4. **`rsi/backlog/{read,rank}.rs`:** R5 adds pattern causes and their score as a new `Obs` source and one rank function
   beside `rank.rs:31`'s; it does not change how fix causes rank. R7 and R4 add their own sources the same way later.
5. **`rsi/sealed.rs`:** R2 adds paths to `KEEL_PATHS` only (and its sync test in `rsi/sealed/tests.rs`). R3 adds the
   `runner` and `relay` modules' `mod` lines and the production `Runner`. R4 adds the hold. None renames what exists.
6. **The protocol:** each row that adds wire types puts them in its own module file (`patterns.rs` for R5, `flow.rs` for
   R6, `mint.rs` for R8) and adds **one** `pub mod` line to `crates/theseus-protocol/src/lib.rs` (4 lines of headroom
   for three rows). Re-export from the module path, not with a new `pub use` line in lib.rs.
7. **The flow types are R6's.** R7 builds a flow spec only through R6's `flow::spec` types; R8 loads one only through
   R6's `flow::load`. **The motif types are R5's** (`rsi::mine::Motif`, `Shape`, `SlotKind`, `Occurrence`): R7 reads
   them, never redefines them.
8. **Ledger kinds:** new rows go in fact modules of their own (`fact/flow.rs`, `fact/rsi_mine.rs`, `fact/rsi_sealed.rs`,
   `fact/rsi_mint.rs`), each with its doc and its cockpit/ledger reader test, as the existing fact modules are.
9. **Never mined, never minted:** calls that face outward or spend (message, email, Discord, AWS writes, hands, web
   posts, `extend.*`, `self.*`, approvals, memory writes). R5 cuts a stretch there; R6's allow-list refuses them.
10. **The data rule:** a motif drawn from private ledger sessions is written and built only on Claude models, failing
    closed (a China-origin profile is refused by origin, never by name). A motif drawn only from bench trials may use
    any model.
11. **Leave `crates/theseus-discord/` and `crates/theseus/src/herdr*` alone**, except R8's morning notice, which posts
    through the existing digest path (`rsi/digest.rs`), not through new Discord code.

## Out of scope for every row

Switching anything on: no default changes to `on`, `act` or `true` for a self or mint key, and nothing touches the
owner's config. The self-bench's 12 authored gaps (the owner's). Seeding a sealed set with real items. The benchmarks
(the owner moved them to right before the v1 cut). `docs/status.md`, the README, `docs/design/` (the maintainer writes
those; say what should change). If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Off by default.** Nothing in this row acts while `[self] mode = "off"`: every new phase asks `rsi::gate` first, and a
  test shows that with the mode off (and with the kill switch halted) the new code reads nothing and writes nothing.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn bench and the
  owner-config fixture test must pass). A new model tool is deferred by default. FAST: nothing new on a plain turn (a
  plain turn stays 5 frames), and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Spend goes through
  `[self.budget]`'s reserve-before-spend; no new spend path. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty unless this task names keel findings; then the report gives, for each rule the guard names, the exact `Keel:`
  trailer line for the joiner's signed commit (this task gives the form). You can't sign here; the joiner signs with the
  owner's key.
- No new dependency: Cargo.lock and the package-lock.json files gain no package. A store format change only if this task
  names one (bump by one from 45 and add the old layout's literal sample to tests_layouts.rs; the joiner renumbers).
- **Secrets and names.** Tool inputs are not scrubbed at rest: hold raw values in memory only, store shapes or ids, and
  run anything that leaves (a spec, a report, a row, a push) through `core.tools.scrubber`. A test plants a
  secret-shaped value and shows it never reaches what is stored. No person's name anywhere in the repo, in commit
  messages or in the report: write "the owner". If you edit a line that carries a person's name, replace it.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your L1 tests you could run and give the maintainer the exact commands for the rest).
