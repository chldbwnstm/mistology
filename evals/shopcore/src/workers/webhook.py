from __future__ import annotations

import hashlib
import hmac

from src.orders.state import OrderRecord, transitionOrder

MAX_WEBHOOK_SKEW_SEC = 300


class WebhookReplayError(ValueError):
    pass


def signWebhookPayload(payload: str, ts_epoch_sec: int, secret: str) -> str:
    msg = f"{ts_epoch_sec}.{payload}".encode("utf-8")
    return hmac.new(secret.encode("utf-8"), msg, hashlib.sha256).hexdigest()


def verifyWebhookSig(
    payload: str,
    ts_epoch_sec: int,
    now_utc_sec: int,
    sig: str,
    secret: str,
) -> None:
    skew = abs(now_utc_sec - ts_epoch_sec)
    if skew > MAX_WEBHOOK_SKEW_SEC:
        raise WebhookReplayError(
            f"timestamp skew {skew}s exceeds {MAX_WEBHOOK_SKEW_SEC}s"
        )
    expected = signWebhookPayload(payload, ts_epoch_sec, secret)
    if not hmac.compare_digest(sig, expected):
        raise WebhookReplayError("invalid webhook signature")


def handleRefundWebhook(
    order: OrderRecord,
    event_id: str,
    refund_minor: int,
    payload: str,
    ts_epoch_sec: int,
    now_utc_sec: int,
    sig: str,
    secret: str,
) -> OrderRecord:
    verifyWebhookSig(payload, ts_epoch_sec, now_utc_sec, sig, secret)
    return transitionOrder(order, event_id, refund_minor)
