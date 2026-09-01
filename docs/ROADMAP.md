# Roadmap

Phases are ordered. Each phase has explicit acceptance criteria — the phase is
not "done" until the criterion is met.

## Phase 1 — Protocol PoC

**Goal:** validate the USB packet format end-to-end without real hardware.

- [x] `docs/PROTOCOL.md` — packet format spec
- [x] `pc-sender/src/enc.rs` — packet encoder (8 unit tests passing, incl. `reference_packet_matches_python`)
- [x] `tools/protocol_py.py` — Python reference implementation of the encoder/decoder
- [x] `tools/mock_sender.py` — emits fake frames to stdout/file
- [x] `tools/mock_receiver.py` — decodes frames to PNG
- [x] `tools/test_roundtrip.py` — 10 tests pass
- [x] `android-app/.../ProtocolParser.kt` — Kotlin port of `protocol_py.py`
- [x] `android-app/.../StreamReceiver.kt` — USB bulk IN reader with permission flow + resync
- [x] `android-app/.../MainActivity.kt` + `res/layout/activity_main.xml` — minimal UI to connect
- [x] **Acceptance:** a static frame travels from sender to receiver and renders correctly. ✅
  - Verified with `tools/mock_sender.py | tools/mock_receiver.py` (3 frames, 1280×720, 5.5 MB).
  - Reference packet bytes for input `(frame_id=0x12345678, w=258, h=772, fmt=RGB565, key=True, payload=b"hello")` are documented in `tools/test_roundtrip.py::test_reference_packet` and `pc-sender/src/enc.rs::tests::reference_packet_matches_python`.
  - **Rust ↔ Python ↔ Kotlin** all parse the same bytes (asserted by tests).

**Phase 1 — fully done.** Moving to Phase 2.

## Phase 2 — xHCI driver in NexTOS

**Goal:** bulk transfer works from the hypervisor.

- [ ] `pc-sender/src/usb/xhci.rs` — controller init (CAP, HCSPARAMS, USBSTS)
- [ ] `pc-sender/src/usb/xhci.rs` — device enumeration
- [ ] `pc-sender/src/usb/bulk.rs` — bulk OUT transfer
- [ ] Integration into NexTOS kernel (MMIO mapping, IRQ handling)
- [ ] **Acceptance:** an echo from the kernel reaches a host-side USB sniffer.

## Phase 3 — Real framebuffer pipeline

**Goal:** actual NexTOS screen content reaches the tablet.

- [ ] `pc-sender/src/fb.rs` — read framebuffer (GOP or kernel LFB)
- [ ] Wire encoder + bulk sender in a 10 fps polling loop
- [ ] `android-app` displays frames live
- [ ] **Acceptance:** the boot screen of NexTOS is visible on the tablet.

## Phase 4 — Tablet local capture (Mode 2)

**Goal:** Mode 2 (own-screen capture) works.

- [ ] `android-app/.../ScreenCapture.kt` — `mmap(/dev/graphics/fb0)` (root)
- [ ] `ScreenCapture.save(path, format)` — PNG export
- [ ] UI button to trigger capture
- [ ] **Acceptance:** a screenshot of the tablet's own screen is saved as PNG.

## Phase 5 — MCP bridge

**Goal:** LLM (Mavis) can read the tablet screen on demand.

- [ ] `pc-mcp-bridge/http_client.py` — HTTP/WebSocket client to tablet
- [ ] `pc-mcp-bridge/mcp_server.py` — MCP server with `screen.read/info/history/save`
- [ ] `android-app/.../HttpServer.kt` — WebSocket server on port 8765
- [ ] Register MCP server in Mavis `mcp.json`
- [ ] **Acceptance:** from this conversation, I can call `screen.read()` and receive the current tablet screen.

## Phase 6 — Optimizations (ongoing)

- [ ] RLE / delta-frame compression (v2 protocol)
- [ ] Adaptive resolution (downscale on CPU bottleneck)
- [ ] Reconnection logic (auto-reconnect on USB unplug)
- [ ] Multi-tile partial updates
- [ ] PNG → MP4 recording option
