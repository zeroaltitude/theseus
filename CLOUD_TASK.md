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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-self-tree-acts`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: self-tree-acts, a self-repair's edits in its own tree never ask the owner (theseus-kq4z4)

Branch: `cloud/20261019-self-tree-acts`. Every commit's subject carries `theseus-kq4z4`. Deadline for the report: 5
hours after you start.

**Why.** On the night of 2026-10-19 two of Theseus's self-repair tasks ("Calls from harness fail" and "anthropic answers
invalid_request") found their fixes and tried to write them in their own trees (`<state>/self/trees/tsk-<id>/`). Every
`fs.edit` there asked the owner, nobody answered within 15 minutes, every edit expired, and both fixes were lost. The
owner's standing rule (2026-10-09): self-improvement acts first and reports after; its safety comes from the keel, the
gates, the sealed scorer and one-click revert at the self-join, never from approvals on its drafts.

**What the record shows** (the owner's daemon's history, read by the maintainer; the session's own words are not
needed): the reads in the tree were open, and each `fs.edit` was held with this decision (path shortened):

```
posture: approve
reason: edit <state>/self/trees/tsk-…/crates/theseus-core/src/compiler/effort.rs: this call starts rust-analyzer on
<state>/self/trees/tsk-… (`rust-analyzer`), a program, judged as proc.run: start rust-analyzer on <state>/self/trees/tsk-…:
proc.run — approve (<state>/self/trees/tsk-… is protected: <state> is on the approve list)
```

So the own-tree rule (theseus-pw1q.21.9, `rsi/own_tree.rs`) already let the edit itself through. What asked was the
**edit hook's language-server start** (L3, `start_on_edit`; rust-analyzer is a preset that starts on edit): the edit is
judged a second time as `proc.run` on the tree's root, and that judgement does not know the self task's own tree, so
the state directory's approve-path hold asks. The owner's config: `[policy] enforcement = "notify"`, `[lsp] enabled =
true`, no `[lsp.servers]` override, `[self] mode = "act"`. The task's parent is a plan item with no session, so
`own_tree::of` names the right tree (`trees/tsk-<the task's id>`).

**Risk: medium.** This is the gate. The rule must stay exactly as narrow as it is: a self session, its own tree, its own
task; everything else in the state directory, other trees, the store, the secrets and the sealed sets keep their holds.

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| D2 | `self-tree-acts` | theseus-kq4z4 | a self session's edit in its own tree is never escalated by the edit hook's server start; an edit outside it still asks; the keel stays at the self-join | `theseus-core/src/lsp/{mod,edits}.rs` (`judge_start`, the edit hook's `gate`), `theseus-core/src/rsi/own_tree.rs`, `theseus-core/src/toolrun/order.rs` (passing `own` on, if needed), `theseus-core/src/rsi/tests_pr.rs` (the `own_tree` module) and the lsp tests | S-M |

## What exists (on your base; lines at `5e0bd195`)
- **The own-tree rule:** `rsi/own_tree.rs` (61 lines) `of(rt, session)` (near :27) gives `<trees>/<slug(root task)>` for
  a session whose principal is `"self"`, whose execution's principal is self, while `pr::trees_root` is set (`[self]
  mode = "act"` and a repo). Its doc comment says `proc.run` is untouched on purpose: a self build's code runs in L1 as
  `sandbox::self_build_view` says, never through this rule.
- **Where `own` reaches the gate:** `toolrun/order.rs` near :114-119 computes it for `fs.*` tools only, and passes it to
  `ToolPolicy::decide_in(tool, plan, tightened, own)` (`policy.rs` near :427). `preempted` (near :513) and `listed`
  (near :622) ask for anything under an approve path unless it is inside `own` and `own` is inside that approve path
  (the `held` closure, near :631-633). Unit test: `policy.rs` near :1185
  `a_self_tasks_own_tree_is_open_and_nothing_else_of_the_state_dir` (other trees, `..`, symlinks, the store, `own =
  None`).
- **The edit hook's second judgement:** `lsp/edits.rs` `gate` (near :108-140): for `fs.write`, `fs.edit`, `fs.patch`
  in a private place, for each written resource whose server has `start_on_edit` and has not started for that root, it
  calls `super::judge_start(rt, &spec, &root, plan, d)`. `lsp/mod.rs` near :1055 `judge_start` builds a `proc.run` plan
  (`Access::Exec` on the root, the server's argv) and calls `rt.policy.decide_with(...)`, **with no `own`**, and the
  stricter decision wins, with the reason quoted above (near :1084). The L2 start (`lsp/mod.rs` near :1049, an
  `lsp.*` tool's start) goes through the same function. `START_ON_EDIT` (near :84) is `["rust-analyzer", "ty", "tsgo"]`.
- **Roots:** `toolrun/guarded.rs` (`guarded_paths`, `pr::trees_root`) is a **keel-guarded file: do not edit it.**
- **The full-core tests:** `rsi/tests_pr.rs` near :458 `mod own_tree`: `held()` is `rig_with(SelfMode::Act, …)` with
  `approve_paths = [state_dir]`; `rig_with` (near :84) sets `enforcement = Posture::Notify` (as the owner's config);
  `task(r, id)` (near :466) appends a self `TaskRecord` and creates `state/self/trees/<id>`; `asked()` (near :493)
  checks `pending_confirms(sid)`. Tests near :500, :516, :556, :574 use `fs_write` and `fs_read` only: no `fs.edit`, and
  no language-server board in the rig, which is why the hook's ask was never seen.
- crates/theseus-core/AGENTS.md near :1022-1035 describes the own-tree rule; the gate's invariant (near :2616-2619) is
  floor, approve lists, allow list, then posture, written once in `toolrun/order.rs`, which `policy.explain` also runs.

## What to build
1. **The edit hook never escalates a self session's edit in its own tree.** Choose one of these, and say why in the
   report:
   - (a) a self session's edit in its own tree **starts no language server** (the hook skips `start_on_edit` there, the
     edit's result says so in `meta.lsp`, e.g. `"not_started": "self tree"`, and a server already up for that root
     still gives its diagnostics); or
   - (b) `judge_start` takes the call's `own` and judges the start with it, so the start inside the own tree is not held
     by the approve path.
   The maintainer's lean is (a): rust-analyzer runs a tree's build scripts and proc macros, and a self tree's code is
   written by the model, so starting it outside L1 would run that code unconfined, which the own-tree rule's comment
   rules out for `proc.run`. If you find that the server already runs confined (in L1 or a sandbox), (b) is fine: show
   where. Either way an `lsp.*` tool's own start (L2) keeps today's judgement unless you show it is the same case.
2. **Nothing else opens.** A self session's edit **outside** its tree (another tree, the state directory, the store, a
   path that resolves out by a link or `..`) asks exactly as today; a non-self session's edit in a tree asks as today; a
   self session's `proc.run` in its tree is judged as today.
3. **The decision says what it did.** `policy.explain` (which runs the same order) shows the same decision as the call,
   so an operator reading why an edit ran sees the self-tree rule named.
4. **The keel stays at the self-join** (`rsi/join.rs`, `rsi/join/keel.rs`): untouched. Say in the report that the
   join's keel and gate still judge the branch.

## Tests (each with a planted revert; the first is the key test)
- **`a_self_tasks_edit_in_its_own_tree_does_not_ask`** (in `rsi/tests_pr.rs`'s `own_tree` module or beside it, through
  the whole core): a rig with the language-server board on and rust-analyzer's preset starting on edit (a stand-in argv,
  so nothing real starts), a self task whose tree holds a `Cargo.toml` and a `.rs` file; its `fs.edit` of that file
  runs with no pending confirm and the file changes. Plant: restore today's `judge_start` call (no own-tree handling):
  the test must fail with a pending confirm whose reason names `proc.run`.
- `a_self_tasks_edit_outside_its_tree_still_asks`: the same session's `fs.edit` of a file in another tree and of a file
  elsewhere under the state directory each leave a pending confirm. Plant: let the rule open the whole trees directory.
- `a_user_sessions_edit_in_a_self_tree_still_asks`: a non-self session's edit there asks. Plant: drop the principal
  check.
- `a_self_tasks_proc_run_in_its_tree_is_judged_as_today` (and, for (b), the L2 `lsp.*` start): unchanged decision.
- `explain_names_the_self_tree_rule`: `policy.explain` for the own-tree edit gives the same posture as the call.
- Keep `policy.rs`'s `a_self_tasks_own_tree_is_open_and_nothing_else_of_the_state_dir` and the four `own_tree` tests
  green, unchanged.

## After the join (the maintainer's, not yours)
The two lost self-repairs are re-run on the owner's machine once this joins. In the report, say what the maintainer
should watch for in their first edit (the ledger's decision for an own-tree `fs.edit`).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree, this task is right: `main` is at `5e0bd195` (2026-10-19 06:43 MST:
v1's tag `bbc65530` plus five merges) or later, with **store format 45** (a join that lands while you clone may move it
by one). The `file:line` references in this task are at `5e0bd195`; on your base a line may have moved a little: search
by name.

Line ceilings at `5e0bd195` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-discord/src/runtime.rs`
**3,497 / 3,500** (full: add at most one line there, and prefer none; another row of this batch may add one too),
`crates/theseus-discord/src/render.rs` 2,963 / 3,009, `crates/theseus-core/src/turn.rs` 3,384 / 3,525,
`crates/theseus-protocol/src/lib.rs` 2,706 / 2,733, `crates/theseus/src/render.rs` 3,108 / 3,128. Unlisted files must
stay at or under 2,500 lines (`scripts/shape.sh`): `crates/theseus-core/src/toolrun.rs` is 2,489 (**add no line to
it**), `crates/theseus-sim/src/fake_discord.rs` is 2,450 (put new stand-in code in a `fake_discord/` module). Never raise
a ceiling and never add a `long-files.txt` entry. Put new code in new modules; in a long file add only the `mod` line, the
field or the call.

## v1 patch1, the owner's Discord and cockpit rows (this batch)

Theseus's v1 is cut. The owner uses it every day through his Discord DM, a voice channel, a forum of task posts and the
cockpit. On 2026-10-19 he looked at what reached him overnight and asked for these four changes (Discord DM, 08:27,
agreeing to the maintainer's recommendation, and asking to tune the questions mechanism so Theseus operates better in
the channel, not only to word things more nicely). His standing rules that bind every row: features ship **on**, with a
named rollback (2026-10-06); right over compatible (one right name, no aliases; stored data stays readable); minimal
prompting: anything a model reads states facts, never instructions (2026-10-09); self-improvement acts first and reports
after, and its safety is the keel, the gates and one-click revert at the self-join, not approvals on its drafts
(2026-10-09).

| Row | Slug | Beads | Area |
|---|---|---|---|
| D1 | `needs-you-cards` | theseus-suvpe | the core's needs-you post (`theseus-core/src/needs_you.rs`, and the asker's name), its Discord card and buttons (`courier/ask.rs`, `render/ask.rs`, `runtime/ask.rs`, a new module for a reply that answers), the DM reply that answers a question |
| D2 | `self-tree-acts` | theseus-kq4z4 | `theseus-core/src/lsp/{mod,edits}.rs` (`judge_start`, the edit hook's `gate`), `theseus-core/src/rsi/own_tree.rs`, `theseus-core/src/toolrun/order.rs` (the `own` argument only), their tests |
| D3 | `task-home` | theseus-lxfz0, theseus-xlxp4, theseus-90qqy | `crates/theseus-discord/src/runtime/{threads,forum,default_forum,task_channels,live_board,board,voice/tasks}.rs`, a new `runtime/task_home.rs`, `courier.rs` (`report_post`, `card()`, `route()` only), `courier/board.rs`, `render/board_v2.rs` + its golden, `render.rs` (`report` only), the `[discord]` config (`config/discord.rs`, the template), the stand-in's channel routes |
| D4 | `fleet-fullscreen` | theseus-uu3b2 | `cockpit/` only: `src/views/Fleet.tsx`, `src/components/TaskGraph.tsx`, a new pure module and its test |

All four run in parallel now. Joins are plain merges, one at a time: D2, D4, D1, then D3.

**Other work in flight today** (stay out of their areas): joins queued on the owner's machine: WS B (the provider-origin
allowlist for private data, routed-turn checkpoints, Jev's warnings: `routing/`, `data_gate.rs`, `catalog.rs`,
`turn/route_step.rs`, `turn/checkpoint_step.rs`, `config/privacy.rs`), the speed rows (the census and first request,
the turn's path before the request leaves, idle memory and binary size), the Google Workspace core (`crates/theseus-google/`,
`theseus-core/src/google/`, one registration line in `toolrun.rs`), turn-end-facts (`toolrun/job.rs`, `turn/`), quick
recall; a cloud row still building job-truths (`theseus-kernel/src/job.rs`, `toolrun/job.rs`, `theseus-tools/src/fs.rs`,
`file_read.rs`); later, the Google mail, drive, sheets and calendar rows and an effort row.

**Contracts between these rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **`crates/theseus-discord/src/courier.rs` is shared by D1 and D3 at different places.** D1 edits only the
   `"needs_you"` dispatch arm (near :894) and, if it records which message holds which question, the landed-write hook
   (near :1642, `shared.noted(...)`) by one call. D3 edits only `report_post` (near :1354), `card()` (near :1053) and
   `route()` (near :1225). Neither touches the other's.
3. **The DM's needs-you ping is D1's; where a task's card and report land is D3's.** D3 moves a task's approval card
   to its forum post and its report and board to the task home; D1 makes the DM's needs-you post a set of answerable
   cards. Each test asserts only its own side.
4. **`runtime.rs`:** at most one added line per row (a `mod` line or one call); D1 and D3 may each take one.
5. **The store format:** none of these rows is expected to change a store layout. If a row finds it must store a field,
   it says why in the report and bumps by one from its base (the joiner renumbers).
6. **No new model-facing instruction.** Any text a model reads (a tool result) states facts: what exists, where a thing
   went, what happened. It never tells the model what to do, never says "must", and never gives examples.

## Out of scope for every row

The owner's config, his daemon and his machine (his config may get a key at the install; say which in the report).
`docs/status.md`, the README, `docs/design/` and the spec: the maintainer writes those; say what should change and
where. The maintainer's own rehearsal scripts (they are not in this repository). Any benchmark run.

## The checks for this row (these replace the preamble's gate line)

This VM has 4 cores and no sccache, and the maintainer runs the full gate on the owner's machine at the join. So scope
your runs to the crates you touch:
- From the preamble's setup, run `cargo --version` first and alone, then `cargo install cargo-nextest --locked`; skip
  `cargo-deny` and its fetch. Run `npm ci` in cockpit/ only if the row touches cockpit/.
- Before every commit: `cargo fmt --all --check`, `bash scripts/shape.sh`, `cargo clippy CRATES --all-targets -- -D
  warnings`, and `TZ=America/Phoenix cargo nextest run --workspace -E 'FILTER'` (crates/theseus-core/AGENTS.md: use the
  `-E` filter, never `cargo test -p` or `nextest -p`, which rebuild the dependencies with other features). CRATES and
  FILTER are named in "Rules for this row".
- Before your last code commit: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`, and paste its
  output in the report. It must be empty.
- A full `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at the end is welcome if your deadline allows,
  never required. Say in the report which checks you ran and their counts.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

## Rules for this row
- CRATES: `-p theseus-core`. FILTER: `package(theseus-core)`.
- No config key unless (a) needs a switch; if you add one, it has a default, sits in the template and its tests, and
  off restores today's decision byte for byte.
- crates/theseus-core/AGENTS.md: update the own-tree paragraph (near :1022-1035) to name the edit hook's case.
