# Theseus documentation

## Where to start

1. **[The README](../README.md)**: what Theseus is, why it exists, and how to set it up.
2. **[Status and roadmap](status.md)**: what works today, what is being wired in, what comes next, and when.
   It is updated with every step that lands.
3. **[Technical overview](technical-overview.md)**: the core in depth. It covers the protocol, the store (a
   write-ahead log with a rebuildable index), the kernel (executions, actions, budgets, cancellation), and the tool
   loop, with the commands to see each one work.
4. **[The Ship of Theseus](spec/README.md)**: the design document, and the source of truth. Since v0.80 it is in
   chapters under [spec/](spec/), each under about 150 KB, and its [index](spec/README.md) says what each holds
   ([the-ship-of-theseus.md](the-ship-of-theseus.md), the one file until v0.79, points there). It is both the
   specification and the record:
   - Part I, the specification: what Theseus is meant to be, and why;
   - Part II, the build plan: the order of the steps, and the test that gates each one;
   - Part III, as built: what each step actually built, how it was proven, where it diverged from the plan, and
     what it left open.

   When the code and Part I disagree, Part III says so, and one of them gets fixed. The PDF is a rendering of the
   whole document, the chapters concatenated in the index's order, sent to the operator with each version and not
   committed.

## The rest

- **[design/](design/)**: the designs, as written before they were built. Its [README](design/README.md)
  indexes them:
  - the v1 roadmap, re-cut into one spine of steps on `main` and parallel lanes;
  - the phase designs: Stage 2 (the operator's surfaces), the AWS toolset, M4 boundaries, M5 judgment, M6 memory,
    and M7 surface;
  - Review 2, a read-only review of complexity, speed, and hardening, whose findings became fix batches.
- **[setup.md](setup.md)**: one command from a checkout to a running daemon (`scripts/setup.sh`): what it needs,
  what each step does, where the config lives and the order theseusd looks for it, a second daemon beside yours,
  and the same steps by hand.
- **[user-service.md](user-service.md)**: running the daemon as a systemd user service. It covers why, the one
  command (`scripts/user-service.sh install`), what each step does, daily use, undoing it, and the WSL notes.
- **[benchmarks.md](benchmarks.md)**: Theseus's results on public benchmarks, published here first, and how each is
  reported. How to run them: [`bench/README.md`](../bench/README.md).
- **[research/](research/)**: what Theseus was measured against. Its [README](research/README.md) indexes it:
  - "Theseus among the harnesses": how Theseus compares with the provider-made and independent agent harnesses,
    as of October 2026;
  - the two research reports behind it, with every source.
- **[notes/](notes/)**: research and design notes the spec drew on: the hooks investigation and comparison, the
  hooks design, the event-driven execution review, and the tool surface review.

The markdown is the source of truth. After editing the spec, regenerate its PDF from the chapters' concatenation
(marked, then headless Chrome; the index gives the one-line concatenation) and send it. Git ignores it
(`docs/*.pdf`), so a new version no longer adds megabytes to every clone.
