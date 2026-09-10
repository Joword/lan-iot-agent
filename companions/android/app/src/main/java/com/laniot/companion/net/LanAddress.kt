package com.laniot.companion.net

import android.os.Build
import java.net.Inet4Address
import java.net.NetworkInterface

/**
 * Pick an IPv4 Hub can POST back to.
 *
 * Real device: first non-loopback IPv4 (wlan preferred).
 * Emulator: 127.0.0.1 — Hub on the host needs `adb forward tcp:PORT tcp:PORT`.
 */
object LanAddress {
    fun isEmulator(): Boolean {
        val fingerprint = Build.FINGERPRINT.lowercase()
        val model = Build.MODEL.lowercase()
        val product = Build.PRODUCT.lowercase()
        return fingerprint.contains("generic") ||
            fingerprint.contains("emulator") ||
            model.contains("sdk") ||
            model.contains("emulator") ||
            product.contains("sdk") ||
            product.contains("emulator")
    }

    fun advertiseHost(): String {
        if (isEmulator()) {
            return "127.0.0.1"
        }
        return firstIpv4() ?: "127.0.0.1"
    }

    fun firstIpv4(): String? {
        val interfaces = NetworkInterface.getNetworkInterfaces() ?: return null
        val found = mutableListOf<String>()
        for (ni in interfaces) {
            if (!ni.isUp || ni.isLoopback) continue
            val name = ni.name.lowercase()
            for (addr in ni.inetAddresses) {
                if (addr is Inet4Address && !addr.isLoopbackAddress && !addr.isLinkLocalAddress) {
                    val host = addr.hostAddress ?: continue
                    if (name.contains("wlan") || name.contains("wifi") || name.startsWith("ap")) {
                        return host
                    }
                    found.add(host)
                }
            }
        }
        return found.firstOrNull()
    }
}
