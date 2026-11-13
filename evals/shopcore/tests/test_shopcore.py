from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal
import unittest

from src.auth.token import TOKEN_SEP, TOKEN_VERSION, issueToken
from src.billing.currency import (
    CURRENCY_EXPONENTS,
    UnsupportedCurrencyError,
    fromMinorUnits,
    toMinorUnits,
)
from src.billing.money import MinorUnitScaleError, addAmounts
from src.config.env import (
    MAX_SESSION_TTL_SEC,
    SESSION_TTL_SEC,
    SETTLEMENT_CUTOFF_UTC,
    ConfigBooleanError,
    parseBoolEnv,
)
from src.health.ping import healthCheck
from src.ledger.settlement import (
    SETTLEMENT_FEE_BPS,
    SettlementMismatchError,
    TimezoneNaiveSettlementError,
    summarizeSettlement,
)
from src.middleware.session import (
    SessionTtlError,
    SignatureMismatchError,
    verifySessionCookie,
)
from src.notify.email import HeaderInjectionError, sendReceiptEmail
from src.orders.coupon import NegativeTotalError, applyCoupon
from src.orders.shipping import PostalCodeFormatError, validatePostalCode
from src.orders.state import DoubleRefundError, OrderRecord, OrderState, transitionOrder
from src.workers.webhook import (
    MAX_WEBHOOK_SKEW_SEC,
    WebhookReplayError,
    handleRefundWebhook,
    signWebhookPayload,
    verifyWebhookSig,
)


class ShopCoreTests(unittest.TestCase):
    def test_parse_bool_env_strict(self) -> None:
        for raw in ("1", "true", "TRUE", " yes "):
            self.assertTrue(parseBoolEnv(raw))
        for raw in ("0", "false", "FALSE", " no "):
            self.assertFalse(parseBoolEnv(raw))
        with self.assertRaisesRegex(
            ConfigBooleanError, "invalid boolean value 'disabled'"
        ):
            parseBoolEnv("disabled")

    def test_token_issue_and_session_verify_coupled(self) -> None:
        secret = "top-secret-key"
        cookie = issueToken("usr_42", "admin", secret)
        self.assertTrue(cookie.startswith(f"{TOKEN_VERSION}{TOKEN_SEP}"))
        claims = verifySessionCookie(cookie, secret)
        self.assertEqual(claims["user_id"], "usr_42")
        self.assertEqual(claims["role"], "admin")
        self.assertEqual(claims["ttl_sec"], SESSION_TTL_SEC)

    def test_session_rejects_legacy_delimiter_and_bad_mac(self) -> None:
        secret = "top-secret-key"
        cookie = issueToken("usr_42", "member", secret)
        legacy_delim = cookie.replace("|", ":")
        with self.assertRaisesRegex(
            SignatureMismatchError, "invalid token delimiter or MAC"
        ):
            verifySessionCookie(legacy_delim, secret)
        with self.assertRaisesRegex(
            SignatureMismatchError, "invalid token delimiter or MAC"
        ):
            verifySessionCookie(cookie, "wrong-secret")

    def test_session_rejects_millisecond_ttl(self) -> None:
        secret = "top-secret-key"
        cookie = issueToken("usr_42", "member", secret)
        self.assertEqual(MAX_SESSION_TTL_SEC, 86400)
        with self.assertRaisesRegex(
            SessionTtlError, "ttl 3600000 exceeds max 86400s"
        ):
            verifySessionCookie(cookie, secret, ttl_sec=3_600_000)
        with self.assertRaises(SessionTtlError):
            verifySessionCookie(cookie, secret, ttl_sec=0)

    def test_currency_minor_units_and_money_addition(self) -> None:
        self.assertEqual(CURRENCY_EXPONENTS["USD"], 2)
        self.assertEqual(CURRENCY_EXPONENTS["KRW"], 0)
        self.assertEqual(toMinorUnits("10.50", "USD"), 1050)
        self.assertEqual(toMinorUnits("1000", "KRW"), 1000)
        self.assertEqual(fromMinorUnits(1050, "USD"), Decimal("10.50"))
        self.assertEqual(fromMinorUnits(1000, "KRW"), Decimal("1000"))
        self.assertEqual(addAmounts("0.10", "0.20", "USD"), Decimal("0.30"))
        self.assertEqual(addAmounts("1500", "2500", "KRW"), Decimal("4000"))

    def test_currency_rejects_unknown_and_excess_scale(self) -> None:
        with self.assertRaisesRegex(UnsupportedCurrencyError, "unknown currency VND"):
            toMinorUnits("50000", "VND")
        with self.assertRaises(MinorUnitScaleError):
            toMinorUnits("1000.5", "KRW")
        with self.assertRaises(MinorUnitScaleError):
            toMinorUnits("1.999", "USD")

    def test_settlement_summary_utc_and_zero_exponent_currency(self) -> None:
        self.assertEqual(SETTLEMENT_CUTOFF_UTC, 0)
        settled_at = datetime(2026, 11, 11, 2, 0, tzinfo=timezone.utc)
        report = summarizeSettlement(
            [(10000, "USD"), (10000, "KRW")],
            settled_at,
        )
        self.assertEqual(report["batch_date"], "2026-11-11")
        self.assertEqual(report["totals"]["USD"]["gross"], Decimal("100.00"))
        self.assertEqual(report["totals"]["USD"]["fee"], Decimal("2.90"))
        self.assertEqual(report["totals"]["USD"]["net"], Decimal("97.10"))
        self.assertEqual(report["totals"]["KRW"]["gross"], Decimal("10000"))
        self.assertEqual(report["totals"]["KRW"]["fee"], Decimal("330"))
        self.assertEqual(report["totals"]["KRW"]["net"], Decimal("9670"))

    def test_settlement_rejects_non_utc_and_missing_fee_rate(self) -> None:
        naive_dt = datetime(2026, 11, 11, 0, 0)
        with self.assertRaisesRegex(
            TimezoneNaiveSettlementError, "settled_at must be UTC"
        ):
            summarizeSettlement([(1000, "USD")], naive_dt)

        kst = timezone(timedelta(hours=9))
        with self.assertRaisesRegex(
            TimezoneNaiveSettlementError, "settled_at must be UTC"
        ):
            summarizeSettlement([(1000, "KRW")], datetime(2026, 11, 11, 9, 0, tzinfo=kst))

        self.assertNotIn("VND", SETTLEMENT_FEE_BPS)
        with self.assertRaisesRegex(
            SettlementMismatchError, "missing fee rate for VND"
        ):
            summarizeSettlement(
                [(25000, "VND")],
                datetime(2026, 11, 11, 1, 0, tzinfo=timezone.utc),
            )

    def test_order_partial_and_full_refund_idempotency(self) -> None:
        order = OrderRecord(order_id="ord_500", total_minor=10000)
        order = transitionOrder(order, "evt_101", 4000)
        self.assertEqual(order.state, OrderState.PARTIALLY_REFUNDED)
        self.assertEqual(order.refunded_minor, 4000)

        with self.assertRaisesRegex(
            DoubleRefundError, "duplicate refund event evt_101 for ord_500"
        ):
            transitionOrder(order, "evt_101", 4000)

        order = transitionOrder(order, "evt_102", 6000)
        self.assertEqual(order.state, OrderState.REFUNDED)
        self.assertEqual(order.refunded_minor, 10000)

    def test_webhook_signature_skew_and_refund_idempotency(self) -> None:
        self.assertEqual(MAX_WEBHOOK_SKEW_SEC, 300)
        secret = "whsec_live"
        payload = '{"order_id":"ord_500","refund":2500}'
        ts = 1_700_000_000
        sig = signWebhookPayload(payload, ts, secret)

        verifyWebhookSig(payload, ts, ts + 120, sig, secret)
        with self.assertRaisesRegex(
            WebhookReplayError, "timestamp skew 950s exceeds 300s"
        ):
            verifyWebhookSig(payload, ts, ts + 950, sig, secret)

        order = OrderRecord(order_id="ord_500", total_minor=5000)
        order = handleRefundWebhook(order, "evt_201", 2500, payload, ts, ts + 10, sig, secret)
        self.assertEqual(order.state, OrderState.PARTIALLY_REFUNDED)
        with self.assertRaises(DoubleRefundError):
            handleRefundWebhook(order, "evt_201", 2500, payload, ts, ts + 15, sig, secret)

    def test_coupon_clamps_at_zero_and_rejects_negative_inputs(self) -> None:
        self.assertEqual(applyCoupon(10000, 3000), 7000)
        self.assertEqual(applyCoupon(3000, 5000), 0)
        with self.assertRaises(NegativeTotalError):
            applyCoupon(3000, -500)

    def test_shipping_postal_code_preserves_leading_zero(self) -> None:
        self.assertEqual(validatePostalCode("01234"), "01234")
        with self.assertRaisesRegex(
            PostalCodeFormatError, "postal code 1234 must be 5 digits"
        ):
            validatePostalCode("1234")

    def test_email_rejects_header_injection(self) -> None:
        msg = sendReceiptEmail("Kim Minji", "minji@example.com")
        self.assertEqual(msg["to"], "Kim Minji <minji@example.com>")
        with self.assertRaisesRegex(
            HeaderInjectionError, "newline in recipient header"
        ):
            sendReceiptEmail("Kim\r\nBcc: attacker@evil.com", "minji@example.com")

    def test_health_check_ok(self) -> None:
        self.assertEqual(healthCheck(), {"ok": True, "service": "shopcore"})


if __name__ == "__main__":
    unittest.main()
