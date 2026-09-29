// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.net.VpnService
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
        val file = File(activity.filesDir, STATUS_FILE)
        val stored = runCatching { JSONObject(file.readText()) }.getOrNull()
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
        val payload = JSObject()
        payload.put("path", activity.filesDir.absolutePath)
        invoke.resolve(payload)
    }

    @Command
    fun memory(invoke: Invoke) {
        val file = File(activity.filesDir, STATUS_FILE)
        val stored = runCatching { JSONObject(file.readText()) }.getOrNull()
        val payload = JSObject()
        if (stored != null && stored.has("memory_bytes") && !stored.isNull("memory_bytes")) {
            payload.put("bytes", stored.getLong("memory_bytes"))
        } else {
            payload.put("bytes", JSONObject.NULL)
        }
        invoke.resolve(payload)
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

    private fun permission(value: String): JSObject {
        val payload = JSObject()
        payload.put("permission", value)
        return payload
    }
}
