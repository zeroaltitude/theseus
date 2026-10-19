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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-onto-core`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: onto-core, the ontology crate grows: Jev, the sweep and the dream may write; the kinds of the design filled; an undo record (theseus-e3mjc)

Branch: `cloud/20261019-onto-core`. Every commit's subject carries `theseus-e3mjc`. Deadline for the report: 5 hours
after you start.

**Why.** The owner approved a design on 2026-10-19: Jev, judging every inbound message, files it into the ontology
and grows the ontology itself, minting new categories and new kinds as the world shows them ("category expansion,
not just category inclusion"). Today the ontology crate refuses everything that design needs. Only four kinds exist
(guild, channel, person, topic). The guild kind is never written, and a channel category never gets its guild as its
parent. The origins `jev`, `sweep` and `dream` are refused by name as "not built" (`Origin::comes_with`,
`Origin::built` in `crates/theseus-ontology/src/kind.rs`). This row is the crate side only: it makes those writes
legal, checked and undoable. Nothing calls Jev here: the in-loop wire, the RPCs, the sweep's logic and the cockpit
are later rows (R3, R5, R7). Build no caller beyond what the tests need.

## What exists (read these; lines move)
- `crates/theseus-ontology/src/kind.rs`: `GIVEN` (three names), `Basis`, `Rule`, `Origin` with `comes_with`/`built`,
  `Kind` and `check_row`/`check_basis`, `seeds()` (the four rows), `given_seed`.
- `crates/theseus-ontology/src/ontology.rs`: `Ontology::put`/`check` (who may write which record), `load` (rebuilds
  from stored records, dropping a record that never checks, with its key and why), `check_member`/`check_members`,
  `check_table`, and the helpers `interprets` and `stored_by` at the bottom, which today name only the operator and
  the import.
- `crates/theseus-ontology/src/category.rs`: `Category` (`added_by`, `parent`, `retired_ms`, `merged_into`),
  `Membership` (`origin`, a confidence, `given`/`operator`/`import` constructors), `MemberList`.
- `crates/theseus-ontology/src/refusal.rs`: `Refusal` (`UnbuiltOrigin`, `Writer`, `Given`, `InUse`, ...).
- `crates/theseus-ontology/src/lib.rs`: `Record` and the store keys (`onto:kind:<name>`, ...).
- `crates/theseus-ontology/src/tests.rs`, `tests_people.rs`, `tests/golden.rs` and `tests/golden/seed-kinds.json`
  (the seed rows as stored), `tests/props.rs`.
- `crates/theseus-core/src/ontology.rs`: `place_category`, `given` (a session's given memberships, read from its
  place at compile, never stored), `bound_category` (the category a place makes at its first bind: today it sets
  `parent: None`), `Walk`.
- `crates/theseus-core/src/rpc/ontology.rs`: `bind_categories` (called from `rpc/bindings.rs`'s `bind_places`), which
  writes the bound places' categories in one frame. `crates/theseus-core/src/places.rs`: `BoundPlace`, whose
  `guild` is a guild channel's guild id.
- `crates/theseus-core/src/fact/ontology.rs`: the ledger rows' facts. `crates/theseus-protocol/src/ledger.rs`: the
  ledger kinds (`OntologyCategory`, `OntologyGuidance`, `OntologyMembership`, `OntologyMerged`).
- The design's sections this row builds, for reference (you have no copy of the design; everything you need is here).

## What to build

1. **The three origins are built.** `Origin::comes_with` returns `None` for `Jev`, `Sweep` and `Dream`, and their
   doc comments say who writes from them: `jev` is an inbound judgment, `sweep` the nightly re-association, `dream`
   the nightly consolidation. If nothing else then uses `Refusal::UnbuiltOrigin`, keep it only if `comes_with` still
   has a `Some` arm; otherwise remove the variant and its arm (a refusal nothing can raise is dead code).
2. **Who may write what.** Extend `Ontology::check` so that:
   - an interpreted kind's categories and member lists may be written by any origin the kind's `assigned_by` names
     (today: the operator always, the import when named). The operator keeps writing any of them;
   - a given kind's stored side (person's) may be written by `jev` when its row names it, as the import's is today;
   - **the operator wins on conflict.** A list written by `jev`, `sweep` or `dream` must keep every entry of the
     current list whose origin is `operator` or `import`, unchanged. It may add its own entries and replace or drop
     entries from the machine origins. A list that drops or changes an operator's entry is refused with a `Writer`
     refusal that names the entry. The operator may still replace anything;
   - a category written by a machine origin has `added_by` of the form `<origin>:<id>`, for example
     `jev:<judgment id>`. Check the prefix matches the writer, and keep the id free text within `ADDED_BY_MAX`;
   - **kinds:** the operator writes any row, as today. `sweep` may add a *new* interpreted kind only. It may not
     change an existing row, may not add a given kind, and its row's `assigned_by` must name `sweep` and `jev`.
     `jev` and `dream` never write a kind.
3. **Kind retire.** A `Kind` gains `retired_ms: Option<u64>` (serde default, skipped when `None`). A retired kind
   takes no new categories or lists. Retiring is refused, with `InUse`, while any category of the kind is live or any
   stored list names it. A given or seeded kind cannot be retired. The operator may retire any other kind, and the
   sweep only a kind the sweep added. Precedence uniqueness ignores retired rows.
4. **Two optional fields on `Kind`:** `examples: Vec<String>` (at most five category names, each a valid name) and
   `mined_from: Vec<MinedFrom>`, a closed enum of the name sources code can mine: `titles`, `paths`, `hosts`,
   `capitalised`. Both are serde default and skipped when empty, so a row without them serialises to exactly
   today's bytes.
5. **The kinds of the design.** `GIVEN` grows to five, `["guild", "channel", "thread", "project", "person"]`, and
   `check_basis` learns the two new seeds. `thread` and `project` are *given by the harness*: facts the core holds,
   read at compile from the thread and project records the way a channel is read from the place, never stored per
   session and never set through the API. Their memberships' origin is `Transport`: widen its doc to "a session's
   place, or the harness's own thread and project records: given kinds only". Add no new origin. `seeds()` becomes
   this table (precedence orders a compile: higher governs):

   | Kind | Basis | assigned_by | Per session | Parent | Precedence | Rule |
   |---|---|---|---|---|---|---|
   | guild | given | transport | 1 | – | 10 | chain |
   | channel | given | transport | 1 | guild | 20 | chain |
   | thread | given | transport | 1 | channel | 25 | chain |
   | project | given | transport | 1 | – | 28 | chain |
   | person | given | transport, operator, import, jev | many | – | 30 | intent_line |
   | topic | interpreted | operator, import, jev, sweep, dream | 3 | topic | 40 | chain |
   | event | interpreted | operator, jev, sweep | 3 | project | 50 | chain |
   | resource | interpreted | operator, jev, sweep | many | – | 60 | intent_line |
   | capability | interpreted | operator, jev, sweep | many | – | 70 | intent_line |
   | expertise | interpreted | operator, jev, sweep | 3 | expertise | 80 | chain |
   | culture | interpreted | operator, dream | many | – | 90 | chain |

   Each row gets a one- or two-sentence description in the existing rows' style:
   - event: a dated happening (a release, an outage, a meeting, a deadline); the date is mined in code, never Jev's;
   - resource: a repository, service, host, document or dataset the session works with;
   - capability: a tool, skill or extension the session needed;
   - expertise: the domain the session draws on;
   - culture: how things are done here, learned from corrections over time.

   Seed `examples` and `mined_from` where they are obvious (resource: `paths`, `hosts`; capability: none; topic:
   `titles`, `capitalised`) and leave the rest empty. `seed-kinds.json` changes by exactly the new rows and the two
   widened `assigned_by` lists. Check that the guild-channel and DM compile goldens do not move. If they do, the walk
   changed and you must find out why before you update one.
6. **The guild is filled and a channel's parent set, at bind.** In `bound_category` / `bind_categories`, a guild
   channel's place with a `guild` id also makes the guild's category (`guild:<id>`, named by the guild's name if the
   binding knows one, else the id), and the channel category's `parent` is that guild category. One frame, as today.
   A re-bind that changes nothing writes nothing, and an existing store's channel categories with `parent: None` are
   updated once at the next bind. `given()` then yields the guild membership too, read through the channel category's
   parent at compile, never stored. A DM is unchanged.
7. **Thread and project at compile.** Find where the core builds a session's given memberships for the walk
   (`given()` and its callers, `Walk::of`). If the session's thread place and its project (`crates/theseus-core/src/
   project/`, `thread_*.rs`) are reachable there without new plumbing across modules, add their memberships, and make
   the categories at the thread's or project's creation the way a place's are made at bind. If that needs more than a
   small, local change, build only the kinds rows, leave the reader to row R6, and say exactly what is missing in
   the report. Either way the reader rule holds: no row without its reader. If you leave the reader, say so in the
   rows' descriptions ("read at compile from R6").
8. **The undo record.** A machine origin's write must be undoable byte for byte:
   - add `ontology.undo` to the ledger kinds (`OntologyUndo` in `crates/theseus-protocol/src/ledger.rs`, next to
     the other ontology rows);
   - in the crate, add one function, for example `Ontology::undo(&self, wrote: &[Record], before: &[Option<Record>])
     -> Result<Vec<Record>, Refusal>`, that gives the records restoring each key to `before`: the previous member
     list as it was, and a category minted by the write retired if nothing else now uses it (else refused with
     `InUse`);
   - in the core (`fact/ontology.rs`), a machine write's ledger rows carry what undo needs (the prior list, or "none"
     for a minted category) in the row's existing payload;
   - add one core function that, given such a frame, writes the inverse records and an `ontology.undo` row (its
     label `wrong`, the undone frame's id) in one frame. Origin of the undo: the operator.
   - The RPC and the CLI are R3's: build only the function and its test.

**Store format.** The design plans no format bump: new fields are optional with serde defaults, and the undo data
rides in the ledger rows' existing payloads, which the store keeps as it does any row. Read AGENTS.md's version rule.
If adding `retired_ms`, `examples` and `mined_from` to the stored kind row still owes a bump in your reading, bump it
by one as the preamble says, add the old-layout sample, and say why in the report. Either way, an old store must
load with nothing dropped (a test below).

## Tests (each with a planted revert; the first is the key test)
- KEY: a `jev` member list for `topic` (whose `assigned_by` names jev) with a confidence is admitted by `put`, and so
  is a `jev`-minted topic category with `added_by = "jev:j-0001"` in the same records. Plant: restore `Origin::Jev`'s
  `comes_with` to `Some("M5, with Jev's categorize.v1")`. The test fails with `UnbuiltOrigin`.
- A `jev` list for `culture` (whose `assigned_by` does not name jev) is refused with `Writer`. So is a `jev`
  category of `culture`.
- Operator wins: the current topic list holds an operator entry. A `jev` list that drops it is refused, naming it. A
  `jev` list that keeps it and adds one is admitted. An operator list that drops a `jev` entry is admitted.
- A `jev` category with `added_by = "sweep:x"` is refused, as is a bare `"jev"`.
- Kinds: `sweep` adds a new interpreted kind (assigned_by naming sweep and jev): admitted. `sweep` changing `topic`,
  `jev` writing any kind, and `sweep` adding a given kind: each refused.
- Retire: retiring a kind with a live category is refused with `InUse`. Once its categories are retired, it
  succeeds. A retired kind refuses a new category. A seed kind's retire is refused.
- `examples`/`mined_from`: a row without them serialises byte for byte as before (compare with a literal of
  today's JSON), and six examples are refused.
- The new seeds pass `check_table`. `seed-kinds.json` matches. The compile goldens are unchanged.
- Bind: binding a guild channel with guild id `g-1` makes `guild:g-1` and a channel category whose parent is it.
  `given()` for that session yields both. A second identical bind writes no frame. A store from before this row
  (channel category with `parent: None`) gets the parent at the next bind.
- Undo: Jev writes a topic list over an earlier list and mints a category. Undo restores the earlier list byte for
  byte (compare the serialised records), retires the minted category, and writes one `ontology.undo` row. Undo of a
  mint whose category another session's list now uses is refused with `InUse`.
- Load: a fixture of today's stored ontology records (four seed rows, a guild channel and a DM category, an operator
  topic list, a person list from the import) loads with no record dropped and serves the same memberships.
- Replace, never delete: today's tests that assert `Jev`/`Sweep`/`Dream` are refused (tests.rs, near
  `(Origin::Jev, "M5")`, `UnbuiltOrigin` at about lines 166-184 and 845, and the `jev.origin = Origin::Jev` case
  near 1334) assert a rule this row deliberately changes. Rewrite each to assert the new rule in its place (built
  now, admitted where `assigned_by` names it, refused where it does not). Delete none. List each rewrite in the
  report with its before and after assertion, so the reviewer can see no guard was lost.

Put new tests in new files (`tests_origins.rs`, `tests_kinds.rs`, and a core `tests_ontology_bind.rs` or the
nearest existing place-test file), not in the long `tests.rs`.

## After the join (the maintainer's, not yours)
The maintainer runs the full gate, installs, and checks that a live re-bind fills the guild and the channel's
parent in the cockpit's Ontology view.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`;
  `cargo clippy -p theseus-ontology -p theseus-core -p theseus-protocol --all-targets -- -D warnings`.
- `cargo nextest run -p theseus-ontology` (all of it), `cargo nextest run -p theseus-protocol`, and
  `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(/ontology|bind|place|person|topic|layout|golden|people|output/)'`
  (scope runs to these crates: the VM is 4 cores with no sccache). If you touched protocol types, regenerate the
  TypeScript as AGENTS.md says and include it.
- `python3 scripts/keel-guard.py --base <your base> --head HEAD --quiet` empty.
- Not here: the full gate, the benches, cargo-deny. Say so in the report.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

