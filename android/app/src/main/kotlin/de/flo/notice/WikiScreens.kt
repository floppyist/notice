package de.flo.notice

import androidx.compose.foundation.background
import androidx.compose.foundation.text.ClickableText
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable
fun WikiScreen(store: Store, nav: Nav) {
    var query by rememberSaveable { mutableStateOf("") }
    val q = query.trim().lowercase()
    val pages = store.wiki.filter { q.isEmpty() || it.title.lowercase().contains(q) || it.content.lowercase().contains(q) }
    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(R.string.tab_wiki), actions = {
            BarAction(stringResource(R.string.wiki_new_short), primary = true) { nav.push(Page.WikiEdit(null)) }
        })
        Box(Modifier.padding(12.dp)) { Field(query, { query = it }, placeholder = stringResource(R.string.search)) }
        LazyColumn(Modifier.weight(1f).padding(horizontal = 12.dp)) {
            if (pages.isEmpty()) item { Empty(stringResource(R.string.no_wiki)) }
            items(pages, key = { it.id }) { w ->
                Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = { nav.push(Page.WikiView(w.id)) }) {
                    BarRow(C.purple, bg = C.surface) {
                        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
                            Tx(w.title, size = 14, maxLines = 1)
                            if (w.updatedAt.isNotBlank()) Tx(w.updatedAt.take(10), color = C.muted, size = 11)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun WikiView(store: Store, nav: Nav, id: Long) {
    val page = store.wiki.firstOrNull { it.id == id }
    var confirm by remember { mutableStateOf(false) }
    if (page == null) { nav.pop(); return }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(page.title, back = { nav.pop() }, actions = {
                BarAction(stringResource(R.string.edit)) { nav.push(Page.WikiEdit(page)) }
            })
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(12.dp)) {
                Markdown(page.content) { target ->
                    val hit = store.wiki.firstOrNull { it.title.equals(target, true) }
                    if (hit != null) nav.push(Page.WikiView(hit.id)) else store.message = target
                }
                Box(Modifier.padding(top = 24.dp)) { Btn(stringResource(R.string.delete), { confirm = true }, Modifier.fillMaxWidth(), color = C.red) }
            }
        }
        if (confirm) Confirm(stringResource(R.string.confirm_delete, page.title), stringResource(R.string.yes), stringResource(R.string.no),
            onYes = { confirm = false; store.act({ nav.pop() }) { deleteWiki(page.id) } }, onNo = { confirm = false })
    }
}

/** Minimales Markdown: Überschriften, Listen, Zitate, **fett**, `code`, [[Wiki-Links]]. */
@Composable
fun Markdown(src: String, onLink: (String) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        src.split("\n").forEach { raw ->
            val line = raw.trimEnd()
            when {
                line.isBlank() -> Box(Modifier.padding(top = 2.dp))
                line.startsWith("### ") -> Tx(line.drop(4), color = C.subtle, size = 13, bold = true)
                line.startsWith("## ") -> Tx(line.drop(3), size = 15, bold = true)
                line.startsWith("# ") -> Tx(line.drop(2), color = C.accentText, size = 17, bold = true)
                line == "---" || line == "***" -> Divider()
                line.startsWith("> ") -> BarRow(C.muted) { Box(Modifier.padding(start = 10.dp)) { Inline(line.drop(2), C.subtle, onLink) } }
                Regex("^\\s*[-*] \\[( |x|X)] ").containsMatchIn(line) -> {
                    val done = Regex("^\\s*[-*] \\[(x|X)] ").containsMatchIn(line)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Tx(if (done) "✓" else "○", color = if (done) C.green else C.subtle)
                        Inline(line.replace(Regex("^\\s*[-*] \\[( |x|X)] "), ""), if (done) C.muted else C.fg, onLink)
                    }
                }
                Regex("^\\s*[-*] ").containsMatchIn(line) -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Tx("•", color = C.accentText)
                    Inline(line.replace(Regex("^\\s*[-*] "), ""), C.fg, onLink)
                }
                else -> Inline(line, C.fg, onLink)
            }
        }
    }
}

@Composable
private fun Inline(line: String, color: androidx.compose.ui.graphics.Color, onLink: (String) -> Unit) {
    val text = buildInline(line)
    ClickableText(
        text = text,
        style = TextStyle(color = color, fontSize = 13.sp, fontFamily = Mono, lineHeight = 20.sp),
        onClick = { off -> text.getStringAnnotations("wiki", off, off).firstOrNull()?.let { onLink(it.item) } },
    )
}

private fun buildInline(line: String): AnnotatedString = buildAnnotatedString {
    val re = Regex("\\[\\[([^\\]]+)]]|\\*\\*([^*]+)\\*\\*|`([^`]+)`")
    var last = 0
    for (m in re.findAll(line)) {
        append(line.substring(last, m.range.first))
        when {
            m.groups[1] != null -> {
                pushStringAnnotation("wiki", m.groupValues[1])
                withStyle(SpanStyle(color = C.purple, textDecoration = TextDecoration.Underline)) { append(m.groupValues[1]) }
                pop()
            }
            m.groups[2] != null -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(m.groupValues[2]) }
            else -> withStyle(SpanStyle(background = C.surface, color = C.aqua)) { append(" ${m.groupValues[3]} ") }
        }
        last = m.range.last + 1
    }
    append(line.substring(last))
}
