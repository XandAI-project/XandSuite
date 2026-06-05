"""
XandSuite Package: Iterative Image Refiner
Self-contained iterative image generation + editing + inspection.

Generates an initial image via ComfyUI, visually inspects it against a
10-category artifact checklist, scores it, and edits to fix issues —
repeating until the quality threshold is met or iterations run out.

All three actions (generate, edit, inspect/log) are tools in THIS package
so the LLM never needs to reach into other packages.

Args
----
    --comfyui-url       ComfyUI server URL (e.g. http://localhost:8188)
    --gen-workflow       Path to txt2img workflow JSON (API format)
    --edit-workflow      Path to img2img workflow JSON (API format)
    --width              Default width (default: 1024)
    --height             Default height (default: 1024)
    --steps              Sampling steps (default: 20)
    --timeout            Polling timeout in seconds (default: 120)
"""

import argparse
import copy
import json
import math
import os
import tempfile
import time
import uuid
from typing import Any, Optional
from urllib.parse import quote

from mcp.server.fastmcp import FastMCP

# ---------------------------------------------------------------------------
# CLI args
# ---------------------------------------------------------------------------
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--comfyui-url", dest="comfyui_url", default="")
_parser.add_argument("--gen-workflow", dest="gen_workflow", default="")
_parser.add_argument("--edit-workflow", dest="edit_workflow", default="")
_parser.add_argument("--width", type=int, default=1024)
_parser.add_argument("--height", type=int, default=1024)
_parser.add_argument("--steps", type=int, default=20)
_parser.add_argument("--timeout", type=int, default=120)
_known, _ = _parser.parse_known_args()

COMFYUI_URL: str = (_known.comfyui_url or "").rstrip("/")
GEN_WORKFLOW_FILE: str = _known.gen_workflow or ""
EDIT_WORKFLOW_FILE: str = _known.edit_workflow or ""
DEFAULT_WIDTH: int = _known.width
DEFAULT_HEIGHT: int = _known.height
DEFAULT_STEPS: int = _known.steps
TIMEOUT_SECS: int = _known.timeout

try:
    import requests as _requests
except ImportError:
    _requests = None  # type: ignore[assignment]

mcp = FastMCP("xandsuite-iterative-image-refiner")

# ---------------------------------------------------------------------------
# ComfyUI HTTP helpers (self-contained, no cross-package dependency)
# ---------------------------------------------------------------------------

_NEGATIVE_HINTS = {"negative", "neg", "uncond", "unconditional"}
_POSITIVE_HINTS = {"positive", "pos", "cond", "prompt"}
_IMAGE_SIGNATURES: list[tuple[bytes, str]] = [
    (b"\x89PNG", ".png"),
    (b"\xff\xd8\xff", ".jpg"),
    (b"RIFF", ".webp"),
    (b"GIF8", ".gif"),
]


def _comfyui_ok() -> str | None:
    """Return an error string if ComfyUI is not configured."""
    if not COMFYUI_URL:
        return ("ComfyUI URL not configured. Set --comfyui-url when "
                "installing the Iterative Image Refiner package.")
    if _requests is None:
        return "requests library not installed."
    return None


def _load_workflow(path: str) -> tuple[dict | None, str | None]:
    if not path:
        return None, "Workflow file path not configured."
    try:
        with open(path, "r", encoding="utf-8") as fh:
            return json.load(fh), None
    except Exception as exc:
        return None, f"Failed to load workflow: {exc}"


def _classify_text_node(node: dict) -> str | None:
    title = (node.get("_meta", {}).get("title", "") or "").lower()
    inputs = node.get("inputs", {})
    text_val = str(inputs.get("text", inputs.get("value", inputs.get("prompt", "")))).lower()
    for w in _NEGATIVE_HINTS:
        if w in title or w in text_val:
            return "negative"
    for w in _POSITIVE_HINTS:
        if w in title or w in text_val:
            return "positive"
    return None


def _inject_prompts(wf: dict, positive: str, negative: str) -> None:
    clip_types = {"CLIPTextEncode", "CLIPTextEncodeSDXL", "CLIPTextEncodeFlux",
                  "PrimitiveStringMultiline", "TextEncodeQwenImageEditPlus"}
    field_map = {"CLIPTextEncode": "text", "CLIPTextEncodeSDXL": "text",
                 "CLIPTextEncodeFlux": "text", "PrimitiveStringMultiline": "value",
                 "TextEncodeQwenImageEditPlus": "prompt"}
    for _nid, node in wf.items():
        if not isinstance(node, dict):
            continue
        ct = node.get("class_type", "")
        if ct not in clip_types:
            continue
        field = field_map.get(ct, "text")
        role = _classify_text_node(node)
        if role == "positive":
            node.setdefault("inputs", {})[field] = positive
        elif role == "negative":
            node.setdefault("inputs", {})[field] = negative


def _substitute(wf: dict, replacements: dict[str, Any]) -> dict:
    wf = copy.deepcopy(wf)
    for _nid, node in wf.items():
        inputs = node.get("inputs", {})
        for key, val in list(inputs.items()):
            if isinstance(val, str) and val in replacements:
                inputs[key] = replacements[val]
    return wf


def _submit_and_poll(workflow: dict) -> tuple[str | None, str | None]:
    """POST workflow, poll until done, return (image_url, error)."""
    client_id = str(uuid.uuid4())
    payload = {"client_id": client_id, "prompt": workflow}
    try:
        resp = _requests.post(f"{COMFYUI_URL}/prompt", json=payload, timeout=30)
        resp.raise_for_status()
    except Exception as exc:
        return None, f"ComfyUI request failed: {exc}"

    prompt_id = resp.json().get("prompt_id", "")
    if not prompt_id:
        return None, "ComfyUI did not return a prompt_id."

    deadline = time.time() + TIMEOUT_SECS
    poll_interval = 2.0
    while time.time() < deadline:
        time.sleep(poll_interval)
        poll_interval = min(poll_interval + 0.5, 10.0)
        try:
            hr = _requests.get(f"{COMFYUI_URL}/history/{prompt_id}", timeout=30)
            hr.raise_for_status()
            hist = hr.json()
        except Exception:
            continue

        entry = hist.get(prompt_id)
        if not entry:
            continue
        status_str = entry.get("status", {}).get("status_str", "")
        if status_str == "error":
            msgs = entry.get("status", {}).get("messages", [])
            return None, f"ComfyUI error: {'; '.join(str(m) for m in msgs)}"

        for _nid, node_out in entry.get("outputs", {}).items():
            images = node_out.get("images", [])
            if images:
                first = images[0]
                fn = first.get("filename", "")
                sf = first.get("subfolder", "")
                ft = first.get("type", "output")
                url = (f"{COMFYUI_URL}/view?filename={quote(fn)}"
                       f"&subfolder={quote(sf)}&type={quote(ft)}")
                return url, None

    return None, f"ComfyUI timed out after {TIMEOUT_SECS}s."


def _upload_image(image_source: str) -> tuple[str | None, str | None]:
    """Upload an image to ComfyUI input folder. Returns (comfyui_name, error)."""
    tmp_path: str | None = None
    try:
        if image_source.startswith("http://") or image_source.startswith("https://"):
            dl = _requests.get(image_source, timeout=60, stream=True)
            dl.raise_for_status()
            tmp_fd, tmp_path = tempfile.mkstemp(suffix=".png", prefix="refiner_")
            with os.fdopen(tmp_fd, "wb") as fh:
                for chunk in dl.iter_content(8192):
                    fh.write(chunk)
            upload_path = tmp_path
            upload_name = os.path.basename(tmp_path)
        else:
            if not os.path.isfile(image_source):
                return None, f"File not found: {image_source}"
            # Validate the file contains actual image data.
            try:
                with open(image_source, "rb") as _cf:
                    _hdr = _cf.read(16)
                if not _hdr:
                    return None, f"Input image file is empty (0 bytes): {image_source}"
                _known = [b"\x89PNG", b"\xff\xd8\xff", b"RIFF", b"GIF8", b"BM"]
                if not any(_hdr.startswith(m) for m in _known):
                    return None, f"Input file does not look like a valid image: {image_source}"
            except OSError as exc:
                return None, f"Could not read input image: {exc}"
            upload_path = image_source
            upload_name = os.path.basename(image_source)

        with open(upload_path, "rb") as fh:
            resp = _requests.post(
                f"{COMFYUI_URL}/upload/image",
                files={"image": (upload_name, fh, "image/png")},
                data={"type": "input", "overwrite": "true"},
                timeout=60,
            )
            resp.raise_for_status()
        name = resp.json().get("name", "")
        if not name:
            return None, "ComfyUI did not return a filename after upload."
        return name, None
    except Exception as exc:
        return None, f"Upload failed: {exc}"
    finally:
        if tmp_path and os.path.exists(tmp_path):
            try:
                os.remove(tmp_path)
            except OSError:
                pass

# ---------------------------------------------------------------------------
# Artifact detection categories — ordered by perceptual impact
# ---------------------------------------------------------------------------

CHECKLIST = [
    {
        "category": "Hands and fingers",
        "what_to_check": (
            "Count fingers on every visible hand (must be exactly 5). "
            "Check thumb placement, grip angles, and look for fused, "
            "merged, or extra digits. Verify natural joint bending."
        ),
        "severity_weight": 15,
    },
    {
        "category": "Text and lettering",
        "what_to_check": (
            "Inspect all visible text: signs, labels, logos, book spines, "
            "screens. Look for garbled, mirrored, or nonsense letters, "
            "mixed-case chaos, and symbols that degrade at small sizes."
        ),
        "severity_weight": 14,
    },
    {
        "category": "Facial anatomy",
        "what_to_check": (
            "Check eye symmetry and matching reflections, teeth shape "
            "and count, ear consistency, nose alignment, and hairline "
            "continuity. Verify both eyes look in the same direction."
        ),
        "severity_weight": 12,
    },
    {
        "category": "Body proportions",
        "what_to_check": (
            "Verify limb lengths are balanced, joint angles are natural, "
            "no missing or extra limbs, torso-to-leg ratio is realistic, "
            "and clothing fits the body shape without impossible folds."
        ),
        "severity_weight": 11,
    },
    {
        "category": "Lighting consistency",
        "what_to_check": (
            "Confirm shadow directions are consistent across all objects, "
            "reflection angles match light sources, specular highlights "
            "are coherent, and ambient/fill light is uniform."
        ),
        "severity_weight": 10,
    },
    {
        "category": "Edge coherence",
        "what_to_check": (
            "Look for blurry or smeared transitions between foreground "
            "and background, halos around subjects, floating disconnected "
            "elements, and objects that appear cut off or pasted."
        ),
        "severity_weight": 9,
    },
    {
        "category": "Texture quality",
        "what_to_check": (
            "Check for waxy or plastic-looking skin, inconsistent "
            "resolution between regions (some sharp, some blurry), "
            "fabric that looks painted rather than woven, and metallic "
            "surfaces that lack proper reflectivity."
        ),
        "severity_weight": 8,
    },
    {
        "category": "Background logic",
        "what_to_check": (
            "Scan for impossible architecture (stairs to nowhere, "
            "windows at wrong angles), repeated background people or "
            "patterns, floating objects, and perspective lines that "
            "do not converge correctly."
        ),
        "severity_weight": 8,
    },
    {
        "category": "Accessories and details",
        "what_to_check": (
            "Inspect jewelry (earring symmetry, necklace continuity), "
            "glasses (lens shape, temple arms), buttons, zippers, "
            "weapon grips, watch faces, and any small prop the subject "
            "is holding or wearing."
        ),
        "severity_weight": 7,
    },
    {
        "category": "Overall composition",
        "what_to_check": (
            "Evaluate subject placement, rule-of-thirds alignment, "
            "whether key elements are cropped awkwardly at edges, "
            "aspect ratio coherence, and whether the image matches "
            "the artistic intent of the original prompt."
        ),
        "severity_weight": 6,
    },
]

TOTAL_WEIGHT = sum(c["severity_weight"] for c in CHECKLIST)

SEVERITY_PENALTY = {"none": 0, "minor": 1, "major": 2, "critical": 3}

QUALITY_SUFFIXES = [
    "sharp focus",
    "high detail",
    "natural lighting",
    "anatomically correct hands with five fingers",
    "photorealistic skin texture",
    "coherent background",
    "no text artifacts",
    "no watermarks",
    "professional composition",
]

NEGATIVE_PROMPT = (
    "blurry, distorted, deformed, extra limbs, extra fingers, "
    "bad anatomy, bad proportions, disfigured, fused fingers, "
    "missing limbs, mutated, artifacts, noise, watermark, signature, "
    "garbled text, broken letters, duplicate, morbid, ugly, "
    "low quality, worst quality, jpeg artifacts, out of frame, "
    "poorly drawn hands, poorly drawn face, mutation, mutated, "
    "extra digit, fewer digits, cropped, normal quality"
)


def _compute_score(findings: list[dict]) -> float:
    raw_penalty = 0.0
    for f in findings:
        sev = f.get("severity", "none").lower()
        penalty = SEVERITY_PENALTY.get(sev, 0)
        cat_name = f.get("category", "")
        weight = next(
            (c["severity_weight"] for c in CHECKLIST if c["category"] == cat_name),
            5,
        )
        raw_penalty += penalty * weight

    max_penalty = 3 * TOTAL_WEIGHT
    if max_penalty == 0:
        return 100.0
    score = max(0.0, 100.0 * (1.0 - raw_penalty / max_penalty))
    return round(score, 1)


def _build_corrective_hint(findings: list[dict]) -> str:
    issues = []
    for f in sorted(
        findings,
        key=lambda x: SEVERITY_PENALTY.get(x.get("severity", "none"), 0),
        reverse=True,
    ):
        sev = f.get("severity", "none").lower()
        if sev == "none":
            continue
        desc = f.get("description", "").strip()
        if desc:
            issues.append(desc)

    if not issues:
        return "No specific corrections needed — maintain overall quality."
    return "; ".join(issues[:5])


# ---------------------------------------------------------------------------
# MCP tools
# ---------------------------------------------------------------------------


@mcp.tool()
def plan_refinement(
    prompt: str,
    iterations: int = 3,
    style_hint: str = "",
) -> str:
    """Plan an iterative image refinement session.

    Call this FIRST before generating. It returns an enhanced prompt with
    quality-boosting suffixes, a comprehensive negative prompt, a 10-category
    inspection checklist, and a step-by-step iteration plan.

    Required: prompt (the user's image description).
    Optional: iterations (minimum 2, default 3), style_hint (e.g. 'photorealistic', 'anime', 'oil painting').
    """
    iterations = max(2, int(iterations))
    if not prompt or not prompt.strip():
        return json.dumps({"error": "prompt is required and must not be empty."})

    suffixes = list(QUALITY_SUFFIXES)
    if style_hint.strip():
        suffixes.insert(0, style_hint.strip())

    enhanced = prompt.strip().rstrip(".") + ", " + ", ".join(suffixes)

    iteration_plan = []
    for i in range(1, iterations + 1):
        if i == 1:
            iteration_plan.append({
                "iteration": i,
                "action": "generate",
                "focus": (
                    "Generate the initial image using the enhanced prompt and "
                    "negative prompt. Aim for the best possible first result."
                ),
            })
        elif i == 2:
            iteration_plan.append({
                "iteration": i,
                "action": "edit",
                "focus": (
                    "Fix the most severe artifacts found in iteration 1. "
                    "Priority: hands/fingers, facial anatomy, text. Use a "
                    "corrective edit prompt that explicitly describes what "
                    "to fix while preserving the original composition."
                ),
            })
        elif i == iterations:
            iteration_plan.append({
                "iteration": i,
                "action": "edit",
                "focus": (
                    "Final polish pass. Target any remaining minor artifacts: "
                    "edge coherence, texture quality, lighting consistency, "
                    "and background logic. Aim for a clean, publication-ready image."
                ),
            })
        else:
            iteration_plan.append({
                "iteration": i,
                "action": "edit",
                "focus": (
                    f"Refinement pass {i}. Address the next tier of artifacts "
                    f"from the previous inspection. Focus on issues rated "
                    f"'major' or above that were not fully resolved."
                ),
            })

    return json.dumps({
        "status": "planned",
        "original_prompt": prompt.strip(),
        "enhanced_prompt": enhanced,
        "negative_prompt": NEGATIVE_PROMPT,
        "style_hint": style_hint.strip() or None,
        "total_iterations": iterations,
        "checklist": CHECKLIST,
        "iteration_plan": iteration_plan,
        "instructions": (
            "For each iteration:\n"
            "1. Generate (iter 1) or edit (iter 2+) the image.\n"
            "2. Visually inspect the result against EVERY category in the checklist.\n"
            "3. Call log_iteration with your findings (one entry per category).\n"
            "4. If the log says 'edit', use the corrective_prompt_hint as the basis "
            "for your next edit_image call, combined with the original prompt.\n"
            "5. After the final iteration, present the image with the quality report."
        ),
    }, ensure_ascii=False)


@mcp.tool()
def log_iteration(
    iteration: int,
    total: int,
    findings: list,
    image_url: str = "",
) -> str:
    """Log the inspection results for one iteration and get guidance for the next.

    Call this AFTER visually inspecting a generated or edited image.

    Required: iteration (1-indexed), total (total planned iterations),
    findings (list of dicts with keys: category, description, severity).

    severity must be one of: 'none', 'minor', 'major', 'critical'.
    Provide one finding per checklist category you inspected.

    Optional: image_url (the URL of the image inspected, for tracking).
    """
    iteration = max(1, int(iteration))
    total = max(2, int(total))

    if not findings or not isinstance(findings, list):
        return json.dumps({
            "error": (
                "findings is required: a list of dicts with keys "
                "'category', 'description', 'severity'."
            )
        })

    clean_findings = []
    for f in findings:
        if not isinstance(f, dict):
            continue
        clean_findings.append({
            "category": str(f.get("category", "Unknown")),
            "description": str(f.get("description", "")),
            "severity": str(f.get("severity", "none")).lower(),
        })

    score = _compute_score(clean_findings)
    passing = score >= 80.0
    is_final = iteration >= total
    corrective_hint = _build_corrective_hint(clean_findings)

    critical_count = sum(
        1 for f in clean_findings
        if f["severity"] == "critical"
    )
    major_count = sum(
        1 for f in clean_findings
        if f["severity"] == "major"
    )
    minor_count = sum(
        1 for f in clean_findings
        if f["severity"] == "minor"
    )

    if is_final:
        next_action = "accept"
        next_focus = "This is the final iteration. Present the result to the user."
    elif passing and iteration >= 2:
        next_action = "accept"
        next_focus = (
            "Quality score is above threshold and minimum iterations reached. "
            "You may accept this result or continue refining if you see room "
            "for improvement."
        )
    else:
        next_action = "edit"
        if critical_count > 0:
            next_focus = (
                "Critical artifacts detected — prioritize fixing these in "
                "the next edit pass. Focus the edit prompt on: "
                + corrective_hint
            )
        elif major_count > 0:
            next_focus = (
                "Major artifacts remain. Target them in the next edit: "
                + corrective_hint
            )
        else:
            next_focus = (
                "Only minor issues remain. Fine-tune with a gentle edit: "
                + corrective_hint
            )

    result = {
        "iteration": iteration,
        "total_iterations": total,
        "quality_score": score,
        "pass": passing,
        "critical_count": critical_count,
        "major_count": major_count,
        "minor_count": minor_count,
        "next_action": next_action,
        "next_focus": next_focus,
        "corrective_prompt_hint": corrective_hint,
        "findings_summary": clean_findings,
    }

    if image_url:
        result["image_url"] = image_url

    if is_final:
        severity_labels = {
            "critical": critical_count,
            "major": major_count,
            "minor": minor_count,
        }
        remaining = {k: v for k, v in severity_labels.items() if v > 0}
        result["final_report"] = {
            "final_score": score,
            "passed": passing,
            "iterations_completed": iteration,
            "remaining_issues": remaining if remaining else "none",
            "summary": (
                f"Completed {iteration} iteration(s). "
                f"Final quality score: {score}/100. "
                + (
                    "Image passed quality threshold."
                    if passing
                    else f"Image did not reach the 80/100 threshold. "
                    f"Remaining: {critical_count} critical, {major_count} major, "
                    f"{minor_count} minor issue(s)."
                )
            ),
        }

    return json.dumps(result, ensure_ascii=False)


# ---------------------------------------------------------------------------
# Action tools — generate, edit, inspect
# ---------------------------------------------------------------------------

@mcp.tool()
def generate_initial_image(
    prompt: str,
    negative_prompt: str = "",
    width: int = 0,
    height: int = 0,
    steps: int = 0,
) -> str:
    """Generate the initial image for the refinement loop.

    Call this AFTER plan_refinement to create the first image. Uses the
    enhanced prompt and negative prompt from the plan.

    Parameters
    ----------
    prompt           The enhanced positive prompt (from plan_refinement).
    negative_prompt  The negative prompt (from plan_refinement).
    width            Output width in pixels (0 = default 1024).
    height           Output height in pixels (0 = default 1024).
    steps            Sampling steps (0 = default).
    """
    err = _comfyui_ok()
    if err:
        return json.dumps({"error": err})

    wf, load_err = _load_workflow(GEN_WORKFLOW_FILE)
    if load_err:
        return json.dumps({"error": f"txt2img workflow: {load_err}"})

    out_w = width if width > 0 else DEFAULT_WIDTH
    out_h = height if height > 0 else DEFAULT_HEIGHT
    out_steps = steps if steps > 0 else DEFAULT_STEPS
    out_seed = int(time.time() * 1000) % (2 ** 32)

    _inject_prompts(wf, prompt, negative_prompt or "")
    wf = _substitute(wf, {
        "__POSITIVE_PROMPT__": prompt,
        "__NEGATIVE_PROMPT__": negative_prompt or "",
        "__WIDTH__": out_w,
        "__HEIGHT__": out_h,
        "__STEPS__": out_steps,
        "__SEED__": out_seed,
    })

    image_url, poll_err = _submit_and_poll(wf)
    if poll_err:
        return json.dumps({"error": poll_err})

    return json.dumps({
        "status": "generated",
        "image_url": image_url,
        "filename": image_url.split("filename=")[-1].split("&")[0] if image_url else "",
        "width": out_w,
        "height": out_h,
        "prompt": prompt,
        "next_step": (
            "Visually inspect the generated image against the checklist. "
            "Then call log_iteration with your findings."
        ),
    })


@mcp.tool()
def refine_image(
    prompt: str,
    image_url: str,
    negative_prompt: str = "",
) -> str:
    """Edit/refine an image for the next iteration of the refinement loop.

    Call this when log_iteration says next_action is "edit". Uses the
    corrective_prompt_hint + original prompt to fix detected artifacts.

    Parameters
    ----------
    prompt           Describe what to fix / the desired corrected result.
                     Combine the original prompt with the corrective hint.
    image_url        URL of the current image to refine (from the previous
                     generate_initial_image or refine_image result).
    negative_prompt  Negative prompt (reuse from plan_refinement).
    """
    err = _comfyui_ok()
    if err:
        return json.dumps({"error": err})

    wf, load_err = _load_workflow(EDIT_WORKFLOW_FILE)
    if load_err:
        return json.dumps({"error": f"img2img workflow: {load_err}"})

    comfyui_name, upload_err = _upload_image(image_url)
    if upload_err:
        return json.dumps({"error": upload_err})

    # Inject the uploaded image into the first LoadImage node
    for _nid, node in wf.items():
        if not isinstance(node, dict):
            continue
        if node.get("class_type") == "LoadImage":
            node.setdefault("inputs", {})["image"] = comfyui_name
            break

    out_w = DEFAULT_WIDTH
    out_h = DEFAULT_HEIGHT
    out_steps = DEFAULT_STEPS
    out_seed = int(time.time() * 1000) % (2 ** 32)

    _inject_prompts(wf, prompt, negative_prompt or "")
    wf = _substitute(wf, {
        "__POSITIVE_PROMPT__": prompt,
        "__NEGATIVE_PROMPT__": negative_prompt or "",
        "__WIDTH__": out_w,
        "__HEIGHT__": out_h,
        "__STEPS__": out_steps,
        "__SEED__": out_seed,
        "__DENOISE__": 0.6,
    })

    # Inject denoise on KSampler nodes if no placeholder was found
    for _nid, node in wf.items():
        if not isinstance(node, dict):
            continue
        ct = node.get("class_type", "")
        if ct in ("KSampler", "KSamplerAdvanced"):
            inputs = node.get("inputs", {})
            if "denoise" in inputs and isinstance(inputs["denoise"], (int, float)):
                inputs["denoise"] = 0.6

    image_url_out, poll_err = _submit_and_poll(wf)
    if poll_err:
        return json.dumps({"error": poll_err})

    return json.dumps({
        "status": "generated",
        "image_url": image_url_out,
        "filename": image_url_out.split("filename=")[-1].split("&")[0] if image_url_out else "",
        "width": out_w,
        "height": out_h,
        "prompt": prompt,
        "next_step": (
            "Visually inspect the refined image against the checklist. "
            "Then call log_iteration with your findings."
        ),
    })


@mcp.tool()
def inspect_image(iteration: int, total: int) -> str:
    """Get the inspection checklist for the current iteration.

    Call this BEFORE log_iteration to remind yourself what to check.
    Visually examine the image and rate each category, then pass your
    findings to log_iteration.

    Parameters
    ----------
    iteration   Current iteration number (1-indexed).
    total       Total planned iterations.
    """
    return json.dumps({
        "iteration": iteration,
        "total_iterations": total,
        "instruction": (
            "Visually examine the image and rate EACH category below. "
            "For each, provide: category (exact name), severity (none/minor/major/critical), "
            "and description (what you see). Then call log_iteration with your findings list."
        ),
        "checklist": CHECKLIST,
        "severity_scale": {
            "none": "No issues detected",
            "minor": "Small imperfection, barely noticeable",
            "major": "Clearly visible problem that detracts from quality",
            "critical": "Severe artifact that breaks realism / usability",
        },
        "example_finding": {
            "category": "Hands and fingers",
            "severity": "major",
            "description": "Left hand has 6 fingers, thumb is fused with index finger",
        },
    }, ensure_ascii=False)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    mcp.run(transport="stdio")
