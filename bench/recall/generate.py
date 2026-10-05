#!/usr/bin/env python3
"""The incidental-recall bench's generator (theseus-7gir.17): one scripted
progression of work from a seed, deterministic (SplitMix64, never `random`).

    python3 bench/recall/generate.py --seed 7 --size smoke --out /tmp/rc-smoke

It writes `progression.json` (the format, `progression.py`) and the starting
`workspace/` into `--out`, and prints its turns, facts and probes, each cell's
count, and the budget: the tokens it expects each arm to read and their
dollars at the catalog's price for `--model`.

- **smoke**: two sessions of 15 turns, three days apart, one compaction mark,
  and one probe of each kind (direct, indirect, abstention) at least, eight
  cells in all.
- **full**: three sessions of 200 turns (two on one day, the third nine days
  later), ten topic blocks each, a compaction mark in each session, and
  `FULL_PER_CELL` probes in every bucket × salience × kind cell (34 cells:
  an abstention has no supersession).

Every name is invented, from the lists below, and a test holds each one out
of theseus-exam's `exam-v2.toml`.

Standard library only.
"""

from __future__ import annotations

import argparse
import datetime as dt
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import checks  # noqa: E402
import progression as pg  # noqa: E402
from progression import Edit, Fact, Probe, Progression, Session, Turn  # noqa: E402

# ---- SplitMix64, as theseus-exam's rng.rs (Vigna's splitmix64.c)

MASK = (1 << 64) - 1


class Rng:
    def __init__(self, seed: int):
        self.state = seed & MASK

    def next_u64(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n: int) -> int:
        """Uniform in 0..n (n > 0); the modulo bias is below 2^-40 here."""
        if n <= 0:
            raise ValueError("below(0)")
        return self.next_u64() % n

    def pick(self, seq):
        return seq[self.below(len(seq))]

    def shuffle(self, v: list) -> None:
        """Fisher–Yates, as rng.rs's."""
        for i in range(len(v) - 1, 0, -1):
            j = self.below(i + 1)
            v[i], v[j] = v[j], v[i]


# ---- the invented names (none in exam-v2.toml: test_generate checks it)

PROJECTS = (
    "alder birch cedar egret finch grebe ibis jacana linnet merlin nuthatch oriole petrel quail rook "
    "siskin veery wigeon bramble cobalt fennel garnet harrow juniper larch nettle ochre pewter russet "
    "thistle umber vetch yarrow zinnia marrow tundra"
).split()
SERVICES = "relay ingest spooler indexer scheduler mailer exporter archiver renderer collector broker poller".split()
ROLES = "staging canary build replica bastion".split()
LIBS = "gantry hawser jetsam keelson lanyard marline oakum ratline scupper tiller bollard capstan davit fairlead grommet halyard".split()
PEOPLE = "orla benedikt tamsin ravi ingrid dorian feodora casimir lucan maren odile pascoe rosalind teodor vashti ysolde zoltan anouk bertil".split()
HOSTWORDS = "brack flint shale loam marl scree tarn gill holm fen".split()
SOURCES = "tufa wicker pumice basalt gneiss sienna vellum".split()
THINGS = (("spool file", "spool", "db"), ("lock file", "lock", "pid"), ("socket", "sock", "sock"), ("cache index", "cache", "idx"))
BUGS = ("lease timeout", "stale cache", "crash loop", "slow drain", "double send", "memory creep", "port clash", "dropped ack")
SYMPTOMS = (
    "checksum mismatch",
    "stalled queue",
    "clock skew warning",
    "truncated batch",
    "replay storm",
    "lease expiry burst",
    "half-open socket",
    "duplicate frame",
)
OUT_VERBS = "check probe status verify inspect audit".split()
ERR_VERBS = "sync deploy rotate migrate refresh publish".split()

NAME_LISTS = {
    "projects": PROJECTS,
    "libs": LIBS,
    "people": PEOPLE,
    "hostwords": HOSTWORDS,
    "sources": SOURCES,
}

KINDS = ("port", "path", "version", "host", "ticket", "date")
VALUE_KINDS = KINDS[:-1]  # the kinds an abstention or a supersession asks after

FULL_PER_CELL = 6
OVERHEAD_TOKENS = 12000  # a guess at each arm's system prompt and tool list
SMOKE_WINDOW = 32000

# The catalog's prices per million tokens (theseus-core's catalog.rs, the
# built-in table; test_generate holds this copy to it): input, output, cache
# read, cache write (5 minutes).
PRICES = {
    "claude-fable-5-1": (10.0, 50.0, 0.25, 12.50),
    "claude-opus-5-5": (4.0, 20.0, 0.20, 5.00),
    "claude-opus-5": (5.0, 25.0, 0.50, 6.25),
    "claude-sonnet-5-5": (2.0, 10.0, 0.20, 2.50),
    "claude-sonnet-5": (2.0, 10.0, 0.20, 2.50),
    "claude-haiku-4-5": (1.0, 5.0, 0.10, 1.25),
}

SMOKE_CELLS = (
    ("near", "incidental", "direct"),
    ("compaction", "incidental", "direct"),
    ("topic_shift", "central", "indirect"),
    ("days", "incidental", "indirect"),
    ("days", "central", "direct"),
    ("near", "incidental", "abstention"),
    ("days", "central", "abstention"),
    ("supersession", "incidental", "direct"),
)


class Layout:
    """The progression's skeleton: sessions, blocks, marks, and which turns
    are free for a fact or a probe."""

    def __init__(self, size: str, rng: Rng):
        if size == "smoke":
            self.session_turns, self.block_len = 15, 5
            day0 = dt.date(2026, 11, 2) + dt.timedelta(days=rng.below(40))
            self.dates = [day0, day0 + dt.timedelta(days=3)]
            self.mark_at = {0: 10}  # session -> its mark's offset
        elif size == "full":
            self.session_turns, self.block_len = 200, 20
            day0 = dt.date(2026, 11, 2) + dt.timedelta(days=rng.below(40))
            self.dates = [day0, day0, day0 + dt.timedelta(days=9)]
            self.mark_at = {0: 120, 1: 120, 2: 120}
        else:
            raise ValueError(f"size is smoke or full, not {size!r}")
        self.size = size
        self.n = self.session_turns * len(self.dates)
        self.marks = [s * self.session_turns + m for s, m in self.mark_at.items()]
        self.role = ["free"] * self.n
        for s in range(len(self.dates)):
            self.role[s * self.session_turns] = "opener"
        for m in self.marks:
            self.role[m] = "bulk"
            self.role[m + 1] = "filler"  # the turn after a mark holds nothing

    def session(self, t: int) -> int:
        return t // self.session_turns

    def block(self, t: int) -> int:
        """The block's index across the whole progression."""
        return t // self.block_len

    def free(self, t: int) -> bool:
        return 0 <= t < self.n and self.role[t] == "free"

    def crosses(self, f: int, p: int) -> bool:
        return pg.crosses(self.marks, f, p)

    def candidates(self, bucket: str, f: int) -> list[int]:
        """The probe turns that put a fact at `f` in `bucket`, as planned."""
        s = self.session(f)
        lo, hi = s * self.session_turns, (s + 1) * self.session_turns
        out: list[int] = []
        if bucket == "near":
            out = [p for p in range(f + 2, f + 5) if self.block(p) == self.block(f) and not self.crosses(f, p)]
        elif bucket == "topic_shift":
            b = self.block(f)
            out = [
                p
                for p in range(f + 1, hi)
                if b < self.block(p) <= b + 2 and not self.crosses(f, p)
            ]
        elif bucket == "compaction":
            out = [p for p in range(f + 1, hi) if self.crosses(f, p)]
        elif bucket in ("session", "days"):
            d = self.dates[s]
            for s2 in range(s + 1, len(self.dates)):
                same = self.dates[s2] == d
                if (bucket == "session") == same:
                    out += list(range(s2 * self.session_turns, (s2 + 1) * self.session_turns))
        elif bucket == "supersession":
            out = [p for p in range(f + 2, min(hi, f + 30))]
        return [p for p in out if self.free(p) and lo <= f]


class Builder:
    def __init__(self, seed: int, size: str):
        self.rng = Rng(seed)
        self.seed, self.size = seed, size
        self.lay = Layout(size, self.rng)
        self.facts: list[Fact] = []
        self.probes: list[Probe] = []
        self.slot: dict[int, tuple[str, object]] = {}  # turn -> ("fact"|"probe", obj)
        self.used_values: set[str] = set()
        self.used_subjects: set[tuple[str, str, str]] = set()
        self.used_pairs: set[tuple[str, str]] = set()  # (a, b) said anywhere
        self.reserved_pairs: set[tuple[str, str]] = set()  # an abstention's: never said
        self.used_libs: set[str] = set()
        self.reserved_libs: set[str] = set()
        self.topics: list[tuple[str, str]] = []
        self.names: set[str] = set()
        self._pick_topics()

    # ---- subjects and values

    def _pick_topics(self) -> None:
        nblocks = self.lay.n // self.lay.block_len
        pairs = [(p, s) for p in PROJECTS for s in SERVICES]
        self.rng.shuffle(pairs)
        self.topics = pairs[:nblocks]
        for t in self.topics:
            self.used_pairs.add(t)

    def _value(self, kind: str, a: str, b: str) -> str:
        r = self.rng
        for _ in range(1000):
            if kind == "port":
                v = str(20000 + r.below(20000))
            elif kind == "path":
                thing = self._thing(b)
                v = f"/srv/{a}/{b}/{thing[1]}-{10 + r.below(90)}.{thing[2]}"
            elif kind == "version":
                v = f"{2 + r.below(8)}.{r.below(20)}.{r.below(30)}"
            elif kind == "host":
                v = f"{r.pick(HOSTWORDS)}-{b}-{10 + r.below(90)}.lan"
            elif kind == "ticket":
                v = f"{a[:3].upper()}-{1000 + r.below(9000)}"
            else:
                raise ValueError(kind)
            if v not in self.used_values:
                self.used_values.add(v)
                return v
        raise RuntimeError("no fresh value")

    def _thing(self, b: str) -> tuple[str, str, str]:
        return THINGS[sum(map(ord, b)) % len(THINGS)]

    def _detail(self, kind: str, a: str, b: str) -> str:
        """The rest of a subject: a path's thing, a ticket's bug, a date's
        symptom; fixed by the pair, so a subject is one thing."""
        if kind == "path":
            return self._thing(b)[0]
        if kind == "ticket":
            return BUGS[(sum(map(ord, a)) + len(b)) % len(BUGS)]
        if kind == "date":
            return SYMPTOMS[(sum(map(ord, a + b))) % len(SYMPTOMS)]
        return ""

    def subject(self, kind: str, a: str, b: str) -> str:
        d = self._detail(kind, a, b)
        return {
            "port": f"the {a} {b}'s port",
            "path": f"the {a} {b}'s {d}",
            "version": f"the {b} version {a} pins",
            "host": f"the {a} {b} box's hostname",
            "ticket": f"the ticket for the {a} {b} {d}",
            "date": f"the date we first saw the {d} on the {a} {b}",
        }[kind]

    def keys(self, kind: str, a: str, b: str) -> list[str]:
        """The phrases every statement of a subject (a, b) carries, which an
        abstention's subject must never show."""
        if kind == "version":
            return [b]
        return [f"{a} {b}", f"{a}-{b}"]

    def _second(self, kind: str) -> list[str]:
        return {"version": LIBS, "host": list(ROLES)}.get(kind, SERVICES)

    def _free_pair(self, kind: str, *, abstention: bool = False, avoid=None) -> tuple[str, str] | None:
        """A fresh (a, b) for a fact of `kind`, or for an abstention (never
        said by anyone)."""
        for _ in range(400):
            a, b = self.rng.pick(PROJECTS), self.rng.pick(self._second(kind))
            if avoid is not None and not avoid(a, b):
                continue
            if (kind, a, b) in self.used_subjects:
                continue
            if kind == "version":
                if abstention and (b in self.used_libs or b in self.reserved_libs):
                    continue
                if not abstention and b in self.reserved_libs:
                    continue
            else:
                if (a, b) in self.reserved_pairs or (a, b) in self.topics:
                    continue
                if abstention and (a, b) in self.used_pairs:
                    continue
            return a, b
        return None

    def _use(self, kind: str, a: str, b: str) -> None:
        self.used_subjects.add((kind, a, b))
        if kind == "version":
            self.used_libs.add(b)
        else:
            self.used_pairs.add((a, b))

    # ---- placement

    def _fact(self, turn: int, kind: str, salience: str, family: str, a: str, b: str, value: str | None = None,
              carrier: str | None = None) -> Fact:
        if carrier is None:
            carrier = "topic" if salience == "central" else self.rng.pick(pg.INCIDENTAL_CARRIERS)
        if kind == "date":
            value = self.lay.dates[self.lay.session(turn)].isoformat()
        elif value is None:
            value = self._value(kind, a, b)
        if family in ("tool_output", "said"):
            family = "tool_output" if carrier in ("output", "error") else "said"
        f = Fact(
            id=f"f{len(self.facts) + 1:03d}",
            subject=self.subject(kind, a, b),
            kind=kind,
            value=value,
            salience=salience,
            family=family,
            carrier=carrier,
            turn=turn,
            source="said",
            marker=value,
        )
        f._ab = (a, b)  # type: ignore[attr-defined]
        self._use(kind, a, b)
        self.facts.append(f)
        self.slot[turn] = ("fact", f)
        self.lay.role[turn] = "fact"
        return f

    def _f_ok(self, salience: str, kind: str):
        """Whether turn t may hold a new fact of this salience and kind."""

        def ok(t: int) -> bool:
            if not self.lay.free(t):
                return False
            if salience == "central":
                a, b = self.topics[self.lay.block(t)]
                if kind in ("host", "version"):
                    return True
                return (kind, a, b) not in self.used_subjects
            return True

        return ok

    def _central_pair(self, t: int, kind: str) -> tuple[str, str] | None:
        a, b = self.topics[self.lay.block(t)]
        if kind == "host":
            r = [r for r in ROLES if (kind, a, r) not in self.used_subjects and (a, r) not in self.reserved_pairs]
            return (a, self.rng.pick(r)) if r else None
        if kind == "version":
            libs = [x for x in LIBS if x not in self.reserved_libs and (kind, a, x) not in self.used_subjects]
            return (a, self.rng.pick(libs)) if libs else None
        return a, b

    def _pairs(self, bucket: str, f_ok, tries: int = 400) -> tuple[int, int] | None:
        fs = [t for t in range(self.lay.n) if f_ok(t)]
        self.rng.shuffle(fs)
        for f in fs[:tries]:
            ps = self.lay.candidates(bucket, f)
            if ps:
                return f, self.rng.pick(ps)
        return None

    def place(self, bucket: str, salience: str, kind_: str) -> bool:
        """One probe of this cell, with its fact (and a supersession's stale
        one, an abstention's distractor); False if nothing fits."""
        kinds = list(VALUE_KINDS if (kind_ == "abstention" or bucket == "supersession") else KINDS)
        self.rng.shuffle(kinds)
        for kind in kinds:
            if self._place(bucket, salience, kind_, kind):
                return True
        return False

    def _place(self, bucket: str, salience: str, pkind: str, kind: str) -> bool:
        lay = self.lay
        if bucket == "supersession":
            # The stale value, earlier in the same session, in another block:
            # said in passing (a central one would be another block's topic).
            def stale_turns(f: int) -> list[int]:
                lo = lay.session(f) * lay.session_turns
                return [s for s in range(lo, f - 2) if lay.free(s) and lay.block(s) != lay.block(f)]

            f_ok = self._f_ok(salience, kind)
            got = self._pairs(bucket, lambda t: f_ok(t) and bool(stale_turns(t)))
            if got is None:
                return False
            f, p = got
            s = self.rng.pick(stale_turns(f))
            ab = self._central_pair(f, kind) if salience == "central" else self._free_pair(kind)
            if ab is None:
                return False
            a, b = ab
            old = self._fact(s, kind, "incidental", "superseded", a, b)
            old.family = "superseded"
            self.used_subjects.discard((kind, a, b))
            new = self._fact(f, kind, salience, "tool_output", a, b)
            new.supersedes, old.superseded_by = old.id, new.id
            self._probe(p, pkind, bucket, salience, new, stale=old.value)
            return True
        if pkind == "abstention":
            # The distractor: a fact of the same kind near the probe, whose
            # subject shares its second word; the probe asks after a pair
            # nobody ever says.
            got = self._pairs(bucket, self._f_ok(salience, kind))
            if got is None:
                return False
            d, p = got
            ab = self._central_pair(d, kind) if salience == "central" else self._free_pair(kind)
            if ab is None:
                return False
            a, b = ab
            if kind == "version":
                miss = self._free_pair(kind, abstention=True, avoid=lambda x, y: x == a and y != b)
            else:
                miss = self._free_pair(kind, abstention=True, avoid=lambda x, y: y == b and x != a)
            if miss is None:
                return False
            dist = self._fact(d, kind, salience, "distractor", a, b)
            dist.family = "distractor"
            ma, mb = miss
            if kind == "version":
                self.reserved_libs.add(mb)
            else:
                self.reserved_pairs.add((ma, mb))
            self._probe(p, pkind, bucket, salience, None, anchor=dist, miss=(kind, ma, mb))
            return True
        got = self._pairs(bucket, self._f_ok(salience, kind))
        if got is None:
            return False
        f, p = got
        ab = self._central_pair(f, kind) if salience == "central" else self._free_pair(kind)
        if ab is None:
            return False
        fact = self._fact(f, kind, salience, "time" if kind == "date" else "tool_output", *ab)
        self._probe(p, pkind, bucket, salience, fact)
        return True

    def _probe(self, turn: int, pkind: str, bucket: str, salience: str, fact: Fact | None, *,
               stale: str | None = None, anchor: Fact | None = None, miss=None) -> Probe:
        pid = f"p{len(self.probes) + 1:03d}"
        if fact is not None:
            kind, (a, b) = fact.kind, fact._ab  # type: ignore[attr-defined]
            subject = fact.subject
        else:
            kind, a, b = miss
            subject = self.subject(kind, a, b)
        f = None
        if pkind == "indirect":
            f = f"notes/{pid}.txt"
            text = indirect_text(kind, a, b, self._detail(kind, a, b), f)
            check = pg.value_check(f"file {checks.quote(f)}", kind, fact.value, stale)
        elif pkind == "direct":
            text = direct_text(kind, a, b, self._detail(kind, a, b))
            check = pg.value_check("reply", kind, fact.value, stale)
        else:
            text = direct_text(kind, a, b, self._detail(kind, a, b))
            check = pg.kind_check(kind)
        anc = fact if fact is not None else anchor
        p = Probe(
            id=pid,
            fact=fact.id if fact is not None else None,
            kind=pkind,
            bucket=bucket,
            salience=salience,
            value_kind=kind,
            turn=turn,
            text=text,
            check=check,
            anchor=anc.id,
            subject=subject,
            file=f,
            stale=stale,
            distractors=[anchor.id] if anchor is not None else [],
        )
        p._keys = self.keys(kind, a, b) if fact is None else []  # type: ignore[attr-defined]
        self.probes.append(p)
        self.slot[turn] = ("probe", p)
        self.lay.role[turn] = "probe"
        return p


# ---- the words of each turn


def direct_text(kind: str, a: str, b: str, d: str) -> str:
    return {
        "port": f"Quick question: which port is the {a} {b} on?",
        "path": f"Where does the {a} {b} keep its {d}? The full path, please.",
        "version": f"Which {b} version does {a} pin?",
        "host": f"What's the hostname of the {a} {b} box?",
        "ticket": f"Which ticket tracks the {a} {b} {d}?",
        "date": f"On which date did we first see the {d} on the {a} {b}?",
    }[kind]


def indirect_text(kind: str, a: str, b: str, d: str, file: str) -> str:
    return {
        "port": f"Write the URL of the {a} {b}'s health check on this machine (http, path /healthz) into {file}. Just the URL.",
        "path": f"Write a one-line shell command that copies the {a} {b}'s {d} into /tmp/backup/, into {file}.",
        "version": f"Write the requirements line that pins {b} the way {a} does ({b}==<version>) into {file}.",
        "host": f"Write the ssh command that logs into the {a} {b} box as the deploy user into {file}.",
        "ticket": f"Write a one-line commit message for the fix of the {a} {b} {d}, citing its ticket, into {file}.",
        "date": f"Add a line to {file} that says on which date (YYYY-MM-DD) we first saw the {d} on the {a} {b}.",
    }[kind]


def said_statement(kind: str, a: str, b: str, d: str, v: str) -> str:
    return {
        "port": f"the {a} {b} is on port {v} these days",
        "path": f"the {a} {b} keeps its {d} at {v}",
        "version": f"{a} pins {b} at {v}",
        "host": f"the {a} {b} box is {v}",
        "ticket": f"the {a} {b} {d} is tracked as {v}",
        "date": f"we just hit a {d} on the {a} {b}",
    }[kind]


def output_line(kind: str, a: str, b: str, d: str, v: str) -> str:
    return {
        "port": f"[{a}-{b}] listening on 127.0.0.1:{v}",
        "path": f"[{a}-{b}] {d} at {v}",
        "version": f"[{a}] {b} {v} loaded",
        "host": f"[{a}-{b}] target host {v}",
        "ticket": f"[{a}-{b}] known issue {v}: {d}",
        "date": f"WARN [{a}-{b}] {d}",
    }[kind]


def error_line(kind: str, a: str, b: str, d: str, v: str) -> str:
    return {
        "port": f"error: [{a}-{b}] connect 127.0.0.1:{v}: connection refused",
        "path": f"error: [{a}-{b}] cannot open {v}: No such file or directory",
        "version": f"error: [{a}] build failed against {b} {v} (pinned)",
        "host": f"error: [{a}-{b}] ssh {v}: connection timed out",
        "ticket": f"error: [{a}-{b}] {d} again (tracked in {v})",
        "date": f"error: [{a}-{b}] {d}",
    }[kind]


def central_text(kind: str, a: str, b: str, d: str, v: str, rng: Rng) -> str:
    lead = {
        "port": f"Today's main thing is the {a} {b}: it moves to port {v}.",
        "path": f"Today's main thing is the {a} {b}: its {d} moves to {v}.",
        "version": f"Today's main thing is {a}'s {b} pin: {a} now pins {b} at {v}.",
        "host": f"Today's main thing is the {a} {b} box: it's now {v}.",
        "ticket": f"Today's main thing is the {a} {b} {d}, tracked as {v}.",
        "date": f"Today's main thing: we just hit a {d} on the {a} {b}.",
    }[kind]
    ask = rng.pick(
        (
            "What should we check before we rely on it? A short list in your reply is enough.",
            "Talk me through the risks in a few lines; no files to change.",
            "What could go wrong with that? Answer in a few lines.",
        )
    )
    return f"{lead} {ask}"


class Workspace:
    """The starting files, and the generated filler work over them."""

    def __init__(self, rng: Rng, proj: str):
        self.rng = rng
        self.files: dict[str, dict] = {}
        self.proj = proj
        self.src: list[tuple[str, str, int]] = []  # (path, function, lines)
        self.docs: list[str] = []
        self._base()

    def put(self, path: str, content: str, executable: bool = False) -> None:
        self.files[path] = {"content": content, "executable": executable}

    def _base(self) -> None:
        r = self.rng
        self.put(
            "README.md",
            f"# {self.proj} tools\n\nA small monorepo of service helpers. `scripts/` holds the checks and syncs the "
            "team runs; `src/` the shared Python; `docs/` the notes. Write scratch output under `notes/`.\n",
        )
        self.put("notes/.keep", "")
        for w in SOURCES:
            fn = f"{w}_{r.pick(('total', 'merge', 'split', 'count', 'scale'))}"
            n = 20 + r.below(60)
            body = [f'"""{w}: helpers."""', "", f"def {fn}(xs):", f"    return sum(xs) * {2 + r.below(9)}", ""]
            for i in range(n):
                body.append(f"def _{w}_{i}(x):\n    return x + {r.below(100)}\n")
            self.put(f"src/{w}.py", "\n".join(body))
            self.src.append((f"src/{w}.py", fn, len("\n".join(body).splitlines())))
        for d in ("runbook", "oncall", "release", "style"):
            paras = []
            for i in range(4 + r.below(4)):
                paras.append(
                    f"## {d} {i + 1}\n\nStep {i + 1} of the {d}: keep the change small, run the checks, "
                    f"and note the result. Allow {5 + r.below(50)} minutes."
                )
            self.put(f"docs/{d}.md", f"# {d}\n\n" + "\n\n".join(paras) + "\n")
            self.docs.append(d)
        self.put("scripts/lint.sh", sh(f"lint: {3 + r.below(20)} files checked\nlint: {r.below(9)} warnings\n"), True)
        self.put("scripts/test.sh", sh(f"tests: {40 + r.below(200)} passed, 0 failed\n"), True)

    def filler(self) -> tuple[str, int]:
        """A small task over the workspace, and the bytes its work reads."""
        r = self.rng
        k = r.below(6)
        if k == 0:
            return "Run ./scripts/lint.sh and tell me how many warnings it reports.", 120
        if k == 1:
            return "Run ./scripts/test.sh: did everything pass?", 80
        if k == 2:
            p, _, _ = r.pick(self.src)
            return f"How many lines are in {p}?", len(self.files[p]["content"])
        if k == 3:
            d = r.pick(self.docs)
            return f"Summarize docs/{d}.md in two sentences.", len(self.files[f"docs/{d}.md"]["content"])
        if k == 4:
            return "Which file under src/ is the largest?", 400
        p, fn, _ = r.pick(self.src)
        return f"What does {fn} in {p} return? One line.", len(self.files[p]["content"])


def sh(output: str, code: int = 0) -> str:
    return f"#!/bin/sh\ncat <<'EOF'\n{output}EOF\nexit {code}\n"


def noise(rng: Rng, n: int) -> list[str]:
    out = []
    for i in range(n):
        k = rng.below(4)
        if k == 0:
            out.append(f"step {i + 1}/{n}: ok")
        elif k == 1:
            out.append(f"build: ok ({3 + rng.below(30)} targets)")
        elif k == 2:
            out.append(f"tests: {10 + rng.below(90)} passed, 0 failed")
        else:
            out.append(f"warnings: {rng.below(5)}")
    return out


def bulk_log(rng: Rng, nbytes: int) -> tuple[str, str]:
    """A long build log, and its slowest step."""
    lines, size, worst, worst_ms, i = [], 0, "", -1, 0
    while size < nbytes:
        name = f"{rng.pick(SOURCES)}-{rng.pick(('compile', 'link', 'pack', 'scan', 'sign'))}-{i:04d}"
        ms = 10 + rng.below(4000)
        line = f"step {i:05d} {name} {ms} ms"
        if ms > worst_ms:
            worst, worst_ms = name, ms
        lines.append(line)
        size += len(line) + 1
        i += 1
    return "\n".join(lines) + "\n", worst


def build(seed: int, size: str) -> Progression:
    b = Builder(seed, size)
    lay, rng = b.lay, b.rng
    # 1. The probes, cell by cell, in a seeded order.
    if size == "smoke":
        plan = list(SMOKE_CELLS)
    else:
        plan = [c for c in pg.expected_cells() for _ in range(FULL_PER_CELL)]
    rng.shuffle(plan)
    # The hardest to fit first: supersessions take three turns, and the
    # session buckets' turns are few.
    order = {"supersession": 0, "session": 1, "days": 2, "compaction": 3, "near": 4, "topic_shift": 5}
    plan.sort(key=lambda c: order[c[0]])
    for cell in plan:
        if not b.place(*cell):
            raise RuntimeError(f"seed {seed}: no room for a {cell} probe")
    # 2. The words of every turn, and the workspace.
    proj = b.topics[0][0]
    ws = Workspace(rng, proj)
    turns: list[Turn] = []
    sessions = [
        Session(label=f"session {i + 1}", date=d.isoformat(), weekday=d.strftime("%A"))
        for i, d in enumerate(lay.dates)
    ]
    scripts: set[str] = set()
    pending: dict[int, list[Edit]] = {}
    bulk_n = 0
    window, bulk_tokens = _window(b, ws)
    for t in range(lay.n):
        s, blk = lay.session(t), lay.block(t)
        ta, tb = b.topics[blk]
        topic = f"the {ta} {tb}"
        role = lay.role[t]
        before = pending.pop(t, [])
        mark = False
        if role in ("opener", "filler", "free"):
            text, nbytes = ws.filler()
            role = "filler" if role == "free" else role
        elif role == "bulk":
            bulk_n += 1
            log, _ = bulk_log(rng, bulk_tokens * 4)
            path = f"logs/build-{bulk_n}.log"
            ws.put(path, log)
            text, nbytes, mark = f"Read {path} and tell me which step took longest.", len(log), True
        elif role == "fact":
            fact = b.slot[t][1]
            text, nbytes, edits = _fact_turn(b, ws, fact, scripts, rng)
            if edits:
                pending.setdefault(t + 1, []).extend(edits)
        else:
            probe = b.slot[t][1]
            text, nbytes = probe.text, 200
        if role == "opener":
            d = sessions[s]
            text = f"It's {d.weekday}, {d.date}. {text}"
        est = (len(text) + nbytes) // 4 + 300
        turns.append(Turn(index=t, session=s, block=blk, topic=topic, text=text, role=role,
                          est_tokens=est, before=before, mark=mark))
    for f in b.facts:
        del f._ab  # type: ignore[attr-defined]
    keys = {p.id: p._keys for p in b.probes}  # type: ignore[attr-defined]
    for p in b.probes:
        del p._keys  # type: ignore[attr-defined]
    names = sorted({w for lst in NAME_LISTS.values() for w in lst if _used(w, turns, ws.files)})
    prog = Progression(
        format=pg.FORMAT,
        seed=seed,
        size=size,
        sessions=sessions,
        turns=turns,
        facts=b.facts,
        probes=b.probes,
        workspace=dict(sorted(ws.files.items())),
        context_window=window,
        names=names,
    )
    pg.validate(prog)
    _never_said(prog, keys)
    return prog


def _used(w: str, turns: list[Turn], files: dict) -> bool:
    if any(w in t.text.lower() for t in turns):
        return True
    return any(w in p.lower() or w in f["content"].lower() for p, f in files.items())


def _never_said(prog: Progression, keys: dict[str, list[str]]) -> None:
    """An abstention's subject is in no turn but its own, and in no file the
    workspace ever holds."""
    texts = [(f"turn {t.index}", t.text.lower(), t.index) for t in prog.turns]
    texts += [(f"file {p}", f["content"].lower(), -1) for p, f in prog.workspace.items()]
    texts += [(f"file {e.path}", (e.content or "").lower(), -1) for t in prog.turns for e in t.before]
    for p in prog.probes:
        for k in keys.get(p.id, []):
            for where, text, idx in texts:
                if idx != p.turn and k.lower() in text:
                    raise pg.Invalid(f"{p.id}: its subject's {k!r} is said in {where}")


def _fact_turn(b: Builder, ws: Workspace, fact: Fact, scripts: set[str], rng: Rng):
    a, bb = fact._ab  # type: ignore[attr-defined]
    d = b._detail(fact.kind, a, bb)
    v = fact.value
    filler, nbytes = ws.filler()
    edits: list[Edit] = []
    if fact.carrier in ("output", "error"):
        verbs = OUT_VERBS if fact.carrier == "output" else ERR_VERBS
        for _ in range(100):
            path = f"scripts/{rng.pick(verbs)}-{a}-{bb}.sh"
            if path not in scripts:
                break
        scripts.add(path)
        lines = noise(rng, 4 + rng.below(5))
        if fact.carrier == "output":
            at = rng.below(len(lines) + 1)
            line = output_line(fact.kind, a, bb, d, v)
            shown = lines[:at] + [line] + lines[at:]
            ws.put(path, sh("\n".join(shown) + "\n"), True)
            # The log rotates before the next turn: the value is gone.
            edits.append(Edit(path, sh("\n".join(lines) + "\n"), True))
            q = rng.pick(("did the build pass?", "are the tests green?", "how many warnings are there?"))
            text = f"Run ./{path}: {q}"
        else:
            line = error_line(fact.kind, a, bb, d, v)
            ws.put(path, sh("\n".join(lines) + "\n" + line + "\n", 1), True)
            # Someone fixes it before the next turn: it now succeeds, and says
            # nothing of the value.
            edits.append(Edit(path, sh("\n".join(lines) + "\nok\n"), True))
            text = f"Run ./{path} and tell me whether it worked."
        fact.source = path
        fact.marker = line if fact.kind == "date" else v
        return text, len(ws.files[path]["content"]), edits
    if fact.carrier == "topic":
        text = central_text(fact.kind, a, bb, d, v, rng)
    elif fact.carrier == "aside":
        text = f"{filler} (Unrelated, but {said_statement(fact.kind, a, bb, d, v)}.)"
    else:
        who = rng.pick(PEOPLE).capitalize()
        text = f"Side note: {who} decided in standup that {said_statement(fact.kind, a, bb, d, v)}. Anyway: {filler}"
    if fact.kind == "date":
        fact.marker = said_statement("date", a, bb, d, v)
    return text, nbytes, edits


def _window(b: Builder, ws: Workspace) -> tuple[int, int]:
    """The scratch context window that brings each arm's compaction near the
    marks, and the bulk read's tokens: each mark's bulk read must cross the
    window, and the rest of its session must not."""
    lay = b.lay
    per_turn = 650  # a guess at a turn's tokens (text, a tool's output, the reply)
    befores = [(m - lay.session(m) * lay.session_turns) * per_turn for m in lay.marks]
    afters = [((lay.session(m) + 1) * lay.session_turns - m - 1) * per_turn for m in lay.marks]
    if lay.size == "smoke":
        window = SMOKE_WINDOW
    else:
        window = OVERHEAD_TOKENS + max(befores) + max(afters) + 4000
    bulk = max(4000, window - OVERHEAD_TOKENS - min(befores) + 2000)
    return window, bulk


# ---- the budget


def estimate(prog: Progression, model: str) -> dict:
    """The tokens each arm is expected to read, and their dollars at the
    catalog's price: every model call re-reads the context from the cache,
    writes what is new, and answers; a compaction at each mark summarizes.
    An estimate for the budget, not a measurement."""
    m = model.split("/", 1)[-1]
    if m not in PRICES:
        raise SystemExit(f"no price for {m!r}: the catalog's models are {', '.join(sorted(PRICES))}")
    pin, pout, pread, pwrite = (x / 1e6 for x in PRICES[m])
    out_tokens = 250
    usd = read = written = 0.0
    ctx = OVERHEAD_TOKENS
    session = -1
    for t in prog.turns:
        if t.session != session:
            session, ctx = t.session, OVERHEAD_TOKENS
            usd += OVERHEAD_TOKENS * pwrite
        calls = 1 if t.role == "probe" else 2
        new = t.est_tokens
        usd += calls * ctx * pread + new * pwrite + calls * out_tokens * pout
        read += calls * ctx
        written += new
        ctx += new
        if t.mark:
            usd += ctx * pin + 2000 * pout  # the summary's call
            ctx = OVERHEAD_TOKENS + 2000
    return {"model": m, "read_tokens": int(read), "new_tokens": int(written), "usd_per_arm": round(usd, 2)}


def summary(prog: Progression, model: str) -> str:
    cells = pg.cells(prog)
    roles: dict[str, int] = {}
    for t in prog.turns:
        roles[t.role] = roles.get(t.role, 0) + 1
    e = estimate(prog, model)
    lines = [
        f"progression {prog.size} seed {prog.seed}: digest {prog.digest()[:16]}",
        f"sessions: " + ", ".join(f"{s.label} ({s.weekday} {s.date}, {len(prog.session_turns(i))} turns)"
                                  for i, s in enumerate(prog.sessions)),
        f"turns: {len(prog.turns)} (" + ", ".join(f"{k} {v}" for k, v in sorted(roles.items())) + ")",
        f"compaction marks: turns {', '.join(map(str, prog.marks()))}; scratch context window {prog.context_window}",
        f"facts: {len(prog.facts)} (" + ", ".join(
            f"{fam} {sum(1 for f in prog.facts if f.family == fam)}" for fam in pg.FAMILIES[:-1])
        + f"; and needs_nothing, the abstentions' subjects, {sum(1 for p in prog.probes if p.kind == 'abstention')}); "
        + ", ".join(f"{s} {sum(1 for f in prog.facts if f.salience == s)}" for s in pg.SALIENCES),
        f"probes: {len(prog.probes)} (" + ", ".join(
            f"{k} {sum(1 for p in prog.probes if p.kind == k)}" for k in pg.PROBE_KINDS) + ")",
        "cells (bucket salience kind: probes):",
    ]
    for c in sorted(cells, key=lambda c: (pg.BUCKETS.index(c[0]), c[1], c[2])):
        lines.append(f"  {c[0]:<12} {c[1]:<10} {c[2]:<10} {cells[c]}")
    lines.append(
        f"estimate per arm at {e['model']}'s catalog price: {e['read_tokens']:,} tokens read from cache, "
        f"{e['new_tokens']:,} new, ${e['usd_per_arm']:.2f} (two arms: ${2 * e['usd_per_arm']:.2f})"
    )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--size", choices=("smoke", "full"), required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--model", default="anthropic/claude-sonnet-5-5", help="for the budget's prices")
    a = ap.parse_args(argv)
    prog = build(a.seed, a.size)
    if a.out.exists() and any(a.out.iterdir()):
        print(f"generate: {a.out} is not empty", file=sys.stderr)
        return 2
    prog.save(a.out)
    pg.materialize(prog, a.out / "workspace")
    print(summary(prog, a.model))
    return 0


if __name__ == "__main__":
    sys.exit(main())
