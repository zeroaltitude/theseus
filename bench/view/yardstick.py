#!/usr/bin/env python3
"""The yardstick (issue 44, §10 of the context design, its last row: the
owner's idea): on past turns, a judge is asked about EVERY earlier exchange
"does this hold something that helps answer this message?", and its answers
are set beside a rule's selection, to show what the rule misses.

    # what it would cost: no model is called
    python3 bench/view/yardstick.py --cases bench/view/cases/synthetic.v1.json --dry-run --cap-usd 1
    # on a COPY of a store: the place's last 20 turns, each against up to 96 exchanges before it
    python3 bench/view/yardstick.py --store /tmp/state-copy --bin-dir target/release \\
        --place 'local:/path/of/the/place' --turns 20 --back 96 --out /tmp/yard-plan --dry-run --cap-usd 1
    ANTHROPIC_API_KEY=... python3 bench/view/yardstick.py ... --out /tmp/yard --cap-usd 1

**The cap is a bound.** It is refused without `--cap-usd`. The model and the
endpoint must be known: a model without a price in the catalogue
(bench/recall's `PRICES`), or any `--api-base` but Anthropic's, is refused
unless the caller states the prices (`--price-in`, `--price-out`, dollars a
million tokens). Before each call the kit RESERVES its worst case: every byte
of the request as a token (a token is never less than a byte), plus
`OVERHEAD_TOKENS` for the message's framing, at the input price (or the cache
write's, if higher), plus `MAX_OUT` output tokens at the output price. A call
whose reservation would take the total past the cap is not made: the run
stops and says so. After a call, the reservation is replaced by the usage the
provider reported only when the stream completed and reported it; otherwise
the reservation stays spent. `--dry-run` sums the reservations (unrounded)
and compares them with the cap, and calls nothing.

**The windows** (§10): the exchanges before a turn, one line each, go to the
judge in overlapping windows of `WINDOW` (24), each starting half a window
after the last and the first starting half a window before the first line,
so every line is judged twice, with different neighbours; its higher score
is kept. A line scores 2 (helps), 1 (might) or 0. A line is the person's
words in that exchange, cut at `LINE_CHARS` (the message at
`MESSAGE_CHARS`); the `did` line the design adds beside it is not read by
this build (the history does not carry it).

**The rule it is compared with** (`rules_basis` in every output):

- on a store, `same_session_heuristic`: every earlier exchange of the turn's
  own session counts as held. It is NOT the transcript strategy's real
  selection (compaction, summaries, recall and other sessions' place tails
  are ignored), and only the chosen place's exchanges are judged, so it
  cannot see what lies in another place;
- on the cases, `held_file:<strategy>` (a strategy's held file, which `cases.py
  build` writes), or `own_place_lines` (every earlier line of the message's
  own place) without one. A case `cases.py` cannot build (`not_built`) is
  judged as a fixture only: it is reported apart, and never in the totals.

**Output**: counts only, printed and written to `--out/yardstick.json`
(checked before anything is written): per turn and in sum, of the lines the
judge answered for, those it marked helpful that the rule held (`both`), that
the rule did not (`judge_only`: what the rule misses), that the rule held and
the judge did not mark (`rules_only`), and `neither`. A line no window
answered for is in no cell: it is counted under `coverage`. Each line's text
is held in memory for its call and written nowhere; a failed call is a fixed
code (`E_JUDGE_…`), never the provider's answer. The judge reads only
`ANTHROPIC_API_KEY` from the caller's environment. `--judge fake` (the tests')
reads a case set's evidence and calls nothing.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "recall"))

import cases as vc  # noqa: E402
from generate import PRICES  # noqa: E402

WINDOW = 24
MAX_OUT = 400
OVERHEAD_TOKENS = 64  # the request's framing beyond its bytes: roles, the system block, the message's wrapper
# A line's and the message's length cut, in characters: 24 lines of LINE_CHARS keep a request near the design's
# 3,000 tokens; a pasted page is judged by its head.
LINE_CHARS = 480
MESSAGE_CHARS = 2000
DEFAULT_MODEL = "claude-haiku-5-5"
API = "https://api.anthropic.com"
HELPS = 2
CELLS = ("both", "judge_only", "rules_only", "neither")
SYSTEM = ("You judge relevance. You are shown one MESSAGE a person sent and numbered LINES, each an earlier "
          "exchange of the same conversation. For each line answer 2 if it holds something that helps answer "
          "the message, 1 if it might, 0 if it does not. Reply with JSON only: {\"scores\": [s1, s2, ...]}, one "
          "score per line, in order.")
SAME_SESSION = "same_session_heuristic"
OWN_PLACE = "own_place_lines"


class CapNeeded(SystemExit):
    pass


class JudgeFailed(RuntimeError):
    """A fixed code: never a response body or an event."""


# ---- the windows


def windows(n: int, size: int = WINDOW) -> list[range]:
    """Overlapping windows over `n` lines: each starts half a window after the last, the first half a window
    before line 0, so every line lies in exactly two (one when `size` is 1)."""
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
    body = "\n".join(f"{i + 1}. {' '.join(t.split())[:LINE_CHARS]}" for i, t in enumerate(lines))
    return f"MESSAGE:\n{message[:MESSAGE_CHARS]}\n\nLINES:\n{body}\n\nScore every line."


# ---- money


class Prices:
    """Dollars a token: input (the highest of input and cache write, for a reservation), output."""

    def __init__(self, model: str, api_base: str, price_in: float | None, price_out: float | None):
        if price_in is not None or price_out is not None:
            if price_in is None or price_out is None or not (price_in > 0 and price_out > 0):
                raise SystemExit("yardstick: give both --price-in and --price-out, above zero")
            self.pin, self.pout, self.pwrite, self.pread = (price_in / 1e6, price_out / 1e6, price_in / 1e6,
                                                            price_in / 1e6)
            return
        if api_base.rstrip("/") != API:
            raise SystemExit(f"yardstick: refused: {api_base} may charge other prices; state them "
                             "(--price-in, --price-out)")
        if model not in PRICES:
            raise SystemExit(f"yardstick: refused: no price for {model}; state them (--price-in, --price-out)")
        pin, pout, pread, pwrite = PRICES[model]
        self.pin, self.pout, self.pread, self.pwrite = pin / 1e6, pout / 1e6, pread / 1e6, pwrite / 1e6

    def reserve(self, system: str, user: str) -> float:
        """The call's worst case: every byte a token, the framing, the output limit."""
        tokens = len(system.encode()) + len(user.encode()) + OVERHEAD_TOKENS
        return tokens * max(self.pin, self.pwrite) + MAX_OUT * self.pout

    def charge(self, usage: dict) -> float:
        return (usage.get("input_tokens", 0) * self.pin + usage.get("output_tokens", 0) * self.pout
                + usage.get("cache_read_input_tokens", 0) * self.pread
                + usage.get("cache_creation_input_tokens", 0) * self.pwrite)


def parse(text: str, n: int) -> list[int]:
    i, j = text.find("{"), text.rfind("}")
    if i < 0 or j < i:
        raise ValueError("no JSON object")
    sc = json.loads(text[i:j + 1]).get("scores")
    if not isinstance(sc, list) or len(sc) != n or not all(x in (0, 1, 2) for x in sc):
        raise ValueError(f"scores must be {n} of 0, 1, 2")
    return sc


# ---- judges: each returns (scores or None, usage, complete)


def ask(base_url: str, key: str, model: str, system: str, user: str, timeout: float = 120.0
        ) -> tuple[str, dict, bool]:
    """One streamed Messages call: its text, its usage, and whether the stream ended (`message_stop`). A
    failure is `JudgeFailed` with a fixed code."""
    body = json.dumps({"model": model, "max_tokens": MAX_OUT, "stream": True, "system": system,
                       "messages": [{"role": "user", "content": user}]}).encode()
    req = urllib.request.Request(base_url.rstrip("/") + "/v1/messages", data=body, method="POST", headers={
        "content-type": "application/json", "x-api-key": key, "anthropic-version": "2023-06-01"})
    text, usage, done = [], {}, False
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            for raw in r:
                line = raw.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                try:
                    e = json.loads(line[5:])
                except json.JSONDecodeError:
                    continue
                t = e.get("type") if isinstance(e, dict) else None
                if t == "message_start":
                    usage.update((e.get("message") or {}).get("usage") or {})
                elif t == "message_delta":
                    usage.update(e.get("usage") or {})
                elif t == "content_block_delta" and (e.get("delta") or {}).get("type") == "text_delta":
                    text.append(str(e["delta"].get("text", "")))
                elif t == "message_stop":
                    done = True
                elif t == "error":
                    raise JudgeFailed("E_JUDGE_STREAM")
    except urllib.error.HTTPError as e:
        raise JudgeFailed(f"E_JUDGE_HTTP {e.code}") from None
    except (urllib.error.URLError, OSError, TimeoutError):
        raise JudgeFailed("E_JUDGE_NETWORK") from None
    usage = {k: v for k, v in usage.items() if isinstance(v, int) and not isinstance(v, bool)}
    return "".join(text), usage, done


class ModelJudge:
    def __init__(self, model: str, base_url: str, key: str, ask_fn=None):
        self.model, self.base_url, self.key, self.ask = model, base_url, key, ask_fn or ask

    def __call__(self, probe: dict, idx: list[int], user: str) -> tuple[list[int] | None, dict, bool]:
        text, usage, done = self.ask(self.base_url, self.key, self.model, SYSTEM, user)
        try:
            return parse(text, len(idx)), usage, done
        except (ValueError, json.JSONDecodeError):
            return None, usage, done


class FakeJudge:
    """The tests' judge: a line helps when the oracle names it for the probe. It calls nothing and reports
    the request's bytes as its input, so the sums run as a model's would."""

    def __init__(self, oracle: dict[str, set[str]]):
        self.oracle = oracle
        self.calls = 0

    def __call__(self, probe: dict, idx: list[int], user: str) -> tuple[list[int] | None, dict, bool]:
        self.calls += 1
        want = self.oracle.get(probe["id"], set())
        return ([HELPS if probe["ids"][i] in want else 0 for i in idx],
                {"input_tokens": len(SYSTEM.encode()) + len(user.encode()), "output_tokens": 4 * len(idx)}, True)


# ---- probes: a turn, the lines before it, and what the rule held


def probes_from_cases(cases: dict, held: dict | None) -> tuple[list[dict], str]:
    basis = f"held_file:{held.get('strategy')}" if held else OWN_PLACE
    out = []
    for c in cases["cases"]:
        rules = (held.get("held") or {}).get(c["id"]) if held else vc.transcript_held(c)
        if rules is None:
            continue
        out.append({"id": c["id"], "class": c["class"], "message": c["message"]["say"],
                    "ids": [h["id"] for h in c["history"]], "texts": [h["say"] for h in c["history"]],
                    "rules": set(rules), "not_built": vc.not_built(c)})
    return out, basis


def probes_from_store(d, place: str, turns: int, back: int) -> tuple[list[dict], str]:
    """The place's last `turns` exchanges, each with up to `back` exchanges of the place before it; a line is
    the person's words in that exchange (its `user_message` nodes), held in memory only. The rule: the
    same-session heuristic (module text)."""
    import scratch  # noqa: PLC0415

    rows = [r for r in scratch.ledger_rows(d, "context.turn")
            if isinstance(r.get("data"), dict) and r["data"].get("place") == place]
    words: dict[str, str] = {}
    for sid in sorted({r.get("session_id") for r in rows if isinstance(r.get("session_id"), str)}):
        for node in scratch.history_nodes(d, sid):
            if node.get("kind") == "user_message" and isinstance(node.get("turn_id"), str):
                words[node["turn_id"]] = (words.get(node["turn_id"], "") + " " + str(node.get("text", ""))).strip()
    ex = [(r["data"]["exchange"], r.get("session_id")) for r in rows if r["data"].get("exchange") in words]
    out = []
    for k in range(max(len(ex) - turns, 0), len(ex)):
        before = ex[max(k - back, 0):k]
        if not before:
            continue
        out.append({"id": ex[k][0], "class": "past_turn", "message": words[ex[k][0]],
                    "ids": [e for e, _ in before], "texts": [words[e] for e, _ in before],
                    "rules": {e for e, s in before if s == ex[k][1]}, "not_built": []})
    return out, SAME_SESSION


# ---- the run


def plan(probes: list[dict], prices: Prices, model: str) -> dict:
    calls, usd = 0, 0.0
    for p in probes:
        for w in windows(len(p["ids"])):
            calls += 1
            usd += prices.reserve(SYSTEM, prompt(p["message"], [p["texts"][i] for i in w]))
    return {"probes": len(probes), "lines": sum(len(p["ids"]) for p in probes), "calls": calls,
            "worst_case_usd": usd, "model": model}


def run(probes: list[dict], judge, prices: Prices, cap_usd: float, model: str = DEFAULT_MODEL) -> dict:
    spent, calls, unparsed, kept = 0.0, 0, 0, 0
    usage: Counter = Counter()
    stopped = None
    per, total, coverage, fixture = [], Counter(), Counter(), {}
    for p in probes:
        best = [None] * len(p["ids"])
        for w in windows(len(p["ids"])):
            idx = list(w)
            user = prompt(p["message"], [p["texts"][i] for i in idx])
            hold = prices.reserve(SYSTEM, user)
            if spent + hold > cap_usd:
                stopped = "E_CAP: the next call's reservation would pass the cap"
                break
            spent += hold  # reserved before dispatch
            calls += 1
            try:
                scores, u, done = judge(p, idx, user)
            except JudgeFailed as e:
                stopped = str(e)  # the reservation stays spent
                break
            if done and "input_tokens" in u and "output_tokens" in u:
                spent += prices.charge(u) - hold
                usage.update(u)
            else:
                kept += 1
            if scores is None:
                unparsed += 1
                continue
            for i, s in zip(idx, scores):
                best[i] = s if best[i] is None else max(best[i], s)
        c = Counter({k: 0 for k in CELLS})
        for i, e in enumerate(p["ids"]):
            if best[i] is None:
                c["unjudged"] += 1
                continue
            c[{(True, True): "both", (True, False): "judge_only", (False, True): "rules_only",
               (False, False): "neither"}[(best[i] == HELPS, e in p["rules"])]] += 1
        row = {"turn": p["id"], "class": p["class"], **c}
        if p["not_built"]:
            row["not_built"] = p["not_built"]
            fixture.setdefault(p["class"], Counter()).update(c)
        else:
            total.update(c)
            coverage.update({"lines": len(p["ids"]), "judged": len(p["ids"]) - c["unjudged"]})
        per.append(row)
        if stopped:
            break
    by_class: dict[str, Counter] = {}
    for r in per:
        if not r.get("not_built"):
            by_class.setdefault(r["class"], Counter()).update({k: v for k, v in r.items() if isinstance(v, int)})
    return {"turns": per, "total": {k: total[k] for k in CELLS}, "coverage": dict(coverage),
            "by_class": {k: dict(v) for k, v in by_class.items()},
            "not_built_fixture_only": {k: dict(v) for k, v in fixture.items()},
            "calls": calls, "unparsed": unparsed, "reservations_kept": kept, "usage": dict(usage),
            "spent_usd": spent, "cap_usd": cap_usd, "stopped": stopped, "model": model}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--cap-usd", type=float, help="REQUIRED: the most it may spend, in dollars")
    ap.add_argument("--dry-run", action="store_true", help="count the calls and the worst case; call nothing")
    ap.add_argument("--judge", choices=("model", "fake"), default="model")
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--api-base", default=API)
    ap.add_argument("--price-in", type=float, help="dollars a million input tokens (an unknown model or endpoint)")
    ap.add_argument("--price-out", type=float, help="dollars a million output tokens")
    ap.add_argument("--cases", help="a labelled case set (the synthetic one)")
    ap.add_argument("--held", help="with --cases: a strategy's held file (else the own-place rule)")
    ap.add_argument("--store", help="a COPY of a daemon's state directory")
    ap.add_argument("--bin-dir", default="target/release")
    ap.add_argument("--place")
    ap.add_argument("--turns", type=int, default=20)
    ap.add_argument("--back", type=int, default=96, help="exchanges before each turn the judge reads")
    ap.add_argument("--out", help="a new directory for yardstick.json (and the scratch daemon)")
    a = ap.parse_args(argv)
    if a.cap_usd is None or not a.cap_usd > 0:
        raise CapNeeded("yardstick: refused: it calls a model; give --cap-usd, the most it may spend")
    prices = Prices(a.model, a.api_base, a.price_in, a.price_out)
    if bool(a.cases) == bool(a.store):
        raise SystemExit("yardstick: give --cases or --store, one of them")
    if a.judge == "fake" and not a.cases:
        raise SystemExit("yardstick: the fake judge reads a case set's evidence: --cases")
    import scratch  # noqa: PLC0415

    store = scratch.check_copy(Path(a.store)) if a.store else None
    out = scratch.check_out(Path(a.out), store) if a.out else None
    if out is not None:
        out.mkdir(parents=True, exist_ok=True)
    if a.cases:
        cs = vc.load(Path(a.cases))
        probes, basis = probes_from_cases(cs, json.loads(Path(a.held).read_text()) if a.held else None)
        oracle = {c["id"]: set(c["evidence"]) for c in cs["cases"]}
        return finish(a, out, probes, basis, prices, lambda: FakeJudge(oracle))
    if not (a.place and out):
        raise SystemExit("yardstick: --store needs --place and --out")
    try:
        with scratch.Scratch(Path(a.bin_dir), out, store=store) as d:
            probes, basis = probes_from_store(d, a.place, a.turns, a.back)
    except scratch.Failed as e:
        raise SystemExit(f"yardstick: {e}") from None
    return finish(a, out, probes, basis, prices, None)


def finish(a, out: Path | None, probes: list[dict], basis: str, prices: Prices, fake) -> int:
    p = plan(probes, prices, a.model)
    if a.dry_run:
        print(json.dumps({"dry_run": True, "rules_basis": basis, **p, "cap_usd": a.cap_usd,
                          "fits_the_cap": p["worst_case_usd"] <= a.cap_usd}, indent=2, sort_keys=True))
        return 0
    if a.judge == "fake":
        judge = fake()
    else:
        key = os.environ.get("ANTHROPIC_API_KEY")
        if not key:
            raise SystemExit("yardstick: the model judge needs ANTHROPIC_API_KEY in its environment")
        judge = ModelJudge(a.model, a.api_base, key)
    r = {"plan": p, "rules_basis": basis, **run(probes, judge, prices, a.cap_usd, a.model)}
    if out is not None:
        (out / "yardstick.json").write_text(json.dumps(r, indent=2, sort_keys=True) + "\n")
    print(json.dumps({k: r[k] for k in ("plan", "rules_basis", "total", "coverage", "by_class",
                                        "not_built_fixture_only", "calls", "spent_usd", "stopped")},
                     indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
