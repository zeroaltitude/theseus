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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261013-modes-r2`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

---
## Your task: modes round 2, the re-integration the review asked for (theseus-lyxy)

Branch: `cloud/20261013-modes-r2`. Every commit's subject carries `theseus-lyxy`. Deadline for the report: 3 hours after you start.

**What happened.** Round 1 built this row on branch `cloud/20261013-modes` (head d6d04c39). The joiner's reviewer held it; the
review is quoted verbatim below. **Start from that work on today's main:** `git fetch origin cloud/20261013-modes` and
merge it into your branch with a plain merge (never a rebase), resolve against main, then do exactly what the review's
re-integration section says, keeping everything it found sound. The owner's decisions it must meet: one plan (plan mode's steps ARE progress's plan items, one store, one view), and new model tools deferred by default (its two tools go into a deferred family; the first request's size must not grow).

**Main today:** store format 43 (if you must bump, take the next free number at your commit and say so). On main: chat,
chat-ask, progress (plan items), deferred-tools (families, `tools_find`), token-budget (the turn bench fails above
8,000 tokens as sent in cli/task and 15,000 in the owner's DM; the DM has about 67 tokens of room, so add nothing to
any always-loaded set; a new tool goes into a deferred family). Never loosen a test, budget or ceiling. Run the row's
own tests 5 times (a flake is a finding, not a retry). The report says, per review point, what you did.

## The review that held round 1 (verbatim)

Verdict: **HOLD for a re-integration** (reviewer C, 2026-10-14): the owner's decision 2 (one plan) and his rule for new tools (deferred by default) are not met, and neither is a join fix

# modes (theseus-lyxy): review, reviewer C

- Branch `cloud/20261013-modes`, code head **1a8aee6d** (one code commit, 72 files, +3,090/−64; CLOUD_REPORT
  d6d04c39, 113 lines, read whole). Base fcdef5e0. No store-format change. Not merged for a gate: see below.
- **What it is**: `session.mode` (`ask`, `edits`, `plan`, `auto`) for one conversation, in memory with one ledger
  row per change and read back after serving; `edits` lowers the file tools under the session's directory to `notify`
  through scoped grants' `grants::covers`; `auto` lowers what only the posture asked to `notify`; `plan` refuses every
  non-read before the order runs and offers `plan_present` (the plan as a question; `e` approves with `edits`) and
  `plan_step` (moves a checklist). Shift+Tab in the chat and the board, `theseus mode`, Discord's board line, the F10
  arm's s5 and s12.

## Why it is held
1. **The owner's decision 2 (2026-10-13 23:24, `~/reports/theseus-decisions/1013-block.md`): "Plan mode's steps are
   the work board's plan items (row `progress`), never a second list of steps."** The row keeps its own
   `PlanStep {text, state}` list on the session (`policy/modes.rs` `ModeState::steps`, the `session.mode` ledger
   rows, `protocol::modes::PlanStep`), as its prompt allowed only while progress had not joined ("the second to join
   converts"). progress (theseus-xlias) is on main since 87009cdb (batch 5): plan items are title-only task records
   (`task_create` with a `title` and no `brief`, closed with `task_close`; `task_graph::PLAN_GUIDE`/`PLAN_NOTE`), and
   the board counts them. modes is the second to join, so the conversion is its own, and it is a design job, not a
   join fix: `plan_present`'s approved steps become plan items (with what parent, for a conversation that is not a
   task? a conversation's items are root records on the board today), `plan_step`'s `doing` has no task state to map
   to (`done`/`skipped` are a `task_close`), the checklist and `plan 2/5` read the items, and the session's ledger row
   stops carrying steps.
2. **Its two tools load in every request.** `plan_present` and `plan_step` are in no tool family
   (`tools_defer/family.rs` puts an unknown wire name in `other`, which always loads), so with deferred-tools on main
   they add about 700 tokens to every first request in every place (the row's own measure: it raised three
   request-size calibrations, `tests_tiering.rs` 41,100 to 41,800 and `tests_route_keep.rs` WINDOW 42,200 to 42,700
   and BILLED 36,700 to 37,200; main has since moved them to 43,100/43,900/38,400 for other rows). The owner's rule
   (2026-10-14 16:36, the brief's first-request update): a new tool is deferred by default and always loaded only if
   the ledger shows use in about 1 of 5 sessions of that place; token-budget (theseus-to0xw, building now) fails the
   gate past 8,000 tokens for the cli's first request. The tools belong in a family (`tasks`, beside the plan items,
   is the natural one) and should load while the conversation is in `plan` mode (the refusal names `plan_present`, so
   a deferred tool in plan mode costs a search on every plan).
3. **The merge on current main is 18 files of conflicts** (on main a3812226 plus chat-ask, chat-sessions and
   context-meter: the chat's dock, events, keys, state and mod; the TUI's app.rs and ui.rs; protocol lib.rs, events.rs,
   ts.rs and the wire fixture's count; the CLI's main.rs and client.rs; theseusd's main.rs; F10's arm and steps.py;
   the two calibration tests). Mostly add-both, but the calibrations must be re-swept after (2), and the chat's key
   table now holds chat-ask's question keys, which the plan question's `e` must share (the report's double `[e]`).

## What I read and found sound (for the re-integration to keep)
- The security claim holds as written: a mode acts in one place (`policy::modes::gate`, inside `Layer::Scoped` after
  scoped grants), lowers only `approve` to `notify` and only when `posture_only` holds (nothing the floor, a layer-1
  change, the approve lists, a path outside the roots or an AWS rule asked), in a private place, outside L1, with no
  `env` and no tightening; every later layer still raises it. `plan` is a refusal by tool class before the order
  (`refusal_at`), so no layer can pass it. The loosening modes need `judge_act(Act::Session)` (an MCP client is
  refused) and the CLI refuses `session.mode` inside a job (the usual speed bump). A plan's `e` must echo the daemon's
  own offer. The row's four plants (floor, plan letting `proc.run` through, edits outside the directory, a shared
  place reading the mode) were each caught by its tests.
- For the owner (not blocking, from the report): `auto` is broader than any grant, by his definition; a mode survives
  a daemon restart (read back from the ledger), a plan's pick does not (it leaves `ask`); network reads still run in
  `plan`.

## What the re-integration needs (a cloud row, or a reviewer with most of a day)
1. Plan steps as progress's plan items (decision 2), with the board's count and the chat's checklist reading them.
2. `plan_present`/`plan_step` in a family (deferred), loaded while the conversation is in `plan` mode; then re-sweep
   the request-size calibrations rather than raise them by the tools' full size.
3. Build on current main with chat-ask (its key table and the question's keys: one `e`) and token-budget if it has
   joined; then the usual review.

## The original task (round 1), for reference

---
## Your task: modes, Shift+Tab cycles ask, edits, plan and auto for the conversation; plan mode is read-only and shows its plan for approval before any edit, then its steps' progress (theseus-lyxy)

Branch: `cloud/20261013-modes`. Every commit's subject carries `theseus-lyxy`. Deadline for the report: 4 hours after
you start.

**Prerequisite:** `scoped-grants-join`: you build on main after scoped-grants (theseus-drjs) has joined. Read its merge
first: the session grants, `grants::covers`, the gate's order (floor, `approve_paths`, place rule, L1 egress, then
grants), `policy.granted` rows.

**Main, as you clone it.** You clone main at `e8444d1c` or later. The preamble above is older; where it differs, this
section wins. **The store's format is 32** (`MANIFEST_FORMAT`, crates/theseus-store/src/store.rs); a bump is one at a
time, with the old layout's reader and its `tests_layouts` sample, and changes two goldens (core_output.txt and
kernel_frames.txt). Read `git log --oneline -30` first. Joined since the preamble and worth knowing: session-dir (a
session keeps its directory), tool-input-fit (a result's first line `[exit code 0 · in /app]`), output-kept, job
handles (`proc_run {background}`, `job_read`, `job_wait`, `job_stop`), earned-autonomy (`policy/earned.rs`: argv-prefix
grants, `theseus policy earned` and `revoke`), why (`theseus why`, `context.explain`'s parts with their tokens,
protocol `why.rs`), fork-a (`session.fork` by reference), standing, money-soft, tui-paste (a paste is one event), and
the F10 driver (`scripts/f10/`).

**Line ceilings** (scripts/long-files.txt; a Rust file not listed may not pass 2,500 lines; never raise one, because
the keel guard fails it): CLI `crates/theseus/src/render.rs` 3,016 of 3,128; protocol `lib.rs` 2,647 of 2,733;
`crates/theseus-core/src/toolrun.rs` **2,499 of 2,500**; `compiler.rs` **2,559 of 2,560**; `turn.rs` 3,496 of 3,525;
Discord `render.rs` 2,886 of 3,009 and `runtime.rs` 3,476 of 3,500. New code goes in modules of its own; a long file
gains only a `mod` line or a call, and where it has no line left, move a few lines out first. **The keel guard** runs
first in every gate: never delete or weaken a test, add an `#[allow]`, or raise a budget, a cap or a ceiling. List
anything you could not avoid under **Keel findings expected**. Never edit scripts/gate.sh or scripts/keel-guard.py.

**F10, the yardstick this row answers to.** F10 is the first ten minutes with a coding agent: 12 steps, 42 match checks
and 5 beat checks, read from the screens (docs/benchmarks/f10/README.md; the checks are `scripts/f10/checks.py`, their
ids never renumbered; the Theseus arm is `scripts/f10/arm_theseus.py`, on the stand-in model, $0). The live run of
2026-10-12: **Theseus 11 of 42** at both sizes (it passes S1d, S3a, S3b, S3d, S6c, S6d, S7c, S8a, S8c, S10b, S11a) and
all 5 beats; **Claude Code 25 of 42**. Its cause, in that report's words: "a bare `theseus` opens no conversation,
replies are raw markdown cut at the edge, a tool line shows only the tool's name, and an edit is approved without its
diff". This row:
1. makes the checks named below pass on its build's screens, and keeps every check that passes today passing;
2. teaches `arm_theseus.py` the key or command its feature adds, in the step methods it names only (other rows edit
   the other steps; an arm with no key records `None`, and its checks say so);
3. runs the driver on its build, `python3 scripts/f10/driver.py theseus --bin-dir <build> --out /tmp/f10` (both sizes),
   then `scripts/f10/scorecard.py`, and puts the per-step table, main's 11 against the branch's, in CLOUD_REPORT.md.
   Use `scripts/build.sh --profile release-thin` when time allows; on a debug build the timing checks (S1b, B1) don't
   count, so say which build ran.

Never change a check, its words, its weight or the scorer to make it pass: a check is a test (keel). A check you think
is wrong goes in the report, unchanged. Don't re-record `scripts/f10/fixtures/` (the maintainer re-records after the
join); `scripts/f10/test_f10.py` must pass as it is. The Claude Code arm is not yours to run.

**F10 checks this row turns green:** S5c (one key switches to accepting file edits for the session, and the screen says
so), S12a (a read-only planning mode one key away, named on the screen), S12b (the plan shown for approval before any
file changes), S12c (progress through the plan's steps: two or more, one done or under way). **Arm steps you edit:**
`s5` (its mode key, after scoped-grants' grant key) and `s12`: Shift+Tab on whichever surface the arm uses there (the
chat once chat-ask has joined; else the TUI), and record the keys.

**Why this work exists.** F10's S12 is 0 of 3 for Theseus (Claude Code 1 of 3), and S5c fails for both. Decided at the
stopping point (B5): a conversation starts in the config's posture; a mode lasts for the conversation; `auto` means
`notify`. Security review: a mode can never pass the floor.

**What to build:**
1. **`session.mode {session_id, mode: "ask" | "edits" | "plan" | "auto"}`** (a protocol module of its own) and the
   mode on the session's view and push frames, so every surface shows it. An overlay on the config's postures for this
   session only, never written into the config:
   - `ask`: the config's postures, as today;
   - `edits`: file tools (`fs.edit`, `fs.write`, `fs.patch`) under the session's directory don't ask: an edits grant held
     by the mode, through scoped-grants' matching, so its notice line and its limits are the grant's;
   - `plan`: read-only. Writers and `proc.run` are refused with a result the model can act on ("plan mode: read-only;
     present the plan with plan_present"), and a `plan_present {steps: [..]}` tool whose question is the plan, shown as
     a numbered list. Approving it moves the session to `ask` or `edits` (the person's pick on the question) and records
     the plan's steps on the session;
   - `auto`: the `notify` posture for this session (it runs and tells; the floor still asks).
   The floor, `approve_paths`, the place rule and the L1 egress ask come first in every mode. A shared place has no
   mode but `ask`. Follow the minimal-prompting order: the tool's description and the refusal's words, no prompt
   sentence.
2. **Progress through the plan.** A `plan_step {index, state: "doing" | "done" | "skipped"}` tool updates the recorded
   steps; the surfaces show the checklist (`☐ ◐ ☑`) in the transcript under the approved plan and as one line in the
   status line (`plan 2/5`). The steps live on the session's record as a ledger row per change (no store format bump;
   say if you find you need one).
3. **The keys and words:** Shift+Tab cycles `ask → edits → plan → auto` in the chat (its key table in
   `crates/theseus-tui/src/chat/keys.rs`, if chat has joined) and in the TUI; the status line names the mode
   (`edits (shift+tab)`); `theseus mode <ask|edits|plan|auto> [SID]` on the CLI; Discord shows the mode on its board line
   only (no key).

**FAST:** one field read per gated call; no store read per call.

**Tests, plants and the security review:** `edits` lets file tools in the session's directory through and still asks
for `proc.run`, for a path outside the directory, and for a floor path; `plan` refuses each writer and `proc.run` with
the result's words, and offers `plan_present`; approving the plan moves the mode; `plan_step` updates the checklist;
`auto` is `notify` and the floor still asks; a shared place refuses every mode but `ask`; the mode does not survive into
a new conversation. Plant a revert per rule (the floor before the mode; plan letting `proc.run` through; edits reaching
outside the directory) and record what caught it. The security review goes in CLOUD_REPORT.md.

**Live check on this VM:** a scratch daemon on the stand-in: Shift+Tab to `plan`, a stand-in rule that calls
`plan_present` then edits after approval, the plan question, the checklist moving; `edits` then an edit with no
question; captures in the report. Then the F10 driver.

**Siblings:**
- **chat-ask** (theseus-wy6v.1) owns the chat dock's key table: add Shift+Tab as a row of it. If chat-ask has not
  joined, add the key to the chat's `keys.rs` as chat left it; the second to join reconciles.
- **progress** (theseus-xlias, after work-board) lays a task's work out as 39a plan items and puts their count on the
  work board. A plan is one notion: if progress has joined when you build, record `plan_present`'s steps as its plan
  items and let `plan_step` move them; if not, keep them as plain session data (`PlanStep {text, state}`) shaped like
  39a's items, and the second to join converts. Name the shape in the report.
- **rewind** (theseus-23rk) restores files; a plan's approval is not a restore point.

**Doing more, filing less:** fix a defect you find in code you are already in, with a test, and name it in the
report. No person's name anywhere: the owner is "the owner". No secrets. Every new config key has a default and a line
in the template. Run the gate before every commit, and commit CLOUD_REPORT.md as the branch's last commit.

The issue: `theseus-lyxy`.

End with a short final message starting `CLOUD REPORT COMPLETE`.

## The owner's decisions (2026-10-13 23:24 MST, verbatim: "i'm happy for yes to all until we're at testing time to eval")
They hold until the evaluation pass; build so each is easy to revisit.
1. **One conversation surface.** Bare `theseus` opens the chat (row `chat`, theseus-cyha, owns it); `theseus chat` is the same thing; the task footer and one-key answers (row `cli-tasks`) live inside that chat, not in a second CLI.
2. **One plan.** Plan mode's steps are the work board's plan items (row `progress`), never a second list of steps.
3. **One owner for Discord threads.** Activity threads (row `thread-discord`, T6) decide which Discord thread a task or message lands in; task rows (`discord-guild-tasks` and others) use its helper and never create threads on their own.
4. **One pacing rule for unprompted posts** (the digest, the morning/horizon view, nudges, the always-on DM ping, proactive voice). Every unprompted post goes through one shared pacer: at most one unprompted DM post per 30 minutes, later items batched into it; the first such row to join builds the pacer (a small module beside the push), later rows use it.
5. The size check joins the gate (a build fails if the binary grows more than 1 MB; the 60 MB miss warns). 6. Programs at once 8 -> 16 (reads 32). 7. Scoped grants are built on Opus with a security review.
