package com.laniot.companion.admin

import android.app.admin.DeviceAdminReceiver
import android.content.Context
import android.content.Intent

/** Device-admin receiver so Companion can call lockNow() after the user enables it. */
class CompanionDeviceAdminReceiver : DeviceAdminReceiver() {
    override fun onEnabled(context: Context, intent: Intent) {
        super.onEnabled(context, intent)
    }
}
