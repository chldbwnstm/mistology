from __future__ import annotations


class NegativeTotalError(ValueError):
    pass


def applyCoupon(subtotal_minor: int, discount_minor: int) -> int:
    if subtotal_minor < 0 or discount_minor < 0:
        raise NegativeTotalError(
            f"negative input amount: subtotal={subtotal_minor}, discount={discount_minor}"
        )
    return max(0, subtotal_minor - discount_minor)
