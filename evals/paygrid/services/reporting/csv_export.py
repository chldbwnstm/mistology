"""Merchant CSV report formatter (downstream of settlement.py)."""
from decimal import Decimal
from services.ledger.settlement import compute_net_payout

REPORT_DECIMALS: dict[str, int] = {
    "USD": 2,
    "EUR": 2,
    "GBP": 2,
    "JPY": 0,
    "KRW": 0,
    "BHD": 3,
}


def format_payout_csv_row(merchant_id: str, gross_minor: int, fee_bps: int, currency: str) -> str:
    code = currency.strip().upper()
    net = compute_net_payout(gross_minor, fee_bps, code)
    places = REPORT_DECIMALS.get(code, 2)
    quant = Decimal(10) ** (-places)
    return f"{merchant_id},{code},{net.quantize(quant)}"
