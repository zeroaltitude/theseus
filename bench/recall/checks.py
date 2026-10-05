"""The check language, as theseus-exam's `check.rs` has it, for the recall
bench's probes: deterministic assertions on an answer. A check is lines, and
every line must hold. A line is one or more clauses joined by ` or `, and
holds when any clause does:

    # comments and blank lines are skipped
    reply has "23817"                      the reply contains it
    reply lacks "29114" | "31007"          it contains none of them
    reply has word "23817"                 a whole word: no letter or digit beside it
    reply has /\\bport\\b/i                 a regex (flags: i, s, m, x)
    file "notes/p007.txt" has word "23817" a file under the arm's workspace
    file "notes/p007.txt" exists           `exists` and `absent` take no pattern

- Strings compare case-insensitively, after folding typographic quotes and
  dashes to ASCII and collapsing runs of whitespace to one space.
- Regexes run on the folded text, case and whitespace kept. They are
  Python's `re`, not Rust's `regex`: the bench's checks use the subset both
  read alike, and look-arounds, which only Python's has.
- `has a | b` holds when any pattern is found; `lacks a | b` when none is.
- A file path is relative, with no `..`; a file that cannot be read fails
  `has` and passes `lacks`.

`check.rs`'s `calls` and `call <tool>` subjects are left out: the two arms
name their tools differently, so a probe is scored by its reply or by its
effect in the workspace, never by a call. Parsing is strict: an unknown word
is an error that names it.

Standard library only.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

_FOLD = str.maketrans(
    {
        "\u2018": "'",
        "\u2019": "'",
        "\u201a": "'",
        "\u2032": "'",
        "\u201c": '"',
        "\u201d": '"',
        "\u201e": '"',
        "\u2033": '"',
        "\u2010": "-",
        "\u2011": "-",
        "\u2012": "-",
        "\u2013": "-",
        "\u2014": "-",
        "\u2212": "-",
        "\u00a0": " ",
        "\u202f": " ",
    }
)


def fold(s: str) -> str:
    """Typographic quotes, dashes and non-breaking spaces to ASCII."""
    return s.translate(_FOLD)


def normalize(s: str) -> str:
    """Folded, lower-cased, whitespace collapsed: what text patterns compare."""
    return " ".join(fold(s).split()).lower()


def _word_char(c: str) -> bool:
    return c.isalnum() or c == "_"


def has_word(hay: str, needle: str) -> bool:
    """`needle` in `hay` (both normalized) with no word character beside it."""
    if not needle:
        return False
    start = 0
    while True:
        i = hay.find(needle, start)
        if i < 0:
            return False
        j = i + len(needle)
        before = hay[i - 1] if i > 0 else ""
        after = hay[j] if j < len(hay) else ""
        if not (before and _word_char(before)) and not (after and _word_char(after)):
            return True
        start = i + 1


@dataclass(frozen=True)
class Pattern:
    kind: str  # text | word | regex
    value: str
    flags: str = ""

    def found(self, raw: str, norm: str) -> bool:
        if self.kind == "text":
            return self.value in norm
        if self.kind == "word":
            return has_word(norm, self.value)
        return _regex(self.value, self.flags).search(fold(raw)) is not None


def _regex(src: str, flags: str) -> re.Pattern:
    f = 0
    for c in flags:
        f |= {"i": re.I, "s": re.S, "m": re.M, "x": re.X}[c]
    return re.compile(src, f)


@dataclass(frozen=True)
class Clause:
    subject: str  # reply | file
    path: str | None
    verb: str  # has | lacks | exists | absent
    patterns: tuple[Pattern, ...]

    def holds(self, reply: str, root: Path | None) -> bool:
        if self.subject == "file":
            f = (root / self.path) if root is not None else None
            if self.verb == "exists":
                return f is not None and f.is_file()
            if self.verb == "absent":
                return f is None or not f.exists()
            try:
                texts = [f.read_text(errors="replace")] if f is not None else []
            except OSError:
                texts = []
        else:
            texts = [reply]
        found = any(p.found(t, normalize(t)) for t in texts for p in self.patterns)
        return found if self.verb == "has" else not found


@dataclass(frozen=True)
class Line:
    source: str
    clauses: tuple[Clause, ...]


class CheckError(ValueError):
    pass


def _tokens(line: str) -> list[tuple[str, str, str]]:
    """Words, strings, regexes and bars: (kind, text, flags)."""
    out, i, n = [], 0, len(line)
    while i < n:
        c = line[i]
        if c.isspace():
            i += 1
        elif c == "|":
            out.append(("bar", "|", ""))
            i += 1
        elif c == '"':
            i += 1
            s = []
            while True:
                if i >= n:
                    raise CheckError('a string is not closed: add a "')
                c = line[i]
                if c == '"':
                    i += 1
                    break
                if c == "\\":
                    if i + 1 >= n:
                        raise CheckError('a string is not closed: add a "')
                    e = line[i + 1]
                    if e not in '"\\n':
                        raise CheckError(f"unknown escape \\{e} (use \\\", \\\\, or \\n)")
                    s.append("\n" if e == "n" else e)
                    i += 2
                    continue
                s.append(c)
                i += 1
            out.append(("str", "".join(s), ""))
        elif c == "/":
            i += 1
            s = []
            while True:
                if i >= n:
                    raise CheckError("a regex is not closed: add a /")
                c = line[i]
                if c == "/":
                    i += 1
                    break
                if c == "\\" and i + 1 < n:
                    s.append("/" if line[i + 1] == "/" else line[i : i + 2])
                    i += 2
                    continue
                s.append(c)
                i += 1
            flags = []
            while i < n and line[i].isalpha():
                flags.append(line[i])
                i += 1
            out.append(("re", "".join(s), "".join(flags)))
        else:
            j = i
            while j < n and not line[j].isspace() and line[j] not in '"|':
                j += 1
            out.append(("word", line[i:j], ""))
            i = j
    return out


def _clause(toks: list[tuple[str, str, str]]) -> Clause:
    if not toks:
        raise CheckError("an empty clause")
    kind, word, _ = toks[0]
    rest = toks[1:]
    path = None
    if kind == "word" and word == "reply":
        subject = "reply"
    elif kind == "word" and word == "file":
        if not rest or rest[0][0] != "str":
            raise CheckError('`file` takes a quoted path: file "notes/x.txt" has …')
        path = rest[0][1]
        p = PurePosixPath(path)
        if p.is_absolute() or ".." in p.parts or not path:
            raise CheckError(f"a file path is relative, with no ..: {path!r}")
        subject = "file"
        rest = rest[1:]
    else:
        raise CheckError(f"unknown subject {word!r} (reply or file)")
    if not rest or rest[0][0] != "word" or rest[0][1] not in ("has", "lacks", "exists", "absent"):
        raise CheckError("expected has, lacks, exists or absent")
    verb = rest[0][1]
    rest = rest[1:]
    if verb in ("exists", "absent"):
        if subject != "file":
            raise CheckError(f"only a file is {verb}")
        if rest:
            raise CheckError(f"{verb} takes no pattern")
        return Clause(subject, path, verb, ())
    pats: list[Pattern] = []
    want = True
    while rest:
        k, v, fl = rest[0]
        if want:
            if k == "str":
                pats.append(Pattern("text", normalize(v)))
                rest = rest[1:]
            elif k == "re":
                for c in fl:
                    if c not in "ismx":
                        raise CheckError(f"unknown regex flag {c!r}")
                try:
                    _regex(v, fl)
                except re.error as e:
                    raise CheckError(f"a bad regex /{v}/: {e}") from None
                pats.append(Pattern("regex", v, fl))
                rest = rest[1:]
            elif k == "word" and v == "word" and len(rest) > 1 and rest[1][0] == "str":
                pats.append(Pattern("word", normalize(rest[1][1])))
                rest = rest[2:]
            else:
                raise CheckError(f"expected a pattern, found {v!r}")
            want = False
        else:
            if k != "bar":
                raise CheckError(f"expected | or the end, found {v!r}")
            want = True
            rest = rest[1:]
    if want:
        raise CheckError("a pattern is missing")
    return Clause(subject, path, verb, tuple(pats))


def parse(src: str) -> list[Line]:
    """A check's lines; at least one."""
    lines = []
    for n, raw in enumerate(src.splitlines(), 1):
        t = raw.strip()
        if not t or t.startswith("#"):
            continue
        toks = _tokens(t)
        groups: list[list] = [[]]
        for tok in toks:
            if tok == ("word", "or", ""):
                groups.append([])
            else:
                groups[-1].append(tok)
        try:
            clauses = tuple(_clause(g) for g in groups)
        except CheckError as e:
            raise CheckError(f"check line {n}: {t}: {e}") from None
        lines.append(Line(t, clauses))
    if not lines:
        raise CheckError("a check needs at least one line")
    return lines


def run(src: str, reply: str, root: Path | None = None) -> list[tuple[str, bool]]:
    """Every line's verdict, in order."""
    return [(ln.source, any(c.holds(reply, root) for c in ln.clauses)) for ln in parse(src)]


def passes(src: str, reply: str, root: Path | None = None) -> bool:
    return all(ok for _, ok in run(src, reply, root))


def quote(s: str) -> str:
    """`s` as a check string."""
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'
