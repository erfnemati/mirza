package dev.erfnemati.mirza

import android.accessibilityservice.AccessibilityService
import android.annotation.SuppressLint
import android.content.Intent
import android.graphics.PixelFormat
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.view.WindowManager
import android.widget.ImageView
import kotlin.math.hypot

/**
 * The floating button, drawn by the typing service as an accessibility overlay (it also shows on
 * the lock screen). Tap: start or stop dictation. Drag: move it; the place is kept. Long press:
 * open Mirza for now (gestures become settings later).
 */
@SuppressLint("ClickableViewAccessibility")
class Bubble(private val svc: AccessibilityService) {
    private val wm = svc.getSystemService(WindowManager::class.java)
    private val size = svc.dp(56)
    private val circle = GradientDrawable().apply { shape = GradientDrawable.OVAL }
    private val view = ImageView(svc).apply {
        setImageResource(R.drawable.ic_mic)
        scaleType = ImageView.ScaleType.CENTER
        background = circle
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
        val screen = svc.resources.displayMetrics
        x = prefs.getInt("bubble_x", screen.widthPixels - size - svc.dp(8))
        y = prefs.getInt("bubble_y", screen.heightPixels / 3)
    }
    private val onState: (Session.State) -> Unit = { paint(it) }
    private var shown = false

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
            svc.startActivity(Intent(svc, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
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
                        Session.toggle()
                    }
                }
                MotionEvent.ACTION_CANCEL -> v.removeCallbacks(longPressAction)
            }
            true
        }
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
        Session.stateListeners.remove(onState)
        runCatching { wm.removeView(view) }
        shown = false
    }

    private fun paint(s: Session.State) {
        circle.setColor(
            when (s) {
                Session.State.IDLE -> 0xFF455A64.toInt()
                Session.State.STARTING -> 0xFFFFA000.toInt()
                Session.State.LISTENING -> 0xFFD32F2F.toInt()
                Session.State.FINISHING -> 0xFF7B1FA2.toInt()
            },
        )
    }
}
