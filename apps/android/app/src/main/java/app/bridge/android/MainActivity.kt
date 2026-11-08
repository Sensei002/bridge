package app.bridge.android

import android.os.Bundle
import android.graphics.Bitmap
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import android.os.Handler
import android.os.Looper
import java.net.ServerSocket
import java.net.Socket
import java.net.InetSocketAddress
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.PrintWriter
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Image
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.layout.ContentScale
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Background = Color(0xFF0A0B0F)
private val Sidebar = Color(0xFF0D0F13)
private val Panel = Color(0xFF14181D)
private val PanelAlt = Color(0xFF1A2025)
private val Border = Color(0xFF2B3238)
private val Accent = Color(0xFF22D3EE)
private val Amber = Color(0xFFFBBF24)
private val Muted = Color(0xFF929AA1)
private val Good = Color(0xFF77D397)
private val Ink = Color(0xFF11150D)

private enum class BridgeTab(val label: String, val symbol: String) {
    Home("Home", "⌂"),
    Devices("Devices", "⌁"),
    Transfers("Files", "⇧"),
    Clipboard("Clip", "▤"),
    Tools("Tools", "⌘"),
    Settings("Settings", "⚙")
}

class MainActivity : ComponentActivity() {
    @Volatile private var destroyed = false
    @Volatile private var discovery: BridgeDiscovery? = null
    @Volatile private var pairingController: BridgePairingController? = null
    private val discoveredPeers = mutableStateListOf<BridgePeer>()
    private val pendingPairings = mutableStateListOf<BridgePendingPairing>()
    private val trustedDevices = mutableStateListOf<BridgeTrustedDevice>()
    private var pairingStatus by mutableStateOf("Preparing secure device identity…")
    private var invitation by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Show the native UI immediately; Keystore and JNI initialization may
        // take time on first launch and must not block Android's main thread.
        setContent {
            BridgeApp(
                discoveredPeers = discoveredPeers,
                pendingPairings = pendingPairings,
                trustedDevices = trustedDevices,
                pairingStatus = pairingStatus,
                invitation = invitation,
                onCreateInvitation = {
                    try { invitation = pairingController?.createInvitation() }
                    catch (error: Exception) { pairingStatus = "Could not create QR invitation: ${error.message}" }
                },
                onDismissInvitation = { invitation = null },
                onScanInvitation = { scanInvitation() },
                onPair = { peer -> pairingController?.pair(peer) },
                onApprove = { pending, code ->
                    if (pairingController?.approve(pending, code) == true) {
                        pendingPairings.remove(pending)
                        trustedDevices.clear()
                        trustedDevices.addAll(pairingController?.trustedDevices().orEmpty())
                    }
                },
                onReject = { pending ->
                    pairingController?.reject(pending)
                    pendingPairings.remove(pending)
                },
                onRevoke = { deviceId ->
                    pairingController?.revoke(deviceId)
                    trustedDevices.clear()
                    trustedDevices.addAll(pairingController?.trustedDevices().orEmpty())
                }
            )
        }

        Thread({
            try {
                val privateKey = BridgeIdentityStore.loadOrCreate(this)
                val keyHex = privateKey.joinToString("") { "%02x".format(it.toInt() and 0xff) }
                val fingerprintResult = org.json.JSONObject(BridgeNative.identityFingerprint(keyHex))
                check(fingerprintResult.optBoolean("ok")) { fingerprintResult.optString("error") }
                val fingerprint = fingerprintResult.getString("fingerprint")
                val identityPreferences = getSharedPreferences("bridge_secure_identity", MODE_PRIVATE)
                val deviceId = identityPreferences.getString("device_id", null) ?: java.util.UUID.randomUUID().toString().also {
                    check(identityPreferences.edit().putString("device_id", it).commit())
                }

                val controller = BridgePairingController(
                    this, privateKey, fingerprint, deviceId,
                    onPending = { pendingPairings.add(it) },
                    onStatus = { pairingStatus = it }
                )
                val restoredDevices = controller.trustedDevices()
                if (destroyed) {
                    controller.stop()
                    return@Thread
                }
                pairingController = controller
                controller.start()

                val networkDiscovery = BridgeDiscovery(this, { peer ->
                    if (peer.deviceId != deviceId && discoveredPeers.none { it.deviceId == peer.deviceId }) {
                        discoveredPeers.add(peer)
                    }
                }, { message -> pairingStatus = message })
                if (destroyed) {
                    controller.stop()
                    return@Thread
                }
                discovery = networkDiscovery
                networkDiscovery.start(deviceId, android.os.Build.MODEL ?: "Bridge Android", fingerprint)

                runOnUiThread {
                    if (!destroyed) {
                        trustedDevices.addAll(restoredDevices)
                        pairingStatus = "Identity protected by Android Keystore · Noise XX ready."
                    }
                }
            } catch (error: Throwable) {
                runOnUiThread {
                    if (!destroyed) {
                        val detail = error.message?.takeIf { it.isNotBlank() } ?: error.javaClass.simpleName
                        pairingStatus = "Startup failed (${error.javaClass.simpleName}): $detail"
                    }
                }
            }
        }, "BridgeStartup").apply { isDaemon = true; start() }
    }

    private fun scanInvitation() {
        GmsBarcodeScanning.getClient(this).startScan()
            .addOnSuccessListener { barcode ->
                val value = barcode.rawValue
                if (value != null) pairingController?.pairFromInvitation(value)
                else pairingStatus = "No QR content was found."
            }
            .addOnCanceledListener { pairingStatus = "QR scanning cancelled." }
            .addOnFailureListener { error -> pairingStatus = "QR scanner unavailable: ${error.message}" }
    }

    override fun onDestroy() {
        destroyed = true
        pairingController?.stop()
        discovery?.stop()
        discovery = null
        super.onDestroy()
    }
}

private const val BRIDGE_PORT = 45821


private val mainHandler = Handler(Looper.getMainLooper())

private fun startBridgeListener(onStatus: (String) -> Unit) {
    Thread {
        try {
            ServerSocket(BRIDGE_PORT).use { server ->
                mainHandler.post { onStatus("Listening for Bridge tests on TCP $BRIDGE_PORT") }
                while (!server.isClosed) {
                    val socket = server.accept()
                    Thread {
                        socket.use { peer ->
                            try {
                                peer.soTimeout = 5000
                                val request = BufferedReader(InputStreamReader(peer.getInputStream())).readLine()
                                if (request == "BRIDGE_PING/1") {
                                    PrintWriter(peer.getOutputStream(), true).println("BRIDGE_PONG/1")
                                    mainHandler.post { onStatus("Network test succeeded with ${peer.inetAddress.hostAddress}") }
                                } else {
                                    mainHandler.post { onStatus("Received an unknown test message") }
                                }
                            } catch (error: Exception) {
                                mainHandler.post { onStatus("Connection test error: ${error.message}") }
                            }
                        }
                    }.start()
                }
            }
        } catch (error: Exception) {
            mainHandler.post { onStatus("Network listener unavailable: ${error.message}") }
        }
    }.apply { isDaemon = true; name = "BridgeNetworkListener" }.start()
}

private fun testBridgeConnection(host: String, onStatus: (String) -> Unit) {
    Thread {
        try {
            Socket().use { socket ->
                socket.connect(InetSocketAddress(host, BRIDGE_PORT), 4000)
                socket.soTimeout = 4000
                PrintWriter(socket.getOutputStream(), true).println("BRIDGE_PING/1")
                val response = BufferedReader(InputStreamReader(socket.getInputStream())).readLine()
                if (response == "BRIDGE_PONG/1") {
                    mainHandler.post { onStatus("Connection test succeeded with $host") }
                } else {
                    mainHandler.post { onStatus("Unexpected response from $host") }
                }
            }
        } catch (error: Exception) {
            mainHandler.post { onStatus("Could not connect to $host:$BRIDGE_PORT — ${error.message}") }
        }
    }.apply { isDaemon = true; name = "BridgeNetworkTest" }.start()
}

@Composable
private fun BridgeApp(
    discoveredPeers: List<BridgePeer>,
    pendingPairings: List<BridgePendingPairing>,
    trustedDevices: List<BridgeTrustedDevice>,
    pairingStatus: String,
    invitation: String?,
    onCreateInvitation: () -> Unit,
    onDismissInvitation: () -> Unit,
    onScanInvitation: () -> Unit,
    onPair: (BridgePeer) -> Unit,
    onApprove: (BridgePendingPairing, String) -> Unit,
    onReject: (BridgePendingPairing) -> Unit,
    onRevoke: (String) -> Unit
) {
    var selected by remember { mutableStateOf(BridgeTab.Home) }
    var showPairDialog by remember { mutableStateOf(false) }
    var clipboardSync by remember { mutableStateOf(true) }
    var notifications by remember { mutableStateOf(true) }
    var autoAccept by remember { mutableStateOf(false) }
    var networkStatus by remember { mutableStateOf("Secure local pairing is ready.") }

    MaterialTheme(colorScheme = darkColorScheme(
        primary = Accent,
        onPrimary = Ink,
        background = Background,
        surface = Panel,
        onSurface = Color.White
    )) {
        Surface(modifier = Modifier.fillMaxSize(), color = Background) {
            Column {
                HeaderBar()
                Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                    when (selected) {
                        BridgeTab.Home -> HomeScreen(
                            onPair = { showPairDialog = true },
                            onNavigate = { selected = it }
                        )
                        BridgeTab.Devices -> DevicesScreen(
                            peers = discoveredPeers,
                            pending = pendingPairings,
                            trusted = trustedDevices,
                            pairingStatus = pairingStatus,
                            onCreateInvitation = onCreateInvitation,
                            onScanInvitation = onScanInvitation,
                            onPair = onPair,
                            onApprove = onApprove,
                            onReject = onReject,
                            onRevoke = onRevoke
                        )
                        BridgeTab.Transfers -> TransfersScreen(onPair = { showPairDialog = true })
                        BridgeTab.Clipboard -> ClipboardScreen(
                            enabled = clipboardSync,
                            onEnabledChange = { clipboardSync = it }
                        )
                        BridgeTab.Tools -> ToolsScreen()
                        BridgeTab.Settings -> SettingsScreen(
                            notifications = notifications,
                            onNotificationsChange = { notifications = it },
                            autoAccept = autoAccept,
                            onAutoAcceptChange = { autoAccept = it }
                        )
                    }
                }
                BottomNavigation(selected = selected, onSelect = { selected = it })
            }
        }
    }

    if (invitation != null) {
        AlertDialog(
            onDismissRequest = onDismissInvitation,
            containerColor = PanelAlt,
            title = { Text("Pair with this device", color = Color.White, fontWeight = FontWeight.Bold) },
            text = {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    val qrBitmap = remember(invitation) {
                        val matrix = QRCodeWriter().encode(invitation, BarcodeFormat.QR_CODE, 720, 720)
                        Bitmap.createBitmap(720, 720, Bitmap.Config.ARGB_8888).also { bitmap ->
                            for (x in 0 until 720) for (y in 0 until 720) bitmap.setPixel(x, y, if (matrix[x, y]) android.graphics.Color.BLACK else android.graphics.Color.WHITE)
                        }
                    }
                    Image(bitmap = qrBitmap.asImageBitmap(), contentDescription = "Short-lived Bridge pairing QR code", modifier = Modifier.size(240.dp))
                    Text("Scan this code on the other device. It expires after two minutes and still requires matching Noise verification codes on both screens.", color = Muted, fontSize = 12.sp)
                    Text("Do not share this code publicly.", color = Amber, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                }
            },
            confirmButton = { Button(onClick = onDismissInvitation) { Text("Close") } }
        )
    }

    if (showPairDialog) {
        AlertDialog(
            onDismissRequest = { showPairDialog = false },
            containerColor = PanelAlt,
            titleContentColor = Color.White,
            textContentColor = Muted,
            title = { Text("Pair a device", fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Select a discovered device from the Devices tab. Bridge authenticates it with Noise XX, then asks you to compare the six-digit code on both screens before trusting it.", color = Color.White, fontSize = 13.sp, lineHeight = 19.sp)
                    Surface(color = Panel, shape = RoundedCornerShape(14.dp), modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(18.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text("▦", color = Accent, fontSize = 34.sp)
                            Text("Authenticated pairing", color = Color.White, fontWeight = FontWeight.SemiBold)
                            Text(pairingStatus, color = Amber, fontSize = 11.sp)
                        }
                    }
                    PrimaryAction("Open Devices", { showPairDialog = false; selected = BridgeTab.Devices }, Modifier.fillMaxWidth())
                }
            },
            confirmButton = {
                Button(onClick = { showPairDialog = false }, colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)) {
                    Text("Got it", fontWeight = FontWeight.Bold)
                }
            }
        )
    }
}

@Composable
private fun HeaderBar() {
    Row(
        modifier = Modifier.fillMaxWidth().background(Sidebar).padding(horizontal = 20.dp, vertical = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Image(
                painter = painterResource(id = R.drawable.bridge_app_icon),
                contentDescription = "Bridge logo",
                contentScale = ContentScale.Fit,
                modifier = Modifier.size(40.dp)
            )
            Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Text("BRIDGE", color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Black, letterSpacing = 2.sp)
                Text("EVERY DEVICE. ONE SPACE.", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Surface(color = Good.copy(alpha = 0.12f), shape = CircleShape, modifier = Modifier.size(7.dp)) {}
            Text("LOCAL MODE", color = Good, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.8.sp)
        }
    }
}

@Composable
private fun BottomNavigation(selected: BridgeTab, onSelect: (BridgeTab) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().background(Sidebar).padding(horizontal = 4.dp, vertical = 7.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.CenterVertically
    ) {
        BridgeTab.values().forEach { tab ->
            val active = selected == tab
            Column(
                modifier = Modifier.weight(1f).clickable { onSelect(tab) }.padding(vertical = 5.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(3.dp)
            ) {
                Text(tab.symbol, color = if (active) Accent else Muted, fontSize = 19.sp, fontWeight = FontWeight.Bold)
                Text(tab.label, color = if (active) Accent else Muted, fontSize = 9.sp, fontWeight = if (active) FontWeight.Bold else FontWeight.Medium, maxLines = 1)
                if (active) Surface(color = Accent, shape = CircleShape, modifier = Modifier.width(14.dp).height(2.dp)) {}
                else Spacer(Modifier.height(2.dp))
            }
        }
    }
}

@Composable
private fun ScreenContent(content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        content = content
    )
}

@Composable
private fun Eyebrow(text: String) {
    Text(text.uppercase(), color = Accent, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.7.sp)
}

@Composable
private fun ScreenHeading(kicker: String, title: String, subtitle: String) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Eyebrow(kicker)
        Text(title, color = Color.White, fontSize = 27.sp, lineHeight = 32.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.6).sp)
        Text(subtitle, color = Muted, fontSize = 13.sp, lineHeight = 19.sp)
    }
}

@Composable
private fun Panel(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = Panel,
        shape = RoundedCornerShape(16.dp)
    ) {
        Column(modifier = Modifier.padding(17.dp), verticalArrangement = Arrangement.spacedBy(10.dp), content = content)
    }
}

@Composable
private fun PrimaryAction(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Button(
        onClick = onClick,
        modifier = modifier.height(46.dp),
        shape = RoundedCornerShape(10.dp),
        colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)
    ) {
        Text(label, fontWeight = FontWeight.Bold, fontSize = 13.sp)
    }
}

@Composable
private fun StatusTag(text: String, color: Color = Muted) {
    Surface(color = color.copy(alpha = 0.10f), shape = RoundedCornerShape(30.dp)) {
        Text(text, color = color, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.6.sp, modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp))
    }
}

@Composable
private fun FeatureLine(symbol: String, title: String, detail: String, status: String = "PLANNED") {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Surface(color = PanelAlt, shape = RoundedCornerShape(10.dp), modifier = Modifier.size(38.dp)) {
            Box(contentAlignment = Alignment.Center) {
                Text(symbol, color = Accent, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            Text(detail, color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
        }
        StatusTag(status)
    }
}

@Composable
private fun HomeScreen(onPair: () -> Unit, onNavigate: (BridgeTab) -> Unit) {
    ScreenContent {
        ScreenHeading("YOUR ECOSYSTEM", "Everything works\nbetter together.", "A private space for the devices you use every day.")
        Panel {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(7.dp)) {
                    Eyebrow("BRIDGE NETWORK")
                    Text("Ready when you are.", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold)
                    Text("No devices connected yet. Pair your first device to get started.", color = Muted, fontSize = 12.sp, lineHeight = 18.sp)
                }
                Surface(color = PanelAlt, shape = CircleShape, modifier = Modifier.size(48.dp)) {
                    Box(contentAlignment = Alignment.Center) { Text("⌁", color = Accent, fontSize = 27.sp) }
                }
            }
            PrimaryAction("＋   Pair a new device", onPair, Modifier.fillMaxWidth())
            Text("PRIVATE BY DEFAULT  ·  NO ACCOUNT REQUIRED", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            MiniMetric("0", "Trusted devices", Modifier.weight(1f))
            MiniMetric("0", "Active transfers", Modifier.weight(1f))
            MiniMetric("LOCAL", "Network mode", Modifier.weight(1f))
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
            Text("Quick access", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("YOUR WORKSPACE", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
        }
        Panel {
            FeatureLine("⌁", "Your devices", "Pair and manage trusted devices.")
            DividerLine()
            FeatureLine("⇧", "File transfers", "Send files directly between devices.")
            DividerLine()
            FeatureLine("▤", "Universal clipboard", "Move text between your screens.")
            DividerLine()
            FeatureLine("⌘", "Device tools", "Use your phone and tablet in new ways.")
            PrimaryAction("Explore device tools  →", { onNavigate(BridgeTab.Tools) }, Modifier.fillMaxWidth())
        }
        Text("BRIDGE PREVIEW  ·  BUILT FOR YOUR DEVICES", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
    }
}

@Composable
private fun MiniMetric(value: String, label: String, modifier: Modifier = Modifier) {
    Surface(modifier = modifier, color = Panel, shape = RoundedCornerShape(13.dp)) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Text(value, color = if (value == "LOCAL") Accent else Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold, maxLines = 1)
            Text(label, color = Muted, fontSize = 9.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun DevicesScreen(
    peers: List<BridgePeer>,
    pending: List<BridgePendingPairing>,
    trusted: List<BridgeTrustedDevice>,
    pairingStatus: String,
    onCreateInvitation: () -> Unit,
    onScanInvitation: () -> Unit,
    onPair: (BridgePeer) -> Unit,
    onApprove: (BridgePendingPairing, String) -> Unit,
    onReject: (BridgePendingPairing) -> Unit,
    onRevoke: (String) -> Unit
) {
    ScreenContent {
        ScreenHeading("YOUR ECOSYSTEM", "Devices", "Find and manage the devices you choose to trust.")
        Panel {
            Eyebrow("SECURE QR PAIRING")
            Text("Connect without browsing the device list", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("Create a short-lived invitation on this device, or scan one from another Bridge device. QR data only bootstraps the connection; Noise XX and matching codes still authenticate both peers.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PrimaryAction("Show my QR", onCreateInvitation, Modifier.weight(1f))
                Button(onClick = onScanInvitation, modifier = Modifier.weight(1f).height(46.dp), colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Accent)) { Text("Scan QR", fontWeight = FontWeight.Bold) }
            }
        }
        Panel {
            Eyebrow("DEVICE DISCOVERY")
            Text("Nearby Bridge devices", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold)
            Text("Discovery metadata is unverified. A device becomes trusted only after Noise XX authentication, matching codes, and explicit approval.", color = Muted, fontSize = 12.sp, lineHeight = 18.sp)
            if (peers.isEmpty()) {
                Text("Searching for Bridge devices on this network…", color = Muted, fontSize = 11.sp)
            } else {
                peers.forEach { peer ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(peer.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("${peer.host}:${peer.port} · ${peer.platform}", color = Muted, fontSize = 10.sp)
                            Text("UNVERIFIED · ${peer.fingerprint.take(16)}", color = Amber, fontSize = 9.sp)
                        }
                        PrimaryAction("Pair", { onPair(peer) })
                    }
                }
            }
        }
        if (pending.isNotEmpty()) {
            pending.forEach { request ->
                var remoteCode by remember(request.handle) { mutableStateOf("") }
                Panel {
                    Eyebrow("VERIFY IDENTITY · NOISE XX")
                    Text(request.name, color = Color.White, fontSize = 17.sp, fontWeight = FontWeight.Bold)
                    Text("${request.platform} · fingerprint ${request.fingerprint.take(24)}", color = Muted, fontSize = 10.sp)
                    Text(request.sas, color = Accent, fontSize = 34.sp, fontWeight = FontWeight.Black, letterSpacing = 4.sp)
                    Text("Compare this code with the other device. Enter the code shown there; do not approve if the codes differ.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                    androidx.compose.material3.OutlinedTextField(
                        value = remoteCode,
                        onValueChange = { if (it.length <= 6 && it.all { character -> character.isDigit() }) remoteCode = it },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        label = { Text("Code on the other device") },
                        placeholder = { Text("000000") }
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        PrimaryAction("Confirm and trust", { onApprove(request, remoteCode) }, Modifier.weight(1f))
                        Button(onClick = { onReject(request) }, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                            Text("Reject")
                        }
                    }
                }
            }
        }
        Panel {
            Eyebrow("TRUSTED DEVICES")
            if (trusted.isEmpty()) {
                Text("No trusted devices yet.", color = Muted, fontSize = 12.sp)
            } else {
                trusted.forEach { device ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(device.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("${device.platform} · ${device.fingerprint.take(16)}", color = Muted, fontSize = 10.sp)
                        }
                        Button(onClick = { onRevoke(device.deviceId) }, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                            Text("Revoke")
                        }
                    }
                }
            }
        }
        Text(pairingStatus, color = Amber, fontSize = 11.sp, lineHeight = 16.sp)
        Text("QR invitations are short-lived endpoint hints. Every scanned invitation is consumed once and its fingerprint is pinned in the shared Rust Noise XX handshake.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
    }
}

@Composable
private fun TransfersScreen(onPair: () -> Unit) {
    ScreenContent {
        ScreenHeading("MOVE WITHOUT LIMITS", "File transfers", "Send files between your trusted devices. Privately.")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            StatusTag("ALL TRANSFERS", Accent)
            StatusTag("SENDING")
            StatusTag("RECEIVED")
        }
        Panel {
            Box(modifier = Modifier.fillMaxWidth().padding(vertical = 20.dp), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(9.dp)) {
                    Text("⇧", color = Accent, fontSize = 42.sp)
                    Text("Nothing moving yet", color = Color.White, fontSize = 17.sp, fontWeight = FontWeight.Bold)
                    Text("Your sent and received files will show up here.", color = Muted, fontSize = 11.sp)
                    PrimaryAction("＋   Choose files", onPair)
                }
            }
        }
        Panel {
            FeatureLine("◇", "Direct device transfer", "Designed for local, peer-to-peer sharing.", "PLANNED")
            DividerLine()
            FeatureLine("↻", "Resume interrupted transfers", "Continue large transfers after a disconnect.", "PLANNED")
        }
        Text("File transport is not active yet.", color = Muted, fontSize = 10.sp)
    }
}

@Composable
private fun ClipboardScreen(enabled: Boolean, onEnabledChange: (Boolean) -> Unit) {
    ScreenContent {
        ScreenHeading("SHARE WHAT YOU COPY", "Universal clipboard", "Copy on one device. Paste on another.")
        Panel {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    Text("Clipboard sync", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text("Allow trusted devices to share text.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                }
                Switch(checked = enabled, onCheckedChange = onEnabledChange)
            }
            StatusTag(if (enabled) "PREFERENCE ENABLED · SERVICE NOT CONNECTED" else "CLIPBOARD SYNC PAUSED", if (enabled) Accent else Muted)
        }
        Panel {
            Eyebrow("RECENT CLIPBOARD")
            Box(modifier = Modifier.fillMaxWidth().padding(vertical = 18.dp), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("▤", color = Accent, fontSize = 36.sp)
                    Text("Nothing copied yet", color = Color.White, fontWeight = FontWeight.SemiBold)
                    Text("Shared text will appear here when sync is connected.", color = Muted, fontSize = 11.sp)
                }
            }
        }
        Text("This switch saves a local UI preference only; clipboard monitoring is not active.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
    }
}

@Composable
private fun ToolsScreen() {
    ScreenContent {
        ScreenHeading("MORE FROM YOUR DEVICES", "Device tools", "Make your phone, PC and tablet work as one.")
        Panel {
            FeatureLine("♫", "Phone as microphone", "Use your phone as a wireless PC input.")
            DividerLine()
            FeatureLine("▣", "Phone as webcam", "Bring your mobile camera into desktop apps.")
            DividerLine()
            FeatureLine("◖", "PC audio on phone", "Listen to your computer from another device.")
            DividerLine()
            FeatureLine("⌖", "Remote input", "Use a trusted phone or tablet as a controller.")
            DividerLine()
            FeatureLine("▤", "Phone notifications", "See selected mobile notifications on desktop.")
            DividerLine()
            FeatureLine("▦", "Second screen", "Explore a tablet as an extended workspace.")
        }
        Text("These capabilities are planned; no microphone, camera or remote-control service is running yet.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
    }
}

@Composable
private fun SettingsScreen(
    notifications: Boolean,
    onNotificationsChange: (Boolean) -> Unit,
    autoAccept: Boolean,
    onAutoAcceptChange: (Boolean) -> Unit
) {
    ScreenContent {
        ScreenHeading("MAKE IT YOURS", "Settings", "Privacy and preferences for your Bridge workspace.")
        Panel {
            Eyebrow("THIS DEVICE")
            Text("Android device", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("Bridge mobile · Version 0.3 preview", color = Muted, fontSize = 11.sp)
            StatusTag("NOT PAIRED")
        }
        Panel {
            Eyebrow("PREFERENCES")
            PreferenceSwitch("Notifications", "Show Bridge activity notifications.", notifications, onNotificationsChange)
            DividerLine()
            PreferenceSwitch("Auto-accept trusted devices", "Skip confirmation for trusted requests.", autoAccept, onAutoAcceptChange)
        }
        Panel {
            Eyebrow("PRIVACY & SECURITY")
            FeatureLine("◇", "Local-first by design", "No mandatory account or cloud relay.", "CORE")
            DividerLine()
            FeatureLine("⌑", "Per-device permissions", "Choose what each device can access.", "PLANNED")
            DividerLine()
            FeatureLine("⚿", "Encrypted connections", "Secure transport foundation in shared core.", "FOUNDATION")
        }
        Panel {
            Eyebrow("ABOUT BRIDGE")
            Text("Your devices. Your rules.", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("Version 0.3 preview · Native Android interface", color = Muted, fontSize = 11.sp)
            Text("BRIDGE · BUILT FOR YOUR DEVICES", color = Accent, fontSize = 8.sp, letterSpacing = 1.sp)
        }
    }
}

@Composable
private fun PreferenceSwitch(title: String, detail: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(title, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            Text(detail, color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
        }
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

@Composable
private fun DividerLine() {
    Spacer(modifier = Modifier.fillMaxWidth().height(1.dp).background(Border))
}
