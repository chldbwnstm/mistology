from __future__ import annotations

import hashlib
import hmac

from src.config.env import MAX_SESSION_TTL_SEC, SESSION_TTL_SEC

TOKEN_VERSION = "v2"
TOKEN_SEP = "|"


class SignatureMismatchError(ValueError):
    pass


class SessionTtlError(ValueError):
    pass


def verifySessionCookie(
    cookie: str,
    secret: str,
    ttl_sec: int = SESSION_TTL_SEC,
) -> dict[str, object]:
    if ttl_sec <= 0:
        raise SessionTtlError(f"ttl {ttl_sec} must be positive")
    if ttl_sec > MAX_SESSION_TTL_SEC:
        raise SessionTtlError(
            f"ttl {ttl_sec} exceeds max {MAX_SESSION_TTL_SEC}s"
        )

    parts = cookie.split(TOKEN_SEP)
    if len(parts) != 4 or parts[0] != TOKEN_VERSION:
        raise SignatureMismatchError("invalid token delimiter or MAC")

    version, user_id, role, mac = parts
    if not user_id or not role:
        raise SignatureMismatchError("invalid token delimiter or MAC")

    payload = f"{version}{TOKEN_SEP}{user_id}{TOKEN_SEP}{role}"
    expected_mac = hmac.new(
        secret.encode("utf-8"),
        payload.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()
    if not hmac.compare_digest(mac, expected_mac):
        raise SignatureMismatchError("invalid token delimiter or MAC")

    return {
        "version": version,
        "user_id": user_id,
        "role": role,
        "ttl_sec": ttl_sec,
    }
