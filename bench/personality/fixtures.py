"""Test fixtures: results for a plan without a daemon or a model, and a second persona that shares no word with the
default (the parametrization's proof). Not a module of the exam; the tests import it."""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import persona as pe  # noqa: E402
from plan import Plan  # noqa: E402

OTHER_TOML = """
[persona]
name = "Marlin"
owner_call = "Sol"
tz = "Europe/Lisbon"
tz_names = ["WET", "WEST", "Lisbon", "Europe/Lisbon"]
voice = "You are dry, exact, and kind."

[[fact]]
key = "tea"
statement = "<<owner>> drinks tea, never coffee, and takes a break at 4 pm."
ask = "What should I have with my afternoon break?"
checks = '''reply has /tea/i'''
oracle = "Tea, as always."
wrong = "A strong coffee."
"""


def other_persona(tmp: Path) -> pe.Persona:
    f = tmp / "marlin.toml"
    f.write_text(OTHER_TOML)
    return pe.load(f, ["Harrow"])


def fake_results(plan: Plan, mode: str, persona_digest: bool = True, wakes: bool = True) -> dict[int, dict]:
    """What an arm would leave: every step answered; a probe's reply its oracle's (mode `oracle`) or its wrong
    answer's (`wrong`); a step that wants the daemon's record gets one (a wake only when the oracle calls for one)."""
    res = {}
    for s in plan.steps:
        r = {"index": s.index, "reply": "ok.", "exit": 0, "cost_usd": 0.0, "compacted": False}
        if s.kind == "probe":
            r["reply"] = s.oracle if mode == "oracle" else s.wrong
            calls = s.oracle_calls if mode == "oracle" else s.wrong_calls
            if "ledger" in s.needs:
                r["ledger"] = {"persona_digest": persona_digest, "wakes_new": len(calls) if wakes else 0}
        res[s.index] = r
    return res
