# Theseus: status and roadmap

_Updated 2026-10-07 03:52 MST. Version 0.0.1; the design document is at v0.84._

**The picture:** [the development timeline](assets/timeline/theseus-timeline.png), from conception to now and what is ahead ([SVG](assets/timeline/theseus-timeline.svg)).

**The north star:** [the plan](design/north-star-plan.md) to match or beat Claude Code at coding and delight, then crush it on memory, personality, speed, scale and task horizons ([PDF](design/north-star-plan.pdf)).

**Self-improvement (RSI):** [the plan](design/rsi-plan.md) for Theseus to build its own code, yardsticks and backlog: act first and tell the owner after, inside a hard keel that still needs his yes; the machinery built now and off until the benchmark rerun ([PDF](design/rsi-plan.pdf)).

**The killer features:** [one page](design/killer-features.md) with a short section per feature (speed, self-improvement, parallel tool calls, task horizons, memory, personality, remote runtimes, speculation, Discord), each linking its plan ([PDF](design/killer-features.pdf)).

**Remote runtimes:** [the design](design/remote-runtimes.md) for the CLI, the daemon and the index each on its own machine, over TLS 1.3 with mTLS, off by default; decided, and waiting for the benchmark rerun ([PDF](design/remote-runtimes.pdf)).

**Speculative activity:** [the design](design/speculation.md) for Theseus to guess the likely next ask, work ahead in a sandbox while the machine is idle, and offer the result, never applied without Use; decided, and waiting for the benchmark rerun ([PDF](design/speculation.pdf)).

This page changes with every step that lands. The [README](../README.md) stays the same and links here. For the
full record of each step (what it built, how it was proven, and where it diverged from the plan), see Part III
of [The Ship of Theseus](the-ship-of-theseus.md). For the whole plan, see [the roadmap](design/roadmap-v2.md), and
for the week after v1, [the v1.1 roadmap](design/roadmap-v1.1.md).

## Working today

As installed on the operator's daemon on October 6 at 22:13 (install #9; the store at format 23). Install #5
(October 5, 16:48) put the v1 build there, install #6 (October 6, 10:12) turned the S3 backup on, and the import of
his earlier history followed install #8 (17:22). What installs #7 to #9 added is listed after this list, and what has
joined since, for install #10, after that. Theseus has been feature complete (v1-beta) since October 6 at 22:04.

- Conversations in Discord (DMs and channels), the terminal, and the browser, with Claude and GLM models.
- **The model for each message, picked by Jev.** `route.v1` reads each message's interaction mode as it arrives and
  picks its model from the operator's routing table, waiting at most 300 ms beside the first compile (his setting; 200
  by default): a greeting or a thank-you goes to Haiku, chat stays on the session's own model, deep coding goes to Opus
  5.5, the most sophisticated work to Fable, and routine coding to GLM-5.3. A routed session keeps its
  move only while `route.v1` acts, and `profile.use` moves it back (Items 139 and
  150); it keeps the base it was moved from, across a restart, and follows a change of that base, and each loop
  records one compile and one start (Item 181). A request the model refuses is retried once on its fallback, Sonnet 5 for Sonnet 5.5
  (and, in his config, Opus 5 for Opus 5.5), and every surface says so (Item 154).
- **Fast replies.** A greeting's reply shows on Discord as its model streams, the verdicts a turn waits on land
  inside their wait, and a trivial message routes to the small model from a 0.4 bar (Item 158). A trivial
  detour sends no recall and writes none (Item 161).
- **Jev in the loop.** Every judgment pack runs beside the turn in shadow: the loop's stopping point, the gate's
  safety call (`security.v1` and `security.v3`), each message's class and role, continuation, and a private
  conversation's topics. Each judgment is marked in the trace, shown by `theseus judge show`, and counted in the
  cockpit's Judgment section (Items 105, 119, 120,
  121, 122 and 126). Three are live:
  - `route.v1`, above;
  - `rerank.v1`: Jev's order of recall goes in front of the model, waited for at most 300 ms (his setting; 200 by
    default), with a breaker of its own (Items 128 and 141);
  - `security.v3`'s notices: after a call the gate let through that Jev is at least 90 % sure was risky, a notice to
    the owner's DM alone, never on the call's path, with `security.v1`'s brake (Item 142).

  The ladder holds each pack's mode, promotes, rolls back and adopts (Item 145). The owner's labels,
  from the cockpit's buttons, feed the learning ledger's nightly report with its frozen holdouts (Item
  129), and the learning loop rewrites a pack version's wording from them, nightly, under a
  $2.00-a-day limit (Item 164). Replay, audit and backfill try a pack version against the record
  (Item 144), and `theseus judge prove` reports lane L3's exit from the daemon's own ledger (Items
  101 and 179). Replay and the loop share one rule for whether a lean was right, the audit runs off its low
  thread, and the prove leaves out a learned loop version's tasks and names its placement (Item
  203).
- **Memory in every turn.** Recall runs live in front of the model, place-safe (a shared place recalls only its own
  sessions; a private one draws on every session), on the `baseline` arm, with `rerank.v1`'s order (Items
  99, 112 and 160). The memory pass after each turn labels
  what was said and links duplicates and corrections, with `memory.v1` and `attribution.v1` in shadow (Item
  136). A long session is compacted into summaries with a floor, on GLM-5.3 Flash by his routing
  table (the session's own model by default), and its
  context is assembled as recall, then the summary, then the tail (Item 131); the system header
  says how a request was assembled (Item 156). A bounded node cache means a turn decodes only what
  it reads (Item 162). Retention, activation and consolidation's cited syntheses are built as arms, each
  off unless `[memory] arm` names it (Items 157, 159 and 160), and
  measured by the four-arm exam (Item 124). Consolidation's nightly writer is on again, at $0.50 a
  day, since a synthesis that opens with a heading is checked without it (Item 188), and
  `+activation`'s warm build yields to a busy machine and never holds a stop (Item 187).
- **Situations, checked before anything is sent** (35a, enforced from the first day). Each turn's compiled
  request is checked against its situation: a turn whose compiled set does not close fails before anything is
  sent, with a `context.unadmitted` row and words that name the piece. A recalled item's header names its place
  and the reply's model, and a volatile value says it is unverified (Item 183).
- **The operator's earlier history, imported.** `theseus import openclaw|list|erase` brings the episodes of
  his earlier agent's history in as imported sessions: closed, private, left out of the session list, each
  message at its own time with an `import` origin, idempotent, and erasable by tag (Item
  201). His went in right after install #8 (October 6, 17:26 to 17:28): 21,779 episodes, none rejected. The
  session lists step over them by key, so a list stays in a few milliseconds (Item 219), and
  their vectors are embedded in the background over days. `theseus import topics --tag T` makes the
  episodes' topic labels the ontology's topics, a tree from their slash paths, and each imported session's
  memberships (origin `import`, at most three), from the stored labels; the tag's erase takes them back
  (theseus-anh3).
- **Tasks as records.** A task has a record of three layers and a place in a graph, edited by compare-and-swap through
  the task tools, its first layer asked of the owner only for tasks the owner created (Items 125 and
  146). A task is created with its arrangement (references, a refusal, a fidelity check; Item
  113), sets one-shot wakes and parks on them (Item 98), can be checked by a task
  independent of it by its claim (Item 132), and is claimed under a 30-minute lease. The task board
  is in Discord (`/tasks`) and the cockpit's task graph (Item 163).
- **MCP both ways.** MCP servers' tools in turns (`[mcp.servers]`, private places only, their results outside text;
  Item 106), their prompts (`/prompt`, `theseus prompt`, the cockpit's picker; Item
  115), and Theseus's own MCP server on loopback, off unless configured (Item 110).
  **Self-proposed extensions:** `extend.propose` freezes a stdio MCP server, tries it in L1 and asks the owner; an acked
  one loads, restarts with the daemon, and can be revoked (`/extensions`, the cockpit's Extensions card; Items
  118 and 133).
- **Tools for code.** Language servers as tools (`lsp.*`: diagnostics, navigation, rename; Item 114),
  an edit's result carrying its files' new errors (Item 127), rust-analyzer's compiler errors
  after a save (Item 153), and servers that watch their own files (Item 151). Terminals
  as tools: `term.open`, `term.send`, `term.read` and `term.close` (Item 109). `proc.run` takes up
  to 16 steps as one batch, judged as the strictest and run in turn to the first failure, and `fs.patch` recounts a
  hunk's header from its body (Item 175).
- **The files people give it:** PDFs from every surface (natively for Claude, as text for GLM), Word documents,
  spreadsheets, decks, notebooks, EPUB, archives, and audio and video, transcribed when the model reads them (Item
  155).
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
  Changes are pushed live to every surface instead of polled, each notification serialized once for every watcher
  (Item 172). `theseus watch --all` follows every session, and `theseus wait` returns the moment a
  session needs you or settles. Every queue says why, and a late result's wake rides its turn's end, so a
  settled wait returns after the late result is read (Item 194); `theseus watch` ends a
  reply's open line when the daemon dies (Item 191).
- A terminal UI, `theseus tui`: every session in one sidebar, the one that needs you a key away, its question
  answered inline, and a finished session marked done until you have looked at it.
- herdr panes that show each session's true state: `theseus watch` inside a pane reports it, and
  `theseus herdr sync` gives every session that needs you or is working a pane of its own. A message typed in a
  pane runs on the session's own model.
- `theseus reach`: where a message went, through the sessions that copied it, from a node's short id; and
  `theseus history` pages a session both ways (Item 193).
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
- **Jobs on Linux's own terms** (Linux only, by design). A job's wrapper and its command start without a fork, so a
  job's start no longer grows with the daemon's size (Item 149). Where the daemon's cgroup is delegated, as
  the user service's is, each ordinary job runs in a cgroup of its own, capped at 4,096 processes and threads and
  stopped exactly (Item 152). A job's completion takes one sync, and the background passes wait while the
  machine is busy (Item 151). A job result that arrives during a restart's startup is taken at once, not
  a minute later (Item 166). Health's cancels count a cgroup-stopped job as `l0`, and telemetry counts each
  cancel as health does, with the index tender's lag and size (Item 190).
- **The harness-only keys.** Your AWS keys and your model providers' keys are never handed to a job: Theseus's own
  tools use them. `theseusd check` and `theseus health` say so by name, and name any of them you grant after all.
- **A cancel that says how it knows.** A cancel or a stop ends every process of a job, one that detached into a
  session of its own too, and says how that is known, in the terminal (`⏹️ cancelled proc.run …a1b2c3 (verified:
  process tree, 2 processes)`), on Discord, and on the cockpit's boundaries board. A sandboxed job's end is verified
  by its own process namespace, and a job's deadline stop waits up to 2 s to verify its whole tree died (Item
  173).
- **Private and shared places.** Your terminal, the web UI, your DMs, and any Discord channel you bind
  `private = true` get everything; a guild you trust makes every channel you bind in it private. Bindings' second
  format holds many guilds, with trust per guild and a ceiling per place (Item 117). Every other
  Discord channel is shared. There Theseus has only its own conversation and the tools whose results anyone may read
  (web search and fetch, wakes, tasks), and files only under the trees you declare public. It cannot run programs or
  use AWS there, and it reads only the context files you marked public; a fetch of a private address asks first, and
  says the page would join a conversation others can read. `theseus places` and `theseus health` list each place and
  its class, and health warns if a channel you bound private, outside a trusted guild, can be viewed by someone else.
- **Publish, and gliding.** To share something from a private place into a shared one, publish it yourself:
  `theseus publish <node or file> --to <place>`, Discord's `/publish`, or the cockpit's publish control. It is
  written there as your message, with your note, and recorded. The agent's own `channel.post` and `channel.read` go
  into a private place always, and out of one, or between two shared places, only once you say (Item
  140).
- **Search over every session.** The index tender, a process of its own beside the daemon, keeps every session in
  a search index (words, exact names, and vectors); `theseus index search` and `theseus memory search` find a phrase
  from any of them.
- **AWS on Theseus's own account.** Reads of any service, the service catalog, who the key is, and S3 listings;
  writes in short-lived role sessions, behind AWS's own guards and the gate's floor; the account's stacks, made by
  the bootstrap and protected by their stack policies; and a $50 monthly budget with AWS's stop at 100 %. The `aws`
  CLI gets a short-lived job session, never the key, in a sandbox too. Every request is attributed in CloudTrail and
  recorded, and counted and timed in telemetry by service, operation and outcome (Item 178).
  - **The curated tools:** S3's get and put, Logs Insights and tail, CloudTrail, and the inventory with a reaper that
    reports; a secret-bearing read returns a handle on the secrets board, never the value; AWS's text is outside text;
    and a daily CloudTrail cross-check (Item 97).
  - **The hour's and the day's lines** are alerts with authority: each hand's worst case is reserved against them,
    and at ten times a line new AWS actions are refused, with words, until the period turns or the owner raises the
    line (runaway mode; Items 116, 147 and 174).
  - `[policy.aws]` keys are checked against the catalog after serving, and an unknown one shows in health (Item
    174).
  - **The S3 backup is on** (since install #6, October 6; the operator's yes of October 5): the durability tender
    ships the store's synced frames and blobs to his bucket and table, each session listing only its own prefix,
    and health and the cockpit's AWS card say how far it has shipped; a restore drill gave back the same sessions
    and nodes (Item 198).
- **Budgets and policy, explained.** `theseus budgets` lists every budget with what is spent and reserved, and
  `policy.explain` says which rule of the gate's order decides a call (Item 130); the cockpit has
  Budgets, Ledger and Policy tabs (Item 165). A provider call in flight at a crash or a stop is booked as
  spent at its reservation when the restart marks it unknown, so a budget's reset frees it (Item
  197).
- **The cockpit is the web UI**, served at `/` (an old `/cockpit/…` address redirects to the same page). It shows
  everything the Observatory did, which is gone: ledger rows in words, the session deck's questions and live turn,
  every execution, every system's card, and the sandbox's settings. It draws the agent as a fleet at night (places,
  sessions as galleys, tool calls as oars, sandboxed calls shielded, tasks under sail), with brass gauges on live
  health; in Live mode its sea rolls gently, and Calm keeps it still (Item 143). A ship's log under
  every page scrubs the Ship, the fleet, the actions, the boundaries board, and the money back to any moment. The money
  river follows every dollar from session to model, the speed wall puts each speed promise against its budget, and the
  build each daemon runs is named in its health, its log and its Systems card (Item 177). New since
  October 3: the Ontology view and a session's memberships (Item 111), the Judgment section, the
  task graph, the Budgets, Ledger and Policy tabs, the Extensions card, and a plain page where WebGL is missing (Item
  135). New since October 7: the Context page, what Theseus knows and what a turn sees (theseus-7n3e): the imported
  episodes filtered, counted by facet and opened to their messages (`import.sessions`), their topics and book hints,
  asking the index, and a turn's request part by part with its tokens and why each note was recalled
  (`context.explain`), from any ship or bench.
- **The ontology, wired in:** categories, guidance and memberships as records, which every compile walks (Item
  100); `categorize.v1` proposes a private conversation's topics, in shadow.
- **Voice in a Discord voice channel**, with Deepgram: `/join` from a private place, speech both ways, replies spoken
  sentence by sentence with barge-in, and speech counted as spend. On since October 3 at 23:38. Since October 6 a
  barge-in holds the reply until the words over it decide: a wordless sound, an echo, a backchannel or "go on"
  resumes the cut sentence from its held audio, words cut it, and a reply waits for the floor (Item
  202).
- **Set it up in one command:** `scripts/setup.sh` checks the machine, builds the cockpit and the binaries, installs
  them, and writes the default config, `/etc/theseus/theseus.toml`, which `theseusd` reads by default (Item
  138). The config holds only what differs: `theseusd config --sparse` cuts one to your own values and
  secret references, so new keys and defaults arrive with the build. A secret may also come from the environment or a
  file (`env:`, `file:`), for containers and CI.
- **The daemon as a systemd user service**, the operator's since 2026-10-02 at 14:23: `scripts/user-service.sh`
  checks the machine and installs it, systemd restarts it after a crash (a bounded loop; Item
  102), and a crash leaves a file the next start reports.
- **Health says what it sees:** the web UI's refusals, the 1-hour cache's writes (priced in `theseus catalog`), the
  memory block, the node cache, and free space crossing a line, which writes one row and tells the operator (Item
  177). A failed provider call's time and span carry its error's class (Item
  178). The durability tender's state, loud when failing or stopped (Item 198).
- **Speed at scale.** With 10,000 parked sessions, a cold start answers in about 22 ms and health in under 1 ms, and
  50 turns at once peak at about 104 MB of memory. Every write goes through one writer thread with group commit, so
  a burst of writes no longer stalls the requests beside it. The polled lists and the ledger read a page through the
  index (an action list in 4.5 ms where it took 147), and a store's open makes nothing durable, so a restart after a
  crash pays two syncs fewer, and none for the log's directory when a mark vouches for its segment (Item
  196). A turn needs 576 KiB of stack where it needed 1,856 (Item 192).
- **A job's end wakes its turn.** A quick command returns in about 30 ms, not 50, a waiting job's wrapper sleeps, and
  a turn that runs a tool writes 9 durable frames, not 12.
- **No acknowledged write is lost silently.** Each frame of the log carries how far the log was known synced, so a
  frame that rots after its sync is refused, never cut, and `theseusd restore --repair` takes it from a backup. A
  failed sync cuts its frames back to the last good sync, so a write told "failed" stays failed after a restart (Item
  176).
- **Benchmarks through Harbor**: `theseus --spawn ask` exits with how its turn ended and stops cleanly on a timeout,
  and `bench/` runs Terminal-Bench and SWE-bench tasks through Theseus. The first full Terminal-Bench 2.0 run, three
  configurations, is in [benchmarks.md](benchmarks.md) (Item 148), and the bench program has three more
  harnesses: efficiency, concurrency, and incidental recall (Items 167, 168 and
  169), measured end to end and held to a recall plan's overhead since October 5 (Items
  184 to 186, 199 and
  200). The static musl build compiles again (Item 195).
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
  install builds only the five binaries it ships. The tests that failed under load wait on what they test, not on
  the clock (Item 189).

## New at installs #7 to #9 (October 6, 13:03, 17:22 and 22:13)

At install #7:
- **A secret printed JSON-escaped** (any of its characters escaped: serde's and Python's forms, `\/`, `\u`
  escapes, Rust's `{:?}`) is withheld before the model and the WAL (Item 204).
- **The gate's layers held by turns**, and `theseus policy explain` names L3 where an edit's language server raised
  the posture (Item 205).
- **A declined call ends its batch's waits:** the later calls that would ask are answered "Not run" with no card,
  and an approved call runs even when its id repeats an earlier answered call's (Item 206).
- **Discord's bindings, read while the daemon runs:** a place added or removed binds or unbinds within 2 s, with no
  restart; a new guild's invite check and voice still wait for one (Item 207).
- **The judgment log pages from the newest**, the notices' brake reads today's rows, the learning rules skip closed
  windows, and inbound and compile judgments record their class (Items 208 and
  209).
- **A search during the paced warm build answers `building` at once**, and a summary's call is reserved on the
  estimate's upper bound (Item 210); the daemon's own telemetry path is held by tests (Item
  211).
- **A routed turn's trace root and metrics name the model it ran on** (Item 213); the core's
  waits are proved (Item 212); a `--stdio` daemon's stop closes its store first, the lifecycle bench
  times a cancel's round trip, and the flaky list is empty (Item 214).
- **In a voice call, an answer that repeats its question's words and a "yes" on a question's last word are turns
  again:** an echo must be a near-whole, in-order copy (Item 215).

At install #8 (17:22), with the import of the operator's earlier history after it:

- **In a voice call the next turn is told what was heard, cut and never said**, replies are shaped for speech (no
  Markdown, table or code block is read aloud), and a failed voice turn says so aloud (Item 216).
- **No person's name in the tree:** the owner is "the owner", or `zeroaltitude` where an account is meant, and the
  assistant is Tabitha/Claude (Item 217).
- **Pi, the benchmarks' fourth arm,** measured as the others are, and a report for every benchmark run under
  [docs/benchmarks/](benchmarks/README.md), eighteen from the retrospective (Items 218 and
  220).
- **The session lists read live sessions by key** and never decode an imported record (Item 219).
- **The judge's frames are written between turns,** and a clean stop writes every settled judgment before the store
  closes; `bench turn --judge` measures the judge's cost on a turn's path (Item 221).

At install #9 (22:13):

- **A learned version's promotion says what stands in the moved one's place** and how to roll it back, and the learning
  cut never runs ahead of the judgments it cuts (Item 222).
- **In Discord, a background job's late result edits its one tool line** and its one notice card instead of posting
  again, and the gateway and the bindings watch end at the daemon's stop (Items 223 and
  224).
- **More of a secret's encodings are withheld:** escaped twice as JSON, in YAML's and Python `repr`'s escapes, and as
  base64 broken by escaped line breaks (Item 226). Five tests that failed under load are fixed at
  their causes (Item 225).
- **The recall benchmark refuses a daemon more than 50 tokens off its plan** (Item 227).
- **A failed routed turn counts under the model it ran on** (Item 228), and a turn's own provider call is
  reserved on the estimate's upper bound (Item 229).
- **A voice call that joins but never hears** is noticed, rejoined once and told, and a voice turn that waits on you
  says so (Item 230). A held reply decides at most 3 s after the last words over it, and a floor held by
  a wordless sound frees at 8 s (Item 231).

## Joined since, on install #10's list

Install #10 waits for a verdict on a job-wrapper test that failed once in a gate (theseus-sdgl). It carries:
- **Every credential mint and stored secret in AWS's catalog is held to a reviewed decision,** and 27 operations that
  read as reads before now run as writes, with a notice (Item 232).
- **Health counts the operator's own sessions apart** from the imported and erased ones ("sessions N · imported M ·
  erased K"), in the CLI and the cockpit too (Item 233), and the benchmarks' Harbor arms are
  comparable: medium effort on every arm, Claude Code pinned, a timed-out agent stopped before it is scored (Item
  234).
- **A running job's cancel settles in two synced frames instead of five**, its round trip in the gate about 30 ms at
  the median where it was about 110 (Item 235), and a stopping daemon begins no retry or model call
  (Item 236).
- **route.v1 asks Jev in a request of its own**, on a connection kept warm, and says when its verdict came late (Item
  237).
- **The cockpit's phase 2, all four lanes:** the watch's sixth plate, "Since you last looked", over the daemon's
  unsettled actions (Item 238); the Ship's words from one place, a console of the engine and
  tokens a minute, the tour with what's new, a sea that is still when nothing happens, and sound, off by default (Item
  239); every chart drawn by the chart method, each with a table view (Item
  242); and the critique's repairs, among them Judgment without overlaps, and a daylight mode
  (Item 243).
- **Discord's bindings watch** acts on no half-written save, so a change binds within two periods (about 2 to 4 s),
  and tries a failed bind again each tick (Item 240); the turn bench names each run's slowest frame
  (Item 241).

## Built, and waiting to be turned on

These are built, tested and installed, and off on the operator's daemon until he says, or until a live check:
- **AWS hands on Lambda and Fargate** (`aws.hands.run`): a signed envelope, a completion poller, cancels verified per
  backend, overdue hands reaped, quotas and waves, `hands.list` and the cockpit's grid, in an existing VPC and never
  with a NAT of their own (Items 107, 116 and 134). The account has
  no hand yet: its stack and image wait for the operator's go (theseus-ongv).
- **The MCP server** on loopback (`[mcp_server]`, off by default).
- **Memory's measured arms:** `+retention`, `+activation` and `+synthesis`, each off unless `[memory] arm` names it.
- **Self-improvement's spine** (joined October 10; on the next install's list): `[self] mode = "off"` by default, so
  nothing self-directed runs; a kill switch anyone may throw (`theseus self halt`, the cockpit's Halt, "halt self" on
  Discord) and only the operator releases (`theseus self resume`, the web UI, or "resume self" from his own Discord
  account), halted until his first resume; `theseus self log`, every change Theseus made to itself with its numbers
  and its undo; a weekly digest to his DM while the mode acts; and health's `self` line. The keel guard, the gate's
  first phase since October 9, fails a branch that deletes or loosens a test, budget or ceiling without his signed ack.

## Under way now

- **Install #10**, the list above, after the verdict on a job-wrapper test that failed once in a gate on October 7
  (theseus-sdgl), which is being looked at.
- **In the cloud or in review:** four more tasks of the tenth and eleventh batches: the daemon's stops, the judge sink's
  speed, a walk over imported sessions, and the recall benchmark's fairness.
- **The v1 week.** Feature complete (v1-beta) since October 6 at 22:04 (install #8 with the import, and voice's fix for
  a deaf call); v1 comes after a week of the operator's daily use with no serious bug, every speed target met
  (route.v1's wait, the last, joined on October 7), and each of B5's losses explained or fixed: B5's held-out rerun
  waits for a quiet machine (theseus-7gir.22).
- **Claude Haiku 5.5 in the stable**, released October 7 (theseus-3okf), in review: its catalog row with the first
  long-prompt price tier (every price 5x past a 100,000-token prompt, reserved and settled at the tier), route.v2 as
  the acting route pack with a `quick` mode run as a one-turn detour, and the placements: acknowledgements and quick
  questions on Haiku 5.5 at effort low for their turn alone, routine programming on Haiku 5.5 at effort high with
  Sonnet 5.5 behind it, and Haiku 5.5 the template's recommended compaction and synthesis profile.
- **The AWS tools as a tree**, decided on October 6 (the operator's answers T1 to T9): typed tools for the services that
  cover 90% of the use, leaves declared and deferred, BM25 search and Jev's branches, to build (theseus-0nnk).
- **With the operator:** a live voice call, to see a deaf call noticed for real (theseus-d93y); the AWS live checks of
  the hands' stack and the runaway brake (theseus-ongv); whether a trusted guild answers in every channel, unbound
  (theseus-yzhv); the question walk's open rows; and the other items §2 of the spec lists as still open.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes and cloud
sessions beside it. Each step must pass the full test gate, the speed budgets, a live check, and a written review
before it joins.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Done. Review 2 was accepted whole on October 1. Its spine items (C6, C2, and S2) are all in, with its security items (installed on October 2) and the lanes for robustness, security, performance, proof points, and the bench. What it deferred is in the v1.1 roadmap. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Done on October 1: the push, the client library, reach, the terminal UI, and herdr. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials at launch, private and shared places, integrity, the ontology, the job host | Done for v1. L1, cancellation verified per backend, egress, credentials at launch, the place rule, integrity's light pieces and the sandbox's trims landed on October 2 and 3; the trusted guild and 18e on October 3's evening; the ontology wired in (21b) and its cockpit view (21c) on October 4. The job host (22b) moves after v1. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | Built. The bound account (C1) and its stacks and writes (C2) by October 3; on October 4 the curated tools (C3), the durability tender (15), restore from S3 (16), and the hands (step 40, both parts, and their network), installed at 14:09; runaway mode at 20:00; fixes to the tender, restore and runaway mode on October 5, and the S3 backup on the operator's daemon from October 6 (install #6); every credential mint held to a reviewed decision that night. The hands' stack and their live checks on the account wait for the operator, and the AWS tools' tree is decided, to build. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | Built, from October 4: the wire-in (23a), its surfaces (23b), the security check and its live notices (24), inbound class and role (25a), continuation (25b), the learning ledger (25c), replay (25d), routing (25e, live), the ladder (26a), the task arrangement (27), independent checks (28a), topics (28b), L3's proof (row 50) and the learning loop; on October 5 and 6, fixes to replay, the judgment log and the learning rules, the judge's sink between turns and a learned version's promotion, and route.v1's own request to Jev. Left: JUDGE_STOP for tasks under canary (26b) and the roles table under canary (26c). |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | Built, from October 4: recall in shadow (30a), then live in front of the model (30b), compaction (30c), the exam's arms (34b), the memory pass (31a), consolidation (31b), retention (32a), activation (32b), Jev's rerank, in shadow and then live (32c, 32d), tiering (33), and situations (35a), enforced from October 5; the importer for the operator's earlier history on October 6, and the import itself that evening, with the lists and health that step over it. Left: lessons (35b). |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | Built, from October 3: repeating wakes (37a) and voice (rows 77 and 78); on October 4 and 5, MCP tools and prompts (36b, 36c), tasks that set wakes (37b), bindings' second format (38a), gliding (38b), the task record and board (39a, 39b), the MCP server (41b), budgets and policy (42a) and their tabs (42b), and self-proposed extensions (43a, 43b); on October 5 and 6, history pages, Discord's bindings read live, and voice's held barge-in and echoes; on October 6 and 7, voice that is told what was heard and notices a deaf call, and the cockpit's rethink (its phase 2's four lanes). |

**After v1:** [the v1.1 roadmap](design/roadmap-v1.1.md) plans the week that follows, about 60 agent-hours: the
kernel's last frames, visibility, the operator's surfaces, cost accuracy, the codebase's shape, proof and the
turn's edges, and Discord.

**When.** Theseus has been feature complete, v1-beta in the operator's terms of October 6 (13:35), since October 6
at 22:04. v1 comes one week after it, under his rule of October 4 (22:09): seven days of his daily use with no
serious bug, every speed target met, and each of B5's losses explained or fixed; so about **October 13** at the
earliest. The spine's last rows (26b, 26c and 35b) run beside it. This
pass did not re-estimate v1.1's date.

## Recently landed

- **2026-10-06 to 07, night:** the eleventh cloud batch's stack for AWS, health and the bench: every credential mint and
  stored secret in the AWS catalog held to a reviewed decision (Item 232), health's own sessions counted
  apart from the imported (Item 233), and the Harbor arms made comparable (Item
  234). A running job's cancel settles in two synced frames (Item 235); a stopping
  daemon begins no retry (Item 236); route.v1 asks Jev in a request of its own, on a warm connection
  (Item 237). The cockpit's phase 2, its four lanes: the watch with a sixth plate, "Since you last
  looked" (Item 238), the Ship's words, console, tour, motion and sound (Item
  239), the chart method on every page (Item 242), and the critique's
  repairs with a daylight mode (Item 243). Discord's bindings watch (Item
  240) and the turn bench's rows (Item 241). All on install #10's list.
- **2026-10-06, evening:** a learned version's promotion and rollback named (Item 222). Discord's
  bound lanes and their tests (Items 223 and 224). Five tests that failed under
  load fixed at their causes (Item 225), and the scrub's other encodings (Item
  226). The recall driver's bounds (Item 227). The core's gaps and a turn's
  reservation on the upper bound (Items 228 and 229). Voice's deaf calls and held
  replies (Items 230 and 231). All installed at 22:13 (install #9); v1-beta declared at
  22:04.
- **2026-10-06, afternoon:** voice told what was heard (Item 216). No person's name in the tree (Item
  217). Pi as the benchmarks' fourth arm (Item 218). The session lists past imported sessions
  (Item 219). A report for every benchmark run, eighteen from the retrospective, in
  [docs/benchmarks/](benchmarks/README.md) (Item 220). The judge's sink between turns, with
  `bench turn --judge` (Item 221). All installed at 17:22 (install #8), and the operator's earlier
  history imported at 17:26.
- **2026-10-06, late morning:** the tools stack: a JSON-escaped secret scrubbed (Item
  204), the gate's layers held by tests and L3 in `policy explain` (Item
  205), a decline that ends its batch's waits (Item 206).
  Discord's bindings read live (Item 207). The judge pair (Items
  208 and 209). Memory's and telemetry's tests (Items
  210 and 211). The core stack: its waits, route.v1's tests and a
  routed turn's trace, and the daemon's proofs (Items 212, 213 and
  214). Voice's echoes (Item 215). All installed at 13:03 (install #7).
- **2026-10-06, morning:** the importer for the operator's earlier history, and the store at format 23 (Item
  201). Voice's held barge-in and the floor (Item 202). The learning
  fixes (Item 203). All installed at 10:12 (install #6), with the S3 backup turned on
  and consolidation's nightly writer at $0.50 a day; the operator's store moved from format 22 to 23 at its first
  write, after the install's backup.
- **2026-10-05 to 06, night:** the eighth cloud batch's stacks T, R and S, and the bench stack. `theseus watch` ends
  its line when the daemon dies (Item 191); the turn's stack (Item
  192); history pages both ways (Item 193); every queue says why
  (Item 194); the smalls, among them the static musl build (Item 195); the WAL
  directory's sync skipped when a mark vouches (Item 196); a crashed call's hold
  booked as spent (Item 197); the durability tender's own prefix and its health lines (Item
  198); and the bench's hygiene and recall plan (Items 199 and
  200). All installed at 10:12 on October 6 (install #6).
- **2026-10-05, afternoon and evening:** route.v1's gaps, the store at format 21 (Item
  181); the reader rule's holes (Item 182); situations, enforced from the first day,
  the store at format 22 (Item 183), these three installed at 16:48 (install #5, the v1 build).
  Then the bench fixes (Items 184 to 186),
  memory's checks (Item 187), a synthesis's heading (Item 188), the
  timing flakes (Item 189) and telemetry's third pass (Item 190),
  installed on October 6 at 10:12 (install #6).
- **2026-10-05, morning:** the seventh cloud batch's fixes. The durability tender ships only synced frames, and a
  restore stitches the tails after a sealed object (Item 171). Each notification is serialized
  once for every watcher (Item 172). A job's deadline stop verifies its whole tree's kill, and after a
  crash an earlier process's provider calls settle as unknown (Item 173). Runaway mode ends on a
  raised line, and `[policy.aws]` keys are checked against the catalog (Item 174). `proc.run` takes up
  to 16 steps, and the store is format 20 (Item 175). A failed WAL sync cuts its frames back to the
  last good sync (Item 176). Health's words: the web UI's refusals, the 1-hour cache, the binary in the
  cockpit, and free space crossing a line (Item 177). A failed provider call's error class, and AWS
  requests counted and timed (Item 178). `judge.prove` from the daemon's own ledger (Item
  179). A confirmed call's run and a late result are traced in the turn that answers them, and each tool call counts once (Item 180). Earlier that morning: a job result that arrives during a restart's startup is
  taken at once (Item 166); three bench harnesses, for efficiency, concurrency and incidental
  recall (Items 167, 168 and 169); and the kernel simulator
  drives stops, tasks and the outbox under crashes (Item 170). All installed at 13:07 (install #4);
  the operator's store moved from format 16 to 20 at its first write, after the install's backup.
- **2026-10-05, night:** the sixth cloud batch, and two lanes. Retention, FSRS-6 as the `+retention` arm (Item
  157); a greeting's reply streams, and trivial messages route at 0.4 (Item 158); spreading
  activation as the `+activation` arm (Item 159); consolidation into cited syntheses, and a private
  place that draws on every session (Item 160); a trivial detour sends no recall (Item
  161); stubs and a bounded node cache (Item 162); claim leases, the task board and
  `/tasks` (Item 163); the learning loop (Item 164); and the cockpit's Budgets, Ledger
  and Policy tabs (Item 165). All installed at 13:07 (install #4).
- **2026-10-04, evening:** B5's first full Terminal-Bench 2.0 run, in [benchmarks.md](benchmarks.md) (Item
  148). Linux jobs: a job starts without a fork (Item 149), one sync per completion and background
  passes that yield to a busy machine (Item 151), and a cgroup of its own with an exact stop (Item
  152). A routed session keeps its move only while `route.v1` acts, and b5's harness fixes (Item
  150). rust-analyzer's compiler errors (Item 153). A refused request goes once
  to its fallback (Item 154). Theseus reads the files people give it, PDFs first (Item
  155). The system header says how a request is assembled (Item 156). Items
  148 and 149, and the route fix, installed at 21:22 (install #3); the rest on October 5 at 13:07 (install #4).
- **2026-10-04, afternoon:** `route.v1` live: Jev picks each message's model (Item 139). Gliding on the
  place rule (Item 140). `rerank.v1` live in front of the model (Item 141).
  `security.v3`'s live notices (Item 142). The Ship's sea rolls (Item 143).
  Replay, audit and backfill (Item 144). The ladder (Item 145). Tasks' and tools' smalls,
  among them AWS's runaway mode (Items 146 and 147). The README's animated logo
  (Item 137) and one-command setup with the `/etc` config (Item 138). All installed at 20:00
  (install #2), with the operator's model profiles; the store moved to format 16.
- **2026-10-04, morning:** the fifth cloud batch, M5's and M6's rows. The judgment surfaces (Item
  119), `security.v1` and `security.v3` at the gate (Item 120), class and
  role at inbound (Item 121), continuation (Item 122), all in shadow. Restore
  from S3 (Item 123). The exam's memory arms (Item 124). The task record and its graph
  (Item 125). Topics in shadow (Item 126). An edit's diagnostics (Item
  127). The `+rerank` arm in shadow (Item 128). The learning ledger (Item
  129). Budgets and `policy.explain` (Item 130). Compaction and the assembled
  context (Item 131). Independent checks (Item 132). Extensions that load
  (Item 133). The hands' network (Item 134). The cockpit without WebGL (Item
  135). The memory pass after each turn (Item 136). All installed at 14:09
  (install #1), the operator's config moved to `/etc/theseus/theseus.toml` and the judge turned on, every pack in
  shadow; the store moved from format 6 to 14.
- **2026-10-04, night:** the fourth cloud batch's harvest and the fifth's first joins. AWS's curated tools (C3; Item
  97). Tasks that set wakes (Item 98). Recall in shadow (Item
  99). The ontology wired in (Item 100). `theseus judge prove` (Item
  101). The user service's restart limits (Item 102). The gate's speed (Item
  103) and its flakes (Item 104). Jev wired in (23a; Item 105). MCP
  tools in turns (Item 106). The hands on Lambda and Fargate (Item 107). The durability
  tender (Item 108). Terminals as tools (Item 109). The MCP server (Item
  110). The cockpit's Ontology view (Item 111). Recall in front of the model (Item
  112). The task arrangement (Item 113). The LSP tools (Item 114).
  MCP prompts (Item 115). The hands' cancels, reservations and the hour's alert (Item
  116). Bindings' second format (Item 117). `extend.propose` (Item
  118). All installed at 14:09 (install #1).
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

- Linux only, by design (the operator, October 4): Theseus uses what the kernel offers. One daily user so far.
- The terminal UI loads a session's newest 200 messages.
- With no params, `session.list` and `execution.list` still read every record: about 140 to 180 ms at 10,000
  sessions. The other polled reads page through the index, and step over imported sessions by key (Item
  219).
- The AWS hands are built and installed, but wait for their stack and their live checks on the account; until a hand
  runs, the runaway brake has not been seen live (theseus-ongv).
- A sandboxed command holds a granted secret for its whole run, and can't ask for one it didn't get at its start.
  It has no memory limit, as an ordinary command has none.
- A daemon run as root refuses sandboxed commands, since Linux exempts root from the process limit L1 sets. Run it
  as your own user, as the user service does.
- A sandboxed command with no network and no secret still waits in a session that read outside text.
- Whether anyone else can view a Discord channel you bound private is checked when the binding starts (at the
  daemon's start, or when the place is added while it runs), so a member added later is seen at the next start.
  In a trusted guild it isn't checked at all: your word is the check.
- A store moves to a newer build's format at that build's first write, one way: the install's backup is the only way
  back to an older build.
- Under WSL, health's free-space lines watch the virtual disk under the state directory, not the Windows drive that
  holds it, which fills first (theseus-83y0).
- A `--stdio` daemon answers `shutdown` and keeps serving (theseus-yg1y); the socket daemon stops.
- An erase of imported history hides it: its text stays in the WAL, in backups and in the S3 copy until the spec's
  in-place redaction (§5.6) is built, and an erased episode cannot be imported again under any tag (Item
  201).
- In Discord, on the installed build, a place added while the daemon runs whose session cannot open is not tried
  again (theseus-u6v6; fixed by Item 240, on install #10's list).
- Voice's speech prices are Deepgram's published rates, assumed until confirmed.
- On the installed build, `aws.call` answers three newer credential mints (two of STS's, one of EKS's) with their values
  instead of a handle (theseus-ye7o), and health's session count includes the imported sessions (theseus-revl); both are
  fixed by joins on install #10's list (Items 232 and 233).
- A deaf voice call is noticed and rejoined once, but the rule's live input, the clients' reports arriving, has not been
  seen in a real call yet (theseus-d93y), and a listed speaker who arrives after the join is held to the call's clock,
  not their own, until theseus-kcng.
- The imported history's vectors embed in the background over days, so until then a search finds an imported episode by
  its words, not its meaning.
- At install #9 the old daemon took 11.87 s to stop, with nothing logged between (install #8's took 298 ms;
  theseus-vjn7), and the old index tender had stopped answering its status socket for hours, which the restart cleared
  (theseus-uazd).
- Replay and audit runs each keep a spend cap of their own and do not draw on the judge's day budget, which the
  October 4 default asked for (Item 144).
- On this machine a turn's harness overhead reads well over the README's 5 ms, mostly the disk's fsyncs under WSL,
  in debug builds. A measurement on a release build is still to come.
