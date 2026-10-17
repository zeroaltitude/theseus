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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-discord-war-room`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: discord-war-room, share a task's thread or channel with a guest who watches and comments; only the owner steers (theseus-bh3t.4)

Branch: `cloud/20261017-discord-war-room`. Every commit's subject carries `theseus-bh3t.4`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:30, verbatim):** "In tasks: what are discord forums and threads -- I didn't
know about these! Does the discord app now have the power to make new channels? If so, I'd love to start with every
task that's multi step or expected at more than 10 minutes of work to get a channel automatically. I love your other
ideas and not yet implemented ideas so please start those ASAP for quick implementation and join to main today".
This row is one of those "other ideas": the owner can bring a collaborator into a task's place to watch and comment,
and the task's model never reads a word of it.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Five sibling rows build in parallel today:
discord-task-channels (theseus-bh3t.1, **joins before you**: it registers each task channel as a row of `TaskThreads`,
`Made.thread` holding the channel's id, so `made_in(channel)` and `join_task` cover channels with no change from you),
discord-reactions (bh3t.2), voice-tasks (bh3t.3), task-offer-chip (bh3t.5), discord-emoji-catchup (theseus-ss36w). Build
against main's threads; decide channel vs thread by the channel's type read from Discord, so a task channel works at the
join.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 4 | `discord-war-room` | theseus-bh3t.4 | `/task name:<task> share:<@user>` / `unshare:<@user>` (owner only, private places): a channel gets a member overwrite, a thread adds the member; a non-owner's message in a private task's place is a **comment**: never sent to any session, relayed to the owner as one quiet line in the parent place, listed under "Comments" in `/task` Details, counted on the report's post | `dc/runtime/task_cmds.rs` (two options, the verb), new `dc/runtime/war_room.rs` (+ `dc/tests_war_room.rs`), the guest branch in `dc/runtime/threads.rs::join_task`, `dc/render/task_view.rs::detail` (Comments), `dc/courier.rs::report_post` + `dc/render.rs::report` (the count); `theseus-sim/src/fake_discord/overwrites.rs` (new) | a guest's comment never becomes a node in the task's or the parent's session and never makes a model call; the owner sees the relay line; Details lists it; the report says `1 guest comment`; share then unshare removes the overwrite; a non-owner's share is refused; share in a shared place is refused | Discord | 1 at the join (channels), none to build | cloud | M |

## What exists (read these first)
- **`/task`** is one command with one required string option `name` (autocomplete): `dc/runtime/task_cmds.rs:82-92`;
  dispatch `:220`, autocomplete `:207`, `What` `:50-57`, `Place::task_cmd` `:362-381`, `task_named` `:390`. `opt()`
  (`:125-130`) reads strings only. No USER option (type 6) exists in the crate; `/thread`'s subcommands are the pattern
  for parsing (`dc/runtime/thread.rs:44-67, 161`). The command list is asserted (count 26 and order) at
  `dc/runtime/voice.rs:1768-1801`: **add no command**.
- **Guests today** (`threads.rs:738-830`, `join_task`): the owner check `:750-756`; a guest's message goes to the task's
  session when it is not running (`turn.submit`, `:779-793`; the core queues a guest's input, `turn/join_step.rs:7`), and
  to the **parent's** session with the redirect line while it runs or after it ends (`:807-824`). Both reach a model.
- **Task threads are public** (type 11, `dc/runtime/guild_thread.rs:209`): access follows the parent channel. Who can view a
  channel: `audience.rs` (`check_private` `:24`, `view` `:40`) and `dc/viewers.rs::can_view`.
- **twilight 0.17** has `update_channel_permission` / `delete_channel_permission` and `add_thread_member` /
  `remove_thread_member`. The fake has none of these routes (`crates/theseus-sim/src/fake_discord.rs:1221-1262`, 2,406 of
  2,500 lines; the fake's threads in `fake_discord/threads.rs`).
- **Details**: `task_view::detail(v, calls, question)` (`dc/render/task_view.rs:72-97`; sections in `body`, `:121-189`),
  filled by `Place::detail_of` (`dc/runtime/live_board.rs:262-272`). **The report post**: `report_post`
  (`dc/courier.rs:1277-1297`) renders `render::report(body, said)` (`dc/render.rs:1004`, facts line `:1048-1053`;
  render.rs is 2,920 of its 3,009-line ceiling).
- **META**: `Store::put_meta/get_meta` (`crates/theseus-core/src/store.rs:724, 729`), as `/quiet` uses them
  (`task_cmds.rs:179-181, 440, 452`).

## The design

**The command.** Keep `/task`'s shape (`name` stays the first, required option) and add two optional USER options,
`share` and `unshare`: `/task name:a1b2c3 share:@guest`. (The owner wrote `/task share user:<@user>`; Discord does not
allow a subcommand beside the existing `name` option, and turning `/task` into subcommands would change every `/task`
the owner already types. Say this in the report so the owner can choose.) A `user_opt` beside `opt`. Only the owner
(`place_rule.owners`), only from a private place (refuse as `start_task.rs:169-180` does), only a task that has a thread or
channel (`task_threads.url_of`/the `Made` row); answers are ephemeral one-liners.

**Access** (`dc/runtime/war_room.rs`): read the place's channel type once (GET channel). A **text channel** (type 0, a
task channel once bh3t.1 joins): `PUT /channels/{c}/permissions/{user}` type 1 (member), allow VIEW_CHANNEL |
SEND_MESSAGES | READ_MESSAGE_HISTORY; unshare deletes it. A **thread**: if the guest can view its parent channel
(`viewers::can_view` over the parent's overwrites), add them to the thread (`PUT /channels/{t}/thread-members/{u}`);
otherwise refuse: `<@guest> can't see <#parent>, so they can't see its threads; a task with its own channel can be
shared`. Unshare removes the member (and say in the reply that a public thread stays visible to whoever sees its
channel). The shared guests are kept per task in META `discord.task_guests.<task>` (a key, no format).

**Comments, the security rule.** In `join_task`, before anything is submitted: when the author is not the owner and the
task's place is private (`Made.private`, `threads.rs:85-105`), the message is a comment, **never** a `turn.submit`, a
steer or a redirect, whether or not the guest was shared explicitly. A shared place's thread keeps today's rule (its
guests are participants of a shared conversation; say so in the report). A comment is stored in META
`discord.task_comments.<task>` (the last 50: author id, display name, `at_ms`, text clipped to 500 characters, message id)
and relayed once to the owner in the parent place as a quiet notice through `say_to` (`thread_bind.rs:415-420`, an
outbox `notice`): `-# 💬 <name> on “<title>”: <first 80 characters> · <jump link>`. The first comment by a guest gets one
quiet line in the task's place: `-# The task doesn't read guests' messages; the owner sees them.` Guest text is
untrusted: it must never reach a compile (no session node, no `WorkView` field, no board or digest line, no ledger row with
its text; the existing `DiscordMessageIn` row carries `chars`, not text, `threads.rs:741-748`).

**Details and the report.** `task_view::detail` gains `comments: &[Comment]`; a `**Comments** (N)` section with the last
5 (name, age, 80 characters) after the questions and before the Stop/Follow row (`task_view.rs:86`); `post_card`
(`:104`) does not show them. The report's Discord post appends ` · N guest comments` to its facts line when N > 0: read
the count from META in `report_post` and pass it to `render::report` (one parameter; add at most 6 lines to render.rs).

**The fake** (`crates/theseus-sim/src/fake_discord/overwrites.rs`, new, at most 3 router lines): `PUT/DELETE
/channels/{c}/permissions/{id}` (updates the channel's `overwrites`, `fake_discord.rs:336-345`) and `PUT/DELETE
/channels/{c}/thread-members/{u}` (a member set per thread, read by tests). discord-task-channels adds
`fake_discord/channels.rs` with create/PATCH/DELETE of guild channels and does not touch these routes.

**FAST:** a comment is a META write and one outbox notice; nothing on a turn.

**Tests** (`dc/tests_war_room.rs`, the rig of `dc/tests_task_threads.rs`; its guest test
`the_owners_message_joins_a_running_task_and_a_guests_is_redirected`, `:425-476`, and `held()`, `:410-418`, which reads a
session's nodes): in a private channel's task thread, a guest posts while the task runs **and** while it waits: neither
text is in `held(&task)` nor `held(&parent)`, `steers.joined(&task) == 0`, and the stand-in model got no request carrying
it (as `dc/tests_gateway.rs:964` checks requests); the parent place shows the relay line; `/task` Details lists the
comment; the report's post says `1 guest comment`. Share on a thread whose parent the guest can view → a thread member;
on a channel → an overwrite, and unshare → none (drive the channel path with a `Made` row whose `thread` is a plain text
channel on the fake: the test helper `made(task, thread)`, `threads.rs:848`). A non-owner's share → refused, nothing
changed. The owner's own message still steers (the existing test passes). Plant a revert for: the comment branch (a
guest's text submitted again), the owner check on share, the relay line, the report count.

**Live check for the maintainer:** in the test guild, from a private channel, start a task, `/task name:<it>
share:@<a test account>`, post from that account in the task's place, see the relay line and Details; check the task's
transcript never shows the text.

**Limits for this row.** No new model tool, no model-visible text. `dc/runtime.rs` has 13 lines of room: add none.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
