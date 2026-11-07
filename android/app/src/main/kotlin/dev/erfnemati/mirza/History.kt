package dev.erfnemati.mirza

import android.content.ClipboardManager
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.DateFormat
import java.util.Date

/** One dictation as it was typed; `raw` is what the speech service heard, when AI revised it. */
data class Recent(val text: String, val raw: String, val atMs: Long)

/**
 * The last texts dictated, newest first, in case something didn't get typed. Kept in the app's
 * private files (no backup), at most `history_size` of them; 0 keeps none.
 */
object History {
    private val file get() = File(MirzaApp.app.filesDir, "mirza/history.json")

    var items by mutableStateOf<List<Recent>>(emptyList())
        private set

    fun load() {
        items = runCatching {
            val a = JSONArray(file.readText())
            List(a.length()) { i ->
                val o = a.getJSONObject(i)
                Recent(o.getString("text"), o.optString("raw"), o.getLong("at"))
            }
        }.getOrDefault(emptyList())
        trim()
    }

    fun add(text: String, raw: String) {
        val max = Core.settings.historySize
        if (max == 0) return
        items = (listOf(Recent(text, raw, System.currentTimeMillis())) + items).take(max)
        save()
    }

    fun clear() {
        items = emptyList()
        file.delete()
    }

    /** Keeps the newest `history_size`; called after any setting changed, so it saves only when it drops some. */
    fun trim() {
        val max = Core.settings.historySize
        if (max == 0) {
            if (items.isNotEmpty() || file.exists()) clear()
        } else if (items.size > max) {
            items = items.take(max)
            save()
        }
    }

    private fun save() {
        val a = JSONArray()
        items.forEach { a.put(JSONObject().put("text", it.text).put("raw", it.raw).put("at", it.atMs)) }
        runCatching {
            file.parentFile?.mkdirs()
            val tmp = File(file.path + ".tmp")
            tmp.writeText(a.toString())
            tmp.renameTo(file)
        }.onFailure { AppLog.add("recent texts not saved: ${describe(it)}") }
    }
}

@Composable
fun HistoryScreen() {
    val ctx = LocalContext.current
    val items = History.items
    val copy = { text: String ->
        ctx.getSystemService(ClipboardManager::class.java).setPrimaryClip(sensitiveClip(text))
        toast("Copied.")
    }
    Text(
        "The last things you dictated, in case something didn't get typed. Kept on this phone only.",
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.padding(horizontal = 16.dp),
    )
    if (items.isEmpty()) {
        Section(null) {
            Text(
                if (Core.settings.historySize == 0) "Keeping recent texts is off." else "Nothing yet. What you dictate shows up here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(16.dp),
            )
        }
    } else {
        Section(null) {
            val time = remember { DateFormat.getTimeInstance(DateFormat.SHORT) }
            val date = remember { DateFormat.getDateInstance(DateFormat.MEDIUM) }
            items.forEachIndexed { i, r ->
                var showRaw by remember(r.atMs) { mutableStateOf(false) }
                Column(Modifier.fillMaxWidth().padding(start = 16.dp, top = if (i == 0) 4.dp else 12.dp)) {
                    Row(verticalAlignment = Alignment.Top) {
                        Text(
                            r.text,
                            style = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content),
                            modifier = Modifier.weight(1f).padding(top = 8.dp),
                        )
                        IconButton(onClick = { copy(r.text) }) { Icon(painterResource(R.drawable.ic_copy), "Copy") }
                    }
                    val at = Date(r.atMs)
                    val sameDay = DateFormat.getDateInstance().format(at) == DateFormat.getDateInstance().format(Date())
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            if (sameDay) time.format(at) else "${date.format(at)}, ${time.format(at)}",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        if (r.raw.isNotEmpty()) {
                            TextButton(onClick = { showRaw = !showRaw }) { Text(if (showRaw) "Hide the original" else "Before AI revision") }
                        }
                    }
                    if (showRaw) {
                        Row(verticalAlignment = Alignment.Top) {
                            Text(
                                r.raw,
                                style = MaterialTheme.typography.bodyMedium.copy(textDirection = TextDirection.Content),
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.weight(1f).padding(top = 8.dp),
                            )
                            IconButton(onClick = { copy(r.raw) }) { Icon(painterResource(R.drawable.ic_copy), "Copy the original") }
                        }
                    }
                }
            }
            ButtonRow {
                OutlinedButton(onClick = {
                    History.clear()
                    toast("Recent texts cleared.")
                }) {
                    Icon(Icons.Filled.Delete, contentDescription = null, modifier = Modifier.padding(end = 8.dp))
                    Text("Clear")
                }
            }
        }
    }
    Section(null) {
        NumberSetting(
            "How many to keep",
            Core.settings.historySize,
            0..200,
            "0 keeps none.",
        ) { Core.set("history_size", it) }
    }
}
