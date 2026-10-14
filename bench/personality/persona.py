"""The persona and card under test, and the templating that parametrizes every probe by them.

Nothing in the exam hard-codes a character. A persona is a TOML file (`persona.example.toml`): the name, what to
call the owner, the owner's time zone and the words that name it, a few facts of the owner's card, and optionally
the preferences' words. `persona_file` and `card_file` name the owner's own files; with none, the persona and the
card are rendered from the fields (a neutral default, so an install with no persona file can still be examined).
The exam compiles the SAME text into every arm: Theseus's `[personas.<n>] files`, Claude Code's CLAUDE.md.

A probe's text is a template: `<<persona>>`, `<<owner>>`, `<<tz>>`, `<<tznames>>` and `<<other>>` (the names of
the other personas under test), and a card fact's or preference's own fields. `fill` replaces them.

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULTS = HERE / "defaults.toml"

PREF_KINDS = ("brevity", "format", "address", "signoff", "clock")


@dataclass
class Pref:
    """A preference of one type: stated once in conversation (`say`), then asked about (`asks`) in each context."""

    kind: str
    say: str
    asks: list[str]
    checks: str
    oracle: str
    wrong: str


@dataclass
class Fact:
    """One fact of the owner's card, and a question that wants it used unasked."""

    key: str
    statement: str  # a line of the card
    ask: str
    checks: str
    oracle: str
    wrong: str


@dataclass
class Persona:
    name: str
    owner_call: str
    tz: str
    tz_names: list[str]
    voice: str
    facts: list[Fact]
    prefs: list[Pref]
    others: list[str] = field(default_factory=list)
    persona_file: Path | None = None
    card_file: Path | None = None
    source: str = "built-in"

    def persona_text(self) -> str:
        if self.persona_file:
            return self.persona_file.read_text()
        return (f"# {self.name}\n\nYour name is {self.name}. {self.voice}\n"
                f"Speak as {self.name} in every conversation, on every surface.\n")

    def card_text(self) -> str:
        if self.card_file:
            return self.card_file.read_text()
        lines = [f"# The owner's card", "", f"- Call the owner {self.owner_call}.",
                 f"- {self.owner_call}'s time zone is {self.tz}; read and give times in it."]
        lines += [f"- {f.statement}" for f in self.facts]
        return "\n".join(lines) + "\n"

    def digest(self) -> str:
        """What the report names: the persona and the card as the arms read them, and the fields the probes use."""
        h = hashlib.sha256()
        for part in (self.persona_text(), self.card_text(),
                     json.dumps(self.fields(), sort_keys=True)):
            h.update(part.encode() + b"\0")
        return h.hexdigest()[:16]

    def fields(self) -> dict:
        return {"name": self.name, "owner_call": self.owner_call, "tz": self.tz, "tz_names": self.tz_names,
                "facts": [f.key for f in self.facts], "prefs": [p.kind for p in self.prefs], "others": self.others}

    def subs(self) -> dict[str, str]:
        return {"persona": self.name, "owner": self.owner_call, "tz": self.tz,
                "tznames": " or ".join(self.tz_names), "tzname": self.tz_names[0], "other": self.others[0] if self.others else "",
                "others": ", ".join(self.others)}

    def fill(self, s: str) -> str:
        for k, v in self.subs().items():
            s = s.replace(f"<<{k}>>", v)
        return s

    def check_fill(self, s: str) -> str:
        """A check line's template: the same fields, quoted for a check string."""
        return self.fill(s)


def _pref(d: dict) -> Pref:
    for k in ("kind", "say", "asks", "checks", "oracle", "wrong"):
        if k not in d:
            raise ValueError(f"a preference needs {k!r}: {d}")
    return Pref(d["kind"], d["say"], list(d["asks"]), d["checks"], d["oracle"], d["wrong"])


def _fact(d: dict) -> Fact:
    for k in ("key", "statement", "ask", "checks", "oracle", "wrong"):
        if k not in d:
            raise ValueError(f"a card fact needs {k!r}: {d}")
    return Fact(d["key"], d["statement"], d["ask"], d["checks"], d["oracle"], d["wrong"])


def defaults() -> dict:
    return tomllib.loads(DEFAULTS.read_text())


def load(path: Path | None, others: list[str] | None = None) -> Persona:
    """A persona from its TOML, over the defaults (`defaults.toml`): a persona's own `[[pref]]` of a kind replaces
    the default's, and its `[[fact]]`s replace the defaults' list when it gives any."""
    base = defaults()
    t = tomllib.loads(Path(path).read_text()) if path else {}
    root = Path(path).resolve().parent if path else HERE
    src = str(path) if path else "built-in"
    merged = {**base["persona"], **t.get("persona", {}), **{k: v for k, v in t.items() if not isinstance(v, (dict, list))}}
    prefs = {p["kind"]: p for p in base["pref"]}
    for p in t.get("pref", []):
        prefs[p["kind"]] = {**prefs.get(p["kind"], {}), **p}
    facts = t.get("fact") or base["fact"]

    def file(key: str) -> Path | None:
        v = merged.get(key)
        if not v:
            return None
        p = Path(v).expanduser()
        p = p if p.is_absolute() else root / p
        if not p.is_file():
            raise ValueError(f"{key} {v!r} is not a file (looked at {p})")
        return p

    per = Persona(
        name=merged["name"], owner_call=merged["owner_call"], tz=merged["tz"], tz_names=list(merged["tz_names"]),
        voice=merged["voice"], facts=[_fact(f) for f in facts], prefs=[_pref(p) for p in prefs.values()],
        others=list(others or merged.get("others", [])), persona_file=file("persona_file"),
        card_file=file("card_file"), source=src)
    validate(per)
    return per


def validate(p: Persona) -> None:
    """A persona the exam can run: names that are single words (a check's `word` match), the five preference types, a
    time zone with names, and no template left unfilled in anything a probe will say."""
    if not p.name.strip() or not p.owner_call.strip():
        raise ValueError("a persona needs a name and a name for the owner")
    if not p.tz_names:
        raise ValueError("a persona's tz_names must name its time zone in the words a reply would use")
    kinds = [x.kind for x in p.prefs]
    if sorted(kinds) != sorted(set(kinds)):
        raise ValueError(f"a preference kind twice: {kinds}")
    for kind in PREF_KINDS:
        if kind not in kinds:
            raise ValueError(f"no preference of kind {kind!r} (the exam tests {', '.join(PREF_KINDS)})")
    for x in p.prefs:
        if len(x.asks) < 6:
            raise ValueError(f"preference {x.kind!r} needs 6 asks, one for each context (it has {len(x.asks)})")
    if not p.facts:
        raise ValueError("a persona needs at least one card fact")
    for t in [x.say for x in p.prefs] + [f.ask for f in p.facts]:
        if "<<" in p.fill(t):
            raise ValueError(f"an unfilled template in {t!r}")
