# Theseus: status and roadmap

_Updated 2026-10-02 08:13 MST. Version 0.0.1; the design document is at v0.76._

This page changes with every step that lands. The [README](../README.md) stays the same and links here. For the
full record of each step (what it built, how it was proven, and where it diverged from the plan), see Part III
of [The Ship of Theseus](the-ship-of-theseus.md). For the whole plan, see [the roadmap](design/roadmap-v2.md), and
for the week after v1, [the v1.1 roadmap](design/roadmap-v1.1.md).

## Working today

- Conversations in Discord (DMs and channels), the terminal, and the browser, with Claude and GLM models.
- Built-in tools: files (read, write, edit, patch, search, list), git (diff and log), commands, web search and
  fetch, background tasks, and reminders.
- Tasks with their own budgets that report back, and personas by context file.
- The compiled context with its manifests, caching shared across sessions, and token estimates that start from
  the provider's own counts.
- Approvals from Discord, the terminal, or the web, including "approve, and trust this session".
- What needs you, the same everywhere: one rule, shown as a pill in the terminal, the web UI, and the cockpit.
  Changes are pushed live to every surface instead of polled. `theseus watch --all` follows every session, and
  `theseus wait` returns the moment a session needs you or settles.
- A terminal UI, `theseus tui`: every session in one sidebar, the one that needs you a key away, its question
  answered inline, and a finished session marked done until you have looked at it.
- herdr panes that show each session's true state: `theseus watch` inside a pane reports it, and
  `theseus herdr sync` gives every session that needs you or is working a pane of its own.
- `theseus reach`: where a message went, through the sessions that copied it.
- An answer takes effect in one step, so no surface shows a session waiting on a question it no longer has.
  Underneath, several kernel state changes commit as one, and every ledger row, live event, and narrative line of
  a turn comes from one typed fact, so they can't disagree.
- A session that outgrows its model's window drops its earliest turns and retries once, by itself. A job
  that prints past its output cap keeps the end of its output, where builds and tests print their verdict.
- A stop reaches a job even while the job is still starting.
- Reading the web holds the session's hands until you trust it again.
- A secret broker that hands a program only the credentials you've granted it.
- Budgets in dollars, the cockpit and The Narrative, crash recovery, restore from a backup copy, and a systemd
  installer.
- The speed budgets, enforced on every commit.

## Built, and being wired in

These are built and tested in their own crates, and each is wired into the core by a step on the roadmap:
- **Jev in the loop:** the client and its question packs (M5, in shadow first).
- **Memory and recall:** the index, vector search, weighted fusion, forgetting, and the memory math. The memory
  exam measures each part before it goes live (M6). The index's wire-in is in progress.
- **Sandboxes** for code the agent writes, with an egress proxy that keeps credentials out of the sandbox (M4).
- **AWS hands:** the client, the service catalog, the guardrails, and the account's templates. The account comes
  under Theseus's ownership with spending tripwires: $50 a month, $5 a day, and $1 an hour. The first wire-in, the
  account's reads, is in progress.
- **MCP** in both directions, and the **voice** engine for Discord (M7).
- **The ontology**, and an installer for running Theseus under its own user.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes beside it.
Each step must pass the full test gate, the speed budgets, a live check, and a written review before the next one
starts.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Done. Review 2 was accepted whole on October 1: its security items are reviewed and join next, C6 and C2 have landed (stage C), and lanes carry performance, proof points, robustness, and the bench. What it deferred is in the v1.1 roadmap. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Done on October 1: the push, the client library, reach, the terminal UI, and herdr. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials as stand-ins, disclosure labels, integrity, the ontology, the job host | Begun. The kernel transaction (C6) and one typed fact per event (C2) have landed; the store's writer thread (S2) comes next. The sandbox, egress proxy, ontology, and installer are built as lanes. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | The client, catalog, guardrails, and templates are built. The first wire-in, the account's reads, is in progress in a lane. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | The client and packs are built. |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | The index, vectors, math, and the exam are built. The index's wire-in is in progress in a lane. |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | The MCP client and server and the voice engine are built. |

**After v1:** [the v1.1 roadmap](design/roadmap-v1.1.md) plans the week that follows, about 60 agent-hours: the
kernel's last frames, visibility, the operator's surfaces, cost accuracy, the codebase's shape, proof and the
turn's edges, and Discord.

**When.** An estimate, updated as steps land: the spine completes around **October 6 or 7, 2026**. v1 follows a
two-week soak in daily use, so it lands around **October 20 or 21**, and v1.1 around **October 28**.

## Recently landed

- **2026-10-02:** one typed fact for each thing that happens. Every ledger row, live event, narrative line, and
  span of a turn comes from one type, recorded once, and the ledger's kinds are a registry (Part III, Item 43).
  The v1.1 roadmap: the week after v1, planned (Item 45).
- **2026-10-01:** stage B's last steps. The terminal UI, `theseus tui` (Items 39 and 41), on the CLI's client
  library (Item 36). herdr panes that show each session's state (Item 42). `theseus reach`, the first edge between
  sessions (Item 38). The kernel transaction: several state changes commit as one, and an answer is one, so a
  session never reads "waiting on you" between an answer and the work it resumes (Item 41).
- **2026-10-01:** a stop that lands while a job is starting now stops it, and three rules got their tests
  (Item 37). The repository's guides for agents, `AGENTS.md` and `CLAUDE.md` (Item 40).
- **2026-10-01:** the push: one rule decides what needs you, every surface shows it, each change is pushed as it
  commits, and `session.wait` waits on the daemon's side. A session past its model's window recovers by itself,
  a job past its output cap keeps its end, a channel keeps a turn's order around an approval card, and free disk
  space is on screen (Items 33 to 35).
- **2026-10-01:** the reader rule (Item 32); batch C part 3, an honest token estimate, caching part 2, telemetry
  corrections, the cockpit's second round, and fix batch 2 (Items 21 to 31); the README for newcomers, and the
  design documents in `docs/` (Item 28).
- **2026-09-30 to 2026-10-01:** the parallel lanes for the sandbox, Jev's client and packs, MCP, the index,
  vectors, the memory math, the ontology, the installer, voice, the exam, and AWS's client, guardrails, and
  templates (Items 16 to 20).

## Known limits

- Linux only. One daily user so far.
- The terminal UI loads a session's newest 200 messages.
- A message typed in a herdr pane runs on the daemon's current model profile, not the session's.
- The memory target, 10,000 parked sessions and 50 active ones in under 1 GB, is designed for but not yet measured.
