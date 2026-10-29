# The Ship of Theseus, chapter 31: Part III, A4's Items 244 to 259 (Discord's pings, and a loop's thinking in its tool message; memory.lookup over the CLI; recall's words in time; the joins of 2026-10-17 and 10-18: owner joins a running task, the China-origin catalog and the Discord task places, the trim rows, the sensitive-names floor, the last features, RSI loop 1, the morning lane; a reach for the owner's personal details held in a shared place; voice layers 3 and 4's switches proven; Deepgram Flux as a call's default listener; live learning (the observer, the card's live pass, live questions in the DM); the principles migration (four labels, two deterministic sources, one switch, a raw record with substitution at the exits)) ([index](README.md))
### Item 244. Discord's pings: today's pings by default with silence per category and per place, each place showing or hiding its tool lines and its thinking, a loop's thinking at the top of its tool message folding to `💭 thought for N s`, and a thinking turn buzzing for its answer alone (theseus-l1y1; theseus-l00z's wave A, the Discord report's section 4.7 and plan step 1; the cloud row discord-silent, fired 2026-10-09 02:05Z, Opus 5.5, its first cut silent by default; overruled by the owner at 2026-10-08 20:01 and redirected as discord-silent-cont, fired 04:21Z; the first review, review-discord-silent, accepted with a join fix, per place and the window off; the owner's two calls of 2026-10-09 08:26 sent as discord-silent-fold, fired 15:53Z, and its review, review-discord-silent-fold, accepted with a join fix (F1); joined 2026-10-09 16:22 at 9b439511, the merge eed6df88 of 292bb147 onto 4882558a and the fix's two picks, by the evening joiner; installed at install #18 (d32377d5, 2026-10-09 19:47); the owner's open calls F2 and F5 and the low findings F3, F4 and F6 made up by lane L4 discord-calls on 2026-10-10, joined by its own join; the follow-up not installed yet)

_The record of the joins of 2026-10-07 to 2026-10-09 before this one is not yet written as Items; this Item is
numbered after the last one written, and they go before it when they are._

**Why.** In the owner's ledger, 53% of 217 messages the binding sent pinged his phone while asking nothing. The first
plan was silence by default: only cards, failures, the first part of a reply to his own message, and disk critical
would ping. The cloud row built that, and the owner overruled it the evening it was built: he wants a lively chat, in
which everything said in a channel is worth a buzz, and thinking and tool lines shown, configurable for those who want
it quieter. So the mechanism stayed (every write says whether it pings, and a table gives each write its category),
and the defaults became today's behaviour, with silence as the opt-in. His two calls of 2026-10-09 morning then set the
rest: (3a) a loop's thinking folds into that loop's tool message, with no message of its own and no extra ping, and
(3b) silence is per place, with the daemon's list as the fallback.

**What landed**
- **Every write says whether it pings.** A create that doesn't carries Discord's `SUPPRESS_NOTIFICATIONS` (flags
  4096; mentions notify no one either), as a notice embed does; an edit never notifies. `policy::TABLE`
  (`crates/theseus-discord/src/policy.rs`) maps each write to its chat category: `cards`, `failures`, `answer` (the
  first text part of the reply to an owner's own message), `later_parts`, `woken`, `tool_lines`, `reports`,
  `notices` and `ops`. The `discord.message.out` row says `ping` and `held`.
- **Today's pings by default.** With no config every chat message pings, as before (17 events,
  `policy::with_nothing_silent_every_chat_message_pings`). A write pings unless its place's `silent` list names its
  category: its `[[channel]]`'s or `[[dm]]`'s own list, else `[discord] silent` (empty by default). Three writes have
  no category and never ping: a card whose question closed while Discord was away (written once, settled), the task
  board, and a loop's process message that holds its thinking. `ping_window_secs` (a place's, else `[discord]`'s; 0,
  off, by default) holds a channel to one ping in that long; a ping it holds goes out silent.
- **Each place shows or hides its tool lines and its thinking.** `show_tools` and `show_thinking` on a
  `[[channel]]` or a `[[dm]]`, both on by default, read live with the rest of the bindings file.
- **The fold (3a).** A loop's thinking (`model.thinking`) streams as `-#` lines at the top of the loop's process
  message, `<turn>:L<n>:tools`, made at the loop's first thinking or first tool line, whichever comes first; once the
  loop's text starts (its first text, `model.answered`, or the loop's end) it folds to `-# 💭 thought for N s`, N its
  own seconds rounded up, never a tool's run time. There is no thinking message of its own. It shows at most what fits
  beside the tool lines in 2,000 bytes, saying what it left out; the whole thinking is the session's history
  (`theseus history`).
- **The first review's join fix** made silence per place and turned the window off by default; **the fold review's
  join fix (F1)** made a reply's post class its first text part the answer whenever the owner's message is still
  owed its answer, whatever else of the turn the stream wrote (the fold makes the process message a key the stream
  writes first, so under load an answer was classed a later part).
- **The follow-up of 2026-10-10 (lane L4).** **F2, the owner's call:** a process message that holds thinking never
  pings, its tool lines with it (`render/process.rs::holds_thinking`, read by `policy::of_live`), so a thinking turn
  buzzes for its answer alone. Before, a tool line that landed in a silent thinking-made create owed its ping to the
  turn's next thinking-made create (`Lane::process_ping`), so the buzz for loop i's tool line came when loop i+1
  started thinking, and the phone's preview showed that thinking; which create carried it depended on whether the
  tool line reached the lane before the create. The debt and its two fields are gone. A loop that does not think pings
  for its tool line as before, and the last loop (thinking, then the answer) still makes one silent create main does
  not. **F4:** the streaming thinking escapes its backticks and backslashes, so a code fence in the thinking no longer
  opens a block over the tool lines below it until the fold. **F5:** `theseus_protocol::notices`' module doc said
  Discord pings for the same reasons as the TUI; it now says the TUI and herdr read that policy, and that the chat's
  buzz is the binding's own table.

**How it is proven.** Through the stand-in's gateway and the fake Discord, which keeps each create's flags:
`tests_silent.rs` (no config, each category silent, a place's own list, a 30 s window daemon-wide and a place's),
`tests_show.rs` (a place that hides its tools and thinking beside one that says nothing; a rewritten file read live),
and `tests_process.rs`: a turn of two thinking tool loops and a thinking answer makes 4 creates and 1 ping (the
answer; every process message silent), against 3 creates and 3 pings for the same turn without thinking; a thinking
answer alone, 2 creates and 1 ping; `show_thinking = false`, 3 and 3 with no `💭`; `silent = ["tool_lines"]` in one
channel, a turn without thinking 3 and 1 there and 3 and 3 in the DM beside it; and F1's test, which holds the
thinking's create at the fake until the reply's post is pending. F3's unit test folds a tool loop at `model.answered`
1.2 s after its thinking and not at its end 9 s later (`thought for 2 s`); F4's shows a fenced thinking escaped above
its tool line; and F6's (`courier/tests_order.rs`) runs a lane against the fake with a post and a new thinking message
both queued before it starts: the thinking goes first, and a tool line alone after the post. The Discord proof in the
gate: 12 of 12 steps, all 7 of the bot's creates pinging. Each follow-up behaviour has a planted revert its tests
caught: the two halves of F2 (a tool line under thinking pinging, and a process message with thinking pinging) failed 5
and 4 tests, among them the end-to-end ping counts; F3's, F4's and F6's each failed its own test.

**What the review found.** The fold review (accept, with F1's join fix): F2 for the owner (the extra silent create,
and the owed ping a loop late); F3, the tool loop's N guarded by no test (removing the `model.answered` fold arm failed
none); F4, a fence in the streaming thinking; F5, the two documents; F6, the lane-order fix (a thinking-made create
queued before the lane's next post) argued, not tested. All five are made up by the follow-up.

**The join** (the evening joiner, 2026-10-09, lock `discord-silent-join`). No conflict on 4882558a; the merge
eed6df88 committed alone, then the review's join fix cherry-picked with `-x` (85c5fca7, 9b439511), each step's tree
checked against the dry runs. The keel guard over 4882558a..9b439511 (set by `THESEUS_KEEL_BASE`, since the gate's own
base was then empty for a join whose head is a pick after its merge): 24 files, 0 findings. The gate green: suite
3,670 of 3,670, lifecycle OK on its first run, turn frames 5 and 9. The follow-up joined by lane L4's own join on
2026-10-10, a plain signed merge.

**The install.** The first part at install #18 (d32377d5): two new `[discord]` keys in the template (`silent = []`,
`ping_window_secs = 0`) and four commented per-place keys in the bindings example; the owner's config names none of
them, so every message keeps pinging. The follow-up waits for the batched install after the joins of 2026-10-10; it
adds no key.

**Divergences.** The first cut's silence by default, overruled. The `thinking` category, which only the first cut had,
is refused at load. The fold's "no extra create" is missed by one silent create for a turn's last loop, unavoidable
while the thinking shows as it streams in a message of the loop's own (the loop cannot know at its first thinking
whether it will call a tool). "No extra ping" became fewer pings for a thinking turn (F2, the owner's call): only its
answer buzzes.

**Known gaps.** theseus-o268 (thinking and tools in summary form, status reactions, `/show` per place). The owner's
live check on the test channel (the review's six steps) is still his.

### Item 245. `memory.lookup` reads a session and lists a span's own sessions, says when to use it over the CLI, and a `proc_run` of the daemon's own CLI ends naming the native call (theseus-8c8s; lane L11 native-memory-tools of 2026-10-10, joined by its own join; not installed yet)

**Why.** On 2026-10-10 the owner's DM asked what had been worked on in a past month. The model answered with four
`proc_run` calls of `theseus index search` (its help, a search, a shell loop of date strings, and a loop to read
sessions), concluded that the search could not keep to a date, and paid a shell for each. `memory.lookup` (theseus-w9qv, Item
not yet written) does exactly that read with `when`, in-process, Read and Async. The evidence, read-only on the
owner's daemon: the turn's compilation (`compilation.list`) offered 51 tools with `memory_lookup` among them, class
private, posture notify, not deferred and not disabled; so the tool was offered and the model preferred the CLI.
That compilation was made at 13:26 that day, the first in the DM to offer the tool; the context before it held the
README's line naming `theseus index search` and an earlier `proc_run` of it that had worked, from before the tool
existed. The tool's description named none of the words the model used (search, index, sessions, a date), the tools
note's "prefer these over proc_run" sentence names only the fs and git tools, and the turn's own recall ended at its
deadline with nothing (theseus-y9a3), so no recalled note framed the question as memory.

**What landed**, by the owner's levers in order (the API, the schema, the description, result metadata; a prompt
sentence only if a bench arm earns it):
- **The API: what the model needed and the tool lacked.** `session` reads one session's messages, oldest first, a
  page at a time within the call's token budget: an imported one leads with its episode's summary; tool calls and
  results, and an imported message of outside text, are left out and counted; a session takes its episode's
  sensitivity (personal and partner-confidential veiled, a catalog miss veiled, fail closed), and the asking session
  must be private, as for the rest. `session` with `words` searches that session alone. A span with no words, book or
  topic lists this harness's own sessions in it (a turn at least, from their start to their last activity) before the
  imported episodes, one cursor across both: the catalog listed only the imported ones.
- **The schema**: every field says what it takes, with an example (`"March 2026"`, `"since August"`, `ses_…`).
- **The description** says when to use it (any question about what was said, done or worked on before), that it
  stands instead of `theseus index search`, `theseus history` and `theseus sessions` through `proc_run`, and that
  several calls in one response run at once (the parallel calls of theseus-d1hi).
- **Result metadata** (`native.rs`): a `proc_run` of `theseus index search` or `memory search`, `history`,
  `sessions` or `sessions list`, or `rpc session.history` or `rpc import.sessions`, given directly, inside a shell's or
  a launcher's command (split on its quotes and operators), or in a batch's step, ends with one line,
  `[native: memory_lookup {"when":"March 2026"} does this in-process, without a shell: …]`, naming the equivalent
  call: a query that is a time goes to `when`, the rest to `words`, a query in a shell variable gives the call's
  shape. It is appended after the cap (`ResultNode.hint`), so no cut takes it, and only where the runtime offers the
  tool. Nothing else of the CLI has a native twin today, so nothing else gets a line.
- **No refusal.** A pure read of the daemon's CLI still runs: the CLI is the right thing to run when it is the work
  (a check of its output, a bench, a bug in it), and the gate never guesses what a command is for (NOTIFY OVER BLOCK).
  The line costs one result line; a refusal would cost a turn whenever the CLI is the work.
- **No prompt sentence.** The tools note is unchanged.

**How it was proven.** 19 tests (`tests_memory_lookup.rs`, `native::tests`), 14 new: the owner's DM (a private
place) is offered the tool; a `proc_run` of the CLI there, through a shell and directly with an output past the cap,
ends with the line naming `{"when":"March 2026"}`; a session's read in order, paged, its summary first, outside text
left out and counted, veiled, refused to a shared place; a span's own sessions first, paged across the two; words in
one session; the description and every field's. Ten planted reverts, each caught: the line first instead of last,
no line on jobs, no shell split, a time not sent to `when`, a read unveiled, outside text kept, own sessions of any
span, a session not narrowing words, the old description, and the read before the place check. A live check on
scratch daemons (the lane's report has its harness): two frozen debug builds, the lane and the same
tree with the description, schema and line reverted, a synthetic store of invented episodes (February to April
2026), the index on, Sonnet 5.5 at medium; 14 trials, $0.68 in all. In the arm with the change, a turn told to run
`theseus index search telemetry` through `proc_run` got its result ending with
`[native: memory_lookup {"words":"telemetry"} …]` (read in the store of two of its three trials, the third's harness
not yet recording it).

**Where it diverged.** The live check did not reproduce the DM's miss: in short sessions Sonnet 5.5 called
`memory_lookup {"when":"March 2026"}` first, and only it, in all 14 trials of both arms: fresh (1 each), with a
README note naming the CLI before the question (3 each), and with that note and a turn that ran the CLI before it
(3 each). The change's arm cost 1 to 3% more a trial ($0.0427 against $0.0423 fresh, $0.0451 against $0.0443 with
the note, $0.0550 against $0.0533 with the CLI turn), its longer description. So the A/B measures no preference change at that scale; what it shows is that the
miss needs the DM's conditions (a long context full of the CLI, Fable 5.1, no recall), which a sub-$2 replay does
not build. The line is the lever that works whatever the context: it is deterministic and tested.

**Left open.** theseus-y9a3: 42 of 57 live recalls of 2026-10-10 ended at the deadline with nothing, not even the
words' fallback.

### Item 246. A recall's words answer in time: the tender answers queries at the daemon's priority and its own work stays lowered, a query embeds on one thread, and health counts recalls by what their answers held (theseus-y9a3; lane L12 recall-deadline of 2026-10-10 to 11, joined by its own join; not installed yet)

**Why.** Lane L11 found the DM's March question had no recall framing it: its recall ended `deadline` at 251.8 ms
with nothing, not even the words' fallback (theseus-w9qv), and the ledger's `recall.ran` rows read 42 `deadline` of
57. Read again for this item, those 57 rows span 2026-10-04 to 10-10, and two regimes are in them. Before the words'
race joined (10-09) a recall asked one query, and a warm model's embedding of a 500-character query took past 250 ms
under load: 36 of the 42. Since: 6 of 2026-10-10's 13 live recalls ended at the deadline with nothing, 5 had the
words alone, and the 2 `ran` had the words alone too (their vector source skipped, "the model is loading"); no
recall since the race had vectors. Each of the six began as the tender's model began to load (unloaded after ten
idle minutes), but the model is not what held them: the words' query never waits for it.

**The cause.** The tender lowered its whole process to nice 10 in the idle I/O class (row 51), its socket's threads
with it. Beside the builds at nice 0 a nice-10 thread gets about a ninth of a nice-0 one's share, and the idle class
reads the disk only when nothing else does: a recall's word search, 3 ms of work, waited for a core, and a cold one
(its pages evicted in the quiet spell that also unloaded the model) waited for the disk too. Reproduced on a copy of
the store and index with a scratch tender, 48 busy loops at nice 0 on 16 cores standing in for the builds: the
installed tender's first, cold words answered at 453 ms (BM25 167 ms, the documents' load 187 ms: the rows'
signature), and a warm 3.9 ms one at 248 ms.

**What landed.**

- **Queries at the daemon's priority** (`theseus-index`'s `priority.rs`, `open_served`). The socket's thread is
  started first, at the priority the tender started at; then the calling thread lowers itself (nice 10, idle I/O)
  and opens the tender, so tantivy's threads, the ingest and the embedding thread are born lowered; then the socket
  is handed its `Shared` and binds. A thread cannot take a priority back (`RLIMIT_NICE` is 0 under the daemon's
  unit), so the order is the fix. A tender refused its index never binds, so it never takes another's socket.
  `index.status` says both (`queries_at`, `work_at`). The queries' embedding pool is born of the first query's thread,
  so it runs at their priority too.
- **One query thread** (`ahead::DEFAULT_QUERY_THREADS`). A query's embedding took half the cores, up to four, chosen
  when it embedded 128 tokens. At the 32-token cut (theseus-zo1y) one thread embeds in about 90 ms against 75 on four
  when the machine is idle, and beside a busy one four lose: each matmul is split across them and waits for its
  slowest job, and a preempted thread stalls the layer. `[index] query_threads` still names more.
- **What a recall's answer held.** Health's recall line counted outcomes, and a `ran` whose vector source the
  model's load skipped counted as ok though it had no vectors. `RecallManifest::answered` reads what a row's answer
  held (`vectors`, `words`, `empty`) from fields every row has, so the ledger's old rows read the same;
  `RecallOutcomes` counts `with_vectors`, `words_alone` and `empty`, and the line ends "held N with vectors, M words
  alone, K empty".

**How it was proven.** Tests: `tests_priority.rs` (a query answered at the starting priority while the work runs
lowered; a tender refused its index leaves the other's socket), `ahead`'s default, `recall::outcomes`' counts, the
CLI's line, and `tests_recall_deadline`'s stand-in tender whose words' query answers late (the vectors in time, the
words alone, nothing past the deadline). Five planted reverts, each caught: lowering before the socket's thread,
binding before the open (caught once the test waited for a stray bind), `answered` reading the wrong source (by two
tests), the line without the held counts, and the old thread sizing. The live A/B, one tender binary, two arms
(the old whole-process lowering, as `nice -n 10 ionice -c3`, against the change), the copy's pages and the model's
evicted, 48 busy loops at nice 0 on 16 cores, a recall's two queries every 1.5 s: the words answered in 8 to 496 ms
with 2 of 16 past 250 under the old lowering, in 8 to 58 ms with none past it under the change; with the model
warm, the vectors came in 1.7 to 5.2 s under the old lowering and in 233 to 680 ms under the change (four threads).
By query threads, the model warm: idle, a median of 90 ms on one and 75 on four, all 16 inside 250 on each; with 24
busy loops, 10 of 12 inside 250 on one, 6 of 12 on two, 1 of 16 on four; with 48, 9 of 16 on one, 1 of 16 on four.

**Where it diverged.** The brief suspected the tender's cold model, the words' query queued behind the vector one,
the query pool, or a lock. The words' query goes first on its connection and touches neither the model nor a vector
lock; the cold model only marks the quiet spell that also evicted the index's pages. Vectors under heavy load are
still late (the A/B's 48 loops: 9 in 16 on time): the words now always come, and health says when the vectors don't.

**Left open.** The model loads on the embedding thread, at nice 19 in the idle I/O class: beside a busy machine a
load took 4 to 9 s on 2026-10-10 (and none finished in the A/B's 35 s burn), and recalls in that time hold the words
alone. Warming on a keystroke is its own row.

_The record of the joins of 2026-10-11 to 2026-10-16 is not yet written as Items. These seven Items, written on 2026-10-18
from each merge's message and its branch's commits, cover the joins from 2026-10-17 01:13 to 2026-10-18 10:21. Where a
merge message names no proof, the Item says only that the row was reviewed, and names the test its join fix added._

### Item 247. The task-join rows: the owner joins a running task from every surface, and returns (theseus-slmj1, theseus-xock5, theseus-go4ig, theseus-peqbl, theseus-mu0e6, theseus-jm46q, theseus-esxyb, theseus-rrkb7, theseus-m638c; nine cloud rows of 2026-10-16, joined 2026-10-17 01:13 to 01:36; installed as #37, `96c592c0`)

**Why.** A task ran apart from the conversation that started it: the owner could watch it, and a message to it was held
until it ended or lost to it. The plan (theseus-l00z) was inspect, join and return: look into a task, say something to
it while it works, and get back to the conversation, the same on every surface.

**What landed.** The kernel's half first, then each surface on it.

| Row (merge) | What it built |
|---|---|
| task-join-core (`756bcfeb`) | The owner's input steers into a running task (`Core::owner_input`: the owner, in a private place, not a job's and not a prompt's); a task whose last call had the owner's message offered parks on input and reports from the next turn; the end frame writes the joined count (a META record, no store format change) and the parent's report ends `(you joined: N messages)`. `joinable()` and `task_redirect_line()` are the protocol's, so every surface says one line when a message cannot join. |
| chat-board-gaps (`8ed50fc3`) | The chat's footer says what tasks spent, `/tasks` lists routines, watches and projects, and the chat reads the seen file. |
| cockpit-notices-policy (`6ca11a40`) | Desktop notices for questions, failures, reminders and finishes, not approvals alone. |
| herdr-horizon-digest (`45d384bf`) | herdr's Space row says what happened since the owner looked, the next reminder and the routines due. |
| tui-task-return (`3c72841e`) | The TUI's task pane names its parent, `p` goes back, `I` inspects, and a message for an ended task goes to main. |
| cockpit-task-page (`88c61b6d`) | A task's page has its header, joins the task, and sends to main once it has ended. |
| discord-task-join (`3f045da8`) | The owner joins a task from its Discord thread or its Follow mirror; a guest's message, and one for an ended task, goes to the conversation with the one line. |
| chat-task-view (`ba931256`) | The chat inspects, joins and returns from a task, and `theseus chat --task ID [--inspect]`. |
| herdr-task-return (`f37900de`) | herdr's prefix+m goes back to the main tab, prefix+i inspects a task, and a task pane has a header. |

**Join fixes** (each a commit after its merge): the cockpit's notices read the daemon's own `work.notice` pushes in
place of a TypeScript port of the policy (`c7f6e15d`); the redirect line carries every surface's reason and the
TUI, the cockpit and the chat keep task-join-core's helpers (`68c9fa44`, `fc197213`); Discord sends the owner's origin so
the core takes the message as the owner's and steers the running task (`8569a95d`).

**How it was proven.** Each row was reviewed, in writing, before its merge. The join fixes each added the test that
holds what they changed: a turn the owner's message began counts as joined (`5aab328a`), a question view counts as seen
by its task's mark (`6278af17`), a joined task's line is the message and never an inspect key (`eedbfc7b`), the task
pane's header, and its absence on a conversation's pane (`fa102956`), and the joined count while the task runs on
Discord (`c6f62abf`).

### Item 248. The afternoon rows of 2026-10-17: the China-origin catalog and route.v6's data question, the Discord task places, reactions, the offer chip, voice, emojis and the default forum (theseus-a1wvq.1, theseus-a1wvq.2, theseus-bh3t.1 to .6, theseus-ss36w, theseus-6ifz1; ten rows, joined 2026-10-17 14:51 to 16:45; installed as #37, `96c592c0`)

**Why.** Two plans met in the same files that afternoon: the owner's rule that Google Workspace and partner matter never
reach a model of China origin (the model-origin half, theseus-a1wvq), and tasks given a place of their own in Discord,
a way to be paused, shared and spoken about (theseus-bh3t, theseus-ss36w). They were reviewed one at a time and merged
in a run, each resolved beside the others.

**What landed.**

| Row (merge) | What it built |
|---|---|
| cn-catalog (`45f86304`) | A model's origin by its endpoint, its family and its catalog row (`origin_of`); DeepSeek's rows; GLM's implicit thinking and three effort levels; Sonnet 5.5's cache read at $0.10. The joiner's live probe of `glm-5.3-flash` took `output_config`'s effort (no 400; output tokens 9 at low, 26 with none, 44 at max) and a thinking block went back on a tool turn, so GLM's effort stays on. A join fix reads a GLM the catalog does not know, behind a host that vouches for us, as `cn` by its family (`7e3730ce`). |
| cn-route-v6 (`5b18838f`) | `route.v6` asks Jev's data question (open, private or unclear) beside route.v5's four, with the operator's `[routing.data]` names in its state. Nothing acted on the answer at the merge: Item 250 wires it. |
| discord-task-channels (`fbd326e1`) | A private channel for each task in a private place with two plan steps, `expect = long`, or still working after ten minutes; a done category, read-only, deleted after 14 days. `task.create` gains `expect` (about 20 tokens; the first-request budgets unchanged), kept as the META key `task.expect.<session>`. |
| discord-war-room (`57d50bd6`) | `/task name:<task> share:@user` and `unshare:`; a non-owner's message in a private task's place is a comment that reaches no session. |
| discord-reactions (`03ab67d0`) | The owner's reaction on a task's message pauses, resumes, stops, retries, follows or pins it, and the core's `task.pause` and `task.resume`. |
| task-offer-chip (`f25bcc3c`) | `classify.v3` asks whether the owner's message is work for later, and a yes at the act bar puts "Make this a task" on the reply. |
| voice-tasks (`6846c432`) | In a private call a task's question and report are spoken once at the next pause, and "approve", "the second one" or "stop it" answers as the owner (`[voice] speak_tasks = false` turns it off). |
| discord-emoji-catchup (`a5df3b4e`) | The bot's animated emojis for task states with the Unicode fallback, the 08:00 and 18:00 digest post when something happened, and one catch-up message after an outage. |
| herdr-wire-fix (`0f93aa81`, a local lane) | herdr's real API, the main and board tabs at boot, and a schema contract test (theseus-6ifz1). |
| discord-default-forum (`c3801e51`) | One private forum in the owner's guild with a post for every task; a task with no other home works in its post, any other gets an index card. |

**Join fixes:** a shadow classify judgment offers no task (`432bbbeb`), a resume that races a task's park still wakes it
(`f5923052`), a shared channel's task that works on with a two-step plan gets no channel (`fdb3037c`), and a task with a
channel of its own has an index card in the forum (`a7e63827`, `0a44e1f1`).

**How it was proven.** Each merge's message says it was reviewed, in writing, and what was resolved beside which rows
(the packs' count, 24 once task-offer-chip and cn-route-v6 had each raised it by one; the stand-in Discord's routes
kept by every row). The invite permission integer for the task channels is 326686067792. The behaviour has the tests of
the rows' own modules (`tests_task_channels`, `tests_war_room`, `tests_default_forum`, `tests_board`).

**Left open.** Nothing acted on `route.v6`'s data answer until the review-fixes row (Item 250). The rule that Google
Workspace content never reaches a model of China origin is the owner's stated policy; what enforces it today is the
model-origin floor of Item 250, which a turn that names a sensitive name triggers, not Google-sourced content as such
(theseus-a1wvq.3).

### Item 249. The trim rows: one name for each thing, and the dead code gone (theseus-rwpbg.1 to .10, theseus-buz3h; twelve rows, joined 2026-10-17 20:18 to 2026-10-18 00:52; installed as #37, `96c592c0`)

**Why.** The owner's direction of 2026-10-17: "more important to be right than to be historically conservative this
early", so aliases, doubled names and dead code go, and every caller is fixed in the same row.

**What landed.**

| Row (merge) | What it built |
|---|---|
| trim-docs (`bf696c9b`) | `docs/cli.md` and `docs/glossary.md`; internal ids out of the one-line help. |
| trim-security-v1 (`065f1bf5`) | `security.v3` alone at the gate; the brake and the notified call's score move to v3; stored v1 judgments stay readable. |
| trim-config-inherit (`91d0b889`) | A profile's unset field takes `[model]`'s; `--sparse` compares resolved profiles; every resolved profile is pinned. |
| trim-effort (`ecb6edbf`) | Sticky effort; keep-warm replays the last effort; per-message effort is catalog data (join fixes: Sonnet 5.5 takes per-message effort, as the live probe showed, and Opus 5.5 stays sticky, `3c4223c7`; a Sonnet 5.5 turn with no level from Jev carries the per-message default, `d2d81749`). |
| trim-utils (`6987413e`) | One clip and cut, one `now_unix_ms`, one civil calendar, one JSON-RPC framing; the wire's own span and reply; 35 dead pub items gone. |
| trim-keel-deletions (`000f1864`, part C only) | The gate clippies the judge crate's probe feature. Parts A, B and D, the deletions, were not done at the merge: its session refused to delete tests. |
| trim-binary (`34629c73`) | The cockpit embedded as brotli and gzip and served by `Accept-Encoding`, no unwind tables in release builds, one brotli decoder, `theseus-sim` built but not installed. |
| trim-tokens (`e7573983`) | Learned tool loads inside the place budget and only calls count as use, a shorter tools note, reminders split from tasks, keep-warm stops at break-even, index commits coalesced. |
| trim-names (`3d14103d`) | The everyday help and `help all`, noun-then-verb folds, one right name for each thing (task class, rung, `on_max_loops`, `[route_bench]`, `[persona_import]`, one `[cache]`, `[watch_for]`); an old config key fails naming the new one. |
| trim-tests-dedup (`40aa83cd`) | One copy of each identical test helper. |
| the answer rename (`c579c49b`, rwpbg.10) | `confirm` is `answer` and `executions` is `sessions --state`, with no aliases; every caller (the goldens, the daemon's tests, bench/async, the smoke script, the guides) names the new spellings. |

**The deletions, done late.** Parts A, B and D of trim-keel-deletions landed on 2026-10-18 at 00:11 as three commits, each
on the owner's approval of the trim review's part 1 and each carrying its `Keel:` acks: the startup unit-budget
rewrite and the retired config keys (`fd33868c`), six superseded pack versions leaving the build (`f6ade10d`), and the
dead code that never landed, with its tests (`b88b360c`, among it the `task.get` RPC: a task is read through
`task.list`).

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Parts A, B and D with the row's merge | Parts A, B and D about four hours after | The row's own session would not delete tests | Done by the joiner on the owner's yes, each ack naming what was removed |
| The keel guard reading a renamed cap key as unchanged | It read the renamed cap keys as removed | A rename is a delete and an add to the guard | Acked with the owner's words on the rename; every value is unchanged |

**How it was proven.** The merge messages name the review, and the join fixes name the tests they put back: a family the
message names is counted on the tools alone (`154c52e7`), the keep-warm effort test runs on a model without per-message
effort (`9862782a`), setup's test installs four binaries and never `theseus-sim` (`f3a1cb3f`), and the joined tree's
tests are in trim-names' words (`c7ad21f7`, `76a80491`). The kernel simulator starts at seed 2 after part A, since with
three fewer seeded executions its random walk moved and seed 1 no longer reached a report that woke its parent.

### Item 250. review-fixes: one list of sensitive names, and a floor off models of restricted origin (theseus-oejh3; a cloud row, joined 2026-10-18 01:25, `0daac1fa`, with its join fixes; installed as #37, `96c592c0`)

**Why.** The owner's rule that a name's matter never goes to a model of China (or unknown) origin needed one list that
every reader of it shared, and a closed task channel needed to be the operator's alone.

**What landed.** `[privacy] sensitive_names` is the one list. Route.v6's `data` question reads it as its private names
(`[routing.data]` keeps `open_topics` alone; the old `private_topics` key is refused), a message that hits a name makes
the verdict's `data` answer `private` at 1.0, the start checks the embedded packs' text against it
(`Core::check_pack_text_after_serving`), and a floor keeps the turn off every profile whose origin is `cn` or `unknown`
(`route_profiles(true)`; none left, the turn ends before any model call, class `data_gate`), with a guard at the
provider chokepoint that refuses a request to such a model holding a name in any string of its system, messages or tools
(`data_gate::guard`). A closed task channel's overwrites are replaced whole (the operator's read and the bot's alone) and
guests are removed from a task's thread or channel at its end.

**Join fixes:** the route's floor reads the whole request, so a name said earlier keeps the next turn off restricted
models too (`f7d202a8`); T28 reads the list, and `private_topics` is in the renamed-key table (`04947173`); the floor's read moves
into `decide_floored` under clippy's line limit (`96c592c0`); and a task's brief is judged with the sensitive names and
routed off restricted profiles like a message (`ace0e857`, with task-model-by-brief).

**How it was proven.** Reviewed; the tests are `tests_data_gate`, `data_gate`'s own, and `tests_route_data`, and the
catalog's rule that a config table cannot loosen a built-in `cn` origin (`a_config_table_cannot_loosen_a_builtin_cn_origin`).

**Left open.** The floor is triggered by a name on the list, not by where content came from: Google-sourced content that
names no listed name is not held back by it (theseus-a1wvq.3).

### Item 251. The last-features rows: the pane, the chat, the cockpit and Discord start, pause, answer and find tasks, one progress line everywhere, and the acceptance journey (theseus-ypnxt.1 to .8, theseus-h9tu1; eight cloud rows, joined 2026-10-18 01:27 to 03:26; the first three installed as #37, `96c592c0`, the rest as #38, `c323da54`)

**Why.** The owner's target for v1: start a task, see it, answer it, pause it and find it later, from any surface, in
the same words.

**What landed.**

| Row (merge) | What it built |
|---|---|
| pane-answers (`2b1710be`) | A herdr pane answers an `owner_ask` by its number or in the owner's own words; a message after the task's end goes to main; prefix+p pauses or resumes the pane's task. |
| chat-start-pause (`a7131e02`) | The chat's `/task new` and "make that a task", `/pause` and `/resume`, the TUI's `P`, and a task picker that searches ended tasks too. |
| cockpit-tasks (`d8728305`) | The cockpit's Tasks page with a New task box, Pause and Resume, the progress line in TypeScript, and a header that counts working tasks. |
| discord-tasks (`373bacb7`) | A task's report links its own post or channel, `/tasks done` and `all` read the store, the offer chip that arrives late, voice "pause it" and "go on", and the cockpit link on the configured port. |
| task-model-by-brief (`a1354a72`) | A task's model is routed from its brief by route.v6 in place of inherited from the conversation that started it. |
| progress-words (`1673ed9a`) | One progress line on every surface (paused, step N of M, $ of the limit, time left), the CLI's step and pause events, Discord's paused and stopped forum tags. |
| task-history (`cd4df28e`) | `task.list` takes `ended`, `since`, words, `id` and `limit`; `theseus tasks --ended` and search, `task show`, `task pause` and `task resume`; a digest that names each reported task. |
| acceptance-journey (`fd88eb5b`) | One cross-surface journey on a real daemon: start, ask, answer elsewhere, join, pause, end, digest, find; `theseus-sim`'s `discord react`; the live script. |

**Join fixes:** the cockpit's progress line in the Rust line's exact words, its test running the Rust golden file
(`090e7465`); a line sent on to main puts the ended task's pane back to its last label (`20f42b9a`); only a private
place's reply waits for a late offer (`c235eb7f`, `2df6ac05`, `5daed979`); discord-tasks' `task.list` call takes
task-history's new params at their defaults (`13a1401e`); the journey answers with `--no-wait` and its steps for the
joined rows run, two stay ignored (`80d7b5ee`); the cockpit's extension proposal says `theseus answer ID approve`
(`da6f06d3`).

**How it was proven.** The merge messages name the conflicts each took and the order the rows joined in (progress-words
after the first three). The journey (`tests/acceptance_journey.rs`) counts the rings, a plain turn's frames and each
change's delay to a watching client; the progress line has one Rust golden, which the cockpit's test reads.

**Left open.** Two of the journey's steps assert contracts of rows that had not joined; Item 253's lane ran them.

### Item 252. RSI loop 1's rows: the flow runner, the pattern miner, the keel guard's watch, self builds in L1 and the sealed scorer's runner (theseus-pw1q.21.1 to .5; five cloud rows, joined 2026-10-18 02:31 to 04:58; the first three installed as #38, `c323da54`, the last two as #39, `70937771`)

**Why.** Self-improvement's first loop (R1 to R6) is the cheapest minted tool, a flow, and the machinery around it: a
miner that finds the recurring work, a keel that watches the sealed paths, builds that cannot reach out of their tree,
and a scorer that runs a candidate as Theseus itself. All of it is off in the owner's config: no `[self]` is in
`/etc/theseus`, and RSI stays off.

**What landed.**

| Row (merge) | What it built |
|---|---|
| flow-runner, R6 (`92c16e5c`) | Flow spec v1 frozen by its digest; each loaded flow is its own deferred tool `flow:<name>` whose steps run through the tool path; the owner's revert of a flow. No flow is loaded on any daemon, since nothing loads one yet (no wire method or CLI verb). |
| pattern-miner, R5 (`e09a0bbd`) | The nightly pattern miner, with no model call, files pattern causes in the self backlog; `theseus self patterns` shows them; `[self.mint]` with `mine = false`. |
| keel-watch-sealed, R2 (`746cd908`) | The keel guard watches the sealed scorer's code, its runner and relay and the exam set; a new `sealed-reach` rule flags code outside the keel files that names the sealed dir. |
| l1-self-build, R1 (`e36570c8`) | A self branch's gate, planted reverts, merge gate, auto-revert gate and install build run in L1, the tree and its build tree the only writable places, no network; the install's L0 half only copies and restarts. Join fix: the install copies the four installed binaries, not `theseus-sim` (`e7efe2fd`). |
| sealed-runner, R3 (`6e3bee86`) | The sealed scorer's production Runner (a scratch daemon of the candidate in L1) and the metered model relay; `[self] sealed_dir` is `[self.sealed] dir`. Join fix: the sealed runner's item view binds nothing read-write (`499fbb6a`, with R1). |

**How it was proven.** Each merge names its conflicts and its order: R5, then R2 (so R5's `[self.mint]` field line lands
before `config/self_improve.rs` becomes a keel file), then R1, then R3. The keel guard's findings on R2, R1 and R3 were
the rows' own and each carries a `Keel:` ack under the owner's one yes for the four keel changes. Join fixes: a leading
`NAME=value` word never reaches a mined shape (`5b7029e8`).

**Left open.** The live run of a self branch in L1 needs a non-root daemon with a job cgroup; a root machine's tests run
the item through a stand-in launcher (the core's guide says so).

### Item 253. The morning lane of 2026-10-18: a routine-coding brief on Sonnet, the cost forecast on the progress line, a Reminders-homed task's forum post, the ping window, `self.patterns`, the Keel ack and herdr's startup hook (theseus-31pgz, theseus-h9tu1; a local lane, `22c1540f..70937771`, fast-forwarded 09:42 to 10:21, no merge; installed as #39, `70937771`)

**Why.** Small defects the owner met on the morning's live run.

**What landed.**

- A routine-coding brief starts its task on Sonnet, and only a trivial one on Haiku (`22c1540f`).
- The progress line reads the spend against the forecast, `$0.40 of ~$0.60 (limit $2)`, beside the time left (`b63a3ae8`).
- A task homed in Reminders still gets its forum post and its board row (`1f75ce00`).
- The ping window never holds a question that waits on the owner (`030f0984`).
- `self.patterns` mines on the blocking pool, not the request's worker (`2cf1f23b`); its `[self.mint]` field line is acked
  under the keel guard (`2f74d06b`).
- herdr's startup hook closes the focused restored shell too, and the journey's two ignored steps run (`b512e40d`,
  `b1e0b3d2`, `70937771`).

**How it was proven.** The lane's commits carry no separate review; each names the test it added (the chat step's
assertions in `tests/acceptance_journey.rs`, the Keel ack for the one config field line). The journey's two ignored
steps run again.

### Item 254. A reach for the owner's personal details is held in a shared place (theseus-i1z13, finishing theseus-gasov; lane/shared-place, 2026-10-21)

**Why.** Item 252's neighbour, theseus-gasov, built the owner's warning (route.v7's `personal` question, the DM, the
`privacy.warned` row) and left the rest of the owner's spec of 2026-10-18 20:46: "outside [the owner-only places], it DMs me
about the case at hand, and returns [a refusal] to the requester". The owner chose the line: "I don't share that here."

**What landed.**

- In a shared place, a turn whose own verdict reads `elicit` or `expose` at the act band, or at the confirm band (the
  bands `PersonalAnswer::warns` already used there), makes **no model call**: the loop's first compile returns, the turn
  ends with `hold_reply`, and `I don't share that here.` is its assistant node and its output, so the reply post and the
  next turn's history both carry it. The stop reason is `held` (theseus-2rvwv): the footers say the reply was held
  and name no model.
- The owner's DM carries the case: who asked (the message's author), where (the place's bound name), the message's first
  200 characters quoted and scrubbed, which of the two reaches it read as, and that the requester was told the line.
- The hold is not rate-limited as the DM is: a second reach in the same place within ten minutes is held and answered, and
  only its DM is folded. With no DM with the owner bound, the requester is answered all the same and the row says
  `posted: false`.
- `privacy.warned` gains `held`. A place only the owner writes in, a task's turn and a verdict that came too late to wait
  for warn or do nothing as before and hold nothing.

**How it was proven.** `tests_personal_hold.rs` (six tests, red first on ARM: a model was called in a shared place and the
DM named no one): the fixed line and no model call, the DM's who, where and what, the confirm band, no DM bound, a second
reach in the window, and an ordinary message and the turn after a hold running; and `holds()`'s own test. Planted
reverts, six, each caught on ARM: the hold off; the loop ignoring the hold; the DM without the asker; the hold only at the
act band; the reply left out of the turn's output; a task's turn held too.

**Left open.** A late verdict never holds: the turn did not wait for it, so a reach in a shared place can still be answered
by the model when Jev is slow. The place rule keeps private recall out of shared places either way.

### Item 255. Voice layers 3 and 4: the switches are proven where the call is built, and the layers are written down (theseus-sa4m5; lane/shared-place, 2026-10-21)

**Why.** The echo canceller (theseus-yaxcp, voice layer 3, joined 2026-10-19) and Jev's judge of unclear transcripts
(theseus-s3dwj, layer 4) each came with a `[voice]` switch the binding copies into the engine's config. The reviews planted
a revert on each copy, and no test noticed: `Voice::default()` with `echo_cancel` false (a config with no `[voice]` section
gets it, and the parse test reads the serde default instead), the binding's three lines (`join_guard`, `echo_cancel`,
`judge_ms`), and `judged::attach` ignoring `judge_unclear = false`. A call with a dead switch plays as one without it, so the
feature could have been off in production with every test green.

**What landed.**

- `engine_config` (the lane `voice-config-wire`, joined 2026-10-20 as `33142ba7`): the three lines moved into one
  function the tests call, with tests that each switch reaches the engine alone and that a config with no `[voice]` section
  starts a call with the join guard and the echo cancel on and the judge at 250 ms. Plants: five, all caught.
- `Engine::has_judge()` in theseus-voice (the seam: the engine had no way to say whether a judge was attached), and
  `runtime/voice/tests_judged.rs`: by default and with `judge_unclear = true` the call's engine has the core's judge, and
  with `false` it has none.
- The docs the two layers left: the voice design (`docs/design/m7-surface.md` §2.8) names layer 3 (the echo
  canceller, `[voice] echo_cancel`, default on) and layer 4 (the judge, `[voice] judge_unclear`, `judge_ms`), and
  `docs/status.md` says what each does and that its switch is tested.

**How it was proven.** The seam's tests passed on the code that was already right, so the proof is two planted reverts on
`attach`, each caught on ARM: the switch ignored (the judge always attached), and the judge never attached.

**Left open.** The layers' live checks stay the owner's (laptop speakers and a long reply for layer 3; a short "yeah" over
Theseus's speech for layer 4).

### Item 256. Deepgram Flux listens to a call, by default: its model ends a turn, nova-3 is the fallback by config (theseus-v774k; v1 patch2's voice lane, the cloud rows p2j-flux and p2j-flux-fix of 2026-10-24, joined into integ/release for install #78)

**Why.** The pause rules end a speaker's turn after `[voice] pause_ms` of silence (1200 ms) and wait `merge_ms` for an
unfinished sentence, so every reply starts at least that late, and a thinking pause mid-sentence still cuts a turn in two.
Deepgram Flux is a streaming listener whose own model decides where a turn ends. The owner asked for it on 2026-10-24
(design and checklist in the issue's notes), then, at 11:55 the same day, for Flux as the default listener in 1.0.2,
tested as such.

**What landed.**

- **The seam and the client** (theseus-voice). `src/listen.rs`'s `Listen` (raw frames in, `Listened` events out:
  `Start`, `Eager`, `Resumed`, `End`, `Down`, `Up`, `Unavailable`), with `StandInListen` for the tests. `src/flux.rs`'s
  `FluxListen`: one v2 socket per listed speaker, opened at their first audio; the key only in the handshake's
  `Authorization` header; 80 ms chunks; a keep-alive; an 8 s outbound queue that drops the oldest audio; a reconnect
  backoff of 1 s doubling to 30 s; `CloseStream` at the call's end.
- **The engine's streaming path** (`src/engine/flux_path.rs`, `Engine::with_listen`). An utterance opens at `Start` and
  closes at `End` with Flux's words; no VAD, no `pause_ms`, no `merge_ms`; from the `Pending` on the engine is the same.
  A `Start` over Theseus's speech holds the clip when the residual after echo removal shows speech in 3 of the last 10
  ticks and the speaker is not echo-prone, wordless or at the join; the `End`'s words then decide through `heard()` and
  Jev's judge, as before. A turn the listener never ends is given up at `TURN_MAX`. A dropped stream hands its turn in
  progress to the pre-recorded recogniser once; a listener that cannot be reached is off for the call (nova-3 and the
  pause rules), said once.
- **The fix round** (the first review rejected for a fix round: F1 High, F2 Medium, L1-L4). Discord sends no frames for a
  quiet speaker, so Flux heard no silence and an end of turn could come in 5 s steps or only at the backstop: the engine
  now feeds zero frames while a turn is open and for `eot_timeout_ms` (Flux's 5 s) after a speaker's last real frame.
  The barge-in gate counts a window of 10 ticks, not the whole turn. The queue of a dead socket is cleared at the
  reconnect; `first_words` drops a reply with no audio; streamed audio no `End` carried is booked (`Event::Streamed`, a
  `speech.transcribed` row with `detail.trailing`).
- **Config** (core `voice.rs`, `voice/flux.rs`). `[voice] stt_model` takes a Flux id (`is_flux`), with `eot_threshold`,
  `eager_eot_threshold` and `eot_timeout_ms` in Deepgram's ranges, refused with any other model. **The join's fix makes
  `flux-general-en` the default** (the owner, 11:55); `stt_model = "nova-3"` is the fallback by config, with the pause
  rules and any language. Flux English with a language that is not English is refused at load, naming both fixes, so a
  `[voice] language = "de"` with no `stt_model` now refuses where it used to parse (right over compatible; the owner's
  config sets no language). Voice notes and file transcripts always use pre-recorded nova-3 (`prerecorded_model`), with
  `smart_format`, which Flux does not have.
- **Spend.** Flux is priced by the minute streamed (`deepgram/flux-general`, $0.0077 a minute, against nova-3's $0.0043
  on utterances alone); a socket opens and a turn starts only while the session's spend has room. The silence fill costs
  at most `eot_timeout_ms` a spell, about $0.08 to $0.19 for a busy hour.
- **Rows.** `voice.joined` names the `listener`; `voice.barge_in` gains `stop_latency_ms` and `ended_latency_ms`; the
  journal line `first word after the end of turn` carries `ms` and `path` (`flux` or `pause rules`) on both paths.

**How it was proven.** The engine's tests run in virtual time against `StandInListen` (`tests/flux.rs`), the client
against a local socket server (`tests/flux_socket.rs`), each fix red first as a planted revert; the join adds that a
config with no `[voice]` table listens through Flux and that `stt_model = "nova-3"` still takes the pause rules. The
DM-flow golden and the echo canceller's golden are byte-identical to main's; store format 50 is unchanged.

**Left open.** The live A/B with Flux as the default (first-word latency by `path`, `voice.resumed` against
`voice.barge_in` on speakers, a `trailing` row at `/leave`, a voice note still read by nova-3) is the owner's retest.
Flux's keep-alive and idle timeout were read from Deepgram's docs only. Falling back to nova-3 when `FluxListen::new`
fails (unreachable today beyond the checks the REST client makes first), a Discord-side test that `Event::Streamed`
books a trailing row, and the `files.rs` race under plain `cargo test` are follow-ups.

### Item 257. Live learning: the exchange-end observer, the card's live pass (Add, Pattern, Stale) and live questions in the owner's DM (theseus-6f8t0; v1 patch2's live-learning lane, the cloud rows p2k-types, p2k-obs, p2k-live and p2k-dm of 2026-10-24, joined into integ/p2-learn for install #79)

**Why.** The owner card learned only from the imported history and from whole-message directives. A fact said in
passing inside a work request ("fix the build. I'm on the M1 now, by the way") judged as `none`, so it was lost;
nothing saw a preference repeated across conversations, or a record contradicted later. Recall could find such a
remark by similarity, but nothing put it on the curated, owner-approved, always-in-context card. The design check
(options A to D) recommended A + C for 1.0.2; the owner's GO (2026-10-24 10:42) took A + C, the DM cards (O1), the
live pass after `card done` (O2) and the thresholds (O3), and left `owner.ask` (B) and the learning bench (D) to 1.0.3.

**What landed.**

- **The types** (p2k-types; no behaviour). `CardObservation` (kind fact | preference | decision | correction, by
  construction no trait kind; text at most 160 characters; its nodes; `contradicts`; state open | used | dropped),
  `card.observed` (RPC, CLI `theseus card observed`, ledger kind, spend kind), `CardProposal`'s `ask` (add | pattern |
  stale, serde default add, so an old proposal reads and writes back byte for byte), `record`, `observations` and
  `posted_ms`, the `end` answer, and `[card] observe` (true), `live_per_night` (5), `stale_per_night` (2),
  `pattern_sessions` (3) and `dm_per_day` (3). **Store format 51**, the lane's only bump, with a literal format-50
  proposal sample.
- **The observer** (p2k-obs; option A, a copy of people.v1's live pattern). At an exchange's due point in a Private
  session the owner is alone in, never a task, one forced-tool call on `[card] profile` reads the owner's own lines
  since its mark (the nodes `turn.submit` noted as the owner's: not a job's, the MCP server's, a prompt's or one with
  files) beside the live fact and preference records, through the session's Google and privacy marks and so the data
  gate, scrubbed and reserved. Code keeps an item only when it cites a line, is lasting, is not one task's or a paste
  and restates no record (a restatement counts as said again); the observations, the mark and the row are one frame.
  The nightly sweep reads what never came due.
- **The live pass** (p2k-live; option C). Nightly at the learning hour, for as long as `[card] observe` is on and
  whether or not the card is done, and after the archive's run in `theseus card propose`: the sweep, then Stale (a
  contradicted live record: Keep, Edit in one `standing.add` with `replaces`, End it through `standing.revoke`), Add (a
  fact or a correction alone, at support 1, with no call; or clustered preferences and decisions) and Pattern (at
  least `pattern_sessions` distinct conversations), capped per night and by `max_open`. Not this and expiry set the
  observations aside for good.
- **The DM cards** (p2k-dm; O1). A poster posts open live questions to the owner's DM as `card_question` cards
  through the pacer, from the morning hour, at most `dm_per_day` a local day, the posts and the `posted_ms` marks in
  one frame; Keep, Edit (a modal holding the words), Not this or End it answer as the presser. The DM task flow's
  messages are untouched.
- **The join's fixes.** The live pass's `// join:` stub calls `observe::sweep`; a named contradiction wins over a
  named restatement; a night's line counts from its learning hour, not a rolling day (a full night left the next none
  by milliseconds); a DM press is acknowledged before `card.answer`, which can wait for a running turn past Discord's
  three seconds; the poster re-checks the pacer after the writer handshake; theseusd's versions test reads format 51.

**How it was proven.** Every row red first or by planted reverts (p2k-dm's by plants), each review's join fix with a
test that fails without it, and the lane's plants on the merged tree. No observation text reaches a compiled request,
the census or the index (the observer's test 7: a census and a DM request byte-identical across an observation); every
golden, the DM-flow golden among them, is byte-identical to main's. A China-origin `[card] profile` is refused at the
chokepoint. The lane's one ARM gate and the x86 finish passed before install #79.

**Left open.** The live check on a scratch daemon with real Haiku (three facts said in passing, a contradiction, a
shared place: the observed list, Add, Pattern and Stale, End it, under $0.05) runs with the owner after install. One
lock for card proposal writes (theseus-kxb8d), a no-expiry read for the Edit modal's words (theseus-5pypl), and whether
three morning questions should buzz once each or one per pacer window are follow-ups; `owner.ask` (theseus-mqret) and
the learning bench (theseus-92q5b) are 1.0.3.

### Item 258. "Not this" on a task: `task.not_this` cancels it, keeps one rolling preference against work like it and labels the offer; one tap each on the CLI, the cockpit and Discord (theseus-o1h3f.2; the feedback leg of notify-work-feedback; the cloud row p3t-not-this of 2026-10-27, cc8bb400, 283cf70b, b6c7257b and 24a1f573, joined at 79a5f2d7; applied to the spec by theseus-cqngz)

**Why.** The owner's model for tasks is notify, work, feedback. A task that started itself (theseus-o1h3f.1: a turn with no owner
input, which must say why it started it, `because`) had no cheap way to be told "I did not want that": the owner could stop it, and
the next such turn would start another like it. This is the feedback leg: one tap that stops the task and leaves a sentence
for the model to read next time. It is a **preference, not a limit**: the owner's standing decision is that there is no fixed helper
cap, only slots and spend, and nothing here counts, caps or refuses a start.

**What landed.**

- `task.not_this { task, reason?, author?, discord? }` (`rpc/task_not_this.rs`, `theseus_protocol::task_not_this`; the wire types are
  one definition, their TypeScript generated; routed with `task.cancel` by `task_stop`). It is the owner's act, judged first as
  `Act::Standing`, which takes `judge_act`'s default arm: the owner, from a private place (the arm `task.rehome` takes as
  `Act::Task`). A refusal leaves the task running and nothing written; the CLI refuses it inside a job. Then three things: **it
  cancels the task with `task.cancel`'s own `task_cancel_by`**, so it releases exactly what Stop releases (a task that had ended is
  answered as it was, not refused; an unknown task is not found); **it writes the one rolling preference**, a `Preference` for
  everywhere whose source is `task.not_this` and which `replaces` the last live one, so a second "not this" ends the first and there
  is one, never a list (the same words again leave the one record); and **it labels the offer**, best effort (a task carries no link
  to the offer that produced it): the newest acting `offer_task` judgment of the task's origin session in the last 2,000 ledger
  rows is labelled false, and the answer's `labeled` names it, or is empty. The preference's text is one line of prose, the task's
  shape (its `because`, else its title) and the owner's reason, which the standing block carries in the header layer like any
  other record for everywhere; the model reads it. No store format change (a standing record and a label that already exist).
- Three surfaces, one tap each: `theseus task not-this ID [REASON]` (in `client::OPERATORS`, printing what it did in words);
  the cockpit's Ops page, `u` pressed twice on a task; and, in Discord, a 👎 reaction on a task that started itself
  (`runtime/task_react.rs`), honoured only on such a task (on any other it is the reply rating's, as before), only from the owner in a
  private place, answered with ✅ or one quiet line. The CLI and the cockpit take any task.

**How it was proven.** The core's `rpc/tests_task_not_this.rs` (the cancel and the preference with the reason; two calls leave one,
the second's, with `replaced` naming the first; an ended task answered and the same words leaving one record; a stranger's call
cancels nothing and writes nothing; an unknown task writes nothing; the text is one line), the CLI's
`not_this_names_the_task_the_preference_and_what_it_replaced`, the cockpit's verb-table row, and the Discord crate's two reaction
tests (a self-started task is cancelled and one preference is kept; a task the owner asked for is left running and no preference is
written). The code half was gated green and reviewed with no blocking findings. No live check of the reaction is recorded here.

**Left open.** The reaction is never pre-added to a task's message, so the owner finds it by hand; offering it (and offering the
CLI's and the cockpit's verbs on the card) is not built. A task carries no link to its offer, so the label is the newest
acting `offer_task` judgment of the origin session, not the one that made it. The reaction sends no `reason`. A guest's or a
shared place's 👎 on a self-started task is taken (a debug line) and never reaches the reply rating, as every other control's does.
The Discord guide's entry for `task_react.rs`'s other controls is theseus-cqngz's.
