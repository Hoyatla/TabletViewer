package com.nextos.screenviewer

/**
 * USB implementation of [DebugApi]. Stub for now.
 *
 * Plan (post-MVP):
 *   - The tablet presents itself as a USB device to the PC, exposing a
 *     vendor-specific bulk interface.
 *   - The PC needs a small driver / libusb client that speaks our protocol
 *     and exposes the same HTTP surface locally (e.g. by binding to
 *     127.0.0.1:8765) so the existing [LanDebugApi] can be used unchanged
 *     over USB.
 *   - The Android UsbDeviceConnection + bulkTransfer plumbing is already
 *     demonstrated in the now-removed StreamReceiver.kt.
 *
 * For the MVP, the LAN path is sufficient and what the app uses.
 */
class UsbDebugApi : DebugApi {
    override suspend fun ping(): String =
        throw UnsupportedOperationException("UsbDebugApi not yet implemented")
    override suspend fun system(): SystemInfo =
        throw UnsupportedOperationException("UsbDebugApi not yet implemented")
    override suspend fun screenshot(): ByteArray =
        throw UnsupportedOperationException("UsbDebugApi not yet implemented")
    override suspend fun processes(top: Int, sortBy: SortBy): List<ProcessInfo> =
        throw UnsupportedOperationException("UsbDebugApi not yet implemented")
    override suspend fun logTail(path: String, tail: Int): LogResponse =
        throw UnsupportedOperationException("UsbDebugApi not yet implemented")
    override suspend fun fileRead(path: String): ByteArray =
        throw UnsupportedOperationException("UsbDebugApi not yet implemented")
}
