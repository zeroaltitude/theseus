# Theseus: status and roadmap

_Updated 2026-10-02 19:40 MST. Version 0.0.1; the design document is at v0.78._

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
- Approvals from Discord, the terminal, or the web, including "approve, and trust this session". Without an
  `[approval]` section only the owner answers, and a question nobody answers expires at the time its card gives.
- What needs you, the same everywhere: one rule, shown as a pill in the terminal, the web UI, and the cockpit.
  Changes are pushed live to every surface instead of polled. `theseus watch --all` follows every session, and
  `theseus wait` returns the moment a session needs you or settles.
- A terminal UI, `theseus tui`: every session in one sidebar, the one that needs you a key away, its question
  answered inline, and a finished session marked done until you have looked at it.
- herdr panes that show each session's true state: `theseus watch` inside a pane reports it, and
  `theseus herdr sync` gives every session that needs you or is working a pane of its own. A message typed in a
  pane runs on the session's own model.
- `theseus reach`: where a message went, through the sessions that copied it.
- An answer takes effect in one step, so no surface shows a session waiting on a question it no longer has.
  Underneath, several kernel state changes commit as one, and every ledger row, live event, and narrative line of
  a turn comes from one typed fact, so they can't disagree.
- **Sandboxed commands (L1).** A command the agent asks to sandbox runs with no network, no secret, its writes
  discarded, and its own memory and process limits, at notify, unless your own setting for commands asks first. It
  never falls back to the host.
- **A cancel that says how it knows.** A cancel or a stop ends every process of a job, one that detached into a
  session of its own too, and says how that is known, in the terminal (`⏹️ cancelled proc.run …a1b2c3 (verified:
  process tree, 2 processes)`) and on Discord. A sandboxed job's end is verified by its own process namespace or
  its cgroup.
- **Confidentiality labels.** Every new message, result, and answer records who may read it, and a session's
  context holds only what its audience may. In a Discord channel other people can see, what your files, commands,
  and AWS return is left out, each as a one-line placeholder that says why; your DMs, the terminal, and the web UI
  hold everything. `theseus labels` shows a session's audience and each message's label, and `theseus health` says,
  for each channel, whether anything is withheld there.
- **Search over every session.** The index tender, a process of its own beside the daemon, keeps every session in
  a search index (words, exact names, and vectors); `theseus index search` finds a phrase from any of them.
- **AWS reads** on Theseus's own account, once the config binds it: any read of any service, the service catalog,
  who the key is, and S3 listings. Every request is attributed in CloudTrail and recorded, and nothing that writes is
  sent.
- **The daemon as a systemd user service**, the operator's since 2026-10-02 at 14:23: `scripts/user-service.sh`
  checks the machine and installs it, systemd restarts it after a crash, and a crash leaves a file the next start
  reports.
- **Speed at scale.** With 10,000 parked sessions, a cold start answers in about 22 ms and health in under 2 ms,
  and 50 turns at once peak at about 104 MB of memory. Every write goes through one writer thread with group
  commit, so a burst of writes no longer stalls the requests beside it.
- A session that outgrows its model's window drops its earliest turns and retries once, by itself. A job
  that prints past its output cap keeps the end of its output, where builds and tests print their verdict.
- A stop reaches a job even while the job is still starting, and cuts the model's reply mid-stream.
- A cancel answers every call it ends, and a corrupt record degrades only what reads it, with a repair from a
  backup copy.
- Reading the web holds the session's hands until you trust it again.
- A secret broker that hands a program only the credentials you've granted it, never through a shell or a
  program that runs others, and never to a cloned project's git hooks.
- Budgets in dollars, the cockpit and The Narrative, crash recovery, restore from a backup copy, and a systemd
  installer.
- The speed budgets, enforced on every commit, and a plain turn held to 5 durable writes.

## Built, and being wired in

These are built and tested in their own crates, and each is wired into the core by a step on the roadmap:
- **Jev in the loop:** the client and its question packs (M5, in shadow first).
- **Memory and recall:** vector search, weighted fusion, forgetting, and the memory math. The memory exam
  measures each part before it goes live (M6). The index tender runs; recall in turns is next.
- **Sandboxes:** L1 runs commands, and a cancel of one is verified; the egress proxy, which keeps credentials out
  of the sandbox, is being wired in now (M4).
- **AWS hands:** the guardrails and the account's templates. The account comes under Theseus's ownership with
  spending tripwires: $50 a month, $5 a day, and $1 an hour. Its reads are in; its writes, with the guards, are
  next.
- **MCP** in both directions, and the **voice** engine for Discord (M7).
- **The ontology**, and an installer for running Theseus under its own user.

## Under way now

- **Egress for sandboxed commands** (stage C, 18c): a sandboxed command reaches only the hosts it is allowed, and
  a result that connected out counts as outside text.
- **Graduation and the held post** (stage C, 19c): the operator can widen who may read a message, and a reply is
  checked against who can see it again as it is posted.
- **The cockpit's restyle** (a lane): The Ship, a live map of the graph, and a new look for every view.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes beside it.
Each step must pass the full test gate, the speed budgets, a live check, and a written review before the next one
starts.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Done. Review 2 was accepted whole on October 1. Its spine items (C6, C2, and S2) are all in, with its security items (installed on October 2) and the lanes for robustness, security, performance, proof points, and the bench. What it deferred is in the v1.1 roadmap. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Done on October 1: the push, the client library, reach, the terminal UI, and herdr. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials as stand-ins, disclosure labels, integrity, the ontology, the job host | Begun. The kernel transaction (C6), one typed fact per event (C2), and the store's writer thread (S2) have landed. On October 2 its first three rows landed and were installed: L1 for `proc.run` (17b), cancellation verified per backend (18a), and confidentiality labels (19a). Under way: egress (18c), and graduation with the held post (19c). Then credentials as stand-ins (18d), the disclosure simulator (19b), and integrity by labels (20a). The ontology and the installer are built as lanes. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | Begun. Its first row, the bound account and its reads (C1), landed on October 2. Next: its stacks and the writes, with the guards. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | The client and packs are built. |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | Begun. Its first row, the index tender, runs since October 2. Recall in turns is next. |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | The MCP client and server and the voice engine are built. |

**After v1:** [the v1.1 roadmap](design/roadmap-v1.1.md) plans the week that follows, about 60 agent-hours: the
kernel's last frames, visibility, the operator's surfaces, cost accuracy, the codebase's shape, proof and the
turn's edges, and Discord.

**When.** An estimate, updated as steps land: the spine completes around **October 6 or 7, 2026**. v1 follows a
two-week soak in daily use, so it lands around **October 20 or 21**, and v1.1 around **October 28**.

## Recently landed

- **2026-10-02:** confidentiality labels: every new message, result, and answer records who may read it, and a
  session's context holds only what its audience may, with a placeholder for the rest (Part III, Item 61;
  installed at 19:13). Cancellation verified per backend: a cancel or a stop ends every process of a job, a
  detached one too, and says how it knows (Item 60; installed at 18:37).
- **2026-10-02:** L1 for `proc.run`, stage C's first row: a command the model asks to sandbox runs with no
  network, no secret, and its writes discarded, at notify, and never falls back to the host. At the join, a stop
  hook in the user service lets the daemon restart while a sandboxed job runs (Part III, Item 58). The operator's
  own setting for commands, or a "should have asked", now reaches a sandboxed call too (Item 59).
- **2026-10-02:** the daemon as a systemd user service, with a setup script and a how-to (Item 56), and the
  operator's config printed whole from a private overlay on the public template (Item 57). Items 44 to 57 were
  installed at 14:23, when the operator's daemon became the service, Item 58 at 16:38, and Item 59 at 16:57.
- **2026-10-02:** the store's writer thread: one thread owns every write, with group commit, so no request waits
  on the disk on a worker, and a new segment is as durable as its frames (Item 52). One exit for a turn, a corrupt
  record that degrades only what reads it, and a crash file (Item 53). Eleven correctness gaps closed, among them
  a stop that cuts the model's stream and a disk floor that stops running jobs (Item 54). The security gaps left on
  v1's path: no grant to a launcher, no hooks for a granted git, writes outside the roots at their tool's posture,
  questions that expire, and a public template that names no one's vault (Item 55).
- **2026-10-02:** AWS's first row, the bound account and its reads (Item 49), and the index tender, stage F's
  first row (Item 50). The gate takes its shared lock only around its tests and benches, so lanes' compiles no
  longer queue behind each other (Item 51).
- **2026-10-02:** reads by state at scale and the memory target measured: 10,000 parked sessions start in about
  22 ms, and 50 turns at once peak at 104 MB against a 1 GB target (Item 46). Discord proven end to end with no
  person, in every gate (Item 47). The benches measure a turn, an idle daemon, and the binaries' size; the
  toolchain is pinned, builds repeat byte for byte, and the gate holds the code's shape (Item 48).
- **2026-10-02:** one typed fact for each thing that happens (Item 43); Review 2's security items, installed at
  14:23 (Item 44); the v1.1 roadmap (Item 45).
- **2026-10-01:** stage B's last steps: the terminal UI, `theseus tui` (Items 39 and 41), on the CLI's client
  library (Item 36); herdr panes (Item 42); `theseus reach` (Item 38); the kernel transaction (Item 41).
- **2026-10-01:** a stop that lands while a job is starting (Item 37); the repository's guides for agents
  (Item 40); the push, a session past its model's window, and a job's output at its edges (Items 33 to 35); the
  reader rule (Item 32); and Items 21 to 31.
- **2026-09-30 to 2026-10-01:** the parallel lanes for the sandbox, Jev's client and packs, MCP, the index,
  vectors, the memory math, the ontology, the installer, voice, the exam, and AWS's client, guardrails, and
  templates (Items 16 to 20).

## Known limits

- Linux only. One daily user so far.
- The terminal UI loads a session's newest 200 messages.
- The list methods (`session.list` and its kin) still read every record: at 10,000 sessions a listing takes about
  150 ms.
- AWS is reads only until its writes land with the guards.
- A sandboxed command has no network and no secret yet: egress and credentials come with the next steps of
  stage C.
- A Discord channel whose viewers can't be read (the bot's Server Members intent is off) counts as public, so a
  session there leaves out what your files and commands return.
- The web UI doesn't show a cancel's verdict yet; the terminal and Discord do.
