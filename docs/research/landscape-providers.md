# Scope A: provider-made agent harnesses, checked against Theseus's rows (facts as of 2026-10-01)

_Checked in on 2026-10-01 from the agents' working reports. Local paths became plain words or links; nothing else changed. Times are MST._

**How I gathered this.** I fetched official docs, changelogs, release notes and the GitHub API on 2026-10-01 between 09:04 and 09:20 UTC. I used web search only to find sources. Anything backed only by a search summary or a third-party article is marked *(secondary)* or **unconfirmed**. Star counts are the GitHub API values at fetch time. The keys in [brackets] point to the source list at the end. The report gives facts only; the verdicts are Tabitha/Claude's.

---

## 0. What changed since mid-2026

1. **Provider CLIs now run long-lived local services.**
   - Claude Code runs background sessions under a "supervisor" (`claude daemon status|stop`), which can be installed as an OS service [A4].
   - Codex CLI 0.157.0 (2026-09-25) "enabled automatic background-server startup for eligible interactive sessions" [O1].
   - Amazon's open-source Kiro Crew is a gateway daemon with a durable work queue [K5][K6].
2. **Providers now offer always-on agents you reach by chat.**
   - OpenAI's **dots** (2026-09-29) are always-on agents with their own cloud computer. You reach a dot in ChatGPT, Slack or Teams, or by voice call [O12].
   - Anthropic's **Claude Tag** puts Claude in Slack (beta) [A17].
   - Claude Code **Channels** push Telegram, Discord or iMessage messages into a running session (research preview) [A12].
3. **A second model now often decides approvals instead of the user.**
   - Claude Code's classifier-based **auto mode** is the built-in starting mode from v2.1.283 [A6].
   - Codex has an **auto-review** reviewer agent [O5], and dots use the same mechanism [O12].
4. **Egress proxies with a domain allowlist** ship in Claude Code [A5], Codex [O6] and Antigravity [G5]. Copilot's cloud agent runs behind a firewall [M2].
5. **Providers now sell their harness as a hosted API.** Anthropic launched Claude Managed Agents (2026-04-08) [A14]. OpenAI launched the Agents API (2026-09-29), which it says is "powered by the open-source Codex harness" [O13].
6. **Several products were merged or renamed.**
   - Gemini CLI is giving way to Antigravity CLI: announced 2026-05-19, consumer cutoff 2026-06-18 [G1].
   - The Codex app merged into the ChatGPT desktop app on 2026-07-09 [O2][O14].
   - Amazon Q Developer CLI was rebranded Kiro [K1].
   - GitHub's "coding agent" is now the "cloud agent" [M1].
7. **Billing moved to metered credits.** GitHub AI Credits started 2026-06-01 [M4]. Codex/ChatGPT credits and a Pro $500 tier [O11], Antigravity AI credits [G6] and Z.ai credit windows [X4] follow the same pattern.
8. **OpenClaw and Jev show up in this landscape.**
   - Kiro Crew's authors say they were "inspired by the momentum of OpenClaw" [K5].
   - Kiro Crew 0.7.0 adds an opt-in "Decisions (Jev)" preview that routes turns [K6]. The page doesn't say whether this is TypeSafe's Jev (**unconfirmed**).
   - A community recap of DevDay says "an OpenClaw enterprise harness was briefly mentioned" in OpenAI's keynote [O4] (**unconfirmed**).

---

## 1. Anthropic

### 1.1 Claude Code (CLI, VS Code and JetBrains, Desktop, web/cloud, mobile)

**Shape.**
- **Surfaces:**
  - Terminal CLI, VS Code and JetBrains extensions.
  - Desktop app for macOS and Windows; Linux beta since the week of 2026-06-29 [A2].
  - Cloud sessions at claude.ai/code, and iOS/Android apps.
  - Chrome extension, generally available since the same week [A2].
  - Remote Control, which continues a *local* session from a phone or browser [A1].
  - Voice dictation in the CLI [A1].
- **Slack:** the older Slack integration is being retired for Team and Enterprise in favour of Claude Tag [A1].
- **Release pace:** the changelog head is **v2.1.286** [A3]. Weekly digests cover 5–10 versions a week; Sept 7–11 covered v2.1.263–269 [A2]. The CLI moved to native binaries in the week of Apr 13–17 [A2].
- **Long-lived service.** `claude agents` (agent view, research preview, week of May 11–15) is one screen for background sessions [A2][A4].
  - Each session is its own process under a supervisor (the "background service"). It keeps running with no terminal attached.
  - State lives in `~/.claude/daemon.log`, `~/.claude/daemon/roster.json` and `~/.claude/jobs/`. `claude daemon status` reports the PID, version, socket directory and worker count.
  - It can run as an installed OS service, or start on demand [A4].
- **Licence:** proprietary. The GitHub repo (issues and plugins) has no licence: `license: null`, 148,765★, 25,117 forks [A18].

**Models.**
- Claude only. It can reach Claude through the Anthropic API, Bedrock, Claude Platform on AWS, Google Cloud's Agent Platform ("formerly Vertex AI"), Microsoft Foundry, or an LLM gateway [A1].
- An `allowedProviders` managed setting (v2.1.285) limits which of those a machine may use [A3]. `fallbackModel` sets up to three fallback models (week of Jun 8–12) [A2].
- **Current models:**
  - Opus 5.5 (September 2026; *secondary* date: Sept 22). It costs $4/$20 per million input/output tokens and $0.20 per million cache reads, generates output ">30% faster" than Opus 5, and is "more resistant than Opus 5 to prompt injection" [A15].
  - Fable 5.1 with 1M context (week of Aug 31) [A2].
  - Opus 5 (week of Jul 20) and Sonnet 5 (week of Jun 29) [A2].
- **Other providers' models:** only through a gateway or custom endpoint. For example, Z.ai documents using its GLM Coding Plan inside Claude Code [X4]. I found no documented native local-model support (**unconfirmed**).
- **Platform change that affects harnesses:** "preserved thinking" is an anti-distillation safeguard that "stops API users from editing Claude's prior context." It applies to Fable 5.1 and Opus 5.5 on API accounts created on or after 2026-08-31 [A15].

**Autonomy and durability.**
- **Subagents:**
  - Subagents run in the background by default (week of Jun 29) and can spawn their own, up to five levels deep (week of Jun 8) [A2].
  - "Fork" subagents inherit the full conversation and are on by default (week of Aug 10) [A2].
  - Agent teams have shared tasks and messaging between agents [A1].
  - Sessions on the same machine can message each other (week of Aug 3) [A2].
- **Dynamic workflows** orchestrate "dozens to hundreds of subagents from a script Claude writes" (week of May 25) [A2]. `/goal` (week of May 11) [A2]. Git worktrees, including per-subagent worktree isolation [A1].
- **Scheduling, in three tiers** [A8]:
  - `/loop` and cron tools are session-scoped, restored on `--resume`, and expire after seven days.
  - Desktop scheduled tasks run on your machine, persist across restarts, and can fire every minute.
  - **Routines** run in the cloud on a schedule, an API call or a GitHub event, at most hourly. They run "without stopping for approval" (research preview, week of Apr 13) [A7].
- **Cloud:** cloud sessions can move between surfaces with `--cloud` and `--teleport` [A1]. Self-hosted cloud environments are in public beta for Team and Enterprise (week of Aug 3) [A2]. The redesigned **Projects** (beta, Sept 2026) is a coordinator that runs parallel cloud threads with a shared memory [A16]. `/code-review ultra` runs a multi-agent review in the cloud [A1].
- **What survives a restart** [A4]:
  - Sessions persist through auto-updates and supervisor restarts, and through machine sleep.
  - The supervisor restarts a session process that exits unexpectedly.
  - When a session's process restarts, its "background shell commands, dynamic workflows, and background subagents… carry over to its next process." Running monitors and shells a subagent started do not.
  - Machine shutdown stops running sessions. They show as failed (under 48 h) or stopped (over 48 h) and resume from the saved conversation when you reply.
  - Idle, unattached processes are stopped after about an hour unless pinned.
  - v2.1.286 fixed `--resume` "sometimes losing every turn after a batch of parallel tool calls when the earlier session crashed or was killed" [A3].

**Safety.**
- **Six permission modes** [A6]: Manual (`default`), `acceptEdits`, `plan`, `auto`, `dontAsk` and `bypassPermissions`.
  - Deny rules block in every mode.
  - Ask rules, `rm` on critical paths, and "protected paths" are never auto-approved.
  - Auto became the default for Pro, Max and Team on Aug 14 (week of Aug 3) [A2], and the built-in starting mode on v2.1.283+ [A6].
- **The auto-mode classifier** is a separate model that reviews each action [A6].
  - It blocks anything that "escalates beyond your request, targets unrecognized infrastructure, or appears driven by hostile content Claude read."
  - Its default block list covers destructive git commands, printing live credentials, writing to session transcripts, and pushing secrets or private material to public repos.
  - It also reviews messages sent to other agents.
  - It can run on the server side, falling back to client-side checks.
  - Limits the user stated in the conversation are re-read from the transcript, so compaction can drop them.
  - After 3 blocks in a row or 20 in total, it falls back to prompting.
- **Sandbox (Bash, PowerShell, Monitor, and their child processes)** [A5]:
  - macOS uses Seatbelt. Linux and WSL2 use bubblewrap plus socat, with an optional seccomp filter that blocks Unix sockets. Native Windows is unsupported.
  - Network goes through a proxy outside the sandbox: `allowedDomains`, `deniedDomains`, `strictAllowlist`, and a managed-only domain lockdown.
  - The proxy filters by hostname and doesn't inspect TLS unless the experimental `tlsTerminate` is on.
  - **Credential masking:** commands see a per-session sentinel value, and the proxy swaps in the real secret only for the listed hosts, including re-signing AWS SigV4 requests.
  - `failIfUnavailable` makes a missing sandbox a hard failure. A "dangerouslyDisableSandbox" retry escape hatch exists and can be disabled.
  - The same primitives ship as `anthropics/sandbox-runtime` (Apache-2.0, 5,407★) [A5][A18].
- **Cloud environments** default to a "Trusted" network allowlist [A7].
- **Secrets:** v2.1.286 tightened secret redaction in logs, transcripts and MCP errors [A3].

**Cost governance.**
- `/usage` shows a session cost estimate at list price, or at contract rates if a `modelPricing` table is set [A9].
  - Subscribers see what drives their plan limits: skills, subagents, plugins, MCP servers and loops.
  - A prompt-cache line shows the share of input served from cache (v2.1.251+) [A9].
- **Caps:** a `--max-budget-usd` flag; monthly spend limits on usage credits for Pro and Max; organisation, group and member limits for Team and Enterprise; workspace spend limits in the Console [A9].
- **Gateway caps:** the self-hosted **Claude apps gateway** caps each developer by day, week or month, "enforced live on every request" [A1].
- **Benchmarks:** enterprise average is "~$13 per developer per active day, $150–250 per month" [A9].
- The pages I read don't say whether `--max-budget-usd` reserves the worst case before a call (as Theseus does) or checks after it.

**Memory.**
- CLAUDE.md at four levels (managed, user, project, local), `AGENTS.md` support, and path-scoped `.claude/rules/` [A11].
- **Auto memory** is on by default [A11]:
  - Per repository at `~/.claude/projects/<project>/memory/`: a `MEMORY.md` index plus one topic file per memory.
  - The first 200 lines or 25 KB load into every session. It is machine-local and shared across worktrees.
  - Subagents can keep their own memory.
- Projects share memory across threads [A16]. Claude Tag builds memory from its channels [A17].
- I found no documented vector or semantic retrieval for Claude Code memory (**unconfirmed**).

**Extensibility.**
- MCP client over stdio and HTTP, with `claude mcp login` [A2], and MCP server mode with `claude mcp serve` [A19].
- Plugins and marketplaces, including plugin evals (week of Sep 7) and dependency versions [A1][A2].
- Skills. Hooks of several kinds: command, HTTP, prompt and MCP-tool hooks, including async ones [A1].
- Channels, the Agent SDK, headless `-p` with JSON output, `claude-cli://` deep links, LSP code-intelligence plugins [A1].

**Observability.**
- OpenTelemetry metrics (OTLP or Prometheus) and logs/events; traces are in beta [A10].
- Managed settings can lock the collector destination [A10].
- Analytics dashboard and a Claude Code Analytics API [A9]. Self-hosted runners export Prometheus metrics [A1].

**Speed.**
- No published cold-start figure.
- The changelog reports only small deltas, such as "~30ms" and "~60ms" startup gains and a fixed "~120ms" regression [A3].
- Fast mode on Opus 5 costs $10/$50 per million tokens (week of Jul 20) [A2].

**Momentum.**
- 148,765★ [A18]. Several releases a week [A2].
- **Launches since June:** Artifacts, auto mode on Pro, dynamic workflows, Desktop in-app browser, Opus 5, Sonnet 5, Fable 5.1, Opus 5.5, cross-session messaging, self-hosted environments, `/design`, Projects [A2][A16].
- A $2.5B run-rate in Feb 2026 is widely reported (*secondary*, **unconfirmed** here).

### 1.2 Claude Agent SDK
- A Python and TypeScript library that "runs the Claude Code binary" [A13].
- It includes built-in tools, hooks, subagents, MCP, permissions, sessions (resume and fork), skills and memory loaded from `.claude/`, and plugins.
- Use is governed by Anthropic's Commercial Terms. Third parties may not offer claude.ai login or plan rate limits without approval [A13].
- The Python repo is labelled MIT on GitHub, with 8,199★ [A18].

### 1.3 Claude Managed Agents (hosted harness)
- **Status:** public beta since **2026-04-08**, beta header `managed-agents-2026-04-01` [A14].
- **Model:** agent, environment, session and events.
  - Sessions run in an Anthropic sandbox or a self-hosted one.
  - Responses stream over SSE, and event history persists on the server. You can steer or interrupt mid-run.
- **Tools:** bash, file tools, web search and fetch with domain allow/block lists, and MCP.
- **Scheduling:** "scheduled deployments" run on cron.
- **Research previews:** multi-agent coordination, self-evaluated "outcomes," MCP tunnels, and "dreaming" (memory).
- **Limits:** not eligible for zero data retention or HIPAA; tracing is built into the Console [A14].
- **Price:** tokens plus a reported $0.08 per active session-hour (*secondary*, **unconfirmed**).

### 1.4 Other current Anthropic agent products
- **Claude Tag (Slack)** [A17]:
  - Beta for Team and Enterprise. One multiplayer Claude per channel that learns from the channels it's in.
  - An optional "ambient" mode lets it act proactively, and it can "schedule tasks for itself… over hours or days."
  - Anthropic: "65% of our product team's code is created by our internal version of Claude Tag."
  - Channels can have spend limits (v2.1.286) [A3]. Launch date June 2026 (*secondary*: Jun 23).
- **Cowork** is a desktop agent for knowledge work, with its own OpenTelemetry form in the admin console [A10]. Its GA date is **unconfirmed** (*secondary*: Apr 9).
- Also: `/design` (Claude Design in the CLI, week of Aug 17), the Claude Security plugin (week of Jul 20), and computer use in the CLI and Desktop [A2].

---

## 2. OpenAI

### 2.1 Codex (CLI, IDE extension, ChatGPT desktop app, Codex Cloud, mobile)

**Shape.**
- **What's open source** [O9][O17]:
  - Codex CLI is Apache-2.0 and written in Rust (127,475★). Latest is **0.159.3** (2026-09-30) [O1].
  - The Codex SDK and the app-server are open source; the IDE extension and Codex Cloud are not.
- **Desktop:** on **2026-07-09** the Codex app merged into the ChatGPT desktop app for macOS and Windows; Linux has been in preview since Aug 10–14 [O2][O14].
- **Other surfaces:** Codex Remote (phone), Slack, Linear, GitHub, and GitLab (beta since Aug 19) [O3][O2].
- **DevDay (Sept 29) CLI update:** two-way voice and an `/agents` view [O4].
- **Integration protocol:** the app-server speaks its own JSON-RPC protocol and is "experimental… not supported for production" [O8].
- **`codex mcp-server` was removed.** Integrations must move to the app-server; MCP *client* support stays [O8].
- **Local service:** 0.157.0 (Sep 25) auto-starts a background server for eligible interactive sessions [O1].

**Models.**
- GPT-6.1 Sol is the default (0.159.1, Sep 29). GPT-6 Astra, Sol and Luna are also available. GPT-5.5 retires from Codex on Oct 14 [O1][O2].
- Custom `model_providers` are supported, with reserved IDs `openai`, `ollama` and `lmstudio`; `openai_base_url` points at proxies [O10]. Amazon Bedrock is supported [O1].
- Sign in with ChatGPT or an API key. API-key users don't get cloud features [O11].

**Autonomy and durability.**
- **Codex Cloud reusable environments (DevDay):** "close your laptop… agents will keep working," and you can steer from a phone [O4].
- Subagents and custom agents [O3]. Git worktrees are on by default [O1].
- **Scheduled tasks** [O16]:
  - In the desktop app they need the computer on.
  - On the web they can trigger from Gmail, Slack or GitHub events (since Aug 25) [O2].
  - Team Tasks run under a service account [O4].
- Opt-in `instant_interrupt` lets new input steer a running turn (0.159.0) [O1].
- Whether local sessions survive a restart isn't documented in the pages I read (**unconfirmed**).

**Safety.**
- **Defaults:** network off and writes limited to the workspace [O6].
- **OS sandbox:** Seatbelt on macOS; bubblewrap on Linux and WSL2, with a bundled helper that needs unprivileged user namespaces; a native Windows sandbox [O5].
- **Modes:** sandbox modes are `read-only`, `workspace-write` and `danger-full-access`. Approval policies are `on-request`, `never` and granular; `untrusted` is retired [O5][O6].
- **Auto-review** (called "Guardian" in changelog PR titles) [O5]:
  - Sends sandbox-escalation requests to a reviewer agent and blocks exfiltration, credential probing, "broad or persistent security weakening" and destructive actions.
  - The reviewer's policy is open source.
  - `--approve-for-me` arrived in 0.147.0 (Aug 3–7) [O2].
- **Network proxy** [O6]:
  - Domain allow/deny rules, where deny wins.
  - Local and private destinations are blocked by default, with a best-effort DNS/IP check against rebinding.
- **Prompt-injection measures:** web search defaults to a **cached** index "to reduce exposure to prompt injection from arbitrary live content" [O6]. Prisma AIRS can scan prompts (enterprise) [O3].
- `.aws` directories are protected under writable roots (0.159.0) [O1].

**Cost governance.**
- **Plans** [O11]: ChatGPT Free ($0), Go ($8), Plus ($20), Pro ($100, $200 or $500), Business ($20 per user per month annually, $25 monthly), Enterprise and Edu.
- **Limits:** five-hour windows on Plus and Business; Pro has none; credits extend usage [O11].
- **Speed tiers cost more:** Fast mode uses limits at 2.5× and Ultrafast at 8× [O11].
- **Admin tools:** per-user spend limits in Enterprise, the Analytics API, and Compliance API audit logs [O11][O3].

**Memory.**
- `AGENTS.md` [O3].
- **Local memories** in `~/.codex/memories/` [O7]:
  - Generated in the background from idle prior chats, with secrets redacted.
  - Skipped when rate-limit headroom is low.
- **Computer History** (macOS, Aug 10–14) turns app and web activity into memories [O2].
- `/import` brings in setup and recent work from Claude Code or Cursor (Aug 11) [O2].

**Extensibility.**
- MCP client [O8], and **MCP Events** (webhooks, requires MCP 2.0; DevDay) [O4].
- Skills, Agent Plugins (0.147.0), hooks that run scripts or MCP tools, the Codex SDK (TypeScript), `codex exec`, a GitHub Action, and WebMCP site tools [O3][O2].

**Observability.**
- Opt-in OpenTelemetry log export (`[otel]`, otlp-http or otlp-grpc) plus counter and histogram metrics [O10].
- Anonymous usage telemetry is on by default [O10].

**Speed.**
- No published startup figure.
- Ultrafast generates tokens "up to 8x faster" than Standard (Pro $500 and Enterprise only) [O11].
- DevDay cited 45% lower API time to first token and >30% faster tool calls (community recap [O4], *secondary*).

**Momentum.**
- 127,475★ [O17].
- "More than 5 million people use Codex every week" (OpenAI, 2026-07-09) [O14].
- DevDay figures of 35M weekly Work and Codex users are *secondary* and **unconfirmed**. ChatGPT has "1.2B weekly users" (DevDay recap, 2026-09-29) [O4].

### 2.2 OpenAI Agents SDK
- v0.22.3 (2026-09-17), MIT licence, 29,795★ [O15][O17].
- "Provider-agnostic… 100+ other LLMs."
- **Features:** `SandboxAgent` (local Unix, Docker or hosted sandbox clients), realtime and voice agents, guardrails, human-in-the-loop, sessions (including Redis), built-in tracing, handoffs, and MCP [O15].

### 2.3 Agents API (hosted Codex harness)
- **Status:** public beta since **2026-09-29** [O13].
- **Where sessions run:** an OpenAI-hosted sandbox, your own infrastructure, or partner sandboxes (Blaxel, Cloudflare, Daytona, DigitalOcean, E2B, Modal, Oracle, Runloop, Vercel).
- **Features:** automatic compaction, tool search, programmatic tool calling, and multi-agent support.
- **Price:** "no additional fees" beyond tokens and tools [O13].

### 2.4 ChatGPT agent mode → ChatGPT Work, and dots
- **ChatGPT Work** launched **2026-07-09** on GPT-5.6 with "Codex technology built-in" [O14].
  - It runs hours-long tasks with approvals and Scheduled Tasks, and shares usage limits with Codex [O14][O11].
  - That it replaced agent mode is *secondary* only.
- **Dots** launched **2026-09-29** [O12]:
  - **How they run:** powered by GPT-6 Astra, each with its "own cloud computer and browser." They work "24/7," including when your computer is off. A dot "can decide when to pause and wake up," and runs background agents.
  - **Where you reach one:** ChatGPT, Slack or Teams, or a voice call; texting is "coming soon." A dot can use one connected personal computer.
  - **Memory:** ChatGPT memory plus the dot's own notes.
  - **Controls:** Custom Rules and auto-review. "Proactive research" runs with read-only tools. A monitoring system "can pause or stop the dot's work," and saved passwords aren't exposed to the model.
  - **Availability:** rolling out on Pro, Business Premium and Enterprise in eligible markets; the EEA, Switzerland and the UK were excluded at launch (forum quote, [O4]).
  - Specialist dots and a Microsoft Agent 365 integration are previewed or planned [O4][O12].

---

## 3. Google

### 3.1 Gemini CLI → Antigravity CLI
- **The transition** (Google, **2026-05-19**) [G1]:
  - Antigravity CLI, "built in Go," became available to everyone that day.
  - On **2026-06-18**, Gemini CLI and the Code Assist IDE extensions stopped serving Google AI Pro and Ultra users and free Code Assist for individuals.
  - Enterprise customers (Code Assist Standard or Enterprise) and paid API-key users keep Gemini CLI.
  - Antigravity CLI keeps "Agent Skills, Hooks, Subagents, and Extensions (now as Antigravity plugins)," and shares its agent harness with Antigravity 2.0.
- **Gemini CLI today** [G9]:
  - Still released nightly (v0.64.0, 2026-10-01). Apache-2.0, 107,211★, TypeScript.
  - Documented features include MCP, `GEMINI.md`, checkpointing, sandboxing, trusted folders, telemetry docs and extensions.
  - Its README still advertises the free tier, which conflicts with the June 18 cutoff.
- **Antigravity CLI is closed source** [G10][G9]:
  - The public repo `google-antigravity/antigravity-cli` holds issues only: no language, no licence, 2,438★.
  - Users in the transition thread complained that it can't log in with an AI Studio key.

### 3.2 Antigravity 2.0 (desktop, May 2026)
- **Shape:** a standalone desktop app for macOS, Linux and Windows with "no IDE" [G2].
  - Dynamic subagents, asynchronous tasks, JSON hooks, **Scheduled Tasks (crons)**, and projects that span several folders.
  - Slash commands include `/goal`, `/schedule` and `/browser`; voice input transcribes live.
  - Remote Control drives instances on other machines from a browser, with push notifications [G7].
- **Latest release (2026-09-30)** [G4]:
  - You can message a subagent directly, and there's a plugin marketplace (Customizations tab).
  - `/plan` follows a Plan Review Policy, and rules get a 20,000-token budget.
  - It fixed a bug that let agents modify `.git`, `.env` and `.vscode` without a prompt.
- **Models** [G6]: Gemini 3.8, 3.7 and 3.6 Flash and 3.1 Pro on every plan. Claude Sonnet 4.6, Claude Opus 4.6 and GPT-OSS-120b are available except on Enterprise.
- **Safety** [G5]:
  - The terminal sandbox uses Linux namespaces or macOS sandbox-exec (Seatbelt), with "no virtual machines… no startup delay."
  - It blocks `~/.ssh` and `.env`. Network is off by default; domains granted under `read_url` join the outbound allowlist.
  - In the presets, Default runs commands in the sandbox and asks before leaving it; Request Review and Turbo run without the sandbox.
  - Another settings path labels Sandbox Mode as "Preview" and says none of its security presets turn it on. `unsandboxed(...)` rules carve exceptions.
- **Pricing** [G6]: a free individual plan with weekly limits, and Pro and Ultra plans with higher limits plus an AI credit pool.
- **Momentum:** "millions of developers have adopted the Antigravity IDE" [G2].
- Memory, OpenTelemetry and licence are not confirmed in the pages I read. The Antigravity SDK's Apache-2.0 licence is *secondary* only.

### 3.3 Jules
- **What it is:** an asynchronous agent in cloud VMs [G8].
- **Recent changes:** CI Fixer (Feb 19), MCP connectors (Feb 2), and a "Planning Critic" that cut task failures by 9.5% (Jan 26) [G8].
- **Other features:** a REST API with repo-less sessions, scheduled tasks (edit, pause, resume), and suggested tasks [G8].
- **No public changelog entries after 2026-03-09.** The last one made Gemini 3.1 Pro the default. Activity since then is **unconfirmed**.

---

## 4. GitHub and Microsoft

### 4.1 Copilot cloud agent (formerly the "coding agent")
- **Renamed:** the docs URL now redirects to "cloud agent" [M1].
- **How it works** [M1]:
  - Runs in an ephemeral GitHub Actions environment. It can research a repo, plan, and iterate on a branch before opening a PR.
  - Triggered from issues, the agents panel, `@copilot`, Slack and Teams (preview), Jira, Linear, Azure Boards, and **automations** (on a schedule or event).
  - Supports custom agents, hooks, skills and MCP; the GitHub and Playwright MCP servers are on by default. **Copilot Memory** is in public preview.
- **Limits** [M1]:
  - A **59-minute hard cap** per session.
  - One repository, one branch and one PR per task.
- **Safety** [M2]:
  - Only users with write access can trigger it.
  - It can push only to its own `copilot/` branch and can't run `git push` itself.
  - Its PRs are drafts that a human must merge, and workflows wait for "Approve and run."
  - A firewall limits internet access.
  - Hidden characters (for example HTML comments) are stripped "to mitigate prompt injection."
  - Its own code goes through CodeQL, the Advisory Database and secret scanning.
  - Commits are signed, and session logs plus audit-log events are kept.
  - Automations get an explicit tool allowlist and ignore events from untrusted users by default.

### 4.2 Copilot CLI and IDE agent mode
- **CLI** [M3]:
  - Interactive or `-p`, with a plan mode.
  - Local sandbox via `/sandbox enable` and cloud sandbox via `copilot --cloud`, both in public preview; the cloud sandbox inherits the cloud agent's firewall.
  - **Bring your own provider:** any OpenAI-compatible endpoint (including Ollama and vLLM), Azure, or Anthropic.
  - Runs as an **ACP server**. Supports hooks, custom agents, MCP and Copilot Memory, and auto-compacts at 95% of context.
  - Repo `github/copilot-cli`: 11,234★, licence "NOASSERTION" [M5].
- **IDE agent mode** "makes autonomous edits directly in your local development environment" [M1].
- **Billing** [M4]:
  - From **2026-06-01**, AI Credits replaced premium requests, metered on input, output and cached tokens.
  - Base prices are unchanged: Pro $10, Pro+ $39, Business $19 per user, Enterprise $39 per user.
  - There is "no fallback" to cheaper models when credits run out.
- **Budgets:** members who hit their budget can request more, and owners approve or adjust it; generally available **2026-09-16** for Business and Enterprise [M6].
- **Adoption:** figures such as 50M users are *secondary* and **unconfirmed**.

---

## 5. Amazon

### 5.1 Kiro (IDE, CLI, Web autonomous mode) and Q Developer
- **Q Developer → Kiro:** "The Amazon Q Developer CLI has been rebranded to Kiro," and the Q console became the Kiro console [K1].
- **Kiro CLI 2.26.0** (2026-09-30) [K2]:
  - The V3 terminal UI with opt-in **Workflows** and stricter approval checks; the Classic interface is deprecated.
  - **2.25** (Sep 28): installable Powers and a SessionEnd hook.
  - **2.24** (Sep 23): `/tools trust-all` for a session; project `.env` files "no longer load automatically."
  - **2.23** (Sep 21): cloud sessions can start before a repo is connected.
- **Models** [K3]: the free tier gets Claude Sonnet 4.5 plus open-weight models (Qwen3 Coder Next, DeepSeek 3.2, MiniMax M2.1). Paid plans add Auto, Claude Sonnet 5 and Claude Opus 5.
- **Kiro Web autonomous mode** [K4]:
  - Works in an isolated sandbox. You assign work at app.kiro.dev, with a `kiro` issue label, or with `/kiro` in a comment.
  - It has no merge rights.
  - In an AWS case study it opened PRs for 87 issues in two months.

### 5.2 Kiro Crew (open source, August 2026), the closest structural analogue to Theseus in my scope
- **Basics:** Apache-2.0, Python; repo created 2026-07-16; 4,232★ [K7]. Launched about **2026-08-04** (SiliconANGLE URL date; InfoQ, Aug 2026) [K5].
- **Origin:** an internal Amazon tool called "MeshClaw," "inspired by the momentum of OpenClaw." Amazon reports 39,000 internal users and nearly 500 contributors [K5].
- **Shape** [K5][K6]:
  - A **gateway** daemon with a desktop app, web dashboard and TUI. Chat surfaces: Slack, Telegram, Discord, WeCom and Webex.
  - It drives agents over **ACP**. Backends: Kiro CLI, Claude Code, Codex, KAS, OpenCode, goose and Pi.
  - **Memory, "lessons" learned from corrections, and auto-generated skills.**
  - Schedules, heartbeats and authenticated webhooks; subagents.
  - **Apps** (DevFleets, Task Runner, Issue Radar) and an App SDK.
- **Durability (0.7.0)** [K6]:
  - Accepted subagent and workflow work enters a "durable queue," and a gateway restart restores it.
  - An optional append-only **Crew log** acts as a "durable session ledger."
  - Auto-updates wait for active turns and runs to go idle.
  - Remote crews run over SSH, AWS SSM or **AWS Fargate** (6-hour task lifetime, at most 10 tasks).
- **Safety** [K5]: an "OS-level sandbox" (mechanism not stated), commands denied by default, "suspicious-pattern blocking," sensitive-path blocking, credential redaction, a "signed audit log of every action," and a dashboard bound to localhost by default.
- **Jev:** "Decisions (Jev)" is an opt-in preview with separate consent switches for what it sends [K6].
- **Costs:** users report it uses tokens "significantly faster than Kiro CLI" (InfoQ, *secondary*) [K5].

---

## 6. Briefly: xAI, Mistral, Moonshot, Z.ai

- **xAI, Grok Build** [X1]:
  - Early beta for SuperGrok and X Premium Plus subscribers. Plan mode; AGENTS.md, plugins, hooks, skills and MCP "out of the box"; parallel subagents in worktrees; headless `-p`; full ACP.
  - Repo `xai-org/grok-build` ("SpaceXAI's coding agent harness and TUI"): **Rust**, Apache-2.0, 27,176★, created 2026-07-14.
  - Launch date **unconfirmed** (*secondary*: May 2026).
- **Mistral, Vibe** [X2]: `mistral-vibe` is Apache-2.0, Python, 5,027★. Release v2.25.8 (2026-09-23) mentions a "Rust CLI," "Vibe Desktop," remote projects and managed worktrees, and ships ACP binaries.
- **Moonshot, Kimi Code CLI** [X3]: `MoonshotAI/kimi-code` is TypeScript, MIT, 7,749★, created 2026-05-22. It succeeds `kimi-cli` (Python, Apache-2.0, 11,432★). The Kimi K3 model details I found are *secondary* and **unconfirmed**.
- **Z.ai** [X4]:
  - The **GLM Coding Plan** is "starting at just 18 USD per month" for Lite, Pro and Max tiers. Usage is metered in credits over a rolling 5-hour window and a weekly window, at half rate off-peak.
  - Models are GLM-5.3 and GLM-5.3-Flash; requests for GLM-5.2 and 5.1 route to 5.3. It works in Claude Code, Cline and OpenCode, and includes web search, web reader and Zread MCP tools.
  - **ZCode** is a free desktop "Agentic Development Environment" with bring-your-own-key and remote steering from Feishu, WeChat or Telegram (VentureBeat).
  - **AutoClaw** is "Z.ai's Official AI Agent," a desktop app with Slack, Telegram, WhatsApp and Lark. A claim that it is an OpenClaw client is *secondary* only.

---

## 7. Facts lined up against Theseus's rows

| Theseus row | Claude Code | Codex | Antigravity | Copilot | Kiro / Kiro Crew |
|---|---|---|---|---|---|
| **One long-lived service** | Supervisor plus a process per session; optional OS service [A4] | Background server auto-starts (0.157.0) [O1]; experimental JSON-RPC app-server [O8] | Desktop app and CLI; Remote Control [G7] | CLI per session; cloud agent capped at 59 min [M1] | Crew gateway daemon [K5] |
| **Work survives restart or crash** | Process restarts carry shells, workflows and subagents; resume after shutdown [A4] | Cloud tasks run with the laptop closed [O4]; local behaviour **unconfirmed** | Not documented in pages read | Ephemeral Actions sessions [M1] | Durable queue across gateway restarts; updates wait for idle [K6] |
| **Chat surfaces** | Channels (Telegram, Discord, iMessage), Claude Tag (Slack), mobile, Remote Control [A12][A17] | Slack, Linear, GitHub/GitLab; dots in ChatGPT, Slack, Teams, voice [O2][O12] | Browser Remote Control with push [G7] | Slack and Teams (preview), Jira, Linear [M1] | Slack, Telegram, Discord, WeCom, Webex [K5][K6] |
| **Model providers** | Claude via 5 platforms or gateways [A1] | OpenAI plus custom, including Ollama and LM Studio [O10] | Gemini, Claude 4.6, GPT-OSS [G6] | Hosted models, or BYO including Ollama (CLI) [M3] | Claude plus open-weight; Crew runs other harnesses [K3][K6] |
| **Native sandbox** | Seatbelt; bubblewrap + seccomp [A5] | Seatbelt; bubblewrap; Windows-native [O5] | Namespaces; sandbox-exec [G5] | Actions VM; CLI sandboxes in preview [M3] | "OS-level sandbox" [K5] |
| **Egress allowlist** | Proxy, allowlist, per-host credential injection [A5] | Proxy; private IPs blocked; rebinding check [O6] | `read_url` domains, off by default [G5] | Firewall [M2] | Not stated |
| **Approval gate** | 6 modes plus classifier; deny/ask rules [A6] | Approval policies plus reviewer agent [O5] | Presets plus allow/deny/ask lists [G5] | Human merge and workflow approval [M2] | `permissions`, trust-all [K2]; approval gates [K5] |
| **Hold after reading the web** | No equivalent found. Closest: classifier blocks actions "driven by hostile content" [A6] | Cached web search by default [O6] | Not found | Hidden-character filtering [M2] | "Suspicious-pattern blocking" [K5] |
| **Dollar budgets** | `--max-budget-usd`; org, member and gateway caps [A9][A1] | Credits; per-user spend limits [O11] | Weekly limits plus credits [G6] | AI Credits, budgets, increase requests [M4][M6] | Not detailed |
| **Ledger / audit** | Transcripts plus OTel events [A10] | OTel logs; Compliance API [O10][O11] | Not found | Session logs, audit log, signed commits [M2] | Signed audit log; append-only Crew log [K5][K6] |
| **OpenTelemetry** | Metrics, logs, traces (beta) [A10] | Logs and metrics [O10] | Not found (Gemini CLI has telemetry docs [G9]) | Usage-metrics API [M1] | Not stated |
| **Memory** | CLAUDE.md plus auto memory (MEMORY.md index) [A11] | AGENTS.md plus local memories [O7] | Rules and skills; memory doc not found | Copilot Memory (preview) [M1] | Memory, lessons, skills [K5] |
| **MCP client / server** | Both (`claude mcp serve`) [A19] | Client only; server mode removed [O8] | Client via plugins [G3] | Client; CLI as an ACP server [M3] | MCP plus ACP [K5] |
| **Startup speed** | No figure; ±tens of ms in changelog [A3] | Rust; no figure | CLI "built in Go… snappier" [G1] | No figure | No figure |
| **Open source** | No (sandbox-runtime is Apache-2.0) | CLI Apache-2.0, Rust | No (Gemini CLI is Apache-2.0) | NOASSERTION | Crew Apache-2.0 |

---

## 8. Not confirmed, or gaps

- **Missing launch dates.** These pages showed no date when fetched: Opus 5.5 (*secondary*: Sep 22), Projects (*secondary*: Sep 17), Claude Tag (*secondary*: Jun 23), Cowork GA (*secondary*: Apr 9), Grok Build (*secondary*: May), and the Kiro Crew 0.7.0 changelog entry.
- **Secondary-only adoption figures:** Claude Code's $2.5B run-rate; Codex and Work's 35M weekly users; Copilot's 50M users and 4.7M paid; Managed Agents' $0.08 per session-hour.
- **Budget semantics:** no provider doc I read describes reserving a worst-case cost per call against a budget, or a session-level "taint, then hold for approval" after web reads. I searched for both and found nothing; this is a reading gap, not proof they don't exist.
- **Not documented in the pages I read:** whether Codex's local background server and Antigravity sessions survive restarts; Antigravity's memory, OpenTelemetry and SDK licence; the mechanism behind Kiro Crew's sandbox; whether Kiro Crew's "Jev" is TypeSafe's Jev; Jules activity after March 2026.

---

## Sources (fetched 2026-10-01 unless a date is given)

- **A1** code.claude.com/docs/llms.txt (docs index) · **A2** code.claude.com/docs/en/whats-new (weekly digests, Mar 23–Sep 11, 2026) · **A3** github.com/anthropics/claude-code/CHANGELOG.md (v2.1.285–2.1.286) · **A4** …/docs/en/agent-view · **A5** …/sandboxing · **A6** …/permission-modes · **A7** …/routines · **A8** …/scheduled-tasks · **A9** …/costs · **A10** …/monitoring-usage · **A11** …/memory · **A12** …/channels · **A13** …/agent-sdk/overview · **A14** claude.com/blog/claude-managed-agents (2026-04-08) and platform.claude.com/docs/en/managed-agents/overview · **A15** anthropic.com/claude-opus-5-5 · **A16** claude.com/blog/projects-redesigned · **A17** anthropic.com/news/introducing-claude-tag · **A18** GitHub API: anthropics/claude-code, claude-agent-sdk-python, sandbox-runtime · **A19** code.claude.com/docs/en/mcp
- **O1** learn.chatgpt.com/docs/changelog (Codex CLI 0.156–0.159.3, Sep 22–30) · **O2** learn.chatgpt.com/docs/whats-new · **O3** learn.chatgpt.com/docs/llms.txt · **O4** community.openai.com/t/devday-2026-announcements-and-developer-resources/1402006 (2026-09-29), /t/meet-the-all-new-codex-cloud/1402399, openai.com/index/devday-2026-recap · **O5** learn.chatgpt.com/docs/sandboxing and /sandboxing/auto-review · **O6** …/agent-approvals-security · **O7** …/customization/memories · **O8** …/mcp-server · **O9** …/open-source · **O10** …/config-file/config-advanced · **O11** …/pricing, /enterprise/usage-limits, /agent-configuration/speed · **O12** learn.chatgpt.com/docs/dots and openai.com/index/introducing-dots (2026-09-29) · **O13** openai.com/index/introducing-the-agents-api (2026-09-29) · **O14** openai.com/index/chatgpt-for-your-most-ambitious-work (2026-07-09) · **O15** openai-agents-python README; release v0.22.3 (2026-09-17) · **O16** learn.chatgpt.com/docs/automations · **O17** GitHub API: openai/codex, openai-agents-python
- **G1** developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli (2026-05-19) · **G2** antigravity.google/blog/introducing-google-antigravity-2 · **G3** antigravity.google/docs/cli/overview · **G4** antigravity.google/docs/changelog (2026-09-30) · **G5** antigravity.google/docs/sandbox · **G6** antigravity.google/docs/models and /pricing · **G7** antigravity.google/blog/remote-control-for-antigravity · **G8** jules.google/docs/changelog · **G9** github.com/google-gemini/gemini-cli (README, releases, discussion #27274, GitHub API) · **G10** GitHub API: google-antigravity/antigravity-cli
- **M1** docs.github.com/en/copilot/concepts/agents/cloud-agent/about-cloud-agent · **M2** …/security-governance-and-network-settings/risks-and-mitigations · **M3** …/agents/copilot-cli/about-copilot-cli · **M4** github.blog/news-insights/company-news/github-copilot-is-moving-to-usage-based-billing (2026-04-27) · **M5** GitHub API: github/copilot-cli · **M6** github.blog/changelog/2026-09-16-copilot-budget-increase-requests-are-generally-available
- **K1** docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/upgrade-to-kiro.html · **K2** kiro.dev/changelog/cli (2.22–2.26, Sep 16–30) · **K3** kiro.dev/pricing · **K4** kiro.dev/blog/tackling-technical-debt-at-scale-with-autonomous-mode · **K5** kiro.dev/blog/introducing-kiro-crew; infoq.com/news/2026/08/kiro-crew-coding-agents; siliconangle.com/2026/08/04/… · **K6** kiro.dev/changelog (Kiro Crew 0.7.0) · **K7** GitHub API: kirodotdev/KiroCrew
- **X1** x.ai/news/grok-build-cli; GitHub API: xai-org/grok-build · **X2** GitHub API: mistralai/mistral-vibe, release v2.25.8 (2026-09-23) · **X3** GitHub API: MoonshotAI/kimi-code and kimi-cli · **X4** docs.z.ai/devpack/overview; venturebeat.com (ZCode launch); autoclaw.z.ai

<!-- REPORT COMPLETE -->
