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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3c-steer-keep-going`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`). This
row needs no store change. If a part truly needs a bump, name it and justify it in your report's first section (the
integrator renumbers it at the join); do not bump for anything else.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines).
`crates/theseus-core/src/turn.rs` sits at its ceiling (3,525): do not add lines to it. At the base:
`crates/theseus-core/src/turn/progress_step.rs` 405, `crates/theseus-core/src/tests_turn_progress.rs` 354,
`crates/theseus-core/src/fact/progress.rs` (read its size).

**Your lane: `integ/p3-turn`** (new: three rows that shape what a long turn does, all fired at about the same time on
the same base). Its other rows:
- `p3c-clear-by-tokens` (`cloud/20261024-p3c-clear-by-tokens`, theseus-5lzox): the compiler's clearing pass keeps
  results by tokens, not by the newest 20 exchanges, and a cleared result's stub carries its line range and a note of
  what was checked (`crates/theseus-core/src/compiler/clearing.rs`, `compiler/tests_ring.rs`).
- `p3c-narrow-reads` (`cloud/20261024-p3c-narrow-reads`, theseus-tdkcn): a narrow-read hint on `deep_coding` turns
  (list functions first, read 100-300-line slices, write findings as you go) and possibly a smaller default `fs_read`
  window (`crates/theseus-core/src/turn/` injection, `crates/theseus-tools/src/fs.rs`).

**Files you share with them, so keep all three additive:** only `crates/theseus-core/AGENTS.md`. You edit only the
progress-check paragraph (near :3000-3018, "the progress check", theseus-2uy56); they edit the "One long instruction"
paragraph (near :119-130) and the tools/turn paragraphs. Do not rewrite theirs. You own `turn/progress_step.rs`,
`tests_turn_progress.rs` and `fact/progress.rs`; neither other row touches them. Do not edit `docs/status.md` (the
integrator writes it at the join).

## Your task: p3c-steer-keep-going, the progress steer must never end open-ended work (theseus-ikr75)

Branch: `cloud/20261024-p3c-steer-keep-going`.

Every commit subject carries `theseus-ikr75`. Deadline for the report: about 2 hours after you start.

**Why (the evidence, from a benchmark the owner ran on 2026-10-24).** In a bug hunt (13 planted bugs in a copy of
this workspace, a 36-minute cap, a $32 spend limit with `spend_limit_mode = "ask"`, no deadline), Theseus fixed 4 of
13 while two rival harnesses fixed about 10 each. Theseus did not run out of time or money: it stopped itself at 21
minutes with $16 of its $32 left. At $16.28 the progress check you are fixing (theseus-2uy56, joined 10-23,
`crates/theseus-core/src/turn/progress_step.rs`) appended its steer:

> [The harness: this turn has used $16.28 of its $32.00 spend limit, and no check has run in its last 3 loops. Replan
> or narrow: say what is left and what has not passed, then take the smallest step that can pass.]

The model ran the test suite (412 passed) and ended its turn 18 seconds later with half its budget and 15 minutes
left. In a second trial the steer fired at $16.07 ("no check has run in its last 83 loops"); the next message was
"The smallest useful step now is a regression test…", and the turn ended 0.7 minutes after the steer. A bug hunt is
mostly reads, so it had no passing command for many loops, and the steer's words ("what has not passed… the smallest
step that can pass") have one natural answer when the suite is green: run it and stop. **This steer is live in the
owner's daemon today, so this is the fix he will feel first.**

**What the rule does at the base** (read the module doc and `decide()`): at each loop's end, with no passing
`proc.run` in the last `MIN_LOOPS_WITHOUT_PASS = 3` loops, it steers once at `STEER_SHARE = 0.5` of the deadline OR
of the execution's spend limit, and stops at `STOP_SHARE = 0.85` of the deadline (time only: spend never stops a
turn). A loop with no command is "no check" and counts toward `since_pass`; reads never count.

**What to build (three changes, constants `STEER_SHARE` / `STOP_SHARE` unchanged, NO new config key):**

(a) **The wording.** The steer must say it is not a stop. New text, in the module's plain style (exact words your
call, but keep every point): what was used; then "This is not a stop: if work is left, keep going."; then "say in a
line what is left, then go on with the next item." Keep "the smallest step that can pass" ONLY when a check is
failing (`failing > 0`), worded about the failing check ("…the smallest step that can make the failing check
pass"); drop it when the turn's checks pass (no failing loop since the last pass, including the case of a suite that
passed earlier in the turn). The stop note (`Verdict::Stop`, time only, near the deadline) is unchanged.

(b) **Spend alone does not steer a guarded or deadline-free turn.** Do not fire on the spend share when the
execution's limit is a guard (`[kernel] spend_limit_mode = "ask"`: the turn ends at its limit anyway, see
`crates/theseus-core/src/config/limits.rs`) or when the turn has no deadline. The time share still steers and stops
exactly as before. Read the mode from the turn runner's config (`self.cfg.kernel.spend_limit_mode`; `progress()` is
an associated fn today: pass what it needs, or read it once when `State` begins in `with()`). Put the decision into
the pure rule so the table test covers it: for example a `Signals` field (`spend_steers: bool`, or `spend: None`
when it may not steer, but then say how the `turn.progress` row still reports the spend), your call; say which.

(c) **Reading is progress until there is something to check.** Pick ONE of these, implement it, and say in the
report which and why (the issue allows either; the first is preferred):
  1. A turn that has not edited yet counts no checks at all: until its first answered edit call (`fs_edit`,
     `fs_write`, and any other tool that changes a file: find them by the tools' own read-only/side-effect flag if
     one exists, else name the list), loops do not add to `since_pass`, so nothing fires on a turn that is only
     reading. And after the first edit, a loop whose read-only calls all answered without error AND opened a path
     the turn had not read before (new coverage) resets `since_pass` like a passing command. Keep the "seen paths"
     set on `State` (bounded: paths only, no contents).
  2. A passing read-only call (fs_read / fs_grep / fs_list / fs_glob or the read-only flag) counts as a passing
     check, like a passing command.
Either way a loop whose command FAILED is still a failed check, and a command sent to the background is still no
check (keep `a_loops_check_comes_from_its_commands_alone` green: extend it, do not loosen it).

**The 2026-10-24 shape must not steer.** A turn with `spend_limit_mode = "ask"`, no deadline, the suite passing
earlier, spend at 0.51 of the limit and 3+ loops of reads since: today `Steer`, after your change `Nothing`. The
same turn under `notify` WITH a deadline at half its time and no check still steers (with the new words): the steer
is not deleted, it is aimed.

## Tests (each red at the base, with a planted revert)

1. `progress_step.rs` table test `the_rule_steers_then_stops_only_without_a_recent_passing_check`: keep its name and
   every row's intent; add rows for (b): spend 0.5+ with no deadline -> `Nothing`; spend 0.5+ under a guard ("ask")
   -> `Nothing`; spend 0.5+ under notify with a deadline -> `Steer`; time 0.5+ under a guard -> `Steer` (time still
   steers); time 0.85+ -> `Stop` regardless of mode. Rows whose expected verdict changes because of (b) (for example
   `(s(None, Some(0.5), 4, 2), false, Steer)`) are rewritten to the new expectation, and each one gets a sibling row
   that keeps the old firing shape under notify + a deadline, so steer coverage does not shrink. Plant P1: remove
   the (b) condition from `decide()`: red.
2. A test for (c): with option 1, a sequence of read-only loops before any edit never steers at any share; after an
   edit, a loop opening a new path resets `since_pass`, and re-reading an already-read path does not. With option 2,
   the matching table. Plant P2: revert the (c) rule: red.
3. `the_notes_say_what_was_used_and_what_has_not_passed`: rewrite its exact-string assertions to the new text (an
   exact equality stays an exact equality: never weaken it to `contains`). Add exact assertions that the passing-case
   note contains "This is not a stop" and does NOT contain "smallest step", and that the failing-case note keeps the
   failing-check step. Plant P3: put the old note back: red.
4. `crates/theseus-core/src/tests_turn_progress.rs` (the whole-core tests; `const STEER: &str = "Replan or narrow"`
   near :129): update the marker to a phrase of the new note. Any test there whose turn steers on spend alone with no
   deadline or under "ask" now expects no steer: rewrite its expectation and give it a sibling that steers under
   notify + a deadline. Add one whole-core test of the 10-24 shape: a scripted stand-in model that reads for many
   loops under `spend_limit_mode = "ask"` with no deadline past half the limit: no `turn.progress` row, no
   `harness:progress` node, and the turn ends only when the model ends it. Plant P4: revert (b): red.
- Keep green: every existing test in `turn::progress_step`, `tests_turn_progress`, `turn::clock_step::tests`,
  `tests_budget_loop`, `tests_limits_notify`, and the whole `theseus-core` suite.

## Not in this row

No change to `STEER_SHARE` / `STOP_SHARE` values or `MIN_LOOPS_WITHOUT_PASS`; no new config key; no change to the
time stop; no change to the clearing pass or the tools (the other two rows). No finish-check / coverage-ledger
feature (a later row). No benchmark run and no spend: you have no key, and the verification rerun of the bug hunt
waits for the owner.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. The steer is
  a harness node in the model's context, not a DM message; but if any change of yours would alter a message of that
  flow, STOP that part and write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove no test, rename no test, lower no test's assertion count, weaken no assertion (an exact string
  stays exact), raise no budget, cap or ceiling, do not edit `scripts/keel-guard.py` or its tests. Before the report
  run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its whole output. If it flags
  anything, STOP that part, paste what it prints, and write it as an owner question in the report (do not add a
  `Keel:` trailer yourself: an unsigned ack counts for nothing).
- **Data floor:** nothing new is sent to a model or routed anywhere.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `ac298666`, green after; prove each with its planted revert); right over
  compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy -p theseus-core --all-targets -- -D warnings`, `cargo nextest run -p
  theseus-core` (or `cargo test`), then `scripts/shape.sh`. If theseusd has tests that read the steer text (grep for
  "Replan" and "harness:progress" across `crates/`), run those too.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), ceilings, any OWNER CALL or STOP, and in
one line each the (b) and (c) rules as built. (2) **The new notes** verbatim (steer, passing case and failing case;
the stop note unchanged). (3) **The rule:** the new `decide()` table, which option of (c) and why, how the mode
reaches the rule. (4) **The 10-24 shape:** what the rule decides today and after, step by step. (5) **Tests** and
plants P1-P4, red at the base and green after. (6) **Gate** commands and results. (7) **Keel** output, whole. (8)
**DM golden** diff result. (9) **Join notes:** every file touched with line ranges, and the AGENTS.md hunk, so the
join with `p3c-clear-by-tokens` and `p3c-narrow-reads` stays additive. (10) **What the verify rerun should look
for** (no `turn.progress` steer row before the limit, or the turn going on after it; wall time and $ at the end).
(11) **Follow-ups** (for example the finish check for open-ended asks). (12) One line on the data floor.
