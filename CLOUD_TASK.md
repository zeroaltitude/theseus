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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-task-offer-chip`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

---
## Your task: task-offer-chip, Jev spots deferrable multi-step work in the owner's message and the reply offers a one-tap "➕ Make this a task" (theseus-bh3t.5)

Branch: `cloud/20261017-task-offer-chip`. Every commit's subject carries `theseus-bh3t.5`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:30, verbatim):** "In tasks: what are discord forums and threads -- I didn't
know about these! Does the discord app now have the power to make new channels? If so, I'd love to start with every
task that's multi step or expected at more than 10 minutes of work to get a channel automatically. I love your other
ideas and not yet implemented ideas so please start those ASAP for quick implementation and join to main today".
This row is one of those "other ideas": when the owner says "we should…", "later can you…", or lists things to do,
the reply carries a button that turns it into a task, pre-filled, in one tap.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Five sibling rows build in parallel today:
discord-task-channels (theseus-bh3t.1, joins before you; it adds an `expect` select as the start modal's last component
without changing `modal`'s signature), discord-reactions (bh3t.2), voice-tasks (bh3t.3), discord-war-room (bh3t.4),
discord-emoji-catchup (theseus-ss36w). **Also in flight today, not a sibling of this batch: the China-origin row
(theseus-a1wvq), which adds `route.v6`.** Do not touch any route pack, `ROUTE_PACK`, or route's tests; expect its join to
meet yours on the `EMBEDDED` list and the pack count (below).

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 5 | `task-offer-chip` | theseus-bh3t.5 | `classify.v3` = `classify.v2` word for word + one question, `offer_task`; the core puts an `offer` on the reply's post when that answer is yes at the Act band, the turn is not trivial/quick, and the input is the owner's from a private place; Discord adds a `➕ Make this a task` button to the reply's last message; the press opens the start-task modal pre-filled | `crates/theseus-judge/packs/classify.v3.toml` (new), `theseus-judge/src/pack.rs` (`EMBEDDED`, the pack tests), `theseus-judge/src/builders.rs` (the builder list), `theseus-judge/fixtures/golden/classify.v3.request.json` (new, generated); `theseus-core/src/judge/inbound.rs` (the answer), `turn/reply_step.rs` (the post body); `dc/courier.rs::reply`, `dc/render.rs` (`Buttons`), `dc/runtime/task_cmds.rs` (the press), `dc/runtime/start_task.rs` (`modal`'s `brief`) | the pack's golden; `classify.v3` is `classify.v2` plus exactly one question; the chip on a yes at Act, none at Confirm, none on a trivial/quick turn, none for a guest, none in a shared place, none when the turn itself made a task; the press opens the modal with the brief pre-filled | Jev + core + Discord | 1 at the join (the modal) | cloud | M |

## What exists (read these first)
- **The packs** (`crates/theseus-judge/packs/`): `classify.v2.toml` (106 lines) is live: `id = "classify"`, `version =
  2`, `point = "inbound"` (`:8-17`); its header (`:1-7`) is the comment to imitate ("classify.v1's questions word for
  word… v1 stays embedded for its history"). Its `should_promote` question (`:48-55`) is about work asked for **now**
  that should run in the background: yours is different, work described for **later**. Bars are `act = 0.90`,
  `confirm = 0.60`; bands `Band { Act, Confirm, Escalate }` (`crates/theseus-judge/src/band.rs:25`).
- **The versioning rules:** "Any change (wording, criteria, thresholds, builder, model) is a new version"
  (`theseus-judge/src/pack.rs:1-4`); "a live pack's options never change in place … v1 to v4 stay embedded for their
  history" (`packs/route.v5.toml:11-13`). Registration: `EMBEDDED` (`pack.rs:47`; classify at `:53-54`, its history in the
  doc comment `:38-46`). Tests to extend: the per-pack shape list (`pack.rs:1133`, classify.v2's at `:1205-1217`), the
  pack count (`:1419-1422`, `22`), classify.v2's rollback rules (`:1388-1394`), the word-for-word check
  `classify_v2_without_threads_is_classify_v1` (`src/tests_threads.rs:66-83`, the pattern for your v3-is-v2 test), the
  goldens (`src/builders.rs:1081-1100` names each builder's pack; `:1116` `each_pack_asks_its_golden_request` needs
  `fixtures/golden/classify.v3.request.json`, written with `UPDATE_GOLDEN=1`, `src/tests.rs:23-31`).
- **Where classify runs:** `crates/theseus-core/src/judge/inbound.rs`: `CLASSIFY_PACK = "classify.v2"` (`:49`), `PACKS`
  (`:67`); on every human message that starts a turn (`:16-20`, `inbound_early` `:268`, `judge_inbound` `:812`; classify,
  role and directive batched, `:874-899`). Only its thread answer reaches the turn today (`thread_answer`, `:741-794`,
  gated on `j.pack == CLASSIFY_PACK && j.actionable()`, handed over as `ThreadAnswered` `:80-90`, taken by the turn at
  `rpc/thread_flow.rs:443`). Other places that name the pack: `learning/prove.rs:363`, core tests expecting
  `"classify.v2/kind"` (`tests_inbound.rs:151`, `tests_capture.rs:164`), the template
  (`crates/theseus-core/config/theseus.example.toml:998`). Move the live pack to v3 exactly as v1 → v2 did (that change
  is 90c84799, `core: the thread router, its ask, the misroute guard and /move`); rewrite the expected strings, never
  delete an assertion.
- **The route's mode** reaches the turn: `TurnRoute.mode` with `trivial` and `quick` (`theseus-protocol/src/route.rs:13-18`;
  `route.v5.toml:34-35`).
- **The reply's post body** is built in `crates/theseus-core/src/turn/reply_step.rs:46-57` (`kind`, `turn_id`, `loops`,
  `result`, `reply_to`, `wakes`, `reports`). `theseus-protocol/src/lib.rs` is at 2,731 of its 2,733-line ceiling, so the
  offer goes on the post body, **not** on `TurnSubmitResult`.
- **Discord:** the reply is assembled in `dc/courier.rs::reply` (`:876`); every part gets `Buttons::Keep` (`:925`) and
  the last `Buttons::Sources(..)` when it cites (`:939-944`). `Buttons` is in `dc/render.rs:52-85` (render.rs is 2,920 of
  its 3,009 ceiling), mapped at `courier.rs:1611` (create) and `:1680` (edit). The start modal: `start_task::modal(quote)`
  (`dc/runtime/start_task.rs:52-88`; `brief` required and not pre-filled `:53-58`; `quote` pre-filled with `"value"`
  `:59-66`; no title field); opened by the message command "Make this a task" (`task_cmds.rs:41, 376-378`) and the board
  button (`:482-483`); submitted as `task.start` (`start_task.rs:129, 160-189`, refusing shared places `:169-180`).
  `dc/runtime/jev.rs:20-92` is a good template for a small button builder and its press.

## The design

**`classify.v3.toml`:** v2's file word for word (header comment updated as v2's was), `version = 3`, plus one question:
```toml
[questions.offer_task]
type = "noul"
instructions = "Does this message describe work to be done later, in more than one step, that the person has not asked to start now?"
act = 0.90
confirm = 0.60
when_true = "The person says what should happen later or lists things to do (\"we should…\", \"later can you…\", \"next: a, b, c\"), and nothing in the message asks for that work to begin in this conversation."
when_false = "The message asks for something now, is a question, is chat, or describes a single quick step."
```
(No `decides` line: it informs, as `fragment` does, `classify.v2.toml:57-63`; it routes nothing.) `classify.v1` and `classify.v2` stay embedded.

**The answer to the turn.** Beside `thread_answer`, an `offer_answer(j) -> bool`: the classify pack, question
`offer_task`, a `Noul{true}` at band `Act` (never `Confirm`). Hand it over the way the thread answer travels (one more
field on what `inbound.rs` already hands the turn, read at the reply without waiting: if the judge has not answered by the
time the reply is written, no chip). At `reply_step.rs`, the body gains `"offer": {"brief": <the person's message, its
first 1,000 characters>}` when all hold: the offer answer is yes at Act; the route's mode is neither `trivial` nor
`quick`; the input is the owner's from a private place (`Core::owner_input`, `rpc/owner_input.rs:14`, or the turn's sink's
`owner` flag as `join_step.rs:30-35` reads it); the turn made no task itself (no `task.create` call in its loops). At most
one per message, by construction (one reply post per turn). Judging the **reply's** text is out of scope here (a new judge
point); say so in the report as the follow-up.

**Discord.** In `courier.rs::reply`, when the body has `offer` and the place is private: the last message gets one more
button, `➕ Make this a task`, custom id `tb:offer:<turn_id>` (≤ 100 characters), beside the sources link if any (a new
`Buttons` variant or a field on `Sources`; keep render.rs's growth under 15 lines). Keep the brief in a bounded map
(turn id → brief, 64 entries) on the courier's shared state. The press (in `task_cmds.rs`'s press path, `:482`): the owner
only (else an ephemeral "Only the owner can start a task here"); `respond_modal(&i, &start_task::modal(None,
Some(&brief)))`; after a restart (brief gone) open it empty. **`modal`'s signature becomes `modal(quote: Option<&str>,
brief: Option<&str>)`**, the brief pre-filled (`"value"`, clipped as `quote` is); its first line is a short title: the
message's first sentence, ≤ 60 characters (`task.start` takes the brief; check how the core titles a task with no
`title`, `crates/theseus-core/src/rpc/task_start.rs:52-62`, and pass `title` in `Submit::params` if the modal's brief's
first line should be it). Don't add a modal component (discord-task-channels adds the fifth). After a submit, the chip's
message is edited to drop the button (one edit).

**FAST and budgets:** classify grows by one question in a Jev request that already runs (its golden shows the size);
nothing new in the turn's first request; nothing waits for the judge.

**Tests:** judge: the golden; `classify_v3_is_classify_v2_and_an_offer_question` (ids, questions, thresholds, rollback);
the pack count and shape lists. Core (`tests_inbound.rs`' fake judge): yes at Act → the reply's body has `offer`; yes at
Confirm → none; a `trivial` route → none; a guest's message → none; a turn that called `task.create` → none. Discord (the
gateway rig, `dc/tests_gateway.rs`; `dc/tests_board/commands.rs:720` drives "Make this a task"): a reply post with
`offer` → its last message has the `tb:offer:` button; `press` it as the owner → the modal response's `brief` value is the
message's text; as a guest → refused; in a shared place → no button. Plant a revert for: the band check (Confirm passes),
the trivial check, the owner check, and the button.

**Live check for the maintainer:** in the owner's DM, write "we should clean up the bench reports later and then re-run
the async suite"; see the chip; press it; see the modal pre-filled; submit; see the task start.

**Limits for this row.** No new model tool, no model-visible text. `dc/runtime.rs` has 13 lines of room: add none.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
