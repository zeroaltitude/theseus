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

### Item 259. The principles migration: four labels, two deterministic sources, one switch, and a record that is raw (theseus-gyjyo, rows R0 to R10; the owner's decisions of 2026-10-27 22:02 to 22:47; v1 patch3's `integ/principles` lane; R0, R1 and R2 joined `main` 2026-10-28 and installed as #98, `3ef994dd`; R8, R3, R7, R5a, R4, R9 and R6 joined the lane, which merges once, after a gate of the merged tree)

**Why.** The principles review of 2026-10-27 (measured on `209f9c52`, with every number counted on the tree) put the owner's
principles against the code and found it disagreed with all three. The owner's principles are four: **P1**, Theseus
runs on a machine its owner set up and trusts, so it categorizes, measures and notifies, and blocks only what cannot be
undone (money and the repo); **P2**, the session is the true record, so nothing in the store is rewritten; **P3**, a
model of restricted origin is operator control: a label the operator can see and remove, not a proof the operator
cannot discharge; **P4**, less code. The code did otherwise. The stored record was redacted at seven sites by a scrubber
whose five shape finders guessed what a secret looked like and caused every false positive in the thirteen
Terminal-Bench losses. The chokepoint for restricted models read the whole transcript on every request to prove every
node had been judged, and that proof's mark (`judged_through` and the span, the CAS merge, the worklist, the sweep, the
landing's park) was where every defect of the week before lived. And the sensitive-names tripwire and the Google
"private" marker were two rules the operator could not remove. One fact nobody had seen: on the owner's daemon no
model of China origin was routed at all (`[routing.classes]` names no `cn_profiles` and no `cn_draft`), so the whole
apparatus had only ever acted on an explicit `-P glm` pick and on benchmark cells.

The owner's answers to the review's five questions (2026-10-27 22:45 to 22:47) took each recommendation: **the record
is raw** and the vault's values are substituted by name only on copies that leave the machine; **an item Jev could not
judge** proceeds, with a notice; **sensitive names** become a removable label source; **the sandbox stays**;
**earned autonomy and scoped grants go**. The injection freeze (his own design of 10-26), the drafts flow (a China-origin
model drafts and a Claude model reviews), the money backstops, the keel and the push guard stay. The migration is eleven
rows, each a builder (red tests first) and a fresh review, joined as plain merges; the 6,900 production and 9,200 test
lines the review predicted gone were the plan, and the rows' own counts are below.

**The design as built.** Read this as the single statement of what Theseus now does about trust, privacy and content;
the rows below say how each part arrived. It supersedes every earlier Item on these mechanisms (the table at the end
names them).

*Four labels.* A session carries at most one label per flag: `injection`, `secret`, `confidential`, `personal`
(`content.rs`, which is pure: no store, no judge). A `Label` names its flag, the node that raised it, the item's kind,
a source the operator is shown (a tool and a resource, or a place), a confidence and band, the time, and the id of the
judgment that raised it (empty when no model did). Labels live on `SessionRecord.data`, survive compaction and restart,
and only the operator removes one (`content.unlabel`, `theseus content remove SESSION FLAG`, the one Discord message's
Remove button, the cockpit's chip); a removal is kept beside them as `removed` history (who, through which surface,
when). The three content flags raise at the confirm band, and `injection` raises only at the act band: a wrong content
label costs a session its cheap model, the safe direction, while a wrong injection label freezes a session and teaches
the operator to ignore the control. `secret`, `confidential` and `personal` keep a session off a model of restricted
origin; `injection` does not. **A Remove sticks** (`ContentFlags::raise_after`): an item that entered the session before
the operator's removal of a flag can never raise that flag again, whichever source asks, because the removal spoke for
everything the session held when it was made; only an item that enters after it raises the flag anew. It is read from
`removed`, which is on the record and which a fork copies, so a restart, a fork and a re-offered item all find it. A task
(`task.create`, `task.start`, a retry, a routine's run, a draft and its first pass) starts with the labels of its parent
that disqualify restricted models (`ContentFlags::inherited`: the removals and the judgment ids stay behind, and so does
`injection`, which says the parent's own transcript tried to steer it and which a task started from a brief has not
read). A fork copies the whole record, `injection` included.

*Three label sources, two of them free.* **Jev's content pack** (`content.v3`; v1 and v2 stay embedded for their stored
judgments): every item that enters a session (the owner's message, another person's message in a shared place, any tool
result, a fetched page, a search, a task's brief, a task's report) is asked the four questions once, off the turn's
path, by `JudgeService::ask_content`; the injection question is never asked of the owner's own message or a task brief.
A `yes` at the flag's bar raises the label. **Names**: an item holding one of `[privacy] sensitive_names` raises
`confidential` with the source `name: <word>`, deterministically, at its entry, with no model call, for every item kind,
the owner's own message included. **Google**: a result of a Google tool raises `confidential` (a sensitive calendar's
result raises `personal`) at the write site, `toolrun/complete.rs`, with the source `google.<tool>`
(`google.calendar.events`, `google.call`); `google/private.rs` is 62 lines (it was 517). The two deterministic sources
have one home, `content::deterministic` (`label`, `from_names`), land through `JudgeService::land_deterministic`, and are
off when `session_flags` is off. A deterministic label has confidence 1, the act band and no judgment id, so removing it
teaches the classifier nothing. Each label is told to the operator once, as the one Discord message with its Remove
button (the locked DM flow, byte for byte), and written as a `content.labelled` row.

*One switch.* `[content_based_security] cn_models = always | follow_flags | never`. The chokepoint
(`data_gate.rs::guard`, wrapped round every provider once, so the turn, compaction, keep-warm, replay and every background
call share it) refuses a request to a model whose origin is restricted (`cn` or `unknown`) when `follow_flags` finds a
disqualifying label on the session it is made for, `never` refuses it always, and `always` sends it. **That is one read
of the session's record and nothing else**: no transcript read (a test reads the store's own counter and asserts it did
not move), no proof that anything was judged, no mark. What it still refuses is about the caller, not the content: a
request made for no session, and a session whose record does not read. The route (`turn/data_step.rs`) reads the same
labels: a profile `never` refuses is skipped, and a turn whose labels allow a restricted profile *and* whose decision
takes one waits at most `wait_ms` for the content asks still outstanding for its session (`content_wait.rs`, a counter;
a lost wake-up costs one 250 ms slice, not the wait), reads the labels again, and **goes either way on what they say
then**; a session already carrying a label never waits, a Claude turn never waits, `0` never waits. A content ask
that fails is asked once more; if it fails again the item is rowed `content.unjudged`, the session's place is told
once, and the session goes on unlabelled for it. The route row's `cn`
word is `allowed`, a flag's word, `never` or `no_record` (it was `pending` and `unjudged`). `session_flags = false` with
`cn_models = "follow_flags"` refuses to start, because with nothing judged there are no labels to follow.

*The injection freeze.* With an `injection` label on a session, every call except a local read of files and code
(`fs.`, `git.`, `text.`, `task.read`, `tools.find`: an allowlist) is refused in words the model can act on, until the
operator removes the label (`policy/modes.rs::content_refusal`; `[content_based_security] injection = refuse | notify`,
and the bench daemon's is `notify`). This is the only place Theseus still blocks on a classifier, at the act bar, by the
owner's choice. **`/trust` removes the `injection` label and nothing else.** What replaced the external-text hold is the
`external` **marker**, which stays on a node (a page, a search, an MCP server marked external, a job that reached a host
beyond `[sandbox] egress`, a Google result, and, from a built-in list, a `gh` program's output) and is read by the
content pack's source, Jev's security state, the index and recall's filter, consolidation (a cluster with an external
source is never synthesized) and egress's "outside text". The marker holds nothing and waits for no one. Quotes a task
was started with, when the owner marked them outside text or an agent started the task, are queued as one
`PersonMessage` item, which asks the injection question, so a mail watch quoting an email, Discord's "make this a task" on
another person's message and the `/task` modal's "read as outside text" are classified like any other outside text.

*The record is raw, and the exits substitute.* Nothing Theseus stores is rewritten: tool result nodes (and their
`diff` meta), the kept whole-output files, the job spool file, imported episodes, the ledger's narrative lines and the
watch rows, a task report's kept copy, a batch's earlier steps. The scrubber (`scrub.rs`) knows exact values only: what
the vault resolved and its encodings (verbatim, base64 at any offset in either alphabet and wrapped, percent-encoded,
JSON-escaped), found by one pass (`Substitution`) that both the record-facing callers and the exits share. The five shape
finders and the decoded-shape pass are gone. On the copies that leave for a model provider, for Discord and for Jev each value is replaced by
`[redacted:<name>]`: the provider request (`exits.rs::Substituting` wraps every provider, and substitutes the copy it
sends, never the compiler's request, so the digest and the prompt cache see the same bytes), every Discord post, edit
and live stream (`Lane::write`, a notice card's title, description and fields, "Show source", the follow mirror), the
Jev state and the push line. A request that carried a vault value writes one `request.redacted` row (the secret's name,
how many times, the model, the loop, whether it was substituted), shown by `theseus inspect [SESSION] [TURN]` and the
cockpit's turn inspector. `[privacy] redact_outbound = false` sends the values as written and the row still names them
with `substituted: false`. **A string that merely looks like a key is a guess and goes through untouched, everywhere.**
The scrubbers that remain on the owner's own screens and prompts (peek, the digest, the ledger window, the work board,
push lines, the end note, an approval question's preview) are exact-value only too. The push guard stands: a tree whose
file or commit message holds a vault value is refused. For a model of restricted origin the `secret` label is the guard
for a key the vault never saw.

*The classifier's loop.* A replay serves a stored content judgment from its blob (R0), a Remove writes a `judge.label`
row in the same frame (R2), a label left standing a week is a system label (R2), and `content.v3` is measured on a frozen
set and on the owner's own history (R9), so the nightly `judge learn` reads the owner's taps against the stored states.

*What is gone.* `policy/earned/*`, `policy/grants/*` but `covers` (the edits mode's rule), the weekly card,
`policy.earned`, `policy.grants`, `policy.revoke`, `theseus policy earned|revoke|grants`; the external-text hold
(`external.rs` 1,198 lines to 242, 140 of them production -- 104 at R6's first build, before its fix put back
`gh`'s marking -- `rpc/trust.rs`, `policy.trust`, `policy.notice_seen`, `session.trusted`,
`confirm{trust}`, the "Approve + trust session" button, `theseus policy trust|dismiss`, the cockpit's hold tiles and
Trust again, `[policy] external_text`, `[policy] external_programs`); the Google marker's records, scopes, lineage and
recall follow, and `data_mark.rs`; the chokepoint's transcript read, `unjudged`, the names walk, `in_scope` and the US
allowlist's use there; `scrub/shapes.rs`, `scrub/decoded.rs`, their corpus and `planted_secrets.rs`; `redact.rs`'s
withholding (the cap and the ring stay). What stays on purpose: `ModelOrigin::cleared_for_sensitive`, one reader,
`rsi::mint::data_rule` (whether that rule stays is the owner's); `pack_names.rs` (the embedded packs' text is checked
against the names list at start, a public-repo guard); `routing_cn.rs` and `data_floor.rs` (load-time checks in words:
cn profiles only in `trivial`, `cn_draft` only in coding classes, background calls over the owner's whole store never
name a cn profile); the sandbox crate; places and ceilings; approvals; the keel, the money lines and the push guard.

**The config surface, and what the loader says.**

```toml
[content_based_security]        # replaces [content]; every key defaults, so an owner's config names only what differs
session_flags = true            # Jev judges every item; off: nothing is classified and no label is raised
cn_models = "follow_flags"      # always | follow_flags | never
injection = "refuse"            # refuse | notify  (the design's word is "freeze"; the spelling in a config is refuse)
wait_ms = 4000                  # how long a turn bound for a restricted model waits for outstanding asks; 0 never waits
max_pieces = 20                 # a long item is split at 6,000 characters a piece; this bounds the pieces read
context = ""                    # one paragraph on what the assistant is and which of its work is public, to the pack as written (cut at 800 characters)

[privacy]
sensitive_names = []            # a hit raises `confidential` on the session: a label, removable, no model call
open_source_paths = []          # absolute or ~/ paths; the pack is told an item from there is public. NOT [places] public_paths
redact_outbound = true          # the vault's values are replaced on copies that leave the machine
```

The old keys are hard errors that say where each went: ``[content] enabled`` was renamed to
``[content_based_security] session_flags`` (and `injection`, `wait_ms`, `max_pieces` to themselves);
``[policy] external_text`` and ``[policy] external_programs`` "was retired: the `injection` label replaced it"
(`config/renamed.rs`, `RETIRED`); a `[privacy] open_source_paths` entry that is not absolute or `~/` is refused, as
`[places] public_paths` refuses one; and `session_flags = false` with `cn_models = "follow_flags"` is refused with the
two ways out. A bare `[content]` table with no key gets the generic unknown-field error. A `[policy.earned]` section
loads, is ignored and is warned of. The owner's own config had no `[content]` table, so it loads on the defaults; **the
line `external_text = "notify"` under `[policy]` must go before the R6 build installs**, or the daemon refuses to start.
`open_source_paths` and `context` are the owner's to write: until he does, `public` and `context` are never sent to the
pack, and `content.v3` runs on its wording alone.

**What the owner sees.** The Discord DM is the one label message with its Remove button, unchanged; a names label reads
like any other, and says its source ("your message from `name: <word>`"). No "held: a call that acts waits" post, no
`/trust` prompt, no weekly card. `/trust` says what it removed, or that the conversation carries no `injection` label.
The cockpit's session chips are unchanged and gain "wrong", which writes the learning label. The Ops view shows
`request.redacted` beside a provider call that carried a vault value, and the turn inspector says which secret, to which
model. `theseus content SESSION` lists labels, and for each says how to call it wrong; `theseus inspect` is new and
hidden, like `open` and `reach`.

**Store format and ledger.** One bump from R2: `MANIFEST_FORMAT` 52 to 53 (`Label.judgment`, optional, so an old record
reads with it empty and writes back byte for byte), with a generated layout sample. R5b's bump to 54 removes
`judged_through` and `unjudged` from `ContentFlags`; an old record reads with them ignored and the first write drops them.
Neither moved a golden of the two the standing rule names. R6 removed `SessionRecord.external` and
`GateDecision.external` without a bump (neither record denies unknown fields, and the layout machinery reads a removal)
and moved the session index's projection `projection.core.4` to `.5`. R7 and R8 changed no stored record: `redactions` in
a node's free-form meta stopped being written, `SessionGrant` was never stored. New ledger kind `request.redacted`; the
kinds `session.external_read`, `session.trusted`, `policy.granted`, `policy.grant_ended` and the four
`policy.earned_*` are no longer written, and old rows still read (kinds are strings).

**Each row, and where it diverged from the plan.**

| row | what shipped | where it differed from the review, and why the code won |
|---|---|---|
| **R0** replay serves content (`lane/pr-r0-replay`, joined `main` `e3b6af10`; +458/-32, 7 files) | `Dynamic::recorded` writes the pack's dynamic sources beside the state blob; `replay_state` serves a stored content judgment, or rebuilds one from its blob (`rebuild::content_input`); `judge backfill` refuses a content pack. **28 of 28** of the owner's content judgments replay, for $0.0025 | The review's second shape (carry `untrusted` as a state scalar) was not taken: it edits `content.v2`'s questions, a new pack version, and changes every stored sha256. `--pack-file <the embedded v2>` cannot be run (a replay refuses the wired version and the incumbent): the workflow is copy, bump, edit; a candidate that only bumps the version is rebanded and exercises nothing. The store held 28 content judgments, not 18. Join fix J1, found by review: a *learned* candidate (version 101 and up) was asked the stored state without the recorded sources, so it was never asked `injection` of a page, a tool result or a person's message while the live pack asks all three, and the replay reported nothing left out; a content record keeps its items, so it now reads them |
| **R1** `[content_based_security]` (`lane/pr-r1-config`, joined `main` `45848bea`; 22 files, +476/-192) | The table, `CnModels`, the start check, the four renamed-key rows; `[privacy]` gains `open_source_paths` and `redact_outbound` | `max_pieces` is kept (the listing omits it). `injection` keeps `refuse`, not `freeze`: each word names what happens to a call, and nothing writes the other. The Rust field is renamed with the table. **`[privacy] public_paths` was renamed `open_source_paths` at the join review**: `[places] public_paths` already exists and opens trees to a shared place's file tools, so one table header out of place would have opened the owner's working trees, untracked files included, to every guest, with nothing to say so; the two lists stay separate, and the new one is validated like the old. Found and filed: the gauntlet harness's cn profile writes `[content]` (theseus-gwceh); `term::tests_idle` is red on any host whose python is 3.10 or older, a real defect of `term.read until_idle` (theseus-3ni3t) |
| **R2** labels teach (`lane/pr-r2-labels-teach`, joined `main` `3ef994dd`; 30 files, +556/-30; store 53) | `Label.judgment`; `content.unlabel` writes a `judge.label` row (source `operator`, the flag's question answered wrong, weight 1.0, key `lbl_<id>`) in the same frame as the removal; a `content` arm in `learning/system.rs` (`kept`: a label still standing 7 days after it was raised, or when its session retires, is a system `yes` at weight 0.5; a removed, disputed or anonymous label is skipped); the chip's "wrong"; `theseus judge label` takes a content judgment | Two review joins. A whole-judgment "wrong" (the cockpit and the CLI offer it on every judgment, with no question) now disputes the label's flags; before, the owner could call a judgment wrong, leave the label, and a week later the arm wrote a `yes` anyway. **The "session retires" leg departs from the review's §6.2**: it needs the label to have stood a day, because in Discord a session is superseded by `/new`, and a label raised two minutes before one would otherwise become a positive with no window to dispute it, in the direction (false positives) the pack's known problem runs. Replay's `right` took the first matching label, not the resolved one: filed and fixed on `main` (theseus-3h8pe). No scratch-daemon live check; the in-process rig drives the same path |
| **R3** names as a label source (`lane/pr-r3-names`, in the lane at `bcf1c7b4`; +559/-314, 16 files, then a fix) | `deterministic::label`/`from_names` and `land_deterministic`; `names_in`, the request walk, the route floor's `sensitive` leg and the names' use of the US allowlist deleted | **The first build did not let a Remove stick**: the names label raised at entry, and the pack's verdict on the same item landed later and answered `yes` too (`content.v2`'s `confidential` reads "or anything the state lists as private", which is the names list), so the label came back 19 ms after the tap. The review sent it back; the fix is `raise_after` and needs no format bump, and it covers every flag, not only `confidential`. `cleared_for_sensitive` stays: `rsi::mint::data_rule` reads it, and that rule is no part of this area. The chokepoint's `sensitive` arm was halved, not deleted, until R4: deleting it whole would have let a session that read a Google result reach a restricted model for one row. The label is not told in the review's §1.3 words. Accepted, and measured by test: a name in a context file or standing record, with no item in the session naming it, no longer stops a request; a model of EU origin is no longer refused for a name. Open: the label's source shows the word in a shared place's post |
| **R4** Google as a label source (`lane/pr-r4-google` and `lane/pr-r4-join`, in the lane at `5539d603`; 79 files, +1,026/-3,298) | The write-site label; the marker's records, task-locals, lineage, recall follow and `data_mark.rs` deleted; 49 tests deleted, each testing only the marker; the six label tests | **`marked` and `marked_sensitive` stay**: they are the channel from the unchanged Google tools to the write site. `data_mark.rs` is more than a mirror: its mark is a wire type (`DataMark`, `SessionInfo.data`, `ProfileInfo.cleared`), which survives, derived from labels (`personal` is `sensitive`; any other disqualifying label is `private`). The source is `google.calendar.events` and `google.call`, not `google.calendar`. **The floor on Google content was an allowlist (US only), so it refused EU as well as `cn` and `unknown`, and a label keeps off only the last two: Google content is now kept off a model of EU origin by nothing**; nothing changes on the owner's config today (Anthropic and zai, no EU model), and it takes effect the day one is added; that reverses a standing owner rule of 10-18 that the review did not surface (Beads theseus-xs5ah). The join fixed two sites the deleted lineage walk used to cover, a retried task and a routine's run, each starting without its parent's labels (the routine's parent is `spec.created_in`; the review's code read the routine's own session, which never carries a label), and decided against inheriting `injection`. Recall, the place digest and `memory.lookup` no longer carry a mark. No scratch-daemon check: no standalone Google fake exists |
| **R5a** the switch and the counter (`lane/pr-r5a-switch`, in the lane at `f04c3d2a`; 16 files, about +660/-340) | `cn_refusal` reads the record only; `never` refuses at the chokepoint too and skips the profile in the route; `always` skips the record read; `cn_wait` is read, wait, read; the draft gate skips under `never` | The "outstanding-ask counter" already existed (`Slot::open`); the row exposed it as `Verdicts::outstanding`. The sweep stayed in `cn_wait` for one row, because deleting it there would have lost the only re-ask of a mark a failed batch left stuck. `always` still refuses a request made for no session. Review join J1, a real defect reproduced at the head: `never` is not a flag, so a coding turn opened a draft task and ended `drafted` with no model call, and the draft's own first request was then refused |
| **R5b** the mark goes (`lane/pr-r5b-mark`; store 54; +518/-1,068 production) | `judged_through`, `judged_run`, the CAS merge, `unjudged` as a worklist, `ask_unsubmitted`, the sweep, the landing's park and its `awaiting` notify, `fork::data_through`'s mark half deleted; a failed ask is asked once more (200 ms later, inside the open count), then rowed `content.unjudged` and told once per run of failures in the session's place: "⚑ unchecked — … could not be checked … Nothing was labelled, so China-origin models are still allowed for this session"; the session goes on | The cn-wedge2 race tests are rewritten as counter tests, not deleted, and each says what it now tests. The retry is the same judge asking again, so an open circuit breaker refuses it at once: "once more" means one more decision point, not always one more request on the wire. `refused` and `unbuilt` are told but not retried. **The price, stated**: an item no write site offers is never asked about, in silence (`every_item_a_turn_writes_is_offered_for_judgment` is its only guard), and an item whose ask fails twice can reach a restricted model with no label, with a notice. Removing the park makes the content landing frame (the `content.*` rows and the `session` record) land during a turn, so the judge bench counts it as the judge's (`perf/judge.rs::is_judges`). `data` on a session with no labels is `None`; `inspect`'s data line loses its `judged to <node>` tail. A fork taken while a check is still out on its source never gets that check's label, silently: the sweep that used to cover it went with the mark (R5b's review F1, theseus-sz6z9, a follow-up). |
| **R6** the hold goes (`lane/pr-r6-hold`, `lane/pr-r6-join`, in the lane at `1058b2c4`; 295 files, +1,804/-7,839 before the fix) | See "What is gone"; `/trust` is `content.unlabel injection`; `egress::gate` becomes a notice (an L1 job naming a host beyond the list runs, and the notice names the host); the retired keys fail the load | The review sized the hold's files at 1,634 production lines down to about 60; **the marker is not the hold**, and it stays at every place the plan named, so `external.rs` is 104 lines, not 60. The hold had two jobs nobody listed and the first build took both away: an outside quote in a task became a `Body::Arrangement`, which is no content item, so nothing classified it; and `external_programs` defaulted to `["gh"]`, so retiring the key stopped marking `gh`'s output (a stranger's issue text could then reach long-term memory). Both fixed with red tests. `tests_outside_text.rs` is not a hold test (it tests the readers of outside text) and stays; two other files moved. **Egress is not simplified** (498 to 504 lines): only the wait became a notice. The approval card's locked golden changed beyond removals (its fixture was the outside-quote ask, which cannot exist; the layout is kept on the posture ask), and the DM golden's line 8 still reads "`/trust` trusts the conversation again after it reads web text"; both need the owner's OK. An AWS-credentialed L1 job that names a host beyond `[sandbox] egress` now runs with a notice, and the old guarantee in the core's guide ("containment is the broker's grants and the egress list") no longer holds for a call-named host: the owner's decision. The bench profiles and `bench/h2h/theseus.py` set the retired keys and were fixed; the gauntlet tree's copies are not |
| **R7** raw record, substitution at the exits (`lane/pr-r7a-raw-record`, `lane/pr-r7b-exits`, one fix branch, in the lane at `37af2377`; R7a -3,162/+704 over 51 files, R7b about +693 production, the fix +845/-120 over 33) | See "The record is raw"; `theseus inspect`; `request.redacted` | **R7a alone is unsafe** (the record raw and nothing substituting on the way out) and R7b alone is safe, so the halves join together or not at all. The first review found a blocker: R7b's exits substituted verbatim only, so `curl -v -u deploy:$TOKEN` printed `Authorization: Basic …`, the value reached the model and Discord encoded, with no `request.redacted` row, and "what key did we send?" would have answered none; fixed by one shared pass (`Substitution`) and the `contains` pre-check deleted. Substitution sits at the provider boundary, not `render_request` (which runs several times a compile and feeds the digest). `theseus inspect` did not exist and was added. Seven record sites were listed and there were more (a node's `meta.diff`, the sandbox fact, a tool's subject, a watch's reply and mail lines, a batch's earlier steps). The "about 1 ms for 200 KB" is the verbatim pass; with the encodings it is 7.8 ms in release, and under 20 ms for the worst inputs built. **Two owner decisions, stated and not fixed**: a vault value whose secret the running daemon does not hold now (failed at this start, rotated, removed, or an AWS value from an earlier process) leaves as written and writes no `request.redacted` row; and a value cut by a cap, a clip or a split between the record and the exit leaks its visible half. The recommendation: accept both, name them, and record the secret's name on the node at save time as a follow-up. Open: the cockpit's ledger window still hides a vault value (a display scrubber); `JobArgs.redact` keeps its name; JSON object keys are not scanned |
| **R8** earned autonomy and scoped grants go (`lane/pr-r8-earned`, in the lane at `72abfc75`; 100 files, +521/-6,593) | See "What is gone" | The review sized the `policy/` layers; the full reach includes `rpc/earned.rs` (746), the daemon end to end, the protocol and Discord halves and the cockpit's types. `notice_press`, the DM-button dispatcher for Jev, standing, question, remind, seal and link cards, lived inside the deleted Discord `earned.rs` and moved to its own file with every branch in the same order. `policy.grants` and `theseus policy grants|revoke` went too, with nothing left to list. `SessionGrant` stays as a two-field struct (`kind`, `root`) that `covers` takes. With scoped grants gone, F10's S5a and S5b score 0 by design: deliberate, not a regression. If the owner ever moves to `enforcement = approve`, the weekly card is the first thing to want back (`git log -- crates/theseus-core/src/policy/earned`). `outbox.rs` still listed the `earned_card` kind and a test that drives every operator kind failed on it; fixed in the lane |
| **R9** `content.v3` (`lane/pr-r9-content-v3`, in the lane at `1a069b8e`; 26 files, +4,307/-80, most of it v2's text copied) | `ContentInput` gains `author`, `place`, `project`, `public`, `context`, filled in one place (`content_scene.rs`, from `prepare_content`); `PACK` flips to `content.v3`; the set is v2's 125 cases verbatim plus 26; `jev-probe --score-at-bar` replaces the offline scorer | Precision at the raising bar **0.956 against v2's 0.875, recall 0.985 against 0.955**, identical across two runs; `sec_14` and `sec_15` 1.00 in all four; `conf_14` (a real company's own test fixture holding its actual customers, missed by v1 at 0.50 and v2 at 0.28 against a 0.60 bar) raised at 0.76 and 0.74 (Beads theseus-znxfh folds into this set); `tb_11` and `tb_14` stay flagged, the price the owner accepted for `sec_14`. **The gain is the wording, not the five fields**: with the fields stripped from the 26 new cases zero answers changed at the bar; the fields buy margin (about +30% input tokens). And the owner's example (`fs.grep` over a public side project) was *already* not raised by v2 (it said yes at 0.32 to 0.40, under the bar); only v1 ever raised it. `public` is true only for an item whose own paths are all under a listed tree, read from the call's `path`, `paths`, `file`, `filename`, `dir` and the rest (a tool result's `source` is the tool's name alone), and an item that names none takes the session's directory; it errs toward false because it is read as the operator's word. `place` is computed and sent and no question reads it yet; `injection` is v2's byte for byte. The wording was iterated against the set, so `conf_14` and a customer list in a public repo's fixtures are a fix to a known case; the independent checks are the replay over the owner's own judgments (30 of them, none left out, $0.0033; one label raised before, none now) and, once the labels accrue, the nightly report. A builder version of 2 applies to v3 and to any learned pack (101 and up); `theseus judge replay content.v2` is refused on the owner's store ("already recorded under another text"), which says the installed v2 text and the file in the tree differ |
| **R10** docs (`lane/pr-r10-docs`) | This Item; `docs/status.md`; the config template's final pass; `docs/privacy.md`, the CLI and herdr guides and the README's promises; the design document's superseded note; Beads `theseus-6rugt` closed as will not do, `theseus-mjowe` as done by R1, `theseus-znxfh` as folded into R9's set | Writes no code. Where a doc and the code disagreed the code won; stale comments in code are listed in the row's report, not fixed |

**What this Item supersedes.** Part I's "External text" section (§3.9) and Items 10, 11, 74 and 84 (the T1 hold, `/trust`,
the integrity lane's latch, a wake held after outside text) describe a hold that is gone; the `external` marker they
introduced stays. Part I's "Scrubbing" section and Items 19, 44, 204 and 226 describe a scrubber that rewrote the record,
withheld a granted value from the spool and guessed shapes; only the exact-value passes and their encodings survive, and
they run at the exits. Item 250's single list of sensitive names (a request-level floor) is now a label source. Items 62 and 77's egress (a host a call names beyond `[sandbox] egress` "waits for your approval" and
holds the session) is now a notice, and the job's result is still marked outside text. The open view (theseus-a1wvq.3 to .17), the content flags'
first form (theseus-9ymz0, 2026-10-26: judged-through mark, settle loop, the chokepoint's proof), the Google private
marker, earned autonomy and its weekly card, and the interim external-text floor (theseus-9bp) never had Items of their
own; this Item is their record, and their end.

**How it was proven.** Every code row was built red first (R8's deletions partly), reviewed by a fresh Opus context that read the code rather than the
report (and ran mutation plants where it could), and joined with the control habit that proved the most valuable of the
lane: **a single red in a full-suite gate gets its module run alone on the lane and on a control cut from the same
base before anything else**, and the numbers go in the join line. The M1 Max was too loaded for one gate to be evidence on
the row's own reds (three gates each reddened one different test, each of which passed ten of ten alone, on the lane and
on the control), and the control habit twice caught a break neither row's tests could see (R8 removed a Discord arm that
main's later test drove; a join fix of R9's `public` rule broke a replay test that rebuilt a message with no paths). The
reviews' findings that mattered most: R3's Remove that undid itself; R7's blocker (an encoded value reaching the provider
unrecorded); R6's two jobs the hold did and nobody listed; R9's review correcting the row's headline (the wording, not the fields);
R1's `public_paths` collision. Each row's report, red and green runs and review are kept with the lane's records. Not verified live:
R2, R4 and R7's CLI against a scratch daemon of its own (the rigs drive the real core, store and chokepoint in process);
the cockpit's chip was not screenshotted.

**Left open, and the owner's.** (1) Google content and a model of EU origin (R4), above. (2) The two exits the owner is
asked to accept (R7). (3) The locked approval card and the DM golden's line 8 (R6). (4) The AWS L1 job and the egress
list (R6). (5) Whether a names label in a shared place should show the word. (6) `open_source_paths` and `context` in
`/etc/theseus/theseus.toml`, the owner's values; and the `[privacy]` comment there. (7) The rest of the gauntlet tree
(`~/lanes/gauntlet`, `bench/theseus-bench*.toml`, `bench/h2h/theseus.py`) still carries the retired keys: any bench run
that pairs a post-R6 binary with it dies at daemon start (theseus-gwceh). (8) The cockpit's profile floor table and its
words still say "US origin" (`cockpit/src/ops/floor.ts`, `test/fixtures/profile_floor.json`). (9) `theseus-6rugt`, to
rewrite a node after a `secret` label, is **will not do**: it is the opposite of "the session is the true record", and
the `secret` label is the guard for a model of restricted origin.
(10) A Remove of a names label lasts only until the next item that names the word, so in an employer repo it
returns on the next read of a file naming the employer: quiet the names source for the session, a per-session
allow, or as built (R3's O1, which this lane's conductor called the deepest question it raised). (11) A session
whose items named a word before R3's install carries no label for it, and it has lost the walk that used to stop
it (R3's O3). (12) A retired session's label counts as agreement after a day rather than at once: a departure
from the review's section 6.2, taken by the conductor and the owner's to veto (R2). (13) The config spells the
injection freeze `refuse`; `freeze` is the owner's own word for it, and no config writes either today (R1).
