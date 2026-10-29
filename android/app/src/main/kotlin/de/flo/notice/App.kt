package de.flo.notice

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

sealed interface Page {
    data class NoteEdit(val note: Note?) : Page
    data class WikiView(val id: Long) : Page
    data class WikiEdit(val wiki: Wiki?) : Page
    data class ContactEdit(val contact: Contact?) : Page
    data object Contacts : Page
    data object Depts : Page
    data object Archive : Page
    data object Trash : Page
    data object Settings : Page
}

enum class Tab { Today, Board, Calendar, Wiki, More }

/** Navigations-Callbacks für alle Bildschirme. */
class Nav(val push: (Page) -> Unit, val pop: () -> Unit, val goTab: (Tab) -> Unit)

@Composable
fun NoticeApp(store: Store?, failed: Boolean, activity: MainActivity) {
    Box(Modifier.fillMaxSize().background(C.bg)) {
        if (store == null || !store.loaded) {
            Boot(failed) { activity.retry() }
        } else {
            Main(store, activity)
        }
    }
}

@Composable
private fun Boot(failed: Boolean, retry: () -> Unit) {
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(32.dp), verticalArrangement = Arrangement.Center) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Tx(BAR, color = C.accent, size = 28, bold = true)
            Tx(" NOTICE", color = C.accentText, size = 22, bold = true, spacing = 2)
        }
        Tx(
            if (failed) stringResource(R.string.boot_failed) else stringResource(R.string.boot_loading),
            color = if (failed) C.red else C.subtle,
            modifier = Modifier.padding(top = 12.dp),
        )
        if (failed) Btn(stringResource(R.string.retry), retry, Modifier.padding(top = 16.dp), primary = true)
    }
}

@Composable
private fun Main(store: Store, activity: MainActivity) {
    var tab by rememberSaveable { mutableStateOf(Tab.Today) }
    var stack by remember { mutableStateOf(listOf<Page>()) }
    val nav = Nav(
        push = { stack = stack + it },
        pop = { if (stack.isNotEmpty()) stack = stack.dropLast(1) },
        goTab = { tab = it; stack = emptyList() },
    )
    BackHandler(stack.isNotEmpty()) { nav.pop() }
    BackHandler(stack.isEmpty() && tab != Tab.Today) { tab = Tab.Today }

    Column(Modifier.fillMaxSize().imePadding()) {
        Box(Modifier.weight(1f).fillMaxWidth()) {
            val page = stack.lastOrNull()
            if (page != null) {
                PageHost(page, store, nav, activity)
            } else {
                when (tab) {
                    Tab.Today -> DashboardScreen(store, nav)
                    Tab.Board -> BoardScreen(store, nav)
                    Tab.Calendar -> CalendarScreen(store, nav)
                    Tab.Wiki -> WikiScreen(store, nav)
                    Tab.More -> MoreScreen(store, nav)
                }
            }
            val msg = store.message
            if (msg != null) {
                LaunchedEffect(msg) { delay(3500); store.message = null }
                Box(
                    Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(C.accent).clickable { store.message = null }.padding(12.dp),
                ) { Tx(msg, color = C.onAccent, size = 12, bold = true) }
            }
        }
        if (stack.isEmpty()) BottomBar(tab) { nav.goTab(it) }
        else Box(Modifier.windowInsetsPadding(WindowInsets.navigationBars))
    }
}

@Composable
private fun PageHost(page: Page, store: Store, nav: Nav, activity: MainActivity) {
    when (page) {
        is Page.NoteEdit -> NoteEditor(store, nav, page.note)
        is Page.WikiView -> WikiView(store, nav, page.id)
        is Page.WikiEdit -> WikiEditor(store, nav, page.wiki)
        is Page.ContactEdit -> ContactEditor(store, nav, page.contact)
        Page.Contacts -> ContactsScreen(store, nav)
        Page.Depts -> DeptsScreen(store, nav)
        Page.Archive -> ArchiveScreen(store, nav)
        Page.Trash -> TrashScreen(store, nav)
        Page.Settings -> SettingsScreen(store, nav, activity)
    }
}

/** Kopfzeile: `▌ TITEL` auf surface, optional Zurück und Aktionen rechts. */
@Composable
fun TopBar(title: String, back: (() -> Unit)? = null, actions: @Composable RowScope.() -> Unit = {}) {
    Column(Modifier.fillMaxWidth().background(C.surface).windowInsetsPadding(WindowInsets.statusBars)) {
        Row(Modifier.fillMaxWidth().height(48.dp), verticalAlignment = Alignment.CenterVertically) {
            if (back != null) {
                Box(Modifier.width(48.dp).height(48.dp).clickable(onClick = back), contentAlignment = Alignment.Center) {
                    Tx("‹", color = C.accentText, size = 26, bold = true)
                }
            } else {
                Tx(BAR, color = C.accent, size = 20, bold = true, modifier = Modifier.padding(start = 12.dp))
            }
            Tx(title.uppercase(), color = if (back != null) C.fg else C.accentText, size = 14, bold = true, spacing = 1, maxLines = 1, modifier = Modifier.padding(start = if (back != null) 0.dp else 6.dp).weight(1f))
            actions()
            Box(Modifier.width(8.dp))
        }
        Divider()
    }
}

@Composable
fun BarAction(label: String, primary: Boolean = false, onClick: () -> Unit) {
    Box(
        Modifier.height(32.dp).background(if (primary) C.accent else Color0).clickable(onClick = onClick).padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) { Tx(label, color = if (primary) C.onAccent else C.accentText, size = 12, bold = true, spacing = 1) }
}

private val Color0 = androidx.compose.ui.graphics.Color.Transparent

@Composable
private fun BottomBar(tab: Tab, onTab: (Tab) -> Unit) {
    val items = listOf(
        Tab.Today to stringResource(R.string.tab_today),
        Tab.Board to stringResource(R.string.tab_board),
        Tab.Calendar to stringResource(R.string.tab_calendar),
        Tab.Wiki to stringResource(R.string.tab_wiki),
        Tab.More to stringResource(R.string.tab_more),
    )
    Column(Modifier.fillMaxWidth().background(C.surface).windowInsetsPadding(WindowInsets.navigationBars)) {
        Divider()
        Row(Modifier.fillMaxWidth().height(52.dp)) {
            items.forEach { (t, label) ->
                val active = t == tab
                Box(
                    Modifier.weight(1f).height(52.dp).background(if (active) C.accent else Color0).clickable { onTab(t) },
                    contentAlignment = Alignment.Center,
                ) { Tx(label.uppercase(), color = if (active) C.onAccent else C.subtle, size = 11, bold = active, spacing = 1, maxLines = 1) }
            }
        }
    }
}
