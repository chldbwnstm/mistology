"""Background retry worker that replays failed payout jobs with fenced lease epochs."""
from services.ledger.idempotency import IdempotencyStore


def replay_payout_job(
    store: IdempotencyStore,
    tenant_id: str,
    client_key: str,
    lease_epoch: int,
    payout_id: str,
) -> bool:
    return store.commit(tenant_id, client_key, expected_epoch=lease_epoch, payout_id=payout_id)
