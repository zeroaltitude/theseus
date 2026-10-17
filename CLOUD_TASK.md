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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-discord-reactions`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: discord-reactions, the owner's reactions on a task's messages as one-tap controls (theseus-bh3t.2)

Branch: `cloud/20261017-discord-reactions`. Every commit's subject carries `theseus-bh3t.2`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:30, verbatim):** "In tasks: what are discord forums and threads -- I didn't
know about these! Does the discord app now have the power to make new channels? If so, I'd love to start with every
task that's multi step or expected at more than 10 minutes of work to get a channel automatically. I love your other
ideas and not yet implemented ideas so please start those ASAP for quick implementation and join to main today".
This row is one of those "other ideas": a reaction on a task's message acts on the task, so the controls are one tap
from anywhere the task shows.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Five sibling rows build in parallel today:
discord-task-channels (theseus-bh3t.1, joins first: a task channel is registered as a row of `TaskThreads`, so
`made_in(channel)` finds a channel's task as it finds a thread's), voice-tasks (bh3t.3), discord-war-room (bh3t.4),
task-offer-chip (bh3t.5), discord-emoji-catchup (theseus-ss36w). Build against main's threads; channels then work at the
join with no change from you.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 2 | `discord-reactions` | theseus-bh3t.2 | ⏸ pause, ▶️ resume, 🛑 stop (confirm), 🔁 retry, 👀 follow, 📌 make it a project, by the owner's reaction in a private place; the bot pre-adds the valid ones on a task's first message and its Follow mirror; a task pause/resume in the core | new `dc/runtime/task_react.rs` (+ tests), one call in `dc/runtime/route.rs:167-172`; `follow.rs` (pre-add on the mirror); `threads.rs` (keep the first message's id); new `crates/theseus-core/src/rpc/task_pause.rs`, `turn/join_step.rs` (park), `theseus-protocol` (two method names on an existing line, params in a module); `theseus-sim/src/fake_discord/reactions.rs` (new) | each emoji maps to its RPC as the owner (task state read back from the kernel); a guest's reaction does nothing; an unknown emoji does nothing; a reaction in a shared place does nothing; 🛑 posts the confirm and acts only on Yes; pause parks a running task at its next loop boundary without reporting, resume wakes it | Discord + core | none | cloud | L |

## What exists (read these first)
- **Intents are on**: `DIRECT_MESSAGE_REACTIONS | GUILD_MESSAGE_REACTIONS` (`dc/runtime.rs:454-462`), and
  `Event::ReactionAdd` is dispatched to `route::reacted` (`dc/runtime.rs:548`) → `Shared::on_reaction`
  (`dc/runtime/route.rs:167`). It skips the bot's own (`:168`), then returns unless the message is a turn reply
  (`self.replies.turn(..)`, `:171`), so **your handler goes before line 171**: `if self.task_reaction(&r).await {
  return; }`, one line.
- **The verbs today**: Stop asks once more (`tb:stop:` → `task_view::confirm_stop`, `dc/render/task_view.rs:199-218`;
  `live_board.rs:222-231`; `tb:yes:` → `stop_task` `:275-286` → `cancel_task` → `task.cancel`, `dc/runtime.rs:2269-2297`).
  Retry: `task.retry` with `TaskRetryParams{execution_id, note, discord}` (`dc/runtime/task_cmds.rs:527-547`; core
  `crates/theseus-core/src/rpc/retry.rs:68`, `retry_failed`). Follow: `Shared::follow` (`dc/runtime/follow.rs:209-237`;
  refuses shared places, at most 3). Projects: `project.create` (`theseus-protocol/src/projects.rs:114-137`, no task
  input) and `project.attach{session, project}` (`:181-185`); core `rpc/projects.rs:91, 237`; CLI `theseus project`
  (`crates/theseus/src/main.rs:765-773`). Every press handler takes an `Interaction` (`TaskPress`, `live_board.rs:37-42`);
  a reaction has none, so call the RPCs from your module, not through those handlers.
- **No task pause exists** (only threads, wakes and routines pause: `theseus-protocol/src/lib.rs:259, 280, 282`).
  `WorkView.paused` (`theseus-protocol/src/work.rs:254-256`, "Held by the owner") is the routines' and reused here.

## The design

**Which messages carry a task** (`task_of_message(channel, message) -> Option<String>`, in your module):
1. any message in a task's thread or channel: `task_threads.made_in(channel)` (`threads.rs:264`);
2. the DM's Follow mirror: `Follows::task_of` (`follow.rs:110`, recorded `:307`);
3. a task's report line: the courier keys it `report:<task>` (`dc/courier.rs:1288`) and notes it with
   `replies.noted(m, &w.key)` (`:1553`); `turn_of_key` drops non-`turn_` keys (`route.rs:87-93`): add the inverse for
   `report:` keys (bounded as `replies` already is).
The board is one message for every task (a reaction can't name its row), and `/task` Details is ephemeral (no
reactions on it): both stay buttons. Say so in the report.

**Who.** The owner (`place_rule.owners(cfg).contains("discord:<user_id>")`, as `threads.rs:750-756`) in a private place
(`Shared::is_private(target)`, `dc/runtime/horizon.rs:168`; the place string as `route::place_of`, `route.rs:152`).
Anyone else, or a shared place: nothing, no reply, no ledger noise beyond a debug line. Every RPC carries the
reaction's `DiscordOrigin` (user, channel, guild) so the core judges the owner as it does a press.

**The map** (one `match` on the Unicode emoji; strip U+FE0F before matching; unknown → return false):
- ⏸ `task.pause {task}` · ▶️ `task.resume {task}` (below);
- 🛑 post the existing confirm (`task_view::confirm_stop(&v)`) as an ordinary message in that place (a reaction has no
  interaction to answer ephemerally); its Yes/No buttons are the existing `tb:yes:`/`tb:no:` flow, unchanged;
- 🔁 `task.retry` for a failed or cancelled task; for a done task, if the core refuses, start a new task with the same
  brief (`task.start`; read how `Kernel::open_retry` finds the brief, `rpc/retry.rs:1-20`, and reuse it); a running task:
  nothing;
- 👀 `Shared::follow` into the owner's DM (its target from the bindings' `[[dm]]`);
- 📌 `project.create {title: v.title, objective: <the brief>, author, discord}` then `project.attach {session:
  v.session_id, project}`; a task already in a project: one quiet line saying which.
After an action the bot adds ✅ to the reacted message (as `route.rs:219-223` does) and removes the owner's reaction
only if that is one call; a refusal posts one quiet line with the core's words.

**Pre-added controls.** On a task's first message in its thread/channel (`say_pointer(thread, back, ..)`,
`threads.rs:537`; keep the created message's id: the outbox post's key → message id, as the courier records keys,
`courier.rs:272`) and on the Follow mirror (`follow.rs:307`): the bot adds ⏸ 🛑 👀 📌 while the task works, ▶️ while
paused. A reaction costs one request and Discord limits them (about 1 per 0.25 s per channel): add them in order with
the twilight client's own rate limiting, never on a turn's path, and never re-add on every change (only at creation and
when pause flips).

**Task pause in the core** (new `crates/theseus-core/src/rpc/task_pause.rs`): `task.pause {task, author, discord}` and
`task.resume {…}`, owner only (`Core::owner_input`, `rpc/owner_input.rs:14`, as `work.answer` judges). Pause sets a flag
per task session (in memory, and a META key `task.paused.<session>` so a restart keeps it: a key, no store format).
At the task's next loop boundary (where `steer_in` runs, `crates/theseus-core/src/turn/steer_step.rs:81`) a paused task's
turn ends as `TurnEnd::Wait { wake: Wake::Input }`, and `task_end` (`turn/join_step.rs:40-66`) keeps that wait instead of
`Complete{"reported"}` while the flag is set (beside `owner_waits`): it parks, never reports, and a model call is never
cut. Resume clears the flag and wakes it with one owner input, `"(resumed by the owner)"`, through the same path as the
owner's steer (`WokenByInput`). `WorkView.paused` is set for a paused task (the work model's task view, under
`crates/theseus-core/src/push/work/`); the board and `/task` already show `paused` where routines use it (check, and add
the word `paused` to the task's state line in `task_view::state_words`, `dc/render/task_view.rs:55-68`, if it is not).
**Protocol:** `crates/theseus-protocol/src/lib.rs` is at 2,731 of its 2,733-line ceiling: add `TASK_PAUSE =
"task.pause", TASK_RESUME = "task.resume"` onto the existing `TASK_CANCEL` line (`:270`), and the params type in a new
module file (`theseus-protocol/src/task_pause.rs`), its `mod` on an existing line. Regenerate the TypeScript if you add a
protocol type (the preamble says how; ts.rs's type list: add to an existing line).

**The fake.** It has no reactions (`crates/theseus-sim/src/fake_discord.rs:1221-1262`; 2,406 of 2,500 lines): a new
`fake_discord/reactions.rs` with `PUT/DELETE /channels/{c}/messages/{m}/reactions/{emoji}/@me` (kept per message, read by
tests) and a `react(user, channel, message, emoji)` helper that dispatches `MESSAGE_REACTION_ADD` through
`FakeGateway::dispatch` (`crates/theseus-sim/src/fake_gateway.rs:99`). At most 3 lines in the router.

**FAST:** a reaction costs a map lookup and one RPC; nothing on a turn; the pause check at the loop boundary is one
atomic load when no task is paused (keep a `paused_any` counter like `steers.any()`, `join_step.rs:25-28`).

**Tests** (`dc/tests_task_react.rs` on the rig, `dc/tests_gateway.rs:50`; `dc/tests_board.rs:652` is the Stop test to
copy): as the owner in a task thread of a private channel: ⏸ → the task parks (state read through `r.core.kernel`, no
report written), ▶️ → it runs and reports; 🛑 → a confirm message, the task still running, then `press` Yes → cancelled;
🔁 on a failed task → a new linked task; 👀 → a mirror in the DM; 📌 → a project with the task attached. A guest's ⏸ →
still running; 🙂 → nothing; ⏸ in a shared channel → nothing. Core unit tests for pause: the turn parks at the boundary,
`task_end` keeps the wait, resume wakes, the META key survives a reload. Plant a revert for the owner check, the
private check, the park and the confirm.

**Live check for the maintainer:** in the test guild, start a two-step task from a private channel, react ⏸ on its
thread's first message, see `paused` on the board, react ▶️, see it finish.

**Limits for this row.** No new model tool, no model-visible text: pause and resume are owner RPCs. `dc/runtime.rs` has
13 lines of room (3,487 of 3,500): add none there (the dispatch is in route.rs).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
