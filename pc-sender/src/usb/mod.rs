//! USB host stack (xHCI + bulk transfer).
//!
//! Stubs: see `xhci.rs` and `bulk.rs`. Real implementation in
//! `docs/ROADMAP.md` Phase 2.

pub mod bulk;
pub mod descriptor;
pub mod xhci;
