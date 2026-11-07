package dev.erfnemati.mirza

import android.accessibilityservice.AccessibilityService
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.provider.Settings
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import kotlin.math.max
import kotlin.math.min

/**
 * Types text into the focused field of whatever app is in front, at the
 * cursor. It reads the focused field only for that. With bubble host 2 it
 * also draws the bubble, as an accessibility overlay.
 */
class TypingService : AccessibilityService() {
    companion object {
        @Volatile var instance: TypingService? = null
            private set

        /** The app last seen in front, for the log. */
        @Volatile var frontPackage: String? = null
            private set

        /**
         * Whether the service is switched on in the accessibility settings. It can be on there and
         * still not running, until Android binds it or after the app's process died.
         */
        fun enabledInSettings(ctx: Context): Boolean {
            val list = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
                ?: return false
            val me = ComponentName(ctx, TypingService::class.java)
            return list.split(':').any { ComponentName.unflattenFromString(it) == me }
        }
    }

    private var lastEditable: AccessibilityNodeInfo? = null
    private var bubble: Bubble? = null
    private val main = Handler(Looper.getMainLooper())

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        TrialLog.add("accessibility service connected")
        if (Prefs.bubbleOn && Prefs.host == BubbleHost.ACCESSIBILITY) showBubble()
    }

    override fun onAccessibilityEvent(e: AccessibilityEvent) {
        val pkg = e.packageName?.toString() ?: return
        if (pkg == packageName) return
        when (e.eventType) {
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> frontPackage = pkg
            AccessibilityEvent.TYPE_VIEW_FOCUSED, AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED -> {
                val node = e.source
                if (node != null && node.isEditable) lastEditable = node
            }
        }
    }

    override fun onInterrupt() {}

    override fun onUnbind(intent: Intent?): Boolean {
        hideBubble()
        instance = null
        TrialLog.add("accessibility service disconnected")
        return super.onUnbind(intent)
    }

    fun showBubble() {
        if (bubble != null) return
        val b = Bubble(this, WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY)
        if (b.show()) {
            bubble = b
            TrialLog.add("bubble shown (accessibility overlay)")
        }
    }

    fun hideBubble() {
        bubble?.hide()
        bubble = null
    }

    /** Types `text` at the cursor and says how it went, for the log. */
    fun type(text: String): String {
        val node = target() ?: return "NOT TYPED: no focused text field (app in front: ${frontPackage ?: "unknown"})"
        val app = node.packageName
        // Never into a password or PIN field, the lock screen's included: paste would get around the
        // check in setText, and passwords shouldn't go through a speech service.
        if (node.isPassword) return "NOT TYPED: password field in $app"
        val method = Prefs.typing
        if (method != TypingMethod.PASTE) {
            val problem = setText(node, text, forced = method == TypingMethod.SET_TEXT)
                ?: return "typed with SET_TEXT into $app" + check(node, text)
            if (method == TypingMethod.SET_TEXT) return "NOT TYPED: SET_TEXT failed in $app ($problem)"
            TrialLog.add("SET_TEXT failed in $app ($problem), trying paste")
        }
        return if (paste(node, text)) {
            "typed with PASTE into $app" + check(node, text)
        } else {
            "NOT TYPED: paste failed in $app"
        }
    }

    /** The focused editable field in another app, in any window. */
    private fun target(): AccessibilityNodeInfo? {
        val focused = findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
        if (focused != null && focused.isEditable && focused.packageName != packageName) return focused
        val last = lastEditable ?: return null
        return if (last.refresh() && last.isEditable && last.isFocused) last else null
    }

    /** Whether the field's text is really its hint, as a plain EditText says or as text equal to the hint. */
    private fun showsHint(node: AccessibilityNodeInfo): Boolean {
        if (node.isShowingHintText) return true
        val text = node.text?.toString() ?: return false
        return text == node.hintText?.toString()
    }

    private fun fieldText(node: AccessibilityNodeInfo): String =
        if (showsHint(node)) "" else node.text?.toString() ?: ""

    /**
     * Replaces the field's text with itself plus `text` at the cursor. Null on success.
     * `forced`: the SET_TEXT mode was picked by hand, so don't hand over to paste.
     */
    private fun setText(node: AccessibilityNodeInfo, text: String, forced: Boolean): String? {
        val old = fieldText(node)
        var s = node.textSelectionStart
        var e = node.textSelectionEnd
        TrialLog.add("field before typing: ${old.length} chars, hint ${if (showsHint(node)) "shown" else "not shown"}, cursor $s..$e")
        if (!forced && old.isNotEmpty() && (s < 0 || e < 0)) {
            // A text field reports a cursor whenever it holds text. Text without a cursor is most
            // likely the hint shown as text, as Telegram and WhatsApp do with "Message": setting the
            // text would keep the hint, or could drop real text. Paste goes in at the real cursor.
            return "the field has text but no cursor, so it may be the hint"
        }
        if (s < 0 || e < 0 || s > old.length || e > old.length) {
            s = old.length
            e = old.length
        }
        val a = min(s, e)
        val b = max(s, e)
        val new = old.substring(0, a) + text + old.substring(b)
        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, new)
        }
        if (!node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) return "the app refused it"
        val pos = a + text.length
        val sel = Bundle().apply {
            putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, pos)
            putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, pos)
        }
        if (!node.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, sel)) {
            TrialLog.add("note: moving the cursor after the text was refused")
        }
        if (node.refresh()) {
            val now = fieldText(node)
            if (now != new) TrialLog.add("note: the field holds ${now.length} chars after SET_TEXT, expected ${new.length}")
        }
        return null
    }

    /** Puts `text` on the clipboard, pastes it, and clears the clipboard. */
    private fun paste(node: AccessibilityNodeInfo, text: String): Boolean {
        val cm = getSystemService(ClipboardManager::class.java)
        val clip = ClipData.newPlainText("Mirza", text)
        if (Build.VERSION.SDK_INT >= 33) {
            // Keeps the text out of the clipboard preview (and history, where respected).
            clip.description.extras = PersistableBundle().apply {
                putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
            }
        }
        cm.setPrimaryClip(clip)
        val ok = node.performAction(AccessibilityNodeInfo.ACTION_PASTE)
        main.postDelayed({ runCatching { cm.clearPrimaryClip() } }, 500)
        return ok
    }

    /** Whether the half-spaces made it into the field, when the text has any. */
    private fun check(node: AccessibilityNodeInfo, text: String): String {
        val sent = text.count { it == '‌' }
        if (sent == 0 || !node.refresh()) return ""
        val field = fieldText(node)
        val inField = field.count { it == '‌' }
        return "; half-spaces sent $sent, field now has $inField" +
            if (field.contains(text)) ", text found intact" else ", text NOT found as sent"
    }
}
