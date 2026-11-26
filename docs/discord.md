# The Discord slash commands

Every act Theseus will do for you in Discord has a command here. **You never need a shell for one of
them** — that is the standing rule this page exists to make true, and it is the half the product was
missing: `/project trust` shipped on 2026-11-12 with no page naming it, so when the owner asked for it
from his own DM the model searched the CLI's source, found only the terminal route, and sent him to a
terminal (the live freeze of 2026-11-13, F5). The commands were real; nothing could say so.

Discord lists them when you type `/`, with the same descriptions as below. The binding registers them
when it connects; `crates/theseus-discord/src/runtime/slash.rs` is the list in code, and the census in
`runtime/voice.rs`'s tests pins its length, so a command added anywhere moves a test.

The CLI's equivalents are in [cli.md](cli.md). Some acts have both routes, some only one; where the core
judges an act as the owner's (`Act::Trust`, `Act::Publish` and their kin), **a press or a command from
your own DM counts on its own** — it is your surface, and the core checks that it is.

## This conversation

| command | what it does |
|---|---|
| `/stop` | Stop what Theseus is doing here: its turn, jobs, and queued messages. The conversation goes on |
| `/new` | Start a fresh session here; the old one stays in the web UI |
| `/status` | This place's session: state, turns, dollars, waiting confirms |
| `/why` | What Theseus was shown for its last turn here, and why each piece was there (only you see it) |
| `/digest` | What finished, failed or was answered here since you last looked (only you see it) |
| `/trust` | Remove the injection label: this conversation's calls that change things run again |

`/trust` is the Remove button's command. It clears this conversation's `injection` label through
`content.unlabel`, and the removal is recorded with who did it. Messages are no longer routed to activity threads.

## Work

| command | what it does |
|---|---|
| `/tasks` | This place's background tasks: state, spend, and what each waits on |
| `/task` | Open a task: its details, Stop, Follow. Or start one with `new` |
| `/answer` | Answer a question that is waiting for you: buttons, a choice, a note, a file |
| `/quiet` | Silence this place's task pings for a while |
| `/cancel <id>` | Stop a background task and its jobs, or cancel a wake; `/tasks` and `/wakes` list them |
| `/wakes` | This place's pending wakes: when each is due, and its note |
| "Make this a task" | A message command: turn the message you right-click into a task |

## What is coming

| command | what it does |
|---|---|
| `/agenda` | Your future in one view: what is coming up, your routines, your watches (only you see it) |
| `/remind` | Remind you of something: it is posted here in your DM when it is due, with Done and Snooze |
| `/snooze` | Move a pending reminder or wake to another time |
| `/routine add\|list\|pause\|resume\|run` | Routines: work that runs on a schedule by itself |
| `/watch add\|list` | Watches: wait on the world with no model, and be told when it is true |

## Historical activity threads

`/thread` and `/move` were removed on 2026-11-22. A message runs in this place's conversation, not in a routed
activity thread. Stored thread records are kept; no command reads them.
Discord task threads are different: they still hold a task's posts and steers.

## Yours alone

These the core judges as the owner's, from a private place.

| command | what it does |
|---|---|
| `/project trust\|untrust\|trusted` | A repository's own instruction files: trust them, take the trust back, or list them |
| `/project item add\|close\|list` | A project's own list of items: add one, close one, or list the open ones |
| `/orders` | Your standing orders, preferences and facts, kept in every prompt of their scope, with Revoke |
| `/extensions` | The loaded extensions: each one's digest, tools, and who acked it, with Revoke |
| `/publish` | Publish a node, a file, or a message into a place (only you, from a private place) |
| `/prompt` | Run an MCP server's prompt as this conversation's next turn (you, from a private place) |

**Signing in to an MCP server, with no one at the machine** (issue 59). Ask in words to sign in to a server that
signs in (`oauth`): the turn can only offer a card, to your direct messages, with **Sign in** and **Sign out**. Your
own press of **Sign in**, in a private place, shows you alone the host you sign in at, every other place the sign-in
goes through (with **I agree** first when there is one), then the address to open and **Paste the address**. The
form takes the whole address your browser ended on and hands it to the daemon's own finish (`theseus mcp login
--manual`'s); it is in no message, log line, row or reply. The answer is fixed words: signed in and the tools found,
or why not (it ran out, not this sign-in's address, refused, the server said no). **Sign out** forgets the sign-in.
Anyone else's press, or a press in a shared channel, is refused and shows no address. When a server you signed in to
has its sign-in refused for good (`needs sign-in`), or an attach's trial was refused for want of one, the daemon
itself posts the same card to your direct messages, once for that change: not per retry, not again after a restart.

**Connecting Google, the same way** (issue 59). Ask in words to connect Google: the turn can only offer a card, to
your direct messages, with **Connect Google** and **Revoke Google**. Your own press of **Connect Google**, in a
private place, shows you alone the consent address and **Paste the address**; the form hands the whole address your
browser ended on to the daemon's own finish (`theseus google auth`'s), which keeps the grant exactly where that
keeps it. It is in no message, log line, row or reply, and the answer is fixed words. **Revoke Google** ends the
grant. Anyone else's press, or a press in a shared channel, is refused.

**Confirming the AWS alerts, the same way** (issue 59). Ask in words to confirm the alerts subscription: the turn can
only offer a card, to your direct messages, with **Confirm alerts**. Your own press opens a form with one field: the
Token=… value from the link in SNS's email (copy the link; do not open it), or the whole address. It goes to the
daemon's own confirm (`theseus aws confirm-alerts`'s), authenticated on unsubscribe, and into no message, log line,
row or reply; the answer is fixed words. All three cards are one pattern, written once.

`/project trust <path>` lets a repository's own `AGENTS.md` and `CLAUDE.md` join the system block;
`/project untrust <path>` takes that back, naming the files without reading them; `/project trusted`
lists what you have granted, with who granted each and by what route. The CLI's
`theseus project trust|untrust [PATH]` does the same thing from a terminal.

## Voice

| command | what it does |
|---|---|
| `/join` | Join a voice channel this place's bindings name, and talk there (only when you invite me) |
| `/leave` | Leave the voice channel; the conversation goes on in text |
| `/mute` | Stop Theseus hearing the voice call until `/unmute`; it finishes what it is saying |
| `/unmute` | Let Theseus hear the voice call again |

## What the model is told

The model does not read this page on every turn. What it is told is one line of the **self card**
(`crates/theseus-core/src/self_card.rs`), on a private Discord place only: that the person can act from
chat with slash commands, that Discord lists them, that `/trust` clears a content label and
`/project trust <path>` grants a repository's files, and that it should not send anyone to a terminal
for something a command here does. The card names the family rather than all twenty-nine commands,
because a list in a cached header goes stale at the next command and the person has Discord's own menu.
