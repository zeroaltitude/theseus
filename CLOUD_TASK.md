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

Merged into `main` tonight (your clone holds them): turn-end facts, the shared tool catalog (speed-send-path), the
store's one blocking wait (speed-idle-size), the fleet's full-screen view, a self task's acts in its own tree, the
task's home thread, the owner's needs-you cards, voice at the join, and the ontology in core (store format 46).

Other cloud sessions like you, each on its own branch: the install build's size (release-size); the DM home split into
threads (home-split); the job truths; the ontology on the wire (onto-wire); per-thread benches (threadbench-threads);
timing flakes; voice's echo cancelling (voice-aec) and voice asking Jev (voice-jev); and two beside you in this batch:
self-repair-fixes: a self task in its own tree, the trees root readable, tree cleanup and `self retry` (crates/theseus-core/src/rsi/); agent-answers: who answered (an agent's CLI vs the owner), money and AWS answers the owner's own (judge_act, approval.rs, the digest).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 46 as you clone it), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-dm-cards-fix`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: dm-cards-fix, a due question's card always reaches the owner's DM, and an expired one offers "Ask again" (theseus-27p6w)

Branch: `cloud/20261019-dm-cards-fix`. Every commit's subject carries `theseus-27p6w`. Deadline for the report: 5
hours after you start.

**Why.** Tonight on the owner's host a self-improvement session asked three questions in a row (an `fs.glob`, then an
`fs.grep` and an `fs.list`, each needing the owner's approval). The owner's DM got no answerable card for any of
them. The only post was "⌛ **Expired**: nobody answered within 15 minutes, so it did not run · Find files
matching ...", with no buttons. In the owner's words: "there is no way to do it in the dm he sent".

**The cause, found by reading the code and the host's state** (prove it with the first test below before you fix it):
`Core::escalate_questions` (`crates/theseus-core/src/needs_you.rs`) finds the questions due (planned at least `[notify]
escalate_after_secs` ago, 60 s with no screen open; not expired; no card; not pinged), then asks the pacer
(`pacer.rs`, `Outbox::pacer`) and returns nothing when its window is closed. The window is `[notify]
unprompted_pace_secs`, 1,800 s (30 minutes) by default, and it is shared: any `needs_you`, `initiative` (the morning
note, a consolidation finding) or `digest` post closes it, and the outbox rereads those posts' times at a start, so
a daemon restart does not reopen it. A question lives `confirm_ttl_ms`, 15 minutes by default. So a question that
falls due while the window is closed waits for a window that opens up to 30 minutes later, and it expires first:
its card is never posted and only the expiry's notice reaches the DM. Everything else checks out: the owner's DM is
bound, the self session's questions are in `Kernel::pending_confirms` (`theseus history` lists them with `theseus
answer <id> approve`), and `[notify] dm_cards` is on. Tonight's sequence had an unprompted post shortly before
the first question, then each question timed out behind the window.

## What exists (read these; lines move)
- `crates/theseus-core/src/needs_you.rs` (`tick_questions`, `escalate_questions`, `post_needs_you`, `card_body`,
  `header_body`) and `needs_you/words.rs`; `pacer.rs` (`UNPROMPTED`, `Pacer::open`/`took`); `outbox.rs` (`has_card`,
  `has_post`, `pinged`, where `pacer.took` is called as a post is written and as the index is read); the `silent`
  field a post body carries (the batch's per-question cards already post with `"silent": true`).
- `crates/theseus-core/src/config/notify.rs` (`escalate_after_secs`, `escalate_with_screen_secs`,
  `unprompted_pace_secs`, `dm_cards`, `wait_ms`).
- The expiry: `crate::ask::expiry_of`, the kernel's expiry of a planned action, `fact/answer.rs` `QuestionExpired`,
  and the settle's words in `crates/theseus-discord/src/render.rs` ("⌛ **Expired**: nobody answered within ... so it
  did not run") and `render/ask.rs` (an `owner_ask`'s expiry). The card's buttons and how a press reaches the core:
  `crates/theseus-discord/src/runtime/` (the card and settle paths), `Core::card_closed`, `reconcile_cards`.
- Tests: `crates/theseus-core/src/tests_needs_you*.rs` (or wherever `escalate_questions` is tested: grep for it), and
  theseus-discord's card tests.

## What to build
1. **Reproduce it.** A test with the pacer's window closed by an unprompted post (as `tests_work_digest.rs` and
   `tests_initiative.rs` do with `outbox.pacer.took(now)`), a question from a `"self"`-principal session (the
   pipeline's or the backlog's rig, or a session opened as `Authority { principal: "self" }`), then
   `escalate_questions` at each minute until its expiry. Today it posts nothing. Commit it with the fix, not alone.
2. **The fix: the pacer governs the ping, never the card.** A question that is due posts its answerable card to the
   owner's DM on that tick, whatever the window. With the window open it pings as today and takes the window. With
   it closed it posts silently (`"silent": true`, as the batch's cards do) and does not take the window; the board and
   every screen show it as now. Several due together keep today's shape (a header and a card each; the header pings
   only when the window is open). The one-post-per-question rule, the index (`pinged`), the settle on close, and
   `dm_cards = false`'s old post are unchanged (with `dm_cards` off, the old one-line post follows the same rule:
   posted on time, silent when the window is closed). Fix the module docs of needs_you.rs and pacer.rs and
   notify.rs's `unprompted_pace_secs` line to say the window paces the buzz, and theseus-core's AGENTS.md if it
   describes the pacing. Design note for the report: the owner set the one-buzz-a-window rule on 2026-10-13; this
   keeps it (a silent card is no buzz). Name the alternative (a question that would expire before the window opens
   pings anyway) as the owner's call, not built.
3. **"Ask again" on an expired card.** When an owner-answerable question expires (an approval of a tool call, a
   budget question, an extension's ack, an `owner_ask`), its settled card says it expired as today and offers one
   button, "Ask again". The owner's press (judged as an answer is: the owner's own account on a bound transport) puts
   the same question to the session again: for a tool call, the session gets a continuation turn with a one-line
   note ("The owner asked again for: <the card's line>"), so the model plans the call again and its new question gets
   a card of its own at once (the owner's press makes it prompted, so neither the wait nor the window holds it).
   Offer it only while the session can still take a turn (its execution is live or continuable: read how a DM reply
   continues a session); otherwise the expired card has no button, as now. Pick the simplest correct mechanism that
   the existing card and press paths allow, and say what you chose and why in the report. No new store record kind
   if an existing one serves; if you add a field or variant to a stored record, follow the store's version rule.

## Tests (each with a planted revert; the first is the key test)
- KEY: the reproduction above, now passing: with the window closed by an unprompted post 5 minutes earlier, a self
  session's approval question posts one card to the owner's DM within one tick of falling due, `silent: true`, with
  its approve/decline buttons (the card body's actions), and the pacer's `last_ms` is unchanged; with the window
  open, the card pings and takes the window. Plant: put back the early `return 0` on a closed window -> the test fails
  with no post.
- One post per question still: a second tick, and a restart (the index reread), post nothing more for it. Plant:
  skip the `pinged` check -> a second card.
- `dm_cards = false` follows the same rule (posted on time, silent when closed), byte for byte today's body otherwise.
- The expired card of an owner-answerable tool-call question carries "Ask again"; a press by the owner continues the
  session with the note and the new question is carded at once; a press by anyone else is refused as an answer
  would be; an expired question of a session that cannot continue has no button. Plant: drop the button from the
  settle -> the test fails.
- theseus-discord renders the button and routes its press (its own test beside the card tests).

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-core -p theseus-discord --all-targets
  -- -D warnings`.
- `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(/needs_you|pacer|notify|card|expir|ask/)'`, `cargo
  nextest run -p theseus-discord`, then `TZ=America/Phoenix cargo nextest run -p theseus-core -p theseusd`.
- `python3 scripts/keel-guard.py --base <your base> --head HEAD --quiet` prints nothing. Existing pacer tests that
  assert "no post while the window is closed" now assert "a silent post, the window untouched": keep their assertion
  counts (each removed check gets its replacement for the same case), so the guard stays empty. If one cannot keep
  its count, put that change in a commit of its own and give its `Keel:` line in the report for the maintainer.
- Then the full gate line from the preamble once at the end, if time allows; say if it did not.
- The live check for the maintainer: with a digest just posted, a session's approval question appears in the DM as a
  silent card with buttons about a minute later, and pressing Approve runs it; let one expire, press "Ask again", and
  a new card appears.

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

