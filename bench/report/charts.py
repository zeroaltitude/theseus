"""Benchmark charts as SVG, from declarative specs, in the house style (theseus-qla4). Standard library only.

A report's figures live in its data file (`docs/benchmarks/<report>.json`, key `figures`) as specs: plain JSON that
says what to draw, never how. `render(spec, mode)` draws one, and `write(spec, img_dir)` writes both modes,
`<name>.svg` (light) and `<name>-dark.svg` (dark), each painting its own surface so it reads on either page.
`picture(report, spec)` is the markdown that shows the right one: GitHub picks a `<picture>`'s
`prefers-color-scheme` source by the reader's theme. `python3 bench/report/draft.py plot <report>.json` does all
three for every figure in a data file.

The method is the dataviz skill's: the form is chosen by the data's job; colour by the arm, in a fixed categorical
order (the palette validated in light and dark, docs/benchmarks/README.md); one axis per chart; recessive hairline
grids; thin marks (bars at most 24 px with a 4 px rounded data end, 2 px lines, 8 px dots with a 2 px surface ring,
2 px surface gaps between touching fills); a legend for two or more series and selective direct labels; text in text
colours, never in a series colour. Every mark carries a `<title>`, so a pointer over it says its value where the SVG
is opened on its own (an `<img>` shows none: the report's tables are each chart's readable twin).

Every spec has:
    name      file stem (letters, digits, dashes)
    form      one of the forms below
    title     what is plotted (drawn at the top)
    question  the one question the figure answers (the SVG's description; the report's caption repeats it)
    note      optional small print under the plot (the source, the n)
    width     optional, default 720

Axes (`x`, `y`, `value`) take `label`, and optionally `min`, `max`, `log` (true for a log scale), and `format`:
`pct` (0.719 -> 71.9%), `num`, `int`, `usd`, `ms`, `s`, `min`, `h`, `x` (a ratio, 2.0x), or `kb`/`mb`.

The forms:
    intervals  a dot and its interval per row: `x`, `rows: [{label, arm, value, lo, hi, tip?, group?}]`, optional
               `reference: [{value, label}]` (vertical lines). Rows sharing a `group` sit under its heading.
    bars       `categories: [..]`, `series: [{arm, label, values: [..]}]`, `value` (the axis), `orientation`
               `h` (default) or `v`, `stacked` (default false), `labels` `ends` (default for <= 3 series) or `none`.
    lines      `x` (with `type` `time`, ISO strings, or `number`), `y`, `series: [{arm, label, points: [[x, y], ..],
               style: line|dots|both, tips?: [..]}]` (a null y breaks the line), optional `limits: [{y, label}]`
               (budgets: ink reference lines), `events: [{x, label}]` (numbered markers, keyed under the plot),
               `marks: [{x, y, label?, status, tip?}]` (status good|warning|serious|critical, keyed by
               `mark_labels: {status: text}`).
    multiples  small multiples of lines sharing an x axis: `x`, `columns` (default 2), `panels: [{title, y, series,
               limits?, events?, marks?}]`, optional shared `events`, `mark_labels`, `panel_height` (default 150).
    scatter    `x`, `y`, `points: [{arm, label, x, y, xlo?, xhi?, ylo?, yhi?, tip?}]`, optional
               `front: "min-x-max-y"` (the Pareto front: less x, more y).
    strip      per-unit values per group: `x`, `groups: [{arm, label, values: [..], tips?: [..]}]`; dots, the median
               and the quartiles.
    matrix     outcomes, rows x columns: `columns: [{label, arm}]`, `rows: [{label, cells: "PPFE-", note?,
               tips?: [..]}]`, one code per column: P solved (filled), F not solved (ring), E ended in an error class
               (ring and slash), - no result. Optional `state_labels: {P: .., F: .., E: .., -: ..}`.
    dumbbell   before -> after per row: `arm`, `x`, `from_label`, `to_label`, `rows: [{label, from, to, tip?}]`.

Optional sizes: `height_plot` (lines, scatter, vertical bars), `panel_height` (multiples). A time axis labels its
ticks in the zone of the data's own timestamps, so a figure renders the same on any machine. A `legend_labels: {arm:
text}` names the arms in a figure's legend (else the registry's names). A long title wraps. In lines and multiples
the series are clipped to the plot: a value past the axis (`y.max`, or a log axis's floor) is an arrowhead at the edge
whose hover says the value, and a mark past it sits on the edge; a multiples panel draws the shared `events` and its
own, numbered after them.

Arms (the `arm` of a row, series, point or column) are the registry's keys, `ARMS` below: the same arm is the same
colour in every report. A key not in the registry is an error, so a new arm is added here, in its slot, and in the
README's palette table, and the palette is validated again. Four neutrals are not slots: `none` (a no-memory
baseline, the de-emphasis gray), `oracle` (a ceiling, the secondary ink), `context` (the gray of the emphasis form: a
second statistic of an arm, or the rest beside the one that matters) and `other` (an arm outside the registry, as
`efficiency.py` charts arms named on its command line: gray, its name on the point).
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any
from xml.sax.saxutils import escape, quoteattr

# ------------------------------------------------------------------ palette

# The dataviz skill's validated categorical palette, in its fixed order; each mode its own steps
# (docs/benchmarks/README.md: the validation, light and dark).
SLOTS = {
    "light": ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#4a3aa7", "#e34948"],
    "dark": ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"],
}

# Chart chrome and ink per mode. Surfaces are the palette's validated chart surfaces.
CHROME = {
    "light": {"surface": "#fcfcfb", "ink": "#0b0b0b", "ink2": "#52514e", "muted": "#898781", "grid": "#e1e0d9",
              "axis": "#c3c2b7", "border": "#e6e5df"},
    "dark": {"surface": "#1a1a19", "ink": "#ffffff", "ink2": "#c3c2b7", "muted": "#898781", "grid": "#2c2c2a",
             "axis": "#383835", "border": "#2c2c2a"},
}

# Status: reserved meaning, never a series; always shipped with a glyph and a label.
STATUS = {"good": "#0ca30c", "warning": "#fab219", "serious": "#ec835a", "critical": "#d03b3b"}

# The arms: key -> (slot, display name). Harness arms take slots 1 to 4; configurations inside Theseus (memory and
# retrieval arms) 5 to 8. Two neutrals are not slots: a baseline in the de-emphasis gray, a ceiling in the ink.
ARMS: dict[str, tuple[int, str]] = {
    "theseus": (1, "Theseus"),
    "claude-code": (2, "Claude Code"),
    "theseus-batching": (3, "Theseus + batching paragraph"),
    "openclaw": (4, "OpenClaw"),
    "bm25": (5, "BM25"),
    "vector": (6, "vectors"),
    "fused": (7, "fused ranks"),
    "entity": (8, "entities"),
}
NEUTRALS = {"none": ("muted", "no memory"), "oracle": ("ink2", "oracle"), "other": ("muted", "other arm"),
            "context": ("muted", "context")}

MODES = ("light", "dark")
FONT = "system-ui, -apple-system, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif"
PAD = 18


def arm_color(arm: str, mode: str) -> str:
    if arm in ARMS:
        return SLOTS[mode][ARMS[arm][0] - 1]
    if arm in NEUTRALS:
        return CHROME[mode][NEUTRALS[arm][0]]
    raise ValueError(f"unknown arm {arm!r}: add it to charts.ARMS (and the README's palette table) first")


def _mix(a: str, b: str, t: float) -> str:
    """a mixed toward b by t (0 = a, 1 = b), in sRGB."""
    ca = [int(a[i:i + 2], 16) for i in (1, 3, 5)]
    cb = [int(b[i:i + 2], 16) for i in (1, 3, 5)]
    return "#" + "".join(f"{round(x + (y - x) * t):02x}" for x, y in zip(ca, cb))


def _luminance(c: str) -> float:
    def ch(v: int) -> float:
        x = v / 255
        return x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4
    r, g, b = (int(c[i:i + 2], 16) for i in (1, 3, 5))
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a: str, b: str) -> float:
    """The WCAG contrast ratio of two colours."""
    la, lb = sorted((_luminance(a), _luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def tint(arm: str, mode: str, t: float = 0.55) -> str:
    """A lighter step of an arm's colour toward the surface, the 'before' of a dumbbell: as far as t, but never so
    far that it falls under 2.1:1 against the surface (the method's floor for an ordinal ramp's light end is 2:1)."""
    full, surface = arm_color(arm, mode), CHROME[mode]["surface"]
    while t > 0 and contrast(_mix(full, surface, t), surface) < 2.1:
        t = round(t - 0.05, 2)
    return _mix(full, surface, max(t, 0.0))


# ------------------------------------------------------------------ text

# Advance widths in em of a common sans (Arial's), for layout; unknown characters count as 0.6 em.
_W = {**dict.fromkeys("ijl.,:;'|!", 0.24), **dict.fromkeys("frt()[]-/", 0.33), **dict.fromkeys("Ifs", 0.42),
      **dict.fromkeys("abcdeghknopquvxyz0123456789$#?_", 0.556), **dict.fromkeys("mw%MW", 0.86), " ": 0.278,
      **dict.fromkeys("ABCDEFGHJKLNOPQRSTUVXYZ&", 0.69), "·": 0.28, "×": 0.58, "→": 0.9, "–": 0.556, "—": 1.0}


def text_width(s: str, size: float, weight: int = 400) -> float:
    w = sum(_W.get(ch, 0.6) for ch in str(s)) * size * 1.12  # DejaVu Sans, the widest common system-ui, is ~12% over
    return w * (1.12 if weight >= 600 else 1.0)


def wrap(s: str, size: float, width: float, weight: int = 400) -> list[str]:
    lines, cur = [], ""
    for word in str(s).split():
        nxt = word if not cur else cur + " " + word
        if cur and text_width(nxt, size, weight) > width:
            lines.append(cur)
            cur = word
        else:
            cur = nxt
    if cur:
        lines.append(cur)
    return lines


# ------------------------------------------------------------------ numbers

def fmt(v: float | None, kind: str | None = None, step: float | None = None, tick: bool = False) -> str:
    """A value as text. `tick` gives the short form an axis uses (no unit word where the axis label carries it)."""
    if v is None or (isinstance(v, float) and math.isnan(v)):
        return "–"
    if v < 0:  # a true minus sign, before any unit or currency sign: −25%, −$0.50
        text = fmt(-v, kind, step, tick)
        return text if not any(ch in "123456789" for ch in text) else "−" + text
    kind = kind or "num"
    if kind == "pct":
        places = (_step_places(step * 100) if step else 0) if tick else 1
        return f"{v * 100:.{places}f}%"
    if kind == "usd":
        if tick:
            if step:
                places = max(2, _step_places(step))
            else:
                places = 2 if not v or abs(v) >= 0.01 else max(2, -math.floor(math.log10(abs(v)) + 1e-9))
            return f"${v:,.{places}f}"
        a = abs(v)
        return f"${v:,.2f}" if a >= 0.1 or v == 0 else (f"${v:.3f}" if a >= 0.01 else f"${v:.4f}")
    if kind == "int":
        return f"{v:,.0f}"
    if kind == "x":
        if tick:
            return f"{v:,.{_places(v, step)}f}×"
        return f"{v:.2f}×" if abs(v) < 10 else f"{v:,.0f}×"
    places = _places(v, step)
    if tick and step is None:  # a log axis's ticks: as few places as the value needs
        places = 0 if v == 0 or abs(v) >= 1 else -math.floor(math.log10(abs(v)) + 1e-9)
    num = f"{v:,.{places}f}"
    if tick:
        return num
    unit = {"ms": " ms", "s": " s", "min": " min", "h": " h", "kb": " KB", "mb": " MB"}.get(kind, "")
    return num + unit


def _step_places(step: float) -> int:
    """The decimal places that print every multiple of a tick step exactly: 20 -> 0, 2.5 -> 1, 0.25 -> 2."""
    for p in range(7):
        if abs(round(step, p) - step) <= abs(step) * 1e-9:
            return p
    return 6


def _places(v: float, step: float | None) -> int:
    if step is not None:
        return _step_places(step)
    a = abs(v)
    return 0 if a >= 100 or a == int(a) else (1 if a >= 10 else 2)


def nice_ticks(lo: float, hi: float, n: int = 5) -> tuple[list[float], float]:
    """Round ticks covering [lo, hi]: steps of 1, 2 or 5 times a power of ten."""
    if hi <= lo:
        hi = lo + 1
    raw = (hi - lo) / max(1, n)
    mag = 10 ** math.floor(math.log10(raw))
    step = next(m * mag for m in (1, 2, 2.5, 5, 10) if m * mag >= raw)
    start = math.floor(lo / step + 1e-9) * step
    ticks = []
    v = start
    while v <= hi + step * 1e-6:
        ticks.append(round(v, 12))
        v += step
    if ticks[-1] < hi - step * 1e-6:
        ticks.append(round(ticks[-1] + step, 12))
    return ticks, step


def log_ticks(lo: float, hi: float) -> list[float]:
    out = []
    e = math.floor(math.log10(lo))
    while 10 ** e <= hi * 1.0001:
        for m in (1, 2, 5):
            v = m * 10 ** e
            if lo * 0.9999 <= v <= hi * 1.0001:
                out.append(v)
        e += 1
    if len(out) > 8:
        out = [v for v in out if math.isclose(math.log10(v) % 1, 0, abs_tol=1e-9)]
    return out


class Scale:
    """A linear or log map from data to pixels."""

    def __init__(self, lo: float, hi: float, a: float, b: float, log: bool = False):
        self.lo, self.hi, self.a, self.b, self.log = lo, hi, a, b, log

    def __call__(self, v: float) -> float:
        if self.log:
            v = max(v, self.lo)
            t = (math.log10(v) - math.log10(self.lo)) / (math.log10(self.hi) - math.log10(self.lo))
        else:
            t = (v - self.lo) / (self.hi - self.lo) if self.hi != self.lo else 0.5
        return self.a + (self.b - self.a) * t


def axis_domain(values: list[float], axis: dict[str, Any], zero: bool = True) -> tuple[float, float, list[float], float]:
    """(lo, hi, ticks, step) for an axis spec over the values it must show."""
    vals = [v for v in values if v is not None and not (isinstance(v, float) and math.isnan(v))]
    if axis.get("log"):
        pos = [v for v in vals if v > 0] or [1.0]
        lo = axis.get("min") or 10 ** math.floor(math.log10(min(pos)))
        hi = axis.get("max") or 10 ** math.ceil(math.log10(max(pos)))
        if hi <= lo:
            hi = lo * 10
        return lo, hi, log_ticks(lo, hi), 0.0
    lo = axis.get("min")
    hi = axis.get("max")
    dlo = min(vals) if vals else 0.0
    dhi = max(vals) if vals else 1.0
    if lo is None:
        lo = 0.0 if zero and dlo >= 0 else dlo
    if hi is None:
        hi = dhi if dhi > lo else lo + 1
        if axis.get("format") == "pct" and hi <= 1 and dhi > 0.8:
            hi = 1.0
    ticks, step = nice_ticks(lo, hi)
    if axis.get("min") is None:
        lo = min(lo, ticks[0])
    if axis.get("max") is None:
        hi = ticks[-1]
    ticks = [t for t in ticks if lo - 1e-9 <= t <= hi + 1e-9]
    return lo, hi, ticks, step


# ------------------------------------------------------------------ time

def parse_time(s: Any) -> datetime:
    if isinstance(s, (int, float)):
        return datetime.fromtimestamp(s)
    return datetime.fromisoformat(str(s).replace("Z", "+00:00"))


def time_ticks(t0: datetime, t1: datetime, n: int = 6, width: float | None = None) -> tuple[list[datetime], str]:
    span = (t1 - t0).total_seconds()
    steps = [(3600, "%H:%M"), (3 * 3600, "%H:%M"), (6 * 3600, "%b %-d %H:%M"), (12 * 3600, "%b %-d %H:%M"),
             (86400, "%b %-d"), (2 * 86400, "%b %-d"), (7 * 86400, "%b %-d")]
    def fits(s: int, f: str) -> bool:
        if span / s > n:
            return False
        if width is None:
            return True
        label = datetime(2026, 10, 30, 23, 0).strftime(f)
        return (span / s + 1) * (text_width(label, 11) + 14) <= width
    step, form = next(((s, f) for s, f in steps if fits(s, f)), steps[-1])
    if step >= 86400:
        start = t0.replace(hour=0, minute=0, second=0, microsecond=0)
    else:
        h = (t0.hour // (step // 3600)) * (step // 3600)
        start = t0.replace(hour=h, minute=0, second=0, microsecond=0)
    ticks = []
    t = start
    while t <= t1:
        if t >= t0:
            ticks.append(t)
        t += timedelta(seconds=step)
    return ticks, form


# ------------------------------------------------------------------ svg

class Svg:
    def __init__(self, mode: str):
        self.mode = mode
        self.c = CHROME[mode]
        self.parts: list[str] = []

    def add(self, s: str) -> None:
        self.parts.append(s)

    def text(self, x: float, y: float, s: Any, size: float = 12, fill: str | None = None, anchor: str = "start",
             weight: int = 400, tabular: bool = False, rotate: float | None = None, title: str | None = None,
             halo: bool = False) -> None:
        attrs = [f'x="{x:.1f}"', f'y="{y:.1f}"', f'font-size="{size}"', f'fill="{fill or self.c["ink2"]}"']
        if halo:  # the surface drawn under the letters, so a line crossing them never runs through them
            attrs.append(f'stroke="{self.c["surface"]}" stroke-width="3" stroke-linejoin="round" paint-order="stroke"')
        if anchor != "start":
            attrs.append(f'text-anchor="{anchor}"')
        if weight != 400:
            attrs.append(f'font-weight="{weight}"')
        if tabular:
            attrs.append('style="font-variant-numeric: tabular-nums"')
        if rotate is not None:
            attrs.append(f'transform="rotate({rotate:.0f} {x:.1f} {y:.1f})"')
        inner = escape(str(s))
        if title:
            inner = f"<title>{escape(title)}</title>{inner}"
        self.add(f"<text {' '.join(attrs)}>{inner}</text>")

    def line(self, x1: float, y1: float, x2: float, y2: float, stroke: str, width: float = 1,
             cap: str | None = None, opacity: float | None = None) -> None:
        extra = f' stroke-linecap="{cap}"' if cap else ""
        if opacity is not None:
            extra += f' stroke-opacity="{opacity}"'
        self.add(f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" stroke="{stroke}" '
                 f'stroke-width="{width}"{extra}/>')

    def circle(self, cx: float, cy: float, r: float, fill: str, ring: bool = True, stroke: str | None = None,
               stroke_width: float = 2, opacity: float | None = None) -> None:
        s = stroke or (self.c["surface"] if ring else None)
        st = f' stroke="{s}" stroke-width="{stroke_width}"' if s else ""
        op = f' fill-opacity="{opacity}"' if opacity is not None else ""
        self.add(f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r}" fill="{fill}"{op}{st}/>')

    def hit(self, cx: float, cy: float, r: float = 12) -> None:
        """A transparent hit area bigger than the mark, for its hover."""
        self.add(f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r}" fill="transparent"/>')

    def group(self, title: str | None) -> None:
        self.add("<g>" + (f"<title>{escape(title)}</title>" if title else ""))

    def end(self) -> None:
        self.add("</g>")


def hbar_path(x0: float, x1: float, y: float, t: float, r: float = 4) -> str:
    """A horizontal bar from x0 (square, the baseline) to x1 (its data end, rounded)."""
    if x1 < x0:  # a negative bar's data end is on the left
        x0, x1 = x1, x0
        r = min(r, t / 2, x1 - x0)
        return (f"M{x1:.1f},{y:.1f}H{x0 + r:.1f}A{r},{r} 0 0 0 {x0:.1f},{y + r:.1f}V{y + t - r:.1f}"
                f"A{r},{r} 0 0 0 {x0 + r:.1f},{y + t:.1f}H{x1:.1f}Z")
    r = max(0.0, min(r, t / 2, x1 - x0))
    return (f"M{x0:.1f},{y:.1f}H{x1 - r:.1f}A{r},{r} 0 0 1 {x1:.1f},{y + r:.1f}V{y + t - r:.1f}"
            f"A{r},{r} 0 0 1 {x1 - r:.1f},{y + t:.1f}H{x0:.1f}Z")


def vbar_path(x: float, t: float, y0: float, y1: float, r: float = 4) -> str:
    """A vertical bar from y0 (the baseline, below) up to y1 (its data end, rounded)."""
    r = max(0.0, min(r, t / 2, y0 - y1))
    return (f"M{x:.1f},{y0:.1f}V{y1 + r:.1f}A{r},{r} 0 0 1 {x + r:.1f},{y1:.1f}H{x + t - r:.1f}"
            f"A{r},{r} 0 0 1 {x + t:.1f},{y1 + r:.1f}V{y0:.1f}Z")


# ------------------------------------------------------------------ frame: title, legend, footer

class Frame:
    """The figure around a plot: the title, the legend, the note and the event key, and the plot's box."""

    def __init__(self, spec: dict[str, Any], mode: str):
        self.spec, self.mode = spec, mode
        self.svg = Svg(mode)
        self.c = CHROME[mode]
        self.w = int(spec.get("width") or 720)
        self.legend: list[tuple[str, str, str]] = []  # (kind, colour, label); kind bar|line|dot|ring|glyph-*
        self.keys: list[str] = []  # the event key's lines
        self.clips = 0

    def clip_id(self) -> str:
        self.clips += 1
        return f'clip-{re.sub(r"[^a-z0-9-]", "", str(self.spec.get("name", "c")))}-{self.clips}'

    def title_lines(self) -> list[str]:
        return wrap(self.spec.get("title", ""), 15, self.w - 2 * PAD, weight=600) or [""]

    def add_legend(self, kind: str, color: str, label: str) -> None:
        if (kind, color, label) not in self.legend:
            self.legend.append((kind, color, label))

    def header_height(self) -> float:
        h = PAD + 18 + 20 * (len(self.title_lines()) - 1)
        if self.legend:
            h += self._legend_rows() * 20 + 6
        return h + 10

    def _legend_layout(self) -> list[list[tuple[str, str, str, float]]]:
        rows: list[list[tuple[str, str, str, float]]] = [[]]
        x = PAD
        for kind, color, label in self.legend:
            iw = 22 + text_width(label, 12) + 18
            if rows[-1] and x + iw > self.w - PAD:
                rows.append([])
                x = PAD
            rows[-1].append((kind, color, label, x))
            x += iw
        return rows

    def _legend_rows(self) -> int:
        return len(self._legend_layout()) if self.legend else 0

    def footer_lines(self) -> list[str]:
        lines = []
        for i, k in enumerate(self.keys, 1):
            lines += wrap(f"{i}  {k}", 11, self.w - 2 * PAD)
        if self.spec.get("note"):
            lines += wrap(self.spec["note"], 11, self.w - 2 * PAD)
        return lines

    def footer_height(self) -> float:
        n = len(self.footer_lines())
        return (n * 15 + 8 if n else 0) + PAD - 4

    def finish(self, body_height: float) -> str:
        """The whole SVG: header, the body already drawn into self.svg at y offset header_height(), the footer."""
        top = self.header_height()
        h = math.ceil(top + body_height + self.footer_height())
        c, spec = self.c, self.spec
        tid = "t-" + re.sub(r"[^a-z0-9-]", "", str(spec.get("name", "chart")).lower())
        head = [
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.w}" height="{h}" viewBox="0 0 {self.w} {h}" '
            f'role="img" aria-labelledby="{tid} {tid}-d" font-family="{FONT}">',
            f'<title id="{tid}">{escape(str(spec.get("title", "")))}</title>',
            f'<desc id="{tid}-d">{escape(str(spec.get("question", "")))}</desc>',
            f'<rect width="{self.w}" height="{h}" rx="8" fill="{c["surface"]}" stroke="{c["border"]}"/>',
        ]
        hs = Svg(self.mode)
        for i, ln in enumerate(self.title_lines()):
            hs.text(PAD, PAD + 14 + 20 * i, ln, size=15, fill=c["ink"], weight=600)
        y = PAD + 14 + 22 + 20 * (len(self.title_lines()) - 1)
        for row in self._legend_layout() if self.legend else []:
            for kind, color, label, x in row:
                self._swatch(hs, kind, color, x, y - 4)
                hs.text(x + 22, y, label, size=12, fill=c["ink2"])
            y += 20
        foot = Svg(self.mode)
        fy = top + body_height + 14
        for i, ln in enumerate(self.footer_lines()):
            foot.text(PAD, fy + i * 15, ln, size=11, fill=c["ink2"])
        return "\n".join(head + hs.parts + self.svg.parts + foot.parts + ["</svg>"]) + "\n"

    def _swatch(self, s: Svg, kind: str, color: str, x: float, y: float) -> None:
        if kind == "bar":
            s.add(f'<rect x="{x:.1f}" y="{y - 6:.1f}" width="14" height="12" rx="3" fill="{color}"/>')
        elif kind == "line":
            s.line(x, y, x + 16, y, color, 2, cap="round")
        elif kind == "dot":
            s.circle(x + 7, y, 5, color, ring=False)
        elif kind == "ring":
            s.circle(x + 7, y, 4.5, self.c["surface"], ring=False, stroke=color, stroke_width=1.6)
        elif kind == "slash":
            s.circle(x + 7, y, 4.5, self.c["surface"], ring=False, stroke=color, stroke_width=1.6)
            s.line(x + 3.5, y + 3.5, x + 10.5, y - 3.5, color, 1.6)
        elif kind == "dash":
            s.line(x + 3, y, x + 11, y, color, 1.6)
        elif kind == "ref":
            s.line(x + 7, y - 7, x + 7, y + 7, color, 1.5)
        elif kind.startswith("status"):
            s.circle(x + 7, y, 5, color, ring=False)
            s.add(f'<path d="M{x + 4:.1f},{y:.1f}h6" stroke="{self.c["surface"]}" stroke-width="1.6"/>')


def _arms_legend(frame: Frame, arms: list[str], kind: str, labels: dict[str, str] | None = None) -> None:
    labels = labels or frame.spec.get("legend_labels")
    seen = []
    for a in arms:
        if a not in seen:
            seen.append(a)
    if len(seen) >= 2:
        for a in seen:
            label = (labels or {}).get(a) or (ARMS[a][1] if a in ARMS else NEUTRALS[a][1])
            frame.add_legend(kind, arm_color(a, frame.mode), label)


def _x_axis(s: Svg, sc: Scale, ticks: list[float], fmt_kind: str | None, step: float, top: float, bottom: float,
            label: str | None, c: dict[str, str], grid: bool = True) -> None:
    for t in ticks:
        x = sc(t)
        if grid:
            s.line(x, top, x, bottom, c["grid"], 1)
        s.text(x, bottom + 16, fmt(t, fmt_kind, step if step else None, tick=True), size=11, anchor="middle",
               tabular=True)
    s.line(sc.a, bottom, sc.b, bottom, c["axis"], 1)
    if label:
        s.text((sc.a + sc.b) / 2, bottom + 34, label, size=12, anchor="middle")


def _y_axis(s: Svg, sc: Scale, ticks: list[float], fmt_kind: str | None, step: float, left: float, right: float,
            label: str | None, c: dict[str, str], top: float) -> None:
    for t in ticks:
        y = sc(t)
        s.line(left, y, right, y, c["grid"], 1)
        s.text(left - 6, y + 4, fmt(t, fmt_kind, step if step else None, tick=True), size=11, anchor="end",
               tabular=True)
    if label:
        s.text(left, top - 8, label, size=11, anchor="start")


def _tick_width(ticks: list[float], kind: str | None, step: float) -> float:
    return max((text_width(fmt(t, kind, step if step else None, tick=True), 11) for t in ticks), default=20)


# ------------------------------------------------------------------ intervals

def _intervals(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    rows = spec["rows"]
    xa = spec.get("x", {})
    kind = xa.get("format")
    refs = spec.get("reference") or []
    vals = [r[k] for r in rows for k in ("value", "lo", "hi") if r.get(k) is not None] + [r["value"] for r in refs]
    lo, hi, ticks, step = axis_domain(vals, xa)
    _arms_legend(f, [r["arm"] for r in rows], "dot")
    for r in refs:
        f.add_legend("ref", c["ink2"], r["label"])
    top = f.header_height() + (16 if refs else 4)
    label_w = max(text_width(r["label"], 12) for r in rows) + 14
    groups = [r.get("group") for r in rows]
    vtexts = [_interval_text(r, kind) for r in rows]
    val_w = max(text_width(t, 11) for t in vtexts) + 14
    left, right = PAD + label_w, f.w - PAD - val_w
    sc = Scale(lo, hi, left, right, log=bool(xa.get("log")))
    y = top
    pos = []
    heads = []
    last_group = object()
    for r, g in zip(rows, groups):
        if g != last_group and g is not None:
            y += 6 if pos else 0
            heads.append((y + 13, g))
            y += 22
        last_group = g
        pos.append(y + 14)
        y += 28
    bottom = y + 4
    for t in ticks:
        s.line(sc(t), top - 4, sc(t), bottom, c["grid"], 1)
    for hy, g in heads:
        s.text(PAD, hy, g, size=12, fill=c["ink"], weight=600, halo=True)
    for r in refs:
        x = sc(r["value"])
        s.group(f'{r["label"]}: {fmt(r["value"], kind)}')
        s.line(x, top - 10, x, bottom, c["ink2"], 1.5)
        s.end()
    for r, cy, vt in zip(rows, pos, vtexts):
        col = arm_color(r["arm"], mode)
        s.text(left - 14, cy + 4, r["label"], size=12, anchor="end", fill=c["ink"])
        s.group(f'{r["label"]}: {vt}' + (f' ({r["tip"]})' if r.get("tip") else ""))
        if r.get("lo") is not None and r.get("hi") is not None:
            s.line(sc(r["lo"]), cy, sc(r["hi"]), cy, col, 2, cap="round")
            for e in (r["lo"], r["hi"]):
                s.line(sc(e), cy - 5, sc(e), cy + 5, col, 1.5, cap="round")
        s.hit(sc(r["value"]), cy)
        s.circle(sc(r["value"]), cy, 5, col)
        s.end()
        s.text(right + 14, cy + 4, vt, size=11, tabular=True)
    s.line(left, bottom, right, bottom, c["axis"], 1)
    for t in ticks:
        s.text(sc(t), bottom + 16, fmt(t, kind, step or None, tick=True), size=11, anchor="middle", tabular=True)
    if xa.get("label"):
        s.text((left + right) / 2, bottom + 34, xa["label"], size=12, anchor="middle")
    return f.finish(bottom + 42 - f.header_height())


def _interval_text(r: dict[str, Any], kind: str | None) -> str:
    v = fmt(r["value"], kind)
    if r.get("lo") is None or r.get("hi") is None:
        return v
    return f'{v}  [{fmt(r["lo"], kind)}, {fmt(r["hi"], kind)}]'


# ------------------------------------------------------------------ bars

def _bars(spec: dict[str, Any], mode: str) -> str:
    if spec.get("orientation", "h") == "v":
        return _vbars(spec, mode)
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    cats, series = spec["categories"], spec["series"]
    va = spec.get("value", {})
    kind = va.get("format")
    stacked = bool(spec.get("stacked"))
    labels = spec.get("labels", "ends" if len(series) <= 3 else "none")
    _arms_legend_series(f, series, "bar")
    if stacked:
        totals = [sum((sr["values"][i] or 0) for sr in series) for i in range(len(cats))]
        vals = totals
    else:
        vals = [v for sr in series for v in sr["values"] if v is not None]
    lo, hi, ticks, step = axis_domain(vals + [0], va)
    top = f.header_height() + 2
    label_w = max(text_width(cat, 12) for cat in cats) + 14
    end_w = (max((text_width(fmt(v, kind), 11) for v in vals), default=0) + 10) if labels != "none" else 8
    left, right = PAD + label_w, f.w - PAD - end_w
    sc = Scale(lo, hi, left, right)
    n = 1 if stacked else len(series)
    t = 18 if n == 1 else max(8, min(16, 30 / n + 4))
    band = n * t + (n - 1) * 2 + 14
    bottom = top + band * len(cats)
    for tk in ticks:
        s.line(sc(tk), top, sc(tk), bottom, c["grid"], 1)
    for i, cat in enumerate(cats):
        y0 = top + i * band + 7
        s.text(left - 10, y0 + (n * t + (n - 1) * 2) / 2 + 4, cat, size=12, anchor="end", fill=c["ink"])
        if stacked:
            x = sc(0)
            nz = [j for j, sr in enumerate(series) if (sr["values"][i] or 0) > 0]
            for j, sr in enumerate(series):
                v = sr["values"][i] or 0
                if v <= 0:
                    continue
                x1 = sc(sc_inv_add(sc, x, v))
                last = j == nz[-1]
                gap_l = 1 if j != nz[0] else 0
                gap_r = 0 if last else 1
                s.group(f'{cat} · {sr["label"]}: {fmt(v, kind)}')
                path = hbar_path(x + gap_l, x1 - gap_r, y0, t, 4 if last else 0)
                s.add(f'<path d="{path}" fill="{arm_color(sr["arm"], mode)}"/>')
                s.end()
                x = x1
            if labels != "none":
                s.text(x + 6, y0 + t / 2 + 4, fmt(totals[i], kind), size=11, tabular=True)
        else:
            for j, sr in enumerate(series):
                v = sr["values"][i]
                if v is None:
                    continue
                yy = y0 + j * (t + 2)
                s.group(f'{cat} · {sr["label"]}: {fmt(v, kind)}' + (f' ({sr["tips"][i]})' if sr.get("tips") else ""))
                s.add(f'<path d="{hbar_path(sc(0), sc(v), yy, t)}" fill="{arm_color(sr["arm"], mode)}"/>')
                s.end()
                if labels != "none":
                    s.text(max(sc(v), sc(0)) + 5, yy + t / 2 + 4, fmt(v, kind), size=11, tabular=True)
    _x_axis(s, sc, ticks, kind, step, top, bottom, va.get("label"), c, grid=False)
    return f.finish(bottom + 42 - f.header_height())


def sc_inv_add(sc: Scale, x: float, v: float) -> float:
    """The data value at pixel x, plus v (for stacking on a linear scale)."""
    base = sc.lo + (x - sc.a) / (sc.b - sc.a) * (sc.hi - sc.lo)
    return base + v


def _arms_legend_series(f: Frame, series: list[dict[str, Any]], kind: str) -> None:
    if len(series) >= 2:
        for sr in series:
            f.add_legend(kind, arm_color(sr["arm"], f.mode), sr.get("label") or ARMS.get(sr["arm"], ("", sr["arm"]))[1])


def _vbars(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    cats, series = spec["categories"], spec["series"]
    va = spec.get("value", {})
    kind = va.get("format")
    stacked = bool(spec.get("stacked"))
    labels = spec.get("labels", "ends" if len(series) <= 3 else "none")
    _arms_legend_series(f, series, "bar")
    totals = [sum((sr["values"][i] or 0) for sr in series) for i in range(len(cats))]
    vals = totals if stacked else [v for sr in series for v in sr["values"] if v is not None]
    lo, hi, ticks, step = axis_domain(vals + [0], va)
    top = f.header_height() + (18 if labels != "none" else 6)
    plot_h = int(spec.get("height_plot") or 220)
    left = PAD + _tick_width(ticks, kind, step) + 10
    right = f.w - PAD
    n = 1 if stacked else len(series)
    band = (right - left) / len(cats)
    t = min(24, max(6, (band - 12 - 2 * (n - 1)) / n))
    rot = any(text_width(cat, 11) > band - 6 for cat in cats)
    bottom = top + plot_h
    sc = Scale(lo, hi, bottom, top)
    _y_axis(s, sc, ticks, kind, step, left, right, va.get("label"), c, top)
    for i, cat in enumerate(cats):
        group_w = n * t + (n - 1) * 2
        x0 = left + i * band + (band - group_w) / 2
        if stacked:
            y = sc(0)
            nz = [j for j, sr in enumerate(series) if (sr["values"][i] or 0) > 0]
            for j, sr in enumerate(series):
                v = sr["values"][i] or 0
                if v <= 0:
                    continue
                y1 = sc(sc_inv_add(sc, y, v))
                last = j == nz[-1]
                s.group(f'{cat} · {sr["label"]}: {fmt(v, kind)}')
                s.add(f'<path d="{vbar_path(x0, t, y - (1 if j != nz[0] else 0), y1 + (0 if last else 1), 4 if last else 0)}" '
                      f'fill="{arm_color(sr["arm"], mode)}"/>')
                s.end()
                y = y1
            if labels != "none":
                s.text(x0 + t / 2, y - 6, fmt(totals[i], kind), size=11, anchor="middle", tabular=True)
        else:
            for j, sr in enumerate(series):
                v = sr["values"][i]
                if v is None:
                    continue
                xx = x0 + j * (t + 2)
                s.group(f'{cat} · {sr["label"]}: {fmt(v, kind)}')
                s.add(f'<path d="{vbar_path(xx, t, sc(0), sc(v))}" fill="{arm_color(sr["arm"], mode)}"/>')
                s.end()
                if labels != "none" and n <= 3:
                    s.text(xx + t / 2, sc(v) - 5, fmt(v, kind), size=10 if n > 1 else 11, anchor="middle",
                           tabular=True)
        cx = left + i * band + band / 2
        if rot:
            s.text(cx + 4, bottom + 12, cat, size=11, anchor="end", rotate=-35)
        else:
            s.text(cx, bottom + 16, cat, size=11, anchor="middle")
    s.line(left, bottom, right, bottom, c["axis"], 1)
    extra = max((text_width(cat, 11) for cat in cats), default=0) * 0.6 + 16 if rot else 26
    return f.finish(bottom + extra - f.header_height())


# ------------------------------------------------------------------ lines and multiples

def _xs(points: list[list[Any]], time: bool) -> list[float]:
    return [parse_time(p[0]).timestamp() if time else float(p[0]) for p in points]


def _tz_of(series: list[dict[str, Any]]):
    """The zone of the data's own timestamps (the first with an offset), so tick labels never depend on the zone of
    the machine that renders them; UTC when none has one."""
    from datetime import timezone
    for sr in series:
        for p in sr.get("points") or []:
            t = parse_time(p[0])
            if t.tzinfo is not None:
                return t.tzinfo
    return timezone.utc


def _x_scale_spec(xa: dict[str, Any], all_x: list[float], left: float, right: float,
                  tz=None) -> tuple[Scale, list[float], list[str], float]:
    time = xa.get("type") == "time"
    if time:
        from datetime import timezone
        lo = parse_time(xa["min"]).timestamp() if xa.get("min") else min(all_x)
        hi = parse_time(xa["max"]).timestamp() if xa.get("max") else max(all_x)
        if hi <= lo:
            hi = lo + 3600
        tz = tz or timezone.utc
        t0 = datetime.fromtimestamp(lo, tz=tz)
        t1 = datetime.fromtimestamp(hi, tz=tz)
        tks, form = time_ticks(t0, t1, width=right - left)
        return Scale(lo, hi, left, right), [t.timestamp() for t in tks], [t.strftime(form) for t in tks], 0
    lo, hi, ticks, step = axis_domain(all_x, xa, zero=False)
    return Scale(lo, hi, left, right, xa.get("log", False)), ticks, [fmt(t, xa.get("format"), step or None, tick=True) for t in ticks], step


def _draw_lines_panel(f: Frame, panel: dict[str, Any], xa: dict[str, Any], box: tuple[float, float, float, float],
                      events: list[tuple[float, int]], small: bool, xlabels: bool, x_override: tuple | None = None) -> None:
    s, c, mode = f.svg, f.c, f.mode
    left, top, right, bottom = box
    time = xa.get("type") == "time"
    ya = panel.get("y", {})
    kind = ya.get("format")
    ys = [p[1] for sr in panel["series"] for p in sr["points"] if p[1] is not None]
    ys += [lm["y"] for lm in panel.get("limits") or []]
    ys += [m["y"] for m in panel.get("marks") or []]
    if ys and not ya.get("log") and ya.get("max") is None:
        ys = ys + [max(ys) * 1.06]
    lo, hi, ticks, step = axis_domain(ys, ya)
    sy = Scale(lo, hi, bottom, top, ya.get("log", False))
    if x_override:
        sx, xticks, xlabs = x_override
    else:
        allx = [x for sr in panel["series"] for x in _xs(sr["points"], time)]
        sx, xticks, xlabs, _ = _x_scale_spec(xa, allx, left, right, _tz_of(panel["series"]) if time else None)
    # the y grid, its labels, and the x ticks
    for t in ticks:
        y = sy(t)
        s.line(left, y, right, y, c["grid"], 1)
        s.text(left - 6, y + 4, fmt(t, kind, step or None, tick=True), size=10 if small else 11, anchor="end",
               tabular=True)
    for xv, lab in zip(xticks, xlabs):
        x = sx(xv)
        s.line(x, bottom, x, bottom + 4, c["axis"], 1)
        if xlabels:
            s.text(x, bottom + 16, lab, size=10 if small else 11, anchor="middle", tabular=True)
    s.line(left, bottom, right, bottom, c["axis"], 1)
    if ya.get("label") and not small:
        s.text(PAD, top - 22 if events else top - 8, ya["label"], size=11)
    # events: hairlines, numbered at the top
    for ex, num in events:
        if left <= sx(ex) <= right:
            x = sx(ex)
            s.group(f"{num}: {f.keys[num - 1]}")
            s.line(x, top - 4, x, bottom, c["muted"], 1, opacity=0.8)
            s.circle(x, top - 11, 7, c["surface"], ring=False, stroke=c["ink2"], stroke_width=1)
            s.text(x, top - 7.5, num, size=9, anchor="middle", fill=c["ink"])
            s.end()
    # limits: ink reference lines, labelled at the right end
    for lm in panel.get("limits") or []:
        y = sy(lm["y"])
        s.group(f'{lm["label"]}: {fmt(lm["y"], kind)}')
        s.line(left, y, right, y, c["ink2"], 1.25)
        s.end()
    # series, clipped to the plot: a value past the axis shows as an arrowhead at the edge, never over a neighbour
    cid = f.clip_id()
    s.add(f'<clipPath id="{cid}"><rect x="{left - 5:.1f}" y="{top - 5:.1f}" width="{right - left + 10:.1f}" '
          f'height="{bottom - top + 10:.1f}"/></clipPath>')
    s.add(f'<g clip-path="url(#{cid})">')
    off: list[tuple[float, float, str, str]] = []
    for sr in panel["series"]:
        col = arm_color(sr["arm"], mode)
        style = sr.get("style", "line")
        for xv, p in zip(_xs(sr["points"], time), sr["points"]):
            if p[1] is not None and (p[1] > hi * 1.0001 or p[1] < lo * 0.9999):
                off.append((xv, p[1], col, sr.get("label", "")))
        pts = [(x, p[1]) for x, p in zip(_xs(sr["points"], time), sr["points"])]
        tips = sr.get("tips") or [None] * len(pts)
        if style in ("line", "both"):
            yv = sorted(y for _, y in pts if y is not None)
            if yv:
                s.group(f'{sr.get("label", "")}: {len(yv)} points, lowest {fmt(yv[0], kind)}, '
                        f'median {fmt(yv[len(yv) // 2], kind)}, highest {fmt(yv[-1], kind)}')
            seg: list[str] = []
            for x, y in pts:
                if y is None:
                    if len(seg) > 1:
                        s.add(f'<polyline points="{" ".join(seg)}" fill="none" stroke="{col}" stroke-width="2" '
                              'stroke-linejoin="round" stroke-linecap="round"/>')
                    seg = []
                    continue
                seg.append(f"{sx(x):.1f},{sy(y):.1f}")
            if len(seg) > 1:
                s.add(f'<polyline points="{" ".join(seg)}" fill="none" stroke="{col}" stroke-width="2" '
                      'stroke-linejoin="round" stroke-linecap="round"/>')
            if yv:
                s.end()
        dense = len(pts) > 60
        for (x, y), tip, p in zip(pts, tips, sr["points"]):
            if y is None:
                continue
            label = f'{sr.get("label", "")}: {fmt(y, kind)}' + (f" at {p[0]}" if time else "") + (f" ({tip})" if tip else "")
            if style in ("dots", "both"):
                s.group(label)
                s.circle(sx(x), sy(y), 2.6 if dense else 4, col, ring=not dense, stroke_width=1.5 if dense else 2,
                         opacity=0.85 if dense else None)
                s.end()
            elif not dense:
                s.group(label)
                s.hit(sx(x), sy(y), 8)
                s.end()
    s.add("</g>")
    # the limits' labels over the series, on a surface halo, so a series crossing one never hides it
    for lm in panel.get("limits") or []:
        s.text(right - 2, sy(lm["y"]) - 4, lm["label"], size=10 if small else 11, anchor="end", fill=c["ink2"],
               halo=True)
    for xv, yv, col, lab in off:
        over = yv > hi
        ey = top if over else bottom
        tip = "up" if over else "down"
        s.group(f"{lab}: {fmt(yv, kind)} (off the scale, {tip})")
        d = (f"M{sx(xv) - 4:.1f},{ey + 2:.1f}l4,-7l4,7Z" if over else f"M{sx(xv) - 4:.1f},{ey - 2:.1f}l4,7l4,-7Z")
        s.add(f'<path d="{d}" fill="{col}" stroke="{c["surface"]}" stroke-width="1"/>')
        s.end()
    # marks: status points with a label (a mark past the axis sits on its edge; its hover says the value)
    for m in panel.get("marks") or []:
        x = parse_time(m["x"]).timestamp() if time else float(m["x"])
        col = STATUS[m.get("status", "critical")]
        my = min(max(m["y"], lo), hi)
        edge = " (off the scale)" if my != m["y"] else ""
        s.group((m.get("label") or m.get("status", "")) + f': {fmt(m["y"], kind)}{edge}'
                + (f' ({m["tip"]})' if m.get("tip") else ""))
        s.hit(sx(x), sy(my), 10)
        s.circle(sx(x), sy(my), 4.5 if small else 5, col)
        s.end()
        if m.get("label") and m.get("show_label", True):
            lx = sx(x) + 8
            anchor = "start"
            if lx + text_width(m["label"], 10) > right:
                lx, anchor = sx(x) - 8, "end"
            s.text(lx, sy(my) + 4, m["label"], size=10, anchor=anchor, fill=c["ink2"], halo=True)


def _event_numbers(f: Frame, events: list[dict[str, Any]], time: bool) -> list[tuple[float, int]]:
    out = []
    for ev in events:
        f.keys.append(ev["label"])
        x = parse_time(ev["x"]).timestamp() if time else float(ev["x"])
        out.append((x, len(f.keys)))
    return out


def _marks_legend(f: Frame, panels: list[dict[str, Any]], labels: dict[str, str] | None) -> None:
    statuses = []
    for p in panels:
        for m in p.get("marks") or []:
            st = m.get("status", "critical")
            if st not in statuses:
                statuses.append(st)
    for st in statuses:
        f.add_legend("status-" + st, STATUS[st], (labels or {}).get(st, st))


def _lines(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    xa = spec.get("x", {})
    time = xa.get("type") == "time"
    _arms_legend_series(f, spec["series"], "line" if any(sr.get("style", "line") != "dots" for sr in spec["series"]) else "dot")
    _marks_legend(f, [spec], spec.get("mark_labels"))
    events = _event_numbers(f, spec.get("events") or [], time)
    top = f.header_height() + (34 if events else 16)
    plot_h = int(spec.get("height_plot") or 260)
    ys = [p[1] for sr in spec["series"] for p in sr["points"] if p[1] is not None] + \
         [lm["y"] for lm in spec.get("limits") or []]
    _, _, ticks, step = axis_domain(ys, spec.get("y", {}))
    left = PAD + _tick_width(ticks, spec.get("y", {}).get("format"), step) + 10
    right = f.w - PAD - 6
    bottom = top + plot_h
    _draw_lines_panel(f, spec, xa, (left, top, right, bottom), events, small=False, xlabels=True)
    if xa.get("label"):
        f.svg.text((left + right) / 2, bottom + 34, xa["label"], size=12, anchor="middle")
    return f.finish(bottom + 42 - f.header_height())


def _multiples(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    xa = spec.get("x", {})
    time = xa.get("type") == "time"
    panels = spec["panels"]
    cols = int(spec.get("columns") or 2)
    series_all = [sr for p in panels for sr in p["series"]]
    seen: dict[str, dict[str, Any]] = {}
    for sr in series_all:
        seen.setdefault(sr.get("label") or sr["arm"], sr)
    if len(seen) >= 2:
        for label, sr in seen.items():
            f.add_legend("line" if sr.get("style", "line") != "dots" else "dot", arm_color(sr["arm"], mode), label)
    _marks_legend(f, panels, spec.get("mark_labels"))
    events = _event_numbers(f, spec.get("events") or [], time)
    gap_x, gap_y = 26, 30
    pw = (f.w - 2 * PAD - (cols - 1) * gap_x) / cols
    ph = int(spec.get("panel_height") or 150)
    top0 = f.header_height() + 4
    allx = [x for p in panels for sr in p["series"] for x in _xs(sr["points"], time)]
    rows = math.ceil(len(panels) / cols)
    head = 42 if events or any(p.get("events") for p in panels) else 28
    for i, p in enumerate(panels):
        r, k = divmod(i, cols)
        x0 = PAD + k * (pw + gap_x)
        y0 = top0 + r * (ph + head + 26 + gap_y)
        f.svg.text(x0, y0 + 12, p.get("title", ""), size=12, fill=f.c["ink"], weight=600)
        ys = [q[1] for sr in p["series"] for q in sr["points"] if q[1] is not None] + \
             [lm["y"] for lm in p.get("limits") or []]
        _, _, ticks, step = axis_domain(ys, p.get("y", {}))
        left = x0 + _tick_width(ticks, p.get("y", {}).get("format"), step) + 8
        right = x0 + pw - 4
        box = (left, y0 + head, right, y0 + head + ph)
        tz = _tz_of([sr for q in panels for sr in q["series"]]) if time else None
        sx, xticks, xlabs, _ = _x_scale_spec(xa, allx, left, right, tz)
        own = _event_numbers(f, p.get("events") or [], time)
        _draw_lines_panel(f, p, xa, box, events + own, small=True, xlabels=True, x_override=(sx, xticks, xlabs))
    body_bottom = top0 + rows * (ph + head + 26 + gap_y) - gap_y + 8
    if xa.get("label"):
        f.svg.text(f.w / 2, body_bottom + 4, xa["label"], size=12, anchor="middle")
        body_bottom += 12
    return f.finish(body_bottom - f.header_height() + 6)


# ------------------------------------------------------------------ scatter

def pareto_front(points: list[tuple[float, float]]) -> set[int]:
    """The indices no other point dominates (as little x, as much y, and better in one)."""
    out = set()
    for i, (x, y) in enumerate(points):
        if not any((x2 <= x and y2 >= y) and (x2 < x or y2 > y) for j, (x2, y2) in enumerate(points) if j != i):
            out.add(i)
    return out


def _scatter(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    pts = spec["points"]
    xa, ya = spec.get("x", {}), spec.get("y", {})
    _arms_legend(f, [p["arm"] for p in pts], "dot")
    front = pareto_front([(p["x"], p["y"]) for p in pts]) if spec.get("front") else set()
    if front:
        f.add_legend("ring", c["ink2"], "on the Pareto front (none does better on both)")
    xs = [p[k] for p in pts for k in ("x", "xlo", "xhi") if p.get(k) is not None]
    ys = [p[k] for p in pts for k in ("y", "ylo", "yhi") if p.get(k) is not None]
    xlo, xhi, xticks, xstep = axis_domain(xs, xa)
    ylo, yhi, yticks, ystep = axis_domain(ys, ya)
    top = f.header_height() + 12
    plot_h = int(spec.get("height_plot") or 300)
    left = PAD + _tick_width(yticks, ya.get("format"), ystep) + 10
    right = f.w - PAD - 10
    bottom = top + plot_h
    sx = Scale(xlo, xhi, left, right, xa.get("log", False))
    sy = Scale(ylo, yhi, bottom, top, ya.get("log", False))
    _y_axis(s, sy, yticks, ya.get("format"), ystep, left, right, ya.get("label"), c, top)
    for t in xticks:
        s.line(sx(t), top, sx(t), bottom, c["grid"], 1)
        s.text(sx(t), bottom + 16, fmt(t, xa.get("format"), xstep or None, tick=True), size=11, anchor="middle",
               tabular=True)
    s.line(left, bottom, right, bottom, c["axis"], 1)
    if xa.get("label"):
        s.text((left + right) / 2, bottom + 34, xa["label"], size=12, anchor="middle")
    if len(front) > 1:
        fr = sorted((pts[i] for i in front), key=lambda p: p["x"])
        d = f"M{sx(fr[0]['x']):.1f},{sy(fr[0]['y']):.1f}"
        for a, b in zip(fr, fr[1:]):
            d += f"H{sx(b['x']):.1f}V{sy(b['y']):.1f}"
        s.add(f'<path d="{d}" fill="none" stroke="{c["ink2"]}" stroke-width="1.25"/>')
    placed: list[tuple[float, float, float, float]] = []
    for p in pts:
        col = arm_color(p["arm"], mode)
        cx, cy = sx(p["x"]), sy(p["y"])
        tip = f'{p["label"]}: {fmt(p["y"], ya.get("format"))} at {fmt(p["x"], xa.get("format"))}'
        if p.get("tip"):
            tip += f' ({p["tip"]})'
        s.group(tip)
        if p.get("xlo") is not None and p.get("xhi") is not None:
            s.line(sx(p["xlo"]), cy, sx(p["xhi"]), cy, col, 1.5, cap="round")
            placed.append((sx(p["xlo"]), cy - 2, sx(p["xhi"]), cy + 2))
        if p.get("ylo") is not None and p.get("yhi") is not None:
            s.line(cx, sy(p["ylo"]), cx, sy(p["yhi"]), col, 1.5, cap="round")
            placed.append((cx - 2, sy(p["yhi"]), cx + 2, sy(p["ylo"])))
        s.hit(cx, cy)
        s.circle(cx, cy, 6, col)
        s.end()
        if pts.index(p) in front:
            s.circle(cx, cy, 10, "none", ring=False, stroke=c["ink2"], stroke_width=1.25)
        placed.append((cx - 11, cy - 11, cx + 11, cy + 11))
    for p in pts:
        cx, cy = sx(p["x"]), sy(p["y"])
        w = text_width(p["label"], 11)
        for dx, dy, anchor in ((10, -8, "start"), (10, 14, "start"), (-10, -8, "end"), (-10, 14, "end"),
                               (0, -14, "middle"), (0, 22, "middle")):
            x0 = cx + dx - (w if anchor == "end" else w / 2 if anchor == "middle" else 0)
            box = (x0, cy + dy - 10, x0 + w, cy + dy + 3)
            if box[0] < PAD or box[2] > f.w - PAD or any(_overlap(box, b) for b in placed):
                continue
            s.text(cx + dx, cy + dy, p["label"], size=11, anchor=anchor, fill=c["ink"], halo=True)
            placed.append(box)
            break
        else:  # nowhere free: the first place, overlapping, beats a point with no name
            s.text(cx + 10, cy - 8, p["label"], size=11, fill=c["ink"], halo=True)
    return f.finish(bottom + 42 - f.header_height())


def _overlap(a: tuple[float, float, float, float], b: tuple[float, float, float, float]) -> bool:
    return not (a[2] <= b[0] or b[2] <= a[0] or a[3] <= b[1] or b[3] <= a[1])


# ------------------------------------------------------------------ strip

def _jitter(i: int, salt: str) -> float:
    h = hashlib.sha256(f"{salt}:{i}".encode()).digest()
    return (h[0] / 255.0) * 2 - 1


def _strip(spec: dict[str, Any], mode: str) -> str:
    from stats_shim import quantile  # noqa: the sibling module, by path (see the bottom of this file)
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    groups = spec["groups"]
    xa = spec.get("x", {})
    kind = xa.get("format")
    _arms_legend(f, [g["arm"] for g in groups], "dot")
    vals = [v for g in groups for v in g["values"] if v is not None]
    if xa.get("log"):
        vals = [v for v in vals if v > 0]
    lo, hi, ticks, step = axis_domain(vals, xa)
    top = f.header_height() + 6
    label_w = max(text_width(g["label"], 12) for g in groups) + 14
    texts = []
    for g in groups:
        v = [x for x in g["values"] if x is not None]
        texts.append(f'median {fmt(quantile(v, 0.5), kind)} · n {len(v)}' if v else "n 0")
    right_w = max(text_width(t, 11) for t in texts) + 14
    left, right = PAD + label_w, f.w - PAD - right_w
    sc = Scale(lo, hi, left, right, xa.get("log", False))
    row_h = 46
    bottom = top + row_h * len(groups)
    for t in ticks:
        s.line(sc(t), top, sc(t), bottom, c["grid"], 1)
    for gi, (g, txt) in enumerate(zip(groups, texts)):
        cy = top + gi * row_h + row_h / 2
        col = arm_color(g["arm"], mode)
        s.text(left - 14, cy + 4, g["label"], size=12, anchor="end", fill=c["ink"])
        v = [x for x in g["values"] if x is not None and (not xa.get("log") or x > 0)]
        if v:
            q1, q2, q3 = quantile(v, 0.25), quantile(v, 0.5), quantile(v, 0.75)
            s.group(f'{g["label"]}: quartiles {fmt(q1, kind)}, {fmt(q2, kind)}, {fmt(q3, kind)}')
            s.add(f'<rect x="{sc(q1):.1f}" y="{cy - 2:.1f}" width="{max(1.0, sc(q3) - sc(q1)):.1f}" height="4" rx="2" '
                  f'fill="{c["ink2"]}" fill-opacity="0.35"/>')
            s.end()
        tips = g.get("tips") or [None] * len(g["values"])
        for i, (x, tip) in enumerate(zip(g["values"], tips)):
            if x is None or (xa.get("log") and x <= 0):
                continue
            s.group(f'{g["label"]}: {fmt(x, kind)}' + (f" ({tip})" if tip else ""))
            s.circle(sc(x), cy + _jitter(i, g["label"]) * 12, 3.2, col, ring=False, opacity=0.75)
            s.end()
        if v:
            s.group(f'{g["label"]}: median {fmt(q2, kind)}')
            s.line(sc(q2), cy - 14, sc(q2), cy + 14, c["ink"], 2, cap="round")
            s.end()
        s.text(right + 14, cy + 4, txt, size=11, tabular=True)
    _x_axis(s, sc, ticks, kind, step, top, bottom, xa.get("label"), c, grid=False)
    f.add_legend("ref", c["ink"], "median")
    return f.finish(bottom + 42 - f.header_height())


# ------------------------------------------------------------------ matrix

_STATE_DEFAULT = {"P": "solved", "F": "not solved", "E": "ended in an error class", "-": "no result"}


def _matrix(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    cols, rows = spec["columns"], spec["rows"]
    labels = {**_STATE_DEFAULT, **(spec.get("state_labels") or {})}
    used = {ch for r in rows for ch in r["cells"]}
    _arms_legend(f, [col["arm"] for col in cols], "dot")
    for code, kind in (("P", "dot"), ("F", "ring"), ("E", "slash"), ("-", "dash")):
        if code in used:
            f.add_legend(kind, c["ink2"], labels[code])
    cell = 15
    label_w = max(text_width(r["label"], 11) for r in rows) + 12
    note_w = max((text_width(r.get("note", ""), 11) for r in rows), default=0)
    # columns, a gap between arms
    xs = []
    x = PAD + label_w + 8
    prev = None
    for col in cols:
        if prev is not None and col["arm"] != prev:
            x += 8
        xs.append(x + cell / 2)
        x += cell
        prev = col["arm"]
    grid_right = x
    # A label wider than its column is drawn on a slant, so neighbours never overprint; the header grows to hold it.
    widest = max((text_width(col["label"], 10) for col in cols), default=0)
    slant = widest > cell - 1
    top = f.header_height() + 18 + (widest * 0.71 if slant else 0)
    for col, cx in zip(cols, xs):
        if slant:
            s.text(cx - 2, top - 5, col["label"], size=10, fill=c["ink2"], rotate=-45)
        else:
            s.text(cx, top - 6, col["label"], size=10, anchor="middle", fill=c["ink2"])
    for ri, r in enumerate(rows):
        cy = top + ri * cell + cell / 2
        if ri % 2 == 1:
            s.add(f'<rect x="{PAD:.1f}" y="{cy - cell / 2:.1f}" width="{grid_right - PAD + note_w + 12:.1f}" '
                  f'height="{cell}" fill="{c["grid"]}" fill-opacity="0.45"/>')
        s.text(PAD + label_w, cy + 4, r["label"], size=11, anchor="end", fill=c["ink"])
        tips = r.get("tips") or [None] * len(cols)
        for col, cx, code, tip in zip(cols, xs, r["cells"], tips):
            colr = arm_color(col["arm"], mode)
            s.group(f'{r["label"]} · {col["label"]}: {labels.get(code, code)}' + (f" ({tip})" if tip else ""))
            s.add(f'<rect x="{cx - cell / 2:.1f}" y="{cy - cell / 2:.1f}" width="{cell}" height="{cell}" fill="transparent"/>')
            if code == "P":
                s.circle(cx, cy, 4.6, colr, ring=False)
            elif code == "F":
                s.circle(cx, cy, 4, c["surface"], ring=False, stroke=colr, stroke_width=1.5)
            elif code == "E":
                s.circle(cx, cy, 4, c["surface"], ring=False, stroke=colr, stroke_width=1.5)
                s.line(cx - 3.4, cy + 3.4, cx + 3.4, cy - 3.4, colr, 1.5)
            else:
                s.line(cx - 3, cy, cx + 3, cy, c["muted"], 1.5)
            s.end()
        if r.get("note"):
            s.text(grid_right + 10, cy + 4, r["note"], size=11, tabular=True)
    bottom = top + len(rows) * cell
    return f.finish(bottom + 6 - f.header_height())


# ------------------------------------------------------------------ dumbbell

def _dumbbell(spec: dict[str, Any], mode: str) -> str:
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    arm = spec["arm"]
    xa = spec.get("x", {})
    kind = xa.get("format")
    full, light = arm_color(arm, mode), tint(arm, mode)
    f.add_legend("dot", light, spec.get("from_label", "before"))
    f.add_legend("dot", full, spec.get("to_label", "after"))
    rows = spec["rows"]
    vals = [r[k] for r in rows for k in ("from", "to") if r.get(k) is not None]
    lo, hi, ticks, step = axis_domain(vals, xa)
    top = f.header_height() + 4
    label_w = max(text_width(r["label"], 12) for r in rows) + 14
    texts = [f'{fmt(r["from"], kind)} → {fmt(r["to"], kind)}' for r in rows]
    val_w = max(text_width(t, 11) for t in texts) + 14
    left, right = PAD + label_w, f.w - PAD - val_w
    sc = Scale(lo, hi, left, right, xa.get("log", False))
    row_h = 28
    bottom = top + row_h * len(rows) + 4
    for t in ticks:
        s.line(sc(t), top, sc(t), bottom, c["grid"], 1)
    for i, (r, txt) in enumerate(zip(rows, texts)):
        cy = top + i * row_h + 14
        s.text(left - 14, cy + 4, r["label"], size=12, anchor="end", fill=c["ink"])
        s.group(f'{r["label"]}: {txt}' + (f' ({r["tip"]})' if r.get("tip") else ""))
        if r.get("from") is not None and r.get("to") is not None:
            s.line(sc(r["from"]), cy, sc(r["to"]), cy, light, 3, cap="round")
        if r.get("from") is not None:
            s.circle(sc(r["from"]), cy, 5, light)
        if r.get("to") is not None:
            s.hit(sc(r["to"]), cy)
            s.circle(sc(r["to"]), cy, 5, full)
        s.end()
        s.text(right + 14, cy + 4, txt, size=11, tabular=True)
    _x_axis(s, sc, ticks, kind, step, top, bottom, xa.get("label"), c, grid=False)
    return f.finish(bottom + 42 - f.header_height())


# ------------------------------------------------------------------ the palette itself

def palette_svg(mode: str) -> str:
    """The house palette as a figure: every arm's colour in this mode in slot order, then the neutrals and the status
    colours (docs/benchmarks/README.md shows it, both modes)."""
    spec = {"name": "palette", "title": "The house palette" + (" (dark mode)" if mode == "dark" else " (light mode)"),
            "question": "Which colour is each arm, in every report?", "width": 720}
    f = Frame(spec, mode)
    s, c = f.svg, f.c
    top = f.header_height()
    rows = [(f"{slot}", key, name, SLOTS[mode][slot - 1]) for key, (slot, name) in ARMS.items()]
    rows += [("–", key, name, CHROME[mode][tok]) for key, (tok, name) in NEUTRALS.items()]
    col_w = (f.w - 2 * PAD) / 2
    for i, (slot, key, name, hexv) in enumerate(rows):
        x = PAD + (i // 6) * col_w
        y = top + (i % 6) * 40
        s.group(f"{key}: {hexv}")
        s.add(f'<rect x="{x:.1f}" y="{y:.1f}" width="34" height="30" rx="4" fill="{hexv}"/>')
        s.end()
        s.text(x + 44, y + 12, f"{slot}  {key}", size=12, fill=c["ink"], weight=600)
        s.text(x + 44, y + 27, f"{name} · {hexv}", size=11)
    y0 = top + 6 * 40 + 6
    s.text(PAD, y0 + 12, "Status (reserved for good or bad, always with a glyph and a key):", size=11, fill=c["ink2"])
    x = PAD
    for name, hexv in STATUS.items():
        x0 = x
        s.group(f"{name}: {hexv}")
        s.circle(x0 + 7, y0 + 32, 6, hexv, ring=False)
        s.end()
        s.text(x0 + 20, y0 + 36, f"{name} {hexv}", size=11)
        x += 24 + text_width(f"{name} {hexv}", 11) + 20
    return f.finish(y0 + 50 - top)


# ------------------------------------------------------------------ the entry points

FORMS = {"intervals": _intervals, "bars": _bars, "lines": _lines, "multiples": _multiples, "scatter": _scatter,
         "strip": _strip, "matrix": _matrix, "dumbbell": _dumbbell}

_NAME = re.compile(r"^[a-z0-9][a-z0-9-]*$")


def check(spec: dict[str, Any]) -> None:
    """Refuse a spec the renderer can't draw honestly: no name, an unknown form or arm, no question."""
    for k in ("name", "form", "title", "question"):
        if not spec.get(k):
            raise ValueError(f"a figure needs its {k}: {json.dumps(spec)[:120]}")
    if not _NAME.match(spec["name"]) or spec["name"].endswith("-dark"):
        raise ValueError(f"figure name {spec['name']!r}: lower-case letters, digits and dashes, not ending -dark")
    if spec["form"] not in FORMS:
        raise ValueError(f"unknown form {spec['form']!r}; one of {', '.join(FORMS)}")
    for arm in _arms_of(spec):
        arm_color(arm, "light")


def _arms_of(spec: dict[str, Any]) -> list[str]:
    out = []
    for key in ("rows", "series", "points", "groups", "columns"):
        items = spec.get(key)
        for item in items if isinstance(items, list) else []:
            if isinstance(item, dict) and "arm" in item:
                out.append(item["arm"])
    for p in spec.get("panels") or []:
        out += [sr["arm"] for sr in p.get("series") or []]
    if spec.get("form") == "dumbbell":
        out.append(spec["arm"])
    return out


def render(spec: dict[str, Any], mode: str = "light") -> str:
    if mode not in MODES:
        raise ValueError(f"mode {mode!r}: light or dark")
    check(spec)
    return FORMS[spec["form"]](spec, mode)


def write(spec: dict[str, Any], img_dir: Path) -> list[Path]:
    """Both modes of one figure, `<name>.svg` and `<name>-dark.svg`."""
    img_dir.mkdir(parents=True, exist_ok=True)
    out = []
    for mode in MODES:
        p = img_dir / (spec["name"] + ("-dark" if mode == "dark" else "") + ".svg")
        p.write_text(render(spec, mode))
        out.append(p)
    return out


def picture(rel_dir: str, spec: dict[str, Any]) -> str:
    """The markdown for one figure: GitHub shows the dark file to a reader in its dark theme."""
    alt = f'{spec["title"]}. {spec["question"]}'
    w = int(spec.get("width") or 720)
    base = f'{rel_dir.rstrip("/")}/{spec["name"]}'
    return ("<picture>\n"
            f'  <source media="(prefers-color-scheme: dark)" srcset="{base}-dark.svg">\n'
            f'  <img alt={quoteattr(alt)} src="{base}.svg" width="{w}">\n'
            "</picture>")


# The strip form needs a quantile; stats.py sits beside this file. Imported by path, so this module works whether it
# is run from the repository's root, from bench/report, or loaded by file path.
def _load_stats():
    import importlib.util
    import sys
    if "stats_shim" in sys.modules:
        return
    p = Path(__file__).resolve().parent / "stats.py"
    spec = importlib.util.spec_from_file_location("stats_shim", p)
    mod = importlib.util.module_from_spec(spec)
    sys.modules["stats_shim"] = mod
    spec.loader.exec_module(mod)


_load_stats()
