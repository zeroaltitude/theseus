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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7b-hold`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ae420a49` (2026-10-22: release stage 6, install #74, which landed `integ/tail6` on top
of stage 5's `f6f2900e`); that is your base. Line numbers quoted below are at `ae420a49` but may still be off by a few:
search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f6f2900e`** (`git log --first-parent f6f2900e..HEAD` lists it):
- `ae420a49`: `integ/tail6` (the paced stand-in model; the ontology sweep's titles off the worker, its pair rule, kept
  kind names and the cockpit's renames; the thread growth race, the held row's timing, confirms and the budget question
  naming their turn; the data-gate word only when the gate decided, its counters; the task home's restart test and
  `sticky_unsure`; the recall bench's later-session plan and read-only test trees).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/turn.rs` 3,467 / 3,525
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you:** nothing else is integrating. Two waves of rows base on `ae420a49` and join the lane
`integ/tail7`: the four `tail7-*` rows started an hour before you (ontology sweep, effort words, threads/TUI, recall
bench and data-gate word), and the six `tail7b-*` rows of this wave. A local lane also edits `scripts/keel-guard.py` and
its test: touch neither. Each row owns the files named in its task's "Rules for this row" and "Not in this row"
sections. Those win over anything older. Stay out of the other rows' files.

**The Discord DM task flow is locked** (the owner, 2026-10-22: "please do not change how it is now, all of the little
touches are so good"): the task reply, the started and thread lines, the board, the forum posts and live cards, the
approval card and the task reports. No row in this wave changes their text, order or buttons. If your change would move
one of them, stop and say so in the report instead.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7b-hold, a notify hold tells the owner once, an unset `external_text` follows `[policy] enforcement`, and the public template's `sensitive_names` line becomes placeholders (theseus-nsqqo, the template line)

Branch: `cloud/20261021-tail7b-hold`. Every commit's subject carries its Beads id (`theseus-nsqqo` for steps 1-2,
`theseus-a9zg9` for step 3). Deadline for the report: 3 hours after you start.

**Why.** Two of the owner's answers from 2026-10-22:
- **The external-text hold (theseus-nsqqo).** After a session reads outside text, every call that acts is held: under
  `[policy] external_text = "ask"` it waits for approval, under `"notify"` it runs with a notice. The owner's words:
  "Didn't we set this to notify instead of ask? I'd say, it seems to me that notifying once is more than enough, and
  then the expiration idea can be retired." So: (a) under `notify`, a hold notifies ONCE, at its first acting call;
  later acting calls under the same hold run quietly (no notice, no card); (b) an unset `external_text` follows
  `[policy] enforcement` instead of defaulting to `ask`. The expiry idea (a hold lapsing after hours) is RETIRED: build
  no lapse, and leave `a_hold_from_another_day_says_the_day_and_how_long_ago` as it is (an old hold still holds, and
  says its age).
- **The public template's `[privacy] sensitive_names` line.** The template's commented example line (and the comment
  prose above it) lists real organisations' names, and theseus-judge's
  `the_pack_text_names_no_organisation` reads that line and hard-codes the same list. The repository is public. The
  owner: replace them with placeholders; his own config keeps the real list.

**Risk: medium for step 1** (it removes notices the owner sees today after the first one), low for 2-3.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7b-hold` | theseus-nsqqo, theseus-a9zg9 (the template line) | notify once per hold; unset `external_text` follows `enforcement`; template placeholders + the judge test | core `external.rs` (`Mode`, `gate`, the wording helpers), `toolrun/order.rs` (only where `gate` is called, if the once-rule needs the call site), core `config.rs` (`PolicyConfig.external_text` only), core `tests_external.rs` (new tests) or a new `tests_external_once.rs`, the template `crates/theseus-core/config/theseus.example.toml` (the `[policy]` `external_text` lines and the `[privacy]` comment block only), `crates/theseus-judge/src/pack.rs` (`the_pack_text_names_no_organisation` only), core AGENTS.md (the external-text paragraph) | M |

## What exists (at `ae420a49`; search by name)
- `crates/theseus-core/src/external.rs` (1,086 lines): `pub enum Mode { #[default] Ask, Notify }` near :58
  (`posture()` near :74: Ask -> `Posture::Approve`, Notify -> `Posture::Notify`); `read()` near :105; `taken()` near
  :260; `held()` near :367; `exempt()` near :382; `gate()` near :392 (raises the call with `Decision::at_least` and puts
  the hold on `Decision.external` only when the hold raised it); `hold()` near :431 (the session record and a
  `SessionExternalRead` ledger row; only the first read counts); `why()`/`why_at()`; unit tests from :652, among them
  `a_call_that_acts_waits_and_a_read_keeps_its_posture` (covers notify near :700-797) and
  `a_hold_from_another_day_says_the_day_and_how_long_ago` (:847).
- `gate()` is called once, from `crates/theseus-core/src/toolrun/order.rs` near :207 (a granted call or a task's own
  close skips it, :199-205).
- A notice: `Decision::at_least` (`policy.rs` near :179) attaches `Notice { kind: "notify", setting: "[policy]
  external_text = notify", .. }`; `toolrun.rs` `start()` (near :1001) -> `Self::notified()` (near :1081) ->
  `fact::tool::ToolNotified` (`fact/tool.rs` near :84): the `ToolNotified` ledger row and `Event::PolicyNotified`.
  Discord draws a "Ran with a notice" card only under `[discord] notice_embeds` (default false); the CLI prints it
  (`crates/theseus/src/render.rs` near :276).
- The hold's record: `ExternalText { since_ms, tool, url, node_id, from_session, via, query }` in
  `crates/theseus-protocol/src/lib.rs` near :1237 (stored on the session).
- Config: `PolicyConfig` in `crates/theseus-core/src/config.rs` near :642: `enforcement: Posture` (`#[serde(default)]`,
  `Posture::Open`; `policy.rs` near :58, `Open < Notify < Approve`), `external_text: external::Mode`
  (`#[serde(default)]`, so unset = `Ask` today). Template `[policy]` near :624-634 (`enforcement = "notify"`,
  `external_text = "ask"` with its comment).
- `crates/theseus-core/src/tests_external.rs` (1,114 lines): the integration tests (`a_fetch_then_a_run_waits_in_this_
  turn_and_the_next_until_the_operator_trusts_it`, `the_hold_survives_a_restart`, `a_task_that_a_holding_session_
  starts_holds_it_too`, ...); helpers `session()`/`a_hold()` in `tests_security.rs` near :156-164.
- The template line: `crates/theseus-core/config/theseus.example.toml` near :1735-1736 (`# [privacy]`, then
  `# sensitive_names = [...]` with four real names), the prose comment above it (near :1720-1733: its matching
  examples use the same names), and `crates/theseus-judge/src/pack.rs` `the_pack_text_names_no_organisation` near :1759
  (reads the line with `include_str!`, asserts the exact four-name list near :1771, then checks no embedded pack text
  hits a name and that a planted text does). `PrivacyConfig` is `config/privacy.rs`; the matcher is
  `crates/theseus-judge/src/names.rs`.

## What to build
1. **Notify once (nsqqo a).** Under `Mode::Notify`, the first call that acts under a hold runs with the notice (as
   today); every later acting call under the SAME hold runs at the posture it would have had without the hold (no
   notice, no `ToolNotified` row from the hold, no `PolicyNotified` event from it). A new hold (after the session is
   trusted and then reads outside text again) notifies once again. `Mode::Ask` is unchanged: each acting call still
   waits until the session is trusted. Where to record "this hold has notified": prefer a fact you already have (the
   `ToolNotified` ledger row naming the hold's `since_ms`, or a META key per session) over a new stored field; if you
   add a serde-default field to `ExternalText`, that is a stored record: bump the store format as the wave note says
   and add the old layout's sample. Say which you chose and why. It must survive a restart (a restarted daemon does not
   notify the same hold twice) and a hold carried into a task, a report or a job (the hold the child takes is a new
   hold for the child's session: it notifies once there too; say so in the report if you find the code treats it
   otherwise).
2. **Unset follows enforcement (nsqqo b).** Make `external_text` an optional key: when it is set, it wins as today;
   when it is unset, the hold's mode follows `[policy] enforcement`: `approve` -> ask, `notify` -> notify, and `open`
   -> notify (a hold never goes silent: its floor is the one notice; say so in a doc comment). Keep one place that
   answers "the mode in force" (a method on `PolicyConfig`), and use it wherever `external_text` is read today (search
   `external_text` across crates, the health/explain lines included: `rpc/explain.rs`, `judge/gate.rs`). The setting
   named in a notice says which key decided (`[policy] external_text = notify`, or `[policy] enforcement = notify`
   when it followed). Update the template's `[policy]` comment: the key is optional, what unset does, and that notify
   tells you once per hold. Leave the template's `enforcement = "notify"` and `external_text` lines as values the
   template shows (the template documents; keep `external_text` shown, commented or not, with a one-line note).
3. **Template placeholders (the owner's call).** Replace the four names on the `# sensitive_names = [...]` line with
   invented placeholders (for example `["examplecorp", "examplecorp-internal", "example-repo"]`) and rewrite the comment
   prose's matching examples to the same placeholders (keep every rule it explains: case-insensitive, whole words, a
   multi-word name as a run, a hyphenated name as a unit, list each spelling). Rewrite
   `the_pack_text_names_no_organisation` so it (a) still reads the line from the template, (b) asserts the template's
   list is placeholders (every entry starts with `example`, or the exact placeholder list), (c) still checks that no
   embedded pack text hits a placeholder name and that a planted text does, and (d) holds a check that the template
   names none of the four old names: the test cannot spell them either, so carry them as a hash (for example a SHA-256
   of each lowercased name, computed once in the test's comment-free constant) and assert no word of the template
   hashes to one. Grep the repository for each of the four old names (you have them from the old line) and list every
   other hit in the report (fixtures, docs, AGENTS.md); change hits only in the files of this row, name the rest.

## Tests (each with a planted revert; the first is the key test)
- **`a_notify_hold_tells_the_owner_once`** (key): a session under `external_text = "notify"` reads outside text, then
  makes three acting calls: the first runs with the notice (one `ToolNotified`, one `PolicyNotified`), the next two run
  with none. Plant: notify on every call (today's behaviour) -> it fails.
- `a_notify_hold_tells_once_across_a_restart` (the rig of `the_hold_survives_a_restart`). Plant: keep the once-mark in
  memory only.
- `a_trusted_session_that_reads_again_is_told_again` (a new hold notifies once more).
- `an_ask_hold_still_waits_on_every_call` (Ask unchanged: two acting calls, two approvals asked).
- `an_unset_external_text_follows_enforcement` (table: approve -> ask, notify -> notify, open -> notify; set wins).
  Plant: keep the old `#[serde(default)] Ask`.
- The rewritten `the_pack_text_names_no_organisation`. Plant: put one old name back on the template line (in your
  scratch tree only; never commit it) -> it fails on the hash check.
- `a_hold_from_another_day_says_the_day_and_how_long_ago` and the other `external.rs`/`tests_external.rs` tests stay
  green unchanged; if one asserts a notice on a second call, it is the old rule: change only that assertion and name it.
- `cargo nextest run -p theseus-core external`, `-p theseus-core policy`, `-p theseus-judge`, `-p theseusd
  default_config`, then `-p theseus-core` whole.

## Not in this row
- No expiry or lapse of any kind (retired by the owner).
- The Discord DM task flow is locked (the wave note): change no Discord file. The "Ran with a notice" card and the 🔔
  notified tool lines keep their text; under a notify hold they simply appear once instead of on every call. That is
  the owner's own asked change, and the only way this row may move what the flow shows: name every test whose expected
  notices drop in the report.
- The other wave rows' files: tail7b-budgets (`turn/detour_step.rs`, `config/threads.rs`, `first_request.rs` tests),
  tail7b-tiers (`turn/route_step.rs`, `config/memory.rs`, `config/defer.rs`, recall's pack step, the ontology walk),
  tail7b-cn (`own_bench/drafts.rs` and the China-origin draft review), tail7b-messages (the proactive messages outside
  the task flow), tail7b-golden (the Discord tests). The tail7 rows: tail7-onto (`judge/onto/*`, cockpit ontology),
  tail7-effort (protocol `route.rs`, `routing/effort.rs`, cockpit `lib/route.ts`, `protocol.gen`, one `held_high`
  template comment), tail7-threads (the TUI, `tests_hold_compile.rs`), tail7-recall (`bench/recall/*`, `scratch.rs`,
  `turn/data_step.rs`, `rpc/mod.rs`). Template edits: only the `[policy]` external-text lines and the `[privacy]`
  comment block are yours (tail7-effort edits one `held_high` comment line elsewhere in the same file).
- `scripts/keel-guard.py` and its test (a local lane).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Fewer notices is the
  owner's asked change, not a weakened guard: Ask stays as strict as it is, and the first notice still fires. A cloud
  row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- **Dependencies.** None new (a SHA-256 is in the workspace already: search `sha2`; if it is not a dependency of
  theseus-judge's tests, use another stable hash the crate has, and say which).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Never write any of the four old names in a
  commit, a test, a comment or the report: refer to them as "the old line's names".
- Read `crates/theseus-core/AGENTS.md` (external text, policy, the store's version rule, the template's tests) first.
  Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: where the once-mark lives and why; how it behaves across a restart, a task, a report and a job; the
mode-in-force method and every reader you moved to it; the notice's setting text before/after; the template's
`[policy]` and `[privacy]` blocks before/after (placeholders only); the grep hits for the old names (file:line only,
never the names); each test and its plant with the failing line; the keel guard's output; the store format (and the
bump, if you made one); a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
