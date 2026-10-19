<!-- CLOUD_REPORT.md: the maintainer drops this commit at the merge, as he drops CLOUD_TASK.md. -->

# Cloud report: D3 `task-home` (theseus-xlxp4, theseus-lxfz0, theseus-90qqy)

Branch `cloud/20261019-task-home`, from `5e0bd195` (store format 45, unchanged by this row). Commits:

| Commit | What |
|---|---|
| `a21a2c66` | step 1: the board counts a failure apart (theseus-90qqy) |
| `098ffdd0` | steps 2 to 5: the report names its post, the task home, cards in the post, a voice place is never a task's home (theseus-xlxp4, theseus-lxfz0, theseus-90qqy) |

Step 2 shares files with step 3 (`forum.rs`, `threads.rs`), so it ships in the second commit with its test; the first
commit stands alone and is green.

## What I found and changed

**Step 1, the headline.** `board_v2::headline(needs, running, done)` took a count, so a failed task read "done since you
looked". It now takes the ended tasks' states and counts each way: `✅ 2 done · ❌ 1 failed since you looked`, then
cancelled and expired the same way; "since you looked" ends the group. The section below is unchanged. Golden lines that
moved on purpose, both in the "all done" case of `golden/board_v2.txt`: the header
`✅ 3 done since you looked` -> `✅ 1 done · ❌ 1 failed · ⛔ 1 cancelled since you looked`, and
`components=14 chars=273` -> `chars=302`. `render/snapshot.rs` and `runtime/emoji.rs`'s test follow the signature
(`[WorkState::Done; 3]`, same words as before).

**Step 2, the report names its post.** `place_of_task` only searched `TaskThreads.made`, so an index-card task (an
activity thread's, a voice place's) had no `<#post>`. `Shared::place_of_task` (threads.rs) asks `TaskThreads` first and
then `TaskPosts::post_of` (forum.rs, the post by task, the default forum's first). The courier's `report_post` and the
`/tasks` history (`tasks.rs`) both call it.

**Step 3, the task home** (`runtime/task_home.rs`, new). `[discord] task_home` (default `"theseus-tasks"`, `""` off,
validated as `forum_channel` is, in the template with a comment and its tests). Found or made like the default forum
(META `discord.task_home`, a configured id, a name, else a private type-0 channel with `OWNER_BITS` and `BOT_BITS`
overwrites and `@everyone` denied View Channel); a 403 or an id that is no text channel is one quiet DM line and the home
is off for the run; a network failure is tried again in a minute. Its lane and a private bound place (the DM's ceiling and
persona) come back at each start from the META record, whatever the switch says, so a report staged for it is delivered
(`revive_task_home`, called from `revive_task_threads`).

*The seam I used and why:* the core's, `Outbox::home_tasks(from, Some(TaskHome { target, label }))`, set in memory by the
binding (`register_task_homes`: the owner's DM place -> the home, each voice place -> the home, else the DM). Every place
that opens a task asks `Outbox::report_target(from)`: `rpc/task_start.rs` (`reports_to_of`, a small helper, since
`task_start` is at clippy's line limit) and the model's `task_create` (`task.rs`). So the task's `reports_to`, the
`task.place.<session>` META record and the report's lane all name the home, and nothing is rerouted in the courier. The
core knows no Discord: it holds a target and a label. The label is what the start tool's result says as a fact
(`Started task … reports to #theseus-tasks and its forum post when it finishes`; "to the DM and its forum post" when a
voice place's tasks fall back to the DM; `here` as before when nothing is homed). I added no instruction to any model
text. A rejected alternative: filtering in the courier after the fact, which would leave the core's records saying the
DM.

What lands there: the DM place's live board (`Place::board_lane`, used by `live_board.rs` and the text `board.rs`; one
pinned board; the home's channel is routed to the DM place so the board's presses work and a message typed there is
answered by one quiet line, as an index card's), every DM-started task's report, and one quiet pointer (`🧵 <#post> ·
title`, silent, no mention) per task: a DM task's from `make_task_thread` (sent to the home instead of the DM), an index
card's from the worker when it makes the card (`Worker::card_made`). A text channel with task threads keeps its own
board, reports and pointers. The DM keeps the conversation, the replies and the needs-you pings.

**Step 4, cards in the post.** `Worker::card_made` binds an index card as its task's place when the task has none of its
own: a lane for the post, a bound place with the parent's class, guild, ceiling and persona (so the task's class does not
loosen), and `Outbox::place_bound`. The approval card the core stages for the task's session therefore lands in its post,
never in the parent channel with an `<@owner>` mention. `Post.bound` (session, parent place; a serde-default field of a
META list, no store format) lets a restart bind it again (`revive_card_places`); the binding and lane go `GRACE` after the
post's last state is shown (`retire_card_place_later`). I did not edit `courier.rs`'s `card()` or `route()`: the post's
lane is a private "channel" place, so the existing route draws the card there. A card staged before the card exists goes
where the task reports.

**Step 5, a voice place is never a task's home** (always, whatever `task_home` says). `TaskHome::set_voice` reads the
bindings' voice channels (and again at a live reload: two lines in `live.rs`). Their tasks report to the home, else the
owner's DM; `board_lane` is `None` for a voice place (it asks the DM place to redraw instead), and the DM place's board
also shows the voice places' tasks while the home stands (`Tasks::project_with`, `Shared::board_sessions`); the task's
forum post is its index card. `voice/tasks.rs`'s "left to the session's turn" rule now also holds for a task started
from the voice place (`started_here`), since its `reports_to` is no longer the voice target (else the end would have been
spoken twice).

Also: `theseus-sim`'s `guild_channels` lists the text channels the bot made outside a category (a task channel sits under
its category and is not listed), so a restart finds the home by name; `crates/theseus-discord/AGENTS.md` has the task
home's paragraph, the corrected report line and the voice notes; `voice/tasks.rs`'s module doc follows.

Files touched outside the row's list, each small: `runtime/live.rs` (2 lines), `runtime/thread_bind.rs` (one guard: a
message in the home starts nothing), `runtime/tasks.rs` (`project_with`), `runtime/default_forum.rs` (two `pub(super)`,
the start call, the index pointer's words), `runtime.rs` (+1 line, `mod task_home;`: 3,498 of 3,500),
`theseus-core` `outbox.rs`, `task.rs`, `rpc/task_start.rs`, `config.rs`, `config/discord.rs`. No ceiling raised, no new
dependency, no protocol type, no `Cargo.lock` change.

## How I proved it

Disk on this VM is a fixed allowance and a `--workspace` debug build of every test binary exhausted it twice, so every run
below is `cargo nextest run -p theseus-discord -p theseus-core -p theseus-sim -E '…'` with `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_INCREMENTAL=0` (not the `--workspace` form AGENTS.md prefers).

- `cargo fmt --all --check`: ok. `bash scripts/shape.sh`: ok. `cargo clippy -p theseus-discord -p theseus-core -p
  theseus-sim --all-targets -- -D warnings`: clean. `cargo check --workspace --all-targets`: clean.
- Tests, `TZ=America/Phoenix`: theseus-discord **553 of 553**; theseus-sim **98 of 98** (needs `target/debug/theseusd`
  built); my new tests: `tests_task_home` 7, `tests_default_forum::an_index_card_tasks_report_names_its_post`,
  `render::board_v2::tests::a_failed_task_is_not_counted_done`, core's
  `outbox::tests::a_places_tasks_report_where_the_binding_homes_them` and
  `config::discord::tests::the_task_home_is_on_by_default_and_empty_is_off`. The three-package run of 4,035 tests had 27
  failures before I fixed the rigs: 24 in discord, 2 in sim (theseusd not built), 1 in core. The core one,
  `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`, passes alone and with
  or without `TZ` (a timing flake under load). The discord ones: most were the task-channel and default-forum rigs, whose
  guild is trusted, now making the home by default (see "Expectations" below), and the shared-channel tests, 4 of which
  still failed in a second run (`tests_task_threads::a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it`,
  `…::react::in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing`,
  `…::a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`,
  `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule`) that timed out at 101 s under that run's load
  and passed alone, together with the rest of their files, and in the final 553-test run.
- Under load (AGENTS.md's recipe: `nice -n 19` and four busy loops at nice 0, pids killed by number): the 8 tests of
  `tests_task_home` and the index-card test, three rounds, 8 of 8 each.
- The Discord proof, my live check of the binding on this VM: `target/debug/theseus-sim discord proof --theseusd
  target/debug/theseusd`: 12 of 12 steps passed in 2.0 s.
- `python3 scripts/keel-guard.py --base 5e0bd195 --head HEAD --quiet`: no output, exit 0. (Against this clone's
  `origin/main`, which is older than my base, it names 18 erosions from commits already on the base: that ref is stale
  here, not a finding.)
- The full gate: not run (no `cargo-deny`, and the workspace test build does not fit this VM's disk).

**Planted reverts**, each replayable: apply the edit, run the test, restore the file and `touch` it, `git status`. I did all
eight, restored each from a copy, and checked `git status` after each.

| # | File: edit (before -> after) | Test and what failed |
|---|---|---|
| P1 key | `runtime/task_home.rs` `set_voice`: `.filter(\|c\| c.voice)` -> `.filter(\|c\| c.voice && false)` (a voice place homes its tasks as a channel again) | `tests_task_home::a_voice_places_task_posts_nothing_in_the_voice_chat`, `assertion left == right failed: the record says where the report goes; left: Some("discord:channel:900000000000000010") right: Some("discord:channel:900000000000500001")` |
| P2 | `runtime/task_home.rs` `find_task_home`: `recorded.filter(\|r\| r.wanted == wanted && r.guild == guild)` -> `… && false)`, and the by-name `find` closure gets `false &&` first (skip the find) | `the_task_home_is_made_once_and_found_after_a_restart`, the made list holds a second channel (`assertion left == right failed` at the count) |
| P3 | `runtime/threads.rs` `make_task_thread`: `self.say_to(at.as_deref().unwrap_or(&p.target), line, &p.session);` -> `let _ = (at, line);` (drop the pointer) | `the_task_home_holds_the_board_reports_and_one_pointer_per_task`, `one pointer to 900000000000500009: left: 0 right: 1` |
| P4 | `runtime/task_home.rs` `start_task_home`: `guild.filter(\|_\| !cfg.task_home.is_empty())` -> `Some(guild).flatten()` and `cfg.task_home.clone()` -> `"theseus-tasks".to_string()` (the switch ignored) | `task_home_off_is_todays_dm`, `timed out: the board in the DM` (the board went to the home) |
| P5 | `runtime/threads.rs` `Shared::place_of_task`: delete `.or_else(\|\| self.task_posts.post_of(work))` (the old lookup) | `tests_default_forum::an_index_card_tasks_report_names_its_post`, the report's first line `📋 **Task 5d5d22 finished** · Anchor-check: check the anchor` has no `<#post>` |
| P6 | `runtime/forum.rs` `card_made`: `s.bind_card_place(&session, post, &parent, …);` -> `let _ = &parent;` (the old route) | `a_tasks_card_goes_to_its_post_not_the_channel`, `timed out: the task answers at its post` |
| P7 | `render/board_v2.rs` `headline`: `ended.into_iter().collect()` -> `ended.into_iter().map(\|_\| WorkState::Done).collect()` (every ended task counted done) | `a_failed_task_is_not_counted_done`, `left: "✅ 2 done since you looked" right: "✅ 1 done · ❌ 1 failed since you looked"` |
| P8 | `runtime/task_home.rs` `register_task_homes`: `home.clone().or(to_dm.clone())` -> `home.clone()` (no DM fallback for a voice place) | `a_voice_places_task_reports_to_the_dm_when_the_home_is_off`, the start tool's result lacks "reports to the DM and its forum post when it finishes" |

## Expectations I changed (each, and why)

None of an assertion. Two rigs now run with `task_home` off, as the task-channel rig already runs with `forum = false`:
`tests_default_forum::tune` (the forum's tests keep their DM expectations; `tests_task_home` turns the home on with its own
`rig`) and `tests_task_channels` (its `rig`, and the two tests that build a rig of their own: the home is a made channel
and would count in `made(&r)`). The golden's two lines (step 1) are the only expectation that moved. `tests_default_forum`'s
helpers became `pub(crate)` for `tests_task_home`.

## The live check for the maintainer

1. `TZ=America/Phoenix cargo nextest run --workspace -E 'test(/tests_task_home|tests_default_forum|tests_task_channels|tests_task_threads|board_v2|tests_war_room/)'`: all pass.
2. `scripts/build.sh` then `target/debug/theseus-sim discord proof --theseusd target/debug/theseusd`: 12 of 12.
3. On a scratch daemon with a test guild trusted (`private = true` beside `guild_id`) and a `[[dm]]`, a `[[channel]]` with
   `voice = true`: after READY a private `#theseus-tasks` exists (only the owner and the bot see it; `theseusd check`/health
   unchanged); a task started from the DM makes a forum post, one `🧵 <#post> · title` line in `#theseus-tasks`, a pinned
   board there, and its `📋 … finished` report there with `<#post>` on the first line; the DM holds the conversation and the
   needs-you ping only. Restart the daemon: no second channel, the board edits in place. A task started from the voice
   channel's chat posts nothing in it (a board, a report, a card, a pointer); its card is in its post. `task_home = ""`
   and restart: the DM's board, reports and pointer lines are as before.
4. Owner's config: the default is on, so his config needs nothing. `[discord] task_home = "theseus-tasks"` is in the
   template; the bot needs Manage Channels and Manage Roles in the guild, as the forum does (one DM line says so if not).

## Left over, uncertain, and choices the owner should hear about

- **A board pinned in the DM before the home existed stays there**, stale at its last state: the DM's board now writes to
  the home. I left it (a `BoardIdle` to the DM lane would draw an "all clear" board in the DM while tasks run). The owner can
  unpin it, or a follow-up can delete it at the first home write.
- **Pointers for every index card**, not only the DM's and the voice's: a shared channel's task, whose forum post is an index
  card, also gets one quiet line in the home (the title of a shared-channel task in a private channel; the private forum
  already lists it). A text channel with task threads that is homed in a Post keeps its pointer in its own channel.
- **Only the first owner DM** is redirected to the home; a second owner DM place keeps its board and reports.
- A report or card staged **before** its index card exists goes where the task reports (the home, or the parent place).
- The tool-result phrase says "and its forum post" whenever `[discord] forum` is on, without checking that the forum
  stands in this guild.
- Docs the maintainer writes: the spec's Part III item (the seam `Outbox::home_tasks`, the voice rule, no store format),
  `docs/status.md`'s landed step, and `docs/design/roadmap-v2.md`'s row; the README is untouched.
