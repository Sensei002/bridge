package app.bridge.android

import android.os.Bundle
import android.view.MotionEvent
import android.graphics.Bitmap
import java.nio.ByteBuffer
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.ExperimentalComposeUiApi
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
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
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import android.net.Uri
import java.io.File

import androidx.compose.foundation.Image
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntSize
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
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.input.pointer.pointerInteropFilter
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
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
    SecondScreen("Screen", "▣"),
    Tools("Tools", "⌘"),
    Settings("Settings", "⚙")
}

class MainActivity : ComponentActivity() {
    private val discoveredPeers get() = BridgeRuntimeState.discoveredPeers
    private val pendingPairings get() = BridgeRuntimeState.pendingPairings
    private val trustedDevices get() = BridgeRuntimeState.trustedDevices
    private val connectedSessionHandles get() = BridgeRuntimeState.connectedSessionHandles
    private val incomingOffers get() = BridgeRuntimeState.incomingOffers
    private val receivedFiles get() = BridgeRuntimeState.receivedFiles
    private val clipboardReceivedText get() = BridgeRuntimeState.clipboardReceivedText.value
    private val clipboardReceivedFrom get() = BridgeRuntimeState.clipboardReceivedFrom.value
    private val clipboardStatus get() = BridgeRuntimeState.clipboardStatus.value
    private val pairingStatus get() = BridgeRuntimeState.pairingStatus.value
    private val transferStatus get() = BridgeRuntimeState.transferStatus.value
    private val latestDisplayFrame get() = BridgeRuntimeState.latestDisplayFrame.value

    private var pendingExportFile: File? = null
    private lateinit var exportFileLauncher: ActivityResultLauncher<String>
    private lateinit var sendFileLauncher: ActivityResultLauncher<Array<String>>
    private lateinit var notificationPermissionLauncher: ActivityResultLauncher<String>
    private var pendingSendHandle: Long? = null
    private var updateStatus by mutableStateOf("Not checked yet.")

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // The foreground service owns discovery, trusted sessions and transfers.
        // Starting it while the Activity is visible is allowed on modern Android.
        startForegroundService(android.content.Intent(this, BridgeConnectionService::class.java))

        notificationPermissionLauncher = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }
        sendFileLauncher = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
            val handle = pendingSendHandle
            pendingSendHandle = null
            if (uri != null && handle != null) {
                BridgeConnectionService.instance?.sendFile(uri, handle)
            }
        }
        exportFileLauncher = registerForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri: Uri? ->
            val source = pendingExportFile
            pendingExportFile = null
            if (uri != null && source != null) {
                Thread({
                    try {
                        contentResolver.openOutputStream(uri)?.use { output ->
                            source.inputStream().use { input -> input.copyTo(output) }
                        } ?: error("Could not open the selected destination.")
                        runOnUiThread { BridgeRuntimeState.transferStatus.value = "Exported ${source.name} successfully." }
                    } catch (error: Exception) {
                        runOnUiThread { BridgeRuntimeState.transferStatus.value = "Could not export ${source.name}: ${error.message}" }
                    }
                }, "BridgeFileExport").apply { isDaemon = true; start() }
            }
        }

        setContent {
            BridgeApp(
                discoveredPeers = discoveredPeers,
                pendingPairings = pendingPairings,
                trustedDevices = trustedDevices,
                connectedSessionHandles = connectedSessionHandles,
                pairingStatus = pairingStatus,
                incomingOffers = incomingOffers,
                transferStatus = transferStatus,
                latestDisplayFrame = latestDisplayFrame,
                onImmersiveModeChange = { setSecondScreenImmersive(it) },
                onDisplayInput = { deviceId, action, pointerId, x, y ->
                    BridgeConnectionService.instance?.sendDisplayInput(deviceId, action, pointerId, x, y)
                },
                receivedFiles = receivedFiles,
                clipboardReceivedText = clipboardReceivedText,
                clipboardReceivedFrom = clipboardReceivedFrom,
                clipboardStatus = clipboardStatus,
                updateStatus = updateStatus,
                onCheckUpdates = { channel ->
                    BridgeUpdater.checkAndInstall(this, channel) { message -> updateStatus = message }
                },
                onExportFile = { file -> pendingExportFile = file; exportFileLauncher.launch(file.name) },
                onPickFile = { handle -> pendingSendHandle = handle; sendFileLauncher.launch(arrayOf("*/*")) },
                onCancelTransfer = { BridgeConnectionService.instance?.cancelTransfers() },
                onSendClipboard = { text -> BridgeConnectionService.instance?.sendClipboard(text) },
                onAcceptTransfer = { offer ->
                    BridgeConnectionService.instance?.acceptTransfer(offer)
                    incomingOffers.remove(offer)
                },
                onRejectTransfer = { offer ->
                    BridgeConnectionService.instance?.rejectTransfer(offer)
                    incomingOffers.remove(offer)
                },
                onPair = { peer -> BridgeConnectionService.instance?.pair(peer) },
                onApprove = { pending ->
                    BridgeConnectionService.instance?.approve(pending)
                    pendingPairings.remove(pending)
                    trustedDevices.clear()
                    trustedDevices.addAll(BridgeConnectionService.instance?.trustedDevices().orEmpty())
                },
                onReject = { pending ->
                    BridgeConnectionService.instance?.reject(pending)
                    pendingPairings.remove(pending)
                },
                onRevoke = { deviceId ->
                    BridgeConnectionService.instance?.revoke(deviceId)
                    trustedDevices.clear()
                    trustedDevices.addAll(BridgeConnectionService.instance?.trustedDevices().orEmpty())
                }
            )
        }

        // Ask for notification permission so Android can show the persistent
        // foreground-service notification in the notification drawer.
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            notificationPermissionLauncher.launch(android.Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    private fun setSecondScreenImmersive(enabled: Boolean) {
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            window.insetsController?.let { controller ->
                if (enabled) {
                    controller.systemBarsBehavior =
                        android.view.WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                    controller.hide(android.view.WindowInsets.Type.systemBars())
                } else {
                    controller.show(android.view.WindowInsets.Type.systemBars())
                }
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = if (enabled) {
                (android.view.View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    or android.view.View.SYSTEM_UI_FLAG_FULLSCREEN
                    or android.view.View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    or android.view.View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    or android.view.View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    or android.view.View.SYSTEM_UI_FLAG_LAYOUT_STABLE)
            } else {
                android.view.View.SYSTEM_UI_FLAG_VISIBLE
            }
        }
    }

    override fun onResume() {
        super.onResume()
        BridgeConnectionService.instance?.refreshReceivedFiles()
    }

    override fun onDestroy() {
        setSecondScreenImmersive(false)
        super.onDestroy()
    }

    // Deliberately do not stop networking here. Android may destroy this Activity
    // when the user dismisses its task; the foreground service remains active.
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
    connectedSessionHandles: Map<String, Long>,
    pairingStatus: String,
    incomingOffers: List<BridgeIncomingOffer>,
    transferStatus: String,
    latestDisplayFrame: BridgeDisplayFrame?,
    onImmersiveModeChange: (Boolean) -> Unit,
    onDisplayInput: (String, Int, Int, Int, Int) -> Unit,
    receivedFiles: List<File>,
    clipboardReceivedText: String?,
    clipboardReceivedFrom: String,
    clipboardStatus: String,
    onSendClipboard: (String) -> Unit,
    onExportFile: (File) -> Unit,
    onPickFile: (Long) -> Unit,
    onCancelTransfer: () -> Unit,
    onAcceptTransfer: (BridgeIncomingOffer) -> Unit,
    onRejectTransfer: (BridgeIncomingOffer) -> Unit,
    onPair: (BridgePeer) -> Unit,
    onApprove: (BridgePendingPairing) -> Unit,
    onReject: (BridgePendingPairing) -> Unit,
    onRevoke: (String) -> Unit,
    updateStatus: String,
    onCheckUpdates: (String) -> Unit
) {
    val context = LocalContext.current
    val updatePreferences = remember { context.getSharedPreferences("bridge_settings", android.content.Context.MODE_PRIVATE) }
    var updateChannel by remember {
        mutableStateOf(updatePreferences.getString("update_channel", "development") ?: "development")
    }
    var selected by remember { mutableStateOf(BridgeTab.Home) }
    var immersiveSecondScreen by remember { mutableStateOf(false) }
    var showPairDialog by remember { mutableStateOf(false) }
    var clipboardSync by remember { mutableStateOf(updatePreferences.getBoolean("clipboard_sync_enabled", false)) }
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
                if (!immersiveSecondScreen) HeaderBar()
                Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                    when (selected) {
                        BridgeTab.Home -> HomeScreen(
                            onPair = { showPairDialog = true },
                            onNavigate = { selected = it }
                        )
                        BridgeTab.Devices -> DevicesScreen(
                            peers = discoveredPeers,
                            trusted = trustedDevices,
                            pairingStatus = pairingStatus,
                            onPair = onPair,
                            onApprove = onApprove,
                            onReject = onReject,
                            onRevoke = onRevoke
                        )
                        BridgeTab.Transfers -> TransfersScreen(
                            incomingOffers = incomingOffers,
                            trustedDevices = trustedDevices,
                            connectedSessionHandles = connectedSessionHandles,
                            onPickFile = onPickFile,
                            onCancelTransfer = onCancelTransfer,
                            transferStatus = transferStatus,
                            receivedFiles = receivedFiles,
                            onExportFile = onExportFile,
                            onAccept = onAcceptTransfer,
                            onReject = onRejectTransfer,
                            onPair = { showPairDialog = true }
                        )
                        BridgeTab.Clipboard -> ClipboardScreen(
                            enabled = clipboardSync,
                            onEnabledChange = {
                                clipboardSync = it
                                updatePreferences.edit().putBoolean("clipboard_sync_enabled", it).apply()
                                BridgeRuntimeState.clipboardStatus.value = if (it) "Clipboard sharing enabled." else "Clipboard sharing paused."
                            },
                            receivedText = clipboardReceivedText,
                            receivedFrom = clipboardReceivedFrom,
                            status = clipboardStatus,
                            onSend = onSendClipboard
                        )
                        BridgeTab.SecondScreen -> SecondScreenView(
                            frame = latestDisplayFrame,
                            connectedDevices = connectedSessionHandles.size,
                            immersive = immersiveSecondScreen,
                            onImmersiveChange = { enabled ->
                                immersiveSecondScreen = enabled
                                onImmersiveModeChange(enabled)
                            },
                            onDisplayInput = onDisplayInput
                        )
                        BridgeTab.Tools -> ToolsScreen()
                        BridgeTab.Settings -> SettingsScreen(
                            notifications = notifications,
                            onNotificationsChange = { notifications = it },
                            autoAccept = autoAccept,
                            onAutoAcceptChange = { autoAccept = it },
                            updateChannel = updateChannel,
                            onUpdateChannelChange = {
                                updateChannel = it
                                updatePreferences.edit().putString("update_channel", it).apply()
                            },
                            updateStatus = updateStatus,
                            onCheckUpdates = { onCheckUpdates(updateChannel) }
                        )
                    }
                }
                if (!immersiveSecondScreen) {
                    BottomNavigation(selected = selected, onSelect = {
                        if (immersiveSecondScreen) {
                            immersiveSecondScreen = false
                            onImmersiveModeChange(false)
                        }
                        selected = it
                    })
                }
            }
        }
    }

    if (pendingPairings.isNotEmpty()) {
        val request = pendingPairings.first()
        AlertDialog(
            onDismissRequest = {},
            containerColor = PanelAlt,
            title = { Text("Pairing request", color = Color.White, fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("${request.name} wants to connect to this device.", color = Color.White, fontSize = 14.sp)
                    Text("${request.platform} · authenticated encrypted connection", color = Muted, fontSize = 11.sp)
                    Text("Only accept if you recognize this device.", color = Amber, fontSize = 11.sp)
                }
            },
            confirmButton = {
                Button(onClick = { onApprove(request) }, colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)) {
                    Text("Accept", fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                Button(onClick = { onReject(request) }, colors = ButtonDefaults.buttonColors(containerColor = Panel, contentColor = Amber)) {
                    Text("Reject")
                }
            }
        )
    }

    val incomingOffer = incomingOffers.firstOrNull()
    if (incomingOffer != null) {
        val senderName = trustedDevices.firstOrNull { it.deviceId == incomingOffer.deviceId }?.name
            ?: "Trusted device " + incomingOffer.deviceId.take(8)
        AlertDialog(
            onDismissRequest = {},
            containerColor = PanelAlt,
            title = { Text("Incoming file", color = Color.White, fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("$senderName wants to send you a file.", color = Color.White, fontSize = 13.sp)
                    Text(incomingOffer.fileName, color = Accent, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text(
                        if (incomingOffer.fileSize == 0L) "Empty file"
                        else "%.2f MB".format(incomingOffer.fileSize / 1048576.0),
                        color = Muted,
                        fontSize = 12.sp
                    )
                    Text("The file is encrypted in transit and its SHA-256 checksum is verified before saving.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                    Text("Accept only if you trust the sender and expected this file.", color = Amber, fontSize = 11.sp)
                }
            },
            confirmButton = {
                Button(
                    onClick = { onAcceptTransfer(incomingOffer) },
                    colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)
                ) { Text("Accept file", fontWeight = FontWeight.Bold) }
            },
            dismissButton = {
                Button(
                    onClick = { onRejectTransfer(incomingOffer) },
                    colors = ButtonDefaults.buttonColors(containerColor = Panel, contentColor = Amber)
                ) { Text("Decline") }
            }
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
                    Text("Choose a nearby Bridge device. It will receive a secure pairing request and can accept or reject it on that device.", color = Color.White, fontSize = 13.sp, lineHeight = 19.sp)
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

@OptIn(ExperimentalComposeUiApi::class)
@Composable
private fun SecondScreenView(
    frame: BridgeDisplayFrame?,
    connectedDevices: Int,
    immersive: Boolean,
    onImmersiveChange: (Boolean) -> Unit,
    onDisplayInput: (String, Int, Int, Int, Int) -> Unit
) {
    var imageSize by remember { mutableStateOf(IntSize.Zero) }
    val activePointers = remember { mutableSetOf<Int>() }
    val bitmap = remember(frame?.sequence, frame?.width, frame?.height) {
        frame?.let {
            Bitmap.createBitmap(it.width, it.height, Bitmap.Config.ARGB_8888).also { image ->
                image.copyPixelsFromBuffer(ByteBuffer.wrap(it.pixels))
            }.asImageBitmap()
        }
    }

    if (immersive) {
        Column(
            modifier = Modifier.fillMaxSize().background(Color.Black).padding(8.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(
                        if (frame == null) "WAITING FOR WINDOWS" else "BRIDGE SECOND SCREEN",
                        color = if (frame == null) Muted else Accent,
                        fontSize = 10.sp,
                        fontWeight = FontWeight.Bold,
                        letterSpacing = 1.sp
                    )
                    Text(
                        if (frame == null) "No display frame yet" else "${frame.width} × ${frame.height} · Frame ${frame.sequence}",
                        color = Color.White,
                        fontSize = 11.sp
                    )
                }
                Button(
                    onClick = { onImmersiveChange(false) },
                    colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Color.White),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp)
                ) { Text("Exit", fontSize = 12.sp) }
            }
            Box(
                modifier = Modifier.weight(1f).fillMaxWidth().background(Color.Black),
                contentAlignment = Alignment.Center
            ) {
                if (bitmap != null) {
                    Image(
                        bitmap = bitmap,
                        contentDescription = "Windows secondary display",
                        contentScale = ContentScale.Fit,
                        modifier = Modifier.fillMaxSize().onSizeChanged { imageSize = it }.pointerInteropFilter { event ->
                            val currentFrame = frame ?: return@pointerInteropFilter true
                            val imageWidth = currentFrame.width.toFloat()
                            val imageHeight = currentFrame.height.toFloat()
                            val viewWidth = imageSize.width.toFloat().coerceAtLeast(1f)
                            val viewHeight = imageSize.height.toFloat().coerceAtLeast(1f)
                            val scale = minOf(viewWidth / imageWidth, viewHeight / imageHeight)
                            val drawnWidth = imageWidth * scale
                            val drawnHeight = imageHeight * scale
                            val left = (viewWidth - drawnWidth) / 2f
                            val top = (viewHeight - drawnHeight) / 2f
                            fun send(index: Int, action: Int) {
                                val pointerId = event.getPointerId(index)
                                if (pointerId !in 0..9) return
                                val rawX = event.getX(index)
                                val rawY = event.getY(index)
                                if (action == 1 && (rawX < left || rawX > left + drawnWidth ||
                                    rawY < top || rawY > top + drawnHeight)) return
                                val x = (((rawX - left) / drawnWidth).coerceIn(0f, 1f) * 65535f).toInt()
                                val y = (((rawY - top) / drawnHeight).coerceIn(0f, 1f) * 65535f).toInt()
                                onDisplayInput(currentFrame.deviceId, action, pointerId, x, y)
                            }
                            when (event.actionMasked) {
                                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                                    val index = event.actionIndex
                                    val id = event.getPointerId(index)
                                    val before = activePointers.contains(id)
                                    send(index, 1)
                                    if (!before && id in 0..9) {
                                        val x = event.getX(index)
                                        val y = event.getY(index)
                                        if (x >= left && x <= left + drawnWidth && y >= top && y <= top + drawnHeight) {
                                            activePointers.add(id)
                                        }
                                    }
                                }
                                MotionEvent.ACTION_MOVE -> {
                                    for (index in 0 until event.pointerCount) {
                                        if (activePointers.contains(event.getPointerId(index))) send(index, 2)
                                    }
                                }
                                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                                    val index = event.actionIndex
                                    val id = event.getPointerId(index)
                                    if (activePointers.remove(id)) send(index, 3)
                                }
                                MotionEvent.ACTION_CANCEL -> {
                                    activePointers.toList().forEach { id ->
                                        if (id in 0..9) onDisplayInput(currentFrame.deviceId, 4, id, 0, 0)
                                    }
                                    activePointers.clear()
                                }
                            }
                            true
                        }
                    )
                } else {
                    Text(
                        if (connectedDevices == 0) "Connect a trusted Windows PC first."
                        else "Waiting for the Windows display stream…",
                        color = Muted,
                        fontSize = 12.sp
                    )
                }
            }
        }
    } else {
        ScreenContent {
            Eyebrow("WIRELESS DISPLAY")
            ScreenHeading(
                "SECOND SCREEN",
                "Your phone as a monitor",
                "Windows sends captured display frames over your trusted encrypted connection."
            )
            Surface(color = Panel, shape = RoundedCornerShape(14.dp), modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text(
                            if (frame == null) "Waiting for Windows frames" else "Live frame received",
                            color = Color.White,
                            fontWeight = FontWeight.Bold,
                            fontSize = 14.sp
                        )
                        Text(
                            if (frame == null) "IDLE" else "RECEIVING",
                            color = if (frame == null) Muted else Good,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold
                        )
                    }
                    if (bitmap != null) {
                        Image(
                            bitmap = bitmap,
                            contentDescription = "Windows secondary display",
                            contentScale = ContentScale.Fit,
                            modifier = Modifier.fillMaxWidth().background(Color.Black, RoundedCornerShape(8.dp))
                        )
                        Text("${frame!!.width} × ${frame.height} · Frame ${frame.sequence}", color = Muted, fontSize = 11.sp)
                    } else {
                        Box(
                            Modifier.fillMaxWidth().height(190.dp).background(Color.Black, RoundedCornerShape(8.dp)),
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                if (connectedDevices == 0) "Connect a trusted Windows PC first."
                                else "Waiting for the Windows display stream…",
                                color = Muted,
                                fontSize = 12.sp
                            )
                        }
                    }
                    Button(
                        onClick = { onImmersiveChange(true) },
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)
                    ) { Text("Open full-screen display", fontWeight = FontWeight.Bold) }
                    Text(
                        "Touch and multi-pointer events are forwarded over the trusted encrypted session. Windows-side input handling is not available yet.",
                        color = Muted,
                        fontSize = 11.sp,
                        lineHeight = 16.sp
                    )
                }
            }
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
private fun PrimaryAction(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true
) {
    Button(
        onClick = onClick,
        modifier = modifier.height(46.dp),
        enabled = enabled,
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
    trusted: List<BridgeTrustedDevice>,
    pairingStatus: String,
    onPair: (BridgePeer) -> Unit,
    onApprove: (BridgePendingPairing) -> Unit,
    onReject: (BridgePendingPairing) -> Unit,
    onRevoke: (String) -> Unit
) {
    val trustedIds = trusted.map { it.deviceId }.toSet()
    val untrustedPeers = peers.filterNot { it.deviceId in trustedIds }
    ScreenContent {
        ScreenHeading("YOUR ECOSYSTEM", "Devices", "Nearby Bridge devices appear here automatically.")
        Panel {
            Eyebrow("DISCOVERED DEVICES")
            Text("Available on your network", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold)
            Text("Keep Bridge open on both devices and connect them to the same Wi-Fi network.", color = Muted, fontSize = 12.sp, lineHeight = 18.sp)
            if (untrustedPeers.isEmpty()) {
                Text("Searching for Bridge devices…", color = Muted, fontSize = 11.sp)
            } else {
                untrustedPeers.forEach { peer ->
                    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(peer.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("${peer.platform} · Nearby", color = Muted, fontSize = 10.sp)
                        }
                        PrimaryAction("Connect", { onPair(peer) })
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
                    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(device.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("${device.platform} · ${device.fingerprint.take(12)}", color = Muted, fontSize = 10.sp)
                        }
                        Button(onClick = { onRevoke(device.deviceId) }, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                            Text("Revoke")
                        }
                    }
                }
            }
        }
        Text(pairingStatus, color = Amber, fontSize = 11.sp, lineHeight = 16.sp)
    }
}

@Composable
private fun TransfersScreen(
    incomingOffers: List<BridgeIncomingOffer>,
    trustedDevices: List<BridgeTrustedDevice>,
    connectedSessionHandles: Map<String, Long>,
    onPickFile: (Long) -> Unit,
    onCancelTransfer: () -> Unit,
    transferStatus: String,
    receivedFiles: List<File>,
    onExportFile: (File) -> Unit,
    onAccept: (BridgeIncomingOffer) -> Unit,
    onReject: (BridgeIncomingOffer) -> Unit,
    onPair: () -> Unit
) {
    ScreenContent {
        ScreenHeading("MOVE WITHOUT LIMITS", "File transfers", "Receive files over your authenticated, encrypted local connection.")
        Panel {
            Eyebrow("SEND A FILE")
            val connected = trustedDevices.filter { connectedSessionHandles.containsKey(it.deviceId) }
            if (connected.isEmpty()) {
                Text("Connect a trusted device first. Online devices will appear here.", color = Muted, fontSize = 12.sp)
            } else {
                Text("Choose a connected device, then select a file from your phone.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                connected.forEach { device ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(device.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("Secure connection ready", color = Good, fontSize = 10.sp)
                        }
                        PrimaryAction("Choose file", { connectedSessionHandles[device.deviceId]?.let(onPickFile) })
                    }
                    DividerLine()
                }
            }
        }
        Panel {
            Eyebrow("INCOMING REQUESTS")
            if (incomingOffers.isEmpty()) {
                Text("No incoming file requests.", color = Muted, fontSize = 12.sp)
            } else {
                incomingOffers.forEach { offer ->
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(offer.fileName, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Bold)
                        Text("${offer.deviceId} · ${"%.2f".format(offer.fileSize / 1048576.0)} MB", color = Muted, fontSize = 10.sp)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            PrimaryAction("Accept", { onAccept(offer) }, Modifier.weight(1f))
                            Button(onClick = { onReject(offer) }, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                                Text("Decline")
                            }
                        }
                    }
                    DividerLine()
                }
            }
        }
        Panel {
            Eyebrow("RECEIVED FILES")
            if (receivedFiles.isEmpty()) {
                Text("Files you accept will appear here.", color = Muted, fontSize = 12.sp)
            } else {
                receivedFiles.forEach { file ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp)
                    ) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(file.name, color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text("%.2f MB".format(file.length() / 1048576.0), color = Muted, fontSize = 10.sp)
                        }
                        Button(onClick = { onExportFile(file) }, colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)) {
                            Text("Save to…", fontWeight = FontWeight.Bold)
                        }
                    }
                    DividerLine()
                }
            }
        }
        Panel {
            Eyebrow("LIVE TRANSFER ACTIVITY")
            Text(transferStatus, color = Color.White, fontSize = 12.sp, lineHeight = 18.sp)
            val transferPercent = Regex("(\\d+)%").find(transferStatus)?.groupValues?.get(1)?.toFloatOrNull()?.div(100f)
            if (transferPercent != null) {
                Box(modifier = Modifier.fillMaxWidth().height(8.dp).background(PanelAlt, RoundedCornerShape(8.dp))) {
                    Box(modifier = Modifier.fillMaxWidth(transferPercent.coerceIn(0f, 1f)).fillMaxHeight().background(Accent, RoundedCornerShape(8.dp)))
                }
            }
            if (transferStatus.startsWith("Sending ") || transferStatus.startsWith("Receiving ") || transferStatus.startsWith("Waiting for the receiving device")) {
                Button(onClick = onCancelTransfer, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                    Text("Cancel transfer")
                }
            }
            Text("Received files stay private until you choose Save to… and select a destination such as Downloads.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
        }
        Panel {
            FeatureLine("◇", "Encrypted in transit", "Uses the authenticated Noise session.", "ACTIVE")
            DividerLine()
            FeatureLine("✓", "SHA-256 integrity", "Files are checked before being published.", "ACTIVE")
            DividerLine()
            FeatureLine("▣", "No overwrite", "Existing files are kept; received files get a numbered name.", "ACTIVE")
        }
        PrimaryAction("＋   Pair a device", onPair, Modifier.fillMaxWidth())
    }
}

@Composable
private fun ClipboardScreen(
    enabled: Boolean,
    onEnabledChange: (Boolean) -> Unit,
    receivedText: String?,
    receivedFrom: String,
    status: String,
    onSend: (String) -> Unit
) {
    val context = LocalContext.current
    var draft by remember { mutableStateOf("") }
    ScreenContent {
        ScreenHeading("SHARE TEXT PRIVATELY", "Universal clipboard", "Send text between your connected, trusted devices.")
        Panel {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    Text("Clipboard sharing", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text("Sharing is manual and only works with trusted connected devices.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                }
                Switch(checked = enabled, onCheckedChange = onEnabledChange)
            }
            StatusTag(if (enabled) "SHARING ENABLED" else "SHARING PAUSED", if (enabled) Accent else Muted)
        }
        Panel {
            Eyebrow("SEND TEXT")
            OutlinedTextField(
                value = draft,
                onValueChange = { if (it.toByteArray(Charsets.UTF_8).size <= 16 * 1024) draft = it },
                enabled = enabled,
                modifier = Modifier.fillMaxWidth(),
                minLines = 3,
                maxLines = 5,
                label = { Text("Text to share") }
            )
            PrimaryAction("Send to connected devices", {
                if (enabled && draft.isNotBlank()) {
                    onSend(draft)
                    draft = ""
                }
            }, Modifier.fillMaxWidth(), enabled = enabled)
        }
        Panel {
            Eyebrow("RECEIVED TEXT")
            Text(receivedFrom, color = Muted, fontSize = 10.sp)
            if (receivedText.isNullOrEmpty()) {
                Text("No shared text received yet.", color = Muted, fontSize = 12.sp)
            } else {
                Text(receivedText, color = Color.White, fontSize = 13.sp, lineHeight = 19.sp)
                PrimaryAction("Copy to phone clipboard", {
                    context.getSystemService(android.content.ClipboardManager::class.java)
                        ?.setPrimaryClip(android.content.ClipData.newPlainText("Bridge shared text", receivedText))
                }, Modifier.fillMaxWidth())
            }
            Text(status, color = Accent, fontSize = 11.sp, lineHeight = 16.sp)
        }
        Text("Bridge does not monitor your system clipboard. You choose what to send; received text is copied only when you tap the button.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
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
    onAutoAcceptChange: (Boolean) -> Unit,
    updateChannel: String,
    onUpdateChannelChange: (String) -> Unit,
    updateStatus: String,
    onCheckUpdates: () -> Unit
) {
    ScreenContent {
        ScreenHeading("MAKE IT YOURS", "Settings", "Privacy and preferences for your Bridge workspace.")
        Panel {
            Eyebrow("THIS DEVICE")
            Text("Android device", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("Bridge mobile · Version ${BuildConfig.VERSION_NAME}", color = Muted, fontSize = 11.sp)
            StatusTag("NOT PAIRED")
        }
        Panel {
            Eyebrow("PREFERENCES")
            PreferenceSwitch("Notifications", "Show Bridge activity notifications.", notifications, onNotificationsChange)
            DividerLine()
            PreferenceSwitch("Auto-accept trusted devices", "Skip confirmation for trusted requests.", autoAccept, onAutoAcceptChange)
        }
        Panel {
            Eyebrow("SOFTWARE UPDATES")
            Text("Update channel", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("development" to "Development", "stable" to "Stable").forEach { (value, label) ->
                    Button(
                        onClick = { onUpdateChannelChange(value) },
                        colors = ButtonDefaults.buttonColors(
                            containerColor = if (updateChannel == value) Accent else PanelAlt,
                            contentColor = if (updateChannel == value) Ink else Color.White
                        )
                    ) { Text(label, fontSize = 11.sp, fontWeight = FontWeight.Bold) }
                }
            }
            Text(
                if (updateChannel == "stable") "Stable releases will appear here when the Stable channel launches."
                else "Development builds contain the newest features and may be less stable.",
                color = Muted, fontSize = 10.sp, lineHeight = 15.sp
            )
            Text(updateStatus, color = Amber, fontSize = 11.sp, lineHeight = 16.sp)
            PrimaryAction("Check for updates", onCheckUpdates, Modifier.fillMaxWidth())
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
