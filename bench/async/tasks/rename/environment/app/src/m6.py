"""Step 6."""

from m1 import fetch_rows


def step_6(table):
    rows = fetch_rows(table)
    return len(rows) + 6
