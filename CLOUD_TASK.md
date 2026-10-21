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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-gw-surfaces`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: gw-surfaces, Google Workspace part 6: mail in the killer features: the mail watch with its deadline, the morning note's day, the digest's Google line, the board's reply wait, and a send card that shows the exact draft it binds (theseus-batvg.6, theseus-dfk31)

Branch: `cloud/20261021-gw-surfaces`. Every commit's subject carries `theseus-batvg.6` (the send-binding commit carries
`theseus-dfk31` as well). Deadline for the report: 5 hours after you start.

## Your base is NOT main: it is `integ/google` (this wins over the section above about `main`)

This row builds on the Google integration lane, which is not on `main` yet. Start like this, before anything else:

```
git fetch origin integ/google
git checkout -B cloud/20261021-gw-surfaces origin/integ/google
```

`integ/google` is `main` at `88ddae57` (it holds `1d092ded`, so the note above on store format 47 and line ceilings
still holds) plus three accepted rows: **gw-mail** (theseus-batvg.2: `crates/theseus-core/src/google/mail.rs` with
`gmail.search/read/draft/send/label`, `google/mime.rs`, the fake mailbox `crates/theseus-google/src/fake/gmail.rs`,
the draft's hold exemption, and `Plan.card: Option<Value>` in `crates/theseus-protocol/src/gate.rs`, in memory only),
**gw-calendar** (batvg.5: `google/calendar.rs`, `calendar.events/put`) and **gw-drive** (batvg.3: `google/drive.rs`).
If `google/mail.rs` or `google/calendar.rs` is missing, stop at once and report it: wrong base. Take any later lane
commits (always the lane's head).

Read first: `google/mod.rs` (`Google::call`, `Meta`, `Called`, `CallFailed`, `NAMES`, `tools()`), `google/external.rs`
(`finish`, `Source`), `google/private.rs` (`scoped`, `in_scope`), `fact/google.rs` (`GoogleCalled`), `mail.rs` and
`calendar.rs` whole, and the AGENTS.md of each crate you touch and of `cockpit`. Use the Google pieces; never redefine
them.

## Why

Task management across horizons is Theseus's north-star feature, and email is where it changes most. Today, on the
lane:
- **"Wake me when they reply" cannot be said about mail.** `watch` has `run`, `fetch`, `file` and `reply` (Discord)
  checks (`crates/theseus-core/src/watch/spec.rs`, `enum Check`); nothing looks at a mailbox.
- **"Follow up if no reply by Friday" cannot be said at all.** A watch that runs out only posts
  `👁 gave up on "…" after …` (`watch/step.rs`, `expired_text`); nothing happens at the deadline.
- **The morning note knows nothing of the day.** `initiative/morning.rs` (`morning_facts`) counts tasks ended, wakes
  fired, promises unkept and memory notes proposed. Today's meetings, the unread mail that matters and the drafts
  waiting are not in it, and it still ends "Nothing is waiting on the owner." when two drafts wait for a send.
- **The digest does not say what Google work was done.** `work.digest` (`crates/theseus-core/src/rpc/digest.rs`,
  words in `crates/theseus-protocol/src/work_digest.rs`) never reads the `google.called` rows.
- **The send card is a line of text.** `gmail.send`'s plan carries the card's facts in `Plan.card`, but nothing reads
  it: `ConfirmRequest.preview` comes from `ToolRuntime::preview` (`crates/theseus-core/src/tool_words.rs`) by tool name
  alone, so Discord's card (`theseus-discord/src/render/preview.rs`, `hunk`) and the cockpit's
  (`cockpit/src/components/ConfirmCard.tsx`, `Preview`) show only the summary line.
- **The approval is not bound to the draft it showed (the gw-mail review's medium finding, theseus-dfk31).** The pin
  is one daemon-wide slot per draft (`Shown.pinned`, set by `SendTool::planned`): session X plans `gmail.send d` (pins
  D1, card C1 waits); another session rewrites `d` (D2) and plans a send (pins D2); the owner approves C1; X's run sees
  pinned == live == D2 and sends what C1 never showed. And `Parsed::digest` omits attachment bytes and the HTML part
  when a plain part exists.

## Risk: medium

A stranger's mail must never start a model turn; a follow-up must never send by itself; the card must show exactly the
draft that goes.

## Prerequisite: `integ/google` (above).

## The owner's decisions (they win over the design)
- **Every send asks the owner, every time.** A follow-up task may read and **draft**; its send waits on the card like
  any other. Nothing in this row sends.
- **Polling, not push.** One account, `users.history.list` from the last `historyId`: about 2 quota units a poll and no
  model call. No Pub/Sub, no public endpoint.
- **Stranger text never starts a turn.** As the Discord `reply` check: notice only (`spec::build` refuses `then: turn`
  or `task` for a reply today). The one exception is below.
- **Real meetings stay in Google Calendar.** Never copy them into Discord guild events (that leaks titles to a guild).

## Your row

Work in this order; each part is a commit of its own. If the deadline comes before the end, parts 5 and 6 are the ones
to leave, named in the report as follow-ups.

### 1. The send binds the draft it showed, and the card shows it (theseus-dfk31)
- `gmail.send`'s input becomes `{draft_id, digest}`, both required (one right shape, no alias). `gmail.draft`
  already returns the digest in its meta; put it in the result text too, and make `gmail.read id "draft:<id>"` return
  it the same way, so the model always has it. The approval binds `{tool, input}` (spec §3.9), so the digest in the
  input is per approval, not per draft.
- Plan: refuse when the digest is not the one this daemon last showed for that draft (the card's facts come from
  `Shown`, so a card is only made for the digest it describes). Run: read the draft again and refuse, sending
  nothing, unless the live digest equals the **input's** digest. Delete `Shown.pinned` and the pin logic: nothing
  shared stays between plan and run. Keep: non-repeatable, `outcome_unknown` on 503, the daily cap reserved and
  released, never retried.
- `Parsed::digest` also covers each attachment's size and content hash (or its `attachmentId` and size, if reading the
  bytes costs a second call; say which in the report) and the HTML part even when a plain part exists.
- The card: `Plan.card` gains `"kind": "mail"` and keeps the full digest. The question's preview comes from the plan:
  in `ToolRuntime::preview` (or at its call site in `toolrun.rs`, where `g.plan` is in hand: `preview:
  self.preview(tc.dir, tool, &call.input)`), a plan with a mail card gives
  `ConfirmPreview { kind: "mail", body }` where `body` is `google::mail::card_text(card)` plus one line
  `Draft <first 12 hex of the digest>`, scrubbed as every preview is. Add `pub const MAIL: &'static str = "mail";`
  to `ConfirmPreview` (`crates/theseus-protocol/src/diff.rs`). No new `ConfirmRequest` field (`lib.rs` has three lines
  of room). The preview is stored with the request (`fact::tool::CallAsked`), so `confirm.list` shows it too: check.
- Discord: `render/preview.rs` `hunk()` renders a `mail` preview as a quote block: From, To/Cc/Bcc with each new
  recipient marked **(new)**, Subject, the body's first lines, the attachments, and the `Draft <12 hex>` line, cut to
  the card's size with a marker as the diff is. The cockpit: `ConfirmCard.tsx` `Preview` (and `previewFor` in
  `cockpit/src/lib/toolwords.ts`) render the same fields, new recipients highlighted, the digest line in small text.

### 2. `Check::Mail`: the mail watch
- Wire: `WatchCheck::Mail { q: String }` in `crates/theseus-protocol/src/watches.rs` (a Gmail search query, required,
  non-empty), and `WatchCreateParams.follow_up: Option<String>` (a brief; part 3). Core: `Check::Mail { q }` in
  `watch/spec.rs` (`kind() == "mail"`, `target()` is the query), stored in the probe's `Subject` JSON as a new
  optional `q` field (the subject is a string inside the wake: no store-format bump; if you find you must add a field
  to a stored record, bump as the note above says).
- `build`: a mail watch needs `[google]` with `gmail` among its services, else it is refused with the reason ("mail
  watches need [google] with gmail"). Its condition is "a new message matches" (`Condition::Exists`, as a reply's);
  `until` must be empty. `every` defaults to `3m` for a mail watch (the `[watch_for]` minimum still applies).
  `then: turn` is refused ("mail is outside text; a turn would act on it"). `then: task` is allowed only under the
  trusted-sender rule below. `notice` is the default.
- The look (`watch/check.rs` `observe`, a new arm; put the Gmail code in a new file `watch/mail.rs`, one `mod` line):
  - No model call, ever. `check::Tools` gains the account handle (`Option<Arc<Google>>`); with none, a mail check
    fails its look with the reason.
  - First look: read `users/me/profile`'s `historyId` and keep it as the cursor. Every later look:
    `users.history.list` with `startHistoryId=<cursor>` and `historyTypes=messageAdded`. With no added messages,
    that is the whole look. With some, run one `messages.list` with `q=<the watch's q>` and keep only the added ids it
    returns; read each match's `From` and `Subject` (metadata format). Advance the cursor to the answer's `historyId`.
  - A 404 on `history.list` (the cursor is too old) re-baselines from the profile and says so in the watch's row,
    never silently.
  - The cursor lives in META under `watch.mail.<wake id>` (written in the frame that records the look), so a restart
    resumes from it; delete the key when the watch ends.
  - Every call goes through `Google::call` with a `Meta` (`class: "read"`), so each has its `google.called` row and
    the private mark.
- A match is one `watch.met` frame and a notice, as a reply's (`watch/reply.rs`): From and Subject and the first 200
  characters of the snippet, scrubbed, marked `external`, posted only in a private place: the watch's own place when
  that is private, else the owner's Home, never a shared place.
- **The trusted-sender exception.** `[google] trusted_senders = ["…@example.test"]` (new key in
  `crates/theseus-core/src/config/google.rs`, default empty, addresses compared case-folded; template line in
  `theseus.example.toml` beside `[google]`'s other keys). A mail watch with `then: task` is refused at create when
  the list is empty. At a match, the task starts only when **every** matching message's `From` address is in the list;
  otherwise the match is a notice that says why ("from a sender not in trusted_senders: notice only"). The task's
  session starts **held**: `external::hold` with the mail as its `ExternalText` before its first turn, so its first
  call that acts asks the owner. The brief is the owner's (from `then.brief`), and the mail reaches the task only as
  quoted, external text.

### 3. The deadline's follow-up
- `follow_up` (a brief) is allowed only on a mail watch, and only with `for_at_most` (the deadline). On expiry, a watch
  with a `follow_up` starts a task with that brief (the owner's words, so not held for them) and posts one line
  instead of the "gave up" notice: `👁 no reply on "<title>" by <deadline>: follow-up task <short> started`. The
  task may read the thread (which holds it, as any mail read does) and draft; its `gmail.send` asks as every send does.
- Store the brief in the probe's subject JSON (`follow_up`), next to `brief`.

### 4. The morning note's day
- New file `initiative/morning_google.rs` (one `mod` line), called from `morning_facts`' caller only when `[google]`
  is configured. Three facts, each read through `Google::call` with its `google.called` row recorded:
  - **Today's events** (`calendar`), in the daemon's zone, from local midnight to midnight: start and end times, the
    title, how many attendees, the call link, and a `conflict` mark on events that overlap. Reuse `calendar.rs`'s
    listing and rendering (make a small `pub(crate)` function there rather than copying it).
  - **Unread that matters** (`gmail`): `is:unread -category:promotions -category:social newer_than:12h`, as a count
    plus the top three senders and subjects.
  - **Drafts waiting** (`gmail`, `users.drafts.list`): a count. With drafts waiting, the note no longer ends "Nothing
    is waiting on the owner."
- A day with events or such mail is a day something happened (the note posts).
- **The note's model call carries private text, so it must never go to a model of restricted origin.** Make the
  initiative request inside `google::private::scoped(true, …)` whenever a Google fact is in the brief; the provider
  floor (`data_gate`) then refuses a restricted-origin profile, and `initiative_speak` falls back to its template
  (it never retries on another profile by itself). The note speaks and acts on nothing.
- A Google read that fails leaves its line out and adds `calendar unreachable` (or `mail unreachable`); it never
  blocks the note. The reads run on the note's own task, never on the tick.

### 5. The digest's Google line
- Count, from the `google.called` rows in the digest's window (read `fact/google.rs` and `Google::call` for the row's
  fields; match on the method id and a successful status): mail sent (`gmail.users.drafts.send`), drafts waiting
  (drafts created or updated whose id has no later successful send in the window), threads waiting on a reply (open
  mail watches), and docs written (Drive file creates and updates).
- Wire: `WorkDigestResult.google: Option<DigestGoogle>` (a struct of four counts) in a new protocol module file,
  re-exported with one line; absent when `[google]` is not configured or every count is 0. `digest_text` adds one line,
  `✉ mail sent 3 · drafts waiting 2 · waiting on 4 replies · docs written 1`, listing only the counts above 0.
  Discord's `/digest`, the digest post (`initiative/digest.rs`) and the CLI then say it with no code of their own.

### 6. The board's reply wait
- A mail watch's board line reads `waiting: reply on '<title>' (2 d, follow-up Fri)`: how long it has waited, and the
  follow-up's day when it has one (no follow-up: only the age in the parenthesis). Word it once, in a pure function
  in `crates/theseus-protocol/src/work/horizon.rs` (beside `horizon_of`), and use it where the watch's view is built
  (`crates/theseus-core/src/push/work/horizon.rs`; the watch's purpose is `Purpose::Watch`). Find the field the board
  shows for a watch and fill it there; every surface (CLI board, TUI, Discord, cockpit) then shows the same words.

## Tests (each with a planted revert; the first is the key test)

All against the loopback fake (`[google] endpoint`). Extend `fake/gmail.rs` with what the tests need: a `historyId`
that advances when a test adds a message, `GET history` (with a 404 for a too-old `startHistoryId`), `messages.list`
honouring a `from:` term of `q`, and `GET drafts`. Keep each fall-through claiming only its own paths.

- **`a_strangers_mail_never_starts_a_turn`**: a mail watch with `then: task` and a trusted list; a message from an
  address not on it gives one notice and no model request and no task. Plant: let any sender start it.
- `a_mail_watch_with_then_turn_is_refused` and `a_mail_watch_needs_google_with_gmail`.
- `a_trusted_senders_mail_starts_a_held_task`: the task's session holds external text, and its first non-Read call
  asks. Plant: start the task unheld.
- `history_poll_resumes_from_the_last_history_id`: across a core restart in the fake, the next look starts from the
  stored cursor (the fake records each `startHistoryId`). Plus `a_stale_history_id_rebaselines_and_says_so`.
- `a_poll_with_no_new_mail_makes_one_call` (one `history.list`, no `messages.list`).
- `a_reply_deadline_fires_the_follow_up` (paused clock) and `the_follow_up_drafts_and_its_send_still_asks`.
- `a_send_is_refused_when_the_input_digest_is_not_the_drafts`: two sessions on one draft, as the Why's sequence; the
  approved call of session X sends nothing. Plant: compare against the shown digest instead of the input's.
- `an_attachment_change_after_the_card_is_refused` and `an_html_only_change_after_the_card_is_refused`.
- `the_send_question_carries_the_mail_preview` (kind `mail`, new recipients marked, the 12-hex digest line, scrubbed).
- `the_discord_send_card_shows_the_preview_and_flags_new_recipients` (a golden under `render/golden` if the module
  uses them) and the cockpit's test beside `ConfirmCard` (vitest).
- `morning_note_lists_todays_events` (times, attendees count, call link, a conflict) and
  `morning_note_counts_unread_and_drafts_waiting`.
- `the_morning_note_never_uses_a_restricted_model`: with `[initiative] profile` set to a restricted-origin profile and
  a Google line in the brief, no request reaches that provider and the note goes out as its template. Plant: drop the
  `scoped(true, …)`.
- `digest_counts_sent_mail` (and drafts waiting, docs written) and `the_board_shows_a_reply_wait`.
- **Rollback by absence:** with no `[google]` section, nothing here reads or adds anything: the morning note, the
  digest's text and wire, and `watch.list` are byte-for-byte the base's (an existing golden or a test that compares).

Run `google::`, `watch::`, `initiative`, `config::` and the protocol's wire and TS tests with `TZ=America/Phoenix`.

## Not in this row
Calendar-invite and Drive-share card previews; Pub/Sub push; a second account; meetings as Discord events; "prep me
15 minutes before the meeting"; sheets and slides; `docs/` (list what changed for the maintainer).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename or reshape (here, `gmail.send`'s input), fix every caller
  (tests, fixtures, the template, tool descriptions). Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. The rollback here is the absence of
  `[google]` (or of `gmail`/`calendar` in its services): a test shows that without it the old behaviour is
  byte-for-byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool; the
  `watch` tool's schema gains the `mail` kind and `follow_up` in as few words as the other kinds use. FAST: nothing new
  on a plain turn (a plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's
  turn bench must not move), nothing on the path before a request leaves, nothing on the start path grows with
  history. The morning note's and the watch's Google reads run on their own tasks, never on the driver's tick.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. The gw-mail tests that
  assert the old pin (a send after another `gmail.draft` is refused) keep their intent: rewrite them to the input
  digest and say so in the report. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/integ/google --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings.
- Line ceilings: `scripts/long-files.txt` (default 2,500 lines for a file not listed). New code goes in new files.
  `crates/theseus-protocol/src/lib.rs` has three lines of room: add nothing there but a `mod`/re-export line if you
  must.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as the note above says.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a notice, a preview, a report, a push) through `core.tools.scrubber`. A `google.called`
  row holds no content, subject or address. No person's name anywhere in the repo, in commit messages or in the
  report: write "the owner". Fixture addresses are invented `@example.test` ones.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).

## Your report
Commit `CLOUD_REPORT.md` at the repository root (editor tools, never a shell heredoc; not empty after the commit):
what changed per part (1-6) with files, and any part left as a follow-up; each test and each plant with the test that
failed (key test first); what could not run on the VM with exact commands (`TZ=America/Phoenix`); the keel guard's
output; the store format (bumped or not, why); wire/TS changes, the `gmail.send` input change and its callers, the
line count of each file you grew, the docs to update; follow-ups.

Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
