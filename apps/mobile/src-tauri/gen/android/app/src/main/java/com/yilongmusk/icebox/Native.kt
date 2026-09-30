// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox

import android.content.Context

/** Host-process JNI. The tunnel process does not load this library. */
object Native {
    init {
        System.loadLibrary("ice_box_mobile_lib")
    }

    @JvmStatic
    external fun initVerifier(context: Context): String
}
