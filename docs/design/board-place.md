# Where the future is drawn, and how a reminder is cancelled from the board (issue 30)

The board's **Coming up**, **Routines** and **Projects** are one snapshot of the whole daemon's future
(`runtime/horizon.rs`, `Tasks.future`), read for every conversation at once. Which place draws what of it is
`runtime/horizon.rs::parts_for`:

- **A task home is standing, or is switched on and still being looked for** (`TaskHome::expected`): the whole future is on
  the board the owner's DM place draws into the home's lane, and no other place's board has any of it. A place that is not
  given the future is not given a board for it: with nothing else to show, a place's board is written only where one is
  pinned (`LaneMsg::BoardIdle`). A board such a place drew earlier is edited once to its true state by that idle write
  ("all clear", or its own tasks) and left pinned; deleting or unpinning it is not done.
- **No home** (`[discord] task_home = ""`, no single trusted guild, or the channel refused): a place shows the reminders and
  check-ins of its own conversations: the lines whose `session_id` is its session, Home's when the place adopts it, and the
  threads that answer there (`Tasks::sessions_shown`). Routines, projects, a routine's runs, a task's start and a watch are
  not a conversation's wake and are shown on every private place, as before.

**With a task channel but no DM bound** there is no place to draw the task channel's board (it is drawn by the owner's DM
place), so the home branch is taken only when there is such a place; otherwise the no-task-channel rule applies and nothing a person
had before is lost. A board of the task channel's own, drawn with no DM, would need a place for its presses (Clear, Quiet, the
select) and for the tasks it holds, and the home has neither: that is a larger change than this one. The owner's DM place is the
first `[[dm]]` whose user is an owner, else the first.

A **line of Coming up** says where a conversation's wake came from (`· from #pier`); a check-in Theseus set for itself says
so (`· set by Theseus in #pier`). Names come from health's places (`Shared::conversation_names`; Home and the threads by
the place that adopts them); a session no place runs is named by nothing (`· set by Theseus`).

**Cancelling and snoozing.** One select under Coming up (`hq:pick`, `render::horizon::reminder_select`) offers Cancel and
Snooze 1 h for each reminder or check-in line shown. A choice is `wake.cancel` or `wake.snooze` as the presser
(`runtime/horizon_cmds.rs::reminder_pick`: the calls of `/cancel` and `/snooze`), told to the presser alone. Who may press
is the gate every press on a place passes (`allowed`: the place's users; the home's are the owners), so there is no rule of
its own. Tests: `tests_task_home.rs` (placement, the stale board, only the owner's press) and `tests_gateway/horizon.rs`
(cancel, snooze, the wording).

## After a restart: finding the board it had (line 3)

A lane's board message id lived in memory, and its only way back to the board was the channel's pins, so a board the bot could
not pin (or whose pin was refused) was made again at the first write after each restart. Now (`courier/board.rs::take_up_board`):

1. **The recorded id** (META `discord.board:<channel>`, written when the board is made): one read of that message. Found, pinned or
   not, however far up the channel it has gone; a message that is gone, or not the bot's, is no board. A board recorded as V2 is not
   taken for the text board, nor the other way round.
2. **The pins**, as before (a board an earlier version made has no record).
3. Neither: a new one is made, recorded and pinned, and the log says it was made.

A read of the recorded message that fails because Discord is away (not a 404, not a refusal) is not an answer: the write waits as
any write does in an outage and looks again, so a board that is there is not made a second time because Discord was busy. The
pins' own read failing is as it was (logged, a board made). A board an earlier version made that is neither recorded nor pinned is
not found, and one new board is made; it is recorded from then on.

**Cost:** one read of one message per lane per run, and when that finds nothing the pins' one read as before; one store write when a
board is made. Nothing is scanned and nothing depends on how busy the channel is. The task home's moved-board clean-up
(`earlier_board`) uses the record first too, so a DM's unpinned earlier board is removed when the home stands.

## A scheduled item is a card (issue 30, after the owner's first use)

The owner's words: the task channel "isn't a board but a normal channel", and a scheduled item should be thought of "similarly to
tasks on the board", "a new card into the task channel that is independent and not a reworked singleton item". A pinned message edited
in place weeks up the channel is nothing a person sees (the pinned board WAS edited when a reminder was set,
`tests_task_home::the_task_channels_old_pinned_board_shows_a_new_reminder_at_once` shows it: the fault was the shape). So a reminder, a
check-in Theseus set for itself, and a routine each get (`runtime/sched.rs`, words in `render/sched.rs`):

1. **A card of its own in the task home**: one new message per item, posted when the item is set, silent, no mention. Words: the
   item's name in bold with its kind's emoji, its time as Discord's `<t:…:f> · <t:…:R>`, and a small line `Reminder · from #pier ·
   `…ab12cd``, or `Check-in · set by Theseus in #pier`, or `Routine · weekdays 08:00`. A reminder or check-in carries **Cancel** and
   **Snooze 1 h** (`hq:cancel:<wake>`, `hq:snooze:<wake>`: the select's own option ids, so `reminder_pick` is the one place that calls
   `wake.cancel` and `wake.snooze`); a routine carries the board line's Run now and Pause or Resume (`hr:…`). The card is edited only for
   its own item's life: snoozed ("😴 Snoozed to this time"), fired ("Fired <t>", buttons gone), cancelled ("Cancelled by ana · <t>"), a
   routine paused or removed ("Removed · <t>").
2. **A post in the default forum** through the forum worker (`forum.rs`): its first message is the same words and a link button to the
   card (no button that acts), a `scheduled` tag (made the first time a scheduled post needs it, so a forum with none keeps its six; it
   is not in `Tag::ALL`), and at the end `done` and ✅ for a fired one, `stopped` and ❌ for a cancelled one, as a task's. A routine's post
   stays `scheduled`. Only the default forum, and only a private one (`takes`). The worker's paces (`CARD_EVERY`, `TAGS_EVERY`) apply.
3. **The pinned board no longer lists Coming up or Routines** where the cards are made (`parts_for`: `sched_target()` is some); it keeps
   its tasks and Projects. With no home, or no owner's DM place, or no V2 cards, nothing changed: the place's own board lists its
   reminders with the select. The cards are made only where the task home stands, the owner's DM place draws into it, and the bot sends V2.

**One worker, one request at a time** (`sched::start`), woken by `Future::dirty` (the work engine changed, a wake was heard, a command
acted) and by the snapshot changing, with a minute's safety read. It reads the pending wakes and routines of the future's snapshot and the
wakes the work model holds in any state (`Done` is fired, `Cancelled` is cancelled; a routine is not in that model). A card write that
fails stops the pass and is tried again after 2 s doubling to a minute. A pass that makes many cards (a first start) makes the first five
at once and each after them 2 s after the one before (`BURST`, `SPACE`): twenty reminders are 5 now and 15 over 30 s, leaving the task
home's channel half its bucket (five in five seconds) for a task's report; no reply waits, since the worker is not the lane.
(It writes with the client directly, as the forum worker does, because the lane's writes return no message id to edit later.)

**Restart.** A card, a forum post, or a task's own thread or channel made at Discord and not yet recorded is found again, not made twice: an intent (`discord.sched_intents`, `discord.post_intents`, `discord.place_intents`) is written before each create and read back first. A card is found by its own acts (a button on the item's full id) or, ended, by the last field of its footer, never the pinned board that lists the same task; a post by a thread the bot owns, in that forum, with the item's short id in its name; a task's thread the same way in its channel; a task's channel by its own name and the short id at the end of its topic; nothing is adopted while the bot's own id is unknown. A card's intent goes when its item has its entry or is over, never by age (a card does not archive); a post's and a thread's after three days (an unrecorded one has archived by then). META `discord.sched` lists each item: its card, the digest last sent, the kind, the time, who cancelled. A create carries the
nonce `sched:<id>` with `enforce_nonce`, so a card made while the list was lost is the same message; a card Discord lost is made again
under `sched:<id>:<old message>`. An item that fired or was cancelled while the daemon was down is nowhere in the snapshot and its card is
edited once to its true end (a wake that was due is "fired", one that was not is "cancelled"), once the work model has been read
(`Tasks::loaded`: before it, an item missing from it is not one gone). An item that already exists when this build first starts gets its
card and post then, once. A finished item's record is kept a day, for a late press, and for as long as its item stands again. An item that
ended more than an hour before it was first seen gets no card.

**Set again.** A fired reminder snoozed from its notice is set again under its own id: the core's work board takes it out of its ended
wakes, the worker trusts a pending agenda line read after the view ended, and the same card stands again ("😴 Snoozed to this time", its buttons back)
while its post is reopened (out of the archive, unlocked, its name without the ✅, `scheduled`).

**Presses** pass the gate of every press on a place (`allowed`: the home's people are the owners), so a member who is not an owner is
refused with the place's own words and nothing is written. A press on a card whose item is over says "That one is over already: its
card shows how it ended", and the worker brings the card to its end. A press on a card whose item neither the work model nor the agenda
holds any more says "That reminder is no longer on the board: nothing was changed." and calls nothing. The answer is the presser's alone
(ephemeral).

**Logs.** One info line a card write: the place's label, the message id, the kind, what (`posted`, `edited`), and the counts. No text.

**Choices** (decided, not asked): the worker writes the cards itself, not through the lane; a card that is deleted is made again while
its item stands, and one whose item has ended is not; cancelled-by is the presser's display name (else their user name: the cancel's
own record keeps the user name), a cancel from the CLI says "Cancelled from the terminal", and one the record names no one for says
"Cancelled" alone.

## A task set to start later is `scheduled` (issue 30, the owner's addition)

"Asking it to schedule the task results in a task in the forum in status 'running'; it should be 'scheduled' until it runs, then
running when it runs, then done." A task that is set to start later is a task whose execution is asleep on its time with no turn
taken (the core's `task_start::starts_at`); its work view is `Waiting` with `doing.what = Sleeping`, `doing.turn = 0` and
`next_at_ms` its start. `render::task_view::waits_to_start` is that test, in the binding, and every place that words a task's state
asks it (no protocol or core change):

- **The forum tag** (`forum::Tag::of`): `needs you` first, then the ends, then `paused`, then **`scheduled`** while it waits, else
  `running`. The one edit that leaves `scheduled` (the start) is not paced by `TAGS_EVERY`, as an end is not; it is one more edit.
- **The post's card and the `/tasks` card** (`task_view::state_words`): `🕒 scheduled · starts <t> · <in N>`, no "started ...", no
  "Doing" line. **`/tasks`' text line** (`render/deferred.rs`): `🕒 scheduled · starts Fri 09:00`.
- **The pinned board**: such a task is listed under its own `🕒 Scheduled` heading, apart from `⚙️ Now`, as `🕒 **title** `…id` ·
  scheduled · starts <t>`; the headline counts it as `🕒 1 scheduled`, never as running (and the status line's running count leaves it
  out, `counts_of`); the `/task` choices mark it 🕒.
- **One card, and no second post.** It is a scheduled item like the reminders: one card of its own in the task home, `Task · from DM
  @ana`, with Cancel (`task.cancel`) and Snooze 1 h (`task.reschedule` with `start_when = "in 1h"`: a task's start CAN be moved by an
  existing call) as the presser, by the same ids `hq:cancel:<id>` and `hq:snooze:<id>` (a task's id is no `wak_` id, which tells
  `Shared::card_press` which call). At its start the same card is edited to `▶️ **title** / Started <t>` with no buttons; if it is
  cancelled first, `Cancelled by <who>`. The post on the board is the task's OWN (the one it already had), not one made for the card.
- **Where the pinned board's Coming up keeps what has no card**: only a reminder or check-in, a routine's run and a task's start are
  taken off it (`parts_for`); a watch's give-up and a milestone stay. (The first version cleared the whole section.)
- **How the view carries it** (checked against the core, not guessed from titles or timing): the ledger of a deferred task is
  `task.created` (state `in_progress`, which is why everything drew it running), `execution.opened` with a future `starts_at_ms`,
  `execution.waiting` with `why: deferred` on a `due_at` wake, then `execution.queued why: due` and `execution.running` at its time.
  The work view carries the same fact as the execution's wait (`waiting_on = DueAt`, so `doing = Sleeping` and `next_at_ms`) and its
  turn count: `waits_to_start` is the core's own `task_start::starts_at` (asleep on `due_at`, no turn taken) read from the view, so
  the core's view needed no new field. The real-core tests (`tests_sched.rs`, a task started with `start_when`) are the proof.
- **The bot's status line** (`runtime/presence.rs`) says `running` only for work that works: a task that only waits for its start is
  not counted (`counts_of`), so with nothing else going the status is cleared; the next thing coming shows only as the next
  reminder's time does today, never as a count.

