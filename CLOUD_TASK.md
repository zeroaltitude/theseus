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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-utils`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-utils, one helper for each small job: one `clip`, jiff for dates, one `now_ms`, the 37 dead pub items gone, one JSON-RPC framing module, and core using protocol's wire types where it mirrors them (theseus-rwpbg.2)

Branch: `cloud/20261017-trim-utils`. Every commit's subject carries `theseus-rwpbg.2`. Deadline for the report: 4 hours after you start.

**Source:** the trim review's part 1: §13 (clip), §10 (civil dates), §18 (`now_ms`), §1 (37 dead pub items), §11
(JSON-RPC framing), §5 (core types mirroring protocol types). None needs an owner decision. The Discord crate's copies
wait (contract 7).

**Prerequisite:** `main`. Builds now; joins 7th of the eight trim rows.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Size |
|---|---|---|---|---|---|---|
| 2 | `trim-utils` | theseus-rwpbg.2 | six sub-steps below, one commit (or a few) each, in this order: dead items, clip, now_ms, dates, jsonrpc, mirrors | many small sites (below); new `crates/theseus-protocol/src/text.rs` (or the existing `term/words.rs`), `crates/theseus-protocol/src/jsonrpc.rs` | `clip_at_most_n_counts_the_mark` (revert: return N+1), each site's existing tests; JSON goldens for the mirrors before the swap | L |

## The sub-steps

### 1. Delete the 37 pub items nothing calls (§1; ~215 lines)
The census: a pub item whose name appears exactly once in the whole workspace (string mentions such as serde
`default = "…"` counted). Re-run it on your clone (`rg -w <name> crates cockpit/src` = 1 hit) before each delete. The
list, at `c6f62abf`: `crates/theseus-sim/src/fake_discord/events.rs:83 add_foreign_event`, `theseus-lsp/src/nav.rs:115
prepare_rename`, `theseus-aws-catalog/src/compile.rs:863 compile_fixtures`, `theseus-sim/src/fake_discord/threads.rs:164
set_forum_tags`, `theseus-lsp/src/client.rs:836 set_settings`, `theseus-core/src/learning/propose.rs:1146
learn_proposals` (fix its doc's claim that the cockpit reads it: the cockpit talks RPC and no method calls it),
`theseus-judge/src/prove.rs:717 arm_counts`, `theseus-aws/src/client.rs:182 with_catalog`,
`theseus-core/src/rsi/sealed/daemon.rs:45 sealed_score`, `theseus-core/src/project/mod.rs:74 open_projects`,
`theseus-mcp/src/server.rs:1050 set_turn`, `theseus-kernel/src/children.rs:365 tender_of`,
`theseus-core/src/compiler/effort.rs:80 without_messages`, `theseus-aws-catalog/src/compile.rs:526 compile_service`,
`theseus/src/render.rs:509 fallback_line`, `theseus-kernel/src/job.rs:916 serving_daemon`,
`theseus-core/src/task_graph/mod.rs:367 open_count`, `theseus-core/src/routine/watch.rs:42 is_trouble`,
`theseus-core/src/push.rs:513 work_root_of_session`, `theseus-core/src/outbox.rs:1026 is_budget`, in
`theseus-aws-catalog/src/model.rs` `is_host_label` (1374), `is_wrapper` (1194), `shape_by_name` (1084),
`request_compression` (1504), `is_endpoint_operation` (1500), `is_eventpayload` (1391), `requires_length` (1189),
`is_fault` (1181), `is_exception` (1177), `is_event` (1165), `theseus-aws-catalog/src/lib.rs:124 compressed_len`,
`theseus-core/src/policy/modes.rs:61 PICK_EDITS`, `theseus-aws/src/creds.rs:38 has_session_token`,
`theseus-lsp/src/jsonrpc.rs:20 SERVER_NOT_INITIALIZED`, `theseus-core/src/learning/mod.rs:50 HOLDOUT_DAYS`,
`theseus-lsp/src/types.rs:88-89 INFORMATION, HINT`.
- **Skip** any item in `crates/theseus-discord/` or `fake_discord/` if a Discord row on your base now calls it (contract
  7), and `sealed_score` only goes because it has no caller: `sealed.rs`'s read half otherwise stays (the sealed holdout
  question is the owner's). `serving_daemon` goes alone: `daemon_in_cmdline`, `DAEMON_VALUE_FLAGS` and `pid_alive` have
  tests and stay. LSP severities: keep `INFORMATION`/`HINT` if deleting them leaves an incomplete set that reads oddly;
  say which you chose.
- Proof: one `rg -w` hit each, before; the workspace builds and clippy is clean after. No test is deleted.

### 2. One `clip` (§13; ~150-180 lines, fixes a bug)
`rg 'fn (clip|cut|truncate|ellipsize|shorten|trim_to)\b'` outside tests finds **54 functions** in 12 crates (CLI 11, core
18, Discord 8, protocol 4, judge 3, kernel 3, sim 2, one each in files, LSP, MCP, TUI, voice). Three meanings are in use:
(A) total length including the `…` (the shared one, `crates/theseus-protocol/src/term/words.rs:9`, which no crate outside
protocol calls); (B) N characters then `…` (N+1 in all); (C) N characters, no mark.
- **The bug:** `crates/theseus-core/src/webui.rs:268-280` says "at most 200 characters" and returns 201; the review
  found five core sites like it. Each site whose doc or name says "at most N" moves to meaning (A) and gets a test that
  its output is ≤ N characters with the mark (`clip_at_most_n_counts_the_mark`; plant: return N+1).
- **Shape:** two public functions in protocol, `text::clip(s, n)` (A) and `text::cut(s, n)` (C, no mark), or keep the
  existing `term::words` home and add the second there: pick the one that needs fewer new lines. Sites that mean (B) and
  feed a golden or user-visible output: keep their exact output, either with (A) at `n + 1` or with a third function;
  pin the output in a test first. The kernel's three byte-identical `chars().take(200).collect()`
  (`crates/theseus-kernel/src/wake_owner.rs:463`, `watch.rs:227`, `wakes.rs:645`) become one call.
- Leave Discord's 8 copies (contract 7). LSP's and MCP's `clip` move with sub-step 5.
- Proof: each site's existing tests; the CLI's `tests/golden.rs` unchanged except where a site was the bug (then say
  which lines and why).

### 3. One `now_ms` (§18; ~60 lines)
16 hand-written `SystemTime::now()` → milliseconds functions: `theseus-protocol/src/lib.rs:2726` and
`theseus-store/src/record.rs:131` (public, identical), copies in kernel `job.rs:1482`, core `lsp/mod.rs:437`,
`node_cache.rs:273`, `rpc/ontology.rs:453`, index `tender.rs:878`, `vectors.rs:1104`, CLI `work.rs:51`, `herdr.rs:85`
(**skip**: herdr lane, contract 7), `status.rs:1197`, TUI `main.rs:291`, sim `fake_discord.rs:1969` (skip: Discord), …
Each crate that already depends on theseus-protocol calls `theseus_protocol::now_ms` (core already does, 467 calls);
theseus-store keeps its own if it does not depend on protocol (check `Cargo.toml`; add no dependency edge). **Leave the
kernel's `Clock` alone** (the injectable clock is a separate question): only replace kernel copies that are plain wall
clock reads outside `Clock`. Proof: same value (wall-clock ms); existing tests.

### 4. Civil dates through jiff (§10; ~120-150 lines)
Howard Hinnant's days↔civil algorithm (`719_468` / `146_097`) is written out in 11 files: `theseus-core/src/wake.rs:863,976`
(`days_from_civil`, `civil_from_days`), `core/src/github.rs:67`, `core/src/aws/hands/start.rs:222` (`utc`),
`core/src/import/catalog.rs:644` (`month_of`), `core/src/threads_import/fixture.rs:38` (`rfc3339`),
`core/src/learning/backfill.rs:59` (`day_start`), `theseus-protocol/src/why.rs:637` (`ymd`), `theseus-tools/src/git.rs:638`
(`fmt_date`), `theseus-exam/src/time.rs:10,21`, `theseusd/src/install/mod.rs:245` (`utc_stamp`), and Discord's
`horizon_cmds.rs:198` (skip: contract 7). The workspace already links `jiff` 0.2 (`crates/theseus-kernel/Cargo.toml:23`,
`crates/theseus/Cargo.toml:36`); theseus-core depends on theseus-kernel, so core, theseusd and the CLI can use jiff with
no new crate in the lock (a crate that does not list jiff yet adds it as a `jiff.workspace = true`-style line only if the
workspace already pins it; Cargo.lock must gain no package).
- protocol (serde-only dependencies) and tools keep one copy each, or protocol gains a tiny `civil` module that tools and
  exam call: pick the smaller.
- **Pin first:** each copy's output is a fixed format (RFC 3339, `YYYY-MM-DD`, `Mon 17 Oct`). Before swapping a copy,
  make sure a test pins its output at a few dates including a leap day and a year boundary (wake.rs, github.rs and exam's
  time.rs have tests at the functions; add a pin where none exists). Month-name tables that repeat go with them.

### 5. One JSON-RPC framing module (§11; ~145 lines)
`crates/theseus-lsp/src/jsonrpc.rs` (208 lines) and `crates/theseus-mcp/src/jsonrpc.rs` (224) export the same eleven
items: `JSONRPC`, `RpcError`, `Incoming`, `classify`, `request`, `notification`, `response`, `error_response`, `id_number`,
`clip` (LSP `:10-142`, MCP `:13-148`). The one real difference: MCP's `classify` returns a `Vec` (it accepts batches).
`theseus-protocol` already defines `Request`, `Response`, `Id` and `RpcError {code, message, data}`
(`crates/theseus-protocol/src/lib.rs:505-530`). Make one `theseus_protocol::jsonrpc` module (a new file;
**`lib.rs` gains only its `pub mod` line**: it is at 2,732 of 2,733, so if even that line does not fit, take a line from
lib.rs by moving one small existing item into the new module, never raise the ceiling) with the builders and both
`classify` (LSP) and `classify_batch` (MCP); LSP and MCP depend on protocol (check they don't already; it is a workspace
crate, so no lock change) and re-export what their callers use, so call sites barely change. Optionally the CLI's
`client.rs:18 CallError` and Discord's (skip Discord) wrap `theseus_protocol::RpcError`, keeping their own `Display`.
Proof: byte-identical frames (a test that the shared builders produce exactly the bytes the old LSP and MCP builders did,
for a request, a notification, a response, an error and a batch); LSP's and MCP's own tests (`theseus-mcp/tests/client.rs`,
the LSP client tests) unchanged.

### 6. Core types that mirror protocol wire types (§5; ~120-160 lines)
| Core type | Protocol type | Hand copy |
|---|---|---|
| `Census` `crates/theseus-core/src/provider.rs:176` (6 fields) | `CensusSummary` `protocol/src/events.rs:138` | `core/src/compiler.rs:371-378` |
| `Estimate` `crates/theseus-core/src/compiler.rs:335` (7) | `EstimateSummary` `protocol/src/events.rs:114` | `compiler.rs:358-380` (`method` enum → string) |
| `Said` `crates/theseus-core/src/own_bench/rebuild.rs:26` (6) | `ReplayReply` `protocol/src/own_bench.rs:40` | `own_bench/replay.rs:77` `reply_of` |
| `TagCounts` `crates/theseus-core/src/import/mod.rs:278` (8) | `ImportTagInfo` `protocol/src/import.rs:319` | `import/write.rs:758` |
| `When` `crates/theseus-core/src/recall/when.rs:29` (3) | `RecallWhen` `protocol/src/memory.rs:193` | `when.rs:42-50` `From` |

Let core use the protocol type (or a `pub use` alias) where the two must stay identical. Keep a core type only where it
carries behaviour the wire must not; an enum the wire spells as a string can become a serde enum on the protocol side
only if the JSON is byte-identical (same snake_case word) **and** the generated TypeScript in `cockpit/src/protocol.gen/`
stays the same or the cockpit still type-checks (`npm run build` in cockpit/; regenerate per AGENTS.md). `Census`
derives `Copy`, which `CensusSummary` would need to gain. **Pin with a golden first:** before each swap, a test
serialises a sample of the core type to JSON; after the swap the same sample of the protocol type gives the same bytes.
If `Census` is stored anywhere, the stored JSON must not change. Not in scope: `theseus-ontology` and `theseus-judge`
mirrors (no protocol dependency on purpose), `PhaseRow` in sim (optional; only if trivial).

**Before calling any wire method or type unused, search the cockpit's TypeScript too** (`rg -n "<name>" cockpit/src`,
single- and double-quoted method strings): the review's first census wrongly found 11 uncalled methods because it missed
the cockpit.

## Proof nothing is lost (report section)
Per sub-step: the census or grep output before; the pinning tests (clip bound, date formats, JSON bytes, frame bytes);
the CLI goldens unchanged (or each changed line explained); lines removed per sub-step (`git diff --stat`).

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
