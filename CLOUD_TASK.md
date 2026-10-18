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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-keel-deletions`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-keel-deletions, the owner-approved deletions the other trim rows were told to leave: the startup unit-budget migration and the retired config keys, the seven superseded Jev pack TOMLs, jev-probe compiled by the gate, and dead code that never landed, each with its tests (theseus-rwpbg.9)

Branch: `cloud/20261017-trim-keel-deletions`. Every commit's subject carries `theseus-rwpbg.9`. Deadline for the report: 4 hours after you start.

**The owner's approval (Discord DM, 2026-10-17 16:44:39 MST), verbatim:** "yes to all four", answering a question
that listed exactly the four parts below (A to D). These parts delete tests and assertions, so they are **keel**: the
joiner lands them in a commit **signed by the owner's key** with trailers quoting that approval. You can't sign here:
build it, and put the exact trailers the guard asks for in the report (see "Keel" at the end).

**This row is the exception to the shared section's "Out of scope" list.** That list (written for the eight sibling
rows) names the review's part 1 §2b, §3, §4, §7, §9, §18 and §19 deletions as out of scope. For **this row**, exactly
the items named below are in scope, and nothing else on that list is: not the sealed holdout set (no `sealed_put`, no
`sealed_score`, nothing under `rsi/sealed/`), not the unwatch RPCs, not the old-frame readers.

**Source:** the trim review's part 1: §9 (startup migration, retired keys), §3 (old Jev packs), §4 (jev-probe), §7
(serving-daemon chain), §2b (`node_at`, `break_even_turns`, `resolved_lines`), §18 (`StopAfterOneLoop`), §19
(`task.get`). Every `file:line` below was read at `0f93aa81` (main, 2026-10-17 16:34 MST); search by name if one moved.

**Prerequisite:** `main`. Builds now; joins **8th**, after trim-utils and before trim-tests-dedup (which stays last).

## Your row
| # | Slug | Beads | Scope | Files | Tests | Size |
|---|---|---|---|---|---|---|
| 9 | `trim-keel-deletions` | theseus-rwpbg.9 | A: §9 startup migration + retired keys (old-frame readers kept); B: §3 seven pack TOMLs out of `EMBEDDED`, `CLASSIFY_V1`; C: §4 jev-probe in the gate's clippy; D: §7, §2b, §18, §19 dead code with its tests | kernel `kernel.rs`, `lib.rs`, `tests_budgets.rs`; core `rpc/mod.rs`, `config.rs`, `sandbox.rs`, `config/*`, `store.rs`, `routing.rs`, `advancer.rs`, `lib.rs`, `learning/prove.rs`, `rpc/methods.rs`; sim `kernel_sim.rs`, `main.rs`; judge `pack.rs`, `packs/`, `builders.rs`, `tests_route_v5.rs`; `scripts/gate.sh`; protocol `tasks.rs`, `lib.rs`, `ts.rs`; `cockpit/src/protocol.gen/` | the old-frame readers' tests unchanged and green; a kept reader test per part (below); keel guard lists only the findings this task names | L |

Do the four parts in this order, one commit (or a few) each, each green before the next: **C, D, B, A** (smallest
first, so a session that runs out of time still lands whole parts).

## C. The gate compiles jev-probe (§4; no deletion)
`crates/theseus-judge/src/probe.rs` is `#[cfg(feature = "probe")]` (`lib.rs:48`) and `src/bin/jev-probe.rs` has
`required-features = ["probe"]` (`Cargo.toml:17-20`); the gate's clippy phase (`scripts/gate.sh:756`, `phase clippy
cargo clippy --workspace --all-targets -q -- -D warnings`) never builds them. Add one phase line after it:
`cargo clippy -p theseus-judge --features probe --all-targets -q -- -D warnings` (follow how gate.sh names and orders
phases; if `phase` names must be unique, call it `clippy-probe`). Run it: if it fails today, fix what rotted (it is
code the owner keeps) and list the fixes. Report the phase's wall time. No other gate change.

## D. Dead code that never landed, with its tests (§7, §2b, §18, §19)
Before each delete, re-run the census on your clone: `rg -n -w <name> crates cockpit/src scripts` (and the cockpit's
single-quoted method strings for D4). An item with a production caller on your base **stays**: say so in the report.

1. **§7, the serving-daemon chain** (`crates/theseus-kernel/src/job.rs`): `DAEMON_VALUE_FLAGS` (`:880`),
   `daemon_in_cmdline` (`:893`), `serving_daemon` (`:916`), `pid_alive` (`:938`), and their tests: job.rs's own
   (`:1828-1858` and `pid_alive`'s), the theseusd unit test that keeps `DAEMON_VALUE_FLAGS` in step with its command
   line (`crates/theseusd/src/main.rs:~1689`), and `pid_alive`'s use in `crates/theseus-core/src/tests_m3.rs` (if that
   test asserts something else besides, keep the test and replace only the `pid_alive` call with the smallest local
   equivalent; say which). Fix `crates/theseusd/AGENTS.md`'s mention of `DAEMON_VALUE_FLAGS`.
   **Shared with trim-utils:** trim-utils (join 7) deletes `serving_daemon` alone. You delete the whole chain,
   `serving_daemon` included (your base still has it, and it calls `daemon_in_cmdline`). Both deletions of
   `serving_daemon` are the same lines; the joiner keeps the deletion. You own `daemon_in_cmdline`,
   `DAEMON_VALUE_FLAGS`, `pid_alive` and the chain's tests; trim-utils owns the other 36 dead pub items: touch none
   of them (its list includes `HOLDOUT_DAYS`, `sealed_score`, `without_messages`, `tender_of`, `open_count`, the
   aws-catalog `is_*` helpers and the LSP constants).
2. **§2b, `Store::node_at`** (`crates/theseus-core/src/store.rs:1054`): its only reader is `tests_tiering.rs:423`.
   Delete it and that use. If the test checks the node cache through `node_at` and also something else, keep the test
   on another reader (`get_node` / `session_nodes`) so the cache keeps a test; say which. Fix
   `crates/theseus-core/AGENTS.md:~2237`, which lists "a recall's source by `Store::node_at`" as a reader.
3. **§2b, `routing::break_even_turns`** (`crates/theseus-core/src/routing.rs:561`) and its tests.
   **trim-tokens** (join 6) adds its own new break-even function for keep-warm in `keep_warm.rs`; it is a different
   function. Don't touch keep_warm.rs. (The joiner: if trim-tokens' branch turns out to call
   `routing::break_even_turns`, drop this item at the join.)
4. **§2b, `arrangement::resolved_lines`: re-check, expected to STAY.** The review called it dead at `c6f62abf`, but
   on `0f93aa81` it has a production caller (`crates/theseus-core/src/task.rs:798`, the task's start reply). If that
   caller is still there on your base, keep `resolved_lines` and its tests, and say so in the report.
5. **§18, `StopAfterOneLoop`** (`crates/theseus-core/src/advancer.rs:36-46`): delete the struct, its `impl`, and its
   tests in advancer.rs. Keep the `Advancer` trait (whether it should become a struct is a later pass; say in the
   report how many implementors it has left). Keep the `"stop_after_one_loop"` string wherever stored data or a test
   fixture carries it as a recorded stop reason (`telemetry/tests.rs:451` is a fixture: old ledger rows hold it).
   Fix `crates/theseus-core/src/lib.rs:4`'s stale doc ("an Advancer whose only policy is `stop_after_one_loop`") to say
   what the turn uses today (`UntilNoToolCalls`). Leave `docs/spec/` alone (it is the record).
6. **§19, the `task.get` RPC:** `TASK_GET` (`crates/theseus-protocol/src/lib.rs:272-273`), `TaskGetParams` /
   `TaskGetResult` (`crates/theseus-protocol/src/tasks.rs:305-320`, and `tasks.rs:8`'s module doc), their `ts.rs:165`
   entries, the handler (`crates/theseus-core/src/rpc/methods.rs:~781-791`) and its dispatch
   (`crates/theseus-core/src/rpc/server.rs:~325`), the test at `tests_task_graph.rs:~415-420` (keep the test if it
   also checks `task.list`'s records: drop only the `task.get` half), and the generated TypeScript
   (`cockpit/src/protocol.gen/TaskGetParams.ts`, `TaskGetResult.ts`, their `index.ts` lines): regenerate per the
   repo's AGENTS.md, never hand-edit, then `npm run build` in `cockpit/` must pass. **KEEP `narrative.unwatch` and
   `executions.unwatch`.** Check the method list's own tests (a test that counts or lists methods) and the CLI
   (`theseus rpc` takes any name, so nothing there names it, but check `rg -n 'task\.get' crates/theseus`).
   Protocol `lib.rs` shrinks by these lines (trim-utils needs one line there for `pub mod jsonrpc;`: leave the freed
   room, raise no ceiling).

## B. The seven superseded Jev pack TOMLs (§3)
`crates/theseus-judge/src/pack.rs:~49-77` `EMBEDDED` lists 25 packs; seven are wired nowhere (`WIRED`,
`crates/theseus-core/src/judge/mod.rs:89-106`): `probe.v1`, `security.v2`, `classify.v1`, `route.v1`, `route.v2`,
`route.v3`, `route.v4`.
- **Delete** `security.v2`, `classify.v1`, `route.v1`-`route.v4`: their `EMBEDDED` lines, their TOML files in
  `crates/theseus-judge/packs/`, and the doc lines above `EMBEDDED` that describe them (keep one sentence that older
  versions are in git and replay by `--pack-file`). **The `security.v2` builder stays** (`builders/security2.rs`:
  `security.v3` is built by it); only its TOML goes. Don't touch `security.v1` (trim-security-v1 keeps it embedded for
  replay) or any wired pack.
- **`probe.v1`: choose the smaller change, and measure it.** It is jev-probe's pack, and also more than that on
  `0f93aa81`: the judge crate's own test pack (`src/batch.rs:180-228`), a parse fixture in `pack.rs`, and the subject
  of `fixtures/golden/probe.v1.request.json`, `fixtures/wire/probe.request.json` and
  `fixtures/wire/discover.response.json`. Option 1: keep `probe.v1` embedded (and say why: its readers). Option 2:
  move it to a test/fixture path that jev-probe and the tests load from a file. Pick by the lines each needs; my
  expectation is option 1 (zero lines). Report the counts either way.
- **Stored judgments stay readable.** The ledger keeps judgments named `route.v2`, `classify.v1` and so on forever.
  Readers that use the stored judgment and only the pack's **name** keep working (`learning/prove.rs:~339` scores past
  `classify.v1` judgments; `correction/layer.rs`, `judge/lineage.rs:50` `LEFT_ALONE`, `judge/ladder/adopt.rs:41-56`,
  `config/judge.rs`, `config/routing.rs` name them): keep each name reference that reads stored data or config, and
  change only the ones that load the pack **text** (`pack::by_name("route.v2")` and the like). For each name reference
  `rg -n -E '"(route\.v[1-4]|classify\.v1|security\.v2)"' crates` finds, write in the report: kept (reads a name) or
  changed (loaded the text), with why.
- **Config:** a config naming a deleted pack (`[judge.packs."route.v2"]`, a routing key naming `route.v4`) must still
  load, as other retired or unknown packs are treated today: find that rule (`config/judge.rs`,
  `config/tests_fresh.rs`) and keep it. A test proves a config naming `route.v2` loads (add it only if none exists).
- **Replay:** `judge.replay --candidate <name>` finds a candidate in `pack::EMBEDDED`
  (`crates/theseus-core/src/learning/replay.rs:~300-310`). A deleted name must fail with a clear error that points at
  `--pack-file` (pin the message in a test); `--pack-file` with the old TOML (from `git show
  0f93aa81:crates/theseus-judge/packs/route.v2.toml` into a test fixture under `tests`, or an inline string) must still
  replay. Keep that test small.
- **Tests that enumerate the packs** (`crates/theseus-judge/src/builders.rs:~1083-1152`, `src/tests_route_v5.rs`, and
  any other a `cargo nextest` run shows failing): update each to the remaining set. A test that asserted a deleted
  pack's own content goes (keel finding); a test that iterates `EMBEDDED` keeps its loop and its assertion (an equality
  stays an equality; only the expected list changes).
- **Rename** `crates/theseus-core/src/learning/prove.rs:339`'s `CLASSIFY_PACK` (= `"classify.v1"`) to `CLASSIFY_V1`
  and its uses (`learning/` and any test that imports it; `judge/inbound.rs:49`'s `CLASSIFY_PACK` = v2 is a different
  constant: leave it).
- Report the bytes this removes from `EMBEDDED` (sum of the deleted TOMLs) and, if you can build a release theseusd in
  the cloud, the binary's size before/after (else say you could not).

## A. The startup unit-budget migration and the retired config keys (§9)
**The owner's config has been checked:** on 2026-10-17 17:03 MST the maintainer read `/etc/theseus/theseus.toml` and
`~/.theseus/config.last-good.toml` (key names only): neither has an `[approval]` or `[labels]` table, a `[kernel]`
table at all, or `sandbox.memory_mb` (its `[sandbox]` keys are `egress` and `ro_paths`), and `[judge]` names no pack.
So deleting these keys loses nothing the owner's config sets.

**KEEP, untouched: the old-frame readers.** These are what lets the owner's first month of WAL replay:
the unmarked-frame reader (`crates/theseus-store/src/wal.rs:~95-160`, `MAGIC_UNMARKED`), `Execution::from_stored`'s
unit-budget decode (`crates/theseus-kernel/src/types.rs:~430-451`), `LedgerKind::RENAMED`
(`crates/theseus-protocol/src/ledger.rs:10-34`), the `"denied"` result alias (`crates/theseus-core/src/node.rs:52`),
`MANIFEST_OLDEST`, the `budget.migrated` ledger kind (`protocol/src/ledger.rs:78`: stored rows carry it) and the
cockpit's readers of it (`cockpit/src/lib/summary.ts:78`, `timemachine.ts:355-363`), and `tests_layouts.rs`. Their
tests stay green and unchanged.

1. **Delete the startup migration:** the rewrite step in kernel startup (`crates/theseus-kernel/src/kernel.rs:~2831-2885`:
   the `terms::one("legacy")` query and the loop that rewrites schema-1 executions), the `LegacySpend` hook (`:444-445`
   type, the `legacy_spend` field `:469`, its clones `:558,582,603`, `with_legacy_spend` `:671-675`, the re-export in
   `kernel/src/lib.rs:112`), the core's wiring (`crates/theseus-core/src/rpc/mod.rs:~650-675`, the `legacy_spend`
   closure), any `StartupReport` field only the migration fills (and its readers), and the sim's LEGACY checks
   (`crates/theseus-sim/src/kernel_sim.rs`: `legacy_migrated` `:169`, `LEGACY` `:266-314`, `legacy_spend`, `:418-428`,
   and `main.rs:~621`'s print). Check scripts that parse the sim's output for `legacy_migrated`
   (`rg -n legacy_migrated scripts tools`), and the lifecycle bench's expectations.
2. **Its tests:** `crates/theseus-kernel/src/tests_budgets.rs` (220 lines; its tests are the startup rewrite and the
   reopen at the next start, theseus-3ebd) goes with its `mod` line (`kernel/src/lib.rs:59`). **But keep a reader test:**
   if, after the delete, no test decodes a unit-budget execution through `Execution::from_stored` (an old record reads
   in dollars), move the smallest such assertion from `tests_budgets.rs` into the kernel's existing tests of `types.rs`
   first, unchanged, then delete the file. Also `crates/theseus-core/src/rpc/tests.rs:~1327` and
   `crates/theseusd/tests/config_copy.rs:~369` (each reads `budget.migrated` rows: if the test is about the migration,
   it goes; if it checks something else and counts migrated rows in passing, keep it and drop the count; say which).
   **Not** `crates/theseus-core/src/tests_budgets.rs`: that is `budget.list` (step 42a), a different file; it stays.
   Check the reopen behaviour (theseus-3ebd, "reopened under its limit") lives elsewhere (`reopen.rs`) for executions
   that are not legacy: keep it and its tests; only the legacy trigger of it goes.
3. **Delete the retired config keys and their warnings:** `[approval]` (`crates/theseus-core/src/config.rs:~241-246`
   field, `APPROVAL_RETIRED` `:362`, its warning in `parse` `:~1369`), the `labels` alias (`:246`, `#[serde(default,
   alias = "labels")]`: delete the alias only, keep the field it names), `kernel.default_budget` /
   `kernel.control_reserve` (`:732-740`, `retired()` `:830-831`, the units warning `:~1344-1350`),
   `sandbox.memory_mb` (`crates/theseus-core/src/sandbox.rs:81-85, 120, 159-165`, `config.rs:1359`), and their tests
   (`sandbox.rs:~931-934` `the_retired_memory_key_loads_with_one_warning`, and the config tests of each warning). After
   this, a config that still names one of these keys **fails to load** (config is `deny_unknown_fields`): that is the
   approved outcome. Add one test that pins it for one key (e.g. `[approval]` is now an unknown key and the error names
   it), so the next reader sees it is deliberate. If `parse`'s warnings list is left with no producer, keep its shape
   (other warnings may use it); say what remains.
   Leave the template (`crates/theseus-core/config/theseus.example.toml`) alone unless it mentions a retired key.
   **trim-config-inherit** (join 4) edits `config.rs`'s `ModelConfig`/`ProfileConfig` (`:1038-1090`) and the profile
   resolution (`:1694-1698`), `config/cache.rs`, `config/sparse.rs` and the template: stay out of those regions.
4. Not in this row: `ServerConfig.store_engine` and `ProviderConfig.kind` (§8): leave them.

## Keel (the joiner's part)
Run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` on your last commit and paste its full
output. Expect `test-removed` / `assert-removed` lines for: job.rs's chain tests and theseusd's flags test (D1),
`tests_tiering.rs` (D2, if changed), routing.rs (D3), advancer.rs (D5), `tests_task_graph.rs` (D6), the pack tests (B),
`kernel/src/tests_budgets.rs` and the retired-key tests (A). Anything else it names (a budget, a cap, a ceiling, an
`allow`) is a mistake: fix it instead of acking it. Write in the report the exact trailer lines for the joiner's
signed commit, in the guard's form (`Keel: <rule> <path>[, <path>...] — <why>`), one per rule, each quoting the
approval, e.g.:

`Keel: test-removed crates/theseus-kernel/src/tests_budgets.rs, crates/theseus-kernel/src/job.rs — the owner, Discord DM 2026-10-17 16:44:39 MST: "yes to all four" (trim review part 1 §9, §3, §4, §7, §2b, §18, §19 deletions)`

and list every test you removed or changed, with the part it belongs to and why.

## Proof nothing is lost (report section)
- Per item: the census before (`rg -w` hits, all in the item and its tests), lines removed (`git diff --stat` per part).
- A: the old-frame readers' tests unchanged and green (name them: `tests_layouts.rs`, wal's unmarked-frame tests,
  `from_stored`'s); the owner-config check quoted above; the pin that a retired key now fails to load.
- B: a stored `classify.v1`/`route.v2` judgment still lists and scores (`learning/prove.rs`, the judge readers' tests);
  replay by `--pack-file` still works; the unwired packs were in no `WIRED` line.
- C: the new phase's output and time.
- D: each item's census; `resolved_lines` re-checked; the cockpit builds after the regenerated TypeScript.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `0f93aa81` (2026-10-17
16:34 MST) or later, with **store format 45**. (The eight sibling rows started from `432bbbeb`, 15:53.) The trim review that this row comes from read `c6f62abf`, so every
`file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

Line ceilings at `432bbbeb` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` **2,732 /
2,733**, `crates/theseus-kernel/src/kernel.rs` **3,026 / 3,030**, `crates/theseus-discord/src/runtime.rs` 3,495 / 3,500,
`crates/theseus/src/render.rs` 3,116 / 3,128, `crates/theseus-core/src/turn.rs` 3,351 / 3,525,
`crates/theseus-core/src/tests_m3.rs` 7,996 / 8,050, `crates/theseus-discord/src/render.rs` 2,926 / 3,009,
`crates/theseus-index/src/vectors.rs` 2,564 / 2,650. A trim should shrink these files, never grow them; never raise a
ceiling in this row.

## The trim rows (nine siblings, built in parallel today from main)

The owner asked for a trim pass before the v1 feature review: "an efficiency and simplicity and elegance check ... what
we can trim without losing anything, both on total code, total binary size, etc." The review found the items; the owner
settled the ones that needed him. Nine cloud rows build them at once, each from `main`, and one joiner merges them in
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
| 8 | `trim-keel-deletions` | theseus-rwpbg.9 | the owner-approved deletions: kernel startup migration, retired config keys, seven pack TOMLs, gate.sh's probe clippy, dead code with its tests (this row) |
| 9 | `trim-tests-dedup` | theseus-rwpbg.1 | theseusd/tests/ and theseus-core test helpers |

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

(For trim-keel-deletions, its task's own list of items overrides this section: see the top of the task.)

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
- **Keel.** Do not delete, ignore or weaken a test beyond the deletions this task names (the owner approved exactly
  those), and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names a keel finding (this row's are listed in its Keel section), and then the report gives
  the exact `Keel:` trailer line the guard asks for, for the joiner's signed commit.
- No store format change. No new dependency: Cargo.lock and the package-lock.json files gain no package (a version
  that drops a duplicate is fine; say so). Never loosen a test, budget or ceiling.
- No person's name anywhere in the repo, in commit messages, or in the report: write "the owner". If you edit a file
  that already carries a person's name, replace it with "the owner" in the lines you touch.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud. Add a section **"Proof nothing was lost"** listing, per change, the test or the
  before/after evidence.
