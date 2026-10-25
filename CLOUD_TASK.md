<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3g-soft-retire`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

## Read this first: the state of your base (it wins over anything above about main, the base, or what may be edited)

Your branch starts from `main` at `0fda4d57`. That is install #83's gated tree: release 1.0.2 plus the joined patch3
work, including **`theseus-odj0m`, the Operations page's one work set**, which this row is written on top of. Fetch it
and branch from `0fda4d57`. Where the release preamble above disagrees with this note about the base, the ceilings,
what may be edited or the work in flight, **this note wins**.

**This is a cockpit-only row. No Rust file changes.** You still run the Rust gate once at the end, as below.

**Early push (required, before any code).** Do setup first, then `npm ci` in `cockpit/` and one `npm test` to see the
suite green at the base. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-99l9z: session started, setup done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3g-soft-retire`

After that, push after EVERY commit, not only at the end. On 2026-10-24 three rows of sixteen ended holding nothing but
their task commit, and because every other row pushed only at the very end there was no way to tell a session that died
at minute 5 from one that worked for two hours and lost it. A run that ends early must leave its work on the branch.

**Store format: untouched.** This row changes no stored record and no wire type. Do not bump `MANIFEST_FORMAT`, and do
not regenerate `cockpit/src/protocol.gen/`.

**Line ceilings.** `scripts/long-files.txt` lists Rust files only; no TypeScript file has a ceiling. Never raise one
anyway.

## What odj0m already landed — read this before you write a line

Read `cockpit/src/ops/state.ts` and `cockpit/src/ops/board.ts` first:

- **`boardRows()` (`ops/board.ts` ~:110) is the one work set**: every card once, each with its one `bucket` and
  whether it `counted`. `RowKind` (~:20) is
  `'task' | 'conversation' | 'job' | 'wake' | 'routine' | 'watch' | 'step' | 'ask'`.
- **The buckets** (`state.ts`) are `needs | working | scheduled | paused | todo | rest | ended`. **This matters to
  you more than to any other row in the lane**: your visibility classes must be read off `row.bucket`, never off a
  list of your own. `isEnded` and `FADED` are there already.
- **`OWNER_CALLS`** (`state.ts` ~:31) is the page's one setting, and every field is already `true`: the owner answered
  all fifteen design questions on 2026-10-24 with the verbatim words **"All recs!"**.
- `cockpit/test/ops-counts-parity.test.ts` exists and must stay green.

## Your lane: `integ/p3-ops2`, and you join LAST of the Graph rows

Four other rows run beside you off this same base, all firing concurrently. **Your branch will be merged after
`theseus-hl0r1`'s**, because it owns the Graph's toolbar and forms in this phase and you add to them. Write your
change so that merge is a plain one:

- **`theseus-0zraw` (`-p3g-explainers-words`) joins FIRST** and owns `lib/explainers.ts`, `test/explainers.test.ts`,
  the `components/TaskGraph.tsx` **heading**, and the one `ops/CommandBar.tsx` help-button line. **Do not touch
  `explainers.ts`, `test/explainers.test.ts` or `CommandBar.tsx`, and add no help button or explainer entry** —
  `explainers.test.ts` fails an entry with no panel *and* a panel with no entry, so either half alone turns a branch
  red. You may change `components/TaskGraph.tsx`'s **body** (see task 3); leave its `title=` and its
  `help="actions.taskgraph"` exactly as they are at your base.
- **`theseus-hl0r1` (`-p3g-graph-forms`) joins immediately before you** and owns `ops/GraphNode.tsx`,
  `ops/GraphLens.tsx`, `ops/graph.ts`, `ops/BoardLens.tsx`. You must add a checkbox group to the Graph lens's toolbar
  and a visibility filter to the layout, which means you touch `GraphLens.tsx` and `graph.ts` too. **Keep every hunk
  small, additive and at the edge of what it changes**, put all new logic in a new module (`ops/retire.ts` is a good
  name) that both the lens and the layout call, and name every hunk with its line range in the report. Do not
  restructure either file.
- `theseus-gnhnk` (`-p3g-card-selection`) owns `ops/Inspector.tsx`, `ops/InspectorSections.tsx`, `ops/sections.ts`,
  `ops/regions.ts` and three hunks of `views/Ops.tsx` (~:183-191, ~:197, ~:491).
- `theseus-brcs4` (`-p3g-threads-lens`) owns `ops/ThreadsLens.tsx` and `lib/threadboard.ts`, and has been told to hide
  nothing of its own, because that choice is yours.

**Your write scope:** a new `cockpit/src/ops/retire.ts`, small additive hunks of `cockpit/src/ops/GraphLens.tsx` and
`cockpit/src/ops/graph.ts`, `cockpit/src/views/Fleet.tsx`, `cockpit/src/components/TaskGraph.tsx` (body only),
`cockpit/src/lib/taskgraph.ts`, `cockpit/src/lib/fullgraph.ts` if the shared helper needs it, and your tests
(`cockpit/test/retire.test.ts` new; `taskgraph.test.ts`, `fullgraph.test.ts`, `ops-graph.test.ts` where an expectation
must move). Touch no other file. If you believe you must, write `OWNER CALL:` in the report and leave it undone.

---

## Your task: p3g-soft-retire, old nodes retire softly and come back on a checkbox (theseus-99l9z)

Branch: `cloud/20261025-p3g-soft-retire`.

Every commit subject carries `theseus-99l9z`. Deadline for the report: about 3 hours after you start.

**Why — the owner's own words, 2026-10-24:**

> Fleet/graph should probably also soft retire old nodes, as it's very busy, but have a checkbox option to include
> ended/retired/stale etc

and two minutes later, about the Actions page:

> the same for the Actions page's Task graph panel

### 1. The default view, and the checkbox group

**The default is live and recent only:** running, needs you, waiting with work, plus anything touched in the last
**N days, default 7**. Ended, retired and stale are hidden.

**A checkbox group brings each class back,** one checkbox per class: **ended**, **retired**, **stale**, **idle**, and
**at rest**. Each class is read off the one work set:

- **ended** is `row.bucket === 'ended'` — never a list of state strings of your own. `state.ts` already has `isEnded`.
- **at rest** is `row.bucket === 'rest'`: a conversation between exchanges, neither open nor ended. The owner's line
  on it, 2026-10-24, verbatim: *"resting is not done -- i mean, stale and done is def differnt"*. It is its own class
  in your group and **is not folded into ended**.
- **retired** is the session's own `retired` flag, which `Fleet.tsx` already draws (~:151, `<StateBadge state={s.state}
  retired={s.retired} />`).
- **stale** and **idle** are time-based: nothing touched within the cutoff, with `idle` the open-but-quiet case and
  `stale` the one past the cutoff. Define each in `retire.ts`, in one place, and say in the report exactly where you
  drew the line between them.

**The choice persists per view** — in the URL **and** in local storage, so a reload and a shared link both keep it.
Use the existing URL-parameter and storage conventions of the page rather than inventing new ones.

**A count chip says how many are hidden**: "+214 older". **Nothing is deleted** — that is the cockpit's standing rule,
no data lost. A hidden node is hidden, never dropped from the set, and the counters do not change because of it: a
counter counts the work set, not what is drawn. **Prove that with a test**, because it is the easy thing to get wrong.

**The same rule for the Fleet view and for the Ops Graph lens**, from the one module.

### 2. Do NOT change `COLUMN_OF.rest`

A separate row, **`theseus-zo72s`**, moves a resting conversation out of the Done column and out DONE TODAY, and that
change carries three `ops-board.test.ts` expectations and a `Keel:` trailer quoting the owner. **It is not yours.**
Your job is the *flag surface*: the `rest` class in the checkbox group, reading `row.bucket === 'rest'`, so that when
zo72s joins the two fit together with no further work. Leave `board.ts`, `state.ts` and `COLUMN_OF` untouched, and say
in the report that you did.

### 3. The Actions page's Task graph panel — and its bug

The same hide-ended-by-default rule and the same checkbox group apply to the Actions page's panel of stored records,
which is mostly terminal-state records today.

**And there is a real bug there.** `cockpit/src/components/TaskGraph.tsx` ~:27-30: the panel's "open as a graph" button
only toggles the `?taskgraph=1` search parameter; `TaskGraphView` renders in a **separate panel further down the page**
(`views/Actions.tsx` ~:81). So nothing visible happens where the owner clicked. **Fix it:** open the graph **in place**
— replace the tree in the same panel — or go fullscreen through the shared `useFullGraph` helper (`lib/fullgraph.ts`,
used by `Fleet.tsx` ~:56 and ~:175), and scroll it into view. Choose one, say which and why, and keep the URL
parameter working as a deep link either way.

**Leave the panel's `title=` and `help="actions.taskgraph"` alone** — 0zraw is rewording the heading, and a second
edit there would be a needless conflict.

---

## Tests (each red at the base, with a planted revert)

The suite is `npm test` in `cockpit/` (node's own runner over `test/*.test.ts`). Red-first means: the new assertion
fails at `0fda4d57`, or under a planted revert of your change, and passes after. **A test that would pass before the
change is not a test.** Put the classifying and the filtering in `retire.ts` as pure functions and test them as data.

New `cockpit/test/retire.test.ts`, over a fixture with running, needs-you and waiting-with-work cards, a conversation
at rest, an ended task, a retired session, a step touched 2 days ago, one touched 30 days ago, and a terminal-state
record backlog:

1. **The default hides exactly the right classes:** ended, retired and stale are out; running, needs you, waiting with
   work and anything touched inside the cutoff are in. Plant P1: include ended by default — red.
2. **Each checkbox brings back exactly its class, and only it**, with every combination of the five checked. In
   particular **`rest` is not `ended`**: checking "ended" does not bring back a resting conversation, and checking
   "at rest" does not bring back an ended one. Plant P2: fold `rest` into `ended` — red.
3. **The classes read `row.bucket`, not a list of state strings.** Assert against the buckets, and add a test that a
   new state string reaching `bucketOfCard` as `ended` is hidden without `retire.ts` being changed. Plant P3: replace
   the bucket read with a hard-coded list — red.
4. **Nothing is deleted and the counters do not move:** with the default hiding on, `countersOf` over the same rows
   gives exactly what it gave with everything shown, and the hidden count chip's number equals the number hidden.
   Plant P4: filter the rows before the counters see them — red.
5. **The choice persists:** written to the URL and to local storage, and read back from either; a reload restores it
   and so does a shared link. Plant P5: drop the storage write — red.
6. **The Actions panel's button:** clicking "open as a graph" puts the graph where the click was (in place or
   fullscreen, whichever you built) and the `?taskgraph=1` deep link still works. Plant P6: restore the base's
   parameter-only toggle — red, and the assertion must say that nothing rendered in that panel, since that is the bug.
- Keep green: `ops-graph.test.ts`, `ops-graph-state.test.ts`, `ops-board.test.ts`, `ops-counts-parity.test.ts`,
  `taskgraph.test.ts`, `fullgraph.test.ts`, `ops-page.test.ts` and the rest of the suite; `npm run lint`;
  `npx tsc -b`.
- **Where an existing expectation must change because the default view changed**, change the assertion rather than
  delete it, put that change in a commit of its own whose message says why, and list it under **Keel findings
  expected**. Never lower an assertion count.

## Not in this row

- **No `ops/board.ts`, `ops/state.ts` or `COLUMN_OF`** — odj0m's and zo72s's.
- **No `lib/explainers.ts`, `test/explainers.test.ts`, `ops/CommandBar.tsx`, no help button, and no change to
  `components/TaskGraph.tsx`'s `title=` or `help=`** — 0zraw's.
- **No restructuring of `ops/GraphLens.tsx` or `ops/graph.ts`** — hl0r1 owns their shape; you add at their edges only.
- No `ops/GraphNode.tsx` or `ops/BoardLens.tsx` (hl0r1's); no `ops/Inspector.tsx`, `ops/InspectorSections.tsx`,
  `ops/sections.ts`, `ops/regions.ts` or `views/Ops.tsx` (gnhnk's); no `ops/ThreadsLens.tsx` or `lib/threadboard.ts`
  (brcs4's).
- No protocol, no Rust, no wire change, no `protocol.gen/` regeneration.
- No benchmark, no model call, no key, no spend, no AWS.

## Rules for this row

- **The Discord DM task flow is LOCKED.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If some part
  seems to need a Discord word changed, STOP that part and write `OWNER CALL:`.
- **Keel:** remove, rename or weaken no test; lower no assertion count; add no `#[allow]` or skip; raise no budget, cap
  or ceiling; do not edit `scripts/keel-guard.py` or its tests. **You cannot sign, so you cannot ack** — any
  expectation you must move goes in its own commit and under **Keel findings expected**. Before the report run
  `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` and paste its output.
- **No ceiling raised and no cap loosened.** The Graph's `NODE_CAP` stays 400; hiding nodes must not be used to raise
  what the cap means.
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner". Fixtures use invented
  titles under `example.test`.
- **Red first; right over compatible.**
- **The gate** (read the root `AGENTS.md` and `cockpit/AGENTS.md` "Building and checking" first). For a cockpit-only
  change: `npm ci`, `npm test`, `npm run lint` and `npx tsc -b` in `cockpit/`, each with its counts; then
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` once before your last commit. **The gate's cockpit step
  hides its own errors when `node_modules` is cold**: "cockpit ok" with no test count is not evidence, so paste the
  number of tests that ran, from `npm test` itself.

## Your report

Commit `CLOUD_REPORT.md` at the repository root as the branch's last commit, subject `cloud report (not for main)`, and
push. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp filesystem has committed an
empty report here before), and check after the commit that it is not empty. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (untouched), any ceiling, OWNER CALL or STOP. (2) **The five
classes**, each with its exact rule and where it reads `row.bucket`, and **where you drew the line between idle and
stale**. (3) **The default view and the cutoff:** N days, where its default lives, and how a person changes it.
(4) **Persistence:** the URL parameter and the storage key, and what wins when they disagree. (5) **Nothing deleted:**
the counters' proof and the hidden-count chip. (6) **The Actions panel:** in place or fullscreen, why, and the bug's
before and after. (7) **Tests** and plants P1-P6, red at the base and green after, with the `npm test` test count
before and after. (8) **Gate** commands and results, with counts. (9) **Keel** output and any **Keel findings
expected**. (10) **DM golden** diff result. (11) **Join notes:** every file touched with its hunks' line ranges, and
**in particular every line you added to `GraphLens.tsx` and `graph.ts`, since hl0r1 joins immediately before you** —
the integrator must be able to see each is additive. State explicitly that you left `board.ts`, `state.ts`,
`COLUMN_OF`, `explainers.ts`, `CommandBar.tsx` and `TaskGraph.tsx`'s heading alone. (12) **Follow-ups**, including
what `theseus-zo72s` still owes once your flag surface is in. (13) One line on the data floor: nothing new is sent to a
model or routed anywhere.
