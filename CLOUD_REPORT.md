# Cloud report: step 43a, a proposed extension, frozen, started in L1, tested, and put to the operator (theseus-ext.5)

Branch `cloud/20261004-extend-propose`, on `main` at e457555 (the task's commit f2938e0 on top).
Started 08:50 UTC and finished 10:15 UTC, inside the 5-hour deadline.

| Commit | Subject |
|---|---|
| ec16ef6 | mcp: a stdio server with sandbox = "l1" runs in L1 |
| 84fdf45 | extend: extend.propose freezes a server, tries it in L1, and asks the operator |
| e39cb9d | extend: a stop during the trial answers the call stopped and leaves nothing on the board |
| 720784b | theseusd: the extension's daemon test reads `theseus extend list` and health's count |

There are no new dependencies, so Cargo.lock and package-lock.json are unchanged. The store's format stays at 7 (see
"Design choices"). Every item of the brief is built, the CLI and health's count included.

## Setup on this VM

- Two `rustup` toolchain installs ran at once and raced, so the first cold build failed with "component download failed".
  Run one after the other, they worked.
- nextest, cargo-deny, `npm ci`, and `cargo deny fetch` all worked. The gate's deny phase ran offline:
  advisories, bans, licenses, and sources ok.
- **The L1 tests as uid 65534.** I ran each test binary like this, from `/tmp`:
  `setpriv --reuid=65534 --regid=65534 --clear-groups env HOME=/tmp TMPDIR=/tmp /home/user/theseus/target/debug/deps/<test>-<hash>`.
  - The binaries are `mcp_l1-*` and `extend-*`, built by `cargo build --workspace --all-targets`.
  - That uid can read the repository and its `target/` (both mode 755), and the tests make their temp dirs under `/tmp`.
  - Both test files also run as root, where they check L1's refusal instead (below).

## Step 1: MCP servers in L1 (ec16ef6)

**What I found.** 36b refused `sandbox = "l1"`. The sandbox's `spawn(spec, init, Stdio)` already accepts a stdin, so no
change to theseus-sandbox was needed. What was missing was a long-lived process to hold the init and its pipes.

**What I changed.**
- **The role.** A new role of the daemon's own binary, `theseusd mcp-sandbox` (`crates/theseus-kernel/src/mcp_l1.rs`).
  `main` dispatches it second, right after `job-sandbox`.
- **How the board starts it.** The board spawns the role as it spawns any stdio server
  (`crates/theseus-core/src/mcp/l1.rs`): through `children::spawn(Kind::Owned)`, in its own process group, with stdin,
  stdout, and stderr piped. The role's spec rides in `THESEUS_MCP_L1`, and the server's environment is the spec's alone.
- **The pipes.** The role hands its own stdin, stdout, and stderr to the init and keeps no end of the pipes. So the
  server talks straight to the board, its stderr goes to `<state>/mcp/<name>.log`, and a daemon that closes the pipes
  ends the server.
- **The view.** The server gets the view a job gets (`Sandbox::job_view`): the workspace read-only under overlays,
  `ro_paths`, the floor and socket hidden, and the job limits. It also gets its own `egress` list through
  `job_egress`'s proxy. An empty list means no listener, and so no network.
- **How it ends.** The role waits on three things:
  - the init's pidfd;
  - a signalfd for SIGTERM, SIGINT, and SIGHUP: SIGTERM to the init, a 2 s grace, then SIGKILL, which ends the
    namespace;
  - the daemon's pidfd: a `kill -9` of the daemon sends SIGKILL to the init at once. The init's parent-death signal
    covers the role itself being killed.
- **The config.**
  - `sandbox = "l1"` now loads for a stdio server. An HTTP server is refused.
  - `egress = [...]` is valid on an L1 server only.
  - `l0` stays the default.
  - There is a field the config never reads, `frozen` (`#[serde(skip)]`): an extension's copy, which becomes the
    server's working directory and a read-only bind.
  - The template's `[mcp.servers]` gained an L1 example, which `the_templates_mcp_section` checks.
- **No fallback to L0.** A root daemon's start fails with L1's refusal, and the server never becomes ready.
- **AGENTS.md.** I updated the AGENTS.md files of theseus-core, theseus-kernel, theseusd, and theseus-sandbox.

**How I proved it.**
- `crates/theseusd/tests/mcp_l1.rs` uses the real daemon, role, and init, with `theseus-sim fake-mcp` bound read-only
  into the view. As uid 65534, 2 of 2 pass:
  - `a_server_in_l1_starts_answers_sees_no_network_and_ends_with_the_daemons_stop` checks that:
    - the server is ready with its 5 tools, and `mcp.list` lists them;
    - the process tree is `mcp-sandbox`, then `job-sandbox`, then `fake-mcp`;
    - the server's network namespace differs from the test's, `/proc/<server>/net/dev` holds only `lo`, and
      `net/route` has no route;
    - after `shutdown`, the role, the init, and the server are all gone.
  - `a_server_in_l1_ends_with_the_daemons_kill_9`: all three are gone after the daemon's SIGKILL.
- As root, the first test checks that the server is not ready, has no pid, and that `last_error` names RLIMIT_NPROC.
  The second test returns early. To confirm the 65534 run really started servers, I added a temporary trace of the
  process tree, then removed it.
- Config unit tests: `an_l1_server_loads_with_its_egress_list`, plus three new refusal cases in
  `unknown_keys_and_bad_servers_are_refused`. The kernel's `mcp_l1` has three unit tests: the spec, a round trip with
  no secret value in its debug form, and the exit status.
- **No planted revert for the role's daemon pidfd.** The fake server ends by itself when its stdin closes, so removing
  the pidfd watch would not make the `kill -9` test fail. The watch is defence in depth for a server that ignores the
  end of its input.

## Step 2: extend.propose (84fdf45, e39cb9d, 720784b)

**What I changed.** Most of it is in `crates/theseus-core/src/extend/`, with small additions to shared files.

- **The tool.** `extend.propose { name, dir, command, description, tests?, network? }` is class `Run`, retry
  `NonRepeatable`, and a harness tool. The template's `[policy.tools]` gets a commented `"extend.propose" = "notify"`
  line; the template tests count it, so its posture is notify, inherited from `enforcement`.
  - `dir` must resolve inside the workspace roots. This is checked in `plan` and again at the run.
  - It waits under T1's hold like any `Run` call.
  - Its trial waits on a server, so the turn awaits it (`ToolRuntime::run_extend`) as it awaits an async tool. It runs
    as a spawned task tracked by `stops`, so a cancel or a `/stop` aborts it. It is never run on a core.
- **The freeze** (`extend/freeze.rs`). It copies `dir` into `<state>/extensions/<name>/<digest>/`. The digest is a
  SHA-256 over the sorted tree: each entry's path and kind, its mode, its length, and its bytes. The mode is kept as
  755 or 644, as git keeps it. That way the read-only copy (555 or 444) digests the same as its source, and an
  executable bit still counts.
  - A symbolic link is refused, so nothing frozen can point back at something an edit changes.
  - A tree is capped at 512 entries and 16 MiB.
  - The copy is made beside its final place, sealed read-only, checked against the digest, and then renamed into place.
- **The trial** (`mcp/trial.rs`).
  - The board starts the frozen copy as `ext-<name>` through its own `Connect`, so in L1 through the role. It uses the
    proposal's network list, gives no secret and no environment beyond the job's, and puts the server in a new state,
    `proposed`.
  - A server on trial is never one of the board's configured servers. So `rebuild` never offers its tools.
  - The trial runs `initialize`, `tools/list`, and each test as a `tools/call`. A test expects either `contains` or
    `equals`, and an error result fails it.
  - When its `Trial` value is dropped, the server is stopped. A guard takes the server off the trial list if the start
    fails or is aborted (that was the e39cb9d fix).
  - Health's `mcp[]` shows the server while it is on trial.
- **The manifest.** It is a META record, `extend.manifest.<name>.<digest>`, holding:
  - the name, digest, description, command, source, and frozen path;
  - the tools, with their schemas;
  - the tests, with whether each passed, what came back (cut to 500 characters), and why it failed;
  - the capabilities: network and scratch;
  - who proposed it: the session, execution, call, principal, and place;
  - its state (`proposed`, `failed`, `acked`, or `declined`) and who answered.

  The call's result shows it, and its `meta` carries it whole.
- **The ack.**
  - The question is a planned `extend.ack` action on the proposing execution (`plan_confirm_with`, which reserves
    nothing). Its card and the manifest are written in the same frame.
  - `confirm.list`, the card, and the push show it as `extend.ack`, with the reason "Load wordcount 3f2a1c: 5 tools, 2
    of 3 tests passed, no network?".
  - It is answered by `action.confirm`, after `judge_act`'s place rule (`extend/answer.rs`). An ack binds the
    question's confirm and writes `extend.acked`. A decline writes `extend.declined`, and so does a question nobody
    answered within `confirm_ttl`. `action.confirm_answered` is written as for any answer.
  - Nothing wakes the execution, no turn runs, and nothing loads.
  - A server that does not come up is recorded with the manifest state `failed`, and nothing is asked.
- **Rows and surfaces.**
  - The facts `extend.proposed`, `extend.tested`, `extend.acked`, and `extend.declined`, each with its narrative line
    (`fact/extend.rs`).
  - The protocol module `extend.rs` and the method `extend.list`.
  - `theseus extend list` (`crates/theseus/src/extend.rs`).
  - Health's `extensions`: proposals, waiting, acked, declined, and failed.
  - The cockpit's generated types, regenerated.
  - `theseus confirm` prints "approved … · nothing resumes: an extension's ack loads nothing in this build" for an
    approval that resumes nothing. Before, it printed the budget question's wording.
- **A restart mid-proposal.** The call is run again, as harness calls are, and answers from its manifest if one exists.
  Otherwise it says to propose again.

**How I proved it.**
- `extend/tests.rs`: 7 tests through the whole core, with the in-process fake MCP server standing in for the frozen
  copy. The stand-in picks the fake's mode from a `mode` file in the directory it is given, so whatever runs is
  whatever that directory holds.
  - `a_proposal_is_frozen_tried_in_l1_and_put_to_the_operator`:
    - the digest is the tree's;
    - the frozen copy alone is started, as `ext-wordcount`, in L1, with `frozen` set and no egress or environment;
    - 5 tools and the results `[pass, pass, fail]` are recorded, with the failure's reason;
    - the two rows are written, the question waits with the exact reason above, and `tool.confirm_requested` is
      written;
    - nothing runs once the trial is over;
    - `extend.list` and health's count are right.
  - `an_edit_after_proposing_changes_nothing_that_runs`: the workspace's `mode` file is edited after the freeze and
    before the start, and the tests still pass as frozen. The same tree proposed again gives the same digest and the
    same copy.
  - `no_tool_is_offered_before_the_ack_nor_after_it`: no `mcp__` tool is in the catalog while the trial runs, none is
    offered at the next turn, and none once acked. After the ack, `confirm.list` is empty and health counts 1 acked.
  - `an_ack_from_a_shared_place_is_refused`: the owner pressing in a guild channel is refused with `approval.refused`.
    The question still waits and the manifest is still `proposed`. The owner's DM counts (`via: discord:dm`).
  - `a_decline_loads_nothing_and_neither_does_an_expiry`:
    - after a decline, `extend.declined` carries the note, the action is Cancelled, the catalog is empty, no model
      request is made, and the execution is still Waiting;
    - an expiry (`expire_questions`) declines the second proposal with by `expiry`, and no turn runs.
  - `what_cannot_be_tried_is_said_and_nothing_is_asked`: a tree outside the roots is refused. A server that exits at
    its start gives the manifest state `failed`, no question, every test failed, and `error` on its row.
  - `a_stop_during_the_trial_ends_it_and_asks_nothing`: a server that never answers its handshake, then `/stop`. The
    call is Cancelled "stopped by cli", no question is asked, there is no `extend.tested` row, and the board is empty.
- `extend/freeze.rs` has 3 unit tests: the digest is stable and follows every byte, mode, and name; the copy is
  read-only and unchanged by a later edit; a link is refused.
- `crates/theseusd/tests/extend.rs`: the real daemon, a stand-in Messages API asking for the proposal, and a word
  counter whose `server.sh` execs `theseus-sim fake-mcp`. As uid 65534:
  - 2 of 2 tests passed in L1;
  - health's `mcp[]` is empty after the trial, and no zombie is left;
  - `confirm.list` shows `extend.ack` ending in ": 5 tools, 2 of 2 tests passed, no network?";
  - `theseus extend list` shows `proposed  2 of 2 tests passed  no network` and the `theseus confirm <id>` that
    answers it;
  - `theseus confirm <id>` with `THESEUS_SESSION` set is refused ("theseus confirm refused"), and the question still
    waits;
  - without it, `theseus confirm <id>` acks: `extend.acked` is written, `extend list` says `acked by`, health counts 1
    acked, `mcp.list` has no tools, and `confirm.list` is empty.

  As root it checks the refusal: the result names RLIMIT_NPROC, `extend.tested` has an `error`, nothing is asked, and
  `extend list` says `failed`.

**Planted reverts.** Each was restored from a copy and `touch`ed, with `git status` checked after.

| Planted bug | What failed |
|---|---|
| The trial started from the workspace's dir instead of the frozen copy (`trial_cfg(&i.command, &src, …)`) | `an_edit_after_proposing_changes_nothing_that_runs` (`[false, false, false]`: the edited `error` mode ran) and `a_proposal_is_frozen_tried_in_l1_and_put_to_the_operator` (`frozen` was the workspace path) |
| A proposed server's tools offered: `rebuild` chained the trial list, and `trial()` called `rebuild()` | `no_tool_is_offered_before_the_ack_nor_after_it` (the offered list held `mcp__ext-wordcount__add`, `__echo`, and the other three) and `a_decline_loads_nothing_and_neither_does_an_expiry` |
| Before e39cb9d's fix (the test written first, then the fix) | `a_stop_during_the_trial…` failed twice: first on an `Error` result where `Cancelled` belongs, then, with the result fixed, on a trial left `proposed` on the board |

**Runs under load** (nice 19, with four `yes > /dev/null` busy loops at nice 0, killed by their own pids). The
recipe's `sh -c 'while :; do :; done'` loop was blocked by this session's command checker, so I used `yes` instead.
- The core's `extend::` and `mcp::` tests, 5 runs: 22 of 22 each time.
- theseusd's `tests/extend.rs` and `tests/mcp_l1.rs` as uid 65534, 5 runs each: all passed (about 7–8 s and 3–4 s a
  run under the load).
- I added the "no zombie" check to `tests/extend.rs` after those runs. It passed once as 65534 with no load.

## Design choices the owner should hear about

1. **The manifest is a META record, not a node.** There is no `kind: extension` node body and no `trust` field, and
   adding either would be a new record kind or field and a format bump. Like 36b's `mcp.tools.<server>`, the META
   record keeps the store at format 7, and the proposing call's result shows it in full. The `derived_from` edge the
   design names is replaced by `proposed_by.correlation_id` in the manifest.
2. **The ack is a kernel question**, a planned `extend.ack` action, not a new card type.
   - So every existing surface answers it with no change: Discord's buttons, `theseus confirm`, the cockpit's card,
     `confirm.list`, and the push.
   - An ack binds the action's confirm and leaves it planned, so 43b can authorize and dispatch it as "the load".
   - A decline or expiry settles it Cancelled.
   - It belongs to the proposing execution, so a `/stop` or a cancel of that session declines it. That matches the
     design's "its ceiling is the proposing execution's at the ack".
3. **It is judged with `Act::Answer`, not a new `Act::Extend`.** The rule is the same (the owner, from a private
   place), and other branches add to `Act`.
4. **Discord's buttons still read Approve and Decline.** The card's text carries "Load wordcount 3f2a1c: …?".
   Relabelling them Load and Decline is a small change in theseus-discord's `render.rs`, which other changes touch.
5. **The digest's mode is git-like (755 or 644)**, so the read-only copy digests the same as its source. A link in the
   tree is refused outright.
6. **The trial is not a configured server.** It never enters `servers`, so the "offer" path is structurally closed.
   43b's load will need to add acked extensions to the catalog explicitly.

## What is left or uncertain

- **A proposal while an earlier one still waits.** A second proposal of the same name and digest overwrites the
  manifest's `question` with the new one, while the older question still waits. Answering either one updates the same
  manifest.
- **Two proposals of the same name at once** share one trial-list key. The first to end takes the entry off the list
  for both, which affects health's display only.
- **A failed proposal leaves its frozen copy.** Nothing deletes what cannot be rebuilt.
- **A trial aborted mid-handshake leaves no `extend.tested` row.** It is reported in the call's result only.
- **Not done here (43b):** loading on the ack, restart, revoke, `/extensions`, and acked extensions' posture under
  `[policy.mcp] "ext-<name>"`.
- **The cockpit's view of a proposal** (I left the cockpit alone) should show:
  - the manifest: name, digest, command, capabilities, who proposed it and from where;
  - the frozen files, readable from `<state>/extensions/<name>/<digest>/`, which needs a read method;
  - a diff against the version it would replace: the newest acked manifest of the same name;
  - each test with its arguments, expectation, result, and reason;
  - the Load and Decline buttons, which `action.confirm` already answers.
- **Docs for the maintainer to update.**
  - `docs/design/m7-surface.md` §2.7 and its 43a entry: the manifest is a META record and the ack a kernel question
    (choices 1 and 2), and `judge_act` uses `Act::Answer`.
  - The spec's Part III item, `docs/status.md`, and `docs/design/README.md` if it lists M7 rows.
  - The 36b note that L1 servers are a follow-up is now done.

## The live check (the maintainer's, on the owner's machine: L1 refuses a root daemon)

This uses a scratch daemon on a fresh state dir, the fake Discord, and GLM. Ports follow scripts/AGENTS.md.

```bash
S=$(mktemp -d /tmp/ext-live.XXXX); mkdir -p "$S/work"
theseus-sim fake-discord --addr 127.0.0.1:9447 --gateway 127.0.0.1:9449 --log "$S/discord.log" &
FAKE=$!
# The scratch config: the GLM profile live; [tools] projects_dir = "$S/work"; [discord] enabled, with
# rest_proxy = "127.0.0.1:9447" and gateway_proxy = "ws://127.0.0.1:9449"; a bindings file whose DM with the
# owner is bound (the owner = [places] owner); the web UI on 7435 or off.
theseusd --config "$S/config.toml" --state-dir "$S/state" --socket "$S/sock" 2>"$S/theseusd.log" &
D=$!
theseus --socket "$S/sock" health | grep -i -A3 sandbox       # L1 not refused (no RLIMIT_NPROC line)
theseus --socket "$S/sock" ask "In $S/work/wc write a tiny stdio MCP server in Python (python3, no packages): \
one tool, count_words { text }, answering the number of words as text. Then propose it with extend_propose: \
name wordcount, command [\"python3\", \"server.py\"], and three tests, among them \
{tool: count_words, arguments: {text: \"one two three\"}, expect: {equals: \"3\"}}."
```

What each step should show:

1. The answer quotes the call's result: `Proposed extension wordcount <6 hex> … In L1 it listed 1 tool: count_words …
   3 of 3 tests passed. The operator is asked: "Load wordcount <6 hex>: 1 tool, 3 of 3 tests passed, no network?"`.
2. `theseus-sim discord read` shows the card in the owner's DM: "**Approve?** `extend.ack` …" with "Load wordcount …:
   1 tool, 3 of 3 tests passed, no network?", and Approve and Decline buttons.
3. `theseus --socket "$S/sock" confirm` lists `extend.ack` with that reason. Then run
   `theseus --socket "$S/sock" confirm <id>`. It prints "approved <id> · session … · nothing resumes …", and the
   Discord card settles to approved (`discord read` again).
4. `theseus --socket "$S/sock" ledger --kind extend.proposed`, then `--kind extend.tested`, then `--kind
   extend.acked`: one row each. `extend.tested` has `"passed": 3, "tests": 3`, and `extend.acked` has `"by"` the CLI's
   label.
5. `theseus --socket "$S/sock" extend list` shows `wordcount <hex>  acked  3 of 3 tests passed  no network` and
   `acked by …`.
6. `theseus --socket "$S/sock" mcp` lists no server and no tool. Ask a new turn "what tools do you have?": no
   `mcp__ext-` tool is offered.
7. `ls -l "$S/state/extensions/wordcount/"*/` shows the frozen copy, mode r-x or r--. `cat
   "$S/state/mcp/ext-wordcount.log"` holds the server's stderr from its trial.
8. Clean up with `theseus --socket "$S/sock" shutdown`, then `kill $FAKE`. Then `chmod -R u+w "$S/state/extensions"`
   before removing `$S`.

It costs only the model's tokens. A second run, as a decline, should give `extend.declined`, `extend list` saying
`declined`, and still no tool offered.

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree (720784b):

- fmt, shape, features, clippy, cockpit (lint, test, build), test build, and the reader rule (9 of 9) all passed.
- **The suite:** 1,993 tests run, 1,960 passed (1 slow), 33 failed, 17 skipped. All 33 failures are the L1 tests a root
  VM fails (theseus-pv6i), the same on every commit of this branch:
  - `theseus-sandbox::contract`: 20 cases (every clause, egress_18b ×3, exit_status_and_signals,
    scratch_is_reported_and_discarded, sigterm_is_forwarded_to_the_command, a_job_that_cannot_start_says_why);
  - `theseus-sandbox::bench spawn_100`;
  - `theseusd::sandbox`: 12 cases (a_probe_script_in_l1…, l1_argv_routes_a_call_to_l1,
    health_reports_the_last_real_l1_launch, the egress cases, the grant cases, the cancel case, the_jobs_bench_l1_row,
    a_running_l1_jobs_command_is_read_before_its_row_and_matches_it, a_host_beyond_the_list_once_approved_is_outside_text).
- `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` passed only on a retry in two of my five
  gate runs. It is on `.config/nextest.toml`'s flaky list.
- One gate run of commit 84fdf45 first failed `theseusd::default_config` and `theseusd::extend` because the build was
  stale. I had restored the files from a tar of the full change, with their old mtimes, after committing step 1. After
  `touch`ing every file I had restored, the rerun was clean, and that is the run this report counts.
- **After the suite, run by hand:**
  - protocol types: ok (`cockpit/src/protocol.gen` committed);
  - turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`): plain 5 of 5 frames, tool 9 of 9;
  - deny (offline): advisories, bans, licenses, and sources ok.
  - Lifecycle and jobs were skipped: `THESEUS_GATE_NO_BENCH`.
- **The timezone.** I ran the gate with `TZ=America/Los_Angeles` because `tests_output::the_cores_output_matches_its_golden`
  pins a negative UTC offset in a `wake.at` result line ("… #:#:# -#:# …"). Under this VM's UTC, its only difference is
  "+#:#". It is unrelated to this change, and it passes with any negative-offset zone.
