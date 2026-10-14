"""The blind-pair sheet: 20 pairs of replies (Theseus with the persona against the fair Claude Code arm) as a file the
owner answers with one key per pair, not knowing which is which.

    exam.py blind --run DIR [--pairs 20] [--seed N]           writes DIR/blind/sheet.md and key.json
    exam.py blind --run DIR --answers DIR/blind/answers.txt   tallies them

The sheet shows, for each pair, what the owner said and the two replies as A and B (which arm is which is drawn per
pair from the seed and kept only in `key.json`). The answers file is one line a pair: `07 a`, `08 b`, `09 =` (no
difference), `10 -` (cannot say); `#` starts a comment. A pair is chosen from the probes both arms answered, the
character-bearing ones first (those the judge scores), spread over the families, and only where the two replies
differ.

The tally counts a win for the arm whose reply was picked; ties and passes are reported, not counted. The interval
is Wilson's on wins over decided pairs, and the sign test (exact binomial, two-sided) says whether the split is
more than chance. With 20 pairs it is a signal for the owner, not a proof.

Standard library only.
"""

from __future__ import annotations

import json
import math
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "report"))
sys.path.insert(0, str(HERE.parent / "recall"))
sys.path.insert(0, str(HERE))

from generate import Rng  # noqa: E402
from stats import wilson  # noqa: E402

A_ARM, B_ARM = "theseus-persona", "claude-code-fair"


def choose(plan_steps: list[dict], left: dict[int, dict], right: dict[int, dict], n: int, seed: int) -> list[dict]:
    """Up to `n` pairs, round-robin over families, the judged probes first within each family."""
    rng = Rng(seed * 104729 + 7)
    cand: dict[str, list[dict]] = {}
    for s in plan_steps:
        if s["kind"] != "probe":
            continue
        i = s["index"]
        l, r = left.get(i), right.get(i)
        if not l or not r or l.get("skipped") or r.get("skipped"):
            continue
        lt, rt = (l.get("reply") or "").strip(), (r.get("reply") or "").strip()
        if not lt or not rt or lt == rt:
            continue
        cand.setdefault(s["family"], []).append({"index": i, "probe": s["probe"], "family": s["family"],
                                                 "judge": bool(s.get("judge")), "text": s["text"],
                                                 "left": lt, "right": rt})
    for fam in cand.values():
        rng.shuffle(fam)
        fam.sort(key=lambda c: not c["judge"])
    out, fams = [], sorted(cand)
    while len(out) < n and any(cand.values()):
        for f in fams:
            if cand[f] and len(out) < n:
                out.append(cand[f].pop(0))
    return out


def build(plan: dict, left: dict, right: dict, n: int = 20, seed: int = 7) -> tuple[str, dict]:
    pairs = choose(plan["steps"], left, right, n, seed)
    rng = Rng(seed * 31 + 5)
    key, lines = {}, ["# Blind pairs", "",
                      "For each pair, pick the reply you would rather have had from your own assistant. Answer in "
                      "`answers.txt`, one line a pair: `01 a`, `02 b`, `03 =` (no difference), `04 -` (cannot say).",
                      ""]
    for k, p in enumerate(pairs, 1):
        flip = rng.below(2) == 1
        a, b = (p["right"], p["left"]) if flip else (p["left"], p["right"])
        key[f"{k:02d}"] = {"a": B_ARM if flip else A_ARM, "b": A_ARM if flip else B_ARM, "probe": p["probe"],
                           "family": p["family"]}
        lines += [f"## {k:02d}", "", f"You said: {p['text']}", "", "**A**", "", quote(a), "", "**B**", "", quote(b), ""]
    return "\n".join(lines) + "\n", key


def quote(s: str) -> str:
    return "\n".join("> " + ln if ln.strip() else ">" for ln in s.splitlines())


def parse_answers(text: str, key: dict) -> dict[str, str]:
    out = {}
    for n, raw in enumerate(text.splitlines(), 1):
        ln = raw.split("#", 1)[0].strip()
        if not ln:
            continue
        m = re.fullmatch(r"(\d{1,3})\s+([abAB=\-])", ln)
        if not m:
            raise ValueError(f"answers line {n}: {raw!r}: want `07 a`, `07 b`, `07 =` or `07 -`")
        k = f"{int(m.group(1)):02d}"
        if k not in key:
            raise ValueError(f"answers line {n}: there is no pair {k}")
        if k in out:
            raise ValueError(f"answers line {n}: pair {k} answered twice")
        out[k] = m.group(2).lower()
    return out


def binom_two_sided(k: int, n: int) -> float:
    """Exact two-sided sign test against p = 1/2."""
    if n == 0:
        return 1.0
    pk = [math.comb(n, i) for i in range(n + 1)]
    cut = min(k, n - k)
    return min(1.0, 2 * sum(pk[: cut + 1]) / 2 ** n)


def tally(key: dict, answers: dict[str, str]) -> dict:
    wins = {A_ARM: 0, B_ARM: 0}
    ties = passes = 0
    for k, ans in answers.items():
        if ans == "=":
            ties += 1
        elif ans == "-":
            passes += 1
        else:
            wins[key[k][ans]] += 1
    decided = sum(wins.values())
    p, lo, hi = wilson(wins[A_ARM], decided) if decided else (None, None, None)
    return {"pairs": len(key), "answered": len(answers), "wins": wins, "ties": ties, "passes": passes,
            "decided": decided, "share": p, "lo": lo, "hi": hi, "sign_p": binom_two_sided(wins[A_ARM], decided),
            "unanswered": len(key) - len(answers)}
