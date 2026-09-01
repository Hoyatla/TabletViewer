package com.nextos.screenviewer

import android.app.Activity
import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import android.widget.Toast

/**
 * Main entry point. Hosts:
 *  - Mode 1: connect to the NexTOS USB device, read incoming frames, show
 *    metadata (resolution, frame id, format) in the status area.
 *  - Mode 2: trigger a self-capture of the tablet's own screen (Phase 4).
 */
class MainActivity : Activity() {

    private lateinit var statusText: TextView
    private lateinit var connectButton: Button
    private lateinit var captureButton: Button

    private var receiver: StreamReceiver? = null
    private var connected = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        statusText    = findViewById(R.id.status_text)
        connectButton = findViewById(R.id.connect_button)
        captureButton = findViewById(R.id.capture_button)

        connectButton.setOnClickListener { onConnectClicked() }
        captureButton.setOnClickListener { onCaptureClicked() }

        updateStatus("Ready. Plug the NexTOS device and tap Connect.")
    }

    private fun onConnectClicked() {
        if (connected) {
            disconnect()
            return
        }
        val r = StreamReceiver(this)
        r.onFrame = { frame ->
            // Worker thread — jump back to UI.
            runOnUiThread {
                updateStatus(
                    "frame #${frame.frameId}  ${frame.width}×${frame.height}  " +
                        "fmt=${formatName(frame.format)}  " +
                        "key=${frame.isKeyFrame}  " +
                        "payload=${frame.payload.size} B"
                )
            }
        }
        r.onError = { err ->
            runOnUiThread { updateStatus("ERR: ${err.message ?: err::class.simpleName}") }
        }
        r.onConnected = {
            runOnUiThread {
                connected = true
                connectButton.text = getString(R.string.disconnect)
                updateStatus("Connected. Waiting for first frame…")
            }
        }
        r.onDisconnected = {
            runOnUiThread {
                connected = false
                connectButton.text = getString(R.string.connect)
            }
        }
        try {
            r.start()
            receiver = r
        } catch (e: Throwable) {
            updateStatus("Connect failed: ${e.message ?: e::class.simpleName}")
        }
    }

    private fun disconnect() {
        receiver?.stop()
        receiver = null
        connected = false
        connectButton.text = getString(R.string.connect)
        updateStatus("Disconnected.")
    }

    private fun onCaptureClicked() {
        // TODO: Phase 4 — mmap /dev/graphics/fb0 via su, save to /captures/.
        Toast.makeText(this, R.string.capture_not_yet, Toast.LENGTH_SHORT).show()
    }

    private fun updateStatus(text: String) {
        statusText.text = text
    }

    private fun formatName(fmt: Int): String = when (fmt) {
        ProtocolParser.FORMAT_RGB565 -> "RGB565"
        ProtocolParser.FORMAT_RGBA32 -> "RGBA32"
        ProtocolParser.FORMAT_JPEG   -> "JPEG"
        else -> "?"
    }

    override fun onDestroy() {
        receiver?.stop()
        super.onDestroy()
    }
}
