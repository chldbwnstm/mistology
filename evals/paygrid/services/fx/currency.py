"""FX currency minor-unit conversions for PayGrid."""
from __future__ import annotations
from decimal import Decimal

# v3: Explicit ISO-4217 minor-unit exponent registry (0, 2, or 3 decimals).
CURRENCY_EXPONENTS: dict[str, int] = {
    "USD": 2,
    "EUR": 2,
    "GBP": 2,
    "JPY": 0,
    "KRW": 0,
    "BHD": 3,
}


def get_exponent(currency: str) -> int:
    code = currency.strip().upper()
    if code not in CURRENCY_EXPONENTS:
        raise ValueError(f"Unsupported currency: {currency}")
    return CURRENCY_EXPONENTS[code]


def to_minor_units(amount: Decimal | str, currency: str) -> int:
    exp = get_exponent(currency)
    dec = Decimal(str(amount))
    scale = Decimal(10) ** exp
    scaled = dec * scale
    if scaled != scaled.to_integral_value():
        raise ValueError(f"Precision overflow for {currency} (exponent={exp}): {amount}")
    return int(scaled)


def from_minor_units(minor: int, currency: str) -> Decimal:
    exp = get_exponent(currency)
    scale = Decimal(10) ** exp
    return Decimal(minor) / scale
