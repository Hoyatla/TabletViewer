# Protocol — ScreenViewerOnTablet

## Transport

- USB bulk transfer, **vendor-specific** class (no standard OS driver).
- Direction: **PC (host) → Tablet (device)**, single bulk endpoint pair.
- Endpoints: 1× bulk OUT (PC side) / 1× bulk IN (tablet side).

## USB device descriptor

| Field            | Value                                              |
|------------------|----------------------------------------------------|
| bDeviceClass     | `0xFF` (vendor-specific)                           |
| idVendor         | `0x1209` (pid.codes test range — replace for prod) |
| idProduct        | `0x0001`                                           |
| iManufacturer    | `"NexTOS"`                                         |
| iProduct         | `"ScreenStream"`                                   |
| iSerial          | unique per session                                 |
| bcdDevice        | `0x0100`                                           |

> For production, obtain a real USB-IF VID or stay in the pid.codes test range
> (acceptable for personal/hobbyist use, prohibited for commercial sale).

## Frame packet format

All integers are **little-endian**.

| Offset | Size | Field        | Description                                       |
|--------|------|--------------|---------------------------------------------------|
| 0      | 4    | `magic`      | `0x4E 0x54 0x53 0x53` → ASCII `"NTSS"`            |
| 4      | 1    | `version`    | Protocol version (currently `1`)                 |
| 5      | 1    | `flags`      | bit 0 = key frame (reserved bits = 0)             |
| 6      | 2    | `width`      | Pixel width                                       |
| 8      | 2    | `height`     | Pixel height                                      |
| 10     | 2    | `format`     | `0` = RGB565, `1` = RGB32, `2` = JPEG             |
| 12     | 4    | `frame_id`   | Monotonically increasing per session              |
| 16     | 4    | `payload_len`| Size of payload in bytes                          |
| 20     | 4    | `crc32`      | CRC32 of payload only (header excluded)           |
| 24     | N    | `payload`    | Pixel data                                        |

Header is fixed at 24 bytes. Total packet = 24 + `payload_len`.

## Behavior

1. PC sends one packet per frame at fixed rate (default 10 fps, configurable).
2. Tablet acknowledges each packet with a zero-length packet (ZLP) on bulk OUT.
3. On CRC32 mismatch, tablet drops the packet silently and continues.
4. If no packet is received for 500 ms, tablet displays a "no signal" overlay.
5. The first packet after connection must have `flags & 1 == 1` (key frame).
6. `frame_id` is incremented by 1 per packet, wrapping at `u32::MAX`.

## Versioning

- **v1 (current)**: key frames only, RGB565 preferred.
- **v2 (planned)**: delta frames, RLE compression, multi-resolution tiles.
- **Forward compatibility**: v2+ receivers must accept v1 packets.

## Constraints (Tab S9 FE)

- USB-C 2.0 only → 480 Mbps theoretical, ~40 MB/s real throughput.
- 1920×1080 RGB565 = 4.15 MB/frame → ~9.6 fps max → 10 fps is on the edge.
- Recommended: downscale to 1280×720 RGB565 = 1.84 MB/frame → ~21 fps headroom.
