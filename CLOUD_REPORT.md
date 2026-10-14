# CLOUD_REPORT: self-pr, the branch builder (theseus-pw1q.8)

Branch `cloud/20261013-self-pr`, cut from main at `9fa84735`. Store format unchanged (32): the step adds a ledger
kind (`self.branch.built`), files under `<state>/self/`, and no stored record or field.

## The step

### What I found

- **No scrub script and no may-build rule live in the repo.** The brief says to reuse "the same scrub check the outer
  loop runs over docs/ (find it under scripts/)" and "the may-build rule if the repo has one". Neither is under
  `scripts/`: the spec mentions `may-build.sh` (Items 159, 194, 234), but it lives on the owner's machine, not in the
  tree. So I built both as hooks on **main's** copy. The push runs the daemon's own scrubber (every vault value and
  every secret shape, `scrub.rs`) over each line the branch adds and each commit message. When main holds
  `scripts/scrub.sh`, it also runs that, read from the `origin/main` blob and never from the tree. When main holds
  `scripts/may-build.sh`, the gate waits for it to say yes, asking once a minute for up to 30 minutes. Today neither
  file exists, so neither hook runs.
- **Builds cannot run as an L1 job yet.** L1's view keeps the workspace read-only and throws away whatever a job
  writes, it has no network, and on a root daemon it refuses every job (theseus-pv6i). A gate needs a tree and a
  build tree it can write, so it cannot run inside L1. What it runs as instead, an owned child at background
  priority, is described under "What I changed". **For the owner:** a real L1 build needs L1 to be able to mount a
  writable tree and target directory. That is a sandbox change and keel.
- **A tree under `<state>` sits outside the workspace roots.** `fs.write` there would wait for approval every time.
  The floor is narrower than the whole state directory (only `store/`, `spool/`, the bindings file, the vault token
  and the config copy), so I made `<state>/self/trees` join the workspace roots, but only while `[self] mode = "act"`
  and `[self] repo` is set. **This is a widening of the file tools' reach that the owner turns on.** It applies to
  every private session, not only `"self"` ones, because roots are global (`toolrun/guarded.rs`). See "Left and
  uncertain".
- **Nothing in the code opens a `"self"` session yet.** Conversations open as `operator`, MCP clients as their own
  principal, and tasks inherit their parent's. Seam S3 (self-backlog) opens one. Until it lands, a daemon cannot
  reach the tool's real path. The tests open a `"self"` execution directly through the kernel.
- **The tool list is filtered by place, never by principal.** So under `mode = "act"` the tool is offered in every
  private session. Outside a self session it is refused at run time and writes nothing (tested, and seen live).
  Filtering by principal needs the principal in `TurnRunner::spec_of`, and `turn.rs` is 29 lines under its ceiling.
- **`toolrun.rs` was at 2,499 lines, one under 2,500.** I moved `guarded_paths` whole into `toolrun/guarded.rs`, and
  the file is now 2,479.
- **Defect, outside my area:** `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
  fails every time on this VM, and identically on main with my changes stashed. It times out after 20 s at "the
  thinking's create, held, and the reply's post". It is not on the known list. It is in discord-calls' area, so I
  left it alone.

### What I changed

One commit, `b217c2b`: `self: the branch builder: a tree of its own, main's gate through the door, a frozen report, a self/ branch (theseus-pw1q.8)`.

- **Config** (`config/rsi.rs`, the template's `[self]` block, placed before `[self.budget]`):
  - `repo` (default unset): no self branches, and the tool says so.
  - `remote` (default unset): branches stay local. A push uses the credential git already has for that remote. No
    code here reads a secret.
- **`rsi::tree`** (seam S6, `rsi/tree.rs`): `Place { repo, state, env }`, `Tree { slug, branch, dir, target }`, and
  these calls:
  - `open` makes a worktree off `origin/main` at `<state>/self/trees/<slug>`, on a new branch
    `self/<YYYYMMDD>-<slug>` (`-2`, `-3`… when the name is taken), with `CARGO_TARGET_DIR` at
    `<state>/self/target/<slug>`. It refuses trees inside the repo.
  - `gate` copies main's `scripts/gate.sh` blob to `<state>/self/gates/gate-<blob>.sh` and runs it with
    `THESEUS_GATE_ROOT=<tree>`. It runs under `nice -n 19`, and `ionice -c 3` where ionice exists, as an owned child
    in its own process group. It sets `THESEUS_GATE_NO_BENCH=1`, an empty `RUSTC_WRAPPER` (no sccache) and its own
    target, drops every `THESEUS_*` variable, and stops after 2 h. It links `target -> <its target>` in the tree,
    because the gate runs `target/debug/theseus-sim`. Its log goes to `<state>/self/gates/<branch>.log`.
  - The verdict is `green` (exit 0), `keel` (`gate: FAILED in keel` or `in door`), or `red`.
  - `push` sends `refs/heads/<b>:refs/heads/<b>` only. It refuses any name outside `self/` (with `check-ref-format`
    too), and no `+` (force) can appear. The scrub runs first, and a hit stops the push.
  - `close` removes the worktree, its target and the link. The branch stays.
  - `next_branch`, `commit`, `commits`, `diffstat`, `branches`.
- **`self.propose_change { item, issue, why }`** (`rsi/pr.rs`; dispatched in `toolrun/self_pr.rs`): a Harness tool,
  registered only under `mode = "act"`. It runs only for an execution whose `authority.principal` is `"self"`.
  - The first call for an item opens its tree and returns its path, for editing with the file tools.
  - The next call:
    1. asks `rsi::gate` before each of commit, gate and push;
    2. commits as Theseus, subject `self: <why> (<item>)`;
    3. runs main's gate;
    4. writes the frozen report at `<state>/self/reports/<branch with / as ->.md`, mode 0444, scrubbed, never
       overwritten, ending `<!-- REPORT COMPLETE -->`;
    5. pushes a green branch when `remote` is set, then closes its tree.
  - A red attempt keeps its tree, and the next attempt goes on a new branch with its own report.
  - **Every attempt writes one `self.branch.built` row** (`fact/rsi_pr.rs`), with the `SELF_ROWS` fields plus
    `issue` and `stopped_at`. A halt between phases writes `gate: "halted"` and `stopped_at` naming the phase.
  - **A keel verdict** posts a `self_keel` card to the owner's DM (`Outbox::stage_to_operator`), and writes the mark
    `<state>/self/trees/<slug>.keel`. The item's later calls are refused, so nothing retries around it.
  - Before a tree is open, a halt starts nothing and writes no row.
- **`self.branches`** (`rpc/rsi.rs`, protocol types at the end of `theseus-protocol/src/rsi.rs` under a `self-pr`
  comment, TypeScript regenerated) and **`theseus self branches`** (`self_cmd/branches.rs`, one match arm). Together
  they show the repo and remote, then each `self/*` branch with its head, its report's verdict, its report and its
  open tree.
- **Small edits in shared files:**
  - `LedgerKind::SelfBranchBuilt`, `method::SELF_BRANCHES` (on the existing line), and the ts.rs list (on the
    existing line).
  - `facts!`, `core_tools()` and the template's tool-list test: one line each.
  - `[policy.tools]` gets `# "self.propose_change" = "notify"`.
  - `rpc/mod.rs`: the board's `attach`.
  - **theseus-discord's `courier.rs`** (discord-calls' area): `"self_keel"` added to the DM-only kinds beside
    `"self_digest"`. This is one token in an existing match arm.
  - `crates/theseus-core/AGENTS.md`: a paragraph under the self-improvement spine.
- **The tool's prompt** is its description, schema and results, with no examples and no "MUST". The results carry
  the branch, head, verdict, report and log, plus the gate's tail when it failed.

### How I proved it

- **The tests** (all new except where noted):
  - `rsi/tests_tree.rs` (6), over a bare upstream, a clone, and a stand-in `gate.sh`, all with hermetic git:
    - the tree is apart from the repo and has its own target; the repo's work tree is untouched;
    - main's gate judges a tree that wrote a weakened `gate.sh`: red, blob checked, `THESEUS_SESSION` absent,
      the tree's target used;
    - a keel verdict;
    - a push lands only `refs/heads/self/…`, and `main`, `refs/heads/main`, `self/`, `self/../main`, `+self/x` and
      `self/x:main` are refused;
    - a token shape (built from parts) in a file, or in a commit message, stops the push, and so does main's
      `scripts/scrub.sh`;
    - close.
  - `rsi/tests_pr.rs` (6), through a whole core with a stand-in model:
    - mode off has no tool, and it is not in the request; act offers it;
    - an `operator` session is refused, with no tree and no row;
    - green: one row, the report frozen, complete and read-only, pushed to `self/*`, the tree and target gone,
      listed by `self.branches`;
    - red then green: the tree is kept, the second attempt is on `<branch>-2`, two rows;
    - a halt: row `halted` with `stopped_at: "commit"`, nothing pushed;
    - the keel: row `keel`, one `self_keel` card, and the next call refused with no new row.
  - `config::rsi::tests::the_self_repo_and_remote_are_unset_by_default` and the CLI's
    `self_cmd::branches::tests::the_branches_say_their_verdict_report_and_tree`.
  - `example_template_uncommented_still_parses`'s tool count changed from 55 to 56, for the one new line.
- **Planted reverts**, each restored and `touch`ed, with `git status` checked after:

  | Planted bug | Test that failed |
  |---|---|
  | The gate runs the tree's own `gate.sh` | `the_gate_is_mains_copy_even_when_the_tree_edits_its_own`, at "red" |
  | `is_self_branch("main")` is true | `a_push_goes_to_self_only_and_main_is_refused` |
  | Scrub hits ignored | `a_scrub_hit_stops_the_push` |
  | The principal check off | `a_session_that_is_not_self_cannot_call_it` |
  | The halt check before the commit removed | `a_halt_between_phases_stops_it_and_the_row_says_where` |
  | The tool registered in mode off | `mode_off_offers_no_tool_and_act_does` |
  | The keel mark ignored | `a_keel_finding_stops_at_the_owners_card_and_is_never_retried` |

- **Under load:** 5 rounds of the 15 new and changed tests at `nice -n 19`, beside four busy loops at nice 0
  (`yes > /dev/null`: the harness refused `sh -c 'while :; do :; done'`). 15/15 passed in every round.
- **FAST:**
  - `theseus-sim bench turn --runs 5 --burst 0 --check`: plain turn 4 frames, tool-call turn 8, batch 14, all
    unchanged. The brief's "5 and 9" is older than theseus-2uby.
  - Mode off adds nothing to the hot path: no tool and no new root.
  - Each phase's self check is `rsi::gate`, one read of the cached switch.
- **Live, on a scratch daemon of this build** (fresh `/tmp/live/state`, the stand-in model on loopback, Jev, Discord
  and the web UI off):
  - **Mode act:**
    - `theseus self resume`, then `theseus self branches`, printed: "Built from /tmp/live/repo's origin/main; green
      branches are pushed to origin. No self branch yet."
    - `theseus ask "propose the deck fix"` (the stand-in calls `self_propose_change`):
      - the request offered `self_propose_change`;
      - the result was `Not run: self.propose_change runs only in a self session (principal "self"), and this
        session's is "operator".`;
      - no `<state>/self` was made;
      - the CLI's roots line named `/tmp/live/state/self/trees`.
  - **Mode off** (its own state dir):
    - the request held 33 tools and none was `self_*`;
    - the roots line named only the projects dir;
    - `self branches` said "mode off".
  - **Not done live:** the full build (a stand-in model making a one-line fix). No daemon path opens a `"self"`
    session until self-backlog (S3) lands. The in-process tests cover that path with real git, a real worktree and a
    real gate script.

### The live check for the maintainer

The setup below uses a fixture repo with a stand-in `scripts/gate.sh` that fails while `deck.txt` holds "teh", so a
one-line fix turns it green.

1. **Build the fixture** (paths are examples):
   ```bash
   export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=f@example.invalid GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=f@example.invalid
   L=/tmp/selfpr && mkdir -p $L/seed/scripts $L/work && cd $L
   printf '#!/bin/bash\ncd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"\nif grep -q teh deck.txt; then echo "gate: FAILED in suite"; exit 1; fi\necho "gate: ok"\n' > seed/scripts/gate.sh
   printf 'the mast holds\nteh sails hold\n' > seed/deck.txt; printf 'target\n' > seed/.gitignore
   git -C seed init -q -b main && git -C seed add -A && git -C seed commit -qm seed
   git clone -q --bare seed upstream.git && git clone -q upstream.git repo
   ```
2. **Write a scratch config:** the template with every `op://` reference set to `env:FAKE_KEY`; `[model] api_base`
   on `theseus-sim fake-model`; Discord and the web UI off; `[judge] enabled = false`;
   `[tools] projects_dir = "$L/work"`; `[server]` state and socket under `$L`; and:
   ```toml
   [self]
   mode = "act"
   repo = "/tmp/selfpr/repo"
   remote = "origin"
   ```
3. **Without a self session** (works today):
   - Start the daemon, then run `theseus self resume`.
   - `theseus ask "propose the deck fix"` should show the result `Not run: … runs only in a self session …` and
     write no `$L/state/self`.
   - `theseus self branches` should show "No self branch yet."
4. **The full flow, once self-backlog's `"self"` task opener has joined:**
   - Open a self task and script the stand-in model to make three calls:
     1. `self_propose_change {item: "deck-typo", issue: "theseus-deck1", why: "Fix the deck's typo."}`;
     2. `fs_edit {path: "$L/state/self/trees/deck-typo/deck.txt", old_string: "teh", new_string: "the"}`;
     3. the same `self_propose_change` again.
   - The second call should say `…: the gate is green (exit 0, …); report …/self-<date>-deck-typo.md. Pushed to
     origin refs/heads/self/<date>-deck-typo:… The tree and its build tree are closed; the branch stays.`
   - `git -C $L/upstream.git for-each-ref` should list `refs/heads/main` and `refs/heads/self/<date>-deck-typo` only.
   - The report should end `<!-- REPORT COMPLETE -->` and be mode 0444.
   - `theseus self log` should show one `self.branch.built … gate green` row.
   - `theseus self branches` should list the branch with `gate green` and its report.
5. **On the owner's real checkout** (not in a scratch check): `repo` would name a real checkout, and the gate would
   be the real `scripts/gate.sh`. A first build is about 22 minutes cold at nice 19 beside busy neighbours.

### Left and uncertain; choices for the owner

- **The trees joining the workspace roots** under act and repo is a permission widening. The owner's own two settings
  turn it on. It applies to every private session, not only self ones. Narrowing it to `"self"` sessions needs the
  principal in the gate's order (`toolrun/order.rs`), like the tool-list filter below. I'd call it keel-adjacent and
  want the owner's word.
- **The tool is offered in every private session under act** and refused outside self ones. Filtering by principal
  needs `spec_of` to know the principal, and `turn.rs` has 29 lines of room.
- **Builds are not L1** (see above). The gate runs as an owned child in its own process group at background priority.
  It has no job cgroup of its own: `cgroup::Job` is private to the kernel and is made by a job wrapper.
- **Only a green branch is pushed.** A red or keel branch stays local, with its report.
- **No `git fetch` at open.** The tree branches from the repo's `origin/main` as it stands, so the owner's checkout
  must be fetched. A fetch would be network I/O the brief did not ask for.
- **A cancel or `/stop` does not cut a running attempt short**, like every Harness call. The kill switch is asked
  between phases, and the gate is bounded at 2 h. An attempt holds its turn's admission slot while the gate runs;
  self sessions are the task class, the lowest.
- **The keel's mark is a file the owner removes** (`<state>/self/trees/<slug>.keel`) to let the item go on. No
  answer verb exists yet: the card says to ack it with a signed `Keel:` trailer or to drop the branch.
- **A restart mid-attempt** answers `INTERRUPTED`. The tree is found again on the next call; nothing is done twice.
- **Docs to update at review:**
  - `docs/status.md`: the row and the landed step;
  - the spec's Part III item;
  - `docs/design/rsi-plan.md`'s self-pr row: the door's blob run, the `self_keel` card, and the roots;
  - `scripts/AGENTS.md`'s "The door": `rsi::tree::gate` is a caller that "trusts nothing in the tree".

## Keel findings expected

None. `python3 scripts/keel-guard.py` → `keel: ok (a branch: merge-base(HEAD, origin/main)..HEAD, 9fa84735c0; 29 files
changed, 0 findings acked, 0.3 s)`. The template test's tool count changed from 55 to 56, an equality that keeps its
meaning, and the guard found nothing in it.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, with `CARGO_INCREMENTAL=0`: the first run filled the
disk with 16 GB of incremental caches, which I deleted, as they are regenerable.

- **Phases:** door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build and the reader rule passed.
- **The suite:** 4,351 run, 4,317 passed, 34 failed, 47 skipped.
  - **33 are the known L1 tests:** theseus-sandbox's contract tests and `spawn_100`, and theseusd's sandbox tests,
    on a root VM (theseus-pv6i).
  - **1 is `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`.** It
    is not on the known list, fails alone every time, and fails identically on main with this branch stashed.
- **The phases after the suite, run by hand:**
  - the protocol types are clean once committed;
  - `cargo deny --offline check`: advisories, bans, licenses and sources ok;
  - the turn bench: 4, 8 and 14 frames.
- **Second gate**, before the report's commit, on the same tree: 4,351 run, 4,316 passed, 35 failed. The failures
  were the same 34 plus `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`.
  - It is not on the known list. It is a count: `gone_jobs.rs:358`, "the live wrapper's job runs on:
    {"dispatched":2,"outcome_unknown":3,"succeeded":11}", `left: 2` (once `3`), `right: 1`.
  - Alone it failed 1 of 3 runs on this branch, and **2 of 8 on main `9fa84735` checked out by itself**, failing at
    the same line. So it is a pre-existing intermittent failure on this 4-core VM, not this branch's.
  - It has the shape of a negative check: a wrapper whose job should run on is settled as well. It is in the
    gone-jobs area (theseus-vej5), so it is worth an issue.
