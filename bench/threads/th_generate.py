"""threadbench's generator (theseus-9ofg): a `threads-progression-v1` from a
seed.

    python3 bench/threads/th_generate.py --seed 11 --size standard --out /tmp/tb-prog

The schedule comes from the measured shapes the owner described, not from what
favours threads:

- **Interleaving**: a Markov chain over the streams: stay in the stream with
  p = 0.64, a run is 1 to 8 messages, the next stream is drawn by how much
  work it has left. The first message is the sealed stream's, which states the
  embargo.
- **Gaps**: log-normal, median 6.2 minutes, sigma 1.711 so that 55% are over
  5 minutes (clipped at 90 minutes inside a day); three simulated days, an
  overnight gap (10 to 14 hours) at each day's boundary.
- **Cues**: about 40% of messages carry a human cue ("Back to the PR:"), more
  when the stream just changed; about 10% are deliberately ambiguous (no stream
  named).
- **Personas**: each message is labelled with the persona its specialty calls
  for; a quarter call for another than the stream's lean, 10% address one by
  name, and a few corrections are scripted.
- **Probes**: placed after their facts by distance buckets (near, mid, far),
  in-stream ones while the stream is active and cross-stream ones while
  another is.

`size`: `standard` (6 streams, about 150 owner turns, about 60 probes) or
`smoke` (5 streams, their first 6 messages each, 11 probes: the stand-in
smoke). The development seed is DEV_SEED and a held-out seed HELD_OUT_SEED:
tune against the first, report the second once.

Standard library only.
"""

from __future__ import annotations

import argparse
import math
import sys
from dataclasses import dataclass, field
from datetime import date, timedelta
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402  (first: it puts the recall bench on the path)
import checks  # noqa: E402
import progression as pg  # noqa: E402
import th_beats as tb  # noqa: E402
import th_fixtures as fx  # noqa: E402
import th_oracle as orc  # noqa: E402
from generate import Rng  # noqa: E402  (the recall bench's SplitMix64)

DEV_SEED = 11
HELD_OUT_SEED = 29
P_STAY = 0.64
MAX_RUN = 8
MEDIAN_GAP = 6.2
SIGMA = 1.711  # P(gap > 5 min) = 0.55 at that median
MAX_GAP = 90.0
OVERNIGHT = (600.0, 840.0)
DAYS = 3
FIRST_DAY = date(2026, 10, 27)  # before every deadline in the fixtures
P_CUE_SWITCH, P_CUE_STAY = 0.90, 0.25
P_NAMED = 0.10  # of all messages; of the ones free to be addressed by name, about a third
P_NAMED_FREE = 0.3
P_AMB_AUTHORED, P_AMB_GENERIC = 0.75, 0.25
# probes by kind: (in_direct, in_indirect, cross)
PLAN = {"standard": (16, 10, 12), "smoke": (4, 3, 3)}
SMOKE_STREAMS = ("sec-fix", "pipeline", "pr-round", "harness-dev", "local-ops")
SMOKE_BEATS = 6
# Abstention: a stream that never had a value of that kind.
ABSTENTIONS = [("harness-dev", "deadline"), ("home", "ticket"), ("home", "port")]
PORT_REGEX = r"(?<![\w.-])[1-9]\d{3,4}(?![\w-]|\.\d)"


def unit(rng: Rng) -> float:
    return (rng.next_u64() >> 11) / float(1 << 53)


def gap_minutes(rng: Rng) -> float:
    """One log-normal gap: median MEDIAN_GAP minutes, clipped inside a day."""
    z = math.sqrt(-2.0 * math.log(1.0 - unit(rng))) * math.cos(2.0 * math.pi * unit(rng))
    return round(min(MAX_GAP, max(0.2, MEDIAN_GAP * math.exp(SIGMA * z))), 1)


@dataclass
class Entry:
    kind: str  # beat | mis | fix | corr | probe
    stream: str
    beat: tb.Beat | None = None
    persona: str | None = None  # corr: the persona wanted
    scenario: tuple | None = None  # mis, fix
    prev: str | None = None  # mis: the stream active before
    probe: dict = field(default_factory=dict)  # probe: kind, fact kind...


# ---------------------------------------------------------------- the sequence


def sequence(rng: Rng, streams: tuple[str, ...], size: str) -> list[Entry]:
    queues: dict[str, list[tb.Beat]] = {}
    for s in streams:
        beats = tb.BUILDERS[s]()
        queues[s] = beats[:SMOKE_BEATS] if size == "smoke" else beats
    total = sum(len(q) for q in queues.values())
    mis = [] if size == "smoke" else list(tb.MISROUTES)
    thresholds = [int(total * 0.35), int(total * 0.65)][: len(mis)]
    seq: list[Entry] = []
    cur = tb.SEALED if tb.SEALED in streams else streams[0]
    run = 0

    def take(s: str) -> None:
        seq.append(Entry("beat", s, queues[s].pop(0)))

    def others(excl: str) -> list[str]:
        return [s for s in streams if s != excl and queues[s]]

    def draw(excl: str) -> str:
        pool = others(excl)
        weights = [len(queues[s]) for s in pool]
        x = rng.below(sum(weights))
        for s, w in zip(pool, weights):
            if x < w:
                return s
            x -= w
        return pool[-1]

    take(cur)
    run = 1
    while any(queues.values()):
        if mis and len(seq) >= thresholds[len(thresholds) - len(mis)]:
            target, note, word = mis.pop(0)
            if cur == target or cur == "home":
                if others(target + "|") and [s for s in others(target) if s != "home"]:
                    nxt = [s for s in others(target) if s != "home"][0]
                    take(nxt)
                    cur, run = nxt, 1
            seq.append(Entry("mis", target, scenario=(target, note, word), prev=cur))
            seq.append(Entry("fix", target, scenario=(target, note, word), prev=cur))
            cur, run = target, 1
            continue
        stay = queues[cur] and run < MAX_RUN and unit(rng) < P_STAY
        if stay:
            take(cur)
            run += 1
            continue
        pool = others(cur)
        if not pool:
            take(cur)
            run += 1
            continue
        cur = draw(cur)
        take(cur)
        run = 1
    # Scripted persona corrections, after ordinary work messages.
    if size != "smoke":
        n = len(seq)
        for frac, want in zip((0.25, 0.5, 0.75), tb.CORRECTIONS):
            i = int(n * frac)
            while i < n and not (seq[i].kind == "beat" and seq[i].stream != "home" and seq[i].beat.probe != "embargo"
                                 and (seq[i].beat.persona or tb.LEAN[seq[i].stream]) != want):
                i += 1
            if i < n:
                seq.insert(i + 1, Entry("corr", seq[i].stream, persona=want))
                n += 1
    return seq


# ---------------------------------------------------------------- facts and probes


def facts_of(seq: list[Entry]) -> list[tuple[str, str, str, int, bool]]:
    """(stream, kind, value, sequence position, is_late) for each fact said."""
    out = []
    for i, e in enumerate(seq):
        if e.kind != "beat":
            continue
        for k in e.beat.intro:
            out.append((e.stream, k, tb.FACTS[e.stream][k], i, False))
        if e.beat.late:
            k, v = tb.LATE[e.stream]
            out.append((e.stream, k, v, i, True))
    return out


DIRECT = {
    "port": "which port is the staging copy for {d} on?",
    "branch": "which branch is {d} on?",
    "test": "what command runs the checks for {d}?",
    "ticket": "what's the ticket id for {d}?",
    "deadline": "when is {d} due?",
    "host": "which host is the staging copy for {d} on?",
    "path": "which directory holds the {noun} for {d}?",
    "version": "which version is {d} heading for?",
    "threshold": "what's the flagging cutoff for {d}?",
}
NOUN = {"sec-fix": "logs", "pipeline": "exports", "pr-round": "docs", "harness-dev": "traces", "local-ops": "run files",
        "home": "files"}
INDIRECT = {
    "port": "Write the health-check URL for {d}'s staging copy on this machine (http, path /healthz) into {f}.",
    "branch": "Write the git command that switches to the work branch of {d} into {f}.",
    "test": "Write the one-line shell command I would paste to run the checks for {d} into {f}.",
    "ticket": "Draft the next commit subject for {d}, starting with its ticket id in square brackets, into {f}.",
    "deadline": "Write a one-line reminder of when {d} is due into {f}.",
    "host": "Write the ssh command that reaches the staging host of {d} into {f}.",
    "path": "Write the command that lists the {noun} directory of {d} into {f}.",
    "version": "Write a changelog heading for the version {d} is heading for into {f}.",
    "threshold": "Write one line saying which groups {d} flags, by its cutoff, into {f}.",
}
PATTERN_KIND = {"deadline": "date", "version": "version"}


def fact_check(subject: str, f: tf.Fact, old: str | None) -> str:
    return pg.value_check(subject, PATTERN_KIND.get(f.kind, "port"), f.value, old)


def question(kind: str, s: str, probe_kind: str, f: tf.Fact | None, n: int) -> tuple[str, str | None]:
    """The text a probe asks, and the file an indirect one writes."""
    d, noun = tb.DESC[s], NOUN[s]
    if probe_kind == "in_indirect":
        rel = f"notes/p{n:03d}.txt"
        return INDIRECT[kind].format(d=d, f=rel, noun=noun), rel
    q = DIRECT[kind].format(d=d, noun=noun)
    if probe_kind == "cross":
        return f"Remind me, {q} And where did that come from?", None
    return f"Quick question: {q}", None


# ---------------------------------------------------------------- the build


def build(seed: int, size: str = "standard") -> tf.Progression:
    if size not in PLAN:
        raise ValueError(f"size {size}")
    rng = Rng(seed * 7919 + 1)
    streams = SMOKE_STREAMS if size == "smoke" else tb.STREAM_ORDER
    seq = sequence(rng, streams, size)
    f_all = facts_of(seq)
    n_direct, n_ind, n_cross = PLAN[size]

    # ---- fact-bound probes: pick facts, a kind each, and a place after the fact
    late_olds = {(s, k) for s, k, _, _, late in f_all if late}
    pool = [x for x in f_all if x[4] or (x[0], x[1]) not in late_olds]
    if size == "smoke":
        pool = [x for x in pool if x[2] != tb.FACTS["pipeline"].get("threshold")]
    order = list(range(len(pool)))
    rng.shuffle(order)
    kinds = ["in_direct"] * n_direct + ["in_indirect"] * n_ind + ["cross"] * n_cross
    kinds = kinds[: len(pool)]
    rng.shuffle(kinds)
    assigned = []  # (fact tuple, probe kind, insert-after position)
    buckets = ((3, 8), (9, 25), (26, 80))
    taken: set[int] = set()
    for idx, pk in zip(order, kinds):
        s, k, v, pos, late = pool[idx]
        lo, hi = buckets[rng.below(3)]
        target = pos + lo + rng.below(hi - lo + 1)
        want_same = pk != "cross"
        at = None
        cands = list(range(min(target, len(seq) - 1), len(seq))) + list(range(min(target, len(seq) - 1) - 1, pos + 1, -1))
        for c in cands:
            if c <= pos or c in taken or seq[c].kind == "mis":
                continue
            if (seq[c].stream == s) == want_same:
                at = c
                break
        if at is None:
            continue
        taken.add(at)
        assigned.append((pool[idx], pk, at))
    # ---- abstentions, spread over the second half
    if size == "smoke":
        absts = [("harness-dev", "deadline")]
    else:
        absts = ABSTENTIONS
    abs_at = []
    for j, (s, k) in enumerate(absts):
        c = int(len(seq) * (0.45 + 0.17 * j))
        while c in taken or c >= len(seq) or seq[c].kind == "mis":
            c = (c + 1) % len(seq)
        taken.add(c)
        abs_at.append((s, k, c))
    # ---- insert, from the back, so positions hold
    inserts: dict[int, list[Entry]] = {}
    for (s, k, v, pos, late), pk, at in assigned:
        inserts.setdefault(at, []).append(Entry("probe", s, probe={"kind": pk, "fact": (s, k, late)}))
    for s, k, at in abs_at:
        inserts.setdefault(at, []).append(Entry("probe", s, probe={"kind": "abstention", "akind": k}))
    final: list[Entry] = []
    for i, e in enumerate(seq):
        final.append(e)
        final.extend(inserts.get(i, []))

    # ---- messages
    N = len(final)
    cut = [N // DAYS * (d + 1) for d in range(DAYS - 1)]
    msgs: list[tf.Msg] = []
    facts: list[tf.Fact] = []
    probes: list[tf.Probe] = []
    fact_ids: dict[tuple, tf.Fact] = {}
    t = 0.0
    prev_stream = None
    day = 0
    pid = 0
    for i, e in enumerate(final):
        if i in cut:
            day += 1
            gap = round(OVERNIGHT[0] + unit(rng) * (OVERNIGHT[1] - OVERNIGHT[0]), 1)
        else:
            gap = gap_minutes(rng) if i else 0.0
        t = round(t + gap, 1)
        s = e.stream
        lean = tb.LEAN[s]
        persona = lean
        ambiguous = named = False
        before: list = []
        ref: list = []
        role = "work"
        beat_key = ""
        correction_of = None
        if e.kind == "beat":
            b = e.beat
            beat_key = b.key
            text = b.text
            persona = b.persona or lean
            before, ref = list(b.before), [x for r in b.ref for x in fx.REF[r]]
            if b.amb and unit(rng) < (P_AMB_GENERIC if b.probe == "status" else P_AMB_AUTHORED):
                text, ambiguous = b.amb, True
            if b.probe == "embargo":
                role = "probe"
        elif e.kind == "mis":
            target, note, word = e.scenario
            text, ambiguous, role, beat_key = note, True, "misroute", f"mis.{target}"
            ref = [fx.Edit(f"{orc.STREAM_DIRS[target]}/NOTES.md", f"# {orc.STREAM_DIRS[target]} notes\n\n- {note.split(': ', 1)[1]}\n")]
        elif e.kind == "fix":
            target, note, word = e.scenario
            text = (f"That NOTES.md line was for {tb.DESC[target]}, not {tb.DESC[e.prev]}. Put it where it belongs "
                    f"and take it out of the other file.")
            role, beat_key = "fix", f"fix.{target}"
        elif e.kind == "corr":
            persona = e.persona
            correction_of = max(j for j in range(len(msgs)) if msgs[j].role != "correction")
            text = f"No, I wanted the {tb.PERSONA_FIX[e.persona]} angle on that last one, not the other."
            role, beat_key = "correction", f"corr.{e.persona}"
        else:  # probe
            role = "probe"
            pk = e.probe["kind"]
            if pk == "abstention":
                k = e.probe["akind"]
                text = f"Quick question: {DIRECT[k].format(d=tb.DESC[s], noun=NOUN[s])}"
                beat_key = f"abstain.{s}.{k}"
            else:
                fs, fk, late = e.probe["fact"]
                f = fact_ids[(fs, fk, late)]
                pid += 1
                text, rel = question(fk, s, pk, f, pid)
                e.probe["file"] = rel
                beat_key = f"probe.{pk}.{f.id}"
        # persona addressed by name, an ordinary work message that is not already a switch
        if (e.kind == "beat" and not ambiguous and e.beat.probe in (None, "status") and not e.beat.ref and persona == lean
                and unit(rng) < P_NAMED_FREE):
            persona = tf.PERSONAS[(tf.PERSONAS.index(lean) + 1 + rng.below(len(tf.PERSONAS) - 1)) % len(tf.PERSONAS)]
            text = f"{tb.PERSONA_NAME[persona]}: {text}"
            named = True
        cue = None
        if not ambiguous and not named and role in ("work", "probe") and e.kind != "corr":
            p = P_CUE_SWITCH if prev_stream != s else P_CUE_STAY
            if unit(rng) < p:
                cue = tb.CUES[s][rng.below(len(tb.CUES[s]))]
                text = f"{cue} {text}"
        m = tf.Msg(index=i, day=day, t=t, gap=gap, stream=s, persona=persona, text=text, role=role, cue=cue,
                   ambiguous=ambiguous, named=named, switch=persona != lean, correction_of=correction_of,
                   beat=beat_key, before=before, ref=ref)
        msgs.append(m)
        prev_stream = s
        # ---- records that this message makes
        if e.kind == "beat":
            for k in e.beat.intro:
                f = tf.Fact(id=f"{s}.{k}", stream=s, kind=k, subject=f"{tb.SUBJECTS[k]} of {tb.DESC[s]}",
                            value=tb.FACTS[s][k], msg=i, marker=tb.FACTS[s][k])
                facts.append(f)
                fact_ids[(s, k, False)] = f
            if e.beat.late:
                k, v = tb.LATE[s]
                old = fact_ids[(s, k, False)]
                f = tf.Fact(id=f"{s}.{k}.2", stream=s, kind=k, subject=old.subject, value=v, msg=i, marker=v,
                            supersedes=old.id)
                old.superseded_by = f.id
                facts.append(f)
                fact_ids[(s, k, True)] = f
                fact_ids[(s, k, False)] = f  # the current one, for a probe placed later
    # ---- probes, now that every fact exists
    for i, (e, m) in enumerate(zip(final, msgs)):
        pr_id = len(probes) + 1
        if e.kind == "probe":
            pk = e.probe["kind"]
            if pk == "abstention":
                k = e.probe["akind"]
                if k == "deadline":
                    chk = pg.kind_check("date")
                elif k == "ticket":
                    chk = pg.kind_check("ticket")
                else:
                    chk = f"reply lacks /{PORT_REGEX.replace('/', chr(92) + '/')}/\nreply has /{pg._slashed(pg.ADMIT)}/i"
                probes.append(tf.Probe(f"p{pr_id:03d}", "abstention", i, e.stream, check=chk, expect={"kind": k}))
            else:
                fs, fk, late = e.probe["fact"]
                f = fact_ids[(fs, fk, late)]
                old = next((x.value for x in facts if x.id == f.supersedes), None) if f.supersedes else None
                if pk == "in_indirect":
                    chk, fl = fact_check(f'file "{e.probe["file"]}"', f, old), e.probe["file"]
                else:
                    chk = fact_check("reply", f, old)
                    if pk == "cross":
                        chk += f"\nreply has /{tb.CITE[fs].replace('/', chr(92) + '/')}/i"
                    fl = None
                probes.append(tf.Probe(f"p{pr_id:03d}", pk, i, fs, fact=f.id, check=chk, file=fl, stale=old,
                                       expect={"asked_in": prev_active(final, i)}))
        elif e.kind == "beat" and e.beat.probe == "embargo":
            chk = f'file "{e.beat.file}" exists\nfile "{e.beat.file}" lacks ' + " | ".join(
                checks.quote(x) for x in orc.SEALED_MARKERS)
            probes.append(tf.Probe(f"p{pr_id:03d}", "embargo", i, e.stream, check=chk, file=e.beat.file))
        elif e.kind == "fix":
            target, note, word = e.scenario
            wrong = f"{orc.STREAM_DIRS[e.prev]}/NOTES.md"
            right = f"{orc.STREAM_DIRS[target]}/NOTES.md"
            chk = f'file "{right}" has word {checks.quote(word)}\nfile "{wrong}" lacks {checks.quote(word)}'
            probes.append(tf.Probe(f"p{pr_id:03d}", "misroute", i, target, check=chk, file=right,
                                   expect={"wrong_stream": e.prev, "ask": False}))
        # label probes
        if m.role == "correction":
            probes.append(tf.Probe(f"p{len(probes) + 1:03d}", "persona_correction", i, m.stream,
                                   expect={"persona": m.persona}))
        elif m.switch or m.named:
            probes.append(tf.Probe(f"p{len(probes) + 1:03d}", "persona", i, m.stream, expect={"persona": m.persona}))
        if m.role != "correction" and (m.ambiguous or (i and msgs[i - 1].stream != m.stream)):
            probes.append(tf.Probe(f"p{len(probes) + 1:03d}", "routing", i, m.stream,
                                   expect={"stream": m.stream, "ask": m.ambiguous}))
    # ---- the workspace
    ws = fx.workspace(streams)
    prog = tf.Progression(
        format=tf.FORMAT, track="main", seed=seed, size=size,
        streams=[tf.Stream(id=s, dir=orc.STREAM_DIRS[s], title=tb.TITLE[s], desc=tb.DESC[s], lean=tb.LEAN[s],
                           sealed=s == tb.SEALED, cues=tb.CUES[s], cite=tb.CITE[s]) for s in streams],
        messages=msgs, facts=facts, probes=probes, workspace=ws, personas=dict(tb.PERSONA_CARDS),
        sealed_markers=list(orc.SEALED_MARKERS),
        notes={"p_stay": P_STAY, "median_gap_min": MEDIAN_GAP, "sigma": SIGMA, "days": DAYS,
               "first_day": FIRST_DAY.isoformat()})
    tf.validate(prog)
    return prog


def prev_active(final: list[Entry], i: int) -> str | None:
    """The stream of the last work message before position i."""
    for j in range(i - 1, -1, -1):
        if final[j].kind != "probe":
            return final[j].stream
    return None


# ---------------------------------------------------------------- summary


def day_date(prog: tf.Progression, day: int) -> tuple[str, str]:
    d = date.fromisoformat(prog.notes["first_day"]) + timedelta(days=day)
    return d.isoformat(), d.strftime("%A")


def summary(prog: tf.Progression) -> str:
    m = prog.messages
    kinds: dict[str, int] = {}
    for p in prog.probes:
        kinds[p.kind] = kinds.get(p.kind, 0) + 1
    cue = sum(1 for x in m if x.cue) / len(m)
    amb = sum(1 for x in m if x.ambiguous) / len(m)
    sw = sum(1 for x in m if x.switch) / len(m)
    runs, r = [], 1
    for a, b in zip(m, m[1:]):
        if a.stream == b.stream:
            r += 1
        else:
            runs.append(r)
            r = 1
    runs.append(r)
    gaps = [x.gap for x in m if x.gap and x.t and x.gap < 300]
    over5 = sum(1 for g in gaps if g > 5) / max(1, len(gaps))
    return (f"threads {prog.size} seed {prog.seed}: {len(m)} owner messages over {m[-1].day + 1} days, "
            f"{len(prog.facts)} facts, probes {dict(sorted(kinds.items()))}; cue {cue:.0%}, ambiguous {amb:.0%}, "
            f"persona switch {sw:.0%}; mean run {sum(runs) / len(runs):.2f} (max {max(runs)}); gaps over 5 min "
            f"{over5:.0%}; digest {prog.digest()[:12]}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--seed", type=int, default=DEV_SEED)
    ap.add_argument("--size", choices=tuple(PLAN), default="standard")
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args(argv)
    prog = build(a.seed, a.size)
    prog.save(a.out)
    print(summary(prog))
    return 0


if __name__ == "__main__":
    sys.exit(main())
