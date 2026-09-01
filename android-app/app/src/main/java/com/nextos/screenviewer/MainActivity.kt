package com.nextos.screenviewer

import android.graphics.BitmapFactory
import android.os.Bundle
import android.text.InputType
import android.util.Log
import android.view.View
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.Spinner
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Main entry point. Hosts a small UI to talk to a `pc-agent` either over
 * the LAN (auto-discovered or manual URL) or over USB (accessory mode +
 * a PC-side bridge binary).
 *
 *  - Pick a transport in the spinner (LAN / USB).
 *  - LAN: optionally tap "Discover" to mDNS-browse for `_pcagent._tcp.`,
 *    then tap "Connect" to ping + fetch system info. The URL field can
 *    also be edited manually.
 *  - USB: tap "Connect" to request the accessory permission and open the
 *    bridge. Without the PC bridge running, all calls except `ping` will
 *    throw — the bridge is what forwards bulk ↔ localhost:8766.
 *  - Action buttons invoke the matching endpoint on whichever API is
 *    currently connected. Screenshots appear in the ImageView; other
 *    results print to the text area.
 */
class MainActivity : AppCompatActivity() {

    private lateinit var transportSpinner: Spinner
    private lateinit var discoverButton: Button
    private lateinit var urlInput: EditText
    private lateinit var statusText: TextView
    private lateinit var outputText: TextView
    private lateinit var screenshotView: ImageView

    private val usbApi by lazy { UsbDebugApi(this) }

    private var api: DebugApi? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        transportSpinner = findViewById(R.id.transport_spinner)
        discoverButton = findViewById(R.id.btn_discover)
        urlInput = findViewById(R.id.url_input)
        statusText = findViewById(R.id.status_text)
        outputText = findViewById(R.id.output_text)
        screenshotView = findViewById(R.id.screenshot_view)

        // Transport selector.
        val transports = arrayOf("LAN (auto-discover or type URL)", "USB (accessory)")
        transportSpinner.adapter = ArrayAdapter(
            this, android.R.layout.simple_spinner_dropdown_item, transports,
        )

        // Wire buttons.
        findViewById<Button>(R.id.btn_connect).setOnClickListener { onConnect() }
        findViewById<Button>(R.id.btn_system).setOnClickListener { onSystem() }
        findViewById<Button>(R.id.btn_processes).setOnClickListener { onProcesses() }
        findViewById<Button>(R.id.btn_screenshot).setOnClickListener { onScreenshot() }
        findViewById<Button>(R.id.btn_log).setOnClickListener { onLog() }
        findViewById<Button>(R.id.btn_file).setOnClickListener { onFile() }
        discoverButton.setOnClickListener { onDiscover() }

        // Sensible default. The PC IP will vary by network; we use the
        // address Discover would auto-fill, but Discover is still the
        // preferred path (mDNS handles IP changes).
        urlInput.setText("http://192.168.1.103:8765")
        updateStatus("Not connected. Pick a transport and tap Connect.")
        setActionsEnabled(false)
    }

    override fun onDestroy() {
        super.onDestroy()
        runCatching { usbApi.close() }
    }

    private fun onConnect() {
        when (transportSpinner.selectedItemPosition) {
            0 -> connectLan()
            1 -> connectUsb()
            else -> Toast.makeText(this, "Unknown transport", Toast.LENGTH_SHORT).show()
        }
    }

    private fun connectLan() {
        val url = urlInput.text.toString().trim()
        if (url.isEmpty()) {
            Toast.makeText(this, "URL is required", Toast.LENGTH_SHORT).show()
            return
        }
        val newApi = LanDebugApi(url)
        api = newApi
        updateStatus("Connecting to $url ...")
        setActionsEnabled(false)
        lifecycleScope.launch {
            try {
                val pong = newApi.ping()
                val sys = newApi.system()
                updateStatus(
                    "Connected. $pong\n" +
                        "Host: ${sys.hostname}  OS: ${sys.os} ${sys.arch}  " +
                        "CPU: ${sys.cpuCount}× ${sys.cpuBrand}\n" +
                        "Mem: ${sys.memAvailKb / 1024} MB free / ${sys.memTotalKb / 1024} MB total  " +
                        "Uptime: ${sys.uptimeS}s"
                )
                setActionsEnabled(true)
            } catch (e: Exception) {
                Log.e(TAG, "connect failed", e)
                api = null
                updateStatus("Connect failed: ${e.message ?: e::class.simpleName}")
            }
        }
    }

    private fun connectUsb() {
        updateStatus("Opening USB accessory...")
        setActionsEnabled(false)
        try {
            usbApi.openIfNeeded()
            api = usbApi
            updateStatus(
                "USB accessory open. Only ping() works without the PC bridge " +
                    "(see docs/USB.md)."
            )
            // Probe with a ping so the user gets immediate feedback.
            lifecycleScope.launch {
                try {
                    val pong = usbApi.ping()
                    updateStatus("USB: $pong")
                    setActionsEnabled(true)
                } catch (e: Exception) {
                    Log.e(TAG, "USB ping failed", e)
                    api = null
                    updateStatus("USB ping failed: ${e.message ?: e::class.simpleName}")
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "USB open failed", e)
            api = null
            updateStatus("USB open failed: ${e.message ?: e::class.simpleName}")
        }
    }

    private fun onDiscover() {
        updateStatus("Discovering pc-agent instances on the LAN...")
        discoverButton.isEnabled = false
        lifecycleScope.launch {
            try {
                val agents = NetworkDiscovery.discover(this@MainActivity, timeoutMs = 4000)
                if (agents.isEmpty()) {
                    updateStatus("No pc-agent found. You can still type the URL manually.")
                } else {
                    val first = agents.first()
                    transportSpinner.setSelection(0) // LAN
                    urlInput.setText(first.baseUrl)
                    val others = if (agents.size > 1) " (+${agents.size - 1} more)" else ""
                    updateStatus(
                        "Found ${agents.size} agent(s)$others. " +
                            "Using ${first.name} → ${first.baseUrl}. Tap Connect."
                    )
                }
            } catch (e: Exception) {
                Log.e(TAG, "discover failed", e)
                updateStatus("Discover failed: ${e.message ?: e::class.simpleName}")
            } finally {
                discoverButton.isEnabled = true
            }
        }
    }

    private fun onSystem() = withApi("system") { api ->
        val sys = api.system()
        val sb = StringBuilder()
        sb.appendLine("Host:     ${sys.hostname}")
        sb.appendLine("OS:       ${sys.os} ${sys.arch} (${sys.osVersion ?: "?"})")
        sb.appendLine("Kernel:   ${sys.kernel ?: "?"}")
        sb.appendLine("CPU:      ${sys.cpuCount} × ${sys.cpuBrand}")
        sb.appendLine("Memory:   ${sys.memAvailKb / 1024} MB free / ${sys.memTotalKb / 1024} MB total")
        sb.appendLine("Swap:     ${sys.swapFreeKb / 1024} MB free / ${sys.swapTotalKb / 1024} MB total")
        sb.appendLine("Uptime:   ${sys.uptimeS} s  (boot ${sys.bootTimeS})")
        sb.appendLine("Load:     ${"%.2f".format(sys.loadAvg.first)}  " +
            "${"%.2f".format(sys.loadAvg.second)}  ${"%.2f".format(sys.loadAvg.third)}")
        showText(sb.toString())
    }

    private fun onProcesses() = withApi("processes") { api ->
        val procs = api.processes(top = 20, sortBy = SortBy.CPU)
        val sb = StringBuilder("Top ${procs.size} processes by CPU:\n")
        procs.forEach { p ->
            sb.appendLine(
                "  %5d  %5.1f%%  %6d MB  %-20s  %s".format(
                    p.pid, p.cpuPct, p.memKb / 1024,
                    p.name.take(20),
                    p.cmd.take(60).replace("\n", " "),
                )
            )
        }
        showText(sb.toString())
    }

    private fun onScreenshot() = withApi("screenshot") { api ->
        val png = api.screenshot()
        showText("Screenshot: ${png.size} bytes")
        withContext(Dispatchers.Main) {
            val bmp = BitmapFactory.decodeByteArray(png, 0, png.size)
            if (bmp != null) {
                screenshotView.setImageBitmap(bmp)
                screenshotView.visibility = View.VISIBLE
            } else {
                appendText(" (could not decode PNG)")
            }
        }
    }

    private fun onLog() {
        val path = promptForPath("Tail log file", default = "C:/Windows/System32/drivers/etc/hosts")
            ?: return
        withApi("log") { api ->
            val r = api.logTail(path, tail = 50)
            val sb = StringBuilder()
            sb.appendLine("Log: ${r.path}  (${r.lines.size} lines, truncated=${r.truncated})")
            r.lines.forEach { sb.appendLine("  $it") }
            showText(sb.toString())
        }
    }

    private fun onFile() {
        val path = promptForPath("Read file", default = "C:/Windows/System32/drivers/etc/hosts")
            ?: return
        withApi("file") { api ->
            val bytes = api.fileRead(path)
            showText("File: $path  (${bytes.size} bytes)\n\n${String(bytes)}")
        }
    }

    // ---- helpers ----

    private fun withApi(label: String, block: suspend (DebugApi) -> Unit) {
        val a = api ?: run {
            updateStatus("Not connected.")
            return
        }
        lifecycleScope.launch {
            try {
                block(a)
            } catch (e: Exception) {
                Log.e(TAG, "$label failed", e)
                appendText("ERR: ${e.message ?: e::class.simpleName}\n")
            }
        }
    }

    private fun showText(s: String) = runOnUiThread {
        outputText.text = s
        screenshotView.visibility = View.GONE
    }

    private fun appendText(s: String) = runOnUiThread {
        outputText.append(s)
    }

    private fun updateStatus(s: String) = runOnUiThread {
        statusText.text = s
    }

    private fun setActionsEnabled(enabled: Boolean) {
        listOf(
            R.id.btn_system, R.id.btn_processes, R.id.btn_screenshot,
            R.id.btn_log, R.id.btn_file,
        ).forEach { id ->
            findViewById<Button>(id).isEnabled = enabled
        }
    }

    private fun promptForPath(title: String, default: String): String? {
        val input = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT
            setText(default)
            setSelection(text.length)
        }
        val padding = (16 * resources.displayMetrics.density).toInt()
        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(padding, padding / 2, padding, 0)
            addView(input)
        }
        val dialog = android.app.AlertDialog.Builder(this)
            .setTitle(title)
            .setView(container)
            .setPositiveButton("OK") { _, _ ->
                input.tag = input.text.toString()
            }
            .setNegativeButton("Cancel", null)
            .create()
        dialog.show()
        val result = input.tag as? String
        return if (result.isNullOrBlank()) null else result
    }

    private companion object {
        const val TAG = "MainActivity"
    }
}
