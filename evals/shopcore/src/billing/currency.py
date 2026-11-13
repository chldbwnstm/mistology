from __future__ import annotations

from decimal import Decimal, InvalidOperation

CURRENCY_EXPONENTS: dict[str, int] = {
    "USD": 2,
    "EUR": 2,
    "KRW": 0,
    "JPY": 0,
}


class UnsupportedCurrencyError(ValueError):
    pass


class MinorUnitScaleError(ValueError):
    pass


def toMinorUnits(amount_str: str, currency: str) -> int:
    if currency not in CURRENCY_EXPONENTS:
        raise UnsupportedCurrencyError(f"unknown currency {currency}")
    exp = CURRENCY_EXPONENTS[currency]
    try:
        dec = Decimal(amount_str)
    except InvalidOperation as exc:
        raise MinorUnitScaleError(f"invalid amount {amount_str!r}") from exc
    if not dec.is_finite():
        raise MinorUnitScaleError(f"non-finite amount {amount_str!r}")
    scaled = dec * (Decimal(10) ** exp)
    if scaled != scaled.to_integral_value():
        raise MinorUnitScaleError(
            f"{currency} {amount_str} exceeds exponent {exp}"
        )
    return int(scaled)


def fromMinorUnits(minor: int, currency: str) -> Decimal:
    if currency not in CURRENCY_EXPONENTS:
        raise UnsupportedCurrencyError(f"unknown currency {currency}")
    exp = CURRENCY_EXPONENTS[currency]
    quant = Decimal(10) ** (-exp)
    return (Decimal(minor) * quant).quantize(quant)
