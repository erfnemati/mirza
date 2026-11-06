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

/** The settings Kotlin needs, read from the core's JSON. The core owns the file and the defaults. */
data class Settings(
    val activeProvider: String,
    val silenceStopSec: Long,
    val maxSessionSec: Long,
    val proxy: String,
    val sounds: Boolean,
    val checkUpdates: Boolean,
    val bubbleShow: Boolean,
    val speech: Map<String, SpeechSettings>,
) {
    val activeSpeech: SpeechSettings get() = speech.getValue(activeProvider)

    companion object {
        fun parse(json: String): Settings {
            val o = JSONObject(json)
            val providers = o.getJSONObject("providers")
            return Settings(
                activeProvider = o.getString("active_provider"),
                silenceStopSec = o.getLong("silence_stop_sec"),
                maxSessionSec = o.getLong("max_session_sec"),
                proxy = o.getString("proxy"),
                sounds = o.getBoolean("sounds"),
                checkUpdates = o.getBoolean("check_updates"),
                bubbleShow = o.getJSONObject("bubble").getBoolean("show"),
                speech = SPEECH_SERVICES.associateWith { id ->
                    val p = providers.getJSONObject(id)
                    SpeechSettings(
                        model = p.getString("model"),
                        languages = p.getJSONArray("languages").strings(),
                        strictLanguages = p.getBoolean("strict_languages"),
                        terms = p.getJSONArray("terms").strings(),
                    )
                },
            )
        }

        private fun JSONArray.strings() = List(length()) { getString(it) }
    }
}

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

    /** Saves a key, or removes it when empty. Returns why it failed, or null. */
    fun setKey(service: String, key: String): String? {
        return try {
            api.setKey(service, key)
            keysVersion++
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
