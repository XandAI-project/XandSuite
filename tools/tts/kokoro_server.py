#!/usr/bin/env python3
"""
KokoroTTS HTTP server — powered by hexgrad/Kokoro-82M v1.0.

Runs in two modes:

  Normal server mode (default):
    python kokoro_server.py --port 8766 --hf-home /path/to/hf-cache

  Offline download mode (run once before going offline):
    python kokoro_server.py --download --hf-home /path/to/hf-cache

All Python dependencies are installed into an isolated virtual environment
inside <hf-home>/tts-venv so the user's global Python install is never
modified.  The script re-execs itself inside the venv automatically.

Endpoints (server mode):
  GET  /health  → {"status": "ok"}
  POST /tts     → Body: {"text": str, "voice": str, "speed": float}
                  Response: audio/wav (24 kHz, 16-bit PCM mono)

Voice naming — first character is the KPipeline lang_code:
  a = American English    b = British English    p = Portuguese
  e = Spanish             f = French             i = Italian
  h = Hindi               j = Japanese           k = Korean
  z = Mandarin Chinese
"""

import argparse
import os
import sys

# ---------------------------------------------------------------------------
# Logging  (must work before anything else is imported)
# ---------------------------------------------------------------------------

_log_fh = None


def _log(msg: str) -> None:
    import time
    line = f"[kokoro-server {time.strftime('%H:%M:%S')}] {msg}\n"
    if _log_fh is not None:
        # Write only to the log file — Rust already redirects our stderr to
        # the same file, so writing to both would duplicate every line.
        try:
            _log_fh.write(line)
            _log_fh.flush()
        except Exception:
            pass
    else:
        # No log file yet (e.g. --log-file not passed); fall back to stderr.
        try:
            sys.stderr.write(line)
            sys.stderr.flush()
        except Exception:
            pass


def _open_log(path: str) -> None:
    global _log_fh
    try:
        # Append so re-exec continuations don't erase earlier output.
        _log_fh = open(path, "a", buffering=1, encoding="utf-8")
    except OSError as exc:
        print(f"[kokoro-server] WARNING: cannot open log file: {exc}",
              file=sys.stderr, flush=True)


# ---------------------------------------------------------------------------
# Isolated venv bootstrap
# ---------------------------------------------------------------------------

def _venv_python(hf_home: str) -> str:
    d = os.path.join(hf_home, "tts-venv")
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
    This completely isolates us from the user's (possibly broken) global
    Python environment.
    """
    python = _venv_python(hf_home)
    venv_dir = os.path.join(hf_home, "tts-venv")
    os.makedirs(hf_home, exist_ok=True)

    if not os.path.exists(python):
        _log(f"Creating isolated Python environment at {venv_dir} …")
        import venv as _venv_mod
        _venv_mod.create(venv_dir, with_pip=True, clear=False)
        _log("Isolated environment created.")

    if not _in_venv(hf_home):
        _log(f"Re-launching inside isolated env: {python}")
        # Close the log handle so the child process can append to it cleanly.
        global _log_fh
        if _log_fh is not None:
            try:
                _log_fh.close()
            except Exception:
                pass
            _log_fh = None
        # Use subprocess instead of os.execv — os.execv on Windows does not
        # quote paths that contain spaces, causing the script path to be split
        # on the space character.  subprocess.run uses list2cmdline which
        # properly quotes every argument.
        result = subprocess.run([python] + sys.argv)
        sys.exit(result.returncode)


# ---------------------------------------------------------------------------
# pip helpers  (run inside the venv Python)
# ---------------------------------------------------------------------------

import subprocess


def _pip(*args: str) -> int:
    proc = subprocess.Popen(
        [sys.executable, "-m", "pip", *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    for line in proc.stdout:  # type: ignore[union-attr]
        stripped = line.rstrip()
        if stripped:
            _log(f"  pip: {stripped}")
    proc.wait()
    return proc.returncode


def _pip_install(*packages: str) -> None:
    _log(f"pip install {' '.join(packages)} …")
    rc = _pip("install", "--no-warn-script-location", "--progress-bar", "off",
              *packages)
    if rc != 0:
        raise RuntimeError(
            f"pip install failed (exit {rc}) for: {' '.join(packages)}"
        )
    _log(f"pip install done: {' '.join(packages)}")


# ---------------------------------------------------------------------------
# Dependency management  (called inside the venv — starts from zero)
# ---------------------------------------------------------------------------

# Packages installed regardless of device.
_DEPS_BASE = [
    "transformers>=4.40.0",
    "kokoro>=0.9.2",
    "soundfile",
    "espeakng-loader",
    "numpy",
    "huggingface_hub",
]

# Torch install specs per device variant.
# Pin to torch 2.5.1 which is the last version fully compatible with the
# kokoro package on Python 3.10 (torch 2.6 removed torch.utils.serialization).
_TORCH_SPECS: dict[str, tuple[str, str]] = {
    # variant → (pip_spec, index_url)
    "cuda11": ("torch==2.5.1", "https://download.pytorch.org/whl/cu118"),
    "cuda12": ("torch==2.5.1", "https://download.pytorch.org/whl/cu124"),
}
_TORCH_CPU_SPEC = "torch==2.5.1"


def _torch_ok_for_device(device: str) -> bool:
    """Return True if the installed torch satisfies the requested device."""
    try:
        import torch  # type: ignore
    except ImportError:
        return False

    ver = getattr(torch, "__version__", "0")

    if device == "cpu":
        return True

    # For CUDA variants, the build must have CUDA support AND be the pinned
    # version (torch 2.6 has a broken torch.utils.serialization).
    has_cuda = torch.cuda.is_available() or "+cu" in ver
    if not has_cuda:
        return False

    # Check version matches the pin (2.5.1).
    spec = _TORCH_SPECS.get(device)
    if spec:
        pinned = spec[0].split("==")[1] if "==" in spec[0] else ""
        if pinned and not ver.startswith(pinned):
            _log(f"torch {ver} does not match pin {pinned} — will reinstall")
            return False
    return True


def _ensure_torch(device: str) -> None:
    """Install (or reinstall) torch with the correct index URL for `device`."""
    needs_cuda = device in _TORCH_SPECS

    if not needs_cuda:
        # CPU mode — just make sure *some* torch is installed.
        try:
            import torch  # type: ignore  # noqa: F401
            _log(f"torch {torch.__version__} already installed (CPU)")
        except ImportError:
            _log("Installing torch (CPU) …")
            _pip_install(_TORCH_CPU_SPEC)
        return

    # CUDA path ──────────────────────────────────────────────────────────────
    if _torch_ok_for_device(device):
        _log(f"torch CUDA build already installed — OK for device '{device}'")
        return

    pip_spec, index_url = _TORCH_SPECS[device]
    _log(f"Installing torch CUDA build for {device} …")
    _log(f"  spec: {pip_spec}  index: {index_url}")
    _log("(Large download — ~2 GB — this may take several minutes)")

    # Uninstall the existing torch first so pip doesn't skip due to the
    # different build tag (+cpu vs +cu118/+cu124).
    _log("Uninstalling existing torch …")
    _pip("uninstall", "-y", "torch", "torchvision", "torchaudio")

    rc = _pip("install", "--no-warn-script-location",
              pip_spec, "--index-url", index_url)

    if rc != 0:
        _log(f"WARNING: CUDA torch install failed (exit {rc}) — falling back to CPU torch")
        _pip_install(_TORCH_CPU_SPEC)
    else:
        _log("torch CUDA build installed successfully.")


def _ensure_deps(device: str = "cpu") -> None:
    _log(f"Checking Python dependencies (device={device}) …")
    missing: list[str] = []

    for spec in _DEPS_BASE:
        import_name = spec.split(">=")[0].split("==")[0].split("[")[0]
        import_name = import_name.replace("-", "_")
        try:
            __import__(import_name)
        except ImportError:
            missing.append(spec)

    if missing:
        _log(f"Installing missing packages: {', '.join(missing)}")
        _log("This may take several minutes on first launch …")
        _pip_install(*missing)
        _log("Base dependencies installed.")
    else:
        _log("Base dependencies already installed.")

    # Torch is handled separately because it may need a special index URL.
    _ensure_torch(device)


# ---------------------------------------------------------------------------
# Download mode
# ---------------------------------------------------------------------------

def _download_models(hf_home: str, device: str = "cpu") -> None:
    """Download hexgrad/Kokoro-82M into the app-local HF cache."""
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
# Resolved torch device string (e.g. "cpu", "cuda") set during startup.
_torch_device: str = "cpu"


def _resolve_torch_device(variant: str) -> str:
    """Map the settings variant string to a torch device string."""
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


# ---------------------------------------------------------------------------
# HTTP server
# ---------------------------------------------------------------------------

import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # suppress access log
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

    def do_POST(self):
        if self.path != "/tts":
            self._json(404, {"error": "not found"})
            return

        length = int(self.headers.get("Content-Length", 0))
        try:
            body = json.loads(self.rfile.read(length))
        except json.JSONDecodeError as exc:
            self._json(400, {"error": f"Invalid JSON: {exc}"})
            return

        text = body.get("text", "").strip()
        if not text:
            self._json(400, {"error": "text is required"})
            return

        voice = body.get("voice", "af_heart")
        speed = float(body.get("speed", 1.0))

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
    parser.add_argument("--device",   default="cpu",
                        choices=["cpu", "cuda11", "cuda12"],
                        help="Torch device: cpu, cuda11 (CUDA 11.8), or cuda12 (CUDA 12.4)")
    args = parser.parse_args()

    if args.log_file:
        _open_log(args.log_file)

    # ── Step 1: ensure we're inside the isolated venv ──────────────────────
    # This may re-exec the entire process inside the venv Python.
    _bootstrap_venv(args.hf_home)

    # ── If we reach here we're already inside the venv ─────────────────────
    _log(f"Python {sys.version.split()[0]}  |  exe: {sys.executable}")

    os.environ["HF_HOME"] = args.hf_home
    os.makedirs(args.hf_home, exist_ok=True)
    os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"

    if args.download:
        _log("=== KokoroTTS offline download ===")
        _download_models(args.hf_home, args.device)
        _log("=== Download complete — server can now run offline ===")
        return

    # ── Server mode ─────────────────────────────────────────────────────────
    global _torch_device
    _torch_device = _resolve_torch_device(args.device)

    _log("=== KokoroTTS server starting (hexgrad/Kokoro-82M v1.0) ===")
    _log(f"Port: {args.port}  |  HF_HOME: {args.hf_home}  |  device: {args.device} ({_torch_device})")

    _log("Step 1/2: Checking / installing Python dependencies …")
    _ensure_deps(args.device)

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
