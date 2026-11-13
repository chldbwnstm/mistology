"""Decoupled replay-protection nonce cache for gateway webhook ingress."""
from __future__ import annotations

DEFAULT_REPLAY_WINDOW_SEC = 300


class WebhookReplayCache:
    """Stores seen webhook signatures within the active replay window."""

    def __init__(self, window_sec: int = DEFAULT_REPLAY_WINDOW_SEC) -> None:
        self.window_sec = window_sec
        self._seen: dict[str, int] = {}

    def check_and_record(
        self,
        signature_hex: str,
        event_ts: int,
        now_ts: int,
        window_sec: int | None = None,
    ) -> bool:
        effective_window = self.window_sec if window_sec is None else window_sec
        # Evict expired nonces outside effective_window
        cutoff = now_ts - effective_window
        expired = [k for k, ts in self._seen.items() if ts < cutoff]
        for k in expired:
            del self._seen[k]

        if abs(now_ts - event_ts) > effective_window:
            return False
        if signature_hex in self._seen:
            return False
        self._seen[signature_hex] = event_ts
        return True
