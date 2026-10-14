# CLOUD_REPORT: thread-recall (T3), theseus-f2g0

Branch `cloud/20261013-thread-recall`, from `main` with T1 (`thread-record`, store format 33) joined. **No store
change**: `ThreadInfo.sealed` and `seal_pending` were T1's; "Not needed" is a ledger row plus a new META key
(`thread.seal_dismissed.<slug>`, no format bump); the four new ledger kinds are all written.

## What I found

- Recall is one pure pass (`theseus_memory::recall`) fed by the core's `Scene`; a candidate's score is the index's
  fused score, **reciprocal-rank-fusion sized (at most about 0.05)**, so the brief's `cross_recall_bar` (0.75) cannot be
  a bar on the raw score. I read it as **a share of the query's best fused score** (`top_fused`, the index's best hit
  of the query): another thread's candidate is admitted only when its score is *above 0.75 of the best*, and pays
  `cross_recall_penalty` (0.35, so ×0.65) **in the pack's rank**. This is the one interpretation the owner should hear
  about (see "Design choices").
- A candidate's thread is not on the node. `Board::of_session` only knows a thread's *current* session, so a thread's
  older sessions are found from the record's supersession link (`SessionLink.place = thread:<slug>`), and a task's
  session from its `task.parent_session`. A session's thread never changes, so the answer is kept (`thread_recall::Memo`).
- The tools block grew with `thread.read`; `tests_route_keep` and `tests_tiering` (each tuned to the request's size,
  as the earlier tool additions' notes say) needed their window up by 300.

## What I changed (commits `12e118e` the feature, `e65f313` three more tests, `6abc1fd` /why's words and the protocol guide; the report is the last commit)

**Pure (theseus-memory)**: `threads.rs` (`ThreadLens`: `judge(session, fused)` → `Pass | Sealed | Distant | Costs(f)`),
`Reason::Sealed` and `Reason::Thread`, `Asker.threads`, the rank multiplies a far thread's score by `1 − penalty`.

**Core**
- `thread_recall.rs`: the session→thread map (`Memo`), `named()` (slug, title or working-set entity as whole words),
  `ThreadScene` (taken when a recall's `Scene` is made; none while no thread exists, so a daemon without threads
  recalls exactly as before), `TurnRunner::thread_scene`.
- Provenance: `recall/render.rs::thread_mark`; `turn/recall_step.rs::hold_node` prefixes a note of another thread:
  `[m:2485de] [from thread «security fix», 2026-10-14, @24] a message from …`.
- Sealed/held threads are dropped as `sealed` in the turn's recall, the rerank (`Recalled.threads`), `refill`, and
  `memory.lookup`'s words (explicit asks keep only the seal, no distance); `memory.search` with a session does too.
  The recall narrative and `theseus memory recalled` say "🔒 N sealed hit(s) withheld" and list no sealed node.
- `rpc/thread_seal.rs`: `thread.seal {slug, reason?, redirect?}` (the answer lists what had crossed:
  "2 facts were already used in «PR round»", from the `derived_from` edges, via `recall`, into the thread's nodes,
  grouped by the session that holds the `Recall` node; `redirect` writes a harness note in each, without the notes'
  words), `thread.unseal`, `thread.seal_dismiss`, `Core::thread_recommend_seal(slug, reason)` (the entry point; sets
  `seal_pending`, writes `thread.seal_proposed`, posts a `seal_card` to the owner's DM; **the Jev question is
  T10a's, theseus-c1uz: not built**), `seal_by_word` (`/seal` or "seal this thread", exact, in a thread's session,
  before the turn; a harness note says so). `thread.new {sealed, split_from}`; a split from a sealed thread starts
  sealed; a task's session is its parent's thread (held with it); `threads::merged_seal` is the merge rule (there is no
  merge method yet; T4 calls it).
- Facts (`fact/thread_seal.rs`): `thread.sealed`, `thread.unsealed`, `thread.seal_proposed`, `thread.seal_dismissed`
  (the label row for Jev's report; META `thread.seal_dismissed.<slug>` keeps the content digest, so one proposal per
  thread until its title, goal, card or working set changes).
- `thread_read.rs`: the model tool `thread.read {thread, query?}` (private places; the card, and for a query the top
  notes through recall's pipeline kept to that thread's sessions, each `[thread «title», day, @pos]`; one sentence in
  its description). `thread_read::gate` raises a sealed (or seal-proposed) thread's read to `approve`, an approval
  question on the asking turn's call, in `toolrun/order.rs` after the whole order (no new `Layer`).
- Config `[threads] cross_recall_penalty = 0.35`, `cross_recall_bar = 0.75` (validated 0..=1, in the template).
- Protocol: `thread.seal`, `thread.unseal`, `thread.seal_dismiss`; `ThreadSealParams`, `ThreadSealResult`,
  `ThreadCrossed`, `crossed_words`; `ThreadNewParams.sealed/split_from`; the TypeScript is regenerated.

**CLI**: `theseus thread seal SLUG [--reason] [--redirect] [--not-needed]`, `thread unseal`, `thread new --sealed
--split-from`; 🔒 (🔒? while proposed) on the name in `thread list` and `thread show`; the three methods are in
`client::OPERATORS` (refused inside a job).

**Discord** (`runtime/seal.rs`): the `seal_card` to the owner's DM alone with **Seal this thread** / **Not needed**
(`seal:<yes|no>:<slug>`, `Buttons::Seal`), each press as the presser (the core judges: owner, private place); the seal's
reply lists what had crossed; `/thread list` shows 🔒.

**Not built** (say what): the TUI's dismiss key (its `/seal` works as text: the core reads the verb); a cockpit 🔒
control (no thread card exists in the cockpit yet); a Discord `/thread seal` slash command; the card is posted to the
**owner's DM**, not "in the thread" (a thread has no Discord place to post to: `outbox::virtual_place`); a merge
method. The seal-time report reads the thread's chain of sessions (current and its predecessors), not task children's
sessions.

## How I proved it

- `cargo nextest run -p theseus-memory`: 73 passed (new: `threads::tests` ×4, `tests_threads` ×5 incl. a proptest of
  100 cases: a sealed note is never admitted; `each_filter_drops_with_its_reason` gained the two new reasons).
- `rpc/tests_thread_recall.rs` (11, all pass): a cross-thread hit names its thread (header contains
  `[from thread «security fix», <day>, @<pos>]`, an unthreaded note has none); under the bar withheld (`thread`), above
  admitted, naming the thread waives; **100 seeded ambient recalls** (varying asker, message and scores) never admit a
  sealed note and count it `sealed`; a pending seal holds the thread out; "Not needed" releases it, writes the label row,
  and a second recommendation is refused until the card changes; inheritance (split, `--sealed`, a task session) and
  `merged_seal`; unseal and seal refused for a stranger in a shared channel; the seal-time report (2 facts in «PR round»)
  and the redirect note; `thread.read` gate (open → open, sealed or by title → approve), the card, a query's cited hit,
  an unknown thread; `/seal` and "seal this thread" seal before the turn (and "seal this thread now", or a session no
  thread serves, do not); the three methods over the protocol.
- Discord: `tests_gateway/seal.rs` (the card reaches ana's DM with two buttons, never #lab; a forged button in #lab is
  refused; "Not needed" writes the label row; a second card after the content changed; Seal writes it as ana),
  `runtime::seal::tests`, `runtime::thread::tests::a_sealed_thread_carries_a_lock`. CLI: `thread::tests` (🔒 in the list
  and the card, the seal report with the redirect offer), `client` job refusal covers the three methods.
- **Planted reverts** (restored, `touch`ed; `git status` clean after each):
  | planted | caught by |
  |---|---|
  | sealed filter skipped (`if t.held { return Sealed }` removed) | 6 tests: `a_sealed_threads_notes_never_come_in_ambiently`, `a_pending_seal_holds…`, `a_split_and_a_task_inherit_the_seal`, `threads::tests::a_sealed_thread_reads_itself_and_no_one_else`, `tests_threads::a_sealed_threads_note_is_withheld…`, `tests_threads::a_sealed_note_never_comes_in` |
  | bar ignored (`share > bar \|\| true`) | `threads::tests::another_threads_candidate_pays_and_needs_the_bar`, `the_bar_withholds_a_far_thread_and_naming_it_waives_that` |
  | pending seal not counted as held | `a_pending_seal_holds_the_thread_out_and_not_needed_is_remembered` |
  | penalty not paid (`Costs(1.0)`) | `threads::tests::another_threads_candidate_pays…`, `tests_threads::another_threads_hit_pays_and_is_withheld_under_the_bar` |
  | provenance mark dropped | `a_cross_thread_hit_names_its_thread` |
  | `thread.read` gate never asks | `reading_a_sealed_thread_asks_the_owner_first` |
  | naming waives nothing | `the_bar_withholds_a_far_thread_and_naming_it_waives_that` |
- No negative assertion ("nothing sealed reached X") failed once, flaky or not.

### Live check (scratch daemon, stand-in model `theseus-sim fake-model`, no Discord/web/judge, BM25 only, 8443/8444)

Script: two threads `security-fix` ("port 8443 …") and `pr-round` ("port 8444 …"). Output, abridged:

```
--- B asks (cross-thread)           (pr-round asks which port the staging proxy listens on)
request-002.json  [Recalled by the harness: 2 notes …]
(1) [m:c708c5] [from thread «security fix», 2026-10-14, @24] a message from … (as of @24)
    "PING-A the embargoed staging proxy listens on port 8443"
--- seal
🔒 The thread security-fix is sealed (embargoed CVE fix).
  2 facts were already used in «PR round»
  `theseus thread seal security-fix --redirect` leaves a note in each, withdrawing them.
--- B asks again → theseus memory recalled <pr-round session>
  admitted 0 · 0 of 1500 tokens
  🔒 1 sealed hit withheld
--- thread.read of the sealed thread waits for the owner
● 50739c  thread:pr-round  confirm thread.read: read the sealed thread security-f…
--- the owner's word seals a thread   (`ask --thread scratch "seal this thread"`)
🔒 scratch (active): scratch pad
  🔒 sealed by the CLI: sealed by the owner's word in the thread (`theseus thread unseal scratch` opens it)
--- list
🔒 security-fix  active …
```
The open thread's `thread.read` ran (`← thread.read ok`). (The first seal reported "1 sealed hit withheld" or 2
depending on which of the thread's notes the earlier recall had already put in the asking context: `sealed` is checked
before `in_context`.)

**Commands for the maintainer** (a scratch dir, not the operator's daemon): build the install, then
`B=target/debug bash live.sh`-style: `fake-model --addr 127.0.0.1:9448 --rules rules.json --dump dump/` with rules
`PING-A`, `PING-B`, `ASK-ONE`, `READ-A` (a `thread_read` call), `seal this thread`; `theseusd --config c.toml --socket
S --state-dir D` with `[model] api_base`, `[secrets] anthropic_api_key = "env:K"`, `[discord]/[web]/[judge]` off, then
`theseus --socket S thread new security-fix --title "security fix"`, `thread new pr-round`, `ask --thread security-fix
"PING-A … port 8443"`, `ask --thread pr-round "PING-B … 8444"`, wait 8 s for the index, `ask --thread pr-round
"ASK-ONE which port does the staging proxy listen on"`: the request dump's recall block must carry
`[from thread «security fix», …]`; `thread seal security-fix --reason x` must print the crossed facts;
`ask --thread pr-round "ASK-TWO …"` then `memory recalled <session>` must say `🔒 N sealed hit(s) withheld` and admit none
of A's; `ask --thread pr-round "READ-A …"` must park on a `confirm thread.read` card.

### FAST

No frame and no timer is added to a turn (the lens is arithmetic in recall's one pass; `thread_scene` reads the
in-memory board, which `warm_threads` builds after serving; a session's thread is one record read, kept). With no
thread, `thread_scene` returns none and the pass is unchanged. `tests_m3::a_plain_turn_stays_within_its_frame_budget`
and the rest of the suite pass. `theseus-sim bench lifecycle --runs 10 --check` on this 4-core VM, **debug** build
(the budgets are the owner's machine's release numbers), two runs: cold start p50 25.9 / 25.2 ms (p95 35.0 / 33.4,
budget 50 + 7), config-copy start 26.9 / 26.0, SIGKILL+restart 27.7 / 27.6, swap 30.3 / 33.9, seed 1.1 / 0.9, cancel 12.4 /
12.8: all ok. One phase missed per run, a different one each time and each by a single outlier of ten: health 5.5 ms p95
against 5 (p50 3.9) in run 1; clean shutdown with a job 156.6 ms against 104 (p50 15.5) in run 2. Nothing in the start
path or health changed here. I did not build a base binary for a side-by-side.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: the suite phase, on the final tree (head `6abc1fd`, the last gate run, 768 s), ran 4,448 tests: **4,414 passed, 34 failed**, and nothing else failed. The 34 are the 33 L1 tests
(`theseus-sandbox::contract` ×17, `theseus-sandbox::bench spawn_100`, `theseusd::sandbox` ×15: the VM runs as root,
theseus-pv6i) and the one Discord timing test below. The phases before the suite (door, keel, fmt, shape, features,
clippy `-D warnings`, cockpit, test build, reader rule) passed; after it, `protocol types` is clean (the regenerated
TypeScript is committed, `git status` clean), and the benches are skipped by `THESEUS_GATE_NO_BENCH`, so I ran the turn
and lifecycle benches by hand: `bench turn --runs 5 --burst 0 --check` passes (plain turn 4 frames, tool turn 8, batch 14,
2 syncs before the first byte; **first request 39.0 KB against its 40 KB budget**: `thread.read` costs about 0.5 KB, so
the margin to 40 is now 1 KB, worth knowing for the next tool), and the lifecycle bench as below. `cargo deny --offline check bans
licenses sources`: ok; Cargo.lock and the package locks gain nothing.

`python3 scripts/keel-guard.py`: `keel: ok … 372 files changed, 0 findings acked`. **Keel findings expected: none.**
(Two test-tuning constants rose with the tools block, as earlier tool additions' notes record: `tests_route_keep`
`WINDOW` 42,200→42,500 and `BILLED` 36,700→37,000, `tests_tiering` `context_window` 41,100→41,400. Neither is a budget,
cap or ceiling; no assertion changed.)

Known/explained failures of the suite on this VM: 33 L1 tests (`theseus-sandbox::contract` ×17, its bench's `spawn_100`,
`theseusd::sandbox` ×15, theseus-pv6i). **One outside the lists**: `theseus-discord
tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` fails here every time (21 s timeout:
`hold_writes_containing("💭 thinking")` never holds because the thinking message is already folded to "thought for 1 s"
when its create is made on this slower VM). **It fails identically on the base** (I stashed my work and ran it on
`0f4b724`: FAIL 21.7 s), so it is the VM's speed, not this change.

## Design choices the owner should hear about

1. **The bar is a share of the best fused score**, not an absolute 0.75 (impossible with RRF scores). A far-thread
   candidate at or under 0.75 of the best is withheld (`thread`), above it pays 0.35 in rank. One more reading would be
   "bar applies after the penalty"; that would admit nothing (max 0.65).
2. **The seal wins over naming**: naming a thread waives the distance, never a seal. A pending proposal holds exactly as a
   seal does (a tightening). The asking thread always reads its own notes.
3. **The seal filter is on every ambient path and on `memory.lookup`'s words** (and so the MCP memory tools' hits), not
   only the turn's recall; an owner's sessionless `theseus memory search` stays unfiltered (the owner reading on purpose).
4. **The proposal card goes to the owner's DM**, the place cards go (a thread has no Discord place).
5. **After a seal, notes already in another thread's context stay there** until a recompile; `--redirect` writes the
   withdrawal note, and the seal's reply says what crossed. A `thread.read` result already in a context is the same.
6. `thread.read` of a sealed thread is raised to `approve` by `thread_read::gate`, **not a new `Layer`** (so
   `policy.explain` does not list it as its own layer); between the gate and the run the seal could be set (a window of
   a call's plan to its run); the read then returns the notes without the question.
7. "Not needed" memory is **content-digest keyed** (title, goal, card, working set): any edit of those lifts it.

## Docs to change (I may not edit them)

- Spec Part III: a new item for theseus-f2g0 (this), its version line; `docs/spec` §3 (the threads section) for the
  distance, the seal and `thread.read`; `docs/status.md` (the roadmap row thread-recall; "Updated").
- `docs/design/roadmap-v2.md`: T3 done; T10a (theseus-c1uz) calls `Core::thread_recommend_seal`; T4's merge calls
  `threads::merged_seal`.
- Not touched: the README. AGENTS.md of core, memory, CLI and Discord are updated in the commit.

## What is left or uncertain

- TUI dismiss key, cockpit control, Discord `/thread seal`, merge method (above).
- The seal-time report does not read task children's sessions, and counts a fact once per session whatever the number of
  recalls that carried it.
- `ThreadScene` is taken at a recall's start; a seal set during the recall's (≤250 ms) wait applies from the next turn.

## Appendix: the live-check script (a scratch dir under /tmp; edit `B` to the build's directory)

```bash
#!/usr/bin/env bash
# Live check of thread recall on a scratch daemon (stand-in model, no Discord, no web).
set -uo pipefail
B=${B:-/home/user/theseus/target/debug}
D=/tmp/live
rm -rf "$D"; mkdir -p "$D/state" "$D/proj" "$D/dump"
cat > "$D/rules.json" <<'JSON'
[
 {"when": "PING-A", "text": "Noted for A."},
 {"when": "PING-B", "text": "Noted for B."},
 {"when": "ASK-ONE", "text": "Answer one."},
 {"when": "ASK-TWO", "text": "Answer two."},
 {"when": "READ-A", "calls": [{"name": "thread_read", "input": {"thread": "security-fix", "query": "staging proxy port"}}], "then": "Read it."},
 {"when": "seal this thread", "text": "Sealed, as you said."}
]
JSON
cat > "$D/theseus.toml" <<TOML
[secrets]
anthropic_api_key = "env:LIVE_KEY"
[model]
api_base = "http://127.0.0.1:9448"
[discord]
enabled = false
[web]
enabled = false
[judge]
enabled = false
[tools]
projects_dir = "$D/proj"
[memory]
mode = "live"
[index]
weights_dir = "$D/none"
TOML
export LIVE_KEY=sk-live-test
"$B/theseus-sim" fake-model --addr 127.0.0.1:9448 --rules "$D/rules.json" --dump "$D/dump" > "$D/model.log" 2>&1 &
MODEL=$!
"$B/theseusd" --config "$D/theseus.toml" --socket "$D/sock" --state-dir "$D/state" > "$D/daemon.log" 2>&1 &
DAEMON=$!
echo "model=$MODEL daemon=$DAEMON"
for i in $(seq 1 100); do [ -S "$D/sock" ] && break; sleep 0.2; done
T="$B/theseus --socket $D/sock"
echo "--- threads"
$T thread new security-fix --title "security fix" --goal "port 8443 for the staging proxy"
$T thread new pr-round --title "PR round" --goal "port 8444 for the review proxy"
echo "--- A says its port"
$T ask --thread security-fix "PING-A the embargoed staging proxy listens on port 8443" 2>&1 | tail -3
echo "--- B says its port"
$T ask --thread pr-round "PING-B the review proxy listens on port 8444" 2>&1 | tail -3
echo "waiting for the index"
sleep 8
echo "--- B asks (cross-thread)"
$T ask --thread pr-round "ASK-ONE which port does the staging proxy listen on" 2>&1 | tail -3
S=$($T thread show pr-round --json | python3 -c 'import sys,json; print(json.load(sys.stdin)["thread"]["session_id"])')
echo "pr-round session: $S"
$T memory recalled "$S" 2>&1 | tail -25
echo "--- thread.read of the open thread (the model calls it)"
$T ask --thread pr-round "READ-A read the security fix thread" 2>&1 | tail -4
echo "--- seal"
$T thread seal security-fix --reason "embargoed CVE fix" 2>&1
echo "--- B asks again"
$T ask --thread pr-round "ASK-TWO which port does the staging proxy listen on" 2>&1 | tail -3
$T memory recalled "$S" 2>&1 | tail -25
echo "--- thread.read of the sealed thread waits for the owner"
( $T ask --thread pr-round "READ-A read the security fix thread again" > "$D/read2.out" 2>&1 & )
sleep 4
$T confirm list 2>&1 | head -8
$T status 2>&1 | head -5
echo "--- the owner's word seals a thread"
$T thread new scratch --title "scratch pad"
$T ask --thread scratch "seal this thread" 2>&1 | tail -3
$T thread show scratch | head -4
echo "--- list"
$T thread list
echo "--- request dump (recall blocks)"
for f in "$D"/dump/*.json; do
  python3 - "$f" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
for m in d.get("messages", []):
    c = m.get("content")
    if isinstance(c, list):
        for b in c:
            t = b.get("text", "") if isinstance(b, dict) else ""
            if "Recalled by the harness" in t:
                print(sys.argv[1].rsplit("/", 1)[1], t[:700])
PY
done
cat "$D/read2.out" | tail -3
$T shutdown
kill "$MODEL" 2>/dev/null
wait "$DAEMON" 2>/dev/null
echo "--- done"
```
