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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-route-data`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `23328545` (2026-10-23: release 1.0.1, install #76). That is your base. Line numbers
quoted below are at `23328545` but may be off by a few: search by name. The release preamble above is older than this
note; where they disagree about `main`, ceilings or the work in flight, this note wins.

**This wave is patch2 (release 1.0.2): the follow-ups beside the Operations page.** Rows start on `23328545` and join
integration lanes named in each task (`integ/p2-route`, `integ/p2-caps`, and others). Other rows land in the same
lanes from other branches at the same time, some built on another host. Your task names them and the files you share
with them: stay additive (one helper, not two), and leave their files alone.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Prefer a META key
or a serde-default field. If your row adds a field or a variant to a stored record, bump it to 50, add the old layout's
literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and say so in your report's first
section. Several rows may bump; the maintainer renumbers at the join. Never write the number as a literal anywhere but
the constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-core/src/toolrun.rs` 2,500 / 2,500 (unlisted, at the cap): add nothing to it.
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733: a new type goes in a module file; `lib.rs` gets at most a field
  line or a method constant.
- `crates/theseus-core/src/turn.rs` 3,469 / 3,525: add nothing; put turn logic in a module beside it.
- `crates/theseus/src/render.rs` 3,126 / 3,128 and `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500: add nothing.
- `crates/theseus-core/src/config.rs` 2,149 (unlisted, cap 2,500): a new config section goes in `config/<name>.rs`.

An unlisted file stays at 2,500 lines or fewer. Never raise a ceiling yourself (the keel guard flags it). If a line
you truly need will not fit, leave the file one line over and say so in your report's first section.

**The owner's rules for this wave** (each task repeats the ones it touches):
1. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
   (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt` before your report). If your
   fix would change any message of that flow (its wording, layout, order or timing), stop that part and write
   "OWNER CALL: <what and why>, before/after text" in your report instead.
2. Keel: no removed or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py` or its
   tests. Run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` before the report and paste its
   output (it must be empty).
3. The data floor is never loosened: nothing private reaches a model of China origin, and nothing marked private or
   sensitive reaches a model the floor does not clear for it. A row near data verdicts ends its report with a
   "Data-routing review" section that a Claude model reviews.
4. The repository is public: no person's name, no real organisation, address or id anywhere. Fixtures use invented
   names and ids under `example.test`. Write "the owner".
5. Red first: each fix comes with a test that is red at the base and green after, and a planted revert named in the
   report. Real-world behaviour, never a benchmark special case: no task name from any benchmark in code or tests.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2-route-data, Jev's data verdicts still call open-source work private or unsure: fix what counts as private, never loosening a floor (theseus-f9qa8)

Branch: `cloud/20261023-p2-route-data`. Every commit's subject carries `theseus-f9qa8`. Deadline for the report: 3 hours
after you start.

**Why.** The route pack's `data` question asks Jev, the routing judge, whether what a message carries may go to an
outside model provider that must never see private matter (`open`, `private`, `unclear`). A restricted model may be
sent a request only on `open` at the `act` band. A benchmark retest on 2026-10-23 ran 27 open-source programming tasks
(self-contained exercises in a container: build this toolchain, fix this public library, train this public model)
and read Jev's verdicts:
- HIGH tier: `private` 12 of 40 (30%), `open (act)` 4;
- MAX tier: 122 verdicts: `private` 19 (escalate 13, confirm 6), `open (act)` 6, `open (escalate)` 51,
  `open (confirm)` 46.
Every one should have been `open` at `act`. Two days earlier route.v7's rewording took `private` from 60% to 17% on
the same kind of work, so the wording works but is not there yet. Real-world effect: a wrong `private` keeps a turn
off models it could use and costs escalations; a low-confidence `open` does the same at the act bar. The fix is in
**what counts as private** (and what makes `open` sure), with a contract test of the verdict table. It must not loosen
any private or sensitive floor.

**Risk: high (data routing).** Your report ends with a "Data-routing review" section that a Claude model (only)
reviews line by line.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins the lane `integ/p2-route`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-route-data` | theseus-f9qa8 | find why open-source work reads private or unsure; route.v8 (v7 word for word but for the `data` question, and the state if it must carry a new open signal); the verdict-table contract test; v8 live as v7 went live | judge: new `packs/route.v8.toml`, its golden request under `fixtures/golden/`, `src/pack.rs` (embed and the v7-to-v8 word-for-word test, as v6-to-v7's), `src/builders.rs` only if the state gains a signal; core: `config/routing_data.rs` only if a signal comes from config, `routing/data.rs` (doc, and the code rule if one is added), where the live route pack is chosen (follow how route.v7 went live), tests; the `reply_data` pack (it asks route.v7's `data` question of one message: follow it to v8 or say why not); core `AGENTS.md` (the data section); `docs/` where packs are listed | M |

## What exists (at `23328545`; search by name)
- **The pack.** `crates/theseus-judge/packs/route.v7.toml`: read its header comment and `[questions.data]` in full
  (`act = 0.90`, `confirm = 0.60`, `no_match = "unclear"`). `open` means general knowledge or public matter, "a
  self-contained exercise in programming, mathematics or science, or work on open-source software or on a project the
  state lists as open, with no sign of anything personal, private, confidential or secret in it"; `private` means a
  sign of private matter (an organisation's non-public plans, people, customers, partners, code or data; anything the
  state lists as private; a person's own matters; anything confidential or under embargo; another person's personal
  data; a credential's value); `unclear`: "work on files or code whose owner it does not show, or it mixes open and
  private matter", which fails closed as `private` does. "A live pack's options never change in place": a wording
  change is a new version (`route.v8`), and v5-v7 stay embedded for their history.
- **How a version is embedded and goes live:** `crates/theseus-judge/src/pack.rs` (`("route.v7",
  include_str!(...))` near :64; the word-for-word test for v6 to v7 near :1644; `rules("route.v7")` near :1435;
  `reply_data.v1` near :1711 asks route.v7's `data` question). Follow the commits that made v7 live
  (`git log --oneline --grep route.v7`) for every place a version is named (config default, docs, goldens).
- **The state Jev reads.** `crates/theseus-judge/src/builders.rs` (`data_names` near :295, `data_names_state` near
  :1006) and `crates/theseus-core/src/config/routing_data.rs` (`names(&privacy) -> DataNames`: the operator's open and
  private names). The inbound input is built in `crates/theseus-core/src/judge/inbound.rs` (`input`, `verdict`,
  `names_hit`).
- **The code rules on the answer.** `crates/theseus-core/src/routing/data.rs`: `DataAnswer`, and
  `DataAnswer::private_by_name()` (a message naming a sensitive name, `[privacy] sensitive_names`, is `private` by
  code whatever Jev said, theseus-oejh3). Where the answer is read for a turn: `turn/data_step.rs` (`cn_would_win`,
  `data_gate_why`), the provider chokepoint `data_gate.rs`. **Read these; do not change them** (see "Shared").
- **Session floors that do not depend on the verdict:** the Google private mark (`google/private.rs`) makes a session
  US-only; nothing here touches it.

## What to do
1. **Find the cause first, then write it down** (report section "Why open work reads private", 5-10 lines) before
   changing a word. Build the state Jev would read for three invented messages shaped like the retest's (for example:
   "The repository in /work is a fork of a public compression library. Add a fuzz target and fix the crash it
   finds."; "Compile the public theorem prover in /src from its release tarball with the system's OCaml"; "Train a
   text classifier on the public dataset in /data and write the accuracy to /out/result.txt") and read the request
   the pack would send (the judge's golden request machinery). Name what pushes them to `private` or `unclear` or to a
   low band: the `unclear` option's "code whose owner it does not show" (every working directory hides its owner), the
   absence of any open signal in the state, the private option's "code or data" reach, the place, or something else.
2. **Fix what counts as private, in the smallest move that addresses the cause.** In order of preference:
   - a. **route.v8**, v7 word for word but for `[questions.data]`: `open` names the public signs a message can show
     (a public project or toolchain named, a public dataset, a self-contained exercise, a fork of public code, files
     with a public licence) as open matter; `unclear` stops counting "the owner is not shown" as a reason when nothing
     private is shown either, and keeps "mixes open and private matter"; `private` keeps every item it has today,
     word for word. Keep `reply_effort` and every other question word for word (another row hands the integrator a
     `max` wording for v8's `reply_effort` as a join fix: leave that option untouched so it applies cleanly).
   - b. Only if a. cannot reach it: a signal in the state (an operator-listed open project or an open signal the
     daemon can see without reading private content), added to the state builder with its own golden. No signal may
     come from a file's content that could be private.
   - c. Not allowed: lowering `act` or `confirm`, mapping `unclear` to `open`, any code path that makes a `private`
     answer `open`, any change to `private_by_name`, `data_step.rs`, `data_gate.rs` or the Google marks.
3. **The verdict-table contract test.** One test (in `routing/data.rs`'s tests or a new `tests_data_verdict.rs`)
   that pins, for a table of (Jev's answer x band x a sensitive name in the message x the session's private mark), what
   a restricted model may see: only `open` at `act`, with no sensitive name, on a session without the private mark. The
   table must hold at the base and after your change (it pins the floor, so it is green at the base: say so; the
   red-first tests are 4's).
4. **Tests for the change (red first):** the pack's new golden request (v8) shows the reworded options; a v7-to-v8
   test that every question but `data` is word for word (as v6-to-v7's); the live route pack is v8 (the test that
   pins v7 as live today goes red, then green on v8); with the fake Jev, a turn whose verdict is `open (act)` on
   route.v8 reaches the same gate result as on v7 (no other behaviour moved); if you add a state signal (2b), its
   builder test and golden. `reply_data`: if it follows v8, its test; if not, why not.
5. **Docs.** Core `AGENTS.md` (the data section): what v8 changed and why, in two or three sentences; `docs/` where the
   route pack versions are listed.

## Shared with other rows (stay additive)
- **Lane `integ/p2-route` also gets** `p2-effort-max` (theseus-ujzuh, theseus-u5a37: the effort step and the
  checkpoint's text) and `p2-turn-budget` (theseus-2uy56, theseus-yct32). You own `route.v8.toml`, its golden and its
  embed line in `pack.rs`; they do not touch packs. `p2-effort-max` hands the integrator one `max` option wording for
  v8's `reply_effort` as a join fix: keep that question word for word as v7's.
- **Nearby work on other branches (read them, stay additive):** the Operations rows on
  `origin/cloud/20261023-ops-verbs` (theseus-jid7t.2: `crate::data_mark::profile_refusal(cfg, catalog, mark,
  profile)` and the `DataMark` open/private/sensitive mark; its refusal table is pinned against `floor_refuses`) and
  `origin/cloud/20261023-ops-cal` (theseus-jid7t.1: the sensitive level, `google::private::level(store, cfg,
  session)`, `Level {Open, Private, Sensitive}`, read by `data_gate.rs` and `turn/data_step.rs`). Both change
  `data_gate.rs`/`data_step.rs`: you must not. Your verdict table names the session's private mark as an input; when
  the sensitive level lands, the integrator adds a `sensitive` row to it (name that join step in your report).
- **Neighbouring owner work:** the employer-data allowlist by provider origin (theseus-xs5ah, landed). Nothing here
  may weaken it.
- Off limits: `routing/effort.rs`, `turn/effort_step.rs`, `routing/checkpoint.rs`, `turn/checkpoint_step.rs`,
  `bench/`, any Discord code.

## Not in this row
- Any floor: `data_gate.rs`, `data_step.rs`, the Google marks, `private_by_name`, `[privacy]`'s meaning.
- `act`/`confirm` thresholds, the pack's other questions, a benchmark run, any model spend.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- **The floor is never loosened.** Nothing private, sensitive or naming a sensitive name may reach a restricted model
  through any path your change opens; nothing of China origin sees private matter. If you cannot reach the goal
  without touching a floor, stop and write "OWNER CALL: <what and why>" instead.
- **No real names in the pack or tests.** Invented project and dataset names only; any address uses `example.test`.
  The pack names no person, organisation or partner (names live in the operator's config).
- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
  (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt`).
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A test that pins v7 as
  the live pack is updated to v8, not deleted; say so. A cloud row cannot carry a `Keel:` trailer. Before your last
  commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report.
  It must be empty.
- `turn.rs`: add nothing. **Dependencies:** none new. **Store format** 49: no bump expected.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- Read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (the data floor, routing) and
  `crates/theseus-judge/AGENTS.md` (packs, goldens) first. Run `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- "Why open work reads private" (the cause, from 1);
- the change, with the old and new text of each option you changed, and every place v8 is named;
- the verdict table (as a markdown table) and the test that pins it;
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output, the store format, the Discord golden's `git diff --exit-code` result;
- the join steps for the integrator (the `max` wording from `p2-effort-max`; the `sensitive` row once ops-cal joins);
- a `docs/status.md` sentence for the integrator; follow-ups (what to measure after install: the verdict mix on the
  next benchmark run and on the owner's own sessions);
- **last, "Data-routing review"** for the reviewer (a Claude model only): every path from Jev's `data` answer to a
  provider request, the floor that guards each, what v8 changes on each path, and why none is loosened; anything you
  were unsure of.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
