"""The exam's check lines: the recall bench's check language (`reply has "x"`, `reply lacks /re/i`, `file …`)
plus the lines a personality exam needs. A probe's `checks` is lines; every line must hold. A line is either:

    prefix "Right,"                the reply starts with it (folded, case-insensitive)
    last_line has "— Quill" | …    the last non-blank line contains any of them
    max_words N / min_words N      word bounds
    max_sentences N                sentence bound (a run ending in . ! or ? and a space or the end)
    max_tokens N / min_tokens N    token bounds by the compiler's rule for text (tokens.py), not a tokenizer
    no_table                       no markdown table (a pipe row beside a |---| rule, or three pipe rows)
    no_bullets                     no line starting - * • or 1.
    tz_used                        the reply names the owner's time zone in one of the persona's `tz_names`
    ledger persona_digest          the manifest: this turn's `context.compiled` row names the persona and a digest
    ledger wake_created            a wake was created in the same turn
    ledger no_wake                 no wake was created in the turn

or any line of `bench/recall/checks.py`'s language (it is handed to it). A `ledger` line needs the daemon's own
record of the turn: an arm that has none (Claude Code) gets `None` for it, and a probe is scored on the lines that
apply; a probe with none that apply is not scored for that arm.

Standard library only.
"""

from __future__ import annotations

import math
import re
import sys
from pathlib import Path

RECALL = Path(__file__).resolve().parent.parent / "recall"
sys.path.insert(0, str(RECALL))

import checks as rc  # noqa: E402
import tokens as tk  # noqa: E402

LEDGER = ("persona_digest", "wake_created", "no_wake")
WORDS = ("prefix", "last_line", "max_words", "min_words", "max_sentences", "max_tokens", "min_tokens", "no_table",
         "no_bullets", "tz_used", "ledger")
TOKEN_RATES = tk.rates_of("claude-sonnet-5-5")


class CheckError(ValueError):
    pass


def first_word(line: str) -> str:
    return line.split(None, 1)[0] if line.split() else ""


def lines_of(src: str) -> list[str]:
    out = [ln.strip() for ln in src.splitlines() if ln.strip() and not ln.strip().startswith("#")]
    if not out:
        raise CheckError("a check needs at least one line")
    return out


def validate(src: str) -> None:
    """Parses every line, as run would; an unknown word is an error that names it."""
    for ln in lines_of(src):
        w = first_word(ln)
        if w in WORDS:
            _parse(ln)
        else:
            rc.parse(ln)


def _int(arg: str, line: str) -> int:
    try:
        n = int(arg)
    except ValueError:
        raise CheckError(f"{line!r}: a whole number is expected") from None
    if n < 0:
        raise CheckError(f"{line!r}: a bound is not negative")
    return n


def _strings(rest: str, line: str) -> list[str]:
    toks = rc._tokens(rest)
    out, want = [], True
    for kind, text, _ in toks:
        if want:
            if kind != "str":
                raise CheckError(f"{line!r}: a quoted string is expected, found {text!r}")
            out.append(rc.normalize(text))
        elif kind != "bar":
            raise CheckError(f"{line!r}: | or the end is expected, found {text!r}")
        want = not want
    if not out or want:
        raise CheckError(f"{line!r}: a string is missing")
    return out


def _parse(line: str) -> tuple:
    w, _, rest = line.partition(" ")
    rest = rest.strip()
    if w == "prefix":
        return (w, _strings(rest, line))
    if w == "last_line":
        v, _, tail = rest.partition(" ")
        if v not in ("has", "lacks"):
            raise CheckError(f"{line!r}: last_line has|lacks \"…\"")
        return (w, v, _strings(tail.strip(), line))
    if w in ("max_words", "min_words", "max_sentences", "max_tokens", "min_tokens"):
        return (w, _int(rest, line))
    if w in ("no_table", "no_bullets", "tz_used"):
        if rest:
            raise CheckError(f"{line!r}: {w} takes no argument")
        return (w,)
    if w == "ledger":
        if rest not in LEDGER:
            raise CheckError(f"{line!r}: ledger takes one of {', '.join(LEDGER)}")
        return (w, rest)
    raise CheckError(f"unknown check word {w!r}")


def words(s: str) -> int:
    return len(re.findall(r"\S+", s))


def sentences(s: str) -> int:
    """Sentences by their ends: a . ! or ? (or … ) followed by a space or the end. A list item or a heading without a
    full stop counts as one, so a bulleted answer is not short for having no full stops. Code fences are one."""
    s = re.sub(r"```.*?```", " CODE. ", s, flags=re.S)
    n = 0
    for para in re.split(r"\n+", s.strip()):
        para = para.strip()
        if not para:
            continue
        n += len([x for x in re.split(r"(?<=[.!?…])\s+", para) if x.strip()])
    return n


TABLE_RULE = re.compile(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)+\|?\s*$", re.M)
PIPE_ROW = re.compile(r"^\s*\|.*\|\s*$", re.M)
BULLET = re.compile(r"^\s*(?:[-*•]\s+|\d+[.)]\s+)", re.M)


def has_table(s: str) -> bool:
    return bool(TABLE_RULE.search(s)) or len(PIPE_ROW.findall(s)) >= 3


def last_line(s: str) -> str:
    ls = [ln for ln in s.strip().splitlines() if ln.strip()]
    return ls[-1] if ls else ""


def run_line(line: str, reply: str, facts: dict | None = None, root: Path | None = None) -> bool | None:
    """One line's verdict: True, False, or None when this arm has no way to know (a `ledger` line without the
    daemon's rows, `tz_used` without names)."""
    facts = facts or {}
    w = first_word(line)
    if w not in WORDS:
        return all(ok for _, ok in rc.run(line, reply, root))
    p = _parse(line)
    norm = rc.normalize(reply)
    if w == "prefix":
        return any(norm.startswith(s) for s in p[1])
    if w == "last_line":
        ll = rc.normalize(last_line(reply))
        hit = any(s in ll for s in p[2])
        return hit if p[1] == "has" else not hit
    if w == "max_words":
        return words(reply) <= p[1]
    if w == "min_words":
        return words(reply) >= p[1]
    if w == "max_sentences":
        return sentences(reply) <= p[1]
    if w in ("max_tokens", "min_tokens"):
        n = tk.user_text(reply).tokens(TOKEN_RATES) - tk.MESSAGE_TOKENS - tk.BLOCK_TOKENS
        return n <= p[1] if w == "max_tokens" else n >= p[1]
    if w == "no_table":
        return not has_table(reply)
    if w == "no_bullets":
        return not BULLET.search(reply)
    if w == "tz_used":
        names = facts.get("tz_names")
        if not names:
            return None
        return any(rc.normalize(n) in norm for n in names)
    # ledger
    kind = p[1]
    if "ledger" not in facts:
        return None
    led = facts["ledger"]
    if kind == "persona_digest":
        return bool(led.get("persona_digest"))
    if kind == "wake_created":
        return led.get("wakes_new", 0) >= 1
    return led.get("wakes_new", 0) == 0


def run(src: str, reply: str, facts: dict | None = None, root: Path | None = None) -> list[tuple[str, bool | None]]:
    return [(ln, run_line(ln, reply, facts, root)) for ln in lines_of(src)]


def verdict(results: list[tuple[str, bool | None]]) -> bool | None:
    """A probe's: every line that applies holds; None when none applies."""
    applies = [ok for _, ok in results if ok is not None]
    return None if not applies else all(applies)


def uses_ledger(src: str) -> bool:
    return any(first_word(ln) == "ledger" for ln in lines_of(src))
