"""Sliding-window API rate limiter keyed by immutable key_id (never secret rotation token)."""
from collections import deque


class SlidingWindowRateLimiter:
    def __init__(self, limit: int, window_sec: int = 60) -> None:
        self.limit = limit
        self.window_sec = window_sec
        self._hits: dict[str, deque[int]] = {}

    def allow(self, key_id: str, now_ts: int) -> bool:
        q = self._hits.setdefault(key_id, deque())
        cutoff = now_ts - self.window_sec
        while q and q[0] <= cutoff:
            q.popleft()
        if len(q) >= self.limit:
            return False
        q.append(now_ts)
        return True
