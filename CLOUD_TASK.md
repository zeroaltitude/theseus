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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-floor-session`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: cn-floor-session, no request reaches a China-origin model without a pass for that one request, and a session's data state is stored (theseus-a1wvq.3, theseus-a1wvq.5)

Branch: `cloud/20261021-cn-floor-session`. Every commit's subject carries `theseus-a1wvq.3` (part A) or
`theseus-a1wvq.5` (part B). Deadline for the report: 6 hours after you start.

Part A (the floor) comes first and is the row's value. Part B (the session's data state) follows. If part A takes
longer than 4 hours, stop part B where it is, commit what passes, and say in the report what is left.

**The owner's words** (2026-10-17 09:26, verbatim; two names redacted, because this repository is public): "[The
owner's company] and any private or personal or confidential information never goes to Chinese models. General
questions and [an open project of the owner's], open source, openclaw, Theseus, etc can go to China." And 09:57,
verbatim: "I think that Chinese models in dms can be evaluated by content rather than by the nature of the channel. My
understanding is also that they do have coding skills that are worth applying at their cost -- I'd always want to do
final review and fixes with a more competent model." "Chinese models" means models of Chinese **origin** (GLM,
DeepSeek), whoever hosts them. Since then, two more answers (11:34, verbatim): "Yes let's say it gets the whole
conversation if jev judges the same as ok" (a Theseus reply joins the open run when Jev judges the reply open, like a
person's message), and "Yes on deepseek flash for bug first review".

The design those words produced: clearance is **per request**. A request to a model of restricted origin (`cn` or
`unknown`) may carry only the **open run** (the newest stretch of the conversation in which every node was judged open,
ending at the newest person message) and the **open view** (no owner material). It goes out only with a `DataPass`
minted for that one compiled request, covering its digest. Building open requests is a later row; this row builds the
floor they must pass, and the stored state they will extend.

## Why
- **Today a China-origin model can be sent anything that names nothing.** `main`'s floor
  (`crates/theseus-core/src/data_gate.rs`, the `Gated` wrapper around every provider, and `turn/data_step.rs`
  `data_floor`) refuses a request to a non-US model only when it names one of `[privacy] sensitive_names`, or when its
  session read a Google result (`google::private::in_scope`). Everything else goes: a message about the owner's health,
  a personal recall, a private context file, none of which is a listed name. And `guard` wraps nothing at all when the
  names list is empty and `[google]` is unset (`if names.is_empty() && cfg.google.is_empty() { return providers; }`).
- **The code's own default routes there.** `config/routing.rs`'s `trivial` default is `["haiku", "glm", "cheapest"]`;
  `cheapest` and the `quick`/`trivial` detours can land on GLM; a session's base, a `-P glm` pin, a hold or a
  correction can too. The owner's live install keeps GLM out of `trivial` by hand only, as an interim measure.
- So the rule "never goes to Chinese models" holds today only for the listed names. After this row it holds for every
  request: none reaches a restricted model without a pass, and nothing mints a pass yet. That is the safe interim state
  every later China-origin row opens a little, per request.

**Risk: medium-high.** Part A changes which model a turn may run on for every configuration that names a GLM or
DeepSeek profile, and it puts every provider behind the wrapper even with no names configured. Many existing tests use
a GLM profile as "a second, cheaper model" (see the test sort below). Part B adds a field to a stored record (a store
format bump).

**Prerequisite: `main`.** Check first that the earlier China-origin rows are on it:
`git log --oneline | grep -E 'theseus-a1wvq\.[12]'` shows `45f86304` (cn-catalog) and `5b18838f` (cn-route-v6).

## What already exists (on your base; search by name)
- **Origin** (cn-catalog, joined): `catalog.rs` `ModelOrigin { Us, Eu, Cn, Unknown }`, `ModelOrigin::restricted()`
  (Cn or Unknown), `ModelOrigin::cleared_for_sensitive()` (an allowlist: `Us` only), `catalog::origin_of(api_base,
  vouched, model, entry)` (China-origin endpoint first, then a China-origin family on any host, then the row), and
  `catalog::cleared_for_sensitive(..)`. `Config::profile_origin(profile, catalog)` and
  `Config::profile_cleared_for_sensitive(profile, catalog)` (`config.rs`).
- **Jev's data question** (cn-route-v6, then route.v7 in the data lane, joined at `63596193`): `routing/data.rs`
  `DataAnswer { choice, confidence, band }` (`open`, `private` or `unclear`), read on the verdict and "acted on by
  nothing yet". route.v7 judges content, not the place; `private` needs a sign; work whose owner the message does not
  show is `unclear`, which fails closed. `[routing.data] open_topics` (`config/routing_data.rs`); the private names are
  `[privacy] sensitive_names` (`config/privacy.rs`; the old `private_topics` key is refused).
- **The names floor** (theseus-oejh3, theseus-xs5ah):
  - the chokepoint: `data_gate.rs` `Gated` (`refuse`, `stream_message`, `count_tokens`), `names_in`, `guard`
    (called once in `Core::build`, `rpc/mod.rs`: `let providers = crate::data_gate::guard(providers, &cfg, &catalog);`),
    `DATA_GATE_CLASS = "data_gate"`, `ProviderError::DataGate { message }` (`provider.rs`; not transient);
  - the route's floor: `turn/data_step.rs` (`sensitive_message`, `sensitive_request`, `private_session`,
    `uncleared_profile`, `data_floor`, which moves a decision off an uncleared profile to the `[model]` default profile
    (`DEFAULT_PROFILE = "default"`) with `Reason::DataGate`, or fails the turn with class `data_gate`);
  - `turn/route_step.rs` `route_profiles(&self, sensitive: bool)`, which marks a profile `unusable = Some("data_gate")`
    when `sensitive` and the profile is not cleared for the names; its callers pass `t.route.sensitive` (in
    `route_step.rs`, `brief_step.rs`, `checkpoint_step.rs` twice, `route_step/correct.rs`); `decide_floored` and the
    turn checkpoint (`checkpoint_step.rs`) call `data_floor`.
  - `routing.rs` `Reason::DataGate` (`"data_gate"`) exists.
- **Tests of that floor:** `data_gate.rs`'s own tests, `tests_data_gate.rs`, `google/tests_private.rs`. The rig is
  `tests_route.rs` `rig` (two fakes, `claude` and `zai`, the second serving GLM models; `Core::build` wraps both).
- **The digest:** `ProviderRequest::digest()` (`provider.rs`, sha256 of the body with sorted keys, `digest::of`);
  `ProviderRequest.image_tokens` is the precedent for a `#[serde(skip)]` field that is never sent.
- **Session state precedents:** `SessionRecord` (`session.rs`) boxes `routed`, `imported` and `forked_from`;
  `take_turns_fields` copies the turn's fields back (`self.routed.clone_from(&turn.routed)`); the sticky `external`
  taint is set by `external::hold` and carried into a task in `task.rs` (`crate::external::hold(rec, taken, None)`).
  Forks: `fork.rs`, `rpc/fork.rs`, `tests_fork.rs`. Layout samples: `tests_layouts.rs` (`SESSION_BEFORE_*`).
  A ledger kind: `theseus-protocol/src/ledger.rs` (`PrivacyWarned = "privacy.warned"`), its fact in
  `fact/privacy.rs`, listed in `fact/mod.rs`'s `FACTS`.

**Not built anywhere yet:** `crate::data` (`DataPass`, `DataWhy`, `Clearance`), a check on a request's pass, a floor
for restricted origin on its own, `SessionRecord.data`. Search `git log --grep a1wvq` before you start in case a branch
landed one since this was written; if one did, build on it and say so.

## Part A, theseus-a1wvq.3: the floor
**Decision (taken for this row, stated so you do not redo it):** build the pass on `main`'s two existing functions, the
`Gated` chokepoint and `data_floor`, as the owner's tracker asked ("build the DataPass gate on those two functions"),
not on a second check inside `Anthropic::stream_message_impl`. The fallback stays the existing one, the `[model]`
default profile; the design's `[routing.data] fallback` and `enabled` keys are **not** added (routing's mode lists
already skip an unusable profile, so the default is only the last resort, as for the names today). Say in the report if
you find a case where that is wrong.

1. **`crates/theseus-core/src/data.rs`** (new, `pub mod data;` in `lib.rs`): the shared names later rows build on, so
   keep them exactly.
   - `DataPass`: private fields and constructor; holds the digest of the **one** request it covers;
     `covers(&self, digest: &str) -> bool`. Not `Clone` and not `Default`: a pass is minted, never copied into another
     request. Tests get one through a `#[cfg(test)]` constructor in `data.rs` that still takes the digest.
   - `DataWhy` (serde snake_case): `unchecked`, `private_view`, `not_in_run`, `unvetted`, `sealed`, `attachment`,
     `verdict_private`, `verdict_unclear`, `below_act`, `no_verdict`, `late`, `pack_not_live`, `tool_result`. Later
     rows use the rest; this row uses `unchecked`.
   - `Clearance`: opaque; `Clearance::closed(DataWhy)`, `is_cleared()`, `why() -> Option<DataWhy>`,
     `pass(&self, digest: &str) -> Option<DataPass>`. Only `data.rs` can build a cleared one (no public constructor
     takes "cleared"; the later row adds the checked one). Here every turn is `Clearance::closed(DataWhy::Unchecked)`.
2. **The chokepoint** (`data_gate.rs`):
   - `ProviderRequest.data_pass: Option<DataPass>`, `#[serde(skip)]`, so the body and the digest are byte-identical
     with and without it. Every `ProviderRequest { .. }` literal gains `data_pass: None` (or uses a constructor).
   - `Gated::refuse`: a model whose `origin_of(..)` is restricted is refused unless the request carries a pass that
     `covers` this request's digest; the names check and the Google mark stay as they are (a pass never lifts them:
     a named or Google-marked request to a model that is not cleared for the names stays refused). The digest is
     computed only for a restricted model; for any other the check is the origin and nothing else.
   - `guard` wraps **every** provider, always (the early return goes). Keep an unrestricted, unnamed request's path
     cheap: the origin per model may be memoised in `Gated`, never recomputed with a walk or a digest.
   - The refusal's message names the model and why ("… is of restricted origin and this request has no pass for its
     data; nothing was sent"), and writes one `data_gate.blocked` ledger row (model, origin, the session when the call
     is a session's). `guard` may take what it needs to write it (the store handle `Core::build` holds). A new
     `LedgerKind` goes in `theseus-protocol/src/ledger.rs` and its fact in a new `fact/data.rs`, listed in `FACTS`.
   - A refusal is never retried into a send: confirm that the driver's one retry of a non-transient class is refused
     again with nothing sent, or make `data_gate` not retried, and test it.
   - Check every place a provider is built (`git grep -n 'Anthropic::new\|Arc<dyn Provider>'`): each one a turn,
     compaction, keep-warm, replay or a background call uses must reach it through `guard`. List them in the report.
3. **Routing marks restricted profiles unusable.** `route_profiles` takes the turn's floor (the names' `sensitive` and
   the turn's `Clearance`) instead of the bare `bool`, and sets `unusable = Some("data_gate")` for a profile whose
   `Config::profile_origin` is restricted unless the turn is cleared (never, in this row). `cheapest`, `pick`,
   `dear_guard`, `step_down` and the detours then skip it with no change in `routing.rs`. Every caller passes it.
4. **The base gate.** `data_floor` today acts only when `t.route.sensitive`. It now also acts when the profile the
   turn would run on (the decision's, else `t.target.profile`) is restricted and the turn is not cleared: the `[model]`
   default runs when it is usable and unrestricted (and, for a named request, US), with `Reason::DataGate`; otherwise
   the turn fails before any model call with "No model cleared for this request's data is usable: <why>". This must
   hold on **every** turn whatever the routing state: a verdict, no verdict, a late or unsure one, shadow, a pinned
   `-P glm`, a session base on a GLM profile, a hold, a correction, a detour, a turn checkpoint, and a turn that asked
   no route at all (routing off). Find the paths that never reach `data_floor` today (the module's own doc says the
   chokepoint is the only floor "for a turn that asked no route") and put them under it, so a restricted base moves
   rather than failing at the chokepoint.
5. **The turn carries its clearance.** The turn's route state (`t.route`, beside `sensitive`) holds a `Clearance`,
   set to `closed(Unchecked)` at the turn's start. Nothing attaches a pass to a request in this row. Leave one clear
   seam (named in the report) where the later row mints a pass for the compiled request it judged.
6. **Config refuses a restricted profile for owner-material calls.** `Config::validate` refuses, in words
   ("<key> may not name a profile of restricted origin: its input is the owner's private material"), a restricted
   profile for `[people] extract_profile`, `[card] profile`, `[threads] import_profile`, `[judge.learn]
   writer_profile`, `[self] reviewer_profile`, `[initiative] profile` and `[ontology] name_profile`; and a `[judge]
   api_base` that `catalog::origin_of` makes `cn` (Jev reads every message and calls outside the core's providers, so
   the chokepoint never sees its requests). The origin needs the catalog: build it the way `Core::build` does
   (`Catalog::with_overrides(&cfg.catalog)`). Put the check in a new module under `config/`, one call in `validate`.
   `[memory] summary_profile` and `synth_profile` are a later row's (they get a fallback, not a refusal); the
   chokepoint already stops them.
7. **The code's `trivial` default still names `glm`.** Leave the list to the routing-composition row (theseus-a1wvq.4);
   with this row a `trivial` verdict skips it to the next usable profile. Show it in a test.

### Tests, part A (the first is the key test; it must fail on a planted revert)
- **`the_provider_refuses_a_restricted_model_without_a_pass`** (T24): a core built with **no** sensitive names and no
  `[google]`; a plain request ("Rename the call sites in the billing module.") to a GLM model through
  `core.runner.providers["zai"]` fails with class `data_gate`, the `zai` fake records **zero** requests, and one
  `data_gate.blocked` row is written. The same request to the Claude provider is sent. Plant 1: put back `guard`'s
  early return. Plant 2: drop the restricted-origin check in `refuse`. Each alone must fail it.
- `a_pass_covers_only_the_request_it_was_minted_for`: a pass minted for request A sends A; on request B (A with one
  changed byte, standing for a later loop's request) it is refused and the fake records only A. Plant: make `covers`
  return true.
- `data_pass_never_changes_the_request_digest` (T25): `digest()` and the serialized body are equal with and without a
  pass.
- `a_restricted_base_moves_to_the_fallback_with_data_gate` (T14, every case not cleared): no verdict, late, unsure,
  shadow, a `-P glm` pin, a correction to GLM, a hold on GLM, a session whose base is GLM, a `trivial` detour, and
  routing off. Each runs on the default with a `route.decided` row whose reason is `data_gate`, and the `zai` fake
  records nothing. Plant: skip the restricted case in `data_floor` (or in the routing-off path).
- `no_usable_fallback_fails_the_turn_and_sends_nothing` (T15): the default itself restricted; the turn fails with the
  sentence above, both fakes record nothing.
- `background_profiles_refuse_a_restricted_profile` (T6): one case per key of item 6, the `[judge] api_base` case
  included; an unrestricted profile for each still loads.
- `the_trivial_default_skips_glm`: a `trivial` verdict with the code's default list runs on Haiku's profile.
- `a_named_request_still_needs_a_us_model`: the names floor's behaviour holds beside the new one (a named request to an
  EU-vouched Claude is refused; a pass for a GLM request never lifts a name).
- FAST: `tests_m3::a_plain_turn_stays_within_its_frame_budget` and the first-request tests pass unchanged; say the
  turn bench's number if you can run it.

### The test sort (budget about 1.5 hours for it)
After items 1-4, run `cargo nextest run -p theseus-core` and list every test the floor turns red. Expect the ones that
send to the `zai` fake (`tests_route.rs`, `tests_data_gate.rs`, `google/tests_private.rs`, `tests_task_brief.rs`,
`tests_route_base.rs`, `tests_route_model.rs`, `tests_route_effort.rs`, `tests_standing_ring.rs`,
`own_bench/tests_sampler.rs`) and some that route to a `glm` profile (`tests_compaction.rs`, `tests_continuations.rs`,
`rpc/tests.rs`). Sort each into one of two kinds and handle it so:
- **Its subject is a model of restricted origin or the data floor** (for example `data_gate.rs`'s
  `a_restricted_model_is_never_sent_a_sensitive_name`, whose plain half sends to DeepSeek; `tests_data_gate.rs`'s
  `a_sensitive_name_skips_restricted_profiles` and `a_sensitive_name_with_only_restricted_profiles_sends_nothing`,
  whose plain halves run on GLM): the owner's rule reverses that half. Rewrite it to assert the floor, or, where the
  test is about the names, move its "plain" case to a profile that is unrestricted but not US (an EU-vouched
  provider), so it still tells the two floors apart. A test about how a GLM request is *built* (thinking, effort,
  pricing on the wire) mints a pass with the `#[cfg(test)]` constructor for that request's digest.
- **GLM is only "a second, cheaper model"** (a test of routing, holds, compaction or effort that does not care about
  origin): give it an unrestricted second model instead (for example a second fake provider serving a Haiku model, or
  the rig's second provider pointed at an unrestricted model), keeping every assertion's meaning. Never delete the
  test and never drop an assertion.
List every changed test in the report (file, test, which kind, one line on why the meaning holds). These are the
row's expected keel findings: the keel guard will name them, and the report names each.

### Not in part A
Minting a pass for any request, the open run, the open view, open tasks (theseus-a1wvq.6, .7, .8); `cn_profiles`,
`Ask.cleared`, `Profile.restricted`, the `trivial` list (theseus-a1wvq.4); compaction, consolidation, keep-warm and
replay falling back instead of failing (theseus-a1wvq.10); the footer and route-row fields (theseus-a1wvq.11).

## Part B, theseus-a1wvq.5: the session's data state (structure only)
Nothing opens a session open yet and nothing computes a run yet (later rows do): your tests build the state directly.
1. **`SessionRecord.data: Option<Box<crate::data::DataState>>`**, `#[serde(default, skip_serializing_if =
   "Option::is_none")]`, boxed as `routed` is. In `data.rs`:
   - `DataState { opened: Option<Opened>, run: Option<OpenRun>, taint: Option<Taint> }`;
   - `Opened { how, at_ms }` (`how`: `task` for an open task, `draft` for a China-origin coding draft; serde words);
   - `OpenRun { from: String, through: String }`: node ids of the first and last node of the current open run;
   - `Taint { why: DataWhy, node: Option<String>, at_ms }`: for an open session only, the first node that was not
     open. **Bookkeeping only** (the ledger, inspect, the benchmark): it never gates a request.
   Absent `data` reads as the private view with no open run: every old record, with no migration (fail closed).
2. **The chain of blessed writers** (the doc comment on `OpenRun`; later rows write it). The run extends over a node
   only when that node is: a person message Jev judged `open` at the act band; a Theseus reply Jev judged `open` at
   the act band (the owner's answer of 11:34; the question about replies is asked off the turn path by the
   open-request row); or an open request's tool result that passed the screen. Any other node ends it: a private or
   unclear message, a seal, an attachment, an unscreened tool result, `channel.read` text, a merge, a split, a join
   steer, a task's report, a wake. So a writer nobody thought of can never extend a run, and a plain turn writes
   nothing new (FAST).
3. **Store format.** Your base is 47 (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`): bump it by one,
   and never write the number as a literal anywhere but the constant and its tests. Add `SESSION_BEFORE_DATA` to
   `tests_layouts.rs` (a sample of the newest session layout before this field, as the file's rule says) with its
   round trip.
4. **The turn's write-back** (`take_turns_fields`) merges `data`, never overwrites: the turn's `run` is taken only
   when the stored `run` is still the one the turn read at its start (compare and set), else the stored one stays; a
   stored `taint` always wins; `opened` is never cleared by a turn.
5. **Tasks and forks.** A task never inherits its parent's run (a task's brief is its only start): the task's record
   starts with no `run`, whatever the parent's (`task.rs`, beside the `external::hold` call). A task's report never
   extends or changes the parent's run. A fork copies `opened` and `taint`, and the run cut at the fork point:
   `from..=fork point` when `from` is at or before it, else none (`fork.rs`).
6. **Ledger** `session.data` (a `LedgerKind` in `ledger.rs`, the fact in `fact/data.rs`): rows `opened { how }` and
   `tainted { why, node }`, with writer functions in `data.rs` (callers come with the later rows; keep them `pub` so
   they are not dead code). Reader: `rpc/inspect.rs` shows a session's data line (opened, the run's ends, the taint).
   No new line in `theseus-protocol/src/lib.rs` (three lines of room): a health count only if it fits without one.

### Tests, part B (the first is the key test)
- **`turn_write_back_never_erases_a_stored_taint_or_moves_a_changed_run`** (T11): a stored taint survives a turn that
  read none; a run another writer moved after the turn started is not replaced by the turn's stale one; an unchanged
  run takes the turn's. Plant: make `take_turns_fields` copy `data` plainly; both halves must fail.
- `a_task_never_inherits_its_parents_run` (T20, the structure half): a parent with an open run gets a task with no run;
  the task's report leaves the parent's run as it was.
- `fork_copies_data_state_through_the_fork_point` (T21): `from` before the fork point gives `from..=fork point`; `from`
  after it gives none; `opened` and `taint` copied.
- The layout sample and round trip (T30), `SESSION_BEFORE_DATA`.
- `session_data_ledger_rows_round_trip`, and the inspect line.

### Not in part B
Computing a run, judging replies, opening tasks, the open view (theseus-a1wvq.6, .7, .8). The ontology's private mark
(another row, theseus-p6zgq) will read and write this state later: do not build its mark.

## Contracts with other rows
- **Keep the names** in `data.rs` exactly as written above; later rows build on them and never rename them. If you
  find one wrong, keep it and say so in the report.
- **The routing-composition row** (theseus-a1wvq.4) will edit `route_step.rs` to fill `Profile.restricted` and
  `Ask.cleared` and own `routing.rs`'s `runs`. Do not add those; keep your `route_step.rs` change to `route_profiles`'s
  parameter, its restricted case and the callers.
- **`ProviderRequest` literals:** another row may add a field at the same time. Add `data_pass: None` the same way
  everywhere (one line per literal), so the join merges line by line.
- **The data rule binds this session too.** Your own model is Claude. No request of yours goes to a GLM or DeepSeek
  endpoint (there is no key on the VM; never add one), and every fixture is invented: made-up names such as "Acme",
  "Widgetco" and "Examplecorp", invented messages, no real person, company or project.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing, late or unparsable is restricted or closed, never open. Never
  loosen a check, a threshold or a ceiling to make a test pass.
- **On, measured, with a named rollback.** The floor ships on, with no switch: the rollback is reverting this row's
  part A commits, and the report names them. Part B changes no behaviour.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (a plain turn stays within `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and
  the gate's turn bench must not move), and on an unrestricted model's request nothing but the origin check before it
  leaves.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key tests** (named first under
  each part's tests) must be shown failing on their planted reverts. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. Its
  only findings may be the test sort's changes above (and the store format bump), each named in the report.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **The lane gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passes on your last commit (run the
  touched crates' tests with `cargo nextest run -p <crate>` first). Say what it printed.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Never write the owner's company or private project names
  (they live only in the operator's config). Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md` has the data rule's
  invariants: update its passage on the floor in the same commit). Keep CLOUD_REPORT.md honest: what you ran, what
  passed, what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on
  this VM; say which of your tests you could run and give the maintainer the exact commands for the rest).
- Out of scope: the owner's config, daemon and machine; any benchmark run; `docs/status.md`, the README, `docs/design/`
  and the spec (the maintainer writes those: say in the report what should change).

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its size
after the commit, it must not be empty), push, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
It holds, in this order:
1. Any file left over a line ceiling, with the reason (or "none").
2. What changed, part A then part B, with the commits; where part B stopped if it did.
3. Each test, and each plant with the test that failed on it.
4. The test sort: every changed test, its kind, why its meaning holds.
5. Every place a provider is built, and how it reaches `guard`.
6. The seam where the open-request row mints a pass, by file and function.
7. What could not run on the VM, with the exact commands for the maintainer.
8. The keel guard's output, verbatim.
9. The store format: from 47 to what, and the sample's name.
10. Follow-ups, each as one line a person could act on (for example: the `trivial` default list; compaction falling
    back instead of failing; anything in this row you would design differently).
