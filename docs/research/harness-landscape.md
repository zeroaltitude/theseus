# Theseus among the harnesses

*How Theseus compares with the provider-made and independent agent harnesses, as of 2026-10-01. Tabitha/Claude's analysis,
for the owner (theseus-zfg).*

_Checked in on 2026-10-01. One local path became a link to the research reports; nothing else changed. Its PDF was "Theseus among the harnesses"._

---

## The answer on one screen

**In 2026 the field converged on Theseus's shape.** The major tools now run a long-lived local service, not a
process per chat:
- Claude Code runs a supervisor and installs it as an OS service.
- Codex's CLI starts a background server (0.157.0, 2026-09-25).
- Amazon's Kiro Crew is a gateway daemon with a durable queue, "inspired by the momentum of OpenClaw."
- OpenClaw has a 391,000-star gateway.

You reach these agents by chat: Claude Code's Channels (Discord, Telegram, iMessage), OpenAI's always-on "dots",
Claude Tag in Slack. The work runs in an OS sandbox behind an egress allowlist. So Theseus's architecture is
*confirmed* by the market, not novel. That is good news: the bet was right.

**Where Theseus stands alone** is a different axis from the one the field competes on. The others compete on
reach and autonomy: more surfaces, more models, more agents at once. Theseus competes on **guarantees**. As far as
two researchers reading primary sources on 2026-10-01 could find, no other harness does any of these:

1. **Money is a gate, not a report.** Every call reserves its worst case against the session's budget before it
   runs; reaching the limit asks, and a reset needs approval. The field offers visibility, plus monthly caps per
   organisation or user.
2. **Speed is a tested contract.** Cold start p95 is about 30 ms, shutdown about 75 ms, a binary swap under 200 ms,
   and the test gate fails on a miss. No harness publishes a startup budget, let alone gates its tests on one.
3. **Reading the web stops the hands.** After a session reads external text, its acting calls wait for a human.
   The field leans on classifiers here instead. Cursor's own docs say of theirs, "Auto-review is not a security
   boundary."
4. **Memory has to pass an exam before it ships.** Everyone has memory now. Nobody publishes a test that decides
   whether memory helps, let alone gates its rollout on one.
5. **(Planned) The agent owns its cloud account, with guardrails enforced twice**: Theseus's gate, then AWS's own
   guard policies and SCPs.

**Where it trails, and what to do about it:**
- **Reach.** Theseus has three surfaces and two providers; OpenClaw has dozens of surfaces and about 70
  providers. This is mostly by design. A third provider is cheap insurance: OpenAI is pulling its models from
  Cursor on 2026-11-12, a reminder that any one provider can go away.
- **The second-model approver.** In 2026 the field moved approvals to a classifier: Claude Code's auto mode is the
  default, and Codex and Cursor ship reviewer agents. Theseus's answer, Jev on a shadow-to-live ladder, is the more
  rigorous design, but it isn't live. The pilot's builder lost an hour to approval cards; this is where that
  friction gets solved.
- **Credentials that never enter the job.** Claude Code, OpenClaw and IronClaw give a sandboxed command a stand-in
  value, and swap in the real secret at the proxy, only for the hosts allowed. Theseus's L1 hands the job the real
  value. **Fix this before the AWS hands write.**
- **ACP.** The Agent Client Protocol is how editors and gateways host other vendors' agents (Zed, Devin Desktop,
  Kiro Crew, OpenClaw, Copilot CLI). Theseus speaks MCP but not ACP. It's worth adding after v1.

**My verdict.** Theseus is not a smaller Claude Code or a Rust OpenClaw. It is the **governed personal daemon**:
- one operator;
- hard limits that hold whether or not a model behaves;
- a ledger that can account for every cent and every action;
- reliability engineered and tested, not hoped for.

The field's own documents concede that its safety is soft:
- Cursor: "Auto-review is not a security boundary."
- Hermes: "The only security boundary against an adversarial LLM is the operating system."
- OpenClaw ships with sandboxing off: "a trusted single-operator assistant."

Theseus is the harness that takes those sentences seriously. For v1, keep that the headline:
1. Finish the guarantees.
2. Put Jev to work on approvals, in shadow first.
3. Move credentials behind the proxy before the AWS hands write.
4. Publish the numbers, because nobody else does.

---

## How this was gathered, and how far to trust it

- **Two research runs on 2026-10-01**, both re-runs of runs the 00:32 VM crash killed:
  - **Scope A** covered the provider-made harnesses: Anthropic, OpenAI, Google, GitHub/Microsoft, Amazon, and
    briefly xAI, Mistral, Moonshot and Z.ai.
  - **Scope B** covered the independents: OpenClaw, Cursor, Cognition, the open-source coding agents, Amp,
    Factory, Warp, Zed, the memory-centric agents, the "Claw" daemons, and two frameworks.
  - Both fetched official docs, changelogs, release notes, and the GitHub API between 09:04 and 09:20 UTC.
  - Search engines were used only to *find* sources. A claim with only a secondary source is marked as such.
- **"Not found" is a reading gap, not proof.** The researchers searched specifically for:
  - per-call budget reservation;
  - a hold after web reads;
  - published startup budgets;
  - memory exams.

  They found none. That makes Theseus's lead on those rows a strong **hypothesis**, not a **conclusion**. Where I
  say "alone", read "alone as far as the public docs show".
- **The OpenClaw facts come from its local docs** (checkout `2c69d06`, 2026-09-29). Two observations come from our
  own install, marked as such: children killed with their parent, and the taint hold from our provenance plugin.
- The full reports, with every source, are [landscape-providers.md](landscape-providers.md) and [landscape-independents.md](landscape-independents.md).

---

## 1. The field in October 2026

**Six things changed since mid-2026:**

1. **Harnesses became services.**
   - Claude Code runs every session as a process under a supervisor that can be installed as an OS service. Shells,
     workflows and subagents carry over when a session's process restarts.
   - Codex 0.157.0 auto-starts a background server.
   - Kiro Crew (Amazon, open source, August 2026) is a gateway daemon with a durable queue and an append-only
     session ledger, and its auto-updates wait for turns to go idle.
   - The "always-on agent" became a consumer product: OpenAI's **dots** (2026-09-29) each have their own cloud
     computer, "work 24/7", and "decide when to pause and wake up."
2. **Chat became a surface for coding agents.**
   - Claude Code Channels push Telegram, Discord or iMessage into a running session.
   - Claude Tag is one Claude per Slack channel. Anthropic says "65% of our product team's code is created by our
     internal version of Claude Tag."
   - Devin added Slack, Teams and Microsoft 365. Kiro Crew speaks Slack, Telegram, Discord, WeCom and Webex.
3. **A second model now decides approvals.**
   - Claude Code's classifier-based auto mode became the default for Pro, Max and Team on Aug 14.
   - Codex's auto-review (internally "Guardian") reviews sandbox escalations, and the dots use it.
   - Cursor's Auto-review runs on Gemini 3.5 Flash Lite.
   - OpenClaw's `workspace` mode has an LLM reviewer that escalates to a human after three denials.
4. **OS sandboxes and egress proxies became table stakes.**
   - Seatbelt on macOS; bubblewrap, Landlock and seccomp on Linux.
   - Claude Code, Codex and Antigravity ship domain allowlists.
   - Codex blocks private destinations by default, with a DNS-rebinding check.
5. **Providers now sell the harness itself.**
   - Claude Managed Agents (public beta 2026-04-08).
   - OpenAI's Agents API (2026-09-29), "powered by the open-source Codex harness."
   - OpenClaw Enterprise (2026-09-29), a multi-tenant control plane co-developed with Red Hat and NVIDIA.
6. **Consolidation, and Rust.**
   - SpaceX is buying Cursor ($60B), and OpenAI is withdrawing its models from Cursor (proposed shutoff
     2026-11-12).
   - Windsurf became Devin Desktop; Cognition is valued at $48B.
   - Roo Code shut down. Gemini CLI gave way to the closed Antigravity CLI.
   - New daemons and agents are increasingly Rust: Codex CLI, Grok Build, goose, Zed, Warp, Devin Local (a rewrite
     of Cascade), and the Claw clones ZeroClaw, IronClaw and Moltis.

**The interop layer settled.** Everyone has:
- an MCP client;
- `SKILL.md` skills;
- hooks;
- plugin bundles.

**ACP** became how one tool hosts another vendor's agent. Kiro Crew drives Claude Code, Codex, OpenCode and goose
over ACP, and OpenClaw does the same.

**The momentum, in GitHub stars on 2026-10-01:**

| Project | Stars |
|---|---|
| OpenClaw | 391,081 |
| Hermes Agent | 250,438 |
| opencode | 211,227 |
| Claude Code (issues and plugins repo) | 148,765 |
| Codex | 127,475 |
| Gemini CLI | 107,211 |

Theseus is private and has one operator. It isn't in this race, and shouldn't be.

---

## 2. Head to head

The closest comparisons:
- the two provider flagships, **Claude Code** and **Codex**;
- the two biggest independent daemons, **OpenClaw** and **Hermes**;
- the structural twin, **Kiro Crew**;
- the Rust single-binary **Claws**: ZeroClaw, IronClaw and Moltis.

| | **Theseus** | **Claude Code** | **Codex** | **OpenClaw** | **Kiro Crew** | **Hermes** | **Rust Claws** |
|---|---|---|---|---|---|---|---|
| Long-lived service | One daemon: conversations, tasks, wakes | Supervisor, a process per session, optional OS service | Background server (0.157) | Gateway daemon | Gateway daemon | Gateway process | Single binary, system service |
| Survives a crash or restart | WAL store; crash simulator; jobs and PTYs survive a binary swap | Restarts carry shells, workflows, subagents; resume after shutdown | Cloud: laptop-closed; local: not documented | Resumes eligible interrupted turns; we've seen children die with their parent | Durable queue; updates wait for idle | Not documented | Not documented |
| Surfaces | Discord, web, CLI | CLI, IDEs, desktop, web, mobile, Channels, Slack (Tag) | CLI, IDE, ChatGPT app, Slack, Linear, GitHub | Web, CLI, TUI, desktop and mobile apps, ~20 chat channels | Desktop, web, TUI, five chat apps | Telegram, Discord, Slack, WhatsApp, Signal, email; CLI, desktop | Many channels (ZeroClaw: 30+) |
| Providers | Anthropic, Z.ai | Claude only (5 platforms) | OpenAI plus custom (Ollama, LM Studio) | About 70, plus local | Through the agents it hosts | Any | ~20 (ZeroClaw) |
| Sandbox | Native L1: userns, seccomp, fresh `/proc`, cgroups; 3.7 ms start (built, wiring in) | Seatbelt; bubblewrap and seccomp | Seatbelt; bubblewrap; native Windows | Off by default; Docker or OpenShell | "OS-level" (mechanism not stated) | Docker and six other backends | Landlock, bwrap, Seatbelt; WASM (IronClaw) |
| Egress | Allowlist proxy, public-only resolver | Allowlist proxy, credential stand-ins | Allowlist; private IPs blocked; rebinding check | `network: none` in the sandbox; secret proxy | Not stated | Docker allowlist | Endpoint allowlist (IronClaw) |
| Approvals | Posture per tool, a floor, cards from Discord, web, CLI | Six modes plus a classifier | Policies plus a reviewer agent | Modes, LLM reviewer, chat cards | Permissions, gates | Dangerous-command approval | Supervised by default (ZeroClaw) |
| Hold after web reads | **Hard hold until `/trust`** | Classifier watches for "hostile content" | Cached web search | Taint governs memory only | "Suspicious-pattern blocking" | Not found | Pattern detection (IronClaw) |
| Money | **Per-session budget, per-call worst-case reservation, reset by approval** | `--max-budget-usd` (semantics not documented), org caps | Credits, per-user limits | Visibility only | Not detailed | `/usage` only | Not found |
| Speed | **Gated budgets** | No figure | No figure | No figure | No figure | No figure | No figure |
| Memory | BM25 and entity index, FSRS-6, activation, **an exam** (wiring in) | Files plus auto memory | Files plus local memories | Files, hybrid search, dreaming, Active Memory | Memory, lessons, auto-skills | FTS5, Honcho, autonomous skills | Hybrid FTS and vectors |
| MCP / ACP | MCP client and server; no ACP | MCP both | MCP client; server removed | MCP both; ACP both; A2A | MCP; ACP | MCP; ACP | MCP |
| Judgment model | Jev client, shadow ladder (built) | Classifier | Reviewer agent | **TypeSafe Jev** decision models (2026.9.6) | "Decisions (Jev)" preview | — | — |
| Licence | Private | Proprietary | CLI Apache-2.0 | MIT | Apache-2.0 | MIT | Apache-2.0 / MIT |

---

## 3. Where Theseus leads

Each lead is rated: a **conclusion** where the evidence is direct, a **hypothesis** where it rests on not finding
something.

### 3.1 Money as a gate (hypothesis: unique; conclusion: uncommon)

**What the field does:**
- Organisation, group and member caps: Claude Code for Team and Enterprise, Copilot's AI Credits with budget
  increase requests (GA 2026-09-16), Devin's monthly ACU caps per user.
- Credit pools that run out: Codex, Antigravity, Z.ai.
- Claude Code's `--max-budget-usd`, whose docs don't say whether it reserves before a call or checks after.
- Manus's Cue: "a budget the user sets" per agent, in early access.
- OpenClaw shows cost in detail and caps nothing.

**What Theseus does:** before each call it reserves the call's worst case (its maximum output at list price)
against the session's budget. A call that won't fit asks the operator, and since kks, a call bigger than the
whole limit says so and names both remedies. A reset needs approval, and the ledger accounts for every cent.

**Why it matters:** for an agent that runs while you sleep, a check after the call finds a runaway loop's cost on
the bill. A reservation before the call bounds it. The difference is the same as between a speed camera and a
governor. The field built cameras.

### 3.2 Speed as a contract (hypothesis: unique)

**What the field publishes:** no startup or shutdown figure from any harness in either scope. Claude Code's
changelog mentions deltas of "~30ms" and "~60ms". Rust peers (Codex CLI, Grok Build, goose, Moltis, ZeroClaw,
Devin Local) are probably fast, but none states a budget, and none gates its tests on one.

**What Theseus does:**
- Cold start p95 about 30 ms, clean shutdown about 75 ms, a binary swap under 200 ms.
- The lifecycle bench is in the gate, so a regression fails the build. Every gate this morning reran it: 7.0 to
  7.8 s for the bench, OK every time.

**Why it matters:** a daemon you restart freely is one you update freely. Updates flow because restarts cost
nothing, and that is part of how Theseus ships a fix in minutes.

### 3.3 Reading the web stops the hands (hypothesis: unique as a hard hold)

**What the field does:**
- Claude Code's auto-mode classifier blocks actions that appear "driven by hostile content Claude read", but it
  falls back to prompting after 3 blocks in a row.
- Codex defaults web search to a **cached** index "to reduce exposure to prompt injection."
- Copilot strips hidden characters from issue text.
- IronClaw detects injection patterns.
- OpenClaw's core taints network-sourced output, but the taint governs what enters memory, not which tools run.

**What Theseus does:** a session that reads external text latches. From then on its acting calls wait for
approval, until the operator says `/trust`.

**Where the idea came from:** the owner's own OpenClaw install runs this as the provenance plugin we maintain. Theseus
is where the idea becomes core, not a plugin. Anthropic says Opus 5.5 is "more resistant than Opus 5 to prompt
injection". That helps, and it's a probability. The hold is a guarantee.

### 3.4 Memory has to pass an exam (hypothesis: unique)

**What the field does:** everyone has memory.
- File memory: CLAUDE.md, AGENTS.md, Claude Code's auto memory, Codex's local memories.
- Hybrid search and consolidation: OpenClaw's hybrid search, "dreaming" and Active Memory; IronClaw's RRF hybrid.
- User models and git-backed memory: Hermes's Honcho user model and autonomous skills; Letta's git-backed MemFS.

Nobody publishes a test that says whether memory helps. OpenClaw's docs cite LongMemEval only as design rationale.

**What Theseus does:**
- Theseus measured the headroom first. On synthetic tasks, memory is worth +74 points: 26% without it, 100% with
  the oracle's notes.
- Exam v2 separates keyword search from the oracle, so vector search, reranking and time-aware ranking each have
  to earn their place.
- Recall goes into prompts through shadow, then canary, then live, each step judged by the exam.

The embedding spike this morning is the method at work. It measured candle against tract on this machine and
chose candle on the numbers. It also found that a query's 85 to 90 ms embedding means recall's 60 ms target
can't hold with the vector search waited on. That got found before it was built, not after.

### 3.5 Durability you can test (conclusion: rarer than it looks)

**What the field does:**
- Claude Code's supervisor restarts sessions, carries shells and subagents across a process restart, and resumes
  from the transcript after a shutdown.
- OpenClaw resumes "eligible" interrupted turns. In our own install we've watched a parent's abort kill its running
  children (openclaw-gmuk).
- Kiro Crew keeps a durable queue and holds updates until turns go idle.

None documents a crash simulator, or a running shell surviving an in-place binary swap.

**What Theseus does:** the WAL store is verified by a crash simulator, and a job's shell or PTY survives both a
restart and an exec-swap.

### 3.6 (Planned) An agent that owns its cloud account (hypothesis: unique)

**What the field does:**
- Cloud execution is common: Codex Cloud, Cursor's cloud agents, Devin Cloud, the dots' own computers, Managed
  Agents.
- Kiro Crew runs remote crews on Fargate: 6-hour tasks, at most 10.
- All of these are the *vendor's* cloud, or a sandbox you lend them.

**What Theseus plans:** to own an AWS account outright, with IaC by construction, and guardrails enforced twice.
The gate checks first. Then AWS's own guard policies and SCPs refuse the calls the gate should have refused, even
if the gate fails.

**What exists already:** the guard lane produced the guard policies and SCPs this week, all valid in Access
Analyzer, with 125 simulated actions and 0 mismatches.

---

## 4. Where Theseus is level

These are table stakes now. Theseus has them, or has them built and wiring in:
- **A long-lived service, reached by chat, with approval cards in the chat.** Since this morning's fix, a card
  @-mentions exactly the people who can answer it.
- **A posture gate with per-tool overrides and a floor.** This compares with OpenClaw's deny-wins tool policy,
  Claude Code's deny and ask rules, and Factory's autonomy levels.
- **A native unprivileged sandbox with an egress allowlist.** Theseus's public-only resolver is the same idea as
  Codex's block on private destinations with its rebinding check. Theseus's 3.7 ms start is in line with
  Antigravity's "no startup delay."
- **MCP client and server.** Claude Code, OpenClaw and Devin also serve MCP. Codex *removed* its MCP server.
- **OpenTelemetry**: Claude Code, Codex, OpenClaw, OpenHands.
- **A judgment model.**
  - OpenClaw 2026.9.6 ships TypeSafe Jev as an optional decision model, the same vendor Theseus's client speaks to.
  - Kiro Crew 0.7.0 has a "Decisions (Jev)" preview. Whether that's TypeSafe's Jev is unconfirmed.
  - Theseus isn't alone here, and that's validation of the choice.
- **Self-building.**
  - Others do it at far larger scale: Claude Tag writes 65% of Anthropic's product team's code, OpenAI's
    Androidclaw "finds the PR… and merge[s] it", and Warp's Oz agents implement community issues in the open.
  - Theseus's builder fixed a bug end to end in about 14 minutes for $3.22. That's real, but small.
  - What's distinctive is the governance around it: the spec, a gate of 941 tests (since this morning's merges),
    and the crash simulator.

---

## 5. Where Theseus trails, and whether it matters

### 5.1 The second-model approver (matters, for friction)

**The field's choice.** The big shift of 2026 is that a model, not the user, approves most actions:
- Claude Code's auto mode is the default for Pro, Max and Team, and the built-in starting mode from v2.1.283.
- Codex and the dots have auto-review.
- Cursor's Auto-review is its default run mode.

**The cost of not having one.** The pilot showed it: the builder lost an hour to two approval cards.

**Theseus's answer is better founded, and not live.** Jev makes calibrated, typed micro-decisions on a
shadow-to-canary-to-live ladder. The classifiers are opaque, and their makers say they're no boundary. Cursor's
docs say so in those words. Claude Code's falls back to prompting after 3 blocks in a row, or 20 in total.

**Recommendation:**
- Make "does this act need a human?" Jev's first live decision in M5, under the posture and above the floor.
- Say plainly, as Cursor does, that the judge is not the boundary. The floor, the hold and the sandbox stay hard
  underneath it.
- The friction batch (theseus-2tw and its siblings) attacks the same hour from the other side.

### 5.2 Credentials that never enter the job (matters, before AWS)

**What the field does:**
- **Claude Code:** sandboxed commands see a per-session stand-in value. The proxy swaps in the real secret only for
  the hosts allowed, including re-signing AWS SigV4 requests.
- **OpenClaw:** a secret egress proxy with "bypass-surviving sentinels."
- **IronClaw:** injects secrets at the host boundary, with leak detection.

**What Theseus does now.** M4's design for L1 hands the value into the job: as its environment for a spawn grant,
or through `theseus-cred get` at run time. L1's pid namespace makes that grant private to the job, which closes
the old gaps. But a job that can read a key can send it anywhere its egress allows.

**Recommendation:** before the AWS hands make their first write, add stand-in values with a swap at L1's proxy.
- This needs TLS termination for the listed hosts, which M4 filed for a different reason (recognizing request
  shapes).
- For AWS, the plan's short-lived role sessions limit the damage meanwhile.
- Filed as theseus-gh7 (P2, under M4). Making the AWS step (14b) wait on it is your call.

### 5.3 Reach: surfaces and providers (mostly by design)

**The gap:**
- **Surfaces:** Theseus has Discord, web and CLI. OpenClaw reaches about twenty chat channels plus mobile apps,
  Claude Code has phones and IDEs, and dots take voice calls.
- **Providers:** Theseus has Anthropic and Z.ai; OpenClaw has about 70 plus local models.

**Does it matter?** For one operator, three surfaces is enough, and Discord voice is already planned.

**The provider risk is real.** OpenAI is pulling its models from Cursor because Cursor changed owners. An OpenAI
profile (GPT-6 Astra or Sol) would be cheap insurance and a third opinion. The catalog and profile machinery
already exist; the work is the provider's wire format. It's a post-v1 candidate, and the owner's call.

### 5.4 ACP (post-v1)

**What it is.** The Agent Client Protocol is how hosts embed agents. Zed, Devin Desktop, Kiro Crew, OpenClaw,
Copilot CLI, Grok Build and Mistral Vibe speak it.

**What it would give Theseus:**
- As an **ACP server**, Theseus would run inside Zed or Devin Desktop, with its gate and budget intact.
- As an **ACP client**, Theseus could hand a coding job to Claude Code or Codex as a governed worker. That is
  Kiro Crew's whole model, with Theseus's guarantees around it.

It doesn't block v1. It's the cheapest way to multiply what Theseus can reach.

### 5.5 Scale of delegation (doesn't matter yet)

Claude Code's dynamic workflows run "dozens to hundreds of subagents"; Cursor's Projects claims "thousands."
Theseus has tasks, and task graphs are planned. A personal daemon doesn't need a swarm for v1. When it wants one,
the ledger and the per-session budgets are exactly what a swarm lacks.

---

## 6. Ideas worth borrowing (small, concrete)

1. **Hash-chain the ledger.** ZeroClaw puts "cryptographic receipts on every tool call," and Kiro Crew's log is
   append-only. Theseus's ledger rows are already in a WAL. Chaining each row's digest into the next makes the
   ledger tamper-evident for almost nothing.
2. **Strip hidden text from what the web returns.** Copilot strips hidden characters (HTML comments, zero-width
   text) "to mitigate prompt injection." It's cheap defence in depth under the hold.
3. **Keep the operator's stated limits out of compaction's reach.** Claude Code warns that limits the user states
   in conversation are re-read from the transcript, "so compaction can drop them." Theseus's context compiler
   should pin such limits. That's a design check, not a known bug.
4. **Updates wait for idle.** Kiro Crew's auto-updates wait for active turns. Theseus's swap already keeps jobs
   alive. Deferring a swap until no turn is mid-call would remove the last mid-turn edge.
5. **Cached search as a lower-exposure default.** Codex serves web search from a cached index by default, "to
   reduce exposure to prompt injection from arbitrary live content." Theseus could offer the same mode for plain
   lookups. It should still latch the hold, though: cached text is still someone else's text.
6. **Preserved thinking is real.** Anthropic confirms the safeguard: it "stops API users from editing Claude's prior
   context" for Fable 5.1 and Opus 5.5, on API accounts created on or after 2026-08-31. Theseus has never run the
   three-step check on how it renders requests (theseus-3za, P2). If the account behind Theseus's Anthropic key is
   that new, 3za becomes urgent. Worth checking which account it is.

---

## 7. What this means for v1

The landscape doesn't change v1's direction. It sharpens the order:

1. **Finish the guarantees.** These are the moat. They are also the parts the field says it can't promise.
2. **Put Jev to work on approvals**, in shadow now and live in M5. That turns the pilot's lost hour into
   seconds, without giving up a hard boundary.
3. **Move credentials behind the proxy before the AWS hands write** (theseus-gh7, a new P2 under M4).
4. **Publish the numbers.** The startup budget, the crash simulator and the memory exam are rigor the field doesn't
   show. If Theseus ever goes public, they're its argument.
5. **After v1:** ACP both ways, and an OpenAI profile.

The field is building agents that can do more. Theseus is building one you can **trust to do less than it could**:
it stops at the limits it's given, every time, and can show its work. That's the rarer thing, and for an agent
that lives on the owner's machine with keys to an AWS account, it's the one that matters.

*— written by Tabitha/Claude*
