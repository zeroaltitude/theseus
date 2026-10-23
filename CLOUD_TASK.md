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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7b-tiers`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail7b-tiers, injection sized to the prompt: three tiers from Jev's route mode for recall and tool admission, more generous recall, and the ontology walk measured (theseus-d9je7 rows 2, 4, 5)

Branch: `cloud/20261021-tail7b-tiers`. Every commit's subject carries `theseus-d9je7`. Deadline for the report: 4 hours
after you start.

**Why.** An audit of Theseus's token budgets (theseus-d9je7) asked the owner five questions. His answer, 2026-10-22:
"Sure, just remember that the budgets should be far more generous on tokens. Trivial request should be a find-a-tool --
that's a nice touch. Deep prompts should get more recall, yes. Yes, long sessions are fine, and threads should have
high leeway. The first-request size should continue to be a regression alarm, though we could consider how small to
keep it." The principle: budgets limit what Theseus INJECTS relative to what the prompt needs (a greeting gets a small
injection, a deep task gets more); they do not limit how large a session may grow. Today the recall pack is 1,500
tokens and 6 items whatever the prompt is, and it is full more often than not (169 of 272 recent recalls hit 6 items;
4,514 candidates were dropped "budget"). Jev's route pack already classifies every message into a mode (answer p50
129 ms, p90 205 ms), and detours already use it to shed context. This row extends that to recall and tool admission.
A sibling row (tail7b-budgets) takes the trivial detour's tools, threads' cold resume and the first-request report.

**The approved numbers (the owner's "far more generous"):**
- Tiers, folded from Jev's mode: **small** = `trivial`; **ordinary** = `quick`, `chat`, `routine_coding`, and anything
  else (a late, missing or low-confidence verdict, routing off); **deep** = `sophisticated`, `deep_coding`, `review`.
- Recall pack: small **0** (as today: trivial detours already skip recall); ordinary **3,000 tokens / 8 items** (was
  1,500 / 6); deep **8,000 tokens / 16 items**.
- `[memory] session_recall_cap_tokens` (the recall notes a session's tail may hold): **64,000** (was 12,000).
- Tool admission past the place's budget (`[tools.defer] admit_ceiling`, 2.0 today for every turn): by tier. The
  audit proposed small 1.0, ordinary 1.5, deep 2.0; the owner asked for more generous, so: small **1.0** (a trivial
  detour carries no families anyway after tail7b-budgets), ordinary **2.0** (today's: no prompt gets less than today),
  deep **3.0**. Report what deep 3.0 admits on the test shapes.
- The first-request budgets (8k / 15k ...) are untouched: they stay the census's regression alarm.

**Risk: medium.** A new seam on the turn path; recall currently packs before the verdict is read (see below).

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7b-tiers` | theseus-d9je7 (rows 2, 4, 5) | the `InjectionBudget` seam and the tier fold; recall sized by tier; `session_recall_cap_tokens` 64,000; `admit_ceiling` by tier; the walk's tokens on `context.compiled` (measure only) | a new core module for the tier (e.g. `turn/tier.rs` or `injection.rs`, one `mod` line), core `turn/route_step.rs` (`RouteState`: one field, set where `result` is set), core `config/memory.rs` (defaults and validation), a new config table if you need one (under `config/`), core `turn/recall_step.rs`, `recall.rs` (`Scene.budget_tokens`, `Begun.max_items`: the existing per-turn overrides), `turn/rerank_step.rs` (only if the pack call must move), core `turn.rs` (the order near :1853: at most a few lines; tight ceiling), core `config/defer.rs` and `turn/defer_step.rs` near :142 (`Fit::of`'s ceiling by tier), protocol `events.rs` (`ContextCompiled`: one walk field) and `cockpit/src/protocol.gen/` (regenerate that file only), core `turn/compile_step.rs` (fill it), core `rpc/explain.rs` or wherever `theseus why` reads the route (one line), new tests `tests_injection_tiers.rs`, the template's `[memory]` and `[tools.defer]` lines, core AGENTS.md (recall and deferral paragraphs) | L |

## What exists (at `ae420a49`; search by name; under `crates/theseus-core/src/`)
- Modes: `config/routing.rs` (747 lines) `MODES` near :190 (seven: trivial, quick, chat, sophisticated, deep_coding,
  routine_coding, review; "other" is not in it), `DETOURS` near :205, `RECALLING_DETOURS = ["quick"]` near :216,
  `detour_recalls` near :219.
- `RouteState` in `turn/route_step.rs` (1,349 lines) near :66-140: `detour_recall` (near :86, set near :713),
  `detour_kind` (near :89), `keeps`, `families` (near :135), `result: Option<TurnRoute>` set in `record_route` (near
  :856, set near :924, `mode: verdict.map(|v| v.mode.clone())`); also `checkpoint_step.rs` near :567. `TurnRoute.mode`:
  protocol `route.rs` near :18 (tail7-effort edits that file: read only).
- Recall: `config/memory.rs` (608 lines) `recall_budget_tokens` 1,500 (`default_budget` near :200), `recall_max_items`
  6 (near :206), `session_recall_cap_tokens` 12,000 (near :228), `MAX_RECALL_ITEMS` 40 (near :270), validation near
  :322-336 (session cap >= budget, budget > 0, items 1-40), `params()` near :402. `recall.rs`: `Scene.budget_tokens:
  Option<u64>` (near :680, default applied near :773) and `Begun.max_items: Option<usize>` (near :622, `Begun::params`
  near :640, applied near :842): per-turn overrides already exist. `turn/recall_step.rs` (687 lines): `scene()` near
  :207 sets `budget_tokens` only when assembled (near :242); the session cap near :376; `recall_routed` near :408 drops a
  trivial detour's recall. The pack call: `turn/rerank_step.rs` near :43.
- **The ordering problem:** `turn.rs` near :1853 runs `recall_first` BEFORE `compile_routed`, and the verdict is read
  inside `compile_routed`. So today recall is packed before the mode is known. The audit's design: the pack uses the
  tier when the verdict has landed, and ordinary when it has not (never wait for it). Find where the recall index's
  candidates and the pack separate (index ~190 ms, verdict p50 129 ms): pack (or re-pack the already-ranked
  candidates) after the verdict when it lands in time, without a new model call and without making a turn wait.
- Tool admission: `config/defer.rs` (190 lines) `admit_ceiling` 2.0 (near :45-46, default near :78), `judge_families`
  true, `judge_wait_ms` 200; `the_templates_defer_section_is_the_default` near :140; consumed at `turn/defer_step.rs`
  near :142 (`Fit::of`, `tools_defer/fit.rs` near :52). Tests: `tests_families.rs`
  (`a_cli_detour_naming_aws_loads_it_past_the_budget_by_exactly_its_size` near :46, `a_family_jev_judges_relevant_
  loads_without_being_named` near :174, `jev_late_or_unreachable_falls_back_to_the_named_rule` near :226).
- The walk: `crates/theseus-ontology/src/text.rs` `GUIDANCE_MAX` (16 KiB per category), `judge/categorize.rs`
  `MEMBERSHIPS` 5, no total cap; the walk enters `context_text` at `ontology.rs` near :417 (`guided`);
  `Compilation.guidance: Vec<GuidanceUsed>` (`compiler.rs` near :118/:240) records category, version and digest only.
  `ContextCompiled` (protocol `events.rs` near :159) has no walk size; it is built in `turn/compile_step.rs` near
  :278-330. Guidance tokens are counted only in `context.explain` (`rpc/context.rs` near :450, :577-582) and census
  (`census.rs` near :97, :173, :244), from `context_parts.rs` near :108-120.
- Template: `crates/theseus-core/config/theseus.example.toml` `[tools.defer]` near :533 (`admit_ceiling` :539), the
  `[memory]` commented recall keys near :1053-1055.
- The owner's config sets none of these keys (the maintainer checked): the defaults are what he gets.

## What to build
1. **The seam (row 2).** One value, for example `InjectionBudget { tier, recall_tokens, recall_items, admit_ceiling }`,
   derived from the verdict by one fold function (mode -> tier; a late, missing or below-bar verdict, or routing off,
   -> ordinary) and kept on `RouteState` (one field, set where `result` is set). Defaults per the approved numbers.
   Configurable: ordinary's recall stays `[memory] recall_budget_tokens` / `recall_max_items` (their defaults become
   3,000 / 8); deep's are two new `[memory]` keys (for example `deep_recall_tokens = 8000`, `deep_recall_items = 16`,
   validated like the others, items <= 40); the admit multiples are keys beside `admit_ceiling` (which stays the
   ordinary tier's value, default 2.0) or a small table; pick the shape that reads simplest and say why. A config whose
   tiers equal one another is the rollback: say how in AGENTS.md.
2. **Recall by tier (row 2).** The pack takes the tier's tokens and items when the verdict has landed before the pack;
   otherwise ordinary. No turn waits longer than today for recall. Cache rule: inside a session the tier changes only
   the tail (recall); it must not change a cached prefix byte (the persona layer, context files, tools prefix) from turn
   to turn. `session_recall_cap_tokens` default 64,000 (validation still: cap >= the largest tier's tokens). The ledger
   (`route.decided` or `recall.ran`) records the tier and the pack size applied, and "ordinary (verdict late)" when it
   fell back; `theseus why` says it in one line ("deep: recall 8,000 tokens, 16 items").
3. **Admission by tier (row 4).** `Fit::of` takes the tier's multiple instead of the one `admit_ceiling`. Named and
   Jev-judged families past the place budget load up to it; the first-request census (an ordinary fresh request) is
   unchanged.
4. **The walk, measured (row 5, measure only).** Add the walk's guidance tokens to `context.compiled` (one field, the
   same count `context_parts.rs` gives census), and show it in `theseus why` if `why` already lists parts. Do NOT add a
   walk budget: the owner gets a week of numbers first. Regenerate `protocol.gen` for `ContextCompiled` only (the
   theseus-protocol test that writes it; tail7-effort regenerates `TurnRoute.ts` beside you: touch no other generated
   file).

## The keel line you must not edit
The template's `# recall_budget_tokens = 1500` line is a cap key to the keel guard (`cap_toml`, `CAP_KEY` matches
`budget`): raising its number there is a `cap-raised` finding, which needs a signed `Keel:` trailer a cloud row cannot
carry. So: change the Rust default (`default_budget` -> 3,000; the guard does not read it) and every other template line,
but leave that one template line's number at 1500 and add nothing to it. In the report's first section, give the exact
line the maintainer will apply in the lane's signed merge (the owner approved it on 2026-10-22). If a test asserts the
template's `[memory]` values equal the defaults and fails only on that line, say so and leave it red for that one
reason; do not change the test. Any NEW key you add (with "budget" in its name or not) is not a raise. Paste the guard's
output: it must show nothing else.

## Tests (each with a planted revert; the first is the key test)
- **`a_deep_prompt_gets_the_deep_recall_pack`** (key): a verdict `deep_coding` that lands before the pack: the pack
  holds up to 8,000 tokens / 16 items (stage more than 8 candidates that fit). Plant: ignore the tier -> 8 items.
- `a_late_verdict_packs_ordinary_and_says_so` (the ledger's "verdict late"). Plant: wait for the verdict.
- `an_ordinary_prompt_gets_three_thousand_and_eight`, `a_trivial_detour_still_has_no_recall`.
- `the_tier_never_changes_a_cached_prefix_byte` (two turns, ordinary then deep, same session: the prefix bytes are
  equal). Plant: let the tier touch a prefix part.
- `admission_follows_the_tier` (deep admits a judged family a 2.0 ceiling refuses; ordinary as today). Plant: one
  ceiling for all.
- `the_walks_tokens_are_on_context_compiled`. Plant: write 0.
- `why_names_the_tier_and_its_pack`.
- `tests_families.rs`, `tests_route*.rs`, `tests_recall*.rs`, `tests_first_request.rs` green;
  `the_templates_defer_section_is_the_default` green (with your new keys in the template).
- `cargo nextest run -p theseus-core recall`, `-p theseus-core route`, `-p theseus-core families`, `-p theseus-core
  first_request`, `-p theseus-protocol`, then `-p theseus-core` whole. Measure one ordinary and one deep turn's request
  size on the test rig before/after and quote them.

## Not in this row
- The first-request budgets (`first_request.rs`), the trivial detour's tool set, threads' working size and
  `resume_keep_turns` (tail7b-budgets: `turn/detour_step.rs`, the trivial case in `turn/defer_step.rs`'s
  `deferral_in`, `config/threads.rs`, `compiler.rs`, `compiler/cold.rs`). You share `turn/defer_step.rs`: your edit is
  the `Fit::of` ceiling near :142 only; theirs is the trivial case. Keep both small; the integrator merges them.
- A walk budget (after measurement), a tier for the persona layer or context files (cache rule), `memory_lookup`'s
  sizes, Jev's own limits.
- The other wave rows' files: tail7b-hold (`external.rs`, `PolicyConfig.external_text`, template `[policy]`/`[privacy]`,
  theseus-judge `pack.rs`), tail7b-cn (`draft.rs`, `draft/*`, `config/routing_cn.rs`, `toolrun/guarded.rs`),
  tail7b-messages (proactive messages outside the task flow), tail7b-golden (the Discord tests). The tail7 rows:
  tail7-onto (`judge/onto/*`, cockpit ontology), tail7-effort (protocol `route.rs`, `routing/effort.rs`, cockpit
  `lib/route.ts`, `protocol.gen/TurnRoute.ts`), tail7-threads (the TUI, `tests_hold_compile.rs`; a seam in
  `route_step.rs` only if it needs one: if you both touch `RouteState`, keep yours to one field), tail7-recall
  (`bench/recall/*`, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`).
- `scripts/keel-guard.py` and its test (a local lane).

## Rules for this row
- Model roles: you design and write it; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- `turn.rs` is 3,467 / 3,525 lines: put the logic in your new module; add only calls there.
- **Keel.** Do not delete, ignore or weaken a test; raise no first-request budget and no ceiling in
  `scripts/long-files.txt`. The recall and admission raises are the owner's approved change; the one template line
  above is left for the maintainer. A cloud row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report.
- **Dependencies.** None new. **Store format** 49: a new field on a ledger event or a stored record follows AGENTS.md's
  rule (serde-default; bump to 50 only if the rule says a stored layout changed, and add the old layout's sample).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (routing, recall, tool deferral, the cache rule, the store's version rule) and
  `crates/theseus-protocol/AGENTS.md` (regenerating `protocol.gen`) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. Its first section: the template line for the maintainer's signed merge, and anything left one line over. Then:
the seam's shape and where it is set; how recall gets the tier in time (and how often it would fall back, from the
timings you can measure); the cache rule's proof; the config keys and defaults; admission by tier and what deep 3.0
admits; the walk field; the request sizes before/after; each test and its plant with the failing line; the keel
guard's output; the store format; a `docs/status.md` sentence for the integrator; follow-ups (the walk budget after a
week of numbers).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
