# The Ship of Theseus, chapter 5: Part I, §4 to §9, the context graph, memory, storage, shells, verification, and efficiency ([index](README.md))
## 4. The context graph

One master graph per deployment. Everything the agent could put in front of a model is a node; everything relating nodes is a typed edge. This is the session model, the transcript, and the memory system at once.

### 4.1 Nodes, edges, roots

- **Nodes:** `Message`, `ToolCall`, `ToolResult`, `Judgment`, `Task`, `Execution`, `Heartbeat`, `Person`, `Topic`, `Event`, `Resource`, `Capability`, `Role`, `Summary`, `Synthesis`, `Suppression`, `Redaction`, `Root`. Every node has an immutable **record** (`kind`, `origin`, payload reference, timestamps, authored edges) and a mutable **projection** (`retention` and FSRS state, heat, `trust`, task state, index membership) that is rebuildable from the record and the ledger. Append-only applies to the record; projections change freely.
- **Edges (typed, directed, timestamped):** `next`, `replies_to`, `in_channel`, `by`, `about`, `derived_from`, `summarizes`, `supersedes`, `contradicts`, `same_entity`, `mentions_conversation`, `in_role`, `evidence_for`, `judged_by`, `part_of`, `in_session`, `includes` (compilation → node, ranked), `reports_to`, `addressed_to`.
- **Multi-parent by design.** The graph is a DAG, not a tree. One `ToolResult` written by a task execution is `in_session` of that task, `in_channel` of the channel it was reported to, `includes`-d by any number of later compilations, and `summarizes`-d by a compaction. Each membership is its own edge; none is privileged. **Order is positional**: every record carries its monotonic WAL position, and any lineage (a channel's transcript, a session's tail, a compilation's contents) is a filter over membership edges sorted by position. `next` is a convenience edge for the per-channel fast path, never the definition of order.
- **Roots:** a node a lineage starts from. A channel's first node is its first root. A `Compilation` (§4.4a) is a root for the session that made it; ~~a compaction's `Summary` is a compilation whose `summarizes` edges point at the range it stands in for.~~ _A compaction's `Summary` carries the range it stands in for (its first and last position and its node count) as its lineage, with no per-node `summarizes` edges, and the latest summary is the floor of every later recompile: the compiler renders it first and never selects its range again (since 2026-10-04, step 30c; Part III Item 131)._ The range is not removed; it is simply outside the default view. The graph never loses a node.
- **A context is a path selection.** The guaranteed default: follow `next` back to the nearest root within the channel. That is a plain transcript, served as a sequential read of an append-only per-channel log, never a graph traversal, and it must work with no Jev and no index.
- **Classificatory kinds are ontology.** `Person`, `Topic`, `Event`, `Resource`, `Capability`, and `Role` are seed kinds of the ontology (§4.1a), and their instances are categories. A new kind is a row in the ontology's table, not new code. The machinery kinds (messages, tool calls, tasks, executions, compilations, summaries, judgments, suppressions, redactions) stay code.

### 4.1a Ontology: kinds, categories, memberships, guidance (the owner, 2026-09-27)

_The ontology is fungible (§2). This section began as openrig's pods, which it calls "context domains" (Appendix F), and was generalized at the owner's direction so that nothing about which kinds of context exist is hard-coded. The first new kind is the **topic**. Beads: theseus-8kk._

**What is code, and what is data.** The code is small and fixed:
- the machinery kinds (§4.1);
- one generic `Category` node;
- one membership edge, `member_of`, carrying `origin` (`transport | operator | jev | sweep | dream`), `confidence`, and `as_of`;
- `Guidance` nodes attached to categories;
- a closed set of **composition rules** the compiler knows:
  - `chain`: walk up the parents, nearer overriding farther (for rules);
  - `intent_line`: one line per category (for intent);
  - `ranked`: admitted within the budget by relevance (for lessons);
  - `recall_only`: never admitted automatically.

The data is a versioned **kinds table**, like the role table (§3.4). Its seed rows are `channel`, `guild`, and `person` (given), and `topic`, `culture`, and `expertise` (interpreted). Each row declares:
- what may be a member, and how many categories of the kind a session may hold;
- its parent kind (topics nest);
- its precedence against other kinds, so that clashes resolve the same way every time;
- which origins may assign membership;
- its composition rule;
- a description, which is embedded.

The owner or an operator adds rows in the web UI. _(Since 2026-10-04, 21c; Part III Item 111: in the cockpit's Ontology view, `/ontology`: the kinds, the category tree, a topic added under a parent, and a category's guidance set, each write confirmed with the words it sends; a session's deck lists its memberships and adds or removes a topic, which applies at the next recompile. `theseus ontology` does the same from the CLI. Kinds are not added there yet.)_ Jev or a dream may propose rows, which are accepted like new roles. Every row carries `added_by`.

**Four verbs.**
- *Added*, as above.
- *Trained*: a Jev `categorize.v1` judgment assigns memberships, in shadow first, and operator corrections are its signal.
- *Re-associated*: a membership is an append-only record. A sweep, a dream, or a live check writes a newer one that supersedes the old.
- *Embedded*: each category keeps its description and a centroid of its members in the index tender. They are used to propose memberships for new sessions, to find near-duplicate categories to merge, and to find drifted ones to split.

**Two guardrails.**
- *Given versus interpreted.* Channel, guild, and person memberships come from the transport. They are facts, and they are never re-associated. Topic, culture, and expertise memberships are interpretations, and they may be.
- *Interpretations route context but never grant access.* Which nodes a principal may read, and which audiences may see them, is decided by §3.9's ~~labels~~ place rule (since 2026-10-03, Part III Item 76) and the bindings the operator declares. An interpreted membership never decides it. It is the same line §1 draws for roles.

**The compiler.**
- A compile looks up the session's current memberships, one read per kind, all precomputed. FAST forbids an embedding search or a Jev call on this path.
- It walks each category's parents and admits guidance by its kind's rule and precedence.
- The manifest records every membership used, with its origin and as-of, so "why did it know that?" always has an answer.
- An interpreted membership change takes effect at the session's next recompile, so the prompt cache survives it. A declared change that alters access forces a recompile (§4.4a). _(As built, 21b, 2026-10-04; Part III Item 100: the compile composes twice, an append's spec from the memberships its manifest recorded and a recompile's from the current ones, so the choice to append rests on the recorded set and a change waits for the next recompile. The guidance goes in the system's second block, after the context files, under `# Guidance (topic <name>)`, so the header and its cache breakpoint are untouched. A given membership is read from the session's place at compile and never stored. A shared place's walk keeps only its own place's given membership, so it gets only that place's guidance; no `guild:` categories yet.)_

**Consequences** (§3.9) were also a kind in this table, on the authority side, with memberships from deterministic detection only. _Superseded 2026-09-28: the consequence kinds were removed with the detection that assigned them (§3.9; Part III A3b)._

**Phasing.**
- **M4:** the kinds table, declared memberships, guidance, and the compile walk, beside ~~the labels~~ the place rule (Item 76). _(Built 2026-10-04, 21b; Part III Item 100; store format 7.)_
- **M5:** `categorize.v1` in shadow. _(Built 2026-10-04, step 28b; Part III Item 126: at a private conversation's exchange end, once 10 human messages have come since its last judgment or after 30 minutes' quiet, over the topics the kinds table lets the operator assign (at most 50 offered). The operator accepts a proposal, which writes an `operator`-origin membership and its label in one frame, or rejects it, and either is the pack's label. No topic declared, no judgment.)_
- **M6:** embeddings, sweeps and dreams, lessons as guidance, and §5.5's namespaces as kinds.

Every kind picks one of the closed composition rules, so no kind can exist without a reader (P0's standing rules).

### 4.2 Continuation strategies

1. **Transcript**: root → current. Always correct, always cheap.
2. **Ring**: transcript truncated from the front at the token boundary. Zero model cost.
3. **Compaction root**: summarize the range the ring would leave out into a `Summary` and re-root. One cheap call; the raw range stays in the graph and reachable. _(Built 2026-10-04, step 30c; Part III Item 131: where the ring would cut, `[memory] summary_profile` summarizes the dropped range into a `Summary`, by default `session`, the turn's own profile, provider and model, so no second provider reads the session (the owner's decision 3, until Jev routes it). The call is a kernel action, reserved and settled at its real cost, and a second compaction folds the first summary in. Every failure (no provider, an unpriced model, the spend limit, a failed call, a summary that would not fit) falls back to the ring, and the row says why.)_ _(Since 2026-10-06, theseus-6fn.8; Part III Item 210: the summary's call is reserved on the input estimate's upper bound, so a provider counting the input over the estimate cannot settle past the reservation (live: 1,608 µ$ reserved where main reserved 1,435, against 1,483 settled). A turn's own call still reserves on the estimate's tokens (theseus-ps9i).)_
4. **Assembled**: transcript tail plus a budgeted, Jev-selected neighbourhood: durable nodes, the task subgraph in play, borrowed summaries, participant `Person` nodes, the active role's hints. _(Built 2026-10-04 in a first form, step 30c: a task's first compile and a compaction put a recall section first in the prefix (recall's pipeline at `[memory] assembled_budget_tokens`, 4,000, following the session's arm), then the summary, then the tail, and the compilation records the section as its `recall_id`. Nothing else of the neighbourhood is selected yet, and Jev selects none of it.)_

Strategy 1 is what *appending* means; strategies 2 to 4 are what a *recompile* produces (§4.4a). Jev's `continue.v1` answers both questions at once: append, or recompile with which strategy, and whether a stronger model performs the compaction or synthesis. Strategies 1 to 3 need neither Jev nor the index.

### 4.3 Indexer and budgeter

The **indexer** turns "everything is eligible" into a few hundred candidates: exact indexes by channel, person, topic, and time; hybrid similarity through the index tender (§6). Incremental on every append. The **budgeter** allocates tokens across prompt sections and money across judgment calls per turn, weighted by the active role, aware of cache-read versus cache-write pricing, and logs every allocation for the ledger.

### 4.4 The context compiler

Graph selection yields node ids; the **compiler** turns them into a valid provider request. Its contract: preserve tool-call/tool-result pairing (never emit one without the other), preserve message ordering within a channel, honour model-specific constraints (thinking blocks tied to the provider and model that produced them, below; no forced tool use on models that reject it), truncate only at message boundaries (the ring never cuts inside a message or a tool pair), and emit a **manifest** that records node ids, compiler version, renderer version, binding revision, model, and pack versions, so a turn can be reproduced faithfully from the ledger. The compiler takes the **session** (§3.2a) as its first input: the session's roots decide where traversal starts, so a task session compiles from its `Task` node outward (evidence, subtasks, the originating conversation as a secondary root, recalled memory) while a conversation session compiles from the channel's temporal view; both then pass through the same budgeter, labels, and manifest. _(Since 2026-10-04, step 28a; Part III Item 132: a check task (`task.create { check_of }`) compiles from its brief and an `Arrangement` node that admits the checked task's objective and acceptance pieces and its report as a claim, and nothing else of the maker's session. A piece of its own that derives from the maker's session by `derived_from` edges is refused, and a brief or piece sharing 12 words or more with it is flagged; the basis is recorded on the check's session record and shown beside its report.)_ The compiler is deterministic and unit-tested in the simulator against every strategy in §4.2. **Reproducibility** requires more than node ids because projections mutate: the manifest records an **as-of WAL position**, tool-schema versions, role versions (and hook versions, until the hook system was deleted on 2026-09-28), pack versions, binding revision, model, and every prompt-affecting transformation, plus a **canonical request digest** so a reconstruction can be verified byte-for-byte. Judgment records likewise store the bounded state itself (or enough versioned references to rebuild it), not only its hash and size.

**Thinking goes back only to its own provider** (theseus-kol; built 2026-09-30). Thinking is replayed byte for byte, and only to the provider that wrote it.
- An assistant message another provider wrote is rendered without its thinking blocks, wherever it sits, since a signature is its own provider's to verify (Anthropic refused GLM's with a 400).
- Every recompile strips the prefix's thinking: a system, tools, model, or provider change, a ring, fresh, or transcript. The earlier model's reasoning goes; its text, calls, and results stay.
- Within one provider, a message keeps its thinking byte for byte, a tool loop's last message included, as the provider requires. The rule compares providers, not models, since a node records the model that served it, which can differ from the one asked for. A model change is caught at the compilation, whose manifest records the model asked for.

**Context files** (2026-09-29; theseus-58a, 76e7d35, a29e9f7; two levels since theseus-c48, 68cb127). The config names files that the compiler puts into the system block, in two levels. In the owner's words: "Context files by persona with a system/default level", and "the Jev will classify which persona is in play, based on a dynamic, growing ontology of personas. Pre-jev, it will always pick the system/default persona."
- **The system level,** `[context] files`: the files every session gets.
- **A persona's files,** `[personas.<name>] files`: added after the system level's while that persona is in play.
- **Which persona.** `[context] default_persona` is the only choice until Jev chooses one from the ontology (§4.1a; theseus-8kk, theseus-0j2).
  - An unknown name is refused at load, and the error names the known ones.
  - With none set, the system level alone is used, and personas defined without a default warn once at load.
- **Placement.** The system block is the built-in persona, the tools note, and the profile's `system`, then the system level's files, then the persona's. _(Since 2026-10-05, theseus-fpm2, on the owner's ask of 2026-10-04 23:55; Part III Item 156: an assembly note follows the built-in persona, before the tools note, so the model knows its context is assembled: "Context. The harness assembles each request: the conversation, its older part perhaps summarized; notes it recalled from earlier sessions, marked as recalled; files the person attached; tool results; and its own notices, such as wakes, task reports, and late results. A recalled note is background the harness chose, not something the person sent. The person sees their own messages and your replies, not the rest of the request, so use a recalled note when it helps, and otherwise don't mention it." It is static, 81 words, so every session of a profile still shares one cached header.)_ Each file is in list order, under a header that names it and its level: `# Context file (system): <path>` or `# Context file (persona theseus): <path>`.
- **No gate.** The compiler reads the files itself, not through a tool, so no posture or approval applies.
- **Digest.** Their text is part of the system block, so the system digest covers it. An edit to a file at either level is one `system_changed` recompile, and an unchanged file appends.
- **Timing.** A turn's system block is fixed for all its loops. A file edited during a turn takes effect at the next turn, never between a tool call and its result.
- **FAST.** Nothing is read at startup. A turn stats each file and rereads it only when its size, mtime, or inode changed, or when it had changed less than two seconds before it was last read. A file named at both levels is read once.
- **Failure.** A missing or unreadable file never stops a turn. The block says it is missing, and the daemon warns once per file per run (a log line and a `context.file_missing` row).
- **Cap.** A file over 64 KB is cut at a character boundary and marked as cut.
- **Records.** The manifest and every `context.compiled` row record each file's path, the digest of the text included, its bytes, whether it was cut or missing, and its persona (absent at the system level). The row names the persona in play. Health, `theseus health`, and the Observatory show the persona in play and each level's files.
- _(Amended 2026-09-29, theseus-c48: until then a profile carried `context_files`, defaulting to `[model].context_files`; both are gone.)_

**Attachments and images** (2026-09-29; theseus-9g2, 7bcfd9a, 48fd2c8). A user message carries the files that
came with it, and a tool result may carry an image.
- **Where they come from.** `turn.submit` takes `attachments: [{name, media_type, size, text? | data? (base64) |
  not_read?}]`.
  - The Discord binding downloads text files, known by type or extension, up to `[tools].max_read_bytes`, and
    PNG, JPEG, GIF, and WebP images up to 5 MiB. It downloads them beside its gateway loop, and the message's
    turn waits for them. ~~Anything else is listed with its type, size, and reason.~~ _Since 2026-10-05 (theseus-c9l6; Part III Item 155), any other file up to `[tools] max_attachment_bytes` (32 MiB) is downloaded too, as its bytes, and kept whole in the store's blobs (`AttachmentContent::File`, store format 17); notebooks and RTF travel as bytes though they are text._
  - `theseus ask --attach <file>` sends a file the same way.
- **What the node keeps.**
  - Text is capped at `[tools].max_read_bytes` on a character boundary and marked if it was cut.
  - An image is a reference (digest, type, size, and dimensions) to its bytes. The bytes are stored once in
    `<store dir>/blobs/<sha256>` and never in a WAL frame, so the frame budget and recovery are unchanged.
  - A file that was not read keeps the reason.
  - Nothing about an attachment fails a turn.
- **Placement.** Each attachment is its own block before the typed text, under a header that names the file and
  its sender (`[Attachment message.txt from discord:zeroaltitude, 5,012 bytes]`). The
  header is what marks the text as the file's (no label on a node does, since Part III Item 76).
- **Vision.** The catalog entry of the compilation's model decides how an image shows. A model with vision gets
  an image block. Any other model gets one line: `[Image photo.png from discord:zeroaltitude, 1.2 MB: not shown, this
  model has no vision]`. `fs.read` of an image does the same inside its `tool_result`.
- **Byte-stable.** The render reads only the node, the model's vision flag, and the blob, through a bounded cache.
  The same node renders to the same bytes, and a message without attachments renders exactly as before. _(Since 2026-10-05, theseus-c9l6, in two joins; Part III Item 155: a file is read once, by its digest, in a capped child of the daemon's own image (`theseusd files-convert`: 30 s, 1 GiB, no file writes, an empty environment), and what was read is a blob the node names. A PDF reaches a Claude model as a `document` block of the file, or the largest part the request has room for, with a line naming the pages left out; GLM gets its text page by page, a scanned page saying it has no text. PDFs are budgeted in the order they render, so an earlier one never renders differently because of a later one and the cached prefix holds; a PDF the provider refuses shows as its text from then on, as a refused image does. Word, OpenDocument, RTF, Excel, PowerPoint, EPUB and notebooks are read into text on arrival, for every model; an archive shows its list. Long text is cut at 256 KiB a file, with words that say how to read on, and `file_read` reads the rest: a PDF's other pages, a document's sections, an archive's member, a recording's transcript (only when the model asks, at about $0.26 an hour, booked as `speech.transcribed`, capped by `[tools] transcribe_max_minutes`), a video's transcript and four frames, an image's text by OCR.)_
- **Budget.** The input estimate leaves out base64 data and adds each image's estimated tokens. The image's long
  edge is scaled to 1,568 px (Haiku 4.5) or 2,576 px (the rest), then it counts ⌈w/28⌉ × ⌈h/28⌉, capped at 1,568
  or 4,784.
- **Refused.** An image over 5 MiB or 8,000 px on a side, or whose bytes are not one of the four types, is listed
  with the reason.
- **An image the provider refuses is shown as its line** (theseus-0s4; built 2026-10-01, Part III A4 Item 15). A node
  is written once, so an image the provider rejected went out again in every later request of its session, and
  failed it the same way. Examples: corrupt pixel data behind a valid header, or a limit the sniffer doesn't check.
  - A 400 that names an image now marks it not shown in the session record (`not_shown`, by its blob's digest;
    session schema 4), with an `image.not_shown` row.
  - The turn makes the call again at once, with the image's line ("not shown, the provider refused it (…)") in its
    place.
  - Which image a 400 means: a 400 names one by its block's path. Anthropic's own "Could not process image" names
    none, so then the error means the images the model has not answered over yet: those after its last answer, or
    the request's only image when none is new.
  - This happens once a turn. A refusal that names no image fails as any other, under §3.15's rule.
  - Every later request, recompile, and restart renders the line, for every copy of the image.
  - When a copy sat before one of the model's answers, the retry's compilation strips the thinking before it
    (`image_not_shown`), since the provider binds a thinking block to every message before it.

### 4.4a When a session is recompiled

Compiling every turn from scratch would be wrong twice over: it burns a prompt-cache miss on every turn, and it discards the plain fact that most turns in a live thread are simply the next thing said. So a session's context has two parts:

- a **compilation**: the prefix, produced by the compiler from the session's roots at some as-of position, persisted as a `Compilation` node (root kind) with its manifest. A compaction root (§4.2) is one kind of compilation; an assembled context is another.
- an **append tail**: everything the session has added since, in order, with no selection. This is the transcript strategy, and it is the default motion of every session.

Each turn the harness asks one question before the model runs: **append, or recompile?** The answer is layered so that the expensive judge is consulted only when something has actually changed.

1. **Deterministic triggers force a recompile** and never consult Jev: ~~the audience or a confidentiality label in play changed (disclosure, §3.9);~~ a declared ontology membership changed in a way that alters access (§4.1a; an interpreted membership change waits for the next recompile, so the prompt cache survives it); policy, tool schemas, role, or binding revision changed; the model changed; the tail would overflow the window or the configured tail budget; the session is new (a promoted task's first turn is always a compile); the execution glided to another channel; a redaction touched a node inside the current compilation. _(The audience: built 2026-10-02 in 19a as the `audience` trigger, Part III Item 61. A compile for another audience than its manifest's recompiles, its prefix's thinking stripped, and any change of audience counts, not only one that changes an admission (theseus-osl1). A compilation from before labels keeps appending until something in its session would be withheld.)_ _(The audience trigger went with the labels on 2026-10-03, Part III Item 76.)_
2. **Candidate signals arm the judge**, cheaply and deterministically: a reference to another channel or an old topic (`mentions_conversation`, a recall hit outside the tail), a material task-state change in the session's scope, a long dormancy gap, a role hint change, a human asking for a fresh look, the tail crossing a soft length band, a cache-state change reported by the provider. If no signal fired, the turn **appends** and Jev is not called. _(Built 2026-10-04, step 25b; Part III Item 122: dormancy (`[judge.signals] dormancy_minutes`, 360), the tail's band rising (`tail_band`, half the window, then each further quarter), a task's report or a wake arriving, and a cache miss under an unchanged prefix, each measured since the model's last answer and computed on every compile, judge on or off. A signal is a value and words, carried on `context.compiled`. The other signals in this list are not built.)_
3. **Jev decides when a signal fired**: `continue.v1` receives the signals, the tail length, the cache state, the current compilation's manifest summary, and the budget, and answers `append` or `recompile(strategy)`. Jev owns this judgment as a core responsibility: it is deciding whether the world has changed enough that the model needs a rebuilt view rather than one more message. _(In shadow since 2026-10-04, step 25b: `continue.v1` is asked only when the compile appended and a signal fired, so no trigger asks it; it runs off the turn's path, and its answer is recorded, never acted on. A signal can fire at a later loop of a turn, so the pack may be asked mid-turn.)_

Consequences. A long single-thread conversation appends turn after turn; its prefix is stable and cached; it behaves exactly like a normal transcript, because it is one. A task promoted from that conversation gets its own session and therefore its own first compilation, built from the task outward, then appends as it works. Diverse work is thereby compelled into distinct sessions, each compiled when it was needed and not again until something real changes. Recompiles are ledgered with their trigger, their cost (the cache miss, the compaction call), and the trajectory that followed, so the learning loop can tune the soft bands and Jev's threshold against outcomes: a recompile that did not change what the model did next was waste, and a stale append that preceded an error was a missed recompile.

Recompilation never loses anything. The old compilation, the tail, and the new compilation all remain in the graph with `derived_from` edges; the manifest of every turn names which compilation and which tail range it used, so any turn is reproducible.

**How the compiler sizes a request** (theseus-f5hf; built 2026-10-01, Part III A4 Item 31).

- **What is counted.** From a compilation's second call on, the estimate counts on the provider. The session's latest answer came from this compilation's last request, whose input the provider counted (input, cache reads, and cache writes), and the answer re-enters the next request at its own output tokens. Only what was written since is estimated.
- **What is estimated.** The new part is estimated from its bytes by class, at the catalog's `bytes_per_token`: JSON (tool schemas, tool inputs, tool results) and text, plus a few tokens of framing per message and block, and 15 per tool call id. A request with nothing counted (a new session, a recompile, the ring's candidates) is estimated from bytes whole.
- **The ring.** Overflow rings when the counted part plus the estimated part × 1.4 passes the window, less the output cap and 4,096 tokens of headroom. It then drops leading turns at a user message until the estimate is under 60 % of that.
- **What is kept.** Nothing is stored for it: the counts are on the answers' nodes, so a restart changes nothing. Every `context.compiled` row carries the estimate: its method, both parts, the bound, the request's JSON bytes (a fourth of them was the estimate before), and its bytes by class.

- **The provider's word** (theseus-9p88; Part III Item 34). A request the provider says passed the window rings
  whatever the estimate says. That is a 400, "prompt is too long: N tokens > M maximum", or an answer that stops
  with `model_context_window_exceeded` (on the 4.5-and-later models a prompt plus `max_tokens` past the window is
  not refused; the answer stops there). The ring uses the smaller of the catalog's window and the provider's (the
  refusal's maximum, or a cut answer's prompt plus its output), and reads each candidate at the provider's count:
  its estimate times the count over the estimate of the request that overflowed. The call is made once more. A
  cut answer stays in the record, and is left out of the retry and of every later request, with its calls; its
  turn's later answer replaced it. A retry that passes the window too, or a ring with nothing earlier to drop,
  fails the turn with `context_window`, naming the window and the estimate, and the session waits on its next
  message, which gives the ring a place to cut. Each is a `context.overflow` row. A GLM refusal is read only in
  Anthropic's wording for now (theseus-kucs). _(Since 2026-10-04, step 30c; Part III Item 131: where the ring would cut, a compaction summarizes the dropped range first (§4.2), and the ring is its fallback. When the ring's last cut still has an overage, or a request has nothing to drop, the turn fails before any call with `context_overage`, naming the model, the window, the estimate and its upper bound, the limit and how far over, with a `context.overage` row, and the session waits on its next message as for `context_window`. Both classes stay: `context_window` is the provider's verdict after a send, `context_overage` the estimate's before one.)_

### 4.4b How sessions persist across runtime restarts

A session is **durable by reference**, and that is the whole plan. Suppose a thousand sessions each hold a custom-selected context. Nothing about those thousand contexts is held in runtime memory that matters; each resolves in the store to a persisted **lineage**.

**What is stored per session.** One small record, updated in the same WAL transaction as the turn that changes it (§4.6):

```
Session {
  id, kind: conversation | task, roots: [channel id | task id, ...],
  execution_id,                                   // §3.15, itself durable
  compilation_id,                                 // current prefix
  tail: { from_position, last_position },         // append tail = in_session nodes in this range
  strategy, budget_ref, labels_in_play, config_versions
}
```

_(As built in 19a, Part III Item 61: what a compilation was compiled for, its audience, the meet of what its prefix admitted, the integrity in play, and the nodes it withheld, is in the compilation's manifest (COMPILATION schema 4). The session record has no `labels_in_play`, and 19a left it unchanged.)_ _(COMPILATION 5 drops the audience, the readers, the integrity and the withheld nodes, with the labels; the manifest marks each context file public or not, and `context.compiled` records the place's class and what it withheld; Part III Item 76.)_

**Finding a session's own records.** Positions are global across every session, so a `tail` position range is full of other sessions' records on a busy runtime. The index therefore carries a per-session ordered table, `session_id ‖ position → ()`, and the tail walk is a range scan over exactly this session's records. The table is the same shape as the per-kind table and is rebuilt from the WAL like the rest of the index. _(Added v0.24, before M2 began; Session records are M2 work.)_

**What a compilation is.** A `Compilation` node whose record is the **selection**: ranked `includes` edges to the nodes it admitted, plus the manifest (§4.4: compiler and renderer versions, as-of position, model, binding revision, pack versions, request digest). Its rendered form, the provider-message bytes, is a **cache** stored alongside it and rebuildable byte-for-byte because the compiler is deterministic given the manifest. A thousand custom contexts are a thousand `Compilation` nodes that share the underlying nodes; no content is copied per session. At a few hundred references each, that is a few hundred thousand edges, which is nothing.

**What a lineage is.** `Session → Compilation → derived_from → Compilation → … → first Compilation`, each carrying the tail range it was compiled from, so the full history of *how this session saw the world* is a chain of selections over one shared history. Promotion (§3.2a) is a branch in that DAG: a task's first compilation has `derived_from` the conversation's current compilation as well as `includes` edges into the conversation's nodes, which is why the graph must be multi-parent and why it is one.

**Restart.** The runtime reads Session and Execution records back from the store; nothing is "resumed" in memory. On a session's next turn the compiler loads the current compilation (the manifest, and the rendered cache if it is still resident on SSD), scans the tail by position, and proceeds with the append-or-recompile question exactly as if no restart had happened. The rendered prefix is reproducible, so a restart inside the provider's prompt-cache window still hits the provider cache. Sessions that are `waiting` cost a record each and nothing more; the resident graph (§6) rehydrates what a turn touches and only that.

**What compile touches, and what it never touches.** On the common turn the compiler loads the current `Compilation` by key (one index lookup), takes its rendered cache if resident or re-renders from its `includes` list (positioned reads, mostly arena hits), walks the session's tail through the per-session table (a handful of records since the last compile), and decides append or recompile (§4.4a). A recompile builds its candidate set from the retrieval indexes (§6.1) and adjacency scans bounded by the session's neighbourhood, then writes a new `Compilation` with `derived_from` the old one. Every step is a key lookup, a range scan, or a positioned read. The WAL is walked from a checkpoint in exactly two situations, crash recovery and index rebuild, and neither is on a turn.

**Consequences for storage.** Rendered compilation caches are the first thing tiering demotes, since they are rebuildable; selections and manifests are records and stay durable like everything else. Redaction (§5.6) of a node included by a compilation marks that compilation dirty, which is a deterministic recompile trigger (§4.4a), and restore applies tombstones before any rendered cache is trusted.

### 4.5 Prompt caching layout

Most stable first: persona and role hints; tool schemas; frozen transcript prefix from root to the last compaction; cache breakpoint; dynamic assembly; recent tail and new message. Compaction roots keep the prefix small and stable by construction. `continue.v1` receives cache state so it can prefer appending on hot conversations, and a recompile is scheduled at a natural boundary (after a tool loop closes, not mid-loop) whenever the trigger allows deferral. The system leads the prefix, in two blocks (theseus-ev1; built 2026-10-01, Part III A4 Item 29). The first, the **header**, holds the built-in persona, the tools note, and the profile's `system`. Every session of a profile and their tasks share its bytes, and nothing retractable goes in it (Appendix F). The second holds the context files, the system level's, then the persona's (§4.4). Each block carries a cache breakpoint, and the conversation's is the request's top-level automatic one: 3 of the provider's 4. An edited file, or a persona that Jev switches, rewrites the second block and what follows, while the tools and the header still read from the cache.

**The rules a breakpoint follows.**
- **The minimum.** A block whose prefix (the tools and the system through it) is under 2 bytes a token of the model's `cache_min_tokens` gets none, since the provider could never cache it. The bound errs toward placing a breakpoint: one the provider skips costs nothing, and one dropped that would have cached costs a rewrite in every session. The header, about 13 KB with the tools (5,045 tokens on Sonnet 5.5, just under Haiku 4.5's 4,096), is sent with its breakpoint on every built-in model, and on Haiku the provider skips it for now. A model whose catalog row says `caches = false` gets no breakpoint at all.
- **The record.** The compilation's manifest records the layout (`cache`: whether the model caches, its minimum, and each block's prefix in bytes and its mark).
- **The TTL.** A profile's `cache_ttl` is `5m` (the default) or `1h`, and applies to every breakpoint, the automatic one included. A task's own conversation keeps `5m`, after the header's `1h` entries, so a longer-lived entry never follows a shorter one, as the provider requires.
- **The price.** A 1-hour write costs 2 × input, against 1.25 × for 5 minutes. The usage carries the 1-hour part (`cache_creation_1h_input_tokens`), and the catalog's `cache_write_1h_per_mtok` prices it everywhere a call is priced.
- **What a token is, by model.** The catalog's `bytes_per_token` says how densely a model's tokenizer reads a request's JSON and its prose (§4.4a uses it). On Sonnet 5.5, the header's 15 tool schemas with the provider's tool prompt run about 2.5 bytes a token, tool results and code about 2.4 (from 1.76 to 2.84 in the owner's DM), and prose 3.35. GLM-5.3 Flash reads them at about 3.8, 3.3, and 4.5. The caching minimum's own bound, 2 bytes a token, stays below them all.

**One header across sessions** (the owner, 2026-09-26; theseus-ev1). The header is kept as static as possible and reused across sessions and the parts of sessions (derived tasks), so one provider cache entry serves many of them instead of each session warming its own. This is the **provider-safe caching** work, scheduled after M3; M3 built only a breakpoint on the system block plus automatic caching of each session's growing prefix (Part III A3). It respects each provider's rules (a header shorter than the model's `cache_min_tokens` in the catalog never caches) and never rewrites earlier history to win hits, since that breaks preserved thinking signatures. It lands together with money budgets (theseus-0sg): under a token budget a cache read counts as much as fresh input, so better caching would lower the bill without stretching the budget. _(Money budgets landed first, on 2026-09-29, after the owner's DM hit its unit limit; a cache read now counts at its own price, §3.13. The caching work landed in two parts: the cache lane (the header test and the Observatory's cache figures, Item 16), and 13c's two blocks, minimum, TTL, and 1-hour price, Item 29, on 2026-10-01.)_

### 4.6 What a turn writes

Inbound `Message` nodes; the turn trace (§3.3a); the `Compilation` node when the turn recompiled (selection plus manifest, and its rendered cache), otherwise a manifest reference to the compilation and tail range used; model output as `Message`; `ToolCall`/`ToolResult` with `in_session` and, when delivered, `in_channel` edges; loop `Judgment`s; `Task` mutations; and the updated `Session` record. Each is written in the WAL frame of the kernel transition that produced it (the model's message with its provider completion, a tool call with its planned action, an in-process result with its completion, a late result when it is absorbed), not in one transaction at the end of the turn, so a crash mid-turn leaves exactly the nodes whose effects are durable and a continuation resumes from them (as built in M3, Part III A3). The memory pass (§5) runs over these afterwards.

**The frame rule.** Every frame is an fdatasync, so a turn writes no more frames than its durability needs.
- A state transition, a node, the session record, and a compilation are each in a frame that is synced
  before the call that wrote it returns, as above.
- An observability row that is no state transition waits in the turn's store handle. It rides at the
  front of the next frame that the turn, or the kernel, commits for it, in the order it was written. These
  rows are `turn.started`, `context.compiled`, `loop.started`, `loop.ended`, `turn.ended`, `turn.trace`, and
  the tool layer's rows. `provider.call` rides in its call's completion frame, and `tool.notified` in its
  call's plan frame. A crash in the middle of a turn can lose some of these rows, but never a transition
  or a node. _(Since 2026-10-06, theseus-nhg4; Part III Item 195: a loop that ends on the budget question writes its `loop.ended` row too, in the frame the question writes (advancer and decision `budget`, no stop reason, no calls, zero usage), so every `loop.started` has its end; a hands group over the budget, checked before any loop opens, writes no `loop.ended`.)_
- An action that never waits for a confirm is planned, authorized, and dispatched in one frame: the
  provider call, and any tool call the policy runs (`open` or `notify`). Each transition keeps its own
  record and row, so the record still reads as three transitions.
- A result the turn reads itself (the provider call's answer, an in-process call's result, a job the turn
  waited for) is not queued for a later turn: the queue holds only what settled outside the turn. A turn
  that faults after settling one is woken (`execution.queued`, why `fault`), so its continuation reads
  what it settled and resumes its unanswered calls. A confirmed call is authorized and dispatched in one
  frame, and an input's wake and admission are one frame.
- A turn's session write at its end rides in `end_turn`'s frame, the turn's last, in the place it had as a
  frame of its own, under the session record's lock until that frame is written (theseus-l6y).
- A plain one-loop turn writes 5 frames: the input's wake and admission; the input; the provider call's
  plan, authorization, and dispatch; its completion; and the turn's end with its session write. Each
  further loop with one in-process tool call writes 4. `tests_m3::a_plain_turn_stays_within_its_frame_budget`
  holds the first number.
- A turn's target rides in its input's frame when it differs from the session's last (a session's first
  turn, a profile switch): one record more, in a frame the turn writes anyway (theseus-kol).
- A late result's node rides in the frame that takes it from the execution's queue, with its
  `tool.late_result` row (`Kernel::take_results_with`), so no crash between them can lose it (theseus-kol). _(Since 2026-10-06, theseus-2xep and theseus-6qwr, v1.1's step V3; Part III Item 194: every queue writes its row in its frame. A completion that moves a waiting execution to queued writes `execution.queued {why: result}` right after the action's row, in the completion's own frame, from `accept_completion`, so the AWS hands' poller, the turn, the reconcile and kernel-sim write it too; a turn that ends with results already waiting says `why: result` as well, and kernel-sim fails a run on a frame that queues an execution without its row. A late result's wake rides in the turn's end frame (`end_and_wake`, one frame that reads the stop under its lock, ends the turn and, when the turn says `rewake` and no stop was read, wakes the execution in a nested frame whose failure takes back only itself), so a turn that took a late result writes one frame fewer, no reader sees the session `waiting` between that turn and the late result's, and a settled wait parked before returns at the late result's turn's end. The push board reads a queue's why from `execution.queued` alone.)_
- A turn reads its session's transcript once, at its first reader, and every node its frames write
  joins that view. No reader in the turn decodes the transcript again: not resume, not absorb, not the
  check for anything new, and not each loop's compile.

_(Amended 2026-09-29, theseus-qa0 step F2: until then a plain turn wrote 17 frames, and decoded its transcript at every reader; and 2026-09-30, step F4b, theseus-l6y: 8 frames until then. Part III A3c.)_

**Calls that run together** (theseus-a60).
- A response's tool calls are gated in order. The first call that waits for the operator ends the gating,
  and the calls after it are left for the continuation, as before.
- The calls the policy runs go in groups. A run of consecutive reads is one group. Each write or program is
  a group of its own, a barrier: it starts after every call before it has finished, and the calls after it
  start after it finishes.
- An unknown tool or invalid input is answered at once, wherever it is.
- A group's calls run at once, in the turn's own task, and each keeps its own frames, plan and completion,
  written as they happen. So within a group, the WAL holds the calls' plan frames first, then their
  completions as they finish. Nowhere else does the order change.
- The next request carries the results in the order the model asked, because the compiler places each
  result after its call, whatever its position.
- A question for the operator is asked after every call before it has finished, so it keeps its sequential
  place.

_(Amended 2026-09-29, theseus-a60 step F3; Part III A3c.)_

## 5. Memory

The graph is append-only. Compaction adds `Summary` nodes and re-roots the default view; the trimmed range stays in the graph, reachable through `summarizes`, merely absent from that view. Decay lowers a node's retention and heat until the tiering tender moves its payload to cold storage, but the node, its edges, and its stub remain. Nothing is ever lost.

"Memory", then, is not a kind of node and not a survival test. It is **any node whose retention is high enough to be selected into a prompt**. Ingest is a durability decision over the nodes a turn wrote: how much retention each starts with, and which edges tie it into the graph. Not extraction of a separate object, and never a decision to discard.

### 5.1 `MemoryScience` trait, native implementation

```
trait MemoryScience {
    fn gate(&self, candidate: &Node, neighbours: &[NodeRef]) -> GateDecision;    // store | merge_into | contradicts | drop
    fn schedule(&self, node: &Node, event: AccessEvent) -> Retention;           // FSRS-6 update on shown/ignored/rated
    fn activate(&self, seeds: &[NodeRef], budget: usize) -> Vec<(NodeRef, f32)>; // spreading activation over typed edges
    fn decay_sweep(&self, now: Time) -> Vec<Demotion>;                           // candidates for the tiering tender
}
```

Native v1: gate by embedding cosine plus exact `about` overlap for duplicates, Jev `kind`/`durability` for store-vs-drop, and a Jev Noul for contradiction between a candidate and its top neighbour; FSRS-6 stability and difficulty per node with dual-strength kept as two numbers; weighted BFS over typed edges with per-edge-type weights the role adjusts; decay by retention plus heat. Parameters are versioned data tuned by the ledger. A remote scorer is possible as a plugin; none is planned. Vestige's science informed this design; its code is not used (AGPL, SQLite-first).

**FSRS-6 is checked against its reference** (theseus-3ht; built 2026-10-01, Part III A4 Item 26). The reference is the `fsrs` crate 6.6.2 (BSD-3-Clause), read, never copied. Theseus matches it on:
- the 21 defaults, digit by digit;
- the same-day step, whose factor is floored at 1 from Hard up, so a same-day Hard never lowers stability. The reference's model and optimizer do this; only its workload simulator floors from Good (theseus-b9l4 asks for a second source);
- the prior's stability and difficulty, clamped into their bounds before a review reads them;
- parameters clipped into the reference's bounds, as its `FSRS::new` does. A non-finite parameter is still an error.

The clipping is silent. When the parameters become configurable, the loader says which values it clipped (theseus-3fjv). The one deliberate departure is the same-day test, `t < 1` rather than a calendar day, since an agent has no day boundary. _(Since 2026-10-05, step 32a's wire-in; Part III Item 157: FSRS-6 runs under real nodes as the `+retention` arm. Retention is a projection folded from the memory pass's own rows, in WAL position order and at each row's time: `memory.labeled` is first sight, graded by durability; `memory.used` a use, graded by its outcome; `memory.label` a review, graded by the label (`useful` and `should_have` Easy, `wrong` and `stale` Again). Exposure without use is no review. The projection is one for every arm, built after serving and only once an arm or a search reads it. `schedule` returns `Option<Retention>`, since a science may keep none, and the trait gained `reads_retention()`. The arm ranks by `fused × (0.5 + 0.5 R)`, `R` at the turn's time, and a node with no retention keeps its fused score; the weight 0.5 is a starting point, named in the arm's digest `retention@<hex>`.)_

### 5.2 The memory pass

1. Jev `memory.v1` labels each new node: kind (preference · fact · decision · procedure · episode · transient · other), durability, `about` targets, trust.
2. Policy: transient and low-durability nodes stay heat-managed; secrets and external untrusted text never become durable without operator confirmation.
3. `gate()` assigns initial retention, merges by adding `same_entity` edges (the duplicate node remains; the edge routes recall to the canonical one), and flags contradictions with `contradicts` plus `supersedes` when the human is the source. "Drop" means starting retention at the floor so the node is cold from birth; it does not mean deletion.
4. Compaction `Summary` nodes take the same pass, so a summary can be durable while its range is not.
5. **Recursion exclusion:** `Judgment` and `Heartbeat` nodes, context manifests, and ledger records are never themselves candidates for the memory pass or for memory judgments. Only human and agent `Message`, `ToolResult`, `Task`, `Summary`, and `Synthesis` nodes are. Routine bookkeeping is deterministic, not judged. _(As built, 2026-10-04, step 31a; Part III Item 136: the pass runs after each turn on its own task, off the turn's path, and writes its frames only between turns (the owner's decision 10). Its labels come from a deterministic, table-driven labeler (kind, durability, volatile values, trust from DD5's `external`, and `about` from the index tender's one extractor), with `memory.v1` in shadow beside it. Eligible are user and assistant messages, tool results with text, and compaction summaries, which carry their range's trust; never a `Recall`. `gate()` reads `index.neighbours`: an operator's correction whose top neighbour reaches 0.75 supersedes it, however close; otherwise 0.92 or over is a duplicate (`same_entity`). No `contradicts` edge is written. Attribution writes a `memory.used` row per recalled item, with `attribution.v1` in shadow, and baseline's second version keeps only the newest of a `same_entity` group and drops the older side of a `supersedes`.)_

### 5.3 Recall

`BUILD_CONTEXT` gathers candidates from the exact indexes and the hybrid similarity index, adds `activate()` neighbours of the seeds, dedups by id, fans out one Jev relevance Noul per candidate in a single request, then lets the budgeter allocate by node kind under the role's weights. _(Since 2026-10-04, steps 32c and 32d; Part III Items 128 and 141: Jev's relevance pass is `rerank.v1`, one request per recall over the top twenty notes that passed every filter, as two per-item Nouls of ten (`helps`, `helps_more`). With memory live and the judge on it is live by default: the turn waits at most `[memory] rerank_wait_ms` (200) for Jev's order and packs again in that order through the same filters, or keeps recall's own order and records the answer `late`. A note the owner labeled wrong or stale never reaches Jev or the repack.)_ Every recall records what was shown so outcome labels can flow back to both Jev tuning and `schedule()`. _(Since 2026-10-04, 30b, in canary and live; Part III Item 112: what was shown is a `Recall` node of references, each a source's position and frozen range, rendered after the new message as testimony, with a `derived_from` edge to each source and a `recall.ran` row; a session's arm is fixed once, by a hash of the experiment and the session. The operator's labels (`memory.label`: wrong, stale, useful, should have) are the first outcome labels: a wrong or stale node leaves recall from the next turn on.)_ _(The Jev relevance step was built in shadow on 2026-10-04 as the `+rerank` arm, step 32c; Part III Item 128: after a shadow recall, `rerank.v1` asks one Noul per note over recall's top twenty that passed every filter (two questions of ten), reorders them by Jev's probability, and repacks them through the same filters, under its own 600 ms deadline and from the judge's day budget; its row says what `+rerank` would have admitted beside what recall did. It went live with 32d (Item 141).)_ _(Amended 2026-10-02, theseus-u55z, Part III Item 50: the exact and the hybrid indexes are the index tender's (§6). `index.query { text, k, as_of, … }` ranks by BM25, exact entities, and vectors, and fuses them by weighted reciprocal rank (vectors 6, the others 1; theseus-jz8), as of a WAL position, so a replay never sees its future. `theseus index search` asks it by hand; recall on the turn (30a) will ask it under its deadline.)_ _(Since 2026-10-04, 30a, in shadow; Part III Item 99: the turn asks it as its first loop's model call goes out, and reads the answer after the call, never past `[memory] recall_deadline_ms` (250). The baseline science's pipeline drops each candidate with its reason (`place`, `in_context`, `untrusted`, `recursion`, `threshold`, then `budget`), packs the rest under 1,500 tokens and 6 items, and the turn records a `recall.shadow` row of references only, never another session's text or the query; the model's request is unchanged byte for byte. `memory.search` runs the same pipeline and writes nothing. `[memory]` is off by default.)_ _(Since 2026-10-05, step 32b's wire-in; Part III Item 159: under the `+activation` arm, `activate()` runs before the pipeline over an adjacency projection folded from the record, never stored: neighbouring turns, tool pairs, EDGEs weighted by kind and route in code (a recall's edge weighs 0, and a `Recall` node is no one's neighbour, so exposure never spreads), and entities from the memory pass's labels. The spread is bounded by what is left of the index's deadline; a reached hit gains a reciprocal-rank term, and at most 20 nodes the index did not return join the candidates, never one the turn holds, with every filter, the place rule first, run on them.)_

### 5.4 Consolidation (what remains of dreaming)

Re-scoring offline is unnecessary; selection is recomputed live. Synthesis is kept, as a memory-tender job: cluster nodes the ledger shows are recalled together, have a cheap model propose one synthesis per cluster, have Jev citation-check it against its sources, store accepted ones as `Synthesis` nodes with `derived_from` edges, `trust: agent`, ~~and the most restrictive confidentiality label of their sources~~ and the place of the harness session they live in, which is private (labels went with the place rule, Part III Item 76). Shadow first, with a precise meaning: a shadow synthesis is *scored* (would the relevance judge have selected it; does an independent assessment rate it as supported and non-redundant) but its live *utility* cannot be shown while it is withheld. Promotion to live recall is a **bounded canary** measured on trajectory outcomes, exactly as question packs are (§3.10). No main-thread work, no unbounded replay. _(Built 2026-10-05, step 31b; Part III Item 160: clusters are node pairs admitted together in at least 3 distinct turns, shadow recall included, joined into components of 3 to 8; the session's own profile writes at most 120 words a cluster, every sentence citing its sources, under `[memory] synth_limit_usd_per_day` ($0.50); deterministic checks, then `citation.v1` in shadow, one Noul per sentence and cited source, any under 0.5 rejecting; a kept synthesis is a `Synthesis` node in the harness session `memory.session`, written between turns; its shadow score is its best admitted source's. The nightly run is at `[memory] consolidate_hour` (4), in `SCHED_IDLE` after a PSI wait; `+synthesis` is the only arm that shows one, and only once checked. On the owner's daemon the writer is off (`synth_limit_usd_per_day = 0`) until theseus-8edz is fixed.)_

### 5.5a Memory science must earn its place

FSRS models human recall; agent context selection has a different objective, selecting what improves the current task. The transfer is a hypothesis. There is also a known self-reinforcing loop: selection strengthens retention, strength increases selection, and repetition is mistaken for usefulness. Being shown is not being useful. Theseus therefore ships and measures a **baseline first**: transcript tail + task graph + summaries + BM25/embedding retrieval + deterministic freshness and provenance rules. Graph spreading activation, FSRS-style retention, Jev relevance reranking, learned role weights, and synthesized memories are each **ablated independently** against that baseline at a fixed total budget that includes judgment cost. Metrics: task success, false completion, unnecessary continuation, stale or contradictory recall, disclosure violations, latency, total cost. Retrieval agreement alone is not a success metric. _(The first honest exam, 2026-10-04, 34b's wire-in; Part III Item 124: one scratch daemon per arm over the real recall pipeline, on GLM, 576 cells for $0.87. Held in, `baseline` passed 70 % [55, 85], `bm25` 42 % [25, 58] and `none` 15 % [3, 27]; `baseline − none` gained 55 points held in and 50 held out, and `baseline − bm25` 29 held in (a gain) and 14 held out (insufficient). With no canary data, recall and vectors both stay at the decision rule's third clause, off by default.)_

### 5.5 Namespaces, trust, forgetting

Namespaces are kinds in the ontology (§4.1a): `person:<discord_user>`, `guild:<id>`, `channel:<id>`, `topic:<id>`, `global`, and any kind added to the table later. Bindings declare read and write sets; personal preferences always write to the person namespace. Trust labels on every durable node; external-derived nodes are recalled with their label and never gate policy. Forgetting is always an append: FSRS decay by disuse lowers retention; `supersedes` chains leave the superseded node in place so backward reach still works; operator `forget` appends a `Suppression` node that excludes its target from every view, every index, and tiering rehydration, with a receipt. Payload erasure is the single exception, specified in §5.6.

### 5.6 Redaction (the exception to append-only)

Suppression hides a node from views; it does not remove information that has already spread into summaries, syntheses, tool-result copies, ledger snapshots, indexes, embeddings, backups, and previously assembled contexts. For secrets and other must-not-exist payloads, Theseus permits **payload erasure with preserved structure**: the node record keeps its id, kind, origin, timestamps, and edges; the payload is replaced by an erasure marker (not a plain hash, which leaks low-entropy values), and a `Redaction` node is appended with a receipt naming who ordered it and why. Redaction is **lineage-aware**: it walks `summarizes`, `derived_from`, `part_of`, and `same_entity` edges to find descendants that may carry the content, flags each for re-summarization or erasure, removes the vectors and index entries, invalidates cached contexts that included the node, and records which backup segments contain the original so a backup policy (retention window, or targeted rewrite) can act. Low retention is not low persistence; a secret that slipped past ingest policy is redacted, never merely cooled. _(2026-10-06, theseus-0lrr.6; Part III Item 201: the first erase built is the importer's, by tag: a tombstone record over each node (`Body::Erased`, its id, origin, author and time kept) and a receipt on each session, with the index's forget, which a rebuild keeps. No `Redaction` record exists yet and nothing rewrites a WAL frame, so the payloads stay in the WAL and in backups: an erase hides, it does not delete, until this section's in-place erasure and its invalidation of cached contexts are built.)_

## 6. Storage and tenders

Speed first, durability second, cost last.

```
theseus (core)                          tenders  (children of the daemon)
┌────────────────────────────┐          ┌────────────────────────────────────────────────────┐
│ in-memory arena            │  WAL     │ durability: WAL segments → S3, index rows → Dynamo │
│  typed node arena          │ ───────► │ tiering:    heat + retention → demote to stubs;    │
│  CSR edge columns per type │  local   │             rehydrate on reference / typing event  │
│  per-channel logs          │  SSD     │ index:      theseus-index, a binary of its own:    │
│  hot exact indexes         │ ◄─────── │             Nomic v1.5 + usearch HNSW + tantivy    │
│                            │  reads   │ memory:     consolidation, decay sweeps            │
└────────────────────────────┘          └────────────────────────────────────────────────────┘
```

_(Amended in v0.78, theseus-1nyg: the diagram drew every tender as a role of the core's own binary, `theseus --tender <role>`. The index tender, the first that runs, is a binary of its own, `theseus-index` (Part III Item 50). The durability, tiering, and memory tenders are not built yet.)_ _(Since 2026-10-04, step 15; Part III Item 108: the durability tender is built, as a task inside the daemon that reads the WAL through `theseus-follow`, not a child process, so the AWS session it signs in stays in the core (§3.5).)_

- **The logical graph is permanent; the resident graph is a bounded cache over durable history.** Nothing about append-only requires anything to stay in RAM. Resident memory scales with the active working set, not lifetime traffic.
- **Arena.** Nodes by monotonic id; edge storage as immutable sorted segments per edge type with an in-memory delta, compacted in the background (compressed-sparse-row columns are a benchmark candidate for cold segments, not a commitment); per-channel logs as segments in a shared append file with an allocation index. Cold nodes leave RAM entirely, metadata and adjacency included, represented only by their id in a compact presence filter, and rehydrate from the SSD index on demand. Memory targets (§9) cover the whole process tree including tenders and loaded embedding weights.
- **Storage kernel (built in M1; Part III A1).** Two layers, one contract. The **WAL** is the truth: segment files of checksummed frames, one frame per append, one `fdatasync` per frame; a frame holding several records is atomic, which is how `settle(completion, continuation)` commits both or neither. Recovery checks the WAL from the frame after the index's checkpoint to its end, where the next position and any torn frame are, and truncates a torn tail in the last segment. _(Since 2026-10-03, Part III Items 80 and 92: a bad frame in the last segment is refused, with nothing cut, when its first position is at or before the larger of the index's checkpoint, which claims only synced positions, and the newest synced mark of a whole frame after it. Each frame carries that mark, the last position whose sync had returned when the frame was written: the marked layout (magic `THWM`) puts it in the body's first 8 bytes under a crc that covers the magic, and the unmarked frames before it read as they did. Only rot in the log's last batch, until a checkpoint reaches it, or in an unmarked frame that no marked frame follows, is still cut, as a torn write is, and a cut says how far the log was known synced.)_ When the log there is not what the index says, it checks every segment, as it always did, and refuses to guess at corruption anywhere else. The history before the checkpoint is checked after serving, by a thread using about 5 % of a core. A corrupt frame there is an error in the log, a `store.corrupt` ledger row, and a refusal of that frame's reads, since record reads check no checksum of their own. A checkpoint is taken with no append between its WAL write and its index write, so the position it claims is synced and indexed. Every record carries its kind and its kind's schema number. `MANIFEST.json` (format 3) names the newest schema written for each kind, and a build that finds one newer than it reads, or a kind it does not know, refuses to open the store and says to install the newer build. The manifest is marked, durably and before the record, the first time a build appends a record newer than it says, so a start never writes it. _(Since F4a, theseus-qa0 and theseus-8ni; Part III A3c.)_ _Superseded 2026-10-03 by Tier 7.9 as amended (Part III Item 82): the store has one format number, `MANIFEST_FORMAT`, bumped by any step that adds a field to a stored record or changes the encoding; a record header's schema is frozen at 0, and the per-kind marks are gone. A build refuses a store of a newer format. An older store moves to this build's format at the writer's first frame, durably and once, and a store only read keeps its format. Format 5 is a wake's repeat (Item 84), and 6 the WAL's synced mark (Item 92). Since then, by `MANIFEST_FORMAT`'s own list: 7 is the ontology's `onto:*` records and a compilation's memberships and guidance (21b, Item 100); 8 memory's `Recall` node and a compilation's budget (30b, Item 112); 9 a task's arrangement (27, Item 113); 10 a node whose origin is MCP, a server's prompt as a turn's input (36c, Item 115); 11 a hand's cancel verified by ECS and the hour's alert mark (step 40's second part, Item 116); 12 the `TASK` record kind (39a, Item 125); 13 memory's `Summary` node and a compilation's recall id (30c, Item 131); 14 a check task's basis and its claim on its arrangement (28a, Item 132); 15 a session's `routed` (25e, Item 139); 16 a task's `origin.by_model` (Item 146); 17 a message's kept file (Item 155); 18 memory's `Synthesis` node (31b, Item 160); 19 a task's claim, its lease (39b, Item 163); 20 a `proc.run` batch's steps (Item 175); 21 a session's routed base, `routed.from`, the profile routing first moved it from (25e's gaps, Item 181); 22 a compilation's `situation` (35a, Item 183); and 23 an imported session: a session's `imported`, a node's `import` origin, and the `Imported`, `ImportedSummary` and `Erased` node bodies (Item 201). The operator's store was at 6 until install #1 moved it to 14 (2026-10-04, 14:09); install #2 moved it to 16 (20:00), install #3 left it there (21:22), install #4 moved it to 20 (2026-10-05, 13:07), install #5 to 22 (16:48) and install #6 to 23 (2026-10-06, 10:12), each move one way, after the install's backup._ The **index** is a rebuildable projection of the WAL in a pure-Rust embedded store, `redb` (the M1 benchmark's pick; `fjall`, the other candidate, was removed in batch C, theseus-0g4): position → location, (kind, key) → latest position, (kind, position) for per-kind scans, and a checkpoint position. _(Since 2026-10-03, Part III Item 91: and the index's shape, nine tables more, marked whole by its checkpoint under `index.shape.3`: counts per kind, key, scope and term; a clock per kind with each minute's first position; the tags of ledger rows, nodes and actions; and each key's birth. So a page, a count, or a window of time is read without walking history. An index another build wrote has the shape rebuilt after serving, ending in a durable checkpoint of its own.)_ Index writes are non-durable; a checkpoint makes them durable; open replays the WAL past the checkpoint, so deleting the index entirely loses nothing. _(Since Tier 7.7, Item 82, an open itself makes nothing durable: the tables' creation is a non-durable commit and a replay takes no checkpoint; the replayed tail counts toward the next periodic one.)_ The arena remains the cache over this, never a second source of truth. Pure Rust keeps the static musl build honest. _(Since S2, theseus-vni9 and theseus-xprd, Part III Item 52: one writer thread, `store-writer`, owns every append. It writes every frame queued back to back, syncs once for the batch, indexes the batch in one redb transaction, and then answers each caller, who waits without holding a runtime worker (`theseus_store::blocking`). A frame is still atomic, and one sync covers a batch. A write cut short is cut back off, and a new segment's name, and a new log's directory, are synced before any frame in the segment is reported durable, so a segment is as durable as its frames.)_ _(Since 2026-10-05, theseus-ljgm and theseus-c67g; Part III Item 176: a sync that fails cuts the last segment back to the end of the last frame a good sync covered, syncs the cut, and rolls the writer back, so the next frame takes the first cut position and a frame its caller was told failed never comes back after a restart; `synced` is untouched, and syncs run one at a time. The log then takes frames again. It goes broken until a restart only when the cut or its sync fails, or when a sync fails before any good one over frames the open found past the last position known synced. A follower checks, at each read, the frame its cursor stands after, so a cut behind it that was written again is a rewind, for the index's tender and the durability tender alike. And an open that finds its last segment syncs the log's directory with its first frame, once, so a segment a dead process created is as durable as its frames too: one more sync wait before serving, which theseus-3q29 would skip when a checkpoint or mark vouches for the segment.)_ _(Since 2026-10-06, theseus-3q29; Part III Item 196: it does. The open keeps the found segment's first position, and when the index's checkpoint, or the open's synced position (the larger of the checkpoint and the newest mark), is at or past it, that segment's name was synced by the sync that covered the position (`Wal::sync` moves the mark only after the pending directory fsyncs land), so the directory's sync is left out; an empty found segment is never vouched for. A store an older format wrote (`behind`) syncs it once, with its first frame, since its marks may be a pre-c67g build's. A restart, clean or after a SIGKILL, no longer fsyncs `store/wal`. One hole is open: the manifest moves before the first frame's sync (theseus-xva3).)_
  - **An index that is not a database** is moved aside, never deleted, to `index.redb.bad-<unix ms>`, and the
    open builds a new index from the whole WAL, as `restore` does (theseus-0b8; built 2026-10-01, Part III A4
    Item 17). A kill inside the store's very first open, while redb writes the file's header, leaves such a
    file. The open says so in a WARN, in the startup store phase's `index_moved_aside`, and in a
    `store.index_replaced` ledger row once serving. redb's error kind tells this case from the others (never
    its text), and the move happens under the file's lock:
    - a file another process holds is that process's: the open waits for it, then refuses, as it always did;
    - a redb database that fails some other way (a newer format, a bad commit slot, a file cut short) is
      refused and left where it is.
  - **A corrupt record degrades what reads it, and nothing else** (Review 2's R4, theseus-15g; built
    2026-10-02, Part III Item 53). A read of a record in a frame the history check found corrupt is refused.
    A list read (the driver's runnable executions, startup's reads by state, the session list, a transcript,
    the ledger's tail) skips it instead of failing whole, logs it once, and counts it, and health shows the
    count with its repair. No check loosens: a session's hold and an execution's budget are read by key,
    which is still refused. **The repair** is `theseusd restore --repair --from <a copy>`: each frame of the
    WAL that doesn't check is taken whole from the same offset of a copy of the store, which must hold it
    whole (any backup taken after the frame was written does, since the WAL only grows), and every other byte
    is the store's own, so nothing written after the backup is lost. The store it repaired is kept aside.
- **The turn lock and eventual durability.** "Speed first" is preserved by *where* the time goes, not by skipping durability. Within a channel exactly one turn advances at a time; that lock is held only while the core is doing local work. A turn is mostly waiting: a Messages API call is seconds, a shell job is seconds to hours, a judge call is hundreds of milliseconds, a human is minutes. At every such offload boundary the turn releases the lock and the core spends the surrendered time on **asynchronous durability work**: sealing the current WAL segment and handing it to the durability tender, taking checkpoints, flushing index updates, compacting edge segments, running the memory pass, uploading. The floor remains unchanged (intent is fsynced locally before dispatch); what changes is the off-node recovery point, which becomes **eventual: 5–60 s** rather than 1–2 minutes, achieved for free from time the loop was not using anyway. The tender scheduler prioritizes by staleness: the oldest unshipped committed record bounds the current recovery-point exposure, and that number is exported as a metric and alarmed on.
- **WAL.** Every record appends to the local SSD and is fsynced on a short group-commit interval before the turn proceeds. _(As built, S2, Part III Item 52: every frame goes to the store's one writer thread, which writes every frame queued, fsyncs once for all of them, indexes them, and answers each, so the turn proceeds once its frame is durable and indexed. The periodic checkpoint runs on the writer after its answers, so no append pays it; theseus-avvb.)_ Records carry a length prefix and checksum; a torn tail is truncated on recovery. Periodic **checkpoints** snapshot the arena so recovery is checkpoint plus tail, not full-history replay. Schema versions are stamped on every record, by kind. Old layouts are read in place, through serde's defaults or a reader such as `Execution::from_stored`. A layout that needs rewriting will get a forward-only transform run by a tender; none has needed one yet. A stored float reads back as the value it was written from (serde_json's `float_roundtrip`; theseus-k52m, Part III Item 54), so a record decodes and encodes again byte for byte. Disk-full is handled by refusing new turns with a clear message while tenders continue to drain (designed; what is built is the job floor under "Free space" below). The SSD is a persistent volume that survives instance death and is encrypted at rest by the platform (EBS encryption, LUKS on a desktop), not by Theseus.
- **Tenders** consume the WAL and answer rehydration over a local socket; they never touch the arena directly. Durability ships to S3 and DynamoDB when configured, with a 5–60 s target measured as "age of the oldest unshipped committed record." _(As built, step 15, 2026-10-04; Part III Item 108: with `[aws.accounts.<id>] durability = true`, sealed segments go whole (a multipart upload past 8 MiB), the open segment's new whole frames as byte-range tails, and new blobs, each object with its SHA-256 checksum, under `durability/<deployment>/` in the foundation's bucket; index rows, among them each keyed record's latest position, go to the `theseus-durability` table. The session `theseus-durability` is narrowed to that prefix and those rows, and can delete nothing. A durable cursor outside the store makes a restart ask S3 before it sends. The tender wakes on each WAL change, settles 5 s, and ships, with a 60 s backstop. On the account it caught up 4.0 s after a start and shipped a turn 11.2 s after it. The age is health's `oldest_unshipped` and the histogram `theseus.durability.lag_ms`.)_ Tiering demotes payloads by heat and retention (stub stays in the arena, payload on SSD and S3; nothing is removed from the graph), with a Jev backup opinion for lower heat bands; rehydration misses are logged. _(Since 2026-10-05, step 33's thin slice; Part III Item 162: tiering's first cut is in the core, not a tender. A transcript read gives payload stubs, each holding its record's id, kind, origin, turn and summary range from a peek that skips the payload, and decoding its node at a reader's first touch; a bounded heat cache of decoded nodes by WAL position, `[memory] node_cache_mb` (64 MB of records), serves every reader across turns and evicts `decay_sweep`'s hints first, then the coldest. A failed rehydration is logged and counted in health's `node cache` line. On a 2,000-node session each turn after the first decodes its 2 new nodes, where every turn had decoded all of them. Demotion to SSD and S3 and the Jev opinion wait for a store that needs them.)_ Index owns embeddings: 768-d stored, 256-d indexed, 768-d rerank; usearch memory-mapped from SSD; tantivy BM25; reciprocal-rank fusion then Jev relevance; asynchronous after commit; long nodes chunked with `part_of`. Memory runs consolidation and decay sweeps. _(Since 2026-10-04, the Linux survey's card 4, theseus-tood; Part III Item 151: background passes wait while the machine is busy. Between two chunks (never before the first), the WAL's history check, the index's terms and shape builds, the nightly learning run and the index tender's embedding thread each look at PSI (`some avg10` of `/proc/pressure/cpu` and `/io`) and wait while CPU pressure is 20 % or more or IO 10 %, the gate's own settle lines, looking each second for at most 10 s a chunk, so a machine that stays busy still gets its passes done. Only the nightly learning run's thread takes SCHED_IDLE; the embedding thread serves queries (it loads the model for a waiting one, and rayon's pool takes its policy), so it keeps nice 19. `theseusd check` says when the state's disk schedules with `none`, which ignores I/O priorities, so the tender's idle I/O class changes nothing there.)_
  - **The index tender** (theseus-u55z; built 2026-10-02, Part III Item 50) is the first tender that runs. It
    is `theseus-index`, a binary of its own installed beside `theseusd`, not a role of the daemon's (as the
    diagram above had it until v0.78), so the daemon links neither tantivy nor candle. The socket daemon starts it 2 s
    after its socket answers, never on the start path nor beside the start's aftermath, as `theseus-index
    serve --store <state>/store --index <state>/index --parent <its pid>`, at nice 10 and the idle I/O class,
    with an environment that holds no secret. It follows the WAL read-only into BM25, exact entities, and,
    with the Nomic weights in `[index] weights_dir`, vectors, and answers on `<state>/index/sock` (0600), the
    core its only client. One tender per index directory: it takes `<state>/index/LOCK`, and a second exits
    3. It exits when its daemon does (a pidfd), so none outlives a crash holding the lock, and a restart in
    place takes the running tender over. The core restarts it whenever it exits: after 1 s, the wait doubling
    to 60 s while it keeps failing, and 1 s again after a minute's run; a kept one whose `[index]` settings
    changed is restarted. A stop sends it SIGTERM and never waits. Each start, take-over, and exit is an
    `index.tender` row. `[index]`: `enabled` (true), `weights_dir`, `threads` (1), `idle_unload_mins` (10).
- **Completion spool.** A directory on the same SSD as the WAL where job wrappers write results before attempting delivery. Startup drains it before accepting events; the heartbeat reconciler reads it every minute. It is the reason a harness restart never loses a finished job. _(Since 2026-10-04, the Linux survey's card 1, theseus-yxiv; Part III Item 151: a wrapper writes its result with one sync, not two: the tmp is written, `flock`ed and fsynced, then renamed, with no directory sync, since on ext4 (`data=ordered`) and xfs a new file's fsync commits the journal transaction that made its name, so a crash can lose only the rename. The start finishes such a rename: it takes a `.json.tmp` whose writer's lock is free, which parses whole and names its own job, renames it into place, and drains it with the rest; a torn tmp, a live writer's and one naming another job stay as they are. A completion whose action is already settled writes nothing at the start. The daemon hears of a job one fdatasync sooner: 6.6 ms at L0 and 6.7 ms at L1 on the build machine.)_
- **Free space, and the spool's sweep** (theseus-102, theseus-2ij; built 2026-10-01, Part III A4 Item 25). On a
  full disk every WAL append fails, including the rows that would say so. Health reports the free space of the
  filesystem that holds the state dir (`statvfs`), and calls it low under `[server] disk_warn_mb` (5 GB). Under
  `[server] disk_floor_mb` (1 GB) a job is refused, with its reason as the call's result and a `job.refused` row.
  ~~The floor leaves room for what already runs to finish: eight turns' jobs at their 64 MiB output cap, a WAL
  segment, and the index's checkpoint.~~ Below the floor every running job is stopped too, with its reason
  ("stopped by the disk floor (812 MB free, below the floor of 1,024 MB)") and a `job.stopped_below_floor` row,
  and its conversation wakes to read it (theseus-ht82; built 2026-10-02, Part III Item 54). A job's captured
  output is capped, but the files it writes itself are not, so headroom alone doesn't keep a runaway job from
  filling the disk, and at L0 nothing names the writer. M4's per-job quota will (theseus-dszu). Health sees only the filesystem Linux reports. Under WSL that filesystem is
  a file on the Windows drive, which can fill first. _(Since 2026-10-05, theseus-f337, v1.1's V10; Part III Item
  177: a crossing of either line, or the way back, writes one `disk.low`, `disk.below_floor` or
  `disk.ok` row and tells the operator once (a notice the Discord courier posts on the operator's channel), checked
  on the heartbeat's 60 s beat after serving. Falling counts at once; rising only past a margin of a twentieth of the
  line, at least 32 MB, so a wobble says nothing; a first read that is fine says nothing. Under WSL the crossings
  read the same virtual disk, so they cannot yet warn of a full Windows drive: theseus-83y0.)_ A tender after serving, at start and then hourly, sweeps the
  raw job output no result will absorb, by the store's state, and keeps what a turn may still read. Each file's
  fate:
  - removed: `absorbed` (its result is written), `ended` (its execution ended), `unknown` (no action owns it, and
    it is a day old);
  - kept: `running`, `pending`, `young`, `unread`.

  Health's `spool` shows the last sweep. A `spool.swept` row is written when a sweep removed a file, or when a
  start's first sweep found one.
- **Restore.** Rebuilding a node from S3 segments plus the DynamoDB index is a first-class, tested path from the first release (~~`theseus restore --from s3://…`~~ `theseusd restore --from s3://…`, built 2026-10-04), because S3 is presented as disk-failure recovery. Periodic automated drills remain deferred. _(As built 2026-10-04, step 16; Part III Item 123: the restore signs in a session of its own that only reads, minted for the URL's deployment; the deployment's rows decide what is current, never a listing; a segment comes from its sealed object, or from its tails stitched only where each begins where the last ended; every object is checked against its row's length and SHA-256, and one that differs refuses the whole restore; a gap stops the restore there, which restores the prefix before it and says so. When the config's deployment is the one restored, the tender's cursor and the shipped blobs are seeded, so the next start ships only the restore's own row. It refuses while a daemon answers on the socket.)_
- **A local restore is durable before it says so** (theseus-ez3; built 2026-10-01, Part III A4 Item 19).
  `theseusd restore --from <wal or store dir>` syncs each copied segment and blob before it opens them; syncs the
  staging store's `wal/` and `blobs/`, and the staging store once its open has written the manifest and the index;
  and syncs the state dir after moving an occupied store aside and again after the restored store takes its name.
  Only then does it print "restored", so a power loss after that loses none of it.
- **Embedding weights** are a versioned artifact fetched to the SSD on first run and pinned by hash, distributed separately from the static executable.
- **Desktop mode.** Same binary; tenders write to a local directory and SQLite; every AWS-side dependency is absent without error.

### 6.1 Graph state

_Written 2026-09-26 in answer to the owner's questions before M2. The durable layer and the structural index are conclusions implied by §4.4b and the kinds reserved in M1; the arena layout is a hypothesis to be benchmarked in M3 and is written here so the benchmark has something to confirm or overturn._

**No graph database.** The WAL is the truth for the graph as for everything else, and the graph is a set of projections over it. An embedded graph store (CozoDB, IndraDB) or SQLite would add a second durability story and a second recovery path beside the one proven in M1; a graph query language buys nothing when every traversal needed is "neighbours of X by edge type" or "lineage of session Y".

**Durable layer.** Nodes, edges, and compilations are ordinary WAL records (kinds `NODE`, `EDGE`, `COMPILATION`, `JUDGMENT`, reserved in M1). A node is keyed by its id, a UUIDv7 so ids sort by creation time. An edge is keyed `type ‖ from ‖ to`; its payload carries rank and weight. Both are append-only: a redaction or re-ranking is a new record carrying a tombstone or superseding flag, and `latest_by_key` gives current state. A `Compilation` record holds the manifest and the ordered `includes` list for fast rendering; the reverse `includes` edges, needed by redaction to ask "which compilations include this node", are written in the same frame. _(Amended 2026-10-01, theseus-n4m, step 12a, Part III Item 38: no reverse `includes` edges are written. Every `includes` today is a run of its own session's nodes, which the compilation record holds, so `node.reach` derives "which compilations include this node" with one read of the session, and "which loops' contexts held it" from each reply's `compilation_id`: a loop held it when its compilation's `includes` holds it, or left it in the tail. About 300 reverse edges per recompile would have repeated the record in every recompile's frame. The first route that admits a node from outside its session (M6's recall, M7's borrowing) writes its own reverse entries, and `node.reach` reads them then (P0's rule 3). The first edge written is `derived_from`, keyed `derived_from ‖ <copy> ‖ <source>` and scoped `in:<source>`, in the frame that writes the copy: a task's report relayed into its parent, from the task's last message, and a task's brief, from the parent's reply that holds the `task.create` call. EDGE stays at schema 1, which every manifest marks already, so the first edge rewrites none.)_ A turn that creates a node and a compilation with three hundred edges appends one frame; record count inside a frame costs microseconds and the frame pays the disk's one fsync.

**Four projections, all rebuildable.**

| Projection | Store | Answers | Lag behind commit |
|---|---|---|---|
| Structural index | redb (§6 storage kernel) | position → location; latest by key; per-kind, per-session, and adjacency range scans (`type ‖ from ‖ to`, plus a reverse column `type ‖ to ‖ from` for `includes`, `mentions`, and the edges §5.6's lineage walk follows: `derived_from`, `summarizes`, `part_of`, `same_entity`; Appendix F) _(as built at 12a: the reverse column is the scope `in:<to>` on the scope index the store already keeps, one range for every edge kind into a node, in WAL order; no new table, and no change of layout)_ | none (same call, non-durable until checkpoint) |
| Resident arena | process memory | the working set: metadata columns, payloads, adjacency segments | none |
| Retrieval index | Index tender: usearch (256-d indexed, 768-d stored and reranked) + tantivy BM25, memory-mapped from SSD | "things about E" by paraphrase and by literal mention; reciprocal-rank fusion, then Jev relevance | seconds |
| Entity edges | Memory tender writing `mentions` and related edges back through the WAL | "things about E" as one adjacency scan once E is an entity node | seconds to minutes |

Live traffic is arena first, structural index on a miss, and the WAL only as a byte source at a known offset. Nothing populates the whole graph into memory at startup; rehydration is lazy and per node, which is why a thousand waiting sessions cost a record each and no RAM. Writes go to WAL, index, and arena in one call, and only the WAL fsync is on the critical path.

**Resident arena.** A struct-of-arrays arena with a dense process-local handle per node and edges as sorted columns per edge type:

```rust
type Handle = u32;                          // dense, process-local, never persisted

struct Arena {
    ids:      Vec<NodeId>,                  // handle → uuid v7
    by_id:    HashMap<NodeId, Handle>,
    kind:     Vec<NodeKind>,
    position: Vec<u64>,                     // WAL position of the latest record
    loc:      Vec<RecordLocation>,          // where the payload lives
    heat:     Vec<Heat>,                    // last-touch tick + count, for eviction
    payload:  Vec<Option<Arc<Bytes>>>,      // None = stub (evicted)
    free:     Vec<Handle>,
    edges:    [EdgeColumn; EdgeType::COUNT],
}

struct EdgeColumn {
    sealed: Vec<Arc<Segment>>,              // immutable, sorted by (from, rank, to)
    delta:  Vec<(Handle, Handle, u32)>,     // unsorted appends since last compaction
    rev:    Vec<Arc<Segment>>,              // sorted by (to, from); only for types that need it
}
```

Handles index every column directly, so a node's metadata is a few contiguous reads with no pointers or lifetimes; handles are never persisted and the uuid is the durable identity. Columns keep traversals cache-friendly and let a payload be evicted without disturbing the row. A neighbours query binary-searches each sealed segment and scans the small delta; compaction merges the delta into a new segment during turn-lock gaps, and readers hold `Arc<Segment>` so they never block the writer (one writer, the turn holding the lock; `arc-swap` on the segment lists, a short `RwLock` around the delta). An evicted node keeps its row and drops the payload; a node never touched in this process has no handle and is answered by the index until first touch. Not chosen: `petgraph` as the store (its value is algorithms; a throwaway view over a subgraph is fine for those), per-node `Vec<Edge>` lists (fragment, defeat eviction), and CSR in the first cut (the natural format for a sealed cold segment, and still a benchmark candidate). The arena sits behind a trait so the segment format can change under a benchmark without touching the compiler.

**What does not scale, and where it is handled.** WAL bytes grow forever by design; compaction of live-latest records into fresh segments and shipping sealed segments off-node are M5 Tenders, and until then `max_total_bytes` refuses appends rather than filling the disk. Index rebuild from scratch is linear in WAL size and is a recovery path, never a startup path. Edge payloads stay tiny (a large graph is millions of forty-byte edges, a WAL of a few hundred megabytes); node payloads carry the bulk and are what tiering demotes.

**Durability boundary, stated plainly:**

| Failure | Guarantee |
|---|---|
| Harness process crash, SSD intact | Committed intent, results, and pending continuations recover exactly (WAL + spool) |
| A kill inside the store's first open, the index half written | The index is moved aside and built again from the WAL; nothing is lost (theseus-0b8) |
| Node restart, persistent disk intact | Same, plus job reconciliation; uncertainty reported where execution evidence is unavailable |
| SSD loss | Recover to the backed-up prefix with the stated 5–60 s recovery-point loss; actions in the gap may have happened externally without an intent record, so restore runs reconciliation of surviving external work with a conservative policy near the gap, and never reuses action identities after rollback |
| External side effect without recoverable evidence | Report uncertainty; never invent success, never blindly repeat |

The DynamoDB index is rebuildable from S3 segments and is never a source of truth whose consistency recovery depends on. _(As built, step 16: the restore reads the rows first, and they decide which objects are current, each object checked against its row; a segment with no row is a gap. The reconciliation near the gap and the redaction tombstones are not part of it.)_ **Redaction receipts** distinguish completed local erasure, pending backup expiry or rewrite, and copies outside Theseus's control (already sent to Discord, a provider, or another service); restore applies redaction tombstones before restored content becomes visible. **Control-plane tampering:** with L0 as default, code running under the operator's credentials could write the WAL, policy store, or spool. The runtime and its storage can run under a separate OS identity from L0 jobs (a dedicated `theseus` user owning the WAL, store, spool, and policy files, with L0 jobs running as the operator). This is an option, not a default, and the installer and documentation recommend it in strong terms: without it, L0 offers no protection of the record against the code it runs.

## 7. Shells

One tool family, many backends. Policy first determines the set of classes an execution's authority permits; Jev `shell.v1` then picks among that permitted set. Jev never widens the security boundary, only chooses within it. The model never names an instance.

```
shell.run {cmd, cwd?, env?, timeout_s?, workspace?, class_hint?}   shell.open/send/close (persistent PTY)
workspace.{create, attach_repo, snapshot, list}
```

| Class | Where | Speed | Cost | Durable access | Isolation | Default for |
|---|---|---|---|---|---|---|
| **L0 host** | the node, its SSD | ms | included | full: source trees, SSH agent, caches, instance role | none; `privileged` tag | coding in known repos |
| **L1 native sandbox** | the node; Linux namespaces + cgroups + overlayfs, no daemon; contract below | ~100 ms | small | read-only binds of chosen trees, scratch overlay | moderate, by contract | code the agent just wrote; package installs |
| **A1 Fargate** | ECS task per job, EFS or S3 workspace | 20–60 s | per second, zero idle | EFS durable | strong | purpose-built one-offs; long batch |
| **A2 Lambda** | container image | ~1 s warm | per invocation, 15-min cap | none durable | strong | short stateless jobs |
| **A3 SSM** | tagged fleet | seconds | fleet's | the host's | the host's | operating a specific host |
| **A4 dev box** | named EC2 | seconds | its own | full, curated | the box's | curated environments |

**L1 contract** (what "sandbox" means here, so it is not called strong by assertion): user, pid, mount, uts, ipc, and **net** namespaces; no network by default, with an explicit per-job egress allowlist and **no access to the instance metadata service or to localhost services**, including Theseus's own MCP server and web UI; capabilities dropped to none; a default seccomp profile; no device nodes beyond null/zero/random; masked `/proc` and `/sys`; cgroup limits on CPU, memory, pids, and disk with output size caps; the whole process tree killed on timeout or cancel. Anything the contract does not grant is denied. L0 grants everything the operator's user can do, and the spec says so plainly. _(As built since 2026-10-03, the sandbox trims, Part III Item 77: no cgroup. A job's limits are `RLIMIT_NPROC` (its processes), `RLIMIT_FSIZE` (any one file) and the scratch caps, with no memory limit, as at L0; a root daemon's L1 job is refused, since Linux exempts root from `RLIMIT_NPROC`.)_

_(As built 2026-10-02, step 17b, theseus-7ve.1; Part III Item 58.)_ A `proc.run` runs in L1 when the model asks (`sandbox: true`), when `[sandbox] l1_argv` names its program, or when `[sandbox] default = "l1"`; nothing turns L1 back to L0. An L1 job runs at `notify` (the owner, 2026-10-02 at 09:39): its view hides Theseus's floor, the approve list's paths, the daemon's socket, and cargo's credentials files, so the floor and the approve lists have nothing to guard, and a program's broker grant reaches it at its launch, as at L0, at the stricter of the two postures (theseus-w5op, Part III Item 71); a session holding external text still holds it (the exemption the owner chose for a job with no egress and no secret was 20a's, which was dropped, and is not built: theseus-oaf9). The operator's own word about `proc.run` still reaches an L1 call: its `[policy.tools]` line saying `approve`, or a "should have asked" tightening, makes the call wait, still in L1. A looser line never makes it quieter than `notify`, and the inherited `enforcement` never makes it wait (the owner, 2026-10-02 at 16:03; theseus-jfs6, Part III Item 59). The class is in the gate record and in the proposal a confirm binds, so an approved L1 call runs in L1. A job that can't start in L1 fails with the reason and never runs at L0. Its limits: `pids` (`RLIMIT_NPROC`~~, and `pids.max` with a delegated cgroup~~), ~~`memory_mb` (only with a delegated cgroup: the daemon's own systemd service with `Delegate=yes`, judged by systemd's own answer),~~ `scratch_mb` for scratch, `/tmp`, and HOME, and `output_mb` for any one file. What it writes to the workspace goes to scratch and is discarded; its result lists it. ~~The first L1 job turns job limits on in the unit's cgroup, so a unit that keeps its jobs across a stop (`KillMode=process`) needs `ExecStopPost=-theseusd cgroup-release`, which turns them off again, or its next daemon can't start while an old job runs; the installer writes the hook, and the daemon delegates only under a unit that has it.~~ (The cgroup, its limits and its hook went with the sandbox trims, Part III Item 77.) Egress was built in 18c (Part III Item 62, below); credentials are grants at launch (Item 71), after 18d's run-time socket was built and removed (Item 65). Cancellation per backend is built (step 18a, Part III Item 60; §3.16): an L1 job stops through its init, verified by its pid namespace, ~~or by `cgroup.kill` where it has its delegated cgroup, verified by the cgroup~~ (since Item 77, the pid namespace alone).

_(As built 2026-10-02, step 18c, theseus-7ve.4; Part III Item 62.)_ **Egress.** An L1 job has no network unless a list names hosts. `[sandbox] egress` (empty by default) is the operator's list: each entry `host:port`, a `*` in the host matching any run of characters (`*.crates.io:443` is every name under crates.io, not crates.io itself), the port exact. A call may name more, `proc.run { sandbox: { egress: ["pypi.org:443"] } }`, which also asks for L1; a host beyond the operator's list makes the call wait, as a path outside the roots does, and its approval lets that job reach those hosts and no other: the job's whole list is in its proposal, so the digest a confirm binds covers it. A job with a list gets a listener on 127.0.0.1:3128 inside its network namespace, handed to its wrapper, and `HTTPS_PROXY`, `HTTP_PROXY`, and `ALL_PROXY` naming it; the wrapper serves `CONNECT` there from the host's namespace. Each `CONNECT` is matched against the list, then resolved by DD5's public-only rule (a name any of whose addresses is loopback, private, link-local, the metadata service's, or otherwise not public is refused), then connected and copied both ways; a refusal is a 403 whose body says why. Plain `http://` forwarding is not offered, and a program that ignores the variables has no route. The proxy stops when the job ends, a stop or a deadline included, and records every tunnel it opened. A job with no list gets no listener and no proxy. **What it brings back is outside text:** a result whose job connected out is marked external, so its session holds it (§3.9), the reason naming the egress ("proc.run's egress to api.github.com:443"), and its node is untrusted, its readers still the owner's; a job that connected nowhere holds nothing. _(Since the sandbox trims, 2026-10-03, Part III Item 77: only a host beyond `[sandbox] egress`, which an approval of the call's own list let the job reach, makes its result outside text, and the reason names those hosts alone; a job that reached only listed hosts holds nothing. No node has a label since Item 76.)_ Every host on the list is a way out for anything the job can read. Recognizing request shapes would need TLS interception, which is dropped for v1 (Item 69); the list is M4's control.

**The consequence boundary** (M4; §3.9 Consequences). _Superseded in part, 2026-09-28: the consequence kinds are gone (§3.9; Part III A3b), so the boundary has no consequence to name. What stands is the default-safe environment: an L1 job starts with no ambient credentials and reaches the network only through its allowlist. Whether a credential request or a recognized request shape should notify or wait is held for the owner, with M4._ The design as it stood, where L1 is where consequences stop depending on spelling:
- **Credential brokering.** A job starts with no ambient credentials. To push, publish, or post it must ask Theseus for a scoped credential, and that request is the consequence, judged by the gate.
- **Egress recognition.** Allowlisted egress passes a local proxy that recognizes request shapes: a git receive-pack, a PR merge, a registry publish. So `./deploy.sh` is caught when it pushes, however the command was written.

_(As built, 2026-10-03: credential brokering under L1 is the broker's grant at the job's launch, judged at the gate before the job starts, at decision 15's stricter posture (Part III Item 71); 18d's run-time requests (Item 65) were built and removed. Egress recognition stays filed: it would need TLS interception, which is dropped for v1 (Item 69), and the list of hosts (Item 62) is the control.)_

L0 has neither: its job environment keeps `HOME`, so the operator's credential helpers are ambient. Under L0 the gate is all there is: `proc.run`'s posture, the operator's lists, and the floor (§3.9). (Until 2026-09-28 this read "the argv rules and `opaque` are the whole of detection"; both were removed.)

**Default (decided 2026-09-25).** Both L0 and L1 ship in the first useful agent. L0 is the early operator default; it is explicitly provisional and expected to be revisited once L1 has run real work for a while. Roles and Jev may steer a job to L1 within policy at any time; the default only decides what happens when nothing else has an opinion.

Every class launches through the **job wrapper** (§3.16): detached from the harness, result spooled before delivery, cancellable by correlation id. `shell.run` for a fast command still returns synchronously to the tool loop, but the record and the spool exist from the first millisecond, so a harness restart mid-command finds the result waiting rather than lost.

Workspaces point at existing directories on the node by default (the operator's real checkouts); cloning is an explicit attach for A1/A2. Snapshots everywhere. Credentials: L0 has the operator's agent and role; L1 gets nothing unless policy grants short-lived STS tokens or agent forwarding per job; AWS classes use scoped task roles. _(As built 2026-10-03: an L1 job gets what the broker grants its program, at its launch, as an L0 job does; Part III Item 71. An AWS session for an L1 job is to be such a grant, roadmap row 34.)_ Persistent PTY sessions are `Resource` nodes owned by a conversation, reaped by an idle timeout the role may extend. A1–A4 light up only when AWS credentials exist.

## 8. Verification: the simulator and the replay harness

Two test targets are first-class from the first commit.

**Replay harness.** The ledger records every Jev call's state and answers, every context manifest, and every tool call. Replay re-runs recorded states through a candidate question pack, memory-science parameter set, or shell mapping and reports how decisions would have changed against the recorded outcome labels. It is how packs get promoted (§3.10) and how a bad judgment is reproduced from production without touching production. _(The judge's half is built, 2026-10-04, step 25d; Part III Item 144: `theseus judge replay`, `audit` and `backfill`, §3.10. A state is replayed as it was sent when its builder, version and cap match, else rebuilt from the record (today `loop.v1`'s only), else left out with the reason. The memory-science and shell-mapping halves are not built.)_

**Deterministic simulator.** One process, no network, fully scripted, fast enough for CI:
- *Fake Discord*: a gateway that emits scripted events (messages, reactions, component clicks, voice joins) from a scenario file and records every outbound action Theseus takes, with a virtual clock so timeouts and wakes run in milliseconds.
- *Fake provider*: answers keyed by prompt hash from a recorded fixture set, or a small scripted policy ("call tool X then end_turn"), or, in a separate mode, a real cheap model for fuzzier scenarios.
- *Fake Jev*: recorded answers, or rule-based answers, or a real Jev call in a separate mode.
- *Real everything else*: the arena, WAL to a temp directory, tenders, indexer, budgeter, policy gate, MCP client against an in-process fake server.
- *Scenarios* are files: a binding, a cast of people, a script of inbound events, and assertions over the graph, the ledger, and the recorded outbound actions.

What it buys: invariants as property tests (a destructive tool never runs without a recorded confirm from the right person; the loop never exceeds a ceiling without a logged judge failure; a user `/model` pin is never overwritten; a compaction root is always reachable back to its range); regression tests for every incident, by turning a redacted production ledger slice into a scenario; and a way to develop the loop, roles, and memory pass for weeks before the Discord plugin is finished. It also gives the learning loop a dry-run: promote a pack in the simulator first, then in shadow, then canary, then live.

**Standing scenario sets:** crash at every external-action boundary (§3.16); lost completion (event never delivered, reconciler must find the spooled result); duplicate completion (second delivery is a logged no-op); completion arriving during harness restart; `/cancel` of a detached job and proof the scope died; completion with no matching record quarantined; a low-privilege user schedules a privileged action; crash after settlement but before continuation delivery; `outcome_unknown` followed by a genuine success; a late completion after cancellation; arguments changed after confirmation (a hook mutation, until the hooks were deleted on 2026-09-28); two executions on the same task or workspace; interrupted provider streaming with unknown usage; restore from a stale backup containing later-redacted content; a task promoted from a conversation runs while the conversation continues and a human message is routed to each correctly; a task waits a week for an answer and resumes with intact context; a hot thread of two hundred turns appends throughout with a stable cached prefix and a disclosure change forces exactly one recompile; a thousand sessions with distinct compilations survive a restart and each next turn reproduces its prefix byte-for-byte from the manifest; the admission ceiling is hit with a `/cancel` still honored immediately; authority edge cases (role revoked mid-execution, coalesced messages from two principals, confirm with changed arguments); redaction lineage; disk-full and Jev-outage behaviour. **First vertical slice:** one human request, one constrained typed tool action with a confirm, a task that goes `waiting` on a due time, a `/cancel`, and a crash during a dispatched action, all green in the simulator before any Discord code is written.
- **The store's crash test** kills its writer at any moment, its very first open included, then tears the WAL's tail past
  what was reported durable or read back by an open (theseus-0b8, theseus-4x6; Part III A4 Item 17).
- **The kernel simulator's invariants** include: an execution that has ended has no action planned or authorized
  (theseus-w98).
- **The kernel simulator races transactions** (theseus-0owd, theseus-jj9f; Part III Item 41): a second thread
  racing a turn runs kernel transactions against it, and the operator's answers are one frame each. _(Since 2026-10-05, sim2, theseus-celu.35; Part III Item 170: it also drives the operator's `/stop` (between turns, inside one, on the racing thread, one call alone, and a task's stop, refused), tasks under a parent with their carves, reports and report wakes, wakes that fall due while their turn runs, and the outbox, with a fake binding and channel keyed by each post's id and crashes around every transition. Among its invariants: a stop keeps the execution's tasks and wakes and declines what it had planned; a parent's carve for a task equals what the task can still spend; an ended task's report is read once, by its parent; only the sim's own writes add posts, and the channel holds at most one copy of each. Its coverage counts come from a run with no second thread, so they are fixed by the seed (theseus-81ig).)_
- **Discord without a person** (theseus-9kjv; Part III Item 47). `theseus-sim` stands in for all of Discord the
  binding talks to: its REST API and its gateway, with a guild whose members and overwrites answer the viewer
  check. A check types a message as a user and presses a card's button as one, each sent as Discord sends it,
  and reads back every message the binding posted, each version of it, and every answer it gave an
  interaction. `theseus-sim discord proof --theseusd <bin>` runs theseus-kl8m's twelve steps against a real
  daemon in about three seconds, and the gate runs it. A step that touches the binding runs it against its
  release build as its live check, and drives anything new with `theseus-sim fake-discord --gateway` and
  `theseus-sim discord say|press|read`. Real Discord is still checked by hand: the bot's permissions and
  intents, and twilight against Discord itself.

## 9. Efficiency targets (to measure, not assert)

Under FAST (§2), the lifecycle rows are budgets. `theseus-sim bench lifecycle` measures them (M3.5,
P5b):
- cold start to the first `health` answer;
- clean shutdown with executions waiting and a job running;
- SIGKILL, then restart to the first answer;
- a binary swap under load: the `shutdown` request and its answer, then the other build started at once on
  the same store, to its first answer, with the job's wrapper kept and adopted;
- restore from a local WAL, beside a cold sequential read of the same bytes. _(Since 2026-10-06, theseus-nh1k; Part III Item 214: and `cancel`, `execution.cancel` of a running job from request to answer, the answer checked (one cancelled action, `termination_verified`, `killed` > 0, `survivors` 0, nothing left dispatched). Its budget, 250 ms with no margin of its own, is Theseus's own, not one of the table's rows below; on the build machine its quiet p50 is 74 to 77 ms, about 30 ms of it the stop's polls and most of the rest the syncs of its frames, about five (theseus-dwoj, to bring it near 50 ms).)_

Each phase runs N times, with p50 and p95, per phase, per start-path phase, and per kernel step. It runs
on an empty store, on the owner's store, or on a synthetic store of parked sessions.

Beside the lifecycle, three more benches measure what runs (theseus-goa8, Review 2's S4; Part III Item 48):
- `theseus-sim bench turn`: a plain turn and a tool-call turn on the stand-in model, each run N times on one
  warm session: wall time by the bench's clock and the daemon's, and frames per turn, counted from the WAL by a
  read-only tail. The plain turn's frames are a gated budget (below). _Since Tier 7.1 and wz4y (Part III Item 88), a tool-call turn's are too, 9, exact, and each turn's own traced count must equal the WAL's._ Beside them: this disk's `fdatasync`,
  and the daemon's resident memory after the start and after a burst of turns.
- `theseus-sim bench idle`: a daemon with nothing to do over a window (30 s): its CPU time, its wakeups, the
  frames it writes (none), and its memory, on an empty store or a synthetic one of parked sessions. Measured,
  with no budget yet.
- `theseus-sim bench size`: the shipped binaries' sizes, against the binary-size row below.

The gate runs `bench turn --check` on every commit, a lane's gate too: a count of frames needs no quiet
machine. Its timings and the memory are recorded in the bench history only where the lifecycle bench is (the
join's gate), and `scripts/repro.sh --bench` records an optimized build's size, turn, and idle rows.

`scripts/gate.sh` runs it on every commit: ten runs of each phase on an empty store, with debug binaries,
which are never faster than release.
- **Dependencies at opt-level 2.** Since theseus-1hk, the debug profile builds dependencies at opt-level 2.
  The workspace's crates stay at opt-level 0, with their debug assertions. So the bench times the start
  path's own work and its fsyncs, rather than unoptimized serde, sha2, and redb.
- **Budgets and margins.** Each p95 is checked against its budget plus that phase's noise margin. The margin
  is the spread of p95 over repeated runs on the build machine (in 2026-09: 7 ms cold, 4 ms shutdown, 25 ms
  kill, 2 ms swap; restore has no budget yet). Two more were proposed by lane perf1 for the owner's call, and are measured and printed, not gated, until he
  answers (theseus-fsug): restore's, on its p50 (below), and 150 ms for a clean stop with a post in flight,
  the 100 ms of a stop plus the 50 ms grace it may wait out (the bench's `inflight` phase, theseus-ndw; on the
  gate's empty store, debug, p50 67.7 ms and p95 74.0 ms; Part III Item 46).
- **A miss.** With ten runs, nearest rank makes the p95 the slowest run, so one stalled fsync can decide it.
  A miss runs the bench once more, and only a second miss fails the gate.
- **A quiet machine.** Before each run, the gate flushes dirty pages, then waits until the kernel's IO and CPU
  pressure (PSI, `some avg10`) are under 10 % and 20 % (theseus-m2lt, 2026-10-01) and the 1-minute load average
  is under the core count (theseus-611s), and says how long it waited. A neighbour that keeps writing stalls a
  start's fsyncs for seconds, and the bench would measure the neighbour: during an openclaw rotation, a
  restart's p95 was 2.3 s on a tree that had passed at 41 ms. Busy cores with nothing queued slow each thread
  with little CPU pressure (all-core turbo, shared SMT siblings): at load 14 on 16 cores every phase ran about 2×
  slow. The load bar is the core count, not half of it, so a long neighbouring build doesn't stall every gate.
  After 5 minutes it measures anyway _(since theseus-lf1n, Part III Item 81: the load bar is three quarters of the cores, 12 of 16, and the wait 2 minutes, so a gate in the band where strict misses cluster waits, then benches with the allowance calibrated there; the settle line names the bar)_. ~~The budgets don't change.~~ Since theseus-lew7 (Part III Item 75) the
  timing budgets then get the busy allowance: a phase over its limit by no more than
  `THESEUS_GATE_BENCH_ALLOWANCE` per cent (65 by default, calibrated on 22 busy runs; 0 is strict) passes, and
  says so, while the history keeps the strict verdict. A count, such as a plain turn's frames, never gets an
  allowance, and a quiet window keeps every budget strict. A lane's niced gate whose only miss is the bench,
  beside nice-0 neighbours, counts as green when the bench, rerun alone at normal priority, passes.
- **A lane's gate skips the bench** (`THESEUS_GATE_NO_BENCH=1`; 359ac9e, 2026-10-01 at 23:15, for the owner's "we have
  to figure out how to parallelize more"). The bench's wait for a quiet machine held the shared gate lock for
  minutes while every other agent's gate queued. The gate that joins a lane to `main` runs the bench, so every
  change is still benched on `main` before it lands. A lane whose work touches the start path runs
  `theseus-sim bench lifecycle --runs 10 --check` alone, at normal priority, once before its join.
  Since theseus-rx91 (Part III Item 51), a lane's gate takes the shared gate lock itself, only around its
  tests and benches (`THESEUS_GATE_LOCK=inner`), so its compiles never hold another gate up.
  Since theseus-lew7 (Item 75) that is every gate's only mode, the join's included: `outer` is refused, and
  `theseus-quiet.sh`, which paused lanes' compilers during a join's bench, is retired.
- **An L1 job's start** (theseus-mll1; Part III Item 73). After the lifecycle bench, the gate's `jobs` phase runs
  `theseus-sim bench jobs --class l1 --runs 20 --check`: the p95 from the dispatch to the command's exec, under
  25 ms (the M4 design's target), with one rerun after a flush and a settle on a miss. It is skipped where the
  lifecycle bench is, so a join's gate runs it, and the suite measures the row and bounds nothing. At the joins
  of 2026-10-03 its p95 was 5.37 to 7.22 ms.
- **The push's seed** (theseus-in3). The first `executions.watch` or `session.wait` after a start reads every
  execution and action into the board, off the start path. It is measured, with no budget yet: on the gate's
  empty store about 0.5 ms; on the 10,000-session synthetic store, release, p50 37.1 ms and p95 41.5 ms
  (2026-10-01); on the owner's store 285 µs. Past 250 ms, the design puts an index of open actions first, which
  would also speed `session.list` and `confirm.list`. _(2026-10-02, lane perf1, Part III Item 46: on that store, release, p50 45.5 ms and p95
  49.2 ms, about 5 ms more, since the start no longer reads every execution first; the seed's own work is
  unchanged. Past 250 ms, the board's actions can be read by the store's terms, which hold every action's
  state (theseus-lv2); `confirm.list` already reads its planned actions so.)_
- **The history.** Every run, a miss and its rerun both, is appended to a history outside the tree
  (`$THESEUS_BENCH_HISTORY`, by default `~/.cache/theseus/bench-history.csv`, shared by every worktree). Each
  row holds:
  - the time, and the branch and commit judged (`git describe --always --dirty`);
  - the load;
  - each phase's p50, p95, and limit;
  - whether the run passed, strictly, and the busy allowance it passed on, if any (Item 75).

  `theseus-sim bench history` prints each phase's last runs, with the headroom left. A passing run warns,
  without failing, about each phase whose p95 is within 10% of its limit, so drift shows before it fails.
  Re-deriving the margins from a week of `main`'s rows, or moving to `--runs 20`, is open (theseus-zay1).

Between today's store sizes and 10,000 parked sessions, the cold-start budget is the line from 50 ms to 250 ms.
Measured values are in Part III (A3, lifecycle timings, the M3.5 entry, and Item 24).

| Metric | Target |
|---|---|
| Process start to answering the protocol socket (config parsed, store open, WAL tail replayed, spool drained; secrets, credential checks, and the Discord gateway may still be connecting) | under 50 ms at today's store sizes; under 250 ms with 10,000 parked sessions. It grows with the WAL tail since the last checkpoint, never with history _(With the config in the vault, a start serves from the note's last-known-good copy and reads the vault behind the socket. Only a first start, with no copy, reads it before serving, which takes about 1 s. theseus-2fo, step F1b. The bench's `vault` phase holds this to the budget in the gate.)_ _(Since theseus-8ni, F4a: the store's open checks the WAL only from the frame after the index's checkpoint to its end. The history is checked after serving, at about 5 % of a core, and a corrupt frame there is refused and loud. On 10,000 parked sessions the store phase went from 63.2 to 10.1 ms p50, and a cold start from 164.1 to 121.7 ms.)_ _(Since theseus-lv2 and theseus-0dq, lane perf1, Part III Item 46: of the executions, the start reads only those a crash interrupted, those with a unit budget, and those whose limit follows a changed config, by a projection by state in the store's index. Health counts by that projection, and the driver's tick reads only the queued executions and those a due time may wake. The history check after serving starts at the last check's mark, and reads the whole history once a day. A store an older build wrote last has its projection built after serving, and is read as before until then. On 10,000 parked sessions, release, a cold start went from 119.1 to 21.5 ms p50, the kernel's part of it from 35.8 to 7.6 ms.)_ _(Nothing else joined the start path on 2026-10-02. The index tender starts 2 s after serving (row 51, Part III Item 50), so neither a start nor its aftermath shares the disk with the tender's start, and health asks no tender before one runs; the L1 probe followed at 3 s (Item 58), until the sandbox trims removed it (Item 77). AWS adds `[aws]`'s parsing and the tools' fixed definitions, in microseconds: no secret read, no catalog decoded, no client built, and no request before the socket answers, since the account's check runs after serving (Item 49). The lifecycle bench binds an account whose endpoint nothing listens on, so every budget holds with AWS out of reach.)_ _(Since Tier 7.7, Part III Item 82: an open makes nothing durable, so a clean restart pays two syncs where it paid three, and a start after a SIGKILL six where it paid eight; the daemon's store phase went from 8.1 to 4.6 ms. The first start after a format bump pays two more, once, for the manifest's move, theseus-e4jo.)_ |
| Clean shutdown, request to process exit, with work in flight | under 100 ms; nothing in flight is waited for, except the outbox's posts already sent, for at most `[server] stop_grace_ms` (50 ms by default) from the stop's start (theseus-pfv; a post still unanswered then stays dispatched, and the next start sends it again under the same nonce; a longer grace is the operator's choice, outside this budget) _(Since theseus-02k, lane perf1, Part III Item 46: redb's close makes the stop's checkpoints durable, so a stop pays one commit's syncs instead of two: 5 syncs, where it paid 6 or 7. Each phase of a stop is logged at debug. A rare slow stop is one of its two waits on the disk, stalled by the machine's writeback (theseus-26r). The index tender gets SIGTERM and is never waited for (row 51; a test stops a daemon whose tender is SIGSTOPped).)_ |
| Crash to serving again (SIGKILL, then restart) | the cold-start budget plus tail replay, with the checkpoint interval keeping replay under 100 ms _(The bench's budget: the cold-start budget plus 100 ms.)_ |
| Binary upgrade (swap, stop, start; job wrappers keep running) | under 200 ms without a protocol answer _(Since F4b, theseus-qa0: the bench's swap phase holds this in the gate, from the stop's request to the other build's first answer, with a job's wrapper running through every swap and ended at last by a daemon that never started it. `theseus shutdown` answers before the daemon has closed its store, so a start that follows at once waits for the store's lock, at most 3 s, instead of failing. Numbers in Part III A3c, F4b.)_ |
| Store or schema migration | adds nothing before serving: old formats are read in place, and a tender rewrites them in the background using at most 5 % of one core _(Since F4a, theseus-qa0: every record carries its kind's schema, and the manifest names the newest schema written for each kind. A start writes neither: a store an older binary wrote is marked only when this build first appends a newer record. A build older than the store refuses to open it, before anything is written. No layout has needed a rewrite yet; the first that does lands with its tender.)_ _(Since Tier 7.9, Item 82: one format number; the move is the manifest alone, at the writer's first frame.)_ |
| Restore from a local WAL | at the disk's sequential read speed; to measure _(Measured in F4b; see Part III A3c. A restore copies the WAL, then opens the copy with no index, which checks every frame and indexes every record, so it runs 39 to 64 times slower than a cold sequential read of the same bytes. ~~The budget waits on how the index is rebuilt: in bulk, or after serving (theseus-byu).~~)_ _(Since theseus-byu, lane perf1, Part III Item 46: the restore prints its phases (copy, open, counts, record, swap), and its open builds the index in bulk from a replay of 4,096 records or more. At 10,000 sessions the copy takes about 50 ms and the open 314 ms. Proposed for the owner's call (theseus-fsug): a p50 under 250 ms on an empty store and under 750 ms with 10,000 parked sessions, a line between them as for the cold start, with the p95 and a cold read of the same bytes printed beside it. Not the disk's read speed: a restore writes a synced copy and then checks and indexes every frame, and the cold read itself varied twofold between sessions. On the p50, since a restore stalls on the disk now and then, on both builds: 3 of the 160 restores measured took more than twice their run's p50.)_ |
| Binary size, static | under 60 MB with Wasmtime, AWS SDK, voice, search, and web UI; embedding weights are a separate artifact _(Checked by `bench size` on an optimized build, 2026-10-02: `theseusd` 28.9 MB as `release-thin` and 23.4 MB as `release`, the others under 5 MB; Part III Item 48. The AWS layer adds about 3.1 MB, 2.27 MB of it the embedded catalog, within its design's 5 MB; Item 49.)_ |
| RSS at 10,000 parked channels, 50 active executions | under 1 GB including arena metadata for the active set _(Measured, lane perf1, 2026-10-02, Part III Item 46: a release daemon on 10,000 parked sessions idles at 16 MB. With 50 turns in flight at once, on a stand-in model, it peaks at 104 MB. Most of that is a listing of every session the harness asked for, of which glibc keeps 64 MB; the turns add about 23 MB. 9ac009d idled at 62 MB and peaked at 231 MB, most of it held by glibc's per-thread arenas. The index tender is its own process: about 24 MB on BM25, about 536 MiB with the model loaded, which unloads after `idle_unload_mins`; Item 50.)_ |
| Memory a job's output costs the daemon | at most 4 MiB, whatever the job prints (read by seek); its file at most `[tools] job_output_max_bytes` _(Since fb2c, theseus-102: a 200 MB job raised the daemon's VmHWM by 9 MB, against 211 MB before; Part III A4 Item 25)_ |
| Per-turn harness overhead (context compile + gate + WAL commit, warm arena; excludes model, Jev, tokenization, rehydration) | ~~under 5 ms~~ at most **5 WAL frames** for a plain one-loop turn, with a floor of 2; each frame is one `fdatasync`, so the count doesn't depend on the disk _(Restated in theseus-goa8, Review 2's consideration 8, Part III Item 48: no budget in milliseconds holds on a disk where one `fdatasync` is 7 ms. `theseus-sim bench turn --check` holds it at the daemon, in every gate, as `tests_m3::a_plain_turn_stays_within_its_frame_budget` holds it in the core; a step that writes fewer lowers the number in the same commit. Measured 2026-10-02, `release-thin`, a busy machine: 5 frames, 42 to 60 ms a turn, of which the 5 `fdatasync`s were 33 to 47 ms and the harness's own share at most 9 to 16 ms; a tool-call turn writes 11 to 12 frames.)_ _(Since Tier 7.1 and 7.3, Part III Item 88: a tool-call turn writes 9, held as a budget, and a quick `proc.run` returns in about 30 ms, not 50.)_ |
| An idle daemon | no frames written; CPU and wakeups to be budgeted once measured over a week of runs _(`bench idle`, theseus-goa8, `release-thin`, 2026-10-02: on an empty store 8 ms of CPU in 30 s, 0.03 % of a core, and 4.5 wakeups a second over 18 threads, 14 MB resident. At 10,000 parked sessions it never went quiet, 4 to 7 % of a core with no frames written (Review 2's S1), until perf1's reads by state took it to 0.10 %; Part III Items 46 and 48.)_ |
| Jev per turn | to be measured: calls, questions, input tokens, p50/p95/p99, per representative turn class; the ~350 ms / sub-cent figure is one call, and a turn makes several |
| Process start to accepting Discord events (checkpoint loaded, WAL tail replayed; excludes model/index warm-up, which proceeds in tenders) | under 2 s |
| WAL commit latency | ~~under 5 ms at group-commit interval~~ at most two group-commit batches: the one in flight, then its own. One batch alone is one fdatasync (about 7 ms on the build machine's disk) _(Restated in S2, theseus-vni9, Part III Item 52: "under 5 ms" was below this disk's one fdatasync (Review 2's consideration 8), so no design met it. Measured at 32 concurrent turns: p50 27 ms, p99 59 ms, a batch about 14.5 ms, 16 to 17 frames a sync.)_ |

