# Theseus hooks — design proposal

_Tabitha, 2026-09-24. Expands §3.17 of the spec into a buildable design. Inputs: the Claude Agent SDK investigation, the comparison with Strands, AgentCore, Bedrock Agents, and OpenClaw, and the spec's own invariants (authority, append-only ledger, two loops, executions)._

## 0. What hooks are for, and what they are not for

Hooks are how plugins and the owner **observe** the harness and **tighten or transform** its behaviour at named points. They are the extension surface for compiled-in and WASM plugins, and the operator's customization surface through binding config.

Hooks are **not** the security boundary. The policy gate (§3.9) decides what an execution may do; a hook can deny, ask, defer, or transform within that permission, and can never grant. Hooks are also not the control path for deterministic stops: `/stop`, `/cancel`, revocation, and budget exhaustion are observable but not vetoable.

Three design goals fall out of the comparison:

1. **Strands' programming model** (one typed event, explicit mutable fields, providers as bundles) because it is the least error-prone for Rust plugin authors.
2. **OpenClaw's operational model** (fail-closed gates named as such, per-kind failure policy, operator-tunable timeouts, written merge rules) because we have paid for the lack of it elsewhere.
3. **Claude SDK's `defer`** as the one primitive that makes human-in-the-loop composable.

## 1. Core types

```rust
/// Every hookable point implements this. The event carries its own
/// mutable "decision" fields; there is no separate result object to
/// mis-assemble. (Strands model.)
pub trait HookEvent: Send + Sync + 'static {
    const NAME: &'static str;            // "PreToolCall"
    const KIND: HookKind;                // Gate | Transform | Claim | Observe
    const CADENCE: Cadence;              // Harness | Execution | Turn | Tool | Action | Task | Memory | Context | Judgment | Mcp | Discord | Config
    /// Field the `matcher` is tested against, if any (tool name, role id, server name, …).
    fn match_key(&self) -> Option<&str>;
    /// Bounded, redaction-safe projection written to the ledger.
    fn ledger_view(&self) -> LedgerEventView;
}

pub enum HookKind {
    /// May return Allow | Deny | Ask | Defer; precedence Deny > Defer > Ask > Allow.
    /// Fails CLOSED on error or timeout. Sequential, priority order; a Deny stops the chain.
    Gate,
    /// May rewrite designated fields (inputs, outputs, context sections). Sequential; each
    /// handler sees the previous handler's result. Fails OPEN per handler (skip it, keep others),
    /// unless the event marks the field `required`, in which case the turn aborts.
    Transform,
    /// May take ownership of the event (e.g. answer an elicitation programmatically).
    /// Sequential; first `Claimed` wins and the chain stops.
    Claim,
    /// Read-only. Runs concurrently; results ignored; may be `async`. Fails OPEN.
    Observe,
}
```

Every event struct is plain data with the fields a handler may set in a `decision` sub-struct. Example:

```rust
pub struct PreToolCall {
    // read-only context
    pub execution: ExecutionRef,          // id, channel, authority (principal, ceilings), role
    pub tool: ToolRef,                    // name, tags, source (aws | mcp:<server> | shell | task | memory)
    pub input: Json,                      // the model's arguments
    pub policy_verdict: PolicyVerdict,    // what the gate ALREADY decided: Allow | Confirm | Deny
    pub turn: TurnRef,                    // turn id, step index, spend so far, projected input tokens
    // decision fields (mutable)
    pub decision: GateDecision,           // default Allow
    pub updated_input: Option<Json>,      // full replacement, re-checked against policy
    pub additional_context: Vec<ContextFragment>,   // capped; spilled to a Resource node if large
}

pub enum GateDecision {
    Allow,
    Deny { reason: String },              // reason goes to the model as the tool error
    Ask  { reason: String },              // upgrades policy Allow → Confirm; cannot downgrade Confirm
    Defer{ token: DeferToken, prompt: HumanPrompt },   // parks the execution; see §5
}
```

The invariant "hooks never widen authority" is enforced in the runner, not left to authors: `Ask` and `Deny` are always honoured; `Allow` on an event whose `policy_verdict` is `Confirm` or `Deny` is a no-op and is logged as `hook_attempted_widening`. `updated_input` is re-evaluated by the policy gate before execution, exactly as the original input was.

## 2. Handlers and providers

```rust
pub trait Handler<E: HookEvent>: Send + Sync {
    fn id(&self) -> HandlerId;            // stable: "<plugin>:<name>@<version>"
    fn priority(&self) -> i32 { 0 }       // higher runs first; reverse for After* events
    fn matcher(&self) -> Option<Matcher>; // exact | list | anchored regex over E::match_key()
    fn predicate(&self) -> Option<Predicate<E>>;  // typed filter over the event payload
    fn timeout(&self) -> Option<Duration>;
    async fn handle(&self, ev: &mut E, cx: &HookCx) -> Result<(), HookError>;
}

/// Strands' HookProvider: a plugin registers a bundle in one place.
pub trait HookProvider {
    fn register(&self, reg: &mut HookRegistry);
}
```

**Handler kinds** (the same `Handler` trait, different backing):

| Kind | Backing | Allowed for | Notes |
|---|---|---|---|
| Native | compiled-in Rust | all kinds | zero cost; the AWS, Discord, Jev, memory plugins register this way |
| WASM | wasmtime component implementing the hook world (WIT) | all kinds | default for third parties; capability-scoped: an event's read-only fields are the only imports |
| HTTP | loopback-only POST with the event's ledger view + decision fields; JSON back | Gate (fail closed), Transform, Observe | never off-host; for local tooling and the web UI |
| Jev pack | a question pack over the event's state; answers mapped to a decision by a declarative table | Gate (as `Ask`/`Deny` only, never `Allow`), Observe | the Theseus version of Claude Code's `prompt`/`agent` hooks; every call is a recorded judgment |
| Shell | `shell.run` through the shell classes and policy; stdout JSON back | Observe, Transform | it is a tool call; it inherits the execution's authority and the L0/L1 mapping; never a Gate |

Registration sources: plugin code (compiled-in or WASM manifest) and the owner's binding config (which may attach HTTP, Jev-pack, and Shell handlers by reference). Nothing in a workspace or repository can register a hook.

## 3. Dispatch

```
emit(ev):
  handlers = registry.for::<E>()            // already sorted by priority; reversed if E is After*
  for h in handlers where h.matches(ev):
    if E::KIND == Observe: spawn(h.handle(ev.snapshot(), cx))   // concurrent, detached, timeout enforced
    else:
      match timeout(h.timeout, h.handle(&mut ev, cx)):
        Ok  => ledger.record(HookRun{ ok })
        Err => ledger.record(HookRun{ err }); apply_failure_policy(E::KIND, h, ev)
      if E::KIND == Gate && ev.decision is Deny|Defer: break    // Deny/Defer stop the chain
      if E::KIND == Claim && ev.claimed: break
  return ev
```

**Merge rules (written down, per kind):**
- Gate: precedence `Deny > Defer > Ask > Allow` across handlers; first `Deny` or `Defer` short-circuits. `additional_context` fragments accumulate.
- Transform: sequential; each handler sees the latest value; the last write to a field wins **and is attributed** (the ledger records which handler produced the final value). Fields marked `intersect` (tool allowlists, context section budgets) intersect rather than overwrite.
- Claim: first claimant wins.
- Observe: no merge.

**Failure policy (per kind, not per event):**

| Kind | On error / timeout / malformed result |
|---|---|
| Gate | **fail closed**: treat as `Deny { reason: "hook <id> failed" }`; ledger `hook_failure`; web UI shows it |
| Transform | skip this handler, keep others; if the event declares a `required` transform (e.g. `MessageSending` for a channel that requires a redaction pass), abort the turn with a message |
| Claim | continue to next handler |
| Observe | log, continue |

Default timeouts: Gate 5 s, Transform 5 s, Claim 5 s, Observe 30 s, Jev-pack handlers 3 s. Owner may override per handler or per event in binding config, capped at 60 s. A timed-out handler is cancelled (Rust futures drop; WASM instances are trapped; shell handlers are killed) so timeouts also bound side effects, which OpenClaw could not do.

**Recursion exclusion:** the runner sets `cx.in_hook = true`; any `emit` from inside a handler is a no-op that records `hook_reentry_suppressed`. `JudgmentMade` and `Heartbeat` are Observe-only by type.

## 4. The ledger record

Every handler invocation appends:

```
HookRun { ts, event: E::NAME, event_id, execution_id?, handler_id, handler_version,
          matched_by: matcher|predicate|none, input_hash, decision_before, decision_after,
          fields_written: [..], latency_ms, outcome: ok|error|timeout|malformed|widening_attempt,
          cost: { jev_calls, tokens }? }
```

This is what makes hooks debuggable in the web UI ("why did this tool call get denied?"), replayable in the simulator, and tunable by the learning loop (a Jev-pack handler's decisions are judgments with outcome labels like any other).

## 5. `defer` and `resume`

`Defer` is the single primitive under three features: the destructive confirm gate, MCP elicitation routed to a human, and any plugin that needs an out-of-band answer.

1. A Gate handler returns `Defer { token, prompt }`. The runner writes `ActionPlanned` (§3.16) with the pending call and the token, transitions the execution to `waiting { on: defer(token) }`, and hands `prompt` to the Discord plugin, which renders a component to the **requesting principal** (or the owner for requester-less work).
2. The execution costs nothing while waiting. The harness loop's heartbeat enforces the prompt's expiry; on expiry the pending call is `failed { reason: "no answer" }` and the execution resumes so the model can react.
3. On answer, `ComponentInteraction` fires; the Discord plugin resolves the token; the runner re-emits `PreToolCall` for the same call with `defer_answer: Some(answer)` set, so the original handler can now return `Allow` (optionally with `updated_input` carrying the answer). Authority and confirmation binding are re-validated at this point (§3.9), so a stale confirm after a role change is refused.
4. Only one deferred call per turn; a batch containing a deferred call has the other calls executed first, then the deferred one alone. This avoids Claude Code's "defer ignored in a batch" limitation while keeping resume simple.

`resume` (from Strands) is not a separate hook feature: an `ExecutionEnded` Observe handler that wants the agent to keep going submits a wake with input through the normal harness-loop API, which creates the next turn under the same execution and authority. Loop protection is the loop judge plus budgets, not a hook-level cap.

## 6. Event catalogue (normative)

Names, kinds, and the decision fields each exposes. `After*` events run handlers in reverse priority.

**Harness loop (all Observe):** `Heartbeat`, `QuiescentEntered`, `WakeFired{source}`, `TenderHealthChanged`, `BudgetThreshold{budget, pct}`.

**Execution:** `ExecutionQueued` (Observe), `ExecutionStarted` (Transform: `additional_context`), `ExecutionWaiting{reason}` (Observe), `ExecutionResumed` (Observe), `ExecutionCancelled{by}` (Observe), `ExecutionEnded{outcome}` (Observe).

**Turn:** `InboundReceived` (Gate: Deny blocks the turn with a reason to the channel; Transform: `additional_context`), `Classified` (Observe: role, topics, people, risk, continuation hint), `RoleSwitching{from,to}` (Gate: Allow/Deny), `ContextBuilt` (Transform over the manifest: add fragments, drop sections by id, adjust section budgets by intersection; Gate: Deny aborts the turn), `ModelResponse` (Observe), `StopJudged{verdict}` (Transform: `override: Some(Continue{reason})`, honoured at most 8 consecutive times per execution, flagged `override_active`), `ContinuationChosen{strategy}` (Observe).

**Tool:** `PreToolCall` (Gate + Transform as in §1), `PostToolCall` (Transform: `updated_output` must match the tool's output schema, `additional_context`), `PostToolCallFailure` (Transform: `additional_context`, `retry: bool` bounded by the execution's retry budget), `PostToolBatch` (Transform: `additional_context`, `end_turn: Option<FinalText>` from Strands).

**External action (all Observe):** `ActionPlanned`, `ActionAuthorized`, `ActionDispatched`, `ActionSettled{outcome}`.

**Task:** `TaskCreating` (Gate), `TaskUpdating` (Gate), `TaskCompleting` (Gate: Deny with reason returns to the model as feedback), `TaskWoke` (Observe).

**Memory:** `PreIngest` (Transform: may lower `kind`/`durability`/`trust`, never raise; may set `drop_to_floor`), `PostRecall` (Transform: may remove candidates, never add), `ConsolidationProposed` (Gate).

**Context:** `PreCompaction` (Gate; Transform: `strategy_hint`), `PostCompaction` (Observe), `PreRedaction` (Observe), `PostRedaction` (Observe).

**Judgment:** `PreJudgment` (Transform: may add named state fields; questions and criteria are read-only), `JudgmentMade` (Observe).

**MCP:** `ElicitationRequested` (Claim: answer programmatically; unclaimed → routed to Discord via `Defer`), `ElicitationAnswered` (Transform: `updated_answer`; Gate: Deny), `SamplingRequested` (Gate), `PromptExpanding` (Gate), `ServerListChanged` (Observe).

**Discord:** `MessageSending` (Transform: `updated_content`; Gate: Deny cancels delivery), `MessageSent` (Observe), `VoiceSpeaking` (Transform/Gate like MessageSending), `ComponentInteraction` (Observe; resolved by the runner for `defer`), `ChannelInvited`/`ChannelLeft` (Observe).

**Config (all Observe):** `BindingChanged`, `PolicyChanged`, `PackPromoted`, `RoleTableChanged`.

## 7. Examples

**A redaction pass every outbound message must pass (owner-required transform):**
```toml
[[bindings.default.hooks]]
event = "MessageSending"
handler = { kind = "native", id = "secret-scrub@1" }
required = true            # failure aborts delivery instead of skipping
```

**Jev as a second opinion on risky shell commands (Ask only, never Allow):**
```toml
[[bindings.ops.hooks]]
event = "PreToolCall"
matcher = "shell.run"
handler = { kind = "jev", pack = "shell-risk.v1", map = { "risk>=0.7" = "Ask", "destructive_intent>=0.8" = "Deny" } }
```

**A WASM plugin that appends conventions once per tool batch (Strands/Claude `PostToolBatch` idea):**
```rust
impl HookProvider for Conventions {
    fn register(&self, reg: &mut HookRegistry) {
        reg.on::<PostToolBatch>(Handler::new("conventions@2", |ev, _cx| {
            if ev.batch.iter().any(|c| c.tool.name.starts_with("git.")) {
                ev.additional_context.push(ContextFragment::text(GIT_CONVENTIONS));
            }
            Ok(())
        }));
    }
}
```

**Programmatic elicitation answer for a known server:**
```rust
reg.on::<ElicitationRequested>(Handler::new("jira-defaults@1", |ev, cx| {
    if ev.server == "jira" && ev.schema.has_only(["project"]) {
        ev.claim(ElicitationAnswer::accept(json!({"project": cx.binding.default_project()})));
    }
    Ok(())
}).with_matcher("jira"));
```

## 8. Testing

Every event has a simulator fixture. Property tests, run on every commit: a Gate handler returning `Allow` never changes a `Confirm` or `Deny` policy verdict; a failed Gate handler always yields `Deny`; `Deny` from any handler wins regardless of priority; a deferred call resumes with the same tool, arguments, and authority or is refused; Observe handlers cannot mutate (enforced by type: they receive a snapshot); no handler can emit an event; every invocation produces exactly one `HookRun`.

## 9. Open questions

1. WASM hook world: expose events as WIT records with only the mutable fields writable, or hand WASM a JSON view and validate the returned patch? The first is safer and faster; the second is far less work for plugin authors. Proposal: WIT records for the top ten events, JSON view for the long tail.
2. Should the owner be able to register **Gate** handlers of kind HTTP at all, given they fail closed? Convenient for local tooling, dangerous if the endpoint is down. Proposal: allowed, with a mandatory health check at registration and a loud web-UI banner while any HTTP gate is failing.
3. Handler ordering across plugins: priority is per handler, but two plugins may both claim priority 100. Proposal: ties broken by plugin load order, load order is explicit in binding config, and the web UI shows the effective chain per event.
