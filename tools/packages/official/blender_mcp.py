"""
XandSuite Package: Blender MCP

Connect to Blender 3D through the BlenderMCP addon (https://github.com/ahujasid/blender-mcp).
Create, modify, and delete 3D objects, apply materials, run Python code in
Blender, capture viewport screenshots, and pull assets from Poly Haven,
Sketchfab, Hyper3D Rodin, and Hunyuan3D.

This is a self-contained bridge: it speaks the BlenderMCP addon's JSON-over-TCP
protocol directly, with no dependency on the upstream `blender-mcp` pip package
(and therefore no telemetry / supabase requirement).

Prerequisites
-------------
1. Blender 3.0+ with the BlenderMCP `addon.py` installed and enabled.
2. In Blender: open the 3D View sidebar (press N), find the "BlenderMCP" tab,
   and click "Connect to Claude" to start the addon's socket server.

Args
----
    --host   Blender addon socket host (default: 127.0.0.1)
    --port   Blender addon socket port (default: 9876)

Note: the default host is 127.0.0.1 (not "localhost") to avoid the Windows
case where "localhost" resolves to IPv6 ::1 while the Blender addon listens on
IPv4 only, which would make the connection silently fail.
"""

import argparse
import base64
import json
import os
import socket
import tempfile
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlparse

from mcp.server.fastmcp import FastMCP

# ---------------------------------------------------------------------------
# CLI args — parsed before FastMCP initialises to avoid argument conflicts
# ---------------------------------------------------------------------------
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--host", default="127.0.0.1")
_parser.add_argument("--port", type=int, default=9876)
_known, _ = _parser.parse_known_args()

BLENDER_HOST: str = _known.host or "127.0.0.1"
BLENDER_PORT: int = _known.port or 9876

# Match the addon's own socket timeout so long operations (asset import,
# generation) are not cut short.
SOCKET_TIMEOUT: float = 180.0


# ---------------------------------------------------------------------------
# Socket bridge — a minimal reimplementation of the upstream BlenderConnection
# ---------------------------------------------------------------------------
class BlenderConnection:
    """Persistent JSON-over-TCP connection to the BlenderMCP addon."""

    def __init__(self, host: str = BLENDER_HOST, port: int = BLENDER_PORT):
        self.host = host
        self.port = port
        self.sock: Optional[socket.socket] = None

    def connect(self) -> bool:
        if self.sock:
            return True
        try:
            self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.sock.connect((self.host, self.port))
            return True
        except Exception:
            if self.sock:
                try:
                    self.sock.close()
                except OSError:
                    pass
            self.sock = None
            return False

    def disconnect(self) -> None:
        if self.sock:
            try:
                self.sock.close()
            finally:
                self.sock = None

    def _receive_full_response(self, buffer_size: int = 8192) -> bytes:
        """Read until a complete JSON object has been received."""
        chunks = []
        assert self.sock is not None
        self.sock.settimeout(SOCKET_TIMEOUT)
        try:
            while True:
                try:
                    chunk = self.sock.recv(buffer_size)
                    if not chunk:
                        if not chunks:
                            raise Exception("Connection closed before any data was received")
                        break
                    chunks.append(chunk)
                    # Attempt to parse what we have so far; if it parses, we are done.
                    try:
                        data = b"".join(chunks)
                        json.loads(data.decode("utf-8"))
                        return data
                    except json.JSONDecodeError:
                        continue
                except socket.timeout:
                    break
        except (ConnectionError, BrokenPipeError, ConnectionResetError) as exc:
            raise Exception(f"Socket connection error during receive: {exc}")

        if chunks:
            data = b"".join(chunks)
            try:
                json.loads(data.decode("utf-8"))
                return data
            except json.JSONDecodeError:
                raise Exception("Incomplete JSON response received from Blender")
        raise Exception("No data received from Blender")

    def send_command(self, command_type: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        """Send one command and return the addon's `result` payload."""
        if not self.sock and not self.connect():
            raise ConnectionError("Not connected to Blender")

        command = {"type": command_type, "params": params or {}}
        assert self.sock is not None
        try:
            self.sock.sendall(json.dumps(command).encode("utf-8"))
            self.sock.settimeout(SOCKET_TIMEOUT)
            response_data = self._receive_full_response()
            response = json.loads(response_data.decode("utf-8"))

            if response.get("status") == "error":
                raise Exception(response.get("message", "Unknown error from Blender"))
            return response.get("result", {})
        except socket.timeout:
            self.disconnect()
            raise Exception("Timeout waiting for Blender response - try simplifying your request")
        except (ConnectionError, BrokenPipeError, ConnectionResetError) as exc:
            self.disconnect()
            raise Exception(f"Connection to Blender lost: {exc}")
        except json.JSONDecodeError as exc:
            self.disconnect()
            raise Exception(f"Invalid response from Blender: {exc}")
        except Exception as exc:
            self.disconnect()
            raise Exception(f"Communication error with Blender: {exc}")


# Global persistent connection, lazily (re)created and validated on each use.
_connection: Optional[BlenderConnection] = None
_polyhaven_enabled: bool = False


def get_blender_connection() -> BlenderConnection:
    """Return a live connection, recreating it if the previous one died."""
    global _connection, _polyhaven_enabled

    if _connection is not None:
        try:
            result = _connection.send_command("get_polyhaven_status")
            _polyhaven_enabled = result.get("enabled", False)
            return _connection
        except Exception:
            try:
                _connection.disconnect()
            except Exception:
                pass
            _connection = None

    if _connection is None:
        _connection = BlenderConnection(host=BLENDER_HOST, port=BLENDER_PORT)
        if not _connection.connect():
            _connection = None
            raise Exception(
                "Could not connect to Blender. Make sure Blender is running with the "
                "BlenderMCP addon enabled and 'Connect to Claude' clicked in the "
                "BlenderMCP sidebar tab."
            )
    return _connection


def _err(message: str) -> str:
    return json.dumps({"error": message})


def _process_bbox(bbox: list[float] | None) -> list[int] | None:
    if bbox is None:
        return None
    if all(isinstance(i, int) for i in bbox):
        return bbox
    if any(i <= 0 for i in bbox):
        raise ValueError("Incorrect number range: bbox must be bigger than zero!")
    return [int(float(i) / max(bbox) * 100) for i in bbox] if bbox else None


# ---------------------------------------------------------------------------
# FastMCP server
# ---------------------------------------------------------------------------
mcp = FastMCP("xandsuite-blender-mcp")


# ── Core scene + object tools ──────────────────────────────────────────────

@mcp.tool()
def get_scene_info() -> str:
    """Get detailed information about the current Blender scene: objects,
    cameras, lights, and their basic properties. Call this FIRST to understand
    the scene before creating or modifying anything."""
    try:
        blender = get_blender_connection()
        result = blender.send_command("get_scene_info")
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error getting scene info: {exc}")


@mcp.tool()
def get_object_info(object_name: str) -> str:
    """Get detailed information about a specific object in the Blender scene,
    including its transform, dimensions, materials, and mesh data.

    Parameters
    ----------
    object_name   The exact name of the object to inspect.
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command("get_object_info", {"name": object_name})
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error getting object info: {exc}")


_BPY_CHEATSHEET = """
## bpy cheat sheet — use EXACTLY these operator names

### Primitives (all end with _add)
bpy.ops.mesh.primitive_cube_add(size=2, location=(0,0,0))
bpy.ops.mesh.primitive_uv_sphere_add(radius=1, location=(0,0,0))
bpy.ops.mesh.primitive_ico_sphere_add(radius=1, subdivisions=2, location=(0,0,0))
bpy.ops.mesh.primitive_cylinder_add(radius=1, depth=2, vertices=32, location=(0,0,0))
bpy.ops.mesh.primitive_cone_add(radius1=1, radius2=0, depth=2, vertices=32, location=(0,0,0))
bpy.ops.mesh.primitive_torus_add(major_radius=1, minor_radius=0.25, location=(0,0,0))
bpy.ops.mesh.primitive_plane_add(size=2, location=(0,0,0))
bpy.ops.mesh.primitive_circle_add(radius=1, vertices=32, location=(0,0,0))
bpy.ops.mesh.primitive_monkey_add(size=2, location=(0,0,0))

### Rename the active object
bpy.context.active_object.name = "MyObject"

### Select / set active
obj = bpy.data.objects["MyObject"]
bpy.context.view_layer.objects.active = obj
obj.select_set(True)

### Transform
obj.location = (x, y, z)
obj.rotation_euler = (rx, ry, rz)   # radians! use math.radians(deg) to convert
obj.scale = (sx, sy, sz)

### Simple diffuse material
mat = bpy.data.materials.new("Red")
mat.use_nodes = True
bsdf = mat.node_tree.nodes["Principled BSDF"]
bsdf.inputs["Base Color"].default_value = (1, 0, 0, 1)  # RGBA
obj.data.materials.append(mat)

### Delete object
bpy.data.objects.remove(bpy.data.objects["MyObject"], do_unlink=True)

### Clear scene (keep camera + lights)
import bpy
for obj in list(bpy.data.objects):
    if obj.type not in ('CAMERA', 'LIGHT'):
        bpy.data.objects.remove(obj, do_unlink=True)

### Join objects into one
bpy.ops.object.select_all(action='DESELECT')
for name in ["Part1", "Part2"]:
    bpy.data.objects[name].select_set(True)
bpy.context.view_layer.objects.active = bpy.data.objects["Part1"]
bpy.ops.object.join()

### Set origin to geometry center
bpy.ops.object.origin_set(type='ORIGIN_GEOMETRY', center='MEDIAN')

### Subdivision modifier (low-poly smoothing)
mod = obj.modifiers.new("Subsurf", 'SUBSURF')
mod.levels = 1

IMPORTANT: There is NO operator called `primitive_cone` or `primitive_cylinder` etc.
They ALL end with `_add`: `primitive_cone_add`, `primitive_cylinder_add`, etc.
""".strip()

_ERROR_HINTS: list[tuple[str, str]] = [
    ("has no attribute 'unit_scale'",
     "HINT: `unit_scale` is on `bpy.context.scene.unit_settings`, not on the Scene directly. "
     "Use: bpy.context.scene.unit_settings.scale_length"),
    ("could not be found",
     "HINT: operator name is wrong. All mesh primitives end with _add. "
     "Correct names: primitive_cube_add, primitive_cone_add, primitive_cylinder_add, "
     "primitive_uv_sphere_add, primitive_ico_sphere_add, primitive_plane_add, "
     "primitive_torus_add, primitive_circle_add, primitive_monkey_add"),
    ("has no attribute 'active_object'",
     "HINT: use bpy.context.active_object (not bpy.context.scene.active_object)"),
    ("not found in bpy.data.objects",
     "HINT: that object name doesn't exist. Call get_scene_info first to see "
     "the actual object names in the scene."),
    ("is not default context",
     "HINT: some operators need the correct context. Try: "
     "with bpy.context.temp_override(area=..., region=...): ..."),
    ("name 'bpy' is not defined",
     "HINT: you forgot to import bpy. Add 'import bpy' at the top of your code."),
    ("name 'math' is not defined",
     "HINT: you forgot to import math. Add 'import math' at the top of your code."),
    ("already has a 'Principled BSDF'",
     "HINT: the material already has a Principled BSDF node. Get the existing one "
     "with: mat.node_tree.nodes.get('Principled BSDF')"),
]


def _enrich_error(error_msg: str) -> str:
    """Append corrective hints when the error matches a known pattern."""
    lower = error_msg.lower()
    hints = [hint for pattern, hint in _ERROR_HINTS if pattern.lower() in lower]
    if hints:
        return error_msg + "\n\n" + "\n".join(hints)
    return error_msg


@mcp.tool()
def execute_blender_code(code: str) -> str:
    """Run a SHORT bpy snippet in Blender — ONE small step per call.

    RULES:
    1. Put actual Python code in `code`. Never empty.
    2. ONE step per call (create one object, set one material, move one thing).
    3. Call `get_scene_info` first. Never reference objects you didn't confirm exist.
    4. After each step, verify with `get_scene_info` or `get_object_info`.

    Parameters
    ----------
    code   A short bpy Python snippet. Must not be empty.
    """
    if not code or not code.strip():
        return _err(
            "The 'code' parameter is empty. You must provide the actual Python "
            "code to execute. Example: "
            "bpy.ops.mesh.primitive_cube_add(size=1, location=(0,0,0))"
        )
    try:
        blender = get_blender_connection()
        result = blender.send_command("execute_code", {"code": code})
        return json.dumps({
            "status": "executed",
            "result": result.get("result", ""),
        })
    except Exception as exc:
        return _err(_enrich_error(f"Error executing code: {exc}"))


execute_blender_code.__doc__ = (execute_blender_code.__doc__ or "") + "\n\n" + _BPY_CHEATSHEET


# ---------------------------------------------------------------------------
# High-level wrapper tools — the LLM fills in parameters, we build the bpy.
# ---------------------------------------------------------------------------

_PRIMITIVE_MAP = {
    "CUBE":        "bpy.ops.mesh.primitive_cube_add(size={size}, location={loc})",
    "SPHERE":      "bpy.ops.mesh.primitive_uv_sphere_add(radius={size}, segments=32, ring_count=16, location={loc})",
    "ICO_SPHERE":  "bpy.ops.mesh.primitive_ico_sphere_add(radius={size}, subdivisions=2, location={loc})",
    "CYLINDER":    "bpy.ops.mesh.primitive_cylinder_add(radius={radius}, depth={depth}, vertices=32, location={loc})",
    "CONE":        "bpy.ops.mesh.primitive_cone_add(radius1={radius}, radius2=0, depth={depth}, vertices=32, location={loc})",
    "PLANE":       "bpy.ops.mesh.primitive_plane_add(size={size}, location={loc})",
    "TORUS":       "bpy.ops.mesh.primitive_torus_add(major_radius={size}, minor_radius={minor}, location={loc})",
    "CIRCLE":      "bpy.ops.mesh.primitive_circle_add(radius={size}, vertices=32, location={loc})",
    "MONKEY":      "bpy.ops.mesh.primitive_monkey_add(size={size}, location={loc})",
}


def _run_code(code: str) -> str:
    """Send a bpy snippet to Blender and return JSON result."""
    try:
        blender = get_blender_connection()
        result = blender.send_command("execute_code", {"code": code})
        return json.dumps({"status": "ok", "result": result.get("result", "")})
    except Exception as exc:
        return _err(_enrich_error(str(exc)))


@mcp.tool()
def create_object(
    type: str,
    name: str = "",
    location: list[float] | None = None,
    scale: list[float] | None = None,
    size: float = 1.0,
    radius: float = 0.5,
    depth: float = 2.0,
    minor_radius: float = 0.25,
) -> str:
    """Create a 3D primitive in Blender.

    Parameters
    ----------
    type    Primitive type. One of: CUBE, SPHERE, ICO_SPHERE, CYLINDER,
            CONE, PLANE, TORUS, CIRCLE, MONKEY.
    name    Optional name for the object (e.g. "Fuselage").
    location  [x, y, z] position (default [0,0,0]).
    scale     [sx, sy, sz] scale (default [1,1,1]).
    size      Overall size for CUBE/SPHERE/PLANE/CIRCLE/MONKEY (default 1).
    radius    Radius for CYLINDER/CONE (default 0.5).
    depth     Height/depth for CYLINDER/CONE (default 2).
    minor_radius  Inner radius for TORUS (default 0.25).
    """
    ptype = type.upper().strip()
    if ptype not in _PRIMITIVE_MAP:
        return _err(f"Unknown type '{type}'. Must be one of: {', '.join(_PRIMITIVE_MAP)}")

    loc = tuple(location or [0, 0, 0])
    scl = tuple(scale or [1, 1, 1])
    template = _PRIMITIVE_MAP[ptype]
    op_line = template.format(
        size=size, loc=loc, radius=radius, depth=depth, minor=minor_radius,
    )

    lines = ["import bpy", op_line]
    if name:
        lines.append(f'bpy.context.active_object.name = "{name}"')
    if scl != (1, 1, 1):
        lines.append(f"bpy.context.active_object.scale = {scl}")
    lines.append(
        f'print("Created {ptype}" + (f" named {name}" if "{name}" else ""))'
        if not name
        else f'print("Created {ptype} named {name}")'
    )

    return _run_code("\n".join(lines))


@mcp.tool()
def delete_object(name: str) -> str:
    """Delete an object from the Blender scene by name.

    Parameters
    ----------
    name   Exact name of the object to delete (from get_scene_info).
    """
    code = (
        "import bpy\n"
        f'obj = bpy.data.objects.get("{name}")\n'
        "if obj is None:\n"
        f'    raise Exception("Object \\"{name}\\" not found")\n'
        "bpy.data.objects.remove(obj, do_unlink=True)\n"
        f'print("Deleted {name}")'
    )
    return _run_code(code)


@mcp.tool()
def move_object(name: str, location: list[float]) -> str:
    """Move an object to an absolute [x, y, z] position.

    Parameters
    ----------
    name      Exact object name.
    location  [x, y, z] target position.
    """
    loc = tuple(location)
    code = (
        "import bpy\n"
        f'obj = bpy.data.objects["{name}"]\n'
        f"obj.location = {loc}\n"
        f'print("Moved {name} to {loc}")'
    )
    return _run_code(code)


@mcp.tool()
def rotate_object(name: str, rotation: list[float], degrees: bool = True) -> str:
    """Rotate an object to an absolute orientation.

    Parameters
    ----------
    name      Exact object name.
    rotation  [rx, ry, rz] rotation values.
    degrees   If True (default), values are in degrees and will be converted
              to radians. Set to False if already in radians.
    """
    if degrees:
        code = (
            "import bpy, math\n"
            f'obj = bpy.data.objects["{name}"]\n'
            f"obj.rotation_euler = (math.radians({rotation[0]}), "
            f"math.radians({rotation[1]}), math.radians({rotation[2]}))\n"
            f'print("Rotated {name} to {rotation} degrees")'
        )
    else:
        rot = tuple(rotation)
        code = (
            "import bpy\n"
            f'obj = bpy.data.objects["{name}"]\n'
            f"obj.rotation_euler = {rot}\n"
            f'print("Rotated {name}")'
        )
    return _run_code(code)


@mcp.tool()
def scale_object(name: str, scale: list[float]) -> str:
    """Scale an object to an absolute [sx, sy, sz] scale.

    Parameters
    ----------
    name   Exact object name.
    scale  [sx, sy, sz] scale factors (1 = original size).
    """
    scl = tuple(scale)
    code = (
        "import bpy\n"
        f'obj = bpy.data.objects["{name}"]\n'
        f"obj.scale = {scl}\n"
        f'print("Scaled {name} to {scl}")'
    )
    return _run_code(code)


@mcp.tool()
def set_material(
    name: str,
    color: list[float],
    material_name: str = "",
    metallic: float = 0.0,
    roughness: float = 0.5,
) -> str:
    """Apply a solid-color material to an object.

    Parameters
    ----------
    name            Exact object name.
    color           [R, G, B] or [R, G, B, A] values from 0.0 to 1.0.
                    Examples: red=[1,0,0], green=[0,1,0], blue=[0,0,1],
                    white=[1,1,1], dark gray=[0.2,0.2,0.2].
    material_name   Name for the material (auto-generated if empty).
    metallic        Metallic value 0.0-1.0 (default 0 = non-metal).
    roughness       Roughness value 0.0-1.0 (default 0.5).
    """
    rgba = list(color) + [1.0] if len(color) == 3 else list(color)
    mat_name = material_name or f"Mat_{name}"
    code = (
        "import bpy\n"
        f'obj = bpy.data.objects["{name}"]\n'
        f'mat = bpy.data.materials.new("{mat_name}")\n'
        "mat.use_nodes = True\n"
        'bsdf = mat.node_tree.nodes["Principled BSDF"]\n'
        f"bsdf.inputs['Base Color'].default_value = {tuple(rgba)}\n"
        f"bsdf.inputs['Metallic'].default_value = {metallic}\n"
        f"bsdf.inputs['Roughness'].default_value = {roughness}\n"
        "if obj.data.materials:\n"
        "    obj.data.materials[0] = mat\n"
        "else:\n"
        "    obj.data.materials.append(mat)\n"
        f'print("Applied material {mat_name} to {name}")'
    )
    return _run_code(code)


@mcp.tool()
def rename_object(old_name: str, new_name: str) -> str:
    """Rename an object in the scene.

    Parameters
    ----------
    old_name  Current name of the object.
    new_name  Desired new name.
    """
    code = (
        "import bpy\n"
        f'bpy.data.objects["{old_name}"].name = "{new_name}"\n'
        f'print("Renamed {old_name} -> {new_name}")'
    )
    return _run_code(code)


@mcp.tool()
def duplicate_object(
    name: str,
    new_name: str = "",
    offset: list[float] | None = None,
) -> str:
    """Duplicate an object, optionally placing the copy at an offset.

    Parameters
    ----------
    name      Exact name of the object to duplicate.
    new_name  Name for the copy (auto-generated if empty).
    offset    [dx, dy, dz] positional offset from the original (default [0,0,0]).
    """
    off = tuple(offset or [0, 0, 0])
    nn = new_name or f"{name}_copy"
    code = (
        "import bpy\n"
        f'src = bpy.data.objects["{name}"]\n'
        "new_obj = src.copy()\n"
        "new_obj.data = src.data.copy()\n"
        f'new_obj.name = "{nn}"\n'
        f"new_obj.location = (src.location.x + {off[0]}, "
        f"src.location.y + {off[1]}, src.location.z + {off[2]})\n"
        "bpy.context.collection.objects.link(new_obj)\n"
        f'print("Duplicated {name} -> {nn}")'
    )
    return _run_code(code)


@mcp.tool()
def join_objects(names: list[str], result_name: str = "") -> str:
    """Join multiple objects into a single mesh.

    Parameters
    ----------
    names        List of object names to join (at least 2).
    result_name  Name for the joined result (defaults to first object's name).
    """
    if len(names) < 2:
        return _err("Need at least 2 object names to join.")
    rname = result_name or names[0]
    names_str = ", ".join(f'"{n}"' for n in names)
    code = (
        "import bpy\n"
        "bpy.ops.object.select_all(action='DESELECT')\n"
        f"for n in [{names_str}]:\n"
        "    bpy.data.objects[n].select_set(True)\n"
        f'bpy.context.view_layer.objects.active = bpy.data.objects["{names[0]}"]\n'
        "bpy.ops.object.join()\n"
        f'bpy.context.active_object.name = "{rname}"\n'
        f'print("Joined into {rname}")'
    )
    return _run_code(code)


@mcp.tool()
def clear_scene(keep_camera: bool = True, keep_lights: bool = True) -> str:
    """Remove all objects from the scene (optionally keeping camera and lights).

    Parameters
    ----------
    keep_camera   Keep camera objects (default True).
    keep_lights   Keep light objects (default True).
    """
    skip_types = []
    if keep_camera:
        skip_types.append("'CAMERA'")
    if keep_lights:
        skip_types.append("'LIGHT'")
    skip = ", ".join(skip_types)
    code = (
        "import bpy\n"
        "removed = 0\n"
        "for obj in list(bpy.data.objects):\n"
        f"    if obj.type not in ({skip}):\n"
        "        bpy.data.objects.remove(obj, do_unlink=True)\n"
        "        removed += 1\n"
        'print(f"Cleared {removed} objects")'
    )
    return _run_code(code)


@mcp.tool()
def add_modifier(
    name: str,
    modifier_type: str,
    levels: int = 1,
) -> str:
    """Add a modifier to an object.

    Parameters
    ----------
    name            Exact object name.
    modifier_type   One of: SUBSURF, MIRROR, ARRAY, SOLIDIFY, BEVEL, BOOLEAN,
                    DECIMATE, SMOOTH, EDGE_SPLIT.
    levels          Viewport subdivision/repetition level (default 1).
    """
    valid = {"SUBSURF", "MIRROR", "ARRAY", "SOLIDIFY", "BEVEL",
             "BOOLEAN", "DECIMATE", "SMOOTH", "EDGE_SPLIT"}
    mt = modifier_type.upper().strip()
    if mt not in valid:
        return _err(f"Unknown modifier '{modifier_type}'. Must be one of: {', '.join(sorted(valid))}")
    code = (
        "import bpy\n"
        f'obj = bpy.data.objects["{name}"]\n'
        f'mod = obj.modifiers.new("{mt}", \'{mt}\')\n'
    )
    if mt == "SUBSURF":
        code += f"mod.levels = {levels}\n"
    elif mt == "ARRAY":
        code += f"mod.count = {levels}\n"
    elif mt == "BEVEL":
        code += "mod.width = 0.02\nmod.segments = 2\n"
    elif mt == "SOLIDIFY":
        code += "mod.thickness = 0.1\n"
    code += f'print("Added {mt} modifier to {name}")'
    return _run_code(code)


@mcp.tool()
def set_origin(name: str, origin_type: str = "GEOMETRY") -> str:
    """Set the origin point of an object.

    Parameters
    ----------
    name          Exact object name.
    origin_type   One of: GEOMETRY (center of mesh), CURSOR (3D cursor),
                  BOUNDS (center of bounding box). Default: GEOMETRY.
    """
    type_map = {
        "GEOMETRY": "ORIGIN_GEOMETRY",
        "CURSOR": "ORIGIN_CURSOR",
        "BOUNDS": "ORIGIN_GEOMETRY",
    }
    ot = origin_type.upper().strip()
    blender_type = type_map.get(ot, "ORIGIN_GEOMETRY")
    code = (
        "import bpy\n"
        "bpy.ops.object.select_all(action='DESELECT')\n"
        f'bpy.data.objects["{name}"].select_set(True)\n'
        f'bpy.context.view_layer.objects.active = bpy.data.objects["{name}"]\n'
        f"bpy.ops.object.origin_set(type='{blender_type}', center='MEDIAN')\n"
        f'print("Set origin of {name} to {ot}")'
    )
    return _run_code(code)


# ---------------------------------------------------------------------------
# Complex / compound tools
# ---------------------------------------------------------------------------

@mcp.tool()
def create_compound_object(
    name: str,
    parts: list[dict],
    join: bool = True,
    color: list[float] | None = None,
    material_name: str = "",
    location: list[float] | None = None,
) -> str:
    """Create a complex object from multiple primitives in a SINGLE call.

    Each part is a dict with:
      type       Required. CUBE, SPHERE, CYLINDER, CONE, PLANE, TORUS, etc.
      name       Required. Unique part name.
      location   [x,y,z] position (default [0,0,0]).
      rotation   [rx,ry,rz] in degrees (default [0,0,0]).
      scale      [sx,sy,sz] scale (default [1,1,1]).
      size       float, overall size for CUBE/SPHERE/PLANE (default 1).
      radius     float, for CYLINDER/CONE (default 0.5).
      depth      float, for CYLINDER/CONE (default 2).
      color      [R,G,B] 0-1, per-part color (overrides top-level color).

    Parameters
    ----------
    name      Name for the final joined object.
    parts     List of part dicts (see above).
    join      Join all parts into one object (default True).
    color     [R,G,B] default color for all parts (individual parts can override).
    material_name   Name for the shared material (auto-generated if empty).
    location  [x,y,z] final position of the joined object (default [0,0,0]).

    Example
    -------
    create_compound_object(
        name="Airplane",
        parts=[
            {"type":"CYLINDER","name":"Fuselage","radius":0.3,"depth":4,"rotation":[0,90,0]},
            {"type":"PLANE","name":"Wings","scale":[3,0.5,1],"location":[0,0,0]},
            {"type":"CONE","name":"Nose","radius":0.3,"depth":0.8,"location":[2.4,0,0],"rotation":[0,90,0]},
            {"type":"CONE","name":"Tail_Fin","radius":0.2,"depth":0.6,"location":[-2,0,0.3],"rotation":[-90,0,0],"scale":[0.5,0.5,1]},
        ],
        join=True,
        color=[0.2, 0.4, 0.8],
    )
    """
    if not parts or len(parts) == 0:
        return _err("No parts provided. Supply at least one part dict.")

    lines = ["import bpy, math"]

    part_names = []
    for i, p in enumerate(parts):
        ptype = p.get("type", "").upper().strip()
        if ptype not in _PRIMITIVE_MAP:
            return _err(f"Part {i}: unknown type '{p.get('type')}'. Must be one of: {', '.join(_PRIMITIVE_MAP)}")

        pname = p.get("name", f"{name}_part{i}")
        part_names.append(pname)
        loc = tuple(p.get("location", [0, 0, 0]))
        rot = p.get("rotation", [0, 0, 0])
        scl = tuple(p.get("scale", [1, 1, 1]))
        size = p.get("size", 1.0)
        radius = p.get("radius", 0.5)
        depth = p.get("depth", 2.0)
        minor = p.get("minor_radius", 0.25)

        template = _PRIMITIVE_MAP[ptype]
        op_line = template.format(size=size, loc=loc, radius=radius, depth=depth, minor=minor)
        lines.append(f"\n# Part: {pname}")
        lines.append(op_line)
        lines.append(f'bpy.context.active_object.name = "{pname}"')
        lines.append(f'obj = bpy.data.objects["{pname}"]')

        if rot != [0, 0, 0]:
            lines.append(
                f"obj.rotation_euler = (math.radians({rot[0]}), "
                f"math.radians({rot[1]}), math.radians({rot[2]}))"
            )
        if scl != (1, 1, 1):
            lines.append(f"obj.scale = {scl}")

        part_color = p.get("color") or color
        if part_color:
            rgba = list(part_color) + [1.0] if len(part_color) == 3 else list(part_color)
            mat_n = f"Mat_{pname}"
            lines.append(f'mat = bpy.data.materials.new("{mat_n}")')
            lines.append("mat.use_nodes = True")
            lines.append(f'mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = {tuple(rgba)}')
            lines.append("if obj.data.materials:")
            lines.append("    obj.data.materials[0] = mat")
            lines.append("else:")
            lines.append("    obj.data.materials.append(mat)")

    if join and len(part_names) > 1:
        lines.append("\n# Join all parts")
        lines.append("bpy.ops.object.select_all(action='DESELECT')")
        for pn in part_names:
            lines.append(f'bpy.data.objects["{pn}"].select_set(True)')
        lines.append(f'bpy.context.view_layer.objects.active = bpy.data.objects["{part_names[0]}"]\n')
        lines.append("bpy.ops.object.join()")
        lines.append(f'bpy.context.active_object.name = "{name}"')
    elif len(part_names) == 1:
        lines.append(f'\nbpy.data.objects["{part_names[0]}"].name = "{name}"')

    final_loc = tuple(location or [0, 0, 0])
    if final_loc != (0, 0, 0):
        lines.append(f'bpy.data.objects["{name}"].location = {final_loc}')

    lines.append(f'print("Created compound object {name} with {len(part_names)} parts")')

    return _run_code("\n".join(lines))


@mcp.tool()
def create_text_3d(
    text: str,
    name: str = "",
    location: list[float] | None = None,
    rotation: list[float] | None = None,
    size: float = 1.0,
    extrude: float = 0.1,
    color: list[float] | None = None,
    font_path: str = "",
    align_x: str = "CENTER",
    align_y: str = "CENTER",
) -> str:
    """Create a 3D text object in the scene.

    Parameters
    ----------
    text       The text string to display.
    name       Object name (default "Text3D").
    location   [x, y, z] position (default [0,0,0]).
    rotation   [rx, ry, rz] in degrees (default [0,0,0]).
    size       Font size (default 1).
    extrude    Depth/thickness of the 3D text (default 0.1).
    color      [R, G, B] color 0-1 (optional).
    font_path  Absolute path to a .ttf/.otf font file (uses Blender default if empty).
    align_x    Horizontal alignment: LEFT, CENTER, RIGHT, JUSTIFY, FLUSH.
    align_y    Vertical alignment: TOP, TOP_BASELINE, CENTER, BOTTOM_BASELINE, BOTTOM.
    """
    obj_name = name or "Text3D"
    loc = tuple(location or [0, 0, 0])
    rot = rotation or [0, 0, 0]
    escaped_text = text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")

    lines = [
        "import bpy, math",
        f"bpy.ops.object.text_add(location={loc})",
        f'obj = bpy.context.active_object',
        f'obj.name = "{obj_name}"',
        f'obj.data.body = "{escaped_text}"',
        f"obj.data.size = {size}",
        f"obj.data.extrude = {extrude}",
        f'obj.data.align_x = "{align_x.upper()}"',
        f'obj.data.align_y = "{align_y.upper()}"',
    ]

    if font_path:
        fp = font_path.replace("\\", "/")
        lines.append(f'obj.data.font = bpy.data.fonts.load("{fp}")')

    if rot != [0, 0, 0]:
        lines.append(
            f"obj.rotation_euler = (math.radians({rot[0]}), "
            f"math.radians({rot[1]}), math.radians({rot[2]}))"
        )

    if color:
        rgba = list(color) + [1.0] if len(color) == 3 else list(color)
        lines.extend([
            f'mat = bpy.data.materials.new("Mat_{obj_name}")',
            "mat.use_nodes = True",
            f'mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = {tuple(rgba)}',
            "obj.data.materials.append(mat)",
        ])

    lines.append(f'print("Created 3D text: {obj_name}")')
    return _run_code("\n".join(lines))


@mcp.tool()
def create_array(
    source_name: str,
    pattern: str = "LINEAR",
    count: int = 5,
    offset: list[float] | None = None,
    radius: float = 2.0,
    merge: bool = False,
) -> str:
    """Create copies of an object in a pattern (linear, grid, or circular).

    Parameters
    ----------
    source_name  Name of the object to duplicate.
    pattern      LINEAR (line), GRID (2D grid), or CIRCULAR (ring).
    count        Number of copies for LINEAR/CIRCULAR, or per-axis for GRID.
    offset       [dx, dy, dz] spacing between copies (LINEAR/GRID).
                 For GRID, x/y offsets are used. Default [2,0,0].
    radius       Radius for CIRCULAR pattern (default 2).
    merge        Join all copies into one object (default False).
    """
    pat = pattern.upper().strip()
    if pat not in ("LINEAR", "GRID", "CIRCULAR"):
        return _err(f"Unknown pattern '{pattern}'. Use LINEAR, GRID, or CIRCULAR.")
    if count < 2:
        return _err("Count must be at least 2.")

    off = offset or [2, 0, 0]

    lines = ["import bpy, math", f'src = bpy.data.objects["{source_name}"]', "created = []"]

    if pat == "LINEAR":
        lines.append(f"for i in range(1, {count}):")
        lines.append("    new = src.copy()")
        lines.append("    new.data = src.data.copy()")
        lines.append(f'    new.name = f"{source_name}_{{i}}"')
        lines.append(f"    new.location = (src.location.x + {off[0]}*i, "
                      f"src.location.y + {off[1]}*i, src.location.z + {off[2]}*i)")
        lines.append("    bpy.context.collection.objects.link(new)")
        lines.append("    created.append(new.name)")

    elif pat == "GRID":
        lines.append(f"for row in range({count}):")
        lines.append(f"    for col in range({count}):")
        lines.append("        if row == 0 and col == 0:")
        lines.append("            continue")
        lines.append("        new = src.copy()")
        lines.append("        new.data = src.data.copy()")
        lines.append(f'        new.name = f"{source_name}_{{row}}_{{col}}"')
        lines.append(f"        new.location = (src.location.x + {off[0]}*col, "
                      f"src.location.y + {off[1]}*row, src.location.z)")
        lines.append("        bpy.context.collection.objects.link(new)")
        lines.append("        created.append(new.name)")

    elif pat == "CIRCULAR":
        lines.append(f"for i in range(1, {count}):")
        lines.append(f"    angle = 2 * math.pi * i / {count}")
        lines.append("    new = src.copy()")
        lines.append("    new.data = src.data.copy()")
        lines.append(f'    new.name = f"{source_name}_{{i}}"')
        lines.append(f"    new.location = (src.location.x + {radius}*math.cos(angle), "
                      f"src.location.y + {radius}*math.sin(angle), src.location.z)")
        lines.append("    bpy.context.collection.objects.link(new)")
        lines.append("    created.append(new.name)")

    if merge:
        lines.append("\nbpy.ops.object.select_all(action='DESELECT')")
        lines.append("src.select_set(True)")
        lines.append("for n in created:")
        lines.append("    bpy.data.objects[n].select_set(True)")
        lines.append("bpy.context.view_layer.objects.active = src")
        lines.append("bpy.ops.object.join()")

    lines.append(f'print(f"Created {{len(created)}} copies of {source_name} in {pat} pattern")')
    return _run_code("\n".join(lines))


@mcp.tool()
def get_viewport_screenshot(max_size: int = 800) -> str:
    """Capture a screenshot of the current Blender 3D viewport. Use this to
    visually inspect the scene. The image is saved and displayed in chat.

    Parameters
    ----------
    max_size   Maximum size in pixels for the largest dimension (default 800).
    """
    temp_path = os.path.join(
        tempfile.gettempdir(), f"blender_screenshot_{os.getpid()}.png"
    )
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "get_viewport_screenshot",
            {"max_size": max_size, "filepath": temp_path, "format": "png"},
        )
        if "error" in result:
            return _err(result["error"])
        if not os.path.exists(temp_path):
            return _err("Screenshot file was not created by Blender")

        with open(temp_path, "rb") as fh:
            image_bytes = fh.read()
        encoded = base64.b64encode(image_bytes).decode("ascii")

        return json.dumps({
            "status": "generated",
            "filename": "blender_viewport.png",
            "mime_type": "image/png",
            "image_b64": encoded,
        })
    except Exception as exc:
        return _err(f"Screenshot failed: {exc}")
    finally:
        try:
            if os.path.exists(temp_path):
                os.remove(temp_path)
        except OSError:
            pass


# ── Poly Haven tools ───────────────────────────────────────────────────────

@mcp.tool()
def get_polyhaven_status() -> str:
    """Check whether the Poly Haven integration is enabled in Blender. Poly
    Haven offers free HDRIs, textures, and 3D models."""
    try:
        blender = get_blender_connection()
        result = blender.send_command("get_polyhaven_status")
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error checking Poly Haven status: {exc}")


@mcp.tool()
def get_polyhaven_categories(asset_type: str = "hdris") -> str:
    """List the available categories for a Poly Haven asset type.

    Parameters
    ----------
    asset_type   One of: hdris, textures, models, all.
    """
    try:
        blender = get_blender_connection()
        if not _polyhaven_enabled:
            return _err(
                "Poly Haven integration is disabled. Enable it in the BlenderMCP "
                "sidebar in Blender, then try again."
            )
        result = blender.send_command(
            "get_polyhaven_categories", {"asset_type": asset_type}
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error getting Poly Haven categories: {exc}")


@mcp.tool()
def search_polyhaven_assets(
    asset_type: str = "all",
    categories: str = "",
) -> str:
    """Search Poly Haven for assets, optionally filtered by category.

    Parameters
    ----------
    asset_type   One of: hdris, textures, models, all.
    categories   Optional comma-separated list of categories to filter by.
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "search_polyhaven_assets",
            {"asset_type": asset_type, "categories": categories or None},
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error searching Poly Haven assets: {exc}")


@mcp.tool()
def download_polyhaven_asset(
    asset_id: str,
    asset_type: str,
    resolution: str = "1k",
    file_format: str = "",
) -> str:
    """Download a Poly Haven asset and import it into Blender.

    Parameters
    ----------
    asset_id      The ID of the asset to download.
    asset_type    The type of asset (hdris, textures, models).
    resolution    Resolution to download (e.g. 1k, 2k, 4k).
    file_format   Optional file format (hdr/exr for HDRIs; jpg/png for
                  textures; gltf/fbx for models).
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "download_polyhaven_asset",
            {
                "asset_id": asset_id,
                "asset_type": asset_type,
                "resolution": resolution,
                "file_format": file_format or None,
            },
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error downloading Poly Haven asset: {exc}")


@mcp.tool()
def set_texture(object_name: str, texture_id: str) -> str:
    """Apply a previously downloaded Poly Haven texture to an object.

    Parameters
    ----------
    object_name   The object to apply the texture to.
    texture_id    The ID of the Poly Haven texture (must be downloaded first).
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "set_texture", {"object_name": object_name, "texture_id": texture_id}
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error applying texture: {exc}")


# ── Sketchfab tools ────────────────────────────────────────────────────────

@mcp.tool()
def get_sketchfab_status() -> str:
    """Check whether the Sketchfab integration is enabled in Blender. Sketchfab
    has a wide variety of realistic downloadable 3D models."""
    try:
        blender = get_blender_connection()
        result = blender.send_command("get_sketchfab_status")
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error checking Sketchfab status: {exc}")


@mcp.tool()
def search_sketchfab_models(
    query: str,
    categories: str = "",
    count: int = 20,
    downloadable: bool = True,
) -> str:
    """Search Sketchfab for 3D models.

    Parameters
    ----------
    query         Text to search for.
    categories    Optional comma-separated list of categories.
    count         Maximum number of results (default 20).
    downloadable  Only include downloadable models (default True).
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "search_sketchfab_models",
            {
                "query": query,
                "categories": categories or None,
                "count": count,
                "downloadable": downloadable,
            },
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error searching Sketchfab models: {exc}")


@mcp.tool()
def download_sketchfab_model(uid: str, target_size: float) -> str:
    """Download a Sketchfab model by UID and import it, scaled so its largest
    dimension equals `target_size` (in Blender meters).

    Parameters
    ----------
    uid           The unique identifier of the Sketchfab model.
    target_size   REQUIRED target size in meters for the largest dimension.
                  Examples: chair 1.0, table 0.75, car 4.5, person 1.7,
                  small object (cup/phone) 0.1-0.3.
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "download_sketchfab_model",
            {"uid": uid, "normalize_size": True, "target_size": target_size},
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error downloading Sketchfab model: {exc}")


# ── Hyper3D Rodin tools ────────────────────────────────────────────────────

@mcp.tool()
def get_hyper3d_status() -> str:
    """Check whether the Hyper3D Rodin integration is enabled in Blender.
    Hyper3D generates single 3D models from text or images."""
    try:
        blender = get_blender_connection()
        result = blender.send_command("get_hyper3d_status")
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error checking Hyper3D status: {exc}")


@mcp.tool()
def generate_hyper3d_model_via_text(
    text_prompt: str,
    bbox_condition: list[float] | None = None,
) -> str:
    """Start a Hyper3D Rodin job to generate a 3D model from a text prompt.
    The model is generated asynchronously; poll with `poll_rodin_job_status`
    then import with `import_generated_asset`.

    Parameters
    ----------
    text_prompt     Short description of the desired model, in English.
    bbox_condition  Optional [length, width, height] ratio (list of 3 floats).
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "create_rodin_job",
            {
                "text_prompt": text_prompt,
                "images": None,
                "bbox_condition": _process_bbox(bbox_condition),
            },
        )
        if result.get("submit_time", False):
            return json.dumps({
                "task_uuid": result.get("uuid"),
                "subscription_key": result.get("jobs", {}).get("subscription_key"),
            })
        return json.dumps(result)
    except Exception as exc:
        return _err(f"Error generating Hyper3D task: {exc}")


@mcp.tool()
def generate_hyper3d_model_via_images(
    input_image_paths: list[str] | None = None,
    input_image_urls: list[str] | None = None,
    bbox_condition: list[float] | None = None,
) -> str:
    """Start a Hyper3D Rodin job to generate a 3D model from reference images.
    Provide EITHER `input_image_paths` (MAIN_SITE mode) OR `input_image_urls`
    (FAL_AI mode), not both.

    Parameters
    ----------
    input_image_paths  Absolute local image paths (MAIN_SITE mode).
    input_image_urls   Image URLs (FAL_AI mode).
    bbox_condition     Optional [length, width, height] ratio.
    """
    try:
        if input_image_paths is not None and input_image_urls is not None:
            return _err("Conflicting parameters: provide only one of paths or URLs.")
        if input_image_paths is None and input_image_urls is None:
            return _err("No image given.")

        if input_image_paths is not None:
            if not all(os.path.exists(p) for p in input_image_paths):
                return _err("Not all image paths are valid.")
            images = []
            for path in input_image_paths:
                with open(path, "rb") as fh:
                    images.append(
                        (Path(path).suffix, base64.b64encode(fh.read()).decode("ascii"))
                    )
        else:
            if not all(urlparse(u) for u in input_image_urls):
                return _err("Not all image URLs are valid.")
            images = list(input_image_urls)

        blender = get_blender_connection()
        result = blender.send_command(
            "create_rodin_job",
            {
                "text_prompt": None,
                "images": images,
                "bbox_condition": _process_bbox(bbox_condition),
            },
        )
        if result.get("submit_time", False):
            return json.dumps({
                "task_uuid": result.get("uuid"),
                "subscription_key": result.get("jobs", {}).get("subscription_key"),
            })
        return json.dumps(result)
    except Exception as exc:
        return _err(f"Error generating Hyper3D task: {exc}")


@mcp.tool()
def poll_rodin_job_status(
    subscription_key: str = "",
    request_id: str = "",
) -> str:
    """Poll a Hyper3D Rodin generation job. Provide `subscription_key`
    (MAIN_SITE mode) or `request_id` (FAL_AI mode). The task is finished when
    statuses are all "Done" (MAIN_SITE) or "COMPLETED" (FAL_AI)."""
    try:
        blender = get_blender_connection()
        if subscription_key:
            kwargs = {"subscription_key": subscription_key}
        elif request_id:
            kwargs = {"request_id": request_id}
        else:
            return _err("Provide either subscription_key or request_id.")
        result = blender.send_command("poll_rodin_job_status", kwargs)
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error polling Hyper3D task: {exc}")


@mcp.tool()
def import_generated_asset(
    name: str,
    task_uuid: str = "",
    request_id: str = "",
) -> str:
    """Import a completed Hyper3D Rodin asset into the Blender scene. Provide
    `task_uuid` (MAIN_SITE mode) or `request_id` (FAL_AI mode).

    Parameters
    ----------
    name        The name to give the imported object in the scene.
    task_uuid   MAIN_SITE mode task identifier.
    request_id  FAL_AI mode request identifier.
    """
    try:
        blender = get_blender_connection()
        kwargs: dict[str, Any] = {"name": name}
        if task_uuid:
            kwargs["task_uuid"] = task_uuid
        elif request_id:
            kwargs["request_id"] = request_id
        else:
            return _err("Provide either task_uuid or request_id.")
        result = blender.send_command("import_generated_asset", kwargs)
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error importing generated asset: {exc}")


# ── Hunyuan3D tools ────────────────────────────────────────────────────────

@mcp.tool()
def get_hunyuan3d_status() -> str:
    """Check whether the Hunyuan3D integration is enabled in Blender.
    Hunyuan3D generates single 3D models from text or an image."""
    try:
        blender = get_blender_connection()
        result = blender.send_command("get_hunyuan3d_status")
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error checking Hunyuan3D status: {exc}")


@mcp.tool()
def generate_hunyuan3d_model(
    text_prompt: str = "",
    input_image_url: str = "",
) -> str:
    """Start a Hunyuan3D job to generate a 3D model from text and/or an image.
    Poll with `poll_hunyuan_job_status` then import with
    `import_generated_asset_hunyuan`.

    Parameters
    ----------
    text_prompt      Optional short description (English or Chinese).
    input_image_url  Optional local or remote image URL.
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "create_hunyuan_job",
            {
                "text_prompt": text_prompt or None,
                "image": input_image_url or None,
            },
        )
        response = result.get("Response", {})
        if "JobId" in response:
            return json.dumps({"job_id": f"job_{response['JobId']}"})
        return json.dumps(result)
    except Exception as exc:
        return _err(f"Error generating Hunyuan3D task: {exc}")


@mcp.tool()
def poll_hunyuan_job_status(job_id: str) -> str:
    """Poll a Hunyuan3D generation job. The task is finished when status is
    "DONE" (returns the result ZIP path) and still running when status is
    "RUN".

    Parameters
    ----------
    job_id   The job identifier returned by `generate_hunyuan3d_model`.
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command("poll_hunyuan_job_status", {"job_id": job_id})
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error polling Hunyuan3D task: {exc}")


@mcp.tool()
def import_generated_asset_hunyuan(name: str, zip_file_url: str) -> str:
    """Import a completed Hunyuan3D asset into the Blender scene.

    Parameters
    ----------
    name          The name to give the imported object.
    zip_file_url  The result ZIP path/URL from `poll_hunyuan_job_status`.
    """
    try:
        blender = get_blender_connection()
        result = blender.send_command(
            "import_generated_asset_hunyuan",
            {"name": name, "zip_file_url": zip_file_url},
        )
        return json.dumps(result, indent=2)
    except Exception as exc:
        return _err(f"Error importing Hunyuan3D asset: {exc}")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    mcp.run(transport="stdio")
