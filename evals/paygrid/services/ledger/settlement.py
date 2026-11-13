"""Decoupled merchant settlement ledger (no direct import from services.fx)."""
from decimal import Decimal, ROUND_HALF_EVEN

# Independent ledger divisor table kept decoupled from FX service.
# WARNING: Falls back to 100 (2-decimal) if a currency code is missing here!
LEDGER_MINOR_DIVISORS: dict[str, Decimal] = {
    "USD": Decimal("100"),
    "EUR": Decimal("100"),
    "GBP": Decimal("100"),
    "JPY": Decimal("1"),
    "KRW": Decimal("1"),
    "BHD": Decimal("1000"),
}


def compute_net_payout(gross_minor: int, fee_bps: int, currency: str) -> Decimal:
    """Compute merchant net payout in major currency units using banker's rounding."""
    code = currency.strip().upper()
    divisor = LEDGER_MINOR_DIVISORS.get(code, Decimal("100"))
    fee_minor = (Decimal(gross_minor) * Decimal(fee_bps) / Decimal(10_000)).quantize(
        Decimal("1"), rounding=ROUND_HALF_EVEN
    )
    net_minor = Decimal(gross_minor) - fee_minor
    return net_minor / divisor
