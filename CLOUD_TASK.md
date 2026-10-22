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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail6-threads`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: your base for this wave (it wins over anything below about `main`)

Your branch starts from **`integ/tail5` at `33e589e2`**, not from `main`. The code your row fixes exists only there: the
lane cut `integ/tail5` from `main` `49352e8a` (2026-10-22: release stage 4, install #72) and merged five rows on it:
theseus-fauy3 (the markdown render's CPU clock), tail5-threads (`b1cd38e9`), tail5-effort (+ join fix `9b37c091`),
tail5-t4fix (`474a9b56`) and tail5-discord (`77709f89`), then `33e589e2` (its `docs/status.md` entry).
`git log --first-parent origin/main..33e589e2` lists them. Line numbers quoted below are at `33e589e2` but may still be
off by a few: search by name. Run the keel guard against `33e589e2`, not `origin/main`:
`python3 scripts/keel-guard.py --base 33e589e2 --head HEAD --quiet`.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Never write the number as a literal anywhere but the constant and its tests. Prefer serde-default
fields: most rows need no bump.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,500 at most (tail5-discord used its room: add nothing)
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (add at most two lines: the release branch counts other lanes' too)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you:** `integ/tail5` is gating now and lands as the next release stage; your branch joins after
it (its last round, or the next lane: the integrator's call). Rows on `main` `49352e8a` in this wave (tail6-cn,
tail6-onto, tail6-tests) and the row beside you on `33e589e2` are named in your task's "Rules for this row". Stay out of
their files.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail6-threads, tail5-threads' follow-ups: the growth race, the held row's timing, confirms that name their turn (theseus-hat0c, theseus-m1a51, theseus-fkvb0)

Branch: `cloud/20261021-tail6-threads`. Every commit's subject carries the Beads id of what it changes (`theseus-hat0c`,
`theseus-m1a51`, `theseus-fkvb0`). Deadline for the report: 3 hours after you start.

**Why.** tail5-threads (theseus-ddfe7, theseus-qiyxq) merged on `integ/tail5` as `b1cd38e9` (the lane's tip is now `33e589e2`, with all four tail5 rows). Its review left three
follow-ups the owner wants in before the release closes:
- **theseus-hat0c (F1, a race).** `rpc/thread_grow.rs` checks `is_ungrown(turn_id)` once, at the start of `thread_grow`
  (near :79). A move that lands between that check and `route.grew(...)` marks the turn, yet the growth still commits
  into the origin thread's working set and its `grew` entry stays in the ring.
- **theseus-m1a51 (F2/F3).** `turn/route_step.rs` `fn held` (near :678) records the held compile failure's
  `route.decided` row with `Timed::default()` and steering `None`, so the row shows waited 0, late false, no
  `answered_ms`, and a steered turn that fails to compile records no correction source. The real values are in scope at
  the `Err(f)` call site. And `held` runs on every compile failure, not only held ones: its doc comment says "held turns".
- **theseus-fkvb0 (ddfe7's own proposal).** `confirm.list` (and a push's `PendingConfirm`) do not say which turn asks,
  so the TUI's placed-thread filter (`ours_in`, theseus-dos5f) cannot keep a placed thread's other turns' confirms out
  of this chat after a reconnect; the waiting budget question (`BudgetQuestionInfo`) has the same gap. Plus a doc nit:
  core `AGENTS.md` near :1592 "`ContextDetour` carry `recall`" should read "carries".

**Risk: low.** Bookkeeping, one serde-default wire field, a TUI filter.

**Prerequisite: base `origin/integ/tail5` at `33e589e2`** (the code is only there; see the note above). Your branch
joins after tail5: `integ/tail5`'s last round, or `integ/tail6` once `integ/tail5` lands (the integrator's call).
Run the keel guard against `33e589e2`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail6-threads` | theseus-hat0c, theseus-m1a51, theseus-fkvb0 | re-check the move inside the growth's update; the held row's real timing and steering, and its doc; `turn_id` on a confirm and on the budget question; the TUI filters by it; one AGENTS.md word | core `rpc/thread_grow.rs`, `thread_flow.rs` (only if the check needs a helper), `rpc/tests_thread_grow.rs`, `turn/route_step.rs` (`fn held` and its one caller's arguments only), `tests_hold_compile.rs`, `rpc/confirms.rs` and `rpc/answer.rs` (fill `turn_id`), protocol `push.rs` (`PendingConfirm`), `budgets.rs` (`BudgetQuestionInfo`), the TUI's `chat/thread_ask.rs` (`ours_in`'s callers for confirms), `chat/tests_thread_ask.rs`; cockpit `src/protocol.gen/*` regenerated; core `AGENTS.md` (the one word) | M |

## What exists (at `33e589e2`; search by name)
- `crates/theseus-core/src/rpc/thread_grow.rs` near :79 `if self.runner.threads.route.is_ungrown(turn_id)`, then the
  store update and `route.grew(turn_id, &t.slug, added)`. `crates/theseus-core/src/thread_flow.rs` near :161
  `pub fn is_ungrown`, `take_grew`, the `ungrown` ring capped at `TURNS`. `rpc/tests_thread_grow.rs` drives growth and
  moves (`a_turn_moved_before_its_growth_grows_nothing_in_the_origin` is tail5-threads' key test).
- `crates/theseus-core/src/turn/route_step.rs` near :678 `fn held(...)` calling
  `record_route(t, verdict, None, reason, Default::default(), None)`; its caller at the compile's `Err(f)`.
  `crates/theseus-core/src/tests_hold_compile.rs` (`a_held_compile_failure_records_its_route`).
- Confirms: `crates/theseus-protocol/src/lib.rs` near :2256 `ConfirmRequest` (it has `session_id` and `turn_id`) (the `confirm.list` item; `CONFIRM_LIST`
  near :230, `ConfirmListResult` near :2343); `crates/theseus-protocol/src/push.rs` near :91 `PendingConfirm`;
  `crates/theseus-protocol/src/budgets.rs` near :109 `BudgetQuestionInfo`. Core: `rpc/confirms.rs` near :232 and :260
  (`confirm_list`), `rpc/answer.rs` near :158 `confirm_list_for`, `rpc/methods.rs` near :543 and :677.
- TUI: `crates/theseus-tui/src/chat/thread_ask.rs` near :154 `ours_in(session_id, turn_id)`;
  `chat/tests_thread_ask.rs` (`a_placed_threads_other_turn_does_not_ask_in_this_chat`).

## What to build
1. **hat0c.** Check `is_ungrown` again inside the store's update closure, before it returns the new set: a turn marked
   by then grows nothing and pushes no `grew`. Keep the early check (it saves the work).
2. **m1a51.** `held` takes the turn's real `Timed` (waited, late, `answered_ms`) and its steering from the call site and
   passes them to `record_route`; a steered turn whose compile fails records its correction source as a normal routed
   turn would. Fix the doc: it runs on every compile failure, and holds only where the verdict holds.
3. **fkvb0.** `ConfirmRequest` already has `turn_id: Option<String>`, but `rpc/confirms.rs` builds it with
   `turn_id: None` (near :88 and :128): fill it where the core knows the asking turn. `PendingConfirm` and
   `BudgetQuestionInfo` gain the same field (`#[serde(default, skip_serializing_if = "Option::is_none")]`). The TUI
   keeps a confirm or budget question out of this chat when its `turn_id` names another turn of a placed thread
   (`ours_in`), on `confirm.list` after a reconnect as well as on the push; `None` still shows (an older daemon, a
   background worker). Regenerate the cockpit's `protocol.gen`.
4. **AGENTS nit.** "carry" → "carries" near :1592.

## Tests (each with a planted revert; the first is the key test)
- **`a_move_during_growth_commits_nothing_to_the_origin`**: the rig lands a move between the early check and the update
  (a hook, or the closure seam you add); the origin's set is unchanged and the ring holds no `grew` for the turn. Plant:
  drop the inner check.
- `a_held_compile_failure_records_its_wait_and_steer` (waited, `answered_ms` and the correction source on the row).
  Plant: pass `Default::default()` again.
- `confirm_list_carries_the_asking_turn` (core). Plant: leave `turn_id` `None`.
- The TUI case: after a reconnect, `confirm.list` with another placed turn's `turn_id` does not ask in this chat; one
  with `None` does. Plant: drop the `turn_id` test from the filter.
- The protocol round trip: an old payload without `turn_id` still parses.
- `cargo nextest run -p theseus-core thread`, `... hold_compile`, `... confirm`, `-p theseus-protocol`, `-p theseus-tui
  thread_ask` pass, then the three crates whole.

## Not in this row
- Thread routing's scores, `thread_route.rs`, `thread_title.rs` (tail5-t4fix edits `thread_title.rs`).
- The Discord approval card and DM task messages: the owner locked them. Discord ignores the new field.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Line count.** `crates/theseus-protocol/src/lib.rs` is at 2,563 / 2,733: `ConfirmRequest` needs no new field; edit
  nothing in `lib.rs` unless its doc for `turn_id` is wrong (one line).
- **A row beside you on `integ/tail5` 33e589e2:** tail6-t5fix edits `turn/effort_step.rs`, `routing/effort.rs` and
  Discord only. In `route_step.rs` you edit only `held` and the arguments of its one call (tail5-effort's
  `effort::decide` call sits in the same file: leave it).
- **Rows on `main` 49352e8a beside you (lane `integ/tail6`):** tail6-cn edits `route_step.rs`'s data-gate lines near
  :860 only, `turn/data_step.rs` and the data gate; tail6-onto the ontology; tail6-tests `bench/recall` and `scratch.rs`.
  Stay out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base 33e589e2 --head HEAD --quiet` and
  paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; the new field is on the wire only, not stored.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-core/AGENTS.md` (threads, routing, confirms), `crates/theseus-protocol/AGENTS.md` and
  `crates/theseus-tui/AGENTS.md` first. Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; the race seam you used; which timing and steering values the held row now
carries; where `turn_id` is filled and where it stays `None`; each test and its plant with the failing line; the keel
guard's output; `lib.rs`'s line count; a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
