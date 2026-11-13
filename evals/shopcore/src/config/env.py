from __future__ import annotations

SESSION_TTL_SEC = 3600
MAX_SESSION_TTL_SEC = 86400
SETTLEMENT_CUTOFF_UTC = 0

_TRUE_VALUES = {"1", "true", "yes"}
_FALSE_VALUES = {"0", "false", "no"}


class ConfigBooleanError(ValueError):
    pass


def parseBoolEnv(raw: str) -> bool:
    norm = raw.strip().lower()
    if norm in _TRUE_VALUES:
        return True
    if norm in _FALSE_VALUES:
        return False
    raise ConfigBooleanError(f"invalid boolean value {raw!r}")
