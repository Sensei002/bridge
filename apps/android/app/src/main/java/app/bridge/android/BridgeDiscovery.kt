package app.bridge.android

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo

/**
 * Publishes this device and browses for Bridge peers over DNS-SD.
 * Advertised metadata is untrusted; identity is confirmed only by Noise XX.
 */
internal class BridgeDiscovery(
    context: Context,
    private val onPeerFound: (BridgePeer) -> Unit,
    private val onPeerLost: (String) -> Unit,
    private val onStatus: (String) -> Unit
) {
    private val manager = context.applicationContext.getSystemService(NsdManager::class.java)
    @Volatile private var stopped = false
    private var registered = false
    private var localDeviceId: String? = null

    private val registrationListener = object : NsdManager.RegistrationListener {
        override fun onServiceRegistered(serviceInfo: NsdServiceInfo) {
            registered = true
            onStatus("Bridge is visible on the local network.")
        }
        override fun onRegistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
            onStatus("Could not advertise Bridge service (NSD error $errorCode).")
        }
        override fun onServiceUnregistered(serviceInfo: NsdServiceInfo) { registered = false }
        override fun onUnregistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
            onStatus("Could not stop Bridge advertisement (NSD error $errorCode).")
        }
    }

    private val resolveListener = object : NsdManager.ResolveListener {
        override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) = Unit

        override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
            if (stopped) return
            val host = serviceInfo.host?.hostAddress ?: return
            val attributes = serviceInfo.attributes
            fun attr(key: String) = attributes[key]?.toString(Charsets.UTF_8).orEmpty()
            val id = attr("id")
            val fingerprint = attr("fingerprint")
            val platform = attr("platform")
            val name = attr("name").ifBlank { serviceInfo.serviceName }
            if (id.isBlank() || id == localDeviceId || fingerprint.length != 64 ||
                !fingerprint.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) return
            onPeerFound(BridgePeer(name, host, serviceInfo.port, id, fingerprint, platform))
        }
    }

    private val discoveryListener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(serviceType: String) = Unit
        override fun onDiscoveryStopped(serviceType: String) = Unit
        override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
            onStatus("Bridge discovery failed to start (NSD error $errorCode).")
            stop()
        }
        override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
            onStatus("Bridge discovery failed to stop (NSD error $errorCode).")
            stop()
        }
        override fun onServiceFound(serviceInfo: NsdServiceInfo) {
            if (stopped || serviceInfo.serviceType != SERVICE_TYPE) return
            @Suppress("DEPRECATION")
            manager.resolveService(serviceInfo, resolveListener)
        }
        override fun onServiceLost(serviceInfo: NsdServiceInfo) {
            if (!stopped) onPeerLost(serviceInfo.serviceName)
        }
    }

    fun start(deviceId: String, displayName: String, fingerprint: String) {
        if (registered || localDeviceId != null) return
        stopped = false
        localDeviceId = deviceId
        val service = NsdServiceInfo().apply {
            serviceName = "Bridge-${deviceId.take(8)}"
            serviceType = SERVICE_TYPE
            port = BRIDGE_PORT
            setAttribute("id", deviceId)
            setAttribute("name", displayName.take(64))
            setAttribute("platform", "android")
            setAttribute("version", "1")
            setAttribute("fingerprint", fingerprint)
        }
        manager.registerService(service, NsdManager.PROTOCOL_DNS_SD, registrationListener)
        manager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discoveryListener)
    }

    fun stop() {
        if (stopped) return
        stopped = true
        try { manager.stopServiceDiscovery(discoveryListener) } catch (_: IllegalArgumentException) { }
        if (registered) {
            try { manager.unregisterService(registrationListener) } catch (_: IllegalArgumentException) { }
        }
        registered = false
        localDeviceId = null
    }

    private companion object {
        const val SERVICE_TYPE = "_bridge._tcp."
        const val BRIDGE_PORT = 45821
    }
}
