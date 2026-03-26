"""
XandSuite Built-in MCP Tool: Code Runner
Executes Python and shell snippets in a sandboxed subprocess with a timeout.
Network access is NOT blocked at the OS level — rely on the workspace sandbox
and user trust model. No imports of dangerous modules are allowed for Python.
"""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-code-runner", version="1.0.0")

_WORKSPACE = Path(os.environ.get("XANDSUITE_WORKSPACE", Path.home() / "xandsuite_workspace"))
_DEFAULT_TIMEOUT = 30  # seconds
_BLOCKED_IMPORTS = {"os", "subprocess", "shutil", "socket", "ctypes", "importlib"}


def _check_python_safety(code: str) -> str | None:
    """Return an error message if the code contains blocked imports, else None."""
    import ast
    try:
        tree = ast.parse(code)
    except SyntaxError as e:
        return f"Syntax error: {e}"
    for node in ast.walk(tree):
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            names = (
                [alias.name for alias in node.names]
                if isinstance(node, ast.Import)
                else ([node.module] if node.module else [])
            )
            for name in names:
                base = name.split(".")[0]
                if base in _BLOCKED_IMPORTS:
                    return f"Import of '{base}' is not allowed in the sandbox."
    return None


@mcp.tool()
def run_python(code: str, timeout: int = _DEFAULT_TIMEOUT) -> str:
    """
    Execute a Python code snippet and return stdout/stderr.
    Imports of os, subprocess, shutil, socket, ctypes, importlib are blocked.
    The working directory is set to the XandSuite workspace.

    Args:
        code: Python source code to run.
        timeout: Maximum execution time in seconds (default 30, max 120).
    """
    timeout = max(1, min(120, timeout))
    safety_error = _check_python_safety(code)
    if safety_error:
        return json.dumps({"error": safety_error, "stdout": "", "stderr": ""})

    _WORKSPACE.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".py", dir=str(_WORKSPACE), delete=False, encoding="utf-8"
    ) as f:
        f.write(code)
        tmp_path = f.name

    try:
        result = subprocess.run(
            [sys.executable, tmp_path],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=str(_WORKSPACE),
            env={**os.environ, "PYTHONPATH": ""},
        )
        return json.dumps({
            "stdout": result.stdout[:8000],
            "stderr": result.stderr[:2000],
            "exit_code": result.returncode,
        })
    except subprocess.TimeoutExpired:
        return json.dumps({"error": f"Timed out after {timeout}s", "stdout": "", "stderr": ""})
    except Exception as e:
        return json.dumps({"error": str(e), "stdout": "", "stderr": ""})
    finally:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass


@mcp.tool()
def run_shell(command: str, timeout: int = _DEFAULT_TIMEOUT) -> str:
    """
    Execute a shell command and return the output.
    The working directory is set to the XandSuite workspace.
    Use with caution — shell commands have fewer restrictions than run_python.

    Args:
        command: Shell command string to execute.
        timeout: Maximum execution time in seconds (default 30, max 120).
    """
    timeout = max(1, min(120, timeout))
    _WORKSPACE.mkdir(parents=True, exist_ok=True)
    try:
        result = subprocess.run(
            command,
            shell=True,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=str(_WORKSPACE),
        )
        return json.dumps({
            "command": command,
            "stdout": result.stdout[:8000],
            "stderr": result.stderr[:2000],
            "exit_code": result.returncode,
        })
    except subprocess.TimeoutExpired:
        return json.dumps({"error": f"Timed out after {timeout}s", "stdout": "", "stderr": ""})
    except Exception as e:
        return json.dumps({"error": str(e), "stdout": "", "stderr": ""})


if __name__ == "__main__":
    mcp.run(transport="stdio")
