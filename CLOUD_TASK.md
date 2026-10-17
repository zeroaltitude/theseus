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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-cn-route-v6`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

---
## Your task: cn-route-v6, route.v6: Jev's data question, and the operator's names in Jev's state (theseus-a1wvq.2)

Branch: `cloud/20261017-cn-route-v6`. Every commit's subject carries `theseus-a1wvq.2`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:26, verbatim; two names redacted, because the public repo never carries them):** "DeepSeek is also anthropic messages API compatible. Please add the best flash (as of today, V4.1-Flash-1017) and non flash models to our supported and automatically selected models list. Also let's update jevs selection input. [The owner's company] and any private or personal or confidential information never goes to Chinese models. General questions and [an open project of the owner's], open source, openclaw, Theseus, etc can go to China. In addition, we need to carefully evaluate the glm and DeepSeek and give a more nuanced coding guidance that picks them where appropriate." Decisions taken since (the owner's questions Q1-Q4 of design §8): standing orders stay in the open view; no DeepSeek key exists yet, so nothing may need one (its live probe waits for the key); Q2 and Q3 are moot after the update below. Origin is judged by provider endpoint as well as by model. **The owner's update (2026-10-17 09:57, verbatim):** "I think that Chinese models in dms can be evaluated by content rather than by the nature of the channel. My understanding is also that they do have coding skills that are worth applying at their cost -- I'd always want to do final review and fixes with a more competent model." So clearance is per request (a China-origin request carries only the open run and the open view; the DM included), China-origin models only draft code, and a Claude model always reviews and fixes the draft before anything is reported or applied. The sticky session gate and `session.data_clear` are gone, and so are open places, open threads, `theseus --open` and the cockpit toggle.

**Prerequisite:** `main` (`c6f62abf` or later; store format 45). Wave 1: builds now.

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `c6f62abf` or later, with **store format 45**, route.v5 acting, and GLM wired as provider `zai`. Every `file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

## Your row (from the plan's table)
| # | Slug | Beads | Wave | Scope | Files | Tests (planted revert) | Depends on | Model | Size |
|---|---|---|---|---|---|---|---|---|---|
| 2 | `cn-route-v6` | `theseus-a1wvq.2`, child of `theseus-a1wvq` | 1 | route.v6 pack, its goldens and adoption; `Verdict.data`; `[routing.data]` with the operator's topic names, carried to Jev as `data_names`; nothing acts on the answer yet | `theseus-judge/packs/route.v6.toml`, `theseus-judge/src/pack.rs`, `builders.rs`, `fixtures/golden/`, `core judge/inbound.rs`, `routing.rs` (`Verdict`), `judge/ladder/adopt.rs`, `config/routing.rs`, the four string asserts | T26 (revert: change one word of v5's text in v6), T27, T28, `routing_data_topics_parse`, `the_verdict_reads_the_data_answer`; `inbound.state.json` byte-identical without names | `main` | Sonnet 5.5 | M |

## What to build
Design §4, as written. In short:
1. `crates/theseus-judge/packs/route.v6.toml`: route.v5's header and its four questions and both `[[rollback]]` rules
   **word for word**, plus the `data` question exactly as design §4.1 gives it (`decides = false`, `act = 0.90`,
   `confirm = 0.60`, `no_match = "unclear"`, the three options' words). `version = 6`, `jev_model = "jev-1.13.0"`.
   The header comment follows v5's pattern.
2. `pack.rs`: the `EMBEDDED` line (`pack.rs:48-77`); T26 beside `route_v5_is_v4_...` (`pack.rs:1589`); the
   description row (`pack.rs:1298-1307`) and the rules row (`pack.rs:1379-1386`); the pack count
   (`pack.rs:1419-1422`, 22 -> 23). The Discord row task-offer-chip adds `classify.v3` the same way today, so the
   joiner makes the count 24 when both land: say so in your commit.
3. `builders.rs`: `InboundInput.data_names: DataNames { open, private }` (`builders.rs:226`), skipped when both lists
   are empty, so `fixtures/golden/inbound.state.json` and `classify.v2`'s and `role.v1`'s requests stay byte-identical
   (`builders.rs:1079-1111`). A names fixture (made-up names: "Acme" private, "Widgetco" open) backs route.v6's golden
   `fixtures/golden/route.v6.request.json`, written with `UPDATE_GOLDEN=1` (`builders.rs:1116-1132`).
4. `judge/inbound.rs`: `ROUTE_PACK = "route.v6"` (`inbound.rs:61`); fill `data_names` from `[routing.data]
   open_topics` / `private_topics` where the state is built (`inbound.rs:627-700`); `verdict` (`inbound.rs:708-738`)
   reads `choice("data")` into `Verdict.data` with its band.
5. `routing.rs`: `DataAnswer` and `Verdict.data` (shared contracts; `Verdict` at `routing.rs:155-169`). Nothing acts on
   it yet: cn-open-request reads it. Every `Verdict { .. }` literal gains `data: None`.
6. `judge/ladder/adopt.rs`: `ADOPTED` gains route.v6 with `WHY_ROUTE_V6 = "the owner's decision of 2026-10-17"`
   (`adopt.rs:40-85`); v6 goes live at once, as v2 to v5 did.
7. `config/routing.rs`: the `[routing.data]` table (`RoutingData`, shared contracts) with `open_topics` and
   `private_topics`, default empty. cn-gate-floor and cn-open-view add their fields later.
8. The string asserts that name the pack move with it: `tests_inbound.rs:107`, `tests_continue.rs:493`,
   `tests_route_wait.rs:90-112`, `crates/theseusd/tests/route_owner.rs:110-117`.
9. **T28** `the_pack_text_names_no_organisation`: the test holds a deny list of SHA-256 digests (lowercased words and
   two-word spans of the pack text are hashed and checked), so the test itself names nobody. Write the mechanism with
   the digest of one made-up name ("acme-private") and a comment that the operator's names are added by digest; the
   maintainer adds the real digests at the join. Never write a real organisation's name or its digest.

The turn's request is unchanged: route.v6 adds about 150 tokens to Jev's own request only. (The owner's update judges
each request by its content, so this answer is read per message; nothing in this row changes for it.)

**Key test (planted revert):** T26: change one word of v5's `mode` text in v6 and see it fail.

## Shared contracts (all thirteen cn rows; keep the names exactly, so the joins dedupe cleanly)

The China-origin data gate is thirteen rows (Beads `theseus-a1wvq.1` to `.13`), joined to `main` in waves. Each row
owns the names in its line. A later row builds on them and never renames them. A row that needs a name owned by a row
not yet on your base stops and says so in the report, rather than inventing a second one.

**The rule since the owner's update (D1, D2):** clearance is **per request**. A request to a China-origin (or
unknown-origin) model carries only the **open run** (the newest stretch of the transcript in which every node is open,
ending at the newest person message, which was judged `open` at `act`) and the **open view** (no owner material). Its
`DataPass` covers that one request's digest. China-origin models act only as the `trivial` fallback behind Haiku and as
**coding drafts** in open tasks, and a Claude profile always reviews and fixes a draft before it is reported or
applied. There is no sticky session gate, no owner override, and no open place, open thread or `--open`.

| Name | Where | Owner row |
|---|---|---|
| `ModelOrigin { Us, Eu, Cn, Unknown }` (`#[default] Unknown`, serde snake_case, unknown words fail the load); `ModelOrigin::restricted(self) -> bool` (Cn or Unknown) | `crates/theseus-core/src/catalog.rs` | cn-catalog |
| `CatalogEntry.origin` (`#[serde(default)]`), `CatalogRow.origin: Option<ModelOrigin>`, `ThinkingMode::Implicit` | catalog.rs | cn-catalog |
| `catalog::origin_of(api_base: &str, vouched: Option<ModelOrigin>, model: &str, entry: Option<&CatalogEntry>) -> ModelOrigin` | catalog.rs | cn-catalog |
| `ProviderConfig.origin: Option<ModelOrigin>`; `Config::profile_origin(&self, profile: &str, catalog: &Catalog) -> ModelOrigin` | config.rs | cn-catalog |
| `[routing.data]` = `RoutingConfig.data: RoutingData` (`#[serde(default)]`, `deny_unknown_fields`) with `open_topics: Vec<String>`, `private_topics: Vec<String>` | `config/routing.rs` | cn-route-v6 |
| `RoutingData.enabled: bool` (default true), `RoutingData.fallback: Vec<String>` (default `["haiku", "sonnet"]`) | config/routing.rs | cn-gate-floor |
| `RoutingData.open_paths: Vec<String>` (default empty) | config/routing.rs | cn-open-view |
| `routing::DataAnswer { choice: String, confidence: f64, band: theseus_judge::band::Band }`; `Verdict.data: Option<Box<DataAnswer>>` | `routing.rs` | cn-route-v6 |
| `InboundInput.data_names: DataNames { open: Vec<String>, private: Vec<String> }` (skipped when empty) | `theseus-judge/src/builders.rs` | cn-route-v6 |
| `crate::data` (new `crates/theseus-core/src/data.rs`): `DataPass` (private fields and constructor; holds the digest of the **one** request it covers; `covers(&self, digest) -> bool`); `DataWhy` (snake_case words: `disabled`, `unchecked`, `private_view`, `not_in_run`, `unvetted`, `sealed`, `attachment`, `verdict_private`, `verdict_unclear`, `below_act`, `no_verdict`, `late`, `pack_not_live`, `tool_result`); `Clearance` (opaque: only `data.rs` builds a cleared one, bound to one request's digest; `Clearance::closed(DataWhy)`, `is_cleared()`, `why()`, `pass() -> Option<DataPass>`) | data.rs | cn-gate-floor |
| `ProviderRequest.data_pass: Option<DataPass>` (`#[serde(skip)]`); `ProviderError::DataGate { model }` (never retried, never sent); `Reason::DataGate` (`"data_gate"`); the `data_gate.blocked` row | provider.rs, routing.rs | cn-gate-floor |
| `Ask.cleared: bool` (an open request can be built for this ask), `Profile.restricted: bool`, `ModeProfiles.cn_profiles: Vec<String>`, `.cn_after: Option<String>`, `routing::effective(..)` | routing.rs, config/routing.rs | cn-routing-compose |
| `SessionRecord.data: Option<Box<DataState>>`; `DataState { opened: Option<Opened>, run: Option<OpenRun>, taint: Option<Taint> }`; `OpenRun { from: String, through: String }` (node ids); `Opened { how, at_ms }` (`how`: `task` or `draft`); `Taint { why: DataWhy, node: Option<String>, at_ms }` (bookkeeping only, never a gate); ledger kind `session.data` (`opened`, `tainted`) | session.rs, data.rs | cn-session-data |
| `TaskStartParams.open`, task.create's `open` | theseus-protocol `task_start.rs`, task.rs | cn-open-tasks |
| `PlaceView.open`, `PlaceView::content_class()`, recall's `Place::Open` | ceiling.rs, recall | cn-open-view |
| `data::admits(s: Option<&DataState>, c: &Check<'_>) -> Result<OpenRun, DataWhy>`, `data::Check`, `data::OpenRequest`, `data::clearance(run: &OpenRun, r: &OpenRequest) -> Clearance`, `turn/data_step.rs`, the detour's open compile | data.rs, turn/ | cn-open-request |
| `data::screen(..)` | data.rs | cn-tool-screen |
| `ModeProfiles.cn_draft: Option<String>`, `ModeProfiles.final_review: Vec<String>` (TOML key `final`); the draft task (`Opened.how = "draft"`) and its tree `<state>/drafts/<task>` on branch `draft/<task>`; the review as a check task (`check_of`); the `task.draft` row | config/routing.rs, new `draft.rs`, task.rs, check.rs | cn-draft-review |
| `TurnRoute.data: Option<TurnData { verdict, clearance, gate, origin }>`; `route.decided`'s `data`, `data_confidence`, `clearance`, `run_nodes`, `data_gate`, `provider`, `model`, `origin` | theseus-protocol `route.rs`, `fact/route.rs` | cn-observability |

The join order: cn-catalog + cn-route-v6 → cn-gate-floor + cn-routing-compose → cn-session-data → cn-open-tasks +
cn-open-view → cn-open-request → cn-tool-screen + cn-background-gate → cn-draft-review → cn-observability →
cn-config-guidance.

**Other rows in flight today** (the Discord tasks batch, branches `cloud/20261017-discord-*`, `-voice-tasks`,
`-task-offer-chip`): they own `crates/theseus-discord/src/runtime/{threads,start_task,follow,task_cmds}.rs`,
`dc/render/task_view.rs`, `dc/courier.rs`, `theseus-sim/src/fake_discord*`, core `turn/join_step.rs`, and
`classify.v3` (which adds an `EMBEDDED` pack line beside route.v6's). None of them bumps the store format: the gate's
cn-session-data takes 46. `crates/theseus-protocol/src/lib.rs` has 2 lines of room under its ceiling (2,731 of 2,733,
`scripts/long-files.txt`): put new wire types in their module files, and a new method constant on an existing line.

## This feature's rules (in addition to the rules at the end)
- **Fail closed, always.** Anything unknown, missing, late or unparsable is restricted or closed, never open. Never
  loosen a check, a threshold, a test or a ceiling to make a test pass.
- **No names.** The repository is public. Never write a person's name, the owner's employer's name, or the owner's
  private project names in code, tests, fixtures, docs or commit messages: say "the owner" and "the owner's company".
  Organisation and project names live only in the operator's config (`[routing.data] open_topics` /
  `private_topics`, which reach Jev as route.v6's `data_names`). Tests use made-up names such as "Acme" and
  "Widgetco".
- **The lane gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passes on your last commit (run the
  touched crates' tests with `cargo nextest run -p <crate>` first). Say in the report what it printed.
- **A planted revert for your key test** (named in your row): break the line the test guards, see that test fail with
  the expected message, restore. The report names the test, the line, the failure and the restore.
- **The token keel:** the first request stays <= 8,000 tokens for cli/task and <= 15,000 for the DM
  (`crates/theseus-core/src/first_request.rs:24`, `:26`; `tests_first_request.rs`). Jev's route pack is Jev's own
  request, not the turn's. A new field a model sees in a tool schema stays tiny (a description of 12 words at most),
  and the first-request tests pass unchanged.
- **The design's test names** (T1-T30, as the owner's update renames, narrows or retires them) are the names to
  use; your row lists the ones it builds.

## The owner's update (`design-delta-1017.md`, 2026-10-17 09:57; verbatim; it overrides the design)

### Design delta, 2026-10-17 10:0x: content, not channel; China models draft, Claude finishes

The owner's message, 2026-10-17 09:57 (verbatim): "I think that Chinese models in dms can be evaluated by content rather
than by the nature of the channel. My understanding is also that they do have coding skills that are worth applying at
their cost -- I'd always want to do final review and fixes with a more competent model."

This delta **overrides** design.md wherever the two disagree. Everything else in design.md stands:
- the catalog origin, judged by endpoint as well as by row;
- fail closed;
- the chokepoint `DataPass`;
- route.v6's `data` question;
- background calls obey the gate;
- observability.

#### D1. Judge the request, not the place or the whole session

- **Removed:** "the owner's DM session never reaches a China model" (design §0, §5 consequence, T18). Removed too: the
  requirement that a session be *opened* open before any China-origin model may see anything from it.
- **New rule:** a China-origin request may be built in **any** place, the DM included. It may contain only content
  judged open.
  - **Open-run history:** the history it carries is the run of most recent nodes up to the newest person message.
    Every person message in that run was judged `open` at ≥ act. The run ends at the first node that is not open: a
    private or unclear message, a seal, an attachment, an unscreened tool result, or a node written outside the
    blessed writers.
  - **Nothing older:** history before that point is not sent.
  - **Open view:** the prefix is the design's open view (§5): no owner context files, standing facts, preferences,
    people guidance or personal/private recall. Recall comes only from open-labelled sources.
  - **Standing orders stay in the open view** (Q1 default).
- **Clearance is per request.** The `DataPass` is minted for one compiled request whose every included node is
  open, and it covers that request's digest. The pass replaces session-level sticky clearance as the precondition.
- **Sticky session taint stays only as bookkeeping** for open sessions and tasks (R4a), not as a gate on the DM.
- **Consequence:** in the DM, a message judged open can be served by a China-origin model. That model sees only the
  open run and the open view. Private content earlier in the conversation is cut off, never sent.

#### D2. China models draft; a Claude model always does the final review and fixes

- **Where China-origin models act:** coding work only, as **drafts**: routine coding, test writing, mechanical
  refactors, and well-specified feature implementation from an agreed plan.
- **They never give the final answer on code.** Every China-origin coding draft is followed by a **mandatory final
  review-and-fix pass on a Claude profile** before the result is reported or accepted:
  - routine work: `sonnet`;
  - deep work: `sonnethi`, then `opus`.
  
  The reviewer reads the diff, runs the tests, fixes what is wrong, and says what it changed.
- **Mechanism: the task is the unit.** An eligible coding request becomes an **open task**:
  - **The brief.** It is built only from open content, and Jev's `data` question is asked about the brief itself.
    The task runs in the open view on the China-origin profile (`dsflash`).
  - **The report.** It goes to a **review step on Claude**, either a review turn in the parent or a follow-on review
    task in the private view. The owner sees the Claude-reviewed result with one line: "drafted by DeepSeek Flash,
    reviewed and fixed by Sonnet".
  - **Never applied unreviewed.** If the review step cannot run (no usable Claude profile, spend limit), the draft is
    not applied and the owner is asked.
- **Config:** `[routing.modes.<mode>] cn_draft = "dsflash"` plus `final = ["sonnet"]` (exact names are the writer's
  call), on routine_coding and deep_coding. deep_coding's `cn_draft` is **off by default**: unknown-cause debugging
  and design stay on Claude. The owner can turn it on.
- **Never on China-origin models:** sophisticated, review, and the conversational modes (chat, quick, trivial).
  - Haiku is cheaper for short turns.
  - **GLM-5.3 stays out of routing.** It costs more per coding task than Sonnet.
  - GLM-5.3 Flash stays only as the trivial fallback, under D1's per-request rule.

#### D3. Measure it

- Every draft and review records:
  - the draft's model and cost;
  - the reviewer's model and cost;
  - how much the reviewer changed (lines changed, tests fixed).
- The benchmark rerun (theseus-lgakz, then theseus-ji599) gets an arm: China-origin draft plus Claude review,
  against Claude alone, on the night set's routine coding tasks. Report solve rate, total cost and wall time.

#### Tests that change

| Test | Change |
|---|---|
| T18 | **Replaced** by `a_dm_open_message_reaches_dsflash_with_only_the_open_run`. The compiled request has no node from before the last private message and no owner material. |
| `a_private_message_in_the_dm_cuts_the_open_run` | **New.** The next open message's China-origin request starts after it. |
| `a_cn_coding_draft_is_never_reported_without_a_claude_review` | **New.** The review step's absence leaves the draft unapplied and asks the owner. |
| `draft_and_review_costs_are_recorded` | **New.** |
| T8 | **Narrowed** to open sessions' bookkeeping. |
| T17 | **Narrowed** to open sessions' bookkeeping. |

## The design (`design.md` of 2026-10-17, written against `c6f62abf`; excerpts verbatim; implement your row's part of it)
**Precedence:** "What to build" above is the latest decision; then the owner's update (the section above); then the design. Where a design excerpt disagrees with either (a session-level sticky gate, `session.data_clear`, the late grace, open places and threads, "the DM never reaches a China model", `cn_min_tokens`), the design excerpt is superseded.

### design.md §0 (lines 7-40)

## 0. The rule, and the design in one paragraph

The owner's rule (2026-10-17, verbatim): *"[the owner's company] and any private or personal or confidential information never goes
to Chinese models. General questions and [an open project of the owner's], open source, openclaw, Theseus, etc can go to China."* "Chinese
models" means models of Chinese **origin** (GLM from Zhipu/Z.ai, DeepSeek), whoever hosts them.

The design has four layers. Every one of them fails closed.

1. **The model's origin is a catalog fact.** Every model row carries `origin`. A model whose origin is `cn` or
   `unknown` is **restricted**.
2. **A restricted model only ever sees a cleared session.** Clearance is sticky and session-level. A session is
   cleared only when all of the following hold:
   - it was opened as an **open session**, so it is compiled under the **open view**, which carries no owner material
     by construction;
   - every person message in it was judged `open` by Jev at confidence ≥ `act`;
   - nothing unvetted entered its transcript;
   - it is not sealed;
   - its place is not marked private.
3. **Routing never picks a restricted profile for an uncleared session.** This covers every path: the mode lists,
   `cheapest`, the session's own base, a pin, a correction and a detour. When a restricted base is skipped, the turn
   moves to `[routing.data] fallback` and says `data_gate`.
4. **A deterministic backstop at the single HTTP chokepoint** (`Anthropic::stream_message_impl`). It refuses to send
   any request for a restricted model unless the request carries a `DataPass`. Only the clearance check can mint a
   `DataPass`. Every background call (summaries, consolidation, initiative, people, cards, the learning writer,
   replays, keep-warm) therefore obeys the same gate, including any call path nobody remembered.

Jev decides the content question. The structure (open view, place, seal, chain, chokepoint) makes sure Jev is never
the only guard, as `AGENTS.md` requires ("Jev is never the only guard on safety").

**Consequence for the owner's DM, stated plainly.** The DM's own session is compiled under the private view: USER-like
context files, standing facts and preferences, people guidance, and live recall of the owner's personal and work
history. Its content is never sent to a China-origin model. From the DM, China models are reachable only through an
**open thread or open task** whose context starts clean (a brief only), and only while every message in it stays
`open`.

### design.md §1.3 (lines 93-118)

### 1.3 Jev and the route pack

- The acting pack is `ROUTE_PACK = "route.v5"` (`judge/inbound.rs:61`). Its questions are `mode` and `reply_effort`
  (both `decides = true`) and `persona` and `seal` (`decides = false`) (`crates/theseus-judge/packs/route.v5.toml`).
- A live pack's options never change in place, so a new question means route.v6 (`route.v5.toml:12-13`;
  `pack.rs:1-4`).
- `decides` means "the pack's live action reads it" and it gates promotion through 200 labels
  (`theseus-judge/src/pack.rs:241-242`, `learn.rs:196-226`). Persona and seal are acted on at `act` while
  `decides = false`, which is the precedent for a question that acts without holding promotion.
- The verdict is read only from an actionable judgment, one answered by the pinned model with no drift
  (`judge/inbound.rs:708-738`, `theseus-judge/src/judge.rs:238-240`). Bands are Act / Confirm / Escalate
  (`theseus-judge/src/band.rs:25-29`).
- **The state Jev sees** (`judge/inbound.rs:627-700`, `InboundInput` at `theseus-judge/src/builders.rs:226`): the new
  message, the previous human message, the last reply, the place kind, live task briefs, live threads, personas and
  the thread's turns. Attachments are not seen.
- **Jev's own model** is `jev-1.13.0` from TypeSafe (`catalog.rs:808-811`; `judge_prices` `provider: "typesafe"`).
  It is called only through `theseus_judge::client::Client::call` → `POST https://api.typesafe.ai/v1/systemone`
  (`theseus-judge/src/client.rs:21-22`, `:1015`, `:1086`). It is not a China-origin model, and Jev's states go nowhere
  else, with two exceptions:
  - the learning writer (`[judge.learn] writer_profile`, default `opus`, `learning/propose.rs:681-739`);
  - the audit (`learning/audit.rs:239`, `:344`).

  §6 closes both.
- Packs are embedded with `include_str!` in `pack.rs:48-77`. The golden is `each_pack_asks_its_golden_request`
  (`builders.rs:1116-1132`), comparing `crates/theseus-judge/fixtures/golden/route.v5.request.json`. Adoption is
  `ADOPTED` in `judge/ladder/adopt.rs:40-85` (`WHY_ROUTE_V5 = "decision of 2026-10-13"`).

### design.md §4 (lines 392-477)

## 4. Requirement 3: Jev's selection input, route.v6

### 4.1 The pack

`crates/theseus-judge/packs/route.v6.toml` contains route.v5's header fields, and its four questions **word for word**
(`mode`, `reply_effort`, `persona`, `seal`, and both `[[rollback]]` rules), plus one question. The header comment
follows v5's pattern. `version = 6`, `jev_model = "jev-1.13.0"`, and the description gains: "…, and whether what the
message carries may go to a model provider outside the owner's trust (open or private)."

```toml
# v6's own (theseus-a1wvq). Asked of every message. The owner's data rule: private,
# personal or confidential matter never goes to a China-origin model; general
# questions and open-source work may. The core lets a restricted model see a
# session only while every message in it is `open` at `act`: anything else, or
# no answer, closes it (fail closed). It decides nothing for the pack's
# promotion, and acts from the day the pack does: a safety gate does not wait
# for a holdout. Project and organisation names live in the operator's config
# (the state's `data_names`), never in this file.
[questions.data]
type = "choice"
decides = false
instructions = "May what the person's new message carries be shared with an outside model provider that must never see private matter?"
act = 0.90
confirm = 0.60
no_match = "unclear"
options = [
  { id = "open", means = "Yes: only general knowledge or public matter, such as a general question, or work on open-source software or on a project the state lists as open, with nothing personal, private, confidential or secret in it." },
  { id = "private", means = "No: it carries or refers to something private: work for a company or organisation (its plans, people, customers, partners, code or data) other than public open-source work or a project the state lists as open; anything the state lists as private; personal, family, health, money or legal matters; anything confidential, under embargo or not yet public, such as an unfixed vulnerability; another person's data; or a credential, key, token or password." },
  { id = "unclear", means = "The message gives too little to tell, or it mixes open and private matter." },
]
```

**The owner's words map to the options:**

- "[the owner's company]" and "work" → *work for a company or organisation*, plus the private topic list.
- "personal" → *personal, family, health, money or legal*.
- "private" and "confidential" → *confidential, under embargo or not yet public*.
- "partner data" → *another person's data* and *partners*.
- "credentials" → *credential, key, token or password*.
- "general questions" → *general knowledge*.
- "open source, OpenClaw, Theseus, [an open project of the owner's]" → *open-source software* and *a project the state lists as open*.

**No organisation or project name appears in the public repository.** `git grep -il [the owner's company]` and `[an open project of the owner's]` both find
nothing today, and the employer's name must not land in a public file. The names travel in the state instead.
`InboundInput` (`builders.rs:226`) gains:

```rust
#[serde(default, skip_serializing_if = "DataNames::is_empty")]
pub data_names: DataNames,   // { open: Vec<String>, private: Vec<String> }
```

- It is filled from `[routing.data] open_topics` / `private_topics` in the operator's config. The live config would
  list "OpenClaw", "Theseus" and "[an open project of the owner's]" as open, and the employer as private.
- When both lists are empty it serializes to nothing. The shared inbound state, and so `classify.v2`'s and
  `role.v1`'s requests and `fixtures/golden/inbound.state.json` (`builders.rs:1079-1111`), stay byte-identical
  without names.
- A new input fixture with names backs route.v6's golden.

### 4.2 Gating from day one

- **The question acts from the day the pack does.** Its `decides = false` only keeps it from holding up `mode`'s
  promotion, as with persona and seal.
- **route.v6 goes live at once**, as v2 to v5 did. Its `ADOPTED` entry is `WHY_ROUTE_V6` (`adopt.rs:40-85`), "the
  owner's decision of 2026-10-17".
- **The gate reads only `open` in band `Act`.** Everything else fails closed, as §3.4 lists.
- In shadow (a pin, or `[routing] mode = "shadow"`) the answer is still asked and recorded, and it still **taints**,
  since narrowing is always safe. It never **permits**.
- **The threshold rule.** The data question's `act` may be raised by a later pack version, never lowered below the
  route pack's own `act` without the owner's `Keel:` ack. The keel guard already treats a loosened threshold as
  keel-file (`AGENTS.md`, "The gate before every commit").

### 4.3 Jev's model, and Jev's state

- Jev runs on TypeSafe's `jev-1.13.0` (§1.3), which is not of Chinese origin. route.v6 adds one question to Jev's
  **own** request: about 150 tokens at $0.042 a million (`catalog.rs:825-845`). **The turn's request is unchanged**, so
  the token keel (first request ≤ 8k cli/task, ≤ 15k DM) is untouched by the pack.
- **Jev's state never goes to a restricted model.** Its only readers besides TypeSafe are the learning writer and the
  audit. §6.4's config rule refuses a restricted profile for both, and the chokepoint refuses any call without a pass.
  Neither of them can mint a pass, because a pass needs a cleared session and Jev's states are cross-session.

### 4.4 `Verdict`

`Verdict` (`routing.rs:155-169`) gains `pub data: Option<DataAnswer>`, read in `judge::inbound::verdict`
(`inbound.rs:708-738`) by the same `choice("data")` helper. `ROUTE_PACK = "route.v6"` (`inbound.rs:61`). The string
asserts named in the pack checklist move with it: `tests_inbound.rs:107`, `tests_continue.rs:493`,
`tests_route_wait.rs:90-112` and `theseusd/tests/route_owner.rs:110-117`.

### design.md §6.2 (lines 607-628)

### 6.2 `[routing.data]`

```toml
[routing.data]
# The owner's data rule (theseus-a1wvq): a model of cn or unknown origin sees only a
# cleared open session. Off: no restricted model is ever usable (the gate never opens;
# it is not "no gate").
enabled = true
# Where a turn goes when the gate skips a restricted profile it would have run on
# (the session's base, a pin, a hold): the first usable, within the place's cap.
# Every entry must be an unrestricted profile (checked at load).
fallback = ["haiku", "sonnet"]
# Trees an open session's file tools and proc.run may reach; a result from elsewhere
# never reaches a restricted model. Default none.
open_paths = []
# Names Jev reads with the message (route.v6's `data`): operator config only.
open_topics = []
private_topics = []
```

`[routing.modes.<mode>]` (`ModeProfiles`, `config/routing.rs:132-141`) gains
`cn_profiles: Vec<String>` (default empty in code). **It applies only when the turn is cleared.**

### design.md §9 (lines 748-837)

## 9. Requirement 7: tests

Unit tests are pure (`data.rs`, `routing.rs`, `catalog.rs`). Core tests use the fake providers in `tests_route.rs`.
The daemon test runs on a scratch daemon.

**Catalog and config**

- **T1** `every_builtin_row_names_its_origin_and_glm_and_deepseek_are_cn` (`catalog.rs` tests).
- **T2** `a_config_model_without_origin_is_unknown_and_restricted`.
- **T3** `a_config_table_cannot_loosen_a_builtin_cn_origin`: validate error. Tightening is accepted.
- **T4** `deepseek_rows_are_priced_as_research_says`: prices, window, `pdf=false`, `thinking`, `effort` (`<price>`).
- **T5** `routing_data_parses_and_fallback_must_be_unrestricted`; `cn_profiles_parse_per_mode`; the template still
  parses (`example_template_uncommented_still_parses`).
- **T6** `background_profiles_refuse_a_restricted_profile`, one case per key in §6.4.

**Clearance (pure, `data.rs`)**

- **T7** `fail_closed_matrix`, one assertion per `DataWhy`: no state; taint; place private; chain broken; sealed;
  seal pending; seal yes at confirm; attachment; private; unclear; below act; no verdict; pack not live. Only
  open + Act + all structure passes.
- **T8** `taint_is_sticky`: after a private message, a run of open messages never clears; only `data_clear` does.
- **T9** `late_grace_applies_once`: a late open clears the previous message; a late private taints; no late verdict
  taints with `no_verdict`.
- **T10** `an_unknown_writer_breaks_the_chain`: a node written outside the blessed writers (a merge, a channel.read,
  a join steer) leaves the next turn `unvetted` and tainted.
- **T11** `turn_write_back_never_erases_a_stored_taint` (`take_turns_fields`).

**Routing**

- **T12** `a_restricted_profile_runs_only_when_cleared`: pick, cheapest, dear guard, step-down.
- **T13** `cn_profiles_come_first_only_when_cleared_and_obey_the_cap`.
- **T14** `a_restricted_base_moves_to_the_fallback_with_data_gate`: no verdict, late, unsure, shadow, pinned `-P glm`,
  a correction to glm, a hold to glm.
- **T15** `no_usable_fallback_fails_the_turn_and_sends_nothing`: the fake provider counts zero requests.
- **T16** `a_detour_to_a_cn_profile_needs_clearance`.

**Turn (core, fake providers)**

- **T17** `an_open_session_reaches_glm_and_a_private_message_moves_it_back`: two turns, `route.decided` rows, a
  `session.data` taint row.
- **T18** `the_dm_session_never_reaches_a_cn_profile`: a private-view session with every message judged open.
- **T19** `a_private_tool_result_stops_a_cleared_turn`: a scrubber hit, a path outside `open_paths`. The result node
  is written, the next loop is not sent, and the session is tainted.
- **T20** `task_inheritance`:
  - a cleared parent's task is cleared;
  - a tainted parent's task is tainted;
  - an open task from a private parent carries no arrangement and needs its brief judged open;
  - a tainted task's report taints its parent.
- **T21** `fork_copies_data_state_through_the_fork_point`.
- **T22** `open_view_request_carries_no_owner_material`:
  - a private context file, a standing fact and a personal imported episode are all absent from the compiled
    request;
  - a public-labelled episode is recallable;
  - the first request is ≤ 8k tokens (cli/task) and ≤ 15k (thread).
- **T23** `summaries_obey_the_gate`:
  - `session` on a cleared cn turn carries a pass;
  - a named glm `summary_profile` in an uncleared session goes to fallback;
  - consolidation with one uncleared source goes to fallback.

**Chokepoint**

- **T24** `the_provider_refuses_a_restricted_model_without_a_pass`: a fake HTTP server records zero requests; the
  error class is `DataGate`; a `data_gate.blocked` row is written.
- **T25** `data_pass_never_changes_the_request_digest`.

**Pack**

- **T26** `route_v6_is_v5_word_for_word_and_a_data_question` (beside `route_v5_is_v4_…`, `pack.rs:1589`). It also
  covers the description row (`pack.rs:1298-1307`) and the rules row (`pack.rs:1379-1386`).
- **T27** The golden `route.v6.request.json` via `UPDATE_GOLDEN=1` (`builders.rs:1116-1132`). `inbound.state.json` is
  unchanged with empty names; a names fixture backs the v6 golden.
- **T28** `the_pack_text_names_no_organisation`: route.v6.toml contains no name from a deny list kept in the test as
  a hash, so the test itself names nobody.

**Telemetry**

- **T29** `route_decided_records_data_clearance_gate_provider_model_origin`. Plus the `TurnRoute` round trip; the
  footer strings (Discord `routed`, CLI `status_line`); the own-bench report columns.

**Layouts**

- **T30** `SESSION_BEFORE_DATA` sample and round trip (`tests_layouts.rs`).

**Live check, on a scratch daemon of the step's build** (`AGENTS.md`, "A live check on a scratch daemon")

- An open thread with an open message routes `trivial` to glm (the z.ai key exists).
- A private message in it taints, and the next turn runs on haiku.
- The DM session never shows glm.
- Then, with the owner's DeepSeek key: one DeepSeek turn, its served model and token counts read from the provider
  (Item 26's rule).

### design.md §10 (lines 841-872)

## 10. Requirement 8: implementation rows

Each row is sized for one Sonnet 5.5 coder, in a lane or in the cloud. Rows that touch shared files (a protocol type,
a dispatch `match`, the CLI's `main.rs`, `cockpit/src/protocol.ts`) say so and land at a join. Every row runs the lane
gate (`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`), proves its fix against a planted revert, and keeps the token keel.

| Row | Title | Depends | Files | Acceptance |
|---|---|---|---|---|
| **R1** | Catalog origin | — | `catalog.rs` (`ModelOrigin`, `CatalogEntry.origin`, `CatalogRow.origin`, `restricted`), `config.rs` validate (no loosening a built-in cn row), template's `[catalog]` example | T1-T3; `BUILTIN_VERSION` bumped; every built-in row's origin asserted |
| **R2** | DeepSeek provider plumbing | R1; research.md | `catalog.rs` (`fn deepseek`, rows, `TokenRates::of` branch), template (`[providers.deepseek]`, secret, `dsflash`/`ds`), `broker.rs` test, `fixtures/owner_shape.toml` | T4; template parses; a live probe once the owner's key exists (served model, token counts, PDF/vision/thinking behaviour recorded in the catalog's `source`) |
| **R3** | Fail-closed foundation | R1 | new `data.rs` (`DataPass`, `DataWhy`, `restricted`), `provider.rs` (`data_pass` `#[serde(skip)]`, the check in `stream_message_impl`, `ProviderError::DataGate`), `rpc/mod.rs` (restriction fn at build), `route_step.rs` (`route_profiles` marks restricted `unusable = "data"` **unconditionally**), the base gate + `Reason::DataGate` + `[routing.data] enabled/fallback` parse, `Config::validate` for §6.4's keys | T6, T14 (uncleared ⇒ always), T15, T24, T25. **After R3, no request ever reaches a China-origin model: the safe interim state.** |
| **R4a** | Session data state | R3 | `session.rs` (`data`, write-back merge), `store.rs` format bump, `tests_layouts.rs` sample, `data.rs` (`DataState`, `Taint`), `task.rs` propagation beside `external::hold`, report write (`turn.rs:2150-2158`), fork, new `LedgerKind` `session.data` + inspect line | T11, T20 (structure half), T21, T30 |
| **R4b** | Opening open sessions (join: protocol, CLI `main.rs`, Discord verbs) | R4a | `PlaceCeiling.data` (protocol + bindings parse + health), `ThreadNewParams.open`, `TaskStartParams.open`, `task.create` `open`, CLI `--open`, cockpit toggle (generated TS), Discord `/thread open` | Each opening writes `opened`; `data = "private"` refuses an open session in words; protocol TS regenerated |
| **R5** | The open view | R4a | `ceiling.rs` (`PlaceView.open`, `content_class`), every content reader (`turn.rs:797`, `standing.rs`, `ontology.rs`, `session_block.rs`, `prefix.rs` key, `compiler/situation.rs`), recall `Place::Open` (`theseus-memory/src/recall.rs`, core `recall.rs:1215`), tool offer and refusal with `open_paths` + `proc.run` cwd rule (`places.rs:496`, `:593`, `toolrun/order.rs:81`), `[routing.data] open_paths` | T22; goldens of a private session's request byte-identical (a refactor golden first) |
| **R6** | route.v6 and the data answer | — (parallel with R3-R5) | `packs/route.v6.toml`, `pack.rs` EMBEDDED + tests, golden + names fixture, `builders.rs` `InboundInput.data_names`, `judge/inbound.rs` (`ROUTE_PACK`, `verdict` reads `data`), `routing.rs` `Verdict.data`, `adopt.rs` `WHY_ROUTE_V6`, `[routing.data] open_topics/private_topics`, the named string asserts | T26-T28; `inbound.state.json` unchanged without names |
| **R7a** | Clearance in the turn | R4a, R5, R6 | new `turn/data_step.rs` (compute at `compile_first` and the no-wait path, the late grace via `take_late`, the seal inputs, attachments, write `cleared_through`/`taint`), `call_model` mints the pass, `route_profiles(cleared)`, `session.data_clear` (owner, private place) | T7-T10, T17, T18; T14 now for cleared vs. uncleared |
| **R7b** | Routing composition | R3 (pure; parallel with R4-R7a) | `routing.rs` (`Ask.cleared`, `Profile.restricted`, `runs`), `config/routing.rs` (`ModeProfiles.cn_profiles`, the effective list, `glm` out of the code's `trivial` default), detours | T12, T13, T16; existing routing tests unchanged with `cleared = false` and no cn profiles |
| **R7c** | Tool-result screen | R7a | `data::screen` (scrubber + paths), the loop boundary in the turn (stop before the next loop, taint, the line) | T19 |
| **R8** | Background calls obey the gate | R4a, R7a | `turn/compaction.rs`, `consolidate/run.rs`, `keep_warm.rs`, `own_bench/replay.rs`, `learning/audit.rs`, `initiative/speak.rs` | T23; each call path's blocked case writes a row |
| **R9** | Observability (join: protocol) | R7a, R7b | `fact/route.rs`, `route_step.rs` `record_route`, `theseus-protocol/src/route.rs` `TurnData`, Discord `routed`, CLI `status_line`, `provider.call` origin, metric, `own_bench/sampler.rs` + `report.rs` columns | T29; the theseus-lgakz report reads origin/data per turn |
| **R10** | Config, coding guidance, docs | all; research.md | template `[routing.data]` + `cn_profiles` (research's guidance, commented), a proposed live-config diff for the owner and maintainer (`/etc/theseus/theseus.toml`: `[providers.deepseek]`, profiles, `[routing.data]` names and `open_paths`, `cn_profiles`, `glm` back via `trivial.cn_profiles`), spec Part III item, `docs/status.md`, `crates/theseus-core/AGENTS.md` (the data gate in "Invariants" and "Traps") | Template parses; the live diff reviewed by the owner before install |

**The order:**

```
R1 → R3 → R4a → {R5, R4b} → R7a → {R7c, R8} → R9 → R10
       ↘ R7b ─────────────↗
R6 (any time before R7a)        R2 after R1 (+ research.md; its live probe after the owner's key)
```

R3 is the first row that changes behaviour, and it makes the system safe: China-origin models become unreachable
everywhere, visibly (`data_gate` in the footer and rows). The rest open the door only for cleared sessions.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
