# Jev in the loop grows the ontology (theseus-f9c7r)

Design, not code. Written by Tabitha/Claude on 2026-10-19 against `origin/main` at `4ab80841`.
The owner approved the direction on 2026-10-19 (11:52 to 11:57). No model calls were made for this document.

---

## 0. The goal, in the owner's words

> "Jev is multi modal and can classify other things (esp images, maybe more!)."

> "The killer feature I call 'jev' is both this cool model, but also, that it is one of the earliest attempts to
> prove that putting fast, functional LLM classification in the turn/loop can vastly improve harness power."

> "Category expansion, not just category inclusion, is the name of the game."

He opened the cockpit's Ontology view and saw three kinds: person, channel, topic. That is what confused him.
Theseus has a kinds table built so that "a new kind is a row, not new code" (spec §4.1a), and nothing grows it.

**The approved shape.** Every message, image or attachment is judged by Jev on arrival: which project and thread,
which people, which topics, how private. That one judgment files the session into the ontology, routes the turn
and sets its context, with no extra model call on the turn's path. Projects and threads become kinds. Image and
media inputs are classified too. And we measure what each judgment changed (tokens, latency, accuracy, decisions
taken off the main model), as evidence for the thesis.

**Glossary, for the rest of this document.**
- *Jev*: TypeSafe's judgment model. It answers typed questions (a Choice among options, a Score on a rubric, a
  yes/no Noul) with probabilities. It never writes text. One call answers many questions at once.
- *Ontology*: the kinds table (person, topic, ...), the categories of each kind (a particular topic), a session's
  memberships (this session is about that topic), and guidance (text a category adds to the prompt).
- *Given* kinds are facts (which channel a session is in). *Interpreted* kinds are judgments (what it is about).
  Interpretations route context; they never grant access. That rule stays.
- *The inbound point*: the Jev request Theseus already sends for every message a person sends.
- *Act band*: the confidence above which Theseus acts on an answer without asking.

---

## 1. What exists today

| Piece | State on 2026-10-19 | Where |
|---|---|---|
| Kinds table | Four seed rows: guild, channel, person (given), topic (interpreted). No RPC adds a kind | `crates/theseus-ontology/src/kind.rs:357-421` |
| Guild | Seeded, never written: no guild category is made at a bind, and a channel's parent is never set | `crates/theseus-core/src/ontology.rs` (`bind_places`) |
| Who may write | Operator and import only. The origins `jev`, `sweep` and `dream` are refused by name as "not built" | `kind.rs:122-131`, `ontology.rs:156-247` |
| `categorize.v1` | At a private conversation's exchange end, every 10 human messages or after 30 minutes' quiet. Proposes a topic; the operator accepts or rejects. Writes nothing itself | `crates/theseus-core/src/judge/categorize.rs` |
| `people.v1` | A cheap model extracts candidate names, Jev selects, code verifies. A proposal, never a membership | `crates/theseus-core/src/judge/people/` |
| The inbound point | `classify.v3` and `role.v1` ride one request per human message; `route.v6` rides its own beside it. The turn waits at most `[threads] route_wait_ms` (200 by default, 300 on the owner's daemon) for the route verdict and the `thread` answer. The batch's other answers land when they land | `crates/theseus-core/src/judge/inbound.rs`, `docs/status.md:52-54` |
| Threads | Built and on: up to 50 live, five live on the owner's daemon. A thread is a place; `classify.v3`'s `thread` question routes an unaddressed message to one | `crates/theseus-core/src/threads.rs`, `thread_route.rs` |
| Projects | Built: a TASK record of kind `project`, with milestones and steps; a conversation attaches to one and its request ends with the project's slice | `crates/theseus-core/src/project/mod.rs` |
| Attachments | A message node carries attachments; an image is a blob stored once by its SHA-256. The inbound state sends none of it to Jev | `crates/theseus-core/src/node.rs:88-115` |
| Jev and images | Our reference (2026-09-21) observed Jev as text only. The owner says it is multimodal now. Unverified | our Jev reference notes (kept outside the repo) |
| Cockpit | `/ontology`: kinds, the category tree, add a topic, set guidance. No kinds added, no origin shown, no history | `cockpit/src/views/Ontology.tsx` |
| Thread bench | Built, not yet run on the real model | `bench/threads/` |

Two facts shape everything below. Jev cannot name a thing: it picks among options we give it. And the turn's
hot path already waits on one Jev request per message, so there is a free ride for more questions.

---

## 2. The design in one picture

```
 a message (text, images, files) arrives
      │
      ▼
 inbound point: ONE batched request (classify.v3 + role.v1 + onto.v1)      route.v6 (own request, as today)
      │  onto.v1 asks, per kind: which category? a new one? still these?
      │  and for images: what is it, who/what does it show, how private?
      ▼
 the turn waits ≤ route_wait_ms for `thread` (as today); the rest lands off-path
      │
      ├── act band ───► memberships written (origin `jev`), category minted when `new` ──► one-line footer; undo
      ├── confirm band ► a card: "file under X?"  (nothing written)
      └── below ──────► recorded only
      │
      ▼
 next recompile: the compile walk composes the new memberships' guidance; recall boosts same-category notes
      │
 nightly sweep (origin `sweep`): re-associate, merge, split, rename; cluster "other" into a new KIND
 nightly dream (origin `dream`): guidance for a category from its sessions
```

---

## 3. Expansion: Jev proposes and mints categories, and kinds

### 3.1 Categories

**Fit.** For each interpreted kind Jev may assign (section 6), `onto.v1` asks one Choice: the candidates (at most
20 for the kind), plus `new` and `none`. Candidates are chosen in code: the session's own categories first, then
the thread's, then those whose name or description matches words of the message (a cheap text match), then the
most recently used. The state stays under about 3,000 tokens.

**New.** Jev cannot write a name, so a new category needs one from somewhere else. Two sources, in order:

1. **Code-mined names, free.** The message and its session already hold names: the thread's title, the project's
   title, repository and directory names, hosts and service names, capitalised terms, file paths. Code lists up to
   10 of them, and the same request asks `<kind>_name`: a Choice among them plus `none`. If Jev says `new` at the
   act band and names one at the act band, the category is minted in the same frame as the membership. No extra
   call of any kind.
2. **A naming call, off the turn's path, rare.** When Jev says `new` but no mined name fits, Theseus asks a cheap
   US-based model (`[ontology] name_profile`, default `haiku`) for three names and a one-line description, the
   way `people.v1`'s extract step already works. The next `onto.v1` judgment of that session offers those names.
   Jev confirms; code mints. This is the only model call the design adds, it is never on the turn's path, it is
   priced and counted like every judgment, and it runs at most once per session per kind per day.

**Mint = act, then tell, then undo.** A minted category is a `Category` record with `added_by = jev:<judgment id>`,
a ledger row, and a membership in the same frame. The reply's footer says `filed: topic › theseus › ontology (new)`.
A mint also sends one card to the owner's DM, with an undo button; a plain membership does not (the daily digest
lists them). Undo retires the category if nothing else uses it, restores the previous member list, and writes an
`ontology.undo` row whose label (`wrong`) feeds the pack's learning ledger. One click in the card, `/undo` in
chat, or the cockpit's button.

**Split, merge, rename** are not decided on the hot path. They need more than one message of evidence, so they
belong to the sweep (section 4.4): near-duplicate pairs get a `same_thing` Noul and merge through the merge
machinery that people already have (`merged_into`, the `ontology.merged` row, undoable); a large category gets a
`two_things` Noul over a sample of its members' titles and, when yes, two children named by the sources above;
a rename is a Choice among mined names with the old name kept in the description. Each is act, tell, undo.

### 3.2 Kinds

A kind is a row in the table. Today only the operator may write one, and only through the store (no RPC). The
design adds `ontology.kind.add` and `ontology.kind.retire` for the operator and the cockpit, and lets the sweep
mint one.

**When Jev proposes a kind.** `onto.v1` asks one more Choice each message: `kind_of_thing`, "what kind of thing is
this message mostly about?", over the kinds in the table plus `other`. An `other` at the act band is recorded with
the message's mined names. The nightly sweep clusters the day's `other` answers (the same mined name, or a
`same_kind` Noul over pairs). A cluster of three or more sessions mints a kind: interpreted, assigned by `jev`,
`sweep` and the operator, per session 3, rule `chain`, precedence after the last row, named from the cluster's
names (a Choice again), with its first categories the cluster's. Act, tell with a card, undo (retire the kind and
its categories; nothing in the table may be retired while a session's membership names it).

**The reader rule holds.** "Nothing declared without its reader" is satisfied by the compile walk itself: every
interpreted kind's memberships compose guidance under `chain` or `intent_line`, and every category is a recall
filter and a cockpit group. A minted kind has a reader on the day it is born. It just has no guidance yet.

**Kind schema Jev can extend.** The row gains two optional fields, both with serde defaults so the store format does
not bump: `examples` (up to five category names, shown to Jev as the kind's meaning, since question ids are not
shown to the model) and `mined_from` (which name sources apply: `titles`, `paths`, `hosts`, `capitalised`). The
sweep sets both when it mints; the operator may edit them.

### 3.3 Where the keel is

Interpretations never grant access, so Jev's ontology writes touch no policy and are not keel. Three things are:

- the judge's day budget, if the new pack needs it raised (`[judge]` spend lines: a yes);
- the private mark written from an image judgment (section 5): it changes what later turns may send and where,
  so it is a policy change, and its first switch-on takes the owner's yes;
- any store format bump. None is planned: new rows and optional fields only.

---

## 4. The in-loop judgment: `onto.v1`

### 4.1 Which request it rides

`onto.v1` joins the inbound batch beside `classify.v3` and `role.v1`: one state, one request, cost split by
question count, as the batch already does. It does not join `route.v6`'s request, which is kept small because the
turn waits on it. The turn's wait is unchanged: it waits for the `thread` answer and the route verdict, at most
`route_wait_ms`, as today. Memberships are written by the sink's task off the turn's path, in the judgment's frame
with its row, as `categorize.v1`'s mark is.

**Latency gate.** TypeSafe's documentation says more questions in one request barely change its latency. We do not
take that on faith. The `judge.call` row already records each call's time. Row 3 ships with a one-week measurement
on the owner's daemon: the batch's p50 and p95 before and after. If p95 grows by more than 50 ms, `onto.v1` moves to
a third request beside the batch, which the turn never waits on. Either way the turn's own wait does not change.

### 4.2 What `categorize.v1` becomes

`categorize.v1` keeps running at the exchange end in shadow for one more release, as the comparison arm: the same
sessions judged two ways. Once row 8's numbers show `onto.v1` at or above it on the labelled set, `categorize.v1`
retires. Its proposals surface and its labels are reused.

### 4.3 The output schema

State (JSON, named fields, scrubbed as the inbound state is): the message, the author's class, the place kind,
the thread's title, the project's title, the previous human message and last reply trimmed, the candidates per
kind with descriptions, the session's current memberships (at most 5 per kind), the mined names, and for an image
the image itself (section 5).

Questions, all in one request:

| Question | Type | Options | Used for |
|---|---|---|---|
| `topic`, `project`, `event`, `resource`, `capability`, `expertise` | Choice | ≤20 candidates, `new`, `none` | the membership |
| `<kind>_name` | Choice | ≤10 mined names, `none` | the mint |
| `still_<kind>` | Noul, one per current membership (≤5) | | re-association: dropped after two answers under 0.2 |
| `kind_of_thing` | Choice | the table's kinds, `other` | kind proposals (sweep) |
| `personal` | Score | none / mentions / discloses / sensitive | the private mark; theseus-gasov's warning |
| `elicits` | Noul | | theseus-gasov: a reach for the owner's details |
| `image_subject` | Choice | code, chart, photo, screenshot, document, meme, other | filing; which questions apply |
| `shows_people`, `shows_screen_or_document`, `shows_identifiers` | Noul | | the private mark |

About twenty questions. `project` is asked only when the thread has no project; `thread` stays `classify.v3`'s.
Questions whose evidence is absent are left out, so a text-only message's state is byte for byte today's plus the
candidates.

**Confidence bands** (`[ontology]`, defaults): act at 0.80 and above, confirm from 0.55, record below. The three-band
gate from TypeSafe's own guidance, tuned on our labels after row 8, never before. The model version is pinned in the
pack file.

### 4.4 Writing with origin `jev`, and the sweep and dream

- `Origin::Jev`, `Sweep` and `Dream` stop being refused (`kind.rs:122-131`). `Ontology::check` admits a category
  and a member list from an origin the kind's `assigned_by` names; the kinds table rows name `jev` where section 6
  says so. The ledger rows carry the origin and the judgment id, so every write is traceable to its answer.
- A membership written by Jev carries its confidence. A later judgment of the same session supersedes the list,
  as the operator's does; an operator's membership is never overwritten by Jev's (the operator's origin wins on
  conflict, and Jev's answer is recorded as a disagreement label).
- **The sweep** (`[ontology] sweep`, nightly, origin `sweep`), built on the people sweep's shape
  (`judge/people/sweep.rs`): re-asks the day's sessions with the whole day as evidence, merges, splits, renames,
  decays memberships no judgment has confirmed in 30 days, and mints kinds (section 3.2). Every write is undoable
  and listed in the morning digest.
- **The dream** (origin `dream`, M6's nightly consolidation): for a category with five or more sessions and no
  guidance, a synthesis of what those sessions settled becomes its guidance, at most 400 characters, with a card.
  This is a model call inside the dream's existing budget, never a new one.

---

## 5. Multimodal: images and attachments

**What is verified and what is not.** Our reference saw Jev as text only on 2026-09-21. The owner says it is
multimodal. Row 1 settles it with one live probe: the request shape for an image, the size limit, the price, and
whether a `Choice` over an image answers sensibly. The result goes into the pack file's comment and `refs/jev.md`.
If Jev still takes text only, the fallback is a short caption from the naming profile (`haiku`, US-based), off
the turn's path, and Jev judges the caption. The questions and everything downstream are the same either way.

**What is sent.** For each image attachment (at most four per message): the image, re-encoded in code to at most
1,024 pixels on the long edge and 1 MB, with all metadata stripped (EXIF, GPS, maker notes), plus the message text
and the candidates. A file larger than 20 MB is skipped and the row says so. For text-like attachments (plain
text, Markdown, code, PDF text, CSV): the first 2,000 characters, extracted in code. Audio is already transcribed
before it reaches the turn, so it is text. Video is out of scope.

**Privacy and data rules.**
- The place rule first: nothing from a shared place is sent with the session's interpreted memberships, as today.
  An image posted in a shared place is judged only for the `image_subject` and privacy questions, with no
  candidates from any private session.
- The owner's consent of 2026-09-30 covers sending session content to Jev. Images are session content, but they
  can show people, screens and documents, so the design asks him once more (decision 2).
- Jev is a US-based model, so the US-origin rule (theseus-xs5ah: `[privacy] sensitive_names` go to US models only)
  is satisfied. The China-origin rule is untouched by this design, and no image or attachment goes to any model that rule excludes.
- **The private mark.** A `personal` score at `discloses` or above, or `shows_people`/`shows_identifiers` at the act
  band, marks the node private (the private-node mark of theseus-a1wvq). From then on, a request to a model outside
  the US allowlist carries nothing before that node, as the rule already says. The mark is tell-after with undo and,
  being a policy effect, its first switch-on is keel (section 3.3).
- The elicitation Noul feeds theseus-gasov's warning, so one judgment serves both designs.

---

## 6. The kinds to add first

Precedence orders a compile: a higher number's guidance comes later and governs.

| Kind | Basis | Memberships come from | Per session | Parent | Precedence | Rule | Jev learns it? |
|---|---|---|---|---|---|---|---|
| guild | given | transport; **filled**: the category made at a place's first bind | 1 | – | 10 | chain | no |
| channel | given | transport; **parent set** to its guild at bind | 1 | guild | 20 | chain | no |
| thread | given | the harness: the thread place a session lives in | 1 | channel | 25 | chain | routes it (`classify.v3` `thread`, today) |
| project | given | the harness: the project a thread or task is attached to | 1 | – | 28 | chain | attaches it: `onto.v1`'s `project` question, when the thread has none, at the act band |
| person | given + stored | transport, operator, import, **jev** (`people.v1` at the act band writes, not proposes) | many | – | 30 | intent_line | yes |
| topic | interpreted | operator, import, **jev, sweep, dream** | 3 | topic | 40 | chain | yes, with mint |
| event | interpreted | jev, sweep, operator: a dated happening (a release, an outage, a meeting, a deadline) | 3 | project | 50 | chain | yes, with mint; the date is code-mined, never Jev's |
| resource | interpreted | jev, sweep, operator: a repository, service, host, document, dataset the session works with | many | – | 60 | intent_line | yes; candidates code-mined, Jev confirms |
| capability | interpreted | jev, sweep, operator: a tool, skill or extension the session needed | many | – | 70 | intent_line | yes; candidates from the session's tool calls and the extensions table |
| expertise | interpreted | jev, sweep, operator: the domain the session draws on | 3 | expertise | 80 | chain | yes, with mint |
| culture | interpreted | operator, dream: how things are done here, learned from corrections over time | many | – | 90 | chain | not per message: the dream proposes from clusters of the owner's corrections |

**Given by the harness** is new wording, not a new basis. `thread` and `project` are facts the core holds, read at
compile from the thread and project records the way channel is read from the place, never stored per session and
never set through the API. `GIVEN` grows from three names to five, and `check_basis` learns the two new seeds.

**Role** stays the role table (`role.v1`, step 26c's versioned table) for now. Folding it into the kinds table is a
later row, after the table's live switch, and is out of this design's scope.

---

## 7. How the ontology feeds routing, context and recall

- **Routing.** The thread is routed as today. The `project` answer attaches an unattached thread to a project at
  the act band, so its slice (goal, milestones, next steps) reaches the request on the next compile. The persona
  question of `route.v6` gets the session's category names in its state, as evidence, with no new question.
- **Context compilation.** The compile walk is unchanged in shape: it composes the guidance of every membership by
  precedence. New kinds add rows, not code. A membership written by Jev takes effect at the next recompile, never
  on an append, so the prompt cache survives it (the standing rule for interpreted memberships). The manifest
  records each membership used with its origin and confidence, so a turn can say why it got what it got.
- **Memory recall.** Recall's candidates gain one term: a note from a session that shares a category with this
  session gets a reciprocal-rank boost, the way activation's reached hits do, weighted by the kind (project and
  thread highest, topic next, the rest low). The place rule and every other filter run unchanged. A thread's recall
  from other threads (`thread_recall.rs`) uses the same term, and still names its source. Nothing here changes
  what may be read.

---

## 8. The cockpit: an Ontology view that shows growth

The view keeps its three panels (kinds, the tree, guidance) and adds:

- **Growth.** A small chart: categories and kinds added per day over the last 90 days, stacked by origin
  (operator, import, jev, sweep, dream), read from the `ontology.*` ledger rows. Beside it, three counts: kinds,
  categories, and the share Jev minted.
- **What Jev minted.** A list, newest first: the category, its kind, the session, the judgment's confidence, a link
  to the judgment's row, and an **undo** button that calls `ontology.undo`. Undone items stay listed, struck through.
- **Origin and confidence on every membership** in the tree and in a session's deck, with a filter by origin.
- **Add a kind.** A form for the operator (`ontology.kind.add`), confirmed with the words it sends, as a topic is.
- **The pack's line** in the Observatory's learning report: judgments, acts, confirms, undos, and the undo rate per
  kind, which is the accuracy number the owner sees daily.

---

## 9. Measurement: evidence for the thesis

Three measurements, each a number in a report under `docs/benchmarks/` (every benchmark run gets a report).

1. **The judgment's own cost.** Per `onto.v1` judgment: the batch's added latency (ms, p50 and p95 against the
   week before), tokens, dollars, the band it landed in, and what it changed (membership written, category minted,
   project attached, private mark, nothing). A daily line in the learning report.
2. **Accuracy, from ground truth we already own.** `theseus import topics` wrote import-origin memberships from the
   owner's past history's labels. Row 8 replays those sessions through `onto.v1` with the labels hidden
   (`theseus judge replay`) and reports precision and recall per kind against the import's labels, plus the undo
   rate on live sessions after a week. That is a labelled set with no labelling work.
3. **The thesis A/B, on the thread bench.** The bench (`bench/threads/`) gains an arm, `+onto`: the same six
   interleaved streams, with the ontology filing, project attachment and category-boosted recall on. Against the
   bench's existing arms it reports per-stream correctness, contamination, routing accuracy, context tokens and
   dollars per turn, and latency. One more column, **decisions taken off the main model**: the count of routing,
   filing and privacy decisions per turn that the oracle arm makes by asking the main model and `+onto` makes with
   Jev, with the dollars each way. This is the headline number for "fast classification in the loop improves
   harness power", and it is reported whatever it says. Nothing is tuned to the bench.

---

## 10. Rows

Each row is sized for one Sonnet cloud session, with its scope, files, tests and dependencies. Every row runs
`scripts/gate.sh` before its commit and lands as a plain merge through the join queue.

| Row | Scope | Files | Tests | Needs |
|---|---|---|---|---|
| **R1. Jev image probe and the `onto.v1` pack** | One live probe of Jev with an image (shape, limit, price, a sensible Choice); the pack file with section 4.3's questions; the state builder with the candidate picker and the name miner; the fake judge's scripted answers | `crates/theseus-judge/packs/onto.v1.toml`, `crates/theseus-judge/src/builders.rs` (an `OntoInput`), `src/names.rs` or a new `mine.rs`, `src/fake.rs` | builder round-trips; the miner on five message shapes; a scripted judgment parses into bands | – |
| **R2. The ontology crate: origins built, kinds filled** | `jev`, `sweep`, `dream` built; `check` admits categories and member lists from an origin the row names, operator wins on conflict; the kinds rows of section 6 (guild filled at bind, channel parent, thread, project, event, resource, capability, expertise, culture); `GIVEN` to five; `examples` and `mined_from` fields; the undo record and `ontology.undo` ledger row; kind retire | `crates/theseus-ontology/src/{kind,category,ontology,refusal}.rs`, `tests.rs`; `crates/theseus-core/src/ontology.rs` (`bind_places`, `given`), `fact/ontology.rs`, `crates/theseus-protocol/src/ledger.rs` | every refusal that changes has a test; a Jev write of a kind not naming it is refused; undo restores the list byte for byte; load of a store with the old rows still serves | – |
| **R3. The in-loop wire** | `onto.v1` in the inbound batch; the sink writes memberships and mints at the act band in the judgment's frame; the confirm card; the footer line; the naming call off-path; `ontology.undo`, `ontology.kind.add`, `ontology.kind.retire` RPCs and `theseus ontology` subcommands; `[ontology]` config; the one-week latency measurement and its report | `crates/theseus-core/src/judge/{inbound,sink,mod,warm}.rs`, a new `judge/onto.rs`, `rpc/ontology.rs`, `config/ontology.rs`, `crates/theseus-protocol/src/lib.rs`, the CLI | scripted Jev at inbound writes a membership and a mint; a confirm-band answer writes nothing; undo through the RPC; the state without attachments is byte for byte today's plus candidates; the turn's wait is unchanged (a timing test with the fake judge) | R1, R2 |
| **R4. Multimodal inputs** | Images and text attachments into the state per section 5 (re-encode, strip metadata, limits); the privacy questions; the private mark and its switch (off by default until the owner's yes); the shared-place reduction | `judge/onto.rs`, `node.rs` readers, a small image module (re-encode with the `image` crate already in the tree, or document the dependency), `scrub.rs` | a PNG with GPS metadata leaves without it; a 25 MB file is skipped with its row; a shared-place image gets no candidates; the mark sets and undoes | R1, R3; the owner's yes for the mark |
| **R5. The sweep and the dream** | The nightly sweep: re-association, merge, split, rename, decay, kind minting from `other` clusters; the dream's guidance proposal; the morning digest lines | `judge/onto_sweep.rs` after `judge/people/sweep.rs`, `consolidate.rs` or the dream's module, `digest` | a planted duplicate pair merges and undoes; three `other` sessions mint a kind; a lone one does not; guidance stays within 400 characters | R3 |
| **R6. Routing, context and recall** | The `project` answer attaches a thread; category names in `route.v6`'s persona state; the compile walk over the new rows (a manifest test); recall's category boost and its weights | `judge/inbound.rs`, `thread_route.rs`, `compiler` walk, `recall.rs`, `thread_recall.rs` | a scripted `project` answer attaches; a same-category note ranks above an otherwise equal one; the place rule still drops a shared place's candidate; a golden manifest for a session in six kinds | R3 |
| **R7. The cockpit** | Growth chart, the minted list with undo, origin and confidence on memberships, the kind form, the Observatory line | `cockpit/src/views/Ontology.tsx`, its hooks, `cockpit/src/api`, the learning report | cockpit tests; the dry run's `tsc`; a snapshot of the growth panel from a fixture ledger | R2, R3 |
| **R8. Measurement** | The import-labelled replay with precision and recall per kind; the `+onto` bench arm and the "decisions off the main model" column; the first report | `crates/theseus-judge/src/replay.rs`, `bench/threads/th_drive.py`, `th_score.py`, `report.py`, `docs/benchmarks/` | the replay on a fixture with known labels scores 1.0; the arm runs on the scripted model in the smoke test | R3, R6 |

R1 and R2 run in parallel. R3 is the spine. R4 to R8 run in parallel after R3 (R8 waits for R6).

Filed on 2026-10-19, each depending on theseus-f9c7r and waiting for the owner: R1 theseus-s1vkl, R2 theseus-e3mjc,
R3 theseus-xzxqv, R4 theseus-und4g, R5 theseus-63tfk, R6 theseus-jrf4c, R7 theseus-dd3kw, R8 theseus-mqqc2.

---

## 11. What needs the owner's yes

Everything else in this document is settled by a stated principle (act first, FAST, default trust, interpretations
never grant access) and can be overruled afterwards.

1. **The naming call.** Jev cannot write a name. When no code-mined name fits, Theseus makes one cheap US-model call
   off the turn's path (`haiku`, about a tenth of a cent, at most once per session per kind per day). This is the
   one model call the design adds. Default: yes.
2. **Images to Jev, and the private mark.** Images from private places go to Jev with their metadata stripped; a
   judged-private image or message marks its node private, which changes what later requests may send to models
   outside the US allowlist. The mark is a policy effect, so it ships off until he says on. Default: yes to both.

---

## 12. Validation gates and non-goals

- `scripts/gate.sh` on every row; the sim's standing scenarios unchanged; the store loads a pre-design store with
  no dropped records (R2's load test).
- Nothing on the start path. Nothing new waited for on the turn's path. A failing or slow Jev changes no request
  byte, as today.
- No store format bump. If one becomes necessary, it stops at the keel.
- Out of scope: video; shared-place memberships; folding the role table into the kinds table; Slack; tuning any
  threshold before row 8's numbers exist.
