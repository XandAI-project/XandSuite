"""
Tests for tools/packages/official/currency_rates.py

Run with:
    cd "D:/XandNet Project/XandSuite/tools/packages/tests"
    pip install pytest requests
    pytest test_currency_rates.py -v

Two test classes:
  - TestHelpers   : pure-unit tests, no network (mock with unittest.mock)
  - TestIntegration: live API tests against api.frankfurter.app
"""

import json
import sys
import unittest
from unittest.mock import MagicMock, patch

# ---------------------------------------------------------------------------
# Resolve import: allow running from any cwd
# ---------------------------------------------------------------------------
import os

_OFFICIAL_DIR = os.path.join(os.path.dirname(__file__), "..", "official")
sys.path.insert(0, os.path.abspath(_OFFICIAL_DIR))

import currency_rates as cr  # noqa: E402  (path manipulation before import)


# ---------------------------------------------------------------------------
# Helper: build a fake requests.Response
# ---------------------------------------------------------------------------

def _make_response(json_data: dict, status_code: int = 200) -> MagicMock:
    resp = MagicMock()
    resp.status_code = status_code
    resp.json.return_value = json_data
    if status_code >= 400:
        http_err = __import__("requests").exceptions.HTTPError(response=resp)
        resp.raise_for_status.side_effect = http_err
    else:
        resp.raise_for_status.return_value = None
    return resp


# ---------------------------------------------------------------------------
# Unit tests (mocked network)
# ---------------------------------------------------------------------------

class TestHelpers(unittest.TestCase):

    # ── _normalize_code ────────────────────────────────────────────────────

    def test_normalize_code_upper(self):
        self.assertEqual(cr._normalize_code("usd"), "USD")

    def test_normalize_code_strips_spaces(self):
        self.assertEqual(cr._normalize_code("  EUR  "), "EUR")

    # ── _validate_date ─────────────────────────────────────────────────────

    def test_validate_date_valid(self):
        ok, result = cr._validate_date("2023-06-15")
        self.assertTrue(ok)
        self.assertEqual(result, "2023-06-15")

    def test_validate_date_future(self):
        ok, msg = cr._validate_date("2099-01-01")
        self.assertFalse(ok)
        self.assertIn("future", msg.lower())

    def test_validate_date_too_old(self):
        ok, msg = cr._validate_date("1990-01-01")
        self.assertFalse(ok)
        self.assertIn("1999", msg)

    def test_validate_date_bad_format(self):
        ok, msg = cr._validate_date("15-06-2023")
        self.assertFalse(ok)
        self.assertIn("YYYY-MM-DD", msg)

    # ── list_currencies (mocked) ────────────────────────────────────────────

    @patch("currency_rates.requests.get")
    def test_list_currencies_success(self, mock_get):
        mock_get.return_value = _make_response({"USD": "US Dollar", "EUR": "Euro"})
        result = json.loads(cr.list_currencies())
        self.assertEqual(result["count"], 2)
        self.assertIn("USD", result["currencies"])

    @patch("currency_rates.requests.get")
    def test_list_currencies_connection_error(self, mock_get):
        import requests as rq
        mock_get.side_effect = rq.exceptions.ConnectionError
        result = json.loads(cr.list_currencies())
        self.assertIn("error", result)

    # ── get_exchange_rate (mocked) ──────────────────────────────────────────

    @patch("currency_rates.requests.get")
    def test_get_exchange_rate_success(self, mock_get):
        mock_get.return_value = _make_response(
            {"base": "USD", "date": "2026-04-11", "rates": {"EUR": 0.9215}}
        )
        result = json.loads(cr.get_exchange_rate("usd", "eur"))
        self.assertEqual(result["base"], "USD")
        self.assertEqual(result["target"], "EUR")
        self.assertAlmostEqual(result["rate"], 0.9215)
        self.assertEqual(result["date"], "2026-04-11")

    @patch("currency_rates.requests.get")
    def test_get_exchange_rate_unknown_target(self, mock_get):
        mock_get.return_value = _make_response(
            {"base": "USD", "date": "2026-04-11", "rates": {}}
        )
        result = json.loads(cr.get_exchange_rate("USD", "XYZ"))
        self.assertIn("error", result)
        self.assertIn("XYZ", result["error"])

    # ── convert_amount (mocked) ─────────────────────────────────────────────

    @patch("currency_rates.requests.get")
    def test_convert_amount_success(self, mock_get):
        mock_get.return_value = _make_response(
            {"base": "USD", "date": "2026-04-11", "rates": {"BRL": 572.30}}
        )
        result = json.loads(cr.convert_amount(100.0, "USD", "BRL"))
        self.assertEqual(result["from_currency"], "USD")
        self.assertEqual(result["to_currency"], "BRL")
        self.assertAlmostEqual(result["original_amount"], 100.0)
        self.assertAlmostEqual(result["converted_amount"], 572.30, places=2)

    @patch("currency_rates.requests.get")
    def test_convert_amount_zero(self, mock_get):
        mock_get.return_value = _make_response(
            {"base": "USD", "date": "2026-04-11", "rates": {"EUR": 0.0}}
        )
        result = json.loads(cr.convert_amount(0.0, "USD", "EUR"))
        self.assertIsNone(result["rate"])  # division by zero guard

    # ── compare_against_base (mocked) ───────────────────────────────────────

    @patch("currency_rates.requests.get")
    def test_compare_against_base_multi(self, mock_get):
        mock_get.return_value = _make_response(
            {"base": "USD", "date": "2026-04-11", "rates": {"EUR": 0.92, "GBP": 0.78}}
        )
        result = json.loads(cr.compare_against_base("USD", "EUR,GBP"))
        self.assertEqual(result["base"], "USD")
        self.assertEqual(result["rate_count"], 2)
        self.assertIn("EUR", result["rates"])
        self.assertIn("GBP", result["rates"])

    @patch("currency_rates.requests.get")
    def test_compare_against_base_empty_targets(self, mock_get):
        result = json.loads(cr.compare_against_base("USD", "  "))
        self.assertIn("error", result)

    # ── get_historical_rate (mocked) ────────────────────────────────────────

    @patch("currency_rates.requests.get")
    def test_get_historical_rate_success(self, mock_get):
        mock_get.return_value = _make_response(
            {"base": "USD", "date": "2023-01-02", "rates": {"EUR": 0.9380}}
        )
        result = json.loads(cr.get_historical_rate("USD", "EUR", "2023-01-02"))
        self.assertEqual(result["base"], "USD")
        self.assertAlmostEqual(result["rate"], 0.9380)

    def test_get_historical_rate_invalid_date(self):
        result = json.loads(cr.get_historical_rate("USD", "EUR", "01/01/2023"))
        self.assertIn("error", result)

    def test_get_historical_rate_future_date(self):
        result = json.loads(cr.get_historical_rate("USD", "EUR", "2099-12-31"))
        self.assertIn("error", result)

    # ── get_rate_change (mocked) ────────────────────────────────────────────

    @patch("currency_rates.requests.get")
    def test_get_rate_change_up(self, mock_get):
        # First call: current rate, second call: historical rate
        mock_get.side_effect = [
            _make_response({"base": "USD", "date": "2026-04-11", "rates": {"EUR": 0.95}}),
            _make_response({"base": "USD", "date": "2026-04-04", "rates": {"EUR": 0.90}}),
        ]
        result = json.loads(cr.get_rate_change("USD", "EUR", days_ago=7))
        self.assertEqual(result["direction"], "up")
        self.assertAlmostEqual(result["change"], 0.05, places=5)
        self.assertAlmostEqual(result["percent_change"], 5.5556, places=2)

    @patch("currency_rates.requests.get")
    def test_get_rate_change_down(self, mock_get):
        mock_get.side_effect = [
            _make_response({"base": "USD", "date": "2026-04-11", "rates": {"GBP": 0.75}}),
            _make_response({"base": "USD", "date": "2026-04-04", "rates": {"GBP": 0.80}}),
        ]
        result = json.loads(cr.get_rate_change("USD", "GBP", days_ago=7))
        self.assertEqual(result["direction"], "down")
        self.assertLess(result["change"], 0)

    def test_get_rate_change_invalid_days(self):
        result = json.loads(cr.get_rate_change("USD", "EUR", days_ago=0))
        self.assertIn("error", result)

        result2 = json.loads(cr.get_rate_change("USD", "EUR", days_ago=400))
        self.assertIn("error", result2)


# ---------------------------------------------------------------------------
# Integration tests (live network — skipped if offline)
# ---------------------------------------------------------------------------

def _check_online() -> bool:
    try:
        import requests as rq
        rq.get("https://api.frankfurter.app/currencies", timeout=5).raise_for_status()
        return True
    except Exception:
        return False


_ONLINE = _check_online()
_SKIP_MSG = "Skipping — no internet access to api.frankfurter.app"


@unittest.skipUnless(_ONLINE, _SKIP_MSG)
class TestIntegration(unittest.TestCase):

    def test_list_currencies_live(self):
        result = json.loads(cr.list_currencies())
        self.assertNotIn("error", result)
        self.assertGreater(result["count"], 20)
        self.assertIn("USD", result["currencies"])
        self.assertIn("EUR", result["currencies"])
        print(f"\n  [live] list_currencies: {result['count']} currencies returned")

    def test_get_usd_to_eur_live(self):
        result = json.loads(cr.get_exchange_rate("USD", "EUR"))
        self.assertNotIn("error", result)
        self.assertEqual(result["base"], "USD")
        self.assertEqual(result["target"], "EUR")
        self.assertIsInstance(result["rate"], float)
        self.assertGreater(result["rate"], 0)
        print(f"\n  [live] USD → EUR = {result['rate']}  (date: {result['date']})")

    def test_get_eur_to_usd_live(self):
        result = json.loads(cr.get_exchange_rate("EUR", "USD"))
        self.assertNotIn("error", result)
        self.assertGreater(result["rate"], 0)
        print(f"\n  [live] EUR → USD = {result['rate']}")

    def test_convert_100_usd_to_brl_live(self):
        result = json.loads(cr.convert_amount(100.0, "USD", "BRL"))
        self.assertNotIn("error", result)
        self.assertGreater(result["converted_amount"], 0)
        print(f"\n  [live] 100 USD = {result['converted_amount']} BRL  (rate: {result['rate']})")

    def test_compare_usd_against_majors_live(self):
        result = json.loads(cr.compare_against_base("USD", "EUR,GBP,JPY,BRL,CAD,CHF"))
        self.assertNotIn("error", result)
        self.assertEqual(result["base"], "USD")
        self.assertGreaterEqual(result["rate_count"], 1)
        rates = result["rates"]
        for code in ("EUR", "GBP", "JPY"):
            if code in rates:
                self.assertGreater(rates[code], 0)
        print(f"\n  [live] USD vs majors: {json.dumps(result['rates'], indent=2)}")

    def test_historical_rate_2020_live(self):
        result = json.loads(cr.get_historical_rate("USD", "EUR", "2020-01-02"))
        self.assertNotIn("error", result)
        self.assertGreater(result["rate"], 0)
        print(f"\n  [live] USD → EUR on 2020-01-02 = {result['rate']}  (actual date: {result['date']})")

    def test_rate_change_7_days_live(self):
        result = json.loads(cr.get_rate_change("USD", "EUR", days_ago=7))
        self.assertNotIn("error", result)
        self.assertIn("direction", result)
        self.assertIn(result["direction"], ("up", "down", "unchanged"))
        print(
            f"\n  [live] USD/EUR change over 7 days: "
            f"{result['change']:+.6f}  ({result['percent_change']:+.4f}%)  [{result['direction']}]"
        )

    def test_unknown_currency_live(self):
        result = json.loads(cr.get_exchange_rate("USD", "XXX"))
        self.assertIn("error", result)
        print(f"\n  [live] unknown currency test: {result['error'][:80]}")


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    unittest.main(verbosity=2)
