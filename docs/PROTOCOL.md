# Protocol — ScreenViewerOnTablet

Bidirectional debug API between a `pc-agent` (Rust HTTP server) and an
Android client (Kotlin). The transport is HTTP/JSON over TCP; the only
choice is **how** the tablet reaches the agent — LAN or USB.

## 1. Transport

### 1.1 LAN (default)

- TCP, plaintext HTTP/1.1. Port `8765` by default (`PC_AGENT_PORT`).
- Bind: `0.0.0.0` by default (`PC_AGENT_BIND`).
- The agent also advertises itself on the LAN via mDNS so the tablet can
  auto-discover it without typing the IP.

### 1.2 USB accessory + bridge (optional)

- Tablet = **device** (USB accessory role), PC = **host**.
- A small PC-side bridge binary (`pc-bridge`, not yet written — see
  `ROADMAP.md`) opens the accessory via libusb and forwards each request
  it reads from bulk OUT to `http://127.0.0.1:8766` on the PC, then
  writes the response back to bulk IN.
- The Android side just speaks localhost HTTP to `127.0.0.1:8766` and
  reuses the `LanDebugApi` parsing — no separate USB-side protocol.
- Without the bridge running, only `ping()` works on USB (a raw bulk
  round-trip); the other endpoints throw `UnsupportedOperationException`.

## 2. Service advertisement (mDNS)

- Service type: `_pcagent._tcp.local.`
- Default instance: `pcagent` (fullname `pcagent._pcagent._tcp.local.`)
- TXT records:
  - `version` = pc-agent's `CARGO_PKG_VERSION` (e.g. `0.1.0`)
  - `platform` = `windows` / `linux` / `macos`
  - `api` = `v1`
- PC side: `mdns-sd 0.11` (`pc-agent/src/discovery.rs`).
- Android side: platform `NsdManager` (`NetworkDiscovery.kt`), 4 s
  default timeout.

## 3. Endpoints

All under `/v1`. Errors are JSON: `{"error": "..."}` with the matching
4xx/5xx status. The agent is single-threaded per-request; no caching
beyond what's in `AppState` (a `sysinfo::System` and a `Capturer`).

### 3.1 `GET /v1/ping`

- Auth: none.
- Response: `200` text/plain `"pong"`.

### 3.2 `GET /v1/system`

- Auth: none.
- Response: `200` JSON. Fields (all present, may be empty/zero):

| Field         | Type     | Notes                                              |
|---------------|----------|----------------------------------------------------|
| `hostname`    | string   | `sysinfo::System::host_name()`                     |
| `os`          | string   | `std::env::consts::OS` (`windows`/`linux`/`macos`) |
| `arch`        | string   | `std::env::consts::ARCH`                           |
| `kernel`      | string?  | `sysinfo::System::kernel_version()`                |
| `os_version`  | string?  | `sysinfo::System::os_version()`                    |
| `cpu_brand`   | string   | first CPU's `brand()`                              |
| `cpu_count`   | integer  | `cpus().len()`                                     |
| `mem_total_kb`| u64      | total RAM in KiB                                   |
| `mem_avail_kb`| u64      | available RAM in KiB                               |
| `swap_total_kb`| u64     | total swap in KiB                                  |
| `swap_free_kb`| u64      | free swap in KiB                                   |
| `uptime_s`    | u64      | `System::uptime()`                                 |
| `boot_time_s` | u64      | unix epoch seconds of last boot                    |
| `loadavg`     | float[3] | `[1min, 5min, 15min]`                              |

### 3.3 `GET /v1/screenshot`

- Auth: yes (when token is set on the agent).
- Response: `200` `image/png` body, headers:
  - `Content-Type: image/png`
  - `X-Screenshot-Width: <u32>`
  - `X-Screenshot-Height: <u32>`
- `503` `{"error":"no display available (headless?)"}` if no screen.
- `500` `{"error":"..."}` on capture or PNG-encode failure.
- The capture is offloaded to `tokio::task::spawn_blocking` because the
  underlying Windows GDI call can take ~50 ms.

### 3.4 `GET /v1/processes?top=N&sort=cpu|mem`

- Auth: yes.
- Query params:
  - `top` (optional, default `20`, max = matches) — how many entries to return.
  - `sort` (optional, `cpu` default, or `mem`).
- Response: `200` JSON array. Each item:

```json
{
  "pid": 1234,
  "parent_pid": 1,
  "name": "explorer",
  "cmd": "C:\\Windows\\explorer.exe",
  "exe": "C:\\Windows\\explorer.exe",
  "cwd": "C:\\Users\\User",
  "cpu_pct": 1.5,
  "mem_kb": 65536,
  "status": "Run"
}
```

### 3.5 `GET /v1/log?path=<file>&tail=<N>`

- Auth: yes.
- Query params:
  - `path` (URL-encoded absolute path) — required.
  - `tail` (default `100`) — keep last N lines.
- Response: `200` JSON:

```json
{
  "path": "C:\\Windows\\System32\\drivers\\etc\\hosts",
  "lines": ["...", "...", "..."],
  "truncated": true
}
```

- `403` `{"error":"path not in allowed roots"}` if `--roots` is set and
  the canonicalized path is outside any allowed root.
- `500` `{"error":"..."}` on I/O failure.

### 3.6 `GET /v1/file?path=<file>`

- Auth: yes.
- Response: `200` `application/octet-stream` raw bytes.
- Same `403`/`500` semantics as `/v1/log`.

## 4. Authentication

- Optional, opt-in via `--token` / `PC_AGENT_TOKEN`.
- When set, requests must include `Authorization: Bearer <token>` on
  the endpoints that call `check_auth` (currently: `screenshot`,
  `processes`, `log`, `file`).
- `/v1/ping` and `/v1/system` are public regardless (liveness +
  dashboard).
- If no token is set, the agent logs a `WARN` on startup and accepts
  any request.

## 5. Path restriction

- Optional, opt-in via `--roots <dir1>,<dir2>` /
  `PC_AGENT_ROOTS=<dir1>,<dir2>`.
- Applies to `/v1/file` and `/v1/log`. Empty = allow any path
  (LAN-only trusted setup).

## 6. Client side

- `LanDebugApi.kt` — OkHttp 4.12, base URL `http://<ip>:8765`, timeouts
  5 s connect / 30 s read / 10 s write, bearer token injected if
  provided. JSON via `org.json`.
- `UsbDebugApi.kt` — `UsbManager` + `UsbAccessory` with permission flow;
  the actual HTTP/JSON exchange is delegated to the PC bridge over
  localhost. Without the bridge only `ping()` runs (a raw bulk write +
  read).
- `DebugApi.kt` — the interface both impls implement; the UI talks to
  this.

## 7. Versioning

- API version is exposed in the mDNS TXT record (`api=v1`) and in the
  URL prefix (`/v1/...`).
- New endpoints can be added under `/v1/...` without breaking existing
  clients. Breaking changes require a `/v2/...` prefix and a bump of
  the `api` TXT record.
