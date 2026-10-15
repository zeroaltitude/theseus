# CLOUD_REPORT: haiku-tools (theseus-jpjcd)

## Step: turns on a model without tool search reach deferred tools

**Found.** `defer_step.rs::deferral` chose `Mechanism::Find` for a catalog entry with `tool_search: false` and loaded
only the place kind's families; the rest hid behind `tools_find`, which the observed Haiku never calls. A detour to
Haiku compiles fresh (`current: None`), so its request always takes the fresh set: that is where a fix applies.

**Choice: (a), by the message's own words, not (b) or (c).** The route verdict (`route.v3`) carries mode, judgment and
effort but no tool signal; adding a question to the pack means a new prompt pack, shadow period and labels, far more
than the bug. (b) would send tool-needing turns to Sonnet and cost more. (c) is a measurement that needs live Haiku
calls, which this task forbids. So `tools_defer/want.rs` applies for the model what `tools_find` would have done: it
matches the turn's message against the hidden tools. A family loads when the message says a tool's whole wire name of
two words or more (`aws_whoami`, or "aws whoami"; 2 points) or the family's name (1 point; not the everyday words
code, self, tasks, web, memory, files, commands, channels, extensions). Best first, each kept only while the request's
tool tokens (bytes / 2.4, as the estimate counts) stay within `PlaceKind::tools_budget_tokens`: 8,000 for cli, task and
headless; 15,000 for dm and channel. No new config key.

**Changed** (commit cf627d05): `tools_defer/want.rs` (new), `family.rs` (`tools_budget_tokens`), `find.rs` (`tokens`
visible to the module), `turn/defer_step.rs` (`deferral_for(.., ask)`; `deferral` is unchanged for census, keep_warm and
context_parts, which pass no text), `turn/inbound_step.rs` and `route_step.rs` (`RouteState.ask`, the first 4,000
characters of the message, set before the judge-off return), core `AGENTS.md`. No store format change, no protocol
change, no new dependency. Mechanism `Search` never reads `ask`, so its first request is byte for byte as before
(tested).

**Proved.**
- `cargo nextest run -p theseus-core -E 'test(/defer|want/)'`: 66 passed. New: `tests_defer::a_model_that_never_calls_tools_find_still_reaches_the_family_the_message_names`
  (Haiku stand-in, never calls `tools_find`, calls `term_open` once offered; 1 tool call, no `tools.missed`, and the
  next turn's tool bytes identical), `a_message_that_names_nothing_leaves_the_fallback_request_as_it_was`,
  `a_model_with_tool_search_sends_the_same_first_request_whatever_the_message_names`, and four in `want::tests`
  (naming, everyday words, order, the budget by place kind).
- Planted revert (`if false && mechanism == Mechanism::Find` in `deferral_for`): the Haiku test FAILED at
  tests_defer.rs:643 (term_open not offered). File restored and touched; `git status` clean.
- Caught on the way: my first version set `ask` in `correction_point`, which the judge-off path never reaches; the
  test failed and the text moved to the top of `inbound_point`.
- Full `theseus-core` suite: 2629 passed, 21 skipped. Not run under load (no timing in the change).
- fmt check, shape, `cargo clippy --workspace --all-targets -- -D warnings`: clean.

**Gate.** `scripts/gate.sh` fails at `keel` (and `door`) on a finding that is not from this branch: `crates/theseus-sim/src/lifecycle.rs:155`
`budget-raised`, whose ack commit 1c2beb2747 (and 238ecead06) is refused here because its signature does not verify
(`%G? is E`, the owner's key is not in this VM's keyring). My diff touches neither. I then ran the later phases myself:
fmt, shape, clippy pass; `cargo deny` not run (no `cargo deny fetch` done; offline check needs the database); the
workspace suite could not be built here because the VM's disk allowance ran out (rustc "No space left on device"), so
only `-p theseus-core` ran (the only crate changed). Benches skipped (`THESEUS_GATE_NO_BENCH=1`). The maintainer's
full gate on the owner's machine is needed for the other crates' suites, cockpit and deny. `python3 scripts/keel-guard.py`
finding: only the lifecycle.rs one above. Keel findings expected: none from this change.

**Live check for the maintainer** (a scratch daemon on a fresh state dir, a config whose router sends to Haiku 5.5):
1. `theseus --socket S send "who am I on AWS? call aws_whoami"` then `theseus --socket S tools`: expect `aws_whoami`
   offered, a `tools.used` row for family `aws` and no `tools.missed`; before the fix, 0 tool calls.
2. `theseus --socket S context explain` on that turn: the tool list holds the `aws` family and `tools_find`; tool tokens under 8,000 (cli) / 15,000 (dm).
3. The same prompt on Sonnet: first request unchanged (search tool first, `aws` deferred).

**Left / uncertain.**
- An append keeps the conversation's recorded set (the cache design), so a Haiku conversation already running with
  `aws` hidden does not gain it when a later message names it, until a boundary (compaction, recompile) or a detour. If
  the owner wants it sooner, the miss path could request a recompile; that rewrites the cached prefix, so I left it.
- The match is lexical (the message's words against wire names and family names), so a request in other words ("what
  account am I in") is not caught; `tools_find` stays as the fallback. A Jev question for it is the follow-up.
- Docs to update at review: spec Part III item for theseus-jpjcd; the AGENTS.md paragraph is written.
- The cargo-deny database was not fetched; the VM's disk allowance (about 38 GB used) forced deleting workspace build
  artifacts mid-run.
