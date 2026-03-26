"""
XandSuite Built-in MCP Tool: File Operations
Sandboxed to the XandSuite workspace directory.
"""
import json
import os
from pathlib import Path
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-file-ops", version="1.0.0")

# Workspace root is passed via env var set by XandSuite on subprocess spawn.
# Falls back to the current working directory for manual testing.
_WORKSPACE = Path(os.environ.get("XANDSUITE_WORKSPACE", Path.home() / "xandsuite_workspace"))


def _resolve(path: str) -> Path:
    """Resolve `path` inside the sandbox and reject path traversal attempts."""
    resolved = (_WORKSPACE / path).resolve()
    if not str(resolved).startswith(str(_WORKSPACE.resolve())):
        raise PermissionError(f"Access denied: '{path}' is outside the workspace.")
    return resolved


@mcp.tool()
def read_file(path: str) -> str:
    """
    Read the contents of a file inside the workspace directory.

    Args:
        path: Relative path to the file within the workspace.
    """
    try:
        target = _resolve(path)
        if not target.exists():
            return json.dumps({"error": f"File not found: {path}"})
        if not target.is_file():
            return json.dumps({"error": f"Not a file: {path}"})
        content = target.read_text(encoding="utf-8", errors="replace")
        return json.dumps({"path": path, "content": content, "size": len(content)})
    except PermissionError as e:
        return json.dumps({"error": str(e)})
    except Exception as e:
        return json.dumps({"error": str(e)})


@mcp.tool()
def write_file(path: str, content: str, overwrite: bool = True) -> str:
    """
    Write content to a file inside the workspace directory.
    Creates parent directories as needed.

    Args:
        path: Relative path to the file within the workspace.
        content: Text content to write.
        overwrite: Whether to overwrite an existing file (default True).
    """
    try:
        target = _resolve(path)
        if target.exists() and not overwrite:
            return json.dumps({"error": f"File already exists: {path}. Set overwrite=true to replace."})
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
        return json.dumps({"path": path, "bytes_written": len(content.encode("utf-8")), "success": True})
    except PermissionError as e:
        return json.dumps({"error": str(e)})
    except Exception as e:
        return json.dumps({"error": str(e)})


@mcp.tool()
def list_directory(path: str = ".") -> str:
    """
    List the contents of a directory inside the workspace.

    Args:
        path: Relative path to the directory (default: workspace root).
    """
    try:
        target = _resolve(path)
        if not target.exists():
            return json.dumps({"error": f"Directory not found: {path}"})
        if not target.is_dir():
            return json.dumps({"error": f"Not a directory: {path}"})
        entries = []
        for item in sorted(target.iterdir()):
            entries.append({
                "name": item.name,
                "type": "directory" if item.is_dir() else "file",
                "size": item.stat().st_size if item.is_file() else None,
            })
        return json.dumps({"path": path, "entries": entries, "count": len(entries)})
    except PermissionError as e:
        return json.dumps({"error": str(e)})
    except Exception as e:
        return json.dumps({"error": str(e)})


@mcp.tool()
def delete_file(path: str) -> str:
    """
    Delete a file inside the workspace directory.

    Args:
        path: Relative path to the file within the workspace.
    """
    try:
        target = _resolve(path)
        if not target.exists():
            return json.dumps({"error": f"File not found: {path}"})
        if not target.is_file():
            return json.dumps({"error": f"Not a file (use a directory-specific command): {path}"})
        target.unlink()
        return json.dumps({"path": path, "deleted": True})
    except PermissionError as e:
        return json.dumps({"error": str(e)})
    except Exception as e:
        return json.dumps({"error": str(e)})


if __name__ == "__main__":
    mcp.run(transport="stdio")
