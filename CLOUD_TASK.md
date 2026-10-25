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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3b-onto-never`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
Aim for NO bump (see Part 1: an existing META key/value record is the first choice). If a new stored record kind is
truly needed, bump to 52 the way 49-51 did (an entry in `crates/theseus-core/src/tests_layouts.rs`, the rule in
`crates/theseus-core/AGENTS.md` ~:3418-3421) and name it in the report's first section: it is renumbered at the join.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every Rust file not listed there caps at 2,500 lines).
At the base: `crates/theseus-core/src/config.rs` 2,107; `config/ontology.rs` 153; `config/people.rs` 107;
`judge/categorize.rs` 613; `judge/people/mod.rs` 443; `rpc/proposals.rs` 838; `rpc/people.rs` 456;
`rpc/ontology.rs` 626; `tests_categorize.rs` 1,171; `crates/theseus-protocol/src/lib.rs` 2,606 of 2,733 (put new wire
types in `crates/theseus-protocol/src/ontology.rs`, 635); `crates/theseus-core/config/theseus.example.toml` 1,848;
`cockpit/src/components/People.tsx` 194.

**Your lane: `integ/p3-onto`** (this row alone). **ANOTHER LANE edits the same widget: read this.** `p3b-busy-wheel`
(theseus-6o8q8, lane `integ/p3-cockpit`) changes the Ontology proposals panel (`cockpit/src/components/People.tsx`
`ProposalsPanel` :123-193) and `components/OntologyParts.tsx`: it adds the busy lock to the two checkbox `disabled=`
attributes (:169 and :183), one progress element right after `<Refused words={refused} />` (:163), a ship's-wheel in
`Act` (`OntologyParts.tsx` :59-65), a `progress` field on `useOntologyWrite` (:23-49), and possibly a batching change
inside `answer()` (:134-149). To keep the two merges clean:
- Put your row controls in a NEW component file, `cockpit/src/components/ProposalActions.tsx` (Reject and Never for
  one row; and the bulk Reject/Never), and change `People.tsx` only by: inserting ONE new cell per topic row AFTER the
  confidence cell (~:185) holding `<ProposalActions …/>`; adding a Never beside the person rows' existing ✓/✗
  (:175-178); placing your bulk buttons immediately AFTER the header's `Accept` `<Act>` (:161). Do not touch the
  checkbox lines, the `<Refused>` line, or `OntologyParts.tsx`.
- Every new button reads `busy` from `useOntologyWrite` and is `disabled={disabled || busy}` like the existing ✓/✗.
- `answer()` (:141) always sends the picked TOPICS to `accept_all`, even for a reject: fix it in ONE line (the topics
  call picks `accept_all` or `reject_all` by `accept`). The other lane may also touch `answer()`: name your exact line
  in the report.
Other lanes of this wave work in the cockpit's time formatter and Context chart, the voice code and the trust marks;
the wave-1 lane works in recall, the catalog and the index. Stay out of all of them. Do not edit `docs/status.md`.

## Your task: p3b-onto-never, a durable "never propose this", and categorize.v1 skips small talk (theseus-648f8)

Branch: `cloud/20261024-p3b-onto-never`.

Every commit subject carries `theseus-648f8`. Deadline for the report: about 3 hours after you start.

**What the owner saw (2026-10-24):** the cockpit's proposals panel showed 937 proposals (255 people, 191 hidden). The
maintainer's look via `ontology.proposals`: (a) many rows are about sessions whose first line is small talk or a
status check ("How is it going?") or are agent/subagent/bench sessions (first lines like "[Subagent Context] You are
running as a subagent" or "Operator task (planning only…"); (b) "a new topic" rows carry no name; (c) a person
proposal named `owner` slipped past the exclusions; (d) topic rows have no reject or never control. The owner wants a
durable "never propose this" (like `[people] not_people`) and no proposals for small talk.

### What the code does today (confirm each)

- `[people] not_people`: `crates/theseus-core/src/config/people.rs` :34 (`PeopleConfig`, `deny_unknown_fields`,
  default :70, doc :8; mounted `config.rs` :243); template `theseus.example.toml` :1373-1383; applied in `NotPeople::of`
  (`judge/people/mod.rs` :174-182), combined in `Core::not_people` (`judge/people/house.rs` :102), used by
  `rpc/proposals.rs` :293, `rpc/people.rs` :275, `rpc/import.rs` :412, `rpc/import_sync.rs` :93,
  `judge/people/run.rs` :181, `judge/people/live.rs` :170; tests `judge/people/tests.rs` :103, `tests_excl.rs` :239.
- **The config is the operator's: nothing in the daemon writes it** (root `AGENTS.md` ~:215; `config/judge.rs`
  :94-95, "his config note, which agents can't write (§2.7)"). It is read at start; a changed config source restarts in
  place (`config_gate.rs` :1-18). So a cockpit button must NOT write the config file.
- `[ontology]`: `OntologyConfig`, `config/ontology.rs` :46-68 (fields `judge, act, confirm, name_profile, request,
  recall_boost, private_mark, sweep, sweep_usd_per_day, dream_guidance`; `validate()` :129; mounted `config.rs`
  :251); template `theseus.example.toml` ~:1425; tests `config/tests_ontology.rs` (defaults :11, bad keys :47,
  template :88) and `config/tests.rs` :177 `example_template_uncommented_still_parses`.
- categorize.v1: pack `crates/theseus-judge/packs/categorize.v1.toml` (the `none` option :31: "no single subject, or
  are too few or too scattered to place"); runner `judge/mod.rs` :561 `after_turn` -> `judge/categorize.rs` :282
  `at_exchange_end` -> `prepare_categorize` :316 (returns early for a non-private place :319 and when `due()` :138
  finds no trigger :326; builds `input()` ~:356) -> `judge_categorize` :491. Existing skips: tasks, pack off/not
  sampled, shared places, the due rule (10 human messages or 30 minutes' quiet). **No small-talk filter.**
- A topic proposal is not a record: it is the answered judgment's ledger row, read by `Proposed::of`
  (`rpc/proposals.rs` :78; drops `none` at :90); listed at :288 and :321. Reject RPCs exist:
  `ontology.proposal.reject` (`rpc/ontology.rs` :399, `rpc/server.rs` :352, handler `rpc/proposals.rs` :543,
  `reject_open` :562) and `reject_all` (`rpc/ontology.rs` :429, handler `rpc/people.rs` :416; it takes any listed
  judgment ids, topics included). A `new_topic` proposal has no name (`crates/theseus-protocol/src/ontology.rs`
  :325-335).
- The `owner` gap: `NotPeople::of` (`judge/people/mod.rs` :176-180) and `with_owners` (:216) exclude the owner by
  handle; at :204 `else if a != "owner"` skips the author string without adding the word; `HOUSE` (:153) lacks it; so a
  candidate named `owner` passes `excludes_name` (:292).
- categorize.v1 has no backfill or import path (`learning/backfill.rs` handles loop.v1 only; `learning/rebuild.rs`
  :42-45 refuses Categorize); imported sessions get people proposals (`rpc/import.rs` :412, `import_sync.rs` :93).
  So find how topic proposals reached imported agent sessions (a sweep? `[ontology] sweep`; or imported sessions that
  later got live turns): say what you found.

### Part 1: a durable "never propose this"

1. **Config:** `[ontology] not_topics = [..]` on `OntologyConfig` (serde default empty, in `Default`, documented in
   the module doc and in `theseus.example.toml`'s `[ontology]` block in the `[people] not_people` style). Matching is by
   the proposed topic's name, case- and space-insensitive (reuse `not_people`'s normaliser if it has one).
2. **The Never button's record:** the cockpit cannot write the config. Find whether the store has a generic META
   key/value record that others already use for small durable owner choices (for example `external.told.<sid>` or
   `policy.notice_seen` from recent rows: `rg -n "META|meta_put|put_meta" crates/theseus-store/src crates/theseus-core/src`).
   If it does, store each never as one META key (for example `ontology.never.topic.<normalised name>`,
   `ontology.never.person.<normalised name>`, and for an unnamed new-topic row `ontology.never.session.<sid>`: never
   propose a new topic for that session), with who and when, and NO format bump. If it does not, add the smallest
   record kind and bump to 52 (named, as above).
3. **RPCs** (wire types in `crates/theseus-protocol/src/ontology.rs`, methods registered as the reject ones are):
   `ontology.proposal.never {judgments: [..]}` (marks each proposal's topic/person/session as never AND rejects those
   proposals, under the same confirm/act rule as `reject_all`), `ontology.never.list` and `ontology.never.undo {key}`
   (the cockpit rule: nothing is lost; a never can be undone). Owner-only, as `reject_all` is.
4. **Applied:** the effective never-set = config `not_topics` (topics) / `not_people` (people) UNION the stored nevers.
   `proposals_listed` hides matching proposals (count them into the existing `hidden` figure, so the panel's "N hidden
   by the exclusions" stays honest); `prepare_categorize` does not propose a never'd topic (if the judgment answers
   one, the proposal is dropped as `none` is, at `Proposed::of`); people never'd stay out of people proposals the way
   `not_people` names do (`Core::not_people`).
5. **Cockpit:** in `ProposalActions.tsx`: Reject and Never per topic row, Never beside each person row's ✓/✗, bulk
   Reject and Never for the picked rows; each confirmed first (`window.confirm` via `writeEach`, as today) with words
   that say what Never does ("never propose 'X' again; undo it under nevers"). An unnamed `new_topic` row reads `a new
   topic (unnamed)` and its Never says "never propose a new topic for this session". A small "nevers" list (with
   Undo) in the panel's footer or a collapsed section of the same component file.
6. **The `owner` word:** add `owner` (and `the owner`) to the people exclusions (`HOUSE` or explicitly), with a test in
   `judge/people/tests.rs` beside :103.

### Part 2: categorize.v1 skips small talk and status checks

- A deterministic pre-filter in `prepare_categorize` after `input()` (~:356): when every recent human message in the
  judged window is small talk or a status check, return `None` (no judgment, no spend). Small talk = short (a named
  constant, for example at most 6 words) AND matches a greeting/status phrase set (`how is it going`, `how's it going`,
  `status?`, `any update`, `what's up`, `hi`, `hello`, `thanks`, `ok`, `good morning` …; a named, commented list), or
  is only a greeting. A window with ANY substantive message is judged as today.
- **Make sure a skip does not re-trigger at once:** the skip returns before the judgment, so the due rule's mark does
  not move. Check `due()` (:138): if the next exchange end would trigger again immediately, move the mark on a skip
  (the way a judged exchange does) or record the skip; test it.
- **Agent/subagent/bench sessions:** if Part 1's investigation finds topic proposals come from imported agent
  transcripts (a sweep or similar), skip sessions whose import tag (or first line) marks them as agent, subagent or
  bench sessions; name the rule and where. If the path is not categorize.v1 at all, say so and leave it as a follow-up.
- **Do NOT edit `categorize.v1.toml`'s prompt** unless the judge crate's rules allow editing a pack in place (read
  `crates/theseus-judge/AGENTS.md` and how packs are versioned and promoted: `placed(PACK, ..)`, the ladder). A
  prompt line on the `none` option would also move the goldens `crates/theseus-judge/fixtures/golden/
  categorize.v1.request.json` and `categorize.state.json`. If in-place edits are not allowed, propose the line for a
  `categorize.v2` in the report instead.

## Tests (red first; plants)

1. Config: `not_topics` parses, defaults empty, appears in the template, the uncommented template still parses
   (`config/tests.rs` :177); a bad key still fails (`deny_unknown_fields`).
2. A topic in `not_topics` is never listed as a proposal and is counted in `hidden`; a stored never does the same; an
   undo brings it back. Plant P1: drop the never-set from `proposals_listed`: red.
3. `ontology.proposal.never` on a topic row, a person row and an unnamed new-topic row: each proposal is rejected,
   each never is stored, `ontology.never.list` shows them, and a later categorize judgment that answers the never'd
   topic yields no proposal.
4. Small talk: a session whose human messages are "How is it going?", "thanks", "ok" makes no categorize judgment (no
   Jev call: the test's stand-in counts calls) and does not re-trigger on the next exchange end; a session with one
   substantive message among them is judged. Plant P2: remove the pre-filter: red. Mirror the existing triggers in
   `tests_categorize.rs` (:117, :143, :496 `ten_human_messages_bring_one_judgment_and_its_proposal`).
5. `owner` as a candidate name is excluded. Plant P3: remove it from the exclusions: red.
6. Cockpit (node's own test runner over `cockpit/test/*.test.ts`; pure modules only, `.ts` imports; read
   `cockpit/AGENTS.md` ~:373-380): the pure part of `ProposalActions` (which call a Reject/Never sends for topic,
   person and unnamed rows; that a reject of topics sends `reject_all`) in a new test file.
Keep green: every `tests_categorize.rs` test (incl. :813 new-topic accept/reject, :939
`a_shared_place_and_a_task_are_never_judged`), `judge/people/tests.rs`, `tests_excl.rs`, `config/tests_ontology.rs`,
the categorize goldens (untouched), and the cockpit's tests.

## Not in this row

No one-time bulk reject of the owner's backlog (that is a later local step the owner approves; the bulk Reject/Never
buttons let him do it). No grouping of proposals by category (a follow-up). No config-file writes from the daemon or
the cockpit. No change to onto.v1's sweep beyond a skip rule, if Part 2 finds it is the path.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test; edit no golden. Before the report run `python3 scripts/keel-guard.py --base ac298666 --head
  HEAD --quiet` and paste its output.
- **Cockpit data rule:** nothing deleted; a never hides and can be undone; the hidden count stays honest.
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Fixtures use invented people
  and topics under `example.test`. Write "the owner".
- **Red first;** right over compatible.
- **The gate:** `cargo fmt --check`; `cargo clippy -p theseus-core -p theseus-protocol -p theseus-judge -D warnings`;
  `cargo nextest run --workspace -E 'package(theseus-core)'` with `TZ=America/Phoenix`; `cargo nextest run -p
  theseus-protocol -p theseus-judge`; in `cockpit/`: `npm ci`, `npm run lint`, `npm test`, `npm run build`; then
  `scripts/shape.sh`. If protocol types changed, regenerate the cockpit's protocol bindings the way the repo does
  (`cockpit/src/protocol.gen/`: read how) and commit them.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51 untouched, or the bump and why), ceilings, every OWNER
CALL and STOP. (2) **Config keys and defaults:** `[ontology] not_topics` (default, matching rule), the small-talk
word limit and phrase list (their names and defaults), and the template lines. (3) **The never record:** where it
lives, its keys, the RPCs (names, params, results), undo. (4) **Where the topic proposals on imported agent sessions
came from**, and what you did. (5) **Small talk:** the filter, where it runs, and the no-re-trigger proof.
(6) **The cockpit:** the controls, their words, and the exact `People.tsx` lines you changed (the cockpit lane's
busy-wheel row edits the same panel). (7) **Tests** and plants P1-P3. (8) **Gate** commands and results. (9) **Keel**
output. (10) **DM golden** diff result. (11) **Join notes:** every file touched. (12) **Follow-ups** (grouping by
category, a `categorize.v2` prompt line if you proposed one, the owner's one-time backlog reject).
