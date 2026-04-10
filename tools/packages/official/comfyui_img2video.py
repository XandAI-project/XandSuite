"""
XandSuite Package: ComfyUI Image-to-Video
Animate a still image into a video via a local ComfyUI instance using any i2v workflow.

Compatible with LTX-Video 2.3, WAN i2v, AnimateDiff, and any workflow that:
  - Contains a LoadImage node to receive the input image
  - Contains CLIPTextEncode nodes for positive / negative prompts
  - Outputs via a SaveVideo or CreateVideo node

Args:
    --url            ComfyUI server base URL (e.g. http://localhost:8188)
    --workflow-file  Absolute path to a ComfyUI API-format workflow JSON file on this machine
    --frames         Default number of frames to generate (default: 242)
    --timeout        Polling timeout in seconds (default: 1800, i.e. 30 min)

How it works
------------
The package:
  1. Accepts a source image URL (from a previous generation, user upload, or any URL).
  2. Downloads or reads the image, then uploads it to ComfyUI via POST /upload/image.
  3. Finds the first LoadImage node in the workflow and sets its input filename.
  4. Injects positive / negative prompts into CLIPTextEncode nodes automatically.
  5. Randomises seeds in all RandomNoise nodes so each call produces a unique result.
  6. Injects the frame count into PrimitiveInt nodes whose title contains "length" / "frames".
  7. Submits the workflow to /prompt and polls /history for the video output.

Prompt injection
----------------
By default, prompts are injected automatically: the package scans for CLIPTextEncode
nodes and classifies them as positive or negative based on title / content heuristics.

Explicit placeholder tokens (optional, for advanced workflows):
  __POSITIVE_PROMPT__   positive text prompt
  __NEGATIVE_PROMPT__   negative text prompt
  __INPUT_IMAGE__       input image filename (set automatically by the package)
  __FRAMES__            frame count (integer)
  __SEED__              random seed (integer)
"""

import argparse
import json
import os
import tempfile
import time
import uuid
from typing import Any, Optional

from mcp.server.fastmcp import FastMCP

# ---------------------------------------------------------------------------
# CLI args — parsed before FastMCP initialises to avoid argument conflicts
# ---------------------------------------------------------------------------
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--url", default="")
_parser.add_argument("--workflow-file", dest="workflow_file", default="")
_parser.add_argument("--frames", type=int, default=242)
_parser.add_argument("--timeout", type=int, default=1800)
_known, _ = _parser.parse_known_args()

COMFYUI_URL: str = _known.url.rstrip("/")
WORKFLOW_FILE: str = _known.workflow_file
DEFAULT_FRAMES: int = _known.frames
TIMEOUT_SECS: int = _known.timeout

# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------
try:
    import requests as _requests
    _REQUESTS_OK = True
except ImportError:
    _REQUESTS_OK = False


def _check_config() -> Optional[str]:
    """Return an error string if the connector is not fully configured."""
    if not COMFYUI_URL:
        return "ComfyUI URL not configured. Set --url when installing the package."
    if not WORKFLOW_FILE:
        return "Workflow file not configured. Browse for your API-format workflow JSON when installing."
    if not _REQUESTS_OK:
        return "The 'requests' library is not installed. Run: pip install requests"
    return None


def _load_workflow() -> tuple:
    """Load the workflow JSON from the local file path provided at install time.

    Returns (workflow_dict, error_string). One of them will be None.
    """
    try:
        with open(WORKFLOW_FILE, "r", encoding="utf-8") as fh:
            return json.load(fh), None
    except FileNotFoundError:
        return None, (
            f"Workflow file not found: '{WORKFLOW_FILE}'. "
            "Re-configure the package and browse for the correct file."
        )
    except json.JSONDecodeError as exc:
        return None, (
            f"Workflow file is not valid JSON: {exc}. "
            "Make sure you exported the workflow via 'Save (API format)' in ComfyUI."
        )
    except OSError as exc:
        return None, f"Could not read workflow file: {exc}"


# ---------------------------------------------------------------------------
# Image upload helper (from comfyui_image_edit.py)
# ---------------------------------------------------------------------------

_DOWNLOAD_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/124.0.0.0 Safari/537.36"
    ),
    "Accept": "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
}

_IMAGE_SIGNATURES: list = [
    (b"\x89PNG\r\n\x1a\n", ".png"),
    (b"\xff\xd8\xff", ".jpg"),
    (b"RIFF", ".webp"),
    (b"GIF87a", ".gif"),
    (b"GIF89a", ".gif"),
    (b"BM", ".bmp"),
    (b"\x00\x00\x01\x00", ".ico"),
]


def _detect_image_ext(data: bytes) -> Optional[str]:
    """Return a file extension if data looks like a supported image, else None."""
    for magic, ext in _IMAGE_SIGNATURES:
        if data[:len(magic)] == magic:
            if ext == ".webp" and data[8:12] != b"WEBP":
                continue
            return ext
    return None


def _upload_image(image_source: str) -> tuple:
    """Upload an image to ComfyUI's input folder.

    image_source may be:
    - A remote URL (http/https) — downloaded first to a temp file, then uploaded.
    - A local file path — uploaded directly.

    Returns (comfyui_filename, error_string). One will be None.
    """
    tmp_path: Optional[str] = None

    try:
        if image_source.startswith("http://") or image_source.startswith("https://"):
            try:
                dl = _requests.get(
                    image_source,
                    headers=_DOWNLOAD_HEADERS,
                    timeout=60,
                    stream=True,
                )
                dl.raise_for_status()
            except _requests.exceptions.ConnectionError:
                return None, f"Could not download image from '{image_source}': connection refused."
            except _requests.exceptions.HTTPError as exc:
                return None, (
                    f"Could not download image from '{image_source}': "
                    f"HTTP {exc.response.status_code}. "
                    "The URL may require authentication or may have expired."
                )
            except Exception as exc:
                return None, f"Could not download image from '{image_source}': {exc}"

            first_chunk = b""
            tmp_fd, tmp_path = tempfile.mkstemp(suffix=".bin", prefix="xandsuite_i2v_")
            with os.fdopen(tmp_fd, "wb") as fh:
                for chunk in dl.iter_content(chunk_size=8192):
                    if not first_chunk:
                        first_chunk = chunk[:512]
                    fh.write(chunk)

            detected_ext = _detect_image_ext(first_chunk)
            if detected_ext is None:
                content_type = dl.headers.get("Content-Type", "unknown").split(";")[0].strip()
                snippet = first_chunk[:120].decode("utf-8", errors="replace").replace("\n", " ")
                return None, (
                    f"The URL did not return a valid image (got Content-Type '{content_type}'; "
                    f"response starts with: {snippet!r}). "
                    "Please provide a direct image URL (ending in .jpg, .png, .webp, etc.)."
                )

            upload_name = f"xandsuite_i2v_{uuid.uuid4().hex[:8]}{detected_ext}"
            new_tmp = os.path.join(os.path.dirname(tmp_path), upload_name)
            os.rename(tmp_path, new_tmp)
            tmp_path = new_tmp
            upload_path = tmp_path
        else:
            if not os.path.isfile(image_source):
                return None, f"Input image file not found: '{image_source}'"
            upload_path = image_source
            upload_name = os.path.basename(image_source)

        _mime_map = {
            ".jpg": "image/jpeg",
            ".jpeg": "image/jpeg",
            ".png": "image/png",
            ".webp": "image/webp",
            ".gif": "image/gif",
            ".bmp": "image/bmp",
        }
        _up_ext = os.path.splitext(upload_name)[1].lower()
        _mime = _mime_map.get(_up_ext, "image/png")

        with open(upload_path, "rb") as img_fh:
            try:
                resp = _requests.post(
                    f"{COMFYUI_URL}/upload/image",
                    files={"image": (upload_name, img_fh, _mime)},
                    data={"type": "input", "overwrite": "true"},
                    timeout=60,
                )
                resp.raise_for_status()
            except _requests.exceptions.ConnectionError:
                return None, f"Cannot connect to ComfyUI at {COMFYUI_URL}. Is it running?"
            except _requests.exceptions.HTTPError as exc:
                body = ""
                try:
                    body = exc.response.text[:400]
                except Exception:
                    pass
                return None, f"ComfyUI upload failed ({exc.response.status_code}): {body}"
            except Exception as exc:
                return None, f"Upload request failed: {exc}"

        upload_data = resp.json()
        comfyui_name: str = upload_data.get("name", "")
        if not comfyui_name:
            return None, f"ComfyUI did not return a filename after upload. Response: {upload_data}"

        return comfyui_name, None

    finally:
        if tmp_path and os.path.exists(tmp_path):
            try:
                os.remove(tmp_path)
            except OSError:
                pass


# ---------------------------------------------------------------------------
# Workflow injection helpers
# ---------------------------------------------------------------------------

_TEXT_PROMPT_NODE_TYPES = {
    "CLIPTextEncode",
    "CLIPTextEncodeSDXL",
    "CLIPTextEncodeFlux",
    "LTXAVTextEncoderLoader",  # LTX 2.3 uses LTXAVTextEncoderLoader; actual text goes to CLIPTextEncode
}

_NEGATIVE_HINT_WORDS = {"negative", "neg", "uncond", "unconditional"}
_POSITIVE_HINT_WORDS = {"positive", "pos", "cond", "prompt"}

_LOAD_IMAGE_NODE_TYPES = {"LoadImage"}


def _classify_text_node(node: dict) -> Optional[str]:
    """Return 'positive', 'negative', or None for a CLIPTextEncode node."""
    text_val = node.get("inputs", {}).get("text", "")

    if isinstance(text_val, str) and text_val.startswith("__") and text_val.endswith("__"):
        return None
    if isinstance(text_val, list):
        return None

    title = (node.get("_meta", {}).get("title", "") or "").lower()
    for noise in ("clip text encode", "(prompt)"):
        title = title.replace(noise, "")
    title = title.strip()

    if title and any(w in title for w in _NEGATIVE_HINT_WORDS):
        return "negative"
    if title and any(w in title for w in _POSITIVE_HINT_WORDS):
        return "positive"

    if isinstance(text_val, str) and len(text_val) < 400:
        low = text_val.lower()
        neg_keywords = {"blurry", "low quality", "watermark", "deformed", "ugly",
                        "bad", "worst quality", "nsfw", "text", "logo", "still frame"}
        if sum(1 for kw in neg_keywords if kw in low) >= 2:
            return "negative"

    return "positive"


def _auto_inject_input_image(workflow: dict, comfyui_filename: str) -> bool:
    """Set the first LoadImage node's image input to the uploaded filename.

    Skipped when the workflow already contains an __INPUT_IMAGE__ placeholder.
    Returns True if a node was updated.
    """
    raw = json.dumps(workflow)
    if "__INPUT_IMAGE__" in raw:
        return False

    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") in _LOAD_IMAGE_NODE_TYPES:
            node.setdefault("inputs", {})["image"] = comfyui_filename
            return True

    return False


def _auto_inject_prompts(workflow: dict, positive: str, negative: str) -> bool:
    """Inject prompts into CLIPTextEncode nodes when no __POSITIVE_PROMPT__ placeholder exists.

    Returns True if the positive prompt was injected.
    """
    raw = json.dumps(workflow)
    if "__POSITIVE_PROMPT__" in raw:
        return False

    pos_injected = False
    neg_injected = False

    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") not in {"CLIPTextEncode", "CLIPTextEncodeSDXL", "CLIPTextEncodeFlux"}:
            continue

        current_val = node.get("inputs", {}).get("text", "")
        if isinstance(current_val, list):
            continue  # wired from another node, not injectable

        role = _classify_text_node(node)
        if role == "positive" and not pos_injected:
            node.setdefault("inputs", {})["text"] = positive
            pos_injected = True
        elif role == "negative" and not neg_injected:
            node.setdefault("inputs", {})["text"] = negative
            neg_injected = True

        if pos_injected and neg_injected:
            break

    return pos_injected


def _auto_inject_random_noise_seeds(workflow: dict, seed: int) -> bool:
    """Randomise noise_seed in all RandomNoise nodes.

    Uses seed for the first node and seed+1 for subsequent nodes so the two
    sampling passes in LTX 2.3 get different — but deterministic — seeds.
    Skipped when the workflow contains a __SEED__ placeholder.
    Returns True if at least one node was updated.
    """
    raw = json.dumps(workflow)
    if "__SEED__" in raw:
        return False

    updated = False
    offset = 0
    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") == "RandomNoise":
            inputs = node.setdefault("inputs", {})
            if "noise_seed" in inputs:
                inputs["noise_seed"] = seed + offset
                offset += 1
                updated = True

    return updated


def _auto_inject_frames(workflow: dict, frames: int) -> bool:
    """Set the frame count on PrimitiveInt nodes whose title suggests frame length.

    Targets nodes with class_type "PrimitiveInt" whose _meta.title contains
    "length", "frames", "frame", or "count" (case-insensitive).
    Skipped when the workflow contains a __FRAMES__ placeholder.
    Returns True if at least one node was updated.
    """
    raw = json.dumps(workflow)
    if "__FRAMES__" in raw:
        return False

    _frame_title_keywords = {"length", "frames", "frame", "count"}
    updated = False

    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") != "PrimitiveInt":
            continue

        title = (node.get("_meta", {}).get("title", "") or "").lower()
        if any(kw in title for kw in _frame_title_keywords):
            node.setdefault("inputs", {})["value"] = frames
            updated = True

    return updated


def _substitute(workflow: dict, replacements: dict) -> dict:
    """Deep-copy the workflow dict replacing placeholder token strings with values."""
    import copy
    wf = copy.deepcopy(workflow)

    def _walk(obj: Any) -> Any:
        if isinstance(obj, dict):
            return {k: _walk(v) for k, v in obj.items()}
        if isinstance(obj, list):
            return [_walk(item) for item in obj]
        if isinstance(obj, str) and obj in replacements:
            return replacements[obj]
        return obj

    return _walk(wf)


# ---------------------------------------------------------------------------
# FastMCP server
# ---------------------------------------------------------------------------
mcp = FastMCP("xandsuite-comfyui-img2video")


@mcp.tool()
def image_to_video(
    prompt: str,
    image_url: str,
    negative_prompt: str = "",
) -> str:
    """
    Animate a still image into a video using ComfyUI and the configured i2v workflow.

    Call this when the user wants to animate, bring to life, or create a video from
    an existing image. The image can be:
    - A previously generated image (use the image_url from a generate_image result)
    - A user-uploaded/attached image — when a user attaches an image to the chat,
      a local URL is automatically injected into the message in the format:
        [Attached image: filename.jpg — local URL: http://localhost:PORT/images/ID]
      Extract that URL and pass it as image_url. ALWAYS use this URL when available
      — do NOT write code to process the image yourself.

    Parameters
    ----------
    prompt          Describe the motion, animation, and desired video content.
                    Be specific about movement, camera motion, and atmosphere.
    image_url       URL of the input image to animate. Use the local URL from
                    "[Attached image: ... — local URL: ...]" in the message, or
                    the image_url from a previous generate_image result.
    negative_prompt Things to avoid in the video (optional). Typical values:
                    "blurry, low quality, still frame, watermark, overlay".
    """
    err = _check_config()
    if err:
        return json.dumps({"error": err})

    if not image_url:
        return json.dumps({"error": "image_url is required. Pass the URL of the image to animate."})

    workflow, load_err = _load_workflow()
    if load_err:
        return json.dumps({"error": load_err})

    out_frames = DEFAULT_FRAMES
    out_seed = int(time.time() * 1000) % (2 ** 32)

    # ── Step 1: Upload the input image to ComfyUI ────────────────────────────
    comfyui_filename, upload_err = _upload_image(image_url)
    if upload_err:
        return json.dumps({"error": f"Failed to upload input image: {upload_err}"})

    # ── Step 2: Inject the input image into the LoadImage node ───────────────
    injected_image = _auto_inject_input_image(workflow, comfyui_filename)

    # ── Step 3: Inject prompts into CLIPTextEncode nodes ─────────────────────
    _auto_inject_prompts(workflow, prompt, negative_prompt or "")

    # ── Step 4: Randomise seeds in all RandomNoise nodes ─────────────────────
    _auto_inject_random_noise_seeds(workflow, out_seed)

    # ── Step 5: Inject frame count into PrimitiveInt / Length nodes ──────────
    _auto_inject_frames(workflow, out_frames)

    # ── Step 6: Substitute explicit placeholder tokens ───────────────────────
    replacements: dict = {
        "__POSITIVE_PROMPT__": prompt,
        "__NEGATIVE_PROMPT__": negative_prompt or "",
        "__INPUT_IMAGE__": comfyui_filename,
        "__FRAMES__": out_frames,
        "__SEED__": out_seed,
    }
    workflow = _substitute(workflow, replacements)

    if not injected_image and "__INPUT_IMAGE__" not in json.dumps({"v": comfyui_filename}):
        return json.dumps({
            "error": (
                "No LoadImage node found in this workflow. "
                "An i2v workflow must contain a LoadImage node to receive the input image. "
                "Open the workflow in ComfyUI, add a LoadImage node, and re-export it."
            )
        })

    # ── Step 7: Submit to ComfyUI ─────────────────────────────────────────────
    client_id = str(uuid.uuid4())
    payload = {"client_id": client_id, "prompt": workflow}

    try:
        resp = _requests.post(
            f"{COMFYUI_URL}/prompt",
            json=payload,
            timeout=30,
        )
        resp.raise_for_status()
    except _requests.exceptions.ConnectionError:
        return json.dumps({"error": f"Cannot connect to ComfyUI at {COMFYUI_URL}. Is it running?"})
    except _requests.exceptions.HTTPError as exc:
        body = ""
        try:
            body = exc.response.text[:400]
        except Exception:
            pass
        return json.dumps({"error": f"ComfyUI returned {exc.response.status_code}: {body}"})
    except Exception as exc:
        return json.dumps({"error": f"Request failed: {exc}"})

    queue_data = resp.json()
    prompt_id: str = queue_data.get("prompt_id", "")
    if not prompt_id:
        return json.dumps({"error": f"ComfyUI did not return a prompt_id. Response: {queue_data}"})

    # ── Step 8: Poll /history until video output appears ─────────────────────
    deadline = time.time() + TIMEOUT_SECS
    video_info: Optional[tuple] = None  # (filename, subfolder, type)
    poll_interval = 2.0
    consecutive_errors = 0
    MAX_CONSECUTIVE_ERRORS = 10

    while time.time() < deadline:
        time.sleep(poll_interval)
        poll_interval = min(poll_interval + 0.5, 10.0)

        try:
            hist_resp = _requests.get(
                f"{COMFYUI_URL}/history/{prompt_id}",
                timeout=30,
            )
            hist_resp.raise_for_status()
            hist = hist_resp.json()
            consecutive_errors = 0
        except Exception:
            consecutive_errors += 1
            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS:
                return json.dumps({
                    "error": (
                        f"Lost connection to ComfyUI after {MAX_CONSECUTIVE_ERRORS} "
                        "consecutive polling failures. Is ComfyUI still running?"
                    )
                })
            continue

        entry = hist.get(prompt_id)
        if not entry:
            continue

        status_str = entry.get("status", {}).get("status_str", "")
        if status_str == "error":
            messages = entry.get("status", {}).get("messages", [])
            msg_text = "; ".join(str(m) for m in messages) if messages else "unknown error"
            return json.dumps({"error": f"ComfyUI reported an error: {msg_text}"})

        outputs = entry.get("outputs", {})
        for _node_id, node_out in outputs.items():
            for key in ("videos", "images"):
                items = node_out.get(key, [])
                if items:
                    first = items[0]
                    video_info = (
                        first.get("filename", ""),
                        first.get("subfolder", ""),
                        first.get("type", "output"),
                    )
                    break
            if video_info:
                break

        if video_info:
            break

    if video_info is None:
        elapsed = int(time.time() - (deadline - TIMEOUT_SECS))
        return json.dumps({
            "error": (
                f"ComfyUI did not produce output within {elapsed}s "
                f"(timeout: {TIMEOUT_SECS}s / {TIMEOUT_SECS // 60} min). "
                "Check the ComfyUI console for errors, or increase the --timeout setting."
            )
        })

    filename, subfolder, file_type = video_info

    from urllib.parse import quote
    video_url = (
        f"{COMFYUI_URL}/view"
        f"?filename={quote(filename)}"
        f"&subfolder={quote(subfolder)}"
        f"&type={quote(file_type)}"
    )

    return json.dumps({
        "status": "generated",
        "video_url": video_url,
        "filename": filename,
        "frames": out_frames,
        "seed": out_seed,
        "input_image_url": image_url,
        "prompt": prompt,
    })


@mcp.tool()
def get_workflow_info() -> str:
    """
    Return information about the configured i2v workflow: nodes, placeholder locations,
    auto-injectable nodes (LoadImage, CLIPTextEncode, RandomNoise, PrimitiveInt), and
    connector defaults. Use this to explain what parameters can be controlled.
    """
    err = _check_config()
    if err:
        return json.dumps({"error": err})

    workflow, load_err = _load_workflow()
    if load_err:
        return json.dumps({"error": load_err})

    _TOKENS = {
        "__POSITIVE_PROMPT__",
        "__NEGATIVE_PROMPT__",
        "__INPUT_IMAGE__",
        "__FRAMES__",
        "__SEED__",
    }

    nodes = []
    placeholders_found: list = []
    load_image_nodes: list = []
    random_noise_nodes: list = []
    primitive_int_nodes: list = []

    for node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        class_type = node.get("class_type", "unknown")
        inputs = node.get("inputs", {})
        title = node.get("_meta", {}).get("title", "")
        nodes.append({"id": node_id, "class_type": class_type, "title": title})

        if class_type in _LOAD_IMAGE_NODE_TYPES:
            load_image_nodes.append({
                "node_id": node_id,
                "current_image": inputs.get("image", ""),
            })

        if class_type == "RandomNoise":
            random_noise_nodes.append({
                "node_id": node_id,
                "current_seed": inputs.get("noise_seed", ""),
            })

        if class_type == "PrimitiveInt":
            primitive_int_nodes.append({
                "node_id": node_id,
                "title": title,
                "current_value": inputs.get("value", ""),
            })

        for input_name, value in inputs.items():
            if isinstance(value, str) and value in _TOKENS:
                placeholders_found.append({
                    "node_id": node_id,
                    "class_type": class_type,
                    "input": input_name,
                    "token": value,
                })

    # Detect auto-injectable prompt nodes
    auto_prompt_nodes: list = []
    has_prompt_placeholder = "__POSITIVE_PROMPT__" in {p["token"] for p in placeholders_found}
    if not has_prompt_placeholder:
        for node_id, node in workflow.items():
            if not isinstance(node, dict):
                continue
            if node.get("class_type", "") not in {"CLIPTextEncode", "CLIPTextEncodeSDXL", "CLIPTextEncodeFlux"}:
                continue
            current_val = node.get("inputs", {}).get("text", "")
            if isinstance(current_val, list):
                continue
            role = _classify_text_node(node)
            if role:
                preview = (str(current_val)[:80] + "…") if len(str(current_val)) > 80 else str(current_val)
                auto_prompt_nodes.append({
                    "node_id": node_id,
                    "class_type": node.get("class_type"),
                    "role": role,
                    "current_text_preview": preview,
                })

    return json.dumps({
        "workflow_file": WORKFLOW_FILE,
        "node_count": len(nodes),
        "load_image_nodes": load_image_nodes,
        "random_noise_nodes": random_noise_nodes,
        "primitive_int_nodes": primitive_int_nodes,
        "placeholders_found": placeholders_found,
        "missing_placeholders": sorted(_TOKENS - {p["token"] for p in placeholders_found}),
        "auto_prompt_nodes": auto_prompt_nodes,
        "prompt_injection_mode": "placeholder" if has_prompt_placeholder else "auto-detect",
        "connector_defaults": {
            "frames": DEFAULT_FRAMES,
            "timeout_secs": TIMEOUT_SECS,
        },
        "info": (
            "The package automatically injects the input image into the first LoadImage node, "
            "prompts into CLIPTextEncode nodes, seeds into RandomNoise nodes, and frame count "
            "into PrimitiveInt nodes whose title contains 'length', 'frames', or 'frame'. "
            "For full control, use __INPUT_IMAGE__, __POSITIVE_PROMPT__, __NEGATIVE_PROMPT__, "
            "__FRAMES__, __SEED__ placeholder tokens in your workflow JSON."
        ),
    }, indent=2)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    mcp.run(transport="stdio")
