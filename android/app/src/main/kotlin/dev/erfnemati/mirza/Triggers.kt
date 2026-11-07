package dev.erfnemati.mirza

import android.accessibilityservice.AccessibilityButtonController
import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.view.KeyEvent
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityEvent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** What a gesture does, wherever it came from: the bubble, the accessibility button or volume down. */
fun perform(ctx: Context, g: Gesture) {
    when (g) {
        Gesture.DICTATE, Gesture.REVISE, Gesture.TASK -> Session.toggle(g.mode)
        Gesture.OPEN_APP -> ctx.startActivity(Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        Gesture.NONE -> {}
    }
}

/** The accessibility button or volume down: stops what runs, like a tap on the bubble, or does `g`. */
fun press(ctx: Context, g: Gesture) {
    if (Session.state != Session.State.IDLE) Session.toggle() else perform(ctx, g)
}

/**
 * "Mirza button", a second accessibility service whose only job is Android's accessibility button.
 * From Android 12 a service asks for the button in its XML only, and then Android turns the button
 * and the service on and off together; on the typing service that would tie typing to the button.
 * It gets no events and reads nothing.
 */
class ButtonService : AccessibilityService() {
    companion object {
        /** Compose state: on and connected, so the button shows. */
        var running by mutableStateOf(false)
            private set
    }

    private val button = object : AccessibilityButtonController.AccessibilityButtonCallback() {
        override fun onClicked(c: AccessibilityButtonController) {
            AppLog.add("accessibility button")
            if (TypingService.instance == null) {
                toast("Turn on Mirza in the accessibility settings too.")
            } else {
                press(this@ButtonService, Core.settings.gestures.button)
            }
        }
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        accessibilityButtonController.registerAccessibilityButtonCallback(button, Handler(Looper.getMainLooper()))
        running = true
        AppLog.add("button service connected")
    }

    override fun onAccessibilityEvent(e: AccessibilityEvent) {}

    override fun onInterrupt() {}

    override fun onUnbind(intent: Intent?): Boolean {
        accessibilityButtonController.unregisterAccessibilityButtonCallback(button)
        running = false
        return super.onUnbind(intent)
    }
}

/**
 * Volume down as a trigger, through the typing service's key filter: held, pressed twice, or both.
 * Only volume down, only with the screen on and outside calls; every other key goes on untouched.
 * A press is held back to see what it becomes, so a plain press (or holding it without a gesture
 * for that) changes the volume here, as Android would have. While Mirza listens, one press stops
 * it, like a tap on the bubble.
 */
class VolumeKey(private val svc: AccessibilityService) {
    private val main = Handler(Looper.getMainLooper())
    private val audio = svc.getSystemService(AudioManager::class.java)
    private val power = svc.getSystemService(PowerManager::class.java)
    private val longTimeout = ViewConfiguration.getLongPressTimeout().toLong()
    /**
     * How long a release waits for a second press. Android holds each volume-down press back
     * 150 ms (it might become the screenshot chord) before Mirza sees it, but not the release, so
     * the double-tap time gets those 150 ms on top.
     */
    private val doubleTimeout = ViewConfiguration.getDoubleTapTimeout().toLong() + 150

    /** A press taken by Mirza is down; its key-up is Mirza's too. */
    private var down = false
    /** This press already did its gesture (long press, or the second of two). */
    private var used = false
    /** Held with no long-press gesture: the volume goes down step by step, as when held normally. */
    private var holding = false
    /** A single press waits to see whether a second one follows. */
    private var waiting = false

    private val held = Runnable {
        val g = Core.settings.gestures.volumeLongPress
        if (g != Gesture.NONE) {
            used = true
            AppLog.add("volume down held")
            press(svc, g)
        } else {
            holding = true
            repeat.run()
        }
    }
    private val repeat = object : Runnable {
        override fun run() {
            lower()
            main.postDelayed(this, 100)
        }
    }
    private val single = Runnable {
        waiting = false
        lower()
    }

    fun wanted(g: Gestures) = g.volumeLongPress != Gesture.NONE || g.volumeDoublePress != Gesture.NONE

    /**
     * Forgets a press in progress, when the keys stop coming (the service goes, or no volume
     * gesture is set any more): its release may never arrive, and a timer must not act for it.
     */
    fun reset() {
        main.removeCallbacks(held)
        main.removeCallbacks(repeat)
        main.removeCallbacks(single)
        down = false
        used = false
        holding = false
        waiting = false
    }

    /** Whether Mirza takes the key. Called on the main thread with every key the phone gets. */
    fun onKey(e: KeyEvent): Boolean {
        if (e.keyCode != KeyEvent.KEYCODE_VOLUME_DOWN) return false
        val g = Core.settings.gestures
        return when (e.action) {
            KeyEvent.ACTION_DOWN -> when {
                // Repeats while held: Android's own long press counts like the timer.
                e.repeatCount > 0 -> {
                    if (down && e.isLongPress && !used && !holding) {
                        main.removeCallbacks(held)
                        held.run()
                    }
                    down
                }
                !wanted(g) || !power.isInteractive || inCall() -> {
                    reset()
                    false
                }
                Session.state != Session.State.IDLE -> {
                    main.removeCallbacks(single)
                    waiting = false
                    down = true
                    used = true
                    AppLog.add("volume down while listening")
                    Session.toggle()
                    true
                }
                waiting && g.volumeDoublePress != Gesture.NONE -> {
                    main.removeCallbacks(single)
                    waiting = false
                    down = true
                    used = true
                    AppLog.add("volume down pressed twice")
                    press(svc, g.volumeDoublePress)
                    true
                }
                else -> {
                    down = true
                    used = false
                    holding = false
                    main.postDelayed(held, longTimeout)
                    true
                }
            }
            KeyEvent.ACTION_UP -> {
                if (!down) return false
                down = false
                main.removeCallbacks(held)
                main.removeCallbacks(repeat)
                when {
                    used || holding -> {}
                    g.volumeDoublePress != Gesture.NONE -> {
                        waiting = true
                        main.postDelayed(single, doubleTimeout)
                    }
                    else -> lower()
                }
                used = false
                holding = false
                true
            }
            else -> down
        }
    }

    /** A step down, on whatever Android would have turned down, with its volume panel. */
    private fun lower() {
        runCatching {
            audio.adjustSuggestedStreamVolume(AudioManager.ADJUST_LOWER, AudioManager.USE_DEFAULT_STREAM_TYPE, AudioManager.FLAG_SHOW_UI)
        }.onFailure { AppLog.add("volume not changed: ${describe(it)}") }
    }

    /** During a call (or while it rings) volume down keeps its own meaning. */
    private fun inCall() = audio.mode != AudioManager.MODE_NORMAL
}
