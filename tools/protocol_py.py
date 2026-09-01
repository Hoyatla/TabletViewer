"""Python reference implementation of the ScreenViewerOnTablet wire protocol.

Mirrors `pc-sender/src/enc.rs`. Both implementations must produce bit-identical
output for the same input. This file serves as:
  1. Documentation of the wire format (readable alongside docs/PROTOCOL.md).
  2. A second implementation we can cross-check against the Rust one.
  3. A builder for mock sender/receiver scripts that don't need the Rust toolchain.

If you change one, change the other.
"""

import struct
import zlib

# --- Constants (must match pc-sender/src/lib.rs and docs/PROTOCOL.md) ---

MAGIC = b"NTSS"  # 4 bytes
PROTOCOL_VERSION: int = 1
HEADER_LEN: int = 24

# Pixel format codes (must match enc::PixelFormat in Rust)
RGB565: int = 0
RGBA32: int = 1
JPEG: int = 2


def encode_packet(
    frame_id: int,
    width: int,
    height: int,
    fmt: int,
    is_key_frame: bool,
    payload: bytes,
) -> bytes:
    """Build a wire packet identical to `enc::encode()` in pc-sender/src/enc.rs.

    Layout (24-byte header + payload):
        [0..4]   magic         b"NTSS"
        [4]      version       PROTOCOL_VERSION
        [5]      flags         bit 0 = key frame
        [6..8]   width         u16 LE
        [8..10]  height        u16 LE
        [10..12] format        u16 LE
        [12..16] frame_id      u32 LE
        [16..20] payload_len   u32 LE
        [20..24] crc32         u32 LE (CRC32 of payload only, IEEE 802.3)
    """
    if len(payload) > 0xFFFFFFFF:
        raise ValueError("payload too large")

    header = bytearray(HEADER_LEN)
    header[0:4] = MAGIC
    header[4] = PROTOCOL_VERSION & 0xFF
    header[5] = 1 if is_key_frame else 0
    struct.pack_into("<H", header, 6, width & 0xFFFF)
    struct.pack_into("<H", header, 8, height & 0xFFFF)
    struct.pack_into("<H", header, 10, fmt & 0xFFFF)
    struct.pack_into("<I", header, 12, frame_id & 0xFFFFFFFF)
    struct.pack_into("<I", header, 16, len(payload))
    crc = zlib.crc32(payload) & 0xFFFFFFFF
    struct.pack_into("<I", header, 20, crc)
    return bytes(header) + payload


def decode_packet(packet: bytes) -> dict:
    """Parse a wire packet.

    Returns a dict with keys: version, is_key_frame, width, height, format,
    frame_id, payload_len, payload.

    Raises ValueError on bad magic, unsupported version, truncated payload,
    or CRC32 mismatch.
    """
    if len(packet) < HEADER_LEN:
        raise ValueError(f"packet too short: {len(packet)} < {HEADER_LEN}")
    if packet[0:4] != MAGIC:
        raise ValueError(f"bad magic: {packet[0:4]!r}")
    version = packet[4]
    if version != PROTOCOL_VERSION:
        raise ValueError(f"unsupported version: {version}")
    flags = packet[5]
    width, height = struct.unpack_from("<HH", packet, 6)
    fmt = struct.unpack_from("<H", packet, 10)[0]
    frame_id = struct.unpack_from("<I", packet, 12)[0]
    payload_len = struct.unpack_from("<I", packet, 16)[0]
    crc = struct.unpack_from("<I", packet, 20)[0]
    if len(packet) < HEADER_LEN + payload_len:
        raise ValueError(
            f"truncated payload: have {len(packet) - HEADER_LEN}, "
            f"want {payload_len}"
        )
    payload = packet[HEADER_LEN:HEADER_LEN + payload_len]
    actual_crc = zlib.crc32(payload) & 0xFFFFFFFF
    if actual_crc != crc:
        raise ValueError(
            f"CRC32 mismatch: got {actual_crc:#010x}, want {crc:#010x}"
        )
    return {
        "version": version,
        "is_key_frame": (flags & 1) != 0,
        "width": width,
        "height": height,
        "format": fmt,
        "frame_id": frame_id,
        "payload_len": payload_len,
        "payload": payload,
    }
