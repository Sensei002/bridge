package app.bridge.android

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.os.Handler
import android.os.Looper
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/** Verifies a GitHub release APK before handing installation to Android. */
object BridgeUpdater {
    private const val API = "https://api.github.com/repos/Sensei002/bridge/releases?per_page=30"
    private const val RELEASE_PREFIX = "https://github.com/Sensei002/bridge/releases/download/"
    private val mainHandler = Handler(Looper.getMainLooper())

    fun checkAndInstall(activity: Activity, channel: String, onStatus: (String) -> Unit) {
        val selectedChannel = if (channel == "stable") "stable" else "development"
        onStatus("Checking the $selectedChannel channel…")
        Thread({
            try {
                val releases = fetchJsonArray(API)
                val release = (0 until releases.length()).map { releases.getJSONObject(it) }.firstOrNull {
                    !it.optBoolean("draft", true) &&
                        it.optBoolean("prerelease", false) == (selectedChannel == "development")
                } ?: throw IllegalStateException(
                    if (selectedChannel == "stable") "No Stable release is available yet." else "No Development release is available yet."
                )

                val assets = release.getJSONArray("assets")
                var manifestUrl: String? = null
                for (i in 0 until assets.length()) {
                    val asset = assets.getJSONObject(i)
                    if (asset.optString("name") == "bridge-update.json") {
                        manifestUrl = asset.optString("browser_download_url")
                        break
                    }
                }
                val safeManifestUrl = manifestUrl?.takeIf { it.startsWith(RELEASE_PREFIX) }
                    ?: throw IllegalStateException("No official Bridge update manifest was found.")
                val manifest = fetchJson(safeManifestUrl)
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
                require(downloadUrl.startsWith(RELEASE_PREFIX)) { "APK URL is not an official Bridge release asset." }
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

    private fun fetchJsonArray(url: String): JSONArray {
        val connection = open(url)
        return try { JSONArray(connection.inputStream.bufferedReader().use { it.readText() }) }
        finally { connection.disconnect() }
    }

    private fun fetchJson(url: String): JSONObject {
        val connection = open(url)
        return try { JSONObject(connection.inputStream.bufferedReader().use { it.readText() }) }
        finally { connection.disconnect() }
    }

    private fun open(url: String): HttpURLConnection {
        val parsed = URL(url)
        require(parsed.protocol == "https" && parsed.host in setOf("api.github.com", "github.com")) {
            "Only official GitHub HTTPS endpoints are allowed."
        }
        return (parsed.openConnection() as HttpURLConnection).apply {
            connectTimeout = 15000
            readTimeout = 30000
            setRequestProperty("User-Agent", "Bridge-Android-Updater")
            setRequestProperty("Accept", "application/vnd.github+json")
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
