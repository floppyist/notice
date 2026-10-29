package de.flo.notice

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.horizontalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Design-Tokens, siehe DESIGN.md (dotfiles-Palette). */
object C {
    val bg = Color(0xFF191C1C)
    val surface = Color(0xFF252A2A)
    val overlay = Color(0xFF313838)
    val fg = Color(0xFFEBDBB2)
    val subtle = Color(0xFFA89984)
    val muted = Color(0xFF7C6F64)
    val accent = Color(0xFFCC241D)
    val accentText = Color(0xFFFB4934)
    val onAccent = Color(0xFF191C1C)
    val red = Color(0xFFFB4934)
    val green = Color(0xFFB8BB26)
    val yellow = Color(0xFFFABD2F)
    val blue = Color(0xFF83A598)
    val purple = Color(0xFFD3869B)
    val aqua = Color(0xFF8EC07C)
    val orange = Color(0xFFFE8019)

    fun status(s: String): Color = when (s) {
        "backlog" -> blue
        "in_progress" -> aqua
        "review" -> yellow
        "done" -> green
        else -> muted
    }

    fun priority(p: String): Color = when (p) {
        "high" -> red
        "medium" -> yellow
        else -> muted
    }
}

val Mono = FontFamily(
    Font(R.font.jbm_regular, FontWeight.Normal),
    Font(R.font.jbm_bold, FontWeight.Bold),
)

const val BAR = "▌"

@Composable
fun Tx(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = C.fg,
    size: Int = 13,
    bold: Boolean = false,
    maxLines: Int = Int.MAX_VALUE,
    spacing: Int = 0,
) {
    BasicText(
        text = text,
        modifier = modifier,
        style = TextStyle(
            color = color,
            fontSize = size.sp,
            fontFamily = Mono,
            fontWeight = if (bold) FontWeight.Bold else FontWeight.Normal,
            letterSpacing = spacing.sp,
            lineHeight = (size * 1.45f).sp,
        ),
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
    )
}

@Composable
fun TxA(text: AnnotatedString, modifier: Modifier = Modifier, size: Int = 13, color: Color = C.fg) {
    BasicText(
        text = text,
        modifier = modifier,
        style = TextStyle(color = color, fontSize = size.sp, fontFamily = Mono, lineHeight = (size * 1.5f).sp),
    )
}

/** Linker Balken (`▌`-Look) mit Inhalt; `selected` füllt die Fläche mit surface. */
@Composable
fun BarRow(
    barColor: Color,
    modifier: Modifier = Modifier,
    bg: Color = Color.Transparent,
    content: @Composable () -> Unit,
) {
    Row(modifier.height(IntrinsicSize.Min).background(bg)) {
        Box(Modifier.width(3.dp).fillMaxSize().background(barColor))
        Box(Modifier.weight(1f)) { content() }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun Tap(
    modifier: Modifier = Modifier,
    onLong: (() -> Unit)? = null,
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    Box(Modifier.combinedClickable(onClick = onClick, onLongClick = onLong).then(modifier)) { content() }
}

/** Schaltfläche. primary = Akzentfläche, sonst Umriss in overlay. */
@Composable
fun Btn(label: String, onClick: () -> Unit, modifier: Modifier = Modifier, primary: Boolean = false, color: Color? = null) {
    val bg = if (primary) C.accent else C.surface
    val fg = if (primary) C.onAccent else (color ?: C.fg)
    Box(
        modifier
            .heightIn(min = 40.dp)
            .background(bg)
            .border(BorderStroke(1.dp, if (primary) C.accent else C.overlay))
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 8.dp),
        contentAlignment = Alignment.Center,
    ) { Tx(label, color = fg, size = 12, bold = true, spacing = 1) }
}

/** Reiter/Chip: aktiv = Akzent-Block. */
@Composable
fun Chip(label: String, active: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier, dot: Color? = null) {
    Row(
        modifier
            .heightIn(min = 36.dp)
            .background(if (active) C.accent else C.surface)
            .border(BorderStroke(1.dp, if (active) C.accent else C.overlay))
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (dot != null && !active) Box(Modifier.size(7.dp).background(dot))
        Tx(label, color = if (active) C.onAccent else C.subtle, size = 12, bold = active)
    }
}

@Composable
fun Badge(text: String, color: Color, modifier: Modifier = Modifier) {
    Box(modifier.background(color).padding(horizontal = 6.dp, vertical = 1.dp)) {
        Tx(text, color = C.onAccent, size = 10, bold = true, spacing = 1)
    }
}

@Composable
fun Tag(text: String, color: Color = C.aqua) {
    Box(Modifier.border(BorderStroke(1.dp, C.overlay)).padding(horizontal = 6.dp, vertical = 1.dp)) {
        Tx(text, color = color, size = 10)
    }
}

@Composable
fun Divider(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().height(1.dp).background(C.overlay))
}

@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier, right: String? = null) {
    Row(modifier.fillMaxWidth().padding(top = 16.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Tx(text.uppercase(), color = C.subtle, size = 11, bold = true, spacing = 1, modifier = Modifier.weight(1f))
        if (right != null) Tx(right, color = C.muted, size = 11)
    }
}

@Composable
fun Panel(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Box(modifier.fillMaxWidth().background(C.surface).border(BorderStroke(1.dp, C.overlay)).padding(12.dp)) { content() }
}

/** Beschriftetes Eingabefeld, eckig; Fokus = Akzentrahmen. */
@Composable
fun Field(
    value: String,
    onChange: (String) -> Unit,
    label: String? = null,
    modifier: Modifier = Modifier,
    singleLine: Boolean = true,
    minLines: Int = 1,
    placeholder: String = "",
    keyboard: KeyboardOptions = KeyboardOptions.Default,
) {
    var focused by remember { mutableStateOf(false) }
    Column(modifier) {
        if (label != null) {
            Tx(label.uppercase(), color = if (focused) C.accentText else C.muted, size = 10, bold = true, spacing = 1, modifier = Modifier.padding(bottom = 4.dp))
        }
        Box(
            Modifier.fillMaxWidth()
                .background(C.bg)
                .border(BorderStroke(1.dp, if (focused) C.accent else C.overlay))
                .padding(horizontal = 10.dp, vertical = 10.dp),
        ) {
            if (value.isEmpty() && placeholder.isNotEmpty()) Tx(placeholder, color = C.muted)
            BasicTextField(
                value = value,
                onValueChange = onChange,
                singleLine = singleLine,
                minLines = minLines,
                keyboardOptions = keyboard,
                textStyle = TextStyle(color = C.fg, fontSize = 13.sp, fontFamily = Mono, lineHeight = 19.sp),
                cursorBrush = SolidColor(C.accentText),
                modifier = Modifier.fillMaxWidth().onFocusChanged { focused = it.isFocused },
            )
        }
    }
}

/** Auswahl aus festen Werten als Chip-Reihe. */
@Composable
fun <T> Choice(label: String, options: List<Pair<T, String>>, selected: T, onSelect: (T) -> Unit) {
    Column {
        Tx(label.uppercase(), color = C.muted, size = 10, bold = true, spacing = 1, modifier = Modifier.padding(bottom = 4.dp))
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            options.forEach { (v, l) -> Chip(l, v == selected, { onSelect(v) }) }
        }
    }
}

data class SheetItem(val label: String, val color: Color = C.fg, val dot: Color? = null, val current: Boolean = false, val onClick: () -> Unit)

/** Aktionsliste am unteren Rand (ersetzt das Wheel-Menü). */
@Composable
fun Sheet(title: String, items: List<SheetItem>, onDismiss: () -> Unit) {
    Box(
        Modifier.fillMaxSize()
            .background(Color(0xB8191C1C))
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onDismiss),
        contentAlignment = Alignment.BottomCenter,
    ) {
        Column(
            Modifier.fillMaxWidth()
                .background(C.bg)
                .border(BorderStroke(1.dp, C.overlay))
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) {}
                .windowInsetsPadding(WindowInsets.navigationBars),
        ) {
            Box(Modifier.fillMaxWidth().background(C.surface).padding(horizontal = 14.dp, vertical = 10.dp)) {
                Tx("$BAR $title", color = C.subtle, size = 11, maxLines = 1)
            }
            Divider()
            items.forEach { it ->
                Row(
                    Modifier.fillMaxWidth()
                        .background(if (it.current) C.surface else Color.Transparent)
                        .clickable { it.onClick() },
                ) {
                    Box(Modifier.width(3.dp).height(48.dp).background(if (it.current) C.accent else Color.Transparent))
                    Row(
                        Modifier.heightIn(min = 48.dp).padding(horizontal = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        if (it.dot != null) Box(Modifier.size(8.dp).background(it.dot))
                        Tx(it.label, color = it.color, size = 14)
                    }
                }
                Divider()
            }
        }
    }
}

/** Bestätigungsdialog im selben Stil. */
@Composable
fun Confirm(message: String, yes: String, no: String, onYes: () -> Unit, onNo: () -> Unit) {
    Box(
        Modifier.fillMaxSize().background(Color(0xB8191C1C))
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onNo),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier.padding(24.dp).fillMaxWidth().background(C.bg).border(BorderStroke(1.dp, C.accentText)).padding(16.dp)
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) {},
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Tx(message, size = 14)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Btn(yes, onYes, Modifier.weight(1f), primary = true)
                Btn(no, onNo, Modifier.weight(1f))
            }
        }
    }
}
