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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-routing-compose`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**Store format on your base: 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
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
## Your task: cn-routing-compose, routing's grammar for China-origin profiles: they enter a class only through `cn_profiles`, run only for a cleared ask, `cheapest` never picks one, and the config refuses them where the owner never wants them (theseus-a1wvq.4)

Branch: `cloud/20261021-cn-routing-compose`. Every commit's subject carries `theseus-a1wvq.4`. Deadline for the report: 4
hours after you start.

**The owner's words** (2026-10-17 09:26, verbatim; two names redacted, because this repository is public): "[The
owner's company] and any private or personal or confidential information never goes to Chinese models. General
questions and [an open project of the owner's], open source, openclaw, Theseus, etc can go to China." And 09:57,
verbatim: "I think that Chinese models in dms can be evaluated by content rather than by the nature of the channel. My
understanding is also that they do have coding skills that are worth applying at their cost -- I'd always want to do
final review and fixes with a more competent model." "Chinese models" means models of Chinese **origin** (GLM,
DeepSeek), whoever hosts them.

The design those words produced (its rule D2, which this row implements for routing): China-origin models act only
(a) as the `trivial` class's **fallback behind Haiku**, for a request that carries only open content, and (b) as
**coding drafts** in open tasks that a Claude profile always reviews and fixes (a later row, `cn_draft`). They never
answer on `chat`, `quick`, `sophisticated`, `review`, `routine_coding` or `deep_coding`. A later answer of the owner's
("Yes on deepseek flash for bug first review") lets DeepSeek Flash do a first-pass bug review inside the draft-review
row's task; that is a task step, not a routing class, so `review` still refuses China-origin profiles here.

## Why
- **The code's own default lists a China-origin model.** `crates/theseus-core/src/config/routing.rs`, `fn trivial()`:
  `list(&["haiku", "glm", CHEAPEST])`. The template (`crates/theseus-core/config/theseus.example.toml`, the commented
  `[routing.classes.trivial]`) says the same, and the owner-shaped fixture (`crates/theseus-core/fixtures/owner_shape.toml`,
  `[routing.classes.trivial] profiles = ["haiku", "glm"]`) mirrors a real config that listed it.
- **Nothing at load says a class list names a China-origin profile.** `RoutingConfig::validate` checks only that names
  are not empty and that no profile is called `cheapest`. A config can put `glm` or a DeepSeek profile first in `chat`
  or `review` and it loads silently.
- **`cheapest` lands on GLM.** `routing::cheapest` picks the usable profile cheapest for a short turn, whatever its
  origin, and GLM-5.3 Flash is cheaper than the Claude rows beside it: `routing.rs`'s own test
  `cheapest_is_the_usable_profile_cheapest_for_a_short_turn` asserts `Some("glm")`. So `trivial`'s `cheapest`, and any class ending in it, reaches GLM whatever the list says.
- **There is no way to say "this profile runs only for an open request".** `routing::Profile` has no origin and
  `routing::Ask` no clearance, so the later row that builds open requests (theseus-a1wvq.8) has nowhere to put GLM back
  as trivial's fallback without it reading as unrestricted intent.

After this row: a restricted profile (origin `cn` or `unknown`) can enter a class only by name in that class's
`cn_profiles`, and only `trivial` accepts one; it runs only when `Ask.cleared` is true (always false until .8);
`cheapest` never returns one; a class's `profiles` naming one fails the load in words.

**Risk: low-medium.** The routing rule is pure and every caller passes `cleared: false`, so no turn routes differently
on this branch except that `trivial`'s default no longer names `glm`. The real risk is the load refusal: a config whose
class list names a GLM or DeepSeek profile stops loading. The fixtures and template change with it, and the report
must give the maintainer the exact edit for an operator's config before install.

**Prerequisite: `main`.** Check first that the earlier China-origin rows are on it:
`git log --oneline | grep -E 'theseus-a1wvq\.[12]'` shows `45f86304` (cn-catalog) and `5b18838f` (cn-route-v6). Your base
is `88ddae57` or later, store format 47.

## What already exists (on your base; search by name)
- **Origin** (cn-catalog, joined): `catalog.rs` `ModelOrigin { Us, Eu, Cn, Unknown }`, `ModelOrigin::restricted()` (Cn or
  Unknown), `catalog::origin_of(api_base, vouched, model, entry)`; `ProviderConfig.origin` (an operator's vouch);
  `Config::profile_origin(&self, profile, catalog)` in `config.rs` (a profile or provider that does not resolve is
  `Unknown`).
- **Routing** (`crates/theseus-core/src/routing.rs`): `Profile { provider, model, unusable, short_cost, input_per_mtok,
  vision }`; `Profile::runs(&self, images)` = `unusable.is_none() && short_cost.is_some() && (vision || !images)`;
  `cheapest(profiles, images, cap)`, `within`, `pick(list, profiles, images, cap)`, `dear_guard(list, profiles,
  images)`, `Ask<'a> { verdict, base, est_tokens, hold, from, images, cap }`, `step_down`, `decide(cfg, profiles, a)`
  (it reads the list with `cfg.classes.of(&a.verdict.mode)`), `steer_to(cfg, profiles, a, profile, at_once)`. Its
  tests' fixtures `p(..)`, `five()`, `seven()` and `ask(..)` are in its `mod tests`.
- **Class lists** (`config/routing.rs`): the old `[routing.modes]` is now `[routing.classes]` (`config/renamed.rs` maps
  the old key), `ModeProfiles` is now `ClassProfiles { profiles, switch_confidence }` (`deny_unknown_fields`),
  `RoutingClasses { trivial, quick, chat, sophisticated, deep_coding, routine_coding, review }` with `table(mode)` and
  `of(mode)`, `MODES`, `DETOURS`, `is_detour`, the default functions `trivial()` … `review()` built by `list(..)`.
  `RoutingConfig::validate(&self, profiles: impl Fn(&str) -> bool)` is called once from `Config::validate`
  (`config.rs`, `self.routing.validate(|p| self.all_profiles().contains_key(p))?`). Further down the same
  `Config::validate`, `let catalog = crate::catalog::Catalog::with_overrides(&self.catalog);` is built for the
  `refusal_fallback_model` check.
- **Where `routing::Ask` is built** (each needs the new field): `turn/route_step.rs` `decide_route`;
  `turn/route_step/correct.rs` (the correction's `let a = routing::Ask { .. }`); `turn/brief_step.rs`;
  `routing/checkpoint.rs` (from `CheckpointAsk`); `routing/brief.rs` (`brief_base` uses `..a.clone()`, its tests build
  one); `routing.rs` tests' `ask(..)`.
- **Where `routing::Profile` is built**: `turn/route_step.rs` `route_profiles(&self, sensitive: bool)` (the one
  production site); tests in `routing.rs`, `routing/brief.rs` and `correction/mod.rs`.
- **`pick` outside `routing.rs`**: `correction/mod.rs` (`pick(cfg.classes.of(m), profiles, false, None)`).
- **The names floor** (`data_gate.rs`, `turn/data_step.rs`, `route_profiles`' `data_gate` mark): not yours; see below.

**Not built anywhere yet:** `Profile.restricted`, `Ask.cleared`, `cn_profiles`, `cn_after`, the effective list, and any
load check on a profile's origin in a class. Run `git log --oneline --grep a1wvq.4` before you start in case a branch
landed part of it; if one did, build on it and say so.

## Beside you: the cn-floor-session row (read this before you touch a shared file)
Another session is building theseus-a1wvq.3 and .5 now, on branch `cloud/20261021-cn-floor-session`, from the same
base. It joins **before** you. It owns, and you must keep out of:
- `crates/theseus-core/src/data.rs` (new: `DataPass`, `DataWhy`, `Clearance`, `DataState`), `data_gate.rs`,
  `provider.rs` (`ProviderRequest.data_pass`), `turn/data_step.rs` (`data_floor`'s base gate), `session.rs`,
  `task.rs`, `fork.rs`, `rpc/inspect.rs`, `rpc/mod.rs`, `theseus-protocol/src/ledger.rs`, `fact/data.rs`, `fact/mod.rs`;
- **the store format**: it bumps 47 to 48 (`SessionRecord.data`) with a `SESSION_BEFORE_DATA` layout sample. **You
  bump nothing**: this row adds no stored field (`Ask`, `Profile` and the config are not stored), and `Routed`/`Hold`
  stay as they are;
- `route_profiles`' **`unusable` chain and its parameter**: it changes `route_profiles(&self, sensitive: bool)` to take
  the turn's floor and adds a restricted-and-uncleared case that marks the profile `unusable = Some("data_gate")`, and
  it edits every caller line `self.route_profiles(..)` (in `route_step.rs`, `brief_step.rs`, `checkpoint_step.rs`
  twice, `route_step/correct.rs`);
- its **test sort**: about 20 core test files send to the `zai` fake or route to a `glm` profile
  (`tests_route.rs`'s `glm_placements` and its users in `tests_route*.rs`, `tests_offer.rs`, `tests_onto.rs`,
  `tests_standing_ring.rs`, `tests_data_gate.rs`, `google/tests_private.rs`, `tests_task_brief.rs`,
  `tests_compaction.rs`, `tests_continuations.rs`, `rpc/tests.rs`, `own_bench/tests_sampler.rs`). It rewrites them to
  unrestricted second models. **Do not edit those files.**
- a new module under `config/` with one call in `Config::validate` (refusals of restricted profiles for
  owner-material keys such as `[people] extract_profile`).

**Files both rows touch, and how you keep the join clean:**
- `turn/route_step.rs`: you add exactly two lines: the `restricted` field as the **last** field of `route_profiles`'
  `routing::Profile { .. }` literal (after `vision`), and `cleared: false` as the last field of `decide_route`'s
  `routing::Ask { .. }` literal (after `cap`). Nothing else in the file.
- `turn/route_step/correct.rs`, `turn/brief_step.rs`: one line each, `cleared: false` as the last field of the
  `routing::Ask` literal. Never the `route_profiles` call lines.
- `config.rs`: one line in `Config::validate`, right after the `refusal_fallback_model` loop, calling your new module
  with the `catalog` built just above it. The joiner merges it beside the floor row's line.
- `crates/theseus-core/AGENTS.md`: one short passage in the routing section (the class lists), never the data rule's
  passage, which the floor row rewrites.

**`restricted` stays `false` in `route_profiles` on your branch.** Write the field as
`restricted: false, // set from the origin at the cn-floor join (theseus-a1wvq.4)`. Why: filling it from
`profile_origin` here turns red exactly the ~20 test files the floor row is already sorting, and two sorts of the same
tests cannot join. It changes nothing on its own: once the floor row has joined, its `data_gate` mark already makes
every restricted profile unusable for an uncleared turn, and nothing clears a turn until theseus-a1wvq.8. The joiner
replaces the line with `restricted: self.cfg.profile_origin(name, &self.catalog).restricted(),` after the floor row's
sort is on the line, where it changes no test. Put that exact replacement line, and this reason, in your report's
join notes. Every other piece of this row is real: the routing rule, `cheapest`, the effective list and the load
checks are all tested with restricted profiles built directly.

## Your row
Pure routing, as the design's §6.3 rules 1, 2, 8 and 9, narrowed by D2. `cleared` means "an open request can be built
for this ask"; every caller passes `false` until theseus-a1wvq.8.

1. **`routing.rs`**:
   - `Profile.restricted: bool` (doc: origin `cn` or `unknown`; such a profile runs only for a cleared ask, and only
     when a class names it in `cn_profiles`).
   - `Ask.cleared: bool` (doc: an open request can be built for this ask; false until the open-request row).
   - `Profile::runs` gains the clearance: usable && priced && vision-ok && (`!restricted || cleared`). Pass it through
     `pick` (a `cleared: bool` parameter), `dear_guard`, `step_down` and `steer_to` (both read `a.cleared`), so each
     inherits the rule with no special case. Holds and corrections to a restricted profile then break when not cleared
     (rule 9) because `pick`/`steer_to` no longer return it.
   - **`cheapest` never returns a restricted profile**, cleared or not: filter `!p.restricted`. A restricted profile
     enters a class only by name. The `cheapest` keyword inside a list therefore never reaches one either.
   - `decide` reads its list with `cfg.classes.effective(&a.verdict.mode, a.cleared)` instead of `of(..)`. The
     detours (`trivial`, `quick`, rule 8) go through `decide`, so they obey it with no extra code.
2. **`config/routing.rs`**, `ClassProfiles` gains, each `#[serde(default, skip_serializing_if = ..)]` so a config
   without them, and the sparse writer's output (`config/sparse.rs`), are byte-for-byte as before:
   - `cn_profiles: Vec<String>`: restricted profiles this class may use, **only for a cleared ask**;
   - `cn_after: Option<String>`: the entry of `profiles` the `cn_profiles` go right after; unset, they go first.
   `list(..)` builds the rest with `..Default::default()`.
   - `RoutingClasses::effective(&self, mode: &str, cleared: bool) -> Cow<'_, [String]>`, beside `of`: `profiles`
     borrowed when not cleared or when `cn_profiles` is empty (the common path allocates nothing); else `cn_profiles`
     spliced first, or right after `cn_after`. (The shared contract called it `routing::effective(..)`; it lives on
     `RoutingClasses` now that the lists are `RoutingClasses`. Keep this name: later rows use it.)
   - The default `trivial()` becomes `list(&["haiku", CHEAPEST])`. Fix its doc comment: GLM-5.3 Flash comes back only
     through `cn_profiles`, for a cleared ask, behind Haiku.
3. **The load checks**, in a new module `crates/theseus-core/src/config/routing_cn.rs` (one `mod` line in `config.rs`,
   one call in `Config::validate` as said above; it takes `&Config` and the `&Catalog`). Skip a profile name that is
   not configured in `profiles` (the defaults name profiles a sparse config may lack, which routing passes over).
   Each refusal names the key and says what to do:
   - **No class's `profiles` names a restricted profile**: "routing.classes.chat.profiles: glm is of restricted origin
     (cn): list it in routing.classes.trivial.cn_profiles, where it runs only for an open request". An `unknown`-origin
     profile is refused the same way, and the message names the way out: `[providers.<name>] origin` vouches its
     endpoint. A `cheapest` entry is fine (it never returns one).
   - **`cn_profiles` is refused on every class but `trivial`**: "routing.classes.review.cn_profiles: a China-origin
     model never answers review (the owner's rule: it drafts code that a Claude model reviews; the draft is
     `cn_draft`, a later key)". D2 keeps them off `chat` and `quick` as well: Haiku is cheaper for short turns.
   - Each `cn_profiles` entry names a configured profile of restricted origin (an unrestricted one belongs in
     `profiles`; `cheapest` is refused there), and `cn_after` names an entry of the same class's `profiles`. `cn_after`
     without `cn_profiles` is refused.
4. **Every `routing::Ask` literal** gets `cleared: false` (the sites listed above), and every test `routing::Profile`
   literal gets `restricted: false`. `correction/mod.rs`'s `pick` call passes `false`.
5. **The template and fixtures.** In `theseus.example.toml`'s commented `[routing.classes.trivial]`, the list becomes
   `["haiku", "cheapest"]`, followed by two commented lines, `cn_profiles = ["glm"]` and `cn_after = "haiku"`, under one
   short comment ("China-origin profiles, only for a request that carries open content; trivial only"). The
   un-commented template must still parse and validate (`example_template_uncommented_still_parses`; its `glm` profile
   is configured, so the check passes). In `fixtures/owner_shape.toml` and `fixtures/owner_shape_old.toml`, the trivial
   table becomes `profiles = ["haiku"]`, `cn_profiles = ["glm"]`, `cn_after = "haiku"` (the old file under its old
   `[routing.modes.trivial]` header), so the before-and-after resolve test still holds.
6. **AGENTS.md** (`crates/theseus-core/AGENTS.md`, the routing passage where `[routing.classes.<class>]` is described):
   two sentences: a restricted profile enters a class only through `trivial`'s `cn_profiles` and runs only for a
   cleared ask; `cheapest` never picks one; a class's `profiles` naming one fails the load.

## Tests (each with a planted revert; the first is the key test)
Put the new pure tests in a new file `crates/theseus-core/src/routing/tests_cn.rs` (`#[cfg(test)] mod tests_cn;` in
`routing.rs`), reusing `five()`/`seven()`/`verdict()`/`ask()` (make them `pub(super)` where needed) and marking the
`zai` profiles `restricted = true` in your own fixture. The config tests go in `config/routing_cn.rs`'s test module.
- **`a_restricted_profile_runs_only_when_cleared`** (T12): uncleared, `pick` skips a restricted profile named in a
  list, `dear_guard` and `step_down` never land on one, `steer_to` a restricted profile stays (`Reason::Fallback`);
  cleared, `pick` returns it by name. **Plant:** drop the `!restricted || cleared` term from `runs`; `pick` returns the
  restricted profile for an uncleared ask.
- `cheapest_never_picks_a_restricted_profile`: with GLM cheapest and restricted, `cheapest` gives the next
  unrestricted profile, cleared or not, and a list ending in `cheapest` never reaches GLM. **Plant:** drop the filter.
- `cn_profiles_come_first_only_when_cleared_and_obey_the_cap` (T13): `effective` gives `profiles` (borrowed) uncleared,
  `cn_profiles ++ profiles` cleared; a place's cap still skips a dear cn profile (`Reason::Capped`). **Plant:** make
  `effective` ignore `cleared`.
- `cn_after_places_cn_profiles_after_the_named_profile`: `["haiku","cheapest"]` with `cn_after = "haiku"` gives
  `["haiku","glm","cheapest"]` cleared. **Plant:** ignore `cn_after`.
- `a_detour_to_a_cn_profile_needs_clearance` (T16): a `trivial` verdict with `cn_profiles = ["glm"]`, `cn_after =
  "haiku"`: uncleared it detours to Haiku, and with Haiku unusable it detours to `cheapest`'s pick, never GLM; cleared
  with Haiku usable it is Haiku; cleared with Haiku unusable it is GLM. A `quick` verdict likewise never reaches GLM.
- `cn_profiles_parse_per_mode`: each class parses both keys; a config without them serializes as before; an unknown key
  is still refused.
- `a_mode_list_never_names_a_restricted_profile`: a GLM, a DeepSeek and an unknown-origin profile in a class's
  `profiles` each fail the load with the message above; the unknown one vouched `us` loads; an unconfigured name
  loads; the default config with the template's profiles loads. **Plant:** put `"glm"` back in `fn trivial()`; the
  default case fails.
- `cn_profiles_are_refused_where_china_models_never_answer`: `cn_profiles` on each of `quick`, `chat`,
  `sophisticated`, `review`, `routine_coding`, `deep_coding` fails; on `trivial` it loads; an unconfigured, an
  unrestricted or a `cheapest` entry fails; `cn_after` outside `profiles`, or without `cn_profiles`, fails. **Plant:**
  drop the class check.
- **Unchanged and passing:** every existing test in `routing.rs` (its fixtures stay `restricted: false`: they weigh
  cost order; say so in a doc line on `five()`), `the_owner_shaped_fixture_resolves_the_same_before_and_after`,
  `example_template_uncommented_still_parses`, `tests_first_request.rs` on the defaults and the owner's shape,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`.
- **Expected to change** (name each in the report; these are the row's keel findings): `config/routing.rs`
  `the_defaults_are_the_steps_table` and `trivial_and_quick_are_the_detours` assert the trivial default with `glm`;
  they now assert `["haiku", "cheapest"]` (each still pins the default). If a core test outside the floor row's list
  changes because the default lost `glm`, give it the list it needs explicitly, keep every assertion, and name it. If
  one in the floor row's list changes, stop and say so in the report rather than editing it.

## Not in this row
- Filling `Profile.restricted` from the origin in `route_profiles` (the cn-floor join; see above), the base gate, the
  `DataPass`, `data_gate` marks and refusals (theseus-a1wvq.3).
- Computing `cleared` for a turn and building an open request (theseus-a1wvq.8); `cn_draft`, `final` and the draft
  review, DeepSeek's first-pass bug review included (theseus-a1wvq.13); the route row's `clearance` fields
  (theseus-a1wvq.11); the template's full guidance and the proposed live config (theseus-a1wvq.12).
- The own bench's rung table (`own_bench/rung.rs`) reading `cn_profiles`: say in the report whether it should.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing or unparsable is restricted or refused, never open. Never loosen a
  check, a threshold or a ceiling to make a test pass.
- **On, measured, with a named rollback.** The rule ships on, with no switch: the rollback is reverting this row's
  commits; name them in the report.
- No new always-loaded model text and no model tool: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). FAST: nothing new on a plain turn (a
  plain turn stays within `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not
  move); `effective` allocates nothing when not cleared.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. Its
  only findings may be the expected changes named under Tests, each named in the report.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. **No store format change.**
- **The lane gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passes on your last commit (run
  `cargo nextest run -p theseus-core` first). Say what it printed.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the
  owner". Never write the owner's company or private project names (they live only in the operator's config).
  Fixtures use invented names such as "Acme" and "Widgetco".
- **The data rule binds this session too.** Your own model is Claude. No request of yours goes to a GLM or DeepSeek
  endpoint (there is no key on the VM; never add one).
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
- Out of scope: the owner's config, daemon and machine; any benchmark run; `docs/status.md`, the README,
  `docs/design/` and the spec (the maintainer writes those: say in the report what should change).

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its size
after the commit, it must not be empty), push, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
It holds, in this order:
1. Any file left over a line ceiling, with the reason (or "none").
2. What changed, with the commits (the rollback).
3. Each test, and each plant with the test that failed on it.
4. Every existing test you changed, why, and why its meaning holds.
5. **Join notes for the cn-floor join:** the exact `restricted:` replacement line for `route_profiles` and why it waits;
   each shared file and the lines you added there; that `Ask.cleared` should later come from the turn's `Clearance`
   (`is_cleared()`), so the floor row's `data_gate` mark and `runs` lift together.
6. **Before install:** the load refusal's effect on an operator's config. Give the exact edit for a class list that
   names `glm` (or any DeepSeek profile): move it to `[routing.classes.trivial] cn_profiles`, with `cn_after = "haiku"`,
   and drop it from every other class.
7. What could not run on the VM, with the exact commands for the maintainer.
8. The keel guard's output, verbatim.
9. The store format: unchanged at 47 (say so).
10. Follow-ups, each as one line a person could act on (for example: the bench rung table and `cn_profiles`; anything
    in this row you would design differently).
