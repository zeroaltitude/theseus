# Tool surface review: what Claude Code, Codex, and OpenClaw expose, and what Theseus keeps

_Tabitha, 2026-09-26. Input to spec §3.24. Evidence: my own tool list as Claude Code 2.1.280; the Codex tool names referenced in OpenClaw's codex plugin source; OpenClaw's `docs/tools` (74 pages) and the tool names registered across `src/agents/tools` and `extensions/*` in the integration checkout._

## 1. What exists today

### Claude Code (2.1.280), core tools the model sees

| Tool | What it is | Verdict for Theseus |
|---|---|---|
| Bash | free-form shell string, background option, timeout | **replace** with `proc.run` (typed argv, no shell parsing) |
| Read | file read with offset/limit; images, PDFs, notebooks | **keep** as `fs.read`; media becomes node types |
| Write | whole-file write | **keep** as `fs.write` |
| Edit | exact-string replace, replace_all | **keep** as `fs.edit` (the best-designed tool in the set) |
| Glob, Grep | file discovery and content search | **keep** as `fs.glob`, `fs.grep` |
| WebFetch, WebSearch | fetch-and-extract, search | **keep** as `http.fetch`, `web.search` |
| Agent / Task | spawn a subagent | **drop**: Theseus has task sessions, not multi-agent (§3.2a) |
| TodoWrite | model-maintained plan list | **drop**: the task graph is kernel state with structured actions (§3.5) |
| NotebookEdit | Jupyter cell edits | **drop**: `fs.edit` plus a notebook-aware plank if ever needed |
| AskUserQuestion | multiple-choice prompt to the human | **keep the idea** as `channel.ask` (confirm, choose, free text) |
| EnterPlanMode / ExitPlanMode | mode switch | **drop**: roles and the task graph carry this |
| Skill | invoke a packaged instruction set | **drop as a tool**: MCP prompts and roles cover it |
| ToolSearch | fetch deferred tool schemas | **keep the idea** inside the toolchain manager, not as a model tool |
| Workflow | scripted multi-agent orchestration | **drop**: no multi-agent |
| SendMessage, ListAgents, TaskStop, KillShell, Monitor | agent/process plumbing | **drop**: kernel controls (`/cancel`, executions) |
| CronCreate/List/Delete | scheduling | **replace** with `wake.at` plus tasks |
| EnterWorktree / ExitWorktree | git worktree isolation | **replace** with `git.*` and workspace snapshots (§7) |
| ReportFindings, DesignSync, PushNotification, RemoteTrigger, ShareOnboardingGuide | product-specific | **drop** |

About twenty tools, of which six (Read, Write, Edit, Glob, Grep, Bash) do almost all the work. That ratio is the lesson.

### Codex CLI (as referenced by OpenClaw's codex plugin)

`shell_command` / `exec_command` (+ `write_stdin` for interactive sessions), `apply_patch` (a unified-diff-like patch format instead of an edit tool), `update_plan`, `view_image`, `web_search`, `request_user_input`, `spawn_agent`, plus MCP tools. Seven native tools. Two ideas worth taking: **apply_patch** shows that a patch format is a good second edit primitive when several hunks change at once, and **write_stdin** shows that a persistent interactive process is a real need (a REPL, a debugger) that a one-shot `proc.run` does not cover.

### OpenClaw (defaults plus common plugins)

74 documented tool pages; over a hundred distinct documented tool names; several hundred registered tool name strings across the extensions. The session I am running in exposes roughly seventy top-level tools plus fifteen deferred ones: `message` alone has 30+ actions; `browser` 25; `nodes` 20; `automations` 10; fourteen memory tools across two systems (`memory_*`, `vestige_*`); five `sessions_*`; `subagents`; image, music, video, TTS generation; `pdf`, `view_image`; `computer`, `mobile_ui`, `canvas`, `dashboard`, `portal`; `secrets`; `gateway`, `openclaw`, `plugins`; `skill_workshop`; wiki tools; goals; intents; transcripts; Google Meet.

What that surface teaches, from living inside it:

- **Breadth is paid for on every turn.** Schemas for seventy tools enter every prompt (or get deferred and then fetched). The model routinely chooses a weak tool because a better one was not in view.
- **Multi-action tools hide intent.** `message` with an `action` enum of thirty values is one tool to the schema and thirty to the policy. The gate cannot cheaply tell `send` from `channel-delete`. The same holds for `browser` and `nodes`.
- **Overlapping tools breed inconsistency.** Two memory systems with seven tools each; three ways to reach another session; `exec` and `gateway_exec` and `computer`. The model picks one arbitrarily and the ledger tells two stories.
- **Operator surface leaks into the agent surface.** `gateway`, `openclaw`, `plugins`, `secrets`, `sessions` patching: these are things the operator does, exposed as tools the model can call. Every one is a gate that has to say no most of the time.
- **The good parts are the small typed ones.** `web_fetch`, `session_status`, `ask_user`: one thing, typed, composable.

## 2. Principles for the trim

1. **One tool, one verb.** No `action` enums. If it needs an enum of verbs it is several tools.
2. **Typed in, node out.** Every argument has a schema the gate can read; every result is a node with an id and provenance (§3.16). Composition happens by passing node references, not by parsing text.
3. **The kernel is not a tool.** Sessions, executions, cancellation, budgets, policy, config, secrets, and the operator's controls are protocol requests or deterministic commands, never model-callable tools.
4. **Memory is compiled, not called.** Recall happens in the compiler every turn (§5). One explicit lookup tool exists for the rare deliberate search; one explicit note tool exists for "remember this."
5. **The shell is reachable, typed, and counted.** `proc.run` with an argv is the only way to a shell; every use is a data point (§3.23).
6. **Everything else is a plank.** Browser automation, image generation, PDFs, voice, calendars, notebooks: MCP servers under the tool contract (§3.12), loaded when a deployment wants them, never in the core set.

## 3. The selected set

Thirty tools in ten families. Each is one verb.

| Family | Tools | Notes |
|---|---|---|
| `fs` | `read`, `write`, `edit`, `patch`, `glob`, `grep`, `list` | `edit` is exact-string replace with occurrence control; `patch` applies a unified diff (Codex's lesson); `list` is stat + tree; `read` returns typed nodes for text, image, PDF |
| `proc` | `run`, `session.open`, `session.send`, `session.close` | `run` is one-shot typed argv with cwd, env allowlist, timeout, shell class; `session.*` is the persistent interactive process (REPL, debugger; Codex's `write_stdin` lesson). No `bash` tool. |
| `text` | `diff`, `query` | `diff` between two nodes or files; `query` is a jq-subset over JSON/YAML/TOML nodes |
| `http` | `fetch` | GET/POST with extraction; returns a node; allowlist by policy |
| `web` | `search` | provider behind config |
| `git` | `diff`, `log`, `commit` | read side native first; everything else is `proc.run git …` until the fallback ratio says otherwise |
| `task` | `create`, `update`, `move`, `split`, `merge`, `close`, `claim`, `handoff` | the structured actions already specified in §3.5 |
| `memory` | `recall`, `note` | the only two; recall is otherwise compiled |
| `channel` | `post`, `react`, `ask` | `ask` covers confirm, choose, free text, to the requesting human or the owner |
| `node` | `read` | read any graph node by id, with a byte or line range; the reference-passing primitive |
| `wake` | `at` | schedule a due-time wake for this execution or task |
| `extend` | `propose`, `promote` | §3.21 |

Count: 7 + 4 + 2 + 1 + 1 + 3 + 8 + 2 + 3 + 1 + 1 + 2 = **35**, of which the eight task actions are one structured surface and the four `proc.session` calls are one capability. Offered to the model by family per turn (§3.23), so a coding turn sees perhaps fifteen schemas, not seventy.

## 4. Explicit rejections, so they are not relitigated

| Rejected | Why | Where the need goes |
|---|---|---|
| free-form `bash` | opaque to policy, quoting tax, unmeasurable | `proc.run` argv |
| subagents / spawn / workflows | no multi-agent (§1) | task sessions (§3.2a) |
| todo / plan-mode tools | the task graph is kernel state | `task.*` |
| multi-action `message`, `browser`, `nodes` | one tool must be one verb | `channel.post/react/ask`; browser and nodes are planks |
| fourteen memory tools | recall is compiled every turn | `memory.recall`, `memory.note` |
| `sessions_*`, `subagents`, `automations` | kernel controls, not tools | protocol requests, `wake.at`, `/cancel` |
| `secrets`, `gateway`, `openclaw`, `plugins`, config patching | operator surface | the operator's CLI and web UI |
| image / music / video / TTS / PDF / view_image | media as tools | node types for input; generation is a plank |
| `skill` | instructions are not tools | roles, MCP prompts |
| notebook editing | niche | `fs.edit`, or a plank |
| worktree tools | shell-shaped git plumbing | `git.*`, workspace snapshots |

## 5. What this buys

A model that sees fifteen typed tools and never a shell string; a gate that reads intent from arguments; a ledger where every call names a verb; a fallback ratio that tells us which toollet to build next; and a surface small enough that the model actually knows it. The distinguishing claim is not that Theseus has fewer tools, it is that each tool is the whole of one idea and composes with the others through the graph.
