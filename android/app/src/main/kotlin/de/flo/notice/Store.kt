package de.flo.notice

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Zustand der App; lädt alles vom lokalen Server und hält es für Compose bereit. */
class Store(private val api: Api, private val scope: CoroutineScope) {
    var notes by mutableStateOf(listOf<Note>())
    var contacts by mutableStateOf(listOf<Contact>())
    var wiki by mutableStateOf(listOf<Wiki>())
    var depts by mutableStateOf(listOf<Dept>())
    var archive by mutableStateOf(listOf<Note>())
    var trash by mutableStateOf(Trash(emptyList(), emptyList()))
    var config by mutableStateOf(JSONObject())
    var message by mutableStateOf<String?>(null)
    var loaded by mutableStateOf(false)

    fun refresh() {
        scope.launch {
            try {
                val r = withContext(Dispatchers.IO) {
                    listOf(api.notes(), api.contacts(), api.wiki(), api.departments(), api.archive(), api.trash(), api.config())
                }
                @Suppress("UNCHECKED_CAST")
                notes = (r[0] as List<Note>).sortedWith(compareByDescending<Note> { it.pinned }.thenBy { it.sortOrder })
                @Suppress("UNCHECKED_CAST")
                contacts = (r[1] as List<Contact>).sortedBy { it.name.lowercase() }
                @Suppress("UNCHECKED_CAST")
                wiki = (r[2] as List<Wiki>).sortedBy { it.title.lowercase() }
                @Suppress("UNCHECKED_CAST")
                depts = r[3] as List<Dept>
                @Suppress("UNCHECKED_CAST")
                archive = r[4] as List<Note>
                trash = r[5] as Trash
                config = r[6] as JSONObject
                loaded = true
            } catch (e: Exception) {
                message = e.message ?: "Fehler"
            }
        }
    }

    /** Aktion auf dem Server ausführen und danach alles neu laden. */
    fun act(done: (() -> Unit)? = null, block: Api.() -> Unit) {
        scope.launch {
            try {
                withContext(Dispatchers.IO) { api.block() }
                refresh()
                done?.invoke()
            } catch (e: Exception) {
                message = e.message ?: "Fehler"
            }
        }
    }

    fun setStatus(n: Note, status: String) = act { patchNote(n.id, JSONObject().put("status", status)) }
    fun togglePin(n: Note) = act { patchNote(n.id, JSONObject().put("pinned", !n.pinned)) }

    /** Eigene Ladefunktion für Export (liest rohe Daten). */
    suspend fun <T> io(block: Api.() -> T): T = withContext(Dispatchers.IO) { api.block() }
}
