# ScreenViewerOnTablet

Stream the framebuffer of a bare-metal hypervisor (NexTOS) onto an Android tablet over USB, with two modes:

- **Mode 1** — Tablet receives a video stream from the PC (USB bulk IN).
- **Mode 2** — Tablet captures its own screen (root + `/dev/graphics/fb0`).

The received stream can be displayed live, recorded as a PNG sequence on the tablet, or exposed via an HTTP/WebSocket bridge to an MCP server for LLM access.

## Repository layout

```
ScreenViewerOnTablet/
├── pc-sender/         Rust no_std crate (integrated into NexTOS) — xHCI driver + packet encoder
├── android-app/       Android app (Kotlin) — USB receiver + framebuffer capture + HTTP/WS server
├── pc-mcp-bridge/     Python MCP server — proxies to the Android app over HTTP
├── protocol/          Binary spec of USB packets
├── docs/              Architecture, roadmap, protocol, decisions
└── tools/             Mock sender/receiver for testing without hardware
```

## Prerequisites

- Tablet rooted (tested on Galaxy Tab S9 FE with Magisk)
- Android 8.0+ (API 26+)
- PC with an xHCI USB host controller
- Python 3.10+ for the MCP bridge
- Rust 1.75+ (no_std) for the sender crate

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Protocol](docs/PROTOCOL.md)
- [Roadmap](docs/ROADMAP.md)
- [Decisions log](docs/DECISIONS.md)

## License

TBD
