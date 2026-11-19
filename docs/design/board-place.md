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
fails stops the pass and is tried again after 2 s doubling to a minute. Discord paces a burst: twenty reminders are twenty creates in one
channel's bucket (five in five seconds), the last a few seconds behind the first; no reply waits, since the worker is not the lane.
(It writes with the client directly, as the forum worker does, because the lane's writes return no message id to edit later.)

**Restart.** META `discord.sched` lists each item: its card, the digest last sent, the kind, the time, who cancelled. A create carries the
nonce `sched:<id>` with `enforce_nonce`, so a card made while the list was lost is the same message; a card Discord lost is made again
under `sched:<id>:<old message>`. An item that fired or was cancelled while the daemon was down is nowhere in the snapshot and its card is
edited once to its true end (a wake that was due is "fired", one that was not is "cancelled"). An item that already exists when this
build first starts gets its card and post then, once. A finished item's record is kept a day, for a late press. An item that ended more
than an hour before it was first seen gets no card.

**Presses** pass the gate of every press on a place (`allowed`: the home's people are the owners), so a member who is not an owner is
refused with the place's own words and nothing is written. A press on a card whose item is over says "That one is over already: its
card shows how it ended", and the worker brings the card to its end. The answer is the presser's alone (ephemeral).

**Logs.** One info line a card write: the place's label, the message id, the kind, what (`posted`, `edited`), and the counts. No text.

**Choices** (decided, not asked): the worker writes the cards itself, not through the lane; a card that is deleted is made again while
its item stands; cancelled-by is the presser's name, and a cancel from the CLI says "Cancelled" alone.

