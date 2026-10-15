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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261013-cli-tasks-r2`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: cli-tasks round 2, the re-integration the review asked for (theseus-mnd9g)

Branch: `cloud/20261013-cli-tasks-r2`. Every commit's subject carries `theseus-mnd9g`. Deadline for the report: 3 hours after you start.

**What happened.** Round 1 built this row on branch `cloud/20261013-cli-tasks` (head b6cfc4b0). The joiner's reviewer held it; the
review is quoted verbatim below. **Start from that work on today's main:** `git fetch origin cloud/20261013-cli-tasks` and
merge it into your branch with a plain merge (never a rebase), resolve against main, then do exactly what the review's
re-integration section says, keeping everything it found sound. chat-ask is on main now: reuse its question handling and its keys; delete the duplicate, keep one way to answer a question in the chat. The owner's decision: bare `theseus` opens chat; the cli-tasks footer lives inside the chat.

**Main today:** store format 43 (if you must bump, take the next free number at your commit and say so). On main: chat,
chat-ask, progress (plan items), deferred-tools (families, `tools_find`), token-budget (the turn bench fails above
8,000 tokens as sent in cli/task and 15,000 in the owner's DM; the DM has about 67 tokens of room, so add nothing to
any always-loaded set; a new tool goes into a deferred family). Never loosen a test, budget or ceiling. Run the row's
own tests 5 times (a flake is a finding, not a retry). The report says, per review point, what you did.

## The review that held round 1 (verbatim)

Verdict: **HOLD for a re-integration with chat-ask** (reviewer C, 2026-10-14): two rows built the chat's question-answering twice, with different keys; the merge is a design choice, not a join fix.

# cli-tasks (theseus-mnd9g): review, reviewer C

- Branch `cloud/20261013-cli-tasks`, head **b6cfc4b0** (code commits 6289128a, 7c986ad3, 08e5c1b7, 314b443e;
  CLOUD_REPORT b6cfc4b0, pushed 04:29Z, read whole; this review began from the diff before it landed). Base
  fcdef5e0. No store change, no config key, no protocol type. The report says its full gate did not finish (the
  workspace suite, cockpit, deny and the TS phase were not run on the final tree; disk full), and no live check.
- **What it is**: `theseus tasks` draws the work board as a one-line-per-task tree from one `work.watch` read
  (`render/task_tree.rs`: mark, short id, title, elapsed, what it does; the answer command under a task that asks; a
  footer of counts; columns give way title first), `--watch` prints a line per task event and marks the watcher
  `interactive`, `theseus work` is `tasks --watch`, `--graph`/`--json` keep the old outputs. The chat: a footer of
  working and asking tasks (a dock provider, repainted on pushes and the minute), task event lines in the transcript,
  one-key answers to the conversation's own and its tasks' questions through `work.answer`, `/tasks` (Ctrl-T),
  `/cancel ID`, `/stop [all]`, and `chat --plain` (or a pipe, or TERM=dumb) as `watch --interactive` with the same
  task lines.

## Why it is held
1. **The chat's questions, twice.** chat-ask (theseus-wy6v.1, accepted beside this) owns "the question in the dock":
   `ask.rs`, one key table `keys::ASK_KEYS` ("a later row (modes, cli-tasks) adds a row"), `y`/`n`+note/`v`/`t`/`a`/`e`
   through `action.confirm` with the call's diff, Esc back to typing, Tab gives the question the focus. cli-tasks built
   its own from the same base: `chat/tasks.rs` `Pending` covers the conversation's own question **and** its tasks',
   `y`/`n`/`t`/`1`..`6`/`v` through `work.answer`, Esc is "writing", Tab is the next question, and
   `footer::question_rows` draws it. On one tree both would draw the same question twice and bind `y`, `n`, `v`, `t`,
   Esc and Tab to two meanings. The owner's decision 1 puts cli-tasks' footer and one-key answers inside the chat; it
   does not say whose question model wins, and that is the merge.
2. **The merge on main** (on a3812226 with chat-ask, chat-sessions, context-meter, turn-inspector and rewind): 13
   files of conflicts, 12 of them in the CLI's `work.rs`, which main has changed since the base (tui-rows, progress,
   job-follow, retry-dismiss), and `/stop` beside stop-all's `/stop --tasks` and chat-ask's Esc stop with `keep_jobs`.
3. Its own report confirms the overlap: "The chat had y/n/t for its own question only" (the base before chat-ask), so
   it added `y`, `n`+note, `t`, `1`..`6`, `v`, Tab and Esc-as-writing for the conversation's own question and its
   tasks'; and it names no `retry`/`dismiss` verbs because main had none at its base (retry-dismiss has joined since).

## What the re-integration should do (a cloud row on main after chat-ask joins)
- Extend chat-ask's model rather than keep a second one: the tasks' questions join `ask.rs`'s dock (who asks, the
  task's title), answered through `work.answer` for a task's and `action.confirm` for the conversation's own; an `ask`'s
  choices `1`..`6` and Tab-to-the-next-question become rows of `ASK_KEYS`/the focus rule; Esc keeps chat-ask's meaning.
- Keep the rest as built: `theseus tasks`/`work` and `render/task_tree.rs`, the footer of working tasks, the task event
  lines, `/tasks` and Ctrl-T, `/cancel`, `chat --plain`; `/stop` as one command with stop-all's `--tasks` and the Esc
  stop's `keep_jobs`.
- Rebase `work.rs` on main's (tui-rows' overlay, progress's plan counts, job-follow's last line, retry-dismiss), then
  the usual gate, plants and a report.

## The original task (round 1), for reference

---
## Your task: cli-tasks, `theseus tasks` as a one-line-per-task tree and `theseus chat`, the interactive CLI with a live task footer and one-key answers (theseus-mnd9g)

Branch: `cloud/20261013-cli-tasks`. Every commit's subject carries `theseus-mnd9g`. Deadline for the report: 5 hours
after you start.

**Why this work exists.** The owner's target (2026-10-08): "you can have lots of tasks going and you'll always be kept
in the loop on status, and when your feedback is needed". Measured on 2026-10-08: the CLI you type in
(`theseus watch --interactive`) showed five bare `task.create ok` lines and none of its tasks' progress, questions,
failures or reports; `theseus tasks` prints records, not what each task is doing. Wave A gave tasks names and
`theseus status`; this row gives the terminal where you type the rest.

**Your base: main after `work-board` and `work-answer` have joined.** Read both merges first: `work.watch {roots,
interactive}` / `work.changed` / `work.notice`, `theseus work` (the board row's minimal reader, which your tree
replaces), `work.answer` and `theseus answer`. Then the CLI: `main.rs` (`Tasks` at line ~352), `cmd.rs` (`tasks`),
`interactive.rs`, `status.rs` (one read, the board forms), `resolve.rs`, `seen.rs`, `render/tasks.rs`,
`render/task_graph.rs`; the TUI's `board.rs` and `notice.rs` (the policy's `deliver()` already used there). The
design is the CLI investigator's §4.3 and §4.5 (summarised; not in the repo).

**What to build:**
1. **`theseus tasks`, a tree** (`render/task_tree.rs`, new): one line per task under its conversation, `● ◐ ✗ ✓` and
   the short id, title, elapsed, and what it is doing (`job 2:14 · leg 5: 4.5 fathoms`, `model 0:31`, `asks: write
   regatta/paint-log.md (37 B)`), the report's first line on a finished task. Each row that needs you carries its exact
   command beneath it (`theseus answer 2a329e approve · … decline --note …`); a failed one `theseus tasks retry
   5a877c · theseus dismiss 5a877c` (only if those verbs exist on main; else omit). Columns fit the terminal width,
   cutting the title first, then the activity; a footer of counts; `--graph` keeps today's task-graph view;
   `--json` byte-identical but for added fields; `theseus work` becomes an alias of `theseus tasks --watch`. One
   `work.watch {limit}` read.
2. **`theseus chat [SID]`** (new `chat.rs` and `footer.rs`; crossterm is already a dependency of the TUI): the friendly
   name for `watch --interactive`, opening a new conversation when no SID is given. On a terminal it keeps a **footer**
   above the input line, repainted only on push events: one summary line, then one line per task of *this
   conversation* that needs you or works (5 at most, then `+3 more · ctrl-t`). Between and during turns it prints one
   line per task event that matters: started (with title), reported (first line), failed (reason in words), asks
   (question and keys). `--plain`, a pipe or `TERM=dumb` keeps line mode with the same event lines.
3. **One-key answers.** While a task's question waits, the input line asks it: `y` / `n` (then an optional note) /
   `t`, or `1`..`6` for an `ask` question, sent through `work.answer` with author `theseus chat`; `v` shows the call in
   full; `tab` the next question; `esc` back to writing (the question stays in the footer). A message that starts
   with "y" never meets a one-key answer. Slash commands: `/tasks` or `ctrl-t` (the tree in place), `/cancel ID`,
   `/stop`. Refused inside a job (`client::refuse_in_a_job`).
4. **Connects as a screen.** `theseus chat` and the TUI open their watch with `interactive: true`, so the always-on
   channel (`owner-ask`) knows a person is looking; `theseus tasks --watch` does too, plain `theseus tasks` does not.
5. **Notices in words** from `notices::policy` through `deliver()` (the TUI's path), the bell only for an Interrupt.

**FAST:** `theseus tasks` one read, 6 ms p90 at most on the bench's store; chat startup unchanged; repaints at most
30 a second; elapsed repaints on the minute (each second only for a question's countdown).

**Tests:** goldens of the tree at 80, 120 and 200 columns (`COLUMNS`); one line per task; the command beneath a
needs-you row; `--json` unchanged but for added fields; a scripted daemon (`Conn::over`, as the TUI's rig): five tasks
print five titled `started` lines, a question is asked naming its task and `y` sends `work.answer` with author
`theseus chat`, `2` answers an ask question, inside a job `y` is refused; `--plain` goldens. Plant a revert per
behaviour (the footer repainting on a timer; `y` meeting a typed message) and record what caught it.

**Live check on this VM:** a scratch daemon on the stand-in, the five-task regatta scenario in tmux at 100x30:
captures of `theseus chat` (the footer, a one-key answer) and `theseus tasks` at 120 columns, in the report.

**Rules.** No store change. `crates/theseus/src/render.rs` is at 3,016 of 3,128 lines: new files. Overlaps:
`work-digest` adds one line to `status.rs` and a `seen.rs` position; `owner-ask` adds `theseus answer`'s numeric
options and the TUI's 1..6 keys (reuse them). No person's name. Every new key has a default and is in the template.

Run the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit, and end with a short final
message starting `CLOUD REPORT COMPLETE`.

## The owner's decisions (2026-10-13 23:24 MST, verbatim: "i'm happy for yes to all until we're at testing time to eval")
They hold until the evaluation pass; build so each is easy to revisit.
1. **One conversation surface.** Bare `theseus` opens the chat (row `chat`, theseus-cyha, owns it); `theseus chat` is the same thing; the task footer and one-key answers (row `cli-tasks`) live inside that chat, not in a second CLI.
2. **One plan.** Plan mode's steps are the work board's plan items (row `progress`), never a second list of steps.
3. **One owner for Discord threads.** Activity threads (row `thread-discord`, T6) decide which Discord thread a task or message lands in; task rows (`discord-guild-tasks` and others) use its helper and never create threads on their own.
4. **One pacing rule for unprompted posts** (the digest, the morning/horizon view, nudges, the always-on DM ping, proactive voice). Every unprompted post goes through one shared pacer: at most one unprompted DM post per 30 minutes, later items batched into it; the first such row to join builds the pacer (a small module beside the push), later rows use it.
5. The size check joins the gate (a build fails if the binary grows more than 1 MB; the 60 MB miss warns). 6. Programs at once 8 -> 16 (reads 32). 7. Scoped grants are built on Opus with a security review.
