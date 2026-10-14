#!/usr/bin/env python3
"""threadbench's persona track (theseus-9ofg): five synthetic personas, each a ~60-token specialization card; every
message labelled with the persona its specialty calls for (a quarter call for another than the stream's lean, a
tenth address one by name, a few are corrections). An OFFLINE driver puts a draft question (`packs/persona.v0.toml`,
modelled on role.v1 and route.v4) to an answerer for each message on its light history slice, and scores the answer
against three baselines:

- **thread prior**: the persona the message's own stream has used most recently (a thread's habit); the first
  message of a stream, with no habit yet, is a generalist's;
- **cards keywords**: the persona whose card shares the most words with the message (ties go to the prior);
- **oracle**: the label.

    python3 bench/threads/th_persona.py --seed 11 --out /tmp/tb-persona
    python3 bench/threads/th_persona.py --seed 11 --out /tmp/tb-persona --dump-inputs     # InboundInput JSON per message
    python3 bench/threads/th_persona.py --seed 11 --out /tmp/tb-persona --jev-probe target/release/jev-probe

The default answerer is a STAND-IN: it answers the label, wrong at a seeded rate (`--noise`, 0.2), so that the
plumbing, the baselines and the scores are proved offline; its accuracy says nothing about Jev. The maintainer
runs the same inputs on Jev with `--jev-probe` (best-effort parse of the probe's printed answers: check the first
output by hand, `--dump-inputs` writes the exact files). Messages that correct (role `correction`) are scored
apart: the answer must follow the correction, which is the label.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import th_generate as gen  # noqa: E402
from generate import Rng  # noqa: E402

PACK = HERE / "packs" / "persona.v0.toml"
SLICE = 3  # the light history: this many owner messages before the one asked
STOP = set("a an and are as at be by for from has have how i in is it its my of on or so that the this to we what "
           "when which with you your do not no if can will".split())


def words(s: str) -> set[str]:
    return {w for w in re.findall(r"[a-z][a-z-]+", s.lower()) if w not in STOP and len(w) > 2}


# Words a person uses when they want the work a persona's specialty does: the card's own, and the plain ones.
HINTS = {
    "security": "attacker hostile risk secret trust embargo exploit unsafe safe fix leak",
    "builder": "implement tests test run suite add flag fix green merge refactor write",
    "research": "numbers why how data assumptions compare cutoff statistics usually opinion",
    "ops": "restart pid version service running verify check shutdown deploy stand-in",
    "generalist": "quick plain words summarize line status paste convert errand",
}


def keywords(persona: str, card: str) -> set[str]:
    return words(card) | set(HINTS[persona].split())


def history(prog: tf.Progression, i: int) -> str:
    return " | ".join(m.text for m in prog.messages[max(0, i - SLICE): i])


def inbound_input(prog: tf.Progression, i: int, current: str) -> dict:
    """The message as role.v1's `inbound` builder takes it (theseus-judge's InboundInput)."""
    m = prog.messages[i]
    return {"message": m.text, "author": "operator", "place_kind": "cli",
            "previous_human_message": history(prog, i) or None, "last_reply": None,
            "minutes_since_last_message": int(m.gap) if i else None, "current_role": current,
            "roles": [{"id": p, "stance": prog.personas[p]} for p in tf.PERSONAS]}


def thread_prior(prog: tf.Progression, i: int, labels: list[str]) -> str:
    """The persona the stream's own latest message used (the thread's habit), a generalist's if there is none."""
    s = prog.messages[i].stream
    for j in range(i - 1, -1, -1):
        if prog.messages[j].stream == s:
            return labels[j]
    return "generalist"


def keyword_pick(prog: tf.Progression, i: int, prior: str) -> str:
    w = words(prog.messages[i].text)
    best, top = [], -1
    for p in tf.PERSONAS:
        n = len(w & keywords(p, prog.personas[p]))
        if n > top:
            best, top = [p], n
        elif n == top:
            best.append(p)
    return prior if prior in best else best[0]


class StandIn:
    """Answers the label, wrong at a seeded rate: the plumbing's answerer, not a model."""

    def __init__(self, prog: tf.Progression, seed: int, noise: float):
        self.prog, self.rng, self.noise = prog, Rng(seed * 31 + 7), noise

    def __call__(self, i: int, inp: dict) -> dict:
        label = self.prog.messages[i].persona
        if gen.unit(self.rng) < self.noise:
            other = [p for p in tf.PERSONAS if p != label]
            return {"persona": other[self.rng.below(len(other))], "p": 0.6}
        return {"persona": label, "p": 0.95}


class JevProbe:
    """`jev-probe --pack <pack> --input <file>` per message. The probe prints each question's answers with their
    probabilities; the parse below takes the persona id with the highest number printed beside it. It is
    best-effort and unverified against a live run: read the first output (`raw/`)."""

    def __init__(self, binary: str, pack: str, raw: Path | None):
        self.binary, self.pack, self.raw = binary, pack, raw

    def __call__(self, i: int, inp: dict) -> dict:
        f = (self.raw or Path("/tmp")) / f"persona-{i:03d}.input.json"
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(json.dumps(inp))
        r = subprocess.run([self.binary, "--pack", self.pack, "--input", str(f)], capture_output=True, text=True, timeout=120)
        if self.raw:
            (self.raw / f"persona-{i:03d}.out.txt").write_text(r.stdout + r.stderr)
        best, bp = "other", 0.0
        for line in r.stdout.splitlines():
            m = re.search(r"\b(security|builder|research|ops|generalist|other)\b\D{0,12}(0?\.\d+|1\.0+)", line)
            if m and float(m.group(2)) > bp:
                best, bp = m.group(1), float(m.group(2))
        return {"persona": best, "p": bp}


def run(prog: tf.Progression, answer) -> dict:
    labels = [m.persona for m in prog.messages]
    rows = []
    for i, m in enumerate(prog.messages):
        # what the thread used last, as the question's `current_role`
        prior = thread_prior(prog, i, labels)
        a = answer(i, inbound_input(prog, i, prior))
        kw = keyword_pick(prog, i, prior)
        rows.append({"msg": i, "stream": m.stream, "label": m.persona, "switch": m.switch, "named": m.named,
                     "correction": m.role == "correction", "answer": a["persona"], "p": a["p"], "prior": prior,
                     "keywords": kw})

    def acc(sel, key):
        rs = [r for r in rows if sel(r)]
        return {"n": len(rs), "correct": sum(r[key] == r["label"] for r in rs),
                "rate": round(sum(r[key] == r["label"] for r in rs) / len(rs), 4) if rs else None}

    groups = {"all": lambda r: True, "switch": lambda r: r["switch"], "steady": lambda r: not r["switch"],
              "named": lambda r: r["named"], "correction": lambda r: r["correction"]}
    oracle_rows = [dict(r, oracle=r["label"]) for r in rows]
    out = {g: {"answer": acc(f, "answer"), "prior": acc(f, "prior"), "keywords": acc(f, "keywords"),
               "oracle": {"n": acc(f, "answer")["n"], "correct": acc(f, "answer")["n"], "rate": 1.0 if acc(f, "answer")["n"] else None}}
           for g, f in groups.items()}
    conf: dict[str, dict[str, int]] = {}
    for r in rows:
        conf.setdefault(r["label"], {}).setdefault(r["answer"], 0)
        conf[r["label"]][r["answer"]] += 1
    return {"groups": out, "confusion": conf, "rows": rows, "messages": len(rows),
            "cards_tokens_approx": {p: round(len(prog.personas[p]) / 4) for p in tf.PERSONAS}}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--seed", type=int, default=gen.DEV_SEED)
    ap.add_argument("--size", choices=tuple(gen.PLAN), default="standard")
    ap.add_argument("--progression", type=Path, default=None)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--noise", type=float, default=0.2, help="the stand-in answerer's seeded error rate")
    ap.add_argument("--jev-probe", default=None, help="the jev-probe binary: ask Jev instead of the stand-in")
    ap.add_argument("--pack", default="role.v1", help="with --jev-probe: the embedded pack id (persona.v0 once embedded)")
    ap.add_argument("--dump-inputs", action="store_true")
    a = ap.parse_args(argv)
    prog = tf.load(a.progression) if a.progression else gen.build(a.seed, a.size)
    a.out.mkdir(parents=True, exist_ok=True)
    if a.dump_inputs:
        d = a.out / "inputs"
        d.mkdir(exist_ok=True)
        labels = [m.persona for m in prog.messages]
        for i in range(len(prog.messages)):
            (d / f"persona-{i:03d}.input.json").write_text(
                json.dumps(inbound_input(prog, i, thread_prior(prog, i, labels)), indent=1))
    if a.jev_probe:
        raw = a.out / "raw"
        raw.mkdir(exist_ok=True)
        answerer, who = JevProbe(a.jev_probe, a.pack, raw), f"jev-probe --pack {a.pack}"
    else:
        answerer, who = StandIn(prog, prog.seed, a.noise), f"stand-in (noise {a.noise})"
    res = run(prog, answerer)
    res["answerer"] = who
    res["pack"] = str(PACK.relative_to(HERE.parent.parent))
    (a.out / "persona.json").write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")
    g = res["groups"]
    print(f"persona track ({who}) over {res['messages']} messages: answer {g['all']['answer']['rate']}, prior "
          f"{g['all']['prior']['rate']}, keywords {g['all']['keywords']['rate']}; on the {g['switch']['answer']['n']} "
          f"switch messages: answer {g['switch']['answer']['rate']}, prior {g['switch']['prior']['rate']}, "
          f"keywords {g['switch']['keywords']['rate']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
