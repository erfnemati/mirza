package dev.erfnemati.mirza

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.erfnemati.mirza.core.coreVersion

enum class Screen(val title: String) { HOME("Mirza"), SPEECH("Speech service"), GENERAL("General"), ABOUT("About") }

/** What the screens ask the activity to do. */
interface Actions {
    fun fix(step: Step)
    fun tick(step: Step)
    fun appInfo()
    fun open(url: String)
    fun go(screen: Screen)
}

@Composable
fun HomeScreen(steps: List<Step>, a: Actions) {
    val s = Core.settings
    val ready = steps.none { !it.done && !it.optional }
    StatusCard(ready)

    Updates.available?.let { r ->
        Card(
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.tertiaryContainer),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Mirza ${r.version} is out", style = MaterialTheme.typography.titleMedium)
                    Text("You have ${BuildConfig.VERSION_NAME}.", style = MaterialTheme.typography.bodyMedium)
                }
                Button(onClick = { a.open(r.url) }) { Text("Open") }
            }
        }
    }

    if (steps.any { !it.done }) {
        Section("Set up") {
            steps.forEach { StepRow(it, a) }
        }
    }

    Section(null) {
        SwitchRow("Show the bubble", "Over every app. Drag it anywhere; a long press opens Mirza.", s.bubbleShow) {
            Core.set("bubble.show", it)
        }
    }

    Section("Settings") {
        val service = s.activeProvider
        NavRow(
            "Speech service",
            serviceName(service) + if (Core.keySet(service)) "" else ", no API key yet",
        ) { a.go(Screen.SPEECH) }
        NavRow("General", "Proxy, sounds, updates") { a.go(Screen.GENERAL) }
        NavRow("About", "Version ${BuildConfig.VERSION_NAME}, log") { a.go(Screen.ABOUT) }
    }
}

@Composable
private fun StatusCard(ready: Boolean) {
    val state = Session.state
    val (title, detail) = when {
        state == Session.State.LISTENING -> "Listening" to "Tap the bubble again to stop."
        state != Session.State.IDLE -> "Working" to "Finishing the last words."
        ready -> "Ready" to "Tap into any text field, then tap the bubble. Tap it again to stop."
        else -> "Almost ready" to "Finish the steps below, then the bubble shows over every app."
    }
    val circle by animateColorAsState(
        when {
            state == Session.State.LISTENING -> Color(0xFFD32F2F)
            ready -> MaterialTheme.colorScheme.primary
            else -> MaterialTheme.colorScheme.outline
        },
        label = "status",
    )
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(56.dp).clip(CircleShape).background(circle),
                contentAlignment = Alignment.Center,
            ) {
                Icon(painterResource(R.drawable.ic_mic), contentDescription = null, tint = Color.White)
            }
            Column(Modifier.padding(start = 16.dp)) {
                Text(title, style = MaterialTheme.typography.headlineSmall)
                Text(detail, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun StepRow(step: Step, a: Actions) {
    ListItem(
        leadingContent = {
            when {
                step.done -> Icon(Icons.Filled.CheckCircle, "Done", tint = MaterialTheme.colorScheme.primary)
                step.optional -> Icon(Icons.Filled.Info, "Optional", tint = MaterialTheme.colorScheme.outline)
                else -> Icon(Icons.Filled.Warning, "To do", tint = MaterialTheme.colorScheme.error)
            }
        },
        headlineContent = { Text(step.title + if (step.optional && !step.done) " (optional)" else "") },
        supportingContent = if (step.done) {
            null
        } else {
            {
                Column {
                    Text(step.detail)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
                        FilledTonalButton(onClick = { a.fix(step) }) { Text(step.button) }
                        if (step.fix == Fix.TYPING) TextButton(onClick = a::appInfo) { Text("App info") }
                        if (Setup.canTick(step.fix)) TextButton(onClick = { a.tick(step) }) { Text("Done") }
                    }
                }
            }
        },
        colors = clearItem,
    )
}

@Composable
private fun NavRow(title: String, detail: String, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(detail) },
        trailingContent = { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null) },
        colors = clearItem,
        modifier = Modifier.clickable(onClick = onClick),
    )
}

private class Service(val id: String, val blurb: String, val keyUrl: String, val strict: Boolean)

private val SERVICES = listOf(
    Service("soniox", "Real-time, strong in Persian", "https://console.soniox.com", true),
    Service("elevenlabs", "Scribe v2 real-time", "https://elevenlabs.io/app/settings/api-keys", true),
    Service("openai", "GPT transcribe and Whisper", "https://platform.openai.com/api-keys", false),
)

@Composable
fun SpeechScreen(a: Actions) {
    val s = Core.settings
    val active = SERVICES.first { it.id == s.activeProvider }
    val c = s.activeSpeech
    val path = "providers.${active.id}"
    Text(
        "Mirza sends your voice to one of these services and types what it hears.",
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.padding(horizontal = 16.dp),
    )
    Section("Service") {
        SERVICES.forEach { svc ->
            val pick = { Core.set("active_provider", svc.id) }
            ListItem(
                leadingContent = { RadioButton(selected = svc == active, onClick = { pick() }) },
                headlineContent = { Text(serviceName(svc.id)) },
                supportingContent = { Text(svc.blurb + if (Core.keySet(svc.id)) "" else ". No key yet.") },
                colors = clearItem,
                modifier = Modifier.clickable { pick() },
            )
        }
    }
    Section("${serviceName(active.id)} API key") {
        KeyField(active.id)
        ButtonRow { TextButton(onClick = { a.open(active.keyUrl) }) { Text("Get a key") } }
    }
    Section("Recognition") {
        TextSetting("Model", c.model, "Empty puts back the default.") { Core.set("$path.model", it.trim().ifEmpty { null }) }
        TextSetting(
            "Languages",
            c.languages.joinToString(", "),
            "Language codes, like fa, en. Empty lets the service work it out.",
        ) { Core.set("$path.languages", splitList(it)) }
        if (active.strict) {
            SwitchRow(
                "Only these languages",
                "Never guess another language. Turn it on if your Persian sometimes comes out as Arabic.",
                c.strictLanguages,
            ) { Core.set("$path.strict_languages", it) }
        }
        TextSetting(
            "Words to expect",
            c.terms.joinToString(", "),
            "Names and terms the service should listen for, separated by commas.",
            singleLine = false,
        ) { Core.set("$path.terms", splitList(it)) }
    }
}

private fun splitList(s: String) = s.split(',', '،', '\n').map { it.trim() }.filter { it.isNotEmpty() }

/** The key is never shown once saved; typing a new one replaces it. */
@Composable
private fun KeyField(service: String) {
    val saved = Core.keySet(service)
    var text by remember(service) { mutableStateOf("") }
    var error by remember(service) { mutableStateOf<String?>(null) }
    val save = {
        error = Core.setKey(service, text.trim())
        if (error == null) {
            text = ""
            toast("Key saved.")
        }
    }
    OutlinedTextField(
        value = text,
        onValueChange = {
            text = it
            error = null
        },
        label = { Text(if (saved) "Saved. Paste a new key to replace it." else "Paste your ${serviceName(service)} API key") },
        singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Password,
            autoCorrectEnabled = false,
            imeAction = ImeAction.Done,
        ),
        keyboardActions = KeyboardActions(onDone = { if (text.isNotBlank()) save() }),
        isError = error != null,
        supportingText = error?.let { { Text(it) } },
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
    )
    ButtonRow {
        Button(onClick = save, enabled = text.isNotBlank()) { Text("Save key") }
        if (saved) {
            OutlinedButton(onClick = { error = Core.setKey(service, "") }) { Text("Remove") }
        }
    }
}

@Composable
fun GeneralScreen() {
    val s = Core.settings
    Section("Network") {
        TextSetting(
            "Proxy",
            s.proxy,
            "Empty connects directly. Or http://host:port, socks5://host:port. Used for every connection.",
            keyboard = KeyboardType.Uri,
        ) { Core.set("proxy", it.trim()) }
    }
    Section(null) {
        SwitchRow("Sounds", "A short sound when dictation starts and stops, and on errors.", s.sounds) { Core.set("sounds", it) }
        SwitchRow(
            "Check for updates",
            "Once a day when you open Mirza, it asks GitHub for the latest version number. Nothing about you or what you say is sent.",
            s.checkUpdates,
        ) {
            Core.set("check_updates", it)
            Updates.onOpen()
        }
    }
}

@Composable
fun AboutScreen(a: Actions) {
    val ctx = LocalContext.current
    Section(null) {
        ListItem(headlineContent = { Text("Version") }, supportingContent = { Text(BuildConfig.VERSION_NAME) }, colors = clearItem)
        ListItem(headlineContent = { Text("Core") }, supportingContent = { Text(remember { coreVersion() }) }, colors = clearItem)
        ButtonRow {
            FilledTonalButton(onClick = { Updates.check(byHand = true) }, enabled = !Updates.checking) { Text("Check for updates") }
            if (Updates.checking) CircularProgressIndicator(Modifier.size(24.dp))
        }
        Updates.result?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
        }
        Updates.available?.let { r -> ButtonRow { Button(onClick = { a.open(r.url) }) { Text("Open ${r.version}") } } }
    }
    Section("Help") {
        ListItem(
            headlineContent = { Text("Copy log") },
            supportingContent = { Text("What Mirza did lately, for a bug report. It never has your words or keys.") },
            colors = clearItem,
            modifier = Modifier.clickable {
                ctx.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Mirza log", AppLog.text()))
                toast("Log copied.")
            },
        )
        ListItem(
            headlineContent = { Text("Project page") },
            supportingContent = { Text("Source, releases and issues on GitHub") },
            colors = clearItem,
            modifier = Modifier.clickable { a.open(PROJECT_URL) },
        )
    }
}
