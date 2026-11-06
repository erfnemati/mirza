# Android trial (roadmap step 2)

A small Kotlin app over the Rust core, built to answer the questions that decide the design of
the Android app (step 5) before anyone builds it: can a floating bubble start the microphone while
another app is in front, can an AccessibilityService type the result into that app with Persian
half-spaces intact, and what does a sideloaded APK run into. It is not the Android app: no bubble
size, logo or gestures, no AI revision, no tasks, a plain settings screen.

## What is in the repo

- `crates/mobile` (`mirza-mobile`): the Rust core for the phone apps, exposed through UniFFI. A
  `Dictation` object takes 16 kHz s16le audio from the app and hands settled text (Persian letters
  fixed) to a listener; it streams through `mirza-core`'s Soniox provider, so the proxy setting,
  error messages and the `Authorization` header are the desktop's. It builds and its tests run on
  the desktop too; the bindgen tool is behind the `bindgen` feature, so desktop CI never builds it.
  `examples/dictate.rs` feeds an audio file through `Dictation` the way the app does.
- `crates/core`: three cfg lines, so it compiles for Android (no cpal, no sound player there).
- `android/`: the Gradle project (Gradle 9.8.2 wrapper, AGP 9.5.1 with built-in Kotlin, JNA
  5.19.1, compileSdk/targetSdk 36, minSdk 29). `android/build-rust.sh` runs before every build:
  cargo-ndk builds `libmirza_mobile.so` for arm64-v8a and x86_64, and the Kotlin bindings are
  generated from an unstripped host build (the stripped release `.so` has no UniFFI metadata left).
- Application id `dev.erfnemati.mirza.trial`; the real app will be `dev.erfnemati.mirza`.

## Building the APK

Needs Rust with `aarch64-linux-android` and `x86_64-linux-android`, `cargo install cargo-ndk`,
JDK 17+, and the Android SDK with an NDK (r30 used here), with `ANDROID_HOME` set.

```sh
cd android
./gradlew assembleDebug          # app/build/outputs/apk/debug/app-debug.apk
./gradlew assembleDebug -Pmirza.abis=arm64-v8a -Pmirza.rustProfile=dev   # faster while working
```

Behind a proxy, pass it to Gradle: `-Dhttps.proxyHost=127.0.0.1 -Dhttps.proxyPort=10808`.

## The trial app

One screen: permissions, the Soniox key (empty: the microphone is tested without sending
anything), proxy, language hints, the experiment switches, and a log with a Copy button. The log
never shows recognized text unless "Show recognized text in the log" is on.

The bubble: tap starts and stops dictation (grey idle, amber starting, red listening, purple
finishing), drag moves it, long press types a test sentence with half-spaces into the focused
field.

Experiment switches:

| Switch | Choices |
|---|---|
| Bubble | 1: overlay window (`SYSTEM_ALERT_WINDOW`) held by a foreground service; 2: `TYPE_ACCESSIBILITY_OVERLAY` drawn by the typing service |
| Microphone on tap | A: the bubble's foreground service adds the `microphone` type on tap; B: a separate microphone service started on tap; C: the bubble's service has the `microphone` type all the time; D: no microphone service, just record |
| Typing | set text (spliced at the cursor) with paste as fallback; set text only; paste only |

Every tap writes one line with the modes, the Android version and the app in front, then whether
each service start was accepted or refused (with the exception), and after two seconds whether the
audio is real or all zeros and whether Android says it silenced the app (Android mutes the
microphone of apps it doesn't consider in use instead of failing). At the end it logs how many
pieces were typed and how many partial updates Soniox sent.

adb test hooks (`DebugReceiver`, protected by the `DUMP` permission, which only the shell has):
`mirza.TYPE`, `mirza.TAP`, `mirza.MODE`, `mirza.SHOW`, `mirza.HIDE`; see the file for the syntax.

## Results

Everything below was tested on the Android 16 emulator on the development server (API 36,
`google_apis` x86_64 image, software CPU), with the virtual microphone playing a recorded clip and
taps sent with `input tap` on the bubble. Nothing has run on a real phone yet; the checklist at the
end is for that. Erfan's phone is a Xiaomi with HyperOS 1.0 on Android 13, and its rules differ
from Android 16's: holding "draw over other apps" is enough there to start a foreground service
from the background (the visible-window condition came with Android 15), foreground service types
are only enforced from Android 14, and Xiaomi adds its own background limits. The phone test can
change the conclusions.

### Building

- `mirza-core` builds for Android with three `cfg` lines; nothing else in core changed.
- UniFFI 0.32 works as is: Kotlin calls `Dictation`, Rust calls the Kotlin listener back from its
  own threads.
- Debug APK: 8.4 MB with both ABIs. `libmirza_mobile.so` is 2.4 MB (arm64-v8a) and 2.6 MB
  (x86_64), aligned to 16 KB pages, which Google Play requires for apps targeting Android 15+.
- The bindings must come from an unstripped build; the stripped release library has no UniFFI
  metadata left.

### Starting the microphone from the bubble

"Works" means Android let the app record; "silenced" means Android muted it (it hands the app
zeros instead of failing); "refused" means `startForegroundService` threw. The emulator's own
microphone input is unreliable (below), so all-zero audio alone proves nothing there. The recorder
therefore asks Android directly (`AudioRecord.getActiveRecordingConfiguration().isClientSilenced()`)
and the log says "Android says it silenced this app" or "did not silence". The deciding cases were
run again with that check; the first runs relied on whether any real sound arrived.

| Accessibility service | Bubble, trigger | A: promote | B: separate | C: always | D: none |
|---|---|---|---|---|---|
| off | overlay, tapped, Contacts in front | works | works | works | silenced (Android says so) |
| off | overlay visible, adb broadcast instead of a tap | | works | | silenced |
| off | no bubble, home screen in front, adb broadcast | | refused | | silenced |
| on | overlay, tapped | works | | | works |
| on | accessibility overlay, tapped | works | works | works | works |
| on | no bubble, adb broadcast | | works | | works |
| on | accessibility overlay, keyboard up in Contacts | works | works | | works |

What this shows:

- Without the accessibility service, recording from another app needs a microphone foreground
  service (A, B or C). Android 15+ lets an app with "draw over other apps" start one from the
  background only while its overlay window is visible; the tap itself doesn't matter (the adb
  broadcast worked as well), and with the bubble hidden the start is refused with
  `ForegroundServiceStartNotAllowedException ... mAllowStartForeground false`.
- With the accessibility service on, Android binds it with
  `BIND_FOREGROUND_SERVICE_WHILE_AWAKE | BIND_INCLUDE_CAPABILITIES` (seen in
  `dumpsys activity services`), so the app's process counts as a foreground process with the
  microphone capability while the screen is on. That is why D (no service at all) records, and why
  B can start its microphone service even with no bubble on screen.
- The emulator's microphone often gives a second of sound and then zeros, sometimes only zeros,
  even to the app itself in front, while the host side carries the clip and Android says the app
  is not silenced. That explains two odd silent runs in the first batch (A with the keyboard up,
  and one D run with accessibility on); the keyboard-up A case, run again with the check, was
  silent with "Android says it did not silence this app".
- The accessibility overlay (bubble 2) needs no "draw over other apps" permission and no permanent
  notification, and taps on it work like taps on the normal overlay.

### Typing into another app

The accessibility service typed into the Contacts "First name" field:

- `می‌خواهم این test را با Mirza امتحان کنم؛ کتاب‌ها و API‌ها. ` (three half-spaces, Persian and
  English mixed) came out exactly, read back character by character, with "set text", with "set
  text" at the cursor in the middle of existing text, and with "paste".
- It still came out exactly when the field already held 839 characters (899 characters and 12
  half-spaces after).

Real apps (Telegram, WhatsApp, Chrome, Xiaomi's apps) are on the phone checklist; their fields
behave differently from a plain `EditText`.

### Soniox through the Rust core

- On the desktop, `cargo run -p mirza-mobile --example dictate -- clip.wav http://127.0.0.1:10808`
  feeds a clip through the same `Dictation` API in the same 50 ms chunks as the app: the full
  Persian and English text came back, with half-spaces (U+200C) and Persian letters, then
  "finished".
- From the emulator without a proxy, the connection timed out and the app showed the core's plain
  message ("couldn't connect to Soniox: timed out. If Soniox is blocked where you are, set a proxy
  ..."). The message points to the desktop settings window; the phone app needs its own wording.
- From the emulator through the server's proxy (`http://10.0.2.2:10808`), the session connected and
  ended cleanly, but no text came back (0 to 3 partial updates, nothing final). The reason is the
  emulator's microphone, not the app: with the debug switch that saves what the app recorded
  (`save_audio` in the prefs, set with adb), the file held about one second of sound and then only
  zeros, while the virtual microphone on the host carried the whole clip. Fed that saved file, the
  desktop got no text either, while the same clip with two thirds of it cut out (20 ms kept out of
  every 60 ms) still gave 53 partial updates and final text. Live recognition has to be tested on
  the phone.
- While the connection isn't up yet, `Dictation` queues 10 seconds of audio and then drops chunks.
  The phone app should keep the audio until the connection is up (or stop with a message) instead.

### Sideloading and "restricted settings"

Installed the way a user would, from the Files app (`packageSource=3`, local file):

1. The installer says "For your security, your phone currently isn't allowed to install unknown
   apps from this source", with a Settings button to "Allow from this source".
2. In Accessibility, the service is listed under "Downloaded apps" as "Controlled by Restricted
   Setting", greyed out. Tapping it shows "App was denied access ... It's possible the app won't
   work properly without this restricted permission. Learn how to allow access" with only Close.
3. App info, three-dot menu, "Allow restricted settings" (it is there only after step 2). On the
   emulator it needed no confirmation; it has no screen lock, so a phone with one may ask for it.
4. After that the service turns on with the normal "Allow Mirza trial typing to have full control
   of your device?" dialog.

Android 13 shows an older dialog ("Restricted setting"), and HyperOS may add its own warnings; the
checklist asks for the exact wording. An app installed from Google Play is not restricted. adb
installs are not restricted either, so tests that use `adb install` never show this.

## What it means for the Android app (step 5)

- Draw the bubble from the accessibility service (`TYPE_ACCESSIBILITY_OVERLAY`). Besides the
  microphone permission, that one setting then covers the bubble, typing into other apps and
  starting the microphone; no "draw over other apps", no permanent notification.
- On tap, still start a short microphone foreground service (B) for the length of the dictation.
  It worked in every accessibility case here, gives the user the microphone notification, and
  doesn't depend on the "while awake" binding, which ends when the screen turns off and may differ
  on Xiaomi. D is not enough on its own.
- Without the accessibility service, offer the overlay bubble with a foreground service (A or B) as
  a fallback; the text can then only go to the clipboard.
- The setup screen has to walk the user through restricted settings for a sideloaded APK; it is
  the step most people will get stuck on.
- Keep audio while connecting instead of dropping it after 10 seconds, and give phone-specific
  error wording.

## The emulator on this server

No KVM here, so the emulator runs on software CPU emulation, and that runs every virtual CPU on one
host thread; more cores only add load, and the multi-threaded mode never finished booting. What
made it usable: 2 cores and 3 GB, Google apps disabled, Bluetooth off (it crashed in a loop),
animations off, a smaller screen (`wm size 540x1200`, `wm density 210`), and two system settings
that stop `system_server` from restarting itself (`system_server_watchdog_timeout_ms 900000`, and
`device_config put connectivity always_ratelimit_networkstack_crash true` with syncing disabled).
A cold boot then takes about 7 minutes, and one test run about 2.5 minutes. Its microphone input
is unreliable (a second of sound, then zeros), so it shows whether Android lets the app record, but
not whether recognition works.

## Phone checklist

Install it the way a user would, from the phone's browser or file manager, not with adb (adb
installs never hit the restricted-settings block). The log in the app never contains the key, and
contains recognized text only if you turn that switch on.

1. Install: open the APK link in the phone's browser, download it, open it. Allow installs from
   that app when asked, and let HyperOS run its security scan if it wants to. Note every warning.
2. Open Mirza trial. Tap 1 (microphone and notifications) and allow both.
3. Tap 2 (drawing over other apps), turn it on for Mirza trial, come back.
4. Tap "Xiaomi: other permissions" and turn on "Display pop-up windows while running in the
   background" (and "Display pop-up window" if it is there). Note what the screen offers.
5. Tap 3 (Accessibility) and find "Mirza trial typing" (on HyperOS it may be under "Downloaded
   apps" or "Installed services"). Try to turn it on. Note whether it is greyed out or shows a
   "Restricted setting" dialog, and its exact words. Then go back to the app, tap "App info", open
   the three-dot menu at the top right, choose "Allow restricted settings", confirm, go back to
   Accessibility and turn the service on. Note whether HyperOS adds a countdown or another warning.
6. Paste the Soniox key into the key field (leave it empty to test only the microphone). If Soniox
   is blocked on your network, either keep your VPN on and leave Proxy empty, or put your VPN
   app's local proxy in the Proxy field (for example `socks5://127.0.0.1:10808`).
7. Tap "Show bubble". Open a chat in Telegram and tap the bubble: it turns amber, then red. Say a
   sentence in Persian with an English word and a half-space, for example «می‌خواهم این feature را
   فردا test کنم و کتاب‌ها را بیاورم.» Tap again. Check the message box: are the half-spaces there
   (no extra space, no joined letters), and is the English word in the right place?
8. Long-press the bubble in a few apps: it types a fixed test sentence with half-spaces. Try
   Telegram, WhatsApp or Bale/Eitaa, Chrome (the address bar and a text field on a web page),
   Google Keep or Xiaomi Notes, Gmail, and a field that already has a long text in it.
9. Repeat step 7 with each "Microphone on tap" choice (A, B, C, D) and both "Bubble" choices.
   Changing a choice hides the bubble; tap "Show bubble" again. Each tap writes its result to the
   log: "audio OK" or "SILENT", whether Android says it silenced the app, "refused" with the
   reason, how many pieces were typed, and how many partial updates came back.
10. With bubble 2 and choice B (what the real app would use): lock the screen and unlock it again,
    turn battery saver on, and swipe Mirza trial away from the recent apps. Does the bubble
    survive, and does dictation still work?
11. Watch for the green microphone dot at the top of the screen while it listens, and whether it
    goes away after.
12. Back in the app, tap "Copy log" and send the log.
