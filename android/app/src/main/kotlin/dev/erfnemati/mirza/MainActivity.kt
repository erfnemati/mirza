package dev.erfnemati.mirza

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * The settings app: setup checklist, status and settings. Kept out of recents (manifest): Xiaomi
 * force-stops an app swiped away there, which turns typing off.
 */
class MainActivity : ComponentActivity(), Actions {
    /** Bumped when the activity comes back, since permissions and Accessibility change outside the app. */
    private var resumes by mutableIntStateOf(0)
    private var screen by mutableStateOf(Screen.HOME)
    private var asked: String? = null

    private val askPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        val perm = asked
        if (!granted && perm != null && !shouldShowRequestPermissionRationale(perm)) {
            // Denied for good: only App info can change it now.
            toast("Allow it in App info, under Permissions.")
            appInfo()
        }
        resumes++
    }

    @OptIn(ExperimentalMaterial3Api::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        savedInstanceState?.getString("screen")?.let { screen = Screen.valueOf(it) }
        Updates.onOpen()
        setContent {
            MirzaTheme {
                resumes.let { } // read, so the checklist is checked again on resume
                val steps = Setup.steps(this)
                BackHandler(enabled = screen != Screen.HOME) { screen = Screen.HOME }
                Scaffold(
                    topBar = {
                        TopAppBar(
                            title = { Text(screen.title) },
                            navigationIcon = {
                                if (screen != Screen.HOME) {
                                    IconButton(onClick = { screen = Screen.HOME }) {
                                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                                    }
                                }
                            },
                        )
                    },
                ) { pad ->
                    AnimatedContent(
                        targetState = screen,
                        transitionSpec = { fadeIn() togetherWith fadeOut() },
                        label = "screen",
                        modifier = Modifier.padding(pad),
                    ) { shown ->
                        Column(
                            verticalArrangement = Arrangement.spacedBy(12.dp),
                            modifier = Modifier
                                .fillMaxSize()
                                .verticalScroll(rememberScrollState())
                                .padding(16.dp),
                        ) {
                            when (shown) {
                                Screen.HOME -> HomeScreen(steps, this@MainActivity)
                                Screen.SPEECH -> SpeechScreen(this@MainActivity)
                                Screen.GENERAL -> GeneralScreen()
                                Screen.ABOUT -> AboutScreen(this@MainActivity)
                            }
                        }
                    }
                }
            }
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString("screen", screen.name)
    }

    override fun onResume() {
        super.onResume()
        resumes++
    }

    override fun fix(step: Step) {
        when (step.fix) {
            Fix.MIC -> ask(Manifest.permission.RECORD_AUDIO)
            Fix.NOTIFICATIONS -> ask(Manifest.permission.POST_NOTIFICATIONS)
            Fix.TYPING, Fix.TRIAL -> start(Setup.accessibilityIntent(this))
            Fix.KEY -> go(Screen.SPEECH)
            Fix.AUTOSTART -> if (!start(Setup.autostartIntent())) appInfo()
            Fix.BATTERY -> appInfo()
        }
    }

    override fun tick(step: Step) {
        Setup.tick(step.fix)
        resumes++
    }

    override fun appInfo() {
        start(Setup.appInfoIntent(this))
    }

    override fun open(url: String) {
        if (!start(Intent(Intent.ACTION_VIEW, Uri.parse(url)))) toast("No browser to open $url")
    }

    override fun go(screen: Screen) {
        this.screen = screen
    }

    private fun ask(perm: String) {
        asked = perm
        askPermission.launch(perm)
    }

    private fun start(i: Intent): Boolean = try {
        startActivity(i)
        true
    } catch (e: ActivityNotFoundException) {
        AppLog.add("couldn't open ${i.action ?: i.component}: ${describe(e)}")
        false
    } catch (e: SecurityException) {
        AppLog.add("couldn't open ${i.action ?: i.component}: ${describe(e)}")
        false
    }
}
