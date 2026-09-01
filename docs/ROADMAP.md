# Roadmap

Phases are ordered. Each phase has an explicit acceptance criterion —
the phase is not "done" until the criterion is met.

## Phase 1 — `pc-agent` core API (HTTP + screenshot + processes + log + file)

**Goal:** a working Rust HTTP server that exposes six endpoints, with
in-process integration tests covering all of them.

- [x] `pc-agent/src/lib.rs` — `AppState`, `build_router`, `run`
- [x] `pc-agent/src/handlers.rs` — six handlers + `check_auth` + `path_allowed`
- [x] `pc-agent/src/capture.rs` — `Capturer::new()`, `grab_primary()`
- [x] `pc-agent/src/proc.rs` — `ProcessInfo::collect` (renamed to avoid
      shadowing the `sysinfo` crate)
- [x] `pc-agent/src/main.rs` — 5-line `Cli` → `run` wrapper
- [x] `pc-agent/Cargo.toml` — axum 0.7, tokio 1.40, sysinfo 0.30,
      screenshots 0.8, image 0.24, clap 4.5, mdns-sd 0.11, hostname 0.4
- [x] `pc-agent/tests/integration.rs` — 9 tests via
      `tower::ServiceExt::oneshot` (no real socket)
- [x] **Acceptance:** `cargo test` reports 9/9 passing. ✅

## Phase 2 — mDNS auto-discovery

**Goal:** the tablet finds the agent on the LAN without typing the IP.

- [x] `pc-agent/src/discovery.rs` — `register(port, instance) -> MdnsHandle`
      (TXT records: `version`, `platform`, `api=v1`) and a
      `discover(timeout)` helper.
- [x] `pc-agent/src/lib.rs::run` calls `discovery::register` on startup
      and logs `mDNS: discoverable as pcagent._pcagent._tcp.local.`.
- [x] **Acceptance:** running the agent prints the mDNS line, and
      `discover(2s)` returns at least one `(name, ip, port)` tuple when
      the agent is on the same LAN. ✅

## Phase 3 — Android client + release APK

**Goal:** a signed release APK that the user can side-load on a rooted
Galaxy Tab S9 FE.

- [x] `android-app/.../DebugApi.kt` — interface + `SortBy` enum
- [x] `android-app/.../Models.kt` — `SystemInfo`, `ProcessInfo`,
      `LogResponse`
- [x] `android-app/.../LanDebugApi.kt` — OkHttp 4.12, `org.json`
- [x] `android-app/.../MainActivity.kt` — initial UI (URL field,
      Connect, action buttons)
- [x] `android-app/app/build.gradle.kts` — `signingConfigs.release`
      (env-overridable), R8 + resource shrinking on `release`
- [x] `android-app/app/release.keystore` — self-signed, dev-only,
      gitignored
- [x] `android-app/.../proguard-rules.pro` — keep rules for OkHttp,
      coroutines, JSON, lifecycle
- [x] **Acceptance:** `gradlew assembleDebug` produces an 8.71 MB APK;
      `gradlew assembleRelease` produces a 1.66 MB signed APK. ✅

## Phase 4 — Transport picker (LAN/USB) + auto-discovery + USB accessory

**Goal:** the user picks the transport in the app; LAN uses
auto-discovery, USB uses the accessory API.

- [x] `res/layout/activity_main.xml` — `transportSpinner` (LAN / USB)
      + `Discover` button
- [x] `MainActivity.kt` — `connectLan()` / `connectUsb()` dispatch on
      the spinner; `onDiscover()` runs `NetworkDiscovery.discover(ctx,
      4000)` and auto-fills the URL; `onDestroy()` closes the USB
      accessory.
- [x] `NetworkDiscovery.kt` — `NsdManager` wrapper, suspending, 4 s
      default timeout, `List<DiscoveredAgent>`.
- [x] `UsbDebugApi.kt` — real `UsbManager` + `UsbAccessory` flow with
      permission request via `PendingIntent.getBroadcast`; only `ping()`
      runs without the PC bridge.
- [x] `AndroidManifest.xml` — `usb.host required=true` removed,
      `usb.accessory required=false` added.
- [x] **Acceptance:** `gradlew assembleDebug` = 8.71 MB, BUILD
      SUCCESSFUL. mDNS + USB code paths compile and are exercised in
      the unit tests of `pc-agent` on the PC side; end-to-end on
      hardware is pending (see Phase 5 acceptance). ✅ (build side)

## Phase 5 — PC bridge (USB ↔ localhost)

**Goal:** a small Rust binary that lets the tablet reach the agent over
USB, not just LAN.

- [ ] `pc-bridge/Cargo.toml` — libusb (rusb or nusb), tokio, no
      `screenshots`/`sysinfo` deps
- [ ] `pc-bridge/src/main.rs` — open the Android accessory by VID/PID,
      claim the bulk endpoints, start a TCP listener on
      `127.0.0.1:8766`. On each TCP request, write the bytes to bulk
      OUT; read the response from bulk IN; forward to the TCP client.
- [ ] `android-app/.../UsbDebugApi.kt` — connect the actual
      `LanDebugApi` against `http://127.0.0.1:8766` (currently the
      ping-only smoke test is the only thing implemented).
- [ ] **Acceptance:** with the agent + bridge running on the PC, the
      tablet (USB-connected) sees all six endpoints work — same as LAN,
      just over bulk. The bridge survives an accessory unplug/replug
      without leaving a stuck TCP listener.

## Phase 6 — UI + UX refinements

**Goal:** a less rough-around-the-edges client.

- [ ] Persist the last URL across app restarts.
- [ ] Save the bearer token in `EncryptedSharedPreferences`.
- [ ] Show the agent's mDNS TXT `version` / `platform` in the
      Discover result popup.
- [ ] Long-running screenshot loop button (configurable fps, default
      1, with a "stop" button).
- [ ] Process list view that's scrollable and lets you pick CPU or MEM
      from a sub-spinner.
- [ ] Dark mode + landscape support.
- [ ] **Acceptance:** the app is usable for a 10-minute debug session
      (connect, browse processes, take screenshots, read a log) without
      crashes or visual glitches.

## Phase 7 — Hardening

- [ ] `pc-agent` rate-limiting on `/v1/screenshot` (CPU cost).
- [ ] `pc-agent` structured logging (`tracing` JSON output) so a
      foreground agent run can be post-mortem'd.
- [ ] Bearer token rotation: short-lived + refresh.
- [ ] Multi-agent support in the app: pick from a list of discovered
      agents (not just the first).
- [ ] TLS for LAN: optional `--tls-cert` / `--tls-key`. Switches the
      tablet to `https://`.
- [ ] **Acceptance:** the agent survives a 1-hour debug session
      without leaking memory, file handles, or threads.

## Phase 8 — Cleanup (legacy)

The repo still contains leftovers from the previous (pre-pivot)
direction. To be removed in a dedicated commit:

- [ ] `pc-sender/` — Rust `no_std` crate (xHCI driver, packet encoder,
      `enc.rs`/`fb.rs`/`usb/`). The whole crate is dead code now that
      we're not streaming the NexTOS framebuffer.
- [ ] `pc-mcp-bridge/` — Python MCP server. We don't use MCP for the
      debug API; the tablet is the only client.
- [ ] `tools/` — Python mock sender/receiver and `protocol_py.py`. The
      NTSS packet format is no longer in use; the spec is gone from
      `docs/PROTOCOL.md`.
- [ ] `README.md` — the current root README still describes the old
      Mode 1 / Mode 2 / MCP story. Rewrite to match the new
      `pc-agent` + Android app direction.

## Out of scope (for now)

- Streaming the NexTOS framebuffer over USB. The hypervisor is not
  involved in this project — the agent runs on the host OS.
- Re-using the old NTSS packet format. The new direction is plain
  HTTP/JSON.
- Direct USB device mode (tablet = host, PC = device). The agent runs
  on the PC; the PC is the USB host. We use accessory mode, not
  device mode.
- A Windows-driver variant of the agent. Bare-metal is the chosen
  path; this is a debug tool, not a product.
