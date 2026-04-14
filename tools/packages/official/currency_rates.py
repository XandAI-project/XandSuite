"""
XandSuite Package: Currency Exchange Rates
Fetch live and historical exchange rates between any two currencies using the
free, no-key-required Frankfurter API (data sourced from the European Central
Bank and other official sources).

Tools provided:
  • list_currencies         — list all supported currency codes and names
  • get_exchange_rate       — current rate between any two currencies
  • convert_amount          — convert a monetary amount between currencies
  • compare_against_base    — compare one base currency against many targets
  • get_historical_rate     — look up the rate on a specific past date

No configuration arguments required — install and use immediately.
"""

import json
from datetime import date, datetime, timedelta
from typing import Optional

import requests
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-currency-rates")

# ---------------------------------------------------------------------------
# API client helpers
# ---------------------------------------------------------------------------

_BASE_URL = "https://api.frankfurter.app"
_TIMEOUT = 10  # seconds


def _get(path: str, params: dict = None) -> dict:
    """Make a GET request to the Frankfurter API and return parsed JSON."""
    try:
        resp = requests.get(
            f"{_BASE_URL}{path}",
            params=params or {},
            timeout=_TIMEOUT,
            headers={"Accept": "application/json"},
        )
        resp.raise_for_status()
        return resp.json()
    except requests.exceptions.ConnectionError:
        return {"error": "Cannot reach the Frankfurter API. Check your internet connection."}
    except requests.exceptions.Timeout:
        return {"error": f"Request timed out after {_TIMEOUT}s. Try again later."}
    except requests.exceptions.HTTPError as exc:
        status = exc.response.status_code if exc.response is not None else "?"
        body = exc.response.text[:300] if exc.response is not None else ""
        return {"error": f"HTTP {status}: {body}"}
    except Exception as exc:  # noqa: BLE001
        return {"error": str(exc)}


def _normalize_code(code: str) -> str:
    return code.strip().upper()


def _validate_date(date_str: str) -> tuple[bool, str]:
    """Return (is_valid, normalised_date_or_error_message)."""
    try:
        parsed = datetime.strptime(date_str.strip(), "%Y-%m-%d").date()
        if parsed > date.today():
            return False, "Date is in the future. Only past dates are supported."
        if parsed < date(1999, 1, 4):
            return False, "ECB data starts on 1999-01-04."
        return True, parsed.isoformat()
    except ValueError:
        return False, f"Invalid date format '{date_str}'. Use YYYY-MM-DD."


# ---------------------------------------------------------------------------
# MCP tools
# ---------------------------------------------------------------------------


@mcp.tool()
def list_currencies() -> str:
    """List all currencies supported by the exchange rate service.

    Returns a JSON object mapping currency codes (e.g. "USD") to their full
    names (e.g. "US Dollar").
    """
    data = _get("/currencies")
    if "error" in data:
        return json.dumps(data)
    return json.dumps({
        "count": len(data),
        "currencies": data,
    })


@mcp.tool()
def get_exchange_rate(base: str, target: str) -> str:
    """Get the current exchange rate between two currencies.

    Args:
        base:   Source currency code, e.g. "USD", "EUR", "GBP".
        target: Target currency code, e.g. "BRL", "JPY", "CHF".

    Returns a JSON object with the rate, base, target, and the date the rate
    was last updated.  Example: {"base": "USD", "target": "EUR", "rate": 0.9215,
    "date": "2026-04-11"}
    """
    base_code = _normalize_code(base)
    target_code = _normalize_code(target)

    data = _get("/latest", params={"from": base_code, "to": target_code})
    if "error" in data:
        return json.dumps(data)

    rates = data.get("rates", {})
    if target_code not in rates:
        return json.dumps({
            "error": f"Currency '{target_code}' not found in the response. "
                     "Use list_currencies to see supported codes."
        })

    return json.dumps({
        "base": base_code,
        "target": target_code,
        "rate": rates[target_code],
        "date": data.get("date"),
    })


@mcp.tool()
def convert_amount(amount: float, from_currency: str, to_currency: str) -> str:
    """Convert a monetary amount from one currency to another.

    Args:
        amount:        The numeric amount to convert (e.g. 100.0).
        from_currency: Source currency code, e.g. "USD".
        to_currency:   Target currency code, e.g. "EUR".

    Returns a JSON object with the converted amount rounded to 4 decimal
    places, the exchange rate used, and the date of the rate.
    """
    from_code = _normalize_code(from_currency)
    to_code = _normalize_code(to_currency)

    data = _get("/latest", params={"from": from_code, "to": to_code, "amount": amount})
    if "error" in data:
        return json.dumps(data)

    rates = data.get("rates", {})
    if to_code not in rates:
        return json.dumps({
            "error": f"Currency '{to_code}' not found. Use list_currencies to see supported codes."
        })

    converted = rates[to_code]
    rate = round(converted / amount, 6) if amount != 0 else None

    return json.dumps({
        "from_currency": from_code,
        "to_currency": to_code,
        "original_amount": amount,
        "converted_amount": round(converted, 4),
        "rate": rate,
        "date": data.get("date"),
    })


@mcp.tool()
def compare_against_base(base: str, targets: str) -> str:
    """Compare a base currency against multiple target currencies at once.

    Useful for building a rate table or answering "how does USD compare to
    EUR, GBP, JPY, and BRL today?"

    Args:
        base:    Base currency code, e.g. "USD".
        targets: Comma-separated list of target currency codes,
                 e.g. "EUR,GBP,JPY,BRL,CAD".  Pass "ALL" to fetch every
                 available currency (may return ~30+ entries).

    Returns a JSON object with the base currency and a sorted rate table.
    """
    base_code = _normalize_code(base)
    params: dict = {"from": base_code}

    if targets.strip().upper() != "ALL":
        codes = [_normalize_code(c) for c in targets.split(",") if c.strip()]
        if not codes:
            return json.dumps({"error": "No target currencies provided."})
        params["to"] = ",".join(codes)

    data = _get("/latest", params=params)
    if "error" in data:
        return json.dumps(data)

    rates = data.get("rates", {})
    sorted_rates = dict(sorted(rates.items()))

    return json.dumps({
        "base": base_code,
        "date": data.get("date"),
        "rate_count": len(sorted_rates),
        "rates": sorted_rates,
    })


@mcp.tool()
def get_historical_rate(base: str, target: str, date: str) -> str:
    """Look up the exchange rate between two currencies on a specific past date.

    Args:
        base:   Source currency code, e.g. "USD".
        target: Target currency code, e.g. "EUR".
        date:   Date in YYYY-MM-DD format. Must be on or after 1999-01-04
                (start of ECB data) and not in the future.

    Returns a JSON object with the rate, both currency codes, and the date.
    """
    base_code = _normalize_code(base)
    target_code = _normalize_code(target)

    valid, date_or_error = _validate_date(date)
    if not valid:
        return json.dumps({"error": date_or_error})

    data = _get(f"/{date_or_error}", params={"from": base_code, "to": target_code})
    if "error" in data:
        return json.dumps(data)

    rates = data.get("rates", {})
    if target_code not in rates:
        return json.dumps({
            "error": f"No rate for '{target_code}' on {date_or_error}. "
                     "The ECB may not have published data for that date (weekends/holidays "
                     "use the previous business day)."
        })

    return json.dumps({
        "base": base_code,
        "target": target_code,
        "rate": rates[target_code],
        "date": data.get("date", date_or_error),
        "note": "ECB data: weekends and holidays fall back to the previous business day.",
    })


@mcp.tool()
def get_rate_change(base: str, target: str, days_ago: int = 7) -> str:
    """Calculate how an exchange rate has changed over the last N days.

    Args:
        base:     Source currency code, e.g. "USD".
        target:   Target currency code, e.g. "EUR".
        days_ago: How many calendar days to look back (default: 7, max: 365).
                  The actual historical date used is the nearest past business
                  day at that offset.

    Returns a JSON object with the current rate, the historical rate, the
    absolute change, and the percentage change.
    """
    base_code = _normalize_code(base)
    target_code = _normalize_code(target)

    if days_ago < 1 or days_ago > 365:
        return json.dumps({"error": "days_ago must be between 1 and 365."})

    # Current rate
    current_data = _get("/latest", params={"from": base_code, "to": target_code})
    if "error" in current_data:
        return json.dumps(current_data)

    current_rates = current_data.get("rates", {})
    if target_code not in current_rates:
        return json.dumps({"error": f"Currency '{target_code}' not found."})

    current_rate = current_rates[target_code]
    current_date = current_data.get("date")

    # Historical rate
    past_date = (datetime.today() - timedelta(days=days_ago)).strftime("%Y-%m-%d")
    hist_data = _get(f"/{past_date}", params={"from": base_code, "to": target_code})
    if "error" in hist_data:
        return json.dumps({
            "warning": f"Could not fetch historical rate for {past_date}: {hist_data['error']}",
            "current_rate": current_rate,
            "current_date": current_date,
        })

    hist_rates = hist_data.get("rates", {})
    if target_code not in hist_rates:
        return json.dumps({
            "warning": f"No historical rate for '{target_code}' on {past_date}.",
            "current_rate": current_rate,
            "current_date": current_date,
        })

    hist_rate = hist_rates[target_code]
    hist_date = hist_data.get("date", past_date)

    change = round(current_rate - hist_rate, 6)
    pct_change = round((change / hist_rate) * 100, 4) if hist_rate != 0 else None
    direction = "up" if change > 0 else ("down" if change < 0 else "unchanged")

    return json.dumps({
        "base": base_code,
        "target": target_code,
        "current_rate": current_rate,
        "current_date": current_date,
        "historical_rate": hist_rate,
        "historical_date": hist_date,
        "change": change,
        "percent_change": pct_change,
        "direction": direction,
        "period_days": days_ago,
    })


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    mcp.run()
