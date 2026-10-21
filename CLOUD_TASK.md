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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-onto-r5-extras`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: onto-r5-extras, the nightly sweep merges and splits with Jev's judgment, mints a kind from `other` answers, the dream writes guidance, and the morning note lists every write with its undo (theseus-nzzz4, theseus-81zx7, theseus-olt3h, theseus-0umqz)

Branch: `cloud/20261021-onto-r5-extras`. Every commit's subject carries the Beads id of the part it builds
(`theseus-nzzz4`, `theseus-81zx7`, `theseus-olt3h` or `theseus-0umqz`). Deadline for the report: 6 hours after you start.

**Why.** The ontology grows from what Jev files as messages arrive, and a nightly sweep tidies it (`7895bb45`, the
first slice of the design's sweep). What a person meets today:
- **Near-duplicates stay apart.** The sweep merges two machine-made categories only when their names fold to the same
  letters (`Rust Harness` and `rust-harness`). `release checklist` and `release process`, or `voice echo` and `echo
  cancel`, stay two topics forever, so recall's category boost and the compile's guidance split across them.
- **A big category never splits.** A topic that 40 sessions sit in files everything and tells nothing.
- **`other` is thrown away.** `onto.v1` asks every message `kind_of_thing` (the table's kinds plus `other`), and
  `judge/onto/decide.rs` never reads the answer. The owner's rule is "category expansion, not just category
  inclusion": when several sessions say `other` about the same mined name, Theseus should mint a kind for it.
- **Guidance stays empty.** The dream may write a category's first guidance (`check_guidance_writer`), and nothing
  does, so a topic with twenty sessions composes nothing into their compile.
- **The owner cannot see what the night changed.** Each sweep write is undoable (`theseus ontology undo <write>`), but
  its id is only in the ledger.

**Risk: medium.** Model calls off the turn's path, inside a new daily cap; every write is a machine write that
`Core::ontology_undo` takes back. Nothing here is on the turn's path or the start path.

**Prerequisite: `main`.**

## The design (quoted: its file may not be on your base)
"Split, merge, rename are not decided on the hot path [...] they belong to the sweep [...] Each is act, tell, undo."
"A cluster of three or more sessions mints a kind." The dream's guidance "is a model call inside the dream's existing
budget, never a new one." "Every write is undoable and listed in the morning digest." The parts below spell it out.

## What exists on `main` (search by name; lines move)
- **The crate, `crates/theseus-ontology/`.** `sweep.rs`: `Ontology::sweep_plan(now_ms) -> Vec<Step>`, `Step::Decay`
  and `Step::Merge { absorbed, survivor, sessions, records }`, `fold(name)`, `DECAY_MS`; every planned record is
  checked as the sweep's write on a working copy. `merge.rs`: `Ontology::merge_category(absorbed, survivor, by)`, the
  general same-kind merge (refuses guidance or children on the absorbed one; moves machine memberships as the
  sweep's). `machine.rs`: the writer checks; `check_kind_writer` already lets `Origin::Sweep` add a new interpreted
  kind whose `added_by` is `sweep:<id>` and whose `assigned_by` names `sweep` and `jev`; `check_guidance_writer`:
  the dream only, `dream:<id>`, non-empty, at most `DREAM_GUIDANCE_MAX` (400) characters, only where none exists.
  `kind.rs`: `Kind` (with `examples` and `mined_from`, both optional), `GIVEN`, the seeds. Tests: `tests_sweep.rs`.
- **The core.** `judge/onto/sweep.rs`: `Core::onto_sweep(now_ms) -> Swept` writes each step through
  `Core::ontology_machine_write(Origin::Sweep, "sweep-<ms>-<n>-<word>", records)`; its tender
  `onto_sweep_after_serving` runs at `[judge] learning_hour` and once after a start, no mark stored (a run is
  idempotent). `rpc/ontology_machine.rs`: `Machine::new` accepts Kind and Guidance records; `ontology_machine_write`,
  `ontology_undo`. `judge/onto/decide.rs`: `decide(j, o, bands, now_ms) -> Plan` (`Step::{Filed, Ask, Attached,
  Unnamed, Refused}`; mined names in `j.context["onto"]["names"]`). `judge/onto/stage.rs`: `card(plan, step)` (the
  mint card, `"kind": "onto_card"`, through `core.outbox.stage_to_operator`) and the `OntoFiled` row (`fact/onto.rs`,
  `onto.filed`). `fact/ontology.rs`: `Undoable`, `OntologyUndo`. Tests: `tests_onto{,_sweep}.rs`,
  `tests_ontology_undo.rs`.
- **How a sweep asks Jev**: the people sweep, `judge/people/sweep.rs` (`[people] sweep_usd_per_day`, a session not
  begun when its worst case would pass the day's cap) and `judge/people/run.rs` `JudgeService::judge_people` (build
  the input, `theseus_judge::prepare`, the state's blob, `Ask::new`, `reserve_micros`, `self.reserve(&today, need)`,
  `judge(DecisionPoint { urgency: Urgency::Shadow })`, `budget.settle`). Copy that shape.
- **Packs**: `crates/theseus-judge/packs/*.toml`, registered in `pack.rs` (`include_str!` list), a builder input in
  `builders.rs` (`enum Input`, `Builder`) or a module under `builders/`, a golden request under
  `fixtures/golden/<pack>.request.json`; per-item Nouls (`per = "<source>"`, as `onto.v1`'s `still_topic`); the name
  miner `mine.rs`/`names.rs`; the scripted judge `fake.rs` (`FakeJev`, `script_when`). Read `onto.v1.toml` and
  `people.v1.toml` whole first: your packs follow their form (header comment, `jev_model`, `point`, `state`, bands).
- **The dream** is M6's nightly consolidation: `crates/theseus-core/src/consolidate/{mod,run,tender}.rs`. Its money:
  `[memory] synth_limit_usd_per_day`, the day's spend read from its own `synthesis.proposed` rows; its model:
  `[memory] synth_profile` (`"session"` = the profile every source's session last used; sources whose sessions
  disagree wait, counted).
- **The morning note**: `initiative/morning.rs` (`morning_facts` counts ledger kinds of the last day;
  `morning_note` voices them through `initiative_speak` and posts with `initiative_post`; paced by the one pacing
  rule, `pacer.rs`). Its stand-in words: `initiative/lines.rs`.

## Your row
Do the parts in this order; each is its own commit. If time runs short, finish and prove what you started rather
than begin the next, and say in the report what is left.

**1. `other` answers are recorded (theseus-81zx7, first half).** In `decide.rs`, a `kind_of_thing` answer of `other`
at the act band becomes a step `Step::Other { names, confidence }` carrying the mined names (`ctx["names"]`, at most
ten). It writes no record, sends no card, adds nothing to the footer; `OntoFiled`'s `step_row` writes it as
`{"kind": "kind_of_thing", "outcome": "other", "names": [...], "confidence": c}`. A plan holding only this step is
not empty (the row is the record). Say in the report that such a turn's frame gains one `onto.filed` row; a turn
whose answer is a listed kind, or `other` below the act band, writes nothing new.

**2. The sweep mints a kind (theseus-81zx7, second half).** In the core sweep, after the deterministic plan: read
the `onto.filed` rows of the last 7 days (a reversible default; the design says "the day's", and three sessions in a
day is rare: say so), take the `other` steps, fold each mined name (`fold`), and find a name that three or more
**distinct sessions** share. Two sessions do not mint. A cluster mints, as one machine write of the sweep's
(`sweep-<ms>-<n>-kind`):
- a `Kind`: basis interpreted, `assigned_by` [operator, jev, sweep], `per_session` at most 3, no parent, rule
  `chain`, precedence above the highest in the table (the next free multiple of 10), `added_by`
  `sweep:<write id>`, `examples` the cluster's names (at most `EXAMPLES_MAX`), a description stating the facts ("A
  kind the sweep made from N sessions whose messages were about none of the listed kinds: <names>.");
- its name: the most shared mined name, folded to the table's kind-name rule (`text::kind_name`, crate-private: check
  the built row with `Kind::check_row`, or expose one `pub` checker from the crate); a name that does not pass, or
  that the table already holds (retired or not), mints nothing and is counted;
- one category of the new kind named by that name, and each cluster session's membership in it.
A name a minted kind already lists in `examples` is not clustered again (so a repeat run writes nothing). Undo
retires the kind and its category and drops the memberships; check that `Ontology::undo` has an arm for a Kind
whose `before` is null (a new kind) and add it in the crate with a test if it does not. The mint sends the owner one
card in the mint card's form (part 4 decides when it posts). Rename, the `same_kind` Noul over differing names, and
a Jev Choice for the kind's name are NOT in this row (filed as theseus-63tfk.1).

**3. Model-judged merge and split (theseus-nzzz4).** Two packs, each a new file in `crates/theseus-judge/packs/`,
registered, each with a builder input, a golden request and `FakeJev` scripting:
- **`same_thing.v1`**: state = two categories of one kind (name, description, path, and up to five member session
  titles each, scrubbed as the inbound state is); one Noul `same_thing` ("Are these two categories the same thing,
  named two ways?"), act 0.85, confirm 0.60.
- **`two_things.v1`**: state = one category and a sample of at most 12 member session titles, numbered; one Noul
  `two_things` ("Do these sessions fall into two distinct things that would each be worth a category of its own?"),
  and two Choices `first_name` and `second_name` over names code mined from the sampled titles (`mine.rs`, at most
  ten) plus `none`, and one per-item Noul `in_first` over the sampled sessions ("Is this session about the first of
  the two?"); add the per-item source the pack loader needs, following how `onto.v1` names its per-item sources.
In the core sweep (a new module beside `judge/onto/sweep.rs`, e.g. `judge/onto/judged.rs`; keep `sweep.rs`'s
deterministic plan as it is):
- **Pairs**: categories of one kind under one parent, both machine-written, with no guidance and no children, not
  already a folded-name pair, whose names share a word of four or more letters or where one name contains the
  other; at most 20 pairs a run, oldest pairs first. At the act band, merge through the crate's merge path: extract
  the record-building half of `sweep_plan`'s merge into a crate function both use (one survivor rule: the
  operator's, else the more-held, else the smaller id) and write it as a `Step::Merge` of the sweep's.
- **Splits**: a category of a kind whose categories nest (its kind's `parent` is its own name: topic, expertise)
  that 12 or more sessions hold (a reversible default), at most 3 a run, largest first, never one the operator wrote.
  At the act band with both names chosen at the act band and different: two child categories (parent = the large
  one, origin sweep) and the sampled sessions moved by `in_first` at the act band (yes to the first child, no to the
  second; below the act band they stay in the parent). Unsampled sessions stay in the parent: Jev's next filing
  offers the children as candidates. One write per split.
- **Money**: a new `[ontology] sweep_usd_per_day` (default 1.0; 0 turns the judged part off and the deterministic
  sweep still runs: that is the rollback), documented in `config/ontology.rs` and the example config, read the way
  `[people] sweep_usd_per_day` is (a call not begun when its worst case passes what is left of the local day's
  cap). Every call also passes the judge's own day budget (`self.reserve`) exactly as `judge_people` does. Do not
  raise any judge budget.
- `Swept` gains `judged_merges`, `splits`, `kinds`, `spent`; the tender's log line names them.

**4. The dream writes guidance (theseus-olt3h).** A new module (e.g. `consolidate/guidance.rs`), run by the
consolidation's nightly tender after its own run (one call in `consolidate/tender.rs`), never by the owner's
`memory.consolidate`:
- **Which categories**: a category of an interpreted kind that five or more sessions hold, with no guidance, never
  the operator's or a shared place's, at most 3 a night, most-held first.
- **The call**: one model call per category through the provider path consolidation uses (so the data rule's gate
  and the China-origin rule apply unchanged), with the profile chosen by `synth_profile`'s rule over the category's
  sessions (`"session"`: all must share their last profile, else the category waits and is counted). Input: the
  category's name and path, and for its newest 12 sessions the title and the first and last human message, each
  trimmed to 300 characters and passed through the core's scrubber. Ask for what those sessions settled, as plain
  facts, at most 400 characters. Reserve the worst case at the profile's prices first; the night stops before a
  call would pass `[memory] synth_limit_usd_per_day`, counting consolidation's spend and these calls' (a refused
  answer's call too: keep the day's spend where a restart reads it back, as the people sweep's `SweepMark` does).
  No new budget.
- **The write**: an answer that is empty, over 400 characters after trimming, or not text is refused and counted
  (never cut to fit). Else `Core::ontology_machine_write(Origin::Dream, "dream-<ms>-<n>", vec![Record::Guidance(..)])`
  with `added_by` `dream:<id>`; `theseus ontology undo` restores the empty guidance (built and tested already).
  Guidance changes the compile's bytes for that category's sessions at their next recompile: say so in the report.
- **The card**: one owner card per write, in the mint card's form (`"kind": "onto_card"`, `"undo": true`, the
  write id, `theseus ontology undo <id>` in its text). **No post outside the pacing window**: read how consolidation
  tells the owner (`initiative/consolidation.rs`, `Line::Consolidation`) and follow it; if that path would post at
  night, the card's line goes into the morning note instead (part 5) and the report says which you did. The kind
  mint of part 2 follows the same rule.

**5. The morning note lists the night's writes (theseus-0umqz).** A pure function (a new module, e.g.
`judge/onto/digest.rs`) that reads the last day's ontology ledger rows whose `undo.write` starts with `sweep-` or
`dream-` and words one line per write, with the id `ontology undo` takes, e.g.:
`- merged topic "release process" into "release checklist" (3 sessions) · undo: theseus ontology undo sweep-…-merge`,
`- dropped 4 memberships unconfirmed for 30 days · undo: …`, `- split topic "theseus" into "voice" and "cockpit"`,
`- made the kind "recipe" from 3 sessions`, `- wrote guidance for topic "release checklist"`. An undone write's line
says it was undone. In `morning.rs`: `morning_facts` counts these writes as one more item ("ontology changes by the
night's sweep and dream"), and `morning_note` appends the lines verbatim after the voiced words (the model never
rewords an undo id). The stand-in path (no model) appends them too. No `ontology.swept` ledger row unless you find
the lines cannot be built from the writes' own rows: if you add one, it is a new `LedgerKind` (expect the protocol's
ledger golden and the cockpit's generated types under `cockpit/src/protocol.gen/` to change: regenerate them as
`crates/theseus-protocol/AGENTS.md` says) and the report says why.

## Tests (each with a planted revert; the first is the key test)
- **`the_sweep_merges_a_pair_jev_calls_the_same_thing`** (core, `FakeJev` scripted per pair): two machine topics
  `release process` and `release checklist`, Jev answers yes at 0.9: one merge write, the survivor by the rule, the
  memberships moved, and `ontology_undo` restores the ontology equal to before (compare whole). A pair answered at
  0.7 writes nothing. Plant: drop the act-band check (merge at confirm). Second plant: the pair builder skips pairs
  the folded-name rule does not catch.
- `a_large_topic_splits_into_two_named_children` and its undo; a split refused when the two names are equal, or one
  is `none`; the operator's large topic is never split. Plant: move every sampled session to the first child.
- `three_other_sessions_mint_a_kind_and_two_do_not`: three sessions' `onto.filed` rows with `other` and a shared
  mined name mint one kind, one category, three memberships; two sessions mint nothing; a second run writes nothing;
  undo retires the kind. Plant: a cluster of two mints.
- `other_below_the_act_band_is_not_recorded` and `a_plan_with_only_other_writes_its_row`.
- `the_judged_sweep_stops_at_its_day_cap` (scripted costs: the call that would pass the cap is not begun) and
  `sweep_usd_per_day_zero_leaves_the_deterministic_sweep` (byte for byte the old run's writes). Plant: ignore the cap.
- `the_dream_writes_guidance_for_a_category_of_five` (scripted provider), `four_sessions_get_none`,
  `guidance_over_400_characters_is_refused_not_cut`, `the_dream_stops_at_synth_limit`, and undo. Plant: cut a long
  answer to 400.
- `the_morning_note_lists_each_write_with_its_undo_id` (a merge, a decay, a guidance, one undone), and a day with
  no ontology write adds no line. Plant: drop the undo id from the line.
- Pack goldens: each new pack's request golden; `FakeJev` answers parse into bands.

## Not in this row
- **The cockpit** (an ARM lane does the Ontology view's extras: the tree's origin filter, the add-a-kind form, the
  Observatory line, the learning report's ontology line; `cockpit/` and the learning report's code). Touch nothing
  under `cockpit/` and not `learning/report.rs`.
- **Measurement** (another cloud row, onto-r8-r6: the labelled replay, `bench/threads/`, the compile's
  thread/project reader in `ontology.rs`'s walk). Do not edit `learning/`, `bench/`, or `TurnRunner::walk`.
- Rename, the `same_kind` Noul, Jev naming a kind (theseus-63tfk.1). The in-turn `onto.v1` pack's questions
  (unchanged: you read its `kind_of_thing` answer, you do not change the pack).
- Threads files (`thread_route.rs`, `thread_recall.rs`, `theseus-memory/src/threads.rs`), the data gate, Discord.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. The rollbacks: `[ontology] sweep =
  false` (all of the sweep), `[ontology] sweep_usd_per_day = 0` (the judged part only; a test shows the old run's
  writes byte for byte), `[memory] synth_limit_usd_per_day` already bounds the dream.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). A new model tool is deferred by
  default (this row adds none). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the
  path before a request leaves, and nothing on the start path grows with history (the sweep's first run is the
  tender's, never within 10 minutes of a start).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`,
  then `git status`); list each plant in the report with the test that failed. The row's **key test** (named first
  under Tests) must be shown failing on its planted revert. A pure move (the merge builder's extraction) needs no
  plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last
  commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the
  report. It must be empty unless this task names keel findings. New config caps are new keys, not raises.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change
  only as the note above says (none is expected: new rows and optional fields only).
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push, a model call's input) through `core.tools.scrubber`. No person's
  name anywhere in the repo, in commit messages or in the report: write "the owner". Fixture paths and names are
  invented.
- **Model-facing text states facts.** A pack's instructions, the dream's request and a kind's description say what
  is; they never tell the model what to do beyond the question, never say "must", never give examples of answers.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first, where it has one (`theseus-core` does).
  Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud (L1 needs a non-root
  daemon with a job cgroup: about 33 L1 tests fail on this VM; none of yours should need L1), and give the maintainer
  the exact commands for the rest.

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its
size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. The report has: any file left over a line ceiling (first section, with why); what changed,
per part and Beads id; each test and each plant with the test that failed; the reversible defaults you chose (the
7-day window, 12 sessions to split, 20 pairs and 3 splits a run, the cap's default, where the dream's card posts);
what could not run on the VM and the exact commands for the maintainer (`cargo nextest run -p theseus-ontology -p
theseus-judge -p theseus-core` plus the gate, `scripts/gate.sh`); the keel guard's output; the store format (none
expected); follow-ups.
