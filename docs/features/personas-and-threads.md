# Many characters, one memory; separate lines of work

*One page on personas and threads. Numbers come from the tests and checks it cites. Updated 2026-10-28.*

## What it is

**Personas** are characters of one agent, not separate agents. Give Theseus more than one: a name, an emoji, a
speaking voice, and a file that describes its character. Bind one to a channel, or start a message with `@name` or
`/as name`, and that persona answers in the same conversation, with the same record and the same memory, so what you
told one, the others know.

**Threads** are separate lines of work. Make one for each thing you are working on, and each is a session of its own
with its own card, notes and context, so two can run at once without mixing. You don't have to say which thread a
message is for: a router places it, and asks in one line when it cannot tell.

## How it works

- **Personas** (`crates/theseus-core/src/persona.rs`, `engage.rs`): your persona file replaces the default one
  whole; who speaks is decided first by the session's own choice, then the channel's, then the default; a task
  speaks as the persona that started it. The personas' context files are one layer of every compiled request, so
  switching personas changes no byte of the cached prompt ([Context compiled for each turn](compiled-context.md)).
  `theseus persona import` brings another agent's workspace over as a persona, and that agent's own past ranks a
  little higher in recall while its persona is speaking.
- **Threads** (`crates/theseus-core/src/rpc/threads.rs` and the `thread_*.rs` beside it;
  [the design](../design/threads.md)): `theseus thread new site-redesign --topic web` opens a thread on
  `thread:<slug>`. Its working set grows from the paths, repos and branches its turns use (`thread_grow.rs`). The
  router fuses a free pre-score (the working set, which thread you were just in) with Jev's `thread` answer
  (`thread_flow.rs`, `tests_thread_route/`); when unsure it asks, listing up to four live threads, and offers a new
  thread rather than making one. The guidance you wrote for a topic comes with every thread on that topic. Threads
  can be paused, sealed, moved, merged, split and linked, and a turn that ran at home but wrote only one thread's
  files becomes that thread's memory.
- **One memory under both.** Personas and threads are views and sessions over the same graph
  ([It remembers everything](memory-across-sessions.md)); a thread keeps other threads' notes out of recall unless
  they are about the same subject.

## What you see on each surface

- **Discord:** a persona answers under its own name and emoji, and speaks in its own voice in a call; a thread can
  have a Discord thread of its own (`discord_channel`, off by default).
- **Terminal:** `theseus persona`, `persona voice`, `persona rate`; `theseus thread`, `thread new|show|edit`,
  `thread split home@<place>` when a place's home grows past its working budget; `theseus tui` lists every thread
  in one sidebar.
- **Cockpit:** the Threads page, and the Threads lens on the Operations page.

## The proof

- **Tests check each persona rule** (`crates/theseus-core/src/tests_persona.rs`, `tests_engage.rs`): the file
  replaces the default whole; the order of who speaks; a task speaks as its starter; and no byte of the cached
  prompt changes on a switch.
- **Tests show each thread opens its own session**, that two threads' turns overlap in time, and that a thread you
  name always wins (`crates/theseus-core/src/rpc/tests_threads.rs`, `tests_thread_route.rs`). On 40 scripted
  messages with a scripted Jev, the router puts at least 90% in the right thread and asks about at most 10%.
- **Live:** five personas are set up and five threads have been live on the owner's daemon since 2026-10-18.

## The limits today

- Jev's per-message persona pick runs in shadow: its pick is recorded, and the persona that answered last keeps
  answering until the pick is shown to be right.
- The personality exam (`bench/personality/`) and the thread benchmark (`bench/threads/`) are built and have not
  been run on the real model.
- A place's home that grows past `[threads] working_tokens` tells you once; nothing splits on its own.

## What is coming

- **After 1.0.4:** the thread router and the personality exam measured on the real model, published with the
  other benchmarks.

Related: [Jev in the core](jev-in-the-core.md) (the persona and thread questions),
[Durable tasks](durable-tasks.md) (a task speaks as the persona that started it).
