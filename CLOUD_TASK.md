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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-private-authored`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

This task was written against an older `main`. Your branch starts from the integration branch `integ/cn` at `0bf2dfee` (2026-10-21; integ/cn after its round 3: the open request, cn-tool-screen, cn-observability, cn-bg-rest), not from `main`; that tip is "the integ/cn tip named in the note". Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,405 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- Google part 2 rows: gw-mail, gw-calendar, gw-drive, gw-sheets-slides (`crates/theseus-google/`, each its own
  service module, `fake/<service>.rs`, one registration line each).
- Harness rows: job-truths (job command lines, `theseus-kernel/src/job.rs`, fs/file read results), effort-on-evidence
  (turn checkpoints' effort), turn-end follow-ups and long blind waits, recall abstention and the `service` orphan.
- Threads rows: routing (`crates/theseus-core/src/thread_route.rs`, working sets, claims) and cross-stream reach
  (`theseus-memory/src/threads.rs`, `thread_recall.rs`, a place digest, Jev's near-miss check, thread links).
- Ontology follow-ups (`crates/theseus-ontology/`, the sweep, the dream's guidance, the replay), test-infrastructure
  rows (Discord task-thread tests, leaking shell loops), the China-origin model rows (`data_gate.rs`, session data),
  rough edges in progress words, detours and `theseus why`.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: cn-private-authored, what a Google-private conversation wrote for other conversations is left out of every request to a China-origin model (theseus-lt4xh)

Branch: `cloud/20261021-cn-private-authored`. Every commit's subject carries `theseus-lt4xh`. Deadline for the report: 4
hours after you start.

## Your base: `integ/cn` after its round 3 (read this before anything else)
Your branch starts from the integration branch `integ/cn` at the tip named in the note above, **not** from `main`.
Where the note says your base is `main`, this section is right. `integ/cn` is `main` plus the China-origin rows, each
its own merge. You build on:
- the restricted-origin floor (theseus-a1wvq.3): `data_gate.rs` wraps every provider; a request to a model of
  restricted origin (`cn` or `unknown`, `Config::profile_origin(..).restricted()`) is refused unless its
  `ProviderRequest.data_pass` covers its digest. `turn/data_step.rs`: `restricted_profile`, `data_floor`,
  `private_session`, `DEFAULT_PROFILE`.
- the open view (.7, `open_view.rs`): a request a restricted model may receive is always compiled in the open view
  (`t.tc.open` is `Some`). It carries no task view and no project slice (`turn/compile_step.rs` `tail_blocks`, the two
  `(!open).then(..)` calls), only the standing **orders** (never preferences or facts, by the shared content class),
  the commitments block (pending wakes and routines) and the persona line.
- the open request (.8): an open session's turns (`open_of`) and an open detour (`compile_open_detour` in
  `turn/open_step.rs`, through `TurnRunner::open_request` in `open_view.rs`) are the only requests that get a pass.
- the Google private mark (`google/private.rs`): a session that read a Google result holds a META mark
  (`meta_key(session)`); `session`, `session_on`, `any_node` (a node the store cannot read counts as private),
  `recalled_from`, `followed`, `inherited`. A marked session's own requests never reach a restricted model
  (`private_session` feeds `RouteState.sensitive`). A task a private session opens inherits the mark. The mark follows
  recalled nodes (lane `cn-private-follow`).
- round 3: the tool screen (.9), observability (.11) and the background rest (.10: compaction's own pass, consolidation
  falls back, keep-warm skips and `turn.replay` refuses a restricted target).

**Check first:**
- `git log --oneline | grep -E 'theseus-a1wvq\.(8|9|10|11)|theseus-39dj0'` shows each of those commits;
- `grep -n 'pub fn any_node\|pub fn session_on\|pub fn meta_key' crates/theseus-core/src/google/private.rs` finds the
  three;
- `grep -n '(!open)' crates/theseus-core/src/turn/compile_step.rs` finds the task view's and the slice's gates;
- `grep -n 'fn standing_block\|fn standing_blocks\|fn stand' crates/theseus-core/src/turn/standing_step.rs` finds them.

If `.8`'s commit, `any_node` or the two `(!open)` gates are missing, stop: write that in `CLOUD_REPORT.md`, push it,
and end. If the join renamed any of them, use the code's names and say which in the report's first section.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48). **This row bumps
nothing.** No stored record gains a field: a standing record already names where it came from (`source`), and a
compilation's manifest already lists a record it did not show (`StandingUsed { shown: false }`).

## Why
A conversation that read a Google result is private: its own requests go only to models of US origin. But some of
what it writes is shown to **other** conversations, and those carry it with no mark. A review of the mark's follow-up
found four callers that attach such text to every request of their scope (`task_graph/view.rs` `attach`,
`project/slice.rs` `attach`, `commitments.rs` `attach`, `standing.rs` `Block::attach_here`, each through
`task_graph::view::attach_text`), plus the standing block in the system (`Block::apply`).

What reaches a China-origin model today, concretely:
- **A kept standing order.** The owner says, in a DM that read a Gmail thread, "from now on, always cc the Heronsgate
  survey lead". `capture/act.rs` keeps it as a `StandingRecord` with `source: "capture:<node>"`, scope everywhere.
  Every conversation's system carries it, including an open session's request to `dsflash` and an open detour's (the
  open view keeps standing orders). A Google-derived word reaches the restricted model.
- **A surface's record** ("Standing here:", `Block::here`), the same, in the request's tail.
- **A milestone's reminder** (`project/remind.rs`): its note is `⚑ due today: <project title>, milestone <n>:
  <milestone title>`, a wake in the owner's Home or in the conversation that changed the milestone, shown in that
  conversation's commitments block. Any conversation, a private one included, may have written either title, and a
  task record does not say who last edited its title.
- **The task view and the project slice** carry task and project titles written by any session, but the open view
  already leaves both out, so no restricted request carries them today. This row pins that with tests.

The owner's decision (2026-10-21), option (b): text a Google-private session authored into the shared blocks is **left
out of requests to a China-origin (restricted-origin) model**; requests to models of US origin are unchanged; and the
mark does **not** spread to the sessions that see the text. Marking every session that sees a private session's text
would spread the mark across the whole task graph, which the owner did not want.

## Risk
Medium in kind (Google content and a China-origin model), small in size. Every change only removes lines from a
request bound for a restricted model; a request to a model of US origin is byte-identical; nothing new is stored; the
chokepoint is untouched and stays the last check.

## Prerequisite: `integ/cn` after round 3 (not `main`), as "Your base" says.

## Your row

### 1. Which standing records a private session authored (`google/private.rs`)
Add one function beside `any_node`:

```rust
/// The ids of `records` a session holding the mark authored: a record kept
/// from a message (`source` `capture:<node>`) whose node's session holds it.
/// Nothing is read with no `[google]`; a node the store cannot read counts
/// as private (`any_node`).
pub fn authored<'a>(store: &Store, cfg: &crate::Config,
    records: impl IntoIterator<Item = &'a theseus_protocol::StandingRecord>) -> HashSet<String>
```

- `source` is `capture:<node id>` for a kept record (`capture/act.rs` `keep`); any other source is a surface the owner
  typed on (`cli`, `discord`, the cockpit: `rpc/capture.rs`, `rpc/standing.rs`), which is the owner's own words and
  never held. An edit through `rpc/standing.rs` is a new record with a surface source: the owner retyped it.
- Ask `any_node` once per distinct node, not per record.
- With `cfg.google` empty, return an empty set before any read (`any_node` already does; keep the early return
  explicit so the plain path is visibly free).

### 2. A held record is listed and not shown (`standing.rs`)
- The private `render` (the method and the free function) take one more argument, `held: &HashSet<String>`.
  **Filter after `live()`**, never before: `live` drops a record another one ends (an edit's older half), so removing
  a held newer half from `all` would bring its older text back. A held record goes into `used` as
  `StandingUsed { id, shown: false }` and its text is never written; a kind whose records are all held prints no title.
  No line tells the model a record was held.
- `Block::of` and `Block::here` keep their signatures and pass an empty set (their other callers: the census,
  initiative). Add `Block::of_held` and `Block::here_held`, the same with the set.
- The checks `missing` and `missing_here` read only `shown` records, so a held record never trips `standing_missing`.
  Do not change them.

### 3. The turn holds them back for a restricted target (`turn/standing_step.rs`)
- `standing_blocks(session_id, class, surface, persona, restricted: bool)`: when `restricted`, compute the held set
  with `google::private::authored` over the live records of the scope (`standing::live(&all, Some(scope), now)`),
  and call the `_held` pair; otherwise the empty set and no read.
- `stand` passes `self.restricted_profile(&t.target.profile)`. `route_to` (`route_step.rs`) calls `stand` after it
  sets `t.target`, so a routed turn's spec is built for the profile it goes to; a turn whose base is restricted (an
  open session on `dsflash`) is held from its first compile.
- `standing_block(session_id, class)` gains `restricted: bool`. Its callers:
  - `open_view.rs` `open_request` (the open detour): pass
    `self.cfg.profile_origin(&target.profile, &self.catalog).restricted()` (`restricted_profile` is `pub(super)` in
    `turn`; this is the same predicate). `compile_open_detour` then calls `stand` for the check's copy, and both hold
    the same records, so the check passes.
  - `keep_warm.rs` and `prefix/turn.rs` `warm_prefix_tokens`: pass the same predicate for their target (keep-warm skips
    a restricted target since `.10`, so it is false there in practice; the prefix's figure is an estimate).
  Each is a one-argument hunk.

### 4. Milestone reminders stay out (`commitments.rs`, `turn/compile_step.rs`)
- `commitments::attach` gains `restricted: bool`. When it is set, `render_with` leaves out every wake whose `purpose`
  is `Purpose::Remind` and whose note starts with `theseus_protocol::notices::MILESTONE_MARK` (the due and overdue
  reminders both start with it, `project/remind.rs` `note`), before the bound is counted, so the "and N more" line
  never counts them.
- **All of them, not only those a private session wrote.** A task record names who created it (`origin.session`) and
  its own session, but not who last edited its title (`task.update` from any session that sees it), so a reminder's
  titles can never be shown to be free of a private session's words. Fail closed. The same text is the project's,
  which the open view already leaves out as the project slice.
- The conversation's own wakes (`wake.at`, its own turns' words: a marked conversation never reaches a restricted
  model) and its routines (`routine::lines` of what it made) stay. An owner's reminder (`rpc/wake_owner.rs`) is the
  owner's words and stays.
- `tail_blocks` passes `self.restricted_profile(&t.target.profile)`. Detours carry no commitments block.

### 5. No mark on the reading session
Nothing in this row writes a META record or calls `followed`/`inherited`. The reading session's `private_session`
answer is what it was.

### 6. Docs
- `docs/privacy.md`, "Who else sees it", in the paragraph that begins "Google content is held back by more than
  names" (not the China-origin paragraph above it: a sibling row edits that one), add: "What a private conversation
  wrote for other conversations to see is left out of every request to a model of China or unknown origin: a standing
  order Theseus kept from its words, and any milestone reminder, since a milestone's title does not record who wrote
  it. Requests to models of US origin carry them as before, and a conversation is not made private by seeing them."
- `crates/theseus-core/AGENTS.md` has no passage for the Google private mark yet. Add one bullet in the data rule's
  list, directly after the bullet "**A session's data state**": **The Google private mark** (`google/private.rs`):
  the META mark a Google result writes; `scoped`/`bound` carry it to the floor; it follows recalled nodes and a task's
  parent; and text a marked session authored for others (a kept standing record, by its `capture:` source) is held
  back, listed `shown: false`, from a request to a restricted profile (`standing_step.rs`, `authored`), milestone
  reminders with it (`commitments.rs`), while the task view and project slice are already outside the open view. The
  mark never spreads to the readers. Tests: `tests_private_authored.rs`.

## Tests (each with a planted revert; the first is the key test)
Put them in a new `crates/theseus-core/src/tests_private_authored.rs` (one `mod` line in `lib.rs`), except the pure one.
Build the rig by copying the few lines you need, never by editing the sibling files:
- `[google]` configured and a session marked, as `tests_private_recall.rs` and `google/tests_private.rs`
  (`rig_on`, `rig_tweaked`) do; mark a session by writing `google::private::record`'s META record or its `meta_key`;
- a restricted profile and an open session, as `tests_open_request.rs` does: `c.copy_profile("dsflash", "glm")`, of
  restricted origin; `c.routing.classes.trivial.cn_profiles = vec!["dsflash".into()]`; its `open_session` shape
  (`profile.use dsflash` in an open session); **do not** call `serve_glm_unrestricted`;
- a plain record through `Core::standing_add`, as `tests_standing.rs`' `add` does; a kept record as `capture/act.rs`
  `keep` writes one (a `StandingRecord` with `source: format!("capture:{node}")`, `node` a real node of the private
  session). The set is kept in memory (`standing::Board`): read how the board learns a new record and write the kept
  one through that path, so the next compile sees it.

Fixtures are invented ("Heronsgate", "Acme", "Widgetco"); never a real-looking key or name.

1. **`a_private_sessions_kept_order_stays_out_of_a_restricted_request`** (key). Session P is marked and holds a node
   N. Two live orders, scope everywhere: `std_a` "Always cc the Heronsgate survey lead" with source `capture:N`, and
   `std_b` "Answer in short lines" with source `cli`. Session S is plain and open, on `dsflash`, admitted. Assert:
   - the restricted fake's request has "Answer in short lines" and not "Heronsgate", anywhere (system or messages);
   - S's compilation manifest lists `std_a` with `shown: false` and `std_b` shown;
   - no `standing_missing` refusal and no `data_gate.blocked` row; the turn answers;
   - the same S after `profile.use` of a Claude profile: that fake's request has "Heronsgate";
   - afterwards `google::private::session(store, cfg, S)` is false and no META record `meta_key(S)` exists.

   Plant A: pass `false` for `restricted` in `stand`. Plant B: make `authored` return an empty set. Show both failing.
2. **`a_private_surface_record_stays_out_of_the_tail`**. As 1, with `std_a` scoped to the turn's surface: the
   "Standing here:" tail of the restricted request lacks it, `missing_here` does not refuse, a US request has it.
   Plant: drop the set in `standing_blocks`' `here_held` call (pass an empty set). The test fails.
3. **`an_open_detour_holds_back_the_same_records`**. A plain DM session on a Claude profile, admitted open, whose
   trivial message detours to `dsflash` (`compile_open_detour`): the detour's request lacks "Heronsgate", keeps
   `std_b`, and the check passes. Plant: pass `false` in `open_request`'s `standing_block` call. The request carries
   it (or the check refuses); the test fails.
4. **`a_milestone_reminder_stays_out_of_a_restricted_request`**. S open on `dsflash` holds two pending wakes in its
   execution: its own `wake.at` ("check the Widgetco build") and a milestone's due reminder (set with
   `project::remind`'s `due_id` and `note`, or `Kernel::create_wake` with `Purpose::Remind`, `WakeRun::Notice` and a
   note made by `note`). The restricted request's commitments block has the `wake.at` line and no `⚑` line, and its
   count says 1; the same session on a Claude profile has both. Plant: ignore `restricted` in `render_with`. The test
   fails.
5. **`the_task_view_and_slice_never_reach_a_restricted_request`** (a guard of what `.7` already does). P creates a
   task under S's scope titled "Heronsgate tide tables" (`task.create` in P, or the record written as
   `tests_open_task.rs` writes one) and a project S is attached to, with a milestone of the same title. S open on
   `dsflash`: the request has neither `task_graph::view::HEAD` nor `project::slice::HEAD` and no "Heronsgate".
   Plants: drop the `(!open)` gate of the task view in `tail_blocks`, then of the slice; each makes the test fail.
   Restore both.
6. **`a_held_record_is_listed_unshown_and_its_older_half_stays_ended`** (pure, in `standing.rs`'s test module):
   records `std_1` "Reply by noon" and its edit `std_2` "Reply by 3 pm" (`ends: Some("std_1")`). `Block::of_held` with
   `std_2` held: the text has neither line, `used` is `[std_2 shown: false]`, and `std_1` is nowhere. Plant: filter
   `all` before `live()`. "Reply by noon" comes back; the test fails.
7. **`without_google_nothing_is_held`**. No `[google]`, the same records, S open on `dsflash`: the request carries
   both orders and `authored` reads nothing (as `tests_private_recall.rs`'
   `with_no_google_the_recall_reads_no_mark` checks its no-read path). No plant: it pins the rollback.

Unchanged and passing: `tests_standing.rs`, `tests_standing_ring.rs` (`a_detour_carries_the_standing_block`),
`tests_commitments.rs`, `tests_projects.rs`, `tests_private_recall.rs`, `google/tests_private.rs`,
`tests_private_background.rs`, `tests_open_request.rs`, `tests_open_view.rs` (with
`tests/golden/private_requests.txt` byte-identical), `tests_open_task.rs`, `tests_bg_gate.rs`, `tests_tool_screen.rs`,
`tests_data_floor.rs`, `tests_data_gate.rs`, `tests_keep_warm.rs`, `tests_initiative.rs`, `tests_first_request.rs`,
`tests_route.rs`, and `tests_m3::a_plain_turn_stays_within_its_frame_budget`.

## Files (and the rows beside you)
Yours:
- `google/private.rs` (`authored`);
- `standing.rs` (`render`'s argument, `of_held`, `here_held`, the pure test);
- `turn/standing_step.rs` (`standing_blocks`, `stand`, `standing_block`'s argument);
- `open_view.rs`, `keep_warm.rs`, `prefix/turn.rs`: one argument each at their `standing_block` call;
- `commitments.rs` (`attach`/`render_with`'s argument and the filter);
- `turn/compile_step.rs`: one argument in `tail_blocks`' `commitments::attach` call;
- `tests_private_authored.rs` (new), `lib.rs` (one `mod` line), `AGENTS.md`, `docs/privacy.md`.

Beside you, from the same tip: **cn-draft-review** (theseus-a1wvq.13): `config/routing.rs`, `config/routing_cn.rs`,
`draft.rs`, `turn/draft_step.rs`, `turn.rs`, `session.rs`, `check.rs`, `fact/draft.rs`, the protocol's `ledger.rs`,
`open_view.rs`, `tests_draft.rs`, `lib.rs`, a paragraph of `AGENTS.md` beside the data floor's passage. Shared with
you: `open_view.rs` (keep yours to the one argument), `lib.rs` (a `mod` line each) and `AGENTS.md` (different
bullets). After both: **cn-config-guidance** (.12) edits `docs/privacy.md`'s China-origin paragraph, the AGENTS.md
invariants and traps; stay out of both.

Line room: none of your files is in `scripts/long-files.txt`, so each stays at 2,500 lines or fewer (all are under
1,000). List `wc -l` of each in the report.

## Not in this row
- Marking the readers (the owner chose not to), and any change to the mark's existing carriers.
- A per-edit author on a task record (it would be a store format change). Until one exists, milestone reminders are
  held back whole from a restricted request.
- The persona line (configured text, no session's words) and a watch's shown value (its own conversation's watch).
- Background callers to a restricted profile: `.10` falls them back or refuses them, and the chokepoint refuses the
  rest; none of them builds the commitments block, and the census and initiative build standing blocks only for their
  own (US) profiles.
- Leaving private-authored text out of requests to models of US origin: unchanged, by the owner's decision.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing, late or unparsable is closed, never open. A node the store
  cannot read is private; a profile whose origin does not resolve is `unknown`, which is restricted. Never loosen a
  check, a threshold, a test or a ceiling to make a test pass.
- **On, measured, with a named rollback.** The behaviour ships on by default. The rollback is the config: with no
  `[google]` (nothing is read, nothing held) or no restricted profile in any `cn_profiles` and no open task on one,
  none of the new branches runs, and every existing test passes unchanged.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new
  on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's
  turn bench must not move). The store is read only for a request bound for a restricted profile with `[google]` set.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The key test must be shown failing on both
  of its planted reverts.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `git fetch origin integ/cn` and then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet`
  and paste its output in the report. It must be empty. (A `main` base would flag the earlier rows' changes as yours.)
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name, company or private
  project name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture names and paths
  are invented ("Acme", "Widgetco", "Heronsgate").
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, never a shell heredoc, and check
after the commit that it is not empty. Its sections, in order:
1. The names as built: `authored`, `of_held`/`here_held`, `standing_blocks`' and `standing_block`'s new argument,
   `commitments::attach`'s. Name any base name the code had under another name, and the one-argument hunks in
   `open_view.rs` (shared with cn-draft-review), `keep_warm.rs` and `prefix/turn.rs`.
2. What changed, file by file, with each file's line count.
3. Each test and its plant: the line broken, the failure seen, the restore.
4. What could not run on the VM, with the exact commands for the maintainer.
5. The keel guard's output.
6. "store format: unchanged (48)".
7. The maintainer's live check after the join, on a scratch daemon with `[google]` signed in to a test account and
   `trivial.cn_profiles = ["glm"]`: in a DM that read a Google result, say a standing order with an invented word;
   then in a plain conversation send a trivial message that detours to glm. Check that glm's request (the
   `context.compiled` row's digest and the provider's request log) lacks the word, the same conversation on Claude
   carries it, and the plain conversation holds no mark (`theseus why` or the session's META). Name the rows to read.
8. Follow-ups: a per-edit author on task records (format bump) if the owner wants milestone reminders back in
   restricted requests; anything else you found that carries one session's words into another's request.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
