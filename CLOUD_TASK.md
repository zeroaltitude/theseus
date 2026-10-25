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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3b-voice-length`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
This row needs no store change (prompt text only). Do not bump it.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At the
base: `crates/theseus-discord/src/runtime/voice.rs` 2,032 lines; `crates/theseus-core/persona/default.md` 5 lines.

**Your lane: `integ/p3-infra`.** Its other row, fired at about the same time on the same base: `p3b-flux-follow`
(theseus-82rbm: the pump's `Event::Streamed` arm in `crates/theseus-discord/src/runtime/voice.rs` and a new
Discord-side test for it, `start_turn` in the Flux engine, `silence_ticks` in `flux_path.rs`). **Shared with you:**
only `crates/theseus-discord/src/runtime/voice.rs`, where you change only the `FRAMING` const (:87-91) and its doc
comment, and `p3b-flux-follow` changes only the pump's `Event::Streamed` arm and adds tests in a new or existing test
module: keep your edit to those lines so the two merge cleanly. Other lanes of this wave (separate branches) work in
the cockpit, the ontology code and the trust marks; the wave-1 lane works in recall, the catalog and the index. Stay
out of all of them. Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3b-voice-length, spoken replies sized to what the moment needs, and an audit of every voice cap (theseus-dgbcx)

Branch: `cloud/20261024-p3b-voice-length`.

Every commit subject carries `theseus-dgbcx`. Deadline for the report: about 2 hours after you start.

**What the owner said (2026-10-24, after a voice retest in which the agent explained that its shortness came from a
standing instruction):** relax the shortness constraint to "be focused in the context of what is needed in a voice
chat" rather than "say less". The owner's own installed persona already carries the new line (changed by hand that
day); the built-in default and the code still carry the old one.

**The line the owner chose** (use it verbatim in the built-in persona):

> Spoken aloud: focused on what the moment needs, as long as the answer deserves; plain sentences that sound right
> said out loud (no lists, tables or code), brief for a quick back-and-forth.

### Part 1: the two places that say "one to three short sentences" today

1. `crates/theseus-core/persona/default.md` :3, the `Voice.` paragraph: replace `Spoken aloud: one to three short
   sentences.` with the line above, keeping the rest of the paragraph (Terminal, A direct message, A shared channel,
   MCP) byte-identical.
   - This text is rendered into the system prompt, so the golden
     `crates/theseus-core/tests/golden/private_requests.txt` (:34 and :90 hold the old sentence) moves with it. Its
     test is `crates/theseus-core/src/tests_open_view.rs` :32-37; rewrite the golden with `THESEUS_GOLDEN=write`
     on that test only, and commit ONLY the lines your change moves (`git diff` must show the two persona lines and
     nothing else). Grep every other golden for the old sentence (`rg -n "one to three short sentences" crates docs`)
     and move each the same way. A golden whose line changes only because the persona text changed is the one
     expected golden edit of this row: say so in the report, with each file and line.
   - `crates/theseus-core/src/tests_persona.rs` :207-224 asserts the default text contains `"Spoken aloud:"` (keep
     that prefix) and is under 1,024 bytes (a cap: do not raise it; the new text must fit; `turn.rs` :129 uses
     `DEFAULT_TEXT` as the persona).
2. `crates/theseus-discord/src/runtime/voice.rs` :87-91, the `FRAMING` const that opens every voice turn's input:
   `Answer in one to three short sentences of plain speech: no lists, tables, code, markdown or long numbers.` This
   is the stronger cap, because it is repeated on every turn. Rewrite its middle sentence to the same rule as the
   persona: focused on what the moment needs, plain speech, no lists/tables/code/markdown/long numbers, brief for a
   quick back-and-forth. Keep its first sentence ("they hear your reply, they don't read it") and its last ("If you
   were cut off, don't assume they heard the rest."). Keep it a single bracketed line.
   - `crates/theseus-discord/src/runtime/voice/tests_heard.rs` :438 asserts `FRAMING.chars().count() < 300`. That
     bound is a cap: do NOT raise it. Your new text must fit under it.
   - `FRAMING` is also used by `voice.rs` :1770 (a test), `voice/tests_heard.rs` :426/:435 and
     `voice/tests_status.rs` :400. Where a test builds its expectation from the const, it follows the change. Where a
     test spells the OLD text out literally, update that literal to the new text (that is the expectation following
     an owner-asked text change, not a weakening): list each such edit in the report.
   - Update the doc comment above `FRAMING` if it describes the length rule.
   - `docs/design/m7-surface.md` :799 and `docs/spec/28-part3-item-216.md` :43-44 quote the old framing line. In the
     design note add the new wording as a new dated clause ("Since 2026-10-24, theseus-dgbcx: ..."); do not rewrite
     the old quote (it is the record of what was true). Leave the spec's quote as history unless it states the rule
     as current, in which case add one dated line after it.
- **The owner's yes for this text change** is in the issue's notes (2026-10-24 13:49): "We should relax the shortness
  constraint to 'be focused in the context of what is needed in a voice chat' rather than 'say less'". If
  `keel-guard.py` flags the persona or `FRAMING` edit as a loosened rule, do not work around it: paste the flag in the
  report beside that quote, so the maintainer can add the signed `Keel:` ack at the join.

### Part 2: the audit (half of this row)

Find EVERY other cap on the voice path that could shorten a spoken reply or make the model say less. For each one,
record in the report: file:line, what it limits, its value, whether it applies to conversational replies or only to
other speech (task status lines, notices, the heard line), and what you did.

A read-only look before this task found these; CONFIRM each (do not trust this list) and find any it missed:
- `crates/theseus-discord/src/runtime/voice_status.rs` :45 `SPOKEN_MAX` 200 chars, the board line before a voice
  turn's words (clipped at ~:183): status only.
- `crates/theseus-discord/src/runtime/voice/tasks.rs` :48 `SPOKEN_MAX` 220 chars, a task's question (one sentence,
  ~:266) and done/failed report (two sentences, ~:349/:353): task reports only.
- `crates/theseus-discord/src/runtime/voice/notes.rs` :14-22, the heard line's `FIRST_WORDS` 40, `QUOTE` 80,
  `MAX_NOTES` 3, `MAX_LINE` 400, `KEPT_TURNS` 32: context notes, not output.
- `crates/theseus-voice/src/engine.rs` :250-258 `CutWhy` (`voice.cut`): `Words` (barge-in), `Superseded`,
  `CallEnded`; no length reason.
- `engine.rs` ~:1677-1690 `reply()` speaks all of `speakable()`; `crates/theseus-voice/src/sentences.rs` splits and
  swaps a table or code block for a pointer sentence (no length cut); `speech_fits` (`voice.rs` ~:1400) is a money
  budget.
- The voice turn submits with `profile/model: None` (`voice.rs` ~:752-754), so it gets the normal
  `max_output_tokens` (`crates/theseus-core/config/theseus.example.toml` :49-53, default the model's ceiling); the
  toml's `[voice]` section (~:949 on) has no token or length setting.

Places to search for anything missed (not exhaustive):
- `SPOKEN_MAX` (the owner's note says it is for task status lines only: confirm, with the line that uses it);
- `voice.cut` and its reasons (a reply cut by the caller speaking is not a length cap: say which reasons exist);
- any `max_tokens` / output-token budget set for a voice turn or a voice profile (the voice crate, the Discord voice
  runtime, theseusd's voice code, `crates/theseus-core/config/theseus.example.toml`'s voice and profile sections);
- any truncation of the text sent to text-to-speech (a char or sentence limit before synthesis);
- any other prompt text that tells the model to be short in a call (`rg -n -i "short|brief|concise|one sentence|two
  sentences|terse" crates/theseus-discord/src/runtime crates/theseus-voice* crates/theseusd/src` and the persona and
  skill files under `crates/theseus-core/`).

What to do with each cap you find:
- **A prompt line that tells the model to keep conversational spoken replies short:** align it with the owner's rule
  (focused on the moment; brief for quick back-and-forth), as in Part 1.
- **A numeric cap on conversational spoken replies (tokens, chars, sentences):** do NOT raise or remove it in this
  row (that is a Keel change and needs the owner's yes). Name it in the report under `OWNER CALL: <cap, value,
  file:line, what it cuts, a proposed value>`.
- **A cap on other speech (status lines, notices, the heard line's 400 chars):** leave it; list it.

### Part 3: tests (red first)

1. A test in `crates/theseus-core/src/tests_persona.rs` (beside :207-224) that
   the built-in persona's `Voice.` paragraph carries the owner's line and no longer says `one to three short
   sentences`. Red at the base.
2. A test in `tests_heard.rs` (or beside it) that `FRAMING` no longer caps the reply at a sentence count (no
   `one to three`, no `short sentences`) and still says plain speech with no lists, tables, code, markdown or long
   numbers, and still says the cut-off sentence. Red at the base.
3. Keep green: `tests_heard.rs`'s framing tests (:426 and :435 build the expected input from `FRAMING` itself, so they
   follow the const), the `< 300` bound, every persona and golden test after the golden moves.
- Planted revert P1: restore the old persona sentence: test 1 red. P2: restore the old FRAMING middle sentence: test
  2 red.

## Not in this row

No numeric cap raised or removed. No change to the heard line, the cut logic, TTS, the voice provider or any Discord
text-channel message. The owner's installed persona (`~/.theseus/persona/theseus.md`) is on his machine, not in the
repo: nothing to do for it.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. You touch
  only voice text, not the DM task flow, but check: run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If any change
  you would make touches a message of that flow (wording, layout, order, timing), STOP that part and write
  `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling (the `< 300` framing bound included); do not
  edit `scripts/keel-guard.py` or its tests; rename no test. The only golden edits are the persona-text lines named
  above: list them. Before the report run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and
  paste its output.
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Fixtures use invented content
  under `example.test`. Write "the owner".
- **Red first;** right over compatible.
- **The gate** (read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` and `crates/theseus-discord/AGENTS.md`
  first): `cargo fmt --check`, `cargo clippy` on the touched crates with `-D warnings`, `cargo nextest run` (or
  `cargo test`) for `theseus-core` (with `TZ=America/Phoenix`) and `theseus-discord`; then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), any ceiling, every OWNER CALL. (2) **The
two lines:** the persona's and FRAMING's old and new text, FRAMING's new length in chars. (3) **The audit:** a table,
one row per cap found (file:line, limits what, value, conversational or other, action). Say plainly whether any
numeric cap still shortens conversational spoken replies. (4) **Goldens moved:** each file and line, and that each
moved only for the persona text. (5) **Tests** and plants P1-P2, red at the base and green after. (6) **Gate**
commands and results. (7) **Keel** output. (8) **DM golden** diff result. (9) **Join notes:** every file touched, and
the exact `voice.rs` lines (so the lane's other row, `p3b-flux-follow`, merges cleanly). (10) **For the owner's live
check:** one voice call question that used to get a clipped answer (for example "walk me through what changed today")
and what a right answer now sounds like. (11) **Follow-ups.**
