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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-mail`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `6918e3c1` (2026-10-21, after the patch1 stage-1 landing), unless your task names another base (an integration branch). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
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
## Your task: tail-mail, a mail watch that trusts only mail Gmail authenticated, skips drafts and sent mail, and stops logging quiet looks; a send card that flags only recipients and catches a swapped attachment (theseus-g4lit, theseus-36sm6, theseus-55j04, theseus-axbqw, theseus-u4dvr)

Branch: `cloud/20261021-tail-mail`. Every commit's subject carries the Beads id(s) it closes (`theseus-g4lit`,
`theseus-36sm6`, `theseus-55j04`, `theseus-axbqw`, `theseus-u4dvr`). Deadline for the report: 5 hours after you start.

**Why.** Google Workspace's mail row shipped in patch1 with five follow-ups from its review. Each is a thing a person
hits:
- **A forged From starts a task (g4lit).** A mail watch with `then: task` starts a held task with the owner's brief
  when every matching message's `From` address is in `[google] trusted_senders`. The From header is the sender's own
  claim: anyone can write `From: <a trusted address>`. The task is held and private, so the damage is bounded to the
  model reading the stranger's text, but the trust rule itself is spoofable. The owner's call on Theseus (verbatim):
  "This tends to be caught directly by gmail, which classifies them according to this standard ... Ship and follow
  up is fine." The follow-up: trust only mail that Gmail itself authenticated (DKIM or DMARC pass for the From's
  domain), read from Gmail's own `Authentication-Results` header.
- **Drafts and sent mail read as news (36sm6).** Gmail's history reports the owner's own drafts and sent messages as
  added, and `messages.list` returns them, so a watch on a thread sees the owner's own reply as a match; with the
  owner's address in `trusted_senders`, the agent's own draft could start a task.
- **Every quiet look is a ledger row (55j04).** A look with no new mail is one `history.list` call, so one
  `google.called` row: about 480 rows a day per mail watch at the 3-minute default, all saying "nothing".
- **Draft text can fake the "new recipient" flag (axbqw).** The Discord send card bolds every literal `(new)` in the
  draft's quoted lines, so a subject or a body line containing `(new)` reads like the new-recipient warning.
- **A swapped attachment of the same name and size sends unseen (u4dvr).** The send digest names an attachment by
  name, type and size (Gmail mints a new attachment id per read, so the id cannot be in it). A draft whose attachment
  is replaced, after the owner saw the card, by another file of the same name, type and byte size sends under the
  owner's approval of the first.

**Risk: low to medium.** g4lit tightens trust: mail that Gmail did not authenticate no longer starts a task (it still
notices). 36sm6 narrows what a look matches. 55j04 changes how many `google.called` rows a quiet watch writes (not what
any digest counts: `rpc/digest_google.rs` counts sends, drafts and drive writes, never `history.list`). u4dvr adds one
attachment read per attachment at the card and at the send.

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| T5 | `tail-mail` | theseus-g4lit, theseus-36sm6, theseus-55j04, theseus-axbqw, theseus-u4dvr | trust needs Gmail's authentication; drafts and sent mail are no match; quiet looks are tallied, not logged one by one; `(new)` bold only on recipient lines; attachment content in the send digest | `crates/theseus-core/src/watch/{mail,mail_then,step,run,board}.rs`, `watch/mail_auth.rs` (new), `watch/quiet.rs` (new), the `mod` lines in `crates/theseus-core/src/watch.rs`, `watch/tests_mail.rs`, `watch/tests_watch.rs` (a helper's call only), `watch/tests_mail_tail.rs` (new), `crates/theseus-core/src/google/{mail,mime}.rs`, `google/tests_mail.rs`, `crates/theseus-google/src/fake/gmail.rs`, `crates/theseus-discord/src/render/preview.rs` | M |

## What exists (lines at `6918e3c1`; search by name)
- **The look**: `crates/theseus-core/src/watch/mail.rs` `look` (near :110). First look: `baseline` reads the profile's
  `historyId`. Later looks: `users.history.list` from the cursor (`historyTypes=messageAdded`), then, only if ids were
  added, one `messages.list` with `q` (the watch's query as the owner wrote it, near :154-157), keeping listed ids that
  are in `added`, at most `MAX_MATCHES`; each match is read with `format=metadata` and `metadataHeaders` `From` and
  `Subject` (near :176-180). `sender_of` lowercases the From's address (an unparseable From gives `""`, which is no one
  trusted). `Seen` (near :39) holds `cursor`, `rebaselined`, `senders`, `trusted`; it is memory only (not stored).
- **The trust decision**: `watch/run.rs` `Core::watch_seen` (near :142) writes the look's rows as `google.called`
  facts in one frame, then sets `m.trusted = self.mail_trusted(m)`. `watch/mail_then.rs` `Core::mail_trusted` (near
  :30): every sender is in `[google] trusted_senders` (case-insensitive) and there is at least one. `watch/step.rs`
  (near :203) puts `{"external": true, "trusted": m.trusted}` in the news' detail; `mail_then` (near :49) reads
  `news.detail["trusted"]` and otherwise appends `"\nfrom a sender not in trusted_senders: notice only, no task
  started"`.
- **`mime::header`** (`google/mime.rs` near :250) returns the **first** header of a name in the payload's order.
  Gmail's metadata response lists headers top to bottom as received, so the first `Authentication-Results` is the one
  Gmail's own server (authserv-id `mx.google.com`) prepended on arrival. A sender can put their own
  `Authentication-Results` lines lower in the message; they are not Gmail's.
- **The rows**: `Core::watch_check` (`watch/run.rs` near :125-138) runs `check::observe` with a `rows` vector, then
  `watch_seen(&rows, result)`, then `watch_finish`. Callers of `watch_seen`: `run.rs`, `watch/tests_mail.rs` (near
  :392, `a_poll_with_no_new_mail_makes_one_call`), `watch/tests_watch.rs` (near :142, the `look` helper). A row is
  built by `Google::row` (`google/mod.rs` near :547): `service, tool, method, resource, class, status, attempts,
  quota_units, outcome, ms` (never content). `watch/step.rs` `Entry` (near :36) is memory only; it already keeps a
  memory-only `kept_ms` so the cursor is written to the store at most every `CURSOR_KEEP_MS` (6 h): the same idea fits
  quiet looks. `watch/board.rs` `Board::remove` (near :57) takes an entry off the board.
- **The send card and digest**: `google/mail.rs` `Parsed::digest` (near :260) hashes thread, from, to, cc, bcc,
  subject, body, html and each file as `[name, mime, size]`. `mime::walk` (near :303) collects
  `Attachment {name, mime, size, id}` from parts with a filename and an `attachmentId`. `card_text` (near :311) writes
  `To:/Cc:/Bcc:` lines with ` (new)` after a new recipient. The Discord renderer
  `crates/theseus-discord/src/render/preview.rs` `mail` (near :75) does `l.replace("(new)", "**(new)**")` on **every**
  quoted line.
- **Tests that already cover u4dvr's pieces** (`google/tests_mail.rs`): `an_html_only_change_after_the_card_is_refused`
  (near :376), `an_attachment_change_after_the_card_is_refused` (near :344, adds a file), and
  `an_unchanged_draft_with_an_attachment_sends` (near :362). So an HTML change and an added file are caught today; a
  same-name, same-type, same-size swap is not.
- **The fake** (`crates/theseus-google/src/fake/gmail.rs`): `MsgSpec::header` sets any header, `MsgSpec::label`,
  `deliver`, `attach_to_draft`, `edit_draft_html`. `list_messages` (near :569) treats a `q` that merely **contains**
  `in:sent` as the sent-to check (`list_sent`), so `-in:sent` would trip it today; it honours `-category:*` by label.
- None of the five issues has landed: no commit on `main` names them, and the code above is as described.

## What to build
1. **Trust needs Gmail's authentication (g4lit; the key change).**
   - New `crates/theseus-core/src/watch/mail_auth.rs`: `pub fn authenticated(payload: &Value, from: &str) -> bool`,
     pure. Take the **first** `Authentication-Results` header (via `mime::header`). It counts only if its authserv-id
     (the text before the first `;`, trimmed) is `mx.google.com`, case-insensitive. Then, with `domain` = the From
     address's domain, lowercased, it passes when either:
     - a `dmarc=pass` result whose `header.from` equals `domain`, or
     - a `dkim=pass` result whose `header.d` (or, if absent, the domain of `header.i`) is aligned with `domain`
       (relaxed alignment: equal, or one is a subdomain of the other at a dot boundary, never a bare suffix:
       `evilexample.test` is not aligned with `example.test`).
     Parse the results as `;`-separated `method=result` clauses with `key=value` properties; ignore comments in
     parentheses; unknown methods are ignored. Missing header, another authserv-id, `fail`, `none`, `neutral`,
     `temperror`, `permerror`, or a misaligned pass: **false** (fail closed). An empty From domain: false.
   - `look` adds `Authentication-Results` to the matched messages' `metadataHeaders` and records, per matched
     message, whether `authenticated(&msg["payload"], &from)` held. `Seen` gains `pub unverified: Vec<String>`: the
     (lowercased) senders whose mail Gmail did not authenticate.
   - `mail_trusted` is true only when every sender is listed **and** `unverified` is empty. The notice text keeps its
     two reasons apart: a sender not on the list keeps today's line exactly; listed senders whose mail was not
     authenticated get `"\nfrom a trusted address Gmail did not authenticate (no DKIM or DMARC pass): notice only, no
     task started"`. Carry the second reason to `mail_then` through the news detail (`watch/step.rs`: add
     `"unverified": !m.unverified.is_empty()` beside `"trusted"`). If both apply, the not-listed line wins.
   - **No switch.** This closes a spoof; a switch to reopen it would be the wrong default and a config surface the
     owner did not ask for. The rollback is reverting the commit. Do not touch `[google]` config or the template.
2. **Drafts and sent mail are no match (36sm6).** In `look`, list with `q` = `"({q}) -in:drafts -in:sent"` (the
   owner's query in parentheses, so an `OR` in it keeps its meaning), and, as the guard that does not depend on the
   query, drop a matched message whose metadata `labelIds` contain `DRAFT` or `SENT` before it becomes a line or a
   sender (the metadata read already returns `labelIds`; no new call). The fake's `list_messages` learns the two
   exclusion terms by label, and its sent-to check matches only a whole `in:sent` term not preceded by `-` (the
   `gmail.send` "never mailed before" check, `list_sent`, must keep working: its tests are in `google/tests_mail.rs`,
   `send_card_flags_new_recipients`).
3. **Quiet looks are tallied (55j04).** A look is **quiet** when it succeeded, matched nothing, did not rebaseline,
   and every one of its rows has `outcome == "ok"`. A quiet look's rows are not written one by one: they are added to
   a memory-only tally on the watch's `Entry` (new `watch/quiet.rs`: `Quiet { looks, calls, quota_units, ms, since_ms }`
   with `add(&rows, now)` and `take(&self) -> Option<Value>`), and the tally is written as **one** `google.called`
   row: `{"service": "gmail", "tool": <the mail search tool name>, "method": "gmail.users.history.list", "class":
   "read", "status": 200, "outcome": "ok", "quiet_looks": n, "calls": c, "quota_units": u, "ms": total}` (no
   resource, no address). It is written, in the same frame as the rows that trigger it, when (a) the next non-quiet
   look of that watch writes its rows (the tally first), (b) the tally's first look is `QUIET_ROW_MS` (1 hour, a
   `const` in `quiet.rs`) old at a later look, or (c) the watch leaves the board (`Board::remove` or the ending path in
   `watch_finish`; one place, not both). So a quiet watch writes about 24 rows a day instead of about 480, and every
   call is still counted. A first look (baseline), a rebaseline, a failure and a match write their rows as today.
   `watch_seen` gains the wake id it needs (update its three callers). A restart loses at most one hour's tally
   (memory only, like `kept_ms`): say so in the report; no stored field, no store format change.
4. **`(new)` bold only on recipient lines (axbqw).** `preview.rs` `mail`: apply the `**(new)**` replacement only to a
   line that starts with `To: `, `Cc: ` or `Bcc: ` (the lines `card_text` writes). Subject and body lines keep a
   literal `(new)` as plain text.
5. **Attachment content in the digest (u4dvr).** `Parsed::digest`'s file entry becomes `[name, mime, size, sha256 of
   the bytes]`. Where the card's draft is read and where the send re-reads it (`fetch_draft`/`show` and the send's
   live check in `google/mail.rs`), fetch each attachment's bytes once the way `save` (near :809) already does
   (`messages/{mid}/attachments/{id}`, method `gmail.users.messages.attachments.get`, class `read`, through `rest`,
   so it has its `google.called` row; share one helper with `save` rather than a copy), hash, drop the bytes. A failed
   attachment read fails the card or refuses the send (fail closed), with Gmail's error. A part with a filename and
   inline `body.data` but no `attachmentId` is skipped by `mime::walk` today: hash its inline data so it is not
   invisible to the digest either. Cost: one read (5 quota units, `quota_units`'s default) per attachment per read; a
   draft without attachments makes no new call.

## Tests (each with a planted revert; the first is the key test)
- **`a_forged_from_of_a_trusted_sender_only_notices`** (new, `watch/tests_mail_tail.rs`, through the whole core as
  `tests_mail.rs` does, with `mail_rig` and `trust`): a delivered message `From:` the trusted fixture address with no
  `Authentication-Results`, and a second look with a message whose only `Authentication-Results` is a sender-written
  `evil.example.test; dkim=pass header.d=example.test` (not `mx.google.com`), both start **no** task and post the
  "Gmail did not authenticate" notice; the same message with Gmail's header
  (`mx.google.com; dkim=pass header.i=@example.test header.s=s1; dmarc=pass (p=NONE) header.from=example.test`)
  starts the held task. Plant: make `mail_trusted` ignore `unverified`. It must fail.
- `authentication_results_are_read_as_gmail_wrote_them` (unit, in `mail_auth.rs`): DMARC pass aligned; DKIM pass with
  `header.d` aligned (equal, subdomain); DKIM pass with an unaligned `header.d` (`evilexample.test`); `dkim=fail`;
  `dmarc=none` with no DKIM; a forged header below Gmail's (only the first counts); a missing header; an uppercase
  authserv-id; comments in parentheses. Plant: accept any `dkim=pass` regardless of domain.
- Update the existing trusted-sender tests in `watch/tests_mail.rs` (`a_trusted_senders_mail_starts_a_held_task`,
  `a_mail_with_no_address_beside_a_trusted_one_only_notices`, `history_poll_resumes_from_the_last_history_id`) so the
  trusted fixture message carries Gmail's pass header (`MsgSpec::header`). That is a fixture change to keep each
  test's meaning, not a weakening: say so per test in the report. Each must assert what it asserted before.
- `the_owners_own_draft_and_sent_mail_are_no_match`: deliver into a watched thread a message labelled `SENT` from the
  owner's address and one labelled `DRAFT`, both matching `q` and both from a listed, authenticated address: no
  `watch.met`, no task, no post; the cursor still moves. Two plants: drop the label guard (the q terms still exclude
  them, so also run the plant with the fake ignoring the q terms) and show which assertion fails for each.
- `a_quiet_watch_writes_one_row_an_hour`: N quiet looks within the hour write **no** `google.called` row; the first
  look past `QUIET_ROW_MS` (drive the entry's clock or pass `now`, as `watch_finish` does with `kernel.now_ms()`;
  never sleep) writes one row with `quiet_looks: N` and the summed `calls`; a match after quiet looks writes the tally
  row then its own rows in one frame. Change `a_poll_with_no_new_mail_makes_one_call`'s frame assertion from
  `frames + 1` to `frames` (a quiet look now writes **no** frame) and its row check to the tally row: say in the
  report it is a stricter assertion of the new behaviour. Plant: write quiet rows as before; the key assertion fails.
- `a_watch_that_ends_writes_its_tally`: quiet looks, then the watch is cancelled (or meets): the tally row is written.
  Plant: drop the flush on removal.
- `the_discord_send_card_bolds_only_recipient_flags` (in `preview.rs`'s tests beside
  `the_discord_send_card_shows_the_preview_and_flags_new_recipients`): a subject `Plan (new)` and a body line
  `  see the (new) doc` stay plain; `To: … (new)` is bold. Plant: restore the replace on every line.
- `a_same_name_same_size_attachment_swap_is_refused` (`google/tests_mail.rs`): a draft with `ledger.csv`
  (`a,b\n1,2\n`), the card shown, then the fake swaps the bytes for `a,b\n9,9\n` (same name, type, size: add a fake
  helper `replace_draft_file(id, name, bytes)`): the send is refused as changed and nothing is sent. Plant: drop the
  hash from the digest's file entry. Also keep `an_unchanged_draft_with_an_attachment_sends` and
  `an_html_only_change_after_the_card_is_refused` green (they are the HTML-part and unchanged-send confirmations the
  issue asks for), and add `a_draft_attachment_read_failure_refuses_the_send` (the fake answers 500 to the
  attachment read: the send is refused, nothing sent).

## Not in this row
- **The live check on the owner's account (the rest of u4dvr)** is the maintainer's; put these steps, verbatim, in
  your report's "for the maintainer" section: with the daemon on the branch's build, in Gmail save a draft to an
  invented test address with a small attachment and an HTML body; ask the agent to read the draft and send it; expect
  exactly one send card listing the attachment and, on approval, one send (Gmail's Sent shows it once, the ledger shows
  one `gmail.users.drafts.send` row with `outcome: ok`). Then save a second such draft, let the card show, replace the
  attachment in Gmail with another file of the same name and size, approve: expect a refusal naming the change and no
  send. Then: a mail watch with `then: task` and a `trusted_senders` address you control; mail from it starts a held
  task; the same From sent through a server without DKIM for that domain only notices.
- The `trusted_senders` comment in `crates/theseus-core/config/theseus.example.toml` (it should say "mail Gmail
  authenticated from these addresses"): other rows edit that template now; name the line in the report and stop.
- `crates/theseus-core/src/google/{private,tests_config,tests_private}.rs`: another lane is changing them. Do not
  touch them.
- Docs (`docs/status.md`, the spec, `docs/design/`): say what should change.
- Any change to how a mail watch decides a match beyond the two drafts/sent guards, or to `trusted_senders` config.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- **On, with a named rollback.** Each behaviour ships on. This row adds no switch (see item 1); the rollback of each
  item is reverting its commit, so keep one item per commit.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`), nothing on the path before a request leaves.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. The test changes this
  task names (fixture headers, the stricter frame count) are the only edits to existing tests. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must
  be empty, or list only those named edits with your one-line reason each.
- **Store format: 48** on your base (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row stores
  nothing new (the tally and `unverified` are memory only); do not bump it. A new ledger fact field on `google.called`
  (`quiet_looks`, `calls`) is fine.
- **Line ceilings** (`scripts/long-files.txt`): none of your files is listed; `google/mail.rs` is 1,436 lines and
  `crates/theseus-discord/src/render.rs` (listed, nearly full) is not one of yours. Put new code in the two new
  modules. Never raise a ceiling.
- No new dependency: Cargo.lock gains no third-party package (`sha2` is already used by `google/mail.rs`).
- **Secrets and names.** No address, subject or content in any `google.called` row (the tally row too: assert it in a
  test as `a_poll_with_no_new_mail_makes_one_call` does). No person's name anywhere in the repo, in commit messages or
  in the report: write "the owner". Fixture addresses are invented `@example.test` ones.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core`, `crates/theseus-google`,
  `crates/theseus-discord`). Run `cargo test -p theseus-core watch::`, `cargo test -p theseus-core google::`,
  `cargo test -p theseus-google`, `cargo test -p theseus-discord render::preview`, then `cargo clippy --workspace
  --all-targets -- -D warnings` and `cargo fmt --check`. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit): what
changed per item and per commit; each test and its plant (which assertion failed); the existing tests you edited and
why each still asserts what it did; the maintainer's live steps above; the keel guard's output; the store format
(unchanged, 48); follow-ups (at least: the template comment line).
Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
