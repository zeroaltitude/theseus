# Theseus's killer features

*2026-11-14 · Tabitha/Claude · one page: for each feature, the sentence a developer would repeat, and where it
stands. This replaces the page of 2026-10-09, which answered the same question when most of the list was still a
plan. The measurements are in [Proof points](../proof.md), and how each feature is built is under
[features/](../features/README.md).*

## The answer first

A developer asked the owner: "what's Theseus's killer feature?" In October the honest answer was a list of nine,
most of them plans. A month of daily use has changed the answer.

**The killer feature is what Theseus does between your messages.** You can walk away from it. It keeps the work
going, knows where every piece of it stands, remembers what you told it, asks only when it must, and is a little
better in the morning than it was the night before. Each feature below is one part of that.

The goal behind it has not changed: match or beat the best coding agents at coding and be as pleasant to use, then
be far ahead on memory, personality, speed, scale and tasks at every horizon.

| | Feature | The sentence a developer would repeat | Where it stands |
|---|---|---|---|
| 1 | **Tasks at every horizon** | "I started it on Monday, answered its one question from my phone, and it finished on Wednesday, through two upgrades." | Built, in daily use |
| 2 | **A thread for each line of work** | "Each thing I'm working on has its own thread and its own context, and my message lands in the right one." | Built, in daily use |
| 3 | **One memory** | "It remembered what I decided last week in another channel, and linked the message." | Built; dates are its weak spot |
| 4 | **A self-guided, expanding ontology** | "It worked out for itself who I work with and what I work on, and it files everything under them." | Built, in daily use |
| 5 | **Jev picks the model** | "A thank-you costs a fraction of a cent, a hard refactor gets the big model, and it tells me why." | Built, in daily use |
| 6 | **It asks only when it matters** | "It doesn't nag. It stops for the one thing I couldn't take back." | Built; the undo question is new in 1.0.6 |
| 7 | **It improves itself** | "It found a bug in itself overnight, fixed it, passed its own gate, and told me at breakfast." | Built; one unattended landing so far, and rough |
| 8 | **Fast as a testable feature** | "It's one small daemon that answers in milliseconds, and a change that makes it slower doesn't land." | Built and measured |
| 9 | **Chat is the control surface** | "I run it from Discord and from a voice call. I rarely open a terminal for it." | Built; some owner commands still need a command line |
| 10 | **Many characters, one mind** | "Five personas with their own voices, and they all remember the same things." | Built; the automatic persona pick only records |
| 11 | **You can ask why** | "`theseus why` shows me exactly what went into the prompt, and what was left out." | Built |
| 12 | **Benchmarks in the loop** | "They publish where they lose." | Fast benchmarks on every change that lands; the long ones return at 2.x |
| 13 | **Remote runtimes** | "The daemon on my server, the terminal on my laptop, the index on a third box." | Designed; planned for 3.0.0 |
| 14 | **Working ahead** | "While I read its answer it had already fixed the next red test in a sandbox. One key to use it." | Designed; planned for 3.0.0 |

Where Theseus does **not** win yet is on [Proof points](../proof.md#where-theseus-loses), first on that page.

---

## 1. Tasks at every horizon

**The pitch.** "Do this now", "check back tomorrow", "remind me on Friday", "every morning, the status": one kind
of thing for all of it, durable across restarts and upgrades, and visible wherever you are.

**Today.** A long ask becomes a task with a home of its own. Its question appears on every surface at once, and one
answer clears it everywhere. You can type into a running task and it takes your message at its next step. A
reminder is a wake: at its time it gives the conversation a turn with your note, or only posts the note if that is
all you asked for. A routine holds one pending wake and opens a fresh task each time it fires, so a run's context
never grows. A task with nothing to do is a record on disk, so thousands can wait.

**Rough edges.** The kill-and-resume benchmark is twelve trials, and its intervals are wide.

**More:** [durable tasks](../features/durable-tasks.md).

## 2. A thread for each line of work

**The pitch.** The owner called this the feature that moved him over: separate lines of work that do not bleed into
each other, each with the context it needs and nothing else.

**Today.** A thread holds the repositories, branches and paths of one piece of work. A router places each incoming
message by them and asks in one line when it cannot tell; a thread you name always wins. Two threads' turns can
overlap in time. Recall reaches across threads when it should.

**Rough edges.** Recall across threads is live and has not been measured on a real model.

**More:** [personas and threads](../features/personas-and-threads.md).

## 3. One memory

**The pitch.** One store holds every message with its time and place, and recall points at sources instead of
copying them. So a reply can cite the message it remembers, and your history from other tools can come along on the
first day.

**Today.** Recall runs in front of the model on every turn, with a judged order. `theseus import` brings in history
from Claude Code, Codex and OpenClaw. OpenClaw's is kept current by a nightly sync that can also be run on demand;
the other two are imported again by hand. Every persona, thread and task draws on the same memory.

**Rough edges.** Dates are the weak kind of fact. The vector half of recall has a 250 ms deadline that it does not
always meet on a large store.

**More:** [memory across sessions](../features/memory-across-sessions.md).

## 4. A self-guided, expanding ontology

**The pitch.** Theseus keeps its own map of the people, places and things it meets, grows it without being asked,
and makes it part of indexed memory.

**Today.** The kinds of context are data, not code: a versioned table of twelve kinds, from where you are (a guild,
a channel, a thread, a project, a person) to what it is about (a topic, an event, a resource, a capability, an
expertise, a culture, a persona). Entries form a tree, any entry can carry guidance, and in a private place every
compile walks the entries a session is in and adds their guidance. A note that shares an entry with the
conversation ranks higher in recall. As you talk, each private conversation is filed under the entries it fits, and
a new entry is started when nothing fits, with a notice and an undo. An import's topics are filed with one command.
A nightly sweep merges duplicates, lets a machine-made membership lapse after 30 days unless it is renewed, renames
what has drifted, and creates and names a new kind when the same unlisted answer keeps coming up. A person is one
record with many handles, holding what they do and own. Imported history is sorted into seven books (diary,
encyclopedia, cookbook, procedures, casebook, register, dictionary), read from the memory index. The map routes
context and never grants access.

**Rough edges.** The notice that announces a new entry is hard to read, and a message Theseus wrote to itself has
been filed as if a person wrote it. The books are the import's labels, browsed; books compiled from the whole graph
are the next step.

**More:** [the ontology's design](m4-boundaries.md#28-the-ontologys-first-slice).

## 5. Jev picks the model

**The pitch.** No model menu and no effort slider. A small typed judgment reads each message and chooses the model
and how hard it should think, from one table of guidance that a person can read and review.

**Today.** Jev chooses the model and its effort wherever a model is chosen. A model you name is always honoured.
Every judgment is recorded with its probabilities and replays against the record, so a new version of a question is
measured before it decides anything. If Jev is slow or down, the work goes on without it.

**Rough edges.** It sometimes calls a hard task routine. A turn whose loops keep failing asks again and moves up.

**More:** [Jev in the core](../features/jev-in-the-core.md).

## 6. It asks only when it matters

**The pitch.** Theseus is built for a machine its owner set up and trusts. It judges what content *is* and what an
action *does*, not where either came from, and it does not build a paper wall of permissions over that.

**Today.** It notifies rather than blocks. A sensitive name or something fetched from outside raises a label you
can tap away. Before a tool acts, one question is asked: can this be undone? If there is a safer way to do the same
work, the model is sent back to take it. What is final by its nature is raised to you. Beneath the judgment is one
rule no model can lower: a plain write, edit or patch to a file whose current contents nothing else holds is held.

**Rough edges.** The undo question is new in 1.0.6. Tests hold its behaviour; how well it judges in real use has
not been measured.

**More:** [it trusts its environment](../features/trusted-environment.md).

## 7. It improves itself

**The pitch.** Theseus keeps a ledger of everything it did and closes loops on itself. It acts first and tells you
afterwards.

**Today.** Each night it reads its own record, files what it finds on a backlog of its own, builds a fix in its own
sandbox, passes the same gate as any other change, is reviewed by a different model, and lands it. In the morning
it says what changed, why, and what it cost. A short keel still needs the owner's yes: loosening a test or a
budget, security and permission changes, spending limits, and the off switch.

**Rough edges.** It has landed one change entirely by itself so far. When a build of its own cannot go green it
retries noisily; a limit on attempts and a place for what failed are planned.

**More:** [self-improvement](../features/self-improvement.md), and [the design](rsi-plan.md).

## 8. Fast as a testable feature

**The pitch.** One small Rust daemon, not a process for every terminal. The speed is written down as budgets, and a
change that misses one does not land.

**Today.** Start to answering in 50 ms, a crash and restart in 150 ms, a binary swap under load in 200 ms: each is
a budget checked on every change before it lands. Restarting or upgrading is something you do in the middle of a
sentence.

**Rough edges.** A tool call's own overhead is a few tens of milliseconds, most of it one sync to disk on the
critical path. Durability stays; a budget for those syncs is open work.

**More:** [fast, as a contract](../features/fast-as-a-contract.md).

## 9. Chat is the control surface

**The pitch.** You should rarely need a terminal to run your agent. Discord is a full surface: a lively chat, a
home for every task, and a voice channel.

**Today.** Tool lines stream into the chat, small and grey so they read apart from the answer. Every task has a
post of its own and a board that does not scroll away. In a voice call it listens, answers, and lets you interrupt.
It speaks first when something happens: a finished task, a question, a morning note of what it changed overnight.
Answering a call that waits for your approval is a button or a spoken word, and trusting a project is a slash
command.

**Rough edges.** Some owner commands still need a command line. Moving the rest into chat, as tools the model
chooses to call, is current work.

**More:** [Discord](../discord.md).

## 10. Many characters, one mind

**The pitch.** Several personas, each with a name, an emoji, a voice and a character, over one memory. Switching
between them costs nothing.

**Today.** Start a message with `@name` and that persona answers in the same conversation, with the same memory.
Switching personas changes no byte of the cached prompt. Five personas have been live on the owner's daemon since
2026-10-18.

**Rough edges.** The automatic choice of persona only records what it would have picked; the persona that answered
last keeps answering.

**More:** [personas and threads](../features/personas-and-threads.md).

## 11. You can ask why

**The pitch.** Every request is compiled for its turn and carries a manifest. Nothing about a turn has to be
guessed.

**Today.** `theseus why` prints each block of the prompt, its tokens, where it came from, and what was left out and
why; it also says which model the turn ran on and the reason. Every turn, call, approval and dollar is in a
tamper-evident ledger.

**More:** [compiled context](../features/compiled-context.md).

## 12. Benchmarks in the loop

**The pitch.** Measuring is part of how a change lands, not something saved for a launch.

**Today.** Fast benchmarks run in the gate that every change passes to land, against written budgets, and a missed
budget stops the change. The long benchmarks run on every even major version, on the build that shipped and against
the field: odd majors build, even majors measure. After a run, what it found becomes work aimed at real use;
nothing is tuned to a score.

**Rough edges.** The long benchmarks are parked until 2.x, so today's head-to-head numbers are from the v1 and
1.0.3 builds.

**More:** [Proof points](../proof.md), [the omnibus](../benchmarks/omnibus.md).

## 13. Remote runtimes

**The pitch.** "The daemon runs on my server or in my cloud account, the terminal on my laptop, the index on
another machine, and nothing local got slower."

**Where it stands.** Designed and decided, not built. A paired machine with a valid certificate has the same reach
as the local client; the listener stays local until a machine is paired.

**More:** [the design](remote-runtimes.md), and [the work](https://github.com/zeroaltitude/theseus/issues/1).

## 14. Working ahead

**The pitch.** "While I read its answer or step away, it guesses what I will ask next and does that work in a
throwaway copy. When I come back the diff and the passing tests are there: one key to use it, one to throw it
away."

**Where it stands.** Designed and decided, not built. It runs only when the machine is idle, spends from its own
small budget, and nothing reaches your files without a keypress.

**More:** [the design](speculation.md), and [the work](https://github.com/zeroaltitude/theseus/issues/2).

---

## Also true, and worth a sentence

- **Its keys never reach the model.** Every value the vault gave Theseus is replaced by its name on the way out;
  what is stored is never rewritten.
- **Real tools.** Language servers, AWS on an account of its own, Google Workspace, MCP in both directions, and
  commands that can run in a sandbox with no network, no credentials and an empty home.
- **Money is a gate.** Every model call reserves its worst-case cost before it runs.
