"""
Integration tests for the Blender MCP package.

These tests spin up a REAL TCP server on localhost that speaks the BlenderMCP
addon's JSON protocol, then exercise the package's BlenderConnection and tools
end-to-end over an actual socket. No Blender required.

Run with:  python -m unittest test_blender_mcp_integration -v
(from the tools/packages/official directory)
"""

import importlib.util
import json
import socket
import sys
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

_MOD_PATH = Path(__file__).resolve().parent / "blender_mcp.py"
_spec = importlib.util.spec_from_file_location("blender_mcp", _MOD_PATH)
blender_mcp = importlib.util.module_from_spec(_spec)
sys.modules["blender_mcp"] = blender_mcp
_spec.loader.exec_module(blender_mcp)


class MockBlenderServer:
    """A localhost TCP server that mimics the BlenderMCP addon.

    `handler(command_type, params)` returns the dict placed under the response
    "result" key (or raise to emit an error response). `chunk_size` forces the
    response to be sent in small pieces to exercise chunked reassembly.
    """

    def __init__(self, handler, chunk_size=0, drop_first=False):
        self.handler = handler
        self.chunk_size = chunk_size
        self.drop_first = drop_first
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(5)
        self.host, self.port = self._sock.getsockname()
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._running = True
        self._connections_served = 0

    def start(self):
        self._thread.start()
        return self

    def _serve(self):
        while self._running:
            try:
                client, _ = self._sock.accept()
            except OSError:
                break
            self._connections_served += 1
            # Optionally drop the very first connection immediately to exercise
            # the package's auto-reconnect path.
            if self.drop_first and self._connections_served == 1:
                client.close()
                continue
            threading.Thread(target=self._handle, args=(client,), daemon=True).start()

    def _handle(self, client):
        client.settimeout(5.0)
        try:
            while self._running:
                # Read a complete JSON command.
                buf = b""
                while True:
                    try:
                        chunk = client.recv(4096)
                    except socket.timeout:
                        return
                    if not chunk:
                        return
                    buf += chunk
                    try:
                        command = json.loads(buf.decode("utf-8"))
                        break
                    except json.JSONDecodeError:
                        continue

                try:
                    result = self.handler(command.get("type"), command.get("params", {}))
                    response = json.dumps({"status": "success", "result": result})
                except Exception as exc:  # noqa: BLE001
                    response = json.dumps({"status": "error", "message": str(exc)})

                data = response.encode("utf-8")
                if self.chunk_size > 0:
                    for i in range(0, len(data), self.chunk_size):
                        client.sendall(data[i:i + self.chunk_size])
                        time.sleep(0.005)
                else:
                    client.sendall(data)
        finally:
            client.close()

    def stop(self):
        self._running = False
        try:
            self._sock.close()
        except OSError:
            pass


class IntegrationBase(unittest.TestCase):
    def _make_conn(self, server):
        return blender_mcp.BlenderConnection(host=server.host, port=server.port)


class TestRoundTrips(IntegrationBase):
    def test_basic_round_trip(self):
        server = MockBlenderServer(
            lambda cmd, params: {"echo_type": cmd, "echo_params": params}
        ).start()
        self.addCleanup(server.stop)

        conn = self._make_conn(server)
        result = conn.send_command("create_object", {"type": "CUBE", "name": "Box"})

        self.assertEqual(result["echo_type"], "create_object")
        self.assertEqual(result["echo_params"], {"type": "CUBE", "name": "Box"})
        conn.disconnect()

    def test_large_chunked_response(self):
        big_scene = {"objects": [f"Object_{i}" for i in range(500)]}
        server = MockBlenderServer(
            lambda cmd, params: big_scene, chunk_size=64
        ).start()
        self.addCleanup(server.stop)

        conn = self._make_conn(server)
        result = conn.send_command("get_scene_info")
        self.assertEqual(len(result["objects"]), 500)
        self.assertEqual(result["objects"][-1], "Object_499")
        conn.disconnect()

    def test_error_response_raises(self):
        def handler(cmd, params):
            raise RuntimeError("object not found")

        server = MockBlenderServer(handler).start()
        self.addCleanup(server.stop)

        conn = self._make_conn(server)
        with self.assertRaises(Exception) as ctx:
            conn.send_command("get_object_info", {"name": "Ghost"})
        self.assertIn("object not found", str(ctx.exception))
        conn.disconnect()

    def test_sequential_commands_reuse_connection(self):
        calls = []
        server = MockBlenderServer(
            lambda cmd, params: calls.append(cmd) or {"n": len(calls)}
        ).start()
        self.addCleanup(server.stop)

        conn = self._make_conn(server)
        conn.send_command("a")
        conn.send_command("b")
        conn.send_command("c")
        self.assertEqual(calls, ["a", "b", "c"])
        conn.disconnect()


class TestToolsOverSocket(IntegrationBase):
    """Exercise the public tools end-to-end against the mock server."""

    def _route_connection(self, server):
        """Point get_blender_connection() at our mock server."""
        # Reset module globals and build a fresh connection bound to the server.
        blender_mcp._connection = None

        real_conn = self._make_conn(server)

        def fake_get():
            return real_conn

        return mock.patch.object(blender_mcp, "get_blender_connection", side_effect=fake_get), real_conn

    def test_get_scene_info_tool(self):
        server = MockBlenderServer(
            lambda cmd, params: {"objects": ["Cube", "Camera", "Light"]}
        ).start()
        self.addCleanup(server.stop)

        patcher, conn = self._route_connection(server)
        with patcher:
            out = json.loads(blender_mcp.get_scene_info())
        self.assertEqual(out["objects"], ["Cube", "Camera", "Light"])
        conn.disconnect()

    def test_execute_code_tool_round_trip(self):
        captured = {}

        def handler(cmd, params):
            captured["cmd"] = cmd
            return {"result": f"ran {len(params['code'])} chars"}

        server = MockBlenderServer(handler).start()
        self.addCleanup(server.stop)

        patcher, conn = self._route_connection(server)
        with patcher:
            out = json.loads(blender_mcp.execute_blender_code("bpy.ops.mesh.primitive_cube_add()"))
        # The addon-facing command type must be `execute_code`.
        self.assertEqual(captured["cmd"], "execute_code")
        self.assertEqual(out["status"], "executed")
        self.assertIn("chars", out["result"])
        conn.disconnect()


class TestReconnect(IntegrationBase):
    def test_reconnect_after_dropped_connection(self):
        """get_blender_connection() should rebuild a dead connection."""
        # Server that answers get_polyhaven_status (used as the liveness ping).
        def handler(cmd, params):
            if cmd == "get_polyhaven_status":
                return {"enabled": False}
            return {"ok": True}

        server = MockBlenderServer(handler).start()
        self.addCleanup(server.stop)

        # Force the module to use our server host/port for a fresh connection.
        blender_mcp._connection = None
        with mock.patch.object(blender_mcp, "BLENDER_HOST", server.host), \
             mock.patch.object(blender_mcp, "BLENDER_PORT", server.port):
            conn1 = blender_mcp.get_blender_connection()
            # Kill the underlying socket to simulate Blender dropping the link.
            conn1.disconnect()
            self.assertIsNone(conn1.sock)
            # Next call should validate (ping fails) and rebuild a live conn.
            conn2 = blender_mcp.get_blender_connection()
            result = conn2.send_command("get_scene_info")
            self.assertTrue(result["ok"])
            conn2.disconnect()

        blender_mcp._connection = None

    def test_connection_refused_raises_helpful_error(self):
        # Bind+immediately close a socket to obtain a definitely-closed port.
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind(("127.0.0.1", 0))
        _, dead_port = s.getsockname()
        s.close()

        blender_mcp._connection = None
        with mock.patch.object(blender_mcp, "BLENDER_HOST", "127.0.0.1"), \
             mock.patch.object(blender_mcp, "BLENDER_PORT", dead_port):
            with self.assertRaises(Exception) as ctx:
                blender_mcp.get_blender_connection()
            self.assertIn("Could not connect to Blender", str(ctx.exception))
        blender_mcp._connection = None


if __name__ == "__main__":
    unittest.main(verbosity=2)
