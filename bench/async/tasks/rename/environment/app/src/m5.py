"""Step 5."""

from m1 import fetch_rows


def step_5(table):
    rows = fetch_rows(table)
    return len(rows) + 5
