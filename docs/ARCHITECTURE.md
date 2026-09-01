# Architecture

## High-level

The project is a **client/server debug API**, not a streamer.

- **`pc-agent`** is a long-running HTTP server on the PC. It exposes a
  small REST/JSON API and is also a screenshotter.
- **`android-app`** is a thin Kotlin client. It calls the API over LAN
  (HTTP) or USB (accessory + a small PC bridge), and renders the
  results.
- The two endpoints can talk either directly over TCP, or over USB bulk
  with a PC-side bridge in the middle.

```
                         (LAN: HTTP/JSON)
   ┌────────────────┐                                  ┌──────────────────────┐
   │  android-app   │ ───────── TCP 0.0.0.0:8765 ────► │      pc-agent        │
   │  (Kotlin UI)   │ ◄────────── responses ───────── │  (Rust HTTP server)  │
   └────────────────┘                                  │                      │
            │                                          │  /v1/ping            │
            │  mDNS browse _pcagent._tcp.local.        │  /v1/system          │
            │                                          │  /v1/screenshot      │
            │                                          │  /v1/processes       │
            │                                          │  /v1/log             │
            │                                          │  /v1/file            │
            │                                          └──────────────────────┘
            │
            │  (USB accessory + bridge, optional)
            │         bulk IN/OUT
            ▼
   ┌────────────────┐                                  ┌──────────────────────┐
   │  UsbDebugApi   │ ── HTTP/JSON on 127.0.0.1:8766 ─►│   pc-bridge (Rust,   │
   │  (UsbAccessory)│ ◄───────── responses ────────── │   libusb, ~100 LoC)  │
   └────────────────┘                                  └──────────┬───────────┘
                                                                 │ HTTP/JSON
                                                                 ▼
                                                      ┌──────────────────────┐
                                                      │      pc-agent        │
                                                      │   (127.0.0.1:8765)   │
                                                      └──────────────────────┘
```

The user picks the transport in the app (LAN spinner entry or USB
spinner entry). The rest of the app does not care — both impls
implement `DebugApi`.

## Components

### `pc-agent/` — Rust crate, lib + bin

Path: `pc-agent/`. Toolchain: `stable-x86_64-pc-windows-gnu` (no MSVC
build tools needed).

- `src/lib.rs` — `Cli` (clap, env-overridable: `PC_AGENT_PORT`,
  `PC_AGENT_BIND`, `PC_AGENT_TOKEN`, `PC_AGENT_ROOTS`), `AppState`
  (token, allowed roots, cached `sysinfo::System`, cached `Capturer`),
  `build_router(state)` (the public router — used by integration tests
  too), `run(cli)` (initializes tracing, builds state, registers mDNS,
  binds TCP, serves).
- `src/handlers.rs` — six `pub async fn` handlers, one per endpoint,
  plus `check_auth(&state, &headers)` and `path_allowed(&state, &path)`.
  `/v1/screenshot` offloads the capture to `spawn_blocking` and
  encodes to PNG with `image::write_to` over a `Cursor<Vec<u8>>`.
- `src/capture.rs` — `Capturer::new()` and `grab_primary() ->
  image::RgbaImage`. Uses the `screenshots 0.8` crate, which returns
  RGBA images directly.
- `src/proc.rs` — `ProcessInfo` + `collect(&System)`. Local module is
  named `proc` (NOT `sysinfo`) to avoid shadowing the external
  `sysinfo` crate in the same scope.
- `src/discovery.rs` — `register(port, instance) -> MdnsHandle` and
  `discover(timeout) -> Vec<(name, ip, port)>` over the `mdns-sd 0.11`
  crate. TXT records: `version`, `platform`, `api=v1`.
- `src/main.rs` — 5-line wrapper that parses `Cli` and calls
  `pc_agent::run(cli).await`.
- `tests/integration.rs` — 9 in-process tests using
  `tower::ServiceExt::oneshot` on `build_router` (no real socket):
  ping, system shape, processes, screenshot (200 on real display / 503
  on headless), file read, log tail, auth missing 401, auth valid
  accepted, roots forbidden.

### `android-app/` — Kotlin (Android 8.0+, `compileSdk = 35`)

- `DebugApi.kt` — interface with six `suspend` methods + `SortBy` enum.
- `LanDebugApi.kt` — OkHttp 4.12, `org.json` for parsing, timeouts
  5/30/10 s, bearer token via `Authorization: Bearer`.
- `UsbDebugApi.kt` — `UsbManager` + `UsbAccessory`, with permission
  request via `PendingIntent.getBroadcast`. Opens the accessory, gets
  a `ParcelFileDescriptor`. Without the PC bridge, only `ping()` is
  usable (raw HTTP over bulk for a smoke test); the other five methods
  throw `UnsupportedOperationException` with a message pointing to the
  bridge docs.
- `NetworkDiscovery.kt` — `NsdManager` browse for `_pcagent._tcp.`
  with a 4 s timeout. Returns `List<DiscoveredAgent>` with
  `name/host/port`. Resolved IPs come from `NsdServiceInfo.host`.
- `MainActivity.kt` — `AppCompatActivity`. Top bar: `transportSpinner`
  (LAN / USB) + `Discover` button. Second row: URL `EditText` + Connect
  button. Status line, action button rows (System, Proc, Screen, Log,
  File), `ImageView` for the screenshot, scrolling text output. Connects
  to a `DebugApi` (LAN or USB) and dispatches button presses through
  it. `onDestroy()` closes the USB accessory.
- `res/layout/activity_main.xml` — the layout above.
- `app/build.gradle.kts` — `compileSdk = 35`, `minSdk = 26`,
  `targetSdk = 35`. `signingConfigs.release` reads
  `KEYSTORE_FILE`/`KEYSTORE_PASS`/`KEY_ALIAS`/`KEY_PASS` env vars
  (fallback: `${rootDir}/app/release.keystore`, `changeit`/`changeit`,
  `pcdebug`/`changeit`). `buildType.release` runs R8 (`isMinifyEnabled
  = true`) + resource shrinker (`isShrinkResources = true`).
- `app/src/main/AndroidManifest.xml` — `uses-feature
  android.hardware.usb.accessory required="false"`, `usesCleartextTraffic
  = "true"` (LAN HTTP), no required permissions beyond the legacy
  `READ_/WRITE_EXTERNAL_STORAGE` up to API 28/32.

## Data flow

### LAN (default)

1. PC: `pc-agent --port 8765 --token s3cret --roots C:\\logs`.
2. PC: agent binds `0.0.0.0:8765`, advertises `_pcagent._tcp.local.`,
   prints `mDNS: discoverable as pcagent._pcagent._tcp.local.`.
3. Tablet: user opens the app, taps `Discover`. `NetworkDiscovery`
   browses for 4 s, resolves the service, fills the URL field with
   `http://<ip>:8765`.
4. Tablet: user taps `Connect`. `LanDebugApi.ping()` and `.system()`
   run. Status line confirms the host.
5. Tablet: tapping any action button calls the matching `DebugApi`
   method, which issues an HTTP GET. Result renders in the
   `outputText` (or the `screenshot_view` `ImageView` for `Screen`).

### USB accessory + bridge

1. PC: `pc-agent` is running (the bridge is a thin shim that just
   relays requests to it).
2. PC: `pc-bridge` opens the Android accessory via libusb, claims the
   bulk endpoints, and starts a TCP listener on `127.0.0.1:8766`.
3. Tablet: user connects via USB, picks USB in the spinner, taps
   `Connect`. `UsbDebugApi.openIfNeeded()` requests permission, opens
   the accessory, gets the FD.
4. Tablet: `LanDebugApi` (re-used) is configured against
   `http://127.0.0.1:8766`; each HTTP call is wrapped in a bulk
   round-trip by the bridge.
5. Without the bridge, only `ping()` works (raw `GET /v1/ping HTTP/1.1`
   is written to bulk OUT, response read from bulk IN). The other five
   methods throw `UnsupportedOperationException` until the bridge
   exists.

## Storage layout

There is no on-tablet storage of API results. The app keeps the
in-memory `Bitmap` for the most recent screenshot, the text output,
and the URL field. All process lists, log tails, and file reads are
fetched on demand and displayed; nothing is persisted.

## Configuration

### Agent CLI

```
pc-agent [--port 8765] [--bind 0.0.0.0] [--token <s>] [--roots <dir1>,<dir2>]
```

Every flag also reads from an env var (`PC_AGENT_PORT`,
`PC_AGENT_BIND`, `PC_AGENT_TOKEN`, `PC_AGENT_ROOTS`). `RUST_LOG`
controls tracing verbosity.

### Tablet URL field

`http://<ip>:8765` — IP can be the LAN IP (auto-filled by Discover) or
`127.0.0.1` when the PC bridge is in use.

## Constraints

- LAN mode: agent is open to anything on the network unless `--token`
  is set. Use `--token` (or `--roots` for filesystem isolation) for any
  setup that isn't a trusted local network.
- USB mode on Tab S9 FE: USB-C 2.0, ~40 MB/s real throughput. Plenty
  for the JSON payload sizes here (a `system` response is ~400 B, a
  screenshot is whatever the screen produces).
- mDNS only works inside a single LAN / multicast domain. It does not
  cross routers. For WAN or restricted networks, the URL field accepts
  the IP directly.
- Screenshots require an active display. On a headless host the
  endpoint returns `503 {"error":"no display available (headless?)"}`.
