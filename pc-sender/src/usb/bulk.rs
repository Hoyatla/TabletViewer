//! USB bulk transfer — submit and wait.
//!
//! Spec: xHCI §4.11 Bulk Transfers, §6.4.1 Normal TRB, §6.5 Transfer Ring.
//!
//! This module owns the *transfer ring* for a single endpoint. The caller
//! has already:
//!   - Allocated the transfer ring buffer (≥ 16 TRBs, 16-byte aligned, DMA-safe)
//!   - Set the endpoint's dequeue pointer via Set TR Dequeue Pointer command
//!   - Configured the endpoint context in the device context
//!
//! We then expose submit/wait primitives that ring the doorbell and consume
//! transfer events from the event ring.

use core::sync::atomic::{fence, Ordering};

use super::xhci::{
    trb_link, Trb, XhciController, TRB_TYPE_NORMAL, CC_SUCCESS,
    CC_STALL, CC_SHORT_PACKET, CC_BABBLE, CC_USB_TRANSACTION_ERROR,
};

/// Number of TRBs in a transfer ring. Must be a power of two for the
/// wraparound math to be simple, and at minimum 16 per the xHCI spec.
pub const TRANSFER_RING_LEN: usize = 16;

/// A bulk OUT transfer endpoint, identified by (device_slot, endpoint_id).
#[derive(Debug, Clone, Copy)]
pub struct BulkEndpoint {
    pub device_slot: u8,
    pub endpoint_id: u8,
}

/// One transfer ring for one endpoint.
///
/// The integrator provides the underlying memory (must be physically
/// contiguous, 16-byte aligned, DMA-safe). We keep the base address and
/// the producer index (which TRB to write to next) here.
pub struct TransferRing {
    /// Virtual address of the ring (for CPU writes).
    pub base: *mut Trb,
    /// Physical address of the ring (for xHCI DMA reads).
    pub base_phys: u64,
    /// Producer index: index of the next TRB to write. 0..TRANSFER_RING_LEN.
    producer: usize,
    /// Current cycle state (1 = producer writes with cycle=1, 0 = cycle=0).
    cycle: bool,
}

impl TransferRing {
    /// # Safety
    /// `base` must be 16-byte aligned, point to at least TRANSFER_RING_LEN
    /// Trb slots, and be valid for both CPU and DMA access.
    pub unsafe fn new(base: *mut Trb, base_phys: u64) -> Self {
        Self {
            base,
            base_phys,
            producer: 0,
            cycle: true,
        }
    }

    /// Submit a bulk OUT transfer: write a Normal TRB and ring the doorbell.
    ///
    /// `buf_phys` is the physical address of the source data buffer.
    /// `length` is in bytes (must fit in 17 bits, so ≤ 128 KiB per TRB).
    ///
    /// # Safety
    /// `buf_phys` must point to a DMA-safe buffer at least `length` bytes
    /// long. The endpoint must have been configured.
    pub unsafe fn submit_bulk(
        &mut self,
        ctl: &XhciController,
        ep: BulkEndpoint,
        buf_phys: u64,
        length: u32,
    ) -> Result<(), &'static str> {
        if length > 0x1_FFFF {
            return Err("bulk payload too large for a single TRB (>128 KiB)");
        }

        let idx = self.producer;
        let trb_ptr = self.base.add(idx);

        // Write a Normal TRB. Layout (xHCI §6.4.1):
        //   param      = data buffer pointer [31:0]
        //   status     = data buffer pointer [63:32]
        //   addr_low   = cycle (bit 0) | TRB type (10-15) | endpoint (16-20) | slot (24-31)
        //   addr_high  = transfer length (bits 0-16) | TD size | interrupter target
        let mut word3: u32 = (TRB_TYPE_NORMAL as u32) << 10;
        if self.cycle { word3 |= 1; }
        word3 |= (ep.endpoint_id as u32 & 0x1F) << 16;
        word3 |= (ep.device_slot as u32 & 0xFF) << 24;

        (*trb_ptr) = Trb {
            param: buf_phys as u32,
            status: (buf_phys >> 32) as u32,
            addr_low: word3,
            addr_high: length & 0x1_FFFF,
        };
        fence(Ordering::SeqCst);

        // If this is the last slot in the ring, append a Link TRB that
        // toggles the cycle bit and loops back to the start.
        let next = (self.producer + 1) % TRANSFER_RING_LEN;
        if next == 0 {
            let link_ptr = self.base.add(self.producer + 1);
            (*link_ptr) = trb_link(self.base_phys, /*toggle_cycle=*/true, !self.cycle);
            fence(Ordering::SeqCst);
        }

        // Advance producer.
        self.producer = next;

        // Ring the doorbell: value = endpoint id (1..31) for endpoint rings.
        ctl.ring_doorbell(ep.device_slot, ep.endpoint_id);
        Ok(())
    }

    /// Toggle the cycle bit. Call this after the controller has consumed
    /// the Link TRB we wrote at the end of the ring (the Link TRB itself
    /// toggles, so subsequent submissions must use the new cycle).
    pub fn toggle_cycle(&mut self) {
        self.cycle = !self.cycle;
    }
}

/// Parse a Transfer Event TRB's completion code from its `status` field.
#[inline]
pub fn completion_code(event_trb: &Trb) -> u32 {
    // xHCI §6.4.5: completion code in bits 24..32 of the TRB status field.
    (event_trb.status >> 24) & 0xFF
}

/// Returns true if the completion code indicates success (including
/// short packet, which is a normal OK for bulk).
pub fn is_success(cc: u32) -> bool {
    cc == CC_SUCCESS || cc == CC_SHORT_PACKET
}

/// Returns a human-readable label for common completion codes.
pub fn completion_label(cc: u32) -> &'static str {
    match cc {
        CC_SUCCESS => "Success",
        CC_SHORT_PACKET => "Success (short packet)",
        CC_STALL => "Stall",
        CC_BABBLE => "Babble",
        CC_USB_TRANSACTION_ERROR => "USB transaction error",
        _ => "Other",
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb::xhci::Trb;

    #[test]
    fn completion_code_extracts_high_byte() {
        // status bits 24..32 = completion code
        let mut t = Trb::default();
        t.status = (CC_SUCCESS as u32) << 24;
        assert_eq!(completion_code(&t), CC_SUCCESS);

        t.status = (CC_STALL as u32) << 24;
        assert_eq!(completion_code(&t), CC_STALL);
    }

    #[test]
    fn is_success_recognises_short_packet() {
        assert!(is_success(CC_SUCCESS));
        assert!(is_success(CC_SHORT_PACKET));
        assert!(!is_success(CC_STALL));
        assert!(!is_success(CC_BABBLE));
    }

    #[test]
    fn completion_label_known_codes() {
        assert_eq!(completion_label(CC_SUCCESS), "Success");
        assert_eq!(completion_label(CC_STALL), "Stall");
        assert_eq!(completion_label(99), "Other");
    }

    #[test]
    fn ring_size_is_power_of_two() {
        // Sanity: the ring length should be a power of two so that the
        // producer wraparound is correct.
        assert!(TRANSFER_RING_LEN > 0);
        assert_eq!(TRANSFER_RING_LEN & (TRANSFER_RING_LEN - 1), 0);
    }
}
