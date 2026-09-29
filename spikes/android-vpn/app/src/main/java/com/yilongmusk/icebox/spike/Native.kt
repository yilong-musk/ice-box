package com.yilongmusk.icebox.spike

import android.content.Context

/** Rust engine (host process only). */
object Native {
    init {
        System.loadLibrary("icebox_spike")
    }

    @JvmStatic external fun init(context: Context): String

    /** `source`: `url:<subscription URL>`, `raw:<body>`, or `direct`. */
    @JvmStatic external fun buildConfig(source: String, sharedDir: String, secret: String): String

    @JvmStatic external fun httpsCheck(url: String): String

    @JvmStatic external fun clashCheck(secret: String): String

    @JvmStatic external fun clashDelay(secret: String, tag: String): String
}
