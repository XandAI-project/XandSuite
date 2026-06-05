"""
XandSuite Package: ComfyUI Image Editing
Edit and transform images via a local ComfyUI instance using any img2img workflow.

Args:
    --url            ComfyUI server base URL (e.g. http://localhost:8188)
    --workflow-file  Absolute path to a ComfyUI API-format workflow JSON file on this machine
    --width          Default output width in pixels (default: 1024)
    --height         Default output height in pixels (default: 1024)
    --steps          Default sampling steps (default: 20)
    --denoise        Default denoise strength 0.0–1.0 (default: 0.7)
    --timeout        Polling timeout in seconds (default: 120)

How it works
------------
The package:
  1. Accepts a source image URL (from a previous generation, user upload, or any URL).
  2. Downloads the image if it is a remote URL, then uploads it to ComfyUI's
     input folder via POST /upload/image.
  3. Finds the first LoadImage node in the workflow and sets its input to the
     uploaded filename.
  4. Injects prompts and substitutes placeholders in the workflow JSON.
  5. Submits the workflow to /prompt and polls /history for the result.

Prompt injection
----------------
By default, prompts are injected **automatically** into CLIPTextEncode nodes
(same heuristic as comfyui_image.py). Explicit placeholders are optional:

  __POSITIVE_PROMPT__   positive text prompt
  __NEGATIVE_PROMPT__   negative text prompt
  __INPUT_IMAGE__       input filename (set by the package automatically)
  __WIDTH__             output width (integer)
  __HEIGHT__            output height (integer)
  __STEPS__             sampling steps (integer)
  __SEED__              random seed (integer)
  __DENOISE__           denoise strength (float, 0.0–1.0)

When __INPUT_IMAGE__ is present in the workflow, the auto-injection of the
LoadImage node is skipped and only the placeholder is substituted. When
__DENOISE__ is present, the default denoise value is substituted there too.

Workflow requirements
---------------------
The workflow must contain at least one LoadImage node (class_type "LoadImage")
for the input image to be injected. If none is found and __INPUT_IMAGE__ is
also absent, the tool returns an error.

denoise guidance
----------------
  0.0 – 0.3   Minor corrections (colour, lighting tweaks)
  0.3 – 0.6   Moderate restyle (style transfer, moderate changes)
  0.6 – 0.85  Major transformation (heavy restyle, scene changes)
  0.85 – 1.0  Near-full regeneration (essentially txt2img with loose guidance)
"""

import argparse
import json
import os
import tempfile
import time
import uuid
from typing import Any

from mcp.server.fastmcp import FastMCP

# ---------------------------------------------------------------------------
# CLI args — parsed before FastMCP initialises to avoid argument conflicts
# ---------------------------------------------------------------------------
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--url", default="")
_parser.add_argument("--workflow-file", dest="workflow_file", default="")
_parser.add_argument("--width", type=int, default=1024)
_parser.add_argument("--height", type=int, default=1024)
_parser.add_argument("--steps", type=int, default=20)
_parser.add_argument("--denoise", type=float, default=0.7)
_parser.add_argument("--timeout", type=int, default=120)
_known, _ = _parser.parse_known_args()

COMFYUI_URL: str = _known.url.rstrip("/")
WORKFLOW_FILE: str = _known.workflow_file
DEFAULT_WIDTH: int = _known.width
DEFAULT_HEIGHT: int = _known.height
DEFAULT_STEPS: int = _known.steps
DEFAULT_DENOISE: float = _known.denoise
TIMEOUT_SECS: int = _known.timeout

# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------
try:
    import requests as _requests
    _REQUESTS_OK = True
except ImportError:
    _REQUESTS_OK = False


def _check_config() -> str | None:
    """Return an error string if the connector is not fully configured."""
    if not COMFYUI_URL:
        return "ComfyUI URL not configured. Set --url when installing the package."
    if not WORKFLOW_FILE:
        return "Workflow file not configured. Browse for your API-format workflow JSON when installing."
    if not _REQUESTS_OK:
        return "The 'requests' library is not installed. Run: pip install requests"
    return None


def _load_workflow() -> tuple[dict | None, str | None]:
    """Load the workflow JSON from the local file path provided at install time."""
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
# Image upload helper
# ---------------------------------------------------------------------------

_DOWNLOAD_HEADERS = {
    # Mimic a real browser so CDNs / auth-gated servers don't return HTML error pages.
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/124.0.0.0 Safari/537.36"
    ),
    "Accept": "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
}

# Known image magic bytes → canonical extension.
_IMAGE_SIGNATURES: list[tuple[bytes, str]] = [
    (b"\x89PNG\r\n\x1a\n", ".png"),
    (b"\xff\xd8\xff", ".jpg"),
    (b"RIFF", ".webp"),   # followed by WEBP at offset 8, checked below
    (b"GIF87a", ".gif"),
    (b"GIF89a", ".gif"),
    (b"BM", ".bmp"),
    (b"\x00\x00\x01\x00", ".ico"),
]


def _detect_image_ext(data: bytes) -> str | None:
    """Return a file extension if ``data`` looks like a supported image, else None."""
    for magic, ext in _IMAGE_SIGNATURES:
        if data[:len(magic)] == magic:
            if ext == ".webp" and data[8:12] != b"WEBP":
                continue
            return ext
    return None


def _upload_image(image_source: str) -> tuple[str | None, str | None]:
    """Upload an image to ComfyUI's input folder.

    ``image_source`` may be:
    - A remote URL (http/https) — downloaded first to a temp file, then uploaded.
    - A local file path — uploaded directly.

    Returns (comfyui_filename, error_string).
    ``comfyui_filename`` is the name ComfyUI assigned to the uploaded file in
    its ``input/`` folder (e.g. ``"image.png"``).
    """
    tmp_path: str | None = None

    try:
        if image_source.startswith("http://") or image_source.startswith("https://"):
            # Download to a temp file with a browser-like UA to avoid HTML error pages.
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

            # Read the first 512 bytes to validate it is actually an image.
            first_chunk = b""
            tmp_fd, tmp_path = tempfile.mkstemp(suffix=".bin", prefix="xandsuite_edit_")
            with os.fdopen(tmp_fd, "wb") as fh:
                for chunk in dl.iter_content(chunk_size=8192):
                    if not first_chunk:
                        first_chunk = chunk[:512]
                    fh.write(chunk)

            detected_ext = _detect_image_ext(first_chunk)
            if detected_ext is None:
                # Check what the server actually sent so we can give a helpful message.
                content_type = dl.headers.get("Content-Type", "unknown").split(";")[0].strip()
                snippet = first_chunk[:120].decode("utf-8", errors="replace").replace("\n", " ")
                return None, (
                    f"The URL did not return a valid image (got Content-Type '{content_type}'; "
                    f"response starts with: {snippet!r}). "
                    "The URL may require login, have expired, or be a web page rather than an image file. "
                    "Please provide a direct image URL (ending in .jpg, .png, .webp, etc.) "
                    "or upload the file locally."
                )

            # Rename the temp file with the correct extension for ComfyUI.
            upload_name = f"xandsuite_edit_{uuid.uuid4().hex[:8]}{detected_ext}"
            new_tmp = os.path.join(os.path.dirname(tmp_path), upload_name)
            os.rename(tmp_path, new_tmp)
            tmp_path = new_tmp
            upload_path = tmp_path
        else:
            # Local file path
            if not os.path.isfile(image_source):
                return None, f"Input image file not found: '{image_source}'"
            # Validate the file is actually an image (guards against empty or
            # corrupt files from gallery resolution).
            try:
                with open(image_source, "rb") as _check_fh:
                    _check_bytes = _check_fh.read(512)
                if not _check_bytes:
                    return None, (
                        f"Input image file is empty (0 bytes): '{image_source}'. "
                        "The gallery image may not have been saved correctly."
                    )
                if _detect_image_ext(_check_bytes) is None:
                    snippet = _check_bytes[:80].decode("utf-8", errors="replace").replace("\n", " ")
                    return None, (
                        f"Input file does not appear to be a valid image: '{image_source}' "
                        f"(starts with: {snippet!r}). Expected PNG, JPEG, WebP, GIF or BMP."
                    )
            except OSError as exc:
                return None, f"Could not read input image file: {exc}"
            upload_path = image_source
            upload_name = os.path.basename(image_source)

        # Determine MIME type from extension for the multipart upload.
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

        # Upload to ComfyUI
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
        # Clean up temp file if we created one
        if tmp_path and os.path.exists(tmp_path):
            try:
                os.remove(tmp_path)
            except OSError:
                pass


# ---------------------------------------------------------------------------
# Workflow injection helpers
# ---------------------------------------------------------------------------

# Node class_types whose inputs carry a text prompt, mapped to the field name.
# Nodes with multiple possible field names use a tuple (tried left-to-right).
_TEXT_PROMPT_FIELD: dict[str, str | tuple[str, ...]] = {
    "CLIPTextEncode": "text",
    "CLIPTextEncodeSDXL": "text",
    "CLIPTextEncodeFlux": "text",
    # Qwen / FLUX-Kontext / AuraFlow edit workflows
    "PrimitiveStringMultiline": "value",      # standalone prompt primitive
    "TextEncodeQwenImageEditPlus": "prompt",  # Qwen image-edit text encoder
}

_NEGATIVE_HINT_WORDS = {"negative", "neg", "uncond", "unconditional"}
_POSITIVE_HINT_WORDS = {"positive", "pos", "cond", "prompt"}

# Node class_types that load an input image.
_LOAD_IMAGE_NODE_TYPES = {"LoadImage"}


def _text_field(class_type: str) -> str | None:
    """Return the field name that carries the text prompt for this node type."""
    spec = _TEXT_PROMPT_FIELD.get(class_type)
    if spec is None:
        return None
    return spec if isinstance(spec, str) else spec[0]


def _classify_text_node(node: dict) -> str | None:
    """Return 'positive', 'negative', or None for a text-prompt node.

    Works for CLIPTextEncode, PrimitiveStringMultiline, TextEncodeQwenImageEditPlus, etc.
    """
    class_type = node.get("class_type", "")
    field = _text_field(class_type)
    if field is None:
        return None

    text_val = node.get("inputs", {}).get(field, "")

    # Already a placeholder token — let _substitute() handle it.
    if isinstance(text_val, str) and text_val.startswith("__") and text_val.endswith("__"):
        return None

    # If the prompt input is a node reference (list) it's wired up dynamically;
    # we can only inject into nodes where the value is a plain string.
    if isinstance(text_val, list):
        return None

    title = (node.get("_meta", {}).get("title", "") or "").lower()
    # Strip common boilerplate from title for cleaner matching.
    for noise in ("clip text encode", "(prompt)", "textencode", "textencodeqwen"):
        title = title.replace(noise, "")
    title = title.strip()

    if title and any(w in title for w in _NEGATIVE_HINT_WORDS):
        return "negative"
    if title and any(w in title for w in _POSITIVE_HINT_WORDS):
        return "positive"

    # Heuristic: if the current value contains typical negative-prompt keywords,
    # treat the node as the negative encoder.
    if isinstance(text_val, str) and len(text_val) < 400:
        low = text_val.lower()
        neg_keywords = {"blurry", "low quality", "watermark", "deformed", "ugly",
                        "bad", "worst quality", "nsfw", "text", "logo"}
        if sum(1 for kw in neg_keywords if kw in low) >= 2:
            return "negative"

    # Empty string prompt with no title → likely the negative encoder
    # (Qwen workflows use an empty TextEncodeQwenImageEditPlus for negative).
    if isinstance(text_val, str) and text_val == "" and class_type == "TextEncodeQwenImageEditPlus":
        return "negative"

    return "positive"


def _auto_inject_prompts(workflow: dict, positive: str, negative: str) -> bool:
    """Inject prompts into text-prompt nodes when no __POSITIVE_PROMPT__ placeholder exists.

    Handles CLIPTextEncode, PrimitiveStringMultiline (Qwen/FLUX-Kontext), and
    TextEncodeQwenImageEditPlus nodes automatically.
    """
    raw = json.dumps(workflow)
    if "__POSITIVE_PROMPT__" in raw:
        return False

    pos_injected = False
    neg_injected = False

    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        class_type = node.get("class_type", "")
        field = _text_field(class_type)
        if field is None:
            continue

        # Only inject into nodes where the prompt input is a plain string
        # (not a node reference).
        current_val = node.get("inputs", {}).get(field, "")
        if isinstance(current_val, list):
            continue

        role = _classify_text_node(node)
        if role == "positive" and not pos_injected:
            node.setdefault("inputs", {})[field] = positive
            pos_injected = True
        elif role == "negative" and not neg_injected:
            node.setdefault("inputs", {})[field] = negative
            neg_injected = True

        if pos_injected and neg_injected:
            break

    return pos_injected


def _auto_inject_input_image(workflow: dict, comfyui_filename: str) -> bool:
    """Set the first LoadImage node's image input to the uploaded filename.

    Returns True if a LoadImage node was found and updated, False otherwise.
    Only runs when the workflow does NOT contain an __INPUT_IMAGE__ placeholder
    (in that case, _substitute() handles it).
    """
    raw = json.dumps(workflow)
    if "__INPUT_IMAGE__" in raw:
        return False  # placeholder path — let _substitute() handle it

    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") in _LOAD_IMAGE_NODE_TYPES:
            node.setdefault("inputs", {})["image"] = comfyui_filename
            return True

    return False


def _auto_inject_denoise(workflow: dict, denoise: float) -> bool:
    """Set the denoise input on the first KSampler / KSamplerAdvanced node.

    Only runs when the workflow does NOT contain a __DENOISE__ placeholder.
    Returns True if a sampler node was updated.

    IMPORTANT: Never overrides a workflow-level denoise of 1.0 with a lower value.
    Workflows with denoise=1.0 are typically Lightning / Turbo few-step models
    (e.g. Qwen Image Edit Lightning, SDXL-Turbo, SD-Turbo) that require full
    denoising to operate correctly — setting a lower value partially aborts their
    sampling schedule and produces unchanged or degraded output.
    """
    raw = json.dumps(workflow)
    if "__DENOISE__" in raw:
        return False

    sampler_types = {"KSampler", "KSamplerAdvanced", "SamplerCustom"}
    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") in sampler_types:
            inputs = node.setdefault("inputs", {})
            try:
                current = float(inputs.get("denoise", 1.0))
            except (TypeError, ValueError):
                current = 1.0
            # Never lower denoise=1.0 — protects Lightning/Turbo models.
            if current >= 1.0 and denoise < 1.0:
                return False
            if "denoise" in inputs or node.get("class_type") == "KSampler":
                inputs["denoise"] = denoise
                return True

    return False


def _auto_inject_seed(workflow: dict, seed: int) -> bool:
    """Randomise the seed in KSampler / KSamplerAdvanced nodes.

    Workflows typically ship with a hardcoded seed. Without randomisation every
    call with the same input image would produce identical output.
    Only runs when the workflow does NOT contain a __SEED__ placeholder
    (in that case _substitute() handles the injection instead).
    Returns True if at least one sampler node was updated.
    """
    raw = json.dumps(workflow)
    if "__SEED__" in raw:
        return False

    sampler_types = {"KSampler", "KSamplerAdvanced", "SamplerCustom"}
    updated = False
    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type", "") in sampler_types:
            inputs = node.setdefault("inputs", {})
            if "seed" in inputs:
                inputs["seed"] = seed
                updated = True
    return updated


def _substitute(workflow: dict, replacements: dict[str, Any]) -> dict:
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
mcp = FastMCP("xandsuite-comfyui-image-edit")


@mcp.tool()
def edit_image(
    prompt: str,
    image_url: str,
    negative_prompt: str = "",
) -> str:
    """
    Edit or transform an existing image using ComfyUI and an img2img workflow.

    Call this when the user wants to modify, restyle, recolour, or transform an
    existing image. The image can be:
    - A previously generated image (use the image_url from a generate_image result)
    - A user-uploaded/attached image — when a user attaches an image to the chat,
      a local URL is automatically injected into the message in the format:
        [Attached image: filename.jpg — local URL: http://localhost:PORT/images/ID]
      Extract that URL and pass it as image_url. ALWAYS use this URL when available
      — do NOT write code to process the image yourself.

    Parameters
    ----------
    prompt          Describe the desired result or the changes to apply.
                    Be specific and detailed — for these models the prompt is the
                    ONLY way to control editing strength and direction.
    image_url       URL of the input image to edit. Use the local URL from
                    "[Attached image: ... — local URL: ...]" in the message, or
                    the image_url from a previous generate_image result.
    negative_prompt Things to avoid in the result (optional).
    """
    err = _check_config()
    if err:
        return json.dumps({"error": err})

    if not image_url:
        return json.dumps({"error": "image_url is required. Pass the URL of the image to edit."})

    workflow, load_err = _load_workflow()
    if load_err:
        return json.dumps({"error": load_err})

    # All generation parameters come from the workflow or connector defaults.
    # They are NOT exposed to the LLM — the workflow controls sampling behaviour.
    out_width = DEFAULT_WIDTH
    out_height = DEFAULT_HEIGHT
    out_steps = DEFAULT_STEPS
    out_denoise = DEFAULT_DENOISE
    out_seed = int(time.time() * 1000) % (2 ** 32)

    # ── Step 1: Upload the input image to ComfyUI ────────────────────────────
    comfyui_filename, upload_err = _upload_image(image_url)
    if upload_err:
        return json.dumps({"error": f"Failed to upload input image: {upload_err}"})

    # ── Step 2: Inject the input image into the LoadImage node ───────────────
    injected_image = _auto_inject_input_image(workflow, comfyui_filename)

    # ── Step 3: Inject prompts into text-prompt nodes ─────────────────────────
    _auto_inject_prompts(workflow, prompt, negative_prompt or "")

    # ── Step 4a: Auto-inject denoise (skipped for Lightning models w/ denoise=1) ──
    _auto_inject_denoise(workflow, out_denoise)

    # ── Step 4b: Always randomise seed so each call gives a unique result ─────
    _auto_inject_seed(workflow, out_seed)

    # ── Step 5: Substitute explicit placeholder tokens ───────────────────────
    replacements: dict[str, Any] = {
        "__POSITIVE_PROMPT__": prompt,
        "__NEGATIVE_PROMPT__": negative_prompt or "",
        "__INPUT_IMAGE__": comfyui_filename,
        "__WIDTH__": out_width,
        "__HEIGHT__": out_height,
        "__STEPS__": out_steps,
        "__SEED__": out_seed,
        "__DENOISE__": out_denoise,
    }
    workflow = _substitute(workflow, replacements)

    # Warn if no LoadImage node was found and __INPUT_IMAGE__ was not a placeholder
    if not injected_image and "__INPUT_IMAGE__" not in json.dumps(replacements):
        return json.dumps({
            "error": (
                "No LoadImage node found in this workflow. "
                "An img2img workflow must contain a LoadImage node to receive the input image. "
                "Open the workflow in ComfyUI, add a LoadImage node, and re-export it."
            )
        })

    # ── Step 6: Submit to ComfyUI ─────────────────────────────────────────────
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

    # ── Step 7: Poll /history until output appears ────────────────────────────
    deadline = time.time() + TIMEOUT_SECS
    image_info: tuple[str, str, str] | None = None  # (filename, subfolder, type)
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
            items = node_out.get("images", [])
            if items:
                first = items[0]
                image_info = (
                    first.get("filename", ""),
                    first.get("subfolder", ""),
                    first.get("type", "output"),
                )
                break

        if image_info:
            break

    if image_info is None:
        elapsed = int(time.time() - (deadline - TIMEOUT_SECS))
        return json.dumps({
            "error": (
                f"ComfyUI did not produce output within {elapsed}s "
                f"(timeout: {TIMEOUT_SECS}s). "
                "Check the ComfyUI console for errors, or increase the --timeout setting."
            )
        })

    filename, subfolder, file_type = image_info

    from urllib.parse import quote
    result_url = (
        f"{COMFYUI_URL}/view"
        f"?filename={quote(filename)}"
        f"&subfolder={quote(subfolder)}"
        f"&type={quote(file_type)}"
    )

    return json.dumps({
        "status": "generated",
        "image_url": result_url,
        "filename": filename,
        "width": out_width,
        "height": out_height,
        "denoise": out_denoise,
        "input_image_url": image_url,
        "prompt": prompt,
    })


@mcp.tool()
def get_workflow_info() -> str:
    """
    Return information about the configured img2img workflow: nodes, placeholder
    locations, auto-injectable nodes, and whether a LoadImage node is present.
    Use this to explain what parameters can be controlled.
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
        "__WIDTH__",
        "__HEIGHT__",
        "__STEPS__",
        "__SEED__",
        "__DENOISE__",
    }

    nodes = []
    placeholders_found: list[dict] = []
    load_image_nodes: list[dict] = []

    for node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        class_type = node.get("class_type", "unknown")
        inputs = node.get("inputs", {})
        nodes.append({"id": node_id, "class_type": class_type})

        # Track LoadImage nodes
        if class_type in _LOAD_IMAGE_NODE_TYPES:
            current_image = inputs.get("image", "")
            load_image_nodes.append({
                "node_id": node_id,
                "current_image": current_image,
                "has_placeholder": isinstance(current_image, str) and current_image == "__INPUT_IMAGE__",
            })

        # Track placeholder tokens
        for input_name, value in inputs.items():
            if isinstance(value, str) and value in _TOKENS:
                placeholders_found.append({
                    "node_id": node_id,
                    "class_type": class_type,
                    "input": input_name,
                    "token": value,
                })

    missing = _TOKENS - {p["token"] for p in placeholders_found}

    # Detect auto-injectable text-prompt nodes
    auto_prompt_nodes: list[dict] = []
    has_prompt_placeholder = "__POSITIVE_PROMPT__" in {p["token"] for p in placeholders_found}
    if not has_prompt_placeholder:
        for node_id, node in workflow.items():
            if not isinstance(node, dict):
                continue
            class_type = node.get("class_type", "")
            field = _text_field(class_type)
            if field is None:
                continue
            current_val = node.get("inputs", {}).get(field, "")
            if isinstance(current_val, list):
                continue  # wired from another node, not injectable
            role = _classify_text_node(node)
            if role:
                preview = (str(current_val)[:80] + "…") if len(str(current_val)) > 80 else str(current_val)
                auto_prompt_nodes.append({
                    "node_id": node_id,
                    "class_type": class_type,
                    "field": field,
                    "role": role,
                    "current_text_preview": preview,
                })

    has_load_image = len(load_image_nodes) > 0
    has_input_image_placeholder = "__INPUT_IMAGE__" in {p["token"] for p in placeholders_found}
    image_injection_mode = "placeholder" if has_input_image_placeholder else (
        "auto-detect" if has_load_image else "none — no LoadImage node found"
    )

    return json.dumps({
        "workflow_file": WORKFLOW_FILE,
        "node_count": len(nodes),
        "nodes": nodes,
        "load_image_nodes": load_image_nodes,
        "image_injection_mode": image_injection_mode,
        "placeholders_found": placeholders_found,
        "missing_placeholders": sorted(missing),
        "auto_prompt_nodes": auto_prompt_nodes,
        "prompt_injection_mode": "placeholder" if has_prompt_placeholder else "auto-detect",
        "connector_defaults": {
            "width": DEFAULT_WIDTH,
            "height": DEFAULT_HEIGHT,
            "steps": DEFAULT_STEPS,
            "denoise": DEFAULT_DENOISE,
            "timeout_secs": TIMEOUT_SECS,
        },
        "info": (
            "The package automatically injects the input image into the first LoadImage node "
            "and prompts into CLIPTextEncode nodes. For full control, replace inputs with "
            "__INPUT_IMAGE__, __POSITIVE_PROMPT__, __NEGATIVE_PROMPT__, __WIDTH__, __HEIGHT__, "
            "__STEPS__, __SEED__, __DENOISE__ in your workflow JSON."
        ),
    }, indent=2)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    mcp.run(transport="stdio")
