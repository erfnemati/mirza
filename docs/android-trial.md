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
  5.19.1, compileSdk/targetSdk 36, minSdk 29). Since step 5 the trial is the `:trial` module
  (`android/trial`), next to the real app (`:app`); both get the Rust core from `:core`
  (`android/core`). `android/build-rust.sh` runs before every build:
  cargo-ndk builds `libmirza_mobile.so` for arm64-v8a and x86_64, and the Kotlin bindings are
  generated from an unstripped host build (the stripped release `.so` has no UniFFI metadata left).
- Application id `dev.erfnemati.mirza.trial`; the real app will be `dev.erfnemati.mirza`.

## Building the APK

Needs Rust with `aarch64-linux-android` and `x86_64-linux-android`, `cargo install cargo-ndk`,
JDK 17+, and the Android SDK with an NDK (r30 used here), with `ANDROID_HOME` set.

```sh
cd android
./gradlew :trial:assembleDebug   # trial/build/outputs/apk/debug/trial-debug.apk
./gradlew :trial:assembleDebug -Pmirza.abis=arm64-v8a -Pmirza.rustProfile=dev   # faster while working
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

Since build trial3 the screen shows a button when typing isn't running ("Typing is off" or "on
but not running"), which opens the accessibility settings, and each start of
the app logs how its last process ended and whether typing is on in the settings. The screen stays
out of the recent apps once you leave it (see "First results on a phone").

Every tap writes one line with the modes, the Android version and the app in front, then whether
each service start was accepted or refused (with the exception), and after two seconds whether the
audio is real or all zeros and whether Android says it silenced the app (Android mutes the
microphone of apps it doesn't consider in use instead of failing). At the end it logs how many
pieces were typed and how many partial updates Soniox sent.

adb test hooks (`DebugReceiver`, protected by the `DUMP` permission, which only the shell has):
`mirza.TYPE`, `mirza.TAP`, `mirza.MODE`, `mirza.SHOW`, `mirza.HIDE`; see the file for the syntax.

## Results

Everything below was tested on the Android 16 emulator (API 36, `google_apis` x86_64 image,
software CPU), with the virtual microphone playing a recorded clip and taps sent with `input tap`
on the bubble, unless it says "on a phone"; the checklist at the end is for the phone. The test
phone is a Xiaomi with HyperOS 1.0 on Android 13, and its rules differ
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
microphone input was unreliable during the trial (below), so all-zero audio alone proved nothing. The recorder
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
  silent with "Android says it did not silence this app". The cause was the software CPU
  emulation: with KVM, the same emulator heard a 12-second test tone without a gap, twice (see
  "The emulator").
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
- From the emulator through a proxy on the host (`http://10.0.2.2:10808`), the session connected and
  ended cleanly, but no text came back (0 to 3 partial updates, nothing final). The reason is the
  emulator's microphone, not the app: with the debug switch that saves what the app recorded
  (`save_audio` in the prefs, set with adb), the file held about one second of sound and then only
  zeros, while the virtual microphone on the host carried the whole clip. Fed that saved file, the
  desktop got no text either, while the same clip with two thirds of it cut out (20 ms kept out of
  every 60 ms) still gave 53 partial updates and final text.
- With KVM, live recognition worked on the emulator. The same clip (Persian then
  English, made with espeak-ng) was played into the emulator's microphone, and dictation was started
  and stopped by tapping the accessibility bubble (mode B, keyboard up, Contacts "Company" field in
  front). The field ended up with the whole sentence, 110 characters with both half-spaces and the
  Persian letters intact: `سلام. من می‌خواهم این برنامه را امتحان کنم. کتاب‌ها روی میز هستند.
  Hello, this is a test of Murner on Android.` ("Murner" is espeak's "Mirza"). That took 31 partial
  updates and 4 typed pieces, with the first partial text 4.5 s after the tap.
- In that run, the app's check right after each "set text" sometimes still read the field's old
  text ("the field holds 5 chars after SET_TEXT, expected 43"), although the final text was
  right. Moving the cursor after the text was refused in that field while the keyboard was up.
  Step 5 should check the result after the field reports the change, not right after the call.
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

### First results on a phone

Xiaomi with HyperOS 1.0 on Android 13, 2026-11-06. This is a first run, not yet the whole checklist.

- Installing from the phone's browser was stopped by Google Play Protect: "App blocked to protect
  your device". Play Protect blocks apps from outside the Play Store that declare one of four
  sensitive permissions, and an accessibility service is one of them; SMS and notification access
  are the others. It installed once "Scan apps with Play Protect" was turned off in the Play Store
  for the install. Anyone who installs a sideloaded Mirza APK will hit this before restricted
  settings. adb installs are not blocked.
- In Accessibility the service was blocked with Android 13's wording: "Restricted setting: For
  your security, this setting is currently unavailable". "Allow restricted settings" in App info
  unlocked it.
- Dictation worked in Telegram (Saved Messages) and WhatsApp: tap, the bubble turns red, speak, tap
  again and it turns back. But in an empty field, the first piece came out with "Message" in front
  ("Message1,2,3"). Both apps report their hint, "Message", as the text of an empty field without
  marking it as a hint. An empty Android field reports no cursor (seen on the emulator as "cursor
  -1..-1"), so build trial2 (`0.1.5-trial2`) pastes when a field has text but no cursor, and logs
  the field's length, hint state and cursor before each piece. It still has to be checked on the
  phone.
- With the overlay bubble, Android shows its own "Mirza trial is displaying over other apps"
  notification ("If you don't want Mirza trial to use this feature, tap to open settings and turn
  it off"), and the app shows "Bubble on". On the emulator, the accessibility bubble with mode B
  showed neither while idle, only the microphone notification while recording.
- Trial2 fixed "Message" in Telegram and WhatsApp.
- Swiping Mirza trial away in the recent apps turned typing off: the service was off in
  Accessibility, and it had to be turned on again (and off and on once more) before the bubble came
  back. On the emulator, a force stop (`am force-stop`) does exactly that: Android removes a
  force-stopped app's accessibility service from the enabled list, by design. A plain kill of the
  process doesn't: Android restarted the service and the bubble came back by itself within 5
  seconds. So HyperOS force-stops an app swiped away in recents, as Xiaomi is known to do for apps
  not locked there. No app can stop that. Build trial3 keeps its screen out of recents once you
  leave it, so there is no card to swipe; logs how the last process ended ("FORCE STOPPED" is the
  case above); shows a button to the accessibility settings when typing is off; and on Xiaomi
  adds buttons for Autostart and Battery saver ("No restrictions"). Whether HyperOS's "clear all"
  still stops it is not known yet.
- Trial3 on the phone: after Home, Mirza trial leaves recents and the bubble keeps working.
  Swiping it away while its screen is still open stops it, bubble included, as expected.
- The accessibility bubble also shows on the lock screen. There it could only type into the PIN
  field, so trial3 never types into password fields (before, the paste fallback could).

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
  the step most people will get stuck on. Before that, the install guide has to cover Play
  Protect's block. A Mirza keyboard (IME) would avoid both, since a keyboard is not on Play
  Protect's list; it is untested and changes how dictation starts.
- Don't trust a field's text blindly: some apps report their hint as text. Use the cursor (no
  cursor means the field is empty or unknown) and paste when in doubt.
- Keep audio while connecting instead of dropping it after 10 seconds, and give phone-specific
  error wording.
- Closing Mirza from recents must not turn typing off: keep the settings screen out of recents,
  check at every start whether the service is on and offer a one-tap way back, and in the Xiaomi
  setup ask for Autostart and Battery saver "No restrictions". The way back can only be the
  Accessibility list: opening one service's own page there needs a system permission
  (`OPEN_ACCESSIBILITY_DETAILS_SETTINGS`). A keyboard (IME) may not have this
  problem; untested.
- On the lock screen the bubble stays, but Mirza never types or acts there. A spoken command (for
  now only "add a task") goes into a queue on the phone, at most 10, and the limit can be set
  lower but never higher. After unlocking, Mirza goes through the queue one by one and runs only
  the commands the user confirms. Recognition needs the internet, so with no connection nothing is
  queued.

## The emulator

On a development machine that is itself a VM, nested virtualization has to be on (in vSphere,
"Expose hardware assisted virtualization to the guest OS" under the VM's CPU settings) for the
emulator to run with KVM. Then a cold boot takes about 2 minutes, and its microphone hears the
virtual microphone without gaps: a 12-second tone came through as 240 of 240 chunks with sound, in
two runs. `scripts/dev/emulator.sh` uses KVM whenever `/dev/kvm` is there; the user needs to be in
the `kvm` group, and a shell that started before that runs the emulator through `sg kvm`.

The trial ran before that, on software CPU emulation, which runs every virtual CPU on one host
thread; more cores only add load, and the multi-threaded mode never finished booting. What made it
usable: 2 cores and 3 GB, Google apps disabled, Bluetooth off (it crashed in a loop), animations
off, a smaller screen (`wm size 540x1200`, `wm density 210`), and two system settings that stop
`system_server` from restarting itself (`system_server_watchdog_timeout_ms 900000`, and
`device_config put connectivity always_ratelimit_networkstack_crash true` with syncing disabled).
A cold boot then took about 7 minutes, one test run about 2.5 minutes, and its microphone gave a
second of sound and then zeros. Without KVM the script still falls back to 2 cores and 3 GB; the
other settings were made by hand with adb.

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
    turn battery saver on, and use "clear all" in the recent apps. Does the bubble survive, and does
    dictation still work? On Xiaomi, first turn on Autostart and set Battery saver to "No
    restrictions" with the buttons in the app. After a restart of the app, the log's "last exit"
    line says how it ended.
11. Watch for the green microphone dot at the top of the screen while it listens, and whether it
    goes away after.
12. Back in the app, tap "Copy log" and send the log.
