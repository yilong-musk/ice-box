// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageInstaller
import android.net.Uri
import android.net.VpnService
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.result.ActivityResult
import app.tauri.PermissionState
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.RandomAccessFile
import org.json.JSONObject

@TauriPlugin(
    permissions = [
        Permission(
            strings = [Manifest.permission.POST_NOTIFICATIONS],
            alias = "notifications",
        ),
    ],
)
class TunnelPlugin(private val activity: Activity) : Plugin(activity) {
    private val statusLock = Any()
    private var lastStatus: JSONObject? = null
    private var pendingExport: ByteArray? = null
    // Keep these strings aligned with TunnelFiles.kt in the app module.
    // They stay inside this class so the two modules can share a package
    // without clashing top-level names.
    private companion object {
        const val ACTION_START = "com.yilongmusk.icebox.START"
        const val ACTION_STOP = "com.yilongmusk.icebox.STOP"
        const val ACTION_RELOAD = "com.yilongmusk.icebox.RELOAD"
        const val SERVICE_CLASS = "com.yilongmusk.icebox.tunnel.TunnelService"
        const val STATUS_FILE = "tunnel-status.json"
        const val STATUS_LOCK_FILE = ".tunnel-status.lock"
        const val PERMISSION_FILE = "vpn-permission.txt"
        // Connected snapshots are rewritten every 5s. Three missed writes means
        // the tunnel process is gone. Connecting has no heartbeat, so a dead
        // "connecting" file is allowed to sit longer than a slow start.
        const val CONNECTED_STATUS_MAX_AGE_MS = 15_000L
        const val OTHER_LIVE_STATUS_MAX_AGE_MS = 60_000L
        const val MAX_EXPORT_BYTES = 16 * 1024 * 1024
    }
    @Command
    fun prepare(invoke: Invoke) {
        val notifications = getPermissionState("notifications")
        if (notifications != PermissionState.GRANTED) {
            requestPermissionForAlias("notifications", invoke, "afterNotifications")
            return
        }
        askVpn(invoke)
    }

    @PermissionCallback
    fun afterNotifications(invoke: Invoke) {
        askVpn(invoke)
    }

    private fun askVpn(invoke: Invoke) {
        val intent = VpnService.prepare(activity)
        if (intent == null) {
            invoke.resolve(permission("granted"))
            return
        }
        startActivityForResult(invoke, intent, "vpnPermission")
    }

    @ActivityCallback
    fun vpnPermission(invoke: Invoke, result: ActivityResult) {
        val granted = result.resultCode == Activity.RESULT_OK
        val file = File(activity.filesDir, PERMISSION_FILE)
        if (granted) file.delete() else file.writeText("denied")
        invoke.resolve(permission(if (granted) "granted" else "denied"))
    }

    @Command
    fun start(invoke: Invoke) = launch(invoke, ACTION_START)

    @Command
    fun stop(invoke: Invoke) = launch(invoke, ACTION_STOP)

    @Command
    fun reload(invoke: Invoke) = launch(invoke, ACTION_RELOAD)

    @Command
    fun status(invoke: Invoke) {
        val stored = readStatus()
        // The status file is written by the :tunnel process. A killed process
        // leaves the last "connected" snapshot behind, so the UI must not
        // trust a live phase unless that process is still running.
        val phase = phaseWhileTunnelAlive(stored)
        val denied = runCatching {
            File(activity.filesDir, PERMISSION_FILE).readText().trim() == "denied"
        }.getOrDefault(false)
        val permission = when {
            VpnService.prepare(activity) == null -> "granted"
            denied -> "denied"
            else -> "unknown"
        }
        val payload = JSObject()
        payload.put("phase", phase)
        payload.put("permission", permission)
        payload.put("package_name", activity.packageName)
        // A forced "stopped" must not keep the dead tunnel's last error or RSS.
        if (phase != "stopped") {
            if (stored != null && stored.has("message") && !stored.isNull("message")) {
                payload.put("message", stored.getString("message"))
            }
            if (stored != null && stored.has("memory_bytes") && !stored.isNull("memory_bytes")) {
                payload.put("memory_bytes", stored.getLong("memory_bytes"))
            }
        }
        invoke.resolve(payload)
    }

    @Command
    fun shared_dir(invoke: Invoke) {
        ensureGeoipRuleSets(activity)
        val payload = JSObject()
        payload.put("path", activity.filesDir.absolutePath)
        invoke.resolve(payload)
    }

    @Command
    fun memory(invoke: Invoke) {
        val stored = readStatus()
        val payload = JSObject()
        if (stored != null && stored.has("memory_bytes") && !stored.isNull("memory_bytes")) {
            payload.put("bytes", stored.getLong("memory_bytes"))
        } else {
            payload.put("bytes", JSONObject.NULL)
        }
        invoke.resolve(payload)
    }

    @Command
    fun device_status(invoke: Invoke) {
        val payload = JSObject()
        payload.put("battery_unrestricted", batteryUnrestricted())
        payload.put("private_dns_strict", privateDnsStrict())
        payload.put("always_on_vpn", alwaysOnVpn())
        invoke.resolve(payload)
    }

    @Command
    fun request_battery_exemption(invoke: Invoke) {
        val direct = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
            data = Uri.parse("package:${activity.packageName}")
        }
        if (start(direct)) {
            invoke.resolve(JSObject())
            return
        }
        open(invoke, Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
    }

    @Command
    fun open_network_settings(invoke: Invoke) {
        open(invoke, Intent(Settings.ACTION_WIRELESS_SETTINGS))
    }

    @Command
    fun open_vpn_settings(invoke: Invoke) {
        open(invoke, Intent(Settings.ACTION_VPN_SETTINGS))
    }

    @Command
    fun open_https_url(invoke: Invoke) {
        val url = invoke.getArgs().getString("url").orEmpty()
        if (!isAllowedDownloadUrl(url)) {
            invoke.reject("refusing url")
            return
        }
        open(invoke, Intent(Intent.ACTION_VIEW, Uri.parse(url)))
    }

    @Command
    fun install_apk(invoke: Invoke) {
        val path = invoke.getArgs().getString("path").orEmpty()
        val file = File(path)
        if (!isUpdateApk(file)) {
            invoke.reject("refusing path")
            return
        }
        if (!activity.packageManager.canRequestPackageInstalls()) {
            start(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES).apply {
                    data = Uri.parse("package:${activity.packageName}")
                },
            )
            invoke.reject("install permission required")
            return
        }
        try {
            commitInstall(file)
            invoke.resolve(JSObject())
        } catch (err: Exception) {
            invoke.reject(err.message ?: err.javaClass.simpleName)
        }
    }

    /**
     * A live phase is real while `:tunnel` is running, or while the status
     * file is still being refreshed. `getRunningAppProcesses` sometimes lists
     * only the calling process, so a miss must not overwrite a fresh
     * `connected` snapshot. The tunnel rewrites that snapshot every 5s;
     * silence past [CONNECTED_STATUS_MAX_AGE_MS] means the process is gone.
     */
    private fun phaseWhileTunnelAlive(stored: JSONObject?): String {
        val phase = stored?.optString("phase", "stopped").orEmpty().ifEmpty { "stopped" }
        if (phase != "connecting" && phase != "connected" && phase != "disconnecting") {
            return phase
        }
        if (tunnelProcessRunning()) return phase
        if (!statusFileStale(phase)) return phase
        return if (persistStoppedIfStillStale(phase)) "stopped" else phase
    }

    private fun tunnelProcessRunning(): Boolean {
        return runCatching {
            val manager = activity.getSystemService(ActivityManager::class.java) ?: return@runCatching false
            val name = "${activity.packageName}:tunnel"
            manager.runningAppProcesses.orEmpty().any { it.processName == name }
        }.getOrDefault(false)
    }

    private fun statusFileStale(phase: String): Boolean {
        val file = File(activity.filesDir, STATUS_FILE)
        if (!file.exists()) return true
        val age = System.currentTimeMillis() - file.lastModified()
        if (age < 0) return false
        val limit = if (phase == "connected") {
            CONNECTED_STATUS_MAX_AGE_MS
        } else {
            OTHER_LIVE_STATUS_MAX_AGE_MS
        }
        return age > limit
    }

    /**
     * Write `stopped` only when the file is still stale after the cross-process
     * lock. A heartbeat that landed while we waited keeps the live phase.
     * Returns whether this call stored `stopped`.
     */
    private fun persistStoppedIfStillStale(phase: String): Boolean {
        val dir = activity.filesDir
        val text = stoppedStatusText()
        return synchronized(statusLock) {
            val wrote = runCatching {
                withStatusFileLock(dir) {
                    if (!statusFileStale(phase)) return@withStatusFileLock false
                    val dest = File(dir, STATUS_FILE)
                    val tmp = File(dir, ".tunnel-status.host.tmp")
                    FileOutputStream(tmp).use { out ->
                        out.write(text.toByteArray(Charsets.UTF_8))
                        out.fd.sync()
                    }
                    if (!tmp.renameTo(dest)) {
                        tmp.copyTo(dest, overwrite = true)
                        tmp.delete()
                    }
                    true
                }
            }.getOrDefault(false)
            if (wrote) lastStatus = JSONObject(text)
            wrote
        }
    }

    private fun stoppedStatusText(): String {
        val payload = JSONObject()
        payload.put("phase", "stopped")
        payload.put("package_name", activity.packageName)
        payload.put("message", JSONObject.NULL)
        payload.put("memory_bytes", JSONObject.NULL)
        return payload.toString()
    }

    /**
     * Same lock file as `TunnelFiles.withStatusFileLock`. The two modules do
     * not share code; the file name is the protocol.
     */
    private inline fun <T> withStatusFileLock(dir: File, block: () -> T): T {
        val lockFile = File(dir, STATUS_LOCK_FILE)
        return RandomAccessFile(lockFile, "rw").use { raf ->
            val lock = raf.channel.lock()
            try {
                block()
            } finally {
                lock.release()
            }
        }
    }

    /**
     * System save dialog. Android WebView ignores `<a download>`, so the
     * export is written to the URI the user picks. `"cancelled"` when the
     * dialog is dismissed. The bytes are not logged.
     */
    @Command
    fun save_text_document(invoke: Invoke) {
        if (pendingExport != null) {
            invoke.reject("export already in progress")
            return
        }
        val args = invoke.getArgs()
        val filename = exportFilename(args.optString("filename", ""))
        val content = if (args.has("content") && !args.isNull("content")) {
            args.getString("content")
        } else {
            ""
        }
        val bytes = content.toByteArray(Charsets.UTF_8)
        if (bytes.size > MAX_EXPORT_BYTES) {
            invoke.reject("export is too large")
            return
        }
        pendingExport = bytes
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/json"
            putExtra(Intent.EXTRA_TITLE, filename)
        }
        try {
            startActivityForResult(invoke, intent, "saveTextDocument")
        } catch (err: Exception) {
            pendingExport = null
            invoke.reject(err.message ?: err.javaClass.simpleName)
        }
    }

    @ActivityCallback
    fun saveTextDocument(invoke: Invoke, result: ActivityResult) {
        val bytes = pendingExport
        pendingExport = null
        if (result.resultCode != Activity.RESULT_OK || bytes == null) {
            resolveOutcome(invoke, "cancelled")
            return
        }
        val uri = result.data?.data
        if (uri == null) {
            resolveOutcome(invoke, "cancelled")
            return
        }
        try {
            val stream = activity.contentResolver.openOutputStream(uri)
                ?: error("no output stream")
            stream.use { it.write(bytes) }
            resolveOutcome(invoke, "saved")
        } catch (err: Exception) {
            invoke.reject(err.message ?: err.javaClass.simpleName)
        }
    }

    private fun resolveOutcome(invoke: Invoke, outcome: String) {
        val payload = JSObject()
        payload.put("outcome", outcome)
        invoke.resolve(payload)
    }

    private fun exportFilename(raw: String): String {
        val cleaned = buildString(raw.length) {
            for (ch in raw) {
                if (ch.isISOControl() || ch == '\u007f' || ch in "\\/:*?\"<>|") {
                    append('_')
                } else {
                    append(ch)
                }
            }
        }.trim().trim('.')
        if (cleaned.isEmpty()) return "sing-box.json"
        return if (cleaned.endsWith(".json", ignoreCase = true)) cleaned else "$cleaned.json"
    }

    /**
     * A torn read used to parse as "stopped" and skip a live reload.
     * The writer renames a complete file into place. If this read still
     * fails while the file exists, keep the last snapshot that parsed.
     */
    private fun readStatus(): JSONObject? {
        val file = File(activity.filesDir, STATUS_FILE)
        val parsed = runCatching { JSONObject(file.readText()) }.getOrNull()
        synchronized(statusLock) {
            if (parsed != null) {
                lastStatus = parsed
                return parsed
            }
            if (file.exists()) {
                return lastStatus
            }
            lastStatus = null
            return null
        }
    }

    private fun batteryUnrestricted(): Boolean {
        val manager = activity.getSystemService(PowerManager::class.java) ?: return false
        return manager.isIgnoringBatteryOptimizations(activity.packageName)
    }

    private fun privateDnsStrict(): Boolean {
        val mode = Settings.Global.getString(activity.contentResolver, "private_dns_mode")
        return mode == "hostname"
    }

    private fun alwaysOnVpn(): Boolean {
        val selected = Settings.Secure.getString(activity.contentResolver, "always_on_vpn_app")
        return selected == activity.packageName
    }

    private fun start(intent: Intent): Boolean {
        return try {
            activity.startActivity(intent)
            true
        } catch (_: Exception) {
            false
        }
    }

    private fun open(invoke: Invoke, intent: Intent) {
        if (start(intent)) {
            invoke.resolve(JSObject())
        } else {
            invoke.reject("settings unavailable")
        }
    }

    private fun launch(invoke: Invoke, action: String) {
        try {
            val intent = Intent(action).setClassName(activity.packageName, SERVICE_CLASS)
            activity.startForegroundService(intent)
            invoke.resolve(JSObject())
        } catch (err: Exception) {
            invoke.reject(err.message ?: err.javaClass.simpleName)
        }
    }

    private fun isUpdateApk(file: File): Boolean = isDownloadedUpdateApk(activity, file)

    private fun commitInstall(file: File) {
        val installer = activity.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setSize(file.length())
        params.setAppPackageName(activity.packageName)
        params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
        val sessionId = installer.createSession(params)
        val session = installer.openSession(sessionId)
        try {
            session.openWrite("base.apk", 0, file.length()).use { out ->
                FileInputStream(file).use { input -> input.copyTo(out) }
                session.fsync(out)
            }
            // The system reports STATUS_PENDING_USER_ACTION on this sender.
            // InstallResultReceiver starts EXTRA_INTENT; the main activity does not.
            val callback = PendingIntent.getBroadcast(
                activity,
                sessionId,
                Intent(activity, InstallResultReceiver::class.java).apply {
                    putExtra(InstallResultReceiver.EXTRA_APK_PATH, file.canonicalPath)
                },
                PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            session.commit(callback.intentSender)
        } catch (err: Exception) {
            session.abandon()
            throw err
        } finally {
            session.close()
        }
    }

    private fun isAllowedDownloadUrl(url: String): Boolean {
        val prefix = "https://github.com/yilong-musk/ice-box/releases/download/v"
        if (!url.startsWith(prefix)) return false
        val rest = url.removePrefix(prefix)
        val slash = rest.indexOf('/')
        if (slash <= 0 || slash != rest.lastIndexOf('/')) return false
        val version = rest.substring(0, slash)
        if (!Regex("""\d+\.\d+\.\d+""").matches(version)) return false
        return rest.substring(slash + 1) == "ice-box_${version}_android_arm64.apk"
    }

    private fun permission(value: String): JSObject {
        val payload = JSObject()
        payload.put("permission", value)
        return payload
    }
}
