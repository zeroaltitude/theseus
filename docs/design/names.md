# Recognition: topics per exchange, numbered items, scoped names, and a thing's card

Issue 44, step 5's recognition half. The code is `crates/theseus-core/src/names.rs` and its modules: readers
and an index. They reach the model through the view's sources (`view/sources_names.rs`; "The view's sources"
below). Every turn uses the view; there is no alternate transcript request path.

## Topics per exchange (`names::topics`)

The judge's `onto.v1` already rides the request sent for every message a person sends, and files the
*session* under categories. The main conversation is one long session, so a topic could not find the exchanges
that were about it. Now, in the sink's frame that writes the judgment's row, each category the answer chose at
`[ontology] act` writes a touch to the exchange of that message: `touch.thing:<category id>␁…␁<turn>`, at the
exchange's own start time. A category id is already `<kind>:<local>` (`topic:rust-harness`), and no ontology
kind is a type of the index's extractor, so the id is the term.

- A listed candidate touches whether or not the filing writes anything (a session already filed under it
  writes no membership; the exchange was still about it). A retired or merged category does not.
- A category minted at the act band touches the exchange that minted it.
- `project` is not a category and touches nothing here.
- The same judgments the filing acts on, and no others: answered by the model the pack pins, in a mode that
  acts. The exchange ledger is always on, and a task's turn is never judged by `onto.v1`. When the
  judgment's frame is written before the exchange's (the sink writing beside a running turn), the touch takes
  the judgment's turn and session at the message's own time.
- No judge call is added; the request is byte-identical.

The read is `names::topics::timeline(store, id, n)`: one bounded read of the touch index, newest first.

### History, filed once (`judge::onto::history`, `theseus ledger topics`, issue 46)

Touches began with the build that added them, so a topic's timeline started there. `theseus ledger topics` files
the exchanges from before, oldest first. It is the owner's act, in person, at his own terminal: the terminal alone
(the web UI, a chat, MCP and an unnamed connection are refused before any history is read), then the people
backfill's check (the owner, from a private place; the CLI refuses it inside a job), then the in-person rule (never
an agent's process). Each refusal is ledgered. Run `--dry-run` first: it counts the exchanges it would ask and
prices their worst case, and asks nothing.

- **Which**: an exchange that touches no category of a kind `onto.v1` asks, that this job did not ask before, that
  opens with a person's own typed message with words (not a task's, not Theseus's own, not a slash command), and
  whose session the ontology's judge acts on now (`[ontology] judge` on, the pack live).
- **What it asks**: the live point's `onto` question, once, about the opening message alone (no message before it,
  no reply, no attachment), through the judge's one client and its scrubber.
- **What it writes: touches only.** The ask goes in shadow, so the sink writes its row and files nothing: no
  membership, no category minted, no card, no `still_*` answer acted on. The job writes the touches with
  `names::topics::touches` and no plan, so a `new` answer names nothing. Each touch takes its exchange's start.
- **The message's words** reach the judge and nothing else: the row's context leaves out the names mined from the
  message, no state blob is kept, and the command prints counts and dollars. Nor does a word the answer brings
  back reach a row: the judgment is asked unrecorded and recorded only once its error's message, a malformed
  answer's reason and model, a model that is not the pinned one and the rate headers are withheld. Its class and
  its accounting stay (the outcome's class, a status, usage, cost and reservation), with the options' ids it
  chose; no other field is the answer's own words.
- **Resume**: each judgment's context says `purpose: history`. A rerun asks nothing of an exchange whose answered
  judgment says so, and writes its touches again from that row where they are missing.
- **Once**: one run at a time; a second, a dry run included, is refused while one goes and asks nothing. Each
  answer's row is written by the sink before the next exchange is asked and before the run returns, so the next
  run's plan reads every answer, one that touched nothing included. What cannot be promised: a daemon crash after
  the provider took a request and before its answer was stored may pay for that one exchange again.
- **Money**: the run's cap (`--cap-usd`, default $10) is checked before each call against the call's worst case,
  and the run stops before the exchange that would pass it. Each call reserves on the judge's day budget, which
  holds on the daemon's day ceiling, and settles at its price. Its calls are shadow's, and shadow's lines are
  `judge/spend.rs`'s, unchanged: its line is 70% of `[judge] shadow_limit_usd_per_day`, and a call past it is still
  made, with a `judge.over_line` row and a word to the owner at the line and at each multiple of it; the run stops
  at ten times the line (seven times the day's limit), or sooner at the daemon's day ceiling or the run's cap. Paced by the machine's quiet; the daemon's stop
  ends it between two exchanges, and a rerun goes on.

## Numbered items (`names::items`, the index's `entity::item`)

The index's extractor gains an `item` kind: `issue 33` and `issue #33` are `item:issue-33`, `PR 12` and
`pull request 12` are `item:pr-12`, a bare `#33` is `item:33`, and `owner/repo#33` is `item:owner/repo#33`. A
`#` after a word (`C#3`), anything inside a URL (every URL is blanked before the patterns run), Discord's
`<#id>` and a number of more than six digits are none.

Items are kept **apart from the entity field**: `entity::entities` is unchanged, so the index's exact arm and the
memory pass's labels (what recall and the gate read) see nothing new. The core asks for items by a method of
their own, `index.items` (the params and result of `index.entities`); a tender older than this build answers "no
method" and the exchange simply has no items.

Items are asked for apart from the other things, each with its own receiver, and given up after five seconds
(`names::items::WAIT`): a slow or stuck `index.items` never costs the things named beside them.

A bare number never makes an identity by itself. When the session the message *arrived at* (its owner's mark's
`arrived_session`; the running session only when there is no mark) serves a project, the scope is kept with the
exchange for the reply's items too, and an
unqualified item is scoped by it (`item:project:<id>#issue-33`); with none, it stays `item:issue-33`, an
unscoped candidate that a reader may offer and never treats as one thing. `owner/repo#33` names its repository
and is kept as said. The touches are written as for the other things: the message's when the exchange ends,
the reply's by the place's next exchange.

The exchange rebuild (`exchange.rebuild`) does not yet ask for items, so a rebuilt exchange has none.

## Scoped names (`names::table`)

A table of names built from the stores that hold them, with no word list and no pattern:

| Thing | Key | Names |
|---|---|---|
| a historical line of work (a stored thread, through the board) | `thread:<slug>` | its title, its slug |
| a task | `task:<id>` | its title; its bare id is qualified too |
| a topic (a category of a kind `onto.v1` files under, `project` apart) | its id, `topic:<local>` | its name |

The table is built for a caller (`table::build_for(core, gate)`): a thing whose record the gate refuses
(`card::Admits::thing`, with the thread, task or category record) is left out before any alias is resolved, so
it is no match, no candidate and makes no visible alias ambiguous. Old thread records remain readable, but
seals and moves no longer affect this admission. `table::build` is `build_for` with `table::Whole`, which admits everything.

The matcher reads a person's words. A **qualified name** (a key, or a task's id, as a whole token) wins and names
one thing. An **alias** is matched as whole words, case ignored, at most eight words and at least three
characters; a longer name said wins over a shorter one inside it, and each word belongs to one match. An alias is
accepted only when exactly one *live* thing holds it (an active thread not merged away, an open task, a category
not retired or merged); otherwise every holder is returned as a candidate, the live ones first.

## A thing's card (`names::card`)

One read: the title (`Topic: Ledger (topic:ledger)`, with `(closed)` for a thing no longer live), and under it at
most 12 lines: what it is about (a thread's goal, a task's objective, a category's description), up to five
current facts, up to two open tasks, the owner's guidance (a category's), and the newest three exchanges of its
timeline with the words that opened each, cut by code. A cut list says how many it left out.

- **Facts** are the owner's standing records whose scope is the thing's own place or session (a line of work's
  `thread:<slug>` place and its session, a task's session), live ones only (`standing::live`: a revoked,
  replaced or lapsed record is dropped). No stored record names a fact's subject yet, so a topic has no facts of
  its own until one does.
- **The gate is a parameter** (`card::Admits`) and covers the whole card: the thing's own record first (refused,
  there is no card at all: no title, goal, task, guidance or id), then every open task, the guidance, every
  standing record and every exchange, before it is read. A method a gate does not give denies.
- **Facts are decided inside the admitted history**: the gate filters the standing records first and only then
  is what is current resolved, so a refused correction ends nothing the card shows.
- **Exchanges are paged** back through the timeline until three are admitted or it ends: refused exchanges crowd
  out none it admits, and no hidden count is reported. The view's gate (`view::gate`) is meant to stand behind it.

## The view's sources (`view::sources_names`, `view::sources_waiting`, `view::sources_people`)

A turn builds the names table once, before assembly, with the core the turn's runner
is attached to, through the turn's own permit: every thread, task and topic is made a piece with its own marks
(a line of work's and a task's are its session's, as the gate reads them; a topic's are the owner's filing,
private and of no place a turn is anchored to) and asked of the gate first, so a refused thing is no name, no
candidate, and makes no alias it shares ambiguous. An exchange a card or "in play" reads is admitted only when
its session's current place and its own arrival place are readable. Its words are read only then, and the
sessions it was read from join the view's dependencies. Old seals and moves are not admission rules. The message is
matched against that table (a qualified name, or an alias one live admitted thing holds; candidates name
nothing), and its qualified numbered items are the ones the exchange's own naming told the turn
(`names::items::WAIT` bounds the wait); an unscoped item is a candidate and brings no card. Words the owner's
mark proves his may name any admitted thing; another person's, or unmarked words (an agent's process, an
import, a rebuilt exchange), name nothing private and no item. Words in play that are not his in a private
place (anyone's in company, his included) name only from that place's own namespace: the things the turn's
gate admits AND that are of that place, built before any alias is resolved, so nothing their audience could
not read is named or makes an alias ambiguous. A tool's result is never matched; text quoted inside his own
typed message counts as his message (so an ordinary alias in pasted prose may call a card for him alone). A card keeps each excerpt's author: words not his are
outside text inside it. A matched thing brings its card
(`names::card`, every part admitted first): its title, what it is, its current facts, its open tasks and its
newest exchanges, as one piece of slot H. What is in play (slot D) is the things the person's words name in the
newest three admitted exchanges here. A numbered item's card is its key and its admitted exchanges; an item
with none has no card.

What waits (slot C, `view::sources_waiting`) is the kernel's pending confirmations and the tasks waiting on the
owner or proposed for his yes, an id and one line each, each admitted with its session's marks before it is
read or depended on. Facts about the person (slot B, `view::sources_people`) are the held person the speaker is
(the owner by the place rule's owner handles): its name and the owner's guidance for it, marked as the owner's
filing. Its role line is not laid: no reader clears an operator's or an import's description by the memory
rules. Each text is read once for the turn and laid by the renderer as one block of its own provenance. A card
and a waiting line carry the sessions and exchanges they were read from. The view record checks that provenance
again before showing the piece (`card`, `waiting`); a record with none is unproven and neither shown nor counted.
Seals and moved-message records do not revoke a card. In the view, standing facts come through the standing
reader rather than the card (`ViewAdmits::fact` returns false). Authorship and name matching still describe where
a card came from; they are not a private-to-shared memory boundary.
