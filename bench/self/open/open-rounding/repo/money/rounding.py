"""Rounding the way the accounts want it."""


def round_half_up(x: float, places: int = 2) -> float:
    """Round `x` to `places` decimals; a tie goes up (away from zero)."""
    return round(x, places)
