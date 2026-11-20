# Answering a Discord interaction in time (issue 29)

The chat service wants the **first response** to an interaction (a button, a select, a slash command, a form's submit) within
**three seconds**. After that the interaction is unknown (error 10062) and the person sees "Something went wrong", whatever the
binding then says. The owner met it on the keep form: the vault's program took longer than three seconds, the binding answered the
submit only after the write, the value was kept, and the owner was told it had failed.

## The rule

A handler that waits for the daemon (or for the chat service's own API, or a file's download) before its first response may miss
the three seconds. So:

1. **A handler whose daemon call is not provably instant acknowledges first** and completes the same response after
   (`runtime/command_reply.rs`): `ack_ephemeral` (a deferred reply only the presser sees, completed by `finish_command` or
   `finish_data`) or `ack_update` (a deferred update of the pressed message, completed by `edit_pressed` or `edit_original_v2` where the press is the card's one writer, and by nothing where the daemon's settle is (see "One writer per card"), with a
   refusal told by `followup` to the presser alone). The acknowledgement carries nothing: no words, no value.
2. **A handler that must answer with a form** cannot acknowledge first (a form cannot follow an acknowledgement), so it waits for the
   daemon's read at most 2.5 seconds (`within_ack`) and says "Theseus was slow to answer. Nothing was done: try again" when it is slower.
   An autocomplete is the same, and offers no choices.
3. **A completed response waits for the daemon at most 14 minutes** (`within_follow_up`; the service lets a response be completed for
   fifteen): after that it says the card shows the outcome.
4. **A second submit of the keep form for a card that is with the daemon is refused at once**, in words that are true while it is
   ("a value for this card is being kept right now"), and asks the daemon for nothing.

"Provably instant" is a read of the daemon's own memory (`confirm.list`) and anything that asks the daemon nothing. A vault's program,
a model call, a task start, anything that writes a frame (it fsyncs), an MCP server and a download are not.

## What a deferral's flag says is who sees what completes it

**An answer that was private stays private, and when in doubt the acknowledgement is private.** In the service a deferral's flag
decides the visibility of what completes it: an ephemeral deferral (callback type 5 with the ephemeral flag) is completed
privately, an unflagged one publicly, and a deferred UPDATE of a pressed message (type 6) is completed by an edit of that message
for everyone, with any refusal told as an ephemeral follow-up. So each handler acknowledges as its final answer was before this
sweep: a private reply with an ephemeral deferral, a card updated for everyone with a deferred update.

`/prompt`'s form submit is the one that needs both: its refusals were private (one quotes what the person typed) and the prompt's
own line is for the place. It acknowledges **privately**, completes that private response (a refusal, or a short line that it
was sent to the place, which may still refuse it), and posts the place's line as a message of its own (a follow-up with no ephemeral flag). The arguments are in neither.
`/prompt name:` with no arguments is unchanged: its deferral is public and the place's line completes it, as it always did.

**The table that keeps it so** (`tests_gateway/acks.rs`, `every_handler_the_sweep_changed_acknowledges_as_its_answer_was_before`)
fires 36 of these handlers' acknowledgements (and `/task share` and a new task's submit as two more, 38 in all), through the real gateway path, with ids the daemon has never heard of (every handler acknowledges before
its daemon call, so the first response is the same whatever the daemon then says), and holds each first response to what its answer
was before the sweep. It does not reach: a scheduled card's Cancel and Snooze, for a reminder or a task (`card_press` refuses an item the board does not
hold, before `reminder_pick` and `task_pick`, with an immediate private answer, and the table has no such item), and a watch command's approval beyond its `no` button. `reminder_pick` is
in the table through the board's reminder select. `task_pick` is reached, and its privacy held, by
`tests_sched::the_owners_cancel_and_snooze_on_a_scheduled_tasks_card_act_on_the_task` (it asserts the answer is for the presser alone). A routine's button on the list is its own test
(`a_routines_button_on_the_list_updates_the_private_list`).

## One writer per card

A card is written by exactly one thing, and which one is decided by who settles it:

- **A card the daemon settles itself** (a question's card or needs-you post: the outbox's settle post, written at the moment the daemon
  answers the press's `work.answer`) is written **only by the settle**. The press on it acknowledges (an update of the card, so the
  three seconds are met) and **writes nothing to the card**: no words, and not the buttons either, because a Components V2 card keeps its
  words in its components, so clearing "only the buttons" re-sends the words the press read and is the same race. A read of the card
  before the write cannot make this safe: the settle lands between the read and the write. A press whose call fails or is refused tells
  the presser alone (a private follow-up); the card is as it was, with its buttons. `Shared::settled_by_daemon(question)` is the question
  ("has the question a post of the outbox"); a card from before the outbox has no settle, and its press is the one writer.
- **A card the daemon does not settle** keeps the press as its one writer (`edit_pressed`, `edit_original_v2`): the "asked again" line on an
  expired card, a thread question's card, a reminder's, a routine's or watch's post, a watch command's approval, a capture card's, a
  content label's.

| Handler | Its card | Its one writer |
|---|---|---|
| `ask_press`, `confirm` buttons (`on_interaction`), `answer_modal` and `proof_form` from a card | a question's card or needs-you post | the daemon's settle (the press writes only where `has_post` is false: a card from before the outbox) |
| the task board's answer press (`live_board.rs`, V2 cards) | an outbox card, or the ephemeral view of a task | the daemon's settle for an outbox card (`has_card`); the press for an ephemeral view and for a message that is no outbox card |
| `stop_task` (`live_board.rs`) | a task's view | the press |
| `keep_form` | the keep card | the daemon's settle (`vault_keep/settle.rs`); the press completes its own private response only |
| `ask_again_press` | the settled card of an expired question | the press |
| `thread_ask_press` | a thread question's card | the press |
| `remind_press` | a reminder | the press |
| `post_press` (routine, watch) | the routine's or watch's post | the press |
| `watch_approval` | the approval message, ephemeral | the press |
| `card_answer_as` (capture card), content label | the card | the press |

The stand-in holds both orders on purpose (`withhold_edits`, `withhold_completions`): the settle applied after the press is over, and
the press's completion applied after the settle. In both the card ends with the settle, word for word (`tests_gateway/ask.rs`).

## The proof

`theseus-sim`'s fake chat service keeps the rule (`set_ack_limit_ms`, default three seconds; `backdate_interaction` for a test that
must not wait): a first response after the limit is refused with 10062, a response completed after an acknowledgement is accepted at
any time, and an edit of a response shows in the view the person sees (`reply_message`), an ephemeral deferral making what completes
it ephemeral. `runtime/tests_ack.rs` holds a daemon call open (no sleep) and reads that the press is already acknowledged;
`theseusd/tests/keep_form.rs::a_slow_vault_does_not_make_the_form_miss_the_services_three_seconds` holds the vault's program open with
a pipe and reads the same for the keep form, a second submit, the completed outcome, and the value found nowhere.

## The sweep (every handler that awaited the daemon before its first response)

Acknowledge first, new in this change: the keep form's submit; the proof form's submit; an `ask` card's option and "Ask again"; a
thread ask's answer; a new persona's soul; the answer modal (`/answer`'s note and file); a routine's or watch's post buttons; the
reminder select and a scheduled task's card (cancel, snooze); `/agenda`, `/routine list|pause|resume|run`, `/snooze`, `/watch list`, the
routine list's buttons, `/routine add` and `/watch add`'s submits, a watch's approval; `/orders` and its Revoke; `/extensions` and its
Revoke; `/project`; `/thread`; `/remind` and a reminder's buttons; a cited source's "Show source"; `/task share`; a new task's submit;
`/tasks done|all` and its refresh; a failed task's Retry and Dismiss; a task's confirmed Stop; a prompt's submit.

Already acknowledged first (checked, unchanged): a card's Approve and Decline, slash commands in a place, `/why`, `/digest`, "should
have asked", a card question's answer, Jev's labels, a standing record, a seal, a link, an ontology undo, a content label, a self-note's
buttons, a persona's Decline, a task board's answer press.

Forms, waiting at most 2.5 seconds for a read: the keep form, the proof form, a card question's Edit, `/prompt`'s arguments (and its
autocomplete).

Instant, unchanged: opening a task's view, the board's pick and clear, a new-task or routine or watch form, `/quiet`, `/task`,
Follow, a card's Details and `/answer` (they read `confirm.list`).
