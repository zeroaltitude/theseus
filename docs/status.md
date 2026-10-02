# Theseus: status and roadmap

_Updated 2026-10-01 19:15 MST. Version 0.0.1; the design document is at v0.75._

This page changes with every step that lands. The [README](../README.md) stays the same and links here. For the
full record of each step (what it built, how it was proven, and where it diverged from the plan), see Part III
of [The Ship of Theseus](the-ship-of-theseus.md). For the whole plan, see [the roadmap](design/roadmap-v2.md).

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
- A session that outgrows its model's window drops its earliest turns and retries once, by itself. A job
  that prints past its output cap keeps the end of its output, where builds and tests print their verdict.
- Reading the web holds the session's hands until you trust it again.
- A secret broker that hands a program only the credentials you've granted it.
- Budgets in dollars, the cockpit and The Narrative, crash recovery, restore from a backup copy, and a systemd
  installer.
- The speed budgets, enforced on every commit.

## Built, and being wired in

These are built and tested in their own crates, and each is wired into the core by a step on the roadmap:
- **Jev in the loop:** the client and its question packs (M5, in shadow first).
- **Memory and recall:** the index, vector search, weighted fusion, forgetting, and the memory math. The memory
  exam measures each part before it goes live (M6).
- **Sandboxes** for code the agent writes, with an egress proxy that keeps credentials out of the sandbox (M4).
- **AWS hands:** the client, the service catalog, the guardrails, and the account's templates. The account comes
  under Theseus's ownership with spending tripwires: $50 a month, $5 a day, and $1 an hour.
- **MCP** in both directions, and the **voice** engine for Discord (M7).
- **The ontology**, and an installer for running Theseus under its own user.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes beside it.
Each step must pass the full test gate, the speed budgets, a live check, and a written review before the next one
starts.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Fix batches 1 and 2, the pilot, the cuts, and the reader rule are done. Review 2's remaining proposals are next, once they're chosen. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Telemetry, caching part 2, and the push (attention, live updates, `session.wait`) are done. The CLI's client library is in progress; the terminal UI and herdr build on it. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials as stand-ins, disclosure labels, integrity, the ontology, the job host | The sandbox, egress proxy, ontology, and installer are built as lanes. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | The client, catalog, guardrails, and templates are built. It follows C. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | The client and packs are built. |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | The index, vectors, math, and the exam are built. |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | The MCP client and server and the voice engine are built. |

**When.** An estimate, updated as steps land: the spine completes around **October 6 or 7, 2026**. v1 follows a
two-week soak in daily use, so it lands around **October 20 or 21**.

## Recently landed

- **2026-10-01:** the push. One rule decides what needs you, and every surface shows it. Each change is pushed
  as it commits, `session.wait` waits on the daemon's side, and a client that falls behind is told what it
  missed instead of growing the daemon's memory (Part III, Item 33).
- **2026-10-01:** a session past its model's window recovers by itself, a job past its output cap keeps its
  end, and a stopped job's raw output goes with its result (Item 34). A stop that lands while a job's
  process is starting up is no longer lost, a channel keeps a turn's order around an approval card, and free disk space is on
  screen (Item 35).
- **2026-10-01:** the reader rule: a gate test that fails anything declared without a reader (a crate, a protocol
  method, a notification, an edge kind, or a label). Each crate built ahead of its reader names the roadmap step
  that wires it in (Part III, Item 32).
- **2026-10-01:** batch C part 3: the CLI's commands split, one typed definition per wire shape, and the web apps'
  types generated from the Rust ones. Also an honest token estimate, caching part 2, telemetry corrections, the
  cockpit's second round, and fix batch 2: output caps, verified stops, and sweeping old job output (Part III,
  Items 21 to 31).
- **2026-10-01:** the README for newcomers, and the design documents and research in `docs/` (Item 28).
- **2026-09-30 to 2026-10-01:** the parallel lanes for the sandbox, Jev's client and packs, MCP, the index,
  vectors, the memory math, the ontology, the installer, voice, the exam, and AWS's client, guardrails, and
  templates (Items 16 to 20).

## Known limits

- Linux only. One daily user so far.
- A stop that lands earlier, while a job still waits for its secrets before its process starts, can miss the
  job. A fix is in progress.
- The memory target, 10,000 parked sessions and 50 active ones in under 1 GB, is designed for but not yet measured.
