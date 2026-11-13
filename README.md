<p align="center">
  <picture>
    <source media="(prefers-reduced-motion: reduce)" srcset="docs/assets/theseus-logo.svg">
    <source srcset="docs/assets/theseus-logo-animated.svg" type="image/svg+xml">
    <img src="docs/assets/theseus-logo-animated.gif" alt="Theseus: a Greek ship on an old coin, sailing at night. Most of its planks are worn ivory; a few are new gold ones." width="200">
  </picture>
</p>

<h1 align="center">Theseus</h1>

<p align="center">
  <strong>An AI agent that doesn't forget, doesn't drop what it was doing, and doesn't make you wait.</strong>
</p>

<p align="center">
  <img alt="Version 1.0.5" src="https://img.shields.io/badge/version-1.0.5-d6a548?style=flat-square">
  <img alt="Rust 1.98 or later" src="https://img.shields.io/badge/rust-1.98%2B-13314d?style=flat-square">
  <img alt="Runs on Linux" src="https://img.shields.io/badge/runs%20on-Linux-13314d?style=flat-square">
  <img alt="License: MIT or Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-13314d?style=flat-square">
</p>

<p align="center">
  <a href="#what-it-does-that-the-others-dont">What sets it apart</a> ·
  <a href="#how-the-release-numbers-work">Release numbers</a> ·
  <a href="#set-it-up">Set it up</a> ·
  <a href="docs/features/README.md">The features, one page each</a> ·
  <a href="docs/status.md">Status and roadmap</a> ·
  <a href="docs/benchmarks/omnibus.md">Benchmarks</a> ·
  <a href="docs/spec/README.md">The design document</a>
</p>

---

## What Theseus is

Theseus is a personal AI agent that runs as a daemon on your own Linux machine. You talk to it in Discord, in a
terminal, in your browser, or in a voice call, and it does real work with real tools: it edits code, runs commands,
reads the web, drives AWS and Google Workspace, and keeps long jobs running in the background. It calls Anthropic's
Claude models directly, and any model served through the same API.

What makes it different from the coding agent you already have is what happens *between* your messages. Every
action is written to a log before it is attempted, so a task survives a kill, a reboot and a binary upgrade.
Everything you have ever said to it is one graph, so last week's decision comes back when it matters, with a link
to the message it came from. And it is a 36-MiB Rust daemon, not a process per terminal, so the prompt is ready in
tens of milliseconds instead of half a second.

It is one daemon (`theseusd`), a thin command line (`theseus`), a terminal UI, and a web cockpit the daemon serves
itself. Linux only, by design: it is built on cgroups, namespaces, seccomp and pidfds, with no portability layer.

<p align="center"><img src="docs/assets/planks.svg" alt="" width="100%"></p>

## What it does that the others don't

Nine things. Each says what you see and how we know it works, and links to a page with the build, the measurements,
the rough edges and what is coming. The task system is first because it is the thing we would show you first.

### 1. Durable tasks you can watch, answer and steer from anywhere

Ask for something long and it becomes a task with a place of its own: a Discord forum post with a live card, a
pinned board that moves from *working* to *needs you* to *done*, and the same task in the terminal, the cockpit and
a voice call. When a task has a question it appears on every surface at once, and one answer — a button, a key,
`theseus answer`, or a spoken "approve" — clears it everywhere. Type into a running task and it takes your message
at its next step. Pull the plug and it picks up where it was. A task with nothing to do is a record on disk, not a
process, and a task due later sleeps until then, so you can hold thousands of them.

<p align="center"><img src="docs/assets/cockpit-tasks/01-tasks-page.png" alt="The cockpit's Tasks page: a New task box, one task that needs an answer with its options as buttons, two working tasks (one paused) with their spend against their limits, and the done tasks with their report lines." width="720"></p>

**Proof.** Killed with SIGKILL mid-task and restarted, Theseus resumed 10 of 12 trials with 0 effects done twice;
Claude Code repeated an effect in 4 of 12 (2026-10-18). With 10,000 parked sessions the idle daemon spends 0.37% of
one CPU and 117 MB. [The whole page](docs/features/durable-tasks.md).

### 2. Fast, and it is a contract rather than a boast

The prompt is ready in tens of milliseconds, the model's first words appear as it writes them, and restarting or
upgrading the daemon is something you do mid-sentence.

**Proof.** Against Claude Code on the same stand-in model (2026-10-18): a ready prompt in 39 ms against 498, 10 ms
of CPU a turn against 1,655, 70 MB idle against 360. The speed is written down as budgets — start to answering
50 ms, crash and restart 150 ms, a binary swap under load 200 ms — and **every commit's gate fails the commit when
one slips**. [The whole page](docs/features/fast-as-a-contract.md).

### 3. Jev in the core: small, fast judgments steer every message

Before the model answers, Theseus asks [TypeSafe](https://typesafe.ai)'s Jev a handful of small typed questions:
what kind of message this is, which model should answer and at what effort, which persona speaks, which thread it
belongs in, and what the message carries. A thank-you goes to a small model; a hard refactor to a large one.
`theseus why` says which model a turn ran on and why. If Jev is slow or down, nothing waits on it.

**Proof.** On all 89 tasks of Terminal-Bench 2.0 at high effort (2026-10-26), Jev ran 41 of 267 trials entirely on
Haiku 5.5, and the run solved 78.3%. Every judgment is recorded with its probabilities and replays against the
record, so a new version of a question is measured before it decides anything.
[The whole page](docs/features/jev-in-the-core.md).

### 4. Context compiled for each turn, not piled up

Each request is compiled from the store for that turn: the standing instructions and personas, your context files,
the guidance for the thread's topics, the conversation so far with a summary in place of what no longer fits, the
notes recall found in the order Jev put them, and an attached project's slice at the end. Every request has a
manifest, and `theseus why` prints it: each block, its tokens, where it came from, and what was left out and why.

**Proof.** Tests check the layers, that sessions and threads share their first layers byte for byte so the
provider's cache holds, and that a project's slice survives three compactions. On the owner's daemon a `theseus why`
listed a slice of 177 tokens, five context files, five recalled notes with reasons, and 30 left out for the budget
(2026-10-19). [The whole page](docs/features/compiled-context.md).

### 5. It remembers everything, across sessions and tools

Something you said last week, in another channel, about another project, comes back when it matters, with a link to
the message it came from. Bring your history from Claude Code, Codex or OpenClaw with `theseus import`, and it
knows your past from day one.

**Proof.** Over 600 scripted turns in three sessions, Theseus answered 73.1% of recall probes against 56.9% for Pi
and 50.7% for Claude Code (2026-10-18). Two fresh seeds on the 1.0.3 build scored 68.8% and 77.8%, so about five
points of any one number is the seed (2026-10-26).
[The whole page](docs/features/memory-across-sessions.md).

### 6. It fixes itself, and tells you afterwards

Each night it reads its own record for what went wrong and for work it keeps repeating, fixes the faults, turns a
repeated unit of work into a tool, and in the morning says what changed, why, and what it cost. A change that makes
things worse reverts itself. A short keel still needs your yes: loosening a test or a budget, security and
permission changes, spending limits, and the off switch.

**Proof.** The nightly loop has been on since 2026-10-18, and in 1.0.5 **it landed a change on `main` by itself,
unattended, for the first time**: it mined a real defect out of its own record, built the fix inside its own sandbox
view, passed the project's full gate there (9,879 of 9,879 tests), was reviewed by a model other than the one that
wrote it, and joined under the join lock. The fix was small and real — `fs.edit` had answered an edit whose old text
equals its new text as invalid input, and now answers "No change made" and leaves the file alone. The keel guard
fails any commit range that weakens a test without a signed, quoted yes, Theseus's own commits included, and the
file that defines what a self build may reach is itself in a class that stops a self-made join for the owner's word,
so the loop cannot widen its own view. One thing it does **not** yet have: the two commits of that merge are not
verified by the forge — the content commit is unsigned and the merge is signed by the machine's own key — and whether
a self-made join should carry the verified mark is an open question, not a solved one.
[The whole page](docs/features/self-improvement.md).

### 7. Many characters, one memory; separate lines of work

Give it more than one persona: a name, an emoji, a voice, and a file that describes its character. Start a message
with `@name` and that persona answers, in the same conversation, with the same memory. Make a thread for each thing
you are working on, and a router places each message by the repos, branches and paths the thread holds, asking in
one line when it cannot tell.

**Proof.** Tests check that switching personas changes no byte of the cached prompt, that two threads' turns overlap
in time, and that a thread you name always wins. On 40 scripted messages the router places at least 90% and asks
about at most 10%. Five personas and five threads have been live on the owner's daemon since 2026-10-18.
[The whole page](docs/features/personas-and-threads.md).

### 8. It trusts its environment and keeps a true record

Theseus is built for a machine its owner set up and trusts, so its guards are light and sit at the exits. It
notifies rather than blocks: a self-started task, a long turn, a depth-ten chain of tasks all run, and you hear
about them. The stored record is raw and is never rewritten; secrets are substituted only on the copies that leave
the machine. A sensitive name or a web result raises a label you can tap away, not a refusal you cannot. A model of
China origin sees only what was judged open for that one request, and that floor has no off switch.

**Proof.** Landed in 1.0.3, in twelve reviewed rows: four removable labels on a session read by one switch, and
6,593 lines of the old earned-autonomy machinery deleted because it never fired. The content classifier that raises
the labels was measured at precision 0.956 at the raising bar against the previous version's 0.875, and recall 0.985
against 0.955. [The whole page](docs/features/trusted-environment.md).

### 9. Coding at the level of the field, on its own model

It edits, builds, tests and fixes the way a good coding agent does, with language servers as tools and each edit's
new compiler errors in its result.

**Proof, including where it loses.** On all 89 tasks of Terminal-Bench 2.0 at high effort, the frozen 1.0.3 build
solved 78.3% of its trials (2026-10-26) — level with the harnesses on its own model: Claude Code 75.3%, OpenClaw
74.2%, OpenHands 71.9%. It is **behind** the harnesses on GPT-6 Astra (Codex 85.4%, OpenClaw 82.0%), and on the 30
hardest tasks at top effort it solved 65.6% to Codex's 86.7% (2026-10-27). And it was **expensive**: 1.0.3 cost 2.2
to 2.8 times what the v1 build cost a trial, like for like, because a model refusal switched the rest of a turn to
a fallback model. The cause was named rather than guessed at, and it is narrower now — a refused request is first
made again on the model that refused it, and only a refusal that survives that goes to the fallback. **What the cost
is today is not measured**: the benchmarks are parked until the 2.x line, so this paragraph's numbers are the last
ones that were run and not a claim about the build you would install.
[The whole page](docs/features/coding.md).

<p align="center"><img src="docs/assets/planks.svg" alt="" width="100%"></p>

## How the release numbers work

Theseus numbers releases `MAJOR.MINOR.PATCH`, but the three fields are not doing the usual job, because this
project ships one build to one machine rather than a library to the world. What they mean here:

- **The major number is the milestone.** `1` means *v1*: the feature set the design document's Part I specifies,
  built and running. v1 was cut on 2026-10-18 at a frozen tree, with the benchmarks run against that exact build.
  Nothing has been numbered `2` yet, and the repository does not yet record what would earn it — see "what is not
  decided" below.
- **The patch number is a themed release, not a bug-fix dribble.** Each `1.0.N` is a bundle of reviewed rows,
  joined one at a time, then tagged `release-1.0.N` and built with the `release` profile (fat LTO). A patch release
  has a *subject*, and the status page names it:
  - **1.0.0** — v1 itself: features frozen, benchmarked, tagged.
  - **1.0.1** — Google Workspace as tools: mail, calendar, drive, sheets and slides.
  - **1.0.2** — voice: Deepgram Flux as the default listener, and live learning from the owner's own lines.
  - **1.0.3** — the principles migration: trust, privacy and content rebuilt in twelve rows (feature 8 above).
  - **1.0.4** — Jev chooses the model and its effort on every turn, and the approve lists are retired in favour of
    a notify list and the floor.
  - **1.0.5** — the broad feature release of the 1.x line, and the one where the self loop landed its own change
    on `main` unattended (feature 6 above). 323 of 335 planned lines closed; tagged `release-1.0.5` on 2026-11-12,
    with the full gate green on two hosts (9,881 and 9,884 tests, both exit 0). The twelve lines left open are
    named in the tag rather than discovered later, and move to 1.0.6.
  - **1.0.6** — the release being built now, planned as one GitHub issue per bucket of work, each issue tagged with
    the release it is planned for.
- **The minor number is unused so far.** Every release to date has been `1.0.N`; there has been no `1.1.0`. The
  *roadmap* called "v1.1" is a planning document for the week after v1, and its work landed as patch releases, not
  as a `1.1.0` tag.
- **The version is one number for the whole workspace**, and the store's format is a separate one. Every crate
  carries the release number and `theseus --version` prints it; `MANIFEST_FORMAT` moves whenever a stored record
  changes shape, so a build refuses a store written by a newer one.

**What `2.0.0` and `3.0.0` are: both are decided.** **`2.0.0` is the last bugs and issues of the 1.x line** —
not new capability, and not measurement. **Measuring is the `2.0.x` line**, which is what the 1.0.5 tag means by
*"the benchmarks stay parked. The 2.x line is where measuring begins"*: once 2.0.0 closes out the remaining
bugs, the benchmarking plans are populated and the project runs in a regime of benchmark-and-improve, so the
numbers in this README get run again, against the rivals, on a build worth measuring. **`3.0.0` is the two lines
of work that are designed, decided and unbuilt**: the daemon, the CLI and the index each on its own machine over
mTLS ([remote runtimes](docs/design/remote-runtimes.md)), and working ahead in a sandbox on the likely next ask
([speculation](docs/design/speculation.md)). Both are filed for that release rather than this one. Beside them
the [north-star plan](docs/design/north-star-plan.md) names what "crush it" would mean on memory, personality,
speed, scale and task horizons; it is a direction the numbered releases are measured against, not a release of
its own.
When it is decided, it lands here and on [the status page](docs/status.md).

## How it works

- **One agent over one graph.** Every message, tool call, result, task and summary is a node in one store.
  Personas are views of the same agent, and tasks and threads are sessions of their own, so nothing is copied
  between agents.
- **A durable kernel under the model.** Every action is written before it is attempted and its result when it
  settles, so a restart resumes and nothing runs twice. The write-ahead log of checksummed frames is the truth, and
  every index is a projection rebuilt from it.
- **Judgment in the loop.** Small typed judgments sit inside the control loop and steer each message, recorded with
  their probabilities and outcomes, in shadow before they act.
- **Context is compiled, not piled up.** Each prompt is built from the graph for that turn, with a manifest of what
  went in and what was left out.
- **Append-only.** The record only grows: compaction, supersession and forgetting are new records, not edits. The
  one receipted exception is erasing a payload on request.
- **Linux only, by design.** cgroups, namespaces, seccomp and pidfds, with no portability layer. A command can run
  in a sandbox with no network, no credentials and an empty home.

## The promises it keeps

These hold whatever the model does:

| | Promise | What it means |
|---|---|---|
| 💵 | **Money is a gate.** | Every model call reserves its worst-case cost before it runs. Limits warn you; a runaway stops. |
| 🌐 | **A label you can remove, not a refusal you can't.** | Text that tries to steer it, a name you listed, or anything fetched from outside puts a label on the session, and you remove it with one tap. A steering label stops what acts until you do. |
| 🔑 | **Its own keys are replaced on the way out.** | Every value the vault gave Theseus is replaced by its name in each request to a model and each Discord post; what is stored is never rewritten. A value of a secret the daemon no longer holds, or one a size limit cut in two, is not recognised. |
| 📒 | **You can always ask why.** | Every turn, call, approval and dollar goes into a tamper-evident ledger, and `theseus why` reads it back. |
| 🧱 | **A test does not quietly get weaker.** | Deleting a test, ignoring one, dropping an assertion or raising a budget fails the commit gate unless a signed commit carries the owner's quoted yes. |

## Where it stands

Theseus is at **version 1.0.5**, runs on Linux, and has one daily user: the broad feature release of the 1.x line,
tagged on 2026-11-12 with the full gate green on two hosts. 1.0.6 is being built now.
**[The status page](docs/status.md)** says what works today, what is still rough and what comes next, and
[the benchmarks](docs/benchmarks/omnibus.md) say where it loses — including the places it loses badly. Those pages
change with every step that lands.

**About the name.** Plutarch's ship of Theseus was kept afloat by replacing its planks one at a time as they wore
out, until a philosopher could ask whether it was still the same ship. Theseus is built the same way: in small,
reviewed, tested steps, by AI agents working under one person's direction, each step gated and each one recorded.
Its [design document](docs/spec/README.md) is both the specification and the record of every plank replaced.

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
config is written, for you to fill in your references; run it again and it finishes, starts the daemon as a systemd
user service, and shows its health. It is safe to rerun, and after a `git pull` it installs what changed.
[docs/setup.md](docs/setup.md) walks through each step.

Then talk to it: `theseus ask "Hello! What can you do?"`, `theseus tui`, or the cockpit at
<http://127.0.0.1:7433/>.

- **The service, day to day:** `scripts/user-service.sh status`, `logs` and `restart`
  ([docs/user-service.md](docs/user-service.md)).
- **Connect Discord:** `theseusd example-bindings` prints the bindings file's format.
- **Learn the command line:** `theseus --help`. A good first look at a session is `theseus watch`.

## Documentation

- **[docs/README.md](docs/README.md)**: where to start, and what each document is for.
- **[The features](docs/features/README.md)**: one page per feature above — how it is built, what is measured, what
  is rough, and what is coming.
- **[Status and roadmap](docs/status.md)**: what works today and what comes next.
- **[Benchmarks](docs/benchmarks/omnibus.md)**: every run, its numbers, and what it changed.
- **[The Ship of Theseus](docs/spec/README.md)**: the design document, in chapters — specification, plan and the
  as-built record.
- **[Technical overview](docs/technical-overview.md)**: the store, the kernel, the tool loop, the protocol and the
  command line, with a command to see each part work.
- **[Design documents](docs/design/)** and **[research](docs/research/)**: how each part was designed, and what it
  was measured against.
- **[AGENTS.md](AGENTS.md)**: the guide an AI agent reads before it changes this repository.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
