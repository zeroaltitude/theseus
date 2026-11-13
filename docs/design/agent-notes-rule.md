# The durable-versus-run-state rule for agent notes

_What decides whether one of an agent's own notes enters Theseus's memory (`b4-theseus-holds-its-own-notes`,
row B4, `theseus-da9hv`). Written before the bulk import runs, because the rule is the undone design the row
named: roughly seven hundred documents is not a judgement a builder should make unaided._

## Why there is a rule at all

Everything an agent needs in order to work **on** Theseus — how we build, what the gates are, how the lanes and
the cloud routine work, the standing decisions — should live in Theseus's own memory and not only in the agent
workspace it was written in. The row imports five sources. Four of them are small and hand-named: the agent-side
reference, the lane procedure, the cloud-routine procedure, and the shared log's decision lines. The fifth is not:
it is the whole of a workspace's memory notes and per-topic documents, about seven hundred files, and most of them
are not rules at all. They are what one run of one task left behind.

Importing those as memory would be worse than not importing them. A note that says *"ran the gate on the lane,
thirty-six reds, all of them the sandbox family, pushing now"* is true of one afternoon. Recalled six weeks later
beside a question about the gate it is not merely useless: it is testimony, with a date on it, that the gate reds
thirty-six tests. The durable notes and the run-state notes are mixed in one directory, written by the same hand,
in the same format, and only their content tells them apart.

So: **a note enters memory when what it says is still true after the work that produced it is finished.** It stays
out when reading it later tells you only what one run did. That sentence is the rule. The rest of this document is
how code decides it, what the code gets wrong, and what the owner has to look at before the bulk pass runs.

## The rule, as the code reads it

`crates/theseus-core/src/import/notes.rs` is the one implementation. It is pure: a note's **name** and its
**text**, in, and a reading out. It asks no model, reads no store, and spends nothing, so the same note always
reads the same way and a reading can be shown to a person before anything is written.

It asks three questions, in order, and the first that answers wins.

**1. Is it a dated log?** A note is a dated log when any part of its name is a date (`2026-11-09`, or a name that
begins with one) or is one of a closed list of log words — `log`, `logs`, `journal`, `daily`, `diary`, `standup`,
`worklog`. Then it is **run state**, whatever it says, and no amount of rule-shaped text overrides it. The reason
is not that such a note holds nothing durable; it is that tomorrow's log supersedes it, so each one is a snapshot
with a date and the series, not the file, is the memory. A rule that a dated log states is in the notes directory
twice — once in the log and once in the rules file that the log was written against — and the rules file is the
copy worth holding.

**2. Does it state rules?** Count the note's **rule lines**. A line is a rule line when it is **imperative** (its
first word is a bare verb from a closed list: `read`, `run`, `never`, `always`, `keep`, `use`, …) or **normative**
(it holds `must`, `must not`, `never`, `always`, `only`, `should`, `do not`, `don't`, `cannot`, `by default`,
`is required`, `every`, `each`, `prefer`, as whole words), **and** all three of:

- it has at least four words, so a heading or a bullet of two or three words counts for nothing;
- it does not say it holds for one thing only (`this task`, `this branch`, `this run`, `this PR`, … — the
  predicate is `card::evidence::one_task`, already in the tree for the owner card's own drafting, and reused here
  rather than written twice);
- it names no single run: no branch-shaped path (`lane/…`, `cloud/…`), no issue or pull-request number (`#4120`),
  no session or execution id, no commit-shaped hex token, no date.

A note with **three or more** rule lines is **durable**, and the reading says how many it found.

**3. Otherwise it is run state**, and the reading names the strongest run mark it found, or says the note states
no rule.

### The two numbers, named

`MIN_RULES = 3` and `MIN_WORDS = 4` are the only knobs, and both are judgement rather than measurement. Three is
the smallest count that a run log does not reach by accident: a log's imperative lines are its own next steps
(*"push it"*, *"rerun the gate"*), which are short, and the four-word floor takes most of those out, but one or
two survive in a long log. One rule line is not a rules document; three is. `MIN_WORDS = 4` is
`card::evidence::MIN_WORDS`, taken from there deliberately so that the two rules about an agent's own writing
agree on what a thin line is, and it counts whitespace-separated words exactly as that one does — not content
words, which is what an earlier draft of this document said and the code never did.

Neither number should be tuned against the seven hundred documents until the owner has read a sample of what the
rule does to them, because tuning a threshold on the data you are about to classify is the mistake the spec's own
proof rules name ("choose the rule before the data").

## What the code will get wrong

Written down because the owner is being asked to look at the rule, and a rule presented without its failures is
not being presented honestly.

- **A durable note written as prose** states its rules in sentences with no imperative and no modal — *"The gate
  takes the shared lock itself"* — and reads as run state. The rule under-keeps; the correction is to say so in
  the note, which costs the note's author one word (*"the gate always takes…"*).
- **A long log that quotes a rules file** can reach three rule lines inside a dated name, and question 1 drops it
  anyway. That is the intended direction: the rules file itself is in the same directory.
- **A log word inside a longer name is not a log word.** `logging.md` and `dialogue.md` are not dated logs: a
  name part matches only whole, so `log` matches `log.md` and `2026-11-09-log/note.md` and neither of those two.
  The shape to watch is the opposite one — a genuine run log named something else entirely (`notes.md`), which
  question 1 cannot see and question 2 then has to carry alone.
- **A note that is a reference table** — a list of ids, hosts, or ports, with no sentence — states no rule and
  reads as run state. Some such tables are durable. The rule does not catch them, and nothing here pretends it
  does.
- **Deduplication is not in the rule.** The row's own text asks for a dedupe of the roughly seven hundred
  documents, and this rule does not do it: it judges one note at a time. The import already skips an episode it
  holds with the same hash, so two byte-identical notes under one name import once; two copies of a rule under
  two names import twice. That is left open on purpose, because a dedupe that folds two notes has to decide
  which one's wording survives, and that is a second judgement the owner has not been asked for.

## What is enforced, and what waits

The rule's code is the authority, in the daemon, at the import. Nothing a client sends can go round it.

- `[import.notes] enabled` is **off by default**. While it is off, an `agent-notes` episode is refused and the
  refusal names what the rule read it as, with its reason. That makes the off state the dry run: run the import
  with the switch off and every note comes back classified, with nothing written. **That is what the owner is to
  be shown before the bulk pass, and the switch is what it waits on.**
- `[import.notes] keep_run_state` is **off by default** as well. With the path on, a note the rule reads as run
  state is still refused, and named. The owner turns it on only if he wants the run-state half too.

Both default off because this import widens what Theseus holds to our own operating notes, including lane and
infrastructure procedure, and the standing rule for anything that widens what Theseus may hold is that it lands
switched off.

## The other four sources

They are the same mechanism with none of the doubt: a handful of hand-named files, each of which is a rules or
procedure document, so the rule reads them as durable and the dry run shows that it does. They are in the same
switch because they arrive through the same path, and there is no reason to give four files a second door.

The Beads rows and the GitHub issues are not notes and this rule does not judge them: a row is durable until it
is retired, and the read path for them says so in its own words.

_Written by Tabitha/Claude for `b4-theseus-holds-its-own-notes`, at base `97310d4e`._
