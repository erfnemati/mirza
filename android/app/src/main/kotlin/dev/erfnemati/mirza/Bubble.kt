package dev.erfnemati.mirza

import android.accessibilityservice.AccessibilityService
import android.annotation.SuppressLint
import android.content.Intent
import android.graphics.PixelFormat
import android.graphics.drawable.GradientDrawable
import android.os.SystemClock
import android.view.Gravity
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.view.WindowManager
import android.widget.ImageView
import kotlin.math.hypot

/**
 * The floating button, drawn by the typing service as an accessibility overlay (it also shows on
 * the lock screen). What a tap, a double tap and a long press do comes from the settings; a drag
 * moves it and the place is kept. Size and look are fixed for its life: the service makes a new
 * one when they change.
 */
@SuppressLint("ClickableViewAccessibility")
class Bubble(private val svc: AccessibilityService, private val look: BubbleSettings) {
    private val wm = svc.getSystemService(WindowManager::class.java)
    private val size = svc.dp(look.size)
    private val shape = GradientDrawable().apply {
        if (look.logo) {
            // The desktop logo: a rounded square, light teal at the top left, dark at the bottom right.
            shape = GradientDrawable.RECTANGLE
            cornerRadius = size * 0.22f
            orientation = GradientDrawable.Orientation.TL_BR
        } else {
            shape = GradientDrawable.OVAL
        }
    }
    private val view = ImageView(svc).apply {
        setImageResource(if (look.logo) R.drawable.ic_logo_mic else R.drawable.ic_mic)
        scaleType = ImageView.ScaleType.FIT_CENTER
        // The logo's microphone is drawn in place; the plain one fills the middle of the circle.
        if (!look.logo) (size * 0.28f).toInt().let { setPadding(it, it, it, it) }
        background = shape
        elevation = svc.dp(4).toFloat()
        contentDescription = "Mirza: start or stop dictation"
    }
    private val prefs = svc.getSharedPreferences("state", 0)
    private val lp = WindowManager.LayoutParams(
        size,
        size,
        WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
        // Not focusable: the keyboard and the focused field stay with the other app.
        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
        PixelFormat.TRANSLUCENT,
    ).apply {
        gravity = Gravity.TOP or Gravity.START
        val (px, py) = place(svc)
        x = px
        y = py
    }
    private val onState: (Session.State) -> Unit = { paint(it) }
    private var shown = false

    /** When a tap that may become a double tap began something (or is waiting to), else 0. */
    private var firstTapAt = 0L
    private val doubleTap = ViewConfiguration.getDoubleTapTimeout().toLong()
    private val single = Runnable {
        firstTapAt = 0
        act(Core.settings.gestures.tap)
    }

    companion object {
        /** Where the bubble goes: the place it was dragged to, or the right edge a third down. */
        fun place(svc: AccessibilityService): Pair<Int, Int> {
            val prefs = svc.getSharedPreferences("state", 0)
            val screen = svc.resources.displayMetrics
            val size = svc.dp(Core.settings.bubble.size)
            return prefs.getInt("bubble_x", screen.widthPixels - size - svc.dp(8)) to
                prefs.getInt("bubble_y", screen.heightPixels / 3)
        }

        /** Forgets the dragged place; the next bubble starts at the right edge again. */
        fun resetPlace(ctx: android.content.Context) {
            ctx.getSharedPreferences("state", 0).edit().remove("bubble_x").remove("bubble_y").apply()
        }
    }

    init {
        val slop = ViewConfiguration.get(svc).scaledTouchSlop
        val longPress = ViewConfiguration.getLongPressTimeout().toLong()
        var downX = 0f
        var downY = 0f
        var startX = 0
        var startY = 0
        var dragging = false
        var longPressed = false
        val longPressAction = Runnable {
            longPressed = true
            view.removeCallbacks(single)
            firstTapAt = 0
            act(Core.settings.gestures.longPress)
        }
        view.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = e.rawX
                    downY = e.rawY
                    startX = lp.x
                    startY = lp.y
                    dragging = false
                    longPressed = false
                    v.postDelayed(longPressAction, longPress)
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = e.rawX - downX
                    val dy = e.rawY - downY
                    if (!dragging && hypot(dx, dy) > slop) {
                        dragging = true
                        v.removeCallbacks(longPressAction)
                    }
                    if (dragging) {
                        lp.x = startX + dx.toInt()
                        lp.y = startY + dy.toInt()
                        wm.updateViewLayout(view, lp)
                    }
                }
                MotionEvent.ACTION_UP -> {
                    v.removeCallbacks(longPressAction)
                    if (dragging) {
                        prefs.edit().putInt("bubble_x", lp.x).putInt("bubble_y", lp.y).apply()
                    } else if (!longPressed) {
                        tapped()
                    }
                }
                MotionEvent.ACTION_CANCEL -> v.removeCallbacks(longPressAction)
            }
            true
        }
        view.alpha = look.opacity / 100f
    }

    /**
     * A tap. While something runs, it stops it, unless it is the second tap of a double tap. When
     * idle, a tap that records starts at once and a second tap only changes the mode, so a plain tap
     * never waits; a tap that doesn't record waits to see whether a second one follows.
     */
    private fun tapped() {
        val g = Core.settings.gestures
        val now = SystemClock.uptimeMillis()
        val second = firstTapAt != 0L && now - firstTapAt <= doubleTap
        firstTapAt = 0
        if (second) {
            view.removeCallbacks(single)
            if (g.tap.records) {
                // The first tap already started; the double tap changes what that becomes.
                when {
                    Session.state == Session.State.IDLE -> {} // the first tap couldn't start (it said why)
                    g.doubleTap.records -> Session.switchMode(g.doubleTap.mode)
                    g.doubleTap == Gesture.OPEN_APP -> {
                        Session.cancel("double tap")
                        openApp()
                    }
                    else -> Session.toggle()
                }
            } else {
                act(g.doubleTap)
            }
            return
        }
        if (Session.state != Session.State.IDLE) return Session.toggle()
        when {
            g.doubleTap == Gesture.NONE -> act(g.tap)
            g.tap.records -> {
                firstTapAt = now
                act(g.tap)
            }
            else -> {
                firstTapAt = now
                view.postDelayed(single, doubleTap)
            }
        }
    }

    private fun act(g: Gesture) {
        when (g) {
            Gesture.DICTATE, Gesture.REVISE -> Session.toggle(g.mode)
            Gesture.OPEN_APP -> openApp()
            Gesture.NONE -> {}
        }
    }

    private fun openApp() {
        svc.startActivity(Intent(svc, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    fun show(): Boolean {
        if (shown) return true
        return try {
            wm.addView(view, lp)
            shown = true
            Session.stateListeners.add(onState)
            paint(Session.state)
            true
        } catch (e: Exception) {
            AppLog.add("the bubble couldn't be shown: ${describe(e)}")
            false
        }
    }

    fun hide() {
        if (!shown) return
        view.removeCallbacks(single)
        Session.stateListeners.remove(onState)
        runCatching { wm.removeView(view) }
        shown = false
    }

    /** Opacity only while idle; while it works the bubble is always solid, so it's clear what happens. */
    fun setOpacity(percent: Int) {
        view.alpha = if (Session.state == Session.State.IDLE) percent / 100f else 1f
    }

    private fun paint(s: Session.State) {
        val colors = when (s) {
            Session.State.IDLE -> if (look.logo) intArrayOf(LOGO_LIGHT, LOGO_DARK) else intArrayOf(GREY, GREY)
            Session.State.STARTING -> intArrayOf(AMBER, AMBER)
            Session.State.LISTENING -> intArrayOf(RED, RED)
            Session.State.FINISHING -> intArrayOf(PURPLE, PURPLE)
        }
        shape.colors = colors
        view.alpha = if (s == Session.State.IDLE) Core.settings.bubble.opacity / 100f else 1f
    }
}

private const val LOGO_LIGHT = 0xFF2DD4BF.toInt()
private const val LOGO_DARK = 0xFF0F766E.toInt()
private const val GREY = 0xFF455A64.toInt()
private const val AMBER = 0xFFFFA000.toInt()
private const val RED = 0xFFD32F2F.toInt()
private const val PURPLE = 0xFF7B1FA2.toInt()

/** The dictation mode a recording gesture starts. */
val Gesture.mode: Session.Mode get() = if (this == Gesture.REVISE) Session.Mode.REVISE else Session.Mode.DICTATE
