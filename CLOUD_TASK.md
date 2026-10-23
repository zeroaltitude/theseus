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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7d-misroute`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `f9c11594` (2026-10-22: release stage 7a, install #74a, the card hotfix, on top of
stage 6's `ae420a49`); that is your base. Stage 7a changed only the card's files, `docs/status.md` and one core
`AGENTS.md` paragraph (the owner card), so every line number below, quoted at `ae420a49`, still holds within a few
lines: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Neither row in this
wave needs a bump: the misroute guard reads the stored working sets as they are, and the thread lens stores nothing. If
you find you need a stored field after all, bump it to 50, add the old layout's literal sample to
`crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and never write the number as a literal anywhere but the
constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). An unlisted file stays at 2,500 lines or
fewer. Every file these rows touch is unlisted and under that: `toolrun/misroute.rs` 288, `rpc/thread_flow.rs` 853,
`rpc/tests_thread_route.rs` 1,686, `thread_recall.rs` 370, theseus-memory `threads.rs` 368, `rpc/tests_own_top.rs` 175,
`rpc/tests_thread_mix.rs` 224, `config/threads.rs` 549. Never raise a ceiling yourself (the keel guard flags it).

**Work in flight beside you** (rows on `ae420a49` joining the lane `integ/tail7`): the ontology (`judge/onto/*`), effort
routing, the TUI, `tests_hold_compile.rs`, the recall bench, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`
(`build_telemetry`), external text and holds (`external.rs`, `toolrun/order.rs`), budgets (`turn/defer_step.rs`,
`turn/detour_step.rs`, `config/threads.rs`'s working size, `compiler.rs`, `compiler/cold.rs`, `thread_context.rs`),
injection tiers (`recall.rs`, `turn/recall_step.rs`, `turn/rerank_step.rs`, `config/memory.rs`, `turn.rs`), drafts,
message texts, the config template and Discord. Stay inside the files your task names. If you must edit a file outside
that list, name it and the reason in your report's first section.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7d-misroute, the misroute guard reads a thread's relative roots under the session's directory, so a guess-placed turn inside its own thread is not held; and its question loses the doubled article (theseus-4gb4w)

Branch: `cloud/20261021-tail7d-misroute`. Every commit's subject carries `theseus-4gb4w`. Deadline for the report: 2 hours
after you start.

**Why (P1, on the owner's daemon too).** Threadbench T13 (install #74, 2026-10-22) held 44 turns at Approve across six
threads-arm runs. 33 of them were in the right thread. The cause:
- Since theseus-98bz.5, a thread's working set grows from its turns. `thread_grow.rs` `touched_in` makes each path
  relative to the session's directory and stores its first component as a root. So the stored roots are **relative**:
  `tarnlib`, `datatidy`, `ledgerweb`, `homesvc`, `notes`, `quillhn`.
- The misroute guard (theseus-rha2, `crates/theseus-core/src/toolrun/misroute.rs`) normalises each root with no base,
  in `Guards::arm` (near :94: `normal(r, home)`). `Guards::check` (near :117) then tests
  `fold(&r.path).starts_with(root)` against the plan's **absolute** path, for example
  `/out/tb-a1-r1/workspace/homesvc/scripts/restart.sh` against `homesvc`. That never matches.
- So the first `fs.write`, `fs.edit` or `proc.run` of every turn placed below the act bar is held with a false question.
  In the bench nobody answers a tool confirm, so the held turn's work is lost: 8-9 turns per run stop at
  `awaiting_confirm`. At T12 (before 98bz.5) the working sets were empty and the guard never armed, so 0 turns were held.
  On the owner's daemon it means a spurious approval card on a correct placement.

The question also reads "Go on, or move to the a path-traversal fix in a small web service?". A thread's title can
start with an article, and `Held::why` (near :45) writes "the {alt}" and "the {thread} thread" around it.

**Risk: low.** One comparison gets the base the grower used. The guard still holds a real misplacement (11 of the 44
in the bench were real). The question's text changes only for a title that starts with an article.

**Prerequisite: base `origin/main` at `f9c11594`.** Your branch joins the lane `integ/tail7` (stage 7, install #75), and
the integrator merges it **first among the tail7d rows**. Keep it small and fast. Run the keel guard against
`origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7d-misroute` | theseus-4gb4w | relative roots are read under the session's directory before the guard compares; the question has no doubled article; red-first tests at unit and rpc level | core `toolrun/misroute.rs` (`Guards::arm`, `Held::why`, its tests), core `rpc/thread_flow.rs` (`arm_guard` only, near :437-462, and its one call near :216 if the base must be passed in), core `rpc/tests_thread_route.rs` (one or two new tests beside the guard's, near :850-935); core `AGENTS.md` (the misroute-guard sentence, near :604-606) | S |

## What exists (at `f9c11594`; search by name)
- `toolrun/misroute.rs` (288 lines). `normal(path, home)` (near :63) expands `~` and folds. `fold` is near :72.
  `Guards::arm(session, title, roots, alt)` (near :94) stores `Armed { roots: normal(r, home) }`. `check` (near :117)
  takes the first write/exec resource and holds it when no root is a prefix. `Held::why` is near :45. The unit tests
  are near :180-288: `the_first_write_outside_the_roots_is_held_once` (absolute roots only),
  `a_thread_with_no_roots_and_a_session_never_armed_hold_nothing`, and `home_is_expanded_in_a_root_and_shown_in_the_question`.
- `rpc/thread_flow.rs` `arm_guard(&self, tag, run, live, scored)` (near :437). It takes `t.working_set.roots` as stored and
  picks `alt` as the next-likeliest live thread's title. Its caller is near :216:
  `self.arm_guard(&tag, &run_session.session_id, &live, &scored)`, after `session_for`, so `run_session` (a
  `SessionRecord` with `dir: Option<String>`) is the session the turn's tools run on.
- `toolrun/session_dir.rs` `ToolRuntime::cwd_for(dir)` (near :34) is the one resolver: the session's directory
  (canonical), else `[tools] cwd`. Every tool's default directory comes from it, and so does the base `thread_grow` used
  (`rpc/thread_grow.rs` `grow_for` stores `dir: self.tools.cwd_for(session.dir.as_deref())`).
- `thread_route.rs` `entry_path(e, dir, home)` (near :578) already reads a working-set entry as a path: `~` expanded, a
  relative entry joined under `dir`, and none without one. `holds_path` (near :593) uses it. It is the router's
  directory rule (theseus-vw8k7). Reuse it; do not write a third resolver.
- `rpc/home_claim.rs` `keep_apart_by_dir` (near :108) reads a thread's relative roots under its own session record's
  `dir`, with no `[tools] cwd` fallback. Read it; see "What to build" item 4.
- `rpc/tests_thread_route.rs` `a_write_outside_the_threads_roots_is_held_only_below_the_act_bar` (near :853) is the rig to
  copy. It uses `write_to(path)`, `thread(c, slug, title, ws(..))`, `thread_answer(&jev, slug, 0.75)`, `home(c, None)`,
  `submit`, `confirm_list`, and `rows(c, "thread.misroute_held")`. Its roots are absolute temp paths, which is why it
  never caught this bug.

## What to build
1. **Read relative roots under the turn's directory.** Pick one of these, justify it in the report, and test it:
   - **(a, recommended) Resolve at arm time.** `arm_guard` passes the run session's directory,
     `self.tools.cwd_for(run_session.dir.as_deref())`, and `Guards::arm` reads each root with
     `thread_route::entry_path(r, Some(base), home)`. An absolute or `~` root is unchanged. That base is the one the
     grower made the roots relative to, and the one the turn's tools resolve the plan's paths against.
   - (b) Store roots absolute in `touched_in`. Do not pick this unless (a) cannot work. The owner's store already holds
     relative roots, so they would need a migration (a format bump). The router's entity hits, the recall lens's naming
     waiver (`waived`), Jev's thread inputs, `added_by` and `without` all compare entries by name (`tarnlib`). The
     directory rule already reads relative roots under a base, as (a) does.
2. **A relative root with no base** cannot happen through `cwd_for`, which always gives a directory. If your signature
   allows a missing base, a relative root without one must not arm the guard on a false premise. Either skip that root
   (and arm nothing when no root is left, as an empty set does now) or keep it as it is. Say which, with a test.
3. **The question reads right.** In `Held::why`, a thread title or `alt` that starts with an article (`a `, `an `,
   `the `, any case) is not given a second one. "Go on, or move to the a path-traversal fix ..." becomes "Go on, or move
   to a path-traversal fix ...". Treat "isn't in the {thread} thread" the same way. Choose the smallest wording that
   reads right and say which. A title with no article must keep its exact current text: the existing test's string
   (`This edits /work/vault/src/a.rs, which isn't in the pr round thread. Go on, or move to the security fix?`) stays
   byte-identical. That text reaches the owner's approval cards, and the DM task flow is locked, so change nothing else
   in it.
4. **Check its twin, and report.** `keep_apart_by_dir` (near :108) reads a thread's relative roots under the thread's
   own session `dir`, with no fallback. A thread whose session has no `dir` has its relative roots unread there, so the
   directory rule skips that thread. That direction is safe, but it is not the base the grower used. Report what you
   find, and change it **only** if it is the same bug and a line or two with the same `cwd_for` base. Otherwise file it
   as a follow-up in the report. Do not change the router's scores in this row.
5. **Docs.** `misroute.rs`'s module doc and the core `AGENTS.md` misroute-guard sentence (near :604-606) say, in one clause,
   that relative roots are read under the session's directory, the one the working set grew them in.

## Tests (red first: write each, see it fail on the base code, then fix; each fix also gets a planted revert)
- **`a_relative_root_is_read_under_the_session_directory`** (misroute.rs unit; the key test). Arm with roots
  `["tarnlib"]` and base `/work/ws` (through whatever signature you chose). A write to `/work/ws/tarnlib/retry.py` is
  **not** held, and the guard lets go. Arm again; a write to `/work/ws/homesvc/scripts/restart.sh` **is** held, and the
  question names that path. A sibling `/work/ws/tarnlib-old/x` is held (component-wise prefix, as now).
  Plant: drop the base, so roots are read as before. The first assertion fails.
- **`a_path_outside_every_root_is_still_held`**. Mixed roots: one absolute (`/work/other`), one `~` (`~/projects/x`), one
  relative (`notes`). A write under each is not held; a write under none is held. This pins that (a) did not loosen
  the guard.
- **rpc level, `a_guess_placed_turn_inside_its_threads_relative_root_writes_without_a_hold`** in
  `rpc/tests_thread_route.rs`. Copy the rig of `a_write_outside_the_threads_roots_is_held_only_below_the_act_bar`, but
  give `pr-round` the **relative** root `tidewatch` and run the turn with the temp dir as the session's directory (or as
  `[tools] cwd`, whichever the rig sets). Place it at 0.75 (below the act bar), with the scripted write inside
  `tidewatch/`. Then `awaiting_confirm` is none, the file exists, and there is no `thread.misroute_held` row. In the same
  test or a second one, a write into `vault/` is still held with one row. The base code holds the first write: that is
  the red.
  Plant: as the unit test.
- **`the_question_has_no_doubled_article`**. `alt` = "a path-traversal fix in a small web service" and a thread titled
  "the pipeline" each give text with no "the a" and no "the the". The existing exact-string assertion still passes.
  Plant: restore the old `format!`. The new test fails.
- Run `cargo nextest run -p theseus-core misroute`, `-p theseus-core thread_route`, then `-p theseus-core` whole.

## Not in this row
- The recall lens and cross-stream contamination: that is row `tail7d-contam` (theseus-stwkb), beside you. Do not touch
  `thread_recall.rs`, theseus-memory `threads.rs`, `turn/cross_step.rs` or `config/threads.rs`.
- `thread_grow.rs` and stored working sets: unchanged under (a). The bench harness's simulated owner (theseus-uxum0)
  is out of scope too.
- Router scoring, `apart_by_dir`, and `home_claim.rs` beyond item 4's report.
- Discord: the approval card renders the reason as given. Change no Discord file.
- Every file of the rows in flight that the wave note names.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. The owner
  needs this row first on #75: the smallest diff that removes the false hold at its cause, plus the tests that pin it.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49: no bump under (a).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (the thread router, the misroute guard, a thread's working set) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- the cause in two lines, the option you picked (a or b) and why, and the base you read roots under;
- what a relative root with no base does;
- the question's old and new text for a title with an article, and proof that a title without one is unchanged;
- what you found in `keep_apart_by_dir` (item 4), and whether you changed it;
- each test, and its plant with the failing line, the red-first run on the base included;
- the keel guard's output and the store format;
- a `docs/status.md` sentence for the integrator;
- how the owner's daemon behaves after the install: a guess-placed turn inside its thread's roots writes with no card,
  and one outside them still asks once;
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
