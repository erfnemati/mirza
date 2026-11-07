package dev.erfnemati.mirza

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Base64

/**
 * Test hooks for adb, in debug builds only; only the shell can send these (see the manifest).
 *
 *   am broadcast -n dev.erfnemati.mirza/.DebugReceiver -a mirza.TAP     same as tapping the bubble
 *   ... -a mirza.TYPE --es text_b64 <base64>                            types without dictation
 *   ... -a mirza.SET --es path proxy --es json '"socks5://10.0.2.2:10808"'   changes a setting
 *   ... -a mirza.UPDATE                                                 checks for updates now
 */
class DebugReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            "mirza.TAP" -> Session.toggle()
            "mirza.TYPE" -> {
                val text = intent.getStringExtra("text_b64")?.let { String(Base64.decode(it, Base64.DEFAULT)) } ?: return
                val svc = TypingService.instance
                AppLog.add("adb type: " + if (svc == null) "typing is off" else svc.type(text) ?: "typed")
            }
            "mirza.SET" -> {
                val path = intent.getStringExtra("path") ?: return
                val json = intent.getStringExtra("json") ?: "null"
                AppLog.add("adb set $path: ${Core.setJson(path, json) ?: "ok"}")
            }
            "mirza.UPDATE" -> Updates.check(byHand = true)
        }
    }
}
