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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-xlri5-families`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `87696a7c` (2026-10-21), unless your task names another base (an integration branch). Where the
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
## Your task: xlri5-families, Jev judges each tool family against the prompt and a family it admits may raise the request cap, under a hard ceiling (theseus-xlri5)

Branch: `cloud/20261021-xlri5-families`. Every commit's subject carries `theseus-xlri5`. Deadline for the report: 5
hours after you start.

**Why.** theseus-3bs4e counts a family the message names against the whole first request (`want::extra` holds it to
`Fit`, counted on `detour_spec`). At the cli a detour's request is about 7,300 tokens before any named family (the
default families' tools are about 6,100), so no AWS family (892-1,271 tokens) and no terminals family (1,264) fits
under 8,000. A person who asks a cli detour to "call aws_whoami" or "open a terminal" gets a Haiku-class model with
neither tool, reachable only through `tools_find`, which install #29 showed that model never calls. Two tests were
re-pointed to keep passing. The owner's decision, verbatim:

> "How about we just relax the limit when a special toolset is called for? I'm happy to always try to jev categorize
> each toolset as relevant to the new in focus prompt -- that way, we independently decide tool group by tool group,
> and if more tools are sometimes required, we could relax the hard limit on request base size."

So: Jev (the fast in-loop classifier) judges each hidden tool family's relevance to the prompt in focus, one
classification per prompt, cached. A family Jev judges relevant, or that the prompt names, is admitted and loads even
past the place's base budget: the cap becomes soft by exactly the admitted families' size, under a hard ceiling. A
ledger and footer line names the families that raised it.

**Risk: medium.** It touches the first request of every turn on a model without tool search, and adds one Jev call
per prompt with a bounded wait. A wrong "relevant" costs tokens, never correctness: the ceiling bounds it, and learned
families never ride the raise. Jev off, unreachable, over budget or late: the named-family rule alone.

**Prerequisite: integ/threads 6d4e0dd9.** See "Your base".

## Your base
Your branch starts from `integ/threads` at `6d4e0dd9` or later, not `main`: `main` at `88f2e48f` plus quick-vocab,
detours-why (3bs4e, v4u66, r5ydi), threads-cross, threads-routing, threads-links. **This section wins over the
preamble above** about your base, store format and line ceilings. If a name moved, search by name.
- **Store format: 48** (`MANIFEST_FORMAT`, `crates/theseus-store/src/store.rs`). No bump expected: `Deferral`
  (`crates/theseus-core/src/tools_defer.rs`) is not serialized, the ledger row gains an additive JSON key, the result
  gains a wire-only `Option`. If you must add a field to a stored record, bump to 49 and say why (the maintainer
  renumbers at the release join).
- **Line ceilings** (`scripts/long-files.txt`): `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (at most 3 lines,
  see D); `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (none); `crates/theseus-core/src/turn.rs` 3,414 /
  3,525 (one or two lines). New code in new files.

## Overlap
Two rows run from this same tip: threads-routing-rest (98bz.3/.5/.7: `rpc/thread_flow.rs`, `rpc/methods.rs`
`turn_submit`, `thread_split.rs`, `rpc/inspect.rs`, `thread_route.rs`, `threads_import/`, `submit.rs`,
`config/threads.rs`, `bench/`) and threads-home-claim (98bz.6: `rpc/thread_flow.rs`, `rpc/home_claim.rs`,
`config/threads.rs`, `rpc/mod.rs`, protocol `lib.rs`'s `THREAD_*` line, two ledger kinds after `ThreadUnmerged`).
Touch none of those files. Shared files take small additive edits: `mod` lines in alphabetical place, one `EMBEDDED`
entry at the end of the pack list, one `TurnSubmitResult` field far from `THREAD_*`. All three rows regenerate
`cockpit/src/protocol.gen/`; the integrator regenerates at the join.

## What exists (on `integ/threads`)
- **`crates/theseus-core/src/tools_defer/want.rs`**: `named(ask, hidden)` ranks the hidden families a message names
  (`PLAIN` everyday words name nothing; `ASK_CHARS` 4,000). `extra(ask, catalog, loaded, fit)` keeps each while
  `fit.total(now) <= fit.budget`, skipping one that does not fit. Tests include
  `a_named_family_is_counted_on_the_whole_request`.
- **`tools_defer/fit.rs`**: `Fit { budget, system, held, cost }`; `Fit::of(kind, spec, rates, how)` takes `budget`
  from `PlaceKind::tools_budget_tokens()` (`family.rs`, the `first_request` constants); `Fit::total`; `Fit::demote`
  demotes `Why::Learned` families, largest first, while `total(loaded) > budget`. `loaded` includes the message's
  extras, so today an extra over the budget would evict learned families.
- **`turn/defer_step.rs`**: `defer_tools(t, spec)` counts on `detour_step::detour_spec(spec)` when
  `t.route.keeps.is_some()`. `deferral_in(sid, kind, (spec, counted), ask)` picks `Mechanism::Search` or `Find` from
  the catalog's `tool_search`, builds `Fit::of(kind, counted, ..)`, calls `board.select(.., &Offer { known, extra, fit
  })`, and for `Find` only adds `want::extra(ask, ..)` and selects again. `defer_ended` records `ToolsUsed`
  (`fact/tools_defer.rs`, kind `tools.used`: `place`, `mechanism`, `loaded`, `deferred`, `used`, `searched`,
  `unused`).
- **`tools_defer/select.rs`** `Why::Extra` is documented "Offered per message by the caller (a seam: Jev's
  branches)": your seam. `board.rs` `select` turns `Extra` into `Wanted`, then calls `fit.demote`.
- **`turn/route_step.rs`**: async `compile_routed` -> async `compile_first` (waits for the route verdict `beside` the
  first compile) -> sync `route_to`, which calls `self.defer_tools(t, &mut s)` for the routed profile (a detour's
  tools). `detouring` sets `t.route.keeps` first. **`turn.rs`** `run_inner_body` (near :1741) calls `defer_tools`
  for the session's own profile before the first compile.
- **`turn/inbound_step.rs`** `inbound_point` sets `t.route.ask` from the message, then checks
  `self.judge.config().enabled` and starts the inbound packs.
- **Jev's in-loop pattern: commit `d3365737`** (theseus-98bz.10, `recall_cross.v1`) is the whole shape to copy: the
  pack toml (a `noul` question `per = "notes"`), `crates/theseus-judge/src/builders/cross.rs` and
  `Input::RecallCross`, fixtures (`inputs/recallcross.json`, `golden/recall_cross.v1.request.json`,
  `golden/recallcross.state.json`, `eval/recall_cross.v1.json`), `pack.rs` `EMBEDDED`, `replay.rs`, `eval.rs`,
  `eval_labelled.rs`, and in core `judge/cross.rs` `cross_live` (mode check, `jev_unreachable()`, a spawned task, a
  `oneshot`, `tokio::time::timeout`, `judge` spans; `same_subject` reads only an actionable non-shadow judgment at the
  confirm bar) and `turn/cross_step.rs` (the bounded wait). Visit every file it touched, including `tests_judge.rs`,
  `tests_inbound.rs`, `tests_continue.rs`, `crates/theseusd/tests/judge.rs`, `learning/rebuild.rs`: pack-count
  assertions move with you. `theseus_judge::fake::FakeJev` has `script`, `set_latency`, `hold_when`/`release`,
  `set_mode`, `seen`.
- **The re-pointed tests**: `git show 7d92404e -- crates/theseus-core/src/tests_defer.rs
  crates/theseus-core/src/tests_route_defer.rs` shows each before and after (part E names them). 3bs4e's own cli test,
  `tests_detour::a_detour_naming_a_family_stays_within_its_place_budget`, asserts no `aws_*` tool and
  `context.detoured` `est_tokens <= CLI_TASK_TOKENS`.
- **Budgets** (`crates/theseus-core/src/first_request.rs`): `CLI_TASK_TOKENS` 8,000, `DM_TOKENS` 15,000,
  `CHANNEL_TOKENS` 7,900, `HEADLESS_TOKENS` 8,600. The keel guard treats any change as keel: **none changes**.
  `tests_first_request::every_kind_of_place_is_within_its_budget` prints each place's first request on the defaults
  and on `OWNER_SHAPE`.
- **Footer path**: `TurnSubmitResult::harness_lines()` (`crates/theseus-protocol/src/nudge.rs`) chains the fallback,
  the nudge and `files_line()`; Discord's `render.rs` `footer`, the CLI's `cmd.rs` and the TUI's `chat/events.rs`
  print them.

## Your row
**A. The ceiling: new config with a default.** In `[tools.defer]` (`crates/theseus-core/src/config/defer.rs` after
`hide_after_days`: field, default fn, `Default`, `check`; template line in
`crates/theseus-core/config/theseus.example.toml`):
- `admit_ceiling = 2.0`: the hard ceiling as a multiple of the place kind's own first-request budget. `check` refuses
  outside 1.0-3.0. At 1.0 the soft cap is off, byte for byte today's behaviour: the cap's rollback.
- `judge_families = true`: Jev's per-family call; off is the named rule only: Jev's rollback.
- `judge_wait_ms = 200`: the most a turn waits for that call, counted from its start at `inbound` (`check`: at most
  2,000; 200 matches `[routing] max_wait_ms`).
- **Why 2.0** (put it in the doc comment and the report): the smallest round multiple that admits the largest single
  family, tasks (about 6,000 tokens), beside a cli detour's 7,300 base with room for an AWS family and terminals (7,300
  + 6,000 + 1,271 + 1,264 = 15,835 < 16,000); at 1.5 (12,000) tasks could never load at the cli. A multiple of the
  place's own budget keeps one set of numbers (the keel-guarded constants). It is a ceiling, not a target. Compute its
  worst-case extra cost per request at the catalog's Haiku-class price.

**B. The rule** (`tools_defer/want.rs`, `fit.rs`).
- `Fit` gains `ceiling: u64` (`floor(budget × admit_ceiling)`, the multiple passed into `Fit::of`) and `raised: u64`.
- `want::extra` takes admissible families in order (named, by `named`'s score; then Jev's, by confidence) and keeps
  each while (1) the request without admitted families fits the base, `fit.total(loaded) <= fit.budget` (today's
  precondition), and (2) the whole fits the ceiling, `fit.total(loaded ∪ kept ∪ {f}) <= fit.ceiling`. That is "soft by
  exactly the admitted families' size". It returns each family with its tokens and its source (`Named` or `Jev`).
- `Fit::demote` holds learned families to `budget + raised`: admitting AWS neither evicts a learned family nor lets
  one ride the raise. `deferral_in` sets `fit.raised` from what `extra` kept, before the second `offer`.
- `Mechanism::Search` changes nothing: no Jev set consumed, no raise (those models search). Say in the report whether
  loading Jev's families up front for them is worth a bench row.

**C. Jev's per-family judgment: pack `families.v1`, in-loop, cached per prompt.**
- `crates/theseus-judge/packs/families.v1.toml`, shaped like `recall_cross.v1`: `point = "inbound"`, `action =
  "none"`, one `noul` question `relevant`, `per = "families"`, `max` at the client's per-item limit (`client.rs`).
  Instructions in the spirit of "Does answering the message need a tool of the family numbered {item}?"; `when_true`
  / `when_false` say "needs" means the task calls for a tool, not that the subject is mentioned. Bars as cross's;
  justify them.
- `builders/families.rs`: `Input::Families(FamiliesInput { message, families: Vec<FamilyItem> })`, each item the
  family's name and up to three of its tools (wire name plus the description's first sentence, from the catalog's
  definitions), scrubbed and capped at `state_cap_tokens`. Fixtures (input, golden request and state, a small
  labelled eval: AWS, terminals, tasks, an MCP server, everyday messages needing none), `EMBEDDED`, replay and eval
  wiring, as `d3365737`.
- `crates/theseus-core/src/judge/families.rs`, as `judge/cross.rs`: `JudgeService::families_start(..) ->
  Option<oneshot::Receiver<..>>` (`None` when `judge_families` is off, the pack's mode is off, `jev_unreachable()`, or
  nothing to ask); `relevant(&Judgment) -> Vec<(String, f64)>` (actionable, non-shadow, confirm bar). Candidates: the
  catalog's hideable families (`family::hideable`) not in `always_loaded` and not loaded by default everywhere. The
  pack ships **live**, with its switch.
- **Start at `inbound`**: in `inbound_point`, after the judge-enabled check, one call that starts the judgment and
  parks its receiver and deadline (`now + judge_wait_ms`) on the turn. A steered message starts none.
- **Take it before each `defer_tools`, bounded**: an async `families_ready(t)` in a new `turn/families_step.rs`. When
  the counted spec's model is `Find` and an answer is pending, wait until the deadline at most (no wait once in), and
  store the relevant set on the turn; `defer_tools` passes it to `deferral_in`. Call it before `defer_tools` in
  `turn.rs` (one line) and in `compile_first` before `route_to`. `route_to` stays sync.
- **Cached per prompt**: later loops and the routed recompile reuse the stored set, never a second call for one
  message. A turn with no new message (a continuation, a task's later loop) reuses the session's last set, held in
  memory on the defer board keyed by the prompt's digest. Nothing persisted.
- **Fail safe**: late, failed, unreachable, over budget or off gives an empty set; the named rule works alone. A late
  answer is dropped (`answered: false` in the `judge` span).

**D. The ledger and footer line.**
- `tools.used` (`ToolsUsed`) gains `raised`, only when something was admitted past the base budget:
  `{"budget": 8000, "ceiling": 16000, "tokens": <whole request>, "families": [{"family": "aws", "tokens": 1012, "by":
  "named"}]}`. Carry it on `Deferral` (not stored), then `TurnUse`. Families admitted under the base are not listed.
- `TurnSubmitResult` (protocol `lib.rs`): one doc line, one serde line, `pub raised: Option<route::ToolsRaised>`,
  exactly the 3 lines of room. The type and its `fn line(&self) -> String` go in `crates/theseus-protocol/src/route.rs`;
  `harness_lines()` chains it, so Discord, the CLI and the TUI print it with no renderer edit. Words:
  `Tools raised the request to 9,612 tokens (budget 8,000, ceiling 16,000): aws +1,012 (named), terminals +1,264 (Jev).`
  If `lib.rs` would end one over at the join because a sibling adds a line, leave it and say so. Regenerate
  `cockpit/src/protocol.gen/` with the repo's ts-rs export test.

**E. Restore the two tests; pin 3bs4e's as the rollback.**
- `tests_route_defer::a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget` (3bs4e moved it to a DM
  session with `DM_TOKENS`): its body as at `88ddae57` (cli session `None`, `CLI_TASK_TOKENS` on the request's
  tools), plus a check that `res.raised` names the AWS family it loaded, `by: named`. The rig's `places.owner` line
  may stay.
- `tests_defer::a_model_that_never_calls_tools_find_still_reaches_the_family_the_message_names` (3bs4e made it say
  "detach the project"): the terminals body back ("open a terminal and run true", `term_open`, "and another
  terminal"). Keep the projects variant as a new
  test of its own.
- `tests_detour::a_detour_naming_a_family_stays_within_its_place_budget`: name and assertions unchanged; its rig sets
  `admit_ceiling = 1.0` and its doc says it pins the rollback.

## Measure first (before any code, and at the end)
On the base and on your head, on the defaults and on `OWNER_SHAPE`, record each place kind's first request (`cargo
nextest run -p theseus-core every_kind_of_place_is_within_its_budget --no-capture`, the `first request` lines), and
the cli detour's `context.detoured` `est_tokens` when the message names `aws_whoami` and when it names a terminal (an
`eprintln!` in your new test; on the base, a scratch test you do not commit). Put a before/after table in the report.
Expected: every census figure unchanged (an empty `ask` admits nothing, Jev is not asked); the cli AWS detour from
about 7,300 tokens with no AWS tool to about 8,300-8,600 with `aws_whoami`, under 16,000.

## Tests (each with a planted revert; the first is the key test)
1. **Key: `tests_families::a_cli_detour_naming_aws_loads_it_past_the_budget_by_exactly_its_size`.** Owner shape, cli,
   Haiku detour (`tests_detour`'s rig), a message naming `aws_whoami`. The first request offers `aws_whoami`; its
   `est_tokens` exceeds the same message's request without the family by exactly the family's `Fit` cost (state the
   rounding tolerance) and is at or under the ceiling; `tools.used` `raised.families` is the AWS family `by named`;
   `res.harness_lines()` holds the "Tools raised" line. Plant: `extra` holds to `fit.budget` again.
2. **`a_family_jev_judges_relevant_loads_without_being_named`**: a message asking which cloud account the session is
   on (no family word, no tool name); FakeJev scripts `relevant` true for the AWS family: it loads, `by: "jev"`. With
   `judge_families = false` nothing extra loads. Plant: `deferral_in` ignores Jev's set.
3. **`jev_late_or_unreachable_falls_back_to_the_named_rule`**: FakeJev held past `judge_wait_ms`; a message naming
   `aws_whoami` still loads it (named), and the turn waited at most `judge_wait_ms` plus a stated slack; Jev
   unreachable: no wait. Plant: wait without the deadline.
4. **`one_families_judgment_per_prompt`**: a three-loop turn on a detour that recompiles on the routed profile makes
   exactly one `families.v1` call (`FakeJev::seen`); a second prompt makes a second. Plant: start the call in
   `defer_tools`.
5. **`a_raise_never_evicts_or_admits_a_learned_family`** (`fit.rs` unit): with and without 1,200 admitted tokens, the
   same learned families load. Plant: `demote` back to `budget`.
6. **`admission_stops_at_the_ceiling`** (`want.rs` unit): named then Jev in order; a family past the ceiling is
   skipped and a smaller one after it loads; a base already over the budget admits nothing.
7. **The restored AWS and terminals tests.** Plant: default `admit_ceiling` 1.0; both fail.
8. **`a_detour_naming_a_family_stays_within_its_place_budget`** unchanged at `admit_ceiling = 1.0`: the rollback.
9. **`a_model_with_tool_search_asks_no_families_judgment_and_raises_nothing`**. Plant: drop the `Find` check.
10. Config: `check` refuses `admit_ceiling` 0.5 and 3.5 and `judge_wait_ms = 5000`; the template equals the default
    (extend the existing template test). Pack: builder golden, state, eval labels, pack count.

Must pass unchanged: all of `tests_first_request`, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and
`want.rs`'s and `fit.rs`'s existing tests (if `a_named_family_is_counted_on_the_whole_request`'s claim needs
`admit_ceiling = 1.0`, set it in its fixture and say so; its assertions stay).

## Not in this row
- Jev's families up front for models with tool search: a measured follow-up, if the report says so.
- A smaller default set for detours, or a named family displacing a default: the owner chose the soft cap instead.
- Any first-request budget constant: keel, and not asked.
- A threadbench or Terminal-Bench run: **the maintainer runs bench measurement after the join, never this session:
  the VM has no API keys.**
- `docs/`, the spec, `docs/status.md`: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. `admit_ceiling = 1.0` (the cap) and
  `judge_families = false` (Jev) are the rollbacks: tests show each brings the old behaviour back byte for byte.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget` passes unchanged, and the
  gate's turn bench must not move); Jev's call starts at `inbound`, beside the turn's other work, and the wait is
  bounded from that start; a tool-search model and an unreachable Jev never wait. Say in the report how long the call
  and the wait take on a test turn, and Jev's cost per prompt from `price.rs`.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling constant. Before your
  last commit run `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` (if that ref is not
  in your clone, the commit your branch started from, `6d4e0dd9` or later) and paste its output in the report. It must
  be empty; if it flags a restored line, paste the flag and show from `git show 88ddae57:<file>` that it restores the
  earlier claim.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change
  only as "Your base" says.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a judge state, a report, a push) through `core.tools.scrubber`. No person's name
  anywhere in the repo, in commit messages or in the report: write "the owner". Fixture paths, accounts and names are
  invented (reuse the tests' existing fake account).
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Add AGENTS.md sentences as their own lines, never by
  rewording another row's. Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run on the VM
  (about 33 L1 tests fail here; none is yours), and the exact commands for the maintainer.

## Your report
Commit `CLOUD_REPORT.md` at the repository root, written with your editor tools: what changed; the before/after
first-request table; each test and each plant with the test that failed; the ceiling's reasoning and worst-case cost;
Jev's latency and cost per prompt; what could not run on the VM and the exact commands for the maintainer (a live
check on the owner's cli with routing live and a Haiku-class detour: "call aws_whoami and tell me the account" calls
it with no `tools_find` and shows the "Tools raised" footer; "which cloud account am I on?" shows a `by: jev` entry
in the turn's `tools.used` row; name the real CLI commands); the keel guard's output; the store format (unchanged at
48, or why it moved); and the follow-ups (tool-search models, the eval set's size, docs). Push the branch, check
`CLOUD_REPORT.md` is not empty in the pushed commit, and end with a final message that starts `CLOUD REPORT COMPLETE`.
