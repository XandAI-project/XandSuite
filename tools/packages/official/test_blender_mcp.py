"""
Unit tests for the Blender MCP package.

Run with:  python -m unittest test_blender_mcp -v
(from the tools/packages/official directory)

These tests mock the TCP socket and the Blender connection so they run fully
offline, without Blender or a real socket server.
"""

import importlib.util
import json
import os
import socket
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# ---------------------------------------------------------------------------
# Import the package module by path (it lives next to this test file).
# ---------------------------------------------------------------------------
_MOD_PATH = Path(__file__).resolve().parent / "blender_mcp.py"
_spec = importlib.util.spec_from_file_location("blender_mcp", _MOD_PATH)
blender_mcp = importlib.util.module_from_spec(_spec)
sys.modules["blender_mcp"] = blender_mcp
_spec.loader.exec_module(blender_mcp)


class FakeSocket:
    """A minimal stand-in for socket.socket that scripts recv() responses."""

    def __init__(self, recv_payloads=None, fail_connect=False):
        # recv_payloads: list of byte chunks recv() should yield in order.
        self._recv_payloads = list(recv_payloads or [])
        self._fail_connect = fail_connect
        self.sent = []
        self.closed = False
        self.timeout = None

    def connect(self, addr):
        if self._fail_connect:
            raise ConnectionRefusedError("refused")

    def settimeout(self, t):
        self.timeout = t

    def sendall(self, data):
        self.sent.append(data)

    def recv(self, bufsize):
        if self._recv_payloads:
            return self._recv_payloads.pop(0)
        return b""

    def close(self):
        self.closed = True


class TestBlenderConnection(unittest.TestCase):
    def test_command_serialization(self):
        """send_command must serialize {type, params} as JSON over the socket."""
        response = json.dumps({"status": "success", "result": {"ok": True}}).encode("utf-8")
        fake = FakeSocket(recv_payloads=[response])
        conn = blender_mcp.BlenderConnection("localhost", 9876)
        conn.sock = fake

        result = conn.send_command("create_object", {"type": "CUBE"})

        self.assertEqual(result, {"ok": True})
        self.assertEqual(len(fake.sent), 1)
        sent = json.loads(fake.sent[0].decode("utf-8"))
        self.assertEqual(sent["type"], "create_object")
        self.assertEqual(sent["params"], {"type": "CUBE"})

    def test_command_defaults_empty_params(self):
        response = json.dumps({"status": "success", "result": {}}).encode("utf-8")
        fake = FakeSocket(recv_payloads=[response])
        conn = blender_mcp.BlenderConnection()
        conn.sock = fake

        conn.send_command("get_scene_info")

        sent = json.loads(fake.sent[0].decode("utf-8"))
        self.assertEqual(sent["params"], {})

    def test_error_status_raises(self):
        response = json.dumps({"status": "error", "message": "boom"}).encode("utf-8")
        fake = FakeSocket(recv_payloads=[response])
        conn = blender_mcp.BlenderConnection()
        conn.sock = fake

        with self.assertRaises(Exception) as ctx:
            conn.send_command("bad_command")
        self.assertIn("boom", str(ctx.exception))

    def test_chunked_response_reassembly(self):
        """A response split across multiple recv() chunks must be reassembled."""
        full = json.dumps({"status": "success", "result": {"big": "x" * 50}}).encode("utf-8")
        mid = len(full) // 2
        fake = FakeSocket(recv_payloads=[full[:mid], full[mid:]])
        conn = blender_mcp.BlenderConnection()
        conn.sock = fake

        result = conn.send_command("get_scene_info")
        self.assertEqual(result["big"], "x" * 50)

    def test_connection_closed_invalidates_socket(self):
        """An empty recv() with no data should raise and drop the socket."""
        fake = FakeSocket(recv_payloads=[b""])
        conn = blender_mcp.BlenderConnection()
        conn.sock = fake

        with self.assertRaises(Exception):
            conn.send_command("get_scene_info")
        self.assertIsNone(conn.sock)

    def test_connect_failure_returns_false(self):
        conn = blender_mcp.BlenderConnection()
        with mock.patch.object(
            blender_mcp.socket, "socket", return_value=FakeSocket(fail_connect=True)
        ):
            self.assertFalse(conn.connect())
        self.assertIsNone(conn.sock)

    def test_disconnect_closes_socket(self):
        fake = FakeSocket()
        conn = blender_mcp.BlenderConnection()
        conn.sock = fake
        conn.disconnect()
        self.assertTrue(fake.closed)
        self.assertIsNone(conn.sock)

    def test_timeout_invalidates_socket(self):
        fake = FakeSocket()

        def _raise_timeout(_):
            raise socket.timeout()

        fake.sendall = _raise_timeout
        conn = blender_mcp.BlenderConnection()
        conn.sock = fake

        with self.assertRaises(Exception) as ctx:
            conn.send_command("get_scene_info")
        self.assertIn("Timeout", str(ctx.exception))
        self.assertIsNone(conn.sock)


class TestProcessBbox(unittest.TestCase):
    def test_none_passthrough(self):
        self.assertIsNone(blender_mcp._process_bbox(None))

    def test_int_list_passthrough(self):
        self.assertEqual(blender_mcp._process_bbox([1, 2, 3]), [1, 2, 3])

    def test_float_normalization(self):
        # Largest value maps to 100; others scale proportionally.
        self.assertEqual(blender_mcp._process_bbox([0.5, 1.0, 0.25]), [50, 100, 25])

    def test_non_positive_raises(self):
        with self.assertRaises(ValueError):
            blender_mcp._process_bbox([0.0, 1.0, 2.0])


class TestTools(unittest.TestCase):
    """Tools call get_blender_connection(); we patch it to a fake connection."""

    def _patch_conn(self, send_command):
        fake_conn = mock.Mock()
        fake_conn.send_command = send_command
        return mock.patch.object(blender_mcp, "get_blender_connection", return_value=fake_conn)

    def test_get_scene_info_formats_json(self):
        with self._patch_conn(lambda *a, **k: {"objects": ["Cube"]}):
            out = json.loads(blender_mcp.get_scene_info())
        self.assertEqual(out["objects"], ["Cube"])

    def test_execute_blender_code_sends_execute_code_command(self):
        # The addon command type MUST be `execute_code` (not `execute_blender_code`).
        captured = {}

        def send(cmd, params=None):
            captured["cmd"] = cmd
            captured["params"] = params
            return {"result": "done"}

        with self._patch_conn(send):
            out = json.loads(blender_mcp.execute_blender_code("print(1)"))

        self.assertEqual(captured["cmd"], "execute_code")
        self.assertEqual(captured["params"], {"code": "print(1)"})
        self.assertEqual(out["status"], "executed")
        self.assertEqual(out["result"], "done")

    def test_wrapper_tools_use_execute_code_not_raw_commands(self):
        # The addon does NOT support create_object / modify_object / etc. as
        # raw command types.  Our wrapper tools must send everything through
        # the `execute_code` command instead.
        captured = {}

        def send(cmd, params=None):
            captured["cmd"] = cmd
            return {"result": "ok"}

        with self._patch_conn(send):
            blender_mcp.create_object(type="CUBE", name="Test")
            self.assertEqual(captured["cmd"], "execute_code")

            blender_mcp.delete_object(name="Test")
            self.assertEqual(captured["cmd"], "execute_code")

            blender_mcp.set_material(name="Test", color=[1, 0, 0])
            self.assertEqual(captured["cmd"], "execute_code")

        # modify_object was never reimplemented — it doesn't exist.
        self.assertFalse(
            hasattr(blender_mcp, "modify_object"),
            "modify_object should not exist (use move/rotate/scale wrappers instead)",
        )

    def test_tool_error_returns_error_json(self):
        def send(*a, **k):
            raise Exception("connection lost")

        with self._patch_conn(send):
            out = json.loads(blender_mcp.get_object_info("Cube"))
        self.assertIn("error", out)
        self.assertIn("connection lost", out["error"])

    def test_polyhaven_categories_blocked_when_disabled(self):
        blender_mcp._polyhaven_enabled = False
        with self._patch_conn(lambda *a, **k: {"categories": {}}):
            out = json.loads(blender_mcp.get_polyhaven_categories("hdris"))
        self.assertIn("error", out)
        self.assertIn("disabled", out["error"].lower())

    def test_hyper3d_text_returns_keys_on_submit(self):
        def send(cmd, params=None):
            return {
                "submit_time": "2026-01-01",
                "uuid": "abc",
                "jobs": {"subscription_key": "key123"},
            }

        with self._patch_conn(send):
            out = json.loads(blender_mcp.generate_hyper3d_model_via_text("a chair"))
        self.assertEqual(out["task_uuid"], "abc")
        self.assertEqual(out["subscription_key"], "key123")

    def test_hyper3d_images_conflict_validation(self):
        out = json.loads(
            blender_mcp.generate_hyper3d_model_via_images(
                input_image_paths=["a.png"], input_image_urls=["http://x/y.png"]
            )
        )
        self.assertIn("error", out)

    def test_poll_rodin_requires_an_id(self):
        with self._patch_conn(lambda *a, **k: {}):
            out = json.loads(blender_mcp.poll_rodin_job_status())
        self.assertIn("error", out)

    def test_hunyuan_generate_formats_job_id(self):
        def send(cmd, params=None):
            return {"Response": {"JobId": "42"}}

        with self._patch_conn(send):
            out = json.loads(blender_mcp.generate_hunyuan3d_model("a vase"))
        self.assertEqual(out["job_id"], "job_42")


class TestErrorHints(unittest.TestCase):
    """Tests for the _enrich_error helper and the empty-code guard."""

    def test_enrich_adds_hint_for_unit_scale(self):
        msg = blender_mcp._enrich_error("Error: 'Scene' object has no attribute 'unit_scale'")
        self.assertIn("HINT", msg)
        self.assertIn("unit_settings", msg)

    def test_enrich_adds_hint_for_missing_operator(self):
        msg = blender_mcp._enrich_error('Calling operator "bpy.ops.mesh.primitive_cone" error, could not be found')
        self.assertIn("HINT", msg)
        self.assertIn("primitive_cone_add", msg)

    def test_enrich_adds_hint_for_missing_import_bpy(self):
        msg = blender_mcp._enrich_error("NameError: name 'bpy' is not defined")
        self.assertIn("HINT", msg)
        self.assertIn("import bpy", msg)

    def test_enrich_adds_hint_for_missing_import_math(self):
        msg = blender_mcp._enrich_error("NameError: name 'math' is not defined")
        self.assertIn("HINT", msg)
        self.assertIn("import math", msg)

    def test_enrich_no_hint_for_unknown_error(self):
        msg = blender_mcp._enrich_error("Something completely unexpected happened")
        self.assertNotIn("HINT", msg)

    def test_empty_code_returns_error(self):
        out = json.loads(blender_mcp.execute_blender_code(""))
        self.assertIn("error", out)
        self.assertIn("empty", out["error"].lower())

    def test_whitespace_only_code_returns_error(self):
        out = json.loads(blender_mcp.execute_blender_code("   \n\t  "))
        self.assertIn("error", out)
        self.assertIn("empty", out["error"].lower())

    def test_cheatsheet_in_docstring(self):
        doc = blender_mcp.execute_blender_code.__doc__ or ""
        self.assertIn("primitive_cube_add", doc)
        self.assertIn("primitive_cone_add", doc)
        self.assertIn("_add", doc)

    def test_error_hint_appended_to_tool_output(self):
        """When execute_blender_code gets an error matching a pattern, the hint
        should appear in the JSON error field returned to the LLM."""
        def send(cmd, params=None):
            raise Exception("Code execution error: 'Scene' object has no attribute 'unit_scale'")

        fake_conn = mock.Mock()
        fake_conn.send_command = send
        with mock.patch.object(blender_mcp, "get_blender_connection", return_value=fake_conn):
            out = json.loads(blender_mcp.execute_blender_code("bpy.context.scene.unit_scale"))
        self.assertIn("error", out)
        self.assertIn("HINT", out["error"])
        self.assertIn("unit_settings", out["error"])


class TestWrapperTools(unittest.TestCase):
    """Tests for the high-level wrapper tools that generate bpy code internally."""

    def _patch_conn(self):
        """Return a context manager that patches get_blender_connection and
        captures the code sent to execute_code."""
        captured = {}

        def send(cmd, params=None):
            captured["cmd"] = cmd
            captured["params"] = params or {}
            captured["code"] = (params or {}).get("code", "")
            return {"result": "ok"}

        fake_conn = mock.Mock()
        fake_conn.send_command = send
        patcher = mock.patch.object(
            blender_mcp, "get_blender_connection", return_value=fake_conn
        )
        return patcher, captured

    # ── create_object ──

    def test_create_cube(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_object(type="CUBE", name="MyCube", location=[1, 2, 3]))
        self.assertEqual(out["status"], "ok")
        self.assertEqual(cap["cmd"], "execute_code")
        self.assertIn("primitive_cube_add", cap["code"])
        self.assertIn('"MyCube"', cap["code"])
        self.assertIn("(1, 2, 3)", cap["code"])

    def test_create_cylinder(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_object(
                type="CYLINDER", name="Tube", radius=0.3, depth=5
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn("primitive_cylinder_add", cap["code"])
        self.assertIn("radius=0.3", cap["code"])
        self.assertIn("depth=5", cap["code"])

    def test_create_cone(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_object(type="CONE", name="Nose"))
        self.assertIn("primitive_cone_add", cap["code"])

    def test_create_unknown_type_error(self):
        out = json.loads(blender_mcp.create_object(type="BANANA"))
        self.assertIn("error", out)
        self.assertIn("Unknown type", out["error"])

    def test_create_with_scale(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_object(type="CUBE", name="Box", scale=[2, 1, 0.5])
        self.assertIn("(2, 1, 0.5)", cap["code"])

    # ── delete_object ──

    def test_delete_object(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.delete_object(name="OldCube"))
        self.assertEqual(out["status"], "ok")
        self.assertIn("remove", cap["code"])
        self.assertIn('"OldCube"', cap["code"])

    # ── move_object ──

    def test_move_object(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.move_object(name="Cube", location=[5, 0, 3]))
        self.assertEqual(out["status"], "ok")
        self.assertIn("(5, 0, 3)", cap["code"])
        self.assertIn(".location", cap["code"])

    # ── rotate_object ──

    def test_rotate_degrees(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.rotate_object(name="Wing", rotation=[90, 0, 45]))
        self.assertEqual(out["status"], "ok")
        self.assertIn("math.radians(90)", cap["code"])
        self.assertIn("math.radians(45)", cap["code"])

    def test_rotate_radians(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.rotate_object(
                name="Wing", rotation=[1.57, 0, 0], degrees=False
            ))
        self.assertNotIn("math.radians", cap["code"])
        self.assertIn("rotation_euler", cap["code"])

    # ── scale_object ──

    def test_scale_object(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.scale_object(name="Body", scale=[2, 2, 2]))
        self.assertIn("(2, 2, 2)", cap["code"])

    # ── set_material ──

    def test_set_material_rgb(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.set_material(
                name="Cube", color=[1, 0, 0], material_name="Red"
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn("materials.new", cap["code"])
        self.assertIn('"Red"', cap["code"])
        self.assertIn("(1, 0, 0, 1.0)", cap["code"])

    def test_set_material_rgba(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.set_material(name="Cube", color=[0.5, 0.5, 0.5, 0.8])
        self.assertIn("(0.5, 0.5, 0.5, 0.8)", cap["code"])

    def test_set_material_metallic_roughness(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.set_material(
                name="Cube", color=[1, 1, 1], metallic=1.0, roughness=0.1
            )
        self.assertIn("Metallic", cap["code"])
        self.assertIn("1.0", cap["code"])
        self.assertIn("0.1", cap["code"])

    # ── rename_object ──

    def test_rename_object(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.rename_object("Cube", "Fuselage"))
        self.assertEqual(out["status"], "ok")
        self.assertIn('"Cube"', cap["code"])
        self.assertIn('"Fuselage"', cap["code"])

    # ── duplicate_object ──

    def test_duplicate_object(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.duplicate_object(
                name="Wing", new_name="Wing_R", offset=[0, -4, 0]
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn('"Wing"', cap["code"])
        self.assertIn('"Wing_R"', cap["code"])
        self.assertIn("-4", cap["code"])

    def test_duplicate_default_name(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.duplicate_object(name="Part")
        self.assertIn('"Part_copy"', cap["code"])

    # ── join_objects ──

    def test_join_objects(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.join_objects(
                names=["Part1", "Part2", "Part3"], result_name="Airplane"
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn("join()", cap["code"])
        self.assertIn('"Airplane"', cap["code"])

    def test_join_too_few_error(self):
        out = json.loads(blender_mcp.join_objects(names=["OnlyOne"]))
        self.assertIn("error", out)

    # ── clear_scene ──

    def test_clear_scene(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.clear_scene())
        self.assertEqual(out["status"], "ok")
        self.assertIn("CAMERA", cap["code"])
        self.assertIn("LIGHT", cap["code"])
        self.assertIn("remove", cap["code"])

    def test_clear_scene_remove_all(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.clear_scene(keep_camera=False, keep_lights=False)
        self.assertNotIn("CAMERA", cap["code"])
        self.assertNotIn("LIGHT", cap["code"])

    # ── add_modifier ──

    def test_add_subsurf(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.add_modifier(name="Body", modifier_type="SUBSURF", levels=2))
        self.assertEqual(out["status"], "ok")
        self.assertIn("SUBSURF", cap["code"])
        self.assertIn("levels = 2", cap["code"])

    def test_add_invalid_modifier_error(self):
        out = json.loads(blender_mcp.add_modifier(name="X", modifier_type="NOPE"))
        self.assertIn("error", out)

    # ── set_origin ──

    def test_set_origin(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.set_origin(name="Mesh", origin_type="GEOMETRY"))
        self.assertEqual(out["status"], "ok")
        self.assertIn("ORIGIN_GEOMETRY", cap["code"])


class TestCompoundObject(unittest.TestCase):
    """Tests for create_compound_object, create_text_3d, and create_array."""

    def _patch_conn(self):
        captured = {}

        def send(cmd, params=None):
            captured["cmd"] = cmd
            captured["params"] = params or {}
            captured["code"] = (params or {}).get("code", "")
            return {"result": "ok"}

        fake_conn = mock.Mock()
        fake_conn.send_command = send
        patcher = mock.patch.object(
            blender_mcp, "get_blender_connection", return_value=fake_conn
        )
        return patcher, captured

    # ── create_compound_object ──

    def test_compound_creates_multiple_parts(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_compound_object(
                name="Ship",
                parts=[
                    {"type": "CUBE", "name": "Hull", "scale": [3, 1, 0.5]},
                    {"type": "CYLINDER", "name": "Mast", "radius": 0.1, "depth": 3, "location": [0, 0, 1.5]},
                ],
                join=True,
            ))
        self.assertEqual(out["status"], "ok")
        self.assertEqual(cap["cmd"], "execute_code")
        self.assertIn("primitive_cube_add", cap["code"])
        self.assertIn("primitive_cylinder_add", cap["code"])
        self.assertIn('"Hull"', cap["code"])
        self.assertIn('"Mast"', cap["code"])
        self.assertIn("join()", cap["code"])
        self.assertIn('"Ship"', cap["code"])

    def test_compound_with_color(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_compound_object(
                name="Box",
                parts=[{"type": "CUBE", "name": "C1"}],
                color=[1, 0, 0],
            )
        self.assertIn("materials.new", cap["code"])
        self.assertIn("(1, 0, 0, 1.0)", cap["code"])

    def test_compound_per_part_color_overrides(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_compound_object(
                name="Duo",
                parts=[
                    {"type": "CUBE", "name": "A", "color": [0, 1, 0]},
                    {"type": "CUBE", "name": "B"},
                ],
                color=[1, 0, 0],
            )
        self.assertIn("(0, 1, 0, 1.0)", cap["code"])
        self.assertIn("(1, 0, 0, 1.0)", cap["code"])

    def test_compound_with_rotation(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_compound_object(
                name="R",
                parts=[{"type": "CONE", "name": "Tip", "rotation": [90, 0, 0]}],
            )
        self.assertIn("math.radians(90)", cap["code"])

    def test_compound_no_join(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_compound_object(
                name="Loose",
                parts=[
                    {"type": "CUBE", "name": "A"},
                    {"type": "CUBE", "name": "B", "location": [3, 0, 0]},
                ],
                join=False,
            )
        self.assertNotIn("join()", cap["code"])

    def test_compound_with_final_location(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_compound_object(
                name="Placed",
                parts=[{"type": "CUBE", "name": "P"}],
                location=[5, 10, 0],
            )
        self.assertIn("(5, 10, 0)", cap["code"])

    def test_compound_no_parts_error(self):
        out = json.loads(blender_mcp.create_compound_object(name="Empty", parts=[]))
        self.assertIn("error", out)

    def test_compound_invalid_type_error(self):
        out = json.loads(blender_mcp.create_compound_object(
            name="Bad", parts=[{"type": "BANANA", "name": "X"}]
        ))
        self.assertIn("error", out)
        self.assertIn("unknown type", out["error"].lower())

    # ── create_text_3d ──

    def test_text_3d_basic(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_text_3d(text="Hello", name="MyText"))
        self.assertEqual(out["status"], "ok")
        self.assertIn("text_add", cap["code"])
        self.assertIn('"Hello"', cap["code"])
        self.assertIn('"MyText"', cap["code"])

    def test_text_3d_with_color(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_text_3d(text="Hi", color=[0, 0, 1])
        self.assertIn("materials.new", cap["code"])
        self.assertIn("(0, 0, 1, 1.0)", cap["code"])

    def test_text_3d_with_rotation(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_text_3d(text="Up", rotation=[90, 0, 0])
        self.assertIn("math.radians(90)", cap["code"])

    def test_text_3d_with_extrude(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_text_3d(text="3D", extrude=0.5)
        self.assertIn("extrude = 0.5", cap["code"])

    # ── create_array ──

    def test_array_linear(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_array(
                source_name="Pillar", pattern="LINEAR", count=5, offset=[3, 0, 0]
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn("range(1, 5)", cap["code"])
        self.assertIn("3*i", cap["code"])

    def test_array_circular(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_array(
                source_name="Post", pattern="CIRCULAR", count=8, radius=5.0
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn("math.cos", cap["code"])
        self.assertIn("math.sin", cap["code"])
        self.assertIn("5.0", cap["code"])

    def test_array_grid(self):
        patcher, cap = self._patch_conn()
        with patcher:
            out = json.loads(blender_mcp.create_array(
                source_name="Tile", pattern="GRID", count=3, offset=[2, 2, 0]
            ))
        self.assertEqual(out["status"], "ok")
        self.assertIn("for row", cap["code"])
        self.assertIn("for col", cap["code"])

    def test_array_merge(self):
        patcher, cap = self._patch_conn()
        with patcher:
            blender_mcp.create_array(
                source_name="Brick", pattern="LINEAR", count=3, merge=True
            )
        self.assertIn("join()", cap["code"])

    def test_array_invalid_pattern_error(self):
        out = json.loads(blender_mcp.create_array(source_name="X", pattern="SPIRAL"))
        self.assertIn("error", out)

    def test_array_count_too_low_error(self):
        out = json.loads(blender_mcp.create_array(source_name="X", pattern="LINEAR", count=1))
        self.assertIn("error", out)


class TestScreenshot(unittest.TestCase):
    def test_screenshot_returns_base64_and_cleans_temp(self):
        png_bytes = b"\x89PNG\r\n\x1a\n" + b"fakeimagedata"

        # The tool writes a temp path; emulate Blender creating that file.
        def send(cmd, params=None):
            with open(params["filepath"], "wb") as fh:
                fh.write(png_bytes)
            return {}

        fake_conn = mock.Mock()
        fake_conn.send_command = send
        with mock.patch.object(blender_mcp, "get_blender_connection", return_value=fake_conn):
            out = json.loads(blender_mcp.get_viewport_screenshot(max_size=400))

        self.assertEqual(out["status"], "generated")
        self.assertEqual(out["mime_type"], "image/png")
        import base64
        self.assertEqual(base64.b64decode(out["image_b64"]), png_bytes)

        # The temp file must have been cleaned up.
        temp_path = os.path.join(
            tempfile.gettempdir(), f"blender_screenshot_{os.getpid()}.png"
        )
        self.assertFalse(os.path.exists(temp_path))

    def test_screenshot_missing_file_returns_error(self):
        # Blender "succeeds" but never writes the file.
        with mock.patch.object(blender_mcp, "get_blender_connection", return_value=mock.Mock(
            send_command=lambda *a, **k: {}
        )):
            out = json.loads(blender_mcp.get_viewport_screenshot())
        self.assertIn("error", out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
