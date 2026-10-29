package de.flo.notice

import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate

data class Check(val text: String, val done: Boolean)

/** Termin innerhalb einer Notiz: start = YYYY-MM-DD, time = HH:MM (optional). */
data class Appt(
    val title: String,
    val start: String,
    val time: String = "",
    val end: String = "",
    val endTime: String = "",
    val done: Boolean = false,
)

data class Note(
    val id: Long = 0,
    val title: String = "",
    val text: String = "",
    val checklist: List<Check> = emptyList(),
    val status: String = "backlog",
    val priority: String = "medium",
    val dueDate: String? = null,
    val departments: List<String> = emptyList(),
    val appointments: List<Appt> = emptyList(),
    val repeat: String? = null,
    val pinned: Boolean = false,
    val sortOrder: Long = 0,
) {
    val overdue: Boolean get() = status != "done" && dueDate != null && dueDate < today()
    val dueToday: Boolean get() = status != "done" && dueDate == today()

    fun contentJson(): String {
        val cl = JSONArray()
        checklist.forEach { cl.put(JSONObject().put("text", it.text).put("done", it.done)) }
        return JSONObject().put("text", text).put("checklist", cl).toString()
    }

    fun apptJson(): String = apptsToJson(appointments)
}

data class Contact(
    val id: Long = 0,
    val name: String = "",
    val title: String = "",
    val departments: List<String> = emptyList(),
    val phone: String = "",
    val mobile: String = "",
    val fax: String = "",
    val email: String = "",
    val description: String = "",
)

data class Wiki(val id: Long = 0, val title: String = "", val content: String = "", val updatedAt: String = "")
data class Dept(val name: String, val count: Int)
data class Trash(val notes: List<Note>, val contacts: List<Contact>)

fun today(): String = LocalDate.now().toString()

private fun JSONObject.str(k: String): String = if (isNull(k)) "" else optString(k, "")
private fun JSONObject.strOrNull(k: String): String? = if (isNull(k)) null else optString(k).ifBlank { null }

fun parseStringList(raw: String?): List<String> {
    if (raw.isNullOrBlank()) return emptyList()
    return try {
        val a = JSONArray(raw)
        (0 until a.length()).map { a.optString(it).trim() }.filter { it.isNotEmpty() }
    } catch (e: Exception) {
        emptyList()
    }
}

fun listToJson(items: List<String>): String = JSONArray(items).toString()

fun apptsToJson(items: List<Appt>): String {
    val a = JSONArray()
    items.forEach {
        val o = JSONObject().put("title", it.title).put("start", it.start)
        if (it.time.isNotBlank()) o.put("time", it.time)
        if (it.end.isNotBlank()) o.put("end", it.end)
        if (it.endTime.isNotBlank()) o.put("endTime", it.endTime)
        if (it.done) o.put("done", true)
        a.put(o)
    }
    return a.toString()
}

fun parseAppts(raw: String?): List<Appt> {
    if (raw.isNullOrBlank()) return emptyList()
    return try {
        val a = JSONArray(raw)
        (0 until a.length()).mapNotNull { i ->
            val o = a.optJSONObject(i) ?: return@mapNotNull null
            Appt(o.str("title"), o.str("start"), o.str("time"), o.str("end"), o.str("endTime"), o.optBoolean("done", false))
        }
    } catch (e: Exception) {
        emptyList()
    }
}

fun parseNote(o: JSONObject): Note {
    val raw = o.str("content")
    var text = raw
    var checks = emptyList<Check>()
    try {
        val c = JSONObject(raw)
        text = c.str("text")
        val a = c.optJSONArray("checklist")
        if (a != null) checks = (0 until a.length()).mapNotNull { i ->
            a.optJSONObject(i)?.let { Check(it.str("text"), it.optBoolean("done", it.optBoolean("checked", false))) }
        }
    } catch (e: Exception) {
        // Altformat: reiner Text
    }
    val deps = parseStringList(o.strOrNull("departments")).ifEmpty { listOfNotNull(o.strOrNull("department")) }
    return Note(
        id = o.getLong("id"),
        title = o.str("title"),
        text = text,
        checklist = checks,
        status = o.str("status").ifBlank { "backlog" },
        priority = o.str("priority").ifBlank { "medium" },
        dueDate = o.strOrNull("due_date"),
        departments = deps,
        appointments = parseAppts(o.strOrNull("appointments")),
        repeat = o.strOrNull("repeat_rule")?.takeIf { it != "none" },
        pinned = o.optInt("pinned", 0) != 0,
        sortOrder = o.optLong("sort_order", 0),
    )
}

fun parseContact(o: JSONObject): Contact = Contact(
    id = o.getLong("id"),
    name = o.str("name"),
    title = o.str("title"),
    departments = parseStringList(o.strOrNull("departments")).ifEmpty { listOfNotNull(o.strOrNull("department")) },
    phone = o.str("phone"),
    mobile = o.str("mobile"),
    fax = o.str("fax"),
    email = o.str("email"),
    description = o.str("description"),
)

fun parseWiki(o: JSONObject): Wiki = Wiki(o.getLong("id"), o.str("title"), o.str("content"), o.str("updated_at"))

fun <T> JSONArray.mapObjects(f: (JSONObject) -> T): List<T> = (0 until length()).mapNotNull { optJSONObject(it)?.let(f) }
