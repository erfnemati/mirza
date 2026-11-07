package dev.erfnemati.mirza

import android.accounts.Account
import android.app.Activity
import android.content.Intent
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.IntentSenderRequest
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.ClearTokenRequest
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.auth.api.identity.RevokeAccessRequest
import com.google.android.gms.common.api.ApiException
import com.google.android.gms.common.api.CommonStatusCodes
import com.google.android.gms.common.api.Scope
import com.google.android.gms.tasks.Tasks
import dev.erfnemati.mirza.core.GoogleToken
import dev.erfnemati.mirza.core.GoogleTokens
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/**
 * Google sign-in for voice tasks, through Google Play services: no secret in the app, since Google
 * knows Mirza by its package name and signing certificate. There is no refresh token on the phone:
 * the core asks [Tokens] before every Google call, and Play services answers from its cache or
 * asks Google, over the phone's own network (not Mirza's proxy).
 */
object Google {
    private const val TASKS = "https://www.googleapis.com/auth/tasks"
    private const val CALENDAR = "https://www.googleapis.com/auth/calendar.events.owned"
    private val scopes = listOf(Scope(TASKS), Scope(CALENDAR))

    private val prefs get() = MirzaApp.app.getSharedPreferences("google", 0)

    /** Compose state: signed in on this phone. Google may still ask again (Tokens notices it). */
    var signedIn by mutableStateOf(false)
        private set

    /** Calendar was allowed too, so tasks with a time also become events. */
    var calendar by mutableStateOf(false)
        private set

    /** Why the last sign-in or token failed, for the Voice tasks screen; "" when fine. */
    var problem by mutableStateOf("")
        private set

    fun init() {
        signedIn = prefs.getBoolean("signed_in", false)
        calendar = prefs.getBoolean("calendar", false)
    }

    private fun client() = Identity.getAuthorizationClient(MirzaApp.app)

    /** The account chosen at sign-in, kept on the phone only (never logged). */
    private fun account(): Account? = prefs.getString("account", null)?.let { Account(it, "com.google") }

    private fun request(pickAccount: Boolean): AuthorizationRequest {
        val b = AuthorizationRequest.Builder().setRequestedScopes(scopes)
        if (pickAccount) b.setPrompt(AuthorizationRequest.Prompt.SELECT_ACCOUNT) else account()?.let { b.setAccount(it) }
        return b.build()
    }

    /**
     * "Sign in" on the Voice tasks screen: Google's own account and consent screens, through
     * `launcher`, which hands its result to [finish].
     */
    fun signIn(activity: Activity, launcher: ActivityResultLauncher<IntentSenderRequest>) {
        problem = ""
        Identity.getAuthorizationClient(activity).authorize(request(pickAccount = true))
            .addOnSuccessListener { r ->
                val pending = r.pendingIntent
                if (r.hasResolution() && pending != null) {
                    launcher.launch(IntentSenderRequest.Builder(pending.intentSender).build())
                } else {
                    done(r)
                }
            }
            .addOnFailureListener { failed(it) }
    }

    /** The result of Google's screens; `data` is null when they were closed. */
    fun finish(data: Intent?) {
        if (data == null) return
        try {
            done(client().getAuthorizationResultFromIntent(data))
        } catch (e: ApiException) {
            if (e.statusCode != CommonStatusCodes.CANCELED) failed(e)
        }
    }

    private fun done(r: AuthorizationResult) {
        val granted = r.grantedScopes
        if (r.accessToken == null || TASKS !in granted) {
            problem = "Google didn't allow Mirza into Google Tasks. Sign in again and tick the Tasks box."
            AppLog.add("google: sign-in without Tasks")
            return
        }
        val edit = prefs.edit().putBoolean("signed_in", true).putBoolean("calendar", CALENDAR in granted)
        r.toGoogleSignInAccount()?.account?.name?.let { edit.putString("account", it) }
        edit.apply()
        signedIn = true
        calendar = CALENDAR in granted
        problem = ""
        AppLog.add("google: signed in" + if (calendar) "" else " without Calendar")
        toast("Signed in to Google.")
        VoiceTasks.signedIn()
    }

    private fun failed(e: Exception) {
        problem = "Couldn't sign in: ${why(e)}"
        AppLog.add("google: sign-in failed: ${describe(e)}")
    }

    /** Signs out here and takes Mirza's access back at Google. */
    fun signOut() {
        val acct = account()
        Tokens.last?.let { t -> client().clearToken(ClearTokenRequest.builder().setToken(t).build()) }
        Tokens.last = null
        if (acct != null) {
            client().revokeAccess(RevokeAccessRequest.builder().setAccount(acct).setScopes(scopes).build())
                .addOnFailureListener { AppLog.add("google: revoke failed: ${describe(it)}") }
        }
        forget()
        AppLog.add("google: signed out")
    }

    private fun forget() {
        prefs.edit().clear().apply()
        signedIn = false
        calendar = false
    }

    /** Google wants the user to sign in again (the access was taken back, or a test sign-in ran out). */
    private fun lost() {
        toMain {
            if (!signedIn) return@toMain
            forget()
            problem = "Google asked to sign in again."
            AppLog.add("google: asked to sign in again")
        }
    }

    /** Plain words for a Play services failure. */
    private fun why(e: Throwable?): String = when {
        e is ApiException && offline(e) -> "Google couldn't be reached. Check the connection (and the phone's VPN)."
        e is ApiException && e.statusCode == CommonStatusCodes.DEVELOPER_ERROR ->
            "Google doesn't know this copy of Mirza (code 10). The Android client in the Google Cloud project must have this package name and signing certificate."
        e is ApiException -> "Google Play services said ${CommonStatusCodes.getStatusCodeString(e.statusCode)} (code ${e.statusCode})."
        else -> e?.message ?: "unknown error"
    }

    private fun offline(e: ApiException) = e.statusCode == CommonStatusCodes.NETWORK_ERROR ||
        e.statusCode == CommonStatusCodes.TIMEOUT || e.statusCode == CommonStatusCodes.INTERRUPTED

    /** The core's way to tokens. Called on the core's own threads, so it may block. */
    object Tokens : GoogleTokens {
        @Volatile var last: String? = null

        override fun accessToken(fresh: Boolean): GoogleToken {
            if (!signedIn) return GoogleToken.SignedOut("")
            return try {
                val c = client()
                if (fresh) {
                    // Google refused the last token: drop it from the cache, so a new one comes.
                    last?.let { runCatching { Tasks.await(c.clearToken(ClearTokenRequest.builder().setToken(it).build()), 20, TimeUnit.SECONDS) } }
                    last = null
                }
                val r = Tasks.await(c.authorize(request(pickAccount = false)), 30, TimeUnit.SECONDS)
                val t = r.accessToken
                if (r.hasResolution() || t == null) {
                    lost()
                    GoogleToken.SignedOut("Google asked to sign in again")
                } else {
                    last = t
                    GoogleToken.Token(t)
                }
            } catch (e: ExecutionException) {
                val cause = e.cause
                AppLog.add("google: no token: ${describe(cause ?: e)}")
                if (cause is ApiException && !offline(cause)) GoogleToken.SignedOut(why(cause)) else GoogleToken.Offline("no answer from Google")
            } catch (e: TimeoutException) {
                GoogleToken.Offline("no answer from Google in 30 s")
            } catch (e: Exception) {
                // Never let an exception reach the core: UniFFI would stop the app.
                AppLog.add("google: no token: ${describe(e)}")
                GoogleToken.Offline(e.message ?: "unknown error")
            }
        }
    }
}
