from __future__ import annotations

import hashlib
import hmac

TOKEN_VERSION = "v2"
TOKEN_SEP = "|"


def issueToken(user_id: str, role: str, secret: str) -> str:
    if not user_id or not role or TOKEN_SEP in user_id or TOKEN_SEP in role:
        raise ValueError("invalid token claim")
    payload = f"{TOKEN_VERSION}{TOKEN_SEP}{user_id}{TOKEN_SEP}{role}"
    mac = hmac.new(
        secret.encode("utf-8"),
        payload.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()
    return f"{payload}{TOKEN_SEP}{mac}"
