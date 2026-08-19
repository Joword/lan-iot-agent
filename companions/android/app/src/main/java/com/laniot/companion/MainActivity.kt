package com.laniot.companion

import android.Manifest
import android.app.admin.DevicePolicyManager
import android.content.ComponentName
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import com.laniot.companion.admin.CompanionDeviceAdminReceiver
import com.laniot.companion.http.CommandServer
import com.laniot.companion.notify.CompanionNotifier
import com.laniot.companion.pair.PairTokenStore
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

/**
 * Companion launcher: local `/command` server + Hub pair/register.
 *
 * Flow: pair → `POST {hub}/api/v1/auth/pair` → store `lit_…` →
 * `POST {hub}/api/v1/companions` with this device's LAN base_url.
 */
class MainActivity : AppCompatActivity() {

    private var server: CommandServer? = null
    private lateinit var tokenStore: PairTokenStore
    private lateinit var statusView: TextView
    private val io = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        tokenStore = PairTokenStore(this)
        statusView = findViewById(R.id.status)

        findViewById<Button>(R.id.btn_start).setOnClickListener { startCompanionServer() }
        findViewById<Button>(R.id.btn_stop).setOnClickListener { stopCompanionServer() }
        findViewById<Button>(R.id.btn_pair_stub).setOnClickListener { pairAndRegister() }
        findViewById<Button>(R.id.btn_enable_admin).setOnClickListener { requestDeviceAdmin() }
        CompanionNotifier.ensureChannel(this)
        if (Build.VERSION.SDK_INT >= 33) {
            ActivityCompat.requestPermissions(
                this,
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                1001,
            )
        }

        refreshStatus("idle — start server, then Pair to Hub")
    }

    override fun onDestroy() {
        stopCompanionServer()
        io.shutdownNow()
        super.onDestroy()
    }

    private fun startCompanionServer() {
        if (server != null) {
            refreshStatus("already listening on :${CommandServer.DEFAULT_PORT}")
            return
        }
        val s = CommandServer(applicationContext, port = CommandServer.DEFAULT_PORT)
        s.start(daemon = false)
        server = s
        refreshStatus(
            "listening :${CommandServer.DEFAULT_PORT}/command — tap Pair to register on Hub",
        )
    }

    private fun stopCompanionServer() {
        server?.stop()
        server = null
        refreshStatus("server stopped")
    }

    private fun requestDeviceAdmin() {
        val admin = ComponentName(this, CompanionDeviceAdminReceiver::class.java)
        val intent = Intent(DevicePolicyManager.ACTION_ADD_DEVICE_ADMIN)
            .putExtra(DevicePolicyManager.EXTRA_DEVICE_ADMIN, admin)
            .putExtra(
                DevicePolicyManager.EXTRA_ADD_EXPLANATION,
                "Allows Hub lock commands to lock this phone.",
            )
        startActivity(intent)
    }

    /**
     * Real Hub pair + companion register (runs off main thread).
     * Hub base URL from PairTokenStore (default http://10.0.2.2:3000 for emulator).
     */
    private fun pairAndRegister() {
        refreshStatus("pairing…")
        io.execute {
            try {
                val hub = tokenStore.hubBaseUrl() ?: DEFAULT_HUB
                val pairJson = postJson("$hub/api/v1/auth/pair", JSONObject())
                val token = pairJson.getString("token")
                tokenStore.save(hubBaseUrl = hub, token = token)

                val companionId = "companion.android_${android.os.Build.MODEL}"
                    .replace(Regex("[^a-zA-Z0-9_.]"), "_")
                    .lowercase()
                val baseUrl = "http://${lanHint()}:${CommandServer.DEFAULT_PORT}"
                val body = JSONObject()
                    .put("id", companionId)
                    .put("name", "Android ${android.os.Build.MODEL}")
                    .put("base_url", baseUrl)
                    .put("kind", "phone")
                postJson(
                    "$hub/api/v1/companions",
                    body,
                    bearer = token,
                )
                main.post {
                    refreshStatus(
                        "paired+registered id=$companionId base_url=$baseUrl hub=$hub",
                    )
                }
            } catch (e: Exception) {
                main.post {
                    refreshStatus("pair/register failed: ${e.message}")
                }
            }
        }
    }

    private fun lanHint(): String {
        // Emulator → host loopback via 10.0.2.2 is for Hub; companion itself
        // must be reachable from Hub host — user replaces with real LAN IP.
        return "10.0.2.2"
    }

    private fun postJson(
        url: String,
        body: JSONObject,
        bearer: String? = null,
    ): JSONObject {
        val conn = (URL(url).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            setRequestProperty("Content-Type", "application/json; charset=utf-8")
            if (bearer != null) {
                setRequestProperty("Authorization", "Bearer $bearer")
            }
            doOutput = true
            connectTimeout = 8_000
            readTimeout = 8_000
        }
        conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
        val code = conn.responseCode
        val stream = if (code in 200..299) conn.inputStream else conn.errorStream
        val text = stream?.bufferedReader()?.readText().orEmpty()
        if (code !in 200..299) {
            throw IllegalStateException("HTTP $code $text")
        }
        return if (text.isBlank()) JSONObject() else JSONObject(text)
    }

    private fun refreshStatus(message: String) {
        val paired = tokenStore.token() != null
        statusView.text = buildString {
            appendLine(message)
            appendLine("paired=$paired port=${CommandServer.DEFAULT_PORT}")
            append("hub=${tokenStore.hubBaseUrl() ?: DEFAULT_HUB}")
        }
    }

    companion object {
        /** Android emulator → host machine Hub. */
        const val DEFAULT_HUB: String = "http://10.0.2.2:3000"
    }
}
