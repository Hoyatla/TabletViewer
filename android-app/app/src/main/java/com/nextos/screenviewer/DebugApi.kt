package com.nextos.screenviewer

/**
 * Public API for the debug client. Hides the transport (LAN, USB, anything
 * else) behind a single interface. The app's UI talks to this; the rest is
 * the network layer.
 *
 * All methods are `suspend` because the underlying transport is async.
 */
interface DebugApi {
    /** Liveness check. Returns "pong" if the server is reachable. */
    suspend fun ping(): String

    /** Full system info. */
    suspend fun system(): SystemInfo

    /** Capture the current screen as a PNG. */
    suspend fun screenshot(): ByteArray

    /** Top-N processes sorted by the chosen column. */
    suspend fun processes(top: Int, sortBy: SortBy): List<ProcessInfo>

    /** Tail a log file. */
    suspend fun logTail(path: String, tail: Int): LogResponse

    /** Read a file in full. */
    suspend fun fileRead(path: String): ByteArray
}

enum class SortBy(val wire: String) {
    CPU("cpu"),
    MEMORY("mem");

    companion object {
        fun fromWire(s: String): SortBy = entries.firstOrNull { it.wire == s } ?: CPU
    }
}
