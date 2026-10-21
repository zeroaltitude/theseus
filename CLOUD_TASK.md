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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-gw-drive`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
file of their own, re-exported with one line. If you truly must pass a ceiling, raise it in `scripts/long-files.txt`
with the reason in the same commit, as its header says; never pass it silently (the gate's shape step fails).

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
## Your task: gw-drive, Google Workspace part 3 of 6: Drive as four typed tools, Docs as markdown both ways, sharing and trash always asked (theseus-batvg.3)

Branch: `cloud/20261021-gw-drive`. Every commit's subject carries `theseus-batvg.3`. Deadline for the report: 4 hours
after you start.

**Why.** The owner's files: find them, read them (a Doc as markdown, a Sheet's first rows, Slides' text), write them,
and share them only with his yes. **Source:** the design's §3 and §4.2, copied below.

**Risk: low to medium.** Writes are undoable (Drive keeps revisions); sharing and trash reach other people or remove
things, so they are authority.

**Prerequisite: G1 (`gw-core`, theseus-batvg.1) joined `main`.** Your base has `crates/theseus-google/` (the client,
the classifier, the fake), `theseus-core/src/google/` (`Google`, `Google::call`, `google::finish`, `google::tools`),
the scope map naming your four tools, the `docs` family and the template's `# gw-drive:` marker. Read G1's module docs
first. **Use them; never redefine them** (shared contracts 2 to 7). If your base has no `crates/theseus-google/`, stop
at once and report that: you were launched too early.

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| G3 | `gw-drive` | theseus-batvg.3 | `drive.find`, `drive.read`, `drive.write`, `drive.share` (the design's §3.1 rows 8-11): typed fields compiled to Drive's `q` (`text` is `fullText contains`); a Doc exported as `text/markdown`, a Sheet's tabs and first 200 rows as a table (or `range`), Slides' per-slide text with object ids and layouts, binaries and PDFs to `save_to` (required past 1 MB); markdown → Doc and CSV → Sheet by Drive's import conversion; move and rename; every permission change and every trash under **authority** (`Plan.authority`, at every posture); the owner-authored rule's metadata (`owners[].me`, `shared`) handed to `google::finish` | `src/google/drive.rs` (new), `crates/theseus-google/src/fake/drive.rs` (new) with its two lines in `fake.rs`, the template's `# gw-drive:` marker | M |

## Tests (each with a planted revert; the first is the key test)
- **`share_and_trash_always_ask_whatever_the_posture`** (posture `open`, an earned grant covering `drive.share`: a
  share and a trash still wait). Revert: make them a posture line.
- `markdown_round_trips_through_a_doc` (write markdown as a Doc in the fake, read it back: headings and lists kept).
- `a_shared_owned_file_is_external`, `an_owned_unshared_file_is_not_external`.
- `find_compiles_typed_fields_to_drive_q` (quotes and backslashes in a name are escaped). Revert: no escaping.
- `a_large_binary_needs_save_to`.
- Every drive tool is deferred and its census row is within ~220 tokens.

## The design's own words: §3. The tool surface

**Principles:**
- The fewest tools that cover the real jobs, typed where the harness adds value: MIME building, HTML to text,
  markdown to Doc conversion, threading headers, External marking, a send bound to a digest.
- One generic escape hatch, `google.call`, for the long tail, as `aws.call` is for AWS. The tool-tree measurement
   found Opus 5.5 writes `aws.call` from its own knowledge and never used the
  typed AWS helpers. Google's REST is just as well known, so the typed set stays small.
- **Every Google tool is deferred.** A deferred declaration costs a constant ~93 tokens total however many there are,
  so the first-request budgets (8,000 cli/task, 15,000 DM; `first_request.rs`) are untouched. `[tools.defer]
  load_share = 0.10` promotes a tool to always-loaded only once the ledger shows real use. `gmail.search` and
  `calendar.events` in the DM are the likely first.
- **Families** (`tools_defer/family.rs` `FAMILIES`, `of_wire()`):
  - `mail` (gmail.\*), `calendar` (calendar.\*), and `docs` (drive.\*, sheets.\*, slides.\*, google.call);
  - without an entry, the tools would land in `other`, which `hideable()` never hides.
- **Schema size:** each tool's description plus schema is aimed at ≤ 220 tokens, held by `tests_census.rs` rows. All
  15 loaded together come to about 3k tokens, which is exactly why they are deferred.
- **Names** follow the dotted internal / underscored wire convention (`wire_name`, `theseus-tools/src/lib.rs:434`).
  The template's routine-grant example `gog.gmail.search` becomes `gmail.search` (right over compatible).

### 3.1 The tools

The postures below are template defaults in `[policy.tools]`. The template must list every tool
(`example_template_lists_every_tool_under_policy_tools`). "inherit" means the tool takes `[policy] enforcement`, which
is `notify` on the owner's machine. **Ext** means the result is marked External (§4.2). **Authority** means
`Plan.authority` forces approve whatever the config says.

| # | Tool | Does | Class | Posture | Ext |
|---|---|---|---|---|---|
| 1 | `gmail.search` | `{q, max≤50=20, page?}`. Gmail query syntax. One row per thread: id, from, subject, date, snippet, labels, unread, count. | Read | inherit | yes, unless every sender is the account |
| 2 | `gmail.read` | `{id, save_attachments?: dir}`. A thread or a message: headers, plain-text bodies (HTML → text, quoted tails folded), attachments listed (name, type, size). Saving them writes under the workspace for the file and PDF tools. | Read | inherit | yes, unless every message is from the account |
| 3 | `gmail.draft` | `{to[], cc?, bcc?, subject, body, reply_to?: message id, draft_id?}`. Creates or replaces a draft. `reply_to` sets threadId, In-Reply-To, References and a `Re:` subject. Plain text by default. | Write | notify | — |
| 4 | `gmail.send` | `{draft_id}`. Sends **an existing draft only**. The plan reads the draft and binds the call to its content digest, so a draft changed after the card was shown is refused (the `aws.stack.apply` pattern, `aws/stack.rs:58`). Counts against `max_sends_per_day`. | Write | **approve** (a posture, see Q3) | — |
| 5 | `gmail.label` | `{ids[], add?[], remove?[], trash?: bool}`. Archive (remove INBOX), mark read (remove UNREAD), star, user labels; `trash` moves to Trash (recoverable). | Write | notify; `trash` → **authority** | — |
| 6 | `calendar.events` | `{from?, to?, q?, calendar?="primary", max?}`. Events with attendees and their responses, conference link, organizer. | Read | inherit | yes, unless the organizer is the account |
| 7 | `calendar.put` | `{event_id?, summary, start, end, attendees?, location?, description?, notify_attendees?: bool=false, cancel?: bool}`. Create, update or cancel. | Write | notify; **authority** when invitations or cancellations go out (`notify_attendees` with attendees) | — |
| 8 | `drive.find` | `{q?, text?, name?, folder?, type?: doc\|sheet\|slides\|pdf\|folder, recent?: bool, max?}`. Typed fields compile to Drive `q`. `text` is `fullText contains` (the Drive API does have full-text search; gog doesn't). Rows: id, name, type, owner, modified, shared, link. | Read | inherit | yes, if any row isn't the account's own unshared file |
| 9 | `drive.read` | `{id, range?, save_to?}`. A Doc → markdown; a Sheet → its tabs plus the first 200 rows as a table (or `range`); Slides → per-slide text with object ids and layouts; PDFs and binaries → `save_to` (required past 1 MB). | Read | inherit | yes, unless owned by the account **and** `shared == false` |
| 10 | `drive.write` | `{id?, name?, folder?, content? \| from_path?, as?: doc\|sheet\|slides\|raw, move_to?, rename?}`. Create, or replace a file's content (Drive keeps revisions), or move or rename it. Markdown → Doc and CSV → Sheet go through Drive's import conversion. | Write | notify | — |
| 11 | `drive.share` | `{id, email?, role?: reader\|commenter\|writer, anyone_with_link?: bool, remove?: bool, notify?: bool}`, and `trash?: bool` for files. Every permission change and every trash. | Write | **authority** | — |
| 12 | `sheets.read` | `{id, ranges[]}`. Values, formatted or raw, plus tab metadata. | Read | inherit | as drive.read |
| 13 | `sheets.write` | `{id?, title?, range, values[][], mode: update\|append\|clear, input?: user\|raw}`. No `id` creates a spreadsheet. `user` = USER_ENTERED, so formulas evaluate. | Write | notify | — |
| 14 | `slides.write` | `{id?, title?, slides?: [{layout?, title?, body?, notes?, replace?: object id}], replace_text?: {find: replace}, requests?: [...]}`. No `id` creates a deck. Simple ops for the common case; `requests` passes raw `batchUpdate` through for anything else. | Write | notify | — |
| 15 | `google.call` | `{method: "drive.files.list" \| "gmail.users.settings.filters.list" …, params?, body?}`. Any REST method of the granted services. Classified by a method table (§3.2). | per method | per method | yes, always |

Not built:
- Contacts (no use found);
- Gmail settings: forwarding and filters need `gmail.settings.*` scopes, which aren't granted, and auto-forwarding is
  the classic exfiltration move;
- permanent delete (scope);
- Pub/Sub push (§5).

### 3.2 Postures and the `google.call` classifier

**Reading:** the template leaves reads to inherit `enforcement` (notify on the owner's machine), as the brief asked.
`[policy.tools]` can open them per tool, as the live config opens `aws.describe` and `fs.read`.

**Drafting, labelling, writing files, sheets and slides:** notify. Nobody else sees a draft until it is sent, and
Drive keeps revisions, so all of these can be undone.

**Sending mail:** approve, as a posture line (`"gmail.send" = "approve"`), not authority, so the owner can loosen it.
That matters for the killer use (§5). Earned grants and routine grants (`policy/earned.rs`,
`routine/grant.rs`) can then let one named routine send unattended; a routine grant can loosen only a posture-only
wait. This is owner question Q3.

**The approval card for a send** (Discord `render/preview.rs`, cockpit, `theseus confirm`) shows:
- from, to, cc and bcc, with every recipient **not already on the thread and never mailed before** flagged "new";
- the subject;
- the first ~15 lines of the body;
- the attachment names.

**Sharing, permission changes, trash, and calendar invitations or cancellations:** approve by authority. These are
outward or reach other people, so no config line loosens them, as AWS deletions of stateful resources wait for the
operator today.

**`google.call` classes:**
- GET → Read.
- Other HTTP verbs → Write.
- These methods → authority: `*.delete`, `*.trash`, `*.untrash`, `permissions.*` (create, update, delete), `*.send`
  (`messages.send`, `drafts.send`), `messages.import` and `messages.insert`, `calendar.acl.*`,
  `calendars.insert`/`delete`/`clear` (the full `calendar` scope allows them), and `events.*` with
  `sendUpdates != none`.
- Unknown methods → Write, and they still need a granted scope at Google.

`google.call` counts sends against the daily cap too, so the escape hatch can't route around the brake.


## The design's own words: §4.2 Inbound mail and shared docs are external untrusted text (prompt-injection vector #1)

**Anyone can email the owner, and anyone can share a doc with them.** So Google reads return
`theseus_tools::External { url }` (`theseus-tools/src/lib.rs:117`). The url is a stable `gmail:thread/<id>`,
`drive:<id>` or `calendar:<id>` marker, as `aws/external.rs` does with `s3://`. The existing hold then does the rest:
- `SessionRecord.external` is set, and a `session.external_read` row is written;
- after that, every non-Read call asks (`[policy] external_text = "ask"`) until the owner trusts the session (`/trust`
  or "Approve + trust session");
- the hold spreads to tasks the session starts.

**Two light refinements**, both in keeping with default trust:
- **Owner-authored content is not external.** A thread where every message is from the account, an event the account
  organizes, or a file the account owns that is not shared: none of these is stranger text. One metadata check per
  result: `From` headers, `organizer.self`, `owners[].me && !shared`. This removes most of the friction on the
  owner's own docs.
- **`gmail.draft` is exempt from the hold, as `wake.at` is.** A draft is invisible to everyone until `gmail.send`,
  which keeps its own approve posture and shows the draft on the card. Without the exemption, "read this thread and
  draft a reply" costs two cards (draft, then send) where one carries all the risk. Labels are not exempt: an
  injected mail must not be able to archive or mark read the mail it wants hidden.

**Residual risk, named and not solved here.** A held session can still call Read-class tools, and `http.fetch` is
Read. An injected mail could steer a fetch to `attacker.example/?q=<private text>`. This exposure exists today for
web text; Google makes it richer. Follow-up: in a session that holds both private Google content and the external
hold, `http.fetch` to a host outside `[tools] egress`/allowlists asks. That is a one-predicate change in
`external::gate`, filed with this work as a separate small row and **not** in the first cut unless the owner wants it.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `bbc65530` (2026-10-18
21:32 MST, the v1 cut's last join fix) or later, with **store format 45**. The Google Workspace design quoted below was
written against `c7ad21f7`, so every `file:line` in it is from then; on your base a line may have moved: search by name.

Line ceilings at `bbc65530` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` 2,706 /
2,733, `crates/theseus-discord/src/runtime.rs` 3,497 / 3,500 (full: never add a line there), `crates/theseus-core/src/turn.rs`
3,384 / 3,525, `crates/theseus/src/render.rs` 3,108 / 3,128, `crates/theseus-discord/src/render.rs` 2,963 / 3,009,
`crates/theseus-core/src/tests_m3.rs` 7,973 / 8,050, `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030. Files near
2,500 lines that are not listed yet: `crates/theseus-core/src/toolrun.rs` 2,489 (one registration call there, no more:
a file that passes 2,500 needs a ceiling entry, which this batch may not add). Never raise a ceiling in this batch. Put
new code in new modules; in a long file add only the `mod` line, the field or the call.

## v1 patch1: the night's cloud rows

Theseus's v1 is cut. v1 patch1 is the first fix release after it. The owner's words (Discord DM, 2026-10-19 00:21,
verbatim excerpt): "Yes to all. Looks wonderful. ... Please feel free to aggressively use cloud lanes ... to get this
work across the finish line quickly." "Yes to all" answered, among others: the Calendar build step is in, and the speed
regressions are fixed overnight with the benchmark measurement in the morning on a quiet host. Earlier (2026-10-18
12:18): "Make sure we set up Google workspace as a primary task for v1, patch1."

| Row | Slug | Beads | Prerequisite | Area |
|---|---|---|---|---|
| G1 | `gw-core` | theseus-batvg.1 | main | `crates/theseus-google/` (new), `theseus-core/src/google/{mod,auth,call,external,private,send_cap}.rs` (new), `config/google.rs` (new), `fact/google.rs` (new), `tools_defer/family.rs`, `toolrun.rs` (one call), `broker.rs` (the crown jewels), `data_gate.rs` and the route's data step (one predicate each), `rpc/google.rs` (new), `theseusd/src/main.rs` (the `google.check` phase), `theseus/src/{main,cmd}.rs` + a `google` CLI module, `theseus-protocol/src/google.rs` (new), the template, `docs/privacy.md` |
| G2 | `gw-mail` | theseus-batvg.2 | G1 joined | `src/google/{mail,mime}.rs` (new), `external.rs` (the draft exemption), its fake stubs, its template lines, census rows |
| G3 | `gw-drive` | theseus-batvg.3 | G1 joined | `src/google/drive.rs` (new), its fake stubs, its template lines |
| G4 | `gw-sheets-slides` | theseus-batvg.4 | G1 joined | `src/google/{sheets,slides}.rs` (new), its fake stubs, its template lines |
| G5 | `gw-calendar` | theseus-batvg.5 | G1 joined | `src/google/calendar.rs` (new), its fake stubs, its template lines |
| G6 | `gw-surfaces` | theseus-batvg.6 | G2 and G5 joined | `src/watch/{spec,mail}.rs`, `initiative/{morning,digest}.rs`, `rpc/work_board.rs`, `theseus-discord/src/render/preview.rs`, the cockpit's approval card |
| S1 | `speed-first-request` | theseus-nsp9l.1 | main | `config.rs` (`context_paths`), `census.rs`, `first_request.rs`, `tests_first_request.rs`, `fixtures/owner_shape.toml` |
| S2 | `speed-send-path` | theseus-nsp9l.2 | main | the turn's path before the request leaves (found by profiling; see the row) |
| S3 | `speed-idle-size` | theseus-nsp9l.3 | main | what an idle daemon keeps resident, and what grew `theseusd` (found by measuring; see the row) |

G2 to G5 run in parallel after G1's join; G6 after G2's and G5's. S1 to S3 run beside them. Joins are plain merges, one
at a time.

**Other work in flight on the owner's machine tonight** (local lanes; stay out of their areas): the data-routing lane
(an allowlist of model providers by origin for private data, failing closed; then routing hard tasks; then Jev's
warnings on prompts that draw out personal details: `routing/data.rs`, `data_gate.rs`, `catalog.rs`'s origin, the
route's data step, `config/routing_data.rs`, `config/privacy.rs`); a Discord test hang and the installer's timer PATH;
the import sync of the owner's decisions; the README; a voice fix.

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **G1 owns every shared Google piece.** The crate `theseus-google` (OAuth client, token refresh, PKCE, the loopback
   listener and the paste parser, the REST client, retry and backoff, the error map, the method classifier), the
   `Google` handle in `theseus-core/src/google/mod.rs`, the scope map with **every** tool's accepted scopes (the
   tools G2 to G5 build are named in it now: `gmail.search`, `gmail.read`, `gmail.draft`, `gmail.send`, `gmail.label`,
   `calendar.events`, `calendar.put`, `drive.find`, `drive.read`, `drive.write`, `drive.share`, `sheets.read`,
   `sheets.write`, `slides.write`, `google.call`), the families (`mail`, `calendar`, `docs`) for all those names, the
   `google.called` fact and the auth facts, the daily send counter, the External and owner-authored helper, and the
   `private` marker. G2 to G5 **use** them and never redefine them; if one lacks what a row needs, the row adds the
   smallest public accessor in G1's module and says so in its report.
3. **One way in for tools.** G1 registers Google's tools with **one** call in `toolrun.rs` (beside the AWS tools),
   which takes the list from `google::tools(&Google)` in `google/mod.rs`. A later row adds its tools by adding **one
   line** for its own module to that list (keep the list in the order mail, drive, sheets, slides, calendar, with each
   row's line separated from the next by G1's marker comments, so two rows' lines never touch). No later row edits
   `toolrun.rs`.
4. **Every Google call goes through G1's `Google::call`** (or a typed wrapper of it), which writes the `google.called`
   row, retries what may be retried, refuses a repeat of what may not (`Retry::NonRepeatable`), and maps errors. No row
   writes its own fact or its own HTTP client.
5. **Every Google tool result goes through G1's `google::finish`**, which sets the `private` marker and decides External
   with the owner-authored rule from the metadata the row hands it (`From` headers, `organizer.self`, `owners[].me` and
   `shared`). A row never builds a Google `ToolOutput` any other way.
6. **The fake.** G1 builds the loopback fake Google in `crates/theseus-google/src/fake.rs` (the token endpoint, the
   userinfo/`id_token` account, a route table, a request log, and a switch for 429s, 5xx, `invalid_grant` and
   `SERVICE_DISABLED`). Each later row puts its service's stubs in its own file, `fake/<service>.rs`, with **one** `mod`
   line and **one** route-table line in `fake.rs`. No test touches a real Google account.
7. **The template** (`crates/theseus-core/config/theseus.example.toml`): G1 writes the `[google]` section and, under
   `[policy.tools]`, the `google.call` line plus four marker comments, each on its own line with an unchanged line
   between them: `# gw-mail: the gmail.* lines`, `# gw-drive: the drive.* lines`, `# gw-sheets-slides: the sheets.* and
   slides.* lines`, `# gw-calendar: the calendar.* lines`. Each later row replaces only its own marker with its lines.
   If the template test `example_template_lists_every_tool_under_policy_tools` refuses a marker or a line for a tool
   that is not registered yet, follow what it allows and say so.
8. **The protocol:** G1 puts its wire types (the auth begin/finish, revoke and status methods and results) in
   `crates/theseus-protocol/src/google.rs` and adds **one** `pub mod` line to `lib.rs`; G6 adds its types to that
   module, no new line in `lib.rs`. Regenerate `cockpit/src/protocol.gen/` when you change protocol types.
9. **The store format:** G1 bumps it once, 45 → 46, for everything it stores (a session's private-source mark if it is
   a field, the new ledger kinds), with the old layout's literal sample in `tests_layouts.rs`. G2 to G5 store nothing
   new. G6 bumps by one from its base for `Check::Mail`. S1 to S3 store nothing new; if one must, it says why. The
   joiner renumbers.
10. **The data rule:** a Google result reaches the data rule only through G1's mark: once a session has read any Google
    result, the session's data answer is `private` by code, as a sensitive name makes it today
    (`routing::data::DataAnswer::private_by_name`, theseus-oejh3), and the provider floor (`data_gate.rs`) refuses a
    request of that session to a model of restricted origin. G1 adds one condition to each of those two places and no
    origin logic of its own (the data-routing lane is changing what "restricted" means tonight: call its predicate,
    never a model or provider name).
11. **Sends always ask, deterministically** (the owner's decision, which overrides the design): see G2 and G1.
12. **Leave `crates/theseus-discord/` and the cockpit alone** except G6's send card preview. Never add a line to
    `theseus-discord/src/runtime.rs`.

## Out of scope for every row

Any call to a real Google account (the live check is the maintainer's, with the owner, after the joins). The owner's
config, his 1Password items and his daemon. Pub/Sub push, Contacts, Gmail settings (forwarding, filters), permanent
delete, a second account. The `http.fetch` egress predicate for sessions holding private Google content and the
external hold (the design's §4.2 residual risk: a follow-up row). The benchmarks themselves: speed is measured in the
morning on a quiet host, never by a row. `docs/status.md`, the README, `docs/design/` and the spec (the maintainer
writes those; say what should change). If your work shows one of these is needed, write it in the report and stop
there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Nothing without config.** With no `[google]` section the Google code does nothing: no tool is registered, no
  startup phase runs, no file or secret is read, and a plain turn is byte-for-byte what it is on `main` (a test shows
  it). Speed rows change no behaviour a person can see beyond speed and size, unless the row says so.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). Every Google tool is deferred. FAST:
  nothing new on a plain turn (a plain turn stays 5 frames), and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package (a new workspace crate's
  own entry is fine). The workspace already has what this batch needs: `reqwest` 0.12 (rustls), `serde_json`, `sha2`,
  `base64`, `rand`, `url`, `percent-encoding`, `chrono`, `zeroize`, `axum` and `tokio`. HTML to text is
  `theseus-core/src/web/html.rs`'s `to_text`. A store format change only as the shared contracts say (bump by one
  from 45 and add the old layout's literal sample to tests_layouts.rs; the joiner renumbers).
- **Secrets and names.** A token, a client secret or an authorization code is held in memory only (`Zeroizing`, a
  masked `Debug`), never on a command line (`/proc` shows argv), never in a log, a fact, an error message, a report or
  a test's output; a test plants a secret-shaped value and shows it never reaches what is stored or printed. Run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture accounts are invented (`owner@example.test`,
  `collaborator@example.test`); never a real address, vault or item name.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
