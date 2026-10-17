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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-discord-task-channels`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: discord-task-channels, a private text channel per multi-step or long task in the owner's guild (theseus-bh3t.1)

Branch: `cloud/20261017-discord-task-channels`. Every commit's subject carries `theseus-bh3t.1`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:30, verbatim):** "In tasks: what are discord forums and threads -- I didn't
know about these! Does the discord app now have the power to make new channels? If so, I'd love to start with every
task that's multi step or expected at more than 10 minutes of work to get a channel automatically. I love your other
ideas and not yet implemented ideas so please start those ASAP for quick implementation and join to main today".
Background: today a task started in a guild channel gets a public thread there (`dc/runtime/threads.rs:1-43`, where
`dc/` is `crates/theseus-discord/src/`); a task started in the DM gets none (a DM has no threads), only the board row
and the Follow mirror. The bot was invited with 6 permissions and has never created a channel.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Five sibling rows build in parallel today, each from
main: discord-reactions (theseus-bh3t.2), voice-tasks (bh3t.3), discord-war-room (bh3t.4), task-offer-chip (bh3t.5),
discord-emoji-catchup (theseus-ss36w). **You join first**; the others merge onto you. Keep to the shared contracts below
so their joins are mechanical.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `discord-task-channels` | theseus-bh3t.1 | a private text channel per qualifying task under a bot-made category; owner steers there; quiet pointer in the parent and the Follow mirror; on end: report, move to the done category, read-only, delete after N days; 403 and cap fall back to today | new `dc/runtime/task_channels.rs` (+ `tests_task_channels.rs`), small hooks in `dc/runtime/threads.rs`, `follow.rs`, `guilds.rs`, `start_task.rs`; `theseus-protocol/src/task_start.rs`; `theseus-core/src/task.rs` (the `expect` field); `config.rs` + `config/discord.rs` + the template; `theseus-sim/src/fake_discord/channels.rs` (new) | fake Discord rig: qualifies by steps / by `expect = long` / by promotion after N minutes (paused clock) and not for a short task; owner message in the channel is a `TurnSteered`; 403 → thread/Follow as today + one owner line; end → moved + read-only; cap → fallback | Discord | none | cloud | L |

## Shared contracts (siblings rely on these; keep the names)
1. **A task channel is a row of `TaskThreads`.** Register a task channel exactly as a task thread is registered: an
   entry in `Made` (`threads.rs:85-105`; add `#[serde(default)] pub kind: PlaceKind` with `Thread` the default and
   `Channel`, and `#[serde(default)] pub deleted_after_ms: Option<u64>` if you need it), its id in the `threads` set
   (`Inner`, `threads.rs:179-195`), its url in `urls`, its place started by `start_task_place` (`:443`), its posts bound
   by `Outbox::bind_place` (`:526`). Then `is_thread`, `made_in`, `url_of`, `task_message`, `join_task`
   (`:249, :264, :326, :711, :738`) work for channels unchanged, which is what discord-war-room (guest comments) and
   discord-reactions (a reaction on any message in the place) rely on. `Made.thread` keeps its name (it holds the
   channel's id for a channel); say so in its doc comment.
2. **Config keys** (flat, under `[discord]`, as a block right after `live_board` in `DiscordConfig`,
   `crates/theseus-core/src/config.rs:302`, and in the same spot of the template
   `crates/theseus-core/config/theseus.example.toml:752-787`): `task_channels = true`, `task_channel_after_minutes =
   10`, `task_channel_cap = 25`, `task_channel_keep_days = 14`. discord-emoji-catchup adds its keys after `status_line`;
   don't move other keys.
3. **The start modal** (`start_task.rs:52`): add your `expect` select as the modal's **last** component (after `report`,
   `:76-86`; a modal holds 5, this makes 5) and do not change `modal`'s signature. task-offer-chip adds a `brief`
   parameter to the signature; the joiner keeps both.
4. **The fake.** Put every new route in a new `crates/theseus-sim/src/fake_discord/channels.rs` (fake_discord.rs is 2,406
   lines of a 2,500 limit): `POST /guilds/{g}/channels` (types 0 and 4, `parent_id`, `permission_overwrites`, `name`),
   `PATCH /channels/{c}` for a text channel (today's PATCH arm goes to `thread_update`, `fake_discord.rs:1221-1262`;
   branch on the channel's kind), `DELETE /channels/{c}`, and a `refuse_channels` flag answering 403 like
   `refuse_pins` (`:663`). Add at most 4 lines to the router. discord-war-room owns `PUT/DELETE
   /channels/{c}/permissions/{id}` (its own module); you need neither (overwrites go in the create and the PATCH).

## The design

**Where.** The owner's guild: the one trusted guild in bindings (`Bindings::trusted()`, `dc/bindings.rs:296`; a
`[[guild]]` with `private = true`). With none, or more than one, channels stay off and nothing changes (say so in the
template comment; no new key for it). Channels are for tasks whose place is **private** (the DM, or a channel bound
private: `place_rule.class(..) == PlaceClass::Private`, as `Shared::is_private`, `dc/runtime/horizon.rs:168`). A task in
a shared channel keeps its public thread, because its guests are there (state this choice in the report).

**Qualifies** (one pure fn, `qualifies(v: &WorkView, now_ms, after_ms) -> Option<Why>`, unit-tested): `Why::Steps` when
`v.progress.steps_total >= 2` (`theseus-protocol/src/work.rs:303-313`, counted at `push/work/settle.rs:410-430`),
`Why::Long` when the task's `expect` is `long`, `Why::Promoted` when it is still working `after_ms` after
`v.started_at_ms` (`work.rs:175`). Never for an ended task. The decision runs where task threads are decided today, in
the place's sync (`Place::sync_task_threads`, `threads.rs:685`, called on every `PlaceMsg::Tasks` from
`live_board.rs:148`); extend `parent_of_tasks` (`:664`) so a private DM place also syncs (the DM makes channels only,
never threads). The sync is change-driven, so a quiet task needs a wake for promotion: when a working task is seen
that does not qualify yet, schedule **one** `tokio::time::sleep_until(started + after)` per task that then sends
`PlaceMsg::Tasks` to its place (the pattern of `retire_later`, `threads.rs:587-598`). No new periodic timer.

**`expect` = short | long.** `TaskStartParams` (`theseus-protocol/src/task_start.rs:26-75`) gains `#[serde(default)]
expect: Option<Expect>`; the model's `task.create` input (`crates/theseus-core/src/task.rs:88`, `deny_unknown_fields`;
schema `:256-339`, `additionalProperties:false`) gains the same field. **Budget:** `task_create` is resident in the DM
and channels (`core/first_request.rs:118-122`) and the DM has about 1,700 tokens of room under 15,000
(`first_request.rs:26`): the schema entry must be one enum with a description of at most 8 words
(`"long: likely over 10 minutes"`), about 20 tokens; the first-request tests must pass unchanged. Carry it to
`WorkView` the way `title` is carried (find where `WorkView.title` is filled for a task under
`crates/theseus-core/src/push/work/`) as `#[serde(default, skip_serializing_if = ...)] expect`. If carrying it means a
new field on a stored record, bump `MANIFEST_FORMAT` to 46 (`crates/theseus-store/src/store.rs:662`) with the old-layout
sample, as the preamble says; prefer a path with no stored field (a META key per task like `task::joined_key`,
`core/task.rs:968`, is a key, not a format). The modal's select: "Expected" with `short` (default) / `long (over 10 min)`.

**Making it.** A new sibling helper in `dc/runtime/task_channels.rs`, beside the thread helper
(`Shared::guild_thread_create`, `guild_thread.rs:187`, which stays the one maker of threads): `task_channel_create(guild,
name, task) -> Result<u64, Refused>`, `task_channel_close(channel)`, `task_channel_delete(channel)`, and the category
`ensure_category(guild, done: bool)`. REST through `self.http` (twilight 0.17: `create_guild_channel`,
`update_channel`, `delete_channel`; raw `Request::builder(&HttpRoute::..)` as `forum.rs:584-617` does when a builder
lacks a field). Categories `⚙️ Theseus tasks` and `⚙️ Theseus tasks · done` (type 4), made once, ids in META
(`discord.task_category`, `discord.task_category_done`; `Store::put_meta/get_meta`, `crates/theseus-core/src/store.rs:724,
729`), remade if Discord says 404. Every channel (and category) gets explicit overwrites: `@everyone` (role id = guild
id) deny VIEW_CHANNEL (1024); the owner (each `discord:<id>` of `place_rule.owners(cfg)`, `core/places.rs:179`) allow
VIEW_CHANNEL | SEND_MESSAGES | READ_MESSAGE_HISTORY | ADD_REACTIONS; the bot allow those plus MANAGE_CHANNELS. Name:
`<short6>-<title-slug>` (short = `board_v2::short`, `dc/render/board_v2.rs:60`; slug lowercase a-z0-9 and `-`, total ≤ 100
characters; Discord lowercases text-channel names anyway). Topic: the task's title and `…short`. A category holds at
most 50 channels: when the done category is full, delete its oldest first.

**What the channel is.** The task's place, exactly as a thread is today (contract 1): its tool lines, stream, cards and
report post there; the owner's messages there go through `task_message` → `join_task` and steer into the task
(theseus-esxyb, `threads.rs:711-830`). No guest is ever added (discord-war-room does sharing). The channel's first
message: for `Steps`/`Long`, `-# ↩ from <#parent>` or `-# ↩ from the DM`; for `Promoted`, a summary of what happened so
far: the task's detail card (`task_view::detail`, `dc/render/task_view.rs:72`, as `Place::detail_of` builds it,
`live_board.rs:262-272`: title, doing, plan, last calls). The parent place gets one quiet line, as the thread's
(`threads.rs:540`): `📺 <#channel> · <title>`. The DM's Follow mirror (`Mirror::text`, `follow.rs:154-172`) shows
`-# in <#channel>` under its head when the task has one. When a task that already has a thread is promoted, the thread
gets `-# moved to <#channel>` and is archived through `guild_thread_set` (`guild_thread.rs:220`); the session's posts are
re-bound to the channel (check `Outbox::bind_place`/`place_bound`, `crates/theseus-core/src/outbox.rs:530, 547`, for
rebinding; if a session cannot be re-bound, say so and bind the channel only for tasks with no thread yet).

**On end** (the `Next::Finish` arm, `threads.rs:300-307`): the report posts in the channel and, as today, where the task
reports; then one PATCH moves the channel to the done category and sets the owner's overwrite to deny SEND_MESSAGES
(read-only), and its name gets `✅`/`❌` like the thread's mark (`Mark`, `threads.rs:127-150`). Delete after
`task_channel_keep_days`: a sweep at start (after `revive_task_threads`, `threads.rs:423`) and once a day on one
`sleep_until`, deleting channels whose `ended_ms` is older; the `Made` row goes with it.

**Fallbacks.** A 403 from any channel call (twilight `ErrorType::Response { status, .. }`; the courier's classifier is at
`dc/courier.rs:119-140`): the task goes on as today (a thread in a guild channel, the Follow mirror in the DM), channels
stop being tried until the next start, and the owner gets ONE DM line: `-# I can't make task channels in <guild>: grant
the bot Manage Channels and Manage Roles (re-invite: <url>)`, once per daemon run (a META flag so a restart says it once
more, not on every task). At `task_channel_cap` open channels, a new qualifier falls back the same way, silently.
**Invite permissions** (`dc/runtime/guilds.rs:193`, today `117824`): `326686067792` = today's six plus MANAGE_CHANNELS
(1<<4), MANAGE_ROLES (1<<28), MANAGE_THREADS (1<<34), CREATE_PUBLIC_THREADS (1<<35), SEND_MESSAGES_IN_THREADS (1<<38);
put the number in a named const with that sum in its comment.

**Restart.** The rows already persist in `discord.task_threads` (`threads.rs:81`, `save` `:371`); a channel row revives as
a thread row does (`revive_task_threads`). The categories' ids come from META.

**FAST:** nothing on a plain turn; a sync is a read of the work model plus at most one create per qualifying task; one
`sleep_until` per not-yet-qualifying working task, dropped when it ends.

**Tests** (`dc/tests_task_channels.rs`, the rig of `dc/tests_task_threads.rs`: `guild()` `:143`, `bindings()` `:151`; a
private guild set on the fake): (a) a task with 2 plan steps gets a channel under the category, private overwrites
asserted; (b) a task started with `expect = long` (task.start) gets one at once; (c) promotion: `qualifies` unit tests on
explicit `now_ms`, plus the wake under `#[tokio::test(start_paused = true)]` as `follow.rs:403` does; (d) a short
one-step task gets none (a thread in a guild channel as today); (e) the owner's message in the channel is a steer
(`steers.joined(&task) == 1`, as `tests_task_threads.rs:474-476`); (f) `refuse_channels` → a thread, and exactly one
owner line across two tasks; (g) end → the channel's parent is the done category and the owner's overwrite denies
SEND_MESSAGES; (h) at the cap → fallback; (i) the sweep deletes an old ended channel. Plant a revert for (a), (d), (e), (f)
and (g).

**Live check for the maintainer** (write it in the report): bind the test guild private, start a task with `expect =
long` from the DM, see the channel under `⚙️ Theseus tasks`, type in it, see the steer, stop the task, see it move.

**Limits for this row.** The one exception to "no new model tool field": `task.create` gains `expect` (about 20
tokens, above); nothing else model-visible. `dc/runtime.rs` has 13 lines of room under its ceiling (3,487 of 3,500):
add at most 2 lines there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
