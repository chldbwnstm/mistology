"""EU/UK VAT calculation using Decimal ROUND_HALF_EVEN."""
from decimal import Decimal, ROUND_HALF_EVEN


def calculate_vat_minor(net_minor: int, vat_rate_pct: str) -> int:
    rate = Decimal(vat_rate_pct) / Decimal("100")
    vat = (Decimal(net_minor) * rate).quantize(Decimal("1"), rounding=ROUND_HALF_EVEN)
    return int(vat)
