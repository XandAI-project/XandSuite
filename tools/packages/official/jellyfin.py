"""
XandSuite Package: Jellyfin Media Server
Allows the LLM to search and browse a Jellyfin media library.

Args:
    --url       Jellyfin server base URL (e.g. http://192.168.0.13:8096)
    --api-key   Optional API key for authenticated requests
    --user-id   Optional Jellyfin user ID (auto-detected if not provided)
"""

import argparse
import json
import sys
from mcp.server.fastmcp import FastMCP

# ---------------------------------------------------------------------------
# CLI args (parsed before FastMCP starts to avoid conflicts)
# ---------------------------------------------------------------------------
parser = argparse.ArgumentParser(add_help=False)
parser.add_argument("--url", default="")
parser.add_argument("--api-key", default="")
parser.add_argument("--user-id", default="")
known, _ = parser.parse_known_args()

JELLYFIN_URL = known.url.rstrip("/")
API_KEY = known.api_key
USER_ID = known.user_id

# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------
try:
    import requests
    _REQUESTS_OK = True
except ImportError:
    _REQUESTS_OK = False


def _headers() -> dict:
    h = {"Accept": "application/json", "Content-Type": "application/json"}
    if API_KEY:
        h["X-Emby-Token"] = API_KEY
    return h


def _get(path: str, params: dict = None) -> dict:
    if not JELLYFIN_URL:
        return {"error": "Jellyfin URL not configured. Set --url when installing the package."}
    if not _REQUESTS_OK:
        return {"error": "requests library not installed. Run: pip install requests"}
    url = f"{JELLYFIN_URL}{path}"
    if API_KEY and params is not None:
        params = {**params, "api_key": API_KEY}
    elif API_KEY:
        params = {"api_key": API_KEY}
    try:
        resp = requests.get(url, headers=_headers(), params=params, timeout=10)
        resp.raise_for_status()
        return resp.json()
    except requests.exceptions.ConnectionError:
        return {"error": f"Cannot connect to Jellyfin at {JELLYFIN_URL}. Check the URL and network."}
    except requests.exceptions.HTTPError as e:
        return {"error": f"HTTP {e.response.status_code}: {e.response.text[:200]}"}
    except Exception as e:
        return {"error": str(e)}


def _resolve_user_id() -> str:
    """Return USER_ID if provided, else fetch the first public user."""
    if USER_ID:
        return USER_ID
    data = _get("/Users/Public")
    if isinstance(data, list) and data:
        return data[0].get("Id", "")
    # Try admin endpoint as fallback
    data = _get("/Users")
    if isinstance(data, list) and data:
        return data[0].get("Id", "")
    return ""


# ---------------------------------------------------------------------------
# FastMCP server
# ---------------------------------------------------------------------------
mcp = FastMCP("xandsuite-jellyfin")


@mcp.tool()
def search_media(
    query: str = "",
    genre: str = "",
    media_type: str = "Movie,Series",
    year: int = 0,
    limit: int = 10,
) -> str:
    """
    Search the Jellyfin media library.

    Args:
        query:      Title keyword to search for (leave blank to browse).
        genre:      Filter by genre name (e.g. "Action", "Comedy", "Drama").
        media_type: Comma-separated item types: Movie, Series, Episode, MusicAlbum, etc.
        year:       Filter by production year (0 = any).
        limit:      Maximum number of results (1-50).
    """
    limit = max(1, min(50, limit))
    params: dict = {
        "Recursive": "true",
        "IncludeItemTypes": media_type,
        "Limit": limit,
        "Fields": "Name,ProductionYear,Genres,Overview,OfficialRating,CommunityRating",
        "SortBy": "SortName",
        "SortOrder": "Ascending",
    }
    if query:
        params["SearchTerm"] = query
    if genre:
        params["Genres"] = genre
    if year:
        params["Years"] = year

    uid = _resolve_user_id()
    path = f"/Users/{uid}/Items" if uid else "/Items"
    data = _get(path, params)

    if "error" in data:
        return json.dumps(data)

    items = data.get("Items", [])
    results = [
        {
            "id": i.get("Id"),
            "name": i.get("Name"),
            "type": i.get("Type"),
            "year": i.get("ProductionYear"),
            "genres": i.get("Genres", []),
            "rating": i.get("CommunityRating"),
            "overview": (i.get("Overview") or "")[:300],
        }
        for i in items
    ]
    return json.dumps(
        {"total": data.get("TotalRecordCount", len(results)), "results": results},
        ensure_ascii=False,
    )


@mcp.tool()
def get_genres(media_type: str = "Movie,Series") -> str:
    """
    Get a list of available genres from the Jellyfin library.

    Args:
        media_type: Comma-separated item types to filter genres for.
    """
    params = {
        "IncludeItemTypes": media_type,
        "Recursive": "true",
    }
    data = _get("/Genres", params)
    if "error" in data:
        return json.dumps(data)
    genres = [i.get("Name") for i in data.get("Items", []) if i.get("Name")]
    return json.dumps({"genres": genres}, ensure_ascii=False)


@mcp.tool()
def get_recently_added(media_type: str = "Movie,Series", limit: int = 10) -> str:
    """
    Get recently added media from the Jellyfin library.

    Args:
        media_type: Comma-separated item types (Movie, Series, Episode, etc.).
        limit:      Maximum number of items to return (1-50).
    """
    limit = max(1, min(50, limit))
    uid = _resolve_user_id()

    if uid:
        params = {
            "IncludeItemTypes": media_type,
            "Limit": limit,
            "Fields": "Name,ProductionYear,Genres,Overview,DateCreated",
        }
        data = _get(f"/Users/{uid}/Items/Latest", params)
        items = data if isinstance(data, list) else data.get("Items", [])
    else:
        params = {
            "Recursive": "true",
            "IncludeItemTypes": media_type,
            "Limit": limit,
            "SortBy": "DateCreated",
            "SortOrder": "Descending",
            "Fields": "Name,ProductionYear,Genres,Overview,DateCreated",
        }
        data = _get("/Items", params)
        if "error" in data:
            return json.dumps(data)
        items = data.get("Items", [])

    results = [
        {
            "id": i.get("Id"),
            "name": i.get("Name"),
            "type": i.get("Type"),
            "year": i.get("ProductionYear"),
            "genres": i.get("Genres", []),
            "added": i.get("DateCreated", ""),
        }
        for i in items
    ]
    return json.dumps({"results": results}, ensure_ascii=False)


@mcp.tool()
def get_libraries() -> str:
    """
    List all media libraries (virtual folders) on the Jellyfin server.
    Returns library names and their content types (Movies, TV Shows, Music, etc.).
    """
    data = _get("/Library/VirtualFolders")
    if "error" in data:
        return json.dumps(data)
    if isinstance(data, list):
        libs = [
            {
                "name": lib.get("Name"),
                "type": lib.get("CollectionType", "mixed"),
                "locations": lib.get("Locations", []),
            }
            for lib in data
        ]
        return json.dumps({"libraries": libs}, ensure_ascii=False)
    return json.dumps({"error": "Unexpected response format", "raw": str(data)[:200]})


@mcp.tool()
def get_item_details(item_id: str) -> str:
    """
    Get detailed information about a specific media item by its Jellyfin ID.

    Args:
        item_id: The Jellyfin item ID (obtained from search_media results).
    """
    uid = _resolve_user_id()
    path = f"/Users/{uid}/Items/{item_id}" if uid else f"/Items/{item_id}"
    params = {
        "Fields": "Name,ProductionYear,Genres,Overview,OfficialRating,CommunityRating,"
                  "People,Studios,Tags,Taglines,RunTimeTicks,MediaStreams",
    }
    data = _get(path, params)
    if "error" in data:
        return json.dumps(data)

    runtime_ticks = data.get("RunTimeTicks", 0)
    runtime_minutes = round(runtime_ticks / 600_000_000) if runtime_ticks else None

    result = {
        "id": data.get("Id"),
        "name": data.get("Name"),
        "type": data.get("Type"),
        "year": data.get("ProductionYear"),
        "genres": data.get("Genres", []),
        "rating": data.get("CommunityRating"),
        "official_rating": data.get("OfficialRating"),
        "overview": data.get("Overview", ""),
        "tagline": (data.get("Taglines") or [None])[0],
        "runtime_minutes": runtime_minutes,
        "studios": [s.get("Name") for s in (data.get("Studios") or [])],
        "tags": data.get("Tags", []),
        "cast": [
            {"name": p.get("Name"), "role": p.get("Role")}
            for p in (data.get("People") or [])
            if p.get("Type") == "Actor"
        ][:10],
    }
    return json.dumps(result, ensure_ascii=False)


if __name__ == "__main__":
    mcp.run(transport="stdio")
