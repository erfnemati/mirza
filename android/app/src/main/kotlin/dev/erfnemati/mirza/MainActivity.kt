package dev.erfnemati.mirza

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.View
import android.view.WindowInsets
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast

/** Showing and hiding the bubble, the same way from the screen and from adb. */
object BubbleControl {
    fun show(ctx: Context): String? {
        if (Prefs.micMode == MicMode.ALWAYS &&
            ctx.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED
        ) {
            return "Mode C needs the microphone permission first."
        }
        when (Prefs.host) {
            BubbleHost.OVERLAY -> {
                if (!Settings.canDrawOverlays(ctx)) return "Allow drawing over other apps first (step 2)."
            }
            BubbleHost.ACCESSIBILITY -> {
                val svc = TypingService.instance ?: return "Turn on Mirza trial typing in Accessibility first (step 3)."
                svc.showBubble()
            }
        }
        Prefs.bubbleOn = true
        // The overlay host needs the service for its window; modes A and C need
        // it for the microphone whatever the host is.
        if (Prefs.host == BubbleHost.OVERLAY || Prefs.micMode == MicMode.PROMOTE || Prefs.micMode == MicMode.ALWAYS) {
            try {
                ctx.startForegroundService(Intent(ctx, BubbleService::class.java))
            } catch (e: Exception) {
                TrialLog.add("startForegroundService(BubbleService) REFUSED: ${describe(e)}")
                return "Android refused to start the bubble service; see the log."
            }
        }
        return null
    }

    fun hide(ctx: Context) {
        Prefs.bubbleOn = false
        Session.cancel("bubble hidden")
        ctx.stopService(Intent(ctx, BubbleService::class.java))
        TypingService.instance?.hideBubble()
        TrialLog.add("bubble hidden")
    }
}

/**
 * The trial's only screen: permissions, the Soniox key, the experiment
 * modes, and the log of what happened.
 */
class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var log: TextView
    private val refreshLog = Runnable { log.text = TrialLog.text() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(8), dp(16), dp(24))
        }
        val scroll = ScrollView(this).apply { addView(root) }
        if (Build.VERSION.SDK_INT >= 30) {
            // Android 15+ draws apps edge to edge; keep the content clear of the bars and the keyboard.
            scroll.setOnApplyWindowInsetsListener { v, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.ime())
                v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                WindowInsets.CONSUMED
            }
        }
        setContentView(scroll)

        status = TextView(this).apply { setTextIsSelectable(true) }
        root.addView(heading("Status"))
        root.addView(status)

        root.addView(heading("Set up"))
        root.addView(button("1. Allow microphone and notifications") { askPermissions() })
        root.addView(button("2. Allow drawing over other apps") {
            open(Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:$packageName")))
        })
        root.addView(button("3. Turn on \"Mirza trial typing\" in Accessibility") {
            open(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
        })
        root.addView(button("App info (⋮ > Allow restricted settings)") {
            open(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
        })
        if (Build.MANUFACTURER.equals("Xiaomi", ignoreCase = true)) {
            root.addView(button("Xiaomi: other permissions (pop-up windows in background)") {
                open(
                    Intent("miui.intent.action.APP_PERM_EDITOR")
                        .setClassName("com.miui.securitycenter", "com.miui.permcenter.permissions.PermissionsEditorActivity")
                        .putExtra("extra_pkgname", packageName),
                )
            })
        }

        root.addView(heading("Soniox"))
        root.addView(field("API key (empty: microphone test only)", Prefs.apiKey, secret = true) { Prefs.apiKey = it })
        root.addView(field("Proxy: empty for none, or http:// or socks5:// URL", Prefs.proxy) { Prefs.proxy = it })
        root.addView(field("Language hints", Prefs.languages) { Prefs.languages = it })
        root.addView(field("Model (empty: stt-rt-v6)", Prefs.model) { Prefs.model = it })

        root.addView(heading("Experiment"))
        root.addView(choice("Bubble", BubbleHost.entries.map { it.label }, Prefs.host.ordinal) {
            Prefs.host = BubbleHost.entries[it]
            modesChanged()
        })
        root.addView(choice("Microphone on tap", MicMode.entries.map { it.label }, Prefs.micMode.ordinal) {
            Prefs.micMode = MicMode.entries[it]
            modesChanged()
        })
        root.addView(choice("Typing", TypingMethod.entries.map { it.label }, Prefs.typing.ordinal) {
            Prefs.typing = TypingMethod.entries[it]
        })
        root.addView(CheckBox(this).apply {
            text = "Show recognized text in the log"
            isChecked = Prefs.logText
            setOnCheckedChangeListener { _, on -> Prefs.logText = on }
        })
        root.addView(button("Show bubble") {
            BubbleControl.show(this)?.let { toast(it) }
            refreshStatus()
        })
        root.addView(button("Hide bubble") {
            BubbleControl.hide(this)
            refreshStatus()
        })

        root.addView(heading("Log"))
        val row = LinearLayout(this)
        row.addView(button("Copy log") {
            getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Mirza trial log", TrialLog.text()))
            toast("Log copied")
        })
        row.addView(button("Clear log") { TrialLog.clear() })
        root.addView(row)
        log = TextView(this).apply {
            typeface = Typeface.MONOSPACE
            textSize = 11f
            setTextIsSelectable(true)
        }
        root.addView(log)
    }

    override fun onResume() {
        super.onResume()
        refreshStatus()
        TrialLog.listeners.add(refreshLog)
        refreshLog.run()
    }

    override fun onPause() {
        TrialLog.listeners.remove(refreshLog)
        super.onPause()
    }

    private fun modesChanged() {
        if (Prefs.bubbleOn) {
            BubbleControl.hide(this)
            toast("Modes changed: show the bubble again")
        }
        refreshStatus()
    }

    private fun refreshStatus() {
        fun yes(b: Boolean) = if (b) "yes" else "NO"
        val mic = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        val notes = Build.VERSION.SDK_INT < 33 ||
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        status.text = buildString {
            appendLine("${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            appendLine("Trial ${BuildConfig.VERSION_NAME}, core ${dev.erfnemati.mirza.core.coreVersion()}")
            appendLine("Microphone allowed: ${yes(mic)}")
            appendLine("Notifications allowed: ${yes(notes)}")
            appendLine("Drawing over other apps allowed: ${yes(Settings.canDrawOverlays(this@MainActivity))}")
            appendLine("Typing service on: ${yes(TypingService.instance != null)}")
            appendLine("Soniox key set: ${yes(Prefs.apiKey.isNotEmpty())}")
            append("Bubble: ${if (Prefs.bubbleOn) "shown" else "hidden"}")
        }
    }

    private fun askPermissions() {
        val perms = mutableListOf(Manifest.permission.RECORD_AUDIO)
        if (Build.VERSION.SDK_INT >= 33) perms += Manifest.permission.POST_NOTIFICATIONS
        requestPermissions(perms.toTypedArray(), 1)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        refreshStatus()
    }

    private fun open(intent: Intent) {
        try {
            startActivity(intent)
        } catch (e: Exception) {
            toast("This phone doesn't have that screen")
            TrialLog.add("couldn't open ${intent.action ?: intent.component}: ${describe(e)}")
        }
    }

    private fun toast(msg: String) = Toast.makeText(this, msg, Toast.LENGTH_LONG).show()

    private fun heading(text: String) = TextView(this).apply {
        this.text = text
        textSize = 18f
        setTypeface(typeface, Typeface.BOLD)
        setPadding(0, dp(16), 0, dp(4))
    }

    private fun button(text: String, onClick: () -> Unit) = Button(this).apply {
        this.text = text
        isAllCaps = false
        setOnClickListener { onClick() }
    }

    private fun field(hint: String, value: String, secret: Boolean = false, onChange: (String) -> Unit) =
        EditText(this).apply {
            this.hint = hint
            setText(value)
            isSingleLine = true
            inputType = if (secret) {
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            } else {
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            }
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}

                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}

                override fun afterTextChanged(s: Editable?) = onChange(s?.toString() ?: "")
            })
        }

    private fun choice(title: String, options: List<String>, selected: Int, onChange: (Int) -> Unit): View {
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        box.addView(TextView(this).apply {
            text = title
            setPadding(0, dp(8), 0, 0)
        })
        val group = RadioGroup(this)
        options.forEachIndexed { i, label ->
            group.addView(RadioButton(this).apply {
                id = View.generateViewId()
                text = label
                isChecked = i == selected
                setOnClickListener { onChange(i) }
            })
        }
        box.addView(group)
        return box
    }
}
