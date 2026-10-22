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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-config-guidance`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from the integration branch `integ/cn` at `2d30cc48` (2026-10-22; integ/cn after its round 4: the open request, cn-tool-screen, cn-observability, cn-bg-rest, cn-draft-review), not from `main`. Where the
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
## Your task: cn-config-guidance, the template's China-origin coding guidance (cn_draft and final), the bench report's draft columns, and the data rule's invariants in the docs (theseus-a1wvq.12)

Branch: `cloud/20261021-cn-config-guidance`. Every commit's subject carries `theseus-a1wvq.12`. Deadline for the report:
3 hours after you start.

## Your base: `integ/cn` with the draft review in (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. Where the note above says your base is
`main`, or names a store format, this section is right. `integ/cn` is `main` plus the China-origin rows, each its own
merge. You build on:
- routing (theseus-a1wvq.4): `[routing.classes.<class>]` is `ClassProfiles` in `crates/theseus-core/src/config/routing.rs`
  (`#[serde(deny_unknown_fields)]`), with `profiles`, `switch_confidence`, `cn_profiles`, `cn_after`;
  `RoutingConfig::table(mode)`; the load checks in `config/routing_cn.rs` `validate`. The table was renamed from
  `[routing.modes.*]` (`config/renamed.rs` refuses the old name): never write `routing.modes` or `ModeProfiles`.
- the names (theseus-oejh3): the private list is `[privacy] sensitive_names` (`config/privacy.rs`), matched by code and
  sent only to models of US origin. `[routing.data]` (`config/routing_data.rs`, `RoutingData`) holds only
  `open_topics` and `open_paths`; its old `private_topics` key is refused at load. There is no `[routing.data]
  enabled` or `fallback`: an unadmitted turn on a restricted profile moves to the `[model]` default
  (`turn/data_step.rs`, `Reason::DataGate`).
- the open request (theseus-a1wvq.8): `data::admits`, `RouteState.admitted`, `data::clearance`, and the per-request
  `DataPass` at the provider chokepoint (`data_gate.rs`).
- observability (theseus-a1wvq.11), from the row cn-observability's task: `route.decided`'s data keys grouped in
  `record_route` (`fact/route.rs` `RouteDecided`); in theseus-protocol `own_bench.rs`, `RouteCell.origin`,
  `RouteReportResult.data: Vec<DataCell>` and `RouteReportResult.gates: Vec<GateCount>` (`GateCount { why, turns }`);
  the report's `Turn` (`own_bench/report.rs`) carrying the row's `origin`, `data` and `data_gate`, read in
  `Core::turns_since` (`own_bench/sampler.rs`); the CLI's `report_lines` (`crates/theseus/src/render/own_bench.rs`).
- the draft review (theseus-a1wvq.13), from the row cn-draft-review's task. **Its keys are your contract:**
  - on `ClassProfiles`: `cn_draft: Option<String>` (TOML `cn_draft`, one restricted-origin profile) and
    `final_review: Vec<String>` with `#[serde(rename = "final")]` (TOML `final`, a first-usable list of Claude
    profiles), both unset in the code defaults, both only on `routine_coding` and `deep_coding`; `final` is required
    when `cn_draft` is set and refused when it is not; every `final` entry must be a Claude profile;
  - on `route.decided`: `draft_skip`, written only when the turn's class has `cn_draft` and the turn did not draft,
    one of `not_admitted`, `in_task`, `wake`, `dir_not_open`, `not_git`, `draft_unusable`, `no_reviewer`;
  - the ledger row `task.draft` (`LedgerKind::TaskDraft`): `draft_task`, `draft_profile`, `draft_model`,
    `draft_cost_usd`, `draft_outcome`, `review_task`, `review_profile`, `review_model`, `review_cost_usd` (null when
    no review ran), `lines_changed` (measured), `tests_fixed_reported` (reported, absent when the reviewer gave no
    line), `applied`, `why`;
  - a draft is offered no program tools (`proc.run`, `job.*`, `term.*`); its Claude reviewer runs the tests.

**Check first:**
- `git log --oneline | grep -E 'theseus-a1wvq\.(8|11|13)'` shows commits of each;
- `grep -n 'cn_draft\|final_review' crates/theseus-core/src/config/routing.rs` finds both fields;
- `grep -rn 'draft_skip' crates/theseus-core/src/` finds the word list; `grep -rn 'TaskDraft' crates/theseus-protocol/src/`
  finds the ledger kind;
- `grep -n 'pub struct GateCount\|pub struct DataCell' crates/theseus-protocol/src/own_bench.rs` finds .11's types.

If `.13`'s commits, either config field, `draft_skip` or `TaskDraft` is missing, stop: write that in
`CLOUD_REPORT.md`, push it, and end. If `.11`'s report types are missing, go on: put `DraftCell` and the skip counts
beside `cells` and define the `GateCount` shape yourself, and say so. If a join renamed any name above, use the code's
name and say which in your report's first section. Each earlier row's `CLOUD_REPORT.md` (`git log --all --
CLOUD_REPORT.md`) has a "contract as built" section: read `.13`'s and `.11`'s before you start.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48 when this was written).
**This row bumps nothing**: the template is text, and the report reads ledger rows by key.

## Why
- The template's `[routing.classes.routine_coding]` and `deep_coding` (`crates/theseus-core/config/theseus.example.toml`)
  show only `profiles`: an operator cannot find `cn_draft` or `final`, and nothing says deep work stays on Claude. And
  `example_template_uncommented_still_parses` (`config/tests.rs`, which un-comments every documented key and loads it
  under `deny_unknown_fields`) cannot catch a misspelt `cn_drafts` that is not in the template.
- The bench report cannot answer the owner's D3 question (draft cost, review cost, how much the reviewer changed):
  `task.draft` rows and `draft_skip` words reach no report, so the benchmark arm has no numbers to read.
- `crates/theseus-core/AGENTS.md`'s "Invariants" and "Traps" say nothing about the data rule.

## Risk
Low: the template is comments (both draft keys stay unset in code), the report is read-only over `.13`'s rows, the
docs are prose.

## Prerequisite: `integ/cn` with `.8`, `.11` and `.13` merged (not `main`), as "Your base" says.

## Your row

### 1. The template's guidance (`crates/theseus-core/config/theseus.example.toml`, commented)
Some of it already exists on your base. Keep what is right and add only what is missing.

**Already there:**
- the commented `[profiles.dsflash]` (effort high), `[profiles.dsflashmax]` (effort max) and `[profiles.dspro]`
  (about line 155);
- the commented `[providers.deepseek]` with `api_key_secret = "deepseek_api_key"` (about line 279), and the
  commented secret line (about line 310);
- `[routing.classes.trivial]`'s commented `cn_profiles = ["glm"]` and `cn_after = "haiku"` (about line 1608);
- `[routing.data]`'s `open_topics` and `open_paths`;
- the `[privacy]` block (about line 1645).

**Add the D2 guidance.** Each item gets a one-line why in a comment. The owner's rule (2026-10-17): China-origin models
only draft code, and a Claude model always does the final review and fixes.
- `[routing.classes.routine_coding]`: `# cn_draft = "dsflash"` and `# final = ["sonnet"]`. The why: DeepSeek Flash
  drafts routine coding, tests, mechanical refactors and planned features, at about a sixth of Sonnet's cost per
  trial. Sonnet always reviews and fixes the draft before anything is applied. It needs the DeepSeek key and a
  session directory that is a git tree under `[routing.data] open_paths`. The draft runs no programs; its reviewer
  runs the tests.
- `[routing.classes.deep_coding]`: `# cn_draft = "dsflashmax"` and `# final = ["sonnethi", "opus"]`, as a pair that is
  off by default. The why: debugging with an unknown cause, and design, stay on Claude. Vendor Terminal-Bench 4.0 has
  deepseek-flash at 42.3 against Sonnet 5.5's 70.6. Say that the two keys go on together: `final` without `cn_draft`
  fails the load.
- `sonnethi` is not a profile in the template today (the owner's shape has one). Add a commented `[profiles.sonnethi]`
  (provider `anthropic`, model `claude-sonnet-5-5`, effort `high`) next to `[profiles.opushi]`, with one line saying
  it is deep_coding's first reviewer. It stays commented, so `Config::example()` does not change.
- Say once, in the routing classes' comment block, that no China-origin model is named for `chat`, `quick`,
  `sophisticated` or `review`. The why: Haiku 5.5 is two to three times cheaper per short turn and has no forced
  thinking, and review and judgment stay on Claude. Then say that a first-pass bug review by DeepSeek Flash is a later
  key (theseus-a1wvq.17), not yet in this table.
- trivial's lines stay as they are. Check that their comment says GLM-5.3 Flash serves only as Haiku's fallback, for a
  request that carries open content. If it does not, fix the comment.
- `[profiles.glm53]`: one comment line saying it is in no class list, because GLM-5.3 costs more per coding task than
  Sonnet 5.5 ($0.34-0.47 against $0.30 a trial). `dspro`'s comment says "in no mode list": make it "in no class
  list".

**Watch the planted-key test's parser.** `example_template_uncommented_still_parses` un-comments any `# ` line that
starts with `[` or whose text before its first `=` is a bare key, so a prose line like `# final = ...` becomes a key:
write the whys as trailing comments or as prose with no `=`. Un-commented, the template must still validate
(`dsflashmax` restricted and configured, `sonnethi` and `opus` configured Claude profiles).

**Assert the guidance.** Extend `example_template_uncommented_still_parses` with three asserts:
- `routine_coding`'s `cn_draft` is `Some("dsflash")` and its `final_review` is `["sonnet"]`;
- `deep_coding`'s pair is `dsflashmax` and `["sonnethi", "opus"]`;
- `trivial`'s `cn_profiles` is `["glm"]`.

If `.13` already put draft lines in the template (for `example_template_mentions_every_key`), move them into this
shape rather than adding a second set.

**Leave the `[privacy]` block's name list exactly as it is.** The owner chose its words. A test in theseus-judge
(`the_pack_text_names_no_organisation`) reads that line, and no row of this bundle changes it.

### 2. The bench report's draft columns (`own_bench/`, protocol `own_bench.rs`, the CLI's `render/own_bench.rs`)
This answers D3 from rows that already exist. It is read only. Put the new code in a new
`crates/theseus-core/src/own_bench/drafts.rs` (one `mod` line), because `report.rs` is at 363 lines.
- **The rows.** `route_report_now` reads `self.rows_since(LedgerKind::TaskDraft, since)` the way it reads
  `RouteReplayJudged`, building a `Draft` per row: `draft_model`, `review_model` (`none` when null), `draft_cost_usd`,
  `review_cost_usd` (0 when null), `lines_changed` (0 when absent), `applied`, `draft_outcome`. A row missing a key
  reads its default. A missing key is never an error.
- **The cells.** `drafts(rows: &[Draft]) -> Vec<DraftCell>` is pure and sorted. There is one cell per (`draft_model`,
  `review_model`), with these fields:
  - `drafts`, the rows;
  - `applied`, the rows with `applied: true`;
  - `reviewed`, the rows with a review model;
  - `draft_usd` and `review_usd`;
  - `lines_changed`, a sum;
  - `total_usd = draft_usd + review_usd`, the number the bench arm puts against Claude alone.

  `tests_fixed_reported` is not summed: it is reported, not measured, and a sum would read as a measurement.
- **The skips.** The report's turn read (`turns_since`, which after `.11` reads `origin`, `data` and `data_gate` from
  `route.decided`) also reads `draft_skip` onto the report's `Turn` as an `Option<String>`.
  `RouteReportResult.draft_skips: Vec<GateCount>` counts turns per word, sorted. A turn whose class had no `cn_draft`
  has no word and is not counted.
- **The wire.** theseus-protocol `own_bench.rs` gains `DraftCell`, with the same derive and `ts` attributes as
  `RouteCell`. `RouteReportResult` gains `drafts: Vec<DraftCell>` and `draft_skips: Vec<GateCount>`, each
  `#[serde(default)]` so an older daemon's result still reads. `crates/theseus-protocol/src/lib.rs` is not touched.
  Regenerate the cockpit's types with the protocol test (`cockpit/src/protocol.gen/`,
  `the_cockpits_types_are_generated_from_the_rust_ones`; `crates/theseus-protocol/AGENTS.md` names the update flag).
  Then `git add` the new `DraftCell.ts`.
- **The CLI.** `report_lines` prints one line per draft cell, for example
  `draft dsflash → sonnet: 4 drafts, 3 applied, $0.19 + $0.88 = $1.07, 212 lines changed`, then one `draft skips:` line
  (`dir_not_open 5 · not_git 1`). It prints nothing when both lists are empty. A report with no drafts is
  byte-for-byte what it was.

### 3. Docs
- **`crates/theseus-core/AGENTS.md` "Invariants"** (about line 2803) gains one bullet for the data rule:
  - a model of restricted origin (`cn` or `unknown`) sees only an open request: the open run and the open view;
  - the provider refuses a request to one without a `DataPass` for that exact request's digest;
  - origin is judged by endpoint and family as well as by catalog row;
  - `[privacy] sensitive_names` go only to models of US origin;
  - a China-origin model only drafts code, in its own tree, with no programs, and nothing it drafts is applied or
    reported before a Claude review (`final`).

  Cite the passages that already explain each, by their headings. Do not repeat them.
- **"Traps"** (about line 2947) gains short bullets:
  - DeepSeek's Anthropic endpoint serves a `claude*` model id with a DeepSeek model, so origin comes from the endpoint
    as well as the id;
  - a config row cannot loosen a built-in China-origin row's origin;
  - `deepseek-flash` is a floating alias, so its served model can change under the same id;
  - a later loop whose request leaves the open run moves to the `[model]` default;
  - `final` without `cn_draft` fails the load, so deep_coding's pair goes on together;
  - `[routing.data] private_topics` was renamed `[privacy] sensitive_names`.

  Check each against the code; leave out (and report) any the code shows wrong.
- **`crates/theseus-core/AGENTS.md`'s own-bench passage:** one sentence on the draft columns and the skip words.
- **`docs/status.md`:** one line in the 2026-10-19 v1 patch1 data and routing entry (about line 509). It names the
  template's draft guidance and the report's draft columns.
- **`docs/privacy.md`, "Who else sees it"** (about line 37): if it says what a China-origin model may receive, add one
  sentence saying such a model only drafts code that a Claude model reviews before it is applied.
- **The spec** (`docs/spec/`) is a copy of a master kept outside the repository. Do not edit it. Instead, write a short
  spec item for the maintainer in the report: what this row and `.13` built, in the voice of `docs/spec/31-part3-item-244.md`'s
  Item 248.

### 4. The proposed live-config block (in `CLOUD_REPORT.md` only, never in the repo)
The owner reviews it before install. TOML: `[providers.deepseek]` and the `deepseek_api_key` secret line ("once the
key exists"); `[profiles.dsflash]` and `[profiles.dsflashmax]` (no `dspro`); `[routing.data]` with
`open_topics = ["<the owner's open projects>"]` and `open_paths = ["<the owner's open trees>"]`; `[privacy]
sensitive_names` ("unchanged"); trivial's `cn_profiles = ["glm"]`, `cn_after = "haiku"` ("already in the owner's
shape"); routine_coding's `cn_draft = "dsflash"`, `final = ["sonnet"]`; deep_coding's pair commented out. The new
keys join the owner's existing class tables beside their `profiles`. No real name, project or path in the block.

## Tests (each with a planted revert; the key test first)
1. **`example_template_uncommented_still_parses`** (key; extend the existing test in `config/tests.rs` with item 1's
   asserts). Plant (a): misspell `cn_draft` as `cn_drafts` on routine_coding's line in the template. The test fails
   with `deny_unknown_fields`' unknown-field message. Plant (b): write `# final = ["dsflash"]` on deep_coding's line.
   The test fails on `.13`'s Claude-profile refusal. Restore each and `touch` the template.
2. `example_template_parses_and_validates` passes unchanged. Add an assert to it, or a sibling test
   `the_templates_draft_guidance_is_off_by_default`, that `Config::example()` has no `cn_draft` and no `final` on any
   class. Plant: un-comment routine_coding's two lines. The test fails.
3. `the_proposed_live_block_loads_on_the_owner_shape` (in `config/tests.rs`, or beside the owner-shape tests in
   `tests_first_request.rs`). Load `crates/theseus-core/fixtures/owner_shape.toml` with routine_coding's
   `cn_draft = "dsflash"` and `final = ["sonnet"]` set (edit the parsed `toml::Value`: a second table header is a TOML
   error) through the owner-shape tests' load. It validates, and the owner shape's first-request budgets are
   unchanged. Plant: `final = []`; the load fails naming `final`. Do not change `owner_shape.toml` (the shape as
   installed; the block is not installed yet).
4. **`the_report_counts_drafts_and_their_review_cost`**, pure, in `drafts.rs`'s tests, from planted rows:
   - two applied dsflash → sonnet drafts;
   - one dsflash draft with no review (`review_model` null, `applied: false`, `why` set);
   - one dsflashmax → sonnethi draft;
   - one row missing `lines_changed`.

   Check each cell's `drafts`, `applied`, `reviewed`, `draft_usd`, `review_usd`, `total_usd` and `lines_changed`, and
   check that the cells are sorted. Plant (a): count `reviewed` from `applied`. The unreviewed row lands in the wrong
   count. Plant (b): add `review_cost_usd` into `draft_usd`. The sums fail.
5. `the_report_counts_draft_skips_per_word`: planted route turns. Three have `draft_skip` words (two `dir_not_open`,
   one `not_git`) and two have none. The result is `[dir_not_open: 2, not_git: 1]`. Plant: count a turn with no word
   as `none`.
6. A `report_lines` test in `render/own_bench.rs` covers two cases. A result with drafts and skips prints the cell line
   and the skips line. A result with neither prints exactly what it printed before (compare it to the line set of an
   existing test). Plant: print the skips line when its list is empty.
7. `the_report_reads_a_draft_from_the_ledger`: a `Core` rig (the own-bench tests' rig) writes one `task.draft` row and
   one `route.decided` row with a `draft_skip`. `route_report_now` returns one draft cell and one skip count. Plant:
   read `LedgerKind::TaskCheck` (or any other kind) instead of `TaskDraft`.

Run these unchanged and passing: `tests_first_request.rs`, the own-bench tests, `config/routing_cn.rs`'s tests,
`tests_draft.rs` (`.13`'s), the protocol's generated-types test, the CLI render tests, and
`tests_m3::a_plain_turn_stays_within_its_frame_budget`.

## Line room (check before you edit)
`scripts/long-files.txt` sets the ceilings; an unlisted file stays at 2,500 lines or fewer. Before `.11`/`.13`: the
template 1,729, `config/tests.rs` 1,309, `report.rs` 363, protocol `own_bench.rs` 250, CLI `render/own_bench.rs` 344.
Read them again. Never raise a ceiling.

## Not in this row
- **DeepSeek Flash as a first-pass bug reviewer of a draft** (the owner's R7 answer of 2026-10-17: dsflash at max may
  review a draft's diff first, on open content only, and its findings go to the Claude reviewer, who adjudicates; it
  is never the only or final reviewer). This is theseus-a1wvq.17, a later row with its own key beside `cn_draft` and
  `final`. Do not add that key or a template line for it. The template only says it is a later key (item 1).
- **Running a draft's programs in the sandbox's open-roots view**, which would let a draft run its own tests:
  theseus-a1wvq.16, a local row. Until it lands, the template says the reviewer runs the tests.
- The live config itself (`/etc/theseus/theseus.toml`) is the operator's. This row only proposes the block (item 4).
  The DeepSeek key and its first live probe wait for the owner.
- The benchmark arm that uses these columns (China-origin draft plus Claude review, against Claude alone, on the night
  set's routine coding tasks) is the bench rerun's own work.
- Openings the owner's update of 2026-10-17 dropped (open places and threads, `theseus --open`, a cockpit toggle, a
  measured "tests fixed", a mid-turn provider switch for a stopped draft, a `data` label on native memory): name them
  in the report's follow-ups only.
- Jev judging Theseus's own replies (theseus-a1wvq.15), and the background callers (theseus-a1wvq.10).

## Rules for this row
- Model roles: you write the code and the docs; keep them minimal and elegant. Every line you add should earn its
  place. Prefer one right name over a compatibility alias: use `.13`'s and `.11`'s names exactly.
- **Fail closed, always.** This row changes no gate, no route and no default. If guidance would turn a China-origin
  model on anywhere by default, it is wrong. Never loosen a check, a threshold, a test or a ceiling to make a test
  pass.
- **On, measured, with a named rollback.** The report columns are on by default, and with no `task.draft` row and no
  `draft_skip` the report is byte-for-byte what it was (test 6). The template's guidance is commented, so the config
  stays the rollback: with no `cn_draft` in any class (the code default), nothing drafts.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new
  on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's
  turn bench must not move). The report reads rows only when the owner asks for it.
- Every behaviour change gets a test and a planted revert: break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed. The key test must be shown failing on both
  of its plants. A pure docs change needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. The template is a
  guarded file: comments only, and no cap's value changes. Before your last commit run `git fetch origin integ/cn` and
  then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` and paste its output in the report.
  It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** The repository is public. No person's name, company, client or private project name may
  appear anywhere: not in the template, a test, a fixture, a doc, a commit message or the report. Write "the owner"
  and "the owner's company"; test names are invented ("Acme", "Widgetco"). The `[privacy]` block's existing list is
  the owner's and stays as it is. Do not copy it, quote it or add to it. Tool inputs and outputs are not scrubbed at
  rest: hold raw values in memory only, and run anything that leaves (a fact, a report, a push) through
  `core.tools.scrubber`.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (core, protocol, the CLI). Keep CLOUD_REPORT.md honest:
  what you ran, what passed, and what you could not run in the cloud. L1 needs a non-root daemon with a job cgroup,
  and about 33 L1 tests fail on this VM: say which of your tests you could run, and give the maintainer the exact
  commands for the rest.
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core -p theseus-protocol -p theseus` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, never a shell heredoc, and check
after the commit that it is not empty. Its sections, in order:
1. The contract as built: the template's keys and their lines, `DraftCell`'s fields, `draft_skips`, and any name from
   `.11` or `.13` that the code had under another name.
2. What changed, file by file, with each file's line count against its ceiling.
3. Each test and its plant: the line broken, the failure seen, the restore.
4. What could not run on the VM, with the exact commands for the maintainer.
5. The keel guard's output.
6. "store format: unchanged (<n>)", and the regenerated cockpit types.
7. The proposed live-config block (item 4), for the owner's review before install.
8. The spec item draft for the maintainer (item 3).
9. The maintainer's live check after the join, on a scratch daemon with `routine_coding.cn_draft = "glm"` (standing in
   for dsflash) and `final = ["sonnet"]`:
   - after one drafted ask and one ask from a directory outside `open_paths`, `theseus model bench` shows a draft
     cell `glm → sonnet` and the skip line `dir_not_open 1`;
   - name the `task.draft` and `route.decided` rows to read.
10. The follow-ups, including the dropped openings above.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
