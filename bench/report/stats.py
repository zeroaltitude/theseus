"""The statistics every benchmark report uses, so a past run's numbers and a future run's come from the same code.

- `wilson(k, n)`: a pass rate and its Wilson score interval (95% by default). It stays inside [0, 1] and is honest at
  0 and n, where the normal interval collapses to a point.
- `bootstrap_mean(xs)`: the mean and a percentile bootstrap interval, seeded, so a report regenerates the same
  numbers.
- `mcnemar_exact(b, c)`: the two-sided exact McNemar test for paired outcomes (a sign test on the discordant pairs):
  `b` pairs where only the first arm passed, `c` where only the second did.
- `quantile(xs, q)`: the linear-interpolation quantile (the usual "type 7"), and `median(xs)`.

Standard library only.
"""

from __future__ import annotations

import math
import random
from typing import Sequence

Z95 = 1.959963984540054


def wilson(k: int, n: int, z: float = Z95) -> tuple[float, float, float]:
    """(rate, low, high) for `k` passes in `n` trials. n = 0 gives (nan, 0, 1): nothing is known."""
    if n <= 0:
        return math.nan, 0.0, 1.0
    if not 0 <= k <= n:
        raise ValueError(f"k={k} outside 0..n={n}")
    p = k / n
    z2 = z * z
    centre = (p + z2 / (2 * n)) / (1 + z2 / n)
    half = z * math.sqrt(p * (1 - p) / n + z2 / (4 * n * n)) / (1 + z2 / n)
    return p, max(0.0, centre - half), min(1.0, centre + half)


def quantile(xs: Sequence[float], q: float) -> float:
    """The q-quantile (0 <= q <= 1) by linear interpolation between order statistics."""
    if not xs:
        return math.nan
    s = sorted(xs)
    if len(s) == 1:
        return float(s[0])
    pos = q * (len(s) - 1)
    lo = math.floor(pos)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (pos - lo)


def median(xs: Sequence[float]) -> float:
    return quantile(xs, 0.5)


def bootstrap_mean(xs: Sequence[float], iters: int = 10_000, seed: int = 7,
                   level: float = 0.95) -> tuple[float, float, float]:
    """(mean, low, high): the sample mean and a percentile bootstrap interval of it. Fewer than two values give the
    mean itself as both ends: no interval can be drawn from one value."""
    xs = [float(x) for x in xs]
    if not xs:
        return math.nan, math.nan, math.nan
    mean = sum(xs) / len(xs)
    if len(xs) < 2:
        return mean, mean, mean
    rng = random.Random(seed)
    n = len(xs)
    means = []
    for _ in range(iters):
        means.append(sum(xs[rng.randrange(n)] for _ in range(n)) / n)
    a = (1 - level) / 2
    return mean, quantile(means, a), quantile(means, 1 - a)


def bootstrap_paired_diff(a: Sequence[float], b: Sequence[float], iters: int = 10_000, seed: int = 7,
                          level: float = 0.95) -> tuple[float, float, float]:
    """(mean of b - a, low, high) over paired values (the same task under two arms), resampling the pairs."""
    if len(a) != len(b):
        raise ValueError("paired samples need the same length")
    return bootstrap_mean([y - x for x, y in zip(a, b)], iters, seed, level)


def mcnemar_exact(b: int, c: int) -> float:
    """Two-sided exact p for b pairs won only by the first arm against c won only by the second: twice the smaller
    binomial tail at p = 1/2, capped at 1. No discordant pair gives 1."""
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return min(1.0, 2 * tail)
