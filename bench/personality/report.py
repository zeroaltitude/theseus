#!/usr/bin/env python3
"""The personality exam's report: `docs/benchmarks/<date>-personality-<slug>.md`, its JSON and CSV, and SVG figures.

    python3 bench/personality/report.py --scores DIR/scores.json --date 2026-10-14 --slug first-run [--out DIR]
    python3 bench/personality/exam.py report --run DIR --date … --slug …          (the same)

Written from `scores.json` (what `exam.py score` leaves): the answer first (computed from the numbers, and the
numbers only), the per-family scores by arm with Wilson intervals and the persona-free ("generic") score beside, the
preference matrix by context, the rubric judge's means, the blind-pair result when the owner has answered, the
cost, the threats, the commands that run it again, and the improvement assessment: what the persona bought
Theseus, and the gaps that remain against the fair Claude Code arm. The maintainer writes the report's prose
around it; every number in it is the data's. Figures use `bench/report/charts.py`'s house style.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPORT = HERE.parent / "report"
sys.path.insert(0, str(REPORT))
sys.path.insert(0, str(HERE))

import draft  # noqa: E402  (bench/report/draft.py: write_outputs, skeleton, figures_md)

from draft import charts, pct  # noqa: E402

ARM_ORDER = ("theseus", "theseus-persona", "claude-code", "claude-code-fair")
LABELS = {"theseus": "Theseus (as configured)", "theseus-persona": "Theseus + persona",
          "claude-code": "Claude Code (bare)", "claude-code-fair": "Claude Code + persona (fair)"}
# The house palette has a fixed slot per arm (charts.ARMS); the exam's four arms take four of its harness slots, and
# the legend names them (a series' `label`), so a slot's name in another report does not carry over.
CHART_ARM = {"theseus": "theseus", "theseus-persona": "theseus-batching", "claude-code": "claude-code",
             "claude-code-fair": "openclaw"}
FAMILY_ORDER = ("identity", "owner", "preference", "correction", "candour", "register", "initiative", "surfaces",
                "models")
FAMILY_WORDS = {
    "identity": "P1 identity and voice", "owner": "P2 knows the owner", "preference": "P3 a preference stated once",
    "correction": "P4 learns from a correction", "candour": "P5 candour", "register": "P6 register and warmth",
    "initiative": "P7 initiative", "surfaces": "the same prompt on another surface",
    "models": "the same probe on each routed profile",
}


def arms_in(sc: dict) -> list[str]:
    return [a for a in ARM_ORDER if a in sc["arms"]]


def rate_text(r: dict) -> str:
    if not r["n"]:
        return "–"
    return f'{pct(r["p"])} ({r["k"]}/{r["n"]}; {pct(r["lo"])} to {pct(r["hi"])})'


def families_of(sc: dict) -> list[str]:
    seen = set()
    for a in sc["arms"].values():
        seen |= {f for f, r in a["cell"]["families"].items() if r["n"]}
    return [f for f in FAMILY_ORDER if f in seen] + sorted(seen - set(FAMILY_ORDER))


def family_table(sc: dict) -> str:
    arms = arms_in(sc)
    t = "| Family | " + " | ".join(LABELS[a] for a in arms) + " |\n|---|" + "---|" * len(arms) + "\n"
    for f in families_of(sc):
        t += f"| {FAMILY_WORDS.get(f, f)} | " + " | ".join(rate_text(sc["arms"][a]["cell"]["families"].get(
            f, {"n": 0})) for a in arms) + " |\n"
    t += "| **All probes** | " + " | ".join(f'**{rate_text(sc["arms"][a]["cell"]["overall"])}**' for a in arms) + " |\n"
    t += "| Persona-free probes only | " + " | ".join(
        rate_text(sc["arms"][a]["cell"]["overall"]["generic"]) for a in arms) + " |\n"
    return t


def context_table(sc: dict) -> str:
    arms = arms_in(sc)
    ctxs = [c for c in ("same", "compaction", "new_session", "project", "surface", "later")
            if any(sc["arms"][a]["cell"]["contexts"].get(c, {"n": 0})["n"] for a in arms)]
    if not ctxs:
        return "_The run had no preference probes._\n"
    t = "| Context | " + " | ".join(LABELS[a] for a in arms) + " |\n|---|" + "---|" * len(arms) + "\n"
    for c in ctxs:
        t += f"| {c} | " + " | ".join(rate_text(sc["arms"][a]["cell"]["contexts"].get(c, {"n": 0})) for a in arms) + " |\n"
    forced = [(a, k, v["forced"]) for a in arms for k, row in sc["arms"][a]["cell"]["preferences"].items()
              for c, v in row.items() if c == "compaction"]
    miss = [f"{LABELS[a]} {k}" for a, k, f in forced if f is False]
    if miss:
        t += ("\n**The compaction context was not forced for** " + "; ".join(miss) +
              ": no compaction ran between the preference and its ask, so that cell tests the session, not a "
              "summary.\n")
    return t


def judge_table(sc: dict) -> str:
    arms = arms_in(sc)
    if not any(sc["arms"][a]["cell"]["judge"]["n"] for a in arms):
        return "_The rubric judge was not run (`exam.py judge`)._\n"
    dims = ("voice", "candour", "register", "warmth", "initiative")
    t = "| Mean score, 1 to 5 | " + " | ".join(LABELS[a] for a in arms) + " |\n|---|" + "---|" * len(arms) + "\n"
    for d in dims:
        row = []
        for a in arms:
            vals = [fam[d] for fam in sc["arms"][a]["cell"]["judge"]["by_family"].values() if d in fam]
            row.append(f"{sum(vals) / len(vals):.2f}" if vals else "–")
        t += f"| {d} | " + " | ".join(row) + " |\n"
    t += "| replies judged / unscored | " + " | ".join(
        f'{sc["arms"][a]["cell"]["judge"]["n"]} / {sc["arms"][a]["cell"]["judge"]["errors"]}' for a in arms) + " |\n"
    t += "| flagged as flattery | " + " | ".join(_flat(sc["arms"][a]["cell"]["judge"]) for a in arms) + " |\n"
    return t


def _flat(j: dict) -> str:
    n = sum(1 for _ in j["flattery"].values())
    if not n or not j["n"]:
        return "–"
    return pct(sum(v * 1 for v in j["flattery"].values()) / n)


def blind_text(b: dict | None) -> str:
    if not b:
        return ("_Not answered yet._ The sheet (`blind/sheet.md`) holds 20 pairs of replies, Theseus + persona against "
                "the fair Claude Code arm, with the arms hidden; the owner answers one key a pair in "
                "`blind/answers.txt`, and `exam.py blind --answers` tallies it.\n")
    w = b["wins"]
    return (f'{b["answered"]} of {b["pairs"]} pairs answered. Theseus + persona was picked {w["theseus-persona"]} '
            f'times, Claude Code + persona (fair) {w["claude-code-fair"]}; {b["ties"]} ties, {b["passes"]} passes. '
            f'Of the {b["decided"]} decided, Theseus took {pct(b["share"])} (95% Wilson {pct(b["lo"])} to '
            f'{pct(b["hi"])}; sign test p = {b["sign_p"]:.3f}).\n')


def cost_table(sc: dict) -> str:
    arms = arms_in(sc)
    t = "| | " + " | ".join(LABELS[a] for a in arms) + " |\n|---|" + "---|" * len(arms) + "\n"
    t += "| Model dollars | " + " | ".join(f'${sc["arms"][a]["run"].get("spent_usd", 0):.2f}' for a in arms) + " |\n"
    t += "| Steps run / planned | " + " | ".join(
        f'{sc["arms"][a]["run"].get("steps_done", 0)} / {sc["plan"]["steps"]}' for a in arms) + " |\n"
    t += "| Compactions (steps) | " + " | ".join(
        str(sc["arms"][a]["run"].get("compactions") or "none") for a in arms) + " |\n"
    j = sc.get("judge")
    t += f"\nThe judge ({j['model']}, {j['calls']} calls): ${j['spent_usd']:.2f}.\n" if j else ""
    return t


def gaps(sc: dict) -> tuple[list[str], list[str]]:
    """What the persona bought Theseus (family by family), and where Theseus + persona is still behind the fair
    Claude Code arm by more than ten points."""
    bought, behind = [], []
    if "theseus" in sc["arms"] and "theseus-persona" in sc["arms"]:
        for f in families_of(sc):
            a = sc["arms"]["theseus"]["cell"]["families"].get(f, {"n": 0})
            b = sc["arms"]["theseus-persona"]["cell"]["families"].get(f, {"n": 0})
            if a["n"] and b["n"] and abs(b["p"] - a["p"]) >= 0.1:
                bought.append(f'{FAMILY_WORDS.get(f, f)}: {pct(a["p"])} to {pct(b["p"])}')
    if "theseus-persona" in sc["arms"] and "claude-code-fair" in sc["arms"]:
        for f in families_of(sc):
            a = sc["arms"]["theseus-persona"]["cell"]["families"].get(f, {"n": 0})
            b = sc["arms"]["claude-code-fair"]["cell"]["families"].get(f, {"n": 0})
            if a["n"] and b["n"] and b["p"] - a["p"] > 0.1:
                behind.append(f'{FAMILY_WORDS.get(f, f)}: {pct(a["p"])} against {pct(b["p"])}')
    return bought, behind


def answer(sc: dict) -> str:
    arms = arms_in(sc)
    parts = [f'{LABELS[a]} {rate_text(sc["arms"][a]["cell"]["overall"])}' for a in arms]
    s = (f'On {sc["plan"]["probes"]} probes in {len(families_of(sc))} families, scored by deterministic checks: '
         + "; ".join(parts) + ". ")
    bought, behind = gaps(sc)
    if bought:
        s += "The persona moved Theseus on " + "; ".join(bought) + ". "
    elif "theseus" in sc["arms"] and "theseus-persona" in sc["arms"]:
        s += "The persona moved no family by ten points. "
    if "claude-code-fair" in sc["arms"] and "theseus-persona" in sc["arms"]:
        s += ("Against the fair arm Theseus + persona is behind by more than ten points on " + "; ".join(behind) + ". "
              if behind else "Against the fair arm, Theseus + persona is behind on no family by ten points. ")
    if sc.get("blind"):
        s += blind_text(sc["blind"]).strip()
    return s.strip()


def improvement_text(sc: dict) -> str:
    bought, behind = gaps(sc)
    lines = ["What the persona bought Theseus (families that moved by ten points or more):"]
    lines += [f"- {x}" for x in bought] or ["- nothing at that threshold"]
    lines += ["", "Where Theseus + persona is still behind the fair Claude Code arm by more than ten points:"]
    lines += [f"- {x}" for x in behind] or ["- nowhere at that threshold"]
    pm = {a: sc["arms"][a]["cell"]["preferences"] for a in arms_in(sc)}
    lost = []
    for a, rows in pm.items():
        for kind, row in rows.items():
            for ctx, v in row.items():
                if v["ok"] is False and ctx in ("new_session", "project", "surface", "later"):
                    lost.append(f"{LABELS[a]}: {kind} lost in {ctx}")
    lines += ["", "Preferences lost across a session, project, surface or restart (P3):"]
    lines += [f"- {x}" for x in lost[:24]] or ["- none"]
    if len(lost) > 24:
        lines.append(f"- … and {len(lost) - 24} more in the data file")
    return "\n".join(lines) + "\n"


def threats(sc: dict) -> str:
    unreached = sum(sc["arms"][a]["cell"]["overall"]["unreached"] for a in sc["arms"])
    items = [
        "**The stand-in is not a model.** The exam's offline run proves the checks and the plumbing: oracle answers "
        "score 100% and planted wrong ones 0% on every reply check. It says nothing about a model's character.",
        "**Deterministic checks are blunt.** A regex for \"holds the position\" passes a reply that merely contains "
        "\"still\". The rubric judge and the owner's blind picks are the character signal; the checks are the floor.",
        f'**n is small.** {sc["plan"]["probes"]} probes, one run per arm, one seed ({sc["plan"]["seed"]}): the '
        "Wilson intervals are wide, and a difference under about 15 points between arms is not a finding.",
        "**The fair arm is not equal in every way.** Claude Code gets the persona and card as CLAUDE.md in every "
        "project; it has no wakes (its initiative probes are scored on the reply alone, `ledger` lines n/a), and "
        "its compaction is `/compact` after the plan's marks, not a window. Theseus's compaction is the window.",
        "**\"A week later\" is a restart and later sessions**, not a week: the clock is not moved.",
        "**Surfaces driven here: the CLI, the cockpit's JSON-RPC and a DM in a fake Discord.** Theseus's MCP server "
        "serves memory reads, not turns, so it is not a surface for a persona. Claude Code has one surface; its "
        "other steps run as ordinary turns. The fake Discord is not Discord: no rate limits, no real rendering.",
        "**The judge shares a model family with the arms** when the arms run Sonnet; it sees the rubric, the "
        "persona text and the reply, never the arm's name. It was not calibrated against the owner's picks.",
        f"**Steps not reached:** {unreached} (the budget stopped a run) are counted apart, never as misses.",
    ]
    return "\n".join(f"- {t}" for t in items) + "\n"


def repro(sc: dict) -> str:
    r = sc["run"]
    persona = f" --persona {r['persona_source']}" if r.get("persona_source") not in (None, "built-in") else ""
    return ("```bash\n"
            f"python3 bench/personality/exam.py run --arms {','.join(arms_in(sc))} --model {r['model']} "
            f"--seed {sc['plan']['seed']} --size {sc['plan']['size']} --budget {r['budget_usd']}{persona} "
            "--bin-dir target/release --out /tmp/px\n"
            "python3 bench/personality/exam.py judge --run /tmp/px\n"
            "python3 bench/personality/exam.py blind --run /tmp/px            # then answer blind/answers.txt\n"
            "python3 bench/personality/exam.py score --run /tmp/px\n"
            f"python3 bench/personality/exam.py report --run /tmp/px --date {r['date']} --slug <slug>\n```\n")


def figures(sc: dict) -> list[dict]:
    arms = arms_in(sc)
    figs = []
    fams = families_of(sc)
    series = []
    for a in arms:
        fs = sc["arms"][a]["cell"]["families"]
        series.append({"arm": CHART_ARM[a], "label": LABELS[a],
                       "values": [fs[f]["p"] if f in fs and fs[f]["n"] else None for f in fams]})
    figs.append({"name": "families", "form": "bars", "title": "Probes passed, by family",
                 "question": "On which families does each arm pass the deterministic checks?",
                 "categories": [FAMILY_WORDS.get(f, f) for f in fams], "series": series,
                 "value": {"label": "share of scored probes that pass", "format": "pct", "min": 0, "max": 1},
                 "note": f'Plan {sc["plan"]["digest"]}, seed {sc["plan"]["seed"]}; unscored probes are blank, never zero.'})
    ctxs = [c for c in ("same", "compaction", "new_session", "project", "surface", "later")
            if any(sc["arms"][a]["cell"]["contexts"].get(c, {"n": 0})["n"] for a in arms)]
    if ctxs:
        figs.append({"name": "preference-contexts", "form": "bars", "title": "A preference stated once, by context",
                     "question": "Does a preference stated once hold in the session, after a compaction, in a new session, "
                                 "another project, another surface, and after a restart?",
                     "categories": ctxs, "series": [
                         {"arm": CHART_ARM[a], "label": LABELS[a],
                          "values": [sc["arms"][a]["cell"]["contexts"].get(c, {"n": 0, "p": None})["p"]
                                     if sc["arms"][a]["cell"]["contexts"].get(c, {"n": 0})["n"] else None
                                     for c in ctxs]} for a in arms],
                     "value": {"label": "share of preference probes honoured", "format": "pct", "min": 0, "max": 1}})
    if any(sc["arms"][a]["cell"]["judge"]["n"] for a in arms):
        dims = ("voice", "candour", "register", "warmth", "initiative")

        def mean(a, d):
            vals = [fam[d] for fam in sc["arms"][a]["cell"]["judge"]["by_family"].values() if d in fam]
            return sum(vals) / len(vals) if vals else None
        figs.append({"name": "judge", "form": "bars", "title": "The rubric judge's scores",
                     "question": "How does the judge, blind to the arm, score voice, candour, register, warmth and initiative?",
                     "categories": list(dims), "series": [{"arm": CHART_ARM[a], "label": LABELS[a],
                                                           "values": [mean(a, d) for d in dims]} for a in arms],
                     "value": {"label": "mean score, 1 to 5", "format": "num", "min": 1, "max": 5}})
    return figs


def rows_of(sc: dict) -> list[dict]:
    rows = []
    for a in arms_in(sc):
        for p in sc["arms"][a]["cell"]["probes"]:
            rows.append({"arm": a, "probe": p["probe"], "family": p["family"], "dim": p["dim"], "context": p["ctx"],
                         "preference": p["pref"], "surface": p["surface"], "profile": p["profile"],
                         "passed": p["ok"], "unreached": p["unreached"], "generic": p["generic"]})
    return rows


def make(sc: dict, date: str, slug: str, out: Path, force: bool = False) -> list[Path]:
    name = draft.report_name(date, "personality", slug)
    figs = figures(sc)
    fm = draft.figures_md(name, figs)
    sc = {**sc, "run": {**sc["run"], "date": date}}
    glance = [("Suite", "the personality exam (`bench/personality`)"),
              ("Arms", "; ".join(LABELS[a] for a in arms_in(sc))),
              ("Model", f'{sc["run"]["model"]} at {sc["run"]["effort"]} effort; judge {sc["run"].get("judge_model", "–")}'),
              ("Persona", f'{sc["run"]["persona"]["name"]} (digest `{sc["run"]["persona_digest"]}`), owner called '
                          f'{sc["run"]["persona"]["owner_call"]}, {sc["run"]["persona"]["tz"]}'),
              ("Plan", f'`{sc["plan"]["digest"]}`: seed {sc["plan"]["seed"]}, {sc["plan"]["size"]}, '
                       f'{sc["plan"]["probes"]} probes, window {sc["plan"]["window"]:,} tokens'),
              ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    sections = {
        "The setup": (
            "Four arms on one plan: Theseus as configured (no persona file), Theseus with the persona and the "
            "owner's card compiled as `[personas.<n>] files`, Claude Code bare, and Claude Code with the same persona "
            "and card in its user-level CLAUDE.md. Each arm runs on fresh state, behind distractor sessions of "
            "invented facts, with compactions forced by a scratch context window. The probes are generated from "
            "TOML families and parametrized by the persona and card under test; none is shown to a model before it "
            "is asked.\n"),
        "Results": ("### Scores by family\n\n" + family_table(sc) + "\n" + fm.get("families", "") +
                    "\n### A preference stated once\n\n" + context_table(sc) + "\n" + fm.get("preference-contexts", "") +
                    "\n### The rubric judge\n\n" + judge_table(sc) + "\n" + fm.get("judge", "") +
                    "\n### The owner's blind picks\n\n" + blind_text(sc.get("blind")) +
                    "\n### What the persona bought, and what is left\n\n" + improvement_text(sc)),
        "Threats to validity": threats(sc),
        "What it cost": cost_table(sc),
        "Reproduction": repro(sc),
        "Data": f"[`{name}.json`]({name}.json): every arm's scores, probe verdicts and the figures' specs; "
                f"[`{name}.csv`]({name}.csv): one row per probe and arm, no transcript.",
    }
    sk = draft.skeleton(f"Personality: Theseus and Claude Code, with and without a persona ({date})", glance, sections)
    sk = sk.replace("> **The answer.** _One paragraph: the headline with its numbers and their intervals. Say what the "
                    "run teaches that the tables don't._", "> **The answer.** " + answer(sc))
    sk = sk.replace("_What the run asked, and why it mattered then._",
                    "Is Theseus one identity the owner shapes, the same on every surface, model and compaction, and does "
                    "it know the owner, hold a stated preference, learn from a correction, stay candid, fit the moment "
                    "and follow up unasked, as well as Claude Code given the same persona?")
    data = {"report": name, "suite": "personality", "date": date, "arms": [{"key": a, "label": LABELS[a]}
                                                                          for a in arms_in(sc)],
            "run": sc["run"], "plan": sc["plan"], "summary": {a: {k: v for k, v in sc["arms"][a]["cell"].items()
                                                                  if k != "probes"} for a in arms_in(sc)},
            "blind": sc.get("blind"), "judge": sc.get("judge"), "figures": figs}
    return draft.write_outputs(out, name, data, rows_of(sc), sk, force)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--scores", type=Path, required=True)
    ap.add_argument("--date", required=True, help="YYYY-MM-DD, the day the run happened")
    ap.add_argument("--slug", default="")
    ap.add_argument("--out", type=Path, default=draft.DOCS)
    ap.add_argument("--force", action="store_true")
    a = ap.parse_args(argv)
    for p in make(json.loads(a.scores.read_text()), a.date, a.slug, a.out, a.force):
        print(p)
    return 0


if __name__ == "__main__":
    sys.exit(main())
