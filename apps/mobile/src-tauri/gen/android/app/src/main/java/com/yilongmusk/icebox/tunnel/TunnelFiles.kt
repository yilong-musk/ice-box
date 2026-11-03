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

/**
 * Size cap for the sing-box log. The phone core logs warnings and errors only
 * (`MOBILE_LOG_LEVEL` in crates/ice-config/src/build.rs), so 5 MiB keeps a
 * long history and always fits the 16 MiB export limit.
 */
const val CORE_LOG_MAX_BYTES = 5L * 1024 * 1024

/** sing-box writes `log.output` here (`apply_shared_root` in build.rs). */
fun coreLogFile(context: Context): File = File(sharedDir(context), "sing-box.log")

/**
 * Keep the sing-box log at or under [CORE_LOG_MAX_BYTES]. Mirrors
 * `ice_core::cap_log_file`: drop at least a quarter from the front, starting
 * at a line boundary, and compact the rest in place. The file keeps its inode,
 * so libbox's `O_APPEND` writer carries on at the new end.
 */
fun capCoreLog(context: Context) {
    val file = coreLogFile(context)
    if (file.length() <= CORE_LOG_MAX_BYTES) return
    RandomAccessFile(file, "rw").use { raf ->
        val size = raf.length()
        if (size <= CORE_LOG_MAX_BYTES) return
        val buf = ByteArray(64 * 1024)
        val drop = maxOf(CORE_LOG_MAX_BYTES / 4, size - CORE_LOG_MAX_BYTES).coerceAtMost(size - 1)
        val start = nextLineStart(raf, drop, size, buf)
        var from = start
        fun copyUntil(end: Long) {
            while (from < end) {
                val count = minOf(buf.size.toLong(), end - from).toInt()
                raf.seek(from)
                raf.readFully(buf, 0, count)
                raf.seek(from - start)
                raf.write(buf, 0, count)
                from += count
            }
        }
        copyUntil(size)
        // Keep lines appended while the copy ran.
        copyUntil(raf.length())
        raf.setLength(from - start)
    }
}

/** First line start at or after [start]; [start] itself when no newline follows. */
private fun nextLineStart(raf: RandomAccessFile, start: Long, size: Long, buf: ByteArray): Long {
    raf.seek(start - 1)
    if (raf.read() == '\n'.code) return start
    var pos = start
    while (pos < size) {
        val count = raf.read(buf, 0, minOf(buf.size.toLong(), size - pos).toInt())
        if (count <= 0) break
        for (i in 0 until count) {
            if (buf[i] == '\n'.code.toByte()) {
                val next = pos + i + 1
                return if (next < size) next else start
            }
        }
        pos += count
    }
    return start
}
