"""MCP server exposing the ScreenViewerOnTablet tablet screen.

Tools:
    - screen.read()        -> current frame as PNG bytes
    - screen.info()        -> resolution, fps, format
    - screen.history(n)    -> last N frames (most recent last)
    - screen.save(path)    -> save current frame to disk on PC

Connects to the Android app via HTTP/WebSocket (port 8765 by default).
Use `adb reverse tcp:8765 tcp:8765` to forward the tablet's port to localhost.
"""

import argparse
import os

from mcp.server.fastmcp import FastMCP

import http_client


mcp = FastMCP("screen-viewer-bridge")


@mcp.tool()
def screen_read() -> bytes:
    """Return the current tablet screen frame as PNG bytes."""
    return http_client.fetch_current_frame()


@mcp.tool()
def screen_info() -> dict:
    """Return tablet stream metadata: resolution, fps, format."""
    return http_client.fetch_info()


@mcp.tool()
def screen_history(n: int = 5) -> list[bytes]:
    """Return the last N frames as PNG bytes (most recent last)."""
    return http_client.fetch_history(n)


@mcp.tool()
def screen_save(path: str) -> str:
    """Save the current frame to `path` on the PC. Returns the absolute path."""
    abspath = os.path.abspath(path)
    data = http_client.fetch_current_frame()
    with open(abspath, "wb") as f:
        f.write(data)
    return abspath


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--tablet",
        default="http://127.0.0.1:8765",
        help="Tablet HTTP base URL (use `adb reverse` to forward the port).",
    )
    args = parser.parse_args()
    http_client.set_base(args.tablet)
    mcp.run()
