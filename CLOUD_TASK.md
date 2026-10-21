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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-threads-links`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: threads-links, the owner links two threads so they recall each other freely, and Jev offers a link once when two streams keep sharing notes (theseus-98bz.11)

Branch: `cloud/20261021-threads-links`. Every commit's subject carries `theseus-98bz.11`. Deadline for the report: 5
hours after you start.

**Why.** Activity threads keep separable streams of work apart, and the thread lens holds another stream's notes out
of a thread's recall unless they pay a distance (a penalty, a bar against the query's best hit, and they must still
beat the thread's own best hit) or the message names the other thread by slug, title or a working-set entity. That is
right for unrelated streams, and it is the reason threads exist. It is wrong for two streams the owner knows belong
together (a feature branch and the release checklist that ships it; a data cleanup and the report built on it): today
they pay the distance on every recall, and the owner has no way to say "these two are related". The synthetic
threadbench (plan T12) showed the cost on real near misses: fused scores sit close together, so a penalised note from
another stream rarely clears the "beats own" rule. The cross-stream row just before you (C10, now on your base) added two deliberate paths: a place digest on "what else is going on"
turns, and Jev reading up to three near-miss notes per recall and admitting the ones about the message's subject
(`recall_cross.v1`). Neither lets the owner declare a relation once and have it hold. This row does that.

**Risk: low to medium.** A link is the owner's explicit choice, made from a private place, and it never crosses a seal:
a sealed or seal-pending thread cannot be linked, and a link made before a seal stops waiving the moment the seal
lands. The proposal (part B) only posts a sentence to the owner's DM; it never links anything itself. The store format
is bumped (a stored record gains a field).

**Prerequisite: the threads lane with C10 in.** See "Your base".

## Your base

Your branch starts from the threads integration branch (`integ/threads`) with C10 (`cloud/20261021-threads-cross`,
plan rows 9, 8 and 10) merged into it. This task was written against `main` at `87696a7c` plus C10's diff; the names
below are as they are there. If a name moved, search by name. What C10 left you, on your base:
- `crates/theseus-core/src/thread_recall.rs`: `ThreadScene` (fields `here`, `distance`, `waived`, `penalty`, `bar`,
  `base`, `beats_own`, `own_top_admissible`, `needs_entity`, `admitted`, `board`, `store`, `memo`),
  `ThreadScene::admit`, `ThreadScene::lens(candidates, in_context)`, and `TurnRunner::thread_scene(session, message,
  ambient)`, which sets `waived = named(&board, message)` when `distance` is on (an ambient recall in a threaded
  session). `named` is the "the message names a thread" rule.
- `crates/theseus-memory/src/threads.rs`: `ThreadLens::judge`. Its order matters to you and you do not change it: an
  `admitted` node costs the penalty; a candidate of the asking thread passes; a held (sealed or seal-pending) thread's
  candidate is `Sealed`; then `!distance || waived.contains(slug)` passes unpaid; then the entity rule, the bar and
  `beats_own`. So a slug in `waived` passes with no penalty and **a seal still wins**.
- `crates/theseus-core/src/turn/cross_step.rs`: `TurnRunner::cross_judged` sends the near misses to Jev and, on a yes,
  calls `ThreadScene::admit(yes)` then refills the manifest. This is where part B counts.
- `crates/theseus-core/src/judge/cross.rs`: the `recall_cross.v1` call, which records its `judge.call` off the turn's
  path (in its own task): the pattern for part B's write.
- `[threads]` keys C10 added (`crates/theseus-core/src/config/threads.rs`): `own_top_admissible`,
  `cross_recall_judge`, `cross_recall_judge_k`, `place_digest`, `place_digest_line_chars`.

Plan row 8 (`own_top` counts only what can still be admitted) loosens the lens and may ship only beside row 9 or 10.
Both are on your base with it, so that rule is met; this row neither needs nor changes row 8.

## Your row (names as on your base)

### A. The link (must do)
- **The record.** `crates/theseus-protocol/src/threads.rs` `ThreadInfo` gains `links: Vec<String>` (doc: "Threads the
  owner linked to this one: their notes reach this thread's recall with no distance; a seal still holds them out"),
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, placed after `split_from`. `ThreadInfo` is a stored
  record (META `thread:<slug>`), so bump `MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` from 47 to 48 and
  follow the store's version rule in the root AGENTS.md: `THREAD_AT_33` in `crates/theseus-core/src/tests_layouts.rs`
  must still read and write back byte for byte (it will, since an empty `links` is skipped); add a `THREAD_AT_47`
  literal (a thread as format 47 writes it, with a seal and a `split_from`) and its byte-for-byte test beside
  `a_format_33_thread_record_reads_and_writes_back_byte_for_byte`; move the tests that name the current format number
  (for example `a_format_32_store_opens_with_no_threads_and_takes_them`'s "moves the manifest to 47" and any
  `["format"], 47` assertion) to read the constant or the new number. Add a line for 48 to the format history in
  `crates/theseus-core/AGENTS.md` (the list that has the 47 entry).
- **The wire.** Two methods, `thread.link` and `thread.unlink`. Add `THREAD_LINK = "thread.link", THREAD_UNLINK =
  "thread.unlink",` **onto the existing one-line `THREAD_*` method list** in `crates/theseus-protocol/src/lib.rs`
  (after `THREAD_UNSPLIT`): that file has three lines of room under its ceiling and other rows want them, so **this
  row adds no line to `lib.rs`**. Their params type `ThreadLinkParams { slug, other, author, discord }` (author and
  discord as in `ThreadNameParams`) goes in `threads.rs`; the result is a `ThreadRow` of `slug`'s thread, as
  `thread.edit` gives. Regenerate the TypeScript (`cockpit/src/protocol.gen/`: `ThreadInfo.ts`, the new
  `ThreadLinkParams.ts`, `index.ts`) with the repo's generator, never by hand.
- **The core.** A new module `crates/theseus-core/src/rpc/thread_link.rs` (one `mod` line in `rpc`'s module list, and
  one dispatch arm for the two methods in `Core::rpc_threads` in `rpc/threads.rs`, as `THREAD_MERGE`'s arm routes to
  `rpc_thread_joins`). Model it on `thread_merge` in `rpc/thread_join.rs`, the other two-thread act:
  - judged with `judge_thread_pair(&who, method::THREAD_LINK, a, b)` (in `rpc/thread_join.rs`): the owner, from a private place, as every act on
    the threads is. **No model tool links**: add none, and no existing tool may reach these methods.
  - `threads_on()` first; both slugs trimmed; refused: the same thread twice ("a thread cannot be linked to itself"),
    an unknown slug (the message names `theseus thread list`), a thread merged into another ("the thread X was merged
    into Y: link that one"), and **a sealed or seal-pending end** ("the thread X is sealed: a sealed thread's notes
    never reach another thread, so it cannot be linked"; for a pending seal say a seal is proposed and waits for the
    owner). A thread may hold at most 8 links (a const), refused past it with the number.
  - written with one `self.runner.threads.write(...)`: **both** records (each gains the other's slug, kept sorted and
    unique) and one ledger row in the same frame. Linking an existing link and unlinking an absent one write nothing
    and answer the thread as it is (as `thread_edit` does when nothing differs).
  - `thread.unlink` removes the slug from both ends, and is allowed whatever the seal (it only tightens).
  - announce the change on each end with `announce_thread` so surfaces that follow threads see it.
- **The ledger.** Kinds `ThreadLinked = "thread.linked"` and `ThreadUnlinked = "thread.unlinked"` right after
  `ThreadUnsealed` in `crates/theseus-protocol/src/ledger.rs`, each with a fact in `crates/theseus-core/src/fact/
  thread.rs` (carrying both slugs, who and via, as `ThreadMerged` does) registered where `fact/mod.rs` registers the
  thread facts. The repo has a test that fails a ledger kind nothing writes: both must be written by a test path.
- **The lens.** In `TurnRunner::thread_scene` (`thread_recall.rs`), when `distance` is on and the asking session has a
  thread (`here`), `waived` also takes the threads linked with `here`: every slug in `here`'s `links`, and every
  thread whose `links` holds `here` (read both directions, so a half-written pair from a bug can never make the lens
  one-sided), each resolved through `board.live_of(..)` so a link to a thread later merged follows the live thread.
  Nothing else in the lens changes. A held thread still drops `sealed` because `judge` checks the seal before
  `waived`. Put the "linked slugs of `here`" function on the board or as a free function in `thread_recall.rs`, with a
  unit test.
- **The CLI.** `crates/theseus/src/thread.rs` `ThreadCmd` gains `Link { slug: String, other: String }` and `Unlink {
  slug: String, other: String }` with doc lines in the house voice ("Link thread SLUG with OTHER: each one's notes
  reach the other's recall with no distance. Neither may be sealed."). `thread show` lists `linked with` beside
  `merged from` (the `("merged from", ...)` rows near the show renderer). JSON output as the other subcommands give.
- **The cockpit.** `cockpit/src/lib/threadboard.ts` `Links` (parent, merged into, split from) gains the linked threads,
  labelled `Linked with`, one entry each; extend `cockpit/test/threadboard.test.ts`.

### B. Jev offers a link once (do after A is committed; may be left as a follow-up if time is short)
- **When.** In `cross_judged` (`turn/cross_step.rs`), after Jev's yes is admitted: for each admitted node whose thread
  (`lens.thread_of_node(session, node)`) is another live, unheld thread than `here`, count the pair once per recall
  (not once per note). Nodes of the place's own conversation (no thread) are not counted.
- **The count, off the turn's path.** The write happens in a spawned task after the recall, never before the request
  leaves (judge/cross.rs records its `judge.call` the same way). META key `thread.link_seen.<a>.<b>` with the pair's
  slugs sorted, holding `{ "recalls": u32, "asked": bool }` (a new META record, not a field on an existing one: no
  extra format bump). On the second recall of a pair, when the pair is not linked, neither end is held or merged, and
  `asked` is false: post the proposal, set `asked`, and write a ledger row `thread.link_proposed` (a third kind, after
  `ThreadUnlinked`, with its fact). Once asked, a pair is never asked again, whatever the owner did.
- **The post.** One sentence to the owner's DM through the outbox (`stage_to_operator`, as `thread_recommend_seal` in
  `rpc/thread_seal.rs` posts its card), using a post kind the Discord binding already renders as text in the owner's
  DM (read `crates/theseus-discord/src/courier.rs`: the `"jev_notice" | ... | "onto_card"` arm and `jev_post`; pick
  the kind whose body you can fill honestly, and say which in the report). Text, as a fact: `🔗 Notes of «B title»
  answered questions in «A title» twice. If the two belong together: theseus thread link a b`. **No new buttons and
  no new post kind**: `crates/theseus-discord/src/runtime.rs` is at its ceiling, and a card with Link / Not needed
  buttons is a follow-up (below).
- **The switch.** `[threads] link_proposals = true` in `config/threads.rs` (with the template line in
  `crates/theseus-core/config/theseus.example.toml`). Off: nothing is counted, written or posted, byte for byte as
  before (a test shows it).

## Tests (each with a planted revert; the first is the key test)
- **`a_linked_threads_note_passes_the_lens_unpaid`** (key; a whole-recall test beside C10's
  `rpc/tests_own_top.rs` and `rpc/tests_cross_judge.rs`, reusing their fixtures): two threads, A asking, a note of B
  that today drops with a `thread` reason (under `beats_own`). After `thread.link A B` the same recall admits it with
  its fused score unchanged (no penalty) and no `recall_cross.v1` request is made for it. After `thread.unlink` it
  drops again. Plant: take the linked slugs out of `waived` in `thread_scene`; the test must fail.
- `a_seal_after_a_link_still_holds_the_notes_out`: link A and B, then `thread.seal B`; B's note drops `sealed` in A's
  recall, and B's title, goal and card are in no part of A's model request (C10's leak check in
  `rpc/tests_place_digest.rs` is the pattern). Plant: in `thread_scene`, mark a linked thread's candidates as not
  held (or move the `waived` check before the seal check in a scratch copy); the test must fail. Restore.
- `linking_a_sealed_or_pending_thread_is_refused`: either end sealed, or with a seal proposed: refused with the
  message, nothing written (the store's record count is unchanged). Plant: drop the check.
- `a_link_is_written_on_both_threads_in_one_frame`: after `thread.link a b` both records list the other, one frame
  holds both records and the `thread.linked` row; `thread.unlink b a` clears both and writes `thread.unlinked`;
  linking twice writes nothing the second time. Plant: write only `slug`'s end; the lens test's "both directions" read
  would hide it, so this test reads the stored records.
- `only_the_owner_links`: a connection that is not the owner's, or the owner from a shared place, is refused, as
  `tests_thread_join.rs` tests `thread_merge` with a stranger. Plant: skip the judge.
- `a_link_follows_a_merged_thread`: link A and B, merge B into C; A's recall waives C's notes.
- `a_link_to_self_unknown_or_merged_is_refused`, and the 8-link cap.
- Layout: `THREAD_AT_33` still round-trips; the new `THREAD_AT_47` round-trips; the store tests on the new number.
- Part B: `a_pair_admitted_twice_is_proposed_once` (Jev scripted to say yes as in `tests_cross_judge.rs`: two recalls
  admitting B's notes into A post one proposal, write `asked` and one `thread.link_proposed`; a third recall posts
  nothing; a linked pair is never counted; a held thread is never proposed; with `link_proposals = false` no META key
  and no post appear). Plant: drop the `asked` check; the test must fail.
- CLI: `thread link` / `unlink` parse and print; `thread show` names `linked with` (beside
  `show_names_the_threads_it_was_merged_into_or_split_from`).
- FAST: `tests_m3::a_plain_turn_stays_within_its_frame_budget` and `tests_first_request.rs` pass unchanged; part B's
  write adds no frame to the turn (assert the turn's frame count in the part B test is the same with the switch on and
  off).

## Not in this row
- A Discord card with Link / Not needed buttons for the proposal, and a Discord `/thread link` command (the Discord
  runtime file is at its ceiling): a follow-up, theseus-98bz.12.
- Linking from the cockpit (it only shows links), and linking a thread to a place's own conversation (home notes keep
  the `base` distance).
- Any change to the lens's rules, the place digest, `recall_cross.v1`, `own_top`, routing (`thread_route.rs`) or the
  drop causes. A threadbench arm with a linked pair: the maintainer adds one to a later plan revision.

## Beside you
The routing row (C9, `cloud/20261021-threads-routing`, plan rows 2-7) may still be coding from `main` beside you and
joins the same lane. Its files, and how to stay mergeable:
- `crates/theseus-core/src/thread_route.rs`, `crates/theseus-core/src/rpc/thread_flow.rs`, `threads_import/`: yours
  touch none of them.
- `crates/theseus-memory/src/threads.rs` and `recall.rs`: C9 splits the drop reason into `Reason::Thread(Cause)`
  (`thread_bar`, `thread_own`, `thread_entity`, `thread_base`) and adds a `Distant` struct. **Do not edit the lens
  file.** In your tests, compare a drop reason with `starts_with("thread")`, never `== "thread"`, so they pass on
  either side of C9's join; match `Reason::Thread` as C10 does and expect the integrator to write `Reason::Thread(_)`.
- `crates/theseus-core/src/config/threads.rs`: C9 adds `sticky_alone_asks` and a test. Put `link_proposals` last in
  the struct, its default and the template, and your config test as a new function at the end of the tests module.
- `crates/theseus-protocol/src/lib.rs`: three lines of room under its ceiling (2,730 of 2,733) shared with the whole
  bundle; you add no line, only two entries on the existing `THREAD_*` line. If another branch edits that same line,
  the join keeps both.
- `cockpit/src/protocol.gen/`: C9 changes `RecallDrop.ts` and `RecallManifest.ts`; you change `ThreadInfo.ts`,
  `index.ts` and add `ThreadLinkParams.ts`. Generated: the integrator regenerates at the join.
- **Store format**: C9 and C10 do not bump it; other lanes of the bundle may also bump to 48. Write the number only in
  the constant and the tests that must name it; the maintainer renumbers at the join.
- `crates/theseus-core/AGENTS.md` and `crates/theseus-memory/AGENTS.md`: add your sentences as their own lines (the
  thread-recall paragraph: "linked threads join `waived`"), not by rewording C9's or C10's.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. The link's rollback is `thread unlink`;
  the proposal's is `[threads] link_proposals = false`, and a test shows that with it off the old behaviour is byte
  for byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool. FAST:
  nothing new on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`,
  and the gate's turn bench must not move), nothing on the path before a request leaves (the scene's link read is an
  in-memory board read), and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` (your base; if that ref is not
  in your clone, use the commit your branch started from) and paste its output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. The store format bump is
  the one this task names (47 to 48), with its layout sample.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths, thread titles and slugs are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (theseus-core, theseus-memory, theseus-protocol,
  theseus, the cockpit). Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud
  (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM, so say which of your tests you
  could run and give the maintainer the exact commands for the rest). The VM's disk is about 39 GB: run the gate with
  `CARGO_INCREMENTAL=0` after a clean `target/` if space runs short, as C10 did.
- Commit A, with its tests and plants green, before you start B. A part you do not reach is named in the report as not
  started; a part half done is reverted from the branch, never committed half done.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: what changed, by part; each test and its plant, with the test that failed; the post kind part
B uses and why; the store format bump and the layout samples added; what could not run on the VM and the exact
commands for the maintainer; the keel guard's output; any file left over its ceiling (there should be none) and the
follow-ups (the Discord card and `/thread link` command at least). Push, and end with a final message that starts
`CLOUD REPORT COMPLETE`.
