from __future__ import annotations


def healthCheck() -> dict[str, object]:
    return {"ok": True, "service": "shopcore"}
