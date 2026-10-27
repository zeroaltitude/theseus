<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, the owner's machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold). Export `CARGO_INCREMENTAL=0`: incremental artefacts are most of a target directory's size and this VM has no room to spare.
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (verified at your base), so none retries: rerun a failing one alone, and name it in the report.
  - theseus-core: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up`; `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`; `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`; `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits`; and `term::tests::a_close_leaves_no_child_behind` (a negative assertion: keep its output);
  - theseus-core's aws/hands `runaway_mode_refuses_at_ten_times_the_hours_line` and `ten_times_refuses_and_a_cancel_still_runs` fail when a run crosses an hour boundary (they read the wall clock's hour, theseus-5a50): rerun them mid-hour;
  - theseus-kernel's `children::a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` (a count of how processes were classed, so its failure is a finding) and theseus-aws-catalog's `one_service_decodes_in_under_5_ms`;
  - **theseus-discord is broadly flaky under parallel load** (theseus-a0kte, measured 2026-10-27): five distinct tests red across six runs with none repeating, at the same rate on plain main as on a merge. If a theseus-discord test reds once, rerun it alone and record both results; a single red there is not by itself a regression. Named in that measurement: `tests_default_forum::a_dm_task_with_a_channel_gets_an_index_card_linking_it`, `tests_task_home::a_voice_places_task_posts_nothing_in_the_voice_chat`, `tests_forum_archive::a_read_post_a_day_over_is_archived_and_locked_at_the_sweep`, `tests_task_react::pause_parks_a_running_task_without_a_report_and_resume_lets_it_report`;
  - theseusd: `bench_profile::a_first_byte_timeout_is_retried_inside_the_headless_turn` (a count: keep its output), and `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`.
- A negative assertion ("nothing of X reached Y", "no process is left", "no retry") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. Any failure outside the lists above is yours to explain.

**What main holds.** You clone main at `c36c437e` or later, with **store format 52** (`MANIFEST_FORMAT` in crates/theseus-store/src/store.rs:692 -- verified at your base; check it yourself before you bump it). Your clone has v1's milestones and everything the patch1, patch2 and patch3 bundles have joined, including, in the last two days: content flags (`9470df70`), the recall refill pin (`47b94066`), "Not this" -- the feedback leg of notify-work-feedback (`79a5f2d7`, theseus-o1h3f.2), one definition of a content item (`9cc63f93`), the RSI door's bounded git fetch (`5342a01c`, theseus-6vlr4), the static fan-out cue (`71b105aa`, theseus-z9mze.4), and the three rival-arm gauntlet cells with their docs (`f3fdd4eb`), and the Benchmarks CARD view hotfix (`c36c437e`, theseus-s5qwv, merged 2026-10-27 13:43).

**Your base, and the fact that it moves.** Your base is `origin/main` at `c36c437e` as this prompt was written, on 2026-10-27 at about 13:5x MST. **Main may have moved by the time you are fired, and that is normal.** A base one or two merges stale is expected. **Do not rebase and do not merge main into your branch** -- the maintainer merges. Instead, at your start, read `git log origin/main` and say in your report what landed after your base. Re-verifying every `file:line` in this prompt against your own clone is step 0 of your task, below.

**Other changes in flight.** These are being built elsewhere and join `main` one at a time while you work. Stay out of their areas unless your task needs it; if it does, keep the edit small and say so in the report.
- `integ/p3z-wave4b`: three wave-4 cloud rows being integrated on the owner's machine -- theseus-o1jnt (two x86-only reds), theseus-zsddv (scripts/gate.sh: `phase suite` moves to the end of `machine_checks`) and theseus-6s7ll (a Resume button on the self-build halt note, theseus-discord). It may land before or after you start.
- Two P0 cockpit hotfixes the owner asked for on 2026-10-27. **theseus-s5qwv** (the Benchmarks **card** view: stale and duplicate cards, junk headlines, a ragged grid) **LANDED at `c36c437e` and is in your base**; it changed `cockpit/src/lib/bench.ts`, `cockpit/src/lib/benchcards.ts`, `cockpit/src/lib/benchindex.ts`, `cockpit/src/components/BenchIndex.tsx`, `cockpit/src/views/Benchmarks.tsx` and three tests. **theseus-6ln2q** (the Graph view's collapsed thread groups overlap -- `cockpit/src/ops/graph.ts`, `GraphNode.tsx`) is still being written on the owner's machine. If your task touches the cockpit, read those paths before you edit near them.

**Shared files, and how to stay mergeable.**
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). **It must not move unless your task says to move it.** If your task genuinely adds a stored field, variant or record kind, bump it by one from main's number as you cloned it (**52** at your base) and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says; say why in the report. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line. **Measured at your base: `crates/theseus-core/src/turn.rs` is 3,512 lines of its 3,525 ceiling -- thirteen lines of headroom -- and `crates/theseus-protocol/src/lib.rs` is near its 2,733 ceiling. Two branches can each stay under a ceiling and break it once merged**, so if your change adds to either, say in the report exactly how many lines you added.
- **If you add a `mod` line, put it inside the sorted block.** Wave 4 lost a gate to a `mod` line appended after the sorted `mod` declarations: it died in `cargo fmt --all --check` at eight seconds, and no reading review would have caught it. **Run `cargo fmt --all --check` yourself before you report**, not only before you commit, and paste its result.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test. Regenerate it if you add or change protocol types. `git diff --check` reports trailing whitespace in protocol.gen; that is expected and is not yours to fix.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh) and count the commit green. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you would grow a file past its ceiling, split it: never raise a ceiling (the keel, below).

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, claude-sonnet-5-5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/BRANCH`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- **First, before any of that: push a start commit.** As soon as your setup and first build finish, make an empty commit (`git commit --allow-empty -m "session started"`) and push your branch. Do this even though you have nothing proved yet, and do it before you begin the work. **Why it is mandatory and not merely advised:** on 2026-10-24, three of sixteen rows ended with nothing on their branch but the task commit, and because every other row had pushed only at the very end, a session that died at minute 5 and one that died at minute 110 left byte-identical branches -- so there was no way to tell a row that never started from one that worked for two hours and lost it. A start commit makes those two cases distinguishable, and it costs one second.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, the owner's machine, accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is genuinely needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. **This is not a formality and it has cost this bundle real time:** two waves ago a sibling row shipped real people's names in its test fixtures, and clearing them was a blocking join fix that held the merge. Grep your own diff for names before every commit.
- **Scrub over the range, not the tip.** When you check your branch is clean of names, check **every commit's tree and every commit's message** across your whole range (`git log --format=%B origin/main..HEAD` and `git diff origin/main..HEAD`), not just the working tree. A cleanup commit placed on top of the names it cleans still leaves them in the public history, and that is exactly how a name reached a public repository in this project once already.
- Don't edit the spec (docs/spec/), docs/status.md, the README, docs/benchmarks.md, docs/benchmarks/, or docs/design/. The maintainer writes those at review. Where a doc under those paths should change, say exactly what and where in the report, with the text you would have written.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- **step 0's result: every citation in this prompt you checked, and every one that was wrong.** Name them even when they were right;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and **each planted revert with the exact `left` and `right` values the assertion printed when it failed**;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about;
- the result of `cargo fmt --all --check`;
- **Integrator notes: exactly which of your commits are for main, and which are not.** Name each commit by hash. This section is not optional and its absence sends the branch back.

Then the gate's result, naming each failing test and why. **The last line of CLOUD_REPORT.md must be exactly `CLOUD REPORT COMPLETE`** and nothing after it. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**A well-argued STOP is a good outcome, and it beats a speculative fix.** In the wave before yours, a row came back "the defect does not reproduce, and here is the line-by-line argument for why", with no production change at all. That was the right answer and it was useful. If your reading of the code says the defect described below is not there, or is not there in the way this prompt describes, **stop, write the argument with `file:line` evidence, and do not invent a replacement task.** Do not manufacture a change so the branch has something on it.

**The keel.** The gate's keel phase (scripts/keel-guard.py; its docstring lists the rules) fails a range that deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench budget, a spend or loop cap, or a ceiling in scripts/long-files.txt, scripts/shape.sh or .config/nextest.toml, or edits the guard's own files, unless a commit signed by the owner's key acks it. **You can't sign, so you can't ack.** Rewrite an assertion rather than delete it (a rewritten assertion is not a finding), split a file rather than raise its ceiling, and keep caps. Where your task itself changes a cap or a ceiling, or a test must change its meaning, make that change in a commit of its own whose message says why, and list it in the report under **Keel findings expected** (rule, path, why) -- and because the maintainer's `Keel:` trailer has to quote the owner verbatim, **give the maintainer the exact sentence you would want the owner quoted on.** Run `python3 scripts/keel-guard.py` before each commit and paste its findings into the report, **including when they are empty** -- an empty run is evidence too.

> **The stale-base trap, which bit two sessions in the wave before yours.** A bare `python3 scripts/keel-guard.py` run against a base that has moved prints findings for *other people's already-merged work*, and both of those sessions nearly reported that noise as their own. **Scope the guard to your own range.** Read the guard's docstring for how it selects a range, run it over `origin/main..HEAD` only, and if you see a finding in a file your branch does not touch, that is the trap -- say so in the report rather than acting on it.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Red-first is the rule, not a preference.** A test that would have passed before your change is not a test of your change. For every behaviour you change, show the test **failing at your base** first, then passing on your change, and put both outputs in the report **with the assertion's printed `left` and `right` values**. "It failed" is not evidence; the values are. A step whose report has no red-first evidence will be sent back.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

**Locked surfaces, which no task in this wave may move:**
- **`crates/theseus-discord/src/golden/dm_flow.txt` is LOCKED.** The Discord DM task flow is the owner's, by his explicit standing instruction. That golden must come back **byte-identical**: `git diff --stat origin/main..HEAD -- crates/theseus-discord/src/golden/dm_flow.txt` must be empty. It is never regenerated. A task that seems to need a DM task message changed must **STOP and say so in its report** rather than change it.
- **`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` must not move** unless your task body says to move it.

**No AWS, no spend, no install, no benchmark run.** Do not run `theseus-sim bench` for numbers of record, do not touch any cloud account, and install nothing on any machine.

---
# Your task: the RSI pipeline's own turns are excluded from the task-tools loosening (`theseus-lmyvl`)

## BASE UPDATE, 2026-10-27 14:4x by the patch3 conductor (read this before step 0)

**Your base is `origin/main` = `e461c62d`**, not the `c36c437e` written elsewhere in this file.
`integ/p3z-wave4b` — listed above as "may land before or after you start" — **LANDED**, at
`e461c62d`. `theseus-o1jnt`, `theseus-zsddv` and `theseus-6s7ll` are all in main and closed.

Three consequences, and the first one has already bitten this prompt once:

1. **Line numbers in `crates/theseus-core/src/rsi/*` and `crates/theseus-discord/*` may have
   shifted.** The join changed `rsi/pr.rs`, `rsi/tree.rs`, `rsi/infra.rs`, `rsi/tests_pr/infra.rs`,
   `term/pty.rs`, `term/tests_idle.rs`, `judge/tests_content.rs`, `courier.rs`, `render.rs`,
   `runtime/halt.rs`, `runtime/self_note.rs` and `tests_gateway/self_note.rs`. One anchor in the
   `x1zz8` prompt moved by exactly ten lines (`rsi/tree.rs:180` → `:190`) and has been corrected.
   **Re-verify every citation at YOUR OWN base and report any that are wrong** — that check is
   doing real work, not ceremony.

2. **The gate's locked part now runs the SUITE LAST** (`theseus-zsddv`). The order is registry,
   protocol types, lifecycle, jobs, turn, size, then the suite. So **any instruction in this file
   to "run the phases after the suite yourself" is obsolete: there are none.** If the suite reds
   only on the known cases, the phases before it have already reported. A failed locked part now
   also prints `gate: NOT RUN, no verdict, after the failure in <phase>: <phases>`.

3. **`theseus-discord` is known to be flaky under parallel load** (`theseus-a0kte`, filed today
   with a reproducible recipe). A single unexplained red in `theseus-discord` or in
   `theseusd::...` is **not** automatically yours: re-run it alone before you treat it as a
   regression, and say in your report that you did. A test that passes alone in ~3 s and fails at
   ~105 s in company is that population, not your change.


**Branch: `cloud/20261027-p3y-self-turns-noticed`. Model `claude-sonnet-5-5`. Deadline: 6 hours from your start, by the clock, waits included.**

## Step 0, before anything else: re-verify every citation in this prompt

**Do this first and report the result.** Every `file:line` below was checked against `origin/main` = `f3fdd4eb` and re-checked against `c36c437e` on 2026-10-27 at about 13:5x MST. Open each in your own clone and say in your report what it actually was. **Name every citation this prompt got wrong**, even the ones that were right.

**Three corrections to the row's own notes, found at your base.** They matter, and the third changes the shape of the work:

1. **The author string on the third site is wrong in the row.** The row says the three pipeline turns pass `author` of `self:backlog` / `self:mint` / **`self:join`**. The third is actually **`self:review`** (`crates/theseus-core/src/rsi/join/review.rs:138`). If you write a predicate or a test against the literal `self:join`, it will never match and your test will pass for the wrong reason.
2. **`turn.rs`'s headroom is thirteen lines, not fifteen.** The row says 3,510 of 3,525. At your base `crates/theseus-core/src/turn.rs` is **3,512** lines, and `scripts/long-files.txt:9` gives its ceiling as **3,525**. Thirteen lines.
3. **The author may already be plumbed further than the row thinks, and you must check before you spend a ceiling line.** The row says this "needs `TurnRequest.author` carried onto `Turn` -- a one-line `turn.rs` field add". But **`struct Asked` at `crates/theseus-core/src/turn.rs:474-485` already has `author: String` at `:479`**, with the doc comment *"What a turn's body takes from its request, beside what its `Turn` holds."* `struct Turn<'a>` at `:379` has `continuation: bool` at `:383` and no `author`. **So the author already reaches the turn's body; the question is only whether it reaches the place the predicate is computed.** Determine that first. If `Asked` is in scope at the predicate, you may not need a `Turn` field at all -- and on a file with thirteen lines left, that difference is worth ten minutes of reading.

## The defect

`theseus-o1h3f.3` loosened the task tools onto every turn with no owner input. **The predicate it used was `t.continuation`**, verified at your base:

```
crates/theseus-core/src/turn/defer_step.rs:33:    noticed: bool,
crates/theseus-core/src/turn/defer_step.rs:73:            noticed: t.continuation,
```

and the module doc at `:26` states the intent: *"the work it sees is work Theseus noticed itself (theseus-o1h3f.3)."*

`continuation` is set at `crates/theseus-core/src/turn.rs:1090`: `let continuation = req.input.is_none();`.

**The RSI pipeline's own turns do not qualify**, because all three pass `input: Some(...)`:

- `crates/theseus-core/src/rsi/backlog/tender.rs:109-115`: `self.runner.run(TurnRequest { session, input: Some(s.objective.clone()), target, sink, author: "self:backlog".into(), ... })` -- `input` at **`:112`**, `author` at **`:115`**.
- `crates/theseus-core/src/rsi/mint/run.rs:289-296`: `input: Some(s.objective.clone())` at **`:293`**, `author: "self:mint".into()` at **`:296`**.
- `crates/theseus-core/src/rsi/join/review.rs:131-138`: `input: Some(prompt(ask))` at **`:135`**, `author: "self:review".into()` at **`:138`**.

So `continuation` is **false** on all three, `noticed` is false, and **the tasks family stays deferred on the one pipeline whose whole purpose is Theseus noticing its own work.** The ask in the original row was unqualified; this is the gap it left.

**Why it was left:** closing it needs the author carried to the predicate, and `turn.rs` was at its ceiling, which that row was forbidden to touch twice over. The review marked it NOT BLOCKING for exactly that reason and said it was cheapest done by whoever next holds `turn.rs`'s remaining ceiling lines. **That is you.**

## What closes this row

1. **The turn's author reaches the predicate.** Either a field on `Turn` carrying `TurnRequest.author`, or -- if `Asked` is already in scope where `noticed` is computed -- the existing `Asked.author` at `turn.rs:479`, with no new field. Say in the report which you chose and why.
2. **The `noticed` predicate is also true when the author starts `self:`.** Not a list of the three literals: a prefix test, so a fourth pipeline turn added later is covered by construction. `defer_step.rs:73` is the line.
3. **A test per pipeline turn** (backlog tender, mint, join review) that the tasks family is offered -- **red first against the base**.
4. **`turn.rs`'s ceiling respected**, or raised with a `Keel:` trailer quoting the owner (see the keel section).

## The three other sites that read `noticed`, which you must think about

`noticed` is not read only at `:73`. At your base it is also read at:

- **`defer_step.rs:222`** -- `if noticed && ...`, guarding (per the comment at `:219`) *"the model cannot start work it noticed from a tool it cannot see"*;
- **`defer_step.rs:237`** -- `let added: BTreeSet<String> = (noticed && loaded.contains(tools_defer::family::TASKS)) ...`;
- **`defer_step.rs:262`** -- the `dropped` set, below.

**Read all three and say in your report what your change does at each.** Widening `noticed` widens all of them at once. That is probably right -- they are all the same idea -- but it must be a decision you made, not one you discovered afterwards.

## WATCH OUT: the `dropped` set, which is the trap in this row

An earlier integration added a `dropped` set in `turn/defer_step.rs` that drops a loosened family at the next **person** turn. At your base:

```
259:        // The converse of `added`: a person's turn drops a family only the loosening loaded, so the
260:        // append's tools differ from the compilation's and it recompiles without it. Read after the
261:        // message's own admissions, so a family this turn loads is never dropped.
262:        let dropped: BTreeSet<String> = (!noticed && !loaded.contains(tools_defer::family::TASKS))
263:            .then(|| tools_defer::family::TASKS.to_string())
264:            .into_iter()
265:            .collect();
```

**`!noticed` is the test for "a person's turn".** A `self:` turn must not be read as a person's turn by that path. Today a `self:backlog` turn has `noticed == false`, so **it falls into `dropped` and actively strips the tasks family** -- which is worse than merely not offering it. **Widening `noticed` fixes this site too, and you should say so explicitly in the report with the before/after behaviour**, because it is the strongest single argument that the prefix test belongs at `:73` rather than in a separate predicate at `:222`/`:237` only.

**Write a test for this specifically**: a `self:` turn must not drop the tasks family. That test is independent of the three "is it offered" tests and it is the one a future refactor is most likely to break.

The row names two documents as required reading -- `reviews/p3i-task-tools-wake.md` Finding 3, and `local/p3o-joinfix-report.md` fix 1. **Both live on the maintainer's machine and are not in this repository.** Do not spend time hunting for them. Everything they say that matters is in the paragraph above; **if you find either file present in your clone, read it and say so, and if it is absent, say that too** rather than treating its absence as a blocker.

## Red-first

For each of the four tests (three pipeline turns offered the family, plus the `dropped` test):

- Show it **failing at your base** with the assertion's **`left` and `right` values pasted**, then passing on your change.
- Then **plant a revert**: narrow `noticed` back, show each test red again, restore the file and `touch` it, run `git status`. A test that still passes with its plant applied has not been written.
- Use the substring form of the nextest filter and run `cargo nextest list` first to assert it selects exactly the tests you mean. **An exact filter with a bare function name and no module path matches nothing, runs zero tests and exits 0, which reads as a pass.**

Look for an existing rig rather than building one: `defer_step`'s behaviour is already tested somewhere in `theseus-core`, and the three pipeline entry points each have a caller you can model a test on. **If a new test file is needed, make it a `tests_<subject>.rs` of its own and add its `mod` line INSIDE the sorted `mod` block** -- a `mod` line appended after the block dies in `cargo fmt --all --check` at eight seconds, which is how a gate was lost in the wave before yours. **Run `cargo fmt --all --check` before you report and paste the result.**

## The ceiling and the keel

- **`crates/theseus-core/src/turn.rs`: 3,512 of 3,525 at your base.** Count your added lines and report the number. `crates/theseus-core/src/turn/defer_step.rs` is not in `scripts/long-files.txt`, so its ceiling is the default 2,500: `wc -l` it before you add.
- **Prefer the change that spends no `turn.rs` line.** If `Asked.author` at `:479` can reach the predicate, take that route.
- **Do not raise the ceiling in `scripts/long-files.txt`.** That is a keel finding and you cannot ack it. Split the file instead, or spend fewer lines. **Only if neither is possible** do you raise it -- in its own commit, listed under **Keel findings expected**, with the exact sentence you want the owner quoted on in the `Keel:` trailer. The owner's standing position, which is what to quote toward, is that our development guardrails such as line ceilings are adjustable for a real need; write the sentence you want quoted rather than paraphrasing it yourself.
- **The keel itself stays.** This row loosens which tools a self-initiated turn is offered; it does not loosen any guard on what that turn may do.
- Run `python3 scripts/keel-guard.py` scoped to `origin/main..HEAD` before every commit and paste the output **including when it is empty**. A bare run against a stale base prints other people's merged work as findings -- two sessions in the wave before yours nearly reported that noise as their own.

## What will get this row rejected

- A predicate matching the three literal author strings instead of the `self:` prefix.
- A test written against `self:join`, which does not exist (it is `self:review`).
- Leaving the `dropped` set at `:262` unaddressed, or addressing it without saying so.
- Spending a `turn.rs` ceiling line without having checked whether `Asked.author` already reaches the predicate.
- Raising a ceiling in `scripts/long-files.txt` without a Keel findings section and a quotable sentence.
- Red-first evidence without the `left`/`right` values, or a planted revert that did not turn the test red.
- A `mod` line outside the sorted block.
