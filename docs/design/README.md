# Design documents

Theseus's phase designs and the re-cut v1 roadmap, checked in on 2026-09-30 from the design lanes. They are the
record of what was designed, before it was built. [The spec, The Ship of Theseus](../the-ship-of-theseus.md),
stays the source of truth, and its Part III records what was built. Each document was scrubbed while the repository
was public (it has been private since 2026-09-30); the line under its title names the kinds of detail replaced.

- [roadmap-v2.md](roadmap-v2.md): the v1 roadmap re-cut into one spine on `main` and parallel lanes, with a timeline estimate and the conflicts found across the six designs. It orders every step below, from Stage 1's remainder (steps 4b to 8) to step 45.
- [stage2-operator-surfaces.md](stage2-operator-surfaces.md): Stage 2, the operator's surfaces: the protocol push, the TUI, herdr, the first edges, telemetry, and caching. Roadmap steps 9 to 13.
- [aws-toolset.md](aws-toolset.md): the AWS toolset for an owner-Theseus: the dynamic client, the guardrails, credentials, stacks, durability, and hands. Roadmap steps 14 to 16 and 40, and AWS credentials in L1.
- [m4-boundaries.md](m4-boundaries.md): M4, boundaries apart from AWS: the L1 sandbox, verified cancellation, egress and credential brokering, labels, the ontology's first slice, and control-plane separation. Roadmap steps 17 to 22.
- [m5-judgment.md](m5-judgment.md): M5, judgment: the Jev client, its packs in shadow, the learning ledger, and the ladder from shadow to live. Roadmap steps 23 to 28.
- [m6-memory.md](m6-memory.md): M6, memory as an experiment: the memory exam, the index tender, recall, and the ablation harness. Roadmap steps 29 to 35.
- [m7-surface.md](m7-surface.md): M7, the surface: the MCP client and server, recurring wakes, bindings, the task graph, the web UI, self-extension, and voice. Roadmap steps 36 to 39 and 41 to 45.

And one review:

- [review-2.md](review-2.md): Review 2 (2026-09-30), a read-only review of the code as built through the Daily
  Driver, in three layers: complexity, speed (the lifecycle budgets and the runtime's threads), and hardening (the
  web UI, the spool, the git tools, the secret broker). Its accepted findings became the roadmap's fix batches and
  lanes. The spec's Part III records each one's fix. Checked in on 2026-10-01.

_Index written by Tabitha/Claude, 2026-09-30; Review 2 added 2026-10-01._
