# CLOUD REPORT: theseus-uhc2d (the manifest must say which model each arm ran)

Branch `cloud/20261026-p3p-manifest-framing`, base `78fcfb6d`. Python only, under `bench/report/`. No store/SCHEMA change,
nothing under `cockpit/`, `docs/`, or `scripts/` touched. Commit: `8a5533c7`.

## What I found
- `kind` holds the bench family, not a framing: `harness` (six Oct 18/21/23 Terminal-Bench datasets), `harbor` (the
  Oct 7/10/12/13 ones, and what `draft.py` writes), `omnibus`, **absent** (the newest, `2026-10-25-terminal-bench-codex-high`),
  and other families. Nothing in the repo writes `harness`. **Implemented derivation:** framing comes from the per-arm
  models, not `kind`; `kind` is never read (except the existing omnibus skip). A dataset's own top-level `framing` wins if valid.
  `draft.py` still writes `"kind": "harbor"` (left as is: it is the family, and changing it would make new datasets
  disagree with the Oct 7-13 ones for no reader).
- Wrong in the prompt: the 10-23 retests are not all "six-task": `-high-timeouts` has 6 tasks, `-max` has 27 of 30 planned
  (both carry a `scope` object). Both are `subset:<n>`.
- `codex-high` has `model: None` everywhere; its model (`GPT-6-Astra`) is only in a label, so its rows say `harness`
  framing though the model differs. Only a dataset edit (`summary.codex.model`) fixes that; I may not edit `docs/benchmarks/`.
- No published dataset has a per-arm `model`, so all old rows keep the prose string in `model`, as before.
- `file:line`: `manifest.py:122` model line, `:93` `rows_of`, `:97` omnibus skip: all as cited. `draft.py:359`/`:408-409`: as cited (shifted by my insertions).

## Framing vocabulary (explicit)
`harness` = A (same model, harness varies), `model` = B (model varies), `routed` = T (an arm that picked its own models).
Asserted in `test_the_vocabulary_maps_onto_the_cockpits_framings`. **`mixed` is dropped**: every dataset varies the harness,
so "varies both" is every dataset and carries no information. `routed` is the fourth value that expresses T. It is per row
(a routed arm's row says `routed`; other rows say the dataset's `harness`/`model`). `benchframing.ts` can prefer `row.framing`
(map harness->A, model->B, routed->T) and fall back to its regexes when the key is null/absent.
**OWNER CALL:** confirm `routed` replaces `mixed`; and whether `arms.json`'s `framing` field (`theseus-snjhp`, not present at base)
uses A/B/T or these words. They map 1:1 but must be one vocabulary.

## Keys added to every row (always present; unknown = `null`, never absent)
| key | type | old dataset gets |
|---|---|---|
| `model` (changed meaning) | string / sorted list / prose fallback | the prose string, unchanged |
| `model_note` | string/null | the dataset's prose `model` |
| `provider` | string/null | null (unless a model is named) |
| `routing` | `routed`/`pinned`/`n/a`/null | `routed` if the variant key contains "routed", else null |
| `effort` | list of strings/null | the entry's `effort`, as a list |
| `k` | int/null | `trials/tasks` only for a full-set arm that divides evenly; stated `k` wins; null for a subset |
| `task_set` | `full`/`subset:<n>` | `full`, or `subset:<n>` when the dataset has a `scope` or the arm ran fewer tasks than its widest peer |
| `subset_of` | report/null | the dataset's own `subset_of`, else null |
| `framing` | harness/model/routed | `harness` (the default) |
Collision: row `k` (attempts per task) is not `metrics.pass_rate.k` (solved trials). **OWNER CALL:** rename to `attempts`?
`draft.py` writes `summary[variant].model` (string for a pinned arm, sorted list for a routed one, null when no trial names one) and `provider`.
It does not yet write `effort`, `routing` or `scope`: the summary has no source for them.

## Proof (bench suite: `python3 -m unittest discover -s bench/report`)
- Base (changes stashed, new tests in): `Ran 77 tests ... FAILED (failures=2, errors=7)`: e.g. `AssertionError: Tuples differ: ('model-x', 'model-x') != ('anthropic/claude-sonnet-5-5', 'openai/gpt-x')`, `KeyError: 'task_set'`, `KeyError: 'framing'`, `AttributeError: ... no attribute 'models_of'`. The omnibus test passes on base (expected: it guards existing behaviour). The published-datasets guard fails on base only on its "gain the new keys" half; its old-keys half (model/trials unchanged) is the regression guard and holds after the change.
- After: `Ran 77 tests ... OK`. `python3 -m unittest discover -s bench/harbor`: `Ran 205 tests ... OK (skipped=45)`.
- Keel: `python3 scripts/keel-guard.py --base 78fcfb6d`: `keel: ok (... 0 findings)`. The default base (origin/main, now `e3c7d0e0`) reports findings from main's own later commits, not this branch's.

## Cockpit row must
Add `framing`, `model`, `model_note`, `provider`, `routing`, `k`, `task_set`, `subset_of`, `effort` to `IndexRow`; have `framingOfRow` use `row.framing` when non-null; render `model` as string-or-list; never put rows with different non-null `k` or `task_set` beside each other unannotated.

## Live check
`python3 bench/report/manifest.py build --dir /tmp/m` over a copy of `docs/benchmarks/`; every old key of every row should equal the committed `index.json`'s, with the 9 new keys added.

## Gate
See the section below.
