# bench/gauntlet

The reporting scripts the Terminal-Bench and recall cells were published with, taken
from the gauntlet's driver directory so they are reviewed and versioned with the harness
they read. They run from a gauntlet run directory (`runs/<cell>`); `DOCS` is the
checkout whose `docs/benchmarks` holds the published baselines.

- `tb_report.py` builds a Terminal-Bench cell's dataset and its comparison against a
  published baseline.
- `recall_report.py` builds the recall cell's dataset and its comparison against the
  published seed-7 run.
- `gatewindows.py` is the join-gate window helper `tb_report.py` imports.

Both reports compare cost, and a cost comparison cannot cross two price tables
(`bench/harbor/price_basis.py`): they refuse a ratio across two price digests unless both
arms are repriced onto one table.

    python3 -m unittest discover -s bench/gauntlet
