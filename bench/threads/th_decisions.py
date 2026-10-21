"""The decisions column, sharper (theseus-2e6c1): which answers of Jev's were decisions, counted per question.

`th_threads.judgment_row` keeps, per judgment, the few fields this reads (never the state or any text); `th_score.decisions`
delegates here; `report.decisions_section` words the result. The earlier column counted a judgment: one `onto.v1`
judgment that filed a topic, a resource and a privacy answer was one filing, and privacy counted only `security.*`
packs. Here a **decision** is a decision question answered at the act band, by class:

    filing    `onto.*`'s kind questions (topic, project, event, resource, capability, expertise); `categorize.*`
    routing   `route.*`; `classify.*`'s `thread` question
    privacy   `security.*`; `onto.*`'s `personal`, `elicits` and `shows_*`

Each decision says whether its answer chose something (a Choice other than `none` or `other`, a Noul yes), so a
report can tell a filing from a "nothing to file". The per-pack count stays beside it as `judgments`.

A decision belongs to the turn its judgment's `context.turn` names when `turns.jsonl` carries that id; else to the turn
of the judgment's session whose window (`ended_ms` less `latency_ms`, to `ended_ms`) holds the judgment's time; else
"between turns". A run whose judgment rows lack the new fields (an old run) still scores, by judgment, and says
the per-question numbers are not recorded. Standard library only.
"""

from __future__ import annotations

FILING_KINDS = ("topic", "project", "event", "resource", "capability", "expertise")
CLASSES = ("routing", "filing", "privacy")
NOT_RECORDED = "not recorded"


def pack_class(pack: str | None) -> str | None:
    """The class of a pack's judgments as a whole: what an old row (no answers kept) is counted as."""
    return next((c for c, ps in (("routing", ("route.", "classify.")), ("filing", ("onto.", "categorize.")),
                                  ("privacy", ("security.",))) if pack and pack.startswith(ps)), None)


def question_class(pack: str | None, question: str | None) -> str | None:
    """The class of a decision question, or None for a question that decides nothing (a naming, a `still_*`)."""
    pack, q = pack or "", question or ""
    if pack.startswith("onto."):
        if q in FILING_KINDS:
            return "filing"
        return "privacy" if q in ("personal", "elicits") or q.startswith("shows_") else None
    if pack.startswith("categorize."):
        return "filing"
    if pack.startswith("route."):
        return "routing"
    if pack.startswith("classify."):
        return "routing" if q == "thread" else None
    return "privacy" if pack.startswith("security.") else None


def answer_row(a: object) -> dict | None:
    """One ledger answer kept as its question, band and whether it chose something; None for anything else."""
    if not isinstance(a, dict) or not a.get("def"):
        return None
    ans, band = a.get("answer") or {}, a.get("band") or {}
    kind = ans.get("type")
    if kind == "choice":
        chose = ans.get("choice") not in (None, "none", "other")
    elif kind == "noul":
        chose = (ans.get("noul") or 0) >= 0.5
    else:
        chose = False
    return {"q": a["def"], "band": band.get("band"), "chose": bool(chose)}


def attribute(row: dict, turns: list[dict]) -> tuple[int | None, str]:
    """The index into `turns` of the turn a judgment belongs to, and how that was found: `turn_id`, `time` or
    `between` (None)."""
    if row.get("turn"):
        for i, t in enumerate(turns):
            if t.get("turn_id") == row["turn"]:
                return i, "turn_id"
    at = row.get("at_ms")
    if at is not None:
        for i, t in enumerate(turns):
            end, took = t.get("ended_ms"), t.get("latency_ms")
            if end is None or took is None:
                continue
            same = not row.get("session") or t.get("session_id") in (None, row["session"])
            if same and end - took <= at <= end:
                return i, "time"
    return None, "between"


def decisions(rows: list[dict], turns: list[dict] | int, price) -> dict | None:
    """The run's decisions taken off the main model. `rows` are `judgment_row`s, `turns` the run's `turns.jsonl`
    records (or just their count), `price` the main model's `(input, output)` dollars per million tokens. None
    when no answered row is of a decision pack (an arm that asks Jev nothing)."""
    done = [r for r in rows if r.get("outcome") == "answered" and pack_class(r.get("pack"))]
    if not done:
        return None
    records = turns if isinstance(turns, list) else []
    n_turns = max(1, len(records) if isinstance(turns, list) else turns)
    recorded = any("answers" in r for r in done)
    r_in, r_out = price
    by: dict[str, dict] = {}

    def bucket(cls: str) -> dict:
        return by.setdefault(cls, {"n": 0, "chose": 0, "judgments": 0, "jev_usd": 0.0, "main_model_usd": 0.0})

    for r in done:
        c = bucket(pack_class(r["pack"]))
        c["judgments"] += 1
        c["jev_usd"] += r.get("cost_micros", 0) / 1e6
        c["main_model_usd"] += (r.get("input_tokens", 0) * r_in + r.get("output_tokens", 0) * r_out) / 1e6
    questions: dict[str, dict] = {}
    per_turn = [0] * len(records)
    how = {"turn_id": 0, "time": 0, "between": 0}
    for r in done:
        found = [(a, question_class(r["pack"], a["q"])) for a in r.get("answers", [])]
        found = [(a, c) for a, c in found if c]
        for a, c in found:
            q = questions.setdefault(f'{r["pack"]}/{a["q"]}', {"class": c, "answered": 0, "act": 0, "chose": 0})
            q["answered"] += 1
            if a["band"] == "act":
                q["act"] += 1
                q["chose"] += int(a["chose"])
                bucket(c)["n"] += 1
                bucket(c)["chose"] += int(a["chose"])
                i, source = attribute(r, records)
                how[source] += 1
                if i is not None:
                    per_turn[i] += 1
        if not recorded:
            by[pack_class(r["pack"])]["n"] += 1  # an old row: the judgment is the count, as it was
    n = sum(c["n"] for c in by.values())
    jev = sum(c["jev_usd"] for c in by.values())
    main = sum(c["main_model_usd"] for c in by.values())
    for c in by.values():
        c["per_turn"] = round(c["n"] / n_turns, 3)
        c["jev_usd"], c["main_model_usd"] = round(c["jev_usd"], 6), round(c["main_model_usd"], 6)
    timed = recorded and bool(records) and sum(how.values()) > 0
    out = {"by_class": by, "n": n, "per_turn": round(n / n_turns, 3), "judgments": len(done),
           "jev_usd": round(jev, 6), "main_model_usd": round(main, 6), "saved_usd": round(main - jev, 6),
           "unanswered": sum(1 for r in rows if r.get("outcome") != "answered" and pack_class(r.get("pack"))),
           "recorded": recorded, "questions": questions if recorded else NOT_RECORDED,
           "turns": {"mean": round(n / n_turns, 3),
                     "share_with_decision": round(sum(1 for x in per_turn if x) / len(per_turn), 3) if timed and per_turn
                     and how["turn_id"] + how["time"] else None,
                     "attributed": how if timed else NOT_RECORDED}}
    return out
