# Decisions log

Each entry: **date — decision — rationale**.

## 2026-09-01 — Project pivot

The project was originally a one-way framebuffer streamer (NexTOS →
tablet over USB bulk, with a custom packet format and an MCP bridge
for LLM access). It was re-scoped on 2026-09-01 to be a **bidirectional
debug API** instead: the tablet is a client, the PC runs a small
HTTP/JSON agent, and there are two transports (LAN, USB accessory).

The rest of the decisions below apply to the pivoted design.

### D1. HTTP/JSON over a custom binary protocol

**Decision:** the agent speaks plain HTTP/1.1 with JSON bodies (and
PNG for screenshots). The tablet uses OkHttp 4.12.

**Rationale:** no parser to write on either side, every tool on earth
can already talk to the agent for debugging, and the screen sizes
involved (~400 B for `system`, a few KB for `processes`, the screen
itself for `screenshot`) don't justify a custom binary format. The
old NTSS packet format is gone — see `docs/PROTOCOL.md` for the new
spec.

### D2. Axum on the Rust side

**Decision:** `axum 0.7` + `tokio 1.40` + `tower-http 0.6`.

**Rationale:** mature, well-documented, ergonomic, and integrates
with the tower ecosystem (`TraceLayer`, `CorsLayer`, and most
importantly `ServiceExt::oneshot` for the integration tests). No
performance reason to reach for `hyper` directly.

### D3. mDNS for LAN auto-discovery

**Decision:** the agent advertises itself as `_pcagent._tcp.local.`
on startup (`mdns-sd 0.11`). The tablet uses Android's built-in
`NsdManager` to browse for 4 s and pick the first match.

**Rationale:** zero-config on the same LAN, no app to type an IP, and
both sides ship a mDNS stack. WAN/routed networks are out of scope
for this debug tool; the URL field stays editable for those.

### D4. Optional bearer token, not mandatory

**Decision:** `--token` / `PC_AGENT_TOKEN` is opt-in. When unset, the
agent logs a `WARN` and accepts any request.

**Rationale:** the user explicitly asked for "debug a PC by plugging
it in or putting it on the LAN". A mandatory token would defeat
that. The user can opt in for any non-trusted setup. The `WARN` log
line is the signal that auth is off.

### D5. Optional `--roots` for filesystem isolation

**Decision:** `--roots <dir1>,<dir2>` (or `PC_AGENT_ROOTS`) restricts
`/v1/file` and `/v1/log` to the canonicalized paths under those
directories. Empty = any path.

**Rationale:** on a personal machine this is unnecessary, on a shared
machine it's nice to have. The same flag also serves as a passive
"don't read /etc/shadow" guard against accidental mis-use.

### D6. Screenshot via Windows GDI (`screenshots` crate)

**Decision:** `screenshots 0.8` returns `image::RgbaImage`; the
handler re-encodes to PNG with `image::write_to` over a
`Cursor<Vec<u8>>`.

**Rationale:** `screenshots 0.8` returns the RGBA pixels directly
(not a `screenshots::Image` as in 0.7+), which means we don't pull
in an extra encoder crate. PNG is lossless, supported everywhere,
and ~200 KB to ~2 MB depending on screen contents — well within the
30 s OkHttp read timeout.

### D7. `sysinfo 0.30`, not 0.32

**Decision:** pin `sysinfo = "0.30"`.

**Rationale:** `sysinfo 0.32` changed `refresh_processes()` to take
a `ProcessesToUpdate` argument; the 0.30 zero-arg version matches the
agent's "refresh everything" use case. Pinning 0.30 avoids the
break and is API-stable enough for what we need.

### D8. `Vec<u8>` → `Cursor<Vec<u8>>` for `image::write_to`

**Decision:** when encoding a screenshot, build a
`std::io::Cursor::new(Vec::with_capacity(...))`, hand it to
`image::write_to`, and then `into_inner()` to get the bytes back.

**Rationale:** `image::write_to` requires a `Write + Seek`; `Vec<u8>`
only implements `Write`. `Cursor<Vec<u8>>` wraps it and adds the
seek. The pre-allocated capacity avoids the first few reallocs.

### D9. Two transports on the tablet, not one

**Decision:** the app exposes both LAN and USB in a `Spinner`. The
UI doesn't care which is active — both impls implement
`DebugApi`.

**Rationale:** the user explicitly asked for "branchement USB ou
réseau local". The transport picker keeps the option open without
adding complexity at the call sites.

### D10. USB accessory mode, not device mode

**Decision:** the tablet is the USB **device** (accessory role), the
PC is the **host**. The PC bridge uses libusb to talk to the
accessory.

**Rationale:** the agent runs on the PC. The PC is the natural USB
host. With device mode (tablet = host), the tablet would need a USB
host API and a driver on the PC side; accessory mode puts the driver
on the PC, which is the side that can install things.

### D11. Reuse `LanDebugApi` over USB via a localhost bridge

**Decision:** the PC bridge listens on `127.0.0.1:8766` and forwards
each request to `127.0.0.1:8765` (the agent) after wrapping it in a
bulk round-trip. The Android side just builds a `LanDebugApi` against
`http://127.0.0.1:8766`.

**Rationale:** the HTTP/JSON parser already lives in `LanDebugApi`.
Reusing it means no duplicated parsing/serialization code. The
bridge is a thin USB-to-TCP tunneler, ~100 lines of Rust.

### D12. OkHttp on the Android side, not `HttpURLConnection`

**Decision:** `com.squareup.okhttp3:okhttp:4.12.0`.

**Rationale:** ergonomic timeouts, easy `Request.Builder`, easy
header injection for the bearer token, and well-known keep-alive
behavior. `HttpURLConnection` works but is awkward.

### D13. R8 + resource shrinker on release

**Decision:** `isMinifyEnabled = true` and `isShrinkResources = true`
on `buildType.release`.

**Rationale:** the app has a small surface (six endpoints, one
activity, one layout). The shrinker cuts the APK from 8.71 MB (debug)
to 1.66 MB (release) without any manual keep-rule work beyond the
basics (OkHttp, coroutines, JSON, lifecycle).

### D14. Self-signed keystore for now

**Decision:** generated a self-signed `release.keystore` (`pcdebug` /
`changeit`, 10-year validity) and `.gitignored` it. The Gradle config
reads `KEYSTORE_FILE` / `KEYSTORE_PASS` / `KEY_ALIAS` / `KEY_PASS` env
vars and falls back to the dev keystore.

**Rationale:** this is a personal/hobbyist debug tool. Real signing
keys belong to the user and don't go in the repo. The env-var
override is the right escape hatch for a real keystore when the
project graduates to that.

### D15. No crypto on the API

**Decision:** the bearer token is a single shared secret, not a real
auth protocol. There is no signing, no nonce, no replay protection.

**Rationale:** same as the old D7 in the previous version of this
file: this is a debug tool, not a control channel. The agent runs
on a host the user already controls. A real auth model would burn
complexity with no security gain — anyone on the LAN with the token
can already do anything the host's user can do.

### D16. NsdManager has no `discoverFor(timeoutMs)` future

**Decision:** `NetworkDiscovery.discover()` polls: it kicks off
`NsdManager.discoverServices(...)`, schedules a
`Handler.postDelayed` to call `stopServiceDiscovery` after
`timeoutMs`, and resumes the coroutine with whatever the listener
collected.

**Rationale:** `NsdManager` has no built-in "discover for N ms and
give me the result" API. The listener pattern + timer is the
standard idiom and is what most NsdManager wrappers on the Play Store
do.

## Pending

- [ ] PC bridge binary (`pc-bridge`) — see `ROADMAP.md` Phase 5.
- [ ] Multi-agent picker in the Discover result (currently auto-fills
  the URL with the first match).
- [ ] TLS for LAN — opt-in via `--tls-cert` / `--tls-key`.
- [ ] Final cleanup commit removing `pc-sender/`, `pc-mcp-bridge/`,
  `tools/`, and the stale root `README.md` (see `ROADMAP.md` Phase 8).
- [ ] End-to-end test on the actual Tab S9 FE (USB smoke + LAN
  smoke). The agent side is covered by the 9 integration tests; the
  client side is covered by `assembleDebug`/`assembleRelease` building
  clean. The on-device run is still pending.
