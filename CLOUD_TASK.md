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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-job-wait-status`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: job-wait-status, a silent job's beat also posts one status line to the owner's place, and a test holds the waiter's look at a minute or less (theseus-5d204)

Branch: `cloud/20261021-job-wait-status`. Every commit's subject carries `theseus-5d204`. Deadline for the report: 3
hours after you start.

**Why.** A `job_wait` on a job that prints nothing now returns at `[tools] job_wait_beat_secs` (600 s by default, rule
theseus-7m7w.3, built by theseus-53ujg.6) with what it saw: how long the job ran, its CPU where its cgroup gives a
reading, and its output tail. That result goes only to the model. The person who started the work sees nothing in
their place for as long as the model keeps waiting: a quiet 30-minute build or search is half an hour of silence in
the chat, with no sign whether the job is alive, busy or stuck. The owner's design words (verbatim): "I would like
the checker cycle time to be 60 seconds or faster, and the 10 minute rule to output a proactive status message sounds
great." So the beat stays on by default, and each time it fires the person also gets one plain line in their place,
written by the daemon from facts it already holds, with no model call. The waiter already looks at its job every
second (`waits::LOOK`) besides its event wakes, so the "60 seconds or faster" part holds today; nothing pins it, so a
later change could slow it unseen. This row pins it with a test.

**Risk: low.** One new post at a moment that already exists (the beat's early return), through the outbox's existing
`notice` kind. The wait's result, its text and its meta keys do not change. No store layout changes.

**Prerequisite: integ/harness 38310054.** Your base is the harness integration lane at `38310054` or later, not
`main`. That lane holds the turn-end follow-ups row (the merge `c680676c`), which added the beat. Every path and name
below is as it is there.

## What exists (on your base, lines at `38310054`; search by name if a line moved)
- **The wait:** `crates/theseus-core/src/toolrun/jobs_tools.rs`, `async fn wait(core, f: Found, secs, asked)` (near
  :436-540). Its loop takes one look a pass (`now(&core, &f)` plus `Silence::look`, inside one
  `theseus_store::blocking`), wakes on a settle (`rt.job_waits.on_settle()`), on a steer, on a keep-warm read, or on a
  sleep of `left.min(look)`, where `look` is `super::waits::LOOK` or, while it watches for silence, the time until the
  beat (`s.until(beat)`, floored at 1 ms) if that is sooner. The beat arm:
  `if let (Some(s), false) = (silent, left.is_zero()) { match theseus_store::blocking(|| clock.beat(&core, &f, s,
  asked, reads)) { Some(out) => return Ok((out, None, None)), None => held = true } }`. `None` means the job's output
  passed its file's head (`peek.held`): not silence, no beat.
- **`Found`** (near :67): `corr`, `name` (the short id, `j1`), `argv`, `action: Action` (the job's own action: its
  `session_id`, `execution_id`, `dispatched_at_ms`, `planned_at_ms`). `Found::called()` gives
  `j7 (cargo test --workspace)`.
- **The facts:** `crates/theseus-core/src/toolrun/jobs_tools/wait_facts.rs` (302 lines; declared by `mod wait_facts;`
  in `jobs_tools.rs` near :36). `Silence` (`start`, `look`, `quiet`, `until`, `grew`, `path`), `Facts { called,
  ran_ms, waited_ms, beat_of, cpu_usec, peek, quiet_secs }`, `text(&Facts)` (the model's lines, including
  `CPU {cpu_s} s in {ran_s} s, about {x:.1} CPU.` only when there is a reading and `ran_s > 0`), `Clock { began,
  ran0_ms }` with `start(&Action)`, `said(..)` and `beat(..)`, and the private `read(..)` that builds `Facts` (the
  peek of the spool result file, `theseus_kernel::cgroup::job_cpu_usec(&f.corr)`, the quiet seconds).
- **The last line:** `crates/theseus-core/src/toolrun/peek.rs` `pub fn last_line(path: &Path) -> Option<String>`:
  the last complete line, a progress bar's last rewrite, escapes and control characters dropped, at most 4 KiB read.
  Use it; do not write another tail reader.
- **The look:** `crates/theseus-core/src/toolrun/waits.rs` `pub const LOOK: Duration = Duration::from_secs(1);`
  ("The backstop: a waiting turn looks at its job at least this often."). It is used by the `job_wait` loop above,
  by a synchronous `proc.run`'s wait (`toolrun/job.rs` near :356, `let mut nap = left.min(super::waits::LOOK);`)
  and by `toolrun/steps.rs` near :175. `waits.rs` is 190 lines.
- **How other notices reach a place:** the outbox (`crates/theseus-core/src/outbox.rs`). `Outbox::target(session_id)`
  is where a session's posts go (its place, or for a task where it reports); `Outbox::post_for(session_id,
  execution_id, body) -> Result<Option<Action>>` writes a post in a frame of its own when the session has a place and
  returns `Ok(None)` when it has none. The budget notice is the model to copy for the body
  (`turn/budget_step.rs`, `fn notice`: `json!({"kind": "notice", "text": text})`; it stages into the turn's frame
  because it runs inside a turn step; a tool's wait is not a turn step, so use `post_for`). The `notice` kind is not
  in `pacer::UNPROMPTED` (it is an answer to work the person started, not an unprompted post), and the Discord
  courier already renders it (`theseus-discord/src/courier.rs`, the `"notice"` arm). Tests read posts with
  `core.outbox.open_for("discord:dm:7")` filtered by `crate::outbox::kind_of(a) == "notice"` and
  `crate::outbox::body_of(a)` (see `tests_stopping.rs` `fn notices`, and its place binding near :121-130:
  `core.outbox.bind_place("dm:7", &sid)` plus `core.runner.place_rule.bind_one(..)`).
- **The beat's tests:** `crates/theseus-core/src/tests_wait_beat.rs` (165 lines), a child of `tests_wait_cost.rs`
  (`#[path] mod beat;`), on tokio's paused clock with the test holding the jobs (`Gated`), `rig(script(n),
  beat(600))`, `print(&r, &corr, text)` writes into the spool result file, `r.result(&sid, "t2")` reads a call's
  result. `a_silent_jobs_wait_returns_at_the_beat_with_what_it_saw`, `a_job_that_prints_keeps_its_wait`,
  `the_beat_off_is_todays_wait` (exact text and key set with `beat(0)`).
- **Config:** `[tools] job_wait_beat_secs` (`config.rs` `ToolsConfig.job_wait_beat_secs`, default
  `config/limits.rs` `default_job_wait_beat_secs()` = 600, checked by `limits::check_job_wait`; template line
  `crates/theseus-core/config/theseus.example.toml:479`; doc `crates/theseus-core/AGENTS.md:704`). Leave its default,
  range and tests exactly as they are: the owner keeps the beat on.
- **Scrubbing:** `core.tools.scrubber` (`toolrun.rs` `pub scrubber: Arc<Scrubber>`; `scrub(&str)` returns the scrubbed
  text and the redactions). A tool's result is scrubbed on its way to the model (`toolrun.rs` near :810); an outbox
  post is not scrubbed for you. A job's last output line can carry a secret value, so the status line is scrubbed
  before it is posted.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `job-wait-status` | theseus-5d204 | at each beat, one `notice` post to the session's place with the job's facts, no model call; a switch as its rollback; a test pinning `waits::LOOK` at 60 s or less | `toolrun/jobs_tools/wait_status.rs` (new), `toolrun/jobs_tools.rs` (one `mod` line, one call in the beat arm), `toolrun/jobs_tools/wait_facts.rs` (only if a fact must be made reachable: a `pub(super)` accessor, no change to `text`), `toolrun/waits.rs` (a `#[cfg(test)]` test), `config.rs` + `config/limits.rs` (the switch), `theseus.example.toml` (one line), `AGENTS.md` (one sentence), `tests_wait_beat.rs` (new tests appended; existing ones untouched) | S |

## What to build
1. **The status line.** A new module `crates/theseus-core/src/toolrun/jobs_tools/wait_status.rs` (declared with one
   `mod wait_status;` line beside `mod wait_facts;`). It builds one line from facts the beat already gathered, plus
   the last output line, and posts it:
   - The line, one line, no newline in it, facts only (it is owner-facing text, but keep the house's plain fact
     style): the job (`Found::called()`, cut to about 80 characters with an ellipsis), how long it has run, how long
     it has printed nothing, its CPU share when there is a reading (`about 1.0 CPU`, computed as `text` computes it:
     CPU seconds over run seconds, only when the reading exists and run time is above 0 s), and its last output line
     in quotes, cut to about 160 characters, or that it has printed nothing yet. For example:
     `Job j3 (cargo test --workspace) is still running: 10 min so far, no new output for 10 min, about 3.9 CPU. Last line: "test store::tests::compact ... ok"`
     and with no cgroup and no output:
     `Job j1 (bash -c ./search.sh) is still running: 25 min so far, no output since it started.`
     Durations through `narrative::duration` (as `text` does). The silent span is `Silence::quiet()` (the same value as
     the result's `silent_secs`), the run time is the beat's `ran_ms`.
   - The last line from `peek::last_line(path)` on the spool result path (`Silence::path()`), never a second reader.
     A line that is empty after `trim` counts as none.
   - The whole line through `core.tools.scrubber.scrub(..)` before it leaves.
   - Posted with `core.outbox.post_for(&f.action.session_id, &f.action.execution_id, json!({"kind": "notice", "text":
     line}))`, inside `theseus_store::blocking` (it writes a frame). A session with no place posts nothing
     (`Ok(None)`). An error is logged with `tracing::warn!` and swallowed: the wait's result never depends on the
     post.
   - One call in the beat arm of `wait`, in the `Some(out)` branch only, just before its `return`. The `None` branch
     (held output, not silence) posts nothing. The still-running result at `left.is_zero()`, the steer return and a
     job's end post nothing. The beat's computed facts should be reused, not read twice: the simplest shape is for
     `Clock::beat` to keep returning `Option<ToolOutput>` and for `wait_status` to take what it needs from `Clock`,
     `Silence` and `Found` (add a small `pub(super)` accessor on `Clock` for the run time if you need one). Do not
     change `wait_facts::text`, the beat result's text or its meta keys: `the_beat_off_is_todays_wait` and the beat
     tests pin them.
   - No model call, no new model-facing text, no new tool, no change to any tool description. The line is not added to
     the conversation the model reads.
2. **One line per beat.** Each beat that fires posts exactly one line. A wait that returns at the beat and a next
   `job_wait` that beats again post two lines in total, one per beat. Nothing else is rate-limited or batched.
3. **The rollback switch.** `[tools] job_wait_status` (bool, default `true`; `ToolsConfig.job_wait_status`, a
   `default_job_wait_status()` in `config/limits.rs` beside `default_job_wait_beat_secs`, the field in `Default`, one
   template line under `job_wait_beat_secs` in `theseus.example.toml` with a short comment: "at each beat, one status
   line to the session's place (no model call)"). `false` posts nothing and leaves the beat itself as it is. The beat
   off (`job_wait_beat_secs = 0`) posts nothing either, since no beat fires. If the `pub use limits::{..}` list in
   `config.rs` needs the new default, add it there (that list is a reflowed line; keep the edit to that one item).
   If a config golden (`resolved_profiles` or a template test) lists the `[tools]` keys, update it for the new key
   and say which in the report; that is a new key, not a weakened assertion.
4. **The look, pinned.** A `#[cfg(test)] mod tests` in `toolrun/waits.rs` (or a test in `tests_wait_beat.rs` if you
   prefer, it is the same rule) with `the_waiters_look_is_a_minute_or_less`: `assert!(LOOK <= Duration::from_secs(60))`
   with a message that quotes the rule (the owner's "checker cycle 60 seconds or faster", theseus-5d204). Keep
   `LOOK` at 1 s.
5. **AGENTS.md:** one sentence after the beat's paragraph (`crates/theseus-core/AGENTS.md` near :704): at each beat
   the session's place gets one status line (`[tools] job_wait_status`, on; no model call), and `waits::LOOK` stays
   at 60 s or less (a test pins it).

## Tests (each with a planted revert; the first is the key test)
Put the wait tests in `tests_wait_beat.rs` (append; the rig is `super::*`). A session bound to a place: copy the
binding from `tests_stopping.rs` (`bind_place("dm:7", ..)` plus `place_rule.bind_one(..)`), and read posts with a
`fn statuses(core) -> Vec<String>` helper over `open_for("discord:dm:7")` and `kind_of == "notice"`, body `text`.
- **`a_silent_jobs_beat_posts_one_status_line_to_its_place`** (the key test): a bound session, `script(1500)`,
  `beat(600)`, the job prints `round 1\nround 2\n` at the start (with `print`) and nothing after. After the turn ends,
  exactly one status notice exists; it contains the job's short id (`j1`), `still running`, a run time of about
  10 min, `no new output`, and `"round 2"`; it has no newline. The model's requests are exactly as many as the beat
  test without a place (no extra model call: compare `r.turns.requests().len()` with the plain beat test's count),
  and the next request's messages do not contain the status line's text. Plant: remove the call in the beat arm. It
  must fail.
- `no_status_line_without_a_beat`: a job that prints every 100 s keeps its 900 s wait and posts nothing; the same
  silent job with `beat(0)` posts nothing; with `job_wait_status = false` the beat still returns at 600 s (same text
  as with it on, byte for byte) and posts nothing. Plant: ignore the switch (always post). It must fail.
- `each_beat_posts_its_own_line`: the model asks `job_wait` twice on the same silent job (script the second call),
  each returns at its beat, and there are exactly two status notices. Plant: post only on the first beat of a job
  (or twice per beat). It must fail.
- `a_held_job_posts_nothing` if the rig can make `peek.held` true cheaply; else say so in the report.
- `the_status_line_is_scrubbed`: a job whose last line carries a secret value the scrubber knows (register one in the
  rig's scrubber the way other scrub tests do; search `scrubber` in `crates/theseus-core/src/tests_*.rs`) posts a
  line without the value. Plant: skip the scrub. It must fail.
- `an_unbound_session_posts_nothing`: the plain beat test's session (no place) gets its beat result and no post, and
  no error.
- Unit tests in `wait_status.rs` for the line builder: CPU share shown only with a reading and run time above 0 s;
  no output gives `no output since it started`; a long command and a long last line are cut with an ellipsis; the
  line never holds a newline (a last line with `\r` rewrites gives the last rewrite). A pure formatter needs no plant
  beyond these asserts; say so.
- `the_waiters_look_is_a_minute_or_less`: plant `LOOK = Duration::from_secs(61)`. It must fail. Restore 1 s.
- The existing beat tests (`tests_wait_beat.rs`), `tests_wait_cost.rs`, `config/tests.rs`
  `job_wait_beat_secs_has_a_default_and_a_range` and `tests_first_request.rs` pass unchanged.

Run the focused tests with `cargo nextest run -p theseus-core -E 'test(/wait_beat|wait_cost|waits::|wait_status|job_wait/)'`
(or the closest filter your nextest takes), then `cargo nextest run -p theseus-core` and `cargo clippy -p theseus-core
--all-targets -- -D warnings`. None of these needs L1, a cgroup or root: the CPU share shows only where a cgroup
reading exists, so cover it with the unit tests.

## Not in this row
- The beat's default, range, early-return rule, its result text and meta: unchanged (the owner keeps it on at 600 s).
- Any change to `job_wait`'s tool description (a separate, owner-decided item), and any new model-facing text.
- A status line for a synchronous `proc.run`'s wait, for `term` or for a job's end: only the `job_wait` beat posts.
- Pacing, batching or a buzz rule for these notices, and any Discord rendering change (the courier's `notice` arm
  renders it as is). If the line renders badly in a place, say so in the report.
- `docs/status.md`, the README, `docs/design/` and the spec: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. Its rollback is `[tools] job_wait_status
  = false`: a test shows that with it off the beat's result is byte-for-byte the same and nothing is posted.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the path
  before a request leaves, and nothing on the start path grows with history. The post happens only at a beat.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. This row edits no budget
  and no keel file, and no existing test's assertion (new tests are appended; a golden that lists config keys may gain
  the new key, say which). Before your last commit run `python3 scripts/keel-guard.py --base origin/integ/harness
  --head HEAD --quiet` and paste its output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. **Store format: unchanged**
  (47 on your base, `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). An outbox post of an existing kind
  is not a layout change. If you find you must store something new, stop and say why in the report instead.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a post, a report, a push) through `core.tools.scrubber`. No person's name anywhere in
  the repo, in commit messages or in the report: write "the owner". Fixture paths and names are invented.
- Line ceilings (`scripts/long-files.txt`): none of this row's files is listed (`jobs_tools.rs` 656 lines,
  `wait_facts.rs` 302, `waits.rs` 190, `tests_wait_beat.rs` 165). Put new code in the new module; in
  `jobs_tools.rs` add only the `mod` line and the call. Never raise a ceiling.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md` at least). Keep
  CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud (L1 needs a non-root daemon
  with a job cgroup: about 33 L1 tests fail on this VM; this row needs none of them, so say which suites you ran).

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: what changed, file by file; the status line's exact text for the key test's job; each test
and each plant with the test that failed on it; the commands you ran and their results; anything you could not run
on the VM and the exact commands for the maintainer (a live check on the owner's machine: a `sleep 660` job under a
delegated unit, its place getting one line with the CPU share at 600 s); the keel guard's output; the store format
(unchanged, 47, or why not); and follow-ups (what `docs/status.md` and the spec should say). Push, and end with a
short final message that starts `CLOUD REPORT COMPLETE`.
