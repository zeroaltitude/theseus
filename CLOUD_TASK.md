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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-task-home`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: task-home, a private task-home channel; a voice channel is never a task's home; three task-report fixes (theseus-lxfz0, theseus-xlxp4, theseus-90qqy)

Branch: `cloud/20261019-task-home`. Every commit's subject carries the Beads id of the step it builds
(`theseus-xlxp4`, `theseus-lxfz0` or `theseus-90qqy`). Deadline for the report: 6 hours after you start.

**Why.** The owner's Discord has a DM (his conversation with Theseus), a voice channel, a forum with one post per task
(`[discord] forum`), and long tasks' own channels under "⚙️ Theseus tasks". Overnight, tasks started from a voice
conversation put their board, "📋 Task … finished" reports and early approval cards in the voice channel's text chat,
which he found awkward; he expected the forum or a private channel. He agreed (2026-10-19 08:27) to **one private text
channel, owner and Theseus only, as the task home**: the dashboard of his tasks. It holds the pinned live board, the task
reports and one quiet pointer per task to its forum post; the forum keeps one post per task; long tasks keep their own
channel; the DM stays the conversation, plus the needs-you pings. Approval cards go to the task's forum post. A rehearsal
on the same night also found three bugs in reports and the board.

**Risk: medium.** It moves where task posts land for every place. The DM, threads, forum posts and task channels must
keep working with the task home off, byte for byte.

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| D3 | `task-home` | theseus-xlxp4, theseus-lxfz0, theseus-90qqy | the task home (`[discord] task_home`): made in the trusted guild if missing, the pinned live board, the reports, one pointer per task; a voice place's tasks homed there (or the DM); approval cards in the task's forum post; the report names its post; the board counts a failure apart | `runtime/{threads,forum,default_forum,task_channels,live_board,board,voice/tasks}.rs`, a new `runtime/task_home.rs`, `courier.rs` (`report_post`, `card()`, `route()` only), `courier/board.rs`, `render/board_v2.rs` + `render/golden/board_v2.txt`, `render.rs` (`report` only), `theseus-core/src/config/discord.rs` + the template, `theseus-sim/src/fake_discord/{channels,forums}.rs`, the tests named below | L |

## What exists (on your base; lines at `5e0bd195`; paths under `crates/theseus-discord/src/` unless named)
**Where a task's posts land.**
- A task's `reports_to` is set when it opens: `theseus-core/src/rpc/task_start.rs` near :81-84 (`ReportTo::Home` uses
  `outbox.target(home_session)`, `ReportTo::Board` none); the model's tool path `theseus-core/src/task.rs` near :559,
  :578 (`target = tc.outbox.target(tc.session_id)`); stored as META `task.place.<session>` (`theseus-core/src/outbox.rs`
  near :40, `task_record` near :566); `outbox.rs` near :488 `try_target` resolves bound place, then `reports_to`, then
  `shown`. In Discord the lane posts to its own channel (`courier.rs` near :1279 `place_channel`).
- The 📋 report: `courier.rs` near :1354 `report_post`, which calls `TaskThreads::place_of_task` (near :1367) and
  `render::report` (`render.rs` near :1006; the ` · <#id>` suffix near :1039-1042; tests near :2657-2714).
  crates/theseus-discord/AGENTS.md near :17 says the report names the task's own post or channel.
- Approval cards: the core stages a card to `outbox.target(task_session)` (`theseus-core/src/toolrun.rs` near
  :1321-1326, `turn.rs` near :2516-2519). That is the task's thread or post only after `bind_place` runs for it
  (`runtime/threads.rs` near :640-643, `task_channels.rs` near :498); before that, or for a forum **index-card** task,
  which is never bound, it falls back to `reports_to`, the parent place. `courier.rs` near :1053 `card()`:
  `Route::Here` in a `"channel"` lane mentions the answerers (near :1094-1109); `route()` near :1225.
- **Homes:** `runtime/default_forum.rs` near :80 `home_of(forum_homed, is_channel_place)` gives Post, Thread or None;
  `Place::forum_homes` near :438, `Place::home` near :462. `runtime/forum.rs` near :441-450 treats a task with its own
  channel, and every other non-Post task, as an **index card** (`TaskPosts.posts`, near :197; each `Post` has `task`,
  `post`, `link`, near :151-165). `TaskThreads::place_of_task` (`runtime/threads.rs` near :307) searches only
  `TaskThreads.made`, so an index-card task's report gets no `<#post>`. `url_of` (`forum.rs` near :262) gives a URL,
  `index_of` (near :310) channel to link, `owns` (near :304) a bool: nothing gives a post id by task.
- **The default forum** (the model for the task home): `runtime/default_forum.rs`: `start_default_forum` (near :209,
  only with exactly one trusted guild and `cfg.forum` on), `ensure_forum` (near :224), the off and retry paths (near
  :249-276), `forum_off` (near :278, one owner-DM line), `find_forum` (near :341: META `discord.default_forum` near :40,
  then a configured id, then `GetChannels` by name, then `make_forum` near :377: a type-15 channel with `OWNER_BITS` and
  `BOT_BITS` overwrites near :61, :65), `route_index` (near :415), `index_pointer` (near :476).
- **The config:** `theseus-core/src/config.rs` near :279 `pub struct DiscordConfig` (`forum`, `forum_channel` near
  :348-354; validation near :1467); defaults in `config/discord.rs` near :90-118 (`default_forum_channel` near :120,
  `validate_forum_channel` near :126); the template `theseus-core/config/theseus.example.toml` (`[discord]` near :784,
  the forum lines near :816-823), with `config/tests.rs`'s `example_template_parses_and_validates` (near :47),
  `example_template_uncommented_still_parses` (near :133) and `example_template_mentions_every_key` (near :1145).
- **Task channels:** `runtime/task_channels.rs`: `CATEGORY` "⚙️ Theseus tasks" (near :57), `channels_wanted` (near
  :234), `task_overwrites` (near :274), `ensure_category` (near :308), `task_channel_create` (near :345),
  `open_channel` (near :453); its own `🏁` line near :619-623. A thread or post is made in `make_task_thread`
  (`threads.rs` near :600-670; the parent's quiet `🧵 <#thread> · title` near :660-661); `parent_of_tasks` near :780.
- **Boards:** the live V2 board `Place::live_board` (`runtime/live_board.rs` near :155) posts to the place's own lane;
  links from `task_links` (`forum.rs` near :404); the plain board `runtime/board.rs` near :85; pins in
  `courier/board.rs` (`board` near :20, `find_board` near :48, `pin` near :65, best effort) and `courier/v2.rs` near
  :302. `runtime/tasks.rs` near :559 `project()` puts every ended state (Done, Failed, Cancelled, Expired) in `done`
  (near :575-578). `render/board_v2.rs` near :89 `headline(needs, running, done)` prints `✅ {done} done since you
  looked` from `p.done.len()` (called near :260); the section `✅ Done, not yet seen` near :306; `done_line` (near :182)
  varies only the glyph. The golden test near :740-803 (`THESEUS_GOLDEN=write`); its "all done" case
  (`golden/board_v2.txt` near :104-110) counts done, cancelled and failed as "3 done". `headline` is also used by
  `render/snapshot.rs` near :91 and tested in `runtime/emoji.rs` near :433-441.
- **Voice:** `bindings.rs` near :80 `ChannelBinding.voice`; `runtime/live.rs` near :455 and :678 configure
  `task_threads` as `c.task_threads && !c.voice`, so a voice place is a `"channel"` with threads off: no thread or post
  home, only an index card. `runtime/voice.rs` near :120 `Voice.places` is private (no "is voice" accessor).
  `runtime/voice/tasks.rs` near :136 `see()`: a task reporting to the voice place's own session is left to that
  session's turn (near :161); otherwise it is spoken via `Command::Report`. The spoken reply is the model's; the start
  tool's result text is `theseus-core/src/task.rs` near :774-787 ("Started task … It works on its own and reports here
  when it finishes …").
- **The stand-in** (`theseus_sim::fake_discord`): `refuse_pins(bool)` (`fake_discord.rs` near :681) is global and
  answers 50013; `channel_create` (`fake_discord/channels.rs` near :104) takes types 0 and 4, type 15 goes to
  `forum_create` (`forums.rs` near :102); `refuse_channels` (`channels.rs` near :93), `made_channels` (near :81);
  `guild_channels` (`forums.rs` near :67) lists seeded channels and made forums, **not bot-made text channels**, so a
  restart cannot find a made task home by name yet; there is no voice channel type. `fake_discord.rs` is at 2,450 of
  2,500 lines: new stand-in code goes in its modules.
- Tests: `tests_task_threads.rs` (rig near :55-211; `a_forum_post_is_the_tasks_live_card…` near :671;
  `a_task_waiting_on_a_card_has_the_card_in_its_thread…` near :788), `tests_task_channels.rs`, `tests_default_forum.rs`
  (`forum_up` near :143; `every_task_gets_one_post_across_four_kinds_of_place` near :470;
  `a_dm_task_with_a_channel_gets_an_index_card_linking_it` near :801), `tests_board.rs`
  (`three_tasks_one_asking_make_one_silent_pinned_board…` near :178), `runtime/voice/tests_tasks.rs`
  (`a_report_to_the_voice_places_own_session_is_left_to_its_turn` near :674; `started_task` near :769).

## What to build (one commit or more per step, in this order)
1. **The board counts a failure apart (theseus-90qqy item 2).** The headline counts only tasks that finished well as
   "done since you looked", and names the rest by kind ("✅ 2 done · ❌ 1 failed since you looked"); the section lists
   them as today with their glyphs. Update the golden on purpose and say each moved line. The snapshot's and emoji
   test's uses follow.
2. **The report names its post (90qqy item 1).** One lookup gives a task's own place by task, a thread, a channel **or
   its forum post, index card included** (extend `place_of_task`, or one function both callers use), so the 📋 report's
   first line carries `<#post>` for every task that has one, as AGENTS.md says.
3. **The task home (theseus-xlxp4).**
   - `[discord] task_home`: the channel's name or id, default `"theseus-tasks"`; `""` turns it off (the rollback: today's
     behaviour byte for byte). Validate it as `forum_channel` is. In the template with a comment, and in its tests.
   - Made like the default forum, in a new `runtime/task_home.rs`: with exactly one trusted guild, find it (META
     `discord.task_home`, then a configured id, then by name), else create a **private text channel** (type 0) whose
     overwrites let only the owner and the bot see it (the forum's `OWNER_BITS` / `BOT_BITS`); on a refusal, one DM line
     says why and the home is off until the next start, as `forum_off` does. It is a private place (the place rule).
   - What lands there: the pinned live board of the owner's tasks (one board, the one the DM shows today), every task
     report that today lands in the DM or in a voice place, and **one quiet pointer per task** to its forum post (`🧵
     <#post> · title`, never a ping), posted when the post is made. The DM keeps the conversation, the replies, and the
     needs-you pings; its board and its reports move to the task home. A place other than the DM and a voice place
     (a text channel with task threads) keeps its own behaviour.
   - Through the core's existing seam: the task's `reports_to` (or the outbox's target) names the task home's place, so
     the core's records say where a report went; do not reroute after the fact in the courier. Say in the report which
     seam you used and why.
4. **Approval cards in the task's forum post (90qqy item 3, folded into xlxp4).** A task's card goes to its own forum
   post (index-card tasks included) or its thread or channel; never to the parent channel with an `<@owner>` mention
   when the task has a post. The owner's alert is the DM's needs-you ping (another row reworks that ping's content).
5. **A voice place is never a task's home (theseus-lxfz0).** A task whose parent place is a voice binding gets its forum
   post as usual, reports to the task home (else the DM when the home is off), and posts nothing in the voice
   channel's text chat: no board, no report, no card. The start tool's result states where the task reports, as a fact
   ("It reports to #theseus-tasks and its forum post when it finishes"), so the spoken reply can say it in one line; add
   no instruction to the model.

## Tests (each with a planted revert; the first is the key test)
- **`a_voice_places_task_posts_nothing_in_the_voice_chat`** (through the stand-in, with a voice binding; add a voice
  channel type to the stand-in if you need one): a task started from the voice place gets its forum post, its report in
  the task home with `<#post>`, a card in its post, and nothing at all in the voice channel's chat. Plant: let a voice
  place home its tasks as a channel again.
- `the_task_home_is_made_once_and_found_after_a_restart`: no channel → one private text channel made with the owner's
  and the bot's overwrites; a restart finds it (META, then by name: teach the stand-in's `guild_channels` to list made
  text channels) and makes no second. A refusal → one DM line, the home off. Plant: skip the find.
- `the_task_home_holds_the_board_reports_and_one_pointer_per_task`: two DM tasks: one pinned board in the task home, two
  reports there, two quiet pointers, nothing new in the DM but the conversation. Plant: drop the pointer.
- `task_home_off_is_todays_dm`: with `task_home = ""`, the DM's board and reports are byte for byte today's (reuse
  `three_tasks_one_asking_make_one_silent_pinned_board…`'s expectations).
- `an_index_card_tasks_report_names_its_post` and `a_tasks_card_goes_to_its_post_not_the_channel`. Plants: the old
  lookup; the old route.
- `a_failed_task_is_not_counted_done`: the headline with one done and one failed. Plant: count `p.done.len()` again.
- Keep `every_task_gets_one_post_across_four_kinds_of_place` and the task-thread, task-channel and voice tests green;
  update an expectation only where this row moves a post on purpose, and list each in the report.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree, this task is right: `main` is at `5e0bd195` (2026-10-19 06:43 MST:
v1's tag `bbc65530` plus five merges) or later, with **store format 45** (a join that lands while you clone may move it
by one). The `file:line` references in this task are at `5e0bd195`; on your base a line may have moved a little: search
by name.

Line ceilings at `5e0bd195` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-discord/src/runtime.rs`
**3,497 / 3,500** (full: add at most one line there, and prefer none; another row of this batch may add one too),
`crates/theseus-discord/src/render.rs` 2,963 / 3,009, `crates/theseus-core/src/turn.rs` 3,384 / 3,525,
`crates/theseus-protocol/src/lib.rs` 2,706 / 2,733, `crates/theseus/src/render.rs` 3,108 / 3,128. Unlisted files must
stay at or under 2,500 lines (`scripts/shape.sh`): `crates/theseus-core/src/toolrun.rs` is 2,489 (**add no line to
it**), `crates/theseus-sim/src/fake_discord.rs` is 2,450 (put new stand-in code in a `fake_discord/` module). Never raise
a ceiling and never add a `long-files.txt` entry. Put new code in new modules; in a long file add only the `mod` line, the
field or the call.

## v1 patch1, the owner's Discord and cockpit rows (this batch)

Theseus's v1 is cut. The owner uses it every day through his Discord DM, a voice channel, a forum of task posts and the
cockpit. On 2026-10-19 he looked at what reached him overnight and asked for these four changes (Discord DM, 08:27,
agreeing to the maintainer's recommendation, and asking to tune the questions mechanism so Theseus operates better in
the channel, not only to word things more nicely). His standing rules that bind every row: features ship **on**, with a
named rollback (2026-10-06); right over compatible (one right name, no aliases; stored data stays readable); minimal
prompting: anything a model reads states facts, never instructions (2026-10-09); self-improvement acts first and reports
after, and its safety is the keel, the gates and one-click revert at the self-join, not approvals on its drafts
(2026-10-09).

| Row | Slug | Beads | Area |
|---|---|---|---|
| D1 | `needs-you-cards` | theseus-suvpe | the core's needs-you post (`theseus-core/src/needs_you.rs`, and the asker's name), its Discord card and buttons (`courier/ask.rs`, `render/ask.rs`, `runtime/ask.rs`, a new module for a reply that answers), the DM reply that answers a question |
| D2 | `self-tree-acts` | theseus-kq4z4 | `theseus-core/src/lsp/{mod,edits}.rs` (`judge_start`, the edit hook's `gate`), `theseus-core/src/rsi/own_tree.rs`, `theseus-core/src/toolrun/order.rs` (the `own` argument only), their tests |
| D3 | `task-home` | theseus-lxfz0, theseus-xlxp4, theseus-90qqy | `crates/theseus-discord/src/runtime/{threads,forum,default_forum,task_channels,live_board,board,voice/tasks}.rs`, a new `runtime/task_home.rs`, `courier.rs` (`report_post`, `card()`, `route()` only), `courier/board.rs`, `render/board_v2.rs` + its golden, `render.rs` (`report` only), the `[discord]` config (`config/discord.rs`, the template), the stand-in's channel routes |
| D4 | `fleet-fullscreen` | theseus-uu3b2 | `cockpit/` only: `src/views/Fleet.tsx`, `src/components/TaskGraph.tsx`, a new pure module and its test |

All four run in parallel now. Joins are plain merges, one at a time: D2, D4, D1, then D3.

**Other work in flight today** (stay out of their areas): joins queued on the owner's machine: WS B (the provider-origin
allowlist for private data, routed-turn checkpoints, Jev's warnings: `routing/`, `data_gate.rs`, `catalog.rs`,
`turn/route_step.rs`, `turn/checkpoint_step.rs`, `config/privacy.rs`), the speed rows (the census and first request,
the turn's path before the request leaves, idle memory and binary size), the Google Workspace core (`crates/theseus-google/`,
`theseus-core/src/google/`, one registration line in `toolrun.rs`), turn-end-facts (`toolrun/job.rs`, `turn/`), quick
recall; a cloud row still building job-truths (`theseus-kernel/src/job.rs`, `toolrun/job.rs`, `theseus-tools/src/fs.rs`,
`file_read.rs`); later, the Google mail, drive, sheets and calendar rows and an effort row.

**Contracts between these rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **`crates/theseus-discord/src/courier.rs` is shared by D1 and D3 at different places.** D1 edits only the
   `"needs_you"` dispatch arm (near :894) and, if it records which message holds which question, the landed-write hook
   (near :1642, `shared.noted(...)`) by one call. D3 edits only `report_post` (near :1354), `card()` (near :1053) and
   `route()` (near :1225). Neither touches the other's.
3. **The DM's needs-you ping is D1's; where a task's card and report land is D3's.** D3 moves a task's approval card
   to its forum post and its report and board to the task home; D1 makes the DM's needs-you post a set of answerable
   cards. Each test asserts only its own side.
4. **`runtime.rs`:** at most one added line per row (a `mod` line or one call); D1 and D3 may each take one.
5. **The store format:** none of these rows is expected to change a store layout. If a row finds it must store a field,
   it says why in the report and bumps by one from its base (the joiner renumbers).
6. **No new model-facing instruction.** Any text a model reads (a tool result) states facts: what exists, where a thing
   went, what happened. It never tells the model what to do, never says "must", and never gives examples.

## Out of scope for every row

The owner's config, his daemon and his machine (his config may get a key at the install; say which in the report).
`docs/status.md`, the README, `docs/design/` and the spec: the maintainer writes those; say what should change and
where. The maintainer's own rehearsal scripts (they are not in this repository). Any benchmark run.

## The checks for this row (these replace the preamble's gate line)

This VM has 4 cores and no sccache, and the maintainer runs the full gate on the owner's machine at the join. So scope
your runs to the crates you touch:
- From the preamble's setup, run `cargo --version` first and alone, then `cargo install cargo-nextest --locked`; skip
  `cargo-deny` and its fetch. Run `npm ci` in cockpit/ only if the row touches cockpit/.
- Before every commit: `cargo fmt --all --check`, `bash scripts/shape.sh`, `cargo clippy CRATES --all-targets -- -D
  warnings`, and `TZ=America/Phoenix cargo nextest run --workspace -E 'FILTER'` (crates/theseus-core/AGENTS.md: use the
  `-E` filter, never `cargo test -p` or `nextest -p`, which rebuild the dependencies with other features). CRATES and
  FILTER are named in "Rules for this row".
- Before your last code commit: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`, and paste its
  output in the report. It must be empty.
- A full `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at the end is welcome if your deadline allows,
  never required. Say in the report which checks you ran and their counts.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

## Rules for this row
- CRATES: `-p theseus-discord -p theseus-core -p theseus-sim`. FILTER: `package(theseus-discord) |
  package(theseus-core) | package(theseus-sim)`. Run the Discord crate's tests under nextest, as its AGENTS.md says.
- `runtime.rs` is full (3,497 / 3,500): at most one line there. `fake_discord.rs` is at 2,450 / 2,500: new routes in its
  modules.
- crates/theseus-discord/AGENTS.md: add the task home's paragraph and correct the report line (near :17) and the voice
  notes, in the file's style. Say in the report what the owner's config should hold (the default is on; his config may
  need nothing).
- The maintainer's rehearsal script that started the night's tasks in the voice session is ours, not in this
  repository: leave it.
