// SPDX-License-Identifier: GPL-3.0-or-later

package com.yilongmusk.icebox.tunnel

import io.nekohasekai.libbox.NetworkInterfaceIterator
import io.nekohasekai.libbox.StringIterator
import io.nekohasekai.libbox.NetworkInterface as BoxInterface

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
