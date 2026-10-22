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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail2-threads`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from the integration branch `integ/threads` at `8b641b6b` (2026-10-21: `main` `6918e3c1` plus threads round 2: threads-home-claim, threads-routing-rest, xlri5-families, and part of the protocol split); that branch, not `main`, is your base. Where the
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

## Your task: tail2-threads, the threads lane's open follow-ups: a turn from another directory kept out of a thread, the new-thread offer only where the cap held, the cross judge's own breaker, a guard test that cannot go vacuous, two home-claim lows, steadier families tests, and the lane's docs (theseus-vw8k7, theseus-q3ggn, theseus-pt7lt, theseus-1uiiu, theseus-powrh, theseus-l4n39, theseus-6juq4, theseus-zokhn, theseus-3vb1z, theseus-8cl9f)

Branch: `cloud/20261021-tail2-threads`. Every commit's subject carries the Beads id of what it changes. Deadline for
the report: 5 hours after you start.

**Base: `origin/integ/threads` at `8b641b6b`**, not `main`. That lane holds `main` (`6918e3c1`) plus three rows main
does not have yet: the home claim (theseus-98bz.6, `rpc/home_claim.rs`), the rest of the routing (theseus-98bz.3, .5,
.7: the bench clock, working-set growth, the one-thread offer) and the tool families (theseus-xlri5,
`turn/families_step.rs`), plus a partial split of the protocol's `lib.rs`. This row joins back into `integ/threads`.
Where the wave note above says `origin/main`, read `origin/integ/threads`.

**Why.** The reviews of the threads lane's rows left these open. None is large; together they are the lane's tail:
- **A message from an unrelated directory is placed in a thread (theseus-vw8k7).** On the owner's walkthrough a plain
  `[dry-run] Start 2 tasks, $1 each: bash slow.sh census 2 …` sent from `~/projects/walkthrough` (no Theseus path in
  it) was placed in `thread:theseus-dev`. The likely cause is evidence that names nothing of the thread: stickiness to
  the place's last thread, the reply chain, or a learned correction (`learned_thread` lifts a thread to `LEARNED` 0.92
  when the message's words are near a message the owner once moved there; earlier dry runs went there). The router
  never looks at the directory the turn will run in, although the submit carries it (`TurnSubmitParams.dir`, else the
  session's `SessionRecord.dir`) and a thread's working set has `roots`.
- **The entity-based `new` offer is on every ask (theseus-q3ggn).** With `[threads] sticky_alone_asks` on, `decide`
  offers a new thread in any ask where the message names two or more entities no thread holds. The row that built it
  (theseus-eqp1v) was asked for that offer only in the ask the stickiness cap made.
- **The cross judge shares Jev's general breaker (theseus-pt7lt).** `recall_cross.v1` failing five times in a row opens
  the breaker every other pack uses, so a flaky cross judge can stop `classify` and routing. `rerank.v1` already has a
  breaker of its own (32d).
- **A guard test can pass with nothing to guard (theseus-1uiiu).** `no_call_when_the_digest_fired` checks that the
  recall dropped thread notes and that no cross call was made, but not that one of those drops was above the cross
  judge's bar. If a scoring change puts every drop under the bar, the test passes vacuously (its first version did).
- **Families lows (theseus-powrh).** Two families tests wait for FakeJev inside the 200 ms `judge_wait_ms` default,
  so a loaded suite can flake them. `TurnSubmitResult.raised` has no doc comment (the protocol's `lib.rs` was at its
  ceiling). The review's first low, a `families.v1` call on every routed prompt even when the session's model has
  tool search, is **not a change**: it follows the owner's decision of 2026-10-21 14:12 ("always try to jev categorize
  each toolset"). Write that down in the docs; do not change the call.
- **Home-claim lows (theseus-l4n39).** `thread_unclaim` counts a node an unsplit already set to null as `kept`. The
  multi-page read in `turn_nodes_back` has no test. The claim reads the nodes' keys and appends later, outside any
  lock, so a split landing between the read and the append is overwritten.
- **The lane's docs (theseus-6juq4, theseus-zokhn, theseus-3vb1z, theseus-8cl9f).** Four rows changed what threads
  do and left the repo's docs behind: the thread recall and the cross judge with the place digest (98bz.8-.10), the
  router's ask and sticky rule (98bz.2, eqp1v, 98bz.4), the bench clock, growth and the one-thread offer (98bz.3, .5,
  .7), and detours with `theseus why` (v4u66, r5ydi, 3bs4e). The spec's Part III lives outside this repository: list
  the Part III items in your report for the maintainer, and do the repo's parts.

**Risk: medium.** vw8k7 changes where messages go; everything else is local. The routing change must leave every
existing routing test's decision unchanged except the ones you can show were the bug.

**Prerequisite: `origin/integ/threads` `8b641b6b`.**

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail2-threads` | theseus-vw8k7, q3ggn, pt7lt, 1uiiu, powrh, l4n39, 6juq4, zokhn, 3vb1z, 8cl9f | directory rule in the router; `new` offer gated on the cap; `recall_cross` breaker; non-vacuous guard; families rig waits; unclaim's count, multi-page test, the race; docs | `crates/theseus-core/src/thread_route.rs`, `rpc/tests_thread_route.rs`, `rpc/home_claim.rs`, `rpc/tests_home_claim.rs`, `judge/mod.rs` (the `with_breaker` chain only), `judge/cross.rs`, `judge/sink.rs` (one arm if needed), `rpc/tests_cross_judge.rs`, `tests_families.rs`, `crates/theseus-core/AGENTS.md`, `docs/status.md`, `docs/design/` (new `threads.md`, a line in `docs/design/README.md`); one line in `rpc/thread_flow.rs` only if vw8k7 needs it (below) | M |

## What exists (lines at `8b641b6b`; search by name, a line may have moved)
- **The router.** `crates/theseus-core/src/thread_route.rs` (1,792 lines). The module doc (:1-35) states the order:
  addressing, the pre-router, fusion with Jev, the decision. `pre_route(text, threads, last, now_ms, half_life_ms)`
  (:462) scores each live thread from entity hits in its working set (`entity_hits`), `sticky` (:435, 0.6 halving
  every `sticky_half_life_secs`) and `chain` (:441), combined by `noisy_or`, capped below the act bar. `Pre` (:449)
  carries `entity`, `sticky`, `chain`, `score`, `hits`, `learned`. `Last` (:421) is `{ slug, at_ms, replied_ms }`.
  `LEARNED` = 0.92 (:51). `hold_sticky` (:621) caps a sticky-only top thread Jev did not pick at `STICKY_CAP`
  (`CONFIRM - 0.01`). `decide` (:845) runs `hold_sticky` when `rules.sticky_alone_asks`, then the table; the ask arm's
  `raised` (:889-891) is `ranked has Choice::New || (rules.sticky_alone_asks && rules.new_entities >= 2)`. `Rules`
  (:571) holds `sticky_alone_asks`, `new_entities`, `ask_every_thread`. `new_entities` (:595). `decide_single` and
  `lone_offer` (theseus-98bz.7, the one-thread offer: its own rule with `new_entities`, not this issue's) follow.
  Unit tests in the same file from :1250 (`the_decision_table_is_the_same_with_the_sticky_rule_on` near :1491, the
  `new_entities` cases near :1517-1570).
- **The caller.** `crates/theseus-core/src/rpc/thread_flow.rs` `place_unaddressed` (near :322): `pre_route(&p.input,
  live, state.last(home).as_ref(), now, half)` then `self.learned_thread(&p.input, &mut pre)` (:453: lifts the nearest
  corrected thread to `LEARNED`, `learned = true`), then one live thread → `offer_beside_one`/`decide_single`, a
  `decisive` pre-router → quiet route, else `decide_with_jev`. `may_route` (:288) decides whether a message is routed
  at all. **This file belongs to a sibling row (tail2-chat) on the same base.** `p.dir` is `TurnSubmitParams.dir`
  (`crates/theseus-protocol/src/submit.rs`: "the directory the session works in"); the session's own is
  `SessionRecord.dir` (`crates/theseus-core/src/session.rs:113`).
- **Roots matching that exists.** `rpc/home_claim.rs` `thread_of(paths, dir, live, home)` (:73): a path is a thread's
  when it is one of its `files` or under one of its `roots` (absolute, `~/`, or relative under the turn's dir;
  component prefixes; `normal`/`fold` from `toolrun/misroute.rs`). Its `holds` closure is the predicate you want for a
  directory; lift it to a small `pub fn` both can call rather than writing a second matcher.
- **Breakers.** `crates/theseus-core/src/judge/mod.rs` `built()` (near :453): `JevJudge::new(client, prices,
  BreakerConfig::default()).with_breaker(rerank::BREAKER, &[rerank::RERANK_PACK], BreakerConfig::default())`.
  `theseus-judge/src/judge.rs` `with_breaker(name, packs, config)` (:357) and `own_breakers()`. `rerank.rs`
  `BREAKER = "rerank"` (:56) and `rerank_no_wait` (near :239: unreachable, own breaker open, budget). `judge/sink.rs`
  near :419 has an arm for `rerank::BREAKER` (read what it does and decide if the cross breaker needs the same).
  `fact/judge.rs` near :268-305: the `judge.circuit` row and its line already name an own breaker (`breaker: Some(b)`).
  `judge/ladder/adopt.rs` near :82 has a rerank-specific adoption rule: leave the ladder alone, note it in the report.
  The cross pack: `judge/cross.rs` `CROSS_PACK` (:32), `cross_live` (near :56) returns `None` when the pack is off or
  `jev_unreachable()`. Model test: `tests_rerank_live.rs` `five_rerank_timeouts_open_reranks_breaker_alone` (:109).
- **The guard test.** `rpc/tests_cross_judge.rs` `no_call_when_the_digest_fired` (:207). The cross candidates are the
  `thread_*` drops with `d.fused / lens.top_fused > lens.bar` (`turn/cross_step.rs` :113-131; `lens.bar` is
  `[threads] cross_recall_bar`, 0.75). `RecallDrop` (`theseus-protocol/src/memory.rs` :529) carries `share` (this
  note's share of the query's best) for `thread_*` drops.
- **Families tests.** `tests_families.rs`: `a_family_jev_judges_relevant_loads_without_being_named` (:174) and
  `one_families_judgment_per_prompt` (:281) need FakeJev's answer inside `[tools.defer] judge_wait_ms` (`config/defer.rs`
  :54/:86, validated at most 2,000). `jev_late_or_unreachable_falls_back_to_the_named_rule` (:221) tests the deadline
  itself: leave its timing as it is. `turn/families_step.rs` `families_start` (:57) skips Jev only when the model has
  tool search **and** the route pack is below canary: keep.
- **Home claim.** `rpc/home_claim.rs` (380 lines; module doc :1-21). `try_claim` reads each node's
  `moved.of.<node>` key, keeps the nodes without one, then one `store.append`. `turn_nodes_back` (:212) pages back
  `PAGE` = 64 at a time, at most `MOST` = 1,024. `thread_unclaim` (:247): `kept += 1` for any node whose key is not this
  claim's value, which includes `None` (a node an unsplit nulled). Tests: `rpc/tests_home_claim.rs` (672 lines;
  `split_over_the_claim` :493, `an_unclaim_restores_the_home_turn_and_leaves_a_later_split` :531). A split writes in
  `thread_split.rs` (`split_from_session`, run under `theseus_store::blocking`); the review notes a split refuses while
  the source session's turn runs.
- **Docs.** `crates/theseus-core/AGENTS.md` (3,011 lines): thread recall and seals near :524-560, the thread router
  near :562, the home claim near :667, detours near :1546-1550. `docs/status.md` (809 lines): Jev's judgments near
  :52, activity threads near :87-98. `docs/design/` has no threads note; `docs/design/README.md` indexes the folder.
- **Line ceilings** (`scripts/long-files.txt` at `8b641b6b`): none of this row's files is listed (each must stay at
  2,500 lines or fewer; `thread_route.rs` is 1,792). `crates/theseus-core/src/turn.rs` 3,525 and
  `crates/theseus-protocol/src/lib.rs` 2,733 are listed: you edit neither.

## What to build
1. **theseus-vw8k7: reproduce first, then the directory rule.**
   - Write the failing test first, `a_message_from_another_directory_is_not_placed_by_stickiness` in
     `rpc/tests_thread_route.rs` (use its `rig` near :271): two live threads, one (`dev`) with `roots` under a
     project directory and a recent placed turn so stickiness and the reply chain stand; the owner moved an earlier
     dry-run message to it (so `learned_thread` lifts it); then a plain dry-run message naming no entity of either
     thread, submitted with `dir` set to an unrelated directory. Run it on the unchanged code and record what placed
     it (which of sticky, chain, learned or Jev's pick, from the `thread.routed` row's `pre`). If the reproduction
     shows another cause, fix that cause and say so; do not fit the test to the hypothesis.
   - The rule (yours to state precisely; write it in the report as an owner-visible change): **evidence that names
     nothing of a thread (stickiness, the reply chain, a learned correction) does not count for a thread whose `roots`
     are set when the turn's directory is known and lies under none of them.** An entity hit still counts, and Jev's
     pick still fuses. A thread with no roots, or a turn with no known directory, routes exactly as today. Put the
     rule in `thread_route.rs` as a pure function over `&mut [Pre]`, the threads and the directory (with `home` for
     `~`), using the roots predicate lifted out of `home_claim::thread_of`. Update the module doc's step 2.
   - The call: the directory is `p.dir`, else the session's `dir`. If the cleanest call site is `place_unaddressed`
     right after `learned_thread`, add exactly that one line in `rpc/thread_flow.rs`, touch nothing else in that file,
     and name the line in your report's first section (a sibling row edits other parts of the file). Do not change
     `pre_route`'s signature.
   - Decide whether `may_route` should also skip routing a message whose directory lies under no live thread's roots
     while every live thread has roots; if you do not change it, say why.
2. **theseus-q3ggn: the `new` offer only in the ask the cap made.** `hold_sticky` returns whether it held a thread
   (capped it), `decide` keeps that bool, and the ask arm's `raised` becomes `Jev raised New || (held && new_entities
   >= 2)`. `lone_offer` is not changed. If an existing assertion in `thread_route.rs` or `tests_thread_route.rs`
   changes because of this, it is because it encoded the bug: say which and why in the commit body and the report.
3. **theseus-pt7lt: `recall_cross.v1`'s own breaker.** `cross.rs` gains `pub const BREAKER: &str = "recall_cross"`;
   `built()` chains a second `.with_breaker(cross::BREAKER, &[cross::CROSS_PACK], BreakerConfig::default())`. In
   `cross_live`, an open cross breaker returns `None` at once with no call and no wait (mirror `rerank_no_wait`'s
   read of `own_breakers()`). Check `sink.rs` :419's rerank arm and `fact/judge.rs`'s line: the `judge.circuit` row
   names `recall_cross` when its breaker moves.
4. **theseus-1uiiu: the guard cannot go vacuous.** In `no_call_when_the_digest_fired`, assert that at least one
   `thread_*` drop's `share` is above `cross_recall_bar` (the same predicate as `cross_step.rs`), with a message that
   prints the drops. If no drop clears the bar today, the test is vacuous now: change the fixture until one does,
   and say so.
5. **theseus-powrh.** In the two families tests that need an answer, set `c.tools.defer.judge_wait_ms = 2_000` in the
   rig (the most the config takes) so FakeJev's answer is waited for, not raced; the default and the deadline test are
   unchanged. No change to `families_start`. The `raised` field's doc goes in the docs (item 7); the one-line doc
   comment for `TurnSubmitResult.raised` in `crates/theseus-protocol/src/lib.rs` is written into your report for the
   maintainer (another row edits the protocol).
6. **theseus-l4n39.**
   - `thread_unclaim`: `kept` counts only nodes whose key holds another claim's or split's value; a null key is
     neither restored nor kept (the protocol's result type does not change; say in its doc comment in `home_claim.rs`
     what `kept` counts).
   - A multi-page test: a home turn of more than 64 nodes (at least 70, built cheaply by appending nodes to the
     store as the existing helpers do) is claimed whole; and a turn past `MOST` claims nothing.
   - The race: decide between (a) re-reading each node's key in the same blocking closure just before the append and
     dropping changed nodes, (b) taking the home session's execution lock for the read and the append, or (c)
     documenting why it cannot happen (a split refuses while the source's turn runs; the claim runs right after that
     turn ends). Pick by what the code guarantees, show it with a test if it is (a) or (b), and write the reason in the
     module doc. No new lock type.
7. **Docs (theseus-6juq4, theseus-zokhn, theseus-3vb1z, theseus-8cl9f, and the docs halves of powrh and l4n39).**
   - `crates/theseus-core/AGENTS.md`: the thread recall paragraph gains the cross judge (`recall_cross.v1`, its bar
     and k, its own breaker) and the place digest (98bz.9, the `scope` answer of `classify.v4`); the inbound paragraph
     gains `Inbound.in_thread`; the router paragraph gains the ask's options (`new`, `home`, `other` and a slug), the
     sticky rule with this row's cap gate, the ticket shape the extractor reads as a ref, this row's directory rule, the
     bench clock (`route_at_ms` under `[threads] bench_clock`), growth (`thread_grow.rs`, `MAX_SET`, no eviction yet)
     and the one-thread offer (`single_offers_new`). Keep each addition to the facts a maintainer needs; no history.
   - `docs/status.md`: the activity-threads entry (near :87) says what the router, growth, the claim, links and the
     cross judge do now; the Jev entry (near :52) names `classify.v4` and `recall_cross.v1`; a detours line says
     `theseus why` names a detour and `context.explain` shows its parts (theseus-r5ydi, 3bs4e, v4u66). Plain words;
     no counts you did not measure.
   - New `docs/design/threads.md`: the router's order (link the module doc), `route_at_ms` and `[threads]
     bench_clock` (the bench's clock, refused without the switch), the deferred-tools budget rule (soft cap plus
     `admit_ceiling`, `config/defer.rs`), the `raised` field, the families call's owner decision, and the home claim's
     race decision. One line for it in `docs/design/README.md`.
   - Your report lists the spec Part III items the maintainer should write: 98bz.2, eqp1v, 98bz.4, 98bz.8, .9, .10,
     98bz.3, .5, .7, xlri5, 98bz.6, v4u66, r5ydi, 3bs4e, and this row's vw8k7, each with one sentence of what changed.

## Tests (each with a planted revert; the first is the key test)
- **`a_message_from_another_directory_is_not_placed_by_stickiness`** (`rpc/tests_thread_route.rs`): red on the base,
  green after; its twin, the same message from under the thread's root, still goes to the thread; and a thread with
  no roots routes as before. A pure unit test of the rule in `thread_route.rs`. Plant: skip the rule's call; the key
  test must fail.
- `the_new_offer_rides_only_the_ask_the_cap_made` (`thread_route.rs`): two new entities, top thread capped → `new`
  offered; two new entities, an ask with no cap (two threads within `CLOSE`) → no `new` unless Jev raised it. Plant:
  the old `raised`.
- `a_failing_cross_judge_opens_only_its_own_breaker` (beside the rerank one or in `rpc/tests_cross_judge.rs`): five
  cross failures open `recall_cross`, `classify` still judges, and the next cross wait is skipped with no request.
  Plant: drop the `with_breaker` line.
- `no_call_when_the_digest_fired` with its new assertion. Plant: raise the bar in that test's rig past every drop;
  the new assertion must fail (and would have caught the vacuous pass).
- Home claim: `an_unclaim_counts_a_nulled_node_as_neither` (unsplit after the claim, then unclaim: `kept` 0),
  `a_long_home_turn_is_read_across_pages`, and the race test if you chose (a) or (b). Plant each.
- Families: no plant (a rig timing change); run each test 20 times under load (`cargo nextest run -p theseus-core
  tests_families --retries 0` in a loop beside `stress-ng --cpu 4` or a `yes > /dev/null` per core) and paste the
  counts.
- Docs: no test; `scripts/gate.sh`'s shape step must pass.

## Not in this row
- `rpc/thread_flow.rs` beyond the one vw8k7 line, `rpc/thread_link.rs`, the TUI, `theseus-discord`, the protocol
  crate (a sibling row, tail2-chat, owns them on this base).
- `turn/route_step.rs`, `theseus why`, `context.explain`, `crates/theseusd/tests/judge.rs` (sibling row tail2-route).
- The `families.v1` call on tool-search models (owner's decision, kept). Working-set eviction (a later issue).
- The spec's Part III (outside the repo; listed in your report). Live checks after install (the maintainer's).

## Rules for this row
- Model roles: you design the vw8k7 rule and the l4n39 race decision; write each as one paragraph in the report:
  the rule, the evidence from your reproduction, what it changes for the owner, and how to turn it back (if you add a
  `[threads]` switch for the rule, default it on, add its template line under `[threads]` in
  `crates/theseus-core/config/theseus.example.toml` and its `config/threads.rs` field; say so).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`,
  then `git status`); list each plant with the failing line. The key test must be shown red on its plant.
- No new always-loaded model text; nothing new on a plain turn's path (`tests_m3::a_plain_turn_stays_within_its_frame_budget`
  must pass). The router adds no model call.
- **Keel.** Do not delete, ignore or weaken a test; do not raise a budget, cap or ceiling (the families rig's
  `judge_wait_ms` is a test rig's wait, not a default: say so in its commit body). If a line will not fit, split the
  code; never edit `scripts/long-files.txt`. Before your last commit run `python3 scripts/keel-guard.py --base
  origin/integ/threads --head HEAD --quiet` and paste its output in the report. Any finding it prints is explained
  there (a changed assertion that encoded a bug).
- No new dependency. **Store format 48** on your base (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`); this
  row adds no stored field. If you find you must, bump to 49 and add the old layout's sample to `tests_layouts.rs`.
- **Commits:** one or more per issue, each subject with its Beads id, each with the trailer
  `Co-Authored-By: Tabitha/Claude` exactly as the base's own commits carry it (copy the line from
  `git log -1 --format=%B 8b641b6b`); sign them as the VM's git is set up to (do not turn signing
  off). No person's name anywhere in the repo, the commits or the report: write "the owner". Fixture paths and names are
  invented.
- Read `crates/theseus-core/AGENTS.md` (the parts named above) and `crates/theseus-judge/AGENTS.md` first. Run
  `cargo nextest run -p theseus-core` and `-p theseus-judge`, and `cargo clippy --workspace --all-targets -- -D
  warnings` before the last commit. About 33 L1 tests need a non-root daemon with a job cgroup and fail on this VM;
  name the ones you saw fail and that none is in your files.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it must
not be empty) with, in this order: the one `thread_flow.rs` line if you added it; per Beads id what changed; the vw8k7
reproduction (what placed the message on the base) and the rule; the l4n39 race decision; each test and its plant with
the failing line; the families loop counts; the keel guard's output; the store format (unchanged at 48); the Part III
items for the maintainer; the `TurnSubmitResult.raised` doc line; follow-ups you found. Push, and end with a final
message that starts `CLOUD REPORT COMPLETE`.
