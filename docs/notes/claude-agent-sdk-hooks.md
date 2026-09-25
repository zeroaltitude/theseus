# Claude Agent SDK / Claude Code hooks — investigation for Theseus

_Tabitha, 2026-09-24. Sources: code.claude.com/docs/en/agent-sdk/hooks, code.claude.com/docs/en/hooks (reference), and the event-name strings embedded in the installed Claude Code 2.1.280 binary (27 events confirmed present)._

## 1. What it is

Hooks are callbacks that run at named points in the agent lifecycle. Two delivery forms share one contract:

- **Settings hooks** (Claude Code): `type: command | http | mcp_tool | prompt | agent`. Input arrives as JSON on stdin (command) or as a POST body (http); output is JSON on stdout / the response body. `prompt` and `agent` hooks ask a model to make the decision (`$ARGUMENTS` carries the input JSON; a fast model by default; the model must answer `{"ok": bool, "reason"}`).
- **SDK callback hooks** (Agent SDK, TS and Python): in-process functions registered in `options.hooks`, keyed by event, with the same JSON output shape. Settings hooks still load alongside them when `settingSources` allows.

Three cadences: per session (`SessionStart`, `SessionEnd`), per turn (`UserPromptSubmit`, `Stop`, `StopFailure`), per tool call (`PreToolUse`, `PostToolUse`, and friends). Plus standalone async observability events (`ConfigChange`, `InstructionsLoaded`, `CwdChanged`, `FileChanged`, `DirectoryAdded`, `Notification`) and model/compaction/worktree/MCP-elicitation events.

## 2. Event inventory (TypeScript SDK is the superset; Python lacks the session, task, elicitation, model-switch, and display events)

| Group | Events | Can block? |
|---|---|---|
| Session | `Setup`, `SessionStart`, `SessionEnd` | no (context only on start) |
| Turn | `UserPromptSubmit`, `UserPromptExpansion`, `Stop`, `StopFailure`, `MessageDisplay` | prompt yes, expansion yes, Stop yes (keeps going), failure no, display transforms only |
| Tool | `PreToolUse`, `PermissionRequest`, `PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch` | Pre yes (allow/deny/ask/defer, updatedInput); PermissionRequest via `decision.behavior`; Post can annotate/replace output, not undo |
| Subagents / teams | `SubagentStart`, `SubagentStop`, `TeammateIdle` | Stop and Idle yes |
| Tasks | `TaskCreated`, `TaskCompleted` | yes (roll back creation; refuse completion) |
| Context | `PreCompact`, `PostCompact`, `PreModelSwitch`, `PostModelSwitch` | PreCompact yes; PreModelSwitch yes |
| MCP | `Elicitation`, `ElicitationResult` | answer or override programmatically |
| Environment | `ConfigChange`, `InstructionsLoaded`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`, `Notification` | mostly observe; worktree hooks replace behaviour |

## 3. The output contract

- **Universal fields:** `continue: false` + `stopReason` stops the agent entirely and outranks every event-specific decision. `systemMessage` shows the user a warning. `suppressOutput` is accepted and ignored. `terminalSequence` for OSC notifications.
- **Top-level `decision: "block"` + `reason`** on UserPromptSubmit, PostToolUse, PostToolUseFailure, PostToolBatch, Stop, SubagentStop, ConfigChange, PreCompact, TaskCreated.
- **`hookSpecificOutput`** (must include `hookEventName`) for richer control: PreToolUse `permissionDecision` allow|deny|ask|defer, `permissionDecisionReason`, `updatedInput` (replaces the whole input), `additionalContext`; PermissionRequest `decision.behavior` allow|deny with `updatedInput`, `updatedPermissions` (rules/mode/dirs with a `destination` of session/local/project/user), `message`, `interrupt`; PostToolUse `updatedToolOutput` (must match the tool's output shape), `additionalContext`, `classifierContext`; Stop `additionalContext` as non-error feedback; SessionStart `additionalContext`, `initialUserMessage`, `watchPaths`, `sessionTitle`, `reloadSkills`; Elicitation `action` accept|decline|cancel + `content`; MessageDisplay `displayContent`.
- **Precedence across multiple hooks:** deny > defer > ask > allow.
- **Context strings capped at 10,000 chars**; overflow is spilled to a file with a 2,000-char preview.
- **Loop protection on Stop:** `stop_hook_active` input flag plus an 8-consecutive-continuation cap.
- **Async:** command hooks with `async: true`, or SDK callbacks returning `{async: true, asyncTimeout}`; side effects only, cannot block or inject.
- **`defer`** (PreToolUse, non-interactive `-p` only, single tool call per turn): ends the query with `stop_reason: "tool_deferred"` so a host process can collect human input and resume with `updatedInput`. This is how a host builds its own approval UI.

## 4. The exit-code contract (command hooks), and why it is a footgun

- Exit 0: success; stdout parsed as JSON if it starts with `{` and ends with `}`, else plain text (added as context only on UserPromptSubmit, UserPromptExpansion, SessionStart, PostModelSwitch).
- **Exit 2: block**, cannot be overridden by JSON; stderr is the reason.
- **Any other non-zero exit (including 1): non-blocking**; the action proceeds with a `hook error` notice. A missing or non-executable script exits 127 and *also proceeds*. The docs warn: a mistyped path leaves a policy gate silently disabled.
- Timeouts: a timed-out command/http/mcp_tool hook renders no decision and **does not block** PreToolUse; a timed-out SDK callback **does** block. Defaults 600 s (30 s on prompt-facing events), 30 s for prompt hooks, 60 s for agent hooks.

## 5. Filtering

- `matcher`: exact string or `|`/`,` list when it contains only `[A-Za-z0-9_\- ,|]`; otherwise an unanchored JS regex. Matched against a per-event field (tool name, agent type, model name, compaction trigger, notification type, MCP server name, ...).
- `if`: one permission-rule expression (`Bash(git *)`, `Edit(*.ts)`) evaluated against tool name + args on tool events; Bash matching checks each subcommand and `$()`; when it cannot tell, it runs the hook anyway. Docs: "the `if` filter is best-effort, use the permission system rather than a hook to enforce a hard allow or deny."

## 6. Assessment for Theseus

**Worth adopting**
1. The **event taxonomy by cadence** (session / turn / tool call / standalone), and the per-event "can block?" table as a normative artifact.
2. One **structured result type** per event with `hookSpecificOutput`-style discriminated payloads, a universal `continue:false` stop, `additionalContext` injection, `updatedInput` / `updatedOutput` transforms, and the **deny > defer > ask > allow** precedence.
3. **`defer`** as the primitive for out-of-band human input: pause the execution at a tool call, collect input elsewhere (Discord component), resume with the answer. This is exactly the Theseus confirm gate and elicitation flow.
4. **Async observer hooks** that cannot block, for logging and telemetry, distinct from gating hooks.
5. **Loop protection** on stop-override hooks (a continuation cap and a "you already overrode once" flag).
6. **Context-size caps** with spill-to-file.
7. `PostToolBatch` (once per batch before the next model call) and `PreCompact`/`PostCompact` as first-class points.
8. `prompt` / `agent` hook kinds: a hook can be a model judgment. In Theseus that is a **Jev question pack as a hook handler**, which is a cleaner version of the same idea.

**Deliberately not adopting**
1. **Exit codes as control flow.** Exit 2 blocks, exit 1 does not, exit 127 (missing script) proceeds. Theseus hooks return typed results only; a handler that fails to run or fails schema validation is a *loud, ledgered* error, and for gating hooks it **fails closed**.
2. **Fail-open timeouts on gates.** Theseus gating hooks fail closed on timeout; observer hooks fail open.
3. **Best-effort `if` matching as a safety mechanism.** Hooks are never the security boundary in Theseus; the policy gate (§3.9) is. Hooks can only *tighten* a decision the gate already permits, never loosen it. This is the reviewer's "Jev chooses within policy, never widens it" rule applied to hooks.
4. **Ad hoc shell command hooks as the default handler kind.** They contradict the static-binary and shell-class model. Theseus handler kinds are: compiled-in plugin, WASM component, HTTP endpoint (loopback only), and Jev pack. A shell handler exists but runs as a normal `shell.run` through the shell classes and policy, and is therefore a tool call, not a privileged side channel.
5. **Settings-file discovery of hooks from the working tree.** Hooks are registered by plugins and by the owner's binding config, never picked up from a repository the agent happens to be working in (that is an injection vector Claude Code accepts because a human is at the keyboard).

**Observed gaps Theseus should fill**
- No hook sees the assembled context or its manifest before the model call (`UserPromptSubmit` sees the prompt only). Theseus exposes `ContextBuilt` with the manifest and lets a hook add or veto sections.
- No hook on memory ingest or recall; Theseus has both.
- No hook on the *judgment* itself; Theseus emits `JudgmentMade` (observe-only, for the ledger and for shadow evaluation) and allows a `PreJudgment` hook to add state, never to alter the answer.
- Hook runs are not first-class records; in Theseus every hook invocation is a ledger row with input hash, output, latency, and handler version.
