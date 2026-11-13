from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal

from src.billing.currency import fromMinorUnits
from src.config.env import SETTLEMENT_CUTOFF_UTC

SETTLEMENT_FEE_BPS: dict[str, int] = {
    "USD": 290,
    "EUR": 250,
    "KRW": 330,
    "JPY": 300,
}


class SettlementMismatchError(ValueError):
    pass


class TimezoneNaiveSettlementError(ValueError):
    pass


def summarizeSettlement(
    items: list[tuple[int, str]],
    settled_at_dt: datetime,
) -> dict[str, object]:
    if settled_at_dt.tzinfo != timezone.utc:
        raise TimezoneNaiveSettlementError("settled_at must be UTC")

    batch_day = settled_at_dt.date()
    if settled_at_dt.hour < SETTLEMENT_CUTOFF_UTC:
        batch_day = batch_day - timedelta(days=1)

    gross_by_currency: dict[str, int] = {}
    for amount_minor, currency in items:
        if currency not in SETTLEMENT_FEE_BPS:
            raise SettlementMismatchError(f"missing fee rate for {currency}")
        gross_by_currency[currency] = (
            gross_by_currency.get(currency, 0) + amount_minor
        )

    totals: dict[str, dict[str, Decimal]] = {}
    for currency, gross_minor in gross_by_currency.items():
        fee_minor = (gross_minor * SETTLEMENT_FEE_BPS[currency]) // 10_000
        net_minor = gross_minor - fee_minor
        totals[currency] = {
            "gross": fromMinorUnits(gross_minor, currency),
            "fee": fromMinorUnits(fee_minor, currency),
            "net": fromMinorUnits(net_minor, currency),
        }

    return {
        "batch_date": batch_day.isoformat(),
        "totals": totals,
    }
