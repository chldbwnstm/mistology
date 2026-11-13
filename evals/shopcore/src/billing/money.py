from __future__ import annotations

from decimal import Decimal

from src.billing.currency import (
    MinorUnitScaleError,
    fromMinorUnits,
    toMinorUnits,
)

__all__ = ["MinorUnitScaleError", "addAmounts"]


def addAmounts(a: str, b: str, currency: str) -> Decimal:
    total_minor = toMinorUnits(a, currency) + toMinorUnits(b, currency)
    return fromMinorUnits(total_minor, currency)
