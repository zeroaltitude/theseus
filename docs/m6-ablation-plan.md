# M6's ablation plan, version 1 (experiment `m6-1`)

The plan M6's memory is judged by, written before any canary runs (design `docs/design/m6-memory.md`, §2.9: "The
plan is written before the canary starts"). Every report names this file's digest, the SHA-256 of its bytes, which
`theseus-exam report` prints on its first lines. A change to this file is a new version of the plan, and a report
says which data came under which.

## 1. The arms, and their versions

Each arm is a scratch daemon of its own, in `[memory] mode = "live"`, whose config names it (`[memory] arm`); the
arm is config, never a field of `turn.submit`. Every arm gets the same recall budget (`recall_budget_tokens`, 1,500
by default), the same items (`recall_max_items`, 6), the same session cap (`session_recall_cap_tokens`, 12,000), and
the same context files.

| Arm | What reaches the model | Version |
|---|---|---|
| `none` | Today's compiler: the session's own transcript, and no recall. The daemon asks the index nothing. | the daemon's build |
| `bm25` | Recall from the index's BM25 and exact-entity sources, fused, filtered, and packed by `baseline`'s science. | the science each recall row names (`baseline@<digest>`) |
| `baseline` | Recall from BM25, entities, and vectors, fused, filtered, and packed by `baseline`'s science. A tender without its model's files answers without vectors, and its rows say so in `skipped`. | as `bm25` |
| `oracle` | The exam's gold nodes, rendered as the core renders a `Recall` node, after the task, sent to the `none` daemon: the ceiling. | the exam's digest |

- **The science's version** is the digest each `recall.ran` row records (`science`, `baseline@<digest>`). Step 31a
  changes `baseline` (the freshness and provenance rules), and with it the digest; a report names every digest its
  rows carry, and data under two digests are two arms.
- **The exam's version** is its digest (`theseus-exam list`): exam-v2 at this plan's writing. A corrected exam is a
  new version, and rescored records say which scored them.
- **Later arms** (`+retention`, `+activation`, `+rerank`, `+synthesis`, `+lessons`, `full`) join as new versions of
  this plan, each differing from `baseline` in one feature, at the same budget.

## 2. The primary metrics

- **The exam** (instrument 1): an item's pass rate, the share of its runs whose deterministic check passed. Cost per
  pass (the model's spend, plus the arm's own, over the passes), and recall's latency (p95 of each recall's
  `total_ms`), go beside it.
- **The canary** (instrument 3): task success and false completion (M5's system labels); stale or contradictory
  recall; disclosure violations; re-supply per 100 human messages; turn latency p50 and p95; total cost.
- **Shadow diagnostics** (instrument 2) never decide alone: recall and precision at k, MRR, and the stale rate,
  against the silver labels.

## 3. The unit

- **The exam: the item.** Each item runs under every arm (paired); its runs are averaged into its rate and never
  counted as samples (clustered by item).
- **The canary: the session.** A session's arm is sticky, from a hash of the session and the experiment, recorded
  once as `memory.arm`; the others are control (`none` live, `baseline` in shadow).

## 4. The minimum samples

- **The exam**: every held-in item (36 in exam-v2) under every arm, 3 runs each, on the cheap profile, spend capped
  by the driver's limit; then the held-out half (36 items), once, after every choice is made. The held-out half is
  never used to tune a threshold, a word, or a weight.
- **The canary**: 120 sessions per arm before any verdict (moving task success from 70% to 85% at a two-sided 5%
  level with 80% power). Until then a report says "insufficient", with its n.

## 5. The decision rule, per feature

A feature is `baseline` against `none` (recall itself), each later `+x` against `baseline`, and, for vectors,
`baseline` against `bm25`.

1. **A disclosure violation it caused: off, and a P1 bug.** In the exam, a private-family item the feature's arm
   failed in a run its comparison passed.
2. **On by default only if** all hold:
   - the canary shows no harm to task success or false completion: the one-sided 90% interval excludes a drop of more
     than 5 points;
   - it gains on at least one of task success, stale recall, re-supply, or cost per success;
   - the exam's held-out half agrees in sign;
   - its recall p95 stays within budget (`recall_deadline_ms`).
3. **Otherwise off by default**, and marked experimental in the spec. "Insufficient" counts as otherwise.

## 6. The analysis

- **An arm's pass rate**: the mean of its items' rates, with a 95% Student t interval over the items, clamped to
  [0, 1], and its n (items, then cells with a verdict). A cell that ended in an error has no verdict: it is left out,
  and counted.
- **A paired difference**, `b − a`, item by item, for `baseline − none`, `bm25 − none`, `oracle − none`,
  `baseline − bm25`, and `oracle − baseline`: its mean in points, with a 95% t interval over the items both arms
  have a verdict on, clamped to [−1, 1], and an exact two-sided sign test over the items that moved. It says `gain`
  when its interval lies above zero, `loss` below, and `insufficient` when it holds zero, or when one item gives no
  interval.
- **The halves** are read apart: all items, the held-in half, and the held-out half.
- **The order** is a seeded shuffle, run by run, with an item's arms next to each other, so every arm of an item
  meets the same provider weather. Each run starts fresh daemons from each arm's snapshot of the store, so no
  daemon recalls an item's answer from an earlier run.
- **The replay** (`theseus-exam replay`, over a copy of a store) recomputes every arm over the recorded turns, each
  with its `as_of`, and drops any hit written at or after it; it scores the packs against the silver labels
  (re-supply as an 8-word run, reference, re-derivation, should-have). It reads no outbox, so it applies no place
  rule, and it never decides alone.
- **What could not be measured** is said, with why: the canary while it has no data, vectors where the tender had no
  model.
- **The report** is a frozen file (`theseus-exam report --out`), never overwritten, naming this plan's digest, the
  exam's, each arm's science, and the data's window.
