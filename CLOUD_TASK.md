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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3i-task-tools-wake`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
- `git commit --allow-empty -m "theseus-o1h3f.3: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3i-task-tools-wake`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches. **A routine
that fires and pushes nothing is indistinguishable from a dead one.** If your work fails, or you run out of time
mid-step, push anyway, with your findings in `CLOUD_REPORT.md`.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs` :688).
This row stores nothing new, so leave it alone. The kernel's `theseus_kernel::types::SCHEMA` (2, `types.rs` :33) is
not yours to touch either.

**Line ceilings — never raise one** (`scripts/long-files.txt`; every Rust file the list does not name fails past
2,500 lines). Measured at your base: `crates/theseus-core/src/turn.rs` 3,498 of 3,525 (**do not edit `turn.rs` at
all in this row**; sibling rows in flight need its remaining lines), `crates/theseus-discord/src/runtime.rs` 3,496 of
3,500, `crates/theseus-protocol/src/lib.rs` 2,612 of 2,733. Your own files are nowhere near one:
`tools_defer/family.rs` 427, `tools_defer/want.rs` 462, `task.rs` 1,443 (unlisted, so its limit is 2,500),
`arrangement.rs` short. Put new tests in a `tests_<subject>.rs` of their own.

## Why this row exists

The owner's standing line, 2026-10-24 21:52: *"we should be very very lightweight on any limits to theseus tasking
itself based on what it sees and what we are working on. notify, work, and allow feedback is the model."* He answered
the design's questions **"All recs!"** (10-24 23:47) and **"Yes to all"** (10-25 09:54).

The design's inventory found 45 gates on self-started work and called **this row the real limiter**: gate #1, the
first of the four that matter most. In a DM and at the CLI the whole `tasks` family is **deferred** — it loads only
when the owner's message *asks for a task* (`tools_defer/want.rs` `asks_for_task`). So in a **wake turn, a task
report turn, or a plain DM turn where Theseus noticed work**, the model cannot see `task_create` at all unless it
searches for it. Everything else in the audit is downstream of that: there is no point loosening what a task may do
if the model cannot see the tool that starts one.

The deferral itself is right and stays: it exists because the `tasks` family costs **13,290 tokens** of the DM's
15,000-token first-request budget (the owner's rule of 2026-10-14, and those token budgets are keel-guarded in
`crates/theseus-core/src/first_request.rs`). The fix is narrow: load `tasks` on the turns that have **no owner input**
— where no budget was promised to a person — and leave a person's turn exactly as it is today.

## Your task: p3i-task-tools-wake, the task tools load when Theseus noticed the work (theseus-o1h3f.3, o1h3f row C)

Branch: `cloud/20261025-p3i-task-tools-wake`.

Every commit subject carries `theseus-o1h3f.3`. Deadline for the report: about 2.5 hours after you start.
**Your specification is the **o1h3f design check** (the maintainer holds it; it is not in this repository), section 4 row C, with section 1.1
items 1 and 7 as its reasoning.** You do not have that file — its content is reproduced below. Do not re-derive the
design; build what is written here.

Read first: `crates/theseus-core/src/tools_defer/family.rs` whole, and especially the per-place default list at
:272-290 (`PlaceKind::Cli => &["memory"]` at :272, `PlaceKind::Dm => &["memory", "reminders", "web"]` at :286, with
the comment at :283-285 explaining that the tasks family was deferred there because `task_create`, `task_update`,
`task_close`, `task_claim` and `task_split` had no call in 30 days and cost the DM's request 13,290 tokens);
`tools_defer/want.rs` `asks_for_task` (:89) with its `MAKES` verb list and `GAP`; `crates/theseus-core/src/turn/
admission.rs` `Class` (:55-64) and `Class::of(kind, has_input)` (:69-76); the `t.tc.class` read in
`turn/reply_step.rs` (~:93); `crates/theseus-core/src/task.rs`'s `create` and the tool's description; and
`crates/theseus-core/src/arrangement.rs` (`REFUSAL` at :47, `MIN_QUOTE_CHARS`, `MAX_PIECES`, and the fidelity
constants below it).

### What to build

**1. Load the `tasks` family on turns with no owner input.**

A turn with no owner input is one whose class is not the person's: a wake, a task report, a continuation, a routine's
run. **The turn already knows this** — `Class::of(kind, has_input)` in `turn/admission.rs` :69-76, carried on the
turn's context and read as `t.tc.class` in `reply_step.rs` ~:93. Follow that read. Do **not** invent a second notion
of "no input", and do **not** derive it from the session kind alone: a task session's turn is `Class::Task` whether or
not someone typed into it, and that distinction is exactly what must not be lost.

- On such a turn, the `tasks` family is **not** deferred, in a DM and at the CLI both.
- **A person's turn is unchanged.** Today's deferral, today's `asks_for_task` search path, today's token budget. A
  test must prove the person's turn's loaded families are byte-identical to the base.
- Also load `tasks` when **`offer_task` said yes** — the judgment `By::Jev` path (`judge/inbound.rs` :59, read by
  `turn/reply_step.rs` :86-104). If that signal is not reachable where the deferral is decided at your base, say so
  in the report with the `file:line` you looked at, implement the no-owner-input half (which is the large half), and
  leave the offer half as a named follow-up. **Do not restructure the turn to reach it** — that is `turn.rs`, which
  this row may not touch.
- `PlaceKind::Channel` already loads `tasks` (:287) and `PlaceKind::Task` loads nothing by default (:290): leave both
  alone.

**2. The arrangement's words accept work Theseus noticed itself** (design §1.1 item 7).

`task.create` needs an `arrangement` quoting "the message that asked for it", and refuses without one
(`arrangement.rs` :47 `REFUSAL`: *"Promotion needs an arrangement: quote the messages that define this work."*). On a
self-started turn there is no such message, and the refusal's words point the model at something that does not exist.

The description and the refusal **already allow** a quote from "your own earlier replies, or a tool result" — so this
is a wording gate, not a logic one. Add to both the tool's description and the refusal one short sentence to the
effect of: *for work you noticed yourself, quote the tool result or your own line that shows it.* Match the file's
voice; keep it to one sentence in each place.

- **Change no rule.** `MIN_QUOTE_CHARS`, `MAX_PIECES`, `PIECE_CHARS`, the fidelity constants and the
  must-match-exactly-one-node rule all stay exactly as they are. This is text.
- The `task.create` description change is **this one sentence only**. A `because` input and its own sentence belong to
  row A, which is in flight; do not add, rename or re-describe `because`.

### Tests (each red at the base, with a planted revert)

Put new tests in a `tests_<subject>.rs` of their own where the crate's convention allows, or beside the closest
existing deferral tests; name the file in the report.

1. **The wake turn sees the tool.** A DM-bound session's wake turn (no input) has `tasks` among its loaded families
   and `task_create` in the tools it offers the model. The same for a CLI place. Plant P1: restore the unconditional
   deferral: red.
2. **The person's turn is untouched.** A DM turn with the owner's input that does **not** ask for a task has exactly
   the base's loaded families and exactly the base's first-request token count — assert the family list and the token
   number, not just "tasks is absent". Plant P2: load `tasks` on every DM turn: red (the token assertion fires).
   **This test is the row's safety property**: it is the evidence that the owner's 13,290-token rule still holds.
3. **The search path still works.** A person's DM turn whose message *does* ask for a task still loads `tasks`
   through `asks_for_task`, unchanged. Plant P3: break `asks_for_task`: red.
4. **The class, not the session kind.** A **task** session's turn **with** typed input is a person's turn for this
   purpose and does not get the loosening; the same session's wake turn does. Plant P4: decide from the session kind
   instead of the turn's class: red. This is the mistake the design explicitly warns against.
5. **The offer's yes** (only if you implemented that half): a turn where `offer_task` said yes loads `tasks`. Plant
   P5: drop that arm: red.
6. **The words.** The refusal's text and the tool description each contain the new sentence; the refusal still refuses
   a `task.create` with no arrangement at all, with the same behaviour as the base. Plant P6: revert the text: red
   (assert on the string).
- **Keep green:** the whole `theseus-core` suite, `theseus-protocol`, and in particular **every existing
  tools-deferral and first-request test**. If a first-request token test's number moves, that means the person's turn
  changed and your read is catching a turn it should not: **fix the read, do not edit the test.**
- If an existing test does have to change its expectation, **do not delete or weaken it**: rewrite it with the same
  number of assertions, keep its name, and list it under **Keel findings expected** with its line and reason.

### Not in this row

No `TaskOrigin`, `self_started` or `because` (row A, in flight). No `task.not_this`, no standing preference, no judge
label, no Ops or cockpit change, no `.tsx` edit (row B). No RSI change (row F). No admission or burn change (rows D
and E). **No Discord change of any kind** and `golden/dm_flow.txt` stays byte-identical: the DM task flow is locked.
No `max_wakes` change (row H). **No `turn.rs` edit.** No new config key. No change to any number in
`crates/theseus-core/src/first_request.rs` — those are the owner's signed token budgets and they are keel-guarded. No
new dependency. No benchmark, no spend, no AWS, no model call.

## Shared files: fires are parallel, joins serialise

Eleven other patch3 rows are in flight right now against this same `0fda4d57`, in three lanes. That is deliberate:
the review queue, not the build, is our bottleneck, so rows fire concurrently and the integrator serialises the joins.

**Your shared file is `crates/theseus-core/src/task.rs`.** Three other rows own hunks in it:

- **`theseus-z9mze.3`** (branch `cloud/20261025-p3h-fan-out`, lane `integ/p3-fanout`) factors an `open_one()` out of
  `create()` so a fan-out can open several tasks in one call, and adds the `task.fan_out` tool definition.
- **row A, `theseus-o1h3f.1`** (branch `cloud/20261025-p3h-self-started`, same lane) adds the `because` input, the
  self-started read and `by_model = true`.
- you, for the description sentence.

**The join order the integrator will follow is: `z9mze.3`, then A, then C (you). Repeat that line verbatim in your
own report** — it is the thing the joiner acts on.

**Write your row against `main 0fda4d57` as if you were the only one editing these files. Do NOT try to coordinate
with the other rows, do NOT stub, and do NOT guess at their shape.** Your branch cannot see theirs and theirs cannot
see yours; that is fine, because your join comes last and is a text change. To make that join mechanical:

- Keep your `task.rs` edit **as small and as local as it can be**: the description sentence and nothing else. A
  one-line text change inside a description string survives an `open_one()` factor untouched.
- **List every hunk you place in `task.rs` individually** — file, line range, function, and why — in the report's join
  notes. The joiner reads that list and nothing else.
- `tools_defer/{family,want}.rs` and `arrangement.rs` are yours alone; no row in any lane touches them. Say so.
- If you find you need a second `task.rs` hunk, or any hunk in `theseus-protocol/src/tasks.rs` (row A's and
  `z9mze.6`'s), **stop, leave that part out, and write `JOIN SEAM: <file> — <why I needed it>` in the report.**

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result.
- **Keel.** The gate's keel phase (`scripts/keel-guard.py`; its docstring lists the rules) fails a change that
  deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, weakens an assertion in place, adds an
  `#[allow]`, raises a bench budget, a spend or loop cap, or a ceiling, or edits the guard's own files, unless a commit
  signed by the owner's key acks it. **You cannot sign, so you cannot ack, and you must never add a `Keel:` trailer
  yourself.** Run `python3 scripts/keel-guard.py` before every commit and
  `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and paste the whole output.
  **If it flags a behaviour cap, STOP that part, put the old value back, and write `OWNER CALL:`.** The guard's
  `budget-raised` rule watches `first_request.rs`'s `CLI_TASK_TOKENS`, `DM_TOKENS`, `CHANNEL_TOKENS` and
  `HEADLESS_TOKENS` by name: you change none of them, and the report says so.
- **The token budget is the thing a reviewer will check hardest.** Give the report a table: for each of a DM person
  turn, a DM wake turn, a CLI person turn and a CLI wake turn, the loaded families and the first request's token
  count, at the base and at your head. The person rows must be identical.
- **No ceiling raised and no cap loosened without saying so explicitly in `CLOUD_REPORT.md`.**
- **Data floor:** the loosening sends **more tool definitions** to the model on turns with no owner input, and
  nothing else. No new data leaves the machine. Say that in one line.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it so cargo rebuilds it, and run `git status` after every restore); right over compatible.
- **The gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-core -p
  theseus-protocol` plus every other crate whose files you touched. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, **never a shell heredoc**, and check
after the commit that it is not empty. Push the branch. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched) and `SCHEMA` (2, untouched); ceilings; any
`OWNER CALL:`, `JOIN SEAM:` or STOP. (2) **The read:** how "no owner input" is decided, at `file:line`, and the cases
it deliberately excludes (the task session's typed turn above all). (3) **The token table** described in the rules.
(4) **The offer half:** done, or not done with the `file:line` you looked at and why. (5) **The words:** the new
sentence in the description and in the refusal, quoted. (6) **Tests** and plants P1-P6, red at the base and green
after, with counts. (7) Any existing test whose expectation you rewrote, under **Keel findings expected**. (8)
**Gate** commands and results. (9) **Keel** output, whole. (10) **DM golden** diff result. (11) **Join notes:** the
join order line `z9mze.3, then A, then C` verbatim; every file touched with line ranges; **every `task.rs` hunk named
individually**; and a statement that you edited no `.tsx` file, nothing under `crates/theseus-discord/`, not
`turn.rs`, and not `first_request.rs`. (12) **Follow-ups.** (13) One line on the data floor.
