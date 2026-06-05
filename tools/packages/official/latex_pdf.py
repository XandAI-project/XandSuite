"""
XandSuite Package: LaTeX PDF

Generate beautifully typeset PDFs with complex math equations, Greek symbols,
matrices, integrals, and scientific notation. Compiles real LaTeX via
Tectonic (recommended) or a local TeX distribution (pdflatex / xelatex /
lualatex).

Tools:
  - compile_latex         : raw .tex source passthrough.
  - create_latex_pdf      : Markdown body + inline $..$ and display $$..$$ math.
  - render_equation       : single-equation standalone PDF with tight borders.
  - create_math_document  : multi-section document with equations and tables.
  - list_math_symbols     : reference catalogue of common LaTeX math commands.
  - ensure_latex_engine   : pre-warm the engine cache (lazy auto-download).

Auto-install:
  When `--latex-engine` is `auto` (the default) and no engine is on PATH, the
  package downloads Tectonic for the host OS into a per-user cache directory
  on the first compile call (~50 MB, one-time). Subsequent runs reuse the
  cached binary instantly. Disable with `--no-auto-install`.

CLI args (set at install time via the connector):
  --output-dir         Directory where generated PDFs are saved. Default ~/Desktop.
  --latex-engine       LaTeX engine to use:
                         auto      -> probe tectonic, pdflatex, xelatex, lualatex
                                      on PATH, then the cached download
                                      (first found wins). This is the default.
                         tectonic  -> standalone single-binary engine that auto-
                                      downloads packages on demand.
                         pdflatex  -> classic engine from TeX Live / MiKTeX.
                         xelatex / lualatex -> unicode-aware alternatives.
  --timeout            Compile timeout in seconds. Default 120.
  --no-auto-install    Disable lazy Tectonic auto-download fallback.
"""

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile
from typing import Optional

# ---------------------------------------------------------------------------
# CLI args — parsed before FastMCP takes over sys.argv
# ---------------------------------------------------------------------------

parser = argparse.ArgumentParser(add_help=False)
parser.add_argument(
    "--output-dir",
    default=os.path.expanduser("~/Desktop"),
    help="Directory for generated PDF output files.",
)
parser.add_argument(
    "--latex-engine",
    default="auto",
    help="LaTeX engine: auto | tectonic | pdflatex | xelatex | lualatex.",
)
parser.add_argument(
    "--timeout",
    type=int,
    default=120,
    help="Compile timeout in seconds.",
)
parser.add_argument(
    "--no-auto-install",
    nargs="?",
    const="true",
    default="false",
    help=(
        "Disable automatic Tectonic download fallback. Accepts an optional "
        "value (true/false/yes/no/1/0). When the flag is given with no "
        "value it implies 'true'."
    ),
)
args, _ = parser.parse_known_args()


def _parse_bool(value: object) -> bool:
    """Parse an argparse string flag into a bool (true/yes/on/1 → True)."""
    return str(value).strip().lower() in ("true", "1", "yes", "on")


OUTPUT_DIR: str = args.output_dir
ENGINE_NAME: str = (args.latex_engine or "auto").strip().lower() or "auto"
TIMEOUT: int = int(args.timeout or 120)
AUTO_INSTALL_ENABLED: bool = not _parse_bool(args.no_auto_install)

# ---------------------------------------------------------------------------
# FastMCP server
# ---------------------------------------------------------------------------

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-latex-pdf")


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

# Matches display math $$..$$ (non-greedy) OR inline math $..$ (single line).
# Display is listed first so the alternation prefers $$ over $.
_MATH_RE = re.compile(r"\$\$[\s\S]+?\$\$|\$[^$\n]+?\$")


# Preferred order when `--latex-engine auto` is used: Tectonic first (single-
# binary, auto-downloads packages), then the classic TeX Live / MiKTeX engines.
_ENGINE_PREFERENCE: tuple[str, ...] = ("tectonic", "pdflatex", "xelatex", "lualatex")


def _find_engine(name: str) -> Optional[str]:
    """Return the absolute path of a LaTeX engine binary, or None if missing."""
    if not name:
        return None
    return shutil.which(name)


def _resolve_engine(name: str) -> tuple[Optional[str], list[str]]:
    """
    Resolve a `--latex-engine` value to a concrete engine path.

    When `name` is "auto" (or empty), probe the preference list and return the
    first installed engine. Otherwise look up that specific engine.

    Returns (path_or_None, tried_names). `tried_names` is useful for building
    an actionable error message when no engine is found.
    """
    resolved_name = (name or "").strip().lower() or "auto"
    if resolved_name == "auto":
        tried = list(_ENGINE_PREFERENCE)
        for candidate in _ENGINE_PREFERENCE:
            found = shutil.which(candidate)
            if found:
                return found, tried
        return None, tried
    found = shutil.which(resolved_name)
    return found, [resolved_name]


def _is_tectonic(engine_path: str) -> bool:
    """True when the resolved engine is Tectonic (different CLI from classic TeX)."""
    if not engine_path:
        return False
    base = os.path.splitext(os.path.basename(engine_path))[0].lower()
    return base == "tectonic"


def _build_latex_cmd(engine_path: str, tex_file: str, out_dir: str) -> list[str]:
    """
    Build the engine-appropriate subprocess argv for compiling `tex_file`
    into `out_dir`.

    Tectonic uses a different CLI: `--outdir` (no equals sign), non-interactive
    by default, handles multiple passes internally. Classic engines need
    `-interaction=nonstopmode -halt-on-error -output-directory`.
    """
    if _is_tectonic(engine_path):
        return [
            engine_path,
            "--outdir", out_dir,
            "--keep-logs",
            "--chatter", "minimal",
            tex_file,
        ]
    return [
        engine_path,
        "-interaction=nonstopmode",
        "-halt-on-error",
        "-output-directory", out_dir,
        tex_file,
    ]


# ---------------------------------------------------------------------------
# Tectonic auto-install (lazy: only triggered when no engine is on PATH)
# ---------------------------------------------------------------------------

# Pinned to a known-good Tectonic release. Override with the
# XANDSUITE_TECTONIC_VERSION env var if you need a newer one.
_TECTONIC_VERSION: str = os.environ.get("XANDSUITE_TECTONIC_VERSION", "0.15.0")
_TECTONIC_DOWNLOAD_BASE: str = (
    "https://github.com/tectonic-typesetting/tectonic/releases/download"
)


def _user_cache_dir() -> str:
    """Return the per-user cache directory where the bundled Tectonic lives."""
    sys_name = platform.system().lower()
    if sys_name == "windows":
        base = os.environ.get("LOCALAPPDATA") or os.path.expanduser(
            "~/AppData/Local"
        )
        return os.path.join(base, "XandSuite", "tectonic")
    if sys_name == "darwin":
        return os.path.expanduser("~/Library/Caches/XandSuite/tectonic")
    base = os.environ.get("XDG_CACHE_HOME") or os.path.expanduser("~/.cache")
    return os.path.join(base, "xandsuite", "tectonic")


def _platform_asset_name(version: str) -> Optional[tuple[str, str]]:
    """
    Return ``(asset_filename, archive_format)`` for the host platform, or
    ``None`` if there's no published prebuilt for it.

    ``archive_format`` is ``"zip"`` (Windows) or ``"tar.gz"`` (Unix).
    """
    sys_name = platform.system().lower()
    machine = platform.machine().lower()

    if sys_name == "windows":
        if machine in ("amd64", "x86_64"):
            return (
                f"tectonic-{version}-x86_64-pc-windows-msvc.zip",
                "zip",
            )
        return None
    if sys_name == "darwin":
        if machine in ("arm64", "aarch64"):
            return (
                f"tectonic-{version}-aarch64-apple-darwin.tar.gz",
                "tar.gz",
            )
        if machine in ("x86_64", "amd64"):
            return (
                f"tectonic-{version}-x86_64-apple-darwin.tar.gz",
                "tar.gz",
            )
        return None
    if sys_name == "linux":
        if machine in ("aarch64", "arm64"):
            return (
                f"tectonic-{version}-aarch64-unknown-linux-musl.tar.gz",
                "tar.gz",
            )
        if machine in ("x86_64", "amd64"):
            return (
                f"tectonic-{version}-x86_64-unknown-linux-musl.tar.gz",
                "tar.gz",
            )
        return None
    return None


def _cached_tectonic_path() -> str:
    """Return the expected absolute path of the cached Tectonic binary."""
    name = "tectonic.exe" if platform.system().lower() == "windows" else "tectonic"
    return os.path.join(_user_cache_dir(), name)


def _extract_tectonic_binary(
    archive_path: str, archive_fmt: str, target: str, bin_name: str
) -> None:
    """Extract the ``tectonic`` binary from a downloaded archive into ``target``."""
    target_tmp = target + ".tmp"
    try:
        if archive_fmt == "zip":
            with zipfile.ZipFile(archive_path) as zf:
                for member in zf.namelist():
                    if os.path.basename(member).lower() == bin_name:
                        with zf.open(member) as src, open(target_tmp, "wb") as dst:
                            shutil.copyfileobj(src, dst)
                        break
                else:
                    raise RuntimeError(
                        f"'{bin_name}' not found inside downloaded archive"
                    )
        else:  # tar.gz
            with tarfile.open(archive_path, "r:gz") as tf:
                for member in tf.getmembers():
                    if os.path.basename(member.name).lower() == bin_name:
                        src = tf.extractfile(member)
                        if src is None:
                            continue
                        with open(target_tmp, "wb") as dst:
                            shutil.copyfileobj(src, dst)
                        break
                else:
                    raise RuntimeError(
                        f"'{bin_name}' not found inside downloaded archive"
                    )
        os.replace(target_tmp, target)
    finally:
        if os.path.exists(target_tmp):
            try:
                os.remove(target_tmp)
            except OSError:
                pass


def _download_tectonic(
    version: str = _TECTONIC_VERSION, timeout: int = 300
) -> str:
    """
    Download and install the Tectonic binary for the host platform.

    The archive is fetched from the official GitHub releases, the binary is
    extracted into the per-user cache, and the path is returned. Raises
    :class:`RuntimeError` on any failure with a message safe to surface to
    end users.
    """
    asset = _platform_asset_name(version)
    if asset is None:
        raise RuntimeError(
            f"No prebuilt Tectonic available for "
            f"{platform.system()} {platform.machine()}. "
            "Install a TeX distribution (TeX Live / MiKTeX) manually."
        )
    asset_name, archive_fmt = asset
    url = f"{_TECTONIC_DOWNLOAD_BASE}/tectonic@{version}/{asset_name}"

    cache_dir = _user_cache_dir()
    try:
        os.makedirs(cache_dir, exist_ok=True)
    except OSError as exc:
        raise RuntimeError(
            f"Failed to create cache directory '{cache_dir}': {exc}"
        ) from exc

    archive_path = os.path.join(cache_dir, asset_name)
    try:
        # urllib.request honours HTTP_PROXY / HTTPS_PROXY env vars by default.
        with urllib.request.urlopen(url, timeout=timeout) as resp:  # noqa: S310
            with open(archive_path, "wb") as f:
                shutil.copyfileobj(resp, f)
    except Exception as exc:  # urllib raises a wide tree of exceptions
        if os.path.exists(archive_path):
            try:
                os.remove(archive_path)
            except OSError:
                pass
        raise RuntimeError(
            f"Failed to download Tectonic from {url}: {exc}"
        ) from exc

    bin_name = "tectonic.exe" if platform.system().lower() == "windows" else "tectonic"
    target = os.path.join(cache_dir, bin_name)
    try:
        _extract_tectonic_binary(archive_path, archive_fmt, target, bin_name)
    finally:
        try:
            os.remove(archive_path)
        except OSError:
            pass

    if platform.system().lower() != "windows":
        try:
            os.chmod(target, 0o755)
        except OSError:
            pass

    return target


def _resolve_or_install_engine(
    name: str,
) -> tuple[Optional[str], list[str], Optional[str]]:
    """
    Resolve a ``--latex-engine`` value, with optional auto-install fallback.

    Resolution order for ``"auto"``:
      1. Probe ``_ENGINE_PREFERENCE`` on PATH (user-installed engines win).
      2. Probe the cached download at ``_cached_tectonic_path()``.
      3. If ``AUTO_INSTALL_ENABLED`` is true, download Tectonic and use it.

    Returns a 3-tuple ``(engine_path, tried_names, install_error)``:
      * ``engine_path`` is ``None`` when nothing usable was found.
      * ``tried_names`` mirrors what :func:`_resolve_engine` returned.
      * ``install_error`` is a string explaining why an auto-install attempt
        failed, or ``None`` when no attempt was made or the attempt
        succeeded.
    """
    path, tried = _resolve_engine(name)
    if path is not None:
        return path, tried, None

    resolved = (name or "").strip().lower() or "auto"
    if resolved not in ("auto", "tectonic"):
        return None, tried, None

    cached = _cached_tectonic_path()
    if os.path.isfile(cached):
        return cached, tried, None

    if not AUTO_INSTALL_ENABLED:
        return None, tried, None

    try:
        installed = _download_tectonic()
    except Exception as exc:
        return None, tried, str(exc)
    return installed, tried, None


def _safe_output_path(filename: str) -> str:
    """Resolve and create the output path, ensuring a .pdf extension."""
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    name = (filename or "document").strip() or "document"
    if not name.lower().endswith(".pdf"):
        name = name + ".pdf"
    # Strip any path separators from filename to keep it flat
    name = os.path.basename(name)
    return os.path.join(OUTPUT_DIR, name)


# Characters whose LaTeX replacement itself contains braces (\textbackslash{},
# \textasciitilde{}, \textasciicircum{}). These must go through a sentinel so
# the simple {}→\{\} pass below does not double-escape the braces we just added.
_ESCAPE_COMPLEX = [
    ("\\", "\x02BSLASH\x02", r"\textbackslash{}"),
    ("~",  "\x02TILDE\x02",  r"\textasciitilde{}"),
    ("^",  "\x02CARET\x02",  r"\textasciicircum{}"),
]

# Simple single-char replacements that are safe to apply sequentially.
_ESCAPE_SIMPLE = [
    ("&", r"\&"),
    ("%", r"\%"),
    ("$", r"\$"),
    ("#", r"\#"),
    ("_", r"\_"),
    ("{", r"\{"),
    ("}", r"\}"),
]


def _escape_latex(text: str) -> str:
    """Escape LaTeX special characters in plain user text."""
    if not text:
        return ""
    out = text
    for src, sent, _final in _ESCAPE_COMPLEX:
        out = out.replace(src, sent)
    for src, dst in _ESCAPE_SIMPLE:
        out = out.replace(src, dst)
    for _src, sent, final in _ESCAPE_COMPLEX:
        out = out.replace(sent, final)
    return out


# Sentinels for bold/italic markers — chosen to contain no LaTeX special chars
# so they survive the escape pass intact.
_SENT_BO = "\x01BO\x01"
_SENT_BC = "\x01BC\x01"
_SENT_IO = "\x01IO\x01"
_SENT_IC = "\x01IC\x01"


def _format_inline(text: str) -> str:
    """
    Apply **bold** / *italic* and LaTeX-escape plain text while preserving
    math spans ($..$ inline and $$..$$ display) verbatim.

    Display math $$..$$ is rewritten as \\[ .. \\] for proper block placement.
    """
    if not text:
        return ""

    out_parts: list[str] = []
    last = 0
    for m in _MATH_RE.finditer(text):
        if m.start() > last:
            out_parts.append(_format_plain_segment(text[last:m.start()]))
        span = m.group(0)
        if span.startswith("$$") and span.endswith("$$"):
            inner = span[2:-2].strip()
            out_parts.append(f"\\[ {inner} \\]")
        else:
            out_parts.append(span)  # inline $..$ kept verbatim
        last = m.end()
    if last < len(text):
        out_parts.append(_format_plain_segment(text[last:]))
    return "".join(out_parts)


def _format_plain_segment(seg: str) -> str:
    """Apply bold/italic via sentinels, then escape, then restore commands."""
    if not seg:
        return ""
    tmp = re.sub(r"\*\*(.+?)\*\*", _SENT_BO + r"\1" + _SENT_BC, seg)
    tmp = re.sub(r"\*(.+?)\*", _SENT_IO + r"\1" + _SENT_IC, tmp)
    tmp = _escape_latex(tmp)
    tmp = (
        tmp.replace(_SENT_BO, r"\textbf{")
        .replace(_SENT_BC, "}")
        .replace(_SENT_IO, r"\textit{")
        .replace(_SENT_IC, "}")
    )
    return tmp


def _markdown_to_latex(md: str) -> str:
    """
    Convert a subset of Markdown to LaTeX body source.

    Supported:
      # / ## / ###            -> \\section / \\subsection / \\subsubsection
      - item / * item         -> itemize environment
      **bold** / *italic*     -> \\textbf{} / \\textit{}
      ---                     -> centered horizontal rule
      $..$ / $$..$$           -> preserved; display math rewritten as \\[ .. \\]
      blank lines             -> paragraph break

    Text outside math spans is LaTeX-escaped.
    """
    lines = (md or "").split("\n")
    out: list[str] = []
    in_list = False

    def close_list():
        nonlocal in_list
        if in_list:
            out.append(r"\end{itemize}")
            in_list = False

    for raw in lines:
        line = raw.rstrip()

        if not line.strip():
            close_list()
            out.append("")  # blank line = paragraph break
            continue

        if re.match(r"^[-_*]{3,}$", line.strip()):
            close_list()
            out.append(r"\begin{center}\rule{0.9\linewidth}{0.4pt}\end{center}")
            continue

        if line.startswith("### "):
            close_list()
            out.append(r"\subsubsection*{" + _format_inline(line[4:].strip()) + "}")
            continue
        if line.startswith("## "):
            close_list()
            out.append(r"\subsection*{" + _format_inline(line[3:].strip()) + "}")
            continue
        if line.startswith("# "):
            close_list()
            out.append(r"\section*{" + _format_inline(line[2:].strip()) + "}")
            continue

        m = re.match(r"^[-*+]\s+(.*)", line)
        if m:
            if not in_list:
                out.append(r"\begin{itemize}")
                in_list = True
            out.append(r"  \item " + _format_inline(m.group(1)))
            continue

        close_list()
        out.append(_format_inline(line))

    close_list()
    return "\n".join(out)


_DEFAULT_PREAMBLE = r"""\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage{lmodern}
\usepackage[margin=1in]{geometry}
\usepackage{amsmath}
\usepackage{amssymb}
\usepackage{amsfonts}
\usepackage{mathtools}
\usepackage{physics}
\usepackage{siunitx}
\usepackage{graphicx}
\usepackage{xcolor}
\usepackage{booktabs}
\usepackage{array}
\usepackage{hyperref}
\hypersetup{colorlinks=true,linkcolor=blue!60!black,urlcolor=blue!60!black}
"""


def _build_document(
    title: str,
    author: str,
    body_latex: str,
    doc_class: str = "article",
    extra_preamble: str = "",
) -> str:
    """Assemble a full LaTeX source with a standard math-ready preamble."""
    title_line = ""
    author_line = ""
    maketitle = ""
    if title:
        title_line = r"\title{" + _escape_latex(title) + "}"
        maketitle = r"\maketitle"
    if author:
        author_line = r"\author{" + _escape_latex(author) + "}"

    parts = [
        r"\documentclass[11pt]{" + doc_class + "}",
        _DEFAULT_PREAMBLE.strip(),
        extra_preamble.strip() if extra_preamble else "",
        title_line,
        author_line,
        r"\date{\today}" if title else "",
        r"\begin{document}",
        maketitle,
        body_latex,
        r"\end{document}",
    ]
    return "\n".join(p for p in parts if p)


def _read_log_tail(log_path: str, max_lines: int = 40) -> str:
    """Return the last `max_lines` lines of a LaTeX .log file for diagnostics."""
    try:
        with open(log_path, "r", encoding="utf-8", errors="replace") as f:
            content = f.read()
    except OSError:
        return ""
    lines = content.splitlines()
    return "\n".join(lines[-max_lines:])


def _parse_pages(log_path: str) -> Optional[int]:
    """Extract page count from a LaTeX .log (e.g. 'Output written on foo.pdf (3 pages, ...)')."""
    try:
        with open(log_path, "r", encoding="utf-8", errors="replace") as f:
            content = f.read()
    except OSError:
        return None
    m = re.search(r"Output written on .+?\((\d+) page", content)
    if m:
        try:
            return int(m.group(1))
        except ValueError:
            return None
    return None


def _run_latex(
    tex_source: str,
    filename: str,
    engine_path: str,
    timeout: int,
) -> dict:
    """
    Compile a .tex source into a PDF using the given engine.

    Two compilation passes are performed so cross-references / ToC resolve.
    All intermediate files live in a temporary directory; only the final PDF
    is copied to OUTPUT_DIR.
    """
    base = os.path.splitext(os.path.basename(filename or "document"))[0]
    if not base:
        base = "document"

    with tempfile.TemporaryDirectory(prefix="xandsuite_tex_") as tmp:
        tex_file = os.path.join(tmp, base + ".tex")
        try:
            with open(tex_file, "w", encoding="utf-8") as f:
                f.write(tex_source)
        except OSError as exc:
            return {"error": f"Failed to write .tex source: {exc}"}

        cmd = _build_latex_cmd(engine_path, tex_file, tmp)

        # Tectonic drives its own rerun loop internally; classic engines need
        # a second pass so ToC / cross-references / \ref{} resolve correctly.
        passes = 1 if _is_tectonic(engine_path) else 2
        for _pass in range(passes):
            try:
                proc = subprocess.run(
                    cmd,
                    capture_output=True,
                    timeout=timeout,
                    cwd=tmp,
                )
            except subprocess.TimeoutExpired:
                return {
                    "error": f"LaTeX compilation timed out after {timeout}s",
                    "engine": os.path.basename(engine_path),
                }
            except OSError as exc:
                return {
                    "error": f"Failed to execute LaTeX engine: {exc}",
                    "engine": os.path.basename(engine_path),
                }

            if proc.returncode != 0:
                log_path = os.path.join(tmp, base + ".log")
                log_tail = _read_log_tail(log_path) if os.path.isfile(log_path) else ""
                if not log_tail:
                    stderr = (proc.stderr or b"").decode("utf-8", errors="replace")
                    stdout = (proc.stdout or b"").decode("utf-8", errors="replace")
                    log_tail = (stderr + "\n" + stdout)[-4000:]
                return {
                    "error": "LaTeX compilation failed",
                    "engine": os.path.basename(engine_path),
                    "log_tail": log_tail,
                }

        src_pdf = os.path.join(tmp, base + ".pdf")
        if not os.path.isfile(src_pdf):
            return {
                "error": "LaTeX finished but no PDF was produced",
                "engine": os.path.basename(engine_path),
            }

        out_path = _safe_output_path(filename or base + ".pdf")
        try:
            shutil.copy2(src_pdf, out_path)
        except OSError as exc:
            return {"error": f"Failed to copy PDF to output dir: {exc}"}

        log_path = os.path.join(tmp, base + ".log")
        pages = _parse_pages(log_path) if os.path.isfile(log_path) else None

        return {
            "status": "created",
            "filename": os.path.basename(out_path),
            "path": out_path,
            "pages": pages,
            "engine": os.path.basename(engine_path),
        }


_INSTALL_HINT = (
    "Install Tectonic (recommended — single binary ~50 MB, auto-downloads "
    "packages on first use). Windows: download tectonic-*-x86_64-pc-windows-msvc.zip "
    "from https://github.com/tectonic-typesetting/tectonic/releases, extract, and "
    "add the folder to your PATH (or run 'scoop install tectonic' if you use "
    "Scoop). macOS: 'brew install tectonic'. Linux: see "
    "https://tectonic-typesetting.github.io/en-US/install.html. "
    "Alternatively install a full TeX distribution: TeX Live (Linux/macOS) or "
    "MiKTeX (Windows)."
)


def _engine_missing_error(
    engine_name: str,
    tried: Optional[list[str]] = None,
    install_error: Optional[str] = None,
) -> dict:
    """Build a structured error payload when no LaTeX engine could be resolved."""
    tried_list = list(tried) if tried else [engine_name]
    if engine_name == "auto" or len(tried_list) > 1:
        head = (
            "No LaTeX engine found on PATH or in the cache. Tried: "
            + ", ".join(tried_list)
            + ". "
        )
    else:
        head = f"LaTeX engine '{engine_name}' not found on PATH. "

    if install_error:
        # Auto-install was attempted but failed — surface the reason instead
        # of repeating the generic install hint.
        msg = head + (
            "Auto-install of Tectonic failed: "
            + install_error
            + ". "
            + _INSTALL_HINT
        )
    else:
        msg = head + _INSTALL_HINT

    payload = {
        "error": msg,
        "engine_searched": engine_name,
        "tried": tried_list,
    }
    if install_error:
        payload["install_error"] = install_error
    return payload


# ---------------------------------------------------------------------------
# Math-symbols reference catalogue
# ---------------------------------------------------------------------------

_SYMBOLS: dict[str, list[dict]] = {
    "greek_lower": [
        {"latex": r"\alpha",   "description": "alpha",   "unicode": "α"},
        {"latex": r"\beta",    "description": "beta",    "unicode": "β"},
        {"latex": r"\gamma",   "description": "gamma",   "unicode": "γ"},
        {"latex": r"\delta",   "description": "delta",   "unicode": "δ"},
        {"latex": r"\epsilon", "description": "epsilon", "unicode": "ε"},
        {"latex": r"\zeta",    "description": "zeta",    "unicode": "ζ"},
        {"latex": r"\eta",     "description": "eta",     "unicode": "η"},
        {"latex": r"\theta",   "description": "theta",   "unicode": "θ"},
        {"latex": r"\iota",    "description": "iota",    "unicode": "ι"},
        {"latex": r"\kappa",   "description": "kappa",   "unicode": "κ"},
        {"latex": r"\lambda",  "description": "lambda",  "unicode": "λ"},
        {"latex": r"\mu",      "description": "mu",      "unicode": "μ"},
        {"latex": r"\nu",      "description": "nu",      "unicode": "ν"},
        {"latex": r"\xi",      "description": "xi",      "unicode": "ξ"},
        {"latex": r"\pi",      "description": "pi",      "unicode": "π"},
        {"latex": r"\rho",     "description": "rho",     "unicode": "ρ"},
        {"latex": r"\sigma",   "description": "sigma",   "unicode": "σ"},
        {"latex": r"\tau",     "description": "tau",     "unicode": "τ"},
        {"latex": r"\phi",     "description": "phi",     "unicode": "φ"},
        {"latex": r"\chi",     "description": "chi",     "unicode": "χ"},
        {"latex": r"\psi",     "description": "psi",     "unicode": "ψ"},
        {"latex": r"\omega",   "description": "omega",   "unicode": "ω"},
    ],
    "greek_upper": [
        {"latex": r"\Gamma",   "description": "Gamma",   "unicode": "Γ"},
        {"latex": r"\Delta",   "description": "Delta",   "unicode": "Δ"},
        {"latex": r"\Theta",   "description": "Theta",   "unicode": "Θ"},
        {"latex": r"\Lambda",  "description": "Lambda",  "unicode": "Λ"},
        {"latex": r"\Xi",      "description": "Xi",      "unicode": "Ξ"},
        {"latex": r"\Pi",      "description": "Pi",      "unicode": "Π"},
        {"latex": r"\Sigma",   "description": "Sigma",   "unicode": "Σ"},
        {"latex": r"\Phi",     "description": "Phi",     "unicode": "Φ"},
        {"latex": r"\Psi",     "description": "Psi",     "unicode": "Ψ"},
        {"latex": r"\Omega",   "description": "Omega",   "unicode": "Ω"},
    ],
    "operators": [
        {"latex": r"\pm",      "description": "plus-minus",        "unicode": "±"},
        {"latex": r"\mp",      "description": "minus-plus",        "unicode": "∓"},
        {"latex": r"\times",   "description": "multiplication",    "unicode": "×"},
        {"latex": r"\div",     "description": "division",          "unicode": "÷"},
        {"latex": r"\cdot",    "description": "center dot",        "unicode": "·"},
        {"latex": r"\ast",     "description": "asterisk operator", "unicode": "∗"},
        {"latex": r"\star",    "description": "star operator",     "unicode": "⋆"},
        {"latex": r"\circ",    "description": "ring / composition","unicode": "∘"},
        {"latex": r"\oplus",   "description": "direct sum",        "unicode": "⊕"},
        {"latex": r"\otimes",  "description": "tensor product",    "unicode": "⊗"},
    ],
    "relations": [
        {"latex": r"\leq",      "description": "less than or equal",    "unicode": "≤"},
        {"latex": r"\geq",      "description": "greater than or equal", "unicode": "≥"},
        {"latex": r"\neq",      "description": "not equal",             "unicode": "≠"},
        {"latex": r"\approx",   "description": "approximately equal",   "unicode": "≈"},
        {"latex": r"\equiv",    "description": "identity",              "unicode": "≡"},
        {"latex": r"\sim",      "description": "similar",               "unicode": "∼"},
        {"latex": r"\propto",   "description": "proportional to",       "unicode": "∝"},
        {"latex": r"\ll",       "description": "much less than",        "unicode": "≪"},
        {"latex": r"\gg",       "description": "much greater than",     "unicode": "≫"},
    ],
    "arrows": [
        {"latex": r"\to",          "description": "right arrow",           "unicode": "→"},
        {"latex": r"\leftarrow",   "description": "left arrow",            "unicode": "←"},
        {"latex": r"\leftrightarrow", "description": "left-right arrow",   "unicode": "↔"},
        {"latex": r"\Rightarrow",  "description": "double right arrow",    "unicode": "⇒"},
        {"latex": r"\Leftarrow",   "description": "double left arrow",     "unicode": "⇐"},
        {"latex": r"\Leftrightarrow", "description": "double left-right",  "unicode": "⇔"},
        {"latex": r"\iff",         "description": "if and only if",        "unicode": "⇔"},
        {"latex": r"\mapsto",      "description": "maps to",               "unicode": "↦"},
    ],
    "sets": [
        {"latex": r"\in",         "description": "element of",      "unicode": "∈"},
        {"latex": r"\notin",      "description": "not element of",  "unicode": "∉"},
        {"latex": r"\ni",         "description": "contains as member", "unicode": "∋"},
        {"latex": r"\subset",     "description": "subset",          "unicode": "⊂"},
        {"latex": r"\supset",     "description": "superset",        "unicode": "⊃"},
        {"latex": r"\subseteq",   "description": "subset or equal", "unicode": "⊆"},
        {"latex": r"\cup",        "description": "union",           "unicode": "∪"},
        {"latex": r"\cap",        "description": "intersection",    "unicode": "∩"},
        {"latex": r"\setminus",   "description": "set minus",       "unicode": "∖"},
        {"latex": r"\emptyset",   "description": "empty set",       "unicode": "∅"},
        {"latex": r"\mathbb{R}",  "description": "real numbers",    "unicode": "ℝ"},
        {"latex": r"\mathbb{N}",  "description": "natural numbers", "unicode": "ℕ"},
        {"latex": r"\mathbb{Z}",  "description": "integers",        "unicode": "ℤ"},
        {"latex": r"\mathbb{Q}",  "description": "rational numbers","unicode": "ℚ"},
        {"latex": r"\mathbb{C}",  "description": "complex numbers", "unicode": "ℂ"},
    ],
    "delimiters": [
        {"latex": r"\left( \right)",   "description": "scalable parentheses"},
        {"latex": r"\left[ \right]",   "description": "scalable brackets"},
        {"latex": r"\left\{ \right\}", "description": "scalable braces"},
        {"latex": r"\lvert x \rvert",  "description": "absolute value"},
        {"latex": r"\lVert x \rVert",  "description": "norm"},
        {"latex": r"\lfloor x \rfloor","description": "floor"},
        {"latex": r"\lceil x \rceil",  "description": "ceiling"},
        {"latex": r"\langle x \rangle","description": "angle brackets"},
    ],
    "accents": [
        {"latex": r"\hat{x}",    "description": "hat"},
        {"latex": r"\bar{x}",    "description": "bar"},
        {"latex": r"\vec{x}",    "description": "vector arrow"},
        {"latex": r"\tilde{x}",  "description": "tilde"},
        {"latex": r"\dot{x}",    "description": "dot (time derivative)"},
        {"latex": r"\ddot{x}",   "description": "double dot"},
        {"latex": r"\overline{xyz}",  "description": "overline"},
        {"latex": r"\underline{xyz}", "description": "underline"},
        {"latex": r"\widehat{xyz}",   "description": "wide hat"},
        {"latex": r"\widetilde{xyz}", "description": "wide tilde"},
    ],
    "big_operators": [
        {"latex": r"\sum_{i=1}^{n}",        "description": "summation",       "unicode": "∑"},
        {"latex": r"\prod_{i=1}^{n}",       "description": "product",         "unicode": "∏"},
        {"latex": r"\int_{a}^{b}",          "description": "integral",        "unicode": "∫"},
        {"latex": r"\oint",                 "description": "contour integral","unicode": "∮"},
        {"latex": r"\iint",                 "description": "double integral", "unicode": "∬"},
        {"latex": r"\iiint",                "description": "triple integral", "unicode": "∭"},
        {"latex": r"\lim_{x \to \infty}",   "description": "limit"},
        {"latex": r"\limsup",               "description": "limit superior"},
        {"latex": r"\liminf",               "description": "limit inferior"},
        {"latex": r"\bigcup",               "description": "big union",       "unicode": "⋃"},
        {"latex": r"\bigcap",               "description": "big intersection","unicode": "⋂"},
        {"latex": r"\bigoplus",             "description": "big direct sum",  "unicode": "⨁"},
    ],
    "functions": [
        {"latex": r"\sin",  "description": "sine"},
        {"latex": r"\cos",  "description": "cosine"},
        {"latex": r"\tan",  "description": "tangent"},
        {"latex": r"\csc",  "description": "cosecant"},
        {"latex": r"\sec",  "description": "secant"},
        {"latex": r"\cot",  "description": "cotangent"},
        {"latex": r"\arcsin", "description": "arc sine"},
        {"latex": r"\arccos", "description": "arc cosine"},
        {"latex": r"\arctan", "description": "arc tangent"},
        {"latex": r"\sinh", "description": "hyperbolic sine"},
        {"latex": r"\cosh", "description": "hyperbolic cosine"},
        {"latex": r"\log",  "description": "logarithm"},
        {"latex": r"\ln",   "description": "natural log"},
        {"latex": r"\exp",  "description": "exponential"},
        {"latex": r"\sqrt{x}",    "description": "square root"},
        {"latex": r"\sqrt[n]{x}", "description": "nth root"},
        {"latex": r"\frac{a}{b}", "description": "fraction"},
        {"latex": r"\binom{n}{k}","description": "binomial coefficient"},
    ],
    "matrices": [
        {"latex": r"\begin{matrix} a & b \\ c & d \end{matrix}",   "description": "plain matrix"},
        {"latex": r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", "description": "matrix with parentheses"},
        {"latex": r"\begin{bmatrix} a & b \\ c & d \end{bmatrix}", "description": "matrix with brackets"},
        {"latex": r"\begin{Bmatrix} a & b \\ c & d \end{Bmatrix}", "description": "matrix with braces"},
        {"latex": r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}", "description": "determinant (single bars)"},
        {"latex": r"\begin{Vmatrix} a & b \\ c & d \end{Vmatrix}", "description": "norm (double bars)"},
    ],
    "environments": [
        {"latex": r"\begin{equation} E = mc^2 \end{equation}",            "description": "numbered equation"},
        {"latex": r"\begin{equation*} E = mc^2 \end{equation*}",          "description": "unnumbered equation"},
        {"latex": r"\begin{align} a &= b \\ c &= d \end{align}",          "description": "aligned equations"},
        {"latex": r"\begin{align*} a &= b \\ c &= d \end{align*}",        "description": "unnumbered aligned"},
        {"latex": r"\begin{gather} a = b \\ c = d \end{gather}",          "description": "centered gathered"},
        {"latex": r"\begin{cases} x, & x\geq 0 \\ -x, & x<0 \end{cases}", "description": "piecewise"},
        {"latex": r"\begin{split} a &= b \\ &= c \end{split}",            "description": "split long equation"},
    ],
}


def _latex_symbols_table() -> dict[str, list[dict]]:
    """Return the full symbols catalogue (used by list_math_symbols and tests)."""
    return _SYMBOLS


# ---------------------------------------------------------------------------
# Equation delimiter normalisation
# ---------------------------------------------------------------------------

_MATH_WRAPPERS = (
    ("$$", "$$"),
    ("$", "$"),
    (r"\(", r"\)"),
    (r"\[", r"\]"),
)
_MATH_ENVS = (
    "equation",
    "equation*",
    "align",
    "align*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "eqnarray",
    "eqnarray*",
    "displaymath",
    "math",
)


def _strip_math_delimiters(eq: str) -> str:
    """Normalise an equation string to the raw form expected inside a
    `\\begin{equation}` wrapper.

    Models routinely wrap equations the tool hands back to LaTeX in a second
    math environment (e.g. `"$\\mathbf{x}^T$"` or `"\\[ x^2 \\]"`), which
    nests math modes and blows up with `Display math should end with $$`.
    This helper peels one layer of the most common wrappers so callers can
    be sloppy:

        "$x^2$"                       -> "x^2"
        "$$x^2$$"                     -> "x^2"
        "\\(x^2\\)"                   -> "x^2"
        "\\[x^2\\]"                   -> "x^2"
        "\\begin{equation}x^2\\end{equation}" -> "x^2"
        "\\begin{align*}a&=b\\end{align*}"    -> kept with environment intact
        "x^2"                         -> "x^2"

    Aligned environments (`align`, `gather`, `multline`) are preserved
    because wrapping them inside `\\begin{equation}` is still a LaTeX error,
    but their callers (here: `create_math_document`) will detect the
    leading `\\begin{...}` and emit them as-is rather than re-wrapping.
    """
    if not eq:
        return ""
    s = eq.strip()

    # Strip `$$..$$` and `$..$` first since `$$` would match `$` otherwise.
    for opener, closer in _MATH_WRAPPERS:
        if s.startswith(opener) and s.endswith(closer) and len(s) >= len(opener) + len(closer):
            inner = s[len(opener): len(s) - len(closer)].strip()
            if inner:
                return inner

    # Strip `\begin{equation}..\end{equation}` style wrappers we are about
    # to add ourselves. Multi-line alignment environments are left alone.
    for env in _MATH_ENVS:
        begin = r"\begin{" + env + "}"
        end = r"\end{" + env + "}"
        if s.startswith(begin) and s.endswith(end):
            if env in {"equation", "equation*", "displaymath", "math"}:
                inner = s[len(begin): len(s) - len(end)].strip()
                if inner:
                    return inner
            # Aligned envs: keep as-is; caller must not re-wrap.
            return s

    return s


def _looks_like_math_environment(eq: str) -> bool:
    """True when the equation opens with a multi-line math environment
    (align*, gather, etc.) that must not be nested inside another
    `\\begin{equation}` block.
    """
    s = eq.lstrip()
    for env in _MATH_ENVS:
        if env in {"equation", "equation*", "displaymath", "math"}:
            continue
        if s.startswith(r"\begin{" + env + "}"):
            return True
    return False


# ---------------------------------------------------------------------------
# Table rendering
# ---------------------------------------------------------------------------

def _render_latex_table(table: list) -> str:
    """Render a 2D list as a centered booktabs tabular environment."""
    if not table:
        return ""
    n_cols = max(len(row) for row in table) if table else 0
    if n_cols == 0:
        return ""
    col_spec = "l" * n_cols

    def fmt_row(row: list) -> str:
        padded = list(row) + [""] * (n_cols - len(row))
        return " & ".join(_escape_latex(str(c)) for c in padded) + r" \\"

    lines = [
        r"\begin{center}",
        r"\begin{tabular}{" + col_spec + "}",
        r"\toprule",
        fmt_row(table[0]),
        r"\midrule",
    ]
    for row in table[1:]:
        lines.append(fmt_row(row))
    lines.append(r"\bottomrule")
    lines.append(r"\end{tabular}")
    lines.append(r"\end{center}")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# MCP tools
# ---------------------------------------------------------------------------

@mcp.tool()
def compile_latex(source: str, filename: str) -> str:
    """Compile raw LaTeX source into a PDF (escape hatch for full \\documentclass documents). Required: source (full .tex), filename. Returns JSON {status,filename,path,pages,engine} or {error,...}."""
    if not source or not source.strip():
        return json.dumps({"error": "source is required and must not be empty."})

    engine_path, tried, install_err = _resolve_or_install_engine(ENGINE_NAME)
    if engine_path is None:
        return json.dumps(
            _engine_missing_error(ENGINE_NAME, tried, install_err),
            ensure_ascii=False,
        )

    result = _run_latex(source, filename, engine_path, TIMEOUT)
    if result.get("status") == "created":
        result["source"] = {
            "generator_tool": "compile_latex",
            "format": "latex",
            "body": source,
            "args": {"filename": filename},
        }
    return json.dumps(result, ensure_ascii=False)


@mcp.tool()
def create_latex_pdf(
    filename: str,
    title: str,
    content: str,
    author: str = "",
) -> str:
    """Create a styled PDF from a Markdown body with inline $..$ and display $$..$$ math. Supports #/##/### headings, **bold**, *italic*, bullets, ---. Required: filename, title, content. Optional: author. Returns JSON {status,filename,path,pages,engine}."""
    engine_path, tried, install_err = _resolve_or_install_engine(ENGINE_NAME)
    if engine_path is None:
        return json.dumps(
            _engine_missing_error(ENGINE_NAME, tried, install_err),
            ensure_ascii=False,
        )

    body_latex = _markdown_to_latex(content or "")
    source = _build_document(title=title or "", author=author or "", body_latex=body_latex)
    result = _run_latex(source, filename, engine_path, TIMEOUT)
    if result.get("status") == "created":
        result["source"] = {
            "generator_tool": "create_latex_pdf",
            "format": "markdown",
            "body": content,
            "args": {"filename": filename, "title": title, "author": author},
        }
    return json.dumps(result, ensure_ascii=False)


@mcp.tool()
def render_equation(
    equation: str,
    filename: str,
    display: bool = True,
    fontsize: int = 12,
) -> str:
    """Render a single LaTeX equation as a tightly-cropped standalone PDF. Any wrapping accepted (`$..$`, `$$..$$`, `\\(..\\)`, `\\[..\\]` or bare). Required: equation, filename. Optional: display (bool, default True), fontsize (8-48). Returns JSON {status,filename,path,pages,engine}."""
    if not equation or not equation.strip():
        return json.dumps({"error": "equation is required and must not be empty."})

    engine_path, tried, install_err = _resolve_or_install_engine(ENGINE_NAME)
    if engine_path is None:
        return json.dumps(
            _engine_missing_error(ENGINE_NAME, tried, install_err),
            ensure_ascii=False,
        )

    try:
        fs = int(fontsize)
    except (TypeError, ValueError):
        fs = 12
    fs = max(8, min(fs, 48))

    # Normalise: accept `$x$`, `$$x$$`, `\(x\)`, `\[x\]`, or raw `x` and
    # always wrap ourselves. Without this the model pasting `"$x^2$"` yields
    # nested `$$ $x^2$ $$` which LaTeX rejects with "Display math should end
    # with $$".
    eq_raw = _strip_math_delimiters(equation)
    math_body = (
        f"\\[ {eq_raw} \\]" if display else f"$ {eq_raw} $"
    )
    source = (
        f"\\documentclass[preview,border=12pt,{fs}pt]{{standalone}}\n"
        r"\usepackage{amsmath}" + "\n"
        r"\usepackage{amssymb}" + "\n"
        r"\usepackage{amsfonts}" + "\n"
        r"\usepackage{mathtools}" + "\n"
        r"\usepackage{physics}" + "\n"
        r"\begin{document}" + "\n"
        f"{math_body}\n"
        r"\end{document}" + "\n"
    )
    result = _run_latex(source, filename, engine_path, TIMEOUT)
    if result.get("status") == "created":
        result["source"] = {
            "generator_tool": "render_equation",
            "format": "latex",
            "body": source,
            "args": {"equation": equation, "filename": filename, "display": display, "fontsize": fontsize},
        }
    return json.dumps(result, ensure_ascii=False)


@mcp.tool()
def create_math_document(
    filename: str,
    title: str,
    sections: list,
    author: str = "",
) -> str:
    """Create a multi-section LaTeX PDF with numbered equations and tables. Required: filename, title, sections. Each section dict: {heading, body (Markdown; body math MUST use `$..$` inline or `$$..$$` display — never `\\(..\\)` or `\\[..\\]`), equations (LaTeX list; any wrapping accepted, outer delimiters auto-stripped), table (2D list, first row is header)}. Optional: author. Returns JSON {status,filename,path,pages,engine}."""
    if not sections or not isinstance(sections, list):
        return json.dumps({"error": "sections is required and must be a non-empty list."})

    engine_path, tried, install_err = _resolve_or_install_engine(ENGINE_NAME)
    if engine_path is None:
        return json.dumps(
            _engine_missing_error(ENGINE_NAME, tried, install_err),
            ensure_ascii=False,
        )

    body_parts: list[str] = []
    for section in sections:
        if not isinstance(section, dict):
            continue
        heading = str(section.get("heading", "")).strip()
        body_md = str(section.get("body", "")).strip()
        equations = section.get("equations") or []
        table_data = section.get("table")

        if heading:
            body_parts.append(r"\section{" + _escape_latex(heading) + "}")
        if body_md:
            body_parts.append(_markdown_to_latex(body_md))

        if isinstance(equations, list):
            for eq in equations:
                eq_text = _strip_math_delimiters(str(eq))
                if not eq_text:
                    continue
                # When the author already handed us a full multi-line math
                # environment (align*, gather, …), emit it verbatim — wrapping
                # it in `\begin{equation}` would be a nested-math error.
                if _looks_like_math_environment(eq_text):
                    body_parts.append(eq_text)
                else:
                    body_parts.append(
                        r"\begin{equation}" + "\n" + eq_text + "\n" + r"\end{equation}"
                    )

        if table_data and isinstance(table_data, list) and len(table_data) > 0:
            body_parts.append(_render_latex_table(table_data))

    body_latex = "\n\n".join(body_parts)
    source = _build_document(title=title or "", author=author or "", body_latex=body_latex)
    result = _run_latex(source, filename, engine_path, TIMEOUT)
    if result.get("status") == "created":
        result["source"] = {
            "generator_tool": "create_math_document",
            "format": "sections",
            "body": json.dumps(sections, ensure_ascii=False),
            "args": {"filename": filename, "title": title, "author": author},
        }
    return json.dumps(result, ensure_ascii=False)


@mcp.tool()
def ensure_latex_engine() -> str:
    """Resolve a LaTeX engine (PATH first, then cache, then download Tectonic ~50MB if allowed). No args. Returns JSON {status:'ready',engine,path,source:'path'|'cache'|'downloaded'} or {error,...}."""
    cached = _cached_tectonic_path()
    engine_path, tried, install_err = _resolve_or_install_engine(ENGINE_NAME)
    if engine_path is None:
        return json.dumps(
            _engine_missing_error(ENGINE_NAME, tried, install_err),
            ensure_ascii=False,
        )

    if engine_path == cached:
        # Either reused an existing cache or just downloaded — distinguish by
        # checking whether we expected the cache to already be present. The
        # simplest heuristic: treat a cache hit as "downloaded" only when the
        # binary was created within the last 60 s.
        try:
            mtime = os.path.getmtime(engine_path)
            recently_created = (
                abs(mtime - __import__("time").time()) < 60.0
            )
        except OSError:
            recently_created = False
        source = "downloaded" if recently_created else "cache"
    else:
        source = "path"

    return json.dumps(
        {
            "status": "ready",
            "engine": os.path.basename(engine_path),
            "path": engine_path,
            "source": source,
            "auto_install_enabled": AUTO_INSTALL_ENABLED,
        },
        ensure_ascii=False,
    )


@mcp.tool()
def list_math_symbols(category: str = "") -> str:
    """List LaTeX math symbols/commands. Optional: category (greek_lower, greek_upper, operators, relations, arrows, sets, delimiters, accents, big_operators, functions, matrices, environments). Returns JSON catalogue."""
    table = _latex_symbols_table()
    cat = (category or "").strip().lower()

    if not cat:
        total = sum(len(v) for v in table.values())
        return json.dumps(
            {
                "categories": list(table.keys()),
                "total": total,
                "symbols": table,
            },
            ensure_ascii=False,
        )

    entries = table.get(cat, [])
    return json.dumps(
        {"category": cat, "count": len(entries), "entries": entries},
        ensure_ascii=False,
    )


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    mcp.run(transport="stdio")
