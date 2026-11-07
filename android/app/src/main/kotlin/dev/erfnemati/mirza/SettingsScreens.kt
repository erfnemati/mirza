package dev.erfnemati.mirza

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.RevisedText
import dev.erfnemati.mirza.core.UsageInfo
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Persian inside an English line, kept right to left so its «» don't flip. */
private fun fa(s: String) = "⁧$s⁩"

private val STYLES = listOf(
    Triple("clean", "Clean only", "Fillers, repeats, punctuation. Your words stay."),
    Triple("light", "Light polish", "Also grammar and run-on sentences, few changes."),
    Triple("full", "Full polish", "Reads like written text. Every point stays."),
)

private class Preset(val id: String, val name: String, val detail: String)

private val PRESETS = listOf(
    Preset("plain_words", "Plain words", "No words that sound AI-written (delve, leverage, robust…) and no jargon or bookish words."),
    Preset("formal_persian", "Formal Persian", "Written Persian instead of spoken: ${fa("«می‌خوام»")} becomes ${fa("«می‌خواهم»")}."),
    Preset("digits", "Numbers as digits", "${fa("«صد و پنجاه هزار»")} becomes ${fa("«۱۵۰ هزار»")}, “twenty” becomes 20."),
    Preset("no_dashes", "No dashes or emoji", "Commas and periods instead of dashes between sentences."),
)

private val EFFORTS = listOf(
    "default" to "The model's default",
    "none" to "None (fastest)",
    "minimal" to "Minimal",
    "low" to "Low",
    "medium" to "Medium",
    "high" to "High (slowest)",
)

private fun problem(e: Throwable) = (e as? MobileException)?.message ?: describe(e)

@Composable
fun RevisionScreen(a: Actions) {
    val s = Core.settings
    val r = s.revision
    val p = aiProvider(r.provider)
    Intro(
        "When you stop talking, an AI model cleans up the text before it's typed: it drops fillers and repeats, " +
            "keeps only your last correction, and fixes punctuation and half-spaces. It never translates.",
    )
    val g = s.gestures
    if (p != null && Gesture.REVISE !in listOf(g.tap, g.doubleTap, g.longPress)) {
        Notice("No gesture starts it yet. Choose “Dictate and revise with AI” for one in Bubble and gestures.")
        ButtonRow { FilledTonalButton(onClick = { a.go(Screen.BUBBLE) }) { Text("Bubble and gestures") } }
    }
    Section("Provider") {
        RadioRows(
            r.provider,
            listOf(Triple("", "Off", "Type exactly what was heard")) +
                AI_PROVIDERS.map { Triple(it.id, it.name, it.blurb) },
        ) { Core.set("revision.provider", it) }
    }
    if (p == null) return
    val c = s.ai.getValue(p.id)
    val path = "ai.${p.id}"
    if (!p.optionalKey && !Core.keySet(p.id)) Notice("Add your ${p.name} API key to use AI revision.")
    Section(if (p.optionalKey) "API key (optional)" else "${p.name} API key") {
        KeyField(p.id, p.name)
        p.keyUrl?.let { url -> ButtonRow { TextButton(onClick = { a.open(url) }) { Text("Get a ${p.name} key") } } }
    }
    Section("Model") {
        if (p.id == "local") {
            TextSetting(
                "Server address",
                c.baseUrl,
                "Where your server's OpenAI-compatible API is, ending in /v1. Servers on your network are reached directly, not through the proxy.",
                keyboard = KeyboardType.Uri,
            ) { Core.set("$path.base_url", it.trim().ifEmpty { null }) }
        }
        ModelField(p, c.model)
        ChoiceRow("Thinking", "How much the model may think first. More is slower and rarely helps here.", c.effort, EFFORTS) {
            Core.set("$path.effort", it)
        }
        if (p.id != "local") {
            TextSetting(
                "Server address",
                c.baseUrl,
                "Leave empty unless you use a gateway in front of the provider.",
                keyboard = KeyboardType.Uri,
            ) { Core.set("$path.base_url", it.trim()) }
        }
    }
    Section("How much to change") {
        RadioRows(r.style, STYLES) { Core.set("revision.style", it) }
        Text(
            "Polish takes a little longer. Line breaks are never added, since Enter would send a chat message.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
        )
    }
    Section("Rules") {
        PRESETS.forEach { pr ->
            SwitchRow(pr.name, pr.detail, pr.id in r.presets) { on ->
                // Keep ids a newer version wrote, so switching here doesn't drop them.
                val known = PRESETS.map { it.id }
                val ids = known.filter { if (it == pr.id) on else it in r.presets }
                Core.set("revision.presets", ids + r.presets.filter { it !in known })
            }
        }
        TextSetting(
            "Your own rules",
            r.instructions,
            "Added to Mirza's rules and the switches above, and they win over them. For example: Use British spelling.",
            singleLine = false,
        ) { Core.set("revision.instructions", it.trim()) }
    }
    Section(null) {
        NumberSetting(
            "Time limit in seconds",
            r.timeoutSec.toInt(),
            1..120,
            "If the AI hasn't answered by then, the original text is typed. Long dictations get a little more.",
        ) { Core.set("revision.timeout_sec", it) }
    }
    Section("Try it") { TryRevision(p) }
}

/** The model name, with the provider's own list (or Mirza's suggestions) to pick from. */
@Composable
private fun ModelField(p: AiProvider, model: String) {
    var models by remember(p.id) { mutableStateOf<List<String>>(emptyList()) }
    var loading by remember(p.id) { mutableStateOf(false) }
    var error by remember(p.id) { mutableStateOf<String?>(null) }
    var open by remember { mutableStateOf(false) }
    val keys = Core.keysVersion
    val base = Core.settings.ai.getValue(p.id).baseUrl
    LaunchedEffect(p.id, keys, base) {
        models = emptyList()
        error = null
        if (!p.optionalKey && !Core.keySet(p.id)) return@LaunchedEffect
        loading = true
        val r = withContext(Dispatchers.IO) { runCatching { Core.api.aiModels(p.id) } }
        loading = false
        r.onSuccess { models = it }.onFailure { error = problem(it) }
    }
    val hint = when {
        loading -> "Loading the list of models…"
        error != null -> "Couldn't load the list ($error). You can still type a model name."
        p.id == "local" -> "Leave empty to use the first model the server lists."
        else -> "The default is fast and cheap, which is all cleaning up text needs."
    }
    TextSetting("Model", model, hint) { Core.set("ai.${p.id}.model", it.trim().ifEmpty { null }) }
    val list = models.ifEmpty { p.models }
    if (list.isNotEmpty()) {
        Box(Modifier.padding(horizontal = 8.dp)) {
            TextButton(onClick = { open = true }) {
                Text(if (models.isNotEmpty()) "Choose from ${p.name}'s list" else "Choose a suggested model")
                Icon(Icons.Filled.ArrowDropDown, contentDescription = null)
            }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                list.forEach { m ->
                    DropdownMenuItem(text = { Text(m) }, onClick = {
                        open = false
                        Core.set("ai.${p.id}.model", m)
                    })
                }
            }
        }
    }
}

@Composable
private fun TryRevision(p: AiProvider) {
    var text by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<RevisedText?>(null) }
    var ms by remember { mutableLongStateOf(0L) }
    var runs by remember { mutableIntStateOf(0) }
    Text(
        if (p.id == "local") "Sent to your server, not stored by Mirza." else "Sent to ${p.name}, like your dictation will be.",
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
    )
    OutlinedTextField(
        value = text,
        onValueChange = { text = it },
        placeholder = { Text("Type or paste something the way you'd say it, fillers and all.") },
        minLines = 3,
        textStyle = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
    )
    ButtonRow {
        Button(onClick = { runs++ }, enabled = !busy && text.isNotBlank()) { Text(if (busy) "Revising…" else "Revise") }
        if (busy) CircularProgressIndicator(Modifier.size(24.dp))
    }
    LaunchedEffect(runs) {
        if (runs == 0) return@LaunchedEffect
        busy = true
        val start = System.currentTimeMillis()
        val input = text
        result = withContext(Dispatchers.IO) { Core.api.revise(input) }
        ms = System.currentTimeMillis() - start
        busy = false
    }
    result?.let { r ->
        if (!r.revised) {
            Box(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) { Notice(r.note ?: "Not revised.") }
        } else {
            Text(
                r.text,
                style = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content),
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
            Text(
                String.format(Locale.US, "Took %.1f s.", ms / 1000.0),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
fun BubbleScreen() {
    val ctx = LocalContext.current
    val s = Core.settings
    val b = s.bubble
    val g = s.gestures
    Section(null) {
        SwitchRow("Show the bubble", "Over every app. Drag it anywhere; the place is kept.", b.show) { Core.set("bubble.show", it) }
        SwitchRow(
            "On the lock screen",
            "Mirza never types there: what you say becomes a task that waits until you unlock and confirm it.",
            b.lockScreen,
        ) { Core.set("bubble.lock_screen", it) }
    }
    Section("Look") {
        SliderSetting("Size", b.size, 40..88, 4, { "$it dp" }) { Core.set("bubble.size", it) }
        SliderSetting("Opacity while idle", b.opacity, 30..100, 10, { "$it%" }) { Core.set("bubble.opacity", it) }
        SwitchRow("Mirza's logo", "The teal square of the app icon. Off: a plain grey circle.", b.logo) { Core.set("bubble.logo", it) }
        ButtonRow {
            OutlinedButton(onClick = {
                Bubble.resetPlace(ctx)
                TypingService.instance?.redrawBubble()
                toast("The bubble is back at the right edge.")
            }) { Text("Move it back to the start") }
        }
    }
    Section("Gestures") {
        val all = Gesture.entries.map { it to it.label }
        ChoiceRow("Tap", null, g.tap, all.filter { it.first != Gesture.NONE }) { Core.set("gestures.tap", it.id) }
        ChoiceRow("Double tap", null, g.doubleTap, all) { Core.set("gestures.double_tap", it.id) }
        ChoiceRow("Long press", null, g.longPress, all) { Core.set("gestures.long_press", it.id) }
        Text(
            "While Mirza listens, a tap stops it. A tap that dictates starts at once; a second tap right after only " +
                "changes what that dictation becomes, so a plain tap never waits.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
        if (Gesture.REVISE in listOf(g.tap, g.doubleTap, g.longPress) && aiProvider(s.revision.provider) == null) {
            Box(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                Notice("AI revision is off, so “Dictate and revise” types the text as it was said. Choose a provider in AI revision.")
            }
        }
    }
    Section("Other ways to start") {
        val all = Gesture.entries.map { it to it.label }
        ChoiceRow(
            "Accessibility button",
            "Android's own button, in the navigation bar or floating at the edge of the screen.",
            g.button,
            all,
        ) { Core.set("gestures.button", it.id) }
        if (!ButtonService.running) {
            SmallNote("To get the button, turn on “Mirza button” in Android's accessibility settings.")
            ButtonRow {
                OutlinedButton(onClick = { ctx.startActivity(Setup.accessibilityIntent(ctx, ButtonService::class.java)) }) {
                    Text("Accessibility settings")
                }
            }
        }
        ChoiceRow("Hold volume down", null, g.volumeLongPress, all) { Core.set("gestures.volume_long_press", it.id) }
        ChoiceRow("Press volume down twice", null, g.volumeDoublePress, all) { Core.set("gestures.volume_double_press", it.id) }
        SmallNote(
            "Volume down works only while the screen is on, and never during a call. While Mirza listens, one press " +
                "stops it. Otherwise a short press still turns the volume down; with “twice” set, a moment later.",
        )
    }
}

@Composable
private fun SmallNote(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

@Composable
fun ListeningScreen() {
    val s = Core.settings
    Intro("When Mirza stops listening.")
    Section(null) {
        SwitchRow("Stop when I stop talking", "Ends dictation after a stretch of silence.", s.silenceStopSec > 0) {
            Core.set("silence_stop_sec", if (it) 60 else 0)
        }
        if (s.silenceStopSec > 0) {
            NumberSetting("Seconds of silence", s.silenceStopSec.toInt(), 5..600, null) { Core.set("silence_stop_sec", it) }
        }
        NumberSetting(
            "Time limit in minutes",
            (s.maxSessionSec / 60).toInt(),
            1..120,
            "The longest one dictation can run.",
        ) { Core.set("max_session_sec", it * 60) }
    }
}

@Composable
fun TypingScreen() {
    val s = Core.settings
    Intro(
        "Mirza types each phrase into the app you're in, as soon as the speech service is sure of it. " +
            "With AI revision it types the whole text at the end.",
    )
    Section(null) {
        SwitchRow(
            "Persian letters",
            "Write ${fa("ی")} and ${fa("ک")} instead of the Arabic ${fa("ي")} and ${fa("ك")}.",
            s.persianLetters,
        ) { Core.set("typing.persian_letters", it) }
        SwitchRow(
            "Keep a copy in the clipboard",
            "After each dictation, the whole text is also in your clipboard.",
            s.keepOnClipboard,
        ) { Core.set("typing.keep_on_clipboard", it) }
    }
}

private val USAGE_PAGES = mapOf(
    "soniox" to "https://console.soniox.com",
    "elevenlabs" to "https://elevenlabs.io/app/usage",
    "openai" to "https://platform.openai.com/usage",
)

private fun money(v: Double) =
    if (v != 0.0 && kotlin.math.abs(v) < 0.01) String.format(Locale.US, "$%.4f", v) else String.format(Locale.US, "$%.2f", v)

@Composable
fun UsageScreen(a: Actions) {
    val s = Core.settings
    val id = s.activeProvider
    val name = serviceName(id)
    Intro("What you've spent on $name.")
    if (id != "soniox") {
        Section(null) {
            Text(
                "$name doesn't share usage with apps, so Mirza can't show it.",
                modifier = Modifier.padding(16.dp),
            )
            ButtonRow { FilledTonalButton(onClick = { a.open(USAGE_PAGES.getValue(id)) }) { Text("Open $name usage") } }
        }
        return
    }
    if (!Core.keySet(id)) {
        Notice("Enter your Soniox API key in Speech service first.")
        return
    }
    var usage by remember { mutableStateOf<UsageInfo?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var updated by remember { mutableLongStateOf(0L) }
    var loads by remember { mutableIntStateOf(0) }
    val credit = s.credit[id]
    LaunchedEffect(loads, credit) {
        loading = true
        val r = withContext(Dispatchers.IO) { runCatching { Core.api.sonioxUsage() } }
        loading = false
        r.onSuccess {
            usage = it
            error = null
            updated = System.currentTimeMillis()
        }.onFailure { error = problem(it) }
    }
    val u = usage
    if (u != null && error != null) Notice("Usage wasn't updated: $error")
    Section(null) {
        if (u == null) {
            Text(
                if (error != null) "Couldn't get usage: $error" else "Loading usage…",
                modifier = Modifier.padding(16.dp),
            )
        } else {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                Stat("This month", money(u.month), Modifier.weight(1f))
                Stat("Today", money(u.today), Modifier.weight(1f))
            }
            Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                Stat("Audio this month", "${u.monthMinutes.toLong()} min", Modifier.weight(1f))
                Stat("Dictations", u.requests.toString(), Modifier.weight(1f))
            }
            if (updated > 0) {
                Text(
                    "Updated at " + DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(updated)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
        }
        ButtonRow {
            OutlinedButton(onClick = { loads++ }, enabled = !loading) {
                Icon(Icons.Filled.Refresh, contentDescription = null, modifier = Modifier.padding(end = 8.dp))
                Text(if (loading) "Refreshing…" else "Refresh")
            }
        }
    }
    Balance(name, credit, u)
}

@Composable
private fun Stat(label: String, value: String, modifier: Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.titleLarge)
    }
}

/** Soniox doesn't report the balance, so it's entered once and Mirza subtracts what's used since. */
@Composable
private fun Balance(name: String, credit: Credit?, u: UsageInfo?) {
    val tracking = credit != null && credit.amount > 0 && credit.since.isNotEmpty()
    var editing by remember { mutableStateOf(false) }
    if (tracking && !editing) {
        Section(null) {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                Text("Balance left", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                val left = u?.sinceCredit?.let { money(credit.amount - it) } ?: "…"
                Text(left, style = MaterialTheme.typography.headlineMedium)
                val since = runCatching {
                    DateFormat.getDateInstance(DateFormat.MEDIUM).format(SimpleDateFormat("yyyy-MM-dd", Locale.US).parse(credit.since)!!)
                }.getOrDefault(credit.since)
                Text(
                    "You had ${money(credit.amount)} on $since. Mirza subtracts what you've used since then.",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            ButtonRow {
                OutlinedButton(onClick = { editing = true }) { Text("Update") }
                TextButton(onClick = {
                    Core.set("credit.soniox.amount", null)
                    Core.set("credit.soniox.since", null)
                }) { Text("Stop tracking") }
            }
        }
        return
    }
    Section("Track your balance") {
        Text(
            "$name doesn't tell apps how much credit you have left. Enter the balance your $name account shows now, " +
                "and Mirza will count down from it.",
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
        )
        TextSetting("Balance now, in dollars", "", null, keyboard = KeyboardType.Decimal) { text ->
            val v = text.trim().replace(',', '.').toDoubleOrNull()
            if (v == null || v <= 0) return@TextSetting "Enter the balance in dollars, like 12.50."
            val today = SimpleDateFormat("yyyy-MM-dd", Locale.US).format(Date())
            Core.set("credit.soniox.amount", v) ?: Core.set("credit.soniox.since", today).also { if (it == null) editing = false }
        }
        if (editing) ButtonRow { TextButton(onClick = { editing = false }) { Text("Cancel") } }
    }
}
