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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-ops-cal`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: ops-cal, the Operations page's calendars: `[google] calendars`, a calendar cache, `calendar.window`, the sensitive mark, and the morning brief over every calendar (theseus-jid7t.1)

Branch: `cloud/20261023-ops-cal`. Every commit's subject carries `theseus-jid7t.1`. Deadline for the report: 5 hours
after you start.

**Why.** The owner wants the cockpit's Agenda to grow into an Operations page that shows two Google calendars beside
Theseus's own marks: the owner's personal calendar, and a work calendar shared into the same Google account. The work
calendar is **sensitive**: its events must never reach a model that is not of US origin (so never one of China
origin), never reach a shared place, and never appear in a ledger row. This row is the daemon half: the config, the
reads, the cache, a read-only protocol method for the page, the mark that keeps the content where it belongs, and the
morning brief over every calendar. The page that draws the calendars (the Timeline lens) is a later row; it consumes
`calendar.window` exactly as this row specifies it.

**Risk: medium.** This row touches the data floor. Your report ends with a data-routing section a reviewer reads line
by line (see "Your report").

**Prerequisite: base `origin/main` at `2d1d8d6f`.** Your branch joins the lane `integ/ops`. Run the keel guard against
`origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `ops-cal` | theseus-jid7t.1 | `[google] calendars`; check reachability; a calendar cache (events over a window, 5 min, syncToken, freebusy fallback); `calendar.window` (owner on a private connection only); the `calendar.changed` push; the sensitive mark (a META level; the floor = US origin only; it follows recall); `calendar.events` by label and `calendars: "all"`; the morning brief over every calendar | core `config/google.rs` (+ a new `config/google_calendars.rs` if it grows), `config/tests_google.rs`, the config template's `# [google]` block and its template test; core `google/calendar.rs`, a new `google/calendar_cache.rs`, `google/private.rs`, `google/tests_calendar.rs` + a new `google/tests_calendar_cache.rs` and `google/tests_sensitive.rs`; `initiative/morning_google.rs` + `google/tests_morning.rs`; `data_gate.rs` (the chokepoint's level); a new `rpc/calendar.rs` + one dispatch arm in `rpc/server.rs`; the theseusd `check` line for Google; `crates/theseus-google/src/fake/calendar.rs` (and `fake.rs` if a route needs it); protocol: a new `calendar.rs` module, the method and notification constants in `lib.rs` (at most 4 lines), `ts.rs`, `tests/wire.rs` + `tests/wire/calendar_window.json`; `cockpit/src/protocol.gen/` (regenerated only); core `AGENTS.md` (the Google section), `docs/` Google page if one documents `[google]` | L |

## What exists (at `2d1d8d6f`; search by name)
- **Config.** `crates/theseus-core/src/config/google.rs`: `GoogleConfig` (near :16, `deny_unknown_fields`: `account`,
  `services`, `max_sends_per_day`, `trusted_senders`, `credentials`, `endpoint`), `Default`, `service_list`, and the
  loader check near :123. Tests: `config/tests_google.rs` (the template's `# [google]` block is un-commented and
  loaded near :135). The template is `crates/theseus-core/config/theseus.example.toml`.
  `config/calendar.rs` is `[discord.calendar]`, a different thing (the agenda mirrored into a guild's Events tab):
  leave it alone.
- **The calendar tool.** `crates/theseus-core/src/google/calendar.rs`: `EVENTS = "calendar.events"`, `PUT`, `tools`,
  `listing` (near :295: the `calendarId` param defaults to `"primary"` near :311), `listed` (near :335: the result's
  `meta` names the calendar and the event ids), `render`, `call_link`, `organized_by`, `today` (near :187, the brief's
  request) and `briefs` (near :196).
- **Every Google call** goes through `Google::call` (`google/call.rs`), which writes the `google.called` ledger row
  (method and status; no content, subject, address or token). `google/mod.rs` holds `Google`, `finish` (which sets
  `"private": true` on every result's `meta`) and `resolve`.
- **The private mark.** `google/private.rs` (read its module doc first): `KEY = "private"`, `marked`, `meta_key`,
  `record` (the META record a frame writes when a result is marked), `session`, `session_on`, `lineage`, `any_session`,
  `any_node`, `recalled_from`, `followed` (recall carries the mark to the asker), `inherited` (a task inherits its
  parent's), `bound`, `scoped`, `within`, `in_scope` (the task-local the chokepoint reads).
- **The floors.** Two of them:
  - the route step, `turn/data_step.rs`: `private_session` (near :70) makes a Google-read session run **only on a
    profile of US origin** (`uncleared_profile`, `ModelOrigin::cleared_for_sensitive`), as a sensitive name does;
  - the provider chokepoint, `data_gate.rs`: it refuses a **restricted** origin (`cn`, `unknown`) for a call made
    under `private::in_scope()`. Background steps (keep-warm, the memory pass, learning, the people judge, cards,
    naming, replays, the census) reach a provider only through this chokepoint (`private::bound`).
  So at the base a private session's *turns* are US-only, but its *background* calls only exclude restricted origins:
  a model of EU origin passes the chokepoint. Confirm this by reading before you build on it.
- `catalog.rs`: `ModelOrigin` (`us`, `cn`, `eu`, `unknown`, ...), `cleared_for_sensitive` (near :118 and :167),
  `origin_of`. `Config::profile_cleared_for_sensitive` and `Config::profile_origin` (config.rs near :2024).
- **The morning brief.** `initiative/morning_google.rs` (module doc near :1-18): today's events, unread mail and
  drafts, each through `Google::call`; the note's model call runs inside `private::scoped(true, ...)`.
- **The fake Google.** `crates/theseus-google/src/fake.rs` (a loopback Google with a route table and failure switches)
  and `fake/calendar.rs` (the calendar stubs; `calendarList` is near fake.rs :412). `crates/theseus-google/src/methods.rs`
  already lists `calendar.freebusy.query` (near :389). The core's Google tests build a `GoogleConfig` with
  `endpoint: Some(fake.url())` (`google/tests.rs` near :71).
- **Who may read.** `rpc/answer.rs` `may_answer` (near :149) asks `places::owner_in_private(who, place_rule, cfg)`:
  whether a connection is the owner in a private place. `calendar.window` uses the same check.
- **Shared places.** `places.rs` (the place rule) and `rpc/publish.rs` (`place.publish`): private recall is already
  kept out of shared places.
- **Protocol.** The method table is in `crates/theseus-protocol/src/lib.rs` (the `GOOGLE_*` and `DIGEST_*` lines are
  your neighbours; the notifications' table is near :482). The Google wire types are in `google.rs` and
  `digest_google.rs`. Dispatch is the `match` in `crates/theseus-core/src/rpc/server.rs` (near :363 for a model arm).
  `cockpit/src/protocol.gen/` is written by theseus-protocol's test (`crates/theseus-protocol/AGENTS.md`, near :250).

## What to build
1. **`[google] calendars`.** A list whose entries are a calendar id string or a table `{ id, label?, color?,
   sensitive? }`. Default `["primary"]`, so a config without the key behaves exactly as today. Validation at load:
   ids unique, `primary` at most once, a `color` is `#rrggbb`. `label` defaults to the calendar's own summary (read
   once from `calendarList`, cached) and otherwise to the id. `color` defaults to a fixed palette of four materials
   that avoids the cockpit's signal tones (brass `#c9a24a`, verdigris `#5fa08d`, then two more of your choosing, named
   in a comment). `sensitive` defaults to false. Add the key to the template's `# [google]` block with an invented
   `example.test` id and a comment, and keep the template test green. Put the type in a new
   `config/google_calendars.rs` if `google.rs` would pass ~300 lines.
2. **`theseusd check` reachability.** For each configured calendar, one `calendarList.get` (or `events.list` with
   `maxResults=1`) through `Google::call`: `ok`, `not_found` (404: not shared, or a wrong id), `busy_only` (events 403
   and `freebusy.query` answers), `forbidden`, `auth_lapsed`. One line per calendar, by label, never by title of any
   event. Find where `check` reports Google today and add beside it.
3. **The calendar cache**, a new `google/calendar_cache.rs`. Per configured calendar: `events.list` (`singleEvents`,
   `orderBy=startTime`) over the window asked for, widened by one week each side; kept 5 minutes; then refreshed
   incrementally with Google's `syncToken` (a 410 from a stale token drops the token and reads the window again); on
   a 403/404 for events, `freebusy.query` gives busy blocks (`busy: true`, no title). Every call through
   `Google::call`. The cache lives in memory only: nothing about an event is written to the store or the ledger.
   Bound it (the windows kept and the events per calendar), and say the bounds in its module doc.
4. **`calendar.window { from_ms, to_ms }` → `{ calendars: [CalendarWindow] }`**, a new method, types in a new
   `crates/theseus-protocol/src/calendar.rs`. Each `CalendarWindow`: `id`, `label`, `color`, `sensitive`, `status`
   (`ok` · `busy_only` · `not_found` · `forbidden` · `auth_lapsed` · `off`), `detail` (the daemon's words for a non-ok
   status, with its one fix, e.g. `theseus google auth`), `events: [{ key, start_ms, end_ms, all_day, title?,
   location?, call?, attendees, self_response?, organizer_self, html_link, busy }]` (`attendees` is a count). `key` is
   stable across reads (calendar id + event id + start, hashed is fine) and opaque. With no `[google]`, every
   calendar is `off` and nothing is read. **Who may call it:** only the owner on a private connection (`may_answer`'s
   check); anything else gets a refusal in the daemon's words and no data. It is not a model tool: nothing it returns
   enters any session's context. Keep the window bounded (refuse a span over 62 days).
5. **The `calendar.changed` notification**, with no content (only that the window moved, and which calendar label
   changed), sent when a refresh finds a change. The page reads the window again on it. Nothing is sent while no
   connection has read a window since the daemon started.
6. **The sensitive mark.** A read of a calendar with `sensitive = true` marks the reading session **sensitive**: the
   result's `meta` carries `"sensitive": true` beside `"private": true`, and `private::record` writes a second META
   record under a new key (for example `sensitive:<session>`; a new META key needs no format bump), in the same
   frame as the private mark. Then:
   - `private.rs` gains the level: `sensitive_session`, `sensitive_lineage` (a task under a sensitive session is
     sensitive) and recall's `followed` carries the sensitive level as it carries the private one (a session that
     recalls a sensitive node becomes sensitive). Keep one function the rest of the code asks (for example
     `private::level(store, cfg, session) -> Level { Open, Private, Sensitive }`); the next row
     (ops-verbs, in parallel) reads a session's mark for `SessionInfo.data`, and the integrator wires its reader to
     this function at the join, so make it `pub(crate)` and document it.
   - **The floor:** a sensitive session's turns run only on a profile cleared for sensitive (the route step already
     does this for any private session: confirm it, and pin it with a test for the sensitive case). The **chokepoint**
     `data_gate.rs` gains the level: a call made under the sensitive level is refused unless its model is
     `cleared_for_sensitive` (US only). The task-local carries the level, not a bool; `scoped`/`within`/`bound`
     callers that pass `true` today keep their meaning (private), and the session-reading helpers pass the level.
     Fail closed: nothing lifts it.
   - **Shared places:** a sensitive session's content is refused by `place.publish` and by the Discord binding in any
     non-private place. Sensitive is a subset of private, so the place rule should already cover it: write the test
     that pins it, and fix only if it fails.
   - The level is never shown as an event's title anywhere it is logged: the refusal names the calendar's label at
     most.
7. **The model tool `calendar.events`** takes `calendar: "<label>"` as well as an id (label match is
   case-insensitive; an unknown label is refused with the configured labels listed) and `calendars: "all"` (every
   configured calendar, each event line prefixed with its label; a sensitive calendar's events mark the session as
   in 6). Its description says so in one sentence.
8. **The morning brief reads every configured calendar.** Each brief line is labelled by calendar. If any calendar
   in the brief is sensitive, the brief's model call runs at the sensitive level (US only), and the brief's session is
   marked sensitive. A calendar that fails says `<label> unreachable` and never blocks the note, as today.
9. **Docs.** Core `AGENTS.md`'s Google section: the calendars key, the cache, `calendar.window` and who may read it,
   the sensitive level and both floors, in a short paragraph. If `docs/` has a Google page that lists `[google]`'s
   keys, add the key there with an `example.test` id.

## Tests (red first: write each, see it fail on the base code, then fix; each behaviour change gets a planted revert)
Build a fake Google with **two calendars** under invented ids (for example `personal.cal.example.test` and
`work.cal.example.test`; Google's own ids look like mail addresses, and yours may too as long as the domain is
`example.test`; the second is `sensitive = true`) and the failure switches you need:
- **config:** a string entry, a table entry, the default `["primary"]`, a duplicate id refused, a second `primary`
  refused, a bad colour refused; the template's block loads.
- **check:** one line per calendar for each of ok, 404 (`not_found`), 403 with freebusy (`busy_only`), lapsed consent
  (`auth_lapsed`).
- **cache:** a window read twice within 5 minutes makes one `events.list`; after 5 minutes the refresh sends the
  `syncToken`; a 410 re-reads; a 403 gives busy blocks with no title; `calendar.changed` is sent once on a change and
  not on a refresh that finds none (paused clock).
- **`calendar.window`:** the two calendars with their statuses and colours; refused on a connection that is not the
  owner in a private place (the refusal carries no event data); `off` with no `[google]`; a span over 62 days refused.
- **the sensitive floor:** after a read of the sensitive calendar, the session's next turn refuses a profile of China
  origin and one of EU origin, and runs on a US one; a background call for that session (through `private::bound`)
  is refused for an EU model at the chokepoint (red on the base: the base lets EU through); a task under that session
  is sensitive; a session that recalls a node of the sensitive session becomes sensitive; a read of the personal
  calendar marks private, not sensitive.
- **shared places:** a sensitive session's content refused by `place.publish` to a shared place, and by the Discord
  binding in a non-private place.
- **no titles in the ledger:** read both calendars (events with distinctive invented titles, places and call links),
  run the brief, then scan every ledger row's JSON: none of the titles, places, attendee addresses or call links
  appears.
- **the tool:** `calendar.events` by label, by id, `calendars: "all"` (lines labelled), an unknown label refused.
- **the brief:** labels by calendar; a sensitive calendar makes the brief's call US-only (a CN and an EU stand-in
  refused, a US one used) and marks its session; one unreachable calendar leaves its line out.
- **wire:** `tests/wire/calendar_window.json` round-trips; the cockpit types regenerate.
- Run `cargo nextest run -p theseus-google`, `-p theseus-core google`, `-p theseus-core data_gate`, `-p theseus-core
  config`, `-p theseus-protocol`, then the gate.

## Not in this row
- The cockpit's pages and components: the Timeline lens that draws the calendars, redact mode, and the screenshot
  scripts are a later row (ops-timeline). Only `cockpit/src/protocol.gen/` changes here, by regeneration.
- `session.profile`, `task.rehome`, `task.reschedule`, `SessionInfo.data`, `WorkView.data`, `ProfileInfo.origin`:
  ops-verbs, in parallel. Do not add those fields; expose the `level` function they will read.
- Writing to Google Calendar beyond what `calendar.put` does today. New Discord messages of any kind; the Discord DM
  task flow (`crates/theseus-discord/src/golden/dm_flow.txt`) stays byte-identical.
- `[privacy] sensitive_names` and its floor: unchanged.
- The owner's real config: you cannot reach it. Ids and addresses in code, tests and docs are invented, under
  `example.test`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- **Data first.** Calendar content (titles, places, attendees, call links, descriptions) goes only to: the model tool's
  result (marked), the brief's model call (at the level), and `calendar.window`'s answer to the owner. Nowhere else:
  not the ledger, not a log line, not telemetry, not a journal line, not a store record. If a path you find would
  carry it elsewhere, stop and report it.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Protocol `lib.rs`:** at most 4 added lines (the `CALENDAR_WINDOW` method line, the `CALENDAR_CHANGED` notification
  line, the module line, one doc line). Everything else in `calendar.rs`.
- **Dependencies.** None new. **Store format** 49: a new META key needs no bump; say in the report if you found you
  needed one.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". No real calendar id, address or domain: only
  `example.test`.
- Read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (Google, the data floor, the place rule),
  `crates/theseus-google/AGENTS.md` if present, and `crates/theseus-protocol/AGENTS.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-9), with the file and function for each;
- what you found about the two floors at the base (the route step and the chokepoint), confirmed by reading, and what
  changed;
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output, the store format, and the `lib.rs` lines you added;
- the `level` function's name and signature, for the integrator to wire ops-verbs' `data` field to;
- a `docs/status.md` sentence for the integrator;
- follow-ups;
- **last, a section "Data-routing review"** for the reviewer (a Claude model only reviews it): every path calendar
  content can take, from the Google response to each consumer, with file and function; for each, the floor that
  guards it and the test that pins it; every place you checked it does NOT go (ledger, logs, telemetry, store,
  journal, notifications); and anything you were unsure of.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
