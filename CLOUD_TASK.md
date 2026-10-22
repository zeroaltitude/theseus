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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail3-tests`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `29f5dcad` (2026-10-22: install #69, which landed the patch1 tail lane `integ/tail` and the walk1 fixes); that is your base. Where the
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
- `crates/theseus-protocol/src/lib.rs` 2,731 / 2,733 (two lines of room)
- `crates/theseus/src/render.rs` 3,122 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections below. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail3-tests, waits that name what they wait for, the effort lapse on a virtual clock, the turn link with no web UI, and the mail-auth row's gaps (theseus-0wu8t, theseus-2ix1k, theseus-40cuo, theseus-ta1be, theseus-y4ajh item 1)

Branch: `cloud/20261021-tail3-tests`. Every commit's subject carries the Beads id of what it changes (`theseus-0wu8t`,
`theseus-2ix1k`, `theseus-40cuo`, `theseus-ta1be`, `theseus-y4ajh`). Deadline for the report: 3 hours after you start.

**Why.** Five small follow-ups the reviews of the patch1 tail rows left open, now on `main`. None changes what the owner
sees in a reply; two fix a wrong link and a missing test, the rest make a hang or a regression name itself.
- **theseus-0wu8t.** `test_support::until_event` has no deadline (by design: a slow host and a lost event differ only in
  time), so a hang dies at nextest's 120 s kill with no line saying what was awaited. The same is true of
  `until_not_deciding` (judge people tests) and the `turn.submit` hold in `crates/theseus/tests/herdr.rs`. And
  `tests_task_brief::run` still waits on a 20 s clock for a task's first turn; under load seven checkpoint tests flaked
  on it (the tail-flakes row moved `task_run` in `tests_task_checkpoint.rs` to `until_event`, not `run`).
- **theseus-2ix1k.** The effort refusal mark (`turn/effort_step.rs`, `REFUSAL_HOLDS_MS`, one hour) reads
  `self.kernel.now_ms()`, but `Core::build` (`rpc/mod.rs`) hard-wires `theseus_kernel::RealClock`, so no daemon-level
  test can drive the hour. Only the pure `a_refusal_mark_lapses_after_an_hour` covers it.
- **theseus-40cuo.** `CockpitConfig::turn_url` (`config/cockpit.rs`) falls back to `localhost:<default web port 7433>`
  when the web UI is off, so `theseus inspect` / `theseus open turn` on a daemon with `[web] enabled = false, port =
  17433` prints a 7433 link to nothing. `turn_link` already returns `None` in that case.
- **theseus-ta1be.** From the tail2-mailauth review: (1) theseus-1qexl changed `fact/route.rs`'s turn-checkpoint
  narration to "the conversation moves" and no test asserts it; (2) `crates/theseus-core/Cargo.toml` has `psl = "2"` as
  a direct version line while third-party versions live in the root `[workspace.dependencies]`; (3) `psl` embeds the
  suffix list at release: a new suffix needs a crate bump, which no doc says; (4) the beat closure in
  `toolrun/jobs_tools.rs` posts a status line only when `clock.beat` returns `Some`, and no test fails if it posted on
  `None` (a job past its file's head printing into its wrapper).
- **theseus-y4ajh, item 1 only.** `scripts/AGENTS.md`'s commit recipe does not say that `Keel:` lines go in the last
  paragraph beside `Co-Authored-By` (the guard now refuses an ack in an earlier paragraph). Items 2 and 3 of that issue
  edit `scripts/keel-guard.py`: **not yours** (see Not in this row).

**Risk: low.** Test helpers, one test-only constructor, one link fallback, one manifest line, docs.

**Prerequisite: base `origin/main` at `29f5dcad`.** Your branch joins the new lane `integ/tail3`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail3-tests` | theseus-0wu8t, theseus-2ix1k, theseus-40cuo, theseus-ta1be, theseus-y4ajh (item 1) | named waits; `run` on an event; a clock seam for tests; the link with no web UI; a narration test; `psl` in workspace deps; a beat test; one docs paragraph | `crates/theseus-core/src/test_support.rs`, `tests_task_brief.rs`, `judge/people/tests.rs`, `crates/theseus/tests/herdr.rs` (the hold only), `crates/theseus-core/src/rpc/mod.rs` (`Core::build` only), one new test file + its `mod` line, `config/cockpit.rs`, `rpc/inspect.rs` (one line), `crates/theseus/src/open.rs`, `fact/route.rs` (tests only), `Cargo.toml`, `crates/theseus-core/Cargo.toml`, `Cargo.lock` (only if it moves), `watch/mail_auth.rs` (module doc only), `tests_wait_beat.rs`, `scripts/AGENTS.md` | S-M |

## What exists (lines at `29f5dcad`; search by name, a line may have moved)
- **Waits.** `crates/theseus-core/src/test_support.rs` (245 lines): `until_within(what, secs, f)` :185, `until_event(f)`
  :199 (10 ms polls, no deadline, doc says why), `until(what, f)` (20 s), `until_within_10ms`. `until_event` callers:
  `tests_task_checkpoint.rs` :143, :631, :635. `until_not_deciding(core)` is `judge/people/tests.rs` :269, called at
  :541. The herdr hold: `crates/theseus/tests/herdr.rs` near :1650-1660 (the fake daemon holds the task's `turn.submit`
  refusal until the fake herdr has recorded "sending your message").
- **`run`.** `crates/theseus-core/src/tests_task_brief.rs` :53 `pub(crate) async fn run(r, p)`: `task_start`, then
  `until("the task's turn", || exec(..).state == ExecState::Complete)` (the 20 s clock). Other files call it
  (`git grep -n "tests_task_brief::run\|task_brief::run" crates`), among them `tests_task_checkpoint.rs`.
- **The clock.** `crates/theseus-core/src/rpc/mod.rs`: `pub struct Parts` :352 (about 40 `Parts { .. }` literals
  across the crate's tests and `theseus-discord`; do **not** add a field to it); `Core::build` destructures it near :666
  and builds `Kernel::new(store.shared(), Arc::new(theseus_kernel::RealClock), ..)` near :683. `Parts::for_tests` is
  near :1431. `theseus_kernel::VirtualClock::new(ms)` exists (`tests_day_ceiling.rs` :405 uses one).
- **The refusal mark.** `crates/theseus-core/src/turn/effort_step.rs`: `REFUSAL_HOLDS_MS` :21, `mark`/`all` on
  `self.kernel.now_ms()` :46/:69, the `provider.effort_refused` row; the 400 test is in `tests_turn_effort.rs` near
  :194-230 (it filters `provider.effort_refused` rows); `compiler/effort.rs` :73-77 decides which 400 counts (one that
  names `output_config` on a request that carried one).
- **The link.** `crates/theseus-core/src/config/cockpit.rs`: `resolve(web_port, web_enabled)` sets `base` to `None`
  when the web UI is off; `turn_url` :68 falls back to `local_base(default_web_port())`; `turn_link` :75 returns
  `None`. The tests near :90-120 (`cfg.cockpit.turn_url("ses_a", "turn_b")` for default, relay, ported, both).
  `rpc/inspect.rs` :138 `url: self.cfg.cockpit.turn_url(sid, tid)` into `TurnInspectResult.url: String`
  (`theseus-protocol/src/inspect.rs` :160, doc "what `theseus open turn` opens"). `crates/theseus/src/open.rs` near :54
  prints/opens `r.url`.
- **Narration.** `crates/theseus-core/src/fact/route.rs` near :118-137: a source starting `turn checkpoint` gives
  `("turn checkpoint", "conversation")`, and the line "Routing ({source}: …): the {what} moves from A to B."
- **psl.** `crates/theseus-core/Cargo.toml` :11 `psl = "2"`; root `Cargo.toml` `[workspace.dependencies]` near :51 holds
  the third-party versions (`anyhow = "1"`, `serde`, `tokio`, …); members say `x.workspace = true`.
  `crates/theseus-core/src/watch/mail_auth.rs` module doc names the alignment rule.
- **The beat.** `toolrun/jobs_tools.rs` near :483-493: `theseus_store::blocking(|| { let out = clock.beat(..)?;
  wait_status::post(..); Some(out) })`, `None => held = true` ("A job past its file's head prints into its wrapper: not
  silent"). Tests: `tests_wait_beat.rs` (`a_job_that_prints_keeps_its_wait`, `a_silent_jobs_beat_posts_one_status_line_to_its_place`,
  `no_status_line_without_a_beat`, the `statuses(r)` helper :187).
- **Docs.** `scripts/AGENTS.md`: the commit recipe (search `Co-Authored-By` and `Keel:`).

## What to build
1. **theseus-0wu8t.**
   - `until_event` takes `what: &str` first and prints `waiting for {what} ({n} s)` with `eprintln!` every ~30 s of
     wall time while it polls (no deadline: keep the doc's reason, and add that the line names a hang in nextest's
     captured output). Update its three callers with a short `what`. Give `until_not_deciding` and the herdr hold the
     same print (a helper, not a copy, where they share a crate; the herdr test is another crate, so a small local
     loop there is fine).
   - `tests_task_brief::run` waits with `until_event("the task's first turn", ..)` instead of `until`'s 20 s clock.
     Nothing else in `run` changes. Do not touch any other `until(..)` call.
2. **theseus-2ix1k.** Add a test-only way to build a `Core` on a given clock without touching `Parts`: for example
   `Core::build` becomes a thin call to `Core::build_on(parts, clock: Arc<dyn theseus_kernel::Clock>)` (check the
   trait's real name in `theseus-kernel`), with `build` passing `RealClock`. Then a new test file
   `crates/theseus-core/src/tests_effort_lapse.rs` (one `mod` line beside `tests_turn_effort`) drives it on a
   `VirtualClock`, reusing `tests_turn_effort.rs`'s fake-provider set-up (move a helper to `pub(crate)` if needed,
   do not copy it):
   - a 400 naming `output_config` marks the model (one `provider.effort_refused` row); the next request to that model
     carries no `output_config`;
   - at mark + 1 h − 1 ms the request still carries none; at mark + 1 h it carries one again;
   - review finding 2: a 400 that does **not** name `output_config` is neither retried without effort nor marked (no
     `provider.effort_refused` row; the next request still carries `output_config`).
3. **theseus-40cuo.** `turn_url` goes: `TurnInspectResult.url` becomes `self.cfg.cockpit.turn_link(sid, tid)
   .unwrap_or_default()` (keep the wire type a `String`; empty means no web UI; add that to its doc in
   `theseus-protocol/src/inspect.rs`, doc only). `theseus open turn` (`open.rs`) with an empty url prints one line saying
   no web UI runs and how to get one (`[web] enabled = true`, or `[cockpit] public_url`) and opens nothing; `theseus
   inspect`'s printed link is absent, not 7433. Move `cockpit.rs`'s tests from `turn_url` to `turn_link` and add the
   case `[web] enabled = false, port = 17433`, no `public_url` → `None`. If another caller of `turn_url` exists
   (`git grep -n turn_url crates`), give it the same treatment and name it in the report.
4. **theseus-ta1be.**
   - (1) A test in `fact/route.rs`'s tests: a turn checkpoint's move narrates "the conversation moves from X to Y"
     (and a task checkpoint's still says its own word). Plant: put back "turn".
   - (2) Move `psl`'s version to the root `[workspace.dependencies]` (`psl = "2"`) and make core say `psl.workspace =
     true`. `Cargo.lock` must not change other than nothing at all (same version): check `git diff --stat Cargo.lock`.
   - (3) One sentence in `mail_auth.rs`'s module doc: the suffix list is the one compiled into the `psl` release in
     `Cargo.lock`; a suffix added later reaches the check only with a crate bump.
   - (4) A test in `tests_wait_beat.rs`: a job whose file has passed its head (it prints into its wrapper, so the beat
     returns `None`) posts no status line (`statuses(r)` empty) while its wait goes on. Plant: post on `None` too.
5. **theseus-y4ajh item 1.** In `scripts/AGENTS.md`'s commit recipe, one or two sentences: `Keel:` lines go in the
   commit message's last paragraph, beside `Co-Authored-By`; the guard refuses an ack in an earlier paragraph.

## Tests (each with a planted revert; the first is the key test)
- **The effort lapse on a virtual clock** (step 2), all three cases. Plant: make `all` ignore the hour (keep marks
  forever): the "+1 h carries one again" case fails. Plant 2: mark on any 400: finding 2's case fails. Paste each
  failing line.
- The link: `[web] enabled = false` gives no link (plant: restore the 7433 fallback in `inspect.rs`).
- The narration test and the beat test (plants above).
- 0wu8t has no behaviour to plant; show one captured `waiting for …` line from a deliberately hung local run (then
  revert it) or say why you could not.
- `cargo nextest run -p theseus-core tests_task_brief tests_task_checkpoint tests_turn_effort tests_effort_lapse
  tests_wait_beat config::cockpit fact::route judge::people` and `cargo nextest run -p theseus herdr` must pass.

## Not in this row
- theseus-y4ajh items 2-3 (`scripts/keel-guard.py` `compare_arms` and `match_arms`): any edit to the keel guard's own
  files needs a signed `Keel: keel-file` trailer quoting the owner's yes, which a cloud row cannot carry. Leave it.
- theseus-k7lzu (re-running the horizon agenda test under load on `main`): the conductor's measurement.
- theseus-94782 (effort docs), every `docs/status.md` and `crates/theseus-core/AGENTS.md` edit: another row of this wave
  (tail3-google) owns those two files. If your change needs an AGENTS.md line, put the sentence in your report for it.
- `google/*`, the Discord crate, `theseus-voice`, `thread_*`, `turn/route_step.rs`, `turn.rs`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- No new always-loaded model text and no new tool.
- **Line ceilings** (`scripts/long-files.txt`): none of your files is listed; each stays at or under 2,500 lines
  (`rpc/mod.rs` is 1,459). Never raise a ceiling.
- **In-flight rows beside you on `main` 29f5dcad, joining `integ/tail3` with you:** tail3-discord (`theseus-discord`,
  `theseus-voice`) and tail3-google (`google/*`, `rpc/learning.rs`, the learning renderers, `docs/status.md`, core and
  cockpit `AGENTS.md`). Stay out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer: if a change would need one, leave it out and say so. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- **Dependencies.** No new package. `Cargo.lock` unchanged.
- **Store format** on your base is 48 (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row changes no
  stored record.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so in the report). Put the Beads id in each
  subject. End each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
- Read `crates/theseus-core/AGENTS.md` (tests, effort) and `crates/theseus/AGENTS.md` before you edit. Run
  `cargo nextest run -p theseus-core`, `cargo nextest run -p theseus`, then `cargo clippy --workspace --all-targets --
  -D warnings` and `cargo fmt --check` before the last commit. L1 tests need a non-root daemon with a job cgroup and
  fail on this VM; this row needs none of them, so say which tests you ran.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit: it must not be empty. It holds: what changed per Beads id; each test and its plant, with the failing output's
line; the clock seam's shape and why; every `turn_url` caller you changed; the `Cargo.lock` diff stat (empty); what
could not run on the VM, with the exact commands for the maintainer; the keel guard's output; the store format
(unchanged); any AGENTS.md sentence for tail3-google's docs; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
