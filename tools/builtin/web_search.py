"""
XandSuite Built-in MCP Tool: Web Search
Uses DuckDuckGo (no API key required).
"""
import json
import sys
from mcp.server.fastmcp import FastMCP
from duckduckgo_search import DDGS

mcp = FastMCP("xandsuite-web-search", version="1.0.0")


@mcp.tool()
def web_search(query: str, max_results: int = 5) -> str:
    """
    Search the web using DuckDuckGo and return a list of results.
    Each result includes a title, URL, and snippet.

    Args:
        query: The search query string.
        max_results: Maximum number of results to return (1-10).
    """
    max_results = max(1, min(10, max_results))
    try:
        with DDGS() as ddgs:
            results = list(ddgs.text(query, max_results=max_results))
        if not results:
            return json.dumps({"results": [], "message": "No results found."})
        formatted = [
            {
                "title": r.get("title", ""),
                "url": r.get("href", ""),
                "snippet": r.get("body", ""),
            }
            for r in results
        ]
        return json.dumps({"results": formatted}, ensure_ascii=False)
    except Exception as e:
        return json.dumps({"error": str(e), "results": []})


@mcp.tool()
def fetch_page(url: str) -> str:
    """
    Fetch the text content of a webpage.

    Args:
        url: The full URL to fetch.
    """
    import httpx

    try:
        headers = {"User-Agent": "Mozilla/5.0 XandSuite/1.0"}
        with httpx.Client(timeout=10, follow_redirects=True) as client:
            resp = client.get(url, headers=headers)
            resp.raise_for_status()
        # Strip HTML tags simply
        import re
        text = re.sub(r"<[^>]+>", " ", resp.text)
        text = re.sub(r"\s{2,}", " ", text).strip()
        # Limit to ~4000 chars to fit context
        return json.dumps({"url": url, "content": text[:4000]}, ensure_ascii=False)
    except Exception as e:
        return json.dumps({"error": str(e), "url": url})


if __name__ == "__main__":
    mcp.run(transport="stdio")
