package dev.erfnemati.mirza

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.erfnemati.mirza.core.MirzaCore
import dev.erfnemati.mirza.core.MobileException
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.CopyOnWriteArrayList

/** One speech service's settings. */
data class SpeechSettings(
    val model: String,
    val languages: List<String>,
    val strictLanguages: Boolean,
    val terms: List<String>,
)

data class BubbleSettings(
    val show: Boolean,
    /** Diameter in dp. */
    val size: Int,
    /** Percent, while idle. */
    val opacity: Int,
    val logo: Boolean,
    val lockScreen: Boolean,
)

/** What a gesture on the bubble does; the ids are the config's. */
enum class Gesture(val id: String, val label: String) {
    DICTATE("dictate", "Dictate"),
    REVISE("revise", "Dictate and revise with AI"),
    TASK("task", "Say a task"),
    OPEN_APP("open_app", "Open Mirza"),
    NONE("none", "Nothing"),
    ;

    /** Whether it records, so a tap can start at once and a double tap only changes what happens after. */
    val records get() = this == DICTATE || this == REVISE || this == TASK

    companion object {
        fun of(id: String) = entries.firstOrNull { it.id == id } ?: NONE
    }
}

data class Gestures(
    val tap: Gesture,
    val doubleTap: Gesture,
    val longPress: Gesture,
    /** Android's accessibility button. */
    val button: Gesture,
    val volumeLongPress: Gesture,
    val volumeDoublePress: Gesture,
)

data class RevisionSettings(
    /** "" when off. */
    val provider: String,
    val timeoutSec: Long,
    val style: String,
    val presets: List<String>,
    val instructions: String,
)

data class AiSettings(val model: String, val baseUrl: String, val effort: String)

data class TaskSettings(
    /** "" for the same AI as revision. */
    val provider: String,
    val timeoutSec: Long,
    /** List id; "" for the account's default list. */
    val defaultList: String,
    val eventMinutes: Int,
    val timedDueDate: Boolean,
    /** Other names per list id. */
    val listHints: Map<String, String>,
    /** How many tasks said on the lock screen may wait. */
    val heldLimit: Int,
)

data class Credit(val amount: Double, val since: String)

/** The settings Kotlin needs, read from the core's JSON. The core owns the file and the defaults. */
data class Settings(
    val activeProvider: String,
    val silenceStopSec: Long,
    val maxSessionSec: Long,
    val proxy: String,
    val sounds: Boolean,
    val soundVolume: Int,
    val checkUpdates: Boolean,
    val historySize: Int,
    val persianLetters: Boolean,
    val keepOnClipboard: Boolean,
    val bubble: BubbleSettings,
    val gestures: Gestures,
    val speech: Map<String, SpeechSettings>,
    val revision: RevisionSettings,
    val ai: Map<String, AiSettings>,
    val credit: Map<String, Credit>,
    val tasks: TaskSettings,
) {
    val activeSpeech: SpeechSettings get() = speech.getValue(activeProvider)
    val bubbleShow: Boolean get() = bubble.show

    companion object {
        fun parse(json: String): Settings {
            val o = JSONObject(json)
            val providers = o.getJSONObject("providers")
            val bubble = o.getJSONObject("bubble")
            val gestures = o.getJSONObject("gestures")
            val typing = o.getJSONObject("typing")
            val revision = o.getJSONObject("revision")
            val ai = o.getJSONObject("ai")
            val credit = o.getJSONObject("credit")
            val tasks = o.getJSONObject("tasks")
            val hints = tasks.getJSONObject("list_hints")
            return Settings(
                activeProvider = o.getString("active_provider"),
                silenceStopSec = o.getLong("silence_stop_sec"),
                maxSessionSec = o.getLong("max_session_sec"),
                proxy = o.getString("proxy"),
                sounds = o.getBoolean("sounds"),
                soundVolume = o.getInt("sound_volume"),
                checkUpdates = o.getBoolean("check_updates"),
                historySize = o.getInt("history_size"),
                persianLetters = typing.getBoolean("persian_letters"),
                keepOnClipboard = typing.getBoolean("keep_on_clipboard"),
                bubble = BubbleSettings(
                    show = bubble.getBoolean("show"),
                    size = bubble.getInt("size"),
                    opacity = bubble.getInt("opacity"),
                    logo = bubble.getBoolean("logo"),
                    lockScreen = bubble.getBoolean("lock_screen"),
                ),
                gestures = Gestures(
                    tap = Gesture.of(gestures.getString("tap")),
                    doubleTap = Gesture.of(gestures.getString("double_tap")),
                    longPress = Gesture.of(gestures.getString("long_press")),
                    button = Gesture.of(gestures.getString("button")),
                    volumeLongPress = Gesture.of(gestures.getString("volume_long_press")),
                    volumeDoublePress = Gesture.of(gestures.getString("volume_double_press")),
                ),
                speech = SPEECH_SERVICES.associateWith { id ->
                    val p = providers.getJSONObject(id)
                    SpeechSettings(
                        model = p.getString("model"),
                        languages = p.getJSONArray("languages").strings(),
                        strictLanguages = p.getBoolean("strict_languages"),
                        terms = p.getJSONArray("terms").strings(),
                    )
                },
                revision = RevisionSettings(
                    provider = revision.getString("provider"),
                    timeoutSec = revision.getLong("timeout_sec"),
                    style = revision.getString("style"),
                    presets = revision.getJSONArray("presets").strings(),
                    instructions = revision.getString("instructions"),
                ),
                ai = AI_PROVIDERS.associate { p ->
                    val a = ai.getJSONObject(p.id)
                    p.id to AiSettings(a.getString("model"), a.getString("base_url"), a.getString("effort"))
                },
                credit = credit.keys().asSequence().associateWith { id ->
                    val c = credit.getJSONObject(id)
                    Credit(c.getDouble("amount"), c.getString("since"))
                },
                tasks = TaskSettings(
                    provider = tasks.getString("provider"),
                    timeoutSec = tasks.getLong("timeout_sec"),
                    defaultList = tasks.getString("default_list"),
                    eventMinutes = tasks.getInt("event_minutes"),
                    timedDueDate = tasks.getBoolean("timed_due_date"),
                    listHints = hints.keys().asSequence().associateWith { hints.getString(it) },
                    heldLimit = tasks.getInt("held_limit"),
                ),
            )
        }

        private fun JSONArray.strings() = List(length()) { getString(it) }
    }
}

/** An AI provider for revision, as the settings show it. */
data class AiProvider(
    val id: String,
    val name: String,
    val blurb: String,
    val keyUrl: String?,
    /** Suggestions for the model field; the provider's own list can be loaded too. */
    val models: List<String>,
    /** A local server may not need one. */
    val optionalKey: Boolean = false,
)

/** The AI providers the core knows, in the desktop's order. */
val AI_PROVIDERS = listOf(
    AiProvider(
        "claude", "Claude", "Anthropic's Haiku", "https://console.anthropic.com/settings/keys",
        listOf("claude-haiku-5-5", "claude-sonnet-5-5"),
    ),
    AiProvider("openai", "OpenAI", "GPT, fast and cheap", "https://platform.openai.com/api-keys", listOf("gpt-6-luna")),
    AiProvider(
        "gemini", "Gemini", "Google's Flash-Lite", "https://aistudio.google.com/apikey",
        listOf("gemini-3.5-flash-lite", "gemini-3.8-flash"),
    ),
    AiProvider(
        "openrouter", "OpenRouter", "Many models, one key", "https://openrouter.ai/settings/keys",
        listOf("openai/gpt-6-luna", "google/gemini-3.5-flash-lite", "anthropic/claude-haiku-5.5"),
    ),
    AiProvider("local", "Local server", "vLLM, Ollama, LM Studio", null, emptyList(), optionalKey = true),
)

fun aiProvider(id: String) = AI_PROVIDERS.firstOrNull { it.id == id }

/** The speech services the core knows, in the order the settings show them. */
val SPEECH_SERVICES = listOf("soniox", "elevenlabs", "openai")

fun serviceName(id: String) = when (id) {
    "soniox" -> "Soniox"
    "elevenlabs" -> "ElevenLabs"
    "openai" -> "OpenAI"
    else -> id
}

/**
 * The Rust core and the settings it keeps. `settings` is Compose state, so screens update by
 * themselves; services listen through [listeners]. Changes come from the main thread.
 */
object Core {
    lateinit var api: MirzaCore
        private set

    private val state = mutableStateOf<Settings?>(null)

    /** Read in a composable, the screen updates when it changes. */
    val settings: Settings get() = state.value ?: error("Core.init hasn't run")

    /** Called on the main thread after any setting changed. */
    val listeners = CopyOnWriteArrayList<(Settings) -> Unit>()

    /** Bumped when a key is saved or removed, so screens that show "key saved" update. */
    var keysVersion by mutableStateOf(0)
        private set

    fun init(ctx: Context) {
        api = MirzaCore(ctx.filesDir.resolve("mirza").path)
        state.value = Settings.parse(api.configJson())
        api.configProblem()?.let { AppLog.add("settings: $it") }
    }

    /**
     * Changes one setting by its dotted path, e.g. `set("providers.soniox.languages", listOf("fa"))`;
     * null puts it back to the default. Returns why it was refused, or null.
     */
    fun set(path: String, value: Any?): String? = setJson(path, toJson(value))

    /** [set] with the value as JSON. */
    fun setJson(path: String, json: String): String? {
        try {
            api.setValue(path, json)
        } catch (e: MobileException) {
            return e.message ?: "refused"
        }
        state.value = Settings.parse(api.configJson())
        listeners.forEach { it(settings) }
        return null
    }

    fun keySet(service: String): Boolean {
        keysVersion // read, so Compose asks again after a change
        return api.keySet(service)
    }

    /** Saves a key, or removes it when empty. Returns why it failed, or null. Listeners hear of it too. */
    fun setKey(service: String, key: String): String? {
        return try {
            api.setKey(service, key)
            keysVersion++
            listeners.forEach { it(settings) }
            null
        } catch (e: MobileException) {
            e.message ?: "couldn't save the key"
        }
    }

    private fun toJson(v: Any?): String = when (v) {
        null -> "null"
        is String -> JSONObject.quote(v)
        is Boolean, is Number -> v.toString()
        is List<*> -> JSONArray(v).toString()
        else -> error("unsupported setting value ${v::class.simpleName}")
    }
}
