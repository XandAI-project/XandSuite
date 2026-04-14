#!/usr/bin/env python3
"""
KokoroTTS HTTP server — powered by hexgrad/Kokoro-82M v1.0.

Runs in three modes:

  Normal server mode (default):
    python kokoro_server.py --port 8766 --hf-home /path/to/hf-cache

  Setup mode (install deps once, then exit):
    python kokoro_server.py --setup --hf-home /path/to/hf-cache --device cuda12

  Offline download mode (run once before going offline):
    python kokoro_server.py --download --hf-home /path/to/hf-cache

All Python dependencies are installed into an isolated virtual environment
inside <hf-home>/tts-venv so the user's global Python install is never
modified.  The script re-execs itself inside the venv automatically.

A stamp file (<hf-home>/tts-venv/.deps-ok-<device>) is written after a
successful install.  On subsequent launches the dependency check is skipped
entirely, making startup near-instant.  Changing the --device flag
invalidates the stamp and triggers a one-time reinstall.

Endpoints (server mode):
  GET  /health      → {"status": "ok"}
  POST /tts         → Body: {"text": str, "voice": str, "speed": float}
                      Response: audio/wav (24 kHz, 16-bit PCM mono)
  POST /tts/stream  → Body: same as /tts
                      Response: chunked raw PCM (16-bit signed LE, 24 kHz, mono)
                      Streams audio segments as they are generated.

Voice naming — first character is the KPipeline lang_code:
  a = American English    b = British English    p = Portuguese
  e = Spanish             f = French             i = Italian
  h = Hindi               j = Japanese           k = Korean
  z = Mandarin Chinese
"""

import argparse
import os
import shutil
import sys

# ---------------------------------------------------------------------------
# Logging  (must work before anything else is imported)
# ---------------------------------------------------------------------------

_log_fh = None


def _log(msg: str) -> None:
    import time
    line = f"[kokoro-server {time.strftime('%H:%M:%S')}] {msg}\n"
    if _log_fh is not None:
        try:
            _log_fh.write(line)
            _log_fh.flush()
        except Exception:
            pass
    else:
        try:
            sys.stderr.write(line)
            sys.stderr.flush()
        except Exception:
            pass


def _open_log(path: str) -> None:
    global _log_fh
    try:
        _log_fh = open(path, "a", buffering=1, encoding="utf-8")
    except OSError as exc:
        print(f"[kokoro-server] WARNING: cannot open log file: {exc}",
              file=sys.stderr, flush=True)


# ---------------------------------------------------------------------------
# Isolated venv bootstrap
# ---------------------------------------------------------------------------

def _venv_dir(hf_home: str) -> str:
    return os.path.join(hf_home, "tts-venv")


def _venv_python(hf_home: str) -> str:
    d = _venv_dir(hf_home)
    if sys.platform == "win32":
        return os.path.join(d, "Scripts", "python.exe")
    return os.path.join(d, "bin", "python")


def _in_venv(hf_home: str) -> bool:
    try:
        return (os.path.normcase(os.path.abspath(sys.executable)) ==
                os.path.normcase(os.path.abspath(_venv_python(hf_home))))
    except Exception:
        return False


def _bootstrap_venv(hf_home: str) -> None:
    """
    Ensure we're running inside the isolated venv.
    If not, create it (if necessary) and re-exec the script inside it.
    """
    python = _venv_python(hf_home)
    venv = _venv_dir(hf_home)
    os.makedirs(hf_home, exist_ok=True)

    if not os.path.exists(python):
        _log(f"Creating isolated Python environment at {venv} …")
        import venv as _venv_mod
        _venv_mod.create(venv, with_pip=True, clear=False)
        _log("Isolated environment created.")

    if not _in_venv(hf_home):
        _log(f"Re-launching inside isolated env: {python}")
        global _log_fh
        if _log_fh is not None:
            try:
                _log_fh.close()
            except Exception:
                pass
            _log_fh = None
        result = subprocess.run([python] + sys.argv)
        sys.exit(result.returncode)


# ---------------------------------------------------------------------------
# Stamp file — marks a successful dep install for a given device variant
# ---------------------------------------------------------------------------

_STAMP_VERSION = "2"


def _stamp_path(hf_home: str, device: str) -> str:
    return os.path.join(_venv_dir(hf_home), f".deps-ok-{device}")


def _deps_ready(hf_home: str, device: str) -> bool:
    """Return True if deps were already fully installed for this device."""
    p = _stamp_path(hf_home, device)
    if not os.path.isfile(p):
        return False
    try:
        content = open(p, "r", encoding="utf-8").read().strip()
        return content == _STAMP_VERSION
    except OSError:
        return False


def _write_stamp(hf_home: str, device: str) -> None:
    """Write the stamp file after a successful install."""
    p = _stamp_path(hf_home, device)
    try:
        with open(p, "w", encoding="utf-8") as f:
            f.write(_STAMP_VERSION)
    except OSError as exc:
        _log(f"WARNING: could not write stamp file: {exc}")


def _clear_stamps(hf_home: str) -> None:
    """Remove all stamp files (forces reinstall on next launch)."""
    venv = _venv_dir(hf_home)
    if not os.path.isdir(venv):
        return
    for name in os.listdir(venv):
        if name.startswith(".deps-ok-"):
            try:
                os.remove(os.path.join(venv, name))
            except OSError:
                pass


# ---------------------------------------------------------------------------
# Package installer abstraction (prefers uv, falls back to pip)
# ---------------------------------------------------------------------------

import subprocess

_use_uv: bool | None = None


def _find_uv() -> str | None:
    """Return path to uv binary if available."""
    return shutil.which("uv")


def _installer_is_uv() -> bool:
    """Determine once whether to use uv or pip."""
    global _use_uv
    if _use_uv is None:
        uv = _find_uv()
        if uv:
            _log(f"Using uv ({uv}) as package installer (fast mode)")
            _use_uv = True
        else:
            _log("uv not found — using pip (consider installing uv for faster installs)")
            _use_uv = False
    return _use_uv


_PIP_ONLY_FLAGS = {
    "--no-warn-script-location",
    "--progress-bar=off",
    "--progress-bar=on",
    "-y",
    "--yes",
}


def _run_installer(args: list[str], label: str = "",
                    expected_download_mb: float = 0) -> int:
    """Run uv pip or pip with the given args, streaming output to log.

    When *expected_download_mb* > 0, a background thread monitors the
    installer's cache/temp directories for growing files and logs
    estimated download progress every few seconds.  This is necessary
    because both uv and pip suppress progress bars when stdout is not a
    real TTY (our case with subprocess.PIPE).
    """
    if _installer_is_uv():
        clean = [a for a in args if a not in _PIP_ONLY_FLAGS]
        cmd = [_find_uv(), "pip", *clean, "--python", sys.executable]
        prefix = "uv"
    else:
        cmd = [sys.executable, "-m", "pip", *args]
        prefix = "pip"

    if label:
        _log(f"{label} …")

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )

    stop_monitor = None

    if expected_download_mb > 0:
        stop_monitor = _start_download_monitor(prefix, expected_download_mb)

    for line in proc.stdout:  # type: ignore[union-attr]
        stripped = line.rstrip()
        if stripped:
            _log(f"  {prefix}: {stripped}")

    proc.wait()

    if stop_monitor:
        stop_monitor()

    return proc.returncode


def _start_download_monitor(prefix: str, expected_mb: float):
    """Spawn a daemon thread that logs download progress every few seconds.

    Works by scanning for the largest recently-modified file in the
    installer's cache directory (where partial .whl downloads live).
    Both uv and pip suppress progress bars when stdout is not a TTY,
    so this is the only way to report progress to the log.
    """
    import threading
    import time
    import tempfile

    stop_event = threading.Event()
    expected_bytes = expected_mb * 1024 * 1024
    start_time = time.monotonic()

    # Build a focused list of cache dirs to scan
    scan_dirs: list[str] = []
    if _installer_is_uv():
        uv_cache = os.environ.get("UV_CACHE_DIR", "")
        if not uv_cache:
            if sys.platform == "win32":
                uv_cache = os.path.join(
                    os.environ.get("LOCALAPPDATA", tempfile.gettempdir()),
                    "uv", "cache")
            else:
                uv_cache = os.path.join(
                    os.environ.get("XDG_CACHE_HOME",
                                   os.path.expanduser("~/.cache")),
                    "uv")
        # uv stores wheel downloads under <cache>/wheels-v4/ or <cache>/archive-v0/
        for sub in ("wheels-v4", "archive-v0", "sdists-v6", ""):
            d = os.path.join(uv_cache, sub) if sub else uv_cache
            if os.path.isdir(d):
                scan_dirs.append(d)
    else:
        # pip stores downloads in temp and its http cache
        scan_dirs.append(tempfile.gettempdir())
        if sys.platform == "win32":
            scan_dirs.append(os.path.join(
                os.environ.get("LOCALAPPDATA", tempfile.gettempdir()),
                "pip", "cache", "http"))
        else:
            scan_dirs.append(os.path.join(
                os.environ.get("XDG_CACHE_HOME",
                               os.path.expanduser("~/.cache")),
                "pip", "http"))

    def _scan_largest_recent() -> int:
        """Find the single largest file modified since we started."""
        best = 0
        cutoff = time.time() - 120  # files modified in last 2 min
        for d in scan_dirs:
            if not os.path.isdir(d):
                continue
            try:
                for entry in os.scandir(d):
                    try:
                        if entry.is_file(follow_symlinks=False):
                            st = entry.stat()
                            if st.st_mtime > cutoff and st.st_size > best:
                                best = st.st_size
                        elif entry.is_dir(follow_symlinks=False):
                            # One level deep is enough for most cache layouts
                            for sub in os.scandir(entry.path):
                                try:
                                    if sub.is_file(follow_symlinks=False):
                                        st2 = sub.stat()
                                        if st2.st_mtime > cutoff and st2.st_size > best:
                                            best = st2.st_size
                                except OSError:
                                    pass
                    except OSError:
                        pass
            except OSError:
                pass
        return best

    def _monitor():
        last_logged_pct = -1
        stale_count = 0
        prev_size = 0

        while not stop_event.is_set():
            stop_event.wait(5.0)
            if stop_event.is_set():
                break

            current = _scan_largest_recent()
            elapsed = time.monotonic() - start_time

            if current > 0 and expected_bytes > 0:
                pct = min(99, int(current * 100 / expected_bytes))
                done_mb = current / (1024 * 1024)
                total_mb = expected_mb
                speed = done_mb / elapsed if elapsed > 1 else 0
                eta_s = (total_mb - done_mb) / speed if speed > 0.1 else 0
                eta_m = int(eta_s) // 60
                eta_sec = int(eta_s) % 60

                if pct > last_logged_pct or elapsed > 30:
                    _log(f"  {prefix}: Downloading — "
                         f"{done_mb:.0f}/{total_mb:.0f} MB "
                         f"({pct}%) "
                         f"[{speed:.1f} MB/s, ETA {eta_m}m{eta_sec:02d}s]")
                    last_logged_pct = pct

                # Stale detection
                if current == prev_size:
                    stale_count += 1
                else:
                    stale_count = 0
                prev_size = current

                if stale_count >= 12:  # ~60s with no progress
                    _log(f"  {prefix}: WARNING — download appears stalled "
                         f"at {done_mb:.0f} MB for {stale_count * 5}s")
            else:
                # No file found yet — still resolving or download hasn't started
                if elapsed > 10:
                    _log(f"  {prefix}: Waiting for download to start… ({elapsed:.0f}s)")

        elapsed = time.monotonic() - start_time
        _log(f"  {prefix}: Download phase finished ({elapsed:.0f}s)")

    t = threading.Thread(target=_monitor, daemon=True)
    t.start()

    def _stop():
        stop_event.set()
        t.join(timeout=2)

    return _stop


def _install_packages(*packages: str, extra_args: list[str] | None = None) -> None:
    args = ["install", "--no-warn-script-location"]
    if not _installer_is_uv():
        args.append("--progress-bar=off")
    if extra_args:
        args.extend(extra_args)
    args.extend(packages)

    label = f"Installing {' '.join(packages)}"
    rc = _run_installer(args, label)
    if rc != 0:
        raise RuntimeError(
            f"Install failed (exit {rc}) for: {' '.join(packages)}"
        )
    _log(f"Installed: {' '.join(packages)}")


# ---------------------------------------------------------------------------
# Dependency management
# ---------------------------------------------------------------------------

_DEPS_BASE = [
    "transformers>=4.40.0",
    "soundfile",
    "espeakng-loader",
    "numpy",
    "huggingface_hub",
]

_TORCH_SPECS: dict[str, tuple[str, str]] = {
    "cuda11": ("torch==2.5.1", "https://download.pytorch.org/whl/cu118"),
    "cuda12": ("torch==2.5.1", "https://download.pytorch.org/whl/cu124"),
}
_TORCH_CPU_SPEC = "torch==2.5.1"


def _pip_version() -> tuple[int, ...]:
    try:
        result = subprocess.run(
            [sys.executable, "-m", "pip", "--version"],
            capture_output=True, text=True, timeout=10,
        )
        ver_str = result.stdout.split()[1]
        return tuple(int(x) for x in ver_str.split(".")[:3])
    except Exception:
        return (0,)


def _get_torch_version() -> str | None:
    """Return installed torch version string, or None."""
    # Try direct import first — fastest and works regardless of pip/uv
    try:
        result = subprocess.run(
            [sys.executable, "-c",
             "import torch; print(torch.__version__)"],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode == 0:
            ver = result.stdout.strip()
            if ver:
                return ver
    except Exception:
        pass

    # Fallback: uv pip show or pip show
    try:
        if _installer_is_uv():
            cmd = [_find_uv(), "pip", "show", "torch",
                   "--python", sys.executable]
        else:
            cmd = [sys.executable, "-m", "pip", "show", "torch"]
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=15,
        )
        for line in result.stdout.splitlines():
            if line.startswith("Version:"):
                return line.split(":", 1)[1].strip()
    except Exception:
        pass
    return None


def _torch_ok_for_device(device: str) -> bool:
    ver = _get_torch_version()
    if ver is None:
        return False

    _log(f"Installed torch version: {ver}")

    if device == "cpu":
        if ver.startswith("2.5.1"):
            _log(f"torch {ver} is OK for CPU")
            return True
        _log(f"torch {ver} does not match pin 2.5.1 — will reinstall")
        return False

    if "+cu" not in ver:
        _log(f"torch {ver} is a CPU build — need CUDA for device '{device}'")
        return False

    spec = _TORCH_SPECS.get(device)
    if spec:
        pinned = spec[0].split("==")[1] if "==" in spec[0] else ""
        if pinned and not ver.startswith(pinned):
            _log(f"torch {ver} does not match pin {pinned} — will reinstall")
            return False

    _log(f"torch {ver} is OK for device '{device}'")
    return True


def _ensure_pip_modern() -> None:
    if _installer_is_uv():
        return
    ver = _pip_version()
    if ver >= (23,):
        return
    _log(f"pip {'.'.join(str(x) for x in ver)} is too old — upgrading …")
    _run_installer(["install", "--upgrade", "pip"])
    _log("pip upgraded.")


def _ensure_torch(device: str) -> None:
    if _torch_ok_for_device(device):
        return

    needs_cuda = device in _TORCH_SPECS

    if not needs_cuda:
        existing = _get_torch_version()
        if existing and "+cu" in existing:
            # Switching from CUDA → CPU: must uninstall first then install clean
            _log(f"Replacing torch {existing} with CPU build …")
            _run_installer(["uninstall", "torch", "-y"], "Uninstalling old torch")
        _install_packages(_TORCH_CPU_SPEC)
        return

    pip_spec, index_url = _TORCH_SPECS[device]

    # If a wrong torch is installed (CPU or wrong CUDA), uninstall first
    existing = _get_torch_version()
    if existing:
        _log(f"Removing torch {existing} before installing CUDA build …")
        _run_installer(["uninstall", "torch", "-y"], "Uninstalling old torch")

    _log(f"Installing torch CUDA build for {device} …")
    _log(f"  spec: {pip_spec}  index: {index_url}")
    _log("(Large download — ~2 GB — this may take several minutes)")

    max_retries = 3
    for attempt in range(1, max_retries + 1):
        if attempt > 1:
            _log(f"Retry {attempt}/{max_retries} …")

        rc = _run_installer(
            ["install", "--no-warn-script-location",
             pip_spec, "--index-url", index_url],
            f"torch CUDA ({device}) attempt {attempt}",
            expected_download_mb=2400,  # ~2.3 GiB CUDA wheel
        )
        if rc == 0:
            _log("torch CUDA build installed successfully.")
            return

        _log(f"Attempt {attempt} failed (exit {rc})")

    _log("WARNING: All CUDA torch install attempts failed — falling back to CPU torch")
    _install_packages(_TORCH_CPU_SPEC)


def _check_all_deps_installed() -> list[str]:
    missing: list[str] = []
    try:
        __import__("kokoro")
    except ImportError:
        missing.append("kokoro")

    for spec in _DEPS_BASE:
        import_name = spec.split(">=")[0].split("==")[0].split("[")[0]
        import_name = import_name.replace("-", "_")
        try:
            __import__(import_name)
        except ImportError:
            missing.append(import_name)
    return missing


def _ensure_deps(device: str = "cpu") -> None:
    _log(f"Checking Python dependencies (device={device}) …")

    _ensure_pip_modern()
    _ensure_torch(device)

    missing = _check_all_deps_installed()
    if not missing:
        _log("All dependencies already installed.")
        return

    _log(f"Missing packages: {', '.join(missing)}")

    if "kokoro" in missing:
        _log("Installing kokoro (without pulling its own torch) …")
        _install_packages("kokoro>=0.9.2", extra_args=["--no-deps"])
        missing.remove("kokoro")

    if missing:
        import_to_spec = {}
        for spec in _DEPS_BASE:
            iname = spec.split(">=")[0].split("==")[0].split("[")[0].replace("-", "_")
            import_to_spec[iname] = spec
        to_install = [import_to_spec.get(m, m) for m in missing]
        _install_packages(*to_install)

    _log("All dependencies installed.")


# ---------------------------------------------------------------------------
# Download mode
# ---------------------------------------------------------------------------

def _download_models(hf_home: str, device: str = "cpu") -> None:
    _log(f"=== Download mode: hexgrad/Kokoro-82M → {hf_home} ===")
    os.environ["HF_HOME"] = hf_home
    os.makedirs(hf_home, exist_ok=True)

    _ensure_deps(device)

    _log("Downloading model snapshot from HuggingFace …")
    _log("(~300 MB on first download — this may take several minutes)")

    os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")

    from huggingface_hub import snapshot_download  # type: ignore

    local_dir = snapshot_download(
        repo_id="hexgrad/Kokoro-82M",
        local_dir_use_symlinks=False,
        ignore_patterns=["*.msgpack", "flax_model*", "tf_model*", "rust_model*"],
    )
    _log(f"Download complete → {local_dir}")


# ---------------------------------------------------------------------------
# KPipeline cache
# ---------------------------------------------------------------------------

_pipelines: dict = {}
_torch_device: str = "cpu"


def _resolve_torch_device(variant: str) -> str:
    if variant.startswith("cuda"):
        return "cuda"
    return "cpu"


def _lang_code_from_voice(voice: str) -> str:
    return (voice or "a")[0].lower()


def _get_pipeline(lang_code: str):
    if lang_code not in _pipelines:
        _log(f"Loading KPipeline lang_code='{lang_code}' device='{_torch_device}' …")
        from kokoro import KPipeline  # type: ignore
        device = _torch_device
        try:
            _pipelines[lang_code] = KPipeline(lang_code=lang_code, device=device)
        except RuntimeError as exc:
            if "cuda" in str(exc).lower() and device != "cpu":
                _log(f"WARNING: {exc} — falling back to CPU")
                _pipelines[lang_code] = KPipeline(lang_code=lang_code, device="cpu")
            else:
                raise
        _log(f"Pipeline '{lang_code}' ready")
    return _pipelines[lang_code]


# ---------------------------------------------------------------------------
# Synthesis
# ---------------------------------------------------------------------------

def _synthesize(text: str, voice: str, speed: float) -> bytes:
    import io
    import numpy as np
    import soundfile as sf

    pipeline = _get_pipeline(_lang_code_from_voice(voice))
    chunks = []
    for _, _, audio in pipeline(text, voice=voice, speed=speed):
        chunks.append(audio)

    if not chunks:
        raise RuntimeError("KPipeline produced no audio")

    combined = np.concatenate(chunks)
    buf = io.BytesIO()
    sf.write(buf, combined, 24000, format="WAV", subtype="PCM_16")
    return buf.getvalue()


def _synthesize_stream(text: str, voice: str, speed: float):
    """Generator that yields raw 16-bit PCM bytes (24 kHz mono) per
    internal Kokoro segment.  Each yield is a `bytes` object ready to
    be written to the HTTP response body."""
    import numpy as np

    pipeline = _get_pipeline(_lang_code_from_voice(voice))
    any_audio = False
    for _, _, audio in pipeline(text, voice=voice, speed=speed):
        any_audio = True
        pcm16 = (np.clip(audio, -1.0, 1.0) * 32767).astype(np.int16)
        yield pcm16.tobytes()

    if not any_audio:
        raise RuntimeError("KPipeline produced no audio")


# ---------------------------------------------------------------------------
# HTTP server
# ---------------------------------------------------------------------------

import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        pass

    def _json(self, code: int, obj: dict) -> None:
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _audio(self, data: bytes) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "audio/wav")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/health":
            self._json(200, {"status": "ok"})
        else:
            self._json(404, {"error": "not found"})

    def _parse_tts_body(self):
        """Parse and validate the JSON body common to /tts and /tts/stream."""
        length = int(self.headers.get("Content-Length", 0))
        try:
            body = json.loads(self.rfile.read(length))
        except json.JSONDecodeError as exc:
            self._json(400, {"error": f"Invalid JSON: {exc}"})
            return None
        text = body.get("text", "").strip()
        if not text:
            self._json(400, {"error": "text is required"})
            return None
        voice = body.get("voice", "af_heart")
        speed = float(body.get("speed", 1.0))
        return text, voice, speed

    def do_POST(self):
        if self.path == "/tts/stream":
            parsed = self._parse_tts_body()
            if parsed is None:
                return
            text, voice, speed = parsed
            try:
                self.send_response(200)
                self.send_header("Content-Type", "audio/pcm; rate=24000; encoding=signed-integer; bits=16")
                self.send_header("Transfer-Encoding", "chunked")
                self.send_header("X-TTS-Format", "pcm16_24000_mono")
                self.end_headers()
                for pcm_chunk in _synthesize_stream(text, voice, speed):
                    hex_len = f"{len(pcm_chunk):X}\r\n".encode()
                    self.wfile.write(hex_len)
                    self.wfile.write(pcm_chunk)
                    self.wfile.write(b"\r\n")
                self.wfile.write(b"0\r\n\r\n")
                self.wfile.flush()
            except Exception as exc:
                _log(f"Stream synthesis error: {exc}")
            return

        if self.path != "/tts":
            self._json(404, {"error": "not found"})
            return

        parsed = self._parse_tts_body()
        if parsed is None:
            return
        text, voice, speed = parsed

        try:
            self._audio(_synthesize(text, voice, speed))
        except Exception as exc:
            _log(f"Synthesis error: {exc}")
            self._json(500, {"error": str(exc)})


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description="KokoroTTS server / offline downloader (hexgrad/Kokoro-82M v1.0)"
    )
    parser.add_argument("--port",     type=int, default=8766)
    parser.add_argument("--hf-home",  required=True,
                        help="App-local HuggingFace cache + venv directory")
    parser.add_argument("--log-file", default=None)
    parser.add_argument("--download", action="store_true",
                        help="Download model to --hf-home and exit")
    parser.add_argument("--setup",    action="store_true",
                        help="Install/update deps for --device, write stamp, and exit")
    parser.add_argument("--device",   default="cpu",
                        choices=["cpu", "cuda11", "cuda12"],
                        help="Torch device: cpu, cuda11 (CUDA 11.8), or cuda12 (CUDA 12.4)")
    parser.add_argument("--skip-deps", action="store_true",
                        help="Skip dependency check (used when stamp file is valid)")
    args = parser.parse_args()

    if args.log_file:
        _open_log(args.log_file)

    _bootstrap_venv(args.hf_home)

    _log(f"Python {sys.version.split()[0]}  |  exe: {sys.executable}")

    os.environ["HF_HOME"] = args.hf_home
    os.makedirs(args.hf_home, exist_ok=True)
    os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"

    # ── Setup mode: install deps, write stamp, exit ────────────────────────
    if args.setup:
        _log(f"=== Setup mode: installing deps for device={args.device} ===")
        _clear_stamps(args.hf_home)
        _ensure_deps(args.device)
        _write_stamp(args.hf_home, args.device)
        _log("=== Setup complete — deps installed and stamp written ===")
        return

    # ── Download mode ──────────────────────────────────────────────────────
    if args.download:
        _log("=== KokoroTTS offline download ===")
        _download_models(args.hf_home, args.device)
        _write_stamp(args.hf_home, args.device)
        _log("=== Download complete — server can now run offline ===")
        return

    # ── Server mode ────────────────────────────────────────────────────────
    global _torch_device
    _torch_device = _resolve_torch_device(args.device)

    _log("=== KokoroTTS server starting (hexgrad/Kokoro-82M v1.0) ===")
    _log(f"Port: {args.port}  |  HF_HOME: {args.hf_home}  |  device: {args.device} ({_torch_device})")

    if args.skip_deps and _deps_ready(args.hf_home, args.device):
        _log("Stamp file valid — skipping dependency check (fast start)")
    else:
        _log("Step 1/2: Checking / installing Python dependencies …")
        _ensure_deps(args.device)
        _write_stamp(args.hf_home, args.device)

    _log("Step 2/2: Pre-warming English pipeline …")
    _get_pipeline("a")
    _log("=== KokoroTTS server ready ===")

    server = HTTPServer(("127.0.0.1", args.port), Handler)
    _log(f"Listening on http://127.0.0.1:{args.port}")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        if _log_fh:
            _log_fh.close()


if __name__ == "__main__":
    main()
