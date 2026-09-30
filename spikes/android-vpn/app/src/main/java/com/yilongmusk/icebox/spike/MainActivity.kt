package com.yilongmusk.icebox.spike

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import kotlin.concurrent.thread

/**
 * Host process. Every action is reachable from the buttons and from adb:
 * `am start -n <pkg>/.MainActivity --es cmd <name> [--es source|url|tag <v>] [--ez exclude_self <b>]`.
 */
class MainActivity : Activity() {
    private lateinit var output: TextView
    private lateinit var sourceInput: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        copyGeoip()
        buildUi()
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 2)
        }
        handle(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        val cmd = intent?.getStringExtra("cmd") ?: return
        run(cmd, intent)
    }

    private fun run(cmd: String, intent: Intent?) {
        val extra = { key: String, default: String -> intent?.getStringExtra(key) ?: default }
        when (cmd) {
            "prepare" -> {
                val consent = VpnService.prepare(this)
                if (consent == null) log("prepare: already granted") else startActivityForResult(consent, 1)
            }
            "start", "stop", "reload" -> {
                val action = mapOf("start" to ACTION_START, "stop" to ACTION_STOP, "reload" to ACTION_RELOAD)[cmd]
                val service = Intent(this, TunnelService::class.java).setAction(action)
                    .putExtra(EXTRA_EXCLUDE_SELF, intent?.getBooleanExtra(EXTRA_EXCLUDE_SELF, true) ?: true)
                if (cmd == "start") startForegroundService(service) else startService(service)
                log("$cmd: sent")
            }
            "clear" -> File(sharedDir(this), "spike-results.log").delete()
            else -> background(cmd) {
                when (cmd) {
                    "init" -> Native.init(applicationContext)
                    "build" -> {
                        val source = extra("source", sourceInput.text.toString().ifBlank { "direct" })
                        // `file:` reads a body pushed onto the device, so a subscription
                        // does not have to survive an adb intent extra.
                        val resolved = if (source.startsWith("file:")) {
                            "raw:" + File(source.removePrefix("file:")).readText()
                        } else {
                            source
                        }
                        Native.buildConfig(resolved, sharedDir(this).path, clashSecret(this))
                    }
                    "https" -> Native.httpsCheck(extra("url", "https://www.baidu.com"))
                    "clash" -> Native.clashCheck(clashSecret(this))
                    "delay" -> Native.clashDelay(clashSecret(this), extra("tag", "proxy"))
                    "probe" -> probe(extra("url", "https://api.ipify.org"))
                    else -> "unknown command $cmd"
                }
            }
        }
    }

    @Deprecated("Activity result API is not used in the spike")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == 1) log("prepare: result ${if (resultCode == RESULT_OK) "granted" else "denied"}")
    }

    /** Plain JVM HTTP from the host process: tunneled only when the app is not excluded. */
    private fun probe(url: String): String {
        val started = System.nanoTime()
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = 10_000
        connection.readTimeout = 10_000
        return try {
            val code = connection.responseCode
            val body = (if (code < 400) connection.inputStream else connection.errorStream)
                ?.bufferedReader()?.use { it.readText() } ?: ""
            val ms = (System.nanoTime() - started) / 1_000_000
            "probe $url -> $code in $ms ms: ${body.take(160).replace('\n', ' ')}"
        } finally {
            connection.disconnect()
        }
    }

    private fun background(cmd: String, block: () -> String) {
        thread(name = "spike-$cmd") {
            val message = try {
                block()
            } catch (e: Throwable) {
                "ERROR $cmd: ${e.javaClass.simpleName}: ${e.message}"
            }
            log(message)
        }
    }

    private fun log(message: String) {
        report(this, "host", message)
        runOnUiThread { output.append(message + "\n\n") }
    }

    private fun copyGeoip() {
        val target = File(sharedDir(this), "geoip").apply { mkdirs() }
        for (name in assets.list("rule-set").orEmpty()) {
            val file = File(target, name)
            if (!file.exists()) {
                assets.open("rule-set/$name").use { input -> file.outputStream().use { input.copyTo(it) } }
            }
        }
    }

    private fun buildUi() {
        val column = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(32, 96, 32, 32) }
        sourceInput = EditText(this).apply { hint = "direct | url:https://... | raw:..." }
        column.addView(sourceInput)
        for (cmd in listOf("init", "build", "https", "prepare", "start", "stop", "reload", "clash", "delay", "probe")) {
            column.addView(Button(this).apply { text = cmd; setOnClickListener { run(cmd, null) } })
        }
        output = TextView(this).apply { setTextIsSelectable(true) }
        column.addView(output)
        setContentView(ScrollView(this).apply { addView(column) })
    }
}
