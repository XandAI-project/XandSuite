"""
Tests for tools/packages/official/latex_pdf.py

Run with:
    cd "D:/XandNet Project/XandSuite/tools/packages/tests"
    pip install pytest mcp
    pytest test_latex_pdf.py -v

Two test classes:
  - TestHelpers     : pure-unit tests, no subprocess, no LaTeX install needed.
  - TestIntegration : live compilation tests; skipped if `pdflatex` is not
                      available on PATH.
"""

import json
import os
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

# ---------------------------------------------------------------------------
# Resolve import: allow running from any cwd
# ---------------------------------------------------------------------------

_OFFICIAL_DIR = os.path.join(os.path.dirname(__file__), "..", "official")
sys.path.insert(0, os.path.abspath(_OFFICIAL_DIR))

import latex_pdf as lp  # noqa: E402  (path manipulation before import)


# ---------------------------------------------------------------------------
# Unit tests — no network, no subprocess, no TeX install required
# ---------------------------------------------------------------------------

class TestHelpers(unittest.TestCase):

    # ── _escape_latex ──────────────────────────────────────────────────────

    def test_escape_latex_ampersand(self):
        self.assertEqual(lp._escape_latex("Profit & Loss"), r"Profit \& Loss")

    def test_escape_latex_percent(self):
        self.assertEqual(lp._escape_latex("95%"), r"95\%")

    def test_escape_latex_hash(self):
        self.assertEqual(lp._escape_latex("issue #42"), r"issue \#42")

    def test_escape_latex_underscore(self):
        self.assertEqual(lp._escape_latex("var_name"), r"var\_name")

    def test_escape_latex_braces(self):
        self.assertEqual(lp._escape_latex("{x}"), r"\{x\}")

    def test_escape_latex_tilde(self):
        self.assertEqual(lp._escape_latex("a~b"), r"a\textasciitilde{}b")

    def test_escape_latex_caret(self):
        self.assertEqual(lp._escape_latex("2^3"), r"2\textasciicircum{}3")

    def test_escape_latex_backslash(self):
        self.assertEqual(lp._escape_latex(r"path\file"), r"path\textbackslash{}file")

    def test_escape_latex_preserves_plain_ascii(self):
        self.assertEqual(lp._escape_latex("Hello, world!"), "Hello, world!")

    def test_escape_latex_empty_string(self):
        self.assertEqual(lp._escape_latex(""), "")

    # ── _safe_output_path ──────────────────────────────────────────────────

    def test_safe_output_path_adds_extension(self):
        with tempfile.TemporaryDirectory() as tmp:
            orig = lp.OUTPUT_DIR
            try:
                lp.OUTPUT_DIR = tmp
                path = lp._safe_output_path("report")
                self.assertTrue(path.lower().endswith(".pdf"))
                self.assertTrue(os.path.dirname(path) == tmp)
            finally:
                lp.OUTPUT_DIR = orig

    def test_safe_output_path_keeps_existing_extension(self):
        with tempfile.TemporaryDirectory() as tmp:
            orig = lp.OUTPUT_DIR
            try:
                lp.OUTPUT_DIR = tmp
                path = lp._safe_output_path("report.pdf")
                self.assertTrue(path.lower().endswith(".pdf"))
                self.assertFalse(path.lower().endswith(".pdf.pdf"))
            finally:
                lp.OUTPUT_DIR = orig

    def test_safe_output_path_strips_directory_parts(self):
        with tempfile.TemporaryDirectory() as tmp:
            orig = lp.OUTPUT_DIR
            try:
                lp.OUTPUT_DIR = tmp
                path = lp._safe_output_path("sub/dir/evil.pdf")
                self.assertEqual(os.path.dirname(path), tmp)
                self.assertEqual(os.path.basename(path), "evil.pdf")
            finally:
                lp.OUTPUT_DIR = orig

    def test_safe_output_path_creates_output_dir(self):
        with tempfile.TemporaryDirectory() as tmp:
            nested = os.path.join(tmp, "nested", "deep")
            orig = lp.OUTPUT_DIR
            try:
                lp.OUTPUT_DIR = nested
                path = lp._safe_output_path("note.pdf")
                self.assertTrue(os.path.isdir(nested))
                self.assertEqual(os.path.dirname(path), nested)
            finally:
                lp.OUTPUT_DIR = orig

    # ── _markdown_to_latex: headings ───────────────────────────────────────

    def test_markdown_h1(self):
        out = lp._markdown_to_latex("# Main Title")
        self.assertIn(r"\section", out)
        self.assertIn("Main Title", out)

    def test_markdown_h2(self):
        out = lp._markdown_to_latex("## Subtitle")
        self.assertIn(r"\subsection", out)
        self.assertIn("Subtitle", out)

    def test_markdown_h3(self):
        out = lp._markdown_to_latex("### Topic")
        self.assertIn(r"\subsubsection", out)
        self.assertIn("Topic", out)

    # ── _markdown_to_latex: bullets ────────────────────────────────────────

    def test_markdown_bullets(self):
        out = lp._markdown_to_latex("- first\n- second\n- third")
        self.assertIn(r"\begin{itemize}", out)
        self.assertIn(r"\end{itemize}", out)
        self.assertIn(r"\item first", out)
        self.assertIn(r"\item second", out)
        self.assertIn(r"\item third", out)

    def test_markdown_bullets_closed_by_blank_line(self):
        out = lp._markdown_to_latex("- a\n- b\n\nParagraph after list.")
        self.assertIn(r"\begin{itemize}", out)
        # Closing tag must appear BEFORE the paragraph text
        idx_end = out.index(r"\end{itemize}")
        idx_para = out.index("Paragraph after list.")
        self.assertLess(idx_end, idx_para)

    # ── _markdown_to_latex: inline formatting ──────────────────────────────

    def test_markdown_bold(self):
        out = lp._markdown_to_latex("This is **bold** text.")
        self.assertIn(r"\textbf{bold}", out)

    def test_markdown_italic(self):
        out = lp._markdown_to_latex("This is *italic* text.")
        self.assertIn(r"\textit{italic}", out)

    def test_markdown_horizontal_rule(self):
        out = lp._markdown_to_latex("Before\n\n---\n\nAfter")
        self.assertIn(r"\rule", out)

    # ── _markdown_to_latex: math passthrough ───────────────────────────────

    def test_markdown_inline_math_preserved(self):
        out = lp._markdown_to_latex("Energy: $E=mc^2$ is famous.")
        self.assertIn(r"$E=mc^2$", out)

    def test_markdown_display_math_rewritten(self):
        out = lp._markdown_to_latex(r"$$\int f$$")
        self.assertIn(r"\[", out)
        self.assertIn(r"\int f", out)
        self.assertIn(r"\]", out)

    def test_markdown_escape_outside_math(self):
        """Special chars OUTSIDE math spans are escaped; math is preserved verbatim."""
        out = lp._markdown_to_latex("a & b $x^2$")
        self.assertIn(r"a \& b", out)
        self.assertIn(r"$x^2$", out)
        # The unescaped ampersand should NOT appear outside of a math span
        # (i.e. there is no literal " & " sequence in plain text)
        parts_before_math = out.split("$", 1)[0]
        self.assertNotIn(" & ", parts_before_math)

    def test_markdown_math_not_escaped_internally(self):
        """Backslash commands inside $..$ must NOT be escaped to \\textbackslash."""
        out = lp._markdown_to_latex(r"Formula: $\alpha + \beta = \gamma$")
        self.assertIn(r"$\alpha + \beta = \gamma$", out)
        self.assertNotIn(r"\textbackslash", out)

    def test_markdown_mixed_bold_and_math(self):
        out = lp._markdown_to_latex(r"**Result:** $E=mc^2$")
        self.assertIn(r"\textbf{Result:}", out)
        self.assertIn(r"$E=mc^2$", out)

    # ── _build_document ────────────────────────────────────────────────────

    def test_build_document_contains_title(self):
        src = lp._build_document("My Title", "Alice", r"Body text.")
        self.assertIn(r"\title{My Title}", src)
        self.assertIn(r"\author{Alice}", src)
        self.assertIn(r"\maketitle", src)

    def test_build_document_escapes_title_and_author(self):
        src = lp._build_document("A & B", "Alice_Bob", r"Body.")
        self.assertIn(r"\title{A \& B}", src)
        self.assertIn(r"\author{Alice\_Bob}", src)

    def test_build_document_math_packages_loaded(self):
        src = lp._build_document("T", "", "body")
        for pkg in ("amsmath", "amssymb", "amsfonts", "mathtools", "physics"):
            self.assertIn(pkg, src, msg=f"Missing package: {pkg}")

    def test_build_document_has_document_class_and_env(self):
        src = lp._build_document("T", "", "body")
        self.assertIn(r"\documentclass", src)
        self.assertIn(r"\begin{document}", src)
        self.assertIn(r"\end{document}", src)

    def test_build_document_no_title_block_when_empty(self):
        src = lp._build_document("", "", "just body")
        self.assertNotIn(r"\maketitle", src)
        self.assertNotIn(r"\title{", src)

    # ── _find_engine / _resolve_engine ─────────────────────────────────────

    def test_find_engine_returns_none_for_missing(self):
        self.assertIsNone(lp._find_engine("definitely-not-a-real-binary-xyz-9999"))

    def test_find_engine_returns_none_for_empty(self):
        self.assertIsNone(lp._find_engine(""))

    def test_resolve_engine_auto_prefers_tectonic(self):
        """auto probes tectonic first; returns it when present."""
        def fake_which(name):
            return f"/usr/bin/{name}" if name == "tectonic" else None
        with patch("latex_pdf.shutil.which", side_effect=fake_which):
            path, tried = lp._resolve_engine("auto")
        self.assertEqual(path, "/usr/bin/tectonic")
        self.assertEqual(tried, list(lp._ENGINE_PREFERENCE))

    def test_resolve_engine_auto_falls_back_to_pdflatex(self):
        """auto falls back to pdflatex when tectonic is missing."""
        def fake_which(name):
            return f"/usr/bin/{name}" if name == "pdflatex" else None
        with patch("latex_pdf.shutil.which", side_effect=fake_which):
            path, tried = lp._resolve_engine("auto")
        self.assertEqual(path, "/usr/bin/pdflatex")
        self.assertIn("tectonic", tried)
        self.assertIn("pdflatex", tried)

    def test_resolve_engine_auto_all_missing(self):
        """auto returns None and the full tried list when nothing is installed."""
        with patch("latex_pdf.shutil.which", return_value=None):
            path, tried = lp._resolve_engine("auto")
        self.assertIsNone(path)
        self.assertEqual(tried, list(lp._ENGINE_PREFERENCE))

    def test_resolve_engine_specific_engine_found(self):
        with patch("latex_pdf.shutil.which", return_value="/usr/bin/xelatex"):
            path, tried = lp._resolve_engine("xelatex")
        self.assertEqual(path, "/usr/bin/xelatex")
        self.assertEqual(tried, ["xelatex"])

    def test_resolve_engine_specific_engine_missing(self):
        with patch("latex_pdf.shutil.which", return_value=None):
            path, tried = lp._resolve_engine("pdflatex")
        self.assertIsNone(path)
        self.assertEqual(tried, ["pdflatex"])

    def test_resolve_engine_empty_treated_as_auto(self):
        with patch("latex_pdf.shutil.which", return_value=None):
            path, tried = lp._resolve_engine("")
        self.assertIsNone(path)
        self.assertEqual(tried, list(lp._ENGINE_PREFERENCE))

    # ── _is_tectonic / _build_latex_cmd ────────────────────────────────────

    def test_is_tectonic_detects_by_basename(self):
        self.assertTrue(lp._is_tectonic("/usr/bin/tectonic"))
        self.assertTrue(lp._is_tectonic(r"C:\Program Files\Tectonic\tectonic.exe"))
        self.assertFalse(lp._is_tectonic("/usr/bin/pdflatex"))
        self.assertFalse(lp._is_tectonic(""))

    def test_build_latex_cmd_tectonic(self):
        cmd = lp._build_latex_cmd("/usr/bin/tectonic", "/tmp/doc.tex", "/tmp/out")
        self.assertEqual(cmd[0], "/usr/bin/tectonic")
        self.assertIn("--outdir", cmd)
        self.assertIn("/tmp/out", cmd)
        self.assertIn("/tmp/doc.tex", cmd)
        self.assertIn("--keep-logs", cmd)
        # Tectonic must NOT receive classic-engine flags
        self.assertNotIn("-interaction=nonstopmode", cmd)
        self.assertNotIn("-halt-on-error", cmd)

    def test_build_latex_cmd_pdflatex(self):
        cmd = lp._build_latex_cmd("/usr/bin/pdflatex", "/tmp/doc.tex", "/tmp/out")
        self.assertEqual(cmd[0], "/usr/bin/pdflatex")
        self.assertIn("-interaction=nonstopmode", cmd)
        self.assertIn("-halt-on-error", cmd)
        self.assertIn("-output-directory", cmd)
        self.assertIn("/tmp/out", cmd)
        self.assertIn("/tmp/doc.tex", cmd)
        # Classic pdflatex must NOT receive Tectonic's --outdir
        self.assertNotIn("--outdir", cmd)

    # ── _engine_missing_error ──────────────────────────────────────────────

    def test_engine_missing_error_auto_lists_all_tried(self):
        err = lp._engine_missing_error("auto", ["tectonic", "pdflatex", "xelatex", "lualatex"])
        self.assertIn("tectonic", err["error"])
        self.assertIn("pdflatex", err["error"])
        self.assertIn("Tectonic", err["error"])  # install hint
        self.assertEqual(err["engine_searched"], "auto")
        self.assertEqual(err["tried"], ["tectonic", "pdflatex", "xelatex", "lualatex"])

    def test_engine_missing_error_specific_names_that_engine(self):
        err = lp._engine_missing_error("pdflatex", ["pdflatex"])
        self.assertIn("pdflatex", err["error"])
        self.assertEqual(err["engine_searched"], "pdflatex")
        self.assertEqual(err["tried"], ["pdflatex"])

    # ── list_math_symbols ──────────────────────────────────────────────────

    def test_list_math_symbols_no_category_has_all_groups(self):
        result = json.loads(lp.list_math_symbols(""))
        self.assertIn("categories", result)
        self.assertIn("symbols", result)
        expected = [
            "greek_lower", "greek_upper", "operators", "relations", "arrows",
            "sets", "delimiters", "accents", "big_operators", "functions",
            "matrices", "environments",
        ]
        for cat in expected:
            self.assertIn(cat, result["categories"], msg=f"Missing category: {cat}")
        self.assertEqual(len(result["categories"]), 12)
        self.assertGreater(result["total"], 50)

    def test_list_math_symbols_filter_greek_lower(self):
        result = json.loads(lp.list_math_symbols("greek_lower"))
        self.assertEqual(result["category"], "greek_lower")
        self.assertGreater(result["count"], 0)
        latex_cmds = [e["latex"] for e in result["entries"]]
        self.assertIn(r"\alpha", latex_cmds)
        self.assertIn(r"\beta", latex_cmds)
        self.assertIn(r"\omega", latex_cmds)

    def test_list_math_symbols_filter_big_operators(self):
        result = json.loads(lp.list_math_symbols("big_operators"))
        latex_cmds = [e["latex"] for e in result["entries"]]
        self.assertTrue(any(r"\sum" in c for c in latex_cmds))
        self.assertTrue(any(r"\int" in c for c in latex_cmds))
        self.assertTrue(any(r"\prod" in c for c in latex_cmds))

    def test_list_math_symbols_filter_matrices(self):
        result = json.loads(lp.list_math_symbols("matrices"))
        self.assertGreaterEqual(result["count"], 5)
        latex_cmds = [e["latex"] for e in result["entries"]]
        self.assertTrue(any("pmatrix" in c for c in latex_cmds))
        self.assertTrue(any("bmatrix" in c for c in latex_cmds))

    def test_list_math_symbols_filter_is_case_insensitive(self):
        result = json.loads(lp.list_math_symbols("GREEK_LOWER"))
        self.assertEqual(result["category"], "greek_lower")
        self.assertGreater(result["count"], 0)

    def test_list_math_symbols_unknown_category(self):
        result = json.loads(lp.list_math_symbols("xyzzy"))
        self.assertEqual(result["count"], 0)
        self.assertEqual(result["entries"], [])

    # ── _render_latex_table ────────────────────────────────────────────────

    def test_render_latex_table_has_booktabs_markers(self):
        out = lp._render_latex_table([["Name", "Score"], ["Alice", "95"], ["Bob", "87"]])
        self.assertIn(r"\begin{tabular}", out)
        self.assertIn(r"\toprule", out)
        self.assertIn(r"\midrule", out)
        self.assertIn(r"\bottomrule", out)
        self.assertIn(r"\end{tabular}", out)
        self.assertIn("Alice", out)
        self.assertIn("Bob", out)

    def test_render_latex_table_escapes_special_chars(self):
        out = lp._render_latex_table([["Metric", "Value"], ["Growth %", "+12%"]])
        self.assertIn(r"Growth \%", out)
        self.assertIn(r"+12\%", out)

    def test_render_latex_table_empty_returns_empty(self):
        self.assertEqual(lp._render_latex_table([]), "")

    # ── Engine-missing error paths (mocked) ────────────────────────────────

    @patch(
        "latex_pdf._resolve_or_install_engine",
        return_value=(None, ["tectonic", "pdflatex"], None),
    )
    def test_compile_latex_engine_missing(self, _mock_resolve):
        result = json.loads(lp.compile_latex(
            r"\documentclass{article}\begin{document}Hi\end{document}", "x.pdf"
        ))
        self.assertIn("error", result)
        self.assertIn("PATH", result["error"])
        self.assertIn("Tectonic", result["error"])  # install hint present
        self.assertEqual(result["engine_searched"], lp.ENGINE_NAME)
        self.assertEqual(result["tried"], ["tectonic", "pdflatex"])

    @patch(
        "latex_pdf._resolve_or_install_engine",
        return_value=(None, ["tectonic", "pdflatex"], None),
    )
    def test_create_latex_pdf_engine_missing(self, _mock_resolve):
        result = json.loads(lp.create_latex_pdf("x.pdf", "T", "Body"))
        self.assertIn("error", result)
        self.assertEqual(result["engine_searched"], lp.ENGINE_NAME)

    @patch(
        "latex_pdf._resolve_or_install_engine",
        return_value=(None, ["tectonic", "pdflatex"], None),
    )
    def test_render_equation_engine_missing(self, _mock_resolve):
        result = json.loads(lp.render_equation(r"x^2", "eq.pdf"))
        self.assertIn("error", result)
        self.assertEqual(result["engine_searched"], lp.ENGINE_NAME)

    @patch(
        "latex_pdf._resolve_or_install_engine",
        return_value=(None, ["tectonic", "pdflatex"], None),
    )
    def test_create_math_document_engine_missing(self, _mock_resolve):
        result = json.loads(lp.create_math_document(
            "x.pdf", "T", [{"heading": "S", "body": "b"}]
        ))
        self.assertIn("error", result)

    @patch(
        "latex_pdf._resolve_or_install_engine",
        return_value=(None, ["tectonic"], "Network unreachable"),
    )
    def test_compile_latex_surfaces_install_error(self, _mock_resolve):
        result = json.loads(lp.compile_latex(
            r"\documentclass{article}\begin{document}Hi\end{document}", "x.pdf"
        ))
        self.assertIn("error", result)
        self.assertIn("Auto-install of Tectonic failed", result["error"])
        self.assertIn("Network unreachable", result["error"])
        self.assertEqual(result["install_error"], "Network unreachable")

    # ── Input-validation error paths (no engine call required) ─────────────

    def test_compile_latex_empty_source(self):
        result = json.loads(lp.compile_latex("", "x.pdf"))
        self.assertIn("error", result)

    def test_create_math_document_empty_sections(self):
        result = json.loads(lp.create_math_document("x.pdf", "T", []))
        self.assertIn("error", result)

    def test_render_equation_empty_equation(self):
        result = json.loads(lp.render_equation("   ", "eq.pdf"))
        self.assertIn("error", result)


# ---------------------------------------------------------------------------
# Auto-install (lazy Tectonic download) — pure unit tests, no real network
# ---------------------------------------------------------------------------

class TestAutoInstall(unittest.TestCase):
    """Tests for the lazy Tectonic auto-download fallback."""

    # ── _platform_asset_name ───────────────────────────────────────────────

    def test_platform_asset_windows_x64(self):
        with patch("latex_pdf.platform.system", return_value="Windows"), \
             patch("latex_pdf.platform.machine", return_value="AMD64"):
            asset = lp._platform_asset_name("0.15.0")
        self.assertIsNotNone(asset)
        name, fmt = asset
        self.assertIn("x86_64-pc-windows-msvc", name)
        self.assertTrue(name.endswith(".zip"))
        self.assertEqual(fmt, "zip")

    def test_platform_asset_macos_arm64(self):
        with patch("latex_pdf.platform.system", return_value="Darwin"), \
             patch("latex_pdf.platform.machine", return_value="arm64"):
            asset = lp._platform_asset_name("0.15.0")
        self.assertIsNotNone(asset)
        name, fmt = asset
        self.assertIn("aarch64-apple-darwin", name)
        self.assertTrue(name.endswith(".tar.gz"))
        self.assertEqual(fmt, "tar.gz")

    def test_platform_asset_linux_x64(self):
        with patch("latex_pdf.platform.system", return_value="Linux"), \
             patch("latex_pdf.platform.machine", return_value="x86_64"):
            asset = lp._platform_asset_name("0.15.0")
        self.assertIsNotNone(asset)
        name, _ = asset
        self.assertIn("x86_64-unknown-linux-musl", name)

    def test_platform_asset_unknown_platform_returns_none(self):
        with patch("latex_pdf.platform.system", return_value="FreeBSD"), \
             patch("latex_pdf.platform.machine", return_value="x86_64"):
            self.assertIsNone(lp._platform_asset_name("0.15.0"))

    # ── _user_cache_dir ────────────────────────────────────────────────────

    def test_user_cache_dir_windows_uses_localappdata(self):
        with patch("latex_pdf.platform.system", return_value="Windows"), \
             patch.dict(os.environ, {"LOCALAPPDATA": r"C:\Users\test\AppData\Local"}):
            path = lp._user_cache_dir()
        self.assertTrue(path.endswith(os.path.join("XandSuite", "tectonic")))
        self.assertIn("AppData", path)

    def test_user_cache_dir_linux_respects_xdg(self):
        with patch("latex_pdf.platform.system", return_value="Linux"), \
             patch.dict(os.environ, {"XDG_CACHE_HOME": "/custom/cache"}):
            path = lp._user_cache_dir()
        # Use os.path.join so the assertion is correct on Windows test runners
        # (which use backslashes) as well as on POSIX hosts.
        self.assertEqual(path, os.path.join("/custom/cache", "xandsuite", "tectonic"))

    # ── _cached_tectonic_path ──────────────────────────────────────────────

    def test_cached_tectonic_path_has_correct_extension(self):
        with patch("latex_pdf.platform.system", return_value="Windows"):
            self.assertTrue(lp._cached_tectonic_path().endswith("tectonic.exe"))
        with patch("latex_pdf.platform.system", return_value="Linux"):
            self.assertTrue(lp._cached_tectonic_path().endswith("tectonic"))
            self.assertFalse(lp._cached_tectonic_path().endswith(".exe"))

    # ── _resolve_or_install_engine ─────────────────────────────────────────

    def test_resolve_or_install_uses_path_when_available(self):
        """If an engine is on PATH, no cache lookup or download is attempted."""
        with patch("latex_pdf.shutil.which", return_value="/usr/bin/pdflatex"), \
             patch("latex_pdf.os.path.isfile") as mock_isfile, \
             patch("latex_pdf._download_tectonic") as mock_dl:
            path, tried, err = lp._resolve_or_install_engine("auto")
        self.assertEqual(path, "/usr/bin/pdflatex")
        self.assertIsNone(err)
        mock_isfile.assert_not_called()
        mock_dl.assert_not_called()

    def test_resolve_or_install_uses_cache_when_path_empty(self):
        """When PATH probe fails but a cached binary exists, use it (no download)."""
        cached = lp._cached_tectonic_path()
        with patch("latex_pdf.shutil.which", return_value=None), \
             patch("latex_pdf.os.path.isfile", return_value=True), \
             patch("latex_pdf._download_tectonic") as mock_dl:
            path, _tried, err = lp._resolve_or_install_engine("auto")
        self.assertEqual(path, cached)
        self.assertIsNone(err)
        mock_dl.assert_not_called()

    def test_resolve_or_install_downloads_when_cache_missing(self):
        """When PATH and cache are both empty AND auto-install is enabled, download."""
        fake_path = "/tmp/cache/tectonic"
        with patch("latex_pdf.shutil.which", return_value=None), \
             patch("latex_pdf.os.path.isfile", return_value=False), \
             patch("latex_pdf.AUTO_INSTALL_ENABLED", True), \
             patch("latex_pdf._download_tectonic", return_value=fake_path) as mock_dl:
            path, _tried, err = lp._resolve_or_install_engine("auto")
        self.assertEqual(path, fake_path)
        self.assertIsNone(err)
        mock_dl.assert_called_once()

    def test_resolve_or_install_skips_download_when_disabled(self):
        """--no-auto-install must prevent any download attempt."""
        with patch("latex_pdf.shutil.which", return_value=None), \
             patch("latex_pdf.os.path.isfile", return_value=False), \
             patch("latex_pdf.AUTO_INSTALL_ENABLED", False), \
             patch("latex_pdf._download_tectonic") as mock_dl:
            path, _tried, err = lp._resolve_or_install_engine("auto")
        self.assertIsNone(path)
        self.assertIsNone(err)
        mock_dl.assert_not_called()

    def test_resolve_or_install_returns_error_when_download_fails(self):
        with patch("latex_pdf.shutil.which", return_value=None), \
             patch("latex_pdf.os.path.isfile", return_value=False), \
             patch("latex_pdf.AUTO_INSTALL_ENABLED", True), \
             patch(
                 "latex_pdf._download_tectonic",
                 side_effect=RuntimeError("HTTP 503"),
             ):
            path, _tried, err = lp._resolve_or_install_engine("auto")
        self.assertIsNone(path)
        self.assertEqual(err, "HTTP 503")

    def test_resolve_or_install_specific_pdflatex_does_not_download(self):
        """If user pinned --latex-engine pdflatex, never silently fall back to Tectonic."""
        with patch("latex_pdf.shutil.which", return_value=None), \
             patch("latex_pdf.os.path.isfile", return_value=True), \
             patch("latex_pdf._download_tectonic") as mock_dl:
            path, tried, err = lp._resolve_or_install_engine("pdflatex")
        self.assertIsNone(path)
        self.assertEqual(tried, ["pdflatex"])
        self.assertIsNone(err)
        mock_dl.assert_not_called()


# ---------------------------------------------------------------------------
# Integration tests — require tectonic OR pdflatex on PATH
# ---------------------------------------------------------------------------

_HAS_ENGINE = any(shutil.which(name) for name in ("tectonic", "pdflatex"))
_SKIP_MSG = (
    "No LaTeX engine (tectonic or pdflatex) on PATH — skipping integration tests"
)


@unittest.skipUnless(_HAS_ENGINE, _SKIP_MSG)
class TestIntegration(unittest.TestCase):

    def setUp(self):
        self._tmp = tempfile.mkdtemp(prefix="xandsuite_latex_test_")
        self._orig_output_dir = lp.OUTPUT_DIR
        lp.OUTPUT_DIR = self._tmp

    def tearDown(self):
        lp.OUTPUT_DIR = self._orig_output_dir
        shutil.rmtree(self._tmp, ignore_errors=True)

    # ── compile_latex raw ──────────────────────────────────────────────────

    def test_compile_latex_hello_world(self):
        source = (
            r"\documentclass{article}"
            r"\begin{document}Hello, World!\end{document}"
        )
        result = json.loads(lp.compile_latex(source, "hello.pdf"))
        self.assertNotIn("error", result, msg=str(result))
        self.assertEqual(result["status"], "created")
        self.assertTrue(os.path.isfile(result["path"]))
        self.assertGreater(os.path.getsize(result["path"]), 100)
        print(f"\n  [live] hello.pdf  {result.get('pages')} pages  "
              f"{os.path.getsize(result['path'])} bytes")

    def test_compile_error_reports_log_tail(self):
        bad = (
            r"\documentclass{article}\begin{document}"
            r"\thiscommanddoesnotexist" + "\n" +
            r"\end{document}"
        )
        result = json.loads(lp.compile_latex(bad, "broken.pdf"))
        self.assertIn("error", result)
        self.assertIn("log_tail", result)
        self.assertIsInstance(result["log_tail"], str)
        self.assertGreater(len(result["log_tail"]), 0)

    # ── create_latex_pdf ───────────────────────────────────────────────────

    def test_create_latex_pdf_basic(self):
        result = json.loads(lp.create_latex_pdf(
            "basic", "Basic Document", "Hello world.", author="Tester"
        ))
        self.assertNotIn("error", result, msg=str(result))
        self.assertTrue(os.path.isfile(result["path"]))

    def test_create_latex_pdf_inline_math(self):
        content = "Einstein's famous formula is $E=mc^2$, relating energy and mass."
        result = json.loads(lp.create_latex_pdf(
            "inline_math", "Relativity", content
        ))
        self.assertNotIn("error", result, msg=str(result))

    def test_create_latex_pdf_display_math(self):
        content = (
            "Consider the integral below:\n\n"
            r"$$\int_0^1 \sin(x)\,dx$$" + "\n\n"
            "It has a closed-form value."
        )
        result = json.loads(lp.create_latex_pdf(
            "display_math", "Calculus", content
        ))
        self.assertNotIn("error", result, msg=str(result))

    def test_create_latex_pdf_complex_math(self):
        content = (
            "# Fourier Series\n\n"
            "The Fourier expansion of $f(x)$ is:\n\n"
            r"$$f(x) = \frac{a_0}{2} + \sum_{n=1}^{\infty}"
            r"\left( a_n \cos\frac{n\pi x}{L} + b_n \sin\frac{n\pi x}{L} \right)$$"
            "\n\n"
            "where the coefficients involve integrals like "
            r"$a_n = \frac{1}{L}\int_{-L}^{L} f(x)\cos\frac{n\pi x}{L}\,dx$."
        )
        result = json.loads(lp.create_latex_pdf(
            "fourier", "Fourier Series", content, author="Mathematician"
        ))
        self.assertNotIn("error", result, msg=str(result))

    def test_title_with_special_chars_compiles(self):
        """Title containing & and % must be escaped automatically."""
        result = json.loads(lp.create_latex_pdf(
            "special", "Profit & Loss at 95%", "Content body."
        ))
        self.assertNotIn("error", result, msg=str(result))

    # ── render_equation ────────────────────────────────────────────────────

    def test_render_equation_display_sum(self):
        result = json.loads(lp.render_equation(
            r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}", "sum_eq"
        ))
        self.assertNotIn("error", result, msg=str(result))
        self.assertTrue(os.path.isfile(result["path"]))

    def test_render_equation_inline_quadratic(self):
        result = json.loads(lp.render_equation(
            r"x^2 + y^2 = r^2", "quadratic", display=False
        ))
        self.assertNotIn("error", result, msg=str(result))

    def test_render_equation_greek_and_integrals(self):
        result = json.loads(lp.render_equation(
            r"\alpha + \beta = \int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}",
            "greek_integral",
        ))
        self.assertNotIn("error", result, msg=str(result))

    def test_render_equation_matrix(self):
        result = json.loads(lp.render_equation(
            r"A = \begin{pmatrix} a & b \\ c & d \end{pmatrix}, "
            r"\det(A) = ad - bc",
            "matrix_det",
        ))
        self.assertNotIn("error", result, msg=str(result))

    # ── create_math_document ───────────────────────────────────────────────

    def test_create_math_document_multi_section(self):
        sections = [
            {
                "heading": "Kinematics",
                "body": (
                    "Under constant acceleration, the position is "
                    r"$x = x_0 + v_0 t + \tfrac{1}{2} a t^2$."
                ),
                "equations": [
                    r"v = v_0 + a t",
                    r"v^2 = v_0^2 + 2 a \Delta x",
                ],
            },
            {
                "heading": "Sample Data",
                "body": "The following table lists measurements.",
                "table": [
                    ["t (s)", "x (m)"],
                    ["0",     "0"],
                    ["1",     "4.9"],
                    ["2",     "19.6"],
                    ["3",     "44.1"],
                ],
            },
        ]
        result = json.loads(lp.create_math_document(
            "physics_notes", "Physics Notes", sections, author="A. Einstein"
        ))
        self.assertNotIn("error", result, msg=str(result))
        self.assertTrue(os.path.isfile(result["path"]))
        self.assertGreater(os.path.getsize(result["path"]), 500)


# ---------------------------------------------------------------------------
# Regression tests — compressed tool descriptions must still advertise their
# required fields. The executor's client-side validator and the LLM's tool-
# selection prompt both rely on each tool's docstring to list "Required: ..."
# so that empty-args misfires can be caught and corrected.
# ---------------------------------------------------------------------------

class TestToolDescriptionsAdvertiseRequiredFields(unittest.TestCase):
    """Ensure docstring compression (<= ~200 chars) still lists required args.

    This prevents regressions where a shortened description accidentally drops
    a required field, causing the model to emit `{}` tool-call arguments that
    the server then rejects with a 500 JSON-parse error.
    """

    # Target is ~200 chars; we allow up to 425 for `create_math_document`
    # which spells out the per-section schema, forbids `\( \)` / `\[ \]` in
    # the body Markdown, and advertises that equation-field wrapping is
    # auto-stripped so models don't double-wrap.
    MAX_DESCRIPTION_CHARS = 425

    EXPECTED_REQUIRED_FIELDS = {
        "compile_latex":        ["source", "filename"],
        "create_latex_pdf":     ["filename", "title", "content"],
        "create_math_document": ["filename", "title", "sections"],
        "render_equation":      ["equation", "filename"],
        # `ensure_latex_engine` and `list_math_symbols` take no required args.
    }

    def _func(self, name):
        fn = getattr(lp, name, None)
        self.assertIsNotNone(fn, f"latex_pdf.{name} not found")
        self.assertIsNotNone(fn.__doc__, f"latex_pdf.{name} has no docstring")
        return fn

    def test_descriptions_are_concise(self):
        for name in list(self.EXPECTED_REQUIRED_FIELDS) + [
            "ensure_latex_engine", "list_math_symbols"
        ]:
            fn = self._func(name)
            doc = fn.__doc__.strip()
            self.assertLessEqual(
                len(doc), self.MAX_DESCRIPTION_CHARS,
                f"{name} docstring too long ({len(doc)} > "
                f"{self.MAX_DESCRIPTION_CHARS}): {doc!r}"
            )

    def test_required_fields_are_listed_in_docstring(self):
        for name, required in self.EXPECTED_REQUIRED_FIELDS.items():
            fn = self._func(name)
            doc = fn.__doc__
            self.assertIn(
                "Required", doc,
                f"{name} docstring must advertise its required fields: {doc!r}"
            )
            for field in required:
                self.assertIn(
                    field, doc,
                    f"{name} docstring dropped required field '{field}' "
                    f"after compression: {doc!r}"
                )

    def test_zero_arg_tools_do_not_claim_required_fields(self):
        # These tools take no required arguments — the LLM should not be
        # misled into sending placeholder params.
        for name in ("ensure_latex_engine", "list_math_symbols"):
            fn = self._func(name)
            doc = fn.__doc__
            self.assertNotIn(
                "Required:", doc,
                f"{name} has no required params but docstring claims "
                f"'Required:' — {doc!r}"
            )


class TestStripMathDelimiters(unittest.TestCase):
    """Regression tests for `_strip_math_delimiters`.

    The model was routinely handing us equations wrapped in `$..$` which the
    tool then wrapped in `\\begin{equation}` again, producing
    `"Display math should end with $$"` at compile time. The helper must
    neutralise every common wrapping form.
    """

    def test_passes_through_bare_latex(self):
        self.assertEqual(lp._strip_math_delimiters(r"x^2 + y^2"), r"x^2 + y^2")

    def test_strips_single_dollar(self):
        self.assertEqual(lp._strip_math_delimiters(r"$x^2$"), r"x^2")

    def test_strips_double_dollar(self):
        self.assertEqual(lp._strip_math_delimiters(r"$$x^2$$"), r"x^2")

    def test_strips_paren_delimiters(self):
        self.assertEqual(lp._strip_math_delimiters(r"\(x^2\)"), r"x^2")

    def test_strips_bracket_delimiters(self):
        self.assertEqual(lp._strip_math_delimiters(r"\[x^2\]"), r"x^2")

    def test_strips_equation_environment(self):
        self.assertEqual(
            lp._strip_math_delimiters(r"\begin{equation}x^2\end{equation}"),
            r"x^2",
        )

    def test_preserves_align_environment(self):
        # Aligned envs cannot be nested inside `\begin{equation}`. Keep the
        # whole block verbatim so the caller can emit it as-is.
        src = r"\begin{align*}a &= b \\ c &= d\end{align*}"
        self.assertEqual(lp._strip_math_delimiters(src), src)
        self.assertTrue(lp._looks_like_math_environment(src))

    def test_trims_whitespace_inside_delimiters(self):
        self.assertEqual(lp._strip_math_delimiters(r"$  \alpha + \beta  $"), r"\alpha + \beta")

    def test_empty_or_whitespace_returns_empty(self):
        self.assertEqual(lp._strip_math_delimiters(""), "")
        self.assertEqual(lp._strip_math_delimiters("   "), "")

    def test_lone_dollar_not_stripped(self):
        # `$` alone is not a valid math span — leave it untouched rather
        # than turning it into an empty string.
        self.assertEqual(lp._strip_math_delimiters("$"), "$")

    def test_reported_production_payload_normalises(self):
        # Exact equation strings from the production failure report.
        cases = [
            (r"$\mathbf{x} = [x_1, x_2, \dots, x_n]^T$", r"\mathbf{x} = [x_1, x_2, \dots, x_n]^T"),
            (r"$z = \mathbf{w}^T \mathbf{x} + b = \sum_{i=1}^n w_i x_i + b$",
             r"z = \mathbf{w}^T \mathbf{x} + b = \sum_{i=1}^n w_i x_i + b"),
            (r"$\sigma_{\text{sigmoid}}(z) = \frac{1}{1 + e^{-z}}$",
             r"\sigma_{\text{sigmoid}}(z) = \frac{1}{1 + e^{-z}}"),
        ]
        for inp, expected in cases:
            with self.subTest(inp=inp):
                self.assertEqual(lp._strip_math_delimiters(inp), expected)


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    unittest.main(verbosity=2)
