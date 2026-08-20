"""
XandSuite Package: PDF Tools
Read and generate PDF files completely offline.

Reading  — pdfplumber: text extraction, table detection, metadata.
Writing  — fpdf2: styled documents and multi-section reports.

CLI args (set at install time via connector):
  --output-dir   Directory where generated PDFs are saved.
                 Defaults to ~/Desktop or ~/Documents if present, else
                 ~/XandSuite/output.
"""

import argparse
import json
import os
import platform
import re
from typing import Optional


def _default_output_dir() -> str:
    """Pick a sane, always-writable default output directory.

    ``~/Desktop`` doesn't exist on headless Linux servers, minimal window
    managers, or many container images, which used to make the very first
    PDF generation fail with a bare ``FileNotFoundError``. Fall back through
    common alternatives before creating an app-owned directory that is
    guaranteed to exist.
    """
    home = os.path.expanduser("~")
    for candidate in (os.path.join(home, "Desktop"), os.path.join(home, "Documents")):
        if os.path.isdir(candidate):
            return candidate
    fallback = os.path.join(home, "XandSuite", "output")
    os.makedirs(fallback, exist_ok=True)
    return fallback


# ---------------------------------------------------------------------------
# CLI args — parsed before FastMCP takes over sys.argv
# ---------------------------------------------------------------------------

parser = argparse.ArgumentParser(add_help=False)
parser.add_argument(
    "--output-dir",
    default=_default_output_dir(),
    help="Directory for generated PDF output files.",
)
args, _ = parser.parse_known_args()

OUTPUT_DIR: str = args.output_dir

# ---------------------------------------------------------------------------
# FastMCP server
# ---------------------------------------------------------------------------

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-pdf-tools")


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _parse_page_spec(spec: str, total: int) -> list[int]:
    """
    Convert a human-readable page spec to a 0-indexed list.

    Examples:
        "1"       -> [0]
        "1,3"     -> [0, 2]
        "2-5"     -> [1, 2, 3, 4]
        "1,3-5,7" -> [0, 2, 3, 4, 6]
        ""        -> [0, 1, ..., total-1]
    """
    if not spec.strip():
        return list(range(total))

    indices: list[int] = []
    for part in spec.split(","):
        part = part.strip()
        m = re.fullmatch(r"(\d+)-(\d+)", part)
        if m:
            lo, hi = int(m.group(1)), int(m.group(2))
            for p in range(lo, hi + 1):
                idx = p - 1
                if 0 <= idx < total:
                    indices.append(idx)
        elif re.fullmatch(r"\d+", part):
            idx = int(part) - 1
            if 0 <= idx < total:
                indices.append(idx)
    # Deduplicate preserving order
    seen: set[int] = set()
    result = []
    for i in indices:
        if i not in seen:
            seen.add(i)
            result.append(i)
    return result


def _safe_output_path(filename: str) -> str:
    """Resolve output path, creating the directory if needed."""
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    name = filename.strip()
    if not name.lower().endswith(".pdf"):
        name = name + ".pdf"
    # Strip any path separators from filename to keep it flat
    name = os.path.basename(name)
    return os.path.join(OUTPUT_DIR, name)


# ---------------------------------------------------------------------------
# Reading tools
# ---------------------------------------------------------------------------

@mcp.tool()
def read_pdf(file_path: str, pages: str = "") -> str:
    """Extract text from a PDF (offline; best on machine-generated PDFs). Required: file_path (absolute). Optional: pages (e.g. '1,3-5'), empty=all. Returns JSON {file,total_pages,pages_read,text[],full_text}."""
    try:
        import pdfplumber
    except ImportError:
        return json.dumps({"error": "pdfplumber is not installed. Run: pip install pdfplumber"})

    file_path = os.path.expandvars(os.path.expanduser(file_path))
    if not os.path.isfile(file_path):
        return json.dumps({"error": f"File not found: {file_path}"})

    try:
        with pdfplumber.open(file_path) as pdf:
            total = len(pdf.pages)
            indices = _parse_page_spec(pages, total)

            page_texts = []
            for idx in indices:
                text = pdf.pages[idx].extract_text() or ""
                page_texts.append({"page": idx + 1, "text": text})

            full_text = "\n\n".join(
                f"--- Page {p['page']} ---\n{p['text']}" for p in page_texts
            )

            return json.dumps({
                "file": os.path.basename(file_path),
                "total_pages": total,
                "pages_read": [p["page"] for p in page_texts],
                "text": page_texts,
                "full_text": full_text,
            }, ensure_ascii=False)
    except Exception as exc:
        return json.dumps({"error": str(exc)})


@mcp.tool()
def get_pdf_info(file_path: str) -> str:
    """Get PDF metadata and structure (page count, dimensions, title/author/creator/creation). Required: file_path (absolute). Returns JSON {file,file_size_kb,total_pages,metadata,pages[]}."""
    try:
        import pdfplumber
    except ImportError:
        return json.dumps({"error": "pdfplumber is not installed. Run: pip install pdfplumber"})

    file_path = os.path.expandvars(os.path.expanduser(file_path))
    if not os.path.isfile(file_path):
        return json.dumps({"error": f"File not found: {file_path}"})

    try:
        with pdfplumber.open(file_path) as pdf:
            total = len(pdf.pages)
            meta = pdf.metadata or {}

            # Collect page dimensions
            page_info = []
            for i, page in enumerate(pdf.pages):
                page_info.append({
                    "page": i + 1,
                    "width_pt": round(page.width, 2),
                    "height_pt": round(page.height, 2),
                })

            # Decode bytes metadata fields (PDF stores them as bytes sometimes)
            def _decode(v):
                if isinstance(v, bytes):
                    try:
                        return v.decode("utf-8", errors="replace")
                    except Exception:
                        return str(v)
                return v

            return json.dumps({
                "file": os.path.basename(file_path),
                "file_size_kb": round(os.path.getsize(file_path) / 1024, 1),
                "total_pages": total,
                "metadata": {k: _decode(v) for k, v in meta.items()},
                "pages": page_info,
            }, ensure_ascii=False)
    except Exception as exc:
        return json.dumps({"error": str(exc)})


@mcp.tool()
def extract_pdf_tables(file_path: str, page_number: int = 1) -> str:
    """Extract tables from a PDF page (best on machine-generated bordered tables). Required: file_path (absolute). Optional: page_number (1-based, default 1). Returns list-of-tables, each a list of rows."""
    try:
        import pdfplumber
    except ImportError:
        return json.dumps({"error": "pdfplumber is not installed. Run: pip install pdfplumber"})

    file_path = os.path.expandvars(os.path.expanduser(file_path))
    if not os.path.isfile(file_path):
        return json.dumps({"error": f"File not found: {file_path}"})

    try:
        with pdfplumber.open(file_path) as pdf:
            total = len(pdf.pages)
            idx = page_number - 1
            if idx < 0 or idx >= total:
                return json.dumps({
                    "error": f"Page {page_number} out of range. PDF has {total} pages."
                })

            page = pdf.pages[idx]
            raw_tables = page.extract_tables()

            # Clean None cells
            tables = []
            for raw in raw_tables:
                cleaned = [
                    [cell if cell is not None else "" for cell in row]
                    for row in raw
                ]
                tables.append(cleaned)

            return json.dumps({
                "file": os.path.basename(file_path),
                "page": page_number,
                "total_tables": len(tables),
                "tables": tables,
            }, ensure_ascii=False)
    except Exception as exc:
        return json.dumps({"error": str(exc)})


# ---------------------------------------------------------------------------
# Generation tools
# ---------------------------------------------------------------------------

def _find_system_fonts() -> dict[str, str]:
    """Discover system TTF fonts for regular/bold/italic/bold-italic styles.

    Returns a dict like ``{"": "/path/Regular.ttf", "B": "/path/Bold.ttf", ...}``
    with only the styles that actually exist on disk.  Empty dict if nothing found.
    """
    families: list[dict[str, str]] = []
    system = platform.system()

    if system == "Windows":
        fd = os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts")
        families.append({
            "": os.path.join(fd, "arial.ttf"),
            "B": os.path.join(fd, "arialbd.ttf"),
            "I": os.path.join(fd, "ariali.ttf"),
            "BI": os.path.join(fd, "arialbi.ttf"),
        })
        families.append({
            "": os.path.join(fd, "segoeui.ttf"),
            "B": os.path.join(fd, "segoeuib.ttf"),
            "I": os.path.join(fd, "segoeuii.ttf"),
            "BI": os.path.join(fd, "segoeuiz.ttf"),
        })
    elif system == "Darwin":
        families.append({
            "": "/Library/Fonts/Arial.ttf",
            "B": "/Library/Fonts/Arial Bold.ttf",
            "I": "/Library/Fonts/Arial Italic.ttf",
            "BI": "/Library/Fonts/Arial Bold Italic.ttf",
        })
        supp = "/System/Library/Fonts/Supplemental"
        families.append({
            "": os.path.join(supp, "Arial.ttf"),
            "B": os.path.join(supp, "Arial Bold.ttf"),
            "I": os.path.join(supp, "Arial Italic.ttf"),
            "BI": os.path.join(supp, "Arial Bold Italic.ttf"),
        })
    else:
        for base in [
            "/usr/share/fonts/truetype/dejavu",
            "/usr/share/fonts/TTF",
            "/usr/share/fonts/dejavu",
        ]:
            families.append({
                "": os.path.join(base, "DejaVuSans.ttf"),
                "B": os.path.join(base, "DejaVuSans-Bold.ttf"),
                "I": os.path.join(base, "DejaVuSans-Oblique.ttf"),
                "BI": os.path.join(base, "DejaVuSans-BoldOblique.ttf"),
            })
        for base in [
            "/usr/share/fonts/truetype/liberation",
            "/usr/share/fonts/liberation-sans",
        ]:
            families.append({
                "": os.path.join(base, "LiberationSans-Regular.ttf"),
                "B": os.path.join(base, "LiberationSans-Bold.ttf"),
                "I": os.path.join(base, "LiberationSans-Italic.ttf"),
                "BI": os.path.join(base, "LiberationSans-BoldItalic.ttf"),
            })

    for candidate in families:
        if os.path.isfile(candidate.get("", "")):
            return {s: p for s, p in candidate.items() if os.path.isfile(p)}
    return {}


_SYSTEM_FONTS: dict[str, str] = _find_system_fonts()
_UNI_FAMILY = "UniSans"


def _new_pdf():
    """Create a pre-configured FPDF instance with Unicode font support."""
    from fpdf import FPDF

    class _PDF(FPDF):
        _font_family_name = "Helvetica"

        def header(self):
            pass

        def footer(self):
            self.set_y(-12)
            self.set_font(self._font_family_name, "I", 8)
            self.set_text_color(150, 150, 150)
            self.cell(0, 10, f"Page {self.page_no()}", align="C")

    pdf = _PDF()
    pdf.set_auto_page_break(auto=True, margin=18)
    pdf.set_margins(20, 20, 20)

    if _SYSTEM_FONTS:
        for style, path in _SYSTEM_FONTS.items():
            pdf.add_font(_UNI_FAMILY, style=style, fname=path)
        pdf._font_family_name = _UNI_FAMILY
    return pdf


def _font(pdf) -> str:
    """Return the registered font family name for *pdf*."""
    return getattr(pdf, "_font_family_name", "Helvetica")


# ---------------------------------------------------------------------------
# Markdown helpers
# ---------------------------------------------------------------------------

def _strip_inline_md(text: str) -> str:
    """Remove inline markdown markers (bold, italic, code), returning plain text."""
    # Bold before italic to avoid partial matches
    text = re.sub(r'\*\*(.+?)\*\*', r'\1', text)
    text = re.sub(r'__(.+?)__', r'\1', text)
    text = re.sub(r'\*(.+?)\*', r'\1', text)
    text = re.sub(r'_(.+?)_', r'\1', text)
    text = re.sub(r'`(.+?)`', r'\1', text)
    return text


def _write_inline(pdf, text: str, row_h: int = 6):
    """
    Write a single line with **bold** and *italic* inline support.
    Uses write() with font switching so text wraps inside the page margins.
    Appends a newline at the end.
    """
    fam = _font(pdf)
    sz = pdf.font_size_pt or 11
    # Split on **bold** or *italic* spans
    parts = re.split(r'(\*\*[^*\n]+?\*\*|\*[^*\n]+?\*)', text)
    for part in parts:
        if not part:
            continue
        if part.startswith('**') and part.endswith('**') and len(part) > 4:
            pdf.set_font(fam, 'B', sz)
            pdf.write(row_h, part[2:-2])
            pdf.set_font(fam, '', sz)
        elif part.startswith('*') and part.endswith('*') and len(part) > 2:
            pdf.set_font(fam, 'I', sz)
            pdf.write(row_h, part[1:-1])
            pdf.set_font(fam, '', sz)
        else:
            pdf.write(row_h, part)
    pdf.ln(row_h)


def _render_markdown(pdf, text: str):
    """
    Render basic markdown as styled PDF content.

    Supported syntax:
      # H1 / ## H2 / ### H3   — styled headings
      - item / * item          — bulleted list with indent
      **bold** / *italic*      — inline emphasis (in paragraphs)
      ---                      — horizontal rule
      blank lines              — paragraph spacing
    """
    row_h = 6
    in_list = False

    for raw in text.split('\n'):
        line = raw.rstrip()

        # ── Blank line ────────────────────────────────────────────────────
        if not line.strip():
            if in_list:
                in_list = False
            pdf.ln(3)
            continue

        # ── Horizontal rule ───────────────────────────────────────────────
        if re.match(r'^[-_*]{3,}$', line.strip()):
            if in_list:
                in_list = False
            pdf.set_draw_color(180, 180, 200)
            pdf.set_line_width(0.4)
            y = pdf.get_y() + 2
            pdf.line(pdf.l_margin, y, pdf.w - pdf.r_margin, y)
            pdf.ln(6)
            continue

        # ── H3 ───────────────────────────────────────────────────────────
        if line.startswith('### '):
            if in_list:
                in_list = False
            pdf.set_font(_font(pdf), "B", 12)
            pdf.set_text_color(50, 60, 120)
            pdf.multi_cell(0, 7, _strip_inline_md(line[4:].strip()))
            pdf.set_text_color(30, 30, 30)
            pdf.ln(1)
            continue

        # ── H2 ───────────────────────────────────────────────────────────
        if line.startswith('## '):
            if in_list:
                in_list = False
            pdf.set_font(_font(pdf), "B", 14)
            pdf.set_text_color(30, 40, 100)
            pdf.multi_cell(0, 8, _strip_inline_md(line[3:].strip()))
            pdf.set_text_color(30, 30, 30)
            pdf.ln(2)
            continue

        # ── H1 ───────────────────────────────────────────────────────────
        if line.startswith('# '):
            if in_list:
                in_list = False
            pdf.set_font(_font(pdf), "B", 18)
            pdf.set_text_color(20, 20, 80)
            pdf.multi_cell(0, 9, _strip_inline_md(line[2:].strip()))
            pdf.set_text_color(30, 30, 30)
            pdf.ln(3)
            continue

        # ── Bullet list item ──────────────────────────────────────────────
        m = re.match(r'^[-*+]\s+(.*)', line)
        if m:
            in_list = True
            content = _strip_inline_md(m.group(1))
            pdf.set_font(_font(pdf), "", 11)
            pdf.set_text_color(30, 30, 30)
            indent_mm = 8
            bullet_w = 5
            text_x = pdf.l_margin + indent_mm + bullet_w
            usable_w = pdf.w - text_x - pdf.r_margin
            # Bullet glyph
            pdf.set_x(pdf.l_margin + indent_mm)
            pdf.cell(bullet_w, row_h, "\u2022" if _SYSTEM_FONTS else "-")
            # Text (multi_cell handles long line wrap within usable width)
            pdf.set_x(text_x)
            pdf.multi_cell(usable_w, row_h, content)
            pdf.ln(0.5)
            continue

        # ── Regular paragraph ─────────────────────────────────────────────
        if in_list:
            in_list = False
            pdf.ln(1)
        pdf.set_font(_font(pdf), "", 11)
        pdf.set_text_color(30, 30, 30)
        _write_inline(pdf, line, row_h)


@mcp.tool()
def create_pdf_document(
    filename: str,
    title: str,
    content: str,
    author: str = "",
) -> str:
    """Create a simple styled PDF from a Markdown body (no math; use latex_pdf for equations). Required: filename, title, content. Optional: author. Returns JSON {status,filename,path,pages}."""
    try:
        from fpdf import FPDF  # noqa: F401 — trigger import error early
    except ImportError:
        return json.dumps({"error": "fpdf2 is not installed. Run: pip install fpdf2"})

    try:
        pdf = _new_pdf()
        pdf.add_page()

        # Title block
        pdf.set_font(_font(pdf), "B", 22)
        pdf.set_text_color(20, 20, 60)
        pdf.multi_cell(0, 10, title, align="L")
        pdf.ln(2)

        if author:
            pdf.set_font(_font(pdf), "I", 10)
            pdf.set_text_color(100, 100, 120)
            pdf.cell(0, 6, f"By {author}")
            pdf.ln(2)

        # Horizontal rule
        pdf.set_draw_color(180, 180, 200)
        pdf.set_line_width(0.4)
        pdf.line(pdf.get_x(), pdf.get_y() + 2, pdf.w - 20, pdf.get_y() + 2)
        pdf.ln(6)

        # Body — render markdown formatting
        _render_markdown(pdf, content)

        out_path = _safe_output_path(filename)
        pdf.output(out_path)

        return json.dumps({
            "status": "created",
            "filename": os.path.basename(out_path),
            "path": out_path,
            "pages": pdf.page,
            "source": {
                "generator_tool": "create_pdf_document",
                "format": "markdown",
                "body": content,
                "args": {"filename": filename, "title": title, "author": author},
            },
        }, ensure_ascii=False)
    except Exception as exc:
        return json.dumps({"error": str(exc)})


@mcp.tool()
def create_pdf_report(
    filename: str,
    title: str,
    sections: list,
    author: str = "",
) -> str:
    """Create a multi-section PDF report with optional tables (no math). Required: filename, title, sections (list of dicts with keys: heading, body markdown, optional table 2D list; first row is header). Optional: author. Returns JSON {status,filename,path,pages}."""
    try:
        from fpdf import FPDF  # noqa: F401
    except ImportError:
        return json.dumps({"error": "fpdf2 is not installed. Run: pip install fpdf2"})

    if not sections:
        return json.dumps({"error": "sections list is required and must not be empty."})

    try:
        pdf = _new_pdf()
        pdf.add_page()

        # ── Cover block ──────────────────────────────────────────────────────
        pdf.ln(10)
        pdf.set_font(_font(pdf), "B", 26)
        pdf.set_text_color(20, 20, 60)
        pdf.multi_cell(0, 12, title, align="L")
        pdf.ln(2)

        if author:
            pdf.set_font(_font(pdf), "I", 11)
            pdf.set_text_color(100, 100, 120)
            pdf.cell(0, 7, f"By {author}")
            pdf.ln(2)

        pdf.set_draw_color(80, 100, 200)
        pdf.set_line_width(1.0)
        pdf.line(20, pdf.get_y() + 3, pdf.w - 20, pdf.get_y() + 3)
        pdf.ln(10)

        # ── Sections ─────────────────────────────────────────────────────────
        for section in sections:
            heading = str(section.get("heading", "")).strip()
            body = str(section.get("body", "")).strip()
            table_data = section.get("table")

            # Section heading
            if heading:
                pdf.set_font(_font(pdf), "B", 14)
                pdf.set_text_color(20, 20, 60)
                pdf.set_fill_color(240, 242, 250)
                pdf.cell(0, 8, heading, fill=True, ln=True)
                pdf.ln(2)

            # Body text — render markdown formatting
            if body:
                _render_markdown(pdf, body)

            # Optional table
            if table_data and isinstance(table_data, list) and len(table_data) > 0:
                _render_table(pdf, table_data)
                pdf.ln(4)

            pdf.ln(4)

        out_path = _safe_output_path(filename)
        pdf.output(out_path)

        return json.dumps({
            "status": "created",
            "filename": os.path.basename(out_path),
            "path": out_path,
            "pages": pdf.page,
            "source": {
                "generator_tool": "create_pdf_report",
                "format": "sections",
                "body": json.dumps(sections, ensure_ascii=False),
                "args": {"filename": filename, "title": title, "author": author},
            },
        }, ensure_ascii=False)
    except Exception as exc:
        return json.dumps({"error": str(exc)})


def _render_table(pdf, table_data: list):
    """Render a table using fpdf2 with a styled header row."""
    if not table_data:
        return

    usable_w = pdf.w - pdf.l_margin - pdf.r_margin
    n_cols = max(len(row) for row in table_data)
    if n_cols == 0:
        return
    col_w = usable_w / n_cols
    row_h = 7

    for ri, row in enumerate(table_data):
        is_header = ri == 0
        if is_header:
            pdf.set_fill_color(60, 80, 160)
            pdf.set_text_color(255, 255, 255)
            pdf.set_font(_font(pdf), "B", 9)
        else:
            fill = ri % 2 == 0
            pdf.set_fill_color(245, 246, 252) if fill else pdf.set_fill_color(255, 255, 255)
            pdf.set_text_color(30, 30, 30)
            pdf.set_font(_font(pdf), "", 9)

        # Pad row to n_cols
        padded = list(row) + [""] * (n_cols - len(row))

        x_start = pdf.get_x()
        # Check if we need a page break before this row
        if pdf.get_y() + row_h > pdf.h - pdf.b_margin:
            pdf.add_page()

        for ci, cell in enumerate(padded):
            pdf.cell(col_w, row_h, str(cell)[:40],
                     border=1, fill=(ri == 0 or ri % 2 == 0),
                     align="C" if is_header else "L")
        pdf.ln()


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    mcp.run(transport="stdio")
