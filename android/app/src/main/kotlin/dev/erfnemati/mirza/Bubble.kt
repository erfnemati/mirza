package dev.erfnemati.mirza

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.PixelFormat
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.view.WindowManager
import android.widget.ImageView
import kotlin.math.hypot

/**
 * The floating button. Tap: start or stop dictation. Drag: move it. Long
 * press: type a test sentence into the focused field (trial only). Size, logo,
 * opacity and more gestures come with the real app.
 *
 * `type` is TYPE_APPLICATION_OVERLAY (needs SYSTEM_ALERT_WINDOW) or
 * TYPE_ACCESSIBILITY_OVERLAY (only from an accessibility service's context).
 */
@SuppressLint("ClickableViewAccessibility")
class Bubble(private val ctx: Context, type: Int) {
    private val wm = ctx.getSystemService(WindowManager::class.java)
    private val size = ctx.dp(56)
    private val circle = GradientDrawable().apply { shape = GradientDrawable.OVAL }
    private val view = ImageView(ctx).apply {
        setImageResource(R.drawable.ic_mic)
        scaleType = ImageView.ScaleType.CENTER
        background = circle
        contentDescription = "Mirza: start or stop dictation"
    }
    private val lp = WindowManager.LayoutParams(
        size,
        size,
        type,
        // Not focusable: the keyboard and the focused field stay in the other app.
        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
        PixelFormat.TRANSLUCENT,
    ).apply {
        gravity = Gravity.TOP or Gravity.START
        x = ctx.resources.displayMetrics.widthPixels - size - ctx.dp(8)
        y = ctx.resources.displayMetrics.heightPixels / 3
    }
    private val onState: (Session.State) -> Unit = { paint(it) }
    private var shown = false

    init {
        val slop = ViewConfiguration.get(ctx).scaledTouchSlop
        val longPress = ViewConfiguration.getLongPressTimeout().toLong()
        var downX = 0f
        var downY = 0f
        var startX = 0
        var startY = 0
        var dragging = false
        var longPressed = false
        val longPressAction = Runnable {
            longPressed = true
            TrialLog.add("long press: typing the test sentence")
            val svc = TypingService.instance
            TrialLog.add(svc?.type(TEST_SENTENCE) ?: "NOT TYPED: the accessibility service is off")
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
                    if (!dragging && !longPressed) Session.toggle(ctx, "bubble")
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
            TrialLog.add("ERROR the bubble couldn't be shown: ${describe(e)}")
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
                Session.State.STOPPING -> 0xFF7B1FA2.toInt()
            },
        )
    }

    companion object {
        /** Persian with half-spaces (U+200C) and English in one sentence. */
        const val TEST_SENTENCE = "می‌خواهم این test را با Mirza امتحان کنم؛ کتاب‌ها و API‌ها. "
    }
}
