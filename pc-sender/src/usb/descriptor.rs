//! USB device descriptor constants.
//!
//! These describe the **tablet-side** device (which presents itself to the PC
//! when the tablet is in device mode). On a stock Android tablet the OS does
//! not put the device's USB controller in device mode automatically — that
//! requires the companion Android app to claim the USB role via
//! `UsbManager.setDeviceRole` or similar (API 30+).
//!
//! In our current pipeline:
//!   - VID = 0x1209 (pid.codes test range, free for hobbyist use)
//!   - PID = 0x0001 (placeholder)
//!   - Class = 0xFF (vendor-specific, no standard driver needed on PC)
//!   - bcdDevice = 0x0100 (matches PROTOCOL_VERSION in enc.rs)
//!
//! If you ship this commercially, obtain a real USB-IF VID.
//! See https://pid.codes/ for the test VID rules.

pub const VENDOR_ID: u16 = 0x1209;
pub const PRODUCT_ID: u16 = 0x0001;
pub const MANUFACTURER: &str = "NexTOS";
pub const PRODUCT: &str = "ScreenStream";
pub const BCD_DEVICE: u16 = 0x0100;

// Standard USB descriptor type codes (USB 2.0 spec §9.4).
pub const DESC_TYPE_DEVICE: u8 = 1;
pub const DESC_TYPE_CONFIGURATION: u8 = 2;
pub const DESC_TYPE_STRING: u8 = 3;
pub const DESC_TYPE_INTERFACE: u8 = 4;
pub const DESC_TYPE_ENDPOINT: u8 = 5;
pub const DESC_TYPE_DEVICE_QUALIFIER: u8 = 6;
pub const DESC_TYPE_OTHER_SPEED_CONFIG: u8 = 7;
pub const DESC_TYPE_INTERFACE_POWER: u8 = 8;
pub const DESC_TYPE_OTG: u8 = 9;
pub const DESC_TYPE_DEBUG: u8 = 10;
pub const DESC_TYPE_INTERFACE_ASSOCIATION: u8 = 11;
pub const DESC_TYPE_BOS: u8 = 15;
pub const DESC_TYPE_DEVICE_CAPABILITY: u8 = 16;

// Standard endpoint attributes (USB 2.0 spec §9.6.6 Endpoint Descriptor).
pub const EP_ATTR_CONTROL: u8 = 0x00;
pub const EP_ATTR_ISOCHRONOUS: u8 = 0x01;
pub const EP_ATTR_BULK: u8 = 0x02;
pub const EP_ATTR_INTERRUPT: u8 = 0x03;

// Endpoint directions.
pub const EP_DIR_OUT: u8 = 0x00; // Host → Device
pub const EP_DIR_IN: u8 = 0x80;  // Device → Host

// Standard bEndpointAddress encoding: bit 7 = direction, bits 0..3 = number.
pub const EP_ADDR_OUT: u8 = 0x01; // EP 1 OUT
pub const EP_ADDR_IN: u8 = 0x81;  // EP 1 IN

// bmAttributes packed for a bulk IN endpoint: bulk (0x02), no sync, no usage.
pub const EP_BULK_IN_ATTR: u8 = EP_ATTR_BULK | 0x00; // bulk, no sync/usage
pub const EP_BULK_OUT_ATTR: u8 = EP_ATTR_BULK | 0x00;
