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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3h-admission-headroom`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `0fda4d57`. That is install #83's gated tree: release 1.0.2 (signed tag
`release-1.0.2` at `ac298666`) plus the joined patch3 work. Fetch it and branch from `0fda4d57`. Line numbers below
were read at `0fda4d57` and are right unless a line moved; search by name anyway. **Where the release preamble above
disagrees with this note about the base, the store format, ceilings, the keel or the work in flight, this note wins.**
The preamble above is older than your base: ignore its store-format numbers, its list of changes in flight, and its
sentence that lets you raise a line ceiling. The rules here replace them.

**Early push (required, before any code).** Do setup first (`cargo --version` alone, then nextest) and your first
`cargo build -p theseus-core`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-o1h3f.4: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3h-admission-headroom`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches. The early
commit costs one second and tells those two cases apart.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs` :688);
the kernel's record schema is `theseus_kernel::types::SCHEMA = 2` (`types.rs` :33). **This row stores nothing and must
touch neither.** Admission state is in memory only.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every Rust file the list does not name fails past 2,500
lines). At the base: `crates/theseus-core/src/turn/admission.rs` is **611 lines** and is NOT on the long-files list,
so it caps at 2,500 — you have room, but its `mod tests;` at :611 means the tests live in
`crates/theseus-core/src/turn/admission/tests.rs`. Put new tests there. `crates/theseus-core/src/turn.rs` is 3,498 of
its 3,525 ceiling: **do not edit `turn.rs` at all in this row** (27 lines of room, and two sibling rows in this lane
need them).

## Why this row exists, and why it is not optional

This row is one of **five that must land in ONE install**. It is a co-ship, and that is the owner's call, not a
convenience. The owner's standing line on self-tasking (2026-10-24) is *"we should be very very lightweight on any
limits to theseus tasking itself … notify, work, and allow feedback is the model"*, and on the fan-out design's cap
question he answered **"All recs!"** — which resolved to: **no fixed helper cap; a fan-out is bounded by admission
slots and spend, shipping with this row.** A design audit then found, in the code, that removing the helper cap is
unsafe without this row:

- A queued task turn ranks one class higher every `AGE_STEP` (30 s, `admission.rs` :47, `Class::aged` :87-90), so
  after 60 s it ranks **with a person's input** and is served in arrival order. The owner's own message, arriving
  after a 20-helper fan-out, would wait behind every helper queued 60 s earlier.
- A helper's turn keeps its slot for its whole turn (many model calls); it lends only around a foreground job
  (`LEND_AFTER`, `toolrun/job.rs` :351-367). **Eight helper turns fill all eight slots** (`admission_ceiling` is 8).
- If no turn is admitted for the stuck window (`[kernel] admission_stuck_secs`, 600 s), **every** waiter fails with
  `admission_stuck` (`admission.rs` :520-575), whatever its class. Eight helper turns of over ten minutes each would
  therefore fail the owner's own queued DM.

With the old cap of 6 helpers and a lending parent, two slots always stayed free and none of this could happen. This
row restores that guarantee as a **slot rule** instead of a child cap, which is exactly what the owner's line keeps.

**READ THIS TWICE: this row LOWERS a limit. It raises nothing.** Task-class turns may hold at most `ceiling − 1`
slots. The admission ceiling itself **stays at 8** and you must not change it, nor `admission_stuck_secs`, nor
`AGE_STEP`, nor `LEND_AFTER`. If a test of yours seems to need a bigger ceiling to pass, the test is wrong, not the
ceiling: the entire point is that the owner's typed message can always get a slot. Raising a ceiling to make a test
pass would undo the row.

## Your task: p3h-admission-headroom, task turns leave one slot free (theseus-o1h3f.4, o1h3f row D)

Branch: `cloud/20261025-p3h-admission-headroom`.

Every commit subject carries `theseus-o1h3f.4`. Deadline for the report: about 2 hours after you start.

Read `crates/theseus-core/src/turn/admission.rs` whole first — its module doc (:1-25) states the properties you must
not break: one queue per class, first come first served inside a class, aging so no class starves, a freed slot going
to exactly the next turn with no poll and no herd, the queue's state one mutex held across no `.await`, and a `Grant`
that gives its slot back when dropped unread.

### What to build

**1. Task-class turns hold at most `ceiling − 1` slots.**

The shape of the problem: `State` (:123-137) counts only `in_use`, with no idea which class holds each slot. The
class is known at every point where a slot is taken or given back, so thread it through:

- **Taking:** `State::admit_now` (:164-168) is called from `Admission::try_take` (:347) and `Admission::join`
  (:360-363), both of which have the class; and from `hand_on` (:481-505), where the popped waiter's class is its
  queue index (`Class::ALL[i]`, as `pop_next` already uses at :146-157).
- **Giving back:** `give_back` (:475-479) has no class today. Its two callers do, or can: `Slot` carries `class`
  (:218-224, dropped at :263-285) and `Grant` (:200-218) does not — a grant is built in `hand_on`, where the class is
  known, so carry it on the `Grant` (or on whatever you add) rather than guessing.
- **Lending:** a lent slot is not counted in `in_use` (:124, `lend` :442-455, `retake` :457-473). Keep your per-class
  count consistent with that: a lent slot is not held. The `bound` map already stores `(lent, class)` per execution
  (:255-258), which is the precedent to follow.

Then make the rule bite in the two places a `Class::Task` turn can get a slot:

- `State::pop_next` (:146-157) must not return a `Class::Task` waiter when admitting it would take the task count to
  the ceiling. **Do not drop that waiter**: leave it at its queue's head so it is admitted the moment a task slot
  frees, and let `pop_next` consider the other queues' heads instead. Think about `hand_on`'s `while s.in_use <
  ceiling` loop (:483): if the only waiters are task turns at the task limit, the loop must end with the slot free
  and `moved` updated, exactly as it does today when nothing waits (:485-488) — **a free slot that nobody may take
  must still count as the queue moving**, or your change turns a healthy queue into an `admission_stuck` failure for
  everyone. That is the single most important correctness point in this row; test it directly.
- `Admission::try_take` (:347-356) and `Admission::join`'s fast path (:360-366) must apply the same limit before
  `admit_now`, so a task turn arriving at an empty queue cannot take the last slot either.

A `front` retake (`join(class, front = true)`, used by `retake` when a lending turn comes back) is a turn that
**already held** a slot, so it must not be blocked by the limit: a parent or helper that lent its slot and is
retaking is not a new holder past the limit. Decide this deliberately, write the reason in the code's doc, and test
it — a retake refused by the limit would deadlock the very wait the fan-out relies on.

Derive the limit from the ceiling (`ceiling.saturating_sub(1)`, with a ceiling of 1 or 0 handled so the daemon still
runs — say in the report what you chose for ceiling 1 and why; a ceiling of 1 must not make task turns unrunnable
forever). Add no config key: the owner's line keeps the ceiling as the one slot knob, and a new key here would be a
second one.

**2. Task aging stops at the `Wake` rank and never reaches `Person`.**

`Class::aged` (:87-91) subtracts one rank per step, to 0 (`Person`). For `Class::Task` it must floor at
`Class::Wake as u64` (1): a waiting task turn still ages out of starvation into the wakes' rank, but never ranks with
a person's typed input. `Class::Wake` and `Class::Person` keep today's aging exactly. Update the `AGE_STEP` doc
comment (:30-46), which currently says "a turn that has waited this long ranks with the wakes, and twice this long
with a person's input" — the second half stops being true for task turns, and a stale doc here is how the next reader
re-breaks it.

**3. The module doc.** Add the two rules to `admission.rs`'s module doc (:1-25) in its own voice, and to
`crates/theseus-core/AGENTS.md` wherever admission is described — **as a new paragraph**; do not rewrite existing
ones (another row in this lane has its own AGENTS.md hunk).

### Tests (each red at the base, with a planted revert)

In `crates/theseus-core/src/turn/admission/tests.rs`, beside the existing admission tests, using
`Admission::with_step(ceiling, stuck, step)` (:293) so a test can age a waiter in milliseconds rather than 30 s.

1. **A task never takes the last slot.** Ceiling 4. Four task turns ask: three are admitted and the fourth waits. A
   `Person` turn then arrives and is **granted at once**, with no wait. Plant P1: remove the limit from `pop_next`
   and the fast paths: red (the fourth task takes the last slot and the person waits).
2. **A free slot nobody may take is still the queue moving.** Ceiling 2, stuck window short (say 200 ms). One task
   turn holds a slot; a second task turn waits; nothing else arrives. Assert the waiting task turn does **not** fail
   `admission_stuck` when the first turn is still running and a slot is free but reserved for a non-task — i.e. that
   `hand_on`'s loop updated `moved` when it declined to admit the task waiter. Then end the holder and assert the
   waiter is admitted at once. Plant P2: in `hand_on`, `return` without setting `moved` when the only waiter is a
   declined task: red (the waiter fails `admission_stuck`).
3. **A retake is not blocked by the limit.** Ceiling 2. Two task turns hold both slots; one lends (`lend`) and then
   retakes (`retake`) while the other still holds its slot and a third task turn waits in the queue. The retake
   completes (it held a slot before) and the fresh task waiter does not jump it. Plant P3: apply the limit to the
   `front` path too: red (the retake never returns — give the test a timeout and assert on it rather than hanging the
   suite).
4. **Aging stops at `Wake`.** A `Class::Task` waiter aged two or more steps does **not** outrank a freshly arrived
   `Person` waiter: with both queued and one slot freed, the person is served first. A `Class::Task` waiter aged one
   step still outranks a fresh `Class::Task` waiter behind it (no starvation inside the class), and a `Class::Wake`
   waiter's aging to `Person` rank is unchanged. Plant P4: restore the unfloored `aged`: red.
5. **The ceiling is unchanged.** A plain assertion that `Admission::ceiling()` is what it was constructed with, and
   that no default in `crates/theseus-core/src/config/` moved — this is the test that stops a later session from
   "fixing" a failure by raising the ceiling. Say in its message that the rule lowers a limit and never raises one.

- **Keep green:** every existing test in `theseus-core`, in particular the whole admission suite (the FIFO-within-a-
  class tests, the aging tests, the stuck-window tests, the lend/retake tests) and `theseusd`'s admission and task
  tests. If an existing admission test asserts that a task turn takes the last free slot, **do not delete or weaken
  it**: it encodes the old rule. Rewrite its expectation in the same number of assertions, say exactly which test and
  which line in the report under **Keel findings expected**, and keep its name. A rewritten assertion is not a keel
  finding; a removed one is.
- **Under load:** run the new timing-sensitive tests (2 and 3) once with the preamble's load recipe (`nice -n 19`
  plus four busy loops at nice 0) and report the result. A negative assertion that fails even once ("the person was
  not served first") is a finding, not a flake: keep its output.

### Not in this row

No change to `admission_ceiling`, `admission_stuck_secs`, `AGE_STEP`, `LEND_AFTER` or any config default. No new
config key. No change to `turn.rs` (it is 27 lines from its ceiling and two sibling rows need that room), to
`toolrun/job.rs`'s lend timing, to the kernel, to the cockpit or to any `.tsx` file. No `task.wait` or `task.fan_out`
work: two other rows in this lane own those. No benchmark, no spend, no AWS, no model call.

## Your lane: `integ/p3-fanout`, and the four rows beside you

All five rows of this lane fire at once off `0fda4d57`, so their branches do not see each other. The integrator joins
them in this order:

1. `p3h-self-started` (theseus-o1h3f.1) — `theseus-protocol/src/tasks.rs`, `theseus-core/src/task.rs`, the CLI's task
   renders.
2. `p3h-task-wait` (theseus-z9mze.2) — new `theseus-core/src/task_wait.rs`, `toolrun/waits.rs`, `turn/join_step.rs`,
   `keep_warm_wait.rs`, `toolrun/parallel.rs`, and `TurnCtx.room` in `turn.rs`.
3. `p3h-fan-out` (theseus-z9mze.3) — `theseus-core/src/task.rs`, new `turn/helper_step.rs`, `thread_context.rs`, and
   the `ask_budget` caller in `turn.rs`.
4. **this row** and `p3h-burn-parent` (theseus-o1h3f.5, `theseus-kernel/src/day_ceiling.rs`), before the lane's gates.

**You touch `turn/admission.rs` and its tests, and nobody else in this lane does.** Row 2 reads `Admission::lend` and
`retake` but does not edit them. So your join should be clean at file level; name every hunk with its line range in
the report's join notes so the integrator can confirm that.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If anything
  of yours would change a message of that flow, STOP that part and write `OWNER CALL: <what and why>` in the report.
- **Keel.** The gate's keel phase (`scripts/keel-guard.py`; its docstring lists the rules) fails a change that
  deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench
  budget, a spend or loop cap, or a ceiling in `scripts/long-files.txt`, `scripts/shape.sh` or `.config/nextest.toml`,
  or edits the guard's own files, unless a commit signed by the owner's key acks it. **You cannot sign, so you cannot
  ack, and you must never add a `Keel:` trailer yourself.** Run `python3 scripts/keel-guard.py` before every commit
  and `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and paste the whole
  output. **If it flags a behaviour cap, STOP that part, put the old value back, and write `OWNER CALL:` saying what
  and why — do not raise the cap.** This row is expected to produce **no** finding: it lowers a limit and rewrites at
  most one existing expectation (see the keep-green note), which belongs under **Keel findings expected** with its
  rule, path and reason.
- **Data floor:** nothing new is sent to a model or routed anywhere. This row changes only which queued turn is
  admitted next.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it, and run `git status` after every restore); right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy -p theseus-core --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-core` plus
  `theseusd`'s task and admission tests. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp
filesystem has committed an empty report here before), and check after the commit that it is not empty. Push the
branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51 and SCHEMA 2, both untouched), ceilings, any
`OWNER CALL:` or STOP, and a one-line statement that the ceiling was not raised. (2) **The slot rule:** where the
per-class count lives, every place it is incremented and decremented, how lending is kept consistent, what `pop_next`
and `hand_on` do with a declined task waiter, how `moved` is kept truthful, the `front`/retake decision and its
reason, and what happens at ceiling 1. (3) **Aging:** the floor, and the doc lines you corrected. (4) **Tests** and
plants P1-P4 (and 5), red at the base and green after, with the load run's result. (5) Any existing test whose
expectation you rewrote, named with its line, under **Keel findings expected**. (6) **Gate** commands and results.
(7) **Keel** output, whole. (8) **DM golden** diff result. (9) **Join notes:** every file touched with line ranges,
and a statement that no file of the other four rows in the lane was touched. (10) **Follow-ups**, including anything
the fan-out rows should know about the slot arithmetic. (11) One line on the data floor.
