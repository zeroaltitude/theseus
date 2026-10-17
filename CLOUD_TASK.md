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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261016-herdr-task-return`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: herdr-task-return, a row of universal task visibility: inspect, join and return (theseus-m638c)

Branch: `cloud/20261016-herdr-task-return`. Every commit's subject carries `theseus-m638c`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-16):** 22:54 "Probably it would be a good idea to be able to both inspect, and join,
tasks, and easily return back to your mainline chat"; 22:54 "Part of the universal task visibility work we wanted to
do"; 23:07 "can we implement and test tonight?"; 23:37 "you can use cloud threads to parallelize work". Background:
tasks are their own child sessions (crates/theseus-core/src/task.rs header: brief in, report out, depth one) and are
the harness's "no agent swarm" design.

**Prerequisite:** `main` (3e1a7ed0 or later; store format 45). Nine rows build in parallel tonight; row
task-join-core is the engine change the others rely on at runtime. Client rows work through RPCs that already exist
(`turn.submit` to the task's session, `session.history`, `session.watch`, `turn.inspect`) and test against fakes, so
they do not need task-join-core merged to build and test.

## Your row (from the plan's table)
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 6 | `herdr-task-return` | **create**, child of 0f933 | prefix+m to the mainline tab (the follower keeps one `main` tab), prefix+i inspect popup, the pane header | `cli/herdr_up.rs`, `cli/herdr_up/state.rs`, `cli/herdr_keys.rs`, `cli/herdr_setup.rs`, `contrib/herdr/theseus/herdr-plugin.toml`, `cli/interactive.rs` (header) | scripted herdr (`crates/theseus/tests/herdr*.rs`): prefix+m focuses the main tab, creating it once (revert: create every time → two tabs); prefix+i's popup argv | herdr | 2 (`chat --task --inspect`) | cloud | S |

**Shared contract (rows build in parallel; keep it identical so joins dedupe cleanly).** In
`crates/theseus-protocol/src/work.rs`, if absent on your base, add exactly:
```rust
/// Whether the owner can join a task (its session takes input): running, queued, waiting or blocked.
pub fn joinable(v: &WorkView) -> bool { /* match v.state on the Running | Queued | Waiting | Blocked variants */ }
/// The one line every surface prints when a message for an ended task goes to the mainline instead.
pub fn task_redirect_line(title: &str, short: &str, why: &str) -> String {
    format!("[Written to the task “{title}” (…{short}), and {why}] ")
}
```
(adapt the match to WorkView's real state enum; keep the names, signatures and the line's words, which are Discord's
today: `crates/theseus-discord/src/runtime/threads.rs` ~765). Row task-join-core owns these; other rows may add them
identically and must not change them.

## The design (the plan, sections 3.1 to 3.3, verbatim; implement your row's part of it)
## 3. Design: inspect / join / return (theseus-0f933)

### 3.1 Words

- **Mainline:** the conversation that started the task (`WorkView.parent` → its `session_id`; tasks are depth one, so
  the parent is always a conversation). A client that opened a task cold returns there.
- **Inspect:** read-only. The task's transcript so far (`session.history`) and its live stream (`session.watch`),
  plus a link into the turn inspector for any turn (`turn.inspect`).
- **Join:** inspect, plus the input line targets the task's session: `turn.submit {session_id: <task's>}`.
- **Return:** the input line targets the mainline again, which is redrawn with one line saying what happened while you
  were away.
- Inspect and join are a client's view, never stored (as `current.rs` keeps a terminal's thread, a convenience, not
  a record). No new record kind, no store format, no frame on a plain turn.

### 3.2 The protocol: two kernel rules, no new RPC

Everything a client needs is already on the wire: `WorkView.session_id`, `.parent`, `.state`, `.title`
(`proto/work.rs:140-153`), `session.history`, `session.watch`, `turn.inspect`, `turn.submit`. The kernel changes:

1. **The owner's input steers into a running task.** In `steered()` (`core/turn/steer_step.rs:35-41`), accept
   `Class::Task` as well as `Class::Person` when the request carries typed input **from the owner**: the same judgement
   `work.answer` uses (`owner_in_private`, `core/places.rs:453-470`; the listener's surface from
   `core/approval.rs:12-38`; Discord's author checked against the owner). Admission's class stays `Task`
   (`admission.rs:72`), so fairness and concurrency limits don't change. The message becomes the task's context at
   its next loop boundary through the existing `steer_in` (`steer_step.rs:76-110`): an `Origin::Operator` node with
   its author and surface, `reply_to` kept, one `TurnSteered` frame (`steer_step.rs:105-110`). A model call is never
   cut. Anyone else's input (a guest in a shared guild thread, a job) queues as today.
2. **A task with the owner's input waiting does not report.** In the end rule (`core/turn.rs:3250-3262`), beside
   `parks_on_wake`: if an owner message was offered and not taken (it arrived during the turn's last call), or one
   is queued on the task's hold, the turn ends as `Wait { Input }`. The task parks, the queued message wakes it
   (`WokenByInput`, `turn.rs:1117-1121`), and that turn's last message becomes the report. The check is an O(1)
   per-session counter on `Steers` (`core/toolrun/waits/steer.rs:146-181`), read only by a task's turn at its end:
   one atomic load, never on a conversation's turn.
3. **A task that has ended stays ended.** Kernel unchanged: `execution_complete` (`turn.rs:1103-1110`,
   `kernel/held.rs:50-55`). Each client turns that refusal into "the task reported; this goes to main", and sends the
   message to the mainline with **one shared redirect line**, moved from Discord (`dc/runtime/threads.rs:765-769`)
   into `proto/` as `task_redirect_line(title, short, why)`. Every surface uses the same words. A follow-up as a new
   linked task stays retry's verb (theseus-7zgsm), offered as `f`, never automatic.
4. **The report says you were there.** When the owner's messages were taken during a task, the report's node in the
   parent (`read_reports`, `core/turn.rs:2094-2140`) gets one trailing line, `(you joined: 2 messages)`, about 10
   tokens, only for joined tasks. The parent's model learns that its brief was changed by its owner, not by the task.
   The count is carried on the end frame's outbox post (no read at report time).
5. **Concurrency with the parent's report.** A task reports once, in the frame that ends it (`task.rs:23-27`).
   Joining delays that frame and never duplicates it. With `wake_parent: true` the parent wakes once, at the real
   end. If the owner's message and the end frame race (the decision is taken just before the offer lands), the offer
   fails with `execution_complete`, and the client uses rule 3: never lost, never silent. While joined, a cancel
   (`/stop` in the task view) reports "cancelled" as today.
6. **Notices while joined:** the client sets the shared viewer's `focused` to the task's session, so the policy
   (`proto/notices.rs:296`) doesn't ring for what you are looking at; the mainline's questions still reach the dock.
7. **Budgets and FAST.** No new model tool, no new always-loaded text: first-request budgets unchanged
   (`core/first_request.rs:24` cli/task 8,000; `:26` DM 15,000; the DM's ~1,700 tokens of room untouched). The parent
   pays ~10 tokens once per joined task's report. A plain turn stays 5 frames; a steer costs its one frame only when
   it happens.

### 3.3 Per surface

| Surface | Inspect | Join | Return | Task ends while joined |
|---|---|---|---|---|
| **Chat** (`tui/chat/`) | `ctrl-t` opens the task picker, the tree with a cursor, instead of printing it (`keys.rs:120`): ↑↓ move, **Enter** inspect, **j** join, **c** cancel (confirm), **a** answer, **Esc** close. `/task ID` inspects. The view replays the task's turns and streams it, reusing `resume_info` (`tui/chat/reopen.rs:141-159`). Header: `── ⚙ task a1b2c3 · Sound the north channel · running 4:12 · $0.31 ── j join · i inspector · Esc back to main`. **i** prints the turn inspector's link for the shown turn (as `/inspect` does today, `commands.rs:115`); `/inspect [ID]` takes a task id. | **j**, or `/join ID`. The prompt reads `a1b2c3 ›`; a sent line shows `↪ the task reads this at its next step`. | **Esc** on an empty line, or `/back`. In a task view Esc never stops anything (stopping a task is `/stop` there, with a confirm); Esc Esc's rewind is the mainline's only. The mainline is redrawn (`resume_info` on the saved session) with one line: `── back in main · a1b2c3 still running · main: 1 reply while you were away ──`. | `🏁 a1b2c3 reported: <first line>`; the prompt goes back to `main ›`; a line typed meanwhile is sent to main with the shared redirect line; **f** offers a linked follow-up task. |
| **CLI** | `theseus chat --task ID --inspect`; `theseus open turn --task ID` (the short-id resolver beside `--session`, `cli/open.rs:43, 67`); `theseus tasks` rows print the `chat --task` hint for running tasks | `theseus chat --task ID` opens the chat joined; `--plain` maps to `watch <sid> --interactive` (`cli/chat_cmd.rs:97-99`) | `/back`/Esc lands in the task's parent conversation | as the chat |
| **TUI** (`tui/app.rs`) | Enter on a task row (today, `app.rs:1200-1213`); a breadcrumb header `task a1b2c3 of <parent title> · p parent · I inspector`; **I** opens the inspector link for the selected turn | **i** (today), with the steer's words; refusals in words | **p**, or Backspace, focuses the parent conversation (`focus(parent_session_id)`) | the end line; `i` on an ended task offers "send to main" |
| **Cockpit** (`cockpit/src/views/SessionDeck.tsx`) | a TaskHeader on `/session/<task>`: title, state, doing, cost, Stop, the forecast; turn groups already link to `TurnInspect` (`Transcript.tsx:112`) | the Composer (today), placeholder "Message this task; it reads it at its next step" | `← back to <parent title>` in the header (the "from {parent}" link, `Actions.tsx:115`, lifted onto the page) | the Composer becomes "Send to main about this task" (shared redirect line) |
| **Discord** | `/task` Details (today), Follow mirror (today) | **Guild:** a message in the task's thread from the owner goes to the task even while it runs (steered), replacing today's redirect for the owner (`threads.rs:717-772`); guests keep the redirect. **DM** (no threads): a reply to the task's Follow mirror joins (record the mirror's id in the `replies` map, `thread_ask.rs:123`, and route it through `task_message`'s rule); the mirror's footer says `-# reply here to talk to the task`. Thread choice stays with thread-discord's helper (`Shared::guild_thread_create`, `runtime/guild_thread.rs:187`; `sync_task_threads`, `threads.rs:679`), per the owner's 10-13 decision 3. This row creates no thread. | Guild: the thread's first message gets a jump link back to the parent channel; Discord's own back navigation does the rest. DM: write normally (not a reply) and it's the mainline. | the redirect, with the shared line (today's behaviour, now one wording) |
| **herdr** | the pane (today: `watch <sid> --interactive`); **prefix+i** opens `theseus chat --task ID --inspect` in a popup | type in the pane (today), now steered | **prefix+m**: focus the mainline tab. The follower keeps one `main` tab running `theseus chat` on the home conversation, made at the first prefix+m if absent. The pane header names the parent and the key. | the pane prints the end line and the redirect choice |
| **Voice / status line** | ✗ by design (counts only, `voice_status.rs:1-23`) | ✗ out of scope | n/a | n/a |
| **Digest** | n/a | n/a | the chat's return line is a one-session digest; nothing new is stored | the report shows in `work.digest` as any report |

**The one rule every client shares:** `proto/work.rs` gets `joinable(&WorkView) -> bool` (Running, Queued, Waiting
or Blocked) and the redirect line, so no surface invents its own.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
