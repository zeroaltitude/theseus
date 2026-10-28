<p align="center">
  <picture>
    <source media="(prefers-reduced-motion: reduce)" srcset="docs/assets/theseus-logo.svg">
    <source srcset="docs/assets/theseus-logo-animated.svg" type="image/svg+xml">
    <img src="docs/assets/theseus-logo-animated.gif" alt="Theseus: a Greek ship on an old coin, sailing at night. Most of its planks are worn ivory; a few are new gold ones." width="200">
  </picture>
</p>

<h1 align="center">Theseus</h1>

<p align="center">
  <strong>A fast, durable AI agent that runs many tasks at once, lets you watch and steer each one from wherever
  you are, and remembers everything it has done.</strong>
</p>

<p align="center">
  <img alt="Version 1.0.2" src="https://img.shields.io/badge/version-1.0.2-d6a548?style=flat-square">
  <img alt="Rust 1.98 or later" src="https://img.shields.io/badge/rust-1.98%2B-13314d?style=flat-square">
  <img alt="Runs on Linux" src="https://img.shields.io/badge/runs%20on-Linux-13314d?style=flat-square">
  <img alt="License: MIT or Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-13314d?style=flat-square">
</p>

<p align="center">
  <a href="docs/status.md">Status and roadmap</a> ·
  <a href="docs/benchmarks/omnibus.md">Benchmarks</a> ·
  <a href="docs/the-ship-of-theseus.md">The design document</a> ·
  <a href="#set-it-up">Set it up</a>
</p>

Theseus is an AI agent for one person or a small team. You talk to it in Discord, in a terminal, or in your
browser, and it does real work with real tools: it edits code, runs commands, reads the web, and runs long jobs in
the background. It is one Rust daemon and a small client. It calls Anthropic's Claude models directly, and any
model served through the same API.

Eleven things set it apart. Each one below says what you see, and how we know it works. The first six are measured.
The next two are what the rest stand on, and we have started to prove them. The last three are newer: they are on, and
each one says plainly where it is still rough.

<p align="center"><img src="docs/assets/planks.svg" alt="" width="100%"></p>

## 1. Tasks you can watch, answer, and join from anywhere

**What you see.** Ask for something long, and it becomes a task with a place of its own: a Discord forum post or a
private channel with a live card, a pinned board that moves from *working* to *needs you* to *done*, and the same
task in the terminal, the web cockpit, and [herdr](docs/herdr.md). When a task has a question, it shows up on every
surface. Answer it with a Discord button, a key in the chat, `theseus answer`, or a herdr key, and it clears
everywhere at once. Type into a running task, and it takes your message at its next step; one key takes you back to
your conversation. A task with nothing to do is a small record on disk, not a process, so you can hold thousands.

<p align="center"><img src="docs/assets/cockpit-tasks/01-tasks-page.png" alt="The cockpit's Tasks page: a New task box, one task that needs an answer with its options as buttons, two working tasks (one paused) with their spend against their limits, and the done tasks with their report lines." width="720"></p>

**Proof.** The whole path was run live on a scratch daemon with a stand-in Discord (2026-10-17): a task's forum
post, its own channel, the board's edits, an approval given in Discord and another from the CLI, each cleared on
every surface, and a message typed into a running task that landed 25 ms after the wait it cut short.

## 2. As good at coding as Claude Code

**What you see.** It edits, builds, tests, and fixes the way a good coding agent does, with language servers as
tools and each edit's new compiler errors in its result.

**Proof.** On all 89 tasks of Terminal-Bench 2.0, with every harness at high effort, Theseus as shipped solved
73.0% of its trials at $0.136 a solved task, the least of the four. The best other harness, Claude Code on its best
model, solved 75.3% at $0.173. Task by task the two are level: −2.2 points, 95% interval −7.9 to +3.0. OpenCode
(70.8%) and Pi (70.4%) follow (2026-10-18). On the same model, Claude Sonnet 5.5 for both, Theseus solved 75.7% to
Claude Code's 71.9% (2026-10-13). [The reports](docs/benchmarks/omnibus.md#does-it-solve-real-tasks).

## 3. Fast, and it stays fast

**What you see.** The prompt is ready in tens of milliseconds, the first words of a reply appear as the model
writes them, and restarting or upgrading it is routine.

**Proof.** Against Claude Code on the same stand-in model (2026-10-18): a ready prompt in 39 ms against 498, 10 ms
of CPU a turn against 1,655, and 70 MB idle against 360. Speed has written budgets, and every commit's test gate fails
the commit if one slips:

| What | Budget | Release build |
|---|---|---|
| Start to answering | 50 ms | 18.5 ms |
| Clean shutdown, with work in flight | 100 ms | 41.2 ms |
| Crash, restart, and answer again | 150 ms | 23.7 ms |
| Swap in a new binary under load | 200 ms | 54.2 ms |

## 4. Kill it, and nothing runs twice

**What you see.** Pull the plug in the middle of a task. When it comes back, it picks up where it was. Nothing it
already did is done again.

**Proof.** Killed with SIGKILL mid-task and restarted, Theseus resumed 10 of 12 trials with 0 effects done twice and
no finished step redone. One trial in 12 left a process running. Claude Code repeated an effect in 4 of 12
(2026-10-18; [the report](docs/benchmarks/2026-10-18-async-kill-family.md)). Every action is written to disk before it
is attempted, and its result when it settles, the way a bank treats a payment.

## 5. It remembers your other sessions

**What you see.** Something you said last week, in another channel, about another project, comes back when it
matters, with a link to the message it came from. Bring your history from Claude Code, Codex, or OpenClaw
(`theseus import`), and it knows your past from day one.

**Proof.** Over 600 scripted turns in three sessions, Theseus answered 73.1% of recall questions, against 56.9% for
Pi and 50.7% for Claude Code. Across sessions it answered 27 of 46; they answered 0 and 1 (2026-10-18;
[the report](docs/benchmarks/omnibus.md#does-it-remember)).

## 6. It fixes itself, and tells you after

**What you see.** Each night it reads its own record for what went wrong and for work it keeps repeating. It fixes
the faults and turns a repeated unit of work into a tool of its own. A new MCP tool loads that night, and a new
built-in tool ships in the next nightly build. In the morning it tells you what changed, why, and what it cost. A
change that makes things worse reverts itself. A short list of things always needs your yes: loosening a test or a
budget, security and permission changes, spending limits, and the off switch.

**Proof.** Its tests walk each step. A green self-made branch is reviewed by a different model, joined, signed, and
announced once (`crates/theseus-core/src/rsi/join/tests.rs`). A regression planted after a self-made join is found,
bisected, and reverted (`crates/theseus-core/src/rsi/revert/tests.rs`). The build runs in the sandbox, where it
cannot see the held-out exams that score it (`crates/theseusd/tests/self_build.rs`).
[The plan](docs/design/rsi-plan.md).

## 7. Small, fast judgments steer every message

**What you see.** Before the model answers a message you send, Theseus asks [TypeSafe](https://typesafe.ai)'s Jev a
handful of small questions with typed answers (a choice, a score, a yes or no): what kind of message this is, which
model should answer it and with how much effort, which persona speaks, which thread it belongs in, and whether what it
carries looks private. Jev also orders the notes recall found, and during the turn it judges each tool call for risk
and the loop for whether it has reached its stopping point. So a thank-you goes to a small model and a hard refactor to
a large one at high effort. Each answer takes about a tenth of a second and a small fraction of a cent, and each one is
recorded with its probabilities: `theseus why` says which model a turn ran on and why, and `theseus judge log` lists
the judgments. If Jev is slow or down, nothing waits on it: the turn keeps its own model and recall's own order.

**Proof.** On all 89 tasks of Terminal-Bench 2.0 at high effort (2026-10-18), Jev ran 59% of Theseus's turns on Sonnet
5.5 at high effort, 24% on Sonnet 5.5 at medium and 15% on Haiku 5.5. Theseus solved 73.0% at $0.136 a solved task,
level task by task with Claude Code's 75.3% at $0.173. On the 30 hardest tasks at top effort, Jev kept 90% of turns at
high effort or below: Theseus solved 67.8% to Claude Code's 75.6%, at $0.36 a solved task against $1.92
([the report](docs/benchmarks/2026-10-18-terminal-bench-v1-max.md)). Tests drive each judgment with a scripted Jev: a
hard question goes to Opus, a quick one detours to Haiku without changing a byte of the next cached prompt, a failing
Jev leaves the request as it was, and Jev's order of recall is the order in the request
(`crates/theseus-core/src/tests_route.rs`, `tests_rerank_live.rs`). Still rough: it sometimes calls a hard task
routine and sends it to Haiku, and every turn it called routine on the hardest tasks failed. Its data verdict calls
open-source work private about 60% of the time, so nothing acts on that verdict yet. Both are being fixed in v1 patch1.

## 8. Context built for each turn, not piled up

**What you see.** Each request the model gets is compiled from the store for that turn, and the conversation is only
one part of it. It holds the standing instructions and personas, the context files you listed, the guidance you wrote
for the thread's topics, the conversation so far (with a summary in place of what no longer fits), and the notes
recall found for this message, in the order Jev put them. Attach a session to a project (`theseus project attach`),
and every request ends with the project's slice: its goal, its open milestones with their dates, its next steps, and
the first of its "done when" lines, in at most 300 tokens. A compiled request has a manifest that names each node,
file and summary that went in. `theseus why` prints it: each block with its tokens and where it came from, each
recalled note with its date and why it was recalled, and what was left out and why.

**Proof.** Tests check that a request is three layers, a session block and the messages
(`crates/theseus-core/src/compiler/tests_layers.rs`); that threads and sessions share their first layers byte for
byte, so the cache holds, and that a thread's topics make its third layer
(`crates/theseus-core/src/rpc/tests_thread_context.rs`); that an attached conversation keeps its project's slice
through three compactions (`crates/theseus-core/src/tests_projects.rs`); and that `theseus why` prints a turn's route,
recalls, drops and gated tools (`crates/theseus/tests/why.rs`). We have started to prove it live: the v1 patch1
project is attached to the owner's sessions, and in a check on 2026-10-19 a session attached the same way quoted the
project's goal and first "done when" line from its context, while `theseus why` listed the slice (2 lines, 177
tokens), five context files, five recalled notes with their reasons, and 30 more left out for the budget. A turn Jev
sends to a small model as a quick question used to carry only its last few exchanges, without recall. That is how, on
2026-10-18, a fresh session asked about that evening's decisions did not bring them back, though recall had found
them. The fix is in v1 patch1, and on the owner's daemon on 2026-10-19 the same kind of question, sent to the small
model as a quick one, was answered from recall: it named the release tag chosen for 1.0.0. One gap remains: `theseus
why` can't show a turn that went to the small model yet.

## 9. Talk to it

**What you see.** Go into a Discord voice channel, type `/join`, and talk. It answers aloud, in the voice of the
persona that is speaking, one sentence at a time, making the next sentence's audio while the current one plays. Talk
over it and it stops. Say "go on" and it starts the cut-off sentence again. In a private call, when a task has a
question, it reads the question to you at the next pause. Say "approve", "stop it" or "pause it", and that counts
exactly as pressing the button would. Speech is paid for out of the same budget as everything else.

**Proof.** Voice has been on in the owner's Discord since October 3. In a four-minute call on October 5 it
transcribed whole sentences, and when the owner cut in and then said "please continue", it carried on. In the v1
check (2026-10-18) it joined, transcribed the owner, and spoke long answers that sounded right, and it counted two
barge-ins, one of them resumed. The same check found what still breaks: in one six-minute call it heard only a few
seconds of what was said, some lines came out clipped, and its short greeting clips. Voice is new and being hardened,
and [the status page](docs/status.md#on-and-still-being-hardened) tracks the fixes. Its tests drive calls with
scripted audio. A barge-in stops playback within 300 ms (`crates/theseus-voice/tests/pipeline.rs`), and a spoken
"approve" settles a task's question as the owner (`crates/theseus-discord/src/runtime/voice/tests_tasks.rs`).

## 10. Separate lines of work, each with its own context

**What you see.** Make a thread for each thing you are working on: `theseus thread new site-redesign --topic web`.
Each thread is a session of its own, with its own card, notes, and context, so two threads can run at once without
mixing them up. You don't have to say which thread a message is for. A router places it: it uses the repos, branches
and paths the thread is working on, which thread you were just in, and Jev's answer. When it is unsure, it asks in one
line, and it offers a new thread rather than making one. The guidance you wrote for a topic comes with every thread on
that topic. Threads can be paused, sealed, moved, merged and split.

**Proof.** Tests show that each thread opens its own session, that two threads' turns overlap in time, and that a
thread you name always wins (`crates/theseus-core/src/rpc/tests_threads.rs`, `tests_thread_route.rs`). On 40 scripted
messages with a scripted Jev, the router puts at least 90% in the right thread and asks about at most 10%. Five
threads have been live on the owner's daemon since 2026-10-18. A benchmark of the router on the real model is built
(`bench/threads/`) but has not been run yet.

## 11. Several characters, one memory

**What you see.** Give it more than one persona: a name, an emoji, a speaking voice, and a file that describes its
character. Bind one to a channel, or start a message with a persona's name, `@name` or `/as name`, and that persona
answers in the same conversation. Personas are characters of one agent, not separate agents. They share one record
and one memory, so what you told one, the others know. `theseus persona import` brings another agent's workspace over
as a persona, and that agent's own past ranks a little higher in recall while its persona is speaking.

**Proof.** Tests check each rule (`crates/theseus-core/src/tests_persona.rs`, `tests_engage.rs`):
- your persona file replaces the default one whole;
- who speaks is decided first by the session's own choice, then the channel's, then the default;
- a task speaks as the persona that started it;
- switching personas changes no byte of the cached prompt, so the cache still holds.

Five personas are set up on the owner's daemon. For a message that names no persona, Jev already picks one, but only in
shadow: its pick is recorded, and the persona that answered last keeps answering until the pick is shown to be right.
The personality exam (`bench/personality/`) is built and not yet published.

<p align="center"><img src="docs/assets/planks.svg" alt="" width="100%"></p>

## How it works

- **One agent over one graph.** Every message, tool call, result, task, and summary is a node in one store.
  Personas are views of the same agent, and tasks and threads are sessions of their own, so nothing is copied
  between agents.
- **Judgment in the loop: Jev.** Small typed judgments sit inside the control loop and steer each message
  ([feature 7](#7-small-fast-judgments-steer-every-message)).
- **Context is compiled, not piled up.** Each prompt is built from the graph, with a manifest of what went in
  ([feature 8](#8-context-built-for-each-turn-not-piled-up)).
- **Linux only, by design.** It is built on the kernel's own machinery (cgroups, namespaces, seccomp, pidfds), with
  no portability layer. A command can run in a sandbox with no network, no credentials, and an empty home.

## The promises it keeps

These hold whatever the model does:

| | Promise | What it means |
|---|---|---|
| 💵 | **Money is a gate.** | Every model call reserves its worst-case cost before it runs. Limits warn you; a runaway stops. |
| 🌐 | **A label you can remove, not a refusal you can't.** | Text that tries to steer it, a name you listed, or anything from Google puts a label on the session, and you remove it with one tap. A steering label stops what acts until you do. |
| 🔑 | **Its own keys never reach the model.** | Every value the vault gave Theseus is replaced by its name in each request to a model and each Discord post; what is stored is never rewritten. |
| 📒 | **You can always ask why.** | Every turn, call, approval, and dollar goes into a tamper-evident ledger. |

## Where it stands

Theseus is at v1 patch2 (version 1.0.2), runs on Linux, and has one daily user. Its weak spots are measured and published
alongside its wins: [the benchmarks](docs/benchmarks/omnibus.md) say where it loses, and
**[the status page](docs/status.md)** says what works today and what comes next, including what is still rough in
Jev's routing, the compiled context, voice, threads and personas. That page changes with every step;
this one doesn't.

**About the name.** Plutarch's ship of Theseus was kept by replacing its planks one at a time as they wore out.
Theseus is built the same way, in small, reviewed, tested steps, by AI agents under one person's direction. Its
[design document](docs/the-ship-of-theseus.md) records every plank that was replaced.

## Set it up

One command, on Linux with systemd, from a checkout. You'll need rustup, Node.js 22 or later (for the cockpit, the
web UI), an Anthropic API key (or a key for another provider that speaks the same API), and
[1Password](https://1password.com/) with a service account, the recommended home for every secret.

```bash
git clone https://github.com/zeroaltitude/theseus && cd theseus
scripts/setup.sh --dry-run                                     # what it would do; it changes nothing
scripts/setup.sh --op-token-file ~/.config/theseus/op-token    # your service account's token, in a mode-600 file
```

It checks the machine, builds the cockpit and the binaries, installs them into `~/.local/bin`, and writes
`/etc/theseus/theseus.toml` with every secret as an `op://` reference, never a value. The first run stops once the
config is written, for you to fill in your references; run it again, and it finishes, starts the daemon as a
systemd user service, and shows its health. It is safe to rerun, and after a `git pull` it installs what changed.
[docs/setup.md](docs/setup.md) walks through each step.

Then talk to it: `theseus ask "Hello! What can you do?"`, `theseus tui`, or the cockpit at <http://127.0.0.1:7433/>.

- **The service, day to day:** `scripts/user-service.sh status`, `logs`, and `restart`
  ([docs/user-service.md](docs/user-service.md)).
- **Connect Discord:** `theseusd example-bindings` prints the bindings file's format.
- **Learn the command line:** `theseus --help`. A good first look at a session is `theseus watch`.

## Documentation

- **[docs/README.md](docs/README.md)**: where to start, and what each document is for.
- **[Status and roadmap](docs/status.md)**: what works today and what comes next.
- **[Benchmarks](docs/benchmarks/omnibus.md)**: every run, its numbers, and what it changed.
- **[The Ship of Theseus](docs/the-ship-of-theseus.md)**: the design document, both specification and record.
- **[Technical overview](docs/technical-overview.md)**: the store, the kernel, the tool loop, the protocol, and the
  command line.
- **[Design documents](docs/design/)** and **[research](docs/research/)**: how each part was designed, and what it
  was measured against.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
