# Retired judge questions and modes (issue 44)

What the trim of 2026-11-21 took out of the judge and its settings because nothing used it, and what stays readable.
Nothing here changes a question any pack asks today: each live version is byte for byte what it was.

## The packs

- **Out of the binary**: `classify.v2`, `classify.v3`, `route.v5` to `route.v8`, `content.v1` to `content.v3`,
  `security.v1`, `directive.v1` (versions the live ones superseded), and the two shadows that never graduated,
  `role.v1` (ROLE_GUESS at the inbound point) and `continue.v1` (CONTINUE at the compile point). They are listed in
  `theseus_judge::pack::RETIRED`, and their files are in `crates/theseus-judge/packs/retired/`.
- **Still readable**: a stored judgment of any of them decodes, because every builder and its input stay
  (`theseus_judge::builders::Input`), and it replays from its file by `--pack-file`. The tests read the builders
  through those files (`pack::retired`, also under the crate's `fake` feature for other crates' tests). The
  learning report and `judge.get` read a retired pack's rows as they read any pack's the build lacks.
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
- Both keys load whatever their value, are ignored, and are named in the same startup line.
