#!/usr/bin/env python3
"""The incidental-recall bench's scorer (theseus-7gir.17): every run's
probes, scored deterministically, and each arm's curve.

    python3 bench/recall/score.py /tmp/rc-th /tmp/rc-cc --out /tmp/rc-report

It reads each run directory `drive.py` wrote (they must share one
progression) and writes `scores.json`, `report.md` and `curve.svg` into
`--out`.

- **The checks** are the probes' own, in the check language (`checks.py`, as
  theseus-exam's `check.rs`): the value present and a superseded one absent;
  an abstention gives no value of the asked kind and admits it. An indirect
  probe is scored by its effect in the arm's workspace (the file its task
  writes), never by the arm's words.
- **Excluded, and counted**: a probe whose fact never reached the arm (its
  marker is not in the arm's transcript), and a probe whose turn failed (no
  reply, a nonzero exit). Neither is a miss.
- **The distance** of each probe is measured where the arm actually
  compacted (`run.json`'s `compactions`), never by the script's marks alone:
  a probe planned past a mark that its arm did not compact at is a
  `topic_shift` or `near` probe for that arm, and one planned short of a
  compaction the arm did make is a `compaction` probe. With it, the turns
  and the tokens since the fact (each turn's new tokens: input, cache
  writes and output).
- **The curve**: recall accuracy (direct and indirect probes) per arm by
  bucket and salience, nearest first; abstention accuracy beside it.
- **The half-life**: where accuracy falls to half its nearest bucket's,
  interpolated linearly between the two buckets around it, in turns and in
  tokens since the fact; `not reached` if it never does, `undefined` when
  the nearest bucket recalls nothing.
- **Confident-wrong**: a wrong answer that gives a value of the asked kind
  (an abstention's: any value) and does not hedge. **Stale**: a superseded
  value given; with `--stale retracted`, not where the reply names it only
  to take it back (counted as *old named*, and right). **Cites**: a right direct answer that says where (its script
  or file, or that the user said it) and when (its session's date or
  weekday, or a relative time).
- **Cost and latency** per probe: its turn's dollars and wall time.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import checks  # noqa: E402
import progression as pg  # noqa: E402

SAID = r"\b(?:you (?:said|mentioned|told me|noted|wrote)|you'd (?:said|mentioned)|(?:said|mentioned) (?:it )?(?:earlier|before)|in (?:a|an|the|your) (?:aside|side note|note))\b"
WHEN = (
    r"\b(?:earlier today|today|this morning|this afternoon|yesterday|last (?:session|week|time)|"
    r"(?:a few|\d+|several|two|three|nine) days ago|days ago|earlier|previous session|last conversation|"
    r"monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b"
)


@dataclass
class Scored:
    arm: str
    probe: str
    kind: str
    salience: str
    value_kind: str
    carrier: str  # how its fact was said (an abstention's: its distractor's)
    planned: str
    bucket: str
    status: str  # scored | undelivered | failed
    correct: bool | None
    confident_wrong: bool | None
    stale: bool | None
    hedged: bool | None
    cites_where: bool | None
    cites_when: bool | None
    turns_since: int
    tokens_since: int
    cost_usd: float | None
    latency_ms: int | None
    # Right under `--stale retracted` with the old value named, only where
    # the reply retracts it ("ignore the 27340"); None off a supersession.
    old_named: bool | None = None


@dataclass
class ArmRun:
    label: str
    dir: Path
    meta: dict
    turns: dict[int, dict]
    delivered: dict[str, dict]
    prog: pg.Progression


# The stale rules (`--stale`): `strict`, theseus-exam's (a superseded value
# named anywhere is stale, and wrong); `retracted`, a right answer that
# names the old value only in a sentence that retracts it.
STALE_RULES = ("strict", "retracted")
RETRACT = (
    r"\b(?:ignore|disregard|no longer|not any ?more|anymore|moved from|replaced|replaces|superseded|"
    r"instead of|used to be|previously|formerly|was\b[^.?!]{0,60}?\bbefore|earlier answer|i was wrong|"
    r"correction|scratch that|not \S+ as i said)\b"
)


def sentences(text: str) -> list[str]:
    """`text` cut into sentences: at a stop followed by space, and at line
    breaks (a version's dots have no space after them)."""
    return [x for x in re.split(r"(?<=[.!?])\s+|\n+", checks.fold(text)) if x.strip()]


def retracts_only(text: str, kind: str, old: str) -> bool:
    """Every sentence of `text` that names `old` retracts it (RETRACT), and
    at least one names it."""
    hit = "reply has " + " | ".join(pg.value_patterns(kind, old))
    named = [x for x in sentences(text) if checks.passes(hit, x)]
    return bool(named) and all(re.search(RETRACT, x, re.I) for x in named)


def load_run(d: Path) -> ArmRun:
    meta = json.loads((d / "run.json").read_text())
    prog = pg.load(d / "progression.json")
    turns = {}
    for line in (d / "turns.jsonl").read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            turns[r["index"]] = r
    delivered = json.loads((d / "delivered.json").read_text())
    label = meta["arm"] + (f"/{meta['memory_arm']}" if meta.get("memory_arm") else "")
    return ArmRun(label, d, meta, turns, delivered, prog)


def new_tokens(rec: dict | None) -> int:
    if not rec:
        return 0
    t = rec.get("tokens") or {}
    return int(t.get("input", 0)) + int(t.get("cache_write", 0)) + int(t.get("output", 0))


def _date_forms(date: str, weekday: str) -> list[str]:
    y, m, d = (int(x) for x in date.split("-"))
    mon = pg.MONTHS[m - 1]
    return [date, f"{mon} {d}", f"{mon[:3]} {d}", f"{d} {mon}", weekday.lower()]


def check_of(p: pg.Probe) -> str:
    """The check a probe is scored by: its own, but an abstention's is
    derived again from its kind, so a run kept from before a change to the
    admission rule (`progression.ADMIT`) is scored by today's."""
    return pg.kind_check(p.value_kind) if p.kind == "abstention" else p.check


def score_probe(run: ArmRun, p: pg.Probe, stale_rule: str = "strict") -> Scored:
    prog = run.prog
    facts = prog.facts_by_id()
    anchor = facts[p.anchor]
    rec = run.turns.get(p.turn)
    bucket = pg.bucket_of(prog, anchor.turn, p.turn, list(run.meta.get("compactions", [])),
                          p.bucket == "supersession")
    since = sum(new_tokens(run.turns.get(i)) for i in range(anchor.turn + 1, p.turn))
    base = dict(arm=run.label, probe=p.id, kind=p.kind, salience=p.salience, value_kind=p.value_kind,
                carrier=anchor.carrier, planned=p.bucket, bucket=bucket, turns_since=p.turn - anchor.turn, tokens_since=since,
                cost_usd=(rec or {}).get("cost_usd"), latency_ms=(rec or {}).get("latency_ms"))
    blank = dict(correct=None, confident_wrong=None, stale=None, hedged=None, cites_where=None, cites_when=None,
                 old_named=None)
    if p.fact is not None and not run.delivered.get(p.fact, {}).get("delivered"):
        return Scored(status="undelivered", **base, **blank)
    if rec is None or rec.get("exit") not in (0,) or not (rec.get("reply") or "").strip():
        return Scored(status="failed", **base, **blank)
    reply = rec.get("reply") or ""
    root = run.dir / "workspace"
    correct = checks.passes(check_of(p), reply, root)
    if p.kind == "indirect":
        try:
            text = (root / p.file).read_text(errors="replace")
        except OSError:
            text = ""
    else:
        text = reply
    old_named = None
    if p.stale is not None:
        old_named = False
        if stale_rule == "retracted" and not correct and retracts_only(text, p.value_kind, p.stale):
            # The check less its `lacks` line: the new value is there, and
            # the old one only where the reply takes it back.
            has = p.check.splitlines()[0]
            correct = old_named = checks.passes(has, reply, root)
    hedged = re.search(pg.HEDGE, checks.fold(text), re.I) is not None
    values = pg.find_values(p.value_kind, text)
    right = facts[p.fact].value if p.fact else None
    if p.value_kind == "date" and right is not None:
        wrong = [v for v in values if not checks.passes(pg.value_check("reply", "date", right, None), v)]
    else:
        wrong = [v for v in values if v != right]
    stale = None
    if p.stale is not None:
        stale = checks.passes(f"reply has " + " | ".join(pg.value_patterns(p.value_kind, p.stale)), text)
        stale = stale and not old_named
    if old_named:
        # The value it retracts is not one it gives.
        old = "reply has " + " | ".join(pg.value_patterns(p.value_kind, p.stale))
        wrong = [v for v in wrong if not checks.passes(old, v)]
    cw = (not correct) and bool(wrong) and not hedged
    where = when = None
    if p.kind == "direct" and correct:
        f = facts[p.fact]
        n = checks.normalize(reply)
        if f.source == "said":
            where = re.search(SAID, n) is not None
        else:
            src = Path(f.source).name.lower()
            where = src in n or src.removesuffix(".sh") in n
        s = prog.sessions[prog.turns[f.turn].session]
        when = any(checks.has_word(n, x) for x in _date_forms(s.date, s.weekday)) or re.search(WHEN, n) is not None
    return Scored(status="scored", correct=correct, confident_wrong=cw, stale=stale, hedged=hedged,
                  cites_where=where, cites_when=when, old_named=old_named, **base)


def score_run(run: ArmRun, stale_rule: str = "strict") -> list[Scored]:
    if stale_rule not in STALE_RULES:
        raise ValueError(f"the stale rule is one of {', '.join(STALE_RULES)}, not {stale_rule!r}")
    return [score_probe(run, p, stale_rule) for p in run.prog.probes]


# ---- the curve and its half-life


def _mean(xs: list[float]) -> float | None:
    return sum(xs) / len(xs) if xs else None


def curve(rows: list[Scored], salience: str, kinds=("direct", "indirect")) -> list[dict]:
    """Accuracy by bucket (nearest first) for one salience: n, accuracy, and
    the mean turns and tokens since the fact."""
    out = []
    for b in pg.CURVE_BUCKETS:
        xs = [r for r in rows if r.status == "scored" and r.salience == salience and r.kind in kinds and r.bucket == b]
        out.append({
            "bucket": b,
            "n": len(xs),
            "correct": sum(1 for r in xs if r.correct),
            "accuracy": (sum(1 for r in xs if r.correct) / len(xs)) if xs else None,
            "turns_since": _mean([r.turns_since for r in xs]),
            "tokens_since": _mean([r.tokens_since for r in xs]),
        })
    return out


def half_life(points: list[tuple[float, float]]) -> float | str:
    """Where accuracy falls to half its nearest point's: `points` are
    (distance, accuracy), sorted here by distance; linear between the two
    points around the crossing. `not reached` if it never falls that far,
    `undefined` if the nearest point recalls nothing."""
    pts = sorted(points)
    if not pts:
        return "undefined"
    x0, a0 = pts[0]
    if a0 <= 0:
        return "undefined"
    half = a0 / 2
    for (xa, aa), (xb, ab) in zip(pts, pts[1:]):
        if ab <= half:
            if aa == ab:
                return xb
            return xa + (aa - half) / (aa - ab) * (xb - xa)
    return "not reached"


def half_lives(c: list[dict]) -> dict:
    pts_t = [(b["turns_since"], b["accuracy"]) for b in c if b["n"]]
    pts_k = [(b["tokens_since"], b["accuracy"]) for b in c if b["n"]]
    return {"turns": half_life(pts_t), "tokens": half_life(pts_k)}


def summarize(rows: list[Scored]) -> dict:
    scored = [r for r in rows if r.status == "scored"]
    recall = [r for r in scored if r.kind != "abstention"]
    abst = [r for r in scored if r.kind == "abstention"]
    sup = [r for r in scored if r.stale is not None]
    direct_ok = [r for r in scored if r.kind == "direct" and r.correct]
    costs = [r.cost_usd for r in scored if r.cost_usd is not None]
    lats = [r.latency_ms for r in scored if r.latency_ms is not None]

    def rate(xs, f):
        return (sum(1 for r in xs if f(r)) / len(xs)) if xs else None

    return {
        "probes": len(rows),
        "scored": len(scored),
        "undelivered": sum(1 for r in rows if r.status == "undelivered"),
        "failed": sum(1 for r in rows if r.status == "failed"),
        "recall_accuracy": rate(recall, lambda r: r.correct),
        "abstention_accuracy": rate(abst, lambda r: r.correct),
        "by_kind": {k: rate([r for r in scored if r.kind == k], lambda r: r.correct) for k in pg.PROBE_KINDS},
        "by_carrier": {c: rate([r for r in recall if r.carrier == c], lambda r: r.correct) for c in pg.CARRIERS},
        "by_value_kind": {k: rate([r for r in recall if r.value_kind == k], lambda r: r.correct)
                          for k in sorted(pg.KIND_REGEX)},
        "confident_wrong": sum(1 for r in scored if r.confident_wrong),
        "confident_wrong_rate": rate(scored, lambda r: r.confident_wrong),
        "stale": sum(1 for r in sup if r.stale),
        "stale_of": len(sup),
        "old_named": sum(1 for r in sup if r.old_named),
        "cites_where": rate(direct_ok, lambda r: r.cites_where),
        "cites_when": rate(direct_ok, lambda r: r.cites_when),
        "cost_usd_per_probe": _mean(costs),
        "latency_ms_per_probe": _mean(lats),
        "moved": sum(1 for r in rows if r.bucket != r.planned),
        "curves": {s: curve(rows, s) for s in pg.SALIENCES},
        "abstention_curves": {s: curve(rows, s, ("abstention",)) for s in pg.SALIENCES},
        "half_life": {s: half_lives(curve(rows, s)) for s in pg.SALIENCES},
    }


# ---- the report


def _pct(x) -> str:
    return "—" if x is None else f"{100 * x:.0f}%"


def _num(x, fmt="{:.0f}") -> str:
    if x is None:
        return "—"
    if isinstance(x, str):
        return x
    return fmt.format(x)


def report(runs: list[ArmRun], sums: dict[str, dict], stale_rule: str = "strict") -> str:
    prog = runs[0].prog
    stale_words = {
        "strict": "Stale is strict: a superseded value named anywhere is stale, and the answer wrong.",
        "retracted": "Stale is `retracted`: a right answer that names the old value only where it retracts it "
                     "is right, counted under Old named, and not stale.",
    }[stale_rule]
    lines = [
        "# Incidental recall",
        "",
        f"Progression `{prog.size}`, seed {prog.seed}, digest `{prog.digest()[:16]}`: {len(prog.turns)} turns, "
        f"{len(prog.facts)} facts, {len(prog.probes)} probes. {stale_words}",
        "",
        "| Arm | Model | Compacted at | Delivered | Scored | Undelivered | Failed | Recall | Abstention | "
        "Confident-wrong | Stale | Cites where | Cites when | $/probe | ms/probe |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for r in runs:
        s = sums[r.label]
        lines.append(
            f"| {r.label} | {r.meta.get('model')} | {', '.join(map(str, r.meta.get('compactions', []))) or 'never'} | "
            f"{r.meta.get('delivered')}/{r.meta.get('facts')} | {s['scored']} | {s['undelivered']} | {s['failed']} | "
            f"{_pct(s['recall_accuracy'])} | {_pct(s['abstention_accuracy'])} | {s['confident_wrong']} | "
            f"{s['stale']}/{s['stale_of']} | {_pct(s['cites_where'])} | {_pct(s['cites_when'])} | "
            f"{_num(s['cost_usd_per_probe'], '{:.4f}')} | {_num(s['latency_ms_per_probe'])} |"
        )
    lines += ["", "## The curve", "",
              "Recall accuracy (direct and indirect probes) by distance bucket, nearest first, with the mean "
              "turns and tokens since the fact. Each arm's buckets are where it actually compacted.", ""]
    for s in pg.SALIENCES:
        lines += [f"### {s}", "", "| Arm | " + " | ".join(pg.CURVE_BUCKETS) + " | half-life (turns) | half-life (tokens) |",
                  "|---|" + "---|" * (len(pg.CURVE_BUCKETS) + 2)]
        for r in runs:
            c = sums[r.label]["curves"][s]
            hl = sums[r.label]["half_life"][s]
            cells = [
                f"{_pct(b['accuracy'])} ({b['correct']}/{b['n']}; {_num(b['turns_since'])} t, {_num(b['tokens_since'])} tok)"
                if b["n"] else "—"
                for b in c
            ]
            lines.append(f"| {r.label} | " + " | ".join(cells) + f" | {_num(hl['turns'], '{:.1f}')} | {_num(hl['tokens'], '{:.0f}')} |")
        lines.append("")
    lines += ["## Abstention", "", "Never stated: no value of the kind, and said so.", "",
              "| Arm | " + " | ".join(f"{s} {b}" for s in pg.SALIENCES for b in pg.CURVE_BUCKETS) + " |",
              "|---|" + "---|" * (2 * len(pg.CURVE_BUCKETS))]
    for r in runs:
        cells = []
        for s in pg.SALIENCES:
            for b in sums[r.label]["abstention_curves"][s]:
                cells.append(f"{_pct(b['accuracy'])} ({b['correct']}/{b['n']})" if b["n"] else "—")
        lines.append(f"| {r.label} | " + " | ".join(cells) + " |")
    lines += ["", "## By probe kind, and by how the fact was said", "",
              "Recall accuracy (direct and indirect) by the fact's carrier: a tool's output, an error message, an "
              "aside, a side remark, or the topic of the moment.", "",
              "| Arm | direct | indirect | abstention | " + " | ".join(pg.CARRIERS) + " |",
              "|---|---|---|---|" + "---|" * len(pg.CARRIERS)]
    for r in runs:
        s = sums[r.label]
        lines.append(f"| {r.label} | " + " | ".join(_pct(s["by_kind"][k]) for k in pg.PROBE_KINDS) + " | "
                     + " | ".join(_pct(s["by_carrier"][c]) for c in pg.CARRIERS) + " |")
    lines += ["", "## Supersession", "", "| Arm | Right | Stale given | Old named |", "|---|---|---|---|"]
    for r in runs:
        rows = [x for x in score_cache[r.label] if x.status == "scored" and x.planned == "supersession"]
        lines.append(f"| {r.label} | {sum(1 for x in rows if x.correct)}/{len(rows)} | "
                     f"{sum(1 for x in rows if x.stale)}/{len(rows)} | {sum(1 for x in rows if x.old_named)}/{len(rows)} |")
    lines += ["", "## How to read it", "",
              "- A probe planned in one bucket can land in another: its arm compacted elsewhere than the marks. "
              + "; ".join(f"{r.label}: {sums[r.label]['moved']} moved" for r in runs) + ".",
              "- Days are dated text: each session opens with its date, and neither arm's clock moves. A memory "
              "that weighs real elapsed time (retention) sees minutes, not days.",
              "- Excluded probes (undelivered, failed) are counted above and scored nowhere.",
              ""]
    return "\n".join(lines)


score_cache: dict[str, list[Scored]] = {}

COLORS = ("#2a6fdb", "#d9822b", "#2e9e5b", "#b84a8a", "#7a5cc7", "#8c8c3e")


def svg(runs: list[ArmRun], sums: dict[str, dict]) -> str:
    """The curve, by hand: accuracy by bucket, one line per arm and salience
    (incidental solid, central dashed)."""
    w, h, left, top, right, bottom = 720, 360, 60, 30, 180, 50
    pw, ph = w - left - right, h - top - bottom
    n = len(pg.CURVE_BUCKETS)

    def x(i: int) -> float:
        return left + pw * i / (n - 1)

    def y(a: float) -> float:
        return top + ph * (1 - a)

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" '
        'font-family="sans-serif" font-size="12">',
        f'<rect width="{w}" height="{h}" fill="#ffffff"/>',
        f'<text x="{left}" y="18" font-size="14">Incidental recall: accuracy by distance</text>',
    ]
    for k in range(0, 5):
        a = k / 4
        out.append(f'<line x1="{left}" y1="{y(a):.1f}" x2="{left + pw}" y2="{y(a):.1f}" stroke="#dddddd"/>')
        out.append(f'<text x="{left - 8}" y="{y(a) + 4:.1f}" text-anchor="end">{int(a * 100)}%</text>')
    for i, b in enumerate(pg.CURVE_BUCKETS):
        out.append(f'<text x="{x(i):.1f}" y="{top + ph + 20}" text-anchor="middle">{b}</text>')
    ly = top + 10
    for ri, r in enumerate(runs):
        color = COLORS[ri % len(COLORS)]
        for s in pg.SALIENCES:
            c = sums[r.label]["curves"][s]
            pts = [(x(i), y(b["accuracy"])) for i, b in enumerate(c) if b["n"]]
            dash = "" if s == "incidental" else ' stroke-dasharray="6 4"'
            if len(pts) > 1:
                d = " ".join(f"{px:.1f},{py:.1f}" for px, py in pts)
                out.append(f'<polyline points="{d}" fill="none" stroke="{color}" stroke-width="2"{dash}/>')
            for px, py in pts:
                out.append(f'<circle cx="{px:.1f}" cy="{py:.1f}" r="3" fill="{color}"/>')
            out.append(f'<line x1="{left + pw + 15}" y1="{ly}" x2="{left + pw + 40}" y2="{ly}" stroke="{color}" '
                       f'stroke-width="2"{dash}/>')
            out.append(f'<text x="{left + pw + 46}" y="{ly + 4}">{_esc(r.label)} {s}</text>')
            ly += 18
    out.append("</svg>")
    return "\n".join(out) + "\n"


def _esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("runs", nargs="+", type=Path)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--stale", choices=STALE_RULES, default="strict",
                    help="strict: a superseded value named anywhere is stale (the default); retracted: not where "
                         "the reply retracts it")
    a = ap.parse_args(argv)
    runs = [load_run(d) for d in a.runs]
    digests = {r.prog.digest() for r in runs}
    if len(digests) != 1:
        print("score: the runs replayed different progressions", file=sys.stderr)
        return 2
    labels = [r.label for r in runs]
    if len(set(labels)) != len(labels):
        # Two runs of one arm: name each by its directory.
        for r in runs:
            r.label = f"{r.label} ({r.dir.name})"
    sums = {}
    all_rows = []
    for r in runs:
        rows = score_run(r, a.stale)
        score_cache[r.label] = rows
        sums[r.label] = summarize(rows)
        all_rows += rows
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "scores.json").write_text(json.dumps(
        {"digest": runs[0].prog.digest(), "stale_rule": a.stale, "arms": sums,
         "probes": [asdict(x) for x in all_rows]},
        indent=1, sort_keys=True) + "\n")
    (a.out / "report.md").write_text(report(runs, sums, a.stale))
    (a.out / "curve.svg").write_text(svg(runs, sums))
    for r in runs:
        s = sums[r.label]
        print(f"{r.label}: recall {_pct(s['recall_accuracy'])}, abstention {_pct(s['abstention_accuracy'])}, "
              f"{s['scored']} scored, {s['undelivered']} undelivered, {s['failed']} failed, "
              f"{s['confident_wrong']} confident-wrong, half-life (incidental) "
              f"{_num(s['half_life']['incidental']['turns'], '{:.1f}')} turns")
    return 0


if __name__ == "__main__":
    sys.exit(main())
