//! USB device descriptor constants for the tablet-side device we present
//! (when running in device mode for testing) or for the PC-side enumeration
//! (when matching a remote tablet).

/// USB-IF test VID. Acceptable for hobbyist use, prohibited for commercial
/// sale. See https://pid.codes for details.
pub const VENDOR_ID: u16 = 0x1209;
pub const PRODUCT_ID: u16 = 0x0001;
pub const MANUFACTURER: &str = "NexTOS";
pub const PRODUCT: &str = "ScreenStream";
pub const BCD_DEVICE: u16 = 0x0100;
