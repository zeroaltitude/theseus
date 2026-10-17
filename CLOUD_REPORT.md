# CLOUD_REPORT: discord-task-channels (theseus-bh3t.1)

Branch `cloud/20261017-discord-task-channels`, from main c6f62abf (store format 45). One commit for the row, because the
gate takes this VM most of an hour and I could afford one green run; the sub-steps are listed below by what they touch.

## What I found

- A task's place in the DM has no thread and no channel: only the board row and the Follow mirror. A task in a guild
  channel gets a public thread (`threads.rs`). Everything a thread does is keyed by the row (`Made`), the `threads` set,
  `urls`, `start_task_place` and `Outbox::bind_place`, so a channel can be a row of `TaskThreads` with a `kind` and every
  consumer (`task_message`, `join_task`, the board's links, `made_in`) works unchanged. That is contract 1, and it held.
- **`task_create` is a deferred tool in a task's place** (the `tasks` family is not loaded there), so in the rig a task cannot
  plan steps unless `[tools.defer] always_loaded` holds `tasks`. The tests set it. In the daemon a task's plan comes from
  the conversation (or a task whose family has loaded). Not a bug in this row; worth knowing when you try the live check:
  `expect = long` is the reliable trigger, plan steps are not.
- **A task is mostly one long turn, and the renderer shows no loop of a turn whose `turn.started` it did not hear.** A
  channel made while the turn runs (every promotion, and any `Steps` qualifier) would show nothing of that turn: no tool
  line, no stream. Threads have the same gap (their doc comment says so) but it is mostly hidden because a thread is made at
  the task's first sync. `Place::adopt_turn` (task_channels.rs) tells the renderer the turn started the first time an event
  names a turn it has not seen, for a task channel's place only. One call line in `runtime.rs` (3,487 -> 3,488 lines), and
  `Renderer::knows` in `render/held.rs`. Without it the first test (a) timed out waiting for the tool line.
- The base `origin/main` in this clone is older than c6f62abf (9fa84735), so the keel guard, left to its default base,
  reports other rows' changes as erosions. The gate was run with `THESEUS_KEEL_BASE=c6f62abf`.

## What I changed

All in commit **69fedb7b** (`discord: a private channel per multi-step or long task (theseus-bh3t.1)`).

1. **`expect` plumbing (no store format).** `theseus-protocol/src/task_start.rs`: `Expect { Short, Long }`,
   `TaskStartParams.expect`; `WorkView.expect` (`work.rs`, skipped when none). `theseus-core/src/task.rs`: `task.create`'s
   `expect` (schema: one enum, description "long: likely over 10 minutes"; first-request tests pass unchanged, see below),
   `expect_key` / `expect_record`: **a META key `task.expect.<session>`** written in the frame that opens the task (also in
   `rpc/task_start.rs`'s `owned_task_records`), only for `long`. The work board reads it once when it adds an execution
   (`push/work.rs` `add_exec`, `ExecIn.expect`) and `same_work` compares it. So no field of a stored record changed and
   `MANIFEST_FORMAT` stays 45; no `tests_layouts` sample owed. The 14 other `WorkView` literals got `expect: None`.
   The TypeScript is regenerated (`Expect.ts`, `TaskStartParams.ts`, `WorkView.ts`, `index.ts`); `Expect` went on the
   existing type-list line in `ts.rs`.
2. **Config.** `[discord] task_channels`, `task_channel_after_minutes`, `task_channel_cap`, `task_channel_keep_days`
   (defaults true, 10, 25, 14), after `live_board` in `DiscordConfig` and in the template at the same spot; the defaults
   are functions in `config/discord.rs`.
3. **The fake.** `theseus-sim/src/fake_discord/channels.rs`: `POST /guilds/{g}/channels` (types 0 and 4, `parent_id`,
   `permission_overwrites`, `topic`, Discord's 100-character name, 50 to a category), `PATCH /channels/{c}` for a made
   channel (name, parent, topic, overwrites; anything else goes to `thread_update` as before), `DELETE`, and
   `refuse_channels` (403 on all three). `GET /channels/{id}` knows made channels (one arm in `channel_read`). `FakeChannel`
   and `made_channels()` for the tests. Router: the PATCH arm now goes to `channel_patch`, plus two new arms (the budget was 4).
4. **The binding.**
   - `runtime/task_channels.rs` (new): `qualifies` (pure), `Why`, `Chan` (the 403 flag, failed tasks, the promotion timers,
     the in-flight creates that count against the cap), `channel_name`, the category helper `ensure_category` (META
     `discord.task_category`, `discord.task_category_done`, remade when Discord says 400 or 404), `task_channel_create`,
     `task_channel_close` (one PATCH: done category, `✅-`/`❌-` name, owner's Send Messages denied), the delete of the oldest
     when the done category holds 50, `sweep_task_channels` (at the start, then once a day) and `delete_task_channel`,
     `open_channel` (shared by a new task and a promotion), `channel_failed` (the one owner DM line, with the invite link),
     `Place::promote_later` (one `sleep` a task, no periodic timer) and `Place::adopt_turn`.
   - `runtime/threads.rs`: `Made.kind` (`PlaceKind`, serde default `Thread`), `Parent.threads`, `Next::Channel` and
     `Next::Promote`, `next(v, why, threads)`, a DM place that wants channels syncs (it makes channels only), channel rows
     survive `retire_task_thread` until the sweep, are not dropped by the 7-day stale rule, and an ended channel older than
     `GRACE` gets no lane at a restart.
   - `runtime/guilds.rs`: `PlaceBits.trusted_guild()` (exactly one `private = true` guild, else none) and
     `INVITE_PERMISSIONS = 326_686_067_792` (a named const, sum in its comment) used by the invite check's message.
   - `runtime/follow.rs`: the mirror says `-# in <#channel>` under its head. `runtime/start_task.rs`: the modal's last
     component, "Expected", `short` (default) / `long (over 10 min)`; `Submit.long` -> `TaskStartParams.expect`.
   - `runtime.rs`: +1 line (`pub(crate) mod task_channels;`, which replaces nothing) and +1 (`self.adopt_turn(&e)`); it is at 3,488 of 3,500.
   - `tests_task_threads.rs`: helpers made `pub(crate)` so the new tests reuse them (no test changed).

## Choices the owner should hear about

- **Only a private place gets a channel.** A task started in a shared channel (a guest-visible one) keeps its public thread,
  whatever its plan, because its guests are there (`channels_wanted` asks `Shared::is_private` of the parent's target). A task
  in a private guild channel (bound private or in the trusted guild) qualifies the same as one in the DM; if it does not
  qualify it gets the thread it always did.
- **Channels are made in the one trusted guild** (`private = true`); with none, or more than one, `trusted_guild()` is none
  and nothing changes (stated in the template comment).
- **A promotion text, not the V2 detail card.** For `Why::Promoted` the channel's first message is a plain text summary
  (origin line, bold title, what it is doing, the plan line) rather than `task_view::detail`'s V2 card: a notice post carries
  text only, and a V2 post would be a new outbox body kind.
- **The old thread is closed after `GRACE` (10 minutes), not at once.** The thread gets `-# moved to <#channel>` and the
  session's posts are rebound to the channel at once (`Outbox::bind_place` replaces the target; `place_unbound` would have
  removed the new one, so the old place is dropped without it). The thread's archive, its place and its lane go after GRACE
  so the pointer is delivered first (a post into an archived thread would unarchive it). Not tested end to end (the test would
  wait ten minutes); the rest of a promotion is (test j).
- **What a task did before its channel stood is in its thread (or nowhere in the DM).** Live progress is best effort.
- **Invite permissions** are the sum the brief gave; `theseus` does not print them anywhere else yet. Existing installs keep
  their old grant until the owner re-invites; the 403 line carries the link.
- A done category that is full deletes its oldest channel first; the cap counts open channels plus creates in flight.
- `Made.deleted_after_ms` was not needed: the sweep reads `ended_ms` and the config.
- No CLI flag for `expect` (`theseus task start --expect`): not asked; the modal, the model's field and the protocol have it.

## How I proved it

Commands (all with `TZ=America/Phoenix`):

- `cargo nextest run -p theseus-discord -E 'test(/task_channels/)'`: 14 tests (13 in `tests_task_channels.rs`: the rule,
  the name, and 11 rig tests (a) to (i) with the shared-channel and the thread-promotion ones; plus the paused-clock timer
  test), passing three runs in a row (about 4 s each), and again after every edit. Also `follow::tests::a_task_with_a_channel_says_so_under_its_head`
  and the modal's tests (`the_modal_has_a_brief_a_budget_select_a_report_select_and_an_expected_select`, renamed from
  `..._and_a_report_select`: five components now; the board's `new_task_opens_a_modal_and_starts_a_task_homed_here` also
  submits `expect = long` and reads the META key back).
- `cargo nextest run -p theseus-discord -E 'test(/tests_task_threads|tests_task_channels/)'`: 23 pass.
- `cargo nextest run -p theseus-discord` (full): one run before the last test edits had 456 pass and 3 TIMEOUTs
  (two existing `tests_task_threads` tests and my then-broken promotion test); both existing tests pass alone and together
  with mine, and the final gate run is below. I did not reproduce the two existing timeouts.
- `cargo nextest run -p theseus-protocol`: 165 pass (this writes the TypeScript).
- `cargo nextest run -p theseus-core -E 'test(/registry|first_request|example|template|layouts|tests_work|task_start|tests_task|tests_output|config/)'`:
  268 pass (the reader rule, the first-request budgets, the template parse, the layouts, the output golden, the work board).
  The golden did not move.
- A scratch-clock test: `runtime::task_channels::tests::one_timer_a_task_wakes_its_place_at_the_minute_and_marks_it_due`
  (`start_paused`): two syncs set one timer, nothing before the ten minutes, then exactly one `PlaceMsg::Tasks` and the task
  due. The promotion wake does not read the wall clock, which a paused clock does not move: the timer itself marks the task
  due, `qualifies` is pure on explicit `now_ms`.

Planted reverts (each: edit, run the named test, see it fail, `git checkout`/restore + `touch`, `git status` clean):

| plant | test that failed |
|---|---|
| `steps_total >= 3` in `qualifies` | `a_task_qualifies_by_steps_by_a_long_expectation_or_by_the_minutes_and_never_once_over` (0.18 s) |
| (a) no `@everyone` deny in `task_overwrites` | `a_task_with_two_plan_steps_gets_a_private_channel_under_the_category` (assertion, 1.7 s) |
| (d) `qualifies` promotes with no wait (drop the age test) | `a_short_task_gets_the_thread_it_always_did_and_no_channel` (1.5 s) |
| (e) a channel's id left out of the `threads` set | `the_owners_message_in_a_tasks_channel_steers_the_task` (times out at nextest's 120 s: the steer never reaches the task) |
| (f) the 403 flag never set (channels keep being tried, the line is said each time) | `a_refusal_for_want_of_permission_falls_back_to_threads_and_tells_the_owner_once` (two owner lines, 2.7 s) |
| (g) the owner's Send Messages not denied on close | `an_ended_tasks_channel_moves_to_the_done_category_and_closes_to_the_owner` (assertion, 2.9 s) |

I did not plant a revert for (b), (c), (h), (i) or the promotion test (the brief asked for a, d, e, f, g).

Not run in the cloud: the owner's daemon, a real Discord (the fake's rules for channels are my reading of Discord's: 100
characters, 50 to a category, a parent that is a category, 403 without Manage Channels), the benches (`THESEUS_GATE_NO_BENCH=1`).

## The live check for the maintainer

On a **scratch** daemon, a fresh state dir, never the operator's bindings. The bot invited with
`permissions=326686067792`; a bindings file (format 1) whose `guild_id` is the test guild with `private = true` beside it, and a
`[[dm]]` for the owner.

1. In the owner's DM: `/task new`, brief "write a haiku about the reef, slowly", **Expected: long (over 10 min)**, submit.
   Expect: a category `⚙️ Theseus tasks` in the guild with a text channel `<short6>-write-a-haiku-about-the-reef-slowly`
   that only the owner and the bot can see; in the DM one quiet line `📺 <#channel> · …`; in the channel first
   `-# ↩ from the DM`, then the task's tool lines and stream.
2. Type in the channel. Expect it to reach the task: `theseus --socket <s> history <task session>` shows the message, and the
   task's report ends `(you joined: 1 messages)`.
3. `/cancel` the task (or let it finish). Expect: the channel moves under `⚙️ Theseus tasks · done`, is named `✅-…`/`❌-…`,
   a `🏁 …` line is its last post, and the owner can read it but not type.
4. Take **Manage Channels** from the bot's role and start another long task. Expect: no channel, the task goes on as before,
   and one DM line: `I can't make task channels in guild <id>: grant the bot Manage Channels and Manage Roles (re-invite: …)`.
   A second task says nothing more until a restart.
5. `task_channel_keep_days = 0` in a scratch config, restart: the done channel is deleted at the start (the sweep), and its
   row leaves `discord.task_threads`.

## What is left, or uncertain

- The thread-to-channel move's archive of the old thread (after `GRACE`) is not exercised by a test.
- The fake's `permission_overwrites` is not checked against what the bot is allowed to grant itself (real Discord refuses an
  overwrite of a permission the bot lacks).
- `Why::Promoted` counts a task `Waiting` on its own calls as working (a long job is the usual reason a task runs ten
  minutes), but not a deferred task asleep (it has a `next_at_ms`).
- `Made` rows for channels are written in the same META list as threads (`discord.task_threads`); an older build reading a
  channel row would take it for a thread (no `kind`), and an older build is refused anyway only by `MANIFEST_FORMAT`, which
  did not move. Say if you want the kind in the key instead.
- Docs for the maintainer to write: the spec's Part III item and the version line; `docs/status.md` (the row, "channels per
  task"); the `[discord]` section of `docs/` if it lists the keys; `AGENTS.md` of `theseus-discord` and `theseus-sim` already
  have a paragraph each (in this commit).

## The gate

`CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=c6f62abf TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`
(`CARGO_INCREMENTAL=0` because the 20 GB incremental cache filled this VM's disk once, which failed the first full try in
`test build`; the keel base because this clone's `origin/main` is 9fa84735, older than c6f62abf, so the keel guard's default
range held other rows' changes). The door ran the older `origin/main` gate.sh first, then this tree's.

Phases: door, keel, keel tests, fmt, shape, features **ok**; clippy **ok** (an earlier try found a redundant clone in
`fake_discord/channels.rs`; fixed in this commit); cockpit (lint, tests, build) **ok**; test build **ok**; reader rule
**ok**; **suite: 6,251 tests, 6,216 passed, 35 failed, 47 skipped**:

- 33 are the known ones: 19 `theseus-sandbox::contract`, 13 `theseusd::sandbox`, `theseus-sandbox::bench spawn_100` (the
  root VM, L1 refuses a root daemon's job, theseus-pv6i).
- 2 are not in your list and not mine: `theseus-core tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future`
  and `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused` (an agenda that held
  1 line for 200). They failed under the suite's load and **pass alone** (`cargo nextest run -p theseus-core -E
  'test(/tests_horizon::(a_met_watch|the_agenda_expands)/)'`: 2 passed). The agenda is the work board's, which this commit
  touches only by a field (`expect`) on a view; the golden `tests/golden/horizon_board.txt` did not move.

Then the machine checks by hand, since the suite failed on the cases above: protocol types **clean** (the TypeScript is in the
commit); the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`) **ok** (frames 4/8/14, 2 syncs; the DM's first
request 13,983 of 15,000 tokens, the task's 6,788 of 8,000, the cli's 7,532 of 8,000: `expect` costs about 20); the lifecycle
and jobs benches are skipped (`THESEUS_GATE_NO_BENCH=1`) and `size` skips with no release build.
`cargo deny --offline check`: **bans, licences, sources ok; advisories FAILED** after `cargo deny fetch` today:
RUSTSEC-2026-0367, -0368 and -0369, all in the `zip` crate, an existing dependency (Cargo.lock is unchanged by this commit).
The first-request census, the template parse and the layouts passed in the core subset and the suite.

Also not clean in the full theseus-discord run I did before the last test edits: two existing `tests_task_threads` tests
timed out once (`a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`,
`a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it`); they passed in every other run, alone, together,
and in the gate's suite run. Not on the flaky list; named here.
