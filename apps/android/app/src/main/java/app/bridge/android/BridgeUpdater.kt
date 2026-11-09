package app.bridge.android

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.os.Handler
import android.os.Looper
import android.util.Xml
import androidx.core.content.FileProvider
import org.json.JSONObject
import org.xmlpull.v1.XmlPullParser
import java.io.File
import java.io.FileOutputStream
import java.io.StringReader
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/** Verifies a GitHub release APK before handing installation to Android. */
object BridgeUpdater {
    private const val RELEASES_FEED = "https://github.com/Sensei002/bridge/releases.atom"
    private const val RELEASE_PAGE_PREFIX = "https://github.com/Sensei002/bridge/releases/tag/"
    private const val RELEASE_PREFIX = "https://github.com/Sensei002/bridge/releases/download/"
    private val mainHandler = Handler(Looper.getMainLooper())

    fun checkAndInstall(activity: Activity, channel: String, onStatus: (String) -> Unit) {
        val selectedChannel = if (channel == "stable") "stable" else "development"
        onStatus("Checking the $selectedChannel channel…")
        Thread({
            try {
                val tag = latestReleaseTag(selectedChannel)
                val manifestUrl = "${RELEASE_PREFIX}${tag}/bridge-update.json"
                val manifest = fetchJson(manifestUrl)
                require(manifest.optString("channel") == selectedChannel) { "Release channel mismatch." }
                val android = manifest.optJSONObject("android")
                    ?: throw IllegalStateException("This release does not include an Android APK.")
                require(android.optString("application_id") == activity.packageName) {
                    "The release APK belongs to a different Bridge application identity."
                }
                val versionCode = android.optInt("version_code", 0)
                if (versionCode <= BuildConfig.VERSION_CODE) {
                    postStatus(onStatus, "Bridge is up to date (${BuildConfig.VERSION_NAME}).")
                    return@Thread
                }
                val downloadUrl = android.optString("download_url")
                require(downloadUrl.startsWith("${RELEASE_PREFIX}${tag}/")) {
                    "APK URL is not an asset from the selected official Bridge release."
                }
                val expectedHash = android.optString("sha256").lowercase()
                require(expectedHash.matches(Regex("[0-9a-f]{64}"))) { "Invalid APK checksum in release manifest." }

                postStatus(onStatus, "Downloading Bridge ${android.optString("version_name")}…")
                val assetName = android.optString("asset")
                require(assetName == File(assetName).name && assetName.endsWith(".apk", ignoreCase = true)) {
                    "APK filename in the release manifest is invalid."
                }
                val updateDir = File(activity.cacheDir, "updates").apply { mkdirs() }
                val apk = File(updateDir, assetName)
                download(downloadUrl, apk)
                if (!sha256(apk).equals(expectedHash, ignoreCase = true)) {
                    apk.delete()
                    throw SecurityException("APK checksum verification failed. Update blocked.")
                }
                mainHandler.post {
                    try {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
                            !activity.packageManager.canRequestPackageInstalls()
                        ) {
                            val settingsIntent = Intent(
                                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                                Uri.parse("package:${activity.packageName}")
                            )
                            activity.startActivity(settingsIntent)
                            onStatus("Allow Bridge to install apps from this source, then return and check for updates again.")
                            return@post
                        }
                        val uri: Uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", apk)
                        val intent = Intent(Intent.ACTION_VIEW).apply {
                            setDataAndType(uri, "application/vnd.android.package-archive")
                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        }
                        activity.startActivity(intent)
                        onStatus("Verified Bridge ${android.optString("version_name")}. Confirm installation in Android.")
                    } catch (error: Exception) {
                        onStatus("Verified download, but Android could not open the installer: ${error.message}")
                    }
                }
            } catch (error: Exception) {
                postStatus(onStatus, "Update check failed: ${error.message ?: error.javaClass.simpleName}")
            }
        }, "BridgeUpdater").apply { isDaemon = true; start() }
    }

    private fun latestReleaseTag(channel: String): String {
        val feed = fetchText(RELEASES_FEED)
        val parser = Xml.newPullParser()
        parser.setInput(StringReader(feed))
        val expectedPrefix = if (channel == "development") "dev-" else "stable-"
        var event = parser.eventType
        while (event != XmlPullParser.END_DOCUMENT) {
            if (event == XmlPullParser.START_TAG && parser.name == "link") {
                val href = parser.getAttributeValue(null, "href") ?: ""
                if (href.startsWith(RELEASE_PAGE_PREFIX)) {
                    val tag = href.removePrefix(RELEASE_PAGE_PREFIX)
                    if (tag.startsWith(expectedPrefix) &&
                        tag.length > expectedPrefix.length &&
                        tag.removePrefix(expectedPrefix).all { it in '0'..'9' }
                    ) return tag
                }
            }
            event = parser.next()
        }
        throw IllegalStateException("No $channel release is available yet.")
    }

    private fun fetchText(url: String): String {
        val connection = open(url)
        return try { connection.inputStream.bufferedReader().use { it.readText() } }
        finally { connection.disconnect() }
    }

    private fun fetchJson(url: String): JSONObject = JSONObject(fetchText(url))

    private fun open(url: String): HttpURLConnection {
        val parsed = URL(url)
        require(parsed.protocol == "https" && parsed.host == "github.com") {
            "Only official GitHub HTTPS endpoints are allowed."
        }
        return (parsed.openConnection() as HttpURLConnection).apply {
            connectTimeout = 15000
            readTimeout = 30000
            setRequestProperty("User-Agent", "Bridge-Android-Updater")
            instanceFollowRedirects = true
            if (responseCode !in 200..299) {
                disconnect()
                throw IllegalStateException("GitHub returned HTTP $responseCode.")
            }
        }
    }

    private fun download(url: String, destination: File) {
        val connection = open(url)
        try {
            connection.inputStream.use { input ->
                FileOutputStream(destination).use { output -> input.copyTo(output) }
            }
        } finally { connection.disconnect() }
    }

    private fun sha256(file: File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) }
    }

    private fun postStatus(callback: (String) -> Unit, message: String) {
        mainHandler.post { callback(message) }
    }
}
