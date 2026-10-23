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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7d2-misroute-run`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch does **not** start from `main`. It starts from `bb6a43e5`, the fix commit of row `tail7d-misroute`
(theseus-4gb4w, branch `cloud/20261021-tail7d-misroute`), which sits on `main` at `f9c11594` (2026-10-22: release
stage 7a, install #74a, the card hotfix, on top of stage 6's `ae420a49`). That commit is your base: its misroute guard
already reads a thread's relative roots under the session's directory, and you build on that code. Its two cloud
commits (`CLOUD_TASK.md`, `CLOUD_REPORT.md`) are not in your base. Line numbers below are quoted at `bb6a43e5`.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row needs no
bump: the guard keeps its state in memory and reads the stored working sets as they are. If you find you need a stored
field after all, stop and say so in the report instead.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). An unlisted file stays at 2,500 lines or
fewer. Every file this row touches is unlisted and under that: `toolrun/misroute.rs` 388, `rpc/thread_flow.rs` 861,
`rpc/tests_thread_route.rs` 1,762. Never raise a ceiling yourself (the keel guard flags it).

**Work in flight beside you** (rows joining the lane `integ/tail7`): the thread lens and cross-stream recall
(`thread_recall.rs`, theseus-memory `threads.rs`, `rpc/tests_own_top.rs`, `rpc/tests_thread_mix.rs`,
`config/threads.rs`), holds and external text (`external.rs`, `toolrun/order.rs`), budgets (`turn/defer_step.rs`,
`turn/detour_step.rs`, `compiler.rs`, `thread_context.rs`), injection tiers (`recall.rs`, `turn/recall_step.rs`,
`turn/rerank_step.rs`, `config/memory.rs`, `turn.rs`), the ontology, effort routing, the TUI, drafts, message texts,
the config template and Discord. Stay inside the files your task names. If you must edit a file outside that list,
name it and the reason in your report's first section.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7d2-misroute-run, a program run from the session's directory (an ancestor of the thread's roots) is undecided, not held: the misroute guard stays armed and the next write decides (theseus-qu41j)

Branch: `cloud/20261021-tail7d2-misroute-run`. Every commit's subject carries `theseus-qu41j`. Deadline for the report: 2
hours after you start.

**Why (P1).** Threadbench T13 (install #74, 2026-10-22) held 44 turns at Approve across six threads-arm runs. The row
before you (theseus-4gb4w, your base `bb6a43e5`) made the guard read a thread's relative roots (`ledgerweb`,
`tarnlib`, ...) under the session's directory. Its review re-checked every `thread.misroute_held` row of those runs
against the new rule: 20 of the 44 holds are freed and 24 stay. About 15 of the 24 are still false, and all of them
are the same shape:
- A `proc.run` plan's resource is the **directory the program runs in** (`theseus-tools` `proc.rs`, the `Plan` it builds:
  `Resource { path: cwd, access: Access::Exec }`, one per distinct step directory in a batch).
- A turn that runs a program from the **session's directory**, the parent of every relative root, therefore names a
  path that is outside every root. Example from the bench, in the ledgerweb thread, a correct placement:
  `["bash","-c","cd ledgerweb && cat Makefile && python3 -B -m unittest discover -s tests -v ..."]`, run at
  `/out/tb-a1-r1/workspace`. The guard holds it with a false question.
- The issue's title already says "first write **or program**". This is the same symptom by another path.

**Risk: low.** About 8 lines in one function plus tests. A real misplacement is still caught: at its first write
outside the roots, or at a program run inside another thread's tree (the bench's `proc.run` at `WS/tarnlib` from the
ledgerweb thread stays held), or at a program run in an unrelated directory.

**Prerequisite: base `bb6a43e5`** (the 4gb4w fix on `main` `f9c11594`). Your branch joins the lane `integ/tail7` (stage
7, install #75), merged right **after** `tail7d-misroute`. Run the keel guard against `bb6a43e5`, not `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7d2-misroute-run` | theseus-qu41j | an Exec whose path is an ancestor of a root is undecided: no hold, the guard stays armed, the next definite write or program decides; red-first tests at unit and rpc level | core `toolrun/misroute.rs` (`Guards::check`, the module doc, its tests), core `rpc/tests_thread_route.rs` (one or two new tests beside the guard's, after the two 4gb4w tests near :900-976), core `AGENTS.md` (the misroute-guard sentence, near :604-606) | S |

## What exists (at `bb6a43e5`; search by name)
- `toolrun/misroute.rs` (388 lines).
  - `Guards::arm(session, title, roots, base, alt)` (near :115) reads each root with `thread_route::entry_path(r, base,
    home)`; a relative root with no base is left out, and no root left disarms.
  - `Guards::check(session, plan)` (near :149) keeps only the plan's `Write`/`Exec` resources, returns `None` when there
    are none, then **removes** the session's `Armed` from the map (`g.remove(session)?`) and holds the first resource
    whose folded path no root is a prefix of (`let inside = armed.roots.iter().any(|root| p.starts_with(root))`, near
    :162). So the guard always lets go at the first write or program, held or not.
  - `Held { path, verb, thread, alt }` and `Held::why` (near :45). The verb for an Exec is "runs a program in".
  - Unit tests near :208-388: `the_first_write_outside_the_roots_is_held_once`,
    `a_thread_with_no_roots_and_a_session_never_armed_hold_nothing`, `home_is_expanded_in_a_root_and_shown_in_the_question`,
    `a_relative_root_is_read_under_the_session_directory` (near :321), `a_path_outside_every_root_is_still_held` (near
    :348), `the_question_has_no_doubled_article` (near :374). Helper `plan(access, path)` near :213.
- `rpc/thread_flow.rs` `arm_guard` (near :437) arms a placement below the act bar; `thread_routed_turn` (near :822)
  calls `self.tools.misroute.disarm(&r.session)` after the routed turn ran. So an armed guard never outlives its turn:
  "stays armed" means until the turn's next write or program, or the turn's end. Do not change `thread_flow.rs`.
- `ToolRuntime::misroute_hold` (misroute.rs, after `impl Guards`) records `thread.misroute_held` and raises the
  decision to Approve when `check` returns a `Held`. No change needed there.
- `rpc/tests_thread_route.rs`: `write_to(path)` (near :839) scripts one `fs_write`. The rig to copy is
  `a_guess_placed_turn_inside_its_threads_relative_root_writes_without_a_hold` (near :905) and
  `a_guess_placed_write_outside_its_relative_roots_is_still_held` (near :942): the temp dir is both `[tools] roots` and
  `[tools] cwd`, pr-round has the relative root `tidewatch`, security-fix has `vault`, and the turn is placed at 0.75
  (below the act bar) with `thread_answer(&jev, "pr-round", 0.75)`. Other tests script a `proc_run` call with
  `Scripted::tools("...", &[("t1", "proc_run", args)])` (see `rpc/tests_peek.rs` near :132).

## What to build
1. **An Exec at an ancestor of a root is undecided.** In `Guards::check`, sort each Write/Exec resource `p` (folded) into
   one of three:
   - **inside**: some root is a prefix of `p` (as now);
   - **undecided**: `p` is an `Exec` and `p` is an ancestor of some root (`root.starts_with(&p)`, whole components, so
     `/work/ws` is an ancestor of `/work/ws/tidewatch` and `/work/ws/tide` is not);
   - **outside**: anything else, a write at an ancestor included (a write names its own file, so it decides).

   Then: any **outside** resource is held as now (first one, same `Held`, guard let go). Else, if every resource is
   **undecided**, nothing is held and `armed` goes **back into the map** for this session, so the next write or program
   decides. Else (at least one inside, none outside) the guard lets go as now. Keep it the smallest change that reads
   clearly; do not restructure `check` beyond this. A write is never undecided: name that in the doc and a test.
2. **No new state.** The guard is disarmed at the turn's end already; the undecided path only skips the `remove`. No
   counter, no config key, no ledger row for an undecided call. If you think one is needed, say why in the report and
   do not add it.
3. **What stays held, and why it is still safe.** A turn placed in the wrong thread whose only acts are programs run
   from the session's directory is no longer held. Its first write, or a program run inside another tree, is. State
   this trade in one sentence in the module doc. It is the review's recommendation; the owner's approval card was a
   false question in about 15 of 24 bench cases.
4. **The question is unchanged.** `Held::why` and its strings do not change: the text reaches the owner's approval
   card, and the DM task flow is locked.
5. **Docs.** The module doc of `misroute.rs` (its "first write or program is looked at once" sentence) and the core
   `AGENTS.md` misroute-guard sentence (near :604-606) say, in one clause, that a program run from a directory above the
   thread's roots (the session's directory) decides nothing, and the next write or program does. **AGENTS.md line 606 is
   about 200 columns** after the 4gb4w row; its integrator rewraps lines 605-607 to the paragraph's ~120-column width
   at that join. Make your edit there **and** rewrap the same lines to ~120 columns in the same hunk, so the integrator
   can take your side of the conflict whole. Say so in the report's first section.

## Tests (red first: write each, see it fail on the base code, then fix; each fix also gets a planted revert)
- **`a_program_run_from_the_session_directory_neither_holds_nor_lets_go`** (misroute.rs unit; the key test). Arm with
  roots `["tidewatch"]` and base `/work/ws`. `check` with `plan(Access::Exec, "/work/ws")` returns `None` **and**
  `g.armed("ses_a")` is still true. Then a write to `/work/ws/vault/x` **is** held (the question names it) and the guard
  lets go. Arm again: an Exec at `/work/ws`, then a write to `/work/ws/tidewatch/notes.txt`: neither is held and the
  guard lets go. On the base the first `check` lets go (red).
  Plant: in `check`, treat an undecided Exec as inside (lets go). The `armed` assertion fails.
- **`a_write_at_the_session_directory_still_decides`**. An `Access::Write` at `/work/ws` itself (or a file directly in it,
  `/work/ws/x.txt`) is held. Pins that only programs are undecided.
- **`a_program_run_in_an_unrelated_or_other_threads_directory_is_still_held`**. Roots `["tidewatch"]`, base `/work/ws`:
  an Exec at `/work/ws/vault` (another tree) is held, an Exec at `/elsewhere` is held, an Exec at
  `/work/ws/tidewatch/sub` lets go. A batched plan with two Exec resources, `/work/ws` and `/work/ws/vault`, is held at
  `/work/ws/vault`.
  Plant: drop the `Exec` check from "undecided" (any ancestor is undecided). The write test fails.
- **rpc level, `a_guess_placed_program_from_the_session_directory_is_not_held`** in `rpc/tests_thread_route.rs`. Copy the
  4gb4w rig (relative root `tidewatch`, temp dir as `[tools] cwd`, placed at 0.75). Script a `proc_run` at the temp dir
  (no `cwd` argument, so it runs in `[tools] cwd`) with a harmless argv, then an `fs_write` into `tidewatch/`. Expect no
  `thread.misroute_held` row and the file written. In the same test or a second one, script the `proc_run` at the temp
  dir followed by an `fs_write` into `vault/`: exactly one `thread.misroute_held` row, naming the vault path, and the
  file not written. **Check first** whether the rig's policy confirms `proc.run` on its own: if it does, pick an argv
  the policy allows, or assert on the `thread.misroute_held` rows and the confirm's reason (not the guard's question)
  instead of `awaiting_confirm`. Say which in the report. On the base the program call is held (red).
  Plant: as the key unit test.
- Every existing guard test passes unchanged (the six unit tests and the three rpc tests near :854-976).
- Run `cargo nextest run -p theseus-core misroute`, `-p theseus-core thread_route`, then `-p theseus-core` whole. If a
  test outside these files goes red, rerun it alone and on the base before you call it unrelated.

## Not in this row
- `thread_flow.rs` (`arm_guard`, `thread_routed_turn`), `thread_grow.rs`, stored working sets, router scoring,
  `home_claim.rs` `keep_apart_by_dir` (its own follow-up).
- The recall lens and cross-stream contamination: row `tail7d-contam` (theseus-stwkb), beside you. Do not touch
  `thread_recall.rs`, theseus-memory `threads.rs` or `config/threads.rs`.
- `proc.rs` and every theseus-tools file: the plan's resources stay as they are.
- The bench harness (its simulated owner never answers a confirm: theseus-uxum0).
- Discord: the approval card renders the reason as given. Change no Discord file.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base bb6a43e5 --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49: no bump.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line `main`'s recent commits carry (copy it from
  `git log -3 --format=%B f9c11594`; do not retype it, and do not use the one on `bb6a43e5`, which differs).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (the thread router, the misroute guard, a thread's working set) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- the cause in two lines and the rule you built (inside / undecided / outside), with the line it lives on;
- the trade in one sentence (what a misplaced turn that only runs programs from the session directory now does);
- the AGENTS.md hunk and the rewrap, for the integrator (it conflicts with 4gb4w's join fix 2: take this side);
- each test, and its plant with the failing line, the red-first run on the base included;
- the rpc test's policy choice for `proc.run`;
- the keel guard's output and the store format;
- a `docs/status.md` sentence for the integrator;
- how the owner's daemon behaves after the install: a guess-placed turn that runs a program from the session's
  directory gets no card, and its next write outside the thread's roots still asks once;
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
