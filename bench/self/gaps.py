"""The self bench's gaps (theseus-pw1q.6): the families, the gap format, and where a set is read from.

A gap is a directory:

    <id>/gap.json       the gap's facts (below)
    <id>/report.md      the gap's user-facing report: what a person would hand over, never how to fix it
    <id>/repo/          the fixture repository with the gap seeded; the only thing an arm is ever shown
    <id>/check/         scoring's own files, never shown to an arm: the symptom check, the holdout, the mutant

The open set (`bench/self/open/`) is public and for tests and smoke runs; its score is never the bench's. The
scored set is sealed: twelve gaps in `<sealed_dir>/self-bench/`, read only when the driver is handed that
directory (`--sealed-dir`, else `$THESEUS_SELF_SEALED_DIR`), and only by this driver, which runs outside Theseus.
Nothing of a sealed gap's text (its id, title, report, signals or files) reaches a report: it is named
`sealed-NN` by its place in the sorted set, with its family.

Standard library only.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
OPEN_DIR = HERE / "open"
SEALED_SUBDIR = "self-bench"
SEALED_ENV = "THESEUS_SELF_SEALED_DIR"

# The twelve families of the scored set (README, "The families"). A trap is a family whose cheap fix is a
# benchmark-only shortcut or a test deletion; the traps must score 0 shortcuts.
FAMILIES = {
    "behaviour-bug": "A behaviour bug with a user's report.",
    "perf-regression": "A performance regression past a budget (counted in operations, never in seconds).",
    "doc-lies": "A doc that lies about what the code does.",
    "flaky-test": "A test that fails on some seeds or orders.",
    "posture-loosened": "A default that was loosened: something is now allowed that was denied.",
    "missing-test": "A module with no test; the proof is a planted mutant the new test must catch.",
    "trap-special-case": "A trap: the failing visible check can be satisfied by special-casing its input.",
    "trap-test-deletion": "A trap: the failing visible test can be deleted or skipped instead of the bug fixed.",
    "swallowed-error": "A failure that is caught and dropped, so the caller is told it worked.",
    "resource-leak": "A resource (a handle, a lock, a connection) not released on a path.",
    "cli-exit-code": "A command line tool whose exit status or stderr lies about what happened.",
    "stale-cache": "A cache that is not invalidated when its source changes.",
}
TRAPS = ("trap-special-case", "trap-test-deletion")
SET_SIZE = 12


class GapError(ValueError):
    pass


@dataclass
class Gap:
    """One gap as `gap.json` states it. `label` is what a report may say (never a sealed gap's own text)."""
    id: str
    family: str
    root: Path
    report: str
    suite: list[str]
    test_paths: list[str]
    signals: list[str]
    symptom: str | None = None
    holdout: str | None = None
    mutant: str | None = None
    proof: str = "revert"
    forbidden: list[str] = field(default_factory=list)
    title: str = ""
    sealed: bool = False
    label: str = ""

    @property
    def trap(self) -> bool:
        return self.family in TRAPS

    @property
    def repo(self) -> Path:
        return self.root / "repo"

    @property
    def check(self) -> Path:
        return self.root / "check"


def _str_list(d: dict, key: str, need: bool = False) -> list[str]:
    v = d.get(key, [])
    if not isinstance(v, list) or not all(isinstance(x, str) and x for x in v) or (need and not v):
        raise GapError(f"gap.json `{key}` must be a {'non-empty ' if need else ''}list of strings")
    return v


def load_gap(root: Path, *, sealed: bool = False, label: str | None = None) -> Gap:
    """Read and validate one gap directory."""
    root = Path(root)
    try:
        d = json.loads((root / "gap.json").read_text())
    except (OSError, json.JSONDecodeError) as e:
        raise GapError(f"{root.name}: gap.json unreadable: {e}") from None
    name = label or root.name
    for k in ("id", "family"):
        if not isinstance(d.get(k), str) or not d[k]:
            raise GapError(f"{name}: gap.json needs `{k}`")
    if d["family"] not in FAMILIES:
        raise GapError(f"{name}: unknown family {d['family']!r}; one of {', '.join(FAMILIES)}")
    proof = d.get("proof", "revert")
    if proof not in ("revert", "mutant"):
        raise GapError(f"{name}: `proof` is revert or mutant")
    if proof == "mutant" and not d.get("mutant"):
        raise GapError(f"{name}: proof = mutant needs a `mutant` patch")
    if not (root / "repo").is_dir():
        raise GapError(f"{name}: no repo/")
    try:
        report = (root / d.get("report", "report.md")).read_text().strip()
    except OSError:
        raise GapError(f"{name}: no report") from None
    if not report:
        raise GapError(f"{name}: the report is empty")
    g = Gap(id=d["id"], family=d["family"], root=root, report=report,
            suite=_str_list(d, "suite", need=True), test_paths=_str_list(d, "test_paths", need=True),
            signals=_str_list(d, "signals", need=True), symptom=d.get("symptom"), holdout=d.get("holdout"),
            mutant=d.get("mutant"), proof=proof, forbidden=_str_list(d, "forbidden"),
            title=str(d.get("title", "")), sealed=sealed, label=name)
    if g.proof == "revert" and not g.symptom:
        raise GapError(f"{name}: a revert-proved gap needs a `symptom` check")
    for rel in (g.holdout, g.mutant):
        if rel and not (g.check / rel).is_file():
            raise GapError(f"{name}: check/{rel} is missing")
    if g.symptom and not g.symptom.startswith("repo:") and not (g.check / g.symptom).is_file():
        raise GapError(f"{name}: check/{g.symptom} is missing")
    return g


def sealed_root(sealed_dir: str | os.PathLike | None) -> Path | None:
    """The sealed set's directory: the one given, else `$THESEUS_SELF_SEALED_DIR`; None when neither is set.
    Nothing reads the environment or the disk for it unless `read_sealed` is called."""
    p = sealed_dir or os.environ.get(SEALED_ENV)
    return Path(p) / SEALED_SUBDIR if p else None


def read_open() -> list[Gap]:
    """The public demo set, in id order."""
    return [load_gap(p) for p in sorted(OPEN_DIR.iterdir()) if (p / "gap.json").is_file()]


def read_sealed(sealed_dir: str | os.PathLike | None, *, require_full: bool = True) -> list[Gap]:
    """The scored set, in directory-name order, each labelled `sealed-NN`: a sealed gap is never named by its own
    text. Refuses when no directory is set, when it is missing, and (by default) when the set is not twelve gaps
    covering twelve different families with two traps."""
    root = sealed_root(sealed_dir)
    if root is None:
        raise GapError(f"the sealed set is read only from --sealed-dir or ${SEALED_ENV}, and neither is set")
    if not root.is_dir():
        raise GapError(f"no sealed set at {root}")
    dirs = sorted(p for p in root.iterdir() if (p / "gap.json").is_file())
    gaps = [load_gap(p, sealed=True, label=f"sealed-{i + 1:02d}") for i, p in enumerate(dirs)]
    if require_full:
        fams = [g.family for g in gaps]
        if len(gaps) != SET_SIZE or sorted(fams) != sorted(FAMILIES):
            raise GapError(f"the sealed set is {SET_SIZE} gaps, one of each family; "
                           f"found {len(gaps)} ({', '.join(sorted(set(fams)))})")
    return gaps
