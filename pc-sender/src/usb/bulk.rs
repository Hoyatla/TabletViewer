//! USB bulk transfer — stub.

/// A bulk OUT endpoint (PC → tablet).
pub struct BulkEndpoint {
    pub endpoint_addr: u8,
    pub max_packet_size: u16,
}

/// Send `data` over the bulk endpoint. Returns the number of bytes transferred
/// or an error string.
pub fn send(_ep: &BulkEndpoint, _data: &[u8]) -> Result<usize, &'static str> {
    // TODO: ring TRB submit, doorbell ring, wait for completion event TRB.
    Err("bulk::send not yet implemented")
}
