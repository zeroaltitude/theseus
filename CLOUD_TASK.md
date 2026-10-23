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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7-effort`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail7-effort, the effort ledger's word list on the wire and in the cockpit, a held_high that says whether the answer was sure, and a real stop in the task home's restart test (theseus-t7b96)

Branch: `cloud/20261021-tail7-effort`. Every commit's subject carries `theseus-t7b96`. Deadline for the report: 2 hours
after you start.

**Why.** Release stage 6 (`ae420a49`, now on `main`) landed tail6-t5fix: a held `applied_unsure` answer says
`sticky_unsure` (`routing/effort.rs` `EffortReason::StickyUnsure`, `turn/effort_step.rs` `stick`), and a two-start
restart test proves the task home removes the earlier run's DM board. Its review left:
- **F1 (docs on the wire).** `crates/theseus-protocol/src/route.rs` near :38-43, the `effort_reason` field's doc (and the
  generated `cockpit/src/protocol.gen/TurnRoute.ts`), lists `applied, clamped, unsure, unclear, fixed, no_effort,
  carried, recorded, sticky` but not `sticky_unsure`, `applied_unsure`, `held_high` or `floored`. The list must match
  `EffortReason::as_str` exactly.
- **The cockpit's words.** `cockpit/src/lib/route.ts` near :39 shows the reason with `.replace('_', ' ')`, which replaces
  only the first underscore (fine for every word today, wrong for any word with two). `cockpit/src/lib/routefooter.ts`
  was never checked for a switch over the reason words: check it and say what you found.
- **held_high (from theseus-apwfj, now folded in here as item 4).** `routing/effort.rs` `decide` (near :240-252) gives
  `HeldHigh` for an `xhigh`/`max` answer at or under `high_effort_confidence`, whether or not the answer was `sure`
  (its band reached the question's confirm bar). The row cannot tell a sure answer held high from an unsure one that
  only cleared `effort_confidence`, which `applied` vs `applied_unsure` and `sticky` vs `sticky_unsure` already tell.
- **F2 (test strength).** Discord `tests_task_home.rs` `a_restart_removes_the_earlier_runs_dm_board_once_the_home_stands`
  (near :550): run 1 is never stopped, only `r.core.outbox.stop_sending()`, so its binding can still take gateway events
  from the shared fake while run 2 runs; and `assert!(home != ANA_DM)` is trivially true.

**Risk: low.** A doc list, one new ledger value beside existing ones, display words, and a test.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the new lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7-effort` | theseus-t7b96 (items 1, 2, 4) | the `effort_reason` doc list, regenerated TS; the cockpit's reason words; `held_high_unsure`; the restart test's real stop and a meaningful home assert | protocol `route.rs` (the doc only); cockpit `src/protocol.gen/TurnRoute.ts` (regenerated), `src/lib/route.ts`, `src/lib/routefooter.ts` (only if it switches on reasons), their tests under `cockpit/test/`; core `routing/effort.rs`, its tests and `tests_route_effort.rs`; `crates/theseus-core/config/theseus.example.toml` (the one `held_high` comment line near :1637); Discord `tests_task_home.rs` (the restart test), `tests_gateway.rs` (only a rig helper to stop a run, if one is missing); core `AGENTS.md` (the effort section, one or two lines) | S |

## What exists (at `ae420a49`; search by name)
- `crates/theseus-core/src/routing/effort.rs`: `EffortReason` near :67-104, `as_str` near :106, `applied()` near :155
  (which reasons count as applied), `decide` near :225-252 (the `held` gate and the reason `match`), the tests near
  :417-480 (`held_high` cases).
- `crates/theseus-core/src/turn/effort_step.rs` `stick` near :98 (maps `AppliedUnsure` → `StickyUnsure`; the model for
  keeping the bar visible through a hold).
- `crates/theseus-protocol/src/route.rs` near :38-43 (the doc); `cockpit/src/protocol.gen/TurnRoute.ts` (generated:
  `cockpit/AGENTS.md` and root `AGENTS.md` say how to regenerate it from theseus-protocol's test).
- `cockpit/src/lib/route.ts` near :36-39, `cockpit/src/lib/routefooter.ts`, `cockpit/test/routefooter.test.ts`.
- `crates/theseus/src/render.rs` near :3112 has an `effort_reason` literal in a test (do not touch `render.rs`: it is
  at 3,126 / 3,128).
- `crates/theseus-discord/src/tests_task_home.rs` near :550 (the restart test), `tests_gateway.rs` `Rig::start_over`.

## What to build
1. **held_high_unsure (item 4).** A new `EffortReason::HeldHighUnsure`, word `held_high_unsure`, for an `xhigh`/`max`
   answer held at `high` whose answer was not `sure`; `held_high` keeps meaning the sure case, so existing rows mean
   what they did. It counts as applied wherever `HeldHigh` does (`applied()`); a held one held again by the sticky rule
   keeps its bar the way `stick` keeps `applied_unsure` (check `stick`: if it maps only `AppliedUnsure`, decide whether
   `HeldHighUnsure` held sticky should say `sticky_unsure`, and say which in the report). Same shape as tail6-t5fix's
   `StickyUnsure`: a new value, no new field.
2. **F1.** The `effort_reason` doc lists every `as_str` word, in `as_str`'s order, each with a few words of meaning; a
   test (in theseus-core, where `EffortReason` lives, or wherever both are reachable) fails when a word is missing from
   the doc, if that can be done simply (for example a test that reads the doc string through a const shared with the
   protocol crate is too much: then say so and rely on review). Regenerate `protocol.gen` and commit it.
3. **The cockpit's words.** `route.ts` replaces every underscore (`replaceAll` or a split/join), so `held_high_unsure`
   reads "held high unsure". Check `routefooter.ts`; if it switches on reason words, give the new ones their case.
4. **F2.** The restart test stops run 1 for real before run 2 starts: drop or shut down the first rig's binding (find
   how the rig ends a run: a `stop`, an abort handle, or dropping `r`; add the smallest helper to `tests_gateway.rs`
   if none exists), so run 1 can take no gateway events. Replace `assert!(home != ANA_DM)` with an assert that says
   something: that the board's later draws land in the task home channel `home`, or that `home` is the forum the config
   named. Keep every existing assert of the test.
5. **Docs.** Core `AGENTS.md`, the effort section: one line for `held_high_unsure`. The template's `held_high` comment
   line gains "(held_high_unsure when the answer was unsure)" or the like, on the same line or one more.

## Tests (each with a planted revert; the first is the key test)
- **`an_unsure_max_held_high_says_so`** in `effort.rs`'s tests: `max` at a confidence under the confirm bar but at or
  above `effort_confidence` and at or under `high_effort_confidence` → `held_high_unsure`, ran `high`; the sure case
  still says `held_high`. Plant: map both to `HeldHigh`.
- A whole-turn row test in `tests_route_effort.rs` beside the `held_high` one near :352-390, showing the word on the
  `route.decided` row. Plant: the same.
- Cockpit: `route.ts`'s words for a two-underscore reason. Plant: back to `.replace('_', ' ')`.
- The restart test: show in the report that run 1 is stopped (what you call, and a plant: skip the stop and show the
  rig's event count or a log line proving run 1 still took events, if the rig can show it; else say so).
- `cargo nextest run -p theseus-core effort`, `-p theseus-protocol`, `-p theseus-discord task_home`, then the three
  crates whole; `cd cockpit && npm test && npx tsc -b`.

## Not in this row
- No runtime change in Discord: the DM task flow is locked by the owner (its goldens must pass unchanged). This row
  edits Discord tests and the test rig only.
- `routing/effort.rs`'s bars and defaults (`high_effort_confidence`, `effort_confidence`): untouched.
- t7b96 item 3 (the archive tick test's scope): accepted as is by its review. Leave it.
- The other tail7 rows' files: tail7-onto (core `judge/onto/*`, cockpit `lib/ontology.ts`, `OntologyGrowth.tsx`),
  tail7-threads (the TUI, `tests_hold_compile.rs`), tail7-recall (`bench/recall/*`, `scratch.rs`, `turn/data_step.rs`,
  `rpc/mod.rs`).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- In core `AGENTS.md` edit only the effort section; in the template only the `held_high` comment line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49: a new ledger word is a value in an existing field, not a layout
  change; no bump.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (routing, effort), `crates/theseus-discord/AGENTS.md` and `cockpit/AGENTS.md`
  first. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the new word and where it shows; what `stick` does with it; the doc list; what `routefooter.ts` does
with reasons; how run 1 is stopped now; each test and its plant with the failing line; the keel guard's output; the
store format; a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
