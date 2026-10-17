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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-tokens`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-tokens, the first request back under its budget and smaller everywhere: learned tool loads capped at the place budget (the fix for theseus-buz3h), a shorter tools note, reminders split from tasks, keep-warm stopped at break-even, index commits coalesced, and two small correctness fixes (theseus-rwpbg.3)

Branch: `cloud/20261017-trim-tokens`. Every commit's subject carries `theseus-rwpbg.3` (and the T1 commit also
`theseus-buz3h`). Deadline for the report: 4 hours after you start.

**Source:** the trim review's part 3: T1, T3, T5, M3, W1, and its two possible bugs (H7, T4's dotted names).
**The owner's rule** (2026-10-14, accepted): the first request as sent is at most 8,000 tokens in cli/task places and
15,000 in the owner's DM; new model tools are deferred by default, always-loaded only with ledger evidence of use in
about 1 of 5 sessions of that place. **T5 is owner-settled** by that rule: the DM default keeps `wake_at` and the task_*
tools defer. **Do not change `PLAN_NOTE` or merge the plan tools** (the owner said wait).

**Prerequisite:** `main`. Builds now; joins 6th of the eight trim rows.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Size |
|---|---|---|---|---|---|---|
| 3 | `trim-tokens` | theseus-rwpbg.3 (fixes theseus-buz3h) | T1, T3, T5, M3, W1, H7, dotted names (below) | `crates/theseus-core/src/tools_defer/{select,board,want,family}.rs`, `toolrun.rs` (the tools note), `keep_warm.rs` (one fn + call), `turn/compile_step.rs`, `task.rs` / `wake.rs` descriptions, `crates/theseus-index/src/tender.rs` | `learned_loads_stay_inside_the_place_budget` (revert: skip the demotion), `a_search_alone_is_not_use` (revert: count searched), `the_owner_shaped_cli_with_learned_tasks_fits_8000` (census, seeded ledger), `the_tools_note_lists_exceptions` , `the_dm_default_keeps_wake_at_and_defers_tasks`, `keep_warm_stops_at_break_even` (revert: no cap), `index_commits_coalesce_within_the_window` (revert: commit every step), `the_recorded_digest_is_the_sent_body` (revert: digest before the tail), `descriptions_name_wire_tools` | L |

## The live numbers (owner's daemon, 2026-10-17 15:05, `theseus census --exact`)
| place | estimate | exact | budget |
|---|---|---|---|
| cli | 14,788 | **13,665** (system 1,306 + tools 12,359) | 8,000 |
| task | 6,918 | 6,655 | 8,000 |
| dm | 14,310 | 13,290 | 15,000 |

At cli the learned set is tasks (task_create 2,561 + task_update 665 + task_close 506 + task_claim 320 + task_split 280 +
wake_at 1,033 + wake_move 379 + wake_cancel 190 = 5,934 estimated tokens) and aws (aws_call 753 + aws_describe 274 +
aws_whoami 165 = 1,192). Without them cli is ~7,660 estimated / ~7,300 exact. task_create, task_update, task_close,
task_claim and task_split had **0 calls in 30 days** in every session; they were loaded because searches counted as use.

## What to build

### T1. Learned loads capped at the place budget; only calls count as use (theseus-buz3h)
Why the budget is never consulted today (all verified at `c6f62abf`):
1. `tools_defer/select.rs:139-190` (`select`) is pure over counts: it gets family names (`known`), no catalog and no
   sizes. Lines 166-172 load a family whenever `turns >= MIN_TURNS` (10, line 17) and `used/turns >= cfg.load_share`
   (0.10, `config/defer.rs:44-46`). No budget input.
2. The only budget check, `want.rs:76-106` (`extra`), runs only for the Find mechanism (a model without native tool
   search) and only for families a message names; Sonnet/Opus never reach it (`want.rs:12-13`).
3. `board.rs:316-317` (live) and `board.rs:375-376` (replay from ledger rows) count `used = called ∪ searched`.
   And `want.rs:99` compares tools-only tokens to the whole-request budget (the system's ~1.3k not subtracted).

**Fix:** in `Board::select` (`board.rs:153-181`, the caller that has the session and can be given the catalog), after
`select::select`, demote `Why::Learned` families greedily (largest token cost first; ties by lowest use share) while
`system tokens + resolved loaded tools` exceeds `kind.tools_budget_tokens()` (or the place's first-request budget: use
whichever the code already treats as the place budget, and say which). A demoted family is `Deferred` (one
`tool_search` away), with a reason the census can show (`Why::OverBudget` or similar). Defaults (`Why::Default`) and
always-loaded tools are never demoted by this. Count only `called` as use in both the live and the replay path; keep
`searched` as its own stored column (it is already stored). Subtract the system's tokens in `want.rs:99` too.
- **Census proof:** a test that seeds a ledger making tasks and aws Learned at cli (the prompt bench
  `theseus-sim/src/perf/counted.rs:355` starts with an empty ledger and cannot see this), runs the census for the
  owner-shaped config (`fixtures/owner_shape.toml`, as the first-request budget test does), and asserts the cli first
  request's **estimate** is ≤ 8,000 tokens (the estimate reads ~8% high against the provider, so the exact count is
  lower). Plant: skip the demotion, see it fail.
- Also report (no change): `first_request.rs:9-11` states the rule per session ("about 1 of 5 sessions") while the code
  counts turns at 0.10. Say what changing it would take.

### T3. The tools note lists exceptions (~200-260 tokens on every private request)
`toolrun.rs:723-739` builds `"open: a, b, …; notify: …; approve: …"` over **all** registry tools that pass
`tools_defer::listed` (63 names, ~325 tok) at every private place (`toolrun.rs:764-779`), including tools the request
does not carry. Print the exceptions and a default instead: name the posture that most tools share once ("every other
tool: notify", computed, not hard-coded: the default is whichever posture has the most tools), and list by name only the
tools whose posture differs. Drop the note's `Prefer fs_read, fs_edit, … over proc_run` line and its background line,
which proc_run's own description (`theseus-tools/src/proc.rs:147`) and its `background` field already say. Proof: every
tool's posture is still determined (a test reconstructs each tool's posture from the note and compares it with the
registry); enforcement is in `policy`, not the note (no policy change). The header digest changes once (one cache
rewrite per session at deploy: say so in the report). `PLAN_NOTE` (interpolated at `toolrun.rs:769/776`) stays exactly.

### T5. Reminders split from tasks; the DM default keeps wake_at, task_* defers (owner-settled)
`tools_defer/family.rs:114` maps `"task" | "wake" => "tasks"`, and `family.rs:251-255` gives the DM `["memory", "tasks",
"web"]` (Channel adds `channels`). In the DM census the tasks family is ≈ 5,900 of 13,290 tokens; task_* had 0 calls in
30 days, wake_at 1 call (2026-10-10). Make `wake` tools a family of their own, `reminders`; the DM default becomes
`["memory", "reminders", "web"]`; **Channel keeps both** (`tasks` and `reminders`: no behaviour change there); cli/task
defaults unchanged (neither is a default there). task_* at the DM is then deferred and comes back by search or by the
learned rule (T1's calls-only count). Check every place that names the `tasks` family (the search's family index,
`family.rs:285-286`'s table, the census, `budget` tests, the ledger replay of family names: an old ledger row that
recorded `tasks` for a wake call must still count for `reminders` on replay, or say why it need not). Expected: DM ≈ 9k.
Tests whose expected loaded set changes keep an equality (update the expected list; don't weaken it to a contains).

### M3. Keep-warm stops at break-even, per model (contract 2: not in `inherit`)
`config/cache.rs:84-90` (`keep_warm_hours` = 24) with reads every `keep_warm_minutes` (55, `config/cache.rs:10-14`). A
read costs the model's cache-read price on the prefix; a return after expiry costs one 1-hour cache write. Past
`(write_1h / read) × keep_warm_minutes` the reads cost more than the rewrite they avoid. Cap each entry's window at
`min(keep_warm_hours, break_even)` computed from the catalog's prices for the entry's model (one small fn in
`keep_warm.rs` or beside the catalog's prices, one call where keep-warm decides whether an entry is still warm). A model
with no prices in the catalog keeps today's window. Report the caps per catalog model on your base (prices may have moved:
the China-origin rows changed some rows today). Test: an entry past its model's break-even gets no read; one before it
does (plant: drop the cap).

### W1. The index tender coalesces commits
`theseus-index/src/tender.rs:497-522`: each `step()` that indexed anything runs `writer.commit()`, `save()` (writes,
fsyncs, renames `cursor.json`, `theseus-index/src/state.rs:88-96`) and `engine.reload()`; `run()` goes straight back to
`waker.wait` (`tender.rs:816-826`), and the waker returns on the first inotify change (`theseus-follow/src/wake.rs:166-168`).
So every node frame of a turn costs a commit, a segment, a cursor fsync and a reader reload. After a commit, while
caught up, wait a short coalescing window (a const, 100-250 ms; pick one and say why) or enforce a minimum gap between
commits, so a 10-tool turn (~20 frames) makes ~3-5 commits. The cursor is still written only after a commit
(`512-517`), so a crash re-reads from the WAL. Tests that expect immediate indexing get a flush hook (or wait on
`indexed_through`), never a longer sleep. Test: N frames written in a burst produce at most a few commits (count via
a test counter), and a search after the window finds all of them (plant: commit every step). The core only reports
`indexed_through` (`theseus-core/src/recall.rs:783`) and never waits on it: confirm.

### H7 (bug). The recorded `request_digest` is taken before the tail blocks are attached
`turn/compile_step.rs:162` runs `tail_blocks` (which `attach_text`s the tasks/project tail into the request) after the
compile set `compiled.digest`; only `route_step.rs:382` recomputes the digest, and only on an effort change. So
`request_digest` (`turn.rs:2630`) and the overflow retry's "same request" check (`turn.rs:1809`) can describe a body that
differs from the one sent. Recompute the digest after the tail (one call, in `compile_step.rs`, not in `turn.rs`). Test:
the recorded digest equals the digest of the body the provider received (the fake model keeps request bodies); plant:
skip the recompute. Trace the data flow and report whether the overflow retry was actually affected.

### Dotted names in model-visible descriptions (possible bug)
`wake.rs:298-317` names `wake.move`/`wake.cancel`, `task.rs:249` names `task.update, task.split, task.close`, and
`task.rs:231` `wake.at`, while the model sees `wake_move`, `task_update`, `wake_at` (underscore wire names). Use the wire
names in every model-visible description (grep all tool descriptions for `[a-z]+\.[a-z_]+` tool-shaped names; keep
dotted names where they are RPC method names in operator-facing text). Test: every tool name mentioned in a model-visible
description is a registered tool's wire name (plant: put one dotted name back). Token counts barely move; say how much.

## Proof nothing is lost (report section)
- A demoted family is one search away (show the census line for it); 0 calls of the tasks tools in 30 days (cite the
  review's evidence); aws had 14 calls in 30 days across all places, and a search loads it.
- Every tool's posture is still stated (T3's reconstruction test).
- Census before/after for cli, task and dm with the owner-shaped fixture (estimates) in a table.
- The index finds every node after the window (W1's test); the cursor rule is unchanged.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `432bbbeb` (2026-10-17
15:53 MST) or later, with **store format 45**. The trim review that this row comes from read `c6f62abf`, so every
`file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

Line ceilings at `432bbbeb` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` **2,732 /
2,733**, `crates/theseus-kernel/src/kernel.rs` **3,026 / 3,030**, `crates/theseus-discord/src/runtime.rs` 3,495 / 3,500,
`crates/theseus/src/render.rs` 3,116 / 3,128, `crates/theseus-core/src/turn.rs` 3,351 / 3,525,
`crates/theseus-core/src/tests_m3.rs` 7,996 / 8,050, `crates/theseus-discord/src/render.rs` 2,926 / 3,009,
`crates/theseus-index/src/vectors.rs` 2,564 / 2,650. A trim should shrink these files, never grow them; never raise a
ceiling in this row.

## The trim rows (eight siblings, built in parallel today from main)

The owner asked for a trim pass before the v1 feature review: "an efficiency and simplicity and elegance check ... what
we can trim without losing anything, both on total code, total binary size, etc." The review found the items; the owner
settled the ones that needed him. Eight cloud rows build them at once, each from `main`, and one joiner merges them in
this order:

| Join | Slug | Beads | Area |
|---|---|---|---|
| 1 | `trim-binary` | theseus-rwpbg.5 | scripts/build.sh, theseusd's web.rs, the cockpit's build, theseus-aws-catalog's Cargo.toml |
| 2 | `trim-docs` | theseus-rwpbg.8 | docs/glossary.md, docs/cli.md (new), the CLI's clap `about` lines, the CLI help goldens |
| 3 | `trim-security-v1` | theseus-rwpbg.6 | core judge/gate.rs, judge/notice.rs, judge/mod.rs, the security packs |
| 4 | `trim-config-inherit` | theseus-rwpbg.7 | core config.rs, config/cache.rs (`inherit`), config/sparse.rs, the template |
| 5 | `trim-effort` | theseus-rwpbg.4 | core turn/route_step.rs, compiler/effort.rs, keep_warm.rs (effort), catalog.rs (one field) |
| 6 | `trim-tokens` | theseus-rwpbg.3 | core tools_defer/, toolrun.rs's tools note, keep_warm (the window cap), turn/compile_step.rs, theseus-index tender |
| 7 | `trim-utils` | theseus-rwpbg.2 | many small sites: clip, dates, now_ms, dead pub items, protocol jsonrpc, core<->protocol mirrors |
| 8 | `trim-tests-dedup` | theseus-rwpbg.1 | theseusd/tests/ and theseus-core test helpers |

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need to touch, and do not reformat files you only read.
2. **`config/cache.rs`'s `inherit` belongs to trim-config-inherit.** trim-tokens' keep-warm window cap and trim-effort's
   keep-warm effort live in `keep_warm.rs` / the catalog, never in `inherit` or `ProfileConfig`.
3. **`keep_warm.rs`:** trim-effort adds the last request's effort to what keep-warm replays; trim-tokens adds the
   break-even cap on the window. Each adds its own small function and one call; neither restructures the file.
4. **`catalog.rs`:** trim-effort adds one field (`per_message_effort`) to `CatalogEntry` and `CatalogRow`; trim-tokens
   reads the existing cache prices for its cap and adds no field. trim-utils deletes no catalog item.
5. **`compiler/effort.rs`:** trim-utils deletes the dead `without_messages` (`effort.rs:80`); trim-effort must not
   start calling it.
6. **Test helpers:** rows 1-7 add tests in their own `tests_<subject>.rs` files (or a crate's existing test module for
   their subject) and may use today's helpers; trim-tests-dedup moves helpers only in files that exist on `main`.
7. **Leave `crates/theseus-discord/` and `crates/theseus/src/herdr*` alone** (the Discord rows theseus-bh3t.1-.6 and
   theseus-ss36w are joining to main right now, and a herdr lane is open). Discord's copies of `clip` and the date
   math wait for a later pass; say in the report which copies you left.
8. **The China-origin rows** (theseus-a1wvq.3-.13) build later on `catalog.rs`, `config.rs`, routing and the turn path:
   keep your edits there small and additive.

## Out of scope for every trim row (owner questions still open)

CLI command hiding, renames or folds; vocabulary renames (thread, mode, place, surface, section names); the sealed
holdout set; any deletion of a test (the review's part 1 §2b test-only APIs, §3 the old Jev packs, §4 jev-probe, §7 the
serving-daemon chain, §9 the startup migration and retired keys, §19 RPC removals); `PLAN_NOTE` and the plan tools; the
LTO profile. If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. This is a trim: the diff should remove more than it
  adds, and every line you add should earn its place.
- **Nothing lost.** Behaviour, wire JSON, stored records and user-visible output stay the same unless this task says
  otherwise. Where it says otherwise, pin the old output in a test first, then change it, and show the diff.
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore); list them in
  the report. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names a keel finding (only trim-security-v1 may have one), and then the report gives
  the exact `Keel:` trailer line the guard asks for, for the joiner's signed commit.
- No store format change. No new dependency: Cargo.lock and the package-lock.json files gain no package (a version
  that drops a duplicate is fine; say so). Never loosen a test, budget or ceiling.
- No person's name anywhere in the repo, in commit messages, or in the report: write "the owner". If you edit a file
  that already carries a person's name, replace it with "the owner" in the lines you touch.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud. Add a section **"Proof nothing was lost"** listing, per change, the test or the
  before/after evidence.
