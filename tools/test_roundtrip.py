"""Roundtrip + reference-value tests for the wire protocol.

These tests validate the Python reference implementation. They do NOT test the
Rust encoder directly (that needs `cargo test` on your machine). The crucial
test here is `test_reference_packet` — its expected bytes are computable by
hand, so if the Rust unit tests assert the same values, the two
implementations are bit-identical and the wire format is locked.

Run:
    cd tools
    python test_roundtrip.py
"""

import struct
import sys

from protocol_py import (
    MAGIC, PROTOCOL_VERSION, HEADER_LEN, RGB565,
    encode_packet, decode_packet,
)


def test_magic_and_version():
    pkt = encode_packet(0, 1, 1, RGB565, True, b"\x00\x00")
    assert pkt[0:4] == MAGIC == b"NTSS", f"bad magic: {pkt[0:4]!r}"
    assert pkt[4] == PROTOCOL_VERSION == 1, f"bad version: {pkt[4]}"
    assert len(pkt) == HEADER_LEN + 2


def test_roundtrip_simple():
    payload = bytes(range(256)) * 4  # 1024 bytes
    pkt = encode_packet(42, 32, 16, RGB565, True, payload)
    info = decode_packet(pkt)
    assert info["frame_id"] == 42
    assert info["width"] == 32
    assert info["height"] == 16
    assert info["format"] == RGB565
    assert info["is_key_frame"] is True
    assert info["payload"] == payload


def test_roundtrip_empty_payload():
    pkt = encode_packet(0, 1, 1, RGB565, True, b"")
    info = decode_packet(pkt)
    assert info["payload"] == b""
    assert info["payload_len"] == 0


def test_roundtrip_large_payload():
    # 2 MB, exercises the u32 length path
    payload = b"\xAB\xCD" * (1024 * 1024)
    pkt = encode_packet(12345, 1920, 1080, RGB565, False, payload)
    info = decode_packet(pkt)
    assert info["payload"] == payload
    assert info["frame_id"] == 12345
    assert info["is_key_frame"] is False


def test_key_frame_flag():
    pkt_kf = encode_packet(0, 1, 1, RGB565, True, b"")
    pkt_df = encode_packet(0, 1, 1, RGB565, False, b"")
    assert pkt_kf[5] == 1, f"key frame flag set in delta frame: {pkt_kf[5]}"
    assert pkt_df[5] == 0, f"key frame flag not set in key frame: {pkt_df[5]}"
    assert decode_packet(pkt_kf)["is_key_frame"] is True
    assert decode_packet(pkt_df)["is_key_frame"] is False


def test_crc32_mismatch_detected():
    pkt = bytearray(encode_packet(0, 1, 1, RGB565, True, b"\x00\x00"))
    pkt[HEADER_LEN] ^= 0x01  # flip a bit in the payload
    try:
        decode_packet(bytes(pkt))
    except ValueError as e:
        assert "CRC32" in str(e), f"unexpected error: {e}"
        return
    raise AssertionError("CRC32 mismatch was not detected")


def test_bad_magic_detected():
    pkt = bytearray(encode_packet(0, 1, 1, RGB565, True, b""))
    pkt[0] = ord("X")
    try:
        decode_packet(bytes(pkt))
    except ValueError as e:
        assert "magic" in str(e).lower(), f"unexpected error: {e}"
        return
    raise AssertionError("bad magic was not detected")


def test_bad_version_detected():
    pkt = bytearray(encode_packet(0, 1, 1, RGB565, True, b""))
    pkt[4] = 99  # version we don't support
    try:
        decode_packet(bytes(pkt))
    except ValueError as e:
        assert "version" in str(e), f"unexpected error: {e}"
        return
    raise AssertionError("bad version was not detected")


def test_truncated_payload_detected():
    pkt = encode_packet(0, 1, 1, RGB565, True, b"\x00" * 100)
    truncated = pkt[:HEADER_LEN + 50]  # claims 100 bytes, sends 50
    try:
        decode_packet(truncated)
    except ValueError as e:
        assert "truncated" in str(e) or "short" in str(e), f"unexpected: {e}"
        return
    raise AssertionError("truncated payload was not detected")


def test_reference_packet():
    """Ground-truth packet for a known input. Hand-computable.

    If this passes, the encoder is correct. If the Rust unit test
    `crc32_stable_for_same_input` (or its equivalent) produces the same bytes
    for the same input, the two implementations are bit-identical.

    Input:
      frame_id   = 0x12345678
      width      = 0x0102 (258)
      height     = 0x0304 (772)
      format     = 0 (RGB565)
      is_key     = True
      payload    = b"hello"  (5 bytes)
    CRC32(b"hello") = 0x3610A686 (IEEE 802.3, zlib polynomial)

    Expected packet (29 bytes total):
      4E 54 53 53  -- "NTSS"
      01           -- version
      01           -- flags (key frame)
      02 01        -- width LE  = 0x0102
      04 03        -- height LE = 0x0304
      00 00        -- format
      78 56 34 12  -- frame_id LE = 0x12345678
      05 00 00 00  -- payload_len = 5
      86 A6 10 36  -- CRC32 of "hello" LE
      68 65 6C 6C 6F  -- "hello"
    """
    pkt = encode_packet(
        frame_id=0x12345678,
        width=0x0102,
        height=0x0304,
        fmt=RGB565,
        is_key_frame=True,
        payload=b"hello",
    )
    assert len(pkt) == 24 + 5, f"packet length: {len(pkt)}"
    assert pkt[0:4] == b"NTSS"
    assert pkt[4] == 1, f"version: {pkt[4]}"
    assert pkt[5] == 1, f"flags: {pkt[5]}"
    assert pkt[6:8] == b"\x02\x01", f"width LE: {pkt[6:8]!r}"
    assert pkt[8:10] == b"\x04\x03", f"height LE: {pkt[8:10]!r}"
    assert pkt[10:12] == b"\x00\x00", f"format: {pkt[10:12]!r}"
    assert pkt[12:16] == b"\x78\x56\x34\x12", f"frame_id LE: {pkt[12:16]!r}"
    assert pkt[16:20] == b"\x05\x00\x00\x00", f"payload_len: {pkt[16:20]!r}"
    assert pkt[20:24] == struct.pack("<I", 0x3610A686), (
        f"CRC32: {pkt[20:24]!r} (expected 86 A6 10 36)"
    )
    assert pkt[24:29] == b"hello"


def main() -> int:
    tests = [
        test_magic_and_version,
        test_roundtrip_simple,
        test_roundtrip_empty_payload,
        test_roundtrip_large_payload,
        test_key_frame_flag,
        test_crc32_mismatch_detected,
        test_bad_magic_detected,
        test_bad_version_detected,
        test_truncated_payload_detected,
        test_reference_packet,
    ]
    failed = 0
    for t in tests:
        try:
            t()
            print(f"  PASS  {t.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"  FAIL  {t.__name__}: {e}")
        except Exception as e:
            failed += 1
            print(f"  ERROR {t.__name__}: {type(e).__name__}: {e}")
    total = len(tests)
    print(f"\n{total - failed}/{total} passed")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
