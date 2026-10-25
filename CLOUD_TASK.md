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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3f-trust-owner-words`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**This is a second attempt.** A first session was given this same task on 2026-10-24 on another branch and pushed
nothing in four hours. Nobody knows yet whether it died at launch or mid-run, so this run carries one extra rule: the
**early push** below. Work only on your own branch, named below. Never fetch, read or push the first attempt's branch.

Your branch starts from `main` at `b69236c1`. That is install #82's gated tree: release 1.0.2 (signed tag
`release-1.0.2` at `ac298666`) plus twelve commits of joined patch3 work. Fetch it and branch from `b69236c1`.

**What moved since release 1.0.2, so check line numbers by name:**
- `crates/theseus-core/src/tests_external.rs` grew by 14 lines near :130, from a new `core_tuned` helper beside
  `core_mode` and a new `mod split;`. Every test cited below in that file sits about 14 lines lower.
- `tests_external/notice.rs` gained about 110 lines of tests, and `tests_external/split.rs` is new. Keep both green.
- `rpc/confirms.rs` gained 6 lines, `tests_data_mark.rs` 21, and `crates/theseus-protocol/src/lib.rs` is now 2,611 of
  2,733.
- `external.rs`, `rpc/task_start.rs`, `task.rs`, `turn.rs`, `toolrun.rs`, `approval.rs`, `watch/` and the Discord
  `runtime/` are unchanged, so their line numbers below hold.

**Early push (required, before any code).** Do setup first (`cargo --version`, nextest) and your first
`cargo build -p theseus-core`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-nfhaf: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3f-trust-owner-words`

After that, push after EVERY commit, not only at the end. A run that ends early must leave its work on the branch.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
Aim for NO store change: the existing `ExternalText` fields can carry what this row needs (see Part 3). If you find a
new stored field is truly needed, bump to 52 the way `query` was added (`crates/theseus-core/src/tests_layouts.rs`
~:261 and `crates/theseus-core/AGENTS.md`'s store-format rule), and name the bump in the report's first section: the
maintainer renumbers it at the join.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At the
base: **`crates/theseus-core/src/toolrun.rs` is AT 2,500: add no line to it** (put any new code elsewhere);
`crates/theseus-core/src/turn.rs` 3,498 of 3,525; `crates/theseus/src/render.rs` 3,092 of 3,128;
`crates/theseus-protocol/src/lib.rs` 2,611 of 2,733; `crates/theseus-core/src/external.rs` 1,183;
`crates/theseus-core/src/rpc/task_start.rs` 735; `crates/theseus-core/src/task.rs` 1,442.

**Your lane: `integ/p3-trust`** (this row alone). Other lanes of this wave work in the cockpit (durations, a busy
indicator, the Context chart), the ontology code (`judge/categorize.rs`, `rpc/proposals.rs`, people exclusions) and
the voice code; the wave-1 lane works in recall, the catalog and the index. Stay out of all of them. Do not edit
`docs/status.md` (the integrator writes it at the join).

One row in another lane touches a file of yours: `p3e-fanout-kernel` (theseus-z9mze.1). It changes 2 lines in
`rpc/task_start.rs` near :148-151, where `wake_parent: false,` becomes `..Default::default()` in a kernel struct
literal, and adds 1 line to `task.rs`. Leave those lines alone. Your work is at `hold_of` (~:542-563), far from it.

## Your task: p3f-trust-owner-words, the owner's own words never mark a session as holding outside text (theseus-nfhaf, P1)

Branch: `cloud/20261025-p3f-trust-owner-words`.

Every commit subject carries `theseus-nfhaf`. Deadline for the report: about 3 hours after you start.

**What the owner said (2026-10-24):** "I'm not clear on why 'how is it going' got a trust flag -- my own DM or
channel text should never cause a flag and I can't imagine that triggered a tool."

**What happened (the maintainer's diagnosis from the owner's `session.list`):** none of his words marked that DM
session directly. Earlier in it, he had started a task whose brief carried a quote block (tool `task.start`, via
`quote`). `task.start` marked the CHILD task's session as holding outside text because the brief had quotes; when
the child reported back, the report carried the mark into the DM session (`from_session` = the child). The same
chain marked a voice session (from a "summarize a quoted log" dry run) and then a self-analysis task (from the voice
session). So the owner's own pasted quote, two hops later, flagged his conversation.

**The rule to build** (the owner's statement plus the project's default-trust principle,
`docs/spec/02-part1-s0.md` ~:88; §3.9 "External text" is `docs/spec/03-part1-s3.md` ~:280):
1. Text the owner wrote or pasted himself, in a private place (DM, the cockpit, the CLI, the TUI) or a trusted
   channel, NEVER marks a session, quote blocks included.
2. Only content a TOOL fetched from outside marks: web, mail, a file read from outside the workspace, a program's
   egress, an MCP prompt, another person's message. These marks stay exactly as they are today.
3. Inheritance from a child task stays (a report that summarises fetched outside text can carry an injection), and
   now it only ever carries a mark that came from (2), because (1) stops owner quotes from marking at the source.
4. A mark inherited from a child names its source plainly: "from task X, which read <tool> <url>", not "quoted text".

### What the code does today (confirm each; line numbers at b69236c1)

- **One writer:** `external::hold()` (`crates/theseus-core/src/external.rs` :510-532) writes the session record's
  `external` field (`SessionRecord.external`, `session.rs` :59; the type is `ExternalText` in
  `crates/theseus-protocol/src/lib.rs` :1255: `since_ms, tool, url, node_id, from_session, via, query`; `via`
  constants `external.rs` :109-121) and a `session.external_read` ledger row.
- **The owner-quote mark:** `hold_of` in `crates/theseus-core/src/rpc/task_start.rs` :542-563: if the parent has no
  mark and the start has any quotes, it builds `read(arr_id, "task.start", "quoted text")` with `via = "quote"`. It
  never looks at who started the task. Written at ~:382; called at ~:132.
- **Who started it is known but unused:** `conn.answerer(p.author, p.discord)` (~:57); `Answerer` (`approval.rs`
  :138-149: label, surface, discord, agent); `via()` (`approval.rs` :169-175: `cli`, `web`, `discord:dm`,
  `discord:<channel>`); authority `judge_act(Act::Task)` (~:98) resolves to `owner_in_private`
  (`rpc/confirms.rs` ~:772). Each quote becomes a piece with `origin: Operator, trusted: false, author = who.label`
  (`piece_of`, ~:648-660).
- **Callers of task.start with quotes:** the Discord "Make this a task" modal (`crates/theseus-discord/src/runtime/
  start_task.rs` :261-285; it does NOT pass the quoted message's own author); the CLI `--quote-file`
  (`crates/theseus/src/task_start.rs` ~:218, read client-side); the TUI (`new_task.rs`, `chat/task_new.rs`); and
  **the mail watch** (`crates/theseus-core/src/watch/mail_then.rs` :108-135), which passes an email's text as a quote
  with `Answerer { label: "mail watch", surface: Cli }`.
- **Inheritance:** `turn.rs` :2276-2290 marks the parent from a child's report with `taken(h, task, VIA_REPORT,
  report_node)`; `taken` (`external.rs` :285-301) copies the child's `tool`, `url`, `query`, keeps only the immediate
  `from_session`, and overwrites `via` with `task.report`.
- **Wording:** `source_said` (`external.rs` :350-363), `why_said` (:377-390), `narrated` (:709-720); the CLI health
  line `crates/theseus/src/render/external.rs` :51 (via suffixes :31-35); the Discord card
  `crates/theseus-discord/src/render/approval.rs` :57-60 maps `"task.start" => "quoted outside text"`.
- `MIN_QUOTE_CHARS = 4` (`arrangement.rs` :51) only refuses too-short quotes in `task.create`; it marks nothing.

### Part 1: owner-authored quotes do not mark (rule 1), outside quotes still do (rule 2)

- In `hold_of`, a start's quotes mark the child ONLY when they did not come from the owner's own hand. The owner's
  own hand = the start is the owner's (the owner answered it: `owner_in_private`, or the owner in a trusted channel)
  AND it came from a person, not an agent or a watch. Decide this from STRUCTURE, never from a label string:
  - **The mail watch MUST keep marking.** Its text is an outside person's email. Its `Answerer` claims `surface: Cli`,
    so "surface is CLI" cannot be the test. Give the internal call an explicit structural signal (for example the
    watch passes its quotes as outside, or `Answerer` gets a typed origin the watch sets), and test it.
  - An agent-started task (`Answerer.agent` set) whose quotes the agent copied from a tool result: the agent's
    session is already marked if it read outside text, and the parent-mark branch of `hold_of` (the parent's mark
    passes to the task) covers it. Keep that branch exactly as it is.
  - **Discord "Make this a task" on another person's message** (a guild channel message by someone else) MUST mark:
    the quote is that person's text. Today the modal does not pass the quoted message's author. Pass it (the
    author's id, or a flag "quoted message not by the owner") so the daemon can tell. On the owner's own message, or
    in his DM, it does not mark. If passing it would change any message of the locked DM task flow (see Rules), STOP
    that part and write an OWNER CALL; the daemon side alone still fixes the CLI and cockpit cases.
  - The CLI `--quote-file` from the owner does not mark (it is the owner handing text in). Name the residual risk in
    the report (the owner pastes an outside email into a brief): the existing `/trust` posture and the parent-mark
    rule are what remain. If an explicit opt-in to mark ("treat this quote as outside text") is cheap (an optional
    param, default off), add it; otherwise list it as a follow-up for the owner.
- Do NOT change any other marking path (tool results, egress, MCP prompt, job, `task.create` from a marked parent,
  retry, draft step): their behaviour is rule 2.

### Part 2: inheritance carries only outside-content marks (rule 3)

- With Part 1, a child task is never marked by an owner quote, so its report cannot carry one. Check there is no
  other way an owner-authored text can reach `r.external` (`task.rs` :1022-1024, loaded ~:1256) and say so.
- **Existing marks on the owner's store** made under the old rule stay (records are history: never rewrite them
  in code). List in the report how a maintainer could find them (sessions whose mark has `tool = "task.start"` and
  `url = "quoted text"`, directly or via `task.report`) so the owner can clear them with `/trust`; do NOT build an
  automatic clearing.

### Part 3: an inherited mark names its source plainly (rule 4)

- Today a parent marked from a report reads like "(http.fetch <url>, in task X's report, at …)" (see the test
  `a_report_from_a_task_that_read_external_text_makes_its_parent_hold_it`, `tests_external.rs` ~:691). Make every
  surface that describes an inherited mark name the chain in plain words: which task's report brought it, and what
  that task (or its first source) read: tool and url. Surfaces: `source_said`, `why_said`, `narrated`, the CLI
  health line and `render/external.rs`, `rpc/explain.rs` ~:468, `rpc/trust.rs` :90, and the Discord card's `what`
  (`render/approval.rs` :57-60).
- `taken` overwrites `via` with `task.report`, and keeps `tool`/`url` of the first source: so the first source's tool
  and url already survive any number of hops, and the immediate child is `from_session`. That is enough for "from
  task X, which read <tool> <url>" with NO store change. If you want the intermediate hops too, that needs a new field
  (a format bump): do not do it in this row; list it as a follow-up.
- Never word an inherited mark as "quoted outside text" unless its first source really was a non-owner quote (mail
  watch, another person's message): then say whose ("an email the mail watch quoted", "a message by another person").

## Tests (red first; plants)

New tests, each red at the base:
1. **An owner brief with a quote does not mark.** A task started by the owner (CLI, cockpit/web and Discord DM
   variants where the test harness has them) with a quote block: the task's session has no `external`; no
   `session.external_read` row; and after the task reports, its parent DM session has no mark either (the exact
   chain from the bug). Plant P1: restore `hold_of`'s quotes-always-mark line: red.
2. **Outside quotes still mark.** The mail watch's start with an email quote marks the task, `via = quote`, and its
   report marks the parent. A Discord "Make this a task" on another person's message marks (if Part 1's Discord half
   is not an OWNER CALL). Plant P2: make the owner test also cover the mail watch (treat its start as the owner's):
   red.
3. **A child that fetched a web page marks its parent, and the notice names the chain.** A child task that ran an
   `http.fetch` of a page on the invented host `docs.example.test` (path `/page`, https) reports; the parent is
   marked; every surface you changed in Part 3 names the task, the tool and that url. Plant P3: drop the task name
   from `source_said`:
   red.

**Tests that encode the OLD rule (owner quotes mark).** Read each; these are the only existing tests this row may
change, and only where they assert that an OWNER-started quote marks:
- `crates/theseus-core/src/tests_task_start.rs` ~:287 `quotes_hold_the_tasks_session_as_external_text` (a CLI start
  with quotes leaves the task marked `via = quote`).
- `crates/theseusd/tests/task_start.rs` ~:153 `a_quote_file_holds_the_tasks_session_as_external_text` (after
  `--quote-file`, `session.list`'s `external_text.tool` is `task.start`).
- `crates/theseus-discord/src/tests_board/commands.rs` ~:800
  `make_this_a_task_starts_one_whose_quote_holds_the_session_as_outside_text`.
For each: the owner's yes for this exact change is the quote above ("my own DM or channel text should never cause a
flag", issue notes, 2026-10-24 17:16). Do NOT delete or weaken coverage: keep each test's name only if it still says
what it proves; otherwise keep the old test in place but turn its setup into a NON-owner quote (the mail watch, or
another person's message) so it still proves a quote marks, and add the owner case beside it as a new test that
asserts no mark. List every such edit in the report with the before/after assertion. Run `keel-guard.py` (below):
if it flags these, paste its flag beside the owner's quote so the maintainer can add the signed `Keel:` ack at the
join; do not try to work around the guard.

**LOCKED, do not edit:** `crates/theseus-discord/src/render/approval/tests_approval.rs` ~:44
`the_approval_card_reads_as_the_owner_approved_it` and its golden ("change this golden only with the owner's OK"). Its
fixture is a mark with `tool: "task.start", url: "quoted text"` (:31-33) that expects "why it asks: the task quoted
outside text". Such a mark can still exist (an old record, or a non-owner quote), so the card's wording for it may
stay. If your Part 3 change would move that golden, STOP that part and write `OWNER CALL: <the old and new card
line>`.

Keep green: every other test in `tests_external.rs` (:254, :359, :402, :434, :458, :539, :590, :691, :780, :868, :931,
:970, :1001), `tests_external/once.rs`, `tests_external/notice.rs`, `external.rs`'s unit tests (:911 `taken` keeps the
first source, :944, :996, :1110, :1162), `tests_task_start.rs` :332 `a_parent_holding_external_text_passes_it_to_the_
task`, `tests_outside_text.rs`'s property tests, and the CLI goldens under `crates/theseus/tests/golden/`
(`policy_list.txt`, `policy_trust.txt`, `policy_trust_unknown.txt`, `policy_explain_tool.txt`,
`policy_explain_json.txt`, `health_json.txt`, `history_json.txt`, `answer_list_json.txt`): if your Part 3 wording
moves one of those CLI goldens, rewrite it with `THESEUS_GOLDEN=write` and commit only the lines your wording moves,
listing each.

## Not in this row

No change to how tool results, egress, MCP prompts, jobs, `task.create`, retry or draft steps mark. No change to the
`/trust` posture or what a mark gates (§3.9). No automatic clearing of existing marks. No store rewrite.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical (its line 8
  mentions `/trust` and web text). Run `git diff --exit-code b69236c1 -- crates/theseus-discord/src/golden/dm_flow.txt`
  and paste the result. A fix that would change any message of that flow (wording, layout, order, timing; the "Make
  this a task" modal's text included, if the flow shows it) STOPS that part: write `OWNER CALL: <what and why>`.
- **Keel:** remove or weaken no test (the three old-rule tests above are changed only as described); raise no budget,
  cap or ceiling; do not edit `scripts/keel-guard.py` or its tests. Before the report run
  `python3 scripts/keel-guard.py --base b69236c1 --head HEAD --quiet` and paste its output.
- **This is a trust rule: be conservative.** Wherever you are unsure whether a text came from the owner's own hand,
  it marks (as today). Say in the report exactly which inputs mark and which do not, as a table.
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Fixtures use invented content
  under `example.test` (urls, mail addresses, handles). Write "the owner".
- **Red first;** right over compatible.
- **The gate** (read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` "Tests" and "Traps", `crates/theseusd/
  AGENTS.md`, `crates/theseus-discord/AGENTS.md`): `cargo fmt --check`; `cargo clippy` on the touched crates with
  `-D warnings`; `cargo nextest run --workspace -E 'package(theseus-core)'` with `TZ=America/Phoenix`;
  `cargo nextest run -p theseus -p theseus-discord -p theseus-protocol`; `cargo nextest run -p theseusd` (its
  `tests/*.rs` run the real binary; `tests/agent_answers.rs` needs a pty; the known L1/sandbox failures on this VM
  are not yours); then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51 untouched, or the bump), ceilings (toolrun.rs untouched),
every OWNER CALL and STOP. (2) **The rule as built:** a table of inputs (owner DM, owner cockpit, owner CLI incl.
`--quote-file`, owner TUI, owner in a trusted channel, agent-started task, mail watch, Discord "Make this a task" on
another person's message, tool results: web/mail/outside file/egress/MCP prompt/job, child report) -> marks or not,
and the structural signal used for each. (3) **The chain wording:** before/after text on every surface, with an
example for the bug's chain. (4) **Old-rule tests:** each edit, before/after assertion, the owner quote. (5) **Tests**
1-3 and plants P1-P3, red at the base and green after. (6) **Existing marks:** how to find the ones made by owner
quotes, for the owner's `/trust`. (7) **Residual risk and the opt-in** (built or follow-up). (8) **Gate** commands
and results. (9) **Keel** output. (10) **DM golden** diff result, and the approval-card golden untouched. (11) **Join
notes:** every file touched. (12) **Follow-ups** (intermediate hops, the docs line for §3.9 stating the owner-words
rule if you did not add it).
