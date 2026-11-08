package app.bridge.android

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo

/**
 * Browses local Bridge DNS-SD advertisements.
 *
 * All results are untrusted candidates. NSD discovery does not authenticate a
 * peer; callers must never add a result to the trusted-device list from here.
 */
internal class BridgeDiscovery(
    context: Context,
    private val onPeerFound: (name: String, host: String, port: Int) -> Unit,
) {
    private val manager = context.applicationContext.getSystemService(NsdManager::class.java)
    @Volatile private var stopped = false

    private val resolveListener = object : NsdManager.ResolveListener {
        override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) = Unit

        override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
            if (stopped) return
            val host = serviceInfo.host?.hostAddress ?: return
            onPeerFound(serviceInfo.serviceName, host, serviceInfo.port)
        }
    }

    private val discoveryListener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(serviceType: String) = Unit
        override fun onDiscoveryStopped(serviceType: String) = Unit
        override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
            stop()
        }
        override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
            stop()
        }
        override fun onServiceFound(serviceInfo: NsdServiceInfo) {
            if (stopped || serviceInfo.serviceType != SERVICE_TYPE) return
            @Suppress("DEPRECATION")
            manager.resolveService(serviceInfo, resolveListener)
        }
        override fun onServiceLost(serviceInfo: NsdServiceInfo) = Unit
    }

    fun start() {
        stopped = false
        manager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discoveryListener)
    }

    fun stop() {
        if (stopped) return
        stopped = true
        try {
            manager.stopServiceDiscovery(discoveryListener)
        } catch (_: IllegalArgumentException) {
            // The platform may already have stopped discovery.
        }
    }

    private companion object {
        const val SERVICE_TYPE = "_bridge._tcp."
    }
}
