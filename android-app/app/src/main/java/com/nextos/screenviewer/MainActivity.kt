package com.nextos.screenviewer

import android.app.Activity
import android.os.Bundle

/**
 * Main entry point.
 *
 * Hosts:
 *  - Mode 1 UI: connect to USB device, start StreamReceiver, view live stream.
 *  - Mode 2 UI: trigger ScreenCapture, save PNG.
 */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // TODO: inflate layout, wire StreamReceiver + ScreenCapture.
    }
}
