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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-cn-reply`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the integration branch `integ/cn` at `2d30cc48` (2026-10-22: the China-origin chain after its
round 4: the open request, cn-tool-screen, cn-observability, cn-bg-rest, cn-draft-review, cn-private-authored); that
branch, not `main`, is your base, and your row joins `integ/cn` round 6. Line numbers quoted below are at `2d30cc48` but
may still be off by a few: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`; `main` is at 49 because
its join made the session's data state a format bump). This row must add **no stored field**: keep what it needs in a
META key or a ledger row. If you find you cannot, stop and say so in the report's first section.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (add nothing)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (add nothing)
- `crates/theseus/src/render.rs` 3,119 / 3,128
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section.

**Work in flight beside you** (round 5 of `integ/cn`, both from this same base; stay out of their files, and where you
can't, keep your edit small and additive so the joins merge):
- `cn-config-guidance` (theseus-a1wvq.12, branch `cloud/20261021-cn-config-guidance`): owns the template
  `crates/theseus-core/config/theseus.example.toml` (all of it), `crates/theseus-core/src/own_bench/`, protocol
  `own_bench.rs`, CLI `render/own_bench.rs`, `docs/privacy.md`, `docs/status.md`'s China-origin lines, and core
  `AGENTS.md`'s China-origin and own-bench passages. **Touch none of them.** Put any template or docs sentence your row
  needs in your report for the integrator.
- `tail3-cn` (branch `cloud/20261021-tail3-cn`): `open_view.rs`, `task_graph/tools.rs`, `data.rs` (`screen`),
  `fact/data.rs`, `keep_warm.rs`, `toolrun/batch.rs`, `turn/{screen_step,standing_step,data_step,compaction/gate}.rs`.
  Read `git diff origin/integ/cn origin/cloud/20261021-tail3-cn -- <file>` before editing any of these.
- Your task's own "Rules for this row" and "Not in this row" sections name the other row of this wave beside you.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail4-cn-reply, Jev judges each Theseus reply's data so an open run survives the reply (theseus-a1wvq.15)

Branch: `cloud/20261021-tail4-cn-reply`. Every commit's subject carries `theseus-a1wvq.15`. Deadline for the report: 4
hours after you start.

**Why.** In the owner's DM, a China-origin request (the trivial class's fallback) carries only the newest open
message(s): every reply Theseus wrote, on a Claude model in the private view, ends the open run, because the run
extends only when the node just before the input is the stored run's last node, and a reply is never judged. The
owner's answer Q5 (2026-10-17): **the open run includes Theseus's own replies when Jev's data question judges each
reply open at >= act**, the same test as a person message. A reply judged private or unclear, or not judged in time,
ends the run.

**Risk: medium (privacy).** This widens what may reach a restricted-origin model. Every new path must fail closed: a
missing, late, private, unclear or below-act answer is a run-ender, and nothing waits on the send path.

**Prerequisite: base `origin/integ/cn` at `2d30cc48`.** Your branch joins `integ/cn` round 6. Run the keel guard against
`origin/integ/cn`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-cn-reply` | theseus-a1wvq.15 | ask the data question of assistant replies, batched, off the turn path; a reply judged open at act extends the stored run; any other answer, or none, ends it | a new `crates/theseus-core/src/data/reply.rs` (or `reply_data.rs`: the asking, the batch, the write) + one `mod` line; `crates/theseus-core/src/data.rs` (`admits`: only if the chain rule must read a reply's answer; prefer extending the stored run when the answer lands, so `admits` is unchanged); a pack for the question (a new `crates/theseus-judge/packs/reply_data.v1.toml` with route.v7's `data` question and choices verbatim, and its builder), the embedded-pack list and its count test; the hook after a reply is written in a session that has an open run; `crates/theseus-core/src/tests_reply_data.rs` (new) | M-L |

## What exists (at `2d30cc48`; search by name)
- **The data rule.** `crates/theseus-core/src/data.rs` (573 lines): `struct Check { input, before, answer, late, live,
  sealed, attachment }`; `admits(s: Option<&DataState>, c) -> Result<OpenRun, DataWhy>` (near :173): taint first, then
  live, attachment, sealed, late, the answer (`open` at `Band::Act`, else `VerdictPrivate` / `VerdictUnclear` /
  `BelowAct` / `NoVerdict`), then **the chain**: the run extends (`from` kept) only when `c.before == run.through`,
  else it restarts at the input. `admits_again` (an open task's later turn). `OpenRun { from, through }`;
  `DataState { opened, run, taint }` is `SessionRecord.data` (an existing field: updating its `run` is no format
  change); `merge` compares the run as read, for a turn's write-back (read how a concurrent write is handled before
  you write the run from a background job).
- **Where it is called.** `crates/theseus-core/src/turn/open_step.rs` near :120-150 (`data::admits` with
  `before: at.checked_sub(1).map(|b| nodes[b].1.id ...)`), `data_admit_unrouted` (near :155), `data_admitted`.
- **The question.** `crates/theseus-judge/packs/route.v7.toml` `[questions.data]` (near :95: `open`, `private`,
  `unclear`, with their `means` text; the state's `data_names`, never names in the pack). `crates/theseus-core/src/routing/data.rs`
  `DataAnswer { choice, band, .. }`. How a sweep-style background judgment is asked, priced and capped:
  `crates/theseus-core/src/judge/onto/judged.rs` on `main` is a model (`JudgeService::reserve`, the day's money), and
  on your base the judge's day budget is passed the same way by every off-path pack (search `reserve(`).
- **Nodes.** `crates/theseus-core/src/node.rs` `Body::AssistantMessage { blocks, .. }` (near :162). A reply is written
  at a turn's end (search where the final `AssistantMessage` node is stored).

## What to build
1. **When to ask.** After a turn writes its final reply in a session whose data state has a run whose `through` is the
   reply's predecessor (the person message the turn answered, or the previous judged reply), queue the reply for the
   data question. Nothing else is asked (a session with no run, or a reply after a tool result, gets nothing: no cost).
2. **Off the path.** A background task drains the queue in small batches (one judgment per reply is fine if the pack
   takes one input; say why), never awaited by the turn or the send. It passes the judge's day budget; a refused
   reservation means no answer (a run-ender). The reply's text is scrubbed as every judged input is.
3. **The write.** When the answer is `open` at `Act` and the session's run still ends at the reply's predecessor, the
   run's `through` becomes the reply's id (with the store's compare-and-write: if the run moved meanwhile, write
   nothing). Then `admits`'s existing chain rule extends the run at the next person message with no change to
   `admits`. Any other answer writes nothing, so the reply stays a run-ender (fail closed). Record each answer in the
   ledger (a `data.reply` row: session, node, choice, band, cost) so the bench and inspect can see it; no stored record
   gains a field.
4. **The pack.** `reply_data.v1` asks route.v7's `data` question about one assistant reply (state: the operator's
   `data_names` as route.v7 has them). Copy the choices' `means` verbatim; change only the subject ("the message" ->
   "this reply") where the wording needs it, and list each change in the report.
5. **Docs:** module docs only. Core `AGENTS.md`'s data-rule passage and `docs/privacy.md` belong to round 5's
   cn-config-guidance row: put the sentence each needs in your report for the integrator.

## Tests (each with a planted revert; the first is the key test)
- **`a_dm_reply_judged_open_extends_the_run`**: person message (open) -> reply -> the fake judge answers the reply
  `open` at act -> next person message (open): its admitted run starts at the first message. Plant: skip the write:
  the run restarts at the second message.
- `a_reply_with_no_answer_ends_the_run`: the next message arrives before the reply's judgment: the run restarts.
  Plant: treat a missing answer as open.
- `a_reply_judged_private_or_below_act_ends_the_run` (both cases). Plant: accept `unclear`.
- **`the_reply_question_never_delays_a_turn`**: a judge that never answers; the turn and its send finish on time
  (paused clock or a bound). Plant: await the judgment in the turn.
- `a_session_without_a_run_asks_nothing`: no `data.reply` row, no judge call. Plant: ask for every reply.
- `a_run_that_moved_is_not_overwritten`: the run changed between the ask and the answer: unchanged. Plant: write
  unconditionally.
- The existing data-rule and open-request tests pass unchanged. `cargo nextest run -p theseus-core data` and
  `cargo nextest run -p theseus-core open_request`, then the crate, then `cargo nextest run -p theseus-judge`.

## Not in this row
- Any change to which models are restricted, the data floor, the private mark, or the taint rule.
- theseus-a1wvq.16 (programs in the sandbox) and .17 (first-pass review: the row beside you).
- The template, docs, core `AGENTS.md`'s China-origin passages: round 5's cn-config-guidance row.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **The row beside you in this wave, also on `integ/cn` 2d30cc48 for round 6:** tail4-cn-draft (`draft.rs`, `draft/`,
  `turn/draft_step.rs`, `config/routing.rs`, `fact/draft.rs`). Stay out of its files.
- **Round 5 beside you** (named in the note above): tail3-cn changes `data.rs` (`screen` only) and
  `turn/data_step.rs`. If you must edit `data.rs`, keep your hunk away from `screen`.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling (the judge's day budget
  included). A cloud row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` and paste its output in the report. It
  must be empty.
- **Dependencies.** None new. **Store format** 48, unchanged.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/integ/cn`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids and texts are invented; no real
  name in a fixture's `data_names`.
- Read `crates/theseus-core/AGENTS.md` (the data rule) and `crates/theseus-judge/AGENTS.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the flow (when a reply is asked, the batch, the write and its compare); every fail-closed case and
the test that pins it; the pack and each wording change from route.v7; the cost per reply and how it is capped; the
sentences for core `AGENTS.md` and `docs/privacy.md` for the integrator; each test and its plant with the failing
line; the keel guard's output; the store format (unchanged); follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
