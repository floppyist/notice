package de.flo.notice

import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

class ApiException(message: String) : Exception(message)

/** Dünner Client für die REST-API des lokalen Notice-Servers (blockierend, in IO aufrufen). */
class Api(private val base: String) {
    private fun call(method: String, path: String, body: String? = null): String {
        val c = URL(base + path).openConnection() as HttpURLConnection
        try {
            c.requestMethod = method
            c.connectTimeout = 4000
            c.readTimeout = 15000
            if (body != null) {
                c.doOutput = true
                c.setRequestProperty("Content-Type", "application/json")
                c.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            }
            val code = c.responseCode
            val stream = if (code in 200..299) c.inputStream else c.errorStream
            val text = stream?.bufferedReader(Charsets.UTF_8)?.use { it.readText() } ?: ""
            if (code !in 200..299) throw ApiException("HTTP $code $method $path")
            return text
        } finally {
            c.disconnect()
        }
    }

    private fun arr(path: String) = JSONArray(call("GET", path))

    // --- Notizen ---
    fun notes(): List<Note> = arr("/api/notes").mapObjects(::parseNote)
    fun archive(): List<Note> = arr("/api/archive").mapObjects(::parseNote)

    private fun noteBody(n: Note): JSONObject = JSONObject()
        .put("title", n.title)
        .put("content", n.contentJson())
        .put("status", n.status)
        .put("priority", n.priority)
        .put("due_date", n.dueDate ?: "")
        .put("departments", listToJson(n.departments))
        .put("department", n.departments.firstOrNull() ?: "")
        .put("appointments", n.apptJson())
        .put("repeat_rule", n.repeat ?: "")
        .put("pinned", n.pinned)

    fun createNote(n: Note) { call("POST", "/api/notes", noteBody(n).toString()) }
    fun saveNote(n: Note) { call("PUT", "/api/notes/${n.id}", noteBody(n).toString()) }
    fun patchNote(id: Long, patch: JSONObject) { call("PUT", "/api/notes/$id", patch.toString()) }
    fun deleteNote(id: Long) { call("DELETE", "/api/notes/$id") }
    fun archiveNote(id: Long) { call("POST", "/api/notes/$id/archive", "{}") }
    fun duplicateNote(id: Long) { call("POST", "/api/notes/$id/duplicate", "{}") }
    fun restoreNote(id: Long) { call("POST", "/api/notes/$id/restore", "{}") }
    fun forceDeleteNote(id: Long) { call("DELETE", "/api/notes/$id/force") }
    fun restoreArchived(id: Long) { call("POST", "/api/archive/$id/restore", "{}") }
    fun deleteArchived(id: Long) { call("DELETE", "/api/archive/$id") }

    // --- Papierkorb ---
    fun trash(): Trash {
        val o = JSONObject(call("GET", "/api/trash"))
        return Trash(
            o.optJSONArray("notes")?.mapObjects(::parseNote) ?: emptyList(),
            o.optJSONArray("contacts")?.mapObjects(::parseContact) ?: emptyList(),
        )
    }
    fun clearTrash() { call("POST", "/api/trash/clear", "{}") }

    // --- Kontakte ---
    fun contacts(): List<Contact> = arr("/api/contacts").mapObjects(::parseContact)
    private fun contactBody(c: Contact) = JSONObject()
        .put("name", c.name).put("title", c.title)
        .put("departments", listToJson(c.departments)).put("department", c.departments.firstOrNull() ?: "")
        .put("phone", c.phone).put("mobile", c.mobile).put("fax", c.fax)
        .put("email", c.email).put("description", c.description)
    fun saveContact(c: Contact) {
        if (c.id == 0L) call("POST", "/api/contacts", contactBody(c).toString())
        else call("PUT", "/api/contacts/${c.id}", contactBody(c).toString())
    }
    fun deleteContact(id: Long) { call("DELETE", "/api/contacts/$id") }
    fun restoreContact(id: Long) { call("POST", "/api/contacts/$id/restore", "{}") }
    fun forceDeleteContact(id: Long) { call("DELETE", "/api/contacts/$id/force") }

    // --- Wiki ---
    fun wiki(): List<Wiki> = arr("/api/wiki").mapObjects(::parseWiki)
    fun saveWiki(w: Wiki) {
        val b = JSONObject().put("title", w.title).put("content", w.content).toString()
        if (w.id == 0L) call("POST", "/api/wiki", b) else call("PUT", "/api/wiki/${w.id}", b)
    }
    fun deleteWiki(id: Long) { call("DELETE", "/api/wiki/$id") }

    // --- Einrichtungen ---
    fun departments(): List<Dept> = arr("/api/departments").mapObjects { Dept(it.optString("name"), it.optInt("count")) }
    fun addDepartment(name: String) { call("POST", "/api/departments", JSONObject().put("name", name).toString()) }
    fun removeDepartment(name: String) { call("POST", "/api/departments/delete", JSONObject().put("name", name).toString()) }

    // --- Konfiguration / Import ---
    fun config(): JSONObject = JSONObject(call("GET", "/api/config")).getJSONObject("config")
    fun patchConfig(patch: JSONObject) { call("PUT", "/api/config", patch.toString()) }
    fun exportJson(): String = JSONObject().put("notes", arr("/api/notes")).put("contacts", arr("/api/contacts")).put("wiki", arr("/api/wiki")).toString(2)
    fun importData(payload: String) { call("POST", "/api/import", payload) }
}
