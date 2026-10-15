# CLOUD_REPORT: discord-guild-tasks (theseus-i3do6)

Branch `cloud/20261013-discord-guild-tasks`. Commits (on top of the task commit `da021cbe`):

- `a68bbb8a` sim: the fake Discord keeps a thread's type and archive duration, its names and tags, and a forum's tags and V2 first message
- `ca8e7cfb` discord: a thread per task in a guild channel, and an optional task forum whose posts are the task list
- this report, as the last commit.

No store change (META keys only), no new dependency (`Cargo.lock` and `package-lock.json` untouched), no protocol type, no
config section (the bindings file's two new keys are in `bindings.example.toml`, commented, with their defaults). No person's
name anywhere.

I built the work as one feature commit beside the stand-in's, not one per numbered item: the forum shares the bindings key, the
board links, the V2 builders and the tests with the threads, and the tree does not pass clippy (`-D warnings`) with one half.

## What I found (read first)

- **The kernel's rule for an operator turn on a task's session** (`turn.rs` around the admission, `held.rs`): a task that is
  *waiting or blocked* is woken by the input (`WokenByInput`); one that is *over* refuses it (`execution_*`, "complete, not
  runnable"); one that is *running* queues the turn behind its running one (the steer, `turn/steer_step.rs`, takes only a
  person's input, and a task's turn is class `Task`). A task's turn usually ends the task, so the queued input is refused when
  the task ends, minutes after it was said. I saw that in a first cut of the test (a message at 0.5 s, refused with "complete,
  not runnable" 5 s later). So the rule I built (`Place::task_message`): a waiting/blocked task gets the message
  (`turn.submit` on its session); a **running or over task does not** (nothing is sent to it): the message goes **at once to
  the conversation that started the task**, quoting the task, and the thread says so (`-# ↪ the task is running: sent to the
  conversation that started it`). Tests: `a_message_in_a_task_thread_reaches_the_task_or_its_conversation_and_is_never_lost`
  (running: the parent hears it, the task does not, a user the channel does not list is not heard) and
  `a_task_waiting_on_a_card_has_the_card_in_its_thread_and_takes_a_message_there` (waiting: the task's session holds the message,
  the parent does not). **The owner should hear it**: a message to a running task's thread does not steer the task; it reaches
  the conversation, which can act on the task. Steering a running task needs a kernel step (a task turn the steer takes), not
  built here.
- A task's **report** (`📋 Task … finished`) is posted to the place the task reports to, by the task's record, not by
  `Outbox::target`: it stays in the channel (the conversation's news). The task's tool lines and replies are in the thread.
- The task's **earliest lines** are lost: a thread takes a round trip or two to make, and live progress before the task is first
  seen is not replayed (Item 6). The binding calls `session.watch` on the task at the moment it first sees it and keeps the
  events in a mailbox until the thread's place runs, which covers everything after that; the first tool line (proposed before
  the first `work.changed` reaches the binding) and a first loop's deltas (`turn.started` before the watch; a delta names only
  its turn) can be missing. An approval card the task asks in its first moments is staged before the thread exists and lands in
  the channel, as before threads. Found by load: a first cut of the test waited for the first tool line and hung one run in four
  under the recipe's load; the test now holds the task at a gate (a program that waits for a file) until the thread is bound,
  and checks the second tool line.
- A found race, fixed: the DM board's row link to a forum post was drawn only at the next change of the task; the worker now
  tells the place to draw its board when the post is made, and a finished post stays in the list a day for its link.

## What I changed

1. **A thread per task** (`runtime/threads.rs`; `bindings.rs`'s `task_threads`, default on, off in a voice channel):
   `POST /channels/{id}/threads` through the activity threads' one helper (`guild_thread_create`, which now takes the archive
   duration as an argument; owner decision 3: task rows use its helper), `type: 11` and `auto_archive_duration: 1440` sent,
   silent link line (`<#thread>`, a quiet notice) in the channel, name `⚙️ a1b2c3 · <title>` (≤ 100 characters), the task's
   session a place of its own (lane, events, posts), one rename (`✅`/`❌`), self-archive, the place and lane gone `GRACE`
   (10 min) after the rename, the open threads in META `discord.task_threads` (lane and bound place back at `start_lanes`,
   the place at the first sync). **The thread is bound with its parent's class, guild, ceiling and persona** (`bound_place`),
   so a task in a shared channel stays shared in its thread. `/new` in a task's thread is refused. The board's rows link the thread
   (`Parts.links`; `render/board_v2.rs`, one field and `link_of`).
2. **A message in the thread** reaches the task (above).
3. **The optional task forum** (`runtime/forum.rs`; `[[channel]] kind = "forum"`, parsed into `Bindings.forums`, apart from
   `channel`, so no place loop sees it): one worker talks to the forum; a post per task whose first message is the task's V2
   card (`task_view::post_card`: no button that acts, a link to the thread), edited at most once in 2 s; the four tags made once
   if missing; tag edits at most once in 5 minutes with the final state written at once; the DM's board row links the post.
   Open posts in META `discord.task_posts`; a restart takes a post up again (never a second post).
4. **The place rule** (`forum::takes`): a private forum shows every task, a shared forum only the tasks of *shared places in its
   own guild*; the shared channel's threads hold only their own channel's tasks (the work model files a task under its root
   conversation).
5. The stand-in (`a68bbb8a`): `type`, `auto_archive_duration`, every name, forum tags (`PATCH` with `available_tags`, at most 20),
   `applied_tags` (at most 5, each the forum's), a V2 first message with the post's id, `theseus-sim discord read threads`.

**FAST / the busy day** (10 tasks, 200 transitions): a thread is `GET` of its channel (inside the helper), one `POST`, one
quiet notice and one `PATCH` rename: 40 requests a day; the worst minute (ten tasks started in it) is ten creates and ten
notices (the lane and twilight pace them), the renames come as the tasks end; the 200 transitions cost the thread side nothing
(the sync only reads the model). The forum side writes a post's card at most 30 times a minute, so 200 transitions are at most
200 card edits in that minute, and a post's tags at most once in 5 minutes (10 posts: 10 tag edits per 5 minutes, plus the finals).

## How I proved it

- `cargo nextest run -p theseus-discord`: **355 passed** (one full run had `tests_process::an_answer_the_post_writes_after_the_fold_is_still_the_answer`
  fail once; alone it passed 3 of 3, and the next full run passed 355 of 355: a timing test, not mine).
  New: `tests_task_threads.rs` (8), `runtime/threads.rs` (1), `runtime/forum.rs` (4), `bindings.rs` (1), `render/board_v2.rs` (1).
- `cargo nextest run -p theseus-sim`: 95 passed. `cargo nextest run -p theseusd --test discord_proof --test outbox --test tasks
  --test wakes`: 19 passed. `cargo deny --offline check`: ok.
- Planted reverts (each: plant, run, restore with `touch`, `git status` clean after):
  - R1 the thread created as `PrivateThread` (twilight always sends `type`, so "omitted" cannot be planted; the stand-in answers
    12 for a create without it): `a_task_in_a_guild_channel_gets_a_thread…` fails, `left: 12, right: 11`.
  - R2 archive after a week: same test fails, `left: 10080, right: 1440`.
  - R3 a rename for each state the task moves through: same test fails, names `["⚙️ … ", "… [Running]", "✅ …"]` (3, not 2).
  - R4 the thread bound private always: `a_shared_channels_task_thread_is_shared…` fails, `left: Private, right: Shared`.
  - R5 tag edits unpaced: `a_posts_tags_change_at_most_once_in_the_interval…` fails, 22 tag sets instead of 1.
  - R6 `takes` always true: `a_shared_forum_shows_a_shared_places_tasks…` fails, 2 posts instead of 1 (and the unit test for `takes`).
  - R7 the task's events not routed to the thread (`route` without `place_of`): the thread test times out waiting for the tool line.
- Under load (AGENTS.md's recipe: four busy loops at nice 0, the tests at `nice -n 19`): `test(task_threads) | test(forum)`, 8 runs,
  14 of 14 passed each (11–26 s). Before the two fixes above (the early tool line, the board link redraw) two of eight runs
  timed out at nextest's 120 s; that is how I found them.
- The keel guard: `python3 scripts/keel-guard.py` reports one erosion, `crates/theseus-sim/src/lifecycle.rs:155: budget-raised
  "seed" => Some(250.0)`, in the range `merge-base(HEAD, origin/main)..HEAD`: it is from commits already on the branch before mine
  (its ack, on `1c2beb2747`, "does not verify (%G? is E)": this VM has no keys). None of my commits touches a test's
  assertion count, a budget, a cap or a ceiling. **Keel findings expected: none.**

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in `door`/`keel` on that finding (main's gate judges the tree
and cannot verify the signed acks here). I ran the later phases by hand, as the brief says:

| phase | result |
| --- | --- |
| fmt | ok |
| shape | ok (`runtime.rs` is at 3,449 of 3,500) |
| features | **not run** (no dependency or feature changed) |
| clippy `--workspace --all-targets -D warnings` | ok |
| cockpit lint, test, build | ok |
| test build, suite | **the whole workspace's test build does not fit this VM's disk allowance** (28 GB of target before it; "No space left on device" at the test build). I ran the suites of the crates my change reaches instead: theseus-discord 355/355, theseus-sim 95/95, theseusd's discord_proof, outbox, tasks, wakes 19/19 |
| reader rule, protocol types | not run / unchanged (`cockpit/src/protocol.gen` untouched: no protocol type changed; no ledger kind, event or method added) |
| benches | skipped (`THESEUS_GATE_NO_BENCH=1`; a thread costs nothing at start: the lanes of the open threads are made in `start_lanes` from one META key) |
| deny | ok |

The maintainer should run the whole gate (with its signature check) on the owner's machine.

## The live check on this VM (the Discord rig)

A real `theseusd` (debug build) on `theseus-sim fake-discord` (REST and gateway), `fake-model --rules` and the rig's fake `op`,
`#lab` bound private for ana and a private forum bound `kind = "forum"`. ana says `START THREE: run all three as background
tasks, please`; the three tasks run `sleep 6`, `10` and `14`. After 4 s: three threads in `#lab` (`type 11`, `archive 1440`,
names `⚙️ 8f84c3 · Alpha: chart the north reef` and so on), three forum posts (V2 cards, silent, tag `running`); after 18 s: each
thread renamed once, to `✅ …` (two names each), none archived by hand; each post's tags `running` then `done`, its card edited
three times; in each thread the task's tool line (`✅ proc.run sleep 6 · … · 6021 ms`, edited once) and its reply; in `#lab` the
silent link lines (`🧵 <#thread> · Alpha: …`), the silent board (7 edits) with `[↗ thread]` links, the parent's own reply and
the three `📋 Task … finished` reports. (An earlier run with a rule that matched the task's quote finished the tasks in 0.3 s and
still made, filled and renamed each thread.) The rig scripts are in the session's scratchpad (`live/up.sh`, `live/rules.json`).

**The owner's check** (in `#theseus-test`, and a scratch forum the maintainer makes and removes). The bot needs, in the channel:
View Channel, Send Messages, **Create Public Threads**, **Send Messages in Threads**; a thread's creator may rename it, otherwise
**Manage Threads**. For the forum: **Send Messages** and **Create Posts** (Create Public Threads) in the forum, and either
**Manage Channels** on it (the bot adds the four tags itself, once) or the four tags made by hand: `needs you`, `running`, `done`,
`failed`.

1. `#theseus-test` stays bound as it is (bindings are read live, so no restart); `task_threads` is on by default.
2. In `#theseus-test` say: `start three background tasks: each runs "sleep 40" then says its name (Alpha, Beta, Gamma)`.
   Expect within seconds: three public threads named `⚙️ xxxxxx · <title>`, one quiet line each in the channel linking them,
   the board with a `↗ thread` link on each row; the tool lines and replies in the threads, not in the channel.
3. After about 40 s: each thread renamed once to `✅ …`; a day later each archives itself.
4. While one runs, write in its thread: the channel's conversation hears it (`↪ the task is running: sent to the conversation
   that started it`).
5. A task that asks (`fs.write` outside the roots): its card is in its thread; press Approve there.
6. The forum: make a scratch forum channel `#task-forum-test`, add to the bindings file
   `[[channel]] id = "<forum id>"  guild = …  name = "task-forum-test"  kind = "forum"  users = [<owner>]  private = true`
   (a forum anyone can view must say `private = false`, and then it shows only shared places' tasks). Start a task from the DM or
   `#theseus-test`: expect a post named `xxxxxx · <title>` with the card as its first message and the tag `running`, `needs you`
   while it asks (after up to 5 minutes: tag edits are paced), `done` at the end at once, and the DM board's row linking it.
7. Remove the scratch forum and its binding line.

## What is left, and what the owner should hear

- **A running task's thread does not steer the task** (above). If the owner wants messages to reach a running task, that is a
  kernel step.
- **The forum's rule** (private forum: every task; shared forum: shared places' tasks of its guild) is mine; the brief said only
  "a private title never reaches a shared thread or forum". A shared forum shows the tasks of all the shared channels of its guild,
  whose audiences may differ: the stricter rule is a `home` key naming the channels a forum takes, not built.
- A forum post is renamed never (the tags carry the state) and its card has no button that acts (a post is no place that
  answers); the link goes to the thread. A forum without a thread (task started in a DM) links nothing.
- A thread shows tool lines and thinking as its parent does (`View::show`); the lane takes the parent's ping words.
- Threads and posts of tasks finished while the daemon was down: a thread open in the list is renamed at the next sync; a task
  already over with no thread gets none; a post list entry stays a day.
- `Next::Create` retries a failed create after 5 minutes and says it on the board; a 403 (no Create Public Threads) therefore
  repeats every 5 minutes while the task works. The task runs on in the channel as before.
- The thread's `GET` of the channel is the helper's (the activity threads' too); it could be cached.
- The scratch VM's disk: the gate's whole-workspace test build did not fit (28 GB `target/`); with `CARGO_INCREMENTAL=0` and the
  stale duplicates removed I stayed within it for the crates above.

## Where the docs should change (I did not touch them)

- `docs/spec`: Part III, a new item for discord-guild-tasks: what it built (above), how proved, the divergence (the message
  rule; the report stays in the channel; the earliest lines are not replayed; the forum's place rule), what it left open.
- `docs/status.md`: the recently landed step and the roadmap row.
- `docs/design`: the Discord threads row (the plan's §1.8, 8), the forum default (off), and the owner's decision 3 (one owner for
  Discord threads: the helper).
- `crates/theseus-discord/AGENTS.md`: done in `ca8e7cfb` ("A thread per task, and the task forum").
