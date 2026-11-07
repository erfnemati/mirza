package dev.erfnemati.mirza

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.TaskDraft
import dev.erfnemati.mirza.core.TaskList
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import kotlin.concurrent.thread

/** "Personal · tomorrow 15:00", as the desktop shows a task's place. */
private fun TaskDraft.place() = listName + if (whenText.isEmpty()) "" else " · $whenText"

private val content @Composable get() = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content)

@Composable
private fun Small(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
    )
}

/** One task: its title, where and when, and its buttons under them. */
@Composable
private fun TaskRow(title: String, detail: String, buttons: @Composable () -> Unit) {
    ListItem(
        headlineContent = { Text(title, style = content) },
        supportingContent = {
            Column {
                if (detail.isNotEmpty()) Text(detail, style = MaterialTheme.typography.bodyMedium.copy(textDirection = TextDirection.Content))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) { buttons() }
            }
        },
        colors = clearItem,
    )
}

@Composable
fun TasksScreen(a: Actions) {
    val s = Core.settings
    val t = s.tasks
    val changes = VoiceTasks.changes
    val book = remember { VoiceTasks.book() }
    Intro(
        "Say a task, like “call Sara tomorrow at 3 about the contract”. Mirza saves it to Google Tasks right away, " +
            "and to Google Calendar too when you say a time.",
    )
    if (book == null) {
        Notice("Voice tasks can't start: check the proxy on the General screen. Copy the log in About for details.")
        return
    }
    val g = s.gestures
    val gesture = listOf("Tap" to g.tap, "Double tap" to g.doubleTap, "Long press" to g.longPress).firstOrNull { it.second == Gesture.TASK }?.first
    if (gesture == null) {
        Notice("No gesture says a task yet. Choose “Say a task” for one in Bubble and gestures.")
        ButtonRow { FilledTonalButton(onClick = { a.go(Screen.BUBBLE) }) { Text("Bubble and gestures") } }
    } else {
        Intro(
            "$gesture on the bubble and say the task. A tap with no text field in front makes a task too, and so " +
                "does anything said on the lock screen: that waits until you unlock and confirm it.",
        )
    }

    Section("Google account") {
        if (Google.signedIn) {
            ListItem(
                headlineContent = { Text("Signed in") },
                supportingContent = {
                    Text(
                        if (Google.calendar) {
                            "Mirza can add to your Google Tasks and Calendar."
                        } else {
                            "Signed in without Calendar, so tasks with a time are saved as tasks only. Sign out and in again and tick the Calendar box to get events too."
                        },
                    )
                },
                trailingContent = { OutlinedButton(onClick = { Google.signOut() }) { Text("Sign out") } },
                colors = clearItem,
            )
        } else {
            ListItem(
                headlineContent = { Text("Not signed in") },
                supportingContent = { Text("Tasks you say wait on this phone until you sign in.") },
                trailingContent = { Button(onClick = { a.signIn() }) { Text("Sign in") } },
                colors = clearItem,
            )
        }
        if (Google.problem.isNotEmpty()) Box(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) { Notice(Google.problem) }
        Small("Signing in uses the phone's own connection, not Mirza's proxy; the tasks themselves go through the proxy.")
    }

    if (Google.signedIn) ListsSection(t)
    AiSection(t, s, changes)

    Section("Calendar") {
        NumberSetting("Event length in minutes", t.eventMinutes, 5..1440, "For tasks with a time, when you don't say how long.") {
            Core.set("tasks.event_minutes", it)
        }
        SwitchRow(
            "Due date for timed tasks",
            "A task with a time is also due that day in Google Tasks. Turn off to keep the day only on its Calendar event.",
            t.timedDueDate,
        ) { Core.set("tasks.timed_due_date", it) }
    }

    Section("Lock screen") {
        NumberSetting(
            "Tasks that may wait",
            t.heldLimit,
            1..10,
            "What you say on the lock screen stays on the phone, sent nowhere, until you unlock and confirm it. When this many wait, a new one is refused.",
        ) { Core.set("tasks.held_limit", it) }
        if (!s.bubble.lockScreen) Small("The bubble is hidden on the lock screen now (Bubble and gestures).")
    }

    val waiting = remember(changes) { book.waiting() }
    val held = waiting.filter { it.held }.sortedBy { it.saidMs }
    if (held.isNotEmpty()) {
        Section("Said on the lock screen") {
            held.forEach { w ->
                TaskRow(w.text, DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(w.saidMs))) {}
            }
            ButtonRow { Button(onClick = { a.review() }) { Text("Go through them") } }
        }
    }

    val queued = waiting.filter { !it.held }
    if (queued.isNotEmpty()) {
        Section("Waiting to be saved") {
            ListItem(
                headlineContent = { Text("Kept on this phone") },
                supportingContent = {
                    Text(if (Google.signedIn) "Mirza tries again by itself." else "Saved once you sign in to Google, above.")
                },
                trailingContent = {
                    if (Google.signedIn) {
                        OutlinedButton(onClick = {
                            toast("Trying again…")
                            thread(name = "mirza-retry") {
                                book.retryNow()
                                VoiceTasks.retry()
                            }
                        }) { Text("Try now") }
                    }
                },
                colors = clearItem,
            )
            queued.forEach { w ->
                val d = w.draft
                val detail = listOfNotNull(d?.place(), w.lastError.ifEmpty { "Waiting for its turn." }).joinToString("\n")
                TaskRow(d?.title ?: w.text, detail) {
                    TextButton(onClick = { a.edit(w.id) }) { Text("Edit") }
                    UndoButton(w.id, "Delete")
                }
            }
        }
    }

    Section("Saved lately") {
        val recent = remember(changes) { book.recent() }
        Small("Undo deletes the task, and its Calendar event, from Google.")
        if (recent.isEmpty()) Small("Nothing yet. The tasks you say show up here.")
        recent.forEach { r ->
            TaskRow(r.draft.title, r.draft.place() + if (r.hasEvent) " · in Calendar too" else "") {
                TextButton(onClick = { a.edit(r.id) }) { Text("Edit") }
                UndoButton(r.id, "Undo")
            }
        }
    }

    TrySection(t, s)
}

@Composable
private fun UndoButton(id: String, label: String) {
    var busy by remember(id) { mutableStateOf(false) }
    var go by remember(id) { mutableIntStateOf(0) }
    LaunchedEffect(go) {
        if (go == 0) return@LaunchedEffect
        busy = true
        val why = withContext(Dispatchers.IO) { VoiceTasks.undo(id) }
        busy = false
        if (why == null) {
            VoiceTasks.cancelNote(id)
            VoiceTasks.heldChanged()
            toast(if (label == "Undo") "Task undone." else "Task deleted.")
        } else {
            toast("Couldn't: $why")
        }
    }
    TextButton(onClick = { go++ }, enabled = !busy) { Text(label) }
}

@Composable
private fun ListsSection(t: TaskSettings) {
    var lists by remember { mutableStateOf<List<TaskList>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var reads by remember { mutableIntStateOf(0) }
    LaunchedEffect(reads) {
        loading = true
        val refresh = reads > 0
        val r = withContext(Dispatchers.IO) {
            try {
                Result.success(VoiceTasks.book()?.lists(refresh).orEmpty())
            } catch (e: MobileException) {
                Result.failure(e)
            }
        }
        loading = false
        r.onSuccess {
            lists = it
            error = null
        }
        r.onFailure { error = it.message }
    }
    Section("Lists") {
        ListItem(
            headlineContent = { Text("Mirza puts each task in the list that fits what you said") },
            supportingContent = { Text("Give a list other names, like a client or a project, so Mirza knows those tasks belong there too.") },
            colors = clearItem,
        )
        ButtonRow {
            OutlinedButton(onClick = { reads++ }, enabled = !loading) { Text(if (loading) "Reading…" else "Read again") }
            if (loading) CircularProgressIndicator(Modifier.size(24.dp))
        }
        error?.let { Box(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) { Notice(it) } }
        if (lists.isEmpty() && !loading && error == null) Small("No lists read yet.")
        lists.forEach { l ->
            TextSetting("Other names for ${l.title}", t.listHints[l.id].orEmpty(), "Separated by commas") {
                Core.set("tasks.list_hints.${l.id}", it.trim().ifEmpty { null })
            }
        }
        if (lists.isNotEmpty()) {
            ChoiceRow(
                "When nothing fits",
                "Tasks that fit no list go here, and Mirza says so.",
                if (lists.any { it.id == t.defaultList }) t.defaultList else "",
                listOf("" to "Your default list") + lists.map { it.id to it.title },
            ) { Core.set("tasks.default_list", it) }
        }
    }
}

@Composable
private fun AiSection(t: TaskSettings, s: Settings, changes: Int) {
    val rev = aiProvider(s.revision.provider)
    val p = aiProvider(t.provider.ifEmpty { s.revision.provider })
    Section("AI") {
        ChoiceRow(
            "AI for tasks",
            if (p != null) {
                val model = s.ai.getValue(p.id).model.ifEmpty { p.models.firstOrNull() ?: "the server's first model" }
                "Writes a short title, picks the list and reads the day and time. Uses $model, as set for ${p.name} in AI revision."
            } else {
                "Without AI, tasks are saved word for word in the default list, with no day or time."
            },
            t.provider,
            listOf("" to if (rev != null) "Same as AI revision (${rev.name})" else "Same as AI revision (off)") +
                AI_PROVIDERS.map { it.id to it.name },
        ) { Core.set("tasks.provider", it) }
        if (p != null && !p.optionalKey && !Core.keySet(p.id)) {
            KeyField(p.id, p.name)
        } else if (p != null) {
            val problem = remember(changes, Core.keysVersion) { VoiceTasks.book()?.aiProblem().orEmpty() }
            if (problem.isNotEmpty()) Box(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) { Notice(problem) }
        }
        if (p != null) {
            TextSetting(
                "Your own rules",
                t.instructions,
                "For the AI that reads your tasks, added to Mirza's rules and winning over them. AI revision's rules don't apply here. For example: Write the title in English.",
                singleLine = false,
            ) { Core.set("tasks.instructions", it.trim().ifEmpty { null }) }
        }
        NumberSetting(
            "Time limit in seconds",
            t.timeoutSec.toInt(),
            5..120,
            "If the AI hasn't answered by then, the task is saved as you said it.",
        ) { Core.set("tasks.timeout_sec", it) }
    }
}

@Composable
private fun TrySection(t: TaskSettings, s: Settings) {
    val p = aiProvider(t.provider.ifEmpty { s.revision.provider }) ?: return
    var text by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<Result<TaskDraft>?>(null) }
    var ms by remember { mutableLongStateOf(0L) }
    var runs by remember { mutableIntStateOf(0) }
    LaunchedEffect(runs) {
        if (runs == 0) return@LaunchedEffect
        busy = true
        val start = System.currentTimeMillis()
        val input = text.trim()
        result = withContext(Dispatchers.IO) {
            try {
                Result.success(VoiceTasks.book()!!.preview(input))
            } catch (e: MobileException) {
                Result.failure(e)
            }
        }
        ms = System.currentTimeMillis() - start
        busy = false
    }
    Section("Try it") {
        Small("Sent to ${if (p.id == "local") "your server" else p.name}, like the tasks you say. Nothing goes to Google.")
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            placeholder = { Text("Type a task the way you'd say it.") },
            minLines = 2,
            textStyle = content,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        )
        ButtonRow {
            Button(onClick = { runs++ }, enabled = !busy && text.isNotBlank()) { Text(if (busy) "Reading…" else "Read") }
            if (busy) CircularProgressIndicator(Modifier.size(24.dp))
        }
        result?.onFailure { e -> Box(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) { Notice(e.message ?: "Failed.") } }
        result?.onSuccess { d ->
            Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(d.title, style = MaterialTheme.typography.titleMedium.copy(textDirection = TextDirection.Content))
                Text(d.place(), style = content)
                if (d.note.isNotEmpty()) Text(d.note, style = content)
                d.guesses.forEach { Text("Guessed: $it", style = MaterialTheme.typography.bodySmall.copy(textDirection = TextDirection.Content)) }
            }
            Small(String.format(Locale.US, "Took %.1f s. Nothing was saved.", ms / 1000.0))
        }
    }
}
