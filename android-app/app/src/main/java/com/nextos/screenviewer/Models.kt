package com.nextos.screenviewer

/**
 * System info returned by `GET /v1/system`.
 *
 * Matches the JSON shape produced by the Rust agent in
 * `pc-agent/src/handlers.rs::SystemInfo`.
 */
data class SystemInfo(
    val hostname: String,
    val os: String,
    val arch: String,
    val kernel: String?,
    val osVersion: String?,
    val cpuBrand: String,
    val cpuCount: Int,
    val memTotalKb: Long,
    val memAvailKb: Long,
    val swapTotalKb: Long,
    val swapFreeKb: Long,
    val uptimeS: Long,
    val bootTimeS: Long,
    val loadAvg: Triple<Double, Double, Double>,
)

/**
 * One process returned by `GET /v1/processes`.
 */
data class ProcessInfo(
    val pid: Int,
    val parentPid: Int?,
    val name: String,
    val cmd: String,
    val exe: String?,
    val cwd: String?,
    val cpuPct: Float,
    val memKb: Long,
    val status: String,
)

/**
 * Result of `GET /v1/log`.
 */
data class LogResponse(
    val path: String,
    val lines: List<String>,
    val truncated: Boolean,
)
