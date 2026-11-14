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

`/trust` is the Remove button's command. It clears the `injection` label of the session that **ran**,
which is not always this place's own: a message routed into a thread runs in the thread's session, and
that is the one the gate reads (F2b). Either route clears the same label through the same
`content.unlabel` call, and the removal is recorded with who did it.

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

## Threads

| command | what it does |
|---|---|
| `/thread new\|list\|bind\|unbind\|merge` | Threads: lines of work, each with a session of its own |
| `/move` | Move my last message to another thread, and answer it there |

A thread Theseus should stop routing to is taken out of the router's candidate set with
`theseus thread done <slug>` — an archive thread left `active` is still a candidate, which is how a
fresh request landed in one at 137k of context (F6).

## Yours alone

These the core judges as the owner's, from a private place.

| command | what it does |
|---|---|
| `/project trust\|untrust\|trusted` | A repository's own instruction files: trust them, take the trust back, or list them |
| `/orders` | Your standing orders, preferences and facts, kept in every prompt of their scope, with Revoke |
| `/extensions` | The loaded extensions: each one's digest, tools, and who acked it, with Revoke |
| `/publish` | Publish a node, a file, or a message into a place (only you, from a private place) |
| `/prompt` | Run an MCP server's prompt as this conversation's next turn (you, from a private place) |

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
