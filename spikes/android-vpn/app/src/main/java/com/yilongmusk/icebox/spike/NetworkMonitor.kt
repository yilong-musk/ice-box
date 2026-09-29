package com.yilongmusk.icebox.spike

import android.content.Context
import android.net.ConnectivityManager
import android.net.DnsResolver
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.CancellationSignal
import android.system.ErrnoException
import io.nekohasekai.libbox.ExchangeContext
import io.nekohasekai.libbox.InterfaceUpdateListener
import io.nekohasekai.libbox.LocalDNSTransport
import java.net.InetAddress
import java.net.NetworkInterface
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors

/**
 * Tracks the underlying (non-VPN) default network and reports it to libbox,
 * which binds outbound sockets to it and re-dials after network changes.
 */
class NetworkMonitor(private val context: Context) {
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private var listener: InterfaceUpdateListener? = null

    @Volatile
    var defaultNetwork: Network? = null
        private set

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = update(network)
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) = update(network)
        override fun onLost(network: Network) {
            if (network == defaultNetwork) update(null)
        }
    }

    fun start() {
        val request = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
            .build()
        connectivity.requestNetwork(request, callback)
    }

    fun stop() {
        runCatching { connectivity.unregisterNetworkCallback(callback) }
    }

    fun setListener(listener: InterfaceUpdateListener?) {
        this.listener = listener
        notifyListener(defaultNetwork)
    }

    private fun update(network: Network?) {
        if (network == defaultNetwork) return
        defaultNetwork = network
        report(context, "tunnel", "default network: ${network?.let { connectivity.getLinkProperties(it)?.interfaceName }}")
        notifyListener(network)
    }

    private fun notifyListener(network: Network?) {
        val listener = listener ?: return
        if (network == null) {
            listener.updateDefaultInterface("", -1, false, false)
            return
        }
        val name = connectivity.getLinkProperties(network)?.interfaceName ?: return
        val index = runCatching { NetworkInterface.getByName(name).index }.getOrDefault(-1)
        val caps = connectivity.getNetworkCapabilities(network)
        val metered = caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == false
        listener.updateDefaultInterface(name, index, metered, false)
    }
}

/** `local` DNS server: resolve on the underlying network through the system resolver. */
class LocalResolver(private val monitor: NetworkMonitor) : LocalDNSTransport {
    private val executor = Executors.newCachedThreadPool()

    override fun raw() = true

    override fun exchange(ctx: ExchangeContext, message: ByteArray) {
        val network = monitor.defaultNetwork ?: error("missing default network")
        val done = CountDownLatch(1)
        val signal = CancellationSignal()
        ctx.onCancel { signal.cancel(); done.countDown() }
        DnsResolver.getInstance().rawQuery(
            network, message, DnsResolver.FLAG_NO_RETRY, executor, signal,
            object : DnsResolver.Callback<ByteArray> {
                override fun onAnswer(answer: ByteArray, rcode: Int) {
                    if (rcode == 0) ctx.rawSuccess(answer) else ctx.errorCode(rcode)
                    done.countDown()
                }

                override fun onError(error: DnsResolver.DnsException) {
                    val cause = error.cause
                    if (cause is ErrnoException) ctx.errnoCode(cause.errno) else ctx.errorCode(2)
                    done.countDown()
                }
            },
        )
        done.await()
    }

    override fun lookup(ctx: ExchangeContext, network: String, domain: String) {
        val defaultNetwork = monitor.defaultNetwork ?: error("missing default network")
        val done = CountDownLatch(1)
        val signal = CancellationSignal()
        ctx.onCancel { signal.cancel(); done.countDown() }
        val callback = object : DnsResolver.Callback<List<InetAddress>> {
            override fun onAnswer(answer: List<InetAddress>, rcode: Int) {
                if (rcode == 0) ctx.success(answer.mapNotNull { it.hostAddress }.joinToString("\n")) else ctx.errorCode(rcode)
                done.countDown()
            }

            override fun onError(error: DnsResolver.DnsException) {
                val cause = error.cause
                if (cause is ErrnoException) ctx.errnoCode(cause.errno) else ctx.errorCode(2)
                done.countDown()
            }
        }
        val type = when {
            network.endsWith("4") -> DnsResolver.TYPE_A
            network.endsWith("6") -> DnsResolver.TYPE_AAAA
            else -> null
        }
        if (type != null) {
            DnsResolver.getInstance().query(defaultNetwork, domain, type, DnsResolver.FLAG_NO_RETRY, executor, signal, callback)
        } else {
            DnsResolver.getInstance().query(defaultNetwork, domain, DnsResolver.FLAG_NO_RETRY, executor, signal, callback)
        }
        done.await()
    }
}
