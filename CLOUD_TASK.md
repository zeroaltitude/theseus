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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-discord`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `8260dbab` (2026-10-22: release stage 2, which landed the threads lane rounds 1-3b and
the China-origin chain rounds 1-4 on top of install #69's `29f5dcad`); that is your base. Line numbers quoted below are
at `8260dbab` but may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `29f5dcad`** (`git log --first-parent 29f5dcad..HEAD` lists them):
- `bb265d6c` + `8260dbab`: the threads lane (a place's home splits into threads, thread routing with the directory rule
  and the which-thread ask, the place digest, thread moves and growth, the protocol nodes split).
- `38743708`: the China-origin chain (the data rule's open view, the open request, the tool screen, the draft review),
  with join fixes `c0d35275` (a session's data state is store format 49) through `30d6472e`.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail3` (`e29be1ed`) and `integ/threads` round 4 (`a52cac7b`) are being joined into the next release stage.
  Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read their diff
  (`git diff origin/main origin/integ/tail3 -- <file>`). Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail4-discord, the approval card in the owner-approved plain words, and one forum per start (theseus-ono8g, theseus-p5g63)

Branch: `cloud/20261021-tail4-discord`. Every commit's subject carries the Beads id of what it changes
(`theseus-ono8g`, `theseus-p5g63`). Deadline for the report: 3 hours after you start.

**Why.**
- **theseus-ono8g (approval card).** On install #69 the Discord approval card still says (from a scratch run):

  ```
  <@user> **Approve?** task `490f10`: `fs.write` <home>/.../out/harbour.md
  create <home>/.../out/harbour.md (227 bytes): fs.write — approve (this session read external text
  (task.start quoted text, at 07:25), and a call that acts waits for approval after that (§3.9))
  ```diff ... ```
  -# expires <t:...:R> · **Approve + trust session** also lets its later calls run at their postures · you can also answer with `theseus answer`
  ```

  while the TUI's card and `theseus answer`'s card use the plain words ("wants to write `...` · risk: writes · Yes: runs
  this one call ... No: refuses it ..."). The Discord DM task flow is otherwise **locked by the owner** ("please do not
  change how it is now"), and the owner **approved this exact new layout** on 2026-10-22 ("That's approved, nice
  changes"). Build exactly this, and nothing else in the flow:

  ```
  @you ✋ **Approve?** task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)
  risk: writes a file · why it asks: the task quoted outside text at 07:25
  ```diff ... ```
  **Approve** runs this one call · **Approve + trust** also lets its next calls run as usual · **Decline** refuses it; the task is told and carries on
  -# expires <t:R> · or `theseus answer 490f10 approve`
  [Approve] [Approve + trust session] [Decline]
  ```

  Rules (the owner's): the first line is who (the task's title) + a plain verb + the target, no tool names like
  `fs.write`; one short risk line and one short why line, never a spec reference (no "§3.9" or any "§"); one line saying
  what each button does (the trust clause only when the third button shows); home paths shown as `~`. Keep the card's
  placement (the task's own forum post), the @mention, the diff, the expiry and the buttons. The words come from the
  core's plain words (`ConfirmRequest.plain`, built by `crates/theseus-core/src/needs_you/plain.rs`), so Discord, the
  TUI and `theseus answer` agree.
- **theseus-p5g63 (two forums in one start).** On a scratch guild with no forum and no task home, one daemon start made
  **two** forums one second apart (the fake Discord's log: two `POST /guilds/<g>/channels` -> forum, at 14:20:24.114
  and 14:20:25.131); every later post went to the second; nothing used the first. Every task start then logged
  `WARN discord error=task forum tags: ... 404 Unknown Channel` (ledger `discord.error` `{"op":"task forum tags"}`), and
  no post got a tag. Whether the 404 is the fake's (it may not register a bot-made forum where its `GET`/`PATCH
  /channels/<forum>` route looks) or the daemon's (the tags call aimed at the abandoned first forum) is not settled;
  the double creation is the daemon's own.

**Risk: low-medium.** One card's words (owner-approved), and a start-up race.

**Prerequisite: base `origin/main` at `8260dbab`.** Your branch joins the new lane `integ/tail4`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-discord` | theseus-ono8g, theseus-p5g63 | the approval card's plain words + a golden of it; one forum per start, tags on the forum in use | a new `crates/theseus-discord/src/render/approval.rs` (the card's words), `crates/theseus-discord/src/render.rs` (`card`'s approval branch calls it: lines out, not in), their tests, a new golden test file for the card; `crates/theseus-core/src/needs_you/plain.rs` only if a field the card needs is missing (say which); `crates/theseus-discord/src/runtime/default_forum.rs`, `runtime/forum.rs` (`tag_ids`/`read_tags`), their tests; `crates/theseus-sim/src/fake_discord/{forums,channels}.rs` only if the 404 is the fake's; `crates/theseus-discord/AGENTS.md` | M |

## What exists (at `8260dbab`; search by name)
- **The card.** `crates/theseus-discord/src/render.rs` `pub fn card(req, route, elsewhere) -> CardText` (near :806): a
  budget question, a change card and an owner ask each return early; the approval branch builds `line` = `{task}`
  + `` `tool` `` + `summarize(..)`, content = `ASK` (`"**Approve?** "`, near :127) or `FLOOR_ASK` + line, then
  `clip_middle(&req.reason, 300)`, then `preview::hunk(req)` (the diff), then `-# {asked_for}expires <t:..:R>{trust}{also}`.
  `CardText.line` is what the settled card keeps. Called from `courier.rs` near :1068 and `render/board.rs` near :534.
  `render/card_v2.rs` is the components card ("kept as they are"); read it and change it only if it renders the same
  words. The buttons: `Buttons::Card` (AGENTS.md near :101).
- **The plain words.** `crates/theseus-core/src/needs_you/plain.rs`: `Gloss { state, who, own_task, model_why }`,
  `plain(req, g) -> PlainWords { who, what, why, held, risk, yes, no }`, `held_of` (the "why it asks" words: floor,
  external text, posture), `risk_of`, `object_of(path, g)` (friendly object, home as `~`). `ConfirmRequest.plain`
  carries them on the wire. The TUI's card and `theseus answer` render them (`needs_you.rs` near :283:
  `"● {head}: {} wants to {}\nrisk: {}{why}\n{} · {}"`). Note the approved card's why line ("the task quoted outside
  text at 07:25") is the plain `held` words with the external text's source and time: compare with
  `req.external_text` and reuse whatever already names it.
- **Tests that assert the old words:** `render.rs` near :1672 and :2164 (`starts_with("**Approve?** `proc.run` cargo
  test\nrun cargo test")`), `courier.rs` near :2169, `render/card_v2.rs` near :97, and any Discord test that matches
  "Approve + trust session also lets" or "§". The owner approved the new words, so these assertions change to the new
  card: list each old and new assertion in the report. Change no other assertion.
- **The forum.** `crates/theseus-discord/src/runtime/default_forum.rs`: `ready(shared)` spawns `ensure_forum` at each
  gateway READY (and `task_home::ready`); `ensure_forum` holds `d.gate` and calls `find_forum` (recorded in META
  `RECORD_KEY`, else a configured id, else one of that name, else `make_forum`), then `record(..)`; the `Why::Other`
  arm retries after `AGAIN`. The task home: `runtime/task_home.rs` (`ensure_task_home`). Tags: `runtime/forum.rs`
  `tag_ids(&mut self, f)` caches per `f.channel` and `read_tags` does `GetChannel` on `f.channel`. The test hook:
  `Shared::look_for_forum` (`#[cfg(test)]`, near :510).
- **The fake.** `crates/theseus-sim/src/fake_discord.rs` and `fake_discord/{channels,forums,threads}.rs`
  (`channel_patch`, `is_forum`).

## What to build
1. **theseus-ono8g.** A function in `render/approval.rs` builds the approval card's content and settled `line` from
   `req.plain` (and `req.task`, `req.external_text`, `req.expires_at_ms`, the route, `elsewhere`), in exactly the
   approved layout above; `card`'s approval branch calls it. A floor call keeps its 🔒 floor wording in the risk/why
   lines (plain words already say "it touches Theseus's own state or a secret, which always asks"); a request with no
   `plain` (an older daemon) keeps today's card. The `-# ` line ends `· or `theseus answer <short> approve``, where
   today's `elsewhere` said "you can also answer with ...": keep a DM route's "for <place>" part. No "§" anywhere in
   any string this card can show (a test).
2. **The golden.** A golden test of the card's full text for the owner-approved example (a task's `fs.write` of a new
   file under the home after outside text, with a diff), and one for a plain posture ask with no trust button. The
   test's doc comment records: "owner-approved layout, 2026-10-22 (theseus-ono8g); the Discord task flow is locked:
   change this golden only with the owner's OK". Store the expected text inline or as a `.golden` file beside the test,
   the way the crate already does goldens (search `golden`).
3. **theseus-p5g63.** Reproduce the double creation first with a test (a fresh store, a guild with no forum: a READY
   and the start's own look race, or two READYs): exactly one forum is made, and the recorded one is the one used.
   The fix is the smaller of: `ensure_forum` re-reads the META record after taking `d.gate` (so the second caller finds
   the first's forum), or the start path and READY path share one look. Then the tags: `tag_ids` must ask about the
   forum the posts go to. If after the fix the fake still answers 404 for a bot-made forum, fix the fake (it must know
   channels it created) and say so; if the daemon aimed at the wrong forum, the first fix covers it.

## Tests (each with a planted revert; the first is the key test)
- **`the_approval_card_reads_as_the_owner_approved_it`** (the golden): plant: put the old `` `tool` `` line back: it
  fails.
- `an_approval_card_names_no_spec_clause`: every approval card variant (posture, outside text, floor) has no "§".
  Plant: restore the old reason text.
- `a_card_without_plain_words_keeps_the_old_shape`: `plain: None` renders as today. Plant: unwrap `plain`.
- **`one_start_on_a_guild_with_no_forum_makes_one_forum`**: plant: drop the re-read (or the shared look): two forums.
- `task_posts_get_their_tags_on_a_forum_the_bot_made`: a post on the made forum carries the running tag. Plant: aim
  `read_tags` at another channel id.
- `cargo nextest run -p theseus-discord` and `cargo nextest run -p theseus-sim` must pass; if you touched core,
  `cargo nextest run -p theseus-core needs_you`.

## Not in this row
- Any other message of the Discord task flow (the task reply, 🔔 / 🧵 lines, 📋 board, forum posts' live cards, reports):
  **locked by the owner**. Do not change their words, order or timing. If a change of yours would move one, stop and
  say so.
- The proactive-message convention (theseus-x8b9k): another session's, not this row.
- `crates/theseus-discord/src/runtime.rs` (3,499 / 3,500): add nothing. `render.rs` is 3,003 / 3,009: the card's new
  code goes in `render/approval.rs`, and `render.rs` should get shorter.
- The live check on the owner's guild: the owner's, after install.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Rows beside you on `main` 8260dbab, joining `integ/tail4` with you:** tail4-voice (`theseus-voice`, core `voice.rs`,
  Discord `runtime/voice.rs` one call), tail4-tui (TUI, `crates/theseus/src/herdr*`), tail4-route (core `thread_route.rs`,
  `turn/compile_step.rs`), tail4-tests (test helpers, `bench/recall`), tail4-onto (core `judge/onto/`). Stay out of
  their files.
- **Near you, not yet on main:** `integ/tail3` changed `runtime/forum/archive.rs`, `runtime/task_channels.rs`,
  `runtime/task_home.rs` and `runtime/tasks.rs`. Read `git diff origin/main origin/integ/tail3 -- crates/theseus-discord/`
  before you edit `task_home.rs` or `forum.rs`, and keep your edits off its hunks.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids and paths are invented.
- Read `crates/theseus-discord/AGENTS.md` and `crates/theseus-core/AGENTS.md`'s needs-you passage first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; the card before and after for the three variants (outside text, posture,
floor), as plain text the owner can read; each changed old-words assertion, old and new; the double-forum cause and
which fix you took; whether the 404 was the fake's or the daemon's; each test and its plant with the failing line; the
keel guard's output; the store format (unchanged); a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
