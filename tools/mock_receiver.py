"""Reads wire packets from stdin/file and decodes them to PNG.

Decodes the ScreenViewerOnTablet protocol and saves one PNG per frame.
Useful to:
  - Verify a sender (e.g. mock_sender.py or the Rust encoder) produces valid
    packets.
  - Get visual confirmation that the protocol roundtrip works.
  - Replay a saved `.bin` file offline.

Usage:
  # Live: pipe from mock_sender
  python tools/mock_sender.py | python tools/mock_receiver.py

  # From a file
  python tools/mock_receiver.py -i test.bin

  # Custom output directory
  python tools/mock_receiver.py -i test.bin -o ./out/

Requires Pillow:
  pip install pillow
"""

import argparse
import os
import struct
import sys

from protocol_py import HEADER_LEN, RGB565, decode_packet

try:
    from PIL import Image
except ImportError:
    sys.stderr.write(
        "ERROR: Pillow is required. Install with: pip install pillow\n"
    )
    sys.exit(1)


def rgb565_to_pixels(buf: bytes):
    """Convert packed RGB565 bytes (little-endian) to an iterable of (R, G, B)
    8-bit tuples suitable for `PIL.Image.putdata`.
    """
    out = []
    for i in range(0, len(buf), 2):
        lo = buf[i]
        hi = buf[i + 1]
        rgb = lo | (hi << 8)
        r5 = (rgb >> 11) & 0x1F
        g6 = (rgb >> 5) & 0x3F
        b5 = rgb & 0x1F
        # Expand to 8 bits via replication of MSBs.
        r8 = (r5 << 3) | (r5 >> 2)
        g8 = (g6 << 2) | (g6 >> 4)
        b8 = (b5 << 3) | (b5 >> 2)
        out.append((r8, g8, b8))
    return out


def parse_stream(buf: bytes):
    """Walk a byte stream, yielding (info_dict, packet_bytes) for each valid
    packet. Resyncs on bad magic by skipping forward.

    Yielded `info_dict` comes from `protocol_py.decode_packet`. Errors are
    reported via stderr and the stream advances past the bad region.
    """
    pos = 0
    n = len(buf)
    while pos + HEADER_LEN <= n:
        if buf[pos:pos + 4] != b"NTSS":
            pos += 1
            continue
        if pos + HEADER_LEN > n:
            return
        payload_len = struct.unpack_from("<I", buf, pos + 16)[0]
        total = HEADER_LEN + payload_len
        if pos + total > n:
            return  # truncated, caller may wait for more data
        packet = bytes(buf[pos:pos + total])
        try:
            info = decode_packet(packet)
        except ValueError as e:
            sys.stderr.write(f"[skip] {e}\n")
            pos += 1
            continue
        yield info, packet
        pos += total


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("-i", "--input", default="-",
                   help="input file path, or '-' for stdin (default)")
    p.add_argument("-o", "--output", default="./received_frames/",
                   help="output directory for PNG files (default: ./received_frames/)")
    p.add_argument("--prefix", default="frame_",
                   help="PNG filename prefix (default: frame_)")
    args = p.parse_args()

    if args.input == "-":
        data = sys.stdin.buffer.read()
    else:
        with open(args.input, "rb") as f:
            data = f.read()

    os.makedirs(args.output, exist_ok=True)

    saved = 0
    skipped = 0
    for info, _packet in parse_stream(data):
        if info["format"] == RGB565:
            pixels = rgb565_to_pixels(info["payload"])
            img = Image.new("RGB", (info["width"], info["height"]))
            img.putdata(pixels)
            name = f"{args.prefix}{info['frame_id']:06d}.png"
            path = os.path.join(args.output, name)
            img.save(path)
            sys.stderr.write(
                f"frame {info['frame_id']:>4}: "
                f"{info['width']}x{info['height']} RGB565 -> {path}\n"
            )
            saved += 1
        else:
            sys.stderr.write(
                f"frame {info['frame_id']:>4}: "
                f"format {info['format']} not supported, skipping\n"
            )
            skipped += 1

    sys.stderr.write(f"\nDone. saved={saved} skipped={skipped}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
