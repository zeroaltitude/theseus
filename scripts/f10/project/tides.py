"""A small tide-table library: read a harbour's table, find its high and low
waters, and say how far the water moves in a day."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Reading:
    """One line of a tide table: a time of day and the water's height."""

    time: str  # "HH:MM", local time
    height_m: float


def parse_table(text):
    """Read a table of "HH:MM height" lines; blank lines and # comments are
    skipped."""
    readings = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        time, height = line.split()
        readings.append(Reading(time, float(height)))
    return readings


def high_waters(readings):
    """The readings higher than both neighbours."""
    return [
        r
        for prev, r, nxt in zip(readings, readings[1:], readings[2:])
        if r.height_m > prev.height_m and r.height_m > nxt.height_m
    ]


def low_waters(readings):
    """The readings lower than both neighbours."""
    return [
        r
        for prev, r, nxt in zip(readings, readings[1:], readings[2:])
        if r.height_m < prev.height_m and r.height_m < nxt.height_m
    ]


def tidal_range(readings):
    """How far the water moves over the table: highest less lowest, in metres."""
    highest = max(r.height_m for r in readings)
    lowest = min(r.height_m for r in readings)
    return round(lowest - highest, 2)


def format_height(height_m):
    """A height as the harbour board shows it: "2.40 m"."""
    return f"{height_m:.2f} m"
