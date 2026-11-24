# How Theseus is developed

Theseus is built by AI agents in small, reviewed steps. The rules on this page apply to whoever does the work: an agent
session on the owner's machine, a cloud coding session, or, since v1, Theseus itself. They cover where work happens,
what proves it, how it reaches `main`, how it is installed and released, how it is measured, and what Theseus may
change about itself without asking.

Five other documents go deeper, and this page links to them rather than repeating them:

- the root [`AGENTS.md`](../AGENTS.md): the principles that bind code, the crate map, and the generated files;
- [`scripts/AGENTS.md`](../scripts/AGENTS.md): the scripts, and the gate's phases in order;
- [`scripts/RELEASES.md`](../scripts/RELEASES.md): the pinned toolchain, the build script and the release profiles,
  the reproducibility check, the installed service, the smokes, and the build host — moved out of
  `scripts/AGENTS.md` on 2026-11-10, verbatim, to get that file back under the context loader's
  64 KiB ceiling;
- [`scripts/GATE.md`](../scripts/GATE.md): running the gate on a busy machine — the disk floor, the shared lock,
  the bench busy allowance, and how long it takes — moved out of the same file on the same day, the same way;
- [`design/rsi-plan.md`](design/rsi-plan.md): the self-improvement design.

## Contents

1. [The shape of the work](#the-shape-of-the-work)
2. [Where work happens](#where-work-happens)
3. [Proof: what every change carries](#proof-what-every-change-carries)
4. [The gate](#the-gate)
5. [The keel](#the-keel)
6. [Shape: files and functions](#shape-files-and-functions)
7. [Review](#review)
8. [Joins](#joins)
9. [Installing](#installing)
10. [Version cuts and releases](#version-cuts-and-releases)
11. [Budgets the code is held to](#budgets-the-code-is-held-to)
12. [The store's format](#the-stores-format)
13. [Configuration changes](#configuration-changes)
14. [Self-improvement](#self-improvement)
15. [Branch hygiene](#branch-hygiene)
16. [Benchmarks](#benchmarks)
17. [The docs that move with the code](#the-docs-that-move-with-the-code)
18. [Commits](#commits)
19. [Models and roles](#models-and-roles)

## The shape of the work

```text
 issue ──► branch ──► gate ──► review ──► join ──► main's gate ──► push ──► install ──► docs
            │          (no      │          (plain    (with the       (never
            │          benches) │          merge)    benches)        forced)
   lane, cloud row,        written review, planted
   or self branch          revert, live check
```

- **One spine.** `main` moves one join at a time, and only through a join whose gate ran on the merge itself.
- **Many branches beside it.** Independent work runs in parallel. Each piece has its own branch, worktree and build
  directory, and reaches `main` only at a join.
- **Every step is proven, reviewed and recorded.** Each step has a failing test first, a planted revert, a live check
  on a scratch daemon, and a written review by a different session. A docs commit then records what was built, in the
  spec's Part III and in [`status.md`](status.md).
- **The owner decides the keel; everything else acts, then tells.** Some changes need the owner's signed yes:
  deleting or loosening a test, budget or ceiling; a one-way store change; a security, policy or spend change.
  Everything else is built, gated, joined and installed, and the owner is told afterwards what changed.

## Where work happens

### The spine

The spine is the checkout of `main`. Joins happen there, the gate runs there with its benches, and installs are built
from it. Nobody develops in it.

### Lanes

A lane is local work beside the spine:

- **Its own worktree**, on branch `lane/<name>`.
- **Its own build directory, the worktree's own `target/`.** Never use the spine's `target/`: a shared target directory
  poisons both trees' builds. Don't use `CARGO_TARGET_DIR` either, because the gate runs `target/debug/theseus-sim` by
  a relative path. Delete the directory when the lane ends.
- **Its own crate or directory.** Shared files change only at the join:
  - `scripts/gate.sh` and `deny.toml`;
  - the cockpit's routes and `cockpit/src/protocol.ts`;
  - the CLI's `main.rs`;
  - any dispatch `match`.

  The one exception is the line that adds a lane's crate to the workspace's `members`.
- **A niced gate without the timing benches**: `THESEUS_GATE_NO_BENCH=1 nice -n 19 scripts/gate.sh`, with
  `CARGO_BUILD_JOBS=4` and `NEXTEST_TEST_THREADS=4`. A lane that touches the start path also runs the lifecycle bench
  once before its join: alone, at normal priority, with the result quoted.

**The main working tree stays on `main`.** Work on, test or review another commit in a worktree of your own; read
one with `git show <commit>:<path>`. `scripts/main-tree-guard.sh install` (run by `scripts/setup.sh`) installs a hook
that puts the main tree straight back on `main` when a checkout moves it, and fails that checkout (issue 15). No hook
fires for `git reset` or `git merge`, so the guard does not cover them: in the main tree they are for the join's
fast-forward and reset of `main` and the install's fast-forward, never for looking at another commit. A bisect there
is left to finish. Not covered either: `git checkout -B main <commit>`, which moves `main` itself, and a rebase: one of
`main` ends on `main`, but `git rebase <upstream> <other branch>` can end on that branch, and with git's default
backend nothing the hook says during a rebase is shown.

A lane pushes only its own branch. The machine has room for only a few compiling trees at once. Each tree is many
gigabytes, and the whole machine slows past three concurrent compiles. So a build starts only when a capacity check
says there is room, and idle lanes are closed rather than kept.

### Cloud rows

A cloud row is one piece of work done by a cloud coding session on its own branch, `cloud/<date>-<slug>`, cut from a
named commit of `main`.

- **The task is the branch's first commit**, `CLOUD_TASK.md`. It holds:
  - the task and the files in play;
  - the acceptance tests and the planted reverts to make;
  - where `main` is: its commit, store format, and the long-file ceilings at that commit;
  - the sibling rows of the same batch, and which files each one owns;
  - the shared rules every row follows: tests first, nothing lost, no new always-loaded tool text, no store format
    change, no new dependency, no person's name, and never loosening a test, budget, cap or ceiling.
  - **both halves of the keel, named as two things** ([The keel](#the-keel)): (a) the guarded FILE list, about
    *where* the row edits, and (b) the erosion rules, about *what its edit does, in any file anywhere in the tree*.
    A brief that clears (a) has said nothing about (b), and a row told "that file is not keel-class, so no `Keel:`
    trailer is needed" has been told something false. A brief's ruling that deletes or weakens an assertion carries
    the owner's quote, or is marked in the brief as still needing one; a row may never invent it.
- **The report is its last commit**, `CLOUD_REPORT.md`, with the subject `cloud report (not for main)`. It says what
  the row built, the commands that proved it, its planted reverts, the keel guard's output, what it could not run, and
  what it left open. The review reads the report but trusts the diff and its own runs, never the session's own words.
- **A cloud gate skips the timing benches.** The cloud machine is small and shared, so the join's gate runs the
  benches. A row that changes what the lifecycle bench drives (the daemon's start and stop, the turn path, the Discord
  binding) has the bench run on the merge before its join gate.
- **Rows come in batches**: several independent rows cut from one commit, each in its own area, so their joins
  conflict as little as possible. A row that depends on another is staged until that one joins.

Cloud commits are unsigned. The join's merge commit is signed.

### Self branches

Since v1, Theseus builds branches for its own backlog, named `self/<date>-<slug>`. Each lives in a dedicated clone, in
a worktree with its own build directory under the daemon's state directory. Its gate runs below the sandbox floor:
no network, and only the tree and its build directory are writable. See [Self-improvement](#self-improvement).

## Proof: what every change carries

Each of these rules was learned the hard way; the items are in the spec's Part III.

- **A failing test first.** A fix starts from a test that fails the way the bug did.
- **Goldens before a move.** A refactor starts with goldens of today's behaviour and ends byte-identical. A change that
  must alter a golden rewrites it with `THESEUS_GOLDEN=write`, and the review checks that only the intended lines moved.
- **A planted revert.** Switch the fix off and see the new test fail as the bug did, then put the fix back.
  - Restore the file with a fresh mtime (`touch` it). Otherwise cargo keeps the reverted build, and the "fixed" run
    proves nothing.
  - A branch whose tests pass with its fix undone is unproved, and is rejected.
  - A planted revert that no test catches is a finding: add the test, then plant again.
- **A live check on a scratch daemon** for every change in behaviour.
  - Run the step's own build, with its own `--config`, `--socket` and `--state-dir`.
  - Use a fresh state directory or a copy of a store, never the operator's own daemon.
  - Turn `[discord]` and `[web]` off unless the check needs them, and check that a port is free right before binding
    it.
- **A race is reproduced under load before it is fixed.** Priority makes the load, not the number of processes: run
  the load at a *higher* priority than the test (the test niced, the load not). Prove "waits for nothing" on tokio's
  paused clock rather than by timing it.
- **A flaky negative assertion is a finding.** If "nothing of X reached Y" fails once, the leak happens, sometimes.
  Such a test never goes on the flaky list.
- **Hermetic tests.** A fixture never inherits the operator's global config, git's included. Git calls in fixtures
  turn automatic maintenance off.
- **Invented names** in fixtures, tests and commits: the repository is public.

## The gate

`scripts/gate.sh` runs before every commit, in the tree being committed. Chain the commit to it with `&&`: a `;` once
let an unformatted commit through.

```bash
THESEUS_GATE_NO_BENCH=1 scripts/gate.sh && git commit -S -F msg.txt     # a branch
scripts/gate.sh && git commit -S -F msg.txt                             # a join, on main: with the benches
```

The phases run in this order, and the first failure stops the gate:

| Phase | Lock | What it holds |
|---|---|---|
| `door` | no | When this tree's `gate.sh` differs from `main`'s, `main`'s copy judges the tree first |
| `keel`, `keel tests` | no | No test, budget, cap or ceiling deleted or loosened without the owner's signed ack ([The keel](#the-keel)) |
| `fmt` | no | `cargo fmt --all -- --check` |
| `shape` | no | No Rust file over 2,500 lines unless listed with a ceiling |
| `status page` | no | `docs/status.md`'s version line names the workspace version and the newest release tag, and each tag's tree holds its own version (`scripts/status-page.py`) |
| `features` | no | The shipped binaries, built alone, get the same features as in the workspace |
| `clippy` | no | `-D warnings` over the workspace, which also holds the function budget |
| cockpit | no | `npm run lint`, `npm test`, `npm run build` |
| `test build` | no | Builds everything the suite and the benches run, once |
| reader rule | **yes** | Nothing is declared without its reader |
| suite | **yes** | `cargo nextest run --workspace` |
| protocol types | **yes** | the generator test rewrites `cockpit/src/protocol.gen/`, and it must match the commit |
| lifecycle bench | **yes** | The start, shutdown, crash-restart and binary-swap budgets (skipped on a branch) |
| jobs bench | **yes** | An L1 job's start p95 (skipped on a branch) |
| turn bench | **yes** | A plain turn's frames, counted |
| first request | **yes** | Each kind of place's first request, in tokens, against its budget |
| size | no | The release binary's growth, when a release build of this commit exists |
| `deny` | no | Licences, advisories, bans and sources, offline |

- **The lock.** The tests and benches need the machine to themselves, so the gate takes one machine-wide lock, and only
  around the locked phases; it compiles outside the lock. Never wrap the gate in `flock`: it would wait for itself, and
  it refuses to start when a parent process holds the lock.
- **The busy allowance.** Before the benches, the gate waits up to two minutes for a quiet machine. If it finds none,
  the timing budgets (never the counts) get a calibrated overage. A miss reruns once, and only a second miss fails.
  - Runs carried by the allowance are marked in the bench history.
  - A string of them means either a regression that the busy machine hid, or a machine that stays busy. Read them
    before raising the allowance.
- **The flaky list.** A test may be listed for two retries if it fails under load by construction and an issue exists
  to fix it.
  - One test at a time, never a blanket retry.
  - No retry is hidden: the gate prints and logs every test that passed only on a retry.
  - A flaky test that is not on the list fails the gate.
- **The flake hunt: no test may fail by chance.** `scripts/flake-hunt.sh N [--load] [--threads T] [--view]` runs the
  gate's own suite command N whole times and counts, per test, the runs it failed in. Run it on a build host, never on
  the owner's machine: a run is minutes of every core. Before a release, ten of each:
  `scripts/flake-hunt.sh 10 --load` (twice the cores in CPU burners beside the suite),
  `scripts/flake-hunt.sh 10 --threads 96` (the suite crowding itself) and
  `scripts/flake-hunt.sh 10 --view` (inside the sandbox the self loop's join runs the gate in).
  - Every failed test is printed with its own output and kept under `target/flake-hunt/`, with a table at the end. A
    test that fails once in ten runs is a finding: fix its cause (a real clock, a shared port or path, an order
    dependence); never add a retry for it.
  - `--filter EXPR` narrows the hunt to the tests of a nextest filterset, to run one test two hundred times.
  - A test that measures real elapsed time against a short limit runs on tokio's paused clock when its timer is the
    code's own (`#[tokio::test(start_paused = true)]` and `tokio::time::Instant`), or is given a limit load cannot
    reach, or says why it is sound as it is.
  - **A speed promise in a test** is a bound on how fast the product answers (a stop within a second, a change reaching
    an adapter within 10 ms), as opposed to a mechanism (an order, a count, a timer the code owns) or a fixture's
    patience (how long a test waits for something to happen at all). Write a promise with
    `theseus_tools::Busy` (and `theseus_sim::Busy`): `let busy = Busy::before();` just before the timed
    region, then `busy.keeps(elapsed, bound)` for each bound, and put `{busy}` in the message. On a quiet machine the
    factor reads 1 and the bound is exactly the promise; on a machine that gives each thread a fraction of a core it is
    held in proportion to how slowly a thread got its turn, before the region and when the bound is passed, never more
    than `BOUND_CAP` (10) times, so a product that is really slower still fails. `Busy::when_late()` reads the machine
    only when a bound is passed (a long region, or many). A limit that is an input (a fake's latency and the margin
    under it) grows with its bound by `theseus_tools::bound_factor()`.
  - What scaling cannot fix, and a test must not: a bound set just under a constant of the product (a backstop, a
    grace, an answer window), because a scaled bound could then no longer tell the behaviour from a slow machine.
    Those tests say so in their docs; the product's constants are never scaled.
  - **What a fixed limit cannot fix: measure the thing the promise names, on the clock that owns it.** A wall clock
    from a request to a process's exit holds the harness's own polling, the client's connect and the process's
    teardown, which a crowd starves as much as the product (the outbox's grace test read 1 s to 3.5 s beside 64 busy
    loops while the daemon's own log put its whole stop at 74 ms and its wait for the post at 51 ms). When the product
    times the step itself (a `waited_ms` on a log line, a `stop phase` line), the test reads that; the `Busy` limit is
    then over the quantity the test's name promises.
  - **Wait for the event, not for a quiet window or a fixed length of run.** A background writer the test is not about
    (the memory pass's `memory.labeled` rows, ten of them, a second or more after the turns) outlasts any window of
    quiet: switch it off in that rig, or filter its rows out as the cancel-frames test does. A job that prints for four
    seconds and is read at seven is a test of the sampler's luck; let the job print until the test has what it needs
    and says enough. A notification that follows another (`work.notice` after the `work.changed` that ends a question)
    is waited for, never read at the moment the first arrives.
  - **A race between a notice and a step is ordered by hand, not raced.** The stop's notice to the turns of its
    execution (`StopNotice`) and the kernel's own stop can come in either order; the test lands the notice between the
    turn's hold and its admission with `admit_hook`, and calls the stop's second half (`Core::stop_after_landing`)
    after the turn has ended, so the order is the test's and no clock is read.
  - **A test never picks a port and then hands it to a daemon.** Binding `127.0.0.1:0`, dropping the listener and
    passing the number is a promise the kernel does not keep: the port is free for whoever asks next, another test's
    daemon among them, and the test then talks to that one, which serves the same pages and goes away with its own
    test (an empty status line, `its own page at localhost: `, 1 gate in many). Configure `port = 0` and read the port
    from the daemon's log (`Served::web_port`). To see the fault, narrow the ephemeral range in a namespace of your
    own (`unshare -Urn`, then `/proc/sys/net/ipv4/ip_local_port_range` to `40000 43999`) and hammer the old and the new
    code: 65 of 1000 against 0 of 1000.
  - **A hammer finds a 3 % race in minutes.** Whole hunts of one test (`--filter`, one run after another) found the
    stop test 4 times in 150; the test's own binary run 600 times, 16 at a time, beside the burners (`cargo test
    --no-run --message-format=json` names the binary; `xargs -P 16`; each run's output kept) found it 17 times in
    600 in five minutes, and showed the same cause in each failing log, where a debug print planted on the box
    (never committed) named the two stops. Confirm a fix with the same hammer: 0 of 600.

- **A gate at 0 % CPU is waiting on a lock**: this one, or cargo's package cache. The gate's log names the holder;
  find it before waiting longer.
- **The gate's own cost has a history**: a per-phase timing table, printed at the end and appended to a CSV. Read it
  before calling a gate slow.
- **Give the gate time.** Waiting for the lock and for a quiet machine can each add minutes, and a cold tree's first
  gate compiles everything. Give its command a timeout of at least 30 minutes.

## The keel

The keel is what no agent, Theseus included, may erode on its own. `scripts/keel-guard.py`, the gate's first phase,
judges the net change of the range against `main`. It reports each erosion as `path:line: rule: old -> new`:

| Rule | What it catches |
|---|---|
| `test-removed`, `test-ignored` | A test deleted, or marked ignored or skipped |
| `assert-removed`, `assert-weakened` | An assertion deleted, or loosened in place: an equality made a truth test, a bound loosened, an assertion made vacuous, commented out or discarded |
| `allow-added` | A new lint `allow` or `expect`, or a looser clippy threshold |
| `budget-raised` | A lifecycle, turn, jobs or first-request budget, or the busy allowance's default |
| `cap-raised` | A spend or loop cap: the benchmarks', the config template's, or a cap's default in code |
| `ceiling-raised` | A long-file ceiling, the shape limit, or nextest's retries and timeouts |
| `ci-loosened` | A CI step removed or changed, or made to continue on error |
| `keel-file` | The guard itself, its test suite, the gate's door, or a gate phase removed |
| `sealed-reach` | Code outside the sealed scorer's own files that names the sealed holdout sets |
| `locked-flow` | The owner-locked Discord DM task flow changed: a golden under `crates/theseus-discord/src/golden/` edited or removed. The trailer quotes his OK for that change |
| `owner-reach` | A change inside a function that decides who may act as the owner, or from where (the place rule's owner checks and which places count as private, `judge_act`, which surfaces and tools are the owner's alone and the free AWS reads an agent may answer, a channel's privacy, a guild's trust), or one of them gone from its file. It reads inside those items only: a function they call, or a new caller that goes around them, is review's to catch |

**Two halves, and a brief must say which it means.** The table above mixes them, and the confusion
has cost a gate twice.

- **(a) Where you edit: the guarded FILE list.** `KEEL_FILES`, `KEEL_PREFIXES` and `LOCKED_FLOW_PREFIXES` in
  `scripts/keel-guard.py` name files whose edit needs an ack whatever the edit does. Those are the `keel-file` and
  `locked-flow` rules, and they are the only two that are about a list of files.
- **(b) What your edit does: the EROSION rules.** `test-removed`, `test-ignored`, `assert-removed`,
  `assert-weakened`, `allow-added`, `budget-raised`, `cap-raised`, `ceiling-raised` and `ci-loosened` judge the
  change itself, in **any file anywhere in the tree**, guarded or not. A file nobody has ever called keel-class that
  loses one assertion fails the gate in about a second, before a single test runs
  (`tests_route_cliff.rs:117: assert-removed: 3 assertions -> 2` did exactly that on 2026-11-08).
- **Clearing (a) says nothing about (b).** "The file is not in `KEEL_FILES`" therefore never means "no `Keel:`
  trailer is needed", and a brief or a ruling that says so is wrong. A ruling that deletes or weakens an assertion
  comes with the owner's quote, or is marked as needing one.

Moved and renamed tests are matched by name, then by body, then by similarity, so a refactor is not an erosion. The
last of those three is a guess, and across files in a batch it misattributes: a test deleted on one branch pairs with
an unrelated new test on another, the finding changes rule, `test-removed` to `assert-removed`, and file, onto a file
that eroded nothing, and the trailer that named the real file stops covering it. **Never ack across a batch
boundary.** When a finding names a file the change never touched, `git grep` the symbol at the base and at the head
first and prove where the erosion actually is; `scripts/keel-batch-repro.sh` reproduces it. Writing the trailer the
guard's own remedy text asks for there would be a false statement that the owner authorised an erosion where none
exists, and that is never written.

- **The ack.** A finding passes only when a commit in the range carries a trailer
  `Keel: <rule> <path>[, <path>…] — <why>`, signed by a key listed in `scripts/keel-signers` whose class covers
  the rule.
  - The list is read as it is at the base, so a branch cannot list its own key.
  - An unsigned trailer, or one signed by a key the list does not name, counts for nothing. An ssh signature is
    checked against the list's own ssh keys, never the machine's allowed-signers file.
  - `python3 scripts/keel-guard.py --base <main> --head HEAD --quiet` prints the exact lines to fill in.
- **Two classes of key (issue 15; the owner's "yes to all!", 2026-11-14 12:15 MST).**
  - **`routine`**, the key the agents who build this repository sign with, acks `test-removed` only: a test removed
    or replaced, which review checks anyway.
  - **`owner`** acks every rule. His key is an ssh key held by 1Password on his own desktop, which asks him to approve
    each signature, in a vault the agents cannot read (named in the list on his word, 2026-11-14). He gives the
    signature from chat: he says yes in his DM, the agent runs `scripts/keel-ack.sh`, and his approval signs. So every
    ack but `test-removed` now needs that prompt on his screen; a prompt he did not expect is the alarm.
  - **Signing from WSL.** Linux `ssh-keygen` inside WSL does not reach 1Password; `scripts/keel-ssh-sign.sh`, which
    `keel-ack.sh` passes as git's ssh program for that one commit, hands the signature to Windows' own ssh-keygen.
    The machine needs 1Password's ssh agent on, Windows' "OpenSSH Authentication Agent" service stopped, and Windows
    OpenSSH at `C:\Windows\System32\OpenSSH`. Elsewhere it is plain `ssh-keygen` and the operator's own agent.
  - **`owner none`** in the list means no owner key: nothing can sign his class. The guard keeps that reading, and
    refuses `owner none` beside an owner key.
- **What the classes enforce, and what they do not.** Against the agents that build this repository, the
  restriction to `test-removed` is now the key's, not only the brief's. Against Theseus itself, the self-improvement
  loop signs with the key `user.signingkey` names in `[self] repo` (`crates/theseus-core/src/rsi/tree.rs`); if that
  is the agents' key, a self branch can ack `test-removed` and nothing else, and independently of any key, a change
  that touches a keel **path class** stops at the owner's card (`crates/theseus-core/src/rsi/join/keel.rs`). No rule
  is exempt from the ack; every rule is the owner's to sign.
- **Who writes a `Keel:` trailer.** A `test-removed` ack is the agents' own: the owner allowed "a test was removed
  or replaced" without him (2026-11-14). An agent writes it, signed with the agents' key, says why in its report, and
  review checks it. Every other ack is the owner's: written only after he has said yes to that specific erosion,
  quoting the yes in the trailer's reason, and signed with his key (`scripts/keel-ack.sh`).
  - A vendored crate's `allow` is still `allow-added`.
  - Any edit to the guard is still `keel-file`, even one that tightens it.
  - Without a yes, avoid the finding: restore the test rather than ask for an ack to remove it.
- **What "his yes" has to be before an agent may prepare a trailer of his class: all five, or none.** The bullet
  above states the rule; this is the test that applies it when the hand on the keyboard is an agent. An agent may
  prepare such an ack only when every one of these holds, and one failure means stop and report rather than ack:
  1. **A specific, dated, quotable owner utterance already in the record** — not an inference, not a stated
     default, not a prior agent's recommendation, not an assent to a bundle that happened to contain it.
  2. **It covers exactly the bytes that move.** Nothing is acked by generalising from it.
  3. **The file moves *because* of the change he asked for, and the tree reds if it does not move.** The ack is
     forced by his ruling, not chosen; an ack that the change could have avoided is one that should have been.
  4. **The trailer quotes the utterance verbatim, with a dereferenceable id, and names every moving path.**
  5. **The ack is declared, never silent.** The agent's report says that it acked, which finding, why, and hands
     the ratification up. A silent ack is a violation even when the other four hold.
- **The hard negative, whatever the agent believes the owner said.** An agent never acks `keel-file`, `cap-raised`,
  `ceiling-raised`, `ci-loosened`, `sealed-reach` or `owner-reach`, and never adds a key to `scripts/keel-signers`:
  those change what the guard itself can see, or what Theseus may reach, and only the owner's own hand moves them,
  unless he says in words that an agent may do that one thing, and the commit quotes it (2026-11-14: "an agent may
  sign this one", for the two-keys change's acknowledgement; "Yes that sounds good", for naming his key). Since the
  two classes, the guard holds the agents' key to `test-removed` whatever an agent does; where a branch is judged by a
  base whose list is older (one key, the agents', listed for everything), the bound is this sentence alone.
- **The base's guard judges.** The gate runs the base's copy of the guard, never the branch's.
- **The door.** A branch's `gate.sh` could drop the keel phase and then judge itself with its weakened copy. So when a
  tree's `gate.sh` differs from `main`'s, `main`'s copy runs first. A caller that trusts nothing in the tree (a join,
  and Theseus's own self-join) starts `main`'s copy itself:

  ```bash
  blob=$(git rev-parse origin/main:scripts/gate.sh) && git cat-file blob "$blob" >"/tmp/gate-$blob.sh" &&
    THESEUS_GATE_ROOT="$PWD" bash "/tmp/gate-$blob.sh"
  ```

The rest of the hard keel is not a gate phase, but it holds the same way. Each of these needs the owner's yes:

- a one-way store format bump;
- the sealed holdout sets;
- security, policy, secret, permission and IAM changes;
- spend limits;
- resuming self-improvement after its kill switch.

## Shape: files and functions

- **Functions.** clippy's `too_many_lines` (over 100 lines) and `cognitive_complexity` (over 25) are on for the whole
  workspace.
  - Older offenders carry `#[expect(..., reason = "...")]`, so the list can only shrink. A function that drops under
    the limit leaves its `expect` unfulfilled, and that fails the gate until the attribute goes.
  - A new function over the limit is split; it never gets a new `expect`.
  - `scripts/shape-expect.py` regenerates the marks after a threshold change, or after a conflict in them. Never
    resolve those attributes by hand.
- **Files.** No Rust file may exceed 2,500 lines unless `scripts/long-files.txt` lists it with a ceiling and a reason.
  - A listed file past its ceiling fails.
  - A listed file back under 2,500 lines fails until its entry is removed.
  - Raising a ceiling is a keel finding.
- **Ceilings collide at a join.** Two branches can each fit under a file's ceiling and still exceed it once merged.
  Check the merged lengths before the gate. The fix is a split, not a raised ceiling. Near a ceiling, add new code in a
  new module and only a `mod` line to the long file.

## Review

Every branch is reviewed before it joins, by a session other than the one that built it, and preferably on a
different model. Theseus's own branches must be reviewed on a different model.

A review:

- reads the diff whole, against the task and what it was meant to do;
- reruns the row's own tests, and the touched crates' suites, on the branch merged onto today's `main`;
- reruns the planted reverts and records each as caught or missed (Theseus's self-join does this in code, before any
  reviewer is paid);
- checks the report's claims: a report's "environmental" failure has hidden a real one before;
- reads the keel guard's findings and names the exact trailers needed, or "none";
- checks the semantic conflicts listed under [Joins](#joins);
- checks the docs the step owes (Part III, `status.md`, the nearest `AGENTS.md`) and the public-repo rules (no names,
  no private paths, no secrets);
- writes a verdict first: `ACCEPT`, `ACCEPT WITH A JOIN FIX` (with the fix's text), or `REJECT`. The findings follow,
  and the live check to run after the install.

A finding is fixed before the join, or recorded as an issue.

## Joins

A join takes reviewed work onto `main`. Only one join runs at a time, under a join lock. Several reviewed branches
may join together as a batch, landed with one gate.

1. **Dry run.** `git merge-tree --write-tree origin/main <branch>` (git 2.38 or newer) shows the conflicts without
   touching a tree. Chain later merges onto the result with `git commit-tree` to dry-run a whole batch.
2. **Merge, plainly**, in a review tree: `git merge --no-ff <branch>`.
   - **Never rebase, cherry-pick, or re-sign the branch's commits.** A rebase rewrites the reviewed commits, so what
     joins would not be what was reviewed and gated, and the branch would no longer show as merged.
   - Resolve conflicts in the merge commit. Remove the cloud row's `CLOUD_TASK.md` and `CLOUD_REPORT.md` there too.
   - Anything more the join needs, such as a semantic fix the merge did not show, is a separate signed commit after it:
     `join fix: <what>`.
3. **Regenerate, never hand-merge**: `Cargo.lock`, the generated TypeScript, and the shape marks.
4. **Check what a clean merge can still break.** A textually clean merge has more than once quietly undone another
   branch's change. Warm the build first (build, test build, clippy), run each row's own tests on the merged tree, and
   read the merged diff for:
   - a struct one side gave a field and the other built literally (add the field, or `..Default::default()`);
   - an enum one side gave a variant, and a `match` on it the other side wrote (choose the arm on purpose, and test
     it);
   - two sides adding a method of the same name;
   - counts both sides bumped: array lengths, registries, pack counts;
   - caps, limits and budgets that one side set and the other side's older copy dropped (the keel guard is the
     backstop for this, not the method);
   - two branches each under a long-file ceiling and together over it, or two functions that each grew a little and
     together crossed 100 lines;
   - a store format number both sides claimed;
   - a test one side moved and the other changed.
5. **Land and gate.** Take the join lock, fast-forward `main` to the reviewed merge chain, and run `main`'s gate on it
   through the door, with the benches.
   - Every route that lands on `main` recounts `STATS.md` (the landing tool, the self loop's join); the gate refuses a
     tree on `main` whose `STATS.md` line counts are stale (`scripts/stats.py --check`).
   - A red gate is never rerun until it happens to pass.
   - If the red is in the branch, reset `main` (it was never pushed), fix the branch, and take the lock again.
   - If the red is a race in a test no branch touched, fix the race in its own commit, then land and gate the two
     together.
   - **A landing's verdict comes from a run of the full set, on the exact tree that lands.** A gate that ran under a
     default-filter, or with tests withheld, is evidence for a branch and never the verdict for a landing: a test the
     filter left out can be red on `main` while that gate says `ok`, and one did, for hours. The gate says which set
     it ran: `gate: suite set: FULL, …` or `gate: suite set: PARTIAL, run under …`, under its table and again in the
     suite's own output. Where a landing races two hosts, it waits for the host that runs the full set and never
     takes whichever returns first. `acceptance_journey`'s main test is in the default set of this repository.
6. **Push before you release.** Push `main` (never forced) and confirm `origin/main` is the gated commit. Only then
   release the lock. Releasing first lets the next join's merge reach `origin` ungated. If `main` moved before the
   push, redo the dry run, the merge and the gate.
7. **Clean up.** Archive each joined branch's head (report commit included) to `refs/archive/<branch>`, then delete the
   branch, remote and local, together with its worktree and build directory.
8. **Record.** Install ([Installing](#installing)) and write the docs commit for the step.

## Installing

The owner runs Theseus as a systemd user service. A joined build is installed promptly: the owner has given a standing
yes for installs.

1. **Build** the joined commit, from a clean `main` equal to `origin/main`:
   - Build the cockpit first (`npm ci && npm run build` in `cockpit/`). Its output is embedded when present and is not
     committed. After a red cockpit join, rebuild it, because an untracked build survives a reset.
   - Then run `scripts/build.sh --profile release`. It builds with `--locked`, fixed embedded paths, and
     `SOURCE_DATE_EPOCH` from the commit, so two builds of one commit are the same bytes (`scripts/repro.sh` proves
     it).
   - It also checks size: a binary that grew more than 1 MB fails, unless the owner accepted the growth
     (`THESEUS_SIZE_ACCEPT_GROWTH=1`, with the reason recorded).
2. **Back up** the store, the bindings, the unit, the config and the installed binaries. A newer build may move the
   store's format forward, and an older build refuses a newer store: a rollback is a restore.
3. **Copy, then rename**, each installed binary (`build.sh --installed` prints them; `theseus-sim` stays in `target/`):

   ```bash
   for b in $(scripts/build.sh --installed); do
     cp "target/release/$b" "$HOME/.local/bin/.$b.new" && mv -f "$HOME/.local/bin/.$b.new" "$HOME/.local/bin/$b"
   done
   ```

4. **Check the config with the new binary** before the restart: `theseusd --config <file> check` must exit 0. The
   loader rejects unknown keys, so a renamed key fails loudly here instead of being ignored. Fix the operator's file
   in the same step, then check again.
5. **Restart** when no L1 job is running (`scripts/user-service.sh restart`). A running daemon survives the swap, but
   it runs the old build until restarted. The index tender keeps its old image until it restarts too.
6. **Verify**:
   - `theseus health`: secrets ready, the bindings live, the cgroup delegated, the index caught up, and the
     self-improvement line as expected;
   - the journal since the restart has no errors or panics;
   - the installed binaries match the build (`cmp`);
   - `theseus ledger verify` exits 0;
   - `theseus census --place <cli|task|dm> --exact` is within the token budgets;
   - one smoke turn answers;
   - the step's own live check passes against the installed daemon.

`scripts/setup.sh` does all of this on a new machine, from a checkout to a running service ([setup.md](setup.md)).

**Why `release`, and why glibc.** The install is fat LTO with one codegen unit: the smallest binary, which also stays
the least resident, and 8 to 10 % quicker at CPU-bound work. The cost is one slow link per install; a quick build for a
bench or a try-out keeps `release-thin` ([below](#version-cuts-and-releases)). The install is the host's glibc
build, not static musl: the gate, the review's live check and the benches all run glibc builds, so what is installed
is what was tested. The measurements behind both choices are in `scripts/RELEASES.md` ("Releases": the profile pair
under `build.sh`, and "glibc or static musl").

## Version cuts and releases

- **A version cut** freezes features at a named commit and runs the benchmarks on exactly that commit. It then builds
  the `release` profile (fat LTO, one codegen unit) of that commit, tags it, keeps the binaries as a versioned
  artefact, and installs them.
- **Installs and tagged releases are always `release`; `release-thin` is the quick build** (benches, the AWS hand, a
  try-out), never an install.
- **Names.** The first release is **v1**. The fix releases after it are **v1 patch1**, **v1 patch2**, and so on.
- **The freeze holds for what is measured.** The benchmarks build the freeze commit, and a version's benchmark reports
  name it. Fixes may join between the freeze and the cut; the tag names the cut commit, and its message names both.
- **Tags** are signed and annotated, and name the version: `release-1.0.0` for v1, then `release-1.0.1` for v1 patch1,
  `release-1.0.2` for v1 patch2. GitHub lists them under Tags. The workspace version moves with them. The versioned
  binaries are kept with the commit and their sha256 checksums.
- **The status page names the release before the tag.** The cut commit's `docs/status.md` line names the workspace
  version and the newest tag as they are when it is written. Before tagging, `python3 scripts/status-page.py --tag
  release-<version>` refuses a tag name that is not the tree's workspace version. After a tag, the `status page` phase
  fails every gate whose page does not name it, and every gate at all if a tag's tree holds another version.
- **Run `scripts/repro.sh`** before a release, after a toolchain bump, and after a dependency gains a build script.
- **The cut's docs**: the spec's version line, `status.md`, the README's numbers from the cut's benchmark reports, and
  the omnibus of benchmarks.

## Budgets the code is held to

| Budget | Value | Held by |
|---|---|---|
| Start to answering | 50 ms | the lifecycle bench, in the join's gate |
| Clean shutdown | 100 ms | the lifecycle bench |
| Crash and restart | 150 ms | the lifecycle bench |
| Binary swap | 200 ms | the lifecycle bench |
| An L1 job's start, p95 | 25 ms | the jobs bench |
| A plain turn | 4 frames | `tests_m3::a_plain_turn_stays_within_its_frame_budget` and the turn bench |
| First request: a CLI or task session | 9,500 tokens | `tests_first_request` and the gate |
| First request: the owner's DM | 15,000 tokens | `tests_first_request` and the gate |
| First request: a shared channel | 9,700 tokens | `tests_first_request` and the gate |
| First request: headless | 10,100 tokens | `tests_first_request` and the gate |
| Binary growth per build | 1 MB | the size check |

Raising any of these is `budget-raised`: keel.

- **FAST is a primary goal.** A missed lifecycle budget fails the gate just as a failed test does.
  - Serve first: nothing before the socket answers waits on the network, a model, an index rebuild, or work that
    grows with history.
  - New startup work lands with its bench row.
- **Tokens.** The first request's size is the census's own number (`theseus census`), as the provider bills it.
  - Tools are deferred by default (their schemas load when used).
  - A tool is always loaded only when the ledger shows it used in about one session in five of that kind of place.
    It is then named in `first_request::ALWAYS_LOADED` with its reason.
- **Prompting is minimal.** Use the levers in this order:
  1. the provider API's own contract;
  2. the tool's shape;
  3. its description;
  4. its result's metadata;
  5. only then one factual prompt sentence, if a benchmark arm earns it.

  Never examples, persuasion, or "you MUST".
- **Money is a gate.** Every call reserves its worst case before it runs, and every loop has its own day cap. A spend
  limit is not a budget: a limit too close to one call's reservation lets nothing run.

## The store's format

The store has one format number, `MANIFEST_FORMAT`. A step that adds a field to a stored record (nested ones
included), or changes the frame or record encoding:

- **bumps the number in the same commit**, with:
  - a reader for the old layout;
  - a sample of the old layout's bytes, as written by its build, as a literal in theseus-core's `tests_layouts`.
- **Gets its number when it lands on `main`**, not on its branch: two branches would otherwise claim the same number.
  A branch says in its report that it adds a field, and the join assigns the number.
- **Changes the goldens that print the format** with it. Check that each changed line differs only by the number.
- **Is one way for any store it opens.** A build refuses a store newer than it knows, and an older store moves forward
  at its first write. So a format bump is part of the hard keel, and a rollback across one is a restore of the
  pre-install backup.

A new record kind needs no bump. An A/B benchmark across a format bump runs each arm on a store written by its own
build.

## Configuration changes

- **The config is the operator's**, and it holds only what differs from the defaults. A new key or default reaches the
  operator with the build, and nothing rewrites their file.
- **Every new key has a safe default**, documented, commented out, in the template
  (`crates/theseus-core/config/theseus.example.toml`). `example_template_uncommented_still_parses` holds the template
  to the loader.
- **Right over compatible.** A renamed key is renamed everywhere, with no alias. The old key fails loudly at load
  ("renamed to …"), and the operator's file is fixed at install. Stored data stays readable, so the store's format
  rule still applies.
- **Secrets are references**, `op://…` in a vault (or `env:`/`file:` where there is none), never values. Nothing prints,
  logs or stores a secret's value.
- **A key that sets a spend limit, a security posture or a policy is the owner's to set.** A build may add such a key
  with its default, but changing its value on the owner's machine needs the owner's yes.

## Self-improvement

Since v1, Theseus improves itself in one loop, run by its own daemon. The model does not change; the harness around it
does. The design is [`design/rsi-plan.md`](design/rsi-plan.md); the rules are summarised here.

**A new version of a judgment pack goes live on landing**, unless the owner asks for a side-by-side run (the owner,
2026-11-14 13:56 MST, Discord message 1571267707125506159: "ship it as a new version, but put it into action
immediately; only very rarely do we want live a/b tests"). The new version is named in `judge::WIRED` as `Live` and in
its root's constant (`capture::PACK` for `directive.v2`, `CLASSIFY_PACK`); the old one stays embedded, its judgments
under its own text; there is no shadow period and no ladder wait.

### What it does by itself, then tells the owner

- **Every night it files its own backlog.** It reads its ledger: failed calls and turns, provider errors, invalid tool
  input, the owner's routing corrections, wrong labels, turns past their budgets, and spend lines reached. It also
  reads its health and the judge's report.
  - Each problem becomes a backlog item with its evidence, under a stable cause.
  - The backlog is ranked without a model call.
  - A cause the owner dropped is never filed again.
  - A tool's own refusal (an edit whose text is not in the file) is no cause, but a tool whose refusals are many
    and far above its usual share is: one item, with the numbers, while none of its cause is open or newly filed.
- **It writes its own exam items.** Corrections, wrong labels, failed tasks and slow turns become candidate items, half
  of them sealed.
- **It mines its own habits.** Tool-call sequences that recur across sessions and days become tools. Each one is the
  lightest kind that works, in this order:
  1. a flow over existing tools;
  2. a sandboxed MCP extension with no network or secret;
  3. Rust, which goes through a self branch and a join.

  Each mint is checked by replay on held-out instances and by an A/B against the model doing the work by hand. It is
  kept or retired by its use.
- **It builds the top items** as `self/<date>-<slug>` branches.
  - Each has its own worktree and build directory.
  - Its gate is `main`'s, run through the door, below the sandbox floor.
  - It pushes only to `self/*`, and only after a scrub of every added line and commit message.
- **It reviews and joins them.** Each branch goes through, in order:
  1. a planted revert, run by code: if no test fails with the fix undone, the branch is rejected. A test that
     fails is the proof. A build that only breaks (a test that names what the fix added) is accepted as proof but marked
     `build-broke` in the verdict's record. For now the review checks such a proof by reading: an experiment (breaking the fix's
     behaviour in a copy and seeing a test fail) would be stronger, but a reviewer's commands run outside the sandbox and
     a reviewer cannot build inside it, so it is told not to build or run the change's code, and to name, in a line that
     begins `CHECKED BY READING:`, a new or changed test and the assertion that would fail if the fix were wrong, or to
     reject. The row's `proof_check` is `reading`; a review that says nothing of how does not count (a reject).
     The experiment is switched on in code (`REVIEWER_CAN_RUN_THE_EXPERIMENT`) when a reviewer can do it in the
     sandbox. Any other end of a run (on either run) stops it and neither proves nor rejects;
     the same stop on the same head three times in a row holds it for the owner, as a review that keeps failing is;
  2. a review on a different model from the builder's;
  3. the keel check;
  4. a check against the sealed holdout sets, when the change touches behaviour (prompts, tool descriptions,
     routing, ranking, context assembly, the turn loop's stop rules);
  5. the join lock;
  6. a plain, signed `--no-ff` merge;
  7. `main`'s gate with the benches;
  8. the push, never forced.

  Before step 1, a branch that no longer merges into `main`'s tip (it moved while the change waited) is not reviewed: it goes
  back to its builder with `main` merged into a working copy of it, to resolve what clashes, and is checked again. That is no
  try; a third conflict of one item fails it.

  **Libraries** ([`design/self-dependencies.md`](design/self-dependencies.md)): the builder's own commands may reach the
  cargo and npm registries (a closed list of three hosts, `rsi/deps/hosts.rs`) and keep what they download in a package
  cache of their own, which no gate reads. A gate has no network. Before the gate of a branch whose lock files differ
  from main's, the daemon fetches exactly the packages those files name, from the registries, each checked against its
  lock file's checksum, and the gate reads them from there. A dependency from any other source (a git URL, another
  registry, a path outside the tree, a `[patch]` or `[source]`) is refused, with the reason. A change of dependencies
  stops at the owner's card (the keel list below).
- **It installs the join** and tells the owner what changed, why, the numbers, the cost, and the one-command revert.
  A regression within a day of a self-join reverts it.
  - It installs only where it can write the directory the service runs from, which it reads from the unit
    (`rsi/join/layout.rs`). The unit leaves a marker naming the join, and the new daemon checks its own commit against
    it: `self.install.verified`, or `self.install.mismatch` and a note to the owner.
  - Where the service runs a root-owned build, it copies nothing and restarts nothing, records
    `self.install.not_installed`, and tells the owner once that the change is joined and not installed, with the one
    step ([setup.md](setup.md#a-root-owned-install)). `self pipeline`, `self log` and `health` never call such a build
    done.

### What it never does without the owner's yes (the hard keel)

- delete or loosen a test, budget or ceiling;
- read a sealed holdout set: it gets a score, never an item;
- bump the store format;
- change security, policy, secrets, permissions or IAM;
- raise a spend limit;
- add, remove or change a library: any package a lock file (`Cargo.lock`, `cockpit/package-lock.json`) names, shown on
  the card with its old and new version and where it comes from (`rsi/join/dependencies.rs`);
- resume after the kill switch.

A branch that touches any of these stops at a card to the owner, and is never retried at that head. The rest of the
queue goes on.

### The brakes

- `[self] mode = "off"`, the default, runs nothing self-directed.
- `theseus self halt` stops everything at once and may come from anyone. Only the owner's `theseus self resume`
  releases it, and a store that has never seen a resume is halted.
- `[self.budget]` sets a spend line per branch and per day, for all self-improvement together. Raising either is keel.
- A capacity guard: a build starts only when there is a free slot, free disk, low memory pressure, and an unsaturated host (the
  five-minute load average under two runnable tasks a core; CPU *pressure* is shown, not held on: its `some` line
  reads busy on a host with idle cores, and the system-wide `full` line is always zero). A hold on the slots names
  the trees that hold them.
- A cap on tries: an item has three (`rsi/backlog/attempts.rs`: a red gate, a rejecting review, or a build that died
  with no gate; a gate that never judged the code is none, and an aborted gate, exit 75, is run again twice at most, free, and
  one that still aborts is a red of its item, one try and no part of the kill switch's count; a red suite whose only failures are tests outside what the change reaches has those run once more, and is green
  when they pass). The try that reaches it fails the item with every reason
  and one notice, and it waits for `theseus self retry`. A red inside twelve hours of another for the item is not posted
  again; the note lists it.
- No build starts on a red main: the newest recorded whole gate of the commit main is at failed (the gate-times log). A
  later green gate of that commit lifts it, and so does main moving on. No record means nothing is known.
- `theseus self set-aside ITEM` stops one item and forbids nothing, where `drop` forbids its cause for good;
  `theseus self bring-back ITEM` puts it back. Both are the owner's.
- `theseus self revert` undoes a self-join (the gate runs on the revert) and drops its item for good.
- Everything is visible: `theseus self log`, `self pipeline`, `self backlog` and `self branches`, a morning notice,
  and a weekly digest.

## Branch hygiene

The public repository shows `main` and the branches in flight, and nothing else.

- A branch is deleted, remote and local, when it joins. Its head is kept first under `refs/archive/<branch>`, and its
  worktree and build directory go with it.
- A branch that will not join (superseded, abandoned, or rejected twice) is archived the same way and deleted, with a
  note on its issue saying why.
- Lanes are `lane/*`, cloud rows `cloud/*`, Theseus's own branches `self/*`.
- Nothing is ever force-pushed to `main`.

## Benchmarks

Benchmarks measure Theseus against the other agent harnesses, and against itself. How to run them is in
[`bench/README.md`](../bench/README.md); every run's report is in [`benchmarks/`](benchmarks/README.md).

- **Every run gets a report**, whatever its size, including the runs that disappoint. A run is not finished until its
  report has joined.
  - The report is named `<date>-<suite>[-<slug>].md`, with its data (`.json`, `.csv`) and figures beside it.
  - It has ten sections: the answer first, a table at a glance, the question, the setup, results, analysis, threats to
    validity, what it cost, reproduction, and data.
  - Rates carry Wilson 95 % intervals; paired comparisons use exact McNemar tests.
- **Improve, never max.** After each run, ask what it says about real use, and fix that. A change that helps only a
  benchmark is a loss even when the score moves: no task-specific prompt, no special case for a harness, no tuning on
  held-out tasks.
- **Harness against harness.** Theseus, routing its own models, runs against each rival harness on that harness's best
  model: broad first, then the top three against Theseus in depth. Each tier sets one effort level for every harness.
- **Every report that routes models carries a model-selection section**: the share of calls and spend by model and by
  model origin, and the judge's verdicts on what data each model may see.
- **Frozen builds.** A run names the commit it measured. A version's benchmarks run on its freeze commit.
- **Spend is capped** per run and per host before the run starts. Benchmark hosts stop themselves at a fixed deadline,
  and an idle-stop acts only on a host that runs no benchmark. Data transfer costs money too, so watch it like
  compute.
- **The gate's timing benches** (lifecycle, jobs, turn) are a different thing: budgets, not comparisons. Their history
  is kept. An A/B of two builds runs interleaved, inside one hold of the gate lock, so both arms see the same machine,
  and each arm uses its own bench driver.

## The docs that move with the code

- **The spec** (`docs/spec/`) is the source of truth. Each step adds its Part III item: what it built, how it was
  proven, where it diverged from the plan, and what it left open. The version line moves with each item.
- **`status.md`** says what works today and what is next. Its "Updated" line moves with every step.
- **The README stays stable**: what Theseus is and why. What changes goes in `status.md`.
- **`AGENTS.md` files** carry what a step taught that the next agent needs. Put it in the nearest directory's file, in
  the same commit or in the step's docs commit. Every path and command an `AGENTS.md` names must exist.
- **Times come from the clock** (`date`), never from a guess.
- **The repository is public.** No person's name (write "the owner"), no private paths, no hostnames or account ids,
  and no secrets or vault names. A scrub check runs over the whole of `README.md` and `docs/` before docs land, not
  only over the diff.

## Commits

- **Signed** (`git commit -S`), one per green sub-step, each made after the gate.
- **Subject:** `area: what changed (<issue>)`, with the area in lower case (`kernel`, `store`, `cli`, `docs`, …). A
  join's fix is `join fix: … (<issue>)`.
- **Body:** what changed and why, in plain words.
- **A trailer names the agent** that wrote it, so the history says who built what.
- **Keel trailers** only on the owner's quoted yes.
- **Prose comes from a file** (`git commit -F`), never from a heredoc in a command line.

## Models and roles

- **Coding** is done by a strong, fast coding model: Sonnet-class, even for hard problems.
- **Design and review** use the strongest reasoning models (Opus- or Fable-class).
- **A reviewer never runs on the same model as the author** of Theseus's own branches.
- **Model origin is a data rule.** Private, personal or confidential content goes only to models whose origin the
  owner allows. Theseus judges this by the content, counts by the model's origin (not its host), and fails closed.
