// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.os.Process
import android.system.OsConstants
import android.util.Log
import io.nekohasekai.libbox.CommandServer
import io.nekohasekai.libbox.CommandServerHandler
import io.nekohasekai.libbox.ConnectionOwner
import io.nekohasekai.libbox.InterfaceUpdateListener
import io.nekohasekai.libbox.Libbox
import io.nekohasekai.libbox.LocalDNSTransport
import io.nekohasekai.libbox.NetworkInterfaceIterator
import io.nekohasekai.libbox.OverrideOptions
import io.nekohasekai.libbox.PlatformInterface
import io.nekohasekai.libbox.RoutePrefix
import io.nekohasekai.libbox.SetupOptions
import io.nekohasekai.libbox.StringIterator
import io.nekohasekai.libbox.SystemProxyStatus
import io.nekohasekai.libbox.TunOptions
import io.nekohasekai.libbox.WIFIState
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import io.nekohasekai.libbox.NetworkInterface as BoxInterface

/**
 * Tunnel process (`:tunnel`): runs libbox and owns the TUN descriptor.
 * It loads no Rust. It reads `config.json` from the shared files directory.
 */
class TunnelService : VpnService(), PlatformInterface, CommandServerHandler {
    private val worker = Executors.newSingleThreadScheduledExecutor()
    private val monitor by lazy { NetworkMonitor(this) }
    private var monitorStarted = false
    private var server: CommandServer? = null
    private var tun: ParcelFileDescriptor? = null
    private var phase = "stopped"
    private var foregroundFailed = false

    /** Set on the main thread in onDestroy. The worker must not publish a live phase after this. */
    @Volatile
    private var destroyed = false
    private var memoryTask: java.util.concurrent.ScheduledFuture<*>? = null
    private var trafficReporter: Thread? = null
    @Volatile
    private var trafficReporting = false
    @Volatile
    private var trafficSocket: Socket? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        if (!promoteToForeground()) {
            return START_NOT_STICKY
        }
        when (action) {
            ACTION_STOP -> onWorker { stopBox(); stopSelf() }
            ACTION_RELOAD -> onWorker { startOrReload("reload") }
            else -> onWorker {
                startOrReload(if (action == ACTION_START) "start" else "system start ($action)")
            }
        }
        return START_NOT_STICKY
    }

    /** libbox is only touched on [worker]. After [destroyed], further commands are dropped. */
    private fun onWorker(block: () -> Unit) {
        if (destroyed) return
        runCatching { worker.execute(block) }
    }

    private fun startOrReload(reason: String) {
        if (destroyed) return
        publish("connecting", null)
        try {
            val configFile = File(sharedDir(this), "config.json")
            if (!configFile.exists()) error("no config written by the host")
            if (destroyed) return
            val server = server ?: createServer()
            if (destroyed) return
            val options = OverrideOptions()
            options.excludePackage = StringArray(listOf(packageName))
            server.startOrReloadService(configFile.readText(), options)
            if (destroyed) return
            publish("connected", null)
            Log.i(NetworkMonitor.TAG, "$reason ok (libbox ${Libbox.version()})")
            scheduleMemoryReports()
            startTrafficReports()
        } catch (err: Throwable) {
            if (destroyed) return
            val message = "${err.javaClass.simpleName}: ${err.message}"
            publish("error", message)
            Log.e(NetworkMonitor.TAG, "ERROR $reason: $message")
        }
    }

    private fun createServer(): CommandServer {
        val shared = sharedDir(this)
        File(shared, "work").mkdirs()
        Libbox.setup(SetupOptions().apply {
            basePath = shared.path
            workingPath = File(shared, "work").path
            tempPath = cacheDir.path
            logMaxLines = 3000
        })
        Libbox.redirectStderr(File(shared, "stderr.log").path)
        monitor.start()
        monitorStarted = true
        return Libbox.newCommandServer(this, this).also { server = it }
    }

    private fun scheduleMemoryReports() {
        if (memoryTask != null || destroyed) return
        // The host treats a connected snapshot older than 15s as a dead
        // process. Keep this period well under that.
        memoryTask = worker.scheduleWithFixedDelay({
            if (!destroyed && phase == "connected") publish("connected", null)
        }, 5, 5, TimeUnit.SECONDS)
    }

    private fun publish(next: String, message: String?) {
        if (destroyed && (next == "connecting" || next == "connected")) return
        phase = next
        writeStatus(this, next, message)
    }

    private fun stopBox() {
        if (!destroyed) publish("disconnecting", null)
        stopTrafficReports()
        memoryTask?.cancel(false)
        memoryTask = null
        // Leave the CommandServer in place so the next start can reload it.
        // onDestroy is what closes the object, on this same worker.
        runCatching { server?.closeService() }
        runCatching { tun?.close() }
        tun = null
        if (!foregroundFailed) publish("stopped", null)
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    /** Idempotent. Runs on [worker] only, after [destroyed] is set. */
    private fun destroyServer() {
        stopTrafficReports()
        memoryTask?.cancel(false)
        memoryTask = null
        val running = server
        server = null
        runCatching { running?.closeService() }
        runCatching { running?.close() }
        runCatching { tun?.close() }
        tun = null
        if (monitorStarted) {
            monitor.stop()
            monitorStarted = false
        }
    }

    override fun onRevoke() {
        onWorker { stopBox(); stopSelf() }
    }

    override fun onDestroy() {
        // Queue the close behind any start already running on the worker.
        // Publishing "connected" after this flag is set is ignored.
        destroyed = true
        runCatching {
            worker.execute {
                destroyServer()
                if (!foregroundFailed) publish("stopped", null)
            }
        }
        worker.shutdown()
        super.onDestroy()
    }

    /**
     * `systemExempted` throws SecurityException unless the app is already on
     * the battery-optimization allowlist, which kills `:tunnel` on the first
     * connection. `specialUse` is allowed before that exemption. Settings
     * still offers the exemption so the system is less likely to stop the
     * tunnel in the background.
     */
    private fun promoteToForeground(): Boolean {
        return try {
            startForeground(
                NOTIFICATION_ID,
                notification(),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
            true
        } catch (err: Exception) {
            Log.e(NetworkMonitor.TAG, "startForeground failed: ${err.message}")
            foregroundFailed = true
            publish("error", err.message ?: err.javaClass.simpleName)
            stopSelf()
            false
        }
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "VPN", NotificationManager.IMPORTANCE_LOW),
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("ice-box")
            .setContentText("VPN")
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()
    }

    /**
     * The Android status bar only reserves notification icons for third-party
     * apps. Keep the VPN's ongoing notification in the shade and refresh its
     * text once per Clash `/traffic` sample.
     */
    private fun startTrafficReports() {
        if (trafficReporter?.isAlive == true) return
        trafficReporting = true
        trafficReporter = Thread({ trafficReportLoop() }, "ice-box-traffic").apply {
            isDaemon = true
            start()
        }
    }

    private fun stopTrafficReports() {
        trafficReporting = false
        runCatching { trafficSocket?.close() }
        trafficSocket = null
        trafficReporter?.interrupt()
        trafficReporter = null
    }

    private fun trafficReportLoop() {
        while (trafficReporting && !destroyed) {
            try {
                streamTraffic()
            } catch (_: InterruptedException) {
                return
            } catch (err: Exception) {
                if (!trafficReporting || destroyed) return
                Log.d(NetworkMonitor.TAG, "traffic notification retry: ${err.message}")
            }
            try {
                Thread.sleep(1_000L)
            } catch (_: InterruptedException) {
                return
            }
        }
    }

    /**
     * Stream the same Clash `/traffic` endpoint used by the desktop chart. A
     * raw loopback socket avoids Android's cleartext HTTP policy while keeping
     * the API bearer token out of a URL or log line.
     */
    private fun streamTraffic() {
        val config = JSONObject(File(sharedDir(this), "config.json").readText())
        val clashApi = config
            .getJSONObject("experimental")
            .getJSONObject("clash_api")
        val controller = clashApi.getString("external_controller")
        val secret = clashApi.getString("secret")
        val separator = controller.lastIndexOf(':')
        require(separator > 0) { "invalid Clash controller" }
        val host = controller.substring(0, separator).removeSurrounding("[", "]")
        val port = controller.substring(separator + 1).toInt()

        val socket = Socket()
        trafficSocket = socket
        try {
            socket.connect(InetSocketAddress(InetAddress.getByName(host), port), 2_000)
            socket.soTimeout = 5_000
            socket.getOutputStream().bufferedWriter(Charsets.UTF_8).use { writer ->
                writer.write(
                    "GET /traffic HTTP/1.1\r\n" +
                        "Host: $controller\r\n" +
                        "Authorization: Bearer $secret\r\n" +
                        "Accept: text/event-stream\r\n" +
                        "Connection: close\r\n\r\n",
                )
                writer.flush()

                val reader = BufferedReader(InputStreamReader(socket.getInputStream(), Charsets.UTF_8))
                val status = reader.readLine().orEmpty()
                require(status.contains(" 200 ")) { "Clash traffic HTTP $status" }
                while (reader.readLine()?.isNotEmpty() == true) {
                    // Skip response headers; the body is chunked JSON lines.
                }
                while (trafficReporting && !destroyed) {
                    val line = reader.readLine() ?: error("traffic stream ended")
                    if (!line.trimStart().startsWith("{")) continue
                    val sample = JSONObject(line)
                    postTrafficNotification(
                        up = sample.optLong("up", 0L),
                        down = sample.optLong("down", 0L),
                    )
                }
            }
        } finally {
            if (trafficSocket === socket) trafficSocket = null
            runCatching { socket.close() }
        }
    }

    private fun postTrafficNotification(up: Long, down: Long) {
        if (!trafficReporting || destroyed || phase != "connected") return
        runCatching {
            getSystemService(NotificationManager::class.java).notify(
                NOTIFICATION_ID,
                Notification.Builder(this, CHANNEL_ID)
                    .setContentTitle("ice-box")
                    .setContentText("↓ ${formatRate(down)}   ↑ ${formatRate(up)}")
                    .setSubText("VPN")
                    .setSmallIcon(android.R.drawable.ic_lock_lock)
                    .setOngoing(true)
                    .setOnlyAlertOnce(true)
                    .build(),
            )
        }.onFailure {
            Log.d(NetworkMonitor.TAG, "traffic notification update failed: ${it.message}")
        }
    }

    override fun openTun(options: TunOptions): Int {
        if (prepare(this) != null) error("missing VPN permission")
        val builder = Builder().setSession("ice-box").setMtu(options.mtu).setMetered(false)
        val v4 = options.inet4Address.toPrefixes()
        val v6 = options.inet6Address.toPrefixes()
        (v4 + v6).forEach { builder.addAddress(it.address(), it.prefix()) }
        if (options.autoRoute) {
            options.dnsServerAddress?.value?.takeIf { it.isNotEmpty() }?.let { builder.addDnsServer(it) }
            val v4Routes = options.inet4RouteAddress.toPrefixes()
            if (v4Routes.isNotEmpty()) v4Routes.forEach { builder.addRoute(it.toIpPrefix()) }
            else if (v4.isNotEmpty()) builder.addRoute("0.0.0.0", 0)
            val v6Routes = options.inet6RouteAddress.toPrefixes()
            if (v6Routes.isNotEmpty()) v6Routes.forEach { builder.addRoute(it.toIpPrefix()) }
            else if (v6.isNotEmpty()) builder.addRoute("::", 0)
            excludeVpnRoutes(builder, options.inet4RouteExcludeAddress.toPrefixes())
            excludeVpnRoutes(builder, options.inet6RouteExcludeAddress.toPrefixes())
            options.includePackage.toList().forEach { runCatching { builder.addAllowedApplication(it) } }
            options.excludePackage.toList().forEach { name ->
                runCatching { builder.addDisallowedApplication(name) }
                    .onFailure { Log.w(NetworkMonitor.TAG, "exclude $name failed: ${it.message}") }
            }
        }
        val pfd = builder.establish() ?: error("VPN not prepared or revoked")
        tun?.close()
        tun = pfd
        return pfd.fd
    }

    override fun usePlatformAutoDetectInterfaceControl() = true

    override fun autoDetectInterfaceControl(fd: Int) {
        if (!protect(fd)) error("protect($fd) failed")
    }

    override fun useProcFS() = false

    override fun findConnectionOwner(
        ipProtocol: Int,
        sourceAddress: String,
        sourcePort: Int,
        destinationAddress: String,
        destinationPort: Int,
    ): ConnectionOwner {
        val connectivity = getSystemService(ConnectivityManager::class.java)
        val uid = connectivity.getConnectionOwnerUid(
            ipProtocol,
            InetSocketAddress(sourceAddress, sourcePort),
            InetSocketAddress(destinationAddress, destinationPort),
        )
        if (uid == Process.INVALID_UID) error("connection owner not found")
        val packages = packageManager.getPackagesForUid(uid)?.toList().orEmpty()
        return ConnectionOwner().apply {
            userId = uid
            userName = packages.firstOrNull() ?: ""
            setAndroidPackageNames(StringArray(packages))
        }
    }

    override fun startDefaultInterfaceMonitor(listener: InterfaceUpdateListener) = monitor.setListener(listener)

    override fun closeDefaultInterfaceMonitor(listener: InterfaceUpdateListener) = monitor.setListener(null)

    override fun getInterfaces(): NetworkInterfaceIterator {
        val connectivity = getSystemService(ConnectivityManager::class.java)
        val system = java.net.NetworkInterface.getNetworkInterfaces().toList()
        val result = mutableListOf<BoxInterface>()
        @Suppress("DEPRECATION")
        for (network in connectivity.allNetworks) {
            val link = connectivity.getLinkProperties(network) ?: continue
            val caps = connectivity.getNetworkCapabilities(network) ?: continue
            if (caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) continue
            val nif = system.find { it.name == link.interfaceName } ?: continue
            result += BoxInterface().apply {
                name = link.interfaceName
                index = nif.index
                mtu = runCatching { nif.mtu }.getOrDefault(1500)
                addresses = StringArray(
                    nif.interfaceAddresses.map {
                        "${it.address.hostAddress?.substringBefore('%')}/${it.networkPrefixLength}"
                    },
                )
                dnsServer = StringArray(link.dnsServers.mapNotNull { it.hostAddress })
                type = when {
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> Libbox.InterfaceTypeWIFI
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> Libbox.InterfaceTypeCellular
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> Libbox.InterfaceTypeEthernet
                    else -> Libbox.InterfaceTypeOther
                }
                var bits = if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
                    OsConstants.IFF_UP or OsConstants.IFF_RUNNING
                } else {
                    0
                }
                if (nif.isLoopback) bits = bits or OsConstants.IFF_LOOPBACK
                if (nif.isPointToPoint) bits = bits or OsConstants.IFF_POINTOPOINT
                if (nif.supportsMulticast()) bits = bits or OsConstants.IFF_MULTICAST
                flags = bits
                metered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
            }
        }
        return InterfaceArray(result)
    }

    override fun underNetworkExtension() = false

    override fun includeAllNetworks() = false

    override fun readWIFIState(): WIFIState? = null

    override fun systemCertificates(): StringIterator = StringArray(emptyList())

    override fun clearDNSCache() {}

    override fun sendNotification(notification: io.nekohasekai.libbox.Notification) {}

    override fun localDNSTransport(): LocalDNSTransport = LocalResolver(monitor)

    override fun serviceStop() {
        onWorker { stopBox(); stopSelf() }
    }

    override fun serviceReload() {
        onWorker { startOrReload("reload (core)") }
    }

    override fun getSystemProxyStatus() = SystemProxyStatus()

    override fun setSystemProxyEnabled(enabled: Boolean) {}

    override fun writeDebugMessage(message: String) {
        Log.i(NetworkMonitor.TAG, message)
    }

    companion object {
        private const val NOTIFICATION_ID = 1
        private const val CHANNEL_ID = "vpn"

        private fun formatRate(bytesPerSecond: Long): String {
            val rate = bytesPerSecond.coerceAtLeast(0L)
            return when {
                rate < 1_024L -> "$rate B/s"
                rate < 1_048_576L ->
                    String.format(Locale.US, "%.1f KB/s", rate / 1_024.0)
                else ->
                    String.format(Locale.US, "%.2f MB/s", rate / 1_048_576.0)
            }
        }
    }
}

/**
 * VpnService.Builder.excludeRoute rejects a loopback destination
 * (127.0.0.0/8 and ::1) with IllegalArgumentException("Bad address").
 * Android does not deliver loopback into the tunnel, so those prefixes
 * are dropped here instead of failing openTun.
 */
private fun excludeVpnRoutes(builder: VpnService.Builder, prefixes: List<RoutePrefix>) {
    for (prefix in prefixes) {
        val address = InetAddress.getByName(prefix.address())
        if (address.isLoopbackAddress) {
            Log.i(
                NetworkMonitor.TAG,
                "skip exclude ${prefix.address()}/${prefix.prefix()}: loopback",
            )
            continue
        }
        builder.excludeRoute(prefix.toIpPrefix())
    }
}

private fun io.nekohasekai.libbox.RoutePrefixIterator.toPrefixes(): List<RoutePrefix> {
    val out = mutableListOf<RoutePrefix>()
    while (hasNext()) out.add(next())
    return out
}

private fun RoutePrefix.toIpPrefix() = IpPrefix(InetAddress.getByName(address()), prefix())
