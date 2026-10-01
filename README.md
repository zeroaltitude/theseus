# Theseus

**A personal AI agent you can trust with real work, because you can see everything it does.**

Theseus runs AI agents for one person, or for a small team. You talk to it in Discord, in a terminal, or in
your browser. It works for you with real tools: it reads and edits files, runs commands, searches and reads the
web, starts background tasks, and sets itself reminders. It keeps a complete, durable record of everything it
does: what it did, why, what it was shown, what it cost, and who approved it.

It is written in Rust: a daemon that does the work, and a small command-line client. It talks to Anthropic's
Claude models, and to any model served through the same API (GLM from Z.ai is supported today).

> **About the name.** The Ship of Theseus is the old puzzle: if you replace a ship plank by plank, is it still
> the same ship? Theseus is built that way, one small, reviewed, tested step at a time. Its design document is
> also the record of every plank that was replaced.

## Why another agent harness?

There are good agent tools already: Claude Code, Codex, Cursor, OpenClaw (the harness Theseus grew out of), and
more. We use them every day. In 2026 they all converged on the same shape: a long-running local service that you
reach by chat, with the risky work done in a sandbox. Theseus has that shape too.

But running agents for days at a time, on work that matters, kept surfacing the same questions that the other
tools answer softly, or not at all:

- **What exactly did it do, and why?** After an hour of autonomous work, the reasons, the tool calls, the retries,
  and the costs are scattered across logs, when they are kept at all.
- **What happens when something breaks?** A crash, a restart, or a network drop in the middle of a task. Is the
  work lost? Is it done twice?
- **How do I stay in control without babysitting it?** Asking permission for everything trains you to click
  "yes" without reading. Asking for nothing means trusting a model with everything.
- **What did that cost?** Usually you find out from the monthly bill.
- **What was the model actually shown** when it made that choice?

Theseus is our answer. The other tools compete on reach: more chat apps, more models, more agents at once.
Theseus competes on **guarantees**, promises that hold whether or not the model behaves:

- **Money is a gate, not a report.** Every model call reserves its worst-case cost against the session's budget
  before it runs. At the limit, Theseus stops and asks you, and only you can reset it.
- **Speed is a tested contract.** Theseus answers about 30 milliseconds after it starts, and stops cleanly in
  under 100, without losing work in flight. The test suite fails if those numbers slip, so restarting and
  upgrading are routine, never risky.
- **Reading the web stops the hands.** Once a session has read text from the internet, anything it does next
  that acts on the world waits for your approval, until you say you trust it again. A web page can't talk your
  agent into deleting your files.
- **Nothing is ever lost or done twice.** Every action is written to disk before it is attempted, and its result
  is written when it settles, the way a bank treats a payment. Kill Theseus at any moment, and it picks up exactly
  where it was.
- **Memory has to pass an exam before it ships.** Theseus has a memory exam with held-out questions, and recall
  goes live only where it measurably helps.

As far as we could find, no other harness makes the money, speed, web, or memory promises (see [Theseus among the
harnesses](docs/research/harness-landscape.md)). Durability you can test is rarer than it looks, but it isn't
unique.

## What it's like to use

- **You talk to it where you are:** a Discord DM or channel, the `theseus` command in a terminal, or the web.
- **It shows its work.** Discord gets a quiet line per tool call. The terminal streams the reply, and the tool
  calls as they happen. The web cockpit shows everything live:
  - the sessions and what each one is doing;
  - every model call, with its timing, tokens, cache hits, and cost;
  - every tool call's whole life, from the moment it was planned to its result;
  - the budget, the approvals waiting for you, and the complete ledger.
- **It asks before acting, in proportion.** Reading is free. Writing files and running commands either notify you
  or wait for you, as you choose, tool by tool. An approval is bound to the exact command it was asked about.
- **It works in the background.** A long job keeps running after the reply, and its result comes back to the
  conversation, even across a restart. A conversation can start a background task with its own budget, and set
  itself a reminder for later.
- **You can always ask "why?"** Every turn, every call, every approval, and every dollar is in an append-only
  ledger, timed, attributed, and kept.

## Why it might interest you

- **If you run agents for real work**, Theseus is built for you to see, audit, and trust what they do. It speaks
  OpenTelemetry too, so any observability backend can watch it.
- **If you care about reliable software**, Theseus treats an agent's tool calls with the discipline of a
  payments system: write-ahead logging, idempotent completions, verified cancellation, and crash tests that kill
  it at every step.
- **If you're curious how far AI can carefully build software**, Theseus is itself being built by AI agents
  (Claude), under one person's direction, in small steps. Every step has to pass more than a thousand tests, a
  speed bench, a live check against a running copy, and a written review. Every step's record, including where
  it diverged from the plan and why, is in the design document's Part III.
- **If you like small, opinionated tools**, it's a daemon and a command line that speak one protocol
  (JSON-RPC), plus a Discord binding and a web UI. It is not a framework.

## Where it stands (October 2026)

It works today:
- Conversations in Discord (DMs and channels), the terminal, and the browser, with Claude and GLM models.
- Built-in tools: files (read, write, edit, patch, search, list), git (diff and log), commands, web search and
  fetch, background tasks, and reminders.
- Approvals from Discord, the terminal, or the web, including "approve, and trust this session".
- A secret broker that hands a program only the credentials you've granted it.
- Budgets in dollars, the cockpit, crash recovery, restore from a backup copy, and a systemd installer.

Next on [the roadmap](docs/design/roadmap-v2.md):
- sandboxes for code the agent writes;
- a second model (Jev) that judges when work is done and which actions are safe;
- memory and recall, measured by the exam;
- MCP in both directions;
- and an AWS account the agent owns, within hard limits.

Theseus is early (version 0.0.1), runs on Linux, and has one daily user. Expect sharp edges.

## Quick start

You'll need:
- Linux;
- Rust 1.98 or later;
- Node.js 22 or later (to build the web UIs);
- an Anthropic API key, or a key for another provider that speaks the same API;
- [1Password](https://1password.com/) with a service account. Theseus reads every secret from 1Password; the
  service account's token is the only secret it accepts any other way.

```bash
git clone https://github.com/zeroaltitude/theseus && cd theseus

# Build: the two web UIs first, since they're embedded in the binary.
(cd web && npm ci && npm run build)
(cd cockpit && npm ci && npm run build)
cargo build --release
install -m 755 target/release/theseus target/release/theseusd ~/.local/bin/

# Configure: start from the annotated template.
mkdir -p ~/.theseus
theseusd example-config > ~/.theseus/theseus.toml
#   then edit it: point each op:// reference in [secrets] at an item in your own vault.
export THESEUS_CONFIG=~/.theseus/theseus.toml
export OP_SERVICE_ACCOUNT_TOKEN=...      # or keep it in a file: --op-token-file
theseusd check                           # proves every secret resolves, then exits

# Run it, and talk to it.
theseusd &
theseus ask "Hello! What can you do?"
```

Then open the web UI at <http://127.0.0.1:7433/>, or the cockpit at <http://127.0.0.1:7433/cockpit/>.

From there:
- **Run it as a service:** `theseusd install --user` prints the plan, and `--apply` performs it.
- **Connect Discord:** `theseusd example-bindings` prints the bindings file's format.
- **Learn the command line:** `theseus --help`. A good first look at a session is `theseus watch`.

## Documentation

- **[docs/README.md](docs/README.md)**: where to start, and what each document is for.
- **[The Ship of Theseus](docs/the-ship-of-theseus.md)**: the design document. It is both the specification and
  the record of what was built.
- **[Technical overview](docs/technical-overview.md)**: the core in depth (the store, the kernel, the tool loop,
  the protocol, and the command line).
- **[Design documents](docs/design/)**, **[research](docs/research/)**, and **[notes](docs/notes/)**: how each
  part was designed, and what it was measured against.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
