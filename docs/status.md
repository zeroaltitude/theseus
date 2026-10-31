# Theseus: status and roadmap

_Updated 2026-10-28 21:27 MST. Version 1.0.3 (v1 patch3), tag `release-1.0.3`; v1 patch4 (version 1.0.4) is under way; the design document is at v0.92._

**The picture:** [the development timeline](assets/timeline/theseus-timeline.png), from conception to now and what is ahead ([SVG](assets/timeline/theseus-timeline.svg)).

**The north star:** [the plan](design/north-star-plan.md) to match or beat Claude Code at coding and delight, then crush it on memory, personality, speed, scale and task horizons ([PDF](design/north-star-plan.pdf)).

**Self-improvement (RSI):** [the plan](design/rsi-plan.md) for Theseus to build its own code, yardsticks and backlog: act first and tell the owner after, inside a hard keel that still needs his yes; loop 1, the nightly dream, switched on for v1 on 2026-10-18 ([PDF](design/rsi-plan.pdf)).

**The killer features:** [one page](design/killer-features.md) with a short section per feature (speed, self-improvement, parallel tool calls, task horizons, memory, personality, remote runtimes, speculation, Discord), each linking its plan ([PDF](design/killer-features.pdf)).

**Remote runtimes:** [the design](design/remote-runtimes.md) for the CLI, the daemon and the index each on its own machine, over TLS 1.3 with mTLS, off by default; decided, and planned after v1 ([PDF](design/remote-runtimes.pdf)).

**Speculative activity:** [the design](design/speculation.md) for Theseus to guess the likely next ask, work ahead in a sandbox while the machine is idle, and offer the result, never applied without Use; decided, and planned after v1 ([PDF](design/speculation.pdf)).

This page changes with every step that lands. The [README](../README.md) stays the same and links here. For the
full record of each step (what it built, how it was proven, and where it diverged from the plan), see Part III
of [The Ship of Theseus](the-ship-of-theseus.md). For the whole plan, see [the roadmap](design/roadmap-v2.md), and
for the week after v1, [the v1.1 roadmap](design/roadmap-v1.1.md).

## v1 patch3: trust, privacy and content

**v1 patch3 (version 1.0.3)** is the principles migration: the review of 2026-10-27 held the trust, privacy and content
machinery against four of the owner's principles (a trusted machine, so notify and block only the irreversible; the
session is the true record; models of restricted origin are the operator's control, by labels and not by proof; less
code) and rebuilt it in twelve rows (Beads `theseus-gyjyo`). Part III Item 259 is the as-built record, with each row's
divergence from the plan. What it changes:

- **Four labels on a session** (`injection`, `secret`, `confidential`, `personal`), each removable with one tap, read by
  **one switch**, `[content_based_security] cn_models = always | follow_flags | never`. A request to a model of China or
  unknown origin is refused when the session carries a disqualifying label (`follow_flags`, the default), always
  refused (`never`) or always sent (`always`). That is one read of the session's record: the chokepoint no longer reads
  the transcript to prove every item was judged. A turn bound for such a model waits at most `wait_ms` (4 s) for the
  checks still out for its session, then goes on the labels it has.
- **Labels come from three places.** Jev's content pack (`content.v3`) asks four questions of every item that enters a
  session. Two rules are free and exact: an item holding a `[privacy] sensitive_names` word raises `confidential`, and a
  Google result raises `confidential` (a sensitive calendar, `personal`), with no model call. A Remove sticks: an item
  that entered before it cannot raise that flag again. A task, a retry, a routine's run and a draft start with their
  parent's `secret`, `confidential` and `personal` labels.
- **The classifier learns from the owner's taps.** Every Remove writes a `judge.label` row (the chip's "wrong" does the
  same), a label left standing a week counts for the pack, and replay serves every stored content judgment (all 28 of the
  owner's, for a quarter of a cent). `content.v3` reads who wrote an item, where, the session's directory, whether the
  tree is one the owner lists as open source and the owner's own paragraph: precision at the raising bar 0.956 against
  v2's 0.875, recall 0.985 against 0.955, on a set of v2's 125 cases plus 26 new ones, and the gain is the wording more
  than the new fields.
- **The record is raw.** What a tool printed is stored as it was: the node, the kept output, the spool file, imports, the
  ledger's narrative lines. The vault's own values (and their base64, percent and JSON-escaped forms) are replaced by
  `[redacted:<name>]` on the copies that leave for a model, for Discord and for Jev: the request, each post, the Jev state, the
  push line. `theseus inspect` says which secret went in which request to which model (`request.redacted`), and
  `[privacy] redact_outbound = false` sends them as written. Nothing guesses a key's shape any more.
- **Reading the web no longer holds the session.** The `external` marker stays on a result, and the `injection` label is
  the one place outside text can stop a later call: a call that is not a local read of files and code is refused until
  you remove it (`/trust`, `theseus content remove`, or the button). `[policy] external_text` and `external_programs`
  are retired and a config that names one fails to load, saying so. A host an L1 call names beyond `[sandbox] egress`
  runs with a notice.
- **Earned autonomy and scoped grants are gone** (about 6,600 lines, which could never fire under
  `enforcement = notify`): no weekly card, no `theseus policy earned`, `revoke` or `grants`.
- **Config.** `[content]` is now `[content_based_security]`, a hard error naming each key's new home;
  `[privacy]` gains `open_source_paths` and `redact_outbound`; `session_flags = false` with `cn_models = "follow_flags"`
  refuses to start. The owner's config needs the `external_text` line under `[policy]` removed before this installs, and
  his own `open_source_paths` and `context`, which `content.v3` uses when he writes them.

Where it stands: R0, R1 and R2 reached `main` on 2026-10-28 and were installed (#98); the other rows joined the
principles lane, which merged to `main` the same day after a gate of the merged tree. What the owner still decides is listed
at the end of Item 259 (Google content and a model of EU origin; the two exits the substitution cannot see; a locked
approval card and the DM golden's `/trust` line; an AWS L1 job and the egress list; whether a Remove of a names
label holds against the next item naming the word).

Next is **v1 patch4** (tag `release-1.0.4`).

## v1 patch2

**v1 patch2 (version 1.0.2)** is tagged `release-1.0.2`: the second fix release after v1, every row of its bundle joined
and installed. The tagged build is the `release` profile (fat LTO). What it changed (most rows' own entries are under
"Recently landed"):

- **Flux is the default listener** (theseus-v774k): a voice call hears through Deepgram Flux, whose own model decides
  where a turn ends, in place of Nova-3 and the pause rules. Aura-2 stays the voice and voice notes stay on Nova-3; a
  call whose Flux socket never opens falls back to Nova-3 and says so once.
- **Live learning** (theseus-6f8t0): when a private conversation the owner is alone in pauses, one cheap call reads the
  owner's own lines and keeps what should hold from now on as private, unconfirmed notes (`theseus card observed`).
  Each night the card turns them into questions (Add, Pattern, Stale), answered Keep, Edit, Not this or End it, and up
  to 3 a day reach the owner's DM as cards. Nothing becomes a record without the owner's answer.
- **The Operations page** (theseus-jid7t.1 to .6): the cockpit's Agenda grows into `/ops`, one place to see and act on
  every task, session and model. It has a needs-you queue, a command bar, and Board, Threads, Graph and Timeline lenses
  (the Timeline with the owner's calendars; sensitive calendars are marked and masked under redact). Pick many and act
  on them at once, drag to rehome or reschedule with Undo, and use the inspector, the Join composer, the model and mode
  menus and the New task sheet. **Its tidy-up** after the owner's walkthrough (theseus-g00vv): one state model, so the
  queue, the Board, the Graph and Threads agree on what waits, works, rests or is done; a hold on outside text that
  waits on nothing is a notice; collapse, fullscreen, scrolling and the Timeline's hours work.
- **Personas in the ontology** (theseus-utfnl, theseus-8n4yj): a `persona:<key>` category per persona, its soul in a
  small file capped at 2,000 tokens, Jev's per-message persona choice over them (with `none`); `[personas]` is retired.
  In a shared place where the default persona is not allowed, none engages the built-in one.
- **The soul editor keeps the soul** (theseus-lky63): the cockpit's editor opens with the persona's current soul and
  writes the whole file, warns before a write that shrinks it, refuses a write over a newer one, and can put the
  previous soul back.
- **Root-owned binaries** (theseus-qbi30): the binaries the user service runs are owned by root, so a job running as
  the owner's user can no longer replace the daemon, and the Systems page's "jobs can write it" alarm clears.
- **The send path at 29.6 ms** (theseus-nsp9l.4): a new compilation's request no longer waits on its system blocks'
  blob syncs (three fsyncs of about 7 ms each). A typed prompt leaves as a request in 29.6 ms, from 52.5, inside its
  30 ms target.
- **Routing, effort and long turns** (theseus-f9qa8, theseus-ujzuh, theseus-u5a37, theseus-goq94, theseus-2uy56,
  theseus-yct32, theseus-rmjqr, theseus-5mkz1, theseus-5pg8u, theseus-3v37h): route.v8 reads open-source work's public
  signs; Jev's effort can reach `max` on hard work, an `xhigh` needs a sure answer, and a sticky effort ends early on a
  sure later verdict. A turn can carry a deadline: it is told the time and the CPUs it really has, replans at half its
  time and stops to report at 0.85. A call past the provider's 600 s timeout is retried once instead of ending the turn.
- **Calls cut at their cap** (theseus-2c6iz, theseus-1iqg0, theseus-kf4pf, theseus-opypg, theseus-j06y6, theseus-yym79,
  theseus-o6ifc, theseus-ohiu8, theseus-wdzmj, theseus-7q9wt): the compaction summary, the judge audit, the pack writer,
  the small forced-tool calls and the exam reviewer turn thinking off where they can and say plainly when an answer was
  cut; a tool is forced only where thinking is off; the learning writer asks for low effort; dream guidance lets a
  category whose model always thinks wait instead of asking again every night.
- **Threads, the chat and Discord** (theseus-n6mkd, theseus-7nx13, theseus-nde5n, theseus-zcn80, theseus-gg78p,
  theseus-hk0dk, theseus-axno0, theseus-capcs, theseus-s48k6, theseus-002ie, theseus-nxnqc): a thread is named by the
  turn's own words, so another thread's notes stay out of a turn; title hits count as naming; the chat follows a
  thread-placed turn's answered question; the approval card says why it waits; a message asking for tasks starts
  tasks; a needs-you card prints its held clause once; and the DM task flow's golden test no longer flakes under load.
- **The owner card and the ontology** (theseus-ly75u, theseus-0ly2a, theseus-f61px, theseus-7e7hc, theseus-3ys6k): the
  card no longer reads an assistant's setup notes as facts about the owner or one task's instructions as a lasting
  preference, and a kept record can say where it holds; a kept kind-name answer is dropped once its cluster changes.
- **Operations follow-ups** (theseus-i08xr, theseus-n17am, theseus-uy015, theseus-dyukm, theseus-xzf71, theseus-5iqf8,
  theseus-b09do, theseus-vhtuv, theseus-afvat, theseus-bglzl, theseus-arblf, theseus-gbmz1, theseus-izyws): shell verbs
  (`theseus task rehome`, `task reschedule`, `sessions model`), `job.stop` only from a trusted channel, a bounded
  calendar cache, the Graph's keys and folds under its cap, and `drive.read` answering a file over 1 MB with its
  metadata.
- **Benches, tests and the public tree** (theseus-zeyr3, theseus-1j7ib, theseus-it4ta, theseus-nbgi4, theseus-q2npl,
  theseus-jkd9q, theseus-cuv2o, theseus-y8kux, theseus-1kr2g): the recall bench prices from the catalog; the memory
  exam runs with routing off and stops if another model answers; the old organisation names are gone from fixtures and
  tests; the keel guard reads a value's shape and a guard that holds braces; one response's fanned-out programs run at
  once again.

Its numbers ([the omnibus](benchmarks/omnibus.md)). No coding benchmark has run on a 1.0.2 build yet; the latest are
the runs after v1, now in the omnibus's figures (Figure 2, the frontier and the cost figure):

- **Coding, HIGH** ([the 10-21 rerun](benchmarks/2026-10-21-terminal-bench-rerun-high.md)): 77.2% [71.8, 81.8] on the
  89 tasks × 3, against v1's 73.0% and Claude Code's 75.3%, at $0.155 a solve.
- **Coding, MAX** ([the 10-23 retest](benchmarks/2026-10-23-terminal-bench-retest-max.md)): 71.6% [61.0, 80.3] on 27 of
  the hard 30 × 3, against Claude Code's 76.5% on the same tasks, at $0.470 a solve. The six HIGH tasks that timed out
  on 10-21 solved 4 of 18 ([the retest](benchmarks/2026-10-23-terminal-bench-retest-high-timeouts.md)).
- **Speed** (an A/B on the build machine, 10-23): the send path 29.6 ms with the fix against 52.5 ms without it, level
  with the stopping point's 29.4 ms.
- **All the model calls of every run** now come to about $934.

Known limits carried to v1 patch3: the owner's live checks of Flux, live learning and the Operations page are still to
come; Flux does not yet fall back to Nova-3 when its listener cannot be built (theseus-zlmom); two owner-card writes can
still race, so an answer can be reverted (theseus-kxb8d), and the DM's Edit button reads a proposal without the expiry
write (theseus-5pypl); on the Operations page, a resting conversation still counts as done (an owner call,
theseus-zo72s), an event begun on an earlier day opens the Timeline at 00:00 (theseus-ka0lv), and Dismiss all reports
its refusals and disables itself (theseus-jo84p); some daemon tests are owed (theseus-tx1vj, theseus-x1x7s); live checks
of the task tools run on a scratch daemon only (theseus-xiaq4); the bench adapter's deadline ignores Harbor's timeout
multiplier (theseus-hxukw); the daemon's first idle minute reads high because of the fresh binary's page cache, which
the bench should leave out (theseus-nsp9l.5, theseus-d4yx2); the daemon binary is still over its 60 MB budget; and at
top effort on the hard tasks Theseus still trails Claude Code.

Next is **v1 patch3** (tag `release-1.0.3`).

## v1 patch1

**v1 patch1 (version 1.0.1)** is tagged `release-1.0.1`: the first fix release after v1, every row of its bundle joined
and installed. The tagged build is the `release` profile (fat LTO). What it changed (each row's own entry is under
"Recently landed"):

- **Google Workspace tools:** mail, calendar, drive, sheets and slides, each REST call recorded as a `google.called`
  row; a send, an invite or a share asks first.
- **Threads and their routing:** a clear message lands in the right thread 91-93% of the time on threadbench (78% before)
  and asks for 5% of them (11% before); every work stream gets its own thread.
- **One message convention** for everything Theseus says unasked: a kind tag (ℹ️ FYI, ✋ Action, ❓ Decision, ⚠️ Alert), an
  operational title and what triggered it (`docs/messages.md`). The Discord DM task flow is locked by one golden test.
- **The China-origin model rows** under the owner's data rule: Jev's data question keeps private work off them, and
  text naming a `[privacy] sensitive_names` entry goes only to models of US origin, an allowlist that fails closed.
- **Effort on evidence:** effort applies at its own bar below the model-switch line, so MAX now reaches `xhigh`.
- **Fixes from v1's benchmarks and the owner's walkthroughs**, among them the card hot loop (an idle daemon at ~80% of a
  core) and a note Theseus writes itself never posted cut off.

Its numbers ([the omnibus](benchmarks/omnibus.md)):

- **Coding, HIGH** ([the 10-21 rerun](benchmarks/2026-10-21-terminal-bench-rerun-high.md)): 77.2% [71.8, 81.8] on the
  89 tasks × 3, against v1's 73.0% and Claude Code's 75.3%.
- **Coding, MAX** ([the 10-23 retest](benchmarks/2026-10-23-terminal-bench-retest-max.md)): 71.6% [61.0, 80.3] on 27 of
  the hard 30 × 3, level with the 10-21 rerun on the same tasks (−2.5 points, −13.6 to +7.4), at $0.470 a solve, still
  the least of the harnesses. The six HIGH tasks that timed out on 10-21 solved 4 of 18
  ([the retest](benchmarks/2026-10-23-terminal-bench-retest-high-timeouts.md)); their timeouts are the tasks' own.
- **Speed** (quiet machine, 10-22): steady idle memory 40.4 MB for the daemon; the send path 49.5 ms, level with v1 on
  the same window but over its 30 ms target (below).

Known limits carried to v1 patch2: the send path's ~21 ms over target (theseus-nsp9l.4), the daemon's first idle minute
at ~52 MB before it settles at 38 (theseus-nsp9l.5), the release binary at 91.8 MB against a 60 MB budget, a thread
that can still pull in another thread's notes (theseus-n6mkd), and Jev rarely asking for `max` (theseus-ujzuh).

Next is **v1 patch2** (tag `release-1.0.2`).

## v1

**v1 (version 1.0.0) was cut on 2026-10-18** and tagged `release-1.0.0`: features frozen at `351159fc`, which the
benchmarks built; the cut adds two fixes joined after the freeze (a self task's file tools in its own tree, and a voice
call that hears the owner who was already talking) and the cut's docs. The tagged build is the `release` profile
(fat LTO). Its numbers, all from 2026-10-18 at the freeze ([the omnibus](benchmarks/omnibus.md)):

- **Coding** ([Terminal-Bench 2.0, high effort](benchmarks/2026-10-18-terminal-bench-v1-high.md)): Theseus as shipped
  73.0% at $0.136 a solved task, the least of four; Claude Code 75.3%, level task by task (−2.2 points, 95% interval
  −7.9 to +3.0); OpenCode 70.8%, Pi 70.4%. On the 30 hard tasks at each harness's top effort
  ([MAX](benchmarks/2026-10-18-terminal-bench-v1-max.md)), Theseus solved 67.8% against Claude Code's 75.6% (−7.8
  points, −18.9 to +1.1), at $0.36 a solve against $1.92.
- **Speed** ([head to head](benchmarks/2026-10-18-head-to-head-speed.md), [gate](benchmarks/2026-10-18-gate-bench-v1.md)):
  a ready prompt in 39 ms against Claude Code's 498, 10 ms of CPU a turn against 1,655, 70 MB idle against 360; every
  lifecycle budget met.
- **Kill -9** ([the family](benchmarks/2026-10-18-async-kill-family.md)): 0 effects done twice in 12 trials, 10
  resumed; one trial left a process running, the known limit to fix next.
- **Recall** ([the full run](benchmarks/2026-10-18-recall-full-run.md)): 73.1% over 600 turns, against 56.9% for Pi
  and 50.7% for Claude Code; 27 of 46 across sessions, against 0 and 1.
- **Self-improvement** is on: RSI loop 1's nightly dream (fix mining and pattern mining, $10 a branch and $30 a day),
  with its kill switch and the keel.

Next is **v1 patch1** (tag `release-1.0.1`, version 1.0.1), whose main task is the Google Workspace tools.

## On, and still being hardened

Six features in v1 are on and still being hardened. The [README](../README.md) lists them as features 7 to 12. The
owner's rule (2026-10-18) is that a feature that exists ships on, with its broken parts said plainly, and the fixes
follow.

- **Jev's judgments** (`route.v8`, `classify.v4`, `rerank.v1`, `recall_cross.v1` and `security.v3`, live). Each message a person sends
  gets its model and effort, its class and its thread from Jev, waited for at most 300 ms on the owner's daemon (200 by
  default); Jev's persona pick is recorded in shadow, and recall's order waits for `rerank.v1`. At the v1 benchmarks
  (2026-10-18) Theseus paid the least of four harnesses for a solved task at high effort, level with Claude Code on
  the score. Still rough:
  - It sometimes calls a hard task routine and sends it to Haiku. On the 30 hardest tasks, every turn it called
    routine failed, and it never chose `max` effort where `max` was allowed. Since v1 patch1 a turn the route placed
    whose loops keep failing asks Jev again and moves up, as a task does; the next benchmark run measures it.
  - `route.v6`'s data verdict called open-source work `private` in about 60% of turns. `route.v7` rewords the question
    (judge what the message carries, not the place); a probe of 14 made-up messages read every open one `open` at the
    act band, against none on v6. Since v1 patch2 `route.v8` is live: it names the public signs of open-source work and
    no longer treats an unshown owner as a reason for `unclear`. Nothing acts on the verdict yet. The floor is the
    `[privacy] sensitive_names` list, which goes only to models of US origin.

  The fixes go toward v1 patch1 (theseus-a1wvq).
- **The compiled context** (`theseus why`, `theseus project attach`). Each request is compiled from the store, with
  the project's slice of an attached session in its last block, and the v1 patch1 project is attached to the owner's
  sessions. Still rough:
  - A turn Jev detours to a small model as quick or trivial is compiled apart from the session: a quick detour
    carries its recall and the project's slice rides any detour (theseus-pa1xx, theseus-v4u66), and its first request is
    held to the place's budget as sent (theseus-3bs4e). `theseus why` names a detoured turn, and `context.explain` shows
    its request's parts from its `context.detoured` row (theseus-r5ydi). A trivial detour still carries no recall and
    no context files.
  - The slice shows the first "done when" line and a count of the rest.

  The fixes go toward v1 patch1.
- **Voice** (`/join` in a Discord voice channel). `[voice] enabled` is off by default, and it has been on in the
  owner's config since October 3. In the v1 check on 2026-10-18 it joined, heard and transcribed the owner, spoke long
  answers well, and counted two barge-ins, one of them resumed. Still broken:
  - A call can go mostly deaf. One six-minute call decoded about 7 s of the owner's audio, and the deaf-call rule did
    not fire. The leading guess is that the call's end-to-end encryption never switched on at Theseus's end, so the
    owner's encrypted audio was dropped.
  - Some transcripts come out clipped.
  - The short greeting clips.
  - 2026-10-22: the owner's re-test heard sentences chopped at a pause and the first "go ahead" clipped. A quiet listener
    now closes an utterance after `[voice] pause_ms` (1200 ms, was 700), the silence before Theseus's first sound is
    `[voice] lead_in_ms` (600 ms, was 400), a cut-off clip's fade is 10 ms, and an unfinished sentence waits
    `[voice] merge_ms` (1500 ms) for its rest (theseus-bp0lp). Awaiting the owner's live check.
  - 2026-10-22: between a reply's sentences the seam keeps sending silence (a 700 ms warm track after each clip), and a
    sentence that finds nothing warm gets `[voice] gap_lead_in_ms` (200 ms) of silence first (theseus-9d9aq). Awaiting
    the owner's live check.
  - A voice turn has no clock.
  - Discord's typing indicator can stick after a deaf turn.

  The fixes go toward v1 patch1 (theseus-sro04).
- **Activity threads** (`theseus thread`). They are on by default, with up to 50 live, and five have been live on the
  owner's daemon since 2026-10-18.
  - The router (the `thread` question of `classify.v4`, fused with a free pre-score) is tested on 40 scripted
    messages with a scripted Jev. When it cannot tell it asks in one line, listing every live thread up to four, and
    offers a new thread when Jev suggests one or when stickiness alone pointed at the last thread and the message names
    things no thread holds. A message sent from a directory under none of a thread's roots is no longer placed in that
    thread by stickiness or by a move the owner once made (theseus-vw8k7); naming its PR, branch or path still is.
  - A thread's working set grows from the paths and names its turns use. A turn that ran at home but wrote only one
    thread's files becomes that thread's memory (`theseus thread unclaim` undoes it). Two threads can be linked so
    each recalls the other's notes, and Jev offers a link once when one keeps drawing on another.
  - In a thread, notes of other threads are kept out of recall unless near the top; `recall_cross.v1` reads the near
    misses and lets in those about the same subject, and a question across all the place's work gets a digest of the
    other threads instead.
  - The thread benchmark (`bench/threads/`) is built and has not been run on the real model.
  - A Discord thread for each thread needs `discord_channel`, which is off by default.
  - A place's home that grows past `[threads] working_tokens` (60,000 tokens a request unless set) tells the owner once,
    in the DM, with its size and `theseus thread split home@<place>` (undone by `thread unsplit`). Nothing splits on its own.
- **Personas** (`[personas.<name>]`). Five are set up on the owner's daemon.
  - Addressing a persona by name (`name,`, `@name` or `/as name`) and binding one to a channel work live.
  - Jev's per-message pick (the `persona` question of `route.v5` and `route.v6`) runs in shadow
    (`[threads] persona_engage = "shadow"`) until its holdout.
  - The personality exam (`bench/personality/`) is built and not yet published.

## Working today

As installed on the operator's daemon on October 6 at 22:13 (install #9; the store at format 23). Install #5
(October 5, 16:48) put the v1 build there, install #6 (October 6, 10:12) turned the S3 backup on, and the import of
his earlier history followed install #8 (17:22). What installs #7 to #9 added is listed after this list, and what has
joined since, for install #10, after that. Theseus has been feature complete (v1-beta) since October 6 at 22:04.

- **Since the joins of October 17 and 18 (installs #37 to #39; Items 247 to 253).** Tasks are one thing on every surface:
  - **A task is found later and joined.** `theseus tasks --ended` and a word search find any task, open or ended;
    `task show` reads one whole. The owner's message to a running task steers into it from the pane, the chat, the
    cockpit's task page and its Discord thread, and the report ends `(you joined: N messages)`; a message after the task
    has ended goes to main, with one line that says why.
  - **A task's model comes from its brief.** route.v6 reads the brief when a task starts, in place of inheriting the
    conversation's model; a routine-coding brief starts on Sonnet.
  - **One progress line everywhere:** `step 3 of 5 · $0.40 of ~$0.60 (limit $2) · ~6 min left`, paused first when paused,
    in the CLI, the chat, the cockpit, the herdr panes and Discord's forum tags.
  - **herdr pane answers.** A pane answers an `owner_ask` by its number or in the owner's own words, and prefix+p pauses
    or resumes its task.
  - **A post, a channel and a forum entry for each task on Discord,** with a war room for guests, the owner's reactions
    to pause, resume, stop or retry, an offer chip ("Make this a task"), spoken questions in a call, and a digest at 08:00
    and 18:00.
  - **One name for each thing.** A task is a "task"; `theseus answer` is the one verb for an answer (`confirm` and
    `executions` are gone, and an old config key fails naming the new one); the cockpit, Discord, the TUI and the CLI say
    the same words.
  - **Labels keep a session off models of restricted origin** (v1 patch3, the first section). A session carries up to
    four labels, raised by Jev's content pack and by two free rules (a `[privacy] sensitive_names` hit and a Google
    result), each removable with one tap; `[content_based_security] cn_models` reads them. Nothing walks a request for
    a name any more, and the old US-only allowlist is gone.
    Self-improvement (RSI loop 1's builds, scorer and pattern miner) is built, and on since 2026-10-18 at 12:25.
- Conversations in Discord (DMs and channels), the terminal, and the browser, with Claude and GLM models.
- **The model for each message, picked by Jev.** `route.v1` reads each message's interaction mode as it arrives and
  picks its model from the operator's routing table, waiting at most 300 ms beside the first compile (his setting; 200
  by default): a greeting or a thank-you goes to Haiku, chat stays on the session's own model, deep coding goes to Opus
  5.5, the most sophisticated work to Fable, and routine coding to GLM-5.3. A routed session keeps its
  move only while `route.v1` acts, and `profile.use` moves it back (Items 139 and
  150); it keeps the base it was moved from, across a restart, and follows a change of that base, and each loop
  records one compile and one start (Item 181). A request the model refuses is retried once on its fallback, Sonnet 5 for Sonnet 5.5
  (and, in his config, Opus 5 for Opus 5.5), and every surface says so (Item 154).
- **Fast replies.** A greeting's reply shows on Discord as its model streams, the verdicts a turn waits on land
  inside their wait, and a trivial message routes to the small model from a 0.4 bar (Item 158). A trivial
  detour sends no recall and writes none (Item 161).
- **Jev in the loop.** Every judgment pack runs beside the turn in shadow: the loop's stopping point, the gate's
  safety call (`security.v1` and `security.v3`), each message's class and role, continuation, and a private
  conversation's topics. Each judgment is marked in the trace, shown by `theseus judge show`, and counted in the
  cockpit's Judgment section (Items 105, 119, 120,
  121, 122 and 126). Three are live:
  - `route.v1`, above;
  - `rerank.v1`: Jev's order of recall goes in front of the model, waited for at most 300 ms (his setting; 200 by
    default), with a breaker of its own (Items 128 and 141);
  - `security.v3`'s notices: after a call the gate let through that Jev is at least 90 % sure was risky, a notice to
    the owner's DM alone, never on the call's path, with `security.v1`'s brake (Item 142).

  The ladder holds each pack's mode, promotes, rolls back and adopts (Item 145). The owner's labels,
  from the cockpit's buttons, feed the learning ledger's nightly report with its frozen holdouts (Item
  129), and the learning loop rewrites a pack version's wording from them, nightly, under a
  $2.00-a-day limit (Item 164). Replay, audit and backfill try a pack version against the record
  (Item 144), and `theseus judge prove` reports lane L3's exit from the daemon's own ledger (Items
  101 and 179). Replay and the loop share one rule for whether a lean was right, the audit runs off its low
  thread, and the prove leaves out a learned loop version's tasks and names its placement (Item
  203).
- **Memory in every turn.** Recall runs live in front of the model, place-safe (a shared place recalls only its own
  sessions; a private one draws on every session), on the `baseline` arm, with `rerank.v1`'s order (Items
  99, 112 and 160). The memory pass after each turn labels
  what was said and links duplicates and corrections, with `memory.v1` and `attribution.v1` in shadow (Item
  136). A long session is compacted into summaries with a floor, on GLM-5.3 Flash by his routing
  table (the session's own model by default), and its
  context is assembled as recall, then the summary, then the tail (Item 131); the system header
  says how a request was assembled (Item 156). A bounded node cache means a turn decodes only what
  it reads (Item 162). Retention, activation and consolidation's cited syntheses are built as arms, each
  off unless `[memory] arm` names it (Items 157, 159 and 160), and
  measured by the four-arm exam (Item 124). Consolidation's nightly writer is on again, at $0.50 a
  day, since a synthesis that opens with a heading is checked without it (Item 188), and
  `+activation`'s warm build yields to a busy machine and never holds a stop (Item 187).
- **Situations, checked before anything is sent** (35a, enforced from the first day). Each turn's compiled
  request is checked against its situation: a turn whose compiled set does not close fails before anything is
  sent, with a `context.unadmitted` row and words that name the piece. A recalled item's header names its place
  and the reply's model, and a volatile value says it is unverified (Item 183).
- **The operator's earlier history, imported.** `theseus import openclaw|list|erase` brings the episodes of
  his earlier agent's history in as imported sessions: closed, private, left out of the session list, each
  message at its own time with an `import` origin, idempotent, and erasable by tag (Item
  201). His went in right after install #8 (October 6, 17:26 to 17:28): 21,779 episodes, none rejected. The
  session lists step over them by key, so a list stays in a few milliseconds (Item 219), and
  their vectors are embedded in the background over days. `theseus import topics --tag T` makes the
  episodes' topic labels the ontology's topics, a tree from their slash paths, and each imported session's
  memberships (origin `import`, at most three), from the stored labels; the tag's erase takes them back
  (theseus-anh3).
- **Tasks as records.** A task has a record of three layers and a place in a graph, edited by compare-and-swap through
  the task tools, its first layer asked of the owner only for tasks the owner created (Items 125 and
  146). A task is created with its arrangement (references, a refusal, a fidelity check; Item
  113), sets one-shot wakes and parks on them (Item 98), can be checked by a task
  independent of it by its claim (Item 132), and is claimed under a 30-minute lease. The task board
  is in Discord (`/tasks`) and the cockpit's task graph (Item 163).
- **MCP both ways.** MCP servers' tools in turns (`[mcp.servers]`, private places only, their results outside text;
  Item 106), their prompts (`/prompt`, `theseus prompt`, the cockpit's picker; Item
  115), and Theseus's own MCP server on loopback, off unless configured (Item 110).
  **Self-proposed extensions:** `extend.propose` freezes a stdio MCP server, tries it in L1 and asks the owner; an acked
  one loads, restarts with the daemon, and can be revoked (`/extensions`, the cockpit's Extensions card; Items
  118 and 133).
- **Tools for code.** Language servers as tools (`lsp.*`: diagnostics, navigation, rename; Item 114),
  an edit's result carrying its files' new errors (Item 127), rust-analyzer's compiler errors
  after a save (Item 153), and servers that watch their own files (Item 151). Terminals
  as tools: `term.open`, `term.send`, `term.read` and `term.close` (Item 109). `proc.run` takes up
  to 16 steps as one batch, judged as the strictest and run in turn to the first failure, and `fs.patch` recounts a
  hunk's header from its body (Item 175).
- **The files people give it:** PDFs from every surface (natively for Claude, as text for GLM), Word documents,
  spreadsheets, decks, notebooks, EPUB, archives, and audio and video, transcribed when the model reads them (Item
  155).
- Built-in tools: files (read, write, edit, patch, search, list), git (diff and log), commands, web search and
  fetch, background tasks, and reminders that repeat (every evening at nine, on weekdays, until a date).
- Tasks with their own budgets that report back, and personas by context file.
- The compiled context with its manifests, caching shared across sessions, and token estimates that start from
  the provider's own counts.
- **Approvals from your private places, by you:** the terminal, the web UI, your DMs, and the Discord channels you
  bind private. A question asked in a shared channel comes to your
  DM, and a question nobody answers expires at the time its card gives. A command run from inside a job cannot
  answer, untighten, remove a content label, publish, or bootstrap AWS.
- What needs you, the same everywhere: one rule, shown as a pill in the terminal and the cockpit.
  Changes are pushed live to every surface instead of polled, each notification serialized once for every watcher
  (Item 172). `theseus watch --all` follows every session, and `theseus wait` returns the moment a
  session needs you or settles. Every queue says why, and a late result's wake rides its turn's end, so a
  settled wait returns after the late result is read (Item 194); `theseus watch` ends a
  reply's open line when the daemon dies (Item 191).
- A terminal UI, `theseus tui`: every session in one sidebar, the one that needs you a key away, its question
  answered inline, and a finished session marked done until you have looked at it.
- herdr panes that show each session's true state: `theseus watch` inside a pane reports it, and
  `theseus herdr sync` gives every session that needs you or is working a pane of its own. A message typed in a
  pane runs on the session's own model.
- `theseus reach`: where a message went, through the sessions that copied it, from a node's short id; and
  `theseus history` pages a session both ways (Item 193).
- An answer takes effect in one step, so no surface shows a session waiting on a question it no longer has.
  Underneath, several kernel state changes commit as one, and every ledger row, live event, and narrative line of
  a turn comes from one typed fact, so they can't disagree.
- **Sandboxed commands (L1).** A command the agent asks to sandbox runs in namespaces of its own, with its writes
  discarded and its own process limit, at notify unless your own setting for commands asks first. It never falls
  back to the host. `theseus health` reports the last sandboxed launch, and `theseusd check` runs a sandbox self-test
  on demand; nothing runs one at the start any more. A daemon run as root refuses sandboxed commands, which would get
  no process limit there.
  - **Egress:** it reaches only the hosts you list (`[sandbox] egress`), through a proxy of its own; a call that
    names other hosts runs with a notice that names them, and reaches those hosts alone. What it brings back from such
    a host counts as outside text (it carries the `external` marker, which holds nothing); your listed hosts' results
    don't.
  - **Credentials at launch:** it gets a secret only when you grant it to the program it runs, at its start,
    exactly as an ordinary command does, and any approval the secret needs comes before it starts. Your own AWS
    files (`~/.aws`) are hidden from every sandboxed command.
- **Jobs on Linux's own terms** (Linux only, by design). A job's wrapper and its command start without a fork, so a
  job's start no longer grows with the daemon's size (Item 149). Where the daemon's cgroup is delegated, as
  the user service's is, each ordinary job runs in a cgroup of its own, capped at 4,096 processes and threads and
  stopped exactly (Item 152). A job's completion takes one sync, and the background passes wait while the
  machine is busy (Item 151). A job result that arrives during a restart's startup is taken at once, not
  a minute later (Item 166). Health's cancels count a cgroup-stopped job as `l0`, and telemetry counts each
  cancel as health does, with the index tender's lag and size (Item 190).
- **The harness-only keys.** Your AWS keys and your model providers' keys are never handed to a job: Theseus's own
  tools use them. `theseusd check` and `theseus health` say so by name, and name any of them you grant after all.
- **A cancel that says how it knows.** A cancel or a stop ends every process of a job, one that detached into a
  session of its own too, and says how that is known, in the terminal (`⏹️ cancelled proc.run …a1b2c3 (verified:
  process tree, 2 processes)`), on Discord, and on the cockpit's boundaries board. A sandboxed job's end is verified
  by its own process namespace, and a job's deadline stop waits up to 2 s to verify its whole tree died (Item
  173).
- **Private and shared places.** Your terminal, the web UI, your DMs, and any Discord channel you bind
  `private = true` get everything; a guild you trust makes every channel you bind in it private. Bindings' second
  format holds many guilds, with trust per guild and a ceiling per place (Item 117). Every other
  Discord channel is shared. There Theseus has only its own conversation and the tools whose results anyone may read
  (web search and fetch, wakes, tasks), and files only under the trees you declare public. It cannot run programs or
  use AWS there, and it reads only the context files you marked public; a fetch of a private address asks first, and
  says the page would join a conversation others can read. `theseus places` and `theseus health` list each place and
  its class, and health warns if a channel you bound private, outside a trusted guild, can be viewed by someone else.
- **Publish, and gliding.** To share something from a private place into a shared one, publish it yourself:
  `theseus publish <node or file> --to <place>`, Discord's `/publish`, or the cockpit's publish control. It is
  written there as your message, with your note, and recorded. The agent's own `channel.post` and `channel.read` go
  into a private place always, and out of one, or between two shared places, only once you say (Item
  140).
- **Search over every session.** The index tender, a process of its own beside the daemon, keeps every session in
  a search index (words, exact names, and vectors); `theseus index search` and `theseus memory search` find a phrase
  from any of them.
- **AWS on Theseus's own account.** Reads of any service, the service catalog, who the key is, and S3 listings;
  writes in short-lived role sessions, behind AWS's own guards and the gate's floor; the account's stacks, made by
  the bootstrap and protected by their stack policies; and a $50 monthly budget with AWS's stop at 100 %. The `aws`
  CLI gets a short-lived job session, never the key, in a sandbox too. Every request is attributed in CloudTrail and
  recorded, and counted and timed in telemetry by service, operation and outcome (Item 178).
  - **The curated tools:** S3's get and put, Logs Insights and tail, CloudTrail, and the inventory with a reaper that
    reports; a secret-bearing read returns a handle on the secrets board, never the value; AWS's text is outside text;
    and a daily CloudTrail cross-check (Item 97).
  - **The hour's and the day's lines** are alerts with authority: each hand's worst case is reserved against them,
    and at ten times a line new AWS actions are refused, with words, until the period turns or the owner raises the
    line (runaway mode; Items 116, 147 and 174).
  - `[policy.aws]` keys are checked against the catalog after serving, and an unknown one shows in health (Item
    174).
  - **The S3 backup is on** (since install #6, October 6; the operator's yes of October 5): the durability tender
    ships the store's synced frames and blobs to his bucket and table, each session listing only its own prefix,
    and health and the cockpit's AWS card say how far it has shipped; a restore drill gave back the same sessions
    and nodes (Item 198).
- **Budgets and policy, explained.** `theseus budgets` lists every budget with what is spent and reserved, and
  `policy.explain` says which rule of the gate's order decides a call (Item 130); the cockpit has
  Budgets, Ledger and Policy tabs (Item 165). A provider call in flight at a crash or a stop is booked as
  spent at its reservation when the restart marks it unknown, so a budget's reset frees it (Item
  197).
- **The cockpit is the web UI**, served at `/` (an old `/cockpit/…` address redirects to the same page). It shows
  everything the Observatory did, which is gone: ledger rows in words, the session deck's questions and live turn,
  every execution, every system's card, and the sandbox's settings. It draws the agent as a fleet at night (places,
  sessions as galleys, tool calls as oars, sandboxed calls shielded, tasks under sail), with brass gauges on live
  health; in Live mode its sea rolls gently, and Calm keeps it still (Item 143). A ship's log under
  every page scrubs the Ship, the fleet, the actions, the boundaries board, and the money back to any moment. The money
  river follows every dollar from session to model, the speed wall puts each speed promise against its budget, and the
  build each daemon runs is named in its health, its log and its Systems card (Item 177). New since
  October 3: the Ontology view and a session's memberships (Item 111), the Judgment section, the
  task graph, the Budgets, Ledger and Policy tabs, the Extensions card, and a plain page where WebGL is missing (Item
  135). New since October 7: the Context page, what Theseus knows and what a turn sees (theseus-7n3e): the imported
  episodes filtered, counted by facet and opened to their messages (`import.sessions`), their topics and book hints,
  asking the index, and a turn's request part by part with its tokens and why each note was recalled
  (`context.explain`), from any ship or bench.
- **The ontology, wired in:** categories, guidance and memberships as records, which every compile walks (Item
  100); `categorize.v1` proposes a private conversation's topics, in shadow.
- **Voice in a Discord voice channel**, with Deepgram: `/join` from a private place, speech both ways, replies spoken
  sentence by sentence with barge-in, and speech counted as spend. On since October 3 at 23:38. Since October 6 a
  barge-in holds the reply until the words over it decide: a wordless sound, an echo, a backchannel or "go on"
  resumes the cut sentence from its held audio, words cut it, and a reply waits for the floor (Item
  202). Since 1.0.2 a call listens through Deepgram Flux by default, whose own model decides where a turn ends;
  `[voice] stt_model = "nova-3"` brings back the pause rules, and voice notes stay pre-recorded nova-3 (Item 256).
- **Set it up in one command:** `scripts/setup.sh` checks the machine, builds the cockpit and the binaries, installs
  them, and writes the default config, `/etc/theseus/theseus.toml`, which `theseusd` reads by default (Item
  138). The config holds only what differs: `theseusd config --sparse` cuts one to your own values and
  secret references, so new keys and defaults arrive with the build. A secret may also come from the environment or a
  file (`env:`, `file:`), for containers and CI.
- **The daemon as a systemd user service**, the operator's since 2026-10-02 at 14:23: `scripts/user-service.sh`
  checks the machine and installs it, systemd restarts it after a crash (a bounded loop; Item
  102), and a crash leaves a file the next start reports.
- **Health says what it sees:** the web UI's refusals, the 1-hour cache's writes (priced in `theseus catalog`), the
  memory block, the node cache, and free space crossing a line, which writes one row and tells the operator (Item
  177). A failed provider call's time and span carry its error's class (Item
  178). The durability tender's state, loud when failing or stopped (Item 198).
- **Speed at scale.** With 10,000 parked sessions, a cold start answers in about 22 ms and health in under 1 ms, and
  50 turns at once peak at about 104 MB of memory. Every write goes through one writer thread with group commit, so
  a burst of writes no longer stalls the requests beside it. The polled lists and the ledger read a page through the
  index (an action list in 4.5 ms where it took 147), and a store's open makes nothing durable, so a restart after a
  crash pays two syncs fewer, and none for the log's directory when a mark vouches for its segment (Item
  196). A turn needs 576 KiB of stack where it needed 1,856 (Item 192).
- **A job's end wakes its turn.** A quick command returns in about 30 ms, not 50, a waiting job's wrapper sleeps, and
  a turn that runs a tool writes 9 durable frames, not 12.
- **No acknowledged write is lost silently.** Each frame of the log carries how far the log was known synced, so a
  frame that rots after its sync is refused, never cut, and `theseusd restore --repair` takes it from a backup. A
  failed sync cuts its frames back to the last good sync, so a write told "failed" stays failed after a restart (Item
  176).
- **Benchmarks through Harbor**: `theseus --spawn ask` exits with how its turn ended and stops cleanly on a timeout,
  and `bench/` runs Terminal-Bench and SWE-bench tasks through Theseus. The first full Terminal-Bench 2.0 run, three
  configurations, is in [benchmarks.md](benchmarks.md) (Item 148), and the bench program has three more
  harnesses: efficiency, concurrency, and incidental recall (Items 167, 168 and
  169), measured end to end and held to a recall plan's overhead since October 5 (Items
  184 to 186, 199 and
  200). The static musl build compiles again (Item 195).
- A session that outgrows its model's window drops its earliest turns and retries once, by itself. A job
  that prints past its output cap keeps the end of its output, where builds and tests print their verdict.
- A stop reaches a job even while the job is still starting, and cuts the model's reply mid-stream.
- A cancel answers every call it ends, and a corrupt record degrades only what reads it, with a repair from a
  backup copy.
- Text from outside (a page, a search, an MCP result, a mail, `gh`'s output) carries an `external` marker and is
  classified like any item: if Jev finds it trying to steer the assistant, the session carries an `injection` label,
  and every call except a local read of files and code is refused until you remove the label (`/trust`,
  `theseus content remove SESSION injection`, or the button on the message).
- A secret broker that hands a program, sandboxed or not, only the credentials you've granted it, never through a
  shell or a program that runs others, and never to a cloned project's git hooks.
- Budgets in dollars, the cockpit and The Narrative, crash recovery, restore from a backup copy, and a systemd
  installer.
- The speed budgets, enforced on every commit, and a plain turn held to 5 durable writes, a tool-call turn to 9. A
  sandboxed command's start is held under 25 ms in every join's gate. The gate has one way of taking its shared lock,
  waits for a quiet machine (under three quarters of the cores, at most 2 minutes), and on a machine that stays busy
  its timing budgets get a calibrated allowance, shown in its log and history, instead of failing for noise. An
  install builds only the five binaries it ships. The tests that failed under load wait on what they test, not on
  the clock (Item 189).

## New at installs #7 to #9 (October 6, 13:03, 17:22 and 22:13)

At install #7:
- **A secret printed JSON-escaped** (any of its characters escaped: serde's and Python's forms, `\/`, `\u`
  escapes, Rust's `{:?}`) is withheld before the model and the WAL (Item 204).
- **The gate's layers held by turns**, and `theseus policy explain` names L3 where an edit's language server raised
  the posture (Item 205).
- **A declined call ends its batch's waits:** the later calls that would ask are answered "Not run" with no card,
  and an approved call runs even when its id repeats an earlier answered call's (Item 206).
- **Discord's bindings, read while the daemon runs:** a place added or removed binds or unbinds within 2 s, with no
  restart; a new guild's invite check and voice still wait for one (Item 207).
- **The judgment log pages from the newest**, the notices' brake reads today's rows, the learning rules skip closed
  windows, and inbound and compile judgments record their class (Items 208 and
  209).
- **A search during the paced warm build answers `building` at once**, and a summary's call is reserved on the
  estimate's upper bound (Item 210); the daemon's own telemetry path is held by tests (Item
  211).
- **A routed turn's trace root and metrics name the model it ran on** (Item 213); the core's
  waits are proved (Item 212); a `--stdio` daemon's stop closes its store first, the lifecycle bench
  times a cancel's round trip, and the flaky list is empty (Item 214).
- **In a voice call, an answer that repeats its question's words and a "yes" on a question's last word are turns
  again:** an echo must be a near-whole, in-order copy (Item 215).

At install #8 (17:22), with the import of the operator's earlier history after it:

- **In a voice call the next turn is told what was heard, cut and never said**, replies are shaped for speech (no
  Markdown, table or code block is read aloud), and a failed voice turn says so aloud (Item 216).
- **No person's name in the tree:** the owner is "the owner", or `zeroaltitude` where an account is meant, and the
  assistant is Tabitha/Claude (Item 217).
- **Pi, the benchmarks' fourth arm,** measured as the others are, and a report for every benchmark run under
  [docs/benchmarks/](benchmarks/README.md), eighteen from the retrospective (Items 218 and
  220).
- **The session lists read live sessions by key** and never decode an imported record (Item 219).
- **The judge's frames are written between turns,** and a clean stop writes every settled judgment before the store
  closes; `bench turn --judge` measures the judge's cost on a turn's path (Item 221).

At install #9 (22:13):

- **A learned version's promotion says what stands in the moved one's place** and how to roll it back, and the learning
  cut never runs ahead of the judgments it cuts (Item 222).
- **In Discord, a background job's late result edits its one tool line** and its one notice card instead of posting
  again, and the gateway and the bindings watch end at the daemon's stop (Items 223 and
  224).
- **More of a secret's encodings are withheld:** escaped twice as JSON, in YAML's and Python `repr`'s escapes, and as
  base64 broken by escaped line breaks (Item 226). Five tests that failed under load are fixed at
  their causes (Item 225).
- **The recall benchmark refuses a daemon more than 50 tokens off its plan** (Item 227).
- **A failed routed turn counts under the model it ran on** (Item 228), and a turn's own provider call is
  reserved on the estimate's upper bound (Item 229).
- **A voice call that joins but never hears** is noticed, rejoined once and told, and a voice turn that waits on you
  says so (Item 230). A held reply decides at most 3 s after the last words over it, and a floor held by
  a wordless sound frees at 8 s (Item 231).

## Joined since, on install #10's list

Install #10 waits for a verdict on a job-wrapper test that failed once in a gate (theseus-sdgl). It carries:
- **Every credential mint and stored secret in AWS's catalog is held to a reviewed decision,** and 27 operations that
  read as reads before now run as writes, with a notice (Item 232).
- **Health counts the operator's own sessions apart** from the imported and erased ones ("sessions N · imported M ·
  erased K"), in the CLI and the cockpit too (Item 233), and the benchmarks' Harbor arms are
  comparable: medium effort on every arm, Claude Code pinned, a timed-out agent stopped before it is scored (Item
  234).
- **A running job's cancel settles in two synced frames instead of five**, its round trip in the gate about 30 ms at
  the median where it was about 110 (Item 235), and a stopping daemon begins no retry or model call
  (Item 236).
- **route.v1 asks Jev in a request of its own**, on a connection kept warm, and says when its verdict came late (Item
  237).
- **The cockpit's phase 2, all four lanes:** the watch's sixth plate, "Since you last looked", over the daemon's
  unsettled actions (Item 238); the Ship's words from one place, a console of the engine and
  tokens a minute, the tour with what's new, a sea that is still when nothing happens, and sound, off by default (Item
  239); every chart drawn by the chart method, each with a table view (Item
  242); and the critique's repairs, among them Judgment without overlaps, and a daylight mode
  (Item 243).
- **Discord's bindings watch** acts on no half-written save, so a change binds within two periods (about 2 to 4 s),
  and tries a failed bind again each tick (Item 240); the turn bench names each run's slowest frame
  (Item 241).

## Built, and waiting to be turned on

These are built, tested and installed, and off on the operator's daemon until he says, or until a live check:
- **AWS hands on Lambda and Fargate** (`aws.hands.run`): a signed envelope, a completion poller, cancels verified per
  backend, overdue hands reaped, quotas and waves, `hands.list` and the cockpit's grid, in an existing VPC and never
  with a NAT of their own (Items 107, 116 and 134). The account has
  no hand yet: its stack and image wait for the operator's go (theseus-ongv).
- **The MCP server** on loopback (`[mcp_server]`, off by default).
- **Memory's measured arms:** `+retention`, `+activation` and `+synthesis`, each off unless `[memory] arm` names it.
- **Self-improvement's spine** (joined October 10; switched on in the operator's config for v1 on October 18, the
  default still off): `[self] mode = "off"` by default, so
  nothing self-directed runs; a kill switch anyone may throw (`theseus self halt`, the cockpit's Halt, "halt self" on
  Discord) and only the operator releases (`theseus self resume`, the web UI, or "resume self" from his own Discord
  account), halted until his first resume; `theseus self log`, every change Theseus made to itself with its numbers
  and its undo; a weekly digest to his DM while the mode acts; and health's `self` line. The keel guard, the gate's
  first phase since October 9, fails a branch that deletes or loosens a test, budget or ceiling without his signed ack.

## Under way now

- **Install #10**, the list above, after the verdict on a job-wrapper test that failed once in a gate on October 7
  (theseus-sdgl), which is being looked at.
- **In the cloud or in review:** four more tasks of the tenth and eleventh batches: the daemon's stops, the judge sink's
  speed, a walk over imported sessions, and the recall benchmark's fairness.
- **The v1 week** ended with the v1 cut on October 18 (above). v1 patch1 is next, with the Google Workspace tools.
- **Claude Haiku 5.5 in the stable**, released October 7 (theseus-3okf), in review: its catalog row with the first
  long-prompt price tier (every price 5x past a 100,000-token prompt, reserved and settled at the tier), route.v2 as
  the acting route pack with a `quick` mode run as a one-turn detour, and the placements: acknowledgements and quick
  questions on Haiku 5.5 at effort low for their turn alone, routine programming on Haiku 5.5 at effort high with
  Sonnet 5.5 behind it, and Haiku 5.5 the template's recommended compaction and synthesis profile.
- **The AWS tools as a tree**, decided on October 6 (the operator's answers T1 to T9): typed tools for the services that
  cover 90% of the use, leaves declared and deferred, BM25 search and Jev's branches, to build (theseus-0nnk).
- **With the operator:** a live voice call, to see a deaf call noticed for real (theseus-d93y); the AWS live checks of
  the hands' stack and the runaway brake (theseus-ongv); whether a trusted guild answers in every channel, unbound
  (theseus-yzhv); the question walk's open rows; and the other items §2 of the spec lists as still open.

## The roadmap

The build runs as one spine of steps on `main`, one at a time, with independent work in parallel lanes and cloud
sessions beside it. Each step must pass the full test gate, the speed budgets, a live check, and a written review
before it joins.

| Stage | What it brings | Where it is |
|---|---|---|
| **A. Stage 1's remainder** | Fix batches from two reviews, the dogfood pilot, the complexity cuts, the reader rule | Done. Review 2 was accepted whole on October 1. Its spine items (C6, C2, and S2) are all in, with its security items (installed on October 2) and the lanes for robustness, security, performance, proof points, and the bench. What it deferred is in the v1.1 roadmap. |
| **B. The operator's surfaces** | Live updates pushed instead of polled, `session.wait`, the CLI's client library, the first graph edge, a terminal UI, herdr | Done on October 1: the push, the client library, reach, the terminal UI, and herdr. |
| **C. M4, boundaries** | The sandbox wired into `proc.run`, egress, credentials at launch, private and shared places, integrity, the ontology, the job host | Done for v1. L1, cancellation verified per backend, egress, credentials at launch, the place rule, integrity's light pieces and the sandbox's trims landed on October 2 and 3; the trusted guild and 18e on October 3's evening; the ontology wired in (21b) and its cockpit view (21c) on October 4. The job host (22b) moves after v1. |
| **D. AWS** | The bound account, its stacks and budget, curated tools, the durability tender, restore from S3, hands on Lambda and Fargate | Built. The bound account (C1) and its stacks and writes (C2) by October 3; on October 4 the curated tools (C3), the durability tender (15), restore from S3 (16), and the hands (step 40, both parts, and their network), installed at 14:09; runaway mode at 20:00; fixes to the tender, restore and runaway mode on October 5, and the S3 backup on the operator's daemon from October 6 (install #6); every credential mint held to a reviewed decision that night. The hands' stack and their live checks on the account wait for the operator, and the AWS tools' tree is decided, to build. |
| **E. M5, judgment** | Jev wired in: the loop's stopping point, the gate's safety call, roles, continuation, all in shadow and then under canary | Built, from October 4: the wire-in (23a), its surfaces (23b), the security check and its live notices (24), inbound class and role (25a), continuation (25b), the learning ledger (25c), replay (25d), routing (25e, live), the ladder (26a), the task arrangement (27), independent checks (28a), topics (28b), L3's proof (row 50) and the learning loop; on October 5 and 6, fixes to replay, the judgment log and the learning rules, the judge's sink between turns and a learned version's promotion, and route.v1's own request to Jev. Left: JUDGE_STOP for tasks under canary (26b) and the roles table under canary (26c). |
| **F. M6, memory** | Recall in turns, the memory pass, retention and activation as measured arms, tiering, and books | Built, from October 4: recall in shadow (30a), then live in front of the model (30b), compaction (30c), the exam's arms (34b), the memory pass (31a), consolidation (31b), retention (32a), activation (32b), Jev's rerank, in shadow and then live (32c, 32d), tiering (33), and situations (35a), enforced from October 5; the importer for the operator's earlier history on October 6, and the import itself that evening, with the lists and health that step over it. Left: lessons (35b). |
| **G. M7, surface** | MCP in turns, repeating schedules, many-server bindings, the task board, budget and policy views, self-proposed extensions, voice | Built, from October 3: repeating wakes (37a) and voice (rows 77 and 78); on October 4 and 5, MCP tools and prompts (36b, 36c), tasks that set wakes (37b), bindings' second format (38a), gliding (38b), the task record and board (39a, 39b), the MCP server (41b), budgets and policy (42a) and their tabs (42b), and self-proposed extensions (43a, 43b); on October 5 and 6, history pages, Discord's bindings read live, and voice's held barge-in and echoes; on October 6 and 7, voice that is told what was heard and notices a deaf call, and the cockpit's rethink (its phase 2's four lanes). |

**After v1:** [the v1.1 roadmap](design/roadmap-v1.1.md) plans the week that follows, about 60 agent-hours: the
kernel's last frames, visibility, the operator's surfaces, cost accuracy, the codebase's shape, proof and the
turn's edges, and Discord.

**When.** Theseus has been feature complete, v1-beta in the operator's terms of October 6 (13:35), since October 6
at 22:04. v1 comes one week after it, under his rule of October 4 (22:09): seven days of his daily use with no
serious bug, every speed target met, and each of B5's losses explained or fixed; so about **October 13** at the
earliest. The spine's last rows (26b, 26c and 35b) run beside it. This
pass did not re-estimate v1.1's date. v1 was cut on October 18, after the last features, RSI loop 1 and the benchmark rerun.

## Recently landed

- **2026-10-30: the old routing is deleted** (theseus-kn9or, row 6 of route.v10, theseus-h66v4). Jev chooses the model
  and its effort, so what chose them before is gone: the class table (`[routing.classes.*]`) and the walk over it, the
  place cap, the dear tier's guard, the step back down, a task's `max_switches`, the effort bounds and bars, the
  China-origin draft with its review (`cn_draft`, `final`, `first_pass`, `[server] drafts_dir`), the `config::legacy`
  profiles an unset key used to run on, and the closed branch of every site that had both. The retired `[routing]`
  keys, and `[server] drafts_dir`, still load: they warn once and do nothing for one release. The data rule, the
  operator's pin, the money backstops and the refusal fallback are unchanged.

- **2026-10-24: v1 patch2's live learning** (theseus-6f8t0; Item 257). The owner card now learns from live
  conversations, not only the imported history. When a private conversation the owner is alone in pauses (the CLI, the
  web UI, the owner's DM; never a task or a shared place), one cheap call on `[card] profile` reads the owner's own
  lines and keeps what they say about the owner that should hold from now on, as unconfirmed private notes
  (`card.obs.<id>`: never a node, indexed, recalled or in any prompt; the census is unchanged), shown by
  `theseus card observed`. Each night at the learning hour, also after `theseus card done`, the card's live pass sweeps
  what never came due and turns the notes into questions: Add, Pattern (the same preference or decision in 3
  conversations) and Stale ("your card says X; on 10-20 you said Y; still true?", answered Keep, Edit or End it,
  `theseus card end`), at most 5 a night with at most 2 stale, `ask_days` 14. Up to 3 a day post to the owner's DM as
  paced cards with their buttons (`[card] dm_per_day`). Nothing becomes a record without the owner's answer. Store
  format 51 (`[card] observe`, `live_per_night`, `stale_per_night`, `pattern_sessions`, `dm_per_day`).
- **2026-10-24: v1 patch2's voice lane, Deepgram Flux** (theseus-v774k). A call now listens through Deepgram Flux by
  default (`[voice] stt_model = "flux-general-en"`): each listed speaker's audio streams over a socket, and Flux's own
  model decides where a turn ends (`eot_threshold`, `eager_eot_threshold`, `eot_timeout_ms`), so `pause_ms` and
  `merge_ms` do not apply. A quiet speaker is fed silence for the end-of-turn window, so the end comes on time; a
  barge-in over a reply is held when residual speech fills 3 of the last 10 ticks behind the echo canceller; a dropped
  socket reconnects without replaying stale audio; streamed audio no turn carried is booked as a `trailing`
  `speech.transcribed` row ($0.0077 a minute streamed). `stt_model = "nova-3"` is the fallback by config (the pause
  rules, any language); voice notes and file transcripts stay on pre-recorded nova-3; a socket that never opens drops
  the call to nova-3 and the pause rules, said once. A `[voice] language` that is not English with no `stt_model` now
  refuses at load and names `nova-3`. Awaiting the owner's live retest with Flux as the default.
- **2026-10-23: v1 patch2 rows** (theseus-zeyr3, theseus-cuv2o, theseus-utfnl, theseus-nde5n, theseus-zcn80,
  theseus-gg78p, theseus-ujzuh, theseus-u5a37, theseus-f9qa8, theseus-goq94, theseus-2uy56, theseus-yct32,
  theseus-ly75u, theseus-7e7hc, theseus-it4ta, theseus-1j7ib, theseus-i08xr, theseus-n17am, theseus-uy015,
  theseus-dyukm, theseus-xzf71, theseus-0ly2a, theseus-wdzmj, theseus-yym79, theseus-1kr2g, theseus-hk0dk,
  theseus-axno0, theseus-capcs, theseus-s48k6, theseus-izyws, theseus-5iqf8, theseus-b09do, theseus-rmjqr,
  theseus-5mkz1, theseus-5pg8u). The recall bench's generator reads its prices from `catalog.rs` (`catalog_prices`);
  Sonnet 5.5's cache reads are $0.20 (the catalog said $0.10 from 2026-10-17 to 2026-10-25, theseus-rko44), so the seed-7 budget is $0.60 an arm for the smoke and $16.97 for the full run, run again on 2026-10-25 from the plan's own token counts (the $0.48 and $7.89 this row first carried were at $0.10 and the full's earlier, smaller window). The
  chat now follows a thread-placed turn's answered question and its continuation (it watches the placed session), and
  the plain approval card says why it waits: the thread guard's question names both threads, other rules say their
  clause, and a thread session is asked by its thread's title. A message that asks for a task ('start two background
  tasks') now loads the tasks family for that turn, with or without tool search, so a DM's step 1 starts tasks, not
  plain background jobs (theseus-gg78p). The checkpoint's text tells Jev the effort the work ran at and how many
  checkpoints in a row found it failing; a sticky effort hold now ends early when a later verdict is sure enough
  (`sticky_reverted`; a sure answer, or `effort_confidence` in a `sticky_unsure` hold). xhigh now needs a sure answer
  (an unsure xhigh runs high). route.v8 (theseus-f9qa8): Jev's data question names the public signs of open-source work
  and no longer treats an unshown owner as a reason for `unclear`; `private` is unchanged, and the verdict table
  (`tests_data_verdict.rs`) pins that only open at act, naming no sensitive name, on a session without the private mark,
  reaches a restricted model. v8 also differs from v7 in `reply_effort`'s `max`. A turn can be given a deadline
  (`theseus ask --deadline SECS`, `turn.submit`'s `deadline_secs`): the model is told the minutes left, and a turn with
  no passing check is told once, at half its time (or its spend, when it has a deadline and a limit that only
  notifies), that this is not a stop and to go on, and to stop and report at 0.85 of its time (`turn.progress`);
  reading new ground quiets the first and never the second (theseus-ikr75); every session's block names the CPUs its commands really have, from the cgroup quota
  (theseus-2uy56, theseus-yct32). The owner card no longer reads an assistant's own setup notes as facts about the
  owner, never offers one task's instructions as a lasting preference, keeps who acts and on whose behalf, offers no two
  conflicting proposals together, and lets the owner say where a kept record holds (`theseus card keep --scope`, `card
  scope`) (theseus-ly75u). The four old organisation names are gone from fixtures, tests and comments; one digest list
  (`names::OLD`) serves the template test and a tree-wide scan; `policy.explain` reads a hold that has told the owner as
  no hold, as the gate does (theseus-cuv2o). A kept kind-name pick names a kind only while its cluster still holds the
  name; a stale pick is dropped and its cluster asked again under the day cap (theseus-7e7hc). The memory exam's scratch
  daemons run with routing off, each record names its profile and model, and a run or report that finds another model
  answering stops; `bench/recall`'s tests run in the gate, and its budget prices Haiku 5.5's long-prompt tier
  (theseus-1j7ib, theseus-it4ta). Operations follow-ups: Ask Theseus keeps its US profile with a sticky
  `session.profile`; the calendar reads only where shown and stops after a refusal; the night fold's edge is exact; the
  Graph has keys, openable folds under the 400 cap, redact masking, the page's verb menu and row-only picks
  (theseus-i08xr, theseus-n17am, theseus-uy015). The model menu greys a profile by the daemon's own floor answer
  (`profile.list`'s `cleared`, missing greys); `job.stop` stops a job from the cockpit under `work.peek`'s place rule;
  the deck's live turn is the page's `liveOf`, watches are in the palette, and the profile/catalog reads poll no more
  (theseus-dyukm, theseus-xzf71). The owner card keeps a setup line that starts with the owner's name, matches machinery
  phrases on word boundaries, drops a statement only for the lines it cites, refuses a decline with a scope, and names a
  conflict with an earlier call's entry; dream guidance says why a category waits; and the four forced-tool calls plus
  the exam reviewer force their tool only where thinking is off (otherwise auto with a strict, closed tool)
  (theseus-0ly2a, theseus-wdzmj, theseus-yym79). A `cd` both scripts begin with is where they run, not a file they
  share, so one response's fanned-out programs run at once; a name of that directory as a file or argument still waits
  (theseus-1kr2g). The TUI chat marks a placed thread's question owed only for its own turn and unwatches an evicted
  placed session; the apart debug line no longer names a thread kept by its title; the asked `tasks` family is held to
  the ceiling on tool search; the labelled families fixture says the table's lines; `drive.read` of a file over 1 MB
  answers with its metadata (theseus-hk0dk, theseus-axno0, theseus-capcs, theseus-s48k6, theseus-izyws). Operations
  verbs from a shell: `theseus task rehome`, `task reschedule` (its undo prints a time `--at` reads) and `sessions
  model` (all refused inside a job); a rehome fails closed on an unreadable home mark; the calendar cache drops windows
  idle for 30 minutes, keeps sync deltas to their window, and says `unreachable` for a 5xx (theseus-5iqf8,
  theseus-b09do). A turn's progress check now reads a loop's commands alone and its state lives on the turn; a deadline
  belongs to the turn its `turn.submit` started; the bench driver passes `ask --deadline` (the task's timeout less a
  minute); and a live model stream runs to three times `total_secs` before being cut, with a whole-call timeout retried
  once a turn at most (theseus-rmjqr, theseus-5mkz1, theseus-5pg8u). Personas are the ontology's (store format 50): a
  `persona:<key>` category per persona with its soul in a small file (cap `[context] persona_max_tokens`, 2,000), Jev's
  persona question has a `none` item for the default persona, the owner is asked once for a new persona's soul, and
  `[personas]` is read once at the first start of 1.0.2 and ignored after (theseus-utfnl).
- **2026-10-23: v1 patch1 hotfix, install #75a** (theseus-050g2). A note Theseus writes itself is never posted cut
  off: the morning note's call had a 200-token cap and no `thinking` field, so Sonnet 5.5 thought through most of it and
  the note went out as half a sentence. The call now turns thinking off where the model lets it (Sonnet 5.5's
  `between_tools`, the other adaptive models' `disabled`) at 1,024 tokens, asks once more at 2,048 after a `max_tokens`
  stop, else posts the template, and its `initiative.spoke` row records the stop and the output tokens. The nightly
  synthesis and the dream's guidance, which Haiku 5.5's thinking had left empty most nights, ask with thinking off too
  and never store a cut answer. tokio 1.53.3 for RUSTSEC-2026-0386.
- **2026-10-22: v1 patch1 tail, part 7** (theseus-y4ajh, theseus-7zs0f, theseus-bibny, theseus-t7b96, theseus-5ni7j,
  theseus-b80si, theseus-3327o, theseus-hd8l2, theseus-x8b9k, theseus-d9je7, theseus-iilre, theseus-ms3hd,
  theseus-nsqqo, theseus-a9zg9, theseus-4gb4w, theseus-stwkb, theseus-qu41j, theseus-b7vuh). The Discord DM task flow is
  locked by one golden test, `tests_dm_flow_golden::the_dm_task_flow_is_as_the_owner_approved_it` (golden
  `crates/theseus-discord/src/golden/dm_flow.txt`): any change to its text, buttons or order needs the owner's OK
  (theseus-hd8l2). Every message Theseus sends the owner unasked now opens with a kind tag and an operational title (ℹ️
  FYI, ✋ Action, ❓ Decision, ⚠️ Alert) and a line saying what triggered it (`owner_note`, theseus-x8b9k); the catalog is
  `docs/messages.md`, the night note's backlog titles are only partly done, and the Discord DM task flow is unchanged. A
  trivial detour carries `tools_find` plus any family its message names or Jev judged relevant (225 tokens of tools, was
  about 6,500 at the cli); a thread's cold resume works to 40% of the serving model's window (`[threads]
  working_percent`; `working_tokens`, when set, wins) and keeps 20 turns (theseus-d9je7). Draft review trees now live
  under `[server] drafts_dir` (default `~/.cache/theseus/drafts`), outside the state dir, so the review's test runs no
  longer ask; a `first_pass` equal to the `cn_draft` is refused at load; the first pass's bound is tested through its
  timer (theseus-iilre, theseus-ms3hd). An xhigh or max answer held at high now says `held_high_unsure` when it was
  under its confirm bar, and `sticky_unsure` when `stick` then holds it; `held_high` means the sure case; the wire's
  `effort_reason` doc lists all 14 words, the cockpit shows multi-underscore words in full, and the Discord restart test
  stops run 1 for real (theseus-t7b96). The footer's data-gate word and its counter now read the turn's own routing
  decision, made again as for a cleared ask, falling back to the mode's list only for a turn with no decision;
  keep-warm's telemetry is wired on the deferred path; the recall README quotes the 56000/101000 plan (theseus-5ni7j,
  theseus-b80si). The ontology sweep keeps its `same_kind` yeses for 30 days, so a chain asked over two nights still
  mints; it asks a pair only when its groups could reach 3 sessions, and a kept kind-name pick follows its cluster when
  the cluster's key changes (theseus-3327o). A steered held-compile route row is tested (source `correction`,
  `follows`); the TUI sidebar's question list is pinned as the whole daemon's, not filtered by placed thread; the
  ended-task states are one helper (`board::ended_state`) (theseus-7zs0f, theseus-bibny). The keel guard reads an arm
  that names a constant, a guarded arm and an arm that spans lines (theseus-y4ajh); it also judges a guarded arm against
  the arm it shadows, reads two arms on one line, and reads a block with the call after it whole, stricter only
  (theseus-b7vuh). Recall and tool admission are sized to the prompt (theseus-d9je7): the route pack's mode folds into
  small, ordinary and deep; recall packs 3,000 tokens and 8 items for an ordinary prompt and 8,000 and 16 for a deep
  one, and never waits for the verdict (a late one packs ordinary and says so on `route.decided` and in `theseus why`);
  a session's tail may hold 64,000 tokens of recall; admitted families may raise a deep prompt's first request to 3x the
  place's budget; and `context.compiled` carries the ontology walk's tokens, measured only. A notify hold now tells the
  owner once per hold (the META mark `external.told.<session>`, theseus-nsqqo); an unset `[policy] external_text`
  follows `enforcement`; the public template's `sensitive_names` example uses placeholders (theseus-a9zg9). The misroute
  guard reads a thread's relative roots under the session's directory (the grower's base), so a guess-placed turn inside
  its own thread is no longer held; its question no longer doubles an article (theseus-4gb4w). The misroute guard treats
  a program run from a directory above the thread's roots (the session's directory) as undecided: it holds nothing and
  stays armed, and the turn's next write or program decides (theseus-qu41j). Another live thread's recall note must
  again outrank the asking thread's best own hit, in context or not. Only a home note weighs the thread's best not yet
  in context (98bz.8's case), so msg 113's cutoff still gets in. On a five-thread fixture, cross admissions went from 9
  of 12 back to 1 (theseus-stwkb).
- **2026-10-22: v1 patch1 hotfix, install #74a** (theseus-15kn3). A full owner card now records its attempt and scans
  nothing, and the card tender keeps a ran mark like its twins, so a full queue no longer loops the nightly run (it had
  rerun every ~0.65 s on a full queue: most of a core and ~130k journal lines a day).
- **2026-10-22: v1 patch1 tail, part 6** (theseus-4h85l, theseus-atbgl, theseus-uaqox, theseus-e3pxa, theseus-hat0c,
  theseus-m1a51, theseus-fkvb0, theseus-4v88e, theseus-8zgji, theseus-93lkm, theseus-kbfl0, theseus-zwlgz). The route
  footer's `data gate: <why>` now shows (and counts) only when the data rule kept a turn off a China-origin model it
  would otherwise have taken; the provider chokepoint and the compaction, consolidation, keep-warm and replay refusals
  count on `theseus.route.data_gate`. A turn's fetch that redirected is screened at its final URL, and a refused reply
  reservation still lands its row. The ontology sweep reads titles off the worker, asks `same_kind` for singleton
  chains and for two clusters, keeps a paid kind-name pick until the kind mints, and shows Jev-named kinds and renames
  in the morning note and the cockpit. A move landing during a thread's growth no longer commits it to the origin; a
  held compile's `route.decided` row carries the turn's real wait and steering; `confirm.list`, the push and
  `budget.list` name the asking turn, so a chat that follows a placed thread keeps its other turns' questions out.
  `route.decided`'s `effort_reason` gained `sticky_unsure`: a one-step hold of an `applied_unsure` answer (`sticky` is
  unchanged). The recall bench plans a later session at the recall tail's cap (12,000, not one 1,500 note), so the
  windows grow (smoke 56000, full 101000 at 13,943); a read-only test tree is thawed and removed by
  `scratch::remove`.
- **2026-10-22: v1 patch1 tail, part 5** (theseus-7pm8t, theseus-ddfe7, theseus-qiyxq, theseus-bs8dl, theseus-i7kmb,
  theseus-kg4kd, theseus-kbfl0, theseus-7nx13). Jev's effort answer now applies on its own bar, `[routing]
  effort_confidence` (0.35), even when its band is under the question's confirm bar (row `effort_reason`
  `applied_unsure`), at a task checkpoint too (a raise only); `xhigh` and `max` still need `high_effort_confidence`, now
  0.40 by default. A message moved while its turn still ran no longer grows the thread it was moved from; an entry the
  turn touched stays on a move; a held compile failure now counts in route stats. The DM's board pinned by an earlier
  run is removed when the task home stands. `herdr up` no longer fails when herdr refuses to bring the workspace to the
  front: it says the workspace is built but could not be shown.
- **2026-10-22: v1 patch1 tail, part 4** (theseus-v54l9, theseus-tlhjt, theseus-hwv3t, theseus-dowi0.1, theseus-4qdh8,
  theseus-rcb2u, theseus-ono8g, theseus-p5g63, theseus-63tfk.1). `theseus herdr up` switches herdr to the `theseus` workspace (`--no-focus` to skip) and says which, or
  how to attach; the TUI board's `c` refuses an ended task and takes the cursor's row; the chat footer follows a running
  job's last line (views apply by position, then time); the answer card lists waiting questions in tab order. A message
  that names a live thread by its title (every content word of it) gives that thread 0.35 pre-router evidence, so two
  titles named ask which, and one title named can tip a guess toward its thread; an open request carries no place
  digest. The Discord approval card speaks the plain words (who wants to do what, risk, why it asks, what each button
  does; owner-approved 2026-10-22), and the stand-in keeps the forums the bot made, which ended the second forum and the
  tag 404 in scratch runs. The nightly ontology sweep can also rename a stale machine category (Jev picks among names
  mined from its sessions' titles; the old name is kept in the description; undoable), join `other` answers under
  different names into one kind (`same_kind.v1`), and let Jev name a minted kind (`name_it.v1`); all at the act band and
  under the sweep's day cap.
- **2026-10-22: v1 patch1 tail, part 3** (theseus-xe77s, theseus-efb8i, theseus-94782, theseus-wy8pt, theseus-ysp2k,
  theseus-we57k, theseus-40cuo). A double quote in a raw Drive `q` now pairs up as a single quote does (`\"` escapes
  included), so `name = "a(b"` passes and an unpaired `"` is refused as unbalanced instead of slipping past the check
  that keeps a wrapped query one group. A Sheets create whose answer names an empty spreadsheet id is refused like one
  with no id, and an error after no REST call at all carries no `google.called` row. Mail now records one
  `google.called` row for each REST call, as Drive and Sheets do (a send that reads its draft and then sends is two
  rows, the send's last), where it merged them into one. The learning report's ontology undo line is a courtesy: a store
  that cannot read the undo rows leaves the line out and logs a warning, and the rest of the report is still written.
  There is one undo-rate definition (`onto_undo::undo_rates`: Jev's writes per kind in the report's window, and the
  share an `ontology.undo` names); the report now carries the window's days (`undo_days`), and the CLI and the cockpit
  say it instead of a fixed "7 days". `theseus judge replay --onto` hides every membership of a session at its first
  message (the import's, Jev's and the owner's alike) and refuses the flags it would ignore (`--split`, `--errors`,
  `--judgments`, `--report`) with nothing sent. The 2026-10-11 Sonnet 5.5 400 (`output_config: Extra inputs are not
  permitted`) was a request sent without the per-message effort beta, not a refusal of the message; a request that
  carries the server-side fallback is compiled for the profile's model alone, so a fallback model without per-message
  effort that refuses the messages answers the same 400, and the hold lands on the request's model. A read forum post of
  a finished task now leaves the forum within an hour of its 24 h (an hourly archive tick; a Clear done pressed before
  the post's end was written still reads it); the owner's DM no longer keeps a task board drawn before the task home
  stood; the voice merge's open-VAD hold is bounded by the merge window and a joined utterance keeps its barge-in. With
  the web UI off and no `[cockpit] public_url`, a reply's footer carries no inspect link and `theseus open turn` says
  how to enable one and opens nothing.

- **2026-10-22: the home-size warning fires only for a truly large home, and says something runnable** (theseus-d9je7; the
  owner, 2026-10-21: "long sessions can get quite large, and we're comfortable with those"): a place's home now warns the
  owner's DM, once, only past `[threads] home_warn_percent` (50) of the serving model's context window (about 500,000 tokens
  on a 1M model), no longer past `working_tokens`. The text names the place as the owner sees it, the size and its share of
  the window, and one dry-run `theseus thread split` command over the home's oldest settled stretch, with real `@` positions
  and an `earlier-<date>` name.

- **2026-10-21: voice layer 3 and 4's switches are proven** (theseus-sa4m5; the echo canceller joined 2026-10-19, the
  judge of unclear transcripts after it): Theseus cancels its own voice from a listener's stream (`[voice] echo_cancel`,
  default on), so a reply read back by the speakers no longer cuts itself off; `[voice] judge_unclear` (default on) lets Jev
  read a short or unclear transcript. Each switch, and a config with no `[voice]` section, now has a test that reads it
  where the call is built, since a line that stopped copying one would have left the feature dead with every test
  green (Item 255).
- **2026-10-21: v1 patch1 tail** (theseus-jt4nr, theseus-by0vj, theseus-l6u2q, theseus-1qexl). Mail from a trusted
  sender starts a task through a mail watch only when Gmail itself authenticated the sender: DMARC passed for the From's
  domain, or DKIM passed for a domain aligned with it, by organizational domain under the public suffix list (a pass for
  `co.uk` or `github.io` aligns with nothing; two hosts of one organization align). Otherwise the watch only notices. A
  silent `job_wait` posts one status line to the session's place at each beat (`[tools] job_wait_status`, on, no model
  call), and the wait's backstop looks at least once a minute. A profile a routing class chose never runs below its own
  effort (`effort_reason` `floored`), and a checkpoint can raise a turn's effort without moving it. The recall scorer
  tells an honest abstention from a misleading one.
- **2026-10-21: a reach for the owner's personal details is held in a shared place** (theseus-i1z13, finishing
  theseus-gasov's spec): when Jev reads a message in a shared place as trying to draw out the owner's personal details
  (or to open a conversation where they could come out), the turn makes no model call. The person who asked is told "I
  don't share that here.", and the owner's DM gets the case: who asked, where, the message and why. In a place only the
  owner writes in, it is still a warning only. A verdict that comes too late to be waited for neither warns nor holds,
  and a task's turn is warned about, never held.
- **2026-10-19 (night): v1 patch1's data and routing lane** (theseus-xs5ah, theseus-a1wvq, theseus-gasov):
  `[privacy] sensitive_names` go only to models of US origin, an allowlist that fails closed beneath the China-origin
  block; a conversation's turn the route placed asks a checkpoint when its loops keep failing, and moves up, as a task
  does; `route.v7` (live until v1 patch2's `route.v8`) rewords Jev's data question (content, not the place; `private`
  needs a sign; work whose owner the message does not show is `unclear`) and asks a new `personal` question, whose
  yes warns the owner by DM (since
  2026-10-21, with the reply held in a shared place: see the entry above). The template now shows the China-origin
  coding guidance (`cn_draft` and `final` on `routine_coding` and `deep_coding`, commented, with why), and
  `theseus model bench` reports each draft and its review's cost and the turns whose class drafts that did not
  (theseus-a1wvq.12).
- **2026-10-21: China-origin follow-ups** (theseus-gmyoy, theseus-5u16e, theseus-mhqlm, theseus-qul8y, theseus-y8k3u,
  theseus-z1ros, theseus-350xc, theseus-1v7fl):
  - a task the model creates (`task.create`) is tested as `task.start`'s is: no inherited open run, and an open task's
    hold from its parent names no URL or query;
  - an open request is no longer offered `task.create`, `task.split`, `task.claim` or `wake.at`, and a call of one is
    refused;
  - every root a model is shown reads `~/…`, never the owner's home directory;
  - an open session's transcript read would take the outside-text hold (the open view refuses `file.read` today);
  - a task is private while its parent holds the Google mark, even one taken after the task started;
  - `[memory] synth_profile` naming a profile of restricted origin stops the load, as the floor's seven other keys do;
  - the store format is 49 for a session's data state, renumbered at integ/release from integ/cn's 48, which
    linked threads took first (theseus-c1ve9).
- **2026-10-19 (night):** the README adds Jev's judgments and the compiled context as features 7 and 8, and features 7
  to 10 become 9 to 12; [On, and still being hardened](#on-and-still-being-hardened) gains both (theseus-dqn4h).
- **2026-10-18 (late evening):** the README adds voice, activity threads, personas and earned autonomy as features 7
  to 10, each with what is still rough, and this page's [On, and still being hardened](#on-and-still-being-hardened)
  section (theseus-dqn4h).
- **2026-10-18 (evening): the v1 cut** (theseus-pngho, theseus-zlakq, theseus-e6wm8): the README rewritten with the
  six features first and the v1 numbers, [how Theseus is developed](development.md), the workspace version 1.0.0, and
  the `release` build tagged `release-1.0.0` and installed.

- **2026-10-18 (morning):** the morning lane (Item 253, theseus-31pgz, theseus-h9tu1): a routine-coding brief starts on
  Sonnet, the progress line shows the spend against the forecast, a task homed in Reminders gets its forum post and board
  row, the ping window never holds a question, `self.patterns` runs on the blocking pool, and herdr's startup hook closes
  the focused restored shell too. Installed at install #39 (`70937771`, 11:29).
- **2026-10-18 (night to morning):** RSI loop 1's rows (Item 252, theseus-pw1q.21.1 to .5): the flow runner, the pattern
  miner, the keel guard's watch of the sealed paths, self builds in L1 and the sealed scorer's runner with its metered
  relay. Built and off (no `[self]` in the owner's config). The first three are on install #38 (`c323da54`), the last two
  on #39.
- **2026-10-18 (early morning):** the last-features rows (Item 251, theseus-ypnxt.1 to .8): pane answers, chat start and
  pause, the cockpit's Tasks page, Discord's task links and history, a task's model from its brief, one progress line, task
  history, and the acceptance journey. On installs #37 and #38.
- **2026-10-18 (01:25):** review-fixes (Item 250, theseus-oejh3): `[privacy] sensitive_names`, one list read by route.v6's
  data question, the start's pack check and a floor off models of restricted origin; a closed task channel is the
  operator's alone. On install #37.
- **2026-10-17 to 18:** the trim rows (Item 249, theseus-rwpbg.1 to .10): one name for each thing, no aliases, the dead
  code and six superseded packs gone, and the cockpit embedded as brotli and gzip. On install #37.
- **2026-10-17 (afternoon):** the China-origin catalog and route.v6's data question, the Discord task channels, war room,
  reactions, offer chip, voice, emojis and default forum (Item 248, theseus-a1wvq.1 and .2, theseus-bh3t.1 to .6,
  theseus-ss36w). On install #37.
- **2026-10-17 (01:13 to 01:36):** the task-join rows (Item 247, theseus-slmj1 and eight more): the owner joins a running
  task from every surface and returns. On install #37.
- **Not yet written as Items:** the joins of 2026-10-11 to 10-16 (this page's record of them is also missing).

- **2026-10-13:** the root user's AWS check alerts on root's sign-ins and on its calls that are not read-only
  (`readOnly` false, or none at all), and no longer on its reads (theseus-4znao, the owner's yes that morning): one
  visit to the console's Service Quotas page by root had sent hundreds of alerts in ten minutes, where now it sends
  two, the sign-in and the quota request. A change to the posture stack's one rule, through the floor. Its first
  deploy still alerted on root's reads: AWS stores a pattern with `$or` moved first, and a leading `$or` beside other
  top-level keys matched them. The pattern now has `$or` as its only top-level key, each branch carrying every
  condition, so no key order can change what it matches.
- **2026-10-11:** recall's words in time (Item 246, theseus-y9a3): the index tender answers queries at the daemon's
  priority while its own work stays lowered, so beside a busy machine a recall's word search answers in milliseconds,
  not past the 250 ms deadline (6 of October 10's 13 live recalls had nothing); a query embeds on one thread, steadier
  under load; health's recall line counts what the answers held (vectors, the words alone, or nothing). On the next
  install's list.
- **2026-10-10:** `memory.lookup` over the CLI (Item 245, theseus-8c8s): it reads one session's messages
  (`session`, paged, veiled as the rest), lists a span's own sessions beside the imported episodes, says when to use
  it instead of `theseus index search`, `history` or `sessions` through `proc_run`, and a `proc_run` of the daemon's
  own CLI for what it answers ends with a line naming the equivalent call. On the next install's list.
- **2026-10-09 to 10:** Discord's pings (Item 244, theseus-l1y1): today's pings by default; silence per category, per
  place; each place shows or hides its tool lines and its thinking; a loop's thinking tops its tool message and folds
  to `💭 thought for N s`. Installed at install #18 (October 9, 19:47). The follow-up of October 10, a thinking turn
  buzzing for its answer alone and a code fence in the thinking shown as text, is on the next install's list. (This
  page's record of installs #10 to #20 and the joins of October 7 to 9 is not yet written.)
- **2026-10-06 to 07, night:** the eleventh cloud batch's stack for AWS, health and the bench: every credential mint and
  stored secret in the AWS catalog held to a reviewed decision (Item 232), health's own sessions counted
  apart from the imported (Item 233), and the Harbor arms made comparable (Item
  234). A running job's cancel settles in two synced frames (Item 235); a stopping
  daemon begins no retry (Item 236); route.v1 asks Jev in a request of its own, on a warm connection
  (Item 237). The cockpit's phase 2, its four lanes: the watch with a sixth plate, "Since you last
  looked" (Item 238), the Ship's words, console, tour, motion and sound (Item
  239), the chart method on every page (Item 242), and the critique's
  repairs with a daylight mode (Item 243). Discord's bindings watch (Item
  240) and the turn bench's rows (Item 241). All on install #10's list.
- **2026-10-06, evening:** a learned version's promotion and rollback named (Item 222). Discord's
  bound lanes and their tests (Items 223 and 224). Five tests that failed under
  load fixed at their causes (Item 225), and the scrub's other encodings (Item
  226). The recall driver's bounds (Item 227). The core's gaps and a turn's
  reservation on the upper bound (Items 228 and 229). Voice's deaf calls and held
  replies (Items 230 and 231). All installed at 22:13 (install #9); v1-beta declared at
  22:04.
- **2026-10-06, afternoon:** voice told what was heard (Item 216). No person's name in the tree (Item
  217). Pi as the benchmarks' fourth arm (Item 218). The session lists past imported sessions
  (Item 219). A report for every benchmark run, eighteen from the retrospective, in
  [docs/benchmarks/](benchmarks/README.md) (Item 220). The judge's sink between turns, with
  `bench turn --judge` (Item 221). All installed at 17:22 (install #8), and the operator's earlier
  history imported at 17:26.
- **2026-10-06, late morning:** the tools stack: a JSON-escaped secret scrubbed (Item
  204), the gate's layers held by tests and L3 in `policy explain` (Item
  205), a decline that ends its batch's waits (Item 206).
  Discord's bindings read live (Item 207). The judge pair (Items
  208 and 209). Memory's and telemetry's tests (Items
  210 and 211). The core stack: its waits, route.v1's tests and a
  routed turn's trace, and the daemon's proofs (Items 212, 213 and
  214). Voice's echoes (Item 215). All installed at 13:03 (install #7).
- **2026-10-06, morning:** the importer for the operator's earlier history, and the store at format 23 (Item
  201). Voice's held barge-in and the floor (Item 202). The learning
  fixes (Item 203). All installed at 10:12 (install #6), with the S3 backup turned on
  and consolidation's nightly writer at $0.50 a day; the operator's store moved from format 22 to 23 at its first
  write, after the install's backup.
- **2026-10-05 to 06, night:** the eighth cloud batch's stacks T, R and S, and the bench stack. `theseus watch` ends
  its line when the daemon dies (Item 191); the turn's stack (Item
  192); history pages both ways (Item 193); every queue says why
  (Item 194); the smalls, among them the static musl build (Item 195); the WAL
  directory's sync skipped when a mark vouches (Item 196); a crashed call's hold
  booked as spent (Item 197); the durability tender's own prefix and its health lines (Item
  198); and the bench's hygiene and recall plan (Items 199 and
  200). All installed at 10:12 on October 6 (install #6).
- **2026-10-05, afternoon and evening:** route.v1's gaps, the store at format 21 (Item
  181); the reader rule's holes (Item 182); situations, enforced from the first day,
  the store at format 22 (Item 183), these three installed at 16:48 (install #5, the v1 build).
  Then the bench fixes (Items 184 to 186),
  memory's checks (Item 187), a synthesis's heading (Item 188), the
  timing flakes (Item 189) and telemetry's third pass (Item 190),
  installed on October 6 at 10:12 (install #6).
- **2026-10-05, morning:** the seventh cloud batch's fixes. The durability tender ships only synced frames, and a
  restore stitches the tails after a sealed object (Item 171). Each notification is serialized
  once for every watcher (Item 172). A job's deadline stop verifies its whole tree's kill, and after a
  crash an earlier process's provider calls settle as unknown (Item 173). Runaway mode ends on a
  raised line, and `[policy.aws]` keys are checked against the catalog (Item 174). `proc.run` takes up
  to 16 steps, and the store is format 20 (Item 175). A failed WAL sync cuts its frames back to the
  last good sync (Item 176). Health's words: the web UI's refusals, the 1-hour cache, the binary in the
  cockpit, and free space crossing a line (Item 177). A failed provider call's error class, and AWS
  requests counted and timed (Item 178). `judge.prove` from the daemon's own ledger (Item
  179). A confirmed call's run and a late result are traced in the turn that answers them, and each tool call counts once (Item 180). Earlier that morning: a job result that arrives during a restart's startup is
  taken at once (Item 166); three bench harnesses, for efficiency, concurrency and incidental
  recall (Items 167, 168 and 169); and the kernel simulator
  drives stops, tasks and the outbox under crashes (Item 170). All installed at 13:07 (install #4);
  the operator's store moved from format 16 to 20 at its first write, after the install's backup.
- **2026-10-05, night:** the sixth cloud batch, and two lanes. Retention, FSRS-6 as the `+retention` arm (Item
  157); a greeting's reply streams, and trivial messages route at 0.4 (Item 158); spreading
  activation as the `+activation` arm (Item 159); consolidation into cited syntheses, and a private
  place that draws on every session (Item 160); a trivial detour sends no recall (Item
  161); stubs and a bounded node cache (Item 162); claim leases, the task board and
  `/tasks` (Item 163); the learning loop (Item 164); and the cockpit's Budgets, Ledger
  and Policy tabs (Item 165). All installed at 13:07 (install #4).
- **2026-10-04, evening:** B5's first full Terminal-Bench 2.0 run, in [benchmarks.md](benchmarks.md) (Item
  148). Linux jobs: a job starts without a fork (Item 149), one sync per completion and background
  passes that yield to a busy machine (Item 151), and a cgroup of its own with an exact stop (Item
  152). A routed session keeps its move only while `route.v1` acts, and b5's harness fixes (Item
  150). rust-analyzer's compiler errors (Item 153). A refused request goes once
  to its fallback (Item 154). Theseus reads the files people give it, PDFs first (Item
  155). The system header says how a request is assembled (Item 156). Items
  148 and 149, and the route fix, installed at 21:22 (install #3); the rest on October 5 at 13:07 (install #4).
- **2026-10-04, afternoon:** `route.v1` live: Jev picks each message's model (Item 139). Gliding on the
  place rule (Item 140). `rerank.v1` live in front of the model (Item 141).
  `security.v3`'s live notices (Item 142). The Ship's sea rolls (Item 143).
  Replay, audit and backfill (Item 144). The ladder (Item 145). Tasks' and tools' smalls,
  among them AWS's runaway mode (Items 146 and 147). The README's animated logo
  (Item 137) and one-command setup with the `/etc` config (Item 138). All installed at 20:00
  (install #2), with the operator's model profiles; the store moved to format 16.
- **2026-10-04, morning:** the fifth cloud batch, M5's and M6's rows. The judgment surfaces (Item
  119), `security.v1` and `security.v3` at the gate (Item 120), class and
  role at inbound (Item 121), continuation (Item 122), all in shadow. Restore
  from S3 (Item 123). The exam's memory arms (Item 124). The task record and its graph
  (Item 125). Topics in shadow (Item 126). An edit's diagnostics (Item
  127). The `+rerank` arm in shadow (Item 128). The learning ledger (Item
  129). Budgets and `policy.explain` (Item 130). Compaction and the assembled
  context (Item 131). Independent checks (Item 132). Extensions that load
  (Item 133). The hands' network (Item 134). The cockpit without WebGL (Item
  135). The memory pass after each turn (Item 136). All installed at 14:09
  (install #1), the operator's config moved to `/etc/theseus/theseus.toml` and the judge turned on, every pack in
  shadow; the store moved from format 6 to 14.
- **2026-10-04, night:** the fourth cloud batch's harvest and the fifth's first joins. AWS's curated tools (C3; Item
  97). Tasks that set wakes (Item 98). Recall in shadow (Item
  99). The ontology wired in (Item 100). `theseus judge prove` (Item
  101). The user service's restart limits (Item 102). The gate's speed (Item
  103) and its flakes (Item 104). Jev wired in (23a; Item 105). MCP
  tools in turns (Item 106). The hands on Lambda and Fargate (Item 107). The durability
  tender (Item 108). Terminals as tools (Item 109). The MCP server (Item
  110). The cockpit's Ontology view (Item 111). Recall in front of the model (Item
  112). The task arrangement (Item 113). The LSP tools (Item 114).
  MCP prompts (Item 115). The hands' cancels, reservations and the hour's alert (Item
  116). Bindings' second format (Item 117). `extend.propose` (Item
  118). All installed at 14:09 (install #1).
- **2026-10-03, evening:** the cockpit is the web UI, at `/`, and the Observatory is gone (Part III, Item 86). A
  completion another reader took no longer faults a turn, and four timing tests prove by order (Item 87). A job's end
  wakes its turn, each record is written once per frame, and a tool-call turn writes 9 frames (Item 88). A trusted
  guild: every channel bound in it is private (Item 89). A config note of only what differs, and no copy of the prices
  (Item 90). Reads that don't grow with history (Item 91). The log's synced mark: rot past the checkpoint is refused,
  and the store is format 6 (Item 92). Secrets from the environment or a file, a headless run's exit codes, and the
  Harbor adapter (Item 93). The `aws` grant in a sandbox (Item 94). `security.v3` in shadow (Item 95). The LSP client
  (Item 96). All installed at 23:31; the operator's store moved to format 6 at its first write, after a backup.
- **2026-10-03, afternoon:** AWS's bootstrap on the real account, its stacks up with their guards (Item 79). The third
  cloud batch: one exam, `security.v2` in shadow, the cockpit's parity with the Observatory, and rot under the
  checkpoint refused (Item 80). The gate's quiet bar, a private address's card in a shared place, and "Should I have
  asked?" (Item 81). Installed at 16:35. The engine's store picks: one format number, an open that makes nothing
  durable, and a golden of shapes (Item 82). Voice with Deepgram (Item 83). Repeating wakes (Item 84). The engine's
  defences: the config copy acts, approvals from private places, and the CLI's check inside a job (Item 85). Installed
  at 18:52.
- **2026-10-03, midday:** the place rule replaced confidentiality labels: every place is private or shared, and
  you publish into a shared one (Part III, Item 76). Integrity's light pieces: a program that prints outsiders' text
  (`gh`) holds its session, and so does a session a command opens from a held one (Item 74). The sandbox's trims:
  listed hosts don't hold a session, no self-test at the start, no cgroup, and no sandboxed command under a root
  daemon (Item 77). One gate lock mode, with a calibrated allowance on a busy machine (Item 75). AWS's stacks,
  writes, and budget (C2; Item 78). Two cloud sessions steadied ten load-sensitive tests, fixed `op inject`'s lost
  error, and added the gate's sandbox-start bench (Item 73). All installed at 13:47.
- **2026-10-03:** credentials granted at launch: a sandboxed job takes its program's grant at its start, as any job
  does, and the run-time credential socket is gone (Part III, Item 71; installed at 03:02). The simplification
  review's housekeeping: voice out of the build, the spec's PDF out of git, CI cut to what a stock runner passes,
  and an install that builds only its five binaries, 18 % less CPU from cold (Item 72; installed at 03:32). The
  second cloud batch: the cockpit's third round, the build named in health, and the root `AGENTS.md` under its size
  rule (Item 70).
- **2026-10-03:** credentials as stand-ins dropped for v1, and the harness-only keys said aloud in `check` and
  health (Item 69). The disclosure simulator's two findings fixed (Item 68). The first cloud batch: L1's contract
  checked on a root VM (which found a root daemon's sandboxed jobs unlimited without a cgroup), every old-layout
  read test on literal bytes, and five telemetry gaps closed (Item 67).
  All installed at 01:19.
- **2026-10-02:** credentials requested at run time, built and installed at 23:15, and removed the next night
  (Item 65); the disclosure simulator (Item 66; removed with the labels on October 3). The cockpit's Ship and its time machine, boundaries board, money
  river, and speed wall (Item 64; installed at 21:24 and 22:45).
- **2026-10-02:** egress for sandboxed commands (Item 62) and graduation with the held post (Item 63), installed
  together at 21:08. Graduation is the owner's publish since October 3, and the held post is gone.
- **2026-10-02:** confidentiality labels: every new message, result, and answer records who may read it, and a
  session's context holds only what its audience may, with a placeholder for the rest (Item 61; installed at
  19:13; replaced by the place rule on October 3). Cancellation verified per backend: a cancel or a stop ends every process of a job, a detached one too,
  and says how it knows (Item 60; installed at 18:37).
- **2026-10-02:** L1 for `proc.run`, stage C's first row: a command the model asks to sandbox runs with no
  network, no secret, and its writes discarded, at notify, and never falls back to the host. At the join, a stop
  hook in the user service lets the daemon restart while a sandboxed job runs (Part III, Item 58). The operator's
  own setting for commands, or a "should have asked", now reaches a sandboxed call too (Item 59).
- **2026-10-02:** the daemon as a systemd user service, with a setup script and a how-to (Item 56), and the
  operator's config printed whole from a private overlay on the public template (Item 57; the overlay was replaced by
  the sparse note on October 3). Items 44 to 57 were installed at 14:23, when the operator's daemon became the
  service, Item 58 at 16:38, and Item 59 at 16:57.
- **2026-10-02:** the store's writer thread: one thread owns every write, with group commit, so no request waits
  on the disk on a worker, and a new segment is as durable as its frames (Item 52). One exit for a turn, a corrupt
  record that degrades only what reads it, and a crash file (Item 53). Eleven correctness gaps closed, among them
  a stop that cuts the model's stream and a disk floor that stops running jobs (Item 54). The security gaps left on
  v1's path: no grant to a launcher, no hooks for a granted git, writes outside the roots at their tool's posture,
  questions that expire, and a public template that names no one's vault (Item 55).
- **2026-10-02:** AWS's first row, the bound account and its reads (Item 49), and the index tender, stage F's
  first row (Item 50). The gate takes its shared lock only around its tests and benches, so lanes' compiles no
  longer queue behind each other (Item 51).
- **2026-10-02:** reads by state at scale and the memory target measured: 10,000 parked sessions start in about
  22 ms, and 50 turns at once peak at 104 MB against a 1 GB target (Item 46). Discord proven end to end with no
  person, in every gate (Item 47). The benches measure a turn, an idle daemon, and the binaries' size; the
  toolchain is pinned, builds repeat byte for byte, and the gate holds the code's shape (Item 48).
- **2026-10-02:** one typed fact for each thing that happens (Item 43); Review 2's security items, installed at
  14:23 (Item 44); the v1.1 roadmap (Item 45).
- **2026-10-01:** stage B's last steps: the terminal UI, `theseus tui` (Items 39 and 41), on the CLI's client
  library (Item 36); herdr panes (Item 42); `theseus reach` (Item 38); the kernel transaction (Item 41).
- **2026-10-01:** a stop that lands while a job is starting (Item 37); the repository's guides for agents
  (Item 40); the push, a session past its model's window, and a job's output at its edges (Items 33 to 35); the
  reader rule (Item 32); and Items 21 to 31.
- **2026-09-30 to 2026-10-01:** the parallel lanes for the sandbox, Jev's client and packs, MCP, the index,
  vectors, the memory math, the ontology, the installer, voice, the exam, and AWS's client, guardrails, and
  templates (Items 16 to 20).

## Known limits

- Linux only, by design (the operator, October 4): Theseus uses what the kernel offers. One daily user so far.
- The terminal UI loads a session's newest 200 messages.
- With no params, `session.list` and `execution.list` still read every record: about 140 to 180 ms at 10,000
  sessions. The other polled reads page through the index, and step over imported sessions by key (Item
  219).
- The AWS hands are built and installed, but wait for their stack and their live checks on the account; until a hand
  runs, the runaway brake has not been seen live (theseus-ongv).
- A sandboxed command holds a granted secret for its whole run, and can't ask for one it didn't get at its start.
  It has no memory limit, as an ordinary command has none.
- A daemon run as root refuses sandboxed commands, since Linux exempts root from the process limit L1 sets. Run it
  as your own user, as the user service does.
- Whether anyone else can view a Discord channel you bound private is checked when the binding starts (at the
  daemon's start, or when the place is added while it runs), so a member added later is seen at the next start.
  In a trusted guild it isn't checked at all: your word is the check.
- A store moves to a newer build's format at that build's first write, one way: the install's backup is the only way
  back to an older build.
- The record is raw (v1 patch3): a key the vault never saw is in the record, and reaches a model as written; for a model
  of restricted origin the `secret` label is the guard. A vault value whose secret the running daemon does not hold at
  that moment (failed at the start, rotated, removed) leaves as written with no `request.redacted` row, and a value cut
  in two by a size limit between the record and the exit is no longer recognised there.
- An item whose content check fails twice reaches a model of restricted origin with no label; the session gets one
  notice (v1 patch3).
- Google content is kept off a model of `cn` or unknown origin by its label, and off nothing else: a model of EU origin
  is refused nothing (none is configured today), and the owner can remove the label (v1 patch3).
- A label that a sensitive name raises is told in the session's place with the matched word, so in a shared place the
  word is visible to its members (v1 patch3).
- Under WSL, health's free-space lines watch the virtual disk under the state directory, not the Windows drive that
  holds it, which fills first (theseus-83y0).
- A `--stdio` daemon answers `shutdown` and keeps serving (theseus-yg1y); the socket daemon stops.
- An erase of imported history hides it: its text stays in the WAL, in backups and in the S3 copy until the spec's
  in-place redaction (§5.6) is built, and an erased episode cannot be imported again under any tag (Item
  201).
- In Discord, on the installed build, a place added while the daemon runs whose session cannot open is not tried
  again (theseus-u6v6; fixed by Item 240, on install #10's list).
- Voice's speech prices are Deepgram's published rates, assumed until confirmed.
- On the installed build, `aws.call` answers three newer credential mints (two of STS's, one of EKS's) with their values
  instead of a handle (theseus-ye7o), and health's session count includes the imported sessions (theseus-revl); both are
  fixed by joins on install #10's list (Items 232 and 233).
- A deaf voice call is noticed and rejoined once, but the rule's live input, the clients' reports arriving, has not been
  seen in a real call yet (theseus-d93y), and a listed speaker who arrives after the join is held to the call's clock,
  not their own, until theseus-kcng.
- The imported history's vectors embed in the background over days, so until then a search finds an imported episode by
  its words, not its meaning.
- At install #9 the old daemon took 11.87 s to stop, with nothing logged between (install #8's took 298 ms;
  theseus-vjn7), and the old index tender had stopped answering its status socket for hours, which the restart cleared
  (theseus-uazd).
- Replay and audit runs each keep a spend cap of their own and do not draw on the judge's day budget, which the
  October 4 default asked for (Item 144).
- On this machine a turn's harness overhead reads well over the README's 5 ms, mostly the disk's fsyncs under WSL,
  in debug builds. A measurement on a release build is still to come.
