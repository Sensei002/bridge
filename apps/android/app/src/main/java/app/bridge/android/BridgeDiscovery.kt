package app.bridge.android

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import java.util.concurrent.ConcurrentHashMap

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
    private val appContext = context.applicationContext
    private val manager = appContext.getSystemService(NsdManager::class.java)
    // Android filters Wi-Fi multicast packets unless the app holds this lock.
    // Permission in the manifest alone does not guarantee mDNS receive traffic.
    private val multicastLock = appContext.getSystemService(WifiManager::class.java)
        .createMulticastLock("BridgeDiscovery").apply { setReferenceCounted(false) }
    @Volatile private var stopped = false
    private var registered = false
    private var localDeviceId: String? = null
    private var localDisplayName: String? = null
    private var localFingerprint: String? = null
    private val resolving = ConcurrentHashMap<String, NsdManager.ResolveListener>()

    @Volatile private var generation = 0
    private var registrationListener: NsdManager.RegistrationListener? = null
    private var discoveryListener: NsdManager.DiscoveryListener? = null

    private fun createRegistrationListener(runId: Int) = object : NsdManager.RegistrationListener {
        override fun onServiceRegistered(serviceInfo: NsdServiceInfo) {
            if (stopped || generation != runId) {
                try { manager.unregisterService(this) } catch (_: IllegalArgumentException) { }
                return
            }
            registered = true
            onStatus("Bridge is visible on the local network.")
        }
        override fun onRegistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
            if (!stopped && generation == runId) {
                onStatus("Could not advertise Bridge service (NSD error $errorCode).")
            }
        }
        override fun onServiceUnregistered(serviceInfo: NsdServiceInfo) {
            if (generation == runId) registered = false
        }
        override fun onUnregistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
            if (!stopped && generation == runId) {
                onStatus("Could not stop Bridge advertisement (NSD error $errorCode).")
            }
        }
    }

    private fun resolveListener(serviceName: String, runId: Int): NsdManager.ResolveListener =
        object : NsdManager.ResolveListener {
            override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                resolving.remove(serviceName, this)
                if (!stopped && generation == runId) {
                    onStatus("Found ${serviceName}, but Android could not resolve it (NSD error $errorCode).")
                }
            }

            override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                resolving.remove(serviceName, this)
                if (stopped || generation != runId) return
                val host = serviceInfo.host?.hostAddress
                if (host.isNullOrBlank()) {
                    onStatus("Bridge service ${serviceInfo.serviceName} resolved without a usable network address.")
                    return
                }
                val attributes = serviceInfo.attributes
                // DNS-SD TXT keys are case-insensitive. Be tolerant of platform
                // differences in how Android exposes their original casing.
                fun attr(key: String) = attributes.entries
                    .firstOrNull { it.key.equals(key, ignoreCase = true) }
                    ?.value?.toString(Charsets.UTF_8).orEmpty()
                val id = attr("id")
                val fingerprint = attr("fingerprint")
                val platform = attr("platform")
                val name = attr("name").ifBlank { serviceInfo.serviceName }
                if (id.isBlank()) {
                    onStatus("Ignored ${serviceInfo.serviceName}: its Bridge device ID is missing.")
                    return
                }
                if (id == localDeviceId) return
                if (fingerprint.length != 64 ||
                    !fingerprint.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) {
                    onStatus("Ignored ${serviceInfo.serviceName}: its identity fingerprint is missing or invalid.")
                    return
                }
                if (platform.isBlank()) {
                    onStatus("Ignored ${serviceInfo.serviceName}: its platform metadata is missing.")
                    return
                }
                onPeerFound(BridgePeer(name, host, serviceInfo.port, id, fingerprint, platform, serviceInfo.serviceName))
                onStatus("Found nearby Bridge device: $name.")
            }
        }

    private fun createDiscoveryListener(runId: Int) = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(serviceType: String) = Unit
        override fun onDiscoveryStopped(serviceType: String) = Unit
        override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
            if (stopped || generation != runId) return
            onStatus("Bridge discovery failed to start (NSD error $errorCode).")
            stop()
        }
        override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
            if (stopped || generation != runId) return
            onStatus("Bridge discovery failed to stop (NSD error $errorCode).")
            stop()
        }
        override fun onServiceFound(serviceInfo: NsdServiceInfo) {
            if (stopped || generation != runId) return
            onStatus("Found Bridge service ${serviceInfo.serviceName}; resolving…")
            val listener = resolveListener(serviceInfo.serviceName, runId)
            if (resolving.putIfAbsent(serviceInfo.serviceName, listener) != null) return
            @Suppress("DEPRECATION")
            try {
                manager.resolveService(serviceInfo, listener)
            } catch (error: IllegalArgumentException) {
                resolving.remove(serviceInfo.serviceName, listener)
                onStatus("Could not resolve nearby Bridge device: ${error.message}")
            }
        }
        override fun onServiceLost(serviceInfo: NsdServiceInfo) {
            resolving.remove(serviceInfo.serviceName)
            if (!stopped && generation == runId) onPeerLost(serviceInfo.serviceName)
        }
    }

    fun start(deviceId: String, displayName: String, fingerprint: String) {
        if (registered || localDeviceId != null) return
        stopped = false
        val runId = ++generation
        val registration = createRegistrationListener(runId)
        val browser = createDiscoveryListener(runId)
        registrationListener = registration
        discoveryListener = browser
        try {
            if (!multicastLock.isHeld) multicastLock.acquire()
        } catch (error: SecurityException) {
            onStatus("Could not enable local-network discovery (multicast permission unavailable): ${error.message}")
        }
        localDeviceId = deviceId
        localDisplayName = displayName
        localFingerprint = fingerprint
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
        manager.registerService(service, NsdManager.PROTOCOL_DNS_SD, registration)
        manager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, browser)
    }

    fun restart() {
        val id = localDeviceId ?: return
        val name = localDisplayName ?: return
        val keyFingerprint = localFingerprint ?: return
        stop()
        // NSD stop/unregister callbacks are asynchronous. Give Android time to
        // retire the previous listeners before registering fresh listener instances.
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
            if (localDeviceId == null && !stopped) start(id, name, keyFingerprint)
        }, 2500)
    }

    fun stop() {
        if (stopped) return
        stopped = true
        generation++
        resolving.clear()
        val oldBrowser = discoveryListener
        val oldRegistration = registrationListener
        discoveryListener = null
        registrationListener = null
        try { oldBrowser?.let { manager.stopServiceDiscovery(it) } } catch (_: IllegalArgumentException) { }
        if (registered) {
            try { oldRegistration?.let { manager.unregisterService(it) } } catch (_: IllegalArgumentException) { }
        }
        registered = false
        if (multicastLock.isHeld) {
            try { multicastLock.release() } catch (_: RuntimeException) { }
        }
        localDeviceId = null
        localDisplayName = null
        localFingerprint = null
    }

    private companion object {
        const val SERVICE_TYPE = "_bridge._tcp."
        const val BRIDGE_PORT = 45821
    }
}
