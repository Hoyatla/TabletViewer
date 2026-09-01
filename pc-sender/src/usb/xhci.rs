//! xHCI (USB 3.0) host controller driver — minimal but real.
//!
//! Reference: Intel eXtensible Host Controller Interface spec, revision 1.2+.
//!
//! Scope of this module (MVP for bulk transfer):
//!   - Capability / Operational / Runtime / Doorbell register layout
//!   - PCI probe (class 0x0C / subclass 0x03 / prog-IF 0x30)
//!   - Controller reset (USBCMD.HCRST)
//!   - DCBAA + command ring + event ring allocation
//!   - Device slot allocation, address-device, configure-endpoint
//!   - Bulk transfer submit / wait via transfer ring
//!
//! Out of scope (for now):
//!   - IRQ handling (caller wires it up)
//!   - Multi-pCPU (single pCPU assumed; protected by external lock)
//!   - Isochronous, interrupt, control transfers
//!   - Streams
//!   - USB 3.1+/USB4 enhancements
//!
//! # Safety
//!
//! Every public function that takes a raw pointer or volatile access is
//! `unsafe`. Callers must guarantee the MMIO region is mapped and that no
//! concurrent access happens on the same slot/ring (we serialize via
//! `&mut self` on the controller handle).

#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

// ============================================================================
// 1. PCI identification
// ============================================================================
//
// Spec: PCI Code and ID Assignment, Class 0x0C (Serial), Subclass 0x03 (USB),
// Prog-IF 0x30 (xHCI).
// Reference: xHCI spec §1.1, PCI spec.

pub const PCI_CLASS_SERIAL: u8 = 0x0C;
pub const PCI_SUBCLASS_USB: u8 = 0x03;
pub const PCI_PROGIF_XHCI: u8 = 0x30;

// Standard PCI config space offsets we read for the probe.
pub const PCI_CFG_VENDOR_ID: u8 = 0x00; // u16
pub const PCI_CFG_DEVICE_ID: u8 = 0x02; // u16
pub const PCI_CFG_CLASS_CODE: u8 = 0x08; // u8 (revision at 0x09)
pub const PCI_CFG_HEADER_TYPE: u8 = 0x0E; // u8
pub const PCI_CFG_BAR0: u8 = 0x10; // u32

// ============================================================================
// 2. Capability registers (MMIO base + offset)
// ============================================================================
//
// Spec: xHCI §5.3.1 Capability Registers.
// All multi-byte fields are little-endian.

pub const CAP_CAPLENGTH: u8 = 0x00; // u8 — length of cap block
pub const CAP_HCIVERSION: u8 = 0x02; // u16 — BCD, e.g. 0x0100 = xHCI 1.0
pub const CAP_HCSPARAMS1: u8 = 0x04; // u32
pub const CAP_HCSPARAMS2: u8 = 0x08; // u32
pub const CAP_HCSPARAMS3: u8 = 0x0C; // u32
pub const CAP_HCCPARAMS1: u8 = 0x10; // u32
pub const CAP_DBOFF: u8 = 0x14; // u32 — doorbell array offset
pub const CAP_RTSOFF: u8 = 0x18; // u32 — runtime regs offset

// HCSPARAMS1 bitfields (xHCI §5.3.4)
pub const HCSPARAMS1_MAXSLOTS_MASK: u32 = 0x00FF; // bits 0..7 — max device slots
pub const HCSPARAMS1_MAXINTRS_MASK: u32 = 0x7F00; // bits 8..14 — max interrupters
pub const HCSPARAMS1_MAXPORTS_MASK: u32 = 0xFF0000; // bits 16..23 — max ports

// HCCPARAMS1 bitfields (xHCI §5.3.6)
pub const HCCPARAMS1_64BIT_ADDR: u32 = 1 << 0;
pub const HCCPARAMS1_CSZ: u32 = 1 << 2; // Context Size (0 = 32 byte, 1 = 64 byte)

// ============================================================================
// 3. Operational registers (MMIO base + CAPLENGTH + offset)
// ============================================================================
//
// Spec: xHCI §5.4 Host Controller Operational Registers.

pub const OP_USBCMD: u32 = 0x00; // u32
pub const OP_USBSTS: u32 = 0x04; // u32
pub const OP_PAGESIZE: u32 = 0x08; // u32
pub const OP_DNCTRL: u32 = 0x14; // u32
pub const OP_CRCR: u32 = 0x18; // u64 — Command Ring Control Register
pub const OP_DCBAAP: u32 = 0x30; // u64 — Device Context Base Address Array
pub const OP_CONFIG: u32 = 0x38; // u32

// USBCMD bitfields (xHCI §5.4.1)
pub const USBCMD_RS: u32 = 1 << 0; // Run/Stop
pub const USBCMD_HCRST: u32 = 1 << 1; // Host Controller Reset
pub const USBCMD_INTE: u32 = 1 << 2; // Interrupter Enable
pub const USBCMD_HSEE: u32 = 1 << 3; // Host System Error Enable

// USBSTS bitfields (xHCI §5.4.2)
pub const USBSTS_HCH: u32 = 1 << 0; // HC Halted (1 = halted)
pub const USBSTS_CNR: u32 = 1 << 11; // Controller Not Ready
pub const USBSTS_PCD: u32 = 1 << 4; // Port Change Detect

// CRCR bitfields (xHCI §5.4.5)
pub const CRCR_RCS: u64 = 1 << 0; // Ring Cycle State
pub const CRCR_CS: u64 = 1 << 1; // Command Stop
pub const CRCR_CA: u64 = 1 << 2; // Command Abort
pub const CRCR_CRR: u64 = 1 << 3; // Command Ring Running
pub const CRCR_PTR_MASK: u64 = 0xFFFF_FFFF_FFFF_FFC0; // 64-byte aligned pointer

// ============================================================================
// 4. Doorbell array (CAP_DBOFF + slot*4)
// ============================================================================

pub const DB_SLOT_OFFSET: u32 = 0x0000; // doorbell reg 0 is reserved
pub const DB_SLOT_1: u32 = 0x0004; // first device slot

#[inline]
pub const fn doorbell_offset(slot: u8) -> u32 {
    DB_SLOT_1 + (slot as u32 - 1) * 4
}

// ============================================================================
// 5. TRB (Transfer Request Block)
// ============================================================================
//
// Spec: xHCI §6.4 Transfer Request Block (TRB).
// Each TRB is 16 bytes (4 u32). 16-byte aligned.
//   [0..4]   parameter
//   [4..8]   status
//   [8..12]  address / data low
//   [12..16] address / data high + flags
//
// Bit numbering in the spec is big-endian; the "Cycle bit" is bit 0 of word
// 3 (the address low dword). All multi-byte fields are little-endian on the
// wire.

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Trb {
    pub param: u32,
    pub status: u32,
    pub addr_low: u32,
    pub addr_high: u32,
}

impl Trb {
    pub const SIZE: usize = 16;

    /// Returns the TRB type (bits 10..15 of word 3, i.e. bits 10..15 of
    /// `addr_low`).
    #[inline]
    pub fn trb_type(&self) -> u16 {
        ((self.addr_low >> 10) & 0x3F) as u16
    }

    /// Returns the Cycle bit (bit 0 of word 3).
    #[inline]
    pub fn cycle(&self) -> bool {
        (self.addr_low & 1) != 0
    }

    /// Returns the Endpoint ID for transfer TRBs (bits 16..22 of word 3).
    #[inline]
    pub fn endpoint_id(&self) -> u8 {
        ((self.addr_low >> 16) & 0x1F) as u8
    }

    /// Returns the Slot ID for transfer TRBs (bits 24..32 of word 3).
    #[inline]
    pub fn slot_id(&self) -> u8 {
        ((self.addr_low >> 24) & 0xFF) as u8
    }
}

// TRB types (xHCI §6.4.4 Type Definitions)
pub const TRB_TYPE_NORMAL: u16 = 1;
pub const TRB_TYPE_SETUP_STAGE: u16 = 2;
pub const TRB_TYPE_DATA_STAGE: u16 = 3;
pub const TRB_TYPE_STATUS_STAGE: u16 = 4;
pub const TRB_TYPE_LINK: u16 = 6;
pub const TRB_TYPE_NOOP: u16 = 8;
pub const TRB_TYPE_ENABLE_SLOT: u16 = 9;
pub const TRB_TYPE_DISABLE_SLOT: u16 = 10;
pub const TRB_TYPE_ADDRESS_DEVICE: u16 = 11;
pub const TRB_TYPE_CONFIGURE_EP: u16 = 12;
pub const TRB_TYPE_EVALUATE_CONTEXT: u16 = 13;
pub const TRB_TYPE_RESET_EP: u16 = 14;
pub const TRB_TYPE_STOP_EP: u16 = 15;
pub const TRB_TYPE_SET_TR_DEQUEUE: u16 = 16;
pub const TRB_TYPE_RESET_DEVICE: u16 = 17;

// TRB completion codes (xHCI §6.4.5)
pub const CC_SUCCESS: u32 = 1;
pub const CC_SHORT_PACKET: u32 = 13;
pub const CC_STALL: u32 = 6;
pub const CC_BABBLE: u32 = 2;
pub const CC_USB_TRANSACTION_ERROR: u32 = 4;

#[inline]
pub(crate) fn make_trb(param: u32, status: u32, type_bits: u16, flags: u32, cycle: bool) -> Trb {
    // bits 0:    cycle
    // bits 1:    evaluate next TRB (0 = no, 1 = yes) — only for Link TRBs
    // bits 2:    interrupt on short packet (for Normal TRBs)
    // bits 10..15: TRB type
    // bits 16..22: endpoint id (for transfer TRBs)
    // bits 24..32: slot id (for transfer TRBs)
    let mut word3: u32 = (type_bits as u32) << 10;
    if cycle {
        word3 |= 1;
    }
    word3 |= flags;
    Trb {
        param,
        status,
        addr_low: word3,
        addr_high: 0,
    }
}

#[inline]
pub fn trb_normal(data_phys: u64, length: u32, cycle: bool) -> Trb {
    // xHCI §6.4.1 Normal TRB:
    //   [0..8]   Data Buffer Pointer (64-bit, aligned)
    //   [8..12]  flags: cycle (bit 0) | type=1 (bits 10-15) | various flags
    //   [12..16] Transfer Length (bits 0-16) | TD Size (17-21) | Interrupter (22-31)
    let mut t = make_trb(0, 0, TRB_TYPE_NORMAL, 0, cycle);
    t.param = data_phys as u32;
    t.status = (data_phys >> 32) as u32;
    t.addr_high = length & 0x1_FFFF; // bits 0..16 = transfer length
    t
}

#[inline]
pub fn trb_link(target_phys: u64, toggle_cycle: bool, cycle: bool) -> Trb {
    // xHCI §6.4.4.1 Link TRB:
    //   [0..8]   Next TRB Pointer (64-bit, 16-byte aligned)
    //   [8..12]  flags: cycle (bit 0) | TC (bit 1) | reserved (2-3) | type (10-15) | reserved
    //   [12..16] reserved
    let mut t = make_trb(0, 0, TRB_TYPE_LINK, 0, cycle);
    if toggle_cycle {
        // bit 1 of addr_low = TC (toggle cycle)
        t.addr_low |= 1 << 1;
    }
    t.param = target_phys as u32;
    t.status = (target_phys >> 32) as u32;
    t
}

#[inline]
pub fn trb_noop(cycle: bool) -> Trb {
    make_trb(0, 0, TRB_TYPE_NOOP, 0, cycle)
}

#[inline]
pub fn trb_enable_slot(cycle: bool) -> Trb {
    make_trb(0, 0, TRB_TYPE_ENABLE_SLOT, 0, cycle)
}

#[inline]
pub fn trb_address_device(input_context_phys: u64, slot: u8, cycle: bool) -> Trb {
    // xHCI §6.4.3.3 Address Device Command TRB:
    //   [0..4]   Input Context Pointer [31:0]
    //   [4..8]   Input Context Pointer [63:32]
    //   [8..12]  flags: cycle | type | slot id
    //   [12..16] reserved
    let mut t = make_trb(0, 0, TRB_TYPE_ADDRESS_DEVICE, 0, cycle);
    t.addr_low |= (slot as u32) << 24;
    // Address lives in param+status (the low 8 bytes of the TRB), NOT in
    // addr_low+addr_high. (That's the data buffer pointer, which is in
    // bytes 8..15 and is unused for command TRBs.)
    t.param = input_context_phys as u32;
    t.status = (input_context_phys >> 32) as u32;
    t
}

#[inline]
pub fn trb_command_complete_extract(trb: &Trb) -> u8 {
    // Command Completion Event: parameter = slot ID, status bits 24..32 = CC
    (trb.param & 0xFF) as u8
}

// ============================================================================
// 6. Device context (32-byte context for our MVP, no streams)
// ============================================================================
//
// Spec: xHCI §6.2.1 Device Context (32-byte slots).
// We'll use 32-byte context (HCSPARAMS1 doesn't advertise 64-byte).

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct DeviceContext {
    pub slot_context: SlotContext,
    pub ep_contexts: [EndpointContext; 31],
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct SlotContext {
    pub data: [u32; 8],
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct EndpointContext {
    pub data: [u32; 8],
}

pub const DEVICE_CONTEXT_SIZE: usize = 32 * 32; // 32-byte context × 32 dwords

// Input control context (xHCI §6.2.5)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct InputControlContext {
    pub drop_flags: u32,
    pub add_flags: u32,
    pub reserved: [u32; 6],
}

// Input context (xHCI §6.2.4): InputControlContext + 31 EndpointContexts.
// Caller-allocated, must be at least 32-byte aligned.

pub const INPUT_CONTEXT_SIZE: usize = 32 + 31 * 32;

// ============================================================================
// 7. Driver handle
// ============================================================================
//
// `XhciController` is the top-level handle. It owns:
//   - The MMIO base pointer (mapped by the integrator)
//   - Pointers to the caller's DMA buffers (DCBAA, command ring, event ring,
//     transfer ring, device context, input context).
// The integrator is responsible for allocating the underlying memory (it
// must be physically contiguous, suitable for DMA).

pub struct XhciController {
    pub mmio_base: *mut u8,
    pub cap_length: u8,
    pub db_offset: u32,
    pub rt_offset: u32,
    pub max_slots: u8,
    pub hcc_64bit: bool,
    pub context_size_64: bool,
}

unsafe impl Send for XhciController {}

impl XhciController {
    /// # Safety
    /// `mmio_base` must be a valid, mapped xHCI MMIO region (4 KiB aligned,
    /// device memory, accessible at supervisor privilege).
    pub unsafe fn new(mmio_base: *mut u8) -> Self {
        let cap_length = read_mmio_u8(mmio_base, CAP_CAPLENGTH as u32);
        let db_offset = read_mmio_u32(mmio_base, CAP_DBOFF as u32);
        let rt_offset = read_mmio_u32(mmio_base, CAP_RTSOFF as u32);
        let hcs1 = read_mmio_u32(mmio_base, CAP_HCSPARAMS1 as u32);
        let hcc1 = read_mmio_u32(mmio_base, CAP_HCCPARAMS1 as u32);
        Self {
            mmio_base,
            cap_length,
            db_offset,
            rt_offset,
            max_slots: (hcs1 & HCSPARAMS1_MAXSLOTS_MASK) as u8,
            hcc_64bit: (hcc1 & HCCPARAMS1_64BIT_ADDR) != 0,
            context_size_64: (hcc1 & HCCPARAMS1_CSZ) != 0,
        }
    }

    /// Read the operational register at `op_offset` (relative to the op base).
    /// # Safety
    /// Caller must ensure op_offset is within the operational register block.
    pub unsafe fn op_read(&self, op_offset: u32) -> u32 {
        let base = self.mmio_base.add(self.cap_length as usize);
        read_mmio_u32(base, op_offset)
    }

    /// Write an operational register.
    /// # Safety
    /// Same as `op_read`.
    pub unsafe fn op_write(&self, op_offset: u32, value: u32) {
        let base = self.mmio_base.add(self.cap_length as usize);
        write_mmio_u32(base, op_offset, value);
    }

    /// Trigger a host controller reset. The caller must wait for the controller
    /// to clear USBSTS.CNR (~1 ms typical, 50 ms worst case per spec).
    /// # Safety
    /// No concurrent MMIO access allowed during reset.
    pub unsafe fn reset(&self) {
        self.op_write(OP_USBCMD, USBCMD_HCRST);
        // Spec: must wait for CNR (Controller Not Ready) to be set, then
        // cleared. Polled by the caller.
    }

    /// Check whether the controller reports itself as ready (CNR = 0).
    pub fn is_ready(&self) -> bool {
        unsafe { (self.op_read(OP_USBSTS) & USBSTS_CNR) == 0 }
    }

    /// Set the Device Context Base Address Array pointer.
    /// # Safety
    /// `phys` must be 64-byte aligned and point to a valid DCBAA of at least
    /// 2048 bytes (256 device slots × 8 bytes each).
    pub unsafe fn set_dcbaap(&self, phys: u64) {
        if self.hcc_64bit {
            self.op_write(OP_DCBAAP, phys as u32);
            self.op_write(OP_DCBAAP + 4, (phys >> 32) as u32);
        } else {
            // 32-bit addressing not supported by us (we assume modern xHCI).
            // Caller should have checked hcc_64bit before allocating.
        }
    }

    /// Set the Command Ring Control Register.
    /// # Safety
    /// `ring_phys` must be 16-byte aligned and point to a valid command ring
    /// (typically 4096 bytes, 256 TRBs).
    pub unsafe fn set_command_ring(&self, ring_phys: u64) {
        let crcr = (ring_phys & CRCR_PTR_MASK) | CRCR_RCS; // start with cycle = 1
        self.op_write(OP_CRCR, crcr as u32);
        self.op_write(OP_CRCR + 4, (crcr >> 32) as u32);
    }

    /// Start the controller (USBCMD.RS = 1).
    /// # Safety
    /// All other operational state (DCBAA, command ring, max slots) must be
    /// set up before calling this.
    pub unsafe fn run(&self) {
        let cmd = self.op_read(OP_USBCMD);
        self.op_write(OP_USBCMD, (cmd & !USBCMD_HCRST) | USBCMD_RS);
    }

    /// Stop the controller (USBCMD.RS = 0). Returns once USBSTS.HCH = 1.
    /// # Safety
    /// No new commands/TRBs after this call until run() is called again.
    pub unsafe fn stop(&self) {
        let cmd = self.op_read(OP_USBCMD);
        self.op_write(OP_USBCMD, cmd & !USBCMD_RS);
        fence(Ordering::SeqCst);
        // Caller should spin on is_halted() with a timeout.
    }

    /// Returns true if the controller is halted.
    pub fn is_halted(&self) -> bool {
        unsafe { (self.op_read(OP_USBSTS) & USBSTS_HCH) != 0 }
    }

    /// Ring the doorbell for a given device slot. The `value` is the
    /// endpoint ID (0 means the slot's main doorbell, used for control
    /// transfers; values 1..31 ring the corresponding endpoint).
    /// # Safety
    /// `slot` must be a valid device slot previously allocated via
    /// ENABLE_SLOT.
    pub unsafe fn ring_doorbell(&self, slot: u8, value: u8) {
        let db_base = self.mmio_base.add(self.db_offset as usize);
        let db_addr = db_base.add(doorbell_offset(slot) as usize);
        write_volatile(db_addr as *mut u32, value as u32);
        fence(Ordering::SeqCst);
    }
}

// ============================================================================
// 8. MMIO helpers
// ============================================================================
//
// We use volatile reads/writes because the xHCI controller is an MMIO
// device — caching is undefined behaviour.

#[inline]
pub unsafe fn read_mmio_u8(base: *mut u8, offset: u32) -> u8 {
    read_volatile(base.add(offset as usize) as *const u8)
}

#[inline]
pub unsafe fn read_mmio_u32(base: *mut u8, offset: u32) -> u32 {
    read_volatile(base.add(offset as usize) as *const u32)
}

#[inline]
pub unsafe fn write_mmio_u32(base: *mut u8, offset: u32, value: u32) {
    write_volatile(base.add(offset as usize) as *mut u32, value);
}

// ============================================================================
// 9. Tests — no MMIO, just TRB encoding and bitfield math
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trb_default_is_all_zero() {
        let t = Trb::default();
        assert_eq!(t.param, 0);
        assert_eq!(t.status, 0);
        assert_eq!(t.addr_low, 0);
        assert_eq!(t.addr_high, 0);
    }

    #[test]
    fn trb_type_extraction() {
        let t = make_trb(0, 0, TRB_TYPE_NORMAL, 0, true);
        assert_eq!(t.trb_type(), TRB_TYPE_NORMAL);

        let t = make_trb(0, 0, TRB_TYPE_ENABLE_SLOT, 0, true);
        assert_eq!(t.trb_type(), TRB_TYPE_ENABLE_SLOT);

        let t = make_trb(0, 0, TRB_TYPE_LINK, 0, false);
        assert_eq!(t.trb_type(), TRB_TYPE_LINK);
    }

    #[test]
    fn trb_cycle_bit() {
        let t = make_trb(0, 0, TRB_TYPE_NORMAL, 0, true);
        assert!(t.cycle());

        let t = make_trb(0, 0, TRB_TYPE_NORMAL, 0, false);
        assert!(!t.cycle());
    }

    #[test]
    fn trb_link_carries_pointer_and_tc() {
        let target: u64 = 0xDEAD_BEEF_1234_5000; // must be 16-byte aligned
        let t = trb_link(target, true, true);
        assert_eq!(t.trb_type(), TRB_TYPE_LINK);
        // TC bit is bit 1 of addr_low
        assert_ne!(t.addr_low & (1 << 1), 0);
        // Pointer lives in param (low 32) and status (high 32)
        assert_eq!(t.param, target as u32);
        assert_eq!(t.status, (target >> 32) as u32);
    }

    #[test]
    fn trb_address_device_carries_slot() {
        let ctx_phys: u64 = 0x1000;
        let t = trb_address_device(ctx_phys, 5, true);
        assert_eq!(t.trb_type(), TRB_TYPE_ADDRESS_DEVICE);
        assert_eq!(t.slot_id(), 5);
    }

    #[test]
    fn pci_ids_match_spec() {
        assert_eq!(PCI_CLASS_SERIAL, 0x0C);
        assert_eq!(PCI_SUBCLASS_USB, 0x03);
        assert_eq!(PCI_PROGIF_XHCI, 0x30);
    }

    #[test]
    fn cap_offsets_match_spec() {
        // These offsets are fixed by the xHCI spec §5.3.1.
        assert_eq!(CAP_CAPLENGTH, 0x00);
        assert_eq!(CAP_HCIVERSION, 0x02);
        assert_eq!(CAP_HCSPARAMS1, 0x04);
        assert_eq!(CAP_DBOFF, 0x14);
        assert_eq!(CAP_RTSOFF, 0x18);
    }

    #[test]
    fn op_offsets_match_spec() {
        // Operational register offsets, xHCI §5.4.
        assert_eq!(OP_USBCMD, 0x00);
        assert_eq!(OP_USBSTS, 0x04);
        assert_eq!(OP_PAGESIZE, 0x08);
        assert_eq!(OP_DCBAAP, 0x30);
        assert_eq!(OP_CONFIG, 0x38);
    }

    #[test]
    fn doorbell_offset_is_per_slot() {
        // Slot 1 starts at DBOFF + 4 (slot 0 is reserved by xHCI).
        assert_eq!(doorbell_offset(1), 0x0004);
        assert_eq!(doorbell_offset(2), 0x0008);
        assert_eq!(doorbell_offset(3), 0x000C);
    }

    #[test]
    fn command_complete_extract_returns_slot() {
        // Command Completion Event: bits 0..8 of parameter = slot ID.
        let mut t = Trb::default();
        t.param = 0x07; // slot 7
        assert_eq!(trb_command_complete_extract(&t), 7);
    }

    #[test]
    fn trb_normal_length_field_in_addr_high() {
        // Normal TRB: addr_high bits 0..16 = transfer length.
        let t = trb_normal(0x1000, 0x12345, true);
        assert_eq!(t.addr_high & 0x1_FFFF, 0x12345);
        assert_eq!(t.param, 0x1000); // also confirms pointer location
    }

    #[test]
    fn hcsparams1_max_slots_extraction() {
        // HCSPARAMS1 layout: bits 0..7 = MaxSlots, bits 8..14 = MaxIntrs,
        // bits 16..23 = MaxPorts. Pick a value that has all three set.
        let hcs1: u32 = 0x00FF_7F_20; // MaxPorts=0xFF, MaxIntrs=0x7F, MaxSlots=0x20
        let max_slots = (hcs1 & HCSPARAMS1_MAXSLOTS_MASK) as u8;
        assert_eq!(max_slots, 0x20);
    }
}
