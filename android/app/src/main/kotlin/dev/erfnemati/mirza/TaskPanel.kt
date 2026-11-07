package dev.erfnemati.mirza

import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.TaskDraft
import dev.erfnemati.mirza.core.TaskEdit
import dev.erfnemati.mirza.core.TaskList
import dev.erfnemati.mirza.core.TaskOutcome
import dev.erfnemati.mirza.core.TaskState
import dev.erfnemati.mirza.core.WaitingTask
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.text.DateFormat
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Date
import java.util.Locale

/**
 * What the task panel shows. Compose state, so the panel follows the session and the task book;
 * null closes it.
 */
object TaskPanel {
    sealed interface View

    /** Recording: what is heard so far. */
    data object Listening : View

    /** Kept on the phone; the AI and Google are at work. */
    data class Saving(val id: String, val text: String, val review: Boolean = false) : View

    data class Done(val outcome: TaskOutcome, val text: String, val review: Boolean = false) : View

    data class Edit(val id: String, val review: Boolean = false) : View

    /** The tasks said on the lock screen, one at a time. */
    data object Review : View

    var view by mutableStateOf<View?>(null)

    /** The panel is in front, so outcomes go to it instead of a notification. */
    var visible = false

    /** A task recording starts: the panel opens and shows what is heard. */
    fun listen(ctx: Context) {
        view = Listening
        ctx.startActivity(Intent(ctx, TaskPanelActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** The recording became task `id`. */
    fun saving(id: String, text: String) {
        if (view == Listening) view = Saving(id, text)
    }

    /** Shows `o` if the panel waits for it; false when a notification should say it instead. */
    fun outcome(o: TaskOutcome): Boolean {
        val v = view
        if (!visible || v !is Saving || v.id != o.id) return false
        view = Done(o, v.text, v.review)
        return true
    }

    /** The recording ended without a task (nothing heard, cancelled, failed). */
    fun sessionEnded() {
        if (view == Listening) view = null
    }

    fun close() {
        view = null
    }
}

/**
 * The task panel: a card at the bottom over whatever app was in front. Opened by a task
 * recording, by Edit on a notification or the Voice tasks screen, and by the "tasks wait for you"
 * notification after unlocking.
 */
class TaskPanelActivity : ComponentActivity() {
    companion object {
        private const val EDIT = "edit"
        private const val REVIEW = "review"

        fun edit(ctx: Context, id: String): Intent =
            Intent(ctx, TaskPanelActivity::class.java).putExtra(EDIT, id).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

        fun review(ctx: Context): Intent =
            Intent(ctx, TaskPanelActivity::class.java).putExtra(REVIEW, true).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        take(intent)
        setContent {
            MirzaTheme {
                val v = TaskPanel.view
                LaunchedEffect(v == null) { if (v == null) finish() }
                Box(
                    contentAlignment = Alignment.BottomCenter,
                    modifier = Modifier
                        .fillMaxSize()
                        .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { leave() },
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
                        modifier = Modifier
                            .fillMaxWidth()
                            .navigationBarsPadding()
                            .imePadding()
                            .padding(12.dp)
                            // Taps on the card stay on the card.
                            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) {},
                    ) {
                        Column(
                            verticalArrangement = Arrangement.spacedBy(12.dp),
                            modifier = Modifier.padding(20.dp).verticalScroll(rememberScrollState()),
                        ) {
                            when (v) {
                                TaskPanel.Listening -> ListeningView()
                                is TaskPanel.Saving -> SavingView(v)
                                is TaskPanel.Done -> DoneView(v)
                                is TaskPanel.Edit -> EditView(v)
                                TaskPanel.Review -> ReviewView()
                                null -> {}
                            }
                        }
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        take(intent)
    }

    private fun take(i: Intent) {
        i.getStringExtra(EDIT)?.let { TaskPanel.view = TaskPanel.Edit(it) }
        if (i.getBooleanExtra(REVIEW, false)) TaskPanel.view = TaskPanel.Review
        i.removeExtra(EDIT)
        i.removeExtra(REVIEW)
    }

    override fun onStart() {
        super.onStart()
        TaskPanel.visible = true
    }

    override fun onStop() {
        super.onStop()
        TaskPanel.visible = false
        if (!isChangingConfigurations) leave()
    }

    /** A tap outside, Back, or another app in front: a recording still ends and is kept. */
    private fun leave() {
        if (TaskPanel.view == TaskPanel.Listening) Session.stop("panel closed")
        TaskPanel.close()
    }
}

@Composable
private fun Title(text: String) {
    Text(text, style = MaterialTheme.typography.titleLarge)
}

@Composable
private fun Said(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun Hint(text: String) {
    Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun Buttons(content: @Composable () -> Unit) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth(),
    ) { content() }
}

@Composable
private fun ListeningView() {
    val finishing = Session.state == Session.State.FINISHING
    Title(if (Session.state == Session.State.LISTENING) "Say a task" else "Just a moment…")
    LinearProgressIndicator(Modifier.fillMaxWidth())
    val live = Session.live
    if (live.isBlank()) {
        Hint("Like “call Sara tomorrow at 3 about the contract”. Tap Done when you've said it.")
    } else {
        Said(live)
    }
    Buttons {
        TextButton(onClick = {
            Session.cancel("panel")
            TaskPanel.close()
        }) { Text("Cancel") }
        Button(onClick = { Session.stop("panel") }, enabled = !finishing) { Text("Done") }
    }
}

@Composable
private fun SavingView(v: TaskPanel.Saving) {
    Title("Saving…")
    LinearProgressIndicator(Modifier.fillMaxWidth())
    Said(v.text)
    Hint("Kept on the phone already, so nothing is lost if this takes a while.")
    Buttons { TextButton(onClick = { TaskPanel.close() }) { Text("Hide") } }
}

@Composable
private fun DoneView(v: TaskPanel.Done) {
    val o = v.outcome
    var busy by remember(o) { mutableStateOf(false) }
    val more = v.review && VoiceTasks.held().isNotEmpty()
    val next = { TaskPanel.view = if (more) TaskPanel.Review else null }
    when (o.state) {
        TaskState.SAVED -> Title("Added: ${o.title}")
        else -> Title(o.title)
    }
    o.body.lineSequence().forEach { Said(it) }
    var undo by remember(o) { mutableStateOf(0) }
    LaunchedEffect(undo) {
        if (undo == 0) return@LaunchedEffect
        busy = true
        val why = withContext(Dispatchers.IO) { VoiceTasks.undo(o.id) }
        busy = false
        if (why != null) return@LaunchedEffect toast("Couldn't undo: $why")
        toast(if (o.state == TaskState.SAVED) "Task undone." else "Task deleted.")
        if (v.review) VoiceTasks.heldChanged()
        next()
    }
    // A saved task needs nothing more: the panel goes away by itself.
    if (o.state == TaskState.SAVED && !v.review) {
        LaunchedEffect(o) {
            delay(8000)
            if (TaskPanel.view == v) TaskPanel.close()
        }
    }
    Buttons {
        if (busy) CircularProgressIndicator(Modifier.size(24.dp))
        TextButton(onClick = { undo++ }, enabled = !busy) { Text(if (o.state == TaskState.SAVED) "Undo" else "Delete") }
        OutlinedButton(onClick = { TaskPanel.view = TaskPanel.Edit(o.id, v.review) }, enabled = !busy) { Text("Edit") }
        Button(onClick = next, enabled = !busy) { Text(if (more) "Next" else "Done") }
    }
}

@Composable
private fun ReviewView() {
    var changes by remember { mutableStateOf(0) }
    val held = remember(changes, VoiceTasks.changes) { VoiceTasks.held() }
    val t = held.firstOrNull()
    if (t == null) {
        Title("Nothing waits")
        Hint("Every task said on the lock screen is done.")
        Buttons { Button(onClick = { TaskPanel.close() }) { Text("Close") } }
        return
    }
    var busy by remember(t.id) { mutableStateOf(false) }
    Title(if (held.size == 1) "Said on the lock screen" else "Said on the lock screen (1 of ${held.size})")
    Hint(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(t.saidMs)))
    Said(t.text)
    Hint("Times are read from when you said it: “tomorrow” is the day after that.")
    var drop by remember(t.id) { mutableStateOf(false) }
    LaunchedEffect(drop) {
        if (!drop) return@LaunchedEffect
        busy = true
        val why = withContext(Dispatchers.IO) { VoiceTasks.undo(t.id) }
        busy = false
        drop = false
        if (why != null) return@LaunchedEffect toast("Couldn't drop it: $why")
        VoiceTasks.heldChanged()
        changes++
    }
    Buttons {
        if (busy) CircularProgressIndicator(Modifier.size(24.dp))
        TextButton(onClick = { drop = true }, enabled = !busy) { Text("Drop") }
        OutlinedButton(onClick = { TaskPanel.view = TaskPanel.Edit(t.id, review = true) }, enabled = !busy) { Text("Edit") }
        Button(onClick = {
            val b = VoiceTasks.book() ?: return@Button
            if (!b.confirm(t.id)) return@Button run { changes++ }
            VoiceTasks.heldChanged()
            TaskPanel.view = TaskPanel.Saving(t.id, t.text, review = true)
            VoiceTasks.save(t.id)
        }, enabled = !busy) { Text("Confirm") }
    }
}

/** A task as the form starts it: the AI's reading, or what was said when there is none yet. */
private fun TaskDraft?.orSaid(text: String) = this ?: TaskDraft(
    title = text, note = "", list = "", listName = "", date = "", time = "", minutes = 0u, whenText = "",
    guesses = emptyList(), raw = true,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun EditView(v: TaskPanel.Edit) {
    var loaded by remember(v.id) { mutableStateOf(false) }
    var said by remember(v.id) { mutableStateOf("") }
    var lists by remember(v.id) { mutableStateOf<List<TaskList>>(emptyList()) }
    var title by remember(v.id) { mutableStateOf("") }
    var note by remember(v.id) { mutableStateOf("") }
    var list by remember(v.id) { mutableStateOf("") }
    var day by remember(v.id) { mutableStateOf("") }
    var time by remember(v.id) { mutableStateOf("") }
    var minutes by remember(v.id) { mutableStateOf("") }
    var missing by remember(v.id) { mutableStateOf(false) }
    var error by remember(v.id) { mutableStateOf<String?>(null) }
    var saving by remember(v.id) { mutableStateOf(false) }
    LaunchedEffect(v.id) {
        val b = VoiceTasks.book()
        val found = withContext(Dispatchers.IO) {
            b?.recent()?.firstOrNull { it.id == v.id }?.let { it.text to it.draft }
                ?: b?.waiting()?.firstOrNull { it.id == v.id }?.let { w: WaitingTask -> w.text to w.draft.orSaid(w.text) }
        }
        if (found == null) {
            missing = true
            return@LaunchedEffect
        }
        val (text, d) = found
        said = text
        title = d.title
        note = d.note
        list = d.list
        day = d.date
        time = d.time
        minutes = if (d.minutes > 0u) d.minutes.toString() else ""
        loaded = true
        lists = withContext(Dispatchers.IO) { runCatching { b?.lists(false) }.getOrNull().orEmpty() }
    }
    Title("Edit task")
    if (missing) {
        Hint("That task isn't among Mirza's recent tasks any more; change it in Google Tasks.")
        Buttons { Button(onClick = { TaskPanel.close() }) { Text("Close") } }
        return
    }
    if (!loaded) return LinearProgressIndicator(Modifier.fillMaxWidth())
    Hint("You said: $said")
    val content = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content)
    OutlinedTextField(
        value = title,
        onValueChange = { title = it },
        label = { Text("Title") },
        singleLine = true,
        textStyle = content,
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedTextField(
        value = note,
        onValueChange = { note = it },
        label = { Text("Note") },
        placeholder = { Text("Extra details, if any") },
        textStyle = content,
        modifier = Modifier.fillMaxWidth(),
    )
    ListPicker(list, lists) { list = it }
    var pickDay by remember { mutableStateOf(false) }
    var pickTime by remember { mutableStateOf(false) }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        OutlinedButton(onClick = { pickDay = true }) { Text(dayLabel(day)) }
        OutlinedButton(onClick = { pickTime = true }) { Text(time.ifEmpty { "No time" }) }
        if (day.isNotEmpty() || time.isNotEmpty()) {
            TextButton(onClick = {
                day = ""
                time = ""
            }) { Text("Clear") }
        }
    }
    if (time.isNotEmpty()) {
        OutlinedTextField(
            value = minutes,
            onValueChange = { minutes = it.filter(Char::isDigit).take(4) },
            label = { Text("Length in minutes") },
            placeholder = { Text(Core.settings.tasks.eventMinutes.toString()) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.width(200.dp),
        )
    }
    Hint(
        when {
            time.isEmpty() -> "Without a time, the task gets no Calendar event."
            Google.signedIn && !Google.calendar -> "Calendar wasn't allowed at sign-in, so there is no event."
            else -> "With a time, it goes into Google Calendar too."
        },
    )
    if (pickDay) {
        val initial = runCatching { LocalDate.parse(day) }.getOrNull() ?: LocalDate.now()
        val state = rememberDatePickerState(initialSelectedDateMillis = initial.atStartOfDay().toInstant(ZoneOffset.UTC).toEpochMilli())
        DatePickerDialog(
            onDismissRequest = { pickDay = false },
            confirmButton = {
                TextButton(onClick = {
                    state.selectedDateMillis?.let { day = Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate().toString() }
                    pickDay = false
                }) { Text("OK") }
            },
            dismissButton = { TextButton(onClick = { pickDay = false }) { Text("Cancel") } },
        ) { DatePicker(state) }
    }
    if (pickTime) {
        val (h, m) = time.split(":").mapNotNull { it.toIntOrNull() }.takeIf { it.size == 2 }?.let { it[0] to it[1] } ?: (9 to 0)
        val state = rememberTimePickerState(initialHour = h, initialMinute = m, is24Hour = true)
        AlertDialog(
            onDismissRequest = { pickTime = false },
            confirmButton = {
                TextButton(onClick = {
                    time = "%02d:%02d".format(state.hour, state.minute)
                    if (day.isEmpty()) day = LocalDate.now().toString()
                    pickTime = false
                }) { Text("OK") }
            },
            dismissButton = { TextButton(onClick = { pickTime = false }) { Text("Cancel") } },
            text = { TimePicker(state) },
        )
    }
    error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    var go by remember(v.id) { mutableStateOf(0) }
    LaunchedEffect(go) {
        if (go == 0) return@LaunchedEffect
        val edit = TaskEdit(title.trim(), note.trim(), list, day, time, minutes.toUIntOrNull() ?: 0u)
        if (edit.title.isEmpty()) return@LaunchedEffect run { error = "A task needs a title." }
        if (edit.time.isNotEmpty() && edit.date.isEmpty()) return@LaunchedEffect run { error = "A time needs a day." }
        val b = VoiceTasks.book() ?: return@LaunchedEffect
        saving = true
        error = null
        val r = withContext(Dispatchers.IO) {
            try {
                Result.success(b.edit(v.id, edit))
            } catch (e: MobileException) {
                Result.failure(e)
            }
        }
        saving = false
        r.onFailure { error = it.message }
        r.onSuccess { o ->
            if (v.review) VoiceTasks.heldChanged()
            VoiceTasks.cancelNote(v.id)
            VoiceTasks.schedule()
            TaskPanel.view = TaskPanel.Done(o, said, v.review)
        }
    }
    Buttons {
        if (saving) CircularProgressIndicator(Modifier.size(24.dp))
        TextButton(onClick = { TaskPanel.view = if (v.review) TaskPanel.Review else null }, enabled = !saving) { Text("Cancel") }
        Button(onClick = { go++ }, enabled = !saving) { Text("Save") }
    }
}

@Composable
private fun ListPicker(value: String, lists: List<TaskList>, pick: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val name = lists.firstOrNull { it.id == value }?.title ?: "Your default list"
    Box {
        OutlinedButton(onClick = { open = true }) { Text("List: $name") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            (listOf("" to "Your default list") + lists.map { it.id to it.title }).forEach { (id, title) ->
                DropdownMenuItem(text = { Text(title) }, onClick = {
                    open = false
                    pick(id)
                })
            }
        }
    }
}

/** "Fri 13 Nov" (with the year when it isn't this one), as the core writes days; "No day" for none. */
private fun dayLabel(day: String): String {
    val d = runCatching { LocalDate.parse(day) }.getOrNull() ?: return day.ifEmpty { "No day" }
    val pattern = if (d.year == LocalDate.now().year) "EEE d MMM" else "EEE d MMM yyyy"
    return d.format(DateTimeFormatter.ofPattern(pattern, Locale.ENGLISH))
}
