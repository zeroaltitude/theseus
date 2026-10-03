# Theseus: status and roadmap

_Updated 2026-10-03 04:09 MST. Version 0.0.1; the design document is at v0.79._

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
- **Sandboxed commands (L1).** A command the agent asks to sandbox runs in namespaces of its own, with its writes
  discarded and its own memory and process limits, at notify unless your own setting for commands asks first. It
  never falls back to the host.
  - **Egress:** it reaches only the hosts you list (`[sandbox] egress`), through a proxy of its own; a call that
    names other hosts waits for you, and your approval reaches those hosts alone. What it brings back counts as
    outside text, so its session holds it.
  - **Credentials at launch:** it gets a secret only when you grant it to the program it runs, at its start,
    exactly as an ordinary command does, and any approval the secret needs comes before it starts.
- **The harness-only keys.** Your AWS keys and your model providers' keys are never handed to a job: Theseus's own
  tools use them. `theseusd check` and `theseus health` say so by name, and name any of them you grant after all.
- **A cancel that says how it knows.** A cancel or a stop ends every process of a job, one that detached into a
  session of its own too, and says how that is known, in the terminal (`⏹️ cancelled proc.run …a1b2c3 (verified:
  process tree, 2 processes)`), on Discord, and on the cockpit's boundaries board. A sandboxed job's end is verified
  by its own process namespace or its cgroup.
- **Confidentiality labels.** Every new message, result, and answer records who may read it, and a session's
  context holds only what its audience may. In a Discord channel other people can see, what your files, commands,
  and AWS return is left out, each as a one-line placeholder that says why; your DMs, the terminal, and the web UI
  hold everything. `theseus labels` shows a session's audience and each message's label, and `theseus health` says,
  for each channel, whether anything is withheld there.
- **Graduation and the held post.** You can widen who may read something, `theseus graduate` or Graduate in the
  web UI, always as a new message with your reason, never a relabel. A reply bound for a Discord channel that gained
  a viewer since its turn began waits for your approval, and a loop there that drew on your material streams
  nothing before that check.
- **The disclosure simulator.** `theseus-sim disclosure` drives a whole core through seeded worlds of channels,
  people, files, and tasks, and checks that nothing reaches an audience that may not read it. It found two leaks,
  both fixed, and a short run of it is in every gate.
- **Search over every session.** The index tender, a process of its own beside the daemon, keeps every session in
  a search index (words, exact names, and vectors); `theseus index search` finds a phrase from any of them.
- **AWS reads** on Theseus's own account, once the config binds it: any read of any service, the service catalog,
  who the key is, and S3 listings. Every request is attributed in CloudTrail and recorded, and nothing that writes is
  sent.
- **The cockpit's Ship and its time machine.** The cockpit draws the agent as a fleet at night (places, sessions as
  galleys, tool calls as oars, sandboxed calls shielded, tasks under sail), with brass gauges on live health. A
  ship's log under every page scrubs the Ship, the fleet, the actions, the boundaries board, and the money back to
  any moment. The boundaries board shows the latch, approvals, live sandbox gauges, verdicts, labels, held posts,
  and egress; the money river follows every dollar from session to model; and the speed wall puts each speed
  promise against its budget. The build each daemon runs is named in its health and its log.
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
- A secret broker that hands a program, sandboxed or not, only the credentials you've granted it, never through a
  shell or a program that runs others, and never to a cloned project's git hooks.
- Budgets in dollars, the cockpit and The Narrative, crash recovery, restore from a backup copy, and a systemd
  installer.
- The speed budgets, enforced on every commit, and a plain turn held to 5 durable writes. An install builds only
  the five binaries it ships.

## Built, and being wired in

These are built and tested in their own crates, and each is wired into the core by a step on the roadmap:
- **Jev in the loop:** the client and its question packs (M5, in shadow first).
- **Memory and recall:** vector search, weighted fusion, forgetting, and the memory math. The memory exam
  measures each part before it goes live (M6). The index tender runs; recall in turns is next.
- **AWS hands:** the guardrails and the account's templates. The account comes under Theseus's ownership with
  spending tripwires: $50 a month, $5 a day, and $1 an hour. Its reads are in; its writes, with the guards, wait on
  the operator's go-ahead.
- **MCP** in both directions (M7). The **voice** engine for Discord is built, and parked outside the build until
  the operator chooses speech providers.
- **The ontology**, and an installer for running Theseus under its own user.

## Under way now

- **The simplification review** (with the operator). Under his principle that Theseus runs in a default-trusted
  environment, so it does nothing extraordinary or complex for trust, safety, or provenance, three reviewers ranked
  what could go. Decided and done: the housekeeping (Tier 0), an install that builds only what ships (5.2), and
  credentials at launch (Tier 3). Kept by his choice: the ontology, Jev's security judgment and its ladder,
  isolation (the job host and the separate-user install), the memory compiler, and several vector spaces. Still
  with him: labels (Tier 2 and 1.1), the sandbox's trims (Tier 4), the gate's mode (5.3), the periphery and UI
  (Tier 6), and the engine (Tier 7).
- **Cloud sessions.** Small, separable work runs in Claude cloud sessions beside the spine and joins after a review
  here. The first batch's last session, fixes for seven timing tests, joined at 9a8f537 on October 3 at 04:26.
  Three branches of the second batch, and one change, are parked until the review's open tiers are decided.
- **AWS writes** (stage D's C2): no longer waiting on the credentials work, and waiting on the operator's go-ahead
  for the first writes to his account.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes beside it.
Each step must pass the full test gate, the speed budgets, a live check, and a written review before the next one
starts.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Done. Review 2 was accepted whole on October 1. Its spine items (C6, C2, and S2) are all in, with its security items (installed on October 2) and the lanes for robustness, security, performance, proof points, and the bench. What it deferred is in the v1.1 roadmap. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Done on October 1: the push, the client library, reach, the terminal UI, and herdr. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials at launch, disclosure labels, integrity, the ontology, the job host | More than half built. On October 2 and 3 it landed, and was installed: L1 for `proc.run` (17b), cancellation verified per backend (18a), confidentiality labels (19a), egress (18c), graduation with the held post (19c), the disclosure simulator (19b) and the two leaks it found (19d), and credentials: requested at run time (18d), then granted at launch instead. Credentials as stand-ins are dropped for v1. Next: integrity by labels (20a), the ontology wired in (21b), and the job host (22b), each touching a tier of the simplification review the operator has yet to decide. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | Begun. Its first row, the bound account and its reads (C1), landed on October 2. Next: its stacks and the writes, with the guards (C2), waiting on the operator's go-ahead. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | The client and packs are built. |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | Begun. Its first row, the index tender, runs since October 2. Recall in turns is next. |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | The MCP client and server and the voice engine are built. Voice's two rows leave v1 until the operator chooses speech providers. |

**After v1:** [the v1.1 roadmap](design/roadmap-v1.1.md) plans the week that follows, about 60 agent-hours: the
kernel's last frames, visibility, the operator's surfaces, cost accuracy, the codebase's shape, proof and the
turn's edges, and Discord.

**When.** An estimate, updated as steps land: the spine completes around **October 6 or 7, 2026**. v1 follows a
two-week soak in daily use, so it lands around **October 20 or 21**, and v1.1 around **October 28**. Unchanged on
October 3: stage C moved as planned, and voice's two rows leaving v1 take about two slots off the end. But the
spine's next rows each wait on a tier of the simplification review, and the AWS writes on the operator's
go-ahead, so the date now rests on those answers.

## Recently landed

- **2026-10-03:** credentials granted at launch: a sandboxed job takes its program's grant at its start, as any job
  does, and the run-time credential socket is gone (Part III, Item 71; installed at 03:02). The simplification
  review's housekeeping: voice out of the build, the spec's PDF out of git, CI cut to what a stock runner passes,
  and an install that builds only its five binaries, 18 % less CPU from cold (Item 72; installed at 03:32). The
  second cloud batch: the cockpit's third round, the build named in health, and the root `AGENTS.md` under its size
  rule (Item 70).
- **2026-10-03:** credentials as stand-ins dropped for v1, and the harness-only keys said aloud in `check` and
  health (Item 69). The disclosure simulator's two findings fixed (Item 68). The first cloud batch: L1's contract
  checked on a root VM (which found a root daemon's sandboxed jobs unlimited without a cgroup), every old-layout
  read test on literal bytes, and five telemetry gaps closed (Item 67).
  All installed at 01:19.
- **2026-10-02:** credentials requested at run time, built and installed at 23:15, and removed the next night
  (Item 65); the disclosure simulator (Item 66). The cockpit's Ship and its time machine, boundaries board, money
  river, and speed wall (Item 64; installed at 21:24 and 22:45).
- **2026-10-02:** egress for sandboxed commands (Item 62) and graduation with the held post (Item 63), installed
  together at 21:08.
- **2026-10-02:** confidentiality labels: every new message, result, and answer records who may read it, and a
  session's context holds only what its audience may, with a placeholder for the rest (Item 61; installed at
  19:13). Cancellation verified per backend: a cancel or a stop ends every process of a job, a detached one too,
  and says how it knows (Item 60; installed at 18:37).
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
- A sandboxed command holds a granted secret for its whole run, and can't ask for one it didn't get at its start.
- A daemon run as root without a delegated cgroup gives a sandboxed command no process limit, since Linux exempts
  root from the limit L1 sets. Run it as your own user, as the user service does.
- A Discord channel whose viewers can't be read (the bot's Server Members intent is off) counts as public, so a
  session there leaves out what your files and commands return.
- The web UI doesn't show a cancel's verdict yet; the terminal, Discord, and the cockpit do.
- On this machine a turn's harness overhead reads well over the README's 5 ms, mostly the disk's fsyncs under WSL,
  in debug builds. A measurement on a release build is still to come.
