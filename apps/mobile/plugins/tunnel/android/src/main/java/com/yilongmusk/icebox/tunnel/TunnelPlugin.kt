// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.Manifest
import android.app.Activity
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
    // Keep these strings aligned with TunnelFiles.kt in the app module.
    // They stay inside this class so the two modules can share a package
    // without clashing top-level names.
    private companion object {
        const val ACTION_START = "com.yilongmusk.icebox.START"
        const val ACTION_STOP = "com.yilongmusk.icebox.STOP"
        const val ACTION_RELOAD = "com.yilongmusk.icebox.RELOAD"
        const val SERVICE_CLASS = "com.yilongmusk.icebox.tunnel.TunnelService"
        const val STATUS_FILE = "tunnel-status.json"
        const val PERMISSION_FILE = "vpn-permission.txt"
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
        val phase = stored?.optString("phase", "stopped").orEmpty().ifEmpty { "stopped" }
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
        if (stored != null && stored.has("message") && !stored.isNull("message")) {
            payload.put("message", stored.getString("message"))
        }
        if (stored != null && stored.has("memory_bytes") && !stored.isNull("memory_bytes")) {
            payload.put("memory_bytes", stored.getLong("memory_bytes"))
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
            file.delete()
            invoke.resolve(JSObject())
        } catch (err: Exception) {
            invoke.reject(err.message ?: err.javaClass.simpleName)
        }
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

    private fun isUpdateApk(file: File): Boolean {
        val canonical = runCatching { file.canonicalFile }.getOrNull() ?: return false
        val parent = canonical.parentFile ?: return false
        val data = activity.dataDir.canonicalPath + File.separator
        return parent.name == "updates" &&
            canonical.name == "ice-box-update.apk" &&
            canonical.path.startsWith(data) &&
            canonical.isFile &&
            canonical.length() > 0L
    }

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
            val callback = PendingIntent.getActivity(
                activity,
                sessionId,
                Intent(activity, activity.javaClass),
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
