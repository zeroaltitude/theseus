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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7b-budgets`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ae420a49` (2026-10-22: release stage 6, install #74, which landed `integ/tail6` on top
of stage 5's `f6f2900e`); that is your base. Line numbers quoted below are at `ae420a49` but may still be off by a few:
search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f6f2900e`** (`git log --first-parent f6f2900e..HEAD` lists it):
- `ae420a49`: `integ/tail6` (the paced stand-in model; the ontology sweep's titles off the worker, its pair rule, kept
  kind names and the cockpit's renames; the thread growth race, the held row's timing, confirms and the budget question
  naming their turn; the data-gate word only when the gate decided, its counters; the task home's restart test and
  `sticky_unsure`; the recall bench's later-session plan and read-only test trees).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/turn.rs` 3,467 / 3,525
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you:** nothing else is integrating. Two waves of rows base on `ae420a49` and join the lane
`integ/tail7`: the four `tail7-*` rows started an hour before you (ontology sweep, effort words, threads/TUI, recall
bench and data-gate word), and the six `tail7b-*` rows of this wave. A local lane also edits `scripts/keel-guard.py` and
its test: touch neither. Each row owns the files named in its task's "Rules for this row" and "Not in this row"
sections. Those win over anything older. Stay out of the other rows' files.

**The Discord DM task flow is locked** (the owner, 2026-10-22: "please do not change how it is now, all of the little
touches are so good"): the task reply, the started and thread lines, the board, the forum posts and live cards, the
approval card and the task reports. No row in this wave changes their text, order or buttons. If your change would move
one of them, stop and say so in the report instead.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7b-budgets, a trivial detour carries only `tools_find`, a thread's cold resume keeps far more, and a report on the first request's budgets (theseus-d9je7 rows 1, 3, 6)

Branch: `cloud/20261021-tail7b-budgets`. Every commit's subject carries `theseus-d9je7`. Deadline for the report:
3 hours after you start.

**Why.** An audit of Theseus's token budgets (theseus-d9je7) asked the owner five questions. His answer, 2026-10-22:
"Sure, just remember that the budgets should be far more generous on tokens. Trivial request should be a find-a-tool --
that's a nice touch. Deep prompts should get more recall, yes. Yes, long sessions are fine, and threads should have
high leeway. The first-request size should continue to be a regression alarm, though we could consider how small to
keep it." The principle: budgets limit what Theseus INJECTS relative to what the prompt needs; they do not limit how
large a session may grow. This row takes three of the audit's rows; a sibling row (tail7b-tiers) takes the recall tiers,
tool admission by tier and the walk's measurement.
- **Row 1, trivial detour tools.** A trivial detour (Jev's route mode `trivial`: a greeting, a thanks) carries the
  place's whole tool set today: tools are 86-88% of such a request (about 6,400 of 7,500 tokens at the cli, 13,900 of
  15,800 in the owner's DM). The owner: a trivial request gets `tools_find` only (it can still find any tool).
- **Row 3, thread cold resume.** `[threads] working_tokens` (60,000) trims a thread on every cold resume, and
  `resume_keep_turns` is 6. The owner: threads get high leeway. New defaults: the working size is **40% of the serving
  model's context window** (about 400,000 on a 1M model), and `resume_keep_turns` is **20**.
- **Row 6, the first request's budgets as an alarm.** The first-request budgets (cli/task 8,000, DM 15,000, channel
  7,900, headless 8,600) stay exactly as they are: they are the census's regression alarm. The audit also saw the
  installed daemon's channel first request read 8,090 against 7,900. This row reports (no change): why the channel
  reads over, and how small each budget could be kept with today's measured requests (the floor each needs plus a
  margin), so the owner can decide later.

**Risk: low-medium.** A detour's tool set and a thread's resume size change; nothing else.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7b-budgets` | theseus-d9je7 (rows 1, 3, 6) | trivial detour = `tools_find` only; working size 40% of the window, keep 20 turns; a report on the first request's budgets | core `turn/defer_step.rs` (`defer_tools` / `deferral_in`: the trivial case), core `turn/detour_step.rs` (only if the detour's kind must reach `defer_step`), core `tools_defer.rs` / `tools_defer/fit.rs` (only if `tools_find` needs a seam), core `config/threads.rs` (`working_tokens`, `resume_keep_turns`, a `working_percent`), core `compiler.rs` (the one comparison near :781), core `compiler/cold.rs`, core `thread_context.rs` (near :124), tests `tests_detour.rs` / `tests_route_defer.rs` / a new `tests_trivial_tools.rs`, `tests_first_request.rs` (read only, plus a new report-only test if you want one, `#[ignore]`d), the template's `[threads]` lines (near :1181-1183), core AGENTS.md (the detour and threads paragraphs) | M |

## What exists (at `ae420a49`; search by name; under `crates/theseus-core/src/`)
- `turn/detour_step.rs` (153 lines) `detour_spec` near :23-31: drops context_text, persona_text, card, cold,
  context_files and walk, and does NOT touch the tools. It is called from `turn/route_step.rs` near :1085
  (`compile_detour`, near :1050) and from `turn/defer_step.rs` near :46.
- `turn/defer_step.rs` (324 lines) `defer_tools` near :36 runs `deferral_in` (near :111-180) for the session's place
  kind: the board's learned loads, `always_loaded` (default `["files","commands"]`), the place defaults
  (`tools_defer/family.rs` near :254-290), and families the message names or Jev judges relevant (`want::extra`), held to
  `Fit::of(kind, counted, .., admit_ceiling)` (`tools_defer/fit.rs` near :52). The route mode is never consulted.
- `tools_find`: `FIND` in `tools_defer.rs` near :51, `find_definition` near :210; `tooled` near :276 drops it when
  nothing is deferred (a trivial detour that defers everything must keep it). The tool: `tools_defer/find.rs`.
- The route state: `RouteState` in `turn/route_step.rs` near :66-140: `detour_kind` (near :89, set via `detouring` near
  :709-715), `keeps`, `result` (`TurnRoute`, with `mode`).
- Detour tool tests: `tests_detour.rs` (`a_detour_naming_a_family_stays_within_its_place_budget` near :330,
  `why_explains_a_detoured_turn`), `tests_route_defer.rs` (`a_detour_without_tool_search_reaches_a_tool_the_message_
  names` near :56, `..._an_aws_tool_within_the_budget` near :90), `tests_families.rs`.
- `config/threads.rs` (549 lines): `resume_keep_turns` default 6 (near :229), `working_tokens` 60,000 (near :232),
  `home_warn_percent` 50 (near :235: the "share of the window" pattern to copy; its tests in `tests_home_size.rs` and
  `the_home_warning_is_a_share_of_the_window` near :503); validation near :321-330 (`working_tokens >= 2,000`);
  `the_templates_threads_section_is_the_default` near :343. Used: `compiler.rs` near :781
  (`est.tokens > c.working_tokens`), `compiler/cold.rs` near :30/:85 (`fits`), `thread_context.rs` near :124-125.
  `turn/home_size_step.rs` near :202 computes `window * home_warn_percent / 100` from the catalog's `context_window`
  (1,000,000 when unknown, near :41): it no longer reads `working_tokens`.
- `first_request.rs` (159 lines): `CLI_TASK_TOKENS` 8,000, `DM_TOKENS` 15,000, `CHANNEL_TOKENS` 7,900,
  `HEADLESS_TOKENS` 8,600, `budget()` near :34. `tests_first_request.rs`: `every_kind_of_place_is_within_its_budget`
  near :102, `each_kind_of_place_is_counted_with_its_own_families`, `the_proposed_live_block_loads_on_the_owner_shape`.
  The census: core `census.rs` (`context_census` near :459), CLI `crates/theseus/src/census.rs`.

## What to build
1. **Trivial = `tools_find` only (row 1).** When the turn is a `trivial` detour, its request carries no tool families:
   only `tools_find` (and nothing the place's defaults, the board's loads or `always_loaded` would add). A family the
   message NAMES still loads (a "hey, check my AWS bill" that Jev called trivial must still reach the AWS tools: keep
   `want::extra`'s named rule; say whether Jev-judged families also stay, and why). `quick` detours keep today's set.
   Ordinary and deep turns are unchanged. The ledger's `context.detoured` / `why` show the trivial tool set (one line
   that says "trivial: tools_find only").
2. **Threads' leeway (row 3).** Add `[threads] working_percent` (default 40) read like `home_warn_percent` against the
   serving model's window; `working_tokens` becomes optional: when set it wins (an owner who set it keeps it), when
   unset the working size is `window * working_percent / 100`. `resume_keep_turns` default 20. Keep the `>= 2,000`
   floor's spirit for a set `working_tokens`; validate `working_percent` 1-100. One function answers "the working size
   for this model" and both `compiler.rs` and `compiler/cold.rs` use it. Template `[threads]` lines updated, and
   `the_templates_threads_section_is_the_default` stays green.
3. **The first-request report (row 6, no change).** Run the census tests on this VM and, in the report, give for each
   place kind: today's measured first request (cli, task, DM, channel, headless, on the test shapes and on
   `the_proposed_live_block_loads_on_the_owner_shape`), its budget, and the smallest budget that would still pass with a
   5% margin. Then explain the channel's 8,090 > 7,900 on the owner's daemon: compare the test shape with an owner-like
   channel (families on, the owner's place defaults) and name the cause you can show (an older install, a family the
   owner's config adds, or a gap between test and owner shapes). Change no budget and no test.

## Tests (each with a planted revert; the first is the key test)
- **`a_trivial_detour_carries_only_tools_find`** (key): a `trivial` verdict on a cli and on a DM place: the request's
  tools are `tools_find` alone, and its tools part is under 1,000 tokens (measure; quote before/after in the report).
  Plant: skip the trivial case -> the place's families come back.
- `a_trivial_detour_naming_a_family_still_loads_it`. Plant: drop the named rule for trivial.
- `a_quick_detour_keeps_its_tools` (unchanged set). `tests_detour.rs`, `tests_route_defer.rs`, `tests_families.rs`
  green.
- **`the_working_size_is_a_share_of_the_window`**: a 1M model gives 400,000; a 200k model gives 80,000; a set
  `working_tokens` wins. Plant: keep 60,000.
- `a_cold_resume_keeps_twenty_turns` (or the existing resume test's number moved). Plant: 6.
- `the_templates_threads_section_is_the_default` green with the new keys.
- `cargo nextest run -p theseus-core detour`, `-p theseus-core defer`, `-p theseus-core families`, `-p theseus-core
  threads`, `-p theseus-core first_request`, then `-p theseus-core` whole.

## Not in this row
- The first-request budgets themselves (`first_request.rs` constants): unchanged. They are keel-guarded, and the owner
  keeps them as the alarm.
- Recall sizes, `session_recall_cap_tokens`, `[memory]`, tool admission past the budget (`[tools.defer]
  admit_ceiling`), the ontology walk, the tier seam: tail7b-tiers owns them (`turn/route_step.rs` beyond reading
  `detour_kind`, `config/memory.rs`, `config/defer.rs`, `turn/recall_step.rs`, `recall.rs`, `turn/rerank_step.rs`, the
  protocol's `events.rs`). If you need the turn's mode in `defer_step`, read `t.route.detour_kind` / `t.route.result`;
  add no field to `RouteState` (tiers adds one).
- The home-size warning (landed, `home_warn_percent`): leave it.
- The other wave rows' files: tail7b-hold (`external.rs`, `PolicyConfig.external_text`, template `[policy]`/`[privacy]`,
  theseus-judge `pack.rs`), tail7b-cn (`draft.rs`, `draft/*`, `config/routing_cn.rs`, `toolrun/guarded.rs`),
  tail7b-messages (proactive messages outside the task flow), tail7b-golden (the Discord tests). The tail7 rows:
  tail7-onto (`judge/onto/*`), tail7-effort (protocol `route.rs`, `routing/effort.rs`), tail7-threads (the TUI,
  `tests_hold_compile.rs`), tail7-recall (`bench/recall/*`, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`).
- `scripts/keel-guard.py` and its test (a local lane).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling: the first-request
  budgets and `scripts/long-files.txt` stay as they are. The threads' working size is the owner's asked change (it is
  not a key the keel guard reads; if the guard flags anything, stop that edit and say so). A cloud row cannot carry a
  `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; nothing here is stored (if a ledger event gains a field, follow
  AGENTS.md's rule and say so).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (routing and detours, tool deferral, threads, the first request's census) first.
  Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the trivial detour's tools part before/after (cli and DM, tokens); what still loads on a trivial turn
and why; the working size rule and its numbers on a 1M and a 200k model; the first-request table (measured, budget,
smallest with a 5% margin) and the channel's cause; each test and its plant with the failing line; the keel guard's
output; the store format; a `docs/status.md` sentence for the integrator; a line for the owner's config (does his
config set `working_tokens`? the maintainer checks: if it does, it keeps winning); follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
