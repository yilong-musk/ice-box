package com.yilongmusk.icebox.spike

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.Debug
import android.os.ParcelFileDescriptor
import android.os.Process
import android.system.OsConstants
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
import java.io.File
import java.net.InetAddress
import java.net.InetSocketAddress
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import io.nekohasekai.libbox.NetworkInterface as BoxInterface

/**
 * Tunnel process (`:tunnel`): runs libbox and owns the TUN descriptor. It
 * loads no Rust; it only reads the config the host process wrote.
 */
class TunnelService : VpnService(), PlatformInterface, CommandServerHandler {
    private val worker = Executors.newSingleThreadScheduledExecutor()
    private val monitor by lazy { NetworkMonitor(this) }
    private var server: CommandServer? = null
    private var tun: ParcelFileDescriptor? = null
    private var excludeSelf = true

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        report(this, "tunnel", "onStartCommand action=$action pid=${Process.myPid()}")
        when (action) {
            ACTION_STOP -> worker.execute { stopBox(); stopSelf() }
            ACTION_RELOAD -> worker.execute { startOrReload("reload") }
            else -> {
                // ACTION_START from the host, or the system (always-on VPN) with its own intent.
                excludeSelf = intent?.getBooleanExtra(EXTRA_EXCLUDE_SELF, true) ?: true
                startForeground(1, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
                worker.execute { startOrReload(if (action == ACTION_START) "start" else "system start ($action)") }
            }
        }
        return START_NOT_STICKY
    }

    private fun startOrReload(reason: String) {
        try {
            val configFile = File(sharedDir(this), "config.json")
            if (!configFile.exists()) error("no config written by the host")
            val server = server ?: createServer()
            val options = OverrideOptions()
            if (excludeSelf) options.excludePackage = StringArray(listOf(packageName))
            val started = System.nanoTime()
            server.startOrReloadService(configFile.readText(), options)
            val ms = (System.nanoTime() - started) / 1_000_000
            report(this, "tunnel", "$reason ok in $ms ms (exclude_self=$excludeSelf, libbox ${Libbox.version()})")
            scheduleMemoryReports()
        } catch (e: Throwable) {
            report(this, "tunnel", "ERROR $reason: ${e.javaClass.simpleName}: ${e.message}")
        }
    }

    private fun createServer(): CommandServer {
        val shared = sharedDir(this)
        Libbox.setup(SetupOptions().apply {
            basePath = shared.path
            workingPath = File(shared, "work").path
            tempPath = cacheDir.path
            logMaxLines = 3000
        })
        Libbox.redirectStderr(File(shared, "stderr.log").path)
        monitor.start()
        return Libbox.newCommandServer(this, this).also { server = it }
    }

    private var memoryTask: java.util.concurrent.ScheduledFuture<*>? = null

    private fun scheduleMemoryReports() {
        if (memoryTask != null) return
        memoryTask = worker.scheduleWithFixedDelay({
            val info = Debug.MemoryInfo().also { Debug.getMemoryInfo(it) }
            val native = Debug.getNativeHeapAllocatedSize() / 1024
            File(sharedDir(this), "tunnel-status.txt").writeText(
                "pid=${Process.myPid()} pss_kb=${info.totalPss} native_heap_kb=$native tun=${tun?.fd}\n",
            )
        }, 0, 5, TimeUnit.SECONDS)
    }

    private fun stopBox() {
        memoryTask?.cancel(false)
        memoryTask = null
        runCatching { server?.closeService() }
        runCatching { tun?.close() }
        tun = null
        report(this, "tunnel", "stopped")
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    override fun onRevoke() {
        report(this, "tunnel", "revoked by the system")
        worker.execute { stopBox(); stopSelf() }
    }

    override fun onDestroy() {
        runCatching { server?.close() }
        monitor.stop()
        super.onDestroy()
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("vpn", "VPN", NotificationManager.IMPORTANCE_LOW))
        return Notification.Builder(this, "vpn")
            .setContentTitle("ice-box spike")
            .setContentText("Tunnel running")
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setOngoing(true)
            .build()
    }

    // --- PlatformInterface -------------------------------------------------

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
            options.inet4RouteExcludeAddress.toPrefixes().forEach { builder.excludeRoute(it.toIpPrefix()) }
            options.inet6RouteExcludeAddress.toPrefixes().forEach { builder.excludeRoute(it.toIpPrefix()) }
            options.includePackage.toList().forEach { runCatching { builder.addAllowedApplication(it) } }
            options.excludePackage.toList().forEach { name ->
                runCatching { builder.addDisallowedApplication(name) }
                    .onFailure { report(this, "tunnel", "exclude $name failed: ${it.message}") }
            }
        }
        val pfd = builder.establish() ?: error("VPN not prepared or revoked")
        tun?.close()
        tun = pfd
        report(
            this, "tunnel",
            "openTun fd=${pfd.fd} v4=${v4.map { it.string() }} v6=${v6.map { it.string() }} " +
                "mtu=${options.mtu} excluded=${options.excludePackage.toList()}",
        )
        return pfd.fd
    }

    override fun usePlatformAutoDetectInterfaceControl() = true

    override fun autoDetectInterfaceControl(fd: Int) {
        if (!protect(fd)) error("protect($fd) failed")
    }

    override fun useProcFS() = false

    override fun findConnectionOwner(
        ipProtocol: Int, sourceAddress: String, sourcePort: Int, destinationAddress: String, destinationPort: Int,
    ): ConnectionOwner {
        val connectivity = getSystemService(ConnectivityManager::class.java)
        val uid = connectivity.getConnectionOwnerUid(
            ipProtocol, InetSocketAddress(sourceAddress, sourcePort), InetSocketAddress(destinationAddress, destinationPort),
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
                addresses = StringArray(nif.interfaceAddresses.map { "${it.address.hostAddress?.substringBefore('%')}/${it.networkPrefixLength}" })
                dnsServer = StringArray(link.dnsServers.mapNotNull { it.hostAddress })
                type = when {
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> Libbox.InterfaceTypeWIFI
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> Libbox.InterfaceTypeCellular
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> Libbox.InterfaceTypeEthernet
                    else -> Libbox.InterfaceTypeOther
                }
                var bits = if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) OsConstants.IFF_UP or OsConstants.IFF_RUNNING else 0
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

    // --- CommandServerHandler ----------------------------------------------

    override fun serviceStop() {
        worker.execute { stopBox(); stopSelf() }
    }

    override fun serviceReload() {
        worker.execute { startOrReload("reload (core)") }
    }

    override fun getSystemProxyStatus() = SystemProxyStatus()

    override fun setSystemProxyEnabled(enabled: Boolean) {}

    override fun writeDebugMessage(message: String) = report(this, "core", message)
}

private fun io.nekohasekai.libbox.RoutePrefixIterator.toPrefixes(): List<RoutePrefix> {
    val out = mutableListOf<RoutePrefix>()
    while (hasNext()) out.add(next())
    return out
}

private fun RoutePrefix.toIpPrefix() = IpPrefix(InetAddress.getByName(address()), prefix())
