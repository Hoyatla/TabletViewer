//! Packet encoder for the ScreenViewerOnTablet protocol.
//!
//! See `docs/PROTOCOL.md` for the wire format.

use crate::{MAGIC, PROTOCOL_VERSION};
use crc32fast::Hasher;

/// Pixel format of the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PixelFormat {
    Rgb565 = 0,
    Rgba32 = 1,
    Jpeg = 2,
}

impl PixelFormat {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0 => Some(Self::Rgb565),
            1 => Some(Self::Rgba32),
            2 => Some(Self::Jpeg),
            _ => None,
        }
    }
}

/// Frame metadata passed to [`encode`].
#[derive(Debug, Clone, Copy)]
pub struct FrameInfo {
    pub width: u16,
    pub height: u16,
    pub format: PixelFormat,
    pub frame_id: u32,
    pub is_key_frame: bool,
}

/// Encoded packet ready to be sent over USB bulk.
#[derive(Debug)]
pub struct Packet<'a> {
    pub header: [u8; 24],
    pub payload: &'a [u8],
}

/// Encode a single frame into a wire packet.
///
/// The header is 24 bytes, followed by the payload. The CRC32 covers the
/// payload only (header excluded, per protocol spec).
pub fn encode(frame_id: u32, info: FrameInfo, payload: &[u8]) -> Packet<'_> {
    let mut header = [0u8; 24];

    // magic (4 bytes)
    header[0..4].copy_from_slice(&MAGIC);
    // version (1 byte)
    header[4] = PROTOCOL_VERSION;
    // flags (1 byte) — bit 0 = key frame
    header[5] = if info.is_key_frame { 1 } else { 0 };
    // width (2 bytes LE)
    header[6..8].copy_from_slice(&info.width.to_le_bytes());
    // height (2 bytes LE)
    header[8..10].copy_from_slice(&info.height.to_le_bytes());
    // format (2 bytes LE)
    header[10..12].copy_from_slice(&(info.format as u16).to_le_bytes());
    // frame_id (4 bytes LE)
    header[12..16].copy_from_slice(&frame_id.to_le_bytes());
    // payload_len (4 bytes LE)
    header[16..20].copy_from_slice(&(payload.len() as u32).to_le_bytes());

    // CRC32 of payload
    let mut hasher = Hasher::new();
    hasher.update(payload);
    header[20..24].copy_from_slice(&hasher.finalize().to_le_bytes());

    Packet { header, payload }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_24_bytes() {
        let info = FrameInfo {
            width: 1920,
            height: 1080,
            format: PixelFormat::Rgb565,
            frame_id: 42,
            is_key_frame: true,
        };
        let p = encode(info.frame_id, info, &[0u8; 4]);
        assert_eq!(p.header.len(), 24);
    }

    #[test]
    fn magic_is_ntss() {
        assert_eq!(&MAGIC, b"NTSS");
    }

    #[test]
    fn crc32_stable_for_same_input() {
        let info = FrameInfo {
            width: 100,
            height: 100,
            format: PixelFormat::Rgb565,
            frame_id: 1,
            is_key_frame: true,
        };
        let p1 = encode(1, info, b"hello");
        let p2 = encode(1, info, b"hello");
        assert_eq!(p1.header, p2.header);
    }

    #[test]
    fn frame_id_serialized_le() {
        let info = FrameInfo {
            width: 1,
            height: 1,
            format: PixelFormat::Rgb565,
            frame_id: 0x12345678,
            is_key_frame: true,
        };
        let p = encode(info.frame_id, info, &[0u8; 0]);
        assert_eq!(&p.header[12..16], &[0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn width_height_serialized_le() {
        let info = FrameInfo {
            width: 0x0102,
            height: 0x0304,
            format: PixelFormat::Rgb565,
            frame_id: 0,
            is_key_frame: true,
        };
        let p = encode(0, info, &[0u8; 0]);
        assert_eq!(&p.header[6..8], &[0x02, 0x01]);
        assert_eq!(&p.header[8..10], &[0x04, 0x03]);
    }

    #[test]
    fn key_frame_flag_set() {
        let info = FrameInfo {
            width: 1,
            height: 1,
            format: PixelFormat::Rgb565,
            frame_id: 0,
            is_key_frame: true,
        };
        let p = encode(0, info, &[0u8; 0]);
        assert_eq!(p.header[5], 1);

        let info2 = FrameInfo { is_key_frame: false, ..info };
        let p2 = encode(0, info2, &[0u8; 0]);
        assert_eq!(p2.header[5], 0);
    }

    #[test]
    fn empty_payload_yields_valid_packet() {
        let info = FrameInfo {
            width: 1,
            height: 1,
            format: PixelFormat::Rgb565,
            frame_id: 0,
            is_key_frame: true,
        };
        let p = encode(0, info, &[]);
        assert_eq!(&p.header[16..20], &[0, 0, 0, 0]);
    }
}
