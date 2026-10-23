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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7-threads`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ae420a49` (2026-10-22: release stage 6, install #74, which landed `integ/tail6` on top
of stage 5's `f6f2900e`); that is your base. Line numbers quoted below are at `ae420a49` but may still be off by a few:
search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f6f2900e`** (`git log --first-parent f6f2900e..HEAD` lists it):
- `ae420a49`: `integ/tail6` (the paced stand-in model; the ontology sweep's titles off the worker, its pair rule, kept
  kind names and the cockpit's renames; the thread growth race, the held row's timing, confirms and the budget question
  naming their turn; the data-gate word only when the gate decided, its counters; the task home's restart test and
  `sticky_unsure`; the recall bench's later-session plan and read-only test trees).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/turn.rs` 3,467 / 3,525
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you:** nothing else is integrating. The rows beside you in this wave all base on `ae420a49` and
join the new lane `integ/tail7`; each owns the files named in its task's "Rules for this row" and "Not in this row"
sections. Those win over anything older. Stay out of the other rows' files.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7-threads, a steered held-compile row test, the sidebar's confirm list and placed threads, and one shared ended-state list in the TUI (theseus-7zs0f, theseus-bibny)

Branch: `cloud/20261021-tail7-threads`. Every commit's subject carries its Beads id (`theseus-7zs0f` or
`theseus-bibny`). Deadline for the report: 3 hours after you start.

**Why.** Release stage 6 (`ae420a49`, now on `main`) landed tail6-threads: `held` records a failed compile's real
`Timed` and steering (theseus-m1a51), and `confirm.list`, the push and `budget.list` name the asking turn
(`turn_id`, theseus-fkvb0). Its review, and tail5-t4fix's, left:
- **7zs0f item 1.** `crates/theseus-core/src/tests_hold_compile.rs` `a_held_compile_failure_records_its_wait_and_steer`
  (near :185) checks only the wait (`wait_ms`, `late`, `answered_ms`); nothing tests the steering half, though `held`
  passes `steering.as_ref()` to `record_route` (`turn/route_step.rs`). The author said the rig cannot stage a steered
  turn whose compile overflows.
- **7zs0f item 2.** The TUI's sidebar (the app, `crates/theseus-tui/src/app.rs`, `Purpose::Questions` →
  `method::CONFIRM_LIST` near :994 and :1056) now gets `turn_id` on each `confirm.list` row, but applies no
  placed-thread filter. The chat already filters pushed confirms with `ours_in(session_id, turn_id)`
  (`chat/thread_ask.rs` near :154, used from `chat/events.rs`).
- **bibny.** The ended-state list `"complete" | "cancelled" | "failed" | "budget_exhausted"` is written twice: `app.rs`
  near :1362 and `chat/task_picker.rs` `ended` near :74. And core `AGENTS.md` has a ragged re-wrap in the thread
  router's paragraph (near :583, "`rpc/thread_flow.rs` (`route_to_thread`, called from `turn_submit` with `route` true)
  runs in this order:" is one over-long line).

**Risk: low.** A test, a display filter (or a documented no-op), and a dedupe.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the new lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7-threads` | theseus-7zs0f (1-2), theseus-bibny | a steered held-compile test (or an honest rename); the sidebar's confirm list and placed threads; one ended-state helper; the AGENTS.md wrap | core `tests_hold_compile.rs` (and only a test seam in `turn/route_step.rs` if the steered case truly needs one); TUI `app.rs`, `chat/task_picker.rs`, the place the shared helper lives (a TUI module both reach, or `theseus-protocol`'s task types only if a helper there is one line: prefer the TUI), the TUI tests beside them; core `AGENTS.md` (the thread router paragraph: re-wrap only) | M |

## What exists (at `ae420a49`; search by name)
- `crates/theseus-core/src/tests_hold_compile.rs`: `rigged`, `rigged_with`, `run`, `rows`, `said`; the tests
  `a_held_turn_whose_compile_fails_is_still_held`, `a_held_compile_failure_records_its_route`,
  `a_held_compile_failure_records_its_wait_and_steer` (near :185).
- How a steered turn is staged elsewhere: search the core tests for `steer` / `Steering` (the routed path's tests,
  e.g. `tests_route_*` or `tests_steer*`) and reuse their rig if it can be combined with this file's overflow setup.
- `crates/theseus-core/src/turn/route_step.rs`: `compile_first`, `Timed::of`, `held`, `record_route`.
- TUI: `app.rs` (`Purpose::Questions`, the sidebar's question list and how it is drawn; the ended check near :1355-1364),
  `chat/thread_ask.rs` (`ours_in`, `placed.also`, `mine`), `chat/events.rs` (the `ConfirmRequested` arm),
  `chat/task_picker.rs` (`ended`), `crates/theseus-tui/AGENTS.md`.

## What to build
1. **The steered case (7zs0f 1).** Add `a_held_compile_failure_records_its_steer`: a steered turn (an owner steer, or
   whatever `steering` is in `route_step.rs`) whose compile fails and is held; its `route.decided` row carries the
   steering fields the routed path writes. If the rig truly cannot stage it after a real try, rename the existing test
   to `a_held_compile_failure_records_its_wait` instead, and say in the report what stopped you. A rename alone is the
   fallback, not the plan.
2. **The sidebar (7zs0f 2).** First find out what the sidebar's question list is for. If it is the whole daemon's
   questions (every session's, for an owner watching everything), a placed-thread filter would hide questions the owner
   must answer: then change no code, add a test that pins that a question of another turn in a placed session still
   shows in the sidebar, and say why in the report. If it is scoped to this terminal's chat, filter its rows with the
   chat's own `ours_in` rule (one rule, not a copy), with a test. Either way the report says which, with the evidence
   (the code that draws it, its doc, AGENTS.md).
3. **One ended list (bibny).** One helper, for example `fn ended_state(state: &str) -> bool`, used by both `app.rs` and
   `task_picker.rs`. If `WorkState::ended()` (the `w.state.ended()` near :1358) already holds the same four words for a
   different type, say whether one can call the other; do not widen either list.
4. **The wrap.** Re-wrap the over-long thread-router line in core `AGENTS.md` to the paragraph's width. No word changes.

## Tests (each with a planted revert; the first is the key test)
- **`a_held_compile_failure_records_its_steer`**. Plant: make `held` pass `None` for the steering to `record_route`.
- The sidebar test of step 2 (whichever way it goes). Plant: the opposite rule (filter, or no filter).
- The ended helper: an existing test that goes through each call site, or a new one per site, so a word dropped from
  the helper fails both. Plant: drop `budget_exhausted` from the helper.
- `cargo nextest run -p theseus-core hold_compile`, `-p theseus-tui`, then `-p theseus-core` whole.

## Not in this row
- 7zs0f item 3 (`thread_grow` notes growth before `store.append`): its review says leave it unless it bites. Leave it.
- bibny's herdr timing test (`a_finished_tab_closes_a_grace_after_it_is_seen_and_a_cancelled_one_at_once`): a watch
  item. If it fails for you, name it in the report; do not change it.
- No protocol change: `turn_id` is already on the wire.
- The other tail7 rows' files: tail7-onto (core `judge/onto/*`, cockpit ontology), tail7-effort (protocol `route.rs`,
  `routing/effort.rs`, cockpit `lib/route.ts`, `protocol.gen`, Discord tests), tail7-recall (`bench/recall/*`,
  `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- In core `AGENTS.md` touch only the one thread-router line (tail7-onto and tail7-effort edit other sections).
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Renaming a test is not
  weakening it only if its asserts stay. A cloud row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- **Dependencies.** None new. **Store format** 49; nothing here is stored.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (threads, routing) and `crates/theseus-tui/AGENTS.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: how the steered case is staged (or why it could not be); what the sidebar's list is for and what you
did; where the ended helper lives; each test and its plant with the failing line; the keel guard's output; the store
format; a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
