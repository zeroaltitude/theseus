"""F10's scorecard: the table of steps with yes or no per check and per arm,
its totals, and the same as JSON (theseus-qy2a).

    python3 scripts/f10/scorecard.py RUN_DIR [RUN_DIR ...] [--out PREFIX]

Each RUN_DIR is a driver's run (or a fixture): its arm and size come from its
run.json. The markdown goes to stdout, or to PREFIX.md with PREFIX.json beside
it.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import checks  # noqa: E402
import steps  # noqa: E402


def totals(results):
    """{match, beat, by_step}: the yes answers among the 42 match checks, among
    B1 to B5, and per step."""
    by_step = {}
    for sid, _, _, cs in steps.STEPS:
        by_step[sid] = sum(1 for c, _ in cs if results[f"{sid}{c}"][0])
    match = sum(by_step.values())
    beat = sum(1 for b, _ in steps.BEAT[3] if results[b][0])
    return {"match": match, "beat": beat, "by_step": by_step}


def column(run):
    return f"{run.arm} {run.meta['size']}"


def score(runs):
    """Every run's results and totals, in the order given."""
    out = []
    for run in runs:
        results = checks.judge(run)
        out.append({"arm": run.arm, "size": run.meta["size"], "version": run.meta.get("version", ""),
                    "model": run.meta.get("model", ""), "results": results, "totals": totals(results)})
    return out


def markdown(runs, scored):
    """The scorecard: one row per check, a yes or no per run, each step's
    count, the totals, and each answer's evidence below the table."""
    heads = [column(r) for r in runs]
    out = ["| Step | Check | " + " | ".join(heads) + " |", "|---|---|" + "---|" * len(heads)]
    for sid, weight, title, cs in steps.STEPS:
        for c, words in cs:
            cid = f"{sid}{c}"
            cells = ["yes" if s["results"][cid][0] else "no" for s in scored]
            out.append(f"| {sid} {title} ({weight}) | {c}. {words} | " + " | ".join(cells) + " |")
        counts = [f"**{s['totals']['by_step'][sid]} of {len(cs)}**" for s in scored]
        out.append(f"| {sid} | | " + " | ".join(counts) + " |")
    for b, words in steps.BEAT[3]:
        cells = ["yes" if s["results"][b][0] else "no" for s in scored]
        out.append(f"| Beat | {b}. {words} | " + " | ".join(cells) + " |")
    out.append("| **Match** | of 42 | " + " | ".join(f"**{s['totals']['match']}**" for s in scored) + " |")
    out.append("| **Beat** | of 5 | " + " | ".join(f"**{s['totals']['beat']}**" for s in scored) + " |")
    for run, s in zip(runs, scored):
        out += ["", f"### {column(run)}" + (f" ({s['version']})" if s["version"] else ""), ""]
        for cid in steps.check_ids():
            ok, evidence = s["results"][cid]
            out.append(f"- {cid} {'yes' if ok else 'no'}: {evidence}")
    return "\n".join(out) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("runs", nargs="+", help="run directories")
    ap.add_argument("--out", help="write PREFIX.md and PREFIX.json instead of printing")
    a = ap.parse_args(argv)
    runs = [checks.Run.load(p) for p in a.runs]
    scored = score(runs)
    md = markdown(runs, scored)
    if a.out:
        Path(a.out + ".md").write_text(md)
        Path(a.out + ".json").write_text(json.dumps(scored, indent=1) + "\n")
    else:
        sys.stdout.write(md)
    return 0


if __name__ == "__main__":
    sys.exit(main())
