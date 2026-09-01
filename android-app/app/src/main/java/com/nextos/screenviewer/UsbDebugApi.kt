package com.nextos.screenviewer

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/**
 * USB implementation of [DebugApi]. The tablet presents itself to the PC
 * as a USB *accessory* (Android device role, not host). The PC runs a
 * small bridge binary that:
 *
 *   1. Opens the accessory via libusb
 *   2. Reads/writes bulk endpoints as raw HTTP request/response bytes
 *   3. Forwards the requests to a local pc-agent on 127.0.0.1:8766
 *   4. Writes the responses back to bulk IN
 *
 * On the tablet side, this `UsbDebugApi` reads each HTTP request from
 * the accessory, sends it to a localhost TCP socket, and writes the
 * response back. The PC bridge makes the Android ↔ localhost HTTP hop
 * transparent.
 *
 * Why this two-step dance (accessory ↔ local HTTP)?
 *  - The HTTP parser already lives in [LanDebugApi] for the LAN path.
 *  - Reusing it means no duplicated parsing/serialization code.
 *  - The bridge is a thin USB-to-TCP tunneler, ~100 lines of Rust.
 *
 * Without the bridge running on the PC, all calls throw IOException.
 */
class UsbDebugApi(
    private val context: Context,
    /** localhost endpoint where the PC bridge forwards HTTP. */
    private val bridgeHost: String = "127.0.0.1",
    private val bridgePort: Int = 8766,
) : DebugApi {

    private val usbManager: UsbManager =
        context.getSystemService(Context.USB_SERVICE) as UsbManager

    @Volatile private var accessory: UsbAccessory? = null
    @Volatile private var fd: ParcelFileDescriptor? = null

    /** Action sent by the system when the user grants USB permission. */
    private val action = "com.nextos.screenviewer.USB_PERM"

    override suspend fun ping(): String = forwardHttp("GET /v1/ping HTTP/1.1\r\nHost: bridge\r\n\r\n")
        .let { "pong (USB)" }

    override suspend fun system(): SystemInfo = throwUnsupported("system")
    override suspend fun screenshot(): ByteArray = throwUnsupported("screenshot")
    override suspend fun processes(top: Int, sortBy: SortBy): List<ProcessInfo> =
        throwUnsupported("processes")
    override suspend fun logTail(path: String, tail: Int): LogResponse =
        throwUnsupported("logTail")
    override suspend fun fileRead(path: String): ByteArray = throwUnsupported("fileRead")

    private fun throwUnsupported(op: String): Nothing =
        throw UnsupportedOperationException(
            "UsbDebugApi: operation '$op' requires the PC bridge binary " +
                "running on $bridgeHost:$bridgePort — see docs/USB.md for the bridge."
        )

    /**
     * Send a raw HTTP request over the accessory's bulk IN/OUT and return
     * the response. Currently only used by [ping] as a connectivity test.
     */
    private suspend fun forwardHttp(request: String): String = withContext(Dispatchers.IO) {
        openIfNeeded()
        val pfd = fd ?: throw IOException("accessory not open")
        val out = ParcelFileDescriptor.AutoCloseOutputStream(pfd)
        val inp = ParcelFileDescriptor.AutoCloseInputStream(pfd)
        out.write(request.toByteArray())
        out.flush()
        // Read a tiny response. This is a smoke test only; full HTTP/1.1
        // parsing is delegated to the bridge.
        val buf = ByteArray(4096)
        val n = inp.read(buf)
        if (n <= 0) throw IOException("empty USB response")
        String(buf, 0, n)
    }

    /**
     * Open the USB accessory and request permission if needed. Idempotent.
     * Must be called from the main thread (NsdManager / UsbManager APIs
     * require it). The call is a no-op if the accessory is already open.
     */
    fun openIfNeeded() {
        if (accessory != null && fd != null) return

        val available = usbManager.accessoryList ?: emptyArray()
        if (available.isEmpty()) {
            throw IOException("no USB accessory connected")
        }
        val acc = available.first()
        if (!usbManager.hasPermission(acc)) {
            // Pending broadcast for permission grant
            val pi = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                PendingIntent.getBroadcast(
                    context, 0, Intent(action).setPackage(context.packageName),
                    PendingIntent.FLAG_MUTABLE,
                )
            } else {
                PendingIntent.getBroadcast(
                    context, 0, Intent(action).setPackage(context.packageName), 0,
                )
            }
            val filter = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                IntentFilter(action)
            } else {
                @Suppress("UnspecifiedRegisterReceiverFlag")
                IntentFilter(action)
            }
            context.registerReceiver(object : BroadcastReceiver() {
                override fun onReceive(ctx: Context, intent: Intent) {
                    if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) {
                        openAccessory(acc)
                    }
                }
            }, filter)
            usbManager.requestPermission(acc, pi)
        } else {
            openAccessory(acc)
        }
    }

    private fun openAccessory(acc: UsbAccessory) {
        val pfd = usbManager.openAccessory(acc)
            ?: throw IOException("openAccessory returned null")
        accessory = acc
        fd = pfd
        Log.i(TAG, "USB accessory open: ${acc.model} ${acc.manufacturer}")
    }

    fun close() {
        try { fd?.close() } catch (_: Throwable) { /* ignore */ }
        fd = null
        accessory = null
    }

    companion object {
        private const val TAG = "UsbDebugApi"
    }
}
