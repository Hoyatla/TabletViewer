package com.nextos.screenviewer

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * LAN auto-discovery of `pc-agent` instances via mDNS / NSD.
 *
 * The pc-agent advertises itself as `_pcagent._tcp.local.` (see
 * `pc-agent/src/discovery.rs`). On the Android side we use the platform
 * `NsdManager` to discover those services and pick one.
 *
 * The result is a list of `DiscoveredAgent` containing the human-readable
 * name, the resolved IP, and the port. The caller can then build a
 * `LanDebugApi` from the first match.
 */
object NetworkDiscovery {

    private const val SERVICE_TYPE = "_pcagent._tcp."

    data class DiscoveredAgent(
        val name: String,
        val host: String,
        val port: Int,
    ) {
        val baseUrl: String get() = "http://$host:$port"
    }

    private const val TAG = "NetworkDiscovery"

    /**
     * Discover agents with the given timeout (ms). Returns whatever was
     * found within that window — could be 0, 1, or many. The caller picks.
     */
    suspend fun discover(ctx: Context, timeoutMs: Int = 4000): List<DiscoveredAgent> {
        val nsd = ctx.getSystemService(Context.NSD_SERVICE) as NsdManager
        val results = mutableListOf<DiscoveredAgent>()

        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(regType: String) {
                Log.d(TAG, "discovery started: $regType")
            }

            override fun onServiceFound(service: NsdServiceInfo) {
                if (service.serviceType.contains("_pcagent")) {
                    Log.d(TAG, "found service: ${service.serviceName}")
                    resolve(nsd, service, results)
                }
            }

            override fun onServiceLost(service: NsdServiceInfo) {
                Log.d(TAG, "service lost: ${service.serviceName}")
            }

            override fun onDiscoveryStopped(serviceType: String) {
                Log.d(TAG, "discovery stopped: $serviceType")
            }

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "start discovery failed: $errorCode")
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "stop discovery failed: $errorCode")
            }
        }

        return suspendCancellableCoroutine { cont ->
            nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
            cont.invokeOnCancellation {
                runCatching { nsd.stopServiceDiscovery(listener) }
            }
            // Poll for results on a timer (NsdManager has no future/cancel
            // for the whole "discover for N ms" pattern).
            val handler = android.os.Handler(android.os.Looper.getMainLooper())
            val poll = object : Runnable {
                override fun run() {
                    if (cont.isActive) {
                        nsd.stopServiceDiscovery(listener)
                        cont.resume(results.toList())
                    }
                }
            }
            handler.postDelayed(poll, timeoutMs.toLong())
        }
    }

    private fun resolve(
        nsd: NsdManager,
        service: NsdServiceInfo,
        sink: MutableList<DiscoveredAgent>,
    ) {
        val resolveListener = object : NsdManager.ResolveListener {
            override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                Log.w(TAG, "resolve failed for ${serviceInfo.serviceName}: $errorCode")
            }

            override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                val host = serviceInfo.host?.hostAddress ?: return
                val port = serviceInfo.port
                val name = serviceInfo.serviceName.removeSuffix("._pcagent._tcp.")
                val agent = DiscoveredAgent(name = name, host = host, port = port)
                synchronized(sink) { sink.add(agent) }
                Log.d(TAG, "resolved: $agent")
            }
        }
        runCatching {
            nsd.resolveService(service, resolveListener)
        }.onFailure { Log.w(TAG, "resolveService threw: ${it.message}") }
    }
}
