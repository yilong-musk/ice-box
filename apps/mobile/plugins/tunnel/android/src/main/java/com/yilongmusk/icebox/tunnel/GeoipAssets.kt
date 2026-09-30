// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import android.content.Context
import android.util.Log
import java.io.File
import java.io.FileOutputStream

/**
 * Copy bundled `geoip-*.srs` assets into `filesDir/geoip`.
 *
 * The host writes those paths into `config.json`. The `:tunnel` process reads
 * the same directory. A file is replaced when its size differs from the asset,
 * so an app update can refresh a rule-set without deleting the data directory.
 */
fun ensureGeoipRuleSets(context: Context) {
    val names = context.assets.list(ASSET_DIR) ?: return
    val target = File(context.filesDir, "geoip")
    if (!target.isDirectory && !target.mkdirs()) {
        Log.w(TAG, "geoip directory was not created")
        return
    }
    var replaced = 0
    for (name in names) {
        if (!name.endsWith(".srs")) continue
        if (installAsset(context, "$ASSET_DIR/$name", File(target, name))) {
            replaced += 1
        }
    }
    if (replaced > 0) {
        Log.i(TAG, "installed $replaced geoip rule-set(s)")
    }
}

private fun installAsset(context: Context, assetPath: String, dest: File): Boolean {
    val tmp = File(dest.parentFile, "${dest.name}.partial")
    try {
        context.assets.open(assetPath).use { input ->
            FileOutputStream(tmp).use { output ->
                input.copyTo(output)
                output.fd.sync()
            }
        }
        if (dest.isFile && dest.length() > 0L && dest.length() == tmp.length()) {
            tmp.delete()
            return false
        }
        if (!tmp.renameTo(dest)) {
            tmp.copyTo(dest, overwrite = true)
            tmp.delete()
        }
        return dest.isFile && dest.length() > 0L
    } catch (err: Exception) {
        tmp.delete()
        Log.w(TAG, "geoip $assetPath: ${err.message}")
        return false
    }
}

private const val ASSET_DIR = "rule-set"
private const val TAG = "IceBox"
