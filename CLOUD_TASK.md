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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2k-types`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `3bac00ab` (release 1.0.1 plus the patch2 joins so far; the owner's installed
build). Line numbers below are at `3bac00ab` and may be off by a few: search by name. Where the release preamble above
disagrees with this note about the base, ceilings or the work in flight, this note wins.

**Store format at your base: 50** (`const MANIFEST_FORMAT: u32 = 50;` in `crates/theseus-store/src/store.rs`, near
:685). **This row bumps it to 51, and it is the only row in its lane that may.** Name the bump in your report's first
section.

**Line ceilings.** `crates/theseus-protocol/src/lib.rs` is in `scripts/long-files.txt` at 2,733 and is 2,584 at the
base: add method lines only, no types (types go in `card.rs`). Every other file this row touches caps at 2,500 and is
far under. Never raise a ceiling.

**Your lane:** `integ/p2-learn`, new. This row goes FIRST and alone. Three rows start from your pushed tip, in parallel,
when you report: `p2k-obs` (the exchange-end observer that writes `card.obs.` records), `p2k-live` (the card's nightly
live pass and its Add / Pattern / Stale questions) and `p2k-dm` (live card questions in the owner's Discord DM). They
build on the names you define, so get the names and shapes right and document each one; they must not need a second
format bump or a second protocol change.

## Your task: p2k-types, the shared wire and format pieces of live learning (theseus-6f8t0)

Branch: `cloud/20261024-p2k-types`. Every commit subject carries `theseus-6f8t0`.

Theseus learns about the owner today only from the imported archive (the card job) and from explicit directives
(capture). Live learning adds: an observer that, when a private owner-alone conversation pauses, asks Haiku for what
the owner said about the owner's own setup, preferences and decisions that should hold (stored privately as unconfirmed `card.obs.<id>` META records, never a
node, never indexed, never in a prompt), and a nightly card pass that turns those into owner questions: **Add**,
**Pattern** (the same preference or decision in 3 or more conversations) and **Stale** (an observation contradicts a
live card record: Keep / Edit / End it). The owner answers; nothing becomes a standing record without the owner's answer.

**This row adds the types, the format bump, config, the ledger kind and one read. It adds no behaviour**: no observer,
no live pass, no question, no Discord. The design is the brief's source; you do not need it beyond this file.

## Read first
1. The repo's root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (the card section and the store-format notes) and
   `crates/theseus-store/AGENTS.md`.
2. `crates/theseus-protocol/src/card.rs` in full: `CardProposal` (~:64), `CardAnswerParams` (~:209), `CARD_OPTIONS`,
   `CARD_PROPOSAL_PREFIX`, the ts-rs attributes. `crates/theseus-protocol/src/lib.rs`: the method list (~:280,
   `CARD_PROPOSE` ... `CARD_DONE`). `crates/theseus-protocol/src/ledger.rs`: `CardProposed`, `PeopleExtracted`.
   `crates/theseus-protocol/src/ts.rs`: how the TS bindings are exported and checked.
3. `crates/theseus-core/src/config/card.rs` (89 lines: `CardConfig`, defaults, `validate`) and the `[card]` block of
   `crates/theseus-core/config/theseus.example.toml`.
4. `crates/theseus-core/src/day_ceiling.rs` (~:44 doc, ~:73 the spend kinds list with `people.extracted`,
   `card.proposed`); check whether `crates/theseus-kernel/src/day_ceiling.rs` keeps a list too.
5. `crates/theseus-store/src/store.rs` ~:600-690 (the format history lines) and how the last bump (50, commit
   `3972934e`, theseus-utfnl) moved tests: `crates/theseus-core/src/store.rs`'s manifest test and
   `crates/theseus-core/src/tests_layouts.rs` (the old-layout samples and their `assert_eq!(moved["format"], 50)`).
6. `crates/theseus-core/src/rpc/card.rs` (the dispatch, `judge_card`), `crates/theseus/src/card_cmd.rs` (the verbs),
   `crates/theseus/src/client.rs` (~:372 the method -> CLI table; ~:564 the job-process refusal test).

## What to add

### 1. Protocol (`crates/theseus-protocol/src/card.rs`; method lines only in `lib.rs`)
- `CARD_OBS_PREFIX = "card.obs."` (an observation's META key: `card.obs.<id>`) and `CARD_OBSERVE_MARK_PREFIX =
  "card.observe."` (the observer's per-session mark: `card.observe.<session>`), each with a doc line.
- `ObservationKind`: `fact | preference | decision | correction` (snake_case serde). There is no trait or behaviour
  kind, by construction; say so in the doc.
- `ObservationState`: `open | used | dropped` (default `open`).
- `CardObservation` (the stored record): `id` (`cob_` and a short unique id, like `cdp_`), `session`, `nodes`
  (`Vec<String>`, the owner's message nodes it cites, at least one), `kind`, `text` (one short phrase about the owner,
  at most `CARD_OBS_MAX_TEXT = 160` characters), `at_ms`, `contradicts: Option<String>` (a standing record id), `state`,
  `proposal: Option<String>` (the proposal that used or declined it). Serde defaults on every optional field;
  `skip_serializing_if` on the options, as `CardProposal` does.
- `CardObservedParams` (`state: Option<ObservationState>`, default open; `limit: Option<u32>`; `author`/`discord` as
  the other card params carry them) and `CardObservedResult` (`observations: Vec<CardObservation>`, newest first;
  `open: u32`, `used: u32`, `dropped: u32` counts).
- `CardAsk`: `add | pattern | stale`, `#[serde(default)]` = `add`. `CardProposal` gains `ask: CardAsk` (serde default,
  skipped when `add` so an Add proposal's stored bytes are unchanged), `record: Option<String>` (the standing record a
  Stale question is about), `observations: Vec<String>` (the `cob_` ids it rests on; default empty, skipped when
  empty), and `posted_ms: Option<u64>` (when the question was posted to the owner's DM; `p2k-dm` writes it so it never
  posts twice and counts its day). Doc each field with the row that writes it.
- `CARD_STALE_OPTIONS: [&str; 3] = ["Keep", "Edit", "End it"]` beside `CARD_OPTIONS`. `CardAnswerParams.option`'s doc
  gains `end` (End it: on a Stale question, revoke the record). Parsing `end` is p2k-live's behaviour; you only
  document the wire.
- Method `CARD_OBSERVED = "card.observed"` in `lib.rs`'s method list.
- Every new type derives what its neighbours derive (ts-rs under `cfg(test)`, `ts(type = "number")` on ms fields).

### 2. Ledger kind (`crates/theseus-protocol/src/ledger.rs`)
`CardObserved = "card.observed"`: one row per observer call, detail = cost, tokens in/out, model, items returned,
items kept, restated, session. Define the detail struct if the ledger keeps typed details (follow `PeopleExtracted`).

### 3. Bindings
Export the new types in `ts.rs` and regenerate the TS bindings the way the repo does (read `ts.rs`'s doc and the
cockpit's `protocol.ts`); commit the generated files; the bindings check passes.

### 4. Store format 50 -> 51 (`crates/theseus-store/src/store.rs`)
- `const MANIFEST_FORMAT: u32 = 51;` with its history line: `/// 51 = live learning (theseus-6f8t0): the META records
  card.obs.<id> (an owner observation) and card.observe.<session> (the observer's mark), a card proposal's ask,
  record, observations and posted_ms, and the ledger kind card.observed.` (wrap like its neighbours).
- Move every test that pins the format (`crates/theseus-core/src/store.rs`'s manifest test, `tests_layouts.rs`'s
  `moved["format"]` lines, any other `grep -rn '"format"\], 50\|format.*50' crates`) to 51.
- In `tests_layouts.rs`, add a sample of the format-50 layout this replaces where one is on disk: a `card.proposal.`
  META record as format 50 wrote it (literal bytes, no `ask`), proving it reads as `ask = add` and round-trips byte for
  byte. Follow the file's rules (literal old bytes, never today's encoder's).
- No other golden should move. If one does, say which and why in the report.

### 5. Config (`crates/theseus-core/src/config/card.rs`, `theseus.example.toml`)
- `observe: bool` (default `true`): the exchange-end observer and the nightly live pass. Off also when `day_usd = 0`.
- `live_per_night: u32` (default 5, validated 0 to 50): the most live questions one nightly pass proposes.
- `stale_per_night: u32` (default 2, validated 0 to `live_per_night`): of those, the most Stale questions.
- `pattern_sessions: u32` (default 3, validated 2 to 20): distinct conversations a Pattern needs.
- `dm_per_day: u32` (default 3, validated 0 to 10): the most live questions posted to the owner's DM a day (0: CLI
  only).
- Module doc lines for each, in the file's style; `is_default` and `Default` updated; the example toml's `[card]` block
  gains each line with a one-line comment. These are the owner's chosen defaults (pattern at 3 conversations, at most
  5 live questions a night with at most 2 Stale, at most 3 DM questions a day): do not change them.

### 6. Spend (`day_ceiling.rs`)
`card.observed` joins the spend kinds beside `card.proposed` (and in the kernel's list if it keeps one), with the doc
line.

### 7. The one read: `card.observed`
- `crates/theseus-core/src/rpc/card.rs`: a `method::CARD_OBSERVED` arm and `card_observed`, which lists `card.obs.`
  META records filtered by state, newest first, with the counts. It is a read: it goes through the same answerer check
  `card.list` uses and is allowed in a job's process, like `card.list`. Put the reading of `card.obs.` records in one
  function in `crates/theseus-core/src/card/state.rs` (or a new `card/obs.rs` if `state.rs` is the wrong home) that
  `p2k-obs` and `p2k-live` will call: `observations(store, state_filter) -> Vec<CardObservation>`. One helper, not
  two.
- `crates/theseus/src/client.rs`: the table line `(method::CARD_OBSERVED, "theseus card observed")`.
- `crates/theseus/src/card_cmd.rs`: the `observed` verb (`theseus card observed [--all|--used|--dropped] [--json]`):
  one line per observation, `m:` handles of its nodes, kind, text, age; an empty list prints one line saying so and
  that `[card] observe` is on or off.
- This read is yours so the parallel rows need not share `card.observed`'s handler. `p2k-live` adds the `end` answer
  in the same files (a different function); `p2k-obs` does not touch them.

## Tests (red first: each red at `3bac00ab`, green after; say how you showed red)
1. An old `CardProposal` JSON (format 50 bytes, no `ask`/`record`/`observations`/`posted_ms`) decodes as `ask = add`
   and re-encodes to the same bytes.
2. A `CardObservation` round-trips; an unknown `kind` (`"trait"`) fails to decode.
3. `live_per_night = 51` fails validation; `stale_per_night` above `live_per_night` fails; `pattern_sessions = 1`
   fails; `dm_per_day = 11` fails; the defaults are 5 / 2 / 3 / 3 and `observe = true`.
4. The manifest-format test at 51; the new layout sample.
5. `card.observed` on a store holding three `card.obs.` records (one each open, used, dropped, written directly) lists
   the open one by default and each with its flag; a job's process may call it (extend the client.rs refusal test's
   read list).
6. `card.observed` is a spend kind (the day-ceiling test that lists kinds).

## Out of scope (other rows; do not start them)
The observer (p2k-obs), the live pass, question wording, `end`'s behaviour (p2k-live), anything in
`crates/theseus-discord` (p2k-dm), any prompt, recall, index or census change.

## Rules for this row
- **No behaviour:** after this row a daemon behaves exactly as at the base: no new model call, no new record written,
  no new message. The census goldens and every compiled request are byte-identical.
- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
  (`git diff --exit-code 3bac00ab -- crates/theseus-discord/src/golden/dm_flow.txt`).
- **Keel:** no removed or weakened test, no raised budget/cap/ceiling, no edit of `scripts/keel-guard.py` or its tests,
  no renamed test. Moving a format number in a test is the bump, not a weakening; say so. Run `python3
  scripts/keel-guard.py --base 3bac00ab --head HEAD --quiet` before the report and paste its output.
- **Data:** nothing leaves the host. Never a China-origin model anywhere.
- **Public repo:** no person's name, real organisation, channel id, address or email anywhere, fixtures included;
  invented names under `example.test`; write "the owner".
- Right over compatible; one helper, not two; red-first; real behaviour, never a test special case.
- **Gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p
  theseus-protocol`, `cargo test -p theseus-store`, `cargo test -p theseus-core card`, `cargo test -p theseus-core
  layouts`, `cargo test -p theseus-core config`, `cargo test -p theseus-core day_ceiling`, `cargo test -p theseus
  card`, `cargo test -p theseus client`, the census golden test, the TS bindings check, `scripts/shape.sh`. If a cold
  build is too slow for the whole workspace, run the crates you touched and say what you skipped.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it is
not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections:
1. **For the maintainer:** store format 50 -> 51 (the history line); every protocol type and field added, with its
   exact Rust name and serde name (the next three rows read this section to build on you); the config keys and
   defaults; the ledger kind; the helper's path and signature; any golden that moved.
2. **Tests:** each, red-first or not, and how red was shown.
3. **Gate:** each command and its result. **Keel:** the output. **DM golden:** the diff result.
4. **Join notes for `integ/p2-learn`:** every file touched; anything the parallel rows must know.
