"""API key verification and key_id extraction."""
from __future__ import annotations
import hmac

LIVE_PREFIX = "pg_live_"


def parse_api_key(raw_key: str) -> tuple[str, str]:
    """Format: pg_live_<key_id>.<secret_part>"""
    if not raw_key.startswith(LIVE_PREFIX) or "." not in raw_key:
        raise ValueError("Invalid API key format")
    rest = raw_key[len(LIVE_PREFIX) :]
    key_id, secret_part = rest.split(".", 1)
    if not key_id or not secret_part:
        raise ValueError("Incomplete API key")
    return key_id, secret_part


def verify_api_key(raw_key: str, stored_secret_by_id: dict[str, str]) -> str | None:
    key_id, provided_secret = parse_api_key(raw_key)
    expected = stored_secret_by_id.get(key_id)
    if expected is None:
        return None
    if hmac.compare_digest(provided_secret, expected):
        return key_id
    return None
