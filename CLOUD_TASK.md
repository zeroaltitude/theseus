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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-detours-why`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `1d092ded` (2026-10-20). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (67 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
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
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,404 / 3,525
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
- On the maintainer's machine: task questions and reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI
  chat (`crates/theseus-tui`), herdr (`contrib/herdr`).

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: detours-why, a detoured turn keeps the project it serves, loads named tools within the whole request's budget, and `theseus why` explains it (theseus-v4u66, theseus-r5ydi, theseus-3bs4e)

Branch: `cloud/20261021-detours-why`. Every commit's subject carries the id of the piece it fixes (`theseus-v4u66`,
`theseus-r5ydi` or `theseus-3bs4e`). Deadline for the report: 5 hours after you start.

**Why.** A turn the router judges trivial or quick **detours**: it runs on a cheaper routed profile (Haiku) with a
small request built outside the session's compilation (`compile_detour` in `crates/theseus-core/src/turn/route_step.rs`).
Three things go wrong on that path, each seen live:
- **A detour forgets the project (theseus-v4u66).** A CLI conversation attached to a project asked "what is the goal
  of the project this conversation serves?". The router said `mode=quick 0.92`, the turn detoured to Haiku, and the
  model answered that no project is attached. The same question pinned to Sonnet quoted the project's slice.
  `compile_detour` clears `context_text`, `persona_text`, `context_files`, `card`, `cold` and `walk`, and never calls
  `crate::project::slice::attach`, which the normal path calls last in `tail_blocks`
  (`crates/theseus-core/src/turn/compile_step.rs`). The slice is at most 300 tokens (`project::slice::MAX_TOKENS`), in
  the request's tail after the cached prefix, and adds nothing when no project is attached.
- **`theseus why` cannot explain a detoured turn (theseus-r5ydi).** `theseus why --session <a session whose one turn
  detoured>` prints `no turn of this session has compiled a context yet`. `context_explain`
  (`crates/theseus-core/src/rpc/context.rs`) finds a turn only through `context.compiled` ledger rows
  (`compiles_of`), and a detour writes none (its compilation is never stored). So "what was the model shown" has no
  answer for every trivial and quick turn, which is the turn a person is most likely to ask about after a short odd
  answer. The words come from `theseus_protocol::why` (`build`, near line 203) and `theseus_protocol::fill`
  (`context_short`, near line 97). `theseus memory recalled SESSION` already shows a detour's recall row (outcome
  `detoured` or `ran`).
- **A named family can push a detour's first request past its place's budget (theseus-3bs4e).** For a model without
  tool search, a hidden tool family the message names loads up front (`tools_defer::want::extra`,
  `crates/theseus-core/src/tools_defer/want.rs`). Its only caller, `defer_tools` in
  `crates/theseus-core/src/turn/defer_step.rs` (near line 149), passes `system = 0`, so the family is counted on the
  tools alone, not on the whole request. The comment there says counting the system too was "a change waiting on the
  owner's word". That word came on 2026-10-20, when the owner asked for every listed fix to be done, this one
  included. The learned loads beside it are already held to the whole request through `Fit`
  (`crates/theseus-core/src/tools_defer/fit.rs`: `budget`, `system`, `held`, `cost` per family).

**What landed already (do not redo):** a quick detour carries the turn's own recall, a trivial one none (theseus-pa1xx,
`eac26b6d` / `d4982998`, join fix `6fc5884b`; theseus-n7nc `62de744f`); the AWS tools are six deferred families so a
detour naming one can reach it within the cli budget (theseus-3msy0, `a29a4ebe`; test
`tests_route_defer::a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget`); a `tools_find` result is
callable in the same turn (theseus-342qy, `05655588`); `want::extra` already takes a `system` argument
(theseus-rwpbg.3, `5d63b5d2`), it is only passed 0.

**Risk: medium.** The detour's request gains up to 300 tokens when a project is attached; a new ledger fact is written
on every detoured loop; the named-family rule gets stricter at the cli.

**Prerequisite: `main`.**

## Your row
| Piece | Beads | Files (as on `main`) |
|---|---|---|
| A | theseus-v4u66 | `crates/theseus-core/src/turn/route_step.rs` (`compile_detour`) |
| B | theseus-r5ydi | `crates/theseus-core/src/turn/route_step.rs` (`compile_detour` records the new fact), `crates/theseus-core/src/fact/turn.rs` + `fact/mod.rs` (the fact), `crates/theseus-protocol/src/ledger.rs` (one kind), `crates/theseus-protocol/src/context.rs` (the fact's type and `ContextExplainResult`'s new field), `crates/theseus-core/src/rpc/context.rs` (`context_explain`), `crates/theseus-protocol/src/why.rs` and `fill.rs` (the words), `crates/theseus/src/why.rs` (the CLI picks the route row of the detoured turn), `cockpit/src/protocol.gen/` (regenerated) |
| C | theseus-3bs4e | `crates/theseus-core/src/tools_defer/want.rs` (`extra`), `crates/theseus-core/src/turn/defer_step.rs` (`defer_tools`), the detour's spec |

### A. A detour carries the project slice (theseus-v4u66)
- In `compile_detour`, after `Self::persona_line(t, &mut compiled)`, attach the slice exactly as `tail_blocks` does:
  `crate::project::slice::attach(&self.store, &self.kernel, session, &mut compiled)` (`session` is the
  `&SessionRecord` it already has). Trivial and quick detours both: a trivial "and the deadline?" about the project is
  as wrong without it. A conversation with no project attached gets nothing (`attach` returns at
  `session.project.as_deref()?`), so a plain detour's request, tokens and frames are unchanged.
- Keep the slice's summary (`ProjectSliceSummary`) for B's fact.

### B. `theseus why` explains a detoured turn (theseus-r5ydi)
1. **A fact for a detour's request.** A new ledger fact kind `context.detoured` (a new kind is fine; no store format
   change), written by `compile_detour` per loop, beside its `loop.started` record and **in the same frame** (check
   how `t.record` batches into the turn's frame; a detoured plain turn must not gain a frame: test it). It carries no
   text, only names and counts, as `context.compiled` does: `session_id`, `turn_id`, `loop`, the profile and model it
   ran on (the routed target, `t.target`), the kind of detour (`trivial` or `quick`), `est_tokens`, the class of the
   place, and its parts as `{block, name, tokens}`: the system header (the profile's own and the standing records),
   the persona line, the last exchanges (`detour_start` / `trivial_context_turns`: how many, their tokens), the
   message, the turn's recall (its id and tokens, quick only), the project slice (its summary, from A), and the tools
   offered (count, and the families loaded). Its type goes in `crates/theseus-protocol/src/context.rs` (not `lib.rs`:
   `crates/theseus-protocol/src/lib.rs` is at 2,730 of its 2,733-line ceiling).
2. **`context_explain` reads it.** When the session's newest turn (or the asked `turn_id`) has a `context.detoured`
   row and no `context.compiled` row, explain that turn from the detour row: `turn_id`, `turns` (the turn list
   includes detoured turns, marked as such, newest first, interleaved with compiled ones by position), the profile
   and model it ran on (not `system_of`'s session target, which is the session's own profile), the parts from the row,
   and `recalls` through the existing `turn_recalls`. Add one optional field to `ContextExplainResult`, e.g.
   `detour: Option<…>` (kind, profile, model), absent for a compiled turn so every existing reader is unchanged. Rows
   written before this change do not exist for old detours: for those, keep today's answer.
3. **The words.** `why::build`: a detoured turn's head says it ran as a detour on `<profile> (<model>)`, its request
   built outside the conversation's compilation, then its parts with tokens, then **what it left out** as a plain list:
   the context files, the persona's body, the conversation's summary and older turns, earlier recall (and, for a
   trivial detour, the turn's own recall). `fill::context_short` (`theseus why --short`, `/context`) shows the same
   parts against the window. `crates/theseus/src/why.rs` takes the `route.decided` row of the detoured turn so the
   `routing:` line reads `a detour, outside the session's compilation` (`reason_words("detour")`). Discord's `why`
   (`crates/theseus-discord/src/runtime/why.rs`) uses the same `WhyInput` and follows for free: check it.
4. Regenerate the cockpit's TypeScript types (`crates/theseus-protocol/AGENTS.md` says how:
   `the_cockpits_types_are_generated_from_the_rust_ones`) and `git add` `cockpit/src/protocol.gen/`.

### C. A named family is counted on the whole request (theseus-3bs4e)
- `want::extra` keeps a named family only while **the whole first request** fits the place kind's budget: the
  system's tokens, what deferring costs, the families already loaded, and this family, at the model's rates. Use the
  `Fit` that `defer_tools` already builds (`Fit::of(kind, spec, rates, mechanism)`) instead of passing `0`: pass
  `&Fit` (or its `system + held` and per-family `cost`) to `extra` and compare against `Fit.budget`. Families are
  tried best first (`named`'s order) and a family that does not fit is skipped while a smaller later one is still
  tried, so the set kept is the best-ranked set that fits.
- **On a detour, count the request the detour sends.** The spec `defer_tools` sees in `route_to` is the routed
  profile's full spec; `compile_detour` then clears the context, persona and files. Compute the detour's `Fit` on the
  spec as the detour sends it: one function (e.g. `detour_spec(&RequestSpec) -> RequestSpec`) does the clearing, and
  both `compile_detour` and the fit call it, so the two can never drift. With that,
  `a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget` must stay green unchanged: the detour's request
  is small, so the AWS family that holds `aws_whoami` still fits at the cli.
- Remove the comment about the owner's word in `defer_tools`, and update `want.rs`'s module doc and `extra`'s doc to
  the new rule. Where an existing test asserted the tools-alone count, change it in the same commit and say why in the
  report (a deliberate rule change, not a weakened test).

## Tests (each with a planted revert; the first is the key test)
- **`a_quick_detour_in_a_project_conversation_carries_the_slice`** (key test, core, next to the detour tests in
  `crates/theseus-core/src/tests_route_defer.rs` or a new `tests_detour_project.rs` declared with one `mod` line): a
  conversation attached to a project with a goal, a quick verdict from the stand-in Jev, so the turn detours; the
  request the stand-in model receives contains the slice's head (`project::slice::HEAD`) and the goal. A conversation
  with no project: the request is byte-for-byte as before. Plant: drop the `attach` call.
- `why_explains_a_detoured_turn` (core, through `context.explain`): after one detoured turn, the result has the
  detour field, the turn id, the routed profile and model, the parts with non-zero tokens for the exchanges and the
  message, and the turn's recall rows (quick); `why_lines` says it detoured and lists what it left out, never
  `no turn of this session has compiled a context yet`. Plant: do not record the fact.
- `a_compiled_turn_explains_as_before`: a normal turn's `context.explain` result serialises exactly as on `main` (no
  `detour` field). No plant needed beyond the one above; say so.
- `a_detoured_plain_turn_keeps_its_frames`: a detoured turn writes no more frames than it did on `main` (use the frame
  count the `tests_m3` frame-budget test uses). Plant: write the fact in a frame of its own.
- `a_named_family_is_counted_on_the_whole_request` (`want.rs` unit tests): a family whose tools fit the budget alone
  but not beside the system and the loaded families is not added; a smaller named family that fits is. Plant: pass a
  system of 0 again.
- `a_detour_naming_a_family_stays_within_its_place_budget` (core, the owner-shape fixture of
  `crates/theseus-core/src/tests_first_request.rs`): a detoured cli turn naming a large deferred family sends a first
  request whose estimate is within 8,000 tokens, and a named small family is loaded. Plant: compute the detour's fit on
  the full routed spec (the family then never loads, or the budget is passed: show which).
- `tests_first_request.rs` passes on the defaults and on the owner's shape; the 3msy0 test passes unchanged.

## Not in this row
- What a quick detour recalls (theseus-pa1xx, landed) and the quick route's vocabulary (theseus-z706m, queued on
  another machine). Which turns detour (routing's verdicts, `routing/*`, the route pack's prompt).
- Areas busy in other sessions: task questions and reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI
  chat (`crates/theseus-tui`), herdr (`contrib/herdr`), the Google rows (`crates/theseus-google`), job command lines
  (`theseus-kernel/src/job.rs`), effort on evidence (`turn/checkpoint_step.rs`, `routing/effort.rs`,
  `turn/route_step.rs` `route_effort`), the threads rows (`thread_route.rs`, `thread_recall.rs`,
  `theseus-memory/src/threads.rs`). `route_step.rs` is shared with the effort row: edit only `compile_detour`,
  `route_to`'s call into the fit, and a new helper; touch nothing in `route_effort` or `apply_effort`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code).
  Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. No new switch: the rollback is the
  revert of each piece's commit, named in the report.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). The slice is not always-loaded: it
  rides only in a conversation attached to a project, as on the normal path. A new model tool is deferred by default
  (this row adds none). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the path
  before a request leaves beyond the slice's read for an attached conversation, and nothing on the start path grows
  with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless it names only a test this task changes on purpose under C; explain each.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change (a
  new ledger fact kind is not one; if you find you must store something else, say why in the report and bump
  `MANIFEST_FORMAT` by one, never writing the number anywhere but the constant and its tests).
- **Secrets and names.** The new fact carries names and counts, never request text: a private place's file names stay
  out of a shared place's answer exactly as `context.compiled`'s do (`private` in `context_explain`). No person's name
  anywhere in the repo, in commit messages or in the report: write "the owner". Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md` has the detour's rules,
  `crates/theseus-protocol/AGENTS.md` the TypeScript export). Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM;
  none of this row's tests should need L1).

## Your report
Commit `CLOUD_REPORT.md` at the repo root (written with your editor tools, never a shell heredoc; check its size after
the commit, it must not be empty), push the branch, and end with a final message that starts `CLOUD REPORT COMPLETE`.
The report holds:
1. Any file left over its line ceiling, with the reason (first section; "none" if none).
2. What changed, per piece (A, B, C), with a before/after of `theseus why` on a detoured turn (paste both outputs from a
   test's rig).
3. Each test and its plant, and the test that failed on it; any existing test changed under C and why.
4. The new fact's fields, and the frame count of a detoured turn before and after.
5. What could not run on the VM, with the exact commands for the maintainer.
6. The keel guard's output.
7. Follow-ups, each with what a person would hit.
