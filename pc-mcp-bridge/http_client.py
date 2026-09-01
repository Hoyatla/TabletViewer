"""HTTP client to the Android app.

Default base: http://127.0.0.1:8765
Use `adb reverse tcp:8765 tcp:8765` to forward the tablet's port to localhost.
"""

import httpx

_BASE = "http://127.0.0.1:8765"


def set_base(url: str) -> None:
    global _BASE
    _BASE = url


def fetch_current_frame() -> bytes:
    with httpx.Client() as client:
        r = client.get(f"{_BASE}/frame/current", timeout=5.0)
        r.raise_for_status()
        return r.content


def fetch_info() -> dict:
    with httpx.Client() as client:
        r = client.get(f"{_BASE}/info", timeout=5.0)
        r.raise_for_status()
        return r.json()


def fetch_history(n: int) -> list[bytes]:
    with httpx.Client() as client:
        r = client.get(
            f"{_BASE}/frame/history",
            params={"n": n},
            timeout=10.0,
        )
        r.raise_for_status()
        return r.json()  # caller decodes base64 entries
