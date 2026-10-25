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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3i-modal-label`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `0fda4d57`. That is install #83's gated tree: release 1.0.2 (signed tag
`release-1.0.2` at `ac298666`) plus the joined patch3 work. Fetch it and branch from `0fda4d57`. Line numbers below
were read at `0fda4d57` and are right unless a line moved; search by name anyway. **Where the release preamble above
disagrees with this note about the base, the store format, ceilings, the keel or the work in flight, this note wins.**
The preamble above is older than your base: ignore its store-format numbers, its list of changes in flight, and its
sentence that lets you raise a line ceiling. The rules here replace them.

**Early push (required, before any code).** Do setup first (`cargo --version` alone, then nextest) and your first
`cargo build -p theseus-discord`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-tbm1q: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3i-modal-label`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches. **A routine
that fires and pushes nothing is indistinguishable from a dead one.** This row is small: if it is done in an hour,
push it and write the report. If it fails, push the failed attempt with your findings.

**Store format at your base: 51**; the kernel's `SCHEMA` is 2. This row touches neither.

**Line ceilings — never raise one** (`scripts/long-files.txt`). **This one matters for you.**
`crates/theseus-discord/src/runtime.rs` is **3,496 lines of its 3,500 ceiling — four lines left in the whole file**,
and two other rows in flight also touch the Discord runtime. **Do not add a line to `runtime.rs`.** Your own file,
`crates/theseus-discord/src/runtime/start_task.rs`, is 451 lines and unlisted, so its limit is 2,500.
`crates/theseus-discord/src/render.rs` is 2,982 of 3,009 — you should not need it at all.

## The owner's authorization, and what it does and does not allow

**This is a change to the LOCKED Discord DM task flow, and it has the owner's word.**

> the owner, Discord DM 2026-10-25 09:54: **"Yes to all"**

That was his answer to a list that included, as item (3), verbatim: *"fix the Discord modal label `The message (read
as outside text)` for the owner's own message (locked flow change approved)"*. So this row may land.

**What that authorization does NOT cover:** any other wording, layout, ordering, field, button or timing in the
modal. **No wording change the owner did not ask for.** This row fixes one label that is false for his own message.
It does not restyle the modal, does not touch the brief field, the budget picker, the report picker or the expected
picker, does not reorder anything, and does not change the modal's title.

## Why this row exists

`crates/theseus-discord/src/runtime/start_task.rs` :167 is the new-task modal, `fn modal(quote, brief)`. When a
message is made a task, its text comes along as an editable quote field, and the field's label is **unconditional**
(:175-177):

```
"The message (read as outside text)",
Some("The task reads it verbatim; clear it to start without"),
```

Since **theseus-nfhaf** the owner's own message marks nothing — a quote of his own words is **not** outside text, and
the session does not mark it as such. So for the single most common case, a message he wrote himself, the modal tells
him something false about his own words. `crates/theseus-discord/AGENTS.md` :249-251 records exactly this and says
the string "is in no locked golden, but it is Discord-visible, so correcting it is the owner's call and is
deliberately left (theseus-nfhaf join fix 3)". **That call has now been made.**

## Your task: p3i-modal-label (theseus-tbm1q)

Branch: `cloud/20261025-p3i-modal-label`.

Every commit subject carries `theseus-tbm1q`. Deadline for the report: about 1.5 hours after you start.

Read first, in this order: `crates/theseus-discord/AGENTS.md`, and in it the **"Starting tasks"** paragraph
(~:246-253) and the **"A reply that offers a task"** paragraph that follows it — they are the house account of this
surface and they are part of your task (see point 3). Then `crates/theseus-discord/src/runtime/start_task.rs` whole,
and in particular: `MODAL` (:37, `"nt:"`), `outside_id()` (:51) with its doc at :49-50 (*"The modal id of a message
by someone else: its quote is outside text (theseus-nfhaf), where `nt:` alone carries the presser's own words"*),
`OUTSIDE` (:54), `label()` (:148), `modal()` (:167-213, whose last line hard-codes `"custom_id": MODAL`), and
`Submit.outside` (:227) with its doc *"The quote is another person's message (`nt:!`), not the presser's words"*.

### The good news: the caller already knows

**Do not invent a way to tell whose message it is — one already exists.** The only caller that passes a quote is
`crates/theseus-discord/src/runtime/task_cmds.rs` :430-433:

```rust
let mut m = start_task::modal(quote.as_ref().map(|(q, _)| q.as_str()), None);
if quote.is_some_and(|(_, own)| !own) {
    m["data"]["custom_id"] = start_task::outside_id().into();
}
```

The quote is an `Option<(String, bool)>` where the bool is **`own`** — whether the quoted message is the presser's
own. The caller already uses it to choose the modal's `custom_id` (`nt:` vs `nt:!`), and the submit path already
reads that back into `Submit.outside`. The bug is only that **`own` never reaches the label**.

The other three call sites pass `quote: None`, so they emit no quote field at all and are unaffected by the label:
`task_cmds.rs` :458, `task_cmds.rs` :552, and `start_task.rs` :365 (`offer_press`, which fills `brief` and no quote).
**Verify all four sites yourself with `git grep -n 'start_task::modal(\|modal(' crates/theseus-discord/src` and list
them in the report** — the issue's notes named different line numbers, read against an older tree, and I want the
real ones recorded.

### What to build

**1. `modal()` learns whose message the quote is, and labels it truthfully.**

Give `modal` the information. The cleanest shape at your base is to change the quote parameter so it carries `own`
alongside the text — for example `quote: Option<(&str, bool)>` — since the caller holds exactly that tuple already
and the three `None` sites need no change beyond their type. A separate `own: bool` parameter is acceptable if you
prefer it; **say which you chose and why.** Whichever you choose, `modal` must be impossible to call with a quote and
no answer to "whose is it" — that is the defect you are fixing, so do not leave a defaultable flag that reproduces it.

The label, exactly:

- the quote is **the presser's own message** → label `"The message"`;
- the quote is **someone else's message** → label `"The message (read as outside text)"`, **unchanged from the base**.

The **description line is identical in both cases** and byte-identical to the base:
`"The task reads it verbatim; clear it to start without"`. Do not reword it. Nothing else in the modal changes.

**2. Fold the `custom_id` choice in, if and only if it is strictly a simplification.**

Once `modal` knows `own`, the caller's `m["data"]["custom_id"] = outside_id()` override at `task_cmds.rs` :432 is
information `modal` now has itself, so `modal` could set the right id directly and the override could go. **That is
allowed, and it is tidier, but only if the resulting `custom_id` is byte-identical in both cases** (`nt:` for own,
`nt:!` for someone else's) and `Submit.outside` still reads back exactly as it does today. The `custom_id` is wire
state: a change to it would break a modal already open in the owner's client and is **not** covered by his
authorization. If you cannot make that equivalence airtight with a test, **leave the override where it is** and say
so. A test asserting both ids for both cases is required either way.

**3. Correct `crates/theseus-discord/AGENTS.md`.**

Its "Starting tasks" paragraph (~:249-251) currently says the label "is now false for the owner's own message; the
string is in no locked golden, but it is Discord-visible, so correcting it is the owner's call and is deliberately
left (theseus-nfhaf join fix 3)". That sentence is no longer true. Replace it with what the code now does — the label
distinguishes the presser's own message from someone else's — and reference `theseus-tbm1q` and the owner's approval
of 2026-10-25 in the house's voice. **Edit only that sentence and only within that paragraph.** The rest of the file,
including the "A reply that offers a task" paragraph, stays untouched.

### Tests (each red at the base, with a planted revert)

The existing tests for this surface are `crates/theseus-discord/src/tests_board/commands.rs` (the fake gateway's
`slash`, `autocomplete`, `message_command`, `submit_modal` and `pick_in_reply`, from
`theseus_sim::fake_discord::commands`) and `crates/theseus-discord/src/tests_board/offer.rs`. Add yours there, or in
a `tests_board/` file of your own; name the file in the report.

1. **Own words.** The modal built for a quote of the presser's own message has the label `"The message"` and the
   base's description, and its `custom_id` is `nt:`. Plant P1: pass the outside label for an own quote: red.
2. **Someone else's words.** The modal built for another person's message has the label
   `"The message (read as outside text)"` — **byte-identical to the base** — the same description, and `custom_id`
   `nt:!`. Plant P2: give it the own label: red.
3. **No quote.** `modal(None, …)` is **byte-identical to the base's output**, field for field. Assert the whole JSON
   against a snapshot, not just the label's absence: this is the assertion that proves you restyled nothing. Plant
   P3: change any other label, title or picker option: red.
4. **The submit still reads it.** A submitted modal with `nt:` yields `Submit.outside == false` and one with `nt:!`
   yields `true`, exactly as at the base, and `task.start`'s params are unchanged in both cases. Plant P4: swap the
   ids: red.
- **Keep green:** the whole `theseus-discord` suite, every golden, and `theseus-sim`'s fake-Discord tests.
- If an existing test has to change its expectation, **do not delete or weaken it**: rewrite it with the same number
  of assertions, keep its name, and list it under **Keel findings expected** with its line and reason.

### The locked flow, the golden, and the `Keel:` trailer

**I checked that this label is not in `crates/theseus-discord/src/golden/dm_flow.txt`.** Verify it yourself and paste
the proof:

```
git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt
git grep -c 'read as outside text' -- crates/theseus-discord/src/golden/
```

`dm_flow.txt` must stay byte-identical. **If your change moves it, STOP** — that is a different and larger change
than the owner approved — restore it, and write `OWNER CALL:` in the report with the diff.

**The trailer rule, which is strict.** Run `python3 scripts/keel-guard.py` before every commit and
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and **paste the whole output**.

- **If the guard reports a finding on any golden or flow test this row moves, the commit that moves it carries a
  `Keel:` trailer whose rule name is _the guard's own_** — copied from its output line, never from your judgement,
  and never a rule name you chose — and whose reason quotes the owner:
  `Keel: <rule from the guard> <path> — the owner, Discord DM 2026-10-25 09:54: "Yes to all"`
- **If the guard is silent, write the trailer anyway**, with the rule the guard would use for the file you moved, and
  **say in the report that the guard was silent**. This is a locked-flow change; it carries its authorization
  visibly either way.
- **You cannot sign, so your trailer counts for nothing on its own** — an unsigned ack is no ack, and a branch may not
  ack itself. Only the maintainer's signed merge can make it count. So **also reproduce the exact trailer line,
  verbatim, in `CLOUD_REPORT.md` under a heading `KEEL TRAILER FOR THE MAINTAINER'S SIGNED MERGE`**, so he can carry
  it across. Do not invent a rule name, do not claim the ack is yours, and if the guard flags something you did not
  expect, write `OWNER CALL:` and leave that part out.

### Not in this row

No `TaskOrigin`, `self_started` or `because` (row A). No `task.not_this`, no Stop or Not this button, no Ops or
cockpit change, no `.tsx` edit (row B). No tools-deferral change (row C). No RSI change (row F). No Drop or Halt
button on the self notes (row G). No `max_wakes` or persona change (row H). **No change to the offer button, the
offer flow, `Tasks.offers`, or `offer_press`** — the owner's Q1 answer (the offer starting the task at once, and the
button becoming Stop / Not this) is **row B's** surface, in a separate issue, and it lands in this same file: stay
out of it. No `runtime.rs` line added. No new dependency. No benchmark, no spend, no AWS, no model call.

## Shared files: fires are parallel, joins serialise

Eleven other patch3 rows are in flight right now against this same `0fda4d57`. That is deliberate: the review queue,
not the build, is our bottleneck, so rows fire concurrently and the integrator serialises the joins.

**Your concurrent row is `theseus-o1h3f.2` (row B, "Not this", branch `cloud/20261025-p3i-not-this`).** B adds the
Stop / Not this buttons and the 👎 route; its Discord half is `crates/theseus-discord/src/runtime/route.rs`, but the
owner's Q1 answer puts B's button work in **this same `start_task.rs`** surface.

- **Write your row against `main 0fda4d57` as if you were the only one editing these files. Do NOT try to coordinate
  with row B, do NOT stub, and do NOT guess at its shape.**
- **Your join goes AFTER row B** (`theseus-o1h3f.2`). Put that line in your report.
- `theseus-dgbcx` (child w55) is editing `golden/dm_flow.txt` right now. **That is not a collision with you**: your
  label is not in that golden. Say so in the report, with the grep that proves it.
- **List every hunk you place, with the file, the line range, the function and why** — especially every hunk in
  `start_task.rs` and `task_cmds.rs` — so the join fix is mechanical.
- If you find you need to edit `runtime.rs`, `route.rs`, `courier.rs`, `render.rs`, any `.tsx` file, or
  `crates/theseus-core/src/task.rs`, **stop, leave that part out, and write `JOIN SEAM: <file> — <why I needed it>`
  in the report.**

## Rules for this row

- **The owner's authorization is quoted above and nowhere widened.** One label, two cases, one AGENTS.md sentence.
- **Keel:** as in the trailer rule above. Never add a trailer for anything outside what the guard names.
- **No ceiling raised and no cap loosened without saying so explicitly in `CLOUD_REPORT.md`.** You add no line to
  `runtime.rs`; say so.
- **Data floor:** nothing new is sent to a model or routed anywhere. A label is text rendered to one Discord client.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner" — including in the AGENTS.md sentence you rewrite.
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it so cargo rebuilds it, and run `git status` after every restore); right over compatible.
- **The gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-discord -p
  theseus-sim` plus every other crate whose files you touched. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, **never a shell heredoc**, and check
after the commit that it is not empty. Push the branch. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** the owner's quoted authorization; store format (51, untouched);
ceilings, with `runtime.rs` untouched stated plainly; any `OWNER CALL:`, `JOIN SEAM:` or STOP. (2)
**`KEEL TRAILER FOR THE MAINTAINER'S SIGNED MERGE`** — the exact trailer line, verbatim, and whether the guard
spoke or was silent. (3) **The signature:** which shape you gave `modal` and why, with the before and after. (4)
**Every call site**, with its real `file:line` from your own grep, and what each passes. (5) **The label**, both cases
quoted, with the description shown identical to the base. (6) **The `custom_id`:** folded in or left in the caller,
and the equivalence test. (7) **AGENTS.md:** the old sentence and the new one, quoted. (8) **Tests** and plants P1-P4,
red at the base and green after, with counts. (9) Any existing test whose expectation you rewrote, under **Keel
findings expected**. (10) **Gate** commands and results. (11) **Keel** output, whole. (12) **DM golden:** the diff
result and the grep that shows the label is not in it. (13) **Join notes:** the line that your join goes after row B
(`theseus-o1h3f.2`); every file touched with line ranges; and a statement that you edited no `.tsx` file, not
`runtime.rs`, not `route.rs`, not `courier.rs`, and not `task.rs`. (14) **Follow-ups.** (15) One line on the data
floor.
