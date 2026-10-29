package de.flo.notice

import android.content.Context
import android.util.Log
import java.io.File
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket

/**
 * Startet den mitgelieferten `notice`-Server (aarch64-Binary, als libnotice.so verpackt)
 * auf einem freien Loopback-Port. Arbeitsverzeichnis ist filesDir: config.toml, *.db und
 * backups/ bleiben bei Updates erhalten.
 */
object Server {
    private const val TAG = "Notice"
    private var process: Process? = null
    var port: Int = 0
        private set

    val base: String get() = "http://127.0.0.1:$port"

    /** Startet den Server (falls nicht schon laufend) und wartet bis zu 15 s auf Erreichbarkeit. */
    fun start(ctx: Context): Boolean {
        if (process?.isAlive == true && canConnect(port)) return true
        val bin = File(ctx.applicationInfo.nativeLibraryDir, "libnotice.so")
        if (!bin.canExecute()) {
            Log.e(TAG, "Binärdatei nicht ausführbar: $bin")
            return false
        }
        port = freePort()
        try {
            val pb = ProcessBuilder(bin.absolutePath, "--port", port.toString())
            pb.directory(ctx.filesDir)
            pb.redirectErrorStream(true)
            pb.redirectOutput(File(ctx.filesDir, "notice.log"))
            process = pb.start()
        } catch (e: Exception) {
            Log.e(TAG, "Serverstart fehlgeschlagen", e)
            return false
        }
        val deadline = System.currentTimeMillis() + 15000
        while (System.currentTimeMillis() < deadline) {
            if (process?.isAlive != true) return false
            if (canConnect(port)) return true
            Thread.sleep(150)
        }
        return false
    }

    fun stop() {
        process?.destroy()
        process = null
    }

    private fun freePort(): Int = try {
        ServerSocket(0).use { it.localPort }
    } catch (e: Exception) {
        8080
    }

    private fun canConnect(p: Int): Boolean = try {
        Socket().use { it.connect(InetSocketAddress("127.0.0.1", p), 300); true }
    } catch (e: Exception) {
        false
    }
}
