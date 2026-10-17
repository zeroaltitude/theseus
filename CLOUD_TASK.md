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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-discord-default-forum`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: discord-default-forum, one private forum in the owner's guild that is the home of every task: a post per task, the complete index of tasks (theseus-bh3t.6)

Branch: `cloud/20261017-discord-default-forum`. Every commit's subject carries `theseus-bh3t.6`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 10:00, verbatim):** "I made Theseus an owner of catorce -- let's have a default
forum channel that is intrinsically used for all threads and a config that names that channel and can also disable this
feature". **And the owner's clarification (10:00, verbatim):** "Threads===tasks there, sorry for the terminology drift".
So "all threads" means **all tasks**: every task gets a post, DM-started ones included, and the forum is the complete
index of tasks. **Activity threads are not part of this row**: they stay exactly as they are today (`[threads]
discord_channel`); do not route them into the forum.

catorce is the owner's guild, bound `private = true` in the bindings file (so `Bindings::trusted()`, `dc/bindings.rs:296`,
returns it alone); the bot now holds owner-level permissions there, so it can create channels and set permission
overwrites. Background (`dc/` is `crates/theseus-discord/src/`): a task in a guild channel gets a public thread there
(`dc/runtime/threads.rs:490-553`); a task in the DM gets none, only the board row and the Follow mirror; and the task
forum (`dc/runtime/forum.rs`: a post per task with its live card and four state tags) runs only for a `[[channel]] kind =
"forum"` binding (`Bindings.forums`, `dc/bindings.rs:34-36`, `forum.rs:169-182`). Nothing makes a channel.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Six sibling rows build in parallel today, each from main,
and the joiner merges in this order: discord-task-channels (theseus-bh3t.1), discord-war-room (bh3t.4),
discord-reactions (bh3t.2), task-offer-chip (bh3t.5), voice-tasks (bh3t.3), discord-emoji-catchup (theseus-ss36w), then
**this row last**. Read their prompts before you start (`~/reports/theseus-cloud/prompts/task-discord-task-channels.md`
and `task-discord-war-room.md` are the ones that share your files; the joiner's plan is
`~/reports/theseus-discord-1017/plan.md`). Build against main; keep every edit additive so your join is mechanical.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 7 | `discord-default-forum` | theseus-bh3t.6 | `[discord] forum = true`, `forum_channel = "theseus"`: with one trusted guild, find the forum (recorded id, else the configured id or name) or create it private (type 15, @everyone denied VIEW_CHANNEL, the four state tags); **every task the surface sees gets a post**: a task with no other home (the DM's, a private channel's) has its post as its place, so the owner steers there and the DM gains one quiet link line; any other task (a shared channel's thread, a task started in an activity thread, and after the join a task channel) gets an **index card** post linking where it works, with live state tags; activity threads untouched; `forum = false` is today exactly; a 403 falls back with one owner line | new `dc/runtime/default_forum.rs` (+ `dc/tests_default_forum.rs`); hooks in `dc/runtime/forum.rs`, `threads.rs`, `thread_bind.rs:674`, one line in `dc/runtime.rs`; `crates/theseus-core/src/config.rs` + `config/discord.rs` + the template; `crates/theseus-sim/src/fake_discord/forums.rs` (new) | fake Discord rig: created once when missing, reused when present, reused by id after a rename; a DM task gets a post + the DM line; every task across four kinds of place gets exactly one post; an index card links the task's thread and follows its state; an archived post still takes its final tag; the owner's message in a task's post steers, in an index card gets a pointer line; activity threads make no post; `forum = false` makes no forum request; a 403 falls back with exactly one owner line | Discord | none to build; last at the join | cloud | L |

## Shared files and the contracts you keep (siblings change the same files today)
1. **`dc/runtime/threads.rs`** (task-channels: `Made.kind`, the DM syncs, promotion, end → move; war-room: the guest
   branch in `join_task`). A task whose post is its place has that post as **a row of `TaskThreads`**, exactly as a
   thread is: a `Made` (`threads.rs:85-105`; `thread` holds the post's id, `parent` the place's channel, `parent_session`
   its session), its id in `threads`, its url in `urls`, its place started by `start_task_place` (`:443`), its posts bound
   by `Outbox::bind_place` (`:526`). Then `is_thread`, `made_in`, `url_of`, `task_message` → `join_task` (`:249, :264,
   :326, :711, :738`; routed from `thread_bind.rs:674`) and `Next::Revive/Finish` work for posts unchanged, and so do
   war-room's guest rule and reactions' `made_in`. An index card is **not** a `TaskThreads` row: it is only the forum
   worker's `Post`. Do **not** restructure `make_task_thread` or `parent_of_tasks`: add one branch in each (below) and
   make private helpers `pub(super)` where you call them from your module (a one-word edit).
2. **`parent_of_tasks`** (`threads.rs:664-681`). task-channels widens `wants` so a private DM place also syncs. You widen
   it the same way for a place the default forum homes (`|| self.forum_homes()`). The joiner keeps both terms.
3. **Where a new task works** (the `Next::Create` arm, `threads.rs:696-698`, and `make_task_thread`'s create call,
   `:502-505`): you add `Post` beside today's `Thread`. Put the choice in one pure fn, `home_of(forum_homed: bool,
   is_channel_place: bool) -> Home { Post, Thread, None }`, unit-tested. At the join the order is **channel, then post,
   then thread**, and the joiner adds `Channel` first in this one fn. **Whatever `home_of` says, the task gets a post:**
   `Post` makes the post its place; every other answer (`Thread`, `None`, and after the join `Channel`) gives it an index
   card from the forum worker, linking the task's place. Key the worker's choice on `home_of(..) != Home::Post` (not on
   `== Thread`), so the joiner's `Channel` arm yields an index card with no further edit.
4. **The seam for task channels:** `TaskPosts::moved(task, url)` (forum.rs): a task whose post was its place now works
   elsewhere (task-channels' `Promoted`, at 10 minutes). Its post **stays in the list and stays live**: it becomes the
   task's index card, its card's link is `url` from now on, and its tag keeps following the state to the end, as any
   index card's does. Nothing on this branch calls it but its test; at the join, task-channels' promotion calls it (one
   line) for a row the worker owns (`TaskPosts::owns(task) -> bool`, yours) **instead of archiving it** (its `-# moved to
   <#channel>` line is still posted in the post), because an archived post would not take its tags.
5. **`guild_thread.rs`**: you don't edit it. Activity threads keep today's path, and `guild_thread_create` /
   `guild_thread_set` (`:187, :220`) keep their signatures (task-channels calls `guild_thread_set`).
6. **Config** (`DiscordConfig`, `crates/theseus-core/src/config.rs:273-323`): your two keys go as a block right before
   `thread` (`:316`), and in the template (`crates/theseus-core/config/theseus.example.toml`) right before the
   `# A channel the bindings file names (private)` comment (`:778`). task-channels' keys go after `live_board`, emoji-catchup's
   after `status_line`; don't move other keys.
7. **The fake** (`crates/theseus-sim/src/fake_discord.rs`, 2,406 lines of the 2,500 default limit): put everything in a
   new `fake_discord/forums.rs` (`GET /guilds/{g}/channels`; `POST /guilds/{g}/channels` for type 15 with `name`,
   `permission_overwrites`, `available_tags`; a forum's name and `PATCH` of it for a rename; a `refuse_forum` flag answering
   403 like `refuse_pins`, `:663`; the archived-post rule below), its state one field of `FakeState`, and at most 3 router
   lines (beside `:1221-1238`). task-channels adds `POST /guilds/{g}/channels` for types 0 and 4 in
   `fake_discord/channels.rs`: at the join there is one router arm, theirs, and it hands type 15 to your `forums::create`
   (one line). Write your handler so that is all it takes. war-room owns the permissions and thread-member routes
   (`fake_discord/overwrites.rs`); you need neither.
8. **Line ceilings** (`scripts/long-files.txt`): `dc/runtime.rs` is 3,487 of 3,500, and task-channels (≤ 2) and
   emoji-catchup (1) also add there: add **at most 1** line (the Ready call below). `theseus-protocol/src/lib.rs` is 2,731
   of 2,733: add nothing to it (this row needs no wire type). `dc/render.rs` (2,920 of 3,009): add nothing.

## The design

**Config** (`[discord]`): `forum = true` and `forum_channel = "theseus"`. `forum_channel` is a Discord id (15 to 21 digits,
as a string) or a name; validated in `config/discord.rs` (a name is 1 to 100 characters, no `#`; one call from
`Config::validate`, `config.rs:1465`). **Why "theseus":** it says whose every post is and is the name the owner uses; a
generic `tasks` is likelier to collide with a forum the owner already has, and task-channels' category is `⚙️ Theseus
tasks`, so the two read as one family. Discord shows channel names lowercase anyway. The feature acts only when the
bindings trust **exactly one** guild (`trusted().len() == 1`, the rule task-channels uses for its channels); with none or
two it does nothing and says so in the template comment (no key for it). `forum = false` restores today's behaviour
exactly: every new path checks it first, and nothing is read, created, posted or written to META.

**Finding or making the forum** (`dc/runtime/default_forum.rs`, `Shared::ensure_forum`). The record is one META key,
`discord.default_forum` = `{ guild, channel, wanted }` (`Store::put_meta/get_meta`, `crates/theseus-core/src/store.rs:724,
729`; a key, no store format), `wanted` being the config value it was resolved from. In order: (1) a record whose `wanted`
equals today's `forum_channel`: `GET /channels/{id}`; a type-15 channel in the guild is the forum, **whatever it is called
now** (the owner may rename it; no second forum is made). A 404 goes on to (2). (2) An id in the config: `GET` it; a type-15
channel in the trusted guild is recorded; anything else turns the feature off for this run with one owner line (never make
a forum for an id that does not resolve). (3) A name: `GET /guilds/{g}/channels`, the first type-15 channel with that name
(case-insensitive; a text channel of the same name is not it) is recorded. (4) None: **create it**, `POST
/guilds/{g}/channels` with `type: 15`, the name, `available_tags` (the four state tags, `Tag::ALL`, `forum.rs:67-96`;
`read_tags`, `:583-638`, already finds them by name, so tags the owner deletes come back),
`default_auto_archive_duration: 1440`, and `permission_overwrites`: `@everyone` (role id = guild id) deny VIEW_CHANNEL
(1024); each owner (`place_rule.owners(cfg)`, `core/places.rs:179`, the `discord:<id>` handles) a member overwrite allowing
`274877975616` (VIEW_CHANNEL | SEND_MESSAGES | ADD_REACTIONS | READ_MESSAGE_HISTORY | SEND_MESSAGES_IN_THREADS); the bot
allowing `292057844816` (those plus MANAGE_CHANNELS and MANAGE_THREADS). Put both numbers in named consts with their sums
in the comments. Twilight 0.17's `create_guild_channel` may lack a field: use `Request::builder(&HttpRoute::..)` with raw
JSON as `forum.rs:584-617` does. Then record it. `Tag` is unchanged: the default forum uses the same four state tags as a
bound forum (every post in it is a task, so no kind tag is needed).
- **When.** At start, spawned from `start_task_forums` (`forum.rs:273`, called at `dc/runtime.rs:412`, so no runtime.rs line),
  and again at each `Event::Ready` (a reconnect; `dc/runtime.rs:502-524`): your one runtime.rs line,
  `default_forum::ready(shared);`, which re-checks the record by id and resolves again on a 404. Consumers wait for it:
  the state is a `tokio::sync::watch` of `Pending | On(Forum) | Off`, `Off` from the start when `forum = false` or the
  guild rule fails, so nothing ever waits then.
- **The forum in the worker.** `Forum` (`forum.rs:99-106`) gains `default: bool`; the default forum is its own slot in
  `TaskPosts` (not in `forums`, which `set_forums` replaces at every bindings reload, `live.rs:519`), and `forums()`
  returns the bound ones plus it (dropped when a bound forum has the same id). It is private (the owner's alone), so
  `takes` (`forum.rs:110-112`) gives it every task the worker is told, from private and shared places alike.

**Every task gets a post.** "Every task" is every task the Discord surface sees: `work.watch` carries the sessions of the
binding's places (`dc/runtime/tasks.rs:1-10`: the DM, bound channels, task places, activity threads' places). Every place
already tells the worker its tasks (`sync_task_posts`, `forum.rs:302-330`, from `task_msg`, `live_board.rs:149`), which
returns early when no forum is bound; with the default forum on it no longer does, so the DM's tasks reach it too. A task
the surface doesn't watch (one started from the CLI in a session no place carries) is out of scope; say so in the report.
Each task gets **exactly one post**, of one of two kinds:
- **The task's place (a homed post).** A place is **forum-homed** (`Place::forum_homes`) when the default forum is on, the
  place is private (`place_rule.class(..) == PlaceClass::Private`, as `forum.rs:314-318`), it is the DM or a guild channel
  with `task_threads` on (`threads.rs:244`), and it is neither a task's own place nor an activity thread's (`is_thread`,
  `slug_of`, as `threads.rs:669-670`). For such a place's task, `home_of` says `Post`, and `make_task_thread` asks the
  forum worker for the post instead of calling `guild_thread_create` in the channel: `TaskPosts::make(view) ->
  Result<(u64, Option<u64>)>` over a oneshot, so **the worker stays the one maker of posts** (its `create`,
  `forum.rs:641-705`: the card as the first message, `applied_tags` = the state's tag, the `Slot` filled, the `Post`
  listed). Name the post `task_name(Mark::Running, ..)` (`threads.rs:154`), so the end's one rename
  (`finish_task_thread`, `:569`) gives it `✅`/`❌` as a thread's. Everything after the create in `make_task_thread` then
  runs unchanged: the `Made` row, the binding, the place, the back line (for the DM, `-# ↩ from the DM`, since `<#dm>` is
  no link), and the parent's quiet line, `🧵 <#post> · <title>`, sent with `say_to(&p.target, ..)` (`thread_bind.rs:415`;
  `say_pointer` builds a `channel:` target, which is wrong for the DM). That line is the DM's one-line link to the post;
  the Follow mirror (`follow.rs:153-177`) is not touched (task-channels edits `Mirror::text`). The worker's own sync (`one`,
  `forum.rs:464-494`) never creates a post for a homed task: `Seen` (`:148-156`) gains `homed: HashSet<String>` (the task
  ids whose `home_of` is `Post`), and such a task waits for its `make` (the worker adopts nothing it did not make). Make
  the DM sync: `parent_of_tasks` widened by `forum_homes()` (contract 2).
- **An index card.** Every other task: a shared channel's (it keeps its public thread, which its guests can see), a task
  started in an activity thread or in another task's place, a private channel with `task_threads = false`, and after the
  join a task with its own text channel. The worker makes it as today's bound forum makes a card-only post (`create`
  with the task's `link` from `Seen.links`, `task_threads.url_of`, which after the join is the channel's url): the card
  links where the task works, and its tag follows the state to the end under the existing paces. It is **not** the
  task's place: the work streams where the card links.
- **What a homed post is.** The task's place: its tool lines, stream, approval cards and replies post there (as a thread's
  do; the needs-you card still goes to the DM alone, as every `needs_you` post does), the card above edited at most once in
  2 s, the tag at most once in 5 minutes and the final one at once (`forum.rs:49-53, 117-126`). The task's report stays
  where the task reports (the DM or the channel), as a thread's does (`AGENTS.md`'s "What lands where").
- **Steering.** The owner's message in a homed post is `task_message` → `join_task` (`threads.rs:711-830`): `turn.submit`
  on the task's session, steered into a running task (theseus-esxyb). A guest's follows today's rule; war-room turns it
  into a comment after its join, with no change from you. Check that a post's messages reach a `Place` as a guild
  thread's do (the trusted guild's binding must admit the forum's channel; read `bindings.rs` and say what you found).
- **A message in an index card** starts nothing: one arm in `handle_in` (`thread_bind.rs:674`, after the `is_thread` arm)
  answers it with one quiet line, `-# This task works in <link>.`, and nothing reaches a session or a model
  (`TaskPosts::index_of(channel) -> Option<String>`, the link).
- **An archived post still takes its tags.** A post archives itself after a day of quiet (`ARCHIVE_MINUTES`,
  `forum.rs:59`), and a long task's index card may sit that long; Discord refuses a tag edit on an archived thread. The tag
  `PATCH` (`set_tag`, `forum.rs:738-765`) sends `"archived": false` beside `applied_tags` (one field). The fake models the
  rule in `forums.rs` (a channel `PATCH` with `applied_tags` on an archived post is refused unless the same body unarchives
  it), called from the existing channel `PATCH` handler (one line).
- **Restart.** A homed post's row is in `discord.task_threads` (`threads.rs:81`) and every post's card in
  `discord.task_posts` (`forum.rs:62`); `revive_task_threads` and the worker's `adopt` (`forum.rs:435-460`) bring both back,
  and no second post is made.

**Task channels (row 1) and the forum:** a task that also gets its own text channel **still gets its forum post**, which
links to the channel and keeps its state tags, so the forum stays complete. The channel is where the work streams; the post
is the index card. A task that qualifies for a channel at its start gets an index card from the start (the joiner's
`Channel` arm in `home_of`, contract 3). A task promoted to a channel after its post was its place keeps its post as the
index card: task-channels' promotion posts `-# moved to <#channel>` in it and calls `TaskPosts::moved` instead of
archiving it (contract 4). Say this in the report.

**Activity threads:** unchanged. `[threads] discord_channel` alone decides whether an activity thread gets a Discord thread
(`dc/runtime/guild_thread.rs:86-95`); the default forum never receives one, even when the key is unset or names nothing.

**Fallbacks.** A 403 on any forum call (the fake's `refuse_forum`; twilight `ErrorType::Response { status, .. }`, as the
courier classifies at `dc/courier.rs:119-140`): the state goes `Off` for this daemon run and everything is today's (a
channel thread for a guild task; the Follow mirror alone in the DM). The owner gets **one** DM line per run: `-# I can't
make the Theseus forum in <guild>: the bot needs Manage Channels and Manage Roles there (or set [discord] forum = false)`.
A failed post create is today's (`board.error`, retried after `RETRY`), and the task goes on in its place meanwhile.

**FAST:** nothing on a plain turn. Start: one `GET` (by id), or a list and one create once. A task: one post `POST` (a homed
task's in place of its thread `POST`; a DM task's where it had none; an index card's beside its thread), its card and tag
edits under the existing paces, one quiet line for a homed task.

**Tests** (`dc/tests_default_forum.rs`, the rig of `dc/tests_task_threads.rs`: `guild()` `:143`, `bindings()` `:151`,
`tweak` `:159`; a trusted guild set on the fake (`private = true` beside `guild_id`); `the_owners_message_joins_a_running_task…`
`:425-476` for a steer): (a) **created once when missing**: one `POST /guilds/{g}/channels`, type 15, name `theseus`, the three
overwrites and the four state tags asserted, the META record written; a second start on the same store makes none; (b)
**reused when present**: a type-15 `theseus` on the fake is recorded, no create; (c) **by id after a rename**: rename it on
the fake, start again, no create and the same id; (d) **a DM task → its post**, whose first message is the card, tagged
`running`; the DM gets `🧵 <#post> · <title>` once; the Follow mirror is unchanged; the end renames it `✅` and tags it
`done`; (e) **every task, one post each**: tasks in the DM, a private guild channel, a shared guild channel and an activity
thread's place give exactly four posts in the forum; the private channel's is homed with no channel thread; the shared
channel's keeps its thread and its post is an index card whose card links that thread and whose tag goes `running` →
`done`; the activity thread's is an index card; (f) **an archived post takes its final tag**: archive an index card on the
fake, end its task, the post is tagged `done`; (g) **the owner's message in a homed post steers** (`steers.joined(&task) ==
1`); (h) **a message in an index card** gets the one pointer line and no turn starts (no `turn.submit`, no session made);
(i) **activity threads untouched**: with the forum on and `[threads] discord_channel` unset, `thread.changed { opened }`
makes no post and no Discord thread; with the key set, today's tests pass unchanged; (j) **`forum = false`** on a trusted
guild: no request to `/guilds/{g}/channels`, no META record, no post, no DM line, and the DM-task scenario's request log
equals the same scenario on an untrusted guild; (k) **403** (`refuse_forum`): today's behaviour, and exactly one owner line
across two tasks and a reconnect; (l) a configured id that does not resolve: off, one line, no create; (m) units: `home_of`
(and that the index-card choice is `!= Post`), `TaskPosts::moved` (the post stays listed, its card links the new url, its
tag still follows the state, `owns` stays true, `is_thread` no longer names it once the row is replaced), `index_of`.
Plant a revert for (a), (c), (d), (e), (f), (g), (h), (j) and (k). The whole Discord suite passes with the default on; if
an existing test binds a trusted guild and changes, give its own rig `forum = false` only when the test is about something
else, change no assertion, and name it in the report.

**Live check for the maintainer** (write it in the report): restart the owner's daemon with no new config; see `#theseus`
appear in catorce, visible to the owner and the bot only, with four tags; start a task from the DM, see its post with its
card and the DM's link line, type in the post, see the steer; start a task in a shared channel, see its thread and an index
card in the forum; type in the index card, see the pointer line; rename the forum in Discord, restart, see no second forum.

**Limits for this row.** No new model tool, no tool field, no model-visible text (first request ≤ 8,000 tokens cli/task,
≤ 15,000 DM). No new slash command (the 26-command test at `dc/runtime/voice.rs:1768-1801` stays). No store format change
(META keys only). `dc/runtime.rs`: at most 1 line. The invite permission integer (`dc/runtime/guilds.rs:193`) is
task-channels'; don't change it.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
