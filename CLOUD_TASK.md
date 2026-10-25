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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3i-self-note-buttons`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
- `git commit --allow-empty -m "theseus-o1h3f.7: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3i-self-note-buttons`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches. **A routine
that fires and pushes nothing is indistinguishable from a dead one.** If your work fails, or you run out of time
mid-step, push anyway, with your findings in `CLOUD_REPORT.md`.

**Store format at your base: 51**; the kernel's `SCHEMA` is 2. This row touches neither: it renders existing posts
and calls existing methods.

**Line ceilings — never raise one** (`scripts/long-files.txt`; every Rust file the list does not name fails past
2,500 lines). **Two of these decide how you write this row:**

- `crates/theseus-discord/src/runtime.rs` is **3,496 of 3,500 — four lines left in the whole file**, and two other
  rows in flight also touch the Discord runtime. Your new handler module needs **one** `mod` line there (they are at
  :50-106). **Spend exactly one line, and say in the report that you spent one of the four.** Everything else goes in
  your own module.
- `crates/theseus-discord/src/render.rs` is **2,982 of 3,009 — 27 lines left**, and the `Buttons` enum you extend is
  at :55. Keep your variant to one or two lines and put the components themselves in your own module, the way
  `persona_soul` and `seal` do.

Others: `crates/theseus-discord/src/courier.rs` is 2,222 and unlisted (limit 2,500 — you have room, but it is not
much, so keep your arms tight). `crates/theseus-protocol/src/lib.rs` 2,612 of 2,733: you need **no** new method, so
you should not touch it at all.

## The owner's authorization, and what it does and does not allow

**This row changes the LOCKED Discord DM flow, and it has the owner's word.**

> the owner, Discord DM 2026-10-25 09:54: **"Yes to all"**

The design filed this row as *"1.0.3 yes, **unless** the conductor counts these notes as the locked flow"*. The
conductor's ruling: **they are the locked flow** — they are messages in the owner's DM. And the owner has now
approved locked-flow changes outright, so that condition is settled and this row is in 1.0.3, **with his word quoted
in a `Keel:` trailer on anything it moves.**

**What that authorization does NOT cover:** any wording, layout, ordering or timing of the notes themselves. **No
wording change the owner did not ask for.** You add two buttons. You do not reword the digest, you do not reword the
join note, you do not change when either is posted or how often.

## Why this row exists

The owner's standing line, 2026-10-24 21:52: *"we should be very very lightweight on any limits to theseus tasking
itself based on what it sees and what we are working on. notify, work, and allow feedback is the model."*

The stop exists, but only at the CLI. RSI work has `halt self` (anyone's, `rsi/switch.rs`) and
`theseus self drop <item>` — and the design records that drop as **CLI only** ("The owner's word",
`rsi/backlog.rs`). So when Theseus tells the owner in his DM that it started something by itself, the note gives him
**no way to answer it there**: he has to go to a terminal. That is the gap this row closes, and the reason the design
pairs it with row F: F opens the notify path, G makes it answerable in one tap.

## Your task: p3i-self-note-buttons, Drop and Halt on the self notes (theseus-o1h3f.7, o1h3f row G)

Branch: `cloud/20261025-p3i-self-note-buttons`.

Every commit subject carries `theseus-o1h3f.7`. Deadline for the report: about 2.5 hours after you start.
**Your specification is the **o1h3f design check** (the maintainer holds it; it is not in this repository), section 4 row G, with §2.4's
paragraph on the stop as its reasoning.** You do not have that file — its content is reproduced below. Do not
re-derive the design; build what is written here. The design's words:

> Row G adds a one-tap **Drop** per started item and a **Halt** to the `self_digest` / `self_join` DM notes.

### The shape at your base, which you follow exactly

I read this path at `0fda4d57` so you do not have to rediscover it. **Read these before you write anything:**

- **The kinds are already routed.** `crates/theseus-discord/src/courier.rs` :900-902 lists `self_digest` and
  `self_join` among the kinds that go to `jev_post`, with its comment at :888-899 explaining they are the owner's DM
  alone. **Change nothing there.**
- **`jev_post`** (`courier.rs` :1436) matches on `kind_of(a)` and returns `(content, key, buttons)`. Today
  `self_digest` and `self_join` fall through to the arm that gives them `Buttons::Keep`. Arms that give a card its own
  buttons are right there to copy: `"earned_card"` (:1447), `"seal_card"` (:1455), `"link_card"` (:1462),
  `"onto_card"` (:1478), `"persona_soul"` (:1487), `"card_question"` (:1492). Each one is four to eight lines: a key
  of its own (`format!("self:{}", a.correlation_id)` in your case) and a `Buttons::<Variant>(…)` built by a small
  function in a `runtime/` module.
- **`Buttons`** is `crates/theseus-discord/src/render.rs` :55, with one-line variants (`Seal(String)` :82,
  `OntoUndo(String)` :86, `PersonaSoul(String)` :88, `CardQuestion(String, String)` :92).
- **The variant becomes components in two places**, `courier.rs` :1768 and :1845 (one arm each, a line each:
  `Buttons::PersonaSoul(key) => crate::runtime::persona_soul::buttons(key)` and its `Some(...)` twin). **Both** must
  learn your variant, or the buttons appear in one path and not the other.
- **A press is dispatched through `notice_press`**, `crates/theseus-discord/src/runtime/earned.rs` :89, called from
  `runtime.rs` :1249 with the press's `custom_id`. Your handler hangs off that, keyed by your own `custom_id`
  prefix.
- **The model module to copy whole** is `crates/theseus-discord/src/runtime/persona_soul.rs`: a `MODAL`/prefix
  const, a `buttons(key)` that builds the components, a `key_of(body)` that reads the post's body, and the press
  handler. Follow its structure and its voice.
- **The methods you call already exist**: `self.drop` (`theseus-protocol/src/lib.rs` :425 `SELF_DROP`) and
  `self.halt` (:424 `SELF_HALT`). **Add no method.** `runtime/halt.rs`'s head comment is the account of who may halt:
  *"a halt is anyone's"*, while `self.resume` is the owner's alone. Read it before wiring Halt.

### What to build

**1. A Drop button per started item on the `self_digest` note.**

The digest names the items a tick or a night started. Give **each started item its own Drop button**, calling
`self.drop` for that item id — the same per-candidate shape `Buttons::Earned(…)` uses for the earned-autonomy card
(`courier.rs` :1447, with `crate::runtime::earned_candidates(body)`). So you need a reader that pulls the item ids
out of the post's body, the way `earned_candidates` does for its card.

- **Discord allows five buttons a row and five rows.** A digest naming many items must not build an invalid message.
  Cap the Drop buttons at a number you state, oldest dropped from the row first or newest kept — your call, say
  which — and make sure a digest with more items than the cap still posts, with the rest reachable at the CLI. **Test
  that case**: a note that fails to post is worse than a note with no buttons.
- The item ids must be the real ids `self.drop` takes. If the post's body does not carry them at your base, **say so
  in the report with the `file:line` you looked at**, and fall back to one Drop for the newest item, or to no Drop and
  Halt only — whichever is honest. **Do not invent an id format, and do not change what the core stages** (the
  staging side is row F's file, `rsi/backlog/start.rs`, which you may not touch).

**2. A Halt button on both the `self_digest` and the `self_join` notes.**

One button, calling `self.halt` as the presser with the message's ids, exactly as `runtime/halt.rs` does for the
words "halt self". A halt is anyone's, so the press needs no owner check beyond what `self.halt` itself applies —
**read `halt.rs` and match its behaviour; do not add or remove an authority check.**

**3. The press handlers.**

In a new `crates/theseus-discord/src/runtime/self_note.rs` (one `mod` line in `runtime.rs`, and no other line
there). Each press:

- answers the presser ephemerally with what happened, in the house's voice, as the sibling handlers do;
- is ledgered the way the sibling presses are;
- is idempotent: a second press of the same Drop says it is already dropped rather than erroring, and a second Halt
  says self work is already halted. **Test both.**
- **A guest's press is refused** the way the sibling handlers refuse one, with the same shape of message. For Drop,
  read `rsi/backlog.rs`'s "The owner's word" and match the core's own rule; for Halt, match `halt.rs`. **Do not
  loosen either.**

### Tests (each red at the base, with a planted revert)

Put new tests beside the existing Discord card tests — `crates/theseus-discord/src/tests_board/` is the convention,
and `theseus_sim::fake_discord` gives you the fake gateway; name the file in the report.

1. **The digest's buttons.** A staged `self_digest` post with three started items renders three Drop buttons with the
   right ids, plus one Halt. Plant P1: drop the new `jev_post` arm: red.
2. **Both mapping sites.** The components appear through **both** `courier.rs` :1768 and :1845 paths. Plant P2:
   wire only one: red. This is the bug this test exists to catch.
3. **The join note.** A staged `self_join` post renders Halt and no Drop. Plant P3: give it Drops: red.
4. **The cap.** A digest naming more items than your cap still posts a valid message (at most five buttons a row, at
   most five rows) and says where the rest are. Plant P4: remove the cap: red (assert the component count, and that
   the message is valid).
5. **The presses.** A Drop press calls `self.drop` with that item; a Halt press calls `self.halt`; each answers the
   presser. Plant P5: swap the methods: red.
6. **Idempotence.** A second Drop of the same item, and a second Halt, each answer plainly and do not error. Plant
   P6: remove the guard: red.
7. **Authority.** A guest's Drop press is refused with the sibling handlers' shape; Halt follows `halt.rs`'s rule
   exactly (quote that rule in the test's comment). Plant P7: drop the check: red. **Required** — this is the keel
   property of a new press.
8. **The notes' words are byte-identical.** The `self_digest` and `self_join` **text** is unchanged from the base;
   only components were added. Assert on the content string. Plant P8: change one word of the note: red. **This is
   the test that proves you honoured "no wording change the owner did not ask for"; it is required.**
- **Keep green:** the whole `theseus-discord` and `theseus-core` suites, `theseus-sim`'s fake-Discord tests, and every
  golden.
- If an existing test has to change its expectation, **do not delete or weaken it**: rewrite it with the same number
  of assertions, keep its name, and list it under **Keel findings expected** with its line and reason.

### The locked flow, the goldens, and the `Keel:` trailer

Run, and paste:

```
git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt
git grep -c 'self_digest\|self_join' -- crates/theseus-discord/src/golden/
```

**I checked that `self_digest` and `self_join` do not appear in `golden/dm_flow.txt`** (the only golden in that
directory, 69 lines), so I expect this row to move **no** golden. Verify that; `dm_flow.txt` must stay
byte-identical. **If your change moves it, STOP** — that is larger than what was approved — restore it, and write
`OWNER CALL:` in the report with the diff.

**The trailer rule, which is strict.** Run `python3 scripts/keel-guard.py` before every commit and
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and **paste the whole output**.

- **If the guard reports a finding on any golden or flow test this row moves, the commit that moves it carries a
  `Keel:` trailer whose rule name is _the guard's own_** — copied from its output line, never from your judgement,
  and never a rule name you chose — and whose reason quotes the owner:
  `Keel: <rule from the guard> <path> — the owner, Discord DM 2026-10-25 09:54: "Yes to all"`
- **If you move a golden or flow test and the guard is silent, write the trailer anyway**, with the rule the guard
  would use for the file you moved, and **say in the report that the guard was silent.** A locked-flow change carries
  its authorization visibly either way.
- **If you move no golden and no flow test at all** — the outcome I expect — write no trailer, and instead **quote the
  owner's authorization line in the body of the commit that adds the buttons**, and say in the report that nothing
  keel-guarded moved and no trailer was owed.
- **You cannot sign, so a trailer of yours counts for nothing on its own** — an unsigned ack is no ack, and a branch
  may not ack itself. If you write one, **also reproduce it verbatim in `CLOUD_REPORT.md` under a heading
  `KEEL TRAILER FOR THE MAINTAINER'S SIGNED MERGE`.**
- **If the guard flags anything that is NOT a golden or flow test** — a cap, a ceiling, a removed test, a lowered
  assertion count — **STOP that part, put the old value back, and write `OWNER CALL: <the guard's exact line>`.** His
  "Yes to all" was about the DM flow, not about caps, and you may not stretch it.

### Not in this row

No `TaskOrigin`, `self_started` or `because` (row A). No `task.not_this`, no Stop or Not this button on a **task**
card, no Ops or cockpit change, no `.tsx` edit, and **no `runtime/route.rs` edit** (row B — see "Shared files"). No
tools-deferral change (row C). No admission or burn change (rows D and E). **No change to anything that stages these
posts** — not `rsi/pipeline.rs`, `rsi/join.rs`, `rsi/backlog/start.rs` or `rsi/backlog/word.rs`, and not the notes'
text: that is **row F**, in flight right now, and the staging side is entirely theirs. No `max_wakes` or persona
change (row H). **No change to the new-task modal's quote label** (`runtime/start_task.rs` :175 — that is
`theseus-tbm1q`). No new RPC method, no new config key, no new dependency. **No second line in `runtime.rs` beyond
your one `mod` line.** No benchmark, no spend, no AWS, no model call.

## Shared files: fires are parallel, joins serialise

Eleven other patch3 rows are in flight right now against this same `0fda4d57`, in three lanes. That is deliberate and
it is not a defect to design around: the tag is due today and the **review queue, not the build, is the bottleneck**,
so the rows fire concurrently and the integrator serialises the joins. Your job is to make that join cheap and
mechanical instead of a guess.

**Write your row against `main 0fda4d57` as if you were the only one editing these files. Do NOT try to coordinate
with the other rows, do NOT stub, and do NOT guess at their shape. In your report, list every hunk you place in a
shared file, with the file, the line range, the function and why, so the conductor's join fix is mechanical.**

Your overlaps, by row id:

- **Row B, `theseus-o1h3f.2`** (branch `cloud/20261025-p3i-not-this`) edits
  `crates/theseus-discord/src/runtime/route.rs` and may add a button to a task card. **Your join goes AFTER row B.**
  Put that line in your report. **Stay out of `route.rs` entirely** — if you think you need it, that is a signal you
  have strayed into B's work.
- **Row F, `theseus-o1h3f.6`** (branch `cloud/20261025-p3i-rsi-notify`) owns every file that **stages** these notes,
  and it is adding a `self_digest` line per pipeline tick. **F joins before you** (it joins first in the whole wave,
  on its own). So at your join, the digest may name more items, more often, than it does at your base — **which is
  precisely why point 1's cap and its test matter.** Write your reader so it does not care how many items a digest
  names. Do not edit any `rsi/` file.
- **`crates/theseus-discord/src/runtime.rs`** is contended by its ceiling, not its content: four lines, and you
  spend one. Row B and `theseus-tbm1q` are both instructed to spend none. **Say in the report that you spent exactly
  one and which line it is**, so the integrator can check the arithmetic at the join.
- **`crates/theseus-discord/src/render.rs`** (the `Buttons` enum, 27 lines of headroom) — no other row in this wave
  touches it, but `theseus-dgbcx` (child w55) is working in the Discord crate. Keep your variant to one line and name
  it.
- **`crates/theseus-discord/src/golden/dm_flow.txt`** — `theseus-dgbcx` is editing that golden right now. You should
  not touch it at all; prove it.

If you find you need a hunk in a file not named above — `route.rs`, `start_task.rs`, any `rsi/` file, any `.tsx` file
— **stop, leave that part out, and write `JOIN SEAM: <file> — <why I needed it>` in the report.**

## Rules for this row

- **Keel:** as in the trailer rule above. Never add a trailer for anything outside what the guard names, and never for
  a cap or a ceiling.
- **No ceiling raised and no cap loosened without saying so explicitly in `CLOUD_REPORT.md`.** State the `runtime.rs`
  line arithmetic (3,496 at the base, your one line, 3,500 ceiling) and the `render.rs` arithmetic.
- **Data floor:** nothing new is sent to a model or routed anywhere. The buttons are components on posts the core
  already stages to the owner's DM, and the presses call existing local methods.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it so cargo rebuilds it, and run `git status` after every restore); right over compatible.
- **The gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-discord -p
  theseus-core -p theseus-sim` plus every other crate whose files you touched. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, **never a shell heredoc**, and check
after the commit that it is not empty. Push the branch. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** the owner's quoted authorization; store format (51, untouched) and
`SCHEMA` (2, untouched); the `runtime.rs` and `render.rs` line arithmetic; any `OWNER CALL:`, `JOIN SEAM:` or STOP.
(2) **Keel:** the guard's whole output, whether anything keel-guarded moved, and either
`KEEL TRAILER FOR THE MAINTAINER'S SIGNED MERGE` with the verbatim line, or the statement that no trailer was owed
and where the authorization is quoted instead. (3) **The two arms** in `jev_post`, with their keys and their
`file:line`. (4) **Both mapping sites** (:1768 and :1845) named, with the line you added to each. (5) **The item
ids:** where you read them from, and what you did if the body does not carry them. (6) **The cap:** the number, the
rule, and what a digest over it looks like. (7) **The handlers:** the module, the ephemeral answers quoted, the
idempotence and the authority rule you matched, with the sibling you copied. (8) **The notes' words unchanged** —
state it plainly, with the assertion that proves it. (9) **Tests** and plants P1-P8, red at the base and green after,
with counts. (10) Any existing test whose expectation you rewrote, under **Keel findings expected**. (11) **Gate**
commands and results. (12) **DM golden** diff result and the grep showing the kinds are not in it. (13) **Join
notes:** the line that your join goes **after row B (`theseus-o1h3f.2`)**, and that **row F joins before you**; every
file touched with line ranges and function; the one `runtime.rs` line named; and a statement that you edited no
`rsi/` file, no `route.rs`, no `start_task.rs` and no `.tsx` file. (14) **Follow-ups.** (15) One line on the data
floor.
