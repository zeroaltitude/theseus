"""threadbench's stand-in model script (theseus-9ofg): what a PERFECT arm does for each message, as rules for the
recall bench's stand-in model (`bench/recall/standin.py`: scripted answers, with the request's token count a
provider would report), and for a stand-in `claude`. A message with a reference effect writes its files (`fs_write`);
a question is answered with the right value (a cross-stream one with its source named); an abstention admits it
does not know; every other message gets a plain reply that says no value.

The stand-in is for the offline smoke and the tests: every oracle must score 1 on it, and the scorer must find no
contamination, no leak, and every probe right.

Standard library only.
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import th_beats as tb  # noqa: E402

REPLY = ("That's done: the output above says what it found, and it needs nothing more from me right now. " * 5)[:400]
UNKNOWN = "I don't know: that was never mentioned."


def answer(prog: tf.Progression, p: tf.Probe) -> dict:
    facts = prog.facts_by_id()
    if p.kind == "abstention":
        return {"reply": UNKNOWN}
    f = facts[p.fact]
    if p.kind == "in_indirect":
        return {"file": p.file, "content": f.value + "\n", "reply": "Written."}
    if p.kind == "cross":
        return {"reply": f"It's {f.value}. That came from {tb.DESC[p.stream]}."}
    return {"reply": f"It's {f.value}."}


def plan(prog: tf.Progression) -> dict[int, dict]:
    """Per message index: {"writes": [{path, content}], "reply": str} (a perfect arm's whole turn)."""
    by_msg = {}
    for p in prog.probes:
        if p.kind in ("in_direct", "in_indirect", "cross", "abstention"):
            by_msg[p.msg] = answer(prog, p)
    out = {}
    for m in prog.messages:
        a = by_msg.get(m.index, {})
        writes = [{"path": e.path, "content": e.content} for e in m.ref if e.content is not None]
        if a.get("file"):
            writes.append({"path": a["file"], "content": a["content"]})
        out[m.index] = {"writes": writes, "reply": a.get("reply") or REPLY}
    return out


def rules_for(prog: tf.Progression) -> list[dict]:
    """The recall stand-in's rules: each message's writes as `fs_write` calls or its text; the latest message's
    rule first, so a recall note that quotes an earlier message never takes its rule. A message with nothing to
    do and the generic reply needs no rule (an unmatched message is answered with the generic reply)."""
    rules = []
    for m in prog.messages:
        a = plan(prog)[m.index]
        base = m.text
        if a["writes"]:
            rules.append({"when": base, "calls": [{"name": "fs_write", "input": {"path": w["path"], "content": w["content"]}}
                                                  for w in a["writes"]]})
        elif a["reply"] != REPLY:
            rules.append({"when": base, "text": a["reply"]})
    return list(reversed(rules))


def ambiguous_rules(rules: list[dict]) -> list[str]:
    """Rules whose `when` is inside another rule's: the first matching would take the other's message."""
    bad = []
    for i, r in enumerate(rules):
        for j, o in enumerate(rules):
            if i != j and r["when"] != o["when"] and r["when"] in o["when"]:
                bad.append(r["when"])
    return bad
