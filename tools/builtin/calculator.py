"""
XandSuite Built-in MCP Tool: Calculator
Safe mathematical evaluation using Python's ast module — no eval().
"""
import ast
import json
import math
import operator
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("xandsuite-calculator", version="1.0.0")

# Allowed operators and functions
_OPERATORS = {
    ast.Add: operator.add,
    ast.Sub: operator.sub,
    ast.Mult: operator.mul,
    ast.Div: operator.truediv,
    ast.Pow: operator.pow,
    ast.Mod: operator.mod,
    ast.FloorDiv: operator.floordiv,
    ast.USub: operator.neg,
    ast.UAdd: operator.pos,
}

_SAFE_FUNCTIONS = {
    "abs": abs,
    "round": round,
    "min": min,
    "max": max,
    "sum": sum,
    "sqrt": math.sqrt,
    "ceil": math.ceil,
    "floor": math.floor,
    "log": math.log,
    "log2": math.log2,
    "log10": math.log10,
    "sin": math.sin,
    "cos": math.cos,
    "tan": math.tan,
    "pi": math.pi,
    "e": math.e,
    "inf": math.inf,
}

_UNIT_CONVERSIONS = {
    # Length
    ("km", "m"): 1000,
    ("m", "km"): 0.001,
    ("m", "cm"): 100,
    ("cm", "m"): 0.01,
    ("m", "mm"): 1000,
    ("mm", "m"): 0.001,
    ("mile", "km"): 1.60934,
    ("km", "mile"): 0.621371,
    ("ft", "m"): 0.3048,
    ("m", "ft"): 3.28084,
    ("inch", "cm"): 2.54,
    ("cm", "inch"): 0.393701,
    # Weight
    ("kg", "g"): 1000,
    ("g", "kg"): 0.001,
    ("kg", "lb"): 2.20462,
    ("lb", "kg"): 0.453592,
    ("oz", "g"): 28.3495,
    ("g", "oz"): 0.035274,
    # Temperature (handled specially)
    # Volume
    ("l", "ml"): 1000,
    ("ml", "l"): 0.001,
    ("gallon", "l"): 3.78541,
    ("l", "gallon"): 0.264172,
}


def _safe_eval(node):
    if isinstance(node, ast.Expression):
        return _safe_eval(node.body)
    if isinstance(node, ast.Constant):
        if isinstance(node.value, (int, float, complex)):
            return node.value
        raise ValueError(f"Unsupported constant type: {type(node.value)}")
    if isinstance(node, ast.BinOp):
        op = _OPERATORS.get(type(node.op))
        if op is None:
            raise ValueError(f"Unsupported operator: {type(node.op).__name__}")
        return op(_safe_eval(node.left), _safe_eval(node.right))
    if isinstance(node, ast.UnaryOp):
        op = _OPERATORS.get(type(node.op))
        if op is None:
            raise ValueError(f"Unsupported unary operator: {type(node.op).__name__}")
        return op(_safe_eval(node.operand))
    if isinstance(node, ast.Call):
        if not isinstance(node.func, ast.Name):
            raise ValueError("Only named functions are allowed")
        fn = _SAFE_FUNCTIONS.get(node.func.id)
        if fn is None:
            raise ValueError(f"Function '{node.func.id}' is not allowed")
        args = [_safe_eval(a) for a in node.args]
        return fn(*args)
    if isinstance(node, ast.Name):
        val = _SAFE_FUNCTIONS.get(node.id)
        if val is None:
            raise ValueError(f"Name '{node.id}' is not allowed")
        return val
    raise ValueError(f"Unsupported expression node: {type(node).__name__}")


@mcp.tool()
def calculate(expression: str) -> str:
    """
    Evaluate a mathematical expression safely.
    Supports +, -, *, /, **, %, //, and functions: abs, round, sqrt, sin, cos,
    tan, log, log2, log10, ceil, floor, min, max, sum, and constants pi, e.

    Examples: "2 + 3 * 4", "sqrt(144)", "sin(pi/2)", "log(100, 10)"

    Args:
        expression: A mathematical expression string.
    """
    try:
        tree = ast.parse(expression.strip(), mode="eval")
        result = _safe_eval(tree)
        # Format nicely
        if isinstance(result, float) and result == int(result):
            result = int(result)
        return json.dumps({"expression": expression, "result": result})
    except ZeroDivisionError:
        return json.dumps({"error": "Division by zero", "expression": expression})
    except Exception as e:
        return json.dumps({"error": str(e), "expression": expression})


@mcp.tool()
def convert_units(value: float, from_unit: str, to_unit: str) -> str:
    """
    Convert a value from one unit to another.
    Supported categories: length (m, km, cm, mm, mile, ft, inch),
    weight (kg, g, lb, oz), volume (l, ml, gallon),
    temperature (celsius, fahrenheit, kelvin).

    Args:
        value: The numeric value to convert.
        from_unit: Source unit (lowercase).
        to_unit: Target unit (lowercase).
    """
    fu = from_unit.lower().strip()
    tu = to_unit.lower().strip()

    # Temperature special cases
    if fu == "celsius" and tu == "fahrenheit":
        result = value * 9 / 5 + 32
    elif fu == "fahrenheit" and tu == "celsius":
        result = (value - 32) * 5 / 9
    elif fu == "celsius" and tu == "kelvin":
        result = value + 273.15
    elif fu == "kelvin" and tu == "celsius":
        result = value - 273.15
    elif fu == "fahrenheit" and tu == "kelvin":
        result = (value - 32) * 5 / 9 + 273.15
    elif fu == "kelvin" and tu == "fahrenheit":
        result = (value - 273.15) * 9 / 5 + 32
    elif fu == tu:
        result = value
    else:
        factor = _UNIT_CONVERSIONS.get((fu, tu))
        if factor is None:
            return json.dumps({
                "error": f"Unknown conversion: {from_unit} → {to_unit}",
                "supported": list(set(k[0] for k in _UNIT_CONVERSIONS))
            })
        result = value * factor

    return json.dumps({
        "value": value,
        "from_unit": from_unit,
        "to_unit": to_unit,
        "result": round(result, 8),
    })


if __name__ == "__main__":
    mcp.run(transport="stdio")
