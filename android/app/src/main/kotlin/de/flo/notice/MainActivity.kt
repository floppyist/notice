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
        boot()
    }

    private fun boot() {
        failed = false
        scope.launch(Dispatchers.IO) {
            val ok = Server.start(applicationContext)
            launch(Dispatchers.Main) {
                if (ok) {
                    val s = Store(Api(Server.base), scope)
                    store = s
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
