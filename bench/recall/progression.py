"""The incidental-recall bench's format (theseus-7gir.17): one scripted
progression of work, replayed identically to every arm.

A progression is:

- **sessions**: a label and a date. Each session's first turn opens with its
  date ("It's Tuesday, 2026-11-03."): neither arm's CLI takes a clock, so the
  days between sessions are dated text.
- **turns**: the user's text, in order, each in a session and a topic block,
  with the workspace edits the driver makes before sending it (`before`: the
  world changing under the arm, a log rotated, a script fixed) and the bytes
  its work is expected to read (`est_tokens`, for the budget). A turn with
  `mark` is a compaction mark: a bulk read the script puts where it expects
  the arm's context to fill. Marks are the plan; the scorer measures every
  distance by where each arm actually compacted.
- **facts**: an id, a subject ("the alder relay's port"), its kind (`port`,
  `path`, `version`, `host`, `ticket`, `date`), its value, its salience
  (`incidental`: said once in passing, in a tool's output, an error message,
  an aside or a side remark; `central`: the topic of the moment), its family,
  the turn that states it, its source (the script or file that shows it, or
  `said` for the user's own words), the text whose presence in the arm's
  transcript proves the fact reached it (`marker`), and what it supersedes.
- **probes**: the fact, its kind (`direct`, `indirect`, `abstention`), its
  planned distance bucket, the turn that asks it, and its check (the check
  language, `checks.py`). An indirect probe's check reads the file its task
  writes; an abstention probe asks after a subject never stated, near facts
  of its kind (its distractors).
- **workspace**: the files every arm starts from, identical for each.

Families (theseus-exam's, for their kinds and checks only, and one more):
`tool_output` (the value is in a command's output or error), `said` (the
user's own words: an aside, a side remark, the topic), `superseded` (a value
a later fact replaces, never probed), `time` (when something happened: the
value is its session's date), `distractor` (a same-kind value near an
abstention probe, never probed), and `needs_nothing` (an abstention probe's
subject: nothing to recall, and nothing to invent).

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import stat
from dataclasses import asdict, dataclass, field
from pathlib import Path

import checks

FORMAT = "recall-progression-v1"

SALIENCES = ("incidental", "central")
PROBE_KINDS = ("direct", "indirect", "abstention")
# In the order of distance, nearest first; `supersession` stands apart (a
# probe of a value that replaced another, at any distance).
BUCKETS = ("near", "topic_shift", "compaction", "session", "days", "supersession")
CURVE_BUCKETS = BUCKETS[:-1]
FAMILIES = ("tool_output", "said", "superseded", "time", "distractor", "needs_nothing")
CARRIERS = ("output", "error", "aside", "remark", "topic")
INCIDENTAL_CARRIERS = ("output", "error", "aside", "remark")

# What a value of each kind looks like in an answer: the abstention check (no
# value of the asked kind), and confident-wrong (a value of the kind, not the
# right one). Python's `re`, as the check language runs it.
KIND_REGEX = {
    "port": r"(?<![\w.-])[1-9]\d{4}(?![\w-]|\.\d)",
    "path": r"/srv/[\w.-]+/[\w./-]+",
    "version": r"(?<![\w.])\d+\.\d+\.\d+(?!\w|\.\d)",
    "host": r"\b[a-z]+-[a-z]+-\d{2}\.lan\b",
    "ticket": r"\b[A-Z]{3}-\d{4}\b",
    "date": (
        r"\b20\d\d-\d\d-\d\d\b|\b(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.? \d{1,2}\b"
        r"|\b\d{1,2} (?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)"
    ),
}
KIND_FLAGS = {"date": "i"}

# An answer that hedges (not confident), and one that admits it doesn't know
# (an abstention's second line).
HEDGE = (
    r"\b(?:i think|i believe|probably|might|may be|maybe|not sure|unsure|uncertain|possibly|likely|"
    r"if i recall|i guess|guessing|i don'?t know|don'?t recall|do not recall|can'?t find|couldn'?t find|"
    r"no record|not certain|can'?t confirm|cannot confirm|unclear|i'?m not)\b"
)
ADMIT = (
    r"\b(?:don'?t|do not|didn'?t|did not|can'?t|cannot|couldn'?t|could not|haven'?t|have not|hasn'?t|has not|"
    r"wasn'?t|was not|never|no)\b[^.?!]{0,80}?\b(?:know|recall|remember|record|records|mention|mentioned|"
    r"stated|said|told|seen|see|find|found|information|info|idea|sure|aware|specified|given|note|notes)\b"
    r"|\bnot (?:sure|certain|mentioned|stated|specified|recorded|given|known)\b|\bunknown\b"
    r"|\bno (?:record|mention|information|sign|trace)\b"
)

MONTHS = (
    "january february march april may june july august september october november december".split()
)


@dataclass
class Edit:
    """A file the driver writes (or, with `content` None, removes) in the
    arm's workspace before a turn."""

    path: str
    content: str | None
    executable: bool = False


@dataclass
class Session:
    label: str
    date: str  # YYYY-MM-DD
    weekday: str


@dataclass
class Turn:
    index: int
    session: int
    block: int
    topic: str
    text: str
    role: str  # opener | filler | fact | probe | bulk
    est_tokens: int
    before: list[Edit] = field(default_factory=list)
    mark: bool = False


@dataclass
class Fact:
    id: str
    subject: str
    kind: str
    value: str
    salience: str
    family: str
    carrier: str
    turn: int
    source: str
    marker: str
    supersedes: str | None = None
    superseded_by: str | None = None


@dataclass
class Probe:
    id: str
    fact: str | None  # None for an abstention probe
    kind: str
    bucket: str
    salience: str
    value_kind: str
    turn: int
    text: str
    check: str
    # The fact the distance is measured from: the probed fact, or an
    # abstention's nearest distractor.
    anchor: str
    subject: str
    file: str | None = None  # an indirect probe's output
    stale: str | None = None  # a supersession's old value
    distractors: list[str] = field(default_factory=list)


@dataclass
class Progression:
    format: str
    seed: int
    size: str
    sessions: list[Session]
    turns: list[Turn]
    facts: list[Fact]
    probes: list[Probe]
    workspace: dict[str, dict]  # path -> {"content", "executable"}
    context_window: int  # the scratch window that brings compaction near the marks
    names: list[str] = field(default_factory=list)  # every invented name used

    # ---- I/O

    def to_json(self) -> dict:
        return asdict(self)

    @staticmethod
    def from_json(d: dict) -> "Progression":
        if d.get("format") != FORMAT:
            raise ValueError(f"not a {FORMAT} file: format {d.get('format')!r}")
        return Progression(
            format=d["format"],
            seed=d["seed"],
            size=d["size"],
            sessions=[Session(**s) for s in d["sessions"]],
            turns=[Turn(**{**t, "before": [Edit(**e) for e in t["before"]]}) for t in d["turns"]],
            facts=[Fact(**f) for f in d["facts"]],
            probes=[Probe(**p) for p in d["probes"]],
            workspace=d["workspace"],
            context_window=d["context_window"],
            names=d.get("names", []),
        )

    def canonical(self) -> bytes:
        return json.dumps(self.to_json(), sort_keys=True, separators=(",", ":")).encode()

    def digest(self) -> str:
        return hashlib.sha256(self.canonical()).hexdigest()

    def save(self, out: Path) -> Path:
        out.mkdir(parents=True, exist_ok=True)
        p = out / "progression.json"
        p.write_text(json.dumps(self.to_json(), indent=1, sort_keys=True) + "\n")
        return p

    # ---- lookups

    def fact(self, fid: str) -> Fact:
        for f in self.facts:
            if f.id == fid:
                return f
        raise KeyError(fid)

    def facts_by_id(self) -> dict[str, Fact]:
        return {f.id: f for f in self.facts}

    def marks(self) -> list[int]:
        return [t.index for t in self.turns if t.mark]

    def session_turns(self, s: int) -> list[Turn]:
        return [t for t in self.turns if t.session == s]


def load(path: Path) -> Progression:
    """A progression from its file, or from a directory holding
    `progression.json` (a generator's out or a run's)."""
    p = Path(path)
    if p.is_dir():
        p = p / "progression.json"
    return Progression.from_json(json.loads(p.read_text()))


# ---- the workspace


def materialize(prog: Progression, dest: Path) -> None:
    """Write the starting workspace into `dest` (which must be empty or
    absent): the same bytes for every arm."""
    dest.mkdir(parents=True, exist_ok=True)
    if any(dest.iterdir()):
        raise FileExistsError(f"{dest} is not empty")
    for rel, f in sorted(prog.workspace.items()):
        write_file(dest, Edit(rel, f["content"], f.get("executable", False)))


def write_file(root: Path, e: Edit) -> None:
    p = root / e.path
    if e.content is None:
        if p.exists():
            p.unlink()
        return
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(e.content)
    mode = 0o755 if e.executable else 0o644
    os.chmod(p, mode)


def apply_before(turn: Turn, root: Path) -> None:
    """The world's changes before `turn`."""
    for e in turn.before:
        write_file(root, e)


def is_executable(p: Path) -> bool:
    return bool(p.stat().st_mode & stat.S_IXUSR)


# ---- buckets


def crosses(marks: list[int], f: int, p: int) -> bool:
    """A compaction at turn m (a mark, or where an arm compacted) falls
    between the fact's turn and its probe's: the fact was said before the
    turn whose request was compacted, and the probe asked in that turn or
    after it."""
    return any(f < m <= p for m in marks)


def bucket_of(prog: Progression, f: int, p: int, compactions: list[int], superseding: bool) -> str:
    """The distance bucket of a fact said at turn `f` and probed at turn `p`,
    given the turns at which the arm compacted (`compactions`: a turn during
    which the context was compacted). Supersession stands apart."""
    if superseding:
        return "supersession"
    tf, tp = prog.turns[f], prog.turns[p]
    if tf.session != tp.session:
        sf, sp = prog.sessions[tf.session], prog.sessions[tp.session]
        return "days" if sf.date != sp.date else "session"
    if crosses(compactions, f, p):
        return "compaction"
    if tf.block != tp.block:
        return "topic_shift"
    return "near"


def planned_bucket(prog: Progression, probe: Probe) -> str:
    facts = prog.facts_by_id()
    a = facts[probe.anchor]
    return bucket_of(prog, a.turn, probe.turn, prog.marks(), probe.bucket == "supersession")


# ---- validation


class Invalid(ValueError):
    pass


def validate(prog: Progression) -> None:
    """The format's rules: each fact before its probe and probed once, every
    check parses, a value appears in no turn but its own (and no probe), an
    abstention's subject is never stated, and each probe's planned bucket is
    what the marks say."""
    facts = prog.facts_by_id()
    if len(facts) != len(prog.facts):
        raise Invalid("two facts share an id")
    for i, t in enumerate(prog.turns):
        if t.index != i:
            raise Invalid(f"turn {i} has index {t.index}")
    probed: dict[str, str] = {}
    for p in prog.probes:
        if p.kind not in PROBE_KINDS or p.bucket not in BUCKETS or p.salience not in SALIENCES:
            raise Invalid(f"{p.id}: kind {p.kind}, bucket {p.bucket}, salience {p.salience}")
        try:
            checks.parse(p.check)
        except checks.CheckError as e:
            raise Invalid(f"{p.id}: {e}") from None
        if prog.turns[p.turn].role != "probe":
            raise Invalid(f"{p.id}: turn {p.turn} is a {prog.turns[p.turn].role} turn")
        a = facts.get(p.anchor)
        if a is None:
            raise Invalid(f"{p.id}: no fact {p.anchor}")
        if a.turn >= p.turn:
            raise Invalid(f"{p.id}: asked at turn {p.turn}, not after its fact's turn {a.turn}")
        if p.kind == "abstention":
            if p.fact is not None:
                raise Invalid(f"{p.id}: an abstention probes no fact")
            low = p.subject.lower()
            for t in prog.turns:
                if t.index != p.turn and low in t.text.lower():
                    raise Invalid(f"{p.id}: its subject is said at turn {t.index}")
        else:
            if p.fact not in facts:
                raise Invalid(f"{p.id}: no fact {p.fact}")
            if p.fact in probed:
                raise Invalid(f"fact {p.fact} is probed twice: {probed[p.fact]} and {p.id}")
            probed[p.fact] = p.id
            f = facts[p.fact]
            if f.family in ("superseded", "distractor", "needs_nothing"):
                raise Invalid(f"{p.id}: a {f.family} fact is never probed")
            if (p.bucket == "supersession") != (f.supersedes is not None):
                raise Invalid(f"{p.id}: bucket {p.bucket}, but fact {f.id} supersedes {f.supersedes}")
            if p.kind == "indirect" and not p.file:
                raise Invalid(f"{p.id}: an indirect probe names the file its task writes")
        want = planned_bucket(prog, p)
        if want != p.bucket:
            raise Invalid(f"{p.id}: planned {p.bucket}, but the marks make it {want}")
    for f in prog.facts:
        if f.kind == "date":
            continue
        for t in prog.turns:
            if t.index != f.turn and f.value.lower() in t.text.lower():
                raise Invalid(f"{f.id}'s value {f.value} is in turn {t.index}'s text")
        for p in prog.probes:
            if f.value.lower() in p.text.lower():
                raise Invalid(f"{f.id}'s value {f.value} is in {p.id}'s text")
        if f.supersedes is not None:
            old = facts[f.supersedes]
            if old.superseded_by != f.id or old.subject != f.subject or old.turn >= f.turn:
                raise Invalid(f"{f.id} supersedes {old.id} inconsistently")
    values = [f.value for f in prog.facts if f.kind != "date"]
    if len(set(values)) != len(values):
        raise Invalid("two facts share a value")


def cells(prog: Progression) -> dict[tuple[str, str, str], int]:
    """Probes per (bucket, salience, kind), as planned."""
    out: dict[tuple[str, str, str], int] = {}
    for p in prog.probes:
        k = (p.bucket, p.salience, p.kind)
        out[k] = out.get(k, 0) + 1
    return out


def expected_cells() -> list[tuple[str, str, str]]:
    """Every cell a full progression fills: each bucket × salience × kind,
    but an abstention has no supersession (nothing was stated to replace)."""
    return [
        (b, s, k)
        for b in BUCKETS
        for s in SALIENCES
        for k in PROBE_KINDS
        if not (k == "abstention" and b == "supersession")
    ]


def kind_check(kind: str) -> str:
    """The abstention's lines: no value of the asked kind, and an admission."""
    fl = KIND_FLAGS.get(kind, "")
    return f"reply lacks /{_slashed(KIND_REGEX[kind])}/{fl}\nreply has /{_slashed(ADMIT)}/i"


def _slashed(rx: str) -> str:
    """A regex as the check language writes it: each `/` escaped."""
    return rx.replace("/", "\\/")


def value_patterns(kind: str, value: str) -> list[str]:
    """The check patterns that find `value`: dates in the forms people write."""
    if kind == "date":
        y, m, d = (int(x) for x in value.split("-"))
        mon = MONTHS[m - 1]
        alts = [value, f"{mon} {d}", f"{mon[:3]} {d}", f"{mon[:3]}. {d}", f"{d} {mon}", f"{d} {mon[:3]}"]
        out = []
        for a in alts:
            # "nov 3" must not match "nov 30": a word match.
            out.append(f"word {checks.quote(a)}")
        return out
    return [f"word {checks.quote(value)}"]


def value_check(subject: str, kind: str, value: str, stale: str | None) -> str:
    """The direct or indirect check of `value` in `subject` (`reply`, or
    `file "<path>"`): it is there, and a superseded value is not."""
    lines = [f"{subject} has " + " | ".join(value_patterns(kind, value))]
    if stale is not None:
        lines.append(f"{subject} lacks " + " | ".join(value_patterns(kind, stale)))
    return "\n".join(lines)


def find_values(kind: str, text: str) -> list[str]:
    """The values of `kind` in `text`."""
    flags = re.I if kind in KIND_FLAGS else 0
    return [m.group(0) for m in re.finditer(KIND_REGEX[kind], checks.fold(text), flags)]
