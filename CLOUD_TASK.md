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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-effort`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-effort, an effort change stops rewriting the conversation cache: keep-warm replays the last request's effort, effort is sticky on models without per-message effort, and per-message effort becomes a catalog field (theseus-rwpbg.4)

Branch: `cloud/20261017-trim-effort`. Every commit's subject carries `theseus-rwpbg.4`. Deadline for the report: 3 hours after you start.

**Source:** the trim review's part 3, M1 and M2. **The owner approved (Discord DM, 2026-10-17 16:17):** "Agreed" to:
probe per-message effort on Sonnet 5.5 and Opus 5.5; where a model lacks it, make Jev's effort sticky (keep the previous
request's level unless the cached message prefix is under `keep_warm_min_tokens` or Jev asks for a change of two levels
or more); keep-warm uses the last effort. The maintainer is running that probe on the owner's machine now; its result
sets which models take per-message effort, and this row must make either outcome **one catalog edit**.

**Prerequisite:** `main`. Builds now; joins 5th of the eight trim rows.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Size |
|---|---|---|---|---|---|---|
| 4 | `trim-effort` | theseus-rwpbg.4 | (a) keep-warm replays the effort the last request carried; (b) sticky effort on models without per-message effort; (c) `per_message_effort` is a catalog field (`CatalogEntry`, `CatalogRow`, config override) that `effort::default_of` reads | `crates/theseus-core/src/keep_warm.rs` (one field/fn + call), `turn/route_step.rs:366-383`, `compiler/effort.rs:50-57`, `catalog.rs` (one field, `fn claude`/rows, `over`), a new `turn/effort_step.rs` or `compiler/effort.rs` helper for the sticky rule | `keep_warm_replays_the_last_effort` (revert: profile's effort), `sticky_effort_holds_a_one_level_change_on_a_long_conversation` (revert: always apply), `sticky_effort_applies_under_the_prefix_floor`, `sticky_effort_applies_a_two_level_change`, `the_route_records_asked_and_sent_effort`, `per_message_effort_is_catalog_data` (a catalog row override flips Sonnet 5.5 to per-message with no other change; revert: hard-coded match) | M |

## What exists (at `c6f62abf`)
- `turn/route_step.rs:366-383`: when route's effort applies and the model does **not** take per-message effort, the
  else-branch (376-379) rewrites the top-level `output_config.effort`. `compiler/effort.rs:1-8` documents the cost: the
  messages cache restarts, only tools + system stay cached (one flip on a 625k Fable conversation cost $7.68). The
  per-message fix (theseus-o719) is enabled only for `claude-fable-5-1` (`compiler/effort.rs:52-57`, `default_of`; line
  50-51 says Sonnet 5.5 and Opus 5 are "unproven"). The owner's cli, task and DM run `claude-sonnet-5-5`, so every turn
  whose Jev level differs from the previous request's pays a full 1-hour rewrite (~$0.57 per flip on a 150k Sonnet
  conversation, ~$1.50 at 400k).
- `keep_warm.rs:369-370` builds the read from `spec_of(&target, …)`, whose effort is the profile's (`turn.rs:813`); the
  compiler writes it to `output_config` (`compiler.rs:985`). There is no `effort` in `keep_warm*.rs`. After a turn that
  ran at Jev's level, the keep-warm read carries a different effort: a full rewrite at 2x, and ~20 reads then keep alive
  an entry the next real turn may not hit.

## What to build
**(a) M2, plain fix.** Keep-warm sends exactly the effort the last real request of that entry carried. Record it where
keep-warm already records the entry's last request (the keep-warm record / its target), and use it when building the
read instead of the profile's. If no effort was recorded (an old entry), keep today's behaviour. Keep-warm exists only to
reproduce the last prefix byte for byte; this makes it do so. Contract 3: one field and one call in `keep_warm.rs`; the
window cap is trim-tokens'.

**(b) M1, sticky effort** for a model whose catalog row says `per_message_effort = false`:
- The level **sent** is the previous request's level in this session (the level the last provider call carried), unless
  (i) the cached message prefix of this request is under `[cache] keep_warm_min_tokens` (20,000 by default: a short
  conversation has little cache to lose), or (ii) Jev's asked level differs from the previous level by **two steps or
  more** on the effort ladder (low < medium < high < xhigh < max), or (iii) there is no previous level (the session's
  first request, or a model/profile switch, which rewrites anyway). Then the asked level is sent.
- Measure "cached message prefix" with what the compile already knows (the conversation breakpoint's token count, or the
  last call's `cache_read + cache_creation` for this session): say which and why. Do not add a token count pass.
- **Recorded:** the route decision keeps both levels: the level Jev asked and the level sent, plus why they differ
  (`sticky`). Add the sent level beside the asked one where `route.decided` (or the turn's route record) already carries
  effort; if that is a wire type, add it in the module file that holds it (`theseus-protocol/src/route.rs` or similar),
  **not** in protocol `lib.rs` (2,732 of 2,733), and regenerate the TypeScript.
- Effort bounds still apply (the owner's `effort_bounds` and the 0.70 evidence bar for xhigh/max act on the asked level
  before this rule).
- Models with `per_message_effort = true` are unchanged (the per-message path).

**(c) Per-message effort is catalog data.** Add `per_message_effort: bool` to `CatalogEntry` and `CatalogRow`
(`catalog.rs`; `over` merges a config row's value over the built-in) with serde default false, set true for
`claude-fable-5-1` only (today's behaviour), and have `effort::default_of` (and any other place that decides "this model
takes per-message effort") read the catalog instead of the hard-coded `match`. Bump `BUILTIN_VERSION` as the catalog's
rule says. Then enabling Sonnet 5.5 or Opus 5.5 is one built-in row edit (or one config `[[catalog]]` override on the
owner's machine) and no code. The per-message mechanism's level (`default_of` returns `Some(Effort::High)` for Fable):
keep that value per model as a catalog field too if it is per model, or keep it in code if it is the same for all; say
which.
- **The probe:** the maintainer's live probe (whether Sonnet 5.5 and Opus 5.5 accept per-message effort and keep their
  cache) reports separately. Ship with Fable alone true. In the report, give the exact one-line edit for each outcome
  (Sonnet 5.5 yes / Opus 5.5 yes) so the joiner applies it at the join.
- Contract 4 and 5: this is the only catalog field any trim row adds; do not use `without_messages` (trim-utils deletes
  it). The China-origin rows touched `catalog.rs` today (`ModelOrigin`, DeepSeek and GLM rows): add your field beside
  theirs without moving anything.

## Tests
The six named above, on the existing route/effort test rigs (find the tests for theseus-o719 and the route's effort
tests, and the keep-warm tests; put new ones in a `tests_effort_sticky.rs` or the existing effort test module). For (b),
a scripted conversation: a long session (prefix ≥ 20k) where Jev asks medium then high → high is held at medium
(`sticky`), and the request's top-level effort is unchanged so the messages cache is read, not rewritten (assert on the
fake model's recorded bodies or the compile's cache layout); a short session applies the change; medium → xhigh (two
steps) applies. Plant a revert for each named test.

## Proof nothing is lost (report section)
The answer still runs at most one step from Jev's pick, only on a long conversation; the route record shows the asked
and the sent level; models with per-message effort are untouched; keep-warm's body equals the last request's body (a
test compares them).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `432bbbeb` (2026-10-17
15:53 MST) or later, with **store format 45**. The trim review that this row comes from read `c6f62abf`, so every
`file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

Line ceilings at `432bbbeb` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` **2,732 /
2,733**, `crates/theseus-kernel/src/kernel.rs` **3,026 / 3,030**, `crates/theseus-discord/src/runtime.rs` 3,495 / 3,500,
`crates/theseus/src/render.rs` 3,116 / 3,128, `crates/theseus-core/src/turn.rs` 3,351 / 3,525,
`crates/theseus-core/src/tests_m3.rs` 7,996 / 8,050, `crates/theseus-discord/src/render.rs` 2,926 / 3,009,
`crates/theseus-index/src/vectors.rs` 2,564 / 2,650. A trim should shrink these files, never grow them; never raise a
ceiling in this row.

## The trim rows (eight siblings, built in parallel today from main)

The owner asked for a trim pass before the v1 feature review: "an efficiency and simplicity and elegance check ... what
we can trim without losing anything, both on total code, total binary size, etc." The review found the items; the owner
settled the ones that needed him. Eight cloud rows build them at once, each from `main`, and one joiner merges them in
this order:

| Join | Slug | Beads | Area |
|---|---|---|---|
| 1 | `trim-binary` | theseus-rwpbg.5 | scripts/build.sh, theseusd's web.rs, the cockpit's build, theseus-aws-catalog's Cargo.toml |
| 2 | `trim-docs` | theseus-rwpbg.8 | docs/glossary.md, docs/cli.md (new), the CLI's clap `about` lines, the CLI help goldens |
| 3 | `trim-security-v1` | theseus-rwpbg.6 | core judge/gate.rs, judge/notice.rs, judge/mod.rs, the security packs |
| 4 | `trim-config-inherit` | theseus-rwpbg.7 | core config.rs, config/cache.rs (`inherit`), config/sparse.rs, the template |
| 5 | `trim-effort` | theseus-rwpbg.4 | core turn/route_step.rs, compiler/effort.rs, keep_warm.rs (effort), catalog.rs (one field) |
| 6 | `trim-tokens` | theseus-rwpbg.3 | core tools_defer/, toolrun.rs's tools note, keep_warm (the window cap), turn/compile_step.rs, theseus-index tender |
| 7 | `trim-utils` | theseus-rwpbg.2 | many small sites: clip, dates, now_ms, dead pub items, protocol jsonrpc, core<->protocol mirrors |
| 8 | `trim-tests-dedup` | theseus-rwpbg.1 | theseusd/tests/ and theseus-core test helpers |

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need to touch, and do not reformat files you only read.
2. **`config/cache.rs`'s `inherit` belongs to trim-config-inherit.** trim-tokens' keep-warm window cap and trim-effort's
   keep-warm effort live in `keep_warm.rs` / the catalog, never in `inherit` or `ProfileConfig`.
3. **`keep_warm.rs`:** trim-effort adds the last request's effort to what keep-warm replays; trim-tokens adds the
   break-even cap on the window. Each adds its own small function and one call; neither restructures the file.
4. **`catalog.rs`:** trim-effort adds one field (`per_message_effort`) to `CatalogEntry` and `CatalogRow`; trim-tokens
   reads the existing cache prices for its cap and adds no field. trim-utils deletes no catalog item.
5. **`compiler/effort.rs`:** trim-utils deletes the dead `without_messages` (`effort.rs:80`); trim-effort must not
   start calling it.
6. **Test helpers:** rows 1-7 add tests in their own `tests_<subject>.rs` files (or a crate's existing test module for
   their subject) and may use today's helpers; trim-tests-dedup moves helpers only in files that exist on `main`.
7. **Leave `crates/theseus-discord/` and `crates/theseus/src/herdr*` alone** (the Discord rows theseus-bh3t.1-.6 and
   theseus-ss36w are joining to main right now, and a herdr lane is open). Discord's copies of `clip` and the date
   math wait for a later pass; say in the report which copies you left.
8. **The China-origin rows** (theseus-a1wvq.3-.13) build later on `catalog.rs`, `config.rs`, routing and the turn path:
   keep your edits there small and additive.

## Out of scope for every trim row (owner questions still open)

CLI command hiding, renames or folds; vocabulary renames (thread, mode, place, surface, section names); the sealed
holdout set; any deletion of a test (the review's part 1 §2b test-only APIs, §3 the old Jev packs, §4 jev-probe, §7 the
serving-daemon chain, §9 the startup migration and retired keys, §19 RPC removals); `PLAN_NOTE` and the plan tools; the
LTO profile. If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. This is a trim: the diff should remove more than it
  adds, and every line you add should earn its place.
- **Nothing lost.** Behaviour, wire JSON, stored records and user-visible output stay the same unless this task says
  otherwise. Where it says otherwise, pin the old output in a test first, then change it, and show the diff.
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore); list them in
  the report. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names a keel finding (only trim-security-v1 may have one), and then the report gives
  the exact `Keel:` trailer line the guard asks for, for the joiner's signed commit.
- No store format change. No new dependency: Cargo.lock and the package-lock.json files gain no package (a version
  that drops a duplicate is fine; say so). Never loosen a test, budget or ceiling.
- No person's name anywhere in the repo, in commit messages, or in the report: write "the owner". If you edit a file
  that already carries a person's name, replace it with "the owner" in the lines you touch.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud. Add a section **"Proof nothing was lost"** listing, per change, the test or the
  before/after evidence.
