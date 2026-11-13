"""Fenced idempotency lease store for ledger mutations."""
from __future__ import annotations
from dataclasses import dataclass


@dataclass(frozen=True)
class IdempotencyLease:
    key: str
    tenant_id: str
    lease_epoch: int
    completed_payout_id: str | None = None


class IdempotencyStore:
    def __init__(self) -> None:
        self._leases: dict[str, IdempotencyLease] = {}

    def acquire(self, tenant_id: str, client_key: str) -> IdempotencyLease:
        scoped = f"{tenant_id}:{client_key}"
        prev = self._leases.get(scoped)
        next_epoch = 1 if prev is None else prev.lease_epoch + 1
        lease = IdempotencyLease(
            key=scoped,
            tenant_id=tenant_id,
            lease_epoch=next_epoch,
            completed_payout_id=prev.completed_payout_id if prev else None,
        )
        self._leases[scoped] = lease
        return lease

    def commit(self, tenant_id: str, client_key: str, expected_epoch: int, payout_id: str) -> bool:
        scoped = f"{tenant_id}:{client_key}"
        cur = self._leases.get(scoped)
        if cur is None or cur.lease_epoch != expected_epoch:
            raise RuntimeError(
                f"StaleIdempotencyEpochError: key={scoped} expected={expected_epoch}"
            )
        if cur.completed_payout_id is not None:
            return False
        self._leases[scoped] = IdempotencyLease(
            key=scoped,
            tenant_id=tenant_id,
            lease_epoch=cur.lease_epoch,
            completed_payout_id=payout_id,
        )
        return True
