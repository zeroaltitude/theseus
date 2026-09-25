# Review: replacing the harness loop with an all-webhook event design

_Tabitha, 2026-09-24. Eddie's proposal: eliminate the harness loop outside the tool loop. The harness always goes quiet. Every tool, call, process, scheduled job, and network wait is wrapped so that its completion arrives as a webhook to a minimal REST receiver owned by the gateway, which correlates it to a channel and acts. "We don't call Bash, we call ReturnWebHookResponse(Bash)."_

## 1. Verdict up front

The **principle** is right and I recommend adopting it as the spec's execution model: **nothing in flight is held in harness memory; every in-flight thing is a durable record whose completion arrives as an event from outside the harness's own stack.** That is what makes the harness restartable mid-work, what lets a tool outlive the harness, and what kills the class of "dropped response to ongoing work" bugs we live with today.

Three parts of the **literal** proposal I would change, each on evidence:

1. **"Webhook" is the wrong transport for most completions, but the right envelope.** HTTP to a loopback REST server for in-process or on-node work adds a port, auth, serialization, and an inbound surface for zero benefit over an in-process completion bus carrying the identical event. Keep one envelope; pick the transport per source: in-process channel for native tools, Unix socket for on-node jobs, **SQS long-poll** for AWS completions (which also preserves "no inbound"), loopback HTTP only for things that can do nothing but POST.
2. **"Unkillable" is a liability, "durable and detached" is the goal.** A wrapper that cannot be killed is a runaway build you cannot stop. What you want is a wrapper that is detached from the harness's lifetime, records its own completion durably even when the harness is down, and is still cancellable by the harness through the execution's cancel path.
3. **Pure edge-triggered systems lose events.** Every production system built this way grows a reconciler. We already decided on the one-minute heartbeat; in this design it stops being a nicety and becomes the level-triggered half of the model, scanning the durable record of in-flight things for completions whose event never arrived.

With those three changes the design collapses into the spec's two-loop model with the "tending" state made explicit and durable, plus a stronger statement than the spec currently makes: **the harness holds no in-flight state that a restart would lose.**

## 2. Evidence that the principle is right

**Our own logs, last three days (gateway journal on this node):**
- 24 occurrences of `Active requester session could not be woken for subagent completion; falling back to requester-agent handoff`. A completion arrived and the thing that was supposed to receive it was not there. That is exactly the failure the proposal eliminates: the completion had no durable home independent of the waiting session.
- Hundreds of `stalled session … reason=blocked_tool_call` diagnostics across five Tank subagent sessions, with `activeToolAge` reaching 599 s (the diagnostic's ceiling). A session sat inside a blocking tool call, unresponsive to humans, because the tool's lifetime and the session's lifetime were the same thing. Tank's two-hour wedge on 2026-09-22 was the same shape.
- The file-courier pattern we adopted in AGENTS.md ("the strong model writes a frozen artifact; the heartbeat is a courier") exists because in-memory subagent handles do not survive session rotation. It is a hand-built version of "completion as a durable record delivered by an independent path."
- The codex process storm (fifteen to twenty `codex.js app-server` processes with no codex agent running) was orphaned child work with no completion contract at all.

Every one of these is a symptom of holding in-flight state in the waiting process. The proposal's core move, making completion an externally delivered event against a durable record, addresses all four.

**Prior art that converged on the same shape:**
- **AWS Step Functions `.waitForTaskToken`**: the workflow emits a token, goes idle at zero cost, and the activity calls `SendTaskSuccess`/`SendTaskFailure` with the token. Heartbeat timeouts detect lost activities. This is the proposal, productized, and it pairs the callback with a **timeout-driven reconciler** because callbacks get lost.
- **Temporal / durable execution**: activities run out of process and report completion; the workflow is replayable from its event history after a crash; the SDK adds activity heartbeats and retries because completion delivery is at-least-once and sometimes never. Theseus's WAL plus external-action lifecycle (§3.16) is the same idea.
- **Kubernetes controllers**: the watch stream is edge-triggered; every controller nonetheless resyncs on a timer (level-triggered), because watches drop events on reconnect. The industry lesson is "edge for latency, level for correctness."
- **Lambda asynchronous invocation with destinations, EventBridge, SQS**: AWS's own answer to "how does a job report back" is a queue or an event bus you pull from, not an HTTP callback into your process. Pull preserves the no-inbound posture; push requires an exposed endpoint.
- **Erlang/OTP**: a process sits in `receive` at zero cost; monitors deliver `DOWN` messages when linked work dies; supervisors restart. The "always goes quiet" instinct is the actor model's default, and it works because the mailbox is durable relative to the receiver's activity, not relative to the node.

## 3. Evidence against the literal form

**HTTP for local completions.** Loopback HTTP is fast (a round trip on this box is well under a millisecond), so latency is not the objection. The objections are structural: a listening port is an inbound surface, even on loopback, which the spec forbids by default; every completion needs a bearer or HMAC and replay protection; every payload needs size limits and untrusted-data handling; the receiver becomes a process that must be up for anything to complete, which recreates the coupling the design was trying to remove. None of that buys anything for a tool running in the same process or on the same node. An in-process `mpsc` channel or a Unix domain socket carries the identical `Completion` envelope with none of it. Reserve HTTP for the rare source that can only POST, and then bind it to loopback and put it behind the same per-job HMAC.

**"Unkillable" wrappers.** The failure mode of an unkillable job is precisely the runaway that `/cancel` exists for: a build that will not finish, an SSM command on the wrong fleet, a test suite that forks bombs. The property we want is different: the wrapper's lifetime is **independent of the harness**, the wrapper **spools its completion to disk if the receiver is absent**, and the harness can still **terminate it** by id through the execution's cancel path (kill the systemd scope, stop the ECS task, cancel the SSM command). "Detached, durable, cancellable" rather than "unkillable."

**Edge-only delivery.** Concrete lost-event cases in our environment: the receiver restarts between dispatch and completion; the wrapper's POST fails and the wrapper exits; an AWS callback is delivered while DNS or the network is flapping; a cron fires while the node is down. Step Functions handles these with heartbeat timeouts and the task-token store; Kubernetes with resync; Temporal with activity heartbeats. Theseus already has the record (the external-action lifecycle) and the timer (the heartbeat); the design just needs the heartbeat to **reconcile** the record against reality, not merely check the wake queue.

**Correlation "strong connection to the relevant channel."** A completion must not be correlated by inference. The correlation id is minted by the harness at `ActionPlanned`, written to the WAL before dispatch, carried by the wrapper, and returned in the completion. The receiver looks up the record; if it is missing, the completion is quarantined and surfaced, never guessed into a channel. This is the transactional-outbox discipline from §3.16 and it is the whole answer to "how do we know which channel."

**Fast tools.** A `Read`, a `Glob`, a `task.create`, a memory lookup complete in microseconds to milliseconds. Forcing them through a completion event is pointless ceremony and, for the tool loop, adds a scheduling hop per call. The tool loop stays synchronous for fast in-process tools; the completion-event path is for anything that leaves the process or takes longer than a bound (say 250 ms). The proposal already carved out "outside the tool loop," and this is where that boundary goes.

## 4. What I propose instead: the same idea, stated as invariants

1. **No in-flight state lives only in harness memory.** Every dispatched thing has a WAL record before it starts (`ActionPlanned → Dispatched`), with a harness-minted correlation id.
2. **Completion is an event, delivered on the cheapest transport that preserves durability:** in-process channel for native tools; Unix socket from the on-node job wrapper; SQS long-poll for Lambda, ECS, SSM, and scheduled completions in AWS; loopback HTTP with per-job HMAC only for sources that can do nothing else. One `Completion` envelope across all four.
3. **The on-node wrapper is detached, durable, and cancellable:** launched as a systemd scope (or the L1 sandbox's process tree), it writes its own result to a spool directory before attempting delivery, so a harness restart loses nothing; the harness drains the spool on startup; `/cancel` kills the scope by id.
4. **The harness is quiescent between events.** Parked on `select()` over the completion sources, Discord, and the heartbeat timer. "Tending" is not a busy loop; it is the set of open records the heartbeat reconciles.
5. **The heartbeat is the level-triggered reconciler.** Every minute: for each open record, is there a spool entry, a queue message, a finished scope, or an external status that says it completed without us hearing? Is any record past its deadline? Is any wake due? Missed events become synthesized completions with `outcome_unknown` where the truth cannot be established.
6. **At-least-once delivery, idempotent handling.** Completions are deduplicated by correlation id; a second delivery is a no-op that is logged.
7. **The tool loop stays synchronous for fast in-process tools;** anything crossing the process boundary or exceeding a latency bound becomes an asynchronous completion under 1 to 6.
8. **The model loop is never woken by a busy poll.** Only a completion, a human message, a due wake, a confirmation answer, or a reconciler finding wakes it.

This is Step Functions' task-token model with a local spool and a pull-based AWS path, sitting inside the two-loop design the spec already has.

## 5. What changes in the spec if we adopt it

- §3.3: "tending" is redefined from "the loop keeps running" to "the set of open completion records the heartbeat reconciles"; the harness is quiescent by construction between events. The heartbeat text gains the reconciliation duty.
- §3.16: gains the `Completion` envelope, the four transports, the spool, dedupe by correlation id, and the deadline rule.
- §7 shells: the job wrapper becomes part of the shell contract for every class: detached scope, spooled result, cancellable by id.
- §6 storage: startup adds "drain spool, reconcile open records" before accepting events.
- §8 simulator: standing scenarios for lost completion, duplicate completion, completion arriving during restart, and cancel of a detached job.
- Settled decisions: "Reachability: localhost only, outbound OK, no inbound" is preserved by preferring SQS pull for AWS completions; the loopback HTTP receiver is documented as the exception, off by default.

## 6. Things I could not verify and would want to measure

- Whether SQS long-poll latency (typically tens of milliseconds to a second) is acceptable for the interactive shell path, or whether on-node jobs must always use the Unix-socket path even when they were launched from AWS classes. My expectation: on-node jobs use the socket, AWS classes use SQS, and a human never notices the difference because AWS-class jobs are already seconds long.
- Spool durability on the desktop deployment: the spool directory must be on the same persistent SSD as the WAL so a reboot loses neither.
- The cost of the reconciler at scale: scanning ten thousand open records per minute is trivial in memory; scanning ten thousand ECS task states through the AWS API is not, so reconciliation of AWS-class jobs should be event-first (EventBridge task state changes into the SQS queue) with API polling only for records past their deadline.
