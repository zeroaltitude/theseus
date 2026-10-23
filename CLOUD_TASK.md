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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-dispatch`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `23328545` (2026-10-23: release 1.0.1, install #76). That is your base. Line numbers
quoted below are at `23328545` but may be off by a few: search by name. The release preamble above is older than this
note; where they disagree about `main`, ceilings or the work in flight, this note wins.

**This wave is patch2 (release 1.0.2): the follow-ups beside the Operations page.** Rows start on `23328545` and join
integration lanes named in each task (`integ/p2-threads`, `integ/p2-misc`, and others). Other rows land in the same
lanes from other branches at the same time. Your task names them and the files you share with them: stay additive (one
helper, not two), and leave their files alone.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Prefer a META key
or a serde-default field. If your row adds a field or a variant to a stored record, bump it to 50, add the old layout's
literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and say so in your report's first
section. Several rows may bump; the maintainer renumbers at the join. Never write the number as a literal anywhere but
the constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-core/src/toolrun.rs` 2,500 / 2,500 (unlisted, at the cap): add nothing to it.
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733: a new type goes in a module file; `lib.rs` gets at most a field
  line or a method constant.
- `crates/theseus-core/src/turn.rs` 3,469 / 3,525: add nothing; put turn logic in a module beside it.
- `crates/theseus/src/render.rs` 3,126 / 3,128 and `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500: add nothing.

An unlisted file stays at 2,500 lines or fewer. Never raise a ceiling yourself (the keel guard flags it). If a line
you truly need will not fit, leave the file one line over and say so in your report's first section.

**The owner's rules for this wave** (each task repeats the ones it touches):
1. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
   (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt` before your report). If your
   fix would change any message of that flow (its wording, layout, order or timing), stop that part and write
   "OWNER CALL: <what and why>, before/after text" in your report instead.
2. Keel: no removed or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py` or its
   tests. Run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` before the report and paste its
   output (it must be empty).
3. The repository is public: no person's name, no real organisation, address or id anywhere. Fixtures use invented
   names and ids under `example.test`. Write "the owner".
4. Red first: each fix comes with a test that is red at the base and green after, and a planted revert named in the
   report.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2-dispatch, a message that asks for tasks gets the tasks tools, so the Discord flow's step 1 starts tasks every time (theseus-gg78p)

Branch: `cloud/20261023-p2-dispatch`. Every commit's subject carries `theseus-gg78p`. Deadline for the report: 3 hours
after you start.

**Why.** A walkthrough dry-run of install #75 (a scratch daemon, a stand-in Discord, the owner's config, Sonnet 5.5)
ran the owner's Discord step-1 words in a bound DM: "Start two background tasks, $1 each: A) bash
~/projects/walkthrough/slow.sh census 3; B) slow.sh load 3". Sometimes that starts two plain `proc.run` background
jobs (j1, j2) instead of two tasks: then there are no forum posts, no 🧵 lines, no board rows, no "🔔 started task"
lines, and the reply says "I can't set a per-task $1 cap … these are plain shell jobs rather than model tasks".
- Counts on clean scratch state, same config: install #75: jobs in 2 of 6 runs (the first two), tasks in 4 of 6;
  install #74a: jobs in 1 of 3. Not a regression; an older run saw the same with Haiku in `theseus chat`.
- **Cause, from the ledger:** the `tasks` family is deferred in a DM (`tools.used.deferred` holds `tasks`). In the
  failing runs the model never searches (`searched: []`) and takes the loaded `proc.run` with `background: true`. In
  the good runs it searches "create background task with budget" and finds `task_create`.
- Fix candidates from the issue: load the `tasks` family when a message asks for a task ("task", "background task"),
  or name `task_create` in the deferred catalog line for background work.

**This is the locked Discord flow's own entry point.** The owner locked the DM task flow (golden
`crates/theseus-discord/src/golden/dm_flow.txt`). The fix makes those words start tasks exactly as the locked flow does
today: the golden stays byte-identical, and gains a new case only if the misfiring words need one (and then only a new
case appended in the golden's own style, never a changed line; if the golden's test cannot take a new case without
changing existing lines, write "OWNER CALL" in the report instead).

**Risk: low-medium.** Which tools a first request carries changes for some messages; the DM's defaults do not.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins lane `integ/p2-threads`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-dispatch` | theseus-gg78p | a message that asks for a task loads the `tasks` family for that turn, with or without tool search; the deferred line points background work at `task_create` | core `tools_defer/want.rs`, `tools_defer/family.rs` (the family lines, near :35), `turn/families_step.rs` / `turn/deferred_step.rs` (where a turn's admitted families are chosen), their tests (`tests_families.rs`, the `want.rs` tests); core `AGENTS.md` (the deferred tools section) | S-M |

## What exists (at `23328545`; search by name)
- `crates/theseus-core/src/tools_defer/family.rs`: the family lines (`("tasks", "tasks and sub-sessions, and the task
  graph")` near :35), the word-to-family map (`"task" => "tasks"` near :127), the wire map (`("task_create", "tasks")`
  near :312), and `PlaceKind::defaults` (near :254): **the DM defers `tasks` by the owner's rule of 2026-10-14
  (theseus-rwpbg.3): keep that default exactly.**
- `crates/theseus-core/src/tools_defer/want.rs`: `named(ask, hidden)` scores hidden families a message names; `PLAIN`
  lists everyday family names that do not count (`tasks` is one). Its module doc says a model **with** tool search
  never reaches it ("its first request is as it was"). Sonnet 5.5 has tool search (`catalog.rs`), so for the owner's
  model nothing in `want.rs` runs today.
- `crates/theseus-core/src/turn/families_step.rs` and `deferred_step.rs`: where a turn's loaded families are chosen;
  Jev's `families.v1` judgment admits families it finds relevant (theseus-xlri5); `By` in `want.rs` records who
  admitted one.
- `proc.run`'s `background` input (search `crates/theseus-core/src/tools/` for `background`): its description is what
  the model reads when it takes the job path.
- `crates/theseus-core/src/tests_tasks_deferred.rs` and `tests_families.rs`: rigs for a deferred tasks family.
- The DM golden's rig (`crates/theseus-discord/src/golden/` and the test that writes it: search for `dm_flow.txt`).

## What to build
1. **Red first.** A core test with the DM place kind, a model **with** tool search, and the step-1 words above (with
   invented paths): the first request's tools do not hold `task_create` at the base. Then the same for a model
   without tool search (want.rs's path), to show whether `PLAIN`'s `tasks` blocks it there too.
2. **The fix, smallest that holds for both kinds of model.** A message that asks for a task (the words "task" or
   "tasks" next to a verb that starts or makes one, "background task", "start … tasks") admits the `tasks` family for
   that turn, recorded with its `By` (a new variant if none fits). Keep it narrow: "tasks" as an everyday word in
   another sense ("my tasks today", "what tasks are open") must not load it; put those cases in the test table. Where
   the rule lives: the one place both kinds of model pass through (likely `families_step.rs`), not a copy in each
   path. The DM's default list does not change.
3. **The catalog line.** If the deferred family's one-line description is what the model reads when it decides not to
   search, make it say background work with a budget goes through `task_create` (one line in `family.rs`), and make
   `proc.run`'s `background` description say a background job has no budget and no card (a task does), only if that
   text is not already on the wire in a golden. Measure the first request's size change for the DM (the tasks family
   is about 6,000 tokens): admitted only on the asking turn, it should not move other turns.
4. **The locked flow.** `git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt` must be
   empty. If the golden's rig uses a scripted model, the fix cannot change it; say so in the report.
5. **Docs.** The deferred tools section of core `AGENTS.md`: one paragraph on when a turn's words admit `tasks`.

## Tests (each red at the base, with a planted revert; the first is the key test)
- **`a_dm_message_asking_for_background_tasks_loads_the_tasks_family`** (tool search on). Plant: remove the rule.
- The same without tool search. Plant: put `tasks` back in `PLAIN` (or whatever the fix moved).
- **`everyday_task_words_do_not_load_the_family`**: a table of messages that must not admit it.
- **`the_dm_defaults_still_defer_tasks`**: `PlaceKind::Dm.defaults()` unchanged, and a DM message with no task words
  still defers it.
- Run: `cargo nextest run -p theseus-core families deferred want`, `-p theseus-discord`, then `-p theseus-core`
  whole.

## Not in this row (other rows in lane `integ/p2-threads`, landing at the same time)
- **p2-threads-cards** (theseus-nde5n, theseus-zcn80): the TUI chat's events and the needs-you plain words. Not yours.
- **p2-threads-recall** (theseus-n6mkd, theseus-7nx13; built on another host, branch `lane/p2-threads-recall`): thread
  recall and the thread router. Not yours.
- The effort and routing rows of lane `integ/p2-route` (theseus-ujzuh, theseus-u5a37, theseus-f9qa8, theseus-2uy56):
  `routing/*`, the effort step and the turn budget. Do not touch routing.
- `crates/theseus-core/src/turn.rs` is near its ceiling: add nothing to it.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling (the DM's request budget
  included: the tasks family admitted for one turn must fit the way `Fit` already decides). A cloud row cannot carry a
  `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; nothing here is stored.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (deferred tools, families) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the cause as you confirmed it (both kinds of model); where the rule lives and why there; the word
table; the first request's size for the step-1 words before and after; the DM golden's diff (empty) and why the rig
cannot see the fix (or the OWNER CALL); each test and its plant with the failing line; the keel guard's output; the
store format; a `docs/status.md` sentence for the integrator; follow-ups (for example a live check of the step-1 words
on the next install, 6 runs).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
