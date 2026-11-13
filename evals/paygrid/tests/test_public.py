import unittest
from decimal import Decimal

from services.auth.api_keys import verify_api_key
from services.fx.currency import from_minor_units, to_minor_units
from services.gateway.rate_limit import SlidingWindowRateLimiter
from services.gateway.webhook import sign_webhook_payload, verify_webhook
from services.ledger.idempotency import IdempotencyStore
from services.ledger.settlement import compute_net_payout
from services.tax.vat import calculate_vat_minor
from services.worker.retry_queue import replay_payout_job


class TestPayGridPublic(unittest.TestCase):
    def test_currency_conversions(self) -> None:
        self.assertEqual(to_minor_units("12.34", "USD"), 1234)
        self.assertEqual(to_minor_units("5000", "KRW"), 5000)
        self.assertEqual(to_minor_units("1.250", "BHD"), 1250)
        self.assertEqual(from_minor_units(1250, "BHD"), Decimal("1.25"))

    def test_settlement_payout(self) -> None:
        self.assertEqual(compute_net_payout(10_000, 250, "USD"), Decimal("97.5"))
        self.assertEqual(compute_net_payout(10_000, 250, "KRW"), Decimal("9750"))

    def test_webhook_signature(self) -> None:
        body = b'{"event":"payout.paid","amount":1000}\r\n'
        sig = sign_webhook_payload(body, 1700000000, "whsec_test")
        self.assertTrue(verify_webhook(body, sig, "whsec_test"))
        self.assertFalse(verify_webhook(body, sig, "wrong_secret"))

    def test_idempotency_and_retry(self) -> None:
        store = IdempotencyStore()
        lease = store.acquire("t1", "req-9")
        self.assertTrue(replay_payout_job(store, "t1", "req-9", lease.lease_epoch, "po_1"))
        self.assertFalse(replay_payout_job(store, "t1", "req-9", lease.lease_epoch, "po_1"))

    def test_vat_bankers_rounding(self) -> None:
        self.assertEqual(calculate_vat_minor(125, "20"), 25)
        self.assertEqual(calculate_vat_minor(25, "10"), 2)  # 2.5 -> 2 (ROUND_HALF_EVEN)

    def test_api_key_and_rate_limit(self) -> None:
        secrets = {"acct01": "s3cr3t"}
        kid = verify_api_key("pg_live_acct01.s3cr3t", secrets)
        self.assertEqual(kid, "acct01")
        limiter = SlidingWindowRateLimiter(limit=2, window_sec=60)
        self.assertTrue(limiter.allow(kid, 100))
        self.assertTrue(limiter.allow(kid, 110))
        self.assertFalse(limiter.allow(kid, 120))


if __name__ == "__main__":
    unittest.main()
