package dev.erfnemati.mirza

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Base64

/**
 * Test hooks for adb; only the shell can send these (see the manifest).
 *
 *   am broadcast -n dev.erfnemati.mirza.trial/dev.erfnemati.mirza.DebugReceiver -a mirza.TYPE --es text_b64 <base64>
 *   ... -a mirza.TAP                      same as tapping the bubble
 *   ... -a mirza.MODE --es mic PROMOTE|SEPARATE|ALWAYS|NONE --es host OVERLAY|ACCESSIBILITY --es typing AUTO|SET_TEXT|PASTE
 *   ... -a mirza.SHOW / mirza.HIDE        show or hide the bubble
 */
class DebugReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            "mirza.TYPE" -> {
                val text = intent.getStringExtra("text_b64")?.let { String(Base64.decode(it, Base64.DEFAULT)) }
                    ?: intent.getStringExtra("text")
                    ?: return
                TrialLog.add("adb type: " + (TypingService.instance?.type(text) ?: "NOT TYPED: the accessibility service is off"))
            }
            "mirza.TAP" -> Session.toggle(ctx, "adb")
            "mirza.MODE" -> {
                intent.getStringExtra("mic")?.let { Prefs.micMode = MicMode.valueOf(it) }
                intent.getStringExtra("host")?.let { Prefs.host = BubbleHost.valueOf(it) }
                intent.getStringExtra("typing")?.let { Prefs.typing = TypingMethod.valueOf(it) }
                TrialLog.add("adb modes: mic ${Prefs.micMode.name}, bubble ${Prefs.host.name}, typing ${Prefs.typing.name}")
            }
            "mirza.SHOW" -> BubbleControl.show(ctx)
            "mirza.HIDE" -> BubbleControl.hide(ctx)
        }
    }
}
