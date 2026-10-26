"""The benchmark manifest (theseus-jda0z): `docs/benchmarks/index.json`, one row per measured arm of each published dataset.

The Benchmarks page reads it to say what the newest measurement of each (benchmark, tier, arm) is, and as of when.
Every run's report step (`draft.py`) appends its own rows; this tool also rebuilds the whole file from the datasets
already published.

    python3 bench/report/manifest.py build [--dir docs/benchmarks]      # scan every <report>.json, write index.json
    python3 bench/report/manifest.py append docs/benchmarks/<report>.json   # add or replace one report's rows

A dataset yields rows when its `summary` holds arm entries with a pass rate and its interval, in either of the two
shapes the published datasets use:

- flat: `{"trials", "solved", "rate", "wilson": [lo, hi], "tasks", ...}` (the tiered Terminal-Bench reports);
- nested: `{"trials", "tasks", "pass": {"k", "n", "rate", "lo", "hi"}, ...}` (the earlier Harbor reports).

Anything else (the gate bench, the memory exam, the recall bench, the async families, `omnibus.json`) is skipped with a
named reason, never guessed at. A later report repeats or re-cuts the figures of earlier runs it compares against. A row is not a
new measurement when its report marks its own arms "(this run)" and the row's label lacks it (`comparison`), or when
its figures match an earlier row of the same (benchmark, tier, arm) exactly (`carried_from`, that report). Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
DOCS = HERE.parent.parent / "docs" / "benchmarks"
INDEX = "index.json"
VERSION = 1
ABOUT = ("One row per measured arm of each published benchmark dataset in docs/benchmarks/, written by "
         "bench/report/manifest.py (draft.py appends a run's rows). A row with comparison or carried_from is not a "
         "new measurement. Sorted by date, report, variant.")

# What a row says about the model, the routing, the attempts and the tasks of its arm (theseus-uhc2d). Every one is
# additive and always present: an unknown value is `null`, never an absent key, so a reader tests one way.
#
#   model       the arm's own model(s): a string for a pinned arm, a sorted list for a routed one, from the summary
#               entry's `model`; else the dataset's prose `model` string. `model_note` is always that prose string.
#   provider    `anthropic`, `openai`, ... from the entry's `provider`, else read off a single model's name; else null.
#   routing     `routed` | `pinned` | `n/a` (the entry's `routing`, else from its models and its arm key); else null.
#   effort      the efforts the arm asked for, a list of strings (the entry's `effort`); null when unrecorded.
#   k           attempts per task, an integer, ONLY when the arm ran the whole task set and trials divide into it
#               evenly (or the entry states it); null otherwise. It is NOT `metrics.pass_rate.k` (solved trials).
#   task_set    `full` | `subset:<n>`; `subset_of` names the report whose task set it cuts (null if unstated).
#   framing     what the dataset's contrast is. `harness`: every arm with a known model ran the same one, so only the
#               harness varies (benchframing.ts `A`); `model`: the arms' models differ (`B`); `routed`: this row is an
#               arm that picked its own models (`T`). A dataset's own `framing` wins; with fewer than two known models
#               the default is `harness`. `kind` is the bench family (`harbor`, `harness`), not a framing: never read.
FRAMINGS = ("harness", "model", "routed")
ROUTINGS = ("routed", "pinned", "n/a")
PROVIDERS = (("anthropic", ("claude",)), ("openai", ("gpt", "o1", "o3", "o4", "codex")))

# Summary keys a dataset may carry its money and time under, by shape (the name in the row, the key, the interval).
USD_TOTAL = ("usd_total", "cost_usd", "dollars")


class ManifestError(Exception):
    """A dataset that claims a pass rate but cannot make a whole row."""


def _num(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def _pair(v: Any) -> bool:
    return isinstance(v, list) and len(v) == 2 and all(_num(x) for x in v)


def family(variant: str) -> str:
    """The arm a summary key belongs to: Theseus's variants (`theseus-routed`, `theseus-68`) are one arm; the rest are their own."""
    return "theseus" if variant == "theseus" or variant.startswith("theseus-") else variant


def _pass_rate(entry: dict[str, Any]) -> dict[str, Any] | None:
    """The entry's pass rate with its interval, in either shape; None when it holds none."""
    p = entry.get("pass")
    if isinstance(p, dict) and all(_num(p.get(k)) for k in ("k", "n", "rate", "lo", "hi")):
        return {"value": p["rate"], "lo": p["lo"], "hi": p["hi"], "k": p["k"], "n": p["n"]}
    if _num(entry.get("rate")) and _pair(entry.get("wilson")) and _num(entry.get("solved")) and _num(entry.get("trials")):
        lo, hi = entry["wilson"]
        return {"value": entry["rate"], "lo": lo, "hi": hi, "k": entry["solved"], "n": entry["trials"]}
    return None


def _metrics(entry: dict[str, Any]) -> dict[str, Any] | None:
    rate = _pass_rate(entry)
    if rate is None:
        return None
    m: dict[str, Any] = {"pass_rate": rate}
    ci = entry.get("usd_trial_ci")
    if _num(entry.get("usd_trial")) and _pair(ci):
        m["usd_per_trial"] = {"value": entry["usd_trial"], "lo": ci[0], "hi": ci[1]}
    cpt = entry.get("cost_per_trial")
    if "usd_per_trial" not in m and isinstance(cpt, dict) and all(_num(cpt.get(k)) for k in ("mean", "lo", "hi")):
        m["usd_per_trial"] = {"value": cpt["mean"], "lo": cpt["lo"], "hi": cpt["hi"]}
    for key in USD_TOTAL:
        if _num(entry.get(key)):
            m["usd_total"] = {"value": entry[key]}
            break
    if _num(entry.get("usd_solve")):
        m["usd_per_solve"] = {"value": entry["usd_solve"]}
    if _num(entry.get("agent_s_median")):
        m["agent_s_median"] = {"value": entry["agent_s_median"]}
    return m


def _bare(model: str) -> str:
    return model.rsplit("/", 1)[-1]


def _models(entry: dict[str, Any]) -> str | list[str] | None:
    """The arm's own model: a string, a sorted list of two or more, or None when the entry names none."""
    m = entry.get("model")
    if isinstance(m, str) and m:
        return m
    if isinstance(m, list) and m and all(isinstance(x, str) and x for x in m):
        return m[0] if len(set(m)) == 1 else sorted(set(m))
    return None


def provider_of(model: str | list[str] | None) -> str | None:
    """The provider one model name belongs to (`anthropic/x` names it; `claude-*` and `gpt-*` imply it); None if unsure."""
    names = [model] if isinstance(model, str) else model or []
    found = set()
    for n in names:
        head = n.split("/", 1)[0] if "/" in n else None
        if head:
            found.add(head)
            continue
        found.add(next((p for p, pre in PROVIDERS if n.lower().startswith(pre)), None))
    return found.pop() if len(found) == 1 else None


def _task_sets(data: dict[str, Any], found: list) -> dict[str, tuple[str, int | None]]:
    """Per variant: ("full" | "subset:<n>", tasks). A dataset with a `scope` ran a cut of its suite; an arm that
    ran fewer tasks than its dataset's widest arm ran a cut too. Anything else is the whole set."""
    counts = {k: e.get("tasks") if _num(e.get("tasks")) else None for k, e, _ in found}
    widest = max((n for n in counts.values() if n), default=None)
    scoped = isinstance(data.get("scope"), dict)
    out = {}
    for k, n in counts.items():
        cut = n is not None and (scoped or n < widest)
        out[k] = (f"subset:{int(n)}" if cut else "full", n)
    return out


def _framing(data: dict[str, Any], found: list) -> str:
    if data.get("framing") in FRAMINGS:
        return data["framing"]
    pinned = {_bare(m) for _, e, _ in found if isinstance(m := _models(e), str)}
    return "model" if len(pinned) > 1 else "harness"


def _routing(variant: str, entry: dict[str, Any], model: str | list[str] | None) -> str | None:
    if entry.get("routing") in ROUTINGS:
        return entry["routing"]
    if isinstance(model, list) or "routed" in variant:
        return "routed"
    return "pinned" if isinstance(model, str) else None


def rows_of(data: Any, name: str) -> tuple[list[dict[str, Any]], str | None]:
    """A dataset's rows, or ([], why it has none). Raises ManifestError for one that has a pass rate but no whole row."""
    if not isinstance(data, dict):
        return [], "not a JSON object"
    if data.get("kind") == "omnibus":
        return [], "an omnibus summary, not a run"
    summary = data.get("summary")
    if not isinstance(summary, dict):
        return [], "no `summary` object"
    found = [(k, e, _metrics(e)) for k, e in summary.items() if isinstance(e, dict)]
    found = [(k, e, m) for k, e, m in found if m is not None]
    if not found:
        return [], "no summary entry holds a pass rate with its interval (a different measurement)"
    for field in ("suite", "date"):
        if not isinstance(data.get(field), str) or not data[field]:
            raise ManifestError(f"{name}: has pass rates but no `{field}`")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", data["date"]):
        raise ManifestError(f"{name}: `date` {data['date']!r} is not YYYY-MM-DD")
    report = data.get("report") or name
    run = data.get("run") if isinstance(data.get("run"), dict) else {}
    commit = data.get("commit") or run.get("code_commit")
    # A report that marks its own arms "(this run)" ran only those; the rest are the comparison it drew against.
    marked = any("(this run)" in str(e.get("label", "")) for _, e, _ in found)
    sets = _task_sets(data, found)
    framing = _framing(data, found)
    out = []
    for variant, entry, metrics in found:
        own = _models(entry)
        task_set, n_tasks = sets[variant]
        trials = entry.get("trials") if _num(entry.get("trials")) else metrics["pass_rate"]["n"]
        k = entry.get("k") if isinstance(entry.get("k"), int) and not isinstance(entry.get("k"), bool) else None
        if k is None and task_set == "full" and n_tasks and trials and trials % n_tasks == 0:
            k = int(trials // n_tasks)
        routing = _routing(variant, entry, own)
        effort = entry.get("effort")
        row: dict[str, Any] = {
            "report": report, "benchmark": data["suite"], "tier": data.get("tier"),
            "arm": family(variant), "variant": variant,
            "harness": entry.get("label") or variant,
            "model": own if own is not None else data.get("model"), "date": data["date"], "commit": commit,
            "dataset": f"docs/benchmarks/{report}.json",
            "trials": entry.get("trials") if _num(entry.get("trials")) else metrics["pass_rate"]["n"],
            "tasks": entry.get("tasks") if _num(entry.get("tasks")) else None,
            "metrics": metrics,
            "model_note": data.get("model"),
            "provider": entry.get("provider") or provider_of(own),
            "routing": routing,
            "effort": ([effort] if isinstance(effort, str) else effort) if isinstance(effort, (str, list)) else None,
            "k": k, "task_set": task_set,
            "subset_of": (data.get("subset_of") if task_set != "full" else None),
            "framing": "routed" if routing == "routed" else framing,
        }
        if marked and "(this run)" not in str(entry.get("label", "")):
            row["comparison"] = True
        out.append(row)
    return out, None


def _print_of(row: dict[str, Any]) -> tuple:
    p = row["metrics"]["pass_rate"]
    return (row["benchmark"], row["tier"], row["arm"], p["k"], p["n"], p["lo"], p["hi"], row["tasks"])


def finalize(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Sorted, with `carried_from` set on a row that repeats an earlier row's figures. Pure of its input order."""
    rows = [{k: v for k, v in r.items() if k != "carried_from"} for r in rows]
    rows.sort(key=lambda r: (r["date"], r["report"], r["variant"]))
    first: dict[tuple, str] = {}
    for r in rows:
        key = _print_of(r)
        if key in first and first[key] != r["report"]:
            r["carried_from"] = first[key]
        else:
            first.setdefault(key, r["report"])
    return rows


def render(rows: list[dict[str, Any]]) -> str:
    doc = {"version": VERSION, "about": ABOUT, "runs": finalize(rows)}
    return json.dumps(doc, indent=1, ensure_ascii=False) + "\n"


def build(directory: Path, log=print) -> str:
    """The whole manifest over every `<report>.json` in `directory`; one line per skipped file through `log`."""
    rows: list[dict[str, Any]] = []
    for path in sorted(directory.glob("*.json")):
        if path.name == INDEX:
            continue
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError) as e:
            log(f"skipped {path.name}: unreadable JSON ({e})")
            continue
        try:
            got, why = rows_of(data, path.stem)
        except ManifestError as e:
            log(f"skipped {path.name}: {e}")
            continue
        if why:
            log(f"skipped {path.name}: {why}")
        rows += got
    return render(rows)


def append(dataset: Path, index: Path | None = None, log=print) -> int:
    """Add or replace `dataset`'s rows in the manifest beside it. Returns the rows written (0 when it has none to give).

    A dataset with no pass rate is not an error (a gate bench has none); one that has them and cannot make a whole
    row raises ManifestError, and the manifest is left as it was."""
    index = index or dataset.parent / INDEX
    data = json.loads(dataset.read_text())
    new, why = rows_of(data, dataset.stem)
    if not new:
        log(f"manifest: {dataset.name} gives no rows: {why}")
        return 0
    rows: list[dict[str, Any]] = []
    if index.exists():
        doc = json.loads(index.read_text())
        if doc.get("version") != VERSION or not isinstance(doc.get("runs"), list):
            raise ManifestError(f"{index}: not a version {VERSION} manifest; rebuild it with `manifest.py build`")
        rows = doc["runs"]
    name = new[0]["report"]
    rows = [r for r in rows if r["report"] != name] + new
    index.write_text(render(rows))
    return len(new)


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="rebuild index.json from every dataset in the directory")
    b.add_argument("--dir", default=str(DOCS))
    a = sub.add_parser("append", help="add or replace one report's rows")
    a.add_argument("dataset")
    args = ap.parse_args(argv)
    if args.cmd == "build":
        d = Path(args.dir)
        text = build(d, log=lambda s: print(s, file=sys.stderr))
        (d / INDEX).write_text(text)
        print(f"wrote {d / INDEX}: {len(json.loads(text)['runs'])} rows")
        return 0
    try:
        n = append(Path(args.dataset), log=lambda s: print(s, file=sys.stderr))
    except ManifestError as e:
        print(f"manifest: {e}", file=sys.stderr)
        return 1
    print(f"manifest: {n} rows for {Path(args.dataset).stem}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
