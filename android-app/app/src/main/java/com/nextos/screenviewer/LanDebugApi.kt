package com.nextos.screenviewer

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONArray
import org.json.JSONObject
import java.net.URLEncoder
import java.util.concurrent.TimeUnit

/**
 * LAN implementation of [DebugApi]. Talks HTTP/JSON to a `pc-agent` running
 * on the target PC, reachable at `http://<ip>:8765`.
 *
 * Threading: every method does its I/O on [Dispatchers.IO] and returns
 * `suspend`-friendly values, so the caller can invoke from the main
 * dispatcher without blocking the UI.
 */
class LanDebugApi(
    private val baseUrl: String,
    private val token: String? = null,
) : DebugApi {

    init {
        require(baseUrl.isNotBlank()) { "baseUrl must not be blank" }
        require(baseUrl.startsWith("http://") || baseUrl.startsWith("https://")) {
            "baseUrl must start with http:// or https:// (got: $baseUrl)"
        }
    }

    private val client: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(5, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(10, TimeUnit.SECONDS)
        .build()

    override suspend fun ping(): String = withContext(Dispatchers.IO) {
        executeText("/v1/ping")
    }

    override suspend fun system(): SystemInfo = withContext(Dispatchers.IO) {
        val obj = executeJson("/v1/system")
        SystemInfo(
            hostname = obj.optString("hostname"),
            os = obj.optString("os"),
            arch = obj.optString("arch"),
            kernel = obj.optStringOrNull("kernel"),
            osVersion = obj.optStringOrNull("os_version"),
            cpuBrand = obj.optString("cpu_brand"),
            cpuCount = obj.optInt("cpu_count"),
            memTotalKb = obj.optLong("mem_total_kb"),
            memAvailKb = obj.optLong("mem_avail_kb"),
            swapTotalKb = obj.optLong("swap_total_kb"),
            swapFreeKb = obj.optLong("swap_free_kb"),
            uptimeS = obj.optLong("uptime_s"),
            bootTimeS = obj.optLong("boot_time_s"),
            loadAvg = parseLoadAvg(obj.optJSONArray("loadavg")),
        )
    }

    override suspend fun screenshot(): ByteArray = withContext(Dispatchers.IO) {
        executeBytes("/v1/screenshot")
    }

    override suspend fun processes(top: Int, sortBy: SortBy): List<ProcessInfo> =
        withContext(Dispatchers.IO) {
            val arr = executeJsonArray("/v1/processes?top=$top&sort=${sortBy.wire}")
            List(arr.length()) { i ->
                val o = arr.getJSONObject(i)
                ProcessInfo(
                    pid = o.optInt("pid"),
                    parentPid = if (o.isNull("parent_pid")) null else o.optInt("parent_pid"),
                    name = o.optString("name"),
                    cmd = o.optString("cmd"),
                    exe = o.optStringOrNull("exe"),
                    cwd = o.optStringOrNull("cwd"),
                    cpuPct = o.optDouble("cpu_pct", 0.0).toFloat(),
                    memKb = o.optLong("mem_kb"),
                    status = o.optString("status"),
                )
            }
        }

    override suspend fun logTail(path: String, tail: Int): LogResponse =
        withContext(Dispatchers.IO) {
            val encoded = URLEncoder.encode(path, "UTF-8")
            val obj = executeJson("/v1/log?path=$encoded&tail=$tail")
            val lines = obj.optJSONArray("lines")
            LogResponse(
                path = obj.optString("path"),
                lines = if (lines == null) emptyList() else List(lines.length()) { lines.getString(it) },
                truncated = obj.optBoolean("truncated"),
            )
        }

    override suspend fun fileRead(path: String): ByteArray = withContext(Dispatchers.IO) {
        val encoded = URLEncoder.encode(path, "UTF-8")
        executeBytes("/v1/file?path=$encoded")
    }

    // ---- HTTP internals ----

    private fun executeText(path: String): String = executeRaw(path).text
    private fun executeJson(path: String): JSONObject = executeRaw(path).let { JSONObject(it.text) }
    private fun executeJsonArray(path: String): JSONArray = executeRaw(path).let { JSONArray(it.text) }
    private fun executeBytes(path: String): ByteArray = executeRaw(path).bytes

    private data class RawResponse(val text: String, val bytes: ByteArray, val contentType: String)

    private fun executeRaw(path: String): RawResponse {
        val url = (baseUrl.trimEnd('/') + path).toHttpUrlOrNull()
            ?: throw IllegalArgumentException("invalid url: $baseUrl$path")
        val req = Request.Builder()
            .url(url)
            .apply { if (token != null) header("Authorization", "Bearer $token") }
            .get()
            .build()
        client.newCall(req).execute().use { resp ->
            if (!resp.isSuccessful) {
                val errBody = resp.body?.string().orEmpty()
                throw RuntimeException("HTTP ${resp.code}: $errBody")
            }
            val body = resp.body ?: throw RuntimeException("empty body")
            val bytes = body.bytes()
            val ct = (resp.header("Content-Type") ?: "").lowercase()
            return RawResponse(text = String(bytes), bytes = bytes, contentType = ct)
        }
    }

    private fun parseLoadAvg(arr: JSONArray?): Triple<Double, Double, Double> {
        if (arr == null || arr.length() < 3) return Triple(0.0, 0.0, 0.0)
        return Triple(arr.getDouble(0), arr.getDouble(1), arr.getDouble(2))
    }
}

private fun JSONObject.optStringOrNull(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf { it.isNotEmpty() }
