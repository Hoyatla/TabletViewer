package com.nextos.screenviewer

import android.graphics.BitmapFactory
import android.os.Bundle
import android.text.InputType
import android.util.Log
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Main entry point. Hosts a small UI to talk to a `pc-agent` over LAN:
 *   - Enter the PC's URL (e.g. http://192.168.1.42:8765).
 *   - Tap "Connect" to ping + fetch system info.
 *   - Tap the action buttons to invoke the matching endpoint.
 *   - Screenshots appear in the ImageView; other results print to the
 *     text area.
 */
class MainActivity : AppCompatActivity() {

    private lateinit var urlInput: EditText
    private lateinit var statusText: TextView
    private lateinit var outputText: TextView
    private lateinit var screenshotView: ImageView
    private lateinit var actionBar: LinearLayout

    private var api: DebugApi? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        urlInput = findViewById(R.id.url_input)
        statusText = findViewById(R.id.status_text)
        outputText = findViewById(R.id.output_text)
        screenshotView = findViewById(R.id.screenshot_view)
        actionBar = findViewById(R.id.action_bar)

        // Wire buttons.
        findViewById<Button>(R.id.btn_connect).setOnClickListener { onConnect() }
        findViewById<Button>(R.id.btn_system).setOnClickListener { onSystem() }
        findViewById<Button>(R.id.btn_processes).setOnClickListener { onProcesses() }
        findViewById<Button>(R.id.btn_screenshot).setOnClickListener { onScreenshot() }
        findViewById<Button>(R.id.btn_log).setOnClickListener { onLog() }
        findViewById<Button>(R.id.btn_file).setOnClickListener { onFile() }

        // Sensible default: the agent defaults to 127.0.0.1:8765 (PC mode)
        // or whatever is on the LAN. User can edit.
        urlInput.setText("http://192.168.1.10:8765")
        updateStatus("Not connected. Enter the PC agent URL and tap Connect.")
        setActionsEnabled(false)
    }

    private fun onConnect() {
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
        // Wait for the user to dismiss — but since dialog is modal, we
        // can't await here. Workaround: ask the user, then read the
        // tag on next call. Simpler: use a blocking approach.
        val result = input.tag as? String
        return if (result.isNullOrBlank()) null else result
    }

    private companion object {
        const val TAG = "MainActivity"
    }
}
