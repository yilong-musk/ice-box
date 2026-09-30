// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.util.Log
import java.io.File

/**
 * PackageInstaller does not show the confirmation itself. It delivers
 * [PackageInstaller.STATUS_PENDING_USER_ACTION] here, and the app has to
 * start [PackageInstaller.EXTRA_INTENT].
 */
class InstallResultReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, Int.MIN_VALUE)
        if (status != PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Log.i(TAG, "package install status $status")
            return
        }
        val confirm = intent.getParcelableExtra(PackageInstaller.EXTRA_INTENT, Intent::class.java)
        if (confirm == null) {
            Log.e(TAG, "package install confirmation intent was missing")
            return
        }
        confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(confirm)
        } catch (err: Exception) {
            Log.e(TAG, "package install confirmation failed: ${err.message}")
            return
        }
        // The session already holds its own copy of the APK. Remove the
        // download only after the system UI is on screen, so a missed
        // confirmation can still be retried from the same file. The callback
        // extra is preferred; the fixed name covers a fill-in that drops it.
        val candidates = listOfNotNull(
            intent.getStringExtra(EXTRA_APK_PATH)?.let { File(it) },
            File(context.filesDir, "updates/ice-box-update.apk"),
        )
        for (file in candidates) {
            if (!isDownloadedUpdateApk(context, file)) continue
            if (!file.delete() && file.exists()) {
                Log.w(TAG, "update apk was not removed")
            }
            break
        }
    }

    companion object {
        const val EXTRA_APK_PATH = "com.yilongmusk.icebox.UPDATE_APK_PATH"
        private const val TAG = "IceBox"
    }
}

/** Downloaded self-update only. Rejects every other path, including ones a
 * mutable install callback might have overwritten. */
internal fun isDownloadedUpdateApk(context: Context, file: File): Boolean {
    val canonical = runCatching { file.canonicalFile }.getOrNull() ?: return false
    val parent = canonical.parentFile ?: return false
    val data = runCatching { context.dataDir.canonicalPath }.getOrNull() ?: return false
    return parent.name == "updates" &&
        canonical.name == "ice-box-update.apk" &&
        canonical.path.startsWith(data + File.separator) &&
        canonical.isFile &&
        canonical.length() > 0L
}
