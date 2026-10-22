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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail3-cn`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

This task was written against an older `main`. Your branch starts from the integration branch `integ/cn` at `2d30cc48` (2026-10-22: `main` `88f2e48f` plus the China-origin rows of rounds 1-4, round 4 green); that branch, not `main`, is your base. Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,119 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- The rows beside you in this wave, and the files each owns, are named in your task's own "Prerequisite", "Rules for
  this row" and "Not in this row" sections below. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail3-cn, an open request's task tools reach only its own record, the fetch screen sees the final URL and leaves a row, the first-loop gate checks the session is open, and two small cleanups (theseus-ol8w0, theseus-4aqky, theseus-lm38z, theseus-pohih, theseus-dyx7o)

Branch: `cloud/20261021-tail3-cn`. Every commit's subject carries the Beads id of what it changes. Deadline for the
report: 3 hours after you start.

**Why.** Follow-ups of the China-origin rows merged into `integ/cn` rounds 3 and 4.
- **theseus-ol8w0 (and its duplicate theseus-4aqky; review M2 of tail2-cn).** `open_view::open_public` offers an open
  request `task.update` and `task.close`, meant for "a task's own record". But both act on **any** task by id + version
  (CAS): a restricted model that learns another task's id and version may retitle, re-own or close it. That is a reach,
  not a data leak, but it is not what the allowlist's doc says.
- **theseus-lm38z (cn-tool-screen review).** (1) `http.fetch`'s screen (`data::screen`) checks only `plan.url`: a
  fetch of a public URL that **redirects** into the owner's network passes when `[policy] private_addresses = open`.
  The result's meta already carries `final_url`; screen it too. (2) The screen's failure word reaches only `tracing`,
  never a stored row. (3) Optional: `screen_loop` keeps walking after the first failure.
- **theseus-pohih item 1 (cn-bg-rest review).** The first-loop compaction gate (`turn/compaction/gate.rs` `open_run`)
  reads the session's stored run before the turn's admission is read, and reads the session record before checking
  that the session is open at all (`t.tc.open`): check `t.tc.open` first. Item 2 (a turn-level laundering test) was
  accepted by the review as not distinguishable on this base (an uncleared range always holds a non-loop node past
  `run.through`, which stops `data_end` by itself): do not write it; say so, so the issue can close.
- **theseus-dyx7o (cn-private-authored review, low).** `turn/standing_step.rs` adds `pub(crate) fn restricted()`, a
  one-line alias of `restricted_profile` (kept only for `clippy::too_many_lines` in `keep_warm_request`): make
  `restricted_profile` `pub(crate)` and drop the alias. And no test exercises `google::private::authored` with
  `[google]` set and a `capture:` node the store cannot read (it relies on `any_node` failing closed): add one asserting
  the node's id is held.

**Risk: low.** Each change narrows what a restricted model's request may do or adds a test; none widens a reach.

**Prerequisite: base `origin/integ/cn` at `2d30cc48`** (round 4 green: the tool screen, observability, background rest,
draft review, private-authored and tail2-cn rows), not `main`. Your branch joins `integ/cn` round 5. Run the keel
guard against `origin/integ/cn`. **Another row is running from this same base now: cn-config-guidance (theseus-a1wvq.12).
Do not touch its files**: `crates/theseus-core/config/theseus.example.toml` (its `[routing.*]` and `[profiles.*]`
blocks, and in practice the whole file), `crates/theseus-core/src/own_bench/` (`drafts.rs`, `report.rs`), protocol
`src/own_bench.rs`, `crates/theseus/src/render/own_bench.rs`, the China-origin and own-bench passages of
`crates/theseus-core/AGENTS.md` ("Invariants"), `docs/privacy.md`, and the cn lines of `docs/status.md`. This row
writes **no** docs file: put any sentence for AGENTS.md or status.md in your report for the integrator.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail3-cn` | theseus-ol8w0, theseus-4aqky, theseus-lm38z, theseus-pohih (item 1), theseus-dyx7o | open request's `task.update`/`task.close` own record only; fetch screen on the final URL; a stored screen word; `open_run` checks open first; drop the alias; one private-authored test | `crates/theseus-core/src/open_view.rs`, where an open request's tool calls are refused at run time (find it: the gate or toolrun path that applies `open_public`), `task_graph/tools.rs` (a helper beside `is_own_close`, if shared), `data.rs` (`screen`), `turn/screen_step.rs`, `turn/compaction/gate.rs` (`open_run` only), `turn/standing_step.rs`, `turn/data_step.rs` (visibility only), `keep_warm.rs`, `open_view.rs` and `prefix/turn.rs` (the alias's three callers, one word each); tests: `tests_open_request.rs`, `tests_tool_screen.rs`, `tests_bg_gate.rs`, `tests_private_authored.rs` (or a new test file with one `lib.rs` mod line) | S-M |

## What exists (lines at `2d30cc48`; search by name, a line may have moved)
- **The allowlist.** `crates/theseus-core/src/open_view.rs` (295 lines) :60-76 `open_public(name)`: `web.search`,
  `http.fetch`, `tools.find`, `task_graph::tools::UPDATE`, `CLOSE`; its doc says "act on a task's own record". Find
  where `open_public` is applied (`git grep -n open_public crates`) and where a call's input is checked at run time.
- **Own record.** `crates/theseus-core/src/task_graph/tools.rs` :209 `is_own_close(tool, input, session)`: `tool ==
  CLOSE` and `input.id` (trimmed) equals `super::of_session(session)` or is its tail of ≥ 6 chars; tests in
  `tests_task_close.rs` :52-60. An open request runs as a task (open sessions are tasks only), so "its own record" is
  `of_session(session)`.
- **The screen.** `crates/theseus-core/src/data.rs` :320 `screen(tool, plan, redactions, open) -> Result<(),
  Unscreened>`; `Unscreened::{Program, Tool, Secret, Outside}` with `as_str`; the `http.fetch` arm checks
  `plan.url` is public. `turn/screen_step.rs` (81 lines) `screen_loop`: walks the turn's newest tool-result nodes, plans
  each call again, calls `data::screen`, pushes `t.route.screened` or keeps the earliest failure, then
  `tracing::info!(…, word = why.as_str(), "a tool result failed the screen")` and, for an open session, the taint
  (`DataWhy::ToolResult`, `node`). The result node's `meta` holds `redactions`; `web/fetch.rs` :248 writes
  `"final_url": got.url.as_str()` and :254 `"redirects"` into the fetch result. Tests: `tests_tool_screen.rs`
  (`screen_matrix` :332 and the turn-level tests).
- **The gate.** `crates/theseus-core/src/turn/compaction/gate.rs` (272 lines) :87 `fn open_run(t)`: `Ok(run)` → the
  turn's run; `Err(DataWhy::Unchecked)` → `get_session(..)`, its `data`, the stored run unless tainted; else `None`.
  Tests: `tests_bg_gate.rs` (`a_later_loop_compacts_over_this_turns_screened_results`, the T23 cases).
- **The alias.** `turn/standing_step.rs` :64-68 `pub(crate) fn restricted(&self, profile) -> bool {
  self.restricted_profile(profile) }`; `turn/data_step.rs` :94 `pub(super) fn restricted_profile`. Callers of the
  alias: `keep_warm.rs` near :420, `open_view.rs` near :247, `prefix/turn.rs` near :182 (`catalog.rs`'s
  `ModelOrigin::restricted` is a different fn: leave it).
- **Authored.** `crates/theseus-core/src/google/private.rs` :144 `any_node`, :162 `authored`; tests in
  `tests_private_authored.rs`.

## What to build
1. **theseus-ol8w0 / 4aqky.** In an open request, `task.update` and `task.close` act only on the open task's own record:
   a call naming another task is refused at plan or gate time with a plain sentence (the open request may change only
   its own task), before any write. One helper beside `is_own_close` (e.g. `is_own_record(input, session)` used by
   both) rather than a second copy of the id rule. Update `open_public`'s doc to say "its own record, by id". A shared
   place and every non-open session keep today's behaviour.
2. **theseus-lm38z.**
   - (1) `http.fetch`'s result also fails the screen when its `meta.final_url` is not public (same test as `plan.url`,
     same `Outside` word). Pass what `screen` needs from the node (its meta) without widening its other callers.
   - (2) The failure word is stored: on the failed result node's meta (`screen: <word>`) or one ledger row of an
     existing kind if one fits (search the data-gate rows `record_data_gate` before adding anything). No new ledger
     kind unless nothing fits; if you add one, say so.
   - (3) Stop walking after the first failure only if it keeps "the earliest failure" (the walk is newest first, so it
     probably cannot): otherwise leave it and say why.
3. **theseus-pohih item 1.** `open_run` returns `None` when `t.tc.open` is `None` before reading any record; the
   stored-run path is unchanged otherwise.
4. **theseus-dyx7o.** `restricted_profile` becomes `pub(crate)`; the three callers call it; the alias goes. If that
   pushes `keep_warm_request` over clippy's `too_many_lines`, split a helper out of it rather than adding an allow.
   Then the test in step 5.

## Tests (each with a planted revert; the first is the key test)
- **`an_open_request_closes_only_its_own_task`** (`tests_open_request.rs`): an open request's model calls `task.update`
  then `task.close` on **another** task's id and version: both refused, the other task unchanged; the same calls on
  its own record pass. Plant: drop the own-record check: the other task is retitled or closed. Paste the failing line.
- `a_fetch_that_redirects_into_the_owners_network_fails_the_screen` (`tests_tool_screen.rs`, `private_addresses =
  open`, a fake server whose public URL redirects to a loopback/private one): the result is not screened and the next
  loop's clearance refuses it. Plant: screen `plan.url` only.
- `a_failed_screen_leaves_its_word_in_the_store`: the word you store is readable after the turn. Plant: drop the write.
- `the_first_loop_gate_reads_no_record_for_a_closed_session` (pure or `tests_bg_gate.rs`). Plant: read the record first
  (show the observable difference you test: a record read counted, or a stored run used).
- `authored_holds_an_unreadable_capture_node` (`tests_private_authored.rs`): `[google]` set, a `capture:` node the store
  cannot read: its id is held. Plant: make `any_node` fail open for that case.
- dyx7o's alias removal is a refactor: no plant; `cargo clippy` clean.
- `cargo nextest run -p theseus-core tests_open_request tests_tool_screen tests_bg_gate tests_private_authored
  tests_task_close tests_cn_follow` must pass.

## Not in this row
- theseus-4h85l (cn-observability follow-ups) and theseus-b0ozi (draft-review follow-ups): later rows.
- theseus-iilre (the draft reviewer's programs ask every time): the owner's call.
- The China-origin chain itself (theseus-a1wvq.12 in flight; .15/.16/.17 later) and every file of cn-config-guidance
  listed under Prerequisite.
- Any docs file. Any `Cargo.toml`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- No new always-loaded model text and no new tool. The refusal sentence is a tool result, not prompt text.
- **Line ceilings** (`scripts/long-files.txt`): check each touched file against it (`turn.rs` and protocol `lib.rs` are
  listed: you should need neither). Never raise a ceiling.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD
  --quiet` and paste its output. It must be empty.
- **Dependencies.** None new. **Store format** on your base is 48. A node's meta key is not a layout change; if you add
  a field to a stored record, bump to 49 with the old layout's sample in `crates/theseus-core/src/tests_layouts.rs`.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/integ/cn`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture hosts are invented (`example.test`,
  `127.0.0.1` for the fake server).
- Read `crates/theseus-core/AGENTS.md` (the data rule, open requests, the tool screen) and `docs/privacy.md` first
  (read only). Run `cargo nextest run -p theseus-core`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit. L1 tests need a non-root daemon and fail on this VM; say which tests you
  ran.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; where the own-record check sits and its sentence; where the screen word is
stored; pohih item 2's status (not testable at turn level on this base, per the review); each test and its plant with
the failing line; AGENTS.md/status.md sentences for the integrator; the keel guard's output; the store format;
follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
