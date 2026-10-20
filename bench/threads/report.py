#!/usr/bin/env python3
"""threadbench's report draft (theseus-9ofg): `th_score.py`'s scores in, a report out, through bench/report.

    python3 bench/threads/report.py --scores /tmp/tb-scores/scores.json --date 2026-10-20 --slug before \\
        [--persona /tmp/tb-persona/persona.json] [--out docs/benchmarks]

writes, beside each other, `<out>/<date>-threadbench-<slug>.md`, `.json` (every number the tables use and every
figure's spec), `.csv` (one row per probe per arm, no transcripts) and `<out>/img/<same>/*.svg` (each figure in a
light and a dark file, in the house palette). The sections, in order: the answer, per-stream correctness by arm,
contamination and leaks, context tokens per turn over the run, dollars measured and modelled, routing, latency,
the persona track, the prefix track, threats, reproduction, and the improvement assessment.

The arms are drawn in the house palette's existing slots (a harness's colour is its slot): A3 `theseus`, A4
`claude-code`, A5 `theseus-batching`'s, A6 `openclaw`'s, A7 `pi`, named in each figure's legend as the arm they are
(the palette is bench/report/charts.py's; a new arm would be a palette change, which this lane leaves alone). The
tables are each figure's readable twin. The numbers are the scorer's; every sentence below a table that interprets
them is the draft's reading and the maintainer's to rewrite. A real run's report is T0r's: nothing is committed
under docs/benchmarks/ by this lane.

Standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent
sys.path.insert(0, str(HERE))


def _load(name: str, path: Path):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


draft = _load("bench_report_draft", BENCH / "report" / "draft.py")
charts = draft.charts

import th_format as tf  # noqa: E402
import th_drive as td  # noqa: E402
import th_generate as gen  # noqa: E402

pct, usd = draft.pct, draft.usd


def fr(r: dict | None) -> str:
    """k of n (rate) of a scorer rate."""
    if not r or not r.get("n"):
        return "–"
    return f'{r["k"]}/{r["n"]} ({pct(r["rate"])})'


def mean_p90(t: dict) -> str:
    return f'{t["request_mean"]:,.0f} / {t["request_p90"]:,.0f}' if t.get("request_mean") is not None else "–"


def table(head: list[str], rows: list[list[str]]) -> str:
    return "\n".join(["| " + " | ".join(head) + " |", "|" + "---|" * len(head)] +
                     ["| " + " | ".join(r) + " |" for r in rows]) + "\n"


def legend(runs: list[dict]) -> dict[str, str]:
    return {r["key"]: f'{r["arm"]} {r["name"]}' for r in runs}


# ---------------------------------------------------------------- figures


# The threads arms are configurations inside Theseus: the chart palette's slots 6 to 8 (the memory family's, which
# never meet a threads arm in one figure), so no shared file changes. The records keep their own keys.
CHART_KEY = {"theseus-threads": "vector", "theseus-threads-noprefix": "fused", "theseus-threads-addressed": "entity"}


def figures(runs: list[dict], prog: tf.Progression) -> list[dict]:
    runs = [{**r, "key": CHART_KEY.get(r["key"], r["key"])} for r in runs]
    lab = legend(runs)
    figs = []
    # per-stream correctness: a matrix, P the oracle at 1
    cols = [{"label": r["arm"], "arm": r["key"]} for r in runs]
    rows = []
    for s in prog.streams:
        rows.append({"label": s.id + (" (sealed)" if s.sealed else ""),
                     "cells": "".join("P" if r["streams"][s.id]["oracle"] == 1.0 else "F" for r in runs),
                     "tips": [f'{r["arm"]}: {r["streams"][s.id]["ideal_passed"]} of {r["streams"][s.id]["ideal_checks"]} '
                              f'oracle checks; recall {fr(r["streams"][s.id]["recall"])}' for r in runs]})
    figs.append({"name": "stream-correctness", "form": "matrix", "title": "End-state oracle by stream and arm",
                 "question": "Which arm finished which stream's work correctly?", "columns": cols, "rows": rows,
                 "state_labels": {"P": "oracle holds", "F": "oracle fails"}, "legend_labels": lab,
                 "note": "P: every check the stream's end-state oracle runs holds. Hover a cell for its checks."})
    # contamination: intervals
    crow = []
    for r in runs:
        c = r["contamination"]
        if c["n"]:
            p, lo, hi = c["rate"], c["lo"], c["hi"]
            crow.append({"label": r["arm"], "arm": r["key"], "value": p, "lo": lo, "hi": hi,
                         "tip": f'{c["k"]} of {c["n"]} in-stream answers gave a colliding stream\'s value'})
    if crow:
        figs.append({"name": "contamination", "form": "intervals", "title": "Contamination by arm",
                     "question": "How often does an in-stream answer give another stream's value of the same kind?",
                     "x": {"label": "in-stream answers with a colliding value (95% Wilson)", "format": "pct", "min": 0},
                     "rows": crow, "legend_labels": lab})
    # context tokens over the run
    series = []
    for r in runs:
        curve = r["tokens"]["request_curve"]
        if curve:
            series.append({"arm": r["key"], "label": f'{r["arm"]}', "style": "line",
                           "points": [[i + 1, v] for i, v in enumerate(curve)]})
    if series:
        figs.append({"name": "context-tokens", "form": "lines", "title": "Compiled request tokens per model call",
                     "question": "How big is each request over the run, for one long context against one per stream?",
                     "x": {"label": "owner message", "type": "number", "format": "int"},
                     "y": {"label": "tokens in the request (input, cache read, cache write)", "format": "int", "min": 0},
                     "series": series, "legend_labels": lab, "height_plot": 260,
                     "note": "Provider usage fields divided by the turn's model calls, as the harness reports them (Theseus "
                             "`loops`, Claude Code `num_turns`, Pi's assistant messages)."})
    # dollars
    cats = ["measured", "modelled, 5 min cache", "modelled, 1 h cache"]
    figs.append({"name": "dollars", "form": "bars", "title": "Dollars for the whole run", "orientation": "h",
                 "question": "What does the run cost as measured (cache always warm) and at the owner's gaps?",
                 "categories": cats, "value": {"label": "dollars", "format": "usd", "min": 0},
                 "series": [{"arm": r["key"], "label": r["arm"],
                             "values": [r["usd"]["measured"], r["usd"]["modeled"]["5m"]["usd"],
                                        r["usd"]["modeled"]["1h"]["usd"]] } for r in runs],
                 "legend_labels": lab, "note": "Modelled: the same token breakdown, a cache miss wherever the owner's "
                                               "gap exceeds the TTL (th_price.py), the same for every arm."})
    # latency
    lat = [r for r in runs if r["latency"]["wall_ms_mean"] is not None]
    if lat:
        figs.append({"name": "latency", "form": "bars", "title": "Wall time per message", "orientation": "h",
                     "question": "How long does a message take, start to answer?",
                     "categories": ["mean", "p50", "p90"], "value": {"label": "milliseconds", "format": "ms", "min": 0},
                     "series": [{"arm": r["key"], "label": r["arm"],
                                 "values": [r["latency"]["wall_ms_mean"], r["latency"]["wall_ms_p50"],
                                            r["latency"]["wall_ms_p90"]]} for r in lat], "legend_labels": lab})
    return figs


# ---------------------------------------------------------------- text


ANSWER = ("**Draft: the maintainer rewrites this paragraph from the numbers.** {sentences}")


def answer_text(runs: list[dict], prog: tf.Progression) -> str:
    s = []
    n = len(prog.messages)
    asked = sum(1 for p in prog.probes if p.kind in tf.CHECK_KINDS)
    s.append(f"{len(runs)} arm(s) replayed the same {n} owner messages ({len(prog.streams)} interleaved streams, "
             f"{asked} questions) before threads exist.")
    one = [r for r in runs if r["layout"] == "one"]
    per = [r for r in runs if r["layout"] == "stream"]
    for r in runs:
        s.append(f'{r["arm"]} ({r["name"]}): oracle {r["oracle_mean"]:.0%} of streams, contamination '
                 f'{fr(r["contamination"])}, leaks {r["embargo"]["leak_count"]}, requests {mean_p90(r["tokens"])} '
                 f'tokens (mean / p90), ${r["usd"]["measured"]:.2f} measured and ${r["usd"]["modeled"]["5m"]["usd"]:.2f} '
                 f'modelled at the owner\'s gaps (5 min).')
    if one and per:
        a, b = one[0]["tokens"]["request_mean"], per[0]["tokens"]["request_mean"]
        if a and b:
            s.append(f"The number a threads arm has to beat: one session's requests average {a:,.0f} tokens, a "
                     f"session per stream's {b:,.0f} ({b / a:.0%} of it); the cross-stream questions are answered "
                     f"{fr(per[0]['cross']['value'])} by the split and {fr(one[0]['cross']['value'])} by the one session.")
    return " ".join(s)


def results(runs: list[dict], prog: tf.Progression, figs: dict[str, str]) -> dict[str, str]:
    arms = [r["arm"] for r in runs]
    out: dict[str, str] = {}
    # ---- per-stream correctness
    rows = []
    for s in prog.streams:
        rows.append([f"`{s.id}`" + (" (sealed)" if s.sealed else "")] + [
            f'{pct(r["streams"][s.id]["oracle"])} ({r["streams"][s.id]["ideal_passed"]} of '
            f'{r["streams"][s.id]["ideal_checks"]} checks); recall {fr(r["streams"][s.id]["recall"])}' for r in runs])
    rows.append(["**oracle mean**"] + [f'**{pct(r["oracle_mean"])}**' for r in runs])
    kinds = []
    for k in tf.CHECK_KINDS:
        kinds.append([k] + [f'{fr(r["kinds"][k])}' + (f' (+{r["kinds"][k]["excluded"]} excluded)' if r["kinds"][k]["excluded"] else "")
                            for r in runs])
    out["Per-stream correctness by arm"] = (
        figs["stream-correctness"] + "\n\n" + table(["stream"] + arms, rows) +
        "\nProbes by kind (right / asked; a probe whose fact never reached the arm is excluded, never a miss):\n\n" +
        table(["kind"] + arms, kinds) +
        "\nOracle checks per stream: " + "; ".join(
            f'`{s.id}`: ' + ", ".join(c["name"] for c in runs[0]["streams"][s.id]["checks"]) for s in prog.streams) + ".\n")
    # ---- contamination and leaks
    leaks = []
    for r in runs:
        for x in r["embargo"]["leaks"]:
            leaks.append(f'{r["arm"]}: {x}')
    cont = table(["metric"] + arms, [
        ["in-stream answers with a colliding value"] + [fr(r["contamination"]) for r in runs],
        ["stale value given (of superseded)"] + [fr(r["stale"]) for r in runs],
        ["cross-stream: right value"] + [fr(r["cross"]["value"]) for r in runs],
        ["cross-stream: right value and source named"] + [fr(r["cross"]["cited"]) for r in runs],
        ["cross-stream: colliding value in the answer"] + [fr(r["cross"]["contaminated"]) for r in runs],
        ["abstention: no value, and admits it"] + [fr({k: v for k, v in r["abstention"].items() if k in ("k", "n", "rate")}) for r in runs],
        ["embargo: probes clean"] + [fr(r["embargo"]["probes"]) for r in runs],
        ["embargo: sealed markers in other streams' files"] + [str(r["embargo"]["leak_count"]) for r in runs],
        ["misroute recovery (note in the right file only)"] + [fr(r["misroute"]) for r in runs],
    ])
    out["Contamination and leaks"] = (figs.get("contamination", "") + "\n\n" + cont +
                                       ("\nLeaks found: " + "; ".join(leaks) + ".\n" if leaks else "\nNo leaks found.\n"))
    # ---- tokens
    tok = table(["metric"] + arms, [
        ["request tokens per model call, mean / p90"] + [mean_p90(r["tokens"]) for r in runs],
        ["last request"] + [f'{r["tokens"]["request_curve"][-1]:,}' if r["tokens"]["request_curve"] else "–" for r in runs],
        ["prompt tokens in all (input + cache read + cache write)"] + [f'{r["tokens"]["prompt_total"]:,}' for r in runs],
        ["cache-read share of the prompt"] + [pct(r["tokens"]["cache_read_share"]) for r in runs],
        ["sessions / compactions"] + [f'{r["sessions"]} / {r["compactions"]}' for r in runs],
    ])
    out["Context tokens per turn over the run"] = figs.get("context-tokens", "") + "\n\n" + tok
    # ---- dollars
    dol = table(["dollars"] + arms, [
        ["measured (the harness's count; cache warm throughout)"] + [usd(r["usd"]["measured"]) for r in runs],
        ["per message, measured"] + [usd(r["usd"]["per_turn"]) for r in runs],
        ["modelled, 5-minute cache"] + [f'{usd(r["usd"]["modeled"]["5m"]["usd"])} ({r["usd"]["modeled"]["5m"]["misses"]} misses)' for r in runs],
        ["modelled, 1-hour cache"] + [f'{usd(r["usd"]["modeled"]["1h"]["usd"])} ({r["usd"]["modeled"]["1h"]["misses"]} misses)' for r in runs],
    ])
    out["Dollars measured and modelled"] = (
        figs["dollars"] + "\n\n" + dol +
        "\nThe model (`th_price.py`, the same for every arm): a session's prompt cache lives one TTL after its last "
        "request; a message whose gap since its own session's previous message is over the TTL re-writes its prefix "
        "(its first request's cache reads become writes, at the 5-minute write price, or at twice the input price for "
        "the 1-hour TTL); the gaps are the owner's, drawn once for the progression. An arm with a session per stream "
        "sees longer gaps than one session does. Behaviour on a cold cache is not modelled.\n")
    # ---- routing
    rt = []
    for r in runs:
        x = r["routing"]
        if x.get("owner_routed"):
            rt.append([r["arm"], "the owner's own split: every message goes where the owner puts it", "–", "–"])
        elif "top1" in x:
            rt.append([r["arm"], fr(x["top1"]), fr(x["asked_on_ambiguous"]), fr(x["asked_on_clear"])])
        else:
            rt.append([r["arm"], "no router: nothing to score", "–", "–"])
    out["Routing"] = (table(["arm", "top-1 (true stream)", "asked on ambiguous", "asked on clear"], rt) +
                      f"\nThe progression labels every message's true stream and marks "
                      f"{sum(1 for m in prog.messages if m.ambiguous)} as ambiguous (a good router asks) and "
                      f"{sum(1 for m in prog.messages if m.role == 'misroute')} misrouted notes with their scripted "
                      "correction; only the threads arms (A1 and after) have a router to score.\n")
    # ---- latency
    lat = table(["latency"] + arms, [
        ["wall per message, mean"] + [f'{r["latency"]["wall_ms_mean"]:,.0f} ms' if r["latency"]["wall_ms_mean"] else "–" for r in runs],
        ["p50 / p90"] + [f'{r["latency"]["wall_ms_p50"]:,.0f} / {r["latency"]["wall_ms_p90"]:,.0f} ms' if r["latency"]["wall_ms_p50"] else "–" for r in runs],
        ["first token, mean"] + [f'{r["latency"]["first_token_ms_mean"]:,.0f} ms' if r["latency"]["first_token_ms_mean"] else "not recorded" for r in runs],
        ["harness CPU (user + sys) / peak RSS"] + [
            (f'{r["resources"]["cpu_user_s"] + r["resources"]["cpu_sys_s"]:.1f} s / {r["resources"]["max_rss_kb"] / 1024:.0f} MiB'
             if r.get("resources") else "–") for r in runs],
    ])
    out["Latency"] = figs.get("latency", "") + "\n\n" + lat + (
        "\nFirst-token latency is carried by Theseus's turn result only; Claude Code and Pi report none, so their cells say "
        "so (a T0r that needs it reads each harness's own trace). CPU and RSS are the driven processes' "
        "(`getrusage` of the children: the harness's CLI, and a Theseus daemon), apart from the model's work.\n")
    return out


LOSE = """## Where threads would lose

The streams are chores a developer does, and the schedule is drawn from measured shapes (stay in a stream with
p = 0.64, gaps with a median of 6.2 minutes), not from what favours threads. Threads would lose, or only break even,
where:

- **The owner comes back after the cache is cold.** A thread's context is small, but the first request after a gap
  over the TTL writes it again; the per-stream arms above show the misses the owner's gaps cost
  (the modelled dollars). A long single context re-writes more per miss and misses less often per message.
- **A cross-stream question needs another stream's fact.** The split arms answer those from nothing (see the
  cross-stream rows); a thread design must still answer them, from a shared memory, and that has its own cost and
  its own contamination risk.
- **A message is ambiguous.** About a tenth of the messages name no stream. A router that guesses misroutes them
  (the misroute probes price the recovery); one that asks spends a turn of the owner's.
- **The run is short.** A thread's setup (a header, a persona card of ~60 tokens, a router call) is paid per
  thread; with few messages per stream it is a larger part of the bill than a single long session's growth.
- **Nothing is sealed.** Threads buy an embargo only if the sealed stream's context is kept out of the others;
  where the owner has no embargo, the leak columns are zero for every arm and threads buy nothing there.
"""


def persona_section(p: dict | None) -> str:
    if not p:
        return ("Not run in this report: `python3 bench/threads/th_persona.py --seed <seed> --out <dir>` writes "
                "`persona.json`, and `report.py --persona` reads it.\n")
    g = p["groups"]
    rows = []
    for name, key in (("all messages", "all"), ("switch messages (the persona differs from the stream's lean)", "switch"),
                      ("steady messages", "steady"), ("addressed by name", "named"), ("corrections", "correction")):
        d = g[key]
        rows.append([name, str(d["answer"]["n"]), pct(d["answer"]["rate"]), pct(d["prior"]["rate"]),
                     pct(d["keywords"]["rate"]), pct(d["oracle"]["rate"])])
    return (f'Answerer: **{p["answerer"]}**. {"On the stand-in this proves the plumbing and the baselines, not Jev: " if "stand-in" in p["answerer"] else ""}'
            f'the draft pack is `{p["pack"]}`.\n\n' +
            table(["messages", "n", "answerer", "thread prior", "cards keywords", "oracle"], rows))


def prefix_section(pr: list[dict]) -> str:
    if not pr:
        return ("Not run in this report: `python3 bench/threads/th_prefix.py --seed <seed> --out <dir>` writes the track's "
                "progression; `th_drive.py --progression <dir>` drives it and `th_score.py` scores it. Header probes "
                "whose sibling-valued answer is a false fit are counted apart.\n")
    rows = [[r["arm"], str(r["header"]["n"]), str(r["header"]["correct"]), str(r["header"]["false_fit"]),
             str(r["header"]["other_wrong"]), f'{r["sessions"]}'] for r in pr]
    return table(["arm", "header probes", "right", "false fit (the sibling's value)", "wrong otherwise", "sessions"], rows)


def threats(runs: list[dict]) -> str:
    return """- **One run per arm, one seed.** The development seed is for building; the held-out seed is run once. Rates
  have Wilson intervals in the JSON; the dollars and token curves have none, and nothing here is a significance test.
- **The owner's gaps are modelled, not slept.** Each run replays back to back, so measured dollars assume a cache that
  never expires; the modelled ones assume the stated TTL rule for every arm and ignore behaviour on a cold cache.
- **Compaction is each arm's own.** The window is Sonnet 5.5's 1,000,000: a run of this size rarely compacts, and an
  arm that does is noted in the table. The comparison holds the model, the effort (`medium`) and the messages, not the
  harness's prompt, tools or memory.
- **Probes remind.** A fact is probed once, but an answer in context is a fact said again; later probes of other facts
  are not affected, and the progression's validator holds the rule.
- **The oracle checks end states, not the road.** An arm that does the work right with extra files is scored right;
  a leak is any sealed marker in another stream's files, a deliberately blunt test.
- **Synthetic names and chores.** Six streams of a developer's invented repositories; the shapes (gaps, run lengths,
  cues) are the owner's description, not a measurement of a population.
- **Token fields are each harness's own account of a turn**: summed over its model calls. The per-call split divides by
  the calls the harness reports (Theseus `loops`, Claude Code `num_turns`, Pi's assistant messages), tool calls + 1
  where a run did not keep them (an upper bound); a harness that compacts or trims inside a call is not seen.
- **Harness faults** at the time of the run are named in the run directory's `run.json`, not here.
"""


def improvement(runs: list[dict], prog: tf.Progression, seed: int, size: str) -> str:
    return f"""{LOSE}
### What T0r (the before run) needs

- A release build of Theseus (`scripts/build.sh --profile release-thin`, or `cargo build --release`), Claude Code at
  its pinned version (`{td.pinned('claude-code')}`, bench/harbor), Pi (`{td.pinned('pi')}`), Sonnet 5.5 at effort `medium`
  for every arm, the attempt plan committed before the run, and every oracle at 1 on the stand-in first
  (`python3 -m unittest discover -s bench/threads`).
- About $45 to $75 for the standard size (6 streams, ~150 owner turns, ~60 probes), across the five measurable arms;
  the pause line is $170 with the after run. The drivers' spend limit is a per-arm `--spend-limit` (default $75).
- Run in a throwaway container or VM; one arm after another, never at once (the host's load is part of latency).

```bash
# the progression, once, on the development seed (the held-out seed {gen.HELD_OUT_SEED} is run once, after)
python3 bench/threads/th_generate.py --seed {seed} --size {size} --out /tmp/tb-prog
# the arms (each its own run directory; A5 and A6 read the same progression)
python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t0r.json --arm A3 --bin-dir target/release \\
    --progression /tmp/tb-prog --out /tmp/tb-a3
python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t0r.json --arm A5 --bin-dir target/release \\
    --progression /tmp/tb-prog --out /tmp/tb-a5
python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t0r.json --arm A4 --progression /tmp/tb-prog --out /tmp/tb-a4
python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t0r.json --arm A6 --progression /tmp/tb-prog --out /tmp/tb-a6
python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t0r.json --arm A7 --theseus-overhead "$(python3 -c "import json;print(json.load(open('/tmp/tb-a3/run.json'))['overhead']['planned'])")" \\
    --progression /tmp/tb-prog --out /tmp/tb-a7
python3 bench/threads/th_drive.py --probe-opencode     # A8: measurable, or "not measurable" and why
python3 bench/threads/th_persona.py --seed {seed} --out /tmp/tb-persona          # on Jev: add --jev-probe <bin>
python3 bench/threads/th_prefix.py --seed {seed} --out /tmp/tb-prefix           # the prefix track's progression
for arm in A3 A5; do python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t0r.json --arm $arm \\
    --bin-dir target/release --progression /tmp/tb-prefix --out /tmp/tb-prefix-$arm; done
python3 bench/threads/th_score.py /tmp/tb-prefix-A3 /tmp/tb-prefix-A5 --out /tmp/tb-prefix-scores
python3 bench/threads/th_score.py /tmp/tb-a3 /tmp/tb-a4 /tmp/tb-a5 /tmp/tb-a6 /tmp/tb-a7 --out /tmp/tb-scores
python3 bench/threads/report.py --scores /tmp/tb-scores/scores.json --persona /tmp/tb-persona/persona.json \\
    --prefix-scores /tmp/tb-prefix-scores/scores.json --date <the day of the run> --slug before --out docs/benchmarks
```

The threads arms (A1, A1n, A2) are in "What T12 needs" below; A1p is not measurable (`th_drive.py --arm A1p` says why).

### What T12 (the after run) needs

- The same release binaries as the before run's A3 (frozen: copy them once, run every arm from the copy), Sonnet 5.5
  at effort `medium`, the attempt plan `bench/plans/threadbench-t12.json` committed before the run, and Jev reachable
  for the router's thread question: the bench profile has no `[judge]` table, so without one the pre-router alone places
  messages (the question is asked and a timeout keeps its guess). Give the daemon Jev's endpoint with
  `--config-overlay <toml>` (laid over the scratch config; its sha256 is kept in `run.json`).
- About $35 to $60 for the standard size across A1, A1n, A2 and A3 (A3 is the before run's, not repeated if its
  binaries are the same); the pause line is $170 with the before run, and the per-arm spend limit is $25.
- One arm after another, in a throwaway container or VM. A1p has no switch to be built on (the daemon builds one
  persona-set layer whenever two or more `[personas]` exist): it is named in the plan as probe-only and the report says
  so, as it does for A8.

```bash
python3 bench/threads/th_generate.py --seed {seed} --size {size} --out /tmp/tb-prog      # the same digest as T0r's
for arm in A1 A1n A2; do python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t12.json --arm $arm \
    --bin-dir <frozen bin dir> --config-overlay <jev.toml> --progression /tmp/tb-prog --out /tmp/tb-$arm-t12; done
python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t12.json --arm A3 --bin-dir <frozen bin dir> \
    --progression /tmp/tb-prog --out /tmp/tb-a3-t12
python3 bench/threads/th_drive.py --arm A1p --out /tmp/never      # not measurable, and why
python3 bench/threads/th_prefix.py --seed {seed} --out /tmp/tb-prefix
for arm in A1 A1n A2; do python3 bench/threads/th_drive.py --plan bench/plans/threadbench-t12.json --arm $arm \
    --bin-dir <frozen bin dir> --config-overlay <jev.toml> --progression /tmp/tb-prefix --out /tmp/tb-prefix-$arm; done
python3 bench/threads/th_score.py /tmp/tb-A1-t12 /tmp/tb-A1n-t12 /tmp/tb-A2-t12 /tmp/tb-a3-t12 --out /tmp/tb-t12-scores
python3 bench/threads/report.py --scores /tmp/tb-t12-scores/scores.json --date <the day of the run> --slug after \
    --out docs/benchmarks
```

Each threads arm's run directory also holds `routing.jsonl` (per message: the thread it ran in, whether the router
asked, the persona engaged and the prefix library's pick) and `run.json`'s `owner` (over-split offers, threads the
owner started by hand, every answer given) and `reopen_not_driven`.

### Improvement assessment

The before run sets the number a threads build must beat on the same messages: the per-request context of one long
session against a session per stream (what a person splitting by hand gets), the contamination of the first and the
cross-stream recall the second loses, the embargo's leaks, and the dollars at the owner's gaps with both TTLs. A
threads arm is an improvement when it keeps the split's context size while answering the cross-stream questions the
split cannot, routes the clear messages right and asks on the ambiguous ones, and leaks nothing from the sealed
stream; it is not one if its dollars at the owner's gaps exceed the single session's (the cache-miss cost above), or
if its routing errors cost more recovery than the contamination they avoid. This draft states no verdict: the
numbers above are the before.
"""


def reproduction(slug: str, seed: int, size: str) -> str:
    return f"""```bash
python3 bench/threads/th_generate.py --seed {seed} --size {size} --out <prog>
python3 bench/threads/th_drive.py --arm <A1|A1n|A2|A3|A4|A5|A6|A7> ... --progression <prog> --out <run>
python3 bench/threads/th_score.py <runs...> --out <scores>
python3 bench/threads/report.py --scores <scores>/scores.json --date <day> --slug {slug}
python3 -m unittest discover -s bench/threads    # every oracle at 1 on the stand-in, first
```
"""


def not_measured(opencode: dict | None) -> str:
    """The arms this report could not run, each with its reason."""
    a8 = opencode or td.probe_opencode()
    why8 = ("measurable: " if a8["measurable"] else "not measurable: ") + a8["why"]
    return (f"A1p: not measurable: {td.ARMS['A1p']['unmeasurable']}; A8 (OpenCode): {why8}")


def write_report(runs: list[dict], prog: tf.Progression, out: Path, slug: str, date: str, scratch: bool = False,
                 persona: dict | None = None, prefix_runs: list[dict] | None = None,
                 opencode: dict | None = None) -> str:
    name = draft.report_name(date, "threadbench", slug)
    figs = figures(runs, prog)
    figmd = {f["name"]: charts.picture(f"img/{name}", f) + f"\n\n*{f['question']}*\n" for f in figs}
    sections = results(runs, prog, figmd)
    title = f"Threadbench: the cost of not splitting ({date})" + (" - scratch, not a run" if scratch else "")
    glance = [("Suite", "threadbench (`bench/threads`): six interleaved streams with colliding facts"),
              ("Arms", "; ".join(f'{r["arm"]} {r["name"]}' for r in runs)),
              ("Not measured", not_measured(opencode)),
              ("Progression", f"seed {prog.seed}, {prog.size}, digest `{prog.digest()[:12]}`"),
              ("Model", f'{runs[0]["model"]} at effort {runs[0]["effort"]}'),
              ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    lines = [f"# {title}", "", ANSWER.format(sentences=answer_text(runs, prog)), "", "| | |", "|---|---|"]
    lines += [f"| {k} | {v} |" for k, v in glance]
    for head in ("Per-stream correctness by arm", "Contamination and leaks", "Context tokens per turn over the run",
                 "Dollars measured and modelled", "Routing", "Latency"):
        lines += ["", f"## {head}", "", sections[head].rstrip()]
    lines += ["", "## The persona track", "", persona_section(persona).rstrip(),
              "", "## The prefix-library track", "", prefix_section(prefix_runs or []).rstrip(),
              "", "## Threats to validity", "", threats(runs).rstrip(),
              "", "## Reproduction", "", reproduction(slug, prog.seed, prog.size).rstrip(),
              "", "## Improvement assessment", "", improvement(runs, prog, prog.seed, prog.size).rstrip(), ""]
    md = "\n".join(lines)
    data = {"report": name, "suite": "threadbench", "date": date, "seed": prog.seed, "size": prog.size,
            "digest": prog.digest(), "arms": [{"key": r["key"], "label": f'{r["arm"]} {r["name"]}'} for r in runs],
            "runs": [{k: v for k, v in r.items() if k != "probes"} for r in runs], "persona": persona,
            "prefix_runs": [{k: v for k, v in r.items() if k != "probes"} for r in prefix_runs or []],
            "figures": figs}
    rows = [{"arm": r["arm"], "id": p["id"], "kind": p["kind"], "stream": p["stream"], "msg": p["msg"],
             "status": "excluded" if "excluded" in p else "scored", "correct": p.get("ok"),
             "contaminated": len(p.get("contaminated") or []) or None, "cited": p.get("cited")}
            for r in runs for p in r["probes"]]
    out = Path(out)
    draft.write_outputs(out, name, data, rows, md, force=True)
    return md


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--scores", type=Path, required=True)
    ap.add_argument("--progression", type=Path, default=None, help="default: regenerated from the scores' seed and size")
    ap.add_argument("--opencode", default="opencode", help="the opencode binary, probed for A8")
    ap.add_argument("--prefix-scores", type=Path, default=None, help="th_score.py's output over the prefix track's runs")
    ap.add_argument("--persona", type=Path, default=None)
    ap.add_argument("--date", required=True)
    ap.add_argument("--slug", default="before")
    ap.add_argument("--out", type=Path, default=BENCH.parent / "docs" / "benchmarks")
    a = ap.parse_args(argv)
    runs = json.loads(a.scores.read_text())["runs"]
    if a.progression:
        prog = tf.load(a.progression)
    else:
        prog = gen.build(runs[0]["seed"], runs[0]["size"])
    if any(r["digest"] != prog.digest() for r in runs):
        raise SystemExit("report: the scores are of another progression than the one given (digests differ)")
    prefix_runs = json.loads(a.prefix_scores.read_text())["runs"] if a.prefix_scores else None
    persona = json.loads(a.persona.read_text()) if a.persona else None
    write_report(runs, prog, a.out, a.slug, a.date, persona=persona, prefix_runs=prefix_runs,
                 opencode=td.probe_opencode(a.opencode))
    print(f"report: wrote {a.out}/{draft.report_name(a.date, 'threadbench', a.slug)}.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
