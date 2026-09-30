package com.yilongmusk.icebox.spike

import android.content.Context
import android.util.Log
import io.nekohasekai.libbox.NetworkInterface as BoxInterface
import io.nekohasekai.libbox.NetworkInterfaceIterator
import io.nekohasekai.libbox.StringIterator
import java.io.File
import java.security.SecureRandom
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

const val TAG = "IceSpike"
const val ACTION_START = "com.yilongmusk.icebox.spike.START"
const val ACTION_STOP = "com.yilongmusk.icebox.spike.STOP"
const val ACTION_RELOAD = "com.yilongmusk.icebox.spike.RELOAD"
const val EXTRA_EXCLUDE_SELF = "exclude_self"

/** Both processes of the app share `filesDir`; it stands in for the design's shared root. */
fun sharedDir(context: Context): File = context.filesDir

/** Appends one line to the result log both processes write, and to logcat. */
fun report(context: Context, source: String, message: String) {
    val stamp = SimpleDateFormat("HH:mm:ss.SSS", Locale.US).format(Date())
    val line = "$stamp [$source] $message"
    Log.i(TAG, line)
    synchronized(TAG) {
        File(sharedDir(context), "spike-results.log").appendText(line + "\n")
    }
}

/** Per-install Clash API secret, readable by both processes. */
fun clashSecret(context: Context): String {
    val file = File(sharedDir(context), "clash.secret")
    if (!file.exists()) {
        val alphabet = "abcdefghijklmnopqrstuvwxyz0123456789"
        val random = SecureRandom()
        file.writeText((1..32).map { alphabet[random.nextInt(alphabet.length)] }.joinToString(""))
    }
    return file.readText().trim()
}

class StringArray(private val values: List<String>) : StringIterator {
    private var index = 0
    override fun hasNext() = index < values.size
    override fun len() = values.size
    override fun next() = values[index++]
}

class InterfaceArray(private val values: List<BoxInterface>) : NetworkInterfaceIterator {
    private var index = 0
    override fun hasNext() = index < values.size
    override fun next() = values[index++]
}

fun StringIterator.toList(): List<String> {
    val out = mutableListOf<String>()
    while (hasNext()) out.add(next())
    return out
}
