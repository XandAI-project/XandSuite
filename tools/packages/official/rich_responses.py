"""
XandSuite Package: Rich Responses
Generate inline visual content — SVG charts, metric cards, data tables,
and comparison grids — rendered directly in the chat message bubble.

No external dependencies. All SVG/HTML is self-contained and works offline.
No CLI arguments required: install and use immediately.
"""

import json
import math
from html import escape
from typing import Optional
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-rich-responses")

# ---------------------------------------------------------------------------
# Shared constants
# ---------------------------------------------------------------------------

_PALETTE = [
    "#4e9af1", "#f18f4e", "#4ef1a0", "#f14e7a",
    "#c44ef1", "#f1e14e", "#4ef1e8", "#f1714e",
]

# Wrapper style — NO <style> tag, only inline styles on elements so we never
# leak CSS into the host React app.
_WRAP_STYLE = (
    "font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;"
    "color:#e2e8f0;width:100%;margin:8px 0;box-sizing:border-box"
)

_TITLE_STYLE = (
    "font-size:0.8rem;font-weight:600;letter-spacing:0.04em;"
    "text-transform:uppercase;color:#94a3b8;margin:0 0 10px 0;padding:0"
)


def _wrap(inner_html: str) -> str:
    return f'<div style="{_WRAP_STYLE}">{inner_html}</div>'


def _result(html: str, chart_type: str, title: str) -> str:
    return json.dumps({
        "status": "rendered",
        "display": "inline_html",
        "chart_type": chart_type,
        "title": title,
        "html": html,
        "message": "Rendered inline in the chat.",
    })


# ---------------------------------------------------------------------------
# SVG helpers
# ---------------------------------------------------------------------------

def _fmt_label(v: float) -> str:
    if abs(v) >= 1_000_000:
        return f"{v/1_000_000:.1f}M"
    if abs(v) >= 1_000:
        return f"{v/1_000:.1f}K"
    if v == int(v):
        return str(int(v))
    return f"{v:.2f}"


def _svg_axes(
    svg_w: int, svg_h: int,
    pad_l: int, pad_r: int, pad_t: int, pad_b: int,
    y_min: float, y_max: float, y_label: str,
    x_labels: list[str],
) -> tuple[str, float, float, float, float]:
    px0, py0 = pad_l, pad_t
    pw = svg_w - pad_l - pad_r
    ph = svg_h - pad_t - pad_b

    frags = []
    y_steps = 5
    for i in range(y_steps + 1):
        frac = i / y_steps
        y_val = y_min + frac * (y_max - y_min)
        gy = py0 + ph - frac * ph
        frags.append(
            f'<line x1="{px0}" y1="{gy:.1f}" x2="{px0+pw}" y2="{gy:.1f}" '
            f'stroke="#334155" stroke-width="1"/>'
        )
        frags.append(
            f'<text x="{px0-6}" y="{gy+4:.1f}" text-anchor="end" '
            f'fill="#64748b" font-size="10">{_fmt_label(y_val)}</text>'
        )

    n = len(x_labels)
    for i, lbl in enumerate(x_labels):
        step = pw / max(n - 1, 1) if n > 1 else pw / 2
        gx = px0 + (i * step if n > 1 else pw / 2)
        show = max(1, n // 8)
        if i % show == 0 or i == n - 1:
            frags.append(
                f'<text x="{gx:.1f}" y="{py0+ph+14}" text-anchor="middle" '
                f'fill="#64748b" font-size="10" '
                f'transform="rotate(-30,{gx:.1f},{py0+ph+14})">'
                f'{escape(str(lbl))}</text>'
            )

    frags.append(
        f'<line x1="{px0}" y1="{py0}" x2="{px0}" y2="{py0+ph}" '
        f'stroke="#475569" stroke-width="1.5"/>'
    )
    frags.append(
        f'<line x1="{px0}" y1="{py0+ph}" x2="{px0+pw}" y2="{py0+ph}" '
        f'stroke="#475569" stroke-width="1.5"/>'
    )

    if y_label:
        frags.append(
            f'<text x="{py0+ph/2:.0f}" y="-12" text-anchor="middle" '
            f'fill="#94a3b8" font-size="10" '
            f'transform="rotate(-90) translate(-{py0+ph/2:.0f},-12)">'
            f'{escape(y_label)}</text>'
        )

    return "\n".join(frags), px0, py0, pw, ph


# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------

@mcp.tool()
def line_chart(
    title: str,
    x_labels: list[str],
    series: list[dict],
    y_label: str = "",
) -> str:
    """Render an inline-SVG line chart (time-series, trends). Required: title, x_labels (list[str]), series (list of {label, data[], color?}). Optional: y_label. Returns rich-response JSON with inline HTML."""
    if not series or not x_labels:
        return json.dumps({"error": "series and x_labels are required."})

    all_vals = [v for s in series for v in s.get("data", []) if v is not None]
    if not all_vals:
        return json.dumps({"error": "All series data is empty."})

    y_min, y_max = min(all_vals), max(all_vals)
    if y_min == y_max:
        y_min -= 1; y_max += 1
    pad = (y_max - y_min) * 0.08
    y_min -= pad; y_max += pad

    SVG_W, SVG_H = 580, 280
    PAD_L, PAD_R, PAD_T, PAD_B = 52, 16, 20, 48

    axes_svg, px0, py0, pw, ph = _svg_axes(
        SVG_W, SVG_H, PAD_L, PAD_R, PAD_T, PAD_B,
        y_min, y_max, y_label, x_labels,
    )

    series_frags, legend_frags = [], []
    for si, s in enumerate(series):
        data = s.get("data", [])
        color = s.get("color") or _PALETTE[si % len(_PALETTE)]
        label = s.get("label", f"Series {si+1}")
        n = len(x_labels)

        points = []
        for i, v in enumerate(data):
            if v is None:
                continue
            step = pw / max(n - 1, 1) if n > 1 else pw / 2
            sx = px0 + (i * step if n > 1 else pw / 2)
            sy = py0 + ph - ((v - y_min) / (y_max - y_min)) * ph
            points.append((sx, sy))

        if points:
            pts_str = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
            series_frags.append(
                f'<polyline points="{pts_str}" fill="none" stroke="{color}" '
                f'stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>'
            )
            lx, ly = points[-1]
            series_frags.append(f'<circle cx="{lx:.1f}" cy="{ly:.1f}" r="3.5" fill="{color}"/>')

        lx_leg = 10 + si * 110
        legend_frags.append(
            f'<rect x="{lx_leg}" y="0" width="12" height="3" rx="1.5" fill="{color}"/>'
            f'<text x="{lx_leg+16}" y="4" fill="#94a3b8" font-size="10">{escape(label)}</text>'
        )

    svg = (
        f'<svg viewBox="0 0 {SVG_W} {SVG_H}" width="100%" '
        f'style="max-width:{SVG_W}px;display:block">'
        f'{axes_svg}{"".join(series_frags)}</svg>'
    )
    legend = (
        f'<svg width="{SVG_W}" height="14" style="margin-top:6px;display:block">'
        f'{"".join(legend_frags)}</svg>'
    )
    inner = f'<div style="{_TITLE_STYLE}">{escape(title)}</div>{svg}{legend}'
    return _result(_wrap(inner), "line_chart", title)


@mcp.tool()
def bar_chart(
    title: str,
    labels: list[str],
    series: list[dict],
    y_label: str = "",
    horizontal: bool = False,
) -> str:
    """Render an inline-SVG bar chart (categorical, ranked). Required: title, labels (list[str]), series (list of {label, data[], color?}). Optional: y_label, horizontal (bool). Returns rich-response JSON."""
    if not series or not labels:
        return json.dumps({"error": "series and labels are required."})

    all_vals = [v for s in series for v in s.get("data", []) if v is not None]
    if not all_vals:
        return json.dumps({"error": "All series data is empty."})

    y_min, y_max = 0.0, max(all_vals) * 1.1 or 1.0

    SVG_W, SVG_H = 580, 260
    PAD_L, PAD_R, PAD_T, PAD_B = 52, 16, 16, 48

    if horizontal:
        SVG_H = max(220, len(labels) * 30 + 80)
        PAD_L = max(90, max(len(str(l)) for l in labels) * 6 + 8)
        PAD_B = 24

    axes_svg, px0, py0, pw, ph = _svg_axes(
        SVG_W, SVG_H, PAD_L, PAD_R, PAD_T, PAD_B,
        y_min, y_max, y_label,
        labels if not horizontal else [""] * len(labels),
    )

    n_cats = len(labels)
    n_series = len(series)
    group_w = pw / n_cats
    bar_w = max(4.0, (group_w * 0.8) / max(n_series, 1))

    bar_frags, legend_frags = [], []
    for si, s in enumerate(series):
        data = s.get("data", [])
        color = s.get("color") or _PALETTE[si % len(_PALETTE)]
        label = s.get("label", f"Series {si+1}")

        for ci, v in enumerate(data):
            if v is None:
                continue
            if not horizontal:
                gx = px0 + ci * group_w + group_w * 0.1
                bx = gx + si * bar_w
                bh = max(1.0, (v / y_max) * ph)
                by = py0 + ph - bh
                bar_frags.append(
                    f'<rect x="{bx:.1f}" y="{by:.1f}" width="{bar_w:.1f}" '
                    f'height="{bh:.1f}" rx="2" fill="{color}" opacity="0.85"/>'
                )
            else:
                gy = py0 + ci * (ph / n_cats) + (ph / n_cats) * 0.1
                by2 = gy + si * bar_w
                bw = (v / y_max) * pw
                bar_frags.append(
                    f'<rect x="{px0:.1f}" y="{by2:.1f}" width="{bw:.1f}" '
                    f'height="{bar_w:.1f}" rx="2" fill="{color}" opacity="0.85"/>'
                )
                bar_frags.append(
                    f'<text x="{px0-6}" y="{by2+bar_w/2+4:.1f}" '
                    f'text-anchor="end" fill="#94a3b8" font-size="10">'
                    f'{escape(str(labels[ci]))}</text>'
                )
                bar_frags.append(
                    f'<text x="{px0+bw+4:.1f}" y="{by2+bar_w/2+4:.1f}" '
                    f'fill="#94a3b8" font-size="10">{_fmt_label(v)}</text>'
                )

        lx_leg = 10 + si * 120
        legend_frags.append(
            f'<rect x="{lx_leg}" y="0" width="10" height="10" rx="2" fill="{color}"/>'
            f'<text x="{lx_leg+14}" y="9" fill="#94a3b8" font-size="10">{escape(label)}</text>'
        )

    svg = (
        f'<svg viewBox="0 0 {SVG_W} {SVG_H}" width="100%" '
        f'style="max-width:{SVG_W}px;display:block">'
        f'{axes_svg}{"".join(bar_frags)}</svg>'
    )
    legend = (
        f'<svg width="{SVG_W}" height="14" style="margin-top:6px;display:block">'
        f'{"".join(legend_frags)}</svg>'
    )
    inner = f'<div style="{_TITLE_STYLE}">{escape(title)}</div>{svg}{legend}'
    return _result(_wrap(inner), "bar_chart", title)


@mcp.tool()
def pie_chart(
    title: str,
    labels: list[str],
    values: list[float],
    colors: Optional[list[str]] = None,
) -> str:
    """Render an inline-SVG pie/donut chart (proportions). Required: title, labels (list[str]), values (list[float], any scale). Optional: colors (list[str] hex). Returns rich-response JSON."""
    if not labels or not values or len(labels) != len(values):
        return json.dumps({"error": "labels and values must be non-empty and same length."})

    total = sum(v for v in values if v > 0)
    if total == 0:
        return json.dumps({"error": "All values are zero."})

    cx, cy, r, ri = 130, 130, 110, 60
    SVG_W, SVG_H = 360, 260

    slices = []
    angle = -math.pi / 2
    for i, (lbl, val) in enumerate(zip(labels, values)):
        if val <= 0:
            continue
        sweep = (val / total) * 2 * math.pi
        color = (colors[i] if colors and i < len(colors) else None) or _PALETTE[i % len(_PALETTE)]
        slices.append((angle, sweep, color, lbl, val))
        angle += sweep

    path_frags, legend_frags = [], []
    for i, (a0, sw, color, lbl, val) in enumerate(slices):
        a1 = a0 + sw
        large = 1 if sw > math.pi else 0
        x0o = cx + r * math.cos(a0);  y0o = cy + r * math.sin(a0)
        x1o = cx + r * math.cos(a1);  y1o = cy + r * math.sin(a1)
        x0i = cx + ri * math.cos(a1); y0i = cy + ri * math.sin(a1)
        x1i = cx + ri * math.cos(a0); y1i = cy + ri * math.sin(a0)
        d = (
            f"M {x0o:.2f} {y0o:.2f} A {r} {r} 0 {large} 1 {x1o:.2f} {y1o:.2f} "
            f"L {x0i:.2f} {y0i:.2f} A {ri} {ri} 0 {large} 0 {x1i:.2f} {y1i:.2f} Z"
        )
        path_frags.append(
            f'<path d="{d}" fill="{color}" opacity="0.9" stroke="#1e293b" stroke-width="1.5"/>'
        )
        ly = 16 + i * 18
        pct = 100 * val / total
        legend_frags.append(
            f'<rect x="0" y="{ly-10}" width="10" height="10" rx="2" fill="{color}"/>'
            f'<text x="14" y="{ly}" fill="#e2e8f0" font-size="11">'
            f'{escape(str(lbl))} </text>'
            f'<text x="14" y="{ly}" fill="#64748b" font-size="11" dx="{len(escape(str(lbl)))*6.5:.0f}">'
            f'{pct:.1f}%</text>'
        )

    svg = (
        f'<svg viewBox="0 0 {SVG_W} {SVG_H}" width="100%" '
        f'style="max-width:{SVG_W}px;display:block">'
        f'{"".join(path_frags)}'
        f'<g transform="translate({cx + r + 20}, 16)">{"".join(legend_frags)}</g>'
        f'</svg>'
    )
    inner = f'<div style="{_TITLE_STYLE}">{escape(title)}</div>{svg}'
    return _result(_wrap(inner), "pie_chart", title)


@mcp.tool()
def data_table(
    title: str,
    columns: list[str],
    rows: list[list],
    highlight_last_col: bool = False,
) -> str:
    """Render a styled data table inline. Required: title, columns (list[str]), rows (list[list]). Optional: highlight_last_col (bool) to colour +/- deltas. Returns rich-response JSON."""
    if not columns or not rows:
        return json.dumps({"error": "columns and rows are required."})

    th_style = (
        "background:#1e293b;color:#94a3b8;font-weight:600;"
        "padding:7px 10px;text-align:left;border-bottom:1px solid #334155;"
        "letter-spacing:0.03em;font-size:0.75rem;text-transform:uppercase;"
        "white-space:nowrap"
    )
    td_style = "padding:6px 10px;border-bottom:1px solid #1e293b;color:#e2e8f0;font-size:0.82rem"
    td_last_base = f"{td_style};font-weight:600"

    headers = "".join(f'<th style="{th_style}">{escape(str(c))}</th>' for c in columns)
    n_cols = len(columns)
    body_rows = []
    for row in rows:
        cells = []
        for ci, cell in enumerate(row):
            val = str(cell)
            if highlight_last_col and ci == n_cols - 1:
                color = "#4ade80" if "+" in val else ("#f87171" if "-" in val and not val.startswith("-$") else "#e2e8f0")
                style = f"{td_last_base};color:{color}"
            else:
                style = td_style
            cells.append(f'<td style="{style}">{escape(val)}</td>')
        body_rows.append(f'<tr>{"".join(cells)}</tr>')

    table = (
        f'<table style="width:100%;border-collapse:collapse;font-size:0.82rem">'
        f'<thead><tr>{headers}</tr></thead>'
        f'<tbody>{"".join(body_rows)}</tbody>'
        f'</table>'
    )
    inner = (
        f'<div style="{_TITLE_STYLE}">{escape(title)}</div>'
        f'<div style="overflow-x:auto;border-radius:8px;border:1px solid #1e293b">'
        f'{table}</div>'
    )
    return _result(_wrap(inner), "data_table", title)


@mcp.tool()
def metric_cards(
    cards: list[dict],
    title: str = "",
) -> str:
    """Render a row of KPI / metric cards inline (dashboards, quotes). Required: cards (list of {label, value, change?, positive?, sub?}). Optional: title. Returns rich-response JSON."""
    if not cards:
        return json.dumps({"error": "cards list is required."})

    card_style = (
        "background:#1e293b;border:1px solid #334155;border-radius:10px;"
        "padding:12px 14px;box-sizing:border-box"
    )
    label_style = (
        "font-size:0.7rem;color:#64748b;text-transform:uppercase;"
        "letter-spacing:0.05em;margin:0 0 4px 0;padding:0"
    )
    value_style = "font-size:1.25rem;font-weight:700;color:#f1f5f9;line-height:1.2;margin:0;padding:0"
    sub_style = "font-size:0.7rem;color:#64748b;margin:2px 0 0 0;padding:0"

    card_html = []
    for c in cards:
        label = escape(str(c.get("label", "")))
        value = escape(str(c.get("value", "")))
        change = escape(str(c.get("change", "")))
        sub = escape(str(c.get("sub", "")))
        positive = c.get("positive")

        chg_color = "#4ade80" if positive is True else ("#f87171" if positive is False else "#94a3b8")
        chg_style = f"font-size:0.78rem;font-weight:600;margin:3px 0 0 0;padding:0;color:{chg_color}"

        change_html = f'<p style="{chg_style}">{change}</p>' if change else ""
        sub_html = f'<p style="{sub_style}">{sub}</p>' if sub else ""

        card_html.append(
            f'<div style="{card_style}">'
            f'<p style="{label_style}">{label}</p>'
            f'<p style="{value_style}">{value}</p>'
            f'{change_html}{sub_html}'
            f'</div>'
        )

    grid_style = (
        "display:grid;grid-template-columns:repeat(auto-fill,minmax(140px,1fr));"
        "gap:10px;box-sizing:border-box"
    )
    title_html = f'<div style="{_TITLE_STYLE}">{escape(title)}</div>' if title else ""
    inner = f'{title_html}<div style="{grid_style}">{"".join(card_html)}</div>'
    return _result(_wrap(inner), "metric_cards", title)


@mcp.tool()
def comparison_grid(
    title: str,
    items: list[dict],
    attributes: Optional[list[dict]] = None,
) -> str:
    """Render a side-by-side comparison grid (products, stocks, plans). Required: title, items (list of {name, subtitle?, highlight?, attrs dict}). Optional: attributes (list of {key,label}) to pin order. Returns rich-response JSON."""
    if not items:
        return json.dumps({"error": "items list is required."})

    if attributes:
        attr_keys = [(a["key"], a.get("label", a["key"])) for a in attributes]
    else:
        first_attrs = items[0].get("attrs", {}) if items else {}
        attr_keys = [(k, k) for k in first_attrs.keys()]

    n = len(items)
    col_def = f"repeat({n}, 1fr)"
    grid_style = f"display:grid;grid-template-columns:{col_def};gap:10px;box-sizing:border-box"

    item_style_base = (
        "background:#1e293b;border:1px solid #334155;border-radius:10px;"
        "padding:14px;box-sizing:border-box;min-width:0"
    )
    name_style = (
        "font-size:0.75rem;font-weight:700;color:#94a3b8;"
        "letter-spacing:0.05em;text-transform:uppercase;margin:0;padding:0"
    )
    subtitle_style = (
        "font-size:0.9rem;font-weight:700;color:#f1f5f9;margin:2px 0 10px 0;padding:0"
    )
    attr_label_style = (
        "font-size:0.68rem;color:#64748b;text-transform:uppercase;"
        "letter-spacing:0.04em;margin:0;padding:0"
    )

    item_cols = []
    for item in items:
        name = escape(str(item.get("name", "")))
        subtitle = escape(str(item.get("subtitle", "")))
        highlight = item.get("highlight", "#475569")
        attrs = item.get("attrs", {})

        attr_rows = []
        for key, label in attr_keys:
            val = str(attrs.get(key, "—"))
            val_color = "#4ade80" if ("+" in val and not val.startswith("+$")) else \
                        ("#f87171" if (val.startswith("-") and not val.startswith("-$")) else "#e2e8f0")
            attr_val_style = f"font-size:0.88rem;color:{val_color};font-weight:500;margin:0;padding:0"
            attr_rows.append(
                f'<div style="margin-bottom:6px">'
                f'<p style="{attr_label_style}">{escape(label)}</p>'
                f'<p style="{attr_val_style}">{escape(val)}</p>'
                f'</div>'
            )

        item_style = f"{item_style_base};border-top:3px solid {highlight}"
        item_cols.append(
            f'<div style="{item_style}">'
            f'<p style="{name_style}">{name}</p>'
            f'<p style="{subtitle_style}">{subtitle}</p>'
            f'{"".join(attr_rows)}'
            f'</div>'
        )

    inner = (
        f'<div style="{_TITLE_STYLE}">{escape(title)}</div>'
        f'<div style="{grid_style}">{"".join(item_cols)}</div>'
    )
    return _result(_wrap(inner), "comparison_grid", title)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    mcp.run(transport="stdio")
