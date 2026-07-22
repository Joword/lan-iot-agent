package com.laniot.companion.http

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
                        .put("last_notify", lastNotify),
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
                            .put("last_notify", lastNotify),
                    )

                "notify", "notification", "toast" -> {
                    val title = payload.optString("title", "LanIoT")
                    val body = payload.optString(
                        "body",
                        payload.optString("message", ""),
                    )
                    lastNotify = "$title — $body"
                    json(
                        Response.Status.OK,
                        JSONObject()
                            .put("ok", true)
                            .put("accepted", true)
                            .put("command", "notify")
                            .put("delivered", true)
                            .put("title", title)
                            .put("body", body)
                            .put("note", "stub toast — wire NotificationManager in production"),
                    )
                }

                "lock", "lock_screen" -> {
                    locked = true
                    json(
                        Response.Status.OK,
                        JSONObject()
                            .put("ok", true)
                            .put("accepted", true)
                            .put("command", "lock")
                            .put("locked", true)
                            .put("note", "stub — DevicePolicyManager in production"),
                    )
                }

                "unlock" -> {
                    locked = false
                    json(
                        Response.Status.OK,
                        JSONObject()
                            .put("ok", true)
                            .put("accepted", true)
                            .put("command", "unlock")
                            .put("locked", false),
                    )
                }

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

    private fun json(status: Response.Status, body: JSONObject): Response =
        newFixedLengthResponse(status, "application/json; charset=utf-8", body.toString())

    companion object {
        const val DEFAULT_PORT: Int = 9877
    }
}
