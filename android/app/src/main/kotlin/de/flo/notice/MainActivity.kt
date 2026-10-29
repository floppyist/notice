package de.flo.notice

import android.graphics.Color as AColor
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var store by mutableStateOf<Store?>(null)
    private var failed by mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(AColor.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(AColor.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        setContent {
            NoticeApp(store = store, failed = failed, activity = this)
        }
        Reminders.ensureChannel(this)
        if (!Reminders.hasPermission(this) && android.os.Build.VERSION.SDK_INT >= 33) {
            registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) {}
                .launch(android.Manifest.permission.POST_NOTIFICATIONS)
        }
        boot()
    }

    private fun boot() {
        failed = false
        scope.launch(Dispatchers.IO) {
            val remote = Remote.enabled(applicationContext)
            // Remote: zentraler Server (HTTPS + Token); sonst eingebauten lokalen Server starten.
            val api = if (remote) Api(Remote.url(applicationContext), Remote.token(applicationContext)) else null
            val ok = if (api != null) runCatching { api.config(); true }.getOrDefault(false) else Server.start(applicationContext)
            launch(Dispatchers.Main) {
                if (ok) {
                    val s = Store(api ?: Api(Server.base), scope)
                    store = s
                    s.onNotes = { Reminders.sync(applicationContext, it) }
                    s.refresh()
                } else {
                    failed = true
                }
            }
        }
    }

    fun retry() = boot()

    override fun onDestroy() {
        if (isFinishing) Server.stop()
        super.onDestroy()
    }
}
