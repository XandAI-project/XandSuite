"""
Unit tests for the Iterative Image Refiner package.

Run with:  python -m pytest test_iterative_image_refiner.py -v
(from the tools/packages/official directory)
"""

import importlib.util
import json
import sys
import unittest
from pathlib import Path
from unittest import mock

# ---------------------------------------------------------------------------
# Import the package by path
# ---------------------------------------------------------------------------
_MOD_PATH = Path(__file__).resolve().parent / "iterative_image_refiner.py"
_spec = importlib.util.spec_from_file_location("iterative_image_refiner", _MOD_PATH)
iir = importlib.util.module_from_spec(_spec)
sys.modules["iterative_image_refiner"] = iir
_spec.loader.exec_module(iir)


class TestPlanRefinement(unittest.TestCase):
    def test_basic_plan(self):
        out = json.loads(iir.plan_refinement("a cat on a roof", iterations=3))
        self.assertEqual(out["status"], "planned")
        self.assertIn("a cat on a roof", out["enhanced_prompt"])
        self.assertEqual(out["total_iterations"], 3)
        self.assertEqual(len(out["checklist"]), 10)
        self.assertEqual(len(out["iteration_plan"]), 3)

    def test_min_iterations_enforced(self):
        out = json.loads(iir.plan_refinement("test", iterations=1))
        self.assertEqual(out["total_iterations"], 2)

    def test_empty_prompt_error(self):
        out = json.loads(iir.plan_refinement(""))
        self.assertIn("error", out)

    def test_style_hint_prepended(self):
        out = json.loads(iir.plan_refinement("sunset", style_hint="oil painting"))
        self.assertIn("oil painting", out["enhanced_prompt"])

    def test_negative_prompt_present(self):
        out = json.loads(iir.plan_refinement("a dog"))
        self.assertTrue(len(out["negative_prompt"]) > 20)


class TestLogIteration(unittest.TestCase):
    def _findings(self, severity="none"):
        return [
            {"category": c["category"], "severity": severity, "description": "ok"}
            for c in iir.CHECKLIST
        ]

    def test_all_none_scores_100(self):
        out = json.loads(iir.log_iteration(1, 3, self._findings("none")))
        self.assertEqual(out["quality_score"], 100.0)
        self.assertTrue(out["pass"])

    def test_all_critical_scores_low(self):
        out = json.loads(iir.log_iteration(1, 3, self._findings("critical")))
        self.assertEqual(out["quality_score"], 0.0)

    def test_edit_action_when_below_threshold(self):
        findings = self._findings("none")
        findings[0]["severity"] = "critical"
        findings[0]["description"] = "6 fingers on left hand"
        out = json.loads(iir.log_iteration(1, 3, findings))
        self.assertEqual(out["next_action"], "edit")

    def test_accept_on_final_iteration(self):
        out = json.loads(iir.log_iteration(3, 3, self._findings("minor")))
        self.assertEqual(out["next_action"], "accept")
        self.assertIn("final_report", out)

    def test_empty_findings_error(self):
        out = json.loads(iir.log_iteration(1, 3, []))
        self.assertIn("error", out)

    def test_corrective_hint_describes_issues(self):
        findings = self._findings("none")
        findings[0]["severity"] = "major"
        findings[0]["description"] = "Extra finger on right hand"
        out = json.loads(iir.log_iteration(1, 3, findings))
        self.assertIn("Extra finger", out["corrective_prompt_hint"])


class TestInspectImage(unittest.TestCase):
    def test_returns_checklist(self):
        out = json.loads(iir.inspect_image(1, 3))
        self.assertEqual(out["iteration"], 1)
        self.assertEqual(len(out["checklist"]), 10)
        self.assertIn("severity_scale", out)
        self.assertIn("example_finding", out)


class TestGenerateInitialImage(unittest.TestCase):
    def test_missing_comfyui_url_error(self):
        original = iir.COMFYUI_URL
        iir.COMFYUI_URL = ""
        try:
            out = json.loads(iir.generate_initial_image("test prompt"))
            self.assertIn("error", out)
            self.assertIn("ComfyUI URL not configured", out["error"])
        finally:
            iir.COMFYUI_URL = original

    def test_missing_workflow_error(self):
        original_url = iir.COMFYUI_URL
        original_wf = iir.GEN_WORKFLOW_FILE
        iir.COMFYUI_URL = "http://localhost:8188"
        iir.GEN_WORKFLOW_FILE = ""
        try:
            out = json.loads(iir.generate_initial_image("test prompt"))
            self.assertIn("error", out)
            self.assertIn("workflow", out["error"].lower())
        finally:
            iir.COMFYUI_URL = original_url
            iir.GEN_WORKFLOW_FILE = original_wf

    @mock.patch.object(iir, "_submit_and_poll",
                       return_value=("http://localhost:8188/view?filename=test.png&subfolder=&type=output", None))
    @mock.patch.object(iir, "_load_workflow",
                       return_value=({"1": {"class_type": "CLIPTextEncode", "inputs": {"text": "positive"}}}, None))
    def test_successful_generation(self, mock_wf, mock_poll):
        original_url = iir.COMFYUI_URL
        iir.COMFYUI_URL = "http://localhost:8188"
        try:
            out = json.loads(iir.generate_initial_image("a beautiful sunset"))
            self.assertEqual(out["status"], "generated")
            self.assertIn("image_url", out)
            self.assertIn("next_step", out)
        finally:
            iir.COMFYUI_URL = original_url


class TestRefineImage(unittest.TestCase):
    def test_missing_comfyui_url_error(self):
        original = iir.COMFYUI_URL
        iir.COMFYUI_URL = ""
        try:
            out = json.loads(iir.refine_image("fix hands", "http://x/img.png"))
            self.assertIn("error", out)
        finally:
            iir.COMFYUI_URL = original

    def test_missing_edit_workflow_error(self):
        original_url = iir.COMFYUI_URL
        original_wf = iir.EDIT_WORKFLOW_FILE
        iir.COMFYUI_URL = "http://localhost:8188"
        iir.EDIT_WORKFLOW_FILE = ""
        try:
            out = json.loads(iir.refine_image("fix hands", "http://x/img.png"))
            self.assertIn("error", out)
            self.assertIn("workflow", out["error"].lower())
        finally:
            iir.COMFYUI_URL = original_url
            iir.EDIT_WORKFLOW_FILE = original_wf

    @mock.patch.object(iir, "_submit_and_poll",
                       return_value=("http://localhost:8188/view?filename=edit.png&subfolder=&type=output", None))
    @mock.patch.object(iir, "_upload_image", return_value=("uploaded.png", None))
    @mock.patch.object(iir, "_load_workflow",
                       return_value=({
                           "1": {"class_type": "LoadImage", "inputs": {"image": "placeholder.png"}},
                           "2": {"class_type": "CLIPTextEncode", "inputs": {"text": "positive"}},
                       }, None))
    def test_successful_refinement(self, mock_wf, mock_upload, mock_poll):
        original_url = iir.COMFYUI_URL
        iir.COMFYUI_URL = "http://localhost:8188"
        try:
            out = json.loads(iir.refine_image(
                "fix the extra fingers on the left hand",
                "http://localhost:8188/view?filename=test.png",
            ))
            self.assertEqual(out["status"], "generated")
            self.assertIn("image_url", out)
            self.assertIn("next_step", out)
        finally:
            iir.COMFYUI_URL = original_url


class TestComfyUIHelpers(unittest.TestCase):
    def test_inject_prompts_positive(self):
        wf = {
            "1": {"class_type": "CLIPTextEncode",
                  "_meta": {"title": "Positive Prompt"},
                  "inputs": {"text": "old prompt"}},
            "2": {"class_type": "CLIPTextEncode",
                  "_meta": {"title": "Negative"},
                  "inputs": {"text": "old neg"}},
        }
        iir._inject_prompts(wf, "new positive", "new negative")
        self.assertEqual(wf["1"]["inputs"]["text"], "new positive")
        self.assertEqual(wf["2"]["inputs"]["text"], "new negative")

    def test_substitute_replaces_tokens(self):
        wf = {
            "1": {"class_type": "KSampler",
                  "inputs": {"seed": "__SEED__", "steps": "__STEPS__"}},
        }
        result = iir._substitute(wf, {"__SEED__": 42, "__STEPS__": 20})
        self.assertEqual(result["1"]["inputs"]["seed"], 42)
        self.assertEqual(result["1"]["inputs"]["steps"], 20)

    def test_classify_text_node_positive(self):
        node = {"_meta": {"title": "Positive Prompt"}, "inputs": {"text": ""}}
        self.assertEqual(iir._classify_text_node(node), "positive")

    def test_classify_text_node_negative(self):
        node = {"_meta": {"title": "Negative"}, "inputs": {"text": ""}}
        self.assertEqual(iir._classify_text_node(node), "negative")

    def test_comfyui_ok_no_url(self):
        original = iir.COMFYUI_URL
        iir.COMFYUI_URL = ""
        try:
            self.assertIsNotNone(iir._comfyui_ok())
        finally:
            iir.COMFYUI_URL = original


if __name__ == "__main__":
    unittest.main(verbosity=2)
