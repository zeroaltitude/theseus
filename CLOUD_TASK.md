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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7b-cn`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**Work in flight beside you:** nothing else is integrating. Two waves of rows base on `ae420a49` and join the lane
`integ/tail7`: the four `tail7-*` rows started an hour before you (ontology sweep, effort words, threads/TUI, recall
bench and data-gate word), and the six `tail7b-*` rows of this wave. A local lane also edits `scripts/keel-guard.py` and
its test: touch neither. Each row owns the files named in its task's "Rules for this row" and "Not in this row"
sections. Those win over anything older. Stay out of the other rows' files.

**The Discord DM task flow is locked** (the owner, 2026-10-22: "please do not change how it is now, all of the little
touches are so good"): the task reply, the started and thread lines, the board, the forum posts and live cards, the
approval card and the task reports. No row in this wave changes their text, order or buttons. If your change would move
one of them, stop and say so in the report instead.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7b-cn, draft trees outside the state dir, a first pass may not be its own draft's model, and the first pass's real-timer bound under test (theseus-iilre option a, theseus-ms3hd)

Branch: `cloud/20261021-tail7b-cn`. Every commit's subject carries its Beads id (`theseus-iilre` or `theseus-ms3hd`).
Deadline for the report: 3 hours after you start.

**Why.** The China-origin draft review (a restricted-origin model drafts a change in its own git worktree, an optional
restricted-origin first pass reviews it, and a Claude model always does the final review) is frozen as built; the owner
chose its last two pieces on 2026-10-22:
- **iilre, option (a).** Draft trees live under the state dir (`<state>/drafts/<short>`), and the state dir is on the
  approve list, so the review's `cargo test` in the draft's tree asks the owner every time. The owner: store draft trees
  OUTSIDE the state dir, with no special trust rule (option (b), widening `own_tree::open_to` to `proc.run`, is NOT
  wanted).
- **ms3hd.** Setting a class's `first_pass` to the same profile as its `cn_draft` is accepted today, so the model would
  review its own draft. The owner's rule (2026-10-17): a Claude model always does the final review, and a review by the
  drafting model itself is no review. Refuse it at config check. Also, the first pass's 10-minute bound is tested only
  on the sweep path (a past deadline at start); its real-timer path is untested.

**Risk: medium for step 1** (a path move with the policy's roots), low for 2-3.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7b-cn` | theseus-iilre (option a only), theseus-ms3hd | draft trees in their own root outside the state dir; `first_pass == cn_draft` refused; the bound's timer path tested | core `draft.rs` (`tree_of`, and a `drafts_root` beside it), core `toolrun/guarded.rs` (`guarded_paths`: the new root joins the roots), core `config.rs` (a `drafts_dir()` beside `state_dir()` if you make it a setting), core `config/routing_cn.rs` (`drafts`), core `draft/first_pass.rs` (a test seam for the bound only), `tests_draft.rs` (`rig` near :192-250), `tests_draft_first_pass.rs`, a new `tests_draft_home.rs` if the new tests do not fit, the template's draft comment (`[routing.classes.*]` draft block near :1688-1700, one or two comment lines), core AGENTS.md (the draft-review paragraph) | M |

## What exists (at `ae420a49`; search by name; all under `crates/theseus-core/src/`)
- `draft.rs` (865 lines): `tree_of` near :180 = `cfg.state_dir().join("drafts").join(short(draft_session))`;
  `branch_of` near :184; `own_tree` near :162 (the draft, its review and its first pass); `open_tree` near :308;
  `close_tree` near :398 (`git worktree remove --force`, branch kept); `drop_draft` near :407; `settle_tree` near
  :424-440. The `Draft` record stores the tree's absolute path (`Draft.tree`, near :87-92), so an old draft keeps its
  old path.
- `turn/draft_step.rs` near :123 makes the draft's tree; `draft/first_pass.rs` near :194 makes the first pass's
  (`first_pass_tree` near :286); `draft/sweep.rs` settles unsettled drafts at start.
- The approve list: `default_approve_paths()` in `config.rs` near :496 includes `~/.theseus`; the rule is
  `ToolPolicy::listed` in `policy.rs` near :662-685 ("`{p}` is protected: `{d}` is on the approve list"); it checks
  reads outside the roots too (near :687). The roots and the floor: `toolrun/guarded.rs` `guarded_paths`; the self
  trees join the roots through `rsi::pr::trees_root` (`rsi/pr.rs` near :166, `state/self/trees`): the model to copy.
  `rsi/own_tree.rs` `open_to` near :127 gives the own-tree carve-out to `fs.*` only (leave it as it is).
- Temp and cache places the code already uses: `own_bench/whole/scratch.rs` (0700 `theseus-replay-*` under
  `temp_dir()`), `bench.rs` near :25 (`~/.cache/theseus/...`). There is no `cache_dir`/XDG helper.
- Config checks: `config/routing_cn.rs` `validate` (near :15, called from `config.rs` near :1716) -> `drafts` (near
  :74-128) refuses `first_pass` without `cn_draft`, `cn_draft` outside `DRAFTING`, a non-restricted `cn_draft`, a
  `cn_draft` without `final`, a non-restricted `first_pass` (near :110-117), a non-Claude `final`. Keys: `ClassProfiles`
  in `config/routing.rs` near :165-181 (`cn_draft`, `final` (`final_review`), `first_pass`).
- The bound: `draft/first_pass.rs` `BOUND` (10 min) near :48; `open_first_pass` near :211 sets `deadline_ms`;
  `start_first_pass` near :141 calls `arm_first_pass(sid, BOUND)` near :172; `arm_first_pass` near :308 spawns a task
  that sleeps then calls `expire_first_pass` (near :321-357: re-arms with the time left if the deadline has not passed,
  else `cancel_execution(.., "first pass: past its bound")`).
- Tests: `tests_draft.rs` `rig` puts `state_dir/drafts` into `tools.roots` and the state dir into `approve_paths` by
  hand; `a_draft_writes_only_its_tree`, `a_draft_is_never_offered_a_program`, `a_draft_from_a_subdirectory_applies_at_
  the_top`, `final_must_name_claude_profiles`, `cn_draft_only_on_coding_classes`; `tests_draft_hardening.rs`;
  `tests_draft_first_pass.rs` (`a_first_pass_past_its_bound_is_cancelled_and_the_review_opens` near :156 = the sweep
  path, `first_pass_without_cn_draft_is_refused`, `a_claude_first_pass_is_refused`); `policy.rs`
  `a_self_tasks_own_tree_is_open_and_nothing_else_of_the_state_dir` near :1229. No test asserts where a draft tree is.

## What to build
1. **Draft trees outside the state dir (iilre a).** One function answers the drafts root (for example
   `Config::drafts_dir()`): by default a directory OUTSIDE `server.state_dir` and outside every default approve-list
   entry, persistent across a reboot (drafts wait for review; not `/tmp`). Suggested default: `~/.cache/theseus/drafts`
   (expanded like `state_dir`), created 0700. If you make it a key (`[server] drafts_dir`), it needs a default, the
   template line, and a config check that refuses a value inside the state dir or inside an approve-list entry (that
   would bring the asking back). `tree_of` uses it. The root joins the policy's roots in `guarded_paths` exactly as
   `trees_root` does, so the review's reads and runs in its tree take the tool's normal posture (no approve-list hold),
   while the rest of the state dir stays protected. Old drafts keep the path in their record: settle and close them
   where they are (test it). Do not widen `own_tree::open_to`; the draft model is still never offered a program
   (`a_draft_is_never_offered_a_program` stays green).
2. **Refuse `first_pass == cn_draft` (ms3hd).** In `routing_cn.rs` `drafts`, after the first-pass checks: a class whose
   `first_pass` names the same profile as its `cn_draft` is refused, with a message in the same style as the others that
   says why (the drafting model would review its own draft). Check the shipped template's example classes and the
   owner-shaped configs in tests: none may now fail (say if one did).
3. **The bound's timer path (ms3hd).** Test the real timer, not only the sweep: through a small seam (the bound as a
   parameter `arm_first_pass` already takes, or a `#[cfg(test)]` bound), a first pass whose bound passes while it runs
   is cancelled by the timer with "first pass: past its bound" and the review opens; and a timer that wakes before the
   deadline (the deadline moved later) re-arms and does not cancel early. Use real short durations (tens of
   milliseconds) or tokio's paused clock, whichever the crate's other timer tests use (search `start_paused`); a paused
   clock does not advance past blocking work, so prefer real time if the path calls `spawn_blocking`.

## Tests (each with a planted revert; the first is the key test)
- **`a_drafts_cargo_test_runs_without_asking`** (key): on a rig whose approve list holds the state dir as the default
  does (the `rig` must stop adding `state_dir/drafts` to the roots by hand: the code does it now), the review's
  `proc.run` of a test program in the draft's tree takes the tool's posture and is not held by the approve list. Plant:
  `tree_of` back under the state dir -> it asks (fails).
- `a_draft_tree_is_outside_the_state_dir` (and the first pass's tree too).
- `the_state_dir_is_still_protected_beside_the_drafts_root` (a write elsewhere in the state dir still asks; mirror of
  `a_self_tasks_own_tree_is_open_and_nothing_else_of_the_state_dir`).
- `an_old_draft_under_the_state_dir_still_settles` (a `Draft` record with the old path settles and its tree closes).
- If a key: `a_drafts_dir_inside_the_state_dir_is_refused`.
- **`a_first_pass_on_its_drafts_own_profile_is_refused`**. Plant: drop the check.
- `a_first_pass_past_its_bound_is_cancelled_by_its_timer` and `an_early_timer_rearms_and_does_not_cancel`. Plant: make
  `expire_first_pass` ignore the deadline (cancel at once) for the second; skip the cancel for the first.
- `tests_draft*.rs` all green; `cargo nextest run -p theseus-core draft`, `-p theseus-core policy`, `-p theseus-core
  routing`, then `-p theseus-core` whole.

## Not in this row
- iilre's visibility half (drafts and reviews in `theseus tasks` and the cockpit, a task-graph record): not now (the
  owner froze the chain as built; it is a follow-up for after real use).
- No change to the draft review's models, classes, prompts, or the data floor. No new trust rule (option b).
- The first pass's external-text hold (the review's F3, info only): leave it.
- The other wave rows' files: tail7b-hold (`external.rs`, `PolicyConfig.external_text`, the template's `[policy]`
  external-text lines and `[privacy]` block, theseus-judge `pack.rs`), tail7b-budgets (`turn/detour_step.rs`,
  `config/threads.rs`, `first_request.rs` tests), tail7b-tiers (`turn/route_step.rs`, `config/memory.rs`,
  `config/defer.rs`, recall's pack step, the ontology walk), tail7b-messages (the proactive messages outside the task
  flow), tail7b-golden (the Discord tests). The tail7 rows: tail7-onto (`judge/onto/*`, cockpit ontology), tail7-effort
  (protocol `route.rs`, `routing/effort.rs`, cockpit `lib/route.ts`, `protocol.gen`), tail7-threads (the TUI,
  `tests_hold_compile.rs`), tail7-recall (`bench/recall/*`, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`).
  `config.rs` is shared with tail7b-hold (it edits only `PolicyConfig.external_text`); keep your edit to the new
  function and its default.
- `scripts/keel-guard.py` and its test (a local lane).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling (the 10-minute bound stays
  10 minutes; a seam for tests is not a raise). Moving the drafts out of an approve-list entry is the owner's chosen
  change; the state dir's own protection stays. A cloud row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- **Dependencies.** None new. **Store format** 49: `Draft.tree` is already stored; adding no field needs no bump.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (the draft review, the data rule, policy and the roots) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the drafts root's default and why, how it joins the roots, what the owner sees change (the review's
test runs no longer ask), what happens to old drafts; the refusal's message; how the timer test drives the real path;
each test and its plant with the failing line; the keel guard's output; the store format; a `docs/status.md` sentence
for the integrator; a line for the owner's config if a key was added (its default is right for him: say so or not);
follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
