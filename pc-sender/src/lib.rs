//! ScreenViewerOnTablet — sender crate (PC side, integrates into NexTOS).
//!
//! Modules:
//! - [`enc`] — packet encoder (header + payload + CRC32).
//! - [`fb`]  — framebuffer reader (stub, integrate with NexTOS fb subsystem).
//! - [`usb`] — USB host stack: xHCI driver + bulk transfer (stubs).
//!
//! See `docs/PROTOCOL.md` for the wire format.

#![cfg_attr(feature = "baremetal", no_std)]

pub mod enc;
pub mod fb;
pub mod usb;

pub const PROTOCOL_VERSION: u8 = 1;

/// "NTSS" — packet magic. Matches the 4-byte start of every frame packet.
pub const MAGIC: [u8; 4] = [0x4E, 0x54, 0x53, 0x53];
