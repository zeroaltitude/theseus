# Off-provider agent harnesses compared with Theseus: research report, Scope B (as of 2026-10-01)

_Checked in on 2026-10-01 from the agents' working reports. Local paths became plain words or links, and the research agent's working line above the title was dropped; nothing else changed. Times are MST._

**Prepared for:** Tabitha/Claude, for the owner's Theseus comparison. **Research run 2** (run 1 was lost in the 00:32 WSL crash).
**Method.** OpenClaw comes first from its local docs (a local checkout of its docs at `2c69d06`, 2026-09-29, package `2026.9.6`). Its public presence and every other harness come from the web.
- **GitHub numbers** (stars, licence, dates) come from the GitHub API, fetched 2026-10-01 between 09:06 and 09:17 UTC.
- **Search results are not sources here.** The `web_search` provider returns Gemini-written summaries. I checked every load-bearing claim against a primary page (vendor blog, docs, changelog, press release) or a named outlet. Anything I couldn't check is marked **(unconfirmed)**.
- **No verdicts.** This report is facts only.

---

## 1. What changed in the field since mid-2026

**Ownership and consolidation**
- **SpaceX is buying Cursor.** It announced a $60B all-stock purchase of Anysphere on 2026-06-16 ([CBS](https://www.cbsnews.com/news/spacex-cursor-60-billion-ai-acquisition/)). TNW reports the deal has closed.
- **OpenAI is pulling its models from Cursor.** It notified SpaceX it will wind down its contract, with a proposed shutoff on **2026-11-12**, and no future models such as "Astra" will reach Cursor ([OpenAI](https://openai.com/index/our-decision-on-cursor-following-its-acquisition-by-spacex/)). Search coverage dates this to about 2026-08-28.
- **Windsurf is now Devin Desktop** (2026-06-02). Its Cascade agent was rewritten in Rust as **Devin Local**. Cognition raised $2B at a **$48B** valuation on 2026-09-08.
- **Roo Code shut down.** Its repo was archived on 2026-05-15, and the team pivoted to its "Roomote" cloud agent.
- **Manus is independent again.** Beijing ordered Meta to cancel its $2B purchase, and Manus 2.0 shipped 2026-09-28.
- **Letta** made "Letta Code" its flagship and is retiring server-side features.
- **Amp** split from Sourcegraph (2025-12-02).
- **Repos moved.** goose went to the Linux Foundation's Agentic AI Foundation (`aaif-goose/goose`), and opencode moved to `anomalyco/opencode`.
- **New openness and funding.** Warp open-sourced its client (AGPL-3.0, 2026-04-28). Zed reached 1.0 (2026-04-29). Factory raised $200M at $5B (2026-09-15).
- **OpenClaw went enterprise.** The OpenClaw Foundation announced **OpenClaw Enterprise (OCE)** on 2026-09-29. It is an MIT-licensed control plane that started inside OpenAI and is co-developed with Red Hat and NVIDIA.

**Features everyone now has**
- background or cloud agents
- event and schedule triggers (called "subscriptions" or "automations")
- `SKILL.md` skills
- hooks
- plugin bundles
- an MCP client
- increasingly, **ACP**, so one UI can host other vendors' agents

Local agents increasingly use OS sandboxes:
- Seatbelt on macOS
- Landlock with seccomp, or bubblewrap, on Linux

Network allowlists are spreading, but they are often marked unstable or enforced by a classifier.

**Features I found rarely or never** (relevant to Theseus's rows)
- **Hard per-session dollar budgets with worst-case reservation per call: none found.** What exists is visibility, org- or user-level monthly caps (Devin's ACU caps), or per-agent wallets (Manus Cue).
- **Published startup or shutdown latency budgets: none found** in any harness's docs.
- **A hold that gates actions after reading web text:** I found classifiers (Cursor), content wrapping and memory taint (OpenClaw core), and pattern detection (IronClaw). I found no documented hard hold on actions.
- **Rust is the trend for new daemons and agents.** goose, Zed, Warp, Moltis, ZeroClaw, IronClaw, and Devin Local (rewritten from Cascade) are all Rust.

---

## 2. Snapshot table

| Harness | Shape | Licence | Adoption (2026-10-01) | Latest dated signal |
|---|---|---|---|---|
| **OpenClaw** | Self-hosted Node/TS gateway daemon; chat apps, web, CLI, TUI, desktop and mobile apps | MIT (Foundation; the API shows "Other") | **391,081★**, 82,233 forks, 9,148 open issues | v2026.9.6 (tag 2026-09-22); OCE announced 2026-09-29 |
| **Cursor** | Closed IDE, CLI, cloud agents, SDK | Proprietary | $60B acquisition (SpaceX) | Rollouts and Security Review bots (latest changelog entry) |
| **Cognition** (Devin) | Devin Desktop (IDE), Devin Cloud, Devin CLI | Proprietary | $48B valuation; run-rate about $900M (Sept) | Sign in with ChatGPT, GPT-6.1 Sol (2026-09-29) |
| **Cline** | VS Code, JetBrains, CLI, desktop app, SDK | Apache-2.0 | 69,641★ | desktop-v0.0.40 (2026-09-30) |
| **Kilo Code** | VS Code, JetBrains, CLI, cloud agents; KiloClaw | MIT | 27,461★ | Active (pushed 2026-10-01) |
| **Roo Code** | VS Code extension | Apache-2.0 | 24,289★, **archived** | Archived 2026-05-15 |
| **Aider** | Terminal pair programmer | Apache-2.0 | 49,307★ | Last release v0.86.0 (**2025-08-09**); last push 2026-05-22 |
| **OpenHands** | SDK, CLI, web GUI, Agent Canvas, Cloud | MIT | 89,686★ | Active |
| **goose** (AAIF) | Desktop and CLI, Rust | Apache-2.0 | 54,830★ | v1.52.0 (2026-09-23) |
| **opencode** | TUI, desktop, IDE, GitHub Action | MIT | **211,227★** | v1.18.34 (2026-09-30) |
| **Amp** | CLI/TUI, macOS and iOS apps, cloud "orbs" | Proprietary (unconfirmed) | n/a | "Plaid Speed" (2026-09-29) |
| **Factory** | Droid CLI, Factory App, headless, Missions | Proprietary | $5B valuation | $200M round (2026-09-15) |
| **Warp** | Rust ADE with Oz cloud orchestration | AGPL-3.0 | 65,315★ | Open-sourced 2026-04-28 |
| **Zed** | Rust editor with Agent Panel and ACP | Mixed (API: NOASSERTION) | 91,152★ | 1.0 on 2026-04-29 |
| **Hermes Agent** | Python gateway, CLI/TUI, desktop app | MIT | **250,438★**, 47,655 open issues | Active (pushed 2026-10-01) |
| **Letta / Letta Code** | Memory-first harness (CLI and desktop) | Apache-2.0 | 24,988★ (server), 3,486★ (Letta Code) | Server last pushed 2026-09-10 |
| **Manus** | Web, desktop (Manus Studio), mobile; Cue | Proprietary | n/a | Manus 2.0 (2026-09-28) |
| **NanoClaw** | TS personal agent in containers, on Anthropic's Agent SDK | MIT | 30,865★ | Repo created 2026-01-31 |
| **ZeroClaw** | Rust single-binary personal agent | Apache-2.0 | 32,923★ | Repo created 2026-02-13 |
| **IronClaw** (NEAR AI) | Rust agent OS with WASM tool sandbox | Apache-2.0 | 12,638★ | Repo created 2026-02-03 |
| **Moltis** | Rust single-binary personal agent server | MIT | 2,880★ | Repo created 2026-01-29 |
| LangGraph / CrewAI | Frameworks | MIT / MIT | 42,547★ / 59,251★ | CrewAI 1.15.23 (2026-09-28) |

---

## 3. OpenClaw, in detail

Sources: local docs at checkout `2c69d06` (2026-09-29); GitHub API; [openclaw.ai blog, OCE, 2026-09-29](https://openclaw.ai/blog/openclaw-enterprise); [VentureBeat on OCE](https://venturebeat.com/orchestration/openclaw-launches-free-enterprise-control-plane-for-persistent-ai-agents-backed-by-openai-red-hat-and-nvidia).

**1. Shape**
- **One long-lived Gateway daemon** in Node.js and TypeScript. Node 24.16+ or 26 is required. It is "the single source of truth for sessions, routing, and channel connections," with one Gateway per host.
- **Protocol.** It serves a typed WebSocket JSON protocol (default `127.0.0.1:18789`). The schemas are in TypeBox, and Swift models are generated from them.
- **Surfaces:**
  - Control UI (web)
  - CLI and TUI
  - a macOS menu-bar app and Windows Hub
  - a Linux desktop companion (v2026.8.2)
  - iOS and Android "nodes" (camera, screen, location, voice)
- **Channels.** A2A, Reef, Telegram and WebChat ship in core. Official plugins add Discord, Slack, Signal, WhatsApp, iMessage, Teams, Matrix, IRC, Nostr, SMS, Google Chat and more.
- **Voice.** Discord voice channels support realtime voice, auto-join and voice-follow (`channels/discord.md`). There are also voice-wake and talk modes.
- **Licence and governance.** It is MIT, stewarded by the **OpenClaw Foundation**, a 501(c)(3), and has "no paid tier, no hosted service, no token." The local docs name donors and partners including Amazon, Atlassian, GitHub, Microsoft, NVIDIA, OpenAI, Red Hat and Tencent.
- **History.** The project was named Clawdbot, then Moltbot, then OpenClaw; the January 2026 rename followed a trademark request from Anthropic (`start/lore.md`). Its founder joined OpenAI in early 2026 ([Kilo blog](https://blog.kilo.ai/p/kiloclaw-hosted-openclaw), citing OpenAI's chief executive).
- **Release cadence.** v2026.8.1 ("OpenClaw 2.0") was tagged 2026-08-30, then v2026.8.2 (08-31), 9.1 (09-03), 9.2 (09-05), 9.3 (09-07), 9.4 (09-10), 9.5 (09-18) and 9.6 (09-22). These are local tag dates.

**2. Models**
- **Many providers.** There are about 70 provider docs (Anthropic, OpenAI, Google, Bedrock, OpenRouter, Mistral, Moonshot, Z.ai and others), plus local models (Ollama, vLLM, SGLang, llama.cpp, LM Studio) and any OpenAI- or Anthropic-compatible endpoint. Model failover is documented.
- **Vendor harnesses as runtimes.** OpenClaw can run turns through its built-in `openclaw` loop, the **Codex app-server** (native thread resume and steering), the **GitHub Copilot SDK** session loop, or the **Claude Code CLI** (`claude-cli`) over stdio. External harnesses (Claude Code, Gemini CLI, OpenCode, Cursor, Droid) run over **ACP** (`concepts/agent-runtimes.md`).
- **Subscriptions.** ChatGPT/Codex subscription auth works through OAuth.

**3. Autonomy and durability**
- **Scheduling:** persisted cron "Automations," heartbeat, webhooks, and Gmail PubSub triggers.
- **Delegation:** nested subagents with depth caps, and background sessions and tasks.
- **Managed git worktrees**, snapshotted before removal.
- **Cloud workers.** Ephemeral machines are launched through the bundled Crabbox provider. The transcript and accepted changes stay with the Gateway, and repo-only cloud sessions are supported.
- **Restart recovery.** Per the docs: "Conversations, transcripts, scheduled jobs, native subagent records, and queued outbound messages live on disk. After a gateway restart, eligible work interrupted mid-turn is detected and resumed automatically" (`gateway/restart-recovery.md`). Releases 9.2 and 9.6 extended this.
- **Updates.** "Atomic Updates" (9.5) check the next version before switching over.
- **Limit observed in this install.** Tabitha/Claude's memory notes from 2026-09-30 record that stopping the parent run or the gateway can kill running `sessions_spawn` children. That is a local observation, not a doc claim.

**4. Safety**
- **Sandboxing is off by default.** The docs say so plainly: "Default OpenClaw is a trusted single-operator assistant."
  - Modes are `off`, `non-main` and `all`. Backends are Docker or **NVIDIA OpenShell** (default-deny policy allowlists).
  - `workspaceAccess` can be none, ro or rw. Sandboxed exec defaults to `network: "none"`.
- **Tool policy.** It is layered (profiles, allow and deny, per-provider, sandbox-only), and "deny always wins." There is an "elevated" exec escape hatch.
- **Exec approvals:**
  - allowlist and ask modes
  - `strictInlineEval`, plus mandatory review of any heredoc
  - binary binding by resolved real path, plus a content hash for writable executables
- **Session permission modes:**
  - `read-only`
  - `guarded`: a human reviews after the allowlist fast path
  - `workspace`: an **LLM reviewer** returns allow, deny or ask-human, and three consecutive denials escalate to a human
  - `full`: requires `operator.admin`
- **Where approvals happen:** native chat approval cards or `/approve`, the Control UI, and the apps.
- **Access.** DM pairing is the default, and operator roles can require a sandbox.
- **Secrets.** SecretRefs and a `secrets` tool let the agent request a credential without ever seeing it. A secret egress proxy uses "bypass-surviving sentinels," and an opt-in traffic allowlist for Gateway-hosted exec arrived in August 2026.
- **Prompt injection:**
  - external content is wrapped in `EXTERNAL_UNTRUSTED_CONTENT` markers
  - chat-template special tokens are stripped
  - an outbound sanitizer removes leaked scaffolding
- **Taint.** Memory provenance marks network-sourced tool output as tainted, but it governs memory admission, not tool gating (`concepts/memory-provenance.md`).
- **Stated limits.** Native plugins run in-process and are unsandboxed. Egress allowlisting covers "cooperating traffic only." `openclaw security audit` checks for drift from a hardened baseline. The docs count 647 public repository advisories as of 2026-08-27.
- **This install differs.** The taint-based restrictions this session ran under come from a provenance plugin in the owner's install, not from core docs. Observation: in this session, read-only `grep` Bash calls still ran after web reads.

**5. Cost governance**
- **No per-session or per-turn dollar cap found in the docs.** I searched for budget, spend, maxCost and similar terms.
- **Visibility is extensive:**
  - `/status` shows estimated cost for API-key models
  - `/usage cost`, and the Control UI **Usage** page with complete 30-day reporting (v2026.9.6)
  - provider-reported today, 7-day and 30-day spend when Anthropic or OpenAI **Admin API** keys are present
  - quota windows for subscription plans
- **Provider-side budgets.** A "ClawRouter" provider shows monthly budget windows, but that budget lives at the provider.

**6. Memory**
- **Plain Markdown files.** These are `USER.md`, `MEMORY.md`, daily `memory/YYYY-MM-DD.md` notes and `DREAMS.md`. There is "no hidden state."
- **Search and consolidation.** One SQLite index supports **hybrid embedding-plus-keyword** `memory_search`. A background "dreaming" pass consolidates daily notes into `MEMORY.md`.
- **Deeper recall.** "Active Memory" runs a deep-recall sub-agent, only when the deterministic lane misses. There is also a user model and "standing intents."
- **Provenance and forgetting.** Provenance-gated promotion and `openclaw memory forget` are documented.
- **Plugins and imports.** Optional plugins add Honcho, LanceDB and a memory wiki. OpenClaw can import memory from Codex, Claude Code and Hermes.
- **Learning.** Self-learning turns corrections into skills through the Skill Workshop, in `off`, `propose` or `auto` modes.

**7. Extensibility**
- **Plugins.** About 150 SDK entrypoints are held under "shrink-only" budgets. The **ClawHub** registry runs VirusTotal and static scans.
- **Skills and standards.** Skills follow the AgentSkills spec, and installs auto-detect Codex, Claude and Cursor plugin bundle layouts.
- **Hooks**, and **Lobster** workflow pipelines.
- **MCP client** (stdio, SSE, Streamable HTTP, with OAuth) and an **MCP server** (`openclaw mcp serve`).
- **Other protocols:** **A2A 1.0**, **ACP** (as both client and host), and an OpenAI-compatible HTTP API (off by default).
- **Decision models.** These return typed choices, scores and Booleans. v2026.9.6 added **TypeSafe Jev** as an optional decision model, a direct parallel to Theseus's Jev client.

**8. Observability**
- The `diagnostics-otel` plugin exports **OpenTelemetry** metrics, traces and logs over OTLP/HTTP.
- There is also a Prometheus endpoint, message auditing, `openclaw logs`, and the Control UI Usage and Tasks views.

**9. Speed**
- **No published startup or latency budget.** The perf harness measures startup to HTTP readiness, but the docs publish no number.
- **Third-party size claim.** Moltis's README puts OpenClaw at about 1.1M app lines of code (tokei).

**10. Momentum**
- 391k stars, which makes it the most-starred project in this scope.
- **OCE (2026-09-29)** is "Kubernetes for agents": multi-tenancy, hard trusted/untrusted boundaries, LLM-based reviews and swappable harness, model and sandbox. It runs on docker-compose or Kubernetes and is aimed at internal pilots now, with 1.0 "later this year." OpenAI's internal "Androidclaw" agent "finds the PR and can quickly fix it… publish a PR… and merge it."
- **Ecosystem.** NVIDIA's NemoClaw distribution, and Kilo's hosted "KiloClaw."
- **A fast-growing field of clones** (§7).

---

## 4. Cursor (Anysphere, now owned by SpaceX)

Sources: [changelog](https://cursor.com/changelog), [pricing](https://cursor.com/pricing), [sandboxing blog](https://cursor.com/blog/agent-sandboxing), [run modes](https://cursor.com/docs/agent/security/run-modes), [2.5 changelog](https://cursor.com/changelog/2-5), [forum, 2026-09-10](https://forum.cursor.com/t/will-cursor-discontinue-agent-cli-after-grok-build-acquisition/171257).

1. **Shape.** Closed-source desktop IDE, plus the `agent` CLI (build `2026.09.08` shipped the week of 2026-09-08), cloud agents (web and iOS), Slack, and a TypeScript **Cursor SDK** (`@cursor/sdk`, public beta; [DevOps.com](https://devops.com/cursors-new-sdk-turns-ai-coding-agents-into-deployable-infrastructure/); launch date unconfirmed).
   - Under the same owner, SpaceXAI ships a separate **Grok Build** CLI (alpha). Cursor staff call the two "separate products today."
2. **Models.**
   - Multi-provider: Anthropic, Google, xAI Grok (the plans advertise "generous limits for Grok"), and its own Composer.
   - OpenAI models stop on **2026-11-12**.
   - Search coverage says bring-your-own-key works only on local paths, not on cloud agents **(unconfirmed)**.
3. **Autonomy.**
   - **Cloud agents** (2026-08-19 release) each run on their own VM. "Subscriptions" wake on PRs, Slack threads or schedules. `/goal` holds a long objective, subagents get their own VMs, and you can steer without interrupting.
   - **Projects** (beta, Aug–Sep 2026; exact date not shown) adds a coordinator agent that "delegates tasks to thousands of subagents" and "maintains context over months." It runs in the cloud, so "closing your laptop doesn't stop it."
   - **Self-hosted machines and team pools** hibernate idle workers. Agents can execute on AWS Lambda, Coder, Cloudflare, Daytona, Modal, Namespace, Vercel and E2B.
   - CLI `--worktree` and `agent persist` appear in search summaries **(unconfirmed)**.
4. **Safety.**
   - **Run modes:** **Auto-review** (default), **Allowlist** and **Run Everything**.
   - **The Auto-review classifier** runs on Gemini 3.5 Flash Lite, with Claude 4.5 Haiku as fallback. The docs say: "Auto-review is not a security boundary."
   - **OS sandbox:** macOS **Seatbelt**; Linux **Landlock plus seccomp**, used directly; Windows also covered, mechanism not confirmed. "Sandboxed agents stop 40% less often."
   - **Network controls (2.5):** `sandbox.json` modes are user-only, user-plus-defaults, or allow-all. Enterprise admins can enforce allow and deny lists, plus file and directory controls.
   - **Enterprise extras:** repo, model and MCP access controls, audit logs and privacy mode.
   - **Security Review bot** scans PRs for exploitable bugs (latest changelog entry).
5. **Cost.**
   - Plans: Hobby free; Individual **$20/mo** (Pro, Pro+ and Ultra tiers); Teams **$40/user/mo**; Enterprise custom with pooled usage.
   - Teams get "usage analytics." Per-user spend-limit mechanics were not verified this run.
6. **Memory.** Rules, "custom modes" (a pinned skill), and Project **shared-context files** that sync across every cloud and local machine; agents add what they learn.
7. **Extensibility.** MCP, skills and hooks. **Plugins** on the Cursor Marketplace (2.5) bundle skills, subagents, MCP servers, hooks and rules, with partners including AWS, Figma, Linear and Stripe. There is also an SDK and a Bot Development Kit.
8. **Observability.** Usage analytics, an AI code-tracking API and audit logs (Enterprise). OTel not found.
9. **Speed.** No startup figures published.
10. **Momentum.** The $60B acquisition. OpenAI's exit reduces the model choice on offer.

---

## 5. Cognition (Devin Desktop, Devin Cloud, Devin CLI)

Sources: [Windsurf is now Devin Desktop, 2026-06-02](https://devin.ai/blog/windsurf-is-now-devin-desktop), [blog index](https://devin.ai/blog), [pricing](https://devin.ai/pricing), [docs index](https://docs.devin.ai/llms.txt), [CLI sandbox](https://docs.devin.ai/cli/sandbox.md), [CNA, 2026-09-08](https://www.channelnewsasia.com/business/cognition-ai-raises-2-billion-48-billion-valuation-6371096).

1. **Shape.** Closed source, in three surfaces:
   - **Devin Desktop**: the former Windsurf IDE, whose default view is a Kanban **Agent Command Center**, with "Spaces" for shared context.
   - **Devin Cloud**: autonomous VMs.
   - **Devin CLI**: for macOS, Linux, WSL and Windows; `devin --cloud` steers cloud sessions (2026-09-21).
   - **Devin Local**: a from-scratch **Rust** rewrite of Cascade, "up to 30% more token efficient," with subagents. Cascade stayed available until July 1.
   - Desktop speaks **ACP** and hosts Codex, Claude Agent, OpenCode and in-house agents.
   - Also available: Slack, Teams and Microsoft 365 (2026-09-23), voice mode, and computer use.
2. **Models.**
   - OpenAI, Anthropic, Google, SpaceXAI and open models, plus its own **SWE-2**.
   - "Sign in with ChatGPT" (2026-09-29) uses a Plus or Pro plan for OpenAI usage.
   - **Fusion** pairs a frontier lead model with a cheaper "sidekick."
3. **Autonomy.**
   - Cloud VMs run Linux, macOS (Xcode), Windows or an Android emulator.
   - **Automations** trigger from Slack, GitHub, Linear, schedules or webhooks.
   - **Outposts** run self-hosted workers.
   - CLI subagents run in the foreground or background.
   - Concurrent sessions: up to 10 on Free and Pro, unlimited on Max and above.
4. **Safety.**
   - **CLI `--sandbox`** is OS-level. On Linux it needs `bwrap` and `socat`. On Windows it is unsupported, so the CLI **refuses to start** (fail closed).
   - Domain filtering through a loopback proxy is documented as "currently unstable."
   - Permission rules allow, deny or prompt for commands, files and MCP tools.
   - The secrets store holds org, personal, repo and session secrets, site cookies and TOTP codes.
   - VPC deployment and teamspace isolation are available.
5. **Cost.**
   - Plans: Free; Pro **$20**; Max **$200**; Teams **$80/mo plus $40 per full seat**; Enterprise custom.
   - Quotas refresh daily and weekly, and extra usage is billed "at API pricing."
   - **Per-user monthly ACU caps** come through usage policies, with personal analytics.
   - On 2026-09-28, Fusion and Normal modes became 30–40% cheaper.
6. **Memory.** **Knowledge** (scoped to repo or org), `AGENTS.md` (16 KiB auto-inject limit), Playbooks, `SKILL.md` skills, DeepWiki, and Session Insights.
7. **Extensibility.**
   - MCP marketplace plus custom stdio, SSE or HTTP servers.
   - A **Devin MCP server**, so outside tools can manage sessions, playbooks and knowledge.
   - CLI hooks, skills and plugins; an API; ACP.
8. **Observability.** Session Insights, analytics, and test video recordings. OTel not found.
9. **Speed.** No startup figures. "Up to 30% more token efficient" (Devin Local).
10. **Momentum.** A $2B Series E at **$48B** (2026-09-08). Run-rate grew from $492M in May to "nearly $900M." Bloomberg-sourced coverage reports $1B since. Windsurf was acquired 2025-07-14.

---

## 6. Open-source coding agents

### Cline
- **Shape.** Apache-2.0, 69,641★. Runs in VS Code and JetBrains (the JetBrains plugin is not open source), a CLI (`npm i -g cline`, interactive or headless), and a native **desktop app** for macOS and Windows (Tauri). The desktop app "schedule[s] routines"; desktop-v0.0.40 shipped 2026-09-30. **`@cline/sdk`** is "the same engine" behind every surface ([README](https://github.com/cline/cline)).
- **Safety.** Plan and Act modes. Every edit and command needs approval unless auto-approve is on. Checkpoints allow undo.
- **Memory and models.** `.clinerules`, an MCP marketplace, and bring-your-own-key across many providers.
- **Unconfirmed.** Search summaries mention an SDK launch in May 2026, plus enterprise SSO, OTel export and cost breakdowns.

### Kilo Code
- **Shape.** MIT, 27,461★. Runs in VS Code, JetBrains, a CLI and cloud agents.
- **Rebuild.** After Roo shut down, Kilo **rebuilt its VS Code extension "on the OpenCode server,"** the engine it shares with the Kilo CLI and Cloud Agents. That brought parallel execution, subagent delegation and an Agent Manager ([Kilo blog](https://blog.kilo.ai/p/thank-you-roo)).
- **KiloClaw.** Kilo's managed OpenClaw hosting reached GA with "500+ models" ([Kilo blog](https://blog.kilo.ai/p/kiloclaw-hosted-openclaw); [VentureBeat](https://venturebeat.com/orchestration/kilo-launches-kiloclaw-allowing-anyone-to-deploy-hosted-openclaw-agents-into)). Coverage dates GA to early 2026 **(exact date unconfirmed)**. Kilo is backed by one of GitLab's co-founders.

### Roo Code
- **Shut down.** The repo was archived on 2026-05-15 to go "all-in on Roomote," the team's cloud agent. It had about 3M installs, per Kilo's post. **Out of the running.**

### Aider
- Apache-2.0, 49,307★. A terminal pair programmer that git-commits each change.
- **The last GitHub release is v0.86.0, from 2025-08-09**, and the last push was 2026-05-22. No release in about 14 months.
- No sandbox, background agents or MCP were found. Treat it as dormant.

### OpenHands
- **Shape.** MIT, 89,686★. A Software Agent SDK, CLI (with headless mode), web GUI, "Agent Canvas," and OpenHands Cloud ([docs index](https://docs.openhands.dev/llms.txt)).
- **Sandboxes.** Docker (recommended), rootless Apptainer, Modal, and API- or cloud-based agent servers.
- **Safety.** A **security analyzer with a confirmation policy**, and a secret registry.
- **Memory.** Opt-in two-tier **persistent memory** and a context condenser.
- **Automations.** Cron plus GitHub and webhook events.
- **Observability.** Token, cost and latency metrics, and **OpenTelemetry tracing** (Laminar, MLflow, Honeycomb).
- **Momentum.** Joined the NVIDIA-led Open Secure AI Alliance on 2026-08-10.

### goose
- **Shape.** Now governed by the **Agentic AI Foundation**, a Linux Foundation directed fund formed 2025-12-09 with MCP, goose and AGENTS.md as founding projects. The repo moved to `aaif-goose/goose`. **Rust**, Apache-2.0, 54,830★; v1.52.0 shipped 2026-09-23.
- **Features.** Desktop and CLI; MCP "extensions"; reusable **recipes**; built-in Memory and "Chat Recall" extensions; `.goosehints` or `AGENT.md`. Works with many providers, including Ollama ([goose llms.txt](https://goose-docs.ai/llms.txt)). Topics list ACP.

### opencode (anomalyco, formerly sst)
- **Shape.** MIT, **211,227★**; v1.18.34 shipped 2026-09-30. TUI, desktop, IDE, and a GitHub Action.
- **Permissions.** Pattern-matched allow, ask or deny rules, where the last match wins. `--auto` approves anything not explicitly denied ([docs](https://opencode.ai/docs/permissions/)). No OS sandbox found in the docs **(unconfirmed)**.
- **Models.** Bring-your-own-key across many providers; **Zen**, a pay-as-you-go gateway; **Go** at $10/mo and **Go Plus** at $40/mo for open models (Kimi K3, GLM-5.3-Flash, DeepSeek V4.1 Flash, Qwen3.7 Plus and others; [opencode.ai/go](https://opencode.ai/go)).
- **Reach.** Its server now powers Kilo's extension.

---

## 7. Other coding agents

### Amp (Amp Inc.; split from Sourcegraph 2025-12-02, [Sourcegraph](https://sourcegraph.com/blog/why-sourcegraph-and-amp-are-becoming-independent-companies))
Licence unconfirmed (it appears closed). Dated launches from the [Chronicle](https://ampcode.com/chronicle):

- **Surfaces:**
  - CLI/TUI and editor extension
  - **macOS and iOS apps** (2026-08-28)
  - cloud **orbs** with desktop control (2026-09-04)
  - local **runners** that create worktrees and "know your secrets" (2026-09-22); the Mac app starts a runner itself (2026-09-24)
  - **Puck**, realtime voice control of agents (2026-08-18), and live team talk in threads (2026-08-31)
- **Models:**
  - the "Dial" maps modes to models: Opus 5.5 powers `medium` (2026-09-28), Fable 5.1 powers `ultra` (2026-09-01), and users can customize it (2026-09-10)
  - linking a ChatGPT subscription (2026-08-10)
- **Pricing:** **"Free Agent"**: Amp is free when you bring your own compute and model subscriptions or keys (2026-09-13). The education price is $10/mo, "half the usual price."
- **Extensibility and cost visibility:**
  - global plugins and skills (2026-08-11)
  - MCP in orbs (2026-08-19)
  - "Explain Usage": ask Puck where your tokens went (2026-08-21)
- **Speed:** "Plaid Speed," 6× faster inference for GPT-6 Astra (2026-09-29).
- **Not found:** sandbox mechanics and budgets.

### Factory (Droids)
Sources: [docs index](https://docs.factory.com/llms.txt), [TNW, 2026-09-15](https://thenextweb.com/news/factory-200m-5bn-valuation-ai-coding-agents).
- **Shape.** Closed source. Droid CLI, the Factory App (desktop, with worktrees), **Droid Exec** (headless), **Missions** (multi-feature orchestration with a "Mission Control"), Automations (schedule, Slack, GitHub), and Droid Control (terminal, browser and desktop automation).
- **Safety:**
  - **Autonomy Level** settings: Off, Low, Medium, High
  - testable permission rules
  - modes: Normal, Spec, Mission
  - an **OS-level sandbox** ("kernel-enforced" filesystem and network)
  - **Droid Shield**, which detects secrets in commits and pushes
- **Models.** Bring-your-own-key and local models. **Factory Router** picks a model per task, which the company says cuts token spend by more than 60%.
- **Cost.** **Agent Effectiveness** ties spend to output, with model-policy and autonomy defaults to control cost.
- **Extensibility.** MCP, skills, plugins and hooks.
- **Deployment.** Managed cloud, your own servers, or air-gapped.
- **Momentum.** $200M at **$5B** (2026-09-15); Factory 2.0 (April 2026).

### Warp
- **Open source.** It open-sourced its Rust ADE on 2026-04-28 ([newsroom](https://www.warp.dev/newsroom/2026/4/28/warp-open-sources-its-agentic-development-environment)); the GitHub API reports **AGPL-3.0**, 65,315★.
- **Oz.** Warp's cloud agent orchestration (newsroom path dated 2026-02-10) runs a public contribution model: **Oz agents triage issues, plan, write code and open PRs in the open**. OpenAI is the "flagship sponsor."
- **Unconfirmed:** first-class hosting of Claude Code, Codex, Gemini CLI and OpenCode, and a "Warp Factories" pipeline product.

### Zed
- **Zed 1.0** shipped 2026-04-29 ([blog](https://zed.dev/blog/zed-1-0)). Rust with the GPUI framework; 91,152★; licence mixed (the API reports NOASSERTION).
- **Agents.** Parallel agents and edit predictions. **ACP** brings in Claude Agent, Codex, OpenCode "and more recently Cursor."
- **Zed for Business:** RBAC and centralized billing.
- **Not found:** cloud or background agents, budgets, or a native sandbox.

---

## 8. Memory-centric and personal agents

### Hermes Agent (Nous Research)
Sources: [README](https://github.com/NousResearch/hermes-agent), [docs index](https://hermes-agent.nousresearch.com/docs/llms.txt); OpenClaw's own comparison page.

1. **Shape.** Python, MIT, **250,438★**, 53,548 forks, **47,655 open issues**; the repo was created 2025-07-22.
   - **One gateway process** serves Telegram, Discord, Slack, WhatsApp, Signal and email.
   - Also a CLI/TUI, a **Hermes Desktop** app, a web dashboard and native Windows.
   - **Voice mode works in Discord voice channels**, and there is a "Hey Hermes" wake word.
2. **Models.** Any provider ("no lock-in"). **Nous Portal** is a subscription for 300+ models plus a "Tool Gateway" covering search, image generation, TTS and browser. Per TechCrunch (2026-07-13, cited in OpenClaw's docs), paid tiers run $20–$200/mo, and Nous was in talks to raise $75M at a $1.5B valuation.
3. **Autonomy.**
   - A cron scheduler that delivers results to any platform.
   - Isolated **subagents**, and Python scripts that call tools over RPC.
   - **Seven terminal backends:** local, Docker, SSH, Singularity, Modal, Daytona and Vercel Sandbox. Modal and Daytona "hibernate when idle."
4. **Safety.**
   - Dangerous-command approval, user authorization and container isolation.
   - **Docker network egress isolation** to allowlisted hosts.
   - **Checkpoints and rollback** through shadow git repos.
   - Its SECURITY.md (snapshot cited 2026-08-27) states: "The only security boundary against an adversarial LLM is the operating system."
5. **Cost.** `/usage` and `/insights` commands. No budget cap found.
6. **Memory: the main selling point.** "A closed learning loop":
   - agent-curated memory with nudges
   - **autonomous skill creation and self-improvement**
   - FTS5 session search with LLM summaries
   - **Honcho** user modeling, with other providers available (OpenViking, Mem0, Hindsight and more)
   - `SOUL.md`
7. **Extensibility.** MCP with per-tool filtering, ACP, agentskills.io, and plugins. It imports Claude Code and Codex setups, and **`hermes claw migrate`** imports from OpenClaw.
8. **Observability.** A web dashboard and insights. OTel not found.
9. **Speed.** No figures found.
10. **Momentum.** The second most-starred project in scope, which OpenClaw's own docs treat as its main rival.

### Letta / Letta Code
- **The pivot.** "Letta's Next Phase" ([letta.com](https://www.letta.com/blog/our-next-phase/), undated; server features "deprecated by mid-April") makes **Letta Code** the flagship: a "model-agnostic agent harness with persistent memory."
- **How memory changes.** Memory moves to **MemFS**, git-backed "context repositories" edited with bash. Sleep-time compute moves client-side, as subagents. Skills become the main packaging unit.
- **What is being retired:** templates, server-side MCP integrations and tool rules.
- **Repos.** Letta Code (TypeScript, Apache-2.0) has 3,486★. The Letta server has 24,988★; it was last pushed 2026-09-10, accepts PRs from collaborators only, and shows 0 open issues.

### Manus
- **Ownership.** Meta agreed to buy Manus at the end of 2025. Beijing ordered the $2B deal cancelled, about five months before September. Manus is now based in Singapore.
- **Manus 2.0** (launched Monday, 2026-09-28; [TNW](https://thenextweb.com/news/manus-2-0-cue-ai-agents-email-phone-wallet), [InfoWorld](https://www.infoworld.com/article/4228301/metas-ex-launches-agent-rival-to-metas-muse.html)):
  - a new **Cascade** harness. In one tested configuration it used 23.2% fewer tokens, finished 28.2% faster and cost 32% less.
  - event-triggered **Automations**
  - a persistent **Cloud Computer**
  - **Manus Studio** on desktop
- **Cue.** A personal-agents app where "each agent has its own email, phone number, wallet, and computer" and pays "within a budget the user sets." It is in early access. Meta launched its own personal agent, **Muse**, in September 2026.

### Personal always-on daemons that gained traction in 2026 (the "Claw" field)
- **NanoClaw** (`nanocoai/nanoclaw`; MIT; 30,865★; created 2026-01-31). "A lightweight alternative to OpenClaw that runs in containers… runs directly on Anthropic's Agents SDK." It has memory and scheduled jobs. VentureBeat covered its Slack launch.
- **ZeroClaw** (Rust; Apache-2.0; 32,923★; created 2026-02-13) ([README](https://github.com/zeroclaw-labs/zeroclaw)).
  - A single binary with about 20 providers and **30+ channels**.
  - **Supervised autonomy by default:** medium-risk actions need approval, and high-risk ones are blocked.
  - OS sandboxes: **Landlock, Bubblewrap, Seatbelt or Docker**.
  - **Cryptographic receipts on every tool call.**
  - Installs as a system service; supports hardware GPIO.
  - The widely repeated "<5 MB RAM" figure is **unconfirmed**.
- **IronClaw** (NEAR AI; Rust; Apache-2.0; 12,638★; created 2026-02-03) ([README](https://github.com/nearai/ironclaw)).
  - **WASM sandbox** for untrusted tools, with capability permissions.
  - **Secrets injected at the host boundary with leak detection.**
  - Endpoint allowlisting and prompt-injection pattern detection.
  - Docker jobs with per-job tokens.
  - Routines, heartbeat and self-repair.
  - **"Dynamic tool building"**: describe a tool and IronClaw builds it as WASM.
  - Hybrid full-text and vector search (Reciprocal Rank Fusion), and a full audit log.
- **Moltis** (Rust; MIT; 2,880★; created 2026-01-29) ([README](https://github.com/moltis-org/moltis)).
  - **One binary**, "~270K Rust LoC across 59 crates." The agent runner fits in about 7.5K lines.
  - Sandboxing through Docker, Podman, Apple Container or WASM.
  - Passkey and vault authentication, built-in voice, SQLite with FTS and vector memory, MCP, cron, and **OpenClaw import**.
  - It reached the Hacker News front page.
- **KiloClaw**: managed OpenClaw hosting (see §6).

---

## 9. Frameworks, for contrast

- **LangGraph** (MIT, 42,547★). A runtime for stateful graphs with checkpointing. LangChain now positions three layers: LangGraph as the runtime, LangChain as the framework, and **Deep Agents** as the harness, with hosting under **LangSmith Deployment**, the renamed LangGraph Platform. Feature details are **unconfirmed**; they come from a search summary.
- **CrewAI** (MIT, 59,251★; 1.15.23 on 2026-09-28). A multi-agent orchestration framework with an enterprise "AMP" platform (**unconfirmed**).
- **Neither is a harness** in Theseus's sense. Neither ships its own approval gate, ledger or always-on daemon.

---

## 10. Theseus's rows against the field (facts only)

| Theseus row | Closest matches found (with sources above) | Not found anywhere |
|---|---|---|
| **One long-lived daemon** with conversations, tasks and wakes in a WAL store; jobs and PTYs survive a restart; in-place binary swap | **OpenClaw** (on-disk sessions, jobs and subagents; automatic resume of eligible interrupted turns; Atomic Updates). **ZeroClaw, IronClaw, Moltis** (Rust service daemons). Cloud VMs that survive a laptop closing: Cursor cloud agents and Projects, Devin Cloud, Amp orbs, Manus Cloud Computer, Hermes on Modal or Daytona | A verified crash simulator; job and PTY survival across a **binary swap** |
| **Speed as a gated goal** (cold start p95 ~30 ms, shutdown ~75 ms, swap <200 ms) | Rust single-binary peers (Moltis, ZeroClaw, IronClaw, goose); vendor speed claims (Amp Plaid 6×; Devin Local 30% token efficiency) | **Any published startup or shutdown budget, or a CI gate on one** |
| **Gate posture** (open, notify or approve, per-tool overrides, a floor for secrets and own store) | OpenClaw (tool policy with deny-wins; exec approvals; permission modes including an LLM reviewer); Cursor run modes (Auto-review classifier, "not a security boundary"); Factory autonomy levels; ZeroClaw (supervised by default); Devin and opencode (allow, ask, deny rules) | — |
| **Hold after reading web text**, cleared by `/trust` | OpenClaw core wraps external content and taints memory (memory only); IronClaw pattern detection; Cursor's classifier on non-sandboxed calls | **A documented hard action hold triggered by web taint** |
| **Approvals from Discord, CLI or web** | OpenClaw (chat approval cards, `/approve`, Control UI, apps); Hermes (dangerous-command approval across gateway platforms); Cursor and Devin (in-app) | — |
| **Per-session $ budget with worst-case reservation per call; reset needs approval; full ledger** | Devin **per-user monthly ACU caps**; Manus **Cue per-agent budgets**; Factory model policy and Agent Effectiveness; OpenClaw cost visibility only; OpenHands cost metrics | **Per-session or per-call worst-case reservation** |
| **Observability** (ledger, OTel, Observatory) | OTel: **OpenClaw**, **OpenHands**. Dashboards: Cursor and Devin analytics, Hermes dashboard, Amp "Explain Usage." Tool receipts: ZeroClaw. Audit logs: IronClaw, Cursor Enterprise | — |
| **Native unprivileged sandbox** (userns, seccomp, fresh `/proc`, cgroups, egress proxy with allowlist and public-only resolver) | Cursor (Seatbelt; Landlock and seccomp); Devin CLI (bwrap with a loopback proxy, filtering "unstable"); Factory (kernel-enforced); ZeroClaw (Landlock, Bubblewrap, Seatbelt); IronClaw (WASM); OpenClaw (Docker or OpenShell, `network: none`); Hermes (Docker egress isolation) | A **public-only resolver** in any of these |
| **MCP client and server** | Client: nearly all. Server: **OpenClaw** (`openclaw mcp serve`), **Devin MCP** | — |
| **Judgment client (Jev)** | **OpenClaw Decision Models with TypeSafe Jev (v2026.9.6)**; classifier gates (Cursor Auto-review, OpenClaw's workspace reviewer) | — |
| **Memory** (BM25 and entity index; FSRS-6; spreading activation; a **memory exam**) | Hermes (FTS5, Honcho, autonomous skills); OpenClaw (hybrid search, dreaming, Active Memory, provenance); Letta (MemFS, sleep-time); IronClaw (RRF hybrid); Moltis (FTS plus vector); OpenHands (two-tier) | **A published memory exam or benchmark gating rollout** (OpenClaw's docs cite LongMemEval as design rationale only) |
| **Self-building** (a builder daemon fixed a Theseus bug end to end, about 14 min for $3.22) | Warp's **Oz agents implementing community issues in the open**; OpenAI's **Androidclaw** on OpenClaw ("finds the PR… merge[s] it"); Cursor Projects coordinator; Factory's "self-improving software development" | — |
| **Planned: integrity labels** (content trust flowing through files, jobs and sessions) | OpenClaw's memory taint (memory only); ZeroClaw receipts | **Cross-file, job and session label propagation** |
| **Planned: control-plane separation** | **OCE** ("hard boundaries between trusted and untrusted workloads"); OpenClaw's trusted gateway with untrusted execution; IronClaw's host-boundary secrets | — |
| **Planned: cloud "hands"** (AWS workers, IaC, guardrails enforced twice) | Cursor on Lambda and others, Devin Outposts, OpenClaw cloud workers (Crabbox), Hermes on Modal or Daytona | **Agent-owned cloud account with SCPs as a second enforcement layer** |
| **Planned: Discord voice** | **OpenClaw** (Discord voice channels, realtime); **Hermes** (voice mode in Discord voice channels); Amp Puck (voice control); Devin (voice mode) | — |
| **Planned: schedules and task graphs** | OpenClaw automations; Hermes cron; Cursor subscriptions and Projects; Devin Automations; Factory Missions; OpenHands automations; IronClaw routines; Cline desktop routines | — |
| **Planned: self-extension** | IronClaw dynamic WASM tools; Hermes autonomous skills; OpenClaw self-learning (Skill Workshop, propose or auto) | — |
| **Planned: TUI** | OpenClaw, Hermes, opencode, Cline CLI, Amp | — |

### Unconfirmed items, collected
- Cursor's SDK launch date, CLI `--worktree` and `agent persist` flags, BYOK limits, and spend-limit mechanics.
- Cline's enterprise features (SSO, OTel, cost breakdowns) and SDK launch date.
- Warp's third-party agent hosting and "Factories."
- KiloClaw's GA date.
- Whether opencode has an OS sandbox.
- ZeroClaw's "<5 MB RAM."
- Manus's "My Computer" local-execution details.
- LangSmith Deployment and CrewAI AMP features.
- Cognition's "$1B ARR," which is reported but not stated in a primary source I read.
- The exact Cursor acquisition closing date.

<!-- REPORT COMPLETE -->
