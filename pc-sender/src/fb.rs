//! Framebuffer reader.
//!
//! Stub: to be integrated with the NexTOS GOP (UEFI) or kernel linear
//! framebuffer subsystem. See `docs/ROADMAP.md` Phase 3.

/// Metadata describing a framebuffer region.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bpp: u8,
    pub base: *mut u8,
}

// SAFETY: the caller must ensure the framebuffer memory is valid and that
// no other code is concurrently writing to it.
pub unsafe fn read_current() -> Option<(*const u8, FramebufferInfo)> {
    // TODO: integrate with NexTOS fb subsystem (GOP or kernel LFB).
    None
}
