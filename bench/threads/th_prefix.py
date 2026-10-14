"""threadbench's prefix-library track (theseus-9ofg): two streams of one repo family, each reopened once as a
fresh session after a scripted cold gap, and a near-miss sibling project beside them.

The family (`railfam/`) has a way to check, lint, preview and log changes; the owner says it once, in the first
stream's first message. The sibling (`railsib/`) has the same layout and a different test command, lint command,
port and changelog file, said once in its own message. After the cold gap each family stream reopens and the owner
asks its HEADER (the family's way of working) without restating it: an answer that gives the sibling's value is a
FALSE FIT, counted apart from a plain wrong one. The track measures what a reusable header (a prefix library)
would buy; here, before one exists, it measures what a fresh session already knows.

    python3 bench/threads/th_prefix.py --seed 11 --out /tmp/tb-prefix

Standard library only.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import checks  # noqa: E402
import progression as pg  # noqa: E402
import th_beats as tb  # noqa: E402
import th_generate as gen  # noqa: E402
from generate import Rng  # noqa: E402

DIRS = {"rail-a": "railfam/alpha-svc", "rail-b": "railfam/beta-svc", "rail-sib": "railsib/gamma-svc"}
DESC = {"rail-a": "alpha-svc", "rail-b": "beta-svc", "rail-sib": "gamma-svc"}
FAMILY = {"test": "make verify", "port": "7100", "doc": "CHANGES.md", "lint": "make lint"}
SIBLING = {"test": "./ci/run.sh", "port": "7110", "doc": "HISTORY.md", "lint": "npm run lint"}
OWN = {"rail-a": {"branch": "feat/alpha-timeouts", "ticket": "RLF-3011"},
       "rail-b": {"branch": "feat/beta-retries", "ticket": "RLF-3012"},
       "rail-sib": {"branch": "feat/gamma-cache", "ticket": "RSB-2201"}}
COLD_GAP = (1080.0, 1200.0)  # 18 to 20 hours: past any cache TTL


def section(stream: str, heading: str, line: str) -> list[tf.Edit]:
    p = f"{DIRS[stream]}/NOTES.md"
    return [tf.Edit(p, f"# {DESC[stream]} notes\n\n## {heading}\n\n{line}\n")]


def notes(stream: str, parts: list[tuple[str, str]]) -> tf.Edit:
    body = "".join(f"\n## {h}\n\n{l}\n" for h, l in parts)
    return tf.Edit(f"{DIRS[stream]}/NOTES.md", f"# {DESC[stream]} notes\n{body}")


def build(seed: int = 11) -> tf.Progression:
    rng = Rng(seed * 104729 + 3)
    f, s = FAMILY, SIBLING
    # (stream, text, role, facts introduced [(kind, value, bucket)], notes parts so far)
    steps: list[dict] = [
        dict(s="rail-a", text=f"The railfam services are checked with {f['test']}, linted with {f['lint']}, preview on port "
                              f"{f['port']}, and log their changes in {f['doc']}. I'm on alpha-svc today: branch "
                              f"{OWN['rail-a']['branch']}, ticket {OWN['rail-a']['ticket']}.",
             facts=[("test", f["test"], "fam"), ("lint", f["lint"], "fam"), ("port", f["port"], "fam"),
                    ("doc", f["doc"], "fam"), ("branch", OWN["rail-a"]["branch"], "own"),
                    ("ticket", OWN["rail-a"]["ticket"], "own")]),
        dict(s="rail-a", text="Add a section 'Timeouts' to railfam/alpha-svc/NOTES.md saying calls time out after 30 seconds.",
             note=("rail-a", "Timeouts", "Calls time out after 30 seconds.")),
        dict(s="rail-b", text=f"beta-svc is the other service in the family: branch {OWN['rail-b']['branch']}, ticket "
                              f"{OWN['rail-b']['ticket']}.", facts=[("branch", OWN["rail-b"]["branch"], "own"),
                                                                   ("ticket", OWN["rail-b"]["ticket"], "own")]),
        dict(s="rail-b", text="Add a section 'Retries' to railfam/beta-svc/NOTES.md saying calls retry three times.",
             note=("rail-b", "Retries", "Calls retry three times.")),
        dict(s="rail-sib", text=f"Different project, railsib: the same layout as the family, but its checks run with "
                                f"{s['test']}, lint with {s['lint']}, it previews on port {s['port']}, and it logs changes in "
                                f"{s['doc']}. Branch {OWN['rail-sib']['branch']}, ticket {OWN['rail-sib']['ticket']}. Add a section "
                                f"'Cache' to railsib/gamma-svc/NOTES.md saying entries live ten minutes.",
             facts=[("test", s["test"], "sib"), ("lint", s["lint"], "sib"), ("port", s["port"], "sib"),
                    ("doc", s["doc"], "sib"), ("branch", OWN["rail-sib"]["branch"], "own"),
                    ("ticket", OWN["rail-sib"]["ticket"], "own")],
             note=("rail-sib", "Cache", "Entries live ten minutes.")),
        dict(s="rail-a", text="Add a section 'Logging' to railfam/alpha-svc/NOTES.md saying logs rotate daily.",
             note=("rail-a", "Logging", "Logs rotate daily.")),
        dict(s="rail-b", text="Add a section 'Backoff' to railfam/beta-svc/NOTES.md saying the wait doubles each try.",
             note=("rail-b", "Backoff", "The wait doubles each try.")),
        dict(s="rail-sib", text="Add a section 'Rotation' to railsib/gamma-svc/NOTES.md saying keys rotate weekly.",
             note=("rail-sib", "Rotation", "Keys rotate weekly.")),
        # ---- the cold gap: each family stream reopens as a fresh session and the owner asks its header
        dict(s="rail-a", reopen=True, probe=("in_direct", "test"),
             text="Back on alpha-svc after the night. What command runs the checks for this repo family?"),
        dict(s="rail-a", probe=("in_indirect", "port"),
             text="Write the health-check URL for alpha-svc's preview on this machine (http, path /healthz) into notes/{pid}.txt."),
        dict(s="rail-a", probe=("in_direct", "branch"), text="Which branch is alpha-svc on?"),
        dict(s="rail-a", text="Add a section 'Alerts' to railfam/alpha-svc/NOTES.md saying pages go out after three failures.",
             note=("rail-a", "Alerts", "Pages go out after three failures.")),
        dict(s="rail-b", reopen=True, probe=("in_direct", "doc"),
             text="Back on beta-svc. Where do changes get logged in this repo family?"),
        dict(s="rail-b", probe=("in_indirect", "lint"),
             text="Write the command I run to lint beta-svc before pushing into notes/{pid}.txt."),
        dict(s="rail-b", probe=("in_direct", "ticket"), text="What's the ticket id for beta-svc's work?"),
        dict(s="rail-b", text="Add a section 'Limits' to railfam/beta-svc/NOTES.md saying calls stop after five tries.",
             note=("rail-b", "Limits", "Calls stop after five tries.")),
    ]
    msgs: list[tf.Msg] = []
    facts: list[tf.Fact] = []
    probes: list[tf.Probe] = []
    parts: dict[str, list[tuple[str, str]]] = {k: [] for k in DIRS}
    t, day = 0.0, 0
    fid: dict[tuple[str, str], tf.Fact] = {}
    for i, st in enumerate(steps):
        if st.get("reopen") and day == 0:  # the first reopen follows the cold gap; the second is the same morning
            day = 1
            gap = round(COLD_GAP[0] + gen.unit(rng) * (COLD_GAP[1] - COLD_GAP[0]), 1)
        else:
            gap = gen.gap_minutes(rng) if i else 0.0
        t = round(t + gap, 1)
        ref = []
        if st.get("note"):
            sid, h, line = st["note"]
            parts[sid].append((h, line))
            ref = [notes(sid, parts[sid])]
        text = st["text"]
        if st.get("probe") and "{pid}" in text:
            text = text.replace("{pid}", f"p{i:03d}")
        msgs.append(tf.Msg(index=i, day=day, t=t, gap=gap, stream=st["s"], persona="builder", text=text,
                           role="probe" if st.get("probe") else "work", reopen=bool(st.get("reopen")),
                           beat=f"prefix.{i}", ref=ref))
        for kind, value, scope in st.get("facts", []):
            owner = "rail-a" if scope == "fam" else st["s"]
            tag = {"fam": "family", "sib": "sibling", "own": st["s"]}[scope]
            fobj = tf.Fact(id=f"{tag}.{kind}", stream=owner if scope != "sib" else "rail-sib", kind=kind,
                           subject=f"{tb.SUBJECTS.get(kind, kind)}", value=value, msg=i, marker=value)
            facts.append(fobj)
            fid[(tag, kind)] = fobj
    # ---- probes
    for i, st in enumerate(steps):
        if not st.get("probe"):
            continue
        pk, kind = st["probe"]
        sid = st["s"]
        own_kind = kind in ("branch", "ticket")
        f = fid[(sid, kind)] if own_kind else fid[("family", kind)]
        pid = f"p{len(probes) + 1:03d}"
        if pk == "in_indirect":
            rel = f"notes/p{i:03d}.txt"
            chk = gen.fact_check(f'file "{rel}"', f, None)
            probes.append(tf.Probe(pid, pk, i, sid, fact=f.id, check=chk, file=rel,
                                   family=None if own_kind else f.value, sibling=None if own_kind else SIBLING[kind],
                                   expect={"header": not own_kind}))
        else:
            chk = gen.fact_check("reply", f, None)
            probes.append(tf.Probe(pid, pk, i, sid, fact=f.id, check=chk,
                                   family=None if own_kind else f.value, sibling=None if own_kind else SIBLING[kind],
                                   expect={"header": not own_kind}))
    ws = {}
    for sid, d in DIRS.items():
        ws[f"{d}/NOTES.md"] = {"content": f"# {DESC[sid]} notes\n", "executable": False}
    prog = tf.Progression(
        format=tf.FORMAT, track="prefix", seed=seed, size="prefix",
        streams=[tf.Stream(id=sid, dir=DIRS[sid].split("/")[0], title=f"{DESC[sid]} of "
                           f"{'the railfam family' if sid != 'rail-sib' else 'the near-miss sibling railsib'}",
                           desc=DESC[sid], lean="builder", cues=[], cite=DESC[sid]) for sid in DIRS],
        messages=msgs, facts=facts, probes=probes, workspace=ws, personas=dict(tb.PERSONA_CARDS),
        notes={"first_day": gen.FIRST_DAY.isoformat(), "family": FAMILY, "sibling": SIBLING})
    tf.validate(prog)
    return prog


def oracle(stream: str, root: Path, prog: tf.Progression | None = None) -> list[tuple[str, bool]]:
    """A prefix stream's end state: its notes hold each section the owner asked for."""
    p = root / DIRS[stream] / "NOTES.md"
    text = p.read_text() if p.is_file() else ""
    want = {"rail-a": ["Timeouts", "Logging", "Alerts"], "rail-b": ["Retries", "Backoff", "Limits"],
            "rail-sib": ["Cache", "Rotation"]}[stream]
    return [(f"NOTES.md has the {h} section", f"## {h}" in text) for h in want]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--seed", type=int, default=gen.DEV_SEED)
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args(argv)
    prog = build(a.seed)
    prog.save(a.out)
    print(f"prefix track seed {a.seed}: {len(prog.messages)} messages, {len(prog.facts)} facts, "
          f"{len(prog.probes)} header and stream probes, digest {prog.digest()[:12]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
