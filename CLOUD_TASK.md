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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-turn-budget`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
integration lanes named in each task (`integ/p2-route`, `integ/p2-caps`, and others). Other rows land in the same
lanes from other branches at the same time, some built on another host. Your task names them and the files you share
with them: stay additive (one helper, not two), and leave their files alone.

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
- `crates/theseus-core/src/config.rs` 2,149 (unlisted, cap 2,500): a new config section goes in `config/<name>.rs`.

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
3. The data floor is never loosened: nothing private reaches a model of China origin, and nothing marked private or
   sensitive reaches a model the floor does not clear for it. A row near data verdicts ends its report with a
   "Data-routing review" section that a Claude model reviews.
4. The repository is public: no person's name, no real organisation, address or id anywhere. Fixtures use invented
   names and ids under `example.test`. Write "the owner".
5. Red first: each fix comes with a test that is red at the base and green after, and a planted revert named in the
   report. Real-world behaviour, never a benchmark special case: no task name from any benchmark in code or tests.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2-turn-budget, a turn that knows its time and spend: stop paying on work it is not finishing (theseus-2uy56), and tell long builds the time left and the real CPU count (theseus-yct32)

Branch: `cloud/20261023-p2-turn-budget`. Every commit's subject carries the Beads id it serves (`theseus-2uy56` or
`theseus-yct32`). Deadline for the report: 5 hours after you start.

**Why.** A benchmark retest on 2026-10-23 found two real-world weaknesses (do not special-case any task; the
evidence is only how we noticed):
1. **theseus-2uy56: a turn keeps paying on a task it is not finishing.** One hard build task cost $1.26, $1.30 and
   $1.77 a trial (78% of that run's spend) and timed out at its 15-minute limit every time, as it had two days earlier
   and on every older version. Its container used 0.38 of its 1 CPU: the time went to model turns, not compute. The
   proposal: spend and time awareness inside a turn. When a turn has used most of its time, or a large share of its
   spend, without a passing check, it should replan, narrow, or stop and report what it has, not keep going the same
   way.
2. **theseus-yct32: long builds do not know the time left or the CPU they really have.** Another build task timed out
   at its 40-minute limit with the host 12% busy and its container at 1.77 of its 2-CPU quota (cgroup `cpu.max`).
   Inside a container `nproc` reports the host's 32 CPUs while `cpu.max` allows 2, so a `make -j` sized from `nproc`
   only adds throttling (the container's `nr_throttled` ran into the thousands). The proposal: the turn's tool context
   tells the agent its deadline and time used, and the effective CPU count (`cpu.max` and the cpuset), so long builds
   size `-j` and plan to fit.

**This is a design row ("how should Theseus behave"), on Opus.** Before you write code, write the design in your
report (section "Design", committed first as its own commit): for each of the two issues, 2-3 options, the one you
pick, and why, with what each costs (tokens per loop, prompt-cache effect, model calls) and how it fails safe. Then
build the pick with tests on the fake model.

**No new config value the owner has not approved.** Use defaults in code, documented in core `AGENTS.md` (a named
`const` with its reason), and existing config values where one already says the thing (`[turn]`/`[tools]` caps,
`max_loops`/`on_max_loops`, the kernel's execution deadline `default_deadline_secs`, budgets and carves). If your pick
truly needs a knob, leave it a `const`, and write "OWNER: make <name> a config key?" in follow-ups.

**Risk: medium-high.** The turn loop is the hot path: every loop pays for whatever you add. Nothing here may make a
turn stop on work that is going well, and nothing may break the prompt cache's prefix on every loop.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins the lane `integ/p2-route`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-turn-budget` | theseus-2uy56, theseus-yct32 | the design; a turn progress check (time and spend used, whether a check has passed) that steers the agent to replan, narrow or stop and report; the time left and the effective CPU count in the tool context | core: new module(s) beside `turn.rs` (for example `turn/progress_step.rs`) with at most one call each from the loop; the place a turn's notices are injected (find how `on_max_loops`'s notice reaches the model); the place the tool context or the system prompt's stable environment facts are built; a small `cpu` reader (extend `crates/theseus-core/src/cpu.rs` or the tools crate where commands run, whichever runs where the commands run); tests in new `tests_*` files; core `AGENTS.md` (a new short section); `docs/` if a user-facing behaviour is documented there | L |

## What exists (at `23328545`; search by name; read before designing)
- **The turn loop.** `crates/theseus-core/src/turn.rs` (at 3,469 of 3,525 lines: add at most the `mod` line and one
  call per new step) and its steps in `crates/theseus-core/src/turn/` (`*_step.rs`; read `checkpoint_step.rs` and
  `effort_step.rs` as models of a step that watches the loops).
- **Loop caps.** `[turn] max_loops` and `on_max_loops` (`MaxLoopsMode`) in `crates/theseus-core/src/config.rs` near
  :1084: what happens at the cap today, and how its notice reaches the model, is the closest existing shape for a
  "stop and report" steer.
- **Deadlines and spend.** The kernel's execution deadline (`KernelConfig::default_deadline_ms` from
  `default_deadline_secs`, config.rs near :812 and :909), task budgets and carves (`Kernel::charge_execution`, the
  task's budget), the daemon's spend line (`spend_limit_usd`). Find what a conversation turn and a task's turn each
  know of their own time and spend at a loop boundary.
- **Checks.** What counts as "a passing check" must come from what the turn already sees: a tool result's exit
  status, a test command's result, the checkpoint's `Tally` (`crates/theseus-core/src/routing/checkpoint.rs`:
  `Trigger::Failing`, `Quiet`) records failing loops. Read it; do not edit it (see "Shared").
- **CPU.** `crates/theseus-core/src/cpu.rs` uses `std::thread::available_parallelism`. Commands run through the tools
  crate (`crates/theseus-tools/src/proc.rs`) and possibly a sandbox (`crates/theseus-sandbox`): find where a command
  runs, because the CPU quota that matters is the one there (the container's, not the daemon's). cgroup v2:
  `/sys/fs/cgroup/cpu.max` is `"<quota> <period>"` or `"max <period>"`; the cpuset is
  `/sys/fs/cgroup/cpuset.cpus.effective`. Read through a path parameter so tests use a fixture directory.
- **The prompt cache.** The system prompt and tools are a cached prefix (core `AGENTS.md`, the compiler and
  keep-warm). Stable facts (the CPU count) may sit in the prefix; changing facts (time used) must not, or every loop
  rewrites the cache. A per-loop note belongs after the cached prefix (find how other per-loop notices are added).
- **Locked flow.** `crates/theseus-discord/src/golden/dm_flow.txt`: if a turn that stops early ends with a message the
  DM task flow shows, that message's wording, layout, order and timing must not change.

## What to build (after the Design section is committed)
1. **The progress check (2uy56).** At a loop boundary, a pure function of what the turn already knows (time used
   against its deadline, spend against its budget or a share of it, loops since the last passing check, failing loops)
   decides one of: nothing; a steer note to the model (replan or narrow: say what is left and what has not passed); a
   last loop to stop and report what is done, what is not, and why. Defaults are named `const`s with their reason. The
   note is short, plain and general (no tool, language or benchmark named). A turn whose checks pass keeps going: the
   check fires only without a passing check. Ledger: one row when the check steers or stops (`turn.progress` or the
   shape the loop's other notices use), with the numbers that fired it.
2. **The time left (yct32).** When the turn has a deadline, the model sees the time left at the loop boundary in the
   per-loop place (not the cached prefix), rounded so it does not change every second (for example to the minute).
   When the turn has none, nothing is added.
3. **The effective CPU count (yct32).** `min(cpu.max quota / period rounded up, cpuset count, available parallelism)`
   where commands run, shown once among the stable environment facts ("CPUs available to commands: 2 (cgroup quota;
   the host shows 32)"), so the agent sizes `-j`. If no cgroup limit applies, show the plain count. Never fail a turn
   on an unreadable file: fall back to `available_parallelism`.
4. **Docs.** Core `AGENTS.md`: a short section for the progress check (its defaults and why), the time line and the
   CPU fact; `docs/` where turn behaviour is documented, if it is.

## Tests (red first: write each, see it fail on the base code, then fix; each behaviour change gets a planted revert)
- **The pure check:** a table over (time used share x spend share x loops since a passing check x failing loops):
  nothing while checks pass; a steer at the first threshold; a stop-and-report at the second; never a stop when the
  last loop's check passed.
- **On the fake model (the turn rigs of `tests_turn_checkpoint.rs`/`tests_route.rs`):** a turn whose loops keep
  failing, on a paused clock that advances past the steer threshold, gets the steer note in its next request (the fake
  provider sees it) and the ledger row; past the stop threshold, the turn's next request is its last and asks for the
  report; a turn whose checks pass past the same time gets no note; the cached prefix is byte-identical between two
  loops of one turn (the time line is outside it).
- **CPU:** fixture cgroup files: `200000 100000` gives 2; `max 100000` with a cpuset `0-3` gives 4; unreadable files
  give `available_parallelism`; the environment fact names the quota.
- **No regression:** the existing `max_loops` tests, the checkpoint tests and the Discord golden stay green.
- Run `cargo nextest run -p theseus-core turn`, `-p theseus-core checkpoint`, `-p theseus-tools`, `-p theseus-discord`,
  then the gate.

## Shared with other rows (stay additive)
- **Lane `integ/p2-route` also gets** `p2-effort-max` (theseus-ujzuh, theseus-u5a37: it edits
  `routing/checkpoint.rs`'s `text` and the checkpoint's ask in `turn/checkpoint_step.rs`, and `routing/effort.rs`,
  `turn/effort_step.rs`) and `p2-route-data` (theseus-f9qa8: the route pack's `data` question, a new `route.v8`). Do not
  edit any of those files. If your design wants the route checkpoint to see time or spend, write it as a follow-up for
  the integrator (one line it would add), not as code.
- **Bench tooling** belongs to another row (`p2-bench-driver`, Python under `bench/`): if a benchmark driver should pass
  a deadline to a turn, name it as a follow-up; do not edit `bench/`.
- Core `AGENTS.md`: your own new section; the other rows edit the effort and data sections.
- Off limits as well: `toolrun.rs` (at its cap: add nothing), `render.rs`, Discord runtime code.

## Not in this row
- Any new config key, a raised cap or budget, a benchmark run, any model spend.
- Changing what a check is for the checkpoint (`Tally`), the effort step, routing.

## Rules for this row
- Model roles: you design and write the code; keep it minimal and elegant. Every line you add should earn its place.
- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
  (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt`). If your design would change
  any message of that flow (wording, layout, order or timing), stop that part and write "OWNER CALL: <what and why>,
  before/after text" in your report instead.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Line ceilings:** `turn.rs` the `mod` line and one call per new step at most; `toolrun.rs` nothing. **Dependencies:**
  none new. **Store format** 49: no bump expected (the counters live on the turn).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Names.** No person's name anywhere: write "the owner". No benchmark task name in code or tests.
- Read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (the turn loop, the cache, checkpoints) and
  `crates/theseus-tools/AGENTS.md` first. Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- "Design" first (the options, the pick and why, for each issue; the defaults and their reasons);
- what you built, item by item (1-4), with the file and function for each, and a sample of the steer note, the time
  line and the CPU fact as the model sees them;
- the cache check (where each new text sits and the test that proves the prefix is stable);
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output, the store format, the `turn.rs` lines you added, the Discord golden's
  `git diff --exit-code` result;
- a `docs/status.md` sentence for the integrator;
- follow-ups (OWNER items for any knob; what to measure on the next benchmark run: spend on unsolved tasks, timeouts,
  throttling).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
