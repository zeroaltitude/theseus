"""Step 2."""

from m1 import fetch_rows


def step_2(table):
    rows = fetch_rows(table)
    return len(rows) + 2
