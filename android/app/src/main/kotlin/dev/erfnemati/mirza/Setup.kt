package dev.erfnemati.mirza

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings as AndroidSettings

/** What a setup step's button does. */
enum class Fix { MIC, NOTIFICATIONS, TYPING, TRIAL, KEY, AUTOSTART, BATTERY }

data class Step(
    val fix: Fix,
    val title: String,
    val detail: String,
    val done: Boolean,
    val button: String,
    /** Dictation works without it. */
    val optional: Boolean = false,
)

/** The setup checklist on the home screen. Steps Android can't report are ticked by hand. */
object Setup {
    private val prefs get() = MirzaApp.app.getSharedPreferences("state", 0)

    val xiaomi: Boolean = Build.MANUFACTURER.lowercase() in setOf("xiaomi", "redmi", "poco")

    fun steps(ctx: Context): List<Step> {
        val list = mutableListOf<Step>()
        list += Step(
            Fix.MIC,
            "Microphone",
            "Mirza records only while the bubble is red.",
            granted(ctx, Manifest.permission.RECORD_AUDIO),
            "Allow",
        )
        if (Build.VERSION.SDK_INT >= 33) {
            list += Step(
                Fix.NOTIFICATIONS,
                "Notifications",
                "Only while listening, with a Stop button.",
                granted(ctx, Manifest.permission.POST_NOTIFICATIONS),
                "Allow",
                optional = true,
            )
        }
        val running = TypingService.running
        list += Step(
            Fix.TYPING,
            "Typing",
            when {
                running -> "On. It shows the bubble and types into other apps."
                TypingService.enabledInSettings(ctx) ->
                    "On in Accessibility but not running (a force stop does that). Turn Mirza off and on there."
                else ->
                    "In Accessibility, open Installed apps (or Downloaded apps), then Mirza, and turn it on. " +
                        "If Android says the setting is restricted: App info, the menu at the top, Allow restricted settings, then try again."
            },
            running,
            "Open Accessibility",
        )
        if (TypingService.enabledInSettings(ctx, TypingService.TRIAL)) {
            list += Step(
                Fix.TRIAL,
                "Turn off the trial",
                "The Mirza trial's typing is still on, so two bubbles show. Turn off \"Mirza trial typing\" in Accessibility, or uninstall the trial.",
                false,
                "Open Accessibility",
            )
        }
        val service = Core.settings.activeProvider
        list += Step(
            Fix.KEY,
            "${serviceName(service)} API key",
            "Mirza sends your voice to ${serviceName(service)} with your own key.",
            Core.keySet(service),
            "Enter key",
        )
        if (xiaomi) {
            list += Step(
                Fix.AUTOSTART,
                "Xiaomi: Autostart",
                "Turn on Autostart for Mirza, so the phone restarts typing after it stops it.",
                prefs.getBoolean("done_autostart", false),
                "Open",
            )
            list += Step(
                Fix.BATTERY,
                "Xiaomi: Battery saver",
                "In App info, Battery saver, choose No restrictions.",
                prefs.getBoolean("done_battery", false),
                "Open",
            )
        }
        return list
    }

    /** Marks a step Android can't check. */
    fun tick(fix: Fix) {
        val key = when (fix) {
            Fix.AUTOSTART -> "done_autostart"
            Fix.BATTERY -> "done_battery"
            else -> return
        }
        prefs.edit().putBoolean(key, true).apply()
    }

    fun canTick(fix: Fix) = fix == Fix.AUTOSTART || fix == Fix.BATTERY

    private fun granted(ctx: Context, perm: String) = ctx.checkSelfPermission(perm) == PackageManager.PERMISSION_GRANTED

    /**
     * The accessibility settings, with Mirza marked where the phone supports that. Mirza's own page
     * there (ACCESSIBILITY_DETAILS_SETTINGS) needs a system permission, so apps can't open it.
     */
    fun accessibilityIntent(ctx: Context, service: Class<*> = TypingService::class.java): Intent {
        val name = ComponentName(ctx, service).flattenToString()
        val highlight = Bundle().apply { putString(":settings:fragment_args_key", name) }
        return Intent(AndroidSettings.ACTION_ACCESSIBILITY_SETTINGS)
            .putExtra(":settings:fragment_args_key", name)
            .putExtra(":settings:show_fragment_args", highlight)
    }

    fun appInfoIntent(ctx: Context) =
        Intent(AndroidSettings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${ctx.packageName}"))

    fun autostartIntent(): Intent =
        Intent().setClassName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity")
}
