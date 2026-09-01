# Decisions log

Each entry: **date — decision — rationale**.

## 2026-09-01 — Project bootstrap

### D1. Vendor-specific USB class, not UVC

**Decision:** Use USB vendor-specific class (`0xFF`) with our own protocol.

**Rationale:** No OS driver to write on the Android side. The Kotlin app owns
the entire protocol end-to-end. Trade-off: no out-of-the-box compatibility
with standard USB video viewers, but full control over the wire format.

### D2. RGB565 default pixel format

**Decision:** Encode frames in RGB565 (16 bpp) by default.

**Rationale:** USB-C 2.0 on Tab S9 FE tops at ~40 MB/s real throughput.

- 1080p RGB32 = 8.29 MB/frame → ~5 fps max
- 1080p RGB565 = 4.15 MB/frame → ~9.6 fps max
- 720p RGB565 = 1.84 MB/frame → ~21 fps headroom

RGB565 is the sweet spot. RGB32 reserved for the v2 protocol with compression.

### D3. PNG sequence for recording (not MP4)

**Decision:** Save recordings as numbered PNG files, not MP4/H.264.

**Rationale:** Per-frame image files are easier to inspect, post-process, and
seek. Trade-off: more files for long sessions, but disk space is cheap and we
can compress in post via `ffmpeg`.

### D4. Framebuffer capture (root) for Mode 2

**Decision:** Mode 2 reads `/dev/graphics/fb0` directly via `mmap`. No
`MediaProjection`, no `screencap`, no `UiAutomation`.

**Rationale:**

- `MediaProjection` requires a user prompt + shows a persistent notification.
- `screencap` is a shell command, inaccessible to user-mode apps.
- `UiAutomation` is for instrumentation tests, not regular apps.

Direct framebuffer read is the only API that works in background, with no
prompt, no notification, and from a regular user app — at the cost of root.

### D5. MCP server runs on PC, proxies over HTTP

**Decision:** The MCP server runs on the PC. The Android app exposes
HTTP/WebSocket on port 8765. The PC uses `adb reverse` to reach it.

**Rationale:** The MCP Python SDK is mature and well-supported on desktop. The
tablet just needs a small HTTP server (already a known Android pattern via
`nanohttpd`). Avoids running a full MCP stack on the tablet.

### D6. 10 fps target

**Decision:** Default frame rate is 10 fps. Configurable per session.

**Rationale:** Enough for human visual perception of debug screens. Saves USB
bandwidth (the bottleneck). Configurable so we can dial up if needed.

### D7. No crypto on the screen stream

**Decision:** Screen packets are not signed, not encrypted.

**Rationale:** This is a debug tool, not a control channel. The crypto channel
in NexTOS (HMAC-SHA3-256 + Ed25519) is for VM commands. Adding crypto here
would burn CPU on both the hypervisor and the tablet with no security gain.

### D8. Test VID (`0x1209`)

**Decision:** Use the pid.codes test VID for now.

**Rationale:** Free, no paperwork. Acceptable for personal use. If the project
goes commercial, we must obtain a real USB-IF VID (or move to a different
transport like network — not the current goal).

## Pending

- [ ] Final storage path scheme (currently scoped storage under app's private dir).
- [ ] Adaptive resolution strategy (when does the sender downscale?).
- [ ] Whether to support v2 protocol (delta frames) from the start, or only v1.
- [ ] Exact `mcp.json` registration syntax for Mavis.
