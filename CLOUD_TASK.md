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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-tui`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `8260dbab` (2026-10-22: release stage 2, which landed the threads lane rounds 1-3b and
the China-origin chain rounds 1-4 on top of install #69's `29f5dcad`); that is your base. Line numbers quoted below are
at `8260dbab` but may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `29f5dcad`** (`git log --first-parent 29f5dcad..HEAD` lists them):
- `bb265d6c` + `8260dbab`: the threads lane (a place's home splits into threads, thread routing with the directory rule
  and the which-thread ask, the place digest, thread moves and growth, the protocol nodes split).
- `38743708`: the China-origin chain (the data rule's open view, the open request, the tool screen, the draft review),
  with join fixes `c0d35275` (a session's data state is store format 49) through `30d6472e`.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail3` (`e29be1ed`) and `integ/threads` round 4 (`a52cac7b`) are being joined into the next release stage.
  Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read their diff
  (`git diff origin/main origin/integ/tail3 -- <file>`). Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail4-tui, herdr up shows what it built, the board's cancel key, the chat footer's job line, the answer card's walk order (theseus-v54l9, theseus-tlhjt, theseus-hwv3t, theseus-dowi0.1)

Branch: `cloud/20261021-tail4-tui`. Every commit's subject carries the Beads id of what it changes. Deadline for the
report: 3 hours after you start.

**Why.** Four small things the owner (or a dry run of his walkthrough) hit on install #69.
- **theseus-v54l9 (herdr up, the owner's live walkthrough).** From a plain terminal the owner ran
  `theseus herdr up --follow`; it printed "made the main tab… made the board tab…" and returned. The owner: "Nothing
  happened". herdr had in fact built a workspace `theseus` (main, board) in the owner's running herdr, but focus stayed
  on the workspace the owner was in. Focusing it by hand (`herdr workspace focus <id>`), the owner: "This is hot".
  Wanted: after `up`, focus the theseus workspace (unless `--no-focus`) and print
  `herdr: switched to workspace <n> (theseus)`; when no herdr client is attached, say how to attach; and since
  `--follow` returns at once (the board tab is the follower), say so in the message.
- **theseus-tlhjt (TUI board's `c`).** (1) `c c` on a task that already finished prints "cancelled task 490f10": a
  cancel that cannot have happened. Wanted: for an ended task, no prompt, a flash "that task has already finished".
  (2) While a task's detail view is open, `c` always takes the open task, not the row under the cursor, and nothing
  says which until the prompt names it. After `l` (follow, which took the cursor's row) and Esc, `c` took the open task
  instead; moving the cursor did not change it.
- **theseus-hwv3t (chat footer).** `theseus chat`'s task footer froze a job's last line: for over two minutes it read
  `783e8f … job 2m › census step 2/9 04:33:04` while `theseus tasks` showed `census step 7/9 04:34:44` for the same job,
  and the TUI's follow pane updated live. The elapsed time and cost update; only the `› last line` stays at the first
  line or two the chat saw.
- **theseus-dowi0.1 item 2 (herdr answer card).** The card says "3 more wait: a53b2e …" in one order, but `tab` walks
  the questions in another (longest waiting first): tab moved to 7fcee4 though a53b2e was listed first. (Item 1, Tab
  typed into the line, is already fixed on your base: `decide` takes a Tab-only line and the word `tab` as
  `Decision::Next`.)

**Risk: low.** Four small client-side fixes; the one daemon-side candidate (hwv3t) is a position or scope rule.

**Prerequisite: base `origin/main` at `8260dbab`.** Your branch joins the new lane `integ/tail4`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-tui` | theseus-v54l9, tlhjt, hwv3t, dowi0.1 | focus after up; `c` on ended tasks and on the cursor's row; the footer's live job line; listed order = walk order | `crates/theseus/src/herdr_up.rs`, `herdr_up/wire.rs` (a `workspace.focus` call's params), `crates/theseus/src/herdr_contract_tests.rs` (the new call against the schema), `crates/theseus/src/herdr_answer.rs` (`context`'s `others`), the CLI's `herdr up` args (search `no_focus` / the `Up` subcommand), `crates/theseus/tests/herdr.rs` (only if a test there pins the printed lines); `crates/theseus-tui/src/app.rs` (`arm`, `armed_key`, `on_screen`/`shown`), `crates/theseus-tui/src/tests.rs`; for hwv3t whichever of `crates/theseus-tui/src/chat/tasks.rs` (`work_changed`) or `crates/theseus-core/src/push/` the cause is in, + one test; `docs/herdr.md` (one sentence) | M |

## What exists (at `8260dbab`; search by name)
- **herdr up.** `crates/theseus/src/herdr_up.rs` (644 lines): `base_tabs` prints "made the main tab: the home
  conversation" / "made the board tab: …" (near :196) via `say(ctx, ..)`; `open_tab(ctx, spec, focus)`; the workspace
  is found or made through `workspace.list` (near :133) and `wire::workspace_params()` (`workspace.create`). herdr's API
  is pinned by `tests/fixtures/herdr-0.9.1-api-schema.json` and `herdr_contract_tests.rs` (`valid(&s, method, params)`):
  check that `workspace.focus` (or what herdr 0.9.1 names it) is in the schema, and its params, before calling it; if
  herdr has no such method, say so and only print where the workspace is and the key to switch.
  "No client attached": find what herdr's API says about clients (search the schema for `client`); if nothing, print
  the attach command whenever focus could not be confirmed.
- **The board's `c`.** `crates/theseus-tui/src/app.rs`: `KeyCode::Char('c') => self.arm(Arm::Cancel)` (near :1288);
  `arm` (near :1341) takes `self.on_screen()` (the open session, else "open a session first") and refuses a
  conversation; `armed_key` (near :1550) sends `method::TASK_CANCEL` with `{ "task": sid }`, and the reply's handler
  prints "cancelled task …" (search `Purpose::Cancel`); `armed_words` (near :1823) is the footer prompt. A view's state:
  `self.board.view(&sid)` (search the WorkView state field, e.g. `ended` / `state`). `cursor(rows)` (near :1818) is
  the row under the cursor.
- **The footer.** `crates/theseus-tui/src/chat/footer.rs` `footer_rows` -> `theseus_client`'s
  `render::task_tree::footer` (source: `crates/theseus/src/render/task_tree.rs`; near :221-235 the running job's
  `doing.last_line`). The chat's board: `crates/theseus-tui/src/chat/tasks.rs` `watch_with` (roots: this conversation +
  its thread sessions, `interactive: true`) and `work_changed` (near :127), which **skips a view whose position is not
  newer than the one it holds** (`prev.is_some_and(|p| p.position >= v.position)`). The daemon's sampler:
  `crates/theseus-core/src/push/last_line.rs` (`sample_job` -> `self.work...sample(id, line, now)` ->
  `core.bus.publish_work`); the scoped cut: `crates/theseus-core/src/bus.rs` `publish_work` (`w.scope.cut`).
  **First hypothesis to test:** a sampled line re-sends the job's view without a newer `position`, so the chat drops it,
  while `theseus tasks` (a fresh snapshot) and the follow pane (its own stream) see it. The second: the scoped cut
  drops a job view of a task session the chat's roots do not name. Write the failing test for whichever holds.
- **The answer card.** `crates/theseus/src/herdr_answer.rs`: `queue(qs)` (near :88, the walk order: longest waiting
  first); `context(conn, qs, shown)` (near :394) builds `others` in `qs`' raw order; `Decision::Next` (near :521) walks
  `queue` from the shown one.

## What to build
1. **theseus-v54l9.** After `base_tabs`, focus the `theseus` workspace (new flag `--no-focus` skips it) and print
   `herdr: switched to workspace <n> (theseus)` (`<n>` as herdr numbers it), or the attach hint. With `--follow`, the
   printed lines say the board tab follows the tasks and `up` returns now. One sentence in `docs/herdr.md`.
2. **theseus-tlhjt.** (1) `arm(Arm::Cancel)` on a task whose view has ended (done, failed, cancelled, stopped) flashes
   "that task has already finished" and does not arm. (2) While the detail view is open and the board's cursor is on a
   different row, `c` takes the cursor's row; with the cursor on the open task (or no board visible), the open task.
   The prompt names the target as today. Keep `s`/`S` as they are unless the same rule is a one-line change (say).
3. **theseus-hwv3t.** Fix the cause your test found, at the smaller end: if the daemon re-sends a view without a newer
   position, the sample bumps the view's position like any change (check what else reads positions: the position rule
   must stay monotone per view); if it is the scope, the cut includes a job of a task under a watched root.
4. **theseus-dowi0.1 item 2.** `others` lists the waiting questions in `queue` order starting after the shown one, so
   the first listed is the one `tab` goes to.

## Tests (each with a planted revert)
- **`herdr_up_focuses_the_theseus_workspace_and_says_so`** (`crates/theseus/tests/herdr.rs`' fake herdr or a unit test on
  the call list): plant: skip the focus call. Plus the contract test of the new call's params against the schema.
- `c_on_a_finished_task_says_so_and_cancels_nothing` (TUI, `tests.rs`): no `task.cancel` call. Plant: drop the check.
- `c_takes_the_cursors_row_while_another_task_is_open`: plant: use `on_screen()` again.
- **`the_footer_follows_a_running_jobs_last_line`**: two samples of one job -> the footer's row shows the second.
  Plant: revert your fix.
- `the_listed_waiting_order_is_the_tab_order`: plant: raw order again.
- `cargo nextest run -p theseus -p theseus-tui` must pass; if you touched core, `cargo nextest run -p theseus-core push`.

## Not in this row
- herdr's own config, the plugin manifest (`contrib/herdr/`), and the owner's running herdr: never touched.
- The Discord flow, voice, thread routing: other rows.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Line ceilings:** `crates/theseus/src/render.rs` is 3,126 / 3,128: add nothing there. `app.rs` is 1,954. Never raise a
  ceiling.
- **Rows beside you on `main` 8260dbab, joining `integ/tail4` with you:** tail4-voice, tail4-discord, tail4-route (core
  `thread_route.rs`, `turn/compile_step.rs`), tail4-tests (core test helpers, `bench/recall`), tail4-onto. Stay out of
  their files.
- **Near you, not yet on main:** `integ/threads` round 4 changed `crates/theseus-tui/src/chat/{events,state,task_view,
  thread_ask}.rs` and the TaskChanged turn filter, and `integ/tail3` changed `crates/theseus/src/open.rs`. Read
  `git diff origin/main origin/integ/threads -- crates/theseus-tui/` before you edit `chat/`, and keep your hunks away
  from its hunks (`chat/tasks.rs` is not in its diff).
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus/AGENTS.md` and `crates/theseus-tui/AGENTS.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; the herdr method you called (or why none) and the new printed lines;
which hwv3t hypothesis held, with the evidence; each test and its plant with the failing line; the keel guard's output;
the store format (unchanged); a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
