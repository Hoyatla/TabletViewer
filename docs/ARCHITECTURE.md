# Architecture

## High-level diagram

```
┌────────────────────────┐                  ┌────────────────────────────────┐
│   NexTOS (PC)          │                  │   Galaxy Tab S9 FE (rooted)    │
│                        │  USB bulk IN     │                                │
│  ┌──────────────────┐  │  ──────────────► │  ┌──────────────────────────┐  │
│  │ Framebuffer      │  │                  │  │ StreamReceiver           │  │
│  │   reader         │  │                  │  └────────────┬─────────────┘  │
│  └────────┬─────────┘  │                  │               │                │
│           ▼            │                  │   ┌───────────▼─────────────┐  │
│  ┌──────────────────┐  │                  │   │ Live display (10 fps)   │  │
│  │ Packet encoder   │  │                  │   │ Recorder (PNG seq)      │  │
│  │   (enc.rs)       │  │                  │   │ HTTP/WS server :8765    │  │
│  └────────┬─────────┘  │                  │   └───────────┬─────────────┘  │
│           ▼            │                  │               │                │
│  ┌──────────────────┐  │                  │               │ HTTP           │
│  │ xHCI driver      │  │                  │   ┌───────────▼─────────────┐  │
│  │   (usb/xhci.rs)  │  │                  │   │ MCP server (PC, Python) │  │
│  └──────────────────┘  │                  │   └───────────┬─────────────┘  │
│                        │                  │               │ MCP stdio      │
└────────────────────────┘                  │   ┌───────────▼─────────────┐  │
                                            │   │ LLM (Mavis / MiniMax)   │  │
                                            │   └─────────────────────────┘  │
                                            │                                │
                                            │  ┌─────────────────────────┐  │
                                            │  │ ScreenCapture           │  │
                                            │  │   mmap(/dev/graphics/   │  │
                                            │  │        fb0)             │  │
                                            │  └────────────┬────────────┘  │
                                            │               ▼                │
                                            │  /storage/.../captures/*.png  │
                                            └────────────────────────────────┘
```

## Components

### `pc-sender/` — Rust `no_std` crate

Integrated into the NexTOS kernel. Provides:

- `fb::read()` — read current framebuffer (GOP or kernel linear FB).
- `enc::encode(frame_id, info, payload)` — produce wire packet (header + CRC32).
- `usb::xhci::init()` — initialize xHCI controller.
- `usb::bulk::send(packet)` — send packet over bulk endpoint.

### `android-app/` — Kotlin (Android 8.0+)

Provides:

- `StreamReceiver` — opens USB device, reads bulk IN, dispatches frames.
- `LiveRenderer` — displays frames on a `SurfaceView` at the negotiated fps.
- `Recorder` — saves incoming frames as a PNG sequence under `/storage/.../recordings/`.
- `HttpServer` — exposes frames over `http://0.0.0.0:8765` (WebSocket).
- `ScreenCapture` — reads `/dev/graphics/fb0` (root), returns RGBA pixels.
- `ScreenCapture.save()` — writes PNG to `/storage/.../captures/`.

### `pc-mcp-bridge/` — Python MCP server

Runs on the **PC** (not the tablet). Proxies to the Android app over HTTP/WS.
Exposes MCP tools:

- `screen.read()` → current frame as PNG bytes.
- `screen.info()` → resolution, fps, format.
- `screen.history(n)` → last N frames.
- `screen.save(path)` → save current frame to PC disk.

## Data flow (Mode 1)

1. NexTOS writes pixels to its framebuffer (GOP UEFI or kernel LFB).
2. `pc-sender` reads the framebuffer.
3. `pc-sender` encodes the frame as a packet (header + pixels + CRC32).
4. `pc-sender` bulk-sends the packet over USB.
5. Tablet `StreamReceiver` reads the packet.
6. Packet decoded → RGB565 pixels.
7. Three parallel consumers:
   - `LiveRenderer` displays the frame at 10 fps.
   - `Recorder` (if enabled) writes a PNG to `/storage/.../recordings/`.
   - `HttpServer` pushes the frame to WebSocket clients (e.g. MCP bridge).

## Data flow (Mode 2)

1. User taps "Capture" in the app.
2. `ScreenCapture` mmaps `/dev/graphics/fb0` (requires root).
3. Pixels read directly from the kernel framebuffer (no `MediaProjection`).
4. `ScreenCapture.save(path)` writes a PNG.

## Storage layout (on tablet)

```
/storage/emulated/0/Android/data/com.nextos.screenviewer/files/
├── recordings/        # Mode 1: incoming stream
│   └── 2026-09-01_14-30-00/
│       ├── frame_000001.png
│       ├── frame_000002.png
│       └── ...
├── captures/          # Mode 2: own-screen capture
│   └── 2026-09-01_14-31-00.png
└── logs/
    └── screenviewer.log
```

## Network setup for MCP

The tablet runs the HTTP/WS server on `0.0.0.0:8765`. To reach it from the PC:

```powershell
# Forward tablet's port 8765 to localhost
adb reverse tcp:8765 tcp:8765

# MCP bridge can then use http://127.0.0.1:8765
```

## Constraints

- USB-C 2.0 on Tab S9 FE → 480 Mbps theoretical, ~40 MB/s real.
- 1080p RGB565 = 4.15 MB/frame → ~9.6 fps at the limit. Downscale to 720p recommended.
- Root required for Mode 2.
- No crypto on the screen stream (debug tool, not a control channel).
