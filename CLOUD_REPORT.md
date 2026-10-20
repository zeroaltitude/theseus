# CLOUD_REPORT: agent-answers (theseus-5rvss)

Branch `cloud/20261019-agent-answers`, from `main` as cloned (ae7724f). Started 02:25 UTC, finished about 05:45 UTC.
One step (the row). Two commits, unsigned (no key here; the maintainer's merge is signed):

- `7430a7f` core: an answer from an agent's process is labelled as the agent's, and money and AWS are the owner's own
- `d6e6098` docs: cli.md says how an answer's author is read, and that money and AWS wait for the owner

## What I found

- **The brief's premise was stale.** `judge_act` traces nothing today: theseus-zmgb retired the ppid walk (peer.rs's
  module doc says so), and `from_job` is the CLI's own refusal. So there was no walk to extend; I built the one walk
  (`crates/theseus-core/src/peer_proc.rs`) and ran it from the connection, once, when an act first asks.
- **The kernel's own rows cannot carry the agent.** `bind_confirm` writes `by = "operator"` and `authorize` compares it
  with the policy's confirmer, so the action's `confirm.by` must stay as it is. The board's settle (`action.confirmed`'s
  `by`) therefore never said who answered. The core's own `action.confirm_answered` row does (`by` = the answerer's
  label), so the work board now also reads that row, and only for an agent's answer (an owner's reads byte for byte as
  before).
- **`work.answer` validates the option before `judge_act`.** An agent that sends `approve` to a budget question gets
  "`approve` is not an answer to this question" (its answers are `reset, decline`), not the money refusal; both leave the
  question open. Nothing runs either way.
- No "paid" mark exists on any tool apart from the AWS plan's `cost_bearing`, so "any run the policy marks paid" is the
  cost-bearing AWS tools (`aws.cost`, `aws.inventory`, `aws.logs.query`), which `owners_alone` covers by name.

## What I changed (7430a7f; d6e6098 is docs/cli.md)

**Who is behind a CLI answer** (`peer_proc.rs`, `rpc/server.rs`, `approval.rs`, `theseusd/src/main.rs`).
- `Client` carries the peer's pid (`SO_PEERCRED` at accept; `--stdio` gives its parent). `serve_connection` keeps one
  `Link` per connection (label, surface, `Peer`); `Peer::behind()` reads `/proc` once, at the first act that asks, never
  at accept (nothing on the start path or a plain turn).
- One walk of the parent chain: a listed program anywhere on it (the peer itself first; the nearest names it) is
  `Agent(<program>)`; else a peer with no controlling terminal (`tty_nr` 0) is `Agent("no terminal")`; a peer that
  exited before the walk is `Agent("untraced")`; else `Owner`. A loop or a deep chain ends the walk (64).
- `[policy] agent_processes` (config.rs, template, `theseusd example-config`; template tests pass): default `claude`,
  `codex`, `gemini`, `aider`, `opencode`, `openclaw`. Matched by file name, any case, against the command name and the
  first two words of the command line (so `node /usr/bin/claude` matches; note the kernel truncates a command name to 15
  characters, which the command-line words cover).
- `Answerer.agent: Option<String>`; the label is `agent:<name>`, so the ledger's `by` (`action.confirm_answered`,
  `budget.reset`, `approval.refused`), the narrative, the card's settle and the board all say it. No stored field
  changed, so no store-format bump and no `tests_layouts` sample; a row with no `agent:` reads as the owner's.

**Money and AWS** (`approval::owners_alone`, `Core::judge_act`): an agent's answer, approve or decline, to the budget
question (the spend reset) or to any `aws.*` tool but the free reads (`aws.describe`, `whoami`, `s3.list`, `s3.get`,
`logs.tail`, `trail`) is refused as `approval.refused` with "this spends money or changes AWS, so only the owner answers
it: press it in the DM, or answer from your own terminal"; the question stays open. Both answers are refused: "the
owner's call, either answer". `confirm.list` says `may_answer: false` to an agent for those questions, so no surface
draws a button whose answer is refused. The owner's terminal, the web UI and the owner's Discord press are unchanged.

**Words** (protocol, core, discord): `DigestLine.by` and `Notice.by` (optional, absent for the owner and for every old
line); `agent_by`/`agent_of`/`by_words`. The digest says "✓ answered by an agent (claude): aws.stack.apply" (folded
"✓ an agent (claude) answered 3 questions under …"), keeps an agent's answers out of the owner's folds, and counts them
apart in the summary (`1 answered by you, 3 answered by an agent`); the board's notice says "✓ approved by an agent
(claude): …"; the narrative and Discord's settle say "approved by an agent (claude)". TypeScript regenerated
(`cockpit/src/protocol.gen/DigestLine.ts`, `WorkNotice.ts`).

**Docs**: theseus-core, theseus-protocol and theseusd AGENTS.md; `theseus answer --help`'s long text; docs/cli.md.
The root AGENTS.md is already 20,473 bytes (over its own 20 KB rule), so I added nothing to it.

**Existing tests changed** (none weakened; the keel guard prints nothing): the daemon tests that answer as the owner now
do so from a pty (`tests/common/terminal.rs`): `digest.rs`, `acceptance_journey.rs`; `common::safe_note` lists a host of
its own (`example-agent`) so where the tests run (here: under an agent's shell) decides nothing. Answerer literals gained
`agent: None`; `digest_line_with` gained a `by` argument.

## How I proved it

New tests: `tests_m3/agent_answers.rs` (6), `peer_proc::tests` (7), protocol `tests_work_digest` (2) and
`tests_notices` (1), theseusd `tests/agent_answers.rs` (2: a real CLI).

- `cargo nextest run --workspace -E 'package(theseus-core) & test(/confirm|answer|approval|peer|digest|aws|judge/)'`
  (the brief's `-p` form builds a second feature set that fills this VM's disk; I used the gate's `--workspace` form
  with the same filters): 538 passed.
- `... -E '(package(theseus-protocol) | package(theseus) | package(theseus-discord)) & test(/digest|answer/)'`: 140 passed.
- theseusd: `binary(acceptance_journey) | binary(digest) | binary(agent_answers) | binary(job_approval)`: 15 passed.
- `cargo fmt --all --check`, `bash scripts/shape.sh`, `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- The gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`), twice. Phases door, keel, fmt, shape, features,
  clippy, clippy-probe, cockpit (lint, test, build), test build and the reader rule pass. The suite: 7055 tests, **7015
  passed, 40 failed**: 33 are the known L1 failures (theseus-sandbox's contract and bench tests and `theseusd::sandbox`; the VM
  runs as root, theseus-pv6i), and 7 are theseus-discord task-thread tests (`tests_task_threads::*`,
  `tests_task_channels::a_short_task_*`, `tests_war_room::a_shared_place_refuses_*`), each a 101 s wait for "a thread for
  the task". Those are not mine: a task that finishes in 0.1 s ends before its thread is made, and
  `tests_task_threads::react::in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing` fails alone on ae7724f too (checked
  on a checkout of it); which of the seven fail changes run to run. The first gate run also failed three of my own
  daemon tests (a CLI answer from a test process now reads as an agent's): my `agent_answers` budget test sent `approve`
  (a budget's answer is `reset`), and `digest` and `acceptance_journey` answered without a terminal; fixed in 7430a7f and
  rerun green. `theseus-core rpc::tests_ordered::a_stop_after_an_input_reaches_its_running_turn` failed once in the first
  run, passed in the second and alone (0.09 s): a flake, not on the list.
- The phases after the suite, by hand (the suite failed only on the above): protocol types (`git diff` of
  `cockpit/src/protocol.gen` clean once added), `theseus-sim bench turn --check --runs 5 --burst 0` (4 frames a plain turn;
  first-request tokens cli 7,332 of 8,000, dm 9,411 of 15,000, task 6,585 of 8,000; `tests_first_request` is in the suite),
  `cargo deny --offline check`: bans, licenses and sources ok; **advisories FAILED** on RUSTSEC-2026-0367, -0368 and -0369
  (the `zip` crate), from a database newer than this tree; Cargo.lock is untouched by me. The lifecycle and jobs benches
  and the size check are skipped by `THESEUS_GATE_NO_BENCH` and for want of a release build.
- No runs under load (the row asks for none).

### Planted reverts (each restored, file `touch`ed, `git status` clean)

1. **KEY.** `crates/theseus-core/src/rpc/confirms.rs`, in `judge_act`:
   `if who.agent.is_some() && crate::approval::owners_alone(&action.tool) =>` became
   `if who.agent.is_some() && false && crate::approval::owners_alone(&action.tool) =>`. Failed:
   `tests_m3::agent_answers::an_agents_approve_of_a_stack_apply_is_refused_and_the_owners_binds` at `agent_answers.rs:94`
   ("an agent does not answer a stack apply: Object {"approved": Bool(true), …}": the agent's approve ran), and
   `an_agent_cannot_reset_a_spend_limit_and_the_owner_can`.
2. **Classification.** `crates/theseus-core/src/peer_proc.rs`, first line of `behind_in`: added
   `let agents: &[String] = &[];` (the list ignored). Failed
   `peer_proc::tests::an_agent_above_it_names_it_even_on_a_terminal` (`left: Owner, right: Agent("claude")`, `peer_proc.rs:222`).
3. **Digest words.** `crates/theseus-protocol/src/work_digest.rs`, `digest_words`: `DigestKind::Answered => match by {`
   became `match None::<&str> {`. Failed protocol `work_digest::tests::an_agents_answer_says_so_and_the_owners_and_an_old_lines_say_you`
   (left `✓ you answered: aws.stack.apply`, right `✓ answered by an agent (no terminal): aws.stack.apply`) and core
   `tests_m3::agent_answers::the_digest_names_an_agent_that_answered_and_says_you_for_the_owner` ("from the ring: ✓ you answered: fs.write …").
4. **The board's ring.** `crates/theseus-core/src/push/work.rs`, `act_row`'s `"action.confirm_answered"` arm:
   `.filter(|b| theseus_protocol::agent_of(b).is_some())` became `… .is_some() && false)`. Failed the same core digest
   test, from the ring (the ledger path still said "an agent").
5. **A real CLI.** Plant 1 again, then theseusd `agent_answers`: failed
   `a_cli_answer_with_no_terminal_is_refused_for_a_spend_limit_and_a_terminal_resets_it` at `agent_answers.rs:204`
   (the no-terminal `answer … reset` printed "answered … · reset … resumes").

An old ledger row with no class reads "you answered": protocol test above (a line without `by` deserializes to `None`
and is worded as the owner's) and every existing digest test (the owner's rows carry `the CLI`).

## The live check for the maintainer

On a scratch daemon of the install build (own `--config`, `--socket`, `--state-dir`, never the operator's), with
`SOCK=<its socket>`:

```
# a question that spends money: a tiny spend limit and a turn that exceeds it (or a real aws.stack.* question in the DM)
setsid theseus --socket $SOCK answer <id> reset --no-wait ; echo $?
#   refused: "theseus: this spends money or changes AWS, so only the owner answers it: press it in the DM, or answer
#   from your own terminal"; exit 1. `theseus --socket $SOCK answer` still lists it; the DM card is still open; the
#   ledger has an approval.refused row with by "agent:no terminal".
theseus --socket $SOCK answer <id> reset          # from your own terminal: it runs (budget.reset by "the CLI")
# an ordinary approval, from an agent's exec (no terminal):
setsid theseus --socket $SOCK answer <fs.write id> approve --no-wait   # runs
theseus --socket $SOCK digest        # "✓ answered by an agent (no terminal): fs.write …" (and "1 answered by an agent")
```

Under a real agent host (e.g. from inside a Claude Code shell, which has a terminal-less or listed ancestor) it reads
`agent:claude`. From your own shell, on a terminal, no agent above it: "✓ you answered". On the real account, an agent's
`theseus answer <aws.stack.apply id> approve` is refused and the DM card still presses.

## What is left, uncertain, and for the owner to hear

- **The owner's config should add the gateway's own program names** to `[policy] agent_processes` (the default holds the
  common coding-agent CLIs and `openclaw`). The list is a light guard, as the job marker is: an agent that opens a pty
  and hides its ancestry reads as the owner's; L1 and the DM's card are the boundary.
- **The web UI is unchanged and is a gap.** A web answer is the owner's by the loopback uid check alone; an agent
  of the same user can send `work.answer` over the web port (`peer::client_inode` could find its process, as the old
  walk did). Not built: the brief said unchanged.
- **`aws bootstrap` apply** (`rpc/aws.rs`, CLI only, "on the operator's yes") is not an answer and is not covered.
- **A `--stdio` daemon's client is its parent process.** `theseus --spawn ask` under an agent reads as the agent's; with
  no parent read (`pid` None, a test's connection) the answer is the owner's, as before.
- **`confirm.list`'s `may_answer`** now depends on the question for an agent's connection (a small addition beyond the
  brief; its test is `confirm_list_says_an_agent_may_not_answer_a_money_question`).
- `Client::behind` (test-only, `cfg(test)`) is how core tests say who is behind a connection; daemon tests use real
  processes (a pty for the owner, `setsid` for no terminal, a copy of `sh` named for a listed host).
- **Disk on this VM**: the writable allowance (about 39 GB with the toolchain) holds one build flavor of the workspace's
  tests, not two. I used `CARGO_INCREMENTAL=0` and the gate's `--workspace` form throughout, and deleted stale
  duplicate artifacts in `target/` (inside the repository) twice when a second flavor filled it.
- Docs for the maintainer to write (not touched): the spec's Part III item and `docs/status.md` (an answer's author is
  read from `/proc`; money and AWS are the owner's; `agent_processes`); the root AGENTS.md's "security posture" bullet
  could say the same in one line once it is back under 20 KB.

## Keel

`python3 scripts/keel-guard.py --base ae7724f --head HEAD --quiet` prints nothing: no test removed, ignored or
weakened, no budget, cap or ceiling raised, and no keel-guarded file touched. So there are no per-line `Keel:` trailers
to sign. The policy change itself, for the maintainer to sign at the join:

```
Keel: policy crates/theseus-core/src/approval.rs, crates/theseus-core/src/rpc/confirms.rs — the owner, 2026-10-19 19:11: "5. your recommendation is good" (agent answers labelled as the agent's; money and AWS answers the owner's own)
```

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run twice, ends `FAILED in suite` for these alone:
33 L1 tests (theseus-sandbox's contract and bench tests and theseusd's `sandbox`, root VM, theseus-pv6i) and
7 theseus-discord task-thread tests (baseline-flaky, see above); every other phase passed or was run by hand as above
(deny's advisories fail on the `zip` crate's new RUSTSEC entries, not on this change).
