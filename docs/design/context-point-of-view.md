# One memory, many points of view

**A design for how Theseus builds the context of a turn (issue 44).**

Draft 1 kept each place's transcript as the spine and added a note to it. The owner ruled that out: the cache may
not be used as an argument against a deep redesign. Draft 2 was the deep design. Six critics then went through it
(§13), two of them against the code, and this is what survived: the same idea, with fewer moving parts, its rules
written down, and its claims about the code corrected. What it costs, and what can be won back, is Part Two, kept
apart on purpose. Nothing in Part One was chosen for the cache.

How to read the numbers: **(measured)** comes from the owner's own ledger, counts only. **(est.)** is an estimate
whose basis is given; every token count made from bytes is an estimate at 3.6 bytes a token. Budgets are settings,
not findings; the shadow steps exist to fit them.

---

# Part One: the design

## 1. The design on one page

1. **There is one memory.** Every message is written once, where it was said, and is never moved. No router
   decides where a message "belongs".
2. **There is no kept prompt.** Each turn, the context is assembled from that one memory around a **point of
   view**: who is speaking, where, who is present, what work is at hand, what task is joined, and when.
3. **A place is not a container.** It gives two things only: who is present (which decides what may be shown at
   all) and its own recency (what was last said *here*).
4. **Memory is lopsided, as a person's is.** The owner's own words are kept and shown word for word, from every
   place where he was alone with Theseus and the words are proven his, for a long time: in his main conversation
   they are 2% of the bytes (measured). Theseus's replies thin to a line and a handle. Tool results thin
   fastest; they stay openable, and are read again when the present state matters.
5. **Time is selected from, not summarised.** "Minutes, hours, days, months, years" is one walk back: this
   sitting as it was said; the last two days everywhere, in his words and Theseus's replies; then his words
   alone, fewer with age. Every band ends with a count of what was left out and a handle that opens it.
6. **A name calls its thing.** A recognised mention brings the thing's card, its current facts and its newest
   exchange; things named in the last few exchanges stay in play.
7. **Nothing is in view because it was in view last turn.** Every piece is chosen again each turn by a selector
   with a written rule, and the view records why each piece is there.
8. **A reference with nothing in view is looked for**, in widening steps, as far back as memory goes: by name;
   then by search over what was really said (his words and Theseus's) in the last two days; then by the
   answering model paging back itself, a week, a month, everything, without having to ask. One clear find is
   brought in; several are offered.
9. **A task is a point of view too.** Its context is assembled around its objective, its jobs, its evidence and
   what the owner has said about it anywhere. Joining it gives its status at once from records. Its objective
   changes only through the owner's own gate, on his current words.
10. **One gate stands before every new reader.** What the people present may not see reaches no model through
    them. Outside text keeps its mark through every retelling. A seal stays a wall.
11. **It is measured before it is trusted**: a ledger first (no prompt changes at all), then the whole assembly
    in shadow beside today's, then one quiet private place, then the main conversation. Three steps are deep
    cuts and are the owner's to take. The first step can start today.

## 2. What the owner asked for, and where each sentence lands

| His words | Where it is in this design |
|---|---|
| "immediately prior (and indeed a reasonable tail) of immediate conversational context should always be in context" | The floor (§5.2): the last three exchanges here, in his words and Theseus's, which no budget, judge or router can remove; the sitting around them is preferred whole, within slot G's cap |
| "if an event or issue is noted as a reference as of now unknown, it should be searched by name where possible" | Rung 1 of the reach (§6): a scoped name finds its thing, and the thing's own timeline finds the exchange |
| "where there was no name, it should search an incrementally longer tail of conversation, allowing itself to go way back" | Rungs 2 and 3 (§6): a search of the last two days before the first word; then the model itself pages back, a week, a month, everything, with no horizon, and without asking anyone for the first three looks |
| "a jev classification of 'dangling references' … that would automatically trigger a certain degree of introspection" | Half of it is already asked: the judge answers `fragment` and `mentions_other_conversation` on every message a person sends today, and nothing reads them (checked in the code). His fuller idea is the typed question `leans_on`, added beside them (§6) |
| "references to ontologically recognized things could call into play various nodes in the full total context tree" | §5.4: every thing has its own timeline in one index; a mention reads it |
| "not … separate individual messages onto truly separate threads" | Nothing is routed by a guess. A thread becomes a line of work: a thing with a card and a timeline (§9) |
| "maximally pull together recent conversation, mentioned items, facts about the person … far more sophisticated than the trailing session transcript" | The whole of §5: every slot is read from the graph each turn |
| "Not conversation per place, though place does have its own *recency selector*" | §4 and §5.2: the place is one root among several; its recency is slot G; his recency anywhere is slots E and F |
| "the last thing I said *here* was x about the weather but the last thing I asked it about *somewhere else* was that I wanted to select issue 33" | Slot F shows his words elsewhere as his words. Walked in §8(b) |
| "Theseus is like a person … it doesn't matter whether it's by phone or slack" | True wherever he is alone and proven to be the one speaking. Where others are present it is deliberately less (departure 4 in §14, decision 2 in §15) |
| "tasks and work and graphs of jobs have their own context too … assembled more deliberately" | §7: a task's pieces are chosen by its own links; its objective and origin are pinned, its state is read fresh |
| "When I join a task … it too has a point of view that causes context to assemble around it" | §7. Status comes from the task's records, read by the conversation; a question put to the task itself is answered from the task's own assembled view; a change of objective lays that view again |
| "the base case of a context *is* a time descending session transcript … the place, the task at hand, the person at hand, and memories about all of those … minutes, hours, days, months, years" | When nothing outside is named, the view is: cards for the person, the place and what is at hand, then one timeline in three adjoining lanes (his past, everywhere lately, here). It is *selected* newest first, so what is nearest in time gets in first and fullest; it is *sent* oldest to newest, as a transcript reads (§5.1; departure 6 in §14) |

## 3. The memory: what is stored

**Existing stored records stay unchanged; what is new is kept in separate small records linked to them.**
Messages, replies and tool traffic are written as they are today. The new records are tied to their sources by
the `derived_from` edge that exists; a new kind of record needs no change to the store's format, where a new
field on an old record would. Every
kind has a writer and a reader named; none is added for a future use.

| Record | What it holds | Written by | Read by | New? |
|---|---|---|---|---|
| **Node** | a message, a reply, a tool call or result | the turn, as today | every slot; handles | as built |
| **Owner's mark** | "this message was typed by the owner in person", and the place it arrived in | code, when the message is submitted, from the daemon's live proof of the owner (the one rule that refuses an agent's process). Today nothing of that proof is stored: after a restart, no past message can be proven his | the point of view; whose-words-they-are (§5.1); slots E and F | **new, and first** |
| **Exchange** | one message of a person and everything done until the final reply: place, person, start, end or "still being answered", the ask and the reply **by reference**, the things it touched, marks | code: the ask, its place and its person when the turn starts (so a message still being answered elsewhere can be seen at once); the reply and the touches when it ends. A late reply or line updates the same exchange | slots D to H; the reach | new |
| **`did` line** | at most 200 characters on what Theseus did in an exchange. It holds its source session's id, so it is admitted by its source's audience, and today's transcript never shows it | a small-model job afterwards, from his ask and the final reply (never from tool results); stored with the model and version that wrote it | slots E, F, H | new |
| **Thing** | anything recognised. Today the index recognises paths, files, Beads ids, commits, hosts, crates and mentions; a **line of work** is today's thread record; a **task** is the task record; an `item` kind for numbered things ("issue 33") is new | adapters over the stores that already have writers | slot D, slot H, the reach by name | adapter, plus `item` |
| **Touch** | the one new index: *this exchange touched this thing*, keyed by root and time, newest first, for every root: place, person, line of work, task, thing | code, at the end of an exchange (place, person, exact names). Interpreted topics are filed per session today, so they touch at that grain until the ontology judge is asked per exchange | every recency in the design is one read of it: "here lately", "him lately anywhere", "this thing's latest", "this task's history" | new |
| **Fact** | a statement about a thing, with its source, its time, and what it supersedes | the existing standing-record path (from the owner's own messages), his acceptances of proposed cards, the memory pass | slots B, D, H | as built; its subject is a small record beside it |
| **Task** | as built: the owner's layer (objective and acceptance), every edit a new version under compare-and-set, a model's change a proposal that waits on his card. Added: the owner's words behind each change, by reference | the task tools | §7 | one small record |
| **View** | what one turn was shown: the point of view, every piece by reference with its slot, its reasons, its form and tokens, and what was dropped and why | the assembler, keyed by today's compilation record, which names a new strategy | the renderer, the replay, `theseus view show <turn>` | new record; a renderer for pieces that are not nodes is new code |

Properties that hold for all of it.

- **His words are never paraphrased.** The ask of an exchange is his text, cut by code. Only the `did` line is a
  model's, and it describes Theseus's side.
- **Only proven words are his**, in three grades. *Marked*: the message carries the owner's mark. *Attributed*:
  history from before the mark, in a private direct conversation, whose author holds an owner handle. *By his
  handle, in company*: what he said in a shared channel, where the chat service proves each message's author.
  All three are shown as his words where he is alone. Only a marked message can authorise an act (§5.1). A
  terminal or web message from before the mark is "said on the terminal, author unproven" and is testimony,
  because agents use the same socket. A block he pasted is part of his message, not something he said: it is
  shown with the message and never read as his instruction.
- **A `did` line says where to look.** One standing sentence in the frame tells Theseus to open the exchange
  before relying on a number, a name or a choice in a line. When a message of his is a correction (the judge's
  existing answer), the lines of the exchanges in view are written again, reading the correction; the old
  version is kept.
- **A promise is kept by setting a wake or a task, never by remembering it** (a second standing sentence).
  Theseus's own replies thin; what it has undertaken must not live only in them.
- **Marks only grow.** An exchange is marked *outside text* if any of its tool results, or any piece of the view
  it was answered from, is marked. A `did` line carries its exchange's marks, and *sealed* and *veiled* likewise.
  Retracting or erasing a source finds what was made from it, through the edges.
- **Names are scoped.** A qualified name wins. An unqualified alias is accepted only if it is unique within the
  work at hand; otherwise the matches stay candidates. A bare number never makes an identity by itself: issue 33
  of two repositories is two things.
- **Facts are read only from what this point of view may see.** "Current" means the latest statement in that
  permitted history. Supersession is followed to its end. Two statements that disagree and neither supersedes
  the other are shown as a disagreement, with dates; none is picked by its timestamp.
- **What can be rebuilt is said plainly.** The exchange list and the touches written by code can be recreated
  from the original messages. Three things cannot: the owner's mark on old messages (it was never stored), a
  model-written `did` line (kept, with its version; writing it again is a new version), and the arrival place of
  an old message that a thread took (recovered only where a routing record proves it; otherwise unknown, never
  guessed).

## 4. The point of view, and the gate

A point of view is computed once at the start of a turn, from one snapshot of the store, and recorded on the view:

`{anchor: a place or a task · person · present · at hand · now}`

- **anchor**: where the message arrived, or the task that is running.
- **person**: the author by the daemon's live proof, written down as the owner's mark. A display name, a judge's
  guess or a quoted line never establishes it.
- **present**: who may see this context. Today the memory code has four values (private, shared, open, unknown)
  and the core code only two; a channel bound private counts as private whoever has joined it. The design needs a fifth, narrower one:
  **`owner alone`** = the terminal, the web page or a direct conversation with the owner, **and** this input
  carries the owner's mark (no agent behind it). Everything that crosses places needs `owner alone`. An agent's
  input is treated as a private channel: its own place and recall as today, nothing from anywhere else. A wake
  or a task's report takes the `present` of the turn that set it up. Unknown admits nothing.
- **at hand**: the line of work or task last selected here, by address or by a sure typed answer. It changes
  only on such a selection; an unsure answer leaves it as it was. It ranks optional material. It never moves or hides a
  message, and never removes anything from the floor, the sitting or the person-wide slots (C, E, F).

**The gate.** Every piece carries its source place, source audience, the owner's mark or its absence, origin,
seal and derivation marks. Only content that has passed the permission check can be sent to a model, and the code
is shaped so that a new reader cannot skip the check (a type that only the gate can make). The gate checks those marks against this point
of view: the place rule (a shared place draws only on itself), `owner alone` for every cross-place kind, the
seal, the veil, and the restricted-origin rule that today stops a recall note. A missing or uncertain mark
denies the piece. It runs **before** ranking and before any model reads a candidate, and again wherever text
reaches a model later: a page of history, a tool's result. Permissions are checked again before every model
call and every delivery, even inside a turn: if something already sent is no longer permitted (an audience
changed, a seal was set), that call chain is stopped and a fresh, permitted view is assembled, without the
earlier thinking.

The code decides by place in 69 spots today, each in its own way. They stay as they are; the new readers go
through the gate; a registry test fails on any *new* decision by place outside it. The property the tests hold:
adding or deleting records that this point of view may not see changes nothing that is sent, not a byte, not a
count.

## 5. The assembly

### 5.1 The slots, in the order they are sent

For a conversation where the owner is alone. Sizes are typical / cap, in tokens (est.; settings).

| # | Slot | What it holds | Selected by | Typical / cap |
|---|---|---|---|---|
| A | **Frame** | tools, header and standing orders, persona, project layers, the session block | as built | 13,000 to 20,000 (measured: 13,430 at the terminal, the median of fresh compiles; 19,641 in the main conversation, its one fresh compile) |
| B | **Who and where** | accepted facts about the person; the place and who is present | facts by subject; the binding | 800 / 1,500 |
| C | **Waiting** | what is certainly waiting between them, from any place: questions Theseus has parked for him, tasks waiting on him, a message of his still being answered elsewhere | records that exist today | 300 / 1,000 |
| D | **In play** | the work at hand and the things named in the last three exchanges here: card, current facts, open tasks a line each | at hand; the touch index | 800 / 3,000 |
| E | **Past** | his own words, in the three grades of §3, as dated lines, thinning with age (§5.3) | the person's timeline | 3,000 / 5,000 |
| F | **Lately, everywhere** | the last 48 hours in his other private places, merged by time: **his words verbatim**; per place the newest reply in full (over 1,500 tokens: its first 500 and last 1,000, since questions and conclusions sit at the ends), older replies as their `did` line with a handle. And always, whatever its age, his newest exchange in each other private place (five places at most) | the person's timeline, less this place | 2,500 / 12,000 |
| G | **Here** | this place's own recency (§5.2) | the place's timeline | 8,000 / 24,000, and a live sitting up to its own 100,000 |
| H | **Called, this turn** | what this message calls in: named things not in view, a reached exchange, a joined task's card, recall | the name matcher, the reach, recall | 1,800 / 8,000 |
| I | **Now, and the message** | the clock, the branch line, the task ticker; then his words | code | about 300 |

Typical total about **33,000**. One ceiling of **60,000** for everything except a live sitting, which has its
own cap of 100,000 (§5.2). Today's main conversation is about 360,000 (measured).

**In company** (a shared place, or a private channel others may join) the table is shorter: A, B, D, E, G, H and
I, all from this place alone; slot E holds the words of the people present, said here; C and F are absent. A
model of restricted origin gets the same shortened view, of this place alone, with one typed line saying that
memory of other places is withheld from it (decision 8).

B, C and D are cards. E, F and G are one timeline in three adjoining lanes, oldest to newest: his past; the last
two days everywhere; here. The order has a conversational reason: who and what first, then time running forward
to the message, so that what is nearest in time sits nearest the message.

**On the wire.** A is the system part. B to F are sent as marked blocks ahead of the conversation; H is a marked
block beside the new message. Only G and the new message are sent as real turns of the person and of Theseus,
in the order they happened, with tool calls and results paired as the provider requires.

**The budgeter's rules** (one function; these are all of them):

1. Each selection rule names the source material it wants, why, and whether in full or in a shorter form.
   Selectors do not render, and they do not know about each other.
2. The gate runs first. What it denies is not counted, ranked or mentioned.
3. Ranges of source that are identical or overlap are merged, keeping every reason and the fullest form asked
   for, and are shown once: in G if they belong to the floor or the sitting, otherwise in the first slot of
   rule 6 that asked. A piece is "in view" only if its own range is in the merged set; a thing's name or handle
   being visible does not make its card or exchanges "in view".
4. Where a thing's fact is shown as current and has been superseded, the old statement and its correction are
   one unit: both are admitted or neither. This applies to cards and called-in facts. What was said in the floor
   and the sitting is history, shown as said, with a correction added beside it when one exists.
5. There is one budget: the smaller of the ceiling (plus, for a live sitting, the sitting's own cap, §5.2) and
   the provider's window, less room reserved for the reply and the turn's own tool calls. Everything is charged to it: the frame, every slot, headings, residue lines,
   and pages of history read later in the turn. Sizes are counted with the answering model's own token count.
6. Admission order: A (fixed, counted first); I; G's floor as spoken; F's own words; C; E to 7 days; B; the
   floor's tool results; H's reached; H's task card; H's named; G's sitting; D; F's replies; E to 30 days; G
   before the sitting; E beyond 30 days; recall. Within one of these, newest first, then by id. What he said
   always outranks what a tool returned.
7. Over the ceiling, cut from the end of that order. A piece steps down a form (whole, then spoken, then line)
   before it is dropped, and what is dropped is counted in its band's residue line.
8. G's floor and I are never cut. A cut anywhere else is marked where it happens and carries a handle; a cut
   never breaks the pairing of a tool call with its result, and never rewrites the owner's words.
9. If the floor's own text cannot fit the provider's window (one enormous message), the floor cannot be kept,
   and the turn says exactly that instead of answering from a silently incomplete view: it names what did not
   fit, gives its handle, and asks for a narrower question or a deep look. Every such case is recorded.

**Forms.** A piece is shown *whole* (as it was said, with its tool traffic), *spoken* (his words and the reply's
text; tool traffic as one-line stubs), or as a *line* (`he asked → Theseus did`, his ask verbatim). Every piece,
in every form, carries a handle that opens the fuller one. Where a `did` line has not been written yet, or its
job failed, the line reads "reply not summarised" with the handle; nothing is invented.

**Whose words they are.** A message that carries the owner's mark, typed in another private place, shown word
for word where he is alone, is headed "you said, in <place>, <when>" and is his word, not background the harness
chose. The same holds for attributed history and for his words by his handle in company (§3). Everything else that is not from this place (replies,
lines, cards, recall, unproven terminal history, anything marked outside text) is headed with its place, date
and reason and is testimony, as today. A test holds this: text that is in none of the three grades never renders
as his.

Knowing what he said is not the same as being authorised by it. **Taking on something new needs his marked
message in the current exchange, and exactly one thing in view that it can apply to.** If in one place he said
"select issue 33 when I say go" and in another he says "go", there is one waiting request in view that "go" can
accept; Theseus says what it is acting on, and acts. If there are two, or the scope is unsure, it asks "do you
mean X?". A retrieved quotation of his is never, by itself, a present instruction, and every gate the owner
already has on acts (approvals, cards, the floor) applies as today. A task already under way needs no new
message to keep working inside the objective he accepted; changing that objective does (§7).

**Frozen within a turn.** Every selector runs once, on the first call of a turn, from one snapshot, and its
result holds for all of that turn's calls; a page of history read later appends, under the same gate. This is a
correctness rule of the build, not a saving: within a turn, what the model was already given must stay unchanged
for its saved reasoning to remain valid (the provider binds that reasoning to what precedes it, and the
strongest model has refused a request that broke this). Between turns
the view is built afresh, and earlier turns' reasoning is not carried, as at every recompile today.

### 5.2 Here: the place's own recency

- **The floor:** the last three exchanges that posted to this place, whoever opened them (he, a wake, a job's
  late result), in *spoken* form at least: his words and the text of Theseus's replies, whatever their age,
  never cut, and not dependent on a judge, on an index that may lag, or on a line that may not be written yet
  (it is read from the store itself). The floor needs no model, so the fault that began this (a reply four
  minutes later that lost the answer before it) is answered here; replaying it with the judge unplugged (§10)
  is the proof.
- **The sitting:** everything said here since the last gap of three hours or more stays whole, his words and
  Theseus's replies. Its tool results stay whole, newest first, up to 40,000 tokens; older ones become stubs
  that say how to get the result back. That number is today's own (the build's clearing keeps 40,000 tokens of
  results, chosen by replaying the owner's own bug hunt, after a smaller keep made the model re-read two thirds
  of its lines). The sitting has **its own cap, 100,000 tokens (a setting), outside the ceiling**, and both
  numbers are fitted on that same replay before step 7. A long working session keeps its working state without
  the owner having to ask for it. This answers the sharpest point made against any rebuilt context: that it
  "forgets what we just did" in the middle of live work.
- **Before the sitting:** *spoken*, newest first, while slot G has room; then it is the Past's business.
- **`/new`** puts a mark on this place's timeline. The floor, the sitting and what is in play read only what
  came after it: the first message after `/new` sees slots A to F (D empty), H and I, and nothing
  of G. What was said before is the
  Past's: his words in slot E, the rest by search and paging. Nothing is erased (decision 7).

**What a place is, by surface.** A chat conversation is its binding. The web page is one place. The terminal is
one place per project directory (the session block already holds the directory). A new session starts a new
sitting, not a new place.

### 5.3 The past: how minutes, hours, days, months and years thin

His words are small. In the main conversation: about 490 tokens for the last 24 hours, 1,540 for seven days,
2,900 for thirty, 4,850 for all seven weeks (est. from measured bytes). Across all his private places it is
perhaps twice that (est. from turn counts). So they are not summarised; they are **selected from**.

| Age | His words | Theseus's side |
|---|---|---|
| this sitting, here | all, whole | all, whole |
| the last 48 hours, anywhere | all, verbatim | newest reply per place in full; the rest as `did` lines |
| to 7 days | all, verbatim, dated, with place | `did` lines for what he asked; nothing for harness and wake turns |
| to 30 days | all, while the slot's cap holds; over it, the oldest days go first | nothing; a handle |
| older | only lines that are a correction (the judge's existing answer about the kind of message), that made a standing record, or that are still waiting | nothing; a handle |

His messages are short (about 40 tokens on average, measured), and many are pointers: "no, 41". So a line of
his that the judge marked as a fragment or a correction is never shown bare: at every age it is shown with its
exchange's `did` line, or it is counted in what was left out.

Every band ends with a count of what was left out (how many of his lines, how many exchanges) and a handle that
opens it. Nothing the gate admits is silently absent; what it denies is not counted (rule 2). The rules are deterministic; no model decides what an old line was.

Month and year paragraphs of Theseus's side are **not** in this design. Native history begins 2026-09-26
(measured); there is nothing to fold yet, and a model-written history is the part most likely to be wrong. They
are an option for later (§11, step 10), if labelled misses show something only they would bring.

### 5.4 How a mention calls a thing in, and how it stays

1. **Recognise**, in code: the index's own extractor (paths, files, Beads ids, commits, hosts, crates, mentions),
   the new `item` kind for numbered things, and a matcher of his words against the scoped names and aliases of
   lines of work and tasks (§3). A model's prose is read only by the index's own typed extractor; no other rule
   matches it.
2. **Bring**, for each thing recognised and not already in view: its card (≤12 lines); its current facts; its
   open tasks; the owner's guidance for it; from its timeline the newest exchange in *spoken* form and the two
   before as lines; **and his own words about it through time**: the thing's timeline, his lines verbatim,
   newest first, in the Past's bands, as far as slot H's cap allows. It is the same walk as the Past with
   another root, and the same code. About 400 to 600 tokens for the card and facts, and up to a few thousand
   for his words about a thing he has spoken of often (est.).
3. **Neighbours by name.** The links that exist today, one hop, at most three, in a fixed order (by kind of
   link, then id), as names with handles. No weighting until a measured rule exists. A neighbour becomes a full
   piece the moment it is named.
4. **The pull rule** (rule 4 of the budgeter): a stale value cannot arrive without its correction, headed
   "later, in <place>, <date>".
5. **In play** is not a stored list. Each turn it is read from the touch index: the things named, by him or by
   Theseus, in the last three exchanges here since the sitting began or `/new` was said, and any thing whose
   handle was opened in them; newest first, at most eight. Being shown renews nothing.

**The reasons.** Each piece carries the set of selectors that asked for it, from a fixed list (`floor`,
`sitting`, `here_earlier`, `elsewhere`, `past`, `waiting`, `at_hand`, `in_play`, `named`, `linked`, `reached`,
`task`, `recalled`, `correction`). They are enumerated values on the view record, for `theseus view show` and
for measuring which selector earns its tokens. They are not a second lifecycle: a piece is there this turn
because a selector chose it this turn, and for no other reason.

## 6. A reference with nothing in view

**How a turn knows.** Mostly it does not need to: the floor, the sitting and his last words in every other
private place are in view before anything is detected. The judge already answers two questions about every
message a person sends, and nothing reads them. `fragment` ("an incomplete piece that only makes sense with the
previous message") needs no search: the floor serves it. `mentions_other_conversation` is the trigger for a
search. The owner's own idea, a typed "this leans on something not said just now" (`leans_on`: nothing / what
was just said / something earlier / a named thing / unclear), is added as a pack of its own, beside today's, so
that live routing answers are not disturbed; a new acting question is promoted the way the build already
requires (a frozen holdout of 200 labelled judgments, or the owner's forced promotion). Nothing is inferred from
the wording of the message by rule. The judge runs at a third party; it is sent nothing it is not sent today.

| Rung | Runs when | What runs | Before the first word |
|---|---|---|---|
| 0 | always | nothing: the floor, the sitting, slots C and F | 0 ms |
| 1, by name | a name is recognised with nothing of it in view | the thing's timeline; for a name that is no thing yet, the words index over exchanges, newest three | one index read |
| 2, the last two days | the judge says the message mentions another conversation, or leans on something earlier or unclear | one search over what was really said in the last two days (his words **and** Theseus's replies), skipping what is already in view in full. The top three hits are read in spoken form by a small model on the answering provider, which answers for each: supports the reference or not, and the supporting passage; code checks the answer names one of the candidates it was given | one deadline, 1.5 s (a setting), with the typing mark shown. The index answers in 252 ms at the median but 2,001 ms at the 90th percentile today (measured), so about one such search in ten will miss the deadline and fall to rung 3 |
| 3, widening, by the model itself | rung 2 found nothing or timed out, or the model wants more | the answering model's own tool, `context.page {before, place, thing, words, form, cursor}`, from the same snapshot. `words` uses the same search; `form` defaults to lines, so one look of 12,000 tokens reaches about five weeks (est.). It widens by itself: a week, a month, everything. Every page passes the gate | after the first word; visible ("looking back") |

**Allowing itself.** The model may take three looks in a turn (36,000 tokens of pages, a setting) without asking
anyone. Past that it says how far back it looked and offers to go on.

**What is searched for.** The query is the current message together with the last exchange here. A name that
is not yet a thing is taken as he wrote it; nothing guesses names by pattern.

**What counts as found.** Only a supported passage resolves a reference; a card or a matching name alone does
not, and an answer from the small model that names no candidate it was given counts as a miss. One supported
hit: its exchange comes into slot H in spoken form (cut at 3,000), headed with its place and date. Several: up
to three are offered as candidates with their dates, and Theseus asks which. None: slot H carries one typed line
(`unresolved reference; searched back to <date>`), and the model is told to page before it may ask. Pages list
admitted exchanges newest first, with a stable cursor, across all permitted places unless narrowed. "History is
exhausted" and "this turn's allowance is used up" are reported apart: after the first the model may ask; after
the second it offers to go on. It never implies that all of history was searched when it was not. A turn that
asked without looking is counted as a failure. A found reference adds context; it never authorises an act
(§5.1).

The search reads replies as well as his words for a reason: "the second one" refers to something Theseus said.

**If the judge is absent.** After three timeouts in a row the turn stops waiting until one answers. Rungs 0, 1
and 3 need no judge.

## 7. Tasks

**A task's view** has the same assembler, gate and budgeter, another table:

| Slot | Holds | Cap (est.) |
|---|---|---|
| Frame | as built, with the task's tools and limits | as built |
| Objective | the owner's layer as it stands (objective and acceptance), then each change since, with his words | 1,500 |
| Jobs | the job graph as it is now: each job's state and last line; spend; parent and child tasks a line each | 2,500 |
| Evidence | the `named` pull for each thing in its working set; the quoted pieces it was made from | 6,000 |
| Origin | the exchange that made it, whole; what the owner has said about it since, from any place, newest whole | 3,000 |
| Own steps | its own run: the last three steps whole, older ones as stubs and lines | 24,000 |
| Called | recall over its objective; steers that have arrived | 4,000 |

A task's audience and authority are those of the turn that created it, and it works inside the objective the
owner accepted. It is sized by the same budget and the same forms as a conversation; if its current objective
cannot fit intact, it stops rather than run on a shortened one. "More deliberately" means: its pieces are
chosen by the task's own links, not by what one message happened to name; and its **objective, origin and
choice of evidence are pinned**, laid when it starts and laid again when its objective changes. Its *state* is
never pinned: at every step boundary, jobs, spend, arrived steers, corrections to its evidence and permissions
are read afresh from one snapshot, and a source that has been revoked or sealed disappears at once.

**Joining to ask how it is going.** From any place where he is alone: a match on the task's name, or the judge's
`addressed_task` (asked today; nothing reads it), puts the task's card in slot H: objective, state, spend, plan
step n of m, job states, and the task's last message cut at 300 tokens, each "as of" its time. All from
records; the task is not touched; the answer comes at once.

**Changing its objective.** The build already has the right gate, and this design uses it rather than a second
path: a task's objective and acceptance are the owner's layer; every edit is a new version under
compare-and-set; a model's change is a proposal that waits for his yes on a card. So the judge can only put the
task in view, and the **conversation model** proposes the change through the existing task update, naming his
message. The proposal is applied without a card only if all of these hold: that message carries the owner's
mark and is **of the current exchange**; the task is identified without ambiguity; the version it is based on is
still the newest; and the proposal declares its kind, one of `narrows`, `pauses`, `widens`, `replaces`. Only
`narrows` and `pauses` apply without the card. The other two always show it, with the objective before and
after; so does anything unsure. An "ok" can only confirm one specific proposal already put to him on a card.
A narrowing ("make it staging only") is recorded as a change to the objective with his words beside it, not as
a fragment that replaces it. A change lays again the objective, the choice of evidence and the origin; the
task's own steps are not thinned by it. The task hears it at its
next step boundary. The reply states it back: "version 8 of T's objective accepted: …", and distinguishes
accepted from heard. A "stop" is carried out as a pause, with an undo. A task cannot change its own objective
from anything it reads, and outside text, other people's messages or an agent's process on the terminal are
meant to produce no change, stop or delivery: the red-team cases of §10 must each end with nothing changed
before this goes live.

**Asking the task itself.** For a question the records cannot answer ("why did it drop those tests?"), the
conversation model may put the question to the task: one read-only call over the task's own assembled view, no
tools, no new session, offered only where he is alone, its answer headed as the task's testimony and carrying
the task's marks. This is the task's point of view answering, as the owner described it; it comes second in
step 8, after the card.

## 8. Three turns

Sizes est.

**(a) A reply four minutes later that names nothing.** "ok, the second one, but smaller." It is written where it
was typed; nothing routes it. Slot G holds the sitting, so the long answer of four minutes ago is there in
full. A 19,600 · B to F 7,400 · G 11,000 · H 1,500 (recall only) · I 300 = **about 40,000**. No judge is
needed. On the night of the fault the same message went to another session on a 66% guess and Theseus answered
"I can't tell what you mean"; the main conversation's own turn that night held 378,000 tokens (measured).

**(b) Another private place: something unrelated, then what he asked elsewhere yesterday.** Yesterday in the
main conversation: "I want to select issue 33 for the next step"; Theseus laid out the choice.
*First message, at the terminal: "it's pouring here."* Slot F holds yesterday's exchange: his sentence word for
word, headed as his, and the reply. Theseus answers about the weather, knowing both. About **24,000**.
*Second message: "did we settle which issue?"* His sentence about issue 33 and the reply to it are already in
view in slot F; if the judge's `mentions_other_conversation` fires, the search skips them as already in view
and brings nothing new. Still about **24,000**. In a shared channel none of it is in
view; Theseus says that if it is from a private conversation, it cannot bring it here.

**(c) Joining a running task: how is it going, and change its objective.** "How's the migration going? Make it
staging only." The name matches one task. Slot H gets its card from records (1,200); the reply gives the status
at once. The model proposes the change, naming this message; code checks the owner's mark, the target, the
version and the kind (`narrows`), applies it as version 2, and the reply says so. About **39,000**. The task hears it at its next step,
drops the jobs that no longer serve it by its own typed job calls, and its view is laid again.

## 9. What becomes of threads, sessions and places; what stays a wall

- **A place** is an address, an audience and a timeline. It holds no conversation of its own.
- **A session** is one run: the execution and the provider's chain of one point of view. Two places run at
  once without conflict; a task has its own.
- **A thread** stops being a container. Its record (title, goal, card, working set) is a **line of work**: a
  thing. The router's scoring survives as one input to "at hand". Its act changes from "send the message to
  another session" to "note what work is at hand". The threads benchmark supports this as possible, not as
  proven for this selector: one session holding every stream mixed 0 of 26 answers in each of three runs;
  routed threads mixed 2, 3 and 5 of 26 (measured).
- **Two walls stay.** A **seal**: sealed threads are left exactly as built (their own session, their own lens)
  for the whole of this path, and a message *addressed* to a sealed thread still goes to it; they are converted
  later, alone, because a task is not a wall today (the safety critic lists seven ways a sealed task would
  leak). And **a task's own run**.
- **Streams are kept apart by selection**: every piece not said here is headed with its place, its work and its
  date; when the work at hand is sure, optional pieces of other work come in only if named or linked; a thing
  shows its current facts; a stale value never arrives alone. Sureness about the work never removes the floor
  or anything in slots C, E and F. This is an answer-quality property with a measured gate, not an access rule:
  while the floor is verbatim, selection cannot *prove* that two subjects in one place never mix.
- **`/new`** starts fresh here, as the owner ruled for it: nothing said in this place before it is in slot G
  (§5.2). It does not erase memory: what was said before is still his past (decision 7).

Deleted at the very end, by his decision: thread sessions and their virtual places; the move of a message and
its undo (the stored record of old moves stays readable for ever); the home claim; the rule that treats the
place's own conversation as a foreign stream (the rule behind tonight's fault; it can go early, with his
acknowledgement, because a test of an owner's rule pins it); thread-only cold resume and keep-warm; recall notes
kept in the transcript for ever and the 64,000-token cap that pauses recall; conversation compaction as the
memory mechanism (it stays for tasks and as the window's guard); the judge fields nobody reads, once something
reads them or replaces them.

## 10. How it is measured before it is trusted

| Question | Measured by | What stops or changes it |
|---|---|---|
| Was the previous exchange in view? His last words elsewhere? | the ledger's row on every private turn (step 1), by ids; the fault itself replayed with the judge unplugged | any miss on the floor |
| How often does he name something Theseus can recognise? | the same row: things found in each message and in the reply before it | under 1 in 5 messages: names and the `item` kind come before anything leans on them |
| Does the shadow view hold what today's answer drew on? | the attribution judge (in shadow today for recall) reads the **live** reply, made from today's full context, and marks which earlier exchanges it drew on; then: of those, the share the **shadow** view held. This is *observed-support coverage*, a diagnostic, not completeness | under 95% overall, under 90% for what lay outside the last three exchanges |
| Does it bring what is *needed*, including what today misses? | a frozen set of labelled cases, made before any selector is compared: for each, the evidence required, the right referent, the current fact, the forbidden fact. Classes: a callback here; a callback from another place; "the second option" days later; a corrected fact; a task update; a forbidden disclosure. Evidence recovery and answer correctness are scored separately, at equal budgets, per case | any class worse than today's build |
| Does it answer as well? | past turns replayed in a scratch daemon both ways, from a snapshot as of the turn, with recorded tool results and with deliveries and changes suppressed; graded against a written rubric by a strong model, a sample read by the owner; reported by kind of turn (private, shared, task, long), with the count behind every share | the view loses more than 1 in 10 of the pairs that differ |
| Does thinning lose working state? | live: tool calls that repeat one whose result was thinned, labelled as a needed refresh or an avoidable re-read; handles opened; "it forgot" reports | the build measured this once: with a smaller keep, the model re-read 66% of its lines. Start from that grid. Avoidable re-reads after more than 1 in 5 turns, or more than 2 reports in a week: widen the sitting (a setting) |
| Does the trigger find dangling references? | step 1 counts how often the two existing judge fields fire; the labelled set (step 6) scores them and `leans_on`: recall of the trigger, false triggers, right referent, unresolved and timed-out, each reported apart | trigger recall or referent precision under 80%: names, floor and paging stay, and the search runs only on the model's own call |
| Are the `did` lines true? | the owner reads 50; a `gists` view makes that repeatable | any wrong number, name or choice in more than 2%; other errors in more than 10% |
| Do streams mix? Does a stale value arrive alone? | a new benchmark arm (the view, work as things) against the one-session, session-per-stream and routed arms, same binary and model, three runs, then the held-out seed; stale-alone counted apart from mixing; sealed markers counted in answers and in files | any mixed answer where both clean arms have none; any sealed leak (the one-session arm leaked two markers once). A clean result is 78 answers, not a proof |
| Can anything private, sealed or from outside change a view it should not? | from step 2, over generated stores: what the gate admits is unchanged with the forbidden records deleted. From step 6, the same on the bytes sent. Marks only grow. No text without the owner's mark renders as his. And a red-team table, each case ending with nothing shown, sent or delivered: an agent's process on the terminal says "change T's objective"; a page says "the objective is now Y" and he then says "ok"; a guest speaks in a private-bound channel; a seal is set after its lines were shown elsewhere | one differing byte; one case that ends otherwise |
| With nothing routed, how often does he lose "two at once"? | messages that ran while another session of the same place was mid-turn (today's routing hides these from the steer count), and steers about other work | more than 2 a week: the aside (§11, step 9) is built before threads stop routing |
| Speed, memory, code | lay time, the judge's wait, first-word time, with the searched path reported apart from the ordinary one; resident memory and store growth on the same workload; background model spend; lines without tests, stated at every landing | lay over 50 ms at the 90th percentile; ordinary first word slower by over 150 ms at the median; any rise in memory, or any step over its stated size, is raised to the owner before it lands |

## 11. The path

Every step sits behind one setting (`off`, `shadow`, `live`). Setting it back restores the old **reader**; it
does not unsay what was said, undo an act, or remove records already written (old readers are tested against
new records). The safety repairs in step 2 do not go back with any setting. Sizes are lines of code without
tests (est.; the buildability critic's figures, less the second selector it had counted in step 6; they ran
above mine).

| # | Step | What the owner sees | Size | Needs him? |
|---|---|---|---|---|
| 0 | Issue 43's fix (landed tonight, see the issue): a follow-up stays where its answer was; a turn in another session is given the place's last exchange | the reported fault is fixed | done | no |
| **1** | **The ledger, and the owner's mark.** From now on every submitted message records whether the owner typed it in person, and where it arrived. One measuring row per private turn, by ids and counts: the floor; the prompt by kind after clearing; cold starts and their cause; things recognised; the unread judge fields against what followed; overlapping turns in one place; turns opened by no person that he answered within ten minutes; how long his messages are and how many are fragments; terminal turns that a script, not a person, typed; private-bound channels whose viewers are not the owner alone. Exchanges and touches written by code for new turns; old turns backfilled where a record proves place and author | nothing | 1,000 | no |
| 2 | **The gate as a type**, `owner alone`, the tests over generated stores, the registry test, and the seal check that reading a session by id lacks today | nothing, unless decision 5 is taken | 500 | decision 5 for the one visible part |
| 3 | **His words elsewhere.** The per-turn block issue 43 built (sent beside the message, never stored in the transcript) also carries his last exchange in each other private place: his marked words verbatim, headed as his; the reply cut as in slot F. Sent only when it has changed. The red-team table passes first. **3b:** when his message in company leans on something not in that place, code lays a second view anchored at his private conversation for the same message; it searches there and posts there, with the card that can release one piece. The shared view never learns what was found | Theseus knows what he last asked elsewhere | 250 (+200) | **decisions 1 and 2** |
| 4 | **`did` lines**, by a small-model job with its own spend cap, skipping sealed and veiled sources; backfill of his native turns (about $0.11, est.); a `gists` view | he reads 50 | 900 | no |
| 5 | **Names, the search and paging, in shadow**: thing adapters, scoped aliases, the `item` kind, cards, his words about a thing; the one search interface; `context.page`; the `leans_on` question as a pack in shadow; all recording what they would have brought, changing nothing | nothing | 1,500 | no |
| 6 | **The assembler in shadow**, with every reader above: a pure function from a point of view and a snapshot to a request, run beside the live compile only on sampled turns, sending nothing. `theseus view show <turn>`. Then the labelled set, the paired replay and the new benchmark arm, in a scratch daemon | nothing | 2,000 | no |
| **7** | **The view goes live in one private place he picks**, a quiet one first, then the main conversation. In that place unaddressed messages are no longer routed away, and the view reads by arrival place across sessions already routed | in the main conversation 360,000 becomes about 37,000 to 40,000; that place knows the week everywhere | 300 | **yes: deep cut.** Reading is replaced; writing is not: nodes are still written under the place's run, so the setting brings today's reader back at one cold write (the old reader will then show replies whose evidence its transcript never held). `/new` (decision 7) and what a model of restricted origin is shown (decision 8) are settled first; the rule that delays a change of model in a long session goes, since nothing is left cold |
| 8 | Tasks: the card; a change of objective through the existing update and card; the pinned objective and fresh state; then asking the task itself. The card and the task view run in shadow from step 6, and the replay and benchmark are run again before this goes live | status at once; a change in his words; the task answers for itself | 600 | **decision 6**: a change in his own current words is applied without a card |
| **9** | **Threads become lines of work everywhere**: nothing is routed by a guess; thread sessions are frozen, not deleted; sealed threads untouched. If overlapping work exceeds 2 a week, the **aside** first: a side question during a long turn runs at once on its own read-only call (300) | no misfiled message, anywhere | removals | **yes: deep cut**, after the benchmark arm passes |
| 10 | Only on evidence from labelled misses, each for itself: conversational obligations (a question he was asked, a promise Theseus made) as tracked items with stable ids, where age lowers an item's place but never closes it; month paragraphs of Theseus's side; a second selector that ranks by spreading weight down the timelines (one design's proposal; it reads the same index) | fewer things forgotten | 300 to 1,200 | no, unless it adds memory or time |
| **11** | **Delete** | less code | about −3,000 | **yes: deep cut**, the one step no setting can undo |

**The first step is the ledger and the owner's mark.** It changes no prompt and calls no model. The mark has to
come first because nothing that follows can call a stored sentence "his" without it, and it cannot be added
backwards. The ledger settles the facts the rest stands on: how often the floor was missed, how often he names
something recognisable, what today's prompt is really made of, how often the main conversation starts cold, and
how many of his private channels have other viewers. It can start today.

Raised, as the owner's rules ask: steps 1 to 8 add about **7,250 lines** (the sum of the rows, 3b included);
step 10 and the aside could add up to 1,500 more; the removals at the end are about 3,000 and have not been sized file by file.
It is the largest addition since the compiler. No store-format step is needed if everything new is a new kind of
record, as §3 says; any that turns out to be needed is the owner's, listed before it lands. Memory: the touch
index is a table read on demand; no new model stays resident; any rise is raised before landing. Two slowings: a
private turn in a place with no live threads waits for no judge today, and here it may wait up to 400 ms for
one (the judge's median is 208 ms, measured), beside recall's wait, not after it; and where a search runs before
the first word it may take up to 1.5 s.

## 12. Risks

1. **Thinning loses what the next turn needed.** After a sitting a tool result is a handle; after two days a
   reply is a line. He would feel it as "it forgot its own work from this morning". The sitting rule is the
   guard; avoidable re-reads and his reports are the measure; wider slots are settings, not a different design.
2. **Recovering what Theseus itself said** is the weak side of a lopsided memory. Beyond two days its replies
   are a line and a handle, and finding the right one rests on the search. The labelled set has a class for
   exactly this ("the second option", four days later).
3. **The terminal is where Theseus is least sure who is speaking.** Agents share that socket, and the proof of
   "in person" there is a light guard: a script run from his own shell passes it (the release checks alone typed
   161 terminal turns in 30 days, against his 106, measured). A script must be able to say what it is, or his
   Past fills with its wording. Until the mark has been running a while, most of his terminal history is
   "author unproven" and does not count as his words.
4. **Small models choose part of what the strong one sees**: a wrong `did` line, a wrong "supports". Guards: his
   words are never paraphrased; a find is brought in only when its passage supports it; the floor depends on no
   model. If the search misses its bars, it runs only when the answering model calls it.
5. **A view is harder to see than a transcript, and for some weeks there are two compilers.** "What did it see,
   and why?" must have a one-command answer from step 6 on, or this cannot be debugged or trusted.
6. **Recognition is thin today.** The index knows paths, commits and the like, not "issue 33" or people. If few
   of his messages name anything recognisable, slots D and H are mostly empty. That is by design not fatal:
   with no recognition at all, the view is still "here, elsewhere, his words through time", which already makes
   it one Theseus across places. Step 1 counts it before anything leans on it.

## 13. Who designed and criticised this, and where they disagreed

Twelve designs and thirteen critiques went into it. Round one: Fable 5.2, Opus 5.5, Sonnet 5.5, DeepSeek v4 Pro.
Five critics of draft 1 (cost, safety, behaviour, build, vision). Round two, after the owner's ruling on the
cache: Fable 5.2 three times (once at maximum thinking, once at extra-high), Opus 5.5, GPT Astra twice (once
with the code in front of it), GPT Sol, Gemini Carbon. Six critics of draft 2: Fable (the owner's eyes), Opus
(buildable and safe, against the code), GPT Astra (simplicity, against the code), and GPT Astra, GPT Sol and
Gemini Carbon reading it cold. Two last readers (Opus, GPT Astra) then checked the final text for contradictions, for rules a builder
would still have to invent, and for claims stronger than their evidence.

All eight round-two designs agreed, independently, on: never moving a message; assembling around a point of
view; the place as audience plus recency; a floor no model can remove; a mention bringing a card, facts and
newest exchanges; typed judge answers that can only add; a bounded search before the first word and unbounded
paging after; a gate that runs before selection; seals as walls; shadow before live; and a first step that only
measures.

| Question | The split | Chosen, and why |
|---|---|---|
| What thins the past | Summaries by day, month, year (Opus, GPT Sol, GPT Astra, Gemini, Fable's graph design) against his own words selected and never summarised (Fable at maximum thinking; the vision critic) | **His words, selected.** They are small; a summary of them adds an error rate to save a few tokens. Theseus's side is a line and a handle; model-written history is deferred |
| How selection is decided | A table of slots with written rules (most) against spreading weight down the graph (Fable's graph design) | **The table.** Four critics asked for the second engine to be cut until the first has failed a labelled case; it is on the shelf (step 10), reading the same index |
| Unfinished business that follows him | A judge tracks questions, offers, requests and promises (Fable at maximum) against "nobody asked for a promise tracker; it will linger or vanish wrongly" (three critics) | **Only what is certain is shown** (parked questions, waiting tasks, a message still being answered). Inferred obligations are step 10, on evidence |
| When the context is rebuilt | Every turn (GPT Sol, both GPT Astra, Gemini) against once per sitting, or per epoch with deltas (Fable, Opus) | **Every turn.** Keeping earlier pieces to save a write changes what is shown, so it is a separate decision in Part Two, not a property of the design |
| Who may change a task | The judge at 0.90 (Gemini, Fable's graph design, draft 1) against the conversation model by a typed tool (Opus, GPT Sol, GPT Astra, Fable at maximum) | **The conversation model, through the owner's existing gate**: his mark in the current exchange, one target, the newest version, a declared kind; otherwise his card. The safety critic showed how a quoted page plus "ok, sounds good" becomes a change under the first; the build critic showed the gate already exists |
| Keeping streams apart | Labels and selection, measured (most) against withholding other work's facts even from the immediate tail (GPT Astra) | **Labels and selection**, stated honestly as a measured property. The benchmark's cleanest arm is the one that holds everything |
| How the search widens | Windows at once over his words only (draft 2) against one after another over everything said (four critics; the owner's own word is "incrementally") | **One after another, over his words and Theseus's** |
| The order of time on the wire | Newest first in one merged lane (GPT Astra, GPT Sol) against oldest first throughout (Gemini) | **Oldest to newest, three adjoining lanes**, with "here" as real turns. The merged newest-first lane is a presentation to test in the replay |
| Sealed work | Becomes a task (several) against stays exactly as built (Opus round one; the safety critic) | **Stays as built** until converted alone |

## 14. Where this departs from the owner's words, and why (none of it for cost)

1. **"Maximally pull together"** is built as "everything a rule selects, nothing else", under one ceiling. The
   reason is attention and the ability to say why each piece is there. It is not proven that less is better:
   the benchmark does not show that more context mixes more. So the ceiling is a setting, and the replay is run
   at twice and four times it before anyone defends a number.
2. **"The base case is a time-descending transcript"** holds for what was *said*. Theseus's replies thin faster
   than his words, and tool results fastest. An old result is kept and can be opened (it is history: a test that
   passed last week did pass), but it is not carried, because what it describes may have changed.
3. **"Go way back … if necessary"** is honoured in reach (no horizon, and the model widens by itself) but not
   as an unbounded silent wait: about a second and a half before the first word, then the model pages in the
   open for three looks before it offers to go on, and several finds are offered rather than one assumed. A confident wrong referent is worse than "which of these do you mean?".
4. **"It doesn't matter whether it's by phone or slack"** holds wherever he is alone and proven to be the one
   speaking. Where others are present Theseus is deliberately less than a person: it is not shown private
   memory at all, because a model that has been shown a thing can leak it; "knows but stays silent" is not on
   offer. This is an audience rule for the first build, not the memory model, and decision 2 offers a way to
   release one piece by his own hand. Also: his last ask in another place is always in view, but beyond two
   days the rest of what was said elsewhere, and Theseus's own older explanations, are a search away.
5. **Two walls stay**: sealed work, and a task's own run.
6. **The order on the wire** is oldest to newest, as a transcript reads, in three lanes rather than one;
   "descending" is how it is selected.

## 15. What the owner decides

Steps 1, 4, 5 and 6 need nothing from him, and step 2 only for its one visible part; step 10 only if it adds
memory or time. Each decision below comes with a recommendation:

1. **His marked words across his private places are shown as his words**, and one identified waiting request
   can be accepted from another place ("go"). *Recommended: yes, only where he is alone, only for words that
   carry the mark, and an act still needs his message in the current exchange.*
2. **In a shared place, Theseus is shown nothing he said privately.** *Recommended: yes. When a message of his
   there seems to lean on something that is not in that place, Theseus says so from the visible message alone
   ("if that is from a private conversation, I cannot bring it here"), without confirming that anything
   matching exists. Separately, in his private conversation and under that place's own rules, a card can offer
   to release one piece; nothing crosses without his press (step 3b). This is stricter than his ruling, which
   is about what is shown in a place, not what the model may hold; the reason is that a model shown a thing
   can leak it.*
3. **Look back before asking**: up to about a second and a half of search before the first word, then visible
   paging by the model itself (three looks without asking), rather than asking at once. *Recommended: yes.*
4. **History whose author cannot be proven**: his terminal messages from before the mark, and sessions imported
   from other tools. *Recommended: they stay reachable by search and recall, headed as unproven or imported, and
   do not count as his words in the Past.*
5. **A private channel that others may join** is treated as not-alone from step 2 on: its recall narrows to its
   own place (today it draws on everything), and so does an agent's session on the terminal. *Recommended: yes;
   step 1 first counts how many such channels he has.*
6. **A change to a task's objective in his own current words is applied without a card** (today every model's
   change waits for his card). *Recommended: yes, under the four checks of §7; anything short of them still
   gets the card.*
7. **After `/new`, his earlier words in that place still show in his Past** (slot E), while nothing of that
   place's earlier conversation shows in slot G. *Recommended: yes.*
8. **A model of restricted origin is shown this place only**: no Past from other places, nothing from
   elsewhere, and it says so.
   Today that rule stops recall only. *Recommended: yes.*
9. **The three deep cuts** (steps 7, 9, 11), each when its measurements are in.
10. **The cost talk** he deferred: Part Two.

---

# Part Two: what it costs, and what can be won back

*Kept apart by the owner's ruling. Nothing above was chosen for these numbers.*

## 16. The cost of the design as it is: a fresh view every turn

Prices, per million tokens, from the catalog in the repository: Sonnet 5.5 input $2, output $10, cache read
$0.10, five-minute cache write $2.50, one-hour write $4. Haiku 5.5 input $0.10, output $0.50 (five times that
above a 100,000-token prompt).

**Priced on his own turns.** A simulation priced per-turn assembly over the owner's 106 real turns of the last
30 days, each on the model it really ran on, with the real outputs and tool traces held fixed. These are
counterfactual token estimates for three stand-in assemblies, **not** a replay of this selector: they do not
include the `did` lines or the search, and they cannot say whether answers would be as good.

| | Those 106 turns |
|---|---|
| What he paid (measured) | **$40.24** |
| Assembled each turn, lean stand-in (first prompt about 33,000 at the median) | $9.69 (est.) |
| Assembled each turn, middle stand-in (about 43,000; other places' last day in full) | $12.98 (est.) |
| Assembled each turn, rich stand-in (about 67,000) | $16.45 (est.) |
| For comparison: today's design after a one-off trim of each transcript | $9.82 to $11.21 (est.) |

This design's view is nearest the lean and middle stand-ins. What that supports: **a fresh view every turn is
likely to cost a quarter to a third of today's bill, with no cache cleverness at all**; and a trimmed transcript
costs about the same as assembling, so the cache is not a reason to keep the transcript. The forecast for this
exact selector comes from step 6's shadow rows, not from here. The main conversation is 91% of those 106
turns' spend (measured).

**One turn** (est., Sonnet 5.5, a 33,000-token view written once and read by the turn's later calls while the
cache lives):

| Turn | Today, in the 360,000-token conversation | This design |
|---|---|---|
| Two calls, warm | about $0.10 (last week's median turn: $0.097, measured) | about $0.09 before output |
| The first turn after a long pause | $1.44 before output | about $0.09 before output |
| A 23-call turn (rare for him: 2 of his 106 turns ran ten or more calls, measured) | $0.83 in reads alone | about $0.16 |
| The `did` line | | about $0.0002 an exchange (1,500 tokens in, 80 out, Haiku); about $0.03 a week |
| A search that reads three candidates (rung 2) | | under $0.002 a span (est.: 9,000 tokens in, Haiku) |

A long working sitting carries up to 100,000 tokens of its own (§5.2); while it lasts, its turns cost what a
conversation of that size costs today, and the saving returns when the sitting ends.

Where it costs **more** than today, plainly: a small, fresh place pays for about 9,000 tokens of context it
never had (slots B to F and H, typical), about two cents a turn at the write price; and a warm two-call turn in a small session is cheaper today. Where it costs
less: every cold start, and turns in a conversation that has grown long.

## 17. What can be won back

Without changing what is shown:

1. **Stable first.** The frame is stable and comes first; B, C and E change slowly. How often B to E are
   actually read from cache when two turns are close together is a shadow measurement.
2. **Frozen within a turn** (a correctness rule anyway): every call after the first reads the view from cache.
3. **Five-minute writes, no keep-warm.** A cold start of 33,000 tokens costs less than three warm reads of
   today's conversation. The one-hour write adds 50% and buys nothing here (simulation).
4. **Reuse a prefix only while every piece in it is still selected, permitted and correctly headed**; otherwise
   send the view afresh. The shadow measures how often that holds.

**A separate decision, because it changes what is shown.** Several designs get a warm follow-up down to $0.02
or $0.03 by sending each new view as the old bytes plus what changed, so that pieces called in by earlier turns
stay visible until the next whole send. That is not a cheaper renderer of this design; it is a different
content policy: it shows more than the rules select, and it cannot remove a piece that has been corrected or
whose permission changed without a whole send. If the owner wants it, it is decided and measured for itself,
and it is not in the cost above.

**A lever that exists today, with no code** (read in the code by the build critic, not run): the daemon's
working-window setting would cut the main conversation from 360,000 to about 40,000 to 60,000 by today's own
ring and summary. It is coarse (it applies to every session, and the summary is a model's), and it is not this
design; it is listed because it is the cheapest lever known while the path is walked, and its effect on the
live conversation has not been measured.

*— written by Tabitha/Claude*
