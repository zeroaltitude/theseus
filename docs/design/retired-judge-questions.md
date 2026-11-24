# Retired judge questions and modes (issue 44)

What the trim of 2026-11-21 took out of the judge and its settings because nothing used it, and what stays readable.
No live pack's questions, state or per-pack request changed: each live version is byte for byte what it was.
What did change on the wire: the inbound batch no longer carries `role.v1`'s question, so it asks one question
fewer, and each remaining pack's share of that call's cost is its questions over the smaller total (`classify.v4`
alone in the batch now pays the whole call where it paid six sevenths); and a compile whose candidate signal fired
no longer makes `continue.v1`'s call of its own.

## The packs

- **Out of the binary**: `classify.v2`, `classify.v3`, `route.v5` to `route.v8`, `content.v1` to `content.v3`,
  `security.v1`, `directive.v1` (versions the live ones superseded), and the two shadows that never graduated,
  `role.v1` (ROLE_GUESS at the inbound point) and `continue.v1` (CONTINUE at the compile point). They are listed in
  `theseus_judge::pack::RETIRED`, and their files are in `crates/theseus-judge/packs/retired/`.
- **Still readable**: a stored judgment of any of them decodes, because every builder and its input stay
  (`theseus_judge::builders::Input`). Readers scan every family the build ever had, embedded or retired
  (`pack::readable_ids`), so the learning report, `judge.list`, the learned versions and the ladder's mode rows
  still read `role` and `continue`, the two families with no embedded version left. `judge.get` reads any row.
- **Not replayable**: a replay finds its incumbent by the stored judgment's pack name, and a retired version is
  not in the binary, so replaying its judgments is refused ("this build has no pack …"). `--pack-file` gives the
  replay's candidate, not its incumbent.
- **Their rollback rules stay**: a stored adoption or mode row of a retired version (a live `route.v8`, say) is
  checked by that version's own rules (`pack::retired_rollback`, the files' `[[rollback]]` tables), never by the
  wired version's.
- **One file's text changed**: `packs/retired/security.v1.toml`'s two header comment lines named a person, which
  the public repository may not carry, so they say "the owner". The digest of the whole file therefore differs from
  the one its stored judgments recorded; `pack::SAME_TEXT` records exactly that pair as one text, and a replay's
  digest check accepts that pair and no other.
- The tests read the builders through those files (`pack::retired`, behind the judge crate's `retired-packs`
  feature, which only dev-dependencies enable: no build of the daemon or the simulator carries them).
- **A settings file** that still has a `[judge.packs."<retired>"]` table loads; the table is ignored and named in
  one line at startup.
- **`route.v10`, retired later** (issue 46): its file is under `packs/retired/` like the others, its rollback
  rules are in `pack::RETIRED_ROLLBACK` (a stored adoption of it is checked by them), and its ladder adoption
  row went. The route family's labelled cases file keeps its name (`packs/route.v10.cases.toml`, which the bench
  and the scripts read) and now declares `route.v11`, the acting pack, which `judge replay --cases` asks by
  default. The file is still read for v10's own text (`route_cases::read_as`, both ways), so
  `--pack-file packs/retired/route.v10.toml` replays it.
- **`recall_cross.v1`, retired with the cross-thread recall judge** (XD3): no turn asked it after the cross step
  went, so its service, its wiring and its own breaker went too. Its file is under `packs/retired/`, its builder
  and labelled eval set stay (a stored state still reads; the set's tests read the retired file, as
  `content.v1`'s do), and the ladder's
  `recall_cross` day brake stays in `ladder::adopt`, so an old adoption or mode row is checked as before.
- **`memory.v1` and `attribution.v1`, retired with the memory pass's shadow judgments** (D8a): the pass
  asked both, off the turn's path, and recorded answers nothing applied; the deterministic labels and
  attribution were always what the pass wrote. Their asking (`judge::memory`, the pass's two askers, the
  default wiring) went; their files are under `packs/retired/`, their builders stay (a stored state still
  reads), and old `judge.call` rows scoped `judge:memory` and `judge:attribution` still list and decode.
- **Kept**: `categorize.v1`. It is a shadow too, but its exchange-end point also carries the core handle, the due
  rule and the marks that `people.v1`, `people_seen.v1` and the card's observer read, and
  `ontology.proposals` still lists and answers the proposals it already made.

## The compile's signals

`continue.v1` was the one reader of the compiler's candidate signals (`dormancy`, `tail_band`, `report`, `wake`,
`cache_miss`), so they are no longer computed. `context.compiled` keeps its `signals` field so a row an older daemon
wrote still decodes; this build writes it empty, which the wire omits. `[judge.signals]` loads, is ignored, and is
named at startup.

## The modes

- `[routing] rung` (`live` or `shadow`) went: the route pack's own `[judge.packs."<pack>"] rung` lowers it.
- `[memory] small_talk_bar` went. It was never set; the shadow count it would have promoted
  (`RecallManifest::would_drop`) stays, and a `recall.withheld` row with `why = "small_talk"` still reads.
- Both keys load whatever their value, are ignored, and are named in the same startup line, which says what
  changes where the old value did something: a file that set `rung = "shadow"` is told that routing now follows
  the route pack's own `[judge.packs."route.v11"] rung` and may act live (an unpinned turn can switch model where
  it only recorded the verdict before); one that set `small_talk_bar = true` is told that small-talk turns now
  carry admitted recall (the notes a greeting drew reach the model where they were withheld).

## Issue 46: the live packs lose the questions nothing reads

Each new version below is a new file: the old file minus whole question tables and the rules named, with its own
header comment; every question it keeps is the old one, field for field (a test parses both and says so). The old
file moves to `packs/retired/` byte for byte, so its digest is the one its stored judgments hold, and it is listed in
`pack::RETIRED` with its rollback rules copied into `pack::RETIRED_ROLLBACK`. Its judgments still list and read; a
replay of them is refused, as for every retired version. Each new version is live on landing, the standing rule in
`docs/development.md`. A settings table that names an old version loads, is ignored and is named at startup; no
alias carries it to the new name, so rename it.

- **`route.v12`** is `route.v11` without `seal` and `personal` (no code read either answer) and without the
  `on_path_p95` rule (no code fed it). The `model` instructions lose "and never a model the state lists as not
  allowed", and the `deepseek_flash` option loses "or when the state says China-origin models are not allowed":
  the daemon no longer writes `restricted_models` into any state (the route slice, or the inbound batch's shared
  state), since no model is kept off a session for its origin. The field stays on `RouteFacts`, so a stored input
  and a case file that set it still read. The ladder adopts v12 (`ladder::adopt`, `WHY_ROUTE_V12`); the learning
  loop leaves the route pack alone by its constant (`lineage::LEFT_ALONE`), not by a literal name. The labelled
  cases file declares `route.v12` and is read for v10, v11 and v12 alike. A pinned turn with fewer than two
  personas asks the route pack nothing; its judgment is recorded as skipped for want of a question.
- **`classify.v5`** is `classify.v4` without `thread` and `scope`: no turn offers live threads since a message
  goes to a thread by its address only, so neither could be asked. With them go the "wrong thread" rollback rule
  (no code writes that label) and the `thread_route` action (no code reads a pack's action): `action = "none"`.
  The readers that look a message's classify answers up by name (the exchange row's judged answers, the task its
  words addressed, the offer of a task) read the wired version or the one it replaced
  (`inbound::reads_as_classify`), so a message judged by `classify.v4` just before the upgrade keeps its
  answers; a learned version's are not read, as before. `classify.v4`'s labelled set stays its own frozen record.
- **`onto.v2`** is `onto.v1` without `personal_applies`, `personal`, `elicits`, `shows_people` and
  `shows_identifiers`, the questions that existed for the restricted-provider rule. Their one reader, the private
  mark (`judge::onto::mark`), went with them: it decided what later requests could send to models outside an
  allowlist, ran only under `[ontology] private_mark` (off by default), and even on wrote one trace line.
  `[ontology] private_mark` loads, is ignored, and is named at startup. The two picture questions nothing reads
  yet, `image_subject` and `shows_screen_or_document`, are still asked: whether they become written labels or
  go is the owner's decision. `judge replay --onto` with no candidate replays the wired onto pack by its
  constant, and the cockpit counts the onto pack's judgments under the wired name.
