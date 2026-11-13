from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum


class DoubleRefundError(ValueError):
    pass


class OrderState(str, Enum):
    PAID = "PAID"
    PARTIALLY_REFUNDED = "PARTIALLY_REFUNDED"
    REFUNDED = "REFUNDED"


@dataclass
class OrderRecord:
    order_id: str
    total_minor: int
    refunded_minor: int = 0
    state: OrderState = OrderState.PAID
    applied_events: set[str] = field(default_factory=set)


def transitionOrder(
    order: OrderRecord,
    event_id: str,
    refund_minor: int,
) -> OrderRecord:
    if not event_id:
        raise ValueError("event_id is required")
    if event_id in order.applied_events:
        raise DoubleRefundError(
            f"duplicate refund event {event_id} for {order.order_id}"
        )
    if refund_minor <= 0 or order.refunded_minor + refund_minor > order.total_minor:
        raise ValueError("invalid refund amount")

    new_refunded = order.refunded_minor + refund_minor
    new_state = (
        OrderState.REFUNDED
        if new_refunded == order.total_minor
        else OrderState.PARTIALLY_REFUNDED
    )
    order.refunded_minor = new_refunded
    order.state = new_state
    order.applied_events.add(event_id)
    return order
