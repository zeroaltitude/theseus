"""threadbench's format (theseus-9ofg): `threads-progression-v1`, the scripted
life of one owner across several clearly separable streams of work, replayed
identically to every arm.

A progression is:

- **streams**: an id, its directory in the workspace, whether it is sealed
  (embargoed: nothing of it may reach another stream's artifacts), and the
  persona its work leans on.
- **messages**: each owner message in order: its text, its TRUE stream, the
  persona its specialty calls for, the addressing cue it carries (or none),
  whether it is deliberately ambiguous, the gap before it (minutes) and its
  day, the workspace files that arrive before it (`before`) and the reference
  effect an ideal arm has (`ref`, used by the oracles' tests and the stand-in).
- **facts**: a stream's value of a kind (port, branch, test, ticket, deadline,
  host, path, version, threshold), where it was introduced, whether a later
  fact superseded it, and the marker whose presence in an arm's transcript
  proves it arrived. The same kinds collide across streams on purpose: the
  values differ, some by one digit.
- **probes**: each scored; `in_direct`, `in_indirect`, `cross`, `embargo`,
  `abstention`, `misroute` (all checked by the check language, `checks.py` of
  the recall bench, on the reply or on a file), and `routing`, `persona`,
  `persona_correction` (labels on a message, scored from a routing record or by
  the persona track; they ask no question of their own).
- **workspace**: every stream's fixture, the same bytes for each arm.

The rules the validator holds, as the recall bench's do: a probe never comes
before its fact; a fact is probed once; a value is said in its own message and
nowhere else; every check parses; the sealed stream's embargo is stated before
any embargo probe.

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
RECALL = HERE.parent / "recall"
if str(RECALL) not in sys.path:
    sys.path.insert(0, str(RECALL))

import checks  # noqa: E402,F401  (the recall bench's check language)
from progression import Edit  # noqa: E402

FORMAT = "threads-progression-v1"
TRACKS = ("main", "prefix")
CHECK_KINDS = ("in_direct", "in_indirect", "cross", "embargo", "abstention", "misroute")
LABEL_KINDS = ("routing", "persona", "persona_correction")
PROBE_KINDS = CHECK_KINDS + LABEL_KINDS
# misroute and embargo probes ask no fact; the rest of the check kinds do, but abstention.
FACT_KINDS = ("in_direct", "in_indirect", "cross")
PERSONAS = ("security", "builder", "research", "ops", "generalist")
ROLES = ("work", "probe", "correction", "misroute", "fix")


@dataclass
class Stream:
    id: str
    dir: str
    title: str
    desc: str  # how the owner names it: "the ledgerweb fix"
    lean: str  # the persona its work mostly calls for
    sealed: bool = False
    cues: list[str] = field(default_factory=list)
    cite: str = ""  # a regex naming it, for the source a cross-stream answer cites


@dataclass
class Msg:
    index: int
    day: int
    t: float  # minutes since the run's first message
    gap: float  # minutes since the message before (0 for the first)
    stream: str  # the TRUE stream
    persona: str  # the persona the message's specialty calls for
    text: str
    role: str = "work"  # work | probe | correction | misroute | fix
    cue: str | None = None
    ambiguous: bool = False
    named: bool = False  # the persona is addressed by name
    switch: bool = False  # the persona differs from the stream's lean
    correction_of: int | None = None
    reopen: bool = False  # a scripted cold gap: this message opens a fresh session (the prefix track)
    beat: str = ""
    before: list[Edit] = field(default_factory=list)
    ref: list[Edit] = field(default_factory=list)


@dataclass
class Fact:
    id: str
    stream: str
    kind: str
    subject: str  # "the staging port"
    value: str
    msg: int
    marker: str
    supersedes: str | None = None
    superseded_by: str | None = None


@dataclass
class Probe:
    id: str
    kind: str
    msg: int  # the message that asks it (check kinds) or is labelled (label kinds)
    stream: str  # the stream it concerns
    fact: str | None = None
    check: str = ""
    file: str | None = None
    stale: str | None = None  # a superseded value that must be absent
    family: str | None = None  # the prefix track's header probes: the family's value ...
    sibling: str | None = None  # ... and the near-miss sibling's, a false fit
    expect: dict = field(default_factory=dict)


@dataclass
class Progression:
    format: str
    track: str
    seed: int
    size: str
    streams: list[Stream]
    messages: list[Msg]
    facts: list[Fact]
    probes: list[Probe]
    workspace: dict[str, dict]
    personas: dict[str, str]
    sealed_markers: list[str] = field(default_factory=list)
    notes: dict = field(default_factory=dict)

    # ---- I/O

    def to_json(self) -> dict:
        return asdict(self)

    @staticmethod
    def from_json(d: dict) -> "Progression":
        if d.get("format") != FORMAT:
            raise ValueError(f"not a {FORMAT} file: format {d.get('format')!r}")
        edits = lambda es: [Edit(**e) for e in es]  # noqa: E731
        return Progression(
            format=d["format"], track=d["track"], seed=d["seed"], size=d["size"],
            streams=[Stream(**s) for s in d["streams"]],
            messages=[Msg(**{**m, "before": edits(m["before"]), "ref": edits(m["ref"])}) for m in d["messages"]],
            facts=[Fact(**f) for f in d["facts"]], probes=[Probe(**p) for p in d["probes"]],
            workspace=d["workspace"], personas=d["personas"], sealed_markers=d.get("sealed_markers", []),
            notes=d.get("notes", {}),
        )

    def canonical(self) -> bytes:
        return json.dumps(self.to_json(), sort_keys=True, separators=(",", ":")).encode()

    def digest(self) -> str:
        return hashlib.sha256(self.canonical()).hexdigest()

    def save(self, out: Path) -> Path:
        out.mkdir(parents=True, exist_ok=True)
        p = out / "threads-progression.json"
        p.write_text(json.dumps(self.to_json(), indent=1, sort_keys=True) + "\n")
        return p

    # ---- lookups

    def stream(self, sid: str) -> Stream:
        for s in self.streams:
            if s.id == sid:
                return s
        raise KeyError(sid)

    def facts_by_id(self) -> dict[str, Fact]:
        return {f.id: f for f in self.facts}

    def current_fact(self, stream: str, kind: str, at: int) -> Fact | None:
        """The stream's fact of `kind` in force at message `at`."""
        cur = None
        for f in self.facts:
            if f.stream == stream and f.kind == kind and f.msg <= at:
                cur = f
        return cur

    def probes_at(self, msg: int) -> list[Probe]:
        return [p for p in self.probes if p.msg == msg]


def load(path: Path) -> Progression:
    """A progression from its file, or from a directory holding `threads-progression.json`."""
    p = Path(path)
    if p.is_dir():
        p = p / "threads-progression.json"
    return Progression.from_json(json.loads(p.read_text()))


class Invalid(ValueError):
    pass


def validate(prog: Progression) -> None:
    """The format's rules (see the module doc)."""
    if prog.track not in TRACKS:
        raise Invalid(f"track {prog.track}")
    sids = {s.id for s in prog.streams}
    if len(sids) != len(prog.streams):
        raise Invalid("two streams share an id")
    texts: dict[str, int] = {}
    prev_t = -1.0
    for i, m in enumerate(prog.messages):
        if m.index != i:
            raise Invalid(f"message {i} has index {m.index}")
        if m.stream not in sids:
            raise Invalid(f"message {i}: no stream {m.stream}")
        if m.persona not in PERSONAS or m.role not in ROLES:
            raise Invalid(f"message {i}: persona {m.persona}, role {m.role}")
        if m.gap < 0 or m.t < prev_t or (i and m.day < prog.messages[i - 1].day):
            raise Invalid(f"message {i}: time runs backwards")
        prev_t = m.t
        # Two ambiguous questions that act on nothing may read alike (that is what ambiguity is): nothing else repeats.
        if not (m.ambiguous and m.role == "work" and not m.ref):
            if m.text in texts:
                raise Invalid(f"message {i} repeats message {texts[m.text]}'s text")
            texts[m.text] = i
        if m.ambiguous and m.cue:
            raise Invalid(f"message {i} is ambiguous and carries a cue")
        if m.cue and not m.text.startswith(m.cue):
            raise Invalid(f"message {i}: its cue is not its opening")
    facts = prog.facts_by_id()
    if len(facts) != len(prog.facts):
        raise Invalid("two facts share an id")
    for f in prog.facts:
        if not 0 <= f.msg < len(prog.messages) or prog.messages[f.msg].stream != f.stream:
            raise Invalid(f"{f.id}: introduced at message {f.msg}, not in its stream")
        if f.marker.lower() not in prog.messages[f.msg].text.lower():
            raise Invalid(f"{f.id}: its marker is not said in message {f.msg}")
        if f.supersedes is not None:
            old = facts[f.supersedes]
            if old.superseded_by != f.id or (old.stream, old.kind) != (f.stream, f.kind) or old.msg >= f.msg:
                raise Invalid(f"{f.id} supersedes {old.id} inconsistently")
        # A value is said in its own message and nowhere else.
        for m in prog.messages:
            if m.index != f.msg and f.value.lower() in m.text.lower():
                raise Invalid(f"{f.id}'s value {f.value} is in message {m.index}'s text")
    probed: dict[str, str] = {}
    for p in prog.probes:
        if p.kind not in PROBE_KINDS:
            raise Invalid(f"{p.id}: kind {p.kind}")
        if not 0 <= p.msg < len(prog.messages) or p.stream not in sids:
            raise Invalid(f"{p.id}: message {p.msg}, stream {p.stream}")
        m = prog.messages[p.msg]
        if p.kind in CHECK_KINDS:
            if m.role not in ("probe", "misroute", "fix"):
                raise Invalid(f"{p.id}: message {p.msg} is a {m.role} message, not a question")
            try:
                checks.parse(p.check)
            except checks.CheckError as e:
                raise Invalid(f"{p.id}: {e}") from None
        if p.kind in FACT_KINDS:
            f = facts.get(p.fact or "")
            if f is None:
                raise Invalid(f"{p.id}: no fact {p.fact}")
            if f.msg >= p.msg:
                raise Invalid(f"{p.id}: asked at message {p.msg}, not after its fact's message {f.msg}")
            if f.superseded_by is not None:
                raise Invalid(f"{p.id}: asks a superseded fact")
            if p.fact in probed:
                raise Invalid(f"fact {p.fact} is probed twice: {probed[p.fact]} and {p.id}")
            probed[p.fact] = p.id
            if p.kind == "in_indirect" and not p.file:
                raise Invalid(f"{p.id}: an indirect probe names the file its task writes")
            if f.value.lower() in m.text.lower():
                raise Invalid(f"{p.id}: its question says the answer")
        if p.kind == "abstention":
            if p.fact is not None:
                raise Invalid(f"{p.id}: an abstention probes no fact")
            kind = p.expect.get("kind")
            if any(f.stream == p.stream and f.kind == kind for f in prog.facts):
                raise Invalid(f"{p.id}: stream {p.stream} has a {kind}")
        if p.kind == "embargo":
            sealed = [s for s in prog.streams if s.sealed]
            if not sealed:
                raise Invalid(f"{p.id}: no stream is sealed")
            first = min((x.msg for x in prog.facts if x.stream == sealed[0].id), default=None)
            if first is None or first >= p.msg or p.stream == sealed[0].id:
                raise Invalid(f"{p.id}: the embargo is not stated before it, or it asks the sealed stream")
        if p.kind == "persona_correction" and prog.messages[p.msg].correction_of is None:
            raise Invalid(f"{p.id}: message {p.msg} corrects nothing")
        if p.kind in ("persona", "persona_correction") and p.expect.get("persona") not in PERSONAS:
            raise Invalid(f"{p.id}: no persona")
    for m in prog.messages:
        if m.correction_of is not None and not 0 <= m.correction_of < m.index:
            raise Invalid(f"message {m.index} corrects a later message")
        if m.role == "fix" and (m.index == 0 or prog.messages[m.index - 1].role != "misroute"):
            raise Invalid(f"message {m.index}: a fix follows its misrouted message")
