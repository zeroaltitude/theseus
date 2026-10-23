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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-ops-verbs`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `2d1d8d6f` (2026-10-23: install #75a, a hotfix on release stage 7). That is your base.
Line numbers quoted below are at `2d1d8d6f` but may be off by a few: search by name. The release preamble above is
older than this note; where they disagree about `main`, ceilings or the work in flight, this note wins.

**This wave is patch2 (release 1.0.2): the Operations page.** Three rows start together on `2d1d8d6f` and join a new
lane, `integ/ops`:
- **ops-cal** (R1): the calendars, the calendar cache, `calendar.window`, the sensitive mark, the brief over all
  calendars;
- **ops-verbs** (R2): `session.profile`, `task.rehome`, `task.reschedule`, the data marks on `SessionInfo`/`WorkView`,
  `ProfileInfo.origin`;
- **ops-shell** (R3): the cockpit's `/ops` page, its Board and Threads lenses, the needs-you queue, the parity test.

Three more rows (the Graph, the Timeline, the inspector and the keys) come after these join. Your task's "Not in this
row" section lists what is theirs: leave it for them.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Prefer a META key
or a serde-default field: most of this wave needs no bump. If your row adds a field or a variant to a stored record,
bump it to 50, add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says,
and say so in your report's first section. Several rows may bump; the maintainer renumbers at the join. Never write
the number as a literal anywhere but the constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733. The method table and `SessionInfo`/`ProfileInfo` live here.
  Put every new type in a module file of its own and add to `lib.rs` only the method constants, the `mod`/`pub use`
  line and a field line. Your task names your row's line budget here: keep to it.
- `crates/theseus-core/src/turn.rs` 3,467 / 3,525: add nothing; put turn logic in a module beside it.
- `crates/theseus/src/render.rs` 3,126 / 3,128 and `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500: add nothing.

An unlisted file stays at 2,500 lines or fewer. Never raise a ceiling yourself (the keel guard flags it). If a line
you truly need will not fit, leave the file one line over and say so in your report's first section.

**The owner's rules for this page** (each row's task repeats the ones it touches):
1. Nothing the Agenda, Tasks, Threads and Actions pages show today is lost (a data-parity inventory comes first).
2. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical, and no
   row adds a Discord message. A task change made from the web reaches every task card through the same path the same
   change from any other surface takes; nothing edits a Discord card as such.
3. Calendar data from a sensitive calendar never reaches a model of China origin (nor any model not of US origin),
   never reaches a shared place, and no event title, place, attendee or call link appears in any ledger row.
4. The repository is public: no person's name, no real address, no real calendar id anywhere. Fixtures use invented
   ids under `example.test`. Write "the owner".

**Shared files between this wave's rows** (each row adds; none rewrites):
- `crates/theseus-protocol/src/lib.rs`: ops-cal and ops-verbs each add method constants (ops-cal `calendar.window`
  and the `calendar.changed` notification; ops-verbs `session.profile`, `task.rehome`, `task.reschedule`) and
  ops-verbs adds two field lines. Put your constants on lines of their own, next to the family they belong to.
- `crates/theseus-protocol/src/ledger.rs` (ops-verbs' three row kinds), `crates/theseus-protocol/tests/wire.rs` and
  `tests/wire/*.json`, `crates/theseus-protocol/src/ts.rs` (the type list), `crates/theseus-core/src/rpc/server.rs`
  (the dispatch `match`), and the generated `cockpit/src/protocol.gen/`. Expect the maintainer to merge adjacent
  additions at the join: keep yours together and in one place.
- ops-shell touches no Rust and no generated file.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: ops-verbs, the Operations page's new verbs: a sticky model per session (`session.profile`, with its revert), `task.rehome`, `task.reschedule`, the data mark on `SessionInfo`/`WorkView`, and `ProfileInfo.origin` (theseus-jid7t.2)

Branch: `cloud/20261023-ops-verbs`. Every commit's subject carries `theseus-jid7t.2`. Deadline for the report: 5 hours
after you start.

**Why.** The owner wants the cockpit's Operations page to act on any task, any session and any model at any time.
Almost every act already has a protocol method. This row adds the three that do not, and the read-only facts the page
needs to show what a model switch would do before the owner makes it:
- **a model switch from the web holds for later turns** (the owner's decision), with an easy revert;
- **re-home** a task: its report goes to another conversation or thread from now on;
- **reschedule** a deferred task's start (the page's drag on the timeline, with Undo);
- each session's and task's **data mark** (open, private, sensitive) and each profile's **model origin**, so the page
  greys out exactly what the floor would refuse.

The page itself (the model menu, the drag, the lock glyphs) is a later row. It consumes these methods exactly as this
row specifies them.

**The owner's rule for every web act (decision 3, verbatim in spirit):** a web action never edits a Discord card as
such. A task change made from the web reaches every task card everywhere exactly as the same change from any other
surface would. So none of the three new methods sends a Discord message or calls Discord code; each one changes state
through the path any other surface's change takes (the store, the ledger row, the `task.changed`/session pushes the
surfaces already follow). The locked Discord DM task flow's golden, `crates/theseus-discord/src/golden/dm_flow.txt`,
stays byte-identical.

**Risk: medium.** `session.profile` sits on the data floor: a sticky profile must never let a turn reach a model the
floor would refuse. Your report ends with a data-routing section a reviewer reads line by line.

**Prerequisite: base `origin/main` at `2d1d8d6f`.** Your branch joins the lane `integ/ops`. Run the keel guard against
`origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `ops-verbs` | theseus-jid7t.2 | `session.profile { session_id, profile \| null }` (sticky from the next turn, or a running task's next loop; the floor's refusal; `null` reverts); `task.rehome { task, home }`; `task.reschedule { task, start_at_ms \| start_after_ms \| start_when }`; ledger rows `session.profile_set`, `task.rehomed`, `task.rescheduled`; `SessionInfo.data`, `WorkView.data`, `ProfileInfo.origin`; `protocol.gen` regenerated | protocol: a new `ops.rs` module (params, results, `DataMark`), the method lines and two field lines in `lib.rs` (at most 8 lines), `work.rs` (`WorkView.data`), `ledger.rs` (three kinds), `ts.rs`, `tests/wire.rs` + three `tests/wire/*.json`; core: new `rpc/session_profile.rs`, `rpc/task_rehome.rs` (reschedule beside it or in its own file), one dispatch arm each in `rpc/server.rs`, a new `data_mark.rs` (the session's mark, one function), the sticky read where a turn picks its profile (in a module beside `turn.rs`, not in it), `rpc/sessions.rs`/`rpc/info.rs`/`rpc/work_board.rs` (filling the new fields), the profile list's builder (origin); kernel: a new module for the rehome and reschedule writes (`kernel.rs` is near its ceiling); tests in new `tests_*` files; `cockpit/src/protocol.gen/` (regenerated only); core `AGENTS.md` (one paragraph) | L |

## What exists (at `2d1d8d6f`; search by name)
- **The per-turn model.** `turn.submit` takes `profile` today (one turn only). Its resolution, with `carried`, is in
  `crates/theseus-core/src/rpc/methods.rs` near :1005-1025; the route reads the session's last route near
  `turn/route_step.rs` :325 (`session.routed`). `SessionInfo.profile`/`model` (protocol `lib.rs` near :1899) are "the
  last turn's; continuations reuse it". A task's turns run in its loop; find where a task's loop picks its profile at
  each loop boundary (`task.rs`, `turn/`).
- **The floor.** `turn/data_step.rs` (read its module doc): `floor_refuses`, `private_session`, `uncleared_profile`,
  `restricted_profile`. A session that read Google content (`google::private::session`, or a task whose parent chain
  did: `lineage`) runs only on a profile of US origin (`Config::profile_cleared_for_sensitive`); a profile of
  restricted origin (`cn`, `unknown`) runs only for a turn admitted open. The provider chokepoint is `data_gate.rs`.
  `catalog.rs`: `ModelOrigin`, `origin_of`; `data_gate::word(origin)` gives the origin's word. `Config::profile_origin`
  (config.rs near :2024).
- **Data marks a session can carry today:** the Google private mark (META, `google/private.rs` `KEY`, `meta_key`,
  `session`, `lineage`), and the data judgment's verdict (`routing/data.rs` `DataAnswer`; `judge/reply_data.rs`).
  There is no "sensitive" session mark at the base: **ops-cal (R1, in parallel) adds it** as a level on the private
  mark, with a function `google::private::level(store, cfg, session)`. Do not build it here (see "What to build" 4).
- **Tasks.** `rpc/task_start.rs` (read its module doc): the parent and carve, `report_to` (`reports_to_of` near :63),
  a deferred start (`start_at_ms`, `start_after_ms`, `start_when`, parsed near :203-221) that opens the task asleep in
  `Waiting{DueAt}`. The kernel holds the execution's `reports_to` (`crates/theseus-kernel/src/kernel.rs` near :1105)
  and the due wake (`Wake::DueAt` in `kernel/src/tasks.rs` near :72, :252, :483; `terms.rs` near :177). The task's
  `wake_parent` is in core `task.rs` near :116. `TaskInfo.starts_at_ms` is in protocol `lib.rs` near :1755.
  `rpc/task_pause.rs` is a small method of the same shape as yours: a model for a new task method (refusals, `who`,
  the ledger row, the push).
- **Protocol.** The method table is in `crates/theseus-protocol/src/lib.rs` (`TASK_*` near :287, `SESSION_*` near
  :233, `PROFILE_LIST`/`PROFILE_USE` near :223). `SessionInfo` near :1874, `ProfileInfo` near :2022, `TaskInfo` near
  :1709; `WorkView` in `work.rs` near :141. Ledger kinds in `ledger.rs` (`SessionMode = "session.mode"` near :306 is
  a neighbour). Dispatch is the `match` in `crates/theseus-core/src/rpc/server.rs` (`TASK_PAUSE` near :363).
  `cockpit/src/protocol.gen/` is written by theseus-protocol's test (`crates/theseus-protocol/AGENTS.md` near :250).
- **The Discord golden.** `crates/theseus-discord/src/tests_dm_flow_golden.rs` and `golden/dm_flow.txt`: run
  `cargo nextest run -p theseus-discord dm_flow` (never with `THESEUS_GOLDEN=write`).

## What to build
1. **`session.profile { session_id, profile: string | null }` → `{ session_id, profile, previous, refused? }`.**
   - Sets the profile the session's turns run on from its next turn; for a running task, from its next loop
     boundary. It does what `turn.submit`'s per-turn `profile` does, but it holds. Precedence: a turn's own `profile`
     beats the sticky one for that turn; the sticky one beats the last turn's and the parent's; routing may still move
     a turn (as `carried` allows today), and the checkpoint's `moved_to` still says so.
   - `profile: null` is the **revert**: back to the default (the parent's, or routing's). `previous` is what was set
     before, so the page's Undo is one more call with it.
   - Stored as a META key per session (no format bump), read where the turn picks its profile, in a small module
     beside `turn.rs` (not in it: it is at its ceiling).
   - **Refusal, in the floor's own words, and no state change:** refuse exactly the profiles the floor would refuse
     for that session's next turn whatever its message. Today that is: a session (or task lineage) with the private
     mark refuses any profile not cleared for sensitive (US origin only), as `floor_refuses` does; an unknown profile
     is refused. A restricted-origin profile on an **open** session is NOT refused (a turn admitted open may run on
     it); the answer says that the floor decides per turn and may move it. Derive the refusal from the same functions
     the route step uses (`profile_cleared_for_sensitive`, `profile_origin`, the private-session predicate), so the two
     can never disagree; a later row's model menu greys out by calling the same check, and a contract test there pins
     them together. Put the check in one `pub(crate)` function and name it in the report.
   - **Ledger:** `session.profile_set` (by, session, from, to). No model call.
2. **`task.rehome { task, home }` → `{ task, from, to }`.**
   - `home` is a conversation's session id, or `thread:<slug>`. The task's report target (`reports_to`) and its
     `wake_parent` target move to the new home. **The budget stays carved from the original parent**, and the answer
     says so in one clause.
   - **Refused:** an ended task; a home that does not exist; a home in a shared place; a home whose data mark is
     weaker than the task's (a private task cannot report into an open conversation): use `data_mark.rs` (4) for both
     sides. Authority as `task.start`'s: the owner, from a private place (`judge_act(Act::Task)`).
   - **Ledger:** `task.rehomed` (by, task, from, to). The task's views and cards learn of it through the push the
     surfaces already follow for a task change. No Discord message, no Discord code.
   - If the kernel's `reports_to` is part of a stored record whose layout must change, bump the format to 50 as the
     note says; prefer a write that keeps the layout (the field exists). Kernel code goes in a new module; add to
     `kernel.rs` only the `mod` line and at most one call.
3. **`task.reschedule { task, start_at_ms | start_after_ms | start_when }` → `{ task, starts_at_ms, previous }`.**
   - Only for a deferred task that has not started (`Waiting{DueAt}`, `TaskInfo.starts_at_ms` set); anything else is
     refused with the reason. Exactly one of the three time fields, parsed by `task.start`'s own parser (share it; do
     not copy it). A time in the past is refused.
   - **Ledger:** `task.rescheduled` (by, task, from, to). The page's Undo calls it again with `previous`.
4. **The data mark.** `DataMark = "open" | "private" | "sensitive"` (protocol, in `ops.rs`), on `SessionInfo.data` and
   `WorkView.data` (both `Option`, serde-default and skipped when `None`, so old readers and the wire tests stay
   green). One core function, a new `data_mark.rs`: `session_mark(store, cfg, session_id) -> DataMark`, the mark as
   the floor reads it: `private` when the Google private mark is on the session or its lineage, or the data judgment's
   stored verdict says private; else `open`. **`sensitive`:** ops-cal adds a sensitive level on the private mark in
   parallel. Do not add one here: define the variant, document it, and leave a one-line `// ops-cal:` comment where the
   level will be read, so the integrator wires `google::private::level` in at the join (a named join fix). Test the
   refusal logic of 1 and 2 against `DataMark::Sensitive` directly (a pure function), so the join changes one line and
   no test.
5. **`ProfileInfo.origin: "us" | "cn" | "eu" | "unknown"`** (and any other origin word `data_gate::word` returns), from
   `Config::profile_origin`, filled wherever `profile.list` builds its rows.
6. **Regenerate** `cockpit/src/protocol.gen/` (theseus-protocol's test), and add the three wire samples.
7. **Docs.** Core `AGENTS.md`: one paragraph for the three verbs, the sticky profile's precedence and revert, the
   refusal function, and the data mark. If `docs/` lists protocol methods, add the three there.

## Tests (red first: write each, see it fail on the base code, then fix; each behaviour change gets a planted revert)
- **`session.profile`:** a CN profile on a private session is refused (no state change, no ledger row); a non-US
  (EU) profile on a private session is refused; a US profile on a private session is set; a CN profile on an open
  session is set (and the answer says the floor decides per turn); the sticky profile holds across two turns (the fake
  provider sees the sticky profile's model both times); a turn's own `profile` beats it for one turn and the next turn
  is sticky again; `null` reverts and `previous` round-trips (Undo); a running task picks it up at its next loop and
  not mid-loop; an unknown profile refused; the refusal function and the route step's floor agree for every
  (mark × origin) pair of a small table (the contract a later row's menu relies on); the ledger row's fields.
- **`task.rehome`:** a task's report goes to the new home (its next report lands there, not in the old one); the
  carve stays charged to the original parent (the parent's spend and the new home's are checked); refused for an
  ended task, a missing home, a home in a shared place, and a private task into an open home; `thread:<slug>` works;
  the ledger row; authority refused from a shared place.
- **`task.reschedule`:** a deferred task's start moves (the due scan wakes it at the new time on a paused clock, not
  the old); `previous` and a second call restore it (Undo); each of the three time forms; refused for a started task,
  a task with no deferred start, a time in the past, and two time fields at once; the ledger row.
- **The data mark:** `SessionInfo.data` and `WorkView.data` are `open` for a plain session and `private` after a
  Google read (use the fake Google of `google/tests.rs`) and for a task under it; absent fields deserialize (old
  readers).
- **`ProfileInfo.origin`** for a US, a CN and an unknown profile.
- **No Discord message from any of the three:** with the fake Discord bound, each verb sends nothing; and
  `cargo nextest run -p theseus-discord dm_flow` passes with the golden unchanged (`git diff --exit-code
  crates/theseus-discord/src/golden/`).
- Run `cargo nextest run -p theseus-protocol`, `-p theseus-kernel`, `-p theseus-core task`, `-p theseus-core session`,
  `-p theseus-core data`, `-p theseus-discord`, then the gate.

## Not in this row
- The calendars, `calendar.window`, and the sensitive level itself: ops-cal, in parallel (see 4).
- The cockpit's pages and components (the model menu, the drag, the lock glyph, the `data:` filter): later rows. Only
  `cockpit/src/protocol.gen/` changes here, by regeneration.
- Any Discord code or message, and `golden/dm_flow.txt`. `profile.use` (the daemon's default) is unchanged.
- `[routing]` and the floor's rules: unchanged. This row reads them; it does not change what any turn may run on.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- **The floor is never bypassed.** No path from `session.profile` may let a turn's request reach a model the route
  step or the chokepoint would refuse; if the sticky profile is refused at turn time (a mark taken after it was set),
  the turn moves as the floor moves any refused base today, and the `moved_to` says so. Test that case too.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Protocol `lib.rs`:** at most 8 added lines (the three method lines, the module line, two field lines, and at most
  two doc lines). Everything else in `ops.rs`. `kernel.rs` and `turn.rs`: the `mod` line and at most one call each.
- **Dependencies.** None new. **Store format** 49: META keys need no bump; if the rehome needs one, bump to 50 and say
  so in the report's first section.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented; any address uses
  `example.test`.
- Read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (the data floor, tasks, the place rule),
  `crates/theseus-kernel/AGENTS.md` and `crates/theseus-protocol/AGENTS.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-7), with the file and function for each, and the wire shapes;
- the sticky profile's precedence as built, where it is read, and how a running task picks it up;
- the refusal function's name and signature (the later model menu calls it), and the (mark × origin) table it pins;
- where `// ops-cal:` waits for the sensitive level, and the one line the integrator adds at the join;
- the Discord check: no message from any verb, and the golden's `git diff --exit-code` result;
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output, the store format, and the `lib.rs`/`kernel.rs`/`turn.rs` lines you added;
- a `docs/status.md` sentence for the integrator;
- follow-ups;
- **last, a section "Data-routing review"** for the reviewer (a Claude model only reviews it): every way a turn's
  model is now chosen (the turn's own profile, the sticky one, the last turn's, the parent's, routing, the floor's
  move), and for each, the floor that guards it and the test that pins it; how `task.rehome`'s mark check keeps a
  private task's report out of an open or shared home; and anything you were unsure of.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
