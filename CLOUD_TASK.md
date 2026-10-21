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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-bg-rest`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: cn-bg-rest, background model calls obey the per-request data rule: compaction, consolidation, keep-warm and turn.replay (theseus-a1wvq.10)

Branch: `cloud/20261021-cn-bg-rest`. Every commit's subject carries `theseus-a1wvq.10`. Deadline for the report: 4
hours after you start.

## Your base: `integ/cn` after its round 2 (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. Where the note above says your base is
`main`, or that the store format is 47, this section is right. `integ/cn` is `main` plus the China-origin rows, each
its own merge:
- the data floor and the session's data state (theseus-a1wvq.3, .5): `crate::data` (`DataPass`, `DataWhy`,
  `Clearance`, `DataState { opened, run, taint }`, `OpenRun { from, through }`, `Taint`), `data_gate.rs` (the
  chokepoint: every provider is wrapped by `data_gate::guard`, and a request to a model of restricted origin, `cn` or
  `unknown` by `Config::profile_origin(..).restricted()`, is refused unless `ProviderRequest.data_pass` covers its
  digest; the refusal writes a `data_gate.blocked` row), `turn/data_step.rs`, store format 48;
- routing's compose (.4), open tasks (.6), the open view (.7: `open_view.rs`, `TurnCtx.open`);
- this row's first slice (lane `cn-bg-mark`): every background provider call wraps its provider with
  `google::private::{for_session, over_sessions, ..}` so a private session's content is held by the floor; and lane
  `cn-private-follow` (the mark follows recalled nodes; `Relay::new` refuses a restricted profile). **Not yours.**
- **cn-open-request (theseus-a1wvq.8)**, whose contract this row builds on. Clearance is per request:
  - `data::admits(..) -> Result<OpenRun, DataWhy>`, `data::admits_again`;
  - `RouteState.admitted` (type `open_step::Admitted`, `.0` is the `Result`);
  - `data::Writer { Loop, ToolResult, Other }`, `data::Carry { id, at, writer }`,
    `data::Carried { open_view, from, nodes }` and `data::clearance(run, &Carried) -> Clearance`, the only cleared
    `Clearance`; `Clearance::pass(digest)`;
  - in `turn/open_step.rs`: `writer(n, turn)` (who wrote a node, for a request of this turn), `carried`,
    `data_pass` (each loop's pass, kept in `Turn.data: Held`, `Held.requests`), `data_move` (a later loop leaving the
    run moves to the `[model]` default with `data_gate`), `data_end` (the run is stored only when every request went
    out cleared and only the turn's own loop wrote after the run's last node; an open session's first refusal is its
    taint).

**Check first**:
- `git log --oneline | grep -E 'theseus-a1wvq\.8|theseus-39dj0|theseus-a1wvq\.10'` shows `.8`'s commit, the
  private-follow commit and the `cn-bg-mark` commit;
- `grep -n 'pub fn clearance\|pub struct Carried\|pub enum Writer' crates/theseus-core/src/data.rs` finds the three;
- `grep -n 'fn writer\|fn carried\|fn data_end\|pub(super) struct Held' crates/theseus-core/src/turn/open_step.rs`
  finds them.

If `.8`'s commit, `clearance`, `Carried` or `writer` are missing, stop: write that in `CLOUD_REPORT.md`, push it, and
end. If the join renamed any of them, use the code's names and say which in the report's first section.

**Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48). **This row bumps
nothing.** No stored record gains a field; every word this row adds goes in an existing row's text (a `why`, a
reason) or a returned error.

## Why
The chokepoint already fails closed: no background call can send a session's text to a China-origin model without a
pass. But four callers send to such a model anyway, get refused, and fail in ways a person sees:
- **Compaction breaks an open task's long turn.** An open task on `dsflash` (restricted) passes the window.
  `turn/compaction.rs` `request_for` sets `data_pass: None`, so with `summary_profile = "session"` the summary call
  to `dsflash` is refused: the ring stands, with a `data_gate.blocked` row. When a summary is written (a named Claude
  `summary_profile`), the `Summary` node is `Writer::Other` in `open_step::writer`, so the next request is
  `unvetted`, refused, and the task's run is never stored again.
- **Consolidation fails every night.** `synth_profile` with `"session"` takes the sources' sessions' `last_target`:
  `dsflash` for an open task's session. `synthesize` sends with no pass and the cluster is `failed`; likewise a named
  restricted `synth_profile`. One request holds many sessions' text, so no pass can ever cover it.
- **Keep-warm** builds the session's next compile in its private view for its `last_target`. A DM session last on
  `dsflash` (an open detour) is refused, `stop`s, and logs a blocked row.
- **`turn.replay` to a restricted profile** (`replay_turn_with`) rebuilds a past turn's private-view request and is
  refused at the chokepoint after the rebuild, with no words for the owner.

The owner's rule (design of 2026-10-17, D1): "Compaction may use a restricted target only for an open session whose
dropped range lies in its run, with its own pass. Restricted consolidation always falls back. Keep-warm skips a
restricted target, and turn.replay to one is refused." A pass covers one request, so this is simpler and fails
closed.

## Risk
Medium in kind (a session's text and a China-origin model), small in size. Every new branch only ever sends less to
a restricted model than today, or sends a request the existing `data::clearance` cleared. The chokepoint stays the
last check and is untouched.

## Prerequisite: `integ/cn` with `.8` and `cn-private-follow` merged (not `main`), as "Your base" says.

## Your row

### 1. Compaction (`turn/compaction.rs`, logic in a new `turn/compaction/gate.rs`)
Put the logic in a new file, `crates/theseus-core/src/turn/compaction/gate.rs`. Declare it with one `mod gate;` line
in `compaction.rs`, **not** in `turn.rs`: `turn.rs` is near its ceiling, and a sibling row adds a line there. In
`compact`/`summarize`, add only the calls.

**(a) The range's clearance.** Compaction is gated only when the turn is admitted in the open view:
`t.route.admitted.0` is `Ok(run)` and `t.tc.open` is `Some`. That holds for every turn that may reach a restricted
model, and for a Claude turn in an open session (see (c)). Then:
- Build a `data::Carried` for the dropped range:
  - `open_view: true`;
  - `from`: `run.from`'s transcript position;
  - `nodes`: every transcript node the summary request renders, each as a `Carry` with `open_step`'s writer, **plus
    the run's last node (`run.through`)**, which the request about to go carries anyway.

  Adding `run.through` lets the existing `data::clearance` judge the range unchanged: every node at or after the run's
  first, and every node past the run's last written by this turn's loop. Do not add a second clearance function to
  `data.rs`; a sibling row edits that file.
- `cleared = data::clearance(run, &carried)`.

**(b) Where the summary goes.** Let `summary` be the summary's target (`own` for `session`, else the named profile)
and `restricted(p)` be `self.restricted_profile(p)` (`data_step.rs`).
- `restricted(summary)` **and** cleared: the call goes to it with `request.data_pass =
  cleared.pass(&request.digest())`, set on the final request just before `stream_message`. The pass never changes
  the digest.
- `restricted(summary)` and not cleared (or not admitted in the open view):
  - If the turn's own target is restricted, no summary is made. A summary written elsewhere would ride the
    restricted request. The ring stands, through the existing `Fallback`/`rang` path, and its `why` says `data_gate:
    <word>` with `DataWhy::as_str` (for a turn not admitted in the open view, `private_view`).
  - Otherwise (a Claude turn and a named restricted `summary_profile`), the summary is written with the `[model]`
    default: the profile `turn/data_step.rs` `DEFAULT_PROFILE` names (make that constant `pub(crate)`: a one-word
    hunk). The `context.compacted` row and the summary's header name the profile that ran, and the row's text says
    `data_gate: <word>`. If the default does not resolve or is itself restricted, the ring stands and says so.
- Not `restricted(summary)`: as today.

**(c) The summary joins the run only when its range was cleared.** When (a) cleared the range, whichever profile
wrote the summary, push the new `Summary` node's id onto a new field `Held.summaries: Vec<String>` (`open_step.rs`).
In `open_step::writer`, add one parameter (`summaries: &[String]`) and one arm: a `Kind::Summary` whose id is in it
is `Writer::Loop`. Pass `&t.data.summaries` at its three callers (`carried`, `data_move`, `data_end`). Then
`data_pass` clears the compaction's request, and `data_end` stores the run through the turn's last node.

A summary of a range that was **not** cleared stays `Writer::Other`, even when a Claude profile wrote it. This
matters most for a Claude turn in an open session: its summary may fold in a tool result. If that summary counted as
the loop's, `data_end` would store a run through it, and the next turn would carry it to `dsflash`. Do not add a
`Kind::Summary` arm keyed on the turn id alone.

**Shared function:** cn-tool-screen (theseus-a1wvq.9) runs beside you from the same base. It edits
`open_step::writer` too: a screened tool result becomes `Writer::Loop`. Keep your change to that function to the
parameter and the one arm. Do not restructure it. Say in the report's first section that the join takes both arms.

**(d) The row.** No new frame and no new fact on a plain turn. A turn that is not admitted in the open view, with a
summary profile of US origin, runs exactly as before (one `match`, no transcript read).

### 2. Consolidation falls back (`consolidate/run.rs`)
Restricted consolidation always falls back. One request holds text from many sessions, and one pass covers one
compiled request of one session, so there is nothing to clear it.
- In `synth_profile` (or right after it in `consolidate_plan`), when the chosen profile is restricted
  (`self.cfg.profile_origin(p, &<the catalog the runner reads>).restricted()`), use `DEFAULT_PROFILE` instead.
  If that is restricted too, or does not resolve, the cluster is skipped with the word `restricted_origin` (through
  the existing `skip(&mut r, why)` path), and no request goes anywhere.
- A dry run reports the fallback profile in `report.profile`, so the owner sees where the synthesis would go.
- The cluster's report says why in its existing `why` field: "synth_profile `<p>` is of restricted origin; written
  with `default`".
- The private mark (`google::private::over_sessions`) stays exactly where it is.

### 3. Keep-warm skips a restricted target (`keep_warm.rs`)
In `keep_warm_prepare`, right after `keep_warm_request` builds `b`, check whether `b.target.profile` is restricted.
If it is, forget the session (`kw.forget`) and return `Outcome::Forgotten("its model `<model>` is of restricted
origin: a keep-warm read carries no pass")`. No request is sent and no `data_gate.blocked` row is written. A person's
next message starts its window again as before, and if the session's next target is of US origin it is kept warm.
If `tools_count_keep_warm` takes an open word, count it as `restricted`; if not, leave it.

### 4. `turn.replay` to a restricted profile is refused in words (`own_bench/replay.rs`)
In `replay_turn_with`, right after `resolve_target` and before the rebuild, refuse a restricted target the way
`Crossing` is refused (the same error type or a sibling of it, so the RPC maps it the same way):
"not replayed on `<profile>`: `<model>` is a model of <origin> origin, and a replay carries no pass for a session's
text". `<origin>` is `ModelOrigin`'s word (`cn` or `unknown`). This covers both scopes (`first_loop` and
`whole_turn`). The rebuild does not run, no request is sent, and no `turn.replayed` row is written.

### 5. Docs
In `crates/theseus-core/AGENTS.md`, in the passage "The open request, per request", add three sentences:
- a compaction in an open request summarizes only a range its run clears, with its own pass, and only such a summary
  joins the run;
- consolidation never runs on a restricted model;
- keep-warm skips a restricted target, and `turn.replay` refuses one.

## Tests (each with a planted revert; the first is the key test)
Put the turn-level tests in a new `crates/theseus-core/src/tests_bg_gate.rs` (one `mod` line in `lib.rs`). Build its
rig from `tests_compaction.rs`'s (`rig`/`build_with`, its small `WINDOW` via a catalog row) and `tests_open_request.rs`'s
config:
- `c.copy_profile("dsflash", "glm")`, of restricted origin;
- `c.routing.classes.trivial.cn_profiles = vec!["dsflash".into()]`;
- its `open_session` shape: `profile.use dsflash` in an open session;
- **do not** call `serve_glm_unrestricted`.

Copy the few lines you need rather than editing `tests_open_request.rs` or `tests_tool_screen.rs`: sibling rows edit
those files. Append the consolidation, keep-warm and replay tests to their own suites (`tests_consolidate.rs`,
`tests_keep_warm.rs`, `own_bench/tests.rs`), next to the `cn-bg-mark` tests that already use the same rigs. Never add
a real-looking key or name: fixtures are invented ("Acme", "Widgetco").

1. **T23a `an_open_tasks_compaction_carries_its_own_pass`** (key). An open session on `dsflash` with `summary_profile =
   "session"` and a window small enough that loop 0 rings after a few turns. All of its turns are no-tool answers, so
   the run is whole. Assert:
   - the restricted fake receives the summary request, and its `data_pass` covers that request's digest;
   - a `Summary` node is written;
   - the compaction's request goes to `dsflash` with a pass;
   - no `data_gate.blocked` row is written;
   - after the turn, the session's stored `run.through` is the turn's last node, and its next turn is admitted on that
     run (`admits_again`).

   Plant A: leave the summary's `data_pass` unset. The chokepoint refuses it, the ring stands, a blocked row appears,
   and the test fails. Plant B: drop the `Kind::Summary` arm in `writer`. The compaction's request is `unvetted` and
   refused, and the test fails. Show both failing.
2. **T23b `a_restricted_summary_profile_in_a_private_session_falls_back`.** A DM session on a Claude profile in the
   private view, with `summary_profile = "glm"` (restricted). When it rings:
   - the `glm` fake receives zero requests;
   - the summary is written by the `[model]` default's fake;
   - the row names `default` and says `data_gate: private_view`;
   - no blocked row is written.

   Plant: skip the fallback. The `glm` call is refused, the ring stands with a blocked row, and the test fails.
3. **`a_summary_of_an_uncleared_range_never_joins_the_run`.** This is the laundering case of 1(c). Use an open session
   on a Claude profile, admitted. Put a node the loop did not write past the run's last node (a `Writer::Other` node:
   `tests_open_request.rs` `an_unknown_writer_breaks_the_chain` shows how to plant one), then make the turn ring.
   Assert:
   - a summary is written (Claude may see it);
   - its id is not in `Held.summaries`;
   - the session's stored run does not reach past the planted node;
   - the next turn is not admitted (`unvetted`).

   Plant: make `writer` treat every `Kind::Summary` of the turn as `Loop`. The run is then stored through the
   summary, and the test fails.
4. **`range_clearance_matrix`** (pure, in `turn/compaction/gate.rs`'s tests), one assertion per case:
   - a range wholly in the run is cleared;
   - a range with a node before `run.from` is `not_in_run`;
   - a `ToolResult` past `run.through` is `tool_result`;
   - an `Other` node past it is `unvetted`;
   - a range carried outside the open view is `private_view`;
   - a range of only this loop's nodes past `run.through` is cleared.

   Plant: return cleared first thing. The matrix fails.
5. **T23c `a_restricted_synthesis_falls_back`** (`tests_consolidate.rs`). Make every source session's `last_target`
   `dsflash`, with `synth_profile = "session"`. Assert:
   - the restricted fake receives zero requests;
   - the cluster is synthesized by `default`, `report.profile == "default"`, and its `why` says restricted origin;
   - a dry run reports `default`.

   Repeat with a named `synth_profile = "glm"`. Then make `[model]` itself restricted: the cluster is skipped as
   `restricted_origin`, and no fake gets a request. Plant: drop the fallback. The cluster is `failed`, a blocked row
   appears, and the test fails.
6. **`a_restricted_target_is_never_kept_warm`** (`tests_keep_warm.rs`). A DM session whose last target is `dsflash`,
   due. `keep_warm_once` returns `Forgotten` with the restricted-origin words. The fake receives zero requests, and no
   blocked row is written. A session of US origin beside it is still read. Plant: drop the check. The outcome is
   `Stopped`, with a blocked row.
7. **`a_replay_to_a_restricted_profile_is_refused_in_words`** (`own_bench/tests.rs`). Replay a past Claude turn to
   `dsflash`, in both scopes. Each returns the refusal with `restricted origin` in its text. The fake receives zero
   requests, and no `turn.replayed` row and no blocked row are written. A replay of the same turn to a US profile
   still succeeds. Plant: drop the check. The chokepoint's error and a blocked row appear instead, and the test fails.

Unchanged and passing: `tests_compaction.rs` (it serves glm unrestricted), `tests_consolidate.rs`,
`tests_keep_warm.rs`, `own_bench`, `tests_private_background.rs`, `tests_open_request.rs`, `tests_data_floor.rs`,
`tests_data_gate.rs`, `tests_open_view.rs`, `tests_open_task.rs`, `tests_first_request.rs`, `tests_route.rs`, and
`tests_m3::a_plain_turn_stays_within_its_frame_budget`.

## Files (and the rows beside you)
Yours:
- `turn/compaction.rs` (the `mod gate;` line and the calls);
- `turn/compaction/gate.rs` (new);
- `turn/open_step.rs`: only `Held.summaries`, `writer`'s parameter and arm, and its three callers' arguments;
- `turn/data_step.rs`: only `DEFAULT_PROFILE`'s visibility;
- `consolidate/run.rs`, `keep_warm.rs`, `own_bench/replay.rs`;
- `tests_bg_gate.rs` (new);
- appended tests in `tests_consolidate.rs`, `tests_keep_warm.rs` and `own_bench/tests.rs`;
- `lib.rs` (one `mod` line), `AGENTS.md`.

Beside you, from the same tip: **cn-tool-screen** (.9: `data.rs`, `turn/screen_step.rs`, `RouteState.screened`,
`open_step::writer`, `tests_tool_screen.rs`, `turn.rs`) and **cn-observability** (.11: `record_route` in
`route_step.rs`, footers, `fact/route.rs`, protocol `route.rs`, the metric `Telemetry::record_data_gate`, the own
bench's columns, `tests_observability.rs`). Stay out of their files except `writer` (1c). **Do not add the metric**
(not on your base): list your four skip sites in the follow-ups with the `what` word each should record
(`compaction`, `consolidation`, `keep_warm`, `replay`) and its `why`, so the join adds the calls.

Line room: none of your files is in `scripts/long-files.txt`, so each stays at 2,500 lines or fewer (all are under
1,000 now). List `wc -l` of each in the report.

## Not in this row
- What landed already (lanes `cn-bg-mark` and `cn-private-follow`, "Your base"): do not touch it.
- The other background callers: initiative, learning audit and writer, people, cards, naming, the census, and the
  thread import's card. With a restricted profile they are already refused at the chokepoint, which writes its
  `data_gate.blocked` row. Their fallbacks are a follow-up if the owner ever configures a restricted profile for them.
- Screening tool results (theseus-a1wvq.9). Once it lands, a screened result is `Writer::Loop`, so a range holding one
  clears with no change here.
- The metric and the footers (theseus-a1wvq.11), and Jev judging replies (theseus-a1wvq.15).
- A replay of an open turn to a restricted profile with a pass of its own. It is refused for now; a follow-up if the
  owner wants China-origin models in the own bench.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing, late or unparsable is closed, never open. A profile whose origin
  does not resolve is `unknown`, which is restricted. Never loosen a check, a threshold, a test or a ceiling to make a
  test pass.
- **On, measured, with a named rollback.** The behaviour ships on by default. The rollback is the config: with no
  restricted profile in any `cn_profiles`, no open task on one, and no restricted `summary_profile`/`synth_profile`,
  none of the new branches runs, and every existing test passes unchanged.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new
  on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's
  turn bench must not move). The range check reads the transcript only for a turn admitted in the open view that rang.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The key test must be shown failing on both
  of its planted reverts.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `git fetch origin integ/cn` and then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet`
  and paste its output in the report. It must be empty. (A `main` base would flag the earlier rows' changes as yours.)
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name, company or private
  project name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture names and paths
  are invented ("Acme", "Widgetco").
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
- The lane gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on your last commit (run
  `cargo nextest run -p theseus-core` first). Say what it printed.

## Your report
Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, never a shell heredoc, and check
after the commit that it is not empty. Its sections, in order:
1. The names as built: `Held.summaries`, `writer`'s new parameter, the gate module's functions, the refusal's error
   type. Name any of `.8`'s names the code had under another name, and say plainly that `open_step::writer` is shared
   with cn-tool-screen (both arms are kept at the join).
2. What changed, file by file, with each file's line count.
3. Each test and its plant: the line broken, the failure seen, the restore.
4. What could not run on the VM, with the exact commands for the maintainer.
5. The keel guard's output.
6. "store format: unchanged (48)".
7. The maintainer's live check after the join, on a scratch daemon. Set `trivial.cn_profiles = ["glm"]`, run an open
   task on glm with a long brief so it compacts, and set `synth_profile = "session"`. Check that:
   - the summary call goes to glm with a pass;
   - the task's run is stored;
   - `memory.consolidate` writes with `default`;
   - `turn.replay` to glm is refused in words.

   Name the rows to read.
8. The follow-ups: the four `record_data_gate` call sites for the join with cn-observability, the other background
   callers' fallbacks, and the replay with a pass.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
