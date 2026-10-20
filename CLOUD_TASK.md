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

**Other changes in flight.** Several other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Merged into `main` tonight (your clone holds them if your base is main): turn-end facts, the shared tool catalog
(speed-send-path), the store's one blocking wait, the fleet's full-screen view, a self task's acts in its own tree,
the task's home thread, the owner's needs-you cards, and voice at the join (theseus-yf9ct) with its two join fixes.

Other cloud sessions like you, each on its own branch: the ontology in core (R2, reviewed, joining next); the job
truths; the release build's size (release-size); the timing flakes; the thread bench's threads arms. And this batch,
fired together: home-split (a place's home split into threads), onto-wire (the ontology's in-loop wire, stacked on
R2), voice-aec (voice layer 3, echo cancellation) and voice-jev (voice layer 4, a Jev judgment on unclear transcripts).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (your base, the ontology-core row, is at 46 as you clone it; this row should need no bump), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, on top of its first commit (CLOUD_TASK.md), whose base your task names. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-onto-wire`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: onto-wire, Jev in the inbound loop grows the ontology: onto.v1 in the batch, writes at the act band, cards, undo (theseus-xzxqv, with the rest of theseus-s1vkl)

Branch: `cloud/20261019-onto-wire`. Every commit's subject carries `theseus-xzxqv` (or `theseus-s1vkl` for step 1's
pack commits). Deadline for the report: 5 hours after you start.

**Base: stacked on the ontology-core row (R2), not on main.** This branch starts from `1b663c65`, the code head of
`cloud/20261019-onto-core` (R2, theseus-e3mjc: origins jev/sweep/dream built, operator wins, the kinds rows, `examples`
and `mined_from`, `Ontology::undo`, the `ontology.undo` ledger kind, store format 46), with R2's CLOUD_TASK.md and
CLOUD_REPORT.md commits left out. R2 is reviewed and joins first; this row joins right after it. So the store is at
46 as you clone it, and this row should need no bump. R2's base is an older main (`6fc5884b`): the owner's
needs-you cards and the task home are not in your tree. Use the DM card path your tree has (as the people proposals'
cards do); the maintainer moves it onto the needs-you cards at the join. Don't pull main in.

**The design** is not in your tree: read it with `git fetch origin lane/design-jev-ontology && git show
FETCH_HEAD:docs/design/jev-ontology-growth.md`. Your scope is its sections 3.1, 4.1-4.4 and row R3 in section 10, plus
R1's unfinished part. The owner approved the design and both of section 11's questions on 2026-10-19 at 12:14 (the
naming call: yes; images and the private mark: yes, but images and the mark are row R4's, not yours).

## What to build
1. **R1's rest (theseus-s1vkl): the `onto.v1` pack.** `crates/theseus-judge/packs/onto.v1.toml` with section 4.3's
   text questions (`topic`, `project`, `event`, `resource`, `capability`, `expertise` as Choice over at most 20
   candidates plus `new` and `none`; `<kind>_name` as Choice over at most 10 mined names plus `none`; `still_<kind>`
   Noul per current membership, at most 5; `kind_of_thing`; `personal` and `elicits` recorded only, no effect yet).
   Image questions are R4's: leave them out. An `OntoInput` builder (`crates/theseus-judge/src/builders/onto.rs`) with
   the per-kind candidate picker (session's categories, then the thread's, then a cheap word match on name and
   description, then most recent; at most 20) and a name miner (a new `mine.rs`: thread and project titles, repository
   and directory names, hosts and services, capitalised terms, file paths; at most 10, deduplicated). The state stays
   under about 3,000 tokens; questions whose evidence is absent are left out. Scripted answers in `fake.rs`.
2. **The wire.** `onto.v1` joins the inbound batch (`crates/theseus-core/src/judge/inbound.rs`) beside `classify.v3`
   and `role.v1`: one state, one request, cost split by question count as the batch does. Never in `route.v6`'s
   request. The turn waits exactly as today (the `thread` answer and the route verdict, at most `route_wait_ms`).
   `project` is asked only when the thread has no project. `categorize.v1` keeps running unchanged in shadow.
3. **The sink** (`judge/sink.rs`, a new `judge/onto.rs`): at the act band (>= 0.80) it writes the membership with
   origin `jev`, its confidence and `added_by = jev:<judgment id>`, in the judgment's frame with its row, through R2's
   checks (the operator's entries always win; a refusal is a row, never a panic). `new` at the act band plus a
   `<kind>_name` at the act band mints the category in the same frame. The confirm band (0.55 to 0.80) sends one DM
   card asking and writes nothing; below it, a row only. The reply's footer gets `filed: topic › <parent> › <name>`
   with `(new)` on a mint. A mint sends one DM card with an undo button; a plain membership sends none.
4. **The naming call**, off the turn's path: when Jev says `new` at the act band and no mined name fits, one call to
   `[ontology] name_profile` (default `haiku`) for three names and a one-line description, the way `people.v1`'s
   extract step does it; the session's next `onto.v1` offers them. At most once per session per kind per day, priced
   and counted like every judgment. A test with the fake model shows the cap.
5. **RPCs and CLI**: `ontology.undo` (R2's `Ontology::undo`, an `ontology.undo` row labelled `wrong` for the pack's
   learning ledger), `ontology.kind.add`, `ontology.kind.retire`; `theseus ontology undo|kind add|kind retire`. Protocol
   types in a module file of their own (lib.rs is at its ceiling); regenerate the TypeScript.
6. **`[ontology]` config** (`crates/theseus-core/src/config/ontology.rs`, in the example-config template with its
   tests): `judge = true` (the rollback: off, the batch is byte for byte today's and nothing is written), `act = 0.80`,
   `confirm = 0.55`, `name_profile = "haiku"`, and `request = "batch"` with `"own"` as the latency fallback (onto.v1 in
   a third request the turn never waits on).
7. **The latency measurement.** `judge.call` rows already carry each call's time. Add a small reader
   (`theseus judge latency --pack inbound --since <date>` or the nearest existing command's flag) printing the batch's
   p50 and p95 per day, so the maintainer can compare a week before and after; if p95 grows by more than 50 ms the
   owner flips `request = "own"`. The week itself is the maintainer's.

**Jev's API, for reference** (a live probe, 2026-10-19): Jev takes PNG and JPEG images only behind the header
`TypeSafe-Danger-Zone-Images: true` (else 400); images go in a top-level `assets` map `{ "<uuid>": { "type": "image",
"format": "png", "data_base64": "..." } }` referenced from the state; 280 to 1,568 px per side, never resized; 16 MiB
per image, 32 MiB per body; one bad image fails the whole request. That is R4's work: send no image here, and keep the
builder's shape able to carry an assets map later.

## Tests (each with a planted revert; the first is the key test)
- KEY: with the fake judge scripted `topic = new` at 0.9 and `topic_name = <a mined name>` at 0.9, an inbound message
  ends with the session a member of a newly minted category (`added_by jev:<id>`, the ledger row in the judgment's
  frame), the footer line on the reply, and one mint card. Plant: make the sink skip the write -> the test fails on
  the missing membership.
- A confirm-band answer (0.7) writes nothing and sends one confirm card. Plant: lower the act band to 0.6.
- `ontology.undo` through the RPC restores the member list byte for byte and retires the minted category. Plant: skip
  the retire.
- The operator's membership is never overwritten by a Jev answer (a disagreement row instead).
- A text message's batch state is byte for byte today's plus the onto fields (a golden). With `[ontology] judge =
  false` the batch request is byte for byte today's. Plant: leave onto.v1 in when off.
- The turn's wait is unchanged: with the fake judge holding onto.v1's answer 2 s, the turn's request leaves at the
  same virtual time as without it (paused clock). Plant: await the sink before the turn.
- The builder round-trips; the miner on five message shapes (a path, a host, a repo URL, capitalised terms, plain
  chat); a scripted judgment parses into the three bands; the naming call's once-per-day cap.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-judge -p theseus-ontology -p theseus-core -p theseus-protocol -p theseus --all-targets -- -D warnings`.
- `TZ=America/Phoenix cargo nextest run -p theseus-judge -p theseus-ontology -p theseus-core -p theseus-protocol -p theseus`;
  `tests_first_request` on the defaults (nothing new before a request leaves); the example-config template tests.
- The full gate once before the report if time allows; if not, say so.

## After the join (the maintainer's, not yours)
Install, watch a day of DMs for memberships and mints with their cards, try one undo, and run the latency reader daily
for a week. Give the exact commands in the report.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

