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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3h-burn-parent`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
`cargo build -p theseus-kernel`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-o1h3f.5: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3h-burn-parent`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs` :688);
the kernel's record schema is `theseus_kernel::types::SCHEMA = 2` (`types.rs` :33). **This row should store nothing
new**: the burn window is one in-memory ring per execution, held under `DayCeiling`'s own lock and never written to
the store (`day_ceiling.rs` module doc, "One counter, in memory"). If a part of your work truly needs a stored field,
name it and justify it in your report's first section and **do not bump `SCHEMA`** (`terms.rs` marks every execution
with `schema < SCHEMA` as `legacy`, so a bump would relabel every stored execution).

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every Rust file the list does not name fails past 2,500
lines). At the base: `crates/theseus-kernel/src/day_ceiling.rs` **789**, `overdraw.rs` **134**,
`crates/theseus-kernel/src/tasks.rs` **612**, `kernel.rs` 3,001 of its 3,030 ceiling (**add at most a call line
there**), `crates/theseus-core/src/turn.rs` 3,498 of 3,525 (**do not edit `turn.rs` at all in this row**).
`tests_runaway.rs` holds the existing burn tests.

## Why this row exists, and why it is not optional

This row is one of **five that must land in ONE install**. It is a co-ship, and that is the owner's call, not a
convenience. The owner's standing line on self-tasking (2026-10-24) is *"we should be very very lightweight on any
limits to theseus tasking itself … notify, work, and allow feedback is the model"*, and on the fan-out design's cap
question he answered **"All recs!"** — which resolved to: **no fixed helper cap; a fan-out is bounded by admission
slots and spend, shipping with this row.** A design audit then found, in the code, that "spend bounds the count" is
not true as written:

- The runaway guard the owner leans on is the **$200 in 10 minutes** burn line (`DEFAULT_BURN_MICROS`,
  `BURN_WINDOW_MS`, `day_ceiling.rs` :66-70). It is recorded **against the execution that made the call**:
  `self.ceiling.burn_record(now, &execution_id, cost)` in `Kernel::apply_day` (`day_ceiling.rs` :783), and read back
  per execution by `burn_stop(now_ms, execution)` (:609-622) through `Kernel::burn_stopped` (`overdraw.rs` :125-135).
- **Every helper of a fan-out is its own execution.** So thirty helpers at $3 each ($90), or seventy ($210), never
  trip any single line — the burn guard does not see a fan-out at all.
- It is worse than loose: under `notify` the fan-out's split is half of `max(available, limit)`, so at the default
  $100 limit with a $1-per-helper floor it allows **up to 50 helpers**, and a second fan-out carves again, because
  under notify a carve may pass what the parent has left (`overdraw.rs` :1-12, :30-45).

So: **a task's settled cost must also count in its parent's burn window.** That tightens a guard and raises nothing.
Without this row the fan-out ships with no aggregate runaway line at all.

**This row tightens a limit. It raises nothing, and it loosens nothing.** If something you write would raise or
relax a spend line, a ceiling or a multiple, stop that part and bring it back as an `OWNER CALL:` — do not do it.

## Your task: p3h-burn-parent, a task's spend counts in its parent's burn window (theseus-o1h3f.5, o1h3f row E)

Branch: `cloud/20261025-p3h-burn-parent`.

Every commit subject carries `theseus-o1h3f.5`. Deadline for the report: about 2 hours after you start.

Read first, in this order: `crates/theseus-kernel/src/day_ceiling.rs` whole (its module doc states the properties you
must not break), `overdraw.rs` whole (the notify mode, the hard multiple, `burn_stopped`), `tasks.rs::carry`
(:584-611) and `kernel.rs::carry_to_parent` (:752-770) and `lock_family` (:739-745), and `tx.rs`'s `day_effects`
(:82), `day_len` (:90) and `day_rewind` (:97-107).

### What to build

**1. A settled cost reaches the parent's burn window.**

The one real decision in this row is **where**. Both candidates are real; pick one, implement it, and say in the
report why you rejected the other.

- **(a) At the day effect.** `Kernel::apply_day` (:770-788) matches `Effect::Settle { id, execution_id, reserved,
  actual }`, settles the day's total, then calls `burn_record(now, &execution_id, cost)`. Record the same `cost`
  against the execution's ancestors as well. The ancestor ids have to come from somewhere: either read them here
  (`self.execution(&execution_id)?.parent`, walking up) or carry them on the `Effect` when it is built
  (`Effect::of`, :724-734, from the `Action`, which has `execution_id`). **Carrying them on the effect is the safer
  shape** if reading an execution inside `apply_day` can reach a lock the caller already holds — `apply_day` runs
  after the frame is committed (`day_after`, :763-769; inside a transaction the effects are queued and applied at the
  commit), so check this yourself rather than trusting either of us, and say in the report what you proved about the
  lock order. A deadlock here would hang every settle in the daemon.
- **(b) At the carry.** `tasks::carry` (:584-611) already computes exactly the delta this row wants —
  `spent = child.budget.spent_micros - spent_before` — and `carry_to_parent` already runs under both locks that
  `lock_family` takes. But it runs **inside** the frame, before the commit, and the day's effects exist precisely so
  that money lands once the frame is written (`Effect`'s doc: "What a settle does to the day, applied once its frame
  is written") and can be rewound when it is not (`tx.rs::day_rewind`). If you choose here, you must queue an effect
  rather than calling `burn_record` directly, or you will book a cost for a frame that failed.

Whichever you choose, these must hold, and each needs a test:

- **The day's total is not double counted.** `burn_record` writes to a separate per-execution ring (`self.burn`,
  :586-605); the day's `spent` comes from `settle`/`book` only. Recording the same cost under a parent must leave
  `Today`'s figures (`today`, :630+) **exactly as they are today**. The module doc's line "A task's cost is its own
  call's, counted once: carrying it to its parent's budget adds nothing here" (:42-44) is about the **day total** and
  stays true — but it will read as contradicting this row, so correct the doc to distinguish the day's total (counted
  once) from the burn window (counted for the execution and for each ancestor).
- **How far up.** A task's parent may itself be a task (`tasks.rs` :194 and `stops.rs` :66 both guard depth). Walk
  the whole `parent` chain, not one step — a helper under a task under a conversation must reach the conversation's
  window, which is where the owner's session is. Bound the walk (a small constant, or a seen-set) so a cycle in a
  corrupt store cannot loop forever, and say what bound you chose.
- **The parent's own line then stops the parent.** No new stop path: `burn_stopped` (`overdraw.rs` :125-135) already
  refuses the parent's next provider call with `KernelError::BurnRate` once `burn_stop` reaches the line, and the
  core ends the turn and says why (`turn.rs` :407, :499 — read, do not edit). Check the words the parent sees name
  the aggregate honestly; if the existing message would be misleading when the spend came from children, say so in
  the report rather than editing `turn.rs`.
- **Resume clears the whole family.** `burn_clear(&e.id)` is called on an approved budget question
  (`budget_question.rs` :221). Decide whether resuming a parent should also clear its children's windows, and
  whether resuming a child should clear its parents' — then make the behaviour deliberate and tested. The failure to
  avoid: the owner approves a parent after a fan-out burned $200, the parent's window is cleared, and the children's
  costs settle into it again a second later and stop it immediately. Say what you chose.
- **Memory.** `burn_record` retains at most 512 execution rings (:591-597). Every helper now writes two or more
  entries, so a wide fan-out adds rings faster than before. Check the retain rule still bounds it and say so; do not
  raise 512 (that is a cap).

**2. The parent's hard multiple at carve time.**

The design asks for this in the same row and did not trace it: **does `carve_task` check the parent's hard multiple?**
The hard multiple is `KernelConfig::spend_hard_multiple` (`[kernel] spend_limit_hard_multiple`, 10 by default;
`overdraw.rs` :14-21), which refuses a provider call whose reservation would take the spend past that multiple with
`KernelError::HardStop`. Read `tasks.rs::carve_task` and `overdraw.rs` and answer, in the report, with `file:line`
evidence:

- whether a carve consults the parent's hard multiple at all;
- if it does not, whether a fan-out of many helpers can carve past `hard_multiple × limit` without any single
  provider call tripping the stop.

If the answer is that it cannot (because each helper's own calls are still refused at the multiple), **say so and
change nothing** — a correct finding is the deliverable. If it can, add the check **only** if it is a tightening
(refusing a carve that passes the parent's hard multiple) and it needs no new config key; if it needs anything looser
or any new knob, stop and write `OWNER CALL:` instead. Do not guess: this is the part most likely to tempt a
plausible-looking change that nobody asked for.

### Tests (each red at the base, with a planted revert)

In `crates/theseus-kernel/src/tests_runaway.rs`, beside the existing burn tests (see :54 `braked`, :214 and :243 for
how they read `burn_stop`).

1. **Two children trip the parent's line.** A parent and two task children of it. Each child settles a provider cost
   of just over half the burn line within the window (the tests set the line and the clock: follow how the existing
   tests do it). Assert: neither child's own window has reached the line; the **parent's** `burn_stop` has; and the
   parent's next provider call is refused with `KernelError::BurnRate`. Plant P1: drop the parent record from the
   settle path: red.
2. **Grandchild to the top.** A conversation, a task under it, a helper task under that. The helper's settled cost
   appears in the helper's, the task's and the conversation's windows. Plant P2: walk one step instead of the chain:
   red.
3. **The day is counted once.** The same settles as test 1: `Today`'s `spent` (and the day's held figures) equal what
   they are without this change — write the expected figure explicitly, do not compute it from the new code. Plant
   P3: book the parent's copy through `settle`/`book` instead of `burn_record`: red (the day doubles).
4. **Nothing is booked for a frame that failed.** A settle inside a transaction that then fails: no burn entry for
   the child and none for the parent (the `day_rewind` path, `tx.rs` :97-107). Plant P4: record the parent's copy
   outside the effect, before the commit: red.
5. **A plain task is unchanged, and an execution with no parent is unchanged.** A top-level conversation's settle
   writes exactly one ring entry, as today.
6. **Resume**, as you decided it: a test that pins the choice, with its reason in the test's message.
- **Keep green:** every existing test in `theseus-kernel` — in particular `tests_runaway.rs` whole, the day-ceiling
  tests, `tests_tasks.rs`, `tests_frames.rs`' frame goldens and `tests_deferred.rs` — and `theseusd`'s money and task
  tests. Note that `theseus-core`'s `aws/hands` runaway tests read the wall clock's hour and fail across an hour
  boundary (theseus-5a50): rerun them mid-hour rather than explaining them away.
- If an existing test asserts that a child's cost does **not** reach its parent's window, that is the old rule:
  **do not delete or weaken it.** Rewrite its expectation with the same number of assertions, keep its name, and list
  it in the report under **Keel findings expected** with its line and reason.

### Not in this row

No change to `DEFAULT_BURN_MICROS`, `BURN_WINDOW_MS`, `DEFAULT_CEILING_MICROS`, the runaway line, the hard multiple
(`spend_limit_hard_multiple`), the 512-ring retain, or any other cap, limit or default — this row only makes an existing line see more of
what it was always meant to see. No new config key. No `turn.rs` edit. No admission change (another row in this lane
owns `turn/admission.rs`). No `task.wait` or `task.fan_out` work. No cockpit or `.tsx` change. No benchmark, no spend,
no AWS, no model call.

## Your lane: `integ/p3-fanout`, and the four rows beside you

All five rows of this lane fire at once off `0fda4d57`, so their branches do not see each other. The integrator joins
them in this order:

1. `p3h-self-started` (theseus-o1h3f.1) — `theseus-protocol/src/tasks.rs`, `theseus-core/src/task.rs`, the CLI's task
   renders.
2. `p3h-task-wait` (theseus-z9mze.2) — new `theseus-core/src/task_wait.rs`, `toolrun/waits.rs`, `turn/join_step.rs`,
   `keep_warm_wait.rs`, `toolrun/parallel.rs`, `TurnCtx.room` in `turn.rs`.
3. `p3h-fan-out` (theseus-z9mze.3) — `theseus-core/src/task.rs`, new `turn/helper_step.rs`, `thread_context.rs`, the
   `ask_budget` caller in `turn.rs`.
4. `p3h-admission-headroom` (theseus-o1h3f.4, `turn/admission.rs`) and **this row**, before the lane's gates.

**You touch `theseus-kernel/src/day_ceiling.rs` and `tests_runaway.rs`, and no z9mze row touches either.** Rows 2 and
3 read the kernel's task API (`take_reports_of`, `Opening.helper`) but do not edit `day_ceiling.rs`, `overdraw.rs` or
`tasks.rs::carry`. If your choice (a)/(b) above makes you touch `tasks.rs` or `kernel.rs`, keep the hunk to the call
line and name it with its line range in the join notes, because `kernel.rs` is 29 lines from its ceiling.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If anything
  of yours would change a message of that flow, STOP that part and write `OWNER CALL: <what and why>`.
- **Keel.** The gate's keel phase (`scripts/keel-guard.py`; its docstring lists the rules) fails a change that
  deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench
  budget, a spend or loop cap, or a ceiling in `scripts/long-files.txt`, `scripts/shape.sh` or `.config/nextest.toml`,
  or edits the guard's own files, unless a commit signed by the owner's key acks it. **You cannot sign, so you cannot
  ack, and you must never add a `Keel:` trailer yourself.** Run `python3 scripts/keel-guard.py` before every commit
  and `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and paste the whole
  output. Money is the keel's own territory, so read any finding here carefully: **a finding that says you raised or
  loosened a spend line means you did, and the fix is to put the old value back and write `OWNER CALL:`, never to
  argue with the guard.** A tightening draws no finding.
- **Data floor:** nothing new is sent to a model or routed anywhere. Costs already recorded are recorded in one more
  in-memory window.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it, and run `git status` after every restore); right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-kernel/AGENTS.md` first): `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-kernel -p
  theseus-core` plus `theseusd`'s money tests. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp
filesystem has committed an empty report here before), and check after the commit that it is not empty. Push the
branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51) and `SCHEMA` (2), both untouched or why not; ceilings;
any `OWNER CALL:` or STOP; and one line saying no limit was raised. (2) **Where the parent's record happens:** (a) or
(b), why, what you proved about the lock order and the rewind path, and the chain's bound. (3) **What is counted
once:** the day's total before and after, with the figures. (4) **The stop:** what the parent sees when children
burn its line, and whether the words are honest. (5) **Resume:** the choice and its reason. (6) **The hard multiple
at carve:** your finding with `file:line` evidence, and whether you changed anything. (7) **Tests** and plants P1-P4
(and 5, 6), red at the base and green after. (8) Any existing test whose expectation you rewrote, under **Keel
findings expected**. (9) **Gate** commands and results. (10) **Keel** output, whole. (11) **DM golden** diff result.
(12) **Join notes:** every file touched with line ranges, and a statement that no file of the other four rows in the
lane was touched. (13) **Follow-ups.** (14) One line on the data floor.
