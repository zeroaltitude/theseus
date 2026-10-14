from .rounding import round_half_up


def invoice_total(lines: list[float], tax_rate: float) -> float:
    """The sum of `lines` plus tax at `tax_rate`, rounded half up to cents."""
    subtotal = sum(lines)
    return round_half_up(subtotal + subtotal * tax_rate, 2)
