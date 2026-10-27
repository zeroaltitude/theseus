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
# Your task: the clock-only stops become progress-based (`theseus-x1zz8`)

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


**Branch: `cloud/20261027-p3y-progress-not-clock`. Model `claude-sonnet-5-5`. Deadline: 6 hours from your start, by the clock, waits included.**

## Step 0, before anything else: re-verify every citation in this prompt

**Do this first and report the result.** Every `file:line` below was checked against `origin/main` = `f3fdd4eb` and re-checked against `c36c437e` on 2026-10-27 at about 13:5x MST, but a citation that was right an hour ago can be wrong now, and you are the only thing standing between a wrong line number and six wasted hours. Open each path and line in your own clone and say in your report what it actually was. **Name every citation this prompt got wrong**, even the ones that were right. A correction is worth more to the maintainer than a fast commit.

**Three citations in the row's own notes are already known to be wrong and are corrected below.** They are corrected here as examples of exactly what step 0 is for:

1. The row says the tender deadlines live at `rsi/pipeline/tender.rs:850`, `tender/pair.rs:145` and `tender/status_cache.rs:64`/`:76`. **The directory is wrong for all three.** `crates/theseus-core/src/rsi/pipeline/tender.rs` exists but is **81 lines long** and contains no timeout at all. The real files are `crates/theseus-core/src/tender.rs` (**line 850** -- the line number is exactly right), `crates/theseus-core/src/tender/pair.rs:145`, and `crates/theseus-core/src/tender/status_cache.rs:64` and `:76`.
   **There are eight files named `tender.rs` in this repository** (`card/`, `consolidate/`, `learning/`, `rsi/backlog/`, `rsi/join/`, `rsi/pipeline/`, `rsi/revert/`, and the top-level one), plus `theseus-exam`, `theseus-index` and a theseusd test. Do not grep for `tender.rs` and open the first hit. Name the full path in every line of your report.
2. The row says the jailed self-build path is `rsi/jail.rs:100-109`. At your base the region is **`:100-111`**: `let ms = ...` at `:100`, `probe_run::run_logged(...)` at `:105`, and the `if timed_out { bail!(...) }` block at `:106-111`.
3. The row describes item 2 as living "in the kernel job wrapper that the sandbox shares". That is true of where the deadline is *enforced*, but the **value** is assembled elsewhere, and the row does not say where. The chain is given in full under item 2 below.

If the ground has moved far enough that the defects below no longer describe the tree, stop, write that up, and do not invent a replacement task.

## The principle this row serves

The owner, 2026-10-25: long good work keeps going. **A stop should fire because work has STOPPED PROGRESSING, not because a clock ran out.** Every item in this row is a stop that reads only a clock, so it cannot tell a wedged job from a slow one, and it cuts healthy long work.

The source of truth is `docs/design/self-gates-inventory.md`, **section E, "What was left standing, and why"** (at your base it begins at line **164**, and the paragraph naming these items is at **`:171-174`**). That paragraph says these were left because each sat in a file another row was editing or in the job wrapper the sandbox shares, that those blocking rows have since joined, and that **"They are the first items for the next row."** You are that row.

**Do not edit `docs/design/self-gates-inventory.md`** -- `docs/design/` is on the standing do-not-edit list. Where it should be updated, put the exact text and the exact insertion point in your report and the maintainer will write it.

## Scope: four items, in this order, and one that is NOT yours

**Item 1 of the row's list is NOT yours.** Admission for a wake or task turn is filed separately as `theseus-fzaep`. Do not touch admission. If you find yourself in `admission::Class::of` (`crates/theseus-core/src/turn.rs:1127`), you have gone out of scope.

Work the four items below **in order**, committing each separately. **A written justification is an acceptable outcome for any single item** -- if you read a deadline and conclude it is a genuine deadline on a hung socket rather than a cut on healthy work, say so with the evidence and leave the code alone. That is a result. **Silently leaving an item unexamined is not**, so your report must have a section for each of the four, even if that section says "examined, left alone, here is why."

If you run out of clock, finish the items you have and report honestly which you did not reach. **Do not half-do item 2 to touch item 5.**

### Item 2 (highest value): `proc_timeout_secs`'s clock-only hour

**The chain, verified at your base:**

- The default is **3600 seconds**: `default_proc_timeout_secs()` at `crates/theseus-core/src/config.rs:549-551`, the field at `:486-487`, the struct default at `:605`, and the template line at `crates/theseus-core/config/theseus.example.toml:541` (`proc_timeout_secs = 3600   # default proc.run timeout, when a call names none`). There is also a `proc_timeout_max_secs`, defaulting to 3600, at `config.rs:552-554`.
- The per-call value is assembled in **`crates/theseus-tools/src/proc.rs:245-253`**: `timeout_secs: a.timeout_secs.unwrap_or(ctx.proc_timeout_secs).clamp(1, ctx.proc_timeout_max_secs)`, into the `JobSpec` whose definition is at `crates/theseus-tools/src/lib.rs:182-186` (`pub timeout_secs: u64` at `:185`). The tool's schema documents it at `proc.rs:157` as *"Kill the program after this many seconds."*
- **The kill happens in `crates/theseus-kernel/src/job.rs`, in `run_l0`** (`fn run_l0` at `:583`). `let deadline = Duration::from_millis(args.deadline_ms);` at **`:597`**. The loop at `:601-650` polls, and the clock-only cut is at **`:627-644`**:

```
627:                if t0.elapsed() >= deadline {
628:                    detail["timed_out"] = serde_json::Value::Bool(true);
...
640:                    return (
641:                        Outcome::Failed,
642:                        format!("deadline {} ms exceeded; {how}", args.deadline_ms),
643:                    );
```

`t0.elapsed()` is wall time since the job started. **Nothing in that condition asks whether the job is still producing output.** A `cargo build` that has been compiling steadily for 61 minutes is killed on exactly the same test as one that wedged at minute two.

**The progress signal already exists in this file, and finding it is the first real piece of work.** The wrapper does not read the command's output directly: it hands the command a pipe and a `crate::redact::Copier` reads it into the output file (`:436-444`, `let (read, write) = std::io::pipe()?;` at `:442`, `Copier::spawn(read, out_file, redactor, args.output_max_bytes)` at `:444`), and the copy **counts bytes even past the output cap** (`:438`, *"Past the cap the copy reads on and counts"*). So a count of bytes copied, or an `Instant` of the last copy, is available in principle -- it just lives in the `Copier`, on another thread, and is not visible to `run_l0`'s loop today. **Read `crates/theseus-core/src/redact.rs` before you design anything.**

**What to build.** Turn the hour into a *silence* bound rather than a *duration* bound:

- The job is stopped when it has produced **no output for the configured period**, not when it has *run* for it.
- **At the configured period of wall time, a job that is still producing output gets a notice, not a kill.** The row's words: *"a job that is still writing output is still working."* Find how this wrapper already surfaces a notice (`detail` keys are written throughout `run_l0`; follow where `detail` goes) and use that route -- do not invent a new notification channel, and do not reach for Discord from the wrapper.
- **Keep an absolute ceiling.** `proc_timeout_max_secs` exists and is a cap; do not make it possible for a job that dribbles one byte a minute to run forever. Say in your report what the new ceiling semantics are.
- **Do not change the two default numbers** (3600 and 3600) in this row. Changing the meaning of the existing number is the whole change; changing its value as well makes the diff unreadable. If you believe a default should move, say so in the report.

**Red-first for item 2.** The test that proves this must fail at your base. The obvious shape: a job that writes a byte every N milliseconds for longer than its timeout, asserted to **survive** (today it is killed), beside a job that writes nothing and is asserted to still be stopped. There are existing rigs to copy rather than invent -- `crates/theseusd/tests/job_wrapper.rs`, and the in-crate tests at the end of `crates/theseus-kernel/src/job.rs` (`deadline_ms: 10_000` appears at `:1537`, `:1592`, `:1652`, and `deadline_ms: 30_000` at `:1718`). Use a short deadline so the test is fast. **Paste the `left`/`right` values of the failing assertion at your base.**

### Item 3: the three 2-hour self-build SIGKILL clocks

Verified exact at your base:

- `pub const GATE_LIMIT: Duration = Duration::from_secs(2 * 3600);` -- **`crates/theseus-core/src/rsi/tree.rs:190`**, used at `:641` (`run_niced(p, tree, &script, &log, GATE_LIMIT, benches)`) and `:694` (`limit: GATE_LIMIT`).
- `const BUILD_LIMIT: std::time::Duration = std::time::Duration::from_secs(2 * 3600);` -- **`crates/theseus-core/src/rsi/join/install.rs:98`**, used at `:82` (`limit: BUILD_LIMIT`).
- `const TESTS_LIMIT: std::time::Duration = std::time::Duration::from_secs(2 * 3600);` -- **`crates/theseus-core/src/rsi/join/planted.rs:178`**, used at `:166` (`limit: TESTS_LIMIT`).
- The jailed path: **`crates/theseus-core/src/rsi/jail.rs:100-111`**, which converts the limit to `ms`, passes it to `probe_run::run_logged`, and on `timed_out` bails with *"the build ran past {} minutes and was stopped"*.

The row's justification: **a cold cargo build on a loaded host can legitimately approach two hours.** That is not hypothetical in this project.

**What to build.** The same transformation as item 2: silence-based rather than duration-based, with a notice at the clock rather than a kill, and an absolute ceiling kept. **Three of these four sites share `run_niced`/`run_logged`, so the right change is almost certainly one change at the shared layer and not three.** Find the shared layer first and say in your report what it is. If you change the constants' meaning, their names stop being accurate (`GATE_LIMIT` for a silence bound reads wrong): rename them in the same commit and say so.

**`probe_run` is a dependency, not our code.** If the jailed path's timeout is enforced inside `probe_run::run_logged` and cannot be made silence-based without adding a dependency or a fork, **say that plainly and leave `jail.rs` alone** -- that is item 3's written justification and it is an acceptable outcome for that one site. Do not add a dependency; the preamble forbids it.

### Item 4: `extend`'s 100-second trial abort

Verified at your base in `crates/theseus-core/src/extend/mod.rs`:

```
85:pub const START_TIMEOUT_SECS: u64 = 30;
86:pub const CALL_TIMEOUT_SECS: u64 = 20;
87:pub const TRIAL_LIMIT: Duration = Duration::from_secs(100);
```

with the doc comment at `:82-84`: *"The longest the trial waits for the server's handshake and list, and for each test's call: the whole trial stays under the 120 s a harness call may take."* The abort is at **`:645-652`**:

```
645:    let tried = match tokio::time::timeout(TRIAL_LIMIT, &mut task).await {
...
649:        Err(_) => {
650:            task.abort();
651:            return Err(format!("the trial took over {} s", TRIAL_LIMIT.as_secs()));
652:        }
```

(The row cites `:86-87`; `TRIAL_LIMIT` itself is at `:87`, and `:86` is `CALL_TIMEOUT_SECS`, which is the per-call bound inside the trial -- note both.)

**This item is different from items 2 and 3, and you must weigh that before you change anything.** The inventory calls it the same shape as the 2 h SIGKILL but *"about 70 times tighter"*, and notes it was never assessed against the principle. But the doc comment at `:82-84` gives it a **reason the others do not have**: the trial must stay inside the 120 s an in-process harness call may take. A trial that runs past that bound does not become a long good piece of work -- **it becomes a call that the layer above it has already abandoned.** `CALL_TIMEOUT_SECS` at `:86` and `START_TIMEOUT_SECS` at `:85` sum with the trial's other phases into that 120 s.

So the honest answer for item 4 may well be **"this is a real deadline and it stays"**, and if your reading of the 120 s bound supports that, write that up with the `file:line` for the 120 s figure (find where the 120 s in-process deadline is actually enforced -- do not take the comment's word for it) and move on. **If you do keep it, that section of the report is the deliverable for item 4 and no code change is owed.** If instead you find the 120 s bound does not apply the way the comment says, then the same silence-based treatment applies and the item is live.

### Item 5 (adjacent, do it only if items 2 to 4 are done): the tenders' model-call deadlines

These are **unassessed**, which means your first job is to read them, not to change them. Verified at your base, with the directory corrected:

- **`crates/theseus-core/src/tender.rs:850-852`**:
```
850:    tokio::time::timeout(deadline, ask)
851:        .await
852:        .map_err(|_| no(format!("no answer within {} ms", deadline.as_millis())))?
```
- **`crates/theseus-core/src/tender/pair.rs:145-148`**: `let ended = match tokio::time::timeout(deadline, run).await { Ok(r) => r, Err(_) => Err(no(format!("no answer within {} ms", deadline.as_millis()))) };`
- **`crates/theseus-core/src/tender/status_cache.rs:64-67`** (a timeout on acquiring `self.asking.lock()`) and **`:76-81`** (a timeout on the status call itself, with `left = deadline.saturating_sub(began.elapsed())` at `:73`).

The row's instruction, which is the right one: *"Read them before changing them; if one is genuinely a deadline on a hung socket rather than on work, say so and leave it."*

**My reading, which you should check rather than trust:** `status_cache.rs:64` is a timeout on a *mutex acquisition*, and `:76` is a timeout on a *socket call to an index process* -- both look far more like "the socket is not answering" than like "work is in progress", and `status_cache.rs:183-186` shows the user-facing words are already *"its socket did not answer (...): its status as of 2.5 s ago"*, which is a graceful degrade, not a kill. If you agree, **the written justification is the deliverable for those two and you should not change them.** `pair.rs:145` wraps `run`, which is doing more than waiting on a socket: read what `run` actually does before you judge it.

**A deadline on a model call is not automatically a cut on healthy work** -- a model that has not answered in N ms is usually a model that will not answer. Be sceptical of changing these, and say what you concluded for each of the four sites.

## What closes this row

Each of items 2, 3, 4 and 5 either becomes progress-based (silence, no output, no forward motion) with a notice instead of a kill, **or is justified in writing as a real deadline that must stay.** Both outcomes are complete work. Your report must address all four by name.

## The keel, for this row specifically

**This row touches timing bounds, which is keel territory.** Read the guard's rules before you start.

- Lengthening or removing a stop is **not** obviously a keel finding the way lowering an assertion count is -- but `scripts/keel-guard.py` names *"raises a bench budget, a spend or loop cap"* among its rules, and these constants are adjacent to that language. **Run `python3 scripts/keel-guard.py` scoped to your own range before every commit and paste the output into the report whether or not it is empty.**
- **If the guard fires on any of these constants, do not try to route around it.** Isolate that change in its own commit, list it in the report under **Keel findings expected** with the rule, the path and why, and **supply the exact sentence you want the owner quoted on in the `Keel:` trailer**. The owner's standing position is on record and is the right thing to quote toward: he has said our development guardrails are adjustable for a real need, and that for this work *"long good work keeps going"* and a stop should fire on stopped progress, not on a clock. Write the sentence you want quoted; do not paraphrase it into the trailer yourself.
- **Keep every money stop exactly as it is.** The inventory's section E is explicit that no dollar stop was touched and every money row is marked keep. Spend limits, the daily ceiling and the burn-rate line are **out of scope** and must not move. If your change makes a money stop reachable later than before, say so loudly.

## What will get this row rejected

- Touching admission (item 1, `theseus-fzaep`'s work).
- Editing `docs/design/self-gates-inventory.md`.
- Changing a spend limit, the daily ceiling, or any money stop.
- Changing the numeric defaults (3600, 2 h, 100 s) rather than their meaning.
- A silence bound with no absolute ceiling at all.
- An item with no section in the report -- "unexamined" is the one unacceptable outcome.
- Red-first evidence without the `left`/`right` values.
- A `mod` line outside the sorted block (`cargo fmt --all --check` fails in eight seconds).
