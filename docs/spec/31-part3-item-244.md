# The Ship of Theseus, chapter 31: Part III, A4's Items 244 to 246 (Discord's pings, and a loop's thinking in its tool message; memory.lookup over the CLI; recall's words in time) ([index](README.md))
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
