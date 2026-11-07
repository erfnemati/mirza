# Android app (roadmap step 5): plan

Status: plan of 2026-11-06, checked by a second reviewer and revised. Built on the trial's results
([android-trial.md](android-trial.md)) and Erfan's decisions. Step 4 (voice tasks in the Rust
core, [voice-tasks.md](voice-tasks.md) on its branch) is being built at the same time; this plan
only uses its results.

## What the app does

- A floating bubble over every app. Gestures on it start dictation, dictation revised by AI, or a
  voice task. Text goes into the field you are in, at the cursor, with Persian half-spaces intact.
- Voice tasks: you say a task, AI turns it into a short title, an optional one-line note, a list
  (the project you named) and a date or time. Mirza saves it to Google Tasks right away; anything
  with a clock time also becomes an event in your main Google Calendar, and Google's own reminder
  rings. Mirza has no alarm or reminder of its own. Then a notification with Undo and Edit. No
  confirm step, except on the lock screen.
- On the lock screen the bubble stays, but Mirza never types or acts there. A spoken task waits in
  a queue on the phone (at most 10, the limit can be set lower) and runs only after you unlock and
  confirm it.
- Settings that cover the useful part of the desktop panel, laid out for a phone.

Not in step 5: Mirza's own alarms or reminders, Microsoft To Do (step 6), email or other assistant
jobs, a central server, Play Store, iPhone.

## How it is built

- **No server** (Erfan, 2026-11-06). The app talks to Soniox, the AI provider and Google itself,
  like the desktop app. Each user brings their own Soniox and AI keys.
- **Kotlin on top, the Rust core underneath.** Kotlin does what only Android can do: the bubble,
  the accessibility service, the microphone, notifications, the settings screens, Google sign-in.
  `mirza-core` does the rest: speech recognition, AI revision, the task parser, the Google Tasks
  and Calendar calls, the task queue, config checks and the update check. `crates/mobile` wraps it
  for Kotlin through UniFFI, as in the trial.
- **Every network call goes through the proxy setting** in the core (`net::connect_ws`,
  `usage::http_client`), as on the desktop. The one exception is Google sign-in, which goes through
  Google Play services and so uses the phone's own network (see "Voice tasks on Android").
- **The accessibility service is the center.** It draws the bubble (`TYPE_ACCESSIBILITY_OVERLAY`,
  which also shows on the lock screen, seen on the phone and the emulator), types, and sees
  whether the phone is locked. No "draw over other apps" permission and no permanent
  notification. Each dictation starts a short microphone foreground service (trial mode B). The
  overlay bubble and mic modes A, C and D are dropped.
- **Battery:** nothing runs between dictations except the accessibility service, which only gets
  focus and window events. No alarms, no polling, no wake locks, no permanent foreground service.
  The only background job is one network-constrained retry for unsaved tasks.
- **UI:** Jetpack Compose with Material 3 for the settings and the small task panel, so animations
  and dark mode come with the toolkit. The bubble stays a plain view in the overlay window with its
  own short animations (listening pulse, state colors, drag). The first commit of PR 1 proves that
  Compose builds with AGP 9's built-in Kotlin (compose compiler plugin matching that Kotlin,
  `buildFeatures.compose`).
- **One app id and one signing key from the start.** `dev.erfnemati.mirza`, every build signed with
  the same release key (see "Signing key" below), so updates install over each other and Google
  sign-in sees one certificate. The trial (`dev.erfnemati.mirza.trial`) stays until the real app covers its
  checklist; the setup screen tells you to turn the trial's typing service off, since two typing
  services would both draw bubbles.

## Settings and stored data

- **Config:** TOML in the app's private files, read and written by the core. `crates/mobile` gets a
  mobile config type that reuses the desktop's `Provider`, `Revision` and `AiProviders` types and
  the proxy and history settings, plus its own `[bubble]`, `[gestures]` and `[lock_screen]`
  sections. The desktop `Config` itself is not reused (shortcuts, microphone device, record
  command and start-on-login are desktop-only). Kotlin reads the whole config as JSON
  (`config_json()`) and changes one value at a time (`set_value(path, json)`), using the core's
  `config::update_file` and `config::set_json`; no UniFFI record per field. Keys stay strict
  (`deny_unknown_fields`); Android can't install an older version over a newer one without
  uninstalling, so a newer config is never read by an older app.
- **Keys:** in a separate file in private storage, like the desktop's mode-600 `keys.toml`, never
  logged, never in the config.
- **Recognized text** (history, the lock-screen queue, unsaved tasks): private storage only,
  `allowBackup="false"`, protected by Android's file encryption. History can be turned off.
- **Notifications that contain recognized text** ("Added: ...", the queued tasks) use private
  visibility, so the lock screen shows only "Mirza".

Screens, from the desktop panel:

| Screen | Contents |
|---|---|
| Setup | A checklist that shows what is missing and opens the right Android screen: microphone, notifications, accessibility (with the restricted-settings and Play Protect steps), Xiaomi Autostart and Battery saver, turning the trial's service off. The screen stays out of recents (Xiaomi force-stops an app swiped away there). Shown on its own whenever typing is off. |
| Speech | Service (Soniox first; OpenAI and ElevenLabs if the core's providers work on Android), key, model, languages, "only these languages", words to expect. |
| Bubble and gestures | Size, opacity, logo, start position; what tap, double tap and long press do; show on the lock screen or not. |
| AI revision | Provider, key, model, thinking, server address (local), how much to change, presets, your own rules, time limit, "try it". |
| Tasks | Google account (sign in or out), lists and the project names that pick them, the calendar for timed tasks, lock-screen queue limit. |
| Listening | Stop after seconds of silence, time limit. |
| Typing | Persian letters, keep a copy in the clipboard. |
| History and usage | Recent texts (on the phone only), Soniox balance. |
| General | Proxy, sounds, notifications, check for updates, about. |

## Gestures and triggers

Defaults, all changeable: tap = dictate, double tap = dictate and revise, long press = voice task.
The first tap starts recording at once; a second tap within the double-tap time only marks the
dictation for revision (the two differ only after the audio), so a plain tap never waits. While
listening, a tap stops.

With no text field in front (home screen, a list), a tap opens the small task panel instead,
since there is nowhere to type: it shows what you say and the task it becomes. On the lock screen
a tap records a task for the queue.

Later in step 5: the accessibility shortcut button (`FLAG_REQUEST_ACCESSIBILITY_BUTTON`), and a
long press or double press on volume down through the accessibility service's key events (screen
on only, normal volume keeps working). No Quick Settings tile. Making Mirza the default digital
assistant (long press on power) only if Erfan asks.

## Voice tasks on Android

Step 4 (PR erfnemati/mirza#10) builds the task code in the core (`crates/core/src/tasks/`): the
AI parser (one call through the same `[ai.*]` providers as revision, dates and Jalali in Rust),
the Google Tasks and Calendar calls, and the queue that keeps a transcript until Google confirms
the task. The phone uses it through `TaskBook` in `crates/mobile/src/tasks.rs`: `capture`,
`capture_held`, `confirm`, `process`, `process_due`, `retry_now`, `next_due_ms`, `waiting`,
`recent`, `lists`, `undo`, `edit`. Calls that reach Google or the AI block, so Kotlin runs them on
`Dispatchers.IO`; `capture`, `capture_held` and `confirm` are fast. The notification's title and
text come from the core (`TaskOutcome`); showing it and its buttons is Kotlin's job. Android adds:

- **Google sign-in through Google Play services** (`AuthorizationClient`, Erfan's pick of
  2026-11-06): one tap, no secret in the app, Google's way for Android. It asks for exactly the
  scopes the desktop uses: `https://www.googleapis.com/auth/tasks` and
  `https://www.googleapis.com/auth/calendar.events.owned`. Sign-in and every token refresh use
  the phone's own network, not Mirza's proxy, so the phone's VPN has to reach Google at that
  moment; the Tasks and Calendar calls themselves go through the proxy (`TaskConfig.proxy`).
  Needs Play services (global Xiaomi phones have it). There is no refresh token on the phone: the
  core calls Kotlin's `GoogleTokens.access_token(fresh)` before every Google call, retries
  included, and Kotlin answers with `authorize()` (silent once allowed). `fresh = true` means
  Google just refused the last token, so Kotlin clears it first and asks for a new one. The
  Google Cloud project "Mirza" is the desktop's; Erfan adds an Android OAuth client "Mirza
  Android" for `dev.erfnemati.mirza` with the SHA-1 under "Signing key" (step 7 of
  `docs/voice-tasks.md`). It is published "In production" without verification: up to 100 users
  and a warning screen once ("Testing" would end the sign-in every 7 days).
- **The capture flow:** gesture, microphone service, Soniox, then `capture` and `process`, then a
  notification "Added: <title> (<list>)" with Undo and Edit. Edit opens the task panel. Events
  use the calendar's default reminders; Mirza has no alarm or reminder of its own.
- **One queue.** The lock-screen queue is the core's own queue with a hold flag: while the phone
  is locked (`KeyguardManager.isKeyguardLocked`), Kotlin calls `capture_held(text)`, which stores
  only the transcript and sends nothing to the AI or Google. After unlock, one notification lists
  the held tasks; tapping it opens the panel, where `confirm(id)` then `process(id)` saves one,
  `edit` also releases it, and `undo` drops it. Times are read from the moment the task was
  said, so "tomorrow" stays the day after it was said even if it is confirmed later; the panel
  shows the resolved date. Recognition needs the internet, so with no connection nothing is
  recorded. `TaskConfig.held_limit` (1 to 10) caps the queue; when it is full `capture_held`
  refuses with a plain message (Erfan's decision of 2026-11-07).
- **One retry job:** a WorkManager job with a network constraint, first run after
  `next_due_ms`, that gets a token and calls `process_due` (held tasks are skipped).
- Sign-in, the Tasks screen and the task panel are Kotlin and can be built against a stub before
  step 4 is merged; only the real task calls wait for it.

## Order of work (one PR each into develop)

1. **Skeleton and dictation.** Compose check, the release signing key and signed builds, new app
   id, setup checklist, config and keys through the core, Speech and General screens, update check
   (opens the release page). Dictation from the trial, cleaned up: accessibility bubble only, mode
   B, the trial's typing fixes (hint text, no cursor, password fields), audio kept while
   connecting, notifications only while recording.
2. **The other screens and AI revision.** Bubble and gestures, AI revision ("dictate and revise"
   through the core), Listening, Typing, History and usage.
3. **Voice tasks.** Sign-in, the task panel and the Tasks screen first (against a stub), then the
   capture flow, Undo and Edit, the lock-screen queue and the retry job once step 4 is merged.
4. **Triggers and polish:** accessibility button, volume key, animations, and a battery check over
   a day of use on the phone.
5. **Release:** a signed APK from the Release workflow and an install guide (Play Protect,
   restricted settings, Xiaomi).

Each PR is tested on the KVM emulator with the virtual microphone (Persian clips through live
Soniox), then on Erfan's phone with a short checklist; the PR says which is which. Recording from
the lock screen comes with the queue in PR 3; until then a tap there only plays the error sound
(Android hides toasts behind the lock screen).

## Building and trying it

From `android/`: `./gradlew :app:assembleDebug` gives `app/build/outputs/apk/debug/app-debug.apk`
(add `-Pmirza.abis=x86_64` for the emulator only, which builds faster). Debug builds are signed
with the release key when it is found, so they install over a release build and the other way
around; they also carry `DebugReceiver`, adb hooks that only the shell can use:

    adb shell am broadcast -n dev.erfnemati.mirza/.DebugReceiver -a mirza.TAP      # tap the bubble
    ... -a mirza.SET --es path proxy --es json '"http://10.0.2.2:10808"'           # change a setting
    ... -a mirza.UPDATE                                                             # check for updates

On the emulator the proxy is `http://10.0.2.2:10808` (the server's proxy as the emulator sees it).
A key can go in without showing up in any command line, fed through stdin:
`adb exec-in "run-as dev.erfnemati.mirza sh -c 'mkdir -p files/mirza && cat > files/mirza/keys.toml'" < keys.toml`,
then `adb shell "run-as dev.erfnemati.mirza chmod 700 files/mirza"` and `chmod 600` on the file
the same way (`exec-in` ends the shell when the input ends, so nothing after `cat` runs there).
Remove the key after the test.

## Signing key

Made on the development server on 2026-11-06 (Erfan's OK): RSA 4096, PKCS12, alias `mirza`, owner
`CN=Mirza, O=Erfan Nemati`, valid until 2054. It lives in `.secrets/` at the root of the main
clone (`~/projects/mirza/.secrets/android-release.p12` and `android-release.password`, mode 600),
ignored by git and listed in `.backup-paths`, so Erfan's laptop keeps a copy. The Release workflow
gets it from the GitHub secrets `MIRZA_ANDROID_KEYSTORE` (the file in base64) and
`MIRZA_ANDROID_KEYSTORE_PASSWORD`; GitHub can't show them again, so the laptop copy is the backup.
If the key is lost, every user has to uninstall Mirza and install it again.

Certificate fingerprints (public; Google Cloud asks for the SHA-1 when the Android OAuth client is
created for `dev.erfnemati.mirza`):

- SHA-1: `B2:81:EB:62:D0:7D:2C:89:2C:FD:7A:9B:68:76:AE:CA:74:9E:48:ED`
- SHA-256: `FF:AC:89:7A:EA:E6:59:23:83:9D:79:B6:EE:F0:A1:DE:6D:FE:5E:8B:F6:A0:2A:D1:DA:2C:2C:61:D4:22:8C:42`

## Decisions

Answered by Erfan on 2026-11-06: no central server, the signing key as above, Google sign-in
through Play services. On 2026-11-07: a full lock-screen queue refuses new tasks with a plain
message. Still to ask: confirm held tasks one by one or all at once?

## Risks

- HyperOS may still stop the accessibility service ("clear all", the phone's own cleaner); the
  setup checklist and the "typing is off" screen are the answer, plus a keyboard (IME) later if
  needed.
- Play Protect blocks every sideloaded build with an accessibility service; the install guide has
  to cover it, and it may get stricter.
- Google may tighten unverified apps or the Tasks and Calendar scopes; then the project needs
  verification.
- Compose adds about 1 to 2 MB to the APK after R8, and a longer first build on the server.
