# Cloud report: `categorize.v1` in shadow, and parked tasks in health (step 28b, theseus-vug.1)

Branch `cloud/20261004-categorize-shadow`, from `main` at 3add3f5 (plus the task commit f46c944). One step, one code
commit: **1cefb9e** `judge: categorize.v1 in shadow, its proposals, and parked tasks in health (theseus-vug.1)`.
Started 08:45 UTC, report at about 09:55 UTC.

It is one commit, not three. The trigger, the proposals, and the parked-task block all add to the same shared files
(the protocol's `lib.rs` method table and `HealthResult`, `ledger.rs`, the generated TypeScript, the CLI's
`main.rs`), so I proved it with one gate.

## What I found

- 23a's `JudgeService::after_turn` runs after the turn's last frame and after `session_hold` is dropped (turn.rs,
  about line 1195). Anything spawned from there is already off the turn's path and outside its trace.
- `theseus_judge::Ask` had no `id`. I added it under the six-session convention: `pub id: Option<String>`,
  `Ask::new` sets `None`, and `Judgment::pending` uses it when it is set.
- The service had no route to the place rule or the ontology, and both live on `TurnRunner`, which `Core` owns
  directly (not behind an `Arc`). I added one field, `categorize: categorize::Point`, which holds a
  `OnceLock<Weak<Core>>` and the set of sessions being decided. I also added `JudgeService::attach(&Arc<Core>)`, called
  once in `Core::build`. This is the only new reach into the service. It holds the core by `Weak`, as the Traps
  section of `crates/theseus-core/AGENTS.md` asks.
- The categorize pack's option ids are topic local ids (`harbor`), as in its fixture. A topic whose local id is
  `none` or `new_topic` would collide with the pack's own options, so it is never offered.
- Health had no `tasks` block. `Kernel::open_executions` reads by the store's state terms (O(open)), and a question's
  age is its action's `planned_at_ms`, a point read by correlation id.

## What I changed (1cefb9e)

- **theseus-judge**: `Ask.id`. New test: `a_judgment_takes_the_id_its_dispatch_minted`.
- **The trigger and dispatch**, in a module of its own: `crates/theseus-core/src/judge/categorize.rs`.
  - `judge/mod.rs` gained the `mod` line, the `WIRED` line (`categorize.v1` in shadow), the `Point` field, and one
    call in `after_turn`. 23a's loop path is otherwise unchanged.
  - The point runs at a turn that the baseline ended with no tool calls (`stop_reason == "no_tool_calls"`, the same
    exchange end as `loop.v1`).
  - It skips a task (the `task` flag), and it skips any session whose `class_of` is not private, checked in the
    spawned task.
  - The decision reads the session's mark, META `judge.categorize.<session>` =
    `{judgment, through, through_ms, at_ms}`. Then it reads only the session's records after `through`
    (`scope_after`) and asks `due()`.
  - A human message is a `UserMessage` with origin `Operator`. A task's brief and report, a wake's note, and the
    harness's notices are not human messages.
  - Once a judgment is due, the point builds the state, writes the blob, reserves from the shadow budget, writes the
    mark (one small META frame, off the turn's path), and calls Jev.
  - No topic declared means no judgment.
  - No trace mark is written: the dispatch runs outside every turn, which the convention allows. The id is still
    minted by the core, and the mark names it.
- **How I read "the first exchange end after 30 minutes' quiet"**: take the run of human messages that began the
  latest exchange (its first message, plus any that followed with nothing in between). The exchange is quiet if that
  run came 30 minutes or more after the node before it. When the run is the first thing after the mark, the time
  compared is the mark's message's time (`through_ms`). It also needs at least one human message since the mark. A
  session's very first message is never quiet. The mark moves past the run once it is judged, so one quiet brings one
  judgment.
- **The input**: the session's title and its last ten human messages. A count trigger has ten after the mark already;
  a quiet trigger reads the session in full, which is rare.
  - Candidates come from the ontology's snapshot (`held()`, built only if no reader built it yet): the categories of
    every interpreted kind the kinds table lets the operator assign, which today means topics. Each description is
    sent as `name: description`.
  - **With more than 50 topics**: the session's own topics come first, then topics with a description, then the rest,
    each part ordered by name. The judgment's context records `candidates` and `candidates_left_out`.
  - Memberships: up to 5 interpreted ones, newest first, for `still_member`.
- **Proposals**: `crates/theseus-core/src/rpc/proposals.rs`.
  - `ontology.proposals {session_id?, limit?}` does one scan of scope `judge:categorize`, which holds both the
    judgments and their labels. It lists each answered judgment whose `topic` is `new_topic` or names a topic the
    session is not in now, has no operator label, and is newest first. Each entry carries its confidence, band,
    session title and topic name.
  - `ontology.proposal.accept {judgment, topic?, description?, note?}` and `ontology.proposal.reject {judgment, note?}`
    go through `judge_act(Act::Ontology{method, what})` first, before anything is read.
  - An accept writes the operator-origin `MemberList` through `ontology::Board::write`, together with its
    `ontology.membership` row and the `judge.label` row, in **one frame**.
  - For `new_topic`, `--topic` is required. It names an existing topic, or a new one that is created (with `--desc`)
    in that same frame.
  - A reject writes only the label.
  - A judgment that already has an answer is refused.
  - The label row is a new `LedgerKind::JudgeLabel` (`judge.label`) and a new fact, `fact::judge::JudgeLabel`, with a
    narrative line. It is keyed `lbl_<uuid>` and scoped `judge:categorize`. Its fields follow §2.5: `id`, `judgment`,
    `pack`, `question: "topic"`, `label` (`accepted`/`rejected`), `answer` (what Jev chose), `topic`,
    `source: "operator"`, `who`, `via`, `weight: 1.0`, `note`.
- **Parked tasks**: `crates/theseus-core/src/parked.rs` and `Core::tasks_health`.
  - Health gains `tasks: {parked: [ParkedTask{task_id, short, execution_id, title, state, blocker, detail,
    since_ms}]}`.
  - A task is **not** parked when it has a running turn or a queue place, `resume_pending`, `report_wakes`, a wake on
    actions (a job), a due time, or another execution; when it waits on input but holds a pending wake of its own
    (37b); or when an approval or budget question was asked less than 24 h ago.
  - It **is** parked when it waits on input with no wake (`input`), was stopped (`stopped`), has an approval or budget
    question 24 h old or older (`approval`/`budget`), is `blocked`, or waits with no wake at all (`nothing`).
  - `theseus health` prints `tasks parked: N` and one line per task (`crates/theseus/src/render/parked.rs`). It prints
    nothing when no task is parked.
- **CLI**: `theseus ontology proposals [--session S] [--limit N]`, `theseus ontology accept JUDGMENT [--topic NAME]
  [--desc TEXT] [--note TEXT]`, and `theseus ontology reject JUDGMENT [--note TEXT]`. Accept and reject joined
  `client::OPERATORS`, so they are refused inside a job, and its test covers them.
- **Protocol**: new types in `ontology.rs` (`OntologyProposal*`) and `health.rs` (`TasksHealth`, `ParkedTask`). Three
  method names and the `tasks` field went into `lib.rs` (2554 to 2566 lines, under its ceiling of 2618). The
  TypeScript is regenerated, and the new types are in `ts.rs`'s list.
- **No store bump.** Only ledger rows and a META record under a new key were added; no stored record gained a field.
  `MANIFEST_FORMAT` is unchanged.
- **No config keys.** The thresholds are constants, and the pack's mode and sample use the existing
  `[judge.packs."categorize.v1"]`.
- **Changes in shared or neighbouring files**:
  - `crates/theseus/src/main.rs`: three `OntologyCmd` variants.
  - `rpc/server.rs`: three names on the ontology dispatch arm.
  - `rpc/mod.rs`: the `attach` line.
  - `turn.rs`: untouched.
  - Two of 23a's tests and `crates/theseusd/tests/judge.rs` asserted that health's pack list was exactly
    `["loop.v1: …"]`. They now expect `categorize.v1: shadow` as well.
  - `crates/theseus-core/AGENTS.md` gained the judge, categorize and parked bullets.

## How I proved it

All the new core tests are in `crates/theseus-core/src/tests_categorize.rs` (11 tests), and run against `FakeJev`:

- **The `exchange_end` trigger**:
  - `the_exchange_end_trigger_counts_human_messages_since_the_last`: 9 human messages, each followed by a reply and a
    harness-relayed message, do not trigger; the 10th does.
  - `the_exchange_end_trigger_reads_thirty_minutes_quiet`: a 30-minute gap triggers and 29 minutes does not; a burst
    of messages counts from its first message; a session's first message is never quiet; the mark's time is used
    right after a judgment; nothing new since the mark means no judgment.
- **Candidates from the kinds table**: `candidates_come_from_the_kinds_table` shows topics only (no given channel, no
  topic named `none`), ordered by name. With 63 topics it offers 50 and reports 13 left out: the session's own topic
  first, and the undescribed ones dropped first.
- **One judgment and its proposal**: `ten_human_messages_bring_one_judgment_and_its_proposal`.
  - Nine messages bring no judgment and no connection to Jev. The tenth brings one row, keyed by its id, with
    `categorize.v1`, `shadow` and `exchange_end`, trigger `count`, and 2 candidates.
  - The mark names the judgment.
  - The state blob holds the 10 messages and no `current_memberships`.
  - Jev was offered `harbor`, `garden`, `new_topic` and `none`.
  - `ontology.proposals` lists `topic:harbor` at 0.93, band `act`.
  - An 11th message brings no second judgment.
  - Health shows `categorize.v1: shadow`.
- **Accept and reject**:
  - `accept_writes_an_operator_membership_and_its_label_in_one_frame`: `frames_appended` grows by exactly 1. The
    membership is `topic:harbor` with origin `operator`. The label row is keyed `lbl_…`, labelled `accepted`, source
    `operator`, for the session. The proposal is gone afterwards, and a second answer is refused.
  - `a_new_topic_accept_makes_its_topic_and_a_reject_writes_its_label_alone`: without `--topic` the accept is
    refused. With `--topic moorings --desc …`, one frame creates the topic, the membership and the label. A reject
    writes only its label, with its note, and adds no membership. Proposals are listed newest first.
- **A refused accept writes nothing**: `a_refused_accept_writes_nothing`. A non-owner on a shared channel, and the
  owner on a shared channel, are both refused with `approval::Refusal`, for accept and for reject. No `judge.label`
  row, no membership, and the proposal stays. ("Nothing" here means no label and no membership; `judge_act` still
  writes its own act-refused row, as every refused act does.)
- **A shared place's session and a task are never judged**: `a_shared_place_and_a_task_are_never_judged`. A session
  bound to a shared channel sends 12 messages: no row and no Jev connection. Then a second service is attached to a
  core with the pack off, and handed the same due exchange end as a task's: no row and no connection. Handed as a
  conversation's, the same end is judged, which shows the task flag is what gates it.
- **A slow or failing Jev changes no turn**: `a_slow_or_failing_jev_changes_no_turn`. The 10th (judged) turn, with
  Jev Down, Slow(10 s) or Malformed, matches the judge-off baseline: the same request bytes, output, stop reason,
  loops and cost. Its own frame count (the trace's `frames`) matches and is at most 5. It takes under 3 s. Each
  failure is recorded with its class and leaves no proposal.
- **Parked detection on scripted states**: `parked_tasks_are_detected_on_scripted_states` covers every case listed
  above, including a question at 23 h (not parked) and at 25 h (parked, with detail "an approval unanswered for 25 h"),
  and the ordering of the list. `health_lists_no_parked_task_for_a_conversation` checks that health returns an empty
  list on a store with only a conversation.
- **The CLI**: `render::parked::tests::…` covers the health lines; `client::tests` covers accept and reject being
  refused in a job while `proposals` is allowed.

**Under load**: four `while :; do :; done` loops at nice 0 (started from a script file and killed by their pids), and
the tests at `nice -n 19`. Each run covered `tests_categorize`, `tests_judge`, the id test and the render test, 22
tests per run. 5 runs, 5 times 22/22 passed (53 to 62 s each).

**Planted reverts** (each file restored with `cp`, then `touch`ed, then `git status` checked):

1. *Count every message instead of human ones* (`is_human` changed to "any node but a tool call"):
   `the_exchange_end_trigger_counts_human_messages_since_the_last` failed with `left: Some(Count), right: None`
   ("nine human messages"). The quiet test failed too, because replies counted as part of the run.
2. *Accept skips `judge_act`*: `a_refused_accept_writes_nothing` failed at `unwrap_err()` on an `Ok`. The accept
   went through and wrote `topic:harbor` with origin operator, plus a label.

## The live check (the maintainer's)

Use a scratch daemon on a fresh state dir, with Discord and the web off and the judge on. In a copy of your config
note (the sparse one is enough), set:

```toml
[judge]
enabled = true
[secrets]
jev_api_key = "op://<vault>/<Jev API key item>/notesPlain"   # and your model key as usual
[discord]
enabled = false
[web]
enabled = false
```

```bash
S=/tmp/theseus-28b; mkdir -p $S
theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &   # note its pid
T="theseus --socket $S/sock"
# 1. two topics
$T ontology topic add harbor --desc "Harbour work: tides, moorings, the harbour master"
$T ontology topic add garden --desc "The garden: irrigation, planting plans, seasonal chores"
# 2. ten messages about moorings in one CLI session
SID=$($T ask --json "My mooring line chafes at low tide. Why?" | jq -r .session_id)
for q in "Which knot holds best on a cleat?" "How much scope for a mooring in 4 m of water?" \
  "Should I add a snubber?" "Is chain or nylon better for a swing mooring?" "How often should I inspect the shackle?" \
  "What does the harbour master inspect?" "How do tides change the mooring's swing circle?" \
  "Do I need chafe guards at the fairlead?" "What size mooring buoy for a 9 m boat?"; do
  $T ask -s $SID "$q" >/dev/null; done
sleep 5
$T judge log -n 5                # a `categorize.v1 (shadow)` line for $SID, topic=harbor <conf> <band>
$T ontology proposals            # jdg_…  $SID -> topic:harbor (harbor)  <confidence> <band>
# 3. accept it
$T ontology accept <jdg_… from above>
$T ontology member $SID          # topic:harbor  topic  operator as of …
$T ledger -k judge.label         # one row: label accepted, source operator, judgment jdg_…
# 4. fifty topics: one judgment's output tokens against its reservation
for i in $(seq 1 48); do $T ontology topic add "t$i" --desc "a scratch topic number $i"; done
SID2=$($T ask --json "Moorings again: what is a pennant?" | jq -r .session_id)
for i in $(seq 1 9); do $T ask -s $SID2 "Mooring question $i: how long should a pennant be?" >/dev/null; done
sleep 5
$T ledger -k judge.call --json | jq '.rows[-1].data | {usage, reserve_micros, cost_micros, ctx: .context.candidates}'
#   expect candidates 50; compare usage.output_tokens and cost_micros with reserve_micros (price.rs's open question
#   on a 52-option Choice's allowance)
# 5. a task waiting on an approval is not parked
$T policy tighten proc.run
$T ask "Start a task that runs \`ls\` with proc.run and reports what it saw."
$T tasks                         # the task waits on an approval
$T health                        # no `tasks parked:` line: its question is younger than 24 h
$T shutdown
```

What each step should show is in the comments. The 24-hour case is held by the offline test
`parked_tasks_are_detected_on_scripted_states`. A decline (`theseus confirm <id> --decline`) lets the model carry on, so it does not park the task; the offline tests are the parked cases' proof.

## What is left or uncertain, and choices for the owner

- **The mark is a META record written in a frame of its own** at dispatch: one small frame per `categorize.v1`
  judgment, off the turn's path. It never rides the sink's frame, which was left alone as asked. A judgment that is
  paused by the shadow budget moves no mark, so the next exchange end tries again; that costs a skip count, never a
  call. A Jev failure after dispatch does not roll the mark back, so a failing Jev is not asked again on every
  exchange end.
- **No topic declared, no judgment.** With zero topics, `new_topic` would be the only meaningful answer. I judged
  that not worth a call. Say if the owner wants `new_topic` discovery on an empty ontology.
- **A quiet trigger reads the whole session** to collect the last ten human messages. A count trigger reads only what
  follows the mark.
- **`ontology.proposals` scans the whole `judge:categorize` scope** on each call. That is fine at today's volume (one
  judgment per 10 human messages). The learning ledger (25c) may want an index of labels by judgment.
- **The accept refuses a session that already holds 3 topics.** The ontology's write rules (`AtMost(3)`) refuse it as
  invalid input, and the proposal stays.
- **No trace mark.** The categorize dispatch is outside every turn. If 23b wants the dispatch visible in a session's
  waterfall, the mark would have to ride the next turn.
- **The cockpit's Ontology view (21c)** should show a proposals panel fed by `ontology.proposals`. For each proposal:
  the session (title, linking to the session), the proposed topic (name and id) or "a new topic", Jev's confidence
  with its band as a chip, and when it was judged. Then Accept and Reject buttons calling `ontology.proposal.accept` /
  `.reject`. Accept on a `new_topic` proposal opens a small form: a topic name picked from existing topics or typed
  new, plus a description. Both buttons take an optional note. The panel refreshes on `ledger.tail` rows of kind
  `judge.label` / `judge.call` in scope `judge:categorize`. Health's `tasks.parked` belongs in its health or tasks
  board, as a row per task with its blocker.
- **Docs for the maintainer to write**:
  - The spec's Part III item for 28b.
  - `docs/status.md`: the roadmap row and the recently landed step.
  - `docs/design/m5-judgment.md` §2.12, which should name the CLI and protocol surfaces instead of the Observatory
    and note the trigger's reading above.
  - §2.13's protocol row, which gains `ontology.proposals` / `.proposal.accept` / `.proposal.reject`, and its health
    row's `tasks.parked`.
  - §2.5's `judge.label` row: it is now written, keyed by the label's own id.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on 1cefb9e failed in the suite only: 1974 tests run, 1940 passed (1 slow, 1
flaky), 34 failed, 17 skipped. The failures:

- **33 sandbox tests**, the known root case (theseus-pv6i). The failing tests are `theseus-sandbox::contract` clauses,
  `theseus-sandbox::bench spawn_100`, and `theseusd::sandbox` L1 tests. Each says "the daemon runs as root, and Linux
  exempts root from RLIMIT_NPROC".
- **`theseus-core tests_output::the_cores_output_matches_its_golden`**: a time-zone difference. The `wake.at` preview
  prints the local UTC offset: `+#:#` on this UTC VM, against the golden's `-#:#`. It passes with
  `TZ=America/New_York`, and this change touches nothing in it.
- **The flaky test**: `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` failed twice and passed
  on its third try. It is on the flaky list.

The phases after the suite, run by hand on the committed tree, all passed:

- fmt, shape, clippy `-D warnings`
- the test build
- the generated protocol types: unchanged from the commit
- `theseus-sim bench turn --check --runs 5 --burst 0`: plain turn 5 frames (budget 5), tool turn 9 (budget 9), ok
- `cargo deny --offline check`: advisories, bans, licenses and sources ok, after `cargo deny fetch` ran in setup

The lifecycle and jobs benches were skipped (`NO_BENCH`).
