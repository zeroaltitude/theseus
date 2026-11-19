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

A **line of Coming up** says where a conversation's wake came from (`· from #pier`); a check-in Theseus set for itself says
so (`· set by Theseus in #pier`). Names come from health's places (`Shared::conversation_names`; Home and the threads by
the place that adopts them); a session no place runs is named by nothing (`· set by Theseus`).

**Cancelling and snoozing.** One select under Coming up (`hq:pick`, `render::horizon::reminder_select`) offers Cancel and
Snooze 1 h for each reminder or check-in line shown. A choice is `wake.cancel` or `wake.snooze` as the presser
(`runtime/horizon_cmds.rs::reminder_pick`: the calls of `/cancel` and `/snooze`), told to the presser alone. Who may press
is the gate every press on a place passes (`allowed`: the place's users; the home's are the owners), so there is no rule of
its own. Tests: `tests_task_home.rs` (placement, the stale board, only the owner's press) and `tests_gateway/horizon.rs`
(cancel, snooze, the wording).
