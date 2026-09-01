//! USB host stack (xHCI + bulk transfer).
//!
//! Phase 2 of the project. See `xhci.rs` for the controller init and
//! register layout, `bulk.rs` for the transfer-ring submit/wait primitive,
//! and `descriptor.rs` for the device descriptor constants.

pub mod bulk;
pub mod descriptor;
pub mod xhci;

// Re-export the most common types at the module level for ergonomics.
pub use bulk::{
    BulkEndpoint, TransferRing, TRANSFER_RING_LEN,
    completion_code, completion_label, is_success,
};
pub use descriptor::{VENDOR_ID, PRODUCT_ID, MANUFACTURER, PRODUCT, BCD_DEVICE};
pub use xhci::{
    XhciController,
    Trb, DeviceContext, SlotContext, EndpointContext, InputControlContext,
    trb_link, trb_noop, trb_enable_slot, trb_address_device,
    // Constants
    TRB_TYPE_NORMAL, CC_SUCCESS, CC_SHORT_PACKET, CC_STALL,
    DEVICE_CONTEXT_SIZE, INPUT_CONTEXT_SIZE,
};
