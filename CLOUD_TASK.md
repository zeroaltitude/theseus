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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3g-explainers-words`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
work, including **`theseus-odj0m`, the Operations page's one work set**. Fetch it and branch from `0fda4d57`. Where the
release preamble above disagrees with this note about the base, the ceilings, what may be edited or the work in flight,
**this note wins**.

**This is a cockpit-only row. No Rust file changes.** You still run the Rust gate once at the end, as below.

**Early push (required, before any code).** Do setup first, then `npm ci` in `cockpit/` and one `npm test` to see the
suite green at the base. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-0zraw: session started, setup done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3g-explainers-words`

After that, push after EVERY commit, not only at the end. On 2026-10-24 three rows of sixteen ended holding nothing but
their task commit, and because every other row pushed only at the very end there was no way to tell a session that died
at minute 5 from one that worked for two hours and lost it. A run that ends early must leave its work on the branch.

**Store format: untouched.** This row changes no stored record. Do not bump `MANIFEST_FORMAT`.

**Line ceilings.** `scripts/long-files.txt` lists Rust files only, and no TypeScript file has a ceiling. Never raise
one anyway. `cockpit/src/lib/explainers.ts` is 587 lines at your base; it may grow.

**What odj0m already landed, and you must not redo.** Read `cockpit/src/ops/state.ts` first:
- `OWNER_CALLS` (`ops/state.ts`, ~:31) is the page's one setting for the owner's six calls, and **every field is
  already `true`** — the owner answered all fifteen design questions on 2026-10-24 with the verbatim words
  **"All recs!"**. `RECOMMENDED` beside it is the same object.
- **Q8 is already done in the code:** `counterLabel` (`ops/board.ts`, ~:421-426) returns `'scheduled today'` when
  `calls.scheduledLabel`, and `'still today'` only under the old setting. The `still` key stays in the URL.
  **Your Q8 work is the words of the explainers**, which still describe the counters and the Board in the old nouns.
  Do not touch `board.ts`, `state.ts` or `CommandBar.tsx`'s label logic.

**Your lane: `integ/p3-ops2`.** Four other rows run beside you off this same base. **You join FIRST in the lane**, and
that ordering exists because of this row:

- `theseus-hl0r1` (`cloud/20261025-p3g-graph-forms`) builds the Graph's and Board's visual forms. It owns
  `ops/GraphNode.tsx`, `ops/GraphLens.tsx`, `ops/graph.ts`, `ops/BoardLens.tsx`. **It adds no explainer and mounts no
  help button** — that is yours, which is why you join before it.
- `theseus-gnhnk` (`-p3g-card-selection`) owns `ops/Inspector.tsx`, `ops/InspectorSections.tsx`, `ops/sections.ts`,
  `ops/regions.ts` and three small hunks of `views/Ops.tsx` (~:183-191, ~:197, ~:491).
- `theseus-brcs4` (`-p3g-threads-lens`) owns `ops/ThreadsLens.tsx` and `lib/threadboard.ts`.
- `theseus-99l9z` (`-p3g-soft-retire`) joins after hl0r1 and touches the Graph lens toolbar and `graph.ts`.

**Your write scope, and nothing else:**
- `cockpit/src/lib/explainers.ts`
- `cockpit/test/explainers.test.ts` (only if you add a check; keep every existing one)
- `cockpit/src/ops/CommandBar.tsx` — **one line only**, the `ops.graph` help button (see below)
- `cockpit/src/components/TaskGraph.tsx` — **the panel heading only** (its `title=` text)
- a new `cockpit/test/opswords.test.ts` if you add one

Touch no other file. If you believe you must, write `OWNER CALL:` in the report and leave it undone.

---

## Your task: p3g-explainers-words, plain words in the cockpit's explainers (theseus-0zraw)

Branch: `cloud/20261025-p3g-explainers-words`.

Every commit subject carries `theseus-0zraw`. Deadline for the report: about 3 hours after you start.

**Why.** The owner read the Judgment page's LADDER "?" panel and said, verbatim:

> "pack" is a custom term here that doesn't mean anything to a human -- please update this description

`cockpit/src/lib/explainers.ts` is the one place the "?" words live, keyed by each panel's `help` id. It is full of
internal jargon a person has never met: **pack** (12 uses, around :326-352 and :484), **rung**, **shadow**, **mint**,
**lens**, **place**, **frame**. Separately, the Operations page's nouns were settled by a design the owner approved,
and the explainers still use the old ones ("task record", "task graph", "plan item").

### 1. The glossary — the words every explainer must now use

These six are the owner's own nouns. One line each, in the words a person reads:

| noun | one line |
|---|---|
| **conversation** | Where you talk with Theseus in one place (a DM, the CLI, a channel, the cockpit). It takes one turn at a time. |
| **thread** | A named line of work with its own conversation. On the Graph it is a swimlane, and everything it started hangs in it. |
| **plan** | A piece of work broken into parts. It holds steps and smaller plans, shows *n of m* done, and folds open and shut. |
| **step** | One piece of work, checked off when done. When something is listed under it, it becomes a plan: same card, same history. |
| **task** | An agent Theseus starts to carry out a step or a plan on its own, with its own budget and a fresh context. It reports back to where it came from. |
| **job** | A command run for a turn or a task, such as a build or a test run. |

Five more the UI needs, agreed in the same design:

| noun | one line |
|---|---|
| **turn** | One run of the model in a conversation or a task, from a message to its reply. |
| **ask** | Anything waiting on your answer: an approval, a question, a budget, a failure to look at, a change to a step's goal. |
| **wake** | A time when Theseus starts work by itself: a reminder, a routine's run, a watch, a deferred start. |
| **project** | A long-running plan with dated milestones and a weekly review. |
| **Home** | The lane for work that belongs to no thread: the owner's DM, the CLI and the cockpit's conversations, and what they started. |

**The state words, one per bucket** (they are `ops/state.ts`'s buckets, and the explainers must match them exactly):
*needs you*, *working*, *to do*, *paused·queued*, *scheduled today*, *at rest*, *done*.

**Two hard rules from the design:**
- **"task" means only the running agent.** The owner-facing words never say "task" for a stored record again. A record
  is a **plan** or a **step**. "Task record", "task graph" and "plan item" disappear from owner-facing text.
- **Wire and storage names stay** and are never renamed by this row: `task.record`, `TaskRecord`, `task.list`,
  `task.changed`, `task.split`, `task_create`, `task_close`, `WorkKind::Step`, `PlanItem`. Where a screen still shows
  an internal name, keep it **in parentheses after the plain word**, never instead of it.

### 2. Rewrite these entries to the glossary's nouns

`ops.board`, `ops.queue`, `ops.inspector`, `tasks.new`, `actions.tasks`, `actions.taskgraph`, `threads.list`,
`threads.work`, `fleet.graph`. Each keeps its three parts (goal, how to read it, why it matters) and its own voice;
only the nouns and any stale state word change. Specifically:

- `ops.board`: its five columns must read in the new state words, and the Scheduled column's words must agree with the
  counter's, which is now **"scheduled today"** (Q8). Today's text says a card is "a task, a conversation, a job or a
  wake" — it is now also a **plan** and a **step**.
- `actions.taskgraph`: it is the Actions page's panel of stored records. Its words become **plans and steps**, and the
  panel's own heading in `components/TaskGraph.tsx` becomes **"Plans and steps"** (keep its `· N open of M` counts and
  its `help="actions.taskgraph"` exactly as they are; change the words only).
- `actions.tasks`: it describes records, so it is plans and steps too.
- `threads.work`, `threads.list`, `ops.queue`, `ops.inspector`, `tasks.new`, `fleet.graph`: the same sweep.

### 3. Add `ops.graph`, with its button

The Graph lens has no "?" today. Add the entry **and** mount its button, both in this row:

- the entry `'ops.graph'` in `EXPLAINERS`, saying what the Graph is (the lanes, the cards, the forms — a thread is a
  swimlane with its head card; a plan is a card with a fold and *n of m*; a step is a card with a checkbox glyph; a
  task is a badge strip on the step or plan it carries out; a job is a pill), how to read it, and why it matters;
- the button, as **one line** in `cockpit/src/ops/CommandBar.tsx` beside the Board's, which today reads
  `{lens === 'board' && <HelpButton id="ops.board" title="Board" />}` at ~:76. Add the graph lens's twin there.

**This pair is not optional and must land together**, because `cockpit/test/explainers.test.ts` enforces both
directions: a panel whose `help` id has no entry fails, *and* an entry no panel or `HelpButton` uses fails. An entry
alone would be red on your own branch; a button alone would be red on hl0r1's. That is exactly why you join first.

### 4. Add `ops.words`, a short glossary entry

One entry whose three parts give the six nouns above in a person's words, so the page can point at one place. It needs a
mount like every other entry — put its `<HelpButton id="ops.words" …/>` beside the `ops.graph` one in `CommandBar.tsx`
(that is your second and last line there), or fold it into the Graph's button if you judge two buttons to be clutter —
**if you fold it, do not add the entry at all**, and say so in the report. Do not leave an entry without a mount.

### 5. The pack / rung / shadow / mint / lens / place / frame sweep

The remaining jargon, everywhere in `explainers.ts`:

- **pack** → the thing is one of **Jev's checks** (a named, versioned judgment the loop runs). Use "check" or
  "judgment set"; keep `loop.v1`, `classify.v4` and the like as the examples, since those names are on the screen.
- **rung** → **level**. A promotion moves a check **one level up**.
- **shadow** → keep the word only where the badge says it, and gloss it in place: *"shadow = Jev answers but nothing
  acts on it"*; **live** = Theseus acts on the answer.
- **mint** → say what it is in plain words (the newly made thing the act created), with the internal word in
  parentheses only if the screen shows it.
- **lens**, **place**, **frame** → these are real screen words in some panels and jargon in others. Where the screen
  shows the word, keep it and gloss it once; where it does not, replace it.

**The owner wrote the replacement for `judgment.ladder` himself.** Use it as given, and let it set the voice for the
rest of the sweep:

- Goal: `See how far each of Jev's checks is trusted (watching only, or acting), and move it up or back.`
- How to read it: `One row per check (e.g. loop.v1 decides when to nudge an unfinished turn; classify.v4 sorts each message). Its badge says its level: shadow = Jev answers but nothing acts on it; live = Theseus acts on the answer. The rules under it are its safety limits (e.g. how many labels a day). Promote moves it one level up; Roll back moves it down at once.`
- Why it matters: `A check only starts steering Theseus once it has proved itself in shadow. If one starts making bad calls, roll it back immediately; its earlier level is restored.`

Then sweep the same way through `judgment.health`, `judgment.list`, `judgment.versions`, the learning report entry, the
ontology undo entry (`:484`) and any other entry that still says "pack" or "rung".

**Keep a one-line glossary where a term must stay.** Some internal words are printed on the screen itself; those keep
the word and gain one short gloss in place. Say in the report which terms you kept and why.

---

## Tests (each red at the base, with a planted revert)

The suite is `npm test` in `cockpit/` (node's own runner over `test/*.test.ts`). Red-first means: the new assertion
fails at `0fda4d57`, or under a planted revert of your change, and passes after. **A test that would pass before the
change is not a test.**

1. **A words test, new, `cockpit/test/opswords.test.ts`.** It reads `EXPLAINERS` as data and asserts:
   - **no owner-facing banned word**: none of the three parts of any entry contains `pack`, `rung`, `task record`,
     `task graph` or `plan item` as a whole word, case-insensitively — except on an allow-list you write in the test
     file, each entry of which carries a one-line reason (the screen shows that word). Keep the allow-list short and
     name every member in the report.
   - **the state words**: every state word an entry uses is one of the seven above; "still today" appears nowhere.
   - `ops.graph` and (if you kept it) `ops.words` exist and say the glossary's nouns.
   - Plant P1: put `pack` back into `judgment.ladder.read` — red.
2. **`explainers.test.ts` stays green, every existing test of it unchanged.** In particular "every panel's help id has
   an explainer" and "no explainer is left without a panel" must both pass with your new entries and their mounts; say
   in the report that you ran it and what it printed.
3. **The TaskGraph heading.** A small test (in `opswords.test.ts` is fine) that reads
   `cockpit/src/components/TaskGraph.tsx` as text and asserts its panel title says "Plans and steps" and not "Task
   graph". Plant P2: restore the old heading — red.
- Keep green: the whole `npm test` suite, and `npm run lint` (oxlint) and `npx tsc -b`.

## Not in this row

- **No change to `ops/board.ts`, `ops/state.ts`, `ops/graph.ts`, `ops/GraphLens.tsx`, `ops/GraphNode.tsx`,
  `ops/BoardLens.tsx`, `ops/ThreadsLens.tsx`, `ops/Inspector.tsx` or `views/Ops.tsx`.** Other rows own them.
- No change to `CommandBar.tsx` beyond the one (or two) help-button lines.
- No CLI, TUI, Rust or docs change: `docs/glossary.md` and `crates/theseus/src/render/task_graph.rs` belong to a
  separate row. Where a doc should change, say what and where in the report.
- No benchmark, no model call, no key, no spend, no AWS.

## Rules for this row

- **The Discord DM task flow is LOCKED.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. Nothing in
  this row should go near it; if some part of it seems to need a Discord word changed, STOP that part and write
  `OWNER CALL: <what and why>` in the report.
- **Keel:** remove, rename or weaken no test; lower no assertion count; add no `#[allow]` or skip; raise no budget, cap
  or ceiling; do not edit `scripts/keel-guard.py` or its tests. You cannot sign, so you cannot ack. Before the report
  run `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` and paste its output; it should be empty. If
  it is not, list each finding under **Keel findings expected** with the rule, the path and why.
- **No ceiling raised and no cap loosened.** If you think one must move, say so in the report instead.
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner". Fixtures use invented
  subjects under `example.test`.
- **Red first; right over compatible.**
- **The gate** (read the root `AGENTS.md` and `cockpit/AGENTS.md` "Building and checking" first). For a
  cockpit-only change: `npm ci`, `npm test`, `npm run lint` and `npx tsc -b` in `cockpit/`, each with its counts;
  then `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` once before your last commit. **The gate's cockpit
  step hides its own errors when `node_modules` is cold**: "cockpit ok" with no test count is not evidence, so paste
  the number of tests that ran, from `npm test` itself.

## Your report

Commit `CLOUD_REPORT.md` at the repository root as the branch's last commit, subject `cloud report (not for main)`, and
push. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp filesystem has committed an
empty report here before), and check after the commit that it is not empty. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (untouched), any ceiling, OWNER CALL or STOP, and whether you
kept `ops.words`. (2) **The glossary sweep:** every entry you rewrote, with its id and one line on what changed.
(3) **The jargon sweep:** each banned word, how many uses it had at the base and how many remain, and the allow-list
with a reason per member. (4) **`ops.graph` and its button:** the entry and the exact `CommandBar.tsx` line, with its
line number. (5) **Tests** and plants P1-P2, red at the base and green after, with the `npm test` test count before and
after. (6) **Gate** commands and results, with counts. (7) **Keel** output. (8) **DM golden** diff result. (9) **Join
notes:** every file touched with its hunks' line ranges, and in particular the one or two `CommandBar.tsx` lines and the
`TaskGraph.tsx` heading, so the integrator can see at a glance that you overlap no sibling row. (10) **Follow-ups**,
including what the CLI/TUI/docs row still owes. (11) One line on the data floor: nothing new is sent to a model or
routed anywhere.
