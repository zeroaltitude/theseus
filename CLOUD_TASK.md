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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7c-card`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row needs no
bump: the card's state already has `last_run_ms`, a serde-default field. If you find you need a stored field after all,
bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says,
and never write the number as a literal anywhere but the constant and its tests.

**This row is a hotfix.** It joins its own lane, `integ/hotfix-card`, and lands on `main` early and alone, ahead of the
other rows in flight. Keep it small, so it merges cleanly under them.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). An unlisted file stays at 2,500 lines or
fewer. Every file this row touches is far under that: `card/run.rs` 393, `card/tender.rs` 88, `card/tests_job.rs` 739.
Put a large new test module in a new file (one `mod` line in `card/mod.rs`). Never raise a ceiling yourself (the keel
guard flags it).

**Work in flight beside you:** several rows on `ae420a49` join the lane `integ/tail7`, and they touch the ontology
(`judge/onto/*`), effort routing, the TUI and threads, the recall bench, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`,
holds and policy, budgets, the config template, drafts and Discord. None of them touches `card/`. Stay inside the files
your task names. If you must edit a file outside that list, name it and the reason in your report's first section.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7c-card, a full owner card stops its hot loop: a full run records its attempt, scans nothing, and the card tender keeps the run mark its four twins keep (theseus-15kn3)

Branch: `cloud/20261021-tail7c-card`. Every commit's subject carries `theseus-15kn3`. Deadline for the report: 2 hours
after you start.

**Why (P1, seen on the owner's daemon).** Since 2026-10-22 13:33 local time the owner's daemon (install #74, this base)
has written `INFO card: the run finished proposed=0 spent_usd=0.0 today_usd=0.0 stopped="full: 30 proposals are waiting"`
about every 0.65 s: 3,412 lines in 47 minutes, about 130,000 a day. Over those 47 minutes theseusd averaged 78% of a core,
and its RSS grew to 848 MB (it was 127 MB at the install check). The cause:
- `crates/theseus-core/src/card/run.rs` `Core::card_run` (near :128-220) returns early at near :160-162 when
  `open >= cfg.max_open`. That return comes before `job.st.last_run_ms = Some(now)` and its `people_write` near :217-218,
  so a full run writes no state.
- `card/tender.rs` `next_at` (near :21-35) then finds the run due at once: `learning::tender::due` says `missed` when no
  run is recorded since the latest learning hour.
- `learning::tender::tend` sleeps zero and runs the job again. Each run re-scans the archive's evidence (`evidence::scan`,
  near :151) and clusters it (near :152) before it reaches the full check. That is the CPU, the memory, and the journal.

The tender's own module doc (`card/tender.rs` near :6) promises the job is "off once the card is done or `max_open`
wait". The code does not keep that promise.

**The four twins already have the guard; the card's tender lacks it.** Every other job on `learning::tender::tend` keeps
an in-process mark: `ran`, an `AtomicU64` set when a run starts. Its `next` takes the later of the stored mark and `ran`,
so a run that writes no mark is not due again until the next hour. The four are:
- `learning/tender.rs` `learn_after_serving` (near :145-167);
- `consolidate/tender.rs` `consolidate_after_serving` (near :27-47; the dream's guidance runs inside that run);
- `judge/onto/sweep.rs` `onto_sweep_after_serving` (near :73-91);
- `judge/people/sweep.rs` `people_sweep_after_serving` (near :101-118, whose comment says "A run that wrote no mark (a
  failed one) is not due again at once").

`card/tender.rs` `card_after_serving` reads only the stored `State::last_run_ms`. So any card run that writes no state
loops: a full stop, and also any `Err` (`card_model` failing, "a card run is already going" while the owner's own
`card.propose` runs, a store error).

**Risk: low.** The fix touches one job's pacing and moves one check earlier. The job's calls, budget, and proposals are
unchanged.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins its own lane, `integ/hotfix-card`, which lands on
`main` early and alone. Run the keel guard against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7c-card` | theseus-15kn3 | a full run records its attempt and scans nothing; the card tender keeps a `ran` mark like its four twins; tests on tokio's paused clock | core `card/run.rs`, `card/tender.rs`, `card/tests_job.rs` or a new `card/tests_tender.rs` (one `mod` line in `card/mod.rs`), `card/mod.rs` (its doc, only if a line there becomes untrue); core `AGENTS.md` (the owner-card paragraph, one sentence) | S |

## What exists (at `ae420a49`; search by name)
- `card/run.rs`. The module doc is near :1-14 ("The run stops when `[card] max_open` are waiting"). `card_run` is near
  :128: `RUNNING.try_lock`, then `card_expire` (non-dry runs only), then `State::read`, the `day_usd` off check,
  `state::proposals`, `open`, `state::seen`, `evidence::scan`, `cluster::cluster`, `fresh`, the counts into `out`, the
  full check, the model, the batches, and finally `last_run_ms` and `people_write` near :217. `card_model` is near :327.
- `card/state.rs`. `State` (near :23) has `last_run_ms: Option<u64>` (serde default), `done`, and the day's spend.
  `State::record` writes the META `card.state`.
- `card/tender.rs`. `next_at` is near :21. `card_after_serving` is near :39: its `next` closure reads `State`, returns a
  day's wait when `done`, else `next_at(now, learning_hour, every_days, st.last_run_ms)`. Its `run` closure calls
  `card_run(false)` and writes the journal line.
- `learning/tender.rs`. `due` is near :38 and `tend` near :55. Its paused-clock test
  `the_tender_never_runs_within_ten_minutes_of_a_start` (near :254) is the model for driving `tend` with a counting `run`.
- `card/tests_job.rs`. The `rig` is near :35 (a whole core over the invented archive, a `FakeProvider` script).
  `the_queue_is_capped` (near :417, `max_open = 1`, a second `propose` stops "full") and
  `the_nightly_run_waits_out_the_interval` (near :670, `next_at`'s cases) are your neighbours.
- `rsi/sealed/tests.rs` near :242-290 captures `tracing` output with a `MakeWriter` and `with_default`. Reuse the shape if
  you count journal lines; do not import from `rsi`.

## What to build
1. **A full run records its attempt.** For a non-dry run, a run that stops `full` (the early return and the `job.room == 0`
   break alike) sets `last_run_ms = Some(now)` and writes the state, as a run that ran out of clusters or budget does
   today. The next nightly attempt then comes on the normal schedule (`every_days` after this one, at the learning hour).
   Expiry has already run at the top of the run, so the queue is as small as it can be when the run judges it full. A dry
   run still writes nothing.
2. **A full run scans nothing.** Move the full check before `state::seen`, `evidence::scan` and `cluster::cluster`, right
   after `open` is known. A full run's `evidence`/`clusters`/`fresh` counts become 0. First check every reader of those
   fields on a full result: `rpc/card.rs`, the CLI's `card propose` rendering in `crates/theseus/`, the cockpit if it
   reads `CardProposeResult`, and the tests. If an owner-facing dry run shows those counts and the owner would lose
   something real, keep the scan for `dry_run` only and say so. Report what you found either way.
3. **The tender keeps a `ran` mark** exactly as `people_sweep_after_serving` does: an `AtomicU64` stored when `run`
   starts, and `next` passes `stored.max(here)` to `next_at` in place of the stored `last_run_ms` alone. A failed run (an
   `Err`), or any future path that writes no state, is then not due again until the next learning hour. Keep `done`'s
   one-day look exactly as it is.
4. **The doc keeps its promise.** `card/tender.rs`'s module doc and `card/run.rs`'s say, in one clause each, that a full
   run is an attempt: it records itself, scans nothing, and the next comes on the normal schedule. In core `AGENTS.md`,
   the owner-card paragraph gains one sentence (near "`[card] max_open` waiting at most").
5. **The twins.** Confirm, by reading, that the four jobs above keep their `ran` mark and that no `tend` user is left
   without one (`git grep -n 'tend(started'`). If you find a fifth `tend` user without the guard, fix it the same way, with
   a test, and name it in the report. Then look for the same shape outside `tend`: a loop whose `next` reads only a
   stored mark that some early return skips writing. Search the `*_after_serving` spawns (`keep_warm.rs`,
   `aws/tend.rs`, `aws/durable.rs`, `initiative/digest.rs`). Report each one you checked and what it does. Fix one in
   this row only if it is the same bug and the fix is a few lines; otherwise file it as a follow-up in the report.

## Tests (red first: write each, see it fail on the base code, then fix; each fix also gets a planted revert)
- **`a_full_card_records_its_attempt_and_scans_nothing`** (key test). Use the rig with `max_open = 1`, one proposal open
  (as `the_queue_is_capped` makes it), and the stored `last_run_ms` set back past the interval (write `State` with an old
  `last_run_ms`). Then `card_run(false)`: `stopped` starts with "full", `State::read(..).last_run_ms` is `Some` and is no
  older than the run's start, and the evidence count is 0 (or, if you kept the scan for dry runs, a non-dry run's is 0).
  Then `next_at(now, hour, every_days, last_run_ms)` is at least `every_days` out. Plant: drop the new state write. The
  test fails on `last_run_ms`.
- **`a_full_card_does_not_rerun_within_its_interval`**, on tokio's paused clock (`#[tokio::test(start_paused = true)]`).
  Drive the card's real `next`/`run` pair through `tend` and count runs, by any one of:
  - a small seam: `card_after_serving` builds its pair through a `fn` the test can call, and the test wraps `run` with a
    counter;
  - a `#[cfg(test)]` counter on the run;
  - the captured journal line.
  Advance past `AFTER_START` and then a few hours of tokio time. Exactly one run happens; the base code runs it again and
  again. Plant: drop the state write and the `ran` mark together. The count climbs. Note: `next` reads the wall clock,
  which the paused clock does not move. Keep the test's assertions to "one run, not many" within the window, and do not
  depend on the wall clock moving.
- **`a_failed_card_run_is_not_due_again_at_once`**. Make `card_run` return `Err`, for example a `[card] profile` with no
  provider so `card_model` fails, or hold `RUNNING`'s lock in the test. On the paused clock the tender runs it once in
  the window, not again and again. Plant: drop the `ran` mark. The count climbs.
- **`a_card_below_max_open_runs_again_on_its_normal_schedule`**. Start with a full run recorded. The owner answers one
  proposal (`Core::card_answer`, Not this, as the existing tests answer), so the queue is under
  `max_open`. `next_at` with the recorded attempt is the first learning hour at least `every_days` after it (trigger
  `weekly`), not now and not never. Then a `card_run(false)` makes calls again (the fake's script answers), so the job
  was paused by the queue, not turned off.
- **The journal line is not repeated.** Either the paused-clock test counts the `card: the run finished` line once (with
  the captured subscriber), or the run counter stands for it and the report says the line is written once per run.
  Plant: as the paused-clock test.
- Run `cargo nextest run -p theseus-core card`, `-p theseus-core tender`, then `-p theseus-core` whole, and the CLI crate's
  card tests if you touched a reader (`cargo nextest run -p theseus card`).

## Not in this row
- The owner's daemon and its `/etc/theseus/theseus.toml`: you cannot reach them. The 30 open proposals there are the
  owner's to answer.
- `[card]` defaults (`max_open` 30, `every_days`, `day_usd`, `ask_days`): unchanged.
- `learning/tender.rs` `due` and `tend`: unchanged. The fix is in the card's job and its tender, not in the shared loop.
  A shared "ran mark" helper for all five tenders is a nice-to-have. Do not do it here; name it as a follow-up if you
  think it earns its place.
- Every file of the rows in flight on `integ/tail7`: core `judge/onto/*`, `routing/effort.rs`, `turn/*`, `scratch.rs`,
  `rpc/mod.rs`, `rpc/thread_*`, the TUI, the cockpit, Discord, `bench/`, the config template, `scripts/`. If a twin fix
  would touch one of them (`judge/onto/sweep.rs` is the likely one), stop: report it as a follow-up and do not edit it.
- Core `AGENTS.md`: edit only the owner-card paragraph.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. This is a
  hotfix: the smallest diff that removes the loop at its cause, with the tests that pin it.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49: `last_run_ms` already exists, so there is no bump.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (the owner card; the learning tender) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- the cause in two lines, and the fix: where the full check sits now, what a full run writes, and the `ran` mark;
- what reads the scan counts, and whether a dry run keeps them;
- each twin and loop you checked, and what each does;
- each test, and its plant with the failing line, the red-first run on the base included;
- the keel guard's output and the store format;
- a `docs/status.md` sentence for the integrator;
- how the owner's daemon behaves after the install: one journal line at the next attempt, then quiet until the normal
  schedule or until the owner answers proposals;
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
