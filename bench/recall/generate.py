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
import dataclasses
import datetime as dt
import itertools
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import checks  # noqa: E402
import progression as pg  # noqa: E402
import tokens as tk  # noqa: E402
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
# The system prompt and tools a plan is sized at are the daemon's own, measured
# at run time (`drive.py`: the first compile of a throwaway daemon's first call,
# less its user message), never a constant here: every tool definition moves
# it (60b43fb6: 13,528; c4f79e9f: 13,599; a build of 2026-10-09, 13,943), and
# a constant that main outgrew refused every run (theseus-cs8k). `build` takes
# it, the progression records it, and a pinned plan (`--overhead`) is held to
# the cushion below.
# How far a daemon's measured overhead may be from the one a progression
# was planned at, over or under, before the driver refuses to run it
# (`drive.py`, unless `--allow-overhead`). The generator checks each plan's
# written bounds at its overhead, at this much more and at this much less
# (`plan_misses`), so within it the marks still hold, and past it a mark may
# ring or fail. Over, a larger prompt than planned leaves a mark's summary
# no room: the smoke planned at 13,528 rang at a real 13,599 (71 more), so
# it is under that. Under, the reads may not cross the budget at all
# (theseus-tqa3: smoke seed 12 crosses by 31 tokens at a daemon 101 under
# the default plan, and not at 150 under it).
OVERHEAD_CUSHION = 50
# The rates the plan estimates at: Claude's current family, the bench's
# models (catalog.rs, `TokenRates::CLAUDE`).
PLAN_RATES = tk.RATES["CLAUDE"]
REPLY_BYTES = 400  # a guess at a reply's text, after a turn's work
MARGIN = 0.15  # how far the turns' estimate may be off and a mark still compact
# How far under the request budget a bulk read's turn stays alone, beside
# the system prompt and tools, at the estimate's upper bound: the ring's
# last candidate, which past the budget fails the turn as an overage.
ALONE_MARGIN = 0.10
BULK_MIN_TOKENS = 1000  # a bulk read is at least this long

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

    def run(self, path: str) -> Work:
        return Work("proc_run", {"argv": [f"./{path}"]}, len(self.files[path]["content"]))

    def read(self, path: str) -> Work:
        return Work("fs_read", {"path": path}, tk.fs_read_bytes(self.files[path]["content"]))

    def filler(self) -> tuple[str, "Work"]:
        """A small task over the workspace, and the work a model does for it."""
        r = self.rng
        k = r.below(6)
        if k == 0:
            return "Run ./scripts/lint.sh and tell me how many warnings it reports.", self.run("scripts/lint.sh")
        if k == 1:
            return "Run ./scripts/test.sh: did everything pass?", self.run("scripts/test.sh")
        if k == 2:
            p, _, _ = r.pick(self.src)
            return f"How many lines are in {p}?", self.read(p)
        if k == 3:
            d = r.pick(self.docs)
            return f"Summarize docs/{d}.md in two sentences.", self.read(f"docs/{d}.md")
        if k == 4:
            return "Which file under src/ is the largest?", Work("fs_list", {"path": "src"}, 400)
        p, fn, _ = r.pick(self.src)
        return f"What does {fn} in {p} return? One line.", self.read(p)


class Work:
    """A turn's tool call, as a model makes it, and the bytes of its result
    (as the tool returns them: `fs_read` numbers its lines)."""

    def __init__(self, tool: str, args: dict, nbytes: int):
        self.tool, self.args, self.nbytes = tool, args, nbytes


def turn_census(text: str, work: "Work | list[Work] | None") -> tk.Census:
    """A turn's messages as the compiler counts them: the user's text, each
    work's call and its result, and a reply of REPLY_BYTES."""
    c = tk.user_text(text)
    for w in [] if work is None else work if isinstance(work, list) else [work]:
        c = c + tk.call(w.tool, w.args) + tk.result(w.nbytes)
    return c + tk.Census(messages=1, blocks=1, text=REPLY_BYTES)


def turn_tokens(text: str, work: "Work | list[Work] | None") -> int:
    return turn_census(text, work).tokens(PLAN_RATES)


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
    """A long build log whose `fs_read` comes to `nbytes` at most (each line
    numbered), and its slowest step."""
    lines, size, worst, worst_ms, i = [], 0, "", -1, 0
    while True:
        name = f"{rng.pick(SOURCES)}-{rng.pick(('compile', 'link', 'pack', 'scan', 'sign'))}-{i:04d}"
        ms = 10 + rng.below(4000)
        line = f"step {i:05d} {name} {ms} ms"
        if size + tk.FS_READ_PREFIX + len(line) + 1 > nbytes:
            break
        if ms > worst_ms:
            worst, worst_ms = name, ms
        lines.append(line)
        size += tk.FS_READ_PREFIX + len(line) + 1
        i += 1
    return "\n".join(lines) + "\n", worst


def bulk_rng(seed: int, n: int) -> Rng:
    """The bulk logs' own stream: a bulk resized moves no other turn."""
    return Rng((seed * 0x9E3779B97F4A7C15 + 0xB0C5 * (n + 1)) & MASK)


def build(seed: int, size: str, overhead: int) -> Progression:
    """The progression of `seed` and `size`, its window and bulks planned at
    a system prompt and tools of `overhead` tokens, which it records."""
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
    for t in range(lay.n):
        s, blk = lay.session(t), lay.block(t)
        ta, tb = b.topics[blk]
        topic = f"the {ta} {tb}"
        role = lay.role[t]
        before = pending.pop(t, [])
        mark = False
        if role in ("opener", "filler", "free"):
            text, work = ws.filler()
            role = "filler" if role == "free" else role
        elif role == "bulk":
            # Its log is written once every turn is: its size is the plan's.
            text, work, mark = f"Read {bulk_path(lay.marks.index(t))} and tell me which step took longest.", None, True
        elif role == "fact":
            fact = b.slot[t][1]
            text, work, edits = _fact_turn(b, ws, fact, scripts, rng)
            if edits:
                pending.setdefault(t + 1, []).extend(edits)
        else:
            probe = b.slot[t][1]
            text = probe.text
            work = Work("fs_write", {"path": probe.file, "content": "x" * 60}, 80) if probe.file else None
        if role == "opener":
            d = sessions[s]
            text = f"It's {d.weekday}, {d.date}. {text}"
        turns.append(Turn(index=t, session=s, block=blk, topic=topic, text=text, role=role,
                          est_tokens=turn_tokens(text, work), before=before, mark=mark))
    for f in b.facts:
        del f._ab  # type: ignore[attr-defined]
    keys = {p.id: p._keys for p in b.probes}  # type: ignore[attr-defined]
    for p in b.probes:
        del p._keys  # type: ignore[attr-defined]
    # 3. The bulks: each plan written, its bounds worked again from the
    # written bytes, and the first that holds kept. A plan is in tokens and
    # its logs in bytes, so a plan at a thin margin can miss once written.
    files = dict(ws.files)
    for window, plans in plan_bulks(turns, lay.marks, overhead):
        trial = [dataclasses.replace(t) for t in turns]
        ws.files = dict(files)
        for i, (m, plan) in enumerate(zip(lay.marks, plans)):
            _put_bulks(seed, i, m, plan, trial, ws)
        names = sorted({w for lst in NAME_LISTS.values() for w in lst if _used(w, trial, ws.files)})
        prog = Progression(
            format=pg.FORMAT,
            seed=seed,
            size=size,
            sessions=sessions,
            turns=trial,
            facts=b.facts,
            probes=b.probes,
            workspace=dict(sorted(ws.files.items())),
            context_window=window,
            names=names,
            overhead_tokens=overhead,
        )
        if not plan_misses(prog):
            break
    else:
        raise RuntimeError(f"seed {seed}: no window's written bulks hold the marks")
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
    filler, work = ws.filler()
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
        return text, ws.run(path), edits
    if fact.carrier == "topic":
        text = central_text(fact.kind, a, bb, d, v, rng)
        work = None
    elif fact.carrier == "aside":
        text = f"{filler} (Unrelated, but {said_statement(fact.kind, a, bb, d, v)}.)"
    else:
        who = rng.pick(PEOPLE).capitalize()
        text = f"Side note: {who} decided in standup that {said_statement(fact.kind, a, bb, d, v)}. Anyway: {filler}"
    if fact.kind == "date":
        fact.marker = said_statement("date", a, bb, d, v)
    return text, work, edits


def bulk_path(i: int, k: int = 0) -> str:
    """The mark's own log (k 0), and the turn after's (k 1, 2, …: `b`, `c`)."""
    return f"logs/build-{i + 1}{'' if k == 0 else chr(ord('a') + k)}.log"


def second_text(paths: list[str]) -> str:
    """The turn after a mark, where it reads more logs."""
    if len(paths) == 1:
        return f"Now read {paths[0]} too: which of its steps took longest?"
    return f"Now read {', '.join(paths[:-1])} and {paths[-1]} too: which of their steps took longest?"


MORE_READS = 3  # the most logs the turn after a mark reads


class Bulk:
    """A mark's plan: each log's result tokens, the mark's own first and the
    turn after's (where the mark's own read can't cross the budget), and the
    estimate of the session's turns before the mark."""

    def __init__(self, reads: list[int], before: int):
        self.reads, self.before = reads, before

    def __repr__(self) -> str:
        return f"Bulk(reads={self.reads}, before={self.before})"


def result_tokens(r: int) -> int:
    """A tool result message of `r` tokens of content, framed."""
    return r + tk.MESSAGE_TOKENS + tk.BLOCK_TOKENS + tk.ID_TOKENS


def reply_tokens() -> int:
    return tk.Census(messages=1, blocks=1, text=REPLY_BYTES).tokens(PLAN_RATES)


def _read_turn(text: str, paths: list[str]) -> tuple[int, int]:
    """A read turn's user message and its calls, in tokens."""
    calls = sum(tk.call("fs_read", {"path": p}).tokens(PLAN_RATES) for p in paths)
    return tk.user_text(text).tokens(PLAN_RATES), calls


def mark_turns(i: int, mark_text: str, n: int) -> list[tuple[int, int]]:
    """(user message, calls) in tokens: the mark's turn, and with `n` more
    logs, the turn after's."""
    out = [_read_turn(mark_text, [bulk_path(i)])]
    if n:
        paths = [bulk_path(i, k) for k in range(1, n + 1)]
        out.append(_read_turn(second_text(paths), paths))
    return out


def cross_bound(before: int, turns: list[tuple[int, int]], n: int, r: int, overhead: int) -> int:
    """The upper bound at the last read's answer, the reads taken one at a
    time (the slower crossing): everything before that read's result was
    counted by the provider, and only that result is estimated."""
    u1, c1 = turns[0]
    counted = overhead + before + u1 + c1
    if n:
        u2, c2 = turns[1]
        counted += result_tokens(r) + reply_tokens() + u2 + c2 + (n - 1) * result_tokens(r)
    return tk.upper(counted, result_tokens(r))


def alone_limit(budget: int) -> int:
    """The most a read turn's ring candidate may be at its upper bound:
    `ALONE_MARGIN` under the budget, and room beside it for the compaction's
    summary (`SUMMARY_MAX_TOKENS`), or the ring keeps its cut unsummarized."""
    return min(int(budget / (1 + ALONE_MARGIN)), budget - tk.SUMMARY_MAX_TOKENS)


def fit_bound(before: int, last: int, overhead: int) -> int:
    """The upper bound of the last turn before a mark: what came before it
    counted, its own new part estimated."""
    return tk.upper(overhead + before - last, last)


def plan_bulks(turns: list[Turn], marks: list[int], overhead: int):
    """The scratch context windows, and each mark's bulk reads, by the
    compiler's rule (`tokens.py`), as candidates, the smallest window first
    and in it the fewest reads first: `build` keeps the first whose written
    logs hold (`plan_misses`). Theseus rings when a request's upper bound
    passes the budget (the window less the output cap and 4,096), and a
    turn whose newest exchange alone passes it fails. So, in each window (in
    thousands) where it holds for every mark:

    - the turns before a mark fit, at `MARGIN` over their estimate, and so
      does a session with no mark, whole;
    - each read turn alone, estimated whole beside the system prompt and
      tools, stays `ALONE_MARGIN` under the budget at its upper bound, with
      room beside it for a compaction's summary (`alone_limit`);
    - at `MARGIN` under the turns' estimate, the reads cross the budget:
      what came before counted, the last result at the bound. Where the
      mark's one read can't do both (a short session before it, or a window
      the longest session sets), the turn after the mark, which holds
      nothing, reads up to `MORE_READS` more logs, and the crossing is
      there, the mark's read counted whole by then.

    Every log is the same size, the middle of what its bounds allow, and a
    tool result is capped at `RESULT_MAX_CHARS`."""
    cap = int(tk.RESULT_MAX_CHARS // PLAN_RATES[0]) - 1
    marks_in = []
    for i, m in enumerate(marks):
        s = turns[m].session
        prior = [t for t in turns if t.session == s and t.index < m]
        marks_in.append((i, turns[m].text, sum(t.est_tokens for t in prior), prior[-1].est_tokens if prior else 0))
    # A session with no mark fits whole: it is not meant to compact.
    unmarked = []
    for s in sorted({t.session for t in turns} - {turns[m].session for m in marks}):
        own = [t for t in turns if t.session == s]
        unmarked.append(fit_bound(int(sum(t.est_tokens for t in own) * (1 + MARGIN)),
                                  int(own[-1].est_tokens * (1 + MARGIN)), overhead))
    window = 0
    while window < 2_000_000:
        window += 1000
        budget = pg.request_budget(window)
        if any(f > budget for f in unmarked):
            continue
        options = []
        for i, text, before, last in marks_in:
            if fit_bound(int(before * (1 + MARGIN)), int(last * (1 + MARGIN)), overhead) > budget:
                break
            lo = int(before * (1 - MARGIN))
            fits = []
            for n in range(MORE_READS + 1):
                rt = mark_turns(i, text, n)
                # A few tokens' slack: the plan rounds each message, the census the whole.
                top = min(cap, _largest(lambda r: max(_alone(rt, n, r, overhead)), alone_limit(budget) - 8))
                need = max(BULK_MIN_TOKENS, _smallest(lambda r: cross_bound(lo, rt, n, r, overhead), budget + 1))
                if need <= top:
                    fits.append(Bulk([(need + top) // 2] * (n + 1), before))
            if not fits:
                break
            options.append(fits)
        else:
            for plans in itertools.product(*options):
                yield window, list(plans)


def _alone(rt: list[tuple[int, int]], n: int, r: int, overhead: int) -> list[int]:
    """Each read turn's ring candidate at its answer: the system prompt and
    tools, its user message, its calls and their results (`r` each),
    estimated whole, at the upper bound. Past the budget, an overage."""
    (u1, c1), *rest = rt
    out = [tk.bound(overhead + u1 + c1 + result_tokens(r))]
    for u, c in rest:
        out.append(tk.bound(overhead + u + c + n * result_tokens(r)))
    return out


def _smallest(f, target: float) -> int:
    """The least r ≥ 0 with f(r) ≥ target (f grows with r)."""
    lo, hi = 0, 1
    while f(hi) < target:
        hi *= 2
    while lo < hi:
        mid = (lo + hi) // 2
        if f(mid) >= target:
            hi = mid
        else:
            lo = mid + 1
    return lo


def _largest(f, limit: float) -> int:
    """The greatest r ≥ 0 with f(r) ≤ limit (f grows with r), or -1."""
    if f(0) > limit:
        return -1
    lo, hi = 0, 1
    while f(hi) <= limit:
        lo, hi = hi, hi * 2
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if f(mid) <= limit:
            lo = mid
        else:
            hi = mid
    return lo


def _put_bulks(seed: int, i: int, m: int, plan: Bulk, turns: list[Turn], ws: Workspace) -> None:
    """A mark's logs into the workspace, and its turns' words and
    estimates: the more reads, where there are any, at the turn after."""
    rng = bulk_rng(seed, i)
    works = []
    for k, r in enumerate(plan.reads):
        path = bulk_path(i, k)
        log, _ = bulk_log(rng, int(r * PLAN_RATES[0]))
        ws.put(path, log)
        works.append(Work("fs_read", {"path": path}, tk.fs_read_bytes(log)))
    t = turns[m]
    t.est_tokens = turn_tokens(t.text, works[0])
    if len(works) > 1:
        t = turns[m + 1]
        t.text, t.role = second_text([w.args["path"] for w in works[1:]]), "bulk"
        t.est_tokens = turn_tokens(t.text, works[1:])


# ---- the budget


def planned_overhead(prog: Progression) -> int:
    """The overhead `prog` was planned at: its own record. A file from before
    it was recorded has none, and no constant stands in for it: it is
    generated again at a measured one."""
    if prog.overhead_tokens is None:
        raise ValueError(f"progression {prog.size} seed {prog.seed} records no planned overhead: "
                         "generate it again with --overhead <the daemon's measured overhead>")
    return prog.overhead_tokens


def bounds_of(prog: Progression, overhead: int | None = None) -> list[dict]:
    """Each mark's bounds, from the progression's own bytes (its logs, its
    turns' estimates) by the compiler's rule: the request budget; the turns
    before the mark (`before`, their estimate) and the last one's bound at
    `MARGIN` over it (`fit`); each read turn's bound alone beside the
    system prompt and tools (`alone`, against `alone_limit`); and the bound
    at the last read's answer at `MARGIN` under the turns' estimate
    (`cross`), the turn it falls in (`cross_at`). The system prompt and
    tools are `overhead`, by default the one it was planned at."""
    overhead = planned_overhead(prog) if overhead is None else overhead
    budget = pg.request_budget(prog.context_window)
    out = []
    for i, m in enumerate(prog.marks()):
        s = prog.turns[m].session
        prior = [t for t in prog.turns if t.session == s and t.index < m]
        before = sum(t.est_tokens for t in prior)
        last = prior[-1].est_tokens if prior else 0
        read_turns = [prog.turns[m]] + ([prog.turns[m + 1]] if prog.turns[m + 1].role == "bulk" else [])
        logs, alone, pieces = [], [], []
        for k, t in enumerate(read_turns):
            paths = [bulk_path(i)] if k == 0 else [bulk_path(i, j) for j in range(1, MORE_READS + 1)
                                                   if bulk_path(i, j) in t.text]
            u = tk.user_text(t.text).tokens(PLAN_RATES)
            calls = [tk.call("fs_read", {"path": p}).tokens(PLAN_RATES) for p in paths]
            results = [tk.result(tk.fs_read_bytes(prog.workspace[p]["content"])).tokens(PLAN_RATES) for p in paths]
            logs += [(p, tk.fs_read_bytes(prog.workspace[p]["content"]), r) for p, r in zip(paths, results)]
            whole = tk.user_text(t.text)
            for p in paths:
                whole = whole + tk.call("fs_read", {"path": p}) + tk.result(tk.fs_read_bytes(prog.workspace[p]["content"]))
            alone.append(tk.bound(overhead + whole.tokens(PLAN_RATES)))
            pieces.append((u, calls, results))
        counted = overhead + int(before * (1 - MARGIN))
        for k, (u, calls, results) in enumerate(pieces):
            counted += u + sum(calls) + sum(results)
            if k < len(pieces) - 1:
                counted += reply_tokens()
        cross = tk.upper(counted - results[-1], results[-1])
        out.append({
            "mark": m, "budget": budget, "before": before,
            "fit": fit_bound(int(before * (1 + MARGIN)), int(last * (1 + MARGIN)), overhead),
            "logs": logs, "alone": alone, "alone_limit": alone_limit(budget),
            "cross": cross, "cross_at": read_turns[-1].index,
        })
    return out


def plan_misses(prog: Progression) -> list[str]:
    """Each bound `prog`'s written bytes miss (`bounds_of`), at the overhead
    it was planned at, at `OVERHEAD_CUSHION` more and at that much less, the
    most the driver lets a daemon's differ from it: the turns before a mark, and a session with no
    mark, fit; each read turn alone stays under `alone_limit`; the reads
    cross the budget at the mark or the turn after; and no log passes what
    one tool result shows. None, when the plan holds."""
    planned = planned_overhead(prog)
    budget = pg.request_budget(prog.context_window)
    out = []
    for overhead in (planned, planned + OVERHEAD_CUSHION, planned - OVERHEAD_CUSHION):
        for b in bounds_of(prog, overhead):
            at = f"overhead {overhead:,}, mark {b['mark']}"
            if b["fit"] > budget:
                out.append(f"{at}: the turn before it {b['fit']:,} passes the budget {budget:,}")
            if b["cross"] <= budget or b["cross_at"] not in (b["mark"], b["mark"] + 1):
                out.append(f"{at}: the reads cross at {b['cross']:,} (turn {b['cross_at']}), not over {budget:,}")
            for x in b["alone"]:
                if x > b["alone_limit"]:
                    out.append(f"{at}: a read turn alone {x:,} passes {b['alone_limit']:,}")
            for path, nbytes, _ in b["logs"]:
                if nbytes > tk.RESULT_MAX_CHARS:
                    out.append(f"{at}: {path} is {nbytes:,} bytes, past one result's {tk.RESULT_MAX_CHARS:,}")
        for s in range(len(prog.sessions)):
            own = prog.session_turns(s)
            if not any(t.mark for t in own):
                f = fit_bound(int(sum(t.est_tokens for t in own) * (1 + MARGIN)),
                              int(own[-1].est_tokens * (1 + MARGIN)), overhead)
                if f > budget:
                    out.append(f"overhead {overhead:,}: session {s + 1}, with no mark, {f:,} passes {budget:,}")
    return out


SUMMARY_TOKENS = 2000  # a guess at a compaction summary's length


def estimate(prog: Progression, model: str) -> dict:
    """The tokens each arm is expected to read, and their dollars at the
    catalog's price: every model call re-reads the context from the cache,
    writes what is new, and answers; where a turn's request passes the
    request budget at its upper bound (the compiler's ring), a summary's
    call reads the context and the context starts again from it. An
    estimate for the budget, not a measurement."""
    m = model.split("/", 1)[-1]
    if m not in PRICES:
        raise SystemExit(f"no price for {m!r}: the catalog's models are {', '.join(sorted(PRICES))}")
    pin, pout, pread, pwrite = (x / 1e6 for x in PRICES[m])
    budget = pg.request_budget(prog.context_window)
    overhead = planned_overhead(prog)
    out_tokens = 250
    usd = read = written = 0.0
    ctx = overhead
    session = -1
    compactions = 0
    for t in prog.turns:
        if t.session != session:
            session, ctx = t.session, overhead
            usd += overhead * pwrite
        calls = 1 if t.role == "probe" else 2
        new = t.est_tokens
        if tk.upper(ctx, new) > budget:
            usd += ctx * pin + SUMMARY_TOKENS * pout
            ctx = overhead + SUMMARY_TOKENS
            compactions += 1
        usd += calls * ctx * pread + new * pwrite + calls * out_tokens * pout
        read += calls * ctx
        written += new
        ctx += new
    return {"model": m, "read_tokens": int(read), "new_tokens": int(written), "usd_per_arm": round(usd, 2),
            "compactions": compactions}


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
        f"compaction marks: turns {', '.join(map(str, prog.marks()))}; scratch context window {prog.context_window}, "
        f"a request budget of {pg.request_budget(prog.context_window):,} (the window less its output cap "
        f"{pg.output_cap(prog.context_window):,} and {tk.HEADROOM:,}); the system prompt and tools estimated at "
        f"{planned_overhead(prog):,} (the driver refuses a daemon's more than {OVERHEAD_CUSHION} off it, over or under)",
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
    lines.append("bulk reads (the compiler's estimate; its upper bound is the estimate × 1.4):")
    for b in bounds_of(prog):
        logs = "; ".join(f"{p} {n:,} bytes, {r:,} tokens" for p, n, r in b["logs"])
        lines.append(
            f"  mark {b['mark']}: {logs}. Alone beside the system prompt and tools, bound "
            f"{', '.join(f'{x:,}' for x in b['alone'])} (at most {b['alone_limit']:,}); the {b['before']:,} "
            f"before it at {MARGIN:.0%} less, crossing at turn {b['cross_at']} at {b['cross']:,} (over "
            f"{b['budget']:,}); at {MARGIN:.0%} more, the turn before it {b['fit']:,}"
        )
    lines.append(
        f"estimate per arm at {e['model']}'s catalog price: {e['read_tokens']:,} tokens read from cache, "
        f"{e['new_tokens']:,} new, {e['compactions']} compactions, ${e['usd_per_arm']:.2f} "
        f"(two arms: ${2 * e['usd_per_arm']:.2f})"
    )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--size", choices=("smoke", "full"), required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--model", default="anthropic/claude-sonnet-5-5", help="for the budget's prices")
    ap.add_argument("--overhead", type=int, required=True,
                    help="the system prompt and tools to plan at, in tokens: a daemon's measured overhead "
                         "(`drive.py` measures it and plans at it itself; this pins a plan by hand, and the "
                         "driver then refuses a daemon more than the cushion off it)")
    a = ap.parse_args(argv)
    prog = build(a.seed, a.size, a.overhead)
    if a.out.exists() and any(a.out.iterdir()):
        print(f"generate: {a.out} is not empty", file=sys.stderr)
        return 2
    prog.save(a.out)
    pg.materialize(prog, a.out / "workspace")
    print(summary(prog, a.model))
    return 0


if __name__ == "__main__":
    sys.exit(main())
