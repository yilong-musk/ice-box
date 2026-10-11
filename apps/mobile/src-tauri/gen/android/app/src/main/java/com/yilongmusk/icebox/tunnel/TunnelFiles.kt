// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.content.Context
import android.os.Debug
import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile
import org.json.JSONObject

// Same action strings as TunnelPlugin in the tunnel plugin module.
const val ACTION_START = "com.yilongmusk.icebox.START"
const val ACTION_STOP = "com.yilongmusk.icebox.STOP"
const val ACTION_RELOAD = "com.yilongmusk.icebox.RELOAD"

fun sharedDir(context: Context): File = context.filesDir

private val statusFileLock = Any()

/** The host process reads this file for tunnel phase and memory. */
fun writeStatus(context: Context, phase: String, message: String?) {
    val payload = JSONObject()
    payload.put("phase", phase)
    payload.put("package_name", context.packageName)
    if (message == null) payload.put("message", JSONObject.NULL) else payload.put("message", message)
    val info = Debug.MemoryInfo()
    Debug.getMemoryInfo(info)
    payload.put("memory_bytes", info.totalPss.toLong() * 1024L)
    val text = payload.toString()
    // The service worker and onDestroy can both publish. Take the in-process
    // monitor first: a Java file lock is per VM and throws if a second thread
    // in this process locks the same file. The file lock then excludes the
    // host process, which uses the same lock file.
    val dir = sharedDir(context)
    synchronized(statusFileLock) {
        withStatusFileLock(dir) {
            val dest = File(dir, "tunnel-status.json")
            val tmp = File(dir, ".tunnel-status.json.tmp")
            FileOutputStream(tmp).use { out ->
                out.write(text.toByteArray(Charsets.UTF_8))
                out.fd.sync()
            }
            if (!tmp.renameTo(dest)) {
                dest.writeText(text)
                tmp.delete()
            }
        }
    }
}

/**
 * Exclusive lock shared with the host process. The lock file name matches
 * `TunnelPlugin`.
 */
private inline fun <T> withStatusFileLock(dir: File, block: () -> T): T {
    val lockFile = File(dir, ".tunnel-status.lock")
    return RandomAccessFile(lockFile, "rw").use { raf ->
        val lock = raf.channel.lock()
        try {
            block()
        } finally {
            lock.release()
        }
    }
}
