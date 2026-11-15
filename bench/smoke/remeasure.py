"""route.v10's cases, measured again where a change moves their state (1.0.7, issue 5).

A cases file carries each case's state (the facts the daemon puts beside the message) and the
model and effort the owner says Jev should choose. When a change moves the state of some cases
(Sonnet 5.5's cache-read price moved the switch-cost word of 53), the state is derived but what
Jev answers to it is not: it is measured. `scripts/remeasure-route-cases.sh` replays the file
before and after, interleaved, and calls this module to say which cases changed and what Jev
did with each. Nothing here is reportable, and nothing is written under docs/benchmarks/.

    python3 bench/smoke/remeasure.py cases BASE.toml NEW.toml
    python3 bench/smoke/remeasure.py report OUT_DIR

`report` reads `OUT_DIR/{route,effort}-{base,new}-N.json` (one `theseus --json judge replay
--cases` result each, N from 1) and the two cases files beside them, writes `OUT_DIR/report.md`
and `OUT_DIR/summary.json`, and prints one line. It decides nothing: a label moves only on the
owner's word. Standard library only; Python >= 3.11.
"""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path
from typing import Any

# The fields of a case that are its state, as `route_cases.rs`' `state_of` resolves them, and its labels.
STATE = ("place", "restricted_models", "price_period", "est_tokens", "last_reply_model", "current_model",
         "switch_cost", "cheaper_model")
LABELS = ("model", "effort")


class RemeasureError(Exception):
    """Results that cannot be compared: a missing run, a run that did not answer every case."""


def resolved(text: str) -> dict[str, dict[str, Any]]:
    """Each case of a cases file with its defaults applied, by id: its state fields and its labels.

    `cheaper_model` rides with `cheaper_to_switch` and with nothing else, as the parser reads it, and
    `current_model` is the last reply's unless the case names it."""
    file = tomllib.loads(text)
    d = file.get("defaults", {})
    out: dict[str, dict[str, Any]] = {}
    for c in file["case"]:
        s = {k: c.get(k, d.get(k)) for k in STATE}
        s["current_model"] = c.get("current_model", s["last_reply_model"])
        if s["switch_cost"] != "cheaper_to_switch":
            s["cheaper_model"] = None
        out[c["id"]] = {**s, **{k: c.get(k) for k in LABELS}, "must": bool(c.get("must"))}
    return out


def changed(base: str, new: str) -> list[str]:
    """The ids whose state or labels differ between two cases files, in the new file's order. A case that
    is in one file only is a difference too."""
    a, b = resolved(base), resolved(new)
    ids = [i for i in b if a.get(i) != b[i]]
    return ids + [i for i in a if i not in b]


def _runs(out: Path, question: str, arm: str) -> list[dict[str, Any]]:
    """The replay results of one question and arm, in run order. A run that graded no cases, or answered
    fewer than it graded, is an error: a gap read as a miss would be a measurement of the outage."""
    found = sorted(out.glob(f"{question}-{arm}-*.json"), key=lambda p: int(p.stem.rsplit("-", 1)[1]))
    if not found:
        raise RemeasureError(f"no {question}-{arm}-N.json in {out}")
    runs = []
    for p in found:
        cases = json.loads(p.read_text()).get("cases") or {}
        rows = cases.get("cases") or []
        if not rows or int(cases.get("answered") or 0) != len(rows):
            raise RemeasureError(f"{p.name}: {cases.get('answered')} answered of {len(rows)} graded")
        runs.append(cases)
    return runs


def _bar(run: dict[str, Any], question: str) -> tuple[int, int]:
    for b in run.get("bars") or []:
        if b.get("question") == question:
            return int(b["ok"]), int(b["of"])
    raise RemeasureError(f"a {run.get('pack')} replay printed no {question} bar")


def _row(run: dict[str, Any], case: str) -> dict[str, Any]:
    for r in run["cases"]:
        if r["id"] == case:
            return r
    raise RemeasureError(f"case {case} is not in a {run.get('pack')} replay")


def _says(runs: list[dict[str, Any]], case: str, question: str) -> list[str]:
    """What each run graded for `case`: the answer, with the raw pick and its confidence where the bar turned
    it into the pack's no-match (`sonnet 0.31` reads: top pick, under the bar)."""
    out = []
    for run in runs:
        c = _row(run, case)[question]
        out.append(c["got"] if not c.get("sub_confirm") else f"({c['raw']} {c['confidence']:.2f})")
    return out


def _agree(runs: list[dict[str, Any]], case: str, question: str) -> int:
    return sum(1 for run in runs if _row(run, case)[question]["agrees"])


def summarize(out: Path) -> dict[str, Any]:
    """Both arms' bars, run by run, and for every case whose state or labels changed what each run answered."""
    base_text, new_text = (out / "base.cases.toml").read_text(), (out / "new.cases.toml").read_text()
    ids = changed(base_text, new_text)
    state = {"base": resolved(base_text), "new": resolved(new_text)}
    arms: dict[str, Any] = {}
    cases: dict[str, Any] = {}
    for arm in ("base", "new"):
        route, effort = _runs(out, "route", arm), _runs(out, "effort", arm)
        if len(route) != len(effort):
            raise RemeasureError(f"{arm}: {len(route)} route runs and {len(effort)} effort runs")
        arms[arm] = {"runs": len(route),
                     "model": [_bar(r, "model") for r in route], "must": [_bar(r, "must") for r in route],
                     "effort": [_bar(r, "effort") for r in effort]}
        arms[arm]["_route"], arms[arm]["_effort"] = route, effort
    if arms["base"]["runs"] != arms["new"]["runs"]:
        raise RemeasureError("the two arms ran a different number of times")
    for i in ids:
        row = {"label": {k: state["new"].get(i, {}).get(k) for k in LABELS},
               "must": state["new"].get(i, {}).get("must", False)}
        for arm in ("base", "new"):
            if i not in state[arm]:
                continue
            row[arm] = {"state": {k: state[arm][i][k] for k in ("switch_cost", "cheaper_model")},
                        "model": _says(arms[arm]["_route"], i, "model"),
                        "model_got": [_row(r, i)["model"]["got"] for r in arms[arm]["_route"]],
                        "model_right": _agree(arms[arm]["_route"], i, "model"),
                        "effort": _says(arms[arm]["_effort"], i, "effort"),
                        "effort_close": sum(1 for r in arms[arm]["_effort"] if _row(r, i)["effort"]["close"])}
        cases[i] = row
    for a in arms.values():
        del a["_route"], a["_effort"]
    return {"changed": ids, "arms": arms, "cases": cases}


def _counts(bars: list[tuple[int, int]]) -> str:
    return " / ".join(str(ok) for ok, _ in bars) + f" of {bars[0][1]}"


def report_md(s: dict[str, Any]) -> str:
    """The report: the bars of each arm, then the cases whose answer moved, then the rest of the changed cases."""
    n = s["arms"]["base"]["runs"]
    lines = [f"# route.v10 measured again: {len(s['changed'])} cases changed state, {n} interleaved runs per arm", "",
             "Not reportable, and decides nothing: a label moves only on the owner's word.", "",
             "| bar | before (the base cases file) | after (this branch's) |", "|---|---|---|"]
    for q in ("model", "must", "effort"):
        lines.append(f"| {q} | {_counts(s['arms']['base'][q])} | {_counts(s['arms']['new'][q])} |")
    moved = [i for i in s["changed"] if _moved(s["cases"][i])]
    lines += ["", f"## The {len(moved)} cases whose answer moved (right in a different number of runs, or a different answer)", ""]
    lines += _table(s, moved) if moved else ["None."]
    rest = [i for i in s["changed"] if i not in moved]
    lines += ["", f"## The {len(rest)} other changed cases (answered as before)", ""]
    lines += _table(s, rest) if rest else ["None."]
    return "\n".join(lines) + "\n"


def _moved(c: dict[str, Any]) -> bool:
    b, a = c.get("base"), c.get("new")
    if not b or not a:
        return True
    return (b["model_right"], b["effort_close"]) != (a["model_right"], a["effort_close"]) \
        or sorted(b["model_got"]) != sorted(a["model_got"])


def _word(x: dict[str, Any] | None) -> str:
    """A case's switch-cost word as the state carries it, with the cheaper models it names."""
    if not x:
        return "-"
    named = x["state"]["cheaper_model"]
    return x["state"]["switch_cost"] + (f" ({named})" if named else "")


def _cell(x: dict[str, Any] | None, key: str) -> str:
    return "-" if not x else ", ".join(x[key])


def _table(s: dict[str, Any], ids: list[str]) -> list[str]:
    out = ["| case | label | state before -> after | model before | model after | effort before | effort after |",
           "|---|---|---|---|---|---|---|"]
    for i in ids:
        c = s["cases"][i]
        b, a = c.get("base"), c.get("new")
        lab = f"{c['label']['model']} {c['label']['effort']}" + (" **must**" if c["must"] else "")
        out.append(f"| {i} | {lab} | {_word(b)} -> {_word(a)} | {_cell(b, 'model')} | {_cell(a, 'model')} "
                   f"| {_cell(b, 'effort')} | {_cell(a, 'effort')} |")
    return out


def main(argv: list[str]) -> int:
    if len(argv) == 3 and argv[0] == "cases":
        print(",".join(changed(Path(argv[1]).read_text(), Path(argv[2]).read_text())))
        return 0
    if len(argv) == 2 and argv[0] == "report":
        out = Path(argv[1])
        try:
            s = summarize(out)
        except RemeasureError as e:
            print(f"remeasure: {e}", file=sys.stderr)
            return 1
        (out / "summary.json").write_text(json.dumps(s, indent=1) + "\n")
        (out / "report.md").write_text(report_md(s))
        moved = sum(1 for i in s["changed"] if _moved(s["cases"][i]))
        print(f"remeasure: {len(s['changed'])} cases changed state, {moved} answered differently; "
              f"model {_counts(s['arms']['base']['model'])} before, {_counts(s['arms']['new']['model'])} after; "
              f"report {out / 'report.md'}")
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
