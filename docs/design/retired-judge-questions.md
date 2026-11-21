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
- **Kept embedded**: `route.v10`, though `route.v11` acts. The route family's labelled cases file is written for
  it (`packs/route.v10.cases.toml`), `judge replay --cases` defaults to the file's pack, and the route tests
  hold `route.v11` to it question by question. Retiring it means re-pointing that file and those tests at
  `route.v11` first.
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
