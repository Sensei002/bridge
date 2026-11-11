package app.bridge.android

import android.content.Context
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Persistent, bounded diagnostics. Never records clipboard contents or file data. */
internal object BridgeDiagnostics {
    private const val MAX_BYTES = 512L * 1024L
    @Volatile private var logFile: File? = null
    private val lock = Any()
    private val formatter = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSSZ", Locale.US)

    fun initialize(context: Context) {
        synchronized(lock) {
            val directory = File(context.filesDir, "Bridge/Logs")
            if (directory.exists() || directory.mkdirs()) {
                logFile = File(directory, "android.log")
                recordLocked("app", "Diagnostic logging initialized")
            }
        }
    }

    fun record(component: String, message: String) {
        synchronized(lock) { recordLocked(component, message) }
    }

    private fun recordLocked(component: String, message: String) {
        val file = logFile ?: return
        try {
            file.parentFile?.mkdirs()
            val safe = message.replace('\r', ' ').replace('\n', ' ')
            file.appendText("${formatter.format(Date())} [${component.take(32)}] ${safe.take(4000)}\n")
            if (file.length() > MAX_BYTES) {
                val text = file.readText()
                val keepFrom = text.length / 2
                val newline = text.indexOf('\n', keepFrom).let { if (it < 0) keepFrom else it + 1 }
                file.writeText(text.substring(newline))
            }
        } catch (_: Exception) {
            // Diagnostics must never interfere with Bridge networking or rendering.
        }
    }

    fun snapshot(): String {
        synchronized(lock) {
            val file = logFile
            val contents = try {
                if (file != null && file.isFile) file.readText().takeLast(MAX_BYTES.toInt()) else ""
            } catch (_: Exception) { "" }
            return "Bridge Android diagnostics\nDevice: ${android.os.Build.MANUFACTURER} ${android.os.Build.MODEL}\nVersion: ${BuildConfig.VERSION_NAME}\n${contents.ifBlank { "No diagnostic events recorded yet." }}"
        }
    }
}
