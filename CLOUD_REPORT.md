# Cloud report: one exam, one probe (theseus-vm3n.4)

Branch `cloud/20261003-exam-trims`. Commits:

- `314c5c0` exam: one exam and one probe; exam-v1 and the BM25 port are gone (items 1 and 2 together: the BM25
  port's tests ran on exam-v1, so the two cuts could not be green apart)
- `593ef0a` exam: the memory arm is the scratch daemon's `[memory] arm`, not a turn.submit field (item 3)

## 1. exam-v1 deleted

**Changed.** Removed `exam/exam-v1.toml`, `EXAM_V1`, `Family::V1`, `--exam v1`, `--against v1`, and the tests that
pinned v1: the "40 items" shape test, the "38 items word for word" check inside the v2 shape test, the fixture test
over v1's 36 pasts, and daemon_reads' v1 test. `--exam` now takes only a file path (default: the built-in exam).
Tests that used v1 only as a convenient exam now use `EXAM_V2` (their 38 shared items are unchanged): the check
test, the drive order test (now sized from the exam: 72 × 2 × 3 = 432 cells), the tool-node fixture test, and the
report tests (the report test's hand arithmetic is redone for 72 items: 18 pass under none, sd √(.25 × .75 × 72/71)
= 0.43606, se 0.051392, t(71) = 1.9939, half-width 10.2: none 25% [15, 35], headroom +75 [+65, +85], 54/0/18).
The constant name `EXAM_V2` is kept (the exam's own version string is still `exam-v2`).

## 2. BM25 probe deleted; `probe` is the tender probe

**Changed.** `probe.rs` lost the corpus, BM25, the two tokenizers' ranking, the recency rank, and the in-process
`render` and `compare` (706 → 114 lines). It keeps `KS`, `ItemProbe`, `Recall`, `recall`, `rows` (what `tender.rs`
reads), with a hand-computed test. `probe` now requires `--tender` and `--manifest`; `--tokenizer`, `--half-life-days`,
`--against` are gone.

**Deviation, please check.** The stemmer and tokenizers are not only the probe's: `item.rs` validates the
paraphrase and scale families with `content_stems` (a paraphrase item's task and gold share no content word, even
stemmed, under either tokenizer). Deleting it would weaken what the exam checks of its own items. I moved those
~50 lines unchanged into `src/words.rs` (stop list, dotted-run regex, split tokenizer, stemmer, `content_stems`),
with two small tests. Their behaviour is the same (the exam still loads and every family check passes). If you
want them cut anyway, the family definitions in `item.rs` need another way to say "shares a content word".

`rows()` labelled a group "exam-v1's families"; it now reads "the ten base families" (tender probe output only).

**How the exam starts a tender.** It does not; I found no code in the crate that starts one. The daemon does: write
the store with `theseus-exam write-store --store <state>/store --manifest M`, start a scratch `theseusd` on that
`--state-dir`; it supervises `theseus-index serve --store <state>/store --index <state>/index ...`, which answers
on `<state>/index/sock`; pass that to `probe --tender`. Without model weights the tender answers BM25 and entities
alone. Written in `probe.rs`'s header and the `probe` subcommand's help. I did not run a live probe (no daemon
over the exam's store was started; no model weights on this VM): the tender path is unchanged code.

## 3. The memory arm

**Changed.** `Cargo.toml`'s `reserved_for` (kept as "row 55 ..."; it now names the scratch daemon's `[memory] arm`
config key), `lib.rs`'s new "Memory arms (row 55)" paragraph, `drive.rs`'s header, and a comment at the
`turn.submit` call: the exam's scratch daemon will set `[memory] arm`, one daemon per arm; `turn.submit` carries no
arm field. Nothing was added to the daemon or its config template. The driver still has arms `none` and `oracle`.
`tests_registry` passes with the changed marker.

## Lines removed (git diff --numstat, added/removed)

| file | + | − |
|---|---|---|
| exam/exam-v1.toml | 0 | 1027 |
| src/probe.rs | 42 | 632 |
| src/main.rs | 22 | 79 |
| src/item.rs | 19 | 78 |
| src/fixture.rs | 5 | 18 |
| src/report.rs | 12 | 14 |
| src/lib.rs | 18 | 9 |
| src/drive.rs | 17 | 5 |
| tests/daemon_reads.rs | 4 | 10 |
| Cargo.toml | 1 | 1 |
| src/words.rs (new) | 97 | 0 |

Both commits together (`git diff --numstat a59b7c1 HEAD`, before this report): 1,873 lines removed, 237 added.

## Proof

- Exam tests: before, `cargo nextest run -p theseus-exam`: 54 tests, 52 passed, 2 failed only because `theseusd`
  was not built (daemon_reads). After (with `theseusd` built): 49 tests, 49 passed (the deleted BM25, v1-shape and
  v1-fixture tests, less the 3 added: stemmer, content words, recall).
- Planted revert: `content_stems` without its `.map(stem)`: `words::tests::content_words_come_from_both_tokenizers…`
  and `item::tests::each_hard_family_is_checked_against_its_definition` fail (47 passed, 2 failed). File restored,
  `touch`ed, `git status` clean.
- `cargo run -p theseus-exam -- --help` and `probe --help` parse; `probe --against v1` is rejected ("unexpected
  argument '--against'"); `--exam v1 list` errors "reading v1".
- Offline paths, built before the change and after (`scratchpad/offline.sh`: `list`, `write-store`, `note` for five
  items, and `report` with and without `--rescore` on a synthetic 432-record runs file over all 72 items): `list`,
  the `write-store` summary line ("758 sessions, 1550 keyed nodes, last position 5412"), four notes, and both
  reports are byte-identical. The manifests and WAL differ only in the random session and node ids (the manifest
  is identical, 24,673 lines, once the ids are masked; the WAL segment is the same size, 2,451,822 bytes, and
  the same files). The fifth note (`tool_output-1`, a name that does not exist; the id is `tool-output-1`) is the
  same error both times, differing only in the backtrace's source line number.
- Statistics on recorded samples: no real run records are in the repo; the synthetic runs file above is the
  sample.
- fmt and `scripts/shape.sh`: ok. Clippy on `theseus-exam` alone is clean when core's own 12 unfulfilled-expectation
  errors are allowed (they exist on `main` without my change, on this VM's toolchain).

## Gate

`THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: the suite phase ran 1,682 tests, 1,680 passed, 2
failed: `theseus-sandbox::contract clause_09_limits` (root exempt from `RLIMIT_NPROC`, theseus-pv6i) and
`theseus-core tests_output::the_cores_output_matches_its_golden` (all three tries, theseus-6a7o). Both are the
known ones. The phases before it (fmt, shape, clippy as the gate runs it, builds, reader rule) passed. After the suite
I checked the protocol-types phase by hand (`web/src/protocol.gen` is clean). I did not run the lifecycle, jobs,
and turn benches (budgets belong to the owner's machine), and `cargo deny` was not run.

## Documents that name these (for the maintainer)

- `docs/the-ship-of-theseus.md:5659`: "`theseus-exam` (exam v1.1 and v2, ...)" (history; a Part III record, probably
  leave, or add that v1 was later removed).
- `docs/the-ship-of-theseus.md:6930`: "... through turn.submit's memory_arm" → "... through the scratch daemon's
  `[memory] arm` config key".
- `docs/the-ship-of-theseus.md:6294`: names `theseus-exam probe --tender` (still true).
- `docs/design/roadmap-v2.md:239`: row 55 "34b's wire-in: `turn.submit`'s `memory_arm`" → "the `[memory] arm` config key".
- `docs/design/m6-memory.md:676`: the wire table's "`turn.submit { …, memory_arm? }` | ... `allow_arm_override`" row
  → delete it, and say the arm is the config key; `:703` `allow_arm_override = false` → `arm = "..."` under
  `[memory]`; `:747` (34b's row) and `:776` (the arm override is refused without the flag) follow.
- `docs/design/README.md:36`: `theseus-exam` reserved row 55 (unchanged, still right).
- No `AGENTS.md` names exam-v1, the BM25 probe, or a turn.submit memory arm (`crates/theseus-store/AGENTS.md:5`
  only lists `theseus-exam` as reserved). `crates/theseus-exam` has no `AGENTS.md`.
- The review's "BM25 probe" and `bm25probe.py` appear in no document other than the deleted `probe.rs` header.

## Left / uncertain

- `words.rs` keeps the stemmer and tokenizers (see Deviation).
- `probe` now has no in-process mode: a probe needs a daemon over the written store.
- No live tender run was made.
