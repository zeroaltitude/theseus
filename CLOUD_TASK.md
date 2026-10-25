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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3h-self-started`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
- `git commit --allow-empty -m "theseus-o1h3f.1: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3h-self-started`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs` :688).
This row adds two optional fields to a **stored** record (`TaskRecord.origin`), so read the root `AGENTS.md`'s "The
store's version rule" and decide honestly whether it needs a bump. Use `#[serde(default,
skip_serializing_if = ...)]` on both, so a record written before this change reads unchanged and a task that is not
self-started writes no new key. If you do bump, bump by exactly one from 51, add the literal sample of the old layout
to `theseus-core`'s `tests_layouts.rs` as the rule says, and name and justify it in your report's first section (the
integrator renumbers it at the join; a format bump is known to move two goldens, so say which). The kernel's
`theseus_kernel::types::SCHEMA` (2, `types.rs` :33) is **not** yours to touch: `terms.rs` marks every execution with
`schema < SCHEMA` as `legacy`, so a bump would relabel every stored execution.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every Rust file the list does not name fails past 2,500
lines). At the base: `crates/theseus-protocol/src/tasks.rs` **461**, `crates/theseus-core/src/task.rs` **1,443**,
`crates/theseus-protocol/src/lib.rs` **2,733 of its 2,733 ceiling — it is exactly full, so add nothing to it**, and
`crates/theseus-core/src/turn.rs` 3,498 of 3,525 (**do not edit `turn.rs` at all in this row**; two sibling rows in
this lane need its 27 remaining lines). `theseus-protocol`'s `ts.rs` type-list test sits at clippy's 100-line limit:
you add no new type, so you should not need to touch it — if you do, add to an existing line, never a new one.

## Why this row exists

The owner's standing line, 2026-10-24 21:52: *"we should be very very lightweight on any limits to theseus tasking
itself based on what it sees and what we are working on. notify, work, and allow feedback is the model."* He then
answered all fifteen of the design's questions with **"All recs!"**. This row is the **foundation of that model's
first half — notify**: before Theseus can tell the owner it started something by itself, and before the owner can say
"not this" about it, a task has to **know** it was self-started and **say why**.

It is also the foundation two later rows (F and G of the same audit) build on, and it is **row 1 of five that must
land in ONE install** — this lane's other four rows are the tasks fan-out and the two safety rows that make a
cap-free fan-out safe. Say in your report that you understand your row joins first.

## Your task: p3h-self-started, mark a self-started task and why (theseus-o1h3f.1, o1h3f row A)

Branch: `cloud/20261025-p3h-self-started`.

Every commit subject carries `theseus-o1h3f.1`. Deadline for the report: about 3 hours after you start.

Read first: `crates/theseus-protocol/src/tasks.rs` `TaskOrigin` (:107-118) and `TaskRecord` (:200-240);
`crates/theseus-core/src/task.rs`'s `create` path and the `TaskOrigin` literal at :931-936 (note its comment, "The
owner's: its objective and acceptance are the arrangement's", and `by_model: false`); `crates/theseus-core/src/turn/
admission.rs` `Class::of` (:69-76) and `Class` (:55-64); the `t.tc.class` read in `turn/reply_step.rs` (~:93);
`crates/theseus-core/src/arrangement.rs` (its refusal, ~:47); and `crates/theseus-core/src/task_graph/` (`tools.rs`,
`mod.rs`).

### What to build

**1. `TaskOrigin` learns that a task started itself, and why.** In `crates/theseus-protocol/src/tasks.rs`, beside
`by_model`:

- `self_started: bool`, `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, with a doc comment in the
  file's voice saying what it means and who reads it.
- `because: Option<String>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`, **at most 160
  characters** — the model's own one-line reason, in its own words. Enforce the cap where the input is read, not only
  by hope: a longer `because` is cut at a word boundary or refused with words that name the limit (your call; say
  which and test it).

**2. What counts as self-started.** A task is self-started when its `task.create` call ran in a turn with **no owner
input**: the turn's class is not the person's — a wake, a task report, a continuation, a routine's run — or it is an
RSI backlog item. The turn already knows this: `Class::of(kind, has_input)` and the class carried on the turn's
context. Read how `reply_step.rs` reads `t.tc.class` and follow that; do **not** invent a second notion of "no input"
and do not re-derive it from the session kind alone (a task session's turn is `Class::Task` whether or not someone
typed into it, and that distinction matters).

**3. `task.create` gains a `because` input, required on a self-started turn.** In `crates/theseus-core/src/task.rs`:

- a new optional `because` on the tool's input, described in one short sentence of the tool's description;
- on a self-started turn, a missing or empty `because` is **refused with words that name it** (follow how the file's
  existing refusals are worded and ledgered — see `arrangement::REFUSAL` and the `task.arrangement_refused` fact for
  the house style). On a turn with the owner's input, `because` stays optional and the behaviour is unchanged.
- `create` sets `self_started` and `because` on the `TaskOrigin`, and sets **`by_model = true`** for a self-started
  task. Read `by_model`'s doc first (:111-117): it means the model wrote the objective and acceptance, so a change to
  the task's layer 1 applies at once instead of waiting for the owner. That is the intended loosening here. Correct
  the `TaskOrigin` literal's comment at :936, which currently says the task is the owner's — it is no longer always
  true.
- Every other `TaskOrigin` literal in the tree must keep today's meaning. `TaskOrigin` derives `Default`, but the
  literals name their fields, so each one breaks when a field is added. Add the field (or `..Default::default()`) at
  each site with **one line each and no other change**: `crates/theseus-core/src/project/create.rs`,
  `rpc/projects.rs` (~:130), `rpc/task_start.rs` (~:619), `rsi/backlog.rs` (~:255 and ~:541), `task.rs` (~:931),
  `push/work/tests_step.rs` (~:70), `rsi/tests_pr.rs` (~:486 and ~:753), and wherever else the compiler names one.
  **`rsi/backlog.rs`'s two sites are the RSI backlog's own tasks: those ARE self-started** — set the field there
  deliberately, with a `because` drawn from the backlog item's own cause if one is at hand, and say in the report
  what you set and why. List every touched file in the join notes.

**4. The CLI and TUI say it.** `theseus tasks` and the task tree mark a self-started task with its reason — the
design's shape is one line, `◇ self: <because>` — in `crates/theseus/src/render/task_tree*.rs`, and the TUI's card
(`crates/theseus-tui/src/card.rs`, ~:160) shows the same line. Keep it to one line; a task that is not self-started
renders **byte-identical** to the base, and a golden test should say so.

**5. The cockpit half of this row is DROPPED, deliberately.** The design also asks for an Ops badge and a Graph card
line. **Do not write any `.tsx` change and do not touch `cockpit/src/views/Ops.tsx` or
`cockpit/src/components/TaskGraph.tsx`:** six Ops/cockpit rows are being built right now in another lane against
those exact files, and two lanes editing one surface is how we get semantic conflicts that a merge cannot see. The
badge follows in a later Ops wave. **You must still regenerate `cockpit/src/protocol.gen/*`** if your protocol change
moves it (the generator is `theseus-protocol`'s test; the root `AGENTS.md` says how), because the gate checks it —
that is generated output, not a cockpit edit. Say in the report that the badge was dropped and why, so the Ops wave
picks it up.

### Tests (each red at the base, with a planted revert)

Put new tests in a `tests_<subject>.rs` of their own where the crate's convention allows, or beside the closest
existing task tests; name the file in the report.

1. **The read.** A turn with no owner input (a wake) whose `task.create` runs: the stored `TaskRecord.origin` has
   `self_started: true`, `by_model: true`, and the `because` the call gave. A turn with the owner's input creating a
   task: both flags are as they are today (`self_started` absent, `by_model` false), and the record **serialises
   without the new keys** (assert on the serialized bytes or JSON, not just the struct). Plant P1: make `create` set
   `self_started` from the session kind instead of the turn's class: red (a task session's turn with typed input is
   marked self-started).
2. **The refusal.** A self-started turn whose `task.create` carries no `because` is refused, the words name
   `because`, and **no task is created** (assert the store has none). The same call with a `because` succeeds. A
   person-input turn with no `because` still succeeds. Plant P2: drop the requirement: red.
3. **The 160-character cap**, as you implemented it (cut or refused), with a 161-character input and a 160-character
   one. Plant P3: remove the cap: red.
4. **Old records read unchanged.** A `TaskRecord` written without the two fields (a literal sample, the way
   `tests_layouts.rs` keeps them) decodes as not self-started with no `because`. Plant P4: drop
   `#[serde(default)]`: red.
5. **The render.** The CLI tree and the TUI card show the one line for a self-started task with its reason; a task
   that is not self-started renders byte-identical to the base. Plant P5: drop the marker: red.
- **Keep green:** the whole `theseus-core`, `theseus-protocol`, `theseus` (CLI) and `theseus-tui` suites, and
  `theseusd`'s task tests. In particular every existing task-creation test must still pass **unchanged**: if one
  would now need a `because`, that means your "self-started" read is catching a turn it should not. Fix the read, do
  not edit the test.
- If an existing test does have to change its expectation, **do not delete or weaken it**: rewrite it with the same
  number of assertions, keep its name, and list it under **Keel findings expected** with its line and reason.

### Not in this row

No `task.not_this` RPC, no standing preference, no judge label, no RSI drop, no `initiative.rated` row (that is row B
of the same audit, a separate issue). No `tasks`-family loading change and no `task.create` description rewrite
beyond the one `because` sentence (that is row C). No `max_wakes` change (row H). **No cockpit or `.tsx` edit of any
kind** (see point 5). No Discord change of any kind: the DM task flow is locked, and the 🔔 line and the live card
stay exactly as they are — the "because" on a Discord message is an open owner question and is **not** yours to
answer. No `turn.rs` edit. No new config key. No benchmark, no spend, no AWS, no model call.

## Your lane: `integ/p3-fanout`, and the four rows beside you

All five rows of this lane fire at once off `0fda4d57`, so their branches do not see each other. The integrator joins
them in this order, and **all five must land in one install** (the owner's "All recs!" made the fan-out and its two
safety rows a required co-ship):

1. **this row** — joins **first**: it is the foundation the later o1h3f rows build on.
2. `p3h-task-wait` (theseus-z9mze.2) — new `theseus-core/src/task_wait.rs`, `toolrun/waits.rs`, `turn/join_step.rs`,
   `keep_warm_wait.rs`, `toolrun/parallel.rs`, `TurnCtx.room` in `turn.rs`.
3. `p3h-fan-out` (theseus-z9mze.3) — **`theseus-core/src/task.rs` as well as you**: it factors an `open_one()` out of
   `create()` so a fan-out can open several tasks in one call, and it adds the `task.fan_out` tool definition. **This
   is the one real seam in the lane.** Keep your own edits to `task.rs` as small and as local as you can: put the
   `because` validation and the self-started read in **small functions of your own**, called on their own lines from
   `create`, rather than as new logic woven into `create`'s body — that way an `open_one()` factor can carry your
   calls across without re-deciding anything. Name every hunk you own in `task.rs` with its line range in the join
   notes.
4. `p3h-admission-headroom` (theseus-o1h3f.4, `turn/admission.rs`) and `p3h-burn-parent` (theseus-o1h3f.5,
   `theseus-kernel/src/day_ceiling.rs`), before the lane's gates. Neither touches your files. **Row
   `p3h-admission-headroom` adds no field to `Class` and does not change `Class::of`** — it changes which queued turn
   is admitted — so your read of the turn's class is stable.

A sixth row (the Ops/cockpit half of the fan-out work) is deliberately **not** in this wave, for the collision reason
in point 5.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If anything
  of yours would change a message of that flow, STOP that part and write `OWNER CALL: <what and why>` in the report.
  This row is the most likely of its lane to tempt such a change (a self-started task's 🔔 line) — resist it.
- **Keel.** The gate's keel phase (`scripts/keel-guard.py`; its docstring lists the rules) fails a change that
  deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench
  budget, a spend or loop cap, or a ceiling in `scripts/long-files.txt`, `scripts/shape.sh` or `.config/nextest.toml`,
  or edits the guard's own files, unless a commit signed by the owner's key acks it. **You cannot sign, so you cannot
  ack, and you must never add a `Keel:` trailer yourself.** Run `python3 scripts/keel-guard.py` before every commit
  and `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and paste the whole
  output. **If it flags a behaviour cap, STOP that part, put the old value back, and write `OWNER CALL:` — do not
  raise the cap.** Note that `by_model = true` loosens when a task's layer-1 change applies: that is this row's
  intended behaviour and it is not a keel literal, but say it plainly in the report so the maintainer can carry it to
  the owner.
- **Data floor:** nothing new is sent to a model or routed anywhere. The `because` is the model's own words, stored
  and rendered locally.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner". A `because` in a fixture must be invented text, never anything that reads like a
  real person's words.
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it, and run `git status` after every restore); right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` (your literal sites span crates),
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-core -p
  theseus-protocol -p theseus -p theseus-tui` plus every other crate whose files you touched. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp
filesystem has committed an empty report here before), and check after the commit that it is not empty. Push the
branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51: untouched, or the bump named and justified with which
goldens it moves) and `SCHEMA` (2, untouched); ceilings; any `OWNER CALL:` or STOP. (2) **The read:** how
"self-started" is decided, at `file:line`, and the cases it deliberately excludes. (3) **The fields and the input:**
`self_started`, `because` and its cap, the refusal's words, and `by_model`'s loosening stated plainly. (4) **Every
`TaskOrigin` literal site**, one per line, and what you set at the two `rsi/backlog.rs` sites and why. (5) **CLI and
TUI:** the line, with a before/after sample. (6) **What was dropped:** the cockpit badge and Graph line, why, and
what the later Ops wave must add; plus whether `protocol.gen` moved. (7) **Tests** and plants P1-P5, red at the base
and green after. (8) Any existing test whose expectation you rewrote, under **Keel findings expected**. (9) **Gate**
commands and results. (10) **Keel** output, whole. (11) **DM golden** diff result. (12) **Join notes:** every file
touched with line ranges, the `task.rs` hunks you own named individually for the seam with `p3h-fan-out`, and a
statement that no `.tsx` file was edited. (13) **Follow-ups.** (14) One line on the data floor.
