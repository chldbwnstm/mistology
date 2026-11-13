"""Webhook HMAC-SHA256 verification."""
from __future__ import annotations
import hashlib
import hmac


def parse_signature_header(header_sig: str) -> tuple[int, str]:
    parts: dict[str, str] = {}
    for item in header_sig.split(","):
        if "=" in item:
            k, v = item.strip().split("=", 1)
            parts[k] = v
    if "t" not in parts or "v1" not in parts:
        raise ValueError("Malformed PayGrid-Signature header")
    return int(parts["t"]), parts["v1"]


def sign_webhook_payload(raw_body: bytes, timestamp: int, secret: str) -> str:
    signed_bytes = f"{timestamp}.".encode("ascii") + raw_body
    digest = hmac.new(secret.encode("utf-8"), signed_bytes, hashlib.sha256).hexdigest()
    return f"t={timestamp},v1={digest}"


def verify_webhook(raw_body: bytes, header_sig: str, secret: str) -> bool:
    """Verify HMAC-SHA256 over exact wire bytes."""
    ts, provided_hex = parse_signature_header(header_sig)
    expected_header = sign_webhook_payload(raw_body, ts, secret)
    _, expected_hex = parse_signature_header(expected_header)
    return hmac.compare_digest(provided_hex, expected_hex)
