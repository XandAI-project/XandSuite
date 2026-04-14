#!/usr/bin/env python3
"""
Generate voice preview WAV files for every Kokoro voice.

Run this once (with the KokoroTTS server already running) to pre-bake a short
audio clip for each voice.  The resulting WAV files are stored in
  <project-root>/public/voice-previews/<voice_id>.wav

The VoiceModal in the frontend fetches these files directly for instant
previews — no TTS round-trip needed at runtime.

Usage
-----
  # From the project root (server must be running on the default port):
  npm run gen:previews

  # Or directly, with options:
  python tools/tts/generate_voice_previews.py
  python tools/tts/generate_voice_previews.py --server http://localhost:8766
  python tools/tts/generate_voice_previews.py --speed 1.1 --force
"""

import argparse
import os
import sys
import time

import requests


# ---------------------------------------------------------------------------
# Voice catalogue  (must match KOKORO_VOICES in VoiceModal.tsx)
# ---------------------------------------------------------------------------

VOICES = [
    ("af_heart",    "en-us"),
    ("af_bella",    "en-us"),
    ("af_sarah",    "en-us"),
    ("af_sky",      "en-us"),
    ("am_adam",     "en-us"),
    ("am_michael",  "en-us"),
    ("bf_emma",     "en-gb"),
    ("bf_isabella", "en-gb"),
    ("bm_george",   "en-gb"),
    ("bm_lewis",    "en-gb"),
    ("pf_dora",     "pt-br"),
    ("pm_alex",     "pt-br"),
    ("pm_santa",    "pt-br"),
    ("ef_dora",     "es"),
    ("em_alex",     "es"),
    ("em_santa",    "es"),
    ("ff_siwis",    "fr-fr"),
    ("if_sara",     "it"),
    ("hf_alpha",    "hi"),
    ("hm_omega",    "hi"),
]

# Short, natural sample text per language (same as PREVIEW_TEXT in VoiceModal.tsx)
SAMPLE_TEXT: dict[str, str] = {
    "en-us": "Hey there! I'm ready to help you with anything you need.",
    "en-gb": "Good day! How may I be of assistance to you today?",
    "pt-br": "Olá! Estou aqui para te ajudar com o que precisar.",
    "es":    "¡Hola! Estoy aquí para ayudarte con lo que necesites.",
    "fr-fr": "Bonjour ! Je suis là pour vous aider avec tout ce dont vous avez besoin.",
    "it":    "Ciao! Sono qui per aiutarti con tutto ciò di cui hai bisogno.",
    "hi":    "नमस्ते! मैं आपकी किसी भी चीज़ में मदद करने के लिए यहाँ हूँ।",
}


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _project_root() -> str:
    """Return the project root (two levels above this script)."""
    return os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def _wait_for_server(base_url: str, timeout: int = 30) -> bool:
    """Poll /health until the server responds or timeout expires."""
    health_url = f"{base_url}/health"
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = requests.get(health_url, timeout=3)
            if r.status_code == 200:
                return True
        except requests.exceptions.ConnectionError:
            pass
        time.sleep(1)
    return False


def _synthesize(base_url: str, text: str, voice: str, speed: float) -> bytes:
    """Call the /tts endpoint and return raw WAV bytes."""
    r = requests.post(
        f"{base_url}/tts",
        json={"text": text, "voice": voice, "speed": speed},
        timeout=120,
    )
    r.raise_for_status()
    return r.content


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="Pre-generate Kokoro voice preview WAV files.")
    parser.add_argument(
        "--server",
        default="http://localhost:8766",
        help="KokoroTTS server base URL (default: http://localhost:8766)",
    )
    parser.add_argument(
        "--output-dir",
        default=None,
        help="Directory to write WAV files (default: <project-root>/public/voice-previews)",
    )
    parser.add_argument(
        "--speed",
        type=float,
        default=1.0,
        help="TTS playback speed (default: 1.0)",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-generate files that already exist",
    )
    args = parser.parse_args()

    output_dir = args.output_dir or os.path.join(_project_root(), "public", "voice-previews")
    os.makedirs(output_dir, exist_ok=True)

    print(f"Output directory : {output_dir}")
    print(f"TTS server       : {args.server}")
    print(f"Speed            : {args.speed}")
    print()

    # Check server health
    print("Waiting for TTS server…", end=" ", flush=True)
    if not _wait_for_server(args.server, timeout=10):
        print("UNREACHABLE")
        print(
            "\nERROR: Cannot reach the KokoroTTS server.\n"
            "Start it first with:  npm run tauri dev  (or launch the app)\n"
            "Then re-run this script.",
            file=sys.stderr,
        )
        sys.exit(1)
    print("OK")
    print()

    total = len(VOICES)
    skipped = 0
    failed: list[str] = []

    for idx, (voice_id, lang_code) in enumerate(VOICES, 1):
        out_path = os.path.join(output_dir, f"{voice_id}.wav")

        if os.path.exists(out_path) and not args.force:
            print(f"  [{idx:2}/{total}] {voice_id:<14} — skipped (already exists, use --force to regenerate)")
            skipped += 1
            continue

        text = SAMPLE_TEXT.get(lang_code, SAMPLE_TEXT["en-us"])
        print(f"  [{idx:2}/{total}] {voice_id:<14} — synthesizing… ", end="", flush=True)

        try:
            wav = _synthesize(args.server, text, voice_id, args.speed)
            with open(out_path, "wb") as fh:
                fh.write(wav)
            kb = len(wav) / 1024
            print(f"OK  ({kb:.1f} KB)")
        except Exception as exc:
            print(f"FAILED — {exc}")
            failed.append(voice_id)

    print()
    print("─" * 50)
    generated = total - skipped - len(failed)
    print(f"Generated : {generated}")
    print(f"Skipped   : {skipped}")
    print(f"Failed    : {len(failed)}")
    if failed:
        print(f"  → {', '.join(failed)}")
    print()

    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
