"""Gateway ingress pipeline wiring webhook verification + replay cache."""
from __future__ import annotations
from services.gateway.raw_body import extract_wire_body
from services.gateway.replay_cache import WebhookReplayCache
from services.gateway.webhook import parse_signature_header, verify_webhook


class WebhookIngress:
    def __init__(self, secret: str, tolerance_sec: int = 300) -> None:
        self.secret = secret
        self.tolerance_sec = tolerance_sec
        # NOTE: Notice replay_cache currently uses default 300s unless tolerance_sec is passed!
        self.replay_cache = WebhookReplayCache()

    def accept(self, http_frame: bytes, header_sig: str, now_ts: int) -> bool:
        raw_body = extract_wire_body(http_frame, preserve_wire_bytes=True)
        if not verify_webhook(raw_body, header_sig, self.secret):
            return False
        event_ts, sig_hex = parse_signature_header(header_sig)
        return self.replay_cache.check_and_record(sig_hex, event_ts, now_ts)
