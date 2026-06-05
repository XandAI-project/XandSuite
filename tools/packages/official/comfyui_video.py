"""
XandSuite Package: ComfyUI Video Generation
Generate videos via a local ComfyUI instance using any video workflow.

Args:
    --url            ComfyUI server base URL (e.g. http://localhost:8188)
    --workflow-file  Absolute path to a ComfyUI API-format workflow JSON file on this machine
    --width          Default output width in pixels (default: 848)
    --height         Default output height in pixels (default: 480)
    --frames         Default number of frames to generate (default: 25)
    --timeout        Polling timeout in seconds (default: 1800, i.e. 30 min)

Prompt injection
----------------
By default, prompts are injected **automatically**: the package scans the
workflow for CLIPTextEncode (and similar) nodes, classifies them as positive
or negative based on their title / content, and replaces their ``text`` input
with the prompt the LLM generates.  This means you can point the package at
any exported ComfyUI workflow and it will just work — no manual editing needed.

Workflow placeholders (optional, advanced)
------------------------------------------
For full control you can replace specific input values in the workflow JSON
with placeholder tokens.  When ``__POSITIVE_PROMPT__`` is present the auto-
detection is skipped and only explicit placeholders are substituted:

  __POSITIVE_PROMPT__   positive text prompt
  __NEGATIVE_PROMPT__   negative text prompt
  __WIDTH__             output width (integer)
  __HEIGHT__            output height (integer)
  __FRAMES__            frame count (integer)
  __STEPS__             sampling steps (integer)
  __SEED__              random seed (integer)

Any node input whose string value exactly matches one of these tokens will be
substituted at generation time.
"""

import argparse
import json
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
_parser.add_argument("--width", type=int, default=848)
_parser.add_argument("--height", type=int, default=480)
_parser.add_argument("--frames", type=int, default=25)
_parser.add_argument("--timeout", type=int, default=1800)
_known, _ = _parser.parse_known_args()

COMFYUI_URL: str = _known.url.rstrip("/")
WORKFLOW_FILE: str = _known.workflow_file
DEFAULT_WIDTH: int = _known.width
DEFAULT_HEIGHT: int = _known.height
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
# Placeholder substitution
# ---------------------------------------------------------------------------

_PLACEHOLDER_TYPES = {
    "__WIDTH__": int,
    "__HEIGHT__": int,
    "__FRAMES__": int,
    "__STEPS__": int,
    "__SEED__": int,
}

# Node class_types whose "text" input carries a text prompt.
_TEXT_PROMPT_NODE_TYPES = {
    "CLIPTextEncode",
    "CLIPTextEncodeSDXL",
    "CLIPTextEncodeFlux",
}

_NEGATIVE_HINT_WORDS = {"negative", "neg", "uncond", "unconditional"}
_POSITIVE_HINT_WORDS = {"positive", "pos", "cond", "prompt"}


def _classify_text_node(node: dict) -> str | None:
    """Return 'positive', 'negative', or None for a text-prompt node.

    Heuristics (checked in order):
    1. If the ``text`` value is already a placeholder token → skip (handled
       by the normal substitution path).
    2. Node title contains an *unambiguous* negative or positive keyword
       (ignoring generic titles like "CLIP Text Encode (Prompt)" that
       ComfyUI assigns to every CLIPTextEncode node by default).
    3. The text content itself looks like a typical negative prompt (short,
       mostly quality-exclusion terms).
    4. Otherwise assume positive (the main creative prompt is usually longer).
    """
    text_val = node.get("inputs", {}).get("text", "")
    if isinstance(text_val, str) and text_val.startswith("__") and text_val.endswith("__"):
        return None  # already a placeholder

    title = (node.get("_meta", {}).get("title", "") or "").lower()

    # Strip the generic default title so it doesn't trigger false positives.
    # "clip text encode (prompt)" is the default for every CLIPTextEncode node.
    cleaned_title = title.replace("clip text encode", "").replace("(prompt)", "").strip()

    if cleaned_title and any(w in cleaned_title for w in _NEGATIVE_HINT_WORDS):
        return "negative"
    if cleaned_title and any(w in cleaned_title for w in _POSITIVE_HINT_WORDS):
        return "positive"

    # Content-based: short text with typical negative-prompt keywords → negative
    if isinstance(text_val, str) and len(text_val) < 300:
        low = text_val.lower()
        neg_keywords = {"blurry", "low quality", "watermark", "deformed", "ugly",
                        "bad", "worst quality", "nsfw", "text", "logo"}
        if sum(1 for kw in neg_keywords if kw in low) >= 2:
            return "negative"

    return "positive"


def _auto_inject_prompts(workflow: dict, positive: str, negative: str) -> bool:
    """Find CLIPTextEncode-like nodes and inject prompts when no placeholders exist.

    Returns True if at least the positive prompt was injected.
    """
    # First check whether the workflow already uses placeholder tokens.
    raw = json.dumps(workflow)
    if "__POSITIVE_PROMPT__" in raw:
        return False  # user set up placeholders — let normal substitution handle it

    pos_injected = False
    neg_injected = False

    for _node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        class_type = node.get("class_type", "")
        if class_type not in _TEXT_PROMPT_NODE_TYPES:
            continue

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


def _substitute(workflow: dict, replacements: dict[str, Any]) -> dict:
    """
    Deep-copy the workflow dict replacing token strings with their values.

    String placeholders are replaced in-place; integer placeholders replace
    the string node-input value with the actual integer so ComfyUI receives
    the correct type.
    """
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
mcp = FastMCP("xandsuite-comfyui-video")


@mcp.tool()
def generate_video(
    prompt: str,
    negative_prompt: str = "",
    width: int = 0,
    height: int = 0,
    frames: int = 0,
    steps: int = 20,
    seed: int = -1,
) -> str:
    """
    Generate a video using ComfyUI and the configured workflow.

    Call this whenever the user asks for a video, animation, or clip.
    Write a detailed, descriptive prompt. The video URL will be returned
    for display in the chat.

    Parameters
    ----------
    prompt          Positive text description of the desired video.
    negative_prompt Things to avoid in the video (optional).
    width           Output width in pixels. 0 = use connector default.
    height          Output height in pixels. 0 = use connector default.
    frames          Number of frames. 0 = use connector default.
    steps           Sampling steps (default 20).
    seed            Random seed (-1 for random).
    """
    err = _check_config()
    if err:
        return json.dumps({"error": err})

    workflow, load_err = _load_workflow()
    if load_err:
        return json.dumps({"error": load_err})

    # Resolve defaults
    out_width = width if width > 0 else DEFAULT_WIDTH
    out_height = height if height > 0 else DEFAULT_HEIGHT
    out_frames = frames if frames > 0 else DEFAULT_FRAMES
    out_seed = seed if seed >= 0 else int(time.time() * 1000) % (2 ** 32)

    # Auto-inject prompts into CLIPTextEncode nodes when the workflow does
    # not use explicit placeholder tokens.  This lets users point at any
    # exported ComfyUI workflow without manual editing.
    _auto_inject_prompts(workflow, prompt, negative_prompt or "")

    replacements: dict[str, Any] = {
        "__POSITIVE_PROMPT__": prompt,
        "__NEGATIVE_PROMPT__": negative_prompt or "",
        "__WIDTH__": out_width,
        "__HEIGHT__": out_height,
        "__FRAMES__": out_frames,
        "__STEPS__": steps,
        "__SEED__": out_seed,
    }

    workflow = _substitute(workflow, replacements)

    client_id = str(uuid.uuid4())
    payload = {"client_id": client_id, "prompt": workflow}

    # Submit prompt
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

    # Poll /history until outputs appear or timeout.
    # Video generation can legitimately take 10-12+ minutes, so we use an
    # adaptive polling interval (start fast, slow down) and tolerate transient
    # HTTP failures instead of aborting on the first hiccup.
    deadline = time.time() + TIMEOUT_SECS
    video_info: tuple[str, str, str] | None = None  # (filename, subfolder, type)
    poll_interval = 2.0
    consecutive_errors = 0
    MAX_CONSECUTIVE_ERRORS = 10

    while time.time() < deadline:
        time.sleep(poll_interval)
        # Ramp the interval: 2 → 3 → 4 … → 10 s max
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
                        f"consecutive polling failures. Is ComfyUI still running?"
                    )
                })
            continue

        entry = hist.get(prompt_id)
        if not entry:
            continue

        # Check for error status
        status_str = (
            entry.get("status", {}).get("status_str", "")
        )
        if status_str == "error":
            messages = entry.get("status", {}).get("messages", [])
            msg_text = "; ".join(str(m) for m in messages) if messages else "unknown error"
            return json.dumps({"error": f"ComfyUI reported an error: {msg_text}"})

        # Scan output nodes — prefer "videos", fall back to "images" (gif workflows)
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

    # Build the view URL
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
        "width": out_width,
        "height": out_height,
        "frames": out_frames,
        "seed": out_seed,
        "prompt": prompt,
    })


@mcp.tool()
def get_workflow_info() -> str:
    """
    Return information about the configured workflow: the list of nodes and
    which inputs currently contain placeholder tokens. Use this to explain
    to the user what parameters can be controlled and how to set up the workflow.
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
        "__WIDTH__",
        "__HEIGHT__",
        "__FRAMES__",
        "__STEPS__",
        "__SEED__",
    }

    nodes = []
    placeholders_found: list[dict] = []

    for node_id, node in workflow.items():
        if not isinstance(node, dict):
            continue
        class_type = node.get("class_type", "unknown")
        inputs = node.get("inputs", {})
        nodes.append({"id": node_id, "class_type": class_type})
        for input_name, value in inputs.items():
            if isinstance(value, str) and value in _TOKENS:
                placeholders_found.append({
                    "node_id": node_id,
                    "class_type": class_type,
                    "input": input_name,
                    "token": value,
                })

    missing = _TOKENS - {p["token"] for p in placeholders_found}

    # Detect auto-injectable text-prompt nodes (used when no placeholders are set).
    auto_prompt_nodes: list[dict] = []
    has_prompt_placeholder = "__POSITIVE_PROMPT__" in {p["token"] for p in placeholders_found}
    if not has_prompt_placeholder:
        for node_id, node in workflow.items():
            if not isinstance(node, dict):
                continue
            if node.get("class_type", "") in _TEXT_PROMPT_NODE_TYPES:
                role = _classify_text_node(node)
                if role:
                    text_val = node.get("inputs", {}).get("text", "")
                    preview = (text_val[:80] + "…") if len(text_val) > 80 else text_val
                    auto_prompt_nodes.append({
                        "node_id": node_id,
                        "class_type": node.get("class_type"),
                        "role": role,
                        "current_text_preview": preview,
                    })

    return json.dumps({
        "workflow_file": WORKFLOW_FILE,
        "node_count": len(nodes),
        "nodes": nodes,
        "placeholders_found": placeholders_found,
        "missing_placeholders": sorted(missing),
        "auto_prompt_nodes": auto_prompt_nodes,
        "prompt_injection_mode": "placeholder" if has_prompt_placeholder else "auto-detect",
        "connector_defaults": {
            "width": DEFAULT_WIDTH,
            "height": DEFAULT_HEIGHT,
            "frames": DEFAULT_FRAMES,
            "timeout_secs": TIMEOUT_SECS,
        },
        "info": (
            "Prompts are injected automatically into CLIPTextEncode nodes when no "
            "__POSITIVE_PROMPT__ placeholder is found. For full control, replace input "
            "values with __POSITIVE_PROMPT__, __NEGATIVE_PROMPT__, __WIDTH__, __HEIGHT__, "
            "__FRAMES__, __STEPS__, __SEED__ in your workflow JSON."
        ),
    }, indent=2)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    mcp.run(transport="stdio")
