"""Generates test frames and emits them as wire packets.

Two output modes:
  - stdout (default): pipe to mock_receiver.py
  - file (-o PATH): save a binary blob for replay/testing

Each frame is a deterministic RGB565 pattern (horizontal color bands), so
the receiver can visually confirm what it received matches what was sent.

Usage:
  # Stream to receiver in real time (~10 fps by default)
  python tools/mock_sender.py | python tools/mock_receiver.py

  # Save 30 frames to a file, no delay
  python tools/mock_sender.py --frames 30 --fps 0 -o test.bin

  # 1080p, slower, 5 frames
  python tools/mock_sender.py --width 1920 --height 1080 --fps 2 --frames 5
"""

import argparse
import sys
import time

from protocol_py import encode_packet, RGB565


# RGB565 colors used in the test pattern (8 bands, 16 px tall each).
_BAND_COLORS_RGB565 = [
    0x0000,  # black
    0xF800,  # red
    0x07E0,  # green
    0x001F,  # blue
    0xFFFF,  # white
    0xFFE0,  # yellow
    0x07FF,  # cyan
    0xF81F,  # magenta
]
_BAND_HEIGHT = 16


def make_test_frame(width: int, height: int) -> bytes:
    """Build a deterministic RGB565 test frame.

    Each 16-px-tall horizontal band is filled with one of 8 saturated colors.
    The pattern is fully deterministic and small enough to inspect manually.
    """
    n_bands = len(_BAND_COLORS_RGB565)
    payload = bytearray(width * height * 2)
    for y in range(height):
        band = (y // _BAND_HEIGHT) % n_bands
        color = _BAND_COLORS_RGB565[band]
        lo = color & 0xFF
        hi = (color >> 8) & 0xFF
        row_offset = y * width * 2
        # Fill the whole row in one go
        for x in range(width):
            payload[row_offset + x * 2] = lo
            payload[row_offset + x * 2 + 1] = hi
    return bytes(payload)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--width", type=int, default=1280, help="frame width")
    p.add_argument("--height", type=int, default=720, help="frame height")
    p.add_argument("--frames", type=int, default=10, help="number of frames to send")
    p.add_argument("--fps", type=float, default=10.0,
                   help="frames per second (use 0 for no delay)")
    p.add_argument("-o", "--output", default="-",
                   help="output file path, or '-' for stdout (default)")
    args = p.parse_args()

    if args.output == "-":
        out = sys.stdout.buffer
    else:
        out = open(args.output, "wb")

    period = 1.0 / args.fps if args.fps > 0 else 0.0
    try:
        for frame_id in range(args.frames):
            payload = make_test_frame(args.width, args.height)
            packet = encode_packet(
                frame_id=frame_id,
                width=args.width,
                height=args.height,
                fmt=RGB565,
                is_key_frame=True,
                payload=payload,
            )
            out.write(packet)
            out.flush()
            if frame_id < args.frames - 1 and period > 0:
                time.sleep(period)
    finally:
        if out is not sys.stdout.buffer:
            out.close()

    return 0


if __name__ == "__main__":
    sys.exit(main())
