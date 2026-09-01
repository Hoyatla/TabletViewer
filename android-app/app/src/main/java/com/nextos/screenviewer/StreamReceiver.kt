package com.nextos.screenviewer

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log

/**
 * Mode 1 — USB bulk IN receiver.
 *
 * Opens a USB device matching the NexTOS ScreenStream descriptors
 * (VID `0x1209`, PID `0x0001`), reads bulk packets, decodes them with
 * [ProtocolParser] and dispatches each [Frame] to [onFrame].
 *
 * Threading:
 *   - [start] is called on the main thread; it returns once permission has
 *     been requested (or the device is already open).
 *   - The blocking read loop runs on a dedicated worker thread.
 *   - [onFrame] and [onError] are invoked on the worker thread; the caller
 *     is responsible for marshalling back to the UI thread.
 */
class StreamReceiver(private val context: Context) {

    /** Invoked for every successfully decoded frame. */
    var onFrame: ((Frame) -> Unit)? = null

    /** Invoked on read errors, malformed packets, permission denial, etc. */
    var onError: ((Throwable) -> Unit)? = null

    /** Invoked when a stream is successfully opened. */
    var onConnected: (() -> Unit)? = null

    /** Invoked when the stream is closed (intentionally or not). */
    var onDisconnected: (() -> Unit)? = null

    private val usbManager: UsbManager =
        context.getSystemService(Context.USB_SERVICE) as UsbManager

    private var device: UsbDevice? = null
    private var connection: UsbDeviceConnection? = null
    private var endpointIn: UsbEndpoint? = null
    private var readThread: Thread? = null

    @Volatile private var running = false

    /**
     * Find the NexTOS device, request permission, and start reading.
     * Throws if the device is not plugged in or if open fails.
     */
    fun start() {
        val dev = usbManager.deviceList.values.firstOrNull {
            it.vendorId == VENDOR_ID && it.productId == PRODUCT_ID
        } ?: throw IllegalStateException(
            "NexTOS ScreenStream device not found (VID=0x${VENDOR_ID.toString(16)}, " +
                "PID=0x${PRODUCT_ID.toString(16)}). Plug it in."
        )

        if (!usbManager.hasPermission(dev)) {
            requestPermission(dev)
            return
        }
        openAndRead(dev)
    }

    fun stop() {
        running = false
        readThread?.interrupt()
        readThread = null
        try {
            connection?.close()
        } catch (_: Throwable) { /* ignore */ }
        connection = null
        device = null
        endpointIn = null
        try {
            context.unregisterReceiver(permissionReceiver)
        } catch (_: Throwable) { /* not registered */ }
        onDisconnected?.invoke()
    }

    private fun openAndRead(dev: UsbDevice) {
        val iface = dev.getInterface(0)
        // Find the first bulk IN endpoint.
        val ep = (0 until iface.endpointCount)
            .map { iface.getEndpoint(it) }
            .firstOrNull { it.type == android.hardware.usb.UsbConstants.USB_ENDPOINT_XFER_BULK &&
                it.direction == android.hardware.usb.UsbConstants.USB_DIR_IN }
            ?: throw IllegalStateException("No bulk IN endpoint on interface 0")

        val conn = usbManager.openDevice(dev)
            ?: throw IllegalStateException("usbManager.openDevice returned null")
        if (!conn.claimInterface(iface, true)) {
            conn.close()
            throw IllegalStateException("claimInterface failed")
        }

        device = dev
        connection = conn
        endpointIn = ep
        running = true

        readThread = Thread({ readLoop() }, "StreamReceiver-Read").also { it.start() }
        onConnected?.invoke()
    }

    private fun readLoop() {
        val ep = endpointIn ?: return
        val conn = connection ?: return

        // Buffer sized for the worst-case packet:
        //   header (24) + max payload (1920*1080*4 bytes for RGBA32).
        val buf = ByteArray(HEADER_MAX_BYTES)
        var pos = 0

        while (running) {
            if (pos < ProtocolParser.HEADER_LEN) {
                // Read just enough to complete the header.
                val need = ProtocolParser.HEADER_LEN - pos
                val n = conn.bulkTransfer(ep, buf, pos, need, READ_TIMEOUT_MS)
                if (n <= 0) {
                    if (n < 0) {
                        // timeout or transient error — loop
                        try { Thread.sleep(5) } catch (_: InterruptedException) { return }
                    }
                    continue
                }
                pos += n
                continue
            }

            // Header complete. Determine payload length.
            val payloadLen = ProtocolParser.peekPayloadLength(buf)
            val total = ProtocolParser.HEADER_LEN + payloadLen
            if (total > buf.size) {
                Log.e(TAG, "Payload too big ($payloadLen bytes), resyncing on next NTSS")
                resyncToMagic(conn, ep, buf)
                pos = 0
                continue
            }

            if (pos < total) {
                val need = total - pos
                val n: Int = conn.bulkTransfer(ep, buf, pos, need, READ_TIMEOUT_MS)
                if (n <= 0) {
                    if (n < 0) {
                        try { Thread.sleep(5) } catch (_: InterruptedException) { return }
                    }
                    continue
                }
                pos += n
                continue
            }

            // Full packet in buffer. Decode.
            val packet = buf.copyOfRange(0, total)
            val frame = ProtocolParser.decode(packet)
            if (frame != null) {
                onFrame?.invoke(frame)
            } else {
                onError?.invoke(IllegalStateException("Malformed packet, dropping"))
            }
            pos = 0
        }
    }

    private fun resyncToMagic(conn: UsbDeviceConnection, ep: UsbEndpoint, buf: ByteArray) {
        // Read 1 byte at a time until we see NTSS. Drop garbage.
        // This is a slow path; only used on bad packets.
        while (running) {
            val n = conn.bulkTransfer(ep, buf, 0, 1, READ_TIMEOUT_MS)
            if (n == 1 && buf[0] == ProtocolParser.MAGIC[0]) return
        }
    }

    private val permissionReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            if (intent.action != ACTION_USB_PERMISSION) return
            val dev: UsbDevice =
                intent.getParcelableExtra(UsbManager.EXTRA_DEVICE) ?: return
            if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) {
                try {
                    openAndRead(dev)
                } catch (e: Throwable) {
                    onError?.invoke(e)
                }
            } else {
                onError?.invoke(SecurityException("USB permission denied by user"))
            }
        }
    }

    private fun requestPermission(dev: UsbDevice) {
        val intent = Intent(ACTION_USB_PERMISSION).apply {
            setPackage(context.packageName)
        }
        val pi = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_MUTABLE)
        } else {
            PendingIntent.getBroadcast(context, 0, intent, 0)
        }
        val filter = IntentFilter(ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(permissionReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            context.registerReceiver(permissionReceiver, filter)
        }
        usbManager.requestPermission(dev, pi)
    }

    companion object {
        private const val TAG = "StreamReceiver"
        private const val ACTION_USB_PERMISSION = "com.nextos.screenviewer.USB_PERMISSION"
        private const val VENDOR_ID = 0x1209   // pid.codes test VID
        private const val PRODUCT_ID = 0x0001
        private const val READ_TIMEOUT_MS = 1000
        private const val HEADER_MAX_BYTES = 24 + 1920 * 1080 * 4  // header + worst-case RGBA32
    }
}
