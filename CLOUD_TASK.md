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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail2-mailauth`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from the integration branch `integ/tail` at `cb19957a` (2026-10-21: `main` `6918e3c1` plus the patch1 tail rows tail-hold, tail-mail, job-wait-status, tail-discord, tail-onto); that branch, not `main`, is your base. Where the
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
## Your task: tail2-mailauth, DMARC alignment by the public suffix list, and the docs four merged rows left behind (theseus-jt4nr, theseus-by0vj, theseus-l6u2q, theseus-1qexl)

Branch: `cloud/20261021-tail2-mailauth`. Every commit's subject carries the Beads id of what it changes (`theseus-jt4nr`,
`theseus-by0vj`, `theseus-l6u2q`, `theseus-1qexl`). Deadline for the report: 3 hours after you start.

**Why.** Four follow-ups that the reviews of rows already merged into the integration branch `integ/tail` left open.
One is a real gap in a security check. The other three are docs and one small refactor.
- **DKIM alignment by dot-suffix is wrong in both directions (theseus-jt4nr).** The tail-mail row (theseus-g4lit) made
  a mail watch start a task only when Gmail itself authenticated the sender (`watch/mail_auth.rs`, `authenticated`). A
  DKIM pass counts when its `header.d` is *aligned* with the From's domain, and `aligned` does this by dot-suffix:
  equal, or one a subdomain of the other. That is not DMARC's relaxed alignment, which compares the two
  *organizational domains* (the registrable domain under the public suffix list). The result is wrong both ways:
  - **Too loose (the security hole).** A pass for a public suffix aligns with every domain under it. For example,
    `header.d=co.uk` aligns with `shop.example.co.uk`, and `header.d=github.io` aligns with `owner.github.io`.
  - **Too strict.** Two hosts of one organization do not align: `header.d=news.example.test` against a From at
    `shop.example.test` is refused today, although DMARC passes it.
- **The trusted_senders comment still describes the old rule (theseus-by0vj).** The template's `trusted_senders` line
  says the listed senders' mail "may start a task". Since g4lit, that mail starts a task only when Gmail authenticated
  the sender (DMARC, or DKIM aligned with the From), and otherwise it only notices. `docs/status.md` does not say this
  anywhere.
- **The job_wait status line is undocumented on the status page, and its post runs on the async runtime
  (theseus-l6u2q).** The job-wait-status row (theseus-5d204) posts one status line to the session's place at each beat
  of a silent `job_wait`. Its crate AGENTS.md paragraph exists (search `job_wait_status` in
  `crates/theseus-core/AGENTS.md`), but `docs/status.md` has no line for it. The review also left a LOW item:
  `wait_status::post` reads the disk and writes a frame. It runs on the async runtime right after a
  `theseus_store::blocking(|| clock.beat(..))`, when it belongs in that same blocking closure.
- **The effort-on-evidence row's docs (theseus-1qexl, repo part only).** The row (theseus-53ujg.3, commit `add7062b`)
  added three things in code and module comments that the crate AGENTS.md does not yet name:
  - a routed profile's own effort is its floor, and the row says `floored`;
  - a checkpoint can raise effort without moving, and the sticky rule never holds that raise;
  - an ineligible turn's checkpoint state stays `Off`.
  The narration for a turn checkpoint's move (`fact/route.rs`) may also say "the turn moves" where the move outlives
  the turn. `docs/status.md` has no line for the floor or the recall scorer's abstention fix (theseus-53ujg.7).

**Risk: low.** jt4nr is one pure function behind a fail-closed check. Its only behaviour changes are a public-suffix
pass that now refuses and a same-organization pass that now admits. Both are what DMARC means. The rest is docs and
one call moved into a closure.

**Prerequisite: base `origin/integ/tail` at `cb19957a`** (not `main`). `watch/mail_auth.rs` and
`toolrun/jobs_tools/wait_status.rs` exist only there: it is `main` 6918e3c1 plus the merged rows tail-hold, tail-mail,
job-wait-status, tail-discord and tail-onto. Your branch joins back into `integ/tail`. Run the keel guard against
`origin/integ/tail`, not `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail2-mailauth` | theseus-jt4nr, theseus-by0vj, theseus-l6u2q, theseus-1qexl | organizational-domain alignment in `mail_auth.rs`; the template's `trusted_senders` comment; `wait_status::post` inside the beat's blocking closure; AGENTS.md effort-floor and checkpoint-effort text; one narration word if it is wrong; four `docs/status.md` lines | `crates/theseus-core/src/watch/mail_auth.rs` (+ its tests), `crates/theseus-core/Cargo.toml` and `Cargo.lock` (only if you take the `psl` crate), `crates/theseus-core/config/theseus.example.toml` (the `trusted_senders` comment only), `crates/theseus-core/src/toolrun/jobs_tools.rs` (the beat's call site only), `crates/theseus-core/src/toolrun/jobs_tools/wait_status.rs` (doc comment only), `crates/theseus-core/src/fact/route.rs` (one narration string, if needed), `crates/theseus-core/AGENTS.md` (the named paragraphs), `docs/status.md` (one dated entry) | S |

## What exists (lines at `cb19957a`; search by name, a line may have moved)
- **The check.** `crates/theseus-core/src/watch/mail_auth.rs` (168 lines):
  - `authenticated(payload, from)` takes the From's domain (`mime::address_of`, the part after `@`).
  - It reads only the **first** `Authentication-Results` header, whose authserv-id must be `mx.google.com`, with
    comments removed (`without_comments`).
  - A clause passes when `dmarc=pass` has `header.from` equal to the domain, or `dkim=pass` has `header.d` (else
    `header.i`'s domain) that `aligned(&d, &domain)`.
  - `aligned(a, b)` (near :64) is equal, or one a subdomain of the other at a dot boundary.
  - Tests: `tests::authentication_results_are_read_as_gmail_wrote_them`. It checks equal and subdomain-either-way
    passes, `header.i`, the bare suffix `evilexample.test` refused, fail/none/foreign DMARC/SPF-only refused, only the
    first header counting, and case and comments.
  - The one caller is `watch/mail.rs` near :167 (`authenticated: mail_auth::authenticated(&msg["payload"], &from)`).
    It is not yours: leave `mail.rs` alone.
- **Dependencies.** `Cargo.lock` has no public-suffix crate. `deny.toml` `[licenses] allow` includes MIT and
  Apache-2.0. `crates/theseus-core/Cargo.toml` `[dependencies]` is where one would go.
- **The template.** `crates/theseus-core/config/theseus.example.toml` near :590, in the `[google]` block:
  `# trusted_senders = [<one example address>]   # whose mail may start a task through a mail watch (then: task); empty: a mail watch only notices`.
  `crates/theseus-core/src/config/tests_google.rs` near :149 asserts the template's value once uncommented. Keep the
  value byte for byte and change only the words after the second `#`.
- **The status line.** `crates/theseus-core/src/toolrun/jobs_tools.rs` near :484 has the following:
  ```rust
  match theseus_store::blocking(|| clock.beat(&core, &f, s, asked, reads)) {
      Some(out) => {
          wait_status::post(&core, &f, &clock, s);
          return Ok((out, None, None));
      }
      None => held = true,
  }
  ```
  `wait_status::post` (in `toolrun/jobs_tools/wait_status.rs`, near :25; its doc says "Reads the disk and writes a
  frame") is sync: `last_line` reads the job's file, and `core.outbox.post_for` writes a frame. Its tests are in
  `crates/theseus-core/src/tests_wait_beat.rs`: `a_silent_jobs_beat_posts_one_status_line_to_its_place`,
  `no_status_line_without_a_beat` and `the_status_line_is_scrubbed`. `waits::LOOK` is the backstop's look, pinned at
  60 s or less by a test.
- **The effort docs.**
  - `crates/theseus-core/AGENTS.md` near :1570-1591 holds the checkpoint paragraph, "**A running task's
    checkpoints**", which ends "Not covered: a task a model made with `task.create`".
  - Near :1592 comes "**The effort**", then near :1599 "**A turn's effort keeps the cache**".
  - Where the code says it, read these: `crates/theseus-core/src/routing/effort.rs` module doc ("A routed profile's own
    effort is its floor", `EffortReason::Floored` → `floored`); `crates/theseus-core/src/turn/checkpoint_step.rs`
    module doc ("Effort without a move"); commit `add7062b`'s message (the `Off` state kept: `checkpoint_apply` no longer
    puts `Off` back as `Unread`).
  - The restart-adopted job handles (theseus-53ujg.8) are already in AGENTS.md (near :715, "A job a restart found alive
    gets its short id too"), and the recall scorer's abstention is in `bench/recall/README.md` (near :262). Both parts
    are done: say so in the report.
- **The narration.** `crates/theseus-core/src/fact/route.rs` `narrate` near :118-128 has a switch with a checkpoint:
  - a source starting `turn checkpoint` gives "Routing (turn checkpoint: …): the turn moves from A to B.";
  - any other source gives "the task moves".
  AGENTS.md says that a turn checkpoint's move "persists in the session's `routed` as a route switch does, so the next
  message starts on the moved profile". Check this against `turn/checkpoint_step.rs`. If the move outlives the turn,
  the honest word is "the conversation moves". If it does not, the string is right: say so and change nothing.
- **The status page.** `docs/status.md` has dated entries, newest first, near :515-540. The newest is "2026-10-21: a
  reach for the owner's personal details is held in a shared place". Nothing on the page names trusted senders, the
  job_wait status line, the effort floor or the recall scorer's abstention.

## What to build
1. **theseus-jt4nr: alignment by organizational domain.** Replace `aligned`'s rule as follows. The two domains align
   when their organizational domains (the registrable domain: one label under the longest public suffix that matches)
   are equal. When either domain has no organizational domain (it is itself a public suffix, or it is empty), they do
   not align: fail closed. Choose one of these:
   - **(a) the `psl` crate** (MIT/Apache-2.0; the list is compiled in; no runtime file; `psl::domain_str` or the
     equivalent). Take it only if `cargo deny check` passes with it and it brings at most one or two small transitive
     crates. In the report, name its version, its license and what `cargo deny check` printed.
   - **(b) no dependency.** Write a small compiled-in suffix rule with a documented limit (for example, the multi-label
     suffixes that matter for mail: `co.uk`, `com.au`, …, plus the private-section hosts like `github.io`). Take this
     only if (a) fails `cargo deny`. It is weaker, so the module doc must say exactly what it does not cover.

   Domains with an unknown top-level domain, such as the `.test` names the tests use, follow the list's default rule
   (`*`): the organizational domain of `mail.example.test` is `example.test`. Lower-case both inputs and drop a
   trailing dot before comparing. Update the module doc's sentence about alignment so that it names the rule. `dmarc`'s
   exact `header.from` check stays as it is: Gmail already evaluated DMARC.
2. **theseus-by0vj: the template comment.** Keep the `trusted_senders = [...]` value byte for byte. Change the
   trailing words to say that mail from a listed sender starts a task only when Gmail authenticated the sender (DMARC
   pass, or DKIM aligned with the From); otherwise, and for anyone else, the watch only notices; empty means a mail
   watch only notices. If that does not fit on one line within the block's style, use a second `#` line under it.
   Leave the spec's Part III to the maintainer.
3. **theseus-l6u2q: post inside the blocking closure.** Move `wait_status::post` into the closure that already runs
   `clock.beat`, so that the disk read and the frame write leave the async runtime. For example:
   `theseus_store::blocking(|| clock.beat(..).inspect(|_| wait_status::post(&core, &f, &clock, s)))`.
   The order is unchanged: the beat's facts first, then the post. A `None` beat still posts nothing. Change only
   that call site and, if it then says something false, `post`'s doc comment. This is a pure refactor.
4. **theseus-1qexl (repo part): AGENTS.md and one word.**
   - In "**A running task's checkpoints**", add to its end (before "Tests:") the effort without a move: a verdict that
     keeps the profile and asks more effort raises it by `route_effort`'s rule, with no slot. The sticky rule never
     holds it. A per-message-effort model records it as not applied. A checkpoint never lowers effort. An ineligible
     turn's state stays `Off`. Name `tests_checkpoint_effort.rs`.
   - Add the floor as one new paragraph, "**A routed profile's effort floor**" (theseus-53ujg.3), inserted directly
     **before** the "**The effort**" line. It says: on a profile a routing class chose (not the session's base), the
     turn runs at no less than the profile's own `effort`; this applies after the 0.70 gate and before
     `effort_bounds`; it is not gated; `effort_reason` is `floored` only when it raised the level; and on the base
     profile Jev may still go below.
   - **Do not edit any line from "**The effort**" through the end of "**A turn's effort keeps the cache**".** Another
     row (tail-effort-400) is rewriting those lines now, and its join must merge cleanly with yours.
   - `fact/route.rs`: fix the one narration string as found under What exists, or leave it and say why.
5. **docs/status.md: one dated entry** for all four, put directly **above** the existing "2026-10-21: a reach for the
   owner's personal details…" entry. Title it "**2026-10-21: v1 patch1 tail**", and write it in that page's plain
   prose. It covers four things:
   - mail from a trusted sender starts a task only when Gmail authenticated it (DMARC, or DKIM aligned by organizational
     domain under the public suffix list); otherwise it notices;
   - a silent `job_wait` posts one status line to the place at each beat (`[tools] job_wait_status`, on, no model call),
     and the wait's backstop looks at least once a minute;
   - a profile a routing class chose never runs below its own effort, and a checkpoint can raise effort without moving;
   - the recall scorer tells an honest abstention from a misleading one.
   Change nothing else on the page. Other lanes add entries at the same place, and the maintainer orders them at the
   join.

## Tests (each with a planted revert; the first is the key test)
- **`a_pass_for_a_public_suffix_never_aligns`** (in `mail_auth.rs`'s tests). Each case is `authenticated(&payload(..),
  from)`:
  - `mx.google.com; dkim=pass header.d=co.uk` against a From at `shop.example.co.uk` is **false**;
  - `header.d=github.io` against `site.github.io` is **false**;
  - `header.d=other.github.io` against `site.github.io` is **false** (siblings under a shared private suffix);
  - `header.d=mail.example.co.uk` against `example.co.uk` is **true**.

  Plant: restore the dot-suffix `aligned`. The first two cases must fail. Paste the failing assertion.
- `hosts_of_one_organization_align`: `header.d=news.example.test` against a From at `shop.example.test` is **true**;
  `header.d=example.test.evil.test` against `example.test` is **false**. Plant: the dot-suffix rule fails the first.
- The existing `authentication_results_are_read_as_gmail_wrote_them` passes unchanged. Do not edit its assertions. If
  one of them must change, stop and say why in the report's first section.
- If you write a hand rule (option b), add a test for each suffix family you list and a test for the default rule.
- l6u2q is a pure refactor and has no plant: say so. `cargo nextest run -p theseus-core tests_wait_beat` must pass
  (the three status-line tests above).
- 1qexl and by0vj are docs: no test. If you change the `fact/route.rs` string, update any test that asserts it
  (`git grep -n "turn moves" crates`) and say which tests changed and why. Run `cargo nextest run -p theseus-core
  config::tests_google` for the template.

## Not in this row
- theseus-rslim (the live check of a forged `Authentication-Results` mail and an in-domain trusted sender): the
  owner's, on the installed daemon.
- The `job_wait` live check (`sleep 660`, one notice near 600 s): the owner's.
- The spec's Part III items and rescoring the kept recall v1 run (theseus-1qexl's other half): the maintainer's.
- `crates/theseus-core/src/watch/mail.rs`, `google/*` (another row of this wave is changing the Google tool modules),
  the TUI, `turn.rs`, `turn/route_step.rs`, the Discord crate, `compiler/effort.rs`, `turn/effort_step.rs`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed. The key test must be shown failing on its
  planted revert.
- No new always-loaded model text and no new tool. A plain turn stays within its frame budget
  (`tests_m3::a_plain_turn_stays_within_its_frame_budget`).
- **Line ceilings** (`scripts/long-files.txt`): none of your files is listed. Each must stay at or under 2,500 lines
  (`jobs_tools.rs` is 660). Never raise a ceiling.
- **In-flight rows that join `integ/tail` after you**:
  - tail-flakes (test files only);
  - tail-herdr-cancel (`herdr_*`, the TUI, `config/cockpit.rs`, one line each in `config.rs` and `turn.rs`);
  - tail-effort-400 (`compiler/effort.rs`, `turn/effort_step.rs`, its tests, and the AGENTS.md effort paragraph near
    :1590-1606).
  Your only shared file is `crates/theseus-core/AGENTS.md`: keep to the hunks named in step 4. Do not run `cargo fmt`
  in a way that moves lines in files you were not asked to edit.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer: if a change would need one, split the code instead or leave it out and say so. Before your last
  commit run `python3 scripts/keel-guard.py --base origin/integ/tail --head HEAD --quiet` and paste its output in the
  report. It must be empty.
- **Dependencies.** At most the one public-suffix crate under step 1(a), with `cargo deny check` green. No other new
  package in `Cargo.lock` and none in any `package-lock.json`.
- **Store format** on your base is 48 (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row changes no
  stored record. If you find you must, a bump goes to 49 with the old layout's sample in `tests_layouts.rs`.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so in the report). Put the Beads id in
  each subject. End each message with the same `Co-Authored-By: Tabitha/Claude` trailer the branch's recent commits
  carry (`git log -3 --format=%B origin/integ/tail` shows it).
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture addresses use invented hosts under `example.test` or public suffixes only.
- Read `crates/theseus-core/AGENTS.md` (its mail-watch, job-wait and effort paragraphs) before you edit. Run
  `cargo nextest run -p theseus-core`, then `cargo clippy --workspace --all-targets -- -D warnings`, and (with a new
  crate) `cargo deny check` before the last commit. L1 tests need a non-root daemon with a job cgroup, and about 33 of
  them fail on this VM; this row needs none of them, so say which tests you ran.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit: it must not be empty. It holds:
- what changed per Beads id;
- the alignment rule you chose (a or b), with the crate, its version, its license and the `cargo deny` output, or the
  hand rule's stated limit;
- each test and its plant, with the failing output's line;
- what you found about the turn checkpoint's narration;
- the AGENTS.md hunks you touched, by line range;
- what could not run on the VM, with the exact commands for the maintainer;
- the keel guard's output;
- the store format (unchanged);
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
