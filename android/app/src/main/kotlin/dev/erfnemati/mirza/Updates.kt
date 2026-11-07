package dev.erfnemati.mirza

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.Release
import kotlin.concurrent.thread

const val PROJECT_URL = "https://github.com/erfnemati/mirza"

/**
 * Looks for a newer release on GitHub (through the proxy, in the core) at most once a day, when the
 * app is opened. Nothing runs in the background for it. A newer release shows a card that opens its
 * page.
 */
object Updates {
    private const val DAY_MS = 24 * 60 * 60 * 1000L
    private val prefs get() = MirzaApp.app.getSharedPreferences("state", 0)

    var available by mutableStateOf<Release?>(null)
        private set
    var checking by mutableStateOf(false)
        private set

    /** What the last check by hand found, for the About screen. */
    var result by mutableStateOf<String?>(null)
        private set

    /** On opening the app: shows what an earlier check found, and checks again after a day. */
    fun onOpen() {
        if (!Core.settings.checkUpdates) {
            available = null
            return
        }
        val version = prefs.getString("update_version", null)
        val url = prefs.getString("update_url", null)
        available = if (version != null && url != null && version != BuildConfig.VERSION_NAME) Release(version, url) else null
        if (System.currentTimeMillis() - prefs.getLong("update_checked", 0) > DAY_MS) check(byHand = false)
    }

    fun check(byHand: Boolean) {
        if (checking) return
        checking = true
        if (byHand) result = null
        thread(name = "mirza-update") {
            val outcome = try {
                Result.success(Core.api.newerRelease())
            } catch (e: MobileException) {
                Result.failure(e)
            }
            toMain {
                checking = false
                outcome.onSuccess { r ->
                    AppLog.add("update check: ${r?.let { "${it.version} is out" } ?: "up to date"}")
                    available = r
                    prefs.edit()
                        .putLong("update_checked", System.currentTimeMillis())
                        .putString("update_version", r?.version)
                        .putString("update_url", r?.url)
                        .apply()
                    if (byHand) result = if (r == null) "Mirza is up to date." else "Mirza ${r.version} is out."
                }
                outcome.onFailure { e ->
                    AppLog.add("update check failed: ${e.message}")
                    if (byHand) result = e.message ?: "The check failed."
                }
            }
        }
    }
}
