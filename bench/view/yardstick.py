#!/usr/bin/env python3
"""The yardstick (issue 44, §10 of the context design, its last row: the
owner's idea): on past turns, a judge is asked about EVERY earlier exchange
"does this hold something that helps answer this message?", and its answers
are set beside what the rules selected, to show what the rules miss.

    # what it would cost: no model is called
    python3 bench/view/yardstick.py --cases bench/view/cases/synthetic.v1.json --dry-run --cap-usd 1
    # on a COPY of a store: the place's last 20 turns, each against up to 96 exchanges before it
    python3 bench/view/yardstick.py --store /tmp/state-copy --bin-dir target/release \\
        --place 'local:/path/of/the/place' --turns 20 --back 96 --dry-run --cap-usd 5
    python3 bench/view/yardstick.py ... --judge model --cap-usd 5 --out /tmp/yard

**It calls a model, so it is refused without `--cap-usd`**, and it never
starts a call whose worst case (its input at the model's list price, plus
`MAX_OUT` tokens of output) would take what it has spent past the cap: it
stops there and says so. `--dry-run` counts the calls and prints the worst
case, and calls nothing. The judge is a direct Messages API call
(bench/personality's `judge.ask`), on `--model` (default Haiku 5.5) with
`ANTHROPIC_API_KEY`; `--judge fake` is the tests' judge, which reads an
oracle and calls nothing.

**The windows** (§10): the exchanges before a turn, one line each, go to the
judge in overlapping windows of `WINDOW` (24), each starting half a window
after the last and the first starting half a window before the first line,
so every line is judged twice, with different neighbours; its higher score
is kept. A line scores 2 (helps), 1 (might) or 0. A line is the person's
words in that exchange; the `did` line the design adds beside it is not read
by this build (the history does not carry it).

**The rules' selection**: with `--cases`, a strategy's `held` file
(`cases.py build` writes one per strategy) or, without one, the transcript's
rule; on a store, the transcript's (every earlier exchange of the turn's own
session), because the daemon's preview cannot yet be had as of a past turn
(`replay.py` says when it can).

**Output**: counts only, printed and written to `--out/yardstick.json`: per
turn and in sum, the lines the judge marked helpful that the rules held
(`both`), that the rules did not (`judge_only`: what the rules miss), that the
rules held and the judge did not mark (`rules_only`), and neither; the calls,
the tokens and the dollars. Each line's text is held in memory for its call
and written nowhere.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "recall"))
sys.path.insert(0, str(HERE.parent / "personality"))

import cases as vc  # noqa: E402
from generate import PRICES  # noqa: E402

WINDOW = 24
MAX_OUT = 400  # bench/personality's judge.ask asks for at most this many output tokens
BYTES_PER_TOKEN = 3.6
DEFAULT_MODEL = "claude-haiku-5-5"
API = "https://api.anthropic.com"
HELPS = 2
SYSTEM = ("You judge relevance. You are shown one MESSAGE a person sent and numbered LINES, each an earlier "
          "exchange of the same conversation. For each line answer 2 if it holds something that helps answer "
          "the message, 1 if it might, 0 if it does not. Reply with JSON only: {\"scores\": [s1, s2, ...]}, one "
          "score per line, in order.")


class CapNeeded(SystemExit):
    pass


# ---- the windows


def windows(n: int, size: int = WINDOW) -> list[range]:
    """Overlapping windows over `n` lines: each starts half a window after the last, the first half a window
    before line 0, so every line lies in exactly two (one when `n` is 0 or `size` is 1)."""
    step = max(size // 2, 1)
    out = []
    start = -step
    while start < n:
        lo, hi = max(start, 0), min(start + size, n)
        if lo < hi:
            out.append(range(lo, hi))
        start += step
    return out


def prompt(message: str, lines: list[str]) -> str:
    body = "\n".join(f"{i + 1}. {t}" for i, t in enumerate(lines))
    return f"MESSAGE:\n{message}\n\nLINES:\n{body}\n\nScore every line."


def tokens(text: str) -> int:
    return int(len(text.encode()) / BYTES_PER_TOKEN) + 1


def price(model: str) -> tuple[float, float]:
    p = PRICES.get(model) or PRICES[DEFAULT_MODEL]
    return p[0] / 1e6, p[1] / 1e6


def worst(model: str, system: str, user: str) -> float:
    pin, pout = price(model)
    return (tokens(system) + tokens(user)) * pin + MAX_OUT * pout


def parse(text: str, n: int) -> list[int]:
    i, j = text.find("{"), text.rfind("}")
    if i < 0 or j < i:
        raise ValueError("no JSON object")
    sc = json.loads(text[i:j + 1]).get("scores")
    if not isinstance(sc, list) or len(sc) != n or not all(x in (0, 1, 2) for x in sc):
        raise ValueError(f"scores must be {n} of 0, 1, 2")
    return sc


# ---- judges


class ModelJudge:
    def __init__(self, model: str, base_url: str, key: str):
        import judge as pj  # noqa: PLC0415  (bench/personality's call)

        self.ask, self.model, self.base_url, self.key = pj.ask, model, base_url, key

    def __call__(self, probe: dict, idx: list[int], user: str) -> tuple[list[int] | None, dict]:
        text, usage = self.ask(self.base_url, self.key, self.model, SYSTEM, user)
        try:
            return parse(text, len(idx)), usage
        except ValueError:
            return None, usage


class FakeJudge:
    """The tests' judge: a line helps when the oracle names it for the probe. It calls nothing and reports
    the request's estimate as its input, so the sums run as a model's would."""

    def __init__(self, oracle: dict[str, set[str]]):
        self.oracle = oracle
        self.calls = 0

    def __call__(self, probe: dict, idx: list[int], user: str) -> tuple[list[int] | None, dict]:
        self.calls += 1
        want = self.oracle.get(probe["id"], set())
        return ([HELPS if probe["ids"][i] in want else 0 for i in idx],
                {"input_tokens": tokens(SYSTEM) + tokens(user), "output_tokens": 4 * len(idx)})


# ---- probes: a turn, the lines before it, and what the rules held


def probes_from_cases(cases: dict, held: dict | None) -> list[dict]:
    out = []
    for c in cases["cases"]:
        rules = (held or {}).get("held", {}).get(c["id"]) if held else vc.transcript_held(c)
        if rules is None:
            continue
        out.append({"id": c["id"], "class": c["class"], "message": c["message"]["say"],
                    "ids": [h["id"] for h in c["history"]], "texts": [h["say"] for h in c["history"]],
                    "rules": set(rules)})
    return out


def probes_from_store(d, place: str, turns: int, back: int) -> list[dict]:
    """The place's last `turns` exchanges, each with up to `back` exchanges of the place before it; a line is
    the person's words in that exchange (its `user_message` nodes), held in memory only. The rules: every
    earlier exchange of the turn's own session."""
    import drive  # noqa: PLC0415
    import scratch  # noqa: PLC0415

    rows = [r for r in scratch.ledger_rows(d, "context.turn")
            if isinstance(r.get("data"), dict) and r["data"].get("place") == place]
    words: dict[str, str] = {}
    for sid in {r.get("session_id") for r in rows if r.get("session_id")}:
        _, whole = drive.history_pages(d.cli, sid)
        for node in (whole or {}).get("nodes") or []:
            if node.get("kind") == "user_message" and node.get("turn_id"):
                words[node["turn_id"]] = (words.get(node["turn_id"], "") + " " + node.get("text", "")).strip()
    ex = [(r["data"]["exchange"], r.get("session_id")) for r in rows if r["data"].get("exchange") in words]
    out = []
    for k in range(max(len(ex) - turns, 0), len(ex)):
        before = ex[max(k - back, 0):k]
        if not before:
            continue
        out.append({"id": ex[k][0], "class": "past_turn", "message": words[ex[k][0]],
                    "ids": [e for e, _ in before], "texts": [words[e] for e, _ in before],
                    "rules": {e for e, s in before if s == ex[k][1]}})
    return out


# ---- the run


def plan(probes: list[dict], model: str) -> dict:
    calls, usd = 0, 0.0
    for p in probes:
        for w in windows(len(p["ids"])):
            calls += 1
            usd += worst(model, SYSTEM, prompt(p["message"], [p["texts"][i] for i in w]))
    return {"probes": len(probes), "lines": sum(len(p["ids"]) for p in probes), "calls": calls,
            "worst_case_usd": round(usd, 4), "model": model}


def run(probes: list[dict], judge, model: str, cap_usd: float) -> dict:
    pin, pout = price(model)
    spent, calls, unparsed = 0.0, 0, 0
    usage: Counter = Counter()
    stopped = None
    per, total = [], Counter()
    for p in probes:
        best = [None] * len(p["ids"])
        for w in windows(len(p["ids"])):
            idx = list(w)
            user = prompt(p["message"], [p["texts"][i] for i in idx])
            if spent + worst(model, SYSTEM, user) > cap_usd:
                stopped = f"the next call could pass the cap of ${cap_usd:.2f} (spent ${spent:.4f})"
                break
            scores, u = judge(p, idx, user)
            calls += 1
            spent += u.get("input_tokens", 0) * pin + u.get("output_tokens", 0) * pout
            usage.update({k: v for k, v in u.items() if isinstance(v, int)})
            if scores is None:
                unparsed += 1
                continue
            for i, s in zip(idx, scores):
                best[i] = s if best[i] is None else max(best[i], s)
        if stopped:
            break
        c = Counter()
        for i, e in enumerate(p["ids"]):
            helps, held = best[i] == HELPS, e in p["rules"]
            c[{(True, True): "both", (True, False): "judge_only", (False, True): "rules_only",
               (False, False): "neither"}[(helps, held)]] += 1
            if best[i] is None:
                c["unjudged"] += 1
        per.append({"turn": p["id"], "class": p["class"], **c})
        total.update(c)
    by_class: dict[str, Counter] = {}
    for r in per:
        by_class.setdefault(r["class"], Counter()).update({k: v for k, v in r.items() if isinstance(v, int)})
    return {"turns": per, "total": dict(total), "by_class": {k: dict(v) for k, v in by_class.items()},
            "calls": calls, "unparsed": unparsed, "usage": dict(usage), "spent_usd": round(spent, 6),
            "cap_usd": cap_usd, "stopped": stopped, "model": model}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--cap-usd", type=float, help="REQUIRED: the most it may spend, in dollars")
    ap.add_argument("--dry-run", action="store_true", help="count the calls and the worst case; call nothing")
    ap.add_argument("--judge", choices=("model", "fake"), default="model")
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--api-base", default=API)
    ap.add_argument("--cases", help="a labelled case set (the synthetic one)")
    ap.add_argument("--held", help="with --cases: a strategy's held file (else the transcript's rule)")
    ap.add_argument("--store", help="a COPY of a daemon's state directory")
    ap.add_argument("--bin-dir", default="target/release")
    ap.add_argument("--place")
    ap.add_argument("--turns", type=int, default=20)
    ap.add_argument("--back", type=int, default=96, help="exchanges before each turn the judge reads")
    ap.add_argument("--out", help="a new directory for yardstick.json (and the scratch daemon)")
    a = ap.parse_args(argv)
    if a.cap_usd is None or not a.cap_usd > 0:
        raise CapNeeded("yardstick: refused: it calls a model; give --cap-usd, the most it may spend")
    if bool(a.cases) == bool(a.store):
        raise SystemExit("yardstick: give --cases or --store, one of them")
    if a.judge == "fake" and not a.cases:
        raise SystemExit("yardstick: the fake judge reads a case set's evidence: --cases")
    out = Path(a.out) if a.out else None
    if out is not None:
        out.mkdir(parents=True, exist_ok=False)
    if a.cases:
        cs = vc.load(Path(a.cases))
        probes = probes_from_cases(cs, json.loads(Path(a.held).read_text()) if a.held else None)
        oracle = {c["id"]: set(c["evidence"]) for c in cs["cases"]}
        return finish(a, out, probes, lambda: FakeJudge(oracle))
    if not (a.place and out):
        raise SystemExit("yardstick: --store needs --place and --out")
    import scratch  # noqa: PLC0415

    with scratch.Scratch(Path(a.bin_dir), out, store=Path(a.store)) as d:
        probes = probes_from_store(d, a.place, a.turns, a.back)
    return finish(a, out, probes, None)


def finish(a, out: Path | None, probes: list[dict], fake) -> int:
    p = plan(probes, a.model)
    if a.dry_run:
        print(json.dumps({"dry_run": True, **p, "cap_usd": a.cap_usd,
                          "fits_the_cap": p["worst_case_usd"] <= a.cap_usd}, indent=2, sort_keys=True))
        return 0
    if a.judge == "fake":
        judge = fake()
    else:
        key = os.environ.get("ANTHROPIC_API_KEY")
        if not key:
            raise SystemExit("yardstick: the model judge needs ANTHROPIC_API_KEY in its environment")
        judge = ModelJudge(a.model, a.api_base, key)
    r = {"plan": p, **run(probes, judge, a.model, a.cap_usd)}
    if out is not None:
        (out / "yardstick.json").write_text(json.dumps(r, indent=2, sort_keys=True) + "\n")
    print(json.dumps({k: r[k] for k in ("plan", "total", "by_class", "calls", "spent_usd", "stopped")},
                     indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
