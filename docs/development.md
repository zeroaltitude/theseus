# How Theseus is developed

Theseus is built by AI agents in small, reviewed steps. The rules on this page apply to whoever does the work: an agent
session on the owner's machine, a cloud coding session, or, since v1, Theseus itself. They cover where work happens,
what proves it, how it reaches `main`, how it is installed and released, how it is measured, and what Theseus may
change about itself without asking.

Three other documents go deeper, and this page links to them rather than repeating them:

- the root [`AGENTS.md`](../AGENTS.md): the principles that bind code, the crate map, and the generated files;
- [`scripts/AGENTS.md`](../scripts/AGENTS.md): the gate's mechanics, the build script, and the release profiles;
- [`design/rsi-plan.md`](design/rsi-plan.md): the self-improvement design.

## Contents

1. [The shape of the work](#the-shape-of-the-work)
2. [Tracking: Beads](#tracking-beads)
3. [Where work happens](#where-work-happens)
4. [Proof: what every change carries](#proof-what-every-change-carries)
5. [The gate](#the-gate)
6. [The keel](#the-keel)
7. [Shape: files and functions](#shape-files-and-functions)
8. [Review](#review)
9. [Joins](#joins)
10. [Installing](#installing)
11. [Version cuts and releases](#version-cuts-and-releases)
12. [Budgets the code is held to](#budgets-the-code-is-held-to)
13. [The store's format](#the-stores-format)
14. [Configuration changes](#configuration-changes)
15. [Self-improvement](#self-improvement)
16. [Branch hygiene](#branch-hygiene)
17. [Benchmarks](#benchmarks)
18. [The docs that move with the code](#the-docs-that-move-with-the-code)
19. [Commits](#commits)
20. [Models and roles](#models-and-roles)

## The shape of the work

```text
 issue ──► branch ──► gate ──► review ──► join ──► main's gate ──► push ──► install ──► docs
 (Beads)    │          (no      │          (plain    (with the       (never
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

## Tracking: Beads

The work is tracked in Beads (`bd`), with ids `theseus-<id>`. Its database is in the main checkout's `.beads/`, and
`bd` works from any worktree.

- **Every issue has an owner from the moment it is created.** An unowned issue invites two agents to start the same
  work.
- **Claim before you work**, with `bd update <id> --claim`. It is a compare-and-set: exit 0 means the issue is yours.
  A nonzero exit means someone else holds it, so stop and do nothing for it.
- **Read the notes first** (`bd show <id>`). The title is not the state: an issue can read `open` and already be done.
- **Append to notes, never replace them**: `bd update <id> --append-notes "<text>"`. The `--notes` flag overwrites.
- **Check the branch namespace too** before starting. A branch that already names the id means the work has begun.
- **Ids everywhere.** Commits, briefs, Part III items and reports name their issue by id, so every artefact leads back
  to its issue.
- **Keep the status true**: `in_progress` while executing, `waiting_for_user` while the owner holds the issue, and
  closed when it is done.

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

A lane pushes only its own branch. The machine has room for only a few compiling trees at once. Each tree is many
gigabytes, and the whole machine slows past three concurrent compiles. So a build starts only when a capacity check
says there is room, and idle lanes are closed rather than kept.

### Cloud rows

A cloud row is one piece of work done by a cloud coding session on its own branch, `cloud/<date>-<slug>`, cut from a
named commit of `main`.

- **The task is the branch's first commit**, `CLOUD_TASK.md`. It holds:
  - the issue and the files in play;
  - the acceptance tests and the planted reverts to make;
  - where `main` is: its commit, store format, and the long-file ceilings at that commit;
  - the sibling rows of the same batch, and which files each one owns;
  - the shared rules every row follows: tests first, nothing lost, no new always-loaded tool text, no store format
    change, no new dependency, no person's name, and never loosening a test, budget, cap or ceiling.
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
| `features` | no | The shipped binaries, built alone, get the same features as in the workspace |
| `clippy` | no | `-D warnings` over the workspace, which also holds the function budget |
| cockpit | no | `npm run lint`, `npm test`, `npm run build` |
| `test build` | no | Builds everything the suite and the benches run, once |
| reader rule | **yes** | Nothing is declared without its reader |
| suite | **yes** | `cargo nextest run --workspace` |
| protocol types | **yes** | `cockpit/src/protocol.gen/` matches the Rust types |
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

Moved and renamed tests are matched by name, then by body, then by similarity, so a refactor is not an erosion.

- **The ack.** A finding passes only when a commit in the range carries a trailer
  `Keel: <rule> <path>[, <path>…] — <why>`, and that commit is signed with a GPG key listed in `scripts/keel-signers`.
  - The list is read as it is at the base, so a branch cannot list its own key.
  - An unsigned trailer, or one signed by any other key, counts for nothing.
  - `python3 scripts/keel-guard.py --base <main> --head HEAD --quiet` prints the exact lines to fill in.
- **Only on the owner's explicit yes.** An agent writes a `Keel:` trailer only after the owner has said yes to that
  specific erosion, and quotes the yes in the trailer's reason.
  - A vendored crate's `allow` is still `allow-added`.
  - Any edit to the guard is still `keel-file`, even one that tightens it.
  - Without a yes, avoid the finding: restore the test rather than ask for an ack to remove it.
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

- reads the diff whole, against the task and the issue's acceptance;
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

A finding is either fixed before the join, or filed as its own issue with an owner.

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
     `join fix: <what> (<issue>)`.
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
   - A red gate is never rerun until it happens to pass.
   - If the red is in the branch, reset `main` (it was never pushed), fix the branch, and take the lock again.
   - If the red is a race in a test no branch touched, fix the race in its own commit, then land and gate the two
     together.
6. **Push before you release.** Push `main` (never forced) and confirm `origin/main` is the gated commit. Only then
   release the lock. Releasing first lets the next join's merge reach `origin` ungated. If `main` moved before the
   push, redo the dry run, the merge and the gate.
7. **Clean up.** Archive each joined branch's head (report commit included) to `refs/archive/<branch>`, then delete the
   branch, remote and local, together with its worktree and build directory.
8. **Record.** Close the issue with the merge commit and the gate named, then install
   ([Installing](#installing)) and write the docs commit for the step.

## Installing

The owner runs Theseus as a systemd user service. A joined build is installed promptly: the owner has given a standing
yes for installs.

1. **Build** the joined commit, from a clean `main` equal to `origin/main`:
   - Build the cockpit first (`npm ci && npm run build` in `cockpit/`). Its output is embedded when present and is not
     committed. After a red cockpit join, rebuild it, because an untracked build survives a reset.
   - Then run `scripts/build.sh --profile release-thin`. It builds with `--locked`, fixed embedded paths, and
     `SOURCE_DATE_EPOCH` from the commit, so two builds of one commit are the same bytes (`scripts/repro.sh` proves
     it).
   - It also checks size: a binary that grew more than 1 MB fails, unless the owner accepted the growth
     (`THESEUS_SIZE_ACCEPT_GROWTH=1`, with the reason recorded).
2. **Back up** the store, the bindings, the unit, the config and the installed binaries. A newer build may move the
   store's format forward, and an older build refuses a newer store: a rollback is a restore.
3. **Copy, then rename**, each installed binary (`build.sh --installed` prints them; `theseus-sim` stays in `target/`):

   ```bash
   for b in $(scripts/build.sh --installed); do
     cp "target/release-thin/$b" "$HOME/.local/bin/.$b.new" && mv -f "$HOME/.local/bin/.$b.new" "$HOME/.local/bin/$b"
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

**Why `release-thin`, and why glibc.** The daily install is thin LTO, which rebuilds incrementally in a fraction of
fat LTO's time. A tagged release is fat LTO ([below](#version-cuts-and-releases)). The install is the host's glibc
build, not static musl: the gate, the review's live check and the benches all run glibc builds, so what is installed
is what was tested. The measurements behind both choices are in `scripts/AGENTS.md`.

## Version cuts and releases

- **A version cut** freezes features at a named commit and runs the benchmarks on exactly that commit. It then builds
  the `release` profile (fat LTO, one codegen unit) of that commit, tags it, keeps the binaries as a versioned
  artefact, and installs them.
- **Daily installs are always `release-thin`; a tagged release is always `release`.**
- **Names.** The first release is **v1**. The fix releases after it are **v1 patch1**, **v1 patch2**, and so on.
- **The freeze holds for what is measured.** The benchmarks build the freeze commit, and a version's benchmark reports
  name it. Fixes may join between the freeze and the cut; the tag names the cut commit, and its message names both.
- **Tags** are signed and annotated: `v1`, then `v1-patch1`, `v1-patch2`. The workspace version moves with them:
  1.0.0 for v1, 1.0.1 for v1 patch1. The versioned binaries are kept with the commit and their sha256 checksums.
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
| First request: a CLI or task session | 8,000 tokens | `tests_first_request` and the gate |
| First request: the owner's DM | 15,000 tokens | `tests_first_request` and the gate |
| First request: a shared channel | 7,900 tokens | `tests_first_request` and the gate |
| First request: headless | 8,600 tokens | `tests_first_request` and the gate |
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

### What it does by itself, then tells the owner

- **Every night it files its own backlog.** It reads its ledger: failed calls and turns, provider errors, invalid tool
  input, the owner's routing corrections, wrong labels, turns past their budgets, and spend lines reached. It also
  reads its health and the judge's report.
  - Each problem becomes a backlog item with its evidence, under a stable cause.
  - The backlog is ranked without a model call.
  - A cause the owner dropped is never filed again.
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
  1. a planted revert, run by code: if no test fails with the fix undone, the branch is rejected;
  2. a review on a different model from the builder's;
  3. the keel check;
  4. a check against the sealed holdout sets, when the change touches behaviour (prompts, tool descriptions,
     routing, ranking, context assembly, the turn loop's stop rules);
  5. the join lock;
  6. a plain, signed `--no-ff` merge;
  7. `main`'s gate with the benches;
  8. the push, never forced.
- **It installs the join** and tells the owner what changed, why, the numbers, the cost, and the one-command revert.
  A regression within a day of a self-join reverts it.

### What it never does without the owner's yes (the hard keel)

- delete or loosen a test, budget or ceiling;
- read a sealed holdout set: it gets a score, never an item;
- bump the store format;
- change security, policy, secrets, permissions or IAM;
- raise a spend limit;
- resume after the kill switch.

A branch that touches any of these stops at a card to the owner, and is never retried at that head. The rest of the
queue goes on.

### The brakes

- `[self] mode = "off"`, the default, runs nothing self-directed.
- `theseus self halt` stops everything at once and may come from anyone. Only the owner's `theseus self resume`
  releases it, and a store that has never seen a resume is halted.
- `[self.budget]` sets a spend line per branch and per day, for all self-improvement together. Raising either is keel.
- A capacity guard: a build starts only when there is a free slot, free disk, and low memory and CPU pressure.
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
