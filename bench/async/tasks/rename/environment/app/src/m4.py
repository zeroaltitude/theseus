"""Step 4."""

from m1 import fetch_rows


def step_4(table):
    rows = fetch_rows(table)
    return len(rows) + 4
