package com.laniot.companion.http

import android.app.admin.DevicePolicyManager
import android.content.ComponentName
import android.content.Context
import com.laniot.companion.admin.CompanionDeviceAdminReceiver
import com.laniot.companion.notify.CompanionNotifier
import fi.iki.elonen.NanoHTTPD
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException

/**
 * Local Companion HTTP listener — mirrors `companions/windows/server.py`.
 *
 * Hub POSTs `{base_url}/command` with `{"command":"…"}` after
 * `POST /api/v1/companions/{id}/command`.
 *
 * Default port **9877** (Windows demo uses 9876).
 */
class CommandServer(
    private val context: Context,
    port: Int = DEFAULT_PORT,
) : NanoHTTPD(port) {

    @Volatile
    var lastNotify: String? = null
        private set

    @Volatile
    var locked: Boolean = false
        private set

    @Throws(IOException::class)
    override fun serve(session: IHTTPSession): Response {
        val path = session.uri.trimEnd('/').ifEmpty { "/" }

        return when {
            session.method == Method.GET && (path == "/" || path == "/health") ->
                json(
                    Response.Status.OK,
                    JSONObject()
                        .put("ok", true)
                        .put("service", "companion.android")
                        .put("port", listeningPort)
                        .put("locked", locked)
                        .put("last_notify", lastNotify)
                        .put("device_admin", isDeviceAdmin()),
                )

            session.method == Method.POST && path == "/command" -> handleCommand(session)

            else ->
                json(
                    Response.Status.NOT_FOUND,
                    JSONObject().put("ok", false).put("error", "not_found"),
                )
        }
    }

    private fun handleCommand(session: IHTTPSession): Response {
        val bodyMap = HashMap<String, String>()
        return try {
            session.parseBody(bodyMap)
            val raw = bodyMap["postData"] ?: "{}"
            val payload = JSONObject(if (raw.isBlank()) "{}" else raw)
            val command = payload.optString("command", "").trim().lowercase()
            when (command) {
                "", "ping", "health" ->
                    json(
                        Response.Status.OK,
                        JSONObject()
                            .put("ok", true)
                            .put("accepted", true)
                            .put("command", if (command.isEmpty()) "ping" else command)
                            .put("service", "companion.android")
                            .put("locked", locked)
                            .put("last_notify", lastNotify)
                            .put("device_admin", isDeviceAdmin()),
                    )

                "notify", "notification", "toast" -> {
                    val title = payload.optString("title", "LanIoT")
                    val body = payload.optString(
                        "body",
                        payload.optString("message", ""),
                    )
                    val delivered = CompanionNotifier.post(context, title, body)
                    lastNotify = "$title — $body"
                    json(
                        if (delivered) Response.Status.OK else Response.Status.BAD_REQUEST,
                        JSONObject()
                            .put("ok", delivered)
                            .put("accepted", delivered)
                            .put("command", "notify")
                            .put("delivered", delivered)
                            .put("title", title)
                            .put("body", body)
                            .put(
                                "error",
                                if (delivered) JSONObject.NULL else "notification_permission_denied",
                            ),
                    )
                }

                "lock", "lock_screen" -> lockNow()

                "unlock" ->
                    json(
                        Response.Status.BAD_REQUEST,
                        JSONObject()
                            .put("ok", false)
                            .put("accepted", false)
                            .put("command", "unlock")
                            .put("locked", locked)
                            .put("error", "unlock_not_supported"),
                    )

                else ->
                    json(
                        Response.Status.BAD_REQUEST,
                        JSONObject()
                            .put("ok", false)
                            .put("accepted", false)
                            .put("command", command)
                            .put("error", "unknown_command")
                            .put(
                                "supported",
                                JSONArray(listOf("ping", "notify", "lock", "unlock")),
                            ),
                    )
            }
        } catch (_: Exception) {
            json(
                Response.Status.BAD_REQUEST,
                JSONObject().put("ok", false).put("error", "invalid_json"),
            )
        }
    }

    private fun lockNow(): Response {
        val dpm = context.getSystemService(Context.DEVICE_POLICY_SERVICE) as DevicePolicyManager
        val admin = ComponentName(context, CompanionDeviceAdminReceiver::class.java)
        if (!dpm.isAdminActive(admin)) {
            return json(
                Response.Status.BAD_REQUEST,
                JSONObject()
                    .put("ok", false)
                    .put("accepted", false)
                    .put("command", "lock")
                    .put("locked", locked)
                    .put("error", "device_admin_not_active")
                    .put("note", "Enable device admin in the Companion app, then retry lock"),
            )
        }
        dpm.lockNow()
        locked = true
        return json(
            Response.Status.OK,
            JSONObject()
                .put("ok", true)
                .put("accepted", true)
                .put("command", "lock")
                .put("locked", true)
                .put("via", "DevicePolicyManager.lockNow"),
        )
    }

    private fun isDeviceAdmin(): Boolean {
        val dpm = context.getSystemService(Context.DEVICE_POLICY_SERVICE) as DevicePolicyManager
        val admin = ComponentName(context, CompanionDeviceAdminReceiver::class.java)
        return dpm.isAdminActive(admin)
    }

    private fun json(status: Response.Status, body: JSONObject): Response =
        newFixedLengthResponse(status, "application/json; charset=utf-8", body.toString())

    companion object {
        const val DEFAULT_PORT: Int = 9877
    }
}
