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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7-recall`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail7-recall, the recall bench's README numbers and its slow drive test, a read-only test that guards as root, the data-gate word from the real route, and keep-warm's telemetry on the deferred path (theseus-5ni7j, theseus-b80si)

Branch: `cloud/20261021-tail7-recall`. Every commit's subject carries its Beads id (`theseus-5ni7j` or
`theseus-b80si`). Deadline for the report: 3 hours after you start.

**Why.** Release stage 6 (`ae420a49`, now on `main`) landed tail6-tests (the recall bench plans a later session at the
recall cap; a read-only test tree is thawed and removed) and tail6-cn (the data-gate word and its counter show only
when the data rule really kept a turn off a restricted-origin model). Their reviews left:
- **5ni7j 1.** `bench/recall/README.md` still quotes the old plan: windows 46000 (smoke) / 94000 (full) for seed 7 at
  13,943, the smoke's budget 30,404, the full's 73,904, and about $0.54 / $12.02 an arm (near :106 and :138-139). The
  plan now gives 56000 and 101000 for seed 7 at 13,943, so the budgets and the dollars moved too.
- **5ni7j 2.** The new drive test `test_the_smoke_compacts_once_however_early_the_index_comes_up`
  (`bench/recall/test_drive.py`) sleeps 8 s before the first turn and runs a whole smoke: slow, though deterministic.
- **5ni7j 3.** `crates/theseus-core/src/scratch.rs` `a_read_only_tree_is_removed` (near :170) passes either way as
  root, because root ignores `0o555`: it guards the thaw only on a non-root runner (the cloud VM and some gates run as
  root).
- **b80si 2.** `crates/theseus-core/src/turn/data_step.rs` `cn_would_win` (near :138) models the route as the mode
  list's first usable pick for a cleared ask. A turn whose real decision was a detour, a switch or a steered pick can
  under- or over-show the footer's data-gate word and its counter. (Display and counter only: the floor itself is
  right.)
- **b80si 4.** `keep_warm`'s telemetry cell (`keep_warm.rs` near :91, read near :690) is set when the core is built
  with telemetry (`rpc/mod.rs` near :920) but not by `build_telemetry` (near :539), the path that builds the pipeline
  after serving once its headers secret resolves. On that path keep-warm's counts (its data-gate count included) are
  never exported.

**Risk: low.** Docs, tests, a display/counter decision, and one telemetry wiring line.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the new lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7-recall` | theseus-5ni7j (1-3), theseus-b80si (2, 4) | the README's numbers from the generator; a faster drive test with the same proof; a root-proof read-only test; `cn_would_win` from the turn's real decision; keep-warm telemetry on the deferred path | `bench/recall/README.md`, `bench/recall/test_drive.py` (and `drive.py` only for a test seam), core `scratch.rs` (its tests, and a seam in `remove` only if the test needs one), core `turn/data_step.rs` (`cn_would_win`, `data_gate_why`, and where `data_floor` sees the decision), the data-gate tests (`tests_data_footer.rs` and the tail6-cn tests: search `a_private_turn_a_us_model_wins_anyway_shows_no_data_gate`), core `rpc/mod.rs` (`build_telemetry`: one line) and a test beside the telemetry tests | M |

## What exists (at `ae420a49`; search by name)
- `bench/recall/generate.py` prints, for a plan, the counts, tokens and dollars per arm at the catalog's price
  (`--model`), each mark's reads and the window and its budget (README near :100-107 says how). `tokens.py`,
  `progression.py`, `test_generate.py`. Run them with the Python the README names; they need no cargo build.
- `bench/recall/test_drive.py`: `test_the_smoke_compacts_once_however_early_the_index_comes_up` (the `time.sleep(8)`).
  `git show c666c38d --stat` and `git log -p c666c38d^2 -- bench/recall/test_drive.py` show what tail6-tests added and
  why (the tender coming up late; the plan revert it catches).
- `crates/theseus-core/src/scratch.rs`: `remove`, `remove_all`, the thaw, `a_read_only_tree_is_removed`.
- `crates/theseus-core/src/turn/data_step.rs`: `data_gate_why` near :124, `cn_would_win` near :138, `note_open`,
  `data_floor` (takes the route's `decision: Option<Decision>`), `t.route.gated`, `t.route.admitted`.
- `crates/theseus-core/src/rpc/mod.rs`: `build_telemetry` near :539 (exports to judge, memory, stops, defer, the gate
  counter), the build path near :910-925 (which also sets `runner.keep_warm.telemetry`).

## What to build
1. **The README (5ni7j 1).** Run the generator for seed 7 at 13,943 (smoke and full) and write its numbers into the
   README in place of the old ones: both windows, both budgets, the logs' sizes and the turn of the second log if the
   smoke's plan names one, and the dollars per arm. Quote the command you ran in the report with its output. Change no
   other README text. (The owner hears about the cost change from the maintainer, not from you.)
2. **The drive test (5ni7j 2).** Make it cheaper with the same proof: replace the fixed 8 s hold with the smallest wait
   that still makes the index come up after the first turn (an event the test waits on, or a hold the test releases),
   or shorten the smoke it runs if the case needs fewer turns. It must still fail under the plan revert tail6-tests
   named (find it in that row's commit message or report on `c666c38d^2`'s history; plant it and show the failure).
   Report the test's wall time before and after.
3. **Root-proof read-only test (5ni7j 3).** The test must fail when the thaw is skipped even as root. For example,
   assert on the thaw itself (the tree's modes are writable again just before the removal, through a small seam), or
   check the modes another way root cannot bypass. Plant: skip the thaw in `remove`; the test fails as root (you are
   root on this VM: say so).
4. **The real route (b80si 2).** `data_gate_why` decides from what the turn's route actually chose where it can: if the
   turn's decision (detour, switch, steered pick, or the plain pick) named a restricted-origin profile that the data
   rule then kept the turn off, the word shows; if the decision named a US profile, it does not. Fall back to the list
   walk only where the turn had no decision (say where that happens). Keep the floor's behaviour exactly as it is: only
   the word and its counter change.
5. **Keep-warm telemetry (b80si 4).** `build_telemetry` also sets `self.runner.keep_warm.telemetry` (one line beside the
   others), so both paths export the same counters.

## Tests (each with a planted revert; the first is the key test)
- **`a_detour_to_a_restricted_profile_kept_off_shows_the_data_gate`** (or the steered/switched case, whichever the
  rig stages most simply; name it for what it shows), plus its mirror: a turn whose decision picked a US profile while
  the mode list's first usable pick is restricted shows no word. Plant: restore the list-walk-only `cn_would_win`.
- The existing `a_private_turn_a_us_model_wins_anyway_shows_no_data_gate` and the tail6-cn data-gate tests stay green.
- A keep-warm telemetry test on the deferred path (`build_telemetry` called on a core built without telemetry; the
  keep-warm cell is set). Plant: drop the line.
- The read-only test as root (step 3). Plant: skip the thaw.
- The drive test (step 2) under its plant. Then `python3 -m unittest` over `bench/recall/test_*.py` (or the runner the
  README names): green, except any red the README or AGENTS.md already names as known; name it.
- `cargo nextest run -p theseus-core data`, `-p theseus-core scratch`, `-p theseus-core telemetry`, then
  `-p theseus-core` whole.

## Not in this row
- **b80si item 1 is an owner call:** Discord's `routed: open` (Discord `runtime/route.rs`) stays as it is. Touch no
  Discord file.
- b80si 3 (the whole-workspace phase on the merged tree): the integrator's gate covers it.
- 5ni7j 4 (regenerating the recall progression before a real run) and 5ni7j 5 (theseusd's one leftover tmp entry):
  measurement and investigation for the maintainer. If you see the tmp entry's cause while you work, say so in the
  report; do not chase it.
- No bench run with a real model (no keys here), and no change to the plan's rules in `generate.py`.
- The other tail7 rows' files: tail7-onto (core `judge/onto/*`, cockpit ontology), tail7-effort (protocol `route.rs`,
  `routing/effort.rs`, cockpit `lib/route.ts`, `protocol.gen`, Discord tests), tail7-threads (the TUI,
  `tests_hold_compile.rs`; it does not touch `route_step.rs` beyond a test seam).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- `rpc/mod.rs` is 1,504 lines: add the one line only.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling (the recall plan's budgets
  are computed, not edited: the README only reports them). A faster test that keeps its proof is not a weakened one;
  show the plant. A cloud row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- **Dependencies.** None new. **Store format** 49; nothing here is stored.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (the data rule, routing, telemetry, keep-warm) and `bench/recall/README.md`
  first. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the generator's command and output and the README lines it changed; the drive test's time before and
after and its plant; how the read-only test now guards as root; how `data_gate_why` reads the real decision and where
it still falls back; the telemetry line; each test and its plant with the failing line; the keel guard's output; the
store format; a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
