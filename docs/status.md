# Theseus: status and roadmap

_Updated 2026-10-04 00:05 MST. Version 0.0.1; the design document is at v0.81._

This page changes with every step that lands. The [README](../README.md) stays the same and links here. For the
full record of each step (what it built, how it was proven, and where it diverged from the plan), see Part III
of [The Ship of Theseus](the-ship-of-theseus.md). For the whole plan, see [the roadmap](design/roadmap-v2.md), and
for the week after v1, [the v1.1 roadmap](design/roadmap-v1.1.md).

## Working today

As installed on the operator's daemon on October 3 at 23:31.

- Conversations in Discord (DMs and channels), the terminal, and the browser, with Claude and GLM models.
- Built-in tools: files (read, write, edit, patch, search, list), git (diff and log), commands, web search and
  fetch, background tasks, and reminders that repeat (every evening at nine, on weekdays, until a date).
- Tasks with their own budgets that report back, and personas by context file.
- The compiled context with its manifests, caching shared across sessions, and token estimates that start from
  the provider's own counts.
- **Approvals from your private places, by you:** the terminal, the web UI, your DMs, and the Discord channels you
  bind private, including "approve, and trust this session". A question asked in a shared channel comes to your
  DM, and a question nobody answers expires at the time its card gives. A command run from inside a job cannot
  answer, untighten, trust, publish, or bootstrap AWS.
- What needs you, the same everywhere: one rule, shown as a pill in the terminal and the cockpit.
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
  discarded and its own process limit, at notify unless your own setting for commands asks first. It never falls
  back to the host. `theseus health` reports the last sandboxed launch, and `theseusd check` runs a sandbox self-test
  on demand; nothing runs one at the start any more. A daemon run as root refuses sandboxed commands, which would get
  no process limit there.
  - **Egress:** it reaches only the hosts you list (`[sandbox] egress`), through a proxy of its own; a call that
    names other hosts waits for you, and your approval reaches those hosts alone. What it brings back from such an
    approved host counts as outside text, so its session holds it; your listed hosts don't.
  - **Credentials at launch:** it gets a secret only when you grant it to the program it runs, at its start,
    exactly as an ordinary command does, and any approval the secret needs comes before it starts. Your own AWS
    files (`~/.aws`) are hidden from every sandboxed command.
- **The harness-only keys.** Your AWS keys and your model providers' keys are never handed to a job: Theseus's own
  tools use them. `theseusd check` and `theseus health` say so by name, and name any of them you grant after all.
- **A cancel that says how it knows.** A cancel or a stop ends every process of a job, one that detached into a
  session of its own too, and says how that is known, in the terminal (`⏹️ cancelled proc.run …a1b2c3 (verified:
  process tree, 2 processes)`), on Discord, and on the cockpit's boundaries board. A sandboxed job's end is verified
  by its own process namespace.
- **Private and shared places.** Your terminal, the web UI, your DMs, and any Discord channel you bind
  `private = true` get everything; one line beside a guild's id makes every channel you bind in that guild private,
  as the operator's guild is. Every other Discord channel is shared. There Theseus has only its own
  conversation and the tools whose results anyone may read (web search and fetch, wakes, tasks), and files only
  under the trees you declare public. It cannot run programs or use AWS there, and it reads only the context files
  you marked public; a fetch of a private address asks first, and says the page would join a conversation others can
  read. `theseus places` and `theseus health` list each place and its class, and health warns if a channel you bound
  private, outside a trusted guild, can be viewed by someone else.
- **Publish.** To share something from a private place into a shared one, publish it yourself:
  `theseus publish <node or file> --to <place>`, Discord's `/publish`, or the cockpit's publish control. It is
  written there as your message, with your note, and recorded.
- **Search over every session.** The index tender, a process of its own beside the daemon, keeps every session in
  a search index (words, exact names, and vectors); `theseus index search` finds a phrase from any of them.
- **AWS on Theseus's own account.** Reads of any service, the service catalog, who the key is, and S3 listings;
  writes in short-lived role sessions, behind AWS's own guards and the gate's floor; the account's stacks, made by
  the bootstrap and protected by their stack policies; and a $50 monthly budget with AWS's stop at 100 %. The `aws`
  CLI gets a short-lived job session, never the key, in a sandbox too. Every request is attributed in CloudTrail and
  recorded.
- **The cockpit is the web UI**, served at `/` (an old `/cockpit/…` address redirects to the same page). It shows
  everything the Observatory did, which is gone: ledger rows in words, the session deck's questions and live turn,
  every execution, every system's card, and the sandbox's settings. It draws the agent as a fleet at night (places,
  sessions as galleys, tool calls as oars, sandboxed calls shielded, tasks under sail), with brass gauges on live
  health. A ship's log under every page scrubs the Ship, the fleet, the actions, the boundaries board, and the money
  back to any moment. The money river follows every dollar from session to model, the speed wall puts each speed
  promise against its budget, and the build each daemon runs is named in its health and its log.
- **Voice in a Discord voice channel**, with Deepgram: `/join` from a private place, speech both ways, replies spoken
  sentence by sentence with barge-in, and speech counted as spend. On since October 3 at 23:38.
- **The config note holds only what differs.** `theseusd config --sparse` cuts a note to your own values and secret
  references, so new keys and defaults arrive with the build, and the template carries no copy of the prices. A
  secret may also come from the environment or a file (`env:`, `file:`), for containers and CI.
- **The daemon as a systemd user service**, the operator's since 2026-10-02 at 14:23: `scripts/user-service.sh`
  checks the machine and installs it, systemd restarts it after a crash, and a crash leaves a file the next start
  reports.
- **Speed at scale.** With 10,000 parked sessions, a cold start answers in about 22 ms and health in under 1 ms, and
  50 turns at once peak at about 104 MB of memory. Every write goes through one writer thread with group commit, so
  a burst of writes no longer stalls the requests beside it. The polled lists and the ledger read a page through the
  index (an action list in 4.5 ms where it took 147), and a store's open makes nothing durable, so a restart after a
  crash pays two syncs fewer.
- **A job's end wakes its turn.** A quick command returns in about 30 ms, not 50, a waiting job's wrapper sleeps, and
  a turn that runs a tool writes 9 durable frames, not 12.
- **No acknowledged write is lost silently.** Each frame of the log carries how far the log was known synced, so a
  frame that rots after its sync is refused, never cut, and `theseusd restore --repair` takes it from a backup.
- **Benchmarks through Harbor**: `theseus --spawn ask` exits with how its turn ended and stops cleanly on a timeout,
  and `bench/` runs Terminal-Bench and SWE-bench tasks through Theseus. Results are published in
  [benchmarks.md](benchmarks.md), none yet.
- A session that outgrows its model's window drops its earliest turns and retries once, by itself. A job
  that prints past its output cap keeps the end of its output, where builds and tests print their verdict.
- A stop reaches a job even while the job is still starting, and cuts the model's reply mid-stream.
- A cancel answers every call it ends, and a corrupt record degrades only what reads it, with a repair from a
  backup copy.
- Reading the web, or running a program that prints outsiders' text (`gh`, by default), holds the session's hands
  until you trust it again, and so does a session that a command run from it opens or writes to. A repeating
  reminder set in such a session waits for you; a one-off doesn't.
- A secret broker that hands a program, sandboxed or not, only the credentials you've granted it, never through a
  shell or a program that runs others, and never to a cloned project's git hooks.
- Budgets in dollars, the cockpit and The Narrative, crash recovery, restore from a backup copy, and a systemd
  installer.
- The speed budgets, enforced on every commit, and a plain turn held to 5 durable writes, a tool-call turn to 9. A
  sandboxed command's start is held under 25 ms in every join's gate. The gate has one way of taking its shared lock,
  waits for a quiet machine (under three quarters of the cores, at most 2 minutes), and on a machine that stays busy
  its timing budgets get a calibrated allowance, shown in its log and history, instead of failing for noise. An
  install builds only the five binaries it ships.

## Built, and being wired in

These are built and tested in their own crates, and each is wired into the core by a step on the roadmap:
- **Jev in the loop:** the client and its question packs, with `security.v2` and `security.v3`, candidates in shadow
  (M5). Jev's wire-in is built in a cloud session and in review.
- **Memory and recall:** vector search, weighted fusion, forgetting, and the memory math. The memory exam
  measures each part before it goes live (M6). The index tender runs; recall in shadow is built in a cloud session
  and in review.
- **AWS hands:** the account is Theseus's, its reads and writes are in, and its stacks are made. The curated tools
  (C3) joined after the last install; the durability tender and the hands' first part on Lambda and Fargate are built
  in cloud sessions and in review. The $5-a-day and $1-an-hour tripwires come with the hands.
- **MCP** in both directions (M7): its wiring into turns and the daemon's server are built in cloud sessions and in
  review.
- **The LSP client** (`theseus-lsp`): six language servers' diagnostics, navigation, and rename, proven against the
  real servers; its tools come next (L2).
- **The ontology**, whose wire-in is in review, and an installer for running Theseus under its own user.

## Under way now

- **The fourth cloud batch's harvest.** Its sixteen sessions finished on October 3 by 22:31. Three are joined: Jev's
  `security.v3` and the LSP client before the 23:31 install, and AWS's curated tools (C3) on October 4 at 00:00, after
  this page's cut, so the next version records it. The rest are reviewed and joined one at a time: Jev wired in (23a),
  recall in shadow (30a), tasks that set wakes (37b), the ontology wired in (21b), MCP tools in turns
  (36b), the MCP server's wire-in (41b), the hands on Lambda and Fargate, the durability tender, a terminal toolset,
  `theseus judge prove`'s report, five gate flakes, the gate's speed, and the user service's restart limits. No new
  cloud batch starts until the operator lifts his hold (October 3, 21:00).
- **The first full Terminal-Bench run** (B5): 89 tasks, Theseus plain, Theseus with a batching paragraph, and Claude
  Code, two attempts each on Sonnet 5.5, started on October 3 at 23:01 with the operator's OK. Its results go to
  [benchmarks.md](benchmarks.md) when it finishes.
- **With the operator:** his voice test (his sparse config note, pasted at 23:37, turned voice on and lists AWS's
  hosts for sandboxed `aws` commands); whether a trusted guild should answer in every channel, unbound; and
  gliding's redesign (38b). Decided, to build: a permanent delete of an object version in Theseus's bucket asks
  first.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes and cloud
sessions beside it. Each step must pass the full test gate, the speed budgets, a live check, and a written review
before it joins.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Done. Review 2 was accepted whole on October 1. Its spine items (C6, C2, and S2) are all in, with its security items (installed on October 2) and the lanes for robustness, security, performance, proof points, and the bench. What it deferred is in the v1.1 roadmap. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Done on October 1: the push, the client library, reach, the terminal UI, and herdr. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials at launch, private and shared places, integrity, the ontology, the job host | Mostly built. On October 2 and 3 it landed, and was installed: L1 for `proc.run` (17b), cancellation verified per backend (18a), egress (18c), credentials granted at launch, the place rule, integrity's light pieces, and the sandbox's trims; on October 3's evening the trusted guild and 18e, the `aws` grant in a sandbox. Next: the ontology wired in (21b, in review), then its cockpit view (21c). The job host (22b) moves after v1. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | Begun. The bound account and its reads (C1, October 2), and its stacks, role sessions, writes behind the guards, and budget (C2), installed on October 3, with the stacks made by the bootstrap that afternoon. The curated tools (C3) joined on October 4 at 00:00, not yet installed; the durability tender and the hands' first part are built in cloud sessions and in review. Next: restore from S3, after the tender. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | The client and packs are built, with `security.v2` and `security.v3` in shadow. Jev's wire-in (23a) is in review; the five rows it opens start when it joins. |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | Begun. Its first row, the index tender, runs since October 2. Recall in shadow (30a) is in review. |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | Repeating wakes (37a) and voice (rows 77 and 78) landed on October 3. MCP tools in turns (36b), the MCP server's wire-in (41b), and tasks that set wakes (37b) are in review. Startable now, waiting for the cloud hold to lift: the bindings' second format (38a) and the LSP tools (L2). Gliding (38b) waits on the operator's call on a redesign. |

**After v1:** [the v1.1 roadmap](design/roadmap-v1.1.md) plans the week that follows, about 60 agent-hours: the
kernel's last frames, visibility, the operator's surfaces, cost accuracy, the codebase's shape, proof and the
turn's edges, and Discord.

**When.** An estimate, updated as steps land: the spine completes around **October 6 or 7, 2026**. v1 follows a
two-week soak in daily use, so it lands around **October 20 or 21**, and v1.1 around **October 28**. On October 3
the engine's picks (Tier 7) and voice were built in an afternoon, and the cloud's fan-out started the first rows of
M5, M6, and M7 early, beside the spine. This pass did not re-estimate the dates.

## Recently landed

- **2026-10-03, evening:** the cockpit is the web UI, at `/`, and the Observatory is gone (Part III, Item 86). A
  completion another reader took no longer faults a turn, and four timing tests prove by order (Item 87). A job's end
  wakes its turn, each record is written once per frame, and a tool-call turn writes 9 frames (Item 88). A trusted
  guild: every channel bound in it is private (Item 89). A config note of only what differs, and no copy of the prices
  (Item 90). Reads that don't grow with history (Item 91). The log's synced mark: rot past the checkpoint is refused,
  and the store is format 6 (Item 92). Secrets from the environment or a file, a headless run's exit codes, and the
  Harbor adapter (Item 93). The `aws` grant in a sandbox (Item 94). `security.v3` in shadow (Item 95). The LSP client
  (Item 96). All installed at 23:31; the operator's store moved to format 6 at its first write, after a backup.
- **2026-10-03, afternoon:** AWS's bootstrap on the real account, its stacks up with their guards (Item 79). The third
  cloud batch: one exam, `security.v2` in shadow, the cockpit's parity with the Observatory, and rot under the
  checkpoint refused (Item 80). The gate's quiet bar, a private address's card in a shared place, and "Should I have
  asked?" (Item 81). Installed at 16:35. The engine's store picks: one format number, an open that makes nothing
  durable, and a golden of shapes (Item 82). Voice with Deepgram (Item 83). Repeating wakes (Item 84). The engine's
  defences: the config copy acts, approvals from private places, and the CLI's check inside a job (Item 85). Installed
  at 18:52.
- **2026-10-03, midday:** the place rule replaced confidentiality labels: every place is private or shared, and
  you publish into a shared one (Part III, Item 76). Integrity's light pieces: a program that prints outsiders' text
  (`gh`) holds its session, and so does a session a command opens from a held one (Item 74). The sandbox's trims:
  listed hosts don't hold a session, no self-test at the start, no cgroup, and no sandboxed command under a root
  daemon (Item 77). One gate lock mode, with a calibrated allowance on a busy machine (Item 75). AWS's stacks,
  writes, and budget (C2; Item 78). Two cloud sessions steadied ten load-sensitive tests, fixed `op inject`'s lost
  error, and added the gate's sandbox-start bench (Item 73). All installed at 13:47.
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
  (Item 65); the disclosure simulator (Item 66; removed with the labels on October 3). The cockpit's Ship and its time machine, boundaries board, money
  river, and speed wall (Item 64; installed at 21:24 and 22:45).
- **2026-10-02:** egress for sandboxed commands (Item 62) and graduation with the held post (Item 63), installed
  together at 21:08. Graduation is the owner's publish since October 3, and the held post is gone.
- **2026-10-02:** confidentiality labels: every new message, result, and answer records who may read it, and a
  session's context holds only what its audience may, with a placeholder for the rest (Item 61; installed at
  19:13; replaced by the place rule on October 3). Cancellation verified per backend: a cancel or a stop ends every process of a job, a detached one too,
  and says how it knows (Item 60; installed at 18:37).
- **2026-10-02:** L1 for `proc.run`, stage C's first row: a command the model asks to sandbox runs with no
  network, no secret, and its writes discarded, at notify, and never falls back to the host. At the join, a stop
  hook in the user service lets the daemon restart while a sandboxed job runs (Part III, Item 58). The operator's
  own setting for commands, or a "should have asked", now reaches a sandboxed call too (Item 59).
- **2026-10-02:** the daemon as a systemd user service, with a setup script and a how-to (Item 56), and the
  operator's config printed whole from a private overlay on the public template (Item 57; the overlay was replaced by
  the sparse note on October 3). Items 44 to 57 were installed at 14:23, when the operator's daemon became the
  service, Item 58 at 16:38, and Item 59 at 16:57.
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
- With no params, `session.list` and `execution.list` still read every record: about 140 to 180 ms at 10,000
  sessions. The other polled reads page through the index.
- AWS's $5-a-day and $1-an-hour tripwires come with the hands; until then the $50 budget's stop, which lags spend by
  hours, bounds AWS spend.
- A sandboxed command holds a granted secret for its whole run, and can't ask for one it didn't get at its start.
  It has no memory limit, as an ordinary command has none.
- A daemon run as root refuses sandboxed commands, since Linux exempts root from the process limit L1 sets. Run it
  as your own user, as the user service does.
- A sandboxed command with no network and no secret still waits in a session that read outside text.
- Whether anyone else can view a Discord channel you bound private is checked when the binding starts, so a member
  added mid-run is seen at the next start. In a trusted guild it isn't checked at all: your word is the check.
- A store moves to a newer build's format at that build's first write, one way: the install's backup is the only way
  back to an older build.
- Voice's speech prices are Deepgram's published rates, assumed until confirmed.
- On this machine a turn's harness overhead reads well over the README's 5 ms, mostly the disk's fsyncs under WSL,
  in debug builds. A measurement on a release build is still to come.
